//! bo2mp: the temporal upscalers, FSR 3.1 (`wgpu-ffx`, any GPU on Vulkan) and
//! DLSS (`dlss.rs`, NVIDIA RTX on Vulkan, `dlss` cargo feature). They replace
//! the FSR 1 passes of `upscale.rs` for a frame when they can draw it; when
//! they cannot (shaders still building, no motion target yet, an error) the
//! FSR 1 passes draw that frame, so a frame is never lost.
//!
//! What the lens made at the render size (`upscale.rs`, `motion.rs`,
//! `scene_depth.rs`) is put in the formats the upscalers read:
//!
//! * colour: the lens's frame (sRGB texture, read as plain `Unorm`, so the
//!   gamma-encoded values) copied to `Rgba16Float`. Not HDR: the flag stays
//!   off and the pre-exposure is 1.
//! * depth: `Depth24PlusStencil8` (reverse Z, infinite far, jittered) copied to
//!   `R32Float` by a pass of ours (`copy_depth`).
//! * motion: the `ViewMotion` target as it is (`Rg16Float`, render size).
//!
//! and the finished frame (`Rgba16Float`, window size) is drawn onto the
//! window's texture (`present`), with no RCAS of ours: FSR 3.1's own sharpening
//! (the SHARPNESS slider) runs inside it, DLSS's is its own.
//!
//! Conventions (read from `wgpu-ffx` 0.1.0 and bevy 0.19 sources):
//!
//! * Jitter: bevy's `TemporalJitter::jitter_projection` shifts the clip-space x
//!   by `-2 j.x / width` and y by `+2 j.y / height`; FidelityFX's sample shifts
//!   them by `+2 j.x / width` and `-2 j.y / height`. The offset handed to the
//!   upscaler is therefore the negated one (bevy's own DLSS node does the same).
//!   `BO2MP_FSR3_JITTER_SCALE=x,y` multiplies it (default `-1,-1`) for on-PC tuning.
//! * Motion vectors: ours are `current - previous` in UV units (+y down). FSR
//!   reads `previous = current + mv * scale / render_size`, so the scale is
//!   `(-render_w, -render_h)`. `BO2MP_FSR3_MV_SCALE=x,y` multiplies the render
//!   size by `x`, `y` instead of `-1,-1` (each from -1 to 1; `1,1` is the
//!   opposite sign, `-1,1` flips only y).
//! * Depth: `DEPTH_INVERTED | DEPTH_INFINITE`, so `camera_near` is `f32::MAX`
//!   and `camera_far` the lens's near plane (the field's name is the library's).

use bevy::prelude::*;
use bevy::render::RenderStartup;
use bevy::render::camera::ExtractedCamera;
use bevy::render::render_asset::RenderAssets;
use bevy::render::render_resource::binding_types::{texture_2d, texture_depth_2d};
use bevy::render::render_resource::{
    BindGroupEntries, BindGroupLayoutDescriptor, BindGroupLayoutEntries, Buffer, BufferDescriptor,
    BufferUsages, CachedRenderPipelineId, ColorTargetState, ColorWrites, Extent3d, FragmentState,
    LoadOp, Operations, PipelineCache, RenderPassColorAttachment, RenderPassDescriptor,
    RenderPipeline, RenderPipelineDescriptor, ShaderStages, StoreOp, Texture, TextureDescriptor,
    TextureDimension, TextureFormat, TextureId, TextureSampleType, TextureUsages, TextureView,
    TextureViewDescriptor, VertexState,
};
use bevy::render::renderer::{
    RenderAdapterInfo, RenderContext, RenderDevice, RenderQueue, ViewQuery,
};
use bevy::render::texture::GpuImage;
use bevy::render::view::{ExtractedView, ViewTarget};
use bevy::shader::Shader;
use std::collections::HashMap;
use std::ops::Deref;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, Ordering};
use wgpu_ffx::{
    FsrContext, FsrContextFlags, FsrContextInfo, FsrDispatchFlags, FsrDispatchInfo, FsrView,
};

use super::motion::ViewMotion;
use super::scene_depth::SceneDepthTexture;
use super::upscale::{UpscaleCamera, UpscaleFrame, UpscaleJitter, decision};

const SHADER_PATH: &str = "embedded://render_gpu/drawsurf/temporal.wgsl";
const COLOR_FORMAT: TextureFormat = TextureFormat::Rgba16Float;
const DEPTH_FORMAT: TextureFormat = TextureFormat::R32Float;
const MOTION_FORMAT: TextureFormat = TextureFormat::Rg16Float;
/// The world is in inches: 1 unit is 0.0254 m (`view_space_to_meters_factor`).
const METERS_PER_UNIT: f32 = 0.0254;
/// FSR 3.1 refuses a near plane under this (its own validation).
const MIN_NEAR: f32 = 0.075;

