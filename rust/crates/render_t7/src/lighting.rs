//! The deferred lighting stage as the exe records it (`141d5ca10`, after
//! the G-buffer): the uber resolve (`deferred_permute_uber_resolve_gtao`,
//! `141d5bf40`: per-tile depth extents), the permute clear, the tile
//! classification (`deferred_permute_classify_swizzle_no_sun`: shadow mode
//! 0), the indirect-argument build (`deferred_permute_draw`) and the 64
//! indirect dispatches of `deferred_lighting_permuted(N)` (argument offset
//! `12·N`), writing the lit scene target (code texture 0xa).
//!
//! The exe's scene target is R11G11B10_FLOAT; wgpu cannot write that format
//! from a compute shader, so it is RGBA16F here (more precision, same
//! values). What the stage does not do yet is listed in
//! [`crate::lighting_globals::GAPS`] and in [`Lighting::report`].

#[allow(unused_imports)]
use crate::profile::{ComputeExt as _, EncExt as _, PassExt as _, QueueExt as _};
use std::collections::HashMap;
use std::path::Path;

use render_h1::code::Camera;
use render_h1::gpu::Gpu;

use crate::code::CodeConsts;
use crate::compute::{code_shaders, CodeShader, Program, Res, Slot, SlotKind};
use crate::gbuffer::Targets;
use crate::gpu::{self, Tex};
use crate::lighting_globals::{self as globals, Counts};
use crate::scene::Scene;

pub const SCENE_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba16Float;
pub const PERMUTATIONS: u32 = 64;
const PER_SCENE: u64 = 1488;
/// Cull bit words per 8x8 tile in `gDeferredCullBits`: the classify indexes
/// a tile's words from `tile << 5` (32 words a tile, the first 16 written:
/// `index < (tile << 5) | 16`), and the many-light permutations read them
/// back the same way. A buffer of 16 words a tile held only the first half
/// of the frame's tiles: the lower half's words were dropped or read as
/// garbage on the PC (the hard black band across the lower half of the
/// frame, blocky wrong light), while a software device read them as zero.
const CULL_WORDS_EXE: u64 = 32;
/// `T7_TEST_CULL_WORDS=16`: the old half-size cull bits, to prove the audit
/// (`T7_AUDIT`) reports an allocation smaller than the shaders index.
fn cull_words() -> u64 {
    std::env::var("T7_TEST_CULL_WORDS").ok().and_then(|v| v.parse().ok()).unwrap_or(CULL_WORDS_EXE)
}

/// Everything sized by the frame.
struct Sized {
    width: u32,
    height: u32,
    tiles: u32,
    scene: Tex,
    resolved_normal: Tex,
    /// The depth pyramid ring (exe images `0x14a4fe868`, three per view by
    /// frame: `(frame + 2) % 3` is this frame's): mips DS2x..DS64x, all
    /// mips viewed and each mip alone.
    ring: [(Tex, Vec<wgpu::TextureView>); 3],
    /// `DS8xCopy` (render target 0x1f), which `gtao_2` reads as `DS8x`.
    ds8x_copy: Tex,
    depth_view: wgpu::TextureView,
    /// bo2mp: the G-buffer depth in the exe's encoding, which every
    /// lighting-stage program reads as `GBufferDepth` ([`ExeDepth`]).
    exe_depth: Option<ExeDepth>,
    extents: wgpu::Buffer,
    tiling: wgpu::Buffer,
    cull_bits: wgpu::Buffer,
    /// bo2mp: `gDownsampledScene` (code texture 0x63, render target role
    /// view +0xb0e2e): the quarter-resolution scene the post chain's
    /// `bloom_downsample_l1_scene` writes as its third target, read by the
    /// next frame's SSR permutations.
    down_scene: Tex,
    /// Per program, per frame slot `k = frame % 6`: the depth ring's slot
    /// `(k + 2) % 3` and the motion vectors' parity `k & 1`.
    groups: Vec<(usize, [Vec<wgpu::BindGroup>; GROUP_SLOTS])>,
}

/// bo2mp: `GBufferDepth` as BlackOps3.exe's programs decode it. They take
/// the view depth as `z = 63 / (64·d)` below `63/64` (the world; `1 / (64·d
/// − 63)` above, the view model) and rebuild the eye-relative position as
/// `z · (ndc.x · invP00, ndc.y · invP11, 1)` (DXBC of
/// `deferred_lighting_permuted`, `deferred_permute_uber_resolve_gtao`),
/// while our depth is `near / z` (reversed, infinite). Read raw (before
/// 2026-10-28) every lit point sat `64·near/63` (about 4) times nearer the
/// eye than it is: the sun shadow lookup (and the fog, probe volume and
/// local lights) read the world at the wrong place, a place that moved with
/// the camera, so on every BO3 map with sun shadows the shadows slid with the
/// player and fell on the wrong surfaces. This writes `d · 63 / (64·near)`
/// into an R32Float target each frame, before the lighting stage (as the
/// forward lists' `floatZ`, `crate::forward`). `T7_RAW_GBUFFER_DEPTH=1`:
/// the old raw binding, for before/after checks.
pub(crate) struct ExeDepth {
    pipeline: wgpu::RenderPipeline,
    k: wgpu::Buffer,
    pub(crate) out: Tex,
    group: wgpu::BindGroup,
}

const EXE_DEPTH: &str = r#"
@group(0) @binding(0) var depth: texture_depth_2d;
struct K { k: vec4<f32> }
@group(0) @binding(1) var<uniform> k: K;
@vertex fn vs(@builtin(vertex_index) i: u32) -> @builtin(position) vec4<f32> {
    let p = vec2<f32>(f32((i << 1u) & 2u), f32(i & 2u));
    return vec4<f32>(p * 2.0 - 1.0, 0.0, 1.0);
}
@fragment fn fs(@builtin(position) p: vec4<f32>) -> @location(0) vec4<f32> {
    let d = textureLoad(depth, vec2<i32>(p.xy), 0);
    return vec4<f32>(d * k.k.x, 0.0, 0.0, 0.0);
}
"#;

/// The scale [`ExeDepth`] applies to our `near / z` depth.
pub fn exe_depth_scale(near: f32) -> f32 {
    63.0 / (64.0 * near.max(1e-3))
}

fn raw_gbuffer_depth() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var("T7_RAW_GBUFFER_DEPTH").is_ok_and(|v| !v.is_empty() && v != "0"))
}

impl ExeDepth {
    fn new(gpu: &Gpu, w: u32, h: u32, depth: &wgpu::TextureView) -> Self {
        let dev = &gpu.device;
        let m = dev.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some("GBufferDepth (exe encoding)"), source: wgpu::ShaderSource::Wgsl(EXE_DEPTH.into()) });
        let pipeline = dev.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("GBufferDepth (exe encoding)"),
            layout: None,
            vertex: wgpu::VertexState { module: &m, entry_point: Some("vs"), compilation_options: Default::default(), buffers: &[] },
            primitive: Default::default(),
            depth_stencil: None,
            multisample: Default::default(),
            fragment: Some(wgpu::FragmentState { module: &m, entry_point: Some("fs"), compilation_options: Default::default(), targets: &[Some(wgpu::TextureFormat::R32Float.into())] }),
            multiview_mask: None,
            cache: crate::pipecache::get(&dev).as_ref(),
        });
        let k = crate::audit::buffer(dev, &wgpu::BufferDescriptor { label: Some("GBufferDepth scale"), size: 16, usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST, mapped_at_creation: false });
        gpu.queue.write_buffer_p(&k, 0, bytemuck::cast_slice(&[exe_depth_scale(4.0), 0.0, 0.0, 0.0f32]));
        let out = crate::gpu::target(gpu, "GBufferDepth (exe encoding)", w.max(1), h.max(1), wgpu::TextureFormat::R32Float, wgpu::TextureUsages::TEXTURE_BINDING);
        let group = dev.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("GBufferDepth (exe encoding)"),
            layout: &pipeline.get_bind_group_layout(0),
            entries: &[wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(depth) }, wgpu::BindGroupEntry { binding: 1, resource: k.as_entire_binding() }],
        });
        Self { pipeline, k, out, group }
    }

    fn record(&self, enc: &mut wgpu::CommandEncoder) {
        let mut pass = enc.render_pass_p(&wgpu::RenderPassDescriptor {
            label: Some("GBufferDepth (exe encoding)"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment { view: &self.out.view, depth_slice: None, resolve_target: None, ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT), store: wgpu::StoreOp::Store } })],
            ..Default::default()
        });
        pass.set_pipeline_p(&self.pipeline);
        pass.set_bind_group_p(0, &self.group, &[]);
        pass.draw_p(0..3, 0..1);
    }
}

/// Bind group sets per program: the depth ring's three slots by the
/// motion vectors' two parities.
const GROUP_SLOTS: usize = 6;

/// bo2mp: screen-space reflections, the exe's `deferred_lighting_permuted_ssr`
/// set (exe `141d5ca10`: the SSR set when its dvar is on and the view has
/// motion vectors; the dvar's registered default is 0, the PC settings turn
/// it on). Opt-in until seen on the PC: `BO2MP_T7_SSR=1`.
/// `T7_NO_LIGHTS` (read once: the frame's path asks every frame).
fn no_lights() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var("T7_NO_LIGHTS").is_ok())
}

/// `T7_NO_PROBES` (read once).
fn no_probes() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var("T7_NO_PROBES").is_ok())
}

pub fn ssr_on() -> bool {
    std::env::var("BO2MP_T7_SSR").is_ok_and(|v| v.trim() == "1")
}

#[allow(dead_code)]
/// A permuted dispatch split into pieces of at most `piece` thread groups.
/// The exe's `deferred_permute_draw` writes one `(tiles, 1, 1)` argument per
/// permutation (one group a tile); D3D11 and Vulkan take 65535 groups in a
/// dimension, which a 4K frame passes (480 x 270 tiles of 8 x 8 pixels in
/// one permutation). Past the limit a driver drops or wraps the dispatch, so
/// the tiles listed last (the lower part of the frame) were never lit.
struct Split {
    /// Groups at most in a piece (the device's limit).
    piece: u32,
    pieces: u32,
    /// `[permutation][k - 1]`: the pipeline of piece k >= 1 (piece 0 is the
    /// program's own).
    pipelines: Vec<Vec<wgpu::ComputePipeline>>,
    /// `(piece * 64 + permutation)` arguments: `min(piece size, what is left
    /// of the permutation's tile count)` groups.
    args: wgpu::Buffer,
    make_args: wgpu::ComputePipeline,
    make_args_group: wgpu::BindGroup,
}

