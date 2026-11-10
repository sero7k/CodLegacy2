//! A few screen pixels read back (G-buffer depth and targets) and the same
//! pixels' rays cast on the CPU against the world's triangles, a few times
//! a match (`t7 probe:` lines) when `CODLEGACY_T7_DEBUG_PROBE=1` (or
//! `CODLEGACY_T7_PROBE_FRAMES`) is set; off by default.
//! For each pixel: what the GPU left there (depth 0: no surface wrote it),
//! the nearest world surface the ray crosses (material, technique set,
//! pass, cull mode), whether that triangle faces the camera by the
//! pipeline's winding (so would be culled), and the depth it should have
//! written. A floor that is in the data under the pixel but absent from the
//! G-buffer is then named, with why its triangles drop out if they do.

use std::collections::HashSet;

use glam::{Mat4, Vec3, Vec4};
use render_h1::code::Camera;
use render_h1::gpu::Gpu;

use crate::gbuffer::Targets;
use crate::scene::Scene;

/// The frames (since the first) at which the probe runs.
pub const FRAMES: [u64; 3] = [600, 2400, 6000];

/// Whether the probe runs at all: `CODLEGACY_T7_DEBUG_PROBE=1`, or
/// `CODLEGACY_T7_PROBE_FRAMES` set. Off in normal play (each run reads the whole
/// G-buffer back and waits on the GPU).
pub fn enabled() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| {
        codlegacy_env::var("CODLEGACY_T7_DEBUG_PROBE").is_ok_and(|v| !v.is_empty() && v != "0")
            || codlegacy_env::var_os("CODLEGACY_T7_PROBE_FRAMES").is_some()
    })
}

/// Whether the probe runs at `frame`: [`FRAMES`], or the frames listed in
/// `CODLEGACY_T7_PROBE_FRAMES` (comma-separated; offline checks).
pub fn due(frame: u64) -> bool {
    static F: std::sync::OnceLock<Vec<u64>> = std::sync::OnceLock::new();
    F.get_or_init(|| {
        codlegacy_env::var("CODLEGACY_T7_PROBE_FRAMES")
            .ok()
            .map(|v| v.split(',').filter_map(|x| x.trim().parse().ok()).collect())
            .unwrap_or_else(|| FRAMES.to_vec())
    })
    .contains(&frame)
}

/// The pixels sampled, as fractions of the target (most below the middle,
/// where the floors are).
const SAMPLES: [(f32, f32); 9] = [(0.5, 0.45), (0.5, 0.55), (0.5, 0.65), (0.5, 0.75), (0.5, 0.9), (0.25, 0.8), (0.75, 0.8), (0.1, 0.95), (0.9, 0.95)];

struct Hit {
    t: f32,
    draw: usize,
    tri: usize,
    p: [Vec3; 3],
}

fn ray_tri(o: Vec3, d: Vec3, p: &[Vec3; 3]) -> Option<f32> {
    let e1 = p[1] - p[0];
    let e2 = p[2] - p[0];
    let h = d.cross(e2);
    let a = e1.dot(h);
    if a.abs() < 1e-9 {
        return None;
    }
    let f = 1.0 / a;
    let s = o - p[0];
    let u = f * s.dot(h);
    if !(0.0..=1.0).contains(&u) {
        return None;
    }
    let q = s.cross(e1);
    let v = f * d.dot(q);
    if v < 0.0 || u + v > 1.0 {
        return None;
    }
    let t = f * e2.dot(q);
    (t > 1e-3).then_some(t)
}

fn ray_box(o: Vec3, inv: Vec3, lo: Vec3, hi: Vec3) -> bool {
    let a = (lo - o) * inv;
    let b = (hi - o) * inv;
    let tmin = a.min(b).max_element();
    let tmax = a.max(b).min_element();
    tmax >= tmin.max(0.0)
}

