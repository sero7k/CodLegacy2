//! Black Ops III script objects as Black Ops II VM objects
//! (`gsc_t6::ScriptObject`), so `gsc_t6`'s linker and VM run them.
//!
//! Black Ops III links by canonical hash (namespace, name): a call goes to
//! the export with that pair in any loaded script, else to the engine
//! builtin with that name. [`translate`] makes that decision once, for all
//! the objects together, and writes each call as Black Ops II's linker
//! takes it: a script call names the target's script and its function
//! name; a builtin call has no namespace and the builtin's name. Hashes
//! become names where one is known ([`super::names`]), else `#xxxxxxxx`.
//! A function whose namespace is not its script's main one (class methods,
//! a second `#namespace`) is named `<namespace>::<name>`.
//!
//! Operations Black Ops II has no opcode for use `gsc_t6`'s added ones
//! (`SUPER_EQUAL`, `NEW`, `CLASS_FUNCTION_CALL`, the `classes` and `world`
//! globals, `...`). `self.x` and `level.x` (one operation in Black Ops III)
//! become the field object and the field (two).

use rustc_hash::{FxHashMap, FxHashSet};

use crate::object::{Case, CaseValue as T6Case, DecodedFunction, Import as T6Import, Insn as T6, ScriptObject, op};

use super::names::{builtin, display};
use super::object::{CaseValue, Insn, T7Object};
use super::ops::T7Op;

/// What the translation found (for a report).
#[derive(Clone, Debug, Default)]
pub struct Summary {
    pub scripts: usize,
    pub functions: usize,
    /// Calls bound to builtins, by name (including unnamed `#hash`es).
    pub builtins: FxHashSet<String>,
    /// Namespaced calls whose function no loaded script has (another
    /// mode's), as `namespace::name`.
    pub missing: FxHashSet<String>,
}

fn t6_plain(o: T7Op) -> Option<u8> {
    use T7Op::*;
    Some(match o {
        END => op::END,
        RETURN => op::RETURN,
        GET_UNDEFINED | GET_UNDEFINED_2 | GET_UNRESOLVED_FUNCTION => op::GET_UNDEFINED,
        GET_ZERO => op::GET_ZERO,
        GET_LEVEL_OBJECT => op::GET_LEVEL_OBJECT,
        GET_LEVEL => op::GET_LEVEL,
        GET_OBJECT_844 => op::GET_ANIM_OBJECT,
        GET_844 => op::GET_ANIM,
        GET_OBJECT_834 => op::GET_WORLD_OBJECT,
        GET_834 => op::GET_WORLD,
        GET_CLASSES_OBJECT => op::GET_CLASSES_OBJECT,
        GET_CLASSES => op::GET_CLASSES,
        GET_GAME => op::GET_GAME,
        GET_GAME_REF => op::GET_GAME_REF,
        GET_SELF => op::GET_SELF,
        GET_SELF_OBJECT => op::GET_SELF_OBJECT,
        EVAL_ARRAY => op::EVAL_ARRAY,
        EVAL_ARRAY_REF => op::EVAL_ARRAY_REF,
        EMPTY_ARRAY => op::EMPTY_ARRAY,
        SET_VARIABLE_FIELD => op::SET_VARIABLE_FIELD,
        CLEAR_ARRAY => op::CLEAR_ARRAY,
        CLEAR_PARAMS => op::CLEAR_PARAMS,
        CHECK_CLEAR_PARAMS => op::CHECK_CLEAR_PARAMS,
        PRE_SCRIPT_CALL => op::PRE_SCRIPT_CALL,
        DEC_TOP => op::DEC_TOP,
        SAFE_DEC_TOP => op::SAFE_DEC_TOP,
        CAST_FIELD_OBJECT => op::CAST_FIELD_OBJECT,
        CAST_BOOL => op::CAST_BOOL,
        BOOL_NOT => op::BOOL_NOT,
        BOOL_COMPLEMENT => op::BOOL_COMPLEMENT,
        INC => op::INC,
        DEC => op::DEC,
        BIT_OR => op::BIT_OR,
        BIT_XOR => op::BIT_XOR,
        BIT_AND => op::BIT_AND,
        EQUAL => op::EQUAL,
        NOT_EQUAL => op::NOT_EQUAL,
        SUPER_EQUAL => op::SUPER_EQUAL,
        SUPER_NOT_EQUAL => op::SUPER_NOT_EQUAL,
        LESS_THAN => op::LESS_THAN,
        GREATER_THAN => op::GREATER_THAN,
        LESS_THAN_OR_EQUAL => op::LESS_THAN_OR_EQUAL,
        GREATER_THAN_OR_EQUAL => op::GREATER_THAN_OR_EQUAL,
        SHIFT_LEFT => op::SHIFT_LEFT,
        SHIFT_RIGHT => op::SHIFT_RIGHT,
        PLUS => op::PLUS,
        MINUS => op::MINUS,
        MULTIPLY => op::MULTIPLY,
        DIVIDE => op::DIVIDE,
        MODULUS => op::MODULUS,
        SIZE_OF => op::SIZE_OF,
        IS_DEFINED => op::IS_DEFINED,
        VECTOR => op::VECTOR,
        VECTOR_SCALE => op::VECTOR_SCALE,
        FIRST_ARRAY_KEY => op::FIRST_ARRAY_KEY,
        NEXT_ARRAY_KEY => op::NEXT_ARRAY_KEY,
        GET_TIME => op::GET_TIME,
        WAIT => op::WAIT,
        REAL_WAIT => op::REAL_WAIT,
        WAIT_TILL_FRAME_END => op::WAIT_TILL_FRAME_END,
        WAIT_TILL => op::WAIT_TILL,
        NOTIFY => op::NOTIFY,
        END_ON => op::END_ON,
        _ => return None,
    })
}

