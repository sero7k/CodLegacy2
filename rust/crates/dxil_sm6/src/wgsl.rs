//! Lowering of a graphics DXIL program (vertex or pixel) to WGSL text.
//!
//! The input is the reader's [`Module`]; the output is one WGSL module with
//! an entry point `main`, plus the binding table the caller needs to fill in
//! (`group`, `binding`, class, space, lower bound). Values are function-scope
//! `var`s, handles are resolved at translation time, control flow is
//! rebuilt from the CFG (`if`/`switch` regions closed at the immediate
//! post-dominator; loops are not handled and reported). Operations the
//! translator does not know are reported by name, never skipped.
//!
//! Binding numbers: `group` is the register space; `binding` is the lower
//! bound plus `0` for constant buffers, `256` for shader resources, `512` for
//! samplers and `768` for unordered access views. Unbounded or large ranges
//! become `binding_array`.

use alloc::collections::{BTreeMap, BTreeSet};
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;
use core::fmt::Write;

use crate::ir::{Body, Const, Inst, Module, Type, ValueKind};
use crate::meta::{self, Class, Entry, Range, SigElement, Stage};

/// Why a program was not lowered.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Error {
    /// A construct the translator has no mapping for (the text names it).
    Unsupported(String),
    /// The module is not what a graphics program looks like.
    Malformed(&'static str),
}

impl core::fmt::Display for Error {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Unsupported(s) => write!(f, "unsupported: {s}"),
            Self::Malformed(s) => write!(f, "malformed: {s}"),
        }
    }
}

/// One binding of the lowered module.
#[derive(Clone, Debug)]
pub struct Binding {
    pub range: Range,
    pub group: u32,
    pub binding: u32,
    /// The WGSL global's name.
    pub name: String,
    /// True when the range is declared as a `binding_array`.
    pub array: bool,
    /// A texture used with a comparison sampler (declared as a depth texture)
    /// or a sampler used for comparison.
    pub depth_or_cmp: bool,
}

/// A lowered program.
#[derive(Clone, Debug)]
pub struct Translation {
    pub wgsl: String,
    pub stage: Stage,
    pub bindings: Vec<Binding>,
    pub inputs: Vec<SigElement>,
    pub outputs: Vec<SigElement>,
    /// `(element id, WGSL location)` of the varyings (pixel inputs and vertex
    /// outputs). The location is the element's rank in signature order, so
    /// two stages are linked by matching `(name, semantic_index)`, not by
    /// location.
    pub input_locations: Vec<(u32, u32)>,
    pub output_locations: Vec<(u32, u32)>,
}

type Res<T> = Result<T, Error>;

fn unsup<T>(s: impl Into<String>) -> Res<T> {
    Err(Error::Unsupported(s.into()))
}

/// How a value is held in WGSL.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Sc {
    Bool,
    U32,
    F32,
    /// A four-component result (`ResRet`, `CBufRet`, `Dimensions`): a
    /// `vec4`; the flag says float components.
    Agg(bool),
    Ptr,
    Void,
}

#[derive(Clone, Debug)]
struct Handle {
    range: usize,
    /// Index into the range (absolute register), as an expression; `None`
    /// when the range is a single descriptor.
    index: Option<String>,
}

const BASE_SRV: u32 = 256;
const BASE_SAMPLER: u32 = 512;
const BASE_UAV: u32 = 768;

struct Cx<'a> {
    m: &'a Module,
    body: &'a Body,
    entry: Entry,
    handles: BTreeMap<u32, Handle>,
    /// Per range: used as a depth texture / comparison sampler.
    depth: BTreeSet<usize>,
    /// Per value: which components of a four-wide result are read.
    used: BTreeMap<u32, u8>,
    /// Output text of the body function.
    out: String,
    indent: usize,
    /// Blocks.
    succ: Vec<Vec<usize>>,
    ipdom: Vec<usize>,
    loop_of: BTreeMap<usize, usize>,
    loops: Vec<LoopCtx>,
    switch_depth: usize,
    uses_sample_index: bool,
    nblocks: usize,
    emitted: usize,
    used_ranges: core::cell::RefCell<BTreeSet<usize>>,
    raw_helpers: BTreeSet<usize>,
    globals_decl: core::cell::RefCell<String>,
    globals_done: core::cell::RefCell<BTreeSet<u32>>,
    pointers: BTreeMap<u32, String>,
    pstore: BTreeMap<u32, bool>,
    allocas: Vec<(u32, String)>,
}

fn is_unbounded(r: &Range) -> bool {
    r.size > 1
}

fn range_name(r: &Range, idx: usize) -> String {
    let c = match r.class {
        Class::Srv => "t",
        Class::Uav => "u",
        Class::Cbv => "b",
        Class::Sampler => "s",
    };
    format!("{c}{}_{}_{idx}", r.space, r.lower)
}

fn binding_number(r: &Range) -> u32 {
    r.lower
        + match r.class {
            Class::Cbv => 0,
            Class::Srv => BASE_SRV,
            Class::Sampler => BASE_SAMPLER,
            Class::Uav => BASE_UAV,
        }
}

/// An expression of one scalar class reinterpreted as the other (`from`
/// and `to` say whether each side is a float).
fn conv_bits(e: &str, from: bool, to: bool) -> String {
    match (from, to) {
        (false, true) => format!("d_bitsf({e})"),
        (true, false) => format!("d_bitsu({e})"),
        _ => e.to_string(),
    }
}

fn f32_lit(bits: u32) -> String {
    let f = f32::from_bits(bits);
    if bits == 0x8000_0000 {
        return "(-0.0)".into();
    }
    if f.is_finite() {
        let s = format!("{f:?}");
        // `1e-7` and `1.5e20` are valid WGSL decimals; make sure there is a
        // dot or exponent so it is a float literal, and paren negatives.
        let s = if s.contains('.') || s.contains('e') { s } else { format!("{s}.0") };
        if f < 0.0 { format!("({s})") } else { s }
    } else {
        format!("d_bitsf({bits}u)")
    }
}

fn f16_to_f32(h: u16) -> f32 {
    let s = u32::from(h >> 15) << 31;
    let e = u32::from((h >> 10) & 31);
    let f = u32::from(h & 1023);
    let bits = if e == 0 {
        if f == 0 {
            s
        } else {
            // Subnormal half.
            let mut e2 = 127 - 15 + 1;
            let mut f2 = f;
            while f2 & 1024 == 0 {
                f2 <<= 1;
                e2 -= 1;
            }
            s | (e2 << 23) | ((f2 & 1023) << 13)
        }
    } else if e == 31 {
        s | 0x7f80_0000 | (f << 13)
    } else {
        s | ((e + 127 - 15) << 23) | (f << 13)
    };
    f32::from_bits(bits)
}

