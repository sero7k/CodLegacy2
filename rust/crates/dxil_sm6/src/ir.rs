//! The LLVM 3.7 module DXIL uses, read from the bitstream tree: types,
//! global values, constants, metadata and function bodies.
//!
//! Value numbering follows the reader's: global variables, functions and
//! aliases in record order, then the module constants; inside a function its
//! arguments, its constants, then every instruction that yields a value.
//! Instruction operands are relative (`this - operand`).

use alloc::string::String;
use alloc::vec::Vec;

use crate::bitstream::{self, Block, Error, Item, Record};

/// A type in the module's table.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Type {
    Void,
    Half,
    Float,
    Double,
    Label,
    Metadata,
    Int(u32),
    Pointer { to: u32, space: u32 },
    Array { len: u64, of: u32 },
    Vector { len: u64, of: u32 },
    Struct { name: String, fields: Vec<u32> },
    Function { ret: u32, params: Vec<u32>, vararg: bool },
    Opaque,
}

/// A constant.
#[derive(Clone, Debug, PartialEq)]
pub enum Const {
    Null,
    Undef,
    Int(i64),
    /// IEEE bits as stored (f32 in the low 32, f64 whole, f16 in the low 16).
    Float(u64),
    Aggregate(Vec<u32>),
    Data(Vec<u64>),
    Str(Vec<u8>),
    /// A constant expression this reader keeps raw: (code, operands).
    Expr(u32, Vec<u64>),
}

/// What a value id names.
#[derive(Clone, Debug)]
pub enum ValueKind {
    Global { init: Option<u32> },
    Function { index: usize },
    Constant(Const),
    Arg(u32),
    Inst,
}

/// A value with its type (a type id; `u32::MAX` when not worked out).
#[derive(Clone, Debug)]
pub struct Value {
    pub ty: u32,
    pub kind: ValueKind,
}

/// A function's declaration or definition.
#[derive(Clone, Debug)]
pub struct Function {
    pub name: String,
    /// Function type id.
    pub ty: u32,
    pub is_proto: bool,
    /// Its value id.
    pub value: u32,
    pub body: Option<Body>,
}

/// Instruction operands are value ids (absolute).
#[derive(Clone, Debug, PartialEq)]
pub enum Inst {
    Binop { op: u8, lhs: u32, rhs: u32 },
    Cast { op: u8, val: u32, to: u32 },
    Gep { inbounds: bool, base: u32, idx: Vec<u32> },
    Select { cond: u32, t: u32, f: u32 },
    ExtractElt { vec: u32, idx: u32 },
    InsertElt { vec: u32, elt: u32, idx: u32 },
    Shuffle { a: u32, b: u32, mask: u32 },
    Cmp { pred: u8, lhs: u32, rhs: u32 },
    Ret(Option<u32>),
    Br { t: u32, f: u32, cond: Option<u32> },
    Switch { cond: u32, default: u32, cases: Vec<(u32, u32)> },
    Unreachable,
    Phi { ty: u32, incoming: Vec<(u32, u32)> },
    Alloca { ty: u32, size: u32 },
    Load { ptr: u32, ty: u32 },
    Store { ptr: u32, val: u32 },
    ExtractVal { agg: u32, idx: Vec<u32> },
    InsertVal { agg: u32, val: u32, idx: Vec<u32> },
    Call { callee: u32, args: Vec<u32> },
    /// A record this reader does not decode.
    Other(u32),
}

/// A function body: its basic blocks' instructions in order.
#[derive(Clone, Debug, Default)]
pub struct Body {
    /// Per instruction: its value id (`u32::MAX` if it yields none) and it.
    pub insts: Vec<(u32, Inst)>,
    /// The index into `insts` where each basic block starts.
    pub block_starts: Vec<usize>,
    /// First value id of this function's arguments.
    pub first_arg: u32,
    /// The function's own values from `first_arg` on: arguments, constants,
    /// instruction results.
    pub locals: Vec<Value>,
}

impl Body {
    /// A value of this function (module values are `module.values`).
    #[must_use]
    pub fn local(&self, id: u32) -> Option<&Value> {
        id.checked_sub(self.first_arg)
            .and_then(|i| self.locals.get(i as usize))
    }
}

