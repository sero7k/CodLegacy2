//! `t7_floor_check <map.ff> [w h]`: the G-buffer pass alone (no lighting,
//! no forward lists: fits a small software adapter) from `T7_EYE="x y z
//! pitch yaw"`, its depth read back. Prints how many pixels have depth and,
//! per material whose draws the CPU puts in view, the pixels its triangles
//! cover and whether the G-buffer pass draws it (its G-buffer technique, or
//! the depth prepass of an opaque forward material, `Material::forward_depth`).
//! An opaque forward surface (Nuk3town's road) must have depth here, or a
//! forward surface behind it (the vista water) draws over it.
//! `T7_FORWARD_DEPTH=0` compares with the old way. Keep the scene small with
//! `CODLEGACY_T7_DEBUG_SURF=only=...`.
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let zone = std::path::PathBuf::from(a.get(1).expect("map zone"));
    let w: u32 = a.get(2).and_then(|s| s.parse().ok()).unwrap_or(320);
    let h: u32 = a.get(3).and_then(|s| s.parse().ok()).unwrap_or(180);
    let scene = render_t7::scene::Scene::load(&zone).expect("scene");
    let gpu = render_h1::gpu::Gpu::headless().expect("gpu");
    let targets = render_t7::gbuffer::Targets::new(&gpu, w, h);
    let mut g = render_t7::gbuffer::GBuffer::new(gpu.clone(), std::sync::Arc::new(scene));
    g.check_bindings_without_images = true;
    g.prepare(&targets);
    let v: Vec<f32> = std::env::var("T7_EYE").unwrap_or_else(|_| "0 0 100 10 0".into()).split_whitespace().filter_map(|x| x.parse().ok()).collect();
    let view = render_h1::code::View { eye: glam::Vec3::new(v[0], v[1], v[2]), angles: glam::Vec3::new(v[3], v[4], 0.0), fov_x: 80.0, width: w, height: h, near: 4.0 };
    let cam = render_h1::code::Camera::from(&view);
    g.update(&render_t7::code::CodeConsts::view(&cam));
    let scope = gpu.device.push_error_scope(wgpu::ErrorFilter::Validation);
    let mut enc = gpu.device.create_command_encoder(&Default::default());
    g.record(&mut enc, &targets);
    let row = (w * 4).div_ceil(256) * 256;
    let rb = gpu.device.create_buffer(&wgpu::BufferDescriptor { label: None, size: u64::from(row * h), usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ, mapped_at_creation: false });
    enc.copy_texture_to_buffer(
        wgpu::TexelCopyTextureInfo { texture: &targets.depth.texture, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::DepthOnly },
        wgpu::TexelCopyBufferInfo { buffer: &rb, layout: wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(row), rows_per_image: None } },
        wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
    );
    gpu.queue.submit([enc.finish()]);
    let err = pollster::block_on(scope.pop());
    rb.slice(..).map_async(wgpu::MapMode::Read, |_| {});
    let _ = gpu.device.poll(wgpu::PollType::wait_indefinitely());
    let data = rb.slice(..).get_mapped_range().to_vec();
    let r = &g.report;
    println!("gbuffer: {} pipelines made, {} failed; {} draws ready of {} ({} opaque forward depth); validation {:?}", r.pipelines_made, r.pipelines_failed.len(), r.draws_ready, g.scene.draws.len(), r.forward_depth, err.map(|e| e.to_string()));
    for f in &r.pipelines_failed {
        println!("  pipeline failed: {f}");
    }
    let mut with_depth = 0;
    let mut rows = Vec::new();
    for y in (0..h).step_by((h / 18).max(1) as usize) {
        let mut s = String::new();
        for x in (0..w).step_by((w / 64).max(1) as usize) {
            let o = (y * row + x * 4) as usize;
            let d = f32::from_le_bytes(data[o..o + 4].try_into().unwrap());
            s.push(if d > 0.0 { '#' } else { '.' });
        }
        rows.push(s);
    }
    for y in 0..h {
        for x in 0..w {
            let o = (y * row + x * 4) as usize;
            if f32::from_le_bytes(data[o..o + 4].try_into().unwrap()) > 0.0 {
                with_depth += 1;
            }
        }
    }
    println!("depth written: {with_depth} of {} pixels ('#' has depth):", w * h);
    for s in rows {
        println!("  {s}");
    }
    let s = &g.scene;
    let to_clip = cam.view_projection * glam::Mat4::from_translation(-cam.eye);
    let mut per: std::collections::BTreeMap<String, (f32, bool, bool)> = std::collections::BTreeMap::new();
    let ready: std::collections::HashSet<usize> = g.ready_draws().into_iter().collect();
    for (di, d) in s.draws.iter().enumerate() {
        let (cw, ccw) = render_t7::coverage::expected_pixels(s, d, to_clip, w, h);
        if cw + ccw < 1.0 {
            continue;
        }
        let m = &s.materials[d.material];
        let e = per.entry(format!("{} ({})", m.name, m.techset)).or_insert((0.0, false, m.forward_depth().is_some()));
        e.0 += cw.max(ccw);
        e.1 |= ready.contains(&di);
    }
    for (name, (px, drawn, fd)) in per {
        println!("  {px:8.0} px {name}: {}", if drawn { if fd { "G-buffer pass draws its depth prepass (opaque forward)" } else { "G-buffer pass draws it" } } else { "not in the G-buffer pass" });
    }
}
