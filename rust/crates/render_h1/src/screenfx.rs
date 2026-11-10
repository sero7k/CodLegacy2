//! Screen-space effects for the native Modern Warfare Remastered renderer:
//! ambient occlusion (Graphics > AMBIENT OCCLUSION through the shared
//! `native_quality`), depth of field and camera motion blur. All three are
//! full-screen passes over the fogged HDR scene and the scene depth, drawn
//! before the veil and the tone map; each is off at LOW/MEDIUM as the shared
//! table says, and `BO2MP_H1_SSAO=0`, `BO2MP_H1_DOF=0`, `BO2MP_H1_MOTION_BLUR=0`
//! switch one off.
//!
//! INFERRED, not read from the exe: the game's own passes (`cs_hemi_ao_*`, the
//! `dof_*` and motion-blur programs in `code_post_gfx`) are not translated
//! yet, so the radii, strengths, the far blur band (1000 to 7000 inches, the
//! default of the sibling engine's depth of field) and the 1/120 s shutter
//! are ours. The look constants (fog, film, tone map) are never touched.

use crate::code::Camera;
use crate::gpu::Gpu;
use glam::{Mat4, Vec3};

/// Ambient occlusion: the world radius sampled (inches) and its strength.
pub const AO_RADIUS: f32 = 48.0;
/// Ours, not the exe's. It multiplies the whole lit scene (not only the
/// ambient term), so 0.65 darkened ULTRA about a quarter against LOW
/// (Terminal in game: mean 0.175 against 0.23 offline); 0.35 keeps the
/// contact shading. `BO2MP_H1_AO_STRENGTH` overrides it.
pub const AO_STRENGTH: f32 = 0.35;
/// Maps with automatic exposure: the meter re-lifts the darkening, and the
/// weaker AO made Highrise too bright, so they keep the first value.
pub const AO_STRENGTH_AUTO: f32 = 0.65;

fn ao_strength(auto_exposure: bool) -> f32 {
    std::env::var("BO2MP_H1_AO_STRENGTH").ok().and_then(|v| v.trim().parse().ok()).unwrap_or(if auto_exposure { AO_STRENGTH_AUTO } else { AO_STRENGTH })
}
/// Depth of field: the far band (inches) and the widest blur (share of the
/// frame's height).
pub const DOF_FAR_START: f32 = 1000.0;
pub const DOF_FAR_END: f32 = 7000.0;
pub const DOF_NEAR_START: f32 = 10.0;
pub const DOF_NEAR_END: f32 = 60.0;
pub const DOF_MAX_RADIUS: f32 = 0.006;
/// Motion blur: the shutter (seconds) and the longest blur (share of the width).
pub const SHUTTER: f32 = 1.0 / 120.0;
pub const MAX_LENGTH: f32 = 0.04;
pub const MB_TAPS: u32 = 10;

/// The blur length in uv for a pixel that moved `vel` (uv, per frame time
/// `dt`), under the shutter and the cap (aspect: height over width).
pub fn blur_velocity(vel: [f32; 2], dt: f32, aspect: f32) -> [f32; 2] {
    if dt <= 0.0 {
        return [0.0; 2];
    }
    let k = (SHUTTER / dt).min(1.0);
    let v = [vel[0] * k, vel[1] * k];
    let len = (v[0] * v[0] + (v[1] * aspect).powi(2)).sqrt();
    if len > MAX_LENGTH {
        let s = MAX_LENGTH / len;
        [v[0] * s, v[1] * s]
    } else {
        v
    }
}

/// The depth of field's circle of confusion (0..1) at view distance `z`.
pub fn coc(z: f32) -> f32 {
    let far = ((z - DOF_FAR_START) / (DOF_FAR_END - DOF_FAR_START)).clamp(0.0, 1.0);
    let near = ((DOF_NEAR_END - z) / (DOF_NEAR_END - DOF_NEAR_START)).clamp(0.0, 1.0);
    far.max(near)
}

/// The farthest a draw may start at, in inches, at draw distance 1.0 (the
/// shared quality scales it: 0.5 and 0.75 on LOW and MEDIUM). INFERRED: the
/// game's own per-model distances are not read; ULTRA and HIGH cull nothing.
pub const DRAW_DISTANCE_BASE: f32 = 14000.0;