const SPLIT_ARGS: &str = r"
override PIECES: u32 = 1u;
override PIECE: u32 = 65535u;
@group(0) @binding(0) var<storage, read> src: array<u32>;
@group(0) @binding(1) var<storage, read_write> dst: array<u32>;
@compute @workgroup_size(64, 1, 1)
fn main(@builtin(global_invocation_id) g: vec3<u32>) {
    let n = g.x;
    if (n >= 64u) { return; }
    let count = src[n * 3u];
    for (var k = 0u; k < PIECES; k = k + 1u) {
        let first = k * PIECE;
        var rest = 0u;
        if (count > first) { rest = min(count - first, PIECE); }
        let o = (k * 64u + n) * 3u;
        dst[o] = rest;
        dst[o + 1u] = 1u;
        dst[o + 2u] = 1u;
    }
}
";

/// The map's light data on the GPU.
struct MapLight {
    env_brdf: Tex,
    probes: Tex,
    volume: [Tex; 3],
}

pub struct Lighting {
    /// The view's `relHDRExposure` (set by the host from the post chain's
    /// eye exposure before `update`; (1, 1, 0, 0) until then).
    pub rel_hdr_exposure: [f32; 4],
    gpu: Gpu,
    programs: Vec<Program>,
    resolve: usize,
    gtao_2: usize,
    clear: usize,
    classify: usize,
    draw: usize,
    permuted: Vec<usize>,
    /// bo2mp: the SSR permutation set is drawn ([`ssr_on`]).
    ssr: bool,
    per_scene: wgpu::Buffer,
    /// Every program's code-constant arguments (constant buffer 1), merged
    /// once, and the bytes they are written into each frame.
    code_args: Vec<fastfile_t7::walk::capture::ShaderArg>,
    per_scene_bytes: Vec<u8>,
    globals: wgpu::Buffer,
    light_pack: wgpu::Buffer,
    probe_pack: wgpu::Buffer,
    blend_pack: wgpu::Buffer,
    cull_constants: wgpu::Buffer,
    sun_tree: wgpu::Buffer,
    accumulate: wgpu::Buffer,
    indirect: wgpu::Buffer,
    /// The permuted dispatches in pieces, when a permutation can need more
    /// thread groups than the device dispatches in one dimension.
    split: Option<Split>,
    light: MapLight,
    none: None_,
    samplers: HashMap<&'static str, wgpu::Sampler>,
    sized: Option<Sized>,
    frames_updated: u64,
    /// The lights and probes packed for the frame (the forward lists'
    /// light culling dispatches by them).
    pub counts: Counts,
    /// The packers' volumetric bit buffers (`volumetricSpotLightBits`,
    /// `volumetricOmniLightBits`, `volumetricProbeBits`: 16 words each, one
    /// per 32 listed lights / probes) and the packed light records as a
    /// structured buffer (code buffer 1, the volumetric light march's
    /// `t12`; the same records as `LightingPackCBuffer`).
    volumetric_bits: [wgpu::Buffer; 3],
    light_records: wgpu::Buffer,
    /// The AO pass (`crate::ao`), when the quality dvar runs one.
    ao: Option<crate::ao::Ao>,
    /// Static motion vectors (render targets 0x16/0x17), drawn after the
    /// resolve and read by the AO's temporal denoisers.
    motion: Option<crate::motion::Motion>,
    /// The map's inputs to the per-frame constants.
    probe0: Option<Vec<u8>>,
    sun: fastfile_t7::native::ViewSun,
    mode: i32,
    fog_src: Option<fastfile_t7::fog::FogSources>,
    /// The world's weather grime volumes (`LightingGlobals.weather`).
    pub grime: crate::weather::Grime,
    fog_light: Option<fastfile_t7::native::GlobalLightSettings>,
    fog: fastfile_t7::fog::FogTransitions,
    /// The map's lit fog volumes (records, planes), their client bank
    /// states, and the view's packed list (`crate::volumetric`).
    lit_fog: (Option<(usize, usize)>, Option<(usize, usize)>),
    lit_fog_states: crate::volumetric::BankStates,
    pub lit_fog_packed: crate::volumetric::Packed,
    /// The fog and lit fog records the sources above are into, copied out
    /// of the map zone ([`frame_records`]).
    kept: fastfile_t7::kept::KeptBytes,
    lights: crate::lights::MapLights,
    /// All probe configs (0x148 each; probe 0 the map-wide one).
    probe_configs: Vec<u8>,
    probe_blends: Vec<u8>,
    /// The sun shadow cascades (shadow mode 3), when the map's tree is read.
    pub sun_shadows: Option<crate::sun_shadow::SunShadows>,
    /// The local lights' shadows (`crate::spot_shadow`), when on at load.
    pub spot: Option<crate::spot_shadow::SpotShadows>,
    /// What was made, what is missing, line by line (taken by the host).
    pub report: Vec<String>,
}

/// The resources bound where the stage draws nothing yet (shadow maps,
/// cookies, AO), so the programs bind; each listed in the report.
struct None_ {
    depth_array: Tex,
    sun_shadow: Tex,
    sun_shadow_max: Tex,
    depth_cube_array: Tex,
    colour_array: Tex,
    white: Tex,
    /// Zero motion (`gMotionVectors` without the motion pass).
    black: Tex,
    /// `gCookieArray` as loaded (`cookie_array`).
    cookie: Option<Tex>,
}

