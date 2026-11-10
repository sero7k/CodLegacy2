//! The G-buffer pass as BlackOps3.exe records it: render targets
//! GBufferDiffuseColor (R8G8B8A8_UNORM_SRGB), GBufferNormalAndGloss and
//! GBufferSpecularColorAndOcclusion (R10G10B10A2_UNORM), depth D32_FLOAT_S8
//! (reversed: cleared to 0, tested greater-equal), every surface drawn with
//! its material's G-buffer technique: the programs as they are (translated),
//! the state map's states, the exe's input layout for the vertex source, and
//! each argument bound as the exe binds it (`super::bind`):
//!
//! ```text
//! 0x200 | stage  material texture: location = register, u[0] = the texture
//!                table hash (the image's top level as streamed in)
//! 0x300 | stage  material sampler: location = register, u[0] = the sampler
//!                table hash (state & 0x3fff)
//! 0x400 | stage  code constant: location = byte offset in constant buffer
//!                `buffer`, size bytes, u[0] low byte = index
//! 0x500 | stage  code texture: location = register, u[0] = index (0x4f:
//!                resolvedNormal, the normal target as the opaque surfaces
//!                left it)
//! 0x600 | stage  code buffer: location = register, u[0] = index (6: bone
//!                matrices, 7: model instances)
//! ```
//! Material constant buffers: `technique * 6 + method byte (+2 vertex, +4
//! pixel)`, bound at the program's bind point for the buffer's name.
//! Order: the surfaces that read resolvedNormal (decals) after the others,
//! the normal target copied to resolvedNormal between; each list in the
//! zone's surface order (the world's surfaces are stored sorted).

#[allow(unused_imports)]
use crate::profile::{ComputeExt as _, EncExt as _, PassExt as _, QueueExt as _};
use std::collections::HashMap;
use std::sync::Arc;

use render_h1::gpu::{Gpu, PassLayout};

use crate::code::CodeConsts;
use crate::gpu::{self, Tex};
use crate::layout;
use crate::rdef;
use crate::scene::{Scene, Technique};
use crate::state;

pub const FORMATS: [wgpu::TextureFormat; 3] = [wgpu::TextureFormat::Rgba8UnormSrgb, wgpu::TextureFormat::Rgb10a2Unorm, wgpu::TextureFormat::Rgb10a2Unorm];
pub const DEPTH: wgpu::TextureFormat = wgpu::TextureFormat::Depth32FloatStencil8;
pub const CODE_TEXTURE_RESOLVED_NORMAL: u32 = 0x4f;
/// `shaderConstantSetBuffer`.
pub const CODE_BUFFER_CONSTANT_SETS: u32 = 15;
pub const CODE_BUFFER_BONES: u32 = 6;
pub const CODE_BUFFER_INSTANCES: u32 = 7;

pub struct Targets {
    pub width: u32,
    pub height: u32,
    pub colour: [Tex; 3],
    pub depth: Tex,
    pub resolved_normal: Tex,
}

impl Targets {
    pub fn new(gpu: &Gpu, width: u32, height: u32) -> Self {
        let names = ["GBufferDiffuseColor", "GBufferNormalAndGloss", "GBufferSpecularColorAndOcclusion"];
        Self {
            width,
            height,
            colour: std::array::from_fn(|i| gpu::target(gpu, names[i], width, height, FORMATS[i], wgpu::TextureUsages::COPY_SRC)),
            // (COPY_SRC: the forward lists read the depth through a copy.)
            depth: gpu::target(gpu, "GBufferDepth", width, height, DEPTH, wgpu::TextureUsages::COPY_SRC),
            resolved_normal: gpu::target(gpu, "resolvedNormal", width, height, FORMATS[1], wgpu::TextureUsages::COPY_DST),
        }
    }
}

/// One technique's pipeline for one vertex source.
pub struct Pipeline {
    pub pipeline: wgpu::RenderPipeline,
    pub layout: PassLayout,
    /// The D3D slot of each wgpu vertex buffer, in order.
    pub slots: Vec<u8>,
    pub binds: [Vec<rdef::Binding>; 2],
    pub reads_resolved_normal: bool,
    /// The pixel program discards (cut-out) or target 0 blends: a missing
    /// colour map is not stood in for on it (see `surf_debug::stand_in`).
    pub masked_or_blended: bool,
}

/// The bindings of a material's draws from one vertex source (made once and
/// shared by all of them).
struct Bound {
    groups: [wgpu::BindGroup; 3],
}

/// A draw ready to record.
struct Ready {
    draw: usize,
    pipeline: Arc<Pipeline>,
    bound: Arc<Bound>,
    /// The technique's stencil reference.
    stencil: u8,
}

/// A code constant buffer: one per technique and binding, shared by every
/// draw of the technique (its contents are the view's, not the draw's), and
/// written when the frame changes it.
struct CodeBuf {
    buffer: wgpu::Buffer,
    size: usize,
    /// The technique's arguments of that stage and slot.
    args: Vec<fastfile_t7::walk::capture::ShaderArg>,
    /// What was last written.
    last: Vec<u8>,
}

/// Consecutive draws with one stencil reference, recorded once as a render
/// bundle (replayed every frame).
struct Segment {
    stencil: u32,
    bundle: wgpu::RenderBundle,
    /// What the bundle holds (draws, pipeline and bind group switches),
    /// added to the profile each time it is replayed.
    counts: crate::profile::Counts,
}

#[derive(Default, Debug)]
pub struct Report {
    pub pipelines_made: usize,
    pub pipelines_failed: Vec<String>,
    pub images_missing: Vec<String>,
    pub draws_ready: usize,
    /// Of them, opaque forward materials' depth prepass draws
    /// (`Material::forward_depth`).
    pub forward_depth: usize,
    pub draws_skipped: Vec<String>,
    /// Every G-buffer draw left out: (draw, reason).
    pub skipped_draws: Vec<(usize, String)>,
    /// Missing pictures bound as a stand-in: "material (techset): binding <- image: what".
    pub stand_ins: Vec<String>,
    /// bo2mp: draws' bindings of the frame's `LightingGlobals`
    /// (vegetation wind; `t7 wind:` line).
    pub lighting_globals_binds: usize,
    pub code_constants_missing: Vec<String>,
    /// Seconds spent: translating programs, creating pipelines, decoding
    /// pictures, uploading them (and the pictures' bytes).
    pub t_translate: f32,
    pub t_pipeline: f32,
    pub t_decode: f32,
    pub t_upload: f32,
    pub image_bytes: u64,
    pub images_loaded: usize,
}

pub struct GBuffer {
    gpu: Gpu,
    /// The planes of the last view culled against (`crate::profile`).
    cull_planes: std::sync::Mutex<[[f32; 4]; 4]>,
    /// The last view's eye and LOD context (`crate::lod`).
    cull_lod: std::sync::Mutex<Option<([f32; 3], crate::lod::View)>>,
    /// The code constant buffers' bytes as built each frame (reused).
    code_scratch: Vec<u8>,
    pub scene: Arc<Scene>,
    pub(crate) world: [wgpu::Buffer; 3],
    pub(crate) models: [wgpu::Buffer; 4],
    pub(crate) instance_ids: wgpu::Buffer,
    pub(crate) instances: wgpu::Buffer,
    pub(crate) bones: wgpu::Buffer,
    /// `shaderConstantSetBuffer` (code buffer 15): 384-byte sets, set 0 (the
    /// one the world and static models carry) never written by the exe: zero.
    pub(crate) constant_sets: wgpu::Buffer,
    /// bo2mp: `LightingGlobals` (944 bytes) as the G-buffer programs see
    /// it: a copy of the lighting stage's, refreshed every frame
    /// (`Host::record`). The vegetation programs read its `weather` block
    /// (0x260: the grime volume's wind speed and the sin/cos of its
    /// direction, times `gameTime`) for their sway; the exe binds the
    /// view's one buffer to every pass.
    pub(crate) lighting_globals: wgpu::Buffer,
    /// bo2mp: `volDecalInstanceBuffer` (code buffer 10): the scene's volume
    /// decal elements (`crate::volume_decals`).
    vol_decals: wgpu::Buffer,
    /// bo2mp: `floatZ` (code texture 0x10) for the decal list, made from the
    /// depth after the opaque list (`volume_decals::FloatZPass`).
    float_z: Option<crate::volume_decals::FloatZPass>,
    ready: Vec<Ready>,
    code: HashMap<(usize, u32), CodeBuf>,
    /// The two passes' draws as bundles (before the resolved normal copy,
    /// and the decals after it).
    bundles: [Vec<Segment>; 2],
    /// The draws' frustum culling (`crate::cull`): their indirect
    /// arguments and the culled instance ids, remade with the bundles.
    cull: Option<crate::cull::Cull>,
    pub report: Report,
    /// The resolvedNormal view the decals' groups were made with.
    resolved_for: Option<wgpu::Texture>,
    pipelines: HashMap<(usize, u8), Option<Arc<Pipeline>>>,
    pub(crate) images: HashMap<String, Option<Tex>>,
    pub(crate) samplers: HashMap<u16, wgpu::Sampler>,
    /// Offline checking only (never in the game): an image that is missing
    /// is bound as a 1x1 texture so the rest of the draw's bindings and the
    /// frame can still be validated; each such binding is counted in
    /// `report.images_missing`.
    pub check_bindings_without_images: bool,
    pub(crate) check_image: Option<Tex>,
    /// A 1x1 transparent black 2-D image: what an unbound view reads (the
    /// code textures the frame does not make: `gpuSkinBase`).
    pub(crate) black_image: Option<Tex>,
    /// The GPU skin textures in their rest pose (`gpu_skin`), by code
    /// texture (0x2c..0x2e).
    pub(crate) skin_rest: HashMap<u32, Tex>,
    /// The 1x1 stand-ins for missing pictures, by colour
    /// (`surf_debug::stand_in`).
    pub(crate) stand_in_images: HashMap<[u8; 4], Tex>,
}

