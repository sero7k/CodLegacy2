//! bo2mp: NVIDIA DLSS Super Resolution, behind the `dlss` cargo feature (off by
//! default: building it needs NVIDIA's DLSS SDK, which this repository does not
//! carry; see "Enabling DLSS" in `docs/RENDER.md`).
//!
//! Bevy 0.19's `dlss` feature is used for what only it can do: register the
//! Vulkan instance and device extensions DLSS needs before the renderer
//! starts (`DlssInitPlugin`, which `DefaultPlugins` adds, and which needs
//! the `DlssProjectId` resource: `bootstrap` inserts it). Bevy's own
//! `Dlss` camera component is not used: it wants bevy's depth and motion
//! prepasses, and this game draws its own (`motion.rs`, `scene_depth.rs`), so
//! `bootstrap` leaves bevy's `AntiAliasPlugin` out and the `dlss_wgpu` crate
//! is driven directly, with the same inputs FSR 3.1 takes (`temporal.rs`):
//! the lens's frame, its depth and motion vectors at the render size, the
//! jitter, and the finished frame at the window's size.
//!
//! Conventions are bevy's own DLSS node (`bevy_anti_alias` 0.19): the jitter
//! handed over is the negated offset, the motion vectors' scale is minus the
//! render size (pixels per UV unit, previous = current + mv * scale), the
//! depth is reverse Z, the motion vectors are at the render size, and the
//! exposure is DLSS's own. The quality presets are DLSS's own modes
//! (Quality 67%, Balanced 58%, Performance 50%, Ultra Performance 33%).
//!
//! Which neural network runs (DLSS 4's transformer model, DLSS 4.5's newer
//! one) is the driver's and the DLL's choice: `dlss_wgpu` 4.0 (DLSS SDK
//! 310.5.3) exposes no preset or model hint, so nothing here can ask for one.
//! The NVIDIA app's "DLSS Override - Model Preset: Latest" (or a newer
//! `nvngx_dlss.dll` beside the executable) is how the newest model is chosen.

use bevy::anti_alias::dlss::{DlssProjectId, DlssSuperResolutionSupported};
use bevy::asset::uuid::Uuid;
use bevy::prelude::*;
use bevy::render::RenderApp;
use bevy::render::renderer::raw_vulkan_init::AdditionalVulkanFeatures;
use bevy::render::renderer::{RenderAdapter, RenderContext, RenderDevice, RenderQueue};
use dlss_wgpu::super_resolution::{
    DlssSuperResolution, DlssSuperResolutionExposure, DlssSuperResolutionRenderParameters,
};
use dlss_wgpu::{DlssFeatureFlags, DlssPerfQualityMode, DlssSdk};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use super::temporal::{Inputs, TemporalGpu};

/// DLSS is up: the card, the driver and the runtime offered it and the SDK
/// started (the menu reads this through `frame::Upscaler`).
pub(super) static READY: AtomicBool = AtomicBool::new(false);
static BROKEN: AtomicBool = AtomicBool::new(false);

/// The project id the game gives DLSS (render world).
#[derive(Resource, Clone, Copy)]
pub(super) struct DlssProject(pub Uuid);

#[derive(Default)]
pub(super) struct DlssState {
    sdk: Option<Arc<Mutex<DlssSdk>>>,
    context: Option<(UVec2, frame::UpscaleQuality, DlssSuperResolution)>,
}

fn mode(quality: frame::UpscaleQuality) -> DlssPerfQualityMode {
    match quality {
        frame::UpscaleQuality::Quality => DlssPerfQualityMode::Quality,
        frame::UpscaleQuality::Balanced => DlssPerfQualityMode::Balanced,
        frame::UpscaleQuality::Performance => DlssPerfQualityMode::Performance,
        frame::UpscaleQuality::UltraPerformance => DlssPerfQualityMode::UltraPerformance,
    }
}

fn broken(why: &str) {
    if !BROKEN.swap(true, Ordering::Relaxed) {
        warn!("bo2mp upscale: DLSS failed ({why}); drawing the next best");
    }
    READY.store(false, Ordering::Relaxed);
}

