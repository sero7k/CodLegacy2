//! Effect lights (the omni and spot light elements of Ghosts' effects,
//! types 12 and 13) lighting the world: one full-screen pass over the HDR
//! scene, after the opaque world and models and before the fog.
//!
//! Ghosts bakes each world surface's one local light into the surface's
//! own technique, so a light that appears at run time (a fire, a muzzle
//! flash, a flickering lamp) has no pass of its own there. This pass reads
//! the depth the frame already holds (`FLOATZ`), rebuilds each pixel's
//! position and (from its neighbours) its normal, and for each of the
//! nearest few lights adds `colour x falloff x N.L x spot` times the
//! pixel's own colour: the scene is multiplied by `1 + light`, so a surface
//! keeps its colour and a black one stays black. INFERRED: the multiply
//! stands in for the surface's albedo, which the forward pass does not
//! keep; the falloff `(1 - d / r)^2` and the gain are Ghosts' light
//! shaders' shape, not decoded for effect lights. Needs the owner's GPU.
//!
//! The pass costs nothing when there are no lights (it is not recorded).
//! The graphics preset caps how many (`native_quality::Quality::fx_lights`),
//! `BO2MP_IW6_FX_LIGHTS=0` turns it off, `BO2MP_IW6_FX_LIGHT_GAIN=<n>`
//! scales it (default 0.25).

use glam::{Mat4, Vec3};
use render_h1::gpu::Gpu;

/// The most lights one frame holds.
pub const MAX: usize = 16;

/// One effect light in world space.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Light {
    pub origin: [f32; 3],
    pub radius: f32,
    /// Linear colour in the map's light units.
    pub color: [f32; 3],
    /// The direction a spot shines.
    pub dir: [f32; 3],
    /// 1 for an omni light; a spot's cosine of its half angle.
    pub cos_cone: f32,
}

/// `BO2MP_IW6_FX_LIGHTS` (on; read once).
pub fn on() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| !std::env::var("BO2MP_IW6_FX_LIGHTS").is_ok_and(|v| v.trim() == "0"))
}

/// `BO2MP_IW6_FX_LIGHT_GAIN` (0.25; read once).
pub fn gain() -> f32 {
    static G: std::sync::OnceLock<f32> = std::sync::OnceLock::new();
    *G.get_or_init(|| {
        std::env::var("BO2MP_IW6_FX_LIGHT_GAIN")
            .ok()
            .and_then(|v| v.trim().parse::<f32>().ok())
            .filter(|g| g.is_finite() && *g >= 0.0)
            .unwrap_or(0.25)
    })
}

/// The pass's uniform block, in WGSL's layout (every member a vec4).
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct Uniforms {
    /// View to eye-relative world, as the three columns (the rows of the
    /// world-to-view rotation).
    pub rot: [[f32; 4]; 3],
    /// tan(half fov x), tan(half fov y), near, the light count.
    pub cam: [f32; 4],
    /// The depth band the scene was drawn in (reversed): low, 1 / (high -
    /// low), high, the gain.
    pub band: [f32; 4],
    /// Per light, three vec4: (eye-relative position, 1 / radius),
    /// (colour, cos cone), (direction, 0).
    pub lights: [[f32; 12]; MAX],
}

impl Uniforms {
    /// The block for `lights` seen from `camera` (the lights past `MAX`
    /// or with no radius dropped). `band`: the scene's depth band.
    pub fn build(camera: &render_h1::code::Camera, band: Option<(f32, f32)>, lights: &[Light], gain: f32) -> Self {
        let rot: &Mat4 = &camera.rotation;
        let row = |k: usize| [rot.x_axis[k], rot.y_axis[k], rot.z_axis[k], 0.0];
        let (lo, hi) = band.unwrap_or((0.0, 1.0));
        let mut u = Self {
            rot: [row(0), row(1), row(2)],
            cam: [camera.tan_x, camera.tan_y, camera.near, 0.0],
            band: [lo, 1.0 / (hi - lo).max(1e-6), hi, gain],
            lights: [[0.0; 12]; MAX],
        };
        let mut n = 0;
        for l in lights.iter().filter(|l| l.radius > 0.0 && l.radius.is_finite()) {
            if n == MAX {
                break;
            }
            let o = Vec3::from(l.origin) - camera.eye;
            u.lights[n] = [
                o.x, o.y, o.z, 1.0 / l.radius,
                l.color[0], l.color[1], l.color[2], l.cos_cone,
                l.dir[0], l.dir[1], l.dir[2], 0.0,
            ];
            n += 1;
        }
        u.cam[3] = n as f32;
        u
    }