/// A constant buffer the frame fills (not a material's) that the
/// G-buffer programs read without code-constant arguments: the view's
/// `LightingGlobals` (vegetation wind, weather).
pub fn code_cbuffer(name: &str) -> bool {
    name == "LightingGlobals"
}

pub fn translate(t: &Technique) -> Result<(dxbc_sm5::Translation, dxbc_sm5::Translation), String> {
    use dxbc_sm5::{Options, Stage};
    let vs_c = dxbc_sm5::Container::parse(&t.vs).map_err(|e| format!("vs: {e:?}"))?;
    let ps_c = dxbc_sm5::Container::parse(&t.ps).map_err(|e| format!("ps: {e:?}"))?;
    let vs_p = dxbc_sm5::decode(vs_c.shader).map_err(|e| format!("vs: {e:?}"))?;
    let ps_p = dxbc_sm5::decode(ps_c.shader).map_err(|e| format!("ps: {e:?}"))?;
    let mut po = Options::for_stage(Stage::Pixel);
    po.varyings = dxbc_sm5::link(&vs_c.outputs, &ps_c.inputs);
    let ps = dxbc_sm5::translate_program(&ps_p, &ps_c, &po).map_err(|e| format!("ps: {e:?}"))?;
    let mut vo = Options::for_stage(Stage::Vertex);
    vo.varyings = dxbc_sm5::link(&vs_c.outputs, &ps_c.inputs);
    vo.output_interp = ps.reflection.inputs.iter().map(|i| (i.semantic.to_ascii_uppercase(), i.index, i.interp)).collect();
    let vs = dxbc_sm5::translate_program(&vs_p, &vs_c, &vo).map_err(|e| format!("vs: {e:?}"))?;
    Ok((vs, ps))
}

/// An argument's stage (the type's low byte: 0 vertex, 1 pixel).
fn stage(ty: u16) -> u16 {
    ty & 0xff
}

/// Nanoseconds spent translating programs (all threads), for the timing lines.
pub static TRANSLATE_NS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

pub fn make_pipeline(gpu: &Gpu, t: &Technique, group: u8) -> Result<Pipeline, String> {
    // (The volume decals load `floatZ`, R32F: unfilterable.)
    make_pipeline_for(gpu, t, group, &FORMATS, DEPTH, &crate::forward::floatz_slots(t))
}

/// A technique's pipeline drawing into `formats` (the G-buffer's, or the
/// lit scene target for the forward lists) over `depth`.
pub fn make_pipeline_for(gpu: &Gpu, t: &Technique, group: u8, formats: &[wgpu::TextureFormat], depth: wgpu::TextureFormat, unfilterable_ps: &[u32]) -> Result<Pipeline, String> {
    let t_tr = std::time::Instant::now();
    let (vs, ps) = translate(t)?;
    TRANSLATE_NS.fetch_add(t_tr.elapsed().as_nanos() as u64, std::sync::atomic::Ordering::Relaxed);
    let inputs: Vec<(String, u32, u32, u32)> = vs.reflection.inputs.iter().map(|i| (i.semantic.clone(), i.index, i.location, i.component_type)).collect();
    let streams = layout::streams(&t.decl, group as usize, &inputs)?;
    let dev = &gpu.device;
    let scope = dev.push_error_scope(wgpu::ErrorFilter::Validation);
    let vm = dev.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some("t7 vs"), source: wgpu::ShaderSource::Wgsl(vs.wgsl.clone().into()) });
    // `T7_GBUFFER_WGSL=<dir>`: the pixel programs' translations written there
    // (checks).
    if let Ok(d) = std::env::var("T7_GBUFFER_WGSL") {
        let _ = std::fs::write(format!("{d}/{}.wgsl", t.techset.replace(['/', '#'], "_")), &ps.wgsl);
        let _ = std::fs::write(format!("{d}/{}.vs.wgsl", t.techset.replace(['/', '#'], "_")), &vs.wgsl);
    }
    let pm = dev.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some("t7 ps"), source: wgpu::ShaderSource::Wgsl(ps.wgsl.clone().into()) });
    let layout = render_h1::gpu::pass_layout_with(gpu, &vs.reflection, &ps.reflection, None, unfilterable_ps);
    let mut targets = state::targets(&t.state, formats)?;
    // Targets the program doesn't write keep their contents.
    for (i, tg) in targets.iter_mut().enumerate() {
        if !ps.reflection.targets.contains(&(i as u32))
            && let Some(tg) = tg
        {
            tg.write_mask = wgpu::ColorWrites::empty();
        }
    }
    let buffers: Vec<wgpu::VertexBufferLayout> = streams.iter().map(|s| wgpu::VertexBufferLayout { array_stride: s.stride, step_mode: s.step, attributes: &s.attributes }).collect();
    let pipeline = dev.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some(&t.techset),
        layout: Some(&layout.pipeline_layout),
        vertex: wgpu::VertexState {
            module: &vm,
            entry_point: Some("main"),
            compilation_options: Default::default(),
            buffers: &buffers,
        },
        // (`CODLEGACY_T7_DEBUG_SURF=cull_none`: no face culling, a winding test.)
        // (World surfaces wind counter-clockwise when `winding` found them
        // opposite to the models.)
        primitive: state::primitive(&t.state).map(|p| {
            let p = if group == 3 && crate::winding::world_flipped() { wgpu::PrimitiveState { front_face: wgpu::FrontFace::Ccw, ..p } } else { p };
            // A sky technique draws no face culling: the sky program turns
            // every vertex into a direction, so a triangle wound the other
            // way from its neighbours (a map's own low-poly dome) was a
            // black hole in the sky. `T7_SKY_CULL=1`: the technique's own.
            let sky_open = t.techset.contains("sky") && !std::env::var("T7_SKY_CULL").is_ok_and(|v| v != "0");
            if crate::surf_debug::get().cull_none || sky_open { wgpu::PrimitiveState { cull_mode: None, ..p } } else { p }
        })?,
        depth_stencil: Some(state::depth_stencil(&t.state, depth)?),
        multisample: Default::default(),
        fragment: Some(wgpu::FragmentState { module: &pm, entry_point: Some("main"), compilation_options: Default::default(), targets: &targets }),
        multiview_mask: None,
        cache: crate::pipecache::get(&dev).as_ref(),
    });
    if let Some(e) = pollster::block_on(scope.pop()) {
        return Err(format!("{}: {e}", t.techset));
    }
    // The bindings must be the ones the programs declare (RDEF).
    let binds = [rdef::bindings(&t.vs), rdef::bindings(&t.ps)];
    for (st, r) in [(0usize, &vs.reflection), (1, &ps.reflection)] {
        for c in &r.cbuffers {
            if !binds[st].iter().any(|b| b.kind == rdef::CBUFFER && b.point == c.slot) {
                return Err(format!("{}: stage {st} cbuffer b{} not in RDEF", t.techset, c.slot));
            }
        }
        for x in &r.textures {
            if !binds[st].iter().any(|b| b.kind != rdef::CBUFFER && b.kind != rdef::SAMPLER && b.point == x.slot) {
                return Err(format!("{}: stage {st} texture t{} not in RDEF", t.techset, x.slot));
            }
        }
        for s in &r.samplers {
            if !binds[st].iter().any(|b| b.kind == rdef::SAMPLER && b.point == s.slot) {
                return Err(format!("{}: stage {st} sampler s{} not in RDEF", t.techset, s.slot));
            }
        }
    }
    let reads_resolved_normal = t.args.iter().any(|a| a.ty & 0xff00 == 0x500 && a.u[0] == CODE_TEXTURE_RESOLVED_NORMAL);
    let masked_or_blended = ps.reflection.discards || state::target_blend(t.state.blend[0]).is_ok_and(|(b, _)| b.is_some());
    Ok(Pipeline { pipeline, layout, slots: streams.iter().map(|s| s.slot).collect(), binds, reads_resolved_normal, masked_or_blended })
}

/// A picture decoded from the zone or the packages and uploaded: the
/// texture, its bytes and the seconds decoding and uploading took; or the
/// reason it is missing and the seconds spent.
#[allow(clippy::type_complexity)]
pub(crate) fn load_image(gpu: &Gpu, map: &fastfile_t7::map::T7Map, name: &str) -> Result<(Tex, u64, f32, f32), (String, f32)> {
    load_image_kept(gpu, map, name, None)
}

/// The largest picture level the quality keeps (before any out-of-memory
/// step-down).
pub(crate) fn image_max_dim() -> u32 {
    // (`T7_IMAGE_MAX_DIM`: the largest level kept, 1024 by default: with the
    // game's own pictures already resident a Workshop map's 3600 full-size
    // pictures ran his 16 GB card out of memory (Firing Range, 2 Nov 2026).
    // `0` or a large number keeps the full images.)
    // (Without it, the quality preset's cap: 1024 at ultra.)
    static MAX_DIM: std::sync::OnceLock<Option<u32>> = std::sync::OnceLock::new();
    MAX_DIM.get_or_init(|| std::env::var("T7_IMAGE_MAX_DIM").ok().and_then(|v| v.parse::<u32>().ok()).map(|v| if v == 0 { u32::MAX } else { v })).unwrap_or_else(|| crate::quality::get().image_max_dim)
}

