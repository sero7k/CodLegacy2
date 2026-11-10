//! A check, made on every map load, of which way the world's and the
//! models' triangles wind relative to their own vertex normals, and the
//! world's largest upward-facing (floor) surfaces with the state they are
//! drawn with. Written for the floors that do not show on Skyjacked: a
//! world whose triangles wind the other way from the models' would have
//! its floors culled from above with the same rasterizer state.
//!
//! Per triangle: the face normal `cross(b - a, c - a)` against the sum of
//! its three vertex normals (NORMAL, R10G10B10A2 unorm, `n * 2 - 1`); the
//! share of the triangle area where they agree, per vertex source. The
//! models draw right, so their share says which sign is the drawn
//! convention; a world share on the other side means the world's front
//! faces wind the other way from the models', and (`world_winding=auto`,
//! the default) the world's pipelines take the other front face. A share
//! near one half means the normals do not decode so (inconclusive: nothing
//! is changed).

use std::sync::atomic::{AtomicU8, Ordering};

use crate::scene::Scene;

/// 0 not decided (front faces clockwise), 1 the world's front faces wind
/// counter-clockwise (flipped), 2 kept.
static WORLD_FLIP: AtomicU8 = AtomicU8::new(0);

/// Whether world (group 3) pipelines take the counter-clockwise front face.
pub fn world_flipped() -> bool {
    WORLD_FLIP.load(Ordering::Relaxed) == 1
}

fn decode(w: u32) -> [f32; 3] {
    [0, 10, 20].map(|s| ((w >> s) & 0x3ff) as f32 / 1023.0 * 2.0 - 1.0)
}

fn sub(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn cross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}
fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
fn len(a: [f32; 3]) -> f32 {
    dot(a, a).sqrt()
}

pub fn pos(b: &[u8], v: usize) -> Option<[f32; 3]> {
    let o = 12 * v;
    let s = b.get(o..o + 12)?;
    Some([0, 4, 8].map(|k| f32::from_le_bytes(s[k..k + 4].try_into().unwrap())))
}

/// Per draw: (area agreeing, total area, upward area (face normal z > 0.7
/// once oriented by its vertex normals), downward area) over its triangles.
fn draw_stats(scene: &Scene, d: &crate::scene::Draw) -> Option<[f64; 4]> {
    let (src, stride, at) = if d.group == 3 { (&scene.world, 20usize, 12usize) } else { (&scene.models, 16, 8) };
    let mut out = [0f64; 4];
    let tris = (d.index_count / 3) as usize;
    for t in 0..tris {
        let i0 = d.first_index as usize + 3 * t;
        let ix = src.indices.get(i0..i0 + 3)?;
        let v: Vec<usize> = ix.iter().map(|&i| (d.base_vertex + i32::from(i)) as usize).collect();
        let p = [pos(&src.positions, v[0])?, pos(&src.positions, v[1])?, pos(&src.positions, v[2])?];
        let f = cross(sub(p[1], p[0]), sub(p[2], p[0]));
        let area = 0.5 * len(f);
        if !(area > 1e-6) || !area.is_finite() {
            continue;
        }
        let mut n = [0f32; 3];
        for &vi in &v {
            let o = stride * vi + at;
            let w = u32::from_le_bytes(src.attributes.get(o..o + 4)?.try_into().unwrap());
            let dn = decode(w);
            n = [n[0] + dn[0], n[1] + dn[1], n[2] + dn[2]];
        }
        if len(n) < 1e-3 {
            continue;
        }
        let agree = dot(f, n) > 0.0;
        out[1] += f64::from(area);
        if agree {
            out[0] += f64::from(area);
        }
        // Oriented by the vertex normals (the side the surface faces).
        let nz = n[2] / len(n);
        if nz > 0.7 {
            out[2] += f64::from(area);
        } else if nz < -0.7 {
            out[3] += f64::from(area);
        }
    }
    Some(out)
}

