//! bo2mp: what of this renderer a Black Ops II map takes in the shared look
//! (`BO2MP_BO2_SHARED_LOOK=1`, `docs/BO2_SHARED_LOOK.md`). A Black Ops II
//! map is not drawn here: its materials have no Black Ops III G-buffer
//! techniques, so the world stays in the fallback pass (its Black Ops III
//! branch, `t7_shade`), and two parts of this renderer are run for it:
//!
//! * the sun cascades: [`cascades`] fits Black Ops III's three cascades
//!   (`sun_shadow::fit`, the exe's fit, 32-texel tile snapping) round the
//!   Black Ops II camera for the Black Ops II sun. The map has no streamed
//!   sun shadow tree, so the fit gets a sun space of its own (x, y across
//!   the sun, z toward it, no offset) and a tree of one level, which only
//!   the tree lookups read. The matrices are absolute (world to cascade):
//!   the fallback pass shades with world positions, so the eye-relative
//!   lookup (`ShadowState::lookup`, d1fa2b0) and the G-buffer depth
//!   decode (6450aa49) that the deferred lighting needs do not arise; a test
//!   below holds the same "a world point stays put" property.
//! * the post passes: [`SharedPost`] runs Black Ops III's SMAA 1x
//!   (`post_aa::Smaa`) and camera motion blur (`post_mblur::MotionBlur`,
//!   from a rotation-only velocity: no depth is read) over the finished
//!   Black Ops II frame, from Black Ops III's `core_patch.ff`.

use std::collections::HashMap;
use std::path::Path;

use fastfile_t7::native::ViewSun;
use glam::{Mat4, Vec3};
use render_h1::code::Camera;
use render_h1::gpu::Gpu;

use crate::gpu::Tex;
use crate::sun_shadow::{CASCADES, FitCamera, ShadowInfo, ShadowState, fit};

/// The shadow distance (sun config 0x34) where none is asked: the cascades
/// then reach about 7,000 units (far), 3,500 and 900 (near) across.
pub const SHADOW_DISTANCE: f32 = 4000.0;
/// The cascades' depth range in world units along the sun (the fit's
/// projection: `depth = 1 - z / 16384` above the frustum's lowest point).
pub const DEPTH_RANGE: f32 = 16384.0;

/// The sun's config and the sun space the fit is given for a Black Ops II
/// sun (`toward_sun`: unit, toward the sun).
pub fn sun_inputs(toward_sun: Vec3, distance: f32) -> (ViewSun, ShadowInfo) {
    let d = toward_sun.normalize_or(Vec3::Z);
    let mut sun = ViewSun { direction: d.to_array(), shadow_mode: 3, ..Default::default() };
    // 0x30 the caster bias (world units), 0x34 the shadow distance, 0x38
    // the depth bias scale: the fit's own tests' values.
    sun.config[0x30 / 4] = 1.0f32.to_bits();
    sun.config[0x34 / 4] = distance.max(256.0).to_bits();
    sun.config[0x38 / 4] = 1.0f32.to_bits();
    let u = d.any_orthonormal_vector();
    let w = d.cross(u);
    let mut m = [[0f32; 4]; 4];
    for k in 0..3 {
        m[k] = [u[k], w[k], d[k], 0.0];
    }
    m[3] = [0.0, 0.0, 0.0, 1.0];
    let info = ShadowInfo { ints: [0; 4], matrix: m, size: [1024.0, 1024.0], cell: 64.0, height: 1.0, w60: [0; 2], streamed: 0 };
    (sun, info)
}

/// Three cascades, near to far.
#[derive(Clone, Debug)]
pub struct Cascades {
    /// World to cascade clip (column vectors; x, y in -1..1, depth 0..1
    /// growing away from the sun).
    pub clip_from_world: [Mat4; CASCADES],
    /// A texel's size in world units.
    pub texel: [f32; CASCADES],
    /// The fit's state (offline checks).
    pub state: ShadowState,
}

