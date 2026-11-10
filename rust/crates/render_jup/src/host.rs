//! The frame of a Modern Warfare III / II map: the sun's shadow map, the
//! G-buffer (each surface's material pictures through the fallback shader
//! until the game's own programs can be driven, `crate::mapping`), the
//! lighting pass and the display stage; then the finished image and the
//! scene depth for the host's frame (`crate::present`).
//!
//! Every pipeline is made under a validation error scope when the host is
//! made, so a pipeline or binding this device rejects is reported (and the
//! host not made) instead of failing on the first frame. A thread of its
//! own builds the game's pipeline list for the map (`crate::coverage`) and
//! reports what the game's programs lower to; it never touches the frame.

use std::sync::{Arc, Mutex};

use glam::{Mat4, Vec3};
use render_h1::code::Camera;
use render_h1::gpu::Gpu;
use wgpu::util::DeviceExt;

use crate::scene::{Instance, PROBE_SIZE, Scene, SceneImage, Vertex};
use crate::shaders;

const SCENE_DEPTH: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;
const ALBEDO: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8UnormSrgb;
const NORMAL: wgpu::TextureFormat = wgpu::TextureFormat::Rgba16Float;
const HDR: wgpu::TextureFormat = wgpu::TextureFormat::Rgba16Float;
const FINAL: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;
const SHADOW_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;

/// Half the width of the area the sun's shadow map covers around the eye.
const SHADOW_REACH: f32 = 2600.0;

/// The frame's constants (`common.wgsl`, struct `G`).
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Globals {
    vp: [[f32; 4]; 4],
    inv_vp: [[f32; 4]; 4],
    sun_vp: [[f32; 4]; 4],
    eye: [f32; 4],
    sun_dir: [f32; 4],
    sun_col: [f32; 4],
    fog: [f32; 4],
    fog_col: [f32; 4],
    fog_sun: [f32; 4],
    fog_misc: [f32; 4],
    sky_zenith: [f32; 4],
    sky_horizon: [f32; 4],
    ambient: [f32; 4],
    size: [f32; 4],
    misc: [f32; 4],
    amb_up: [f32; 4],
    amb_side: [f32; 4],
    amb_down: [f32; 4],
    grid: [f32; 4],
    grid2: [f32; 4],
    amb_misc: [f32; 4],
}

struct Targets {
    width: u32,
    height: u32,
    depth: wgpu::TextureView,
    depth_only: wgpu::TextureView,
    albedo: wgpu::TextureView,
    normal: wgpu::TextureView,
    hdr: wgpu::TextureView,
    final_view: wgpu::TextureView,
    light_bind: wgpu::BindGroup,
    display_bind: wgpu::BindGroup,
    _textures: Vec<wgpu::Texture>,
}

/// What the host drew, for the log.
#[derive(Default)]
struct Stats {
    frames: u64,
    announced: bool,
}

/// The renderer of one map.
pub struct Host {
    gpu: Gpu,
    scene_name: String,
    mins: [f32; 3],
    maxs: [f32; 3],
    globals_buf: wgpu::Buffer,
    globals_bind: wgpu::BindGroup,
    vertex_buf: wgpu::Buffer,
    index_buf: wgpu::Buffer,
    instance_buf: wgpu::Buffer,
    /// The opaque and alpha-tested draws.
    draws: Vec<crate::scene::Draw>,
    /// The blended draws (translucent materials and the sea), sorted far to
    /// near each frame.
    blended: Vec<crate::scene::Draw>,
    /// By material: drawn with the water shader.
    water_material: Vec<bool>,
    /// Tool materials (shadow casters, never drawn into the view).
    tool_material: Vec<bool>,
    forward_pipeline: wgpu::RenderPipeline,
    water_pipeline: wgpu::RenderPipeline,
    forward_bind: wgpu::BindGroup,
    material_binds: Vec<wgpu::BindGroup>,
    shadow_pipeline: wgpu::RenderPipeline,
    shadow_view: wgpu::TextureView,
    shadow_size: u32,
    gbuffer_pipeline: wgpu::RenderPipeline,
    light_pipeline: wgpu::RenderPipeline,
    light_layout: wgpu::BindGroupLayout,
    display_pipeline: wgpu::RenderPipeline,
    display_layout: wgpu::BindGroupLayout,
    shadow_sampler: wgpu::Sampler,
    probe_buf: wgpu::Buffer,
    probe_view: wgpu::TextureView,
    sky_view: wgpu::TextureView,
    has_sky: bool,
    grade_view: wgpu::TextureView,
    grade_sampler: wgpu::Sampler,
    has_grade: bool,
    probe_sampler: wgpu::Sampler,
    probe_count: usize,
    grid_base: usize,
    grid_cols: u32,
    grid_rows: u32,
    grid_origin: [f32; 2],
    grid_cell: f32,
    has_grid: bool,
    targets: Option<Targets>,
    env: crate::scene::Env,
    shadows: bool,
    exposure: f32,
    /// The display curve: 0 the soft shoulder, 1 the filmic one (`BO2MP_JUP_TONEMAP`).
    tonemap: f32,
    /// The screen-space ambient occlusion's strength (`BO2MP_JUP_SSAO`, 0 off).
    ssao: f32,
    stats: Stats,
    report: Vec<String>,
    /// Lines from the pipeline check thread.
    check: Arc<Mutex<Vec<String>>>,
    complete: bool,
}

fn env_flag(name: &str, default: bool) -> bool {
    codlegacy_env::var(name).map_or(default, |v| !matches!(v.trim(), "0" | "false" | "off"))
}

fn texture_format(dxgi: u32, srgb: bool) -> Option<wgpu::TextureFormat> {
    Some(match (dxgi, srgb) {
        (71, true) => wgpu::TextureFormat::Bc1RgbaUnormSrgb,
        (71, false) => wgpu::TextureFormat::Bc1RgbaUnorm,
        (80, _) => wgpu::TextureFormat::Bc4RUnorm,
        (98, true) => wgpu::TextureFormat::Bc7RgbaUnormSrgb,
        (98, false) => wgpu::TextureFormat::Bc7RgbaUnorm,
        _ => return None,
    })
}