impl<'a> Cx<'a> {
    fn value(&self, v: u32) -> Option<&'a crate::ir::Value> {
        self.m.values.get(v as usize).or_else(|| self.body.local(v))
    }

    fn ty_of(&self, v: u32) -> u32 {
        self.value(v).map_or(u32::MAX, |x| x.ty)
    }

    fn sc(&self, ty: u32) -> Res<Sc> {
        Ok(match self.m.types.get(ty as usize) {
            Some(Type::Void) => Sc::Void,
            Some(Type::Int(1)) => Sc::Bool,
            Some(Type::Int(8 | 16 | 32)) => Sc::U32,
            Some(Type::Int(w)) => return unsup(format!("i{w}")),
            Some(Type::Float | Type::Half) => Sc::F32,
            Some(Type::Double) => return unsup("double"),
            Some(Type::Pointer { .. }) => Sc::Ptr,
            Some(Type::Struct { name, fields }) => {
                let first = fields.first().copied().unwrap_or(u32::MAX);
                let fl = matches!(
                    self.m.types.get(first as usize),
                    Some(Type::Float | Type::Half)
                );
                let ok = fields.len() >= 4
                    && fields.len() <= 5
                    && fields[..4].iter().all(|&f| {
                        let t = self.m.types.get(f as usize);
                        if fl {
                            matches!(t, Some(Type::Float | Type::Half))
                        } else {
                            matches!(t, Some(Type::Int(32)))
                        }
                    });
                if !ok {
                    return unsup(format!("struct {name}"));
                }
                Sc::Agg(fl)
            }
            _ => return unsup(format!("type #{ty}")),
        })
    }

    fn wty(&self, ty: u32) -> Res<String> {
        Ok(match self.m.types.get(ty as usize) {
            Some(Type::Array { len, of }) => format!("array<{}, {len}>", self.wty(*of)?),
            _ => match self.sc(ty)? {
                Sc::Bool => "bool".into(),
                Sc::U32 => "u32".into(),
                Sc::F32 => "f32".into(),
                Sc::Agg(true) => "vec4<f32>".into(),
                Sc::Agg(false) => "vec4<u32>".into(),
                Sc::Ptr => return unsup("pointer variable"),
                Sc::Void => return unsup("void value"),
            },
        })
    }

    fn zero(&self, ty: u32) -> Res<String> {
        Ok(match self.sc(ty)? {
            Sc::Bool => "false".into(),
            Sc::U32 => "0u".into(),
            Sc::F32 => "0.0".into(),
            Sc::Agg(true) => "vec4<f32>()".into(),
            Sc::Agg(false) => "vec4<u32>()".into(),
            _ => return unsup("zero of a pointer"),
        })
    }

    fn const_int(&self, v: u32) -> Option<i64> {
        match &self.value(v)?.kind {
            ValueKind::Constant(Const::Int(n)) => Some(*n),
            ValueKind::Constant(Const::Null) => Some(0),
            _ => None,
        }
    }

    /// An operand as an expression of its own scalar class.
    fn ex(&self, v: u32) -> Res<String> {
        let Some(val) = self.value(v) else {
            return Err(Error::Malformed("operand out of range"));
        };
        match &val.kind {
            ValueKind::Constant(c) => {
                let width = match self.m.types.get(val.ty as usize) {
                    Some(Type::Int(w)) => *w,
                    _ => 32,
                };
                match (self.sc(val.ty)?, c) {
                    (Sc::Bool, Const::Int(n)) => Ok(if *n != 0 { "true" } else { "false" }.into()),
                    (Sc::Bool, _) => Ok("false".into()),
                    (Sc::U32, Const::Int(n)) => {
                        let mask: u64 = if width >= 32 { 0xffff_ffff } else { (1u64 << width) - 1 };
                        Ok(format!("{}u", (*n as u64) & mask))
                    }
                    (Sc::F32, Const::Float(b)) => Ok(match self.m.types.get(val.ty as usize) {
                        Some(Type::Half) => f32_lit(f16_to_f32(*b as u16).to_bits()),
                        _ => f32_lit(*b as u32),
                    }),
                    (Sc::F32, Const::Int(n)) => Ok(f32_lit(*n as u32)),
                    (_, Const::Null | Const::Undef) => self.zero(val.ty),
                    (_, other) => unsup(format!("constant {other:?}")),
                }
            }
            ValueKind::Global { init } => self.global(v, *init),
            ValueKind::Function { .. } => unsup("function value"),
            _ => {
                if let Some(p) = self.pointers.get(&v) {
                    return Ok(p.clone());
                }
                Ok(format!("v{v}"))
            }
        }
    }

    /// A module-level constant table, declared on first use.
    fn global(&self, v: u32, init: Option<u32>) -> Res<String> {
        let name = format!("g{v}");
        if self.globals_done.borrow().contains(&v) {
            return Ok(name);
        }
        let val = self.value(v).ok_or(Error::Malformed("global"))?;
        let Some(Type::Pointer { to, space }) = self.m.types.get(val.ty as usize) else {
            return unsup("global that is not a pointer");
        };
        let Some(Type::Array { len, of }) = self.m.types.get(*to as usize) else {
            return unsup("global that is not an array");
        };
        let et = self.wty(*of)?;
        let at = format!("array<{et}, {len}>");
        let mut decl = String::new();
        let storage = if *space == 3 { "workgroup" } else { "private" };
        if *space == 3 {
            return unsup("groupshared memory");
        }
        let init_s = match init.and_then(|i| self.value(i)).map(|x| &x.kind) {
            None | Some(ValueKind::Constant(Const::Undef | Const::Null)) => String::new(),
            Some(ValueKind::Constant(Const::Data(d))) => {
                let mut items = Vec::new();
                for &x in d {
                    items.push(match self.sc(*of)? {
                        Sc::F32 => match self.m.types.get(*of as usize) {
                            Some(Type::Half) => f32_lit(f16_to_f32(x as u16).to_bits()),
                            _ => f32_lit(x as u32),
                        },
                        Sc::Bool => (if x != 0 { "true" } else { "false" }).to_string(),
                        _ => format!("{}u", x as u32),
                    });
                }
                format!(" = {at}({})", items.join(", "))
            }
            Some(ValueKind::Constant(Const::Aggregate(ids))) => {
                let mut items = Vec::new();
                for &i in ids {
                    items.push(self.ex(i)?);
                }
                format!(" = {at}({})", items.join(", "))
            }
            Some(_) => return unsup("global initializer"),
        };
        let _ = writeln!(decl, "var<{storage}> {name}: {at}{init_s};");
        self.globals_decl.borrow_mut().push_str(&decl);
        self.globals_done.borrow_mut().insert(v);
        Ok(name)
    }

    fn line(&mut self, s: &str) {
        for _ in 0..self.indent {
            self.out.push_str("  ");
        }
        self.out.push_str(s);
        self.out.push('\n');
    }

    fn handle(&self, v: u32) -> Res<&Handle> {
        self.handles
            .get(&v)
            .ok_or(Error::Malformed("resource operand is not a handle"))
    }

    fn range_of(&self, h: &Handle) -> &Range {
        &self.entry.ranges[h.range]
    }

    /// The WGSL expression for the resource a handle names.
    fn res_ex(&self, h: &Handle) -> String {
        self.used_ranges.borrow_mut().insert(h.range);
        let r = &self.entry.ranges[h.range];
        let n = range_name(r, h.range);
        match &h.index {
            Some(i) if is_unbounded(r) => {
                let rel = if r.lower == 0 { i.clone() } else { format!("({i} - {}u)", r.lower) };
                format!("{n}[{rel}]")
            }
            _ => n,
        }
    }
}

// --- Control flow -----------------------------------------------------------

fn terminator_succ(i: &Inst) -> Vec<usize> {
    match i {
        Inst::Br { t, f, cond } => {
            if cond.is_some() {
                vec![*t as usize, *f as usize]
            } else {
                vec![*t as usize]
            }
        }
        Inst::Switch { default, cases, .. } => {
            let mut v = vec![*default as usize];
            v.extend(cases.iter().map(|c| c.1 as usize));
            v
        }
        _ => Vec::new(),
    }
}

/// Immediate post-dominators over the blocks plus a virtual exit (index
/// `n`). `usize::MAX` where undefined.
fn post_dominators(succ: &[Vec<usize>]) -> Vec<usize> {
    let n = succ.len();
    let exit = n;
    // Reverse graph: preds of x = blocks whose successors include x.
    let mut rsucc: Vec<Vec<usize>> = vec![Vec::new(); n + 1];
    for (b, ss) in succ.iter().enumerate() {
        if ss.is_empty() {
            rsucc[exit].push(b);
        }
        for &s in ss {
            rsucc[s].push(b);
        }
    }
    // Reverse post-order from the exit over the reverse graph.
    let mut order = Vec::new();
    let mut seen = vec![false; n + 1];
    let mut stack: Vec<(usize, usize)> = vec![(exit, 0)];
    seen[exit] = true;
    while let Some(&mut (b, ref mut k)) = stack.last_mut() {
        if *k < rsucc[b].len() {
            let s = rsucc[b][*k];
            *k += 1;
            if !seen[s] {
                seen[s] = true;
                stack.push((s, 0));
            }
        } else {
            order.push(b);
            stack.pop();
        }
    }
    order.reverse();
    let mut num = vec![usize::MAX; n + 1];
    for (i, &b) in order.iter().enumerate() {
        num[b] = i;
    }
    let mut idom = vec![usize::MAX; n + 1];
    idom[exit] = exit;
    let mut changed = true;
    while changed {
        changed = false;
        for &b in order.iter().skip(1) {
            // Predecessors in the reverse graph are the forward successors
            // (the exit for blocks with none).
            let preds: Vec<usize> = if succ[b].is_empty() { vec![exit] } else { succ[b].clone() };
            let mut new = usize::MAX;
            for &p in &preds {
                if idom[p] == usize::MAX {
                    continue;
                }
                new = if new == usize::MAX {
                    p
                } else {
                    let (mut a, mut c) = (p, new);
                    while a != c {
                        while num[a] > num[c] {
                            a = idom[a];
                        }
                        while num[c] > num[a] {
                            c = idom[c];
                        }
                    }
                    a
                };
            }
            if idom[b] != new {
                idom[b] = new;
                changed = true;
            }
        }
    }
    idom
}

