//! Draws an MWR map's world with MWR's own techniques, offline.
//!
//! `cargo run -p render_h1 --release --example h1_native -- <map.ff> <zone.ff>... [--out pic.png] [--size 1280x720] [--cam x y z pitch yaw] [--ev stops]`
//!
//! The first zone is the map; the others (techsets_common_mp,
//! code_post_gfx_mp, common_mp, ...) supply techniques, shaders,
//! materials and pictures the map only names. The camera defaults to the
//! map's intermission view. Writes the linear picture exposed and encoded
//! as sRGB, and prints what was drawn and what was missing.

use std::path::PathBuf;

use glam::Vec3;
use render_h1::code::{Camera, View};
use render_h1::gpu::Gpu;
use render_h1::host::{Host, Look};
use render_h1::scene::Scene;
use render_h1::world::read_rgba16f;

fn main() {
    let mut zones_p: Vec<PathBuf> = Vec::new();
    let mut out = PathBuf::from("h1_native.png");
    let (mut w, mut h) = (1280u32, 720u32);
    let mut cam: Option<(Vec3, Vec3)> = None;
    let mut ev = 0.0f32;
    let mut fov_x = 80.0f32;
    let mut exposure_key = 0.18f32;
    let mut key_given = false;
    let a: Vec<String> = std::env::args().skip(1).collect();
    let mut i = 0;
    while i < a.len() {
        match a[i].as_str() {
            "--out" => {
                out = a[i + 1].clone().into();
                i += 1;
            }
            "--size" => {
                let (x, y) = a[i + 1].split_once('x').expect("WxH");
                w = x.parse().expect("w");
                h = y.parse().expect("h");
                i += 1;
            }
            "--key" => {
                key_given = true;
                exposure_key = a[i + 1].parse().expect("key");
                i += 1;
            }
            "--fov" => {
                fov_x = a[i + 1].parse().expect("fov");
                i += 1;
            }
            "--ev" => {
                ev = a[i + 1].parse().expect("ev");
                i += 1;
            }
            "--cam" => {
                let v: Vec<f32> = a[i + 1..i + 6]
                    .iter()
                    .map(|s| s.parse().expect("cam"))
                    .collect();
                cam = Some((Vec3::new(v[0], v[1], v[2]), Vec3::new(v[3], v[4], 0.0)));
                i += 5;
            }
            p => zones_p.push(p.into()),
        }
        i += 1;
    }
    let t0 = std::time::Instant::now();
    let mut scene = Scene::load(&zones_p).expect("load");
    // H1_MAX_TEX=n: pictures capped at n texels a side (default 2048), for
    // a GPU with little memory free.
    if let Some(n) = std::env::var("H1_MAX_TEX")
        .ok()
        .and_then(|v| v.parse::<u32>().ok())
    {
        scene.images.max_size = n;
    }
    for n in &scene.notes {
        eprintln!("note: {n}");
    }
    eprintln!(
        "{}: loaded in {:.1}s, {} draws, {} materials, {} objects, {} lighting tiles",
        scene.name,
        t0.elapsed().as_secs_f32(),
        scene.draws.len(),
        scene.materials.len(),
        scene.objects.len(),
        scene.tiles.len()
    );
    // Debug switches: no reflection probes, sun scale, no sun specular.
    eprintln!(
        "sun {:?}, lightmaps {:?}",
        scene.sun,
        scene.lightmaps.first()
    );
    if std::env::var("H1_NO_PROBE").is_ok() {
        scene.probes.iter_mut().for_each(|p| *p = None);
    }
    if let Some(k) = std::env::var("H1_SUN_SCALE")
        .ok()
        .and_then(|v| v.parse::<f32>().ok())
    {
        scene.sun.diffuse *= k;
        scene.sun.specular *= k;
    }
    // H1_HIDE=a,b: leave out draws whose material name holds any of them.
    if let Ok(h) = std::env::var("H1_HIDE") {
        let pats: Vec<String> = h.split(',').map(str::to_owned).collect();
        let mats = &scene.materials;
        scene.draws.retain(|d| {
            !pats
                .iter()
                .any(|p| mats[d.material].name.contains(p.as_str()))
        });
    }
    // H1_ONLY=a,b: keep only draws whose material name holds one of them.
    if let Ok(h) = std::env::var("H1_ONLY") {
        let pats: Vec<String> = h.split(',').map(str::to_owned).collect();
        let mats = &scene.materials;
        scene
            .draws
            .retain(|d| pats.iter().any(|p| mats[d.material].name.contains(p.as_str())));
        eprintln!("H1_ONLY: {} draws kept", scene.draws.len());
    }
    // H1_DBG_ENV=a,b: keep only draws in these primary light environments.
    if let Ok(h) = std::env::var("H1_DBG_ENV") {
        let envs: Vec<u16> = h.split(',').filter_map(|v| v.parse().ok()).collect();
        scene.draws.retain(|d| envs.contains(&d.light_env));
        eprintln!("H1_DBG_ENV: {} draws kept", scene.draws.len());
    }
    // H1_DBG_MESH=0|1: keep only world (0) or placed-model (1) draws.
    if let Some(m) = std::env::var("H1_DBG_MESH").ok().and_then(|v| v.parse::<usize>().ok()) {
        scene.draws.retain(|d| d.mesh == m);
    }
    // H1_DBG_LISTMATS=1: print each material's draw count.
    if std::env::var("H1_DBG_LISTMATS").is_ok() {
        let mut n: std::collections::BTreeMap<(&str, &str, u8, u8), usize> = Default::default();
        for d in &scene.draws {
            let m = &scene.materials[d.material];
            *n.entry((m.name.as_str(), m.techset.as_str(), m.camera_region, m.sort_key)).or_default() += 1;
        }
        for ((k, t, c, s), v) in n {
            eprintln!("mat {v:6} {k} {t} region {c} sort {s}");
        }
    }
    // H1_DBG_MATSWAP=from,to: draws of material `from` take material `to`.
    if let Ok(v) = std::env::var("H1_DBG_MATSWAP")
        && let Some((from, to)) = v.split_once(',')
        && let Some(ti) = scene.materials.iter().position(|m| m.name.contains(to))
    {
        let mats = scene.materials.clone();
        let mut n = 0;
        for d in &mut scene.draws {
            if mats[d.material].name.contains(from) {
                d.material = ti;
                n += 1;
            }
        }
        eprintln!("H1_DBG_MATSWAP: {n} draws now {}", mats[ti].name);
    }
    // H1_ONLY_FIRST=n: keep only the draw starting at index n.
    if let Some(n) = std::env::var("H1_ONLY_FIRST")
        .ok()
        .and_then(|v| v.parse::<u32>().ok())
    {
        scene.draws.retain(|d| d.first_index == n);
        eprintln!("H1_ONLY_FIRST: {} draws kept", scene.draws.len());
    }
    if std::env::var("H1_NO_SPEC").is_ok() {
        scene.sun.specular = Vec3::ZERO;
    }
    let (eye, angles) = cam
        .or_else(|| scene.intermission())
        .unwrap_or((Vec3::ZERO, Vec3::ZERO));
    eprintln!("camera {eye} angles {angles}");
    let view = View {
        eye,
        angles,
        fov_x,
        width: w,
        height: h,
        near: 4.0,
    };
    let camera = Camera::from(&view);
    // H1_RDOC=<path to renderdoc.dll>: loaded before the device so wgpu's
    // RenderDoc hook finds it (debug captures without injection).
    let _rdoc_lib = std::env::var("H1_RDOC").ok().and_then(|p| {
        // SAFETY: debugging only; RenderDoc's DLL is meant to be loaded so.
        unsafe { libloading::Library::new(p) }.ok()
    });
    let gpu = Gpu::headless().expect("gpu");
    eprintln!(
        "meshes: {:?} bytes; device max buffer {} B, uniform offset alignment {}",
        scene
            .meshes
            .iter()
            .map(|m| (m.vertex_bytes.len(), m.indices.len() * 2, m.layer_bytes.len()))
            .collect::<Vec<_>>(),
        gpu.device.limits().max_buffer_size,
        gpu.device.limits().min_uniform_buffer_offset_alignment
    );
    {
        let n = std::sync::atomic::AtomicUsize::new(0);
        gpu.device.on_uncaptured_error(std::sync::Arc::new(move |e: wgpu::Error| {
            let k = n.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            if k < 3 {
                eprintln!("wgpu error: {e:?}");
            } else if k == 3 {
                eprintln!("wgpu error: (more errors not shown)");
            }
        }));
    }
    eprintln!(
        "gpu: bc {} float32 filterable {}",
        gpu.bc, gpu.float32_filterable
    );
    let mut host = Host::new(
        gpu.clone(),
        scene,
        wgpu::TextureFormat::Depth32Float,
        zones_p.clone(),
    );
    let depth = gpu.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("depth"),
        size: wgpu::Extent3d {
            width: w,
            height: h,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Depth32Float,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
        view_formats: &[],
    });
    let dv = depth.create_view(&Default::default());
    let shown = render_h1::post::target(&gpu, "shown", w, h, wgpu::TextureFormat::Rgba16Float);
    let sv = shown.create_view(&Default::default());
    let mut look = Look {
        exposure: 1.0,
        fog: std::env::var("H1_FOG_OFF").is_err(),
        film: std::env::var("H1_FILM_OFF").is_err(),
        depth_range: None,
        auto_exposure: false,
    };
    let frame = |host: &mut Host, look: &Look| {
        let mut enc = gpu.device.create_command_encoder(&Default::default());
        host.record(
            &mut enc,
            &dv,
            &dv,
            &sv,
            wgpu::TextureFormat::Rgba16Float,
            w,
            h,
            &camera,
            look,
        )
        .expect("frame");
        gpu.queue.submit([enc.finish()]);
    };
    let t1 = std::time::Instant::now();
    // H1_RDOC=1: a RenderDoc capture of the first frame (run under
    // `renderdoccmd capture`).
    let rdoc = std::env::var("H1_RDOC").is_ok();
    if rdoc {
        // SAFETY: debugging only; nothing else is recording.
        unsafe { gpu.device.start_graphics_debugger_capture() };
    }
    frame(&mut host, &look);
    eprintln!("{}", host.fps_line());
    if rdoc {
        gpu.device.poll(wgpu::PollType::wait_indefinitely()).ok();
        // SAFETY: as above.
        unsafe { gpu.device.stop_graphics_debugger_capture() };
    }
    eprintln!(
        "first frame in {:.1}s: {:#?}",
        t1.elapsed().as_secs_f32(),
        host.renderer.report
    );
    // Exposure: the fogged scene's log-average luminance to `exposure_key`.
    let pic = read_rgba16f(&gpu, host.fogged().expect("fogged"), w, h);
    let (mut lsum, mut n) = (0.0f64, 0usize);
    for p in &pic {
        let l = 0.2126 * p[0] + 0.7152 * p[1] + 0.0722 * p[2];
        if l.is_finite() && l > 0.0 {
            lsum += f64::from(l.max(1e-4)).ln();
            n += 1;
        }
    }
    let avg = if n > 0 {
        (lsum / n as f64).exp() as f32
    } else {
        1.0
    };
    // H1_DUMP_HDR=path: the fogged HDR scene as raw little-endian f32 RGBA.
    if let Ok(p) = std::env::var("H1_DUMP_HDR") {
        let bytes: Vec<u8> = pic
            .iter()
            .flat_map(|c| c.iter().flat_map(|v| v.to_le_bytes()))
            .collect();
        std::fs::write(&p, bytes).expect("dump");
        eprintln!("dumped {w}x{h} HDR to {p}");
    }
    // A map with MWR's filmic tone map and a fixed exposure (its light
    // set) takes that; otherwise the log average is brought to the key.
    if std::env::var("H1_PROFILE").is_ok() {
        let (p, o, b) = host.constants_memory();
        eprintln!("constants: {p} plans, {o} objects, {} MB mirrors", b >> 20);
    }
    let ls = &host.scene().lightset;
    if std::env::var("H1_DUMP_LIGHTSET").is_ok() {
        let mut k: Vec<_> = ls.values.iter().collect();
        k.sort_by(|a, b| a.0.cmp(b.0));
        for (n, v) in k {
            eprintln!("lightset {n} {v:?}");
        }
    }
    let math = render_h1::tonemap::ToneMath::from_env();
    look.exposure = if ls.f("tonemap", 0.0) == 2.0 && ls.f("tonemapauto", 1.0) == 0.0 {
        2f32.powf(ev)
    } else if math == render_h1::tonemap::ToneMath::Exact && !key_given {
        // The game's own key: 0.18 * 2^adjust while locked, else its curve.
        render_h1::tonemap::shown_key(ls, avg, 0.0) / avg * 2f32.powf(ev)
    } else {
        exposure_key / avg * 2f32.powf(ev)
    };
    eprintln!(
        "{}",
        render_h1::tonemap::vision_line(
            &zones_p
                .first()
                .and_then(|p| p.file_stem())
                .map_or_else(String::new, |s| s.to_string_lossy().into_owned()),
            ls,
            &host.scene().vision,
            Some(avg)
        )
    );
    eprintln!(
        "log-average luminance {avg:.4}, exposure x{:.3}",
        look.exposure
    );
    frame(&mut host, &look);
    // H1_WARM=n: n more frames, the line of the last (steady CPU time).
    if let Some(n) = std::env::var("H1_WARM").ok().and_then(|v| v.parse::<u32>().ok()) {
        for _ in 0..n {
            frame(&mut host, &look);
            gpu.device.poll(wgpu::PollType::wait_indefinitely()).ok();
        }
        eprintln!("warm: {}", host.fps_line());
    }
    // H1_METER=n: n frames with the game's automatic exposure (the GPU
    // meter and adaptation, as in game), then the picture.
    if let Some(n) = std::env::var("H1_METER")
        .ok()
        .and_then(|v| v.parse::<u32>().ok())
    {
        let auto = Look {
            auto_exposure: true,
            ..look
        };
        for _ in 0..n {
            frame(&mut host, &auto);
            gpu.device.poll(wgpu::PollType::wait_indefinitely()).ok();
        }
        eprintln!(
            "meter after {n} frames: luminance {:?}",
            host.measured_luminance()
        );
    }
    // H1_BENCH=n: n more frames, timed.
    if let Some(n) = std::env::var("H1_BENCH")
        .ok()
        .and_then(|v| v.parse::<u32>().ok())
    {
        let mut cpu = 0.0f32;
        let tb = std::time::Instant::now();
        for _ in 0..n {
            let tc = std::time::Instant::now();
            frame(&mut host, &look);
            cpu += tc.elapsed().as_secs_f32();
            gpu.device.poll(wgpu::PollType::wait_indefinitely()).ok();
        }
        eprintln!(
            "{n} frames: {:.2} ms each, {:.2} ms recording",
            tb.elapsed().as_secs_f32() * 1000.0 / n as f32,
            cpu * 1000.0 / n as f32
        );
    }
    let pic = read_rgba16f(&gpu, &shown, w, h);
    // The film pass writes display values (sRGB-encoded) already.
    let enc = |c: f32| -> u8 { (c.clamp(0.0, 1.0) * 255.0 + 0.5) as u8 };
    let mut bytes = Vec::with_capacity((w * h * 3) as usize);
    for p in &pic {
        bytes.extend_from_slice(&[enc(p[0]), enc(p[1]), enc(p[2])]);
    }
    let f = std::fs::File::create(&out).expect("out");
    let mut e = png::Encoder::new(std::io::BufWriter::new(f), w, h);
    e.set_color(png::ColorType::Rgb);
    e.set_depth(png::BitDepth::Eight);
    e.write_header()
        .expect("png")
        .write_image_data(&bytes)
        .expect("png");
    eprintln!("wrote {}", out.display());
}
