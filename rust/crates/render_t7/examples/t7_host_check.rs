//! `t7_host_check <map.ff> [w h]`: the game's per-frame path (`Host`) for a
//! few frames on whatever adapter wgpu finds: waits for the lighting stage
//! (made on its own thread), then records frames and reports whether each
//! is complete, has a final image and validates, with the time per frame.
//! `BO2MP_T7_PROFILE=1` (`T7_PROFILE_EVERY=<s>`): the `t7 profile:` lines
//! (CPU per phase, submit, draws, passes, switches, bytes uploaded).
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let zone = std::path::PathBuf::from(a.get(1).expect("map zone"));
    let w: u32 = a.get(2).and_then(|s| s.parse().ok()).unwrap_or(640);
    let h: u32 = a.get(3).and_then(|s| s.parse().ok()).unwrap_or(360);
    let t_start = std::time::Instant::now();
    let gpu = render_h1::gpu::Gpu::headless().expect("gpu");
    // As the game does as the map load begins: the programs and pipelines
    // that need no map are made while the zone is read (`T7_NO_PREWARM=1`
    // leaves it out).
    if !std::env::var("T7_NO_PREWARM").is_ok_and(|v| v != "0") {
        let _ = render_t7::prewarm::start(gpu.clone(), zone.parent().unwrap().to_path_buf());
    }
    let scene = render_t7::scene::Scene::load(&zone).expect("scene");
    println!("timing: Scene::load total {:.2}s ({} draws, {} materials)", t_start.elapsed().as_secs_f32(), scene.draws.len(), scene.materials.len());
    for l in scene.report.iter().filter(|l| l.starts_with("timing") || l.starts_with("static models")) {
        println!("{l}");
    }
    let (mins, maxs) = (scene.map.mins, scene.map.maxs);
    let t_host = std::time::Instant::now();
    let mut host = render_t7::host::Host::new(gpu.clone(), scene);
    println!("timing: Host::new {:.2}s", t_host.elapsed().as_secs_f32());
    let eye = glam::Vec3::new((mins[0] + maxs[0]) * 0.5, (mins[1] + maxs[1]) * 0.5, (mins[2] + maxs[2]) * 0.5 + 200.0);
    // `T7_EYE="x y z pitch yaw"`: another view.
    let (eye, pitch, yaw) = match std::env::var("T7_EYE").ok().map(|v| v.split_whitespace().filter_map(|x| x.parse::<f32>().ok()).collect::<Vec<_>>()) {
        Some(v) if v.len() == 5 => (glam::Vec3::new(v[0], v[1], v[2]), v[3], v[4]),
        _ => (eye, 10.0, 45.0),
    };
    let view = render_h1::code::View { eye, angles: glam::Vec3::new(pitch, yaw, 0.0), fov_x: 80.0, width: w, height: h, near: 4.0 };
    let walk = std::env::var("T7_WALK").is_ok_and(|v| v == "1");
    let t0 = std::time::Instant::now();
    let mut done = 0;
    for frame in 0..100_000 {
        let scope = gpu.device.push_error_scope(wgpu::ErrorFilter::Validation);
        let t = std::time::Instant::now();
        let mut enc = gpu.device.create_command_encoder(&Default::default());
        // `T7_WALK=1`: once frames finish, the eye walks the map (a new
        // place and heading every frame, a cut every 16th), as a player's
        // view moves: the spot shadow tiles, the effects worker's skew and
        // cut paths and the culling see a moving view.
        let cam = if walk && done > 0 {
            let k = done as f32;
            let (cx, cy) = ((mins[0] + maxs[0]) * 0.5, (mins[1] + maxs[1]) * 0.5);
            // (About 40 units a frame, the box's middle 30%: a map's bounds
            // can take in its skybox.)
            let (rx, ry) = (((maxs[0] - mins[0]) * 0.15).max(1.0), ((maxs[1] - mins[1]) * 0.15).max(1.0));
            let a = if done % 16 == 0 { k * 2.399 } else { k * 40.0 / rx };
            let e = glam::Vec3::new(cx + rx * a.cos(), cy + ry * (a * 1.3).sin(), eye.z - 120.0 + 60.0 * (k * 0.21).sin());
            render_h1::code::Camera::from(&render_h1::code::View { eye: e, angles: glam::Vec3::new(5.0, k * 23.0, 0.0), ..view })
        } else {
            render_h1::code::Camera::from(&view)
        };
        let complete = host.record(&mut enc, w, h, &cam).expect("record");
        let t_rec = t.elapsed().as_secs_f32() * 1000.0;
        host.end_frame(&mut enc, f64::from(t_rec));
        let t_sub = std::time::Instant::now();
        gpu.queue.submit([enc.finish()]);
        render_t7::profile::submit(t_sub.elapsed().as_secs_f64() * 1000.0);
        let _ = gpu.device.poll(wgpu::PollType::wait_indefinitely());
        if complete {
            println!("timing: frame {frame}: CPU record {t_rec:.1} ms, submit+wait {:.1} ms", t.elapsed().as_secs_f32() * 1000.0 - t_rec);
        }
        let err = pollster::block_on(scope.pop());
        for l in host.take_report() {
            if l.starts_with("t7 profile") {
                println!("{l}");
                continue;
            }
            if l.starts_with("t7 ") || l.starts_with("frame ms") || l.starts_with("lighting") || l.starts_with("post") || l.contains("first frame") || l.contains("sky") || l.starts_with("forward") || l.starts_with("gbuffer: image") || l.starts_with("debug") || l.starts_with("light ") || l.starts_with("cull ") || l.starts_with("timing") || l.starts_with("gbuffer: ") && l.contains("pipelines made") {
                println!("{l}");
            }
        }
        if complete || err.is_some() {
            if let Some(r) = gpu.device.generate_allocator_report() {
                let mut big: Vec<_> = r.allocations.iter().map(|a| (a.size, a.name.clone())).collect();
                big.sort_by(|a, b| b.0.cmp(&a.0));
                println!("gpu memory: {} MB allocated, {} MB reserved, {} allocations; largest: {:?}", r.total_allocated_bytes >> 20, r.total_reserved_bytes >> 20, r.allocations.len(), big.iter().take(12).map(|(s, n)| format!("{n} {} MB", s >> 20)).collect::<Vec<_>>());
            }
            println!("timing: first complete frame at {:.1}s since start", t_start.elapsed().as_secs_f32());
            println!("frame {frame}: complete {complete}, final image {}, {:.0} ms, validation {:?}", host.final_image().is_some(), t.elapsed().as_secs_f32() * 1000.0, err.map(|e| e.to_string()));
            done += 1;
            // `T7_FRAMES=n`: n frames in all (timing lines).
            let want: usize = std::env::var("T7_FRAMES").ok().and_then(|v| v.parse().ok()).unwrap_or(3);
            if done == 3 {
                if let (Ok(path), Some(img)) = (std::env::var("T7_HOST_PNG"), host.final_image()) {
                    write_png(&gpu, img.texture(), w, h, &path);
                }
            }
            if done >= want.max(3) {
                break;
            }
        } else {
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        if t0.elapsed().as_secs() > 1200 {
            println!("gave up waiting for the lighting stage");
            break;
        }
    }
    // `T7_POISON` / `T7_AUDIT`: what the frames left of the poison, and the
    // buffer accesses the compute shaders made outside their buffers.
    if render_t7::audit::bounds_audit() {
        for l in render_t7::audit::oob_report(&gpu) {
            println!("{l}");
        }
        println!("oob report done");
    }
    if render_t7::audit::poison_textures() {
        for l in render_t7::audit::scan(&gpu, None) {
            println!("{l}");
        }
    }
    if render_t7::audit::poison_storage() || render_t7::audit::poison_uniform() {
        for l in render_t7::audit::scan_buffers(&gpu) {
            println!("{l}");
        }
    }
}

