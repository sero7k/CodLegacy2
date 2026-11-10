//! `t7_ssr_check <zone dir>`: the SSR path's pieces, offline, with no map
//! (memory-light): every `deferred_lighting_permuted_ssr(N)` translated and
//! made into a pipeline, the resources it binds beyond the plain set's (the
//! lighting stage binds `gDownsampledScene`, `gMotionVectors`,
//! `gSSAODepthTexture`), and `bloom_downsample_l1_scene` drawn into its two
//! written targets (0 and 2; target 1 left out of the pass) from a flat
//! input, both read back.
use std::collections::BTreeSet;

fn half(b: [u8; 2]) -> f32 {
    let h = u16::from_le_bytes(b);
    let s = if h & 0x8000 != 0 { -1.0 } else { 1.0 };
    let e = i32::from((h >> 10) & 0x1f);
    let m = f32::from(h & 0x3ff);
    s * if e == 0 { m * 2f32.powi(-24) } else { (1.0 + m / 1024.0) * 2f32.powi(e - 15) }
}

fn mean(gpu: &render_h1::gpu::Gpu, tex: &wgpu::Texture) -> [f32; 4] {
    let (w, h) = (tex.width(), tex.height());
    let row = (w * 8).div_ceil(256) * 256;
    let rb = gpu.device.create_buffer(&wgpu::BufferDescriptor { label: None, size: u64::from(row * h), usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ, mapped_at_creation: false });
    let mut enc = gpu.device.create_command_encoder(&Default::default());
    enc.copy_texture_to_buffer(tex.as_image_copy(), wgpu::TexelCopyBufferInfo { buffer: &rb, layout: wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(row), rows_per_image: None } }, tex.size());
    gpu.queue.submit([enc.finish()]);
    rb.slice(..).map_async(wgpu::MapMode::Read, |_| {});
    let _ = gpu.device.poll(wgpu::PollType::wait_indefinitely());
    let d = rb.slice(..).get_mapped_range().to_vec();
    let mut sum = [0f32; 4];
    for y in 0..h {
        for x in 0..w {
            let o = (y * row + x * 8) as usize;
            for (c, s) in sum.iter_mut().enumerate() {
                *s += half([d[o + 2 * c], d[o + 2 * c + 1]]);
            }
        }
    }
    sum.map(|s| s / (w * h) as f32)
}