impl Cx<'_> {
    fn block_range(&self, b: usize) -> (usize, usize) {
        let start = self.body.block_starts[b];
        let end = self
            .body
            .block_starts
            .get(b + 1)
            .copied()
            .unwrap_or(self.body.insts.len());
        (start, end)
    }

    fn edge(&mut self, from: usize, to: usize) -> Res<()> {
        let (s, e) = self.block_range(to);
        let mut assigns: Vec<(u32, String, bool)> = Vec::new();
        let mut phis = BTreeSet::new();
        for (id, inst) in &self.body.insts[s..e] {
            if let Inst::Phi { .. } = inst {
                phis.insert(*id);
            }
        }
        for (id, inst) in &self.body.insts[s..e] {
            let Inst::Phi { incoming, .. } = inst else { break };
            let Some((v, _)) = incoming.iter().find(|(_, b)| *b as usize == from) else {
                continue;
            };
            let rhs = self.ex(*v)?;
            assigns.push((*id, rhs, phis.contains(v)));
        }
        if assigns.iter().any(|a| a.2) && assigns.len() > 1 {
            for (id, rhs, _) in &assigns {
                let l = format!("let p{id} = {rhs};");
                self.line(&l);
            }
            for (id, _, _) in &assigns {
                let l = format!("v{id} = p{id};");
                self.line(&l);
            }
        } else {
            for (id, rhs, _) in &assigns {
                let l = format!("v{id} = {rhs};");
                self.line(&l);
            }
        }
        Ok(())
    }

    /// Emits blocks from `b` until `stop` (exclusive), a return, or a jump
    /// to the enclosing loop's header (`continue`) or exit (`break`).
    /// `entering` is set when `b` is a loop header whose `loop {` is already
    /// open.
    fn region(&mut self, b: usize, stop: usize, mut entering: bool) -> Res<()> {
        let mut cur = b;
        loop {
            if cur == stop {
                return Ok(());
            }
            if !entering {
                if let Some(l) = self.loops.last() {
                    if cur == l.header {
                        self.line("continue;");
                        return Ok(());
                    }
                    if cur == l.exit {
                        if self.switch_depth > l.switch_depth {
                            return unsup("break out of a switch");
                        }
                        self.line("break;");
                        return Ok(());
                    }
                }
                if self.loop_of.contains_key(&cur) {
                    // Entering a loop from outside.
                    let exit = self.loop_of[&cur];
                    self.line("loop {");
                    self.indent += 1;
                    self.loops.push(LoopCtx {
                        header: cur,
                        exit,
                        switch_depth: self.switch_depth,
                    });
                    self.region(cur, usize::MAX, true)?;
                    self.loops.pop();
                    self.indent -= 1;
                    self.line("}");
                    if exit == usize::MAX {
                        return Ok(());
                    }
                    cur = exit;
                    continue;
                }
            }
            entering = false;
            self.emitted += 1;
            if self.emitted > 40 * self.nblocks + 64 {
                return unsup("control flow too tangled");
            }
            let (s, e) = self.block_range(cur);
            let mut term = None;
            for k in s..e {
                let (id, inst) = &self.body.insts[k];
                match inst {
                    Inst::Phi { .. } => {}
                    Inst::Ret(_) | Inst::Unreachable | Inst::Br { .. } | Inst::Switch { .. } => {
                        term = Some(inst.clone());
                    }
                    _ => self.inst(*id, inst)?,
                }
            }
            match term {
                None | Some(Inst::Ret(_) | Inst::Unreachable) => {
                    self.line("return;");
                    return Ok(());
                }
                Some(Inst::Br { t, cond: None, .. }) => {
                    self.edge(cur, t as usize)?;
                    cur = t as usize;
                }
                Some(Inst::Br { t, f, cond: Some(c) }) => {
                    let merge = self.merge_of(cur);
                    let ce = self.ex(c)?;
                    self.line(&format!("if {ce} {{"));
                    self.indent += 1;
                    self.edge(cur, t as usize)?;
                    self.region(t as usize, merge, false)?;
                    self.indent -= 1;
                    self.line("} else {");
                    self.indent += 1;
                    self.edge(cur, f as usize)?;
                    self.region(f as usize, merge, false)?;
                    self.indent -= 1;
                    self.line("}");
                    if self.diverges_at(merge) {
                        return Ok(());
                    }
                    cur = merge;
                }
                Some(Inst::Switch { cond, default, cases }) => {
                    let merge = self.merge_of(cur);
                    let ce = self.ex(cond)?;
                    self.line(&format!("switch {ce} {{"));
                    self.indent += 1;
                    self.switch_depth += 1;
                    let mut by: BTreeMap<u32, Vec<String>> = BTreeMap::new();
                    for (cv, bb) in &cases {
                        let n = self
                            .const_int(*cv)
                            .ok_or(Error::Malformed("switch case is not a constant"))?;
                        by.entry(*bb).or_default().push(format!("{}u", n as u32));
                    }
                    for (bb, vals) in by {
                        self.line(&format!("case {}: {{", vals.join(", ")));
                        self.indent += 1;
                        self.edge(cur, bb as usize)?;
                        self.region(bb as usize, merge, false)?;
                        self.indent -= 1;
                        self.line("}");
                    }
                    self.line("default: {");
                    self.indent += 1;
                    self.edge(cur, default as usize)?;
                    self.region(default as usize, merge, false)?;
                    self.indent -= 1;
                    self.line("}");
                    self.switch_depth -= 1;
                    self.indent -= 1;
                    self.line("}");
                    if self.diverges_at(merge) {
                        return Ok(());
                    }
                    cur = merge;
                }
                Some(_) => return Err(Error::Malformed("terminator")),
            }
        }
    }

    /// The block where the arms of a branch at `b` meet again.
    fn merge_of(&self, b: usize) -> usize {
        let m = self.ipdom[b];
        if m == self.nblocks || m == usize::MAX { usize::MAX } else { m }
    }

    /// True when code after a branch whose arms meet at `merge` is dead:
    /// no merge, or the merge is the enclosing loop's header or exit (the
    /// arms already jumped there).
    fn diverges_at(&self, merge: usize) -> bool {
        merge == usize::MAX
            || self
                .loops
                .last()
                .is_some_and(|l| merge == l.header || merge == l.exit)
    }
}

struct LoopCtx {
    header: usize,
    exit: usize,
    switch_depth: usize,
}

/// Dominators (Cooper-Harvey-Kennedy) of the forward graph from block 0.
fn dominators(succ: &[Vec<usize>]) -> Vec<usize> {
    let n = succ.len();
    let mut order = Vec::new();
    let mut seen = vec![false; n];
    let mut stack: Vec<(usize, usize)> = vec![(0, 0)];
    seen[0] = true;
    while let Some(&mut (b, ref mut k)) = stack.last_mut() {
        if *k < succ[b].len() {
            let s = succ[b][*k];
            *k += 1;
            if !seen[s] {
                seen[s] = true;
                stack.push((s, 0));
            }
        } else {
            order.push(b);
            stack.pop();
        }
    }
    order.reverse();
    let mut num = vec![usize::MAX; n];
    for (i, &b) in order.iter().enumerate() {
        num[b] = i;
    }
    let mut preds = vec![Vec::new(); n];
    for (b, ss) in succ.iter().enumerate() {
        for &s in ss {
            preds[s].push(b);
        }
    }
    let mut idom = vec![usize::MAX; n];
    idom[0] = 0;
    let mut changed = true;
    while changed {
        changed = false;
        for &b in order.iter().skip(1) {
            let mut new = usize::MAX;
            for &p in &preds[b] {
                if idom[p] == usize::MAX {
                    continue;
                }
                new = if new == usize::MAX {
                    p
                } else {
                    let (mut a, mut c) = (p, new);
                    while a != c {
                        while num[a] > num[c] {
                            a = idom[a];
                        }
                        while num[c] > num[a] {
                            c = idom[c];
                        }
                    }
                    a
                };
            }
            if idom[b] != new {
                idom[b] = new;
                changed = true;
            }
        }
    }
    idom
}

/// Natural loops: header -> exit block (`usize::MAX` when the loop has no
/// single continuation). Fails on irreducible control flow.
fn find_loops(succ: &[Vec<usize>], ipdom: &[usize]) -> Res<BTreeMap<usize, usize>> {
    let n = succ.len();
    let idom = dominators(succ);
    let dom = |a: usize, mut b: usize| -> bool {
        loop {
            if a == b {
                return true;
            }
            if b == 0 || idom[b] == usize::MAX {
                return false;
            }
            b = idom[b];
        }
    };
    // Retreating edges by DFS; each must be a back edge.
    let mut state = vec![0u8; n];
    let mut latches: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
    let mut stack = vec![(0usize, 0usize)];
    state[0] = 1;
    while let Some(&mut (b, ref mut k)) = stack.last_mut() {
        if *k < succ[b].len() {
            let t = succ[b][*k];
            *k += 1;
            match state[t] {
                0 => {
                    state[t] = 1;
                    stack.push((t, 0));
                }
                1 => {
                    if !dom(t, b) {
                        return unsup("irreducible loop");
                    }
                    latches.entry(t).or_default().push(b);
                }
                _ => {}
            }
        } else {
            state[b] = 2;
            stack.pop();
        }
    }
    let mut preds = vec![Vec::new(); n];
    for (b, ss) in succ.iter().enumerate() {
        for &s in ss {
            preds[s].push(b);
        }
    }
    let mut out = BTreeMap::new();
    for (h, ls) in latches {
        let mut inl = vec![false; n];
        inl[h] = true;
        let mut work = ls.clone();
        while let Some(x) = work.pop() {
            if inl[x] {
                continue;
            }
            inl[x] = true;
            work.extend(preds[x].iter().copied());
        }
        // Blocks outside the loop that the loop jumps to; the continuation
        // is the one every other exit leads to (post-dominates them).
        let mut exits: Vec<usize> = Vec::new();
        for b in 0..n {
            if inl[b] {
                for &t in &succ[b] {
                    if !inl[t] && !exits.contains(&t) {
                        exits.push(t);
                    }
                }
            }
        }
        let leads_to = |from: usize, to: usize| -> bool {
            let mut x = from;
            for _ in 0..=n {
                if x == to {
                    return true;
                }
                if x >= n || ipdom[x] == usize::MAX || ipdom[x] == x {
                    return false;
                }
                x = ipdom[x];
            }
            false
        };
        let exit = exits
            .iter()
            .copied()
            .find(|&e| exits.iter().all(|&o| leads_to(o, e)))
            .unwrap_or(usize::MAX);
        out.insert(h, exit);
    }
    Ok(out)
}

// --- Instructions -----------------------------------------------------------

