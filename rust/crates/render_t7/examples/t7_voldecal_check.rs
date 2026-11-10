//! `t7_voldecal_check <map.ff> [w]` (with `BO2MP_T7_VOLUME_DECALS=1`): the
//! G-buffer pass alone (no lighting or post: it fits where the whole frame
//! does not), per volume decal a view looking back along its projection
//! axis (its local x) from 18 units out, drawn with the decals and with
//! them hidden (their instance elements zeroed): the decals' draws (ready
//! or why not), validation, and how many pixels of the diffuse target the
//! decals changed. `T7_VD_PNG=<prefix>`: both diffuse targets per decal.
use std::sync::Arc;

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let zone = std::path::PathBuf::from(a.get(1).expect("map zone"));
    let w: u32 = a.get(2).and_then(|s| s.parse().ok()).unwrap_or(128);
    let h = w;
    let gpu = render_h1::gpu::Gpu::headless().expect("gpu");
    let scene = render_t7::scene::Scene::load(&zone).expect("scene");
    for l in scene.report.iter().filter(|l| l.contains("voldecals")) {
        println!("{l}");
    }
    let mut gb = render_t7::gbuffer::GBuffer::new(gpu.clone(), Arc::new(scene));
    gb.check_bindings_without_images = std::env::var("T7_STAND_IN").is_ok_and(|v| v != "0");
    let targets = render_t7::gbuffer::Targets::new(&gpu, w, h);
    gb.prepare(&targets);
    let sc = gb.scene.clone();
    let is_vd = |d: usize| sc.draws[d].source == u32::MAX - 1;
    let ready: Vec<usize> = gb.ready_draws().into_iter().filter(|&d| is_vd(d)).collect();
    let skipped: Vec<String> = gb.report.skipped_draws.iter().filter(|(d, _)| is_vd(*d)).map(|(d, e)| format!("{}: {e}", sc.materials[sc.draws[*d].material].name)).collect();
    println!("t7 voldecal check: {} box draws ready, skipped {skipped:?}; pipelines failed {:?}", ready.len(), gb.report.pipelines_failed.iter().filter(|f| f.contains("vd/")).collect::<Vec<_>>());
    let elements: Vec<f32> = sc.volume_decals.iter().flatten().copied().collect();
    let zeros = vec![0f32; elements.len()];
    let row = (w * 4).div_ceil(256) * 256;
    let mut draw = |cam: &render_h1::code::Camera, elems: &[f32]| -> (Vec<u8>, Option<String>) {
        gb.write_volume_decals(elems);
        gb.update(&render_t7::lighting_globals::view_consts(cam, w, h, 0.0, 0));
        let scope = gpu.device.push_error_scope(wgpu::ErrorFilter::Validation);
        let mut enc = gpu.device.create_command_encoder(&Default::default());
        gb.record(&mut enc, &targets);
        let tex = &targets.colour[0].texture;
        let rb = gpu.device.create_buffer(&wgpu::BufferDescriptor { label: None, size: u64::from(row * h), usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ, mapped_at_creation: false });
        enc.copy_texture_to_buffer(tex.as_image_copy(), wgpu::TexelCopyBufferInfo { buffer: &rb, layout: wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(row), rows_per_image: None } }, tex.size());
        gpu.queue.submit([enc.finish()]);
        let err = pollster::block_on(scope.pop()).map(|e| e.to_string());
        rb.slice(..).map_async(wgpu::MapMode::Read, |_| {});
        let _ = gpu.device.poll(wgpu::PollType::wait_indefinitely());
        let d = rb.slice(..).get_mapped_range().to_vec();
        (d, err)
    };
    let png_prefix = std::env::var("T7_VD_PNG").ok();
    for (k, e) in sc.volume_decals.iter().enumerate() {
        let c = glam::Vec3::new(e[12], e[13], e[14]);
        let axis = glam::Vec3::new(e[0], e[1], e[2]).normalize();
        let eye = c + axis * 18.0;
        let view = render_h1::code::View { eye, angles: glam::Vec3::new(axis.z.asin().to_degrees(), (-axis.y).atan2(-axis.x).to_degrees(), 0.0), fov_x: 80.0, width: w, height: h, near: 1.0 };
        let cam = render_h1::code::Camera::from(&view);
        let (on, err_on) = draw(&cam, &elements);
        let (off, err_off) = draw(&cam, &zeros);
        let mut changed = 0;
        for y in 0..h {
            for x in 0..w {
                let o = (y * row + x * 4) as usize;
                if on[o..o + 3] != off[o..o + 3] {
                    changed += 1;
                }
            }
        }
        println!("t7 voldecal check: decal {k} at ({:.0}, {:.0}, {:.0}): {changed} of {} pixels changed by the decals; validation {:?} / {:?}", c.x, c.y, c.z, w * h, err_on, err_off);
        if let Some(p) = &png_prefix {
            for (tag, d) in [("on", &on), ("off", &off)] {
                let mut px = Vec::with_capacity((w * h * 3) as usize);
                for y in 0..h {
                    for x in 0..w {
                        let o = (y * row + x * 4) as usize;
                        px.extend_from_slice(&d[o..o + 3]);
                    }
                }
                let f = std::fs::File::create(format!("{p}_{k}_{tag}.png")).expect("png");
                let mut enc = png::Encoder::new(std::io::BufWriter::new(f), w, h);
                enc.set_color(png::ColorType::Rgb);
                enc.write_header().and_then(|mut wr| wr.write_image_data(&px)).expect("png write");
            }
        }
    }
}