    pub fn count(&self) -> usize {
        self.cam[3] as usize
    }

    /// The pixels (x, y, width, height, of a target `size`) the lights can
    /// reach: the union of their spheres' screen boxes, a pixel wider on
    /// every side. A pixel outside every sphere's projection sees no point
    /// inside any sphere, and the pass adds nothing there (the falloff is
    /// zero at the radius), so the pass need only draw this rectangle.
    /// None: no light reaches the view at all (the pass can be left out).
    pub fn reach(&self, size: (u32, u32)) -> Option<[u32; 4]> {
        let (w, h) = (size.0 as f32, size.1 as f32);
        let (tan_x, tan_y, near) = (self.cam[0].max(1e-6), self.cam[1].max(1e-6), self.cam[2]);
        let dot = |r: &[f32; 4], o: [f32; 3]| r[0] * o[0] + r[1] * o[1] + r[2] * o[2];
        let (mut x0, mut y0, mut x1, mut y1) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
        for l in &self.lights[..self.count()] {
            let (o, r) = ([l[0], l[1], l[2]], 1.0 / l[3].max(1e-9));
            let c = [dot(&self.rot[0], o), dot(&self.rot[1], o), dot(&self.rot[2], o)];
            if c[2] + r <= 0.0 {
                // Wholly behind the eye.
                continue;
            }
            if c[2] - r <= near.max(1e-3) {
                // The eye or the near plane is inside the sphere: all of it.
                return Some([0, 0, size.0, size.1]);
            }
            for k in 0..8 {
                let p = [
                    c[0] + if k & 1 == 0 { -r } else { r },
                    c[1] + if k & 2 == 0 { -r } else { r },
                    c[2] + if k & 4 == 0 { -r } else { r },
                ];
                let (nx, ny) = (p[0] / (p[2] * tan_x), p[1] / (p[2] * tan_y));
                let (px, py) = ((nx * 0.5 + 0.5) * w, (0.5 - ny * 0.5) * h);
                x0 = x0.min(px);
                x1 = x1.max(px);
                y0 = y0.min(py);
                y1 = y1.max(py);
            }
        }
        if x0 > x1 {
            return None;
        }
        let (x0, y0) = ((x0.floor() - 1.0).max(0.0), (y0.floor() - 1.0).max(0.0));
        let (x1, y1) = ((x1.ceil() + 1.0).min(w), (y1.ceil() + 1.0).min(h));
        if x1 <= x0 || y1 <= y0 {
            return None;
        }
        Some([x0 as u32, y0 as u32, (x1 - x0) as u32, (y1 - y0) as u32])
    }
}

/// `BO2MP_IW6_FX_LIGHT_SCISSOR=0`: the pass over the whole picture (as before).
pub fn scissor_on() -> bool {
    static S: crate::switch::Switch = crate::switch::Switch::on("BO2MP_IW6_FX_LIGHT_SCISSOR");
    S.get()
}

const WGSL: &str = r"
struct U {
    rot: array<vec4<f32>, 3>,
    cam: vec4<f32>,
    band: vec4<f32>,
    lights: array<vec4<f32>, 48>,
};
@group(0) @binding(0) var zt: texture_2d<f32>;
@group(0) @binding(1) var<uniform> u: U;

@vertex fn vs(@builtin(vertex_index) i: u32) -> @builtin(position) vec4<f32> {
    let p = vec2<f32>(f32(i == 1u) * 4.0 - 1.0, f32(i == 2u) * 4.0 - 1.0);
    return vec4<f32>(p, 0.0, 1.0);
}

