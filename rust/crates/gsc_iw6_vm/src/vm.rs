//! The VM: threads, scheduling, the interpreter loop.

use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
use std::fmt;
use std::sync::Arc;

use gsc_iw6::{Name, Operand};

use crate::census::Census;
use crate::host::{Action, CallCtx, Host, HostReturn};
use crate::ops;
use crate::scripts::{Prepared, ScriptSet};
use crate::value::{AnimRef, ArrRef, EntityId, FuncRef, Key, ObjKind, ObjRef, Value};

pub type ThreadId = u64;

/// One script tick is 50 ms (`wait` rounds seconds to 1/20 s).
pub const TICK_MS: u32 = 50;
/// Frames a call chain may hold (the engine allows 31 nested calls).
pub const MAX_DEPTH: usize = 32;

/// A script error: the thread that raised it is killed.
#[derive(Clone, Debug)]
pub struct ScriptError {
    pub thread: ThreadId,
    pub time_ms: u64,
    /// `file::function` of the failing frame.
    pub function: String,
    /// Code offset of the failing instruction.
    pub offset: usize,
    pub message: String,
    /// The call chain, innermost first.
    pub stack: Vec<String>,
}

impl fmt::Display for ScriptError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "[{} ms, thread {}] {}+{:#x}: {}", self.time_ms, self.thread, self.function, self.offset, self.message)
    }
}
impl std::error::Error for ScriptError {}

/// Builtin ids the VM answers itself (INFERRED ids; set a field to `None` to forward it to the
/// host instead).
#[derive(Clone, Copy, Debug)]
pub struct CoreIds {
    pub isdefined: Option<u16>,
    pub getfirstarraykey: Option<u16>,
    pub getnextarraykey: Option<u16>,
    /// `getarraykeys(array)`: an array of the keys, in iteration order.
    pub getarraykeys: Option<u16>,
    /// `spawnstruct()`: a new empty struct.
    pub spawnstruct: Option<u16>,
}

impl Default for CoreIds {
    fn default() -> CoreIds {
        CoreIds { isdefined: Some(0x2d), getfirstarraykey: Some(0x1c1), getnextarraykey: Some(0x1a6), spawnstruct: Some(0x18c), getarraykeys: Some(0x1c0) }
    }
}

/// Events for the optional trace callback.
#[derive(Clone, Debug)]
pub enum TraceEvent {
    Call { thread: ThreadId, depth: usize, target: String },
    Builtin { thread: ThreadId, id: u16, method: bool, argc: usize },
    Notify { target: String, name: String, args: usize },
    Waittill { thread: ThreadId, target: String, name: String },
    Wait { thread: ThreadId, ticks: u64 },
    ThreadStart { thread: ThreadId, target: String, child: bool },
    ThreadEnd { thread: ThreadId, killed: bool },
    Error(String),
}

/// What a suspended thread waits for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ThreadState {
    Ready,
    Sleeping { until_ms: u64 },
    WaitingNotify { target: String, name: String },
    FrameEnd,
}

/// A live (suspended) thread, for the host's queries.
#[derive(Clone, Debug)]
pub struct ThreadInfo {
    pub id: ThreadId,
    /// `file::function` the thread is in.
    pub function: String,
    pub frames: usize,
    pub self_desc: String,
    pub state: ThreadState,
}

