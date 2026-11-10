//! Runs Ghosts' real map scripts (mp_frag + the common scripts) from the staged zones, when they
//! are there: `IW6_ZONE_DIR`, then `CODLEGACY_GHOSTS_DIR/zone`, then
//! `/mnt/user-data/uploads/ghosts_game_files/zone`. Passes (skips) when none exists.
//! Set `GSC_CENSUS_OUT` to a path to get the census written there.
#[path = "../../gsc_iw6/examples/util/zone_scripts.rs"]
mod zone_scripts;

use std::collections::BTreeMap;
use std::path::PathBuf;

use gsc_iw6_vm::{
    file_name, ArrRef, CallCtx, EntityId, Host, HostReturn, Name, ScriptSet, Value, Vm,
};

fn zone_dir() -> Option<PathBuf> {
    let mut c: Vec<PathBuf> = Vec::new();
    if let Some(d) = std::env::var_os("IW6_ZONE_DIR") {
        c.push(d.into());
    }
    if let Some(d) = codlegacy_env::var_os("CODLEGACY_GHOSTS_DIR") {
        c.push(PathBuf::from(d).join("zone"));
    }
    c.push("/mnt/user-data/uploads/ghosts_game_files/zone".into());
    c.into_iter().find(|p| p.join("mp_frag.ff").is_file())
}

fn load_set(dir: &std::path::Path) -> ScriptSet {
    let mut set = ScriptSet::new();
    // Base zones first; a patch zone's script replaces the base one.
    for z in ["common", "common_mp", "common_core_mp", "patch_common", "patch_common_mp", "patch_common_core_mp", "mp_frag", "patch_mp_frag"] {
        let path = dir.join(format!("{z}.ff"));
        if !path.is_file() {
            continue;
        }
        let zone = zone_scripts::load(path.to_str().expect("utf-8")).expect("zone opens");
        for s in &zone.scripts {
            set.add_bytes(file_name(&s.name), &s.stream, &s.code)
                .unwrap_or_else(|e| panic!("{z}/{}: {e}", s.name));
        }
    }
    set
}

/// Stubs the builtins `main` needs so far and counts everything.
#[derive(Default)]
struct Recorder {
    next_ent: u32,
    functions: BTreeMap<u16, u64>,
    methods: BTreeMap<u16, u64>,
    set_dvars: BTreeMap<String, String>,
    ents: BTreeMap<String, Value>,
    fields: BTreeMap<(u32, u16), Value>,
    unknown_fn: BTreeMap<u16, u64>,
}

impl Recorder {
    fn stub_ent(&mut self, key: String) -> Value {
        if let Some(v) = self.ents.get(&key) {
            return v.clone();
        }
        self.next_ent += 1;
        let v = Value::Entity(EntityId(self.next_ent));
        self.ents.insert(key, v.clone());
        v
    }
}

impl Host for Recorder {
    fn get_entity_field(&mut self, ent: EntityId, field: &Name) -> Option<Value> {
        // Engine-backed fields a map entity carries (INFERRED by use): 430 = origin, 47 = angles, 97 = classname, 542 = spawnflags, 640 = target, 392 = animation, 743 = destructible type.
        let Name::Id(id @ (430 | 47 | 97 | 542 | 640 | 392 | 743)) = field else { return None };
        let default = match id {
            392 => Value::str("mp_frag_crane_anim"),
            743 => Value::str("toy_transformer_small01"),
            97 => Value::str("script_model"),
            640 => Value::str("stub_target"),
            542 => Value::Int(0),
            _ => Value::Vector([0.0; 3]),
        };
        Some(self.fields.get(&(ent.0, *id)).cloned().unwrap_or(default))
    }
    fn set_entity_field(&mut self, ent: EntityId, field: &Name, value: &Value) -> bool {
        let Name::Id(id @ (430 | 47 | 97 | 542 | 640 | 392 | 743)) = field else { return false };
        self.fields.insert((ent.0, *id), value.clone());
        true
    }

    fn call_function(&mut self, id: u16, _ctx: &mut CallCtx<'_>, args: &[Value]) -> Result<HostReturn, String> {
        *self.functions.entry(id).or_default() += 1;
        Ok(match id {
            // setdvar (INFERRED)
            0x32 => {
                if let (Some(Value::Str(n)), Some(v)) = (args.first(), args.get(1)) {
                    self.set_dvars.insert(n.to_string(), v.to_string());
                }
                HostReturn::None
            }
            // bullettrace (INFERRED): hits nothing, at the end point
            0x66 => {
                let a = ArrRef::new();
                a.set(gsc_iw6_vm::Key::Str("position".into()), args.get(2).cloned().unwrap_or(Value::Vector([0.0; 3])));
                a.set(gsc_iw6_vm::Key::Str("fraction".into()), Value::Float(1.0));
                HostReturn::Value(Value::Array(a))
            }
            // sortbydistance (INFERRED): (array, origin) -> the array
            0x1a7 => HostReturn::Value(args.first().cloned().unwrap_or(Value::Undefined)),
            // vectortoangles (INFERRED)
            0x107 => HostReturn::Value(Value::Vector([0.0; 3])),
            // getdvarint (INFERRED)
            0x38 => HostReturn::Value(Value::Int(0)),
            // getdvar (INFERRED)
            0x37 => {
                let name = args.first().and_then(Value::as_str).unwrap_or("").to_ascii_lowercase();
                let v = match name.as_str() {
                    "mapname" => "mp_frag".to_owned(),
                    _ => self.set_dvars.get(&name).cloned().unwrap_or_default(),
                };
                HostReturn::Value(Value::str(&v))
            }
            // spawn (INFERRED)
            0x63 => {
                self.next_ent += 1;
                HostReturn::Value(Value::Entity(EntityId(self.next_ent)))
            }
            // getent (INFERRED): one stub entity per query
            0x188 => {
                let key = format!("{}|{}", args.first().map_or(String::new(), |v| v.to_string()), args.get(1).map_or(String::new(), |v| v.to_string()));
                HostReturn::Value(self.stub_ent(key))
            }
            // getentarray (INFERRED): two stub entities per query
            0x189 => {
                let key = format!("{}|{}", args.first().map_or(String::new(), |v| v.to_string()), args.get(1).map_or(String::new(), |v| v.to_string()));
                let a = self.stub_ent(format!("{key}#0"));
                let b = self.stub_ent(format!("{key}#1"));
                HostReturn::Value(Value::array([a, b]))
            }
            // Everything else: counted and answered with 0 (so conditions on it work).
            _ => {
                *self.unknown_fn.entry(id).or_default() += 1;
                HostReturn::Value(Value::Int(0))
            }
        })
    }
    fn call_method(&mut self, id: u16, _ctx: &mut CallCtx<'_>, _this: &Value, _args: &[Value]) -> Result<HostReturn, String> {
        *self.methods.entry(id).or_default() += 1;
        Ok(match id {
            // getorigin-like (INFERRED)
            0x8092 => HostReturn::Value(Value::Vector([0.0; 3])),
            _ => HostReturn::Unimplemented,
        })
    }
}

