//! Synthetic-bytecode tests: every control-flow and scheduling construct, with a tiny assembler
//! that builds `gsc_iw6::Script`s directly (names are ids, so functions are numbered).

use std::collections::HashMap;

use gsc_iw6::{Case, CaseValue, Function, Instr, Name, Operand, Script};

use crate::*;

// ---- assembler ----------------------------------------------------------------------------

#[derive(Clone)]
enum A {
    U8(u8),
    U16(u16),
    F(f32),
    V3([f32; 3]),
    Tok(u16),
    S(&'static str),
    B(u16),
    Argc(u8),
    Jmp(&'static str),
    Fun(u16),
    Far(u16, u16),
    Cases(Vec<(CaseValue, &'static str)>),
    Tbl(&'static str),
}

enum Item {
    Label(&'static str),
    I(&'static str, Vec<A>),
}

fn i(m: &'static str) -> Item {
    Item::I(m, vec![])
}
fn ia(m: &'static str, a: Vec<A>) -> Item {
    Item::I(m, a)
}
fn lab(l: &'static str) -> Item {
    Item::Label(l)
}
fn byte(n: u8) -> Item {
    ia("get_byte", vec![A::U8(n)])
}
fn string(s: &'static str) -> Item {
    ia("get_string", vec![A::S(s)])
}

fn opcode(m: &str) -> (u8, &'static str) {
    gsc_iw6::opcodes::OPS
        .iter()
        .find(|o| o.mnemonic == m)
        .map(|o| (o.code, o.mnemonic))
        .unwrap_or_else(|| panic!("no opcode {m}"))
}

/// Builds a script from (function id, items). Function ids become `Name::Id`.
fn build(funcs: Vec<(u16, Vec<Item>)>) -> Script {
    // Pass 1: offsets.
    let mut off = 1usize;
    let mut fn_off: HashMap<u16, usize> = HashMap::new();
    let mut labels: Vec<HashMap<&'static str, usize>> = Vec::new();
    for (id, items) in &funcs {
        fn_off.insert(*id, off);
        let mut ls = HashMap::new();
        for it in items {
            match it {
                Item::Label(l) => {
                    ls.insert(*l, off);
                }
                Item::I(..) => off += 8,
            }
        }
        labels.push(ls);
    }
    let code_len = off + 1;
    // Pass 2: instructions.
    let mut instrs: Vec<Instr> = Vec::new();
    let mut functions = Vec::new();
    let mut off = 1usize;
    for (k, (id, items)) in funcs.iter().enumerate() {
        let first = instrs.len();
        let start = off;
        for it in items {
            let Item::I(m, args) = it else { continue };
            let (op, mnemonic) = opcode(m);
            let operands = args
                .iter()
                .map(|a| match a {
                    A::U8(n) => Operand::U8(*n),
                    A::U16(n) => Operand::U16(*n),
                    A::F(f) => Operand::F32(*f),
                    A::V3(v) => Operand::Vec3(*v),
                    A::Tok(t) => Operand::Token(Name::Id(*t)),
                    A::S(s) => Operand::Str((*s).to_owned()),
                    A::B(b) => Operand::Builtin(*b),
                    A::Argc(n) => Operand::Argc(*n),
                    A::Jmp(l) => Operand::Jump(*labels[k].get(l).unwrap_or_else(|| panic!("label {l}"))),
                    A::Tbl(l) => Operand::Table(*labels[k].get(l).unwrap_or_else(|| panic!("label {l}"))),
                    A::Fun(f) => Operand::Func(fn_off[f]),
                    A::Far(file, f) => Operand::Far { file: Name::Id(*file), func: Name::Id(*f) },
                    A::Cases(cs) => Operand::Cases(
                        cs.iter()
                            .map(|(v, l)| Case { value: v.clone(), target: labels[k][l] })
                            .collect(),
                    ),
                })
                .collect();
            instrs.push(Instr { offset: off, op, mnemonic, len: 8, operands });
            off += 8;
        }
        let params = items
            .iter()
            .take_while(|it| !matches!(it, Item::I("check_params", _)))
            .filter(|it| matches!(it, Item::I("param", _)))
            .count();
        functions.push(Function {
            name: Name::Id(*id),
            offset: start,
            end: off,
            first,
            count: instrs.len() - first,
            params,
        });
    }
    Script { prelude: 0x34, code_len, stream_len: 0, stream_used: 0, functions, instrs }
}

fn vm_with(scripts: Vec<(u16, Vec<(u16, Vec<Item>)>)>) -> Vm<NullHost> {
    vm_host(scripts, NullHost)
}

fn vm_host<H: Host>(scripts: Vec<(u16, Vec<(u16, Vec<Item>)>)>, host: H) -> Vm<H> {
    let mut set = ScriptSet::new();
    for (file, funcs) in scripts {
        set.add(Name::Id(file), build(funcs));
    }
    Vm::new(set, host)
}

fn f(vm: &Vm<impl Host>, file: u16, func: u16) -> FuncRef {
    vm.resolve(&Name::Id(file), &Name::Id(func)).expect("function")
}

const LOG: u16 = 900;

/// Appends `value` (already on the stack) to `level.log`.
fn append_top() -> Vec<Item> {
    vec![
        ia("get_level_field", vec![A::Tok(LOG)]),
        i("size"),
        ia("ref_level_field", vec![A::Tok(LOG)]),
        i("ref_array"),
        i("set_variable_field"),
    ]
}

/// `level.log = []`
fn init_log() -> Vec<Item> {
    vec![i("empty_array"), ia("set_level_field", vec![A::Tok(LOG)])]
}

fn log_of(vm: &Vm<impl Host>) -> Vec<i32> {
    match vm.level_obj().get(&Name::Id(LOG)) {
        Some(Value::Array(a)) => a.values().iter().map(|v| v.as_int().expect("int")).collect(),
        other => panic!("no log: {other:?}"),
    }
}

fn cat(parts: Vec<Vec<Item>>) -> Vec<Item> {
    parts.into_iter().flatten().collect()
}

// ---- tests --------------------------------------------------------------------------------

#[test]
fn arithmetic_and_promotion() {
    let main = vec![
        i("check_params"),
        byte(6),
        byte(7),
        i("multiply"),
        byte(2),
        i("minus"),
        i("return"),
    ];
    let div = vec![i("check_params"), byte(3), byte(2), i("divide"), i("return")];
    let mix = vec![i("check_params"), byte(1), ia("get_float", vec![A::F(0.5)]), i("plus"), i("return")];
    let neg = vec![i("check_params"), ia("get_neg_byte", vec![A::U8(4)]), ia("get_ushort", vec![A::U16(300)]), i("plus"), i("return")];
    let mut vm = vm_with(vec![(1, vec![(1, main), (2, div), (3, mix), (4, neg)])]);
    let r = |vm: &mut Vm<NullHost>, n| vm.call(vm.level(), f(vm, 1, n), vec![]);
    assert!(matches!(r(&mut vm, 1), Some(Value::Int(40))));
    assert!(matches!(r(&mut vm, 2), Some(Value::Float(x)) if x == 1.5));
    assert!(matches!(r(&mut vm, 3), Some(Value::Float(x)) if x == 1.5));
    assert!(matches!(r(&mut vm, 4), Some(Value::Int(296))));
}

#[test]
fn division_by_zero_and_type_errors_kill_the_thread() {
    let div0 = vec![i("check_params"), byte(1), i("get_zero"), i("divide"), i("return")];
    let bad = vec![i("check_params"), string("x"), byte(1), i("minus"), i("return")];
    let ok = vec![i("check_params"), byte(5), i("return")];
    let mut vm = vm_with(vec![(1, vec![(1, div0), (2, bad), (3, ok)])]);
    assert!(vm.call(vm.level(), f(&vm, 1, 1), vec![]).is_none());
    assert!(vm.call(vm.level(), f(&vm, 1, 2), vec![]).is_none());
    let errs = vm.take_errors();
    assert_eq!(errs.len(), 2);
    assert!(errs[0].message.contains("divide by 0"), "{}", errs[0]);
    assert!(errs[1].message.contains("cannot apply -"), "{}", errs[1]);
    assert_eq!(errs[0].function, "#1::#1");
    // the VM is fine afterwards
    assert!(matches!(vm.call(vm.level(), f(&vm, 1, 3), vec![]), Some(Value::Int(5))));
}

#[test]
fn loop_with_locals_depth_addressing() {
    // sum = 0; i = 0; while (i < 5) { sum = sum + i; i++; } return sum;
    let main = vec![
        i("check_params"),
        i("get_zero"),
        ia("init_local", vec![A::U8(0)]),
        i("get_zero"),
        ia("init_local", vec![A::U8(1)]),
        lab("top"),
        i("get_local0"),
        byte(5),
        i("less"),
        ia("jump_false", vec![A::Jmp("end")]),
        i("get_local1"),
        i("get_local0"),
        i("plus"),
        ia("set_local", vec![A::U8(1)]),
        i("ref_local0"),
        i("inc"),
        i("set_variable_field"),
        ia("jump_back", vec![A::Jmp("top")]),
        lab("end"),
        i("get_local1"),
        i("return"),
    ];
    let mut vm = vm_with(vec![(1, vec![(1, main)])]);
    assert!(matches!(vm.call(vm.level(), f(&vm, 1, 1), vec![]), Some(Value::Int(10))));
}

#[test]
fn switch_ints_strings_default_and_fallthrough() {
    let body = |with_default: bool| {
        let mut cases = vec![
            (CaseValue::Int(1), "c1"),
            (CaseValue::Int(2), "c2"),
            (CaseValue::Str("abc".into()), "c3"),
        ];
        if with_default {
            cases.push((CaseValue::Default, "cd"));
        }
        vec![
            ia("param", vec![A::U8(0)]),
            i("check_params"),
            i("get_local0"),
            ia("switch", vec![A::Tbl("tbl")]),
            lab("tbl"),
            ia("switch_table", vec![A::Cases(cases)]),
            byte(99),
            i("return"),
            lab("c1"),
            byte(10),
            i("return"),
            lab("c2"),
            byte(20),
            i("return"),
            lab("c3"),
            byte(30),
            i("return"),
            lab("cd"),
            byte(40),
            i("return"),
        ]
    };
    let mut vm = vm_with(vec![(1, vec![(1, body(true)), (2, body(false))])]);
    let run = |vm: &mut Vm<NullHost>, n, v: Value| match vm.call(vm.level(), f(vm, 1, n), vec![v]) {
        Some(Value::Int(i)) => i,
        other => panic!("{other:?}"),
    };
    assert_eq!(run(&mut vm, 1, Value::Int(1)), 10);
    assert_eq!(run(&mut vm, 1, Value::Int(2)), 20);
    assert_eq!(run(&mut vm, 1, Value::str("ABC")), 30);
    assert_eq!(run(&mut vm, 1, Value::Int(7)), 40);
    assert_eq!(run(&mut vm, 2, Value::Int(7)), 99);
    assert_eq!(run(&mut vm, 2, Value::str("abc")), 30);
}

#[test]
fn threads_run_immediately_then_in_wake_order() {
    // worker(id, delay): wait delay; log.append(id)
    let worker = cat(vec![
        vec![ia("param", vec![A::U8(0)]), ia("param", vec![A::U8(1)]), i("check_params")],
        vec![i("get_local0"), i("wait"), i("get_local1")],
        append_top(),
        vec![i("end")],
    ]);
    let spawn = |id: u8, d: f32| {
        vec![ia("get_float", vec![A::F(d)]), byte(id), ia("thread_local", vec![A::Fun(2), A::Argc(2)]), i("dec_top")]
    };
    let main = cat(vec![vec![i("check_params")], init_log(), spawn(1, 0.1), spawn(2, 0.05), spawn(3, 0.1), vec![i("end")]]);
    // (worker pops delay first? param 0 is the first argument = id.) Swap: first arg id on top.
    let mut vm = vm_with(vec![(1, vec![(1, main), (2, worker)])]);
    // worker's first param is the id (pushed last = top); its wait uses local at depth 0 = the
    // newest local = the delay.
    vm.start_thread(f(&vm, 1, 1), vec![]);
    assert_eq!(vm.threads().len(), 3);
    assert!(log_of(&vm).is_empty());
    vm.step(50);
    assert!(log_of(&vm).is_empty());
    vm.step(50);
    assert_eq!(log_of(&vm), vec![2]);
    vm.step(50);
    assert_eq!(log_of(&vm), vec![2, 1, 3], "ties wake in the order the waits began");
    assert!(vm.threads().is_empty());
    assert_eq!(vm.time_ms(), 150);
    assert!(vm.take_errors().is_empty());
}

#[test]
fn wait_rounds_to_ticks() {
    // wait 0.01 -> 1 tick (nonzero waits are at least one tick); wait 0.074 -> 1; 0.076 -> 2
    let w = |secs: f32| {
        cat(vec![vec![i("check_params")], init_log(), vec![ia("get_float", vec![A::F(secs)]), i("wait"), byte(1)], append_top(), vec![i("end")]])
    };
    let mut vm = vm_with(vec![(1, vec![(1, w(0.01)), (2, w(0.074)), (3, w(0.076)), (4, w(1.0))])]);
    for n in 1..=4u16 {
        vm.start_thread(f(&vm, 1, n), vec![]);
    }
    // each thread shares `level.log`; the last init wins, so check by thread state instead
    let mut done_at = Vec::new();
    for _ in 0..25 {
        let before = vm.threads().len();
        vm.step(50);
        done_at.push(before - vm.threads().len());
    }
    let first = done_at.iter().position(|&n| n > 0).expect("some finish");
    assert_eq!(done_at[first], 2, "{done_at:?}"); // 0.01 s and 0.074 s both round to one tick
    assert_eq!(done_at[first + 1], 1, "{done_at:?}"); // 0.076 s -> 2 ticks
    assert_eq!(done_at[first + 19], 1, "{done_at:?}"); // 1.0 s -> 20 ticks
    assert_eq!(done_at.iter().sum::<usize>(), 4);
}

#[test]
fn waittill_notify_with_arguments_in_registration_order() {
    // waiter(id): level waittill("go"); arg -> log; id -> log
    let waiter = cat(vec![
        vec![ia("param", vec![A::U8(0)]), i("check_params"), string("go"), i("get_level"), i("waittill"), ia("new_local", vec![A::U8(1)]), ia("set_waittill_arg", vec![A::U8(0)]), i("clear_params"), i("get_local0")],
        append_top(),
        vec![i("get_local1")],
        append_top(),
        vec![i("end")],
    ]);
    let notifier = vec![
        i("check_params"),
        ia("get_float", vec![A::F(0.2)]),
        i("wait"),
        i("pre_call"),
        byte(77),
        string("go"),
        i("get_level"),
        i("notify"),
        // a second notify with nobody waiting: not remembered
        i("pre_call"),
        byte(78),
        string("go"),
        i("get_level"),
        i("notify"),
        i("end"),
    ];
    let main = cat(vec![
        vec![i("check_params")],
        init_log(),
        vec![byte(1), ia("thread_local", vec![A::Fun(2), A::Argc(1)]), i("dec_top")],
        vec![byte(2), ia("thread_local", vec![A::Fun(2), A::Argc(1)]), i("dec_top")],
        vec![ia("thread_local", vec![A::Fun(3), A::Argc(0)]), i("dec_top"), i("end")],
    ]);
    let mut vm = vm_with(vec![(1, vec![(1, main), (2, waiter), (3, notifier)])]);
    vm.start_thread(f(&vm, 1, 1), vec![]);
    assert_eq!(vm.waiting_on().len(), 1);
    assert_eq!(vm.waiting_on()[0].2.len(), 2);
    vm.step(150);
    assert!(log_of(&vm).is_empty());
    vm.step(100);
    assert_eq!(log_of(&vm), vec![77, 1, 77, 2], "waiters run in registration order, same tick (arg then id)");
    assert!(vm.take_errors().is_empty());
}

#[test]
fn waittillmatch_filters_on_arguments() {
    // waiter: level waittillmatch("evt", "yes") -> log.append(1)
    let waiter = cat(vec![
        vec![i("check_params"), string("yes"), string("evt"), i("get_level"), ia("waittillmatch", vec![A::U16(0xA301)]), i("clear_params"), byte(1)],
        append_top(),
        vec![i("end")],
    ]);
    let send = |arg: &'static str| vec![i("pre_call"), string(arg), string("evt"), i("get_level"), i("notify")];
    let main = cat(vec![vec![i("check_params")], init_log(), vec![ia("thread_local", vec![A::Fun(2), A::Argc(0)]), i("dec_top")], send("no"), vec![i("end")]]);
    let main2 = cat(vec![vec![i("check_params")], send("YES"), vec![i("end")]]);
    let mut vm = vm_with(vec![(1, vec![(1, main), (2, waiter), (3, main2)])]);
    vm.start_thread(f(&vm, 1, 1), vec![]);
    vm.step(50);
    assert!(log_of(&vm).is_empty(), "a non-matching notify leaves the waiter");
    assert_eq!(vm.threads().len(), 1);
    vm.start_thread(f(&vm, 1, 3), vec![]);
    vm.step(50);
    assert_eq!(log_of(&vm), vec![1], "matching is case-insensitive");
}

#[test]
fn endon_kills_the_thread_and_childthread_dies_with_its_parent() {
    // looper: level endon("stop"); while (1) { wait 1; log.append(100) }
    let looper = vec![
        i("check_params"),
        string("stop"),
        i("get_level"),
        i("endon"),
        lab("top"),
        byte(1),
        i("wait"),
        byte(100),
    ];
    let looper = cat(vec![looper, append_top(), vec![ia("jump_back", vec![A::Jmp("top")])]]);
    // child: while (1) { wait 0.5; log.append(5) }
    let child = cat(vec![
        vec![i("check_params"), lab("top"), ia("get_float", vec![A::F(0.5)]), i("wait"), byte(5)],
        append_top(),
        vec![ia("jump_back", vec![A::Jmp("top")])],
    ]);
    // parent: childthread child(); wait 1; (returns)
    let parent = vec![i("check_params"), ia("childthread_local", vec![A::Fun(3), A::Argc(0)]), i("dec_top"), byte(1), i("wait"), i("end")];
    // plain: thread child(); wait 1;
    let plain = vec![i("check_params"), ia("thread_local", vec![A::Fun(3), A::Argc(0)]), i("dec_top"), byte(1), i("wait"), i("end")];
    let stopper = vec![i("check_params"), i("pre_call"), string("stop"), i("get_level"), i("notify"), i("end")];
    let init = cat(vec![vec![i("check_params")], init_log(), vec![i("end")]]);
    let mut vm = vm_with(vec![(1, vec![(1, looper), (2, init), (3, child), (4, parent), (5, plain), (6, stopper)])]);
    vm.start_thread(f(&vm, 1, 2), vec![]);
    vm.start_thread(f(&vm, 1, 1), vec![]);
    vm.step(2500);
    assert_eq!(log_of(&vm), vec![100, 100]);
    vm.start_thread(f(&vm, 1, 6), vec![]);
    vm.flush();
    assert!(vm.threads().is_empty(), "endon killed the looper: {:?}", vm.threads());
    vm.step(3000);
    assert_eq!(log_of(&vm), vec![100, 100]);

    // childthread: the child dies when the parent function returns (after 1 s)
    vm.start_thread(f(&vm, 1, 2), vec![]);
    vm.start_thread(f(&vm, 1, 4), vec![]);
    vm.step(5000);
    assert_eq!(log_of(&vm), vec![5], "child ran at 0.5 s, died with its parent at 1 s");
    assert!(vm.threads().is_empty());
    // plain thread: survives
    vm.start_thread(f(&vm, 1, 2), vec![]);
    vm.start_thread(f(&vm, 1, 5), vec![]);
    vm.step(2100);
    assert_eq!(log_of(&vm).len(), 4);
    assert_eq!(vm.threads().len(), 1);
}

#[test]
fn waittillframeend_runs_after_the_other_threads_of_the_tick() {
    let late = cat(vec![vec![i("check_params"), i("waittill_frame_end_marker")]]);
    drop(late);
    let x = cat(vec![vec![i("check_params"), i("wait_frame_end"), byte(1)], append_top(), vec![i("end")]]);
    let y = cat(vec![vec![i("check_params"), byte(2)], append_top(), vec![i("end")]]);
    let main = cat(vec![
        vec![i("check_params")],
        init_log(),
        vec![ia("thread_local", vec![A::Fun(2), A::Argc(0)]), i("dec_top"), ia("thread_local", vec![A::Fun(3), A::Argc(0)]), i("dec_top"), i("end")],
    ]);
    let mut vm = vm_with(vec![(1, vec![(1, main), (2, x), (3, y)])]);
    vm.start_thread(f(&vm, 1, 1), vec![]);
    assert_eq!(log_of(&vm), vec![2]);
    vm.step(50);
    assert_eq!(log_of(&vm), vec![2, 1]);
}

#[test]
fn arrays_by_reference_keys_and_iteration() {
    // a = []; a["x"] = 5; a[0] = 6; b = a; b["y"] = 7; return b  (shared)
    let main = vec![
        i("check_params"),
        i("empty_array"),
        ia("init_local", vec![A::U8(0)]),
        byte(5),
        string("x"),
        i("ref_local0"),
        i("ref_array"),
        i("set_variable_field"),
        byte(6),
        i("get_zero"),
        i("ref_local0"),
        i("ref_array"),
        i("set_variable_field"),
        i("get_local0"),
        ia("init_local", vec![A::U8(1)]),
        byte(7),
        string("y"),
        i("ref_local0"),
        i("ref_array"),
        i("set_variable_field"),
        i("get_local1"),
        i("return"),
    ];
    // sum = 0; for (k = first(a); isdefined(k); k = next(a, k)) sum += a[k]; return sum
    let sum = vec![
        ia("param", vec![A::U8(0)]),
        i("check_params"),
        i("get_zero"),
        ia("init_local", vec![A::U8(1)]),
        i("get_local1"),
        ia("call_builtin1", vec![A::B(0x1c1)]),
        ia("init_local", vec![A::U8(2)]),
        lab("top"),
        i("get_local0"),
        ia("call_builtin1", vec![A::B(0x2d)]),
        ia("jump_false", vec![A::Jmp("end")]),
        i("get_local0"),
        ia("get_local_array_elem", vec![A::U8(2)]),
        i("get_local1"),
        i("plus"),
        ia("set_local", vec![A::U8(1)]),
        i("get_local0"),
        i("get_local2"),
        ia("call_builtin2", vec![A::B(0x1a6)]),
        i("set_local0"),
        ia("jump_back", vec![A::Jmp("top")]),
        lab("end"),
        i("get_local1"),
        i("return"),
    ];
    let mut vm = vm_with(vec![(1, vec![(1, main), (2, sum)])]);
    let Some(Value::Array(a)) = vm.call(vm.level(), f(&vm, 1, 1), vec![]) else { panic!("array") };
    assert_eq!(a.len(), 3);
    assert!(matches!(a.get(&Key::Str("y".into())), Some(Value::Int(7))));
    assert!(matches!(a.get(&Key::Int(0)), Some(Value::Int(6))));
    assert!(matches!(vm.call(vm.level(), f(&vm, 1, 2), vec![Value::Array(a.clone())]), Some(Value::Int(18))));
    // setting an element to undefined removes it
    let rm = vec![
        ia("param", vec![A::U8(0)]),
        i("check_params"),
        i("get_undefined"),
        string("x"),
        i("ref_local0"),
        i("ref_array"),
        i("set_variable_field"),
        i("get_local0"),
        i("size"),
        i("return"),
    ];
    let mut vm2 = vm_with(vec![(1, vec![(1, rm)])]);
    assert!(matches!(vm2.call(vm2.level(), f(&vm2, 1, 1), vec![Value::Array(a)]), Some(Value::Int(2))));
}

#[test]
fn vectors_and_strings() {
    let vecf = vec![
        i("check_params"),
        // (1, 2, 3): x is on top
        byte(3),
        byte(2),
        byte(1),
        i("make_vector"),
        byte(2),
        i("multiply"),
        ia("get_vector", vec![A::V3([0.5, 0.0, 1.0])]),
        i("plus"),
        i("return"),
    ];
    let strf = vec![i("check_params"), string("n="), byte(5), i("plus"), i("return")];
    let eqf = vec![i("check_params"), string("Hello"), string("hELLO"), i("equality"), i("return")];
    let mut vm = vm_with(vec![(1, vec![(1, vecf), (2, strf), (3, eqf)])]);
    assert!(matches!(vm.call(vm.level(), f(&vm, 1, 1), vec![]), Some(Value::Vector(v)) if v == [2.5, 4.0, 7.0]));
    assert!(matches!(vm.call(vm.level(), f(&vm, 1, 2), vec![]), Some(Value::Str(s)) if &*s == "n=5"));
    assert!(matches!(vm.call(vm.level(), f(&vm, 1, 3), vec![]), Some(Value::Int(1))));
}

#[test]
fn far_calls_pointers_and_parameter_checks() {
    // file 2::add(a, b) = a + b ; file 1::main calls add(3, 4) via far call, via pointer, with a
    // thread, and with too many arguments.
    let add = vec![ia("param", vec![A::U8(0)]), ia("param", vec![A::U8(1)]), i("check_params"), i("get_local1"), i("get_local0"), i("plus"), i("return")];
    let main = vec![i("check_params"), i("pre_call"), byte(4), byte(3), ia("call_far", vec![A::Far(2, 1)]), i("return")];
    let ptr = vec![i("check_params"), i("pre_call"), byte(40), byte(30), ia("get_function_far", vec![A::Far(2, 1)]), i("call_ptr"), i("return")];
    let many = vec![i("check_params"), i("pre_call"), byte(1), byte(1), byte(1), ia("call_far", vec![A::Far(2, 1)]), i("return")];
    let few = vec![i("check_params"), i("pre_call"), byte(1), ia("call_far", vec![A::Far(2, 1)]), i("return")];
    let missing = vec![i("check_params"), ia("call0_far", vec![A::Far(2, 99)]), i("return")];
    let mut vm = vm_with(vec![(2, vec![(1, add)]), (1, vec![(1, main), (2, ptr), (3, many), (4, few), (5, missing)])]);
    assert!(matches!(vm.call(vm.level(), f(&vm, 1, 1), vec![]), Some(Value::Int(7))));
    assert!(matches!(vm.call(vm.level(), f(&vm, 1, 2), vec![]), Some(Value::Int(70))));
    assert!(vm.call(vm.level(), f(&vm, 1, 3), vec![]).is_none());
    // too few arguments: the missing parameter is undefined, and undefined + int is an error
    assert!(vm.call(vm.level(), f(&vm, 1, 4), vec![]).is_none());
    assert!(vm.call(vm.level(), f(&vm, 1, 5), vec![]).is_none());
    let errs = vm.take_errors();
    assert_eq!(errs.len(), 3);
    assert!(errs[0].message.contains("too many parameters"), "{}", errs[0]);
    assert!(errs[1].message.contains("cannot apply +"), "{}", errs[1]);
    assert!(errs[2].message.contains("unresolved function"), "{}", errs[2]);
    assert_eq!(errs[0].stack.len(), 2);
}

#[test]
fn recursion_overflow_and_runaway_loops_are_contained() {
    let rec = vec![i("check_params"), i("call0_local_self")];
    drop(rec);
    let rec = vec![i("check_params"), ia("call0_local", vec![A::Fun(1)]), i("return")];
    let spin = vec![i("check_params"), lab("top"), ia("jump_back", vec![A::Jmp("top")])];
    let mut vm = vm_with(vec![(1, vec![(1, rec), (2, spin)])]);
    vm.budget = 10_000;
    assert!(vm.call(vm.level(), f(&vm, 1, 1), vec![]).is_none());
    assert!(vm.call(vm.level(), f(&vm, 1, 2), vec![]).is_none());
    let errs = vm.take_errors();
    assert!(errs[0].message.contains("stack overflow"), "{}", errs[0]);
    assert!(errs[1].message.contains("budget"), "{}", errs[1]);
}

#[derive(Default)]
struct TestHost {
    calls: Vec<(u16, Vec<Value>)>,
    fields: HashMap<(u32, u16), Value>,
}

impl Host for TestHost {
    fn call_function(&mut self, id: u16, ctx: &mut CallCtx<'_>, args: &[Value]) -> Result<HostReturn, String> {
        self.calls.push((id, args.to_vec()));
        Ok(match id {
            0x100 => HostReturn::Value(Value::Int(args[0].as_int().unwrap_or(0) * 2)),
            0x101 => HostReturn::Wait(1.0),
            0x102 => {
                ctx.notify(ctx.level.clone(), "from_host", vec![Value::Int(9)]);
                HostReturn::None
            }
            0x103 => HostReturn::WaitNotify { target: ctx.level.clone(), name: "wake".into() },
            0x104 => Err("boom".to_owned())?,
            _ => HostReturn::Unimplemented,
        })
    }
    fn call_method(&mut self, id: u16, _ctx: &mut CallCtx<'_>, this: &Value, args: &[Value]) -> Result<HostReturn, String> {
        Ok(match id {
            0x8010 => HostReturn::Value(Value::Int(this.as_entity().map_or(-1, |e| e.0 as i32) + args[0].as_int().unwrap_or(0))),
            _ => HostReturn::Unimplemented,
        })
    }
    fn get_entity_field(&mut self, e: EntityId, field: &Name) -> Option<Value> {
        let Name::Id(id @ 5) = field else { return None };
        Some(self.fields.get(&(e.0, *id)).cloned().unwrap_or(Value::Int(-1)))
    }
    fn set_entity_field(&mut self, e: EntityId, field: &Name, v: &Value) -> bool {
        let Name::Id(id @ 5) = field else { return false };
        self.fields.insert((e.0, *id), v.clone());
        true
    }
}

#[test]
fn builtins_waits_unknown_ids_and_host_errors() {
    let call = |id: u16, argc: u8| vec![i("check_params"), byte(21), ia("call_builtin1", vec![A::B(id)]), i("return")].into_iter().take(if argc == 0 { 0 } else { 99 }).collect::<Vec<_>>();
    let _ = call;
    let double = vec![i("check_params"), byte(21), ia("call_builtin1", vec![A::B(0x100)]), i("return")];
    let unknown = vec![i("check_params"), byte(1), ia("call_builtin1", vec![A::B(0x999)]), i("return")];
    let waiter = cat(vec![
        vec![i("check_params"), ia("call_builtin0", vec![A::B(0x101)]), i("dec_top"), byte(1)],
        append_top(),
        vec![i("end")],
    ]);
    let hostnotify = cat(vec![
        vec![i("check_params"), string("from_host"), i("get_level"), i("waittill"), ia("new_local", vec![A::U8(0)]), ia("set_waittill_arg", vec![A::U8(0)]), i("clear_params"), i("get_local0")],
        append_top(),
        vec![i("end")],
    ]);
    let fire = vec![i("check_params"), ia("call_builtin0", vec![A::B(0x102)]), i("return")];
    let hostwait = cat(vec![
        vec![i("check_params"), ia("call_builtin0", vec![A::B(0x103)]), i("dec_top"), byte(3)],
        append_top(),
        vec![i("end")],
    ]);
    let boom = vec![i("check_params"), ia("call_builtin0", vec![A::B(0x104)]), i("return")];
    let init = cat(vec![vec![i("check_params")], init_log(), vec![i("end")]]);
    let mut vm = vm_host(
        vec![(1, vec![(1, double), (2, unknown), (3, waiter), (4, hostnotify), (5, fire), (6, hostwait), (7, boom), (8, init)])],
        TestHost::default(),
    );
    assert!(matches!(vm.call(vm.level(), f(&vm, 1, 1), vec![]), Some(Value::Int(42))));
    assert!(matches!(vm.call(vm.level(), f(&vm, 1, 2), vec![]), Some(Value::Undefined)));
    assert_eq!(vm.unimplemented_builtins().get(&(false, 0x999)), Some(&1));
    vm.strict = true;
    assert!(vm.call(vm.level(), f(&vm, 1, 2), vec![]).is_none());
    vm.strict = false;
    vm.start_thread(f(&vm, 1, 8), vec![]);
    // a builtin that waits
    vm.start_thread(f(&vm, 1, 3), vec![]);
    vm.step(900);
    assert!(log_of(&vm).is_empty());
    vm.step(200);
    assert_eq!(log_of(&vm), vec![1]);
    // a builtin that queues a notify
    vm.start_thread(f(&vm, 1, 4), vec![]);
    vm.start_thread(f(&vm, 1, 5), vec![]);
    vm.flush();
    assert_eq!(log_of(&vm), vec![1, 9]);
    // a builtin that blocks until a notify
    vm.start_thread(f(&vm, 1, 6), vec![]);
    assert_eq!(vm.waiters_of(&vm.level(), "wake").len(), 1);
    vm.notify(&vm.level(), "WAKE", vec![]);
    vm.flush();
    assert_eq!(log_of(&vm), vec![1, 9, 3]);
    // a host error kills just that thread
    assert!(vm.call(vm.level(), f(&vm, 1, 7), vec![]).is_none());
    let errs = vm.take_errors();
    assert!(errs.last().is_some_and(|e| e.message == "boom"), "{errs:?}");
}

#[test]
fn entity_fields_go_through_the_host_and_methods_need_objects() {
    // self.5 = 3 (engine field); self.6 = self.5 + 1 (script variable); return self method0x10(4)
    let main = vec![
        i("check_params"),
        byte(3),
        ia("set_self_field", vec![A::Tok(5)]),
        ia("get_self_field", vec![A::Tok(5)]),
        byte(1),
        i("plus"),
        ia("set_self_field", vec![A::Tok(6)]),
        byte(4),
        i("get_self"),
        ia("call_builtin_method1", vec![A::B(0x8010)]),
        i("return"),
    ];
    let on_undefined = vec![i("check_params"), byte(4), i("get_undefined"), ia("call_builtin_method1", vec![A::B(0x8010)]), i("return")];
    let mut vm = vm_host(vec![(1, vec![(1, main), (2, on_undefined)])], TestHost::default());
    let e = EntityId(12);
    let id = vm.start_thread_on(Value::Entity(e), f(&vm, 1, 1), vec![]);
    assert!(vm.take_errors().is_empty());
    assert!(id > 0);
    assert!(matches!(vm.host().fields.get(&(12, 5)), Some(Value::Int(3))));
    let vars = vm.entity_vars(e).expect("script variables");
    assert!(matches!(vars.get(&Name::Id(6)), Some(Value::Int(4))));
    assert!(vars.get(&Name::Id(5)).is_none(), "engine fields are not stored as script variables");
    assert!(vm.call(vm.level(), f(&vm, 1, 2), vec![]).is_none());
    assert!(vm.take_errors()[0].message.contains("undefined"));
}

#[test]
fn entity_removal_kills_threads_and_drops_waiters() {
    let waiter = vec![i("check_params"), string("x"), i("get_self"), i("waittill"), i("end")];
    let mut vm = vm_with(vec![(1, vec![(1, waiter)])]);
    let e = Value::Entity(EntityId(3));
    vm.start_thread_on(e.clone(), f(&vm, 1, 1), vec![]);
    assert_eq!(vm.threads().len(), 1);
    assert_eq!(vm.waiters_of(&e, "x").len(), 1);
    vm.entity_removed(EntityId(3));
    assert!(vm.threads().is_empty());
    assert!(vm.waiters_of(&e, "x").is_empty());
}

#[test]
fn script_errors_report_file_function_offset() {
    let bad = vec![i("check_params"), i("get_undefined"), byte(1), i("plus"), i("return")];
    let mut vm = vm_with(vec![(7, vec![(33, bad)])]);
    vm.start_thread(f(&vm, 7, 33), vec![]);
    let e = vm.take_errors().remove(0);
    assert_eq!(e.function, "#7::#33");
    assert_eq!(e.offset, 1 + 8 * 3);
    assert_eq!(e.thread, 1);
}

#[test]
fn far_call_chain_returns_through_frames_and_locals_are_per_frame() {
    // outer: x = 5; y = inner(); return x * 100 + y ; inner has its own local 0 = 7
    let inner = vec![i("check_params"), byte(7), ia("init_local", vec![A::U8(0)]), i("get_local0"), i("return")];
    let outer = vec![
        i("check_params"),
        byte(5),
        ia("init_local", vec![A::U8(0)]),
        ia("call0_local", vec![A::Fun(2)]),
        ia("init_local", vec![A::U8(1)]),
        i("get_local1"),
        byte(100),
        i("multiply"),
        i("get_local0"),
        i("plus"),
        i("return"),
    ];
    let mut vm = vm_with(vec![(1, vec![(1, outer), (2, inner)])]);
    assert!(matches!(vm.call(vm.level(), f(&vm, 1, 1), vec![]), Some(Value::Int(507))));
}

#[test]
fn census_and_trace() {
    let events = std::sync::Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
    let sink = events.clone();
    let main = cat(vec![vec![i("check_params")], init_log(), vec![string("tick"), i("get_level"), i("waittill"), i("end")]]);
    let mut vm = vm_with(vec![(1, vec![(1, main)])]);
    vm.enable_census();
    vm.set_trace(Some(Box::new(move |e| sink.lock().expect("lock").push(format!("{e:?}")))));
    vm.start_thread(f(&vm, 1, 1), vec![]);
    vm.notify(&vm.level(), "tick", vec![]);
    vm.flush();
    let c = vm.census.as_ref().expect("census");
    assert_eq!(c.waittills.get("tick"), Some(&1));
    assert_eq!(c.notifies.get("tick"), Some(&1));
    assert_eq!(c.threads_started, 1);
    let log = events.lock().expect("lock").join("\n");
    assert!(log.contains("ThreadStart") && log.contains("Waittill") && log.contains("ThreadEnd"), "{log}");
    assert!(c.report(vm.scripts()).contains("waittill names"));
}