impl Cx<'_> {
    fn assign(&mut self, id: u32, rhs: &str) {
        if id == u32::MAX {
            return;
        }
        let l = format!("v{id} = {rhs};");
        self.line(&l);
    }

    fn int_ty_width(&self, v: u32) -> u32 {
        match self.m.types.get(self.ty_of(v) as usize) {
            Some(Type::Int(w)) => *w,
            _ => 32,
        }
    }

    fn is_float(&self, v: u32) -> bool {
        matches!(
            self.m.types.get(self.ty_of(v) as usize),
            Some(Type::Float | Type::Half)
        )
    }

    fn inst(&mut self, id: u32, inst: &Inst) -> Res<()> {
        match inst {
            Inst::Binop { op, lhs, rhs } => {
                let (a, b) = (self.ex(*lhs)?, self.ex(*rhs)?);
                let fl = self.is_float(*lhs);
                let bl = matches!(self.sc(self.ty_of(*lhs))?, Sc::Bool);
                let r = if fl {
                    match op {
                        0 => format!("{a} + {b}"),
                        1 => format!("{a} - {b}"),
                        2 => format!("{a} * {b}"),
                        4 => format!("{a} / {b}"),
                        6 => format!("{a} % {b}"),
                        _ => return unsup(format!("float binop {op}")),
                    }
                } else if bl {
                    match op {
                        10 => format!("{a} & {b}"),
                        11 => format!("{a} | {b}"),
                        12 => format!("{a} != {b}"),
                        _ => return unsup(format!("bool binop {op}")),
                    }
                } else {
                    match op {
                        0 => format!("{a} + {b}"),
                        1 => format!("{a} - {b}"),
                        2 => format!("{a} * {b}"),
                        3 => format!("{a} / max({b}, 1u)"),
                        4 => format!("d_u32(d_i32({a}) / d_i32(max({b}, 1u)))"),
                        5 => format!("{a} % max({b}, 1u)"),
                        6 => format!("d_u32(d_i32({a}) % d_i32(max({b}, 1u)))"),
                        7 => format!("{a} << ({b} & 31u)"),
                        8 => format!("{a} >> ({b} & 31u)"),
                        9 => format!("d_u32(d_i32({a}) >> ({b} & 31u))"),
                        10 => format!("{a} & {b}"),
                        11 => format!("{a} | {b}"),
                        12 => format!("{a} ^ {b}"),
                        _ => return unsup(format!("int binop {op}")),
                    }
                };
                self.assign(id, &r);
            }
            Inst::Cast { op, val, to } => {
                let a = self.ex(*val)?;
                let from_bool = matches!(self.sc(self.ty_of(*val))?, Sc::Bool);
                if matches!(self.sc(*to), Ok(Sc::Ptr)) {
                    // A pointer cast keeps the place; loads and stores
                    // convert the bits.
                    let p = self.ex(*val)?;
                    self.pointers.insert(id, p);
                    let f = self.store_float(*val);
                    self.pstore.insert(id, f);
                    return Ok(());
                }
                let to_w = match self.m.types.get(*to as usize) {
                    Some(Type::Int(w)) => *w,
                    _ => 32,
                };
                let from_w = self.int_ty_width(*val);
                let r = match op {
                    0 => {
                        if to_w == 1 {
                            format!("(({a} & 1u) != 0u)")
                        } else if to_w < 32 {
                            format!("({a} & {}u)", (1u32 << to_w) - 1)
                        } else {
                            a
                        }
                    }
                    1 => {
                        if from_bool {
                            format!("select(0u, 1u, {a})")
                        } else {
                            a
                        }
                    }
                    2 => {
                        if from_bool {
                            format!("select(0u, 0xffffffffu, {a})")
                        } else if from_w < 32 {
                            let sh = 32 - from_w;
                            format!("d_u32(d_i32({a} << {sh}u) >> {sh}u)")
                        } else {
                            a
                        }
                    }
                    3 => format!("u32({a})"),
                    4 => format!("d_u32(i32({a}))"),
                    5 => format!("f32({a})"),
                    6 => format!("f32(d_i32({a}))"),
                    7 | 8 => a,
                    11 => {
                        let from_f = self.is_float(*val);
                        let to_f = matches!(self.m.types.get(*to as usize), Some(Type::Float | Type::Half));
                        match (from_f, to_f) {
                            (true, false) => format!("d_bitsu({a})"),
                            (false, true) => format!("d_bitsf({a})"),
                            _ => a,
                        }
                    }
                    _ => return unsup(format!("cast {op}")),
                };
                self.assign(id, &r);
            }
            Inst::Select { cond, t, f } => {
                let (c, t, f) = (self.ex(*cond)?, self.ex(*t)?, self.ex(*f)?);
                self.assign(id, &format!("select({f}, {t}, {c})"));
            }
            Inst::Cmp { pred, lhs, rhs } => {
                let (a, b) = (self.ex(*lhs)?, self.ex(*rhs)?);
                let r = if *pred < 32 {
                    match pred {
                        0 => "false".into(),
                        1 => format!("{a} == {b}"),
                        2 => format!("{a} > {b}"),
                        3 => format!("{a} >= {b}"),
                        4 => format!("{a} < {b}"),
                        5 => format!("{a} <= {b}"),
                        6 => format!("({a} < {b} || {a} > {b})"),
                        7 => format!("({a} == {a} && {b} == {b})"),
                        8 => format!("({a} != {a} || {b} != {b})"),
                        9 => format!("({a} == {b} || {a} != {a} || {b} != {b})"),
                        10 => format!("!({a} <= {b})"),
                        11 => format!("!({a} < {b})"),
                        12 => format!("!({a} >= {b})"),
                        13 => format!("!({a} > {b})"),
                        14 => format!("{a} != {b}"),
                        15 => "true".into(),
                        _ => return unsup(format!("fcmp {pred}")),
                    }
                } else {
                    let bl = matches!(self.sc(self.ty_of(*lhs))?, Sc::Bool);
                    let (sa, sb) = (format!("d_i32({a})"), format!("d_i32({b})"));
                    match pred {
                        32 => format!("{a} == {b}"),
                        33 => format!("{a} != {b}"),
                        34 if !bl => format!("{a} > {b}"),
                        35 if !bl => format!("{a} >= {b}"),
                        36 if !bl => format!("{a} < {b}"),
                        37 if !bl => format!("{a} <= {b}"),
                        38 if !bl => format!("{sa} > {sb}"),
                        39 if !bl => format!("{sa} >= {sb}"),
                        40 if !bl => format!("{sa} < {sb}"),
                        41 if !bl => format!("{sa} <= {sb}"),
                        _ => return unsup(format!("icmp {pred}")),
                    }
                };
                self.assign(id, &r);
            }
            Inst::ExtractVal { agg, idx } => {
                let a = self.ex(*agg)?;
                let k = *idx.first().ok_or(Error::Malformed("extractvalue"))?;
                let r = match k {
                    0 => format!("{a}.x"),
                    1 => format!("{a}.y"),
                    2 => format!("{a}.z"),
                    3 => format!("{a}.w"),
                    4 => "1u".into(),
                    _ => return unsup("extractvalue index"),
                };
                self.assign(id, &r);
            }
            Inst::Alloca { ty, .. } => {
                let t = self.wty(*ty)?;
                let n = format!("a{id}");
                self.allocas.push((id, t));
                self.pointers.insert(id, n);
                let f = self.leaf_float(*ty);
                self.pstore.insert(id, f);
            }
            Inst::Gep { base, idx, .. } => {
                let b = self.ex(*base)?;
                let mut p = b;
                for (k, i) in idx.iter().enumerate() {
                    if k == 0 {
                        if self.const_int(*i) != Some(0) {
                            return unsup("pointer arithmetic");
                        }
                        continue;
                    }
                    let e = self.ex(*i)?;
                    p = format!("{p}[{e}]");
                }
                self.pointers.insert(id, p);
                let f = self.store_float(*base);
                self.pstore.insert(id, f);
            }
            Inst::Load { ptr, .. } => {
                let p = self.ex(*ptr)?;
                let sf = self.store_float(*ptr);
                let lf = self.is_float_ty(self.ty_of(id));
                self.assign(id, &conv_bits(&p, sf, lf));
            }
            Inst::Store { ptr, val } => {
                let p = self.ex(*ptr)?;
                let v = self.ex(*val)?;
                let sf = self.store_float(*ptr);
                let vf = self.is_float(*val);
                self.line(&format!("{p} = {};", conv_bits(&v, vf, sf)));
            }
            Inst::Call { callee, args } => self.call(id, *callee, args)?,
            Inst::Other(c) => return unsup(format!("record {c}")),
            Inst::Phi { .. } => {}
            other => return unsup(format!("instruction {other:?}")),
        }
        Ok(())
    }

    fn is_float_ty(&self, ty: u32) -> bool {
        matches!(self.m.types.get(ty as usize), Some(Type::Float | Type::Half))
    }

    /// True when the scalar leaf of an array/scalar type is a float.
    fn leaf_float(&self, mut ty: u32) -> bool {
        for _ in 0..6 {
            match self.m.types.get(ty as usize) {
                Some(Type::Array { of, .. } | Type::Vector { of, .. }) => ty = *of,
                Some(Type::Pointer { to, .. }) => ty = *to,
                _ => break,
            }
        }
        self.is_float_ty(ty)
    }

    /// Whether the storage a pointer points into holds floats.
    fn store_float(&self, ptr: u32) -> bool {
        if let Some(&f) = self.pstore.get(&ptr) {
            return f;
        }
        self.value(ptr).is_some_and(|v| self.leaf_float(v.ty))
    }

    fn comp_mask(&self, id: u32) -> u8 {
        self.used.get(&id).copied().unwrap_or(0xf)
    }

    #[allow(clippy::too_many_lines)]
    fn call(&mut self, id: u32, callee: u32, args: &[u32]) -> Res<()> {
        let Some(ValueKind::Function { index }) = self.value(callee).map(|v| &v.kind) else {
            return unsup("indirect call");
        };
        let fname = self.m.functions[*index].name.clone();
        let Some(rest) = fname.strip_prefix("dx.op.") else {
            return unsup(format!("call {fname}"));
        };
        let op = args
            .first()
            .and_then(|&a| self.const_int(a))
            .ok_or(Error::Malformed("dx.op without an opcode"))?;
        let a = |k: usize| -> Res<String> {
            args.get(k)
                .map(|&v| self.ex(v))
                .unwrap_or_else(|| Err(Error::Malformed("missing argument")))
        };
        let name = format!("{rest}#{op}");
        match op {
            // loadInput(inputId, row, col, vertex)
            4 => {
                let eid = self.const_int(args[1]).ok_or(Error::Malformed("input id"))? as u32;
                let col = self.const_int(args[3]).ok_or(Error::Malformed("input col"))? as usize;
                let el = self
                    .entry
                    .inputs
                    .iter()
                    .find(|e| e.id == eid)
                    .ok_or(Error::Malformed("unknown input"))?;
                let row = self.const_int(args[2]);
                let comp = ["x", "y", "z", "w"][col & 3];
                let base = if el.rows > 1 {
                    format!("in{eid}[{}]", match row { Some(r) => format!("{r}"), None => format!("{}", a(2)?) })
                } else {
                    format!("in{eid}")
                };
                let fixed = if el.sem == 13 {
                    // front face: DXIL reads it as 0 / -1
                    format!("{base}.{comp}")
                } else {
                    format!("{base}.{comp}")
                };
                self.assign(id, &fixed);
            }
            5 => {
                let eid = self.const_int(args[1]).ok_or(Error::Malformed("output id"))? as u32;
                let col = self.const_int(args[3]).ok_or(Error::Malformed("output col"))? as usize;
                let el = self
                    .entry
                    .outputs
                    .iter()
                    .find(|e| e.id == eid)
                    .ok_or(Error::Malformed("unknown output"))?;
                let comp = ["x", "y", "z", "w"][col & 3];
                let v = a(4)?;
                let target = if el.rows > 1 {
                    let r = match self.const_int(args[2]) {
                        Some(r) => format!("{r}"),
                        None => a(2)?,
                    };
                    format!("out{eid}[{r}].{comp}")
                } else {
                    format!("out{eid}.{comp}")
                };
                self.line(&format!("{target} = {v};"));
            }
            // Unary float / int
            6..=29 | 30..=34 => {
                let x = a(1)?;
                let r = match op {
                    6 => format!("abs({x})"),
                    7 => format!("clamp({x}, 0.0, 1.0)"),
                    8 => format!("({x} != {x})"),
                    9 => format!("(abs({x}) == d_bitsf(0x7f800000u))"),
                    10 => format!("(abs({x}) < d_bitsf(0x7f800000u))"),
                    12 => format!("cos({x})"),
                    13 => format!("sin({x})"),
                    14 => format!("tan({x})"),
                    15 => format!("d_acos({x})"),
                    16 => format!("d_asin({x})"),
                    17 => format!("atan({x})"),
                    18 => format!("cosh({x})"),
                    19 => format!("sinh({x})"),
                    20 => format!("tanh({x})"),
                    21 => format!("d_exp2({x})"),
                    22 => format!("fract({x})"),
                    23 => format!("d_log2({x})"),
                    24 => format!("d_sqrt({x})"),
                    25 => format!("d_inverseSqrt({x})"),
                    26 => format!("round({x})"),
                    27 => format!("floor({x})"),
                    28 => format!("ceil({x})"),
                    29 => format!("trunc({x})"),
                    30 => format!("reverseBits({x})"),
                    31 => format!("countOneBits({x})"),
                    32 => format!("firstTrailingBit({x})"),
                    33 => format!("select(0xffffffffu, 31u - firstLeadingBit({x}), {x} != 0u)"),
                    34 => {
                        let s = format!("d_i32({x})");
                        format!(
                            "select(0xffffffffu, 31u - firstLeadingBit(d_u32(select({s}, ~{s}, {s} < 0))), {x} != 0u && {x} != 0xffffffffu)"
                        )
                    }
                    _ => return unsup(name),
                };
                self.assign(id, &r);
            }
            // binary: fmax, fmin, imax, imin, umax, umin, imul/umul (low only)
            35 => self.assign(id, &format!("max({}, {})", a(1)?, a(2)?)),
            36 => self.assign(id, &format!("min({}, {})", a(1)?, a(2)?)),
            37 => self.assign(
                id,
                &format!("d_u32(max(d_i32({}), d_i32({})))", a(1)?, a(2)?),
            ),
            38 => self.assign(
                id,
                &format!("d_u32(min(d_i32({}), d_i32({})))", a(1)?, a(2)?),
            ),
            39 => self.assign(id, &format!("max({}, {})", a(1)?, a(2)?)),
            40 => self.assign(id, &format!("min({}, {})", a(1)?, a(2)?)),
            46 | 47 => self.assign(id, &format!("fma({}, {}, {})", a(1)?, a(2)?, a(3)?)),
            48 | 49 => self.assign(id, &format!("{} * {} + {}", a(1)?, a(2)?, a(3)?)),
            51 => self.assign(
                id,
                &format!(
                    "d_u32(extractBits(d_i32({}), {} & 31u, {} & 31u))",
                    a(3)?, a(2)?, a(1)?
                ),
            ),
            52 => self.assign(id, &format!("extractBits({}, {} & 31u, {} & 31u)", a(3)?, a(2)?, a(1)?)),
            53 => self.assign(
                id,
                &format!("insertBits({}, {}, {} & 31u, {} & 31u)", a(4)?, a(3)?, a(2)?, a(1)?),
            ),
            54 => self.assign(
                id,
                &format!("dot(vec2<f32>({}, {}), vec2<f32>({}, {}))", a(1)?, a(2)?, a(3)?, a(4)?),
            ),
            55 => self.assign(
                id,
                &format!(
                    "dot(vec3<f32>({}, {}, {}), vec3<f32>({}, {}, {}))",
                    a(1)?, a(2)?, a(3)?, a(4)?, a(5)?, a(6)?
                ),
            ),
            56 => self.assign(
                id,
                &format!(
                    "dot(vec4<f32>({}, {}, {}, {}), vec4<f32>({}, {}, {}, {}))",
                    a(1)?, a(2)?, a(3)?, a(4)?, a(5)?, a(6)?, a(7)?, a(8)?
                ),
            ),
            57 => {} // createHandle: resolved before emission
            58 | 59 => {
                let h = self.handle(args[1])?.clone();
                let r = self.range_of(&h).clone();
                if r.class != Class::Cbv {
                    return Err(Error::Malformed("cbuffer load from a non-cbuffer"));
                }
                let n = self.res_ex(&h);
                let float = fname.ends_with(".f32") || fname.ends_with(".f16");
                if op == 59 {
                    let reg = a(2)?;
                    let e = format!("{n}[{reg}]");
                    self.assign(id, &if float { format!("bitcast<vec4<f32>>({e})") } else { e });
                } else {
                    let off = a(2)?;
                    let e = format!("{n}[({off}) >> 4u][(({off}) >> 2u) & 3u]");
                    self.assign(id, &if float { format!("bitcast<f32>({e})") } else { e });
                }
            }
            60..=65 | 81 | 73 | 74 => self.sample(id, op, args, &fname)?,
            66 => self.tex_load(id, args)?,
            68 | 139 => self.buf_load(id, op, args)?,
            72 => {
                let h = self.handle(args[1])?.clone();
                let r = self.range_of(&h).clone();
                let n = self.res_ex(&h);
                let lvl = a(2)?;
                let dims = match r.kind {
                    1 => format!("vec4<u32>(textureDimensions({n}, {lvl}), 1u, 1u, textureNumLevels({n}))"),
                    2 | 3 => {
                        if r.kind == 3 {
                            format!("vec4<u32>(textureDimensions({n}), 1u, 1u)")
                        } else {
                            format!("vec4<u32>(textureDimensions({n}, {lvl}), 1u, textureNumLevels({n}))")
                        }
                    }
                    4 => format!("vec4<u32>(textureDimensions({n}, {lvl}), textureNumLevels({n}))"),
                    5 => format!("vec4<u32>(textureDimensions({n}, {lvl}), 1u, textureNumLevels({n}))"),
                    7 | 9 => format!(
                        "vec4<u32>(textureDimensions({n}, {lvl}), textureNumLayers({n}), textureNumLevels({n}))"
                    ),
                    11 | 12 | 10 => format!("vec4<u32>(arrayLength(&{n}), 0u, 0u, 0u)"),
                    k => return unsup(format!("getDimensions of kind {k}")),
                };
                let dims = if matches!(r.kind, 5) {
                    format!("vec4<u32>(vec2<u32>(textureDimensions({n}, {lvl})), 1u, textureNumLevels({n}))")
                } else {
                    dims
                };
                self.assign(id, &dims);
            }
            82 => {
                let c = a(1)?;
                self.line(&format!("if {c} {{ discard; }}"));
            }
            83 | 85 => self.assign(id, &format!("dpdx{}({})", if op == 83 { "Coarse" } else { "Fine" }, a(1)?)),
            84 | 86 => self.assign(id, &format!("dpdy{}({})", if op == 84 { "Coarse" } else { "Fine" }, a(1)?)),
            130 => self.assign(id, &format!("pack2x16float(vec2<f32>({}, 0.0)) & 0xffffu", a(1)?)),
            131 => self.assign(id, &format!("unpack2x16float({} & 0xffffu).x", a(1)?)),
            126 => self.assign(id, &format!("d_bitsf({})", a(1)?)),
            127 => self.assign(id, &format!("d_bitsu({})", a(1)?)),
            71 => self.assign(id, &format!("({} != 0u)", a(1)?)),
            90 => {
                self.uses_sample_index = true;
                self.assign(id, "in_sample_index");
            }
            91 => return unsup(name),
            _ => return unsup(name),
        }
        Ok(())
    }

    fn coords(&mut self, kind: u32, args: &[u32], at: usize) -> Res<(String, Option<String>)> {
        let c = |k: usize| self.ex(args[at + k]);
        Ok(match kind {
            1 => (c(0)?, None),
            2 | 3 => (format!("vec2<f32>({}, {})", c(0)?, c(1)?), None),
            4 | 5 => (format!("vec3<f32>({}, {}, {})", c(0)?, c(1)?, c(2)?), None),
            7 | 8 => (
                format!("vec2<f32>({}, {})", c(0)?, c(1)?),
                Some(format!("u32(max(floor({} + 0.5), 0.0))", c(2)?)),
            ),
            9 => (
                format!("vec3<f32>({}, {}, {})", c(0)?, c(1)?, c(2)?),
                Some(format!("u32(max(floor({} + 0.5), 0.0))", c(3)?)),
            ),
            k => return unsup(format!("sample of kind {k}")),
        })
    }

    fn offset(&self, kind: u32, args: &[u32], at: usize) -> Res<String> {
        let mut vals = Vec::new();
        let n = match kind {
            1 => 1,
            2 | 7 => 2,
            4 => 3,
            _ => 0,
        };
        for k in 0..n {
            vals.push(self.const_int(args[at + k]).unwrap_or(0) as i32);
        }
        if vals.iter().all(|&v| v == 0) {
            return Ok(String::new());
        }
        Ok(match n {
            1 => format!(", {}", vals[0]),
            2 => format!(", vec2<i32>({}, {})", vals[0], vals[1]),
            _ => format!(", vec3<i32>({}, {}, {})", vals[0], vals[1], vals[2]),
        })
    }

    fn sample(&mut self, id: u32, op: i64, args: &[u32], fname: &str) -> Res<()> {
        let th = self.handle(args[1])?.clone();
        let tr = self.range_of(&th).clone();
        let t = self.res_ex(&th);
        let kind = tr.kind;
        if op == 81 {
            if kind != 2 {
                return unsup("calculateLOD of a non-2D texture");
            }
            let (uv, _) = self.coords(kind, args, 3)?;
            let sz = format!("vec2<f32>(textureDimensions({t}))");
            let r = format!(
                "(0.5 * log2(max(dot(dpdx({uv} * {sz}), dpdx({uv} * {sz})), dot(dpdy({uv} * {sz}), dpdy({uv} * {sz})))))"
            );
            self.assign(id, &r);
            return Ok(());
        }
        let sh = self.handle(args[2])?.clone();
        let s = self.res_ex(&sh);
        let (uv, arr) = self.coords(kind, args, 3)?;
        let arr_s = arr.map(|x| format!(", {x}")).unwrap_or_default();
        let off = self.offset(kind, args, 7)?;
        let depth = self.depth.contains(&th.range);
        let r = match op {
            60 => format!("textureSample({t}, {s}, {uv}{arr_s}{off})"),
            61 => format!("textureSampleBias({t}, {s}, {uv}{arr_s}, {}{off})", self.ex(args[10])?),
            62 => format!("textureSampleLevel({t}, {s}, {uv}{arr_s}, {}{off})", self.ex(args[10])?),
            63 => {
                let n = match kind {
                    4 | 5 | 9 => 3,
                    _ => 2,
                };
                let dx: Vec<String> = (0..n).map(|k| self.ex(args[10 + k])).collect::<Res<_>>()?;
                let dy: Vec<String> = (0..n).map(|k| self.ex(args[13 + k])).collect::<Res<_>>()?;
                format!(
                    "textureSampleGrad({t}, {s}, {uv}{arr_s}, vec{n}<f32>({}), vec{n}<f32>({}){off})",
                    dx.join(", "),
                    dy.join(", ")
                )
            }
            64 | 65 => {
                let rf = self.ex(args[10])?;
                let f = if op == 64 { "textureSampleCompare" } else { "textureSampleCompareLevel" };
                let v = format!("{f}({t}, {s}, {uv}{arr_s}, {rf}{off})");
                format!("vec4<f32>({v}, {v}, {v}, {v})")
            }
            73 => {
                let ch = self.const_int(args[9]).unwrap_or(0).clamp(0, 3);
                if depth {
                    format!("textureGather({t}, {s}, {uv}{arr_s})")
                } else {
                    format!("textureGather({ch}, {t}, {s}, {uv}{arr_s}{off})")
                }
            }
            74 => {
                let rf = self.ex(args[10])?;
                format!("textureGatherCompare({t}, {s}, {uv}{arr_s}, {rf}{off})")
            }
            _ => return unsup(fname.to_string()),
        };
        let float = fname.ends_with(".f32") || fname.ends_with(".f16");
        let r = if float { r } else { format!("bitcast<vec4<u32>>({r})") };
        // Depth textures return a scalar from a plain sample as well.
        let r = if depth && matches!(op, 60 | 61 | 62 | 63) {
            let inner = r.clone();
            format!("vec4<f32>(({inner}))")
        } else {
            r
        };
        self.assign(id, &r);
        Ok(())
    }

    fn tex_load(&mut self, id: u32, args: &[u32]) -> Res<()> {
        let th = self.handle(args[1])?.clone();
        let tr = self.range_of(&th).clone();
        let t = self.res_ex(&th);
        if tr.class == Class::Uav {
            return unsup("UAV texture load");
        }
        let mip = self.ex(args[2])?;
        let c = |k: usize| self.ex(args[3 + k]);
        let depth = self.depth.contains(&th.range);
        let (coords, arr) = match tr.kind {
            1 => (c(0)?, None),
            2 | 3 => (format!("vec2<u32>({}, {})", c(0)?, c(1)?), None),
            4 => (format!("vec3<u32>({}, {}, {})", c(0)?, c(1)?, c(2)?), None),
            7 => (format!("vec2<u32>({}, {})", c(0)?, c(1)?), Some(c(2)?)),
            k => return unsup(format!("textureLoad of kind {k}")),
        };
        let arr_s = arr.map(|x| format!(", {x}")).unwrap_or_default();
        let r = format!("textureLoad({t}, {coords}{arr_s}, {mip})");
        let float = tr.elem == 9 || tr.elem == 8;
        let r = if depth {
            format!("vec4<f32>({r}, 0.0, 0.0, 0.0)")
        } else if tr.elem == 4 {
            format!("bitcast<vec4<u32>>({r})")
        } else {
            let _ = float;
            r
        };
        let r = if !float && tr.elem != 4 && tr.elem != 5 { r } else { r };
        self.assign(id, &r);
        Ok(())
    }

    fn buf_load(&mut self, id: u32, op: i64, args: &[u32]) -> Res<()> {
        let h = self.handle(args[1])?.clone();
        let r = self.range_of(&h).clone();
        if r.class == Class::Uav {
            return unsup("UAV buffer load");
        }
        let n = self.res_ex(&h);
        let mask = self.comp_mask(id);
        let idx = self.ex(args[2])?;
        let float_out = matches!(self.sc(self.ty_of(id))?, Sc::Agg(true));
        let _ = op;
        match r.kind {
            11 | 12 => {
                self.raw_helpers.insert(h.range);
                let f = format!("rb_{}", range_name(&r, h.range));
                let off = self.ex(args[3])?;
                let word = if r.kind == 11 {
                    format!("((({idx}) + ({off})) >> 2u)")
                } else {
                    if r.stride == 0 {
                        return unsup("structured buffer without a stride");
                    }
                    format!("(({idx}) * {}u + (({off}) >> 2u))", r.stride / 4)
                };
                let comp = |k: u32| {
                    if mask & (1 << k) != 0 {
                        format!("{f}({word} + {k}u)")
                    } else {
                        "0u".to_string()
                    }
                };
                let rhs = format!("vec4<u32>({}, {}, {}, {})", comp(0), comp(1), comp(2), comp(3));
                self.assign(id, &if float_out { format!("bitcast<vec4<f32>>({rhs})") } else { rhs });
            }
            10 => {
                let comps = typed_comps(&r.type_name);
                let buf_float = r.elem == 9 || r.elem == 8;
                let e = format!("{n}[{idx}]");
                let t = if buf_float { "f32" } else { "u32" };
                let one = if buf_float { "1.0" } else { "1u" };
                let zero = if buf_float { "0.0" } else { "0u" };
                let v = match comps {
                    1 => format!("vec4<{t}>({e}, {zero}, {zero}, {one})"),
                    2 => format!("vec4<{t}>({e}.x, {e}.y, {zero}, {one})"),
                    3 => format!("vec4<{t}>({e}.x, {e}.y, {e}.z, {one})"),
                    _ => e,
                };
                let v = match (buf_float, float_out) {
                    (true, false) => format!("bitcast<vec4<u32>>({v})"),
                    (false, true) => format!("bitcast<vec4<f32>>({v})"),
                    _ => v,
                };
                self.assign(id, &v);
            }
            k => return unsup(format!("buffer load of kind {k}")),
        }
        Ok(())
    }
}

