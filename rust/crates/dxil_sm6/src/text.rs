//! A plain-text listing of a module, for reading shaders by eye.

use alloc::format;
use alloc::string::String;

use crate::ir::{Body, Const, Inst, MdNode, Module, Type, ValueKind};

/// A type as short text.
#[must_use]
pub fn type_name(m: &Module, t: u32) -> String {
    match m.types.get(t as usize) {
        None => "?".into(),
        Some(Type::Void) => "void".into(),
        Some(Type::Half) => "half".into(),
        Some(Type::Float) => "float".into(),
        Some(Type::Double) => "double".into(),
        Some(Type::Label) => "label".into(),
        Some(Type::Metadata) => "metadata".into(),
        Some(Type::Opaque) => "opaque".into(),
        Some(Type::Int(w)) => format!("i{w}"),
        Some(Type::Pointer { to, space }) => {
            if *space == 0 {
                format!("{}*", type_name(m, *to))
            } else {
                format!("{} addrspace({space})*", type_name(m, *to))
            }
        }
        Some(Type::Array { len, of }) => format!("[{len} x {}]", type_name(m, *of)),
        Some(Type::Vector { len, of }) => format!("<{len} x {}>", type_name(m, *of)),
        Some(Type::Struct { name, fields }) => {
            if name.is_empty() {
                let f: alloc::vec::Vec<String> = fields.iter().map(|&x| type_name(m, x)).collect();
                format!("{{{}}}", f.join(", "))
            } else {
                format!("%{name}")
            }
        }
        Some(Type::Function { ret, params, .. }) => {
            let p: alloc::vec::Vec<String> = params.iter().map(|&x| type_name(m, x)).collect();
            format!("{} ({})", type_name(m, *ret), p.join(", "))
        }
    }
}

/// A value as text: a constant's literal, a function's name, or `%id`.
#[must_use]
pub fn value_name(m: &Module, body: Option<&Body>, v: u32) -> String {
    let val = m
        .values
        .get(v as usize)
        .or_else(|| body.and_then(|b| b.local(v)));
    match val.map(|x| (&x.kind, x.ty)) {
        Some((ValueKind::Constant(c), ty)) => match c {
            Const::Int(n) => format!("{n}"),
            Const::Float(b) => match m.types.get(ty as usize) {
                Some(Type::Double) => format!("{:?}", f64::from_bits(*b)),
                Some(Type::Half) => format!("half:{b:#06x}"),
                _ => format!("{:?}", f32::from_bits(*b as u32)),
            },
            Const::Null => "null".into(),
            Const::Undef => "undef".into(),
            Const::Aggregate(a) => format!("agg{a:?}"),
            Const::Data(d) => format!("data{d:?}"),
            Const::Str(s) => format!("{:?}", String::from_utf8_lossy(s)),
            Const::Expr(c, o) => format!("expr{c}{o:?}"),
        },
        Some((ValueKind::Function { index }, _)) => format!("@{}", m.functions[*index].name),
        _ => format!("%{v}"),
    }
}

/// The whole module as text.
#[must_use]
pub fn dump(m: &Module) -> String {
    let mut s = String::new();
    for (i, t) in m.types.iter().enumerate() {
        if let Type::Struct { name, fields } = t {
            let f: alloc::vec::Vec<String> = fields.iter().map(|&x| type_name(m, x)).collect();
            s += &format!("type #{i} %{name} = {{ {} }}\n", f.join(", "));
        }
    }
    for f in &m.functions {
        let Some(body) = &f.body else {
            s += &format!("declare {} @{}\n", type_name(m, f.ty), f.name);
            continue;
        };
        s += &format!("define {} @{} {{\n", type_name(m, f.ty), f.name);
        let mut next_block = 0usize;
        for (k, (id, inst)) in body.insts.iter().enumerate() {
            while body.block_starts.get(next_block) == Some(&k) {
                s += &format!("bb{next_block}:\n");
                next_block += 1;
            }
            s += "  ";
            if *id != u32::MAX {
                s += &format!("%{id} = ");
            }
            s += &inst_text(m, Some(body), inst);
            s += "\n";
        }
        s += "}\n";
    }
    for (name, ids) in &m.metadata.named {
        s += &format!("!{name} = {ids:?}\n");
    }
    for (id, n) in m.metadata.nodes.iter().enumerate() {
        s += &match n {
            MdNode::Str(x) => format!("!{id} = {x:?}\n"),
            MdNode::Node(ops) => {
                let o: alloc::vec::Vec<String> = ops.iter().map(|&x| md_op(m, x)).collect();
                format!("!{id} = !{{{}}}\n", o.join(", "))
            }
            MdNode::Value { ty, value } => format!(
                "!{id} = value {} {}\n",
                type_name(m, *ty),
                value_name(m, None, *value)
            ),
            MdNode::Other(c) => format!("!{id} = other{c}\n"),
        };
    }
    s
}

