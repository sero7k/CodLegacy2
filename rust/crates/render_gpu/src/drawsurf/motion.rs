//! bo2mp: the motion vectors of the main view, for a temporal upscaler (FSR 3.1,
//! DLSS; `Upscaler::wants_temporal`), and the debug view of them
//! (`BO2MP_MV_VIEW=1`).
//!
//! What exists, per lens view, while one is wanted (a `TemporalJitter` on the
//! camera, which `upscale::drive_jitter` puts there, or the debug view):
//!
//! * `ViewMotion::view`, an `Rg16Float` target at the lens's size. The main
//!   geometry pass writes it as a second colour attachment, so there is no
//!   pass of its own except the clear (`clear_motion`). The shader value is
//!   bevy's convention: the current minus the previous frame's *unjittered*
//!   position in UV units of the target (+x right, +y down); 0 where nothing
//!   was drawn.
//! * Written: the opaque world, props and models (the draws that write depth),
//!   water, and the sky. Not written: blended draws (decals, glass, particles,
//!   effects): they keep the motion of what is behind them, which is what
//!   the upscalers take them to be (their reactive mask is the way to say
//!   otherwise, not built yet).
//! * How: static things reproject with the previous frame's unjittered
//!   view-projection (`ViewMotion::params`, written here from last frame's
//!   `ExtractedView`); the sky by the camera's turn alone; the first-person gun
//!   and arms 0 (they ride the camera; their own sway ghosts a little). The
//!   extract keeps no previous transform of anything that moves (players,
//!   grenades, effects, script models), so those take the camera-only
//!   reprojection too: a moving player is wrong by his own movement.
//!   The BO3 and MWR native renderers (`t7_native`, `h1_native`) draw their
//!   own worlds with no motion output: the target stays 0 under them.
//!
//! With no temporal upscaler and no debug view nothing here runs: no texture,
//! no second attachment, pipelines without the `MOTION` shader def.

use bevy::core_pipeline::{Core3d, Core3dSystems};
use bevy::prelude::*;
use bevy::render::RenderStartup;
use bevy::render::camera::ExtractedCamera;
use bevy::render::camera::TemporalJitter;
use bevy::render::render_resource::binding_types::texture_2d;
use bevy::render::render_resource::{
    BindGroupEntries, BindGroupLayoutDescriptor, BindGroupLayoutEntries, Buffer, BufferDescriptor,
    BufferUsages, CachedRenderPipelineId, ColorTargetState, ColorWrites, Extent3d, FragmentState,
    LoadOp, Operations, PipelineCache, RenderPassColorAttachment, RenderPassDescriptor,
    RenderPipelineDescriptor, ShaderStages, StoreOp, Texture, TextureDescriptor, TextureDimension,
    TextureFormat, TextureSampleType, TextureUsages, TextureView, VertexState,
};
use bevy::render::renderer::{RenderContext, RenderDevice, RenderQueue, ViewQuery};
use bevy::render::texture::TextureCache;
use bevy::render::view::{ExtractedView, Msaa, ViewTarget};
use bevy::shader::Shader;
use std::collections::HashMap;
use std::sync::OnceLock;

const VIEW_SHADER_PATH: &str = "embedded://render_gpu/drawsurf/motion_view.wgsl";
pub(super) const MOTION_FORMAT: TextureFormat = TextureFormat::Rg16Float;
/// The shader's `MotionParams`: the previous frame's unjittered
/// clip-from-world, then flags (x: 1 for no motion).
pub(super) const MOTION_PARAMS_BYTES: u64 = 80;

/// `BO2MP_MV_VIEW=1`: draw the motion vectors over the frame (grey is still,
/// red and green are x and y, a pixel to 1/16 of the range) and make them
/// even with no upscaler chosen.
pub(super) fn debug_view() -> bool {
    static ON: OnceLock<bool> = OnceLock::new();
    *ON.get_or_init(|| {
        std::env::var("BO2MP_MV_VIEW")
            .is_ok_and(|v| !v.is_empty() && v != "0" && !v.eq_ignore_ascii_case("false"))
    })
}