fn main_ref(vm: &Vm<Recorder>) -> gsc_iw6_vm::FuncRef {
    let file = Name::Text("maps/mp/mp_frag".to_owned());
    vm.resolve(&file, &Name::Id(20445)).expect("mp_frag main")
}

#[test]
fn mp_frag_main_runs() {
    let Some(dir) = zone_dir() else {
        eprintln!("no Ghosts zones found; skipping");
        return;
    };
    let set = load_set(&dir);
    eprintln!("{} scripts; unresolved includes: {:?}", set.len(), set.unresolved_includes());
    let mut vm = Vm::new(set, Recorder::default());
    vm.enable_census();
    // What the engine fills in before the map script runs: `level.struct`-like arrays.
    // One script struct every stub entity points at (`target` = 640, struct `targetname` = 642).
    let mut structs = Vec::new();
    for name in ["stub_target", "ladder_down_loc", "ladder_up_loc"] {
        let st = vm.new_struct();
        st.set(Name::Id(642), Value::str(name));
        st.set(Name::Id(430), Value::Vector([0.0; 3]));
        st.set(Name::Id(47), Value::Vector([0.0; 3]));
        structs.push(Value::Object(st));
    }
    vm.level_obj().set(Name::Id(31988), Value::array(structs));
    // (INFERRED: bot bookkeeping the engine side leaves as an empty array.)
    vm.level_obj().set(Name::Id(5594), Value::Array(ArrRef::new()));
    // (INFERRED: `level.console`, read without a definedness check.)
    vm.level_obj().set(Name::Id(7879), Value::Int(0));
    // (INFERRED: `level.script`, the map name, read by 927::#14794.)
    vm.level_obj().set(Name::Id(502), Value::str("mp_frag"));
    let main = main_ref(&vm);
    vm.start_thread(main, Vec::new());
    let mut errors = vm.take_errors();
    // Sixty seconds of script time.
    for _ in 0..60 {
        errors.extend(vm.step(1000).errors);
    }
    for e in &errors {
        eprintln!("ERROR {e}\n   stack: {:?}", e.stack);
    }
    if let Ok(out) = std::env::var("GSC_CENSUS_OUT") {
        let mut text = vm.census.as_ref().expect("census").report(vm.scripts());
        text.push_str("\n== unimplemented builtins (is method, id): calls ==\n");
        for ((m, id), n) in vm.unimplemented_builtins() {
            text.push_str(&format!("{} {id:#06x}: {n}\n", if *m { "method" } else { "function" }));
        }
        text.push_str(&format!("\n== dvars set ==\n{:#?}\n", vm.host().set_dvars));
        text.push_str(&format!("\n== threads alive at the end: {} ==\n", vm.threads().len()));
        for t in vm.threads() {
            text.push_str(&format!("{t:?}\n"));
        }
        text.push_str("\n== script errors ==\n");
        for e in &errors {
            text.push_str(&format!("{e}\n   stack: {:?}\n", e.stack));
        }
        std::fs::write(out, text).expect("census written");
    }
    // The main thread (thread 1: mp_frag's `main` and everything it calls synchronously) must run
    // clean, and no error may come from the VM itself. The remaining script errors are threads
    // that read map-entity data the stub entities do not carry (fields of script_model / struct
    // entities, the map's own struct data); see the census file.
    const VM_FAULTS: [&str; 12] = [
        "operand stack underflow",
        "ran past the end",
        "unimplemented opcode",
        "script stack overflow",
        "instruction budget",
        "unresolved function",
        "call to a missing",
        "string constant missing",
        "call without a call marker",
        "function called with too many parameters",
        "bad ",
        "no variable selected",
    ];
    let main_errors: Vec<_> = errors.iter().filter(|e| e.thread == 1).collect();
    assert!(main_errors.is_empty(), "main thread failed: {main_errors:?}");
    let faults: Vec<_> = errors.iter().filter(|e| VM_FAULTS.iter().any(|f| e.message.starts_with(f))).collect();
    assert!(faults.is_empty(), "VM faults: {faults:?}");
    eprintln!("{} script errors from stub-data gaps (threads other than main)", errors.len());
    let c = vm.census.as_ref().expect("census");
    assert!(c.instructions > 100_000, "ran {} instructions", c.instructions);
    assert!(!c.waittills.is_empty() && !c.notifies.is_empty());
}
