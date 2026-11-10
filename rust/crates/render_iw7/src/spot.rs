//! Spot light shadows, as the lit pixel programs read them: a light whose
//! record (t24) names a shadow index `i` (word 7's low byte, 255 none)
//! takes t25 record `i` (96 bytes) and layer `i` of code texture 15 (a 2D
//! array, sampled linearly):
//! - bytes 32 to 80: four rows taking the pixel's position about the eye
//!   (w 1) to `p`; the map is sampled at `p.xy / p.w`;
//! - bytes 16 to 32: `(k, a, b, s)`; the light is let through by
//!   `saturate(exp2(1.4427 k (p.z s - stored)) a + b)`, an exponential
//!   test of the pixel's depth against the stored one.
//!
//! Here the layers hold each light's linear depth over its reach (drawn
//! from the shared depth pass's occluders, then resolved), `p.z` is the
//! linear depth, `s` one over the reach, and `a = -1`, `b = 1 + e^(k
//! bias)`: lit until the pixel lies more than the bias behind the stored
//! depth, dark a few units later. The static world is drawn once.
//! The spots taken are those the map lets cast shadows (the light's
//! `canUseShadowMap`), the [`MAX_SPOTS`] widest-reaching.
//! `BO2MP_IW7_SPOT_SHADOWS=0`: none.

use crate::lights::LocalLight;
use crate::screen::{Screen, cam_bytes, depth_tex};
use glam::{Mat4, Vec3, Vec4};
use render_h1::gpu::Gpu;

/// Each layer's size in texels.
pub const SIZE: u32 = 512;
/// The most shadowed spots.
pub const MAX_SPOTS: usize = 32;
/// The near plane (world units).
pub const NEAR: f32 = 2.0;
/// The exponential test's sharpness per world unit.
pub const SHARPNESS: f32 = 0.25;
pub const RECORD_WORDS: usize = 24;

/// One shadowed spot's view.
#[derive(Clone, Copy, Debug)]
pub struct SpotView {
    pub pos: Vec3,
    pub dir: Vec3,
    pub up: Vec3,
    pub fov: f32,
    pub reach: f32,
}

impl SpotView {
    pub fn of(l: &LocalLight) -> Self {
        let dir = -l.toward.normalize_or(Vec3::NEG_Z);
        let up = if dir.z.abs() > 0.99 { Vec3::X } else { Vec3::Z };
        let half = l.cos_outer.clamp(-1.0, 1.0).acos();
        Self { pos: l.pos, dir, up, fov: (half * 2.2).clamp(10f32.to_radians(), 160f32.to_radians()), reach: l.reach.max(1.0) }
    }

    /// Projection times view about the light (reversed, infinite: depth
    /// `NEAR / distance`).
    pub fn view_projection(&self) -> Mat4 {
        Mat4::perspective_infinite_reverse_rh(self.fov, 1.0, NEAR) * Mat4::look_to_rh(Vec3::ZERO, self.dir, self.up)
    }

    /// The bias in world units: two texels' width at half the reach.
    pub fn bias(&self) -> f32 {
        2.0 + 2.0 * (0.5 * self.reach) * (self.fov * 0.5).tan() * 2.0 / SIZE as f32
    }

    /// The t25 record with the eye at `eye`.
    pub fn record(&self, eye: Vec3) -> [u32; RECORD_WORDS] {
        let to_uv = Mat4::from_cols(Vec4::new(0.5, 0.0, 0.0, 0.0), Vec4::new(0.0, -0.5, 0.0, 0.0), Vec4::ZERO, Vec4::new(0.5, 0.5, 1.0, 1.0));
        // (`to_uv` keeps x and y as uv times w and puts w, the linear
        // depth, in z and w.)
        let m = to_uv * self.view_projection() * Mat4::from_translation(eye - self.pos);
        let mut r = [0u32; RECORD_WORDS];
        let k = SHARPNESS * self.reach;
        let b = 1.0 + (SHARPNESS * self.bias()).exp();
        for (i, v) in [k, -1.0, b, 1.0 / self.reach].into_iter().enumerate() {
            r[4 + i] = v.to_bits();
        }
        for row in 0..4 {
            for (c, v) in m.row(row).to_array().into_iter().enumerate() {
                r[8 + row * 4 + c] = v.to_bits();
            }
        }
        r
    }
}

