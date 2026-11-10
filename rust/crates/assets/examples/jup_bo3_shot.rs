//! bo2mp: a Modern Warfare III (2023) map converted as the game now draws it
//! by default (the converted world in Black Ops III's look, MW3's own sky,
//! stand-in pictures from Modern Warfare Remastered / Modern Warfare 2:
//! `assets::jup_preview`), rendered headless (lavapipe works) to PNGs.
//!
//! ```text
//! JUP_OODLE_LIB=<libooz.so> JUP_PACK_DIR=<packs> \
//!   cargo run -p assets --example jup_bo3_shot -- --zone <install>/zone/mp_jup_highrise.ff --out <dir>
//!   [--size 1280x720] [--cam name:x,y,z,pitch,yaw[;...]] [--no-sub]
//! ```
//!
//! The shading is the BO3 look's branch for a converted MW3 map, reduced
//! (`render_gpu`'s `t7_shade` with `t7_bo2` and CoD4 pages, `bo3_look`):
//! albedo times (the ambient page's light, a quarter brighter facing up,
//! plus the sun, hue x 2.6, by N.L and its shadow), the BO3 tone curve at
//! exposure 1, MW3's fog over it and the converted sky cube behind. Not the
//! game's own pass: no rebaked ambient occlusion, no reflections, no BO2
//! grade or bloom; the sun's shadow is a shadow map round the view, not the
//! baked sun map. It shows what is drawn with what picture, in about the
//! light the game gives it. `--no-sub` sets `BO2MP_JUP_SUB=0` (MW3's own
//! pictures only) for a before/after.

use std::path::PathBuf;

use glam::{Mat4, Vec3, Vec4};
use wgpu::util::DeviceExt;

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Vertex {
    pos: [f32; 3],
    normal: [f32; 3],
    uv: [f32; 2],
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Instance {
    a0: [f32; 4],
    a1: [f32; 4],
    a2: [f32; 4],
    origin: [f32; 4],
}

const IDENTITY: Instance = Instance { a0: [1.0, 0.0, 0.0, 1.0], a1: [0.0, 1.0, 0.0, 0.0], a2: [0.0, 0.0, 1.0, 0.0], origin: [0.0; 4] };

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Globals {
    clip_from_world: [[f32; 4]; 4],
    sun_clip: [[f32; 4]; 4],
    world_from_clip: [[f32; 4]; 4],
    eye: [f32; 4],
    sun_dir: [f32; 4],
    sun_col: [f32; 4],
    amb: [f32; 4],
    /// start, halfway, max opacity, exposure
    fog: [f32; 4],
    fog_col: [f32; 4],
    /// sun fog colour, its scale
    sun_fog: [f32; 4],
    /// cos(begin), cos(end)
    sun_fog_ang: [f32; 4],
}

struct Draw {
    material: usize,
    first: u32,
    count: u32,
    base: i32,
    inst: (u32, u32),
}

const SHADER: &str = r#"
struct Globals {
    clip_from_world: mat4x4<f32>,
    sun_clip: mat4x4<f32>,
    world_from_clip: mat4x4<f32>,
    eye: vec4<f32>,
    sun_dir: vec4<f32>,
    sun_col: vec4<f32>,
    amb: vec4<f32>,
    fog: vec4<f32>,
    fog_col: vec4<f32>,
    sun_fog: vec4<f32>,
    sun_fog_ang: vec4<f32>,
};
@group(0) @binding(0) var<uniform> g: Globals;
@group(0) @binding(1) var shadow_map: texture_depth_2d;
@group(0) @binding(2) var shadow_cmp: sampler_comparison;
@group(0) @binding(3) var sky: texture_cube<f32>;
@group(0) @binding(4) var sky_samp: sampler;
@group(1) @binding(0) var albedo: texture_2d<f32>;
@group(1) @binding(1) var samp: sampler;
@group(1) @binding(2) var<uniform> mat: vec4<f32>;

struct VIn {
    @location(0) pos: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) uv: vec2<f32>,
    @location(3) a0: vec4<f32>,
    @location(4) a1: vec4<f32>,
    @location(5) a2: vec4<f32>,
    @location(6) origin: vec4<f32>,
};
struct VOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) world: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) uv: vec2<f32>,
};

fn place(v: VIn) -> VOut {
    var o: VOut;
    let s = v.a0.w;
    let p = v.origin.xyz + s * (v.pos.x * v.a0.xyz + v.pos.y * v.a1.xyz + v.pos.z * v.a2.xyz);
    o.world = p;
    o.normal = v.normal.x * v.a0.xyz + v.normal.y * v.a1.xyz + v.normal.z * v.a2.xyz;
    o.uv = v.uv;
    o.clip = g.clip_from_world * vec4(p, 1.0);
    return o;
}

