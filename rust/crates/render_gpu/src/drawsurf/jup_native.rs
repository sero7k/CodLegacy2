//! bo2mp: a Modern Warfare III / II map drawn by `render_jup` (the game's
//! own lighting and display, its programs where they can be driven; log
//! lines start `jup native:`). The zone is named as the map loads
//! (`asset_world::jup_native_zone`, only with `BO2MP_JUP_NATIVE=1` since 2026-10-28); the scene
//! is read on a thread of its own, the host made on the render thread, and
//! the finished image and scene depth handed into the frame exactly as
//! `t7_native` does, with Black Ops II's grade and bloom skipped. Below is
//! `t7_native`'s description, which this follows.
//!
//! (BO3:) a Black Ops III map drawn the way BlackOps3.exe draws it
//! (`render_t7`): every surface with its material's own G-buffer technique
//! (the map's programs, the exe's state maps, samplers and input layouts),
//! then BO3's lighting and post chain.
//!
//! The map's zone is named as it loads (`asset_world::t7_native_zone`,
//! off with `BO2MP_T7_NATIVE=0`); the scene (zone, the zones beside it, the
//! `.xpak` packages) is read on a thread of its own, then recorded every
//! frame in the main pass. Once the chain gives a finished image
//! (`render_jup::host::Host::final_image`), it is handed into the frame
//! (`render_t7::present`): the image into the view's target and the scene
//! depth into the frame's scene depth, in the scene's depth band, so
//! players, the gun, effects and the HUD drawn after it sit in the world;
//! while it does, the converted world, props and sky stay out (`active`)
//! and Black Ops II's grade and bloom do not run over BO3's. Until then
//! (`render_t7::host::LIGHTING_BUILT`), the G-buffer is recorded once to
//! check every pipeline and binding on this device, the converted world
//! keeps drawing, and what was made and what is missing goes to the log
//! (lines starting `jup native:`). `BO2MP_T7_NATIVE_DEBUG=gbuffer` hands the
//! G-buffer's diffuse target to the screen, to check the hand-off.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, TryRecvError, channel};
use std::sync::Mutex;

use bevy::core_pipeline::core_3d::main_opaque_pass_3d;
use bevy::core_pipeline::{Core3d, Core3dSystems};
use bevy::prelude::*;
use bevy::render::render_resource::{RenderPassDescriptor, StoreOp, TextureViewDescriptor};
use bevy::render::renderer::{RenderContext, RenderDevice, RenderQueue, ViewQuery};
use bevy::render::view::{ExtractedView, ViewTarget};

use render_h1::gpu::Gpu;
use render_jup::game_view::GameView;
use render_jup::host::Host;
use render_jup::present::Present;
use render_jup::scene::Scene;

use super::depth_range::{GFX_DEPTH_RANGE_SCENE, reverse_z_viewport_depth};
use super::scene_depth::{SCENE_DEPTH_FORMAT, SceneDepthTexture};

/// Whether this frame's world was drawn by Black Ops III's chain.
static ACTIVE: AtomicBool = AtomicBool::new(false);

pub(crate) fn active() -> bool {
    ACTIVE.load(Ordering::Relaxed)
}

#[derive(Resource, Default)]
struct JupNative {
    zone: Option<PathBuf>,
    /// The number of the request (`asset_world::t7_native_request`) this was
    /// made for.
    request: u64,
    loading: Option<Mutex<Receiver<Result<Scene, String>>>>,
    host: Option<Host>,
    present: Option<Present>,
    /// When the zone was named (the load began).
    started: Option<std::time::Instant>,
    announced_first_frame: bool,
    /// When the last 'still waiting' line was logged.
    last_wait_note: Option<std::time::Instant>,
    /// A view has reached the native draw since the load began.
    saw_view: bool,
    /// Frames drawn since the load began (for the camera log lines).
    frames: u64,
    /// The depth convention last logged.
    last_convention: String,
}

/// How long the loading screen is held for the native scene before the game
/// goes on without it (and says why).
const LOAD_HOLD_LIMIT_SECS: f32 = 60.0;