/// Components of a typed buffer element from its declared type name.
fn typed_comps(name: &str) -> u32 {
    name.find("vector<")
        .and_then(|at| {
            let t = &name[at..];
            let c = t.find(", ")?;
            t[c + 2..].chars().next()?.to_digit(10)
        })
        .unwrap_or(1)
}

// --- Module assembly --------------------------------------------------------

fn tex_decl(r: &Range, depth: bool) -> Res<String> {
    let st = match r.elem {
        4 => "i32",
        5 => "u32",
        _ => "f32",
    };
    Ok(match (r.kind, depth) {
        (1, _) => format!("texture_1d<{st}>"),
        (2, true) => "texture_depth_2d".into(),
        (2, false) => format!("texture_2d<{st}>"),
        (3, true) => "texture_depth_multisampled_2d".into(),
        (3, false) => format!("texture_multisampled_2d<{st}>"),
        (4, _) => format!("texture_3d<{st}>"),
        (5, true) => "texture_depth_cube".into(),
        (5, false) => format!("texture_cube<{st}>"),
        (7, true) => "texture_depth_2d_array".into(),
        (7, false) => format!("texture_2d_array<{st}>"),
        (9, true) => "texture_depth_cube_array".into(),
        (9, false) => format!("texture_cube_array<{st}>"),
        (k, _) => return unsup(format!("texture kind {k}")),
    })
}

