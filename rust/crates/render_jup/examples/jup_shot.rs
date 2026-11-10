#![allow(unsafe_code)]
//! Renders a Modern Warfare III map with the native renderer on a headless
//! wgpu device (Vulkan on llvmpipe works: `WGPU_BACKEND=vulkan`) and writes
//! PNGs:
//!
//! `jup_shot --zone <install>/zone/<map>.ff --out <dir> [--ents <spawnvars file>]
//!   [--ooz <libooz.so>] [--pack <dir>] [--width 1280] [--height 720] [--only <name>]`
//!
//! The zone is decoded with ooz (`libooz.so`, `--ooz` or `$JUP_OOZ`): the
//! game's own Oodle is Windows only. Views: the map's start points, its
//! intermission camera, a few of its camera entities, an overview from above
//! and one toward the sky; the first view also writes the G-buffer's albedo
//! and normal. Set `BO2MP_JUP_PROGRAM_CHECK=0` to skip the pipeline check
//! thread and `BO2MP_JUP_SHADOW_SIZE=2048` to make shadows cheaper.
use std::path::{Path, PathBuf};

use fastfile_jup::oodle::{Decompressor, OodleError};
use glam::Vec3;
use render_h1::code::{Camera, View};
use render_h1::gpu::Gpu;
use render_jup::game_view::GameView;
use render_jup::host::Host;
use render_jup::scene::Scene;

struct Ooz {
    lib: libloading::Library,
}

impl Decompressor for Ooz {
    fn name(&self) -> &str {
        "ooz"
    }
    fn decompress(&self, src: &[u8], dst: &mut [u8]) -> Result<(), OodleError> {
        // ooz may write a little past the end: decode into a padded buffer.
        let mut tmp = vec![0u8; dst.len() + 64];
        let n = unsafe {
            let f: libloading::Symbol<unsafe extern "C" fn(*const u8, usize, *mut u8, usize) -> i32> =
                self.lib.get(b"ooz_kraken").map_err(|e| OodleError::NoBackend(e.to_string()))?;
            f(src.as_ptr(), src.len(), tmp.as_mut_ptr(), dst.len())
        };
        if n as usize != dst.len() {
            return Err(OodleError::WrongLength { got: i64::from(n), want: dst.len() });
        }
        dst.copy_from_slice(&tmp[..dst.len()]);
        Ok(())
    }
}

struct Ent {
    class: String,
    origin: Vec3,
    angles: Vec3,
}

/// The entities of a Radiant-style text with a class, origin and angles.
fn entities(text: &str) -> Vec<Ent> {
    let mut out = Vec::new();
    let (mut class, mut origin, mut angles) = (String::new(), None, Vec3::ZERO);
    for line in text.lines() {
        let line = line.trim();
        if line == "{" {
            (class, origin, angles) = (String::new(), None, Vec3::ZERO);
        } else if line == "}" {
            if let Some(o) = origin.take() {
                out.push(Ent { class: std::mem::take(&mut class), origin: o, angles });
            }
        } else if let Some((k, v)) = line.split_once(' ') {
            let v = v.trim().trim_matches('"');
            let k = k.trim_matches('"');
            let vec = |v: &str| {
                let f: Vec<f32> = v.split_whitespace().filter_map(|x| x.parse().ok()).collect();
                (f.len() >= 3).then(|| Vec3::new(f[0], f[1], f[2]))
            };
            match k {
                "classname" => class = v.to_owned(),
                "origin" => origin = vec(v),
                "angles" => angles = vec(v).unwrap_or(Vec3::ZERO),
                _ => {}
            }
        }
    }
    out
}

/// `--legacy-camera`: the old 90 degree lens with near 4 and a left-handed
/// view, instead of the game's.
fn legacy_camera() -> bool {
    std::env::args().any(|a| a == "--legacy-camera")
}

/// The camera of a view: the game's own lens (`render_jup::game_view`, the
/// code the game's glue builds its camera with), or the legacy one.
fn make_camera(eye: Vec3, angles: Vec3, w: u32, h: u32) -> Camera {
    if legacy_camera() {
        Camera::from(&View { eye, angles, fov_x: 90.0, width: w, height: h, near: 4.0 })
    } else {
        GameView::from_angles(eye, angles, w, h).camera()
    }
}

fn arg(name: &str) -> Option<String> {
    let a: Vec<String> = std::env::args().collect();
    a.iter().position(|x| x == name).and_then(|k| a.get(k + 1).cloned())
}

/// Copies a texture to bytes (rows tight), `bpp` bytes a texel.
fn read_texture(gpu: &Gpu, tex: &wgpu::Texture, w: u32, h: u32, bpp: u32) -> Vec<u8> {
    let row = (w * bpp).next_multiple_of(256);
    let buf = gpu.device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("shot"),
        size: u64::from(row * h),
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut enc = gpu.device.create_command_encoder(&Default::default());
    enc.copy_texture_to_buffer(
        wgpu::TexelCopyTextureInfo { texture: tex, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
        wgpu::TexelCopyBufferInfo { buffer: &buf, layout: wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(row), rows_per_image: Some(h) } },
        wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
    );
    gpu.queue.submit([enc.finish()]);
    let slice = buf.slice(..);
    slice.map_async(wgpu::MapMode::Read, |_| {});
    let _ = gpu.device.poll(wgpu::PollType::wait_indefinitely());
    let data = slice.get_mapped_range();
    let mut out = Vec::with_capacity((w * h * bpp) as usize);
    for y in 0..h {
        let o = (y * row) as usize;
        out.extend_from_slice(&data[o..o + (w * bpp) as usize]);
    }
    out
}

fn write_png(path: &Path, w: u32, h: u32, rgba: &[u8]) {
    let file = std::fs::File::create(path).expect("create png");
    let mut enc = png::Encoder::new(std::io::BufWriter::new(file), w, h);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    let mut wr = enc.write_header().expect("png header");
    wr.write_image_data(rgba).expect("png data");
}