/// Starts the native renderer's load as the map load names its zone (before
/// the map is read: the programs and pipelines that need no map are made
/// while it is), takes the scene when it is read, and makes the host. Runs
/// without a view.
fn load_jup_native(mut state: ResMut<JupNative>, device: Res<RenderDevice>, queue: Res<RenderQueue>) {
    let (zone, request) = asset_world::jup_native_request();
    if request != state.request {
        state.request = request;
        state.host = None;
        state.present = None;
        state.loading = None;
        state.zone = zone.clone();
        state.announced_first_frame = false;
        state.last_wait_note = None;
        state.saw_view = false;
        state.frames = 0;
        state.last_convention.clear();
        state.started = zone.is_some().then(std::time::Instant::now);
        if let Some(z) = zone {
            let gpu = Gpu::from_parts(device.wgpu_device().clone(), (****queue).clone());
            diag::info!(World, "jup native: load begins for {}", z.display());
            let (tx, rx) = channel();
            let spawned = std::thread::Builder::new().name("jup_native_scene".into()).spawn(move || {
                let t = std::time::Instant::now();
                // (Waits for the map load to hand its map over, so the zone
                // is read once.)
                let scene = std::panic::catch_unwind(|| Scene::load(&z)).unwrap_or_else(|_| Err("the scene read panicked".into()));
                match &scene {
                    Ok(s) => {
                        for l in &s.report {
                            diag::info!(World, "jup native: {l}");
                        }
                        diag::info!(World, "jup native: {} scene ready {:.1}s after the load began ({} draws, {} materials, {} pictures)", s.name, t.elapsed().as_secs_f32(), s.draws.len(), s.materials.len(), s.images.len());
                    }
                    Err(e) => diag::warn!(World, "jup native: the scene did not read: {e}"),
                }
                let _ = tx.send(scene);
            });
            if spawned.is_ok() {
                state.loading = Some(Mutex::new(rx));
            } else {
                frame::settings::t7_native_load::set(frame::settings::t7_native_load::FAILED);
            }
        }
    }
    let received = state.loading.as_ref().map(|rx| rx.lock().map_or(Err(TryRecvError::Disconnected), |rx| rx.try_recv()));
    if let Some(received) = received {
        match received {
            Ok(Ok(scene)) => {
                let gpu = Gpu::from_parts(device.wgpu_device().clone(), (****queue).clone());
                diag::info!(World, "jup native: device features {:?}", gpu.device.features());
                let l = gpu.device.limits();
                diag::info!(World, 
                    "jup native: device limits: workgroups/dimension {}, storage binding {} B, buffer {} B, texture 2d {}, array layers {}, vertex buffers {}, storage textures/stage {}",
                    l.max_compute_workgroups_per_dimension, l.max_storage_buffer_binding_size, l.max_buffer_size, l.max_texture_dimension_2d, l.max_texture_array_layers, l.max_vertex_buffers, l.max_storage_textures_per_shader_stage
                );
                match Host::new(gpu, scene) {
                    Ok(host) => state.host = Some(host),
                    Err(e) => {
                        diag::warn!(World, "jup native: the host was not made: {e}");
                        frame::settings::t7_native_load::set(frame::settings::t7_native_load::FAILED);
                    }
                }
                state.loading = None;
            }
            Ok(Err(_)) | Err(TryRecvError::Disconnected) => {
                state.loading = None;
                // (Nothing else is drawn for the map: the loading screen is let go.)
                frame::settings::t7_native_load::set(frame::settings::t7_native_load::FAILED);
            }
            Err(TryRecvError::Empty) => {}
        }
    }
    // The host takes its load thread's stages here, every tick: a loading
    // screen may have no view that draws (the stages were only taken from a
    // frame before, so a held screen waited for a frame that needed them).
    let mut stages_made = false;
    if let Some(host) = state.host.as_mut() {
        stages_made = host.poll();
        // The host's log lines (the load thread's timings, device checks)
        // come out even before a view draws.
        for line in host.take_report() {
            diag::info!(World, "jup native: {line}");
        }
    }
    use frame::settings::t7_native_load as hold;
    if hold::state() == hold::PENDING {
        let waited = state.started.map_or(0.0, |t| t.elapsed().as_secs_f32());
        let why = if state.loading.is_some() {
            "the scene is still being read".to_owned()
        } else if state.host.is_none() {
            "no host was made".to_owned()
        } else if !stages_made {
            "the host's load thread has not handed over its stages (see the 'load thread:' and 'warm:' lines)".to_owned()
        } else if !state.saw_view {
            "the stages are made but no view has reached the native draw yet".to_owned()
        } else {
            "a view reached the draw but no frame finished (see the frame lines)".to_owned()
        };
        if stages_made && state.host.is_some() && waited > 0.0 {
            // The passes are made: the screen can go; frames follow.
            diag::info!(World, "jup native: the stages are made {waited:.1}s after the load began; letting the loading screen go (first frame follows)");
            hold::set(hold::READY);
        } else if waited > LOAD_HOLD_LIMIT_SECS {
            diag::warn!(World, "jup native: giving up the wait after {waited:.0}s, going on without the first native frame: {why}");
            hold::set(if state.host.is_some() { hold::READY } else { hold::FAILED });
        } else if state.last_wait_note.is_none_or(|t| t.elapsed().as_secs_f32() >= 5.0) && state.started.is_some() {
            state.last_wait_note = Some(std::time::Instant::now());
            diag::info!(World, "jup native: waiting for the first native frame, {waited:.0}s so far: {why}");
        }
    }
}