fn interp_attr(el: &SigElement) -> &'static str {
    if el.comp != 9 && el.comp != 8 {
        return " @interpolate(flat)";
    }
    match el.interp {
        1 => " @interpolate(flat)",
        3 => " @interpolate(perspective, centroid)",
        4 => " @interpolate(linear)",
        5 => " @interpolate(perspective, sample)",
        6 => " @interpolate(linear, centroid)",
        7 => " @interpolate(linear, sample)",
        _ => "",
    }
}

fn vec_of(comp: u32) -> &'static str {
    match comp {
        9 | 8 => "vec4<f32>",
        4 => "vec4<i32>",
        _ => "vec4<u32>",
    }
}

/// Translates the module's entry point (a vertex or pixel program).
///
/// # Errors
/// [`Error::Unsupported`] naming the first construct without a mapping.
#[allow(clippy::too_many_lines)]
pub fn translate(m: &Module) -> Result<Translation, Error> {
    let entry = meta::entry(m).ok_or(Error::Malformed("no entry point metadata"))?;
    if !matches!(entry.stage, Stage::Pixel | Stage::Vertex) {
        return unsup(format!("stage {:?}", entry.stage));
    }
    let f = m
        .functions
        .iter()
        .find(|f| f.name == entry.name)
        .ok_or(Error::Malformed("entry function missing"))?;
    let body = f.body.as_ref().ok_or(Error::Malformed("entry without body"))?;
    let nblocks = body.block_starts.len();
    let mut succ = vec![Vec::new(); nblocks];
    for b in 0..nblocks {
        let end = body.block_starts.get(b + 1).copied().unwrap_or(body.insts.len());
        if let Some((_, last)) = body.insts[..end].last() {
            succ[b] = terminator_succ(last);
        }
        if end <= body.block_starts[b] {
            return Err(Error::Malformed("empty block"));
        }
    }
    let ipdom = post_dominators(&succ);
    let loop_of = find_loops(&succ, &ipdom)?;
    let mut cx = Cx {
        m,
        body,
        entry: entry.clone(),
        handles: BTreeMap::new(),
        depth: BTreeSet::new(),
        used: BTreeMap::new(),
        out: String::new(),
        indent: 1,
        succ,
        ipdom,
        loop_of,
        loops: Vec::new(),
        switch_depth: 0,
        uses_sample_index: false,
        nblocks,
        emitted: 0,
        used_ranges: core::cell::RefCell::new(BTreeSet::new()),
        raw_helpers: BTreeSet::new(),
        globals_decl: core::cell::RefCell::new(String::new()),
        globals_done: core::cell::RefCell::new(BTreeSet::new()),
        pointers: BTreeMap::new(),
        pstore: BTreeMap::new(),
        allocas: Vec::new(),
    };
    // Pass 1: handles, depth use, extracted components.
    for (id, inst) in &body.insts {
        match inst {
            Inst::Call { callee, args } => {
                let Some(ValueKind::Function { index }) = cx.value(*callee).map(|v| &v.kind) else {
                    continue;
                };
                let name = &m.functions[*index].name;
                let op = args.first().and_then(|&a| cx.const_int(a)).unwrap_or(-1);
                if !name.starts_with("dx.op.") {
                    continue;
                }
                if op == 57 {
                    let class = cx.const_int(args[1]).unwrap_or(-1);
                    let rid = cx.const_int(args[2]).unwrap_or(-1);
                    let class = match class {
                        0 => Class::Srv,
                        1 => Class::Uav,
                        2 => Class::Cbv,
                        3 => Class::Sampler,
                        _ => return Err(Error::Malformed("handle class")),
                    };
                    let ri = entry
                        .ranges
                        .iter()
                        .position(|r| r.class == class && i64::from(r.id) == rid)
                        .ok_or(Error::Malformed("handle names no range"))?;
                    let r = &entry.ranges[ri];
                    let index = if is_unbounded(r) { Some(cx.ex(args[3])?) } else { None };
                    cx.handles.insert(*id, Handle { range: ri, index });
                } else if matches!(op, 64 | 65 | 74) {
                    // Resolved after all handles exist (handles come first in
                    // practice, but do not rely on it).
                }
            }
            Inst::ExtractVal { agg, idx } => {
                let k = idx.first().copied().unwrap_or(0);
                if k < 4 {
                    *cx.used.entry(*agg).or_insert(0) |= 1 << k;
                }
            }
            _ => {}
        }
    }
    for (_, inst) in &body.insts {
        if let Inst::Call { callee, args } = inst {
            let Some(ValueKind::Function { index }) = cx.value(*callee).map(|v| &v.kind) else {
                continue;
            };
            if !m.functions[*index].name.starts_with("dx.op.") {
                continue;
            }
            let op = args.first().and_then(|&a| cx.const_int(a)).unwrap_or(-1);
            if matches!(op, 64 | 65 | 74) {
                if let (Some(t), Some(s)) = (cx.handles.get(&args[1]), cx.handles.get(&args[2])) {
                    cx.depth.insert(t.range);
                    cx.depth.insert(s.range);
                }
            }
        }
    }
    // Pass 2: the body.
    cx.region(0, usize::MAX, false)?;
    let body_text = core::mem::take(&mut cx.out);

    // Declarations.
    let mut w = String::new();
    let mut bindings = Vec::new();
    if entry.stage == Stage::Pixel {
        w.push_str("diagnostic(off, derivative_uniformity);\n");
    }
    // Calls to these keep naga from folding a literal argument that is out of
    // the function's domain (the program only runs it on live values).
    w.push_str(
        "fn d_bitsf(x: u32) -> f32 { return bitcast<f32>(x); }\nfn d_bitsu(x: f32) -> u32 { return bitcast<u32>(x); }\nfn d_i32(x: u32) -> i32 { return bitcast<i32>(x); }\nfn d_u32(x: i32) -> u32 { return bitcast<u32>(x); }\n\
         fn d_log2(x: f32) -> f32 { return log2(x); }\nfn d_sqrt(x: f32) -> f32 { return sqrt(x); }\n\
         fn d_inverseSqrt(x: f32) -> f32 { return inverseSqrt(x); }\nfn d_exp2(x: f32) -> f32 { return exp2(x); }\n\
         fn d_acos(x: f32) -> f32 { return acos(x); }\nfn d_asin(x: f32) -> f32 { return asin(x); }\n",
    );
    let used_ranges = cx.used_ranges.borrow().clone();
    for &ri in &used_ranges {
        let r = &entry.ranges[ri];
        let name = range_name(r, ri);
        let group = r.space;
        let binding = binding_number(r);
        let array = is_unbounded(r);
        let depth = cx.depth.contains(&ri);
        let ty = match r.class {
            Class::Cbv => {
                let n = r.cb_bytes.div_ceil(16).max(1);
                format!("array<vec4<u32>, {n}>")
            }
            Class::Sampler => {
                if depth { "sampler_comparison".to_string() } else { "sampler".to_string() }
            }
            Class::Srv | Class::Uav => match r.kind {
                10 => {
                    let t = if r.elem == 9 || r.elem == 8 { "f32" } else { "u32" };
                    match typed_comps(&r.type_name) {
                        1 => format!("array<{t}>"),
                        n => format!("array<vec{n}<{t}>>"),
                    }
                }
                11 | 12 => "array<u32>".to_string(),
                _ => tex_decl(r, depth)?,
            },
        };
        let decl = match r.class {
            Class::Cbv => format!("@group({group}) @binding({binding}) var<uniform> {name}: {ty};\n"),
            Class::Srv if matches!(r.kind, 10..=12) => {
                format!("@group({group}) @binding({binding}) var<storage, read> {name}: {ty};\n")
            }
            Class::Uav => return unsup("unordered access view"),
            _ => {
                if array {
                    format!("@group({group}) @binding({binding}) var {name}: binding_array<{ty}, {}>;\n", r.size)
                } else {
                    format!("@group({group}) @binding({binding}) var {name}: {ty};\n")
                }
            }
        };
        if r.class == Class::Cbv && array {
            return unsup("array of constant buffers");
        }
        w.push_str(&decl);
        bindings.push(Binding {
            range: r.clone(),
            group,
            binding,
            name,
            array: array && r.class != Class::Cbv,
            depth_or_cmp: depth,
        });
    }
    for &ri in &cx.raw_helpers {
        let r = &entry.ranges[ri];
        let n = range_name(r, ri);
        let _ = writeln!(
            w,
            "fn rb_{n}(i: u32) -> u32 {{ if i < arrayLength(&{n}) {{ return {n}[i]; }} return 0u; }}"
        );
    }
    w.push_str(&cx.globals_decl.borrow());
    // Private storage for signature elements.
    for el in &entry.inputs {
        let t = vec_of(el.comp);
        if el.rows > 1 {
            let _ = writeln!(w, "var<private> in{}: array<{t}, {}>;", el.id, el.rows);
        } else {
            let _ = writeln!(w, "var<private> in{}: {t};", el.id);
        }
    }
    for el in &entry.outputs {
        let t = vec_of(el.comp);
        if el.rows > 1 {
            let _ = writeln!(w, "var<private> out{}: array<{t}, {}>;", el.id, el.rows);
        } else {
            let _ = writeln!(w, "var<private> out{}: {t};", el.id);
        }
    }
    // Varying locations: rank in signature order (rows take consecutive ones).
    let varying_locs = |els: &[SigElement], pixel_in: bool| -> BTreeMap<u32, u32> {
        let mut m = BTreeMap::new();
        let mut next = 0u32;
        for el in els {
            if el.sem == 0 || (pixel_in && el.sem == 6) {
                m.insert(el.id, next);
                next += el.rows.max(1);
            }
        }
        m
    };
    let loc_in = varying_locs(&entry.inputs, true);
    let loc_out = varying_locs(&entry.outputs, false);
    // Entry-point interface structs.
    let mut ins = String::new();
    let mut in_assign = String::new();
    let stage = entry.stage;
    for el in &entry.inputs {
        let t = vec_of(el.comp);
        match (stage, el.sem) {
            (Stage::Pixel, 3) => {
                let _ = writeln!(ins, "  @builtin(position) i{}: vec4<f32>,", el.id);
                let _ = writeln!(in_assign, "  in{} = i.i{};", el.id, el.id);
            }
            (Stage::Pixel, 13) => {
                let _ = writeln!(ins, "  @builtin(front_facing) i{}: bool,", el.id);
                let _ = writeln!(
                    in_assign,
                    "  in{} = vec4<u32>(select(0u, 0xffffffffu, i.i{}), 0u, 0u, 0u);",
                    el.id, el.id
                );
            }
            (Stage::Pixel, 12) => {
                let _ = writeln!(ins, "  @builtin(sample_index) i{}: u32,", el.id);
                let _ = writeln!(in_assign, "  in{} = vec4<u32>(i.i{}, 0u, 0u, 0u);", el.id, el.id);
            }
            (Stage::Vertex, 1) => {
                let _ = writeln!(ins, "  @builtin(vertex_index) i{}: u32,", el.id);
                let _ = writeln!(in_assign, "  in{} = vec4<u32>(i.i{}, 0u, 0u, 0u);", el.id, el.id);
            }
            (Stage::Vertex, 2) => {
                let _ = writeln!(ins, "  @builtin(instance_index) i{}: u32,", el.id);
                let _ = writeln!(in_assign, "  in{} = vec4<u32>(i.i{}, 0u, 0u, 0u);", el.id, el.id);
            }
            (_, s) if s != 0 && !(stage == Stage::Pixel && s == 6) => return unsup(format!("input semantic {} (kind {s})", el.name)),
            _ => {
                let attr = if stage == Stage::Pixel { interp_attr(el) } else { "" };
                for r in 0..el.rows.max(1) {
                    let _ = writeln!(
                        ins,
                        "  @location({}){attr} i{}_{r}: {t},",
                        loc_in[&el.id] + r,
                        el.id
                    );
                    if el.rows > 1 {
                        let _ = writeln!(in_assign, "  in{}[{r}] = i.i{}_{r};", el.id, el.id);
                    } else {
                        let _ = writeln!(in_assign, "  in{} = i.i{}_{r};", el.id, el.id);
                    }
                }
            }
        }
    }
    if cx.uses_sample_index && stage == Stage::Pixel {
        if entry.inputs.iter().any(|e| e.sem == 12) {
            let id = entry.inputs.iter().find(|e| e.sem == 12).map_or(0, |e| e.id);
            let _ = writeln!(in_assign, "  in_sample_index = i.i{id};");
        } else {
            ins.push_str("  @builtin(sample_index) sidx: u32,\n");
            in_assign.push_str("  in_sample_index = i.sidx;\n");
        }
        w.push_str("var<private> in_sample_index: u32;\n");
    }
    let mut outs = String::new();
    let mut out_init = String::new();
    let clip_total: u32 = if stage == Stage::Vertex {
        entry
            .outputs
            .iter()
            .filter(|e| e.sem == 6)
            .map(|e| e.cols.max(1) * e.rows.max(1))
            .sum()
    } else {
        0
    };
    if clip_total > 0 {
        w = format!("enable clip_distances;\n{w}");
        let _ = writeln!(outs, "  @builtin(clip_distances) cd: array<f32, {clip_total}>,");
    }
    let mut clip_at = 0u32;
    for el in &entry.outputs {
        let t = vec_of(el.comp);
        match (stage, el.sem) {
            (Stage::Vertex, 6) => {
                for r in 0..el.rows.max(1) {
                    for c in 0..el.cols.max(1) {
                        let comp = ["x", "y", "z", "w"][(c & 3) as usize];
                        if el.rows > 1 {
                            let _ = writeln!(out_init, "o.cd[{clip_at}] = out{}[{r}].{comp};", el.id);
                        } else {
                            let _ = writeln!(out_init, "o.cd[{clip_at}] = out{}.{comp};", el.id);
                        }
                        clip_at += 1;
                    }
                }
            }
            (Stage::Vertex, 3) => {
                let _ = writeln!(outs, "  @builtin(position) o{}: vec4<f32>,", el.id);
                let _ = writeln!(out_init, "o.o{} = out{};", el.id, el.id);
            }
            (Stage::Pixel, 17..=19) => {
                let _ = writeln!(outs, "  @builtin(frag_depth) o{}: f32,", el.id);
                let _ = writeln!(out_init, "o.o{} = out{}.x;", el.id, el.id);
            }
            (Stage::Pixel, 14) => {
                let _ = writeln!(outs, "  @builtin(sample_mask) o{}: u32,", el.id);
                let _ = writeln!(out_init, "o.o{} = out{}.x;", el.id, el.id);
            }
            (Stage::Pixel, 16) => {
                let _ = writeln!(outs, "  @location({}) o{}: {t},", el.semantic_index, el.id);
                let _ = writeln!(out_init, "o.o{} = out{};", el.id, el.id);
            }
            (Stage::Vertex, 0) => {
                let attr = interp_attr(el);
                for r in 0..el.rows.max(1) {
                    let _ = writeln!(outs, "  @location({}){attr} o{}_{r}: {t},", loc_out[&el.id] + r, el.id);
                    if el.rows > 1 {
                        let _ = writeln!(out_init, "o.o{}_{r} = out{}[{r}];", el.id, el.id);
                    } else {
                        let _ = writeln!(out_init, "o.o{}_{r} = out{};", el.id, el.id);
                    }
                }
            }
            (_, s) => return unsup(format!("output semantic {} (kind {s})", el.name)),
        }
    }
    if !ins.is_empty() {
        let _ = writeln!(w, "struct EntryIn {{\n{ins}}}");
    }
    if !outs.is_empty() {
        let _ = writeln!(w, "struct EntryOut {{\n{outs}}}");
    }
    // Value declarations.
    let mut decls = String::new();
    for (id, inst) in &body.insts {
        if *id == u32::MAX {
            continue;
        }
        if matches!(cx.sc(cx.ty_of(*id)), Ok(Sc::Ptr)) {
            continue;
        }
        if matches!(inst, Inst::Alloca { .. } | Inst::Gep { .. } | Inst::Load { .. }) {
            if matches!(inst, Inst::Load { .. }) {
                // A load is a value.
            } else {
                continue;
            }
        }
        if let Inst::Call { callee, args } = inst {
            if let Some(ValueKind::Function { index }) = cx.value(*callee).map(|v| &v.kind) {
                if m.functions[*index].name.starts_with("dx.op.")
                    && args.first().and_then(|&a| cx.const_int(a)) == Some(57)
                {
                    continue;
                }
            }
        }
        let ty = cx.ty_of(*id);
        let t = cx.wty(ty).map_err(|e| match e {
            Error::Unsupported(w) => Error::Unsupported(format!("{w} (value {id}: {inst:?})")),
            other => other,
        })?;
        let _ = writeln!(decls, "  var v{id}: {t};");
    }
    for (id, t) in &cx.allocas {
        let _ = writeln!(decls, "  var a{id}: {t};");
    }
    let _ = writeln!(w, "fn body() {{\n{decls}{body_text}}}");
    let attr = if stage == Stage::Pixel { "@fragment" } else { "@vertex" };
    let ret = if outs.is_empty() { String::new() } else { " -> EntryOut".to_string() };
    let param = if ins.is_empty() { "" } else { "i: EntryIn" };
    let _ = writeln!(w, "{attr}\nfn main({param}){ret} {{\n{in_assign}  body();");
    if !outs.is_empty() {
        let _ = writeln!(w, "  var o: EntryOut;\n  {}\n  return o;", out_init.trim_end().replace('\n', "\n  "));
    }
    w.push_str("}\n");
    let _ = &cx.succ;
    Ok(Translation {
        wgsl: w,
        stage,
        bindings,
        inputs: entry.inputs,
        outputs: entry.outputs,
        input_locations: loc_in.into_iter().collect(),
        output_locations: loc_out.into_iter().collect(),
    })
}
