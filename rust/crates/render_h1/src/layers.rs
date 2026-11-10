//! The world's vertex layer stream: a second vertex stream that layered
//! (blended) world surfaces read their extra texture coordinates, layer
//! normals and blend weights from.
//!
//! Each surface's run starts at its `vertexLayerData` byte offset; its
//! layout is set by its technique set's world vertex format. No file
//! states that layout as a table, but the passes' vertex declarations name
//! every component a format carries: the components are laid out in
//! source order, texture coordinates (sources 6, 7, 8) as two or four
//! floats, everything else as four bytes (UNORM8x4: layer normals,
//! blend weights). A component read both as two and as four floats is two
//! (a pass reading .zw of it gets the default). This reproduces the stride
//! of every format on retail Crash (8 to 60 bytes).
//!
//! Vertex declaration routing: source 0 position, 1 colour, 2 texcoord,
//! 3 normal, 4 tangent, 6 and up the layer stream; destination 0
//! POSITION, 1 NORMAL, 2 COLOR0, 3 COLOR1, 6 + n TEXCOORDn.

use std::collections::{BTreeMap, HashMap};

use crate::pool::{Pool, key};

/// First routing source that lives in the layer stream.
pub const FIRST_LAYER_SOURCE: u8 = 6;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Layout {
    /// (source, byte offset, format).
    pub components: Vec<(u8, u64, wgpu::VertexFormat)>,
    pub stride: u64,
}

impl Layout {
    pub fn find(&self, source: u8) -> Option<(u64, wgpu::VertexFormat)> {
        self.components
            .iter()
            .find(|c| c.0 == source)
            .map(|c| (c.1, c.2))
    }
}

/// The layout of every world vertex format the pool's technique sets use.
pub fn layouts(pool: &Pool) -> HashMap<u8, Layout> {
    // Per format: source → widest... narrowest mask any pass reads.
    let mut masks: HashMap<u8, BTreeMap<u8, u8>> = HashMap::new();
    for (ts, slots) in &pool.techsets {
        let Some(&f) = pool.world_vert_format.get(ts) else {
            continue;
        };
        if f == 0 {
            continue;
        }
        let m = masks.entry(f).or_default();
        for (_, tn) in slots {
            let Some(t) = pool.techniques.get(tn) else {
                continue;
            };
            for pass in &t.pass_info {
                let Some(r) = pass
                    .vertex_decl
                    .as_deref()
                    .and_then(|d| pool.decls.get(&key(d)))
                else {
                    continue;
                };
                for &(src, _, mask) in r {
                    if src >= FIRST_LAYER_SOURCE {
                        let e = m.entry(src).or_insert(mask);
                        // Two floats when any pass reads it as two.
                        if mask.count_ones() < e.count_ones() {
                            *e = mask;
                        }
                    }
                }
            }
        }
    }
    masks
        .into_iter()
        .map(|(f, m)| {
            let mut l = Layout::default();
            for (src, mask) in m {
                let (size, format) = match src {
                    6..=8 if mask & 0xc == 0 => (8, wgpu::VertexFormat::Float32x2),
                    6..=8 => (16, wgpu::VertexFormat::Float32x4),
                    _ => (4, wgpu::VertexFormat::Unorm8x4),
                };
                l.components.push((src, l.stride, format));
                l.stride += size;
            }
            (f, l)
        })
        .collect()
}

/// A vertex shader input's routing destination.
pub fn destination(semantic: &str, index: u32) -> Option<u8> {
    Some(match (semantic.to_ascii_uppercase().as_str(), index) {
        ("POSITION" | "SV_POSITION", 0) => 0,
        ("NORMAL", 0) => 1,
        ("COLOR", 0) => 2,
        ("COLOR", 1) => 3,
        ("TEXCOORD", n) if n < 32 => 6 + n as u8,
        _ => return None,
    })
}

/// Where a vertex shader input comes from in the layer stream, if it does:
/// the routing of `decl` maps the input's destination to a source.
pub fn layer_input(
    decl: Option<&[(u8, u8, u8)]>,
    layout: &Layout,
    semantic: &str,
    index: u32,
) -> Option<(u64, wgpu::VertexFormat)> {
    let dest = destination(semantic, index)?;
    let &(src, _, _) = decl?.iter().find(|r| r.1 == dest)?;
    if src < FIRST_LAYER_SOURCE {
        return None;
    }
    layout.find(src)
}