/// Writes a scene picture's largest level as a PNG (BC1 and BC7).
fn dump_picture(path: &Path, img: &render_jup::scene::SceneImage) {
    let Some(l0) = img.levels.first() else { return };
    let (w, h) = (img.width as usize, img.height as usize);
    let bs = if img.dxgi == 71 || img.dxgi == 80 { 8 } else { 16 };
    let mut px = vec![255u8; w * h * 4];
    let mut tmp = [0u8; 64];
    for by in 0..h.div_ceil(4) {
        for bx in 0..w.div_ceil(4) {
            let at = (by * w.div_ceil(4) + bx) * bs;
            let Some(src) = l0.get(at..at + bs) else { continue };
            if img.dxgi == 80 {
                let mut one = [0u8; 16];
                bcdec_rs::bc4(src, &mut one, 4, false);
                for (k, v) in one.iter().enumerate() {
                    tmp[k * 4..k * 4 + 4].copy_from_slice(&[*v, *v, *v, 255]);
                }
            } else if img.dxgi == 71 {
                bcdec_rs::bc1(src, &mut tmp, 16);
            } else {
                bcdec_rs::bc7(src, &mut tmp, 16);
            }
            for y in 0..4 {
                for x in 0..4 {
                    let (qx, qy) = (bx * 4 + x, by * 4 + y);
                    if qx < w && qy < h {
                        px[(qy * w + qx) * 4..][..4].copy_from_slice(&tmp[y * 16 + x * 4..][..4]);
                    }
                }
            }
        }
    }
    write_png(path, w as u32, h as u32, &px);
}

/// Ground views on path nodes with an open view: horizontal rays from head
/// height in 24 directions against the world and the placed models;
/// a node that something meets within 40 units of is inside or against
/// geometry and is passed over; the view looks down the five neighbouring
/// directions (75 degrees) whose nearest hit is at least 450 units off,
/// scored by their mean reach (held to 2,500; a direction nothing meets
/// within 3,000 counts 1,200, so a view across the roofs beats one off the
/// edge). `n` views at least 1500 units
/// apart, best first.
fn open_views(scene: &Scene, nodes: &[&Ent], n: usize) -> Vec<(Vec3, Vec3)> {
    const DIRS: usize = 24;
    const REACH: f32 = 3000.0;
    const CELL: f32 = 96.0;
    if nodes.is_empty() {
        return Vec::new();
    }
    let (mut zlo, mut zhi) = (f32::MAX, f32::MIN);
    let (mut lo, mut hi) = (Vec3::splat(f32::MAX), Vec3::splat(f32::MIN));
    for e in nodes {
        zlo = zlo.min(e.origin.z);
        zhi = zhi.max(e.origin.z + 60.0);
        lo = lo.min(e.origin);
        hi = hi.max(e.origin);
    }
    let (lo, hi) = (lo - Vec3::splat(REACH), hi + Vec3::splat(REACH));
    // The triangles that a horizontal ray at head height can meet.
    let mut tris: Vec<[Vec3; 3]> = Vec::new();
    for d in &scene.draws {
        let m = &scene.materials[d.material as usize];
        if m.tool || d.maxs[2] < zlo || d.mins[2] > zhi {
            continue;
        }
        for k in d.first_instance..d.first_instance + d.instances {
            let inst = scene.instances[k as usize];
            let (a0, a1, a2, o) = (Vec3::new(inst.a0[0], inst.a0[1], inst.a0[2]), Vec3::from_slice(&inst.a1[..3]), Vec3::from_slice(&inst.a2[..3]), Vec3::from_slice(&inst.origin[..3]));
            if d.model != u32::MAX && (o.x < lo.x || o.y < lo.y || o.x > hi.x || o.y > hi.y || o.z < zlo - 600.0 || o.z > zhi + 600.0) {
                continue;
            }
            let place = |p: [f32; 3]| o + (a0 * p[0] + a1 * p[1] + a2 * p[2]) * inst.a0[3];
            let mut local: Vec<[Vec3; 3]> = Vec::new();
            for t in 0..(d.index_count / 3) as usize {
                let i0 = d.first_index as usize + 3 * t;
                let v = [0, 1, 2].map(|c| place(scene.vertices[scene.indices[i0 + c] as usize].pos));
                if v.iter().all(|p| p.z < zlo) || v.iter().all(|p| p.z > zhi) {
                    continue;
                }
                local.push(v);
            }
            tris.extend(local);
        }
    }
    let cols = (((hi.x - lo.x) / CELL).ceil() as usize).max(1);
    let rows = (((hi.y - lo.y) / CELL).ceil() as usize).max(1);
    let mut grid: Vec<Vec<u32>> = vec![Vec::new(); cols * rows];
    let cell_of = |x: f32, y: f32| -> Option<(usize, usize)> {
        let (cx, cy) = (((x - lo.x) / CELL).floor(), ((y - lo.y) / CELL).floor());
        (cx >= 0.0 && cy >= 0.0 && (cx as usize) < cols && (cy as usize) < rows).then_some((cx as usize, cy as usize))
    };
    for (i, t) in tris.iter().enumerate() {
        let (tlo, thi) = (t[0].min(t[1]).min(t[2]), t[0].max(t[1]).max(t[2]));
        let (Some(a), Some(b)) = (cell_of(tlo.x.max(lo.x), tlo.y.max(lo.y)), cell_of(thi.x.min(hi.x - 1.0), thi.y.min(hi.y - 1.0))) else { continue };
        for cy in a.1..=b.1 {
            for cx in a.0..=b.0 {
                grid[cy * cols + cx].push(i as u32);
            }
        }
    }
    let cast = |eye: Vec3, d: Vec3| -> f32 {
        let mut best = REACH;
        let mut t = 0.0f32;
        let mut seen: Option<(usize, usize)> = None;
        while t < best {
            let p = eye + d * t;
            if let Some(c) = cell_of(p.x, p.y)
                && seen != Some(c)
            {
                seen = Some(c);
                for &i in &grid[c.1 * cols + c.0] {
                    let [a, b, c2] = tris[i as usize];
                    let (e1, e2) = (b - a, c2 - a);
                    let pv = d.cross(e2);
                    let det = e1.dot(pv);
                    if det.abs() < 1e-9 {
                        continue;
                    }
                    let inv = 1.0 / det;
                    let tv = eye - a;
                    let u = tv.dot(pv) * inv;
                    if !(0.0..=1.0).contains(&u) {
                        continue;
                    }
                    let q = tv.cross(e1);
                    let v = d.dot(q) * inv;
                    if v < 0.0 || u + v > 1.0 {
                        continue;
                    }
                    let tt = e2.dot(q) * inv;
                    if tt > 0.0 && tt < best {
                        best = tt;
                    }
                }
            }
            t += CELL * 0.5;
        }
        best
    };
    let step = (nodes.len() / 600).max(1);
    let mut scored: Vec<(f32, Vec3, f32)> = Vec::new();
    for e in nodes.iter().step_by(step) {
        let eye = e.origin + Vec3::new(0.0, 0.0, 60.0);
        let dist: Vec<f32> = (0..DIRS)
            .map(|k| {
                let a = k as f32 / DIRS as f32 * std::f32::consts::TAU;
                cast(eye, Vec3::new(a.cos(), a.sin(), 0.0))
            })
            .collect();
        if dist.iter().any(|&d| d < 40.0) {
            continue;
        }
        let mut best: Option<(f32, usize)> = None;
        for k in 0..DIRS {
            let w: Vec<f32> = (0..5).map(|j| dist[(k + DIRS - 2 + j) % DIRS]).collect();
            let worst = w.iter().copied().fold(f32::MAX, f32::min);
            if worst < 450.0 {
                continue;
            }
            // (A direction nothing meets looks off the roof into the far city; one that
            // meets something 1,000 to 2,500 units off looks across the map.)
            let score = w.iter().map(|&d| if d >= REACH { 1200.0 } else { d.min(2500.0) }).sum::<f32>() / 5.0;
            if best.is_none_or(|b| score > b.0) {
                best = Some((score, k));
            }
        }
        if let Some((score, k)) = best {
            scored.push((score, eye, k as f32 * 360.0 / DIRS as f32));
        }
    }
    scored.sort_by(|a, b| b.0.total_cmp(&a.0));
    println!("open views: {} triangles, {} of {} nodes looked at have an open view", tris.len(), scored.len(), nodes.len().div_ceil(step));
    let mut out: Vec<(Vec3, Vec3)> = Vec::new();
    for (score, eye, yaw) in scored {
        if out.len() >= n {
            break;
        }
        if out.iter().all(|(e, _)| e.distance(eye) > 1500.0) {
            println!("  open view at {eye:.0?} yaw {yaw:.0}: mean reach {score:.0}");
            out.push((eye, Vec3::new(6.0, yaw, 0.0)));
        }
    }
    out
}

