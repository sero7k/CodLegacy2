//! The world's vertex layer stream: the second vertex stream layered
//! (blended) world surfaces read their extra texture coordinates, layer
//! normals and blend weights from (`GfxWorldDraw::vld`), each surface's
//! run at its `vertexLayerData` byte offset, its layout set by its
//! technique set's world vertex format.
//!
//! Ghosts' vertex declarations are named by their routing: `pp.fcc0.3t0t0.
//! fnn.ftt2.ft1t3.fx0t5.fw0td.f` is position to position, colour to COLOR0,
//! texture set 0 to TEXCOORD0, normal, tangent to TEXCOORD2, texture set 1
//! to TEXCOORD3, `x0` to TEXCOORD5, `w0` to TEXCOORD13 (the destination's
//! index in hex), each pair followed by the component mask it is read
//! with (`f` all four, `3` two, `7` three). The world vertex itself holds
//! position, colour, texture set 0 (with the lightmap coordinates), normal
//! and tangent; every other source is in the layer stream. As in
//! `render_h1::layers` the layout is inferred from what the declarations
//! read: texture sets as two floats (four when no pass reads two),
//! everything else four bytes (UNORM8x4), in source order (texture sets,
//! then `x`, then `w`); checked against the stream's own run lengths
//! (`Scene::load` logs the formats whose stride does not match).

use std::collections::{BTreeMap, HashMap};

use crate::pool::Pool;

/// One routing of a declaration: (source, destination, mask).
pub type Route = (String, String, u8);

/// A declaration's routing from its name.
pub fn parse_decl(name: &str) -> Vec<Route> {
    let toks: Vec<&str> = name.trim_start_matches(',').split('.').collect();
    let mut pairs: Vec<String> = Vec::new();
    let mut masks: Vec<u8> = Vec::new();
    for (k, t) in toks.iter().enumerate() {
        let (mask, pair) = if k == 0 { (None, *t) } else { (t.get(..1), t.get(1..).unwrap_or("")) };
        if let Some(m) = mask {
            masks.push(u8::from_str_radix(m, 16).unwrap_or(0xf));
        }
        if !pair.is_empty() {
            pairs.push(pair.to_ascii_lowercase());
        }
    }
    pairs
        .iter()
        .zip(masks.iter().chain(std::iter::repeat(&0xf)))
        .filter_map(|(p, &m)| {
            let b = p.as_bytes();
            let n = if b.len() > 1 && b[1].is_ascii_digit() { 2 } else { 1 };
            let (src, dst) = p.split_at(n.min(p.len()));
            (!dst.is_empty()).then(|| (src.to_owned(), dst.to_owned(), m))
        })
        .collect()
}

/// The destination a vertex shader input is routed to, as a declaration
/// names it.
pub fn destination(semantic: &str, index: u32) -> Option<String> {
    Some(match (semantic.to_ascii_uppercase().as_str(), index) {
        ("POSITION" | "SV_POSITION", 0) => "p".into(),
        ("NORMAL", 0) => "n".into(),
        ("COLOR", n) => format!("c{n}"),
        ("TEXCOORD", n) if n < 16 => format!("t{n:x}"),
        _ => return None,
    })
}

/// Whether a source is in the world vertex itself.
pub fn in_world_vertex(src: &str) -> bool {
    matches!(src, "p" | "c" | "c0" | "t0" | "n" | "t")
}

