//! The screen maps IW7's lit pixel programs read at the pixel's screen uv
//! (pixel position times `1 / target size`, code 109's zw):
//! - code texture 10, the sun's shadow: `min` with the light map's baked
//!   sun visibility (a negative value replaces it), so the world keeps its
//!   baked shadows and models, and what they cast, get the sun's;
//! - code texture 43, ambient occlusion: it scales the light map's (and
//!   the probes') indirect light;
//! - code texture 12 multiplies the sun again (white here).
//!
//! The game makes them from its depth pre-pass, sun shadow cascades and
//! its ambient occlusion pass; those programs are in `code_post_gfx`,
//! whose walk stops early, so these are drawn here with programs of our
//! own: a depth pre-pass of the opaque surfaces and placed models, two
//! sun cascades ([`CASCADES`], following the eye, snapped to their
//! texels), a 3x3 percentage-closer shadow mask, and a hemisphere ambient
//! occlusion pass over the pre-pass depth.
//!
//! The maps have a fixed size ([`Screen::new`]): every pass computes each
//! texel at its own screen uv, and the lit programs sample them by uv, so
//! the frame's size never asks for new bind groups. `BO2MP_IW7_SHADOWS=0`,
//! `BO2MP_IW7_SSAO=0`: white maps instead.

use glam::{Mat4, Vec3};
use render_h1::gpu::Gpu;
use wgpu::util::DeviceExt;

/// The sun cascades' half sizes (world units round the eye).
pub const CASCADES: [f32; 2] = [1024.0, 6144.0];
/// Each cascade's size in texels at full quality (`Screen::set_cascade_size`
/// lowers it).
pub const SHADOW_SIZE: u32 = 2048;
/// How deep the sun's view is (units each side of the cascade's centre).
pub const SHADOW_DEPTH: f32 = 30000.0;
/// The ambient occlusion's reach (world units) and strength.
pub const AO_RADIUS: f32 = 28.0;
pub const AO_STRENGTH: f32 = 1.4;

const DEPTH_VS: &str = r"
struct Cam { vp: mat4x4<f32>, eye: vec4<f32> };
@group(0) @binding(0) var<uniform> cam: Cam;
struct Obj { r0: vec4<f32>, r1: vec4<f32>, r2: vec4<f32>, pad: vec4<f32> };
@group(0) @binding(1) var<storage, read> objs: array<Obj>;
@vertex fn vs(@location(0) p: vec3<f32>, @builtin(instance_index) i: u32) -> @builtin(position) vec4<f32> {
    let o = objs[i];
    let q = vec4<f32>(p, 1.0);
    let w = vec3<f32>(dot(o.r0, q), dot(o.r1, q), dot(o.r2, q)) - cam.eye.xyz;
    return cam.vp * vec4<f32>(w, 1.0);
}
";

/// The depth passes' alpha-tested surfaces: the uv as well, and a texel of
/// the surface's alpha below a half is no occluder.
const CUTOUT: &str = r"
struct Cam { vp: mat4x4<f32>, eye: vec4<f32> };
@group(0) @binding(0) var<uniform> cam: Cam;
struct Obj { r0: vec4<f32>, r1: vec4<f32>, r2: vec4<f32>, pad: vec4<f32> };
@group(0) @binding(1) var<storage, read> objs: array<Obj>;
@group(1) @binding(0) var alpha_map: texture_2d<f32>;
@group(1) @binding(1) var alpha_smp: sampler;
// The channel holding the alpha.
@group(1) @binding(2) var<uniform> channel: vec4<f32>;
struct Vo { @builtin(position) p: vec4<f32>, @location(0) uv: vec2<f32> };
@vertex fn vs(@location(0) p: vec3<f32>, @location(1) uv: vec2<f32>, @builtin(instance_index) i: u32) -> Vo {
    let o = objs[i];
    let q = vec4<f32>(p, 1.0);
    let w = vec3<f32>(dot(o.r0, q), dot(o.r1, q), dot(o.r2, q)) - cam.eye.xyz;
    return Vo(cam.vp * vec4<f32>(w, 1.0), uv);
}
@fragment fn fs(v: Vo) {
    if (dot(textureSample(alpha_map, alpha_smp, v.uv), channel) < 0.5) { discard; }
}
";

const FULL_VS: &str = r"
struct Vo { @builtin(position) p: vec4<f32> };
@vertex fn vs(@builtin(vertex_index) i: u32) -> Vo {
    let x = f32((i << 1u) & 2u);
    let y = f32(i & 2u);
    return Vo(vec4<f32>(x * 2.0 - 1.0, 1.0 - y * 2.0, 0.0, 1.0));
}
";

