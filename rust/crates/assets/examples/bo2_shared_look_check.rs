//! bo2mp: the shared look's passes on Black Ops II maps, headless and by
//! numbers (no pictures): `BO2MP_BO2_SHARED_LOOK`, `docs/BO2_SHARED_LOOK.md`.
//!
//! ```text
//! cargo run --release -p assets --example bo2_shared_look_check -- \
//!   <Black Ops III zone folder> <bo2 zone>/mp_hijacked.ff <bo2 zone>/mp_raid.ff ...
//! ```
//!
//! Per map, from up to three spawn points, five frames with the camera
//! turning 1.5 degrees a frame: the map's world triangles into Black Ops
//! III's three sun cascades (`render_t7::bo2_shared::cascades`, the fit the
//! game uses), the view lit by a reduced shading (a mid-grey surface under
//! the map's sun, by N.L and the cascades' shadow, plus a flat ambient; no
//! lightmap, no textures: the game's own branch is `t7_shade`) and Black Ops
//! III's tone curve, then the post passes the game runs
//! (`render_t7::bo2_shared::SharedPost`: SMAA 1x, camera motion blur). One
//! line per view: NaN texels in the lit frame, black pixels in the final
//! one, its mean, how much of the drawn world is in a live shadow, and the
//! milliseconds of each stage (median, wall clock with the device waited
//! on: on a software adapter they are that adapter's). `FAIL` when the
//! frame has NaN, is black, or a pass reports a validation error.
//! `BO2_CHECK_SIZE=WxH` (default 1280x720). `synthetic` in place of a map
//! zone: a ground and a block (no game files needed beyond core_patch.ff).

use std::path::PathBuf;
use std::time::Instant;

use glam::{Mat4, Vec3};
use render_h1::gpu::Gpu;
use render_t7::bo2_shared::{self, PostSettings, SharedPost, Turn};

const SHADOW: u32 = 2048;
const HDR: wgpu::TextureFormat = wgpu::TextureFormat::Rgba16Float;
const FINAL: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;

const CASTER: &str = r#"
@group(0) @binding(0) var<uniform> m: mat4x4<f32>;
@vertex
fn caster(@location(0) p: vec3<f32>, @location(1) n: vec3<f32>) -> @builtin(position) vec4<f32> {
    return m * vec4(p, 1.0);
}
"#;

const LIT: &str = r#"
struct G {
    clip_from_rel: mat4x4<f32>,
    c0: mat4x4<f32>,
    c1: mat4x4<f32>,
    c2: mat4x4<f32>,
    eye: vec4<f32>,
    sun_dir: vec4<f32>,
    sun_col: vec4<f32>,
    texel: vec4<f32>,
}
@group(0) @binding(0) var<uniform> g: G;
@group(0) @binding(1) var shadow: texture_depth_2d_array;
@group(0) @binding(2) var cmp: sampler_comparison;

struct VOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) world: vec3<f32>,
    @location(1) n: vec3<f32>,
}

@vertex
fn vs(@location(0) p: vec3<f32>, @location(1) n: vec3<f32>) -> VOut {
    return VOut(g.clip_from_rel * vec4(p - g.eye.xyz, 1.0), p, n);
}

// The game's partition read (`sun_shadow_partition`): four compare taps,
// the receiver moved toward the sun by a texel and a half.
fn tap(layer: i32, c: vec4<f32>, texel: f32) -> f32 {
    let uv = c.xy * vec2(0.5, -0.5) + vec2(0.5);
    let depth = c.z - (texel * 1.5 + 0.5) / g.texel.w;
    let o = 0.5 / 2048.0;
    var lit = textureSampleCompareLevel(shadow, cmp, uv + vec2(-o, -o), layer, depth);
    lit += textureSampleCompareLevel(shadow, cmp, uv + vec2(o, -o), layer, depth);
    lit += textureSampleCompareLevel(shadow, cmp, uv + vec2(-o, o), layer, depth);
    lit += textureSampleCompareLevel(shadow, cmp, uv + vec2(o, o), layer, depth);
    return lit * 0.25;
}

