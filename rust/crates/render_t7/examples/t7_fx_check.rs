//! `t7_fx_check <map.ff> [w h] [effect index]`: the map's placed effects
//! drawn offline on the GPU (`forward::fx_draw`) with as little else as the
//! draw needs, to check their pixels on a machine with little memory: one
//! small view, no G-buffer pipelines, no forward world draws, no coverage;
//! the lit scene cleared to a known grey and depth to far (0), the effects
//! drawn over it, read back and compared.
//!
//! The view looks at the placed effect with the most sprites (or the one
//! given) from 400 units. `T7_FORWARD_PNG=<file>` writes the frame;
//! `T7_IMAGE_MAX_DIM=256` keeps the pictures small.
fn main() {
    // (The effects drawn, the world's forward draws left out.)
    // SAFETY: set before any other thread starts.
    unsafe {
        if std::env::var_os("BO2MP_T7_NATIVE_FX").is_none() {
            std::env::set_var("BO2MP_T7_NATIVE_FX", "1");
        }
        std::env::set_var("CODLEGACY_T7_DEBUG_SURF", "no_forward");
    }
    let a: Vec<String> = std::env::args().collect();
    let zone = std::path::PathBuf::from(a.get(1).expect("map zone"));
    let w: u32 = a.get(2).and_then(|s| s.parse().ok()).unwrap_or(320);
    let h: u32 = a.get(3).and_then(|s| s.parse().ok()).unwrap_or(180);
    let pick_arg = a.get(4).cloned();
    let mut scene = render_t7::scene::Scene::load(&zone).expect("scene");
    for l in scene.fx.report.iter().chain(scene.report.iter().filter(|l| l.starts_with("t7 fx"))) {
        println!("{l}");
    }
    if std::env::var_os("T7_FX_CBUFFERS").is_some() {
        let mut seen = std::collections::HashSet::new();
        for m in scene.fx.defs.iter().flat_map(|d| &d.elems).flat_map(|e| e.materials.iter().flatten()) {
            let mat = &scene.materials[*m];
            let Some(t) = mat.forward.as_ref() else { continue };
            if !seen.insert(t.techset.clone()) {
                continue;
            }
            println!("cbuffers: {} ({}), buffer set {}, material's: {:?}", mat.name, t.techset, t.buffer_set, mat.cbuffers.iter().map(|(i, n, d)| (*i, n.clone(), d.len())).collect::<Vec<_>>());
            for (st, p) in [("vs", &t.vs), ("ps", &t.ps)] {
                for b in fastfile_t7::dxbc::reflect(p).map(|r| r.buffers).unwrap_or_default() {
                    println!("    {st} {} ({} B): {:?}", b.name, b.size, b.variables.iter().map(|v| format!("{}@{}{}", v.name, v.offset, if v.used { "" } else { "(unused)" })).collect::<Vec<_>>());
                }
            }
            for zn in [zone.file_stem().and_then(|s| s.to_str()).unwrap_or(""), "mp_common", "core_common", "core_patch", "mp_patch", "core_gfx", "core_post_gfx", "core_pre_gfx"] {
                if let Some(z) = fastfile_t7::shared::zone(zone.parent().unwrap(), zn) {
                    for nm in [mat.name.clone(), format!(",{}", mat.name)] {
                        if let Some(r) = z.capture.material(&nm) {
                            println!("    zone {zn}: {nm}: techset {:?}, cbuffers {:?}", r.techset, r.cbuffers.iter().map(|(i, n, d)| (*i, n.clone(), d.len())).collect::<Vec<_>>());
                        }
                    }
                }
            }
            // (`T7_FX_WGSL=<dir>`: the programs translated.)
            if let Ok(dir) = std::env::var("T7_FX_WGSL") {
                for (st, p) in [("vs", &t.vs), ("ps", &t.ps)] {
                    if let Ok(c) = dxbc_sm5::Container::parse(p)
                        && let Ok(prog) = dxbc_sm5::decode(c.shader)
                        && let Ok(tr) = dxbc_sm5::translate_program(&prog, &c, &dxbc_sm5::Options::for_stage(prog.stage))
                    {
                        let _ = std::fs::write(format!("{dir}/{}.{st}.wgsl", t.techset.replace(['/', '#'], "_")), tr.wgsl);
                    }
                }
            }
            let args: Vec<_> = t.args.iter().map(|a| format!("{:#x}/b{}@{}:{:x?}", a.ty, a.buffer, a.location, a.u)).collect();
            println!("    args: {args:?}");
        }
    }
    let now = 20_000;
    // The placed effect with the most sprites (eye far away: none culled by its spawn range).
    let mut best = (0usize, 0usize);
    let mut out = Vec::new();
    let fx = &scene.fx;
    // The model element particles alive (CPU only: this check draws no G-buffer).
    if !fx.model_names.is_empty() {
        for (di, d) in fx.defs.iter().enumerate() {
            for (ei, e) in d.elems.iter().enumerate().filter(|(_, e)| e.ty == 7) {
                let placed = fx.placed.iter().filter(|p| p.def == di).count();
                let parents = fx.defs.iter().filter(|o| o.elems.iter().any(|x| x.runs.contains(&Some(di)) || x.death == Some(di))).map(|o| o.name.as_str()).collect::<Vec<_>>();
                println!("t7 fx check: model element: {} elem {ei} ({} of {} looping+one-shot) models {:?} spawn {:?} life {:?} delay {:?}; placed {placed}, played by {parents:?}", d.name, if ei < d.looping { "looping" } else { "one-shot" }, d.looping + d.one_shot, e.model_names, e.spawn, e.life, e.delay);
            }
        }
        let mut m = Vec::new();
        for t in [now, now + 5_000] {
            fx.models(t, [0.0; 3], &mut m);
            let near: Vec<String> = m.iter().take(3).map(|i| format!("{} at ({:.0}, {:.0}, {:.0})", fx.model_names[i.model], i.bone[3], i.bone[7], i.bone[11])).collect();
            println!("t7 fx check: model particles alive at {t} ms: {} ({}); pools {:?}", m.len(), near.join(", "), scene.fx_models);
        }
    }
    let one =|i: usize, out: &mut Vec<render_t7::fx::Sprite>| {
        let s = render_t7::fx::FxScene { defs: fx.defs.clone(), placed: vec![fx.placed[i]], report: vec![], model_names: vec![], cull_far: vec![] };
        let p = fx.placed[i].origin;
        s.sprites(now, [p[0] + 300.0, p[1], p[2] + 40.0], out);
    };
    if std::env::var_os("T7_FX_LIST").is_some() {
        let mut names: std::collections::BTreeMap<&str, usize> = Default::default();
        for p in &fx.placed {
            *names.entry(fx.defs[p.def].name.as_str()).or_default() += 1;
        }
        println!("placed effects by name: {names:?}");
        // The keys the map's effect entities carry, and the exploder ones in full.
        let mut keys: std::collections::BTreeMap<String, usize> = Default::default();
        let mut shown = 0;
        for b in scene.map.entities.split('{').filter(|b| b.contains("\"classname\" \"fx\"")) {
            for l in b.lines() {
                if let Some(k) = l.trim().strip_prefix('"').and_then(|r| r.split('"').next()) {
                    *keys.entry(k.to_owned()).or_default() += 1;
                }
            }
            if b.contains("exploder") && shown < 3 {
                shown += 1;
                println!("exploder effect entity: {}", b.replace('\n', " "));
            }
        }
        println!("effect entity keys: {keys:?}");
    }
    // The views: each pick (an index or a part of an effect name; several
    // comma-separated), else the effect of small sprites with the most.
    let mut picks: Vec<usize> = pick_arg.as_deref().unwrap_or("").split(',').filter(|s| !s.is_empty()).filter_map(|s| s.parse::<usize>().ok().or_else(|| fx.placed.iter().position(|p| fx.defs[p.def].name.contains(s)))).collect();
    if picks.is_empty() {
        for i in 0..fx.placed.len() {
            one(i, &mut out);
            // (Effects of small sprites: the ones a 300-unit view frames.)
            let small = out.iter().filter(|s| f32::from_le_bytes(s.record[0x30..0x34].try_into().unwrap()).max(f32::from_le_bytes(s.record[0x34..0x38].try_into().unwrap())) < 48.0).count();
            if small == out.len() && small > best.1 {
                best = (i, small);
            }
        }
        picks.push(best.0);
    }
    let placed: Vec<_> = picks.iter().map(|&i| fx.placed[i]).collect();
    for &i in &picks {
        one(i, &mut out);
        let p = fx.placed[i];
        println!("view: placed effect {i} ({}) at {:?}, {} sprites alone", fx.defs[p.def].name, p.origin, out.len());
        let d = &fx.defs[p.def];
        println!("  def: {} looping, {} one-shot, looping life {} ms, duration {} ms", d.looping, d.one_shot, d.msec_looping_life, d.duration);
        for e in &d.elems {
            println!("  elem type {} spawn {:?} life {:?} delay {:?} materials {:?} runs {:?} {:?} death {:?} {:?}", e.ty, e.spawn, e.life, e.delay, e.materials, e.run_names, e.runs, e.death_name, e.death);
        }
        for s in out.iter().rev().take(4) {
            let r = &s.record;
            let f = |o: usize| f32::from_le_bytes(r[o..o + 4].try_into().unwrap());
            let u16_at = |o: usize| u16::from_le_bytes([r[o], r[o + 1]]);
            println!("sprite: {} dist {:.0} colour {:?} half size {:.1} x {:.1} emissive f16 {:#06x} flags {} alpha mode {}", g_name(&scene, s.material), s.dist, &r[0x10..0x14], f(0x30), f(0x34), u16_at(0x1c), u16_at(0x3c), u16_at(0x38));
        }
    }
    // (Only the picked effects, unless `T7_FX_ALL`.)
    if std::env::var_os("T7_FX_ALL").is_none() {
        scene.fx.placed = placed.clone();
    }
    let gpu = render_h1::gpu::Gpu::headless().expect("gpu");
    let targets = render_t7::gbuffer::Targets::new(&gpu, w, h);
    let mut g = render_t7::gbuffer::GBuffer::new(gpu.clone(), std::sync::Arc::new(scene));
    let mut lit = render_t7::lighting::Lighting::new(&gpu, &g.scene, zone.parent().expect("zone dir")).expect("lighting");
    lit.prepare(&targets).expect("lighting bind");
    let mut fwd = render_t7::forward::Forward::new(gpu.clone(), g.scene.clone());
    fwd.prepare(&mut g, &targets, &lit);
    let grey = 0.18f64;
    for (vi, p) in placed.iter().enumerate() {
        let target = glam::Vec3::from(p.origin);
        // (`T7_FX_FROM=y`: from +y instead of +x; `z`: from above.)
        let eye = target
            + match std::env::var("T7_FX_FROM").as_deref() {
                Ok("y") => glam::Vec3::new(0.0, 300.0, 40.0),
                Ok("z") => glam::Vec3::new(1.0, 0.0, 300.0),
                _ => glam::Vec3::new(300.0, 0.0, 40.0),
            };
        let d = target - eye;
        let yaw = d.y.atan2(d.x).to_degrees();
        let pitch = -(d.z / d.length()).asin().to_degrees();
        let view = render_h1::code::View { eye, angles: glam::Vec3::new(pitch, yaw, 0.0), fov_x: 80.0, width: w, height: h, near: 4.0 };
        let cam = render_h1::code::Camera::from(&view);
        g.update(&render_t7::code::CodeConsts::view(&cam));
        lit.update(&cam, now, vi == 0);
        fwd.update(&lit.view_consts(&cam, now));
        fwd.update_fx(now, eye.to_array());
        for l in fwd.take_fx_lines() {
            println!("{l}");
        }
        // Where the nearest sprites' centres land (CPU), to compare with the pixels.
        let mut near = Vec::new();
        g.scene.fx.sprites(now, eye.to_array(), &mut near);
        for sp in near.iter().rev().take(2) {
            let c = glam::Vec3::from_slice(&sp.record[0..12].chunks(4).map(|b| f32::from_le_bytes(b.try_into().unwrap())).collect::<Vec<_>>());
            let clip = cam.view_projection * (c - cam.eye).extend(1.0);
            let ndc = clip.truncate() / clip.w;
            let f = |o: usize| f32::from_le_bytes(sp.record[o..o + 4].try_into().unwrap());
            let radius = f(0x30).hypot(f(0x34)) / clip.w / cam.tan_x * w as f32 * 0.5;
            println!("sprite centre {c} -> pixel ({:.0}, {:.0}) depth {:.5}; half size {:.1} x {:.1} -> within {radius:.0} px; colour {:?}", (ndc.x * 0.5 + 0.5) * w as f32, (0.5 - ndc.y * 0.5) * h as f32, ndc.z, f(0x30), f(0x34), &sp.record[0x10..0x14]);
        }
        let scene_view = lit.scene_target().expect("scene target");
        let scene_tex = lit.scene_texture().expect("scene texture");
        let scope = gpu.device.push_error_scope(wgpu::ErrorFilter::Validation);
        let mut enc = gpu.device.create_command_encoder(&Default::default());
        {
            let _ = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("clear"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment { view: scene_view, depth_slice: None, resolve_target: None, ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color { r: grey, g: grey, b: grey, a: 1.0 }), store: wgpu::StoreOp::Store } })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment { view: &targets.depth.view, depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(0.0), store: wgpu::StoreOp::Store }), stencil_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(0), store: wgpu::StoreOp::Store }) }),
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
        }
        fwd.record(&mut enc, &g, &targets, scene_view, scene_tex, &lit.counts);
        gpu.queue.submit([enc.finish()]);
        let err = pollster::block_on(scope.pop());
        let _ = gpu.device.poll(wgpu::PollType::wait_indefinitely());
        match err {
            Some(e) => println!("frame: effects recorded with errors: {e}"),
            None => println!("frame: effects recorded and submitted without validation errors"),
        }
        // The sprite light map's first lit layers (`BO2MP_T7_FX_LIGHT=1`).
        if let Some((t, lit)) = fwd.fx_lmap()
            && lit > 0
        {
            let n = lit.min(4);
            let buf = gpu.device.create_buffer(&wgpu::BufferDescriptor { label: None, size: u64::from(256 * 8 * n), usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ, mapped_at_creation: false });
            let mut enc = gpu.device.create_command_encoder(&Default::default());
            enc.copy_texture_to_buffer(t.as_image_copy(), wgpu::TexelCopyBufferInfo { buffer: &buf, layout: wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(256), rows_per_image: Some(8) } }, wgpu::Extent3d { width: 8, height: 8, depth_or_array_layers: n });
            gpu.queue.submit([enc.finish()]);
            buf.slice(..).map_async(wgpu::MapMode::Read, |_| {});
            let _ = gpu.device.poll(wgpu::PollType::wait_indefinitely());
            let d = buf.slice(..).get_mapped_range();
            let h = |b: &[u8]| half_f32(u16::from_le_bytes([b[0], b[1]]));
            for layer in 0..n as usize {
                let mut sum = [0f32; 4];
                let (mut lo, mut hi) = (f32::MAX, f32::MIN);
                for y in 0..8 {
                    for x in 0..8 {
                        let o = layer * 256 * 8 + y * 256 + x * 8;
                        for k in 0..4 {
                            let v = h(&d[o + 2 * k..]);
                            sum[k] += v / 64.0;
                            if k == 0 {
                                lo = lo.min(v);
                                hi = hi.max(v);
                            }
                        }
                    }
                }
                println!("t7 fx check: lmapTex layer {layer} of {lit} lit: mean {:.3} {:.3} {:.3} {:.3}, red {lo:.3}..{hi:.3}", sum[0], sum[1], sum[2], sum[3]);
            }
        }
        let png = std::env::var("T7_FORWARD_PNG").ok().map(|p| if placed.len() > 1 { p.replace(".png", &format!("_{vi}.png")) } else { p });
        read_back(&gpu, scene_tex, w, h, grey, png.as_deref());
    }
}

