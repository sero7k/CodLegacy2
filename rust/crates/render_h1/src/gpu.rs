//! The GPU side: textures, samplers, translated pipelines and draws.

use std::collections::HashMap;

use dxbc_sm5::{Reflection, TextureDim, TextureKind};
use fastfile_h1::image::ImagePixels;
use wgpu::util::DeviceExt;

#[derive(Clone)]
pub struct Gpu {
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
    pub bc: bool,
    pub float32_filterable: bool,
}

impl Gpu {
    /// A host's device and queue.
    pub fn from_parts(device: wgpu::Device, queue: wgpu::Queue) -> Self {
        let f = device.features();
        Self {
            bc: f.contains(wgpu::Features::TEXTURE_COMPRESSION_BC),
            float32_filterable: f.contains(wgpu::Features::FLOAT32_FILTERABLE),
            device,
            queue,
        }
    }

    /// A device with no window (offline rendering, tests).
    pub fn headless() -> Result<Self, String> {
        let instance =
            wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env());
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            force_fallback_adapter: false,
            compatible_surface: None,
        }))
        .map_err(|e| format!("no adapter: {e}"))?;
        let have = adapter.features();
        let mut want = wgpu::Features::empty();
        for f in [
            wgpu::Features::TEXTURE_COMPRESSION_BC,
            wgpu::Features::TEXTURE_COMPRESSION_BC_SLICED_3D,
            wgpu::Features::FLOAT32_FILTERABLE,
            wgpu::Features::DEPTH32FLOAT_STENCIL8,
            // (The shadow comparison sampler's zero border, `comparison`.)
            wgpu::Features::ADDRESS_MODE_CLAMP_TO_BORDER,
            // (Per-pass frame timing, `render_t7::timing`.)
            wgpu::Features::TIMESTAMP_QUERY,
            wgpu::Features::TIMESTAMP_QUERY_INSIDE_ENCODERS,
            // (The G-buffer's culled indirect draws, `render_t7::cull`.)
            wgpu::Features::INDIRECT_FIRST_INSTANCE,
        ] {
            if have.contains(f) {
                want |= f;
            }
        }
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("render_h1"),
            required_features: want,
            // `T7_LIMITS=default`: the limits the game's device has (bevy
            // asks for wgpu's defaults, raised for resolution and
            // alignment only), so what the game's device refuses fails here.
            required_limits: if std::env::var("T7_LIMITS").is_ok_and(|v| v == "default") {
                let mut l = wgpu::Limits::default()
                    .using_resolution(adapter.limits())
                    .using_alignment(adapter.limits());
                // what the game raises on top (bootstrap `plugins.rs`)
                l.max_sampled_textures_per_shader_stage = 64;
                l.max_samplers_per_shader_stage = 32;
                l
            } else {
                adapter.limits()
            },
            ..Default::default()
        }))
        .map_err(|e| format!("no device: {e}"))?;
        Ok(Self {
            device,
            queue,
            bc: want.contains(wgpu::Features::TEXTURE_COMPRESSION_BC),
            float32_filterable: want.contains(wgpu::Features::FLOAT32_FILTERABLE),
        })
    }
}

/// The wgpu format of a DXGI format, and whether it is block compressed.
pub fn dxgi_format(f: u32) -> Option<wgpu::TextureFormat> {
    use wgpu::TextureFormat as T;
    Some(match f {
        2 => T::Rgba32Float,
        10 => T::Rgba16Float,
        11 => T::Rgba16Unorm,
        16 => T::Rg32Float,
        24 => T::Rgb10a2Unorm,
        26 => T::Rg11b10Ufloat,
        28 => T::Rgba8Unorm,
        29 => T::Rgba8UnormSrgb,
        34 => T::Rg16Float,
        35 => T::Rg16Unorm,
        37 => T::Rg16Snorm,
        41 => T::R32Float,
        49 => T::Rg8Unorm,
        54 => T::R16Float,
        56 => T::R16Unorm,
        61 => T::R8Unorm,
        71 => T::Bc1RgbaUnorm,
        72 => T::Bc1RgbaUnormSrgb,
        74 => T::Bc2RgbaUnorm,
        75 => T::Bc2RgbaUnormSrgb,
        77 => T::Bc3RgbaUnorm,
        78 => T::Bc3RgbaUnormSrgb,
        80 => T::Bc4RUnorm,
        81 => T::Bc4RSnorm,
        83 => T::Bc5RgUnorm,
        84 => T::Bc5RgSnorm,
        95 => T::Bc6hRgbUfloat,
        96 => T::Bc6hRgbFloat,
        98 => T::Bc7RgbaUnorm,
        99 => T::Bc7RgbaUnormSrgb,
        87 => T::Bgra8Unorm,
        91 => T::Bgra8UnormSrgb,
        _ => return None,
    })
}

