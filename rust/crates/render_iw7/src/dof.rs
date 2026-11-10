//! Depth of field as Black Ops II asks for it (aiming down the sights:
//! `DepthOfField` from the game's view effects, the same numbers the Black
//! Ops II maps' post pass reads): blur rising from nothing at the near end
//! to `near_blur` at the near start, and from the far start to `far_blur`
//! at the far end, by the pixel's view depth. Black Ops II's blur amounts
//! are radii in 480-line pixels; here each pixel gathers a 24-tap disc of
//! that radius at the frame's size, a far-blurred pixel taking only taps no
//! nearer than itself (so the sights' edge doesn't bleed). Graphics >
//! DEPTH OF FIELD (and the quality's `depth_of_field`) turn it off;
//! `BO2MP_IW7_DOF=0` too.

use glam::{Mat4, Vec3};
use render_h1::gpu::Gpu;

/// The game's depth-of-field numbers for a frame.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Dof {
    pub near_start: f32,
    pub near_end: f32,
    pub near_blur: f32,
    pub far_start: f32,
    pub far_end: f32,
    pub far_blur: f32,
}

impl Dof {
    pub fn active(&self) -> bool {
        (self.near_end > self.near_start + 1.0 && self.near_blur > 0.0) || (self.far_end > self.far_start + 1.0 && self.far_blur > 0.0)
    }

    /// The blur radius (480-line pixels) at view depth `d`.
    pub fn radius(&self, d: f32) -> f32 {
        let ramp = |a: f32, b: f32, x: f32| if b > a { ((x - a) / (b - a)).clamp(0.0, 1.0) } else { 0.0 };
        let near = if self.near_end > self.near_start { (1.0 - ramp(self.near_start, self.near_end, d)) * self.near_blur } else { 0.0 };
        let far = ramp(self.far_start, self.far_end, d) * self.far_blur;
        near.max(far)
    }
}

const DOF: &str = r"
struct Vo { @builtin(position) p: vec4<f32>, @location(0) uv: vec2<f32> };
@vertex fn vs(@builtin(vertex_index) i: u32) -> Vo {
    let x = f32((i << 1u) & 2u);
    let y = f32(i & 2u);
    return Vo(vec4<f32>(x * 2.0 - 1.0, 1.0 - y * 2.0, 0.0, 1.0), vec2<f32>(x, y));
}
struct U {
    inv_vp: mat4x4<f32>,
    // forward (xyz), pixels per 480-line pixel
    fwd: vec4<f32>,
    // near start, near end, near blur, -
    near: vec4<f32>,
    // far start, far end, far blur, -
    far: vec4<f32>,
};
@group(0) @binding(0) var<uniform> u: U;
@group(0) @binding(1) var colour: texture_2d<f32>;
@group(0) @binding(2) var depth: texture_depth_2d;
@group(0) @binding(3) var smp: sampler;

fn view_depth(px: vec2<i32>, uv: vec2<f32>) -> f32 {
    let d = textureLoad(depth, px, 0);
    if (d <= 0.0) { return 1e9; }
    let p = u.inv_vp * vec4<f32>(uv.x * 2.0 - 1.0, 1.0 - uv.y * 2.0, d, 1.0);
    return dot(p.xyz / p.w, u.fwd.xyz);
}

fn ramp(a: f32, b: f32, x: f32) -> f32 {
    if (b <= a) { return 0.0; }
    return clamp((x - a) / (b - a), 0.0, 1.0);
}

fn radius(d: f32) -> f32 {
    var near = 0.0;
    if (u.near.y > u.near.x) { near = (1.0 - ramp(u.near.x, u.near.y, d)) * u.near.z; }
    let far = ramp(u.far.x, u.far.y, d) * u.far.z;
    return max(near, far);
}