/// The scene target's pixels against the grey it was cleared to.
fn read_back(gpu: &render_h1::gpu::Gpu, scene_tex: &wgpu::Texture, w: u32, h: u32, grey: f64, png: Option<&str>) {
    let row = (w * 8).div_ceil(256) * 256;
    let buf = gpu.device.create_buffer(&wgpu::BufferDescriptor { label: None, size: u64::from(row * h), usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ, mapped_at_creation: false });
    let mut enc = gpu.device.create_command_encoder(&Default::default());
    enc.copy_texture_to_buffer(scene_tex.as_image_copy(), wgpu::TexelCopyBufferInfo { buffer: &buf, layout: wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(row), rows_per_image: Some(h) } }, wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 });
    gpu.queue.submit([enc.finish()]);
    buf.slice(..).map_async(wgpu::MapMode::Read, |_| {});
    let _ = gpu.device.poll(wgpu::PollType::wait_indefinitely());
    let data = buf.slice(..).get_mapped_range();
    let half = |b: u16| -> f32 {
        let s = if b & 0x8000 != 0 { -1.0 } else { 1.0 };
        let e = i32::from((b >> 10) & 0x1f);
        let m = f32::from(b & 0x3ff);
        s * if e == 0 { m * 2f32.powi(-24) } else if e == 31 { f32::INFINITY } else { (1.0 + m / 1024.0) * 2f32.powi(e - 15) }
    };
    let (mut changed, mut brighter, mut darker, mut bad, mut max) = (0usize, 0usize, 0usize, 0usize, 0f32);
    let mut sum = [0f64; 3];
    let mut bbox = [u32::MAX, u32::MAX, 0, 0];
    let mut px = Vec::with_capacity((w * h * 3) as usize);
    for y in 0..h {
        for x in 0..w {
            let o = (y * row + x * 8) as usize;
            let c: [f32; 3] = core::array::from_fn(|k| half(u16::from_le_bytes([data[o + 2 * k], data[o + 2 * k + 1]])));
            if c.iter().any(|v| !v.is_finite()) {
                bad += 1;
            }
            let dl: f32 = c.iter().map(|v| v - grey as f32).sum::<f32>() / 3.0;
            if c.iter().any(|v| (v - grey as f32).abs() > 1e-3) {
                changed += 1;
                bbox = [bbox[0].min(x), bbox[1].min(y), bbox[2].max(x), bbox[3].max(y)];
                if dl > 0.0 {
                    brighter += 1
                } else {
                    darker += 1
                }
                for k in 0..3 {
                    sum[k] += f64::from(c[k]);
                }
            }
            for v in c {
                max = max.max(if v.is_finite() { v } else { 0.0 });
                px.push((v.clamp(0.0, 1.0).powf(1.0 / 2.2) * 255.0) as u8);
            }
        }
    }
    let n = changed.max(1) as f64;
    println!(
        "t7 fx check: {changed} of {} pixels changed by the effects ({brighter} brighter, {darker} darker than the grey {grey}), mean of the changed {:.3} {:.3} {:.3}, max {max:.3}, {bad} non-finite; within x {}..{}, y {}..{}",
        w * h,
        sum[0] / n,
        sum[1] / n,
        sum[2] / n,
        bbox[0],
        bbox[2],
        bbox[1],
        bbox[3]
    );
    if let Some(path) = png {
        let f = std::fs::File::create(path).expect("png");
        let mut e = png::Encoder::new(std::io::BufWriter::new(f), w, h);
        e.set_color(png::ColorType::Rgb);
        e.write_header().and_then(|mut wr| wr.write_image_data(&px)).expect("png write");
        println!("frame written to {path}");
    }
}

fn g_name(s: &render_t7::scene::Scene, m: usize) -> String {
    format!("{} ({})", s.materials[m].name, s.materials[m].forward.as_ref().map_or("", |t| t.techset.as_str()))
}

fn half_f32(b: u16) -> f32 {
    let s = if b & 0x8000 != 0 { -1.0 } else { 1.0 };
    let e = i32::from((b >> 10) & 0x1f);
    let m = f32::from(b & 0x3ff);
    s * if e == 0 { m * 2f32.powi(-24) } else if e == 31 { f32::INFINITY } else { (1.0 + m / 1024.0) * 2f32.powi(e - 15) }
}
