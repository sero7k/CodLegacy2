//! A decal cut out of the map's world (`render_iw6::decal`) makes its ring
//! (a new draw and dynamic mesh), the lists are made again, and the frames
//! after it draw; the mark then ages out. Needs a Ghosts map
//! (`IW6_TEST_MAP`, default the owner's upload) and a GPU; skipped without.

use std::path::PathBuf;

use glam::Vec3;
use render_h1::code::{Camera, View};
use render_h1::gpu::Gpu;
use render_iw6::decal::Spawn;
use render_iw6::host::{Host, Look};
use render_iw6::scene::{List, Scene};

/// (Each test loads the whole map: one at a time, or the machine runs out of memory.)
static SERIAL: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[test]
fn a_decal_ring_is_made_and_drawn() {
    let _one = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let map: PathBuf = std::env::var("IW6_TEST_MAP")
        .unwrap_or_else(|_| "/mnt/user-data/uploads/ghosts_game_files/zone/mp_frag.ff".into())
        .into();
    if !map.exists() {
        eprintln!("skipped: no map at {}", map.display());
        return;
    }
    let Ok(gpu) = Gpu::headless() else {
        eprintln!("skipped: no GPU");
        return;
    };
    // SAFETY: nothing else in this test binary reads the environment concurrently.
    unsafe { std::env::set_var("BO2MP_QUALITY", "low") };
    let zones = Scene::zones_for(&map);
    let scene = Scene::load(&zones).expect("load");
    let (eye, angles) = scene.intermission().unwrap_or((Vec3::new(0.0, 0.0, 200.0), Vec3::ZERO));
    let mut host = Host::new(gpu.clone(), scene, wgpu::TextureFormat::Depth32Float, zones);
    // The map's own decal-list surface: its material and where it is.
    let (di, name) = {
        let s = &host.renderer.scene;
        let di = s
            .draws
            .iter()
            .enumerate()
            .position(|(i, d)| d.list == List::Decal && d.object.is_none() && host.renderer.bounds[i].is_some())
            .expect("a world decal surface");
        (di, s.materials[s.draws[di].material].name.clone())
    };
    let (lo, hi) = host.renderer.bounds[di].unwrap();
    let centre = (lo + hi) * 0.5;
    let draws_before = host.renderer.scene.draws.len();
    // (Any of the six facings: the surface's own normal is not known here.)
    let facings: [[[f32; 3]; 3]; 6] = [
        [[0., 0., 1.], [1., 0., 0.], [0., 1., 0.]],
        [[0., 0., -1.], [1., 0., 0.], [0., -1., 0.]],
        [[1., 0., 0.], [0., 1., 0.], [0., 0., 1.]],
        [[-1., 0., 0.], [0., 1., 0.], [0., 0., -1.]],
        [[0., 1., 0.], [1., 0., 0.], [0., 0., -1.]],
        [[0., -1., 0.], [1., 0., 0.], [0., 0., 1.]],
    ];
    let made = facings.iter().any(|axis| {
        host.add_decal(&Spawn { origin: centre.to_array(), axis: *axis, radius: 24.0, color: 0xffff_ffff, life_ms: 1500, material: &name })
    });
    if !made {
        eprintln!("skipped: no facing cut anything from {name} at {centre:?}");
        return;
    }
    assert_eq!(host.renderer.scene.draws.len(), draws_before + 1, "one ring draw");
    let (w, h) = (320u32, 180u32);
    let depth = render_h1::post::target(&gpu, "depth", w, h, wgpu::TextureFormat::Depth32Float);
    let display = render_h1::post::target(&gpu, "display", w, h, wgpu::TextureFormat::Rgba8Unorm);
    let dv = depth.create_view(&Default::default());
    let ds = depth.create_view(&wgpu::TextureViewDescriptor { aspect: wgpu::TextureAspect::DepthOnly, ..Default::default() });
    let ov = display.create_view(&Default::default());
    let camera = Camera::from(&View { eye, angles, fov_x: 80.0, width: w, height: h, near: 4.0 });
    for what in ["with the mark", "fading", "after it ended"] {
        // (Sun shadow proxies ride the first two phases and leave in the third:
        // the cascades go to their own atlas, are copied back each frame and
        // return.)
        let proxy = [render_iw6::caster::Caster::standing([eye.x + 80.0, eye.y, eye.z - 60.0], 70.0, 15.0)];
        host.set_sun_casters(if what == "after it ended" { &[] } else { &proxy });
        for _ in 0..2 {
            let mut enc = gpu.device.create_command_encoder(&Default::default());
            host.record(&mut enc, &dv, &ds, &ov, wgpu::TextureFormat::Rgba8Unorm, w, h, &camera, &Look::default())
                .unwrap_or_else(|e| panic!("{what}: {e}"));
            gpu.queue.submit([enc.finish()]);
        }
        let _ = gpu.device.poll(wgpu::PollType::wait_indefinitely());
        std::thread::sleep(std::time::Duration::from_millis(900));
    }
}