/// The distance from `eye` to the nearest point of the box `lo..hi`.
pub fn box_distance(eye: Vec3, lo: Vec3, hi: Vec3) -> f32 {
    let d = (lo - eye).max(eye - hi).max(Vec3::ZERO);
    d.length()
}

/// What a frame turns on.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Enabled {
    pub ao: bool,
    pub dof: bool,
    pub motion_blur: bool,
}

impl Enabled {
    /// From the shared quality and the per-effect env switches.
    pub fn from_quality(q: &native_quality::Quality) -> Self {
        let off = |k: &str| codlegacy_env::var(k).is_ok_and(|v| v.trim() == "0");
        Self {
            ao: q.ambient_occlusion && !off("BO2MP_H1_SSAO"),
            dof: q.depth_of_field && !off("BO2MP_H1_DOF"),
            motion_blur: q.motion_blur && !off("BO2MP_H1_MOTION_BLUR"),
        }
    }
    pub fn any(self) -> bool {
        self.ao || self.dof || self.motion_blur
    }
}

const COMMON: &str = r"
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
    // depth range low, high, near plane, focal length in pixels
    depth: vec4<f32>,
    // ao: radius, strength, bias, taps
    ao: vec4<f32>,
    // dof: far start, far end, near start, near end
    dof: vec4<f32>,
    // dof max radius (px), motion: max length (uv), taps, 0
    misc: vec4<f32>,
};
@group(0) @binding(0) var<uniform> u: U;
@group(0) @binding(1) var colour: texture_2d<f32>;
@group(0) @binding(2) var depth: texture_depth_2d;
@group(0) @binding(3) var smp: sampler;
fn dnorm(px: vec2<i32>) -> f32 {
    let dims = vec2<i32>(textureDimensions(depth));
    let s = textureLoad(depth, clamp(px, vec2<i32>(0), dims - vec2<i32>(1)), 0);
    return (s - u.depth.x) / max(u.depth.y - u.depth.x, 1e-9);
}
fn view_z(d: f32) -> f32 {
    return u.depth.z / max(d, 1e-7);
}
fn ign(p: vec2<f32>) -> f32 {
    return fract(52.9829189 * fract(dot(p, vec2<f32>(0.06711056, 0.00583715))));
}
";

const AO: &str = r"
@fragment fn fs(v: Vo) -> @location(0) vec4<f32> {
    let px = vec2<i32>(v.p.xy);
    let here = textureLoad(colour, px, 0);
    let d = dnorm(px);
    if (d <= 1e-7) { return here; }
    let z = view_z(d);
    let r_px = clamp(u.ao.x * u.depth.w / z, 2.0, 64.0);
    let n = i32(u.ao.w);
    let rot = ign(v.p.xy) * 6.2831853;
    var occ = 0.0;
    for (var i = 0; i < n; i++) {
        let a = rot + f32(i) * 2.3999632;
        let rr = sqrt((f32(i) + 0.5) / f32(n)) * r_px;
        let o = vec2<i32>(vec2<f32>(cos(a), sin(a)) * rr);
        let zs = view_z(dnorm(px + o));
        let dz = z - zs;
        if (dz > u.ao.z * (1.0 + z * 0.002)) {
            occ += clamp(1.0 - dz / u.ao.x, 0.0, 1.0);
        }
    }
    let fade = 1.0 - smoothstep(1500.0, 3500.0, z);
    let ao = 1.0 - u.ao.y * fade * occ / f32(n);
    return vec4<f32>(here.rgb * ao, here.a);
}
";

const DOF: &str = r"
fn coc_of(z: f32) -> f32 {
    let far = clamp((z - u.dof.x) / (u.dof.y - u.dof.x), 0.0, 1.0);
    let near = clamp((u.dof.w - z) / (u.dof.w - u.dof.z), 0.0, 1.0);
    return max(far, near);
}
@fragment fn fs(v: Vo) -> @location(0) vec4<f32> {
    let px = vec2<i32>(v.p.xy);
    let here = textureLoad(colour, px, 0);
    let d = dnorm(px);
    if (d <= 1e-7) { return here; }
    let c0 = coc_of(view_z(d));
    if (c0 < 0.02) { return here; }
    let r = c0 * u.misc.x;
    var sum = here.rgb * (0.1 + c0);
    var wsum = 0.1 + c0;
    let rot = ign(v.p.xy) * 6.2831853;
    for (var i = 0; i < 16; i++) {
        let a = rot + f32(i) * 2.3999632;
        let rr = sqrt((f32(i) + 0.5) / 16.0) * r;
        let q = px + vec2<i32>(vec2<f32>(cos(a), sin(a)) * rr);
        let ds = dnorm(q);
        var cs = 0.5;
        if (ds > 1e-7) { cs = coc_of(view_z(ds)); }
        let w = 0.1 + cs;
        sum += textureLoad(colour, clamp(q, vec2<i32>(0), vec2<i32>(textureDimensions(colour)) - vec2<i32>(1)), 0).rgb * w;
        wsum += w;
    }
    return vec4<f32>(sum / wsum, here.a);
}
";