fn main() {
    let dir = std::path::PathBuf::from(std::env::args().nth(1).expect("zone dir"));
    let gpu = render_h1::gpu::Gpu::headless().expect("gpu");
    let patch = fastfile_t7::shared::zone(&dir, "core_patch").expect("core_patch");
    let shaders = render_t7::compute::code_shaders(&patch.image, &patch.capture);

    // 1. The SSR permutations (`T7_SSR_ALL=1`: all 64; else every 21st,
    // pipelines being slow on a software device).
    let picks: Vec<u32> = if std::env::var("T7_SSR_ALL").is_ok_and(|v| v == "1") { (0..render_t7::lighting::PERMUTATIONS).collect() } else { vec![0, 21, 42, 63] };
    let mut made = 0;
    let mut extra = BTreeSet::new();
    let plain: BTreeSet<String> = picks
        .iter()
        .filter_map(|n| shaders.get(&format!("deferred_lighting_permuted({n})")))
        .filter_map(|cs| render_t7::compute::Program::new_split(&gpu, cs, &[], &[4]).ok())
        .flat_map(|p| p.slots.into_iter().map(|s| s.name))
        .collect();
    for &n in &picks {
        let name = format!("deferred_lighting_permuted_ssr({n})");
        let Some(cs) = shaders.get(&name) else {
            println!("{name}: not in core_patch");
            continue;
        };
        match render_t7::compute::Program::new_split(&gpu, cs, &[], &[4]) {
            Ok(p) => {
                made += 1;
                extra.extend(p.slots.iter().map(|s| s.name.clone()).filter(|s| !plain.contains(s)));
            }
            Err(e) => println!("{name}: {e}"),
        }
    }
    println!("t7 ssr check: {made} of {} SSR permutations made ({picks:?}); resources beyond the plain set: {extra:?}", picks.len());
    let unbound: Vec<&String> = extra.iter().filter(|s| !["gDownsampledScene", "gMotionVectors", "gSSAODepthTexture", "samplerPointClamp"].contains(&s.as_str())).collect();
    println!("t7 ssr check: {} of them not bound by the lighting stage {unbound:?}", unbound.len());

    // 2. bloom_downsample_l1_scene into targets 0 and 2.
    let techs = render_t7::post_pass::code_techs(&patch.image, &patch.capture, &["bloom_downsample_l1_scene"]).expect("tech");
    let t = &techs["bloom_downsample_l1_scene"];
    let f = wgpu::TextureFormat::Rgba16Float;
    let pass = render_t7::post_pass::CodePass::new(&gpu, t, &[f, f, f], &[0x1d]).expect("pass");
    let (sw, sh) = (32u32, 32u32);
    let (w, h) = (sw / 2, sh / 2);
    let src = render_t7::gpu::target(&gpu, "src (0x2d)", sw, sh, f, wgpu::TextureUsages::COPY_DST);
    let px: Vec<u8> = (0..sw * sh).flat_map(|_| [0x00u8, 0x38, 0x00, 0x34, 0x00, 0x30, 0x00, 0x3c]).collect(); // (0.5, 0.25, 0.125, 1)
    gpu.queue.write_texture(src.texture.as_image_copy(), &px, wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(sw * 8), rows_per_image: None }, src.texture.size());
    let t0 = render_t7::gpu::target(&gpu, "0x35", w, h, f, wgpu::TextureUsages::COPY_SRC);
    let t2 = render_t7::gpu::target(&gpu, "gDownsampledScene", w, h, f, wgpu::TextureUsages::COPY_SRC);
    let per_scene = render_t7::gpu::buffer(&gpu, "ps", bytemuck::cast_slice(&render_t7::post_pass::per_scene(w, h)), wgpu::BufferUsages::UNIFORM);
    let post_fx = render_t7::gpu::buffer(&gpu, "pfx", &[0u8; 2016], wgpu::BufferUsages::UNIFORM);
    let vb = render_t7::gpu::buffer(&gpu, "tri", &render_t7::post_pass::triangle(w, h), wgpu::BufferUsages::VERTEX);
    let buffers = render_t7::post_pass::CodeBuffers::new(&gpu);
    let border = gpu.device.features().contains(wgpu::Features::ADDRESS_MODE_CLAMP_TO_BORDER);
    let samplers: Vec<(u16, wgpu::Sampler)> = pass.sampler_states().into_iter().map(|s| (s, gpu.device.create_sampler(&render_t7::sampler::descriptor(s, render_t7::sampler::ANISO_QUALITY, border).expect("sampler")))).collect();
    let scope = gpu.device.push_error_scope(wgpu::ErrorFilter::Validation);
    let groups = pass.bind(&gpu, &per_scene, &post_fx, &buffers, |id| (id == 0x1a).then_some(&src.view), |st| samplers.iter().find(|s| s.0 == st).map(|s| &s.1)).expect("bind");
    let mut enc = gpu.device.create_command_encoder(&Default::default());
    pass.draw(&mut enc, &[&t0.view, &t0.view, &t2.view], &groups, &vb);
    gpu.queue.submit([enc.finish()]);
    let err = pollster::block_on(scope.pop());
    println!("t7 ssr check: bloom_downsample_l1_scene drawn {w}x{h}, validation {:?}", err.map(|e| e.to_string()));
    println!("t7 ssr check: target 0 (bloom) mean {:?}; target 2 (gDownsampledScene) mean {:?} (input 0.5 0.25 0.125 1)", mean(&gpu, &t0.texture), mean(&gpu, &t2.texture));
}
