//! `t7_sun_shadow_check <map.ff> [w h]`: are the sun shadows right, read
//! exactly as the lighting reads them?
//!
//! Lean (no G-buffer, no lighting: fits a small software adapter): the
//! map's sun shadow tree is filled into the three cascades as the frame does
//! (`render_t7::sun_shadow`: the exe's fit, `depth_fill_from_sst`,
//! `floatz_writedepth`) and read back. For each view (`T7_EYE="x y z pitch
//! yaw"`, several separated by `;`, else a few deathmatch spawns looked at
//! from above), a ray through each pixel of a `w`x`h` grid finds the surface
//! seen there, and that point is shadowed the way `deferred_lighting_
//! permuted` does it (DXBC, `core_patch`): the eye-relative position through
//! the sun constants' lookup matrix (floats 44..59), the reference
//! `1 - (z - f3)/16384`, the first cascade whose band (floats 12+4c..)
//! holds it, `uv = 0.5 ± 0.5·d/s`, layer `c + f37`, the comparison sampler
//! (`GreaterEqual`, linear: 2x2 PCF) and `saturate(pcf + trans - 1)³` with
//! the translucent map white. The truth is a CPU ray from the point toward
//! the sun through the scene's opaque triangles.
//!
//! Per cascade it prints the agreement outside the penumbra (points whose
//! four neighbours 1.5 texels away in the sun's plane see the same as the
//! point) and outside the exe's own contact zone (an occluder closer than
//! the fill's bias, `(max(s30/r, 1) + 1)·r·√2·s38`, lights the point in the
//! exe too), the texel shift and depth offset that would agree best (a
//! misplaced map shows there), and, for the points the map shadows, the
//! ambient the probe volume gives them (none: drawn black).
//!
//! The scene lacks what is not built offline (models whose meshes stream
//! from packages not staged) while the tree holds every caster: "map dark,
//! ray lit" is expected for some points, "map lit, ray dark" should be rare.
//! `T7_SUN_SHADOW_PNG=<file>`: the three layers one above the other.
use glam::{Vec2, Vec3, Vec3A};

/// One seen point.
struct Pt {
    p: Vec3A,
    n: Vec3A,
    /// The pixel (row-major).
    px: usize,
}

