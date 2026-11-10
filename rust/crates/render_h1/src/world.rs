//! A map's world surfaces drawn with each material's own technique.

use std::collections::{BTreeMap, HashMap};

use dxbc_sm5::{Options, Stage, TextureKind};
use fastfile_h1::AssetType;
use fastfile_h1::image::ImagePixels;
use fastfile_h1::walk::{MaterialConstant, PassRec, TechniqueRec};

use crate::bind::{self, CbSource, CbWrite, MaterialView, PassBinds, SampSource, TexSource};
use crate::code::{CodeValues, code_index};
use crate::gpu::{self, Gpu, PassLayout, Tex};
use crate::pool::{Pool, key};
use crate::scene::Scene;
use crate::state::PassState;

/// Technique slots (`MaterialTechniqueType`).
pub mod slot {
    pub const ZPREPASS: usize = 0;
    /// The sun shadow map's casters (`build_shadowmap_*`).
    pub const SHADOW_CASTER: usize = 6;
    /// The translucent casters (`TRANS_SHADOWMAP_SUN`; inferred: the slot
    /// after the depth casters, as in IW's earlier engines).
    pub const TRANS_SHADOW_CASTER: usize = 7;
    pub const UNLIT: usize = 8;
    pub const EMISSIVE: usize = 9;
    pub const LIT: usize = 13;
    pub const LIT_DIR: usize = 14;
    pub const LIT_DIR_SHADOW: usize = 15;
    /// The spot and omni lit passes (`l_spot_*`, `l_omni_*`), read from the
    /// techsets' technique names (3 Nov): 37 and 38, with `_dfog` forms at
    /// 43 and 44 and the shadowed spot at 39 (not drawn).
    pub const LIGHT_SPOT: usize = 37;
    pub const LIGHT_OMNI: usize = 38;
    /// Added to a standard slot: the same technique without displacement
    /// (no hull/domain shaders).
    pub const NO_DISPLACEMENT: usize = 180;
}

/// The "world vertex format" a model draw is given when its placement is
/// vertex-lit (its second stream holds the placement's lighting values).
pub const VERTEX_LIT_WVF: u8 = 254;

/// `Material::cameraRegion` of materials no camera pass draws.
pub const CAMERA_REGION_NONE: u8 = 12;

#[derive(Clone, Debug, Default)]
pub struct Material {
    pub name: String,
    pub techset: String,
    /// (name hash, sampler state, semantic, image name)
    pub textures: Vec<(u32, u8, u8, String)>,
    pub constants: Vec<MaterialConstant>,
    /// `GfxStateBits` blocks and, per technique slot, the first block its
    /// passes use (255: none).
    pub state_bits: Vec<[u8; 40]>,
    pub state_entry: Vec<u8>,
    pub sort_key: u8,
    /// `Material::cameraRegion` (which camera pass draws it).
    pub camera_region: u8,
}

impl Material {
    /// The fixed-function state of pass `pass` of the technique in `slot`.
    pub fn state(&self, slot: usize, pass: usize) -> PassState {
        match self.state_entry.get(slot) {
            Some(&e) if e != 255 => self
                .state_bits
                .get(usize::from(e) + pass)
                .map_or(PassState::default(), |b| PassState::from_bits(b)),
            _ => PassState::default(),
        }
    }
}

/// One draw: a run of a surface's triangles.
#[derive(Clone, Copy, Debug)]
pub struct Draw {
    /// Which of `Meshes` it draws from.
    pub mesh: usize,
    /// Per-object code constants (`Object`), if any.
    pub object: Option<u32>,
    pub material: usize,
    pub lightmap: u8,
    pub probe: u8,
    pub first_index: u32,
    pub index_count: u32,
    pub base_vertex: i32,
    /// Byte offset of its run in the mesh's vertex layer stream.
    pub layer: Option<u32>,
    /// Its primary light environment (`ComPrimaryLightEnv` index).
    pub light_env: u16,
    /// A placed model's level of detail, if it has more than one.
    pub lod: Option<Lod>,
}

/// One level of detail of a placed model: used while the eye is between
/// `min` and `max` inches from `center` (`max` is infinity for the last
/// level; `min` is 0 for the first).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Lod {
    pub level: u8,
    pub min: f32,
    pub max: f32,
    pub center: [f32; 3],
}

impl Lod {
    /// Whether it is the level to draw at `dist` inches, the distances
    /// scaled by `k` (Graphics > LOD distance).
    pub fn selects(&self, dist: f32, k: f32) -> bool {
        dist >= self.min * k && dist < self.max * k
    }
}

/// Where pictures come from: (pixels, faces: 1 or 6).
pub trait ImageSource {
    fn image(&mut self, name: &str) -> Option<(ImagePixels, u32)>;
}

/// A mesh's vertex layout.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum VertexKind {
    /// `GfxWorldVertex`, 44 bytes.
    World,
    /// `GfxPackedVertex` (model surfaces), 32 bytes, then the model
    /// lightmap unwrap (two halves).
    Model,
}

impl VertexKind {
    pub fn stride(self) -> u64 {
        match self {
            Self::World => 44,
            Self::Model => 36,
        }
    }
    pub fn attribute(self, semantic: &str, index: u32) -> Option<(u64, wgpu::VertexFormat)> {
        match self {
            Self::World => world_attribute(semantic, index),
            Self::Model => model_attribute(semantic, index),
        }
    }
}

/// The packed normal/tangent format (`H1_DBG_NOR8`: read as 8-bit, debug).
fn packed_normal() -> wgpu::VertexFormat {
    if std::env::var("H1_DBG_NOR8").is_ok() {
        wgpu::VertexFormat::Unorm8x4
    } else {
        wgpu::VertexFormat::Unorm10_10_10_2
    }
}

/// A model vertex (`GfxPackedVertex`): position and binormal sign, colour,
/// half-float uv, packed normal and tangent.
fn model_attribute(semantic: &str, index: u32) -> Option<(u64, wgpu::VertexFormat)> {
    use wgpu::VertexFormat as F;
    Some(match (semantic.to_ascii_uppercase().as_str(), index) {
        ("POSITION" | "SV_POSITION", 0) => (0, F::Float32x4),
        ("COLOR", 0) => (16, F::Unorm8x4),
        ("TEXCOORD", 0) => (20, F::Float16x2),
        ("NORMAL", 0) => (24, packed_normal()),
        ("TANGENT", 0) | ("TEXCOORD", 2) => (28, packed_normal()),
        // The model lightmap unwrap (`XSurface::lmapUnwrap`, half floats).
        ("TEXCOORD", 4) if std::env::var("H1_DBG_TC4").is_ok() => (20, F::Float16x2),
        ("TEXCOORD", 4) => (32, F::Float16x2),
        _ => return None,
    })
}

/// What a pipeline draws into: the HDR scene (reversed depth, the host's
/// depth format) or the sun shadow map (reversed depth, `Depth32Float`,
/// the game's depth test and shadow bias, no colour written), or the
/// translucent casters' colour map (`TRANS_SHADOWMAP_SUN`: the material's
/// blend into `TRANS_SHADOW_COLOUR`, tested against the cascade's depth,
/// no depth written).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Target {
    Scene,
    Shadow,
    TransShadow,
}

/// The shadow map's depth format.
pub const SHADOW_DEPTH: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;
/// The translucent casters' map (inferred: an 8-bit RGBA colour target;
/// the engine's format id is not mapped).
pub const TRANS_SHADOW_COLOUR: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;
/// The shadow pass's colour attachment (a stand-in nothing is written to:
/// every shadow pipeline has one target with an empty write mask, so the
/// casters' pixel shaders, alpha tested or not, fit one pass).
pub const SHADOW_COLOUR: wgpu::TextureFormat = wgpu::TextureFormat::R8Unorm;

/// A pipeline: technique, pass, vertex layout, fixed-function state, the
/// world vertex format of the layer stream (0: none) and its target.
type PipeKey = (String, usize, VertexKind, PassState, u8, Target);

/// Code constants one object sets for itself (its world matrix, its
/// lighting coordinates), by `code_source` index.
#[derive(Clone, Debug, Default)]
pub struct Object {
    pub rows: Vec<(u16, Vec<[f32; 4]>)>,
    /// Its placement (model to world).
    pub world: glam::Mat4,
    /// Its model lighting tile, if lit from the grid.
    pub tile: Option<usize>,
}

/// The world vertex (`GfxWorldVertex`) a vertex shader input reads.
fn world_attribute(semantic: &str, index: u32) -> Option<(u64, wgpu::VertexFormat)> {
    use wgpu::VertexFormat as F;
    Some(match (semantic.to_ascii_uppercase().as_str(), index) {
        ("POSITION" | "SV_POSITION", 0) => (0, F::Float32x4),
        ("COLOR", 0) => (16, F::Unorm8x4),
        ("TEXCOORD", 0) => (20, F::Float32x4),
        ("NORMAL", 0) => (36, packed_normal()),
        ("TANGENT", 0) | ("TEXCOORD", 2) => (40, packed_normal()),
        _ => return None,
    })
}

struct Pipeline {
    pipeline: wgpu::RenderPipeline,
    /// Reads the vertex layer stream (bound as stream 1).
    layered: bool,
    layout: PassLayout,
    vs_regs: BTreeMap<u32, u32>,
    ps_regs: BTreeMap<u32, u32>,
    /// Its pixel program can discard (alpha tested): no occluder.
    discards: bool,
    /// The same pass reading its per-object block by instance
    /// (`instancing`): pipeline, layout, block stride.
    inst: Option<(wgpu::RenderPipeline, PassLayout, u64)>,
}

/// A draw's instanced form: pipeline and constants group (its dynamic
/// offset is the first block's), and the blocks' stride.
#[derive(Clone)]
struct Inst {
    pipeline: wgpu::RenderPipeline,
    group: wgpu::BindGroup,
    stride: u32,
}

/// What the placed models' draws are (scene pass, `bo2mp h1 props:`): how
/// many are blended, and which materials, so see-through props can be told
/// from over-lit ones.
#[derive(Default, Debug)]
pub struct PropsReport {
    pub opaque: usize,
    /// Blended model draws by material: (draws, depth write, blend).
    pub blended: BTreeMap<String, (usize, bool, String)>,
    /// Model draws whose material has a picture standing in (not loaded),
    /// by material.
    pub defaulted: BTreeMap<String, usize>,
}

#[derive(Default, Debug)]
pub struct Report {
    pub props: PropsReport,
    pub draws: usize,
    pub drawn: usize,
    /// Opaque scene draws with no depth pre-pass that write depth in their
    /// own pass (`PassState::needs_own_depth`).
    pub own_depth: usize,
    pub techniques: usize,
    /// Why some materials weren't drawn, with a count.
    pub skipped: BTreeMap<String, usize>,
    pub missing_images: BTreeMap<String, String>,
    pub missing_code: Vec<String>,
    /// Caster draws in the last sun shadow cascade.
    pub shadow_drawn: usize,
    /// Caster draws the last cascade's culling planes left out.
    pub shadow_culled: usize,
    /// What the frame in progress has drawn so far (scene, local light and
    /// shadow passes summed): the host resets it at the frame's start and
    /// reads it at the end (`gpu_timer::FrameCounters`).
    pub frame: crate::gpu_timer::FrameCounters,
}

/// The occlusion buffer's size, and what counts as an occluder: a surface
/// at least this long and triangles of at least this area (MWR units are
/// inches), the largest few thousand.
const OCCLUSION_W: usize = 192;
const OCCLUSION_H: usize = 108;
const OCCLUDER_MIN_EXTENT: f32 = 48.0;
const OCCLUDER_MIN_AREA: f32 = 400.0;
const OCCLUDER_CAP: usize = 12000;
/// A draw must start this far (view depth) behind the occluder.
const OCCLUSION_MARGIN: f32 = 12.0;

/// `BO2MP_H1_GREY_ALPHA0=1`: a picture that is not loaded stands in as grey
/// with alpha 0 on opaque passes too (before: an alpha-tested prop with a
/// missing picture vanished).
fn grey_alpha0() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var("BO2MP_H1_GREY_ALPHA0").is_ok_and(|v| v.trim() == "1"))
}

