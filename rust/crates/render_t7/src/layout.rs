//! BlackOps3.exe's input layouts: per vertex declaration (count, component
//! ids) it makes twelve, one per layout group; each component id is found in
//! the group's table (the exe's), giving the stream slot, step, byte offset
//! and format; the semantic is the id's (`SEMANTICS`). Every layout also
//! takes `TEXCOORD15` (R32_UINT, slot 15, per instance): the instance index.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Element {
    pub id: u8,
    pub slot: u8,
    pub step: u8,
    pub offset: u32,
    pub dxgi: u32,
}

/// The component ids' semantics (name, index), as the exe names them.
pub const SEMANTICS: [(&str, u32); 32] = [
    ("POSITION", 0), ("NORMAL", 0), ("TANGENT", 0), ("COLOR", 0), ("COLOR", 1), ("COLOR", 2), ("COLOR", 3), ("COLOR", 4),
    ("COLOR", 5), ("DEPTH", 0), ("TEXCOORD", 0), ("TEXCOORD", 1), ("TEXCOORD", 2), ("TEXCOORD", 3), ("TEXCOORD", 4), ("TEXCOORD", 5),
    ("TEXCOORD", 6), ("TEXCOORD", 7), ("TEXCOORD", 8), ("TEXCOORD", 9), ("TEXCOORD", 10), ("TEXCOORD", 11), ("TEXCOORD", 12), ("TEXCOORD", 13),
    ("OFFPOSITION", 0), ("TENSION", 0), ("UVOFFSETS", 0), ("SHARD_INDEX", 0), ("BLENDWEIGHT", 0), ("BLENDINDICES", 0), ("BENTNORMAL", 0), ("SV_DISTANCE", 0),
];

/// The world's streams (positions; colour, UV, normal, tangent).
pub const WORLD_GROUP: usize = 3;
/// A model's streams (positions; colour, half UV, normal, tangent; weights).
pub const MODEL_GROUP: usize = 1;
/// The instance index stream.
pub const INSTANCE_SLOT: u8 = 15;

/// Stream strides by slot for the groups drawn (the exe's vertex buffers).
pub fn stride(group: usize, slot: u8) -> Option<u64> {
    match (group, slot) {
        (WORLD_GROUP, 0) => Some(12),
        (WORLD_GROUP, 1) => Some(20),
        (MODEL_GROUP, 0) => Some(12),
        (MODEL_GROUP, 1) => Some(16),
        (MODEL_GROUP, 2) => Some(12),
        (_, INSTANCE_SLOT) => Some(4),
        // (The effects' sprite records, one per instance: `crate::fx`.)
        (8, 0) => Some(64),
        _ => None,
    }
}

pub fn vertex_format(dxgi: u32) -> Option<wgpu::VertexFormat> {
    use wgpu::VertexFormat as V;
    Some(match dxgi {
        2 => V::Float32x4,
        3 => V::Uint32x4,
        6 => V::Float32x3,
        7 => V::Uint32x3,
        10 => V::Float16x4,
        11 => V::Unorm16x4,
        12 => V::Uint16x4,
        13 => V::Snorm16x4,
        14 => V::Sint16x4,
        16 => V::Float32x2,
        24 => V::Unorm10_10_10_2,
        28 => V::Unorm8x4,
        34 => V::Float16x2,
        42 => V::Uint32,
        57 => V::Uint16,
        _ => return None,
    })
}

/// One stream of a pipeline: its D3D slot and attributes.
#[derive(Clone, Debug)]
pub struct Stream {
    pub slot: u8,
    pub stride: u64,
    pub step: wgpu::VertexStepMode,
    pub attributes: Vec<wgpu::VertexAttribute>,
}