/// The lighting's read of one point: cascade (3: past them), the shadow
/// term and the reference depth.
#[derive(Clone, Copy)]
struct Read {
    c: usize,
    shadow: f32,
    /// How far behind the map's nearest texel the point is (sun units).
    behind: f32,
}

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let zone = std::path::PathBuf::from(a.get(1).expect("map zone"));
    let gw: u32 = a.get(2).and_then(|s| s.parse().ok()).unwrap_or(64);
    let gh: u32 = a.get(3).and_then(|s| s.parse().ok()).unwrap_or(36);
    let scene = render_t7::scene::Scene::load(&zone).expect("scene");
    let map = &scene.map;
    let (img, cap) = map.zone_data().expect("zone data");
    let volume = cap.world.gfx_worlds.first().and_then(|g| g.sun_volumes.first().copied()).expect("sun volume");
    let settings = fastfile_t7::native::global_light_settings(&img, volume, 0).expect("light settings");
    let sun = fastfile_t7::native::view_sun(&settings).with_volume(&img, volume);
    println!("sun: direction {:?}, shadow mode {}, colour {:?}, s30 {} s34 {} s38 {}", sun.direction, sun.shadow_mode, sun.colour, sun.f(0x30), sun.f(0x34), sun.f(0x38));
    // The tree's depth axis (the world direction its z grows along) against
    // the lighting's sun direction (floats 0..2): -1 when they agree. (Read
    // from the zone: no package needed.)
    if let Some(t) = map.light_refs().and_then(|r| r.shadow_tree.clone())
        && let Some(info) = render_t7::sun_shadow::ShadowInfo::read(&t.info, &t.record)
    {
        let m = info.matrix;
        let z = Vec3::new(m[0][2], m[1][2], m[2][2]);
        let (x, y) = (Vec3::new(m[0][0], m[1][0], m[2][0]), Vec3::new(m[0][1], m[1][1], m[2][1]));
        println!("tree: depth axis {} (length {}), x·y {}, x·z {}; sun direction {:?}; dot {}; resident levels from {}, streamed {}", z.normalize(), z.length(), x.dot(y), x.dot(z), sun.direction, z.normalize().dot(Vec3::from(sun.direction)), info.ints[0], info.streamed);
    }
    if std::env::var("T7_AXIS_ONLY").is_ok() {
        return;
    }
    let gpu = render_h1::gpu::Gpu::headless().expect("gpu");
    let patch = fastfile_t7::shared::zone(zone.parent().unwrap(), "core_patch").expect("core_patch");
    let shaders = render_t7::compute::code_shaders(&patch.image, &patch.capture);
    let mut ss = render_t7::sun_shadow::SunShadows::new(&gpu, map, &patch.image, &patch.capture, &shaders).expect("sun shadows");
    for l in &ss.report {
        println!("{l}");
    }
    // The views.
    let mut spawns = Vec::new();
    for block in map.entities.split('}') {
        if !block.contains("mp_dm_spawn") && !block.contains("mp_tdm_spawn") {
            continue;
        }
        let val = |k: &str| block.split('\n').find_map(|l| l.trim().strip_prefix(&format!("\"{k}\" \"")).map(|v| v.trim_end_matches('"').to_owned()));
        if let Some(o) = val("origin") {
            let v: Vec<f32> = o.split_whitespace().filter_map(|x| x.parse().ok()).collect();
            if v.len() == 3 {
                let yaw = val("angles").and_then(|a| a.split_whitespace().nth(1).and_then(|y| y.parse().ok())).unwrap_or(0.0);
                spawns.push((Vec3::new(v[0], v[1], v[2]), yaw));
            }
        }
    }
    let mut views: Vec<(Vec3, f32, f32)> = Vec::new();
    if let Ok(v) = std::env::var("T7_EYE") {
        for one in v.split(';') {
            let v: Vec<f32> = one.split_whitespace().filter_map(|x| x.parse().ok()).collect();
            if v.len() == 5 {
                views.push((Vec3::new(v[0], v[1], v[2]), v[3], v[4]));
            }
        }
    }
    if views.is_empty() {
        let n: usize = std::env::var("T7_VIEWS").ok().and_then(|v| v.parse().ok()).unwrap_or(6);
        for k in 0..n.min(spawns.len()) {
            let (o, yaw) = spawns[k * spawns.len() / n.min(spawns.len())];
            // Standing (looking a little down), and from 300 units up.
            views.push((o + Vec3::Z * 60.0, if k % 2 == 0 { 15.0 } else { 35.0 }, yaw));
            views.push((o + Vec3::Z * 300.0, 40.0, yaw + 30.0));
        }
    }
    // The scene's triangles; shadow rays only meet opaque surfaces.
    let tris = render_t7::completeness::scene_triangles(&scene);
    let opaque: Vec<bool> = scene
        .draws
        .iter()
        .map(|d| {
            let m = &scene.materials[d.material];
            let t = m.technique.as_ref().or(m.forward.as_ref());
            let blended = t.is_none_or(|t| render_t7::state::target_blend(t.state.blend[0]).is_ok_and(|(b, _)| b.is_some()));
            !blended && !m.techset.contains("sky") && !m.name.contains("vista") && !m.techset.contains("decal")
        })
        .collect();
    let all = std::env::var("T7_RAY_ALL").is_ok();
    let keep = |tag: u32| all || opaque.get(tag as usize).copied().unwrap_or(false);
    let toward = Vec3A::from(sun.direction).normalize();
    // The probe volume (the ambient a shadowed point keeps).
    let probe0 = map.light_refs().and_then(|r| r.probes.as_ref()).and_then(|p| p.configs.get(..328).map(<[u8]>::to_vec));
    let vol = fastfile_t7::light::probe_volume_rgba16f(map).map_err(|e| println!("probe volume: {e}")).ok();
    let s30 = sun.f(0x30);
    let s38 = sun.f(0x38);
    let res = render_t7::sun_shadow::MAP_SIZE as f32;
    let mut grand = [[0usize; 4]; 4];
    let mut grand_kept = [[0usize; 4]; 4];
    let mut black = (0usize, 0usize);
    for (vi, &(eye, pitch, yaw)) in views.iter().enumerate() {
        let view = render_h1::code::View { eye, angles: Vec3::new(pitch, yaw, 0.0), fov_x: 80.0, width: gw, height: gh, near: 4.0 };
        let cam = render_h1::code::Camera::from(&view);
        ss.update(&gpu, &sun, &cam);
        let st = ss.state.clone().expect("fit");
        let mut f = [0f32; 64];
        st.sun_constants(&mut f);
        let mut enc = gpu.device.create_command_encoder(&Default::default());
        ss.record(&mut enc);
        gpu.queue.submit([enc.finish()]);
        let layers = read_layers(&gpu, ss.maps());
        if vi == 0
            && let Ok(path) = std::env::var("T7_SUN_SHADOW_PNG")
        {
            write_png(&path, &layers);
        }
        // `T7_MOVE_TEST=1`: the camera steps and turns (sub-texel to many
        // tiles); the cascades are refitted and refilled on the GPU and the
        // first view's seen points read again through the lighting's
        // lookup: a static point's shadow and depth against the map must
        // not change (shadows that follow the player change here).
        if vi == 0 && std::env::var("T7_MOVE_TEST").is_ok() {
            let inv = cam.view_projection.inverse();
            let mut probe = Vec::new();
            for y in (0..gh).step_by(2) {
                for x in (0..gw).step_by(2) {
                    let ndc = glam::Vec4::new((x as f32 + 0.5) / gw as f32 * 2.0 - 1.0, 1.0 - (y as f32 + 0.5) / gh as f32 * 2.0, 0.5, 1.0);
                    let q = inv * ndc;
                    let dir = Vec3A::from((q.truncate() / q.w).normalize());
                    if let Some((t, _, n)) = tris.trace_normal(Vec3A::from(eye), dir, 20000.0, &keep) {
                        let n = if n.dot(dir) > 0.0 { -n } else { n };
                        if n.dot(toward) > 0.0 {
                            probe.push(Vec3A::from(eye) + dir * t + n * 0.5);
                        }
                    }
                }
            }
            let base: Vec<Read> = probe.iter().map(|&p| lighting_read(&f, Vec3A::from(eye), &layers, p, Vec2::ZERO, 0.0)).collect();
            for (step, dp, dyaw) in [(0, Vec3::new(0.37, 0.21, 0.0), 0.0), (1, Vec3::new(13.7, -5.3, 1.2), 2.0), (2, Vec3::new(41.0, 33.0, 0.0), -7.0), (3, Vec3::new(-180.0, 95.0, 30.0), 25.0)] {
                let e2 = eye + dp;
                let v2 = render_h1::code::View { eye: e2, angles: Vec3::new(pitch, yaw + dyaw, 0.0), fov_x: 80.0, width: gw, height: gh, near: 4.0 };
                let c2 = render_h1::code::Camera::from(&v2);
                ss.update(&gpu, &sun, &c2);
                let st2 = ss.state.clone().expect("fit");
                let mut f2 = [0f32; 64];
                st2.sun_constants(&mut f2);
                let mut enc = gpu.device.create_command_encoder(&Default::default());
                ss.record(&mut enc);
                gpu.queue.submit([enc.finish()]);
                let l2 = read_layers(&gpu, ss.maps());
                let (mut same_c, mut flips, mut depth_moved, mut n) = (0, 0, 0, 0);
                for (p, b) in probe.iter().zip(&base) {
                    let r = lighting_read(&f2, Vec3A::from(e2), &l2, *p, Vec2::ZERO, 0.0);
                    if r.c != b.c || r.c == 3 {
                        continue;
                    }
                    same_c += 1;
                    n += 1;
                    if (r.shadow < 0.5) != (b.shadow < 0.5) {
                        flips += 1;
                    }
                    if (r.behind - b.behind).abs() > 0.5 {
                        depth_moved += 1;
                    }
                }
                println!("  move test {step}: eye +{dp} yaw {dyaw:+}: {same_c} of {} points in the same cascade; shadow changed on {flips}, map depth under the point moved > 0.5 units on {depth_moved} of {n}", probe.len());
            }
            ss.update(&gpu, &sun, &cam);
        }
        println!("\nview {vi}: eye {eye} pitch {pitch} yaw {yaw}; cascades {:?} texels {:?}; z base {}", st.distance, st.texel, st.z_base);
        // Pixels: primary rays.
        let inv = cam.view_projection.inverse();
        let mut pts = Vec::new();
        for y in 0..gh {
            for x in 0..gw {
                let ndc = glam::Vec4::new((x as f32 + 0.5) / gw as f32 * 2.0 - 1.0, 1.0 - (y as f32 + 0.5) / gh as f32 * 2.0, 0.5, 1.0);
                let p = inv * ndc;
                // (The camera's view-projection is eye-relative.)
                let dir = Vec3A::from((p.truncate() / p.w).normalize());
                if let Some((t, _, n)) = tris.trace_normal(Vec3A::from(eye), dir, 20000.0, &keep) {
                    // The face toward the eye.
                    let n = if n.dot(dir) > 0.0 { -n } else { n };
                    pts.push(Pt { p: Vec3A::from(eye) + dir * t, n, px: (y * gw + x) as usize });
                }
            }
        }
        let lookup = |p: Vec3A| -> [f32; 3] {
            let r = p - Vec3A::from(eye);
            std::array::from_fn(|j| r.x * f[44 + j] + r.y * f[48 + j] + r.z * f[52 + j] + f[56 + j])
        };
        // The sun plane's axes in the world (for the penumbra neighbours).
        let ax_u = Vec3A::new(f[44], f[48], f[52]).normalize();
        let ax_v = Vec3A::new(f[45], f[49], f[53]).normalize();
        let read = |p: Vec3A, shift: Vec2, dz: f32| -> Read { lighting_read(&f, Vec3A::from(eye), &layers, p, shift, dz) };
        // Truth: a ray toward the sun from just off the surface.
        let ray = |p: Vec3A, n: Vec3A| -> Option<f32> { tris.trace(p + n * 0.25 + toward * 0.5, toward, 30000.0, &keep).map(|(t, _)| t + 0.5) };
        let mut stats = [[0usize; 4]; 4];
        let mut kept = [[0usize; 4]; 4];
        let mut excluded = [(0usize, 0usize, 0usize, 0usize); 4];
        let mut keep_pts: Vec<(usize, bool)> = Vec::new();
        // (cascade, CPU read lit, ray lit or None when excluded, pixel, n·l)
        let mut per_px: Vec<(usize, bool, Option<bool>, usize, f32)> = Vec::new();
        let dump: usize = std::env::var("T7_DUMP").ok().and_then(|v| v.parse().ok()).unwrap_or(0);
        let mut dumped = 0;
        for (i, pt) in pts.iter().enumerate() {
            // Facing away from the sun: no sun in the lighting either.
            if pt.n.dot(toward) <= 0.0 {
                continue;
            }
            let r = read(pt.p, Vec2::ZERO, 0.0);
            let hit = ray(pt.p, pt.n);
            let map_dark = r.shadow < 0.5;
            let k = usize::from(map_dark) * 2 + usize::from(hit.is_some());
            stats[r.c][k] += 1;
            // A caster the offline scene lacks (a static model whose mesh is
            // not built): an unbuilt placement near the sun ray before the
            // map's occluder.
            let unbuilt = (k == 2).then(|| near_unbuilt(&scene, pt.p, toward, r.behind.max(0.0) + 64.0)).flatten();
            per_px.push((r.c, !map_dark, None, pt.px, pt.n.dot(toward)));
            if k == 2 && dumped < dump {
                dumped += 1;
                println!("  map dark, ray lit: cascade {} p {} n {} behind the map by {:.1}; unbuilt near the ray {:?}", r.c, Vec3::from(pt.p), Vec3::from(pt.n), r.behind, unbuilt);
            }
            if unbuilt.is_some() {
                excluded[r.c.min(3)].3 += 1;
                continue;
            }
            if r.c == 3 {
                continue;
            }
            let texel = st.texel[r.c];
            // Penumbra: a neighbour 1.5 texels away sees otherwise.
            // (Neighbours along the surface: the sun plane's axes made
            // tangent, so no neighbour starts inside the surface.)
            let pen = [ax_u, -ax_u, ax_v, -ax_v].iter().any(|a| {
                let t = (*a - pt.n * a.dot(pt.n)).normalize_or_zero();
                ray(pt.p + t * 1.5 * texel, pt.n).is_some() != hit.is_some()
            });
            let contact = (s30 / texel).max(1.0) + 1.0;
            let contact = contact * texel * std::f32::consts::SQRT_2 * s38;
            let near = hit.is_some_and(|t| t < contact.max(2.0 * texel));
            if pen {
                excluded[r.c].0 += 1;
            } else if near {
                excluded[r.c].1 += 1;
            } else {
                kept[r.c][k] += 1;
                keep_pts.push((i, hit.is_some()));
                if let Some(last) = per_px.last_mut() {
                    last.2 = Some(hit.is_none());
                }
            }
            // Shadowed by the map: the ambient left.
            if map_dark && let (Some((vr, vx)), Some(cfg)) = (&vol, &probe0) {
                let g = render_t7::lighting_globals::global_probe(cfg, eye.to_array());
                // (Sun float 8, the probe units: the global probe's config word 0
                // as the globals builder writes it; the shadow state leaves it.)
                let units = f32::from_le_bytes(cfg[0..4].try_into().unwrap());
                let amb = ambient(vr, vx, &g, pt.p - Vec3A::from(eye), pt.n) * units.max(0.0);
                black.0 += 1;
                if amb < 1e-3 {
                    black.1 += 1;
                    excluded[r.c].2 += 1;
                }
            }
        }
        // Past the cascades the permutations read the tree itself (level
        // `max(streamed, 2)`, one cell a texel, floats 40..43 and 60..61):
        // lit when `z <= h + 2·cell`, `h` the tree's first-hit depth. The
        // tree's heights come from `depth_fill_from_sst` run over a window
        // of level 2 at one cell a texel around the points (cascade 2 of a
        // state of our own): `h = (zc - map)·16384`, `zc` the window's
        // DrawSST depth constant.
        // (`T7_FAR_SELF_TEST=1`: cascade 2's points instead, where the
        // window must agree with the cascade's own read.)
        let self_test = std::env::var("T7_FAR_SELF_TEST").is_ok();
        let mut left: Vec<usize> = pts.iter().enumerate().filter(|(_, pt)| pt.n.dot(toward) > 0.0 && read(pt.p, Vec2::ZERO, 0.0).c == if self_test { 2 } else { 3 }).map(|(i, _)| i).collect();
        let cell = st.info_cell;
        let tile = cell * 32.0;
        let half = res * cell * 0.5;
        let mut windows = 0;
        let mut far_dark: Vec<(usize, bool)> = Vec::new();
        while let Some(&first) = left.first() {
            if windows >= 8 || st.streamed > 2 {
                break;
            }
            windows += 1;
            let sp = lookup(pts[first].p);
            let x0 = ((sp[0] - half) / tile).floor() * tile;
            let y0 = ((sp[1] + half) / tile).ceil() * tile;
            let mut st2 = st.clone();
            st2.level[2] = 2;
            st2.texel[2] = cell;
            st2.corner[2] = [x0, y0];
            let k = 1.0 / (cell * 128.0);
            st2.uv[2] = [x0 * k + st.info_size[0] * 0.5, y0 * -k + st.info_size[1] * 0.5];
            let zc = f32::from_bits(st2.sst_constants()[8][0]);
            ss.set_state(&gpu, st2);
            let mut enc = gpu.device.create_command_encoder(&Default::default());
            ss.record(&mut enc);
            gpu.queue.submit([enc.finish()]);
            let win = read_layers(&gpu, ss.maps()).swap_remove(2);
            left.retain(|&i| {
                let sp = lookup(pts[i].p);
                let (u, v) = ((sp[0] - x0) / cell, (y0 - sp[1]) / cell);
                if !(0.0..res).contains(&u) || !(0.0..res).contains(&v) {
                    return true;
                }
                let m = win[v as usize * res as usize + u as usize];
                let h = (zc - m) * 16384.0;
                far_dark.push((i, sp[2] > h + 2.0 * cell));
                if dump > 0 && sp[2] > h + 2.0 * cell && ray(pts[i].p, pts[i].n).is_none() {
                    println!("  tree read dark, ray lit: p {} n {} behind the first hit by {:.1}; unbuilt near the ray {:?}", Vec3::from(pts[i].p), Vec3::from(pts[i].n), sp[2] - h, near_unbuilt(&scene, pts[i].p, toward, sp[2] - h + 64.0));
                }
                false
            });
        }
        if self_test {
            let same = far_dark.iter().filter(|&&(i, d)| (read(pts[i].p, Vec2::ZERO, 0.0).shadow < 0.5) == d).count();
            println!("  tree read self test: the window agrees with cascade 2 on {same} of {} points", far_dark.len());
            far_dark.clear();
        }
        for &(i, dark) in &far_dark {
            let pt = &pts[i];
            let hit = ray(pt.p, pt.n);
            let k = usize::from(dark) * 2 + usize::from(hit.is_some());
            let pen = [ax_u, -ax_u, ax_v, -ax_v].iter().any(|a| {
                let t = (*a - pt.n * a.dot(pt.n)).normalize_or_zero();
                ray(pt.p + t * 1.5 * cell, pt.n).is_some() != hit.is_some()
            });
            let unbuilt = (k == 2).then(|| near_unbuilt(&scene, pt.p, toward, 4096.0)).flatten();
            if pen {
                excluded[3].0 += 1;
            } else if hit.is_some_and(|t| t < 2.0 * cell) {
                excluded[3].1 += 1;
            } else if unbuilt.is_none() {
                kept[3][k] += 1;
            }
            // (The `all` row for the tree read: this emulation's, not the lit default.)
            stats[3][k] += 1;
            stats[3][usize::from(hit.is_some())] -= 1;
        }
        if !left.is_empty() || windows > 0 {
            println!("  tree read emulated for {} points in {windows} windows ({} left)", far_dark.len(), left.len());
        }
        let pc = |s: &[usize; 4]| 100.0 * (s[0] + s[3]) as f32 / s.iter().sum::<usize>().max(1) as f32;
        println!("{} of {} pixels see a surface", pts.len(), gw * gh);
        // `T7_LIT_RAW=<file>`: the lit scene target `t7_lighting_check`
        // wrote for this view (`T7_LIGHTING_RAW`, same size, sun alone:
        // `BO2MP_T7_NATIVE_DEBUG=sunonly T7_NO_LIGHTS=1
        // T7_NO_PROBE_ARRAY=1`): the GPU's own shadow, pixel by pixel.
        if let Ok(path) = std::env::var("T7_LIT_RAW") {
            let raw: Vec<f32> = std::fs::read(&path).expect("lit raw").chunks(4).map(|b| f32::from_le_bytes(b.try_into().unwrap())).collect();
            let lum = |px: usize| 0.2126 * raw[3 * px] + 0.7152 * raw[3 * px + 1] + 0.0722 * raw[3 * px + 2];
            // Brightness per unit n·l of the points the CPU read leaves lit.
            let mut lit_v: Vec<f32> = per_px.iter().filter(|e| e.1 && e.4 > 0.2).map(|e| lum(e.3) / e.4).collect();
            lit_v.sort_by(f32::total_cmp);
            let tau = 0.3 * lit_v.get(lit_v.len() / 2).copied().unwrap_or(0.0);
            let mut gpu_cpu = [[0usize; 4]; 4];
            let mut gpu_ray = [[0usize; 4]; 4];
            for &(c, cpu_lit, ray_lit, px, ndl) in &per_px {
                if ndl <= 0.2 {
                    continue;
                }
                let g = lum(px) / ndl > tau;
                gpu_cpu[c][usize::from(!g) * 2 + usize::from(!cpu_lit)] += 1;
                if let Some(r) = ray_lit {
                    gpu_ray[c][usize::from(!g) * 2 + usize::from(!r)] += 1;
                }
            }
            println!("  GPU lighting (lit when brightness / n·l > {tau:.4}): vs the CPU read [both lit, CPU dark only, GPU dark only, both dark] per cascade {gpu_cpu:?}; vs the ray outside penumbra/contact {gpu_ray:?}");
            for c in 0..4 {
                println!("    {}: GPU vs CPU read agree {:.1}%, GPU vs ray agree {:.1}%", if c == 3 { "past the cascades".to_owned() } else { format!("cascade {c}") }, pc(&gpu_cpu[c]), pc(&gpu_ray[c]));
            }
        }
        for c in 0..4 {
            let name = if c == 3 { "past the cascades (tree read)".to_owned() } else { format!("cascade {c}") };
            println!("  {name}: all [both lit, ray dark only, map dark only, both dark] {:?} agree {:.1}%; outside penumbra/contact {:?} agree {:.1}% (penumbra {}, contact {}, unbuilt caster {}, black ambient {})", stats[c], pc(&stats[c]), kept[c], pc(&kept[c]), excluded[c].0, excluded[c].1, excluded[c].3, excluded[c].2);
            for k in 0..4 {
                grand[c][k] += stats[c][k];
                grand_kept[c][k] += kept[c][k];
            }
        }
        // The best texel shift and depth offset (a misplaced map shows here).
        let score = |shift: Vec2, dz: f32| -> [usize; 3] {
            let mut s = [0usize; 3];
            for &(i, dark) in &keep_pts {
                let r = read(pts[i].p, shift, dz);
                if r.c < 3 && (r.shadow < 0.5) == dark {
                    s[r.c] += 1;
                }
            }
            s
        };
        let base = score(Vec2::ZERO, 0.0);
        let mut best = (base.iter().sum::<usize>(), Vec2::ZERO, 0.0f32);
        for dy in -4..=4 {
            for dx in -4..=4 {
                for dz in [-16.0f32, -4.0, 0.0, 4.0, 16.0] {
                    let s: usize = score(Vec2::new(dx as f32, dy as f32), dz).iter().sum();
                    if s > best.0 {
                        best = (s, Vec2::new(dx as f32, dy as f32), dz);
                    }
                }
            }
        }
        println!("  agreeing points {} at no shift; best {} at texel shift {} depth {}", base.iter().sum::<usize>(), best.0, best.1, best.2);
    }
    let pc = |s: &[usize; 4]| 100.0 * (s[0] + s[3]) as f32 / s.iter().sum::<usize>().max(1) as f32;
    println!("\nall views:");
    for c in 0..4 {
        println!("  {}: {:?} agree {:.1}%; outside penumbra/contact {:?} agree {:.1}%", if c == 3 { "past the cascades".to_owned() } else { format!("cascade {c}") }, grand[c], pc(&grand[c]), grand_kept[c], pc(&grand_kept[c]));
    }
    println!("  shadowed points with no ambient (drawn black): {} of {}", black.1, black.0);
}

