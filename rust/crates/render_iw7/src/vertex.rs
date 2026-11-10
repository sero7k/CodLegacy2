//! Where a vertex program's inputs come from: the pass's vertex declaration
//! routes each input (by destination) to a source, read from the base
//! stream (the world's 44-byte `GfxWorldVertex` or a model's 32-byte
//! `GfxPackedVertex` with its light map uv appended) or from a second
//! stream: the world's layer stream
//! (layered surfaces) or a placed model's lighting stream.
//!
//! Read on `techsets_mp_riot` (`MaterialVertexDeclaration::routing`):
//! - destinations: 0 POSITION, 1 NORMAL, 2 COLOR0, 3 COLOR1, 8 + n
//!   TEXCOORDn;
//! - sources 0 position, 1 colour, 2 uv (and the world's lightmap uv),
//!   3 normal, 4 tangent; 6 and up the world's layer stream; 24, 25, 26 a
//!   placed model's lighting (probe stream: a packed word, four probe
//!   indices, four weights);
//! - the layer stream holds its components in source order, texture
//!   coordinates (sources 6 to 8) as two floats when a pass reads two,
//!   else four, everything else four bytes. This gives the stride of every
//!   world vertex format on riot (8 to 60 bytes, checked against the
//!   surfaces' runs).

use std::collections::{BTreeMap, HashMap};

use fastfile_iw7::walk::Capture;

/// First routing source in the world's layer stream.
pub const FIRST_LAYER_SOURCE: u8 = 6;
/// The placed model lighting stream's sources.
pub const MODEL_LIGHTING_SOURCES: [u8; 3] = [24, 25, 26];
/// Its stride (`GfxStaticModelVertexLighting`).
pub const MODEL_LIGHTING_STRIDE: u64 = 20;

/// A vertex program input's routing destination.
pub fn destination(semantic: &str, index: u32) -> Option<u8> {
    Some(match (semantic.to_ascii_uppercase().as_str(), index) {
        ("POSITION" | "SV_POSITION", 0) => 0,
        ("NORMAL", 0) => 1,
        ("COLOR", 0) => 2,
        ("COLOR", 1) => 3,
        ("TEXCOORD", n) if n < 24 => 8 + n as u8,
        _ => return None,
    })
}

/// A model's base vertex here: its `GfxPackedVertex` (32 bytes), then its
/// light map unwrap uv (two floats; zero when it has none).
pub const MODEL_STRIDE: u64 = 40;
/// The routing source of a lightmapped model's unwrap uv (read on
/// `techsets_mp_fallen`'s `ml_` passes: source 13 to TEXCOORD4, which the
/// vertex program puts through code constant 117, the placement's scale
/// and offset, to the light map uv).
pub const MODEL_LMAP_SOURCE: u8 = 13;

/// The base streams.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Base {
    /// `GfxWorldVertex`, 44 bytes.
    World,
    /// `GfxPackedVertex` (32 bytes) and the light map uv: [`MODEL_STRIDE`].
    Model,
}

impl Base {
    pub fn stride(self) -> u64 {
        match self {
            Self::World => 44,
            Self::Model => MODEL_STRIDE,
        }
    }

    /// A base source's place and format.
    pub fn source(self, src: u8) -> Option<(u64, wgpu::VertexFormat)> {
        use wgpu::VertexFormat as F;
        Some(match (self, src) {
            (_, 0) => (0, F::Float32x4),
            (_, 1) => (16, F::Unorm8x4),
            (Self::World, 2) => (20, F::Float32x4),
            (Self::World, 3) => (36, F::Unorm10_10_10_2),
            (Self::World, 4) => (40, F::Unorm10_10_10_2),
            (Self::Model, 2) => (20, F::Float16x2),
            (Self::Model, 3) => (24, F::Unorm10_10_10_2),
            (Self::Model, 4) => (28, F::Unorm10_10_10_2),
            (Self::Model, MODEL_LMAP_SOURCE) => (32, F::Float32x2),
            _ => return None,
        })
    }
}

/// The layer stream's layout of one world vertex format.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Layout {
    /// (source, byte offset, format)
    pub components: Vec<(u8, u64, wgpu::VertexFormat)>,
    pub stride: u64,
}

impl Layout {
    pub fn find(&self, src: u8) -> Option<(u64, wgpu::VertexFormat)> {
        self.components.iter().find(|c| c.0 == src).map(|c| (c.1, c.2))
    }
}

