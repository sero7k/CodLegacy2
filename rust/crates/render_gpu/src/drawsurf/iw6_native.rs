//! bo2mp: a Ghosts map drawn by Ghosts' own renderer (`render_iw6`): every
//! surface and placed model with its material's own technique and Ghosts'
//! compiled shaders, the map's lightmaps, light grid, probes, sun and sky,
//! then Ghosts' own fog, iris and tone map (`postopaquefx_dfog`,
//! `postfx_macroclut`) into the frame.
//!
//! The map's zones are named as it loads (`asset_world::iw6_native_zones`;
//! `BO2MP_IW6_NATIVE=0` turns it off and the converted world draws); the
//! scene is read on a thread of its own and then drawn every frame, first
//! in the main pass: it clears the scene depth and leaves the world's depth
//! in it (in the scene's depth band), so players, the gun and effects drawn
//! after it sit in the world. On a Ghosts map the converted world, props
//! and sky are never built nor drawn (`active`), and Black Ops II's grade
//! does not run over Ghosts'. One `iw6 native:` line per map logs what was
//! drawn and what fell back.
//!
//! Players, the view gun and effects are graded by Ghosts' post as the world
//! is (`BO2MP_IW6_CHAR_GRADE`, on; `=0`: the old order, drawn over Ghosts'
//! finished picture and graded by nothing): the frame is drawn in stages.
//! Here the shadows and the opaque world go into Ghosts' HDR scene target
//! (`Host::record_scene`); the exact colour pass then draws its opaque
//! draws (players, the gun) into that target, runs Ghosts' fog,
//! colorization and blended world surfaces (`char_grade_mid` ->
//! `Host::record_fog`) and draws its blended draws (effects) into the
//! fogged target; `draw_iw6_post`, after it, runs Ghosts' bloom, iris and
//! final pass into the frame (`Host::record_post`).

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, TryRecvError, channel};
use std::sync::{Arc, Mutex};

use bevy::core_pipeline::core_3d::main_opaque_pass_3d;
use bevy::core_pipeline::{Core3d, Core3dSystems};
use bevy::prelude::*;
use bevy::render::render_resource::{RenderPassDescriptor, StoreOp, TextureView, TextureViewDescriptor};
use bevy::render::renderer::{RenderContext, RenderDevice, RenderQueue, ViewQuery};
use bevy::render::view::{ExtractedView, Msaa, ViewTarget};

use render_h1::code::Camera;
use render_h1::gpu::Gpu;
use render_iw6::host::{Host, Look};
use render_iw6::scene::Scene;

use super::depth_range::{GFX_DEPTH_RANGE_SCENE, reverse_z_viewport_depth};
use super::scene_depth::{SCENE_DEPTH_FORMAT, SceneDepthTexture};

/// Whether this frame's world was drawn by Ghosts' renderer.
static ACTIVE: AtomicBool = AtomicBool::new(false);
/// Whether the map is a Ghosts map drawn natively: Ghosts' renderer is its
/// only renderer, so the converted world, props and sky never draw on it,
/// even before the scene is ready (the loading screen is held until the
/// first native frame).
static IW6_MAP: AtomicBool = AtomicBool::new(false);

pub(crate) fn active() -> bool {
    ACTIVE.load(Ordering::Relaxed) || IW6_MAP.load(Ordering::Relaxed)
}

/// Whether a host (a read scene) is ready to draw: from the frame after it
/// is, the frame may be drawn in stages.
static HOST_READY: AtomicBool = AtomicBool::new(false);
/// Whether this frame's characters, gun and effects go into Ghosts' HDR
/// scene (latched once a frame before anything is prepared, so the exact
/// pass's pipelines and its targets agree for the whole frame).
static GRADE_FRAME: AtomicBool = AtomicBool::new(false);

/// `BO2MP_IW6_CHAR_GRADE=0`: players, the gun and effects drawn over
/// Ghosts' finished picture (the order before 2026-11-03), graded by
/// nothing.
pub(crate) fn char_grade_on() -> bool {
    // (Read once: `latch_char_grade` asks every frame.)
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| !std::env::var("BO2MP_IW6_CHAR_GRADE").is_ok_and(|v| v.trim() == "0"))
}

