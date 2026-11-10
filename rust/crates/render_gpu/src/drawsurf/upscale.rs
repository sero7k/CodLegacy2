//! bo2mp: Options > UPSCALING (FSR 1). With it on, the match's camera (the
//! first-person lens) draws into a texture a share of the window's size
//! (RENDER QUALITY: 67 / 58 / 50 / 33 percent of the width and height), and
//! a camera of our own on the window enlarges that texture with AMD's FSR 1
//! (`upscale.wgsl`: EASU, then RCAS). Everything the lens's passes make (the
//! world, the weapon, bloom, depth of field, the match's own 2D drawing) is
//! drawn at the smaller size; the menus, the console and the HUD that bevy_ui
//! draws are on the window's own cameras and stay at its size.
//!
//! With it off (the default) nothing here touches the lens: the driver only
//! acts while the setting is on or a camera of ours is still standing.
//!
//! The temporal upscalers (`fsr3`, `dlss` in the `upscaler` setting) are in
//! `temporal.rs` (FSR 3.1) and `dlss.rs` (DLSS); this file drives the lens for
//! them (the same smaller target) and is what draws when they cannot: FSR 1.
//! Their input, at the lens's size: a Halton(2,3) sub-pixel jitter on the lens
//! projection (`drive_jitter`, below), the motion-vector target the main
//! geometry passes write (`motion.rs`) and the scene depth (`scene_depth.rs`).
//! A choice this machine cannot run (`frame::Upscaler::effective`: FSR 3.1
//! needs Vulkan, DLSS an NVIDIA RTX card and a build with the `dlss` feature)
//! draws as the next one down, with a line in the log.
//!
//! Not here, and why: frame generation (DLSS FG, FSR 3 / 4 FG: they take over
//! the swap chain and want motion vectors too). See `docs/RENDER.md`.

use bevy::camera::{ClearColorConfig, RenderTarget};
use bevy::core_pipeline::{Core2d, Core2dSystems};
use bevy::prelude::*;
use bevy::render::RenderStartup;
use bevy::render::extract_component::{ExtractComponent, ExtractComponentPlugin};
use bevy::render::extract_resource::{ExtractResource, ExtractResourcePlugin};
use bevy::render::render_asset::RenderAssets;
use bevy::render::render_resource::binding_types::{texture_2d, uniform_buffer_sized};
use bevy::render::render_resource::{
    BindGroupEntries, BindGroupLayoutDescriptor, BindGroupLayoutEntries, Buffer, BufferDescriptor,
    BufferUsages, CachedRenderPipelineId, ColorTargetState, ColorWrites, Extent3d, FragmentState,
    LoadOp, Operations, PipelineCache, RenderPassColorAttachment, RenderPassDescriptor,
    RenderPipeline, RenderPipelineDescriptor, ShaderStages, StoreOp, Texture, TextureDescriptor,
    TextureDimension, TextureFormat, TextureId, TextureSampleType, TextureUsages, TextureView,
    TextureViewDescriptor, VertexState,
};
use bevy::render::renderer::{RenderContext, RenderDevice, RenderQueue, ViewQuery};
use bevy::render::texture::GpuImage;
use bevy::render::camera::TemporalJitter;
use bevy::render::view::{Msaa, ViewTarget};
use bevy::shader::Shader;
use bevy::window::PrimaryWindow;
use std::collections::HashMap;
use std::num::NonZeroU64;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};

const SHADER_PATH: &str = "embedded://render_gpu/drawsurf/upscale.wgsl";
const PARAMS_BYTES: u64 = 32;
/// The lens draws first, our camera second; the window's other cameras (menus,
/// console, loading) have orders from -1 up.
const LENS_ORDER: isize = -10;
const UPSCALE_ORDER: isize = -9;
/// What the lens is drawn into: sRGB, so bevy's own last step writes the
/// frame as it would to the window, and read back below as plain `Unorm` (the
/// gamma-encoded values FSR wants).
const LENS_FORMAT: TextureFormat = TextureFormat::Rgba8UnormSrgb;
const READ_FORMAT: TextureFormat = TextureFormat::Rgba8Unorm;
const MIN_SIDE: u32 = 64;
/// A window under this on a side (minimised, mid-resize) is not upscaled.
const MIN_WINDOW_SIDE: u32 = 32;
/// The freeze watchdog: an upscaler that has been on this long (and this many
/// frames) without one frame written to the window is switched off.
const WATCHDOG_SECS: f32 = 5.0;
const WATCHDOG_FRAMES: u32 = 120;

