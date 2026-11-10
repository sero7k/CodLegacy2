//! The lit fog volumes drawn on the GPU as BlackOps3.exe records them
//! (`docs/T7_NATIVE.md`, "Lit fog and volumetric light"), from the list
//! [`crate::volumetric::pack`] makes per view:
//!
//! 1. **Depth pre-pass** (before the opaque forward lists, as the exe's
//!    pre-pass): `downsample_depth_with_uv_4x_compute` reads `floatZ` and
//!    writes render target 0x25 (quarter R32F, the march's
//!    `downsampledDepth`) and 0x27 (quarter, the 4x4 block's chosen texel as
//!    `y << 2 | x`, the march's `downSampledDepthUV`; R8_UINT in the exe,
//!    R32_UINT here: wgpu has no 8-bit storage format, same values), with
//!    `postFxControl7` = (4, 0, 0, 0) and `postFxConst00` = (1 / w, 1 / h,
//!    quarter w, quarter h), in quarter pixels.
//! 2. **March** (after view list 20, before 21): `downsample_depth_4x_compute`
//!    into render target 0x26 (quarter R32F); render target 0x28 (quarter
//!    RGBA16F) cleared to zero; `volumetric_light_sun_skip_samples` drawn over
//!    it when the list's flags have 1, 4 or 0x10 (the sun-lit volumes), with
//!    `gLitFogVolumes` (code buffer 0xc), `LightingGlobals`, the sun shadow
//!    array, the cookie array and `volLightTexture` (the 3-D volume only the
//!    `3DVolume` mode writes: zero in the default mode, bound as one zero
//!    texel here); the OIT inputs (code textures 0x1d, 0x1e, 0x20) bound to
//!    white, as the exe binds its own images when the view has no OIT (the
//!    march then takes transmittance 1); then, when the flags have 1, 2 or 4,
//!    `volumetric_light_hblur_compute` (0x28 → 0x29) and
//!    `volumetric_light_vblur_compute` (0x29 → 0x28), reading 0x26.
//! 3. **Apply**: the scene copied into `feedback` (code texture 8) and
//!    `volumetric_light_apply` drawn over the scene: `feedback` plus the
//!    depth-aware upsample of 0x28 (gathered against 0x26 and `floatZ`)
//!    scaled by `relHDRExposure.y`.
//!
//! Constants as the exe's `postFxControl0..7`, `A` ([`crate::volumetric::constants`])
//! and `PerSceneConsts` by the techniques' arguments (the code-draw
//! transform for the target, its `renderTargetSize`, the view's `zNear`,
//! `relHDRExposure`). With no lit fog volumes packed for the view nothing
//! is recorded (the exe's gate: the list's count).
//!
//! 4. **Local lights** (list flag 2, a volume with flag 0x40; made only
//!    for a map that has one): in the pre-pass `volumetric_light_cull_compute`
//!    ANDs the forward light culling's `visibleLights` (all 16 depth slices
//!    of a tile) with the light packer's `volumetricSpotLightBits` /
//!    `volumetricOmniLightBits` into two tile lists (16 layers, one per 32
//!    lights); the march then draws `volumetric_light_skip_samples` into
//!    0x28 after the sun's (its blend state adds), reading those lists
//!    (code textures 0x1a, 0x1b), the packed light records (code buffer 1)
//!    and their cull constants (code buffer 4), the spot shadow and cookie
//!    arrays and `LightingGlobals` words 200..203 (the first and last
//!    volumetric spot and omni).
//! 5. **The 3-D volume** (list flag 4, a volume with flag 0x80; made only
//!    for a map that has one): in the pre-pass `volumetric_probe_cull_compute`
//!    (forward `visibleProbes` AND `volumetricProbeBits`) and
//!    `volumetric_light_compute`, which lights `volLightTexture` (one texel
//!    per 16x16 pixels, 64 slices; RGBA16F here, R11G11B10 in the exe:
//!    wgpu cannot store to it) from the probe volume and the volumetric
//!    probes; the march samples it. Without the flag the march samples the
//!    never-written (zero) volume, as in the exe.
//!
//! Not built: the `3DVolume` and `2DRayMarchUnified` modes and the
//! half-resolution apply (dvars, off by default).

#[allow(unused_imports)]
use crate::profile::{ComputeExt as _, EncExt as _, PassExt as _, QueueExt as _};
use render_h1::code::Camera;
use render_h1::gpu::Gpu;
use wgpu::util::DeviceExt;

use crate::code::CodeConsts;
use crate::compute::{Program, Res};
use crate::gbuffer::Targets;
use crate::gpu::Tex;
use crate::lighting::Lighting;
use crate::post_pass::{CodeBuffers, CodePass, CodeTech};
use crate::volumetric::{Dvars, Packed};