/// `BO2MP_H1_OCCLUSION=0` turns the occlusion test off.
pub fn occlusion_on() -> bool {
    !std::env::var("BO2MP_H1_OCCLUSION").is_ok_and(|v| v.trim() == "0")
}

/// The angle (radians) under which a draw is too small to show: about
/// 5, 3.5, 2.5 and 1.5 pixels at 1080p on LOW, MEDIUM, HIGH and ULTRA;
/// `BO2MP_H1_SMALL_CULL` sets it (0: off).
pub fn small_angle(preset: native_quality::Preset) -> Option<f32> {
    let default = match preset {
        native_quality::Preset::Low => 0.010,
        native_quality::Preset::Medium => 0.007,
        native_quality::Preset::High => 0.005,
        native_quality::Preset::Ultra => 0.003,
    };
    let a = std::env::var("BO2MP_H1_SMALL_CULL")
        .ok()
        .and_then(|v| v.trim().parse::<f32>().ok())
        .unwrap_or(default);
    (a > 0.0).then_some(a)
}

pub struct Frame<'a> {
    pub code: &'a CodeValues,
    /// Technique slots to try in order.
    pub slots: &'a [usize],
    /// The depth pre-pass's slots (empty: none).
    pub prepass_slots: &'a [usize],
    /// Which surfaces: MWR draws the opaque ones, fogs them in a post pass,
    /// then draws the blended ones (fogged in their own shaders) on top.
    pub part: Part,
    pub target: Target,
    /// Separate constant buffers for draws of the same pipelines with
    /// other code values in one encoder (the sun shadow's cascades).
    pub variant: u8,
    /// Culling planes (`sun_shadow::Plane`): a draw whose bounds lie
    /// outside any of them is left out (None: every draw).
    pub cull: Option<&'a [glam::Vec4]>,
    /// The eye and the farthest distance a draw's bounds may start at
    /// (Graphics > draw distance; None: no limit).
    pub max_distance: Option<(glam::Vec3, f32)>,
    /// A draw with no known bounds is left out when culling (the local
    /// lights: a draw that cannot be placed inside a light's sphere is not
    /// lit by it).
    pub need_bounds: bool,
    /// The eye and the scale on the models' LOD distances for the main
    /// view; None (the shadow views): each model's first level only.
    pub lod: Option<(glam::Vec3, f32)>,
    /// The camera's occlusion buffer (`Renderer::occlusion`): draws wholly
    /// behind the big opaque surfaces are left out (None: none are).
    pub occlusion: Option<&'a native_cull::Occlusion>,
    /// Draws that span less than this angle (radians) from the eye are left
    /// out (None: none).
    pub small_angle: Option<(glam::Vec3, f32)>,
}

/// Where a frame's part is drawn: an Rgba16Float colour view and a depth
/// view of the renderer's depth format, cleared to `clear` (and depth 0)
/// or kept.
pub struct Targets<'a> {
    pub colour: &'a wgpu::TextureView,
    pub depth: &'a wgpu::TextureView,
    pub clear: Option<[f64; 4]>,
    /// The depth the clear writes (0 for the reversed scene depth, 1 for
    /// the shadow map).
    pub depth_clear: f32,
    /// A viewport (x, y, width, height, min depth, max depth) when the
    /// host draws into part of the target or the depth range (None: all).
    pub viewport: Option<[f32; 6]>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Part {
    All,
    Opaque,
    Blended,
}

/// Picks the technique a material draws with: the first of `slots` its
/// set fills, its no-displacement form when the standard one tessellates
/// or is missing (displacement sets fill only the no-displacement slots).
pub fn pick<'p>(
    pool: &'p Pool,
    techset: &str,
    slots: &[usize],
) -> Option<(usize, &'p TechniqueRec)> {
    for &s in slots {
        let std = pool.technique_in(techset, s);
        let tess = std.is_some_and(|t| t.pass_info.iter().any(|p| p.hull_shader.is_some()));
        let fallback = std.is_none() && !no_displacement_fallback_off();
        if (tess || fallback)
            && let Some(t) = pool.technique_in(techset, s + slot::NO_DISPLACEMENT)
        {
            return Some((s + slot::NO_DISPLACEMENT, t));
        }
        if let Some(t) = std
            && !tess
        {
            return Some((s, t));
        }
    }
    None
}

/// `BO2MP_H1_DISP_FALLBACK=0`: a set without the standard slot draws
/// nothing there, as before.
fn no_displacement_fallback_off() -> bool {
    static OFF: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *OFF.get_or_init(|| std::env::var("BO2MP_H1_DISP_FALLBACK").is_ok_and(|v| v.trim() == "0"))
}

/// One prepared draw: its pipeline, bind groups, the dynamic offset of its
/// object's constants and its index range.
struct Ready {
    pipeline: wgpu::RenderPipeline,
    groups: [wgpu::BindGroup; 3],
    /// Its instanced form, when its pipeline has one.
    inst: Option<Inst>,
    /// One per dynamic binding of group 0 (b0 in each stage that reads it).
    offsets: Vec<u32>,
    /// Layered: the byte offsets the main and layer streams are bound at
    /// (the draw's base vertex is then 0).
    layer: Option<(u64, u64)>,
    mesh: usize,
    first: u32,
    count: u32,
    base: i32,
    /// The draw's world bounds, if known.
    bounds: Option<(glam::Vec3, glam::Vec3)>,
    lod: Option<Lod>,
}

/// Joins each draw to the one before it when both use the same pipeline,
/// bind groups, constants offsets and vertex streams, their index ranges
/// touch, and their bounds (kept as a union) stay close (the union's volume
/// at most 1.5 times the two boxes'), so culling keeps its grain.
fn merge_ready(list: Vec<Ready>) -> Vec<Ready> {
    let vol = |(lo, hi): (glam::Vec3, glam::Vec3)| {
        let d = (hi - lo).max(glam::Vec3::splat(1.0));
        d.x * d.y * d.z
    };
    let mut out: Vec<Ready> = Vec::with_capacity(list.len());
    for r in list {
        if let Some(p) = out.last_mut()
            && p.pipeline == r.pipeline
            && p.groups == r.groups
            && p.offsets == r.offsets
            && p.layer == r.layer
            && p.mesh == r.mesh
            && p.base == r.base
            && p.first + p.count == r.first
            && p.lod == r.lod
        {
            let bounds = match (p.bounds, r.bounds) {
                (None, None) => Some(None),
                (Some(a), Some(b)) => {
                    let u = (a.0.min(b.0), a.1.max(b.1));
                    (vol(u) <= 1.5 * (vol(a) + vol(b))).then_some(Some(u))
                }
                _ => None,
            };
            if let Some(bounds) = bounds {
                p.count += r.count;
                p.bounds = bounds;
                continue;
            }
        }
        out.push(r);
    }
    out
}

/// The surfaces prepared for a set of technique slots: the opaque ones
/// (pre-pass and lit) and the blended ones.
struct Prepared {
    opaque: Vec<Ready>,
    blended: Vec<Ready>,
    /// Plans whose constants are rewritten each frame.
    plans: Vec<usize>,
}

/// A constant buffer's place in the arena.
#[derive(Clone, Copy, Debug)]
struct Slice {
    page: usize,
    offset: u64,
    size: u64,
}

/// A page: its buffer, its CPU mirror, the byte range changed since the
/// last flush.
type ArenaPage = (wgpu::Buffer, Vec<u8>, Option<(u64, u64)>);

/// The frame's and the materials' constant buffers in a few large uniform
/// buffers, mirrored on the CPU: a frame's updates are written into the
/// mirror and each page that changed goes up in one write.
struct Arena {
    pages: Vec<ArenaPage>,
    align: u64,
}

const PAGE: u64 = 4 << 20;

impl Arena {
    fn alloc(&mut self, gpu: &Gpu, size: u64) -> Slice {
        let size = size.max(16);
        if !self.pages.is_empty() {
            let i = self.pages.len() - 1;
            let offset = (self.pages[i].1.len() as u64).next_multiple_of(self.align);
            if offset + size <= PAGE {
                self.pages[i].1.resize((offset + size) as usize, 0);
                return Slice {
                    page: i,
                    offset,
                    size,
                };
            }
        }
        let buffer = gpu.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("h1_constants"),
            size: PAGE,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        self.pages.push((buffer, vec![0; size as usize], None));
        Slice {
            page: self.pages.len() - 1,
            offset: 0,
            size,
        }
    }

    fn write(&mut self, at: Slice, regs: &[[f32; 4]]) {
        let (_, m, dirty) = &mut self.pages[at.page];
        let bytes: &[u8] = bytemuck::cast_slice(regs);
        let n = bytes.len().min(at.size as usize);
        m[at.offset as usize..at.offset as usize + n].copy_from_slice(&bytes[..n]);
        let end = at.offset + n as u64;
        *dirty = Some(match *dirty {
            Some((a, b)) => (a.min(at.offset), b.max(end)),
            None => (at.offset, end),
        });
    }

    fn flush(&mut self, queue: &wgpu::Queue) {
        for (b, m, dirty) in &mut self.pages {
            if let Some((a, e)) = dirty.take() {
                let e = e.next_multiple_of(4).min(m.len() as u64);
                queue.write_buffer(b, a, &m[a as usize..e as usize]);
            }
        }
    }

    fn binding(&self, at: Slice) -> wgpu::BindingResource<'_> {
        wgpu::BindingResource::Buffer(wgpu::BufferBinding {
            buffer: &self.pages[at.page].0,
            offset: at.offset,
            size: std::num::NonZeroU64::new(at.size),
        })
    }
}

/// Bind groups are made once per (pipeline, material, lightmap, probe);
/// the object's constants are picked by dynamic offset.
type GroupKey = (PipeKey, u8, usize, u8, u8);

/// What the draws were prepared for: the lit and pre-pass slots, the
/// target and the constants variant.
type PrepKey = (Vec<usize>, Vec<usize>, Target, u8);

/// A pipeline's constant buffers: the pass's code and literal writes, each
/// slot's size in registers, and its per-object block (b0): one aligned
/// block per object drawn with it (the world's surfaces are one "object").
struct CbPlan {
    writes: Vec<CbWrite>,
    sizes: BTreeMap<u32, u32>,
    /// b0's block size, aligned.
    stride: u64,
    objects: Vec<Option<u32>>,
    object_slot: HashMap<Option<u32>, u32>,
    per_object: Option<wgpu::Buffer>,
    mirror: Vec<u8>,
    /// The frame's slots (b1, b2...) in the arena.
    frame: BTreeMap<u32, Slice>,
}

/// Draws a scene with MWR's techniques. Long-lived: pipelines, textures,
/// buffers and bind groups are made once; each frame rewrites the frame's
/// and the objects' constants and records the draws.
pub struct Renderer {
    pub gpu: Gpu,
    pub scene: Scene,
    /// Depth target format (Depth32Float offline; the host's otherwise).
    pub depth_format: wgpu::TextureFormat,
    /// Code textures by name (`MODEL_LIGHTING`, and any the caller adds).
    pub code_textures: HashMap<String, Tex>,
    /// `LIGHTING_LOOKUP_SCALE` of the model lighting atlas.
    pub lighting_lookup: [f32; 4],
    tex: HashMap<String, Option<Tex>>,
    pipelines: HashMap<PipeKey, Option<Pipeline>>,
    samplers: HashMap<String, wgpu::Sampler>,
    defaults: HashMap<String, Tex>,
    mesh_buffers: Vec<(wgpu::Buffer, wgpu::Buffer, Option<wgpu::Buffer>)>,
    /// Vertex layer stream layouts by world vertex format.
    layer_layouts: HashMap<u8, crate::layers::Layout>,
    plans: Vec<CbPlan>,
    /// Reused by every per-object constants fill.
    fill_scratch: Vec<[f32; 4]>,
    /// Bytes of per-object constants written to the GPU by the last update.
    uploaded: u64,
    plan_of: HashMap<(PipeKey, u8), usize>,
    /// Each material's own constants (b3), per plan.
    material_cb: HashMap<(usize, usize, u32), Slice>,
    arena: Arena,
    groups: HashMap<GroupKey, ([wgpu::BindGroup; 3], Option<wgpu::BindGroup>)>,
    prepared: HashMap<PrepKey, Prepared>,
    /// Code constant indices looked up once.
    wvp_index: u16,
    lookup_index: u16,
    pub report: Report,
    /// Each scene draw's world bounds (`draw_bounds`), for culling.
    pub bounds: Vec<Option<(glam::Vec3, glam::Vec3)>>,
    /// The map's big opaque triangles, world space (found once, by the
    /// first scene `prepare`).
    occluders: Option<Vec<native_cull::Occluder>>,
    /// The last view's occlusion buffer, kept while the view stands still
    /// (time slicing: the work is done when the view changes, not per frame).
    occlusion_cache: Option<native_cull::Occlusion>,
}

