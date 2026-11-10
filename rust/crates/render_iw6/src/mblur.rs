//! Motion blur. Two modes (`BO2MP_IW6_MOTION_BLUR_MODE`):
//!
//! - `velocity` (default): Ghosts' velocity-buffer blur, the `r_mbFastEnable`
//!   0 chain (`docs/exe_notes/IW6_POST.md` 5). A velocity picture
//!   (`SCENE_VELOCITY`, two 8-bit channels, the UV displacement since the
//!   last frame stored as `sign(v) * sqrt(|v|) * 0.5 + 0.5`, which is how the
//!   engine's shaders decode it), a tile maximum along x then y
//!   (`postfx_mblurhq_velocity_pass0` twice, 20 texels per tile), a 3 x 3
//!   neighbour maximum (`postfx_mblurhq_velocity_pass1`, `SCENE_VELOCITY_PASS1`),
//!   then `postfx_mblurhq` (scene, velocity, neighbour maxima, FLOATZ; up to
//!   `r_mbPostfxMaxNumSamples` = 15 taps). Those four passes are the zones'
//!   own shaders; only the velocity picture is ours.
//! - `old`: `render_h1`'s camera-reprojection blur (the earlier INFERRED path).
//!
//! The engine writes the velocity picture while it draws (a `velocity_static`
//! technique for the world, `*_velocity_*` for skinned models, `velocity_vm`
//! for the view model, each from the previous frame's matrices and bones).
//! Here the world's velocity comes from `FLOATZ` and the last frame's
//! camera in one full-screen pass: for static geometry that is the same
//! number the world technique writes (the point's clip position now against
//! the clip position of the same world point under last frame's matrices).
//! INFERRED / not done: skinned models and the view model get no velocity of
//! their own (the view model, nearer than `VIEW_MODEL_Z`, is held still so
//! the gun does not smear with the camera); the camera influence dvars are
//! their defaults (1, 1: no scaling).
//!
//! Off on LOW and by `BO2MP_IW6_MOTION_BLUR=0`. Off when the view has not
//! moved (no velocity, nothing drawn: byte-identical to the effect off).

use crate::code::CodeValues;
use crate::post::Post;
use glam::{Mat4, Vec3};
use render_h1::gpu::Gpu;

/// `BO2MP_IW6_MOTION_BLUR=0`: no motion blur whatever the preset says.
pub fn on() -> bool {
    static S: crate::switch::Switch = crate::switch::Switch::on("BO2MP_IW6_MOTION_BLUR");
    S.get()
}

/// Which blur runs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    /// Ghosts' velocity-buffer chain.
    Velocity,
    /// The camera-reprojection blur.
    Camera,
}

/// `BO2MP_IW6_MOTION_BLUR_MODE`: `old` / `camera` / `reprojection` is the
/// camera-reprojection blur, anything else (default) the velocity chain.
pub fn mode() -> Mode {
    use std::sync::atomic::{AtomicU8, AtomicU32, Ordering::Relaxed};
    static SEEN: AtomicU32 = AtomicU32::new(0);
    static VAL: AtomicU8 = AtomicU8::new(0);
    let g = crate::switch::generation();
    if SEEN.load(Relaxed) != g {
        let m = parse_mode(std::env::var("BO2MP_IW6_MOTION_BLUR_MODE").ok().as_deref());
        VAL.store(u8::from(m == Mode::Camera), Relaxed);
        SEEN.store(g, Relaxed);
    }
    if VAL.load(Relaxed) == 1 { Mode::Camera } else { Mode::Velocity }
}

pub fn parse_mode(v: Option<&str>) -> Mode {
    match v.map(|s| s.trim().to_ascii_lowercase()).as_deref() {
        Some("old" | "camera" | "reprojection") => Mode::Camera,
        _ => Mode::Velocity,
    }
}