pub fn run(gpu: &Gpu, scene: &Scene, targets: &Targets, camera: &Camera, gbuffer_ready: &HashSet<usize>, forward_ready: &HashSet<usize>) -> Vec<String> {
    use crate::debug_views::read_tex;
    let mut out = Vec::new();
    let (w, h) = (targets.width as usize, targets.height as usize);
    if targets.width == 0 || targets.height == 0 {
        return out;
    }
    let depth: Vec<f32> = read_tex(gpu, &targets.depth.texture, wgpu::TextureAspect::DepthOnly, 4).into_iter().map(f32::from_bits).collect();
    let col: Vec<Vec<u32>> = targets.colour.iter().map(|t| read_tex(gpu, &t.texture, wgpu::TextureAspect::All, 4)).collect();
    // World -> clip (world positions are absolute; the camera's matrix is eye-relative).
    let world_to_clip = camera.view_projection * Mat4::from_translation(-camera.eye);
    let inv = camera.view_projection.inverse();
    out.push(format!("t7 probe: eye ({:.0}, {:.0}, {:.0}), {w}x{h}; world front faces {}", camera.eye.x, camera.eye.y, camera.eye.z, if crate::winding::world_flipped() { "counter-clockwise" } else { "clockwise" }));
    let world: Vec<usize> = scene.draws.iter().enumerate().filter(|(_, d)| d.group == 3).map(|(i, _)| i).collect();
    for (k, (fx, fy)) in SAMPLES.iter().enumerate() {
        let (px, py) = (((w as f32 * fx) as usize).min(w - 1), ((h as f32 * fy) as usize).min(h - 1));
        let i = py * w + px;
        let ndc = Vec4::new(2.0 * (px as f32 + 0.5) / w as f32 - 1.0, 1.0 - 2.0 * (py as f32 + 0.5) / h as f32, 0.5, 1.0);
        let q = inv * ndc;
        let dir = (q.truncate() / q.w).normalize();
        let o = camera.eye;
        let invd = Vec3::ONE / dir;
        let mut hits: Vec<Hit> = Vec::new();
        for &di in &world {
            let d = &scene.draws[di];
            if !ray_box(o, invd, Vec3::from(d.mins) - 1.0, Vec3::from(d.maxs) + 1.0) {
                continue;
            }
            for t in 0..(d.index_count / 3) as usize {
                let i0 = d.first_index as usize + 3 * t;
                let Some(ix) = scene.world.indices.get(i0..i0 + 3) else { break };
                let p: Option<Vec<Vec3>> = ix.iter().map(|&x| crate::winding::pos(&scene.world.positions, (d.base_vertex + i32::from(x)) as usize).map(Vec3::from)).collect();
                let Some(p) = p else { break };
                let p = [p[0], p[1], p[2]];
                if let Some(tt) = ray_tri(o, dir, &p) {
                    hits.push(Hit { t: tt, draw: di, tri: t, p });
                }
            }
        }
        hits.sort_by(|a, b| a.t.total_cmp(&b.t));
        let gd = depth.get(i).copied().unwrap_or(0.0);
        let px_of = |t: usize| col.get(t).and_then(|c| c.get(i)).copied().unwrap_or(0);
        let g = [px_of(0), px_of(1), px_of(2)];
        let mut line = format!(
            "t7 probe {k} px ({px}, {py}): GPU depth {gd:.6}{} diffuse {:#010x} normal+gloss {:#010x} spec+occ {:#010x}",
            if gd == 0.0 { " (no surface wrote it)" } else { "" },
            g[0],
            g[1],
            g[2]
        );
        if hits.is_empty() {
            line.push_str("; ray crosses no world triangle");
        }
        for (n, hit) in hits.iter().take(3).enumerate() {
            let d = &scene.draws[hit.draw];
            let m = &scene.materials[d.material];
            let pass = if gbuffer_ready.contains(&hit.draw) {
                "gbuffer"
            } else if forward_ready.contains(&hit.draw) {
                "forward"
            } else {
                "NOT DRAWN"
            };
            let tech = m.technique.as_ref().or(m.forward.as_ref());
            let cull = tech.and_then(|t| crate::state::primitive(&t.state).ok()).and_then(|p| if crate::surf_debug::get().cull_none { None } else { p.cull_mode });
            // Facing by the screen-space winding (NDC, y up): counter-clockwise
            // has positive area; the pipelines' front face is clockwise
            // (world: counter-clockwise when flipped).
            let c: Vec<Vec4> = hit.p.iter().map(|v| world_to_clip * v.extend(1.0)).collect();
            let s: Vec<(f32, f32)> = c.iter().map(|v| (v.x / v.w, v.y / v.w)).collect();
            let area = (s[1].0 - s[0].0) * (s[2].1 - s[0].1) - (s[2].0 - s[0].0) * (s[1].1 - s[0].1);
            let ccw = area > 0.0;
            let front = if crate::winding::world_flipped() { ccw } else { !ccw };
            // (A triangle reaching behind the eye has no screen winding.)
            let behind = c.iter().any(|v| v.w <= 0.0);
            let culled = !behind && match cull {
                Some(wgpu::Face::Back) => !front,
                Some(wgpu::Face::Front) => front,
                None => false,
            };
            let hp = o + dir * hit.t;
            let hc = world_to_clip * hp.extend(1.0);
            line.push_str(&format!(
                "; {} at {:.0} units: world surface #{} tri {} {} ({}) [{pass}] cull {:?}, faces camera {}{}; its depth {:.6}; range {} flags {:#04x}",
                if n == 0 { "nearest" } else { "then" },
                hit.t,
                d.source,
                hit.tri,
                m.name,
                m.techset,
                cull,
                if behind { "? (reaches behind the eye)" } else if front { "front" } else { "back" },
                if culled { " -> CULLED" } else { "" },
                hc.z / hc.w,
                ["A", "B", "C", "D", "none"].get(d.range as usize).unwrap_or(&"?"),
                d.flags
            ));
        }
        out.push(line);
    }
    out
}