fn inv_wv_index() -> u16 {
    static I: std::sync::OnceLock<u16> = std::sync::OnceLock::new();
    *I.get_or_init(|| code_index("INVERSE_TRANSPOSE_WORLD_VIEW_MATRIX0"))
}

/// Code values `fill` reads.
struct FillCtx<'a> {
    code: &'a CodeValues,
    lookup: [f32; 4],
    wvp: u16,
    lookup_i: u16,
}

/// The registers `writes` put in constant buffer `slot`, `size` long.
fn fill(
    writes: &[CbWrite],
    slot: u32,
    size: u32,
    obj: Option<&Object>,
    cx: &FillCtx,
) -> Vec<[f32; 4]> {
    let mut out = Vec::new();
    fill_into(writes, slot, size, obj, cx, &mut out);
    out
}

/// [`fill`] into `out` (cleared first, its allocation kept): nothing is
/// allocated per call once `out` has grown to the buffer's size.
fn fill_into(
    writes: &[CbWrite],
    slot: u32,
    size: u32,
    obj: Option<&Object>,
    cx: &FillCtx,
    out: &mut Vec<[f32; 4]>,
) {
    out.clear();
    out.resize(size.max(1) as usize, [0.0f32; 4]);
    for w in writes.iter().filter(|w| w.buffer == slot) {
        match &w.src {
            CbSource::Code { index, first_row, rows } => {
                // The object's own rows, else its matrices, else the frame's.
                let mats: Option<[[f32; 4]; 4]> = if obj.is_some_and(|o| o.rows.iter().any(|(i, _)| i == index)) {
                    None
                } else if *index == cx.wvp {
                    obj.map(|o| {
                        let m = cx.code.view_projection * glam::Mat4::from_translation(-cx.code.eye) * o.world;
                        std::array::from_fn(|r| m.row(r).to_array())
                    })
                } else if *index == inv_wv_index() {
                    obj.map(|o| {
                        let m = (cx.code.rotation * glam::Mat4::from_translation(-cx.code.eye) * o.world).inverse();
                        std::array::from_fn(|r| m.row(r).to_array())
                    })
                } else {
                    None
                };
                let own: Option<&[[f32; 4]]> = obj
                    .and_then(|o| o.rows.iter().find(|(i, _)| i == index))
                    .map(|(_, v)| v.as_slice())
                    .or(mats.as_ref().map(|m| m.as_slice()))
                    .or_else(|| (*index == cx.lookup_i).then_some(std::slice::from_ref(&cx.lookup)));
                let end = w.dest as usize + *rows as usize;
                if out.len() < end {
                    out.resize(end, [0.0; 4]);
                }
                for r in 0..*rows as usize {
                    let row = *first_row as usize + r;
                    out[w.dest as usize + r] = match own {
                        Some(v) => v.get(row).copied().unwrap_or([0.0; 4]),
                        None => cx.code.row(*index, row),
                    };
                }
            }
            CbSource::Literal(v) | CbSource::Material(v) => {
                let end = w.dest as usize + 1;
                if out.len() < end {
                    out.resize(end, [0.0; 4]);
                }
                out[w.dest as usize] = *v;
            }
            CbSource::MissingMaterial(_) => {
                let end = w.dest as usize + 1;
                if out.len() < end {
                    out.resize(end, [0.0; 4]);
                }
                out[w.dest as usize] = [0.0; 4];
            }
        }
    }
}

/// One surface's pass, found in the first phase of `prepare`.
struct Item {
    pipe: PipeKey,
    plan: usize,
    binds: PassBinds,
    draw: Draw,
    blended: bool,
    bounds: Option<(glam::Vec3, glam::Vec3)>,
    /// Index in the scene's draws.
    di: usize,
    /// Opaque, writes depth, cannot discard: its big triangles occlude.
    occluder: bool,
    /// Which side of its triangles the game draws (`native_cull::Occluder`).
    front: i8,
}

/// Each draw's world bounds (min, max) from its vertices, placed by its
/// object (None: no vertices read).
pub fn draw_bounds(scene: &Scene) -> Vec<Option<(glam::Vec3, glam::Vec3)>> {
    scene
        .draws
        .iter()
        .map(|d| {
            let m = scene.meshes.get(d.mesh)?;
            let stride = m.kind.stride() as usize;
            let first = d.first_index as usize;
            let idx = m.indices.get(first..first + d.index_count as usize)?;
            let (mut lo, mut hi) = (glam::Vec3::splat(f32::MAX), glam::Vec3::splat(f32::MIN));
            for &i in idx {
                let v = (d.base_vertex + i32::from(i)) as usize * stride;
                let b = m.vertex_bytes.get(v..v + 12)?;
                let p = glam::Vec3::from_array(std::array::from_fn(|k| {
                    f32::from_le_bytes([b[4 * k], b[4 * k + 1], b[4 * k + 2], b[4 * k + 3]])
                }));
                lo = lo.min(p);
                hi = hi.max(p);
            }
            if lo.x > hi.x {
                return None;
            }
            match d.object.and_then(|o| scene.objects.get(o as usize)) {
                Some(o) if o.world != glam::Mat4::IDENTITY => {
                    let (mut a, mut b) = (glam::Vec3::splat(f32::MAX), glam::Vec3::splat(f32::MIN));
                    for c in 0..8 {
                        let p = glam::Vec3::new(
                            if c & 1 == 0 { lo.x } else { hi.x },
                            if c & 2 == 0 { lo.y } else { hi.y },
                            if c & 4 == 0 { lo.z } else { hi.z },
                        );
                        let q = o.world.transform_point3(p);
                        a = a.min(q);
                        b = b.max(q);
                    }
                    Some((a, b))
                }
                _ => Some((lo, hi)),
            }
        })
        .collect()
}

impl Renderer {
    pub fn new(gpu: Gpu, mut scene: Scene, depth_format: wgpu::TextureFormat) -> Self {
        let mesh_buffers = scene
            .meshes
            .iter()
            .map(|m| {
                let mut vb: Vec<u8> = m.vertex_bytes.clone();
                if vb.is_empty() {
                    vb.resize(64, 0);
                }
                let vb = gpu.device.create_buffer_init_vertex(&vb);
                let mut idx: Vec<u8> = bytemuck::cast_slice(&m.indices).to_vec();
                while idx.is_empty() || !idx.len().is_multiple_of(4) {
                    idx.extend_from_slice(&[0, 0]);
                }
                let ib = wgpu::util::DeviceExt::create_buffer_init(
                    &gpu.device,
                    &wgpu::util::BufferInitDescriptor {
                        label: Some("ib"),
                        contents: &idx,
                        usage: wgpu::BufferUsages::INDEX,
                    },
                );
                let lb = (!m.layer_bytes.is_empty())
                    .then(|| gpu.device.create_buffer_init_vertex(&m.layer_bytes));
                (vb, ib, lb)
            })
            .collect();
        let bounds = draw_bounds(&scene);
        let atlas = crate::model_lighting::Atlas::build(&gpu, &scene.tiles);
        let scene_pool = scene.pool.clone();
        // Objects name their tile; the shaders want its coordinates.
        let base_coords = code_index("BASE_LIGHTING_COORDS");
        for o in &mut scene.objects {
            let Some(t) = o.tile else { continue };
            for (i, rows) in &mut o.rows {
                if *i == base_coords {
                    rows[0] = atlas.coords(t);
                }
            }
        }
        let lighting_lookup = atlas.lookup_scale();
        let mut code_textures = HashMap::new();
        code_textures.insert("MODEL_LIGHTING".to_string(), atlas.tex);
        let align = u64::from(gpu.device.limits().min_uniform_buffer_offset_alignment);
        Self {
            gpu,
            scene,
            depth_format,
            code_textures,
            lighting_lookup,
            tex: HashMap::new(),
            pipelines: HashMap::new(),
            samplers: HashMap::new(),
            defaults: HashMap::new(),
            mesh_buffers,
            layer_layouts: crate::layers::layouts(&scene_pool),
            plans: Vec::new(),
            fill_scratch: Vec::new(),
            uploaded: 0,
            plan_of: HashMap::new(),
            material_cb: HashMap::new(),
            arena: Arena {
                pages: Vec::new(),
                align,
            },
            groups: HashMap::new(),
            prepared: HashMap::new(),
            wvp_index: code_index("TRANSPOSE_WORLD_VIEW_PROJECTION_MATRIX0"),
            lookup_index: code_index("LIGHTING_LOOKUP_SCALE"),
            report: Report::default(),
            bounds,
            occluders: None,
            occlusion_cache: None,
        }
    }

    fn pipeline(
        &mut self,
        tech: &TechniqueRec,
        pass_i: usize,
        kind: VertexKind,
        state: PassState,
        wvf: u8,
        target: Target,
    ) -> PipeKey {
        let k: PipeKey = (key(&tech.name), pass_i, kind, state, wvf, target);
        if !self.pipelines.contains_key(&k) {
            let made = self.make_pipeline(&tech.pass_info[pass_i], kind, state, wvf, target);
            if let Err(e) = &made {
                *self.report.skipped.entry(e.clone()).or_default() += 1;
            }
            self.pipelines.insert(k.clone(), made.ok());
        }
        k
    }

    fn plan(&mut self, pipe: &PipeKey, variant: u8, binds: &PassBinds) -> Result<usize, String> {
        if let Some(&p) = self.plan_of.get(&(pipe.clone(), variant)) {
            return Ok(p);
        }
        let p = self.pipelines[pipe].as_ref().ok_or("no pipeline")?;
        let mut sizes: BTreeMap<u32, u32> = BTreeMap::new();
        for (s, n) in p
            .vs_regs
            .iter()
            .map(|(s, n)| (bind::buffer_key(*s, false), n))
            .chain(p.ps_regs.iter().map(|(s, n)| (bind::buffer_key(*s, true), n)))
        {
            let e = sizes.entry(s).or_insert(0);
            *e = (*e).max(*n);
        }
        for w in &binds.cbuffer {
            let rows = match &w.src {
                CbSource::Code { rows, .. } => u32::from(*rows),
                _ => 1,
            };
            let e = sizes.entry(w.buffer).or_insert(0);
            *e = (*e).max(u32::from(w.dest) + rows);
        }
        let is_material = |b: u32| b == bind::MATERIAL || b == bind::buffer_key(bind::MATERIAL, true);
        let writes = binds
            .cbuffer
            .iter()
            .filter(|w| !is_material(w.buffer))
            .cloned()
            .collect();
        let b0 = u64::from(sizes.get(&bind::PER_PRIM).copied().unwrap_or(1).max(1)) * 16;
        let stride = if std::env::var("H1_DBG_STRIDE256").is_ok() {
            b0.next_multiple_of(256)
        } else {
            b0.next_multiple_of(self.arena.align)
        };
        let mut frame = BTreeMap::new();
        for (&slot, &n) in &sizes {
            if slot != bind::PER_PRIM && !is_material(slot) {
                frame.insert(slot, self.arena.alloc(&self.gpu, u64::from(n.max(1)) * 16));
            }
        }
        self.plans.push(CbPlan {
            writes,
            sizes,
            stride,
            objects: Vec::new(),
            object_slot: HashMap::new(),
            per_object: None,
            mirror: Vec::new(),
            frame,
        });
        self.plan_of
            .insert((pipe.clone(), variant), self.plans.len() - 1);
        Ok(self.plans.len() - 1)
    }