/// Texels of the velocity picture one tile of the maximum passes covers (READ:
/// the first tile pass takes 20 taps at 1 texel steps).
pub const TILE: u32 = 20;
/// `r_mbPostfxMaxNumSamples`' default (READ), the `MOTION_BLUR_HQ_PARAMS` y.
pub const MAX_SAMPLES: f32 = 15.0;
/// Pixels nearer than this (view distance) are the view model: held still.
pub const VIEW_MODEL_Z: f32 = 24.0;
/// FLOATZ = near * BAND / distance.
const BAND: f32 = 0.984375;

pub const VEL_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rg8Unorm;

pub const TECH_TILE: &str = "postfx_mblurhq_velocity_pass0";
pub const TECH_NEIGHBOUR: &str = "postfx_mblurhq_velocity_pass1";
pub const TECH_APPLY: &str = "postfx_mblurhq";

/// The tile target's size for a render of `w` x `h`.
pub fn tile_size(w: u32, h: u32) -> (u32, u32) {
    (w.div_ceil(TILE).max(1), h.div_ceil(TILE).max(1))
}

/// A UV displacement as the velocity picture holds it (two 8-bit channels).
pub fn encode(v: f32) -> u8 {
    let v = v.clamp(-1.0, 1.0);
    let e = v.signum() * v.abs().sqrt() * 0.5 + 0.5;
    (e * 255.0 + 0.5).floor() as u8
}

/// What the engine's shaders read back (`v = e * 2 - 1`, `v * |v|`).
pub fn decode(b: u8) -> f32 {
    let t = b as f32 / 255.0 * 2.0 - 1.0;
    t * t.abs()
}

/// `MOTION_BLUR_HQ_TILE_MAX_PARAMS` of the first tile pass (along x) from
/// the source width and the tile target's width: (offset x, 0, step x, 0).
pub fn tile_params_x(src_w: u32, tile_w: u32) -> [f32; 4] {
    let (s, t) = (1.0 / src_w as f32, 1.0 / tile_w as f32);
    [0.5 * s - 0.5 * t, 0.0, s, 0.0]
}

/// The second tile pass (along y).
pub fn tile_params_y(src_h: u32, tile_h: u32) -> [f32; 4] {
    let (s, t) = (1.0 / src_h as f32, 1.0 / tile_h as f32);
    [0.0, 0.5 * s - 0.5 * t, 0.0, s]
}

/// The apply pass's `MOTION_BLUR_HQ_PARAMS` (READ: 0, max samples, 1, 0).
pub fn apply_params() -> [f32; 4] {
    [0.0, MAX_SAMPLES, 1.0, 0.0]
}

/// The last frame's view, kept between frames.
#[derive(Clone, Copy, Debug)]
pub struct Prev {
    pub view_projection: Mat4,
    pub eye: Vec3,
}

/// The view has not moved: there is no velocity.
pub fn still(prev: &Prev, vp: &Mat4, eye: Vec3) -> bool {
    prev.view_projection == *vp && prev.eye == eye
}

/// The same reprojection as the velocity shader, on the CPU (the tests, and
/// the reference for the shader): the UV displacement `uv_now - uv_prev` of
/// the point at view distance `z` (None: at infinity, rotation only) seen at
/// `uv` (0..1, y down).
#[allow(clippy::too_many_arguments)]
pub fn reproject(uv: [f32; 2], z: Option<f32>, tan: [f32; 2], inv_rotation: &Mat4, eye: Vec3, prev: &Prev) -> Option<[f32; 2]> {
    let ndc = [uv[0] * 2.0 - 1.0, 1.0 - uv[1] * 2.0];
    let zz = z.unwrap_or(1.0);
    let view = Vec3::new(ndc[0] * tan[0] * zz, ndc[1] * tan[1] * zz, zz);
    let rel = inv_rotation.transform_vector3(view);
    let q = match z {
        Some(_) => prev.view_projection * (rel + (eye - prev.eye)).extend(1.0),
        None => prev.view_projection * rel.extend(0.0),
    };
    if q.w <= 1e-4 {
        return None;
    }
    let p = [q.x / q.w * 0.5 + 0.5, 0.5 - q.y / q.w * 0.5];
    Some([uv[0] - p[0], uv[1] - p[1]])
}

