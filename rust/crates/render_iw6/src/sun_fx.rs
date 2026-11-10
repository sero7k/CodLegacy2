//! The sun in the view: how it blinds (darkens) and glares (lightens) the
//! picture, as the engine does it.
//!
//! READ (`docs/exe_notes/IW6_POST.md` 4): the map's `sunflare_t` (`Scene::sun_flare`)
//! gives the angles (as cosines), the strengths and the fade times; the sun's
//! visibility is the share of its sprite that passes the depth test (the
//! engine counts it with an occlusion query on the sprite; `Probe` counts
//! depth samples over the sprite's disc); the blind and glare states chase
//! `ramp(dot) * visibility` at `1 / fadeIn` or `1 / fadeOut` per millisecond;
//! the darken (`state * blindMaxDarken`) and lighten (`state *
//! glareMaxLighten`) go into the material bloom's apply pass as
//! `GLOW_APPLY.y` (the output alpha, which darkens the scene it is added
//! to) and `GLOW_APPLY.x` (added to every colour).
//!
//! INFERRED: the probe's sampling (8 x 8 over the disc of `spriteSize / 2`
//! at 480 lines to the frame's height, a sample counts when its distance
//! is past `FAR_DISTANCE`), the blend of the apply pass (one, one minus
//! source alpha), and the lens flare sprite is not drawn.

use crate::code::CodeValues;
use fastfile_iw6::walk::SunFlareRec;
use glam::{Mat4, Vec3, Vec4};
use render_h1::gpu::Gpu;

/// `BO2MP_IW6_SUN_FX=0`: no blind or glare from the sun.
pub fn on() -> bool {
    static S: crate::switch::Switch = crate::switch::Switch::on("BO2MP_IW6_SUN_FX");
    S.get()
}

/// `BO2MP_IW6_SUN_FROM_DVARS=1`: the engine's `r_sun_from_dvars` (the level's
/// own flare values are ignored for the `r_sun*` dvars' defaults).
pub fn from_dvars() -> bool {
    static S: crate::switch::Switch = crate::switch::Switch::off("BO2MP_IW6_SUN_FROM_DVARS");
    S.get()
}

/// The `r_sunblind_*` / `r_sunglare_*` / `r_sunsprite_size` dvar defaults
/// (READ) for a sun in direction `position`: angles in degrees become
/// cosines, times in seconds become milliseconds.
pub fn dvar_record(position: [f32; 3]) -> SunFlareRec {
    let c = |deg: f32| deg.to_radians().cos();
    SunFlareRec {
        has_valid_data: true,
        sprite_size: 16.0,
        blind_min_dot: c(30.0),
        blind_max_dot: c(5.0),
        blind_max_darken: 0.75,
        blind_fade_in: 500,
        blind_fade_out: 3000,
        glare_min_dot: c(30.0),
        glare_max_dot: c(5.0),
        glare_max_lighten: 0.75,
        glare_fade_in: 500,
        glare_fade_out: 3000,
        position,
        ..Default::default()
    }
}

/// A depth sample is sky when its distance is past this (units).
pub const FAR_DISTANCE: f32 = 20000.0;

/// The engine's fade step: `cur` toward `target` by `dt / fade` (a fade of
/// under one millisecond is a jump), never past it.
pub fn approach(cur: f32, target: f32, fade_in: i32, fade_out: i32, dt_ms: f32) -> f32 {
    if target > cur {
        if fade_in < 1 {
            return target;
        }
        (cur + dt_ms / fade_in as f32).min(target)
    } else if target < cur {
        if fade_out < 1 {
            return target;
        }
        (cur - dt_ms / fade_out as f32).max(target)
    } else {
        cur
    }
}

/// `(dot - min) / (max - min)` clamped to 0..1 (0 at or under `min`, 1 at or
/// over `max`; the dots are cosines, so `min < max`).
pub fn ramp(dot: f32, min: f32, max: f32) -> f32 {
    if dot <= min {
        0.0
    } else if dot < max {
        (dot - min) / (max - min)
    } else {
        1.0
    }
}