/// Set when FSR 3.1 failed to build or to record: the menu then offers it as
/// "uses FSR 1" and the renderer draws FSR 1 for it.
pub(super) static FSR3_BROKEN: AtomicBool = AtomicBool::new(false);

fn fsr3_failed(why: &str) {
    frame::Upscaler::Fsr3.mark_broken();
    if !FSR3_BROKEN.swap(true, Ordering::Relaxed) {
        warn!("bo2mp upscale: FSR 3.1 failed ({why}); drawing FSR 1 instead");
    }
}

/// `x,y` (two finite numbers, comma or space apart).
pub(crate) fn parse_pair(text: &str) -> Option<[f32; 2]> {
    let mut parts = text
        .split(|c: char| c == ',' || c.is_whitespace())
        .filter(|p| !p.is_empty());
    let x: f32 = parts.next()?.parse().ok()?;
    let y: f32 = parts.next()?.parse().ok()?;
    (parts.next().is_none() && x.is_finite() && y.is_finite()).then_some([x, y])
}

fn env_pair(name: &str, default: [f32; 2]) -> [f32; 2] {
    match codlegacy_env::var(name) {
        Ok(text) if !text.trim().is_empty() => parse_pair(&text).unwrap_or_else(|| {
            warn!("bo2mp upscale: {name}={text:?} is not `x,y`; using {default:?}");
            default
        }),
        _ => default,
    }
}

fn mv_multiplier() -> [f32; 2] {
    static V: OnceLock<[f32; 2]> = OnceLock::new();
    *V.get_or_init(|| env_pair("BO2MP_FSR3_MV_SCALE", [-1.0, -1.0]))
}

fn jitter_multiplier() -> [f32; 2] {
    static V: OnceLock<[f32; 2]> = OnceLock::new();
    *V.get_or_init(|| env_pair("BO2MP_FSR3_JITTER_SCALE", [-1.0, -1.0]))
}

/// FSR's `motion_vector_scale` for a render size: the library divides it by
/// the render size, so `(-w, -h)` is a factor of -1 on our UV-unit vectors.
/// Each multiplier is clamped to -1..1 (the library refuses more than the
/// size) and a zero (also refused) means -1.
pub(crate) fn mv_scale(render: UVec2, multiplier: [f32; 2]) -> [f32; 2] {
    let m = |v: f32| if v == 0.0 { -1.0 } else { v.clamp(-1.0, 1.0) };
    [
        render.x as f32 * m(multiplier[0]),
        render.y as f32 * m(multiplier[1]),
    ]
}

/// The jitter the upscaler is told, from the pixels the projection was moved by.
pub(crate) fn upscaler_jitter(pixels: Vec2, multiplier: [f32; 2]) -> [f32; 2] {
    [pixels.x * multiplier[0], pixels.y * multiplier[1]]
}

/// The near plane and vertical field of view (radians) of an infinite
/// reverse-Z perspective projection (`Mat4::perspective_infinite_reverse_rh`).
pub(crate) fn planes(clip_from_view: &Mat4) -> (f32, f32) {
    let near = clip_from_view.w_axis.z;
    let fov_y = 2.0 * (1.0 / clip_from_view.y_axis.y).atan();
    (near, fov_y)
}

/// Whether the view jumped between two frames (a teleport, a respawn, a new
/// map's first frame): the temporal history of the old place is no use.
/// `previous` is the eye and forward vector of the frame before.
pub(crate) fn view_jumped(previous: Option<(Vec3, Vec3)>, eye: Vec3, forward: Vec3) -> bool {
    /// Units (inches) in one frame: a sprint is ~3, a fall ~20, a spawn hundreds.
    const JUMP_DISTANCE: f32 = 150.0;
    /// A snap past this many degrees in one frame is not a mouse.
    const JUMP_ANGLE_COS: f32 = -0.34; // ~110 degrees
    let Some((prev_eye, prev_forward)) = previous else {
        return true;
    };
    prev_eye.distance(eye) > JUMP_DISTANCE || prev_forward.dot(forward) < JUMP_ANGLE_COS
}

/// Whether a temporal pass drew this frame (render world): `draw_upscale` (FSR 1)
/// then stands down.
#[derive(Resource, Default)]
pub(super) struct TemporalOutcome {
    pub drawn: bool,
}