/// Frames the upscale path wrote to the window (render world), read by the
/// watchdog in `drive_upscale`.
static DRAWN: AtomicU64 = AtomicU64::new(0);
/// The decisions of the frame path, each logged when it changes.
static DECISIONS: Mutex<Vec<(&'static str, String)>> = Mutex::new(Vec::new());

pub(super) fn note_drawn() {
    DRAWN.fetch_add(1, Ordering::Relaxed);
}

/// Logs `what` for `key` once, and again only when the text changes: which
/// camera draws the world, into what, whether the upscale ran or why not.
pub(super) fn decision(key: &'static str, what: String) {
    let Ok(mut all) = DECISIONS.lock() else {
        return;
    };
    match all.iter_mut().find(|(k, _)| *k == key) {
        Some((_, last)) if *last == what => {}
        Some((_, last)) => {
            info!("bo2mp upscale: {key}: {what}");
            *last = what;
        }
        None => {
            info!("bo2mp upscale: {key}: {what}");
            all.push((key, what));
        }
    }
}

/// The last reason the render world gave for not drawing.
fn last_skip() -> String {
    DECISIONS
        .lock()
        .ok()
        .and_then(|all| {
            all.iter()
                .rev()
                .find(|(k, _)| k.starts_with("skipped"))
                .map(|(k, v)| format!("{k}: {v}"))
        })
        .unwrap_or_else(|| "no reason given (the pass never ran)".into())
}

/// The size the lens draws at for a window of `output` pixels and a share
/// `scale` of each side; `None` when that is the window's own size (no point).
pub(crate) fn internal_size(output: UVec2, scale: f32) -> Option<UVec2> {
    if !scale.is_finite()
        || scale >= 0.99
        || output.x < MIN_WINDOW_SIDE
        || output.y < MIN_WINDOW_SIDE
    {
        return None;
    }
    let side = |full: u32| ((full as f32 * scale).round() as u32).clamp(MIN_SIDE.min(full), full);
    let size = UVec2::new(side(output.x), side(output.y));
    (size != output).then_some(size)
}

/// RCAS's strength for the SHARPNESS slider (0..1): 0 is off, 1 the sharpest
/// FSR allows (0 stops of attenuation), 0.8 about 0.4 stops, as AMD's
/// samples; each tenth below is a fifth of a stop weaker.
pub(crate) fn rcas_strength(sharpness: f32) -> f32 {
    if !sharpness.is_finite() || sharpness <= 0.001 {
        return 0.0;
    }
    (-2.0 * (1.0 - sharpness.clamp(0.0, 1.0))).exp2()
}

fn method_name(method: frame::Upscaler) -> &'static str {
    match method {
        frame::Upscaler::Off => "off",
        frame::Upscaler::Fsr1 => "FSR 1",
        frame::Upscaler::Fsr3 => "FSR 3.1",
        frame::Upscaler::Dlss => "DLSS",
    }
}

/// Marks our camera on the window (main and render world).
#[derive(Component, Clone, ExtractComponent)]
pub(super) struct UpscaleCamera;

/// What the render world draws this frame: the lens's image, and the strength.
#[derive(Resource, Clone, Default, ExtractResource)]
pub(super) struct UpscaleFrame {
    pub(super) image: Option<AssetId<Image>>,
    /// FSR 1's RCAS strength.
    pub(super) strength: f32,
    /// The SHARPNESS slider as it is (0..1): FSR 3.1's sharpening.
    pub(super) sharpness: f32,
    /// What draws the frame (the choice, or what it falls back to).
    pub(super) method: frame::Upscaler,
    pub(super) quality: frame::UpscaleQuality,
}

#[derive(Resource, Default)]
struct UpscaleDriver {
    image: Option<Handle<Image>>,
    size: UVec2,
    camera: Option<Entity>,
    /// Reported once per change.
    logged: Option<(UVec2, UVec2)>,
    /// The choice and what it draws as, last reported.
    logged_method: Option<(frame::Upscaler, frame::Upscaler)>,
    /// The freeze watchdog: the write count last seen, since when it has not
    /// moved (seconds of real time, frames).
    watch: Option<(u64, f32, u32)>,
}

