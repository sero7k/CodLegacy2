//! `t7_forward_check <map.ff> [w h]`: the G-buffer, the lighting and the
//! forward lists offline (pipelines, bindings, one frame), what is drawn
//! and what is left out.
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let zone = std::path::PathBuf::from(a.get(1).expect("map zone"));
    let w: u32 = a.get(2).and_then(|s| s.parse().ok()).unwrap_or(640);
    let h: u32 = a.get(3).and_then(|s| s.parse().ok()).unwrap_or(360);
    let t0 = std::time::Instant::now();
    let scene = render_t7::scene::Scene::load(&zone).expect("scene");
    for l in &scene.map.report {
        println!("map: {l}");
    }
    for l in &scene.report {
        println!("scene: {l}");
    }
    println!("scene: {} materials, {} draws, read in {:.1}s", scene.materials.len(), scene.draws.len(), t0.elapsed().as_secs_f32());
    let gpu = render_h1::gpu::Gpu::headless().expect("gpu");
    println!("gpu: {:?} downlevel {:?}", gpu.device.features(), gpu.device.limits().max_texture_dimension_2d);
    let targets = render_t7::gbuffer::Targets::new(&gpu, w, h);
    let mut g = render_t7::gbuffer::GBuffer::new(gpu.clone(), std::sync::Arc::new(scene));
    // Missing images (the big packages are not here) bound as 1x1 for the
    // check only; they are listed below.
    g.check_bindings_without_images = std::env::var("T7_CHECK_STRICT").is_err();
    // (`T7_SKIP_GBUFFER`: the G-buffer's pipelines not made, to check the
    // forward lists on a machine with little memory.)
    let skip_g = std::env::var("T7_SKIP_GBUFFER").is_ok();
    if !skip_g {
        g.prepare(&targets);
    }
    // A view from the first deathmatch spawn (origin and yaw from the
    // entity string), 60 units up; else the map's middle.
    let s = &g.scene;
    let (mins, maxs) = (s.map.mins, s.map.maxs);
    let mut eye = glam::Vec3::new((mins[0] + maxs[0]) * 0.5, (mins[1] + maxs[1]) * 0.5, (mins[2] + maxs[2]) * 0.5 + 200.0);
    let mut yaw = 0.0f32;
    for block in s.map.entities.split('}') {
        if !block.contains("mp_dm_spawn") && !block.contains("mp_tdm_spawn") {
            continue;
        }
        let val = |k: &str| block.split('\n').find_map(|l| l.trim().strip_prefix(&format!("\"{k}\" \"")).map(|v| v.trim_end_matches('"').to_owned()));
        if let Some(o) = val("origin") {
            let v: Vec<f32> = o.split_whitespace().filter_map(|x| x.parse().ok()).collect();
            if v.len() == 3 {
                eye = glam::Vec3::new(v[0], v[1], v[2] + 60.0);
                yaw = val("angles").and_then(|a| a.split_whitespace().nth(1).and_then(|y| y.parse().ok())).unwrap_or(0.0);
                break;
            }
        }
    }
    // `T7_EYE="x y z pitch yaw"`: another view.
    let mut pitch = 5.0f32;
    if let Some(v) = std::env::var("T7_EYE").ok().map(|v| v.split_whitespace().filter_map(|x| x.parse::<f32>().ok()).collect::<Vec<_>>())
        && v.len() >= 5
    {
        (eye, pitch, yaw) = (glam::Vec3::new(v[0], v[1], v[2]), v[3], v[4]);
    }
    println!("view: eye {eye} pitch {pitch} yaw {yaw}");
    let view = render_h1::code::View { eye, angles: glam::Vec3::new(pitch, yaw, 0.0), fov_x: 80.0, width: w, height: h, near: 4.0 };
    let cam = render_h1::code::Camera::from(&view);
    g.update(&render_t7::code::CodeConsts::view(&cam));
    let mut lit = render_t7::lighting::Lighting::new(&gpu, &g.scene, zone.parent().expect("zone dir")).expect("lighting");
    lit.prepare(&targets).expect("lighting bind");
    lit.update(&cam, 0, true);
    for l in &lit.report {
        println!("{l}");
    }
    println!("step: lighting bound"); use std::io::Write; let _ = std::io::stdout().flush();
    let mut fwd = render_t7::forward::Forward::new(gpu.clone(), g.scene.clone());
    fwd.prepare(&mut g, &targets, &lit);
    println!("step: forward prepared"); let _ = std::io::stdout().flush();
    let vc = lit.view_consts(&cam, 0);
    fwd.update(&vc);
    // The water techniques' code constants (PS, type 0x400): buffer slot,
    // register, size, code constant and whether the frame has it.
    {
        let mut seen = std::collections::HashSet::new();
        for m in g.scene.materials.iter().filter(|m| m.techset.contains("water")) {
            let Some(t) = m.forward.as_ref() else { continue };
            if !seen.insert(m.techset.clone()) {
                continue;
            }
            for a in t.args.iter().filter(|a| a.ty & 0xff00 == 0x400) {
                let idx = (a.u[0] & 0xff) as u16;
                let name = render_t7::code::NAMES.iter().find(|(i, _)| *i == idx).map_or("?", |(_, n)| *n);
                println!("water consts {}: stage {} b{} reg {} size {} code {idx:#x} {name}: {:?}", m.techset, a.ty & 0xff, a.buffer, a.location / 16, a.size, vc.get(idx).map(|r| r.first().copied()));
            }
        }
    }
    let r = &fwd.report;
    println!("forward: {} pipelines made, {} failed; {} draws ready of {}", r.pipelines_made, r.pipelines_failed.len(), r.draws_ready, r.draws_total);
    for f in &r.pipelines_failed {
        println!("forward: pipeline failed: {f}");
    }
    for f in &r.draws_skipped {
        println!("forward: skipped: {f}");
    }
    println!("{}", fwd.water_line());
    // The water surfaces (technique sets with `water`): list, whether
    // drawn, why not, bounds, the pixels their triangles cover in view.
    {
        let s = &g.scene;
        let ready: std::collections::HashSet<usize> = fwd.ready_draws().into_iter().collect();
        let to_clip = cam.view_projection * glam::Mat4::from_translation(-cam.eye);
        for (di, d) in s.draws.iter().enumerate() {
            let m = &s.materials[d.material];
            if !m.techset.contains("water") {
                continue;
            }
            let why = r.skipped_draws.iter().find(|(i, _)| *i == di).map_or(String::new(), |(_, e)| e.clone());
            let (cw, ccw) = render_t7::coverage::expected_pixels(s, d, to_clip, w, h);
            let src = if d.group == 1 { s.model_names.get(d.source as usize).cloned().unwrap_or_default() } else { format!("world surface {}", d.source) };
            println!("water draw {di}: {} ({}) {src} x{} list {} {} {why} bounds {:?}..{:?} px {:.0}", m.name, m.techset, d.instances, d.list, if ready.contains(&di) { "drawn" } else { "NOT drawn" }, d.mins, d.maxs, cw.max(ccw));
        }
    }
    let scope = gpu.device.push_error_scope(wgpu::ErrorFilter::Validation);
    let mut enc = gpu.device.create_command_encoder(&Default::default());
    if !skip_g {
        g.record(&mut enc, &targets);
    }
    lit.record(&mut enc);
    if let Some(sv) = lit.scene_target() {
        fwd.record(&mut enc, &g, &targets, sv, lit.scene_texture().expect("scene"), &lit.counts);
        eprintln!("light culling: {} lights, {} probes packed", lit.counts.lights, lit.counts.ref_probes);
    }
    gpu.queue.submit([enc.finish()]);
    let err = pollster::block_on(scope.pop());
    let _ = gpu.device.poll(wgpu::PollType::wait_indefinitely());
    match err {
        Some(e) => println!("frame: recorded with errors: {e}"),
        None => println!("frame: G-buffer, lighting and forward recorded and submitted without validation errors"),
    }
    // The classification's counts and the first tiles' depth extents.
    let (acc, ind, ext) = lit.permute_buffers();
    let read = |b: &wgpu::Buffer, n: u64| -> Vec<u32> {
        let rb = gpu.device.create_buffer(&wgpu::BufferDescriptor { label: None, size: n, usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ, mapped_at_creation: false });
        let mut enc = gpu.device.create_command_encoder(&Default::default());
        enc.copy_buffer_to_buffer(b, 0, &rb, 0, n);
        gpu.queue.submit([enc.finish()]);
        rb.slice(..).map_async(wgpu::MapMode::Read, |_| {});
        let _ = gpu.device.poll(wgpu::PollType::wait_indefinitely());
        let v = bytemuck::cast_slice(&rb.slice(..).get_mapped_range()).to_vec();
        v
    };
    let counts = read(acc, 256);
    println!("tiles per permutation: {:?}", counts.iter().enumerate().filter(|(_, c)| **c > 0).collect::<Vec<_>>());
    println!("indirect args (first 4): {:?}", &read(ind, 768)[..12]);
    if let Some(e) = ext {
        let v = read(e, 64);
        println!("extents (first 8 tiles): {:?}", v.chunks(2).map(|c| (f32::from_bits(c[0]), f32::from_bits(c[1]))).collect::<Vec<_>>());
    }
    // The light culling's masks: words with bits set, per mask.
    if let Some(masks) = fwd.cull_masks() {
        for (name, t) in ["visibleLights", "visibleProbes"].iter().zip(masks) {
            let (tw, th, layers) = (t.width(), t.height(), t.depth_or_array_layers());
            let row = (tw * 4).div_ceil(256) * 256;
            let b = gpu.device.create_buffer(&wgpu::BufferDescriptor { label: None, size: u64::from(row * th * layers), usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ, mapped_at_creation: false });
            let mut enc = gpu.device.create_command_encoder(&Default::default());
            enc.copy_texture_to_buffer(t.as_image_copy(), wgpu::TexelCopyBufferInfo { buffer: &b, layout: wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(row), rows_per_image: Some(th) } }, t.size());
            gpu.queue.submit([enc.finish()]);
            b.slice(..).map_async(wgpu::MapMode::Read, |_| {});
            let _ = gpu.device.poll(wgpu::PollType::wait_indefinitely());
            let v: Vec<u32> = bytemuck::cast_slice(&b.slice(..).get_mapped_range()).to_vec();
            let bits: u32 = v.iter().map(|w| w.count_ones()).sum();
            println!("{name}: {tw}x{th}x{layers}, {} words set, {bits} bits", v.iter().filter(|w| **w != 0).count());
        }
    }
    // Read back the scene target.
    let tex = lit.scene_texture().expect("scene");
    let row = (w * 8).div_ceil(256) * 256;
    let buf = gpu.device.create_buffer(&wgpu::BufferDescriptor { label: None, size: u64::from(row * h), usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ, mapped_at_creation: false });
    let mut enc = gpu.device.create_command_encoder(&Default::default());
    enc.copy_texture_to_buffer(tex.as_image_copy(), wgpu::TexelCopyBufferInfo { buffer: &buf, layout: wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(row), rows_per_image: Some(h) } }, wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 });
    gpu.queue.submit([enc.finish()]);
    buf.slice(..).map_async(wgpu::MapMode::Read, |_| {});
    let _ = gpu.device.poll(wgpu::PollType::wait_indefinitely());
    let data = buf.slice(..).get_mapped_range();
    // `T7_RAW=<file>`: the scene target as stored (RGBA16F rows, tightly
    // packed), to compare frames without a tone map.
    if let Ok(path) = std::env::var("T7_RAW") {
        let mut raw = Vec::with_capacity((w * h * 8) as usize);
        for y in 0..h {
            let o = (y * row) as usize;
            raw.extend_from_slice(&data[o..o + (w * 8) as usize]);
        }
        std::fs::write(&path, raw).expect("raw");
    }
    let half = |b: u16| -> f32 {
        let s = if b & 0x8000 != 0 { -1.0 } else { 1.0 };
        let e = i32::from((b >> 10) & 0x1f);
        let m = f32::from(b & 0x3ff);
        s * if e == 0 { m * 2f32.powi(-24) } else if e == 31 { f32::INFINITY } else { (1.0 + m / 1024.0) * 2f32.powi(e - 15) }
    };
    // `T7_PNG_SCALE`: the linear scene scaled before it is written (no
    // exposure or tone map here).
    let png_scale: f32 = std::env::var("T7_PNG_SCALE").ok().and_then(|v| v.parse().ok()).unwrap_or(1.0);
    let mut px = Vec::with_capacity((w * h * 3) as usize);
    let (mut sum, mut max, mut lit_px, mut bad) = ([0f64; 3], 0f32, 0usize, 0usize);
    for y in 0..h {
        for x in 0..w {
            let o = (y * row + x * 8) as usize;
            for k in 0..3 {
                let v = half(u16::from_le_bytes([data[o + 2 * k], data[o + 2 * k + 1]]));
                if !v.is_finite() {
                    bad += 1;
                }
                let v = if v.is_finite() { v } else { 0.0 };
                sum[k] += f64::from(v);
                max = max.max(v);
                px.push(((v * png_scale).clamp(0.0, 1.0).powf(1.0 / 2.2) * 255.0) as u8);
            }
            if (0..3).any(|k| px[px.len() - 3 + k] > 0) {
                lit_px += 1;
            }
        }
    }
    // The top quarter of the frame (mostly sky from a spawn view).
    let (mut top, mut top_lit) = ([0f64; 3], 0usize);
    for y in 0..h / 4 {
        for x in 0..w {
            let i = ((y * w + x) * 3) as usize;
            for k in 0..3 {
                top[k] += f64::from(px[i + k]);
            }
            if px[i..i + 3].iter().any(|&c| c > 0) {
                top_lit += 1;
            }
        }
    }
    let tn = f64::from(w * (h / 4)).max(1.0);
    println!("scene target top quarter: mean 8-bit {:.1} {:.1} {:.1}, {top_lit} of {} pixels non-black", top[0] / tn, top[1] / tn, top[2] / tn, w * (h / 4));
    let n = f64::from(w * h);
    println!("scene target: mean {:.4} {:.4} {:.4}, max {max:.3}, {lit_px} of {} pixels non-black, {bad} non-finite values", sum[0] / n, sum[1] / n, sum[2] / n, w * h);
    if let Ok(path) = std::env::var("T7_FORWARD_PNG") {
        let f = std::fs::File::create(&path).expect("png");
        let mut e = png::Encoder::new(std::io::BufWriter::new(f), w, h);
        e.set_color(png::ColorType::Rgb);
        e.write_header().and_then(|mut wr| wr.write_image_data(&px)).expect("png write");
        println!("scene target written to {path}");
    }
}