/// The textures that depend on the render and window sizes only.
struct Shared {
    render: UVec2,
    display: UVec2,
    color: Texture,
    color_view: TextureView,
    depth: Texture,
    depth_view: TextureView,
    output: Texture,
    output_view: TextureView,
}

struct Fsr3Resources {
    view: FsrView,
    dilated_depth: Texture,
    dilated_motion: Texture,
    previous_depth: Buffer,
    size: (UVec2, UVec2),
    /// Whether the first dispatch of these resources was run on its own and
    /// came back without a validation error (`self_test`).
    tested: bool,
}

/// Runs `make` with a validation error scope on `device` and returns what it
/// made with the first validation error, if any, as text. (Without the scope
/// a validation error would reach bevy's handler as a frame-dropping error.)
fn scoped<T>(device: &RenderDevice, make: impl FnOnce() -> T) -> (std::thread::Result<T>, Option<String>) {
    let wgpu_device = device.wgpu_device();
    let scope = wgpu_device.push_error_scope(wgpu::ErrorFilter::Validation);
    let made = catch_unwind(AssertUnwindSafe(make));
    let error = bevy::tasks::block_on(scope.pop()).map(|e| e.to_string());
    (made, error)
}

#[derive(Resource)]
pub(super) struct TemporalGpu {
    layout_color: BindGroupLayoutDescriptor,
    layout_depth: BindGroupLayoutDescriptor,
    shader: Handle<Shader>,
    copy_color: Option<CachedRenderPipelineId>,
    copy_depth: Option<CachedRenderPipelineId>,
    present: HashMap<TextureFormat, CachedRenderPipelineId>,
    /// The lens texture read as plain `Unorm`.
    read: Option<(TextureId, TextureView)>,
    shared: Option<Shared>,
    fsr_context: Option<FsrContext>,
    fsr: Option<Fsr3Resources>,
    /// How many history resets (`UpscaleJitter::resets`) have been acted on.
    seen_resets: u32,
    #[cfg(feature = "dlss")]
    pub(super) dlss: super::dlss::DlssState,
}

fn init(mut commands: Commands, asset_server: Res<AssetServer>) {
    commands.insert_resource(TemporalGpu {
        layout_color: BindGroupLayoutDescriptor::new(
            "bo2mp_temporal_color",
            &BindGroupLayoutEntries::single(
                ShaderStages::FRAGMENT,
                texture_2d(TextureSampleType::Float { filterable: false }),
            ),
        ),
        layout_depth: BindGroupLayoutDescriptor::new(
            "bo2mp_temporal_depth",
            &BindGroupLayoutEntries::with_indices(
                ShaderStages::FRAGMENT,
                ((1, texture_depth_2d()),),
            ),
        ),
        shader: asset_server.load(SHADER_PATH),
        copy_color: None,
        copy_depth: None,
        present: HashMap::new(),
        read: None,
        shared: None,
        fsr_context: None,
        fsr: None,
        seen_resets: 0,
        #[cfg(feature = "dlss")]
        dlss: Default::default(),
    });
}

