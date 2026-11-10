//! A pass's bind group layouts with several constant buffer slots bound
//! with a dynamic offset (`render_h1::gpu::pass_layout_with` takes one):
//! the per-object block (the object) and, on shadow targets, the frame
//! blocks too (the view: a sun cascade or a spot tile).

use dxbc_sm5::{Reflection, TextureDim, TextureKind};
use render_h1::gpu::{Gpu, PassLayout};

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

/// The layouts of one vertex + pixel pair; `dynamic`: the constant buffer
/// slots bound with a dynamic offset.
pub fn pass_layout(
    gpu: &Gpu,
    vs: &Reflection,
    ps: &Reflection,
    dynamic: &[u32],
) -> PassLayout {
    pass_layout_with(gpu, vs, ps, dynamic, None)
}

/// As `pass_layout`, constant buffer slot `storage` bound as a read-only
/// storage buffer (an instanced per-object array, `instancing`).
pub fn pass_layout_with(
    gpu: &Gpu,
    vs: &Reflection,
    ps: &Reflection,
    dynamic: &[u32],
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
            let has_dynamic_offset = dynamic.contains(&c.slot);
            dynamic_count += u32::from(has_dynamic_offset);
            cb.push((
                wgpu::BindGroupLayoutEntry {
                    binding: c.binding,
                    visibility: vis,
                    ty: wgpu::BindingType::Buffer {
                        ty: if storage == Some(c.slot) {
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
            let sample_type = match t.kind {
                TextureKind::Float => wgpu::TextureSampleType::Float {
                    filterable: true,
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