const LIGHT_MARCH: &str = "volumetric_light_skip_samples";
const LIGHT_MARCH_ALL: &str = "volumetric_light";
const TEX_VISIBLE_SPOT: u32 = 0x1a;
const TEX_VISIBLE_OMNI: u32 = 0x1b;
const TEX_SPOT_SHADOW: u32 = 0x5;
const BUFFER_LIGHT_RECORDS: u32 = 0x1;
const BUFFER_CULL_CONSTANTS: u32 = 0x4;

const SUN_MARCH: &str = "volumetric_light_sun_skip_samples";
const SUN_MARCH_ALL: &str = "volumetric_light_sun";
const APPLY: &str = "volumetric_light_apply";
/// Registers of the march's `PostFxCBuffer` (`postFxControl0..7`, 8, 9, `A`).
const MARCH_REGISTERS: usize = 11;

/// The march's code textures (`141d4bb60`, view flag on).
const TEX_POST_EFFECT1: u32 = 0xe; // render target 0x25
const TEX_POST_EFFECT0: u32 = 0xd; // render target 0x28 (the apply's)
const TEX_POST_EFFECT_SRC: u32 = 0xb; // volLightTexture
const TEX_CODE0: u32 = 0x1a;
const TEX_CODE1: u32 = 0x1b;
const TEX_CODE3: u32 = 0x1d;
const TEX_CODE4: u32 = 0x1e;
const TEX_CODE5: u32 = 0x1f;
const TEX_CODE6: u32 = 0x20;
const TEX_FEEDBACK: u32 = 0x8;
const TEX_SUN_SHADOW: u32 = 0x3;
const TEX_COOKIE: u32 = 0x7;
const BUFFER_LIT_FOG: u32 = 0xc;

struct March {
    pass: CodePass,
    tech: CodeTech,
    groups: [wgpu::BindGroup; 3],
}

/// The local-light march and its culling (list flag 2).
struct Local {
    cull: (Program, Vec<wgpu::BindGroup>),
    _lists: [Tex; 2],
    march: March,
    scene: wgpu::Buffer,
    tiles: [u32; 2],
}

/// The 3-D volume's probe culling and lighting (list flag 4).
struct Volume3d {
    cull: (Program, Vec<wgpu::BindGroup>),
    _list: Tex,
    compute: (Program, Vec<wgpu::BindGroup>),
    scene: wgpu::Buffer,
    size: [u32; 3],
    tiles: [u32; 2],
}

/// The tile lists the volumetric culling writes (R32_UINT, one layer per
/// 32 lights or probes, 16 layers).
fn tile_list(gpu: &Gpu, label: &str, tiles: [u32; 2]) -> Tex {
    let texture = crate::audit::texture(gpu, &wgpu::TextureDescriptor {
        label: Some(label),
        size: wgpu::Extent3d { width: tiles[0].max(1), height: tiles[1].max(1), depth_or_array_layers: 16 },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::R32Uint,
        usage: wgpu::TextureUsages::STORAGE_BINDING | wgpu::TextureUsages::TEXTURE_BINDING,
        view_formats: &[],
    });
    let view = texture.create_view(&wgpu::TextureViewDescriptor { dimension: Some(wgpu::TextureViewDimension::D2Array), ..Default::default() });
    Tex { texture, view }
}

pub struct VolumetricPass {
    gpu: Gpu,
    size: [u32; 2],
    quarter: [u32; 2],
    with_uv: (Program, Vec<wgpu::BindGroup>),
    down: (Program, Vec<wgpu::BindGroup>),
    blur: [(Program, Vec<wgpu::BindGroup>); 2],
    _rt: [Tex; 5],
    rt28_view: wgpu::TextureView,
    feedback: Tex,
    sun: March,
    apply: March,
    local: Option<Local>,
    volume: Option<Volume3d>,
    _vol: Tex,
    _white: Tex,
    _buffers: CodeBuffers,
    lit_fog: wgpu::Buffer,
    post_fx: wgpu::Buffer,
    _depth_consts: wgpu::Buffer,
    march_scene: wgpu::Buffer,
    apply_scene: wgpu::Buffer,
    quarter_tri: wgpu::Buffer,
    full_tri: wgpu::Buffer,
    /// This frame's passes (`crate::volumetric::plan`); empty: nothing drawn.
    plan: Vec<&'static str>,
    /// This frame's pre-pass programs (`crate::volumetric::prepass`).
    prepass: Vec<&'static str>,
    frames: u64,
    /// `t7 volumetrics` lines (taken by the host).
    pub report: Vec<String>,
}

