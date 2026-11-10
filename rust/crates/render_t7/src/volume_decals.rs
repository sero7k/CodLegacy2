//! Volume decals (`GfxVolumeDecal`, 184 bytes, `GfxWorld` +0x5c8/+0x5d0)
//! drawn as BlackOps3.exe draws them on its instanced path (`141de6db0`,
//! taken when its dvar is on; the shipped `vd/*` technique sets read the
//! instance buffer it fills): per visible decal one 200-byte element of
//! `volDecalInstanceBuffer` (code buffer 10), the decals of one material in
//! one instanced draw of a box mesh, in the G-buffer's decal list (their
//! pixel programs read `resolvedNormal` and `floatZ`, code texture 0x10).
//!
//! The record (words): 0..12 the box's local-to-world 3x4 (three axis
//! columns, then the centre), 12..24 its inverse, 24..27 the half extents,
//! 27..30 the edge fade start, 30..38 two UV rectangles, the material at
//! byte 152, 0xa0 the sort index, 0xa4 the bounding radius, byte 0xb0 a
//! "hidden" flag.
//!
//! The element (floats), as the exe writes it: 0..16 the local-to-world
//! 4x4 (columns) with the axes scaled by the half extents (the box mesh is
//! the cube -1..1), 16..32 its inverse (`142388ca0`), 32..35 the fade start,
//! 35 0 (the pixel program takes the G-buffer normal, not the depth's
//! derivatives), 36..40 the tint (1, 1, 1, 1), 40..48 the two UV
//! rectangles, 48 1, 49 0.

#[allow(unused_imports)]
use crate::profile::{ComputeExt as _, EncExt as _, PassExt as _, QueueExt as _};
pub const RECORD: usize = 184;
pub const ELEMENT_FLOATS: usize = 50;
/// `volDecalInstanceBuffer`'s code buffer index (the `vd/*` techniques'
/// 0x600 argument).
pub const CODE_BUFFER: u32 = 10;
/// `floatZ`.
pub const CODE_TEXTURE_FLOATZ: u32 = 0x10;
/// The exe caps a frame's instanced decals at 0x800.
pub const MAX: usize = 0x800;

fn f(rec: &[u8], word: usize) -> f32 {
    f32::from_le_bytes(rec[4 * word..4 * word + 4].try_into().unwrap())
}

/// Whether the exe draws the decal (byte 0xb0 clear; the per-cell
/// visibility byte, `GfxWorld` +0x3b0, is taken as set).
pub fn visible(rec: &[u8]) -> bool {
    rec.len() >= RECORD && rec[0xb0] == 0
}

/// The decal's centre (words 9..12).
pub fn centre(rec: &[u8]) -> [f32; 3] {
    [f(rec, 9), f(rec, 10), f(rec, 11)]
}

/// The bounding radius (0xa4).
pub fn radius(rec: &[u8]) -> f32 {
    f(rec, 0x29)
}

/// One element of `volDecalInstanceBuffer` from a record (`141de6db0`).
pub fn pack(rec: &[u8]) -> [f32; ELEMENT_FLOATS] {
    let mut e = [0f32; ELEMENT_FLOATS];
    let ext = [f(rec, 0x18), f(rec, 0x19), f(rec, 0x1a)];
    for axis in 0..3 {
        for k in 0..3 {
            e[4 * axis + k] = ext[axis] * f(rec, 3 * axis + k);
        }
    }
    e[12] = f(rec, 9);
    e[13] = f(rec, 10);
    e[14] = f(rec, 11);
    e[15] = 1.0;
    let m = glam::Mat4::from_cols_array(&e[..16].try_into().unwrap());
    e[16..32].copy_from_slice(&m.inverse().to_cols_array());
    e[32] = f(rec, 0x1b);
    e[33] = f(rec, 0x1c);
    e[34] = f(rec, 0x1d);
    e[35] = 0.0;
    e[36..40].copy_from_slice(&[1.0; 4]);
    for k in 0..8 {
        e[40 + k] = f(rec, 0x1e + k);
    }
    e[48] = 1.0;
    e[49] = 0.0;
    e
}