const MOTION: &str = r"
@fragment fn fs(v: Vo) -> @location(0) vec4<f32> {
    let size = vec2<f32>(textureDimensions(colour));
    let px = vec2<i32>(v.p.xy);
    let d = dnorm(px);
    let ndc = vec4<f32>(v.uv.x * 2.0 - 1.0, 1.0 - v.uv.y * 2.0, d, 1.0);
    let p = u.inv_vp * ndc;
    var q: vec4<f32>;
    if (d <= 1e-7) {
        q = u.prev_vp * vec4<f32>(p.xyz, 0.0);
    } else {
        q = u.prev_vp * vec4<f32>(p.xyz / p.w + u.moved.xyz, 1.0);
    }
    let here = textureSampleLevel(colour, smp, v.uv, 0.0);
    if (q.w <= 1e-4) { return here; }
    let prev_uv = vec2<f32>(q.x / q.w * 0.5 + 0.5, 0.5 - q.y / q.w * 0.5);
    var vel = (v.uv - prev_uv) * u.moved.w;
    let len = length(vel * vec2<f32>(1.0, size.y / size.x));
    if (len > u.misc.y) { vel *= u.misc.y / len; }
    if (length(vel * size) < 0.5) { return here; }
    let n = i32(u.misc.z);
    var sum = vec4<f32>(0.0);
    for (var i = 0; i < n; i++) {
        let t = (f32(i) + 0.5) / f32(n) - 0.5;
        sum += textureSampleLevel(colour, smp, clamp(v.uv + vel * t, vec2<f32>(0.0), vec2<f32>(1.0)), 0.0);
    }
    return vec4<f32>(sum.rgb / f32(n), here.a);
}
";

struct Effect {
    pipe: wgpu::RenderPipeline,
    /// Groups reading (the scene, the scratch), made when the size changes.
    groups: Option<[wgpu::BindGroup; 2]>,
}

/// The passes, the scratch image and the last frame's view.
pub struct ScreenFx {
    gpu: Gpu,
    layout: wgpu::BindGroupLayout,
    effects: [Effect; 3],
    uniform: wgpu::Buffer,
    sampler: wgpu::Sampler,
    scratch: Option<(u32, u32, wgpu::TextureView)>,
    prev: Option<(Mat4, Vec3)>,
    last: Option<std::time::Instant>,
    /// A frame time a host sets (seconds) instead of the clock's.
    pub dt_override: Option<f32>,
    /// A host sets this to leave the motion blur pass out while the view is
    /// exactly where it was last frame (no velocity: nothing to blur).
    pub skip_still: bool,
    /// The depth view the groups were made for (a new one remakes them).
    depth_id: Option<wgpu::TextureView>,
}

const NAMES: [&str; 3] = ["h1 ao", "h1 dof", "h1 motion blur"];