// The eye-relative position of a texel, w 0 where there is no scene
// (sky, or the view model's band).
fn pos_at(p: vec2<i32>, size: vec2<f32>) -> vec4<f32> {
    let q = clamp(p, vec2<i32>(0), vec2<i32>(size) - vec2<i32>(1));
    let d = textureLoad(zt, q, 0).r;
    let z_ndc = (d - u.band.x) * u.band.y;
    if (!(z_ndc > 1.0e-7) || d > u.band.z + 1.0e-6) {
        return vec4<f32>(0.0);
    }
    let z = u.cam.z / z_ndc;
    let ndc = vec2<f32>((f32(q.x) + 0.5) / size.x * 2.0 - 1.0, 1.0 - (f32(q.y) + 0.5) / size.y * 2.0);
    let v = vec3<f32>(ndc.x * u.cam.x * z, ndc.y * u.cam.y * z, z);
    return vec4<f32>(u.rot[0].xyz * v.x + u.rot[1].xyz * v.y + u.rot[2].xyz * v.z, 1.0);
}

@fragment fn fs(@builtin(position) fp: vec4<f32>) -> @location(0) vec4<f32> {
    let size = vec2<f32>(textureDimensions(zt));
    let p = vec2<i32>(fp.xy);
    let c = pos_at(p, size);
    if (c.w == 0.0) {
        return vec4<f32>(0.0);
    }
    // The normal from the neighbour on each axis that is nearer in depth.
    let r = pos_at(p + vec2<i32>(1, 0), size);
    let l = pos_at(p - vec2<i32>(1, 0), size);
    let d = pos_at(p + vec2<i32>(0, 1), size);
    let t = pos_at(p - vec2<i32>(0, 1), size);
    var dx = vec3<f32>(0.0);
    if (r.w > 0.0 && (l.w == 0.0 || abs(r.z - c.z) < abs(l.z - c.z))) { dx = r.xyz - c.xyz; } else if (l.w > 0.0) { dx = c.xyz - l.xyz; }
    var dy = vec3<f32>(0.0);
    if (d.w > 0.0 && (t.w == 0.0 || abs(d.z - c.z) < abs(t.z - c.z))) { dy = d.xyz - c.xyz; } else if (t.w > 0.0) { dy = c.xyz - t.xyz; }
    var n = cross(dx, dy);
    let nl = length(n);
    // (No usable neighbours: lit as if facing the light.)
    var have_n = nl > 1.0e-9;
    if (have_n) {
        n = n / nl;
        if (dot(n, c.xyz) > 0.0) { n = -n; }
    }
    var acc = vec3<f32>(0.0);
    let count = u32(u.cam.w);
    for (var i = 0u; i < count; i = i + 1u) {
        let pr = u.lights[i * 3u];
        let col = u.lights[i * 3u + 1u];
        let dir = u.lights[i * 3u + 2u];
        let to = pr.xyz - c.xyz;
        let dist = length(to);
        let tt = clamp(dist * pr.w, 0.0, 1.0);
        let fall = (1.0 - tt) * (1.0 - tt);
        if (fall <= 0.0) { continue; }
        let ldir = to / max(dist, 1.0e-4);
        var ndl = 1.0;
        if (have_n) { ndl = max(dot(n, ldir), 0.0); }
        var cone = 1.0;
        if (col.w < 0.9999) {
            let cs = dot(-ldir, dir.xyz);
            cone = smoothstep(col.w, mix(col.w, 1.0, 0.5), cs);
        }
        acc = acc + col.xyz * (fall * ndl * cone);
    }
    // Multiplied into the scene by the blend (scene x (1 + this)).
    return vec4<f32>(acc * u.band.w, 0.0);
}
";

/// The pass: a pipeline, the uniform block and the bind group over the
/// frame's `FLOATZ` (made again when the targets are).
pub struct Pass {
    pipeline: wgpu::RenderPipeline,
    buffer: wgpu::Buffer,
    group: Option<wgpu::BindGroup>,
    /// Frames the pass was recorded (the report).
    pub recorded: u64,
}