/// `--check-placement`: how many placed models rest on the world's floors
/// under the quaternion as read and under other readings of the same 16-bit
/// words. A model rests when a ray dropped from its box's bottom centre
/// (from 24 units above it) meets a world triangle within 12 units of the
/// bottom. Readings: as read (x, y, z, w), its transpose (the inverse
/// rotation), the words as (w, x, y, z), that transposed, and no rotation.
fn check_placement(scene: &Scene) {
    use glam::{Mat3, Quat};
    const CELL: f32 = 64.0;
    // The world's triangles, by 2-D cell.
    let mut tris: Vec<[Vec3; 3]> = Vec::new();
    let (mut lo, mut hi) = (Vec3::splat(f32::MAX), Vec3::splat(f32::MIN));
    for d in scene.draws.iter().filter(|d| d.model == u32::MAX && !scene.materials[d.material as usize].water) {
        for t in 0..(d.index_count / 3) as usize {
            let i0 = d.first_index as usize + 3 * t;
            let v = [0, 1, 2].map(|c| Vec3::from_array(scene.vertices[scene.indices[i0 + c] as usize].pos));
            for p in &v {
                lo = lo.min(*p);
                hi = hi.max(*p);
            }
            tris.push(v);
        }
    }
    let (lo, hi) = (lo.max(Vec3::splat(-20000.0)), hi.min(Vec3::splat(20000.0)));
    let cols = ((hi.x - lo.x) / CELL).ceil().max(1.0) as usize;
    let rows = ((hi.y - lo.y) / CELL).ceil().max(1.0) as usize;
    let mut grid: Vec<Vec<u32>> = vec![Vec::new(); cols * rows];
    let cell = |x: f32, y: f32| -> Option<usize> {
        let (cx, cy) = (((x - lo.x) / CELL).floor(), ((y - lo.y) / CELL).floor());
        (cx >= 0.0 && cy >= 0.0 && (cx as usize) < cols && (cy as usize) < rows).then(|| cy as usize * cols + cx as usize)
    };
    for (i, t) in tris.iter().enumerate() {
        let (a, b) = (t[0].min(t[1]).min(t[2]), t[0].max(t[1]).max(t[2]));
        if b.x < lo.x || b.y < lo.y || a.x > hi.x || a.y > hi.y || (b.x - a.x) > 8000.0 || (b.y - a.y) > 8000.0 {
            continue;
        }
        let (x0, y0) = (((a.x.max(lo.x) - lo.x) / CELL) as usize, ((a.y.max(lo.y) - lo.y) / CELL) as usize);
        let (x1, y1) = ((((b.x.min(hi.x - 1.0)) - lo.x) / CELL) as usize, (((b.y.min(hi.y - 1.0)) - lo.y) / CELL) as usize);
        for y in y0..=y1.min(rows - 1) {
            for x in x0..=x1.min(cols - 1) {
                grid[y * cols + x].push(i as u32);
            }
        }
    }
    // The floor height below a point (the highest world triangle under it, not above `top`).
    let floor_below = |p: Vec3, top: f32| -> Option<f32> {
        let c = cell(p.x, p.y)?;
        let mut best: Option<f32> = None;
        for &i in &grid[c] {
            let [a, b, c2] = tris[i as usize];
            // Barycentric in the xy plane.
            let (v0, v1, v2) = (b - a, c2 - a, p - a);
            let den = v0.x * v1.y - v1.x * v0.y;
            if den.abs() < 1e-6 {
                continue;
            }
            let u = (v2.x * v1.y - v1.x * v2.y) / den;
            let w = (v0.x * v2.y - v2.x * v0.y) / den;
            if u < 0.0 || w < 0.0 || u + w > 1.0 {
                continue;
            }
            let z = a.z + u * v0.z + w * v1.z;
            if z <= top && best.is_none_or(|bz| z > bz) {
                best = Some(z);
            }
        }
        best
    };
    let names = ["as read", "transposed", "w first", "w first transposed", "no rotation"];
    let mut counts = [[0usize; 3]; 5];
    let mut per_model: std::collections::BTreeMap<String, [usize; 5]> = Default::default();
    let mut placed = 0usize;
    let mut high: std::collections::BTreeMap<String, (usize, f32, Vec3)> = Default::default();
    for d in scene.draws.iter().filter(|d| d.model != u32::MAX) {
        // (Once per model: its first draw stands for it.)
        if scene.draws.iter().position(|o| o.model == d.model) != scene.draws.iter().position(|o| std::ptr::eq(o, d)) {
            continue;
        }
        let verts: Vec<Vec3> = scene
            .draws
            .iter()
            .filter(|o| o.model == d.model)
            .flat_map(|o| (o.first_index..o.first_index + o.index_count).step_by(3).map(|i| Vec3::from_array(scene.vertices[scene.indices[i as usize] as usize].pos)))
            .collect();
        let step = (verts.len() / 400).max(1);
        let name = scene.model_names.get(d.model as usize).cloned().unwrap_or_default();
        for k in d.first_instance..d.first_instance + d.instances {
            let inst = scene.instances[k as usize];
            let m = Mat3::from_cols(Vec3::new(inst.a0[0], inst.a0[1], inst.a0[2]), Vec3::from_slice(&inst.a1[..3]), Vec3::from_slice(&inst.a2[..3]));
            let q = Quat::from_mat3(&m);
            // The words as read were (x, y, z, w) = q; as (w, x, y, z) they are w = q.x, x = q.y, y = q.z, z = q.w.
            let wf = Quat::from_xyzw(q.y, q.z, q.w, q.x).normalize();
            let mats = [m, m.transpose(), Mat3::from_quat(wf), Mat3::from_quat(wf).transpose(), Mat3::IDENTITY];
            let o = Vec3::from_slice(&inst.origin[..3]);
            let s = inst.a0[3];
            if o.x < lo.x || o.y < lo.y || o.x > hi.x || o.y > hi.y || o.length() > 12000.0 {
                continue;
            }
            placed += 1;
            for (r, mat) in mats.iter().enumerate() {
                let (mut blo, mut bhi) = (Vec3::splat(f32::MAX), Vec3::splat(f32::MIN));
                for v in verts.iter().step_by(step) {
                    let p = o + *mat * *v * s;
                    blo = blo.min(p);
                    bhi = bhi.max(p);
                }
                let c = (blo + bhi) * 0.5;
                let bottom = Vec3::new(c.x, c.y, blo.z);
                match floor_below(bottom, blo.z + 24.0) {
                    Some(z) if (blo.z - z).abs() <= 12.0 => {
                        counts[r][0] += 1;
                        per_model.entry(name.clone()).or_default()[r] += 1;
                    }
                    Some(z) if blo.z - z > 200.0 => {
                        counts[r][1] += 1;
                        if r == 0 {
                            let e = high.entry(name.clone()).or_insert((0, 0.0, o));
                            e.0 += 1;
                            e.1 += blo.z - z;
                        }
                    }
                    _ => counts[r][2] += 1,
                }
            }
        }
    }
    // Every reading of the four words: the components in any order with any signs.
    // Scored by how many placements stand upright (up axis within 10 degrees of
    // +z: most props do) and, among the placements tilted as read, how many rest.
    {
        let mut readings: Vec<([usize; 4], [f32; 3], usize, usize, usize, f32)> = Vec::new();
        let perms: Vec<[usize; 4]> = (0..24)
            .map(|mut k| {
                let mut pool = vec![0usize, 1, 2, 3];
                let mut p = [0usize; 4];
                for (i, slot) in p.iter_mut().enumerate() {
                    let f = [6, 2, 1, 1][i];
                    *slot = pool.remove(k / f);
                    k %= f;
                }
                p
            })
            .collect();
        let mut inst_q: Vec<(Quat, Vec3, f32, bool, usize)> = Vec::new();
        for d in scene.draws.iter().filter(|d| d.model != u32::MAX) {
            if scene.draws.iter().position(|o| o.model == d.model) != scene.draws.iter().position(|o| std::ptr::eq(o, d)) {
                continue;
            }
            let heli = scene.model_names.get(d.model as usize).is_some_and(|n| n.ends_with("veh8_mil_air_blima_mp"));
            for k in d.first_instance..d.first_instance + d.instances {
                let inst = scene.instances[k as usize];
                let m = Mat3::from_cols(Vec3::new(inst.a0[0], inst.a0[1], inst.a0[2]), Vec3::from_slice(&inst.a1[..3]), Vec3::from_slice(&inst.a2[..3]));
                inst_q.push((Quat::from_mat3(&m), Vec3::from_slice(&inst.origin[..3]), inst.a0[3], heli, d.model as usize));
            }
        }
        let tilted_as_read: Vec<bool> = inst_q.iter().map(|(q, ..)| (Mat3::from_quat(*q) * Vec3::Z).z < 0.985).collect();
        for p in &perms {
            for sgn in 0..8u32 {
                let s = [if sgn & 1 != 0 { -1.0 } else { 1.0 }, if sgn & 2 != 0 { -1.0 } else { 1.0 }, if sgn & 4 != 0 { -1.0 } else { 1.0 }];
                let (mut upright, mut tilted_rest, mut tilted_n) = (0usize, 0usize, 0usize);
                let mut heli_up = 0.0f32;
                for (i, (q, o, sc, heli, model)) in inst_q.iter().enumerate() {
                    let w = [q.x, q.y, q.z, q.w];
                    let r = Quat::from_xyzw(w[p[0]] * s[0], w[p[1]] * s[1], w[p[2]] * s[2], w[p[3]]).normalize();
                    let mat = Mat3::from_quat(r);
                    let upz = (mat * Vec3::Z).z;
                    upright += usize::from(upz > 0.985);
                    if *heli {
                        heli_up = upz;
                    }
                    if tilted_as_read[i] && o.length() < 12000.0 {
                        tilted_n += 1;
                        // (The model's own vertices, a few, for its box.)
                        let (mut blo, mut bhi) = (Vec3::splat(f32::MAX), Vec3::splat(f32::MIN));
                        for dd in scene.draws.iter().filter(|dd| dd.model as usize == *model).take(2) {
                            for t in (dd.first_index..dd.first_index + dd.index_count).step_by(9) {
                                let v = Vec3::from_array(scene.vertices[scene.indices[t as usize] as usize].pos);
                                let pp = *o + mat * v * *sc;
                                blo = blo.min(pp);
                                bhi = bhi.max(pp);
                            }
                        }
                        let c = (blo + bhi) * 0.5;
                        if let Some(z) = floor_below(Vec3::new(c.x, c.y, blo.z), blo.z + 24.0)
                            && (blo.z - z).abs() <= 12.0
                        {
                            tilted_rest += 1;
                        }
                    }
                }
                readings.push((*p, s, upright, tilted_rest, tilted_n, heli_up));
            }
        }
        readings.sort_by(|a, b| b.2.cmp(&a.2));
        println!("readings of the placement words by upright placements ({} placements, {} tilted as read):", inst_q.len(), tilted_as_read.iter().filter(|t| **t).count());
        for (p, s, up, tr, tn, hu) in readings.iter().take(8) {
            println!("  order {p:?} signs {s:?}: upright {up}, tilted-as-read resting {tr} of {tn}, helicopter up.z {hu:.2}");
        }
        if let Some(r) = readings.iter().position(|r| r.0 == [0, 1, 2, 3] && r.1 == [1.0, 1.0, 1.0]) {
            let (_, _, up, tr, tn, hu) = readings[r];
            println!("  as read (rank {}): upright {up}, tilted-as-read resting {tr} of {tn}, helicopter up.z {hu:.2}", r + 1);
        }
        let mut by_rest = readings.clone();
        by_rest.sort_by(|a, b| b.3.cmp(&a.3));
        for (p, s, up, tr, tn, hu) in by_rest.iter().take(4) {
            println!("  by tilted resting: order {p:?} signs {s:?}: upright {up}, resting {tr} of {tn}, helicopter up.z {hu:.2}");
        }
    }
    // (`--check-model <name part>`: each placement of the matching models, its box
    // as read and the floor under it.)
    if let Some(want) = arg("--check-model") {
        for d in scene.draws.iter().filter(|d| scene.model_names.get(d.model as usize).is_some_and(|n| n.contains(&want))) {
            if scene.draws.iter().position(|o| o.model == d.model) != scene.draws.iter().position(|o| std::ptr::eq(o, d)) {
                continue;
            }
            let verts: Vec<Vec3> = scene.draws.iter().filter(|o| o.model == d.model).flat_map(|o| (o.first_index..o.first_index + o.index_count).map(|i| Vec3::from_array(scene.vertices[scene.indices[i as usize] as usize].pos))).collect();
            let (llo, lhi) = verts.iter().fold((Vec3::splat(f32::MAX), Vec3::splat(f32::MIN)), |(a, b), v| (a.min(*v), b.max(*v)));
            println!("model {}: local box {llo:.0?} .. {lhi:.0?}", scene.model_names[d.model as usize]);
            for k in d.first_instance..d.first_instance + d.instances {
                let inst = scene.instances[k as usize];
                let m = Mat3::from_cols(Vec3::new(inst.a0[0], inst.a0[1], inst.a0[2]), Vec3::from_slice(&inst.a1[..3]), Vec3::from_slice(&inst.a2[..3]));
                let o = Vec3::from_slice(&inst.origin[..3]);
                let (blo, bhi) = verts.iter().fold((Vec3::splat(f32::MAX), Vec3::splat(f32::MIN)), |(a, b), v| {
                    let p = o + m * *v * inst.a0[3];
                    (a.min(p), b.max(p))
                });
                let c = (blo + bhi) * 0.5;
                let fl = floor_below(Vec3::new(c.x, c.y, bhi.z), bhi.z + 1.0);
                println!("  at {o:.0?}: box {blo:.0?} .. {bhi:.0?}, floor under its middle {fl:.0?}");
            }
        }
    }
    println!("placement check: {placed} placements within the world, {} world triangles", tris.len());
    for (r, n) in names.iter().enumerate() {
        println!("  {n:>20}: resting {}, over 200 above the floor {}, other {}", counts[r][0], counts[r][1], counts[r][2]);
    }
    let mut hv: Vec<_> = high.into_iter().collect();
    hv.sort_by(|a, b| b.1.0.cmp(&a.1.0));
    for (n, (c, gap, o)) in hv.iter().take(30) {
        println!("  over 200 above the floor as read: {n} x{c}, mean gap {:.0}, e.g. at {o:.0?}", gap / *c as f32);
    }
    // The models one reading rests far more often than the one as read.
    let mut diff: Vec<(i64, String, [usize; 5])> = per_model.into_iter().map(|(n, c)| ((c[1].max(c[2]).max(c[3]) as i64 - c[0] as i64), n, c)).collect();
    diff.sort_by(|a, b| b.0.cmp(&a.0));
    for (dd, n, c) in diff.iter().take(25).filter(|x| x.0 > 0) {
        println!("  rests better read otherwise (+{dd}): {n} {c:?}");
    }
}