/// The cascades for `camera` with maps of `size` texels a side (a multiple
/// of 32).
pub fn cascades(toward_sun: Vec3, camera: &Camera, size: u32, distance: f32) -> Cascades {
    let (sun, info) = sun_inputs(toward_sun, distance);
    let mut cam = FitCamera::new(camera);
    cam.tiles = (size / 32).max(1) as i32;
    let state = fit(&sun, &info, &cam, false, false);
    let clip_from_world = std::array::from_fn(|c| Mat4::from_cols_array_2d(&state.view_projection(c)));
    Cascades { clip_from_world, texel: state.texel, state }
}

/// The shared look's sun visibility rule (the shader's `t7_shared_sun_vis`,
/// kept here to be tested): Black Ops II's own baked visibility of its
/// primary light when that light is the sun, times the live shadow; no sun
/// where the primary light is another. It never exceeds the baked
/// visibility Black Ops II's own shader multiplies its sun by.
pub fn shared_sun_visibility(primary_is_sun: bool, baked: f32, live: f32) -> f32 {
    if primary_is_sun { baked.clamp(0.0, 1.0) * live.clamp(0.0, 1.0) } else { 0.0 }
}

/// The settings of the post passes (`BO2MP_BO2_SHARED_AA=0` no SMAA,
/// `BO2MP_BO2_SHARED_MBLUR=0` no motion blur, a number its scale (default
/// 1), `BO2MP_BO2_SHARED_MBLUR_QUALITY` 0 / 1 / 2 (default 1)).
#[derive(Clone, Copy, Debug)]
pub struct PostSettings {
    pub aa: bool,
    pub blur_scale: f32,
    pub blur_quality: usize,
}

impl PostSettings {
    pub fn from_env() -> Self {
        let var = |n: &str| {
            codlegacy_env::var(n)
                .ok()
                .map(|v| v.trim().to_ascii_lowercase())
        };
        let aa = !matches!(
            var("BO2MP_BO2_SHARED_AA").as_deref(),
            Some("0" | "off" | "false")
        );
        let blur_scale = match var("BO2MP_BO2_SHARED_MBLUR").as_deref() {
            Some("0" | "off" | "false") => 0.0,
            Some(v) => v.parse::<f32>().ok().filter(|s| s.is_finite() && *s >= 0.0).unwrap_or(1.0),
            None => 1.0,
        };
        let blur_quality = var("BO2MP_BO2_SHARED_MBLUR_QUALITY").and_then(|v| v.parse().ok()).unwrap_or(1usize).min(2);
        Self { aa, blur_scale, blur_quality }
    }
}

/// The camera's turn since the last frame, for the blur velocity: the
/// current rotation-only view-projection's inverse and the previous one
/// (the eye moved to the origin in both: a turn blurs, a step does not).
#[derive(Clone, Copy, Debug)]
pub struct Turn {
    pub inv_current: Mat4,
    pub previous: Mat4,
}

impl Turn {
    /// From two frames' view rotations (world to view, any handedness the
    /// projection matches) and projections.
    pub fn new(clip_from_view: Mat4, view_from_world: Mat4, prev_clip_from_view: Mat4, prev_view_from_world: Mat4) -> Self {
        let rot = |m: Mat4| {
            let mut r = m;
            r.w_axis = glam::Vec4::W;
            r
        };
        let cur = clip_from_view * rot(view_from_world);
        Self { inv_current: cur.inverse(), previous: prev_clip_from_view * rot(prev_view_from_world) }
    }
}

const SHADER: &str = r#"
struct P {
    size: vec4<f32>,
    flags: vec4<f32>,
    inv_current: mat4x4<f32>,
    previous: mat4x4<f32>,
}
@group(0) @binding(0) var src: texture_2d<f32>;
@group(0) @binding(1) var<uniform> p: P;

struct V {
    @builtin(position) pos: vec4<f32>,
}

@vertex
fn vs(@builtin(vertex_index) i: u32) -> V {
    let x = f32((i << 1u) & 2u);
    let y = f32(i & 2u);
    return V(vec4(x * 2.0 - 1.0, 1.0 - y * 2.0, 0.0, 1.0));
}

fn encode(c: vec3<f32>) -> vec3<f32> {
    let lo = c * 12.92;
    let hi = 1.055 * pow(max(c, vec3(0.0031308)), vec3(1.0 / 2.4)) - 0.055;
    return select(hi, lo, c <= vec3(0.0031308));
}