const VELOCITY_WGSL: &str = r"
struct U {
    inv_rot: mat4x4<f32>,
    prev_vp: mat4x4<f32>,
    // eye minus last frame's eye (xyz), near * band (w)
    moved: vec4<f32>,
    // tan x, tan y, view-model distance, 0
    cam: vec4<f32>,
};
@group(0) @binding(0) var<uniform> u: U;
@group(0) @binding(1) var floatz: texture_2d<f32>;
struct Vo { @builtin(position) p: vec4<f32>, @location(0) uv: vec2<f32> };
@vertex fn vs(@builtin(vertex_index) i: u32) -> Vo {
    let x = f32((i << 1u) & 2u);
    let y = f32(i & 2u);
    return Vo(vec4<f32>(x * 2.0 - 1.0, 1.0 - y * 2.0, 0.0, 1.0), vec2<f32>(x, y));
}
fn enc(v: vec2<f32>) -> vec2<f32> {
    let c = clamp(v, vec2<f32>(-1.0), vec2<f32>(1.0));
    return sign(c) * sqrt(abs(c)) * 0.5 + vec2<f32>(0.5);
}
@fragment fn fs(v: Vo) -> @location(0) vec4<f32> {
    let dims = vec2<i32>(textureDimensions(floatz));
    let fz = textureLoad(floatz, clamp(vec2<i32>(v.p.xy), vec2<i32>(0), dims - vec2<i32>(1)), 0).x;
    let ndc = vec2<f32>(v.uv.x * 2.0 - 1.0, 1.0 - v.uv.y * 2.0);
    let sky = fz <= 1e-7;
    var z = 1.0;
    if (!sky) { z = u.moved.w / fz; }
    if (!sky && z < u.cam.z) { return vec4<f32>(0.5, 0.5, 0.0, 1.0); }
    let view = vec4<f32>(ndc.x * u.cam.x * z, ndc.y * u.cam.y * z, z, 0.0);
    let rel = (u.inv_rot * view).xyz;
    var q: vec4<f32>;
    if (sky) {
        q = u.prev_vp * vec4<f32>(rel, 0.0);
    } else {
        q = u.prev_vp * vec4<f32>(rel + u.moved.xyz, 1.0);
    }
    if (q.w <= 1e-4) { return vec4<f32>(0.5, 0.5, 0.0, 1.0); }
    let prev_uv = vec2<f32>(q.x / q.w * 0.5 + 0.5, 0.5 - q.y / q.w * 0.5);
    return vec4<f32>(enc(v.uv - prev_uv), 0.0, 1.0);
}
";

/// The velocity pass: FLOATZ and last frame's camera into `SCENE_VELOCITY`.
pub struct VelocityPass {
    pipe: wgpu::RenderPipeline,
    layout: wgpu::BindGroupLayout,
    uniform: wgpu::Buffer,
    group: Option<(wgpu::TextureView, wgpu::BindGroup)>,
}