/// A lens view with motion vectors this frame (render world).
#[derive(Component, Clone)]
#[allow(dead_code)] // (texture, size: for the upscaler pass to come.)
pub(crate) struct ViewMotion {
    pub texture: Texture,
    pub view: TextureView,
    /// The lens's size in pixels (the target's, the depth's).
    pub size: UVec2,
    /// `MotionParams` for the world; and the same with the "no motion" flag
    /// for the first-person pass.
    pub(super) params: Buffer,
    pub(super) params_viewmodel: Buffer,
}

impl ViewMotion {
    /// The attachment the geometry passes add: the first pass clears
    /// through `clear_motion`, so they all keep what is there.
    pub(super) fn attachment(&self) -> RenderPassColorAttachment<'_> {
        RenderPassColorAttachment {
            view: &self.view,
            depth_slice: None,
            resolve_target: None,
            ops: Operations {
                load: LoadOp::Load,
                store: StoreOp::Store,
            },
        }
    }
}

/// The unjittered view-projection of a view: what the vertices are mapped by
/// without the sub-pixel jitter.
pub(super) fn unjittered_clip_from_world(view: &ExtractedView) -> Mat4 {
    view.clip_from_view * view.world_from_view.to_matrix().inverse()
}

fn params_bytes(prev_clip_from_world: Mat4, no_motion: bool) -> [f32; 20] {
    let mut data = [0.0f32; 20];
    data[..16].copy_from_slice(&prev_clip_from_world.to_cols_array());
    data[16] = if no_motion { 1.0 } else { 0.0 };
    data
}