/// A picture's mip chain on the device.
fn upload(gpu: &Gpu, im: &SceneImage, srgb: bool) -> Result<wgpu::TextureView, String> {
    let format = texture_format(im.dxgi, srgb).ok_or_else(|| format!("{:016x}: DXGI {} not drawn", im.hash, im.dxgi))?;
    if !gpu.bc {
        return Err("the device has no BC texture support".into());
    }
    let (bw, bh) = format.block_dimensions();
    let bsize = format.block_copy_size(None).ok_or("format size")?;
    let (w0, h0) = (im.width.div_ceil(bw) * bw, im.height.div_ceil(bh) * bh);
    let texture = gpu.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("jup_picture"),
        size: wgpu::Extent3d { width: w0, height: h0, depth_or_array_layers: 1 },
        mip_level_count: im.levels.len().max(1) as u32,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    for (l, data) in im.levels.iter().enumerate() {
        let (w, h) = ((im.width >> l).max(1), (im.height >> l).max(1));
        let (bx, by) = (w.div_ceil(bw), h.div_ceil(bh));
        let row = bx * bsize;
        if data.len() < (row * by) as usize {
            return Err(format!("{:016x}: level {l} is {} bytes, wants {}", im.hash, data.len(), row * by));
        }
        gpu.queue.write_texture(
            wgpu::TexelCopyTextureInfo { texture: &texture, mip_level: l as u32, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
            &data[..(row * by) as usize],
            wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(row), rows_per_image: Some(by) },
            wgpu::Extent3d { width: bx * bw, height: by * bh, depth_or_array_layers: 1 },
        );
    }
    Ok(texture.create_view(&wgpu::TextureViewDescriptor::default()))
}

fn solid(gpu: &Gpu, rgba: [u8; 4], srgb: bool) -> wgpu::TextureView {
    let format = if srgb { wgpu::TextureFormat::Rgba8UnormSrgb } else { wgpu::TextureFormat::Rgba8Unorm };
    let texture = gpu.device.create_texture_with_data(
        &gpu.queue,
        &wgpu::TextureDescriptor {
            label: Some("jup_solid"),
            size: wgpu::Extent3d { width: 1, height: 1, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        },
        wgpu::util::TextureDataOrder::LayerMajor,
        &rgba,
    );
    texture.create_view(&wgpu::TextureViewDescriptor::default())
}

/// An f32 as IEEE half bits (round toward zero; the values are light levels).
fn f32_to_f16(v: f32) -> u16 {
    let b = v.to_bits();
    let sign = ((b >> 16) & 0x8000) as u16;
    let exp = ((b >> 23) & 0xff) as i32 - 127 + 15;
    let man = b & 0x007f_ffff;
    if exp >= 31 {
        return sign | 0x7bff;
    }
    if exp <= 0 {
        if exp < -10 {
            return sign;
        }
        let m = (man | 0x0080_0000) >> (1 - exp);
        return sign | (m >> 13) as u16;
    }
    sign | ((exp as u16) << 10) | (man >> 13) as u16
}

/// The five side planes (left, right, bottom, top, near) of a clip matrix;
/// a point is inside when its distance to each is not negative. (The far
/// plane is left out: the camera's is at infinity.)
fn frustum_planes(m: &Mat4) -> [glam::Vec4; 5] {
    let (r0, r1, r2, r3) = (m.row(0), m.row(1), m.row(2), m.row(3));
    [r3 + r0, r3 - r0, r3 + r1, r3 - r1, r2]
}

fn box_visible(planes: &[glam::Vec4; 5], lo: Vec3, hi: Vec3) -> bool {
    planes.iter().all(|p| {
        // The box's corner furthest along the plane's normal.
        let c = Vec3::new(if p.x >= 0.0 { hi.x } else { lo.x }, if p.y >= 0.0 { hi.y } else { lo.y }, if p.z >= 0.0 { hi.z } else { lo.z });
        p.x * c.x + p.y * c.y + p.z * c.z + p.w >= 0.0
    })
}

fn vertex_layouts() -> [wgpu::VertexBufferLayout<'static>; 2] {
    const VERTEX: [wgpu::VertexAttribute; 5] = wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3, 2 => Float32x2, 7 => Unorm8x4, 8 => Float32x2];
    const INSTANCE: [wgpu::VertexAttribute; 4] = wgpu::vertex_attr_array![3 => Float32x4, 4 => Float32x4, 5 => Float32x4, 6 => Float32x4];
    [
        wgpu::VertexBufferLayout { array_stride: std::mem::size_of::<Vertex>() as u64, step_mode: wgpu::VertexStepMode::Vertex, attributes: &VERTEX },
        wgpu::VertexBufferLayout { array_stride: std::mem::size_of::<Instance>() as u64, step_mode: wgpu::VertexStepMode::Instance, attributes: &INSTANCE },
    ]
}

fn uniform_entry(binding: u32, vis: wgpu::ShaderStages) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: vis,
        ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: None },
        count: None,
    }
}

fn texture_entry(binding: u32, vis: wgpu::ShaderStages, sample: wgpu::TextureSampleType) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: vis,
        ty: wgpu::BindingType::Texture { sample_type: sample, view_dimension: wgpu::TextureViewDimension::D2, multisampled: false },
        count: None,
    }
}

fn sun_matrix(eye: Vec3, forward: Vec3, sun: Vec3, size: u32, span: f32) -> (Mat4, f32) {
    let r = SHADOW_REACH;
    let up = if sun.z.abs() > 0.99 { Vec3::X } else { Vec3::Z };
    let mut center = eye + forward * (r * 0.5);
    let back = span.max(4000.0);
    // Snap the centre to the shadow texels so the map does not shimmer.
    let right = sun.cross(up).normalize_or_zero();
    let up_l = right.cross(sun).normalize_or_zero();
    let texel = 2.0 * r / size as f32;
    let (a, b) = (center.dot(right), center.dot(up_l));
    center -= right * a.rem_euclid(texel) + up_l * b.rem_euclid(texel);
    let view = Mat4::look_at_rh(center + sun * back, center, up);
    let proj = Mat4::orthographic_rh(-r, r, -r, r, 1.0, 2.0 * back);
    (proj * view, texel)
}