fn texture(gpu: &Gpu, label: &str, size: wgpu::Extent3d, dim: wgpu::TextureDimension, format: wgpu::TextureFormat, levels: u32, view: wgpu::TextureViewDimension) -> Tex {
    let texture = crate::audit::texture(&gpu, &wgpu::TextureDescriptor {
        label: Some(label),
        size,
        mip_level_count: levels,
        sample_count: 1,
        dimension: dim,
        format,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    let view = texture.create_view(&wgpu::TextureViewDescriptor { dimension: Some(view), ..Default::default() });
    Tex { texture, view }
}

/// A depth pyramid: six mips from `w` × `h` (DS2x), storage-written per
/// mip, sampled whole.
fn depth_ring(gpu: &Gpu, label: &str, w: u32, h: u32) -> (Tex, Vec<wgpu::TextureView>) {
    let levels = 6.min(32 - w.max(h).leading_zeros());
    let texture = crate::audit::texture(&gpu, &wgpu::TextureDescriptor {
        label: Some(label),
        size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
        mip_level_count: levels,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba16Float,
        usage: wgpu::TextureUsages::STORAGE_BINDING | wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let view = texture.create_view(&Default::default());
    let mips = (0..6).map(|k| texture.create_view(&wgpu::TextureViewDescriptor { base_mip_level: k.min(levels - 1), mip_level_count: Some(1), ..Default::default() })).collect();
    (Tex { texture, view }, mips)
}

fn storage_target(gpu: &Gpu, label: &str, w: u32, h: u32, format: wgpu::TextureFormat) -> Tex {
    let texture = crate::audit::texture(&gpu, &wgpu::TextureDescriptor {
        label: Some(label),
        size: wgpu::Extent3d { width: w.max(1), height: h.max(1), depth_or_array_layers: 1 },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        // (RENDER_ATTACHMENT: the forward lists draw into the lit scene.)
        usage: wgpu::TextureUsages::STORAGE_BINDING | wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_SRC | wgpu::TextureUsages::COPY_DST | wgpu::TextureUsages::RENDER_ATTACHMENT,
        view_formats: &[],
    });
    let view = texture.create_view(&Default::default());
    Tex { texture, view }
}

fn zeroed(gpu: &Gpu, label: &str, size: u64, usage: wgpu::BufferUsages) -> wgpu::Buffer {
    crate::audit::buffer(&gpu.device, &wgpu::BufferDescriptor { label: Some(label), size: size.max(16), usage, mapped_at_creation: false })
}

fn sampler(gpu: &Gpu, label: &str, mode: wgpu::AddressMode, compare: Option<wgpu::CompareFunction>) -> wgpu::Sampler {
    gpu.device.create_sampler(&wgpu::SamplerDescriptor {
        label: Some(label),
        address_mode_u: mode,
        address_mode_v: mode,
        address_mode_w: mode,
        mag_filter: wgpu::FilterMode::Linear,
        min_filter: wgpu::FilterMode::Linear,
        mipmap_filter: if compare.is_some() { wgpu::MipmapFilterMode::Nearest } else { wgpu::MipmapFilterMode::Linear },
        compare,
        ..Default::default()
    })
}

/// `gCookieArray` (code texture 7) as BlackOps3.exe binds it: the world's
/// draw image (`GfxWorld` +0x260), a 2-D array whose layers are the sun's
/// cloud layer (sun config 0x50, layer + 1) and the lights' cookies. Its
/// pixels as stored: the zone's, else the top streamed part, every level
/// holding every layer (level-major, as the probe arrays are stored).
/// Returns the texture, its layer count and a description for the log.
fn cookie_array(gpu: &Gpu, map: &fastfile_t7::map::T7Map, name: &str) -> Result<(Tex, u32, String), String> {
    let rec = map.image_info(name).ok_or_else(|| format!("{name}: no image record"))?.clone();
    let format = render_h1::gpu::dxgi_format(rec.format).ok_or_else(|| format!("{name}: DXGI {} not drawn", rec.format))?;
    if format.is_compressed() && !gpu.bc {
        return Err(format!("{name}: the device has no BC texture support"));
    }
    let layers = u32::from(rec.depth.max(1));
    let (w, h, data, from) = if rec.xpak.is_empty() {
        let im = map.image(name, u32::MAX)?;
        (im.width, im.height, im.data, "zone".to_owned())
    } else {
        let p = &rec.xpak[0];
        let d = map.paks().ok_or_else(|| format!("{name}: no packages"))?.read(p.key).map_err(|e| format!("{name}: {e}"))?;
        (u32::from(p.width), u32::from(p.height), d, format!("streamed part 0 of {}", rec.xpak.len()))
    };
    let (bw, bh) = format.block_dimensions();
    let bsize = format.block_copy_size(None).ok_or("format size")?;
    let level = |l: u32| -> (u32, u32, usize) {
        let (lw, lh) = ((w >> l).max(1), (h >> l).max(1));
        (lw, lh, (lw.div_ceil(bw) * lh.div_ceil(bh) * bsize) as usize)
    };
    // The levels the bytes hold for every layer.
    let mut levels = 0;
    let mut want = 0usize;
    while levels < 32 - w.max(h).leading_zeros() && want + level(levels).2 * layers as usize <= data.len() {
        want += level(levels).2 * layers as usize;
        levels += 1;
    }
    let what = format!("{name}: {w}x{h}, {layers} layers, {levels} levels of {} recorded, DXGI {}, {} bytes ({from})", rec.levels, rec.format, data.len());
    if levels == 0 {
        return Err(format!("{what}: no level fits"));
    }
    let t = texture(gpu, "gCookieArray", wgpu::Extent3d { width: w.div_ceil(bw) * bw, height: h.div_ceil(bh) * bh, depth_or_array_layers: layers }, wgpu::TextureDimension::D2, format, levels, wgpu::TextureViewDimension::D2Array);
    let mut at = 0usize;
    for l in 0..levels {
        let (lw, lh, n) = level(l);
        gpu.queue.write_texture_p(
            wgpu::TexelCopyTextureInfo { texture: &t.texture, mip_level: l, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
            &data[at..at + n * layers as usize],
            wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(lw.div_ceil(bw) * bsize), rows_per_image: Some(lh.div_ceil(bh)) },
            wgpu::Extent3d { width: lw.div_ceil(bw) * bw, height: lh.div_ceil(bh) * bh, depth_or_array_layers: layers },
        );
        at += n * layers as usize;
    }
    Ok((t, layers, what))
}

/// The reflection probes' cube array (BC6H, as stored).
fn probe_array(gpu: &Gpu, map: &fastfile_t7::map::T7Map) -> Result<Tex, String> {
    let (p, data) = fastfile_t7::light::probe_cubes(map)?;
    let format = render_h1::gpu::dxgi_format(p.format).ok_or_else(|| format!("reflection probes: DXGI {} not drawn", p.format))?;
    if !gpu.bc {
        return Err("reflection probes: the device has no BC texture support".into());
    }
    let layers = 6 * p.count;
    let t = texture(gpu, "gReflectionProbeArray", wgpu::Extent3d { width: p.size, height: p.size, depth_or_array_layers: layers }, wgpu::TextureDimension::D2, format, p.levels, wgpu::TextureViewDimension::CubeArray);
    let bsize = format.block_copy_size(None).ok_or("format size")?;
    let mut at = 0usize;
    for l in 0..p.levels {
        let s = (p.size >> l).max(1);
        let blocks = s.div_ceil(4);
        let n = (blocks * blocks * bsize) as usize * layers as usize;
        let Some(level) = data.get(at..at + n) else { return Err(format!("reflection probes: {} bytes, level {l} wants {}", data.len(), at + n)) };
        gpu.queue.write_texture_p(
            wgpu::TexelCopyTextureInfo { texture: &t.texture, mip_level: l, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
            level,
            wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(blocks * bsize), rows_per_image: Some(blocks) },
            wgpu::Extent3d { width: blocks * 4, height: blocks * 4, depth_or_array_layers: layers },
        );
        at += n;
    }
    Ok(t)
}

fn probe_volume(gpu: &Gpu, map: &fastfile_t7::map::T7Map) -> Result<[Tex; 3], String> {
    let names = ["gProbeXArray", "gProbeYArray", "gProbeZArray"];
    // As stored (BC6H 3-D) where the device takes block-compressed 3-D
    // textures; else decoded to RGBA16F (8 bytes a texel: large volumes
    // take much memory that way).
    if gpu.bc && gpu.device.features().contains(wgpu::Features::TEXTURE_COMPRESSION_BC_SLICED_3D) {
        let (v, data) = fastfile_t7::light::probe_volume_raw(map)?;
        let size = wgpu::Extent3d { width: v.width as u32, height: v.height as u32, depth_or_array_layers: v.depth as u32 };
        let (bw, bh) = (size.width / 4, size.height / 4);
        let one = (bw * bh * 16) as usize * v.depth;
        let mk = |i: usize| {
            let t = texture(gpu, names[i], size, wgpu::TextureDimension::D3, wgpu::TextureFormat::Bc6hRgbUfloat, 1, wgpu::TextureViewDimension::D3);
            gpu.queue.write_texture_p(t.texture.as_image_copy(), &data[i * one..(i + 1) * one], wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(bw * 16), rows_per_image: Some(bh) }, size);
            t
        };
        return Ok([mk(0), mk(1), mk(2)]);
    }
    let (v, axes) = fastfile_t7::light::probe_volume_rgba16f(map)?;
    let size = wgpu::Extent3d { width: v.width as u32, height: v.height as u32, depth_or_array_layers: v.depth as u32 };
    let mk = |i: usize| {
        let t = texture(gpu, names[i], size, wgpu::TextureDimension::D3, wgpu::TextureFormat::Rgba16Float, 1, wgpu::TextureViewDimension::D3);
        gpu.queue.write_texture_p(
            t.texture.as_image_copy(),
            bytemuck::cast_slice(&axes[i]),
            wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(size.width * 8), rows_per_image: Some(size.height) },
            size,
        );
        t
    };
    Ok([mk(0), mk(1), mk(2)])
}

fn one_texel(gpu: &Gpu, label: &str, format: wgpu::TextureFormat, layers: u32, view: wgpu::TextureViewDimension, value: &[u8]) -> Tex {
    let size = wgpu::Extent3d { width: 1, height: 1, depth_or_array_layers: layers };
    let t = texture(gpu, label, size, wgpu::TextureDimension::D2, format, 1, view);
    if !format.is_depth_stencil_format() {
        let data: Vec<u8> = value.iter().copied().cycle().take(value.len() * layers as usize).collect();
        gpu.queue.write_texture_p(t.texture.as_image_copy(), &data, wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(value.len() as u32), rows_per_image: Some(1) }, size);
    }
    t
}

/// An empty sun shadow map (mode 2 binds no drawn map): depth and
/// per-region maximum zero. (Checked on mp_nuketown_x: 0 or 1 in either
/// gives the same tile classification and the same lit scene, as mode 2's
/// constants put every pixel outside the cascades.)
/// bo2mp: the records [`Lighting`] reads each view (the fog sources' and
/// the lit fog volumes and planes), copied out of the map zone so the zone
/// can be let go (`T7Map::release_zone`), with the sources at their offsets
/// in the copy.
pub(crate) fn frame_records(image: &[u8], fog: Option<fastfile_t7::fog::FogSources>, lit_fog: (Option<(usize, usize)>, Option<(usize, usize)>)) -> (fastfile_t7::kept::KeptBytes, Option<fastfile_t7::fog::FogSources>, (Option<(usize, usize)>, Option<(usize, usize)>)) {
    let mut spans = fog.map_or_else(Vec::new, |f| f.kept_spans(image));
    spans.extend(lit_fog.0.map(|(at, n)| (at, crate::volumetric::VOLUME.saturating_mul(n))));
    spans.extend(lit_fog.1.map(|(at, n)| (at, 16usize.saturating_mul(n))));
    let kept = fastfile_t7::kept::KeptBytes::new(image, spans);
    let fog = fog.map(|f| f.rebased(&kept));
    let lit_fog = (kept.span(lit_fog.0), kept.span(lit_fog.1));
    (kept, fog, lit_fog)
}

fn empty_sun_shadow() -> (f32, f32) {
    (0.0, 0.0)
}

/// A 1x1 one-layer depth array cleared to `depth`.
fn cleared_depth_array(gpu: &Gpu, label: &str, depth: f32) -> Tex {
    let texture = crate::audit::texture(&gpu, &wgpu::TextureDescriptor {
        label: Some(label),
        size: wgpu::Extent3d { width: 1, height: 1, depth_or_array_layers: 1 },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Depth32Float,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::RENDER_ATTACHMENT,
        view_formats: &[],
    });
    let mut enc = gpu.device.create_command_encoder(&Default::default());
    {
        let at = texture.create_view(&wgpu::TextureViewDescriptor { dimension: Some(wgpu::TextureViewDimension::D2), ..Default::default() });
        let _ = enc.render_pass_p(&wgpu::RenderPassDescriptor {
            label: Some(label),
            color_attachments: &[],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment { view: &at, depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(depth), store: wgpu::StoreOp::Store }), stencil_ops: None }),
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
    }
    gpu.queue.submit([enc.finish()]);
    let view = texture.create_view(&wgpu::TextureViewDescriptor { dimension: Some(wgpu::TextureViewDimension::D2Array), ..Default::default() });
    Tex { texture, view }
}

