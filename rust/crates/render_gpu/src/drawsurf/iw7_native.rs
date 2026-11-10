//! bo2mp: an Infinite Warfare map drawn by `render_iw7` (every surface and
//! placed model with its material's own technique, the game's programs
//! translated; log lines start `iw7 native:`), its only renderer then.
//! On by default (`BO2MP_IW7_NATIVE=0`: the converted world, for
//! comparison): the match load names the zone (`asset_world::iw7_native_request`)
//! and builds no converted world (`assets::lane::iw7::load_iw7_world_for`);
//! the map, its techsets and the host are made on a thread of their own
//! while the loading screen is held (`t7_native_load`), let go at the first
//! finished frame (or when the host fails, or after
//! [`LOAD_HOLD_LIMIT_SECS`]). Every frame records the map and hands the
//! display image and the reversed scene depth into the frame
//! (`render_t7::present`, as `jup_native` does), so players, the gun,
//! effects and the HUD drawn after it sit in the world.
//!
//! Unproven code constants are zero (`render_iw7::world::UNPROVEN`); the
//! display pass is a stand-in (exposure, Reinhard, sRGB).

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
use render_iw7::world::{Camera, Host};
use render_jup::present::Present;

use super::depth_range::{GFX_DEPTH_RANGE_SCENE, reverse_z_viewport_depth};
use super::scene_depth::{SCENE_DEPTH_FORMAT, SceneDepthTexture};

/// Whether this frame's world was drawn by IW7's techniques.
static ACTIVE: AtomicBool = AtomicBool::new(false);

pub(crate) fn active() -> bool {
    ACTIVE.load(Ordering::Relaxed)
}

#[derive(Resource, Default)]
struct Iw7Native {
    request: u64,
    loading: Option<Mutex<Receiver<Result<Host, String>>>>,
    host: Option<Host>,
    present: Option<Present>,
    started: Option<std::time::Instant>,
    frames: u64,
    /// The graphics quality revision the host last applied.
    quality_rev: u64,
}

/// How long the loading screen is held for the first native frame.
const LOAD_HOLD_LIMIT_SECS: f32 = 240.0;

/// Reads the map (with the common zones its materials and pictures come
/// from) and its techsets and makes the host (on a thread).
fn make_host(gpu: Gpu, zone: &std::path::Path) -> Result<Host, String> {
    // The pak ranges this map reads, kept once as a small pack
    // (`fastfile_iw7::pack`): the map can then be drawn offline, away from
    // the multi-gigabyte paks. `BO2MP_IW7_PACK=0`: not kept.
    let stem = zone.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    let packs = std::path::Path::new("codlegacy-artifacts").join("cache").join("iw7_packs");
    let recording = !std::env::var("BO2MP_IW7_PACK").is_ok_and(|v| v.trim() == "0") && dir_size(&packs) < PACK_BUDGET && fastfile_iw7::pack::start_recording(&packs, &stem);
    if recording {
        diag::info!(World, "iw7 native: keeping the pak ranges {stem} reads in {}", packs.display());
    }
    let host = make_host_inner(gpu, zone);
    if recording {
        fastfile_iw7::pack::stop_recording();
    }
    host
}

/// The most the kept range packs take on disk.
const PACK_BUDGET: u64 = 12 << 30;

fn dir_size(dir: &std::path::Path) -> u64 {
    std::fs::read_dir(dir).into_iter().flatten().flatten().filter_map(|e| e.metadata().ok()).map(|m| m.len()).sum()
}

fn make_host_inner(gpu: Gpu, zone: &std::path::Path) -> Result<Host, String> {
    let src = fastfile_iw7::map::MapSources::beside(zone).with_common();
    let map = fastfile_iw7::map::Iw7Map::load(&src)?;
    let tech = src.techsets.clone().ok_or("no techsets zone beside the map")?;
    let techsets = fastfile_iw7::Zone::open(&tech).map_err(|e| format!("{}: {e:?}", tech.display()))?;
    // The shared technique sets, whose programs and sets the map's
    // materials may name.
    let shared: Vec<fastfile_iw7::Zone> = render_iw7::world::shared_technique_zones(zone).iter().filter_map(|p| fastfile_iw7::Zone::open(p).ok()).collect();
    Host::new(gpu, &map, &techsets, &shared.iter().collect::<Vec<_>>())
}