struct PrepOut {
    @location(0) colour: vec4<f32>,
    @location(1) luma: vec4<f32>,
}

// The frame into the work target, and its luma (display-encoded) for the
// edge pass.
@fragment
fn prep(v: V) -> PrepOut {
    let c = textureLoad(src, vec2<i32>(v.pos.xy), 0);
    var d = saturate(c.rgb);
    if (p.flags.x > 0.5) {
        d = encode(d);
    }
    let l = dot(d, vec3(0.2126, 0.7152, 0.0722));
    return PrepOut(c, vec4(l, l, l, 1.0));
}

// Black Ops III's blur velocity (`motion_vector_static_with_mb`'s second
// target): pixels moved since the last frame, clamped to 20, as v / 40 +
// 128 / 255 per axis. Here the camera's turn alone.
@fragment
fn velocity(v: V) -> @location(0) vec4<f32> {
    let ndc = vec2(v.pos.x / p.size.x * 2.0 - 1.0, 1.0 - v.pos.y / p.size.y * 2.0);
    let h = p.inv_current * vec4(ndc, 0.5, 1.0);
    let rest = vec4(128.0 / 255.0, 128.0 / 255.0, 1.0, 1.0);
    if (abs(h.w) < 1e-12) {
        return rest;
    }
    let q = p.previous * vec4(h.xyz / h.w, 0.0);
    if (q.w <= 1e-5) {
        return rest;
    }
    var px = (ndc - q.xy / q.w) * vec2(0.5 * p.size.x, -0.5 * p.size.y) * p.size.z;
    let len = length(px);
    if (len > 20.0) {
        px = px * (20.0 / len);
    }
    return vec4(px / 40.0 + vec2(128.0 / 255.0), 1.0, 1.0);
}

@fragment
fn out(v: V) -> @location(0) vec4<f32> {
    return textureLoad(src, vec2<i32>(v.pos.xy), 0);
}
"#;

const PARAMS_BYTES: u64 = 160;
const WORK_FORMAT: wgpu::TextureFormat = crate::post_aa::COLOUR_FORMAT;

struct Sized {
    width: u32,
    height: u32,
    work: Tex,
    luma: Tex,
    velocity: Tex,
}

/// Black Ops III's SMAA 1x and camera motion blur over a finished Black Ops
/// II frame.
pub struct SharedPost {
    gpu: Gpu,
    smaa: Option<crate::post_aa::Smaa>,
    mblur: Option<crate::post_mblur::MotionBlur>,
    layout: wgpu::BindGroupLayout,
    pipeline_layout: wgpu::PipelineLayout,
    module: wgpu::ShaderModule,
    prep: wgpu::RenderPipeline,
    velocity: wgpu::RenderPipeline,
    out: HashMap<wgpu::TextureFormat, wgpu::RenderPipeline>,
    params: wgpu::Buffer,
    sized: Option<Sized>,
    pub settings: PostSettings,
    pub report: Vec<String>,
}