/// A texture ready to bind, with the view dimension it was made for.
pub struct Tex {
    pub texture: wgpu::Texture,
    pub view: wgpu::TextureView,
    pub dim: wgpu::TextureViewDimension,
}

/// Uploads an image's levels (2-D, or a cube when `faces` is 6).
pub fn upload(gpu: &Gpu, label: &str, px: &ImagePixels, faces: u32) -> Result<Tex, String> {
    if label.contains("lightmap") && std::env::var("H1_DBG_LMINFO").is_ok() {
        eprintln!("lmpic {label}: dxgi {} {}x{} levels {}", px.dxgi_format, px.width, px.height, px.levels);
    }
    let format = dxgi_format(px.dxgi_format)
        .ok_or_else(|| format!("{label}: DXGI {} not drawn", px.dxgi_format))?;
    if format.is_compressed() && !gpu.bc {
        return Err(format!("{label}: no BC support"));
    }
    let (bw, bh) = format.block_dimensions();
    let bsize = format.block_copy_size(None).ok_or("format size")?;
    // Levels whose size isn't a whole number of blocks at the top are fine;
    // wgpu wants the top level's size a block multiple for BC.
    let w = px.width.max(1);
    let h = px.height.max(1);
    if format.is_compressed() && (!w.is_multiple_of(bw) || !h.is_multiple_of(bh)) {
        return Err(format!("{label}: {w}x{h} not a block multiple"));
    }
    let size = wgpu::Extent3d {
        width: w,
        height: h,
        depth_or_array_layers: faces,
    };
    let levels = px
        .levels
        .max(1)
        .min(size.max_mips(wgpu::TextureDimension::D2));
    let mut data = Vec::new();
    // DXGI: face by face, each with its levels; wgpu's
    // `create_texture_with_data` wants layer-major too (`LayerMajor`).
    let mut at = 0usize;
    for _ in 0..faces {
        let (mut lw, mut lh) = (w, h);
        for _ in 0..px.levels.max(1) {
            let bx = lw.div_ceil(bw);
            let by = lh.div_ceil(bh);
            let n = (bx * by * bsize) as usize;
            let src = px.data.get(at..at + n);
            at += n;
            lw = (lw / 2).max(1);
            lh = (lh / 2).max(1);
            match src {
                Some(s) => data.extend_from_slice(s),
                None => data.extend(std::iter::repeat_n(0u8, n)),
            }
        }
    }
    // Drop levels beyond what wgpu allows (keep the top `levels`).
    let data = if levels < px.levels.max(1) {
        let mut out = Vec::new();
        let mut at = 0usize;
        for _ in 0..faces {
            let (mut lw, mut lh) = (w, h);
            for l in 0..px.levels.max(1) {
                let n = (lw.div_ceil(bw) * lh.div_ceil(bh) * bsize) as usize;
                if l < levels {
                    out.extend_from_slice(&data[at..at + n]);
                }
                at += n;
                lw = (lw / 2).max(1);
                lh = (lh / 2).max(1);
            }
        }
        out
    } else {
        data
    };
    let texture = gpu.device.create_texture_with_data(
        &gpu.queue,
        &wgpu::TextureDescriptor {
            label: Some(label),
            size,
            mip_level_count: levels,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        },
        wgpu::util::TextureDataOrder::LayerMajor,
        &data,
    );
    let dim = if faces == 6 {
        wgpu::TextureViewDimension::Cube
    } else {
        wgpu::TextureViewDimension::D2
    };
    let view = texture.create_view(&wgpu::TextureViewDescriptor {
        dimension: Some(dim),
        ..Default::default()
    });
    Ok(Tex { texture, view, dim })
}