/// Which lights cast shadows: sets their shadow index and returns their views.
pub fn choose(lights: &mut [LocalLight]) -> Vec<SpotView> {
    if std::env::var("BO2MP_IW7_SPOT_SHADOWS").is_ok_and(|v| v.trim() == "0") {
        return Vec::new();
    }
    let mut order: Vec<usize> = (0..lights.len()).filter(|&i| lights[i].spot && lights[i].casts).collect();
    order.sort_by(|&a, &b| lights[b].reach.total_cmp(&lights[a].reach));
    order.truncate(MAX_SPOTS);
    order
        .into_iter()
        .enumerate()
        .map(|(n, i)| {
            lights[i].shadow = Some(n as u8);
            SpotView::of(&lights[i])
        })
        .collect()
}

const RESOLVE: &str = r"
struct Vo { @builtin(position) p: vec4<f32> };
@vertex fn vs(@builtin(vertex_index) i: u32) -> Vo {
    let x = f32((i << 1u) & 2u);
    let y = f32(i & 2u);
    return Vo(vec4<f32>(x * 2.0 - 1.0, 1.0 - y * 2.0, 0.0, 1.0));
}
struct R { near: f32, reach: f32, pad0: f32, pad1: f32 };
@group(0) @binding(0) var<uniform> r: R;
@group(0) @binding(1) var depth: texture_depth_2d;
@fragment fn fs(v: Vo) -> @location(0) vec4<f32> {
    let d = textureLoad(depth, vec2<i32>(v.p.xy), 0);
    var lin = 1.0;
    if (d > 0.0) { lin = min(r.near / d / r.reach, 1.0); }
    return vec4<f32>(lin, lin, lin, 1.0);
}
";

struct Layer {
    cam: wgpu::BindGroup,
    resolve: wgpu::BindGroup,
    view: wgpu::TextureView,
}

/// The shadowed spots' maps.
pub struct SpotShadows {
    pub views: Vec<SpotView>,
    /// Code texture 15 (one white layer when there are none).
    pub array: (wgpu::Texture, wgpu::TextureView),
    depth: (wgpu::Texture, wgpu::TextureView),
    layers: Vec<Layer>,
    resolve: wgpu::RenderPipeline,
    drawn: bool,
    records: Vec<u32>,
}

impl SpotShadows {
    pub fn new(g: &Gpu, screen: &Screen, views: Vec<SpotView>) -> Self {
        let n = views.len().max(1) as u32;
        let tex = g.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("iw7 spot shadows"),
            size: wgpu::Extent3d { width: SIZE, height: SIZE, depth_or_array_layers: n },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::R16Float,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let array_view = tex.create_view(&wgpu::TextureViewDescriptor { dimension: Some(wgpu::TextureViewDimension::D2Array), ..Default::default() });
        let depth = depth_tex(g, SIZE, SIZE, "iw7 spot depth");
        let module = g.device.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some("iw7 spot resolve"), source: wgpu::ShaderSource::Wgsl(RESOLVE.into()) });
        let resolve = g.device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("iw7 spot resolve"),
            layout: None,
            vertex: wgpu::VertexState { module: &module, entry_point: Some("vs"), compilation_options: Default::default(), buffers: &[] },
            primitive: Default::default(),
            depth_stencil: None,
            multisample: Default::default(),
            fragment: Some(wgpu::FragmentState { module: &module, entry_point: Some("fs"), compilation_options: Default::default(), targets: &[Some(wgpu::TextureFormat::R16Float.into())] }),
            multiview_mask: None,
            cache: None,
        });
        let layers = views
            .iter()
            .enumerate()
            .map(|(i, v)| {
                use wgpu::util::DeviceExt;
                let cam_buf = g.device.create_buffer_init(&wgpu::util::BufferInitDescriptor { label: Some("iw7 spot cam"), contents: &cam_bytes(v.view_projection(), v.pos), usage: wgpu::BufferUsages::UNIFORM });
                let cam = g.device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("iw7 spot cam"),
                    layout: &screen.cam_layout,
                    entries: &[wgpu::BindGroupEntry { binding: 0, resource: cam_buf.as_entire_binding() }, wgpu::BindGroupEntry { binding: 1, resource: screen.objects.as_entire_binding() }],
                });
                let r_buf = g.device.create_buffer_init(&wgpu::util::BufferInitDescriptor { label: Some("iw7 spot resolve"), contents: bytemuck::cast_slice(&[NEAR, v.reach, 0.0, 0.0]), usage: wgpu::BufferUsages::UNIFORM });
                let resolve_group = g.device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("iw7 spot resolve"),
                    layout: &resolve.get_bind_group_layout(0),
                    entries: &[wgpu::BindGroupEntry { binding: 0, resource: r_buf.as_entire_binding() }, wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&depth.1) }],
                });
                let view = tex.create_view(&wgpu::TextureViewDescriptor { dimension: Some(wgpu::TextureViewDimension::D2), base_array_layer: i as u32, array_layer_count: Some(1), ..Default::default() });
                Layer { cam, resolve: resolve_group, view }
            })
            .collect();
        let s = Self { records: Vec::with_capacity(views.len().max(1) * RECORD_WORDS), views, array: (tex, array_view), depth, layers, resolve, drawn: false };
        if s.layers.is_empty() {
            // (No spots: one layer of "nothing in front".)
            let white = s.array.0.create_view(&wgpu::TextureViewDescriptor { dimension: Some(wgpu::TextureViewDimension::D2), ..Default::default() });
            let mut enc = g.device.create_command_encoder(&Default::default());
            clear(&mut enc, &white);
            g.queue.submit([enc.finish()]);
        }
        s
    }

    /// Draws the maps the first time (the world doesn't move).
    pub fn record(&mut self, enc: &mut wgpu::CommandEncoder, screen: &Screen, vbs: &[wgpu::Buffer; 2], ibs: &[wgpu::Buffer; 2]) {
        if self.drawn {
            return;
        }
        self.drawn = true;
        for l in &self.layers {
            screen.draw_occluders(enc, &self.depth.1, &l.cam, vbs, ibs, None);
            let mut rp = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("iw7 spot resolve"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment { view: &l.view, resolve_target: None, depth_slice: None, ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::WHITE), store: wgpu::StoreOp::Store } })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            rp.set_pipeline(&self.resolve);
            rp.set_bind_group(0, &l.resolve, &[]);
            rp.draw(0..3, 0..1);
        }
    }

    /// The t25 records with the eye at `eye`.
    pub fn records(&mut self, eye: Vec3) -> &[u32] {
        self.records.clear();
        for v in &self.views {
            self.records.extend_from_slice(&v.record(eye));
        }
        if self.records.is_empty() {
            self.records.resize(RECORD_WORDS, 0);
        }
        &self.records
    }
}