/// One instruction as text.
#[must_use]
pub fn inst_text(m: &Module, body: Option<&Body>, i: &Inst) -> String {
    let v = |x: &u32| value_name(m, body, *x);
    match i {
        Inst::Binop { op, lhs, rhs } => format!("binop{op} {}, {}", v(lhs), v(rhs)),
        Inst::Cast { op, val, to } => format!("cast{op} {} to {}", v(val), type_name(m, *to)),
        Inst::Gep { base, idx, .. } => {
            let ix: alloc::vec::Vec<String> = idx.iter().map(v).collect();
            format!("gep {}, {}", v(base), ix.join(", "))
        }
        Inst::Select { cond, t, f } => format!("select {}, {}, {}", v(cond), v(t), v(f)),
        Inst::ExtractElt { vec, idx } => format!("extractelement {}, {}", v(vec), v(idx)),
        Inst::InsertElt { vec, elt, idx } => {
            format!("insertelement {}, {}, {}", v(vec), v(elt), v(idx))
        }
        Inst::Shuffle { a, b, mask } => format!("shufflevector {}, {}, {}", v(a), v(b), v(mask)),
        Inst::Cmp { pred, lhs, rhs } => format!("cmp{pred} {}, {}", v(lhs), v(rhs)),
        Inst::Ret(None) => "ret void".into(),
        Inst::Ret(Some(x)) => format!("ret {}", v(x)),
        Inst::Br { t, f, cond: None } => {
            let _ = f;
            format!("br bb{t}")
        }
        Inst::Br {
            t,
            f,
            cond: Some(c),
        } => format!("br {}, bb{t}, bb{f}", v(c)),
        Inst::Switch {
            cond,
            default,
            cases,
        } => format!("switch {}, default bb{default}, {} cases", v(cond), cases.len()),
        Inst::Unreachable => "unreachable".into(),
        Inst::Phi { incoming, .. } => {
            let p: alloc::vec::Vec<String> = incoming
                .iter()
                .map(|(x, b)| format!("[{}, bb{b}]", v(x)))
                .collect();
            format!("phi {}", p.join(", "))
        }
        Inst::Alloca { ty, .. } => format!("alloca {}", type_name(m, *ty)),
        Inst::Load { ptr, ty } => format!("load {} {}", type_name(m, *ty), v(ptr)),
        Inst::Store { ptr, val } => format!("store {}, {}", v(val), v(ptr)),
        Inst::ExtractVal { agg, idx } => format!("extractvalue {}, {idx:?}", v(agg)),
        Inst::InsertVal { agg, val, idx } => {
            format!("insertvalue {}, {}, {idx:?}", v(agg), v(val))
        }
        Inst::Call { callee, args } => {
            let a: alloc::vec::Vec<String> = args.iter().map(v).collect();
            format!("call {}({})", v(callee), a.join(", "))
        }
        Inst::Other(c) => format!("record {c}"),
    }
}

fn md_op(m: &Module, x: u32) -> String {
    use crate::ir::MdRef;
    match m.md_operand(x) {
        MdRef::Null => "null".into(),
        MdRef::Str(s) => format!("{s:?}"),
        MdRef::Int(n) => format!("{n}"),
        MdRef::Value(v) => value_name(m, None, v),
        MdRef::Node(_) => format!("!{}", x - 1),
    }
}
