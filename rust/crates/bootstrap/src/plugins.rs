use assets::AssetPlugin;
use audio::AudioPlugin;
use bevy::{
    log::LogPlugin,
    prelude::*,
    render::{
        RenderPlugin as BevyRenderPlugin,
        pipelined_rendering::PipelinedRenderingPlugin,
        settings::{RenderCreation, WgpuFeatures, WgpuSettings},
    },
};
use bots::BotsPlugin;
use console::ConsolePlugin;
use frame::RuntimeRole;
use hud::HudPlugin;
use net::NetPlugin;
use render::RenderPlugin;
use replay::ReplayPlugin;
use session::SessionPlugin;
use ui::UiPlugin;

/// Bevy's log lines go to stderr on the log writer's thread, never from the
/// frame that logged them (a write Windows held up stalled it for seconds).
fn log_lines(_: &mut App) -> Option<bevy::log::BoxedFmtLayer> {
    Some(Box::new(
        bevy::log::tracing_subscriber::fmt::Layer::default().with_writer(diag::stderr_writer),
    ))
}

/// What a wgpu error the code did not catch does. Bevy's default quits the
/// game on any (a debug pass's invalid query set on Skyjacked ended the
/// match with nothing in the run log). Validation and internal errors are
/// written to the run log and the game goes on; out of memory and a lost
/// device are written to the run log and the game ends, as before (there is
/// no device left to draw with). `CODLEGACY_RENDER_ERRORS=quit`: every error
/// quits (bevy's rule).
fn render_error_policy(
    error: &bevy::render::error_handler::RenderError,
    main_world: &mut World,
    _render_world: &mut World,
) -> bevy::render::error_handler::RenderErrorPolicy {
    use bevy::render::error_handler::{ErrorType, RenderErrorPolicy};
    use std::sync::atomic::{AtomicU32, Ordering};
    static LOGGED: AtomicU32 = AtomicU32::new(0);
    let quit_all =
        codlegacy_env::var("CODLEGACY_RENDER_ERRORS").is_ok_and(|v| v.eq_ignore_ascii_case("quit"));
    let fatal = quit_all || matches!(error.ty, ErrorType::OutOfMemory | ErrorType::DeviceLost);
    let n = LOGGED.fetch_add(1, Ordering::Relaxed);
    if fatal {
        let upscaler = frame::Upscaler::in_use();
        if upscaler.active() {
            // (The upscaler's "running" marker stays, so the next start has it Off.)
            diag::error!(World, "render: the device failed while an upscaler ({}) was drawing; the next start has upscaling Off", upscaler.key());
        }
        diag::error!(World, "render: {:?} error, the game ends: {}", error.ty, error.description);
        diag::flush();
        diag::keep_crash_copy();
        main_world.write_message(AppExit::error());
        RenderErrorPolicy::StopRendering
    } else {
        // (The first 32 in full; then one line in 256, as the same error
        // usually repeats every frame.)
        if n < 32 || n % 256 == 0 {
            diag::error!(World, "render: {:?} error #{} (not fatal; the frame goes on): {}", error.ty, n + 1, error.description);
        }
        // bo2mp: an error that names one of the upscaler's passes switches that
        // upscaler off for the run (FSR 3.1 to FSR 1, FSR 1 to Off), so a bad
        // pass does not drop a frame after frame.
        if let Some(tripped) = frame::Upscaler::trip_for_render_error(&error.description) {
            diag::error!(World, "bo2mp upscale: {} caused a render error ({}); it is switched off for this run", tripped.key(), error.description);
        }
        RenderErrorPolicy::Ignore
    }
}

pub fn add_runtime_plugins(app: &mut App) {
    add_runtime_plugins_with_role(app, RuntimeRole::Listen);
}

