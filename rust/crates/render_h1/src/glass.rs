//! The map's intact breakable glass (`FxWorld` glass system), drawn as
//! two-sided panes. The init pieces and their geometry words are read from
//! the zone (`fastfile_h1` capture `fx_glass`); each piece is a polygon in
//! its own plane: packed `i16` vertices scaled by 1/32 along the frame's
//! first two axes from the piece origin (the sibling engine's glass scale,
//! checked here: a Highrise pane of 384 x 1952 packed units is 12 x 61
//! inches and its radius is 31.09, the half diagonal), the texture
//! coordinates from the definition's `texVecs` and the piece's texture
//! origin. Breaking, shattering and the shards are not here: panes stay
//! whole (`BO2MP_H1_GLASS=0` leaves them out).

use crate::gpu::f32_to_f16;
use fastfile_h1::walk::FxGlassRec;

/// Packed vertex units to inches.
pub const VERT_SCALE: f32 = 1.0 / 32.0;

/// One glass piece, ready for the model mesh: 32-byte packed vertices (the
/// model vertex without its lightmap unwrap), triangle indices, the
/// definition index and the piece's origin.
#[derive(Clone, Debug)]
pub struct Pane {
    pub verts: Vec<[u8; 32]>,
    pub indices: Vec<u16>,
    pub def: usize,
    pub origin: [f32; 3],
    /// The middle of the polygon (the mean of its vertices).
    pub center: [f32; 3],
    /// The piece's axes: along the frame's first two axes, then the normal.
    pub axes: [[f32; 3]; 3],
}

fn f(b: &[u8], o: usize) -> f32 {
    f32::from_le_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]])
}

/// The rotation rows of a unit quaternion (x, y, z, w).
pub fn quat_axes(q: [f32; 4]) -> [[f32; 3]; 3] {
    let [x, y, z, w] = q;
    let (x2, y2, z2) = (x + x, y + y, z + z);
    let (xx, yy, zz) = (x2 * x, y2 * y, z2 * z);
    let (xy, xz, xw) = (x2 * y, x2 * z, x2 * w);
    let (yz, yw, zw) = (y2 * z, y2 * w, z2 * w);
    [
        [1.0 - (yy + zz), xy + zw, xz - yw],
        [xy - zw, 1.0 - (xx + zz), xw + yz],
        [xz + yw, yz - xw, 1.0 - (xx + yy)],
    ]
}

fn unorm10(n: [f32; 3]) -> u32 {
    let c = |v: f32| (((v.clamp(-1.0, 1.0) * 0.5 + 0.5) * 1023.0).round() as u32) & 0x3ff;
    c(n[0]) | (c(n[1]) << 10) | (c(n[2]) << 20) | (3 << 30)
}

fn vertex(p: [f32; 3], uv: [f32; 2], normal: [f32; 3], tangent: [f32; 3]) -> [u8; 32] {
    let mut r = [0u8; 32];
    r[0..4].copy_from_slice(&p[0].to_le_bytes());
    r[4..8].copy_from_slice(&p[1].to_le_bytes());
    r[8..12].copy_from_slice(&p[2].to_le_bytes());
    r[12..16].copy_from_slice(&(-1.0f32).to_le_bytes());
    r[16..20].copy_from_slice(&[255; 4]);
    r[20..22].copy_from_slice(&f32_to_f16(uv[0]).to_le_bytes());
    r[22..24].copy_from_slice(&f32_to_f16(uv[1]).to_le_bytes());
    r[24..28].copy_from_slice(&unorm10(normal).to_le_bytes());
    r[28..32].copy_from_slice(&unorm10(tangent).to_le_bytes());
    r
}