/// Module-level metadata.
#[derive(Clone, Debug, Default)]
pub struct Metadata {
    /// Every metadata item by id: strings, values and nodes in record order.
    pub nodes: Vec<MdNode>,
    /// Named nodes: (name, node ids).
    pub named: Vec<(String, Vec<u32>)>,
}

/// A metadata node.
#[derive(Clone, Debug)]
pub enum MdNode {
    Str(String),
    /// Operand ids plus one (0 = null).
    Node(Vec<u32>),
    Value { ty: u32, value: u32 },
    Other(u32),
}

/// A decoded module.
#[derive(Clone, Debug, Default)]
pub struct Module {
    pub types: Vec<Type>,
    pub values: Vec<Value>,
    pub functions: Vec<Function>,
    pub metadata: Metadata,
}

struct Cur<'a> {
    rec: &'a Record,
    at: usize,
    inst: u32,
}

impl Cur<'_> {
    fn raw(&mut self) -> Result<u64, Error> {
        let v = self.rec.ops.get(self.at).copied().ok_or(Error::Eof)?;
        self.at += 1;
        Ok(v)
    }
    fn done(&self) -> bool {
        self.at >= self.rec.ops.len()
    }
    /// A relative value; with its type when it is a forward reference.
    fn value_ty(&mut self, m: &Module) -> Result<(u32, u32), Error> {
        let raw = self.raw()?;
        let id = self.inst.wrapping_sub(raw as u32);
        if id < self.inst {
            Ok((id, m.values.get(id as usize).map_or(u32::MAX, |v| v.ty)))
        } else {
            let ty = self.raw()? as u32;
            Ok((id, ty))
        }
    }
    fn value(&mut self) -> Result<u32, Error> {
        let raw = self.raw()?;
        Ok(self.inst.wrapping_sub(raw as u32))
    }
}

impl Module {
    fn find_type(&mut self, t: Type) -> u32 {
        if let Some(i) = self.types.iter().position(|x| *x == t) {
            return i as u32;
        }
        self.types.push(t);
        (self.types.len() - 1) as u32
    }

    /// The type a value of `ty` has after indexing by `idx` (GEP rules, the
    /// first index steps over the pointer).
    fn gep_type(&mut self, ptr_ty: u32, idx: &[u32]) -> u32 {
        let Some(Type::Pointer { to, space }) = self.types.get(ptr_ty as usize).cloned() else {
            return u32::MAX;
        };
        let mut cur = to;
        for &i in idx.iter().skip(1) {
            cur = match self.types.get(cur as usize) {
                Some(Type::Struct { fields, .. }) => {
                    let n = match self.values.get(i as usize).map(|v| &v.kind) {
                        Some(ValueKind::Constant(Const::Int(n))) => *n as usize,
                        _ => 0,
                    };
                    fields.get(n).copied().unwrap_or(u32::MAX)
                }
                Some(Type::Array { of, .. } | Type::Vector { of, .. }) => *of,
                _ => return u32::MAX,
            };
        }
        self.find_type(Type::Pointer { to: cur, space })
    }

    fn int_type(&mut self, w: u32) -> u32 {
        self.find_type(Type::Int(w))
    }

    fn read_types(&mut self, b: &Block) {
        let mut name = String::new();
        for r in b.records() {
            let o = &r.ops;
            let g = |i: usize| o.get(i).copied().unwrap_or(0);
            let t = match r.code {
                1 => continue,
                2 => Type::Void,
                3 => Type::Float,
                4 => Type::Double,
                5 => Type::Label,
                6 => Type::Opaque,
                7 => Type::Int(g(0) as u32),
                8 => Type::Pointer {
                    to: g(0) as u32,
                    space: g(1) as u32,
                },
                10 => Type::Half,
                11 => Type::Array {
                    len: g(0),
                    of: g(1) as u32,
                },
                12 => Type::Vector {
                    len: g(0),
                    of: g(1) as u32,
                },
                16 => Type::Metadata,
                18 => Type::Struct {
                    name: String::new(),
                    fields: o.iter().skip(1).map(|&x| x as u32).collect(),
                },
                19 => {
                    name = bitstream::text(o, 0);
                    continue;
                }
                20 => Type::Struct {
                    name: core::mem::take(&mut name),
                    fields: o.iter().skip(1).map(|&x| x as u32).collect(),
                },
                21 => Type::Function {
                    vararg: g(0) != 0,
                    ret: g(1) as u32,
                    params: o.iter().skip(2).map(|&x| x as u32).collect(),
                },
                _ => Type::Opaque,
            };
            self.types.push(t);
        }
    }

