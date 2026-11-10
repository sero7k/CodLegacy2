//! `t7_lighting_check <map.ff> [width height]`: the G-buffer and then the
//! deferred lighting stage (`render_t7::lighting`) for one frame from the
//! first deathmatch spawn, on whatever adapter wgpu finds; prints what the
//! stage made and what it lacks. `T7_LIGHTING_PNG=<file>`: the lit scene
//! target clamped to 0..1 as a picture (a check of the stage's output, not
//! the game's image: exposure and the post chain come after).
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
    g.prepare(&targets);
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
    println!("view: eye {eye} yaw {yaw}");
    // `T7_EYE="x y z pitch yaw"`: another view.
    let (eye, pitch, yaw) = match std::env::var("T7_EYE").ok().map(|v| v.split_whitespace().filter_map(|x| x.parse::<f32>().ok()).collect::<Vec<_>>()) {
        Some(v) if v.len() == 5 => (glam::Vec3::new(v[0], v[1], v[2]), v[3], v[4]),
        _ => (eye, 5.0, yaw),
    };
    let view = render_h1::code::View { eye, angles: glam::Vec3::new(pitch, yaw, 0.0), fov_x: 80.0, width: w, height: h, near: 4.0 };
    let cam = render_h1::code::Camera::from(&view);
    g.update(&render_t7::code::CodeConsts::view(&cam));
    let mut lit = render_t7::lighting::Lighting::new(&gpu, &g.scene, zone.parent().expect("zone dir")).expect("lighting");
    lit.prepare(&targets).expect("lighting bind");
    lit.update(&cam, 0, true);
    lit.update(&cam, 16, false);
    for l in &lit.report {
        println!("{l}");
    }
    let scope = gpu.device.push_error_scope(wgpu::ErrorFilter::Validation);
    let mut enc = gpu.device.create_command_encoder(&Default::default());
    g.record(&mut enc, &targets);
    lit.record(&mut enc);
    gpu.queue.submit([enc.finish()]);
    let err = pollster::block_on(scope.pop());
    let _ = gpu.device.poll(wgpu::PollType::wait_indefinitely());
    match err {
        Some(e) => println!("frame: recorded with errors: {e}"),
        None => println!("frame: G-buffer and lighting recorded and submitted without validation errors"),
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
    // The sun shadow fit, and `T7_SUN_SHADOW_PNG=<file>`: the cascade maps
    // (one above the other, depth as grey).
    if let Some(ss) = lit.sun_shadows.as_ref() {
        if let Some(st) = &ss.state {
            for c in 0..3 {
                println!("sun shadow cascade {c}: level {} texel {:.3} distance {:.1} corner {:?} uv {:?}", st.level[c], st.texel[c], st.distance[c], st.corner[c], st.uv[c]);
            }
            println!("sun shadow: z base {} eye {:?} axis {:?} cone {} level offsets {:?}", st.z_base, st.eye, st.axis, st.cone, st.level_offset);
        }
        if let Ok(path) = std::env::var("T7_SUN_SHADOW_PNG") {
            let t = ss.maps();
            let (mw, mh) = (t.width(), t.height());
            let row = (mw * 2).div_ceil(256) * 256;
            let mut px = Vec::new();
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
                let v: Vec<u16> = (0..mh).flat_map(|y| (0..mw).map(move |x| (y, x))).map(|(y, x)| u16::from_le_bytes([d[(y * row + 2 * x) as usize], d[(y * row + 2 * x + 1) as usize]])).collect();
                let (lo, hi) = (*v.iter().min().unwrap(), *v.iter().max().unwrap());
                println!("sun shadow layer {l}: depth {:.5}..{:.5}", f32::from(lo) / 65535.0, f32::from(hi) / 65535.0);
                let span = f32::from(hi.saturating_sub(lo)).max(1.0);
                px.extend(v.iter().map(|&x| (f32::from(x - lo) / span * 255.0) as u8));
            }
            let f = std::fs::File::create(&path).expect("png");
            let mut e = png::Encoder::new(std::io::BufWriter::new(f), mw, mh * 3);
            e.set_color(png::ColorType::Grayscale);
            e.write_header().and_then(|mut wr| wr.write_image_data(&px)).expect("png write");
            println!("sun shadow maps written to {path}");
        }
    }
    // `T7_AO_PNG=<file>`: the denoised AO (and `<file>.raw.png` GTAO's own).
    if let (Ok(path), Some((ao, raw))) = (std::env::var("T7_AO_PNG"), lit.ao_textures()) {
        for (t, p) in [(ao, path.clone()), (raw, format!("{path}.raw.png"))] {
            let (aw, ah) = (t.width(), t.height());
            let row = aw.div_ceil(256) * 256;
            let buf = gpu.device.create_buffer(&wgpu::BufferDescriptor { label: None, size: u64::from(row * ah), usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ, mapped_at_creation: false });
            let mut enc = gpu.device.create_command_encoder(&Default::default());
            enc.copy_texture_to_buffer(t.as_image_copy(), wgpu::TexelCopyBufferInfo { buffer: &buf, layout: wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(row), rows_per_image: Some(ah) } }, t.size());
            gpu.queue.submit([enc.finish()]);
            buf.slice(..).map_async(wgpu::MapMode::Read, |_| {});
            let _ = gpu.device.poll(wgpu::PollType::wait_indefinitely());
            let d = buf.slice(..).get_mapped_range();
            let px: Vec<u8> = (0..ah).flat_map(|y| d[(y * row) as usize..(y * row + aw) as usize].to_vec()).collect();
            let mean = px.iter().map(|&v| f64::from(v)).sum::<f64>() / px.len() as f64 / 255.0;
            println!("ao {p}: {aw}x{ah}, mean {mean:.4}, min {}", px.iter().min().unwrap());
            let f = std::fs::File::create(&p).expect("png");
            let mut e = png::Encoder::new(std::io::BufWriter::new(f), aw, ah);
            e.set_color(png::ColorType::Grayscale);
            e.write_header().and_then(|mut wr| wr.write_image_data(&px)).expect("png write");
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
    let half = |b: u16| -> f32 {
        let s = if b & 0x8000 != 0 { -1.0 } else { 1.0 };
        let e = i32::from((b >> 10) & 0x1f);
        let m = f32::from(b & 0x3ff);
        s * if e == 0 { m * 2f32.powi(-24) } else if e == 31 { f32::INFINITY } else { (1.0 + m / 1024.0) * 2f32.powi(e - 15) }
    };
    let mut px = Vec::with_capacity((w * h * 3) as usize);
    let mut raw: Vec<f32> = Vec::new();
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
                raw.push(v);
                sum[k] += f64::from(v);
                max = max.max(v);
                px.push((v.clamp(0.0, 1.0).powf(1.0 / 2.2) * 255.0) as u8);
            }
            if (0..3).any(|k| px[px.len() - 3 + k] > 0) {
                lit_px += 1;
            }
        }
    }
    let n = f64::from(w * h);
    println!("scene target: mean {:.6} {:.6} {:.6}, max {max:.3}, {lit_px} of {} pixels non-black, {bad} non-finite values", sum[0] / n, sum[1] / n, sum[2] / n, w * h);
    if let Ok(path) = std::env::var("T7_LIGHTING_RAW") {
        std::fs::write(&path, bytemuck::cast_slice(&raw)).expect("raw");
    }
    if let Ok(path) = std::env::var("T7_LIGHTING_PNG") {
        let f = std::fs::File::create(&path).expect("png");
        let mut e = png::Encoder::new(std::io::BufWriter::new(f), w, h);
        e.set_color(png::ColorType::Rgb);
        e.write_header().and_then(|mut wr| wr.write_image_data(&px)).expect("png write");
        println!("scene target written to {path}");
    }
}