/// Makes (or drops) each lens view's motion target and writes its parameters.
/// Runs with the other `PrepareResources`, so the view bind group made in
/// `prepare_views` (`PrepareBindGroups`) sees the result.
#[allow(clippy::type_complexity)]
pub(super) fn prepare_motion(
    mut commands: Commands,
    device: Res<RenderDevice>,
    queue: Res<RenderQueue>,
    mut cache: ResMut<TextureCache>,
    mut history: Local<HashMap<Entity, Mat4>>,
    mut buffers: Local<HashMap<Entity, (Buffer, Buffer)>>,
    mut said: Local<bool>,
    views: Query<
        (
            Entity,
            &ExtractedCamera,
            &ExtractedView,
            &Msaa,
            Has<TemporalJitter>,
            Has<ViewMotion>,
        ),
        With<Camera3d>,
    >,
) {
    let debug = debug_view();
    let mut live = Vec::new();
    for (entity, camera, view, msaa, jittered, has) in &views {
        let wanted = (jittered || debug) && msaa.samples() == 1;
        let size = camera.physical_target_size.filter(|s| s.x > 0 && s.y > 0);
        let Some(size) = size.filter(|_| wanted) else {
            if has {
                commands.entity(entity).remove::<ViewMotion>();
            }
            continue;
        };
        live.push(entity);
        let current = unjittered_clip_from_world(view);
        // (A view's first frame, or a resize, has no history: no motion.)
        let previous = history.insert(entity, current).unwrap_or(current);
        let (params, params_viewmodel) = buffers
            .entry(entity)
            .or_insert_with(|| {
                let make = |label| {
                    device.create_buffer(&BufferDescriptor {
                        label: Some(label),
                        size: MOTION_PARAMS_BYTES,
                        usage: BufferUsages::UNIFORM | BufferUsages::COPY_DST,
                        mapped_at_creation: false,
                    })
                };
                (
                    make("bo2mp_motion_params"),
                    make("bo2mp_motion_params_viewmodel"),
                )
            })
            .clone();
        queue.write_buffer(
            &params,
            0,
            bytemuck::cast_slice(&params_bytes(previous, false)),
        );
        queue.write_buffer(
            &params_viewmodel,
            0,
            bytemuck::cast_slice(&params_bytes(previous, true)),
        );
        let texture = cache.get(
            &device,
            TextureDescriptor {
                label: Some("bo2mp_motion_vectors"),
                size: Extent3d {
                    width: size.x,
                    height: size.y,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: TextureDimension::D2,
                format: MOTION_FORMAT,
                usage: TextureUsages::RENDER_ATTACHMENT | TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            },
        );
        commands.entity(entity).insert(ViewMotion {
            texture: texture.texture,
            view: texture.default_view,
            size,
            params,
            params_viewmodel,
        });
        if !*said {
            *said = true;
            diag::info!(
                Fpv,
                "bo2mp motion vectors: Rg16Float {}x{} on the main view{}",
                size.x,
                size.y,
                if debug { " (BO2MP_MV_VIEW)" } else { "" }
            );
        }
    }
    history.retain(|e, _| live.contains(e));
    buffers.retain(|e, _| live.contains(e));
}

/// Clears the motion target before the geometry passes (which load it, so
/// the early outs of the pass cannot leave last frame's picture in it).
pub(super) fn clear_motion(view: ViewQuery<&ViewMotion>, mut context: RenderContext) {
    let motion = view.into_inner();
    let encoder = context.command_encoder();
    let _pass = encoder.begin_render_pass(&RenderPassDescriptor {
        label: Some("bo2mp_motion_clear"),
        color_attachments: &[Some(RenderPassColorAttachment {
            view: &motion.view,
            depth_slice: None,
            resolve_target: None,
            ops: Operations {
                load: LoadOp::Clear(LinearRgba::BLACK.into()),
                store: StoreOp::Store,
            },
        })],
        depth_stencil_attachment: None,
        timestamp_writes: None,
        occlusion_query_set: None,
        multiview_mask: None,
    });
}

#[derive(Resource)]
struct MotionViewGpu {
    layout: BindGroupLayoutDescriptor,
    shader: Handle<Shader>,
    pipelines: HashMap<TextureFormat, CachedRenderPipelineId>,
}

fn init_view(mut commands: Commands, asset_server: Res<AssetServer>) {
    commands.insert_resource(MotionViewGpu {
        layout: BindGroupLayoutDescriptor::new(
            "bo2mp_motion_view",
            &BindGroupLayoutEntries::single(
                ShaderStages::FRAGMENT,
                texture_2d(TextureSampleType::Float { filterable: false }),
            ),
        ),
        shader: asset_server.load(VIEW_SHADER_PATH),
        pipelines: HashMap::new(),
    });
}

/// `BO2MP_MV_VIEW`: the motion vectors in place of the finished frame.
fn draw_motion_view(
    view: ViewQuery<(&ViewTarget, &ViewMotion)>,
    gpu: Option<ResMut<MotionViewGpu>>,
    cache: Res<PipelineCache>,
    device: Res<RenderDevice>,
    mut context: RenderContext,
) {
    if !debug_view() {
        return;
    }
    let Some(mut gpu) = gpu else {
        return;
    };
    let (target, motion) = view.into_inner();
    let format = target.main_texture_format();
    if !gpu.pipelines.contains_key(&format) {
        let descriptor = RenderPipelineDescriptor {
            label: Some("bo2mp_motion_view".into()),
            layout: vec![gpu.layout.clone()],
            immediate_size: 0,
            vertex: VertexState {
                shader: gpu.shader.clone(),
                shader_defs: Vec::new(),
                entry_point: Some("vertex".into()),
                buffers: Vec::new(),
            },
            fragment: Some(FragmentState {
                shader: gpu.shader.clone(),
                shader_defs: Vec::new(),
                entry_point: Some("fragment".into()),
                targets: vec![Some(ColorTargetState {
                    format,
                    blend: None,
                    write_mask: ColorWrites::ALL,
                })],
            }),
            primitive: Default::default(),
            depth_stencil: None,
            multisample: Default::default(),
            zero_initialize_workgroup_memory: false,
        };
        let id = cache.queue_render_pipeline(descriptor);
        gpu.pipelines.insert(format, id);
    }
    let Some(pipeline) = gpu
        .pipelines
        .get(&format)
        .and_then(|&id| cache.get_render_pipeline(id))
    else {
        return;
    };
    let layout = cache.get_bind_group_layout(&gpu.layout);
    let bind = device.create_bind_group(
        "bo2mp_motion_view",
        &layout,
        &BindGroupEntries::single(&motion.view),
    );
    let post = target.post_process_write();
    let encoder = context.command_encoder();
    let mut pass = encoder.begin_render_pass(&RenderPassDescriptor {
        label: Some("bo2mp_motion_view"),
        color_attachments: &[Some(RenderPassColorAttachment {
            view: post.destination,
            resolve_target: None,
            ops: Operations {
                load: LoadOp::Clear(LinearRgba::BLACK.into()),
                store: StoreOp::Store,
            },
            depth_slice: None,
        })],
        depth_stencil_attachment: None,
        timestamp_writes: None,
        occlusion_query_set: None,
        multiview_mask: None,
    });
    pass.set_pipeline(pipeline);
    pass.set_bind_group(0, &bind, &[]);
    pass.draw(0..3, 0..1);
}

pub(super) fn register(app: &mut App) {
    bevy::asset::embedded_asset!(app, "motion_view.wgsl");
    let Some(render_app) = app.get_sub_app_mut(bevy::render::RenderApp) else {
        return;
    };
    render_app
        .add_systems(RenderStartup, init_view)
        .add_systems(
            Core3d,
            draw_motion_view
                .in_set(Core3dSystems::PostProcess)
                .after(super::postfx::PostFxSet),
        );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn params_carry_the_matrix_then_the_flag() {
        let m = Mat4::from_translation(Vec3::new(1.0, 2.0, 3.0));
        let world = params_bytes(m, false);
        let gun = params_bytes(m, true);
        assert_eq!(&world[..16], &m.to_cols_array());
        assert_eq!(world[16], 0.0);
        assert_eq!(gun[16], 1.0);
        assert_eq!(std::mem::size_of_val(&world) as u64, MOTION_PARAMS_BYTES);
    }

    /// The shaders carry what the Rust side binds.
    #[test]
    fn shaders_have_their_entry_points_and_bindings() {
        let geometry = include_str!("geometry_diagnostic.wgsl");
        for needle in [
            "#ifdef MOTION",
            "@group(0) @binding(1) var<uniform> motion_params: MotionParams;",
            "@location(1) motion: vec2<f32>",
            "view.unjittered_clip_from_world",
            "fn motion_sky(",
        ] {
            assert!(geometry.contains(needle), "{needle}");
        }
        for entry in [
            "fn fragment(",
            "fn fragment_textured(",
            "fn fragment_lightmapped(",
            "fn fragment_prop(",
            "fn fragment_sky(",
        ] {
            assert!(geometry.contains(entry), "{entry}");
        }
        let view = include_str!("motion_view.wgsl");
        for entry in ["fn vertex(", "fn fragment("] {
            assert!(view.contains(entry), "{entry}");
        }
    }

    /// bo2mp D2: cloth sways only as far as its vertex alpha lets it (Black
    /// Ops II's flag shader), so a pinned table cloth stays on its table.
    #[test]
    fn cloth_sway_is_weighted_by_the_vertex_alpha() {
        let geometry = include_str!("geometry_diagnostic.wgsl");
        for needle in [
            "normal: vec3<f32>, weight: f32) -> vec3<f32> {",
            "let amplitude = 2.5 * reach * gust * saturate(weight);",
            "cloth_offset(in.position, world.xyz, cloth_normal, in.color.a)",
        ] {
            assert!(geometry.contains(needle), "{needle}");
        }
        assert_eq!(geometry.matches("cloth_offset(").count(), 2);
    }

    /// bevy's own `view.wgsl` (the `#import bevy_render::view::View` the
    /// shader composes against), read from the cargo registry copy of the
    /// bevy_render this build uses. None when it cannot be found.
    fn bevy_view_wgsl() -> Option<String> {
        let home = std::env::var_os("CARGO_HOME")
            .map(std::path::PathBuf::from)
            .or_else(|| {
                std::env::var_os("HOME")
                    .or_else(|| std::env::var_os("USERPROFILE"))
                    .map(|h| std::path::PathBuf::from(h).join(".cargo"))
            })?;
        for source in std::fs::read_dir(home.join("registry/src")).ok()?.flatten() {
            let path = source.path().join("bevy_render-0.19.0/src/view/view.wgsl");
            if let Ok(text) = std::fs::read_to_string(path) {
                return Some(text);
            }
        }
        None
    }

    /// The names the shader tests with `#ifdef`, `#ifndef` and `#else ifdef`.
    fn def_names(source: &str) -> Vec<String> {
        let mut names = std::collections::BTreeSet::new();
        for line in source.lines() {
            let line = line.trim();
            for prefix in ["#ifdef ", "#ifndef ", "#else ifdef ", "#else ifndef "] {
                if let Some(rest) = line.strip_prefix(prefix) {
                    names.insert(rest.trim().to_string());
                }
            }
        }
        names.into_iter().collect()
    }

    fn compile(
        view: &str,
        source: &str,
        defs: &[&str],
    ) -> Result<naga::Module, String> {
        use naga_oil::compose::{
            ComposableModuleDescriptor, Composer, NagaModuleDescriptor, ShaderDefValue,
            ShaderLanguage, ShaderType,
        };
        let mut shader_defs = std::collections::HashMap::new();
        for def in defs {
            // (The pipeline's one valued def is the unlit colour scale.)
            let value = if *def == "UNLIT_SCALE" {
                ShaderDefValue::UInt(1)
            } else {
                ShaderDefValue::Bool(true)
            };
            shader_defs.insert((*def).to_string(), value);
        }
        let mut composer = Composer::default();
        let added = composer
            .add_composable_module(ComposableModuleDescriptor {
                source: view,
                file_path: "bevy_render/view.wgsl",
                language: ShaderLanguage::Wgsl,
                shader_defs: shader_defs.clone(),
                ..Default::default()
            })
            .map(|_| ());
        if let Err(e) = added {
            return Err(format!("view.wgsl: {}", e.emit_to_string(&composer)));
        }
        composer
            .make_naga_module(NagaModuleDescriptor {
                source,
                file_path: "geometry_diagnostic.wgsl",
                shader_type: ShaderType::Wgsl,
                shader_defs,
                ..Default::default()
            })
            .map_err(|e| e.emit_to_string(&composer))
    }

    /// `geometry_diagnostic.wgsl` through the composer bevy uses (naga_oil),
    /// not a home-made `#ifdef` pass: with and without `MOTION`, alone and
    /// with each of its other defs, and with the def sets the pipeline builds.
    /// What compiles without `MOTION` must compile with it, and the `MOTION`
    /// build must carry the second colour output the pipelines' second target
    /// takes (`@location(1)`).
    #[test]
    fn geometry_shader_compiles_with_motion_through_naga_oil() {
        let Some(view) = bevy_view_wgsl() else {
            eprintln!("bevy_render's view.wgsl is not in the cargo registry; skipped");
            return;
        };
        let source = include_str!("geometry_diagnostic.wgsl");
        let names: Vec<String> = def_names(source)
            .into_iter()
            .filter(|n| n != "MOTION")
            .collect();
        assert!(names.len() > 10, "{names:?}");

        // The sets the pipeline builds (`specialize`): the plain world, the
        // sky and models, and textured world / prop styles.
        let mut sets: Vec<Vec<&str>> = vec![
            vec![],
            vec!["DIAG_CHECKER"],
            vec!["UNLIT_SCALE"],
            vec!["UNLIT_SCALE", "UNLIT"],
            vec!["UNLIT_SCALE", "UNLIT", "ADD"],
            vec!["UNLIT_SCALE", "UNLIT", "MULTIPLY"],
            vec!["UNLIT_SCALE", "ALPHA_TEST"],
            vec!["UNLIT_SCALE", "SHINE"],
            vec!["UNLIT_SCALE", "SHINE", "WATER"],
            vec!["UNLIT_SCALE", "SHINE", "FLOW"],
            vec!["UNLIT_SCALE", "LAYERS", "LAYER1_BLEND", "LAYER2_MULTIPLY"],
            vec!["UNLIT_SCALE", "LAYERS", "LAYER1_TEST", "LAYER2_ADD", "UNTINTED"],
            vec!["UNLIT_SCALE", "LAYERS", "LAYER1_BLEND", "LAYER2_BLEND", "UNTINTED"],
            vec!["UNLIT_SCALE", "SHINE", "LAYERS", "LAYER1_BLEND", "LAYER1_SPEC", "UNTINTED"],
            vec!["UNLIT_SCALE", "EFFECT", "EFFECT_ADD", "SOFT"],
            vec!["UNLIT_SCALE", "EFFECT", "CLOUD"],
            vec!["UNLIT_SCALE", "FOLIAGE"],
            vec!["UNLIT_SCALE", "OBJECTIVE", "GHOST"],
            // (A Ghosts map's frame drawn in stages: into its HDR scene.)
            vec!["UNLIT_SCALE", "SHINE", "IW6_HDR", "IW6_HDR_OPAQUE"],
            vec!["UNLIT_SCALE", "IW6_HDR", "IW6_HDR_OPAQUE"],
            vec!["UNLIT_SCALE", "FOLIAGE", "IW6_HDR", "IW6_HDR_OPAQUE"],
            vec!["UNLIT_SCALE", "EFFECT", "EFFECT_ADD", "SOFT", "IW6_HDR"],
            vec!["UNLIT_SCALE", "UNLIT", "ADD", "IW6_HDR"],
            vec!["UNLIT_SCALE", "OBJECTIVE", "GHOST", "IW6_HDR"],
        ];
        // Each def alone, on the textured base.
        for name in &names {
            sets.push(vec!["UNLIT_SCALE", name.as_str()]);
        }

        let mut failures = Vec::new();
        let mut compiled = 0;
        for set in &sets {
            let plain = compile(&view, source, set);
            let mut with_motion_defs = set.clone();
            with_motion_defs.push("MOTION");
            let motion = compile(&view, source, &with_motion_defs);
            match (&plain, &motion) {
                (Ok(_), Ok(module)) => {
                    compiled += 1;
                    // Every fragment entry point writes a second colour.
                    for entry in module
                        .entry_points
                        .iter()
                        .filter(|e| e.stage == naga::ShaderStage::Fragment)
                    {
                        let second = entry.function.result.as_ref().is_some_and(|r| {
                            let has_location_1 = |binding: &Option<naga::Binding>| {
                                matches!(
                                    binding,
                                    Some(naga::Binding::Location { location: 1, .. })
                                )
                            };
                            match &module.types[r.ty].inner {
                                naga::TypeInner::Struct { members, .. } => {
                                    members.iter().any(|m| has_location_1(&m.binding))
                                }
                                _ => false,
                            }
                        });
                        if !second {
                            failures.push(format!(
                                "{set:?} + MOTION: fragment `{}` has no @location(1) output",
                                entry.name
                            ));
                        }
                    }
                }
                (Ok(_), Err(error)) => {
                    failures.push(format!("{set:?} compiles, + MOTION does not:\n{error}"))
                }
                // (A set the shader never builds, e.g. a def that needs another.)
                (Err(_), _) => {}
            }
        }
        assert!(failures.is_empty(), "{}", failures.join("\n---\n"));
        // The sets above that the pipeline really builds must compile at all.
        for set in sets.iter().take(16) {
            if let Err(error) = compile(&view, source, set) {
                panic!("{set:?} does not compile:\n{error}");
            }
        }
        assert!(compiled >= 16, "only {compiled} def sets compiled");
    }
}