pub fn add_runtime_plugins_with_role(app: &mut App, role: RuntimeRole) {
    let net = match role {
        RuntimeRole::Listen => NetPlugin::listen(),
        RuntimeRole::Dedicated => NetPlugin::dedicated(),
        RuntimeRole::Client => NetPlugin::client(),
        RuntimeRole::Replay => NetPlugin::replay(),
    };
    if crate::launch::test_window_hidden() {
        // A hidden test run: about 30 updates a second, whatever the focus
        // (the server keeps its own 20 Hz), so it costs the PC little.
        let pace = bevy::winit::UpdateMode::reactive_low_power(std::time::Duration::from_millis(33));
        app.insert_resource(bevy::winit::WinitSettings {
            focused_mode: pace,
            unfocused_mode: pace,
        });
    } else if crate::bench::enabled() || codlegacy_env::var_os("CODLEGACY_WINDOW_POS").is_some() {
        // Unfocused benchmarks (and test runs on another monitor) must not
        // inherit the window runner's 60 Hz sleep.
        app.insert_resource(bevy::winit::WinitSettings::continuous());
    } else if !frame::Headless::requested() && !app.world().contains_resource::<frame::Headless>() {
        // bo2mp: the window he plays in runs flat out, focused or not. Test
        // games are hidden or headless now and keep their own pace above, so
        // a background cap only slowed his game: with another window focused
        // it dropped to 30 fps, which read as "laggier towards the end of the
        // match". CODLEGACY_BACKGROUND_FPS still caps it when set (0 for no limit).
        let fps = codlegacy_env::var("CODLEGACY_BACKGROUND_FPS")
            .ok()
            .and_then(|v| v.trim().parse::<u32>().ok())
            .unwrap_or(0);
        if fps > 0 {
            diag::info!(
                Launch,
                "window: {fps} fps while another window has focus (CODLEGACY_BACKGROUND_FPS)"
            );
        }
        app.insert_resource(bevy::winit::WinitSettings {
            focused_mode: bevy::winit::UpdateMode::Continuous,
            unfocused_mode: match fps {
                0 => bevy::winit::UpdateMode::Continuous,
                fps => bevy::winit::UpdateMode::reactive_low_power(std::time::Duration::from_secs_f64(1.0 / fps as f64)),
            },
        });
    }
    app.add_plugins(AssetPlugin)
        .add_plugins(UiPlugin)
        .add_plugins(ConsolePlugin)
        .add_plugins(net)
        .add_plugins((BotsPlugin, HudPlugin))
        .add_plugins(AudioPlugin)
        .add_plugins(ReplayPlugin)
        .add_plugins(RenderPlugin)
        .add_plugins(SessionPlugin);
    // A wgpu error the code did not catch must not end his match (bevy's
    // default handler quits on any; Skyjacked, 2026-10-26).
    app.insert_resource(bevy::render::error_handler::RenderErrorHandler(render_error_policy));

    app.edit_schedule(Update, |schedule| {
        schedule.set_executor(bevy::ecs::schedule::SingleThreadedExecutor::new());
    });

    app.edit_schedule(First, |schedule| {
        schedule.set_executor(bevy::ecs::schedule::SingleThreadedExecutor::new());
    });
    app.edit_schedule(PreUpdate, |schedule| {
        schedule.set_executor(bevy::ecs::schedule::SingleThreadedExecutor::new());
    });
    app.edit_schedule(PostUpdate, |schedule| {
        schedule.set_executor(bevy::ecs::schedule::SingleThreadedExecutor::new());
    });
    app.edit_schedule(Last, |schedule| {
        schedule.set_executor(bevy::ecs::schedule::SingleThreadedExecutor::new());
    });

    if let Some(render_app) = app.get_sub_app_mut(bevy::render::RenderApp) {
        render_app.edit_schedule(bevy::render::renderer::RenderGraph, |schedule| {
            schedule.set_executor(bevy::ecs::schedule::SingleThreadedExecutor::new());
        });
        render_app.edit_schedule(bevy::core_pipeline::Core3d, |schedule| {
            schedule.set_executor(bevy::ecs::schedule::SingleThreadedExecutor::new());
        });
        render_app.edit_schedule(bevy::core_pipeline::Core2d, |schedule| {
            schedule.set_executor(bevy::ecs::schedule::SingleThreadedExecutor::new());
        });
        render_app.edit_schedule(bevy::render::Render, |schedule| {
            schedule.set_executor(bevy::ecs::schedule::SingleThreadedExecutor::new());
        });
        render_app.edit_schedule(bevy::render::ExtractSchedule, |schedule| {
            schedule.set_executor(bevy::ecs::schedule::SingleThreadedExecutor::new());
        });
    }
}

pub fn assemble_listen_app() -> App {
    let mut app = App::new();
    app.add_plugins(default_plugins_with_quiet_log(WindowPlugin {
        primary_window: None,
        ..default()
    }));
    add_runtime_plugins(&mut app);
    app
}