/// The camera jitter of this frame, in the main and render worlds: what a
/// temporal upscaler is told (`jitter_offset`) and what the lens projection
/// has been moved by. `pixels` is in FSR's convention, the offset of the
/// sample inside the pixel (x right, y down, -0.5..0.5), the same value as
/// bevy's `TemporalJitter::offset`; the projection is shifted by
/// `(2 x / width, -2 y / height)` in clip space (`jitter_projection`).
#[derive(Resource, Clone, Debug, Default, PartialEq, ExtractResource)]
pub(crate) struct UpscaleJitter {
    /// Whether a temporal upscaler is on and the lens is jittered.
    pub active: bool,
    pub pixels: Vec2,
    /// Which of the `phases` this is (the frame count modulo them).
    pub index: u32,
    pub phases: u32,
    /// The lens's size and the window's.
    pub render: UVec2,
    pub display: UVec2,
    /// Counts the times the temporal history must be dropped (a new map, a
    /// teleport or respawn, see `temporal::view_jumped`); the render world
    /// resets when it differs from what it last saw.
    pub resets: u32,
    /// The last frame's time in milliseconds (the upscalers' `frame_time_delta`).
    pub frame_ms: f32,
}

/// The `index`th term (from 1) of the Halton sequence in `base`, 0..1.
pub(crate) fn halton(mut index: u32, base: u32) -> f32 {
    let (mut f, mut r) = (1.0f32, 0.0f32);
    while index > 0 {
        f /= base as f32;
        r += f * (index % base) as f32;
        index /= base;
    }
    r
}

/// FSR's jitter phase count: 8 times the square of the upscale ratio
/// (display over render width), whole. (1.0 gives 8, 2.0 gives 32.)
pub(crate) fn jitter_phase_count(render_width: u32, display_width: u32) -> u32 {
    if render_width == 0 || display_width == 0 {
        return 8;
    }
    let ratio = display_width as f32 / render_width as f32;
    ((8.0 * ratio * ratio) as u32).max(1)
}

/// The jitter of frame `frame` in pixels, -0.5..0.5 (Halton 2 and 3, as
/// FidelityFX's `GetJitterOffset`).
pub(crate) fn jitter_offset(frame: u32, phases: u32) -> Vec2 {
    let index = frame % phases.max(1) + 1;
    Vec2::new(halton(index, 2) - 0.5, halton(index, 3) - 0.5)
}

/// The upscaler a frame can really use. The BO3 and MWR native passes
/// (`t7_native`, `h1_native`, `jup_native`) have no jitter and no motion
/// vectors, so a temporal upscaler (FSR 3.1, DLSS) would be fed garbage on a
/// native view: it draws as FSR 1 there (said once).
pub(crate) fn for_native_world(method: frame::Upscaler, native: bool) -> frame::Upscaler {
    if native && method.wants_temporal() {
        static SAID: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
        if !SAID.swap(true, Ordering::Relaxed) {
            info!(
                "bo2mp upscale: {} needs jitter and motion vectors, which the native renderer's passes (BO3, MWR) do not have: this view draws as FSR 1",
                method_name(method)
            );
        }
        return frame::Upscaler::Fsr1;
    }
    method
}

/// The multisampling the lens must have, when it has another (the native
/// passes and every upscaler need one sample per pixel).
pub(crate) fn lens_msaa_fix(current: Msaa) -> Option<Msaa> {
    (current != render_scene::LENS_MSAA).then_some(render_scene::LENS_MSAA)
}

/// Keeps the lens at `Msaa::Off` whatever was configured: FSR wants a
/// one-sample input and the native passes write one-sample colour, while
/// bevy sizes the view's colour and depth by this component.
fn enforce_lens_msaa(
    settings: Option<Res<frame::GameSettings>>,
    mut lenses: Query<&mut Msaa, With<render_scene::FpvLens>>,
    mut said: Local<bool>,
) {
    for mut msaa in &mut lenses {
        if let Some(fixed) = lens_msaa_fix(*msaa) {
            if !*said {
                *said = true;
                warn!(
                    "bo2mp upscale: the lens camera had Msaa {:?} (aa_samples {}); {:?} is forced, the native passes and upscalers need one sample",
                    *msaa,
                    settings.as_ref().map_or(0, |s| s.aa_samples),
                    fixed
                );
            }
            *msaa = fixed;
        }
    }
}

fn lens_image(size: UVec2) -> Image {
    let mut image = Image::new_target_texture(size.x, size.y, LENS_FORMAT, None);
    image.texture_descriptor.view_formats = &[READ_FORMAT];
    image
}