/// Every init piece of `rec` as a two-sided pane. `image` is the zone image.
pub fn panes(rec: &FxGlassRec, image: &[u8]) -> Vec<Pane> {
    let (Some(ps), Some(gs)) = (rec.init_pieces, rec.init_geo) else {
        return Vec::new();
    };
    let (ps, gs): (&[u8], &[u8]) = (ps.get(image), gs.get(image));
    let mut out = Vec::new();
    let mut at = 0usize;
    for i in 0..rec.init_piece_count {
        let Some(s) = ps.get(i * 56..i * 56 + 56) else { break };
        let (def, vc, fc) = (usize::from(s[50]), usize::from(s[51]), usize::from(s[52]));
        let words = |from: usize, n: usize| gs.get(from * 4..(from + n) * 4);
        let geo_at = at;
        at += vc + fc;
        let (Some(vw), Some(d)) = (words(geo_at, vc), rec.defs.get(def)) else { continue };
        if vc < 3 {
            continue;
        }
        let q = [f(s, 0), f(s, 4), f(s, 8), f(s, 12)];
        let origin = [f(s, 16), f(s, 20), f(s, 24)];
        let uv0 = [f(s, 32), f(s, 36)];
        let ax = quat_axes(q);
        let (n, t) = (ax[2], ax[0]);
        let h = d.half_thickness;
        let mut verts = Vec::with_capacity(vc * 2);
        let mut sum = [0.0f32; 3];
        let pts: Vec<[i16; 2]> = vw
            .chunks_exact(4)
            .map(|w| [i16::from_le_bytes([w[0], w[1]]), i16::from_le_bytes([w[2], w[3]])])
            .collect();
        for side in [1.0f32, -1.0] {
            for p in &pts {
                let (x, y) = (f32::from(p[0]), f32::from(p[1]));
                let pos: [f32; 3] = std::array::from_fn(|k| {
                    origin[k] + (x * ax[0][k] + y * ax[1][k]) * VERT_SCALE + side * h * n[k]
                });
                let uv = [
                    d.tex_vecs[0][0] * x + d.tex_vecs[0][1] * y + uv0[0],
                    d.tex_vecs[1][0] * x + d.tex_vecs[1][1] * y + uv0[1],
                ];
                let nn = [n[0] * side, n[1] * side, n[2] * side];
                for k in 0..3 {
                    sum[k] += pos[k];
                }
                verts.push(vertex(pos, uv, nn, t));
            }
        }
        // The stored triangulation (three one-byte indices per triangle, four
        // to a word), or a fan from vertex 0 when the piece stores none (a
        // convex pane).
        let mut tris: Vec<[u16; 3]> = Vec::new();
        if fc > 0
            && let Some(fw) = words(geo_at + vc, fc)
        {
            let tri_n = vc.saturating_sub(2);
            for k in 0..tri_n {
                let (a, b, c) = (fw.get(3 * k), fw.get(3 * k + 1), fw.get(3 * k + 2));
                if let (Some(&a), Some(&b), Some(&c)) = (a, b, c)
                    && usize::from(a).max(usize::from(b)).max(usize::from(c)) < vc
                {
                    tris.push([u16::from(a), u16::from(b), u16::from(c)]);
                }
            }
        }
        if tris.is_empty() {
            tris = (1..vc as u16 - 1).map(|k| [0, k, k + 1]).collect();
        }
        let mut indices = Vec::with_capacity(tris.len() * 6);
        for t in &tris {
            indices.extend_from_slice(t);
        }
        for t in &tris {
            indices.extend_from_slice(&[t[0] + vc as u16, t[2] + vc as u16, t[1] + vc as u16]);
        }
        let each = 1.0 / (vc * 2) as f32;
        let center = sum.map(|v| v * each);
        out.push(Pane { verts, indices, def, origin, center, axes: ax });
    }
    out
}

/// Which panes are broken, by their number in `panes` order over the map's
/// glass systems (what the match's glass state says; set by the game each
/// frame, read by the host). Panes are hidden, not replaced by shards.
static BROKEN: std::sync::Mutex<(u64, Vec<u32>)> = std::sync::Mutex::new((0, Vec::new()));