impl VelocityPass {
    pub fn new(gpu: &Gpu) -> Self {
        let d = &gpu.device;
        let layout = d.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("iw6 velocity"),
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
        let pl = d.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: Some("iw6 velocity"), bind_group_layouts: &[Some(&layout)], immediate_size: 0 });
        let module = d.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some("iw6 velocity"), source: wgpu::ShaderSource::Wgsl(VELOCITY_WGSL.into()) });
        let pipe = d.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("iw6 velocity"),
            layout: Some(&pl),
            vertex: wgpu::VertexState { module: &module, entry_point: Some("vs"), compilation_options: Default::default(), buffers: &[] },
            primitive: Default::default(),
            depth_stencil: None,
            multisample: Default::default(),
            fragment: Some(wgpu::FragmentState { module: &module, entry_point: Some("fs"), compilation_options: Default::default(), targets: &[Some(VEL_FORMAT.into())] }),
            multiview_mask: None,
            cache: None,
        });
        let uniform = d.create_buffer(&wgpu::BufferDescriptor { label: Some("iw6 velocity"), size: 4 * (32 + 8), usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST, mapped_at_creation: false });
        Self { pipe, layout, uniform, group: None }
    }

    fn record(&mut self, gpu: &Gpu, enc: &mut wgpu::CommandEncoder, floatz: &wgpu::TextureView, dst: &wgpu::TextureView, u: &[f32; 40]) {
        gpu.queue.write_buffer(&self.uniform, 0, f32_bytes(u));
        if self.group.as_ref().is_none_or(|(v, _)| v != floatz) {
            let g = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("iw6 velocity"),
                layout: &self.layout,
                entries: &[
                    wgpu::BindGroupEntry { binding: 0, resource: self.uniform.as_entire_binding() },
                    wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(floatz) },
                ],
            });
            self.group = Some((floatz.clone(), g));
        }
        let Some((_, g)) = self.group.as_ref() else { return };
        let mut rp = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("iw6 velocity"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment { view: dst, resolve_target: None, depth_slice: None, ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::BLACK), store: wgpu::StoreOp::Store } })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        rp.set_pipeline(&self.pipe);
        rp.set_bind_group(0, g, &[]);
        rp.draw(0..3, 0..1);
    }
}

fn f32_bytes(f: &[f32]) -> &[u8] {
    // SAFETY: f32 has no padding and any bit pattern is a valid u8 run.
    unsafe { std::slice::from_raw_parts(f.as_ptr().cast::<u8>(), std::mem::size_of_val(f)) }
}

struct Targets {
    size: (u32, u32),
    vel: wgpu::TextureView,
    tile_a: wgpu::TextureView,
    tile_b: wgpu::TextureView,
    tile_c: wgpu::TextureView,
    out: wgpu::TextureView,
}

/// The velocity-buffer blur: the pass, its targets and the last frame's view.
pub struct Hq {
    vel: VelocityPass,
    t: Option<Targets>,
    pub prev: Option<Prev>,
}

impl Hq {
    pub fn new(gpu: &Gpu) -> Self {
        Self { vel: VelocityPass::new(gpu), t: None, prev: None }
    }

    /// Remember this frame's view (also while the blur is off).
    pub fn remember(&mut self, vp: Mat4, eye: Vec3) {
        self.prev = Some(Prev { view_projection: vp, eye });
    }

    /// Whether the zones hold the four techniques.
    pub fn available(post: &Post) -> bool {
        post.has(TECH_TILE) && post.has(TECH_NEIGHBOUR) && post.has(TECH_APPLY)
    }

