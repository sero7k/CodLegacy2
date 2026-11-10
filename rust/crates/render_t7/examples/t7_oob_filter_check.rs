//! `t7_oob_filter_check <zone dir>`: Black Ops III's out-of-bounds filter
//! (`render_t7::post_oob`, core_common's `generic_filter_out_of_bounds`)
//! drawn on whatever adapter wgpu finds over a test image in the chain's
//! graded scale (a grey ramp, ·2^15 as `bloom_apply_lut` leaves 0x48):
//! reports validation errors and how the pixels moved (the static's
//! darkening, the greying), on two frames apart in time.
/// An f32 as half-float bits (finite, non-negative values below 65504).
fn f16_bits(v: f32) -> u16 {
    if v <= 0.0 {
        return 0;
    }
    let b = v.to_bits();
    let e = ((b >> 23) & 0xff) as i32 - 127 + 15;
    if e <= 0 {
        return 0;
    }
    ((e as u16) << 10) | ((b >> 13) & 0x3ff) as u16
}

fn main() {
    let dir = std::path::PathBuf::from(std::env::args().nth(1).expect("zone dir"));
    let gpu = render_h1::gpu::Gpu::headless().expect("gpu");
    let (w, h) = (320u32, 180u32);
    let format = wgpu::TextureFormat::Rgba16Float;
    let mut f = render_t7::post_oob::OobFilter::new(&gpu, &dir, format).expect("filter");
    let image = render_t7::gpu::target(&gpu, "test 0x48", w, h, format, wgpu::TextureUsages::COPY_SRC | wgpu::TextureUsages::COPY_DST);
    // A ramp: red rises left to right, green top to bottom, blue 0.5.
    let mut px: Vec<u16> = Vec::with_capacity((w * h * 4) as usize);
    for y in 0..h {
        for x in 0..w {
            let c = [x as f32 / w as f32, y as f32 / h as f32, 0.5, 1.0];
            for (i, v) in c.iter().enumerate() {
                let s = if i < 3 { v * 32768.0 } else { *v };
                px.push(f16_bits(s));
            }
        }
    }
    gpu.queue.write_texture(image.texture.as_image_copy(), bytemuck::cast_slice(&px), wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(w * 8), rows_per_image: Some(h) }, image.texture.size());
    let before = render_h1::world::read_rgba16f(&gpu, &image.texture, w, h);
    let scope = gpu.device.push_error_scope(wgpu::ErrorFilter::Validation);
    for frame in 0..2 {
        let mut enc = gpu.device.create_command_encoder(&Default::default());
        f.draw(&gpu, &mut enc, &image).expect("draw");
        gpu.queue.submit([enc.finish()]);
        let after = render_h1::world::read_rgba16f(&gpu, &image.texture, w, h);
        let n = after.len() as f32;
        let finite = after.iter().all(|p| p.iter().all(|v| v.is_finite()));
        let mean = |v: &[[f32; 4]], c: usize| v.iter().map(|p| p[c] / 32768.0).sum::<f32>() / n;
        let changed = after.iter().zip(&before).filter(|(a, b)| (0..3).any(|c| (a[c] - b[c]).abs() > 64.0)).count();
        // How grey: the mean spread between the channels.
        let spread = |v: &[[f32; 4]]| v.iter().map(|p| (p[0].max(p[1]).max(p[2]) - p[0].min(p[1]).min(p[2])) / 32768.0).sum::<f32>() / n;
        println!(
            "frame {frame}: {changed} of {} pixels changed; finite {finite}; mean rgb before ({:.3} {:.3} {:.3}) after ({:.3} {:.3} {:.3}); channel spread {:.3} -> {:.3}",
            after.len(),
            mean(&before, 0),
            mean(&before, 1),
            mean(&before, 2),
            mean(&after, 0),
            mean(&after, 1),
            mean(&after, 2),
            spread(&before),
            spread(&after)
        );
        // (The next frame reads the original again.)
        gpu.queue.write_texture(image.texture.as_image_copy(), bytemuck::cast_slice(&px), wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(w * 8), rows_per_image: Some(h) }, image.texture.size());
        std::thread::sleep(std::time::Duration::from_millis(400));
    }
    match pollster::block_on(scope.pop()) {
        Some(e) => println!("validation error: {e}"),
        None => println!("no validation errors; {} draws", f.drawn),
    }
}