fn t6_int(o: T7Op) -> Option<u8> {
    use T7Op::*;
    Some(match o {
        GET_INTEGER => op::GET_INTEGER,
        EVAL_LOCAL_VARIABLE_CACHED => op::EVAL_LOCAL_VARIABLE_CACHED,
        EVAL_LOCAL_VARIABLE_REF_CACHED => op::EVAL_LOCAL_VARIABLE_REF_CACHED,
        // (Both take the next value off the stack unless it is the call's
        // or the waittill's marker: `waittill(e, a, b)` assigns with the
        // first; Black Ops II's linker form for that is its waittill one.)
        SAFE_SET_VARIABLE_FIELD_CACHED | SAFE_SET_WAITTILL_VARIABLE_FIELD_CACHED => op::SAFE_SET_WAITTILL_VARIABLE_FIELD_CACHED,
        SCRIPT_FUNCTION_CALL_POINTER => op::SCRIPT_FUNCTION_CALL_POINTER,
        SCRIPT_METHOD_CALL_POINTER => op::SCRIPT_METHOD_CALL_POINTER,
        SCRIPT_THREAD_CALL_POINTER => op::SCRIPT_THREAD_CALL_POINTER,
        SCRIPT_METHOD_THREAD_CALL_POINTER => op::SCRIPT_METHOD_THREAD_CALL_POINTER,
        WAIT_TILL_MATCH => op::WAIT_TILL_MATCH,
        _ => return None,
    })
}

fn t6_call(o: T7Op) -> Option<u8> {
    use T7Op::*;
    Some(match o {
        CALL_BUILTIN => op::CALL_BUILTIN,
        CALL_BUILTIN_METHOD => op::CALL_BUILTIN_METHOD,
        SCRIPT_FUNCTION_CALL => op::SCRIPT_FUNCTION_CALL,
        SCRIPT_METHOD_CALL => op::SCRIPT_METHOD_CALL,
        SCRIPT_THREAD_CALL => op::SCRIPT_THREAD_CALL,
        SCRIPT_METHOD_THREAD_CALL => op::SCRIPT_METHOD_THREAD_CALL,
        GET_FUNCTION | GET_API_FUNCTION => op::GET_FUNCTION,
        _ => return None,
    })
}