impl Lighting {
    /// Makes the programs (from `core_patch` in `zone_dir`) and uploads the
    /// map's light (environment BRDF, reflection probes, probe volume).
    pub fn new(gpu: &Gpu, scene: &Scene, zone_dir: &Path) -> Result<Self, String> {
        let mut report = Vec::new();
        let patch = fastfile_t7::shared::zone(zone_dir, "core_patch").ok_or("core_patch.ff not read")?;
        let shaders: HashMap<String, CodeShader> = code_shaders(&patch.image, &patch.capture);
        // Shadow mode (`141ddd1b0`): 0 without a sun, else 3 with cascaded
        // sun shadow maps or 2 without. A map whose tree is not read (or a
        // device that cannot bind it) is lit the mode 2 way.
        let sun_shadowed = matches!(scene.map.zone_data().and_then(|(img, cap)| cap.world.gfx_worlds.first().and_then(|g| g.sun_volumes.first().copied()).and_then(|v| fastfile_t7::native::global_light_settings(&img, v, 0))), Some(s) if fastfile_t7::native::view_sun(&s).shadow_mode != 0);
        // (`T7_NO_SUN_SHADOW=1`: the cascades left out, lit the mode 2 way.)
        let sun_shadows = if sun_shadowed && (std::env::var("T7_NO_SUN_SHADOW").is_ok_and(|v| v != "0") || !crate::quality::get().sun_shadows && std::env::var("T7_NO_SUN_SHADOW").is_err()) {
            report.push("sun shadows: left out (T7_NO_SUN_SHADOW or the quality preset); lit as shadow mode 2".into());
            None
        } else if sun_shadowed {
            match crate::sun_shadow::SunShadows::new(gpu, &scene.map, &patch.image, &patch.capture, &shaders) {
                Ok(s) => {
                    report.extend(s.report.iter().cloned());
                    Some(s)
                }
                Err(e) => {
                    report.push(format!("sun shadows: not drawn ({e}); lit as shadow mode 2"));
                    None
                }
            }
        } else {
            None
        };
        let mode = if !sun_shadowed { 0 } else if sun_shadows.is_some() { 3 } else { 2 };
        // (The programs are made side by side first; the calls below then
        // take them from the memo. The load may have made them already.)
        crate::prewarm::lighting_programs(gpu, &shaders, Some(mode));
        let mut programs = Vec::new();
        let mut make_as = |name: &str, unfilterable: &[u32], split: bool| -> Result<usize, String> {
            let cs = shaders.get(name).ok_or_else(|| format!("{name}: not in core_patch"))?;
            programs.push(if split { Program::new_split(gpu, cs, &[], unfilterable)? } else { Program::new_with(gpu, cs, &[], unfilterable)? });
            Ok(programs.len() - 1)
        };
        let mut make = |name: &str, unfilterable: &[u32]| make_as(name, unfilterable, false);
        // Depth (t1 in the resolve, t4 in the lighting) is read with Load.
        let resolve = make("deferred_permute_uber_resolve_gtao", &[1])?;
        // The resolve's second dispatch (DS8x → DS16x, DS32x, DS64x).
        let gtao_2 = make("deferred_permute_uber_resolve_gtao_2", &[])?;
        let clear = make("deferred_permute_clear", &[])?;
        let classify = make(if mode == 0 { "deferred_permute_classify_swizzle_no_sun" } else { "deferred_permute_classify_swizzle" }, &[])?;
        let draw = make("deferred_permute_draw", &[])?;
        let mut permuted = Vec::new();
        let ssr = ssr_on();
        for n in 0..PERMUTATIONS {
            // (Split: one group a tile, so a permutation can need more
            // groups than the device dispatches in one dimension.)
            // (The SSR set: the same programs plus the reflection march,
            // reading gDownsampledScene, gMotionVectors, gSSAODepthTexture.)
            let name = if ssr { format!("deferred_lighting_permuted_ssr({n})") } else { format!("deferred_lighting_permuted({n})") };
            permuted.push(make_as(&name, &[4], true)?);
        }
        if ssr {
            report.push(format!("t7 ssr: deferred_lighting_permuted_ssr(0..{}) made (BO2MP_T7_SSR=1); the reflections march the depth pyramid and fetch last frame's quarter-res scene (bloom_downsample_l1_scene's third target) through the motion vectors", PERMUTATIONS - 1));
        }
        report.push(format!("lighting: {} programs made; sun shadow mode {mode}", programs.len()));

        let map = &scene.map;
        if let Some(r) = map.light_refs() {
            if let Some(p) = &r.probes {
                report.push(format!("lighting: reflection probes {} x {}px, {} levels, DXGI {}", p.count, p.size, p.levels, p.format));
            }
            if let Some(v) = &r.volume {
                report.push(format!("lighting: probe volume {}x{}x{}", v.width, v.height, v.depth));
            }
        }
        let env_brdf = map
            .image("$env_brdf_generic", 4096)
            .map_err(|e| format!("$env_brdf_generic: {e}"))
            .and_then(|im| gpu::upload(gpu, "gEnvBRDFGeneric", &im))?;
        // (`CODLEGACY_T7_DEBUG_SURF=no_reflect`: one black cube, to tell a probe
        // reflection from the surface's own colour.)
        // (`T7_NO_PROBE_ARRAY`: offline checks on a device that cannot hold
        // the map's array.)
        let probes = match if crate::surf_debug::get().no_reflect {
            Err("CODLEGACY_T7_DEBUG_SURF=no_reflect".to_owned())
        } else if std::env::var("T7_NO_PROBE_ARRAY").is_ok() {
            Err("reflection probes: skipped (T7_NO_PROBE_ARRAY)".to_owned())
        } else {
            probe_array(gpu, map)
        } {
            Ok(t) => t,
            Err(e) => {
                report.push(format!("lighting: {e}; the probe array is one black cube"));
                one_texel(gpu, "gReflectionProbeArray", wgpu::TextureFormat::Rgba16Float, 6, wgpu::TextureViewDimension::CubeArray, &[0; 8])
            }
        };
        let volume = match probe_volume(gpu, map) {
            Ok(v) => v,
            Err(e) => {
                report.push(format!("lighting: {e}; the probe volume is black"));
                let b = |n| texture(gpu, n, wgpu::Extent3d { width: 1, height: 1, depth_or_array_layers: 1 }, wgpu::TextureDimension::D3, wgpu::TextureFormat::Rgba16Float, 1, wgpu::TextureViewDimension::D3);
                [b("gProbeXArray"), b("gProbeYArray"), b("gProbeZArray")]
            }
        };
        let probe_configs = map.light_refs().and_then(|r| r.probes.as_ref()).map(|p| p.configs.clone()).unwrap_or_default();
        let probe_blends = map.light_refs().and_then(|r| r.probes.as_ref()).map(|p| p.blends.clone()).unwrap_or_default();
        report.push(format!("lighting: {} local reflection probes and {} probe blends (packed per frame)", (probe_configs.len() / 328).saturating_sub(1), probe_blends.len() / 0x25c));
        let probe0 = map.light_refs().and_then(|r| r.probes.as_ref()).and_then(|p| p.configs.get(..328).map(<[u8]>::to_vec));
        if probe0.is_none() {
            report.push("lighting: no global probe config; globalProbeConstants zero".into());
        }

        // The sun and fog inputs: the first sun volume, lighting state 0.
        let (zone_image, capture) = map.zone_data().ok_or("the map's zone data is not kept")?;
        let world = capture.world.gfx_worlds.first();
        let fog_light = world.and_then(|g| g.sun_volumes.first()).and_then(|&v| fastfile_t7::native::global_light_settings(&zone_image, v, 0));
        let sun_volume = world.and_then(|g| g.sun_volumes.first().copied());
        let sun = match (fog_light.as_ref(), sun_volume) {
            (Some(s), Some(v)) => fastfile_t7::native::view_sun(s).with_volume(&zone_image, v),
            _ => Default::default(),
        };
        let mut sun = sun;
        // The cookie array (`GfxWorld` +0x260) and the sun's cloud layer in
        // it (sun constants word 24 = config 0x50: layer + 1, 0 none; the
        // programs sample layer − 1 through the cloud transform, words
        // 28..35, and blend `1 + (cookie − 1)·config 0x40`).
        let cookie = match world.and_then(|g| g.cookie_image.clone()) {
            None => {
                report.push("t7 cloud shadows: the world has no cookie array image".into());
                None
            }
            Some(n) => match cookie_array(gpu, map, &n) {
                Ok((t, layers, what)) => {
                    report.push(format!("t7 cloud shadows: gCookieArray {what}"));
                    Some((t, layers))
                }
                Err(e) => {
                    report.push(format!("t7 cloud shadows: gCookieArray not loaded: {e} (bound as one white layer)"));
                    None
                }
            },
        };
        // (Bound only with `T7_CLOUD_SHADOWS=1`. The spot lights' cookie
        // byte (record word 0x3b, the light's word +0x230: layer + 1, 0
        // none) matches the array on every staged map (its largest value
        // is the array's layer count), but nothing drawn has been seen: no
        // staged map's sun (Sector, Apartments, Nuk3town, Skyjacked,
        // Mountain) has a cloud layer, and offline the map lights light
        // nothing, so neither the cloud nor a cookie can be read back.
        // Without it the array is bound as one white layer and the cloud
        // layer cleared. `T7_CLOUD_SHADOWS=0` is that too.)
        let cloud_on = std::env::var("T7_CLOUD_SHADOWS").is_ok_and(|v| v.trim() == "1");
        let cookie = if cloud_on { cookie } else { None };
        let cloud = sun.config[0x50 / 4];
        let f = |k: usize| f32::from_bits(sun.config[k / 4]);
        match (cloud, cookie.as_ref()) {
            (0, _) => report.push(format!("t7 cloud shadows: the map's sun has no cloud layer (config 0x50 = 0): none drawn, as the exe; cookie array {}", if cloud_on { "bound (the spot lights' cookies; T7_CLOUD_SHADOWS=1)" } else { "not bound (opt-in T7_CLOUD_SHADOWS=1: unverified drawn)" })),
            (c, Some((_, layers))) if c <= *layers => report.push(format!("t7 cloud shadows: on: cookie array layer {} of {layers}, intensity (config 0x40) {:.3}, config 0x44..0x4c {:.4} {:.4} {:.4}", c - 1, f(0x40), f(0x44), f(0x48), f(0x4c))),
            (c, _) => {
                report.push(format!("t7 cloud shadows: off: the sun's cloud layer {c} {}", if cloud_on { "has no cookie array layer" } else { "is off (opt-in T7_CLOUD_SHADOWS=1: unverified drawn)" }));
                sun.config[0x50 / 4] = 0;
            }
        }
        if fog_light.is_none() {
            report.push("lighting: no sun volume; sun constants zero".into());
        }
        let lights = crate::lights::MapLights::read(&zone_image, &capture);
        // (`BO2MP_T7_SPOT_SHADOWS=0`, or no shadowed light at the preset:
        // the atlas is not made and the lights are packed unshadowed.)
        let q = crate::quality::get();
        let spot = (crate::spot_shadow::enabled() && q.spot_shadows > 0 && q.local_lights).then(|| crate::spot_shadow::SpotShadows::new(gpu, q.spot_shadow_size, scene.instances.len()));
        report.push(match &spot {
            Some(_) => format!("t7 spot shadows: on, up to {} lights a view, atlas {} px (spot tiles {} px, omni {} px); {} of the map's {} lights can cast (BO2MP_T7_SPOT_SHADOWS=0: off)", q.spot_shadows, q.spot_shadow_size, q.spot_shadow_size / 8, q.spot_shadow_size / 4, lights.shadowed(), lights.len()),
            None => "t7 spot shadows: off (BO2MP_T7_SPOT_SHADOWS=0, or the preset's spot shadows 0)".into(),
        });
        report.push(format!("lighting: {} map lights (packed per frame; no light animation yet)", lights.len()));
        report.push(format!("t7 cloud shadows: spot light cookie layers (light word +0x230: layer, lights): {:?}", lights.cookie_layers()));
        let grime = world.map_or_else(crate::weather::Grime::default, |g| crate::weather::Grime::new(&zone_image, g.grime_volumes, &g.grime_images, g.grime_planes));
        report.push(format!("lighting: {} weather grime volumes", grime.len()));
        let fog_src = world.map(|g| fastfile_t7::fog::FogSources {
            world_header: g.header_at,
            fog_volumes: g.fog_volumes,
            fog_planes: g.fog_planes,
            modifiers: g.fog_modifiers,
            modifier_planes: g.fog_modifier_planes,
        });

        let lit_fog = (world.and_then(|g| g.lit_fog_volumes), world.and_then(|g| g.lit_fog_planes));
        let lit_fog_states = lit_fog.0.map_or_else(Default::default, |vols| crate::volumetric::BankStates::new(&crate::volumetric::Volumes { image: &zone_image, volumes: vols, planes: lit_fog.1 }));
        report.push(format!("t7 volumetrics: {} lit fog volumes in the map (bank 0 each, as at map load)", lit_fog_states.0.len()));
        if let Some((at, n)) = lit_fog.0 {
            for i in 0..n {
                let r = at + crate::volumetric::VOLUME * i;
                let f = |o: usize| zone_image.get(r + o..r + o + 4).map_or(0.0, |b| f32::from_le_bytes(b.try_into().unwrap()));
                let flags = zone_image.get(r + 0x2c..r + 0x30).map_or(0, |b| u32::from_le_bytes(b.try_into().unwrap()));
                report.push(format!("t7 volumetrics: volume {i}: flags {flags:#x}, box {:?} .. {:?}", [f(8), f(12), f(16)], [f(20), f(24), f(28)]));
            }
        }
        let (kept, fog_src, lit_fog) = frame_records(&zone_image, fog_src, lit_fog);

        let (sun_depth, sun_max) = empty_sun_shadow();
        let none = None_ {
            sun_shadow: cleared_depth_array(gpu, "gSunShadowmapArray (empty)", sun_depth),
            sun_shadow_max: one_texel(gpu, "gSunShadowMax (empty)", wgpu::TextureFormat::R16Float, 1, wgpu::TextureViewDimension::D2, &fastfile_t7::light::f16_bits(sun_max).to_le_bytes()),
            depth_array: one_texel(gpu, "shadow map (none)", wgpu::TextureFormat::Depth32Float, 1, wgpu::TextureViewDimension::D2Array, &[]),
            depth_cube_array: one_texel(gpu, "omni shadow map (none)", wgpu::TextureFormat::Depth32Float, 6, wgpu::TextureViewDimension::CubeArray, &[]),
            colour_array: one_texel(gpu, "cookie / trans shadow (none)", wgpu::TextureFormat::Rgba8Unorm, 1, wgpu::TextureViewDimension::D2Array, &[255; 4]),
            cookie: cookie.map(|c| c.0),
            white: one_texel(gpu, "SSAO (none)", wgpu::TextureFormat::Rgba8Unorm, 1, wgpu::TextureViewDimension::D2, &[255; 4]),
            black: one_texel(gpu, "motion (none)", wgpu::TextureFormat::Rgba8Unorm, 1, wgpu::TextureViewDimension::D2, &[0; 4]),
        };
        report.push("lighting: not drawn yet, bound empty: spot/omni/trans shadow maps".into());
        let motion = match crate::motion::Motion::new(gpu, &patch.image, &patch.capture) {
            Ok(m) => {
                report.push(format!("motion vectors: motion_vector_static into {:?}", m.format));
                Some(m)
            }
            Err(e) => {
                report.push(format!("motion vectors: not made: {e}"));
                None
            }
        };
        let (quality, aa_mode) = crate::ao::dvars();
        // (The temporal denoisers need the motion vectors: without them the
        // spatial path, Ultra, runs instead.)
        let quality = if motion.is_none() && std::env::var("T7_SSAO_QUALITY").is_err() && crate::ao::choose(quality, aa_mode, 1).is_some_and(|c| c.temporal) { 5 } else { quality };
        let ao = match crate::ao::choose(quality, aa_mode, 1) {
            None => {
                report.push(format!("ao: quality {quality}: not run (gSSAOTexture is render target 0x19, cleared to 1)"));
                None
            }
            Some(c) => match crate::ao::Ao::new(gpu, &patch.image, &patch.capture, c.clone(), motion.is_some()) {
                Ok(a) => {
                    report.push(format!("ao: quality {quality}, AA mode {aa_mode}: {} + {}", c.gtao, c.denoisers.join(" / ")));
                    Some(a)
                }
                Err(e) => {
                    report.push(format!("ao: quality {quality}: not run: {e} (gSSAOTexture bound to 1)"));
                    None
                }
            },
        };
        report.extend(globals::GAPS.iter().map(|g| format!("lighting gap: {g}")));

        let mut code_args: Vec<fastfile_t7::walk::capture::ShaderArg> = Vec::new();
        for p in &programs {
            for a in p.args.iter().filter(|a| a.ty & 0xff00 == 0x400 && a.buffer == 1) {
                if !code_args.iter().any(|b| b.u[0] == a.u[0] && b.location == a.location && b.size == a.size) {
                    code_args.push(a.clone());
                }
            }
        }
        // (Storage too: the forward programs read the light and probe packs
        // as structured buffers, `lightConstants` / `refProbeConstants`.)
        let u = wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST;
        let s = wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::COPY_SRC;
        let mut samplers = HashMap::new();
        samplers.insert("samp0", sampler(gpu, "samp0", wgpu::AddressMode::ClampToEdge, None));
        // The SSR set's depth pyramid sampler: the exe binds sampler state
        // 0x2a9 at s1 with the SSR inputs (`141d5ba00`).
        let point_clamp = crate::sampler::descriptor(0x2a9, crate::sampler::ANISO_QUALITY, gpu.device.features().contains(wgpu::Features::ADDRESS_MODE_CLAMP_TO_BORDER)).map(|d| gpu.device.create_sampler(&d)).unwrap_or_else(|_| sampler(gpu, "samplerPointClamp", wgpu::AddressMode::ClampToEdge, None));
        samplers.insert("samplerPointClamp", point_clamp);
        samplers.insert("samplerLinear", sampler(gpu, "samplerLinear", wgpu::AddressMode::Repeat, None));
        samplers.insert("samplerLinearClamp", sampler(gpu, "samplerLinearClamp", wgpu::AddressMode::ClampToEdge, None));
        // Reversed depth: nearer is greater.
        samplers.insert("ShadowSamplerState", sampler(gpu, "ShadowSamplerState", wgpu::AddressMode::ClampToEdge, None));
        samplers.insert("ShadowSamplerComparisonState", sampler(gpu, "ShadowSamplerComparisonState", wgpu::AddressMode::ClampToEdge, Some(wgpu::CompareFunction::GreaterEqual)));
        Ok(Self {
            rel_hdr_exposure: [1.0, 1.0, 0.0, 0.0],
            gpu: gpu.clone(),
            programs,
            resolve,
            gtao_2,
            clear,
            classify,
            draw,
            permuted,
            ssr,
            per_scene: zeroed(gpu, "PerSceneConsts", PER_SCENE, u),
            code_args,
            per_scene_bytes: vec![0; PER_SCENE as usize],
            globals: zeroed(gpu, "LightingGlobals", globals::SIZE as u64, u | wgpu::BufferUsages::COPY_SRC),
            light_pack: zeroed(gpu, "LightingPackCBuffer", 65520, u),
            probe_pack: zeroed(gpu, "RefProbePackCBuffer", 61152, u),
            blend_pack: zeroed(gpu, "RefProbeBlendPackCBuffer", 65472, u),
            cull_constants: zeroed(gpu, "gCullConstants", (crate::lights::CULL * (crate::lights::MAX_LIGHTS + 0x111)) as u64, s),
            // (Zero, and room for the index the programs compute from the
            // zeroed shadow constants: an index past a 16-byte buffer reads
            // whatever the device's bounds handling gives.)
            sun_tree: zeroed(gpu, "gSunShadowTree", 65536, s),
            accumulate: zeroed(gpu, "gPermuteAccumulateBuffer", 4 * u64::from(PERMUTATIONS), s),
            indirect: zeroed(gpu, "gPermutedIndirectBuffer", 12 * u64::from(PERMUTATIONS), s | wgpu::BufferUsages::INDIRECT),
            split: None,
            light: MapLight { env_brdf, probes, volume },
            none,
            samplers,
            sized: None,
            frames_updated: 0,
            counts: Counts::default(),
            volumetric_bits: [zeroed(gpu, "volumetricSpotLightBits", 64, s), zeroed(gpu, "volumetricOmniLightBits", 64, s), zeroed(gpu, "volumetricProbeBits", 64, s)],
            light_records: zeroed(gpu, "t7 light records (code buffer 1)", 65520, s),
            ao,
            motion,
            probe0,
            sun,
            mode,
            fog_src,
            grime,
            fog_light,
            fog: Default::default(),
            lit_fog,
            lit_fog_states,
            lit_fog_packed: Default::default(),
            kept,
            lights,
            probe_configs,
            probe_blends,
            sun_shadows,
            spot,
            report,
        })
    }

