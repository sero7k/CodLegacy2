//! The hand-off of a finished Black Ops III frame into a host's frame: its
//! final image into the host's colour target and the scene depth into the
//! host's depth, in the depth band the host keeps for the world, so what
//! the host draws afterwards (players, the gun, effects, the HUD) sits in
//! the world and over it. One full-screen triangle; every pixel is written.

#[allow(unused_imports)]
use crate::profile::{ComputeExt as _, EncExt as _, PassExt as _, QueueExt as _};
use render_h1::gpu::Gpu;

const SHADER: &str = r"
@group(0) @binding(0) var colour: texture_2d<f32>;
@group(0) @binding(1) var depth: texture_depth_2d;
struct Band { lo_hi: vec4<f32>, origin: vec4<f32> };
@group(0) @binding(2) var<uniform> band: Band;

struct Vo { @builtin(position) p: vec4<f32> };

@vertex fn vs(@builtin(vertex_index) i: u32) -> Vo {
    let x = f32((i << 1u) & 2u);
    let y = f32(i & 2u);
    return Vo(vec4<f32>(x * 2.0 - 1.0, 1.0 - y * 2.0, 0.0, 1.0));
}

struct Fo { @location(0) c: vec4<f32>, @builtin(frag_depth) d: f32 };

@fragment fn fs(v: Vo) -> Fo {
    // (The frame's own pixel: the viewport may sit inside a larger target.)
    let px = vec2<i32>(v.p.xy - band.origin.xy);
    let c = textureLoad(colour, px, 0);
    // (Reversed depth: 1 near, 0 at infinity; laid into the host's band.)
    let d = textureLoad(depth, px, 0);
    // (The image is display-encoded by resample_final; an sRGB host
    // target encodes on write, so it is decoded first: z = 1.)
    var rgb = c.rgb;
    if (band.lo_hi.z > 0.5) {
        rgb = select(pow((rgb + 0.055) / 1.055, vec3<f32>(2.4)), rgb / 12.92, rgb <= vec3<f32>(0.04045));
    }
    return Fo(vec4<f32>(rgb, 1.0), band.lo_hi.x + d * (band.lo_hi.y - band.lo_hi.x));
}
";

/// The pipeline for one host colour and depth format.
struct Last {
    colour: wgpu::TextureView,
    depth: wgpu::TextureView,
    bind: wgpu::BindGroup,
    words: [f32; 8],
}

pub struct Present {
    gpu: Gpu,
    pipeline: wgpu::RenderPipeline,
    layout: wgpu::BindGroupLayout,
    band: wgpu::Buffer,
    formats: (wgpu::TextureFormat, wgpu::TextureFormat),
    /// The last binding made, for the image and depth it binds.
    last: std::sync::Mutex<Option<Last>>,
}

