//! bo2mp: the shared look's post passes on a Black Ops II map
//! (`BO2MP_BO2_SHARED_LOOK=1`, `docs/BO2_SHARED_LOOK.md`): Black Ops III's
//! SMAA 1x and camera motion blur (`render_t7::bo2_shared::SharedPost`,
//! its own programs from `core_patch.ff`) over the frame after Black Ops
//! II's bloom and colour table, before the film and depth of field passes.
//!
//! It runs on a frame whose sun shadow was the shared look's cascades
//! ([`FRAME`], set by the fallback pass's sun shadow: the switch and a map
//! the lane marked), never under a game's own renderer. Black Ops III's
//! `zone` folder comes from the map load (`asset_world::
//! shared_look_zone_dir`); the passes are made on a thread of their own.
//! While SMAA is in, Black Ops II's FXAA stays out ([`smaa_ready`]).

use std::path::PathBuf;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, TryRecvError, channel};

use bevy::core_pipeline::{Core3d, Core3dSystems};
use bevy::prelude::*;
use bevy::render::renderer::{RenderContext, RenderDevice, RenderQueue, ViewQuery};
use bevy::render::view::{ExtractedView, ViewTarget};

use render_h1::gpu::Gpu;
use render_t7::bo2_shared::{PostSettings, SharedPost, Turn};

/// This frame's sun shadow was the shared look's (set by
/// `geometry_diagnostic::sun_shadow::draw_sun_shadow`).
pub(crate) static FRAME: AtomicBool = AtomicBool::new(false);
static SMAA: AtomicBool = AtomicBool::new(false);

/// Whether Black Ops III's SMAA ran over the last frame (Black Ops II's
/// FXAA then stays out, `bo2_bloom`).
pub(crate) fn smaa_ready() -> bool {
    SMAA.load(Ordering::Relaxed)
}

#[derive(Resource, Default)]
struct SharedLook {
    zone: Option<PathBuf>,
    loading: Option<Mutex<Receiver<Result<SharedPost, String>>>>,
    post: Option<SharedPost>,
    /// The last frame's projection and view (world to view).
    prev: Option<(Mat4, Mat4)>,
    /// The first frame was logged.
    logged: bool,
}

fn draw_shared_look(
    view: ViewQuery<(&ViewTarget, &ExtractedView)>,
    mut state: ResMut<SharedLook>,
    device: Res<RenderDevice>,
    queue: Res<RenderQueue>,
    mut context: RenderContext,
) {
    if !FRAME.load(Ordering::Relaxed) || super::native_world() {
        SMAA.store(false, Ordering::Relaxed);
        state.prev = None;
        return;
    }
    let zone = asset_world::shared_look_zone_dir();
    if zone != state.zone {
        state.zone = zone.clone();
        state.post = None;
        state.loading = None;
        state.logged = false;
        if let Some(dir) = zone {
            let gpu = Gpu::from_parts(device.wgpu_device().clone(), (****queue).clone());
            let (tx, rx) = channel();
            let spawned = std::thread::Builder::new().name("bo2_shared_post".into()).spawn(move || {
                let t = std::time::Instant::now();
                let made = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| SharedPost::new(&gpu, &dir, PostSettings::from_env()))).unwrap_or_else(|_| Err("making the passes panicked".into()));
                if made.is_ok() {
                    diag::info!(World, "bo2 shared look: post passes made in {:.1}s from {}", t.elapsed().as_secs_f32(), dir.display());
                }
                let _ = tx.send(made);
            });
            if spawned.is_ok() {
                state.loading = Some(Mutex::new(rx));
            }
        }
    }
    let received = state.loading.as_ref().map(|rx| rx.lock().map_or(Err(TryRecvError::Disconnected), |rx| rx.try_recv()));
    match received {
        Some(Ok(Ok(post))) => {
            for l in &post.report {
                diag::info!(World, "{l}");
            }
            state.post = Some(post);
            state.loading = None;
        }
        Some(Ok(Err(e))) => {
            diag::warn!(World, "bo2 shared look: no SMAA or motion blur ({e})");
            state.loading = None;
        }
        Some(Err(TryRecvError::Disconnected)) => state.loading = None,
        _ => {}
    }
    let (target, extracted) = view.into_inner();
    let clip_from_view = extracted.clip_from_view;
    let view_from_world = extracted.world_from_view.to_matrix().inverse();
    let turn = state.prev.map(|(pc, pv)| Turn::new(clip_from_view, view_from_world, pc, pv));
    state.prev = Some((clip_from_view, view_from_world));
    let first = !state.logged;
    let Some(post) = state.post.as_mut() else {
        SMAA.store(false, Ordering::Relaxed);
        return;
    };
    SMAA.store(post.has_smaa(), Ordering::Relaxed);
    let size = target.main_texture().size();
    let format = target.main_texture_format();
    let write = target.post_process_write();
    let encoder = context.command_encoder();
    match post.record(encoder, write.source, format.is_srgb(), write.destination, format, size.width, size.height, turn) {
        Ok(()) if first => {
            diag::info!(World, "bo2 shared look: first frame through SMAA / motion blur ({}x{}, {format:?})", size.width, size.height);
            state.logged = true;
        }
        Ok(()) => {}
        Err(e) => {
            diag::warn!(World, "bo2 shared look: the post passes failed ({e}); off for this map");
            state.post = None;
        }
    }
}

pub(super) fn register(app: &mut App) {
    let Some(render_app) = app.get_sub_app_mut(bevy::render::RenderApp) else {
        return;
    };
    render_app.init_resource::<SharedLook>().add_systems(
        Core3d,
        draw_shared_look
            .in_set(Core3dSystems::PostProcess)
            .after(super::bo2_bloom::Bo2BloomSet)
            .before(super::postfx::PostFxSet),
    );
}