/// A distortion sprite (`render_iw6::distort`) makes its ring, the scene is
/// copied for it in `record_fog`, and the frame draws. Skipped when no zone
/// holds a material on a `distortion*` technique set.
#[test]
fn a_distortion_quad_is_drawn_over_a_scene_copy() {
    let _one = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    use render_iw6::distort::Quad;
    let map: PathBuf = std::env::var("IW6_TEST_MAP")
        .unwrap_or_else(|_| "/mnt/user-data/uploads/ghosts_game_files/zone/mp_frag.ff".into())
        .into();
    if !map.exists() {
        eprintln!("skipped: no map at {}", map.display());
        return;
    }
    let Ok(gpu) = Gpu::headless() else {
        eprintln!("skipped: no GPU");
        return;
    };
    // SAFETY: nothing else in this test binary reads the environment concurrently.
    unsafe { std::env::set_var("BO2MP_QUALITY", "low") };
    let zones = Scene::zones_for(&map);
    let scene = Scene::load(&zones).expect("load");
    let name = scene.zones.iter().find_map(|z| {
        z.cap
            .materials
            .iter()
            .find(|m| m.techset.is_some_and(|t| z.cap.name_of(t).trim_start_matches(',').starts_with("distortion")))
            .map(|m| m.name.clone())
    });
    let Some(name) = name else {
        eprintln!("skipped: no distortion material in the zones");
        return;
    };
    let (eye, angles) = scene.intermission().unwrap_or((Vec3::new(0.0, 0.0, 200.0), Vec3::ZERO));
    let mut host = Host::new(gpu.clone(), scene, wgpu::TextureFormat::Depth32Float, zones);
    let (w, h) = (320u32, 180u32);
    let depth = render_h1::post::target(&gpu, "depth", w, h, wgpu::TextureFormat::Depth32Float);
    let display = render_h1::post::target(&gpu, "display", w, h, wgpu::TextureFormat::Rgba8Unorm);
    let dv = depth.create_view(&Default::default());
    let ds = depth.create_view(&wgpu::TextureViewDescriptor { aspect: wgpu::TextureAspect::DepthOnly, ..Default::default() });
    let ov = display.create_view(&Default::default());
    let camera = Camera::from(&View { eye, angles, fov_x: 80.0, width: w, height: h, near: 4.0 });
    let quad = Quad { center: (eye + Vec3::new(120.0, 0.0, 0.0)).to_array(), right: [0.0, 30.0, 0.0], up: [0.0, 0.0, 30.0], color: 0x80ff_ffff };
    for frame in 0..4 {
        // (The first frame sizes the targets; the quad rides every frame.)
        host.set_distortion(&[(&name, quad)]);
        let mut enc = gpu.device.create_command_encoder(&Default::default());
        host.record(&mut enc, &dv, &ds, &ov, wgpu::TextureFormat::Rgba8Unorm, w, h, &camera, &Look::default())
            .unwrap_or_else(|e| panic!("frame {frame}: {e}"));
        gpu.queue.submit([enc.finish()]);
    }
    let _ = gpu.device.poll(wgpu::PollType::wait_indefinitely());
    // None in view: the ring stays, nothing is copied.
    host.set_distortion(&[]);
    let mut enc = gpu.device.create_command_encoder(&Default::default());
    host.record(&mut enc, &dv, &ds, &ov, wgpu::TextureFormat::Rgba8Unorm, w, h, &camera, &Look::default()).expect("frame without");
    gpu.queue.submit([enc.finish()]);
    let _ = gpu.device.poll(wgpu::PollType::wait_indefinitely());
}
