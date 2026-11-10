//! Players and the view gun as sun shadow casters.
//!
//! The players and the gun are drawn by the effect/character pass of the
//! game (Black Ops II's skinned draws), which cannot write into Ghosts'
//! sun shadow atlas, so they cast into it as proxies: each is a capsule
//! (a segment `a`..`b` and a radius: a standing player, the gun's length)
//! drawn depth-only into both cascades after the world's. The proxy's
//! shadow is the silhouette of a capsule, not of the model: an
//! approximation (INFERRED), cheap (a few hundred triangles for all of
//! them, one draw per cascade) and the same under any preset that has a sun
//! shadow. The quality preset's cap on casters: `Quality::sun_shadow_res`
//! decides whether there is a map at all; `MAX` is the list's size.
//!
//! The world's cascades are cached across frames, so with casters in the
//! frame they are drawn into a second ("static") atlas, and each frame the
//! static atlas is copied over the live one the shaders sample, then the
//! capsules are drawn into the live one (`Host::record_scene`). Nothing is
//! allocated per frame: the capsule mesh, the instance buffer and the
//! constants are made once.
//!
//! `BO2MP_IW6_SUN_CASTERS=0` turns it off (no proxies, the cascades as
//! before).

use glam::Mat4;

/// `BO2MP_IW6_SUN_CASTERS=0`: no player or gun proxies in the sun shadow.
pub fn on() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| !std::env::var("BO2MP_IW6_SUN_CASTERS").is_ok_and(|v| v.trim() == "0"))
}

/// The most proxies in a frame.
pub const MAX: usize = 16;
/// Sides of the capsule's tube and rings of each cap.
const SIDES: usize = 10;
const CAP_RINGS: usize = 3;

/// One proxy: a capsule from `a` to `b` (world units, Ghosts' z up) with
/// `radius`.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Caster {
    pub a: [f32; 3],
    pub b: [f32; 3],
    pub radius: f32,
}

impl Caster {
    /// A standing player whose feet are at `feet`: a capsule of `height`
    /// (Ghosts' standing height is 70) and `radius`.
    pub fn standing(feet: [f32; 3], height: f32, radius: f32) -> Self {
        let r = radius.min(height * 0.5);
        Self { a: [feet[0], feet[1], feet[2] + r], b: [feet[0], feet[1], feet[2] + height - r], radius }
    }
}

/// The unit capsule: per vertex `(cos, sin, along, end)`: the ring's
/// direction scaled by the ring's radius (1 on the tube), the offset along
/// the axis in radii (0 on the tube, outward on a cap) and which end (0 `a`,
/// 1 `b`) it belongs to; and its triangle indices.
pub fn capsule_mesh() -> (Vec<[f32; 4]>, Vec<u16>) {
    let mut v: Vec<[f32; 4]> = Vec::new();
    // Rings from the `a` pole, over its cap's equator, to `b`'s equator and pole.
    let ring = |end: f32, scale: f32, along: f32, v: &mut Vec<[f32; 4]>| {
        for s in 0..SIDES {
            let t = std::f32::consts::TAU * s as f32 / SIDES as f32;
            v.push([t.cos() * scale, t.sin() * scale, along, end]);
        }
    };
    let mut rings: Vec<usize> = Vec::new();
    for end in [0.0f32, 1.0] {
        // (`a`'s cap goes from its pole out to its equator, `b`'s from its equator to its pole.)
        let order: Vec<usize> = if end == 0.0 { (0..=CAP_RINGS).rev().collect() } else { (0..=CAP_RINGS).collect() };
        for k in order {
            let phi = std::f32::consts::FRAC_PI_2 * k as f32 / CAP_RINGS as f32;
            // k = 0: the equator (full ring, no offset); k = CAP_RINGS: the pole.
            rings.push(v.len());
            ring(end, phi.cos(), phi.sin(), &mut v);
        }
    }
    let mut idx: Vec<u16> = Vec::new();
    for w in rings.windows(2) {
        let (r0, r1) = (w[0], w[1]);
        for s in 0..SIDES {
            let (a, b) = (r0 + s, r0 + (s + 1) % SIDES);
            let (c, d) = (r1 + s, r1 + (s + 1) % SIDES);
            idx.extend_from_slice(&[a as u16, c as u16, b as u16, b as u16, c as u16, d as u16]);
        }
    }
    (v, idx)
}

/// The capsule vertex shader's positions (the same maths as the WGSL, for the tests):
/// the world position of unit vertex `u` of the capsule `c`.
pub fn place(c: &Caster, u: [f32; 4]) -> [f32; 3] {
    let a = glam::Vec3::from(c.a);
    let b = glam::Vec3::from(c.b);
    let axis = b - a;
    let len = axis.length();
    let dir = if len > 1e-6 { axis / len } else { glam::Vec3::Z };
    let helper = if dir.z.abs() < 0.99 { glam::Vec3::Z } else { glam::Vec3::X };
    let side = helper.cross(dir).normalize();
    let up = dir.cross(side);
    // (`along` is outward from the end's own side.)
    let (end, sign) = if u[3] < 0.5 { (a, -1.0) } else { (b, 1.0) };
    (end + (side * u[0] + up * u[1]) * c.radius + dir * (sign * u[2] * c.radius)).to_array()
}

