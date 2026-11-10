//! Camera motion blur (Graphics > MOTION BLUR, and the quality's
//! `motion_blur`): each pixel's world position, from the depth, is put
//! through the last frame's view; the screen distance it moved, scaled to a
//! fixed shutter time (so the blur doesn't change with the frame rate) and
//! capped, is the line the HDR colour is averaged along before the tone map.
//! The sky (no depth) blurs by the view's turn alone. The game's own motion
//! blur programs are in `code_post_gfx`, not read here, so the shutter and
//! cap are ours. `BO2MP_IW7_MOTION_BLUR=0`: none.

use glam::{Mat4, Vec3};
use render_h1::gpu::Gpu;

/// The shutter time the blur stands for (seconds).
pub const SHUTTER: f32 = 1.0 / 120.0;
/// The longest blur, as a share of the screen's width.
pub const MAX_LENGTH: f32 = 0.04;
/// Samples along the line.
pub const TAPS: u32 = 10;

const BLUR: &str = r"
struct Vo { @builtin(position) p: vec4<f32>, @location(0) uv: vec2<f32> };
@vertex fn vs(@builtin(vertex_index) i: u32) -> Vo {
    let x = f32((i << 1u) & 2u);
    let y = f32(i & 2u);
    return Vo(vec4<f32>(x * 2.0 - 1.0, 1.0 - y * 2.0, 0.0, 1.0), vec2<f32>(x, y));
}
struct U {
    inv_vp: mat4x4<f32>,
    prev_vp: mat4x4<f32>,
    // eye minus the last frame's eye (xyz), shutter scale (w)
    moved: vec4<f32>,
    // max length (uv), taps
    limit: vec4<f32>,
};
@group(0) @binding(0) var<uniform> u: U;
@group(0) @binding(1) var colour: texture_2d<f32>;
@group(0) @binding(2) var depth: texture_depth_2d;
@group(0) @binding(3) var smp: sampler;
@fragment fn fs(v: Vo) -> @location(0) vec4<f32> {
    let size = vec2<f32>(textureDimensions(colour));
    let px = vec2<i32>(v.p.xy);
    let d = textureLoad(depth, px, 0);
    let ndc = vec4<f32>(v.uv.x * 2.0 - 1.0, 1.0 - v.uv.y * 2.0, d, 1.0);
    let p = u.inv_vp * ndc;
    var q: vec4<f32>;
    if (d <= 0.0) {
        // (The sky: a direction, at infinity.)
        q = u.prev_vp * vec4<f32>(p.xyz, 0.0);
    } else {
        q = u.prev_vp * vec4<f32>(p.xyz / p.w + u.moved.xyz, 1.0);
    }
    let here = textureSampleLevel(colour, smp, v.uv, 0.0);
    if (q.w <= 1e-4) {
        return here;
    }
    let prev_uv = vec2<f32>(q.x / q.w * 0.5 + 0.5, 0.5 - q.y / q.w * 0.5);
    var vel = (v.uv - prev_uv) * u.moved.w;
    let len = length(vel * vec2<f32>(1.0, size.y / size.x));
    if (len > u.limit.x) {
        vel *= u.limit.x / len;
    }
    if (length(vel * size) < 0.5) {
        return here;
    }
    let n = i32(u.limit.y);
    var sum = vec4<f32>(0.0);
    for (var i = 0; i < n; i++) {
        let t = (f32(i) + 0.5) / f32(n) - 0.5;
        sum += textureSampleLevel(colour, smp, clamp(v.uv + vel * t, vec2<f32>(0.0), vec2<f32>(1.0)), 0.0);
    }
    return vec4<f32>(sum.rgb / f32(n), here.a);
}
";

/// The pass and the last frame's view.
pub struct MotionBlur {
    gpu: Gpu,
    pipe: wgpu::RenderPipeline,
    uniform: wgpu::Buffer,
    sampler: wgpu::Sampler,
    /// The group for (colour, depth) last used.
    group: Option<(wgpu::TextureView, wgpu::BindGroup)>,
    prev: Option<(Mat4, Vec3)>,
    pub allowed: bool,
}