impl Host {
    /// Makes the host on the device: the buffers, the pictures, every
    /// pipeline. An error is a pipeline or binding the device rejected.
    pub fn new(gpu: Gpu, scene: Scene) -> Result<Self, String> {
        let t0 = std::time::Instant::now();
        let mut report: Vec<String> = Vec::new();
        let device = &gpu.device;
        let limits = device.limits();
        let vbytes = (scene.vertices.len() * std::mem::size_of::<Vertex>()) as u64;
        let ibytes = (scene.indices.len() * 4) as u64;
        if vbytes.max(ibytes) > limits.max_buffer_size {
            return Err(format!("the map's mesh ({vbytes} B vertices, {ibytes} B indices) is over the device's buffer limit {}", limits.max_buffer_size));
        }
        let scope = device.push_error_scope(wgpu::ErrorFilter::Validation);

        let vertex_buf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor { label: Some("jup_vertices"), contents: bytemuck::cast_slice(&scene.vertices), usage: wgpu::BufferUsages::VERTEX });
        let index_buf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor { label: Some("jup_indices"), contents: bytemuck::cast_slice(&scene.indices), usage: wgpu::BufferUsages::INDEX });
        let instance_buf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor { label: Some("jup_instances"), contents: bytemuck::cast_slice(&scene.instances), usage: wgpu::BufferUsages::VERTEX });
        let globals_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("jup_globals"),
            size: std::mem::size_of::<Globals>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let mut probe_data: Vec<f32> = Vec::new();
        for p in &scene.probes {
            for v in [p.origin, p.up, p.side, p.down] {
                probe_data.extend([v[0], v[1], v[2], 0.0]);
            }
        }
        let probe_count = scene.probes.len();
        if probe_data.is_empty() {
            probe_data = vec![0.0; 16];
        }
        // (After the probes, the sky visibility grid's heights, four to a vec4.)
        let grid_base = probe_data.len() / 4;
        let (grid_cols, grid_rows, grid_origin, grid_cell) = match &scene.heights {
            Some(h) => {
                probe_data.extend_from_slice(&h.heights);
                while probe_data.len() % 4 != 0 {
                    probe_data.push(crate::skylight::NO_HEIGHT);
                }
                (h.cols, h.rows, h.origin, h.cell)
            }
            None => (0, 0, [0.0; 2], 1.0),
        };
        let has_grid = scene.heights.is_some();
        report.push(format!("probe buffer: {} probes, {} height cells ({} MiB)", probe_count, grid_cols * grid_rows, probe_data.len() * 4 >> 20));
        let probe_buf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor { label: Some("jup_probes"), contents: bytemuck::cast_slice(&probe_data), usage: wgpu::BufferUsages::STORAGE });

        // Pictures and the material bind groups.
        let all_stages = wgpu::ShaderStages::VERTEX_FRAGMENT;
        let globals_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor { label: Some("jup_globals"), entries: &[uniform_entry(0, all_stages)] });
        let globals_bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("jup_globals"),
            layout: &globals_layout,
            entries: &[wgpu::BindGroupEntry { binding: 0, resource: globals_buf.as_entire_binding() }],
        });
        let material_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("jup_material"),
            entries: &[
                texture_entry(0, wgpu::ShaderStages::FRAGMENT, wgpu::TextureSampleType::Float { filterable: true }),
                texture_entry(1, wgpu::ShaderStages::FRAGMENT, wgpu::TextureSampleType::Float { filterable: true }),
                texture_entry(2, wgpu::ShaderStages::FRAGMENT, wgpu::TextureSampleType::Float { filterable: true }),
                texture_entry(3, wgpu::ShaderStages::FRAGMENT, wgpu::TextureSampleType::Float { filterable: true }),
                texture_entry(4, wgpu::ShaderStages::FRAGMENT, wgpu::TextureSampleType::Float { filterable: true }),
                texture_entry(5, wgpu::ShaderStages::FRAGMENT, wgpu::TextureSampleType::Float { filterable: true }),
                texture_entry(6, wgpu::ShaderStages::FRAGMENT, wgpu::TextureSampleType::Float { filterable: true }),
                texture_entry(7, wgpu::ShaderStages::FRAGMENT, wgpu::TextureSampleType::Float { filterable: true }),
                texture_entry(8, wgpu::ShaderStages::FRAGMENT, wgpu::TextureSampleType::Float { filterable: true }),
                texture_entry(9, wgpu::ShaderStages::FRAGMENT, wgpu::TextureSampleType::Float { filterable: true }),
                texture_entry(10, wgpu::ShaderStages::FRAGMENT, wgpu::TextureSampleType::Float { filterable: true }),
                wgpu::BindGroupLayoutEntry { binding: 11, visibility: wgpu::ShaderStages::FRAGMENT, ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering), count: None },
                uniform_entry(12, wgpu::ShaderStages::FRAGMENT),
            ],
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("jup_surface"),
            address_mode_u: wgpu::AddressMode::Repeat,
            address_mode_v: wgpu::AddressMode::Repeat,
            address_mode_w: wgpu::AddressMode::Repeat,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Linear,
            anisotropy_clamp: 8,
            ..Default::default()
        });
        let white = solid(&gpu, [255, 255, 255, 255], true);
        let flat_normal = solid(&gpu, [128, 128, 255, 128], false);
        let mid_mask = solid(&gpu, [0, 0, 0, 255], false);
        let n_img = scene.images.len();
        let (mut used_colour, mut used_normal, mut used_mask) = (vec![false; n_img], vec![false; n_img], vec![false; n_img]);
        for m in &scene.materials {
            for c in m.colour.iter().flatten() {
                used_colour[*c] = true;
            }
            for c in m.normal.iter().flatten() {
                used_normal[*c] = true;
            }
            for c in m.mask.iter().flatten() {
                used_mask[*c] = true;
            }
        }
        let mut upload_errors = Vec::new();
        let mut colour_views: Vec<Option<wgpu::TextureView>> = Vec::with_capacity(n_img);
        let mut normal_views: Vec<Option<wgpu::TextureView>> = Vec::with_capacity(n_img);
        let mut mask_views: Vec<Option<wgpu::TextureView>> = Vec::with_capacity(n_img);
        for (i, im) in scene.images.iter().enumerate() {
            let mut put = |want: bool, srgb: bool| -> Option<wgpu::TextureView> {
                if !want {
                    return None;
                }
                match upload(&gpu, im, srgb) {
                    Ok(v) => Some(v),
                    Err(e) => {
                        upload_errors.push(e);
                        None
                    }
                }
            };
            colour_views.push(put(used_colour[i], true));
            normal_views.push(put(used_normal[i], false));
            mask_views.push(put(used_mask[i], false));
        }
        let mut material_binds = Vec::with_capacity(scene.materials.len());
        let mut without_colour = 0usize;
        for m in &scene.materials {
            let colour: Vec<Option<&wgpu::TextureView>> = m.colour.iter().map(|c| c.and_then(|c| colour_views[c].as_ref())).collect();
            let normal: Vec<Option<&wgpu::TextureView>> = m.normal.iter().map(|c| c.and_then(|c| normal_views[c].as_ref())).collect();
            let mask: Vec<Option<&wgpu::TextureView>> = m.mask.iter().map(|c| c.and_then(|c| mask_views[c].as_ref())).collect();
            without_colour += usize::from(colour[0].is_none());
            let has = |v: Option<&wgpu::TextureView>| f32::from(u8::from(v.is_some()));
            // (A layer counts when its colour loaded; an upper layer with no mask of
            // its own is blended by the vertices' painted weight alone, the shader
            // told by bit l - 1 of the mask word.)
            let layers = 1 + (1..4).take_while(|&l| usize::from(m.count) > l && colour[l].is_some()).count();
            let mask_bits = (1..4).filter(|&l| mask[l - 1].is_some()).fold(0u32, |b, l| b | (1 << (l - 1)))
                | (1..4).filter(|&l| m.second_uv[l]).fold(0u32, |b, l| b | (1 << (l + 3)));
            let t = |l: usize| [m.layer_tint[l][0], m.layer_tint[l][1], m.layer_tint[l][2], 0.0];
            let tints: [f32; 16] = [t(0), t(1), t(2), t(3)].concat().try_into().unwrap_or([1.0; 16]);
            let flags: [f32; 28] = [

                layers as f32,
                f32::from(u8::from(m.cutout && colour[0].is_some())),
                has(colour[0]),
                mask_bits as f32,
                has(normal[0]),
                has(normal[1]),
                has(normal[2]),
                has(normal[3]),
                m.paint.map_or(m.tint[0], |p| p[0]),
                m.paint.map_or(m.tint[1], |p| p[1]),
                m.paint.map_or(m.tint[2], |p| p[2]),
                f32::from(u8::from(m.paint.is_some())),
                tints[0], tints[1], tints[2], tints[3],
                tints[4], tints[5], tints[6], tints[7],
                tints[8], tints[9], tints[10], tints[11],
                tints[12], tints[13], tints[14], tints[15],
            ];
            let ubuf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor { label: Some("jup_surface_flags"), contents: bytemuck::cast_slice(&flags), usage: wgpu::BufferUsages::UNIFORM });
            let c = |k: usize| wgpu::BindingResource::TextureView(colour[k].unwrap_or(&white));
            let n = |k: usize| wgpu::BindingResource::TextureView(normal[k].unwrap_or(&flat_normal));
            let k = |k: usize| wgpu::BindingResource::TextureView(mask[k].unwrap_or(&mid_mask));
            material_binds.push(device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("jup_material"),
                layout: &material_layout,
                entries: &[
                    wgpu::BindGroupEntry { binding: 0, resource: c(0) },
                    wgpu::BindGroupEntry { binding: 1, resource: c(1) },
                    wgpu::BindGroupEntry { binding: 2, resource: c(2) },
                    wgpu::BindGroupEntry { binding: 3, resource: c(3) },
                    wgpu::BindGroupEntry { binding: 4, resource: n(0) },
                    wgpu::BindGroupEntry { binding: 5, resource: n(1) },
                    wgpu::BindGroupEntry { binding: 6, resource: n(2) },
                    wgpu::BindGroupEntry { binding: 7, resource: n(3) },
                    wgpu::BindGroupEntry { binding: 8, resource: k(0) },
                    wgpu::BindGroupEntry { binding: 9, resource: k(1) },
                    wgpu::BindGroupEntry { binding: 10, resource: k(2) },
                    wgpu::BindGroupEntry { binding: 11, resource: wgpu::BindingResource::Sampler(&sampler) },
                    wgpu::BindGroupEntry { binding: 12, resource: ubuf.as_entire_binding() },
                ],
            }));
        }
        report.push(format!(
            "pictures on the device: {} colour, {} normal, {} mask ({} materials without a colour picture){}",
            colour_views.iter().flatten().count(),
            normal_views.iter().flatten().count(),
            mask_views.iter().flatten().count(),
            without_colour,
            upload_errors.first().map(|e| format!("; first not drawn: {e}")).unwrap_or_default()
        ));

        // The probes' radiance pictures: one array layer each, with mips.
        let probe_layers = scene.probes.len().max(1) as u32;
        let probe_levels = (PROBE_SIZE as u32).ilog2() + 1;
        let probe_tex = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("jup_probe_maps"),
            size: wgpu::Extent3d { width: PROBE_SIZE as u32, height: PROBE_SIZE as u32, depth_or_array_layers: probe_layers },
            mip_level_count: probe_levels,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba16Float,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        for layer in 0..probe_layers as usize {
            let mut level: Vec<[f32; 3]> = scene.probes.get(layer).map(|p| p.radiance.clone()).unwrap_or_else(|| vec![[0.0; 3]; PROBE_SIZE * PROBE_SIZE]);
            let mut size = PROBE_SIZE;
            for mip in 0..probe_levels {
                let mut bytes: Vec<u8> = Vec::with_capacity(size * size * 8);
                for c in &level {
                    for v in [c[0], c[1], c[2], 1.0] {
                        bytes.extend(f32_to_f16(v.clamp(0.0, 60000.0)).to_le_bytes());
                    }
                }
                gpu.queue.write_texture(
                    wgpu::TexelCopyTextureInfo { texture: &probe_tex, mip_level: mip, origin: wgpu::Origin3d { x: 0, y: 0, z: layer as u32 }, aspect: wgpu::TextureAspect::All },
                    &bytes,
                    wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(size as u32 * 8), rows_per_image: Some(size as u32) },
                    wgpu::Extent3d { width: size as u32, height: size as u32, depth_or_array_layers: 1 },
                );
                let half = (size / 2).max(1);
                let mut next = vec![[0.0f32; 3]; half * half];
                for y in 0..half {
                    for x in 0..half {
                        let mut acc = [0.0f32; 3];
                        for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                            let c = level[((2 * y + dy).min(size - 1)) * size + (2 * x + dx).min(size - 1)];
                            for k in 0..3 {
                                acc[k] += c[k] * 0.25;
                            }
                        }
                        next[y * half + x] = acc;
                    }
                }
                level = next;
                size = half;
            }
        }
        let probe_view = probe_tex.create_view(&wgpu::TextureViewDescriptor { dimension: Some(wgpu::TextureViewDimension::D2Array), ..Default::default() });
        // The map's sky as a cube (a black 1x1 one where it has none).
        let sky_size = scene.sky.as_ref().map_or(1, |s| s.size);
        let sky_tex = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("jup_sky"),
            size: wgpu::Extent3d { width: sky_size, height: sky_size, depth_or_array_layers: 6 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let sky_bytes: Vec<u8> = scene.sky.as_ref().map_or_else(|| [0u8, 0, 0, 255].repeat(6), |s| s.rgba.clone());
        gpu.queue.write_texture(
            wgpu::TexelCopyTextureInfo { texture: &sky_tex, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
            &sky_bytes,
            wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(sky_size * 4), rows_per_image: Some(sky_size) },
            wgpu::Extent3d { width: sky_size, height: sky_size, depth_or_array_layers: 6 },
        );
        let sky_view = sky_tex.create_view(&wgpu::TextureViewDescriptor { dimension: Some(wgpu::TextureViewDimension::Cube), ..Default::default() });
        let has_sky = scene.sky.is_some();
        // The colour grade as a half-float volume (a black 1x1x1 one without).
        let grade_n: u32 = if scene.grade.is_some() { 32 } else { 1 };
        let grade_tex = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("jup_grade"),
            size: wgpu::Extent3d { width: grade_n, height: grade_n, depth_or_array_layers: grade_n },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D3,
            format: wgpu::TextureFormat::Rgba16Float,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let grade_bytes: Vec<u8> = match &scene.grade {
            Some(g) => g.iter().flat_map(|&v| f32_to_f16(f32::from(v) / 65535.0).to_le_bytes()).collect(),
            None => vec![0u8; 8],
        };
        gpu.queue.write_texture(
            wgpu::TexelCopyTextureInfo { texture: &grade_tex, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
            &grade_bytes,
            wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(grade_n * 8), rows_per_image: Some(grade_n) },
            wgpu::Extent3d { width: grade_n, height: grade_n, depth_or_array_layers: grade_n },
        );
        let grade_view = grade_tex.create_view(&wgpu::TextureViewDescriptor::default());
        let grade_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("jup_grade"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let has_grade = scene.grade.is_some();
        let probe_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("jup_probe"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Linear,
            ..Default::default()
        });

        // Shadow map.
        let shadows = env_flag("BO2MP_JUP_SHADOW", true);
        let shadow_size: u32 = std::env::var("BO2MP_JUP_SHADOW_SIZE").ok().and_then(|v| v.trim().parse().ok()).unwrap_or(4096u32).clamp(512, limits.max_texture_dimension_2d.min(8192));
        let shadow_tex = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("jup_shadow"),
            size: wgpu::Extent3d { width: shadow_size, height: shadow_size, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: SHADOW_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let shadow_view = shadow_tex.create_view(&wgpu::TextureViewDescriptor::default());
        let shadow_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("jup_shadow"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            compare: Some(wgpu::CompareFunction::LessEqual),
            ..Default::default()
        });

        // Pipelines.
        let fallback = shaders::material();
        let fallback_mod = device.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some(fallback.label), source: wgpu::ShaderSource::Wgsl(fallback.source.into()) });
        let layouts = vertex_layouts();
        let shadow_pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: Some("jup_shadow"), bind_group_layouts: &[Some(&globals_layout)], immediate_size: 0 });
        let shadow_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("jup_shadow"),
            layout: Some(&shadow_pl),
            vertex: wgpu::VertexState { module: &fallback_mod, entry_point: Some("vs_shadow"), compilation_options: Default::default(), buffers: &layouts },
            fragment: None,
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: Some(wgpu::DepthStencilState {
                format: SHADOW_FORMAT,
                depth_write_enabled: Some(true),
                depth_compare: Some(wgpu::CompareFunction::Less),
                stencil: Default::default(),
                bias: wgpu::DepthBiasState { constant: 2, slope_scale: 2.0, clamp: 0.0 },
            }),
            multisample: Default::default(),
            multiview_mask: None,
            cache: None,
        });
        let gbuffer_pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: Some("jup_gbuffer"), bind_group_layouts: &[Some(&globals_layout), Some(&material_layout)], immediate_size: 0 });
        let gbuffer_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("jup_gbuffer"),
            layout: Some(&gbuffer_pl),
            vertex: wgpu::VertexState { module: &fallback_mod, entry_point: Some("vs_main"), compilation_options: Default::default(), buffers: &layouts },
            fragment: Some(wgpu::FragmentState {
                module: &fallback_mod,
                entry_point: Some("fs_main"),
                compilation_options: Default::default(),
                targets: &[
                    Some(wgpu::ColorTargetState { format: ALBEDO, blend: None, write_mask: wgpu::ColorWrites::ALL }),
                    Some(wgpu::ColorTargetState { format: NORMAL, blend: None, write_mask: wgpu::ColorWrites::ALL }),
                ],
            }),
            // (Two-sided: the maps' windings are not relied on.)
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: Some(wgpu::DepthStencilState {
                format: SCENE_DEPTH,
                depth_write_enabled: Some(true),
                // (Reversed depth: 1 near, 0 at infinity.)
                depth_compare: Some(wgpu::CompareFunction::Greater),
                stencil: Default::default(),
                bias: Default::default(),
            }),
            multisample: Default::default(),
            multiview_mask: None,
            cache: None,
        });

        let light = shaders::light();
        let light_mod = device.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some(light.label), source: wgpu::ShaderSource::Wgsl(light.source.into()) });
        let frag = wgpu::ShaderStages::FRAGMENT;
        let light_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("jup_light"),
            entries: &[
                texture_entry(0, frag, wgpu::TextureSampleType::Float { filterable: false }),
                texture_entry(1, frag, wgpu::TextureSampleType::Float { filterable: false }),
                texture_entry(2, frag, wgpu::TextureSampleType::Depth),
                texture_entry(3, frag, wgpu::TextureSampleType::Depth),
                wgpu::BindGroupLayoutEntry { binding: 4, visibility: frag, ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Comparison), count: None },
                wgpu::BindGroupLayoutEntry {
                    binding: 5,
                    visibility: frag,
                    ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Storage { read_only: true }, has_dynamic_offset: false, min_binding_size: None },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 6,
                    visibility: frag,
                    ty: wgpu::BindingType::Texture { sample_type: wgpu::TextureSampleType::Float { filterable: true }, view_dimension: wgpu::TextureViewDimension::D2Array, multisampled: false },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry { binding: 7, visibility: frag, ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering), count: None },
                wgpu::BindGroupLayoutEntry {
                    binding: 8,
                    visibility: frag,
                    ty: wgpu::BindingType::Texture { sample_type: wgpu::TextureSampleType::Float { filterable: true }, view_dimension: wgpu::TextureViewDimension::Cube, multisampled: false },
                    count: None,
                },
            ],
        });
        let light_pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: Some("jup_light"), bind_group_layouts: &[Some(&globals_layout), Some(&light_layout)], immediate_size: 0 });
        let full_screen = |label: &'static str, module: &wgpu::ShaderModule, fs: &'static str, layout: &wgpu::PipelineLayout, format: wgpu::TextureFormat| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(label),
                layout: Some(layout),
                vertex: wgpu::VertexState { module, entry_point: Some("vs_full"), compilation_options: Default::default(), buffers: &[] },
                fragment: Some(wgpu::FragmentState {
                    module,
                    entry_point: Some(fs),
                    compilation_options: Default::default(),
                    targets: &[Some(wgpu::ColorTargetState { format, blend: None, write_mask: wgpu::ColorWrites::ALL })],
                }),
                primitive: wgpu::PrimitiveState::default(),
                depth_stencil: None,
                multisample: Default::default(),
                multiview_mask: None,
                cache: None,
            })
        };
        let light_pipeline = full_screen("jup_light", &light_mod, "fs_light", &light_pl, HDR);
        let display = shaders::display();
        let display_mod = device.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some(display.label), source: wgpu::ShaderSource::Wgsl(display.source.into()) });
        let display_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("jup_display"),
            entries: &[
                uniform_entry(0, frag),
                texture_entry(1, frag, wgpu::TextureSampleType::Float { filterable: false }),
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: frag,
                    ty: wgpu::BindingType::Texture { sample_type: wgpu::TextureSampleType::Float { filterable: true }, view_dimension: wgpu::TextureViewDimension::D3, multisampled: false },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry { binding: 3, visibility: frag, ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering), count: None },
            ],
        });
        let display_pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: Some("jup_display"), bind_group_layouts: &[Some(&display_layout)], immediate_size: 0 });
        let display_pipeline = full_screen("jup_display", &display_mod, "fs_display", &display_pl, FINAL);

        // The blended pass: the material shader's vertex stage, the forward
        // and water fragment stages, over the lit image.
        let fwd = shaders::forward();
        let fwd_mod = device.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some(fwd.label), source: wgpu::ShaderSource::Wgsl(fwd.source.into()) });
        let fwd_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("jup_forward"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: frag,
                    ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Storage { read_only: true }, has_dynamic_offset: false, min_binding_size: None },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: frag,
                    ty: wgpu::BindingType::Texture { sample_type: wgpu::TextureSampleType::Float { filterable: true }, view_dimension: wgpu::TextureViewDimension::D2Array, multisampled: false },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry { binding: 2, visibility: frag, ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering), count: None },
                wgpu::BindGroupLayoutEntry {
                    binding: 3,
                    visibility: frag,
                    ty: wgpu::BindingType::Texture { sample_type: wgpu::TextureSampleType::Float { filterable: true }, view_dimension: wgpu::TextureViewDimension::Cube, multisampled: false },
                    count: None,
                },
            ],
        });
        let forward_bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("jup_forward"),
            layout: &fwd_layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: probe_buf.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&probe_view) },
                wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::Sampler(&probe_sampler) },
                wgpu::BindGroupEntry { binding: 3, resource: wgpu::BindingResource::TextureView(&sky_view) },
            ],
        });
        let fwd_pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: Some("jup_forward"), bind_group_layouts: &[Some(&globals_layout), Some(&material_layout), Some(&fwd_layout)], immediate_size: 0 });
        let blend = wgpu::BlendState {
            color: wgpu::BlendComponent { src_factor: wgpu::BlendFactor::SrcAlpha, dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha, operation: wgpu::BlendOperation::Add },
            alpha: wgpu::BlendComponent { src_factor: wgpu::BlendFactor::One, dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha, operation: wgpu::BlendOperation::Add },
        };
        let forward_stage = |label: &'static str, fs: &'static str| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(label),
                layout: Some(&fwd_pl),
                vertex: wgpu::VertexState { module: &fwd_mod, entry_point: Some("vs_main"), compilation_options: Default::default(), buffers: &layouts },
                fragment: Some(wgpu::FragmentState {
                    module: &fwd_mod,
                    entry_point: Some(fs),
                    compilation_options: Default::default(),
                    targets: &[Some(wgpu::ColorTargetState { format: HDR, blend: Some(blend), write_mask: wgpu::ColorWrites::ALL })],
                }),
                primitive: wgpu::PrimitiveState::default(),
                depth_stencil: Some(wgpu::DepthStencilState {
                    format: SCENE_DEPTH,
                    depth_write_enabled: Some(false),
                    depth_compare: Some(wgpu::CompareFunction::Greater),
                    stencil: Default::default(),
                    bias: Default::default(),
                }),
                multisample: Default::default(),
                multiview_mask: None,
                cache: None,
            })
        };
        let forward_pipeline = forward_stage("jup_forward", "fs_forward");
        let water_pipeline = forward_stage("jup_water", "fs_water");

        if let Some(e) = pollster::block_on(scope.pop()) {
            return Err(format!("the device rejected a pipeline or binding: {e}"));
        }

        let exposure = std::env::var("BO2MP_JUP_EXPOSURE").ok().and_then(|v| v.trim().parse::<f32>().ok()).filter(|v| *v > 0.0).unwrap_or(0.5);
        report.push(format!(
            "host made in {:.1}s: {} draws ({} world surfaces, {} model draws over {} placements), {} materials on the layered material shader (the game's material-to-program mapping is not in the zones), {} probes, shadows {} ({shadow_size}^2), exposure {exposure}",
            t0.elapsed().as_secs_f32(),
            scene.draws.len(),
            scene.world_surfaces,
            scene.model_draws,
            scene.placements,
            scene.materials.len(),
            probe_count,
            if shadows { "on" } else { "off" },
        ));
        let water_material: Vec<bool> = scene.materials.iter().map(|m| m.water).collect();
        let tool_material: Vec<bool> = scene.materials.iter().map(|m| m.tool).collect();
        let (blended, opaque): (Vec<_>, Vec<_>) = scene.draws.iter().copied().partition(|d| scene.materials.get(d.material as usize).is_some_and(|m| m.blend || m.water));
        let check = Arc::new(Mutex::new(Vec::new()));
        crate::coverage::spawn_check(gpu.clone(), scene.install.clone(), scene.name.clone(), scene.materials.len(), Arc::clone(&check));
        Ok(Self {
            gpu,
            scene_name: scene.name.clone(),
            mins: scene.mins,
            maxs: scene.maxs,
            globals_buf,
            globals_bind,
            vertex_buf,
            index_buf,
            instance_buf,
            draws: opaque,
            blended,
            water_material,
            tool_material,
            forward_pipeline,
            water_pipeline,
            forward_bind,
            material_binds,
            shadow_pipeline,
            shadow_view,
            shadow_size,
            gbuffer_pipeline,
            light_pipeline,
            light_layout,
            display_pipeline,
            display_layout,
            shadow_sampler,
            probe_buf,
            probe_view,
            sky_view,
            has_sky,
            grade_view,
            grade_sampler,
            has_grade,
            probe_sampler,
            probe_count,
            grid_base,
            grid_cols,
            grid_rows,
            grid_origin,
            grid_cell,
            has_grid,
            targets: None,
            env: scene.env,
            shadows,
            exposure,
            ssao: std::env::var("BO2MP_JUP_SSAO").ok().and_then(|v| v.trim().parse::<f32>().ok()).unwrap_or(1.0).max(0.0),
            tonemap: if std::env::var("BO2MP_JUP_TONEMAP").is_ok_and(|v| v == "shoulder") { 0.0 } else { 1.0 },
            stats: Stats::default(),
            report,
            check,
            complete: false,
        })
    }

    /// The host is made when `new` returns; kept so the glue reads like
    /// `render_t7`'s.
    pub fn poll(&mut self) -> bool {
        true
    }

    /// Log lines since the last call (the host's, then the check thread's).
    pub fn take_report(&mut self) -> Vec<String> {
        let mut out = std::mem::take(&mut self.report);
        if let Ok(mut c) = self.check.lock() {
            out.append(&mut c);
        }
        out
    }

    fn make_targets(&self, width: u32, height: u32) -> Targets {
        let device = &self.gpu.device;
        let make = |label: &'static str, format: wgpu::TextureFormat| {
            device.create_texture(&wgpu::TextureDescriptor {
                label: Some(label),
                size: wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_SRC,
                view_formats: &[],
            })
        };
        let depth_t = make("jup_depth", SCENE_DEPTH);
        let albedo_t = make("jup_albedo", ALBEDO);
        let normal_t = make("jup_normal", NORMAL);
        let hdr_t = make("jup_hdr", HDR);
        let final_t = make("jup_final", FINAL);
        let view = |t: &wgpu::Texture| t.create_view(&wgpu::TextureViewDescriptor::default());
        let depth = view(&depth_t);
        let depth_only = depth_t.create_view(&wgpu::TextureViewDescriptor { aspect: wgpu::TextureAspect::DepthOnly, ..Default::default() });
        let (albedo, normal, hdr, final_view) = (view(&albedo_t), view(&normal_t), view(&hdr_t), view(&final_t));
        let light_bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("jup_light"),
            layout: &self.light_layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&albedo) },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&normal) },
                wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::TextureView(&depth_only) },
                wgpu::BindGroupEntry { binding: 3, resource: wgpu::BindingResource::TextureView(&self.shadow_view) },
                wgpu::BindGroupEntry { binding: 4, resource: wgpu::BindingResource::Sampler(&self.shadow_sampler) },
                wgpu::BindGroupEntry { binding: 5, resource: self.probe_buf.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 6, resource: wgpu::BindingResource::TextureView(&self.probe_view) },
                wgpu::BindGroupEntry { binding: 7, resource: wgpu::BindingResource::Sampler(&self.probe_sampler) },
                wgpu::BindGroupEntry { binding: 8, resource: wgpu::BindingResource::TextureView(&self.sky_view) },
            ],
        });
        let display_bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("jup_display"),
            layout: &self.display_layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: self.globals_buf.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&hdr) },
                wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::TextureView(&self.grade_view) },
                wgpu::BindGroupEntry { binding: 3, resource: wgpu::BindingResource::Sampler(&self.grade_sampler) },
            ],
        });
        Targets { width, height, depth, depth_only, albedo, normal, hdr, final_view, light_bind, display_bind, _textures: vec![depth_t, albedo_t, normal_t, hdr_t, final_t] }
    }

    /// Draws every draw whose box is inside `cull` (a view-projection and the
    /// offset subtracted from world places first; `None` draws all).
    fn draw_all(&self, pass: &mut wgpu::RenderPass<'_>, materials: bool, cull: Option<(Mat4, Vec3)>) {
        pass.set_vertex_buffer(0, self.vertex_buf.slice(..));
        pass.set_vertex_buffer(1, self.instance_buf.slice(..));
        pass.set_index_buffer(self.index_buf.slice(..), wgpu::IndexFormat::Uint32);
        let planes = cull.map(|(m, off)| (frustum_planes(&m), off));
        let mut last = u32::MAX;
        for d in &self.draws {
            if let Some((pl, off)) = &planes
                && !box_visible(pl, Vec3::from_array(d.mins) - *off, Vec3::from_array(d.maxs) - *off)
            {
                continue;
            }
            // (A tool material's surfaces only cast shadows.)
            if materials && self.tool_material.get(d.material as usize).copied().unwrap_or(false) {
                continue;
            }
            if materials && d.material != last {
                pass.set_bind_group(1, &self.material_binds[d.material as usize], &[]);
                last = d.material;
            }
            pass.draw_indexed(d.first_index..d.first_index + d.index_count, 0, d.first_instance..d.first_instance + d.instances);
        }
    }

    /// The blended draws over the lit image, far to near.
    fn forward_pass(&self, enc: &mut wgpu::CommandEncoder, t: &Targets, camera: &Camera, cull_on: bool) {
        if self.blended.is_empty() {
            return;
        }
        let planes = cull_on.then(|| frustum_planes(&camera.view_projection));
        let mut order: Vec<(f32, usize)> = Vec::with_capacity(self.blended.len());
        for (k, d) in self.blended.iter().enumerate() {
            if self.tool_material.get(d.material as usize).copied().unwrap_or(false) {
                continue;
            }
            let (lo, hi) = (Vec3::from_array(d.mins) - camera.eye, Vec3::from_array(d.maxs) - camera.eye);
            if planes.as_ref().is_some_and(|p| !box_visible(p, lo, hi)) {
                continue;
            }
            // (The sea is drawn first, whatever its centre.)
            let far = if self.water_material.get(d.material as usize).copied().unwrap_or(false) { f32::MAX } else { ((lo + hi) * 0.5).length() };
            order.push((far, k));
        }
        if order.is_empty() {
            return;
        }
        order.sort_by(|a, b| b.0.total_cmp(&a.0));
        let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("jup_forward"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment { view: &t.hdr, depth_slice: None, resolve_target: None, ops: wgpu::Operations { load: wgpu::LoadOp::Load, store: wgpu::StoreOp::Store } })],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment { view: &t.depth, depth_ops: None, stencil_ops: None }),
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        pass.set_bind_group(0, &self.globals_bind, &[]);
        pass.set_bind_group(2, &self.forward_bind, &[]);
        pass.set_vertex_buffer(0, self.vertex_buf.slice(..));
        pass.set_vertex_buffer(1, self.instance_buf.slice(..));
        pass.set_index_buffer(self.index_buf.slice(..), wgpu::IndexFormat::Uint32);
        let mut last = (u32::MAX, false);
        for (_, k) in order {
            let d = &self.blended[k];
            let water = self.water_material.get(d.material as usize).copied().unwrap_or(false);
            if last.1 != water || last.0 == u32::MAX {
                pass.set_pipeline(if water { &self.water_pipeline } else { &self.forward_pipeline });
            }
            if last.0 != d.material {
                pass.set_bind_group(1, &self.material_binds[d.material as usize], &[]);
            }
            last = (d.material, water);
            pass.draw_indexed(d.first_index..d.first_index + d.index_count, 0, d.first_instance..d.first_instance + d.instances);
        }
    }

    /// Records the frame for `camera` into `enc`. `Ok(true)` when the
    /// finished image and depth are ready for the hand-off.
    pub fn record(&mut self, enc: &mut wgpu::CommandEncoder, width: u32, height: u32, camera: &Camera) -> Result<bool, String> {
        if width == 0 || height == 0 {
            return Ok(false);
        }
        if self.targets.as_ref().is_none_or(|t| t.width != width || t.height != height) {
            self.targets = Some(self.make_targets(width, height));
            self.report.push(format!("targets {width}x{height}"));
        }
        let sun = Vec3::from_array(self.env.sun_dir).normalize_or_zero();
        let sun = if sun == Vec3::ZERO { Vec3::Z } else { sun };
        let cull_on = env_flag("BO2MP_JUP_CULL", true);
        let rot = camera.rotation;
        let forward = Vec3::new(rot.x_axis.z, rot.y_axis.z, rot.z_axis.z).normalize_or_zero();
        let span = Vec3::from_array(self.maxs).distance(Vec3::from_array(self.mins));
        let (sun_vp, texel) = sun_matrix(camera.eye, forward, sun, self.shadow_size, span);
        let inv = camera.view_projection.inverse();
        let fog = self.env.fog;
        let v4 = |v: [f32; 3], w: f32| [v[0], v[1], v[2], w];
        let g = Globals {
            vp: camera.view_projection.to_cols_array_2d(),
            inv_vp: inv.to_cols_array_2d(),
            sun_vp: sun_vp.to_cols_array_2d(),
            eye: [camera.eye.x, camera.eye.y, camera.eye.z, 0.0],
            sun_dir: [sun.x, sun.y, sun.z, 0.0],
            sun_col: v4(self.env.sun_colour, 0.0),
            fog: fog.map_or([0.0; 4], |f| [f.start, f.half, f.opacity, f.normal_scale]),
            fog_col: fog.map_or([0.0; 4], |f| v4(f.colour, 1.0)),
            fog_sun: fog.map_or([0.0; 4], |f| v4(f.sun_colour, 0.0)),
            fog_misc: fog.map_or([0.0; 4], |f| [f.base_height, f.half_height, f.sun_begin_deg, f.sun_end_deg]),
            sky_zenith: v4(self.env.sky_zenith, f32::from(u8::from(self.has_sky))),
            sky_horizon: v4(self.env.sky_horizon, f32::from(u8::from(self.has_grade))),
            ambient: v4(self.env.ambient, self.probe_count as f32),
            amb_up: v4(self.env.amb_up, 0.0),
            amb_side: v4(self.env.amb_side, 0.0),
            amb_down: v4(self.env.amb_down, 0.0),
            grid: [self.grid_origin[0], self.grid_origin[1], self.grid_cell, self.grid_cols as f32],
            grid2: [self.grid_rows as f32, self.grid_base as f32, self.env.probe_reach[0], self.env.probe_reach[1]],
            amb_misc: [self.env.probe_weight, self.ssao, f32::from(u8::from(self.has_grid)), self.tonemap],
            size: [width as f32, height as f32, 1.0 / width as f32, 1.0 / height as f32],
            misc: [f32::from(u8::from(self.shadows)), self.exposure, texel, f32::from(u8::from(self.env.night))],
        };
        self.gpu.queue.write_buffer(&self.globals_buf, 0, bytemuck::bytes_of(&g));
        let Some(t) = self.targets.as_ref() else { return Ok(false) };

        if self.shadows {
            let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("jup_shadow"),
                color_attachments: &[],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &self.shadow_view,
                    depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(1.0), store: wgpu::StoreOp::Store }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_pipeline(&self.shadow_pipeline);
            pass.set_bind_group(0, &self.globals_bind, &[]);
            self.draw_all(&mut pass, false, cull_on.then_some((sun_vp, Vec3::ZERO)));
        }
        {
            let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("jup_gbuffer"),
                color_attachments: &[
                    Some(wgpu::RenderPassColorAttachment { view: &t.albedo, depth_slice: None, resolve_target: None, ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::BLACK), store: wgpu::StoreOp::Store } }),
                    Some(wgpu::RenderPassColorAttachment { view: &t.normal, depth_slice: None, resolve_target: None, ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::BLACK), store: wgpu::StoreOp::Store } }),
                ],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &t.depth,
                    depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(0.0), store: wgpu::StoreOp::Store }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_pipeline(&self.gbuffer_pipeline);
            pass.set_bind_group(0, &self.globals_bind, &[]);
            self.draw_all(&mut pass, true, cull_on.then_some((camera.view_projection, camera.eye)));
        }
        for (label, target, pipeline, bind0, bind1) in [
            ("jup_light", &t.hdr, &self.light_pipeline, Some(&self.globals_bind), &t.light_bind),
            ("jup_display", &t.final_view, &self.display_pipeline, None, &t.display_bind),
        ] {
            let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some(label),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment { view: target, depth_slice: None, resolve_target: None, ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::BLACK), store: wgpu::StoreOp::Store } })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_pipeline(pipeline);
            match bind0 {
                Some(b) => {
                    pass.set_bind_group(0, b, &[]);
                    pass.set_bind_group(1, bind1, &[]);
                }
                None => pass.set_bind_group(0, bind1, &[]),
            }
            pass.draw(0..3, 0..1);
            drop(pass);
            if label == "jup_light" {
                self.forward_pass(enc, t, camera, cull_on);
            }
        }
        self.stats.frames += 1;
        self.complete = true;
        if !self.stats.announced {
            self.stats.announced = true;
            self.report.push(format!("{}: first frame recorded ({} opaque draws, {} blended)", self.scene_name, self.draws.len(), self.blended.len()));
        }
        Ok(true)
    }

    /// The last frame's targets for a screenshot: display image, albedo and
    /// normal (with their formats `Rgba8Unorm`, `Rgba8UnormSrgb`, `Rgba16Float`).
    pub fn debug_textures(&self) -> Option<(&wgpu::Texture, &wgpu::Texture, &wgpu::Texture)> {
        let t = self.targets.as_ref()?;
        Some((t._textures.get(4)?, t._textures.get(1)?, t._textures.get(2)?))
    }

    /// The finished, display-encoded image (the viewport's size).
    pub fn final_image(&self) -> Option<&wgpu::TextureView> {
        self.complete.then(|| self.targets.as_ref().map(|t| &t.final_view)).flatten()
    }

    /// The scene depth (a depth-aspect view; reversed, 1 near and 0 far).
    pub fn scene_depth(&self) -> Option<&wgpu::TextureView> {
        self.complete.then(|| self.targets.as_ref().map(|t| &t.depth_only)).flatten()
    }

    /// The exposure the display pass uses.
    pub fn exposure(&self) -> f32 {
        self.exposure
    }

    pub fn end_frame(&mut self, _enc: &mut wgpu::CommandEncoder, _record_ms: f64) {}
}