const SHADER: &str = "
struct Frame { vp: mat4x4<f32>, eye: vec4<f32> };
struct Caster { a: vec4<f32>, b: vec4<f32> };
@group(0) @binding(0) var<uniform> frame: Frame;
@group(0) @binding(1) var<storage, read> casters: array<Caster>;
@vertex fn vs(@location(0) u: vec4<f32>, @builtin(instance_index) i: u32) -> @builtin(position) vec4<f32> {
    let c = casters[i];
    let a = c.a.xyz; let b = c.b.xyz; let r = c.a.w;
    let axis = b - a;
    let len = length(axis);
    var dir = vec3<f32>(0.0, 0.0, 1.0);
    if (len > 1e-6) { dir = axis / len; }
    var helper = vec3<f32>(0.0, 0.0, 1.0);
    if (abs(dir.z) >= 0.99) { helper = vec3<f32>(1.0, 0.0, 0.0); }
    let side = normalize(cross(helper, dir));
    let up = cross(dir, side);
    var end = a; var sign = -1.0;
    if (u.w >= 0.5) { end = b; sign = 1.0; }
    let p = end + (side * u.x + up * u.y) * r + dir * (sign * u.z * r);
    return frame.vp * vec4<f32>(p - frame.eye.xyz, 1.0);
}
";

/// Per-draw constants slot (a dynamic offset of this many bytes).
const SLOT: u64 = 256;

/// The depth-only capsule pass.
pub struct Casters {
    pipeline: wgpu::RenderPipeline,
    group: wgpu::BindGroup,
    vertices: wgpu::Buffer,
    indices: wgpu::Buffer,
    index_count: u32,
    frame: wgpu::Buffer,
    instances: wgpu::Buffer,
}