/// The streams feeding a vertex program's inputs (semantic, index,
/// location) from a declaration in a layout group, as the exe's layout for
/// that group would.
/// `inputs`: (semantic, index, location, signature component type: 1
/// uint, 2 sint, 3 float). An integer format whose signedness differs from
/// the program's is read with the program's (D3D reinterprets the bits; the
/// values stored, bone indices, are all below 0x8000 where both agree).
pub fn streams(decl: &[u8], group: usize, inputs: &[(String, u32, u32, u32)]) -> Result<Vec<Stream>, String> {
    let count = *decl.first().ok_or("empty declaration")? as usize;
    let ids = decl.get(2..2 + count).ok_or("short declaration")?;
    let table = GROUPS.get(group).ok_or("layout group")?;
    let mut out: Vec<Stream> = Vec::new();
    for (semantic, index, location, component) in inputs {
        let sem = semantic.to_ascii_uppercase();
        let (slot, step, offset, dxgi) = if sem == "TEXCOORD" && *index == 15 {
            (INSTANCE_SLOT, 1u8, 0u32, 42u32)
        } else {
            let id = ids
                .iter()
                .copied()
                .find(|&id| SEMANTICS.get(id as usize).is_some_and(|(n, i)| *n == sem && i == index))
                .ok_or_else(|| format!("input {sem}{index} not in the declaration {ids:02x?}"))?;
            let e = table.iter().find(|e| e.id == id).ok_or_else(|| format!("component {id} not in layout group {group}"))?;
            (e.slot, e.step, e.offset, e.dxgi)
        };
        let format = vertex_format(dxgi).ok_or_else(|| format!("{sem}{index}: DXGI {dxgi} as a vertex format"))?;
        let format = match (format, component) {
            (wgpu::VertexFormat::Sint16x4, 1) => wgpu::VertexFormat::Uint16x4,
            (wgpu::VertexFormat::Uint16x4, 2) => wgpu::VertexFormat::Sint16x4,
            (f, _) => f,
        };
        let stride = stride(group, slot).ok_or_else(|| format!("{sem}{index}: no stream {slot} in group {group}"))?;
        let at = match out.iter().position(|s| s.slot == slot) {
            Some(k) => k,
            None => {
                out.push(Stream {
                    slot,
                    stride,
                    step: if step != 0 { wgpu::VertexStepMode::Instance } else { wgpu::VertexStepMode::Vertex },
                    attributes: Vec::new(),
                });
                out.len() - 1
            }
        };
        out[at].attributes.push(wgpu::VertexAttribute { format, offset: u64::from(offset), shader_location: *location });
    }
    out.sort_by_key(|s| s.slot);
    Ok(out)
}