fn f16(b: u16) -> f32 {
    let e = i32::from((b >> 10) & 31);
    let m = f32::from(b & 1023);
    let v = if e == 0 { m / 1024.0 * 2f32.powi(-14) } else { (1.0 + m / 1024.0) * 2f32.powi(e - 15) };
    if b & 0x8000 != 0 { -v } else { v }
}

fn main() {
    let zone = PathBuf::from(arg("--zone").expect("--zone <map.ff>"));
    let out = PathBuf::from(arg("--out").expect("--out <dir>"));
    let (w, h): (u32, u32) = (arg("--width").and_then(|v| v.parse().ok()).unwrap_or(1280), arg("--height").and_then(|v| v.parse().ok()).unwrap_or(720));
    let ooz_path = arg("--ooz").or_else(|| std::env::var("JUP_OOZ").ok()).unwrap_or_else(|| "/home/claude/bo2-multiplayer-rebuild/context/externals/ooz/libooz.so".into());
    if let Some(p) = arg("--pack") {
        // SAFETY: set at the start of main, before any thread is started.
        unsafe { std::env::set_var("JUP_PACK_DIR", p) };
    }
    let lib = unsafe { libloading::Library::new(&ooz_path) }.unwrap_or_else(|e| panic!("{ooz_path}: {e}"));
    std::fs::create_dir_all(&out).expect("out dir");

    let started = std::time::Instant::now();
    let scene = match Scene::load_with(&zone, Box::new(Ooz { lib })) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("scene: {e}");
            std::process::exit(1);
        }
    };
    println!("scene loaded in {:.1}s", started.elapsed().as_secs_f32());
    for l in &scene.report {
        println!("  {l}");
    }
    let (mins, maxs) = (Vec3::from_array(scene.mins), Vec3::from_array(scene.maxs));
    println!("bounds {mins:?} .. {maxs:?}");
    println!("env {:?}", scene.env);
    // `--dump-tools <dir>`: the pictures of the materials taken as tool materials, and how much each draws.
    if let Some(dir) = arg("--dump-tools") {
        std::fs::create_dir_all(&dir).expect("tools dir");
        for (mi, m) in scene.materials.iter().enumerate().filter(|(_, m)| m.tool) {
            let Some(c) = m.colour[0] else { continue };
            let tris: u64 = scene.draws.iter().filter(|d| d.material as usize == mi).map(|d| u64::from(d.index_count / 3) * u64::from(d.instances)).sum();
            let draws = scene.draws.iter().filter(|d| d.material as usize == mi).count();
            let (mut lo, mut hi) = ([f32::MAX; 3], [f32::MIN; 3]);
            for d in scene.draws.iter().filter(|d| d.material as usize == mi) {
                for a in 0..3 {
                    lo[a] = lo[a].min(d.mins[a]);
                    hi[a] = hi[a].max(d.maxs[a]);
                }
            }
            println!("tool material {mi} {:016x}: picture {:016x}, {draws} draws, {tris} triangles drawn, within {lo:.0?} .. {hi:.0?}", m.hash, scene.images[c].hash);
            dump_picture(&Path::new(&dir).join(format!("{:016x}.png", scene.images[c].hash)), &scene.images[c]);
        }
    }
    if std::env::args().any(|a| a == "--check-placement") {
        check_placement(&scene);
    }
    let text = match arg("--ents") {
        Some(p) => std::fs::read_to_string(p).expect("ents file"),
        None => scene.entities.clone(),
    };
    let ents = entities(&text);
    println!("{} entities with an origin", ents.len());

    // The views.
    let mut views: Vec<(String, Vec3, Vec3)> = Vec::new();
    for e in ents.iter().filter(|e| e.class == "info_player_start" || e.class == "mp_global_intermission") {
        views.push((e.class.clone(), e.origin + Vec3::new(0.0, 0.0, 60.0), e.angles));
    }
    // Spawn points of the modes (`*_spawn`), three spread through the list, at head height.
    {
        let mut hist: std::collections::BTreeMap<&str, usize> = Default::default();
        for e in &ents {
            *hist.entry(e.class.as_str()).or_default() += 1;
        }
        println!("entity classes: {hist:?}");
        // Mode spawns (`mp_*_spawn*`) where the entities have them, facing along their angles.
        let spawns: Vec<&Ent> = ents.iter().filter(|e| e.class.starts_with("mp_") && e.class.contains("spawn")).collect();
        if !spawns.is_empty() {
            for k in [spawns.len() / 6, spawns.len() * 2 / 5, spawns.len() * 3 / 5, spawns.len() * 5 / 6] {
                if let Some(e) = spawns.get(k) {
                    views.push((format!("spawn{k}"), e.origin + Vec3::new(0.0, 0.0, 60.0), e.angles));
                }
            }
        } else {
            // (MW3's map entities carry no mode spawns, neither the zone's nor the exported
            // spawnvars: the game makes them at run time. Ground views stand on path nodes
            // chosen for an open view: not boxed in, not against a wall, looking down the
            // most open quarter.)
            let nodes: Vec<&Ent> = ents.iter().filter(|e| e.class == "node_pathnode").collect();
            for (k, (eye, angles)) in open_views(&scene, &nodes, 4).into_iter().enumerate() {
                views.push((format!("ground{k}"), eye, angles));
            }
        }
    }
    let cams: Vec<&Ent> = ents.iter().filter(|e| e.class == "perf_camera").collect();
    for k in [0usize, cams.len() / 3, 2 * cams.len() / 3] {
        if let Some(e) = cams.get(k) {
            views.push((format!("perf_camera{k}"), e.origin, e.angles));
        }
    }
    // The playable area: the entities' middle 96 per cent (the scene's own bounds
    // include far sheets and vistas).
    let (centre, span) = {
        let mut lo = [0f32; 3];
        let mut hi = [0f32; 3];
        for a in 0..3 {
            let mut v: Vec<f32> = ents.iter().map(|e| e.origin[a]).collect();
            v.sort_by(f32::total_cmp);
            if v.is_empty() {
                continue;
            }
            lo[a] = v[v.len() * 2 / 100];
            hi[a] = v[(v.len() * 98 / 100).min(v.len() - 1)];
        }
        let (lo, hi) = (Vec3::from_array(lo), Vec3::from_array(hi));
        ((lo + hi) * 0.5, (hi - lo).max_element())
    };
    let _ = (mins, maxs);
    views.push(("overview".into(), centre + Vec3::new(-span * 0.6, -span * 0.6, span * 0.55), Vec3::new(45.0, 45.0, 0.0)));
    if let Some(e) = ents.iter().find(|e| e.class == "info_player_start") {
        views.push(("sky".into(), e.origin + Vec3::new(0.0, 0.0, 60.0), Vec3::new(-25.0, e.angles.y, 0.0)));
    }
    // `--cam name:x,y,z,pitch,yaw[;name:...]` adds views.
    if let Some(spec) = arg("--cam") {
        for one in spec.split(';') {
            let Some((name, nums)) = one.split_once(':') else { continue };
            let f: Vec<f32> = nums.split(',').filter_map(|v| v.trim().parse().ok()).collect();
            if f.len() >= 5 {
                views.push((name.to_owned(), Vec3::new(f[0], f[1], f[2]), Vec3::new(f[3], f[4], 0.0)));
            }
        }
    }
    if let Some(only) = arg("--only") {
        views.retain(|v| v.0.contains(&only));
    }

    if let Some(spec) = arg("--pick") {
        // `--pick <view name part>:x,y[;x,y...]`: what the world's triangles under those pixels are.
        let (name, px) = spec.split_once(':').unwrap_or((&spec, "640,360"));
        if let Some((vn, eye, angles)) = views.iter().find(|v| v.0.contains(name)) {
            let camera = make_camera(*eye, *angles, w, h);
            let rot = camera.rotation.transpose();
            for xy in px.split(';') {
                let Some((x, y)) = xy.split_once(',') else { continue };
                let (x, y): (f32, f32) = (x.parse().unwrap_or(0.0), y.parse().unwrap_or(0.0));
                let nx = (x + 0.5) / w as f32 * 2.0 - 1.0;
                let ny = 1.0 - (y + 0.5) / h as f32 * 2.0;
                let d = rot.transform_vector3(Vec3::new(nx * camera.tan_x, ny * camera.tan_y, 1.0)).normalize();
                let mut best: Option<(f32, usize, usize, usize)> = None;
                let mut hits: Vec<(f32, usize)> = Vec::new();
                for (di, dr) in scene.draws.iter().enumerate() {
                    // (The draw's box first: a ray that misses it meets none of its triangles.)
                    let (lo, hi) = (Vec3::from_array(dr.mins), Vec3::from_array(dr.maxs));
                    let (mut t0, mut t1) = (0f32, f32::MAX);
                    for a in 0..3 {
                        let inv = 1.0 / d[a];
                        let (ta, tb) = ((lo[a] - eye[a]) * inv, (hi[a] - eye[a]) * inv);
                        t0 = t0.max(ta.min(tb));
                        t1 = t1.min(ta.max(tb));
                    }
                    if t0 > t1 {
                        continue;
                    }
                    for k in dr.first_instance..dr.first_instance + dr.instances {
                        let inst = scene.instances[k as usize];
                        let (a0, a1, a2, o) = (Vec3::new(inst.a0[0], inst.a0[1], inst.a0[2]), Vec3::from_slice(&inst.a1[..3]), Vec3::from_slice(&inst.a2[..3]), Vec3::from_slice(&inst.origin[..3]));
                        let place = |p: [f32; 3]| o + (a0 * p[0] + a1 * p[1] + a2 * p[2]) * inst.a0[3];
                        for t in 0..(dr.index_count / 3) as usize {
                            let i0 = dr.first_index as usize + 3 * t;
                            let v = |k: usize| place(scene.vertices[scene.indices[i0 + k] as usize].pos);
                            let (a, b, c) = (v(0), v(1), v(2));
                            // Moller-Trumbore, both sides.
                            let e1 = b - a;
                            let e2 = c - a;
                            let p = d.cross(e2);
                            let det = e1.dot(p);
                            if det.abs() < 1e-9 {
                                continue;
                            }
                            let inv = 1.0 / det;
                            let tv = *eye - a;
                            let u = tv.dot(p) * inv;
                            if !(0.0..=1.0).contains(&u) {
                                continue;
                            }
                            let q = tv.cross(e1);
                            let vv = d.dot(q) * inv;
                            if vv < 0.0 || u + vv > 1.0 {
                                continue;
                            }
                            let tt = e2.dot(q) * inv;
                            if tt > 1.0 {
                                hits.push((tt, di));
                            }
                            if tt > 1.0 && best.is_none_or(|b| tt < b.0) {
                                best = Some((tt, di, t, k as usize));
                            }
                        }
                    }
                }
                if best.is_some() {
                    hits.sort_by(|a, b| a.0.total_cmp(&b.0));
                    hits.dedup_by_key(|h| h.1);
                    let near: Vec<String> = hits
                        .iter()
                        .take(6)
                        .map(|h| {
                            let m = &scene.materials[scene.draws[h.1].material as usize];
                            format!("{:.0}: draw {} mat {}{}{}{}", h.0, h.1, scene.draws[h.1].material, if m.blend { " blend" } else { "" }, if m.tool { " tool" } else { "" }, if m.cutout { " cutout" } else { "" })
                        })
                        .collect();
                    println!("  hits along the ray: {}", near.join("; "));
                }
                match best {
                    Some((t, di, tri, inst)) => {
                        let dr = &scene.draws[di];
                        if dr.model != u32::MAX {
                            let i = scene.instances[inst];
                            println!("  model {} {} placed at {:?} axes {:?} {:?} {:?}", dr.model, scene.model_names.get(dr.model as usize).map_or("?", String::as_str), i.origin, i.a0, i.a1, i.a2);
                        }
                        let m = &scene.materials[dr.material as usize];
                        let img = |o: Option<usize>| o.map_or("-".to_owned(), |i| format!("{:016x}({}x{} dxgi {})", scene.images[i].hash, scene.images[i].width, scene.images[i].height, scene.images[i].dxgi));
                        if let Some(dir) = arg("--pick-dump") {
                            let all = m.colour.iter().chain(m.normal.iter()).chain(m.mask.iter()).flatten();
                            for &i in all {
                                dump_picture(&Path::new(&dir).join(format!("{:016x}.png", scene.images[i].hash)), &scene.images[i]);
                            }
                        }
                        let i0 = dr.first_index as usize + 3 * tri;
                        let uv = |k: usize| scene.vertices[scene.indices[i0 + k] as usize].uv;
                        {
                            let i0 = dr.first_index as usize + 3 * tri;
                            let corners: Vec<_> = (0..3).map(|k| scene.vertices[scene.indices[i0 + k] as usize]).collect();
                            println!("  corners {:?} {:?} {:?} normals {:?} {:?} {:?}", corners[0].pos, corners[1].pos, corners[2].pos, corners[0].normal, corners[1].normal, corners[2].normal);
                        }
                        println!(
                            "pick {vn} ({x},{y}): distance {t:.0} draw {di} material {} hash {:016x} layers {} cutout {} blend {}\n  colour {} | {} | {} | {}\n  normal {} | {} | {} | {}\n  mask {} | {} | {}\n  uv {:?} {:?} {:?}",
                            dr.material, m.hash, m.count, m.cutout, m.blend,
                            img(m.colour[0]), img(m.colour[1]), img(m.colour[2]), img(m.colour[3]),
                            img(m.normal[0]), img(m.normal[1]), img(m.normal[2]), img(m.normal[3]),
                            img(m.mask[0]), img(m.mask[1]), img(m.mask[2]), uv(0), uv(1), uv(2)
                        );
                    }
                    None => println!("pick {vn} ({x},{y}): nothing"),
                }
            }
        }
    }
    let gpu = match Gpu::headless() {
        Ok(g) => g,
        Err(e) => {
            eprintln!("gpu: {e}");
            std::process::exit(1);
        }
    };
    let mut host = match Host::new(gpu.clone(), scene) {
        Ok(h) => h,
        Err(e) => {
            eprintln!("host: {e}");
            std::process::exit(1);
        }
    };
    for l in host.take_report() {
        println!("  host: {l}");
    }
    for (i, (name, eye, angles)) in views.iter().enumerate() {
        let t0 = std::time::Instant::now();
        let camera = make_camera(*eye, *angles, w, h);
        if i == 0 {
            println!("  view {name}: {}", if legacy_camera() { "legacy 90 degree lens".to_owned() } else { GameView::from_angles(*eye, *angles, w, h).describe(w, h) });
        }
        let mut enc = gpu.device.create_command_encoder(&Default::default());
        match host.record(&mut enc, w, h, &camera) {
            Ok(true) => {}
            other => {
                eprintln!("{name}: record {other:?}");
                continue;
            }
        }
        gpu.queue.submit([enc.finish()]);
        let (fin, albedo, normal) = host.debug_textures().expect("targets");
        let file = |s: &str| out.join(format!("{i:02}_{name}{s}.png"));
        write_png(&file(""), w, h, &read_texture(&gpu, fin, w, h, 4));
        if arg("--debug-all").is_some() || i == 0 {
            write_png(&file("_albedo"), w, h, &read_texture(&gpu, albedo, w, h, 4).chunks_exact(4).flat_map(|p| [p[0], p[1], p[2], 255]).collect::<Vec<u8>>());
            let n = read_texture(&gpu, normal, w, h, 8);
            let px: Vec<u8> = n
                .chunks_exact(8)
                .flat_map(|p| {
                    let c = |k: usize| (f16(u16::from_le_bytes([p[2 * k], p[2 * k + 1]])) * 0.5 + 0.5).clamp(0.0, 1.0);
                    [(c(0) * 255.0) as u8, (c(1) * 255.0) as u8, (c(2) * 255.0) as u8, 255]
                })
                .collect();
            write_png(&file("_normal"), w, h, &px);
            let gloss: Vec<u8> = n.chunks_exact(8).flat_map(|p| {
                let g = (f16(u16::from_le_bytes([p[6], p[7]])).clamp(0.0, 1.0) * 255.0) as u8;
                [g, g, g, 255]
            }).collect();
            write_png(&file("_gloss"), w, h, &gloss);
        }
        println!("{name}: eye {eye:?} angles {angles:?} in {:.1}s", t0.elapsed().as_secs_f32());
    }
    for l in host.take_report() {
        println!("  host: {l}");
    }
}