/// The probe volume's diffuse light at an eye-relative point with normal `n`
/// as the lighting permutations read it (`globalProbeConstants`: the box
/// mapped to 0..1, scaled into the volume, offset; each axis volume's half by
/// the normal's sign, weighted by the normal squared), nearest texel; the
/// luminance.
fn ambient(v: &fastfile_t7::light::VolumeRef, tex: &[Vec<u16>; 3], g: &[u32; 12], rel: Vec3A, n: Vec3A) -> f32 {
    let f = |i: usize| f32::from_bits(g[i]);
    let d = rel - Vec3A::new(f(0), f(1), f(2));
    let q = Vec3A::new(d.x * f(3), d.y * f(4), d.z * f(5)) + Vec3A::splat(0.5);
    let q = q.clamp(Vec3A::ZERO, Vec3A::ONE);
    let uvw = Vec3A::new(q.x * f(6), q.y * f(7), q.z * f(8)) + Vec3A::new(f(9), f(10), f(11));
    let mut sum = 0.0;
    for k in 0..3 {
        let off = if n[k] > 0.0 { 0.0 } else { 0.5 };
        let w = n[k] * n[k];
        let (x, y, z) = (uvw.x, uvw.y, uvw.z + off);
        let (xi, yi, zi) = (((x * v.width as f32) as usize).min(v.width - 1), ((y * v.height as f32) as usize).min(v.height - 1), ((z * v.depth as f32) as usize).min(v.depth - 1));
        let o = ((zi * v.height + yi) * v.width + xi) * 4;
        let h = |i: usize| f16(tex[k][o + i]);
        sum += w * (0.2126 * h(0) + 0.7152 * h(1) + 0.0722 * h(2));
        if std::env::var("T7_DEBUG_AMB").is_ok() {
            println!("amb k {k} g {:?} uvw {uvw} texel {xi} {yi} {zi} of {}x{}x{} rgb {} {} {}", g.map(f32::from_bits), v.width, v.height, v.depth, h(0), h(1), h(2));
        }
    }
    sum
}