#[allow(clippy::too_many_arguments)]
fn drive_upscale(
    mut commands: Commands,
    settings: Option<Res<frame::GameSettings>>,
    windows: Query<&Window, With<PrimaryWindow>>,
    mut lenses: Query<(&mut Camera, &mut RenderTarget), With<render_scene::FpvLens>>,
    standing: Query<Entity, With<UpscaleCamera>>,
    mut images: ResMut<Assets<Image>>,
    mut driver: ResMut<UpscaleDriver>,
    mut out: ResMut<UpscaleFrame>,
    time: Res<Time<Real>>,
) {
    let plan = settings
        .as_ref()
        .filter(|s| s.upscaler.effective().active())
        .and_then(|s| {
            let window = windows.single().ok()?;
            let output = window.physical_size();
            let size = internal_size(output, s.upscale_quality.scale())?;
            Some((output, size, rcas_strength(s.upscale_sharpness)))
        });
    let lens = lenses.single_mut().ok();
    let lens_present = lens.is_some();
    let (Some((output, size, strength)), Some((mut camera, mut target))) = (plan, lens) else {
        // Off, or no match: give the lens back to the window.
        frame::Upscaler::Off.note_in_use();
        if driver.image.is_none() && driver.camera.is_none() && standing.is_empty() {
            return;
        }
        frame::upscale_guard::end(&diag::profile_dir());
        driver.watch = None;
        decision(
            "frame path",
            format!(
                "upscaling is off (setting {}, effective {}, lens {}): the lens camera draws to the window again at order 0, the upscale camera is gone",
                settings.as_ref().map_or("?", |s| s.upscaler.key()),
                settings
                    .as_ref()
                    .map_or("?", |s| s.upscaler.effective().key()),
                if lens_present { "present" } else { "absent" }
            ),
        );
        driver.logged_method = None;
        if let Ok((mut camera, mut target)) = lenses.single_mut() {
            if !matches!(*target, RenderTarget::Window(_)) {
                *target = RenderTarget::default();
            }
            if camera.order == LENS_ORDER {
                camera.order = 0;
            }
        }
        for entity in &standing {
            commands.entity(entity).despawn();
        }
        driver.camera = None;
        driver.image = None;
        driver.size = UVec2::ZERO;
        driver.logged = None;
        *out = UpscaleFrame::default();
        return;
    };
    if driver.image.is_none() || driver.size != size {
        driver.image = Some(images.add(lens_image(size)));
        driver.size = size;
    }
    let Some(handle) = driver.image.clone() else {
        return;
    };
    if !matches!(&*target, RenderTarget::Image(t) if t.handle == handle) {
        *target = RenderTarget::Image(handle.clone().into());
    }
    if camera.order != LENS_ORDER {
        camera.order = LENS_ORDER;
    }
    if driver.camera.is_none() || standing.is_empty() {
        driver.camera = Some(
            commands
                .spawn((
                    Camera2d,
                    Camera {
                        order: UPSCALE_ORDER,
                        // (Black, not "none": if the pass ever fails to
                        // draw, black says so; a stale picture hides it.)
                        clear_color: ClearColorConfig::Custom(Color::BLACK),
                        ..default()
                    },
                    Msaa::Off,
                    UpscaleCamera,
                ))
                .id(),
        );
    }
    let requested = settings.as_ref().map(|s| s.upscaler).unwrap_or_default();
    let method = for_native_world(requested.effective(), super::native_world());
    method.note_in_use();
    frame::upscale_guard::begin(
        &diag::profile_dir(),
        &format!(
            "{} {}x{} in a {}x{} window",
            method_name(method),
            size.x,
            size.y,
            output.x,
            output.y
        ),
    );
    if driver.logged != Some((size, output)) {
        decision(
            "frame path",
            format!(
                "{} draws the world into a {}x{} image (lens order {}); upscale camera {:?} on the window (order {}) enlarges it to {}x{}",
            method_name(method),
            size.x,
            size.y,
            camera.order,
            driver.camera,
            UPSCALE_ORDER,
            output.x,
            output.y
        ),
        );
    }
    // The freeze watchdog: the picture on screen is the upscale pass's, so if
    // it has written no frame for a few seconds, whatever the reason, the
    // upscaler goes off and the lens draws to the window again.
    let drawn = DRAWN.load(Ordering::Relaxed);
    let now = time.elapsed_secs();
    let (seen, since, frames) = driver.watch.get_or_insert((drawn, now, 0));
    if *seen != drawn {
        *seen = drawn;
        *since = now;
        *frames = 0;
    } else {
        *frames += 1;
        if now - *since >= WATCHDOG_SECS && *frames >= WATCHDOG_FRAMES {
            error!(
                "bo2mp upscale: {} has written no frame to the window for {:.1} s ({} frames): the screen would be frozen. Turning upscaling off for this run. Last decision of the pass: {}",
                method_name(method),
                now - *since,
                *frames,
                last_skip()
            );
            frame::Upscaler::Fsr1.mark_broken();
            driver.watch = None;
            return;
        }
    }
    if driver.logged != Some((size, output)) {
        info!(
            "bo2mp upscale: {} at {}x{} for a {}x{} window",
            method_name(method),
            size.x,
            size.y,
            output.x,
            output.y
        );
        driver.logged = Some((size, output));
    }
    if driver.logged_method != Some((requested, method)) {
        driver.logged_method = Some((requested, method));
        if method != requested {
            info!(
                "bo2mp upscale: {} is not available on this machine; drawing {}",
                method_name(requested),
                method_name(method)
            );
        }
    }
    let sharpness = settings.as_ref().map_or(0.0, |s| s.upscale_sharpness);
    let quality = settings
        .as_ref()
        .map(|s| s.upscale_quality)
        .unwrap_or_default();
    let image = Some(handle.id());
    if out.image != image
        || out.strength != strength
        || out.sharpness != sharpness
        || out.method != method
        || out.quality != quality
    {
        *out = UpscaleFrame {
            image,
            strength,
            sharpness,
            method,
            quality,
        };
    }
}