/// The frame's numbers both screen passes read.
const FRAME: &str = r"
struct F {
    inv_vp: mat4x4<f32>,
    vp: mat4x4<f32>,
    eye: vec4<f32>,
    sun0: mat4x4<f32>,
    c0: vec4<f32>,
    sun1: mat4x4<f32>,
    c1: vec4<f32>,
    // target width, height, ao radius, ao strength
    p: vec4<f32>,
};
@group(0) @binding(0) var<uniform> f: F;
@group(0) @binding(1) var depth: texture_depth_2d;

fn uv_of(v: Vo) -> vec2<f32> { return v.p.xy / f.p.xy; }

// Eye-relative world position at screen uv (reversed depth d).
fn rel_at(uv: vec2<f32>, d: f32) -> vec3<f32> {
    let ndc = vec4<f32>(uv.x * 2.0 - 1.0, 1.0 - uv.y * 2.0, d, 1.0);
    let w = f.inv_vp * ndc;
    return w.xyz / w.w;
}

fn depth_at(uv: vec2<f32>) -> f32 {
    let s = vec2<i32>(textureDimensions(depth));
    return textureLoad(depth, clamp(vec2<i32>(uv * vec2<f32>(s)), vec2<i32>(0), s - 1), 0);
}
";

const SHADOW_FS: &str = r"
@group(0) @binding(2) var sh0: texture_depth_2d;
@group(0) @binding(3) var sh1: texture_depth_2d;
@group(0) @binding(4) var cmp: sampler_comparison;

fn cascade(m: mat4x4<f32>, c: vec4<f32>, world: vec3<f32>, which: i32) -> vec2<f32> {
    let s = m * vec4<f32>(world - c.xyz, 1.0);
    let uv = s.xy * vec2<f32>(0.5, -0.5) + 0.5;
    if (any(uv < vec2<f32>(0.002)) || any(uv > vec2<f32>(0.998)) || s.z <= 0.0 || s.z >= 1.0) {
        return vec2<f32>(1.0, 0.0);
    }
    let texel = 1.0 / f32(textureDimensions(sh0).x);
    // (Reversed: nearer the sun is larger; lit when not below the stored
    // depth by more than the bias.)
    let bias = c.w;
    var lit = 0.0;
    for (var y = -1; y <= 1; y++) {
        for (var x = -1; x <= 1; x++) {
            let o = vec2<f32>(f32(x), f32(y)) * texel;
            if (which == 0) {
                lit += textureSampleCompareLevel(sh0, cmp, uv + o, s.z + bias);
            } else {
                lit += textureSampleCompareLevel(sh1, cmp, uv + o, s.z + bias);
            }
        }
    }
    return vec2<f32>(lit / 9.0, 1.0);
}

@fragment fn fs(v: Vo) -> @location(0) vec4<f32> {
    let uv = uv_of(v);
    let d = depth_at(uv);
    if (d <= 0.0) { return vec4<f32>(1.0); }
    let world = rel_at(uv, d) + f.eye.xyz;
    let a = cascade(f.sun0, f.c0, world, 0);
    if (a.y > 0.5) { return vec4<f32>(a.x); }
    let b = cascade(f.sun1, f.c1, world, 1);
    return vec4<f32>(b.x);
}
";

const AO_FS: &str = r"
fn hash(p: vec2<f32>) -> f32 { return fract(sin(dot(p, vec2<f32>(12.9898, 78.233))) * 43758.5453); }

@fragment fn fs(v: Vo) -> @location(0) vec4<f32> {
    // (Drawn at half the screen maps' size.)
    let uv = v.p.xy / (f.p.xy * 0.5);
    let d = depth_at(uv);
    if (d <= 0.0) { return vec4<f32>(1.0); }
    let p = rel_at(uv, d);
    // (Far away a pixel spans more than the reach: none there.)
    let fade = clamp((length(p) - 1500.0) / 1000.0, 0.0, 1.0);
    if (fade >= 1.0) { return vec4<f32>(1.0); }
    let px = vec2<f32>(1.0) / vec2<f32>(textureDimensions(depth));
    let px_r = rel_at(uv + vec2<f32>(px.x, 0.0), depth_at(uv + vec2<f32>(px.x, 0.0)));
    let px_d = rel_at(uv + vec2<f32>(0.0, px.y), depth_at(uv + vec2<f32>(0.0, px.y)));
    var n = normalize(cross(px_d - p, px_r - p));
    // (Toward the eye.)
    if (dot(n, -p) < 0.0) { n = -n; }
    let t = normalize(select(cross(n, vec3<f32>(0.0, 0.0, 1.0)), cross(n, vec3<f32>(1.0, 0.0, 0.0)), abs(n.z) > 0.9));
    let b = cross(n, t);
    let r = f.p.z;
    let spin = hash(v.p.xy) * 6.2831853;
    var occ = 0.0;
    let count = 12;
    for (var i = 0; i < count; i++) {
        let fi = f32(i);
        let a = spin + fi * 2.39996;
        let h = (fi + 0.5) / f32(count);
        let rad = r * (0.25 + 0.75 * h * h);
        let dir = normalize(t * cos(a) + b * sin(a) + n * (0.3 + 0.7 * fract(fi * 0.618)));
        let q = p + dir * rad;
        let c = f.vp * vec4<f32>(q, 1.0);
        let suv = vec2<f32>(c.x / c.w * 0.5 + 0.5, 0.5 - c.y / c.w * 0.5);
        if (any(suv < vec2<f32>(0.0)) || any(suv > vec2<f32>(1.0))) { continue; }
        let sd = depth_at(suv);
        let s = rel_at(suv, sd);
        // The sample is hidden when the surface there is nearer the eye.
        let closer = length(q) - length(s);
        if (closer > 1.0) {
            occ += clamp(1.0 - (closer - rad) / r, 0.0, 1.0);
        }
    }
    let ao = clamp(1.0 - occ / f32(count) * f.p.w, 0.0, 1.0);
    return vec4<f32>(mix(ao, 1.0, fade));
}
";

