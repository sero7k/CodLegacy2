//! The graphics preset changed inside one process (the in-game option
//! change and `Host::apply_quality` take the same path: every list and bind
//! group is dropped and made again) must keep drawing: no bind group's
//! dynamic offset may run past its buffer (wgpu panics on that).
//!
//! Needs a Ghosts map: `IW6_TEST_MAP=<path of mp_*.ff>` (default: the
//! owner's `ghosts_game_files` upload), and a GPU (a software Vulkan
//! driver is enough). Skipped when either is missing.

use std::path::PathBuf;

use glam::Vec3;
use render_h1::code::{Camera, View};
use render_h1::gpu::Gpu;
use render_iw6::host::{Host, Look};
use render_iw6::scene::Scene;

#[test]
fn presets_cycle_in_one_process() {
    use native_quality::{Preset, Quality};
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
    // (Starts at HIGH: the first lists of ULTRA are large.)
    // SAFETY: nothing else in this test binary reads the environment concurrently.
    unsafe { std::env::set_var("BO2MP_QUALITY", "high") };
    let zones = Scene::zones_for(&map);
    let scene = Scene::load(&zones).expect("load");
    let (eye, angles) = scene.intermission().unwrap_or((Vec3::new(0.0, 0.0, 200.0), Vec3::ZERO));
    let mut host = Host::new(gpu.clone(), scene, wgpu::TextureFormat::Depth32Float, zones);
    let (w, h) = (320u32, 180u32);
    let depth = render_h1::post::target(&gpu, "depth", w, h, wgpu::TextureFormat::Depth32Float);
    let display = render_h1::post::target(&gpu, "display", w, h, wgpu::TextureFormat::Rgba8Unorm);
    let dv = depth.create_view(&Default::default());
    let ds = depth.create_view(&wgpu::TextureViewDescriptor {
        aspect: wgpu::TextureAspect::DepthOnly,
        ..Default::default()
    });
    let ov = display.create_view(&Default::default());
    let camera = Camera::from(&View { eye, angles, fov_x: 80.0, width: w, height: h, near: 4.0 });
    let mut frame = |host: &mut Host, what: &str| {
        for _ in 0..2 {
            let mut enc = gpu.device.create_command_encoder(&Default::default());
            host.record(&mut enc, &dv, &ds, &ov, wgpu::TextureFormat::Rgba8Unorm, w, h, &camera, &Look::default())
                .unwrap_or_else(|e| panic!("{what}: {e}"));
            gpu.queue.submit([enc.finish()]);
        }
        let _ = gpu.device.poll(wgpu::PollType::wait_indefinitely());
    };
    frame(&mut host, "start");
    let order = [Preset::Low, Preset::Medium, Preset::High, Preset::Ultra, Preset::High, Preset::Medium, Preset::Low, Preset::Ultra, Preset::Low];
    for p in order {
        host.apply_quality(Quality::preset(p), false);
        frame(&mut host, &format!("{p:?}"));
    }
}