/// What `step` reports.
#[derive(Clone, Debug, Default)]
pub struct StepReport {
    pub ticks: u32,
    pub errors: Vec<ScriptError>,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
enum ObjKey {
    Obj(u64),
    Ent(u32),
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct WKey {
    obj: ObjKey,
    name: String,
}

#[derive(Clone, Debug)]
enum WaitKind {
    Endon,
    Waittill { matches: Vec<Value> },
    /// A builtin's `WaitNotify`.
    Host,
}

#[derive(Clone, Debug)]
struct Waiter {
    thread: ThreadId,
    seq: u64,
    kind: WaitKind,
}

#[derive(Clone, Debug)]
enum VarRef {
    None,
    Local(usize),
    Game,
    Field(Value, Name),
    Elem(ArrRef, Key),
}

#[derive(Clone, Debug)]
enum Wake {
    None,
    Push(Value),
    Args(Vec<Value>),
}

#[derive(Clone, Debug)]
enum State {
    Ready,
    Sleeping(u64),
    Notify,
    FrameEnd,
}

struct Frame {
    func: FuncRef,
    /// Index of the next instruction in the script's instruction list.
    ip: usize,
    locals: Vec<Value>,
    /// Stack index of this frame's marker.
    base: usize,
    self_val: Value,
    uid: u64,
}

struct Thread {
    id: ThreadId,
    frames: Vec<Frame>,
    stack: Vec<Value>,
    obj_reg: Value,
    var_reg: VarRef,
    wake: Wake,
    state: State,
    /// Wait lists this thread is registered in.
    regs: Vec<WKey>,
    wait_desc: Option<(String, String)>,
}

enum Flow {
    Done(Value),
    Blocked,
    Killed,
}

struct VmErr(String);
impl From<String> for VmErr {
    fn from(s: String) -> VmErr {
        VmErr(s)
    }
}
impl From<&str> for VmErr {
    fn from(s: &str) -> VmErr {
        VmErr(s.to_owned())
    }
}
type VR<T> = Result<T, VmErr>;

/// The script virtual machine.
pub struct Vm<H: Host> {
    scripts: Arc<ScriptSet>,
    host: H,
    level: ObjRef,
    anim: ObjRef,
    game: Value,
    ent_vars: HashMap<u32, ObjRef>,
    threads: BTreeMap<ThreadId, Thread>,
    running: Vec<ThreadId>,
    kill_flags: HashSet<ThreadId>,
    waiters: HashMap<WKey, Vec<Waiter>>,
    children: HashMap<u64, Vec<ThreadId>>,
    ready: VecDeque<ThreadId>,
    frame_end: VecDeque<ThreadId>,
    timeline: BTreeMap<u64, Vec<ThreadId>>,
    tick: u64,
    acc_ms: u32,
    next_thread: ThreadId,
    next_seq: u64,
    next_uid: u64,
    nest: usize,
    errors: Vec<ScriptError>,
    ret_slot: Option<(ThreadId, Value)>,
    unimplemented: BTreeMap<(bool, u16), u64>,
    pub core_ids: CoreIds,
    /// Raise a script error for builtins the host does not implement (default: log and push
    /// `undefined`).
    pub strict: bool,
    /// Instruction budget for one run of a thread (until it blocks or ends).
    pub budget: u64,
    pub census: Option<Census>,
    /// Print every executed instruction with the stack and locals to stderr (debugging only).
    pub trace_ops: bool,
    trace: Option<Box<dyn FnMut(&TraceEvent) + Send + Sync>>,
}

fn obj_key(v: &Value) -> Option<ObjKey> {
    match v {
        Value::Object(o) => Some(ObjKey::Obj(o.id())),
        Value::Entity(e) => Some(ObjKey::Ent(e.0)),
        _ => None,
    }
}

fn ticks_for(v: &Value) -> VR<u64> {
    match v {
        Value::Float(f) => {
            if *f < 0.0 {
                return Err("negative wait".into());
            }
            let t = (f * 20.0 + 0.5).floor() as u64;
            if t == 0 && *f != 0.0 { Ok(1) } else if t > 0xff_fffe { Err("wait time too large".into()) } else { Ok(t) }
        }
        Value::Int(i) => {
            if *i < 0 {
                return Err("negative wait".into());
            }
            let t = (*i as u64) * 20;
            if t > 0xff_fffe { Err("wait time too large".into()) } else { Ok(t) }
        }
        other => Err(VmErr(format!("wait needs a number, not {}", other.type_name()))),
    }
}

impl<H: Host> Vm<H> {
    pub fn new(scripts: ScriptSet, host: H) -> Vm<H> {
        let level = ObjRef::new(1, ObjKind::Level);
        let anim = ObjRef::new(2, ObjKind::Anim);
        Vm {
            scripts: Arc::new(scripts),
            host,
            level,
            anim,
            game: Value::Array(ArrRef::new()),
            ent_vars: HashMap::new(),
            threads: BTreeMap::new(),
            running: Vec::new(),
            kill_flags: HashSet::new(),
            waiters: HashMap::new(),
            children: HashMap::new(),
            ready: VecDeque::new(),
            frame_end: VecDeque::new(),
            timeline: BTreeMap::new(),
            tick: 0,
            acc_ms: 0,
            next_thread: 1,
            next_seq: 1,
            next_uid: 1,
            nest: 0,
            errors: Vec::new(),
            ret_slot: None,
            unimplemented: BTreeMap::new(),
            core_ids: CoreIds::default(),
            strict: false,
            budget: 20_000_000,
            census: None,
            trace_ops: false,
            trace: None,
        }
    }

