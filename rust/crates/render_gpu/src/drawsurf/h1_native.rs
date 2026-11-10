//! bo2mp: a Modern Warfare Remastered map drawn by MWR's own renderer
//! (`render_h1`): every surface with its material's own technique and
//! MWR's compiled shaders, the map's lightmaps, light grid, probes, sun,
//! fog, light set and film, then MWR's tone map and film into the frame.
//!
//! The map's zones are named as it loads (`asset_world::h1_native_zones`);
//! the scene is read on a thread of its own and then drawn every frame,
//! first in the main pass: it clears the scene depth and leaves the
//! world's depth in it (in the scene's depth band), so players, the gun
//! and effects drawn after it sit in the world. While it draws, the
//! converted world, props and sky of the fallback path stay out
//! (`active`), and Black Ops II's grade does not run over MWR's.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, TryRecvError, channel};
use std::sync::{Arc, Mutex};

use bevy::core_pipeline::core_3d::main_opaque_pass_3d;
use bevy::core_pipeline::{Core3d, Core3dSystems};
use bevy::prelude::*;
use bevy::render::render_resource::{RenderPassDescriptor, StoreOp, TextureViewDescriptor};
use bevy::render::renderer::{RenderContext, RenderDevice, RenderQueue, ViewQuery};
use bevy::render::view::{ExtractedView, ViewTarget};

use render_h1::code::Camera;
use render_h1::gpu::Gpu;
use render_h1::host::{Host, Look};
use render_h1::scene::Scene;

use super::depth_range::{GFX_DEPTH_RANGE_SCENE, reverse_z_viewport_depth};
use super::scene_depth::{SCENE_DEPTH_FORMAT, SceneDepthTexture};

/// Whether this frame's world was drawn by MWR's renderer.
static ACTIVE: AtomicBool = AtomicBool::new(false);
/// Whether the map is a Modern Warfare Remastered map: MWR's renderer is
/// its only renderer (as Black Ops III's maps have theirs), so the
/// converted world, props and sky never draw on it, even before the scene
/// is ready (the loading screen is held until the first native frame).
static H1_MAP: AtomicBool = AtomicBool::new(false);

pub(crate) fn active() -> bool {
    ACTIVE.load(Ordering::Relaxed) || H1_MAP.load(Ordering::Relaxed)
}

#[derive(Resource, Default)]
struct H1Native {
    /// The zones the scene below is (being) read from.
    zones: Option<Arc<[PathBuf]>>,
    loading: Option<Mutex<Receiver<Result<Scene, String>>>>,
    host: Option<Host>,
    /// Whether the first view after the scene was ready was logged.
    saw_view: bool,
    /// The `native_quality` revision last applied to the host.
    quality_rev: u64,
    /// The scene depth's view, kept while the texture is the same one
    /// (`BO2MP_NATIVE_VIEW_CACHE=0`: made every frame).
    depth_view: Option<(bevy::render::render_resource::TextureId, bevy::render::render_resource::TextureView)>,
}