    /// The surfaces drawn with `slots` (and `prepass_slots` first),
    /// prepared once: pipelines, constant buffers and bind groups made.
    fn prepare(
        &mut self,
        slots: &[usize],
        prepass_slots: &[usize],
        target: Target,
        variant: u8,
    ) -> Result<PrepKey, String> {
        let pk: PrepKey = (slots.to_vec(), prepass_slots.to_vec(), target, variant);
        if self.prepared.contains_key(&pk) {
            return Ok(pk);
        }
        let draws = self.scene.draws.clone();
        let opaque: Vec<bool> = draws
            .iter()
            .map(|d| {
                let mat = &self.scene.materials[d.material];
                pick(&self.scene.pool, &mat.techset, slots)
                    .is_some_and(|(slot, _)| mat.state(slot, 0).blend_state().is_none())
            })
            .collect();
        // First every surface's passes, so each plan knows how many
        // objects it draws before its buffers are made.
        let mut items: Vec<Item> = Vec::new();
        // Which draws the pre-pass stage drew: an opaque draw it left out
        // writes depth in its own pass (`PassState::needs_own_depth`).
        let mut prepassed = vec![false; draws.len()];
        let mut own_depth = 0usize;
        // (The debug switches, read once, not per draw and pass.)
        let region_none_draws = std::env::var("BO2MP_H1_REGION_NONE").is_ok_and(|v| v == "draw");
        let dbg_blend_draw = std::env::var("H1_DBG_BLEND_DRAW").is_ok();
        let dbg_mat = std::env::var("H1_DBG_MAT").ok();
        let stages: [(&[usize], bool); 2] = [(prepass_slots, true), (slots, false)];
        for (stage_slots, prepass) in stages {
            for (di, (d, &is_opaque)) in draws.iter().zip(&opaque).enumerate() {
                if prepass && !is_opaque {
                    continue;
                }
                let mat = self.scene.materials[d.material].clone();
                // Materials in no camera region (the shadow-caster-only
                // stand-ins, `*_shadowcaster` sets) are drawn into shadow
                // maps only (INFERRED: region 12 is "none", the last value,
                // used only by those sets; `BO2MP_H1_REGION_NONE=draw` old).
                if target == Target::Scene
                    && (mat.camera_region == CAMERA_REGION_NONE || mat.camera_region == CAMERA_REGION_NONE + 1)
                    && !region_none_draws
                {
                    continue;
                }
                // A blended surface whose pictures aren't loaded is left
                // out: drawn with stand-ins it shows what isn't there.
                if !is_opaque && !prepass {
                    let missing = mat.textures.iter().any(|t| {
                        !t.3.is_empty()
                            && fastfile_h1::shared::builtin_pixels(&key(&t.3)).is_none()
                            && self.image_tex(&t.3).is_none()
                    });
                    if missing && !dbg_blend_draw {
                        *self
                            .report
                            .skipped
                            .entry("blended surface with pictures not loaded".into())
                            .or_default() += 1;
                        continue;
                    }
                }
                let kind = self.scene.meshes[d.mesh].kind;
                let Some((tslot, tech)) = pick(&self.scene.pool, &mat.techset, stage_slots) else {
                    if !prepass && target == Target::Scene {
                        *self
                            .report
                            .skipped
                            .entry(format!("no technique in {}", mat.techset))
                            .or_default() += 1;
                    }
                    continue;
                };
                let tech = tech.clone();
                let wvf = match (kind, d.layer) {
                    (VertexKind::World, Some(_)) => self
                        .scene
                        .pool
                        .world_vert_format
                        .get(&key(&mat.techset))
                        .copied()
                        .unwrap_or(0),
                    (VertexKind::Model, Some(_)) => VERTEX_LIT_WVF,
                    _ => 0,
                };
                for pi in 0..tech.pass_info.len() {
                    let mut state = mat.state(tslot, pi);
                    if pi == 0
                        && !prepass
                        && target == Target::Scene
                        && crate::state::own_depth_on()
                        && state.needs_own_depth(is_opaque, prepassed[di])
                    {
                        state = state.with_own_depth();
                        own_depth += 1;
                    }
                    if let Some(w) = dbg_mat.as_deref()
                        && mat.name.contains(w)
                        && di % 50 == 0
                    {
                        eprintln!(
                            "dbg mat {} target {target:?} prepass {prepass} slot {tslot} tech {} pass {pi}: {state:?} blend {:?}",
                            mat.name,
                            tech.name,
                            state.blend_state()
                        );
                    }
                    let pipe = self.pipeline(&tech, pi, kind, state, wvf, target);
                    let pipe_for_occ = pipe.clone();
                    if let Some(w) = dbg_mat.as_deref()
                        && mat.name.contains(w)
                        && di % 50 == 0
                    {
                        eprintln!(
                            "dbg mat {} pipeline made: {} (kind {kind:?}, wvf {wvf})",
                            mat.name,
                            self.pipelines.get(&pipe).is_some_and(Option::is_some)
                        );
                    }
                    if self.pipelines.get(&pipe).is_none_or(Option::is_none) {
                        continue;
                    }
                    let binds = bind::resolve(
                        &tech.pass_info[pi],
                        &MaterialView {
                            textures: &mat.textures,
                            constants: &mat.constants,
                        },
                    );
                    let plan = self.plan(&pipe, variant, &binds)?;
                    let p = &mut self.plans[plan];
                    if !p.object_slot.contains_key(&d.object) {
                        p.object_slot.insert(d.object, p.objects.len() as u32);
                        p.objects.push(d.object);
                    }
                    if prepass {
                        prepassed[di] = true;
                    }
                    if target == Target::Scene && !prepass && variant == 0 && d.mesh == 1 {
                        if is_opaque {
                            self.report.props.opaque += 1;
                        } else {
                            let e = self
                                .report
                                .props
                                .blended
                                .entry(mat.name.clone())
                                .or_insert((0, state.depth_write, format!("{:?}", state.blend)));
                            e.0 += 1;
                        }
                    }
                    items.push(Item {
                        pipe,
                        plan,
                        binds,
                        draw: *d,
                        blended: !is_opaque,
                        bounds: self.bounds.get(di).copied().flatten(),
                        di,
                        front: match (state.cull, kind) {
                            (2, VertexKind::World) | (3, VertexKind::Model) => 1,
                            (3, VertexKind::World) | (2, VertexKind::Model) => -1,
                            _ => 0,
                        },
                        occluder: !prepass
                            && is_opaque
                            && state.write_mask != 0
                            && matches!(state.depth_test, 1..=3)
                            && self
                                .pipelines
                                .get(&pipe_for_occ)
                                .and_then(Option::as_ref)
                                .is_some_and(|p| !p.discards),
                    });
                }
            }
        }
        // Each plan's per-object buffer, sized for every object it draws.
        let mut used: Vec<usize> = items.iter().map(|i| i.plan).collect();
        used.sort_unstable();
        used.dedup();
        for &pi in &used {
            let p = &mut self.plans[pi];
            // (Instancing binds up to `MAX_INSTANCES` blocks from any
            // object's: padded so that never runs past the end.)
            let pad = if crate::instancing::on() { u64::from(crate::instancing::MAX_INSTANCES) * p.stride } else { 0 };
            let need = (p.objects.len() as u64).max(1) * p.stride;
            if p.per_object.as_ref().is_none_or(|b| b.size() < need + pad) {
                // Groups made with a smaller buffer are remade.
                if p.per_object.is_some() {
                    self.groups
                        .retain(|k, _| self.plan_of.get(&(k.0.clone(), k.1)) != Some(&pi));
                    self.prepared.clear();
                }
                p.per_object = Some(self.gpu.device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("h1_objects"),
                    size: need + pad,
                    usage: wgpu::BufferUsages::UNIFORM
                        | wgpu::BufferUsages::COPY_DST
                        | if pad > 0 { wgpu::BufferUsages::STORAGE } else { wgpu::BufferUsages::empty() },
                    mapped_at_creation: false,
                }));
                p.mirror = vec![0; need as usize];
            }
        }
        if target == Target::Scene && variant == 0 && self.occluders.is_none() {
            let found = self.find_occluders(&items);
            self.occluders = Some(found);
        }
        let (mut opaque_ready, mut blended_ready) = (Vec::new(), Vec::new());
        for it in items {
            let d = &it.draw;
            let gk: GroupKey = (it.pipe.clone(), variant, d.material, d.lightmap, d.probe);
            if !self.groups.contains_key(&gk) {
                let g = self.make_groups(&it.pipe, it.plan, &it.binds, d)?;
                self.groups.insert(gk.clone(), g);
            }
            let p = &self.plans[it.plan];
            let offset = p.object_slot[&d.object] * p.stride as u32;
            let pl = self.pipelines[&it.pipe].as_ref().ok_or("no pipeline")?;
            let dynamic = pl.layout.dynamic_count;
            // A layered surface: both streams bound at its first vertex.
            let layer = match (pl.layered, d.layer) {
                (true, Some(l)) => Some((
                    u64::from(d.base_vertex as u32) * self.scene.meshes[d.mesh].kind.stride(),
                    u64::from(l),
                )),
                _ => None,
            };
            let (groups, inst_group) = self.groups[&gk].clone();
            let inst = match (&pl.inst, inst_group) {
                (Some((pipeline, _, stride)), Some(group)) if *stride == p.stride && dynamic == 1 => Some(Inst {
                    pipeline: pipeline.clone(),
                    group,
                    stride: p.stride as u32,
                }),
                _ => None,
            };
            let r = Ready {
                inst,
                layer,
                pipeline: self.pipelines[&it.pipe]
                    .as_ref()
                    .ok_or("no pipeline")?
                    .pipeline
                    .clone(),
                groups,
                offsets: vec![offset; dynamic as usize],
                mesh: d.mesh,
                first: d.first_index,
                count: d.index_count,
                base: d.base_vertex,
                bounds: it.bounds,
                lod: d.lod,
            };
            if it.blended {
                blended_ready.push(r)
            } else {
                opaque_ready.push(r)
            }
        }
        if target == Target::Scene {
            self.report.draws = opaque_ready.len() + blended_ready.len();
            self.report.own_depth = own_depth;
        }
        // Neighbouring draws with the same state and a contiguous index
        // range become one (`BO2MP_H1_MERGE_DRAWS=0`: off).
        if !std::env::var("BO2MP_H1_MERGE_DRAWS").is_ok_and(|v| v == "0") {
            let (a, b) = (opaque_ready.len(), blended_ready.len());
            opaque_ready = merge_ready(opaque_ready);
            blended_ready = merge_ready(blended_ready);
            if std::env::var("H1_PROFILE").is_ok() {
                eprintln!(
                    "merged draws {target:?}: opaque {a} -> {}, blended {b} -> {}",
                    opaque_ready.len(),
                    blended_ready.len()
                );
            }
        }
        self.report.techniques = self.pipelines.values().filter(|p| p.is_some()).count();
        self.prepared.insert(
            pk.clone(),
            Prepared {
                opaque: opaque_ready,
                blended: blended_ready,
                plans: used,
            },
        );
        Ok(pk)
    }

    fn make_groups(
        &mut self,
        pk: &PipeKey,
        plan: usize,
        binds: &PassBinds,
        d: &Draw,
    ) -> Result<([wgpu::BindGroup; 3], Option<wgpu::BindGroup>), String> {
        let (cb_entries, tex_entries, samp_entries) = {
            let p = self.pipelines[pk].as_ref().ok_or("no pipeline")?;
            (
                p.layout.cbuffers.clone(),
                p.layout.textures.clone(),
                p.layout.samplers.clone(),
            )
        };
        enum Cb {
            Arena(Slice),
            Objects(u64),
        }
        let mut cb_bind = Vec::new();
        let ps_material = bind::buffer_key(bind::MATERIAL, true);
        for &(binding, slot, _regs) in &cb_entries {
            // (Pixel-stage buffers are bound from 32 up.)
            let slot = bind::buffer_key(slot, binding >= 32);
            let at = match slot {
                bind::PER_PRIM => Cb::Objects(
                    self.plans[plan]
                        .sizes
                        .get(&slot)
                        .copied()
                        .unwrap_or(1)
                        .max(1) as u64
                        * 16,
                ),
                s if s == bind::MATERIAL || s == ps_material => {
                    let n = self.plans[plan]
                        .sizes
                        .get(&slot)
                        .copied()
                        .unwrap_or(1)
                        .max(1);
                    let k = (plan, d.material, slot);
                    let at = match self.material_cb.get(&k) {
                        Some(&at) => at,
                        None => {
                            // The material's constants never change: written now.
                            let at = self.arena.alloc(&self.gpu, u64::from(n) * 16);
                            let empty = CodeValues::default();
                            let cx = FillCtx {
                                code: &empty,
                                lookup: self.lighting_lookup,
                                wvp: self.wvp_index,
                                lookup_i: self.lookup_index,
                            };
                            let c = fill(&binds.cbuffer, slot, n, None, &cx);
                            self.arena.write(at, &c);
                            self.material_cb.insert(k, at);
                            at
                        }
                    };
                    Cb::Arena(at)
                }
                _ => match self.plans[plan].frame.get(&slot) {
                    Some(&at) => Cb::Arena(at),
                    None => {
                        if std::env::var("H1_DBG_GROUPS").is_ok() {
                            eprintln!(
                                "dbg group {}: no frame buffer for b{slot} ({_regs} registers): 16-byte stand-in",
                                self.scene.materials[d.material].name
                            );
                        }
                        let at = self.arena.alloc(&self.gpu, 16);
                        self.plans[plan].frame.insert(slot, at);
                        Cb::Arena(at)
                    }
                },
            };
            cb_bind.push((binding, at));
        }
        let mut tex_views: Vec<(u32, wgpu::TextureView)> = Vec::new();
        for &(binding, slot, dim, kind, is_ps) in &tex_entries {
            let src = if is_ps {
                binds.ps.textures.get(&slot)
            } else {
                binds.vs.textures.get(&slot)
            };
            let vdim = match dim {
                dxbc_sm5::TextureDim::Cube => wgpu::TextureViewDimension::Cube,
                dxbc_sm5::TextureDim::D3 => wgpu::TextureViewDimension::D3,
                dxbc_sm5::TextureDim::D2Array => wgpu::TextureViewDimension::D2Array,
                _ => wgpu::TextureViewDimension::D2,
            };
            let mut view = None;
            // H1_DBG_TEXDEF=1: placeholders everywhere; =<binding>: only there.
            let texdef = std::env::var("H1_DBG_TEXDEF")
                .is_ok_and(|v| v == "1" || v == binding.to_string());
            // A code texture fits the binding when its view dimension does
            // and it is a depth texture exactly when the shader samples
            // depth (`SHADOWMAP_SUN`).
            if !texdef
                && let Some(TexSource::Code(c)) = src
                && let Some(t) = self
                    .code_textures
                    .get(fastfile_h1::code_source::code_texture_name(*c))
                && t.dim == vdim
                && (kind == TextureKind::Depth) == t.texture.format().has_depth_aspect()
            {
                view = Some(t.view.clone());
            }
            // H1_DBG_LM=white: the lightmaps drawn as white (debug).
            let lm_mode = std::env::var("H1_DBG_LM").unwrap_or_default();
            let lm_white = match src {
                Some(TexSource::LightmapPrimary) => lm_mode == "white" || lm_mode == "primary",
                Some(TexSource::LightmapSecondary) => lm_mode == "white" || lm_mode == "secondary",
                Some(TexSource::ReflectionProbe) => lm_mode == "probe",
                _ => false,
            };
            let name: Option<String> = match src {
                _ if lm_white => None,
                Some(TexSource::Material { image, .. }) => image.clone(),
                Some(TexSource::LightmapPrimary) => self
                    .scene
                    .lightmaps
                    .get(d.lightmap as usize)
                    .and_then(|l| l.0.clone()),
                Some(TexSource::LightmapSecondary) => self
                    .scene
                    .lightmaps
                    .get(d.lightmap as usize)
                    .and_then(|l| l.1.clone()),
                Some(TexSource::ReflectionProbe) => {
                    self.scene.probes.get(d.probe as usize).cloned().flatten()
                }
                _ => None,
            };
            if view.is_none()
                && !texdef
                && kind != TextureKind::Depth
                && let Some(n) = name
                && let Some(t) = self.image_tex(&n)
                && t.dim == vdim
            {
                view = Some(t.view.clone());
            }
            let view = match view {
                Some(v) => v,
                None => {
                    let what = match src {
                        _ if lm_white => "white",
                        Some(TexSource::Material { semantic: 5, .. }) => "normal",
                        Some(TexSource::Code(c))
                            if matches!(
                                fastfile_h1::code_source::code_texture_name(*c),
                                "TRANS_SHADOWMAP_SUN" | "WHITE"
                            ) =>
                        {
                            "white"
                        }
                        Some(TexSource::Material { .. }) => {
                            if d.mesh == 1 {
                                *self
                                    .report
                                    .props
                                    .defaulted
                                    .entry(self.scene.materials[d.material].name.clone())
                                    .or_default() += 1;
                            }
                            // A picture that is not there must not make a
                            // whole prop vanish: an opaque pass (whose
                            // alpha-tested shader would discard alpha 0)
                            // takes solid grey; a blended pass keeps alpha
                            // 0, so it draws nothing (`BO2MP_H1_GREY_ALPHA0=1`:
                            // alpha 0 for both, as before).
                            if pk.3.blend_state().is_none() && !grey_alpha0() {
                                "grey_opaque"
                            } else {
                                "grey"
                            }
                        }
                        _ => "black",
                    };
                    self.default_tex(what, vdim, kind).view.clone()
                }
            };
            if let Ok(w) = std::env::var("H1_DBG_GROUPS")
                && self.scene.materials[d.material].name.contains(w.as_str())
            {
                eprintln!(
                    "dbg group {} binding {binding} slot {slot} dim {vdim:?} kind {kind:?} src {src:?}",
                    self.scene.materials[d.material].name
                );
            }
            tex_views.push((binding, view));
        }
        let mut samps: Vec<(u32, wgpu::Sampler)> = Vec::new();
        for &(binding, slot, is_ps) in &samp_entries {
            let src = if is_ps {
                binds.ps.samplers.get(&slot)
            } else {
                binds.vs.samplers.get(&slot)
            };
            let s = src.copied().unwrap_or(SampSource::Default);
            let comparison = self.pipelines[pk]
                .as_ref()
                .is_some_and(|p| p.layout.comparison_bindings.contains(&binding));
            samps.push((binding, self.sampler(s, comparison)));
        }
        let p = self.pipelines[pk].as_ref().ok_or("no pipeline")?;
        let objects = self.plans[plan]
            .per_object
            .as_ref()
            .ok_or("no object buffer")?;
        let dev = &self.gpu.device;
        let g0 = dev.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("cb"),
            layout: &p.layout.groups[0],
            entries: &cb_bind
                .iter()
                .map(|(b, at)| wgpu::BindGroupEntry {
                    binding: *b,
                    resource: match at {
                        Cb::Arena(at) => self.arena.binding(*at),
                        Cb::Objects(size) => wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                            buffer: objects,
                            offset: 0,
                            size: std::num::NonZeroU64::new(*size),
                        }),
                    },
                })
                .collect::<Vec<_>>(),
        });
        // Its instanced form: the per-object blocks as a storage array of up
        // to `MAX_INSTANCES` blocks from the bound offset.
        let ig = p.inst.as_ref().map(|(_, layout, stride)| {
            dev.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("cb_inst"),
                layout: &layout.groups[0],
                entries: &cb_bind
                    .iter()
                    .map(|(b, at)| wgpu::BindGroupEntry {
                        binding: *b,
                        resource: match at {
                            Cb::Arena(at) => self.arena.binding(*at),
                            Cb::Objects(_) => wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                                buffer: objects,
                                offset: 0,
                                size: std::num::NonZeroU64::new(stride * u64::from(crate::instancing::MAX_INSTANCES)),
                            }),
                        },
                    })
                    .collect::<Vec<_>>(),
            })
        });
        let g1 = dev.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("tex"),
            layout: &p.layout.groups[1],
            entries: &tex_views
                .iter()
                .map(|(b, v)| wgpu::BindGroupEntry {
                    binding: *b,
                    resource: wgpu::BindingResource::TextureView(v),
                })
                .collect::<Vec<_>>(),
        });
        let g2 = dev.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("samp"),
            layout: &p.layout.groups[2],
            entries: &samps
                .iter()
                .map(|(b, s)| wgpu::BindGroupEntry {
                    binding: *b,
                    resource: wgpu::BindingResource::Sampler(s),
                })
                .collect::<Vec<_>>(),
        });
        Ok(([g0, g1, g2], ig))
    }

    /// Rewrites the frame's and the objects' constants for `code`.
    fn update(&mut self, key: &PrepKey, code: &CodeValues) {
        self.uploaded = 0;
        let cx = FillCtx {
            code,
            lookup: self.lighting_lookup,
            wvp: self.wvp_index,
            lookup_i: self.lookup_index,
        };
        let dbg_cb = std::env::var("H1_DBG_CB").ok();
        let names: HashMap<usize, String> = if dbg_cb.is_some() {
            self.plan_of
                .iter()
                .map(|((pk, _), &i)| (i, pk.0.clone()))
                .collect()
        } else {
            HashMap::new()
        };
        for &pi in &self.prepared[key].plans {
            let plan = &mut self.plans[pi];
            if let Some(w) = &dbg_cb
                && names.get(&pi).is_some_and(|n| n.contains(w.as_str()))
            {
                eprintln!("dbg cb plan {pi} {}: sizes {:?}", names[&pi], plan.sizes);
                for w in &plan.writes {
                    eprintln!("  write b{}[{}] <- {:?}", w.buffer, w.dest, w.src);
                }
                for (&slot, _) in &plan.frame {
                    let c = fill(&plan.writes, slot, plan.sizes[&slot], None, &cx);
                    for (r, v) in c.iter().enumerate().filter(|(_, v)| **v != [0.0; 4]) {
                        eprintln!("  b{slot}[{r}] = {v:?}");
                    }
                }
                let c = fill(&plan.writes, bind::PER_PRIM, plan.sizes.get(&bind::PER_PRIM).copied().unwrap_or(1), None, &cx);
                for (r, v) in c.iter().enumerate() {
                    eprintln!("  b0[{r}] = {v:?}");
                }
            }
            for (&slot, &at) in &plan.frame {
                let c = fill(&plan.writes, slot, plan.sizes[&slot], None, &cx);
                self.arena.write(at, &c);
            }
            if let (Some(&n), Some(buf)) =
                (plan.sizes.get(&bind::PER_PRIM), plan.per_object.as_ref())
            {
                let (mut lo, mut hi) = (usize::MAX, 0usize);
                for (i, o) in plan.objects.iter().enumerate() {
                    let obj = o.and_then(|o| self.scene.objects.get(o as usize));
                    fill_into(&plan.writes, bind::PER_PRIM, n, obj, &cx, &mut self.fill_scratch);
                    let bytes: &[u8] = bytemuck::cast_slice(&self.fill_scratch);
                    let at = i * plan.stride as usize;
                    let len = bytes.len().min(plan.stride as usize);
                    // Only what changed goes to the buffer.
                    if plan.mirror[at..at + len] != bytes[..len] {
                        plan.mirror[at..at + len].copy_from_slice(&bytes[..len]);
                        lo = lo.min(at);
                        hi = hi.max(at + len);
                    }
                }
                if lo < hi {
                    // (Queue writes are 4-byte aligned; rows are 16.)
                    self.gpu.queue.write_buffer(buf, lo as u64, &plan.mirror[lo..hi]);
                    self.uploaded += (hi - lo) as u64;
                }
            }
        }
        self.arena.flush(&self.gpu.queue);
    }

    /// Records `frame`'s part of the scene into `encoder`, onto `targets`.
    /// The constants are written through the queue, so the encoder is to
    /// be submitted before the next `record` with other code values.
    /// Per-object constants held: (plans, objects summed, CPU mirror bytes).
    pub fn constants_memory(&self) -> (usize, usize, usize) {
        (
            self.plans.len(),
            self.plans.iter().map(|p| p.objects.len()).sum(),
            self.plans.iter().map(|p| p.mirror.len()).sum(),
        )
    }

    pub fn record(
        &mut self,
        encoder: &mut wgpu::CommandEncoder,
        targets: &Targets,
        frame: &Frame,
    ) -> Result<(), String> {
        let key = self.prepare(
            frame.slots,
            frame.prepass_slots,
            frame.target,
            frame.variant,
        )?;
        let t = std::time::Instant::now();
        self.update(&key, frame.code);
        let profile = std::env::var("H1_PROFILE").is_ok();
        if profile {
            eprintln!("update {:.2} ms", t.elapsed().as_secs_f32() * 1e3);
        }
        let t = std::time::Instant::now();
        let mut drawn = 0;
        let mut culled = 0;
        let (mut small, mut hidden) = (0usize, 0usize);
        let stats: (u32, u32, u32, u64, u32);
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("h1_world"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: targets.colour,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: match targets.clear {
                            Some(c) => wgpu::LoadOp::Clear(wgpu::Color {
                                r: c[0],
                                g: c[1],
                                b: c[2],
                                a: c[3],
                            }),
                            None => wgpu::LoadOp::Load,
                        },
                        store: wgpu::StoreOp::Store,
                    },
                    depth_slice: None,
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: targets.depth,
                    depth_ops: Some(wgpu::Operations {
                        load: if targets.clear.is_some() {
                            wgpu::LoadOp::Clear(targets.depth_clear)
                        } else {
                            wgpu::LoadOp::Load
                        },
                        store: wgpu::StoreOp::Store,
                    }),
                    stencil_ops: (frame.target == Target::Scene
                        && self.depth_format.has_stencil_aspect())
                    .then_some(wgpu::Operations {
                        load: wgpu::LoadOp::Load,
                        store: wgpu::StoreOp::Store,
                    }),
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            let prepared = &self.prepared[&key];
            let lists: &[&Vec<Ready>] = match frame.part {
                Part::All => &[&prepared.opaque, &prepared.blended],
                Part::Opaque => &[&prepared.opaque],
                Part::Blended => &[&prepared.blended],
            };
            if let Some([x, y, w, h, near, far]) = targets.viewport {
                pass.set_viewport(x, y, w, h, near, far);
            }
            let mut bound_mesh = usize::MAX;
            let dbg_nolayer = std::env::var("H1_DBG_NOLAYER").is_ok();
            let dbg_draws = std::env::var("H1_DBG_DRAWS").is_ok();
            let mut last_pipeline: Option<&wgpu::RenderPipeline> = None;
            let mut last_g0: Option<(&wgpu::BindGroup, &[u32])> = None;
            let mut last_g1: Option<&wgpu::BindGroup> = None;
            let mut last_g2: Option<&wgpu::BindGroup> = None;
            let (mut n_pipe, mut n_bg, n_vb, mut n_tris) = (0u32, 0u32, 0u32, 0u64);
            let mut n_inst = 0u32;
            // The draws this view keeps (culled by level of detail, bounds
            // and distance), in list order.
            let mut seen: Vec<&Ready> = Vec::new();
            for r in lists.iter().flat_map(|l| l.iter()) {
                if let Some(l) = r.lod {
                    let keep = match frame.lod {
                        None => l.level == 0,
                        Some((eye, k)) => l.selects((glam::Vec3::from(l.center) - eye).length(), k),
                    };
                    if !keep {
                        culled += 1;
                        continue;
                    }
                }
                if frame.need_bounds && frame.cull.is_some() && r.bounds.is_none() {
                    culled += 1;
                    continue;
                }
                if let (Some(planes), Some((lo, hi))) = (frame.cull, r.bounds)
                    && !crate::sun_shadow::box_inside(planes, lo, hi)
                {
                    culled += 1;
                    continue;
                }
                if let (Some((eye, far)), Some((lo, hi))) = (frame.max_distance, r.bounds)
                    && crate::screenfx::box_distance(eye, lo, hi) > far
                {
                    culled += 1;
                    continue;
                }
                if let (Some((eye, angle)), Some((lo, hi))) = (frame.small_angle, r.bounds)
                    && native_cull::too_small(eye, lo, hi, angle)
                {
                    culled += 1;
                    small += 1;
                    continue;
                }
                if let (Some(o), Some((lo, hi))) = (frame.occlusion, r.bounds)
                    && o.box_hidden(lo, hi)
                {
                    culled += 1;
                    hidden += 1;
                    continue;
                }
                seen.push(r);
            }
            let instancing = crate::instancing::on();
            let mut si = 0;
            while si < seen.len() {
                let r = seen[si];
                si += 1;
                // The next draws that repeat this one but for their object
                // block, each the block after the last: one instanced call.
                let mut n = 1u32;
                if instancing
                    && r.layer.is_none()
                    && let Some(inst) = &r.inst
                {
                    while si < seen.len() && n < crate::instancing::MAX_INSTANCES {
                        let q = seen[si];
                        let follows = q.layer.is_none()
                            && q.pipeline == r.pipeline
                            && q.groups == r.groups
                            && q.mesh == r.mesh
                            && q.first == r.first
                            && q.count == r.count
                            && q.base == r.base
                            && q.lod.map(|l| l.level) == r.lod.map(|l| l.level)
                            && q.offsets.len() == 1
                            && r.offsets.len() == 1
                            && q.offsets[0] == r.offsets[0] + n * inst.stride;
                        if !follows {
                            break;
                        }
                        n += 1;
                        si += 1;
                    }
                }
                let mb = &self.mesh_buffers[r.mesh];
                if r.mesh != bound_mesh {
                    pass.set_index_buffer(mb.1.slice(..), wgpu::IndexFormat::Uint16);
                }
                let base = match (r.layer, &mb.2) {
                    (Some((_, layer)), Some(lb)) if dbg_nolayer => {
                        pass.set_vertex_buffer(0, mb.0.slice(..));
                        pass.set_vertex_buffer(1, lb.slice(layer..));
                        bound_mesh = usize::MAX - 1;
                        r.base
                    }
                    (Some((main, layer)), Some(lb)) => {
                        pass.set_vertex_buffer(0, mb.0.slice(main..));
                        pass.set_vertex_buffer(1, lb.slice(layer..));
                        // The next unlayered draw rebinds stream 0 whole.
                        bound_mesh = usize::MAX - 1;
                        0
                    }
                    _ => {
                        if r.mesh != bound_mesh {
                            pass.set_vertex_buffer(0, mb.0.slice(..));
                            bound_mesh = r.mesh;
                        }
                        r.base
                    }
                };
                if dbg_draws && drawn < 12 {
                    eprintln!(
                        "dbg draw {drawn}: mesh {} first {} count {} base {base} layer {:?} offsets {:?} bound_mesh {bound_mesh}",
                        r.mesh, r.first, r.count, r.layer, r.offsets
                    );
                }
                // Only what changed is set again (each call costs wgpu's
                // validation at submit).
                let instanced = if n > 1 { r.inst.as_ref() } else { None };
                let pipeline = instanced.map_or(&r.pipeline, |i| &i.pipeline);
                if last_pipeline != Some(pipeline) {
                    pass.set_pipeline(pipeline);
                    n_pipe += 1;
                    last_pipeline = Some(pipeline);
                }
                if let Some(i) = instanced {
                    pass.set_bind_group(0, &i.group, &r.offsets);
                    n_bg += 1;
                    last_g0 = None;
                } else if last_g0 != Some((&r.groups[0], &r.offsets[..])) {
                    pass.set_bind_group(0, &r.groups[0], &r.offsets);
                    n_bg += 1;
                    last_g0 = Some((&r.groups[0], &r.offsets[..]));
                }
                if last_g1 != Some(&r.groups[1]) {
                    pass.set_bind_group(1, &r.groups[1], &[]);
                    n_bg += 1;
                    last_g1 = Some(&r.groups[1]);
                }
                if last_g2 != Some(&r.groups[2]) {
                    pass.set_bind_group(2, &r.groups[2], &[]);
                    n_bg += 1;
                    last_g2 = Some(&r.groups[2]);
                }
                pass.draw_indexed(r.first..r.first + r.count, base, 0..n);
                drawn += n as usize;
                n_inst += n - 1;
                n_tris += u64::from(r.count / 3) * u64::from(n);
            }
            stats = (n_pipe, n_bg, n_vb, n_tris, n_inst);
        }
        if profile {
            eprintln!(
                "record {:.2} ms, {drawn} draws ({} folded into instanced calls), {culled} culled ({hidden} behind walls, {small} too small), {} pipeline sets, {} bind-group sets, {} triangles ({:?})",
                t.elapsed().as_secs_f32() * 1e3,
                stats.4,
                stats.0,
                stats.1,
                stats.3,
                frame.target
            );
        }
        {
            let c = &mut self.report.frame;
            let add = |v: &mut Option<u64>, n: u64| *v = Some(v.unwrap_or(0) + n);
            if matches!(frame.target, Target::Shadow | Target::TransShadow) {
                add(&mut c.shadow_draws, drawn as u64);
            } else {
                add(&mut c.draws, drawn as u64);
                add(&mut c.tris, stats.3);
            }
        }
        if frame.target == Target::Shadow {
            self.report.shadow_drawn = drawn;
            self.report.shadow_culled = culled;
            return Ok(());
        }
        if frame.target == Target::TransShadow {
            return Ok(());
        }
        self.report.drawn = drawn;
        self.report.missing_code = frame
            .code
            .missing
            .borrow()
            .iter()
            .map(|&i| fastfile_h1::code_source::code_const_name(i).to_string())
            .collect();
        Ok(())
    }

    /// A colour (Rgba16Float) and depth target for offline rendering.
    pub fn targets(&self, w: u32, h: u32) -> (wgpu::Texture, wgpu::Texture) {
        let make = |label: &str, format: wgpu::TextureFormat| {
            self.gpu.device.create_texture(&wgpu::TextureDescriptor {
                label: Some(label),
                size: wgpu::Extent3d {
                    width: w,
                    height: h,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                    | wgpu::TextureUsages::TEXTURE_BINDING
                    | wgpu::TextureUsages::COPY_SRC,
                view_formats: &[],
            })
        };
        (
            make("colour", wgpu::TextureFormat::Rgba16Float),
            make("depth", self.depth_format),
        )
    }

    /// Offline: `frame` drawn onto `colour` and `depth`, cleared first if
    /// `clear`, and submitted.
    pub fn render_onto(
        &mut self,
        colour: &wgpu::Texture,
        depth: &wgpu::Texture,
        frame: &Frame,
        clear: Option<[f64; 4]>,
    ) -> Result<(), String> {
        let cv = colour.create_view(&Default::default());
        let dv = depth.create_view(&Default::default());
        let mut enc = self.gpu.device.create_command_encoder(&Default::default());
        self.record(
            &mut enc,
            &Targets {
                colour: &cv,
                depth: &dv,
                clear,
                depth_clear: 0.0,
                viewport: None,
            },
            frame,
        )?;
        self.gpu.queue.submit([enc.finish()]);
        Ok(())
    }

    fn make_pipeline(
        &self,
        pass: &PassRec,
        kind: VertexKind,
        state: PassState,
        wvf: u8,
        target: Target,
    ) -> Result<Pipeline, String> {
        let shadow = target == Target::Shadow;
        let trans = target == Target::TransShadow;
        let vs_name = pass.vertex_shader.as_deref().ok_or("no vertex shader")?;
        let ps_name = pass.pixel_shader.as_deref().ok_or("no pixel shader")?;
        let vs_b = self
            .scene
            .pool
            .program(AssetType::VertexShader, vs_name)
            .ok_or_else(|| format!("vertex shader {vs_name} not loaded"))?;
        let vs_c = dxbc_sm5::Container::parse(vs_b).map_err(|e| format!("{e:?}"))?;
        let vs_p = dxbc_sm5::decode(vs_c.shader).map_err(|e| format!("{e:?}"))?;
        // `null.hlsl`: no pixel shader (depth only). Its stand-in writes
        // nothing (the target's write mask is empty).
        let null_ps = key(ps_name).starts_with("null");
        let ps = if null_ps {
            dxbc_sm5::Translation {
                stage: Stage::Pixel,
                wgsl: "@fragment fn main() -> @location(0) vec4<f32> { return vec4<f32>(0.0); }\n"
                    .into(),
                reflection: dxbc_sm5::Reflection::default(),
            }
        } else {
            let ps_b = self
                .scene
                .pool
                .program(AssetType::PixelShader, ps_name)
                .ok_or_else(|| format!("pixel shader {ps_name} not loaded"))?;
            let ps_c = dxbc_sm5::Container::parse(ps_b).map_err(|e| format!("{e:?}"))?;
            let ps_p = dxbc_sm5::decode(ps_c.shader).map_err(|e| format!("{e:?}"))?;
            let mut po = Options::for_stage(Stage::Pixel);
            po.varyings = dxbc_sm5::link(&vs_c.outputs, &ps_c.inputs);
            let mut t = dxbc_sm5::translate_program(&ps_p, &ps_c, &po)
                .map_err(|e| format!("ps {ps_name}: {e:?}"))?;
            // H1_DBG_WHITE=<words>: pixel programs whose name holds one of
            // the words write white (debug).
            if let Ok(w) = std::env::var("H1_DBG_WHITE")
                && w.split(',').any(|p| !p.is_empty() && ps_name.contains(p))
            {
                t.wgsl = t
                    .wgsl
                    .replace("output.sv_t0 = bitcast<vec4<f32>>(o0);", "output.sv_t0 = vec4<f32>(1.0);");
                if std::env::var("H1_DBG_PSWHITE").is_ok() {
                    t.wgsl = t.wgsl.replace("    body();\n", "\n");
                }
            }
            // Models are drawn with counter-clockwise fronts (their winding
            // is the world's reversed, so culling matches the game), which
            // inverts `is_front_face` against D3D's clockwise front: the
            // shaders' two-sided lighting flipped normals (magenta foliage).
            // `H1_DBG_FLIPFF=1` inverts it everywhere instead (debug).
            if (kind == VertexKind::Model) != std::env::var("H1_DBG_FLIPFF").is_ok() {
                t.wgsl = t.wgsl.replace(
                    "select(0u, 0xffffffffu, input.sv_ff)",
                    "select(0xffffffffu, 0u, input.sv_ff)",
                );
            }
            // H1_DBG_PSOUT=<word>|<wgsl vec4 expression>: matching pixel
            // programs write the expression instead (debug).
            if let Ok(v) = std::env::var("H1_DBG_PSOUT")
                && let Some((w, e)) = v.split_once('|')
                && ps_name.contains(w)
            {
                t.wgsl = t.wgsl.replace(
                    "output.sv_t0 = bitcast<vec4<f32>>(o0);",
                    &format!("output.sv_t0 = {e};"),
                );
            }
            t
        };
        let mut vo = Options::for_stage(Stage::Vertex);
        if !null_ps {
            let ps_b = self
                .scene
                .pool
                .program(AssetType::PixelShader, ps_name)
                .ok_or("ps")?;
            let ps_c = dxbc_sm5::Container::parse(ps_b).map_err(|e| format!("{e:?}"))?;
            vo.varyings = dxbc_sm5::link(&vs_c.outputs, &ps_c.inputs);
        }
        vo.output_interp = ps
            .reflection
            .inputs
            .iter()
            .map(|i| (i.semantic.to_ascii_uppercase(), i.index, i.interp))
            .collect();
        let mut vs = dxbc_sm5::translate_program(&vs_p, &vs_c, &vo)
            .map_err(|e| format!("vs {vs_name}: {e:?}"))?;
        if let Ok(dir) = std::env::var("H1_DBG_DUMP")
            && let Ok(w) = std::env::var("H1_DBG_DUMP_MATCH")
            && vs_name.contains(w.as_str())
        {
            let _ = std::fs::write(format!("{dir}/vs_{}.wgsl", key(vs_name)), &vs.wgsl);
            let _ = std::fs::write(format!("{dir}/ps_{}.wgsl", key(ps_name)), &ps.wgsl);
        }
        // H1_DBG_B0ID=<words>: matching vertex programs read an identity
        // world matrix from rows 0..3 of b0 instead of the buffer (debug).
        if let Ok(w) = std::env::var("H1_DBG_B0ID")
            && w.split(',').any(|p| !p.is_empty() && vs_name.contains(p))
        {
            for (r, v) in [
                "vec4<u32>(1065353216u, 0u, 0u, 0u)",
                "vec4<u32>(0u, 1065353216u, 0u, 0u)",
                "vec4<u32>(0u, 0u, 1065353216u, 0u)",
                "vec4<u32>(0u, 0u, 0u, 1065353216u)",
            ]
            .iter()
            .enumerate()
            {
                vs.wgsl = vs.wgsl.replace(&format!("cb0.d[{r}u]"), &format!("({v})"));
            }
        }
        // H1_DBG_VSVID=<words>: matching vertex programs place each vertex
        // by its index alone (debug: no vertex buffer read).
        if let Ok(w) = std::env::var("H1_DBG_VSVID")
            && w.split(',').any(|p| !p.is_empty() && vs_name.contains(p))
        {
            vs.wgsl = vs
                .wgsl
                .replacen("struct In {", "struct In {\n    @builtin(vertex_index) dbg_vid: u32,", 1)
                .replace(
                    "output.sv_pos = bitcast<vec4<f32>>(o0);",
                    "output.sv_pos = vec4<f32>(fract(f32(input.dbg_vid) * 0.1373) * 2.0 - 1.0, fract(f32(input.dbg_vid) * 0.0531) * 2.0 - 1.0, 0.5, 1.0);",
                );
        }
        // H1_DBG_VSPOS=<words>: matching vertex programs place each vertex
        // by its raw position alone (debug).
        if let Ok(w) = std::env::var("H1_DBG_VSPOS")
            && w.split(',').any(|p| !p.is_empty() && vs_name.contains(p))
        {
            vs.wgsl = vs.wgsl.replace(
                "output.sv_pos = bitcast<vec4<f32>>(o0);",
                "output.sv_pos = vec4<f32>(fract(input.l0.x / 512.0) * 2.0 - 1.0, fract(input.l0.y / 512.0) * 2.0 - 1.0, 0.5, 1.0);",
            );
        }
        let mut attrs = Vec::new();
        let mut layer_attrs = Vec::new();
        // A vertex-lit placement's lighting values (`_vlc` techniques read
        // them as TEXCOORD4..6): visibility (UNORM8x4, .x), ambient colour
        // (four halves, .w a direction term), highlight colour (likewise).
        let vlit = kind == VertexKind::Model
            && wvf == VERTEX_LIT_WVF
            && vs.reflection.inputs.iter().any(|i| {
                i.semantic.eq_ignore_ascii_case("TEXCOORD") && (i.index == 5 || i.index == 6)
            });
        let vlit_layout = crate::layers::Layout {
            components: vec![
                (10, 0, wgpu::VertexFormat::Unorm8x4),
                (11, 4, wgpu::VertexFormat::Float16x4),
                (12, 12, wgpu::VertexFormat::Float16x4),
            ],
            stride: 20,
        };
        let layout_l = if vlit {
            Some(&vlit_layout)
        } else {
            self.layer_layouts.get(&wvf).filter(|_| wvf != 0 && kind == VertexKind::World)
        };
        let decl = pass
            .vertex_decl
            .as_deref()
            .and_then(|d| self.scene.pool.decls.get(&key(d)))
            .map(Vec::as_slice);
        for i in &vs.reflection.inputs {
            let from_layer = if vlit {
                crate::layers::destination(&i.semantic, i.index)
                    .and_then(|d| vlit_layout.find(d))
            } else {
                layout_l.and_then(|l| crate::layers::layer_input(decl, l, &i.semantic, i.index))
            };
            if let Some((offset, format)) = from_layer {
                layer_attrs.push(wgpu::VertexAttribute {
                    format,
                    offset,
                    shader_location: i.location,
                });
                continue;
            }
            let (offset, format) = kind.attribute(&i.semantic, i.index).ok_or_else(|| {
                format!(
                    "vertex input {}{} not in a {kind:?} vertex (world vertex format {wvf})",
                    i.semantic, i.index
                )
            })?;
            attrs.push(wgpu::VertexAttribute {
                format,
                offset,
                shader_location: i.location,
            });
        }
        if let Ok(w) = std::env::var("H1_DBG_ATTR")
            && vs_name.contains(w.as_str())
        {
            eprintln!(
                "dbg attr {vs_name} kind {kind:?} wvf {wvf} stride {} layer stride {:?}\n  inputs {:?}\n  stream0 {attrs:?}\n  layer {layer_attrs:?}",
                kind.stride(),
                layout_l.map(|l| l.stride),
                vs.reflection
                    .inputs
                    .iter()
                    .map(|i| (i.semantic.clone(), i.index, i.location))
                    .collect::<Vec<_>>()
            );
        }
        let mut streams = vec![wgpu::VertexBufferLayout {
            array_stride: kind.stride(),
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &attrs,
        }];
        if let Some(l) = layout_l
            && !layer_attrs.is_empty()
        {
            streams.push(wgpu::VertexBufferLayout {
                array_stride: l.stride,
                step_mode: wgpu::VertexStepMode::Vertex,
                attributes: &layer_attrs,
            });
        }
        let dev = &self.gpu.device;
        let scope = dev.push_error_scope(wgpu::ErrorFilter::Validation);
        let vm = dev.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some(vs_name),
            source: wgpu::ShaderSource::Wgsl(vs.wgsl.clone().into()),
        });
        let pm = dev.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some(ps_name),
            source: wgpu::ShaderSource::Wgsl(ps.wgsl.clone().into()),
        });
        let layout = gpu::pass_layout(
            &self.gpu,
            &vs.reflection,
            &ps.reflection,
            Some(bind::PER_PRIM),
        );
        // One colour target (the HDR scene): a depth-only pass writes none
        // of it (its shader's output is a placeholder at location 0).
        let writes = if ps.reflection.targets.contains(&0) && !shadow {
            state.color_writes()
        } else {
            wgpu::ColorWrites::empty()
        };
        if let Ok(w) = std::env::var("H1_DBG_ATTR")
            && (vs_name.contains(w.as_str()) || ps_name.contains(w.as_str()))
        {
            eprintln!(
                "dbg targets {ps_name}: {:?} writes {writes:?} state {state:?} target {target:?}",
                ps.reflection.targets
            );
        }
        let targets = [Some(wgpu::ColorTargetState {
            format: match target {
                Target::Shadow => SHADOW_COLOUR,
                Target::TransShadow => TRANS_SHADOW_COLOUR,
                Target::Scene => wgpu::TextureFormat::Rgba16Float,
            },
            blend: if shadow { None } else { state.blend_state() },
            write_mask: writes,
        })];
        let make = |vm: &wgpu::ShaderModule, pipeline_layout: &wgpu::PipelineLayout| dev.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some(ps_name),
            layout: Some(pipeline_layout),
            vertex: wgpu::VertexState {
                module: vm,
                entry_point: Some("main"),
                compilation_options: Default::default(),
                buffers: &streams,
            },
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                // The world's triangles wind clockwise, models' the other way.
                front_face: match kind {
                    VertexKind::World => wgpu::FrontFace::Cw,
                    VertexKind::Model => wgpu::FrontFace::Ccw,
                },
                cull_mode: state.cull_mode(),
                ..Default::default()
            },
            depth_stencil: Some(if shadow || trans {
                wgpu::DepthStencilState {
                    format: SHADOW_DEPTH,
                    // The translucent casters keep the opaque casters' depth.
                    depth_write_enabled: Some(state.depth_write && !trans),
                    depth_compare: Some(state.depth_compare()),
                    stencil: Default::default(),
                    bias: state.shadow_bias(),
                }
            } else {
                wgpu::DepthStencilState {
                    format: self.depth_format,
                    depth_write_enabled: Some(state.depth_write),
                    depth_compare: Some(state.depth_compare()),
                    stencil: Default::default(),
                    bias: state.depth_bias(),
                }
            }),
            multisample: Default::default(),
            fragment: Some(wgpu::FragmentState {
                module: &pm,
                entry_point: Some("main"),
                compilation_options: Default::default(),
                targets: &targets,
            }),
            multiview_mask: None,
            cache: None,
        });
        let pipeline = make(&vm, &layout.pipeline_layout);
        if let Some(e) = pollster::block_on(scope.pop()) {
            let attrs: Vec<String> = vs
                .reflection
                .inputs
                .iter()
                .map(|i| format!("{}{}@{}", i.semantic, i.index, i.location))
                .collect();
            return Err(format!(
                "pipeline {ps_name} ({kind:?}, inputs {attrs:?}): {e}"
            ));
        }
        let regs =
            |r: &dxbc_sm5::Reflection| r.cbuffers.iter().map(|c| (c.slot, c.registers)).collect();
        let layered = streams.len() > 1;
        // The instanced form: a vertex program reading its per-object block
        // by instance, when the pixel program does not read that block.
        let limits = dev.limits();
        let storage_align = u64::from(limits.min_storage_buffer_offset_alignment);
        let inst = (crate::instancing::on()
            && limits.max_storage_buffers_per_shader_stage > 0
            && !layered
            && !ps.reflection.cbuffers.iter().any(|c| c.slot == 0))
        .then(|| vs.reflection.cbuffers.iter().find(|c| c.slot == 0))
        .flatten()
        .and_then(|c| {
            let stride = (u64::from(c.registers.max(1)) * 16).next_multiple_of(self.arena.align);
            if stride % storage_align != 0 {
                return None;
            }
            let wgsl = crate::instancing::patch_vertex(&vs.wgsl, 0, stride)?;
            let scope = dev.push_error_scope(wgpu::ErrorFilter::Validation);
            let vm = dev.create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some(vs_name),
                source: wgpu::ShaderSource::Wgsl(wgsl.into()),
            });
            let il = gpu::pass_layout_storage(&self.gpu, &vs.reflection, &ps.reflection, Some(bind::PER_PRIM), &[], Some(0));
            let p = make(&vm, &il.pipeline_layout);
            match pollster::block_on(scope.pop()) {
                None => Some((p, il, stride)),
                Some(e) => {
                    eprintln!("note: h1 instancing: {vs_name} / {ps_name} not instanced: {e}");
                    None
                }
            }
        });
        Ok(Pipeline {
            pipeline,
            layered,
            layout,
            inst,
            vs_regs: regs(&vs.reflection),
            ps_regs: regs(&ps.reflection),
            discards: ps.wgsl.contains("discard"),
        })
    }

    /// The big opaque triangles of the map's world surfaces and placements
    /// (`BO2MP_H1_OCCLUSION=0`: none), in world space.
    fn find_occluders(&self, items: &[Item]) -> Vec<native_cull::Occluder> {
        if !occlusion_on() {
            return Vec::new();
        }
        let mut seen = std::collections::HashSet::new();
        let mut tris = Vec::new();
        for it in items.iter().filter(|i| i.occluder && !i.blended) {
            // (A placed model's levels of detail differ: not occluders.)
            if it.draw.lod.is_some() || !seen.insert(it.di) {
                continue;
            }
            let Some((lo, hi)) = it.bounds else { continue };
            if (hi - lo).max_element() < OCCLUDER_MIN_EXTENT {
                continue;
            }
            let d = &it.draw;
            let Some(m) = self.scene.meshes.get(d.mesh) else { continue };
            let stride = m.kind.stride() as usize;
            let first = d.first_index as usize;
            let Some(idx) = m.indices.get(first..first + d.index_count as usize) else {
                continue;
            };
            let world = d
                .object
                .and_then(|o| self.scene.objects.get(o as usize))
                .map_or(glam::Mat4::IDENTITY, |o| o.world);
            let pos = |i: u16| -> Option<glam::Vec3> {
                let v = (d.base_vertex + i32::from(i)) as usize * stride;
                let b = m.vertex_bytes.get(v..v + 12)?;
                let p = glam::Vec3::from_array(std::array::from_fn(|k| {
                    f32::from_le_bytes([b[4 * k], b[4 * k + 1], b[4 * k + 2], b[4 * k + 3]])
                }));
                Some(world.transform_point3(p))
            };
            for t in idx.chunks_exact(3) {
                if let (Some(a), Some(b), Some(c)) = (pos(t[0]), pos(t[1]), pos(t[2])) {
                    tris.push(native_cull::Occluder { tri: [a, b, c], front: it.front });
                }
            }
        }
        if std::env::var("H1_DBG_OCC").is_ok() {
            let total = items.len();
            let occ = items.iter().filter(|i| i.occluder).count();
            let blend = items.iter().filter(|i| i.blended).count();
            let mut areas: Vec<f32> = tris
                .iter()
                .map(|o| (o.tri[1] - o.tri[0]).cross(o.tri[2] - o.tri[0]).length() * 0.5)
                .collect();
            areas.sort_by(|a, b| b.total_cmp(a));
            eprintln!(
                "dbg occ: {total} items, {occ} occluder flagged, {blend} blended, {} tris, areas top {:?}",
                tris.len(),
                &areas[..areas.len().min(8)]
            );
        }
        if std::env::var("H1_DBG_OCC").is_ok() {
            let (mut lo, mut hi) = (glam::Vec3::splat(f32::MAX), glam::Vec3::splat(f32::MIN));
            for t in tris.iter().filter(|o| (o.tri[1] - o.tri[0]).cross(o.tri[2] - o.tri[0]).length() < 1e7) {
                for p in &t.tri {
                    lo = lo.min(*p);
                    hi = hi.max(*p);
                }
            }
            eprintln!("dbg occ: map extent {lo:?} .. {hi:?}");
        }
        let n = tris.len();
        let out = native_cull::largest(tris, OCCLUDER_MIN_AREA, OCCLUDER_CAP);
        eprintln!(
            "note: h1 occlusion: {} occluder triangles kept of {n} opaque ones (BO2MP_H1_OCCLUSION=0 off)",
            out.len()
        );
        out
    }

    /// The camera's occlusion buffer (None: off, or the map has no
    /// occluders yet).
    pub fn occlusion(&mut self, camera: &crate::code::Camera) -> Option<native_cull::Occlusion> {
        let tris = self.occluders.as_deref().filter(|t| !t.is_empty())?;
        if let Some(o) = &self.occlusion_cache
            && o.is_for(&camera.view_projection, camera.eye)
        {
            return Some(o.clone());
        }
        let built = native_cull::Occlusion::new(
            camera.view_projection,
            camera.eye,
            tris,
            OCCLUSION_W,
            OCCLUSION_H,
            OCCLUSION_MARGIN,
        );
        self.occlusion_cache = Some(built.clone());
        Some(built)
    }

    fn image_tex(&mut self, name: &str) -> Option<&Tex> {
        let k = key(name);
        if !self.tex.contains_key(&k) {
            let t = match self.scene.images.image(&k) {
                Some((mut px, faces)) => {
                    if px.dxgi_format == 10
                        && k.contains("lightmap")
                        && k.ends_with("_secondary")
                        && let f = crate::light::secondary_scale(&self.scene.lightset, &self.scene.vision)
                        && (f - 1.0).abs() > 1e-3
                    {
                        gpu::scale_rgba16f(&mut px.data, f);
                    }
                    match gpu::upload(&self.gpu, &k, &px, faces) {
                    Ok(t) => Some(t),
                    Err(e) => {
                        self.report.missing_images.insert(k.clone(), e);
                        None
                    }
                    }
                }
                None => {
                    self.report
                        .missing_images
                        .insert(k.clone(), "not found".into());
                    None
                }
            };
            self.tex.insert(k.clone(), t);
        }
        self.tex.get(&k).and_then(Option::as_ref)
    }

    fn default_tex(
        &mut self,
        what: &str,
        dim: wgpu::TextureViewDimension,
        kind: TextureKind,
    ) -> &Tex {
        let k = format!("{what}/{dim:?}/{kind:?}");
        if !self.defaults.contains_key(&k) {
            let t = if kind == TextureKind::Depth {
                gpu::solid_depth(&self.gpu, 1.0, dim)
            } else {
                let rgba = match what {
                    "white" => [255, 255, 255, 255],
                    "normal" => [128, 128, 255, 255],
                    // A picture that isn't loaded: grey, alpha 0 so a
                    // decal or blend that would show it draws nothing.
                    "grey" => [128, 128, 128, 0],
                    "grey_opaque" => [128, 128, 128, 255],
                    _ => [0, 0, 0, 0],
                };
                gpu::solid(&self.gpu, rgba, dim)
            };
            self.defaults.insert(k.clone(), t);
        }
        &self.defaults[&k]
    }

    fn sampler(&mut self, s: SampSource, comparison: bool) -> wgpu::Sampler {
        let k = format!("{s:?}/{comparison}");
        if let Some(x) = self.samplers.get(&k) {
            return x.clone();
        }
        let x = if comparison {
            gpu::comparison(&self.gpu)
        } else {
            match s {
                SampSource::Material(state) => gpu::material_sampler(&self.gpu, state),
                _ => gpu::linear_clamp(&self.gpu),
            }
        };
        self.samplers.insert(k, x.clone());
        x
    }
}