/// The final image (RGBA8, display-encoded) as a PNG (`T7_HOST_PNG=<file>`).
/// With `BO2MP_T7_NATIVE_DEBUG=lit` the image is the HDR lit scene
/// (RGBA16F): its mean and max are printed (`lit scene:`) and the PNG is
/// `x / (1 + x)` of it times `T7_LIT_SCALE` (default 1).
fn write_png(gpu: &render_h1::gpu::Gpu, tex: &wgpu::Texture, w: u32, h: u32, path: &str) {
    let hdr = tex.format() == wgpu::TextureFormat::Rgba16Float;
    let bpp = if hdr { 8 } else { 4 };
    let row = (w * bpp).div_ceil(256) * 256;
    let rb = gpu.device.create_buffer(&wgpu::BufferDescriptor { label: None, size: u64::from(row * h), usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ, mapped_at_creation: false });
    let mut enc = gpu.device.create_command_encoder(&Default::default());
    enc.copy_texture_to_buffer(tex.as_image_copy(), wgpu::TexelCopyBufferInfo { buffer: &rb, layout: wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(row), rows_per_image: None } }, tex.size());
    gpu.queue.submit([enc.finish()]);
    rb.slice(..).map_async(wgpu::MapMode::Read, |_| {});
    let _ = gpu.device.poll(wgpu::PollType::wait_indefinitely());
    let data = rb.slice(..).get_mapped_range().to_vec();
    let mut px = Vec::with_capacity((w * h * 3) as usize);
    let half = |b: &[u8]| {
        let v = u16::from_le_bytes([b[0], b[1]]);
        let (sign, e, m) = (if v >> 15 == 1 { -1.0f32 } else { 1.0 }, i32::from(v >> 10 & 0x1f), f32::from(v & 0x3ff));
        sign * match e {
            0 => m / 1024.0 * 2f32.powi(-14),
            31 => f32::INFINITY,
            _ => (1.0 + m / 1024.0) * 2f32.powi(e - 15),
        }
    };
    let scale: f32 = std::env::var("T7_LIT_SCALE").ok().and_then(|v| v.parse().ok()).unwrap_or(1.0);
    let (mut sum, mut max) = ([0f64; 3], [0f32; 3]);
    for y in 0..h {
        for x in 0..w {
            let o = (y * row + x * bpp) as usize;
            if hdr {
                for c in 0..3 {
                    let v = half(&data[o + 2 * c..]);
                    sum[c] += f64::from(v);
                    max[c] = max[c].max(v);
                    let t = (v * scale).max(0.0);
                    px.push((t / (1.0 + t) * 255.0).round() as u8);
                }
            } else {
                px.extend_from_slice(&data[o..o + 3]);
            }
        }
    }
    if hdr {
        let n = f64::from(w * h);
        println!("lit scene: mean {:.4} {:.4} {:.4}, max {:.3} {:.3} {:.3}", sum[0] / n, sum[1] / n, sum[2] / n, max[0], max[1], max[2]);
    }
    let f = std::fs::File::create(path).expect("png");
    let mut e = png::Encoder::new(std::io::BufWriter::new(f), w, h);
    e.set_color(png::ColorType::Rgb);
    e.write_header().and_then(|mut wr| wr.write_image_data(&px)).expect("png write");
    println!("final image written to {path}");
}