/// The check's log lines; decides [`world_flipped`].
pub fn analyse(scene: &Scene) -> Vec<String> {
    let mut lines = Vec::new();
    let mut total = [[0f64; 2]; 2];
    let mut seen_models = std::collections::HashSet::new();
    let mut per_draw: Vec<(usize, [f64; 4])> = Vec::new();
    for (i, d) in scene.draws.iter().enumerate() {
        if d.source == u32::MAX {
            continue;
        }
        if d.group == 1 && !seen_models.insert((d.first_index, d.base_vertex)) {
            continue;
        }
        let Some(s) = draw_stats(scene, d) else { continue };
        let g = usize::from(d.group == 3);
        total[g][0] += s[0];
        total[g][1] += s[1];
        if d.group == 3 {
            per_draw.push((i, s));
        }
    }
    let share = |t: [f64; 2]| if t[1] > 0.0 { t[0] / t[1] } else { 0.5 };
    let (m, w) = (share(total[0]), share(total[1]));
    let decisive = |x: f64| !(0.2..=0.8).contains(&x);
    let opposite = decisive(m) && decisive(w) && ((m > 0.5) != (w > 0.5));
    let want = codlegacy_env::var("CODLEGACY_T7_WORLD_WINDING")
        .unwrap_or_default()
        .to_ascii_lowercase();
    let flip = match want.as_str() {
        "flip" => true,
        "keep" => false,
        _ => opposite,
    };
    WORLD_FLIP.store(if flip { 1 } else { 2 }, Ordering::Relaxed);
    lines.push(format!(
        "t7 winding: face normal cross(b-a, c-a) agrees with the vertex normals on {:.1}% of the models' triangle area and {:.1}% of the world's ({}); world front faces: {} (CODLEGACY_T7_WORLD_WINDING={})",
        100.0 * m,
        100.0 * w,
        if opposite {
            "OPPOSITE conventions: the world winds the other way from the models"
        } else if decisive(m) && decisive(w) {
            "same convention"
        } else {
            "inconclusive: the normals may not decode as 10:10:10:2 n*2-1"
        },
        if flip { "counter-clockwise (flipped from the models')" } else { "clockwise, as the models'" },
        if want.is_empty() { "auto" } else { want.as_str() }
    ));
    // The largest floors: world draws by upward area.
    per_draw.sort_by(|a, b| b.1[2].total_cmp(&a.1[2]));
    for (i, s) in per_draw.iter().take(20) {
        if s[2] <= 0.0 {
            break;
        }
        let d = &scene.draws[*i];
        let m = &scene.materials[d.material];
        let (tech, state) = match (&m.technique, &m.forward) {
            (Some(t), _) => ("gbuffer", Some(t)),
            (None, Some(t)) => ("forward", Some(t)),
            _ => ("none", None),
        };
        let cull = state.map_or("-".to_owned(), |t| match crate::state::primitive(&t.state) {
            Ok(p) => format!("{:?}", p.cull_mode),
            Err(e) => e,
        });
        let st = state.map_or(String::new(), |t| format!("depth {:#x} stencil {:#x}/{:#x} raster {:#x} ref {} blend0 {:#010x}", t.state.depth, t.state.stencil[0], t.state.stencil[1], t.state.raster, t.state.stencil_ref, t.state.blend[0]));
        lines.push(format!(
            "t7 floor: world surface #{} {} ({}) up {:.0} sq units, down {:.0}, winding agrees {:.0}%; {tech} cull {cull} {st}; range {} flags {:#04x} list {}; centre ({:.0}, {:.0}, {:.0})",
            d.source,
            m.name,
            m.techset,
            s[2],
            s[3],
            if s[1] > 0.0 { 100.0 * s[0] / s[1] } else { 0.0 },
            ["A", "B", "C", "D", "none"].get(d.range as usize).unwrap_or(&"?"),
            d.flags,
            d.list,
            0.5 * (d.mins[0] + d.maxs[0]),
            0.5 * (d.mins[1] + d.maxs[1]),
            0.5 * (d.mins[2] + d.maxs[2]),
        ));
    }
    // The biggest world surfaces with flags 0x99 (and those named in
    // `CODLEGACY_T7_SURF_CHECK=84,105`): their index and vertex ranges.
    let named: Vec<u32> = codlegacy_env::var("CODLEGACY_T7_SURF_CHECK")
        .ok()
        .map(|v| v.split(',').filter_map(|x| x.trim().parse().ok()).collect())
        .unwrap_or_default();
    let mut cand: Vec<(usize, f64)> = per_draw
        .iter()
        .filter(|(i, _)| scene.draws[*i].flags == 0x99)
        .map(|(i, s)| (*i, s[1]))
        .collect();
    cand.sort_by(|a, b| b.1.total_cmp(&a.1));
    let mut pick: Vec<usize> = cand.iter().take(10).map(|c| c.0).collect();
    for (i, d) in scene.draws.iter().enumerate() {
        if d.group == 3 && named.contains(&d.source) && !pick.contains(&i) {
            pick.push(i);
        }
    }
    for i in pick {
        lines.push(surface_check(scene, i));
    }
    // Draws per technique set (world and models).
    let mut per_tech: std::collections::BTreeMap<(u8, String), usize> = Default::default();
    for d in &scene.draws {
        *per_tech.entry((d.group, scene.materials[d.material].techset.clone())).or_default() += 1;
    }
    let mut pt: Vec<_> = per_tech.into_iter().collect();
    pt.sort_by(|a, b| b.1.cmp(&a.1));
    lines.push(format!(
        "t7 draws per technique set: {}",
        pt.iter().map(|((g, t), n)| format!("{}{} x{n}", if *g == 3 { "world " } else { "" }, if t.is_empty() { "(none)" } else { t })).collect::<Vec<_>>().join(", ")
    ));
    lines
}