@fragment fn fs(v: Vo) -> @location(0) vec4<f32> {
    let size = vec2<f32>(textureDimensions(colour));
    let px = vec2<i32>(v.p.xy);
    let here = textureLoad(colour, px, 0);
    let d = view_depth(px, v.uv);
    let r = radius(d) * u.fwd.w;
    if (r < 0.5) { return here; }
    var sum = here.rgb;
    var wsum = 1.0;
    for (var i = 1; i < 24; i++) {
        let a = f32(i) * 2.39996323;
        let rr = sqrt(f32(i) / 24.0) * r;
        let o = vec2<f32>(cos(a), sin(a)) * rr;
        let q = clamp(v.p.xy + o, vec2<f32>(0.0), size - 1.0);
        let qd = view_depth(vec2<i32>(q), q / size);
        // (A far-blurred pixel takes nothing nearer than itself.)
        var w = 1.0;
        if (qd < d - 16.0 && radius(qd) * u.fwd.w < rr) { w = 0.0; }
        sum += textureSampleLevel(colour, smp, q / size, 0.0).rgb * w;
        wsum += w;
    }
    return vec4<f32>(sum / wsum, here.a);
}
";

/// The pass.
pub struct DepthOfField {
    gpu: Gpu,
    pipe: wgpu::RenderPipeline,
    uniform: wgpu::Buffer,
    sampler: wgpu::Sampler,
    group: Option<(wgpu::TextureView, wgpu::BindGroup)>,
    pub allowed: bool,
}

impl DepthOfField {
    pub fn new(g: &Gpu) -> Self {
        let module = g.device.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some("iw7 dof"), source: wgpu::ShaderSource::Wgsl(DOF.into()) });
        let pipe = g.device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("iw7 dof"),
            layout: None,
            vertex: wgpu::VertexState { module: &module, entry_point: Some("vs"), compilation_options: Default::default(), buffers: &[] },
            primitive: Default::default(),
            depth_stencil: None,
            multisample: Default::default(),
            fragment: Some(wgpu::FragmentState { module: &module, entry_point: Some("fs"), compilation_options: Default::default(), targets: &[Some(wgpu::TextureFormat::Rgba16Float.into())] }),
            multiview_mask: None,
            cache: None,
        });
        let uniform = g.device.create_buffer(&wgpu::BufferDescriptor { label: Some("iw7 dof"), size: 4 * (16 + 12), usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST, mapped_at_creation: false });
        let sampler = g.device.create_sampler(&wgpu::SamplerDescriptor { label: Some("iw7 dof"), mag_filter: wgpu::FilterMode::Linear, min_filter: wgpu::FilterMode::Linear, ..Default::default() });
        Self { gpu: g.clone(), pipe, uniform, sampler, group: None, allowed: !std::env::var("BO2MP_IW7_DOF").is_ok_and(|v| v.trim() == "0") }
    }

    /// Blurs `colour` (with its reversed `depth`) into `out` by `dof`.
    #[allow(clippy::too_many_arguments)]
    pub fn record(&mut self, enc: &mut wgpu::CommandEncoder, colour: &wgpu::TextureView, depth: &wgpu::TextureView, out: &wgpu::TextureView, view_projection: Mat4, forward: Vec3, height: u32, dof: &Dof) {
        let mut f = crate::cull::Pack::<32>::new();
        f.extend_from_slice(&view_projection.inverse().to_cols_array());
        let fwd = forward.normalize_or(Vec3::X);
        f.extend_from_slice(&[fwd.x, fwd.y, fwd.z, height as f32 / 480.0]);
        f.extend_from_slice(&[dof.near_start, dof.near_end, dof.near_blur, 0.0]);
        f.extend_from_slice(&[dof.far_start, dof.far_end, dof.far_blur, 0.0]);
        self.gpu.queue.write_buffer(&self.uniform, 0, f.bytes());
        if self.group.as_ref().is_none_or(|(v, _)| v != colour) {
            let group = self.gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("iw7 dof"),
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
        let Some((_, group)) = self.group.as_ref() else { return };
        let mut rp = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("iw7 dof"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment { view: out, resolve_target: None, depth_slice: None, ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::BLACK), store: wgpu::StoreOp::Store } })],
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn aiming_blurs_the_far_scene_and_leaves_the_middle() {
        let d = Dof { near_start: 0.0, near_end: 0.0, near_blur: 0.0, far_start: 200.0, far_end: 2000.0, far_blur: 2.0 };
        assert!(d.active());
        assert_eq!(d.radius(100.0), 0.0);
        assert!((d.radius(1100.0) - 1.0).abs() < 1e-4);
        assert_eq!(d.radius(5000.0), 2.0);
        assert!(!Dof::default().active());
    }
}