/// A one-texel texture of a colour (any view dimension the shaders want).
pub fn solid(gpu: &Gpu, rgba: [u8; 4], dim: wgpu::TextureViewDimension) -> Tex {
    let layers = match dim {
        wgpu::TextureViewDimension::Cube => 6,
        _ => 1,
    };
    let data: Vec<u8> = (0..layers).flat_map(|_| rgba).collect();
    let texture = gpu.device.create_texture_with_data(
        &gpu.queue,
        &wgpu::TextureDescriptor {
            label: Some("solid"),
            size: wgpu::Extent3d {
                width: 1,
                height: 1,
                depth_or_array_layers: layers,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: if dim == wgpu::TextureViewDimension::D3 {
                wgpu::TextureDimension::D3
            } else {
                wgpu::TextureDimension::D2
            },
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        },
        wgpu::util::TextureDataOrder::LayerMajor,
        &data,
    );
    let view = texture.create_view(&wgpu::TextureViewDescriptor {
        dimension: Some(dim),
        ..Default::default()
    });
    Tex { texture, view, dim }
}

/// A one-texel depth texture holding `depth`.
pub fn solid_depth(gpu: &Gpu, depth: f32, dim: wgpu::TextureViewDimension) -> Tex {
    let layers = if dim == wgpu::TextureViewDimension::Cube {
        6
    } else {
        1
    };
    let texture = gpu.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("solid_depth"),
        size: wgpu::Extent3d {
            width: 1,
            height: 1,
            depth_or_array_layers: layers,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Depth32Float,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::RENDER_ATTACHMENT,
        view_formats: &[],
    });
    let mut enc = gpu.device.create_command_encoder(&Default::default());
    for layer in 0..layers {
        let v = texture.create_view(&wgpu::TextureViewDescriptor {
            dimension: Some(wgpu::TextureViewDimension::D2),
            base_array_layer: layer,
            array_layer_count: Some(1),
            ..Default::default()
        });
        let _ = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("clear_depth"),
            color_attachments: &[],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: &v,
                depth_ops: Some(wgpu::Operations {
                    load: wgpu::LoadOp::Clear(depth),
                    store: wgpu::StoreOp::Store,
                }),
                stencil_ops: None,
            }),
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
    }
    gpu.queue.submit([enc.finish()]);
    let view = texture.create_view(&wgpu::TextureViewDescriptor {
        dimension: Some(dim),
        ..Default::default()
    });
    Tex { texture, view, dim }
}

/// The sampler a material sampler state byte describes.
pub fn material_sampler(gpu: &Gpu, state: u8) -> wgpu::Sampler {
    let filter = state & 7;
    let mip = state & 0x18;
    let clamp = |bit: u8| {
        if state & bit != 0 {
            wgpu::AddressMode::ClampToEdge
        } else {
            wgpu::AddressMode::Repeat
        }
    };
    let lin = !matches!(filter, 1);
    let f = if lin {
        wgpu::FilterMode::Linear
    } else {
        wgpu::FilterMode::Nearest
    };
    let m = match mip {
        0x10 => wgpu::MipmapFilterMode::Linear,
        _ => wgpu::MipmapFilterMode::Nearest,
    };
    let cap = anisotropy_cap();
    let aniso = match filter {
        3 => 2,
        4 => 4,
        6 => 16,
        _ => 1,
    }
    .min(cap);
    gpu.device.create_sampler(&wgpu::SamplerDescriptor {
        label: Some("material"),
        address_mode_u: clamp(0x20),
        address_mode_v: clamp(0x40),
        address_mode_w: clamp(0x80),
        mag_filter: f,
        min_filter: f,
        mipmap_filter: m,
        lod_max_clamp: if mip == 0 { 0.0 } else { 32.0 },
        anisotropy_clamp: if lin && mip == 0x10 { aniso } else { 1 },
        ..Default::default()
    })
}