    pub fn host(&self) -> &H {
        &self.host
    }
    pub fn host_mut(&mut self) -> &mut H {
        &mut self.host
    }
    pub fn scripts(&self) -> &ScriptSet {
        &self.scripts
    }
    /// The `level` object.
    pub fn level(&self) -> Value {
        Value::Object(self.level.clone())
    }
    pub fn level_obj(&self) -> &ObjRef {
        &self.level
    }
    /// The `anim` object.
    pub fn anim_obj(&self) -> &ObjRef {
        &self.anim
    }
    /// The `game` variable (an array, empty at first).
    pub fn game(&self) -> &Value {
        &self.game
    }
    pub fn set_game(&mut self, v: Value) {
        self.game = v;
    }
    /// A new empty struct object.
    pub fn new_struct(&mut self) -> ObjRef {
        ObjRef::new_struct()
    }
    /// The script variables of an entity (those that are not engine fields), if it has any.
    pub fn entity_vars(&self, e: EntityId) -> Option<&ObjRef> {
        self.ent_vars.get(&e.0)
    }
    /// Script time in ms (a multiple of 50).
    pub fn time_ms(&self) -> u64 {
        self.tick * u64::from(TICK_MS)
    }
    pub fn tick(&self) -> u64 {
        self.tick
    }
    pub fn set_trace(&mut self, f: Option<Box<dyn FnMut(&TraceEvent) + Send + Sync>>) {
        self.trace = f;
    }
    pub fn enable_census(&mut self) {
        self.census.get_or_insert_with(Census::default);
    }
    /// Builtins the host reported as unimplemented: ((is method, id) -> calls).
    pub fn unimplemented_builtins(&self) -> &BTreeMap<(bool, u16), u64> {
        &self.unimplemented
    }
    /// Errors raised since the last call (also returned by `step`).
    pub fn take_errors(&mut self) -> Vec<ScriptError> {
        std::mem::take(&mut self.errors)
    }

    fn emit(&mut self, e: TraceEvent) {
        if let Some(t) = self.trace.as_mut() {
            t(&e);
        }
    }
    fn tracing(&self) -> bool {
        self.trace.is_some()
    }

    // ---- public thread API -------------------------------------------------------------

    /// Starts `file::function` on `level` and runs it until it first blocks or ends (like the
    /// engine, a new thread runs at once). Errors land in `take_errors`.
    pub fn start_thread(&mut self, func: FuncRef, args: Vec<Value>) -> ThreadId {
        let lv = self.level();
        self.start_thread_on(lv, func, args)
    }

    /// As `start_thread`, with `self` set to `this`.
    pub fn start_thread_on(&mut self, this: Value, func: FuncRef, args: Vec<Value>) -> ThreadId {
        let scripts = self.scripts.clone();
        self.spawn(&scripts, func, this, args, None, 0)
    }

    /// Runs `func` as a thread and returns its return value when it finishes without ever
    /// blocking (`None` when it blocked, was killed or raised an error).
    pub fn call(&mut self, this: Value, func: FuncRef, args: Vec<Value>) -> Option<Value> {
        let scripts = self.scripts.clone();
        let id = self.spawn(&scripts, func, this, args, None, 0);
        match self.ret_slot.take() {
            Some((t, v)) if t == id => Some(v),
            _ => None,
        }
    }

    /// Resolves `file::function` by name.
    pub fn resolve(&self, file: &Name, func: &Name) -> Option<FuncRef> {
        self.scripts.resolve(file, func)
    }

    /// `target notify(name, args)` from the host: endon-threads die now, waiting threads are
    /// queued to run in the current tick (before time advances), in registration order.
    pub fn notify(&mut self, target: &Value, name: &str, args: Vec<Value>) {
        self.do_notify(target, name, args);
    }