impl Present {
    pub fn new(gpu: Gpu, colour_format: wgpu::TextureFormat, depth_format: wgpu::TextureFormat) -> Self {
        let device = &gpu.device;
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some("t7_present"), source: wgpu::ShaderSource::Wgsl(SHADER.into()) });
        let entry = |binding, ty| wgpu::BindGroupLayoutEntry { binding, visibility: wgpu::ShaderStages::FRAGMENT, ty, count: None };
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("t7_present"),
            entries: &[
                entry(0, wgpu::BindingType::Texture { sample_type: wgpu::TextureSampleType::Float { filterable: false }, view_dimension: wgpu::TextureViewDimension::D2, multisampled: false }),
                entry(1, wgpu::BindingType::Texture { sample_type: wgpu::TextureSampleType::Depth, view_dimension: wgpu::TextureViewDimension::D2, multisampled: false }),
                entry(2, wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: None }),
            ],
        });
        let pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: Some("t7_present"), bind_group_layouts: &[Some(&layout)], immediate_size: 0 });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("t7_present"),
            layout: Some(&pl),
            vertex: wgpu::VertexState { module: &module, entry_point: Some("vs"), compilation_options: Default::default(), buffers: &[] },
            fragment: Some(wgpu::FragmentState {
                module: &module,
                entry_point: Some("fs"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState { format: colour_format, blend: None, write_mask: wgpu::ColorWrites::ALL })],
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: Some(wgpu::DepthStencilState {
                format: depth_format,
                depth_write_enabled: Some(true),
                depth_compare: Some(wgpu::CompareFunction::Always),
                stencil: Default::default(),
                bias: Default::default(),
            }),
            multisample: Default::default(),
            multiview_mask: None,
            cache: crate::pipecache::get(&device).as_ref(),
        });
        let band = crate::audit::buffer(&device, &wgpu::BufferDescriptor { label: Some("t7_present_band"), size: 32, usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST, mapped_at_creation: false });
        Self { gpu, pipeline, layout, band, formats: (colour_format, depth_format), last: std::sync::Mutex::new(None) }
    }

    /// The formats this was made for.
    pub fn formats(&self) -> (wgpu::TextureFormat, wgpu::TextureFormat) {
        self.formats
    }

    /// Writes `colour` (the finished image, the viewport's size) and `depth`
    /// (the scene depth, a depth-aspect view) into the host's targets inside
    /// `viewport` (x, y, width, height in the target's pixels; the whole
    /// target when the camera has it all), the depth laid into `band` (the
    /// host's (low, high) depth range). Pixels outside the viewport are left
    /// as they are: a pass over the whole target read the image past its
    /// edge (black) wherever the viewport was smaller than the target.
    #[allow(clippy::too_many_arguments)]
    pub fn record(
        &self,
        enc: &mut wgpu::CommandEncoder,
        colour: &wgpu::TextureView,
        depth: &wgpu::TextureView,
        host_colour: &wgpu::TextureView,
        host_depth: &wgpu::TextureView,
        band: (f32, f32),
        viewport: [u32; 4],
    ) {
        let words = [band.0, band.1, if self.formats.0.is_srgb() { 1.0 } else { 0.0 }, 0.0, viewport[0] as f32, viewport[1] as f32, 0.0, 0.0];
        // (The binding and the band are made again only when the image,
        // the depth or the band changed: not every frame.)
        let mut last = self.last.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let fresh = last.as_ref().is_none_or(|l| l.colour != *colour || l.depth != *depth);
        if fresh {
            let bind = self.gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("t7_present"),
                layout: &self.layout,
                entries: &[
                    wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(colour) },
                    wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(depth) },
                    wgpu::BindGroupEntry { binding: 2, resource: self.band.as_entire_binding() },
                ],
            });
            *last = Some(Last { colour: colour.clone(), depth: depth.clone(), bind, words: [f32::NAN; 8] });
        }
        let Some(l) = last.as_mut() else { return };
        if l.words != words {
            self.gpu.queue.write_buffer_p(&self.band, 0, bytemuck::cast_slice(&words));
            l.words = words;
        }
        let bind = l.bind.clone();
        drop(last);
        let mut pass = enc.render_pass_p(&wgpu::RenderPassDescriptor {
            label: Some("t7_present"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: host_colour,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations { load: wgpu::LoadOp::Load, store: wgpu::StoreOp::Store },
            })],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: host_depth,
                depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Load, store: wgpu::StoreOp::Store }),
                stencil_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Load, store: wgpu::StoreOp::Store }),
            }),
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        pass.set_pipeline_p(&self.pipeline);
        pass.set_bind_group_p(0, &bind, &[]);
        pass.set_viewport(viewport[0] as f32, viewport[1] as f32, viewport[2] as f32, viewport[3] as f32, 0.0, 1.0);
        pass.set_scissor_rect(viewport[0], viewport[1], viewport[2], viewport[3]);
        pass.draw_p(0..3, 0..1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The image lands in the host's colour target and the depth in its band
    /// (software GPU; skipped when there is none).
    #[test]
    fn the_frame_and_its_depth_are_handed_over() {
        let Ok(gpu) = Gpu::headless() else { return };
        let d = &gpu.device;
        let (w, h) = (4u32, 4u32);
        let tex = |format, usage| d.create_texture(&wgpu::TextureDescriptor {
            label: None,
            size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage,
            view_formats: &[],
        });
        let rt = wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING;
        let image = tex(wgpu::TextureFormat::Rgba8Unorm, rt | wgpu::TextureUsages::COPY_DST);
        gpu.queue.write_texture_p(image.as_image_copy(), &[255u8, 128, 0, 255].repeat((w * h) as usize), wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(w * 4), rows_per_image: None }, image.size());
        let scene_depth = tex(wgpu::TextureFormat::Depth32FloatStencil8, rt);
        let host_colour = tex(wgpu::TextureFormat::Rgba8Unorm, rt | wgpu::TextureUsages::COPY_SRC);
        let host_depth = tex(wgpu::TextureFormat::Depth32Float, rt | wgpu::TextureUsages::COPY_SRC);
        let mut enc = d.create_command_encoder(&Default::default());
        // The scene depth: 0.5 everywhere.
        {
            let v = scene_depth.create_view(&Default::default());
            let _p = enc.render_pass_p(&wgpu::RenderPassDescriptor {
                label: None,
                color_attachments: &[],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &v,
                    depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(0.5), store: wgpu::StoreOp::Store }),
                    stencil_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(0), store: wgpu::StoreOp::Store }),
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
        }
        let present = Present::new(gpu.clone(), wgpu::TextureFormat::Rgba8Unorm, wgpu::TextureFormat::Depth32Float);
        let depth_only = scene_depth.create_view(&wgpu::TextureViewDescriptor { aspect: wgpu::TextureAspect::DepthOnly, ..Default::default() });
        present.record(&mut enc, &image.create_view(&Default::default()), &depth_only, &host_colour.create_view(&Default::default()), &host_depth.create_view(&Default::default()), (0.0, 0.25), [0, 0, w, h]);
        let read = |t: &wgpu::Texture, enc: &mut wgpu::CommandEncoder| {
            let b = crate::audit::buffer(&d, &wgpu::BufferDescriptor { label: None, size: 256 * h as u64, usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ, mapped_at_creation: false });
            enc.copy_texture_to_buffer(t.as_image_copy(), wgpu::TexelCopyBufferInfo { buffer: &b, layout: wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(256), rows_per_image: None } }, t.size());
            b
        };
        let cb = read(&host_colour, &mut enc);
        let db = read(&host_depth, &mut enc);
        gpu.queue.submit([enc.finish()]);
        for b in [&cb, &db] {
            b.slice(..).map_async(wgpu::MapMode::Read, |_| {});
        }
        let _ = d.poll(wgpu::PollType::wait_indefinitely());
        let c = cb.slice(..).get_mapped_range();
        assert_eq!(&c[..4], &[255, 128, 0, 255]);
        let z = f32::from_le_bytes(db.slice(..).get_mapped_range()[..4].try_into().unwrap());
        assert!((z - 0.125).abs() < 1e-4, "{z}");
    }

    /// A viewport inside a larger, odd-sized target (a render scale below 1,
    /// a split screen): every texel of the image lands on its own pixel of
    /// the viewport and nothing outside it is touched.
    #[test]
    fn a_viewport_inside_a_larger_target_is_placed_pixel_for_pixel() {
        let Ok(gpu) = Gpu::headless() else { return };
        let d = &gpu.device;
        let (tw, th, vx, vy, vw, vh) = (11u32, 9u32, 3u32, 2u32, 5u32, 4u32);
        let tex = |w: u32, h: u32, format, usage| d.create_texture(&wgpu::TextureDescriptor {
            label: None,
            size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage,
            view_formats: &[],
        });
        let rt = wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING;
        let image = tex(vw, vh, wgpu::TextureFormat::Rgba8Unorm, rt | wgpu::TextureUsages::COPY_DST);
        let mut px = Vec::new();
        for y in 0..vh {
            for x in 0..vw {
                px.extend_from_slice(&[(x * 40) as u8, (y * 60) as u8, 7, 255]);
            }
        }
        gpu.queue.write_texture_p(image.as_image_copy(), &px, wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(vw * 4), rows_per_image: None }, image.size());
        let scene_depth = tex(vw, vh, wgpu::TextureFormat::Depth32FloatStencil8, rt);
        let host_colour = tex(tw, th, wgpu::TextureFormat::Rgba8Unorm, rt | wgpu::TextureUsages::COPY_SRC | wgpu::TextureUsages::COPY_DST);
        let host_depth = tex(tw, th, wgpu::TextureFormat::Depth32Float, rt | wgpu::TextureUsages::COPY_SRC);
        gpu.queue.write_texture_p(host_colour.as_image_copy(), &[9u8, 9, 9, 9].repeat((tw * th) as usize), wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(tw * 4), rows_per_image: None }, host_colour.size());
        let mut enc = d.create_command_encoder(&Default::default());
        {
            let v = scene_depth.create_view(&Default::default());
            let _p = enc.render_pass_p(&wgpu::RenderPassDescriptor {
                label: None,
                color_attachments: &[],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &v,
                    depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(0.5), store: wgpu::StoreOp::Store }),
                    stencil_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(0), store: wgpu::StoreOp::Store }),
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
        }
        let present = Present::new(gpu.clone(), wgpu::TextureFormat::Rgba8Unorm, wgpu::TextureFormat::Depth32Float);
        let depth_only = scene_depth.create_view(&wgpu::TextureViewDescriptor { aspect: wgpu::TextureAspect::DepthOnly, ..Default::default() });
        present.record(&mut enc, &image.create_view(&Default::default()), &depth_only, &host_colour.create_view(&Default::default()), &host_depth.create_view(&Default::default()), (0.0, 0.25), [vx, vy, vw, vh]);
        let b = d.create_buffer(&wgpu::BufferDescriptor { label: None, size: 256 * u64::from(th), usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ, mapped_at_creation: false });
        enc.copy_texture_to_buffer(host_colour.as_image_copy(), wgpu::TexelCopyBufferInfo { buffer: &b, layout: wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(256), rows_per_image: None } }, host_colour.size());
        gpu.queue.submit([enc.finish()]);
        b.slice(..).map_async(wgpu::MapMode::Read, |_| {});
        let _ = d.poll(wgpu::PollType::wait_indefinitely());
        let c = b.slice(..).get_mapped_range();
        for y in 0..th {
            for x in 0..tw {
                let at = (y * 256 + x * 4) as usize;
                let got = &c[at..at + 4];
                if (vx..vx + vw).contains(&x) && (vy..vy + vh).contains(&y) {
                    let (ix, iy) = (x - vx, y - vy);
                    assert_eq!(got, &[(ix * 40) as u8, (iy * 60) as u8, 7, 255], "pixel {x},{y}");
                } else {
                    assert_eq!(got, &[9, 9, 9, 9], "outside pixel {x},{y}");
                }
            }
        }
    }
}