pub fn default_plugins_with_quiet_log(mut window: WindowPlugin) -> bevy::app::PluginGroupBuilder {
    let headless = frame::Headless::requested();
    if headless {
        window.primary_window = None;
        window.exit_condition = bevy::window::ExitCondition::DontExit;
    }
    if let Some(primary) = window.primary_window.as_mut() {
        primary.desired_maximum_frame_latency = core::num::NonZeroU32::new(frame_latency());
    }
    let mut wgpu = WgpuSettings::default();
    wgpu.features |= WgpuFeatures::TEXTURE_FORMAT_16BIT_NORM
        | WgpuFeatures::TEXTURE_COMPRESSION_BC
        | WgpuFeatures::POLYGON_MODE_LINE
        | WgpuFeatures::TEXTURE_BINDING_ARRAY
        | WgpuFeatures::SAMPLED_TEXTURE_AND_STORAGE_BUFFER_ARRAY_NON_UNIFORM_INDEXING
        | WgpuFeatures::PARTIALLY_BOUND_BINDING_ARRAY;
    // bo2mp: the MWR (h1) native lane runs MWR's own pixel shaders, and the
    // world ones bind up to ~26 sampled textures and more than 16 samplers in
    // one stage. The wgpu default (16) made those pipelines fail validation:
    // the world was skipped (dark maps) and Shipment died on its first
    // native frame. Every desktop Vulkan/DX12 device allows far more.
    // `BO2MP_DEFAULT_LIMITS=1` keeps the old defaults.
    if !std::env::var("BO2MP_DEFAULT_LIMITS").is_ok_and(|v| frame::upscale_guard::truthy(&v)) {
        wgpu.limits.max_sampled_textures_per_shader_stage = 64;
        wgpu.limits.max_samplers_per_shader_stage = 32;
    }
    // bo2mp: FSR 3.1 (`wgpu-ffx`) hands its SPIR-V compute shaders to the driver
    // as they are when the device has PASSTHROUGH_SHADERS (every Vulkan one
    // does). `BO2MP_FSR3_NAGA=1` leaves that feature off so wgpu validates and
    // re-emits them first: the diagnosis for a driver that dies on them.
    if std::env::var("BO2MP_FSR3_NAGA").is_ok_and(|v| frame::upscale_guard::truthy(&v)) {
        wgpu.disabled_features = Some(WgpuFeatures::PASSTHROUGH_SHADERS);
    }
    // bo2mp: the card's memory comes in 8-64 MB blocks instead of 128-256 MB
    // ones, so a game holds less it does not use (8 games share one card).
    // CODLEGACY_GPU_MEMORY=performance brings the big blocks back.
    if !codlegacy_env::var("CODLEGACY_GPU_MEMORY")
        .is_ok_and(|v| v.trim().eq_ignore_ascii_case("performance"))
    {
        wgpu.memory_hints = bevy::render::settings::MemoryHints::MemoryUsage;
    }
    if headless {
        // No backends: bevy makes no graphics device and no render world.
        wgpu.backends = None;
    }
    if !headless && let Some(backends) = chosen_backends() {
        wgpu.backends = Some(backends);
    }
    let plugins = DefaultPlugins
        .set(window)
        .set(log_plugin())
        .set(BevyRenderPlugin {
            render_creation: RenderCreation::Automatic(Box::new(wgpu)),
            ..default()
        });
    // bo2mp: DLSS (cargo feature `dlss`): bevy's DlssInitPlugin, which the
    // default group adds, needs the project id before it builds. Bevy's own
    // anti-aliasing plugin (and with it its DLSS camera component) is left out:
    // render_gpu drives DLSS itself (drawsurf/dlss.rs).
    #[cfg(feature = "dlss")]
    let plugins = plugins
        .add_before::<LogPlugin>(DlssProjectIdPlugin)
        .disable::<bevy::anti_alias::AntiAliasPlugin>();
    if headless {
        // No window loop, sound device or gamepads: a plain loop paced like a
        // hidden test run (about 30 updates a second; the server keeps 20 Hz).
        // With no render world bevy also leaves out its entity sync, yet the
        // components it syncs still report their removal to it: a despawned
        // camera or mesh would panic. Its queue is never drained here; the
        // records are a few bytes each.
        return plugins
            .disable::<bevy::winit::WinitPlugin>()
            .disable::<bevy::audio::AudioPlugin>()
            .disable::<bevy::gilrs::GilrsPlugin>()
            .disable::<PipelinedRenderingPlugin>()
            .add(bevy::render::sync_world::SyncWorldPlugin)
            .add(bevy::app::ScheduleRunnerPlugin::run_loop(
                std::time::Duration::from_millis(33),
            ));
    }
    let plugins = plugins.add(BackendLogPlugin);
    if pipelined_rendering() {
        plugins
    } else {
        plugins.disable::<PipelinedRenderingPlugin>()
    }
}