    /// Kills a thread (and its child threads).
    pub fn kill(&mut self, id: ThreadId) {
        self.kill_thread(id);
    }

    /// An entity left the game: threads whose `self` is the entity die, waiters on it are
    /// dropped, and its script variables are freed.
    pub fn entity_removed(&mut self, e: EntityId) {
        let doomed: Vec<ThreadId> = self
            .threads
            .values()
            .filter(|t| matches!(t.frames.first().map(|f| &f.self_val), Some(Value::Entity(x)) if *x == e))
            .map(|t| t.id)
            .collect();
        for id in doomed {
            self.kill_thread(id);
        }
        let key = ObjKey::Ent(e.0);
        self.waiters.retain(|k, _| k.obj != key);
        self.ent_vars.remove(&e.0);
    }

    /// Advances script time by `dt_ms` (50 ms ticks; the remainder carries over). Each tick runs
    /// every thread due, in the order they were queued, then the `waittillframeend` threads.
    pub fn step(&mut self, dt_ms: u32) -> StepReport {
        let mut rep = StepReport::default();
        self.acc_ms += dt_ms;
        while self.acc_ms >= TICK_MS {
            self.acc_ms -= TICK_MS;
            self.run_tick();
            rep.ticks += 1;
        }
        rep.errors = self.take_errors();
        rep
    }

    /// Processes everything queued for the current tick without advancing time (what
    /// `start_thread` and `notify` leave queued).
    pub fn flush(&mut self) -> Vec<ScriptError> {
        self.drain_ready();
        self.take_errors()
    }

    /// Suspended threads.
    pub fn threads(&self) -> Vec<ThreadInfo> {
        self.threads
            .values()
            .map(|t| {
                let f = t.frames.last().map(|f| self.scripts.describe(f.func)).unwrap_or_default();
                let state = match &t.state {
                    State::Ready => ThreadState::Ready,
                    State::Sleeping(tk) => ThreadState::Sleeping { until_ms: tk * u64::from(TICK_MS) },
                    State::Notify => {
                        let (a, b) = t.wait_desc.clone().unwrap_or_default();
                        ThreadState::WaitingNotify { target: a, name: b }
                    }
                    State::FrameEnd => ThreadState::FrameEnd,
                };
                ThreadInfo {
                    id: t.id,
                    function: f,
                    frames: t.frames.len(),
                    self_desc: t.frames.first().map(|f| describe_value(&f.self_val)).unwrap_or_default(),
                    state,
                }
            })
            .collect()
    }

    /// What threads wait for with `waittill`: (target, notify name) -> thread ids, in
    /// registration order. `endon` registrations are not included.
    pub fn waiting_on(&self) -> Vec<(String, String, Vec<ThreadId>)> {
        let mut out = Vec::new();
        for (k, list) in &self.waiters {
            let ids: Vec<ThreadId> = list
                .iter()
                .filter(|w| !matches!(w.kind, WaitKind::Endon))
                .map(|w| w.thread)
                .collect();
            if !ids.is_empty() {
                let target = match &k.obj {
                    ObjKey::Obj(1) => "level".to_owned(),
                    ObjKey::Obj(2) => "anim".to_owned(),
                    ObjKey::Obj(i) => format!("object#{i}"),
                    ObjKey::Ent(e) => format!("entity#{e}"),
                };
                out.push((target, k.name.clone(), ids));
            }
        }
        out.sort();
        out
    }

    /// Threads waiting (waittill) on `target` for `name`.
    pub fn waiters_of(&self, target: &Value, name: &str) -> Vec<ThreadId> {
        let Some(obj) = obj_key(target) else { return Vec::new() };
        self.waiters
            .get(&WKey { obj, name: name.to_ascii_lowercase() })
            .map(|l| l.iter().filter(|w| !matches!(w.kind, WaitKind::Endon)).map(|w| w.thread).collect())
            .unwrap_or_default()
    }
}

fn describe_value(v: &Value) -> String {
    match v {
        Value::Object(o) if o.id() == 1 => "level".to_owned(),
        Value::Object(o) if o.id() == 2 => "anim".to_owned(),
        other => other.to_string(),
    }
}

mod exec;
mod sched;