impl SharedPost {
    /// From Black Ops III's `zone` folder (its `core_patch.ff`).
    pub fn new(gpu: &Gpu, zone_dir: &Path, settings: PostSettings) -> Result<Self, String> {
        let mut report = Vec::new();
        let patch = fastfile_t7::shared::zone(zone_dir, "core_patch").ok_or_else(|| format!("core_patch.ff not read in {}", zone_dir.display()))?;
        let smaa = if settings.aa {
            let shaders = crate::compute::code_shaders(&patch.image, &patch.capture);
            let im = ["core_patch", "core_common", "mp_common"]
                .iter()
                .filter_map(|z| fastfile_t7::shared::zone(zone_dir, z))
                .find_map(|z| z.capture.images.iter().find(|i| i.name == "$smaa_tex").and_then(|r| fastfile_t7::texture::zone_levels(r, &z.image).ok()))
                .ok_or("$smaa_tex not in core_patch, core_common or mp_common")?;
            let s = crate::post_aa::Smaa::new(gpu, &shaders, &im)?;
            report.push("bo2 shared post: SMAA 1x (smaa_pass1_compute_hq, the work queues, smaa_pass2v/h_compute) over the frame".to_owned());
            Some(s)
        } else {
            report.push("bo2 shared post: no SMAA (BO2MP_BO2_SHARED_AA=0)".to_owned());
            None
        };
        let mblur = if settings.blur_scale > 0.0 {
            let techs = crate::post_pass::code_techs(&patch.image, &patch.capture, &crate::post_mblur::MotionBlur::pass_names())?;
            let m = crate::post_mblur::MotionBlur::new(gpu, &techs, WORK_FORMAT, settings.blur_quality)?;
            report.push(format!("bo2 shared post: camera motion blur (mblur_tile_downsample_0/1, mblur_tile_neighbor, quality {}), scale {}, from the camera's turn", settings.blur_quality, settings.blur_scale));
            Some(m)
        } else {
            report.push("bo2 shared post: no motion blur (BO2MP_BO2_SHARED_MBLUR=0)".to_owned());
            None
        };
        let dev = &gpu.device;
        let module = dev.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some("bo2 shared post"), source: wgpu::ShaderSource::Wgsl(SHADER.into()) });
        let layout = dev.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("bo2 shared post"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture { sample_type: wgpu::TextureSampleType::Float { filterable: false }, view_dimension: wgpu::TextureViewDimension::D2, multisampled: false },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: wgpu::BufferSize::new(PARAMS_BYTES) },
                    count: None,
                },
            ],
        });
        let pipeline_layout = dev.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: Some("bo2 shared post"), bind_group_layouts: &[Some(&layout)], immediate_size: 0 });
        let params = dev.create_buffer(&wgpu::BufferDescriptor { label: Some("bo2 shared post params"), size: PARAMS_BYTES, usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST, mapped_at_creation: false });
        let mut s = Self {
            gpu: gpu.clone(),
            smaa,
            mblur,
            prep: pipeline(dev, &pipeline_layout, &module, "prep", &[WORK_FORMAT, wgpu::TextureFormat::R8Unorm]),
            velocity: pipeline(dev, &pipeline_layout, &module, "velocity", &[crate::motion::BLUR_VELOCITY_FORMAT]),
            out: HashMap::new(),
            layout,
            pipeline_layout,
            module,
            params,
            sized: None,
            settings,
            report,
        };
        s.report.shrink_to_fit();
        Ok(s)
    }

    pub fn has_smaa(&self) -> bool {
        self.smaa.is_some()
    }

    fn group(&self, view: &wgpu::TextureView) -> wgpu::BindGroup {
        self.gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("bo2 shared post"),
            layout: &self.layout,
            entries: &[wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(view) }, wgpu::BindGroupEntry { binding: 1, resource: self.params.as_entire_binding() }],
        })
    }

    fn draw(enc: &mut wgpu::CommandEncoder, label: &str, pipeline: &wgpu::RenderPipeline, group: &wgpu::BindGroup, targets: &[&wgpu::TextureView]) {
        let attachments: Vec<Option<wgpu::RenderPassColorAttachment>> = targets
            .iter()
            .map(|v| Some(wgpu::RenderPassColorAttachment { view: v, depth_slice: None, resolve_target: None, ops: wgpu::Operations { load: wgpu::LoadOp::Load, store: wgpu::StoreOp::Store } }))
            .collect();
        let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor { label: Some(label), color_attachments: &attachments, depth_stencil_attachment: None, timestamp_writes: None, occlusion_query_set: None, multiview_mask: None });
        pass.set_pipeline(pipeline);
        pass.set_bind_group(0, group, &[]);
        pass.draw(0..3, 0..1);
    }

    /// Records the passes: `src` (the finished frame, `w` x `h`, sampled;
    /// `src_srgb`: an sRGB view, so its values are linear) into `dst` (a
    /// render target of `dst_format`, the same size). `turn`: the camera's
    /// turn since the last frame (None: no blur this frame).
    #[allow(clippy::too_many_arguments)]
    pub fn record(&mut self, enc: &mut wgpu::CommandEncoder, src: &wgpu::TextureView, src_srgb: bool, dst: &wgpu::TextureView, dst_format: wgpu::TextureFormat, w: u32, h: u32, turn: Option<Turn>) -> Result<(), String> {
        if w == 0 || h == 0 {
            return Ok(());
        }
        if self.sized.as_ref().is_none_or(|s| s.width != w || s.height != h) {
            let gpu = &self.gpu;
            self.sized = Some(Sized {
                width: w,
                height: h,
                work: crate::gpu::target(gpu, "bo2 shared post work", w, h, WORK_FORMAT, wgpu::TextureUsages::STORAGE_BINDING | wgpu::TextureUsages::COPY_SRC),
                luma: crate::gpu::target(gpu, "bo2 shared post luma", w, h, wgpu::TextureFormat::R8Unorm, wgpu::TextureUsages::empty()),
                velocity: crate::gpu::target(gpu, "bo2 shared post velocity", w, h, crate::motion::BLUR_VELOCITY_FORMAT, wgpu::TextureUsages::COPY_SRC),
            });
        }
        if !self.out.contains_key(&dst_format) {
            let p = pipeline(&self.gpu.device, &self.pipeline_layout, &self.module, "out", &[dst_format]);
            self.out.insert(dst_format, p);
        }
        let mut params = [0f32; (PARAMS_BYTES / 4) as usize];
        params[0] = w as f32;
        params[1] = h as f32;
        params[2] = self.settings.blur_scale;
        params[4] = if src_srgb { 1.0 } else { 0.0 };
        if let Some(t) = turn {
            params[8..24].copy_from_slice(&t.inv_current.to_cols_array());
            params[24..40].copy_from_slice(&t.previous.to_cols_array());
        }
        self.gpu.queue.write_buffer(&self.params, 0, bytemuck::cast_slice(&params));
        let s = self.sized.as_ref().unwrap();
        let g = self.group(src);
        Self::draw(enc, "bo2 shared post prep", &self.prep, &g, &[&s.work.view, &s.luma.view]);
        if let Some(smaa) = self.smaa.as_mut() {
            smaa.record(enc, &s.work.view, &s.luma.view, w, h)?;
        }
        let make = |view: &wgpu::TextureView| {
            self.gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("bo2 shared post"),
                layout: &self.layout,
                entries: &[wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(view) }, wgpu::BindGroupEntry { binding: 1, resource: self.params.as_entire_binding() }],
            })
        };
        let mut g = make(&s.work.view);
        if let (Some(mb), Some(_)) = (self.mblur.as_mut(), turn) {
            Self::draw(enc, "bo2 shared post velocity", &self.velocity, &g, &[&s.velocity.view]);
            let blurred = mb.record(enc, &s.work.view, &s.velocity.view, w, h)?;
            g = make(blurred);
        }
        Self::draw(enc, "bo2 shared post out", &self.out[&dst_format], &g, &[dst]);
        Ok(())
    }

    /// The work target (the frame after SMAA) and the blur velocity of the
    /// last run (offline checks).
    pub fn targets(&self) -> Option<(&wgpu::Texture, &wgpu::Texture)> {
        let s = self.sized.as_ref()?;
        Some((&s.work.texture, &s.velocity.texture))
    }
}