    /// The map's lit fog volumes (`crate::volumetric`).
    pub fn lit_fog_volume_count(&self) -> usize {
        self.lit_fog_states.0.len()
    }

    /// The packed light records as a structured buffer (code buffer 1).
    /// bo2mp: the SSR set's `gDownsampledScene`, for the post chain's
    /// `bloom_downsample_l1_scene` to write (None without SSR).
    /// bo2mp: the motion blur velocity of this frame (role `0xb0e5c`),
    /// when the view blurs (`crate::post_mblur`).
    pub fn blur_velocity(&self) -> Option<&wgpu::TextureView> {
        self.motion.as_ref()?.blur_velocity()
    }

    pub fn downsampled_scene(&self) -> Option<&wgpu::TextureView> {
        if !self.ssr {
            return None;
        }
        self.sized.as_ref().map(|s| &s.down_scene.view)
    }

    /// The view's `LightingGlobals` (the G-buffer copies it each frame).
    pub fn globals_buffer(&self) -> &wgpu::Buffer {
        &self.globals
    }

    pub fn light_records(&self) -> &wgpu::Buffer {
        &self.light_records
    }

    /// The OR of the map's lit fog volumes' flag words (0x40: lit by local
    /// lights, 0x80: the 3-D volume), with `T7_VOLUMETRIC_TEST`'s bits.
    pub fn lit_fog_flags(&self) -> u32 {
        let Some((at, n)) = self.lit_fog.0 else { return 0 };
        let mut f = (0..n).fold(0, |f, i| f | self.kept.bytes().get(at + crate::volumetric::VOLUME * i + 0x2c..at + crate::volumetric::VOLUME * i + 0x30).map_or(0, |b| u32::from_le_bytes(b.try_into().unwrap())));
        if crate::volumetric::test_mode() {
            f |= 0xc0;
        }
        f & 0xffff
    }

    /// The lit scene (code texture 0xa), once a frame has been sized.
    pub fn scene_target(&self) -> Option<&wgpu::TextureView> {
        self.sized.as_ref().map(|s| &s.scene.view)
    }

