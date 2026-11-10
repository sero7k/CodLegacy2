//! The boundary between the VM and the game: builtin functions and methods, entity fields.

use crate::value::{EntityId, FuncRef, Name, Value};

/// What a builtin call hands back to the VM.
#[derive(Clone, Debug)]
pub enum HostReturn {
    /// The builtin's return value.
    Value(Value),
    /// The builtin returns nothing (the VM pushes `undefined`, as the engine does).
    None,
    /// The calling thread sleeps for this many seconds (rounded to 50 ms ticks like `wait`),
    /// then the builtin returns `undefined`.
    Wait(f32),
    /// The calling thread blocks until `name` is notified on `target`; the builtin then returns
    /// `undefined` (the notify's arguments are dropped).
    WaitNotify { target: Value, name: String },
    /// The host does not implement this builtin. The VM logs it once per id and pushes
    /// `undefined` (or raises a script error in strict mode).
    Unimplemented,
}

/// An effect a builtin asks for, applied by the VM after the call returns.
#[derive(Clone, Debug)]
pub enum Action {
    Notify { target: Value, name: String, args: Vec<Value> },
    StartThread { func: FuncRef, this: Value, args: Vec<Value> },
}

/// What the VM tells a builtin about the caller, and where the builtin queues effects.
pub struct CallCtx<'a> {
    /// The calling thread's id.
    pub thread: u64,
    /// `self` of the calling function.
    pub self_val: &'a Value,
    /// `level`.
    pub level: &'a Value,
    /// Script time in milliseconds (50 ms ticks).
    pub time_ms: u64,
    pub(crate) actions: Vec<Action>,
}

impl<'a> CallCtx<'a> {
    /// A context outside a running thread (a host probing its own builtins, tests).
    pub fn detached(self_val: &'a Value, level: &'a Value, time_ms: u64) -> CallCtx<'a> {
        CallCtx { thread: 0, self_val, level, time_ms, actions: Vec::new() }
    }
    /// What the builtin queued (for tests and probes).
    pub fn queued(&self) -> &[Action] {
        &self.actions
    }
}

impl CallCtx<'_> {
    /// Queues `target notify(name, args)`; applied right after the builtin returns.
    pub fn notify(&mut self, target: Value, name: &str, args: Vec<Value>) {
        self.actions.push(Action::Notify { target, name: name.to_owned(), args });
    }
    /// Queues `this thread func(args)`; applied right after the builtin returns.
    pub fn start_thread(&mut self, func: FuncRef, this: Value, args: Vec<Value>) {
        self.actions.push(Action::StartThread { func, this, args });
    }
}

/// The game side. Every method has a do-nothing default, so a host implements what it has.
pub trait Host {
    /// A builtin function call (ids below 0x8000). `args` are in source order (first first).
    fn call_function(
        &mut self,
        _id: u16,
        _ctx: &mut CallCtx<'_>,
        _args: &[Value],
    ) -> Result<HostReturn, String> {
        Ok(HostReturn::Unimplemented)
    }
    /// A builtin method call (`this method(args)`; ids from 0x8000, as scripts encode them).
    fn call_method(
        &mut self,
        _id: u16,
        _ctx: &mut CallCtx<'_>,
        _this: &Value,
        _args: &[Value],
    ) -> Result<HostReturn, String> {
        Ok(HostReturn::Unimplemented)
    }
    /// Reads an engine-backed entity field. `None` means "not an engine field": the VM then
    /// reads the entity's script variable of that name (undefined when never set).
    fn get_entity_field(&mut self, _ent: EntityId, _field: &Name) -> Option<Value> {
        None
    }
    /// Writes an engine-backed entity field; `false` means "not an engine field": the VM stores
    /// a script variable instead.
    fn set_entity_field(&mut self, _ent: EntityId, _field: &Name, _value: &Value) -> bool {
        false
    }
    /// A script sent `target notify(name, args)` (called before waiters are woken).
    fn on_notify(&mut self, _target: &Value, _name: &str, _args: &[Value]) {}
}

/// A host that implements nothing.
#[derive(Default, Debug, Clone, Copy)]
pub struct NullHost;
impl Host for NullHost {}