/// The nearest unbuilt static model placement within 160 units of the ray
/// `p + t·d`, `t` in 0..`len`: (name, distance from the ray, along it).
fn near_unbuilt(scene: &render_t7::scene::Scene, p: Vec3A, d: Vec3A, len: f32) -> Option<(String, f32, f32)> {
    let mut best: Option<(String, f32, f32)> = None;
    for (name, o, _) in &scene.unbuilt {
        let o = Vec3A::from(*o);
        let t = (o - p).dot(d).clamp(0.0, len);
        let dist = (p + d * t - o).length();
        if dist < 160.0 && best.as_ref().is_none_or(|b| dist < b.1) {
            best = Some((name.clone(), dist, t));
        }
    }
    best
}

/// A point's sun shadow as `deferred_lighting_permuted` reads it (see the
/// module): the sun constants `f`, the view's eye, the cascade maps.
fn lighting_read(f: &[f32; 64], eye: Vec3A, layers: &[Vec<f32>], p: Vec3A, shift: Vec2, dz: f32) -> Read {
    let res = render_t7::sun_shadow::MAP_SIZE as f32;
    let lookup = |p: Vec3A| -> [f32; 3] {
        let r = p - eye;
        std::array::from_fn(|j| r.x * f[44 + j] + r.y * f[48 + j] + r.z * f[52 + j] + f[56 + j])
    };
            let sp = lookup(p);
            let refz = 1.0 - (sp[2] + dz - f[3]) / 16384.0;
            if !(0.0..=1.0).contains(&refz) {
                return Read { c: 3, shadow: 1.0, behind: 0.0 };
            }
            for c in 0..3 {
                let d = Vec2::new(sp[0] - f[12 + 4 * c], sp[1] - f[13 + 4 * c]);
                if d.x.abs().max(d.y.abs()) * f[14 + 4 * c] >= 1.0 {
                    continue;
                }
                let uv = Vec2::new(0.5 + 0.5 * d.x * f[14 + 4 * c], 0.5 - 0.5 * d.y * f[14 + 4 * c]);
                let layer = c + f[37].to_bits() as usize;
                let t = uv * res - Vec2::splat(0.5) + shift;
                let (x0, y0) = (t.x.floor(), t.y.floor());
                let (fx, fy) = (t.x - x0, t.y - y0);
                let tap = |x: f32, y: f32| -> f32 {
                    let (x, y) = ((x as i32).clamp(0, res as i32 - 1) as usize, (y as i32).clamp(0, res as i32 - 1) as usize);
                    let m = layers[layer.min(2)][y * res as usize + x];
                    if refz >= m { 1.0 } else { 0.0 }
                };
                let pcf = (tap(x0, y0) * (1.0 - fx) + tap(x0 + 1.0, y0) * fx) * (1.0 - fy) + (tap(x0, y0 + 1.0) * (1.0 - fx) + tap(x0 + 1.0, y0 + 1.0) * fx) * fy;
                let s = pcf.clamp(0.0, 1.0);
                let (cx, cy) = (((t.x + 0.5) as i32).clamp(0, res as i32 - 1) as usize, ((t.y + 0.5) as i32).clamp(0, res as i32 - 1) as usize);
                let behind = (layers[layer.min(2)][cy * res as usize + cx] - refz) * 16384.0;
                return Read { c, shadow: s * s * s, behind };
            }
            Read { c: 3, shadow: 1.0, behind: 0.0 }
        }