fn storage(gpu: &Gpu, label: &str, w: u32, h: u32, format: wgpu::TextureFormat, render: bool) -> Tex {
    let mut usage = wgpu::TextureUsages::STORAGE_BINDING | wgpu::TextureUsages::TEXTURE_BINDING;
    if render {
        usage |= wgpu::TextureUsages::RENDER_ATTACHMENT;
    }
    let texture = crate::audit::texture(gpu, &wgpu::TextureDescriptor {
        label: Some(label),
        size: wgpu::Extent3d { width: w.max(1), height: h.max(1), depth_or_array_layers: 1 },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage,
        view_formats: &[],
    });
    let view = texture.create_view(&Default::default());
    Tex { texture, view }
}

fn texel(gpu: &Gpu, label: &str, dim: wgpu::TextureDimension, format: wgpu::TextureFormat, value: &[u8]) -> Tex {
    let texture = gpu.device.create_texture_with_data(
        &gpu.queue,
        &wgpu::TextureDescriptor { label: Some(label), size: wgpu::Extent3d { width: 1, height: 1, depth_or_array_layers: 1 }, mip_level_count: 1, sample_count: 1, dimension: dim, format, usage: wgpu::TextureUsages::TEXTURE_BINDING, view_formats: &[] },
        wgpu::util::TextureDataOrder::LayerMajor,
        value,
    );
    let view = texture.create_view(&Default::default());
    Tex { texture, view }
}

/// A material sampler state made non-filtering (the apply only gathers,
/// which no filter changes; its R32F inputs cannot be filtered).
fn nearest(gpu: &Gpu, state: u16, border: bool) -> Result<wgpu::Sampler, String> {
    let mut d = crate::sampler::descriptor(state, crate::sampler::ANISO_QUALITY, border)?;
    d.mag_filter = wgpu::FilterMode::Nearest;
    d.min_filter = wgpu::FilterMode::Nearest;
    d.mipmap_filter = wgpu::MipmapFilterMode::Nearest;
    d.anisotropy_clamp = 1;
    d.compare = None;
    Ok(gpu.device.create_sampler(&d))
}

impl VolumetricPass {
    /// Made when the map has lit fog volumes (the passes only run for a view
    /// whose list is not empty).
    /// `visible`: the forward light culling's `visibleLights` and
    /// `visibleProbes` (the local-light and 3-D parts need them).
    pub fn new(gpu: &Gpu, zone_dir: &std::path::Path, targets: &Targets, lighting: &Lighting, visible: Option<[&wgpu::TextureView; 2]>) -> Result<Self, String> {
        let patch = fastfile_t7::shared::zone(zone_dir, "core_patch").ok_or("core_patch.ff not read")?;
        let shaders = crate::compute::code_shaders(&patch.image, &patch.capture);
        let cs = |n: &str| shaders.get(n).ok_or_else(|| format!("{n} not in core_patch"));
        let (w, h) = (targets.width.max(1), targets.height.max(1));
        let (qw, qh) = crate::volumetric::quarter(w, h);
        let mut report = Vec::new();

        // Render targets 0x25..0x29 (the exe's size classes 2 and 9).
        let rt25 = storage(gpu, "volumetric downsampledDepth (0x25)", qw, qh, wgpu::TextureFormat::R32Float, false);
        let rt26 = storage(gpu, "volumetric floatZDownscaled (0x26)", qw, qh, wgpu::TextureFormat::R32Float, false);
        let rt27 = storage(gpu, "volumetric downSampledDepthUV (0x27)", qw, qh, wgpu::TextureFormat::R32Uint, false);
        let rt28 = storage(gpu, "volumetric light (0x28)", qw, qh, wgpu::TextureFormat::Rgba16Float, true);
        let rt29 = storage(gpu, "volumetric light blur (0x29)", qw, qh, wgpu::TextureFormat::Rgba16Float, false);
        let feedback = crate::gpu::target(gpu, "volumetric feedback", w, h, crate::lighting::SCENE_FORMAT, wgpu::TextureUsages::COPY_DST);
        // The map's volume flags: the local-light and 3-D parts are made
        // only for a map that has a volume lit by local lights (0x40) or
        // with the 3-D volume (0x80).
        let map_flags = lighting.lit_fog_flags();
        let want_local = map_flags & 0x40 != 0;
        let want_volume = map_flags & 0x80 != 0;
        let vsize = crate::volumetric::volume_size(w, h);
        // (`volLightTexture`: the 3-D volume when the map can write it, else
        // one zero texel: the default mode never writes it.)
        let vol = if want_volume {
            let texture = crate::audit::texture(gpu, &wgpu::TextureDescriptor {
                label: Some("volumetricLightVolume"),
                size: wgpu::Extent3d { width: vsize[0], height: vsize[1], depth_or_array_layers: vsize[2] },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D3,
                format: wgpu::TextureFormat::Rgba16Float,
                usage: wgpu::TextureUsages::STORAGE_BINDING | wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            });
            let view = texture.create_view(&Default::default());
            Tex { texture, view }
        } else {
            texel(gpu, "volLightTexture (zero)", wgpu::TextureDimension::D3, wgpu::TextureFormat::Rg11b10Ufloat, &[0; 4])
        };
        // (The OIT inputs: white over the whole frame, the empty state the
        // program reads as depth 2000 and transmittance 1; full size, as it
        // loads them at full-resolution texels.)
        let white = {
            let texture = gpu.device.create_texture_with_data(
                &gpu.queue,
                &wgpu::TextureDescriptor { label: Some("volumetric OIT inputs (white)"), size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 }, mip_level_count: 1, sample_count: 1, dimension: wgpu::TextureDimension::D2, format: wgpu::TextureFormat::Rgba8Unorm, usage: wgpu::TextureUsages::TEXTURE_BINDING, view_formats: &[] },
                wgpu::util::TextureDataOrder::LayerMajor,
                &vec![255u8; (w * h * 4) as usize],
            );
            let view = texture.create_view(&Default::default());
            Tex { texture, view }
        };