pub(crate) fn depth_tex(g: &Gpu, w: u32, h: u32, label: &str) -> (wgpu::Texture, wgpu::TextureView) {
    let t = g.device.create_texture(&wgpu::TextureDescriptor {
        label: Some(label),
        size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Depth32Float,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
        view_formats: &[],
    });
    let v = t.create_view(&Default::default());
    (t, v)
}

fn colour_tex(g: &Gpu, w: u32, h: u32, label: &str) -> (wgpu::Texture, wgpu::TextureView) {
    let t = g.device.create_texture(&wgpu::TextureDescriptor {
        label: Some(label),
        size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba16Float,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let v = t.create_view(&Default::default());
    (t, v)
}

/// One occluder draw of the depth passes.
#[derive(Clone, Copy, Debug)]
pub struct Occluder {
    /// Base stream: 0 world (44-byte vertices), 1 models (40), and the
    /// byte offset of the first vertex.
    pub base: (u8, u64),
    /// Index buffer (0 world, 1 models), first index, count.
    pub index: (u8, u32, u32),
    /// The object's matrix (`Screen::objects` index).
    pub object: u32,
    /// World bounds (min, max).
    pub bounds: [Vec3; 2],
    /// Alpha tested: 1 + its [`Screen::add_cutout`] index (0: solid).
    pub cutout: u32,
}

pub(crate) fn cam_bytes(vp: Mat4, eye: Vec3) -> Vec<u8> {
    let mut v: Vec<f32> = vp.to_cols_array().to_vec();
    v.extend_from_slice(&[eye.x, eye.y, eye.z, 1.0]);
    bytemuck::cast_slice(&v).to_vec()
}

/// The screen maps and the passes that make them.
pub struct Screen {
    gpu: Gpu,
    pub width: u32,
    pub height: u32,
    pub shadows_on: bool,
    pub ao_on: bool,
    depth_pipes: [wgpu::RenderPipeline; 2],
    cams: [wgpu::Buffer; 3],
    cam_groups: [wgpu::BindGroup; 3],
    pre: (wgpu::Texture, wgpu::TextureView),
    sun: [(wgpu::Texture, wgpu::TextureView); 2],
    frame: wgpu::Buffer,
    shadow_pipe: wgpu::RenderPipeline,
    shadow_group: wgpu::BindGroup,
    ao_pipe: wgpu::RenderPipeline,
    ao_group: wgpu::BindGroup,
    /// The sun shadow mask (code texture 10) and ambient occlusion (43).
    pub shadow: (wgpu::Texture, wgpu::TextureView),
    pub ao: (wgpu::Texture, wgpu::TextureView),
    pub occluders: Vec<Occluder>,
    /// The cascades' size now, their mask's layout and compare sampler.
    pub shadow_size: u32,
    /// The vision's `r_ssaoStrength` (0: no ambient occlusion on this map).
    pub ao_strength: f32,
    shadow_layout: wgpu::BindGroupLayout,
    cmp: wgpu::Sampler,
    /// The depth passes' camera layout and the objects' rows (for other
    /// depth views: `spot`).
    pub(crate) cam_layout: wgpu::BindGroupLayout,
    pub(crate) objects: wgpu::Buffer,
    /// The alpha-tested occluders' pipelines (world, models), their alpha
    /// maps' groups, layout and sampler.
    cutout_pipes: [wgpu::RenderPipeline; 2],
    cutout_groups: Vec<wgpu::BindGroup>,
    cutout_layout: wgpu::BindGroupLayout,
    cutout_sampler: wgpu::Sampler,
    /// `BO2MP_IW7_CUTOUT_SHADOWS=0`: alpha-tested surfaces cast nothing.
    pub cutouts_on: bool,
}

impl Screen {
    /// The maps at `width` x `height` (ambient occlusion at half), the
    /// objects' world matrices as rows (index 0 the world's identity).
    pub fn new(g: &Gpu, width: u32, height: u32, objects: &[[[f32; 4]; 3]]) -> Self {
        let off = |k: &str| std::env::var(k).is_ok_and(|v| v.trim() == "0");
        let mut obj_bytes: Vec<f32> = Vec::with_capacity(objects.len() * 16);
        for o in objects {
            for r in o {
                obj_bytes.extend_from_slice(r);
            }
            obj_bytes.extend_from_slice(&[0.0; 4]);
        }
        if obj_bytes.is_empty() {
            obj_bytes.resize(16, 0.0);
        }
        let objs = g.device.create_buffer_init(&wgpu::util::BufferInitDescriptor { label: Some("iw7 objects"), contents: bytemuck::cast_slice(&obj_bytes), usage: wgpu::BufferUsages::STORAGE });
        let cam_layout = g.device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("iw7 depth"),
            entries: &[
                wgpu::BindGroupLayoutEntry { binding: 0, visibility: wgpu::ShaderStages::VERTEX, ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: None }, count: None },
                wgpu::BindGroupLayoutEntry { binding: 1, visibility: wgpu::ShaderStages::VERTEX, ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Storage { read_only: true }, has_dynamic_offset: false, min_binding_size: None }, count: None },
            ],
        });
        let cams: [wgpu::Buffer; 3] = std::array::from_fn(|_| g.device.create_buffer(&wgpu::BufferDescriptor { label: Some("iw7 depth cam"), size: 80, usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST, mapped_at_creation: false }));
        let cam_groups: [wgpu::BindGroup; 3] = std::array::from_fn(|i| {
            g.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("iw7 depth cam"),
                layout: &cam_layout,
                entries: &[wgpu::BindGroupEntry { binding: 0, resource: cams[i].as_entire_binding() }, wgpu::BindGroupEntry { binding: 1, resource: objs.as_entire_binding() }],
            })
        });
        let depth_module = g.device.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some("iw7 depth"), source: wgpu::ShaderSource::Wgsl(DEPTH_VS.into()) });
        let depth_layout = g.device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: Some("iw7 depth"), bind_group_layouts: &[Some(&cam_layout)], immediate_size: 0 });
        let depth_pipes: [wgpu::RenderPipeline; 2] = std::array::from_fn(|k| {
            let stride = if k == 0 { 44 } else { crate::vertex::MODEL_STRIDE };
            g.device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("iw7 depth"),
                layout: Some(&depth_layout),
                vertex: wgpu::VertexState {
                    module: &depth_module,
                    entry_point: Some("vs"),
                    compilation_options: Default::default(),
                    buffers: &[wgpu::VertexBufferLayout { array_stride: stride, step_mode: wgpu::VertexStepMode::Vertex, attributes: &[wgpu::VertexAttribute { format: wgpu::VertexFormat::Float32x3, offset: 0, shader_location: 0 }] }],
                },
                primitive: wgpu::PrimitiveState { cull_mode: None, ..Default::default() },
                depth_stencil: Some(wgpu::DepthStencilState {
                    format: wgpu::TextureFormat::Depth32Float,
                    depth_write_enabled: Some(true),
                    depth_compare: Some(wgpu::CompareFunction::Greater),
                    stencil: Default::default(),
                    bias: Default::default(),
                }),
                multisample: Default::default(),
                fragment: None,
                multiview_mask: None,
                cache: None,
            })
        });
        let cutout_layout = g.device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("iw7 cutout"),
            entries: &[
                wgpu::BindGroupLayoutEntry { binding: 0, visibility: wgpu::ShaderStages::FRAGMENT, ty: wgpu::BindingType::Texture { sample_type: wgpu::TextureSampleType::Float { filterable: true }, view_dimension: wgpu::TextureViewDimension::D2, multisampled: false }, count: None },
                wgpu::BindGroupLayoutEntry { binding: 1, visibility: wgpu::ShaderStages::FRAGMENT, ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering), count: None },
                wgpu::BindGroupLayoutEntry { binding: 2, visibility: wgpu::ShaderStages::FRAGMENT, ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: None }, count: None },
            ],
        });
        let cutout_module = g.device.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some("iw7 cutout"), source: wgpu::ShaderSource::Wgsl(CUTOUT.into()) });
        let cutout_pl = g.device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: Some("iw7 cutout"), bind_group_layouts: &[Some(&cam_layout), Some(&cutout_layout)], immediate_size: 0 });
        let cutout_pipes: [wgpu::RenderPipeline; 2] = std::array::from_fn(|k| {
            // (World: GfxWorldVertex's uv, floats at 20; models:
            // GfxPackedVertex's, halves at 20, in 40-byte vertices.)
            let (stride, uv) = if k == 0 { (44, wgpu::VertexFormat::Float32x2) } else { (crate::vertex::MODEL_STRIDE, wgpu::VertexFormat::Float16x2) };
            g.device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("iw7 cutout"),
                layout: Some(&cutout_pl),
                vertex: wgpu::VertexState {
                    module: &cutout_module,
                    entry_point: Some("vs"),
                    compilation_options: Default::default(),
                    buffers: &[wgpu::VertexBufferLayout {
                        array_stride: stride,
                        step_mode: wgpu::VertexStepMode::Vertex,
                        attributes: &[wgpu::VertexAttribute { format: wgpu::VertexFormat::Float32x3, offset: 0, shader_location: 0 }, wgpu::VertexAttribute { format: uv, offset: 20, shader_location: 1 }],
                    }],
                },
                primitive: wgpu::PrimitiveState { cull_mode: None, ..Default::default() },
                depth_stencil: Some(wgpu::DepthStencilState {
                    format: wgpu::TextureFormat::Depth32Float,
                    depth_write_enabled: Some(true),
                    depth_compare: Some(wgpu::CompareFunction::Greater),
                    stencil: Default::default(),
                    bias: Default::default(),
                }),
                multisample: Default::default(),
                fragment: Some(wgpu::FragmentState { module: &cutout_module, entry_point: Some("fs"), compilation_options: Default::default(), targets: &[] }),
                multiview_mask: None,
                cache: None,
            })
        });
        let cutout_sampler = g.device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("iw7 cutout"),
            address_mode_u: wgpu::AddressMode::Repeat,
            address_mode_v: wgpu::AddressMode::Repeat,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Linear,
            ..Default::default()
        });
        let pre = depth_tex(g, width, height, "iw7 pre-pass depth");
        let sun = [depth_tex(g, SHADOW_SIZE, SHADOW_SIZE, "iw7 sun cascade 0"), depth_tex(g, SHADOW_SIZE, SHADOW_SIZE, "iw7 sun cascade 1")];
        let frame = g.device.create_buffer(&wgpu::BufferDescriptor { label: Some("iw7 screen frame"), size: 4 * (16 * 4 + 4 * 4), usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST, mapped_at_creation: false });
        let depth_entry = |b: u32| wgpu::BindGroupLayoutEntry {
            binding: b,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture { sample_type: wgpu::TextureSampleType::Depth, view_dimension: wgpu::TextureViewDimension::D2, multisampled: false },
            count: None,
        };
        let frame_entry = wgpu::BindGroupLayoutEntry { binding: 0, visibility: wgpu::ShaderStages::FRAGMENT, ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: None }, count: None };
        let shadow_layout = g.device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("iw7 shadow mask"),
            entries: &[frame_entry, depth_entry(1), depth_entry(2), depth_entry(3), wgpu::BindGroupLayoutEntry { binding: 4, visibility: wgpu::ShaderStages::FRAGMENT, ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Comparison), count: None }],
        });
        let ao_layout = g.device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor { label: Some("iw7 ao"), entries: &[frame_entry, depth_entry(1)] });
        let full = |label: &str, src: &str, layout: &wgpu::BindGroupLayout| {
            let module = g.device.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some(label), source: wgpu::ShaderSource::Wgsl(format!("{FULL_VS}{FRAME}{src}").into()) });
            let pl = g.device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: Some(label), bind_group_layouts: &[Some(layout)], immediate_size: 0 });
            g.device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(label),
                layout: Some(&pl),
                vertex: wgpu::VertexState { module: &module, entry_point: Some("vs"), compilation_options: Default::default(), buffers: &[] },
                primitive: Default::default(),
                depth_stencil: None,
                multisample: Default::default(),
                fragment: Some(wgpu::FragmentState { module: &module, entry_point: Some("fs"), compilation_options: Default::default(), targets: &[Some(wgpu::TextureFormat::Rgba16Float.into())] }),
                multiview_mask: None,
                cache: None,
            })
        };
        let shadow_pipe = full("iw7 shadow mask", SHADOW_FS, &shadow_layout);
        let ao_pipe = full("iw7 ao", AO_FS, &ao_layout);
        let cmp = g.device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("iw7 shadow compare"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            compare: Some(wgpu::CompareFunction::GreaterEqual),
            ..Default::default()
        });
        let shadow = colour_tex(g, width, height, "iw7 sun shadow mask");
        let ao = colour_tex(g, (width / 2).max(1), (height / 2).max(1), "iw7 ambient occlusion");
        let shadow_group = g.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("iw7 shadow mask"),
            layout: &shadow_layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: frame.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&pre.1) },
                wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::TextureView(&sun[0].1) },
                wgpu::BindGroupEntry { binding: 3, resource: wgpu::BindingResource::TextureView(&sun[1].1) },
                wgpu::BindGroupEntry { binding: 4, resource: wgpu::BindingResource::Sampler(&cmp) },
            ],
        });
        let ao_group = g.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("iw7 ao"),
            layout: &ao_layout,
            entries: &[wgpu::BindGroupEntry { binding: 0, resource: frame.as_entire_binding() }, wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&pre.1) }],
        });
        Self {
            gpu: g.clone(),
            width,
            height,
            shadows_on: !off("BO2MP_IW7_SHADOWS"),
            ao_on: !off("BO2MP_IW7_SSAO"),
            depth_pipes,
            cams,
            cam_groups,
            pre,
            sun,
            frame,
            shadow_pipe,
            shadow_group,
            ao_pipe,
            ao_group,
            shadow,
            ao,
            occluders: Vec::new(),
            cam_layout,
            objects: objs,
            shadow_size: SHADOW_SIZE,
            ao_strength: 1.0,
            shadow_layout,
            cmp,
            cutout_pipes,
            cutout_groups: Vec::new(),
            cutout_layout,
            cutout_sampler,
            cutouts_on: !off("BO2MP_IW7_CUTOUT_SHADOWS"),
        }
    }

    /// An alpha map for alpha-tested occluders (`alpha` the channel holding
    /// it: 0 red, 3 alpha); the value for [`Occluder::cutout`].
    pub fn add_cutout(&mut self, g: &Gpu, view: &wgpu::TextureView, alpha: usize) -> u32 {
        let mut channel = [0.0f32; 4];
        channel[alpha.min(3)] = 1.0;
        let buffer = g.device.create_buffer_init(&wgpu::util::BufferInitDescriptor { label: Some("iw7 cutout channel"), contents: bytemuck::cast_slice(&channel), usage: wgpu::BufferUsages::UNIFORM });
        self.cutout_groups.push(g.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("iw7 cutout"),
            layout: &self.cutout_layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(view) },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(&self.cutout_sampler) },
                wgpu::BindGroupEntry { binding: 2, resource: buffer.as_entire_binding() },
            ],
        }));
        self.cutout_groups.len() as u32
    }

    /// Orders the occluders for drawing: solid first, then by alpha map.
    pub fn sort_occluders(&mut self) {
        self.occluders.sort_by_key(|o| (o.cutout, o.base.0));
    }

    /// A cascade's view-projection about its centre (reversed depth) and
    /// the centre, snapped to its texels.
    /// Makes the cascades `size` texels a side (and the mask's group
    /// again); the volumetric fog must take the new [`Screen::sun_views`].
    pub fn set_cascade_size(&mut self, g: &Gpu, size: u32) {
        let size = size.clamp(256, 8192);
        if size == self.shadow_size {
            return;
        }
        self.shadow_size = size;
        self.sun = [depth_tex(g, size, size, "iw7 sun cascade 0"), depth_tex(g, size, size, "iw7 sun cascade 1")];
        self.shadow_group = g.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("iw7 shadow mask"),
            layout: &self.shadow_layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: self.frame.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&self.pre.1) },
                wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::TextureView(&self.sun[0].1) },
                wgpu::BindGroupEntry { binding: 3, resource: wgpu::BindingResource::TextureView(&self.sun[1].1) },
                wgpu::BindGroupEntry { binding: 4, resource: wgpu::BindingResource::Sampler(&self.cmp) },
            ],
        });
    }

    fn cascade(toward: Vec3, eye: Vec3, half: f32, size: u32) -> (Mat4, Vec3) {
        let dir = -toward.normalize_or(Vec3::Z);
        let up = if dir.z.abs() > 0.99 { Vec3::X } else { Vec3::Z };
        let view = Mat4::look_to_rh(Vec3::ZERO, dir, up);
        // (Snap the centre to whole texels in the sun's view.)
        let texel = 2.0 * half / size as f32;
        let c = view.transform_point3(eye);
        let snapped = Vec3::new((c.x / texel).floor() * texel, (c.y / texel).floor() * texel, c.z);
        let centre = view.inverse().transform_point3(snapped);
        let ortho = Mat4::orthographic_rh(-half, half, -half, half, -SHADOW_DEPTH, SHADOW_DEPTH);
        // Reversed: z' = 1 - z.
        let reverse = Mat4::from_cols(glam::Vec4::X, glam::Vec4::Y, glam::Vec4::new(0.0, 0.0, -1.0, 0.0), glam::Vec4::new(0.0, 0.0, 1.0, 1.0));
        (reverse * ortho * view, centre)
    }

    /// Draws the occluders' depth (reversed: cleared to 0, greater wins)
    /// into `target` through a camera group of [`Screen::cam_layout`].
    /// `view`: only the occluders it sees. Returns how many were drawn.
    pub(crate) fn draw_occluders(&self, enc: &mut wgpu::CommandEncoder, target: &wgpu::TextureView, cam: &wgpu::BindGroup, vbs: &[wgpu::Buffer; 2], ibs: &[wgpu::Buffer; 2], view: Option<&crate::cull::Frustum>) -> u32 {
        let mut rp = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("iw7 depth"),
            color_attachments: &[],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment { view: target, depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(0.0), store: wgpu::StoreOp::Store }), stencil_ops: None }),
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        rp.set_bind_group(0, cam, &[]);
        let mut bound = (u8::MAX, false);
        let mut bound_cutout = 0;
        let mut drawn = 0;
        for o in &self.occluders {
            if view.is_some_and(|f| !f.sees(o.bounds[0], o.bounds[1])) || (o.cutout > 0 && !self.cutouts_on) {
                continue;
            }
            drawn += 1;
            let kind = (o.base.0, o.cutout > 0);
            if kind != bound {
                bound = kind;
                let k = usize::from(o.base.0);
                rp.set_pipeline(if kind.1 { &self.cutout_pipes[k] } else { &self.depth_pipes[k] });
            }
            if o.cutout > 0 && o.cutout != bound_cutout {
                bound_cutout = o.cutout;
                rp.set_bind_group(1, &self.cutout_groups[o.cutout as usize - 1], &[]);
            }
            rp.set_vertex_buffer(0, vbs[usize::from(o.base.0)].slice(o.base.1..));
            rp.set_index_buffer(ibs[usize::from(o.index.0)].slice(..), wgpu::IndexFormat::Uint16);
            rp.draw_indexed(o.index.1..o.index.1 + o.index.2, 0, o.object..o.object + 1);
        }
        drawn
    }

    /// The sun cascades' depth maps (reversed: nearer the sun larger).
    pub fn sun_views(&self) -> [&wgpu::TextureView; 2] {
        [&self.sun[0].1, &self.sun[1].1]
    }

    /// Each sun cascade's view-projection about its centre, the centre and
    /// the depth bias, as [`Screen::record`] draws them.
    pub fn cascades(&self, eye: Vec3, sun_toward: Option<Vec3>) -> [(Mat4, Vec3, f32); 2] {
        let toward = sun_toward.unwrap_or(Vec3::Z);
        std::array::from_fn(|i| {
            let (vp, centre) = Self::cascade(toward, eye, CASCADES[i], self.shadow_size);
            (vp, centre, self.bias(i))
        })
    }

    fn bias(&self, i: usize) -> f32 {
        // (About two texels' worth of the depth range.)
        2.0 * (2.0 * CASCADES[i] / self.shadow_size as f32) / (2.0 * SHADOW_DEPTH) + 1e-5
    }

    /// Records the depth pre-pass, the sun cascades and the two maps.
    #[allow(clippy::too_many_arguments)]
    /// Returns the bytes uploaded, the passes and the occluders drawn into
    /// the pre-pass and each cascade.
    pub fn record(&self, enc: &mut wgpu::CommandEncoder, view_projection: Mat4, eye: Vec3, sun_toward: Option<Vec3>, vbs: &[wgpu::Buffer; 2], ibs: &[wgpu::Buffer; 2]) -> (u64, u32, [u32; 3]) {
        let q = &self.gpu.queue;
        q.write_buffer(&self.cams[0], 0, &cam_bytes(view_projection, eye));
        let toward = sun_toward.unwrap_or(Vec3::Z);
        let c: [(Mat4, Vec3); 2] = std::array::from_fn(|i| Self::cascade(toward, eye, CASCADES[i], self.shadow_size));
        for (i, (vp, centre)) in c.iter().enumerate() {
            q.write_buffer(&self.cams[i + 1], 0, &cam_bytes(*vp, *centre));
        }
        let mut f = crate::cull::Pack::<96>::new();
        f.extend_from_slice(&view_projection.inverse().to_cols_array());
        f.extend_from_slice(&view_projection.to_cols_array());
        f.extend_from_slice(&[eye.x, eye.y, eye.z, 1.0]);
        for (i, (vp, centre)) in c.iter().enumerate() {
            f.extend_from_slice(&vp.to_cols_array());
            let bias = self.bias(i);
            f.extend_from_slice(&[centre.x, centre.y, centre.z, bias]);
        }
        f.extend_from_slice(&[self.width as f32, self.height as f32, AO_RADIUS, AO_STRENGTH * self.ao_strength]);
        q.write_buffer(&self.frame, 0, f.bytes());
        let cull = crate::cull::on();
        let views = [crate::cull::Frustum::new(view_projection, eye), crate::cull::Frustum::new(c[0].0, c[0].1), crate::cull::Frustum::new(c[1].0, c[1].1)];
        let depth_pass = |enc: &mut wgpu::CommandEncoder, target: &wgpu::TextureView, cam: usize| self.draw_occluders(enc, target, &self.cam_groups[cam], vbs, ibs, cull.then_some(&views[cam]));
        let fill = |enc: &mut wgpu::CommandEncoder, target: &wgpu::TextureView, pipe: Option<(&wgpu::RenderPipeline, &wgpu::BindGroup)>| {
            let mut rp = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("iw7 screen map"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment { view: target, resolve_target: None, depth_slice: None, ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::WHITE), store: wgpu::StoreOp::Store } })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            if let Some((p, g)) = pipe {
                rp.set_pipeline(p);
                rp.set_bind_group(0, g, &[]);
                rp.draw(0..3, 0..1);
            }
        };
        let mut drawn = [0u32; 3];
        let mut passes = 2;
        if self.shadows_on || self.ao_on {
            drawn[0] = depth_pass(enc, &self.pre.1, 0);
            passes += 1;
        }
        if self.shadows_on && sun_toward.is_some() {
            drawn[1] = depth_pass(enc, &self.sun[0].1, 1);
            drawn[2] = depth_pass(enc, &self.sun[1].1, 2);
            passes += 2;
            fill(enc, &self.shadow.1, Some((&self.shadow_pipe, &self.shadow_group)));
        } else {
            fill(enc, &self.shadow.1, None);
        }
        fill(enc, &self.ao.1, self.ao_on.then_some((&self.ao_pipe, &self.ao_group)));
        ((80 * 3 + f.len() * 4) as u64, passes, drawn)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A quad over the whole view, alpha tested through a picture whose
    /// left half is opaque: its depth only on the left.
    #[test]
    fn an_alpha_tested_occluder_casts_only_where_its_alpha_is_opaque() {
        let Ok(g) = Gpu::headless() else { return };
        let identity = [[1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0]];
        let mut screen = Screen::new(&g, 64, 64, &[identity]);
        // (World vertices: position, then the uv at 20.)
        let corners = [(-1.0f32, -1.0f32, 0.0f32, 1.0f32), (1.0, -1.0, 1.0, 1.0), (1.0, 1.0, 1.0, 0.0), (-1.0, 1.0, 0.0, 0.0)];
        let mut vb = Vec::new();
        for (x, y, u, v) in corners {
            let mut vert = [0f32; 11];
            vert[..3].copy_from_slice(&[x, y, 0.5]);
            vert[5] = u;
            vert[6] = v;
            vb.extend_from_slice(&vert);
        }
        let buffer = |c: &[u8], usage| g.device.create_buffer_init(&wgpu::util::BufferInitDescriptor { label: None, contents: c, usage });
        let vbs = [buffer(bytemuck::cast_slice(&vb), wgpu::BufferUsages::VERTEX), buffer(&[0u8; 64], wgpu::BufferUsages::VERTEX)];
        let ib: [u16; 6] = [0, 1, 2, 0, 2, 3];
        let ibs = [buffer(bytemuck::cast_slice(&ib), wgpu::BufferUsages::INDEX), buffer(&[0u8; 16], wgpu::BufferUsages::INDEX)];
        let alpha = g.device.create_texture_with_data(
            &g.queue,
            &wgpu::TextureDescriptor {
                label: None,
                size: wgpu::Extent3d { width: 4, height: 1, depth_or_array_layers: 1 },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba8Unorm,
                usage: wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            },
            Default::default(),
            &[0, 0, 0, 255, 0, 0, 0, 255, 0, 0, 0, 0, 0, 0, 0, 0],
        );
        let cutout = screen.add_cutout(&g, &alpha.create_view(&Default::default()), 3);
        screen.occluders.push(Occluder { base: (0, 0), index: (0, 0, 6), object: 0, bounds: [Vec3::splat(-1.0), Vec3::splat(1.0)], cutout });
        g.queue.write_buffer(&screen.cams[0], 0, &cam_bytes(Mat4::IDENTITY, Vec3::ZERO));
        let mut enc = g.device.create_command_encoder(&Default::default());
        let target = g.device.create_texture(&wgpu::TextureDescriptor {
            label: None,
            size: wgpu::Extent3d { width: 64, height: 64, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Depth32Float,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        screen.draw_occluders(&mut enc, &target.create_view(&Default::default()), &screen.cam_groups[0], &vbs, &ibs, None);
        let read = g.device.create_buffer(&wgpu::BufferDescriptor { label: None, size: 256 * 64, usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ, mapped_at_creation: false });
        enc.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo { texture: &target, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::DepthOnly },
            wgpu::TexelCopyBufferInfo { buffer: &read, layout: wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(256), rows_per_image: Some(64) } },
            wgpu::Extent3d { width: 64, height: 64, depth_or_array_layers: 1 },
        );
        g.queue.submit([enc.finish()]);
        read.slice(..).map_async(wgpu::MapMode::Read, |_| {});
        let _ = g.device.poll(wgpu::PollType::wait_indefinitely());
        let data = read.slice(..).get_mapped_range();
        let depth: &[f32] = bytemuck::cast_slice(&data);
        let row = 32 * 64;
        assert!((depth[row + 8] - 0.5).abs() < 1e-3, "left {}", depth[row + 8]);
        assert_eq!(depth[row + 56], 0.0, "right");
    }
}