/// The blind and glare states.
#[derive(Clone, Copy, Debug)]
pub struct SunFx {
    pub rec: SunFlareRec,
    pub blind: f32,
    pub glare: f32,
    /// A visibility held for tests / offline tools (`Some`), else the
    /// probe's reading.
    pub forced: Option<f32>,
}

impl SunFx {
    pub fn new_opt(rec: Option<SunFlareRec>) -> Option<Self> {
        rec.and_then(Self::new)
    }

    pub fn new(rec: SunFlareRec) -> Option<Self> {
        let usable = rec.has_valid_data && (rec.blind_max_darken > 0.0 || rec.glare_max_lighten > 0.0);
        usable.then_some(Self { rec, blind: 0.0, glare: 0.0, forced: None })
    }

    /// The cosine between the view's forward axis and the sun's direction.
    pub fn dot(&self, forward: Vec3) -> f32 {
        Vec3::from_array(self.rec.position).normalize_or_zero().dot(forward)
    }

    /// Whether the sun is near enough the view's axis for any effect (so a
    /// frame away from it takes no probe pass).
    pub fn near(&self, dot: f32) -> bool {
        let r = &self.rec;
        (r.blind_max_darken > 0.0 && dot > r.blind_min_dot) || (r.glare_max_lighten > 0.0 && dot > r.glare_min_dot)
    }

    /// One frame: the states chase their targets; returns (darken, lighten).
    pub fn update(&mut self, dot: f32, visibility: f32, dt_ms: f32) -> (f32, f32) {
        let r = self.rec;
        let vis = self.forced.unwrap_or(visibility).clamp(0.0, 1.0);
        if r.blind_max_darken > 0.0 {
            let t = ramp(dot, r.blind_min_dot, r.blind_max_dot) * vis;
            self.blind = approach(self.blind, t, r.blind_fade_in, r.blind_fade_out, dt_ms);
        } else {
            self.blind = 0.0;
        }
        if r.glare_max_lighten > 0.0 {
            let t = ramp(dot, r.glare_min_dot, r.glare_max_dot) * vis;
            self.glare = approach(self.glare, t, r.glare_fade_in, r.glare_fade_out, dt_ms);
        } else {
            self.glare = 0.0;
        }
        (self.blind * r.blind_max_darken, self.glare * r.glare_max_lighten)
    }

    /// The probe's disc: the sun's pixel position, the radius in pixels
    /// (`spriteSize / 480` of the frame's height, half of it), or None
    /// when the sun is behind the view.
    pub fn disc(&self, view_projection: &Mat4, size: (u32, u32)) -> Option<([f32; 2], f32)> {
        let d = Vec3::from_array(self.rec.position).normalize_or_zero();
        let p = *view_projection * Vec4::new(d.x * 1000.0, d.y * 1000.0, d.z * 1000.0, 1.0);
        if p.w <= 1e-4 {
            return None;
        }
        let (x, y) = (p.x / p.w, p.y / p.w);
        let px = [(x * 0.5 + 0.5) * size.0 as f32, (0.5 - y * 0.5) * size.1 as f32];
        let radius = (self.rec.sprite_size / 480.0 * size.1 as f32 * 0.5).max(2.0);
        Some((px, radius))
    }
}

/// GLOW_APPLY for the apply pass: lighten added, darken as alpha, the
/// bloom's intensity as before.
pub fn glow_apply(code: &mut CodeValues, darken: f32, lighten: f32, intensity: f32) {
    code.set_vec("GLOW_APPLY", [lighten, darken, 0.0, intensity]);
}

