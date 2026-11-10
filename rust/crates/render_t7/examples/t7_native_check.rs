//! `t7_native_check <map.ff> [width height]`: reads a map the way the game
//! will, makes every G-buffer pipeline (each must create and validate, its
//! bindings matching the programs' RDEF), binds every draw, records one
//! frame on whatever adapter wgpu finds (software on a machine without a
//! GPU) and prints what was made, what failed and what is missing.
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
    // `T7_EYE="x y z pitch yaw"`: another view.
    let mut pitch = 5.0f32;
    if let Some(v) = std::env::var("T7_EYE").ok().map(|v| v.split_whitespace().filter_map(|x| x.parse::<f32>().ok()).collect::<Vec<_>>())
        && v.len() == 5
    {
        (eye, pitch, yaw) = (glam::Vec3::new(v[0], v[1], v[2]), v[3], v[4]);
    }
    println!("view: eye {eye} yaw {yaw}");
    let view = render_h1::code::View { eye, angles: glam::Vec3::new(pitch, yaw, 0.0), fov_x: 80.0, width: w, height: h, near: 4.0 };
    let cam = render_h1::code::Camera::from(&view);
    g.update(&render_t7::code::CodeConsts::view(&cam));
    let scope = gpu.device.push_error_scope(wgpu::ErrorFilter::Validation);
    let mut enc = gpu.device.create_command_encoder(&Default::default());
    g.record(&mut enc, &targets);
    gpu.queue.submit([enc.finish()]);
    let err = pollster::block_on(scope.pop());
    let _ = gpu.device.poll(wgpu::PollType::wait_indefinitely());
    // `T7_COVERAGE=1`: every draw alone with an occlusion query.
    // `T7_COVERAGE=<n>` (n > 1): from the first n deathmatch spawns, spread
    // over the list, each view's audit after a `coverage view` line.
    if let Ok(v) = std::env::var("T7_COVERAGE") {
        let views: usize = v.parse().unwrap_or(1).max(1);
        if views == 1 {
            for l in g.coverage_audit(&targets, cam.view_projection * glam::Mat4::from_translation(-cam.eye)) {
                println!("{l}");
            }
        } else {
            let mut spawns: Vec<(glam::Vec3, f32)> = Vec::new();
            for block in g.scene.map.entities.split('}') {
                if !block.contains("mp_dm_spawn") && !block.contains("mp_tdm_spawn") {
                    continue;
                }
                let val = |k: &str| block.split('\n').find_map(|l| l.trim().strip_prefix(&format!("\"{k}\" \"")).map(|v| v.trim_end_matches('"').to_owned()));
                let o: Vec<f32> = val("origin").map(|o| o.split_whitespace().filter_map(|x| x.parse().ok()).collect()).unwrap_or_default();
                if o.len() == 3 {
                    let yaw = val("angles").and_then(|a| a.split_whitespace().nth(1).and_then(|y| y.parse().ok())).unwrap_or(0.0);
                    spawns.push((glam::Vec3::new(o[0], o[1], o[2] + 60.0), yaw));
                }
            }
            let step = (spawns.len() / views).max(1);
            for (i, &(eye, yaw)) in spawns.iter().step_by(step).take(views).enumerate() {
                let view = render_h1::code::View { eye, angles: glam::Vec3::new(5.0, yaw, 0.0), fov_x: 80.0, width: w, height: h, near: 4.0 };
                let cam = render_h1::code::Camera::from(&view);
                g.update(&render_t7::code::CodeConsts::view(&cam));
                println!("coverage view {i}: eye {eye} yaw {yaw}");
                for l in g.coverage_audit(&targets, cam.view_projection * glam::Mat4::from_translation(-cam.eye)) {
                    println!("{l}");
                }
            }
        }
    }
    let r = &g.report;
    println!("pipelines: {} made, {} failed", r.pipelines_made, r.pipelines_failed.len());
    for f in &r.pipelines_failed {
        println!("  pipeline failed: {f}");
    }
    println!("draws: {} ready of {}", r.draws_ready, g.scene.draws.len());
    for d in &r.draws_skipped {
        println!("  skipped: {d}");
    }
    let mut im = r.images_missing.clone();
    im.sort();
    im.dedup();
    println!("images missing: {}", im.len());
    for i in &im {
        println!("  image: {i}");
    }
    for c in &r.code_constants_missing {
        println!("  code constant missing: {c}");
    }
    // `T7_CHECK_PNG=<file>`: the normal target as a picture (geometry check).
    if let Ok(path) = std::env::var("T7_CHECK_PNG") {
        let row = (w * 4).div_ceil(256) * 256;
        let buf = gpu.device.create_buffer(&wgpu::BufferDescriptor { label: None, size: u64::from(row * h), usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ, mapped_at_creation: false });
        let mut enc = gpu.device.create_command_encoder(&Default::default());
        enc.copy_texture_to_buffer(targets.colour[1].texture.as_image_copy(), wgpu::TexelCopyBufferInfo { buffer: &buf, layout: wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(row), rows_per_image: Some(h) } }, wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 });
        gpu.queue.submit([enc.finish()]);
        buf.slice(..).map_async(wgpu::MapMode::Read, |_| {});
        let _ = gpu.device.poll(wgpu::PollType::wait_indefinitely());
        let data = buf.slice(..).get_mapped_range();
        let mut px = Vec::with_capacity((w * h * 3) as usize);
        for y in 0..h {
            for x in 0..w {
                let o = (y * row + x * 4) as usize;
                let v = u32::from_le_bytes(data[o..o + 4].try_into().unwrap());
                for k in 0..3 {
                    px.push((((v >> (10 * k)) & 0x3ff) >> 2) as u8);
                }
            }
        }
        let f = std::fs::File::create(&path).expect("png");
        let mut e = png::Encoder::new(std::io::BufWriter::new(f), w, h);
        e.set_color(png::ColorType::Rgb);
        e.write_header().and_then(|mut wr| wr.write_image_data(&px)).expect("png write");
        println!("normal target written to {path}");
    }
    match err {
        Some(e) => println!("frame: recorded with errors: {e}"),
        None => println!("frame: recorded and submitted without validation errors"),
    }
}