impl Pass {
    pub fn new(gpu: &Gpu, format: wgpu::TextureFormat) -> Self {
        let dev = &gpu.device;
        let module = dev.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("iw6_fx_light"),
            source: wgpu::ShaderSource::Wgsl(WGSL.into()),
        });
        // scene' = scene x light + scene: source factor Dst, destination One.
        let blend = wgpu::BlendState {
            color: wgpu::BlendComponent {
                src_factor: wgpu::BlendFactor::Dst,
                dst_factor: wgpu::BlendFactor::One,
                operation: wgpu::BlendOperation::Add,
            },
            // (The alpha is the bloom mask: left alone.)
            alpha: wgpu::BlendComponent {
                src_factor: wgpu::BlendFactor::Zero,
                dst_factor: wgpu::BlendFactor::One,
                operation: wgpu::BlendOperation::Add,
            },
        };
        let pipeline = dev.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("iw6_fx_light"),
            layout: None,
            vertex: wgpu::VertexState { module: &module, entry_point: Some("vs"), compilation_options: Default::default(), buffers: &[] },
            primitive: Default::default(),
            depth_stencil: None,
            multisample: Default::default(),
            fragment: Some(wgpu::FragmentState {
                module: &module,
                entry_point: Some("fs"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState { format, blend: Some(blend), write_mask: wgpu::ColorWrites::ALL })],
            }),
            multiview_mask: None,
            cache: None,
        });
        let buffer = dev.create_buffer(&wgpu::BufferDescriptor {
            label: Some("iw6_fx_light"),
            size: std::mem::size_of::<Uniforms>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        Self { pipeline, buffer, group: None, recorded: 0 }
    }

    /// The targets were made again: the bind group is.
    pub fn invalidate(&mut self) {
        self.group = None;
    }

    /// Adds the lights to `scene` (loaded, not cleared) from the depth
    /// copy `floatz`. Nothing is recorded for an empty block.
    pub fn record(&mut self, gpu: &Gpu, enc: &mut wgpu::CommandEncoder, floatz: &wgpu::TextureView, scene: &wgpu::TextureView, u: &Uniforms, size: (u32, u32)) {
        if u.count() == 0 {
            return;
        }
        // Only the rectangle the lights reach (`Uniforms::reach`).
        let reach = if scissor_on() { u.reach(size) } else { Some([0, 0, size.0, size.1]) };
        let Some(rect) = reach else { return };
        gpu.queue.write_buffer(&self.buffer, 0, bytemuck::bytes_of(u));
        if self.group.is_none() {
            self.group = Some(gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("iw6_fx_light"),
                layout: &self.pipeline.get_bind_group_layout(0),
                entries: &[
                    wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(floatz) },
                    wgpu::BindGroupEntry { binding: 1, resource: self.buffer.as_entire_binding() },
                ],
            }));
        }
        let Some(group) = &self.group else { return };
        let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("iw6_fx_light"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: scene,
                resolve_target: None,
                ops: wgpu::Operations { load: wgpu::LoadOp::Load, store: wgpu::StoreOp::Store },
                depth_slice: None,
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, group, &[]);
        if rect != [0, 0, size.0, size.1] {
            pass.set_scissor_rect(rect[0], rect[1], rect[2], rect[3]);
        }
        pass.draw(0..3, 0..1);
        self.recorded += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use glam::{Mat4, Vec3};

    fn camera(eye: Vec3) -> render_h1::code::Camera {
        render_h1::code::Camera {
            eye,
            rotation: Mat4::IDENTITY,
            view_projection: Mat4::IDENTITY,
            tan_x: 1.0,
            tan_y: 0.5,
            near: 4.0,
        }
    }

    #[test]
    fn the_block_is_eye_relative_and_counted() {
        let ls = [
            Light { origin: [110.0, 20.0, 30.0], radius: 200.0, color: [1.0, 2.0, 3.0], dir: [0.0, 0.0, -1.0], cos_cone: 0.8 },
            Light { radius: 0.0, ..Light::default() },
        ];
        let u = Uniforms::build(&camera(Vec3::new(10.0, 20.0, 30.0)), Some((0.2, 0.7)), &ls, 0.5);
        assert_eq!(u.count(), 1);
        assert_eq!(&u.lights[0][..4], &[100.0, 0.0, 0.0, 1.0 / 200.0]);
        assert_eq!(&u.lights[0][4..8], &[1.0, 2.0, 3.0, 0.8]);
        assert_eq!(u.band[0], 0.2);
        assert!((u.band[1] - 2.0).abs() < 1e-5);
        assert_eq!(u.band[3], 0.5);
        assert_eq!(&u.cam[..3], &[1.0, 0.5, 4.0]);
    }

    #[test]
    fn more_than_the_block_holds_are_cut() {
        let ls = vec![Light { radius: 10.0, color: [1.0; 3], cos_cone: 1.0, ..Light::default() }; MAX + 5];
        assert_eq!(Uniforms::build(&camera(Vec3::ZERO), None, &ls, 1.0).count(), MAX);
    }

    #[test]
    fn the_block_is_the_size_the_shader_declares() {
        // rot 3 + cam + band + 16 x 3 vec4s, 16 bytes each.
        assert_eq!(std::mem::size_of::<Uniforms>(), 16 * (3 + 1 + 1 + 3 * MAX));
    }

    #[test]
    fn the_scissor_covers_a_sphere_ahead_and_drops_one_behind() {
        // Identity rotation, looking down +z (the shader's view space); tan 1 x 0.5.
        let cam = camera(Vec3::ZERO);
        let ahead = Light { origin: [0.0, 0.0, 100.0], radius: 10.0, color: [1.0; 3], cos_cone: 1.0, ..Light::default() };
        let u = Uniforms::build(&cam, None, &[ahead], 1.0);
        let [x, y, w, h] = u.reach((200, 100)).expect("ahead");
        // Centred, about 10% of the width (radius 10 at depth 100 over tan 1) and 20% of the height.
        assert!(x < 100 && x + w > 100 && y < 50 && y + h > 50, "{x} {y} {w} {h}");
        assert!(w < 40 && h < 50, "{w} {h}");
        let behind = Light { origin: [0.0, 0.0, -100.0], ..ahead };
        assert_eq!(Uniforms::build(&cam, None, &[behind], 1.0).reach((200, 100)), None);
        let inside = Light { origin: [0.0, 0.0, 3.0], ..ahead };
        assert_eq!(Uniforms::build(&cam, None, &[inside], 1.0).reach((200, 100)), Some([0, 0, 200, 100]));
        // A light far off to the side is off the picture.
        let aside = Light { origin: [5000.0, 0.0, 100.0], ..ahead };
        assert_eq!(Uniforms::build(&cam, None, &[aside], 1.0).reach((200, 100)), None);
    }

    #[test]
    fn the_shader_parses() {
        let m = naga::front::wgsl::parse_str(WGSL);
        assert!(m.is_ok(), "{:?}", m.err());
    }

    #[test]
    fn the_rotation_rows_take_a_view_vector_to_the_world() {
        // A camera turned 90 degrees about z: the world's x is the view's y.
        let rot = Mat4::from_rotation_z(std::f32::consts::FRAC_PI_2);
        let mut c = camera(Vec3::ZERO);
        c.rotation = rot;
        let u = Uniforms::build(&c, None, &[], 1.0);
        let v = Vec3::new(0.0, 0.0, 1.0);
        let w = Vec3::from_slice(&u.rot[0][..3]) * v.x + Vec3::from_slice(&u.rot[1][..3]) * v.y + Vec3::from_slice(&u.rot[2][..3]) * v.z;
        // (rotation^-1 * v)
        let want = rot.inverse().transform_vector3(v);
        assert!((w - want).length() < 1e-5);
    }
}