const WGSL: &str = r"
struct U { centre: vec2<f32>, radius: f32, thresh: f32 };
@group(0) @binding(0) var<uniform> u: U;
@group(0) @binding(1) var depth: texture_2d<f32>;
@vertex fn vs(@builtin(vertex_index) i: u32) -> @builtin(position) vec4<f32> {
    let x = f32((i << 1u) & 2u) * 2.0 - 1.0;
    let y = f32(i & 2u) * 2.0 - 1.0;
    return vec4<f32>(x, y, 0.0, 1.0);
}
@fragment fn fs() -> @location(0) vec4<f32> {
    let dims = vec2<i32>(textureDimensions(depth));
    var seen = 0.0;
    var n = 0.0;
    for (var j = 0; j < 8; j++) {
        for (var i = 0; i < 8; i++) {
            let o = (vec2<f32>(f32(i), f32(j)) + 0.5) / 4.0 - 1.0;
            if (dot(o, o) > 1.0) { continue; }
            n += 1.0;
            let p = vec2<i32>(floor(u.centre + o * u.radius));
            if (p.x < 0 || p.y < 0 || p.x >= dims.x || p.y >= dims.y) { continue; }
            if (textureLoad(depth, p, 0).r < u.thresh) { seen += 1.0; }
        }
    }
    return vec4<f32>(seen / max(n, 1.0), 0.0, 0.0, 1.0);
}
";

/// The visibility pass: one pixel of R32Float.
pub struct Probe {
    gpu: Gpu,
    pipeline: wgpu::RenderPipeline,
    uniform: wgpu::Buffer,
    pub target: wgpu::Texture,
    view: wgpu::TextureView,
    layout: wgpu::BindGroupLayout,
    /// The group for the depth view it was made for.
    group: Option<(wgpu::TextureView, wgpu::BindGroup)>,
}