impl ScreenFx {
    pub fn new(gpu: &Gpu) -> Self {
        let entry = |binding: u32, ty: wgpu::BindingType| wgpu::BindGroupLayoutEntry { binding, visibility: wgpu::ShaderStages::VERTEX_FRAGMENT, ty, count: None };
        let layout = gpu.device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("h1 screenfx"),
            entries: &[
                entry(0, wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: None }),
                entry(1, wgpu::BindingType::Texture { sample_type: wgpu::TextureSampleType::Float { filterable: true }, view_dimension: wgpu::TextureViewDimension::D2, multisampled: false }),
                entry(2, wgpu::BindingType::Texture { sample_type: wgpu::TextureSampleType::Depth, view_dimension: wgpu::TextureViewDimension::D2, multisampled: false }),
                entry(3, wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering)),
            ],
        });
        let pipe_layout = gpu.device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: Some("h1 screenfx"), bind_group_layouts: &[Some(&layout)], immediate_size: 0 });
        let make = |i: usize, body: &str| {
            let src = format!("{COMMON}{body}");
            let module = gpu.device.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some(NAMES[i]), source: wgpu::ShaderSource::Wgsl(src.into()) });
            Effect {
                pipe: gpu.device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                    label: Some(NAMES[i]),
                    layout: Some(&pipe_layout),
                    vertex: wgpu::VertexState { module: &module, entry_point: Some("vs"), compilation_options: Default::default(), buffers: &[] },
                    primitive: Default::default(),
                    depth_stencil: None,
                    multisample: Default::default(),
                    fragment: Some(wgpu::FragmentState { module: &module, entry_point: Some("fs"), compilation_options: Default::default(), targets: &[Some(wgpu::TextureFormat::Rgba16Float.into())] }),
                    multiview_mask: None,
                    cache: None,
                }),
                groups: None,
            }
        };
        let uniform = gpu.device.create_buffer(&wgpu::BufferDescriptor { label: Some("h1 screenfx"), size: 4 * (16 * 2 + 4 * 5), usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST, mapped_at_creation: false });
        let sampler = gpu.device.create_sampler(&wgpu::SamplerDescriptor { label: Some("h1 screenfx"), mag_filter: wgpu::FilterMode::Linear, min_filter: wgpu::FilterMode::Linear, ..Default::default() });
        Self { gpu: gpu.clone(), layout, effects: [make(0, AO), make(1, DOF), make(2, MOTION)], uniform, sampler, scratch: None, prev: None, last: None, dt_override: None, skip_still: false, depth_id: None }
    }

    /// The scratch image (where a pass count that is odd leaves the result).
    pub fn scratch_view(&self) -> Option<&wgpu::TextureView> {
        self.scratch.as_ref().map(|s| &s.2)
    }

    /// Remembers this frame's view without drawing (every effect off).
    pub fn skip(&mut self, camera: &Camera) {
        self.prev = Some((camera.view_projection, camera.eye));
        self.last = Some(std::time::Instant::now());
    }

    /// Draws the enabled effects over `scene` (the fogged HDR image, with
    /// `depth` its sampled depth, reversed, in `range`). Returns whether the
    /// result is in the scratch image (else it is in `scene`).
    #[allow(clippy::too_many_arguments)]
    pub fn record(&mut self, enc: &mut wgpu::CommandEncoder, scene: &wgpu::TextureView, depth: &wgpu::TextureView, camera: &Camera, range: (f32, f32), size: (u32, u32), on: Enabled, auto_exposure: bool) -> bool {
        let now = std::time::Instant::now();
        let measured = self.last.replace(now).map_or(0.0, |t| (now - t).as_secs_f32());
        let dt = self.dt_override.map_or(measured, |d| if self.prev.is_some() { d } else { 0.0 });
        let prev = self.prev.replace((camera.view_projection, camera.eye));
        if !on.any() {
            return false;
        }
        let (w, h) = size;
        if self.scratch.as_ref().is_none_or(|s| (s.0, s.1) != (w, h)) {
            let tex = crate::post::target(&self.gpu, "h1_screenfx", w, h, wgpu::TextureFormat::Rgba16Float);
            self.scratch = Some((w, h, tex.create_view(&Default::default())));
            for e in &mut self.effects {
                e.groups = None;
            }
        }
        if self.depth_id.as_ref() != Some(depth) {
            self.depth_id = Some(depth.clone());
            for e in &mut self.effects {
                e.groups = None;
            }
        }
        let Some(scratch) = self.scratch.as_ref().map(|s| s.2.clone()) else { return false };
        let (lo, hi) = (range.0.min(range.1), range.0.max(range.1));
        let focal = 0.5 * h as f32 / camera.tan_y.max(1e-4);
        let (prev_vp, prev_eye) = prev.unwrap_or((camera.view_projection, camera.eye));
        let moved = camera.eye - prev_eye;
        let mut f: Vec<f32> = Vec::with_capacity(52);
        f.extend_from_slice(&camera.view_projection.inverse().to_cols_array());
        f.extend_from_slice(&prev_vp.to_cols_array());
        f.extend_from_slice(&[moved.x, moved.y, moved.z, if dt > 0.0 { (SHUTTER / dt).min(1.0) } else { 0.0 }]);
        f.extend_from_slice(&[lo, hi, camera.near, focal]);
        f.extend_from_slice(&[AO_RADIUS, ao_strength(auto_exposure), 1.5, 12.0]);
        f.extend_from_slice(&[DOF_FAR_START, DOF_FAR_END, DOF_NEAR_START, DOF_NEAR_END]);
        f.extend_from_slice(&[DOF_MAX_RADIUS * h as f32, MAX_LENGTH, MB_TAPS as f32, 0.0]);
        self.gpu.queue.write_buffer(&self.uniform, 0, bytemuck_cast(&f));
        let mut in_scratch = false;
        let still = self.skip_still && prev.is_some_and(|(vp, eye)| vp == camera.view_projection && eye == camera.eye);
        let want = [on.ao, on.dof, on.motion_blur && prev.is_some() && dt > 0.0 && !still];
        for (i, e) in self.effects.iter_mut().enumerate() {
            if !want[i] {
                continue;
            }
            if e.groups.is_none() {
                let mk = |view: &wgpu::TextureView| {
                    self.gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
                        label: Some(NAMES[i]),
                        layout: &self.layout,
                        entries: &[
                            wgpu::BindGroupEntry { binding: 0, resource: self.uniform.as_entire_binding() },
                            wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(view) },
                            wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::TextureView(depth) },
                            wgpu::BindGroupEntry { binding: 3, resource: wgpu::BindingResource::Sampler(&self.sampler) },
                        ],
                    })
                };
                e.groups = Some([mk(scene), mk(&scratch)]);
            }
            let Some(groups) = e.groups.as_ref() else { continue };
            let (group, out) = if in_scratch { (&groups[1], scene) } else { (&groups[0], &scratch) };
            let mut rp = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some(NAMES[i]),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment { view: out, resolve_target: None, depth_slice: None, ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::BLACK), store: wgpu::StoreOp::Store } })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            rp.set_pipeline(&e.pipe);
            rp.set_bind_group(0, group, &[]);
            rp.draw(0..3, 0..1);
            drop(rp);
            in_scratch = !in_scratch;
        }
        in_scratch
    }
}