        let depth = || lighting.named("GBufferDepth", targets);
        let depth_sampler = gpu.device.create_sampler(&wgpu::SamplerDescriptor { label: Some("depthSampler"), ..Default::default() });
        // postFxControl7 and postFxConst00 of both downsamples.
        let mut dc = [[0f32; 4]; 17];
        dc[7] = [4.0, 0.0, 0.0, 0.0];
        dc[16] = [1.0 / w as f32, 1.0 / h as f32, qw as f32, qh as f32];
        let depth_consts = crate::gpu::buffer(gpu, "t7 volumetric depth PostFxCBuffer", bytemuck::cast_slice(&dc), wgpu::BufferUsages::UNIFORM);
        let with_uv = Program::new_opts(gpu, cs("downsample_depth_with_uv_4x_compute")?, &[(0, "r32float"), (1, "r32uint")], &[0], true)?;
        let g = with_uv.bind(gpu, |s| match s.name.as_str() {
            "PostFxCBuffer" => Some(Res::Buffer(&depth_consts)),
            "floatZSampler" => depth(),
            "depthSampler" => Some(Res::Sampler(&depth_sampler)),
            "postEffect1" => Some(Res::View(&rt25.view)),
            "postEffect2" => Some(Res::View(&rt27.view)),
            _ => None,
        })?;
        let with_uv = (with_uv, g);
        let down = Program::new_opts(gpu, cs("downsample_depth_4x_compute")?, &[(0, "r32float")], &[0], true)?;
        let g = down.bind(gpu, |s| match s.name.as_str() {
            "PostFxCBuffer" => Some(Res::Buffer(&depth_consts)),
            "floatZSampler" => depth(),
            "depthSampler" => Some(Res::Sampler(&depth_sampler)),
            "postEffect1" => Some(Res::View(&rt26.view)),
            _ => None,
        })?;
        let down = (down, g);

        let u = wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST;
        let post_fx = gpu.device.create_buffer(&wgpu::BufferDescriptor { label: Some("t7 volumetric PostFxCBuffer"), size: (16 * MARCH_REGISTERS) as u64, usage: u, mapped_at_creation: false });
        let lit_fog = gpu.device.create_buffer(&wgpu::BufferDescriptor { label: Some("gLitFogVolumes"), size: (crate::volumetric::RECORD * crate::volumetric::MAX) as u64, usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST, mapped_at_creation: false });
        // Horizontal 0x28 → 0x29, vertical 0x29 → 0x28 (`141d4ced0`).
        let blur = |name: &str, src: &Tex, dst: &Tex| -> Result<(Program, Vec<wgpu::BindGroup>), String> {
            let p = Program::new_with(gpu, cs(name)?, &[(0, "rgba16float")], &[0, 2])?;
            let g = p.bind(gpu, |s| match s.name.as_str() {
                "PostFxCBuffer" => Some(Res::Buffer(&post_fx)),
                "postEffectSrc" => Some(Res::View(&src.view)),
                "codeTexture0" => Some(Res::View(&rt26.view)),
                "postEffect0" => Some(Res::View(&dst.view)),
                _ => None,
            })?;
            Ok((p, g))
        };
        let blur = [blur("volumetric_light_hblur_compute", &rt28, &rt29)?, blur("volumetric_light_vblur_compute", &rt29, &rt28)?];