/// Jitters the lens while a temporal upscaler is chosen (bevy then moves the
/// view uniform's projection, `clip_from_world`, and keeps the unjittered
/// `unjittered_clip_from_world` for the motion vectors), and takes the
/// jitter off again when it is not. Nothing is touched while none is.
#[allow(clippy::too_many_arguments)]
fn drive_jitter(
    mut commands: Commands,
    settings: Option<Res<frame::GameSettings>>,
    windows: Query<&Window, With<PrimaryWindow>>,
    mut lenses: Query<
        (Entity, Option<&mut TemporalJitter>, &Transform),
        With<render_scene::FpvLens>,
    >,
    time: Res<Time<Real>>,
    mut out: ResMut<UpscaleJitter>,
    mut frame_count: Local<u32>,
    // (How many history resets so far, and the lens, eye and forward of the frame before.)
    mut history: Local<(u32, Option<(Entity, Vec3, Vec3)>)>,
) {
    let plan = settings
        .as_ref()
        .filter(|s| {
            for_native_world(s.upscaler.effective(), super::native_world()).wants_temporal()
        })
        .and_then(|s| {
            let display = windows.single().ok()?.physical_size();
            let render = internal_size(display, s.upscale_quality.scale()).unwrap_or(display);
            Some((render, display))
        });
    let lens = lenses.single_mut().ok();
    let (Some((render, display)), Some((entity, current, transform))) = (plan, lens) else {
        // (Not wanted, or no match: take the jitter off.)
        for (entity, jitter, _) in &lenses {
            if jitter.is_some() {
                commands.entity(entity).remove::<TemporalJitter>();
            }
        }
        history.1 = None;
        if out.active {
            *out = UpscaleJitter::default();
        }
        return;
    };
    // (A new lens is a new map; a jump of the eye or a snap of the aim is a
    // teleport or a respawn: the old picture is no use to the history.)
    let eye = transform.translation;
    let forward = *transform.forward();
    let previous = history
        .1
        .filter(|(e, ..)| *e == entity)
        .map(|(_, eye, forward)| (eye, forward));
    if super::temporal::view_jumped(previous, eye, forward) {
        history.0 = history.0.wrapping_add(1);
    }
    history.1 = Some((entity, eye, forward));
    let frame_ms = (time.delta_secs() * 1000.0).clamp(1.0, 1000.0);
    let phases = jitter_phase_count(render.x, display.x);
    let index = *frame_count % phases;
    *frame_count = frame_count.wrapping_add(1);
    let pixels = jitter_offset(index, phases);
    match current {
        Some(mut jitter) => jitter.offset = pixels,
        None => {
            commands
                .entity(entity)
                .insert(TemporalJitter { offset: pixels });
        }
    }
    *out = UpscaleJitter {
        active: true,
        pixels,
        index,
        phases,
        render,
        display,
        resets: history.0,
        frame_ms,
    };
}