/// bo2mp: Graphics > GRAPHICS API. `None` keeps wgpu's own choice (all
/// backends, `WGPU_BACKEND` read by `WgpuSettings::default()`): AUTO, a set
/// `WGPU_BACKEND` (testing), or a chosen API that finds no adapter on this
/// machine (logged; the game starts anyway). Decided once per run.
fn chosen_backends() -> Option<bevy::render::settings::Backends> {
    use bevy::render::settings::Backends;
    use frame::GraphicsApi;
    static CHOICE: std::sync::OnceLock<Option<Backends>> = std::sync::OnceLock::new();
    *CHOICE.get_or_init(|| {
        if std::env::var_os("WGPU_BACKEND").is_some() {
            return None;
        }
        let api = console::early_graphics_api();
        let backends = match api {
            GraphicsApi::Auto => return auto_backends(),
            GraphicsApi::Vulkan => Backends::VULKAN,
            GraphicsApi::Dx12 => Backends::DX12,
            // bo2mp: wgpu's OpenGL backend has none of the texture-array
            // features every device is asked for (TEXTURE_BINDING_ARRAY,
            // non-uniform indexing, partially bound arrays, 16-bit norm
            // formats): asking a GL device for them panicked before the
            // window opened, and the setting kept the game from starting
            // again. AUTO instead; `WGPU_BACKEND=gl` still forces it.
            GraphicsApi::OpenGl => {
                diag::warn!(
                    Launch,
                    "render: graphics API OPENGL cannot run the game's renderers (no texture arrays); using AUTO"
                );
                return auto_backends();
            }
        };
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends,
            ..wgpu::InstanceDescriptor::new_without_display_handle()
        });
        if bevy::tasks::block_on(instance.enumerate_adapters(backends)).is_empty() {
            diag::warn!(
                Launch,
                "render: graphics API {} has no adapter on this machine; using AUTO",
                api.label()
            );
            return None;
        }
        diag::info!(Launch, "render: graphics API {} asked for in the settings", api.label());
        Some(backends)
    })
}

/// bo2mp: AUTO without starting backends the game never uses. bevy's own
/// default is every backend: wgpu then opens an OpenGL context on a hidden
/// window (the "WGL Instance Thread"; OpenGL cannot run the renderers, see
/// above) and makes a DX12 device for each card to list it, which loads the
/// card's DX12 driver files off the drive, all before the window opens, and
/// slowest on a busy drive. A card Vulkan lists as a discrete GPU is what the
/// old AUTO chose anyway (Vulkan comes first and a discrete card wins), so
/// then Vulkan alone; without one, Vulkan and DX12 as before (no OpenGL).
/// `CODLEGACY_ALL_BACKENDS=1` keeps bevy's default (every backend).
fn auto_backends() -> Option<bevy::render::settings::Backends> {
    use bevy::render::settings::Backends;
    if codlegacy_env::var("CODLEGACY_ALL_BACKENDS").is_ok_and(|v| v.trim() == "1") {
        return None;
    }
    let started = std::time::Instant::now();
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
        backends: Backends::VULKAN,
        ..wgpu::InstanceDescriptor::new_without_display_handle()
    });
    let discrete = bevy::tasks::block_on(instance.enumerate_adapters(Backends::VULKAN))
        .iter()
        .any(|adapter| adapter.get_info().device_type == wgpu::DeviceType::DiscreteGpu);
    drop(instance);
    let backends = if discrete { Backends::VULKAN } else { Backends::VULKAN | Backends::DX12 };
    diag::info!(
        Launch,
        "render: AUTO uses {} ({} discrete card found in {:.2} s; CODLEGACY_ALL_BACKENDS=1 starts every backend)",
        if discrete { "Vulkan" } else { "Vulkan and DX12" },
        if discrete { "Vulkan" } else { "no Vulkan" },
        started.elapsed().as_secs_f64()
    );
    Some(backends)
}

/// bo2mp: one log line once the renderer is up: the backend and adapter.
struct BackendLogPlugin;