impl Probe {
    pub fn new(gpu: &Gpu) -> Self {
        let dev = &gpu.device;
        let module = dev.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some("iw6 sun probe"), source: wgpu::ShaderSource::Wgsl(WGSL.into()) });
        let layout = dev.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("iw6 sun probe"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: None },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture { sample_type: wgpu::TextureSampleType::Float { filterable: false }, view_dimension: wgpu::TextureViewDimension::D2, multisampled: false },
                    count: None,
                },
            ],
        });
        let pl = dev.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: Some("iw6 sun probe"), bind_group_layouts: &[Some(&layout)], immediate_size: 0 });
        let pipeline = dev.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("iw6 sun probe"),
            layout: Some(&pl),
            vertex: wgpu::VertexState { module: &module, entry_point: Some("vs"), compilation_options: Default::default(), buffers: &[] },
            primitive: Default::default(),
            depth_stencil: None,
            multisample: Default::default(),
            fragment: Some(wgpu::FragmentState { module: &module, entry_point: Some("fs"), compilation_options: Default::default(), targets: &[Some(wgpu::TextureFormat::R32Float.into())] }),
            multiview_mask: None,
            cache: None,
        });
        let uniform = dev.create_buffer(&wgpu::BufferDescriptor { label: Some("iw6 sun probe"), size: 16, usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST, mapped_at_creation: false });
        let target = render_h1::post::target(gpu, "iw6_sun_vis", 1, 1, wgpu::TextureFormat::R32Float);
        let view = target.create_view(&Default::default());
        Self { gpu: gpu.clone(), pipeline, uniform, target, view, layout, group: None }
    }

    /// Counts the sun's disc samples that see the sky in `depth` (the
    /// frame's FLOATZ). `thresh`: FLOATZ at `FAR_DISTANCE`
    /// (`near * band / FAR_DISTANCE`).
    pub fn record(&mut self, enc: &mut wgpu::CommandEncoder, depth: &wgpu::TextureView, centre: [f32; 2], radius: f32, thresh: f32) {
        let data = [centre[0], centre[1], radius, thresh];
        self.gpu.queue.write_buffer(&self.uniform, 0, bytemuck::cast_slice(&data));
        if self.group.as_ref().is_none_or(|(d, _)| d != depth) {
            let g = self.gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("iw6 sun probe"),
                layout: &self.layout,
                entries: &[
                    wgpu::BindGroupEntry { binding: 0, resource: self.uniform.as_entire_binding() },
                    wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(depth) },
                ],
            });
            self.group = Some((depth.clone(), g));
        }
        let Some((_, group)) = self.group.as_ref() else { return };
        let mut rp = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("iw6 sun probe"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &self.view,
                resolve_target: None,
                depth_slice: None,
                ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::BLACK), store: wgpu::StoreOp::Store },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        rp.set_pipeline(&self.pipeline);
        rp.set_bind_group(0, group, &[]);
        rp.draw(0..3, 0..1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rec() -> SunFlareRec {
        dvar_record([1.0, 0.0, 0.0])
    }

    #[test]
    fn approach_moves_by_the_fade_time_and_stops_at_the_target() {
        assert_eq!(approach(0.0, 1.0, 500, 3000, 100.0), 0.2);
        assert_eq!(approach(0.9, 1.0, 500, 3000, 100.0), 1.0);
        assert!((approach(1.0, 0.0, 500, 3000, 300.0) - 0.9).abs() < 1e-6);
        assert_eq!(approach(0.05, 0.0, 500, 3000, 300.0), 0.0);
        // No fade time: a jump.
        assert_eq!(approach(0.0, 1.0, 0, 0, 1.0), 1.0);
        assert_eq!(approach(1.0, 0.0, 500, 0, 1.0), 0.0);
        assert_eq!(approach(0.5, 0.5, 500, 500, 100.0), 0.5);
    }

    #[test]
    fn ramp_is_zero_outside_and_one_inside() {
        let (lo, hi) = (30f32.to_radians().cos(), 5f32.to_radians().cos());
        assert_eq!(ramp(0.0, lo, hi), 0.0);
        assert_eq!(ramp(1.0, lo, hi), 1.0);
        let mid = ramp(17.5f32.to_radians().cos(), lo, hi);
        assert!(mid > 0.3 && mid < 0.7, "{mid}");
    }

    #[test]
    fn looking_at_the_sun_blinds_and_glares_to_the_dvar_maxima() {
        let mut s = SunFx::new(rec()).unwrap();
        let fwd = Vec3::X;
        let dot = s.dot(fwd);
        assert!(s.near(dot));
        let mut out = (0.0, 0.0);
        for _ in 0..20 {
            out = s.update(dot, 1.0, 50.0);
        }
        assert!((out.0 - 0.75).abs() < 1e-6 && (out.1 - 0.75).abs() < 1e-6, "{out:?}");
        // Behind a wall: the states fade out over 3 s, 50 ms steps.
        let after = s.update(dot, 0.0, 50.0);
        assert!(after.0 < 0.75 && after.0 > 0.7, "{after:?}");
        for _ in 0..70 {
            s.update(dot, 0.0, 50.0);
        }
        assert_eq!(s.update(dot, 0.0, 50.0), (0.0, 0.0));
        // Looking away: no probe needed, the target is zero.
        assert!(!s.near(s.dot(Vec3::Y)));
        // A map without blind or glare has no state.
        assert!(SunFx::new(SunFlareRec { blind_max_darken: 0.0, glare_max_lighten: 0.0, ..rec() }).is_none());
        assert!(SunFx::new(SunFlareRec { has_valid_data: false, ..rec() }).is_none());
    }

    #[test]
    fn the_disc_is_where_the_sun_projects() {
        let s = SunFx::new(rec()).unwrap();
        // A view down +x with y as screen x and z up: the sun dead ahead is the middle.
        let view = Mat4::look_to_rh(Vec3::ZERO, Vec3::X, Vec3::Z);
        let proj = Mat4::perspective_infinite_reverse_rh(1.0, 16.0 / 9.0, 4.0);
        let (p, r) = s.disc(&(proj * view), (1280, 720)).unwrap();
        assert!((p[0] - 640.0).abs() < 1.0 && (p[1] - 360.0).abs() < 1.0, "{p:?}");
        assert!((r - 8.0 / 480.0 * 720.0).abs() < 1e-3);
        // Behind the view: no disc.
        let back = Mat4::look_to_rh(Vec3::ZERO, -Vec3::X, Vec3::Z);
        assert!(s.disc(&(proj * back), (1280, 720)).is_none());
    }
}