fn load_iw7_native(mut state: ResMut<Iw7Native>, device: Res<RenderDevice>, queue: Res<RenderQueue>) {
    let (zone, request) = asset_world::iw7_native_request();
    if request != state.request {
        *state = Iw7Native { request, ..Default::default() };
        if let Some(z) = zone {
            state.started = Some(std::time::Instant::now());
            diag::info!(World, "iw7 native: load begins for {}", z.display());
            let gpu = Gpu::from_parts(device.wgpu_device().clone(), (****queue).clone());
            let (tx, rx) = channel();
            let z: PathBuf = z;
            let spawned = std::thread::Builder::new().name("iw7_native_scene".into()).spawn(move || {
                let host = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| make_host(gpu, &z))).unwrap_or_else(|_| Err("the host read panicked".into()));
                let _ = tx.send(host);
            });
            if spawned.is_ok() {
                state.loading = Some(Mutex::new(rx));
            } else {
                frame::settings::t7_native_load::set(frame::settings::t7_native_load::FAILED);
            }
        }
    }
    let received = state.loading.as_ref().map(|rx| rx.lock().map_or(Err(TryRecvError::Disconnected), |rx| rx.try_recv()));
    match received {
        Some(Ok(Ok(host))) => {
            let r = &host.report;
            diag::info!(
                World,
                "iw7 native: host made {:.1}s after the load began: {} of {} surfaces and {} model surfaces ({} models placed) drawn by {} pipelines, {} pictures ({} missing)",
                state.started.map_or(0.0, |t| t.elapsed().as_secs_f32()),
                r.drawn,
                r.surfaces,
                r.model_draws,
                r.models_placed,
                r.pipelines,
                r.pictures,
                r.pictures_missing
            );
            diag::info!(World, "iw7 native: host build took {}", r.seconds);
            for (why, n) in &r.skipped {
                diag::info!(World, "iw7 native: skipped {n}: {why}");
            }
            diag::info!(World, "iw7 native: code constants left at zero (index: passes): {:?}", r.zero_constants);
            use frame::settings::native_quality as nq;
            state.quality_rev = nq::revision();
            let mut host = host;
            let line = host.apply_quality(nq::current());
            diag::info!(World, "{line}");
            state.host = Some(host);
            state.loading = None;
        }
        Some(Ok(Err(e))) => {
            diag::warn!(World, "iw7 native: the host was not made: {e}");
            state.loading = None;
            frame::settings::t7_native_load::set(frame::settings::t7_native_load::FAILED);
        }
        Some(Err(TryRecvError::Disconnected)) => {
            state.loading = None;
            frame::settings::t7_native_load::set(frame::settings::t7_native_load::FAILED);
        }
        Some(Err(TryRecvError::Empty)) | None => {}
    }
    use frame::settings::t7_native_load as hold;
    if state.started.is_some() && hold::state() == hold::PENDING {
        let waited = state.started.map_or(0.0, |t| t.elapsed().as_secs_f32());
        if waited > LOAD_HOLD_LIMIT_SECS {
            diag::warn!(World, "iw7 native: giving up the wait after {waited:.0}s ({})", if state.host.is_some() { "no frame finished" } else { "the host is still being made" });
            hold::set(if state.host.is_some() { hold::READY } else { hold::FAILED });
        }
    }
}

fn draw_iw7_native(view: ViewQuery<(&ViewTarget, &SceneDepthTexture, &ExtractedView)>, mut state: ResMut<Iw7Native>, device: Res<RenderDevice>, queue: Res<RenderQueue>, post: Option<Res<super::postfx::ExtractedPostFx>>, mut context: RenderContext) {
    let state = &mut *state;
    let Some(host) = state.host.as_mut() else {
        ACTIVE.store(false, Ordering::Relaxed);
        return;
    };
    let (target, depth, extracted) = view.into_inner();
    if !super::scene_depth::native_view_ok("iw7 native", target, depth) {
        ACTIVE.store(false, Ordering::Relaxed);
        return;
    }
    let [vx, vy, width, height] = extracted.viewport.to_array();
    if width == 0 || height == 0 {
        ACTIVE.store(false, Ordering::Relaxed);
        return;
    }
    // The view as the game set it: IW world coordinates, reversed depth.
    let world_from_view = extracted.world_from_view.to_matrix();
    let eye = extracted.world_from_view.translation();
    let camera = Camera::from_view(eye, world_from_view.inverse(), extracted.clip_from_view, width, height);
    state.frames += 1;
    if state.frames == 1 {
        diag::info!(World, "iw7 native: first frame {width}x{height}, eye {eye:?}");
    }
    {
        use frame::settings::native_quality as nq;
        if nq::revision() != state.quality_rev {
            state.quality_rev = nq::revision();
            let line = host.apply_quality(nq::current());
            diag::info!(World, "{line}");
        }
    }
    // Black Ops II's depth of field (aiming down the sights), as the game
    // set it for this frame.
    host.dof_frame = post.as_ref().map_or_else(Default::default, |p| {
        let d = p.frame.dof;
        render_iw7::dof::Dof { near_start: d.near_start, near_end: d.near_end, near_blur: d.near_blur, far_start: d.far_start, far_end: d.far_end, far_blur: d.far_blur }
    });
    host.record(context.command_encoder(), &camera);
    if state.frames == 1 {
        let s = &host.stats;
        diag::info!(World, "iw7 fps: {}; draws {} of {} after culling, {} pipeline batches, {} bind groups, {} pipelines; occluders {:?} of {}", host.quality.preset.label(), s.draws, s.draws_total, s.pipeline_switches, s.group_switches, host.report.pipelines, s.occluders, s.occluders_total);
    }
    let (Some(image), Some(scene_depth)) = (host.final_image(), host.scene_depth()) else {
        ACTIVE.store(false, Ordering::Relaxed);
        return;
    };
    // Take the frame's first-use clears, then hand the image and depth in.
    {
        let attachments = [Some(target.get_color_attachment())];
        let _pass = context.begin_tracked_render_pass(RenderPassDescriptor {
            label: Some("iw7_native_take_clears"),
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
    }
    let Some(present) = state.present.as_ref() else {
        return;
    };
    let depth_view = depth.texture.create_view(&TextureViewDescriptor::default());
    present.record(context.command_encoder(), image, scene_depth, target.main_texture_view(), &depth_view, reverse_z_viewport_depth(GFX_DEPTH_RANGE_SCENE), [vx, vy, width, height]);
    ACTIVE.store(true, Ordering::Relaxed);
    if state.frames == 1 {
        diag::info!(World, "iw7 native: first finished frame {:.1}s after the load began", state.started.map_or(0.0, |t| t.elapsed().as_secs_f32()));
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
        .init_resource::<Iw7Native>()
        .add_systems(bevy::render::Render, load_iw7_native.in_set(bevy::render::RenderSystems::Prepare))
        .add_systems(Core3d, draw_iw7_native.in_set(Core3dSystems::MainPass).after(main_opaque_pass_3d).before(super::draw::ExactColourDrawSet));
}
