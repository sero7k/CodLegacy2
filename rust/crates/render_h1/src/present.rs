//! The hand-off of the finished MWR frame into the host's colour target.
//!
//! READ (9 Nov 2026, `postfx_color2` in `code_post_gfx_mp`): the film pass
//! encodes its result to sRGB itself (`1.055 x^(1/2.4) - 0.055`, the
//! 0.0031308 linear toe) before the tint, so the exe writes display values
//! into a plain UNORM back buffer. A host target that encodes on write (an
//! sRGB format) or is encoded later (a float target) would encode them a
//! second time: the frame came out pale, washed out, with tinted highlights.
//! So the film pass draws into a float texture of its own and this pass
//! copies it out, decoded first when the target is sRGB or float.
//! `BO2MP_H1_PRESENT=raw` writes the film output straight to the target
//! (the behaviour before).

use crate::gpu::Gpu;

const SHADER: &str = r"
@group(0) @binding(0) var colour: texture_2d<f32>;
struct P { decode: vec4<f32> };
@group(0) @binding(1) var<uniform> p: P;

struct Vo { @builtin(position) pos: vec4<f32> };

@vertex fn vs(@builtin(vertex_index) i: u32) -> Vo {
    let x = f32((i << 1u) & 2u);
    let y = f32(i & 2u);
    return Vo(vec4<f32>(x * 2.0 - 1.0, 1.0 - y * 2.0, 0.0, 1.0));
}

@fragment fn fs(v: Vo) -> @location(0) vec4<f32> {
    let c = textureLoad(colour, vec2<i32>(v.pos.xy), 0);
    var rgb = clamp(c.rgb, vec3<f32>(0.0), vec3<f32>(1.0));
    if (p.decode.x > 0.5) {
        rgb = select(pow((rgb + 0.055) / 1.055, vec3<f32>(2.4)), rgb / 12.92, rgb <= vec3<f32>(0.04045));
    }
    return vec4<f32>(rgb, 1.0);
}
";

/// Whether the hand-off is used (`BO2MP_H1_PRESENT=raw`: not).
pub fn enabled() -> bool {
    !std::env::var("BO2MP_H1_PRESENT").is_ok_and(|v| v.trim().eq_ignore_ascii_case("raw"))
}

/// Whether a target needs the film's display values decoded: it encodes on
/// write (sRGB) or holds linear values encoded later (float).
pub fn decodes(format: wgpu::TextureFormat) -> bool {
    format.is_srgb()
        || matches!(
            format,
            wgpu::TextureFormat::Rgba16Float | wgpu::TextureFormat::Rgba32Float | wgpu::TextureFormat::Rg11b10Ufloat
        )
}

pub struct Present {
    format: wgpu::TextureFormat,
    pipeline: wgpu::RenderPipeline,
    layout: wgpu::BindGroupLayout,
    params: wgpu::Buffer,
}

impl Present {
    pub fn new(gpu: &Gpu, format: wgpu::TextureFormat) -> Self {
        let device = &gpu.device;
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("h1_present"),
            source: wgpu::ShaderSource::Wgsl(SHADER.into()),
        });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("h1_present"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: false },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
        });
        let pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("h1_present"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("h1_present"),
            layout: Some(&pl),
            vertex: wgpu::VertexState {
                module: &module,
                entry_point: Some("vs"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            primitive: Default::default(),
            depth_stencil: None,
            multisample: Default::default(),
            fragment: Some(wgpu::FragmentState {
                module: &module,
                entry_point: Some("fs"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        });
        let params = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("h1_present"),
            size: 16,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let decode: [f32; 4] = [if decodes(format) { 1.0 } else { 0.0 }, 0.0, 0.0, 0.0];
        let bytes: Vec<u8> = decode.iter().flat_map(|f| f.to_le_bytes()).collect();
        gpu.queue.write_buffer(&params, 0, &bytes);
        Self { format, pipeline, layout, params }
    }

    pub fn format(&self) -> wgpu::TextureFormat {
        self.format
    }

    /// Copies `film` (the film pass's display values) into `target`.
    pub fn record(&self, gpu: &Gpu, enc: &mut wgpu::CommandEncoder, film: &wgpu::TextureView, target: &wgpu::TextureView) {
        let bind = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("h1_present"),
            layout: &self.layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(film) },
                wgpu::BindGroupEntry { binding: 1, resource: self.params.as_entire_binding() },
            ],
        });
        let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("h1_present"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: target,
                resolve_target: None,
                ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::BLACK), store: wgpu::StoreOp::Store },
                depth_slice: None,
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &bind, &[]);
        pass.draw(0..3, 0..1);
    }
}