        let d = Dvars::current();
        let sun_name = if d.sun_skip_samples { SUN_MARCH } else { SUN_MARCH_ALL };
        let techs = crate::post_pass::code_techs(&patch.image, &patch.capture, &[sun_name, APPLY])?;
        let buffers = CodeBuffers::new(gpu);
        let border = gpu.device.features().contains(wgpu::Features::ADDRESS_MODE_CLAMP_TO_BORDER);
        let scene_size = |n: &CodeTech| n.args.iter().filter(|a| a.ty & 0xff00 == 0x400 && a.buffer == 1).map(|a| u64::from(a.location) + u64::from(a.size)).max().unwrap_or(16).max(16);
        let globals = match lighting.named("LightingGlobals", targets) {
            Some(Res::Buffer(b)) => b,
            _ => return Err("LightingGlobals not made".into()),
        };
        let view_of = |n: &str| match lighting.named(n, targets) {
            Some(Res::View(v)) => Some(v),
            _ => None,
        };

        // The sun march.
        let t = techs[sun_name].clone();
        let pass = CodePass::new(gpu, &t, &[wgpu::TextureFormat::Rgba16Float], &[TEX_POST_EFFECT1])?;
        let march_scene = gpu.device.create_buffer(&wgpu::BufferDescriptor { label: Some("t7 volumetric march PerSceneConsts"), size: scene_size(&t), usage: u, mapped_at_creation: false });
        let mut samplers = std::collections::HashMap::new();
        for s in pass.sampler_states() {
            samplers.entry(s).or_insert(gpu.device.create_sampler(&crate::sampler::descriptor(s, crate::sampler::ANISO_QUALITY, border)?));
        }
        let shadow = view_of("gSunShadowmapArray").ok_or("gSunShadowmapArray not made")?;
        let cookie = view_of("gCookieArray").ok_or("gCookieArray not made")?;
        let groups = pass.bind_full(
            gpu,
            &march_scene,
            &post_fx,
            &[(2, globals)],
            &[(BUFFER_LIT_FOG, &lit_fog)],
            &buffers,
            |id| match id {
                TEX_POST_EFFECT1 => Some(&rt25.view),
                TEX_POST_EFFECT_SRC => Some(&vol.view),
                TEX_CODE3 | TEX_CODE4 | TEX_CODE6 => Some(&white.view),
                TEX_CODE5 => Some(&rt27.view),
                TEX_SUN_SHADOW => Some(shadow),
                TEX_COOKIE => Some(cookie),
                _ => None,
            },
            |s| samplers.get(&s),
            |n| lighting.named_sampler(n),
        )?;
        let sun = March { pass, tech: t, groups };

        // The apply.
        let t = techs[APPLY].clone();
        let pass = CodePass::new_ext(gpu, &t, &[crate::lighting::SCENE_FORMAT], None, &[TEX_CODE0, TEX_CODE1], true)?;
        let apply_scene = gpu.device.create_buffer(&wgpu::BufferDescriptor { label: Some("t7 volumetric apply PerSceneConsts"), size: scene_size(&t), usage: u, mapped_at_creation: false });
        let mut near = std::collections::HashMap::new();
        for s in pass.sampler_states() {
            near.entry(s).or_insert(nearest(gpu, s, border)?);
        }
        let floatz = view_of("GBufferDepth").ok_or("GBufferDepth not made")?;
        let groups = pass.bind_full(
            gpu,
            &apply_scene,
            &post_fx,
            &[],
            &[],
            &buffers,
            |id| match id {
                TEX_POST_EFFECT0 => Some(&rt28.view),
                TEX_CODE0 => Some(&rt26.view),
                TEX_CODE1 => Some(floatz),
                TEX_FEEDBACK => Some(&feedback.view),
                _ => None,
            },
            |s| near.get(&s),
            |_| None,
        )?;
        let apply = March { pass, tech: t, groups };