fn pipeline(dev: &wgpu::Device, layout: &wgpu::PipelineLayout, module: &wgpu::ShaderModule, entry: &str, formats: &[wgpu::TextureFormat]) -> wgpu::RenderPipeline {
    let targets: Vec<Option<wgpu::ColorTargetState>> = formats.iter().map(|&f| Some(wgpu::ColorTargetState { format: f, blend: None, write_mask: wgpu::ColorWrites::ALL })).collect();
    dev.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some(entry),
        layout: Some(layout),
        vertex: wgpu::VertexState { module, entry_point: Some("vs"), compilation_options: Default::default(), buffers: &[] },
        primitive: wgpu::PrimitiveState::default(),
        depth_stencil: None,
        multisample: wgpu::MultisampleState::default(),
        fragment: Some(wgpu::FragmentState { module, entry_point: Some(entry), compilation_options: Default::default(), targets: &targets }),
        multiview_mask: None,
        cache: crate::pipecache::get(&dev).as_ref(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn camera(eye: Vec3, pitch: f32, yaw: f32) -> Camera {
        let view = render_h1::code::View { eye, angles: Vec3::new(pitch, yaw, 0.0), fov_x: 80.0, width: 160, height: 90, near: 4.0 };
        Camera::from(&view)
    }

    /// A world point lands on the same place inside a texel of each cascade
    /// however the camera steps and turns (the cascades move by whole
    /// tiles), and the cascades nest: near inside middle inside far.
    #[test]
    fn cascades_hold_still_and_nest() {
        let sun = Vec3::new(-0.45, 0.3, 0.84).normalize();
        let p = Vec3::new(820.0, -310.0, 40.0);
        let mut first: Option<Cascades> = None;
        for (step, pitch, yaw) in [(0.0, 5.0, 30.0), (0.37, 5.0, 30.0), (3.1, 4.0, 33.0), (55.0, 8.0, 20.0)] {
            let c = cascades(sun, &camera(Vec3::new(600.0 + step, -200.0 + step * 0.5, 60.0), pitch, yaw), 2048, SHADOW_DISTANCE);
            assert!(c.texel[0] < c.texel[1] && c.texel[1] < c.texel[2], "texels {:?}", c.texel);
            for k in 0..CASCADES {
                let q = c.clip_from_world[k] * p.extend(1.0);
                assert!(q.is_finite() && (q.w - 1.0).abs() < 1e-5);
                if let Some(f) = &first {
                    let q0 = f.clip_from_world[k] * p.extend(1.0);
                    // In texels: the change is a whole number of texels.
                    for a in 0..2 {
                        let d = (q[a] - q0[a]) * 1024.0;
                        assert!((d - d.round()).abs() < 0.02, "cascade {k} axis {a}: moved {d} texels");
                    }
                    // Depth: the same height toward the sun, up to the
                    // depth base's whole steps.
                }
            }
            if first.is_none() {
                first = Some(c);
            }
        }
    }

    /// Depth grows away from the sun, so a caster between the sun and a
    /// receiver is nearer (smaller), as the compare sampler reads it.
    #[test]
    fn depth_grows_away_from_the_sun() {
        let sun = Vec3::new(0.3, -0.2, 0.93).normalize();
        let c = cascades(sun, &camera(Vec3::new(0.0, 0.0, 64.0), 10.0, 0.0), 2048, SHADOW_DISTANCE);
        let receiver = Vec3::new(200.0, 0.0, 0.0);
        let caster = receiver + sun * 100.0;
        for k in 0..CASCADES {
            let a = c.clip_from_world[k] * receiver.extend(1.0);
            let b = c.clip_from_world[k] * caster.extend(1.0);
            assert!((a.x - b.x).abs() < 1e-4 && (a.y - b.y).abs() < 1e-4, "same texel along the sun");
            assert!(b.z < a.z, "cascade {k}: caster {} receiver {}", b.z, a.z);
            assert!(a.z > 0.0 && a.z < 1.0, "in range");
            assert!(((a.z - b.z) * DEPTH_RANGE - 100.0).abs() < 0.5, "16384 units over the depth range");
        }
    }

    /// The double-lighting rule: the shared look's sun is never brighter
    /// than Black Ops II's own sun term (`sun * baked * N.L`), whatever the
    /// live shadow says, and there is none where the primary light is not
    /// the sun.
    #[test]
    fn the_sun_only_modulates() {
        for baked in [0.0f32, 0.3, 1.0, 1.7] {
            for live in [0.0f32, 0.5, 1.0, 2.0] {
                let v = shared_sun_visibility(true, baked, live);
                assert!(v <= baked.clamp(0.0, 1.0) + 1e-6 && v >= 0.0);
                assert_eq!(shared_sun_visibility(false, baked, live), 0.0);
            }
        }
    }

    fn read(gpu: &Gpu, t: &wgpu::Texture, bpp: u32) -> Vec<u8> {
        let (w, h) = (t.width(), t.height());
        let row = (w * bpp).div_ceil(256) * 256;
        let buf = gpu.device.create_buffer(&wgpu::BufferDescriptor { label: None, size: u64::from(row * h), usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ, mapped_at_creation: false });
        let mut enc = gpu.device.create_command_encoder(&Default::default());
        enc.copy_texture_to_buffer(t.as_image_copy(), wgpu::TexelCopyBufferInfo { buffer: &buf, layout: wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(row), rows_per_image: None } }, t.size());
        gpu.queue.submit([enc.finish()]);
        buf.slice(..).map_async(wgpu::MapMode::Read, |_| {});
        let _ = gpu.device.poll(wgpu::PollType::wait_indefinitely());
        let m = buf.slice(..).get_mapped_range();
        let mut out = Vec::new();
        for y in 0..h {
            let o = (y * row) as usize;
            out.extend_from_slice(&m[o..o + (w * bpp) as usize]);
        }
        out
    }

    /// The passes over a synthetic frame (a hard diagonal edge, the camera
    /// turning 2 degrees): SMAA softens the edge, the blur spreads it, the
    /// output has no NaN and is not black. Needs Black Ops III's
    /// core_patch.ff (`T7_ZONE_DIR`) and a device (lavapipe will do). Run
    /// on its own (`cargo test -p render_t7 --lib bo2_shared -- --ignored`):
    /// in one process with `post_aa`'s and `post_mblur`'s device tests, the
    /// devices of earlier tests trip `memo`'s per-device pipelines (wgpu's
    /// storage panics in theirs); the game has one device.
    #[test]
    #[ignore = "GPU, run alone: cargo test -p render_t7 --lib bo2_shared -- --ignored"]
    fn post_runs_over_a_frame() {
        let dir = std::path::PathBuf::from(std::env::var("T7_ZONE_DIR").unwrap_or_else(|_| "/mnt/user-data/uploads/zone".into()));
        if !dir.join("core_patch.ff").exists() {
            return eprintln!("skipped: no core_patch.ff");
        }
        let Ok(gpu) = Gpu::headless() else { return eprintln!("skipped: no device") };
        let mut post = SharedPost::new(&gpu, &dir, PostSettings { aa: true, blur_scale: 1.0, blur_quality: 1 }).expect("shared post");
        let (w, h) = (192u32, 108u32);
        let src = crate::gpu::target(&gpu, "src", w, h, wgpu::TextureFormat::Rgba8Unorm, wgpu::TextureUsages::COPY_DST);
        let dst = crate::gpu::target(&gpu, "dst", w, h, wgpu::TextureFormat::Rgba8Unorm, wgpu::TextureUsages::COPY_SRC);
        let px: Vec<u8> = (0..h).flat_map(|y| (0..w).flat_map(move |x| if x * 2 > y * 3 + 20 { [230u8, 220, 200, 255] } else { [30u8, 35, 40, 255] })).collect();
        gpu.queue.write_texture(src.texture.as_image_copy(), &px, wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(w * 4), rows_per_image: None }, src.texture.size());
        let proj = Mat4::perspective_infinite_reverse_rh(1.0, w as f32 / h as f32, 4.0);
        let view = |yaw: f32| Mat4::from_rotation_y(yaw.to_radians());
        for (turn, label) in [(None, "still"), (Some(Turn::new(proj, view(2.0), proj, view(0.0))), "turning")] {
            let scope = gpu.device.push_error_scope(wgpu::ErrorFilter::Validation);
            let t0 = std::time::Instant::now();
            let mut enc = gpu.device.create_command_encoder(&Default::default());
            post.record(&mut enc, &src.view, false, &dst.view, wgpu::TextureFormat::Rgba8Unorm, w, h, turn).expect("record");
            gpu.queue.submit([enc.finish()]);
            let _ = gpu.device.poll(wgpu::PollType::wait_indefinitely());
            assert!(pollster::block_on(scope.pop()).is_none(), "validation error");
            let out = read(&gpu, &dst.texture, 4);
            let mean: f64 = out.iter().map(|&b| f64::from(b)).sum::<f64>() / out.len() as f64;
            let changed = out.chunks(4).zip(px.chunks(4)).filter(|(a, b)| a[..3].iter().zip(b.iter()).any(|(x, y)| x.abs_diff(*y) > 8)).count();
            eprintln!("{label}: {:.1} ms, mean {mean:.1}, {changed} pixels changed", t0.elapsed().as_secs_f32() * 1000.0);
            assert!(mean > 20.0, "{label}: black output");
            assert!(changed > 0, "{label}: the passes changed nothing");
            if label == "still" {
                assert!(changed < (w * h / 4) as usize, "still: only the edge changes ({changed})");
            }
        }
    }
}