    /// The chain over `scene` (`format`: the scene's). Returns the blurred
    /// picture, or None when nothing was drawn (first frame, still view or
    /// an error: the caller keeps `scene`). Updates the remembered view.
    #[allow(clippy::too_many_arguments)]
    pub fn record(
        &mut self,
        gpu: &Gpu,
        post: &mut Post,
        errors: &mut Vec<String>,
        enc: &mut wgpu::CommandEncoder,
        scene: &wgpu::TextureView,
        floatz: &wgpu::TextureView,
        code: &CodeValues,
        scratch_code: &mut CodeValues,
        camera: &render_h1::code::Camera,
        size: (u32, u32),
        format: wgpu::TextureFormat,
    ) -> Option<&wgpu::TextureView> {
        let prev = self.prev.replace(Prev { view_projection: camera.view_projection, eye: camera.eye })?;
        if still(&prev, &camera.view_projection, camera.eye) {
            return None;
        }
        let (w, h) = size;
        if self.t.as_ref().is_none_or(|t| t.size != size) {
            let (tw, th) = tile_size(w, h);
            let mk = |n: &str, w: u32, h: u32, f| render_h1::post::target(gpu, n, w, h, f).create_view(&Default::default());
            self.t = Some(Targets {
                size,
                vel: mk("iw6_velocity", w, h, VEL_FORMAT),
                tile_a: mk("iw6_velocity_tile_x", tw, h, VEL_FORMAT),
                tile_b: mk("iw6_velocity_tile", tw, th, VEL_FORMAT),
                tile_c: mk("iw6_velocity_neighbour", tw, th, VEL_FORMAT),
                out: mk("iw6_mblur_out", w, h, format),
            });
        }
        let t = self.t.as_ref()?;
        let inv_rot = camera.rotation.inverse();
        let mut u = [0.0f32; 40];
        u[..16].copy_from_slice(&inv_rot.to_cols_array());
        u[16..32].copy_from_slice(&prev.view_projection.to_cols_array());
        let moved = camera.eye - prev.eye;
        u[32..36].copy_from_slice(&[moved.x, moved.y, moved.z, camera.near * BAND]);
        u[36..40].copy_from_slice(&[camera.tan_x, camera.tan_y, VIEW_MODEL_Z, 0.0]);
        self.vel.record(gpu, enc, floatz, &t.vel, &u);

        let (tw, th) = tile_size(w, h);
        let c = scratch_code;
        c.clone_from(code);
        let mut run = |tech: &str, tex: &[(&str, &wgpu::TextureView)], c: &CodeValues, dst: &wgpu::TextureView, f| -> bool {
            match post.record(enc, tech, tex, c, dst, f) {
                Ok(()) => true,
                Err(e) => {
                    let msg = format!("{tech}: {e} (no motion blur)");
                    if !errors.contains(&msg) {
                        errors.push(msg);
                    }
                    false
                }
            }
        };
        c.set_vec("MOTION_BLUR_HQ_TILE_MAX_PARAMS", tile_params_x(w, tw));
        c.set_vec("RENDER_TARGET_SIZE", [tw as f32, h as f32, 1.0 / tw as f32, 1.0 / h as f32]);
        if !run(TECH_TILE, &[("SCENE_VELOCITY", &t.vel)], c, &t.tile_a, VEL_FORMAT) {
            return None;
        }
        c.set_vec("MOTION_BLUR_HQ_TILE_MAX_PARAMS", tile_params_y(h, th));
        c.set_vec("RENDER_TARGET_SIZE", [tw as f32, th as f32, 1.0 / tw as f32, 1.0 / th as f32]);
        if !run(TECH_TILE, &[("SCENE_VELOCITY", &t.tile_a)], c, &t.tile_b, VEL_FORMAT) {
            return None;
        }
        if !run(TECH_NEIGHBOUR, &[("SCENE_VELOCITY_PASS0", &t.tile_b)], c, &t.tile_c, VEL_FORMAT) {
            return None;
        }
        c.set_vec("MOTION_BLUR_HQ_PARAMS", apply_params());
        c.set_vec("RENDER_TARGET_SIZE", [w as f32, h as f32, 1.0 / w as f32, 1.0 / h as f32]);
        let tex = [("RESOLVED_SCENE", scene), ("SCENE_VELOCITY", &t.vel), ("SCENE_VELOCITY_PASS1", &t.tile_c), ("FLOATZ", floatz)];
        if !run(TECH_APPLY, &tex, c, &t.out, format) {
            return None;
        }
        Some(&t.out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mode_switch() {
        assert_eq!(parse_mode(None), Mode::Velocity);
        assert_eq!(parse_mode(Some("velocity")), Mode::Velocity);
        assert_eq!(parse_mode(Some(" OLD ")), Mode::Camera);
        assert_eq!(parse_mode(Some("camera")), Mode::Camera);
    }

    #[test]
    fn velocity_encoding_round_trips_with_the_shaders_decode() {
        assert_eq!(encode(0.0), 128);
        for v in [0.0f32, 0.001, 0.01, 0.05, 0.2, 0.6, -0.001, -0.05, -0.4, 1.0, -1.0] {
            let back = decode(encode(v));
            // 8 bits of the square root: fine near zero, coarser far out.
            let tol = 2.0 * v.abs().sqrt() / 255.0 + 1e-4;
            assert!((back - v).abs() <= tol * 2.0 + 0.002, "{v} -> {back}");
        }
        assert_eq!(encode(5.0), encode(1.0));
        assert!(decode(255) > 0.99 && decode(0) < -0.99);
    }

    #[test]
    fn tile_geometry() {
        assert_eq!(tile_size(1280, 720), (64, 36));
        assert_eq!(tile_size(1281, 721), (65, 37));
        assert_eq!(tile_size(1, 1), (1, 1));
        // The first tap of tile i is the centre of its first source texel.
        let (w, tw) = (1280u32, 64u32);
        let p = tile_params_x(w, tw);
        for i in [0u32, 5, 63] {
            let u = (i as f32 + 0.5) / tw as f32 + p[0];
            let first_texel_centre = (i * TILE) as f32 / w as f32 + 0.5 / w as f32;
            assert!((u - first_texel_centre).abs() < 1e-6);
        }
        assert_eq!((p[1], p[3]), (0.0, 0.0));
        let q = tile_params_y(720, 36);
        assert_eq!((q[0], q[2]), (0.0, 0.0));
        assert!((q[3] - 1.0 / 720.0).abs() < 1e-9);
        assert_eq!(apply_params(), [0.0, 15.0, 1.0, 0.0]);
    }

    fn cam_at(eye: Vec3, yaw: f32) -> (Mat4, Mat4, [f32; 2]) {
        // Eye-relative, x right, y up, z forward; infinite reversed depth.
        let rot = Mat4::from_rotation_y(-yaw);
        let (tx, ty) = (1.0f32, 0.5625);
        let near = 4.0;
        let proj = Mat4::from_cols_array(&[1.0 / tx, 0.0, 0.0, 0.0, 0.0, 1.0 / ty, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, near, 0.0]);
        let _ = eye;
        (rot, proj * rot, [tx, ty])
    }

    #[test]
    fn a_static_world_point_moves_by_the_cameras_motion() {
        let (rot0, vp0, tan) = cam_at(Vec3::ZERO, 0.0);
        let prev = Prev { view_projection: vp0, eye: Vec3::ZERO };
        // Not moved: zero velocity everywhere.
        let inv = rot0.inverse();
        let v = reproject([0.3, 0.7], Some(500.0), tan, &inv, Vec3::ZERO, &prev).unwrap();
        assert!(v[0].abs() < 1e-6 && v[1].abs() < 1e-6, "{v:?}");
        // Turned right by a small angle: the world slides left on screen
        // (uv_now - uv_prev < 0 at the centre ... sign by construction).
        let (rot1, _vp1, _) = cam_at(Vec3::ZERO, 0.02);
        let v = reproject([0.5, 0.5], Some(500.0), tan, &rot1.inverse(), Vec3::ZERO, &prev).unwrap();
        assert!(v[0].abs() > 1e-3 && v[1].abs() < 1e-6, "{v:?}");
        // A far point and a sky point (rotation only) agree with each other.
        let far = reproject([0.5, 0.5], Some(1.0e7), tan, &rot1.inverse(), Vec3::ZERO, &prev).unwrap();
        let sky = reproject([0.5, 0.5], None, tan, &rot1.inverse(), Vec3::ZERO, &prev).unwrap();
        assert!((far[0] - sky[0]).abs() < 1e-4);
        // Moving sideways: near points move more than far ones (parallax).
        let eye = Vec3::new(10.0, 0.0, 0.0);
        let near_v = reproject([0.5, 0.5], Some(100.0), tan, &rot0.inverse(), eye, &prev).unwrap();
        let far_v = reproject([0.5, 0.5], Some(1000.0), tan, &rot0.inverse(), eye, &prev).unwrap();
        assert!(near_v[0].abs() > 5.0 * far_v[0].abs(), "{near_v:?} {far_v:?}");
        assert!(still(&prev, &vp0, Vec3::ZERO) && !still(&prev, &vp0, eye));
    }
}