/// `t7 surface check:` one world draw's index and vertex ranges.
pub fn surface_check(scene: &Scene, i: usize) -> String {
    let d = &scene.draws[i];
    let m = &scene.materials[d.material];
    let src = &scene.world;
    let nv = src.positions.len() / 12;
    let ix = src.indices.get(d.first_index as usize..(d.first_index + d.index_count) as usize).unwrap_or(&[]);
    let (lo, hi) = ix.iter().fold((u16::MAX, 0u16), |(a, b), &x| (a.min(x), b.max(x)));
    let (mut degenerate, mut out_of_range, mut nan) = (0, 0, 0);
    for t in ix.chunks_exact(3) {
        if t[0] == t[1] || t[1] == t[2] || t[0] == t[2] {
            degenerate += 1;
        }
        let v: Vec<i64> = t.iter().map(|&x| i64::from(d.base_vertex) + i64::from(x)).collect();
        if v.iter().any(|&x| x < 0 || x as usize >= nv) {
            out_of_range += 1;
            continue;
        }
        if v.iter().any(|&x| pos(&src.positions, x as usize).is_none_or(|p| p.iter().any(|c| !c.is_finite() || c.abs() > 1e6))) {
            nan += 1;
        }
    }
    format!(
        "t7 surface check: #{} {} ({}): first index {} count {} ({} tris), base vertex {}, indices {}..={} (vertices {}..={} of {nv}), degenerate {degenerate}, out of range {out_of_range}, non-finite/huge {nan}; bounds {:?}..{:?}; index buffer {} long",
        d.source,
        m.name,
        m.techset,
        d.first_index,
        d.index_count,
        d.index_count / 3,
        d.base_vertex,
        lo,
        hi,
        i64::from(d.base_vertex) + i64::from(lo),
        i64::from(d.base_vertex) + i64::from(hi),
        d.mins,
        d.maxs,
        src.indices.len()
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decode_up() {
        let w = 511 | (511 << 10) | (1023 << 20);
        let n = decode(w);
        assert!(n[0].abs() < 0.01 && n[1].abs() < 0.01 && (n[2] - 1.0).abs() < 1e-6);
    }
}