pub fn linear_clamp(gpu: &Gpu) -> wgpu::Sampler {
    gpu.device.create_sampler(&wgpu::SamplerDescriptor {
        label: Some("linear_clamp"),
        mag_filter: wgpu::FilterMode::Linear,
        min_filter: wgpu::FilterMode::Linear,
        mipmap_filter: wgpu::MipmapFilterMode::Linear,
        ..Default::default()
    })
}

/// The shadow maps' comparison sampler as the engine makes it: compare
/// "greater" (the shadow depth is reversed: lit when the receiver is
/// nearer the sun than the caster), linear, border addressing with a zero
/// border (outside the map is lit). Without border support, clamped.
pub fn comparison(gpu: &Gpu) -> wgpu::Sampler {
    let border = gpu
        .device
        .features()
        .contains(wgpu::Features::ADDRESS_MODE_CLAMP_TO_BORDER);
    let mode = if border {
        wgpu::AddressMode::ClampToBorder
    } else {
        wgpu::AddressMode::ClampToEdge
    };
    gpu.device.create_sampler(&wgpu::SamplerDescriptor {
        label: Some("shadow"),
        address_mode_u: mode,
        address_mode_v: mode,
        address_mode_w: mode,
        mag_filter: wgpu::FilterMode::Linear,
        min_filter: wgpu::FilterMode::Linear,
        compare: Some(wgpu::CompareFunction::Greater),
        border_color: border.then_some(wgpu::SamplerBorderColor::TransparentBlack),
        ..Default::default()
    })
}

fn view_dim(d: TextureDim) -> wgpu::TextureViewDimension {
    use wgpu::TextureViewDimension as V;
    match d {
        TextureDim::D1 => V::D1,
        TextureDim::D2 | TextureDim::D2Ms => V::D2,
        TextureDim::D2Array => V::D2Array,
        TextureDim::D3 => V::D3,
        TextureDim::Cube => V::Cube,
        TextureDim::CubeArray => V::CubeArray,
        TextureDim::Buffer | TextureDim::Raw => V::D2,
    }
}

/// The bind group layouts of one vertex + pixel pair (cbuffers, textures,
/// samplers), the stages' reflections merged.
pub struct PassLayout {
    pub groups: [wgpu::BindGroupLayout; 3],
    pub pipeline_layout: wgpu::PipelineLayout,
    /// (binding, stage reflection entry) per group, for building groups.
    pub cbuffers: Vec<(u32, u32, u32)>,
    pub textures: Vec<(u32, u32, TextureDim, TextureKind, bool)>,
    pub samplers: Vec<(u32, u32, bool)>,
    /// Sampler bindings the shaders declare as comparison samplers.
    pub comparison_bindings: Vec<u32>,
    /// Constant buffer bindings with a dynamic offset.
    pub dynamic_count: u32,
}

/// `stage`: 0 vertex, 1 pixel. Entries: (binding, slot, ...).
/// `dynamic`: the constant buffer slot bound with a dynamic offset.
pub fn pass_layout(
    gpu: &Gpu,
    vs: &Reflection,
    ps: &Reflection,
    dynamic: Option<u32>,
) -> PassLayout {
    pass_layout_with(gpu, vs, ps, dynamic, &[])
}

/// As [`pass_layout`]; `unfilterable_ps`: pixel-stage texture slots bound
/// to unfilterable float textures (a depth target read as floats).
pub fn pass_layout_with(
    gpu: &Gpu,
    vs: &Reflection,
    ps: &Reflection,
    dynamic: Option<u32>,
    unfilterable_ps: &[u32],
) -> PassLayout {
    pass_layout_storage(gpu, vs, ps, dynamic, unfilterable_ps, None)
}