fn clear(enc: &mut wgpu::CommandEncoder, view: &wgpu::TextureView) {
    let _ = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some("iw7 spot clear"),
        color_attachments: &[Some(wgpu::RenderPassColorAttachment { view, resolve_target: None, depth_slice: None, ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::WHITE), store: wgpu::StoreOp::Store } })],
        depth_stencil_attachment: None,
        timestamp_writes: None,
        occlusion_query_set: None,
        multiview_mask: None,
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spot() -> LocalLight {
        LocalLight { pos: Vec3::new(100.0, 0.0, 300.0), reach: 500.0, colour: [1.0; 3], toward: Vec3::Z, spot: true, cos_outer: 0.5, cos_inner: 0.9, up: Vec3::X, bulb_radius: 1.0, bulb_length: 0.0, casts: true, shadow: None }
    }

    #[test]
    fn the_record_maps_a_point_below_the_spot_to_the_map_centre_and_its_depth() {
        let v = SpotView::of(&spot());
        let eye = Vec3::new(-50.0, 20.0, 60.0);
        let r = v.record(eye);
        let row = |i: usize| Vec4::new(f32::from_bits(r[8 + i * 4]), f32::from_bits(r[9 + i * 4]), f32::from_bits(r[10 + i * 4]), f32::from_bits(r[11 + i * 4]));
        // 200 units straight below the light, about the eye.
        let p = (Vec3::new(100.0, 0.0, 100.0) - eye).extend(1.0);
        let q = Vec4::new(row(0).dot(p), row(1).dot(p), row(2).dot(p), row(3).dot(p));
        assert!((q.x / q.w - 0.5).abs() < 1e-4 && (q.y / q.w - 0.5).abs() < 1e-4, "{q:?}");
        assert!((q.z - 200.0).abs() < 1e-2, "{q:?}");
        // A stored depth equal to the pixel's lets the light through; one
        // well in front of it doesn't.
        let s = f32::from_bits(r[7]);
        let test = |stored: f32| ((1.4427 * f32::from_bits(r[4]) * (q.z * s - stored)).exp2() * f32::from_bits(r[5]) + f32::from_bits(r[6])).clamp(0.0, 1.0);
        assert!(test(200.0 / 500.0) > 0.99);
        assert!(test(150.0 / 500.0) < 0.01);
    }

    #[test]
    fn only_the_spots_allowed_get_a_shadow() {
        let mut l = vec![spot(), LocalLight { casts: false, ..spot() }, LocalLight { spot: false, ..spot() }];
        let v = choose(&mut l);
        assert_eq!(v.len(), 1);
        assert_eq!(l.iter().map(|l| l.shadow).collect::<Vec<_>>(), vec![Some(0), None, None]);
    }
}