    /// The AO the lighting read last frame and GTAO's raw output (for
    /// checks).
    pub fn ao_textures(&self) -> Option<(&wgpu::Texture, &wgpu::Texture)> {
        let r = self.frames_updated.saturating_sub(1).wrapping_add(2) as usize % 3;
        self.ao.as_ref()?.textures(r)
    }

    /// The lit scene texture (for copies and readback).
    pub fn scene_texture(&self) -> Option<&wgpu::Texture> {
        self.sized.as_ref().map(|s| &s.scene.texture)
    }

    /// The tile lists and the cull bits (`debug_views`).
    pub fn debug_buffers(&self) -> Option<(&wgpu::Buffer, &wgpu::Buffer)> {
        self.sized.as_ref().map(|s| (&s.tiling, &s.cull_bits))
    }

    /// The view's relHDRExposure as (scale, 1 / scale).
    pub fn exposure_scale(&self) -> (f32, f32) {
        (self.rel_hdr_exposure[0], self.rel_hdr_exposure[1])
    }

    /// The per-permutation tile counts and the indirect arguments (for
    /// checks).
    pub fn permute_buffers(&self) -> (&wgpu::Buffer, &wgpu::Buffer, Option<&wgpu::Buffer>) {
        (&self.accumulate, &self.indirect, self.sized.as_ref().map(|s| &s.extents))
    }

    /// Makes the frame-sized buffers and targets and binds every program to
    /// `targets` (the G-buffer).
    pub fn prepare(&mut self, targets: &Targets) -> Result<(), String> {
        let gpu = &self.gpu;
        let (w, h) = (targets.width, targets.height);
        let tiles = w.div_ceil(8) * h.div_ceil(8);
        let s = wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::COPY_SRC;
        // The resolve's depth pyramid ring (read by GTAO and the denoiser).
        let (hw, hh) = crate::ao::Ao::half(w, h);
        let ring = std::array::from_fn(|i| depth_ring(gpu, &format!("depth pyramid {i} (DS2x..DS64x)"), hw, hh));
        let mut sized = Sized {
            width: w,
            height: h,
            tiles,
            scene: storage_target(gpu, "scene (lit)", w, h, SCENE_FORMAT),
            resolved_normal: storage_target(gpu, "gResolvedNormalAndGloss", w, h, wgpu::TextureFormat::Rgba16Float),
            ring,
            ds8x_copy: storage_target(gpu, "DS8xCopy (rt 0x1f)", w.div_ceil(8), h.div_ceil(8), wgpu::TextureFormat::Rgba16Float),
            depth_view: targets.depth.texture.create_view(&wgpu::TextureViewDescriptor { aspect: wgpu::TextureAspect::DepthOnly, ..Default::default() }),
            exe_depth: (!raw_gbuffer_depth()).then(|| ExeDepth::new(gpu, w, h, &targets.depth.texture.create_view(&wgpu::TextureViewDescriptor { aspect: wgpu::TextureAspect::DepthOnly, ..Default::default() }))),
            extents: zeroed(gpu, "gPermutedExtentsBuffer", 8 * u64::from(tiles), s),
            tiling: zeroed(gpu, "gPermutedTilingBuffer", 8 * u64::from(tiles) * u64::from(PERMUTATIONS), s),
            cull_bits: zeroed(gpu, "gDeferredCullBits", 4 * cull_words() * u64::from(tiles), s),
            down_scene: crate::gpu::target(gpu, "gDownsampledScene (role 0xb0e2e)", (w >> 2).max(1), (h >> 2).max(1), wgpu::TextureFormat::Rgba16Float, wgpu::TextureUsages::COPY_SRC),
            groups: Vec::new(),
        };
        if let Some(m) = self.motion.as_mut() {
            let depth = targets.depth.texture.create_view(&wgpu::TextureViewDescriptor { aspect: wgpu::TextureAspect::DepthOnly, ..Default::default() });
            if let Err(e) = m.prepare(w, h, &depth) {
                self.report.push(format!("motion vectors: not bound: {e}"));
                self.motion = None;
            }
        }
        if let Some(ao) = self.ao.as_mut() {
            let d = [&sized.ring[0].0.view, &sized.ring[1].0.view, &sized.ring[2].0.view];
            if let Err(e) = ao.prepare(w, h, &targets.colour[1].view, d, self.motion.as_ref().and_then(|m| m.targets())) {
                self.report.push(format!("ao: not bound: {e}"));
                self.ao = None;
            }
        }
        let mut groups = Vec::new();
        for (i, p) in self.programs.iter().enumerate() {
            let mut per = Vec::new();
            for k in 0..GROUP_SLOTS {
                per.push(p.bind(gpu, |slot| self.resource(slot, targets, &sized, (k + 2) % 3, k & 1))?);
            }
            groups.push((i, per.try_into().map_err(|_| "bind groups")?));
        }
        sized.groups = groups;
        let tiles = sized.tiles;
        self.sized = Some(sized);
        self.split = None;
        // (`T7_MAX_GROUPS`: a smaller limit, to check the split offline.)
        let forced = std::env::var("T7_MAX_GROUPS").ok().and_then(|v| v.parse::<u32>().ok()).filter(|&v| v > 0);
        let limit = forced.unwrap_or(u32::MAX).min(self.gpu.device.limits().max_compute_workgroups_per_dimension).clamp(1, 65535);
        let pieces = tiles.div_ceil(limit);
        self.report.push(format!(
            "lighting: {w}x{h}: {tiles} tiles of 8x8, {} bytes of tile lists, {} of cull bits; the device takes {limit} thread groups a dimension: {} piece(s) a permutation",
            8 * u64::from(tiles) * u64::from(PERMUTATIONS),
            4 * cull_words() * u64::from(tiles),
            pieces.max(1)
        ));
        let binding_limit = u64::from(self.gpu.device.limits().max_storage_buffer_binding_size);
        for (name, bytes) in [("gPermutedTilingBuffer", 8 * u64::from(tiles) * u64::from(PERMUTATIONS)), ("gDeferredCullBits", 4 * cull_words() * u64::from(tiles)), ("gPermutedExtentsBuffer", 8 * u64::from(tiles))] {
            if bytes > binding_limit {
                self.report.push(format!("lighting: WARNING {name} is {bytes} bytes, over the device's storage binding limit {binding_limit}: its tiles past the limit are not lit"));
            }
        }
        if pieces > 1 {
            match self.make_split(limit, pieces) {
                Ok(sp) => self.split = Some(sp),
                Err(e) => self.report.push(format!("lighting: WARNING split dispatch not made: {e}; permutations over {limit} tiles are not fully lit")),
            }
        }
        Ok(())
    }