/// Whether this frame's exact colour pass draws into Ghosts' HDR scene
/// target: the switch, a ready host, and a single-sample view (the HDR
/// target has one sample).
pub(crate) fn char_grade_frame(samples: u32) -> bool {
    samples == 1 && GRADE_FRAME.load(Ordering::Relaxed)
}

/// The exact colour pass's target format for a view of `main` format and
/// `samples`: Ghosts' scene format on a frame drawn in stages.
pub(crate) fn exact_target_format(
    main: bevy::render::render_resource::TextureFormat,
    samples: u32,
) -> bevy::render::render_resource::TextureFormat {
    if char_grade_frame(samples) {
        render_iw6::world::SCENE_COLOUR
    } else {
        main
    }
}

fn latch_char_grade() {
    GRADE_FRAME.store(
        char_grade_on() && HOST_READY.load(Ordering::Relaxed) && IW6_MAP.load(Ordering::Relaxed),
        Ordering::Relaxed,
    );
}

#[derive(Resource, Default)]
pub(super) struct Iw6Native {
    /// The zones the scene below is (being) read from.
    zones: Option<Arc<[PathBuf]>>,
    /// The host being made on the loading thread (the scene read, then
    /// every pipeline and list made, off the render thread).
    loading: Option<Mutex<Receiver<Result<Host, String>>>>,
    host: Option<Host>,
    /// Whether the first view after the scene was ready was logged.
    saw_view: bool,
    /// Frames drawn (the coverage line is logged after the first ones).
    frames: u64,
    /// When the last frame was drawn (the iris adapts by the time between).
    last: Option<std::time::Instant>,
    /// The mover pose list last applied (`asset_world::iw6_mover_poses`).
    poses_seen: u64,
    /// The full view of the scene depth texture, kept while it is the same
    /// texture (a view made every frame is an allocation every frame).
    depth_view: Option<(bevy::render::render_resource::TextureId, TextureView)>,
}

impl Iw6Native {
    fn depth_full(&mut self, texture: &bevy::render::render_resource::Texture) -> TextureView {
        if self.depth_view.as_ref().is_none_or(|(id, _)| *id != texture.id()) {
            self.depth_view = Some((texture.id(), texture.create_view(&TextureViewDescriptor::default())));
        }
        match &self.depth_view {
            Some((_, v)) => v.clone(),
            None => texture.create_view(&TextureViewDescriptor::default()),
        }
    }
}