/// [`load_image`], also handing each picture uploaded at the quality's size
/// (no out-of-memory step-down in force) to `keep`, as the card took it
/// (the map bake writes them).
#[allow(clippy::type_complexity)]
pub(crate) fn load_image_kept(gpu: &Gpu, map: &fastfile_t7::map::T7Map, name: &str, keep: Option<&(dyn Fn(&str, &fastfile_t7::texture::ImageLevels) + Sync)>) -> Result<(Tex, u64, f32, f32), (String, f32)> {
    let max_dim = image_max_dim();
    let mut decoded = 0.0;
    let mut last_size = u32::MAX;
    // (Out of video memory: the picture again at half the size, down to
    // `OOM_FLOOR`, and every later picture of this map at most that size;
    // see `OOM_CAP`.)
    loop {
        let cap = oom_cap().map_or(max_dim, |c| c.min(max_dim));
        let t0 = std::time::Instant::now();
        let levels = map.image(name, cap).map_err(|e| (e, decoded + t0.elapsed().as_secs_f32()))?;
        decoded += t0.elapsed().as_secs_f32();
        let size = levels.width.max(levels.height);
        if size >= last_size {
            // (No smaller level to fall back to.)
            return Err((format!("{name}: out of video memory at {last_size} px and no smaller level"), decoded));
        }
        last_size = size;
        let t1 = std::time::Instant::now();
        match upload_or_oom(gpu, name, &levels) {
            Ok(tex) => {
                let up = t1.elapsed().as_secs_f32();
                if let Some(keep) = keep.filter(|_| oom_cap().is_none()) {
                    keep(name, &levels);
                }
                return Ok((tex, levels.data.len() as u64, decoded, up));
            }
            Err(Upload::Failed(e)) => return Err((e, decoded)),
            Err(Upload::OutOfMemory(e)) => {
                if size <= OOM_FLOOR {
                    return Err((format!("{name}: out of video memory at {size} px ({e})"), decoded));
                }
                let next = (size / 2).max(OOM_FLOOR);
                if lower_oom_cap(next) {
                    crate::prewarm::note(&format!("t7 native: out of video memory loading {name} at {size} px ({e}); this map's pictures from now on at most {next} px"));
                }
            }
        }
    }
}

/// The smallest picture size the out-of-memory step-down goes to.
const OOM_FLOOR: u32 = 256;

/// The picture size cap the card's running out of video memory set for this
/// map (0: none). Black Ops III's largest maps (Shadows of Evil: 3889
/// pictures, 63660 placed models) on a card already holding the game's
/// other pictures ended the game with `render: OutOfMemory` as the lighting
/// stage began (his PC, 2 Nov 2026); an upload that runs out now steps the
/// cap down instead (`reset_oom_cap` at each map).
static OOM_CAP: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

/// The out-of-memory picture cap in force for this map, if any.
pub fn oom_cap() -> Option<u32> {
    match OOM_CAP.load(std::sync::atomic::Ordering::Relaxed) {
        0 => None,
        c => Some(c),
    }
}

/// Lowers the out-of-memory cap to `to` (never raises it); whether it fell.
fn lower_oom_cap(to: u32) -> bool {
    let prev = OOM_CAP.fetch_update(std::sync::atomic::Ordering::Relaxed, std::sync::atomic::Ordering::Relaxed, |c| (c == 0 || to < c).then_some(to));
    prev.is_ok()
}

/// A new map: pictures at the quality's size again.
pub fn reset_oom_cap() {
    OOM_CAP.store(0, std::sync::atomic::Ordering::Relaxed);
}

/// Why a picture was not uploaded.
pub(crate) enum Upload {
    /// The card had no memory for it (caught, not the device's fatal error).
    OutOfMemory(String),
    Failed(String),
}

/// `gpu::upload` inside an out-of-memory error scope: a card out of video
/// memory fails this one picture instead of reaching the device's
/// uncaptured-error handler (which ends the game). The scope is this
/// thread's (wgpu's error scopes are per thread; the warm loads pictures on
/// several).
pub(crate) fn upload_or_oom(gpu: &Gpu, name: &str, levels: &fastfile_t7::texture::ImageLevels) -> Result<Tex, Upload> {
    let scope = gpu.device.push_error_scope(wgpu::ErrorFilter::OutOfMemory);
    let r = gpu::upload(gpu, name, levels);
    let t = std::time::Instant::now();
    let oom = pollster::block_on(scope.pop());
    SCOPE_NS.fetch_add(t.elapsed().as_nanos() as u64, std::sync::atomic::Ordering::Relaxed);
    match (r, oom) {
        (_, Some(e)) => Err(Upload::OutOfMemory(e.to_string())),
        (Ok(t), None) => Ok(t),
        (Err(e), None) => Err(Upload::Failed(e)),
    }
}

/// bo2mp (load timing): nanoseconds the uploads spent waiting on their
/// out-of-memory error scope, over every thread (part of the upload time).
static SCOPE_NS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// bo2mp (load timing): seconds the uploads have waited on their error
/// scopes so far, over every thread.
pub(crate) fn scope_wait() -> f64 {
    SCOPE_NS.load(std::sync::atomic::Ordering::Relaxed) as f64 / 1e9
}

/// The program's name for texture slot `slot` of stage `st` (RDEF).
pub(crate) fn texture_binding_name(p: &Pipeline, st: u16, slot: u32) -> String {
    p.binds[st as usize].iter().find(|b| b.kind != rdef::CBUFFER && b.kind != rdef::SAMPLER && b.point == slot).map(|b| b.name.clone()).unwrap_or_default()
}

/// An empty draw the CPU says covers this many pixels or more is
/// `missing` in the `t7 coverage:` audit.
const COVERAGE_MISSING_PIXELS: f32 = 64.0;

/// Whether the `t7 coverage:` audit runs: `CODLEGACY_T7_DEBUG_COVERAGE=1` only.
/// Never in normal play (thousands of render passes and occlusion queries
/// on one frame; on by default it ended Skyjacked on his PC, 2026-10-26).
pub fn coverage_wanted() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| {
        codlegacy_env::var("CODLEGACY_T7_DEBUG_COVERAGE").is_ok_and(|v| !v.is_empty() && v != "0")
    })
}