    fn read_constants(&mut self, b: &Block) {
        let mut ty = u32::MAX;
        for r in b.records() {
            let o = &r.ops;
            let c = match r.code {
                1 => {
                    ty = o.first().copied().unwrap_or(0) as u32;
                    continue;
                }
                2 => Const::Null,
                3 => Const::Undef,
                4 => {
                    let v = o.first().copied().unwrap_or(0);
                    Const::Int(if v & 1 == 0 {
                        (v >> 1) as i64
                    } else if v == 1 {
                        i64::MIN
                    } else {
                        -((v >> 1) as i64)
                    })
                }
                6 => Const::Float(o.first().copied().unwrap_or(0)),
                7 => Const::Aggregate(o.iter().map(|&x| x as u32).collect()),
                8 | 9 => Const::Str(o.iter().map(|&x| x as u8).collect()),
                22 => Const::Data(o.clone()),
                code => Const::Expr(code, o.clone()),
            };
            self.values.push(Value {
                ty,
                kind: ValueKind::Constant(c),
            });
        }
    }

    fn read_metadata(&mut self, b: &Block) {
        let md = &mut self.metadata;
        let mut pending_name = String::new();
        for r in b.records() {
            let o = &r.ops;
            match r.code {
                35 => {
                    // [count, offset to chars] + blob: vbr6 lengths, then chars.
                    let count = o.first().copied().unwrap_or(0) as usize;
                    let off = o.get(1).copied().unwrap_or(0) as usize;
                    if let Some(blob) = &r.blob {
                        let lens = read_vbr6s(blob, count);
                        let mut at = off;
                        for l in lens {
                            let s = blob.get(at..at + l).unwrap_or(&[]);
                            md.nodes
                                .push(MdNode::Str(String::from_utf8_lossy(s).into_owned()));
                            at += l;
                        }
                    }
                }
                1 => md.nodes.push(MdNode::Str(bitstream::text(o, 0))),
                3 | 5 => md
                    .nodes
                    .push(MdNode::Node(o.iter().map(|&x| x as u32).collect())),
                2 => md.nodes.push(MdNode::Value {
                    ty: o.first().copied().unwrap_or(0) as u32,
                    value: o.get(1).copied().unwrap_or(0) as u32,
                }),
                4 => pending_name = bitstream::text(o, 0),
                10 => {
                    md.named.push((
                        core::mem::take(&mut pending_name),
                        o.iter().map(|&x| x as u32).collect(),
                    ));
                }
                6 => {}
                code => md.nodes.push(MdNode::Other(code)),
            }
        }
    }

    fn read_function(&mut self, b: &Block, fidx: usize) -> Result<Body, Error> {
        let fty = self.functions[fidx].ty;
        let params = match self.types.get(fty as usize) {
            Some(Type::Function { params, .. }) => params.clone(),
            _ => Vec::new(),
        };
        let mut body = Body {
            first_arg: self.values.len() as u32,
            ..Body::default()
        };
        let globals = self.values.len();
        for (i, p) in params.iter().enumerate() {
            self.values.push(Value {
                ty: *p,
                kind: ValueKind::Arg(i as u32),
            });
        }
        let mut at_block_start = true;
        body.block_starts.push(0);
        for item in &b.items {
            match item {
                Item::Block(cb) if cb.id == 11 => self.read_constants(cb),
                Item::Block(_) => {}
                Item::Record(r) => {
                    let inst_no = self.values.len() as u32;
                    if r.code == 1 || r.code == 33 || r.code == 35 {
                        continue;
                    }
                    let (inst, ty, yields) = self.read_inst(r, inst_no)?;
                    let term = matches!(
                        inst,
                        Inst::Ret(_) | Inst::Br { .. } | Inst::Switch { .. } | Inst::Unreachable
                    );
                    let _ = at_block_start;
                    let id = if yields {
                        self.values.push(Value {
                            ty,
                            kind: ValueKind::Inst,
                        });
                        inst_no
                    } else {
                        u32::MAX
                    };
                    body.insts.push((id, inst));
                    at_block_start = term;
                    if term {
                        body.block_starts.push(body.insts.len());
                    }
                }
            }
        }
        body.block_starts.pop();
        // Function-local values go away; the module's stay.
        body.locals = self.values.split_off(globals);
        Ok(body)
    }