fn inside(c: vec4<f32>) -> bool {
    return all(abs(c.xy) < vec2(0.96)) && c.z > 0.0 && c.z < 1.0;
}

fn live(p: vec3<f32>) -> f32 {
    let a = g.c0 * vec4(p, 1.0);
    if (inside(a)) {
        return tap(0, a, g.texel.x);
    }
    let b = g.c1 * vec4(p, 1.0);
    if (inside(b)) {
        return tap(1, b, g.texel.y);
    }
    let c = g.c2 * vec4(p, 1.0);
    let edge = max(abs(c.x), abs(c.y));
    if (edge >= 1.0 || c.z <= 0.0 || c.z >= 1.0) {
        return 1.0;
    }
    return mix(tap(2, c, g.texel.z), 1.0, smoothstep(0.8, 1.0, edge));
}

fn tone(c: vec3<f32>, exposure: f32) -> vec3<f32> {
    let x = max(c * exposure, vec3(0.0)) + vec3(0.00873);
    let t = saturate(log2(x) * 0.072703 + 0.598206);
    let q = (((7.712947 * t - 19.311527) * t + 14.275167) * t - 2.490045) * t + 0.878083;
    return saturate(q * t - 0.06691);
}

@fragment
fn fs(v: VOut, @builtin(front_facing) front: bool) -> @location(0) vec4<f32> {
    var n = v.n;
    if (dot(n, n) < 1e-8) {
        n = vec3(0.0, 0.0, 1.0);
    }
    n = normalize(n);
    let to_eye = g.eye.xyz - v.world;
    if (dot(n, to_eye) < 0.0) {
        n = -n;
    }
    let ndl = saturate(dot(n, g.sun_dir.xyz));
    var vis = 1.0;
    if (ndl > 0.0) {
        vis = live(v.world + n * 1.5);
    }
    let light = vec3(g.sun_col.w) + g.sun_col.rgb * ndl * vis;
    return vec4(tone(vec3(0.35) * light, g.sun_dir.w), vis);
}
"#;

fn tone(c: f32, exposure: f32) -> f32 {
    let x = (c * exposure).max(0.0) + 0.00873;
    let t = (x.log2() * 0.072703 + 0.598206).clamp(0.0, 1.0);
    let p = (((7.712947 * t - 19.311527) * t + 14.275167) * t - 2.490045) * t + 0.878083;
    (p * t - 0.06691).clamp(0.0, 1.0)
}

/// The exposure at which a mid-grey in the sun draws 0.65 (as the look's
/// own check aims).
fn exposure(lit: f32) -> f32 {
    let (mut lo, mut hi) = (0.01f32, 100.0f32);
    for _ in 0..50 {
        let mid = (lo * hi).sqrt();
        if tone(0.35 * lit, mid) < 0.65 { lo = mid } else { hi = mid }
    }
    (lo * hi).sqrt()
}

fn f16(h: u16) -> f32 {
    let s = if h & 0x8000 != 0 { -1.0 } else { 1.0 };
    let e = ((h >> 10) & 0x1f) as i32;
    let f = f32::from(h & 0x3ff);
    if e == 31 {
        return if f == 0.0 { s * f32::INFINITY } else { f32::NAN };
    }
    s * if e == 0 { f / 1024.0 * 2f32.powi(-14) } else { (1.0 + f / 1024.0) * 2f32.powi(e - 15) }
}