impl GBuffer {
    /// The 1x1 stand-in picture of colour `rgba` (made once).
    pub(crate) fn stand_in(&mut self, rgba: [u8; 4]) -> &Tex {
        let gpu = &self.gpu;
        self.stand_in_images.entry(rgba).or_insert_with(|| {
            let t = gpu::target(gpu, "t7 missing picture stand-in", 1, 1, wgpu::TextureFormat::Rgba8Unorm, wgpu::TextureUsages::COPY_DST);
            gpu.queue.write_texture_p(t.texture.as_image_copy(), &rgba, wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(4), rows_per_image: Some(1) }, wgpu::Extent3d { width: 1, height: 1, depth_or_array_layers: 1 });
            t
        })
    }

    /// `t7 coverage:` every G-buffer draw recorded alone (depth cleared,
    /// the frame's view) with an occlusion query: how many samples it
    /// writes. A draw whose bounds are in the view and that writes none
    /// does not rasterise at all (its vertex program puts it elsewhere, its
    /// pixel program discards everything, or its state writes nothing),
    /// whatever else is drawn. Per technique set: draws in view, of them
    /// writing nothing, and those draws' names. Clobbers the G-buffer
    /// targets (the frame's own pass clears them after).
    pub fn coverage_audit(&self, targets: &Targets, world_to_clip: glam::Mat4) -> Vec<String> {
        use std::collections::BTreeMap;
        let dev = &self.gpu.device;
        let n = self.ready.len();
        if n == 0 {
            return vec!["t7 coverage: no G-buffer draws".into()];
        }
        let in_view = |d: &crate::scene::Draw| -> bool {
            if d.mins == d.maxs && d.group == 1 {
                return true;
            }
            let c: Vec<glam::Vec4> = (0..8)
                .map(|k| world_to_clip * glam::Vec4::new(if k & 1 == 0 { d.mins[0] } else { d.maxs[0] }, if k & 2 == 0 { d.mins[1] } else { d.maxs[1] }, if k & 4 == 0 { d.mins[2] } else { d.maxs[2] }, 1.0))
                .collect();
            // Outside when every corner is beyond one clip plane.
            !(c.iter().all(|v| v.x > v.w) || c.iter().all(|v| v.x < -v.w) || c.iter().all(|v| v.y > v.w) || c.iter().all(|v| v.y < -v.w) || c.iter().all(|v| v.w <= 0.0))
        };
        // (wgpu allows at most QUERY_SET_MAX_QUERIES queries in a set:
        // Skyjacked has 7377 draws, and one set of 7377 was invalid. In
        // chunks, each submitted and waited on alone, inside the error
        // scope, so nothing here reaches the device's uncaptured handler.)
        let scope = dev.push_error_scope(wgpu::ErrorFilter::Validation);
        let mut samples: Vec<u64> = vec![0; n];
        let chunk = wgpu::QUERY_SET_MAX_QUERIES as usize;
        for start in (0..n).step_by(chunk) {
            let m = chunk.min(n - start);
            let qs = dev.create_query_set(&wgpu::QuerySetDescriptor { label: Some("t7 coverage"), ty: wgpu::QueryType::Occlusion, count: m as u32 });
            let resolve = dev.create_buffer(&wgpu::BufferDescriptor { label: None, size: 8 * m as u64, usage: wgpu::BufferUsages::QUERY_RESOLVE | wgpu::BufferUsages::COPY_SRC, mapped_at_creation: false });
            let rb = dev.create_buffer(&wgpu::BufferDescriptor { label: None, size: 8 * m as u64, usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ, mapped_at_creation: false });
            let mut enc = dev.create_command_encoder(&Default::default());
            for (k, r) in self.ready[start..start + m].iter().enumerate() {
                let colour: Vec<Option<wgpu::RenderPassColorAttachment>> = targets.colour.iter().map(|t| Some(wgpu::RenderPassColorAttachment { view: &t.view, depth_slice: None, resolve_target: None, ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT), store: wgpu::StoreOp::Discard } })).collect();
                let mut rp = enc.render_pass_p(&wgpu::RenderPassDescriptor {
                    label: Some("t7 coverage"),
                    color_attachments: &colour,
                    depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                        view: &targets.depth.view,
                        depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(0.0), store: wgpu::StoreOp::Discard }),
                        stencil_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(0), store: wgpu::StoreOp::Discard }),
                    }),
                    timestamp_writes: None,
                    occlusion_query_set: Some(&qs),
                    multiview_mask: None,
                });
                rp.begin_occlusion_query(k as u32);
                self.draw_list(&mut rp, std::slice::from_ref(r), start + k);
                rp.end_occlusion_query();
            }
            enc.resolve_query_set(&qs, 0..m as u32, &resolve, 0);
            enc.copy_buffer_to_buffer(&resolve, 0, &rb, 0, 8 * m as u64);
            self.gpu.queue.submit([enc.finish()]);
            let data = crate::debug_views::mapped_bytes(&self.gpu, &rb);
            for (k, c) in data.chunks_exact(8).enumerate().take(m) {
                samples[start + k] = u64::from_le_bytes([c[0], c[1], c[2], c[3], c[4], c[5], c[6], c[7]]);
            }
        }
        let err = pollster::block_on(scope.pop());
        let scene = &self.scene;
        // technique set -> (in view, writing nothing, writing, discards,
        // missing: writing nothing where the CPU says it covers pixels,
        // (expected pixels, name) of the missing ones)
        type Per = (usize, usize, u64, bool, usize, Vec<(f32, String)>);
        let mut per: BTreeMap<String, Per> = BTreeMap::new();
        let (mut all_missing, mut all_empty, mut all_in_view) = (0usize, 0usize, 0usize);
        // The drawn draws' expected pixels by screen winding (which way the
        // drawn front faces wind, for reading the missing ones').
        let mut drawn_px = [0f64; 2];
        for (k, r) in self.ready.iter().enumerate() {
            let d = &scene.draws[r.draw];
            if !in_view(d) {
                continue;
            }
            let m = &scene.materials[d.material];
            let key = format!("{}{}", if d.group == 3 { "world " } else { "" }, m.techset);
            let e = per.entry(key).or_insert((0, 0, 0, r.pipeline.masked_or_blended, 0, Vec::new()));
            e.0 += 1;
            all_in_view += 1;
            e.2 += samples[k];
            let (cw, ccw) = crate::coverage::expected_pixels(scene, d, world_to_clip, targets.width, targets.height);
            if samples[k] > 0 {
                drawn_px[0] += cw as f64;
                drawn_px[1] += ccw as f64;
            }
            if samples[k] == 0 {
                e.1 += 1;
                all_empty += 1;
                // (Front faces: clockwise in clip space, as the draws that
                // write samples show (`cw` in the summary line); the world's
                // the other way when `winding` flipped it. Back faces culled
                // write nothing and are not missing.)
                let front = if d.group == 3 && crate::winding::world_flipped() { ccw } else { cw };
                if front >= COVERAGE_MISSING_PIXELS {
                    e.4 += 1;
                    all_missing += 1;
                    e.5.push((cw + ccw, format!("{}{} #{} (cw {cw:.0} px, ccw {ccw:.0} px)", m.name, if d.group == 3 { " surface" } else { " model" }, d.source)));
                }
            }
        }
        let mut out = vec![format!(
            "t7 coverage: {n} G-buffer draws each drawn alone over a cleared depth with an occlusion query{}; {all_in_view} in view, {all_empty} write no sample, {all_missing} of them should cover {COVERAGE_MISSING_PIXELS} px or more (missing); the drawing ones' triangles cover cw {:.0} px, ccw {:.0} px",
            err.map(|e| format!(" (validation: {e})")).unwrap_or_default(),
            drawn_px[0],
            drawn_px[1],
        )];
        for (t, (inv, empty, total, masked, missing, mut names)) in per {
            names.sort_by(|a, b| b.0.total_cmp(&a.0));
            names.truncate(6);
            out.push(format!(
                "t7 coverage: {t}: {inv} draws in view, {empty} write no sample, {missing} missing{}, {total} samples in all{}{}",
                if masked { " (pixel program discards or target 0 blends)" } else { "" },
                if names.is_empty() { "" } else { "; missing: " },
                names.into_iter().map(|n| n.1).collect::<Vec<_>>().join(", ")
            ));
        }
        out
    }

    /// The draws the G-buffer records (indices into the scene's draws).
    /// bo2mp: rewrites `volDecalInstanceBuffer` (offline checks: zeros hide
    /// every decal, the scene's elements bring them back).
    pub fn write_volume_decals(&self, elements: &[f32]) {
        self.gpu.queue.write_buffer_p(&self.vol_decals, 0, bytemuck::cast_slice(elements));
    }

    pub fn ready_draws(&self) -> Vec<usize> {
        self.ready.iter().map(|r| r.draw).collect()
    }

    /// The draws that cast local light shadows (`crate::spot_shadow`):
    /// ready world surfaces and static models whose pixel program neither
    /// discards nor blends, a model's finest LOD only, not the sky, volume
    /// decals or a static model flagged out of shadows (+0x60 bit 0).
    pub fn shadow_casters(&self) -> Vec<usize> {
        let sc = &self.scene;
        self.ready
            .iter()
            .filter(|r| !r.pipeline.masked_or_blended)
            .map(|r| r.draw)
            .filter(|&di| {
                let d = &sc.draws[di];
                match d.group {
                    3 => true,
                    1 => d.source < u32::MAX - 1 && d.flags & 1 == 0 && !sc.lod_alternate(d),
                    _ => false,
                }
            })
            .collect()
    }

    pub fn new(gpu: Gpu, scene: Arc<Scene>) -> Self {
        use wgpu::BufferUsages as U;
        let s = &scene;
        let idx = |v: &[u16]| -> Vec<u8> { bytemuck::cast_slice(v).to_vec() };
        let world = [
            gpu::buffer(&gpu, "t7 world positions", &s.world.positions, U::VERTEX),
            gpu::buffer(&gpu, "t7 world attributes", &s.world.attributes, U::VERTEX),
            gpu::buffer(&gpu, "t7 world indices", &idx(&s.world.indices), U::INDEX),
        ];
        let models = [
            gpu::buffer(&gpu, "t7 model positions", &s.models.positions, U::VERTEX),
            gpu::buffer(&gpu, "t7 model vertices", &s.models.attributes, U::VERTEX),
            gpu::buffer(&gpu, "t7 model weights", &s.models.weights, U::VERTEX),
            gpu::buffer(&gpu, "t7 model indices", &idx(&s.models.indices), U::INDEX),
        ];
        let ids: Vec<u32> = (0..s.instances.len() as u32).collect();
        let instance_ids = gpu::buffer(&gpu, "t7 instance ids", bytemuck::cast_slice(&ids), U::VERTEX);
        let instances = gpu::buffer(&gpu, "modelInstanceBuffer", bytemuck::cast_slice(&s.instances), U::STORAGE);
        let bones = gpu::buffer(&gpu, "boneMatrixBuffer", bytemuck::cast_slice(&s.bones), U::STORAGE | U::COPY_DST);
        let constant_sets = gpu::buffer(&gpu, "shaderConstantSetBuffer (zero)", &[0u8; 1 << 16], U::STORAGE);
        let vd: Vec<f32> = if s.volume_decals.is_empty() { vec![0.0; crate::volume_decals::ELEMENT_FLOATS] } else { s.volume_decals.iter().flatten().copied().collect() };
        let vol_decals = gpu::buffer(&gpu, "volDecalInstanceBuffer", bytemuck::cast_slice(&vd), U::STORAGE | U::COPY_DST);
        let lighting_globals = crate::audit::buffer(&gpu.device, &wgpu::BufferDescriptor { label: Some("t7 gbuffer LightingGlobals"), size: crate::lighting_globals::SIZE as u64, usage: U::UNIFORM | U::COPY_DST, mapped_at_creation: false });
        Self {
            gpu,
            scene,
            world,
            models,
            instance_ids,
            instances,
            bones,
            constant_sets,
            lighting_globals,
            vol_decals,
            float_z: None,
            ready: Vec::new(),
            code: HashMap::new(),
            bundles: [Vec::new(), Vec::new()],
            cull: None,
            cull_planes: std::sync::Mutex::new([[0.0; 4]; 4]),
            cull_lod: std::sync::Mutex::new(None),
            code_scratch: Vec::new(),
            report: Report::default(),
            resolved_for: None,
            pipelines: HashMap::new(),
            images: HashMap::new(),
            samplers: HashMap::new(),
            check_bindings_without_images: false,
            check_image: None,
            black_image: None,
            skin_rest: HashMap::new(),
            stand_in_images: HashMap::new(),
        }
    }

    fn pipeline(&mut self, t: &Arc<Technique>, group: u8) -> Option<Arc<Pipeline>> {
        let key = (Arc::as_ptr(t) as usize, group);
        if let Some(p) = self.pipelines.get(&key) {
            return p.clone();
        }
        let t0 = std::time::Instant::now();
        let p = match make_pipeline(&self.gpu, t, group) {
            Ok(p) => {
                self.report.pipelines_made += 1;
                Some(Arc::new(p))
            }
            Err(e) => {
                self.report.pipelines_failed.push(format!("{} (group {group}): {e}", t.techset));
                None
            }
        };
        self.report.t_pipeline += t0.elapsed().as_secs_f32();
        self.pipelines.insert(key, p.clone());
        p
    }

    /// Whether a picture is loaded (for the log; `image` may load it).
    pub(crate) fn has_image(&self, name: &str) -> bool {
        self.images.get(name).is_some_and(Option::is_some)
    }

    pub(crate) fn image(&mut self, name: &str) -> bool {
        if !self.images.contains_key(name) {
            let t = match load_image(&self.gpu, &self.scene.map, name) {
                Ok((tex, bytes, dec, up)) => {
                    self.report.image_bytes += bytes;
                    self.report.images_loaded += 1;
                    self.report.t_decode += dec;
                    self.report.t_upload += up;
                    Some(tex)
                }
                Err((e, dec)) => {
                    if std::env::var_os("T7_IMAGE_WHY").is_some() {
                        eprintln!("t7 image: {name}: {e}");
                    }
                    self.report.t_decode += dec;
                    self.report.images_missing.push(e);
                    None
                }
            };
            self.images.insert(name.to_owned(), t);
        }
        self.images[name].is_some()
    }

    pub(crate) fn sampler(&mut self, state: u16) -> Result<(), String> {
        if self.samplers.contains_key(&state) {
            return Ok(());
        }
        let border = self.gpu.device.features().contains(wgpu::Features::ADDRESS_MODE_CLAMP_TO_BORDER);
        let d = crate::sampler::descriptor(state, crate::sampler::material_quality(), border)?;
        self.samplers.insert(state, self.gpu.device.create_sampler(&d));
        Ok(())
    }

    /// Takes what [`crate::warm::run`] made (pipelines, pictures).
    pub fn adopt(&mut self, w: &mut crate::warm::Warm) {
        for (k, v) in w.gbuffer.drain() {
            if v.is_some() {
                self.report.pipelines_made += 1;
            }
            self.pipelines.insert(k, v);
        }
        self.report.pipelines_failed.append(&mut w.failed);
        for (k, v) in w.images.drain() {
            self.images.entry(k).or_insert(v);
        }
        self.report.images_missing.append(&mut w.missing);
        self.report.t_pipeline += w.t_pipeline;
        self.report.t_decode += w.t_decode;
        self.report.t_upload += w.t_upload;
        self.report.image_bytes += w.image_bytes;
        self.report.images_loaded += w.images_loaded;
    }

    /// Makes every draw's bindings (`targets`: for the resolvedNormal the
    /// decals read), after the pipelines and pictures ([`Self::adopt`], else
    /// made here one by one) and then the draws' bundles.
    pub fn prepare(&mut self, targets: &Targets) {
        // (floatZ for the decal list, before the bindings that read it.)
        let depth = targets.depth.texture.create_view(&wgpu::TextureViewDescriptor { aspect: wgpu::TextureAspect::DepthOnly, ..Default::default() });
        self.float_z = Some(crate::volume_decals::FloatZPass::new(&self.gpu, targets.width, targets.height, &depth));
        self.ready.clear();
        self.report.draws_skipped.clear();
        self.report.skipped_draws.clear();
        self.report.stand_ins.clear();
        let scene = self.scene.clone();
        let mut no_technique: HashMap<String, usize> = HashMap::new();
        let mut forward_depth = 0usize;
        // Bindings are the material's and the vertex source's, not the
        // draw's: made once for all of a material's draws.
        let mut bound: HashMap<(usize, u8), Result<Arc<Bound>, String>> = HashMap::new();
        for (di, d) in scene.draws.iter().enumerate() {
            // `BO2MP_T7_NATIVE_DEBUG=sky`: no world, only what is drawn behind it.
            if crate::debug_views::mode() == "sky" {
                break;
            }
            let m = &scene.materials[d.material];
            // (An opaque forward material's depth prepass is drawn here too:
            // `Material::forward_depth`.)
            let Some(t) = m.gbuffer_pass().cloned() else {
                *no_technique.entry(m.techset.clone()).or_default() += 1;
                continue;
            };
            if m.technique.is_none() {
                forward_depth += 1;
            }
            let Some(p) = self.pipeline(&t, d.group) else {
                self.report.skipped_draws.push((di, format!("pipeline failed ({})", t.techset)));
                continue;
            };
            let b = match bound.get(&(d.material, d.group)) {
                Some(b) => b.clone(),
                None => {
                    let b = self.bind(&p, &t, d.material, targets);
                    bound.insert((d.material, d.group), b.clone());
                    b
                }
            };
            match b {
                Ok(bound) => self.ready.push(Ready { draw: di, pipeline: p, bound, stencil: t.state.stencil_ref }),
                Err(e) => {
                    if self.report.draws_skipped.len() < 64 {
                        self.report.draws_skipped.push(format!("{} ({}): {e}", m.name, m.techset));
                    }
                    self.report.skipped_draws.push((di, e));
                }
            }
        }
        for (ts, n) in no_technique {
            self.report.draws_skipped.push(format!("{n} draws of {ts}: no G-buffer technique (another pass draws them; not done yet)"));
        }
        self.report.forward_depth = forward_depth;
        // Decals after the others (they read resolvedNormal).
        self.ready.sort_by_key(|r| r.pipeline.reads_resolved_normal);
        self.report.draws_ready = self.ready.len();
        self.resolved_for = Some(targets.resolved_normal.texture.clone());
        self.make_lists();
    }

    /// The draws' culling and bundles, for `ready` as it now stands.
    fn make_lists(&mut self) {
        self.cull = crate::cull::wanted(&self.gpu.device).then(|| {
            let ready: Vec<usize> = self.ready.iter().map(|r| r.draw).collect();
            crate::cull::Cull::new(&self.gpu.device, &self.scene, &ready, self.scene.instances.len() as u32)
        });
        let split = self.ready.iter().position(|r| r.pipeline.reads_resolved_normal).unwrap_or(self.ready.len());
        self.bundles = if bundles_on() {
            let first = self.segments(&self.ready[..split], 0);
            let second = self.segments(&self.ready[split..], split);
            [first, second]
        } else {
            [Vec::new(), Vec::new()]
        };
    }

    /// The view the G-buffer culls its draws against this frame
    /// (`world_to_clip`: world units to clip space; `eye` and `lod` pick
    /// the static models' LODs, `crate::lod`).
    pub fn set_cull_view(&self, world_to_clip: glam::Mat4, eye: [f32; 3], lod: crate::lod::View) {
        if let Ok(mut p) = self.cull_planes.lock() {
            *p = crate::cull::planes(world_to_clip);
        }
        if let Ok(mut l) = self.cull_lod.lock() {
            *l = Some((eye, lod));
        }
        if let Some(c) = &self.cull {
            c.set_view(&self.gpu.queue, world_to_clip, eye, &lod);
        }
    }

    /// The draws (and instances) the GPU culling leaves for the last view,
    /// worked out on the CPU with the same spheres, planes and LOD picks:
    /// (draws with any instance left, ready draws, instances left,
    /// instances in all).
    pub fn culled_count(&self) -> (usize, usize, u64, u64) {
        let p = self.cull_planes.lock().map(|p| *p).unwrap_or_default();
        let lod = self.cull_lod.lock().ok().and_then(|l| *l);
        let sc = &self.scene;
        let (mut left, mut inst_left, mut inst_all) = (0, 0u64, 0u64);
        for r in &self.ready {
            let d = &sc.draws[r.draw];
            if self.cull.is_none() && sc.lod_alternate(d) {
                continue;
            }
            inst_all += u64::from(d.instances);
            let n = match sc.cull_ranges.get(d.cull as usize).filter(|_| d.cull != crate::scene::NO_CULL && self.cull.is_some()) {
                Some(range) => match (&range.lod, lod) {
                    (Some(rl), Some((eye, v))) if d.lod != crate::scene::ANY_LOD => {
                        range.spheres.iter().enumerate().filter(|(j, s)| crate::cull::visible(&p, **s) && rl.drawn(&v, eye, *j) == Some(usize::from(d.lod))).count() as u64
                    }
                    _ => range.spheres.iter().filter(|s| crate::cull::visible(&p, **s)).count() as u64,
                },
                None if self.cull.is_none() || crate::cull::visible(&p, crate::cull::draw_sphere(d)) => u64::from(d.instances),
                None => 0,
            };
            inst_left += n;
            left += usize::from(n > 0);
        }
        (left, self.ready.len(), inst_left, inst_all)
    }

    /// The G-buffer's bundles: how many, and the draws, pipeline and bind
    /// group switches they hold in all.
    pub fn bundle_stats(&self) -> (usize, crate::profile::Counts) {
        let mut c = crate::profile::Counts::default();
        let mut n = 0;
        for seg in self.bundles.iter().flatten() {
            n += 1;
            c.draws += seg.counts.draws;
            c.pipelines += seg.counts.pipelines;
            c.binds += seg.counts.binds;
        }
        (n, c)
    }

    /// The culling's line for the log (what it culls, once per map).
    pub fn cull_line(&self) -> String {
        match &self.cull {
            Some(c) => format!("t7 cull: G-buffer draws culled to the view on the GPU: {} draws as indirect draws, {} world surfaces by their bounds, {} instanced draws' instances one by one, {} of them split by LOD (T7_NO_GPU_CULL=1: off)", self.ready.len(), c.world_draws, c.culled_ranges, c.lod_ranges),
            None => format!("t7 cull: off ({}); every G-buffer draw drawn whole", if std::env::var("T7_NO_GPU_CULL").is_ok_and(|v| v.trim() != "0") { "T7_NO_GPU_CULL" } else { "no INDIRECT_FIRST_INSTANCE on this device" }),
        }
    }

    /// Drops the depth prepass draws of opaque forward-only materials whose
    /// forward draw did not become ready (a binding the frame cannot make,
    /// a view list drawn before the lighting, a failed pipeline): the
    /// prepass lays depth (`Material::forward_depth`) for a colour draw that
    /// then never comes, which leaves a hole in the G-buffer (depth written,
    /// albedo zero: lit as pure black, and nothing behind it drawn, e.g.
    /// blocky black gaps in arches and under walkways). Without the depth
    /// the surface is simply absent and what is behind it shows. Called by
    /// `Forward::prepare` with the forward draws that are ready; returns
    /// how many were dropped and the materials' names (for the log).
    /// `T7_FORWARD_DEPTH_ORPHANS=keep`: the old way.
    pub(crate) fn drop_orphan_forward_depth(&mut self, forward_ready: &std::collections::HashSet<usize>) -> (usize, Vec<String>) {
        if std::env::var("T7_FORWARD_DEPTH_ORPHANS").is_ok_and(|v| v.trim() == "keep") {
            return (0, Vec::new());
        }
        let scene = self.scene.clone();
        let mut names: Vec<String> = Vec::new();
        let before = self.ready.len();
        self.ready.retain(|r| {
            let m = &scene.materials[scene.draws[r.draw].material];
            let orphan = m.technique.is_none() && !forward_ready.contains(&r.draw);
            if orphan && names.len() < 16 && !names.contains(&m.name) {
                names.push(m.name.clone());
            }
            !orphan
        });
        let dropped = before - self.ready.len();
        if dropped > 0 {
            self.report.forward_depth = self.report.forward_depth.saturating_sub(dropped);
            self.report.draws_ready = self.ready.len();
            self.make_lists();
        }
        (dropped, names)
    }

    #[allow(clippy::type_complexity)]
    fn bind(&mut self, p: &Pipeline, tech: &Arc<Technique>, material: usize, targets: &Targets) -> Result<Arc<Bound>, String> {
        let t: &Technique = tech;
        let scene = self.scene.clone();
        let m = &scene.materials[material];
        let dev = self.gpu.device.clone();
        // Constant buffers.
        let mut cb_buffers: Vec<(u32, wgpu::Buffer, u64)> = Vec::new();
        for &(binding, slot, registers) in &p.layout.cbuffers {
            let st: u16 = if binding >= 32 { 1 } else { 0 };
            let size = (registers.max(1) * 16) as usize;
            let args: Vec<_> = t.args.iter().filter(|a| a.ty & 0xff00 == 0x400 && stage(a.ty) == st && u32::from(a.buffer) == slot).cloned().collect();
            if !args.is_empty() {
                let key = (Arc::as_ptr(tech) as usize, binding);
                let b = self
                    .code
                    .entry(key)
                    .or_insert_with(|| CodeBuf {
                        buffer: crate::audit::buffer(&dev, &wgpu::BufferDescriptor { label: Some("t7 code constants"), size: size as u64, usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST, mapped_at_creation: false }),
                        size,
                        args,
                        last: Vec::new(),
                    })
                    .buffer
                    .clone();
                cb_buffers.push((binding, b, size as u64));
                continue;
            }
            let name = rdef::cbuffer_name(&p.binds[st as usize], slot).ok_or_else(|| format!("b{slot}: no name in RDEF"))?;
            let index = (t.slot * 6) as u8 + t.buffer_set + if st == 0 { 2 } else { 4 };
            // A code buffer the frame makes (not the material's): before
            // 2026-10-27 the material's buffer in the slot was bound in its
            // place (a vegetation program's wind read its `$Globals`: none).
            if code_cbuffer(name) && !m.cbuffers.iter().any(|(i, n, _)| *i == index && n == name) {
                cb_buffers.push((binding, self.lighting_globals.clone(), crate::lighting_globals::SIZE as u64));
                self.report.lighting_globals_binds += 1;
                continue;
            }
            let data = m
                .cbuffers
                .iter()
                .find(|(i, n, _)| *i == index && n == name)
                .or_else(|| m.cbuffers.iter().find(|(i, _, _)| *i == index))
                .map(|(_, _, d)| d.clone())
                .ok_or_else(|| format!("constant buffer {name} (b{slot}, material slot {index}) not in the material"))?;
            let mut bytes = data;
            bytes.resize(size.max(bytes.len()), 0);
            let len = bytes.len() as u64;
            cb_buffers.push((binding, gpu::buffer(&self.gpu, name, &bytes, wgpu::BufferUsages::UNIFORM), len));
        }
        // Textures (and the code buffers bound as raw buffers).
        enum Res {
            Image(String),
            Check,
            Black,
            SkinRest(u32),
            StandIn([u8; 4]),
            Resolved,
            Buffer(u32),
            FloatZ,
        }
        let mut tex_res: Vec<(u32, Res)> = Vec::new();
        for &(binding, slot, dim, kind, ps) in &p.layout.textures {
            let st: u16 = u16::from(ps);
            let a = t.args.iter().find(|a| matches!(a.ty & 0xff00, 0x200 | 0x500 | 0x600) && stage(a.ty) == st && u32::from(a.location) == slot).ok_or_else(|| format!("t{slot} (stage {st}): no argument binds it"))?;
            let r = match a.ty & 0xff00 {
                0x200 => {
                    let img = m.textures.iter().find(|(h, _)| *h == a.u[0]).map(|(_, n)| n.clone()).ok_or_else(|| format!("t{slot}: texture {:08x} not in the material's table", a.u[0]))?;
                    if img.starts_with('<') {
                        return Err(format!("t{slot}: texture {img} not named in the zone"));
                    }
                    if !self.image(&img) {
                        if self.check_bindings_without_images {
                            // (Bound as the grey stand-in; `image` listed it.)
                            Res::Check
                        } else {
                            // A stand-in by what the binding is
                            // (`surf_debug::stand_in`); before 2026-10-26 the
                            // whole draw was left out, floors with one absent
                            // detail map among them.
                            let bname = texture_binding_name(p, st, slot);
                            match crate::surf_debug::stand_in(&bname, &img, p.masked_or_blended) {
                                Some((rgba, what)) => {
                                    self.report.stand_ins.push(format!("{} ({}): t{slot} {bname} <- {img}: {what}", m.name, m.techset));
                                    Res::StandIn(rgba)
                                }
                                None => return Err(format!("t{slot}: image {img} missing ({bname}{})", if p.masked_or_blended { ", masked or blended surface" } else { "" })),
                            }
                        }
                    } else {
                        Res::Image(img)
                    }
                }
                0x500 if a.u[0] == CODE_TEXTURE_RESOLVED_NORMAL => Res::Resolved,
                // The weather grime maps (code textures 0x58..0x5d,
                // `gWeather*`): the exe's neutral defaults (white gloss,
                // grey colour, flat normal), which it binds for a view in
                // no grime volume and for a volume without images of its
                // own. (A volume's own images are not rebound per view.)
                0x500 if crate::weather::CODE_TEXTURES.contains(&a.u[0]) => {
                    let img = crate::weather::DEFAULT_IMAGES[crate::weather::CODE_TEXTURES.iter().position(|&c| c == a.u[0]).unwrap_or(0)];
                    if self.image(img) {
                        Res::Image(img.to_owned())
                    } else if self.check_bindings_without_images {
                        Res::Check
                    } else {
                        return Err(format!("t{slot}: weather image {img} missing"));
                    }
                }
                // `gpuSkinBase`, `gpuSkinQuat`, `gpuSkinPos` (0x2c..0x2e, 2-D float vertex textures): the GPU skin
                // of the fxanim flags, cloth, wires and vegetation (`mcs/`),
                // in its rest pose (`gpu_skin`: identity quaternions; black,
                // the quaternion normalised to NaN and every vertex
                // collapsed). `CODLEGACY_T7_GPU_SKIN=black`: the old black view.
                0x500 if matches!(a.u[0], 0x2c..=0x2e) && dim == dxbc_sm5::TextureDim::D2 => {
                    if crate::gpu_skin::rest_pose_on() {
                        Res::SkinRest(a.u[0])
                    } else {
                        Res::Black
                    }
                }
                0x500 if a.u[0] == crate::volume_decals::CODE_TEXTURE_FLOATZ && self.float_z.is_some() => Res::FloatZ,
                0x500 => return Err(format!("t{slot}: code texture {:#x} not made yet ({dim:?} {kind:?})", a.u[0])),
                _ => match a.u[0] {
                    CODE_BUFFER_BONES | CODE_BUFFER_INSTANCES | CODE_BUFFER_CONSTANT_SETS | crate::volume_decals::CODE_BUFFER => Res::Buffer(a.u[0]),
                    other => return Err(format!("t{slot}: code buffer {other} not made")),
                },
            };
            tex_res.push((binding, r));
        }
        if tex_res.iter().any(|(_, r)| matches!(r, Res::Black)) && self.black_image.is_none() {
            let t = gpu::target(&self.gpu, "t7 unbound view (black)", 1, 1, wgpu::TextureFormat::Rgba8Unorm, wgpu::TextureUsages::COPY_DST);
            self.gpu.queue.write_texture_p(t.texture.as_image_copy(), &[0, 0, 0, 0], wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(4), rows_per_image: Some(1) }, wgpu::Extent3d { width: 1, height: 1, depth_or_array_layers: 1 });
            self.black_image = Some(t);
        }
        for (_, r) in &tex_res {
            if let Res::StandIn(rgba) = r {
                self.stand_in(*rgba);
            }
            if let Res::SkinRest(code) = r
                && !self.skin_rest.contains_key(code)
            {
                let t = crate::gpu_skin::rest_pose(&self.gpu, *code);
                self.skin_rest.insert(*code, t);
            }
        }
        if tex_res.iter().any(|(_, r)| matches!(r, Res::Check)) && self.check_image.is_none() {
            // (Offline checks only, for pictures missing from the packages
            // at hand: mid grey, so materials whose gloss or masks come from
            // a missing picture still reach the lighting.)
            let t = gpu::target(&self.gpu, "check-only", 1, 1, wgpu::TextureFormat::Rgba8Unorm, wgpu::TextureUsages::COPY_DST);
            self.gpu.queue.write_texture_p(t.texture.as_image_copy(), &[128, 128, 128, 255], wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(4), rows_per_image: Some(1) }, wgpu::Extent3d { width: 1, height: 1, depth_or_array_layers: 1 });
            self.check_image = Some(t);
        }
        // Samplers.
        let mut samp: Vec<(u32, u16)> = Vec::new();
        for &(binding, slot, ps) in &p.layout.samplers {
            let st: u16 = u16::from(ps);
            let a = t.args.iter().find(|a| a.ty & 0xff00 == 0x300 && stage(a.ty) == st && u32::from(a.location) == slot).ok_or_else(|| format!("s{slot} (stage {st}): no argument binds it"))?;
            let state = m.samplers.iter().find(|(h, _)| *h == a.u[0]).map(|(_, s)| *s).ok_or_else(|| format!("s{slot}: sampler {:08x} not in the material's table", a.u[0]))?;
            self.sampler(state)?;
            samp.push((binding, state));
        }
        let g0: Vec<wgpu::BindGroupEntry> = cb_buffers.iter().map(|(b, buf, len)| wgpu::BindGroupEntry { binding: *b, resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding { buffer: buf, offset: 0, size: wgpu::BufferSize::new(*len) }) }).collect();
        let g1: Vec<wgpu::BindGroupEntry> = tex_res
            .iter()
            .map(|(b, r)| wgpu::BindGroupEntry {
                binding: *b,
                resource: match r {
                    Res::Image(n) => wgpu::BindingResource::TextureView(&self.images[n].as_ref().unwrap().view),
                    Res::Resolved => wgpu::BindingResource::TextureView(&targets.resolved_normal.view),
                    Res::Check => wgpu::BindingResource::TextureView(&self.check_image.as_ref().unwrap().view),
                    Res::Black => wgpu::BindingResource::TextureView(&self.black_image.as_ref().unwrap().view),
                    Res::SkinRest(code) => wgpu::BindingResource::TextureView(&self.skin_rest[code].view),
                    Res::StandIn(rgba) => wgpu::BindingResource::TextureView(&self.stand_in_images[rgba].view),
                    Res::Buffer(CODE_BUFFER_BONES) => self.bones.as_entire_binding(),
                    Res::Buffer(CODE_BUFFER_CONSTANT_SETS) => self.constant_sets.as_entire_binding(),
                    Res::Buffer(crate::volume_decals::CODE_BUFFER) => self.vol_decals.as_entire_binding(),
                    Res::Buffer(_) => self.instances.as_entire_binding(),
                    Res::FloatZ => wgpu::BindingResource::TextureView(&self.float_z.as_ref().unwrap().out.view),
                },
            })
            .collect();
        let g2: Vec<wgpu::BindGroupEntry> = samp.iter().map(|(b, s)| wgpu::BindGroupEntry { binding: *b, resource: wgpu::BindingResource::Sampler(&self.samplers[s]) }).collect();
        let scope = dev.push_error_scope(wgpu::ErrorFilter::Validation);
        let groups = [
            dev.create_bind_group(&wgpu::BindGroupDescriptor { label: Some("t7 cb"), layout: &p.layout.groups[0], entries: &g0 }),
            dev.create_bind_group(&wgpu::BindGroupDescriptor { label: Some("t7 tex"), layout: &p.layout.groups[1], entries: &g1 }),
            dev.create_bind_group(&wgpu::BindGroupDescriptor { label: Some("t7 samp"), layout: &p.layout.groups[2], entries: &g2 }),
        ];
        if let Some(e) = pollster::block_on(scope.pop()) {
            return Err(format!("bind groups: {e}"));
        }
        Ok(Arc::new(Bound { groups }))
    }

    /// Fills the code constant buffers for this view (one per technique and
    /// binding; a buffer whose contents did not change is not written).
    pub fn update(&mut self, consts: &CodeConsts) {
        if let (Some(f), Some(near)) = (&self.float_z, consts.get(crate::lighting_globals::Z_NEAR).and_then(|r| r.first()).map(|r| r[0])) {
            f.set_near(&self.gpu.queue, near);
        }
        // The sky box sits at the eye (see `Scene::read_sky`).
        if let (Some(b), Some(e)) = (self.scene.sky_bone, consts.get(crate::code::EYE_OFFSET).and_then(|r| r.first())) {
            let mut m = [0.0f32; 16];
            (m[0], m[5], m[10], m[15]) = (1.0, 1.0, 1.0, 1.0);
            (m[3], m[7], m[11]) = (e[0], e[1], e[2]);
            self.gpu.queue.write_buffer_p(&self.bones, 64 * b as u64, bytemuck::cast_slice(&m));
        }
        // (No allocation per frame: one scratch, the names of missing
        // constants only made when the set of them changes.)
        let mut bytes = std::mem::take(&mut self.code_scratch);
        let mut missing_any = false;
        for c in self.code.values_mut() {
            bytes.clear();
            bytes.resize(c.size, 0);
            for a in &c.args {
                let index = (a.u[0] & 0xff) as u16;
                let Some(rows) = consts.get(index) else {
                    missing_any = true;
                    continue;
                };
                let src: &[u8] = bytemuck::cast_slice(rows);
                let at = a.location as usize;
                let n = (a.size as usize).min(src.len()).min(c.size.saturating_sub(at));
                bytes[at..at + n].copy_from_slice(&src[..n]);
            }
            if bytes != c.last {
                self.gpu.queue.write_buffer_p(&c.buffer, 0, &bytes);
                c.last.clear();
                c.last.extend_from_slice(&bytes);
            }
        }
        self.code_scratch = bytes;
        // (Named again only when constants go missing or come back.)
        if missing_any != !self.report.code_constants_missing.is_empty() {
            let mut missing = Vec::new();
            for c in self.code.values() {
                for a in &c.args {
                    let index = (a.u[0] & 0xff) as u16;
                    if consts.get(index).is_none() {
                        missing.push(format!("{:#x} {}", index, crate::code::name(index)));
                    }
                }
            }
            missing.sort();
            missing.dedup();
            self.report.code_constants_missing = missing;
        }
    }

    /// bo2mp: the static models' siege animations advanced to `now`
    /// (seconds of frame clock), their part bones rewritten
    /// (`crate::siege`). Returns how many models play.
    pub fn update_siege(&mut self, now: f32) -> usize {
        let Ok(mut s) = self.scene.siege.lock() else { return 0 };
        if s.playing.is_empty() {
            return 0;
        }
        let (gpu, bones) = (&self.gpu, &self.bones);
        s.update(now, &mut |first, m| gpu.queue.write_buffer_p(bones, 64 * u64::from(first), bytemuck::cast_slice(m)))
    }

    /// The effects' model element particles at `now_ms` (frame clock, ms)
    /// seen from `eye`: the nearest of each model take its pool's bone
    /// matrices, the rest all zero (see `Scene::fx_models`). Returns how
    /// many were placed and how many alive.
    pub fn update_fx_models(&mut self, now_ms: i32, eye: [f32; 3]) -> (usize, usize) {
        if self.scene.fx_models.is_empty() {
            return (0, 0);
        }
        let mut alive = Vec::new();
        self.scene.fx.models(now_ms, eye, &mut alive);
        let mut placed = 0;
        for &(model, first, n) in &self.scene.fx_models {
            let mut bones = vec![[0.0f32; 16]; n as usize];
            for (slot, m) in bones.iter_mut().zip(alive.iter().filter(|m| m.model == model)) {
                *slot = m.bone;
                placed += 1;
            }
            self.gpu.queue.write_buffer_p(&self.bones, 64 * u64::from(first), bytemuck::cast_slice(&bones));
        }
        (placed, alive.len())
    }

    /// Records the pass into `targets`.
    pub fn record(&self, enc: &mut wgpu::CommandEncoder, targets: &Targets) {
        let split = self.ready.iter().position(|r| r.pipeline.reads_resolved_normal).unwrap_or(self.ready.len());
        if let Some(c) = &self.cull {
            c.record(enc);
            crate::timing::mark(enc, "gbuffer cull");
        }
        self.pass(enc, targets, 0, &self.ready[..split], true);
        // (floatZ from the opaque list's depth, for the volume decals.)
        if let Some(f) = &self.float_z
            && split < self.ready.len()
        {
            f.record(enc);
        }
        enc.copy_texture_to_texture(targets.colour[1].texture.as_image_copy(), targets.resolved_normal.texture.as_image_copy(), wgpu::Extent3d { width: targets.width, height: targets.height, depth_or_array_layers: 1 });
        if split < self.ready.len() {
            self.pass(enc, targets, 1, &self.ready[split..], false);
        }
    }

    /// The vertex buffers, index buffer and draw of `r`, after whatever
    /// state the last draw left (`last`: its pipeline, bindings and vertex
    /// source). `set_*` are the recorder's own calls (a pass's or a
    /// bundle's).
    fn pass(&self, enc: &mut wgpu::CommandEncoder, targets: &Targets, list: usize, draws: &[Ready], clear: bool) {
        let colour: Vec<Option<wgpu::RenderPassColorAttachment>> = targets
            .colour
            .iter()
            .map(|t| {
                Some(wgpu::RenderPassColorAttachment {
                    view: &t.view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations { load: if clear { wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT) } else { wgpu::LoadOp::Load }, store: wgpu::StoreOp::Store },
                })
            })
            .collect();
        let mut rp = enc.render_pass_p(&wgpu::RenderPassDescriptor {
            label: Some("t7 gbuffer"),
            color_attachments: &colour,
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: &targets.depth.view,
                depth_ops: Some(wgpu::Operations { load: if clear { wgpu::LoadOp::Clear(0.0) } else { wgpu::LoadOp::Load }, store: wgpu::StoreOp::Store }),
                stencil_ops: Some(wgpu::Operations { load: if clear { wgpu::LoadOp::Clear(0) } else { wgpu::LoadOp::Load }, store: wgpu::StoreOp::Store }),
            }),
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        if bundles_on() {
            for seg in &self.bundles[list] {
                rp.set_stencil_reference(seg.stencil);
                rp.execute_bundles(std::iter::once(&seg.bundle));
                crate::profile::replay(&seg.counts);
            }
        } else {
            let base = if list == 0 { 0 } else { self.ready.len() - draws.len() };
            self.draw_list(&mut rp, draws, base);
        }
    }

    /// `draws` as bundles, one per run of one stencil reference.
    fn segments(&self, draws: &[Ready], base: usize) -> Vec<Segment> {
        let stencil = |r: &Ready| u32::from(r.stencil);
        let mut out = Vec::new();
        let mut i = 0;
        while i < draws.len() {
            let sref = stencil(&draws[i]);
            let mut j = i;
            while j < draws.len() && stencil(&draws[j]) == sref {
                j += 1;
            }
            let mut be = self.gpu.device.create_render_bundle_encoder(&wgpu::RenderBundleEncoderDescriptor {
                label: Some("t7 gbuffer draws"),
                color_formats: &FORMATS.map(Some),
                depth_stencil: Some(wgpu::RenderBundleDepthStencil { format: DEPTH, depth_read_only: false, stencil_read_only: false }),
                sample_count: 1,
                multiview: None,
            });
            // (Counted on this thread while recorded, whether or not the
            // profile is on.)
            crate::profile::bundle_take();
            self.draw_list(&mut be, &draws[i..j], base + i);
            let counts = crate::profile::bundle_take();
            out.push(Segment { stencil: sref, bundle: be.finish(&wgpu::RenderBundleDescriptor { label: Some("t7 gbuffer draws") }), counts });
            i = j;
        }
        out
    }

    /// Records `draws` into a pass or a bundle; a binding or buffer the
    /// last draw already set is not set again.
    /// `base`: the first draw's index in `ready` (its indirect arguments'
    /// place when culled).
    fn draw_list<'a, R: Recorder<'a>>(&'a self, rp: &mut R, draws: &'a [Ready], base: usize) {
        let mut last: Option<(&Arc<Pipeline>, &Arc<Bound>, u8)> = None;
        let mut stencil = u32::MAX;
        let ids = self.cull.as_ref().map_or(&self.instance_ids, |c| &c.ids);
        for (j, r) in draws.iter().enumerate() {
            let d = &self.scene.draws[r.draw];
            // (Without the GPU culling every LOD's draw would draw every
            // placement: the finest only, as before.)
            if self.cull.is_none() && self.scene.lod_alternate(d) {
                continue;
            }
            let same_pipeline = last.is_some_and(|(p, _, _)| Arc::ptr_eq(p, &r.pipeline));
            if !same_pipeline {
                rp.pipeline(&r.pipeline.pipeline);
            }
            if !same_pipeline || !last.is_some_and(|(_, b, _)| Arc::ptr_eq(b, &r.bound)) {
                for (g, bg) in r.bound.groups.iter().enumerate() {
                    rp.bind_group(g as u32, bg);
                }
            }
            if u32::from(r.stencil) != stencil {
                stencil = u32::from(r.stencil);
                rp.stencil(stencil);
            }
            if !same_pipeline || last.is_some_and(|(_, _, g)| g != d.group) {
                for (k, &slot) in r.pipeline.slots.iter().enumerate() {
                    let buf = match (d.group, slot) {
                        (_, layout::INSTANCE_SLOT) => ids,
                        (3, 0) => &self.world[0],
                        (3, 1) => &self.world[1],
                        (1, 0) => &self.models[0],
                        (1, 1) => &self.models[1],
                        (1, 2) => &self.models[2],
                        _ => continue,
                    };
                    rp.vertex(k as u32, buf);
                }
                let ib = if d.group == 3 { &self.world[2] } else { &self.models[3] };
                rp.index(ib);
            }
            match &self.cull {
                Some(c) => rp.draw_indirect(&c.args, crate::cull::Cull::offset(base + j)),
                None => rp.draw(d.first_index..d.first_index + d.index_count, d.base_vertex, d.instance..d.instance + d.instances),
            }
            last = Some((&r.pipeline, &r.bound, d.group));
        }
    }
}

