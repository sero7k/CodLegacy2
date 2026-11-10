//! Facts read off a program without lowering it: how many samples it takes,
//! and which constant offsets it reads from each buffer. Used to learn what
//! the engine's records hold (the material record, the light grid buffers).

use alloc::collections::BTreeMap;
use alloc::string::String;

use crate::ir::{Const, Inst, Module, ValueKind};
use crate::meta::{Class, Entry};

/// One resource range, by its register: `(class, space, lower)`.
pub type RangeKey = (Class, u32, u32);

/// What a program does, in counts.
#[derive(Clone, Debug, Default)]
pub struct Profile {
    /// `dx.op.sample*` calls (all variants).
    pub samples: u32,
    /// `dx.op.textureLoad` calls.
    pub texture_loads: u32,
    /// Per buffer range: the constant byte or element offsets added to a
    /// variable index in its loads (`bufferLoad`, `rawBufferLoad`), with how
    /// often each is read. A load whose index is a plain constant is
    /// recorded with that constant.
    pub buffer_reads: BTreeMap<RangeKey, BTreeMap<i64, u32>>,
    /// Per buffer range: loads whose index has no constant part.
    pub buffer_reads_plain: BTreeMap<RangeKey, u32>,
    /// `dx.op` names called, with counts.
    pub ops: BTreeMap<String, u32>,
}

struct Walk<'a> {
    m: &'a Module,
    body: &'a crate::ir::Body,
    defs: BTreeMap<u32, &'a Inst>,
}

impl Walk<'_> {
    fn value(&self, v: u32) -> Option<&crate::ir::Value> {
        self.m.values.get(v as usize).or_else(|| self.body.local(v))
    }

    fn const_int(&self, v: u32) -> Option<i64> {
        match &self.value(v)?.kind {
            ValueKind::Constant(Const::Int(n)) => Some(*n),
            ValueKind::Constant(Const::Null) => Some(0),
            _ => None,
        }
    }

    fn def(&self, v: u32) -> Option<&Inst> {
        self.defs.get(&v).copied()
    }

    /// The constants summed along an index's outermost add / or chain, and
    /// whether anything else (a variable) is in it.
    fn const_part(&self, v: u32, depth: u32) -> (i64, bool) {
        if let Some(c) = self.const_int(v) {
            return (c, false);
        }
        if depth > 6 {
            return (0, true);
        }
        match self.def(v) {
            Some(Inst::Binop { op: 0 | 11, lhs, rhs }) => {
                let (a, av) = self.const_part(*lhs, depth + 1);
                let (b, bv) = self.const_part(*rhs, depth + 1);
                (a.wrapping_add(b), av || bv)
            }
            _ => (0, true),
        }
    }
}

/// Walks the entry function and counts what it does.
pub fn profile(m: &Module, entry: &Entry) -> Profile {
    let mut out = Profile::default();
    let Some(f) = m.functions.iter().find(|f| f.body.is_some() && f.name == entry.name) else {
        return out;
    };
    let Some(body) = f.body.as_ref() else { return out };
    let defs = body.insts.iter().filter(|(id, _)| *id != u32::MAX).map(|(id, i)| (*id, i)).collect();
    let w = Walk { m, body, defs };
    // value id -> range key, from createHandle(57, class, range id, index, nonuniform)
    let mut handles: BTreeMap<u32, RangeKey> = BTreeMap::new();
    for (id, inst) in &body.insts {
        let Inst::Call { callee, args } = inst else { continue };
        let Some(ValueKind::Function { index }) = w.value(*callee).map(|v| &v.kind) else { continue };
        let name = m.functions[*index].name.as_str();
        let Some(rest) = name.strip_prefix("dx.op.") else { continue };
        *out.ops.entry(rest.into()).or_default() += 1;
        let op = args.first().and_then(|&a| w.const_int(a)).unwrap_or(-1);
        match op {
            57 => {
                let (Some(class), Some(rid)) = (args.get(1).and_then(|&a| w.const_int(a)), args.get(2).and_then(|&a| w.const_int(a))) else {
                    continue;
                };
                let class = match class {
                    0 => Class::Srv,
                    1 => Class::Uav,
                    2 => Class::Cbv,
                    _ => Class::Sampler,
                };
                if let Some(r) = entry.ranges.iter().find(|r| r.class == class && i64::from(r.id) == rid) {
                    handles.insert(*id, (class, r.space, r.lower));
                }
            }
            68 | 139 => {
                let Some(key) = args.get(1).and_then(|h| handles.get(h)).copied() else { continue };
                let Some(&ix) = args.get(2) else { continue };
                let (c, var) = w.const_part(ix, 0);
                if var && c == 0 {
                    *out.buffer_reads_plain.entry(key).or_default() += 1;
                } else {
                    *out.buffer_reads.entry(key).or_default().entry(c).or_default() += 1;
                }
            }
            _ => {}
        }
        if rest.starts_with("sample") {
            out.samples += 1;
        } else if rest.starts_with("textureLoad") {
            out.texture_loads += 1;
        }
    }
    out
}
