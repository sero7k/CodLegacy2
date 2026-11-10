//! The sun cascades kept from frame to frame while the view moves.
//!
//! Each cascade is centred on a texel of its own grid
//! (`sun_shadow::snapped`, READ), so a cascade drawn at one centre holds
//! the same texels as one drawn at another: what changes as the eye moves
//! is only which part of the world the square covers. The far cascade
//! therefore keeps its centre while the eye stays within `FAR_HOLD_TEXELS`
//! of it along the light's axes and is drawn again only when it is moved
//! (re-centred on the eye) or something in it moved; the near cascade is
//! drawn when its own snapped centre or the view changes (as before: the
//! displacement casters read the camera). Ghosts' dvars have no sun shadow
//! cache (`docs/exe_notes/iw6/dvars_all.tsv`: no `sm_cache*`): holding the
//! far cascade is ours.
//!
//! The atlas is one texture, so a cascade drawn alone first clears its own
//! quarter (`Clear`: a triangle at depth 1 over that viewport) and loads
//! the other's.

use glam::Vec2;

/// How far (in far cascade texels, along the light's right or up axis)
/// the eye may move from the far cascade's centre before it is moved:
/// 64 texels, 128 units on ULTRA (1/8 of the cascade's half size, its
/// fade band: the far cascade's edge then never comes nearer the eye than
/// 3/4 of its half size, 768 units on ULTRA, far past the near cascade).
pub const FAR_HOLD_TEXELS: f32 = 64.0;

/// `BO2MP_IW6_SUN_CACHE=0`: both cascades drawn whenever the view moves
/// (the far one not held; its centre follows the eye's texel).
pub fn on() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| !std::env::var("BO2MP_IW6_SUN_CACHE").is_ok_and(|v| v.trim() == "0"))
}

/// The cascades' centres along the light's axes for an eye at `at`
/// (`sun_shadow::eye_on_light`), the far one held at `held` while the eye
/// stays within `FAR_HOLD_TEXELS` of it.
pub fn centres(at: Vec2, held: Option<Vec2>, hold: bool) -> [Vec2; 2] {
    let near = crate::sun_shadow::snapped(at, 0);
    let reach = FAR_HOLD_TEXELS * crate::sun_shadow::texel(1);
    let far = match held {
        Some(c) if hold && (c - at).abs().max_element() <= reach => c,
        _ => crate::sun_shadow::snapped(at, 1),
    };
    [near, far]
}

/// Clears one viewport of a shadow atlas to the far depth (1).
pub struct Clear {
    pipeline: wgpu::RenderPipeline,
}

impl Clear {
    pub fn new(device: &wgpu::Device) -> Self {
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("iw6_shadow_clear"),
            source: wgpu::ShaderSource::Wgsl(
                "@vertex fn vs(@builtin(vertex_index) i: u32) -> @builtin(position) vec4<f32> {\n\
                 let x = f32((i << 1u) & 2u) * 2.0 - 1.0;\n\
                 let y = f32(i & 2u) * 2.0 - 1.0;\n\
                 return vec4<f32>(x, y, 1.0, 1.0);\n}\n\
                 @fragment fn fs() -> @location(0) vec4<f32> { return vec4<f32>(0.0); }\n"
                    .into(),
            ),
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("iw6_shadow_clear"),
            bind_group_layouts: &[],
            immediate_size: 0,
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("iw6_shadow_clear"),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &module,
                entry_point: Some("vs"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            primitive: Default::default(),
            depth_stencil: Some(wgpu::DepthStencilState {
                format: crate::world::SHADOW_DEPTH,
                depth_write_enabled: Some(true),
                depth_compare: Some(wgpu::CompareFunction::Always),
                stencil: Default::default(),
                bias: Default::default(),
            }),
            multisample: Default::default(),
            fragment: Some(wgpu::FragmentState {
                module: &module,
                entry_point: Some("fs"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: crate::world::SHADOW_COLOUR,
                    blend: None,
                    write_mask: wgpu::ColorWrites::empty(),
                })],
            }),
            multiview_mask: None,
            cache: None,
        });
        Self { pipeline }
    }

    /// Depth 1 over `[x, y, size]` of the atlas, the rest kept.
    pub fn record(&self, enc: &mut wgpu::CommandEncoder, colour: &wgpu::TextureView, depth: &wgpu::TextureView, viewport: [f32; 3]) {
        let [x, y, size] = viewport;
        let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("iw6_shadow_clear"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: colour,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Load,
                    store: wgpu::StoreOp::Store,
                },
                depth_slice: None,
            })],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: depth,
                depth_ops: Some(wgpu::Operations {
                    load: wgpu::LoadOp::Load,
                    store: wgpu::StoreOp::Store,
                }),
                stencil_ops: None,
            }),
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        pass.set_viewport(x, y, size, size, 0.0, 1.0);
        pass.set_scissor_rect(x as u32, y as u32, size as u32, size as u32);
        pass.set_pipeline(&self.pipeline);
        pass.draw(0..3, 0..1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_far_centre_is_held_then_moved() {
        let t = crate::sun_shadow::texel(1);
        let at = Vec2::new(10.3, -4.0);
        let [n, f] = centres(at, None, true);
        assert_eq!(n, crate::sun_shadow::snapped(at, 0));
        assert_eq!(f, crate::sun_shadow::snapped(at, 1));
        // A step inside the hold: the far centre stays.
        let step = at + Vec2::new(FAR_HOLD_TEXELS * t * 0.9, 0.0);
        assert_eq!(centres(step, Some(f), true)[1], f);
        // Past it: moved onto the eye's texel.
        let past = at + Vec2::new(0.0, (FAR_HOLD_TEXELS + 1.5) * t);
        assert_eq!(centres(past, Some(f), true)[1], crate::sun_shadow::snapped(past, 1));
        // Not held: always the eye's texel.
        assert_eq!(centres(step, Some(f), false)[1], crate::sun_shadow::snapped(step, 1));
    }
}