fn read(gpu: &Gpu, t: &wgpu::Texture, bpp: u32) -> Vec<u8> {
    let (w, h) = (t.width(), t.height());
    let row = (w * bpp).div_ceil(256) * 256;
    let buf = gpu.device.create_buffer(&wgpu::BufferDescriptor { label: None, size: u64::from(row * h), usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ, mapped_at_creation: false });
    let mut enc = gpu.device.create_command_encoder(&Default::default());
    enc.copy_texture_to_buffer(t.as_image_copy(), wgpu::TexelCopyBufferInfo { buffer: &buf, layout: wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(row), rows_per_image: None } }, t.size());
    gpu.queue.submit([enc.finish()]);
    buf.slice(..).map_async(wgpu::MapMode::Read, |_| {});
    let _ = gpu.device.poll(wgpu::PollType::wait_indefinitely());
    let m = buf.slice(..).get_mapped_range();
    let mut out = Vec::with_capacity((w * h * bpp) as usize);
    for y in 0..h {
        let o = (y * row) as usize;
        out.extend_from_slice(&m[o..o + (w * bpp) as usize]);
    }
    out
}

fn texture(gpu: &Gpu, label: &str, w: u32, h: u32, layers: u32, format: wgpu::TextureFormat, usage: wgpu::TextureUsages) -> wgpu::Texture {
    gpu.device.create_texture(&wgpu::TextureDescriptor {
        label: Some(label),
        size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: layers },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage,
        view_formats: &[],
    })
}

fn median(v: &mut [f32]) -> f32 {
    v.sort_by(f32::total_cmp);
    v.get(v.len() / 2).copied().unwrap_or(0.0)
}

fn wait(gpu: &Gpu, enc: wgpu::CommandEncoder) {
    gpu.queue.submit([enc.finish()]);
    let _ = gpu.device.poll(wgpu::PollType::wait_indefinitely());
}

/// `synthetic` in place of a map: a ground of 4096 units and a block of
/// 256 x 256 x 512 beside the view under a high sun (the passes'
/// plumbing, where no Black Ops II map is at hand; part of the ground is
/// then in the block's shadow).
fn synthetic() -> assets::LookScene {
    let mut positions = Vec::new();
    let mut normals = Vec::new();
    let mut indices = Vec::new();
    let mut quad = |c: [[f32; 3]; 4], n: [f32; 3]| {
        let b = positions.len() as u32;
        positions.extend_from_slice(&c);
        normals.extend_from_slice(&[n; 4]);
        indices.extend_from_slice(&[b, b + 1, b + 2, b, b + 2, b + 3]);
    };
    quad([[-2048.0, -2048.0, 0.0], [2048.0, -2048.0, 0.0], [2048.0, 2048.0, 0.0], [-2048.0, 2048.0, 0.0]], [0.0, 0.0, 1.0]);
    let (x0, x1, y0, y1, z1) = (300.0f32, 556.0f32, -128.0f32, 128.0f32, 512.0f32);
    quad([[x0, y0, z1], [x1, y0, z1], [x1, y1, z1], [x0, y1, z1]], [0.0, 0.0, 1.0]);
    quad([[x0, y0, 0.0], [x1, y0, 0.0], [x1, y0, z1], [x0, y0, z1]], [0.0, -1.0, 0.0]);
    quad([[x0, y1, 0.0], [x1, y1, 0.0], [x1, y1, z1], [x0, y1, z1]], [0.0, 1.0, 0.0]);
    quad([[x0, y0, 0.0], [x0, y1, 0.0], [x0, y1, z1], [x0, y0, z1]], [-1.0, 0.0, 0.0]);
    quad([[x1, y0, 0.0], [x1, y1, 0.0], [x1, y1, z1], [x1, y0, z1]], [1.0, 0.0, 0.0]);
    let sun = Vec3::new(0.5, 0.3, 0.81).normalize();
    assets::LookScene {
        positions,
        normals,
        indices,
        sun_dir: sun.to_array(),
        sun_colour: [2.2, 2.0, 1.8],
        spawns: vec![([0.0, 0.0, 0.0], 0.0), ([0.0, -600.0, 0.0], 45.0), ([-400.0, 300.0, 0.0], -20.0)],
        min: [-2048.0, -2048.0, 0.0],
        max: [2048.0, 2048.0, 512.0],
        report: Vec::new(),
    }
}