    /// The pipelines and arguments of the split permuted dispatch.
    fn make_split(&self, piece: u32, pieces: u32) -> Result<Split, String> {
        let gpu = &self.gpu;
        let dev = &gpu.device;
        let mut pipelines = Vec::new();
        for &p in &self.permuted {
            let mut per = Vec::new();
            for k in 1..pieces {
                per.push(self.programs[p].piece(gpu, k, piece)?);
            }
            pipelines.push(per);
        }
        let args = crate::audit::buffer(&dev, &wgpu::BufferDescriptor {
            label: Some("gPermutedIndirectBuffer (pieces)"),
            size: 12 * u64::from(PERMUTATIONS) * u64::from(pieces),
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::INDIRECT | wgpu::BufferUsages::COPY_SRC,
            mapped_at_creation: false,
        });
        let module = dev.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some("t7 split args"), source: wgpu::ShaderSource::Wgsl(SPLIT_ARGS.into()) });
        let entry = |binding, read_only| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::COMPUTE,
            ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Storage { read_only }, has_dynamic_offset: false, min_binding_size: None },
            count: None,
        };
        let layout = dev.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor { label: Some("t7 split args"), entries: &[entry(0, true), entry(1, false)] });
        let pl = dev.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: Some("t7 split args"), bind_group_layouts: &[Some(&layout)], immediate_size: 0 });
        let constants = [("PIECES", f64::from(pieces)), ("PIECE", f64::from(piece))];
        let make_args = dev.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("t7 split args"),
            layout: Some(&pl),
            module: &module,
            entry_point: Some("main"),
            compilation_options: wgpu::PipelineCompilationOptions { constants: &constants, ..Default::default() },
            cache: crate::pipecache::get(&dev).as_ref(),
        });
        let make_args_group = dev.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("t7 split args"),
            layout: &layout,
            entries: &[wgpu::BindGroupEntry { binding: 0, resource: self.indirect.as_entire_binding() }, wgpu::BindGroupEntry { binding: 1, resource: args.as_entire_binding() }],
        });
        Ok(Split { piece, pieces, pipelines, args, make_args, make_args_group })
    }

    fn resource<'a>(&'a self, slot: &Slot, targets: &'a Targets, sized: &'a Sized, r: usize, parity: usize) -> Option<Res<'a>> {
        let mip = |k: usize| Res::View(&sized.ring[r].1[k]);
        let v = Res::View;
        let b = Res::Buffer;
        Some(match (slot.name.as_str(), &slot.kind) {
            ("PerSceneConsts", _) => b(&self.per_scene),
            ("LightingGlobals", _) => b(&self.globals),
            ("LightingPackCBuffer", _) => b(&self.light_pack),
            ("RefProbePackCBuffer", _) => b(&self.probe_pack),
            ("RefProbeBlendPackCBuffer", _) => b(&self.blend_pack),
            ("GBufferDiffuseColor", _) => v(&targets.colour[0].view),
            ("GBufferNormalAndGloss" | "gGBufferNormalAndGloss", _) => v(&targets.colour[1].view),
            ("GBufferSpecularColorAndOcclusion", _) => v(&targets.colour[2].view),
            ("GBufferDepth" | "gGBufferDepth", _) => v(sized.exe_depth.as_ref().map_or(&sized.depth_view, |d| &d.out.view)),
            ("gEnvBRDFGeneric", _) => v(&self.light.env_brdf.view),
            ("gReflectionProbeArray", _) => v(&self.light.probes.view),
            ("gProbeXArray", _) => v(&self.light.volume[0].view),
            ("gProbeYArray", _) => v(&self.light.volume[1].view),
            ("gProbeZArray", _) => v(&self.light.volume[2].view),
            ("gSpotShadowmapArray", _) => v(self.spot.as_ref().map_or(&self.none.depth_array.view, |s| &s.tiles.view)),
            ("gSunShadowmapArray", _) => v(self.sun_shadows.as_ref().map_or(&self.none.sun_shadow.view, |s| &s.maps_view)),
            ("gSunShadowMax", _) => v(self.sun_shadows.as_ref().map_or(&self.none.sun_shadow_max.view, |s| &s.max.view)),
            ("gOmniShadowmapArray", _) => v(&self.none.depth_cube_array.view),
            ("gCookieArray", _) => v(self.none.cookie.as_ref().map_or(&self.none.colour_array.view, |c| &c.view)),
            ("gTransShadowmapArray", _) => v(&self.none.colour_array.view),
            ("gSSAOTexture", _) => v(self.ao.as_ref().and_then(|a| a.output(r)).unwrap_or(&self.none.white.view)),
            // The SSR set's inputs (exe `141d5ba00`): the quarter-res scene
            // of the frame before, this frame's motion vectors (role
            // +0xb0e58 by frame parity) and the depth pyramid of the frame
            // (`141d6ddb0`: image `0x14a4fe868 + ((frame + 2) % 3)` with GTAO).
            ("gDownsampledScene", _) => v(&sized.down_scene.view),
            ("gMotionVectors", _) => v(self.motion.as_ref().and_then(|m| m.targets()).map_or(&self.none.black.view, |t| t[parity])),
            ("gSSAODepthTexture", _) => v(&sized.ring[r].0.view),
            ("gSunShadowTree", _) => b(self.sun_shadows.as_ref().map_or(&self.sun_tree, |s| &s.tree)),
            ("gCullConstants", _) => b(&self.cull_constants),
            ("volumetricSpotLightBits", _) => b(&self.volumetric_bits[0]),
            ("volumetricOmniLightBits", _) => b(&self.volumetric_bits[1]),
            ("volumetricProbeBits", _) => b(&self.volumetric_bits[2]),
            ("gPermuteExtentsBuffer" | "gPermutedExtentsBuffer", _) => b(&sized.extents),
            ("gPermutedTilingBuffer", _) => b(&sized.tiling),
            ("gPermuteAccumulateBuffer", _) => b(&self.accumulate),
            ("gDeferredCullBits", _) => b(&sized.cull_bits),
            ("gPermutedIndirectBuffer", _) => b(&self.indirect),
            ("litBuffer", _) => v(&sized.scene.view),
            ("gResolvedNormalAndGloss", _) => v(&sized.resolved_normal.view),
            ("DS2x", _) => mip(0),
            ("DS4x", _) => mip(1),
            // The resolve writes DS8x into the ring; gtao_2 reads it from
            // DS8xCopy (render target 0x1f, `0x141d5bf40`).
            ("DS8x", SlotKind::StorageTexture { .. }) => mip(2),
            ("DS8x" | "DS8xCopy", _) => v(&sized.ds8x_copy.view),
            ("DS16x", _) => mip(3),
            ("DS32x", _) => mip(4),
            ("DS64x", _) => mip(5),
            (name, SlotKind::Sampler { .. }) => Res::Sampler(self.samplers.get(name)?),
            _ => return None,
        })
    }

    /// A code resource by the name the programs give it (their RDEF), for
    /// the forward lists' draws, once the stage is sized (agent D).
    pub fn named<'a>(&'a self, name: &str, targets: &'a Targets) -> Option<Res<'a>> {
        let sized = self.sized.as_ref()?;
        let slot = Slot { group: 0, binding: 0, slot: 0, name: name.to_owned(), kind: SlotKind::ReadBuffer };
        // (Bound once: a ring resource is this frame's slot.)
        let f = self.frames_updated.saturating_sub(1);
        let r = f.wrapping_add(2) as usize % 3;
        self.resource(&slot, targets, sized, r, (f & 1) as usize)
    }

    /// A code sampler by its RDEF name (agent D).
    pub fn named_sampler(&self, name: &str) -> Option<&wgpu::Sampler> {
        self.samplers.get(name)
    }

    /// The view's code constants as this stage fills them (the forward
    /// lists take the same).
    pub fn view_consts(&self, camera: &Camera, now: i32) -> CodeConsts {
        let (w, h) = self.sized.as_ref().map_or((1, 1), |s| (s.width, s.height));
        let mut consts = globals::view_consts(camera, w, h, now as f32 / 1000.0, self.frames_updated);
        // The view's relHDRExposure (exe 0x141dddc20: (2^(EV-3), 2^-(EV-3),
        // 0, 0) from the eye exposure; `crate::post::Post::begin_view`).
        consts.set(globals::REL_HDR_EXPOSURE, vec![self.rel_hdr_exposure]);
        // hdrControl0/1 from the same exposure (0x141dddc20), for any lighting
        // program that takes them.
        let hdr = crate::post_run::Post::hdr_controls(self.rel_hdr_exposure);
        consts.set(globals::HDR_CONTROL0, vec![hdr[0]]);
        consts.set(globals::HDR_CONTROL1, vec![hdr[1]]);
        consts
    }

    /// The per-frame constants: the view's code constants into
    /// `PerSceneConsts` (by the programs' arguments), `LightingGlobals`
    /// (fog at the eye, sun, global probe, tiles). `now`: client time in
    /// ms; `cut`: a camera cut (fog transitions jump).
    pub fn update(&mut self, camera: &Camera, now: i32, cut: bool) {
        let Some(sized) = self.sized.as_ref() else { return };
        let (w, h, tiles) = (sized.width, sized.height, sized.tiles);
        let time = now as f32 / 1000.0;
        let consts = self.view_consts(camera, now);
        let mut missing = Vec::new();
        fill_code(&mut self.per_scene_bytes, &self.code_args, &consts, &mut missing);
        if self.frames_updated == 0 {
            missing.sort();
            missing.dedup();
            self.report.extend(missing.into_iter().map(|m| format!("lighting: code constant missing: {m}")));
        }
        if let Some(m) = self.motion.as_mut() {
            // (The view's motion blur scale, exe 0x141ddc6c0:
            // `crate::post_mblur`.)
            m.set_blur_scale(crate::post_mblur::view_scale(crate::post_mblur::mode(), 1.0, crate::post_mblur::amount()));
            m.update(camera);
        }
        if let (Some(ao), Some(vm)) = (self.ao.as_mut(), consts.get(globals::VIEW_MATRIX)) {
            ao.update(camera, vm, w as f32 / h.max(1) as f32, now, self.frames_updated);
        }
        self.frames_updated += 1;
        self.gpu.queue.write_buffer_p(&self.per_scene, 0, &self.per_scene_bytes);

        let eye = camera.eye.to_array();
        if let Some(d) = self.sized.as_ref().and_then(|s| s.exe_depth.as_ref()) {
            self.gpu.queue.write_buffer_p(&d.k, 0, bytemuck::cast_slice(&[exe_depth_scale(camera.near), 0.0, 0.0, 0.0f32]));
        }
        let fog = match &self.fog_src {
            Some(src) => {
                let (bank, scales) = self.fog.update(self.kept.bytes(), src, 0, 1, eye, now, cut);
                let client = fastfile_t7::fog::client_fog(&bank, scales, self.fog_light.as_ref());
                let o = fastfile_t7::fog::evaluate(&client, eye);
                fastfile_t7::fog::fog_constants(&fastfile_t7::fog::config(&o, eye, true))
            }
            None => [0.0; 56],
        };
        let layer = 1u32;
        let q = crate::quality::get();
        let max = if q.local_lights { q.spot_shadows } else { 0 };
        let mut packed = self.lights.pack(camera, &crate::lights::frustum(camera), layer, time, camera.tan_y, &fastfile_t7::scene_light::current(), self.spot.as_mut().map(|s| (&mut s.plan, max)));
        if no_lights() || !crate::quality::get().local_lights {
            packed.count = 0;
            packed.volumetric_bits = Default::default();
            packed.volumetric_range = [3000, 0, 3000, 0];
        }
        if self.frames_updated == 1 {
            self.report.push(format!("lighting: {} map lights packed this frame", packed.count));
            if std::env::var("T7_LIGHT_DUMP").is_ok() {
                for (i, r) in packed.records.chunks(crate::lights::RECORD).take(4).enumerate() {
                    let w: Vec<String> = r.chunks(4).map(|b| { let v = u32::from_le_bytes(b.try_into().unwrap()); format!("{:.3}", f32::from_bits(v)) }).collect();
                    self.report.push(format!("light {i}: flags {:#010x} {}", u32::from_le_bytes(r[28..32].try_into().unwrap()), w.join(" ")));
                }
                for (i, r) in packed.cull.chunks(crate::lights::CULL).take(4).enumerate() {
                    let w: Vec<String> = r.chunks(4).take(14).map(|b| format!("{:.2}", f32::from_le_bytes(b.try_into().unwrap()))).collect();
                    self.report.push(format!("cull {i}: {}", w.join(" ")));
                }
            }
        }
        if packed.count > 0 {
            self.gpu.queue.write_buffer_p(&self.light_pack, 0, &packed.records);
            self.gpu.queue.write_buffer_p(&self.light_records, 0, &packed.records);
            self.gpu.queue.write_buffer_p(&self.cull_constants, 0, &packed.cull);
        }
        let frustum = crate::lights::frustum(camera);
        let mut probes = crate::probes::pack(&self.probe_configs, &self.probe_blends, camera, &frustum);
        if no_probes() {
            probes.count = 0;
            probes.blend_count = 0;
            probes.overrides = 0;
        }
        if probes.blend_count > 0 {
            self.gpu.queue.write_buffer_p(&self.blend_pack, 0, &probes.blends);
        }
        // (The volumetric bits, every frame: a word left from an earlier
        // frame would mark a light that is no longer there.)
        let words = |b: &[u32]| {
            let mut w = [0u32; 16];
            for (k, v) in b.iter().take(16).enumerate() {
                w[k] = *v;
            }
            w
        };
        if no_probes() {
            probes.volumetric_bits.clear();
            probes.volumetric_range = [0xfa, 0];
        }
        for (k, b) in [&packed.volumetric_bits[0], &packed.volumetric_bits[1], &probes.volumetric_bits].into_iter().enumerate() {
            self.gpu.queue.write_buffer_p(&self.volumetric_bits[k], 0, bytemuck::cast_slice(&words(b)));
        }
        if probes.count > 0 {
            self.gpu.queue.write_buffer_p(&self.probe_pack, 0, &probes.records);
            self.gpu.queue.write_buffer_p(&self.cull_constants, (crate::lights::CULL as u64) * u64::from(packed.count), &probes.cull);
        }
        if self.frames_updated == 1 {
            self.report.push(format!("lighting: {} local probes and {} probe blends packed this frame", probes.count, probes.blend_count));
        }
        crate::timing::count_lights(u64::from(packed.count));
        let counts = Counts { lights: packed.count, ref_probes: probes.count, ref_probe_blends: probes.blend_count, override_probes: probes.overrides, volumetric_lights: packed.volumetric_range, volumetric_probes: probes.volumetric_range };
        self.counts = counts;
        let mut g = globals::build(&fog, &self.sun, self.mode, time, self.probe0.as_deref(), eye, w, h, tiles, &counts);
        // The cascades fitted to the view (`141e069a0`): the sun constants'
        // shadow fields and `minMaxConstants` from the shadow state.
        if let Some(s) = self.sun_shadows.as_mut() {
            s.update(&self.gpu, &self.sun, camera);
            s.patch_globals(&mut g, &self.sun, time);
            if self.frames_updated == 1 && let Some(st) = &s.state {
                for c in 0..3 {
                    self.report.push(format!("t7 sun shadow: cascade {c}: tree level {}, texel {:.3}, distance {:.1}, corner {:?}, uv {:?}", st.level[c], st.texel[c], st.distance[c], st.corner[c], st.uv[c]));
                }
            }
        }
        if self.frames_updated == 1 {
            // What the lit frame is built from, for a PC run's log.
            let f = |at: usize| f32::from_le_bytes(g[at..at + 4].try_into().unwrap());
            let sun: Vec<String> = (0..12).map(|i| format!("{:.4}", f(0x110 + 4 * i))).collect();
            self.report.push(format!("lighting: globals: sun[0..12] {}; shadow mode {}; probe units/tint (sun[8..12]) {:.3} {:.3} {:.3} {:.3}", sun.join(" "), self.mode, f(0x130), f(0x134), f(0x138), f(0x13c)));
            let fogv: Vec<String> = (0..56).map(|i| format!("{:.4}", f(0x30 + 4 * i))).collect();
            self.report.push(format!("lighting: fog constants at the eye {eye:?}: {}", fogv.join(" ")));
            self.report.push(format!("lighting: relHDRExposure {:?}", self.rel_hdr_exposure));
        }
        // The view's lit fog list and what the exe would run from it
        // (`crate::volumetric`).
        if let Some(vols) = self.lit_fog.0 {
            self.lit_fog_states.update(now);
            let frustum = crate::lights::frustum(camera);
            let v = crate::volumetric::View { eye, frustum: &frustum, sun_colour: self.sun.colour, sun_intensity: self.sun.intensity, sun_mode: self.mode, lighting_state: 0 };
            self.lit_fog_packed = crate::volumetric::pack(&crate::volumetric::Volumes { image: self.kept.bytes(), volumes: vols, planes: self.lit_fog.1 }, &self.lit_fog_states, &v);
        }
        if self.frames_updated == 1 {
            let d = crate::volumetric::Dvars::current();
            let p = &self.lit_fog_packed;
            let plan = crate::volumetric::plan(&d, p);
            self.report.push(format!(
                "t7 volumetrics: {} lit fog volumes packed for the view at {eye:?} (volumes {:?}, flags {:#x}); the exe's passes: {} ({})",
                p.count,
                p.volumes,
                p.flags,
                if plan.is_empty() { "none".to_owned() } else { plan.join(" -> ") },
                if crate::volumetric::gpu_on() { "drawn by crate::volumetric_pass, on a Workshop map only with T7_VOLUMETRIC_GPU=1" } else { "not drawn: T7_VOLUMETRIC_GPU=0" }
            ));
        }
        // (The view's weather grime, `141ddc190`.)
        g[0x260..0x2b0].copy_from_slice(&self.grime.block(self.grime.select(eye)));
        // The shadow lookups' 8-tap kernel (word 58.y: the exe's bool dvar,
        // default on), with the spot shadows.
        if self.spot.is_some() && crate::spot_shadow::filter_on() {
            g[0x3a4..0x3a8].copy_from_slice(&1u32.to_le_bytes());
        }
        self.gpu.queue.write_buffer_p(&self.globals, 0, &g);
    }

    fn dispatch(&self, pass: &mut wgpu::ComputePass, program: usize, threads: [u32; 3]) {
        let p = &self.programs[program];
        self.bind(pass, program);
        let [x, y, z] = p.groups_for(threads);
        pass.dispatch_p(x, y, z);
    }

    fn bind(&self, pass: &mut wgpu::ComputePass, program: usize) {
        let sized = self.sized.as_ref().expect("prepared");
        let p = &self.programs[program];
        pass.set_pipeline_p(&p.pipeline);
        let k = self.frames_updated.saturating_sub(1) as usize % GROUP_SLOTS;
        for (i, g) in sized.groups[program].1[k].iter().enumerate() {
            pass.set_bind_group_p(i as u32, g, &[]);
        }
    }

    /// Records the stage (after the G-buffer pass) into the scene target.
    /// The spot shadow tiles this frame's packing asked for (before
    /// [`Self::record`], which reads them).
    pub fn record_spot_shadows(&mut self, enc: &mut wgpu::CommandEncoder, g: &crate::gbuffer::GBuffer) {
        let Some(s) = self.spot.as_mut() else { return };
        let first = s.drawn == 0;
        let n = s.record(&self.gpu, enc, g);
        if first && n > 0 {
            self.report.push(format!("t7 spot shadows: {n} tiles drawn for the first view ({} lights shadowed)", s.plan.last_count));
        }
        if s.plan.failed && !s.failure_said {
            s.failure_said = true;
            self.report.push("t7 spot shadows: their code panicked (the CRASH panic lines above say where); the lights are unshadowed for the rest of this map, the game goes on".into());
        }
    }

    pub fn record(&self, enc: &mut wgpu::CommandEncoder) {
        let Some(sized) = self.sized.as_ref() else { return };
        let (w, h) = (sized.width, sized.height);
        let frame = self.frames_updated.saturating_sub(1);
        // The lit scene and the resolved normal start every frame black:
        // the compute passes write only the pixels the G-buffer covers, and
        // a pixel with no surface (the sky, until it is drawn over it) would
        // keep what an earlier frame left there.
        for (label, t) in [("t7 clear scene", &sized.scene), ("t7 clear resolved normal", &sized.resolved_normal)] {
            // (A render pass clear: texture clears need a device feature the game's device lacks.)
            enc.render_pass_p(&wgpu::RenderPassDescriptor {
                label: Some(label),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment { view: &t.view, depth_slice: None, resolve_target: None, ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT), store: wgpu::StoreOp::Store } })],
                ..Default::default()
            });
        }
        // The G-buffer depth in the exe's encoding, for every program below.
        if let Some(d) = &sized.exe_depth {
            d.record(enc);
        }
        // The sun shadow cascades first (`141d5ca10`: before the resolve).
        if let Some(s) = &self.sun_shadows {
            s.record(enc);
        }
        {
            let mut pass = enc.compute_pass_p(&wgpu::ComputePassDescriptor { label: Some("t7 uber resolve"), timestamp_writes: None });
            // Uber resolve: threads to the next multiple of 8, then gtao_2
            // over an eighth of them (`141d5bf40`).
            self.dispatch(&mut pass, self.resolve, [(w + 7) & !7, (h + 7) & !7, 1]);
            self.dispatch(&mut pass, self.gtao_2, [((w + 7) & !7) >> 3, ((h + 7) & !7) >> 3, 1]);
        }
        crate::timing::mark(enc, "clear+resolve");
        // Static motion vectors (`141d5c6b0`), then GTAO (`141d6de80`).
        if let Some(m) = &self.motion {
            m.record(enc, frame);
        }
        crate::timing::mark(enc, "motion");
        if let Some(ao) = &self.ao {
            ao.record(enc, frame);
        }
        crate::timing::mark(enc, "ao");
        let mut pass = enc.compute_pass_p(&wgpu::ComputePassDescriptor { label: Some("t7 deferred lighting"), timestamp_writes: None });
        // Permute clear, classify, draw (`141d5ca10`).
        self.dispatch(&mut pass, self.clear, [1, 1, 1]);
        self.dispatch(&mut pass, self.classify, [(w + 7) >> 3, (h + 7) >> 3, 1]);
        self.dispatch(&mut pass, self.draw, [1, 1, 1]);
        if let Some(sp) = &self.split {
            // Pieces of at most the device's groups a dimension: the
            // arguments are made from the permutation counts, then each
            // permutation's pieces dispatch with their group offset.
            pass.set_pipeline_p(&sp.make_args);
            pass.set_bind_group_p(0, &sp.make_args_group, &[]);
            pass.dispatch_p(1, 1, 1);
            for (n, &p) in self.permuted.iter().enumerate() {
                for k in 0..sp.pieces {
                    if k == 0 {
                        self.bind(&mut pass, p);
                    } else {
                        pass.set_pipeline_p(&sp.pipelines[n][k as usize - 1]);
                    }
                    pass.dispatch_workgroups_indirect(&sp.args, 12 * (u64::from(k) * u64::from(PERMUTATIONS) + n as u64));
                }
            }
            return;
        }
        for (n, &p) in self.permuted.iter().enumerate() {
            self.bind(&mut pass, p);
            pass.dispatch_workgroups_indirect(&self.indirect, 12 * n as u64);
        }
    }
}