/// The box mesh (the cube -1..1): eight corners and twelve triangles,
/// wound as the zones' models are (their face normal `cross(b - a, c - a)`
/// points into the mesh: `crate::winding`), so the models' pipeline state
/// (clockwise front faces) keeps the outside of the box.
pub fn box_mesh() -> ([[f32; 3]; 8], [u16; 36]) {
    let v = std::array::from_fn(|i| [if i & 1 != 0 { 1.0 } else { -1.0 }, if i & 2 != 0 { 1.0 } else { -1.0 }, if i & 4 != 0 { 1.0 } else { -1.0 }]);
    // Each face's corners counter-clockwise seen from outside, then each
    // quad split in two with the order reversed (inward cross products).
    let faces: [[u16; 4]; 6] = [[1, 3, 7, 5], [0, 4, 6, 2], [2, 6, 7, 3], [0, 1, 5, 4], [4, 5, 7, 6], [0, 2, 3, 1]];
    let mut idx = [0u16; 36];
    for (k, q) in faces.iter().enumerate() {
        let t = [q[0], q[2], q[1], q[0], q[3], q[2]];
        idx[6 * k..6 * k + 6].copy_from_slice(&t);
    }
    (v, idx)
}

/// `floatZ` (code texture 0x10) for the decal list: the G-buffer depth as
/// the exe's programs decode it (`d · 63 / (64·near)`, as
/// `forward::FloatZ` writes it for the forward lists), made after the
/// opaque list and before the decals.
pub struct FloatZPass {
    pipeline: wgpu::RenderPipeline,
    k: wgpu::Buffer,
    pub out: crate::gpu::Tex,
    group: wgpu::BindGroup,
}

const CONVERT: &str = r#"
@group(0) @binding(0) var depth: texture_depth_2d;
struct K { k: vec4<f32> }
@group(0) @binding(1) var<uniform> k: K;
@vertex fn vs(@builtin(vertex_index) i: u32) -> @builtin(position) vec4<f32> {
    let p = vec2<f32>(f32((i << 1u) & 2u), f32(i & 2u));
    return vec4<f32>(p * 2.0 - 1.0, 0.0, 1.0);
}
@fragment fn fs(@builtin(position) p: vec4<f32>) -> @location(0) vec4<f32> {
    let d = textureLoad(depth, vec2<i32>(p.xy), 0);
    return vec4<f32>(d * k.k.x, 0.0, 0.0, 0.0);
}
"#;