fn f16(h: u16) -> f32 {
    let (s, e, m) = ((h >> 15) as u32, ((h >> 10) & 0x1f) as i32, (h & 0x3ff) as f32);
    let v = if e == 0 { m * 2f32.powi(-24) } else if e == 31 { f32::INFINITY } else { (1.0 + m / 1024.0) * 2f32.powi(e - 15) };
    if s == 1 { -v } else { v }
}

fn read_layers(gpu: &render_h1::gpu::Gpu, t: &wgpu::Texture) -> Vec<Vec<f32>> {
    let (mw, mh) = (t.width(), t.height());
    let row = (mw * 2).div_ceil(256) * 256;
    let mut layers = Vec::new();
    for l in 0..3 {
        let buf = gpu.device.create_buffer(&wgpu::BufferDescriptor { label: None, size: u64::from(row * mh), usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ, mapped_at_creation: false });
        let mut enc = gpu.device.create_command_encoder(&Default::default());
        enc.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo { texture: t, mip_level: 0, origin: wgpu::Origin3d { x: 0, y: 0, z: l }, aspect: wgpu::TextureAspect::DepthOnly },
            wgpu::TexelCopyBufferInfo { buffer: &buf, layout: wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(row), rows_per_image: Some(mh) } },
            wgpu::Extent3d { width: mw, height: mh, depth_or_array_layers: 1 },
        );
        gpu.queue.submit([enc.finish()]);
        buf.slice(..).map_async(wgpu::MapMode::Read, |_| {});
        let _ = gpu.device.poll(wgpu::PollType::wait_indefinitely());
        let d = buf.slice(..).get_mapped_range();
        let v: Vec<f32> = (0..mh).flat_map(|y| (0..mw).map(move |x| (y, x))).map(|(y, x)| f32::from(u16::from_le_bytes([d[(y * row + 2 * x) as usize], d[(y * row + 2 * x + 1) as usize]])) / 65535.0).collect();
        layers.push(v);
    }
    layers
}

fn write_png(path: &str, layers: &[Vec<f32>]) {
    let n = (layers[0].len() as f32).sqrt() as u32;
    let mut px = Vec::new();
    for v in layers {
        let (lo, hi) = v.iter().fold((f32::MAX, f32::MIN), |(a, b), &x| (a.min(x), b.max(x)));
        px.extend(v.iter().map(|x| ((x - lo) / (hi - lo).max(1e-6) * 255.0) as u8));
    }
    let f = std::fs::File::create(path).expect("png");
    let mut e = png::Encoder::new(std::io::BufWriter::new(f), n, n * 3);
    e.set_color(png::ColorType::Grayscale);
    e.write_header().and_then(|mut wr| wr.write_image_data(&px)).expect("png write");
    println!("written {path}");
}