fn bytemuck_cast(f: &[f32]) -> &[u8] {
    // SAFETY: f32 has no padding and any bit pattern is a valid u8 sequence.
    unsafe { std::slice::from_raw_parts(f.as_ptr().cast::<u8>(), std::mem::size_of_val(f)) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn coc_is_zero_in_the_middle_and_grows_toward_both_ends() {
        assert_eq!(coc(500.0), 0.0);
        assert_eq!(coc(DOF_FAR_START), 0.0);
        assert!((coc(DOF_FAR_END) - 1.0).abs() < 1e-6);
        assert!(coc(4000.0) > 0.4 && coc(4000.0) < 0.6);
        assert_eq!(coc(DOF_NEAR_START), 1.0);
        assert_eq!(coc(DOF_NEAR_END), 0.0);
    }

    #[test]
    fn blur_is_capped_and_follows_the_shutter() {
        // A slow frame (1/30 s) shortens the blur to a quarter.
        let v = blur_velocity([0.008, 0.0], 1.0 / 30.0, 0.5625);
        assert!((v[0] - 0.002).abs() < 1e-6);
        // A huge move is capped at MAX_LENGTH.
        let v = blur_velocity([0.5, 0.0], 1.0 / 120.0, 0.5625);
        assert!((v[0] - MAX_LENGTH).abs() < 1e-6);
        assert_eq!(blur_velocity([0.1, 0.1], 0.0, 1.0), [0.0; 2]);
    }

    #[test]
    fn box_distance_is_zero_inside_and_exact_outside() {
        let (lo, hi) = (Vec3::splat(-1.0), Vec3::splat(1.0));
        assert_eq!(box_distance(Vec3::ZERO, lo, hi), 0.0);
        assert!((box_distance(Vec3::new(4.0, 0.0, 0.0), lo, hi) - 3.0).abs() < 1e-6);
        assert!((box_distance(Vec3::new(4.0, 5.0, 0.0), lo, hi) - 5.0).abs() < 1e-6);
    }

    #[test]
    fn effects_follow_the_quality_preset() {
        let low = native_quality::Quality::preset(native_quality::Preset::Low);
        let ultra = native_quality::Quality::preset(native_quality::Preset::Ultra);
        assert!(!Enabled::from_quality(&low).any());
        let e = Enabled::from_quality(&ultra);
        assert!(e.ao && e.dof && e.motion_blur);
    }
}