        let tiles = [w.div_ceil(64), h.div_ceil(64)];
        let sampler_of = |s: &crate::compute::Slot| match s.kind {
            crate::compute::SlotKind::Sampler { .. } => lighting.named_sampler(&s.name).map(Res::Sampler),
            _ => None,
        };
        // The local-light march (flag 2).
        let local = if !want_local {
            None
        } else {
            let [vis_lights, _] = visible.ok_or("the local-light march: the forward light culling is not made")?;
            let lists = [tile_list(gpu, "visibleVolumetricSpotLights", tiles), tile_list(gpu, "visibleVolumetricOmniLights", tiles)];
            let p = Program::new(gpu, cs("volumetric_light_cull_compute")?, &[(0, "r32uint"), (1, "r32uint")])?;
            let g = p.bind(gpu, |s| match s.name.as_str() {
                "visibleLights" => Some(Res::View(vis_lights)),
                "visibleVolumetricSpotLights" => Some(Res::View(&lists[0].view)),
                "visibleVolumetricOmniLights" => Some(Res::View(&lists[1].view)),
                n => lighting.named(n, targets),
            })?;
            let name = if d.skip_samples { LIGHT_MARCH } else { LIGHT_MARCH_ALL };
            let t = crate::post_pass::code_techs(&patch.image, &patch.capture, &[name])?[name].clone();
            let pass = CodePass::new(gpu, &t, &[wgpu::TextureFormat::Rgba16Float], &[TEX_POST_EFFECT1])?;
            let scene = gpu.device.create_buffer(&wgpu::BufferDescriptor { label: Some("t7 volumetric light march PerSceneConsts"), size: scene_size(&t), usage: u, mapped_at_creation: false });
            for st in pass.sampler_states() {
                samplers.entry(st).or_insert(gpu.device.create_sampler(&crate::sampler::descriptor(st, crate::sampler::ANISO_QUALITY, border)?));
            }
            let res = |n: &str| match lighting.named(n, targets) {
                Some(Res::Buffer(b)) => Some(b),
                _ => None,
            };
            let records = lighting.light_records();
            let cull_constants = res("gCullConstants").ok_or("gCullConstants not made")?;
            let spot_shadow = view_of("gSpotShadowmapArray").ok_or("gSpotShadowmapArray not made")?;
            let groups = pass.bind_full(
                gpu,
                &scene,
                &post_fx,
                &[(2, globals)],
                &[(BUFFER_LIT_FOG, &lit_fog), (BUFFER_LIGHT_RECORDS, records), (BUFFER_CULL_CONSTANTS, cull_constants)],
                &buffers,
                |id| match id {
                    TEX_POST_EFFECT1 => Some(&rt25.view),
                    TEX_POST_EFFECT_SRC => Some(&vol.view),
                    TEX_CODE3 | TEX_CODE4 | TEX_CODE6 => Some(&white.view),
                    TEX_CODE5 => Some(&rt27.view),
                    TEX_VISIBLE_SPOT => Some(&lists[0].view),
                    TEX_VISIBLE_OMNI => Some(&lists[1].view),
                    TEX_SPOT_SHADOW => Some(spot_shadow),
                    TEX_SUN_SHADOW => Some(shadow),
                    TEX_COOKIE => Some(cookie),
                    _ => None,
                },
                |s| samplers.get(&s),
                |n| lighting.named_sampler(n),
            )?;
            Some(Local { cull: (p, g), _lists: lists, march: March { pass, tech: t, groups }, scene, tiles })
        };
        // The 3-D volume (flag 4).
        let volume = if !want_volume {
            None
        } else {
            let [_, vis_probes] = visible.ok_or("the 3-D volume: the forward light culling is not made")?;
            let list = tile_list(gpu, "visibleVolumetricProbes", tiles);
            let p = Program::new(gpu, cs("volumetric_probe_cull_compute")?, &[(0, "r32uint")])?;
            let g = p.bind(gpu, |s| match s.name.as_str() {
                "visibleProbes" => Some(Res::View(vis_probes)),
                "visibleVolumetricProbes" => Some(Res::View(&list.view)),
                n => lighting.named(n, targets),
            })?;
            let cp = Program::new_with(gpu, cs("volumetric_light_compute")?, &[(0, "rgba16float")], &[0])?;
            let scene_bytes = cp.args.iter().filter(|a| a.ty & 0xff00 == 0x400 && a.buffer == 1).map(|a| u64::from(a.location) + u64::from(a.size)).max().unwrap_or(16).max(720);
            let scene = gpu.device.create_buffer(&wgpu::BufferDescriptor { label: Some("t7 volumetric light volume PerSceneConsts"), size: scene_bytes, usage: u, mapped_at_creation: false });
            let cg = cp.bind(gpu, |s| match s.name.as_str() {
                "PerSceneConsts" => Some(Res::Buffer(&scene)),
                "PostFxCBuffer" => Some(Res::Buffer(&post_fx)),
                "postEffect1" => Some(Res::View(&rt25.view)),
                // (Code texture 0x52 is the volumetric probe list here.)
                "visibleProbes" => Some(Res::View(&list.view)),
                "postEffectSrc" => Some(Res::View(&vol.view)),
                n => lighting.named(n, targets).or_else(|| sampler_of(s)),
            })?;
            Some(Volume3d { cull: (p, g), _list: list, compute: (cp, cg), scene, size: vsize, tiles })
        };
        let tri = |label: &str, w: u32, h: u32| gpu.device.create_buffer_init(&wgpu::util::BufferInitDescriptor { label: Some(label), contents: &crate::post_pass::triangle(w, h), usage: wgpu::BufferUsages::VERTEX });
        report.push(format!(
            "t7 volumetrics: GPU passes made for {w}x{h} (quarter {qw}x{qh}): downsample_depth_with_uv_4x_compute, downsample_depth_4x_compute, {sun_name}, volumetric_light_hblur/vblur_compute, {APPLY}; local-light march {}; 3-D volume {}; not built: 3DVolume / unified modes, half-resolution apply",
            if local.is_some() { "made (volumetric_light_cull_compute, volumetric_light_skip_samples)".to_owned() } else { format!("not made (no volume with flag 0x40; map flags {map_flags:#x})") },
            if volume.is_some() { format!("made ({}x{}x{}: volumetric_probe_cull_compute, volumetric_light_compute)", vsize[0], vsize[1], vsize[2]) } else { "not made (no volume with flag 0x80)".to_owned() }
        ));
        let rt28_view = rt28.texture.create_view(&Default::default());
        Ok(Self {
            gpu: gpu.clone(),
            size: [w, h],
            quarter: [qw, qh],
            with_uv,
            down,
            blur,
            _rt: [rt25, rt26, rt27, rt28, rt29],
            rt28_view,
            feedback,
            sun,
            apply,
            local,
            volume,
            _vol: vol,
            _white: white,
            _buffers: buffers,
            lit_fog,
            post_fx,
            _depth_consts: depth_consts,
            march_scene,
            apply_scene,
            quarter_tri: tri("t7 volumetric quarter triangle", qw, qh),
            full_tri: tri("t7 volumetric triangle", w, h),
            plan: Vec::new(),
            prepass: Vec::new(),
            frames: 0,
            report,
        })
    }

    /// The view's list and constants for this frame (after the lighting's
    /// `update`, which packs the list).
    pub fn update(&mut self, lighting: &Lighting, camera: &Camera, now: i32) {
        let d = Dvars::current();
        let p: &Packed = &lighting.lit_fog_packed;
        let mut plan = crate::volumetric::plan(&d, p);
        let mut prepass = crate::volumetric::prepass(&d, p);
        // (What is not made is left out of the frame: the unified mode,
        // the half-resolution apply; the local-light and 3-D parts on a map
        // without their volume flags, which cannot pack them.)
        let (has_local, has_volume) = (self.local.is_some(), self.volume.is_some());
        let skipped: Vec<&str> = plan
            .iter()
            .chain(prepass.iter())
            .copied()
            .filter(|n| match *n {
                "volumetric_light_skip_samples" | "volumetric_light" | "volumetric_light_cull_compute" => !has_local,
                "volumetric_probe_cull_compute" | "volumetric_light_compute" => !has_volume,
                "volumetric_light_unified" | "volumetric_light_upsample" | "volumetric_light_half_res_apply" => true,
                _ => false,
            })
            .collect();
        plan.retain(|n| !skipped.contains(n));
        prepass.retain(|n| !skipped.contains(n));
        if self.frames == 0 || plan != self.plan {
            self.report.push(format!(
                "t7 volumetrics: frame {}: {} lit fog volumes packed (volumes {:?}, flags {:#x}); drawn: {}{}{}",
                self.frames,
                p.count,
                p.volumes,
                p.flags,
                if prepass.is_empty() { String::new() } else { format!("pre-pass {}; ", prepass.join(" -> ")) },
                if plan.is_empty() { "nothing".to_owned() } else { plan.join(" -> ") },
                if skipped.is_empty() { String::new() } else { format!("; not built, left out: {}", skipped.join(", ")) }
            ));
        }
        self.plan = plan;
        self.prepass = prepass;
        self.frames += 1;
        if self.plan.is_empty() {
            return;
        }
        let q = &self.gpu.queue;
        q.write_buffer_p(&self.lit_fog, 0, &p.records);
        let vc = lighting.view_consts(camera, now);
        let vm = vc.get(crate::lighting_globals::VIEW_MATRIX).map(<[_]>::to_vec).unwrap_or_default();
        let pm = vc.get(crate::lighting_globals::PROJECTION_MATRIX).map(<[_]>::to_vec).unwrap_or_default();
        let c = crate::volumetric::constants(&vm, &pm, camera.eye.to_array(), self.size[0], self.quarter[0], p, &d);
        q.write_buffer_p(&self.post_fx, 0, bytemuck::cast_slice(&c));
        let mut missing = Vec::new();
        let mut draws: Vec<(&[fastfile_t7::walk::capture::ShaderArg], &wgpu::Buffer, [u32; 2])> = vec![(&self.sun.tech.args[..], &self.march_scene, self.quarter), (&self.apply.tech.args[..], &self.apply_scene, self.size)];
        if let Some(l) = &self.local {
            draws.push((&l.march.tech.args[..], &l.scene, self.quarter));
        }
        if let Some(v) = &self.volume {
            // (Its target size is the quarter depth's: it maps a texel to
            // quarter texels and divides by it.)
            draws.push((&v.compute.0.args[..], &v.scene, self.quarter));
        }
        for (args, buf, [w, h]) in draws {
            let mut consts: CodeConsts = vc.clone();
            let r = crate::post_pass::per_scene(w, h);
            consts.set(crate::code::VIEW_PROJECTION_MATRIX, r[8..12].to_vec());
            consts.set(crate::code::EYE_OFFSET, vec![[0.0; 4]]);
            // `zNear` (0x15): the march reads its x (the view's near plane).
            consts.set(0x15, vec![[camera.near, 0.0, 0.0, 0.0]]);
            let (fw, fh) = (w as f32, h as f32);
            consts.set(crate::lighting_globals::RENDER_TARGET_SIZE, vec![[fw, fh, 1.0 / fw, 1.0 / fh]]);
            let mut bytes = vec![0u8; buf.size() as usize];
            crate::lighting::fill_code(&mut bytes, args, &consts, &mut missing);
            q.write_buffer_p(buf, 0, &bytes);
        }
        if self.frames == 1 && !missing.is_empty() {
            missing.sort();
            missing.dedup();
            self.report.push(format!("t7 volumetrics: code constants missing: {}", missing.join(", ")));
        }
    }

    /// Whether this frame draws anything.
    pub fn active(&self) -> bool {
        !self.plan.is_empty()
    }

    fn dispatch(enc: &mut wgpu::CommandEncoder, label: &str, p: &(Program, Vec<wgpu::BindGroup>), threads: [u32; 3]) {
        let mut pass = enc.compute_pass_p(&wgpu::ComputePassDescriptor { label: Some(label), timestamp_writes: None });
        pass.set_pipeline_p(&p.0.pipeline);
        for (k, g) in p.1.iter().enumerate() {
            pass.set_bind_group_p(k as u32, g, &[]);
        }
        let [x, y, z] = p.0.groups_for(threads);
        pass.dispatch_p(x, y, z);
    }

    /// The depth pre-pass (before the opaque forward lists).
    pub fn record_prepass(&self, enc: &mut wgpu::CommandEncoder) {
        if self.plan.is_empty() {
            return;
        }
        Self::dispatch(enc, "t7 volumetric depth pre-pass", &self.with_uv, [self.quarter[0], self.quarter[1], 1]);
        if let Some(l) = self.local.as_ref().filter(|_| self.prepass.contains(&"volumetric_light_cull_compute")) {
            Self::dispatch(enc, "t7 volumetric light culling", &l.cull, [l.tiles[0], l.tiles[1], 16]);
        }
        if let Some(v) = self.volume.as_ref().filter(|_| self.prepass.contains(&"volumetric_light_compute")) {
            Self::dispatch(enc, "t7 volumetric probe culling", &v.cull, [v.tiles[0], v.tiles[1], 16]);
            // (Threads: the volume's texels, 8 in z: a group of 4 lights 32
            // slices, 4 at a time.)
            Self::dispatch(enc, "t7 volumetric light volume", &v.compute, [v.size[0], v.size[1], v.size[2] >> 3]);
        }
    }

    /// The march, blur and apply into the scene (after view list 20).
    pub fn record(&self, enc: &mut wgpu::CommandEncoder, scene: &wgpu::TextureView, scene_texture: &wgpu::Texture) {
        if self.plan.is_empty() {
            return;
        }
        let [qw, qh] = self.quarter;
        Self::dispatch(enc, "t7 volumetric floatZDownscaled", &self.down, [qw, qh, 1]);
        {
            // (Render target 0x28 cleared to zero before the march.)
            enc.render_pass_p(&wgpu::RenderPassDescriptor {
                label: Some("t7 volumetric clear"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment { view: &self.rt28_view, depth_slice: None, resolve_target: None, ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT), store: wgpu::StoreOp::Store } })],
                ..Default::default()
            });
        }
        if self.plan.iter().any(|n| n.starts_with("volumetric_light_sun")) {
            self.sun.pass.draw(enc, &[&self.rt28_view], &self.sun.groups, &self.quarter_tri);
        }
        if let Some(l) = self.local.as_ref().filter(|_| self.plan.iter().any(|n| matches!(*n, LIGHT_MARCH | LIGHT_MARCH_ALL))) {
            l.march.pass.draw(enc, &[&self.rt28_view], &l.march.groups, &self.quarter_tri);
        }
        if self.plan.contains(&"volumetric_light_hblur_compute") {
            Self::dispatch(enc, "t7 volumetric hblur", &self.blur[0], [qw, qh, 1]);
            Self::dispatch(enc, "t7 volumetric vblur", &self.blur[1], [qw, qh, 1]);
        }
        enc.copy_texture_to_texture(scene_texture.as_image_copy(), self.feedback.texture.as_image_copy(), self.feedback.texture.size());
        self.apply.pass.draw(enc, &[scene], &self.apply.groups, &self.full_tri);
    }

    /// Render target 0x28 (the blurred march; checks).
    pub fn light_texture(&self) -> &wgpu::Texture {
        &self._rt[3].texture
    }
}
