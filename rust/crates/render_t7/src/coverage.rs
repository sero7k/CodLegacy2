//! The `t7 coverage:` audit's expectation: how many pixels a draw's
//! triangles cover in the view, worked out on the CPU from the scene's own
//! vertex data (world surfaces as stored, static models through their
//! placements' bone matrices). An occlusion query of 0 samples means
//! something only where this says the draw should cover pixels: a draw
//! whose bounds touch the frustum but whose triangles are behind the eye,
//! off screen or under a pixel writes nothing and is fine.
//!
//! Per draw: (pixels of triangles wound clockwise on screen, pixels wound
//! counter-clockwise), each triangle clipped to the near plane and the
//! screen first. Which of the two is the front depends on the pipeline
//! (world flip, cull mode); the audit reports both.

use crate::scene::{Draw, Scene};

/// At most this many placements of one model draw are looked at (Skyjacked
/// has draws of hundreds of placements; the estimate is for the log).
const MAX_PLACEMENTS: u32 = 64;

fn clip_poly(poly: &[[f32; 4]], plane: impl Fn(&[f32; 4]) -> f32) -> Vec<[f32; 4]> {
    let mut out = Vec::with_capacity(poly.len() + 3);
    for i in 0..poly.len() {
        let (a, b) = (poly[i], poly[(i + 1) % poly.len()]);
        let (da, db) = (plane(&a), plane(&b));
        if da >= 0.0 {
            out.push(a);
        }
        if (da >= 0.0) != (db >= 0.0) {
            let t = da / (da - db);
            out.push(std::array::from_fn(|k| a[k] + (b[k] - a[k]) * t));
        }
    }
    out
}

/// Signed screen area in pixels (positive: counter-clockwise with y up) of
/// one clip-space triangle, clipped to the view volume.
pub fn clipped_area(tri: [[f32; 4]; 3], width: u32, height: u32) -> f32 {
    let mut p: Vec<[f32; 4]> = tri.to_vec();
    p = clip_poly(&p, |v| v[3] - 1e-3);
    for (axis, sign) in [(0usize, 1.0f32), (0, -1.0), (1, 1.0), (1, -1.0)] {
        if p.len() < 3 {
            return 0.0;
        }
        p = clip_poly(&p, |v| v[3] - sign * v[axis]);
    }
    if p.len() < 3 {
        return 0.0;
    }
    let s: Vec<[f32; 2]> = p.iter().map(|v| [v[0] / v[3] * 0.5 * width as f32, v[1] / v[3] * 0.5 * height as f32]).collect();
    let mut a = 0.0;
    for i in 0..s.len() {
        let (u, v) = (s[i], s[(i + 1) % s.len()]);
        a += u[0] * v[1] - v[0] * u[1];
    }
    0.5 * a
}

fn pos(b: &[u8], v: usize) -> Option<[f32; 3]> {
    crate::winding::pos(b, v)
}

/// (clockwise pixels, counter-clockwise pixels) of `d` in the view.
pub fn expected_pixels(scene: &Scene, d: &Draw, world_to_clip: glam::Mat4, width: u32, height: u32) -> (f32, f32) {
    let src = if d.group == 3 { &scene.world } else { &scene.models };
    let identity = {
        let mut m = [0.0f32; 16];
        (m[0], m[5], m[10], m[15]) = (1.0, 1.0, 1.0, 1.0);
        m
    };
    let placements: Vec<[f32; 16]> = if d.group == 3 {
        vec![identity]
    } else {
        (d.instance..d.instance + d.instances.min(MAX_PLACEMENTS))
            .filter_map(|i| scene.instances.get(i as usize))
            .map(|ins| scene.bones.get(ins[3] as usize).copied().unwrap_or(identity))
            .collect()
    };
    let (mut cw, mut ccw) = (0.0f32, 0.0f32);
    for m in &placements {
        let to = |p: [f32; 3]| -> [f32; 4] {
            let w: [f32; 3] = std::array::from_fn(|r| m[4 * r] * p[0] + m[4 * r + 1] * p[1] + m[4 * r + 2] * p[2] + m[4 * r + 3]);
            (world_to_clip * glam::Vec4::new(w[0], w[1], w[2], 1.0)).to_array()
        };
        for t in 0..(d.index_count / 3) as usize {
            let i0 = d.first_index as usize + 3 * t;
            let Some(ix) = src.indices.get(i0..i0 + 3) else { break };
            let v = |k: usize| pos(&src.positions, (d.base_vertex + i32::from(ix[k])) as usize);
            let (Some(a), Some(b), Some(c)) = (v(0), v(1), v(2)) else { continue };
            let s = clipped_area([to(a), to(b), to(c)], width, height);
            if s < 0.0 {
                cw -= s;
            } else {
                ccw += s;
            }
        }
    }
    (cw, ccw)
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_full_screen_triangle_pair_covers_the_screen() {
        let q = |x: f32, y: f32| [x, y, 0.5, 1.0];
        let a = super::clipped_area([q(-1.0, -1.0), q(1.0, -1.0), q(1.0, 1.0)], 100, 50);
        assert!((a - 2500.0).abs() < 1.0, "{a}");
        // Wound the other way: negative.
        let b = super::clipped_area([q(-1.0, -1.0), q(1.0, 1.0), q(1.0, -1.0)], 100, 50);
        assert!((b + 2500.0).abs() < 1.0, "{b}");
        // Far bigger than the screen: clipped to it.
        let c = super::clipped_area([q(-9.0, -9.0), q(9.0, -9.0), q(0.0, 9.0)], 100, 50);
        assert!((c - 5000.0).abs() < 1.0, "{c}");
        // Behind the eye: nothing.
        let z = |x: f32, y: f32| [x, y, 0.5, -1.0];
        assert_eq!(super::clipped_area([z(-1.0, -1.0), z(1.0, -1.0), z(1.0, 1.0)], 100, 50), 0.0);
    }
}