/// Whether the G-buffer's draws are replayed as render bundles
/// (`T7_NO_BUNDLES=1`: recorded draw by draw every frame, to compare).
pub(crate) fn bundles_on() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| !std::env::var("T7_NO_BUNDLES").is_ok_and(|v| v != "0"))
}

/// What a render pass and a render bundle encoder both take.
pub(crate) trait Recorder<'a> {
    fn pipeline(&mut self, p: &'a wgpu::RenderPipeline);
    fn bind_group(&mut self, i: u32, g: &'a wgpu::BindGroup);
    fn stencil(&mut self, r: u32);
    fn vertex(&mut self, slot: u32, b: &'a wgpu::Buffer);
    fn index(&mut self, b: &'a wgpu::Buffer);
    fn draw(&mut self, indices: std::ops::Range<u32>, base_vertex: i32, instances: std::ops::Range<u32>);
    fn draw_indirect(&mut self, args: &'a wgpu::Buffer, offset: u64);
}

impl<'a> Recorder<'a> for wgpu::RenderPass<'_> {
    fn pipeline(&mut self, p: &'a wgpu::RenderPipeline) {
        self.set_pipeline_p(p);
    }
    fn bind_group(&mut self, i: u32, g: &'a wgpu::BindGroup) {
        self.set_bind_group_p(i, g, &[]);
    }
    fn stencil(&mut self, r: u32) {
        self.set_stencil_reference(r);
    }
    fn vertex(&mut self, slot: u32, b: &'a wgpu::Buffer) {
        self.set_vertex_buffer(slot, b.slice(..));
    }
    fn index(&mut self, b: &'a wgpu::Buffer) {
        self.set_index_buffer(b.slice(..), wgpu::IndexFormat::Uint16);
    }
    fn draw(&mut self, indices: std::ops::Range<u32>, base_vertex: i32, instances: std::ops::Range<u32>) {
        self.draw_indexed_p(indices, base_vertex, instances);
    }
    fn draw_indirect(&mut self, args: &'a wgpu::Buffer, offset: u64) {
        crate::profile::draws(1);
        self.draw_indexed_indirect(args, offset);
    }
}

impl<'a> Recorder<'a> for wgpu::RenderBundleEncoder<'a> {
    fn pipeline(&mut self, p: &'a wgpu::RenderPipeline) {
        crate::profile::bundle(|c| c.pipelines += 1);
        self.set_pipeline(p);
    }
    fn bind_group(&mut self, i: u32, g: &'a wgpu::BindGroup) {
        crate::profile::bundle(|c| c.binds += 1);
        self.set_bind_group(i, g, &[]);
    }
    // (A bundle takes no stencil reference: the passes set it between bundles.)
    fn stencil(&mut self, _r: u32) {}
    fn vertex(&mut self, slot: u32, b: &'a wgpu::Buffer) {
        self.set_vertex_buffer(slot, b.slice(..));
    }
    fn index(&mut self, b: &'a wgpu::Buffer) {
        self.set_index_buffer(b.slice(..), wgpu::IndexFormat::Uint16);
    }
    fn draw(&mut self, indices: std::ops::Range<u32>, base_vertex: i32, instances: std::ops::Range<u32>) {
        crate::profile::bundle(|c| c.draws += 1);
        self.draw_indexed(indices, base_vertex, instances);
    }
    fn draw_indirect(&mut self, args: &'a wgpu::Buffer, offset: u64) {
        crate::profile::bundle(|c| c.draws += 1);
        self.draw_indexed_indirect(args, offset);
    }
}

#[cfg(test)]
mod oom_tests {
    use super::*;

    /// The out-of-memory cap only falls, and a new map lifts it.
    #[test]
    fn oom_cap_steps_down() {
        reset_oom_cap();
        assert_eq!(oom_cap(), None);
        assert!(lower_oom_cap(512));
        assert_eq!(oom_cap(), Some(512));
        assert!(!lower_oom_cap(1024), "never raised");
        assert_eq!(oom_cap(), Some(512));
        assert!(lower_oom_cap(256));
        assert_eq!(oom_cap(), Some(256));
        reset_oom_cap();
        assert_eq!(oom_cap(), None);
    }

    /// A texture the device cannot hold, made inside the scope
    /// `upload_or_oom` uses, is an error of the scope's, not the device's
    /// uncaptured error (which ends the game).
    #[test]
    fn out_of_memory_is_caught_by_the_scope() {
        let Ok(gpu) = Gpu::headless() else { return eprintln!("skipped: no device") };
        let caught = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let c = caught.clone();
        gpu.device.on_uncaptured_error(std::sync::Arc::new(move |e| {
            eprintln!("uncaptured: {e}");
            c.store(true, std::sync::atomic::Ordering::Relaxed);
        }));
        let l = gpu.device.limits();
        let side = l.max_texture_dimension_2d.min(16384);
        let layers = l.max_texture_array_layers.min(2048);
        let scope = gpu.device.push_error_scope(wgpu::ErrorFilter::OutOfMemory);
        let _t = gpu.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("too big"),
            size: wgpu::Extent3d { width: side, height: side, depth_or_array_layers: layers },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba32Float,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let e = pollster::block_on(scope.pop());
        eprintln!("scope: {e:?}");
        assert!(!caught.load(std::sync::atomic::Ordering::Relaxed), "reached the uncaptured handler");
        assert!(matches!(e, Some(wgpu::Error::OutOfMemory { .. })), "{side}x{side}x{layers}: {e:?}");
    }
}