fn draw_jup_native(view: ViewQuery<(&ViewTarget, &SceneDepthTexture, &ExtractedView)>, mut state: ResMut<JupNative>, device: Res<RenderDevice>, queue: Res<RenderQueue>, mut context: RenderContext) {
    let state = &mut *state;
    let Some(host) = state.host.as_mut() else {
        ACTIVE.store(false, Ordering::Relaxed);
        return;
    };
    if !state.saw_view {
        state.saw_view = true;
        diag::info!(World, "jup native: first view reached the native draw {:.1}s after the load began", state.started.map_or(0.0, |t| t.elapsed().as_secs_f32()));
    }
    let (target, depth, extracted) = view.into_inner();
    if !super::scene_depth::native_view_ok("jup native", target, depth) {
        ACTIVE.store(false, Ordering::Relaxed);
        return;
    }
    let [vx, vy, width, height] = extracted.viewport.to_array();
    if width == 0 || height == 0 {
        ACTIVE.store(false, Ordering::Relaxed);
        return;
    }
    // The view as the game set it: IW world coordinates, a right-handed
    // view (x right, y up, looking down -z), reversed infinite depth.
    let world_from_view = extracted.world_from_view.to_matrix();
    let eye = extracted.world_from_view.translation();
    let view_from_world = world_from_view.inverse();
    let clip_from_view = extracted.clip_from_view;
    // The host's camera is made from the lens's own numbers (the same code
    // `jup_shot` uses), whatever the frame's clip matrix carries.
    let game_view = GameView { eye, view_from_world, clip_from_view, clip_from_world: extracted.clip_from_world };
    let camera = game_view.camera();
    state.frames += 1;
    let convention = game_view.convention();
    if state.frames == 1 || state.frames == 240 || state.last_convention != convention {
        let what = if state.frames == 1 { "first frame" } else if state.frames == 240 { "frame 240" } else { "the convention changed" };
        diag::info!(World, "jup native: {what}: {}; exposure {} (BO2MP_JUP_EXPOSURE)", game_view.describe(width, height), host.exposure());
        state.last_convention = convention;
    }
    let t_record = std::time::Instant::now();
    let complete = host.record(context.command_encoder(), width, height, &camera);
    let record_ms = t_record.elapsed().as_secs_f64() * 1000.0;
    for line in host.take_report() {
        diag::info!(World, "jup native: {line}");
    }
    let complete = match complete {
        Ok(complete) => complete,
        Err(e) => {
            diag::warn!(World, "jup native: {e}");
            false
        }
    };
    let (Some(image), Some(scene_depth)) = (host.final_image(), host.scene_depth()) else {
        ACTIVE.store(false, Ordering::Relaxed);
        return;
    };
    if !complete {
        ACTIVE.store(false, Ordering::Relaxed);
        return;
    }
    // The hand-off: take the frame's first-use clears (the next pass of the
    // frame would clear what is written here otherwise), then the image and
    // the scene depth into the frame.
    {
        let attachments = [Some(target.get_color_attachment())];
        let _pass = context.begin_tracked_render_pass(RenderPassDescriptor {
            label: Some("jup_native_take_clears"),
            color_attachments: &attachments,
            depth_stencil_attachment: Some(depth.get_attachment(StoreOp::Store)),
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
    }
    let format = target.main_texture_format();
    if state.present.as_ref().is_none_or(|p| p.formats() != (format, SCENE_DEPTH_FORMAT)) {
        let gpu = Gpu::from_parts(device.wgpu_device().clone(), (****queue).clone());
        state.present = Some(Present::new(gpu, format, SCENE_DEPTH_FORMAT));
        let t = target.main_texture().size();
        diag::info!(World, "jup native: frame handed to the screen ({width}x{height} at {vx},{vy}, target {}x{}, {format:?}, depth {SCENE_DEPTH_FORMAT:?})", t.width, t.height);
        if t.width != vx + width || t.height != vy + height {
            diag::warn!(World, "jup native: the view ({width}x{height} at {vx},{vy}) is not the whole target ({}x{}): the frame is placed inside the view only", t.width, t.height);
        }
    }
    let Some(present) = state.present.as_ref() else {
        return;
    };
    let depth_view = depth.texture.create_view(&TextureViewDescriptor::default());
    present.record(context.command_encoder(), image, scene_depth, target.main_texture_view(), &depth_view, reverse_z_viewport_depth(GFX_DEPTH_RANGE_SCENE), [vx, vy, width, height]);
    host.end_frame(context.command_encoder(), record_ms);
    ACTIVE.store(true, Ordering::Relaxed);
    if !state.announced_first_frame {
        state.announced_first_frame = true;
        diag::info!(World, "jup native: first finished frame {:.1}s after the load began", state.started.map_or(0.0, |t| t.elapsed().as_secs_f32()));
        // (The loading screen can go.)
        if frame::settings::t7_native_load::state() == frame::settings::t7_native_load::PENDING {
            frame::settings::t7_native_load::set(frame::settings::t7_native_load::READY);
        }
    }
}

pub(super) fn register(app: &mut App) {
    let Some(render_app) = app.get_sub_app_mut(bevy::render::RenderApp) else {
        return;
    };
    render_app
        .init_resource::<JupNative>()
        .add_systems(bevy::render::Render, load_jup_native.in_set(bevy::render::RenderSystems::Prepare))
        .add_systems(Core3d, draw_jup_native.in_set(Core3dSystems::MainPass).after(main_opaque_pass_3d).before(super::draw::ExactColourDrawSet));
}
