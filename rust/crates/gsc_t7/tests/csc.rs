//! Shadows of Evil's client scripts (`.csc`) translated, linked by the
//! Black Ops II VM and their autoexec functions run headless with only the
//! engine-free builtins: how much of the client side links and runs, and
//! what it calls that is not bound yet (`docs/T7_ZOMBIES_PLAN.md`,
//! "Client scripts"). Reads the staged zones (`T7_ZONES`).

mod common;

use std::collections::BTreeMap;
use std::sync::Mutex;

use gsc_t6::program::Op;
use gsc_t6::{Program, Strings, Value, Vm};
use gsc_t7::T7Object;

static UNBOUND: Mutex<BTreeMap<String, usize>> = Mutex::new(BTreeMap::new());

fn unbound(vm: &mut Vm<()>, _: &mut (), id: u32, _: &Value, args: &[Value]) -> Value {
    let (name, method) = vm.program.builtins[id as usize];
    let key = format!("{}{}", if method { "self " } else { "" }, vm.str(name));
    if UNBOUND.lock().unwrap().get(&key).is_none_or(|n| *n < 2) {
        let a: Vec<String> = args.iter().map(|v| vm.to_text(v)).collect();
        eprintln!("first calls of {key}: {a:?}");
    }
    *UNBOUND.lock().unwrap().entry(key).or_default() += 1;
    Value::Undefined
}

#[test]
fn shadows_of_evil_client_scripts_link_and_run() {
    let Some(dir) = common::zone_dir() else {
        eprintln!("no T7 zones; skipped");
        return;
    };
    let map = std::env::var("T7_MAP").unwrap_or_else(|_| "zm_zod".to_owned());
    let mut objects = Vec::new();
    for zone in ["core_patch".to_owned(), "zm_patch".to_owned(), format!("{map}_patch")] {
        let z = fastfile_t7::Zone::open(&dir.join(format!("{zone}.ff"))).expect("zone");
        for bytes in common::objects(&z.image) {
            if let Ok(o) = T7Object::parse(bytes) {
                objects.push(o);
            }
        }
    }
    let csc = objects.iter().filter(|o| o.name.to_ascii_lowercase().ends_with(".csc")).count();
    let (translated, summary) = gsc_t7::translate_ext(&objects, ".csc").expect("translate");
    eprintln!("{csc} client scripts, {} translated, {} functions, {} builtins called, missing {:?}", summary.scripts, summary.functions, summary.builtins.len(), summary.missing.len());
    let mut flags: BTreeMap<u16, usize> = BTreeMap::new();
    for o in &translated {
        for f in &o.functions {
            *flags.entry(f.flags as u16).or_default() += 1;
        }
    }
    eprintln!("function flags: {flags:?}");
    let autoexecs = gsc_t7::autoexecs(&translated, &gsc_t7::include_lists(&objects));
    let mut strings = Strings::default();
    let program = Program::link(translated, &mut strings).expect("link");
    let bad: Vec<String> = program
        .functions
        .iter()
        .filter(|f| f.code.iter().any(|o| matches!(o, Op::Bad(_))))
        .map(|f| format!("{}::{}", program.scripts[f.script as usize].name, strings.get(f.name)))
        .collect();
    eprintln!("functions with operations the VM does not run: {}", bad.len());
    let mut vm: Vm<()> = Vm::new(program, strings);
    vm.t7_events = true;
    gsc_t6::natives::bind_core(&mut vm);
    vm.hooks.unbound = Some(unbound);
    let mut host = ();
    let mut started = 0;
    for (script, name) in &autoexecs {
        if let Some(fi) = vm.program.find(&vm.strings, script, name) {
            let level = Value::Object(vm.level);
            vm.spawn(&mut host, fi, level, Vec::new());
            started += 1;
        }
    }
    for _ in 0..40 {
        vm.time_ms += 50;
        vm.run_frame(&mut host);
    }
    let unbound = UNBOUND.lock().unwrap().clone();
    let mut by_count: Vec<_> = unbound.iter().collect();
    by_count.sort_by(|a, b| b.1.cmp(a.1));
    eprintln!("{started} autoexecs; {} threads left; {} unbound builtins called: {:?}", vm.thread_count(), by_count.len(), &by_count[..by_count.len().min(60)]);
    let errors: Vec<&String> = vm.messages.iter().filter(|m| !m.contains("unbound")).collect();
    eprintln!("{} messages: {:#?}", errors.len(), &errors[..errors.len().min(30)]);
}
