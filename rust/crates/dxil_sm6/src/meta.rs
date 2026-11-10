//! What the metadata says about a program: its stage, its resource ranges
//! (`dx.resources`) and its input/output signatures (`dx.entryPoints`).

use alloc::string::{String, ToString};
use alloc::vec::Vec;

use crate::ir::{MdRef, Module};

/// The pipeline stage of the entry point.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stage {
    Vertex,
    Pixel,
    Geometry,
    Hull,
    Domain,
    Compute,
    Library,
}

/// A resource class (the `createHandle` class operand).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Class {
    Srv = 0,
    Uav = 1,
    Cbv = 2,
    Sampler = 3,
}

/// A declared resource range.
#[derive(Clone, Debug)]
pub struct Range {
    pub class: Class,
    /// The range id `createHandle` names.
    pub id: u32,
    pub space: u32,
    pub lower: u32,
    /// Number of descriptors in the range.
    pub size: u32,
    /// SRV/UAV resource kind (1 1D, 2 2D, 3 2D-MS, 4 3D, 5 cube, 6 1D array,
    /// 7 2D array, 8 2D-MS array, 9 cube array, 10 typed buffer, 11 raw
    /// buffer, 12 structured buffer); 0 for the other classes.
    pub kind: u32,
    /// SRV/UAV element component type (DXIL: 4 i32, 5 u32, 9 f32 ...).
    pub elem: u32,
    /// Constant buffer size in bytes.
    pub cb_bytes: u32,
    /// Structured buffer stride.
    pub stride: u32,
    /// Sampler kind (0 default, 1 comparison, 2 mono).
    pub sampler_kind: u32,
    /// The resource's declared type name (e.g. `class.Buffer<vector<float, 4> >`).
    pub type_name: String,
}

/// One signature element.
#[derive(Clone, Debug)]
pub struct SigElement {
    pub id: u32,
    pub name: String,
    /// DXIL component type (4 i32, 5 u32, 9 f32 ...).
    pub comp: u32,
    /// DXIL semantic kind (0 arbitrary, 1 vertex id, 2 instance id, 3
    /// position, 13 front face, 14 coverage, 16 target, 17 depth ...).
    pub sem: u32,
    pub semantic_index: u32,
    /// Interpolation mode (0 undefined, 1 constant, 2 linear, 3 centroid,
    /// 4 no-perspective, ...).
    pub interp: u32,
    pub rows: u32,
    pub cols: u32,
    pub start_row: u32,
    pub start_col: u32,
}

/// The entry point's facts.
#[derive(Clone, Debug)]
pub struct Entry {
    pub name: String,
    pub stage: Stage,
    pub inputs: Vec<SigElement>,
    pub outputs: Vec<SigElement>,
    pub ranges: Vec<Range>,
}

fn node<'a>(m: &'a Module, id_plus_one: u32) -> Option<&'a [u32]> {
    match m.md_operand(id_plus_one) {
        MdRef::Node(n) => Some(n),
        _ => None,
    }
}

fn int(m: &Module, id_plus_one: u32) -> u32 {
    match m.md_operand(id_plus_one) {
        MdRef::Int(n) => n as u32,
        _ => 0,
    }
}

fn string(m: &Module, id_plus_one: u32) -> String {
    match m.md_operand(id_plus_one) {
        MdRef::Str(s) => s.to_string(),
        _ => String::new(),
    }
}

/// The name of the struct a resource record's global points to (through
/// arrays of resources).
fn resource_type_name(m: &Module, op: u32) -> String {
    use crate::ir::{MdNode, Type};
    let Some(id) = op.checked_sub(1) else { return String::new() };
    let Some(MdNode::Value { ty, .. }) = m.metadata.nodes.get(id as usize) else {
        return String::new();
    };
    let mut t = *ty;
    for _ in 0..4 {
        match m.types.get(t as usize) {
            Some(Type::Pointer { to, .. }) => t = *to,
            Some(Type::Array { of, .. }) => t = *of,
            Some(Type::Struct { name, .. }) => return name.clone(),
            _ => break,
        }
    }
    String::new()
}