fn draw_iw6_native(
    view: ViewQuery<(&ViewTarget, &SceneDepthTexture, &ExtractedView, Option<&Msaa>)>,
    mut state: ResMut<Iw6Native>,
    device: Res<RenderDevice>,
    queue: Res<RenderQueue>,
    mut context: RenderContext,
) {
    // The map's zones: a new map starts a read of its scene.
    let zones = asset_world::iw6_native_zones();
    IW6_MAP.store(zones.is_some(), Ordering::Relaxed);
    let same = match (&zones, &state.zones) {
        (Some(a), Some(b)) => a[..] == b[..],
        (None, None) => true,
        _ => false,
    };
    if !same {
        state.host = None;
        state.loading = None;
        state.saw_view = false;
        state.frames = 0;
        state.last = None;
        state.zones = zones.clone();
        if let Some(z) = zones {
            diag::info!(
                World,
                "iw6 native: load begins ({} zones, first {})",
                z.len(),
                z.first().map_or(String::new(), |p| p.display().to_string())
            );
            let (tx, rx) = channel();
            let list = z.to_vec();
            let gpu = Gpu::from_parts(device.wgpu_device().clone(), (****queue).clone());
            let spawned = std::thread::Builder::new()
                .name("iw6_native_scene".into())
                .spawn(move || {
                    let t = std::time::Instant::now();
                    // The pictures' size cap: the graphics settings'
                    // (`native_quality`: ULTRA 2048, HIGH and MEDIUM 1024,
                    // LOW 512); `BO2MP_IW6_MAX_TEX=n` sets it.
                    let max_tex = std::env::var("BO2MP_IW6_MAX_TEX")
                        .ok()
                        .and_then(|v| v.trim().parse::<u32>().ok())
                        .filter(|n| n.is_power_of_two() && *n >= 64)
                        .unwrap_or(frame::settings::native_quality::current().max_texture.clamp(64, 16384));
                    // The models the map's scripts may spawn or swap in get hidden instances.
                    render_iw6::scene::set_pool_models(asset_world::iw6_scripts().map(|p| p.pool_models.clone()).unwrap_or_default());
                    let scene = Scene::load(&list).map(|mut s| {
                        s.images.max_size = max_tex;
                        s
                    });
                    match &scene {
                        Ok(s) => {
                            diag::info!(World,
                                "iw6 native: {} read in {:.1}s ({} draws, {} materials)",
                                s.name,
                                t.elapsed().as_secs_f32(),
                                s.draws.len(),
                                s.materials.len()
                            );
                            // The props, brush models and movers drawn, and
                            // what the map's own data drives.
                            for n in s.notes.iter().filter(|n| n.starts_with("iw6 props") || n.starts_with("iw6 map logic")) {
                                diag::info!(World, "iw6 native: {}: {n}", s.name);
                            }
                        }
                        Err(e) => {
                            diag::warn!(World, "iw6 native: the scene did not read: {e}");
                            // (Releases the held loading screen.)
                            frame::settings::t7_native_load::set(
                                frame::settings::t7_native_load::FAILED,
                            );
                        }
                    }
                    // The host: pipelines, lists and constant blocks made
                    // here, not on the render thread.
                    let host = scene.map(|scene| {
                        let t = std::time::Instant::now();
                        let h = Host::new(gpu, scene, SCENE_DEPTH_FORMAT, list);
                        diag::info!(World, "iw6 native: lists made in {:.1}s", t.elapsed().as_secs_f32());
                        h
                    });
                    let _ = tx.send(host);
                });
            match spawned {
                Ok(_) => state.loading = Some(Mutex::new(rx)),
                Err(e) => {
                    diag::warn!(World, "iw6 native: the load thread did not start: {e}");
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
            Ok(Ok(host)) => {
                state.host = Some(host);
                state.loading = None;
                state.poses_seen = 0;
            }
            Ok(Err(_)) | Err(TryRecvError::Disconnected) => state.loading = None,
            Err(TryRecvError::Empty) => {}
        }
    }
    HOST_READY.store(state.host.is_some(), Ordering::Relaxed);
    if state.host.is_none() {
        ACTIVE.store(false, Ordering::Relaxed);
        asset_world::set_iw6_decals_active(false);
        asset_world::set_iw6_distortion_active(false);
        asset_world::set_iw6_casters_active(false);
        asset_world::set_iw6_fx_grade(None);
        return;
    }
    if !state.saw_view {
        state.saw_view = true;
        diag::info!(World, "iw6 native: first view reached the native draw");
    }
    let (target, depth, extracted, msaa) = view.into_inner();
    let staged = char_grade_frame(msaa.map_or(1, Msaa::samples));
    if !super::scene_depth::native_view_ok("iw6 native", target, depth) {
        ACTIVE.store(false, Ordering::Relaxed);
        return;
    }
    let [_, _, width, height] = extracted.viewport.to_array();
    if width == 0 || height == 0 {
        ACTIVE.store(false, Ordering::Relaxed);
        return;
    }
    // Take the frame's first-use clears (the next pass of the frame would
    // clear what is drawn here otherwise); this frame's world clears them.
    {
        let attachments = [Some(target.get_color_attachment())];
        let _pass = context.begin_tracked_render_pass(RenderPassDescriptor {
            label: Some("iw6_native_take_clears"),
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
    let now = std::time::Instant::now();
    let dt = state.last.map(|t| (now - t).as_secs_f32().clamp(0.0, 1.0));
    state.last = Some(now);
    let look = Look {
        fog: true,
        depth_range: Some(reverse_z_viewport_depth(GFX_DEPTH_RANGE_SCENE)),
        // (The first frame settles the iris at once.)
        dt,
    };
    // The map's movers (doors) where the sim put them.
    let (seen, poses) = asset_world::iw6_mover_poses();
    if seen != state.poses_seen {
        state.poses_seen = seen;
        if let (Some(poses), Some(host)) = (poses, state.host.as_mut()) {
            for p in poses.iter() {
                let key = match (p.brush, &p.model) {
                    (Some(b), _) => render_iw6::scene::MoverKey::Brush(b),
                    (None, Some(m)) if p.slot.is_some() => render_iw6::scene::MoverKey::Spawned { name: m.clone(), slot: p.slot.unwrap_or(0) },
                    (None, m) => render_iw6::scene::MoverKey::Model {
                        name: m.clone().unwrap_or_default(),
                        origin: p.spawn_origin,
                    },
                };
                match &p.skin {
                    Some(skin) => host.set_mover_skinned(&key, p.origin, p.angles, p.hidden, skin),
                    None => host.set_mover_state(&key, p.origin, p.angles, p.hidden),
                };
            }
        }
    }
    let depth_view = state.depth_full(&depth.texture);
    let encoder = context.command_encoder();
    let Some(host) = state.host.as_mut() else {
        return;
    };
    // The effect lights the game's effect system made this frame
    // (`render_fx::iw6_lights`; the host lights the opaque scene with the
    // nearest few under the graphics preset's cap). A stack copy: nothing
    // is allocated.
    {
        let mut taken = asset_world::Iw6FxLights::default();
        asset_world::iw6_fx_lights_into(&mut taken);
        let mut lights = [render_iw6::fx_light::Light::default(); render_iw6::fx_light::MAX];
        for (to, from) in lights.iter_mut().zip(taken.as_slice()) {
            *to = render_iw6::fx_light::Light {
                origin: from.origin,
                radius: from.radius,
                color: from.color,
                dir: from.dir,
                cos_cone: from.cos_cone,
            };
        }
        host.set_fx_lights(&lights[..taken.as_slice().len().min(lights.len())]);
    }
    // The decals the effect system spawned since the last frame
    // (`render_fx::present`), cut out of the map's world (`render_iw6::decal`;
    // `BO2MP_IW6_DECALS=0` takes none). Stack arrays: nothing is allocated.
    {
        let on = render_iw6::decal::on();
        asset_world::set_iw6_decals_active(on);
        if on {
            let mut taken = asset_world::Iw6Decals::default();
            asset_world::take_iw6_decals(&mut taken);
            for s in taken.as_slice() {
                if let Some(name) = asset_world::iw6_decal_material(s.material) {
                    host.add_decal(&render_iw6::decal::Spawn {
                        origin: s.origin,
                        axis: s.axis,
                        radius: s.radius,
                        color: s.color,
                        life_ms: s.life_ms,
                        material: &name,
                    });
                }
            }
        }
    }
    // The distortion sprites the effect system built (`render_fx::present`),
    // drawn over a copy of the scene (`render_iw6::distort`;
    // `BO2MP_IW6_DISTORTION=0`: none). Stack arrays: nothing is allocated.
    {
        let on = render_iw6::distort::on();
        asset_world::set_iw6_distortion_active(on);
        if on {
            let mut taken = asset_world::Iw6DistortionQuads::default();
            asset_world::iw6_distortion_into(&mut taken);
            let names: [Option<std::sync::Arc<str>>; asset_world::IW6_DISTORTION_MAX] =
                std::array::from_fn(|i| taken.as_slice().get(i).and_then(|q| asset_world::iw6_decal_material(q.material)));
            let mut quads = [("", render_iw6::distort::Quad::default()); asset_world::IW6_DISTORTION_MAX];
            let mut n = 0;
            for (q, name) in taken.as_slice().iter().zip(&names) {
                if let Some(name) = name {
                    quads[n] = (name, render_iw6::distort::Quad { center: q.center, right: q.right, up: q.up, color: q.color });
                    n += 1;
                }
            }
            host.set_distortion(&quads[..n]);
        }
    }
    // The players' and gun's proxies for the sun shadow (`render_iw6::caster`;
    // `BO2MP_IW6_SUN_CASTERS=0`: none): the remote bodies and the view gun the
    // draw plan merge listed, and the local player's capsule under the eye
    // (standing eye height 60, INFERRED). Stack arrays: nothing is allocated.
    {
        let on = render_iw6::caster::on();
        asset_world::set_iw6_casters_active(on);
        let mut list = [render_iw6::caster::Caster::default(); render_iw6::caster::MAX];
        let mut n = 0;
        if on {
            let mut taken = asset_world::Iw6SunCasters::default();
            asset_world::iw6_casters_into(&mut taken);
            for c in taken.as_slice().iter().take(render_iw6::caster::MAX - 1) {
                list[n] = render_iw6::caster::Caster { a: c.a, b: c.b, radius: c.radius };
                n += 1;
            }
            let e = camera.eye;
            list[n] = render_iw6::caster::Caster::standing([e.x, e.y, e.z - 60.0], 70.0, 15.0);
            n += 1;
        }
        host.set_sun_casters(&list[..n]);
    }
    // (`BO2MP_IW6_PROFILE=1`: the effect sprites this frame's effects built.)
    if asset_world::iw6_fx_stats_wanted() {
        let f = asset_world::iw6_fx_stats();
        host.profile_fx(render_iw6::profile::FxFigures { sprites: f.sprites, visible: f.visible, overdraw: f.overdraw, biggest: f.biggest });
    }
    // Drawn in stages: the scene now, the fog in the exact colour pass
    // (or in `draw_iw6_post` when it draws nothing), the post after it.
    let drawn = if staged {
        host.record_scene(encoder, &depth_view, width, height, &camera, &look)
    } else {
        host.record(
            encoder,
            &depth_view,
            depth.view(),
            target.main_texture_view(),
            target.main_texture_format(),
            width,
            height,
            &camera,
            &look,
        )
    };
    match drawn {
        Ok(()) => {
            ACTIVE.store(true, Ordering::Relaxed);
            state.frames += 1;
            // bo2mp: the grade effects drawn after the final pass take
            // (`BO2MP_IW6_FX_GRADE`, on).
            if fx_grade_on()
                && let Some(h) = state.host.as_ref()
            {
                asset_world::set_iw6_fx_grade(Some(h.fx_grade()));
                if state.frames == 30 {
                    let g = h.fx_grade();
                    diag::info!(
                        World,
                        "iw6 fx grade: effects drawn after the native frame take its exposure {:.3} ({}), tone curve and film ({}) (BO2MP_IW6_FX_GRADE)",
                        g.exposure,
                        if h.exposure_read() { "read back from the iris" } else { "the iris's start, not read back yet" },
                        if g.film_on { "on" } else { "off" }
                    );
                }
            }
            if state.frames == 3
                && let Some(h) = state.host.as_ref()
            {
                diag::info!(World, "{}", h.coverage_line());
                diag::info!(World, "{}", h.fps_line());
            }
            if let Some(h) = state.host.as_mut() {
                // Graphics settings applied since (and what waits for the
                // next map); `BO2MP_IW6_PROFILE=1`: the frame profile every
                // five seconds.
                for n in h.quality_notes.drain(..) {
                    diag::info!(World, "{n}");
                }
                if let Some(l) = h.profile_line(render_h1::gpu_timer::shared_every()) {
                    diag::info!(World, "{l}");
                }
            }
            // The first native frame releases the loading screen.
            if frame::settings::t7_native_load::state() == frame::settings::t7_native_load::PENDING {
                frame::settings::t7_native_load::set(frame::settings::t7_native_load::READY);
            }
        }
        Err(e) => {
            diag::warn!(World, "iw6 native: the frame was not drawn: {e}");
            if frame::settings::t7_native_load::state() == frame::settings::t7_native_load::PENDING {
                frame::settings::t7_native_load::set(frame::settings::t7_native_load::FAILED);
            }
            ACTIVE.store(false, Ordering::Relaxed);
        }
    }
}

/// Where an opaque draw of a frame drawn in stages goes: the HDR scene
/// before the fog stage, the fogged scene after it (when the exact colour
/// pass ran the fog for its blended draws).
pub(super) fn staged_scene_target(state: &Iw6Native) -> Option<wgpu::TextureView> {
    let host = state.host.as_ref()?;
    match host.stage()? {
        render_iw6::host::Stage::Scene => host.scene_view().cloned(),
        render_iw6::host::Stage::Fogged => host.fogged_view().cloned(),
    }
}

/// The HDR scene target of a frame drawn in stages, while the opaque
/// characters may still be drawn into it (before `char_grade_mid`).
pub(super) fn char_grade_scene_view(state: &Iw6Native) -> Option<TextureView> {
    let host = state.host.as_ref()?;
    (host.stage() == Some(render_iw6::host::Stage::Scene))
        .then(|| host.scene_view().cloned().map(TextureView::from))
        .flatten()
}

/// Between the exact colour pass's opaque and blended draws on a frame
/// drawn in stages: Ghosts' depth copy, fog, colorization and blended
/// world surfaces (`Host::record_fog`), over the characters drawn so far.
/// The fogged target, which the blended draws (effects) then go into.
pub(super) fn char_grade_mid(
    state: &mut Iw6Native,
    encoder: &mut bevy::render::render_resource::CommandEncoder,
    depth: &SceneDepthTexture,
) -> Option<TextureView> {
    if state.host.as_ref()?.stage() == Some(render_iw6::host::Stage::Scene) {
        let depth_view = state.depth_full(&depth.texture);
        let host = state.host.as_mut()?;
        if let Err(e) = host.record_fog(encoder, &depth_view, depth.view()) {
            diag::warn!(World, "iw6 native: the fog stage was not drawn: {e}");
            return None;
        }
    }
    let host = state.host.as_ref()?;
    (host.stage() == Some(render_iw6::host::Stage::Fogged))
        .then(|| host.fogged_view().cloned().map(TextureView::from))
        .flatten()
}

/// The fogged HDR scene texture of a frame past its fog stage (the
/// distortion effects' resolved scene).
pub(super) fn char_grade_fogged(state: &Iw6Native) -> Option<&wgpu::Texture> {
    let host = state.host.as_ref()?;
    (host.stage() == Some(render_iw6::host::Stage::Fogged)).then(|| host.fogged()).flatten()
}

/// The last stage of a frame drawn in stages, after the exact colour pass:
/// the fog if that pass did not run it, then Ghosts' bloom, iris and final
/// pass into the frame.
fn draw_iw6_post(
    view: ViewQuery<(&ViewTarget, &SceneDepthTexture)>,
    mut state: ResMut<Iw6Native>,
    postfx: Option<Res<super::postfx::ExtractedPostFx>>,
    mut context: RenderContext,
) {
    let Some(host) = state.host.as_mut() else {
        return;
    };
    if host.stage().is_none() {
        return;
    }
    // The depth of field the game set (a script's, or the sights'): the eight
    // distances; the host decides whether the option row and the preset let
    // it draw.
    host.set_dof(postfx.as_ref().map(|p| p.frame.dof).filter(|d| d.active()).map(|d| {
        [d.view_model_start, d.view_model_end, d.near_start, d.near_end, d.far_start, d.far_end, d.near_blur, d.far_blur]
    }));
    let (target, depth) = view.into_inner();
    let encoder = context.command_encoder();
    let mut result = Ok(());
    if host.stage() == Some(render_iw6::host::Stage::Scene) {
        let depth_view = state.depth_full(&depth.texture);
        let Some(host) = state.host.as_mut() else { return };
        result = host.record_fog(encoder, &depth_view, depth.view());
    }
    let Some(host) = state.host.as_mut() else { return };
    if result.is_ok() {
        result = host.record_post(encoder, target.main_texture_view(), target.main_texture_format());
    }
    if let Err(e) = result {
        diag::warn!(World, "iw6 native: the post stage was not drawn: {e}");
        ACTIVE.store(false, Ordering::Relaxed);
    }
}

/// `BO2MP_IW6_FX_GRADE` (on): Ghosts' effects drawn after the native frame
/// take its grade (iris exposure, tone curve, film) on the CPU.
fn fx_grade_on() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var("BO2MP_IW6_FX_GRADE").map_or(true, |v| v.trim() != "0"))
}

pub(super) fn register(app: &mut App) {
    let Some(render_app) = app.get_sub_app_mut(bevy::render::RenderApp) else {
        return;
    };
    render_app
        .init_resource::<Iw6Native>()
        .add_systems(
            bevy::render::Render,
            latch_char_grade.in_set(bevy::render::RenderSystems::PrepareAssets),
        )
        .add_systems(
            Core3d,
            (
                draw_iw6_native
                    .in_set(Core3dSystems::MainPass)
                    .after(main_opaque_pass_3d)
                    .before(super::draw::ExactColourDrawSet),
                draw_iw6_post
                    .in_set(Core3dSystems::MainPass)
                    .after(super::draw::ExactColourDrawSet)
                    .after(super::draw::FallbackDrawSet),
            ),
        );
}