/// As [`pass_layout_with`], vertex-stage constant buffer slot `storage`
/// bound as a read-only storage buffer (an instanced per-object array).
pub fn pass_layout_storage(
    gpu: &Gpu,
    vs: &Reflection,
    ps: &Reflection,
    dynamic: Option<u32>,
    unfilterable_ps: &[u32],
    storage: Option<u32>,
) -> PassLayout {
    let mut cb = Vec::new();
    let mut dynamic_count = 0;
    let mut tx = Vec::new();
    let mut sm = Vec::new();
    let mut cmp = Vec::new();
    for (stage, r, vis) in [
        (0u32, vs, wgpu::ShaderStages::VERTEX),
        (1, ps, wgpu::ShaderStages::FRAGMENT),
    ] {
        for c in &r.cbuffers {
            let has_dynamic_offset = dynamic == Some(c.slot);
            dynamic_count += u32::from(has_dynamic_offset);
            cb.push((
                wgpu::BindGroupLayoutEntry {
                    binding: c.binding,
                    visibility: vis,
                    ty: wgpu::BindingType::Buffer {
                        ty: if stage == 0 && storage == Some(c.slot) {
                            wgpu::BufferBindingType::Storage { read_only: true }
                        } else {
                            wgpu::BufferBindingType::Uniform
                        },
                        has_dynamic_offset,
                        min_binding_size: None,
                    },
                    count: None,
                },
                (c.binding, c.slot, c.registers),
            ));
        }
        for t in &r.textures {
            let unfilterable_float = stage == 1 && unfilterable_ps.contains(&t.slot);
            let sample_type = match t.kind {
                TextureKind::Float => wgpu::TextureSampleType::Float {
                    filterable: !unfilterable_float,
                },
                TextureKind::Sint => wgpu::TextureSampleType::Sint,
                TextureKind::Uint => wgpu::TextureSampleType::Uint,
                TextureKind::Depth => wgpu::TextureSampleType::Depth,
                TextureKind::RawBuffer => wgpu::TextureSampleType::Float { filterable: false },
            };
            let ty = if t.kind == TextureKind::RawBuffer {
                wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Storage { read_only: true },
                    has_dynamic_offset: false,
                    min_binding_size: None,
                }
            } else {
                wgpu::BindingType::Texture {
                    sample_type,
                    view_dimension: view_dim(t.dim),
                    multisampled: t.dim == TextureDim::D2Ms,
                }
            };
            tx.push((
                wgpu::BindGroupLayoutEntry {
                    binding: t.binding,
                    visibility: vis,
                    ty,
                    count: None,
                },
                (t.binding, t.slot, t.dim, t.kind, stage == 1),
            ));
        }
        for s in &r.samplers {
            if s.comparison {
                cmp.push(s.binding);
            }
            sm.push((
                wgpu::BindGroupLayoutEntry {
                    binding: s.binding,
                    visibility: vis,
                    ty: wgpu::BindingType::Sampler(if s.comparison {
                        wgpu::SamplerBindingType::Comparison
                    } else {
                        wgpu::SamplerBindingType::Filtering
                    }),
                    count: None,
                },
                (s.binding, s.slot, stage == 1),
            ));
        }
    }
    let mk = |label: &str, e: &[wgpu::BindGroupLayoutEntry]| {
        gpu.device
            .create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some(label),
                entries: e,
            })
    };
    let g0 = mk("cb", &cb.iter().map(|e| e.0).collect::<Vec<_>>());
    let g1 = mk("tex", &tx.iter().map(|e| e.0).collect::<Vec<_>>());
    let g2 = mk("samp", &sm.iter().map(|e| e.0).collect::<Vec<_>>());
    let pipeline_layout = gpu
        .device
        .create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("pass"),
            bind_group_layouts: &[Some(&g0), Some(&g1), Some(&g2)],
            immediate_size: 0,
        });
    PassLayout {
        groups: [g0, g1, g2],
        pipeline_layout,
        cbuffers: cb.into_iter().map(|e| e.1).collect(),
        textures: tx.into_iter().map(|e| e.1).collect(),
        samplers: sm.into_iter().map(|e| e.1).collect(),
        comparison_bindings: cmp,
        dynamic_count,
    }
}