impl FloatZPass {
    /// For a `w` x `h` frame reading `depth` (its depth aspect).
    pub fn new(gpu: &render_h1::gpu::Gpu, w: u32, h: u32, depth: &wgpu::TextureView) -> Self {
        let dev = &gpu.device;
        let m = dev.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some("t7 decal floatZ"), source: wgpu::ShaderSource::Wgsl(CONVERT.into()) });
        let pipeline = dev.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("t7 decal floatZ"),
            layout: None,
            vertex: wgpu::VertexState { module: &m, entry_point: Some("vs"), compilation_options: Default::default(), buffers: &[] },
            primitive: Default::default(),
            depth_stencil: None,
            multisample: Default::default(),
            fragment: Some(wgpu::FragmentState { module: &m, entry_point: Some("fs"), compilation_options: Default::default(), targets: &[Some(wgpu::TextureFormat::R32Float.into())] }),
            multiview_mask: None,
            cache: crate::pipecache::get(&dev).as_ref(),
        });
        let k = crate::audit::buffer(dev, &wgpu::BufferDescriptor { label: Some("t7 decal floatZ scale"), size: 16, usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST, mapped_at_creation: false });
        let out = crate::gpu::target(gpu, "t7 decal floatZ (code texture 0x10)", w, h, wgpu::TextureFormat::R32Float, wgpu::TextureUsages::empty());
        let group = dev.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("t7 decal floatZ"),
            layout: &pipeline.get_bind_group_layout(0),
            entries: &[wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(depth) }, wgpu::BindGroupEntry { binding: 1, resource: k.as_entire_binding() }],
        });
        Self { pipeline, k, out, group }
    }

    /// The view's near plane (the encoding's scale).
    pub fn set_near(&self, queue: &wgpu::Queue, near: f32) {
        queue.write_buffer_p(&self.k, 0, bytemuck::cast_slice(&[63.0 / (64.0 * near.max(1e-4)), 0.0, 0.0, 0.0f32]));
    }

    pub fn record(&self, enc: &mut wgpu::CommandEncoder) {
        let mut p = enc.render_pass_p(&wgpu::RenderPassDescriptor {
            label: Some("t7 decal floatZ"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment { view: &self.out.view, resolve_target: None, depth_slice: None, ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT), store: wgpu::StoreOp::Store } })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        p.set_pipeline_p(&self.pipeline);
        p.set_bind_group_p(0, &self.group, &[]);
        p.draw_p(0..3, 0..1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `mp_nuketown_x`'s first volume decal (`t7_voldecals`).
    fn nuketown0() -> Vec<u8> {
        let words: [f32; 37] = [
            -0.312_351_47, -0.949_966_25, 0.0, 0.949_966_25, -0.312_351_47, 0.0, 0.0, 0.0, 1.0, -1172.58, 52.6551, -53.0, -0.312_351_47, 0.949_966_25, 0.0, -0.949_966_25, -0.312_351_47, 0.0, 0.0, 0.0, 1.0, -316.2365, 1130.3583, 53.0, 4.0, 4.0, 4.0,
            1.0, 1.0, 1.0, 0.0, 0.0, 1.0, 1.0, 0.0, 0.0, 1.0,
        ];
        let mut r = vec![0u8; RECORD];
        for (i, w) in words.iter().enumerate() {
            r[4 * i..4 * i + 4].copy_from_slice(&w.to_le_bytes());
        }
        r[0x94..0x98].copy_from_slice(&1f32.to_le_bytes());
        r[0xa4..0xa8].copy_from_slice(&6.928_203f32.to_le_bytes());
        r
    }

    #[test]
    fn element_maps_the_cube_to_the_decal_box() {
        let r = nuketown0();
        assert!(visible(&r));
        let e = pack(&r);
        let m = glam::Mat4::from_cols_array(&e[..16].try_into().unwrap());
        let inv = glam::Mat4::from_cols_array(&e[16..32].try_into().unwrap());
        // The centre, a corner 4 units along each axis, the inverse.
        assert!((m.transform_point3(glam::Vec3::ZERO) - glam::Vec3::new(-1172.58, 52.6551, -53.0)).length() < 1e-3);
        let corner = m.transform_point3(glam::Vec3::ONE);
        assert!(((corner - glam::Vec3::new(-1172.58, 52.6551, -53.0)).length() - radius(&r)).abs() < 1e-3);
        assert!((inv * m).abs_diff_eq(glam::Mat4::IDENTITY, 1e-4));
        // The record's own inverse (words 12..24) agrees, unscaled.
        let world = glam::Vec3::new(-1170.0, 50.0, -52.0);
        let local = inv.transform_point3(world) * 4.0;
        let rec_inv = glam::Vec3::new(
            -0.312_351_47 * world.x - 0.949_966_25 * world.y - 316.2365,
            0.949_966_25 * world.x - 0.312_351_47 * world.y + 1130.3583,
            world.z + 53.0,
        );
        assert!((local - rec_inv).length() < 1e-2, "{local} vs {rec_inv}");
        assert_eq!(&e[32..40], &[1.0, 1.0, 1.0, 0.0, 1.0, 1.0, 1.0, 1.0]);
        assert_eq!(&e[40..50], &[0.0, 0.0, 1.0, 1.0, 0.0, 0.0, 1.0, 1.0, 1.0, 0.0]);
    }

    #[test]
    fn box_winds_as_the_models() {
        let (v, idx) = box_mesh();
        for t in idx.chunks(3) {
            let [a, b, c] = [t[0], t[1], t[2]].map(|i| glam::Vec3::from(v[i as usize]));
            let n = (b - a).cross(c - a);
            let centre = (a + b + c) / 3.0;
            // Inward: against the direction from the cube's centre.
            assert!(n.dot(centre) < 0.0, "{t:?}");
            assert!(n.length() > 0.0);
        }
    }
}