fn pipeline(
    gpu: &TemporalGpu,
    layout: &BindGroupLayoutDescriptor,
    entry: &'static str,
    format: TextureFormat,
    shader_defs: Vec<bevy::shader::ShaderDefVal>,
) -> RenderPipelineDescriptor {
    RenderPipelineDescriptor {
        label: Some(format!("bo2mp_temporal_{entry}").into()),
        layout: vec![layout.clone()],
        immediate_size: 0,
        vertex: VertexState {
            shader: gpu.shader.clone(),
            shader_defs: shader_defs.clone(),
            entry_point: Some("vertex".into()),
            buffers: Vec::new(),
        },
        fragment: Some(FragmentState {
            shader: gpu.shader.clone(),
            shader_defs,
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

fn make_texture(
    device: &RenderDevice,
    label: &'static str,
    size: UVec2,
    format: TextureFormat,
    usage: TextureUsages,
) -> Texture {
    device.create_texture(&TextureDescriptor {
        label: Some(label),
        size: Extent3d {
            width: size.x,
            height: size.y,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: TextureDimension::D2,
        format,
        usage,
        view_formats: &[],
    })
}

fn fullscreen(
    encoder: &mut bevy::render::render_resource::CommandEncoder,
    label: &'static str,
    pipeline: &RenderPipeline,
    bind: &bevy::render::render_resource::BindGroup,
    to: &TextureView,
) {
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
    pass.set_pipeline(pipeline);
    pass.set_bind_group(0, bind, &[]);
    pass.draw(0..3, 0..1);
}

/// What the temporal passes of one frame are given.
#[cfg_attr(not(feature = "dlss"), allow(dead_code))] // (DLSS reads the views.)
pub(super) struct Inputs {
    pub render: UVec2,
    pub display: UVec2,
    pub motion: Texture,
    pub motion_view: TextureView,
    pub color: Texture,
    pub color_view: TextureView,
    pub depth: Texture,
    pub depth_view: TextureView,
    pub output: Texture,
    pub output_view: TextureView,
    pub near: f32,
    pub fov_y: f32,
    pub jitter: Vec2,
    pub frame_ms: f32,
    pub reset: bool,
    pub sharpness: f32,
    pub quality: frame::UpscaleQuality,
}

#[allow(clippy::too_many_arguments)]
pub(super) fn draw_temporal(
    view: ViewQuery<(&ViewTarget, &UpscaleCamera)>,
    plan: Res<UpscaleFrame>,
    jitter: Res<UpscaleJitter>,
    lens: Query<(
        &ViewMotion,
        &SceneDepthTexture,
        &ExtractedView,
        &ExtractedCamera,
    )>,
    images: Res<RenderAssets<GpuImage>>,
    gpu: Option<ResMut<TemporalGpu>>,
    mut outcome: ResMut<TemporalOutcome>,
    cache: Res<PipelineCache>,
    device: Res<RenderDevice>,
    queue: Res<RenderQueue>,
    #[cfg(feature = "dlss")] adapter: Res<bevy::render::renderer::RenderAdapter>,
    mut context: RenderContext,
) {
    outcome.drawn = false;
    if !plan.method.wants_temporal() || !jitter.active {
        return;
    }
    let Some(mut gpu) = gpu else {
        decision("skipped (temporal)", "TemporalGpu resource missing".into());
        return;
    };
    let (target, _) = view.into_inner();
    let Some(source) = plan.image.and_then(|id| images.get(id)) else {
        decision("skipped (temporal)", "the lens image is not on the GPU yet".into());
        return;
    };
    let src = UVec2::new(source.texture.size().width, source.texture.size().height);
    let out = UVec2::new(
        target.main_texture().size().width,
        target.main_texture().size().height,
    );
    if src.x < 2 || src.y < 2 || out.x == 0 || out.y == 0 {
        decision("skipped (temporal)", format!("size: lens {src}, window {out}"));
        return;
    }
    let Some((motion, depth, extracted, _)) = lens.iter().find(|(m, ..)| m.size == src) else {
        decision(
            "skipped (temporal)",
            format!(
                "no lens view with motion vectors and depth at {src} ({} lens view(s) have them): FSR 1 draws",
                lens.iter().count()
            ),
        );
        return;
    };

    // (Pipelines: the frame is FSR 1's until they are made.)
    let dst_format = target.main_texture_format();
    if gpu.copy_color.is_none() {
        let p = pipeline(
            &gpu,
            &gpu.layout_color,
            "copy_color",
            COLOR_FORMAT,
            Vec::new(),
        );
        gpu.copy_color = Some(cache.queue_render_pipeline(p));
    }
    if gpu.copy_depth.is_none() {
        let p = pipeline(
            &gpu,
            &gpu.layout_depth,
            "copy_depth",
            DEPTH_FORMAT,
            Vec::new(),
        );
        gpu.copy_depth = Some(cache.queue_render_pipeline(p));
    }
    if !gpu.present.contains_key(&dst_format) {
        let defs = if dst_format.is_srgb() {
            vec!["SRGB_OUT".into()]
        } else {
            Vec::new()
        };
        let p = pipeline(&gpu, &gpu.layout_color, "present", dst_format, defs);
        let id = cache.queue_render_pipeline(p);
        gpu.present.insert(dst_format, id);
    }
    let (Some(copy_color), Some(copy_depth), Some(present)): (
        Option<&RenderPipeline>,
        Option<&RenderPipeline>,
        Option<&RenderPipeline>,
    ) = (
        gpu.copy_color.and_then(|id| cache.get_render_pipeline(id)),
        gpu.copy_depth.and_then(|id| cache.get_render_pipeline(id)),
        gpu.present
            .get(&dst_format)
            .and_then(|&id| cache.get_render_pipeline(id)),
    ) else {
        decision(
            "skipped (temporal)",
            "copy/present pipelines not ready: FSR 1 draws".into(),
        );
        return;
    };

    // (Textures, remade when the render or window size changes.)
    if gpu
        .read
        .as_ref()
        .is_none_or(|(id, _)| *id != source.texture.id())
    {
        let view = source.texture.create_view(&TextureViewDescriptor {
            label: Some("bo2mp_temporal_read"),
            format: Some(TextureFormat::Rgba8Unorm),
            ..Default::default()
        });
        gpu.read = Some((source.texture.id(), view));
    }
    if gpu
        .shared
        .as_ref()
        .is_none_or(|s| s.render != src || s.display != out)
    {
        let color = make_texture(
            &device,
            "bo2mp_temporal_color",
            src,
            COLOR_FORMAT,
            TextureUsages::RENDER_ATTACHMENT | TextureUsages::TEXTURE_BINDING,
        );
        let depth = make_texture(
            &device,
            "bo2mp_temporal_depth",
            src,
            DEPTH_FORMAT,
            TextureUsages::RENDER_ATTACHMENT | TextureUsages::TEXTURE_BINDING,
        );
        let output = make_texture(
            &device,
            "bo2mp_temporal_output",
            out,
            COLOR_FORMAT,
            TextureUsages::STORAGE_BINDING | TextureUsages::TEXTURE_BINDING,
        );
        gpu.shared = Some(Shared {
            render: src,
            display: out,
            color_view: color.create_view(&TextureViewDescriptor::default()),
            color,
            depth_view: depth.create_view(&TextureViewDescriptor::default()),
            depth,
            output_view: output.create_view(&TextureViewDescriptor::default()),
            output,
        });
    }
    let (Some((_, read)), Some(shared)) = (gpu.read.as_ref(), gpu.shared.as_ref()) else {
        return;
    };

    // The colour and depth in the upscalers' formats.
    let layout_color = cache.get_bind_group_layout(&gpu.layout_color);
    let layout_depth = cache.get_bind_group_layout(&gpu.layout_depth);
    let color_bind = device.create_bind_group(
        "bo2mp_temporal_copy_color",
        &layout_color,
        &BindGroupEntries::single(read),
    );
    let depth_bind = device.create_bind_group(
        "bo2mp_temporal_copy_depth",
        &layout_depth,
        &BindGroupEntries::with_indices(((1, depth.view()),)),
    );
    let present_bind = device.create_bind_group(
        "bo2mp_temporal_present",
        &layout_color,
        &BindGroupEntries::single(&shared.output_view),
    );
    {
        let encoder = context.command_encoder();
        fullscreen(
            encoder,
            "bo2mp_temporal_copy_color",
            copy_color,
            &color_bind,
            &shared.color_view,
        );
        fullscreen(
            encoder,
            "bo2mp_temporal_copy_depth",
            copy_depth,
            &depth_bind,
            &shared.depth_view,
        );
    }
    let (near, fov_y) = planes(&extracted.clip_from_view);
    let inputs = Inputs {
        render: src,
        display: out,
        motion: motion.texture.clone(),
        motion_view: motion.view.clone(),
        color: shared.color.clone(),
        color_view: shared.color_view.clone(),
        depth: shared.depth.clone(),
        depth_view: shared.depth_view.clone(),
        output: shared.output.clone(),
        output_view: shared.output_view.clone(),
        near,
        fov_y,
        jitter: jitter.pixels,
        frame_ms: jitter.frame_ms,
        reset: jitter.resets != gpu.seen_resets,
        sharpness: plan.sharpness,
        quality: plan.quality,
    };
    let ok = match plan.method {
        frame::Upscaler::Fsr3 => record_fsr3(&mut gpu, &device, &queue, &mut context, &inputs),
        #[cfg(feature = "dlss")]
        frame::Upscaler::Dlss => super::dlss::record(
            &mut gpu.dlss,
            &device,
            &queue,
            &adapter,
            &mut context,
            &inputs,
        ),
        _ => false,
    };
    if !ok {
        decision(
            "skipped (temporal)",
            "the upscaler did not record this frame (see the FSR 3.1 lines above): FSR 1 draws".into(),
        );
        return;
    }
    decision(
        "temporal pass",
        format!("{:?} recorded {src} -> {out}", plan.method),
    );
    gpu.seen_resets = jitter.resets;
    let post = target.post_process_write();
    fullscreen(
        context.command_encoder(),
        "bo2mp_temporal_present",
        present,
        &present_bind,
        post.destination,
    );
    outcome.drawn = true;
}

/// FSR 3.1: the compute passes of `wgpu-ffx` into `Inputs::output`.
fn record_fsr3(
    gpu: &mut TemporalGpu,
    device: &RenderDevice,
    queue: &RenderQueue,
    context: &mut RenderContext,
    frame: &Inputs,
) -> bool {
    if FSR3_BROKEN.load(Ordering::Relaxed) {
        return false;
    }
    if gpu.fsr_context.is_none() {
        let wgpu_device = device.wgpu_device().clone();
        let (made, error) = scoped(device, || {
            FsrContext::new(FsrContextInfo {
                device: wgpu_device,
                flags: FsrContextFlags::DEPTH_INVERTED | FsrContextFlags::DEPTH_INFINITE,
            })
        });
        match (made, error) {
            (Ok(context), None) => {
                info!("bo2mp upscale: FSR 3.1 context made (its compute pipelines built without a validation error)");
                gpu.fsr_context = Some(context);
            }
            (Ok(_), Some(error)) => {
                fsr3_failed(&format!("building its pipelines gave a validation error: {error}"));
                return false;
            }
            (Err(_), error) => {
                fsr3_failed(&format!(
                    "building its pipelines panicked (see the panic line above){}",
                    error.map_or(String::new(), |e| format!("; validation: {e}"))
                ));
                return false;
            }
        }
    }
    let Some(fsr) = gpu.fsr_context.as_ref() else {
        return false;
    };
    let size = (frame.render, frame.display);
    if gpu.fsr.as_ref().is_none_or(|r| r.size != size) {
        let (made, error) = scoped(device, || {
            let view = fsr.create_view(queue, frame.render.to_array(), frame.display.to_array());
            let dilated_depth = make_texture(
                device,
                "bo2mp_fsr3_dilated_depth",
                frame.render,
                DEPTH_FORMAT,
                TextureUsages::STORAGE_BINDING | TextureUsages::TEXTURE_BINDING,
            );
            let dilated_motion = make_texture(
                device,
                "bo2mp_fsr3_dilated_motion",
                frame.render,
                MOTION_FORMAT,
                TextureUsages::STORAGE_BINDING | TextureUsages::TEXTURE_BINDING,
            );
            let previous_depth = device.create_buffer(&BufferDescriptor {
                label: Some("bo2mp_fsr3_previous_depth"),
                size: frame.render.x as u64 * frame.render.y as u64 * 4,
                usage: BufferUsages::STORAGE | BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            Fsr3Resources {
                view,
                dilated_depth,
                dilated_motion,
                previous_depth,
                size,
                tested: false,
            }
        });
        let made = match (made, error) {
            (Ok(resources), None) => Ok(resources),
            (Ok(_), Some(error)) => {
                fsr3_failed(&format!("its textures gave a validation error: {error}"));
                return false;
            }
            (Err(panic), _) => Err(panic),
        };
        match made {
            Ok(resources) => {
                info!(
                    "bo2mp upscale: FSR 3.1, drawing {}x{} for a {}x{} window ({} MB of history)",
                    frame.render.x,
                    frame.render.y,
                    frame.display.x,
                    frame.display.y,
                    resources.view.estimated_memory_usage() / (1024 * 1024)
                );
                gpu.fsr = Some(resources);
            }
            Err(_) => {
                fsr3_failed("its textures were refused by this device");
                return false;
            }
        }
    }
    let (Some(fsr), Some(resources)) = (gpu.fsr_context.as_ref(), gpu.fsr.as_mut()) else {
        return false;
    };
    let raw = |t: &Texture| Deref::deref(t).clone();
    let mut info = FsrDispatchInfo {
        color: raw(&frame.color),
        depth: raw(&frame.depth),
        motion_vectors: raw(&frame.motion),
        exposure: None,
        reactive_mask: None,
        transparency_and_composition: None,
        dilated_depth: raw(&resources.dilated_depth),
        dilated_motion_vectors: raw(&resources.dilated_motion),
        reconstructed_previous_depth: Deref::deref(&resources.previous_depth).clone(),
        output: raw(&frame.output),
        jitter_offset: upscaler_jitter(frame.jitter, jitter_multiplier()),
        motion_vector_scale: mv_scale(frame.render, mv_multiplier()),
        render_size: frame.render.to_array(),
        upscale_size: frame.display.to_array(),
        enable_sharpening: frame.sharpness > 0.001,
        sharpness: frame.sharpness.clamp(0.0, 1.0),
        frame_time_delta: frame.frame_ms.clamp(1.0, 1000.0),
        pre_exposure: 1.0,
        reset_history: frame.reset,
        camera_near: f32::MAX,
        camera_far: frame.near.max(MIN_NEAR),
        camera_fov_y: frame.fov_y.clamp(0.01, 3.1),
        view_space_to_meters_factor: METERS_PER_UNIT,
        flags: FsrDispatchFlags::empty(),
    };
    if !resources.tested {
        // (The first dispatch of a new set of resources, on its own encoder
        // and queue submission inside a validation scope: a bind group, usage
        // or format mismatch shows up here, once, as a fall-back to FSR 1,
        // and not as an invalid pass that takes the whole frame with it.)
        resources.tested = true;
        let wgpu_device = device.wgpu_device();
        let mut encoder = wgpu_device.create_command_encoder(&Default::default());
        let (ran, error) = scoped(device, || {
            let result = fsr.dispatch(&mut resources.view, &mut encoder, &info);
            queue.submit(std::iter::once(encoder.finish()));
            result
        });
        match (ran, error) {
            (Ok(Ok(())), None) => info!("bo2mp upscale: FSR 3.1 first dispatch passed its self-test"),
            (Ok(Ok(())), Some(error)) => {
                fsr3_failed(&format!("its first dispatch gave a validation error: {error}"));
                return false;
            }
            (Ok(Err(error)), _) => {
                fsr3_failed(&format!("its first dispatch was refused: {error}"));
                return false;
            }
            (Err(_), _) => {
                fsr3_failed("its first dispatch panicked (see the panic line above)");
                return false;
            }
        }
        info.reset_history = true;
    }
    let encoder = context.command_encoder();
    let dispatched = catch_unwind(AssertUnwindSafe(|| {
        fsr.dispatch(&mut resources.view, encoder, &info)
    }));
    match dispatched {
        Ok(Ok(())) => true,
        Ok(Err(error)) => {
            fsr3_failed(&error.to_string());
            false
        }
        Err(_) => {
            fsr3_failed("a dispatch panicked (see the panic line above)");
            false
        }
    }
}

/// The adapter's backend and vendor, and what follows for the upscalers:
/// told to the menu (`frame::Upscaler::set_support`) and logged once.
pub(super) fn refresh_support(
    info: Option<Res<RenderAdapterInfo>>,
    adapter: Option<Res<bevy::render::renderer::RenderAdapter>>,
    device: Option<Res<RenderDevice>>,
    mut last: Local<Option<(bool, bool)>>,
    // (What the adapter allows, checked once: FSR 1 ok, FSR 3.1 ok.)
    mut caps: Local<Option<(bool, bool)>>,
) {
    let Some(info) = info else {
        return;
    };
    let vulkan = format!("{:?}", info.0.backend) == "Vulkan";
    let nvidia = info.0.vendor == 0x10DE;
    if caps.is_none() {
        if let (Some(adapter), Some(device)) = (adapter.as_ref(), device.as_ref()) {
            let limits = device.limits();
            let format = |f| adapter.get_texture_format_features(f);
            let c = super::upscale_caps::DeviceCaps {
                vulkan,
                view_formats: adapter
                    .get_downlevel_capabilities()
                    .flags
                    .contains(bevy::render::render_resource::DownlevelFlags::VIEW_FORMATS),
                features: device.features(),
                limits: &limits,
                format: &format,
            };
            let fsr1 = super::upscale_caps::fsr1_issues(&c);
            let fsr3 = super::upscale_caps::fsr3_issues(&c);
            if fsr1.is_empty() {
                info!("bo2mp upscale: this adapter can run FSR 1");
            } else {
                warn!(
                    "bo2mp upscale: this adapter cannot run FSR 1 ({}); upscaling stays Off",
                    fsr1.join("; ")
                );
                frame::Upscaler::force_off(true);
            }
            if fsr3.is_empty() {
                info!("bo2mp upscale: this adapter can run FSR 3.1");
            } else {
                warn!(
                    "bo2mp upscale: this adapter cannot run FSR 3.1 ({}); a choice of FSR 3.1 draws FSR 1",
                    fsr3.join("; ")
                );
            }
            *caps = Some((fsr1.is_empty(), fsr3.is_empty()));
        }
    }
    let caps_fsr3 = caps.is_none_or(|(_, fsr3)| fsr3);
    let fsr3 = vulkan && caps_fsr3 && !FSR3_BROKEN.load(Ordering::Relaxed);
    let dlss = vulkan && nvidia && dlss_ready();
    frame::Upscaler::set_support(fsr3, dlss);
    if *last != Some((fsr3, dlss)) {
        *last = Some((fsr3, dlss));
        info!(
            "bo2mp upscale: backend {:?}, adapter {}; FSR 3.1 {}, DLSS {}",
            info.0.backend,
            info.0.name,
            if fsr3 {
                "available"
            } else {
                "not available (needs Vulkan)"
            },
            if dlss {
                "available"
            } else if !cfg!(feature = "dlss") {
                "not in this build (cargo feature `dlss`)"
            } else if !vulkan || !nvidia {
                "not available (needs an NVIDIA RTX card on Vulkan)"
            } else {
                "not available (driver or DLSS runtime refused)"
            },
        );
    }
}

#[cfg(feature = "dlss")]
fn dlss_ready() -> bool {
    super::dlss::READY.load(Ordering::Relaxed)
}

#[cfg(not(feature = "dlss"))]
fn dlss_ready() -> bool {
    false
}

pub(super) fn register(app: &mut App) {
    bevy::asset::embedded_asset!(app, "temporal.wgsl");
    app.add_systems(Update, refresh_support);
    #[cfg(feature = "dlss")]
    super::dlss::register_main(app);
    let Some(render_app) = app.get_sub_app_mut(bevy::render::RenderApp) else {
        return;
    };
    render_app
        .init_resource::<TemporalOutcome>()
        .add_systems(RenderStartup, init)
        .add_systems(
            bevy::core_pipeline::Core2d,
            draw_temporal
                .in_set(bevy::core_pipeline::Core2dSystems::PostProcess)
                .before(super::upscale::draw_upscale),
        );
    #[cfg(feature = "dlss")]
    render_app.add_systems(RenderStartup, super::dlss::init_sdk.after(init));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pairs_parse_or_say_no() {
        assert_eq!(parse_pair("1,-1"), Some([1.0, -1.0]));
        assert_eq!(parse_pair(" -0.5  0.5 "), Some([-0.5, 0.5]));
        assert_eq!(parse_pair("1"), None);
        assert_eq!(parse_pair("1,2,3"), None);
        assert_eq!(parse_pair("a,b"), None);
        assert_eq!(parse_pair("nan,1"), None);
    }

    #[test]
    fn motion_scale_flips_our_current_minus_previous() {
        let render = UVec2::new(1280, 720);
        // (Default: previous = current + mv * (-1), in UV units.)
        assert_eq!(mv_scale(render, [-1.0, -1.0]), [-1280.0, -720.0]);
        // (The tuning knob: 1,1 is the other sign, 0 is not allowed, over 1 is held.)
        assert_eq!(mv_scale(render, [1.0, -1.0]), [1280.0, -720.0]);
        assert_eq!(mv_scale(render, [0.0, 5.0]), [-1280.0, 720.0]);
    }

    #[test]
    fn jitter_is_negated_for_the_upscaler() {
        // Bevy moves clip x by -2 j / w, FidelityFX's sample by +2 j / w.
        let j = upscaler_jitter(Vec2::new(0.25, -0.125), [-1.0, -1.0]);
        assert_eq!(j, [-0.25, 0.125]);
        let mut p = Mat4::perspective_infinite_reverse_rh(1.0, 1.6, 4.0);
        let before = p;
        bevy::render::camera::TemporalJitter {
            offset: Vec2::new(0.5, 0.0),
        }
        .jitter_projection(&mut p, Vec2::new(1000.0, 500.0));
        // (A point straight ahead moves in NDC x by -2 * 0.5 / 1000.)
        let ahead = Vec4::new(0.0, 0.0, -10.0, 1.0);
        let (a, b) = (before * ahead, p * ahead);
        let dx = b.x / b.w - a.x / a.w;
        assert!((dx - (-0.001)).abs() < 1e-6, "{dx}");
    }

    #[test]
    fn planes_come_back_out_of_the_projection() {
        let p = Mat4::perspective_infinite_reverse_rh(1.2, 16.0 / 9.0, 4.0);
        let (near, fov) = planes(&p);
        assert!((near - 4.0).abs() < 1e-5 && (fov - 1.2).abs() < 1e-5);
    }

    #[test]
    fn a_jump_is_a_teleport_or_a_snap_not_a_run() {
        let fwd = Vec3::X;
        let eye = Vec3::new(100.0, 0.0, 0.0);
        assert!(view_jumped(None, eye, fwd));
        assert!(!view_jumped(
            Some((eye, fwd)),
            eye + Vec3::new(4.0, 0.0, 0.0),
            fwd
        ));
        assert!(view_jumped(
            Some((eye, fwd)),
            eye + Vec3::new(500.0, 0.0, 0.0),
            fwd
        ));
        assert!(!view_jumped(
            Some((eye, fwd)),
            eye,
            Vec3::new(1.0, 1.0, 0.0).normalize()
        ));
        assert!(view_jumped(Some((eye, fwd)), eye, -Vec3::X));
    }

    #[test]
    fn shader_entry_points_are_there() {
        let source = include_str!("temporal.wgsl");
        for entry in [
            "fn vertex(",
            "fn copy_color(",
            "fn copy_depth(",
            "fn present(",
        ] {
            assert!(source.contains(entry), "{entry}");
        }
    }
}