/// Records the broken panes (sorted or not). Bumps the revision only when
/// the set changed.
pub fn set_broken(mut ids: Vec<u32>) {
    ids.sort_unstable();
    ids.dedup();
    let Ok(mut b) = BROKEN.lock() else { return };
    if b.1 != ids {
        b.0 += 1;
        b.1 = ids;
    }
}

/// The broken panes' revision and numbers.
pub fn broken() -> (u64, Vec<u32>) {
    BROKEN.lock().map(|b| b.clone()).unwrap_or_default()
}

/// The clip brush each pane belongs to, as the sim's glass number (pane
/// number + 1; 0: none). `glass_brushes` are the bounds of every clip
/// brush that holds glass (None: not glass). A brush takes the pane whose
/// middle is in its bounds (grown by `slack`) and nearest its own middle;
/// a brush that holds several panes breaks only that one.
pub fn tag_brushes(
    centers: &[[f32; 3]],
    glass_brushes: &[Option<([f32; 3], [f32; 3])>],
    slack: f32,
) -> Vec<u16> {
    glass_brushes
        .iter()
        .map(|b| {
            let Some((lo, hi)) = b else { return 0 };
            let mid: [f32; 3] = std::array::from_fn(|k| (lo[k] + hi[k]) * 0.5);
            let mut best: Option<(f32, usize)> = None;
            for (i, c) in centers.iter().enumerate() {
                if (0..3).any(|k| c[k] < lo[k] - slack || c[k] > hi[k] + slack) {
                    continue;
                }
                let d: f32 = (0..3).map(|k| (c[k] - mid[k]).powi(2)).sum();
                if best.is_none_or(|(bd, _)| d < bd) {
                    best = Some((d, i));
                }
            }
            match best {
                Some((_, i)) if i < usize::from(u16::MAX) => (i + 1) as u16,
                _ => 0,
            }
        })
        .collect()
}

/// Whether the panes are drawn (`BO2MP_H1_GLASS=0`: left out).
pub fn enabled() -> bool {
    !std::env::var("BO2MP_H1_GLASS").is_ok_and(|v| v.trim() == "0")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn axes_of_a_quarter_turn() {
        // Highrise's first pane: (-.5, -.5, -.5, .5).
        let a = quat_axes([-0.5, -0.5, -0.5, 0.5]);
        for row in a {
            let l: f32 = row.iter().map(|v| v * v).sum();
            assert!((l - 1.0).abs() < 1e-5);
        }
    }

    #[test]
    fn highrise_pane_size() {
        // 384 x 1952 packed units: 12 x 61 inches; half diagonal 31.09.
        let d = ((384.0f32 * VERT_SCALE / 2.0).powi(2) + (1952.0 * VERT_SCALE / 2.0).powi(2)).sqrt();
        assert!((d - 31.0886).abs() < 0.01, "{d}");
    }

    #[test]
    fn a_brush_takes_the_pane_in_it() {
        let centers = [[0.0, 0.0, 0.0], [100.0, 0.0, 0.0], [102.0, 0.0, 0.0]];
        let brushes = [
            Some(([95.0, -10.0, -10.0], [110.0, 10.0, 10.0])),
            None,
            Some(([500.0, 0.0, 0.0], [510.0, 1.0, 1.0])),
        ];
        // The brush at x 95..110 holds panes 1 and 2; its middle (102.5) is
        // nearest the second.
        assert_eq!(tag_brushes(&centers, &brushes, 1.0), vec![3, 0, 0]);
    }

    #[test]
    fn broken_set_bumps_revision_once() {
        set_broken(vec![3, 1, 3]);
        let (r1, ids) = broken();
        assert_eq!(ids, vec![1, 3]);
        set_broken(vec![1, 3]);
        assert_eq!(broken().0, r1);
        set_broken(vec![]);
        assert_eq!(broken().0, r1 + 1);
    }

    #[test]
    fn packed_normal_round_trip() {
        let v = unorm10([0.0, 0.0, 1.0]);
        assert_eq!(v >> 20 & 0x3ff, 1023);
        assert_eq!(v & 0x3ff, 512);
    }
}