impl MotionBlur {
    pub fn new(g: &Gpu) -> Self {
        let module = g.device.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some("iw7 motion blur"), source: wgpu::ShaderSource::Wgsl(BLUR.into()) });
        let pipe = g.device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("iw7 motion blur"),
            layout: None,
            vertex: wgpu::VertexState { module: &module, entry_point: Some("vs"), compilation_options: Default::default(), buffers: &[] },
            primitive: Default::default(),
            depth_stencil: None,
            multisample: Default::default(),
            fragment: Some(wgpu::FragmentState { module: &module, entry_point: Some("fs"), compilation_options: Default::default(), targets: &[Some(wgpu::TextureFormat::Rgba16Float.into())] }),
            multiview_mask: None,
            cache: None,
        });
        let uniform = g.device.create_buffer(&wgpu::BufferDescriptor { label: Some("iw7 motion blur"), size: 4 * (16 * 2 + 8), usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST, mapped_at_creation: false });
        let sampler = g.device.create_sampler(&wgpu::SamplerDescriptor { label: Some("iw7 motion blur"), mag_filter: wgpu::FilterMode::Linear, min_filter: wgpu::FilterMode::Linear, ..Default::default() });
        Self { gpu: g.clone(), pipe, uniform, sampler, group: None, prev: None, allowed: !std::env::var("BO2MP_IW7_MOTION_BLUR").is_ok_and(|v| v.trim() == "0") }
    }

    /// Remembers this frame's view without blurring (the blur is off).
    pub fn skip(&mut self, view_projection: Mat4, eye: Vec3) {
        self.prev = Some((view_projection, eye));
    }

    /// Blurs `colour` (with its `depth`, reversed) into `out` when there is
    /// a last frame to compare with; returns whether it drew.
    #[allow(clippy::too_many_arguments)]
    pub fn record(&mut self, enc: &mut wgpu::CommandEncoder, colour: &wgpu::TextureView, depth: &wgpu::TextureView, out: &wgpu::TextureView, view_projection: Mat4, eye: Vec3, dt: f32) -> bool {
        let Some((prev_vp, prev_eye)) = self.prev.replace((view_projection, eye)) else { return false };
        if dt <= 0.0 {
            return false;
        }
        let mut f = crate::cull::Pack::<40>::new();
        f.extend_from_slice(&view_projection.inverse().to_cols_array());
        f.extend_from_slice(&prev_vp.to_cols_array());
        let moved = eye - prev_eye;
        f.extend_from_slice(&[moved.x, moved.y, moved.z, (SHUTTER / dt).min(1.0)]);
        f.extend_from_slice(&[MAX_LENGTH, TAPS as f32, 0.0, 0.0]);
        self.gpu.queue.write_buffer(&self.uniform, 0, f.bytes());
        if self.group.as_ref().is_none_or(|(v, _)| v != colour) {
            let group = self.gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("iw7 motion blur"),
                layout: &self.pipe.get_bind_group_layout(0),
                entries: &[
                    wgpu::BindGroupEntry { binding: 0, resource: self.uniform.as_entire_binding() },
                    wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(colour) },
                    wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::TextureView(depth) },
                    wgpu::BindGroupEntry { binding: 3, resource: wgpu::BindingResource::Sampler(&self.sampler) },
                ],
            });
            self.group = Some((colour.clone(), group));
        }
        let Some((_, group)) = self.group.as_ref() else { return false };
        let mut rp = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("iw7 motion blur"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment { view: out, resolve_target: None, depth_slice: None, ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::BLACK), store: wgpu::StoreOp::Store } })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        rp.set_pipeline(&self.pipe);
        rp.set_bind_group(0, group, &[]);
        rp.draw(0..3, 0..1);
        true
    }
}