/// The world vertex input a main-stream source is (semantic, index).
pub fn world_source(src: &str) -> Option<(&'static str, u32)> {
    Some(match src {
        "p" => ("POSITION", 0),
        "c" | "c0" => ("COLOR", 0),
        "t0" => ("TEXCOORD", 0),
        "n" => ("NORMAL", 0),
        "t" => ("TANGENT", 0),
        _ => return None,
    })
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Layout {
    /// (source, byte offset, format).
    pub components: Vec<(String, u64, wgpu::VertexFormat)>,
    pub stride: u64,
}

impl Layout {
    pub fn find(&self, src: &str) -> Option<(u64, wgpu::VertexFormat)> {
        self.components.iter().find(|c| c.0 == src).map(|c| (c.1, c.2))
    }
}

/// Sources in their stream order: texture sets, then `x`, then `w` (each
/// by its number), then anything else by name.
fn order(src: &str) -> (u8, u8, String) {
    let class = match src.as_bytes().first() {
        Some(b't') => 0,
        Some(b'x') => 1,
        Some(b'w') => 2,
        _ => 3,
    };
    let n = src[1..].parse::<u8>().unwrap_or(0);
    (class, n, src.to_owned())
}

/// The layout of every world vertex format the pool's technique sets use.
pub fn layouts(pool: &Pool) -> HashMap<u8, Layout> {
    let mut masks: HashMap<u8, BTreeMap<(u8, u8, String), u8>> = HashMap::new();
    for (ts, slots) in &pool.techsets {
        let Some(&f) = pool.world_vert_format.get(ts) else { continue };
        if f == 0 {
            continue;
        }
        let m = masks.entry(f).or_default();
        for (_, tn) in slots {
            let Some(t) = pool.techniques.get(tn) else { continue };
            for pass in &t.passes {
                let Some(d) = pass.vertex_decl.as_deref() else { continue };
                for (src, _, mask) in parse_decl(d) {
                    if in_world_vertex(&src) {
                        continue;
                    }
                    let e = m.entry(order(&src)).or_insert(mask);
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
            for ((class, _, src), mask) in m {
                let (size, format) = match class {
                    0 if mask & 0xc == 0 => (8, wgpu::VertexFormat::Float32x2),
                    0 => (16, wgpu::VertexFormat::Float32x4),
                    _ => (4, wgpu::VertexFormat::Unorm8x4),
                };
                l.components.push((src, l.stride, format));
                l.stride += size;
            }
            (f, l)
        })
        .collect()
}

/// The layouts with each format's stride as the layer stream's runs give
/// it (`Scene::layer_strides`). Where the stream is longer than what the
/// declarations read, the components no technique reads are taken to
/// follow the read ones (INFERRED: their place is not seen; the read ones
/// keep their offsets). Returns the formats whose stride changed, as
/// (format, inferred, read).
pub fn with_strides(layouts: &mut HashMap<u8, Layout>, strides: &HashMap<u8, u32>) -> Vec<(u8, u64, u64)> {
    let mut changed = Vec::new();
    for (f, l) in layouts.iter_mut() {
        if let Some(&st) = strides.get(f)
            && u64::from(st) != l.stride
            && u64::from(st) > l.components.last().map_or(0, |c| c.1)
        {
            changed.push((*f, l.stride, u64::from(st)));
            l.stride = u64::from(st);
        }
    }
    changed.sort_unstable();
    changed
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn declaration_names_route() {
        let r = parse_decl("pp.fcc0.3t0t0.fnn.ftt2.ft1t3.fx0t5.fw0td.f");
        assert_eq!(
            r,
            vec![
                ("p".into(), "p".into(), 0xf),
                ("c".into(), "c0".into(), 0x3),
                ("t0".into(), "t0".into(), 0xf),
                ("n".into(), "n".into(), 0xf),
                ("t".into(), "t2".into(), 0xf),
                ("t1".into(), "t3".into(), 0xf),
                ("x0".into(), "t5".into(), 0xf),
                ("w0".into(), "td".into(), 0xf),
            ]
        );
        let m = parse_decl("pp.ft0t0.3nn.ftt2.f");
        assert_eq!(m[1], ("t0".into(), "t0".into(), 0x3));
        assert_eq!(destination("TEXCOORD", 13).as_deref(), Some("td"));
        assert_eq!(destination("TEXCOORD", 3).as_deref(), Some("t3"));
    }

    #[test]
    fn the_streams_stride_wins() {
        let mut l: HashMap<u8, Layout> = HashMap::new();
        l.insert(
            18,
            Layout {
                components: vec![("t1".into(), 0, wgpu::VertexFormat::Float32x4), ("y0".into(), 28, wgpu::VertexFormat::Unorm8x4)],
                stride: 32,
            },
        );
        let changed = with_strides(&mut l, &[(18u8, 36u32), (2, 12)].into_iter().collect());
        assert_eq!(changed, vec![(18, 32, 36)]);
        assert_eq!(l[&18].stride, 36);
        assert_eq!(l[&18].find("y0"), Some((28, wgpu::VertexFormat::Unorm8x4)));
    }
}