#[derive(Resource)]
pub(super) struct UpscaleGpu {
    layout: BindGroupLayoutDescriptor,
    shader: Handle<Shader>,
    easu: Option<CachedRenderPipelineId>,
    rcas: HashMap<TextureFormat, CachedRenderPipelineId>,
    params: Option<Buffer>,
    /// The lens texture read as `READ_FORMAT`.
    read: Option<(TextureId, TextureView)>,
    /// The enlarged frame before sharpening, for the output size it was made at.
    wide: Option<(UVec2, Texture, TextureView)>,
}

fn init(mut commands: Commands, asset_server: Res<AssetServer>) {
    commands.insert_resource(UpscaleGpu {
        layout: BindGroupLayoutDescriptor::new(
            "bo2mp_upscale",
            &BindGroupLayoutEntries::with_indices(
                ShaderStages::FRAGMENT,
                (
                    (
                        0,
                        texture_2d(TextureSampleType::Float { filterable: false }),
                    ),
                    (
                        1,
                        uniform_buffer_sized(false, NonZeroU64::new(PARAMS_BYTES)),
                    ),
                ),
            ),
        ),
        shader: asset_server.load(SHADER_PATH),
        easu: None,
        rcas: HashMap::new(),
        params: None,
        read: None,
        wide: None,
    });
}

