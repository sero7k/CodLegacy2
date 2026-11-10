//! Shadows of Evil's scripts translated, linked by the Black Ops II VM and
//! their autoexec functions run headless with only the engine-free
//! builtins: what they call that is not bound yet, and any script error.

mod common;

use std::collections::BTreeMap;
use std::sync::Mutex;

use gsc_t6::program::Op;
use gsc_t6::{Program, Strings, Value, Vm};

static UNBOUND: Mutex<BTreeMap<String, usize>> = Mutex::new(BTreeMap::new());

fn unbound(vm: &mut Vm<()>, _: &mut (), id: u32, _: &Value, _: &[Value]) -> Value {
    let (name, method) = vm.program.builtins[id as usize];
    let key = format!("{}{}", if method { "self " } else { "" }, vm.str(name));
    *UNBOUND.lock().unwrap().entry(key).or_default() += 1;
    Value::Undefined
}

#[test]
fn shadows_of_evil_autoexecs_run() {
    let Some(objects) = common::zod_objects() else {
        eprintln!("no T7 zones; skipped");
        return;
    };
    let (translated, summary) = gsc_t7::translate(&objects).expect("translate");
    eprintln!("{} scripts, {} functions, {} builtins, missing {:?}", summary.scripts, summary.functions, summary.builtins.len(), summary.missing);
    let autoexecs = gsc_t7::autoexecs(&translated, &gsc_t7::include_lists(&objects));
    for (i, (s, f)) in autoexecs.iter().enumerate() {
        if s.contains("throttle_shared") || s.contains("ammo_shared") {
            eprintln!("autoexec {i}: {s} {f}");
        }
    }
    let mut strings = Strings::default();
    let program = Program::link(translated, &mut strings).expect("link");
    let bad: Vec<String> = program
        .functions
        .iter()
        .filter(|f| f.code.iter().any(|o| matches!(o, Op::Bad(_))))
        .map(|f| format!("{}::{}", program.scripts[f.script as usize].name, strings.get(f.name)))
        .collect();
    assert!(bad.is_empty(), "functions with operations the VM does not run: {bad:?}");
    let mut vm: Vm<()> = Vm::new(program, strings);
    gsc_t6::natives::bind_core(&mut vm);
    vm.hooks.unbound = Some(unbound);
    let mut host = ();
    let mut started = 0;
    for (script, name) in &autoexecs {
        let fi = vm.program.find(&vm.strings, script, name).expect("autoexec function");
        let level = Value::Object(vm.level);
        vm.spawn(&mut host, fi, level, Vec::new());
        started += 1;
    }
    for _ in 0..40 {
        vm.time_ms += 50;
        vm.run_frame(&mut host);
    }
    let classes: Vec<String> = vm.field_names(vm.classes).iter().map(|f| vm.str(*f).to_owned()).collect();
    eprintln!("classes: {classes:?}");
    let t = vm.strings.find("throttle").unwrap();
    let desc = vm.raw_field(vm.classes, t);
    eprintln!("classes.throttle = {}", vm.to_text(&desc));
    if let Value::Array(a) = &desc {
        let first = a.read().get(&gsc_t6::Key::Int(0)).cloned();
        eprintln!("[0] = {:?}", first.map(|v| vm.to_text(&v)));
    }
    let unbound = UNBOUND.lock().unwrap().clone();
    eprintln!("{started} autoexecs; {} threads left; unbound builtins called: {unbound:?}", vm.thread_count());
    let errors: Vec<&String> = vm.messages.iter().filter(|m| !m.contains("unbound")).collect();
    eprintln!("{} messages: {:#?}", errors.len(), &errors[..errors.len().min(30)]);
    assert!(started > 250, "{started} autoexecs");
}