/// Reads the entry point's stage, signatures and resource ranges.
#[must_use]
pub fn entry(m: &Module) -> Option<Entry> {
    let stage = m
        .metadata
        .named
        .iter()
        .find(|(n, _)| n == "dx.shaderModel")
        .and_then(|(_, ids)| ids.first())
        .and_then(|&id| node(m, id + 1))
        .map(|ops| match string(m, ops[0]).as_str() {
            "vs" => Stage::Vertex,
            "ps" => Stage::Pixel,
            "gs" => Stage::Geometry,
            "hs" => Stage::Hull,
            "ds" => Stage::Domain,
            "cs" => Stage::Compute,
            _ => Stage::Library,
        })?;
    let eid = m
        .metadata
        .named
        .iter()
        .find(|(n, _)| n == "dx.entryPoints")
        .and_then(|(_, ids)| ids.first())
        .copied()?;
    let ops = node(m, eid + 1)?;
    let name = string(m, *ops.get(1)?);
    let (mut inputs, mut outputs) = (Vec::new(), Vec::new());
    if let Some(sigs) = ops.get(2).and_then(|&s| node(m, s)) {
        for (k, out) in [(0usize, false), (1, true)] {
            let list = sigs.get(k).copied().and_then(|s| node(m, s));
            for &e in list.unwrap_or(&[]) {
                let Some(f) = node(m, e) else { continue };
                if f.len() < 10 {
                    continue;
                }
                let el = SigElement {
                    id: int(m, f[0]),
                    name: string(m, f[1]),
                    comp: int(m, f[2]),
                    sem: int(m, f[3]),
                    semantic_index: node(m, f[4]).and_then(|x| x.first()).map_or(0, |&x| int(m, x)),
                    interp: int(m, f[5]),
                    rows: int(m, f[6]),
                    cols: int(m, f[7]),
                    start_row: int(m, f[8]),
                    start_col: int(m, f[9]),
                };
                if out {
                    outputs.push(el);
                } else {
                    inputs.push(el);
                }
            }
        }
    }
    let mut ranges = Vec::new();
    if let Some(res) = ops.get(3).and_then(|&r| node(m, r)) {
        for (k, class) in [Class::Srv, Class::Uav, Class::Cbv, Class::Sampler]
            .into_iter()
            .enumerate()
        {
            let list = res.get(k).copied().and_then(|s| node(m, s));
            for &r in list.unwrap_or(&[]) {
                let Some(f) = node(m, r) else { continue };
                if f.len() < 7 {
                    continue;
                }
                let mut rg = Range {
                    class,
                    id: int(m, f[0]),
                    space: int(m, f[3]),
                    lower: int(m, f[4]),
                    size: int(m, f[5]),
                    kind: 0,
                    elem: 9,
                    cb_bytes: 0,
                    stride: 0,
                    sampler_kind: 0,
                    type_name: resource_type_name(m, f[1]),
                };
                match class {
                    Class::Srv | Class::Uav => {
                        rg.kind = int(m, f[6]);
                        // Tags: pairs (0, element type) and (1, stride).
                        let tags = f.last().copied().and_then(|t| node(m, t));
                        if let Some(t) = tags {
                            for p in t.chunks(2) {
                                if p.len() == 2 {
                                    match int(m, p[0]) {
                                        0 => rg.elem = int(m, p[1]),
                                        1 => rg.stride = int(m, p[1]),
                                        _ => {}
                                    }
                                }
                            }
                        }
                    }
                    Class::Cbv => rg.cb_bytes = int(m, f[6]),
                    Class::Sampler => rg.sampler_kind = int(m, f[6]),
                }
                ranges.push(rg);
            }
        }
    }
    Some(Entry {
        name,
        stage,
        inputs,
        outputs,
        ranges,
    })
}