fn t6_jump(o: T7Op) -> Option<u8> {
    use T7Op::*;
    Some(match o {
        JUMP_ON_FALSE => op::JUMP_ON_FALSE,
        JUMP_ON_TRUE => op::JUMP_ON_TRUE,
        JUMP_ON_FALSE_EXPR => op::JUMP_ON_FALSE_EXPR,
        JUMP_ON_TRUE_EXPR => op::JUMP_ON_TRUE_EXPR,
        JUMP => op::JUMP,
        _ => return None,
    })
}

fn field(name: u32) -> String {
    display(name).to_ascii_lowercase()
}

/// The main namespace of an object: the one most of its exports have.
fn main_namespace(o: &T7Object) -> Option<u32> {
    let mut count: FxHashMap<u32, usize> = FxHashMap::default();
    for f in &o.functions {
        *count.entry(f.export.ns).or_default() += 1;
    }
    count.into_iter().max_by_key(|&(ns, n)| (n, std::cmp::Reverse(ns))).map(|(ns, _)| ns)
}

/// Translate the server scripts of `objects` (later objects of one name
/// replace earlier ones; `.csc` client scripts are left out).
pub fn translate(objects: &[T7Object]) -> Result<(Vec<ScriptObject>, Summary), String> {
    translate_ext(objects, ".gsc")
}