/// The exe's twelve layout tables (component id, slot, step, offset, DXGI).
#[rustfmt::skip]
pub const GROUPS: [&[Element]; 12] = [
    &[Element { id: 0, slot: 0, step: 0, offset: 0, dxgi: 2 }, Element { id: 1, slot: 0, step: 0, offset: 28, dxgi: 24 }, Element { id: 3, slot: 0, step: 0, offset: 16, dxgi: 28 }, Element { id: 10, slot: 0, step: 0, offset: 20, dxgi: 16 }, Element { id: 28, slot: 1, step: 0, offset: 0, dxgi: 28 }, Element { id: 29, slot: 1, step: 0, offset: 4, dxgi: 14 }],
    &[Element { id: 0, slot: 0, step: 0, offset: 0, dxgi: 6 }, Element { id: 1, slot: 1, step: 0, offset: 8, dxgi: 24 }, Element { id: 2, slot: 1, step: 0, offset: 12, dxgi: 24 }, Element { id: 3, slot: 1, step: 0, offset: 0, dxgi: 28 }, Element { id: 10, slot: 1, step: 0, offset: 4, dxgi: 34 }, Element { id: 28, slot: 2, step: 0, offset: 0, dxgi: 28 }, Element { id: 29, slot: 2, step: 0, offset: 4, dxgi: 14 }],
    &[Element { id: 0, slot: 0, step: 0, offset: 0, dxgi: 2 }, Element { id: 1, slot: 0, step: 0, offset: 24, dxgi: 24 }, Element { id: 2, slot: 0, step: 0, offset: 28, dxgi: 24 }, Element { id: 3, slot: 0, step: 0, offset: 16, dxgi: 28 }, Element { id: 10, slot: 0, step: 0, offset: 20, dxgi: 34 }, Element { id: 28, slot: 1, step: 0, offset: 0, dxgi: 28 }, Element { id: 29, slot: 1, step: 0, offset: 4, dxgi: 14 }],
    &[Element { id: 0, slot: 0, step: 0, offset: 0, dxgi: 6 }, Element { id: 1, slot: 1, step: 0, offset: 12, dxgi: 24 }, Element { id: 2, slot: 1, step: 0, offset: 16, dxgi: 24 }, Element { id: 3, slot: 1, step: 0, offset: 0, dxgi: 28 }, Element { id: 10, slot: 1, step: 0, offset: 4, dxgi: 16 }, Element { id: 28, slot: 2, step: 0, offset: 0, dxgi: 28 }, Element { id: 29, slot: 2, step: 0, offset: 4, dxgi: 14 }],
    &[Element { id: 0, slot: 0, step: 0, offset: 0, dxgi: 6 }, Element { id: 10, slot: 0, step: 0, offset: 12, dxgi: 16 }, Element { id: 28, slot: 1, step: 0, offset: 0, dxgi: 14 }, Element { id: 29, slot: 1, step: 0, offset: 4, dxgi: 28 }],
    &[Element { id: 0, slot: 0, step: 1, offset: 0, dxgi: 13 }, Element { id: 28, slot: 1, step: 0, offset: 0, dxgi: 14 }, Element { id: 29, slot: 1, step: 0, offset: 4, dxgi: 28 }],
    &[Element { id: 0, slot: 0, step: 0, offset: 0, dxgi: 6 }, Element { id: 3, slot: 0, step: 0, offset: 12, dxgi: 28 }, Element { id: 28, slot: 1, step: 0, offset: 0, dxgi: 14 }, Element { id: 29, slot: 1, step: 0, offset: 4, dxgi: 28 }],
    &[Element { id: 0, slot: 0, step: 0, offset: 0, dxgi: 6 }, Element { id: 10, slot: 0, step: 0, offset: 12, dxgi: 34 }, Element { id: 28, slot: 1, step: 0, offset: 0, dxgi: 14 }, Element { id: 29, slot: 1, step: 0, offset: 4, dxgi: 28 }],
    &[Element { id: 0, slot: 0, step: 1, offset: 0, dxgi: 2 }, Element { id: 1, slot: 0, step: 1, offset: 32, dxgi: 24 }, Element { id: 3, slot: 0, step: 1, offset: 16, dxgi: 28 }, Element { id: 10, slot: 0, step: 1, offset: 20, dxgi: 7 }, Element { id: 11, slot: 0, step: 1, offset: 36, dxgi: 24 }, Element { id: 12, slot: 0, step: 1, offset: 40, dxgi: 2 }, Element { id: 13, slot: 0, step: 1, offset: 56, dxgi: 12 }],
    &[Element { id: 0, slot: 0, step: 0, offset: 0, dxgi: 6 }, Element { id: 1, slot: 0, step: 0, offset: 20, dxgi: 24 }, Element { id: 2, slot: 0, step: 0, offset: 24, dxgi: 24 }, Element { id: 3, slot: 0, step: 0, offset: 12, dxgi: 28 }, Element { id: 10, slot: 0, step: 0, offset: 16, dxgi: 34 }, Element { id: 27, slot: 0, step: 0, offset: 28, dxgi: 57 }, Element { id: 28, slot: 1, step: 0, offset: 0, dxgi: 28 }, Element { id: 29, slot: 1, step: 0, offset: 4, dxgi: 14 }],
    &[Element { id: 0, slot: 0, step: 0, offset: 0, dxgi: 2 }, Element { id: 10, slot: 0, step: 0, offset: 16, dxgi: 3 }],
    &[Element { id: 0, slot: 0, step: 0, offset: 0, dxgi: 6 }, Element { id: 1, slot: 0, step: 0, offset: 24, dxgi: 24 }, Element { id: 2, slot: 0, step: 0, offset: 28, dxgi: 24 }, Element { id: 3, slot: 0, step: 0, offset: 12, dxgi: 28 }, Element { id: 10, slot: 0, step: 0, offset: 16, dxgi: 16 }, Element { id: 28, slot: 1, step: 0, offset: 0, dxgi: 28 }, Element { id: 29, slot: 1, step: 0, offset: 4, dxgi: 14 }],
];