    #[allow(clippy::too_many_lines)]
    fn read_inst(&mut self, r: &Record, inst_no: u32) -> Result<(Inst, u32, bool), Error> {
        let mut c = Cur {
            rec: r,
            at: 0,
            inst: inst_no,
        };
        let ty_of = |m: &Module, v: u32| m.values.get(v as usize).map_or(u32::MAX, |x| x.ty);
        Ok(match r.code {
            2 => {
                let (lhs, ty) = c.value_ty(self)?;
                let rhs = c.value()?;
                let op = c.raw()? as u8;
                (Inst::Binop { op, lhs, rhs }, ty, true)
            }
            3 => {
                let (val, _) = c.value_ty(self)?;
                let to = c.raw()? as u32;
                let op = c.raw()? as u8;
                (Inst::Cast { op, val, to }, to, true)
            }
            43 => {
                let inbounds = c.raw()? != 0;
                let _src = c.raw()?;
                let (base, bty) = c.value_ty(self)?;
                let mut idx = Vec::new();
                while !c.done() {
                    idx.push(c.value_ty(self)?.0);
                }
                let ty = self.gep_type(bty, &idx);
                (Inst::Gep { inbounds, base, idx }, ty, true)
            }
            29 => {
                let (t, ty) = c.value_ty(self)?;
                let f = c.value()?;
                let (cond, _) = c.value_ty(self)?;
                (Inst::Select { cond, t, f }, ty, true)
            }
            6 => {
                let (vec, vty) = c.value_ty(self)?;
                let (idx, _) = c.value_ty(self)?;
                let ty = match self.types.get(vty as usize) {
                    Some(Type::Vector { of, .. }) => *of,
                    _ => u32::MAX,
                };
                (Inst::ExtractElt { vec, idx }, ty, true)
            }
            7 => {
                let (vec, ty) = c.value_ty(self)?;
                let elt = c.value()?;
                let (idx, _) = c.value_ty(self)?;
                (Inst::InsertElt { vec, elt, idx }, ty, true)
            }
            8 => {
                let (a, ty) = c.value_ty(self)?;
                let b = c.value()?;
                let (mask, _) = c.value_ty(self)?;
                (Inst::Shuffle { a, b, mask }, ty, true)
            }
            9 | 28 => {
                let (lhs, _) = c.value_ty(self)?;
                let rhs = c.value()?;
                let pred = c.raw()? as u8;
                let ty = self.int_type(1);
                (Inst::Cmp { pred, lhs, rhs }, ty, true)
            }
            10 => {
                if c.done() {
                    (Inst::Ret(None), u32::MAX, false)
                } else {
                    (Inst::Ret(Some(c.value_ty(self)?.0)), u32::MAX, false)
                }
            }
            11 => {
                let t = c.raw()? as u32;
                if c.done() {
                    (Inst::Br { t, f: t, cond: None }, u32::MAX, false)
                } else {
                    let f = c.raw()? as u32;
                    let cond = c.value()?;
                    (
                        Inst::Br {
                            t,
                            f,
                            cond: Some(cond),
                        },
                        u32::MAX,
                        false,
                    )
                }
            }
            12 => {
                let _ty = c.raw()?;
                let cond = c.value()?;
                let default = c.raw()? as u32;
                let mut cases = Vec::new();
                while !c.done() {
                    let v = c.raw()? as u32;
                    let bb = c.raw()? as u32;
                    cases.push((v, bb));
                }
                (
                    Inst::Switch {
                        cond,
                        default,
                        cases,
                    },
                    u32::MAX,
                    false,
                )
            }
            15 => (Inst::Unreachable, u32::MAX, false),
            16 => {
                let ty = c.raw()? as u32;
                let mut incoming = Vec::new();
                while c.at + 1 < r.ops.len() {
                    let raw = c.raw()?;
                    let s = if raw & 1 == 0 {
                        (raw >> 1) as i64
                    } else {
                        -((raw >> 1) as i64)
                    };
                    let v = (i64::from(inst_no) - s) as u32;
                    let bb = c.raw()? as u32;
                    incoming.push((v, bb));
                }
                (Inst::Phi { ty, incoming }, ty, true)
            }
            19 => {
                let ty = c.raw()? as u32;
                let _opty = c.raw()?;
                let size = c.raw()? as u32;
                let p = self.find_type(Type::Pointer { to: ty, space: 0 });
                (Inst::Alloca { ty, size }, p, true)
            }
            20 => {
                let (ptr, _) = c.value_ty(self)?;
                let ty = c.raw()? as u32;
                (Inst::Load { ptr, ty }, ty, true)
            }
            44 => {
                let (ptr, _) = c.value_ty(self)?;
                let (val, _) = c.value_ty(self)?;
                (Inst::Store { ptr, val }, u32::MAX, false)
            }
            26 => {
                let (agg, aty) = c.value_ty(self)?;
                let mut idx = Vec::new();
                while !c.done() {
                    idx.push(c.raw()? as u32);
                }
                let mut ty = aty;
                for &i in &idx {
                    ty = match self.types.get(ty as usize) {
                        Some(Type::Struct { fields, .. }) => {
                            fields.get(i as usize).copied().unwrap_or(u32::MAX)
                        }
                        Some(Type::Array { of, .. } | Type::Vector { of, .. }) => *of,
                        _ => u32::MAX,
                    };
                }
                (Inst::ExtractVal { agg, idx }, ty, true)
            }
            27 => {
                let (agg, ty) = c.value_ty(self)?;
                let (val, _) = c.value_ty(self)?;
                let mut idx = Vec::new();
                while !c.done() {
                    idx.push(c.raw()? as u32);
                }
                (Inst::InsertVal { agg, val, idx }, ty, true)
            }
            34 => {
                let _attrs = c.raw()?;
                let cc = c.raw()?;
                if (cc >> 17) & 1 == 1 {
                    let _fmf = c.raw()?;
                }
                let explicit = (cc >> 15) & 1 == 1;
                let fnty = if explicit { Some(c.raw()? as u32) } else { None };
                let (callee, cty) = c.value_ty(self)?;
                let fty = fnty.or_else(|| match self.types.get(cty as usize) {
                    Some(Type::Pointer { to, .. }) => Some(*to),
                    _ => None,
                });
                let (ret, params, vararg) = match fty.and_then(|t| self.types.get(t as usize)) {
                    Some(Type::Function {
                        ret,
                        params,
                        vararg,
                    }) => (*ret, params.clone(), *vararg),
                    _ => return Err(Error::Bad("call without a function type")),
                };
                let mut args = Vec::new();
                for p in &params {
                    if matches!(self.types.get(*p as usize), Some(Type::Label)) {
                        args.push(c.raw()? as u32);
                    } else {
                        args.push(c.value()?);
                    }
                }
                if vararg {
                    while !c.done() {
                        args.push(c.value_ty(self)?.0);
                    }
                }
                let yields = !matches!(self.types.get(ret as usize), Some(Type::Void));
                (Inst::Call { callee, args }, ret, yields)
            }
            code => {
                // Unknown record: assume it yields no value.
                let _ = ty_of(self, 0);
                (Inst::Other(code), u32::MAX, false)
            }
        })
    }
}