fn main() {
    let mut args = std::env::args().skip(1);
    let zone_dir = PathBuf::from(args.next().expect("Black Ops III's zone folder (with core_patch.ff), then Black Ops II map zones"));
    let maps: Vec<PathBuf> = args.map(PathBuf::from).collect();
    let (w, h) = std::env::var("BO2_CHECK_SIZE")
        .ok()
        .and_then(|s| s.split_once('x').and_then(|(a, b)| Some((a.parse().ok()?, b.parse().ok()?))))
        .unwrap_or((1280u32, 720u32));
    let gpu = Gpu::headless().expect("no device");
    let t0 = Instant::now();
    let mut post = SharedPost::new(&gpu, &zone_dir, PostSettings { aa: true, blur_scale: 1.0, blur_quality: 1 }).expect("the post passes");
    for l in &post.report {
        println!("{l}");
    }
    println!("post passes made in {:.1}s", t0.elapsed().as_secs_f32());
    let dev = &gpu.device;
    let caster_module = dev.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some("caster"), source: wgpu::ShaderSource::Wgsl(CASTER.into()) });
    let lit_module = dev.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some("lit"), source: wgpu::ShaderSource::Wgsl(LIT.into()) });
    let vertex_layout = wgpu::VertexBufferLayout {
        array_stride: 24,
        step_mode: wgpu::VertexStepMode::Vertex,
        attributes: &[wgpu::VertexAttribute { format: wgpu::VertexFormat::Float32x3, offset: 0, shader_location: 0 }, wgpu::VertexAttribute { format: wgpu::VertexFormat::Float32x3, offset: 12, shader_location: 1 }],
    };
    let caster = dev.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("caster"),
        layout: None,
        vertex: wgpu::VertexState { module: &caster_module, entry_point: Some("caster"), compilation_options: Default::default(), buffers: std::slice::from_ref(&vertex_layout) },
        primitive: wgpu::PrimitiveState { cull_mode: None, ..Default::default() },
        // (As the game's caster pipeline: constant 2, slope 2.)
        depth_stencil: Some(wgpu::DepthStencilState {
            format: wgpu::TextureFormat::Depth32Float,
            depth_write_enabled: Some(true),
            depth_compare: Some(wgpu::CompareFunction::LessEqual),
            stencil: Default::default(),
            bias: wgpu::DepthBiasState { constant: 2, slope_scale: 2.0, clamp: 0.0 },
        }),
        multisample: Default::default(),
        fragment: None,
        multiview_mask: None,
        cache: None,
    });
    let lit = dev.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("lit"),
        layout: None,
        vertex: wgpu::VertexState { module: &lit_module, entry_point: Some("vs"), compilation_options: Default::default(), buffers: std::slice::from_ref(&vertex_layout) },
        primitive: wgpu::PrimitiveState { cull_mode: None, ..Default::default() },
        depth_stencil: Some(wgpu::DepthStencilState { format: wgpu::TextureFormat::Depth32Float, depth_write_enabled: Some(true), depth_compare: Some(wgpu::CompareFunction::Greater), stencil: Default::default(), bias: Default::default() }),
        multisample: Default::default(),
        fragment: Some(wgpu::FragmentState { module: &lit_module, entry_point: Some("fs"), compilation_options: Default::default(), targets: &[Some(wgpu::ColorTargetState { format: HDR, blend: None, write_mask: wgpu::ColorWrites::ALL })] }),
        multiview_mask: None,
        cache: None,
    });
    let shadow = texture(&gpu, "cascades", SHADOW, SHADOW, 3, wgpu::TextureFormat::Depth32Float, wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING);
    let layers: Vec<wgpu::TextureView> = (0..3)
        .map(|k| shadow.create_view(&wgpu::TextureViewDescriptor { dimension: Some(wgpu::TextureViewDimension::D2), base_array_layer: k, array_layer_count: Some(1), ..Default::default() }))
        .collect();
    let shadow_array = shadow.create_view(&wgpu::TextureViewDescriptor { dimension: Some(wgpu::TextureViewDimension::D2Array), ..Default::default() });
    let cmp = dev.create_sampler(&wgpu::SamplerDescriptor { mag_filter: wgpu::FilterMode::Linear, min_filter: wgpu::FilterMode::Linear, compare: Some(wgpu::CompareFunction::LessEqual), ..Default::default() });
    let hdr = texture(&gpu, "lit", w, h, 1, HDR, wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_SRC);
    let hdr_view = hdr.create_view(&Default::default());
    let depth = texture(&gpu, "depth", w, h, 1, wgpu::TextureFormat::Depth32Float, wgpu::TextureUsages::RENDER_ATTACHMENT);
    let depth_view = depth.create_view(&Default::default());
    let fin = texture(&gpu, "final", w, h, 1, FINAL, wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC);
    let fin_view = fin.create_view(&Default::default());
    let cascade_bufs: Vec<wgpu::Buffer> = (0..3).map(|_| dev.create_buffer(&wgpu::BufferDescriptor { label: Some("cascade"), size: 64, usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST, mapped_at_creation: false })).collect();
    let caster_groups: Vec<wgpu::BindGroup> = cascade_bufs
        .iter()
        .map(|b| dev.create_bind_group(&wgpu::BindGroupDescriptor { label: None, layout: &caster.get_bind_group_layout(0), entries: &[wgpu::BindGroupEntry { binding: 0, resource: b.as_entire_binding() }] }))
        .collect();
    let globals = dev.create_buffer(&wgpu::BufferDescriptor { label: Some("globals"), size: 64 * 4 + 16 * 4, usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST, mapped_at_creation: false });
    let lit_group = dev.create_bind_group(&wgpu::BindGroupDescriptor {
        label: None,
        layout: &lit.get_bind_group_layout(0),
        entries: &[
            wgpu::BindGroupEntry { binding: 0, resource: globals.as_entire_binding() },
            wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&shadow_array) },
            wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::Sampler(&cmp) },
        ],
    });
    let mut failures = 0;
    for map in &maps {
        let name = map.file_stem().map_or_else(|| map.display().to_string(), |s| s.to_string_lossy().into_owned());
        let t = Instant::now();
        let scene = match if name == "synthetic" { Ok(synthetic()) } else { assets::bo2_look_scene(map) } {
            Ok(s) => s,
            Err(e) => {
                println!("{name}: FAIL not read ({e})");
                failures += 1;
                continue;
            }
        };
        let load_s = t.elapsed().as_secs_f32();
        use wgpu::util::DeviceExt;
        let verts: Vec<f32> = scene.positions.iter().zip(scene.normals.iter().chain(std::iter::repeat(&[0.0, 0.0, 1.0]))).flat_map(|(p, n)| [p[0], p[1], p[2], n[0], n[1], n[2]]).collect();
        let vb = dev.create_buffer_init(&wgpu::util::BufferInitDescriptor { label: Some("world"), contents: bytemuck::cast_slice(&verts), usage: wgpu::BufferUsages::VERTEX });
        let indices: Vec<u32> = scene.indices.iter().copied().filter(|&i| (i as usize) < scene.positions.len()).collect();
        let ib = dev.create_buffer_init(&wgpu::util::BufferInitDescriptor { label: Some("world"), contents: bytemuck::cast_slice(&indices), usage: wgpu::BufferUsages::INDEX });
        let n_idx = (indices.len() / 3 * 3) as u32;
        let sun = Vec3::from(scene.sun_dir).normalize_or(Vec3::Z);
        let sun_col = Vec3::from(scene.sun_colour);
        let sun_lum = sun_col.dot(Vec3::new(0.2126, 0.7152, 0.0722)).max(0.05);
        let ambient = sun_lum * 0.3;
        let e = exposure(sun_lum + ambient);
        println!(
            "{name}: read in {load_s:.1}s: {} vertices, {} triangles, sun toward ({:.2}, {:.2}, {:.2}) colour ({:.2}, {:.2}, {:.2}), {} spawns; exposure x{e:.2}",
            scene.positions.len(),
            n_idx / 3,
            sun.x,
            sun.y,
            sun.z,
            sun_col.x,
            sun_col.y,
            sun_col.z,
            scene.spawns.len()
        );
        let n = scene.spawns.len();
        let picks: Vec<([f32; 3], f32)> = if n == 0 {
            vec![([(scene.min[0] + scene.max[0]) * 0.5, (scene.min[1] + scene.max[1]) * 0.5, (scene.min[2] + scene.max[2]) * 0.5], 0.0)]
        } else {
            [0, n / 3, 2 * n / 3].iter().map(|&k| scene.spawns[k.min(n - 1)]).collect()
        };
        for (vi, (origin, yaw0)) in picks.iter().enumerate() {
            let eye = Vec3::new(origin[0], origin[1], origin[2] + 60.0);
            let mut times: [Vec<f32>; 3] = Default::default();
            let mut prev: Option<render_h1::code::Camera> = None;
            let mut line = String::new();
            let mut bad = false;
            for frame in 0..5 {
                let view = render_h1::code::View { eye, angles: Vec3::new(5.0, yaw0 + 1.5 * frame as f32, 0.0), fov_x: 80.0, width: w, height: h, near: 4.0 };
                let cam = render_h1::code::Camera::from(&view);
                let c = bo2_shared::cascades(sun, &cam, SHADOW, bo2_shared::SHADOW_DISTANCE);
                let scope = dev.push_error_scope(wgpu::ErrorFilter::Validation);
                // The cascades.
                for k in 0..3 {
                    gpu.queue.write_buffer(&cascade_bufs[k], 0, bytemuck::cast_slice(&c.clip_from_world[k].to_cols_array()));
                }
                let ts = Instant::now();
                let mut enc = dev.create_command_encoder(&Default::default());
                for k in 0..3 {
                    let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                        label: Some("cascade"),
                        color_attachments: &[],
                        depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment { view: &layers[k], depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(1.0), store: wgpu::StoreOp::Store }), stencil_ops: None }),
                        timestamp_writes: None,
                        occlusion_query_set: None,
                        multiview_mask: None,
                    });
                    pass.set_pipeline(&caster);
                    pass.set_bind_group(0, &caster_groups[k], &[]);
                    pass.set_vertex_buffer(0, vb.slice(..));
                    pass.set_index_buffer(ib.slice(..), wgpu::IndexFormat::Uint32);
                    pass.draw_indexed(0..n_idx, 0, 0..1);
                }
                wait(&gpu, enc);
                let t_shadow = ts.elapsed().as_secs_f32() * 1000.0;
                // The view.
                let mut g = Vec::with_capacity(80);
                g.extend_from_slice(&cam.view_projection.to_cols_array());
                for k in 0..3 {
                    g.extend_from_slice(&c.clip_from_world[k].to_cols_array());
                }
                g.extend_from_slice(&[eye.x, eye.y, eye.z, 0.0, sun.x, sun.y, sun.z, e, sun_col.x, sun_col.y, sun_col.z, ambient, c.texel[0], c.texel[1], c.texel[2], bo2_shared::DEPTH_RANGE]);
                gpu.queue.write_buffer(&globals, 0, bytemuck::cast_slice(&g));
                let tl = Instant::now();
                let mut enc = dev.create_command_encoder(&Default::default());
                {
                    let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                        label: Some("lit"),
                        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                            view: &hdr_view,
                            depth_slice: None,
                            resolve_target: None,
                            ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color { r: 0.45, g: 0.55, b: 0.7, a: 2.0 }), store: wgpu::StoreOp::Store },
                        })],
                        depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment { view: &depth_view, depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(0.0), store: wgpu::StoreOp::Store }), stencil_ops: None }),
                        timestamp_writes: None,
                        occlusion_query_set: None,
                        multiview_mask: None,
                    });
                    pass.set_pipeline(&lit);
                    pass.set_bind_group(0, &lit_group, &[]);
                    pass.set_vertex_buffer(0, vb.slice(..));
                    pass.set_index_buffer(ib.slice(..), wgpu::IndexFormat::Uint32);
                    pass.draw_indexed(0..n_idx, 0, 0..1);
                }
                wait(&gpu, enc);
                let t_lit = tl.elapsed().as_secs_f32() * 1000.0;
                // The post passes, with the turn since the last frame.
                let turn = prev.as_ref().map(|p| Turn::new(cam.view_projection * cam.rotation.inverse(), cam.rotation, p.view_projection * p.rotation.inverse(), p.rotation));
                let tp = Instant::now();
                let mut enc = dev.create_command_encoder(&Default::default());
                if let Err(err) = post.record(&mut enc, &hdr_view, false, &fin_view, FINAL, w, h, turn) {
                    line = format!("post: {err}");
                    bad = true;
                }
                wait(&gpu, enc);
                let t_post = tp.elapsed().as_secs_f32() * 1000.0;
                if let Some(err) = pollster::block_on(scope.pop()) {
                    line = format!("validation: {err}");
                    bad = true;
                }
                if frame > 0 {
                    times[0].push(t_shadow);
                    times[1].push(t_lit);
                    times[2].push(t_post);
                }
                prev = Some(cam);
            }
            // The last frame, by numbers.
            let lit_px = read(&gpu, &hdr, 8);
            let (mut nan, mut drawn, mut shadowed) = (0usize, 0usize, 0usize);
            for px in lit_px.chunks_exact(8) {
                let v: [f32; 4] = std::array::from_fn(|k| f16(u16::from_le_bytes([px[2 * k], px[2 * k + 1]])));
                if v.iter().any(|x| !x.is_finite()) {
                    nan += 1;
                } else if v[3] < 1.5 {
                    drawn += 1;
                    if v[3] < 0.5 {
                        shadowed += 1;
                    }
                }
            }
            let fin_px = read(&gpu, &fin, 4);
            let total = (w * h) as usize;
            let black = fin_px.chunks_exact(4).filter(|p| p[0].max(p[1]).max(p[2]) < 3).count();
            let mean = fin_px.chunks_exact(4).map(|p| 0.2126 * f32::from(p[0]) + 0.7152 * f32::from(p[1]) + 0.0722 * f32::from(p[2])).sum::<f32>() / total as f32 / 255.0;
            let fail = bad || nan > 0 || black * 2 > total || mean < 0.02;
            if fail {
                failures += 1;
            }
            let [mut a, mut b, mut cc] = times;
            println!(
                "{name} view {vi} ({:.0} {:.0} {:.0} yaw {:.0}): {} nan {nan}, black {:.1}%, mean {mean:.3}, world drawn {:.1}% of the frame, {:.1}% of it in a live shadow; ms (median of 4): cascades {:.1}, lit {:.1}, SMAA + blur {:.1}{}",
                eye.x,
                eye.y,
                eye.z,
                yaw0,
                if fail { "FAIL" } else { "OK" },
                100.0 * black as f32 / total as f32,
                100.0 * drawn as f32 / total as f32,
                100.0 * shadowed as f32 / drawn.max(1) as f32,
                median(&mut a),
                median(&mut b),
                median(&mut cc),
                if line.is_empty() { String::new() } else { format!(" ({line})") }
            );
        }
    }
    println!("{} map(s), {failures} failing view(s)", maps.len());
    if failures > 0 {
        std::process::exit(1);
    }
}