impl Plugin for BackendLogPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            Update,
            |info: Option<Res<bevy::render::renderer::RenderAdapterInfo>>,
             adapter: Option<Res<bevy::render::renderer::RenderAdapter>>,
             windows: Query<&bevy::window::Window, With<bevy::window::PrimaryWindow>>,
             mut done: Local<bool>| {
                if *done {
                    return;
                }
                if let Some(info) = info {
                    *done = true;
                    let i = &info.0;
                    // bo2mp: the card's memory lowers the quality the
                    // renderers apply (pictures, shadow maps) before the
                    // first map is made (`native_quality::Vram`).
                    if let Some(adapter) = adapter.as_deref() {
                        let vram = render_gpu::drawsurf::vram::probe(&adapter.0, i);
                        diag::info!(Launch, "render: {}", vram.line());
                        frame::settings::native_quality::set_vram(vram);
                        diag::info!(
                            Launch,
                            "render: native quality now {}",
                            frame::settings::native_quality::current().summary()
                        );
                    }
                    // (The driver and present mode too: a slow frame on a
                    // shared GPU or a vsync cap shows here first.)
                    let present = windows.iter().next().map_or_else(|| "no window".to_owned(), |w| format!("{:?}", w.present_mode));
                    diag::info!(
                        Launch,
                        "render: backend {:?}, adapter {} ({:?}, vendor {:#06x} device {:#06x}), driver {} {}, present mode {present} (CODLEGACY_PRESENT_MODE; the backend: WGPU_BACKEND or the settings' graphics API)",
                        i.backend,
                        i.name,
                        i.device_type,
                        i.vendor,
                        i.device,
                        i.driver,
                        i.driver_info
                    );
                }
            },
        );
    }
}

fn log_plugin() -> LogPlugin {
    #[cfg(feature = "span-stats")]
    return LogPlugin {
        filter: crate::span_stats::FILTER.into(),
        level: bevy::log::Level::WARN,
        custom_layer: crate::span_stats::layer,
        fmt_layer: log_lines,
        ..default()
    };
    #[cfg(not(feature = "span-stats"))]
    LogPlugin {
        filter: "warn,codlegacy=info".into(),
        level: bevy::log::Level::WARN,
        fmt_layer: log_lines,
        ..default()
    }
}

const PIPELINED_RENDERING_ENV: &str = "CODLEGACY_PIPELINED_RENDERING";

/// Overlap rendering with the next main frame. Extraction remains the ownership
/// boundary; the bounded render channel permits one outstanding frame.
/// Set CODLEGACY_PIPELINED_RENDERING=0 for synchronous presentation.
///
/// Off by default on macOS: AppKit only lets the main thread touch the NSView
/// behind the Metal surface. Bevy hands `create_surfaces` back to the main
/// thread through the multi-threaded executor, which the single-threaded
/// `Render` schedule above bypasses, so the render thread would create it and
/// panic in `raw-window-metal`.
fn pipelined_rendering() -> bool {
    match codlegacy_env::var_os(PIPELINED_RENDERING_ENV) {
        None => !cfg!(target_os = "macos"),
        Some(_) => perf::switch(PIPELINED_RENDERING_ENV),
    }
}

const FRAME_LATENCY_ENV: &str = "CODLEGACY_FRAME_LATENCY";

/// How many frames the surface is asked to let the CPU run ahead of the GPU.
///
/// wgpu calls this a hint and the backend is free to clamp it: on Vulkan it is
/// bound to the number of swapchain images, so a run that asked for two did
/// not necessarily get two, and only a measurement says which. Three is the
/// default because it is Black Ops II's: its renderer presents with the sync
/// interval and never sets a frame latency, so D3D's own default of three
/// holds. The variable exists so the
/// other arm needs no rebuild, and the manifest records the number that was
/// asked for — never the number the driver granted, which this process cannot
/// read back.
///
/// The value is a count, not a switch: anything unparseable or zero is the
/// default, and says so rather than silently picking an arm. Read once, so the
/// window and the manifest cannot disagree and the complaint is made once.
pub(crate) fn frame_latency() -> u32 {
    const DEFAULT: u32 = 3;
    static FRAMES: std::sync::OnceLock<u32> = std::sync::OnceLock::new();
    *FRAMES.get_or_init(|| {
        let Some(asked) = codlegacy_env::var_os(FRAME_LATENCY_ENV) else {
            return DEFAULT;
        };
        match asked.to_str().map(str::trim).and_then(|v| v.parse().ok()) {
            Some(frames) if frames > 0 => frames,
            _ => {
                diag::warn!(
                    Launch,
                    "{FRAME_LATENCY_ENV}={} is not a frame count; using {DEFAULT}",
                    asked.to_string_lossy(),
                );
                DEFAULT
            }
        }
    })
}

/// bo2mp: the id DLSS is told the game is (any fixed UUID of the game's own;
/// NVIDIA's programming guide, "Project ID"). Inserted ahead of bevy's
/// `DlssInitPlugin`, which reads it when it builds. Only with the `dlss` feature.
#[cfg(feature = "dlss")]
struct DlssProjectIdPlugin;

#[cfg(feature = "dlss")]
impl Plugin for DlssProjectIdPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(bevy::anti_alias::dlss::DlssProjectId(
            bevy::asset::uuid::uuid!("b575a72d-f09e-4c66-a4fc-8246bdb0d753"),
        ));
    }
}