fn read_vbr6s(blob: &[u8], count: usize) -> Vec<usize> {
    let mut out = Vec::with_capacity(count);
    let mut pos = 0usize;
    let bit = |n: usize, pos: &mut usize| -> Option<u64> {
        let mut v = 0u64;
        for k in 0..n {
            let p = *pos + k;
            let byte = *blob.get(p / 8)?;
            v |= u64::from((byte >> (p % 8)) & 1) << k;
        }
        *pos += n;
        Some(v)
    };
    for _ in 0..count {
        let mut v = 0u64;
        let mut shift = 0;
        loop {
            let Some(piece) = bit(6, &mut pos) else { return out };
            v |= (piece & 31) << shift;
            if piece & 32 == 0 {
                break;
            }
            shift += 5;
        }
        out.push(v as usize);
    }
    out
}

/// Reads a module from the DXIL part's bytes (program header and bitcode).
pub fn read_part(part: &[u8]) -> Result<Module, Error> {
    let bc = bitstream::bitcode_of_part(part)?;
    read(bc)
}

/// Reads a module from raw bitcode.
pub fn read(bitcode: &[u8]) -> Result<Module, Error> {
    let top = bitstream::parse(bitcode)?;
    let module = top
        .blocks()
        .find(|b| b.id == 8)
        .ok_or(Error::Bad("no module block"))?;
    let mut m = Module::default();
    for b in module.blocks() {
        if b.id == 17 {
            m.read_types(b);
        }
    }
    // Global values in record order.
    for r in module.records() {
        let o = &r.ops;
        let g = |i: usize| o.get(i).copied().unwrap_or(0);
        match r.code {
            7 => {
                let explicit = g(1) & 2 != 0;
                let ty = g(0) as u32;
                let pty = if explicit {
                    m.find_type(Type::Pointer {
                        to: ty,
                        space: (g(1) >> 2) as u32,
                    })
                } else {
                    ty
                };
                let init = g(2);
                m.values.push(Value {
                    ty: pty,
                    kind: ValueKind::Global {
                        init: (init != 0).then(|| (init - 1) as u32),
                    },
                });
            }
            8 => {
                let mut ty = g(0) as u32;
                if let Some(Type::Pointer { to, .. }) = m.types.get(ty as usize) {
                    ty = *to;
                }
                let value = m.values.len() as u32;
                let pty = m.find_type(Type::Pointer { to: ty, space: 0 });
                m.values.push(Value {
                    ty: pty,
                    kind: ValueKind::Function {
                        index: m.functions.len(),
                    },
                });
                m.functions.push(Function {
                    name: String::new(),
                    ty,
                    is_proto: g(2) != 0,
                    value,
                    body: None,
                });
            }
            9 | 14 => m.values.push(Value {
                ty: g(0) as u32,
                kind: ValueKind::Global { init: None },
            }),
            _ => {}
        }
    }
    for b in module.blocks() {
        if b.id == 11 {
            m.read_constants(b);
        }
    }
    for b in module.blocks() {
        match b.id {
            15 => m.read_metadata(b),
            14 => {
                // Module symbol table: FNENTRY [valueid, offset, name...] or
                // ENTRY [valueid, name...].
                for r in b.records() {
                    let (skip, ok) = match r.code {
                        3 => (2, true),
                        1 => (1, true),
                        _ => (0, false),
                    };
                    if !ok {
                        continue;
                    }
                    let vid = r.ops.first().copied().unwrap_or(0) as u32;
                    if let Some(f) = m.functions.iter_mut().find(|f| f.value == vid) {
                        f.name = bitstream::text(&r.ops, skip);
                    }
                }
            }
            _ => {}
        }
    }
    // Bodies, in the order of the defined functions.
    let mut defined: Vec<usize> = m
        .functions
        .iter()
        .enumerate()
        .filter(|(_, f)| !f.is_proto)
        .map(|(i, _)| i)
        .collect();
    defined.reverse();
    for b in module.blocks() {
        if b.id == 12 {
            let Some(fi) = defined.pop() else { break };
            let body = m.read_function(b, fi)?;
            m.functions[fi].body = Some(body);
        }
    }
    Ok(m)
}

impl Module {
    /// A function by name.
    #[must_use]
    pub fn function(&self, name: &str) -> Option<&Function> {
        self.functions.iter().find(|f| f.name == name)
    }

    /// A metadata node's operand as a string, an integer constant, or a node.
    #[must_use]
    pub fn md_operand(&self, id_plus_one: u32) -> MdRef<'_> {
        if id_plus_one == 0 {
            return MdRef::Null;
        }
        let id = (id_plus_one - 1) as usize;
        match self.metadata.nodes.get(id) {
            Some(MdNode::Str(s)) => MdRef::Str(s),
            Some(MdNode::Value { value, .. }) => match self.values.get(*value as usize) {
                Some(Value {
                    kind: ValueKind::Constant(Const::Int(n)),
                    ..
                }) => MdRef::Int(*n),
                _ => MdRef::Value(*value),
            },
            Some(MdNode::Node(ops)) => MdRef::Node(ops),
            _ => MdRef::Null,
        }
    }
}

/// A resolved metadata operand.
#[derive(Clone, Debug, PartialEq)]
pub enum MdRef<'a> {
    Null,
    Str(&'a str),
    Int(i64),
    Value(u32),
    Node(&'a [u32]),
}
