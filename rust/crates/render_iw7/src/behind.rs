//! The scene behind the distortion surfaces (heat haze, refracting glass:
//! the `distortion_*` techniques): code texture 18, a copy of the colour
//! drawn before them, which their pixel programs sample at a screen uv
//! bent by their normal map (`p.xy / p.w` from codes 265 and 266, clamped
//! to code 160) and tint. The copy has the screen maps' fixed size and is
//! made by uv, so its bind groups never change with the frame's size.

use render_h1::gpu::Gpu;

const BLIT: &str = r"
struct Vo { @builtin(position) p: vec4<f32>, @location(0) uv: vec2<f32> };
@vertex fn vs(@builtin(vertex_index) i: u32) -> Vo {
    let x = f32((i << 1u) & 2u);
    let y = f32(i & 2u);
    return Vo(vec4<f32>(x * 2.0 - 1.0, 1.0 - y * 2.0, 0.0, 1.0), vec2<f32>(x, y));
}
@group(0) @binding(0) var src: texture_2d<f32>;
@group(0) @binding(1) var smp: sampler;
@fragment fn fs(v: Vo) -> @location(0) vec4<f32> {
    return textureSampleLevel(src, smp, v.uv, 0.0);
}
";

/// The scene copy and the pass that makes it.
pub struct Behind {
    pub texture: wgpu::Texture,
    pub view: wgpu::TextureView,
    pipe: wgpu::RenderPipeline,
    sampler: wgpu::Sampler,
    /// The copy's group for the colour view last copied.
    group: Option<(wgpu::TextureView, wgpu::BindGroup)>,
}

impl Behind {
    pub fn new(g: &Gpu, width: u32, height: u32) -> Self {
        let texture = g.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("iw7 scene behind"),
            size: wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba16Float,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let view = texture.create_view(&Default::default());
        let module = g.device.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some("iw7 scene behind"), source: wgpu::ShaderSource::Wgsl(BLIT.into()) });
        let pipe = g.device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("iw7 scene behind"),
            layout: None,
            vertex: wgpu::VertexState { module: &module, entry_point: Some("vs"), compilation_options: Default::default(), buffers: &[] },
            primitive: Default::default(),
            depth_stencil: None,
            multisample: Default::default(),
            fragment: Some(wgpu::FragmentState { module: &module, entry_point: Some("fs"), compilation_options: Default::default(), targets: &[Some(wgpu::TextureFormat::Rgba16Float.into())] }),
            multiview_mask: None,
            cache: None,
        });
        let sampler = g.device.create_sampler(&wgpu::SamplerDescriptor { label: Some("iw7 scene behind"), mag_filter: wgpu::FilterMode::Linear, min_filter: wgpu::FilterMode::Linear, ..Default::default() });
        Self { texture, view, pipe, sampler, group: None }
    }

    /// Copies `colour` (sampleable `Rgba16Float`) into the scene behind.
    pub fn record(&mut self, g: &Gpu, enc: &mut wgpu::CommandEncoder, colour: &wgpu::TextureView) {
        if self.group.as_ref().is_none_or(|(v, _)| v != colour) {
            let group = g.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("iw7 scene behind"),
            layout: &self.pipe.get_bind_group_layout(0),
            entries: &[wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(colour) }, wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(&self.sampler) }],
            });
            self.group = Some((colour.clone(), group));
        }
        let Some((_, group)) = self.group.as_ref() else { return };
        let mut rp = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("iw7 scene behind"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment { view: &self.view, resolve_target: None, depth_slice: None, ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::BLACK), store: wgpu::StoreOp::Store } })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        rp.set_pipeline(&self.pipe);
        rp.set_bind_group(0, group, &[]);
        rp.draw(0..3, 0..1);
    }
}