/// The same for the objects whose name ends in `ext`: `.csc` for the
/// client scripts (their own program, run by the client side).
pub fn translate_ext(objects: &[T7Object], ext: &str) -> Result<(Vec<ScriptObject>, Summary), String> {
    let mut by_name: FxHashMap<&str, usize> = FxHashMap::default();
    let mut order = Vec::new();
    for (i, o) in objects.iter().enumerate() {
        if !o.name.to_ascii_lowercase().ends_with(ext) {
            continue;
        }
        if by_name.insert(o.name.as_str(), i).is_none() {
            order.push(o.name.as_str());
        }
    }
    let server: Vec<&T7Object> = order.iter().map(|n| &objects[by_name[n]]).collect();
    // Every export's script and name.
    let mut names: Vec<Vec<String>> = Vec::with_capacity(server.len());
    let mut exports: FxHashMap<(u32, u32), (usize, usize)> = FxHashMap::default();
    for (si, o) in server.iter().enumerate() {
        let main = main_namespace(o);
        let mut used = FxHashSet::default();
        let mut fnames = Vec::with_capacity(o.functions.len());
        for (fi, f) in o.functions.iter().enumerate() {
            let e = &f.export;
            let mut n = if Some(e.ns) == main { display(e.name) } else { format!("{}::{}", display(e.ns), display(e.name)) };
            if !used.insert(n.to_ascii_lowercase()) {
                n = format!("{n}@{:x}", e.address);
                used.insert(n.to_ascii_lowercase());
            }
            fnames.push(n);
            exports.entry((e.ns, e.name)).or_insert((si, fi));
        }
        names.push(fnames);
    }
    let mut summary = Summary { scripts: server.len(), ..Default::default() };
    let mut out = Vec::with_capacity(server.len());
    for o in &server {
        // Each import as Black Ops II's linker takes it.
        let imports: Vec<T6Import> = o
            .imports
            .iter()
            .map(|imp| {
                if let Some(&(si, fi)) = exports.get(&(imp.ns, imp.name)) {
                    T6Import { ns: link_name(&server[si].name), name: names[si][fi].clone(), params: imp.params, flags: imp.flags }
                } else if imp.flags & 0x20 != 0 || builtin(imp.name).is_some() {
                    let name = display(imp.name);
                    summary.builtins.insert(name.clone());
                    T6Import { ns: String::new(), name, params: imp.params, flags: imp.flags }
                } else {
                    // Another mode's script: an unbound `ns::name` builtin.
                    let ns = display(imp.ns);
                    summary.missing.insert(format!("{ns}::{}", display(imp.name)));
                    T6Import { ns: format!("t7/{ns}"), name: display(imp.name), params: imp.params, flags: imp.flags }
                }
            })
            .collect();
        let si = out.len();
        let mut functions = Vec::with_capacity(o.functions.len());
        for (fi, f) in o.functions.iter().enumerate() {
            let mut code = Vec::with_capacity(f.code.len() + 8);
            let mut vararg = false;
            let mut byref = 0u64;
            for (at, insn) in &f.code {
                let at = *at;
                let bad = || format!("{}: {} at {at:#x}: no Black Ops II form for {insn:?}", o.name, names[si][fi]);
                match insn {
                    Insn::Plain(p) => code.push((at, T6::Plain(t6_plain(*p).ok_or_else(bad)?))),
                    Insn::Int(p, v) => code.push((at, T6::Int(t6_int(*p).ok_or_else(bad)?, *v))),
                    Insn::U64(_) => return Err(bad()),
                    Insn::Float(v) => code.push((at, T6::Float(*v))),
                    Insn::Vector(v) => code.push((at, T6::Vector(*v))),
                    Insn::Hash(h) => code.push((at, T6::Hash(*h))),
                    Insn::Str(p, s) => code.push((at, T6::Str(if *p == T7Op::GET_ISTRING { op::GET_ISTRING } else { op::GET_STRING }, s.clone()))),
                    Insn::Field(p, h) => {
                        use T7Op::*;
                        let (object, eval) = match p {
                            EVAL_FIELD_VARIABLE => (None, op::EVAL_FIELD_VARIABLE),
                            EVAL_FIELD_VARIABLE_REF => (None, op::EVAL_FIELD_VARIABLE_REF),
                            CLEAR_FIELD_VARIABLE => (None, op::CLEAR_FIELD_VARIABLE),
                            EVAL_SELF_FIELD_VARIABLE => (Some(op::GET_SELF_OBJECT), op::EVAL_FIELD_VARIABLE),
                            EVAL_SELF_FIELD_VARIABLE_REF => (Some(op::GET_SELF_OBJECT), op::EVAL_FIELD_VARIABLE_REF),
                            EVAL_LEVEL_FIELD_VARIABLE => (Some(op::GET_LEVEL_OBJECT), op::EVAL_FIELD_VARIABLE),
                            EVAL_LEVEL_FIELD_VARIABLE_REF => (Some(op::GET_LEVEL_OBJECT), op::EVAL_FIELD_VARIABLE_REF),
                            _ => return Err(bad()),
                        };
                        match object {
                            // (The field goes at the next odd address: no
                            // instruction or jump target is odd.)
                            Some(obj) => {
                                code.push((at, T6::Plain(obj)));
                                code.push((at + 1, T6::Str(eval, field(*h))));
                            }
                            None => code.push((at, T6::Str(eval, field(*h)))),
                        }
                    }
                    Insn::Locals(vars) => {
                        // (The `...` parameter: the last parameter, flagged.)
                        vararg |= vars.iter().any(|v| v.1 & 2 != 0);
                        for (i, v) in vars.iter().enumerate().take(64) {
                            if v.1 & 1 != 0 {
                                byref |= 1 << i;
                            }
                        }
                        code.push((at, T6::Locals(vars.iter().map(|v| display(v.0)).collect())));
                    }
                    Insn::Call(p, _, i) => {
                        let imp = imports.get(*i as usize).cloned().ok_or_else(bad)?;
                        if *p == T7Op::GET_UNRESOLVED_FUNCTION {
                            code.push((at, T6::Plain(op::GET_UNDEFINED)));
                        } else {
                            code.push((at, T6::Call(t6_call(*p).ok_or_else(bad)?, imp)));
                        }
                    }
                    Insn::Method(p, _, h) => {
                        let c = if *p == T7Op::CLASS_FUNCTION_THREAD_CALL { op::CLASS_FUNCTION_THREAD_CALL } else { op::CLASS_FUNCTION_CALL };
                        code.push((at, T6::Int(c, *h as i32)));
                    }
                    Insn::New(h) => code.push((at, T6::Str(op::NEW, field(*h)))),
                    Insn::Anim { tree, anim } => code.push((at, T6::Anim { tree: tree.clone(), anim: anim.clone() })),
                    Insn::AnimTree(t) => code.push((at, T6::AnimTree(t.clone()))),
                    Insn::Jump(p, to) => code.push((at, T6::Jump(t6_jump(*p).ok_or_else(bad)?, *to))),
                    Insn::DevBlock { to } => code.push((at, T6::DevBlock { to: *to })),
                    Insn::Switch { cases, end } => {
                        let cases = cases
                            .iter()
                            .map(|(v, target)| Case {
                                value: match v {
                                    CaseValue::Int(i) => T6Case::Int(*i),
                                    CaseValue::Hash(h) => T6Case::Int(*h as i32),
                                    CaseValue::Str(s) => T6Case::Str(s.clone()),
                                    CaseValue::Default => T6Case::Default,
                                },
                                target: *target,
                            })
                            .collect();
                        code.push((at, T6::Switch { cases, end: *end }));
                    }
                    Insn::EndSwitch { end } => code.push((at, T6::EndSwitch { end: *end })),
                }
            }
            functions.push(DecodedFunction {
                name: names[si][fi].clone(),
                params: f.export.params,
                flags: f.export.flags,
                vararg,
                byref,
                address: f.export.address,
                code,
            });
        }
        summary.functions += functions.len();
        let calls = o.import_sites;
        let strings = o.string_refs;
        out.push(ScriptObject { name: link_name(&o.name), includes: Vec::new(), functions, string_refs: strings, import_sites: calls });
    }
    Ok((out, summary))
}