/// A uniform buffer of `regs` 16-byte registers.
pub fn uniform(gpu: &Gpu, regs: &[[f32; 4]]) -> wgpu::Buffer {
    let data: &[u8] = bytemuck::cast_slice(regs);
    gpu.device
        .create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("cb"),
            contents: if data.is_empty() { &[0u8; 16] } else { data },
            usage: wgpu::BufferUsages::UNIFORM,
        })
}

/// Caches by key.
#[derive(Default)]
pub struct Cache<T> {
    pub map: HashMap<String, Option<T>>,
}

static ANISO_CAP: std::sync::atomic::AtomicU16 = std::sync::atomic::AtomicU16::new(16);

/// The anisotropy the material samplers may use (Graphics > texture
/// filtering through `native_quality`); read as the scene's samplers are
/// made, so a change waits for the next map. `BO2MP_H1_ANISO=n` wins.
pub fn anisotropy_cap() -> u16 {
    std::env::var("BO2MP_H1_ANISO")
        .ok()
        .and_then(|v| v.trim().parse::<u16>().ok())
        .unwrap_or_else(|| ANISO_CAP.load(std::sync::atomic::Ordering::Relaxed))
        .clamp(1, 16)
}

pub fn set_anisotropy_cap(n: u16) {
    ANISO_CAP.store(n.clamp(1, 16), std::sync::atomic::Ordering::Relaxed);
}

/// Half float to f32.
fn f16_to_f32(h: u16) -> f32 {
    let (s, e, m) = ((h >> 15) & 1, (h >> 10) & 0x1f, h & 0x3ff);
    let v = match e {
        0 => f32::from(m) * 2f32.powi(-24),
        31 => {
            if m == 0 {
                f32::INFINITY
            } else {
                f32::NAN
            }
        }
        _ => (1.0 + f32::from(m) / 1024.0) * 2f32.powi(i32::from(e) - 15),
    };
    if s == 1 { -v } else { v }
}

/// f32 to half float (round to nearest, saturating to the largest finite).
pub(crate) fn f32_to_f16(v: f32) -> u16 {
    let b = v.to_bits();
    let s = ((b >> 16) & 0x8000) as u16;
    let a = v.abs();
    if a.is_nan() {
        return s | 0x7e00;
    }
    if a >= 65504.0 {
        return s | 0x7bff;
    }
    if a < 6.103_515_6e-5 {
        // Subnormal.
        return s | (a * 2f32.powi(24)).round() as u16;
    }
    let e = ((b >> 23) & 0xff) as i32 - 127 + 15;
    let m = (b >> 13) & 0x3ff;
    let r = (b >> 12) & 1;
    let h = ((e as u32) << 10) | m;
    s | (h + r) as u16
}

/// Multiplies every RGB half float of an R16G16B16A16_FLOAT picture by `k`
/// (alpha kept).
pub fn scale_rgba16f(data: &mut [u8], k: f32) {
    for px in data.chunks_exact_mut(8) {
        for c in 0..3 {
            let h = u16::from_le_bytes([px[c * 2], px[c * 2 + 1]]);
            let v = f32_to_f16(f16_to_f32(h) * k);
            px[c * 2..c * 2 + 2].copy_from_slice(&v.to_le_bytes());
        }
    }
}

#[cfg(test)]
mod half_tests {
    use super::*;

    #[test]
    fn halves_round_trip_and_scale() {
        for v in [0.0f32, 0.5, 1.0, 2.0, 0.08, 3.25, 100.0] {
            let h = f32_to_f16(v);
            assert!((f16_to_f32(h) - v).abs() <= v * 1e-3 + 1e-7, "{v}");
        }
        let mut d = [0u8; 8];
        d[0..2].copy_from_slice(&f32_to_f16(0.25).to_le_bytes());
        d[6..8].copy_from_slice(&f32_to_f16(1.0).to_le_bytes());
        scale_rgba16f(&mut d, 4.0);
        assert!((f16_to_f32(u16::from_le_bytes([d[0], d[1]])) - 1.0).abs() < 1e-3);
        assert_eq!(f16_to_f32(u16::from_le_bytes([d[6], d[7]])), 1.0);
    }
}