/// Every world vertex format's layer layout, from the routings of the
/// passes of the technique sets that use it.
pub fn layer_layouts(cap: &Capture) -> HashMap<u8, Layout> {
    let mut masks: HashMap<u8, BTreeMap<u8, u8>> = HashMap::new();
    for ts in &cap.techsets {
        if ts.world_vert_format == 0 {
            continue;
        }
        let m = masks.entry(ts.world_vert_format).or_default();
        for pass in ts.passes.iter().flatten() {
            let Some(r) = pass.vertex_decl.and_then(|d| cap.vertex_decls.get(&d.index)) else { continue };
            for &(src, _, mask) in r {
                if (FIRST_LAYER_SOURCE..24).contains(&src) {
                    // Two floats when any pass reads it as two.
                    let e = m.entry(src).or_insert(mask);
                    if mask.count_ones() < e.count_ones() {
                        *e = mask;
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

/// A placed model's lighting stream source.
pub fn model_lighting(src: u8) -> Option<(u64, wgpu::VertexFormat)> {
    use wgpu::VertexFormat as F;
    Some(match src {
        24 => (0, F::Uint32),
        25 => (4, F::Uint16x4),
        26 => (12, F::Unorm16x4),
        _ => return None,
    })
}

/// `format` as the program declares the input (`component_type` 1 uint,
/// 2 sint, 3 float): an integer input reads the same bytes as integers.
pub fn as_declared(format: wgpu::VertexFormat, component_type: u32) -> wgpu::VertexFormat {
    use wgpu::VertexFormat as F;
    match component_type {
        1 => match format {
            F::Unorm8x4 | F::Snorm8x4 => F::Uint8x4,
            F::Unorm16x4 | F::Float16x4 => F::Uint16x4,
            F::Float16x2 | F::Unorm16x2 => F::Uint16x2,
            F::Float32x4 => F::Uint32x4,
            F::Float32x3 => F::Uint32x3,
            F::Float32x2 => F::Uint32x2,
            F::Float32 => F::Uint32,
            F::Unorm10_10_10_2 => F::Uint32,
            f => f,
        },
        2 => match format {
            F::Unorm8x4 | F::Snorm8x4 => F::Sint8x4,
            F::Unorm16x4 | F::Float16x4 => F::Sint16x4,
            F::Float32x4 => F::Sint32x4,
            F::Float32x2 => F::Sint32x2,
            F::Float32 | F::Unorm10_10_10_2 | F::Uint32 => F::Sint32,
            f => f,
        },
        _ => match format {
            F::Uint32 => F::Float32,
            F::Uint16x4 => F::Unorm16x4,
            F::Uint8x4 => F::Unorm8x4,
            f => f,
        },
    }
}

/// Which stream an input reads.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stream {
    Base,
    /// The world's layer stream or a model's lighting stream.
    Second,
}

/// Where one input comes from: (stream, offset, format), or `None` when
/// nothing routes it.
pub fn input(base: Base, routing: Option<&[(u8, u8, u8)]>, layout: Option<&Layout>, semantic: &str, index: u32) -> Option<(Stream, u64, wgpu::VertexFormat)> {
    let dest = destination(semantic, index)?;
    let Some(routing) = routing else {
        // No declaration: the world's own order.
        let src = match dest {
            0 => 0,
            1 => 3,
            2 => 1,
            8 => 2,
            10 => 4,
            _ => return None,
        };
        return base.source(src).map(|(o, f)| (Stream::Base, o, f));
    };
    let &(src, _, _) = routing.iter().find(|r| r.1 == dest)?;
    if let Some((o, f)) = base.source(src) {
        return Some((Stream::Base, o, f));
    }
    match base {
        Base::World => layout?.find(src).map(|(o, f)| (Stream::Second, o, f)),
        Base::Model => model_lighting(src).map(|(o, f)| (Stream::Second, o, f)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn routing_finds_base_and_layer_inputs() {
        // wvf 4's declaration on riot: base sources, a layer uv (6, two
        // floats) and a layer normal (14).
        let r = [(0, 0, 15), (1, 2, 3), (2, 8, 15), (3, 1, 15), (4, 10, 15), (6, 11, 3), (14, 13, 15)];
        let l = Layout { components: vec![(6, 0, wgpu::VertexFormat::Float32x2), (14, 8, wgpu::VertexFormat::Unorm8x4)], stride: 12 };
        assert_eq!(input(Base::World, Some(&r), Some(&l), "TEXCOORD", 3), Some((Stream::Second, 0, wgpu::VertexFormat::Float32x2)));
        assert_eq!(input(Base::World, Some(&r), Some(&l), "TEXCOORD", 5), Some((Stream::Second, 8, wgpu::VertexFormat::Unorm8x4)));
        assert_eq!(input(Base::World, Some(&r), Some(&l), "NORMAL", 0), Some((Stream::Base, 36, wgpu::VertexFormat::Unorm10_10_10_2)));
        assert_eq!(input(Base::World, Some(&r), Some(&l), "TEXCOORD", 2), Some((Stream::Base, 40, wgpu::VertexFormat::Unorm10_10_10_2)));
        assert_eq!(input(Base::Model, None, None, "TEXCOORD", 0), Some((Stream::Base, 20, wgpu::VertexFormat::Float16x2)));
    }

    #[test]
    fn integer_inputs_read_integers() {
        assert_eq!(as_declared(wgpu::VertexFormat::Unorm8x4, 1), wgpu::VertexFormat::Uint8x4);
        assert_eq!(as_declared(wgpu::VertexFormat::Uint32, 3), wgpu::VertexFormat::Float32);
    }
}