@vertex fn vs_main(v: VIn) -> VOut { return place(v); }

@vertex fn vs_shadow(v: VIn) -> VOut {
    var o = place(v);
    o.clip = g.sun_clip * vec4(o.world, 1.0);
    return o;
}

@fragment fn fs_shadow(i: VOut) {
    if (mat.x > 0.5 && textureSample(albedo, samp, i.uv).a < 0.5) { discard; }
}

fn t7_tone(c: vec3<f32>, exposure: f32) -> vec3<f32> {
    let x = max(c * exposure, vec3(0.0)) + vec3(0.00873);
    let t = saturate(log2(x) * 0.072703 + 0.598206);
    let p = (((7.712947 * t - 19.311527) * t + 14.275167) * t - 2.490045) * t + 0.878083;
    return saturate(p * t - 0.06691);
}

fn shadow(p: vec3<f32>, n: vec3<f32>) -> f32 {
    let q = g.sun_clip * vec4(p + n * 2.0, 1.0);
    let uv = vec2(q.x * 0.5 + 0.5, 0.5 - q.y * 0.5);
    if (uv.x < 0.0 || uv.y < 0.0 || uv.x > 1.0 || uv.y > 1.0 || q.z > 1.0) { return 1.0; }
    let texel = 1.0 / f32(textureDimensions(shadow_map).x);
    var lit = 0.0;
    for (var y = -1; y <= 1; y++) {
        for (var x = -1; x <= 1; x++) {
            lit += textureSampleCompareLevel(shadow_map, shadow_cmp, uv + vec2(f32(x), f32(y)) * texel, q.z - 0.0008);
        }
    }
    return lit / 9.0;
}

fn fogged(c: vec3<f32>, p: vec3<f32>) -> vec3<f32> {
    if (g.fog.z <= 0.0 || g.fog.y <= 0.0) { return c; }
    let v = p - g.eye.xyz;
    let d = length(v);
    let f = g.fog.z * (1.0 - exp2(-max(d - g.fog.x, 0.0) / g.fog.y));
    let cosv = dot(v / max(d, 0.001), g.sun_dir.xyz);
    let k = g.sun_fog.w * smoothstep(g.sun_fog_ang.y, g.sun_fog_ang.x, cosv);
    let col = mix(g.fog_col.rgb, g.sun_fog.rgb, saturate(k));
    return mix(c, col, saturate(f));
}

@fragment fn fs_main(i: VOut, @builtin(front_facing) front: bool) -> @location(0) vec4<f32> {
    let t = textureSample(albedo, samp, i.uv);
    if (mat.x > 0.5 && t.a < 0.5) { discard; }
    var n = normalize(i.normal);
    if (dot(n, g.eye.xyz - i.world) < 0.0) { n = -n; }
    let ndl = max(dot(n, g.sun_dir.xyz), 0.0);
    var vis = 0.0;
    if (ndl > 0.0) { vis = shadow(i.world, n); }
    let amb = g.amb.rgb * (0.8 + 0.25 * n.z);
    // (Albedo past 0.5 rolls off at a quarter, as `t7_shade` on an MW3 map.)
    let base = min(t.rgb, vec3(0.5) + max(t.rgb - vec3(0.5), vec3(0.0)) * 0.25);
    let c = base * mat.y * (amb + g.sun_col.rgb * (vis * ndl));
    return vec4(fogged(t7_tone(c, g.fog.w), i.world), 1.0);
}

struct SkyOut { @builtin(position) clip: vec4<f32>, @location(0) ndc: vec2<f32> };
@vertex fn vs_sky(@builtin(vertex_index) k: u32) -> SkyOut {
    let xy = vec2(f32((k << 1u) & 2u), f32(k & 2u)) * 2.0 - 1.0;
    var o: SkyOut;
    o.clip = vec4(xy, 0.0, 1.0);
    o.ndc = xy;
    return o;
}
@fragment fn fs_sky(i: SkyOut) -> @location(0) vec4<f32> {
    let far = g.world_from_clip * vec4(i.ndc, 1.0, 1.0);
    let d = normalize(far.xyz / far.w - g.eye.xyz);
    // (The cube's faces are +x -x +y -y +z -z with z up; WGSL's cube is y up.)
    let c = textureSample(sky, sky_samp, vec3(d.x, d.z, d.y)).rgb;
    return vec4(c, 1.0);
}
"#;

fn mat4(m: Mat4) -> [[f32; 4]; 4] {
    m.to_cols_array_2d()
}