trait VertexInit {
    fn create_buffer_init_vertex(&self, bytes: &[u8]) -> wgpu::Buffer;
}
impl VertexInit for wgpu::Device {
    fn create_buffer_init_vertex(&self, bytes: &[u8]) -> wgpu::Buffer {
        wgpu::util::DeviceExt::create_buffer_init(
            self,
            &wgpu::util::BufferInitDescriptor {
                label: Some("vb"),
                contents: bytes,
                usage: wgpu::BufferUsages::VERTEX,
            },
        )
    }
}

fn f16(h: u16) -> f32 {
    let s = if h & 0x8000 != 0 { -1.0 } else { 1.0 };
    let e = (h >> 10) & 0x1f;
    let m = f32::from(h & 0x3ff);
    s * match e {
        0 => m * 2f32.powi(-24),
        31 => f32::INFINITY,
        _ => (1.0 + m / 1024.0) * 2f32.powi(i32::from(e) - 15),
    }
}

/// Reads back an Rgba16Float texture.
pub fn read_rgba16f(gpu: &Gpu, tex: &wgpu::Texture, w: u32, h: u32) -> Vec<[f32; 4]> {
    let row = (w * 8).div_ceil(256) * 256;
    let buf = gpu.device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("readback"),
        size: u64::from(row * h),
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut enc = gpu.device.create_command_encoder(&Default::default());
    enc.copy_texture_to_buffer(
        tex.as_image_copy(),
        wgpu::TexelCopyBufferInfo {
            buffer: &buf,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(row),
                rows_per_image: Some(h),
            },
        },
        wgpu::Extent3d {
            width: w,
            height: h,
            depth_or_array_layers: 1,
        },
    );
    gpu.queue.submit([enc.finish()]);
    let slice = buf.slice(..);
    slice.map_async(wgpu::MapMode::Read, |_| {});
    let _ = gpu.device.poll(wgpu::PollType::wait_indefinitely());
    let data = slice.get_mapped_range();
    let mut out = Vec::with_capacity((w * h) as usize);
    for y in 0..h {
        for x in 0..w {
            let at = (y * row + x * 8) as usize;
            let c = |k: usize| f16(u16::from_le_bytes([data[at + 2 * k], data[at + 2 * k + 1]]));
            out.push([c(0), c(1), c(2), c(3)]);
        }
    }
    out
}

#[cfg(test)]
mod lod_tests {
    use super::Lod;

    #[test]
    fn a_level_is_chosen_by_the_eye_distance() {
        let l = |level, min, max| Lod { level, min, max, center: [0.0; 3] };
        let levels = [l(0, 0.0, 1200.0), l(1, 1200.0, 1800.0), l(2, 1800.0, f32::INFINITY)];
        let pick = |d: f32, k: f32| levels.iter().find(|l| l.selects(d, k)).map(|l| l.level);
        assert_eq!(pick(10.0, 1.0), Some(0));
        assert_eq!(pick(1500.0, 1.0), Some(1));
        assert_eq!(pick(90000.0, 1.0), Some(2));
        // At half the distances (LOW) the next level starts sooner.
        assert_eq!(pick(700.0, 0.5), Some(1));
        // Exactly one level is chosen at every distance.
        for d in [0.0f32, 599.9, 600.0, 900.0, 5000.0] {
            assert_eq!(levels.iter().filter(|l| l.selects(d, 0.5)).count(), 1);
        }
    }
}