/// Main world: hands the project id to the render world.
pub(super) fn register_main(app: &mut App) {
    let id = app.world().get_resource::<DlssProjectId>().map(|p| p.0);
    let Some(id) = id else {
        warn!("bo2mp upscale: DLSS: no DlssProjectId was given at start-up");
        return;
    };
    if let Some(render_app) = app.get_sub_app_mut(RenderApp) {
        render_app.insert_resource(DlssProject(id));
    }
}

/// Render start-up: the SDK, if the instance and device came up with DLSS's
/// extensions (`DlssInitPlugin`).
pub(super) fn init_sdk(
    mut gpu: ResMut<TemporalGpu>,
    device: Res<RenderDevice>,
    project: Option<Res<DlssProject>>,
    features: Option<Res<AdditionalVulkanFeatures>>,
) {
    let offered = features.is_some_and(|f| f.has::<DlssSuperResolutionSupported>());
    if !offered {
        info!("bo2mp upscale: DLSS: this card, driver or Vulkan instance did not offer it");
        return;
    }
    let Some(project) = project else {
        return;
    };
    match DlssSdk::new(project.0, device.wgpu_device().clone()) {
        Ok(sdk) => {
            gpu.dlss.sdk = Some(sdk);
            READY.store(true, Ordering::Relaxed);
            info!("bo2mp upscale: DLSS Super Resolution is available");
        }
        Err(error) => info!("bo2mp upscale: DLSS: the SDK did not start ({error:?})"),
    }
}

/// DLSS into `Inputs::output`; false when it could not (FSR 1 draws the frame).
pub(super) fn record(
    state: &mut DlssState,
    device: &RenderDevice,
    queue: &RenderQueue,
    adapter: &RenderAdapter,
    context: &mut RenderContext,
    inputs: &Inputs,
) -> bool {
    if BROKEN.load(Ordering::Relaxed) {
        return false;
    }
    let Some(sdk) = state.sdk.clone() else {
        return false;
    };
    let stale = state.context.as_ref().is_none_or(|(display, quality, _)| {
        *display != inputs.display || *quality != inputs.quality
    });
    if stale {
        // (Dropping the old one waits for the device to idle: only on a change.)
        state.context = None;
        let flags = DlssFeatureFlags::LowResolutionMotionVectors
            | DlssFeatureFlags::InvertedDepth
            | DlssFeatureFlags::AutoExposure;
        match DlssSuperResolution::new(
            inputs.display.to_array(),
            mode(inputs.quality),
            flags,
            sdk,
            device.wgpu_device(),
            queue,
        ) {
            Ok(made) => {
                info!(
                    "bo2mp upscale: DLSS, drawing {}x{} for a {}x{} window (DLSS asks {}x{})",
                    inputs.render.x,
                    inputs.render.y,
                    inputs.display.x,
                    inputs.display.y,
                    made.render_resolution()[0],
                    made.render_resolution()[1],
                );
                state.context = Some((inputs.display, inputs.quality, made));
            }
            Err(error) => {
                broken(&format!("{error:?}"));
                return false;
            }
        }
    }
    let Some((_, _, dlss)) = state.context.as_mut() else {
        return false;
    };
    let parameters = DlssSuperResolutionRenderParameters {
        color: &inputs.color_view,
        depth: &inputs.depth_view,
        motion_vectors: &inputs.motion_view,
        exposure: DlssSuperResolutionExposure::Automatic,
        bias: None,
        dlss_output: &inputs.output_view,
        reset: inputs.reset,
        jitter_offset: (-inputs.jitter).to_array(),
        partial_texture_size: Some(inputs.render.to_array()),
        motion_vector_scale: Some((-inputs.render.as_vec2()).to_array()),
    };
    match dlss.render(parameters, context.command_encoder(), adapter) {
        Ok(commands) => {
            context.add_command_buffer(commands);
            true
        }
        Err(error) => {
            broken(&format!("{error:?}"));
            false
        }
    }
}