fn pipeline(
    gpu: &UpscaleGpu,
    entry: &'static str,
    format: TextureFormat,
) -> RenderPipelineDescriptor {
    RenderPipelineDescriptor {
        label: Some(format!("bo2mp_upscale_{entry}").into()),
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
            entry_point: Some(entry.into()),
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
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) fn draw_upscale(
    view: ViewQuery<(&ViewTarget, &UpscaleCamera)>,
    frame: Res<UpscaleFrame>,
    images: Res<RenderAssets<GpuImage>>,
    gpu: Option<ResMut<UpscaleGpu>>,
    cache: Res<PipelineCache>,
    device: Res<RenderDevice>,
    queue: Res<RenderQueue>,
    mut temporal: ResMut<super::temporal::TemporalOutcome>,
    mut context: RenderContext,
) {
    // (FSR 3.1 or DLSS drew this frame: nothing left for FSR 1.)
    if std::mem::take(&mut temporal.drawn) {
        note_drawn();
        decision("temporal pass", "drew the frame (FSR 1 stood down)".into());
        return;
    }
    let Some(mut gpu) = gpu else {
        decision("skipped (FSR 1)", "UpscaleGpu resource missing".into());
        return;
    };
    let (target, _) = view.into_inner();
    let Some(source) = frame.image.and_then(|id| images.get(id)) else {
        decision(
            "skipped (FSR 1)",
            format!(
                "the lens image is not on the GPU yet (frame has image {:?})",
                frame.image
            ),
        );
        return;
    };
    let src_size = source.texture.size();
    let out_size = target.main_texture().size();
    let (src, out) = (
        UVec2::new(src_size.width, src_size.height),
        UVec2::new(out_size.width, out_size.height),
    );
    if src.x == 0 || src.y == 0 || out.x == 0 || out.y == 0 {
        decision("skipped (FSR 1)", format!("zero size: lens {src}, window {out}"));
        return;
    }
    let dst_format = target.main_texture_format();

    if gpu.easu.is_none() {
        let p = pipeline(&gpu, "easu", READ_FORMAT);
        gpu.easu = Some(cache.queue_render_pipeline(p));
    }
    if !gpu.rcas.contains_key(&dst_format) {
        let p = pipeline(&gpu, "rcas", dst_format);
        let id = cache.queue_render_pipeline(p);
        gpu.rcas.insert(dst_format, id);
    }
    let (Some(easu), Some(rcas)): (Option<&RenderPipeline>, Option<&RenderPipeline>) = (
        gpu.easu.and_then(|id| cache.get_render_pipeline(id)),
        gpu.rcas
            .get(&dst_format)
            .and_then(|&id| cache.get_render_pipeline(id)),
    ) else {
        // (The shaders are still being made: this frame shows the window's black.)
        let state = |id: Option<CachedRenderPipelineId>| {
            id.map_or("not queued".to_string(), |id| {
                format!("{:?}", cache.get_render_pipeline_state(id))
            })
        };
        decision(
            "skipped (FSR 1)",
            format!(
                "pipelines not ready: easu {}, rcas {}",
                state(gpu.easu),
                state(gpu.rcas.get(&dst_format).copied())
            ),
        );
        return;
    };

    if gpu
        .read
        .as_ref()
        .is_none_or(|(id, _)| *id != source.texture.id())
    {
        let view = source.texture.create_view(&TextureViewDescriptor {
            label: Some("bo2mp_upscale_read"),
            format: Some(READ_FORMAT),
            ..Default::default()
        });
        gpu.read = Some((source.texture.id(), view));
    }
    if gpu.wide.as_ref().is_none_or(|(size, ..)| *size != out) {
        let texture = device.create_texture(&TextureDescriptor {
            label: Some("bo2mp_upscale_wide"),
            size: Extent3d {
                width: out.x,
                height: out.y,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: TextureDimension::D2,
            format: READ_FORMAT,
            usage: TextureUsages::RENDER_ATTACHMENT | TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let view = texture.create_view(&TextureViewDescriptor::default());
        gpu.wide = Some((out, texture, view));
    }
    let params = gpu
        .params
        .get_or_insert_with(|| {
            device.create_buffer(&BufferDescriptor {
                label: Some("bo2mp_upscale_params"),
                size: PARAMS_BYTES,
                usage: BufferUsages::UNIFORM | BufferUsages::COPY_DST,
                mapped_at_creation: false,
            })
        })
        .clone();
    let data: [f32; 8] = [
        src.x as f32,
        src.y as f32,
        out.x as f32,
        out.y as f32,
        frame.strength,
        if dst_format.is_srgb() { 1.0 } else { 0.0 },
        0.0,
        0.0,
    ];
    queue.write_buffer(&params, 0, bytemuck::cast_slice(&data));

    let (Some((_, read)), Some((_, _, wide))) = (gpu.read.as_ref(), gpu.wide.as_ref()) else {
        decision("skipped (FSR 1)", "read or wide view missing".into());
        return;
    };
    let layout = cache.get_bind_group_layout(&gpu.layout);
    let easu_bind = device.create_bind_group(
        "bo2mp_upscale_easu",
        &layout,
        &BindGroupEntries::with_indices(((0, read), (1, params.as_entire_binding()))),
    );
    let rcas_bind = device.create_bind_group(
        "bo2mp_upscale_rcas",
        &layout,
        &BindGroupEntries::with_indices(((0, wide), (1, params.as_entire_binding()))),
    );
    let post = target.post_process_write();
    let encoder = context.command_encoder();
    let mut run = |label: &'static str,
                   pipe: &RenderPipeline,
                   bind: &bevy::render::render_resource::BindGroup,
                   to: &TextureView| {
        let mut pass = encoder.begin_render_pass(&RenderPassDescriptor {
            label: Some(label),
            color_attachments: &[Some(RenderPassColorAttachment {
                view: to,
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
        pass.set_pipeline(pipe);
        pass.set_bind_group(0, bind, &[]);
        pass.draw(0..3, 0..1);
    };
    run("bo2mp_upscale_easu", easu, &easu_bind, wide);
    run("bo2mp_upscale_rcas", rcas, &rcas_bind, post.destination);
    note_drawn();
    decision(
        "FSR 1 pass",
        format!("ran EASU + RCAS, {src} -> {out}, window format {dst_format:?}"),
    );
}

pub(super) fn register(app: &mut App) {
    let Some(_) = app.get_sub_app(bevy::render::RenderApp) else {
        return;
    };
    bevy::asset::embedded_asset!(app, "upscale.wgsl");
    app.init_resource::<UpscaleDriver>()
        .init_resource::<UpscaleFrame>()
        .init_resource::<UpscaleJitter>()
        .add_plugins((
            ExtractComponentPlugin::<UpscaleCamera>::default(),
            ExtractResourcePlugin::<UpscaleFrame>::default(),
            ExtractResourcePlugin::<UpscaleJitter>::default(),
        ))
        .add_systems(Update, (enforce_lens_msaa, drive_upscale, drive_jitter).chain());
    let Some(render_app) = app.get_sub_app_mut(bevy::render::RenderApp) else {
        return;
    };
    render_app
        .add_systems(RenderStartup, init)
        .add_systems(Core2d, draw_upscale.in_set(Core2dSystems::PostProcess));
    super::temporal::register(app);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_lens_msaa_is_forced_off() {
        assert_eq!(lens_msaa_fix(Msaa::Off), None);
        assert_eq!(lens_msaa_fix(Msaa::Sample4), Some(Msaa::Off));
        assert_eq!(lens_msaa_fix(Msaa::default()), Some(Msaa::Off));
    }

    #[test]
    fn temporal_upscalers_draw_as_fsr1_on_native_views() {
        use frame::Upscaler::*;
        assert_eq!(for_native_world(Fsr3, true), Fsr1);
        assert_eq!(for_native_world(Dlss, true), Fsr1);
        assert_eq!(for_native_world(Fsr3, false), Fsr3);
        assert_eq!(for_native_world(Fsr1, true), Fsr1);
        assert_eq!(for_native_world(Off, true), Off);
    }

    #[test]
    fn presets_draw_the_share_of_each_side() {
        let out = UVec2::new(2560, 1440);
        assert_eq!(internal_size(out, 0.67), Some(UVec2::new(1715, 965)));
        assert_eq!(internal_size(out, 0.58), Some(UVec2::new(1485, 835)));
        assert_eq!(internal_size(out, 0.50), Some(UVec2::new(1280, 720)));
        assert_eq!(internal_size(out, 0.33), Some(UVec2::new(845, 475)));
        // (Whole size, or nothing to draw: no upscaling.)
        assert_eq!(internal_size(out, 1.0), None);
        assert_eq!(internal_size(UVec2::ZERO, 0.5), None);
        assert_eq!(internal_size(out, f32::NAN), None);
        // (A small window never goes under a side of 64.)
        assert_eq!(
            internal_size(UVec2::new(100, 80), 0.33),
            Some(UVec2::new(64, 64))
        );
    }

    #[test]
    fn sharpness_maps_to_rcas_stops() {
        assert_eq!(rcas_strength(0.0), 0.0);
        assert_eq!(rcas_strength(f32::NAN), 0.0);
        assert_eq!(rcas_strength(1.0), 1.0);
        assert!((rcas_strength(0.8) - 0.757).abs() < 0.01);
        assert!((rcas_strength(0.5) - 0.5).abs() < 1e-6);
        assert!(rcas_strength(0.6) > rcas_strength(0.5));
    }

    #[test]
    fn halton_is_the_radical_inverse() {
        let base2: Vec<f32> = (1..=4).map(|i| halton(i, 2)).collect();
        assert_eq!(base2, vec![0.5, 0.25, 0.75, 0.125]);
        assert!((halton(1, 3) - 1.0 / 3.0).abs() < 1e-6);
        assert!((halton(2, 3) - 2.0 / 3.0).abs() < 1e-6);
        assert!((halton(4, 3) - (1.0 / 3.0 + 1.0 / 9.0)).abs() < 1e-6);
    }

    #[test]
    fn phase_count_follows_the_upscale_ratio() {
        assert_eq!(jitter_phase_count(2560, 2560), 8);
        assert_eq!(jitter_phase_count(1280, 2560), 32);
        // (Quality 67%: ratio 1.49, 17 phases.)
        assert_eq!(jitter_phase_count(1715, 2560), 17);
        assert_eq!(jitter_phase_count(0, 2560), 8);
    }

    #[test]
    fn jitter_stays_inside_the_pixel_and_repeats() {
        for phases in [8, 17, 32] {
            for frame in 0..phases * 2 {
                let j = jitter_offset(frame, phases);
                assert!(j.x.abs() < 0.5 && j.y.abs() < 0.5, "{j:?}");
                assert_eq!(j, jitter_offset(frame % phases, phases));
            }
        }
        // (Frame 0 is Halton index 1: (0.5, 1/3) less one half.)
        let first = jitter_offset(0, 8);
        assert!(first.x.abs() < 1e-6 && (first.y + 1.0 / 6.0).abs() < 1e-6);
        // (No two phases of a cycle are the same point.)
        let seen: Vec<Vec2> = (0..8).map(|f| jitter_offset(f, 8)).collect();
        for (i, a) in seen.iter().enumerate() {
            for b in &seen[i + 1..] {
                assert_ne!(a, b);
            }
        }
    }

    /// The shader parses and validates (the pipelines are only made on a GPU).
    #[test]
    fn shader_entry_points_are_there() {
        let source = include_str!("upscale.wgsl");
        for entry in ["fn vertex(", "fn easu(", "fn rcas("] {
            assert!(source.contains(entry), "{entry}");
        }
    }
}