/// RGBA8 with its mip chain (box filtered), at most `max` a side.
fn chain(mut w: u32, mut h: u32, mut px: Vec<u8>, max: u32) -> Vec<(u32, u32, Vec<u8>)> {
    let half = |w: u32, h: u32, px: &[u8]| {
        let (nw, nh) = ((w / 2).max(1), (h / 2).max(1));
        let mut n = vec![0u8; (nw * nh * 4) as usize];
        for y in 0..nh {
            for x in 0..nw {
                for c in 0..4 {
                    let mut s = 0u32;
                    for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                        let (sx, sy) = ((2 * x + dx).min(w - 1), (2 * y + dy).min(h - 1));
                        s += u32::from(px[((sy * w + sx) * 4 + c) as usize]);
                    }
                    n[((y * nw + x) * 4 + c) as usize] = (s / 4) as u8;
                }
            }
        }
        (nw, nh, n)
    };
    while w > max || h > max {
        (w, h, px) = half(w, h, &px);
    }
    let mut out = vec![(w, h, px)];
    while out.last().is_some_and(|(w, h, _)| *w > 1 || *h > 1) {
        let (w, h, p) = out.last().unwrap();
        let n = half(*w, *h, p);
        out.push(n);
    }
    out
}

fn upload(gpu: &render_h1::gpu::Gpu, levels: &[(u32, u32, Vec<u8>)], label: &str) -> wgpu::TextureView {
    let (w, h, _) = levels[0];
    let tex = gpu.device.create_texture(&wgpu::TextureDescriptor {
        label: Some(label),
        size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
        mip_level_count: levels.len() as u32,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8UnormSrgb,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    for (l, (lw, lh, px)) in levels.iter().enumerate() {
        gpu.queue.write_texture(
            wgpu::TexelCopyTextureInfo { texture: &tex, mip_level: l as u32, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
            px,
            wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(lw * 4), rows_per_image: Some(*lh) },
            wgpu::Extent3d { width: *lw, height: *lh, depth_or_array_layers: 1 },
        );
    }
    tex.create_view(&wgpu::TextureViewDescriptor::default())
}

/// A worldspawn vector.
fn ws_vec(ents: &str, key: &str) -> Option<Vec<f32>> {
    let block = ents.split('}').next()?;
    let line = block.lines().find(|l| l.trim().starts_with(&format!("\"{key}\"")))?;
    Some(line.trim().split('"').nth(3)?.split_whitespace().filter_map(|x| x.parse().ok()).collect())
}

/// Spawn origins (`mp_*_spawn*` entities).
fn spawns(ents: &str) -> Vec<[f32; 3]> {
    let mut out = Vec::new();
    for block in ents.split('}') {
        if !block.contains("_spawn") {
            continue;
        }
        let get = |k: &str| block.lines().find(|l| l.trim().starts_with(&format!("\"{k}\""))).and_then(|l| l.trim().split('"').nth(3).map(str::to_owned));
        if get("classname").is_some_and(|c| c.starts_with("mp_") && c.contains("spawn"))
            && let Some(o) = get("origin")
        {
            let v: Vec<f32> = o.split_whitespace().filter_map(|x| x.parse().ok()).collect();
            if v.len() == 3 {
                out.push([v[0], v[1], v[2]]);
            }
        }
    }
    out
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut zone = None;
    let mut out = PathBuf::from("jup_bo3_shot");
    let (mut w, mut h) = (1280u32, 720u32);
    let mut cams: Vec<(String, [f32; 5])> = Vec::new();
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--zone" => zone = it.next().map(PathBuf::from),
            "--out" => out = it.next().map(PathBuf::from).unwrap_or(out),
            "--size" => {
                if let Some((a, b)) = it.next().and_then(|s| s.split_once('x')) {
                    w = a.parse().unwrap_or(w);
                    h = b.parse().unwrap_or(h);
                }
            }
            "--cam" => {
                for c in it.next().map(String::as_str).unwrap_or_default().split(';') {
                    let Some((name, v)) = c.split_once(':') else { continue };
                    let v: Vec<f32> = v.split(',').filter_map(|x| x.trim().parse().ok()).collect();
                    if v.len() == 5 {
                        cams.push((name.to_owned(), [v[0], v[1], v[2], v[3], v[4]]));
                    }
                }
            }
            "--no-sub" => {
                // SAFETY: set before any thread reads the environment.
                unsafe { std::env::set_var("BO2MP_JUP_SUB", "0") };
            }
            other => eprintln!("unknown argument {other}"),
        }
    }
    let Some(zone) = zone else {
        eprintln!("usage: jup_bo3_shot --zone <zone.ff> --out <dir> [--size WxH] [--cam name:x,y,z,pitch,yaw;...] [--no-sub]");
        std::process::exit(2);
    };
    std::fs::create_dir_all(&out).expect("out dir");
    let started = std::time::Instant::now();
    let preview = match assets::jup_preview(&zone) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("not converted: {e}");
            std::process::exit(1);
        }
    };
    let mut log = String::new();
    for l in &preview.report {
        println!("{l}");
        log.push_str(l);
        log.push('\n');
    }
    println!("converted in {:.1}s", started.elapsed().as_secs_f32());
    let map = preview.map;
    let _ = std::fs::write(out.join("entities.txt"), &map.entities);

    // Geometry: the world (identity instance) and each model's surfaces with
    // their placements.
    let mut verts: Vec<Vertex> = Vec::new();
    let mut idx: Vec<u32> = Vec::new();
    let mut insts: Vec<Instance> = vec![IDENTITY];
    let mut draws: Vec<Draw> = Vec::new();
    for s in &map.surfaces {
        let Some(m) = s.material else { continue };
        let base = verts.len() as u32;
        for v in &map.vertices[s.first_vertex as usize..(s.first_vertex + s.vertex_count) as usize] {
            verts.push(Vertex { pos: v.xyz, normal: v.normal, uv: v.uv });
        }
        let first = idx.len() as u32;
        idx.extend(map.indices[s.base_index as usize..s.base_index as usize + 3 * usize::from(s.tri_count)].iter().map(|&i| u32::from(i)));
        draws.push(Draw { material: m, first, count: 3 * u32::from(s.tri_count), base: base as i32, inst: (0, 1) });
    }
    let mut per_model: Vec<Vec<Instance>> = vec![Vec::new(); map.models.len()];
    for sm in &map.static_models {
        if let Some(list) = per_model.get_mut(sm.model) {
            let a = sm.axis;
            list.push(Instance { a0: [a[0][0], a[0][1], a[0][2], sm.scale], a1: [a[1][0], a[1][1], a[1][2], 0.0], a2: [a[2][0], a[2][1], a[2][2], 0.0], origin: [sm.origin[0], sm.origin[1], sm.origin[2], 0.0] });
        }
    }
    let mut props = 0usize;
    for (mi, model) in map.models.iter().enumerate() {
        let list = &per_model[mi];
        if list.is_empty() {
            continue;
        }
        let first_inst = insts.len() as u32;
        insts.extend_from_slice(list);
        props += list.len();
        for s in &model.surfaces {
            let Some(m) = s.material else { continue };
            let base = verts.len() as u32;
            for (k, p) in s.positions.iter().enumerate() {
                verts.push(Vertex { pos: *p, normal: s.normals.get(k).copied().unwrap_or([0.0, 0.0, 1.0]), uv: s.uvs.get(k).copied().unwrap_or_default() });
            }
            let first = idx.len() as u32;
            idx.extend(s.tris.iter().flatten().map(|&i| u32::from(i)));
            draws.push(Draw { material: m, first, count: 3 * s.tris.len() as u32, base: base as i32, inst: (first_inst, list.len() as u32) });
        }
    }
    println!("geometry: {} vertices, {} triangles, {} draws, {props} placements", verts.len(), idx.len() / 3, draws.len());

    let gpu = render_h1::gpu::Gpu::headless().expect("gpu");
    let dev = &gpu.device;
    let vbuf = dev.create_buffer_init(&wgpu::util::BufferInitDescriptor { label: Some("v"), contents: bytemuck::cast_slice(&verts), usage: wgpu::BufferUsages::VERTEX });
    let ibuf = dev.create_buffer_init(&wgpu::util::BufferInitDescriptor { label: Some("i"), contents: bytemuck::cast_slice(&idx), usage: wgpu::BufferUsages::INDEX });
    let instbuf = dev.create_buffer_init(&wgpu::util::BufferInitDescriptor { label: Some("inst"), contents: bytemuck::cast_slice(&insts), usage: wgpu::BufferUsages::VERTEX });
    drop((verts, idx, insts));

    // Materials: the colour picture (RGBA, mips, at most 512), alpha test,
    // drawn or shadow only.
    let samp = dev.create_sampler(&wgpu::SamplerDescriptor {
        address_mode_u: wgpu::AddressMode::Repeat,
        address_mode_v: wgpu::AddressMode::Repeat,
        mag_filter: wgpu::FilterMode::Linear,
        min_filter: wgpu::FilterMode::Linear,
        mipmap_filter: wgpu::MipmapFilterMode::Linear,
        anisotropy_clamp: 8,
        ..Default::default()
    });
    let frag = wgpu::ShaderStages::FRAGMENT;
    let mat_layout = dev.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("mat"),
        entries: &[
            wgpu::BindGroupLayoutEntry { binding: 0, visibility: frag, ty: wgpu::BindingType::Texture { sample_type: wgpu::TextureSampleType::Float { filterable: true }, view_dimension: wgpu::TextureViewDimension::D2, multisampled: false }, count: None },
            wgpu::BindGroupLayoutEntry { binding: 1, visibility: frag, ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering), count: None },
            wgpu::BindGroupLayoutEntry { binding: 2, visibility: frag, ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: None }, count: None },
        ],
    });
    let grey = upload(&gpu, &chain(1, 1, vec![128, 128, 128, 255], 1), "grey");
    let mut mat_binds = Vec::new();
    let mut drawn = Vec::new();
    let mut by_key: std::collections::HashMap<String, std::rc::Rc<wgpu::TextureView>> = Default::default();
    let (mut pictured, mut untextured) = (0, 0);
    for m in &map.materials {
        let alpha = m.name.contains("alphatest");
        let shadow_only = m.name.contains("shadowcaster");
        let view = m.textures.first().and_then(|(_, key)| {
            if let Some(v) = by_key.get(key) {
                return Some(v.clone());
            }
            let l = map.image(key, 1024).ok()?;
            let (lw, lh, px) = fastfile_t7::texture::level_rgba(&l, 0)?;
            let v = std::rc::Rc::new(upload(&gpu, &chain(lw, lh, px, 512), key));
            by_key.insert(key.clone(), v.clone());
            Some(v)
        });
        if view.is_some() {
            pictured += 1;
        } else {
            untextured += 1;
        }
        let ub = dev.create_buffer_init(&wgpu::util::BufferInitDescriptor { label: Some("mat"), contents: bytemuck::cast_slice(&[if alpha { 1.0f32 } else { 0.0 }, 1.0, 0.0, 0.0]), usage: wgpu::BufferUsages::UNIFORM });
        let v: &wgpu::TextureView = view.as_deref().unwrap_or(&grey);
        mat_binds.push(dev.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("mat"),
            layout: &mat_layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(v) },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(&samp) },
                wgpu::BindGroupEntry { binding: 2, resource: ub.as_entire_binding() },
            ],
        }));
        drawn.push(!shadow_only);
    }
    println!("materials: {pictured} with a picture, {untextured} grey");

    // Sky cube.
    let sky = map.sky_cube.clone();
    let (sky_size, sky_px) = match sky.as_ref() {
        Some(c) if c.format == 29 || c.format == 28 => (c.width, c.data[..(c.width * c.height * 4 * 6) as usize].to_vec()),
        _ => (1, [180u8, 200, 220, 255].repeat(6)),
    };
    let sky_tex = dev.create_texture(&wgpu::TextureDescriptor {
        label: Some("sky"),
        size: wgpu::Extent3d { width: sky_size, height: sky_size, depth_or_array_layers: 6 },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    // (Our faces are z up: +x -x +y -y +z -z; WGSL's cube is y up, so the
    // faces go +x, -x, +z(up), -z, +y, -y.)
    let face = (sky_size * sky_size * 4) as usize;
    for (layer, src) in [0usize, 1, 4, 5, 2, 3].iter().enumerate() {
        gpu.queue.write_texture(
            wgpu::TexelCopyTextureInfo { texture: &sky_tex, mip_level: 0, origin: wgpu::Origin3d { x: 0, y: 0, z: layer as u32 }, aspect: wgpu::TextureAspect::All },
            &sky_px[src * face..(src + 1) * face],
            wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(sky_size * 4), rows_per_image: Some(sky_size) },
            wgpu::Extent3d { width: sky_size, height: sky_size, depth_or_array_layers: 1 },
        );
    }
    let sky_view = sky_tex.create_view(&wgpu::TextureViewDescriptor { dimension: Some(wgpu::TextureViewDimension::Cube), ..Default::default() });
    let sky_samp = dev.create_sampler(&wgpu::SamplerDescriptor { mag_filter: wgpu::FilterMode::Linear, min_filter: wgpu::FilterMode::Linear, ..Default::default() });

    // The light, as the BO3 look sets it for a converted MW3 map.
    let ents = &map.entities;
    let angles = ws_vec(ents, "sundirection").filter(|v| v.len() == 3).unwrap_or(vec![-50.0, 35.0, 0.0]);
    let (p, y) = (angles[0].to_radians(), angles[1].to_radians());
    let sun_dir = Vec3::new(p.cos() * y.cos(), p.cos() * y.sin(), -p.sin()).normalize();
    let suncol = ws_vec(ents, "suncolor").filter(|v| v.len() == 3).unwrap_or(vec![1.0, 0.96, 0.9]);
    let peak = suncol.iter().copied().fold(0.0f32, f32::max).max(1e-3);
    let sun_col = [suncol[0] / peak * 2.6, suncol[1] / peak * 2.6, suncol[2] / peak * 2.6];
    let a = map.baked_ambient.unwrap_or([0.4, 0.43, 0.5]);
    let a2 = a.map(|c| c * c);
    let k = (0.4 / (0.2126 * a2[0] + 0.7152 * a2[1] + 0.0722 * a2[2]).max(1e-4)).clamp(0.05, 64.0);
    let amb = a2.map(|c| c * k);
    let fogv = ws_vec(ents, "jup_fog").filter(|v| v.len() == 17);
    let (fog, fog_col, sun_fog, sun_fog_ang) = match &fogv {
        Some(v) => ([v[0].max(0.0), v[1], v[2].clamp(0.0, 1.0), 1.0], [v[4], v[5], v[6], 0.0], [v[7], v[8], v[9], v[3].clamp(0.0, 1.0)], [v[13].to_radians().cos(), v[14].to_radians().cos(), 0.0, 0.0]),
        None => ([0.0, 0.0, 0.0, 1.0], [0.0; 4], [0.0; 4], [1.0, 0.0, 0.0, 0.0]),
    };
    println!("light: sun toward {sun_dir:?} colour {sun_col:?}; ambient {amb:?} (page {a:?} x{k:.2}); fog {fog:?} {fog_col:?}");
    log.push_str(&format!("light: sun toward {sun_dir:?} colour {sun_col:?}; ambient {amb:?}; fog {fog:?}\n"));

    // Views: given, else the reference match, two spawns and an overview.
    if cams.is_empty() {
        cams.push(("ref_match".into(), [1300.0, 900.0, 3300.0, 14.0, 210.0]));
        let sp = spawns(ents);
        for (i, s) in sp.iter().step_by((sp.len() / 3).max(1)).take(3).enumerate() {
            cams.push((format!("spawn{i}"), [s[0], s[1], s[2] + 60.0, 5.0, (i as f32) * 120.0 + 30.0]));
        }
        let mid = sp.iter().fold([0.0f32; 3], |acc, s| [acc[0] + s[0], acc[1] + s[1], acc[2] + s[2]]).map(|c| c / sp.len().max(1) as f32);
        cams.push(("overview".into(), [mid[0] - 2600.0, mid[1] - 2600.0, mid[2] + 1800.0, 28.0, 45.0]));
    }

    let globals_layout = dev.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("globals"),
        entries: &[
            wgpu::BindGroupLayoutEntry { binding: 0, visibility: wgpu::ShaderStages::VERTEX_FRAGMENT, ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: None }, count: None },
            wgpu::BindGroupLayoutEntry { binding: 1, visibility: frag, ty: wgpu::BindingType::Texture { sample_type: wgpu::TextureSampleType::Depth, view_dimension: wgpu::TextureViewDimension::D2, multisampled: false }, count: None },
            wgpu::BindGroupLayoutEntry { binding: 2, visibility: frag, ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Comparison), count: None },
            wgpu::BindGroupLayoutEntry { binding: 3, visibility: frag, ty: wgpu::BindingType::Texture { sample_type: wgpu::TextureSampleType::Float { filterable: true }, view_dimension: wgpu::TextureViewDimension::Cube, multisampled: false }, count: None },
            wgpu::BindGroupLayoutEntry { binding: 4, visibility: frag, ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering), count: None },
        ],
    });
    // (The shadow pass binds a 1x1 depth where the shadow map goes.)
    let module = dev.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some("jup_bo3_shot"), source: wgpu::ShaderSource::Wgsl(SHADER.into()) });
    let layout = dev.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: None, bind_group_layouts: &[Some(&globals_layout), Some(&mat_layout)], immediate_size: 0 });
    let vlayouts = [
        wgpu::VertexBufferLayout { array_stride: 32, step_mode: wgpu::VertexStepMode::Vertex, attributes: &wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3, 2 => Float32x2] },
        wgpu::VertexBufferLayout { array_stride: 64, step_mode: wgpu::VertexStepMode::Instance, attributes: &wgpu::vertex_attr_array![3 => Float32x4, 4 => Float32x4, 5 => Float32x4, 6 => Float32x4] },
    ];
    let depth_format = wgpu::TextureFormat::Depth32Float;
    let main_pipe = dev.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("main"),
        layout: Some(&layout),
        vertex: wgpu::VertexState { module: &module, entry_point: Some("vs_main"), compilation_options: Default::default(), buffers: &vlayouts },
        fragment: Some(wgpu::FragmentState { module: &module, entry_point: Some("fs_main"), compilation_options: Default::default(), targets: &[Some(wgpu::ColorTargetState { format: wgpu::TextureFormat::Rgba8Unorm, blend: None, write_mask: wgpu::ColorWrites::ALL })] }),
        primitive: wgpu::PrimitiveState::default(),
        depth_stencil: Some(wgpu::DepthStencilState { format: depth_format, depth_write_enabled: Some(true), depth_compare: Some(wgpu::CompareFunction::Less), stencil: Default::default(), bias: Default::default() }),
        multisample: Default::default(),
        multiview_mask: None,
        cache: None,
    });
    let shadow_pipe = dev.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("shadow"),
        layout: Some(&layout),
        vertex: wgpu::VertexState { module: &module, entry_point: Some("vs_shadow"), compilation_options: Default::default(), buffers: &vlayouts },
        fragment: Some(wgpu::FragmentState { module: &module, entry_point: Some("fs_shadow"), compilation_options: Default::default(), targets: &[] }),
        primitive: wgpu::PrimitiveState::default(),
        depth_stencil: Some(wgpu::DepthStencilState { format: depth_format, depth_write_enabled: Some(true), depth_compare: Some(wgpu::CompareFunction::Less), stencil: Default::default(), bias: wgpu::DepthBiasState { constant: 2, slope_scale: 2.0, clamp: 0.0 } }),
        multisample: Default::default(),
        multiview_mask: None,
        cache: None,
    });
    let sky_pipe = dev.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("sky"),
        layout: Some(&layout),
        vertex: wgpu::VertexState { module: &module, entry_point: Some("vs_sky"), compilation_options: Default::default(), buffers: &[] },
        fragment: Some(wgpu::FragmentState { module: &module, entry_point: Some("fs_sky"), compilation_options: Default::default(), targets: &[Some(wgpu::ColorTargetState { format: wgpu::TextureFormat::Rgba8Unorm, blend: None, write_mask: wgpu::ColorWrites::ALL })] }),
        primitive: wgpu::PrimitiveState::default(),
        depth_stencil: None,
        multisample: Default::default(),
        multiview_mask: None,
        cache: None,
    });
    const SHADOW: u32 = 4096;
    let depth_tex = |w: u32, h: u32, label: &str| {
        dev.create_texture(&wgpu::TextureDescriptor {
            label: Some(label),
            size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: depth_format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        })
    };
    let shadow_tex = depth_tex(SHADOW, SHADOW, "shadow");
    let shadow_view = shadow_tex.create_view(&Default::default());
    let dummy_depth = depth_tex(1, 1, "dummy").create_view(&Default::default());
    let main_depth = depth_tex(w, h, "depth").create_view(&Default::default());
    let colour = dev.create_texture(&wgpu::TextureDescriptor {
        label: Some("colour"),
        size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let colour_view = colour.create_view(&Default::default());
    let cmp = dev.create_sampler(&wgpu::SamplerDescriptor { compare: Some(wgpu::CompareFunction::LessEqual), mag_filter: wgpu::FilterMode::Linear, min_filter: wgpu::FilterMode::Linear, ..Default::default() });
    let ubuf = dev.create_buffer(&wgpu::BufferDescriptor { label: Some("globals"), size: std::mem::size_of::<Globals>() as u64, usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST, mapped_at_creation: false });
    let globals_bind = |depth: &wgpu::TextureView| {
        dev.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("globals"),
            layout: &globals_layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: ubuf.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(depth) },
                wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::Sampler(&cmp) },
                wgpu::BindGroupEntry { binding: 3, resource: wgpu::BindingResource::TextureView(&sky_view) },
                wgpu::BindGroupEntry { binding: 4, resource: wgpu::BindingResource::Sampler(&sky_samp) },
            ],
        })
    };
    let bind_shadow_pass = globals_bind(&dummy_depth);
    let bind_main = globals_bind(&shadow_view);

    for (name, c) in &cams {
        let t = std::time::Instant::now();
        let eye = Vec3::new(c[0], c[1], c[2]);
        let (p, y) = (c[3].to_radians(), c[4].to_radians());
        let fwd = Vec3::new(p.cos() * y.cos(), p.cos() * y.sin(), -p.sin());
        let view = Mat4::look_to_rh(eye, fwd, Vec3::Z);
        // (65 degrees across, as the game's lens.)
        let fov_y = 2.0 * ((65f32.to_radians() * 0.5).tan() * h as f32 / w as f32).atan();
        let proj = Mat4::perspective_rh(fov_y, w as f32 / h as f32, 4.0, 60000.0);
        let clip = proj * view;
        // The sun's map: 9000 units square round the view's first stretch.
        let centre = eye + fwd * 3000.0;
        let up = if sun_dir.z.abs() > 0.99 { Vec3::X } else { Vec3::Z };
        let sview = Mat4::look_to_rh(centre + sun_dir * 20000.0, -sun_dir, up);
        let sproj = Mat4::orthographic_rh(-4500.0, 4500.0, -4500.0, 4500.0, 1.0, 40000.0);
        let sclip = sproj * sview;
        let gl = Globals {
            clip_from_world: mat4(clip),
            sun_clip: mat4(sclip),
            world_from_clip: mat4(clip.inverse()),
            eye: eye.extend(1.0).into(),
            sun_dir: sun_dir.extend(0.0).into(),
            sun_col: Vec4::new(sun_col[0], sun_col[1], sun_col[2], 0.0).into(),
            amb: [amb[0], amb[1], amb[2], 0.0],
            fog,
            fog_col,
            sun_fog,
            sun_fog_ang,
        };
        let draw_all = |pass: &mut wgpu::RenderPass, shadow: bool| {
            pass.set_vertex_buffer(0, vbuf.slice(..));
            pass.set_vertex_buffer(1, instbuf.slice(..));
            pass.set_index_buffer(ibuf.slice(..), wgpu::IndexFormat::Uint32);
            for d in &draws {
                if !shadow && !drawn[d.material] {
                    continue;
                }
                pass.set_bind_group(1, &mat_binds[d.material], &[]);
                pass.draw_indexed(d.first..d.first + d.count, d.base, d.inst.0..d.inst.0 + d.inst.1);
            }
        };
        // Shadow pass (its own globals: the sun's matrix is read by vs_shadow).
        gpu.queue.write_buffer(&ubuf, 0, bytemuck::bytes_of(&gl));
        let mut enc = dev.create_command_encoder(&Default::default());
        {
            let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("shadow"),
                color_attachments: &[],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment { view: &shadow_view, depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(1.0), store: wgpu::StoreOp::Store }), stencil_ops: None }),
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_pipeline(&shadow_pipe);
            pass.set_bind_group(0, &bind_shadow_pass, &[]);
            draw_all(&mut pass, true);
        }
        {
            let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("sky"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment { view: &colour_view, depth_slice: None, resolve_target: None, ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::BLACK), store: wgpu::StoreOp::Store } })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_pipeline(&sky_pipe);
            pass.set_bind_group(0, &bind_main, &[]);
            pass.set_bind_group(1, &mat_binds[0], &[]);
            pass.draw(0..3, 0..1);
        }
        {
            let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("main"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment { view: &colour_view, depth_slice: None, resolve_target: None, ops: wgpu::Operations { load: wgpu::LoadOp::Load, store: wgpu::StoreOp::Store } })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment { view: &main_depth, depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(1.0), store: wgpu::StoreOp::Store }), stencil_ops: None }),
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_pipeline(&main_pipe);
            pass.set_bind_group(0, &bind_main, &[]);
            draw_all(&mut pass, false);
        }
        let row = (w * 4).div_ceil(256) * 256;
        let rb = dev.create_buffer(&wgpu::BufferDescriptor { label: None, size: u64::from(row * h), usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ, mapped_at_creation: false });
        enc.copy_texture_to_buffer(colour.as_image_copy(), wgpu::TexelCopyBufferInfo { buffer: &rb, layout: wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(row), rows_per_image: None } }, colour.size());
        gpu.queue.submit([enc.finish()]);
        rb.slice(..).map_async(wgpu::MapMode::Read, |_| {});
        let _ = dev.poll(wgpu::PollType::wait_indefinitely());
        let data = rb.slice(..).get_mapped_range().to_vec();
        let mut px = Vec::with_capacity((w * h * 3) as usize);
        for yy in 0..h {
            for xx in 0..w {
                let o = (yy * row + xx * 4) as usize;
                px.extend_from_slice(&data[o..o + 3]);
            }
        }
        let path = out.join(format!("{name}.png"));
        let f = std::fs::File::create(&path).expect("png");
        let mut e = png::Encoder::new(std::io::BufWriter::new(f), w, h);
        e.set_color(png::ColorType::Rgb);
        e.write_header().and_then(|mut wr| wr.write_image_data(&px)).expect("png write");
        let line = format!("{name}: eye {:?} pitch {} yaw {} -> {} [{:.1}s]", [c[0], c[1], c[2]], c[3], c[4], path.display(), t.elapsed().as_secs_f32());
        println!("{line}");
        log.push_str(&line);
        log.push('\n');
    }
    let _ = std::fs::write(out.join("log.txt"), log);
}