/// Copies each code-constant argument (`0x4xx`, constant buffer 1) of a
/// program into `bytes` at its location.
pub(crate) fn fill_code(bytes: &mut [u8], args: &[fastfile_t7::walk::capture::ShaderArg], consts: &CodeConsts, missing: &mut Vec<String>) {
    for a in args.iter().filter(|a| a.ty & 0xff00 == 0x400 && a.buffer == 1) {
        let index = (a.u[0] & 0xff) as u16;
        let Some(rows) = consts.get(index) else {
            missing.push(format!("{index:#x} {}", crate::code::name(index)));
            continue;
        };
        let src: &[u8] = bytemuck::cast_slice(rows);
        let at = a.location as usize;
        let n = (a.size as usize).min(src.len()).min(bytes.len().saturating_sub(at));
        bytes[at..at + n].copy_from_slice(&src[..n]);
    }
}

#[cfg(test)]
mod tests {
    use super::GROUP_SLOTS;

    /// The six bind group sets cover every frame's depth ring slot
    /// (`(frame + 2) % 3`) and motion vector parity (`frame & 1`).
    #[test]
    fn group_slot_matches_ring_and_parity() {
        for f in 0u64..60 {
            let k = f as usize % GROUP_SLOTS;
            assert_eq!((k + 2) % 3, (f as usize + 2) % 3);
            assert_eq!(k & 1, (f & 1) as usize);
        }
    }
}
