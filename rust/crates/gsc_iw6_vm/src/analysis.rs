//! Static analysis of a loaded script set: which builtins the scripts call, which functions a
//! start point can reach, which literal strings are handed to a given builtin. World-independent
//! (the sim's census and the assets lane both use it).

use std::collections::{BTreeMap, BTreeSet, HashSet, VecDeque};

use gsc_iw6::Operand;

use crate::scripts::ScriptSet;
use crate::value::FuncRef;

/// Where a builtin is called from.
#[derive(Clone, Debug, Default)]
pub struct BuiltinUse {
    /// Call sites (a pointer take counts as one).
    pub calls: u64,
    /// Names of the scripts that call it.
    pub scripts: BTreeSet<String>,
}

/// Every builtin the scripts call: `(is a method, id)` -> uses. Methods carry the 0x8000 bias
/// as scripts encode it.
pub fn builtin_uses(set: &ScriptSet) -> BTreeMap<(bool, u16), BuiltinUse> {
    let mut out: BTreeMap<(bool, u16), BuiltinUse> = BTreeMap::new();
    for (_, p) in set.iter() {
        for ins in &p.script.instrs {
            if !matches!(ins.op, 0x1a..=0x20 | 0x83 | 0x99 | 0xa8..=0xae) {
                continue;
            }
            let Some(Operand::Builtin(id)) = ins.operands.iter().find(|o| matches!(o, Operand::Builtin(_))) else { continue };
            let method = *id >= 0x8000 || matches!(ins.op, 0x99 | 0xa8..=0xae);
            let u = out.entry((method, *id)).or_default();
            u.calls += 1;
            u.scripts.insert(p.name.to_string());
        }
    }
    out
}

/// `builtin_uses` over the functions of `within` only (what a start point can reach).
pub fn builtin_uses_within(set: &ScriptSet, within: &HashSet<(u32, u32)>) -> BTreeMap<(bool, u16), BuiltinUse> {
    let mut out: BTreeMap<(bool, u16), BuiltinUse> = BTreeMap::new();
    for (si, p) in set.iter() {
        for (fi, func) in p.script.functions.iter().enumerate() {
            if !within.contains(&(si, fi as u32)) {
                continue;
            }
            for ins in p.script.body(func) {
                if !matches!(ins.op, 0x1a..=0x20 | 0x83 | 0x99 | 0xa8..=0xae) {
                    continue;
                }
                let Some(Operand::Builtin(id)) = ins.operands.iter().find(|o| matches!(o, Operand::Builtin(_))) else { continue };
                let method = *id >= 0x8000 || matches!(ins.op, 0x99 | 0xa8..=0xae);
                let u = out.entry((method, *id)).or_default();
                u.calls += 1;
                u.scripts.insert(p.name.to_string());
            }
        }
    }
    out
}

/// Every function reachable from `entries` through calls, threads and function references
/// (local and far). Function pointers stored in variables are followed where the reference is
/// taken, so this is an over-approximation of what runs.
pub fn reachable(set: &ScriptSet, entries: &[FuncRef]) -> HashSet<(u32, u32)> {
    let mut seen: HashSet<(u32, u32)> = HashSet::new();
    let mut queue: VecDeque<(u32, u32)> = VecDeque::new();
    for e in entries {
        if seen.insert((e.script, e.func)) {
            queue.push_back((e.script, e.func));
        }
    }
    while let Some((s, f)) = queue.pop_front() {
        let Some(p) = set.get(s) else { continue };
        let Some(func) = p.script.functions.get(f as usize) else { continue };
        for ins in p.script.body(func) {
            for o in &ins.operands {
                let target = match o {
                    Operand::Func(off) => p.func_at(*off).map(|f| (s, f)),
                    Operand::Far { file, func } => set.resolve(file, func).map(|r| (r.script, r.func)),
                    _ => None,
                };
                if let Some(t) = target
                    && seen.insert(t)
                {
                    queue.push_back(t);
                }
            }
        }
    }
    seen
}

/// The string literals scripts pass as the first argument of builtin `id`: a `get_string`
/// right before the call (`loadfx("fx/...")`, `precachemodel("...")`). Only functions in
/// `within` are looked at (all when `None`).
pub fn literal_args(set: &ScriptSet, id: u16, within: Option<&HashSet<(u32, u32)>>) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    for (si, p) in set.iter() {
        for (fi, func) in p.script.functions.iter().enumerate() {
            if within.is_some_and(|w| !w.contains(&(si, fi as u32))) {
                continue;
            }
            let body = p.script.body(func);
            for (k, ins) in body.iter().enumerate() {
                if !matches!(ins.op, 0x1a..=0x20) {
                    continue;
                }
                if !ins.operands.iter().any(|o| matches!(o, Operand::Builtin(b) if *b == id)) {
                    continue;
                }
                // The arguments are pushed last to first: the first argument is the last push.
                if k > 0
                    && body[k - 1].op == 0x53
                    && let Some(Operand::Str(s)) = body[k - 1].operands.first()
                {
                    out.insert(s.clone());
                }
            }
        }
    }
    out
}

/// Every string literal (`get_string`) in the functions of `within` (all when `None`). Scripts
/// name effects and models in literals that reach `loadfx` / `setmodel` through variables, so
/// the assets lane matches these against what the zones hold.
pub fn string_literals(set: &ScriptSet, within: Option<&HashSet<(u32, u32)>>) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    for (si, p) in set.iter() {
        for (fi, func) in p.script.functions.iter().enumerate() {
            if within.is_some_and(|w| !w.contains(&(si, fi as u32))) {
                continue;
            }
            for ins in p.script.body(func) {
                if ins.op == 0x53
                    && let Some(Operand::Str(s)) = ins.operands.first()
                {
                    out.insert(s.clone());
                }
            }
        }
    }
    out
}