fn draw_h1_native(
    view: ViewQuery<(&ViewTarget, &SceneDepthTexture, &ExtractedView)>,
    mut state: ResMut<H1Native>,
    device: Res<RenderDevice>,
    queue: Res<RenderQueue>,
    mut context: RenderContext,
) {
    // The map's zones: a new map starts a read of its scene.
    let zones = asset_world::h1_native_zones();
    H1_MAP.store(zones.is_some(), Ordering::Relaxed);
    let same = match (&zones, &state.zones) {
        (Some(a), Some(b)) => a[..] == b[..],
        (None, None) => true,
        _ => false,
    };
    if !same {
        state.host = None;
        state.loading = None;
        state.saw_view = false;
        state.zones = zones.clone();
        if let Some(z) = zones {
            diag::info!(
                World,
                "h1 native: load begins ({} zones, first {})",
                z.len(),
                z.first().map_or(String::new(), |p| p.display().to_string())
            );
            let (tx, rx) = channel();
            let list = z.to_vec();
            let spawned = std::thread::Builder::new()
                .name("h1_native_scene".into())
                .spawn(move || {
                    let t = std::time::Instant::now();
                    // The game already holds several GB of other pictures
                    // on the card; MWR's at 2048 a side ran it out of memory
                    // (OutOfMemory on the first native frame, 16 GB card).
                    // 1024 by default; `BO2MP_H1_MAX_TEX=n` sets it (2048 was
                    // the old size).
                    // (The shared quality's picture cap is an upper bound on 1024:
                    // 2048 ran this card out of memory.)
                    let quality_cap = frame::settings::native_quality::current().max_texture.clamp(64, 1024);
                    let max_tex = std::env::var("BO2MP_H1_MAX_TEX")
                        .ok()
                        .and_then(|v| v.trim().parse::<u32>().ok())
                        .filter(|n| n.is_power_of_two() && *n >= 64)
                        .unwrap_or(quality_cap);
                    let scene = Scene::load(&list).map(|mut s| {
                        s.images.max_size = max_tex;
                        s
                    });
                    match &scene {
                        Ok(s) => diag::info!(World, 
                            "h1 native: {} read in {:.1}s ({} draws, {} materials)",
                            s.name,
                            t.elapsed().as_secs_f32(),
                            s.draws.len(),
                            s.materials.len()
                        ),
                        Err(e) => {
                            diag::warn!(World, "h1 native: the scene did not read: {e}");
                            // (Releases the held loading screen.)
                            frame::settings::t7_native_load::set(
                                frame::settings::t7_native_load::FAILED,
                            );
                        }
                    }
                    let _ = tx.send(scene);
                });
            match spawned {
                Ok(_) => state.loading = Some(Mutex::new(rx)),
                Err(e) => {
                    diag::warn!(World, "h1 native: the load thread did not start: {e}");
                    frame::settings::t7_native_load::set(frame::settings::t7_native_load::FAILED);
                }
            }
        }
    }
    let received = state.loading.as_ref().map(|rx| {
        rx.lock()
            .map_or(Err(TryRecvError::Disconnected), |rx| rx.try_recv())
    });
    if let Some(received) = received {
        match received {
            Ok(Ok(scene)) => {
                let gpu = Gpu::from_parts(device.wgpu_device().clone(), (****queue).clone());
                let zones = state.zones.as_ref().map_or_else(Vec::new, |z| z.to_vec());
                use frame::settings::native_quality as nq;
                render_h1::gpu::set_anisotropy_cap(nq::current().anisotropy);
                let mut host = Host::new(gpu, scene, SCENE_DEPTH_FORMAT, zones);
                // (The scene's look notes: light set, map script, vision.)
                for n in host.renderer.scene.notes.iter().filter(|n| {
                    n.starts_with("h1 light") || n.starts_with("h1 shade") || n.starts_with("h1 map script") || n.starts_with("h1 vision") || n.starts_with("h1 fog")
                }) {
                    diag::info!(World, "h1 native look: {n}");
                }
                state.quality_rev = nq::revision();
                let line = host.apply_quality(nq::current());
                diag::info!(World, "{line}");
                state.host = Some(host);
                state.loading = None;
            }
            Ok(Err(_)) | Err(TryRecvError::Disconnected) => state.loading = None,
            Err(TryRecvError::Empty) => {}
        }
    }
    if state.host.is_none() {
        ACTIVE.store(false, Ordering::Relaxed);
        return;
    }
    if !state.saw_view {
        state.saw_view = true;
        diag::info!(World, "h1 native: first view reached the native draw");
    }
    let state = &mut *state;
    let Some(host) = state.host.as_mut() else {
        return;
    };
    let (target, depth, extracted) = view.into_inner();
    if !super::scene_depth::native_view_ok("h1 native", target, depth) {
        ACTIVE.store(false, Ordering::Relaxed);
        return;
    }
    let [_, _, width, height] = extracted.viewport.to_array();
    if width == 0 || height == 0 {
        ACTIVE.store(false, Ordering::Relaxed);
        return;
    }
    {
        use frame::settings::native_quality as nq;
        if nq::revision() != state.quality_rev {
            state.quality_rev = nq::revision();
            let line = host.apply_quality(nq::current());
            diag::info!(World, "{line}");
        }
    }
    // Take the frame's first-use clears (the next pass of the frame would
    // clear what is drawn here otherwise); this frame's world clears them.
    {
        let attachments = [Some(target.get_color_attachment())];
        let _pass = context.begin_tracked_render_pass(RenderPassDescriptor {
            label: Some("h1_native_take_clears"),
            color_attachments: &attachments,
            depth_stencil_attachment: Some(depth.get_attachment(StoreOp::Store)),
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
    }
    // The view as the game set it: IW world coordinates, a right-handed
    // view (x right, y up, looking down -z), reversed infinite depth.
    let world_from_view = extracted.world_from_view.to_matrix();
    let eye = extracted.world_from_view.translation();
    let view_from_world = world_from_view.inverse();
    let clip_from_view = extracted.clip_from_view;
    let clip_from_world = extracted
        .clip_from_world
        .unwrap_or(clip_from_view * view_from_world);
    let camera = Camera::from_rh(
        eye,
        view_from_world,
        clip_from_world,
        1.0 / clip_from_view.x_axis.x,
        1.0 / clip_from_view.y_axis.y,
        clip_from_view.w_axis.z,
    );
    // The engine's automatic exposure (`render_h1::meter`);
    // `BO2MP_H1_EXPOSURE=fixed` keeps the earlier fixed 1.0.
    static FIXED_EXPOSURE: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    let auto_exposure = !*FIXED_EXPOSURE.get_or_init(|| {
        std::env::var("BO2MP_H1_EXPOSURE").is_ok_and(|v| v.trim().eq_ignore_ascii_case("fixed"))
    });
    let look = Look {
        exposure: 1.0,
        fog: true,
        film: true,
        depth_range: Some(reverse_z_viewport_depth(GFX_DEPTH_RANGE_SCENE)),
        auto_exposure,
    };
    let depth_view = super::scene_depth::cached_depth_view(&mut state.depth_view, &depth.texture);
    if let Some(n) = host.sync_glass() {
        diag::info!(World, "bo2mp h1 glass: {n} panes broken");
    }
    let encoder = context.command_encoder();
    match host.record(
        encoder,
        &depth_view,
        depth.view(),
        target.main_texture_view(),
        target.main_texture_format(),
        width,
        height,
        &camera,
        &look,
    ) {
        Ok(()) => {
            ACTIVE.store(true, Ordering::Relaxed);
            for line in host.pending_log.drain(..) {
                diag::info!(World, "{}", line);
            }
            if host.frames == 1 {
                diag::info!(World, "{}", host.fps_line());
            }
            // The first native frame releases the loading screen.
            if frame::settings::t7_native_load::state() == frame::settings::t7_native_load::PENDING {
                frame::settings::t7_native_load::set(frame::settings::t7_native_load::READY);
            }
        }
        Err(e) => {
            diag::warn!(World, "h1 native: the frame was not drawn: {e}");
            if frame::settings::t7_native_load::state() == frame::settings::t7_native_load::PENDING {
                frame::settings::t7_native_load::set(frame::settings::t7_native_load::FAILED);
            }
            ACTIVE.store(false, Ordering::Relaxed);
        }
    }
}

pub(super) fn register(app: &mut App) {
    let Some(render_app) = app.get_sub_app_mut(bevy::render::RenderApp) else {
        return;
    };
    render_app.init_resource::<H1Native>().add_systems(
        Core3d,
        draw_h1_native
            .in_set(Core3dSystems::MainPass)
            .after(main_opaque_pass_3d)
            .before(super::draw::ExactColourDrawSet),
    );
}
