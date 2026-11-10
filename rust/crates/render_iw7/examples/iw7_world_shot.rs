//! A frame of an IW7 map drawn by its own techniques (`render_iw7::world`):
//! `iw7_world_shot <map.ff> <out.png> [width height] [eye_x eye_y eye_z yaw
//! pitch]`. The techsets zones are the ones beside the map
//! (`techsets_<map>.ff`, `techsets_global_core_mp.ff`). Without an eye, the
//! first spawn point's view, 60 units up. Prints what was drawn, skipped
//! and left at zero. `BO2MP_IW7_LIGHTING_ONLY=1`: white pictures.
//!
//! On a software device every pipeline costs memory, so only what lies in
//! a cone a little wider than the view and within `IW7_SHOT_RANGE` units
//! (default 3000) is drawn, in distance bands of `IW7_SHOT_BAND` surfaces
//! (default 2500), each band's host made, drawn into the shared targets
//! and dropped before the next.
use glam::Vec3;
use render_iw7::world::{self, Camera, Report};

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let zone = std::path::Path::new(&a[1]);
    let out = &a[2];
    let num = |i: usize, d: f32| a.get(i).and_then(|s| s.parse().ok()).unwrap_or(d);
    let (w, h) = (num(3, 960.0) as u32, num(4, 540.0) as u32);
    let src = fastfile_iw7::map::MapSources::beside(zone).with_common();
    let map = fastfile_iw7::map::Iw7Map::load(&src).expect("map");
    let tech_path = src.techsets.clone().expect("techsets zone beside the map");
    let techsets = fastfile_iw7::Zone::open(&tech_path).expect("techsets");
    // The global technique sets beside the map, whose programs its passes may name.
    let shared: Vec<fastfile_iw7::Zone> = world::shared_technique_zones(zone).iter().filter_map(|p| fastfile_iw7::Zone::open(p).ok()).collect();
    let extra: Vec<&fastfile_iw7::Zone> = shared.iter().collect();
    let (eye, yaw, pitch) = if a.len() >= 10 {
        (Vec3::new(num(5, 0.0), num(6, 0.0), num(7, 0.0)), num(8, 0.0), num(9, 0.0))
    } else {
        let s = map.spawns.iter().find(|s| s.classname.contains("tdm")).or(map.spawns.first()).expect("a spawn");
        (Vec3::from(s.origin) + Vec3::Z * 60.0, s.angles[1], 0.0)
    };
    let (y, p) = (yaw.to_radians(), pitch.to_radians());
    let fwd = Vec3::new(p.cos() * y.cos(), p.cos() * y.sin(), -p.sin());
    let cam = Camera::looking(eye, fwd, 65.0, w, h);
    println!("eye {eye:?} yaw {yaw} pitch {pitch}; sun {:?}", world::Sun::of(&map));
    let g = render_h1::gpu::Gpu::headless().expect("gpu");
    let t = std::time::Instant::now();
    let range: f32 = std::env::var("IW7_SHOT_RANGE").ok().and_then(|v| v.parse().ok()).unwrap_or(3000.0);
    // `IW7_SHOT_CONE=0`: everything in range, in view or not (as in game).
    let cone = !std::env::var("IW7_SHOT_CONE").is_ok_and(|v| v.trim() == "0");
    let band: usize = std::env::var("IW7_SHOT_BAND").ok().and_then(|v| v.parse().ok()).unwrap_or(2500);
    // The distance of the nearest point, when in range and in the cone.
    let near = |mins: [f32; 3], maxs: [f32; 3]| -> Option<f32> {
        let (lo, hi) = (Vec3::from(mins), Vec3::from(maxs));
        let d_near = eye.clamp(lo, hi).distance(eye);
        let mid = (lo + hi) * 0.5;
        let radius = (hi - lo).length() * 0.5;
        let to = mid - eye;
        let d = to.length();
        let inside = d <= radius || to.dot(fwd) / d > (55f32.to_radians() + (radius / d).clamp(0.0, 1.0).asin()).min(std::f32::consts::PI).cos();
        (d_near < range && (inside || !cone)).then_some(d_near)
    };
    let mut dists: Vec<f32> = map.surfaces.iter().filter_map(|s| near(s.mins, s.maxs)).collect();
    dists.sort_by(f32::total_cmp);
    let mut edges = vec![0.0f32];
    for c in dists.chunks(band.max(1)).skip(1) {
        edges.push(c[0]);
    }
    edges.push(f32::INFINITY);
    let target = |format, label| {
        g.device.create_texture(&wgpu::TextureDescriptor {
            label: Some(label),
            size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        })
    };
    let colour = target(wgpu::TextureFormat::Rgba16Float, "shot colour");
    let depth = target(wgpu::TextureFormat::Depth32Float, "shot depth");
    let (cv, dv) = (colour.create_view(&Default::default()), depth.create_view(&Default::default()));
    let mut total = Report::default();
    for (k, pair) in edges.windows(2).enumerate() {
        let (lo, hi) = (pair[0], pair[1]);
        let keep = |mins: [f32; 3], maxs: [f32; 3]| near(mins, maxs).is_some_and(|d| d >= lo && d < hi);
        let mut host = world::Host::new_within(g.clone(), &map, &techsets, &extra, &keep).expect("host");
        println!("band {k} host build took {}", host.report.seconds);
        // `IW7_SHOT_PROFILE=<frames>`: that many whole frames (world and
        // post) first, timed, then the shot.
        if let (0, Some(frames)) = (k, std::env::var("IW7_SHOT_PROFILE").ok().and_then(|v| v.parse::<u32>().ok())) {
            let mut avg = render_iw7::profile::Average::default();
            let mut wall = 0.0f32;
            // `IW7_SHOT_TURN=<degrees>`: the view turns that much a frame
            // (motion blur), and the last frame is saved as `<out>.turn.png`.
            let turn: Option<f32> = std::env::var("IW7_SHOT_TURN").ok().and_then(|v| v.parse().ok());
            // (A software device takes seconds a frame: time it as 60 Hz so
            // the blur and the exposure's adaptation are a real frame's.)
            host.fixed_dt = turn.map(|_| 1.0 / 60.0);
            // `IW7_SHOT_DOF=<far start>,<far end>,<far blur>`: Black Ops II's
            // aiming depth of field on those frames (saved as `<out>.turn.png`).
            let dof: Option<Vec<f32>> = std::env::var("IW7_SHOT_DOF").ok().map(|v| v.split(',').filter_map(|x| x.trim().parse().ok()).collect());
            if let Some([fs, fe, fb]) = dof.as_deref() {
                host.dof_frame = render_iw7::dof::Dof { far_start: *fs, far_end: *fe, far_blur: *fb, ..Default::default() };
            }
            for f in 0..frames + 2 {
                let t0 = std::time::Instant::now();
                let mut enc = g.device.create_command_encoder(&Default::default());
                let cam_f = match turn {
                    Some(d) => {
                        let back = (frames + 1 - f) as f32 * d;
                        let (y, p) = ((yaw - back).to_radians(), pitch.to_radians());
                        Camera::looking(eye, Vec3::new(p.cos() * y.cos(), p.cos() * y.sin(), -p.sin()), 65.0, w, h)
                    }
                    None => cam,
                };
                host.record(&mut enc, &cam_f);
                g.queue.submit([enc.finish()]);
                let _ = g.device.poll(wgpu::PollType::wait_indefinitely());
                // (The first two warm up and fill the timer's readback.)
                if f >= 2 {
                    wall += t0.elapsed().as_secs_f32() * 1000.0;
                    avg.add(&host.stats);
                }
            }
            if let (true, Some(tex)) = (turn.is_some() || dof.is_some(), host.final_texture()) {
                let rgba = read_rgba8(&g, tex, w, h);
                let f = std::fs::File::create(format!("{out}.turn.png")).expect("out");
                let mut e = png::Encoder::new(std::io::BufWriter::new(f), w, h);
                e.set_color(png::ColorType::Rgba);
                e.set_depth(png::BitDepth::Eight);
                e.write_header().unwrap().write_image_data(&rgba).unwrap();
            }
            println!("{}", host.quality_line);
            println!("profile over {frames} frames: wall {:.1} ms/frame (CPU encode, submit and the device's work); {}", wall / frames.max(1) as f32, avg.mean().line());
        }
        let mut enc = g.device.create_command_encoder(&Default::default());
        host.record_into(&mut enc, &cam, &cv, &dv, k == 0);
        g.queue.submit([enc.finish()]);
        let _ = g.device.poll(wgpu::PollType::wait_indefinitely());
        // `IW7_SHOT_MAPS=<prefix>`: the screen maps as pictures too.
        if let Ok(prefix) = std::env::var("IW7_SHOT_MAPS") {
            for (name, (tex, mw, mh)) in ["shadow", "ao"].iter().zip(host.screen_maps()) {
                let px = render_h1::world::read_rgba16f(&g, tex, mw, mh);
                let grey: Vec<u8> = px.iter().flat_map(|p| {
                    let v = (p[0].clamp(0.0, 1.0) * 255.0) as u8;
                    [v, v, v, 255]
                }).collect();
                let f = std::fs::File::create(format!("{prefix}_{name}_{k}.png")).expect("out");
                let mut e = png::Encoder::new(std::io::BufWriter::new(f), mw, mh);
                e.set_color(png::ColorType::Rgba);
                e.set_depth(png::BitDepth::Eight);
                e.write_header().unwrap().write_image_data(&grey).unwrap();
            }
        }
        let r = std::mem::take(&mut host.report);
        // (wgpu frees a dropped host's resources at the next poll.)
        drop(host);
        let _ = g.device.poll(wgpu::PollType::wait_indefinitely());
        println!("band {k} ({lo:.0} to {hi:.0}): {} surfaces, {} model surfaces, {} pipelines", r.drawn, r.model_draws, r.pipelines);
        total.surfaces = r.surfaces;
        total.models_placed = r.models_placed;
        total.drawn += r.drawn;
        total.model_draws += r.model_draws;
        total.pipelines += r.pipelines;
        total.pictures += r.pictures;
        total.pictures_missing += r.pictures_missing;
        for (why, n) in r.skipped {
            if !why.contains("left out of this frame") {
                *total.skipped.entry(why).or_default() += n;
            }
        }
        for (c, n) in r.zero_constants {
            *total.zero_constants.entry(c).or_default() += n;
        }
    }
    let px = render_h1::world::read_rgba16f(&g, &colour, w, h);
    println!(
        "{} of {} surfaces and {} model surfaces ({} models placed) drawn by {} pipelines, {} pictures ({} missing), in {:.1}s",
        total.drawn,
        total.surfaces,
        total.model_draws,
        total.models_placed,
        total.pipelines,
        total.pictures,
        total.pictures_missing,
        t.elapsed().as_secs_f32()
    );
    for (why, n) in &total.skipped {
        println!("  skipped {n}: {why}");
    }
    println!("code constants left at zero (index: passes): {:?}", total.zero_constants);
    println!("auto exposure {:.4} (display default {})", world::auto_exposure(&px), world::DEFAULT_EXPOSURE);
    // IW7's display (`render_iw7::post`), or a plain exposure with
    // `IW7_SHOT_PLAIN=1`.
    let rgba = if std::env::var("IW7_SHOT_PLAIN").is_ok() {
        world::to_srgb8(&px)
    } else {
        let vision = map.vision_params();
        let clut = map.colour_table(&vision);
        println!("vision: {vision:?}; colour table {}", clut.is_some());
        let mut post = render_iw7::post::Post::new(&g, vision, clut.as_deref(), wgpu::TextureFormat::Rgba8Unorm);
        if let Some((_, text)) = map.vision.as_ref() {
            post.look = render_iw7::post::Look::from_vision(text);
        }
        let fin = target(wgpu::TextureFormat::Rgba8Unorm, "shot final");
        let mut enc = g.device.create_command_encoder(&Default::default());
        post.record(&mut enc, &cv, &fin.create_view(&Default::default()), w, h, 0.0, true);
        g.queue.submit([enc.finish()]);
        read_rgba8(&g, &fin, w, h)
    };
    let f = std::fs::File::create(out).expect("out");
    let mut e = png::Encoder::new(std::io::BufWriter::new(f), w, h);
    e.set_color(png::ColorType::Rgba);
    e.set_depth(png::BitDepth::Eight);
    e.write_header().unwrap().write_image_data(&rgba).unwrap();
}

fn read_rgba8(g: &render_h1::gpu::Gpu, tex: &wgpu::Texture, w: u32, h: u32) -> Vec<u8> {
    let row = (w * 4).div_ceil(256) * 256;
    let buf = g.device.create_buffer(&wgpu::BufferDescriptor { label: Some("readback"), size: u64::from(row * h), usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ, mapped_at_creation: false });
    let mut enc = g.device.create_command_encoder(&Default::default());
    enc.copy_texture_to_buffer(
        tex.as_image_copy(),
        wgpu::TexelCopyBufferInfo { buffer: &buf, layout: wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(row), rows_per_image: Some(h) } },
        wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
    );
    g.queue.submit([enc.finish()]);
    let slice = buf.slice(..);
    slice.map_async(wgpu::MapMode::Read, |_| {});
    let _ = g.device.poll(wgpu::PollType::wait_indefinitely());
    let data = slice.get_mapped_range();
    (0..h).flat_map(|y| data[(y * row) as usize..(y * row + w * 4) as usize].to_vec()).collect()
}