/// The autoexec functions (export flag 2; 4 is private) of the
/// translated objects, as Black Ops III links them: a script after the
/// scripts it includes (depth first), in load order otherwise; so a class's
/// registration (a private autoexec of its script) runs before an
/// including script's autoexec makes one (`ammo_shared` makes a
/// `throttle`). `includes` are the T7 objects' include lists by name.
/// Returns (script, function name).
pub fn autoexecs(objects: &[ScriptObject], includes: &FxHashMap<String, Vec<String>>) -> Vec<(String, String)> {
    let key = |n: &str| n.to_ascii_lowercase().trim_end_matches(".gsc").trim_end_matches(".csc").to_owned();
    let index: FxHashMap<String, usize> = objects.iter().enumerate().map(|(i, o)| (key(&o.name), i)).collect();
    let mut done = vec![false; objects.len()];
    let mut out = Vec::new();
    fn visit(
        i: usize,
        objects: &[ScriptObject],
        index: &FxHashMap<String, usize>,
        includes: &FxHashMap<String, Vec<String>>,
        done: &mut Vec<bool>,
        out: &mut Vec<(String, String)>,
    ) {
        if done[i] {
            return;
        }
        done[i] = true;
        let o = &objects[i];
        for inc in includes.get(&o.name).into_iter().flatten() {
            if let Some(&j) = index.get(&inc.to_ascii_lowercase()) {
                visit(j, objects, index, includes, done, out);
            }
        }
        out.extend(o.functions.iter().filter(|f| f.flags & 2 != 0).map(|f| (o.name.clone(), f.name.clone())));
    }
    for i in 0..objects.len() {
        visit(i, objects, &index, includes, &mut done, &mut out);
    }
    out
}

/// A client script's name as the linker takes it (`.csc` objects are
/// linked as a program of their own under `.gsc` names).
fn link_name(name: &str) -> String {
    match name.strip_suffix(".csc") {
        Some(base) => format!("{base}.gsc"),
        None => name.to_owned(),
    }
}

/// The include lists of Black Ops III objects, by object name.
pub fn include_lists(objects: &[T7Object]) -> FxHashMap<String, Vec<String>> {
    objects.iter().map(|o| (link_name(&o.name), o.includes.clone())).collect()
}
