//! `t7_post_check <map.ff> [width height] [frames]`: the G-buffer, the
//! deferred lighting and the post chain (`render_t7::post::Post`) for
//! frames from the first deathmatch spawn on whatever adapter wgpu finds;
//! reports validation errors and the post chain's gaps. `T7_POST_PNG=<file>`:
//! the final image (what `Host::final_image` hands to the screen).
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
    // `T7_EYE="x y z pitch yaw"`: this view instead.
    let mut pitch = 5.0f32;
    if let Ok(e) = std::env::var("T7_EYE") {
        let v: Vec<f32> = e.split_whitespace().filter_map(|x| x.parse().ok()).collect();
        if v.len() >= 3 {
            eye = glam::Vec3::new(v[0], v[1], v[2]);
        }
        if v.len() >= 5 {
            (pitch, yaw) = (v[3], v[4]);
        }
    }
    println!("view: eye {eye} pitch {pitch} yaw {yaw}");
    let view = render_h1::code::View { eye, angles: glam::Vec3::new(pitch, yaw, 0.0), fov_x: 80.0, width: w, height: h, near: 4.0 };
    let cam = render_h1::code::Camera::from(&view);
    g.update(&render_t7::code::CodeConsts::view(&cam));
    let mut lit = render_t7::lighting::Lighting::new(&gpu, &g.scene, zone.parent().expect("zone dir")).expect("lighting");
    lit.prepare(&targets).expect("lighting bind");
    for l in &lit.report {
        println!("{l}");
    }
    let frames: u32 = a.get(4).and_then(|s| s.parse().ok()).unwrap_or(3);
    let mut post = render_t7::post::Post::new(&gpu, &g.scene, zone.parent().expect("zone dir")).expect("post");
    // The view's eye exposure first: the lit scene is scaled by it.
    lit.rel_hdr_exposure = post.begin_view(&cam);
    println!("exposure: relHDRExposure {:?}", lit.rel_hdr_exposure);
    lit.update(&cam, 0, true);
    for l in &post.report {
        println!("{l}");
    }
    let scope = gpu.device.push_error_scope(wgpu::ErrorFilter::Validation);
    for f in 0..frames {
        let mut enc = gpu.device.create_command_encoder(&Default::default());
        if f == 0 {
            g.record(&mut enc, &targets);
            lit.record(&mut enc);
        }
        post.run(&mut enc, lit.scene_target().expect("scene target"), w, h, &cam).expect("post run");
        gpu.queue.submit([enc.finish()]);
    }
    let err = pollster::block_on(scope.pop());
    let _ = gpu.device.poll(wgpu::PollType::wait_indefinitely());
    match err {
        Some(e) => println!("frame: recorded with errors: {e}"),
        None => println!("frame: G-buffer, lighting and post chain recorded and submitted without validation errors ({frames} post frames)"),
    }
    println!("exposure: metered {:?} (metered, ev, scale)", post.last_exposure);
    // Each stage's range (RGBA16F / R32F targets read back as floats).
    let stats = |tex: &wgpu::Texture, name: &str, alpha: bool| {
        let fmt = tex.format();
        let bpp = fmt.block_copy_size(None).unwrap();
        let (tw, th) = (tex.width(), tex.height());
        let row = (tw * bpp).div_ceil(256) * 256;
        let rb = gpu.device.create_buffer(&wgpu::BufferDescriptor { label: None, size: u64::from(row * th), usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ, mapped_at_creation: false });
        let mut enc = gpu.device.create_command_encoder(&Default::default());
        enc.copy_texture_to_buffer(tex.as_image_copy(), wgpu::TexelCopyBufferInfo { buffer: &rb, layout: wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(row), rows_per_image: None } }, tex.size());
        gpu.queue.submit([enc.finish()]);
        rb.slice(..).map_async(wgpu::MapMode::Read, |_| {});
        let _ = gpu.device.poll(wgpu::PollType::wait_indefinitely());
        let d = rb.slice(..).get_mapped_range().to_vec();
        let mut v = Vec::new();
        for y in 0..th {
            for x in 0..tw {
                let o = (y * row + x * bpp) as usize;
                let val = match fmt {
                    wgpu::TextureFormat::R32Float => f32::from_le_bytes(d[o..o + 4].try_into().unwrap()),
                    wgpu::TextureFormat::Rgba16Float => if alpha { h16(d[o + 6], d[o + 7]) } else { h16(d[o], d[o + 1]).max(h16(d[o + 2], d[o + 3])).max(h16(d[o + 4], d[o + 5])) },
                    _ => f32::from(d[o]) / 255.0,
                };
                v.push(val);
            }
        }
        let first = v[0];
        v.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        let q = |f: f32| v[((v.len() - 1) as f32 * f) as usize];
        println!("{name}: {tw}x{th} first {first} min {} p50 {} p90 {} max {}", q(0.0), q(0.5), q(0.9), q(1.0));
    };
    stats(lit.scene_texture().expect("scene"), "scene", false);
    for (id, n) in [(0x2du16, "0x2d"), (0x35, "0x35"), (0x37, "0x37"), (0x39, "0x39"), (0x23, "lum 0x23"), (0x40, "blur 0x40"), (0x2e, "bloom 0x2e"), (0x48, "apply 0x48")] {
        if let Some(t) = post.target(id) {
            stats(t, n, false);
            if t.format() == wgpu::TextureFormat::Rgba16Float {
                stats(t, &format!("{n} alpha"), true);
            }
        }
    }
    let fin = post.final_image().expect("final");
    let tex = fin.texture();
    let row = (w * 4).div_ceil(256) * 256;
    let rb = gpu.device.create_buffer(&wgpu::BufferDescriptor { label: None, size: u64::from(row * h), usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ, mapped_at_creation: false });
    let mut enc = gpu.device.create_command_encoder(&Default::default());
    enc.copy_texture_to_buffer(tex.as_image_copy(), wgpu::TexelCopyBufferInfo { buffer: &rb, layout: wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(row), rows_per_image: None } }, tex.size());
    gpu.queue.submit([enc.finish()]);
    rb.slice(..).map_async(wgpu::MapMode::Read, |_| {});
    let _ = gpu.device.poll(wgpu::PollType::wait_indefinitely());
    let data = rb.slice(..).get_mapped_range().to_vec();
    let mut px = Vec::with_capacity((w * h * 3) as usize);
    let mut sum = [0u64; 3];
    for y in 0..h {
        for x in 0..w {
            let o = (y * row + x * 4) as usize;
            for c in 0..3 {
                px.push(data[o + c]);
                sum[c] += u64::from(data[o + c]);
            }
        }
    }
    let n = u64::from(w * h);
    println!("final: mean rgb {} {} {}", sum[0] / n, sum[1] / n, sum[2] / n);
    if let Ok(path) = std::env::var("T7_POST_PNG") {
        let f = std::fs::File::create(&path).expect("png");
        let mut e = png::Encoder::new(std::io::BufWriter::new(f), w, h);
        e.set_color(png::ColorType::Rgb);
        e.write_header().and_then(|mut wr| wr.write_image_data(&px)).expect("png write");
        println!("final: wrote {path}");
    }
}

fn h16(a: u8, b: u8) -> f32 {
    let h = u16::from_le_bytes([a, b]);
    let s = if h & 0x8000 != 0 { -1.0 } else { 1.0 };
    let e = i32::from(h >> 10 & 0x1f);
    let m = f32::from(h & 0x3ff);
    s * if e == 0 { m * 2f32.powi(-24) } else if e == 31 { f32::INFINITY } else { (1.0 + m / 1024.0) * 2f32.powi(e - 15) }
}