impl Casters {
    pub fn new(device: &wgpu::Device) -> Self {
        let (verts, idx) = capsule_mesh();
        let vertices = wgpu::util::DeviceExt::create_buffer_init(
            device,
            &wgpu::util::BufferInitDescriptor { label: Some("iw6_caster_vb"), contents: bytemuck::cast_slice(&verts), usage: wgpu::BufferUsages::VERTEX },
        );
        let mut idx_bytes: Vec<u16> = idx.clone();
        if idx_bytes.len() % 2 == 1 {
            idx_bytes.push(0);
        }
        let indices = wgpu::util::DeviceExt::create_buffer_init(
            device,
            &wgpu::util::BufferInitDescriptor { label: Some("iw6_caster_ib"), contents: bytemuck::cast_slice(&idx_bytes), usage: wgpu::BufferUsages::INDEX },
        );
        // Two cascades: a constants slot each (a dynamic offset).
        let frame = device.create_buffer(&wgpu::BufferDescriptor { label: Some("iw6_caster_frame"), size: SLOT * 2, usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST, mapped_at_creation: false });
        let instances = device.create_buffer(&wgpu::BufferDescriptor { label: Some("iw6_caster_inst"), size: (MAX * 32) as u64, usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST, mapped_at_creation: false });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("iw6_caster"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX,
                    ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: true, min_binding_size: wgpu::BufferSize::new(80) },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::VERTEX,
                    ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Storage { read_only: true }, has_dynamic_offset: false, min_binding_size: None },
                    count: None,
                },
            ],
        });
        let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("iw6_caster"),
            layout: &layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding { buffer: &frame, offset: 0, size: wgpu::BufferSize::new(80) }) },
                wgpu::BindGroupEntry { binding: 1, resource: instances.as_entire_binding() },
            ],
        });
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some("iw6_caster"), source: wgpu::ShaderSource::Wgsl(SHADER.into()) });
        let pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: Some("iw6_caster"), bind_group_layouts: &[Some(&layout)], immediate_size: 0 });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("iw6_caster"),
            layout: Some(&pl),
            vertex: wgpu::VertexState {
                module: &module,
                entry_point: Some("vs"),
                compilation_options: Default::default(),
                buffers: &[wgpu::VertexBufferLayout {
                    array_stride: 16,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &[wgpu::VertexAttribute { format: wgpu::VertexFormat::Float32x4, offset: 0, shader_location: 0 }],
                }],
            },
            primitive: wgpu::PrimitiveState { cull_mode: None, ..Default::default() },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: crate::world::SHADOW_DEPTH,
                depth_write_enabled: Some(true),
                depth_compare: Some(wgpu::CompareFunction::LessEqual),
                stencil: Default::default(),
                bias: wgpu::DepthBiasState { constant: 2, slope_scale: 2.0, clamp: 0.0 },
            }),
            multisample: Default::default(),
            fragment: None,
            multiview_mask: None,
            cache: None,
        });
        Self { pipeline, group, vertices, indices, index_count: idx.len() as u32, frame, instances }
    }

    /// Uploads the frame's proxies (`casters`, at most `MAX`) and the
    /// cascades' constants (`views`: eye-relative world to clip, per
    /// cascade); a stack array of 32 bytes each, nothing allocated.
    pub fn upload(&self, queue: &wgpu::Queue, eye: [f32; 3], views: [Mat4; 2], casters: &[Caster]) {
        let mut inst = [[0.0f32; 8]; MAX];
        let n = casters.len().min(MAX);
        for (o, c) in inst.iter_mut().zip(casters) {
            *o = [c.a[0], c.a[1], c.a[2], c.radius, c.b[0], c.b[1], c.b[2], 0.0];
        }
        queue.write_buffer(&self.instances, 0, bytemuck::cast_slice(&inst[..n.max(1)]));
        for (k, vp) in views.iter().enumerate() {
            let mut block = [0.0f32; 20];
            block[..16].copy_from_slice(&vp.to_cols_array());
            block[16..19].copy_from_slice(&eye);
            queue.write_buffer(&self.frame, k as u64 * SLOT, bytemuck::cast_slice(&block));
        }
    }

    /// Draws the uploaded proxies into cascade `k`'s viewport `[x, y, size]`
    /// of the atlas `depth` (its other texels kept).
    pub fn record(&self, enc: &mut wgpu::CommandEncoder, depth: &wgpu::TextureView, k: usize, viewport: [f32; 3], count: usize) {
        if count == 0 {
            return;
        }
        let [x, y, size] = viewport;
        let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("iw6_sun_casters"),
            color_attachments: &[],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: depth,
                depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Load, store: wgpu::StoreOp::Store }),
                stencil_ops: None,
            }),
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        pass.set_viewport(x, y, size, size, 0.0, 1.0);
        pass.set_scissor_rect(x as u32, y as u32, size as u32, size as u32);
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &self.group, &[(k as u64 * SLOT) as u32]);
        pass.set_vertex_buffer(0, self.vertices.slice(..));
        pass.set_index_buffer(self.indices.slice(..), wgpu::IndexFormat::Uint16);
        pass.draw_indexed(0..self.index_count, 0, 0..count.min(MAX) as u32);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_capsule_mesh_is_closed_and_in_range() {
        let (v, i) = capsule_mesh();
        assert_eq!(v.len(), SIDES * 2 * (CAP_RINGS + 1));
        assert!(i.iter().all(|&k| usize::from(k) < v.len()));
        assert_eq!(i.len() % 3, 0);
        // Every edge is shared by two triangles (closed): count directed edges.
        let mut edges = std::collections::HashMap::new();
        for t in i.chunks_exact(3) {
            for e in 0..3 {
                *edges.entry((t[e], t[(e + 1) % 3])).or_insert(0) += 1;
            }
        }
        // (Pole rings are degenerate fans of repeated points: only edges between distinct positions are counted.)
        let pos = |k: u16| {
            let u = v[usize::from(k)];
            [(u[0] * 1e4).round() as i32, (u[1] * 1e4).round() as i32, (u[2] * 1e4).round() as i32, u[3] as i32]
        };
        for (&(a, b), &n) in &edges {
            if pos(a) == pos(b) {
                continue;
            }
            assert_eq!(n, 1, "edge {a}-{b}");
        }
    }

    #[test]
    fn a_standing_proxy_spans_its_height_and_radius() {
        let c = Caster::standing([100.0, 50.0, 0.0], 70.0, 15.0);
        let (v, _) = capsule_mesh();
        let (mut lo, mut hi) = ([f32::MAX; 3], [f32::MIN; 3]);
        for u in &v {
            let p = place(&c, *u);
            for k in 0..3 {
                lo[k] = lo[k].min(p[k]);
                hi[k] = hi[k].max(p[k]);
            }
        }
        assert!((lo[2] - 0.0).abs() < 0.01 && (hi[2] - 70.0).abs() < 0.01, "{lo:?} {hi:?}");
        assert!((hi[0] - lo[0] - 30.0).abs() < 2.0 && (hi[1] - lo[1] - 30.0).abs() < 2.0, "{lo:?} {hi:?}");
        assert!((lo[0] + hi[0]) * 0.5 - 100.0 < 0.01);
    }

    #[test]
    fn a_tilted_proxy_keeps_its_length() {
        // A gun: 30 long along x, radius 3.
        let c = Caster { a: [0.0, 0.0, 0.0], b: [30.0, 0.0, 0.0], radius: 3.0 };
        let (v, _) = capsule_mesh();
        let xs: Vec<f32> = v.iter().map(|u| place(&c, *u)[0]).collect();
        let (lo, hi) = (xs.iter().cloned().fold(f32::MAX, f32::min), xs.iter().cloned().fold(f32::MIN, f32::max));
        assert!((lo + 3.0).abs() < 0.01 && (hi - 33.0).abs() < 0.01, "{lo} {hi}");
    }
}
