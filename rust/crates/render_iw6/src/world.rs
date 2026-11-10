//! A map's world surfaces and placed models drawn with each material's own
//! technique: the technique's programs translated (`dxbc_sm5`), its
//! constant buffers filled by its argument table (`bind`), the material's
//! fixed-function state (`state`). The machinery (constant arena, per-object
//! blocks with dynamic offsets, pipelines and bind groups made once) is
//! `render_h1::world`'s, for Ghosts' records.

use std::collections::{BTreeMap, HashMap};

use dxbc_sm5::{Options, Stage, TextureKind};
use fastfile_iw6::AssetType;
use render_h1::gpu::{self, Gpu, PassLayout, Tex};

use crate::bind::{self, CbSource, CbWrite, MaterialView, PassBinds, SampSource, TexSource};
use crate::code::{CodeValues, code_index, code_texture_name, tex};
use crate::pool::{Pass, Pool, Technique, key};
use crate::scene::{Draw, List, Object, Scene, VERTEX_LIGHT_STRIDE, VertexKind, vertex_light_input};
use crate::state::PassState;

/// Technique slots (`MaterialTechniqueType`).
pub mod slot {
    pub const DEPTH_PREPASS: usize = 0;
    pub const BUILD_SHADOWMAP_DEPTH: usize = 1;
    pub const UNLIT: usize = 2;
    /// Lit with no primary light (`ln0`); an unlit set's technique too.
    pub const LIT: usize = 7;
    /// Lit by the sun, its baked visibility (`ld0`).
    pub const LIT_SUN: usize = 8;
    /// Lit by the sun over the sun shadow map (`ld1`).
    pub const LIT_SUN_SHADOW: usize = 9;
    /// Lit by a spot light (`ls0`, unshadowed).
    pub const LIT_SPOT: usize = 10;
    /// Lit by a spot light over its shadow map (`ls1`; with the shadow's
    /// fade 0 the same as `ls0`).
    pub const LIT_SPOT_SHADOW: usize = 11;
    /// Lit by an omni light (`lo0`).
    pub const LIT_OMNI: usize = 13;
    /// Added to a slot: the same technique without displacement (no hull
    /// or domain shader; a displacement set fills both).
    pub const NO_DISPLACEMENT: usize = 309;
}

/// The slot of a distortion set's technique (it fills 3 to 6 alike).
const DISTORTION_SLOT: usize = 3;

/// What a pipeline draws into: the HDR scene, or the sun shadow map.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Target {
    Scene,
    /// The sun shadow map.
    Shadow,
    /// A spot light's shadow map (its own depth offset).
    SpotShadow,
}

pub const SHADOW_DEPTH: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;
/// The shadow pass's colour attachment: a stand-in nothing is written to
/// (every caster pipeline has one target with an empty write mask, so the
/// alpha-tested casters' pixel shaders fit the same pass).
pub const SHADOW_COLOUR: wgpu::TextureFormat = wgpu::TextureFormat::R8Unorm;
/// The HDR scene's colour format.
pub const SCENE_COLOUR: wgpu::TextureFormat = wgpu::TextureFormat::Rgba16Float;

/// Technique, pass, vertex layout, fixed-function state, second stream
/// (0 none, `VERTEX_LIGHT` a vertex-lit placement's light, else the world
/// vertex format of the layer stream), target.
type PipeKey = (String, usize, VertexKind, PassState, u8, Target);

/// The second stream of a vertex-lit placement (its baked vertex light).
pub const VERTEX_LIGHT: u8 = 254;

struct Pipeline {
    pipeline: wgpu::RenderPipeline,
    /// Its number (draw order and state tracking).
    id: u32,
    /// Constant buffer bindings with a dynamic offset, in binding order:
    /// (binding, slot).
    dynamic: Vec<(u32, u32)>,
    /// Reads the vertex light stream (bound as stream 1).
    layered: bool,
    layout: PassLayout,
    regs: BTreeMap<u32, u32>,
    /// The same pass reading its per-primitive block per instance
    /// (`instancing`): pipeline, layout, block stride.
    inst: Option<(wgpu::RenderPipeline, PassLayout, u64)>,
    /// Its pixel program can discard (alpha test): its depth is not a
    /// wall's.
    discards: bool,
}

/// A list entry's instanced form: pipeline, constant group, which dynamic
/// offset is the per-primitive block, the blocks' stride.
#[derive(Clone)]
struct Inst {
    pipeline: wgpu::RenderPipeline,
    group: wgpu::BindGroup,
    b0: u8,
    stride: u32,
}

#[derive(Default, Debug, Clone)]
pub struct Report {
    pub draws: usize,
    pub drawn: usize,
    pub techniques: usize,
    /// Techniques whose pipelines did not build, with why.
    pub failed: BTreeMap<String, String>,
    /// Why some draws weren't drawn, with a count.
    pub skipped: BTreeMap<String, usize>,
    /// Draws per technique slot used.
    pub by_slot: BTreeMap<usize, usize>,
    pub missing_images: BTreeMap<String, String>,
    /// Bind groups made with a fallback picture: (opaque grey, alpha-0
    /// grey for blended and cut-out materials, whose picture's alpha is
    /// their shape).
    pub fallback_groups: [usize; 2],
    pub missing_code: Vec<String>,
    pub shadow_drawn: usize,
    /// Opaque scene draws with no depth pre-pass that write depth in their
    /// own pass (`PassState::needs_own_depth`).
    pub own_depth: usize,
}

pub struct Frame<'a> {
    pub code: &'a CodeValues,
    /// Use the sun shadow technique for sunlit surfaces.
    pub sun_shadow: bool,
    pub part: Part,
    pub target: Target,
    /// Separate constant buffers for draws of the same pipelines with
    /// other code values in one encoder (shadow cascades).
    pub variant: u8,
    /// Culling planes: a draw whose bounds lie outside any is left out.
    pub cull: Option<&'a [glam::Vec4]>,
    /// Which of the target's views (`views_for`: a sun cascade, a spot
    /// tile) this is.
    pub view: u32,
    /// The camera's occlusion buffer (`Renderer::occlusion`): draws wholly
    /// behind it are left out (the scene's views only).
    pub occlusion: Option<&'a native_cull::Occlusion>,
    /// Draws spanning less than this angle (radians) from the eye are left
    /// out (`small_angle`; the scene's views only).
    pub small_angle: Option<f32>,
}

pub struct Targets<'a> {
    pub colour: &'a wgpu::TextureView,
    pub depth: &'a wgpu::TextureView,
    pub clear: Option<[f64; 4]>,
    pub depth_clear: f32,
    pub viewport: Option<[f32; 6]>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Part {
    /// Opaque surfaces and decals (fogged by the post-opaque pass).
    Opaque,
    /// Blended surfaces, drawn after the fog.
    Blended,
}

/// The technique a draw takes: the sun's (with or without the shadow map)
/// when its primary light is the sun, else the unlit-by-a-light one; for
/// the shadow map the casters' technique.
/// `lights`: (spot and omni lights drawn, spot lights over their shadow
/// map).
pub fn slots_for(d: &Draw, lights: &[crate::scene::PrimaryLight], sun_shadow: bool, target: Target, local: (bool, bool)) -> &'static [usize] {
    match target {
        // (Not a caster of this light's map: no technique, not drawn.)
        Target::Shadow if !d.casts[0] => &[],
        Target::SpotShadow if !d.casts[1] => &[],
        Target::Shadow | Target::SpotShadow => &[slot::BUILD_SHADOWMAP_DEPTH],
        Target::Scene if d.list == List::Sky => &[slot::LIT, slot::UNLIT],
        Target::Scene if d.sun && sun_shadow => &[slot::LIT_SUN_SHADOW, slot::LIT_SUN, slot::LIT],
        Target::Scene if d.sun => &[slot::LIT_SUN, slot::LIT],
        Target::Scene => match d.light.filter(|_| local.0).and_then(|l| lights.get(usize::from(l))).map(|l| l.ty) {
            Some(2) if local.1 => &[slot::LIT_SPOT_SHADOW, slot::LIT_SPOT, slot::LIT],
            Some(2) => &[slot::LIT_SPOT, slot::LIT],
            Some(3) => &[slot::LIT_OMNI, slot::LIT],
            _ => &[slot::LIT],
        },
    }
}

/// `BO2MP_IW6_MODEL_PREPASS=0`: placed models get no depth pre-pass entry
/// of their own: the world's pre-pass (the big surfaces, where overdraw
/// is) still hides what lies behind a wall from a model's lit pass, and
/// the model's lit pass writes depth itself (nearest wins). It takes about
/// 15% of the draw entries off a view toward the middle of a map,
/// but it is NOT byte-identical: where two models are coplanar (a
/// container's overlapping panels, cables) the pre-pass's equal test and
/// the nearest-wins test pick differently (4,800 of 921,600 pixels on
/// the first inward spawn view, up to 53 levels), so it is off by default.
pub fn model_prepass_on() -> bool {
    static ON: crate::switch::Switch = crate::switch::Switch::on("BO2MP_IW6_MODEL_PREPASS");
    ON.get()
}

/// `BO2MP_IW6_FAR_MIN=0`: casters smaller than a far cascade texel are
/// drawn into it too.
pub fn far_min_on() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| !std::env::var("BO2MP_IW6_FAR_MIN").is_ok_and(|v| v.trim() == "0"))
}

/// `BO2MP_IW6_LOCAL_LIGHTS=0`: surfaces whose primary light is a spot or
/// omni light draw with `ln0` (the baked pages only), as before.
pub fn local_lights_on() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| !std::env::var("BO2MP_IW6_LOCAL_LIGHTS").is_ok_and(|v| v.trim() == "0"))
}

/// The plain twin of a displacement-only unlit effect set: `w_unlit_add_lin_bl_blmod_z1_nfwpf`
/// (a ground layer's glow card: only a hull-shader technique, no `no_tess`
/// form) is the set `wc_unlit_add_lin_ndw_bl_blmod_nfwpf` without its layer
/// suffix (same picture and blend arguments, a trivial vertex shader).
/// INFERRED from the names; `None` when the pool holds no such set.
fn plain_twin(pool: &Pool, techset: &str) -> Option<String> {
    let t = techset.to_ascii_lowercase();
    let rest = t.strip_prefix("w_unlit_")?;
    let mut parts: Vec<&str> = rest.split('_').collect();
    parts.retain(|p| !(p.len() == 2 && matches!(p.as_bytes()[0], b'z' | b'y') && p.as_bytes()[1].is_ascii_digit()));
    let at = parts.iter().position(|p| *p == "bl")?;
    let mut with_ndw = parts.clone();
    if !with_ndw.contains(&"ndw") {
        with_ndw.insert(at, "ndw");
    }
    [with_ndw, parts]
        .into_iter()
        .map(|p| format!("wc_unlit_{}", p.join("_")))
        .find(|n| pool.techsets.contains_key(&key(n)))
}

pub fn pick<'p>(pool: &'p Pool, techset: &str, slots: &[usize]) -> Option<(usize, &'p Technique)> {
    pick_set(pool, techset, slots)
        .or_else(|| {
            // An effect set with no lit slot at all (`mc_effect_falloff_add_..` fills 2..6
            // only): its unlit technique. (Never the shadow casters' proxies, which fill
            // slot 2 too and are not drawn in the scene.)
            (techset.contains("_effect_") && slots.contains(&slot::LIT)).then(|| pick_set(pool, techset, &[slot::UNLIT])).flatten()
        })
        .or_else(|| {
            // A distortion set (`distortion_scale`, `distortion_shade`) fills
            // slots 3 to 6 with one technique, and no lit or unlit slot.
            (techset.starts_with("distortion") && slots.contains(&slot::LIT)).then(|| pick_set(pool, techset, &[DISTORTION_SLOT])).flatten()
        })
        .or_else(|| {
        let twin = plain_twin(pool, techset)?;
        pick_set(pool, &twin, slots)
    })
}

fn pick_set<'p>(pool: &'p Pool, techset: &str, slots: &[usize]) -> Option<(usize, &'p Technique)> {
    let tessellated = |t: &Technique| t.passes.iter().any(|p| p.hull_shader.is_some());
    slots.iter().find_map(|&s| {
        match pool.technique_in(techset, s) {
            Some(t) if !tessellated(t) => Some((s, t)),
            // A displacement technique: its form without (tessellation is
            // not drawn).
            _ => pool
                .technique_in(techset, s + slot::NO_DISPLACEMENT)
                .filter(|t| !tessellated(t))
                .map(|t| (s + slot::NO_DISPLACEMENT, t)),
        }
    })
}

type MeshBuf = (wgpu::Buffer, wgpu::Buffer, Option<wgpu::Buffer>);

/// What the draw loop writes through: a render pass, or a render bundle
/// encoder (the same commands either way).
trait Rec<'a> {
    fn pipeline(&mut self, p: &'a wgpu::RenderPipeline);
    fn group(&mut self, i: u32, g: &'a wgpu::BindGroup, offsets: &[u32]);
    fn index(&mut self, b: wgpu::BufferSlice<'a>);
    fn vertex(&mut self, slot: u32, b: wgpu::BufferSlice<'a>);
    fn draw(&mut self, indices: std::ops::Range<u32>, base: i32, instances: std::ops::Range<u32>);
    fn multi(&mut self, args: &'a wgpu::Buffer, offset: u64, count: u32);
}

impl<'a> Rec<'a> for wgpu::RenderPass<'_> {
    fn pipeline(&mut self, p: &'a wgpu::RenderPipeline) {
        self.set_pipeline(p);
    }
    fn group(&mut self, i: u32, g: &'a wgpu::BindGroup, offsets: &[u32]) {
        self.set_bind_group(i, g, offsets);
    }
    fn index(&mut self, b: wgpu::BufferSlice<'a>) {
        self.set_index_buffer(b, wgpu::IndexFormat::Uint16);
    }
    fn vertex(&mut self, slot: u32, b: wgpu::BufferSlice<'a>) {
        self.set_vertex_buffer(slot, b);
    }
    fn draw(&mut self, indices: std::ops::Range<u32>, base: i32, instances: std::ops::Range<u32>) {
        self.draw_indexed(indices, base, instances);
    }
    fn multi(&mut self, args: &'a wgpu::Buffer, offset: u64, count: u32) {
        self.multi_draw_indexed_indirect(args, offset, count);
    }
}

impl<'a> Rec<'a> for wgpu::RenderBundleEncoder<'a> {
    fn pipeline(&mut self, p: &'a wgpu::RenderPipeline) {
        self.set_pipeline(p);
    }
    fn group(&mut self, i: u32, g: &'a wgpu::BindGroup, offsets: &[u32]) {
        self.set_bind_group(i, g, offsets);
    }
    fn index(&mut self, b: wgpu::BufferSlice<'a>) {
        self.set_index_buffer(b, wgpu::IndexFormat::Uint16);
    }
    fn vertex(&mut self, slot: u32, b: wgpu::BufferSlice<'a>) {
        self.set_vertex_buffer(slot, b);
    }
    fn draw(&mut self, indices: std::ops::Range<u32>, base: i32, instances: std::ops::Range<u32>) {
        self.draw_indexed(indices, base, instances);
    }
    fn multi(&mut self, args: &'a wgpu::Buffer, offset: u64, count: u32) {
        for k in 0..u64::from(count) {
            self.draw_indexed_indirect(args, offset + k * 20);
        }
    }
}

/// What one run of the draw loop did.
#[derive(Default)]
struct Chunk {
    drawn: usize,
    issued: u32,
    /// The indirect records made (5 words each, from `Indirect::base`).
    args: Vec<u32>,
    counts: crate::profile::Counts,
}

impl Chunk {
    fn add(&mut self, o: &Chunk) {
        self.args.extend_from_slice(&o.args);
        self.drawn += o.drawn;
        self.issued += o.issued;
        self.counts += o.counts;
    }
}

/// How many threads record a view's draws as render bundles
/// (`BO2MP_IW6_BUNDLE_THREADS=n`; 1, the default: all in the pass, as
/// before; the picture is byte for byte the same either way). Off by
/// default: on lavapipe the CPU time of `finish` did not fall (wgpu
/// replays a bundle's commands into the driver at `finish` all the same),
/// so there is nothing measured to take the threads' cost for.
fn bundle_threads(draws: usize) -> usize {
    static SET: std::sync::OnceLock<Option<usize>> = std::sync::OnceLock::new();
    let set = *SET.get_or_init(|| std::env::var("BO2MP_IW6_BUNDLE_THREADS").ok().and_then(|v| v.trim().parse::<usize>().ok()));
    match set {
        Some(n) if draws >= 1500 => n.max(1),
        _ => 1,
    }
}

/// The commands for `seen` (indices of `list`, in order) into `rec`.
fn encode_draws<'a, R: Rec<'a>>(
    rec: &mut R,
    list: &'a [Ready],
    seen: &[u32],
    mesh_buffers: &'a [MeshBuf],
    view: u32,
    instancing: bool,
    blockidx: bool,
    indirect: Option<(&'a wgpu::Buffer, u64)>,
    profile: bool,
) -> Chunk {
    let mut out = Chunk::default();
    let counts = &mut out.counts;
    let mut drawn = 0usize;
    let mut bound_mesh = usize::MAX;
    let mut last_pipeline = u32::MAX;
    let mut last_g0 = u32::MAX;
    let mut last_g1 = u32::MAX;
    let mut last_g2 = u32::MAX;
    let mut offsets: Vec<u32> = Vec::new();
    let mut last_offsets: Vec<u32> = Vec::new();
    let (mut bi_offsets, mut bi_last_offsets): (Vec<u32>, Vec<u32>) = (Vec::new(), Vec::new());
    let mut bi_g0 = u32::MAX;
    // A run of draws with the same state over adjacent index ranges
    // goes as one draw: (first, count, base).
    let mut run: Option<(u32, u32, i32, u32, u32)> = None;
    // Runs of the block-indexed path wait in `args` (when indirect draws
    // are on) until the next state change, then go as one multi-draw.
    let inst_mode = blockidx;
    let mut args: Vec<u32> = Vec::new();
    let mut batch_from = 0usize;
    let flush = |rec: &mut R, run: &mut Option<(u32, u32, i32, u32, u32)>, issued: &mut u32, args: &mut Vec<u32>| {
        if let Some((first, count, base, inst, n)) = run.take() {
            if indirect.is_some() && inst_mode {
                args.extend_from_slice(&[count, n, first, base as u32, inst]);
            } else {
                rec.draw(first..first + count, base, inst..inst + n);
                *issued += 1;
            }
        }
    };
    let emit = |rec: &mut R, args: &Vec<u32>, from: &mut usize, issued: &mut u32| {
        if let Some((buf, at)) = indirect {
            let n = (args.len() - *from) / 5;
            if n == 1 {
                let a = &args[*from..*from + 5];
                rec.draw(a[2]..a[2] + a[0], a[3] as i32, a[4]..a[4] + a[1]);
                *issued += 1;
            } else if n > 1 {
                rec.multi(buf, at + *from as u64 * 4, n as u32);
                *issued += 1;
            }
            *from = args.len();
        }
    };
    let mut issued = 0u32;
    let mut last_call = None;
    let mut si = 0;
    while si < seen.len() {
        let i = seen[si];
        si += 1;
        let r = &list[i as usize];
        drawn += 1;
        // This view's dynamic offsets.
        let offs: &[u32] = if view == 0 {
            &r.offsets
        } else {
            offsets.clear();
            offsets.extend(r.offsets.iter().zip(&r.steps).map(|(o, s)| o + view * s));
            &offsets
        };
        // Block-indexed draws (`BO2MP_IW6_BLOCKIDX`): the entry's
        // per-primitive block is the draw's first instance, read from the
        // whole object buffer bound once, so a change of object is no
        // bind-group set and the draws of one state follow each other.
        if blockidx
            && r.layer.is_none()
            && let Some(inst) = &r.inst
        {
            let b0 = usize::from(inst.b0);
            let idx = offs[b0] / inst.stride;
            bi_offsets.clear();
            bi_offsets.extend_from_slice(offs);
            bi_offsets[b0] = 0;
            let id = r.pipe_id | 0x8000_0000;
            let same = r.mesh == bound_mesh
                && last_pipeline == id
                && bi_g0 == r.group_ids[0]
                && bi_last_offsets == bi_offsets
                && last_g1 == r.group_ids[1]
                && last_g2 == r.group_ids[2];
            if same && let Some((first, count, base, i0, n)) = &mut run {
                if *base == r.base && *first + *count == r.first && *n == 1 && *i0 == idx {
                    *count += r.count;
                    counts.merged += 1;
                    continue;
                }
                if *base == r.base && *first == r.first && *count == r.count && *i0 + *n == idx {
                    *n += 1;
                    counts.instanced += 1;
                    continue;
                }
            }
            flush(rec, &mut run, &mut issued, &mut args);
            if !same {
                emit(rec, &args, &mut batch_from, &mut issued);
            }
            let mb = &mesh_buffers[r.mesh];
            if r.mesh != bound_mesh {
                rec.index(mb.1.slice(..));
                rec.vertex(0, mb.0.slice(..));
                counts.index_buffers += 1;
                counts.vertex_buffers += 1;
                bound_mesh = r.mesh;
            }
            if last_pipeline != id {
                rec.pipeline(&inst.pipeline);
                counts.pipelines += 1;
                last_pipeline = id;
                bi_g0 = u32::MAX;
            }
            if !(bi_g0 == r.group_ids[0] && bi_last_offsets == bi_offsets) {
                rec.group(0, &inst.group, &bi_offsets);
                counts.groups[0] += 1;
                bi_g0 = r.group_ids[0];
                std::mem::swap(&mut bi_last_offsets, &mut bi_offsets);
            }
            last_g0 = u32::MAX;
            if last_g1 != r.group_ids[1] {
                rec.group(1, &r.groups[1], &[]);
                counts.groups[1] += 1;
                last_g1 = r.group_ids[1];
            }
            if last_g2 != r.group_ids[2] {
                rec.group(2, &r.groups[2], &[]);
                counts.groups[2] += 1;
                last_g2 = r.group_ids[2];
            }
            run = Some((r.first, r.count, r.base, idx, 1));
            continue;
        }
        let same_mesh = r.layer.is_none() && r.mesh == bound_mesh;
        let same = same_mesh
            && last_pipeline == r.pipe_id
            && last_g0 == r.group_ids[0]
            && last_offsets == offs
            && last_g1 == r.group_ids[1]
            && last_g2 == r.group_ids[2];
        if same && let Some((first, count, base, _, _)) = &mut run
            && *base == r.base
            && *first + *count == r.first
        {
            *count += r.count;
            counts.merged += 1;
            continue;
        }
        if profile {
            let k = (r.pipe_id, r.group_ids, r.mesh, r.first, r.count, r.base);
            if r.layer.is_none() && last_call == Some(k) {
                counts.instanceable += 1;
            }
            last_call = Some(k);
        }
        flush(rec, &mut run, &mut issued, &mut args);
        emit(rec, &args, &mut batch_from, &mut issued);
        // The next entries that repeat this one but for their
        // per-primitive block, each the block after the last: one
        // instanced call (`instancing`).
        let mut n = 1u32;
        if instancing
            && r.layer.is_none()
            && let Some(inst) = &r.inst
        {
            let b0 = usize::from(inst.b0);
            while si < seen.len() && n < crate::instancing::MAX_INSTANCES {
                let q = &list[seen[si] as usize];
                let follows = q.layer.is_none()
                    && q.pipe_id == r.pipe_id
                    && q.group_ids == r.group_ids
                    && q.mesh == r.mesh
                    && q.first == r.first
                    && q.count == r.count
                    && q.base == r.base
                    && q.offsets.len() == r.offsets.len()
                    && q.offsets.iter().zip(&r.offsets).enumerate().all(|(k, (a, b))| {
                        if k == b0 { *a == b + n * inst.stride } else { a == b }
                    });
                if !follows {
                    break;
                }
                n += 1;
                si += 1;
            }
        }
        if n > 1
            && let Some(inst) = &r.inst
        {
            drawn += n as usize - 1;
            counts.instanced += n - 1;
            let mb = &mesh_buffers[r.mesh];
            if r.mesh != bound_mesh {
                rec.index(mb.1.slice(..));
                rec.vertex(0, mb.0.slice(..));
                counts.index_buffers += 1;
                counts.vertex_buffers += 1;
                bound_mesh = r.mesh;
            }
            let id = r.pipe_id | 0x8000_0000;
            if last_pipeline != id {
                rec.pipeline(&inst.pipeline);
                counts.pipelines += 1;
                last_pipeline = id;
            }
            rec.group(0, &inst.group, offs);
            counts.groups[0] += 1;
            last_g0 = u32::MAX;
            if last_g1 != r.group_ids[1] {
                rec.group(1, &r.groups[1], &[]);
                counts.groups[1] += 1;
                last_g1 = r.group_ids[1];
            }
            if last_g2 != r.group_ids[2] {
                rec.group(2, &r.groups[2], &[]);
                counts.groups[2] += 1;
                last_g2 = r.group_ids[2];
            }
            rec.draw(r.first..r.first + r.count, r.base, 0..n);
            issued += 1;
            continue;
        }
        let mb = &mesh_buffers[r.mesh];
        if r.mesh != bound_mesh {
            rec.index(mb.1.slice(..));
            counts.index_buffers += 1;
        }
        let base = match (r.layer, &mb.2) {
            (Some((main, layer)), Some(lb)) => {
                rec.vertex(0, mb.0.slice(main..));
                rec.vertex(1, lb.slice(layer..));
                counts.vertex_buffers += 2;
                bound_mesh = usize::MAX - 1;
                0
            }
            _ => {
                if r.mesh != bound_mesh {
                    rec.vertex(0, mb.0.slice(..));
                    counts.vertex_buffers += 1;
                    bound_mesh = r.mesh;
                }
                r.base
            }
        };
        if last_pipeline != r.pipe_id {
            rec.pipeline(&r.pipeline);
            counts.pipelines += 1;
            last_pipeline = r.pipe_id;
        }
        if !(last_g0 == r.group_ids[0] && last_offsets == offs) {
            rec.group(0, &r.groups[0], offs);
            counts.groups[0] += 1;
            last_g0 = r.group_ids[0];
            last_offsets.clear();
            last_offsets.extend_from_slice(offs);
        }
        if last_g1 != r.group_ids[1] {
            rec.group(1, &r.groups[1], &[]);
            counts.groups[1] += 1;
            last_g1 = r.group_ids[1];
        }
        if last_g2 != r.group_ids[2] {
            rec.group(2, &r.groups[2], &[]);
            counts.groups[2] += 1;
            last_g2 = r.group_ids[2];
        }
        run = Some((r.first, r.count, base, 0, 1));
    }
    // (The last run: the encoder before this dropped it; `BO2MP_IW6_LASTRUN=0` restores that.)
    static LASTRUN: crate::switch::Switch = crate::switch::Switch::on("BO2MP_IW6_LASTRUN");
    if LASTRUN.get() {
        flush(rec, &mut run, &mut issued, &mut args);
    }
    emit(rec, &args, &mut batch_from, &mut issued);
    out.args = args;
    out.drawn = drawn;
    out.issued = issued;
    out
}

struct Ready {
    pipeline: wgpu::RenderPipeline,
    pipe_id: u32,
    groups: [wgpu::BindGroup; 3],
    /// Its instanced form, when its pipeline has one (`instancing`).
    inst: Option<Inst>,
    /// The groups' ids (constant buffers, textures, samplers).
    group_ids: [u32; 3],
    /// The dynamic offsets (in binding order) for view 0, and what each
    /// adds a view.
    offsets: Vec<u32>,
    steps: Vec<u32>,
    layer: Option<(u64, u64)>,
    mesh: usize,
    first: u32,
    count: u32,
    base: i32,
    bounds: Option<(glam::Vec3, glam::Vec3)>,
    /// Its object's origin and the distances it is drawn between (LOD band,
    /// cull distance).
    range: Option<(glam::Vec3, f32, f32)>,
    /// The cells that hold it (`cells`): a run of `Renderer::cell_lists`;
    /// empty: not culled by the portals.
    cells: (u32, u16),
    /// Its draw's index (the occlusion test's per-frame memo).
    di: u32,
}

/// What a list entry is culled by, packed apart from the entry (`Ready`
/// holds the pipeline, groups and offsets: the cull walks tens of
/// thousands of entries a view, and reading these alone keeps it in cache).
#[derive(Clone, Copy)]
struct Cull {
    lo: glam::Vec3,
    hi: glam::Vec3,
    bounded: bool,
    ranged: bool,
    origin: glam::Vec3,
    near: f32,
    far: f32,
    cells: (u32, u16),
    di: u32,
}

impl Cull {
    fn of(r: &Ready) -> Self {
        let (lo, hi) = r.bounds.unwrap_or((glam::Vec3::ZERO, glam::Vec3::ZERO));
        let (origin, near, far) = r.range.unwrap_or((glam::Vec3::ZERO, 0.0, 0.0));
        Self {
            lo,
            hi,
            bounded: r.bounds.is_some(),
            ranged: r.range.is_some(),
            origin,
            near,
            far,
            cells: r.cells,
            di: r.di,
        }
    }
}

struct Prepared {
    /// The leading entries of `opaque` that are the depth pre-pass (the
    /// scene's; 0 for the shadow lists).
    prepass_n: usize,
    opaque: Vec<Ready>,
    blended: Vec<Ready>,
    /// Each entry's `Cull`, in the lists' order.
    opaque_cull: Vec<Cull>,
    blended_cull: Vec<Cull>,
    plans: Vec<usize>,
    /// The frame blocks its plans bind, by light (the frame's own first).
    blocks: Vec<(Option<u16>, Vec<usize>)>,
}

#[derive(Clone, Copy, Debug)]
struct Slice {
    page: usize,
    offset: u64,
    size: u64,
}

/// A page: its buffer, its CPU mirror, the byte range changed since the
/// last flush.
type ArenaPage = (wgpu::Buffer, Vec<u8>, Option<(u64, u64)>);

/// Constant buffers in a few large uniform buffers mirrored on the CPU
/// (`render_h1::world`'s arena). A write that changes nothing is not
/// uploaded.
struct Arena {
    pages: Vec<ArenaPage>,
    align: u64,
    label: &'static str,
}

const PAGE: u64 = 4 << 20;

impl Arena {
    fn alloc(&mut self, gpu: &Gpu, size: u64) -> Slice {
        let size = size.max(16);
        if let Some(i) = self.pages.len().checked_sub(1) {
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
            label: Some(self.label),
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

    /// Writes `regs` at `at` moved on by `skip` bytes.
    fn write(&mut self, at: Slice, skip: u64, regs: &[[f32; 4]]) {
        let (_, m, dirty) = &mut self.pages[at.page];
        let bytes: &[u8] = bytemuck::cast_slice(regs);
        let n = bytes.len().min(at.size as usize);
        let start = at.offset + skip;
        let dst = &mut m[start as usize..start as usize + n];
        if *dst == bytes[..n] {
            return;
        }
        dst.copy_from_slice(&bytes[..n]);
        let end = start + n as u64;
        *dirty = Some(match *dirty {
            Some((a, b)) => (a.min(start), b.max(end)),
            None => (start, end),
        });
    }

    /// Writes the changed ranges; the bytes written.
    fn flush(&mut self, queue: &wgpu::Queue) -> u64 {
        let mut n = 0;
        for (b, m, dirty) in &mut self.pages {
            if let Some((a, e)) = dirty.take() {
                let a = a / 4 * 4;
                let e = e.next_multiple_of(4).min(m.len() as u64);
                queue.write_buffer(b, a, &m[a as usize..e as usize]);
                n += e - a;
            }
        }
        n
    }

    fn binding(&self, at: Slice) -> wgpu::BindingResource<'_> {
        wgpu::BindingResource::Buffer(wgpu::BufferBinding {
            buffer: &self.pages[at.page].0,
            offset: at.offset,
            size: std::num::NonZeroU64::new(at.size),
        })
    }
}

/// (pipeline, variant, material, lightmap, probe).
type GroupKey = (PipeKey, u8, Option<u16>, usize, u8, u8);
/// (sun shadow, target, variant, light flags: 1 spot and omni lights, 2 spot
/// shadow technique).
type PrepKey = (bool, Target, u8, u8);

/// How many views a target's constants hold side by side: the sun's two
/// cascades, the spot lights' four tiles, the scene's one. Every view of a
/// target is drawn from one list in the frame's one encoder: each view's
/// frame blocks and per-object blocks are its own (dynamic offsets).
pub fn views_for(target: Target) -> u32 {
    match target {
        Target::Scene => 1,
        Target::Shadow => 2,
        Target::SpotShadow => crate::spot_shadow::SPOT_LIMIT as u32,
    }
}

/// The constant buffers of one pipeline drawn with one light: its frame
/// blocks (shared by every plan of the same layout) and its per-object
/// block, one row block per object and view.
struct CbPlan {
    writes: Vec<CbWrite>,
    sizes: BTreeMap<u32, u32>,
    stride: u64,
    objects: Vec<Option<u32>>,
    object_slot: HashMap<Option<u32>, u32>,
    per_object: Option<wgpu::Buffer>,
    mirror: Vec<u8>,
    /// Frame blocks (`Renderer::frame_blocks`) by slot.
    frame: BTreeMap<u32, usize>,
    /// The spot or omni light its per-object constants are for.
    light: Option<u16>,
    views: u32,
    /// Whether every object's block holds its rows (made once; the rows
    /// that follow the view are rewritten each frame).
    filled: bool,
    /// (object slot, write) pairs whose rows follow the view or the frame.
    follows: Vec<(u32, u16)>,
    /// Object slots whose placement changed since the last update.
    moved: Vec<u32>,
}

/// One frame-rate constant block (a slot's rows from the frame's values, or
/// a light's) shared by every plan that lays it out the same.
struct FrameBlock {
    at: Slice,
    vstride: u64,
    slot: u32,
    size: u32,
    writes: Vec<CbWrite>,
    light: Option<u16>,
}

/// Draws a scene with Ghosts' techniques. Long-lived: pipelines, textures,
/// buffers and bind groups are made once; each frame rewrites the frame's
/// constants and the per-object rows that follow the view, and records the
/// draws.
pub struct Renderer {
    pub gpu: Gpu,
    pub scene: Scene,
    pub depth_format: wgpu::TextureFormat,
    /// Code textures by `TEXTURE_SRC_CODE` name.
    pub code_textures: HashMap<String, Tex>,
    /// `LIGHTING_LOOKUP_SCALE` of the model lighting volume.
    pub lighting_lookup: [f32; 4],
    tex: HashMap<String, Option<Tex>>,
    /// bo2mp: the map bake of the pictures (`bake`).
    bake: crate::bake::State,
    pipelines: HashMap<PipeKey, Option<Pipeline>>,
    samplers: HashMap<String, wgpu::Sampler>,
    defaults: HashMap<String, Tex>,
    mesh_buffers: Vec<MeshBuf>,
    /// The world's layer stream layouts by world vertex format.
    layer_layouts: HashMap<u8, crate::layers::Layout>,
    plans: Vec<CbPlan>,
    plan_of: HashMap<(PipeKey, u8, Option<u16>), usize>,
    /// The plans each object is drawn with (to refill a moved object).
    plans_of_object: HashMap<u32, Vec<usize>>,
    material_cb: HashMap<(usize, usize, u32), Slice>,
    /// Constant groups by (pipeline, plan, material when the pipeline reads
    /// material constants): draws that differ only in picture, lightmap or
    /// probe share one (`BO2MP_IW6_G0_SHARE=0`: one each, as before).
    g0_cache: HashMap<(PipeKey, usize, usize), (wgpu::BindGroup, u32, Option<wgpu::BindGroup>)>,
    /// Material blocks (written once).
    arena: Arena,
    /// Frame blocks (rewritten each frame where they change).
    frame_arena: Arena,
    frame_blocks: Vec<FrameBlock>,
    frame_block_of: HashMap<String, usize>,
    groups: HashMap<GroupKey, ([wgpu::BindGroup; 3], [u32; 3], Option<wgpu::BindGroup>)>,
    /// Texture and sampler groups by what they bind (`make_groups`).
    shared_groups: HashMap<String, (wgpu::BindGroup, u32)>,
    prepared: HashMap<PrepKey, Prepared>,
    idx: Indices,
    pub report: Report,
    pub bounds: Vec<Option<(glam::Vec3, glam::Vec3)>>,
    /// The spot lights with a shadow map this frame (`spot_shadow`), by
    /// light index; set by the host before the scene is recorded.
    pub spot_shadows: HashMap<u16, crate::spot_shadow::SpotShadow>,
    /// What the last `record` did (`profile`; kept only with the profile on).
    pub stats: crate::profile::Phase,
    /// The last constants written: (list, view, frame); the blended part
    /// of the same list and view in the same frame reuses them.
    last_update: Option<(PrepKey, u32, u64)>,
    /// Counts frames (`begin_frame`).
    frame: u64,
    /// Scratch rows and code values reused every frame.
    rows: Vec<[f32; 4]>,
    /// The values of each local light, built once per `update` (a light has
    /// several plans): (light, the update it was made in, values).
    lit_cache: Vec<(u16, u64, CodeValues)>,
    update_serial: u64,
    /// The code constants last reported missing (indices), so the report's
    /// names are made when they change.
    missing_seen: Vec<u16>,
    /// Scales on the LOD bands' and the cull distances (`native_quality`).
    pub lod_scale: f32,
    pub draw_scale: f32,
    /// For the next shadow view recorded: two axes of the light and a size;
    /// a caster whose bounds span less than it along both is left out (a
    /// caster under one texel of the far sun cascade, `far_min_on`).
    pub min_caster: Option<(glam::Vec3, glam::Vec3, f32)>,
    /// Spot and omni lights drawn with their own techniques; spot lights
    /// over their shadow map (`ls1`, else `ls0`). Lists made after a change
    /// follow it.
    pub local_lights: bool,
    pub spot_shadow_technique: bool,
    /// The largest anisotropy a material sampler takes (made as the groups
    /// are).
    pub anisotropy: u16,
    /// Every list entry's cells, in runs (`Ready::cells`).
    cell_lists: Vec<u16>,
    /// This frame's portal walk (`cells`); the scene's draws are culled by
    /// it while valid.
    pub cell_view: Option<crate::cells::CellView>,
    /// Counts object moves and list rebuilds (what the shadow atlases
    /// hold changes with them).
    pub moved: u64,
    /// The entries seen by the last cull (reused every record).
    seen_scratch: Vec<u32>,
    /// The indirect records of the frame's passes: buffer, capacity, next free byte, frame.
    indirect: Option<(wgpu::Buffer, u64, u64, u64)>,
    /// Shader modules made, by their text (the same program is in many pipelines).
    modules: std::sync::Mutex<HashMap<String, wgpu::ShaderModule>>,
    /// The driver's pipeline cache, and the file it is kept in.
    pipeline_cache: Option<(wgpu::PipelineCache, std::path::PathBuf, usize)>,
    /// The map's big opaque triangles (`find_occluders`), made with the
    /// scene list; None until then.
    occluders: Option<native_cull::OccluderSet>,
    /// The last view's occlusion buffer, kept while the view stands still.
    occlusion_cache: Option<std::sync::Arc<native_cull::Occlusion>>,
    /// Per draw, this frame's occlusion verdict (0 not asked, 1 shows, 2
    /// hidden, 3 too small).
    occ_memo: Vec<u8>,
}

/// Code constant indices computed per object.
#[derive(Clone, Copy)]
struct Indices {
    wvp: u16,
    inv_wv: u16,
    world: u16,
    lookup: u16,
}

fn mat_rows(m: glam::Mat4) -> [[f32; 4]; 4] {
    [m.row(0).to_array(), m.row(1).to_array(), m.row(2).to_array(), m.row(3).to_array()]
}

/// The rows write `w` puts in its block for `obj`, appended to `out`: the
/// object's own rows first, then its `TRANSPOSE_WORLD_VIEW_PROJECTION_MATRIX0`
/// (the frame's eye-relative view-projection, the eye offset, its
/// placement), `INVERSE_TRANSPOSE_WORLD_VIEW_MATRIX0`, a world surface's
/// identity placement, the lighting lookup, else the frame's value.
fn write_rows(w: &CbWrite, obj: Option<&Object>, code: &CodeValues, lookup: [f32; 4], idx: Indices, out: &mut Vec<[f32; 4]>) {
    match &w.src {
        CbSource::Code {
            index,
            first_row,
            rows,
        } => {
            let (first, n) = (*first_row as usize, *rows as usize);
            let pick = |v: &[[f32; 4]], out: &mut Vec<[f32; 4]>| {
                out.extend((first..first + n).map(|r| v.get(r).copied().unwrap_or([0.0; 4])));
            };
            let world = || obj.map_or(glam::Mat4::IDENTITY, |o| o.world);
            if let Some((_, v)) = obj.and_then(|o| o.rows.iter().find(|(i, _)| i == index)) {
                pick(v, out);
            } else if *index == idx.wvp {
                let m = code.view_projection * glam::Mat4::from_translation(-code.eye) * world();
                pick(&mat_rows(m), out);
            } else if *index == idx.inv_wv {
                // The inverse of the object's world to (eye-relative) view.
                let m = (code.rotation * glam::Mat4::from_translation(-code.eye) * world()).inverse();
                pick(&mat_rows(m), out);
            } else if *index == idx.world && obj.is_none() {
                pick(&mat_rows(glam::Mat4::IDENTITY), out);
            } else if *index == idx.lookup {
                pick(&[lookup], out);
            } else {
                out.extend((first..first + n).map(|r| code.row(*index, r)));
            }
        }
        CbSource::Literal(v) | CbSource::Material(v) => out.push(*v),
        CbSource::MissingMaterial(_) => out.push([0.0; 4]),
    }
}

/// Whether write `w`'s rows for `obj` come from the view or the frame (not
/// the object's own rows, a world surface's identity or the lookup).
fn follows(w: &CbWrite, obj: Option<&Object>, idx: Indices) -> bool {
    match &w.src {
        CbSource::Code { index, .. } => {
            !(obj.is_some_and(|o| o.rows.iter().any(|(i, _)| i == index))
                || (*index == idx.world && obj.is_none())
                || *index == idx.lookup)
        }
        _ => false,
    }
}

/// The registers `writes` put in constant buffer `slot`, `size` long, into
/// `out` (`tmp` scratch).
#[allow(clippy::too_many_arguments)]
fn fill_into(
    writes: &[CbWrite],
    slot: u32,
    size: u32,
    obj: Option<&Object>,
    code: &CodeValues,
    lookup: [f32; 4],
    idx: Indices,
    out: &mut Vec<[f32; 4]>,
    tmp: &mut Vec<[f32; 4]>,
) {
    out.clear();
    out.resize(size.max(1) as usize, [0.0; 4]);
    for w in writes.iter().filter(|w| w.buffer == slot) {
        tmp.clear();
        write_rows(w, obj, code, lookup, idx, tmp);
        let end = w.dest as usize + tmp.len();
        if out.len() < end {
            out.resize(end, [0.0; 4]);
        }
        out[w.dest as usize..end].copy_from_slice(tmp);
    }
}

/// `fill_into`, allocating.
fn fill(
    writes: &[CbWrite],
    slot: u32,
    size: u32,
    obj: Option<&Object>,
    code: &CodeValues,
    lookup: [f32; 4],
    idx: Indices,
) -> Vec<[f32; 4]> {
    let (mut out, mut tmp) = (Vec::new(), Vec::new());
    fill_into(writes, slot, size, obj, code, lookup, idx, &mut out, &mut tmp);
    out
}

struct Item {
    pipe: PipeKey,
    plan: usize,
    binds: PassBinds,
    draw: Draw,
    blended: bool,
    bounds: Option<(glam::Vec3, glam::Vec3)>,
    prepass: bool,
    /// Its draw's index (`Scene::draws`).
    di: usize,
    /// Which side of its triangles the game draws (`native_cull::Occluder`).
    front: i8,
    /// Whether it may hide what is behind it: an opaque, depth-writing,
    /// non-discarding world surface.
    occluder: bool,
}

/// Each draw's world bounds from its vertices, placed by its object.
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
                Some(o) => {
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
                None => Some((lo, hi)),
            }
        })
        .collect()
}

/// The occlusion buffer's size, and what counts as an occluder: a triangle
/// of at least this area (units are inches), the largest few thousand
/// (`render_h1`'s numbers).
pub const OCCLUSION_W: usize = 192;
pub const OCCLUSION_H: usize = 108;
const OCCLUDER_MIN_EXTENT: f32 = 96.0;
const OCCLUDER_MIN_AREA: f32 = 150.0;
const OCCLUDER_CAP: usize = 40000;
/// A draw must start this far (view depth) behind the occluder.
const OCCLUSION_MARGIN: f32 = 12.0;

/// `IW6_DBG_CENSUS=1` prints a census of the opaque scene list each frame.
/// `BO2MP_IW6_G0_SHARE=0`: a constant group for every draw's (material,
/// lightmap, probe) as before; the default shares the group of draws whose
/// constant buffers are the same buffers.
fn g0_share() -> bool {
    static ON: crate::switch::Switch = crate::switch::Switch::on("BO2MP_IW6_G0_SHARE");
    ON.get()
}

fn census_on() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var("IW6_DBG_CENSUS").is_ok_and(|v| v == "1"))
}

pub fn occlusion_threads() -> usize {
    static N: crate::switch::Number = crate::switch::Number::new("BO2MP_IW6_OCC_THREADS", f32::NAN);
    let n = N.get();
    if n.is_nan() {
        std::thread::available_parallelism().map_or(1, |c| c.get()).min(4)
    } else {
        (n as usize).max(1)
    }
}

/// `BO2MP_IW6_OCC_THREADS=n`: the threads the occlusion buffer is filled
/// by (default the cores, at most four; 1 is the single-threaded fill,
/// whose buffer the threads' merge reproduces exactly).
/// `BO2MP_IW6_OCCLUSION=0` turns the occlusion test off.
pub fn occlusion_on() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| !std::env::var("BO2MP_IW6_OCCLUSION").is_ok_and(|v| v.trim() == "0"))
}

/// The angle (radians) under which a draw is too small to show: about
/// 5, 3.5 and 1.6 pixels (diagonal) at 1080p on LOW, MEDIUM and HIGH; ULTRA
/// leaves nothing out (the 1.6 px limit removed 89 thin bright pixels from
/// the intermission view: ULTRA keeps the full picture, presets only take
/// away). `BO2MP_IW6_SMALL_CULL` sets it (0: off).
pub fn small_angle(preset: native_quality::Preset) -> Option<f32> {
    let default = match preset {
        native_quality::Preset::Low => 0.010,
        native_quality::Preset::Medium => 0.007,
        native_quality::Preset::High => 0.003,
        native_quality::Preset::Ultra => 0.0,
    };
    static SMALL: crate::switch::Number = crate::switch::Number::new("BO2MP_IW6_SMALL_CULL", f32::NAN);
    let a = Some(SMALL.get()).filter(|v| !v.is_nan()).unwrap_or(default);
    (a > 0.0).then_some(a)
}

/// Whether a box lies on the inner side of every plane (`n.p + w >= 0`).
pub fn box_inside(planes: &[glam::Vec4], lo: glam::Vec3, hi: glam::Vec3) -> bool {
    planes.iter().all(|p| {
        let n = p.truncate();
        let far = glam::Vec3::new(
            if n.x >= 0.0 { hi.x } else { lo.x },
            if n.y >= 0.0 { hi.y } else { lo.y },
            if n.z >= 0.0 { hi.z } else { lo.z },
        );
        n.dot(far) + p.w >= 0.0
    })
}

impl Renderer {
    pub fn new(gpu: Gpu, mut scene: Scene, depth_format: wgpu::TextureFormat) -> Self {
        let pipeline_cache = crate::pipecache::open(&gpu, &scene.name);
        let mesh_buffers = scene
            .meshes
            .iter()
            .map(|m| {
                let mut vb = m.vertex_bytes.clone();
                if vb.is_empty() {
                    vb.resize(64, 0);
                }
                let vb = vertex_buffer(&gpu, &vb);
                let mut idx: Vec<u8> = bytemuck::cast_slice(&m.indices).to_vec();
                while idx.is_empty() || !idx.len().is_multiple_of(4) {
                    idx.extend_from_slice(&[0, 0]);
                }
                let ib = wgpu::util::DeviceExt::create_buffer_init(
                    &gpu.device,
                    &wgpu::util::BufferInitDescriptor {
                        label: Some("iw6_ib"),
                        contents: &idx,
                        usage: wgpu::BufferUsages::INDEX,
                    },
                );
                let lb = (!m.layer_bytes.is_empty()).then(|| vertex_buffer(&gpu, &m.layer_bytes));
                (vb, ib, lb)
            })
            .collect();
        let mut bounds = draw_bounds(&scene);
        // A mover's draws are never culled (its bounds change as it moves).
        let moving: std::collections::HashSet<u32> =
            scene.movers.iter().flat_map(|m| m.objects.iter().copied()).collect();
        for (b, d) in bounds.iter_mut().zip(&scene.draws) {
            if d.object.is_some_and(|o| moving.contains(&o)) {
                *b = None;
            }
        }
        // The layer stream's layouts: what the declarations read, at the
        // stride the stream's runs give (`BO2MP_IW6_LAYER_STRIDE=0`: the
        // inferred stride).
        let mut layer_layouts = crate::layers::layouts(&scene.pool);
        if !std::env::var("BO2MP_IW6_LAYER_STRIDE").is_ok_and(|v| v.trim() == "0") {
            let changed = crate::layers::with_strides(&mut layer_layouts, &scene.layer_strides);
            if !changed.is_empty() {
                eprintln!("note: iw6 layer stream: strides read from the stream's runs differ from the declarations' for formats (format, inferred, read) {changed:?}");
            }
        }
        let volume = crate::model_lighting::Volume::build(&gpu, &scene.tiles);
        let base_coords = code_index("BASE_LIGHTING_COORDS");
        for o in &mut scene.objects {
            let Some(t) = o.tile else { continue };
            for (i, rows) in &mut o.rows {
                if *i == base_coords {
                    rows[0] = volume.coords(t);
                }
            }
        }
        let lighting_lookup = volume.lookup_scale();
        let mut code_textures = HashMap::new();
        code_textures.insert("MODEL_LIGHTING".to_string(), volume.tex);
        let align = u64::from(gpu.device.limits().min_uniform_buffer_offset_alignment);
        Self {
            gpu,
            scene,
            depth_format,
            code_textures,
            lighting_lookup,
            tex: HashMap::new(),
            bake: crate::bake::State::default(),
            pipelines: HashMap::new(),
            samplers: HashMap::new(),
            defaults: HashMap::new(),
            mesh_buffers,
            layer_layouts,
            plans: Vec::new(),
            plan_of: HashMap::new(),
            plans_of_object: HashMap::new(),
            material_cb: HashMap::new(),
            g0_cache: HashMap::new(),
            arena: Arena {
                pages: Vec::new(),
                align,
                label: "iw6_constants",
            },
            frame_arena: Arena {
                pages: Vec::new(),
                align,
                label: "iw6_frame_constants",
            },
            frame_blocks: Vec::new(),
            frame_block_of: HashMap::new(),
            groups: HashMap::new(),
            shared_groups: HashMap::new(),
            prepared: HashMap::new(),
            idx: Indices {
                wvp: code_index("TRANSPOSE_WORLD_VIEW_PROJECTION_MATRIX0"),
                inv_wv: code_index("INVERSE_TRANSPOSE_WORLD_VIEW_MATRIX0"),
                world: code_index("TRANSPOSE_WORLD_MATRIX0"),
                lookup: code_index("LIGHTING_LOOKUP_SCALE"),
            },
            report: Report::default(),
            bounds,
            spot_shadows: HashMap::new(),
            stats: Default::default(),
            last_update: None,
            frame: 0,
            rows: Vec::new(),
            lit_cache: Vec::new(),
            update_serial: 0,
            missing_seen: Vec::new(),
            lod_scale: 1.0,
            min_caster: None,
            draw_scale: 1.0,
            local_lights: local_lights_on(),
            spot_shadow_technique: crate::spot_shadow::on(),
            anisotropy: 16,
            cell_lists: Vec::new(),
            cell_view: None,
            occluders: None,
            occlusion_cache: None,
            occ_memo: Vec::new(),
            moved: 0,
            seen_scratch: Vec::new(),
            indirect: None,
            modules: Default::default(),
            pipeline_cache,
        }
    }

    fn pipeline(&mut self, tech: &Technique, pass_i: usize, kind: VertexKind, state: PassState, layer: u8, target: Target) -> PipeKey {
        let k: PipeKey = (key(&tech.name), pass_i, kind, state, layer, target);
        if !self.pipelines.contains_key(&k) {
            let id = self.pipelines.len() as u32;
            let made = self.make_pipeline(&tech.passes[pass_i], kind, state, layer, target).map(|mut p| {
                p.id = id;
                p
            });
            if let Err(e) = &made {
                self.report.failed.insert(format!("{} pass {pass_i} ({kind:?})", tech.name), e.clone());
            }
            self.pipelines.insert(k.clone(), made.ok());
        }
        k
    }

    /// A draw's second stream (`PipeKey`).
    fn second_stream(&self, d: &Draw, techset: &str) -> u8 {
        match (self.scene.meshes[d.mesh].kind, d.layer) {
            (_, None) => 0,
            (VertexKind::Model, Some(_)) => VERTEX_LIGHT,
            (VertexKind::World, Some(_)) => self.scene.pool.world_vert_format.get(techset).copied().unwrap_or(0),
        }
    }

    fn plan(&mut self, pipe: &PipeKey, variant: u8, light: Option<u16>, binds: &PassBinds) -> Result<usize, String> {
        if let Some(&p) = self.plan_of.get(&(pipe.clone(), variant, light)) {
            return Ok(p);
        }
        let p = self.pipelines[pipe].as_ref().ok_or("no pipeline")?;
        let mut sizes: BTreeMap<u32, u32> = p.regs.clone();
        for w in &binds.cbuffer {
            let rows = match &w.src {
                CbSource::Code { rows, .. } => u32::from(*rows),
                _ => 1,
            };
            let e = sizes.entry(w.buffer).or_insert(0);
            *e = (*e).max(u32::from(w.dest) + rows);
        }
        let writes: Vec<CbWrite> = binds
            .cbuffer
            .iter()
            .filter(|w| w.buffer != bind::MATERIAL)
            .cloned()
            .collect();
        let views = views_for(pipe.5);
        let b0 = u64::from(sizes.get(&bind::PER_PRIM).copied().unwrap_or(1).max(1)) * 16;
        let stride = b0.next_multiple_of(self.arena.align);
        let mut frame = BTreeMap::new();
        for (&slot, &n) in &sizes {
            if slot != bind::PER_PRIM && slot != bind::MATERIAL {
                frame.insert(slot, self.frame_block(slot, n.max(1), views, light, &writes));
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
            light,
            views,
            filled: false,
            follows: Vec::new(),
            moved: Vec::new(),
        });
        self.plan_of.insert((pipe.clone(), variant, light), self.plans.len() - 1);
        Ok(self.plans.len() - 1)
    }

    /// The frame block of `slot` laid out by `writes` (shared by every plan
    /// with the same layout, views and light).
    fn frame_block(&mut self, slot: u32, size: u32, views: u32, light: Option<u16>, writes: &[CbWrite]) -> usize {
        let mine: Vec<CbWrite> = writes.iter().filter(|w| w.buffer == slot).cloned().collect();
        let key = format!("{slot}/{size}/{views}/{light:?}/{mine:?}");
        if let Some(&b) = self.frame_block_of.get(&key) {
            return b;
        }
        let vstride = (u64::from(size) * 16).next_multiple_of(self.frame_arena.align);
        let alloc = self.frame_arena.alloc(&self.gpu, vstride * u64::from(views));
        self.frame_blocks.push(FrameBlock {
            at: Slice {
                size: u64::from(size) * 16,
                ..alloc
            },
            vstride,
            slot,
            size,
            writes: mine,
            light,
        });
        self.frame_block_of.insert(key, self.frame_blocks.len() - 1);
        self.frame_blocks.len() - 1
    }

    /// The light flags of the lists made now (`PrepKey`).
    fn light_flags(&self) -> u8 {
        u8::from(self.local_lights) | (u8::from(self.spot_shadow_technique) << 1)
    }

    fn prepare(&mut self, sun_shadow: bool, target: Target, variant: u8) -> Result<PrepKey, String> {
        let pk: PrepKey = (sun_shadow, target, variant, self.light_flags());
        if self.prepared.contains_key(&pk) {
            return Ok(pk);
        }
        let draws = self.scene.draws.clone();
        let mut items: Vec<Item> = Vec::new();
        let mut by_slot: BTreeMap<usize, usize> = BTreeMap::new();
        // Which draws the pre-pass drew: an opaque draw it left out (no
        // pre-pass technique in its set, or one that could not be made)
        // writes depth in its own pass (`PassState::needs_own_depth`).
        let mut prepassed = vec![false; draws.len()];
        let mut own_depth_n = 0usize;
        // The depth pre-pass first (the opaque surfaces' `zprepass`
        // techniques: their lit passes test less-equal without writing
        // depth), then every surface's own pass.
        for prepass in [true, false] {
        if prepass && target != Target::Scene {
            continue;
        }
        for (di, d) in draws.iter().enumerate() {
            let mat = self.scene.materials[d.material].clone();
            let slots = slots_for(d, &self.scene.lights, sun_shadow, target, (pk.3 & 1 != 0, pk.3 & 2 != 0));
            let Some((tslot, tech)) = pick(&self.scene.pool, &mat.techset, slots) else {
                if target == Target::Scene && !prepass {
                    *self
                        .report
                        .skipped
                        .entry(format!("no technique for slots {slots:?} in {}", mat.techset))
                        .or_default() += 1;
                }
                continue;
            };
            let tech = tech.clone();
            // IW6_DBG_SKIPVS=<word>: techniques whose vertex program's name
            // holds the word are left out (debug).
            if let Ok(w) = std::env::var("IW6_DBG_SKIPVS")
                && tech
                    .passes
                    .iter()
                    .any(|p| p.vertex_shader.as_deref().is_some_and(|v| v.contains(w.as_str())))
            {
                continue;
            }
            if std::env::var("IW6_DBG_SLOT").is_ok_and(|v| v.parse() == Ok(tslot)) && di % 40 == 0 {
                eprintln!(
                    "dbg slot {tslot}: {} ({}) vs {:?} ps {:?}",
                    tech.name,
                    mat.name,
                    tech.passes.first().and_then(|p| p.vertex_shader.clone()),
                    tech.passes.first().and_then(|p| p.pixel_shader.clone())
                );
            }
            // (The sky goes first, under everything, whatever its blend.)
            let opaque = d.list == List::Sky || mat.state(tslot, 0).blend_state().is_none();
            // A placed model that has a pre-pass technique but is left out of
            // the pre-pass (`model_prepass_on`): its lit pass writes depth.
            let own_depth = target == Target::Scene
                && opaque
                && d.list == List::Model
                && !model_prepass_on()
                && mat.state(tslot, 0).depth_test.is_some_and(|t| t != 0)
                && pick(&self.scene.pool, &mat.techset, &[slot::DEPTH_PREPASS]).is_some();
            let (tslot, tech) = if prepass {
                if !opaque || d.list == List::Sky || own_depth {
                    continue;
                }
                // (No pre-pass for a surface whose own pass does not build:
                // its depth would hide what is behind it.)
                let kind = self.scene.meshes[d.mesh].kind;
                let wvf = self.second_stream(d, &mat.techset);
                let main = self.pipeline(&tech, 0, kind, mat.state(tslot, 0), wvf, target);
                if self.pipelines.get(&main).is_none_or(Option::is_none) {
                    continue;
                }
                match pick(&self.scene.pool, &mat.techset, &[slot::DEPTH_PREPASS]) {
                    Some((s, t)) => (s, t.clone()),
                    None => continue,
                }
            } else {
                (tslot, tech)
            };
            if target != Target::Scene && !opaque {
                continue;
            }
            // A blended surface whose pictures aren't loaded is left out:
            // drawn with stand-ins it shows what isn't there.
            if target == Target::Scene && !opaque && !prepass {
                let missing = mat
                    .textures
                    .iter()
                    .filter(|t| t.2 == 2)
                    .any(|t| !t.3.is_empty() && self.image_tex(&t.3).is_none());
                // IW6_DBG_BLENDED=1: drawn with the stand-in picture (to see where they are).
                if missing && std::env::var_os("IW6_DBG_BLENDED").is_none() {
                    *self
                        .report
                        .skipped
                        .entry("blended surface with its colour map not loaded".into())
                        .or_default() += 1;
                    continue;
                }
            }
            let kind = self.scene.meshes[d.mesh].kind;
            let layer = self.second_stream(d, &mat.techset);
            if !prepass {
                *by_slot.entry(tslot).or_default() += 1;
            }
            for pi in 0..tech.passes.len() {
                let mut state = mat.state(tslot, pi);
                if own_depth && !prepass {
                    // (Nearest wins, writing depth: "less or equal" in the
                    // reversed depth, where the pre-pass would have had the
                    // lit pass test for equal.)
                    state.depth_test = Some(3);
                    state.depth_write = true;
                } else if pi == 0
                    && !prepass
                    && target == Target::Scene
                    && d.list != List::Sky
                    && crate::state::own_depth_on()
                    && state.needs_own_depth(opaque, prepassed[di])
                {
                    state = state.with_own_depth();
                    own_depth_n += 1;
                }
                let pipe = self.pipeline(&tech, pi, kind, state, layer, target);
                if self.pipelines.get(&pipe).is_none_or(Option::is_none) {
                    if target == Target::Scene {
                        *self
                            .report
                            .skipped
                            .entry(format!("pipeline not made: {}", tech.name))
                            .or_default() += 1;
                    }
                    continue;
                }
                let binds = bind::resolve(
                    &tech.passes[pi],
                    &MaterialView {
                        textures: &mat.textures,
                        constants: &mat.constants,
                    },
                );
                // (A spot or omni light's constants are per light: a plan
                // per light the pass is drawn with.)
                let light = if matches!(tslot, slot::LIT_SPOT | slot::LIT_SPOT_SHADOW | slot::LIT_OMNI) { d.light } else { None };
                let plan = self.plan(&pipe, variant, light, &binds)?;
                let p = &mut self.plans[plan];
                if !p.object_slot.contains_key(&d.object) {
                    p.object_slot.insert(d.object, p.objects.len() as u32);
                    p.objects.push(d.object);
                    if let Some(o) = d.object {
                        self.plans_of_object.entry(o).or_default().push(plan);
                    }
                }
                if prepass {
                    prepassed[di] = true;
                }
                let is_occluder = target == Target::Scene
                    && prepass
                    && opaque
                    && d.list == List::Opaque
                    && state.depth_write
                    && matches!(state.depth_test, Some(1..=3))
                    && self.pipelines.get(&pipe).and_then(Option::as_ref).is_some_and(|p| !p.discards);
                items.push(Item {
                    pipe,
                    plan,
                    binds,
                    draw: *d,
                    blended: !opaque,
                    bounds: self.bounds.get(di).copied().flatten(),
                    prepass,
                    di,
                    front: match (state.cull, kind) {
                        (2, VertexKind::World) | (3, VertexKind::Model) => 1,
                        (3, VertexKind::World) | (2, VertexKind::Model) => -1,
                        _ => 0,
                    },
                    occluder: is_occluder,
                });
            }
        }
        }
        if target == Target::Scene && self.occluders.is_none() {
            let o = if occlusion_on() { self.find_occluders(&items) } else { Default::default() };
            self.occluders = Some(o);
        }
        let mut used: Vec<usize> = items.iter().map(|i| i.plan).collect();
        used.sort_unstable();
        used.dedup();
        for &pi in &used {
            let p = &mut self.plans[pi];
            let need = (p.objects.len() as u64).max(1) * p.stride * u64::from(p.views);
            // (Instancing binds up to `MAX_INSTANCES` blocks from any
            // object's: padded so that never runs past the end. The
            // buffer is checked against the padded size: a plan that
            // gained objects in a later list (the lists made again after
            // a quality change) must be made again too, or an instanced
            // binding from its last objects runs past the buffer.)
            let pad = if crate::instancing::on() { u64::from(crate::instancing::MAX_INSTANCES) * p.stride } else { 0 };
            if p.per_object.as_ref().is_none_or(|b| b.size() < need + pad || (p.mirror.len() as u64) < need) {
                if p.per_object.is_some() {
                    let plan_of = &self.plan_of;
                    self.groups.retain(|k, _| plan_of.get(&(k.0.clone(), k.1, k.2)) != Some(&pi));
                    self.g0_cache.retain(|k, _| k.1 != pi);
                    // Other lists made earlier hold bind groups of the
                    // old buffer (no longer written): made again on use.
                    self.prepared.retain(|_, pr| !pr.plans.contains(&pi));
                }
                let usage = if crate::instancing::on() { wgpu::BufferUsages::STORAGE } else { wgpu::BufferUsages::empty() };
                p.per_object = Some(self.gpu.device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("iw6_objects"),
                    size: need + pad,
                    usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST | usage,
                    mapped_at_creation: false,
                }));
                p.mirror = vec![0; need as usize];
                p.filled = false;
            }
        }
        // The frame blocks to rewrite each frame, the frame's own first,
        // then each light's.
        let mut blocks: BTreeMap<Option<u16>, Vec<usize>> = BTreeMap::new();
        for &pi in &used {
            for &b in self.plans[pi].frame.values() {
                blocks.entry(self.frame_blocks[b].light).or_default().push(b);
            }
        }
        let blocks: Vec<(Option<u16>, Vec<usize>)> = blocks
            .into_iter()
            .map(|(l, mut v)| {
                v.sort_unstable();
                v.dedup();
                (l, v)
            })
            .collect();
        type SortKey = (u8, u8, u8, u32, u32, u32, u32, usize, u32, usize);
        let (mut opaque_ready, mut blended_ready): (Vec<(SortKey, Ready)>, Vec<Ready>) = (Vec::new(), Vec::new());
        for (order, it) in items.into_iter().enumerate() {
            let d = &it.draw;
            let light = self.plans[it.plan].light;
            let gk: GroupKey = (it.pipe.clone(), variant, light, d.material, d.lightmap, d.probe);
            if !self.groups.contains_key(&gk) {
                let (g, ids, ig) = self.make_groups(&it.pipe, it.plan, &it.binds, d)?;
                self.groups.insert(gk.clone(), (g, ids, ig));
            }
            let p = &self.plans[it.plan];
            let slot = p.object_slot[&d.object];
            let pl = self.pipelines[&it.pipe].as_ref().ok_or("no pipeline")?;
            // Each dynamic binding: the per-object block (view `v`'s run of
            // the objects after view 0's), or a frame block (view `v`'s).
            let (mut offsets, mut steps) = (Vec::new(), Vec::new());
            for &(_, s) in &pl.dynamic {
                if s == bind::PER_PRIM {
                    offsets.push((u64::from(slot) * p.stride) as u32);
                    steps.push((p.objects.len() as u64 * p.stride) as u32);
                } else {
                    offsets.push(0);
                    steps.push(p.frame.get(&s).map_or(0, |&b| self.frame_blocks[b].vstride as u32));
                }
            }
            let layer = match (pl.layered, d.layer) {
                (true, Some(l)) => Some((
                    u64::from(d.base_vertex as u32) * self.scene.meshes[d.mesh].kind.stride(),
                    u64::from(l),
                )),
                _ => None,
            };
            let (groups, group_ids, inst_group) = self.groups[&gk].clone();
            let inst = match (&pl.inst, inst_group) {
                (Some((pipeline, _, stride)), Some(group)) if *stride == p.stride => pl
                    .dynamic
                    .iter()
                    .position(|&(_, s)| s == bind::PER_PRIM)
                    .map(|b0| Inst {
                        pipeline: pipeline.clone(),
                        group,
                        b0: b0 as u8,
                        stride: p.stride as u32,
                    }),
                _ => None,
            };
            let r = Ready {
                inst,
                layer,
                pipeline: pl.pipeline.clone(),
                pipe_id: pl.id,
                groups,
                group_ids,
                offsets,
                steps,
                mesh: d.mesh,
                first: d.first_index,
                count: d.index_count,
                base: d.base_vertex,
                bounds: it.bounds,
                cells: match (d.cell, &self.scene.cells) {
                    (Some(item), Some(c)) => {
                        let list = c.cells_of(item);
                        let at = self.cell_lists.len() as u32;
                        self.cell_lists.extend_from_slice(list);
                        (at, list.len().min(usize::from(u16::MAX)) as u16)
                    }
                    _ => (0, 0),
                },
                di: it.di as u32,
                range: d.range.map(|[lo, hi]| {
                    let o = d.object.and_then(|o| self.scene.objects.get(o as usize)).map_or(glam::Vec3::ZERO, |o| o.world.w_axis.truncate());
                    (o, lo, hi)
                }),
            };
            if it.blended {
                blended_ready.push(r);
            } else {
                // The order drawn: state changes cut by sorting on the
                // pipeline, its groups, the mesh and the index range. Depth
                // decides between opaque surfaces whatever the order (the
                // pre-pass first; the lit passes test against it), but for
                // equal depths, where the material's sort key decides (as
                // the engine sorts its draw lists); decals keep their order
                // within it, and the sky goes first.
                let sky = u8::from(d.list != List::Sky);
                let sort_key = self.scene.materials[d.material].sort_key;
                let key: SortKey = match target {
                    Target::Scene if !it.prepass && d.list == List::Decal => (1, sky, sort_key, 0, 0, 0, 0, 0, 0, order),
                    Target::Scene if !it.prepass => (1, sky, sort_key, r.pipe_id, r.group_ids[1], r.group_ids[2], r.group_ids[0], r.mesh, r.first, order),
                    _ => (0, 0, 0, r.pipe_id, r.group_ids[1], r.group_ids[2], r.group_ids[0], r.mesh, r.first, order),
                };
                opaque_ready.push((key, r));
            }
        }
        // IW6_DBG_NOSORT=1: the scene's order (debug).
        if !std::env::var("IW6_DBG_NOSORT").is_ok_and(|v| v == "1") {
            opaque_ready.sort_by_key(|(k, _)| *k);
        }
        let prepass_n = if target == Target::Scene { opaque_ready.iter().take_while(|(k, _)| k.0 == 0).count() } else { 0 };
        let opaque_ready: Vec<Ready> = opaque_ready.into_iter().map(|(_, r)| r).collect();
        if target == Target::Scene {
            self.report.draws = opaque_ready.len() + blended_ready.len();
            self.report.by_slot = by_slot;
            self.report.own_depth = own_depth_n;
        }
        self.report.techniques = self.pipelines.values().filter(|p| p.is_some()).count();
        self.prepared.insert(
            pk,
            Prepared {
                prepass_n,
                opaque_cull: opaque_ready.iter().map(Cull::of).collect(),
                blended_cull: blended_ready.iter().map(Cull::of).collect(),
                opaque: opaque_ready,
                blended: blended_ready,
                plans: used,
                blocks,
            },
        );
        Ok(pk)
    }

    /// The big opaque triangles of the map's world surfaces, in world space
    /// (`BO2MP_IW6_OCCLUSION=0`: none): what `native_cull::Occlusion`
    /// rasterises each frame. Only draws with a depth pre-pass that cannot
    /// discard, no distance band (a coarser LOD is not a wall) and bounds
    /// (a mover is not one).
    fn find_occluders(&self, items: &[Item]) -> native_cull::OccluderSet {
        let mut seen = std::collections::HashSet::new();
        let mut tris = Vec::new();
        for it in items.iter().filter(|i| i.occluder) {
            if it.draw.range.is_some() || !seen.insert(it.di) {
                continue;
            }
            let Some((lo, hi)) = it.bounds else { continue };
            if (hi - lo).max_element() < std::env::var("IW6_OCC_EXTENT").ok().and_then(|v| v.trim().parse::<f32>().ok()).unwrap_or(OCCLUDER_MIN_EXTENT) {
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
        let n = tris.len();
        let tune = |k: &str, d: f32| std::env::var(k).ok().and_then(|v| v.trim().parse::<f32>().ok()).unwrap_or(d);
        let out = native_cull::largest(tris, tune("IW6_OCC_AREA", OCCLUDER_MIN_AREA), tune("IW6_OCC_CAP", OCCLUDER_CAP as f32) as usize);
        if std::env::var("IW6_DBG_OCC").is_ok() {
            eprintln!("dbg occ: {} occluder items, {} triangles, {} kept", seen.len(), n, out.len());
        }
        eprintln!(
            "note: iw6 occlusion: {} occluder triangles kept of {n} opaque ones (BO2MP_IW6_OCCLUSION=0 off)",
            out.len()
        );
        native_cull::OccluderSet::new(out)
    }

    /// `IW6_DBG_CENSUS=1`: what the opaque scene list draws this frame, by
    /// kind, distance and material (a debug print).
    fn census(&self, seen: &[u32], list: &[Ready], eye: glam::Vec3, prepass_n: usize) {
        use std::collections::BTreeMap;
        let mut by_list: BTreeMap<String, (u32, u64)> = BTreeMap::new();
        let mut by_dist: BTreeMap<u32, (u32, u64)> = BTreeMap::new();
        let mut by_mat: BTreeMap<String, (u32, u64)> = BTreeMap::new();
        let mut by_tri: BTreeMap<u32, (u32, u64)> = BTreeMap::new();
        let mut unique = std::collections::HashSet::new();
        for &i in seen {
            let r = &list[i as usize];
            let d = &self.scene.draws[r.di as usize];
            unique.insert(r.di);
            let tris = u64::from(r.count / 3);
            let kind = format!("{:?}{}{}", d.list, if d.range.is_some() { " banded" } else { "" }, if d.light.is_some() { " light" } else { "" });
            let e = by_list.entry(kind).or_default();
            e.0 += 1;
            e.1 += tris;
            let dist = r.bounds.map_or(0.0, |(lo, hi)| eye.distance((lo + hi) * 0.5));
            let e = by_dist.entry((dist / 500.0) as u32).or_default();
            e.0 += 1;
            e.1 += tris;
            let e = by_mat.entry(self.scene.materials[d.material].name.clone()).or_default();
            e.0 += 1;
            e.1 += tris;
            let b = match r.count / 3 {
                0..=15 => 0,
                16..=63 => 1,
                64..=255 => 2,
                256..=1023 => 3,
                _ => 4,
            };
            let e = by_tri.entry(b).or_default();
            e.0 += 1;
            e.1 += tris;
        }
        // State changes along the order drawn, and how many entries repeat
        // another's mesh range (the instancing candidates), apart for the
        // pre-pass half and the lit half.
        for (label, pre) in [("pre-pass", true), ("lit", false)] {
            let part: Vec<&Ready> = seen.iter().filter(|&&i| ((i as usize) < prepass_n) == pre).map(|&i| &list[i as usize]).collect();
            let mut states = std::collections::HashSet::new();
            let mut keys: std::collections::HashMap<(u32, [u32; 3], usize, u32, u32, i32), (u32, bool)> = std::collections::HashMap::new();
            let (mut dp, mut d1, mut d2, mut d0, mut dm, mut dg) = (0, 0, 0, 0, 0, 0);
            let mut prev: Option<&Ready> = None;
            let mut no_inst = 0;
            for r in &part {
                states.insert((r.pipe_id, r.group_ids[1], r.group_ids[2]));
                let e = keys.entry((r.pipe_id, r.group_ids, r.mesh, r.first, r.count, r.base)).or_insert((0, r.inst.is_some()));
                e.0 += 1;
                if r.inst.is_none() {
                    no_inst += 1;
                }
                if let Some(p) = prev {
                    dp += u32::from(p.pipe_id != r.pipe_id);
                    d1 += u32::from(p.group_ids[1] != r.group_ids[1]);
                    d2 += u32::from(p.group_ids[2] != r.group_ids[2]);
                    d0 += u32::from(p.group_ids[0] != r.group_ids[0] || p.offsets != r.offsets);
                    dm += u32::from(p.mesh != r.mesh);
                    dg += u32::from(p.group_ids[0] != r.group_ids[0]);
                }
                prev = Some(r);
            }
            let repeats: u32 = keys.values().map(|v| v.0 - 1).sum();
            let rep_inst: u32 = keys.values().filter(|v| v.1).map(|v| v.0 - 1).sum();
            let tiny = part.iter().filter(|r| r.count / 3 < 16).count();
            let meshes: std::collections::HashSet<_> = part.iter().map(|r| r.mesh).collect();
            eprintln!(
                "census {label}: {} entries ({tiny} under 16 tris, {no_inst} without an instanced pipeline), {} (pipeline,tex,sampler) states, {} distinct draw keys, {repeats} repeats of a key ({rep_inst} instanceable), {} meshes; changes along the order: pipeline {dp}, tex {d1}, sampler {d2}, constants {d0} (group changes {dg}), mesh {dm}",
                part.len(),
                states.len(),
                keys.len(),
                meshes.len()
            );
        }
        eprintln!("census: {} entries, {} distinct draws", seen.len(), unique.len());
        eprintln!("census by list: {by_list:?}");
        eprintln!("census by distance (500 units): {by_dist:?}");
        eprintln!("census by triangles per draw (<16, <64, <256, <1024, more): {by_tri:?}");
        let mut m: Vec<_> = by_mat.into_iter().collect();
        m.sort_by_key(|(_, (_, t))| std::cmp::Reverse(*t));
        eprintln!("census top materials: {:?}", &m[..m.len().min(10)]);
    }

    /// The camera's occlusion buffer (None: off, or the map has no
    /// occluders yet). Kept while the view stands still.
    pub fn occlusion(&mut self, camera: &render_h1::code::Camera) -> Option<std::sync::Arc<native_cull::Occlusion>> {
        let tris = self.occluders.as_ref().filter(|t| !t.is_empty())?;
        if let Some(o) = &self.occlusion_cache
            && o.is_for(&camera.view_projection, camera.eye)
        {
            return Some(o.clone());
        }
        // (The buffer of the view before is reused once its frame is done with it.)
        if let Some(arc) = self.occlusion_cache.as_mut()
            && let Some(o) = std::sync::Arc::get_mut(arc)
        {
            o.rebuild_from_set_threads(camera.view_projection, camera.eye, tris, OCCLUSION_W, OCCLUSION_H, OCCLUSION_MARGIN, occlusion_threads());
            return Some(arc.clone());
        }
        let built = std::sync::Arc::new(native_cull::Occlusion::from_set(
            camera.view_projection,
            camera.eye,
            tris,
            OCCLUSION_W,
            OCCLUSION_H,
            OCCLUSION_MARGIN,
        ));
        static DBG_OCC: crate::switch::Switch = crate::switch::Switch::off("IW6_DBG_OCC");
        if DBG_OCC.get() {
            eprintln!("dbg occ: rasterised {} triangles, {} pixel writes", built.tris, built.pixels);
        }
        self.occlusion_cache = Some(built.clone());
        Some(built)
    }

    /// Makes every list a frame draws (pipelines, groups, constant blocks)
    /// now, so the first view of anything never waits on them.
    pub fn warm(&mut self, sun_shadow: bool, spot_shadow: bool) -> Result<(), String> {
        self.bake_begin();
        self.prepare(sun_shadow, Target::Scene, 0)?;
        if sun_shadow {
            self.prepare(false, Target::Shadow, 1)?;
        }
        if spot_shadow {
            self.prepare(false, Target::SpotShadow, 3)?;
        }
        self.bake_end();
        Ok(())
    }

    /// bo2mp (map bake): at the first warm, the map's baked pictures are
    /// read and uploaded now (the warm then finds them made), or, with no
    /// usable file, the warm's pictures are written to a new one.
    fn bake_begin(&mut self) {
        if !matches!(self.bake, crate::bake::State::NotBegun) {
            return;
        }
        self.bake = crate::bake::State::Over;
        let zones: Vec<std::path::PathBuf> = self.scene.zones.iter().map(|z| z.path.clone()).collect();
        let Some(b) = crate::bake::Pictures::new(&zones, self.scene.images.max_size) else { return };
        let t = std::time::Instant::now();
        if let Some(reader) = b.open() {
            let (got, bytes, err) = crate::bake::upload_all(&self.gpu, reader);
            let n = got.len();
            for (name, tex) in got {
                self.tex.entry(name).or_insert(Some(tex));
            }
            eprintln!(
                "note: iw6 bake: {n} pictures ({} MB) from {} in {:.1}s",
                bytes >> 20,
                b.dir().display(),
                t.elapsed().as_secs_f32()
            );
            if let Some(e) = err {
                eprintln!("note: iw6 bake: the pictures file stopped reading ({e}); the rest load the old way and it is made again");
                b.remove();
            }
            crate::loadtime::stage("bake pictures read");
            return;
        }
        let why = b.why();
        match b.writer() {
            Some(w) => {
                eprintln!("note: iw6 bake: writing the pictures to {} ({why})", b.dir().display());
                self.bake = crate::bake::State::Writing(b, w, 0);
            }
            None => eprintln!("note: iw6 bake: could not start a pictures file in {}", b.dir().display()),
        }
    }

    /// The first warm is over: the pictures file it wrote is the map's.
    fn bake_end(&mut self) {
        if let crate::bake::State::Writing(b, w, n) = std::mem::replace(&mut self.bake, crate::bake::State::Over) {
            w.finish(true, b.manifest(n));
        }
    }

    /// The `iw6 fps:` numbers of a prepared list: (draws, distinct
    /// pipelines, distinct bind group sets, plans, frame blocks).
    pub fn list_counts(&self, sun_shadow: bool, target: Target, variant: u8) -> Option<(usize, usize, usize, usize, usize)> {
        let p = self.prepared.get(&(sun_shadow, target, variant, self.light_flags()))?;
        let all = || p.opaque.iter().chain(&p.blended);
        let mut pipes: Vec<u32> = all().map(|r| r.pipe_id).collect();
        pipes.sort_unstable();
        pipes.dedup();
        let mut groups: Vec<u32> = all().map(|r| r.group_ids[0]).collect();
        groups.sort_unstable();
        groups.dedup();
        let blocks: usize = p.blocks.iter().map(|(_, v)| v.len()).sum();
        Some((p.opaque.len() + p.blended.len(), pipes.len(), groups.len(), p.plans.len(), blocks))
    }

    /// A draw's three bind groups and their ids: the constant buffers' its
    /// own; the textures' and samplers' shared by every draw binding the same
    /// pictures and samplers through the same layout entries (layouts with
    /// equal entries are one in wgpu), so a run of draws keeps them bound.
    #[allow(clippy::type_complexity)]
    fn make_groups(&mut self, pk: &PipeKey, plan: usize, binds: &PassBinds, d: &Draw) -> Result<([wgpu::BindGroup; 3], [u32; 3], Option<wgpu::BindGroup>), String> {
        let (cb_entries, tex_entries, samp_entries) = {
            let p = self.pipelines[pk].as_ref().ok_or("no pipeline")?;
            (p.layout.cbuffers.clone(), p.layout.textures.clone(), p.layout.samplers.clone())
        };
        let cb_entries_slots: Vec<u32> = cb_entries.iter().map(|c| c.1).collect();
        enum Cb {
            Arena(Slice),
            Frame(Slice),
            Objects(u64),
        }
        let mut cb_bind = Vec::new();
        for &(binding, slot, _regs) in &cb_entries {
            let at = match slot {
                bind::PER_PRIM => Cb::Objects(
                    u64::from(self.plans[plan].sizes.get(&slot).copied().unwrap_or(1).max(1)) * 16,
                ),
                bind::MATERIAL => {
                    let n = self.plans[plan].sizes.get(&slot).copied().unwrap_or(1).max(1);
                    let k = (plan, d.material, slot);
                    let at = match self.material_cb.get(&k) {
                        Some(&at) => at,
                        None => {
                            let at = self.arena.alloc(&self.gpu, u64::from(n) * 16);
                            let c = fill(&binds.cbuffer, slot, n, None, &CodeValues::default(), self.lighting_lookup, self.idx);
                            self.arena.write(at, 0, &c);
                            self.material_cb.insert(k, at);
                            at
                        }
                    };
                    Cb::Arena(at)
                }
                _ => match self.plans[plan].frame.get(&slot) {
                    Some(&b) => Cb::Frame(self.frame_blocks[b].at),
                    None => {
                        let (views, light) = (self.plans[plan].views, self.plans[plan].light);
                        let b = self.frame_block(slot, 1, views, light, &[]);
                        self.plans[plan].frame.insert(slot, b);
                        Cb::Frame(self.frame_blocks[b].at)
                    }
                },
            };
            cb_bind.push((binding, at));
        }
        let mut tex_views: Vec<(u32, wgpu::TextureView)> = Vec::new();
        let mut tex_key = String::new();
        for &(binding, slot, dim, kind, is_ps) in &tex_entries {
            let src = if is_ps { binds.ps.textures.get(&slot) } else { binds.vs.textures.get(&slot) };
            let vdim = match dim {
                dxbc_sm5::TextureDim::Cube => wgpu::TextureViewDimension::Cube,
                dxbc_sm5::TextureDim::D3 => wgpu::TextureViewDimension::D3,
                dxbc_sm5::TextureDim::D2Array => wgpu::TextureViewDimension::D2Array,
                _ => wgpu::TextureViewDimension::D2,
            };
            let mut view = None;
            tex_key += &format!("{binding}/{is_ps}/{dim:?}/{kind:?}=");
            if let Some(TexSource::Code(c)) = src
                && let Some(t) = self.code_textures.get(code_texture_name(*c))
                && t.dim == vdim
                && (kind == TextureKind::Depth) == t.texture.format().has_depth_aspect()
            {
                view = Some(t.view.clone());
                tex_key += &format!("code:{};", code_texture_name(*c));
            }
            let name: Option<String> = match src {
                Some(TexSource::Material { image, .. }) => image.clone(),
                Some(TexSource::LightmapPrimary) => self.scene.lightmaps.get(d.lightmap as usize).and_then(|l| l.0.clone()),
                Some(TexSource::LightmapSecondary) => self.scene.lightmaps.get(d.lightmap as usize).and_then(|l| l.1.clone()),
                Some(TexSource::ReflectionProbe) => self.scene.probes.get(d.probe as usize).cloned().flatten(),
                Some(TexSource::Code(tex::REFLECTION_PROBE)) => self.scene.probes.get(d.probe as usize).cloned().flatten(),
                _ => None,
            };
            if view.is_none()
                && kind != TextureKind::Depth
                && let Some(n) = name
                && let Some(t) = self.image_tex(&n)
                && t.dim == vdim
            {
                view = Some(t.view.clone());
                tex_key += &format!("img:{};", key(&n));
            }
            let view = match view {
                Some(v) => v,
                None => {
                    let what = match src {
                        Some(TexSource::Material { semantic: 5, .. }) => "normal",
                        // A specular map that isn't loaded: black (no
                        // reflection; grey under the materials' fresnel
                        // ramps, up to 10 at grazing angles, turned
                        // surfaces into mirrors of their probe).
                        Some(TexSource::Material { semantic: 8, .. }) => "black",
                        Some(TexSource::Code(tex::IDENTITY_NORMAL_MAP)) => "normal",
                        Some(TexSource::Code(tex::WHITE)) => "white",
                        Some(TexSource::Material { .. }) => {
                            // A picture that is not there must not make a
                            // whole prop vanish: an opaque material's
                            // fallback is opaque grey; only a blended or
                            // cut-out material (whose picture's alpha is
                            // its shape) keeps alpha 0. `BO2MP_IW6_GREY_ALPHA0=1`
                            // returns to alpha 0 everywhere.
                            let m = &self.scene.materials[d.material];
                            let n = format!("{} {}", m.name, pk.0).to_ascii_lowercase();
                            let cut = ["atest", "alpha", "cutout", "leaf", "foliage", "fence", "decal", "trans", "glass", "fx_", "effect"]
                                .iter()
                                .any(|k| n.contains(k));
                            if pk.3.blend[2] != 0 || cut || std::env::var("BO2MP_IW6_GREY_ALPHA0").is_ok_and(|v| v.trim() == "1") {
                                self.report.fallback_groups[1] += 1;
                                "grey"
                            } else {
                                self.report.fallback_groups[0] += 1;
                                "grey_opaque"
                            }
                        }
                        Some(TexSource::LightmapPrimary) => "white",
                        _ => "black",
                    };
                    tex_key += &format!("def:{what};");
                    self.default_tex(what, vdim, kind).view.clone()
                }
            };
            tex_views.push((binding, view));
        }
        let mut samps: Vec<(u32, wgpu::Sampler)> = Vec::new();
        let mut samp_key = String::new();
        for &(binding, slot, is_ps) in &samp_entries {
            let src = if is_ps { binds.ps.samplers.get(&slot) } else { binds.vs.samplers.get(&slot) };
            let s = src.copied().unwrap_or(SampSource::Default);
            let comparison = self.pipelines[pk].as_ref().is_some_and(|p| p.layout.comparison_bindings.contains(&binding));
            samp_key += &format!("{binding}/{is_ps}/{s:?}/{comparison};");
            samps.push((binding, self.sampler(s, comparison)));
        }
        let p = self.pipelines[pk].as_ref().ok_or("no pipeline")?;
        let objects = self.plans[plan].per_object.as_ref().ok_or("no object buffer")?;
        let dev = &self.gpu.device;
        let uses_material = cb_entries_slots.contains(&bind::MATERIAL);
        let g0_key = (pk.clone(), plan, if uses_material { d.material } else { usize::MAX });
        let cached = g0_share().then(|| self.g0_cache.get(&g0_key).cloned()).flatten();
        let (g0, id0, ig) = match cached {
            Some(c) => c,
            None => {
            let g0 = dev.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("iw6_cb"),
                layout: &p.layout.groups[0],
                entries: &cb_bind
                    .iter()
                    .map(|(b, at)| wgpu::BindGroupEntry {
                        binding: *b,
                        resource: match at {
                            Cb::Arena(at) => self.arena.binding(*at),
                            Cb::Frame(at) => self.frame_arena.binding(*at),
                            Cb::Objects(size) => wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                                buffer: objects,
                                offset: 0,
                                size: std::num::NonZeroU64::new(*size),
                            }),
                        },
                    })
                    .collect::<Vec<_>>(),
            });
            // Its instanced form: the per-primitive blocks as a storage array
            // of up to `MAX_INSTANCES` blocks from the bound offset.
            let ig = p.inst.as_ref().map(|(_, layout, stride)| {
                dev.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("iw6_cb_inst"),
                    layout: &layout.groups[0],
                    entries: &cb_bind
                        .iter()
                        .map(|(b, at)| wgpu::BindGroupEntry {
                            binding: *b,
                            resource: match at {
                                Cb::Arena(at) => self.arena.binding(*at),
                                Cb::Frame(at) => self.frame_arena.binding(*at),
                                Cb::Objects(_) => wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                                    buffer: objects,
                                    offset: 0,
                                    size: if crate::instancing::block_index() { None } else { std::num::NonZeroU64::new(stride * u64::from(crate::instancing::MAX_INSTANCES)) },
                                }),
                            },
                        })
                        .collect::<Vec<_>>(),
                })
            });
            let id0 = if g0_share() { self.g0_cache.len() as u32 } else { self.groups.len() as u32 };
            if g0_share() {
                self.g0_cache.insert(g0_key, (g0.clone(), id0, ig.clone()));
            }
            (g0, id0, ig)
            }
        };
        // IW6_DBG_NOSHARE=1: every draw its own texture and sampler groups.
        let share = !std::env::var("IW6_DBG_NOSHARE").is_ok_and(|v| v == "1");
        let n = self.shared_groups.len() as u32;
        let (g1, id1) = match self.shared_groups.get(&tex_key).filter(|_| share) {
            Some(g) => g.clone(),
            None => {
                let g = dev.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("iw6_tex"),
                    layout: &p.layout.groups[1],
                    entries: &tex_views
                        .iter()
                        .map(|(b, v)| wgpu::BindGroupEntry {
                            binding: *b,
                            resource: wgpu::BindingResource::TextureView(v),
                        })
                        .collect::<Vec<_>>(),
                });
                self.shared_groups.insert(if share { tex_key } else { format!("{n}") }, (g.clone(), n));
                (g, n)
            }
        };
        let n = self.shared_groups.len() as u32;
        let samp_key = format!("s:{samp_key}");
        let (g2, id2) = match self.shared_groups.get(&samp_key).filter(|_| share) {
            Some(g) => g.clone(),
            None => {
                let g = dev.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("iw6_samp"),
                    layout: &p.layout.groups[2],
                    entries: &samps
                        .iter()
                        .map(|(b, s)| wgpu::BindGroupEntry {
                            binding: *b,
                            resource: wgpu::BindingResource::Sampler(s),
                        })
                        .collect::<Vec<_>>(),
                });
                self.shared_groups.insert(if share { samp_key } else { format!("{n}") }, (g.clone(), n));
                (g, n)
            }
        };
        Ok(([g0, g1, g2], [id0, id1, id2], ig))
    }

    /// Adds a draw of its own mesh (world vertices, `vertices` x 44 bytes and
    /// `indices` u16 indices, both rewritable with `write_dynamic`) with the
    /// lighting `(lightmap, probe, sun, light)` and the material: a ring of marks (`decal`). Returns the mesh. The lists are
    /// made again on next use (`invalidate_lists`).
    pub fn add_dynamic_draw(&mut self, lighting: (u8, u8, bool, Option<u16>), material: usize, vertices: usize, indices: usize) -> usize {
        let dev = &self.gpu.device;
        let vb = dev.create_buffer(&wgpu::BufferDescriptor {
            label: Some("iw6_dyn_vb"),
            size: (vertices * VertexKind::World.stride() as usize).max(64) as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let ib = dev.create_buffer(&wgpu::BufferDescriptor {
            label: Some("iw6_dyn_ib"),
            size: ((indices * 2).max(4)).next_multiple_of(4) as u64,
            usage: wgpu::BufferUsages::INDEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        // (Zeroed by the first write of the caller; a fresh buffer reads zero.)
        let mesh = self.scene.meshes.len();
        self.scene.meshes.push(crate::scene::MeshData {
            kind: VertexKind::World,
            vertex_bytes: Vec::new(),
            indices: Vec::new(),
            layer_bytes: Vec::new(),
        });
        self.mesh_buffers.push((vb, ib, None));
        self.scene.draws.push(Draw {
            mesh,
            object: None,
            material,
            lightmap: lighting.0,
            probe: lighting.1,
            first_index: 0,
            index_count: indices as u32,
            base_vertex: 0,
            layer: None,
            sun: lighting.2,
            light: lighting.3,
            list: List::Decal,
            range: None,
            cell: None,
            casts: [false, false],
        });
        self.bounds.push(None);
        mesh
    }

    /// Rewrites part of a dynamic mesh (`add_dynamic_draw`): `vertex_bytes`
    /// at byte `vertex_at`, `indices` from index `index_at`.
    pub fn write_dynamic(&self, mesh: usize, vertex_at: u64, vertex_bytes: &[u8], index_at: u64, indices: &[u16]) {
        let Some(mb) = self.mesh_buffers.get(mesh) else { return };
        if !vertex_bytes.is_empty() {
            self.gpu.queue.write_buffer(&mb.0, vertex_at, vertex_bytes);
        }
        if !indices.is_empty() {
            // (Whole even-sized runs at even places: a write is a multiple of four bytes.)
            debug_assert!(index_at.is_multiple_of(2) && indices.len().is_multiple_of(2));
            self.gpu.queue.write_buffer(&mb.1, index_at * 2, bytemuck::cast_slice(indices));
        }
    }

    /// Drops every list and bind group (made again on next use): after a
    /// code texture changed (a shadow map re-made at another size) or the
    /// sampler settings.
    pub fn invalidate_lists(&mut self) {
        self.prepared.clear();
        self.cell_lists.clear();
        self.moved += 1;
        self.groups.clear();
        self.shared_groups.clear();
        self.samplers.clear();
        self.last_update = None;
    }

    /// Writes the driver's pipeline cache to disk if it grew (after the lists are made).
    pub fn save_pipeline_cache(&self) {
        if let Some(c) = &self.pipeline_cache {
            crate::pipecache::save(c);
        }
    }

    /// A new frame: every list's constants are written again at its first
    /// record (the host calls it before recording).
    pub fn begin_frame(&mut self) {
        self.frame += 1;
    }

    /// Walks the portals for this frame's view: `eye`, the view's forward
    /// direction, its near distance and side planes (`n . p + w >= 0`
    /// inside). Without cells nothing is culled by them.
    pub fn update_cells(&mut self, eye: glam::Vec3, forward: glam::Vec3, near: f32, sides: &[glam::Vec4]) {
        let Some(c) = &self.scene.cells else { return };
        let v = self.cell_view.get_or_insert_with(|| crate::cells::CellView::new(c.cell_count()));
        let f = forward.normalize_or_zero();
        let mut frustum = [[0.0f32; 4]; 6];
        frustum[0] = [f.x, f.y, f.z, -f.dot(eye) - near];
        // (Depth is infinite: a far plane past the world.)
        frustum[1] = [-f.x, -f.y, -f.z, f.dot(eye) + 1.0e7];
        let n = sides.len().min(4);
        for (k, p) in sides.iter().take(4).enumerate() {
            frustum[2 + k] = p.to_array();
        }
        v.update(c, eye.to_array(), f.to_array(), &frustum[..2 + n]);
    }

    /// Places object `o` at `world` (its constants follow next frame).
    pub fn set_object_world(&mut self, o: u32, world: glam::Mat4) {
        let Some(obj) = self.scene.objects.get_mut(o as usize) else { return };
        obj.world = world;
        self.moved += 1;
        let rows: Vec<[f32; 4]> = (0..4).map(|r| world.row(r).to_array()).collect();
        let wm = self.idx.world;
        match obj.rows.iter_mut().find(|(i, _)| *i == wm) {
            Some((_, v)) => *v = rows,
            None => obj.rows.push((wm, rows)),
        }
        for &pi in self.plans_of_object.get(&o).map_or(&[][..], Vec::as_slice) {
            let p = &mut self.plans[pi];
            if let Some(&s) = p.object_slot.get(&Some(o))
                && !p.moved.contains(&s)
            {
                p.moved.push(s);
            }
        }
        self.last_update = None;
    }

    /// Writes view `view`'s constants of list `key` for `code`: the frame
    /// blocks (the frame's and each light's), every object's block once
    /// (and again when it moves), and each frame only the per-object rows
    /// that follow the view. Unchanged bytes are not uploaded.
    fn update(&mut self, key: &PrepKey, view: u32, code: &CodeValues) {
        let Some(prepared) = self.prepared.get(key) else { return };
        let (lookup, idx) = (self.lighting_lookup, self.idx);
        let mut rows = std::mem::take(&mut self.rows);
        let mut tmp = Vec::new();
        let mut lit = std::mem::take(&mut self.lit_cache);
        self.update_serial += 1;
        let serial = self.update_serial;
        // (A light's values are made once per update; `BO2MP_IW6_LIGHT_CACHE=0`:
        // made again for every block and plan that uses it, as before.)
        static LIGHT_CACHE: crate::switch::Switch = crate::switch::Switch::on("BO2MP_IW6_LIGHT_CACHE");
        let cache_on = LIGHT_CACHE.get();
        let light_at = |light: Option<u16>, lit: &mut Vec<(u16, u64, CodeValues)>| -> Option<usize> {
            let l = light?;
            let pl = self.scene.lights.get(usize::from(l))?;
            let at = match lit.iter().position(|e| e.0 == l) {
                Some(i) => i,
                None => {
                    lit.push((l, 0, CodeValues::default()));
                    lit.len() - 1
                }
            };
            if !cache_on || lit[at].1 != serial {
                lit[at].1 = serial;
                // (The light's own constants stay between updates; the frame's
                // are written over them. `BO2MP_IW6_LIGHT_CACHE=0` copies afresh.)
                if cache_on { lit[at].2.overlay_from(code) } else { lit[at].2.copy_from(code) }
                crate::local_light::write(pl, code.eye, self.scene.lightmap_width, self.spot_shadows.get(&l), &mut lit[at].2);
            }
            Some(at)
        };
        // Frame blocks: the frame's values, or a light's over them.
        for (light, blocks) in &prepared.blocks {
            let at = light_at(*light, &mut lit);
            let c = match at {
                Some(i) => &lit[i].2,
                None => code,
            };
            for &b in blocks {
                let fb = &self.frame_blocks[b];
                fill_into(&fb.writes, fb.slot, fb.size, None, c, lookup, idx, &mut rows, &mut tmp);
                self.frame_arena.write(fb.at, u64::from(view) * fb.vstride, &rows);
            }
        }
        // Per-object blocks.
        for &pi in &prepared.plans {
            let plan = &mut self.plans[pi];
            let (Some(&n), Some(buf)) = (plan.sizes.get(&bind::PER_PRIM), plan.per_object.as_ref()) else { continue };
            let at = light_at(plan.light, &mut lit);
            let c = match at {
                Some(i) => &lit[i].2,
                None => code,
            };
            let stride = plan.stride as usize;
            let count = plan.objects.len();
            let objects = &self.scene.objects;
            let obj_of = |o: Option<u32>| o.and_then(|o| objects.get(o as usize));
            // Every object's block, every view (once, and for moved objects).
            let refill: Vec<u32> = if plan.filled { std::mem::take(&mut plan.moved) } else { (0..count as u32).collect() };
            if !plan.filled {
                plan.follows.clear();
            }
            let mut dirty: Option<(usize, usize)> = None;
            for &s in &refill {
                let obj = obj_of(plan.objects[s as usize]);
                fill_into(&plan.writes, bind::PER_PRIM, n, obj, c, lookup, idx, &mut rows, &mut tmp);
                let bytes: &[u8] = bytemuck::cast_slice(&rows);
                let len = bytes.len().min(stride);
                for v in 0..plan.views as usize {
                    let at = (v * count + s as usize) * stride;
                    plan.mirror[at..at + len].copy_from_slice(&bytes[..len]);
                }
                if !plan.filled {
                    for (wi, w) in plan.writes.iter().enumerate() {
                        if w.buffer == bind::PER_PRIM && follows(w, obj, idx) {
                            plan.follows.push((s, wi as u16));
                        }
                    }
                }
            }
            if !refill.is_empty() {
                dirty = Some((0, count * stride * plan.views as usize));
            }
            plan.filled = true;
            // The rows that follow the view (or the frame), this view only.
            let base = view as usize * count * stride;
            for &(s, wi) in &plan.follows {
                let w = &plan.writes[wi as usize];
                tmp.clear();
                write_rows(w, obj_of(plan.objects[s as usize]), c, lookup, idx, &mut tmp);
                let at = base + s as usize * stride + usize::from(w.dest) * 16;
                let bytes: &[u8] = bytemuck::cast_slice(&tmp);
                let len = bytes.len().min((base + (s as usize + 1) * stride).saturating_sub(at));
                let dst = &mut plan.mirror[at..at + len];
                if *dst != bytes[..len] {
                    dst.copy_from_slice(&bytes[..len]);
                    dirty = Some(match dirty {
                        Some((a, e)) => (a.min(at), e.max(at + len)),
                        None => (at, at + len),
                    });
                }
            }
            if let Some((a, e)) = dirty {
                let (a, e) = (a / 4 * 4, e.next_multiple_of(4).min(plan.mirror.len()));
                self.gpu.queue.write_buffer(buf, a as u64, &plan.mirror[a..e]);
                self.stats.counts.bytes += (e - a) as u64;
            }
        }
        self.stats.counts.bytes += self.frame_arena.flush(&self.gpu.queue);
        self.stats.counts.bytes += self.arena.flush(&self.gpu.queue);
        self.rows = rows;
        self.lit_cache = lit;
    }

    /// Records `frame`'s part of the scene into `encoder`, onto `targets`,
    /// as view `frame.view` of its list (each view's constants are its own,
    /// so every view of a frame goes into one encoder and one submission).
    pub fn record(&mut self, encoder: &mut wgpu::CommandEncoder, targets: &Targets, frame: &Frame) -> Result<(), String> {
        let profile = crate::profile::on();
        let t0 = profile.then(std::time::Instant::now);
        self.stats = Default::default();
        let key = self.prepare(frame.sun_shadow, frame.target, frame.variant)?;
        let t1 = profile.then(std::time::Instant::now);
        let view = frame.view.min(views_for(frame.target) - 1);
        // (The blended part after the opaque one in the same frame, with the
        // same values: their constants are one set. Every other record, or a
        // new frame, writes them.)
        let stamp = (key, view, self.frame);
        if self.last_update != Some(stamp) {
            self.update(&key, view, frame.code);
            self.last_update = Some(stamp);
        }
        let t2 = profile.then(std::time::Instant::now);
        let mut drawn = 0;
        // (When the cull ended: the profile's split of the encoding.)
        let t_cull;
        let mut counts = crate::profile::Counts {
            passes: 1,
            ..Default::default()
        };
        {
            let colour_ops = wgpu::Operations {
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
            };
            let colour = [Some(wgpu::RenderPassColorAttachment {
                view: targets.colour,
                resolve_target: None,
                ops: colour_ops,
                depth_slice: None,
            })];
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("iw6_world"),
                color_attachments: &colour,
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
                    stencil_ops: (frame.target == Target::Scene && self.depth_format.has_stencil_aspect()).then_some(
                        wgpu::Operations {
                            load: wgpu::LoadOp::Load,
                            store: wgpu::StoreOp::Store,
                        },
                    ),
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            let prepared = &self.prepared[&key];
            let (list, culls) = match frame.part {
                Part::Opaque => (&prepared.opaque, &prepared.opaque_cull),
                Part::Blended => (&prepared.blended, &prepared.blended_cull),
            };
            if let Some([x, y, w, h, near, far]) = targets.viewport {
                pass.set_viewport(x, y, w, h, near, far);
            }
            let eye = frame.code.eye;
            let (near_scale, far_scale) = (self.lod_scale, self.lod_scale.min(self.draw_scale));
            // The cells seen this frame (the scene only: the shadow views
            // see what the light sees).
            let cells = self.cell_view.as_ref().filter(|v| v.valid && frame.target == Target::Scene && crate::cells::on());
            // The entries seen (first over the packed cull data alone),
            // then their commands.
            let mut seen = std::mem::take(&mut self.seen_scratch);
            let min_caster = self.min_caster.filter(|_| frame.target != Target::Scene);
            let (occlusion, small) = (
                frame.occlusion.filter(|_| frame.target == Target::Scene),
                frame.small_angle.filter(|_| frame.target == Target::Scene),
            );
            let mut memo = std::mem::take(&mut self.occ_memo);
            memo.clear();
            if occlusion.is_some() || small.is_some() {
                memo.resize(self.scene.draws.len(), 0);
            }
            seen.clear();
            for (i, c) in culls.iter().enumerate() {
                if let Some(v) = cells
                    && c.cells.1 > 0
                {
                    let at = c.cells.0 as usize;
                    if !v.admits(&self.cell_lists[at..at + usize::from(c.cells.1)]) {
                        counts.culled += 1;
                        counts.portal_culled += 1;
                        continue;
                    }
                }
                if c.ranged {
                    let d = eye.distance(c.origin);
                    if d < c.near * near_scale || d >= c.far * far_scale {
                        counts.culled += 1;
                        continue;
                    }
                }
                if c.bounded
                    && let Some(planes) = frame.cull
                    && !box_inside(planes, c.lo, c.hi)
                {
                    counts.culled += 1;
                    continue;
                }
                if c.bounded
                    && let Some((a, b, size)) = min_caster
                {
                    let span = c.hi - c.lo;
                    if span.dot(a.abs()) < size && span.dot(b.abs()) < size {
                        counts.culled += 1;
                        continue;
                    }
                }
                if c.bounded && (occlusion.is_some() || small.is_some()) {
                    let m = &mut memo[c.di as usize];
                    if *m == 0 {
                        *m = 1;
                        if let Some(angle) = small
                            && native_cull::too_small(eye, c.lo, c.hi, angle)
                        {
                            *m = 3;
                        } else if let Some(o) = occlusion
                            && o.box_hidden(c.lo, c.hi)
                        {
                            *m = 2;
                        }
                    }
                    if *m >= 2 {
                        counts.culled += 1;
                        if *m == 2 {
                            counts.occluded += 1;
                        } else {
                            counts.small += 1;
                        }
                        continue;
                    }
                }
                seen.push(i as u32);
            }
            t_cull = profile.then(std::time::Instant::now);
            if frame.target == Target::Scene && frame.part == Part::Opaque && census_on() {
                self.census(&seen, list, eye, prepared.prepass_n);
            }
            if profile {
                counts.tris += seen.iter().map(|&i| u64::from(list[i as usize].count / 3)).sum::<u64>();
                if frame.target == Target::Scene && frame.part == Part::Opaque {
                    // (What the list is made of, for the profile's table.)
                    for &i in &seen {
                        let r = &list[i as usize];
                        let tris = u64::from(r.count / 3);
                        let d = &self.scene.draws[r.di as usize];
                        if (i as usize) < prepared.prepass_n {
                            counts.prepass += 1;
                            counts.prepass_tris += tris;
                            continue;
                        }
                        match d.list {
                            List::Sky => counts.sky += 1,
                            List::Decal => counts.decals += 1,
                            List::Model => {
                                counts.models += 1;
                                counts.model_tris += tris;
                            }
                            _ => {}
                        }
                        if d.light.is_some() {
                            counts.local_lit += 1;
                        }
                    }
                }
            }
            let instancing = crate::instancing::on();
            let blockidx = instancing && crate::instancing::block_index();
            let view_n = view;
            let chunk = {
                let mesh_buffers = &self.mesh_buffers[..];
                let threads = bundle_threads(seen.len());
                if threads <= 1 {
                    // Indirect records for this pass (`BO2MP_IW6_MULTIDRAW`):
                    // a region of the frame's buffer, written after the walk.
                    let want = blockidx && crate::instancing::multi_draw() && self.gpu.device.features().contains(wgpu::Features::INDIRECT_FIRST_INSTANCE);
                    let region = if want {
                        let need = (seen.len() as u64 * 20).next_multiple_of(256).max(256);
                        let frame = self.frame;
                        let fits = self.indirect.as_ref().is_some_and(|(_, cap, cur, f)| *cap >= *cur + need || *f != frame && *cap >= need);
                        if !fits {
                            let cap = (need * 2).max(1 << 20);
                            let buf = self.gpu.device.create_buffer(&wgpu::BufferDescriptor {
                                label: Some("iw6_indirect"),
                                size: cap,
                                usage: wgpu::BufferUsages::INDIRECT | wgpu::BufferUsages::COPY_DST,
                                mapped_at_creation: false,
                            });
                            self.indirect = Some((buf, cap, 0, frame));
                        }
                        let slot = self.indirect.as_mut().expect("indirect buffer");
                        if slot.3 != frame {
                            slot.2 = 0;
                            slot.3 = frame;
                        }
                        let at = slot.2;
                        slot.2 += need;
                        Some((slot.0.clone(), at))
                    } else {
                        None
                    };
                    let mut ch = encode_draws(&mut pass, list, &seen, mesh_buffers, view_n, instancing, blockidx, region.as_ref().map(|(b, at)| (b, *at)), profile);
                    if let Some((buf, at)) = &region
                        && !ch.args.is_empty()
                    {
                        self.gpu.queue.write_buffer(buf, *at, bytemuck::cast_slice(&ch.args));
                    }
                    counts.multi_records += (ch.args.len() / 5) as u32;
                    ch.args = Vec::new();
                    ch
                } else {
                    // The entries in `threads` runs, each recorded into a
                    // render bundle on its own thread (the commands are
                    // checked there, which is most of the CPU a frame with
                    // thousands of draws costs), then executed in order in
                    // this pass: the same commands in the same order.
                    let colour_formats = [Some(targets.colour.texture().format())];
                    let depth_format = targets.depth.texture().format();
                    let stencil = frame.target == Target::Scene && self.depth_format.has_stencil_aspect();
                    let desc = wgpu::RenderBundleEncoderDescriptor {
                        label: Some("iw6_world bundle"),
                        color_formats: &colour_formats,
                        depth_stencil: Some(wgpu::RenderBundleDepthStencil { format: depth_format, depth_read_only: false, stencil_read_only: !stencil }),
                        sample_count: 1,
                        multiview: None,
                    };
                    let device = &self.gpu.device;
                    let per = seen.len().div_ceil(threads);
                    let made: Vec<(wgpu::RenderBundle, Chunk)> = std::thread::scope(|s| {
                        let handles: Vec<_> = seen
                            .chunks(per)
                            .map(|part| {
                                let desc = &desc;
                                s.spawn(move || {
                                    let mut enc = device.create_render_bundle_encoder(desc);
                                    let ch = encode_draws(&mut enc, list, part, mesh_buffers, view_n, instancing, blockidx, None, profile);
                                    (enc.finish(&wgpu::RenderBundleDescriptor { label: Some("iw6_world bundle") }), ch)
                                })
                            })
                            .collect();
                        handles.into_iter().map(|h| h.join().expect("bundle thread")).collect()
                    });
                    pass.execute_bundles(made.iter().map(|m| &m.0));
                    let mut sum = Chunk::default();
                    for (_, c) in &made {
                        sum.add(c);
                    }
                    sum
                }
            };
            drawn += chunk.drawn;
            counts += chunk.counts;
            counts.calls = chunk.issued;
            self.seen_scratch = seen;
            self.occ_memo = memo;
        }
        if let (Some(t0), Some(t1), Some(t2)) = (t0, t1, t2) {
            let ms = |a: std::time::Instant, b: std::time::Instant| (b - a).as_secs_f64() * 1000.0;
            let now = std::time::Instant::now();
            counts.draws = drawn as u32;
            counts.bytes = self.stats.counts.bytes;
            self.stats = crate::profile::Phase {
                total: ms(t0, now),
                prepare: ms(t0, t1),
                constants: ms(t1, t2),
                encode: ms(t2, now),
                cull: t_cull.map_or(0.0, |t| ms(t2, t)),
                counts,
            };
        }
        match frame.target {
            Target::Shadow | Target::SpotShadow => self.report.shadow_drawn = drawn,
            Target::Scene => {
                if frame.part == Part::Opaque {
                    self.report.drawn = drawn;
                } else {
                    self.report.drawn += drawn;
                }
                // (The names are made when the set changes.)
                if let Ok(m) = frame.code.missing.lock()
                    && !m.iter().copied().eq(self.missing_seen.iter().copied())
                {
                    self.missing_seen.clear();
                    self.missing_seen.extend(m.iter().copied());
                    self.report.missing_code = m.iter().map(|&i| crate::code::code_const_name(i).to_string()).collect();
                }
            }
        }
        Ok(())
    }

    fn make_pipeline(&self, pass: &Pass, kind: VertexKind, state: PassState, layer: u8, target: Target) -> Result<Pipeline, String> {
        let _all = crate::loadtime::timer(3);
        let tr = crate::loadtime::timer(0);
        let shadow = target != Target::Scene;
        let vs_name = pass.vertex_shader.as_deref().ok_or("no vertex shader")?;
        let ps_name = pass.pixel_shader.as_deref().ok_or("no pixel shader")?;
        let pool = &self.scene.pool;
        let vs_b = pool
            .program(AssetType::VertexShader, vs_name)
            .ok_or_else(|| format!("vertex shader {vs_name} not loaded"))?;
        let vs_c = dxbc_sm5::Container::parse(vs_b).map_err(|e| format!("{e:?}"))?;
        let vs_p = dxbc_sm5::decode(vs_c.shader).map_err(|e| format!("{e:?}"))?;
        // `null.hlsl`: no pixel shader (depth only).
        let null_ps = key(ps_name).starts_with("null");
        let ps_c = if null_ps {
            None
        } else {
            let b = pool
                .program(AssetType::PixelShader, ps_name)
                .ok_or_else(|| format!("pixel shader {ps_name} not loaded"))?;
            Some(dxbc_sm5::Container::parse(b).map_err(|e| format!("{e:?}"))?)
        };
        let ps = match &ps_c {
            None => dxbc_sm5::Translation {
                stage: Stage::Pixel,
                wgsl: "@fragment fn main() -> @location(0) vec4<f32> { return vec4<f32>(0.0); }\n".into(),
                reflection: dxbc_sm5::Reflection::default(),
            },
            Some(c) => {
                let p = dxbc_sm5::decode(c.shader).map_err(|e| format!("{e:?}"))?;
                let mut po = Options::for_stage(Stage::Pixel);
                po.varyings = dxbc_sm5::link(&vs_c.outputs, &c.inputs);
                let mut t = dxbc_sm5::translate_program(&p, c, &po).map_err(|e| format!("ps {ps_name}: {e:?}"))?;
                // Models are drawn with counter-clockwise fronts (their
                // winding is the world's reversed), which inverts
                // `is_front_face` against D3D's: the two-sided shaders'
                // normal flip is turned back.
                if kind == VertexKind::Model {
                    t.wgsl = t.wgsl.replace(
                        "select(0u, 0xffffffffu, input.sv_ff)",
                        "select(0xffffffffu, 0u, input.sv_ff)",
                    );
                }
                // IW6_DBG_PSOUT=<word>[,<word>...]|<wgsl vec4>: pixel programs
                // whose name holds one of the words write the expression
                // instead (debug).
                if let Ok(v) = std::env::var("IW6_DBG_PSOUT")
                    && let Some((w, e)) = v.split_once('|')
                    && w.split(',').any(|w| ps_name.contains(w))
                {
                    t.wgsl = t
                        .wgsl
                        .replace("output.sv_t0 = bitcast<vec4<f32>>(o0);", &format!("output.sv_t0 = {e};"));
                }
                // A spot or omni light's falloff lookup when the defs' ramps
                // are not read (`local_light`).
                let falloff = if self.scene.falloff_from_defs { u16::MAX } else { code_index("LIGHT_FALLOFF_PLACEMENT") };
                let counts = pass.arg_counts.map(usize::from);
                for (k, a) in pass.args.iter().enumerate() {
                    if a.ty == bind::arg::CODE_CONST && a.code_const().0 == falloff {
                        let rate = if k < counts[0] {
                            bind::PER_PRIM
                        } else if k < counts[0] + counts[1] {
                            bind::PER_OBJECT
                        } else {
                            bind::STABLE
                        };
                        crate::local_light::patch_falloff(&mut t.wgsl, rate, a.dest);
                    }
                }
                if let Ok(dir) = std::env::var("IW6_DUMP_WGSL")
                    && std::env::var("IW6_DUMP_MATCH").is_ok_and(|m| ps_name.contains(m.as_str()))
                {
                    eprintln!("dump {ps_name}: world vertex format {layer}, declaration {:?}", pass.vertex_decl);
                    let _ = std::fs::write(format!("{dir}/ps_{}.wgsl", key(ps_name)), &t.wgsl);
                }
                t
            }
        };
        let mut vo = Options::for_stage(Stage::Vertex);
        if let Some(c) = &ps_c {
            vo.varyings = dxbc_sm5::link(&vs_c.outputs, &c.inputs);
        }
        vo.output_interp = ps
            .reflection
            .inputs
            .iter()
            .map(|i| (i.semantic.to_ascii_uppercase(), i.index, i.interp))
            .collect();
        let vs = dxbc_sm5::translate_program(&vs_p, &vs_c, &vo).map_err(|e| format!("vs {vs_name}: {e:?}"))?;
        if let Ok(dir) = std::env::var("IW6_DUMP_WGSL")
            && std::env::var("IW6_DUMP_MATCH").is_ok_and(|m| vs_name.contains(m.as_str()))
        {
            let _ = std::fs::write(format!("{dir}/vs_{}.wgsl", key(vs_name)), &vs.wgsl);
        }
        drop(tr);
        let mut attrs = Vec::new();
        let mut layer_attrs = Vec::new();
        let routes = pass.vertex_decl.as_deref().map(crate::layers::parse_decl).unwrap_or_default();
        let world_layout = (kind == VertexKind::World && layer != 0)
            .then(|| self.layer_layouts.get(&layer))
            .flatten();
        let mut layer_stride = VERTEX_LIGHT_STRIDE;
        if let Some(l) = world_layout {
            layer_stride = l.stride.max(4);
        }
        for i in &vs.reflection.inputs {
            if layer == VERTEX_LIGHT
                && kind == VertexKind::Model
                && let Some((offset, format)) = vertex_light_input(&i.semantic, i.index)
            {
                layer_attrs.push(wgpu::VertexAttribute {
                    format,
                    offset,
                    shader_location: i.location,
                });
                continue;
            }
            // A layered world surface: the declaration routes the input
            // from the layer stream or from another world vertex part.
            let route = crate::layers::destination(&i.semantic, i.index)
                .and_then(|d| routes.iter().find(|r| r.1 == d));
            if let (Some(l), Some(r)) = (world_layout, route) {
                if !crate::layers::in_world_vertex(&r.0) {
                    let (offset, format) = l
                        .find(&r.0)
                        .ok_or_else(|| format!("layer source {} not in world vertex format {layer}", r.0))?;
                    layer_attrs.push(wgpu::VertexAttribute {
                        format,
                        offset,
                        shader_location: i.location,
                    });
                    continue;
                }
                if let Some((sem, idx)) = crate::layers::world_source(&r.0)
                    && let Some((offset, format)) = kind.attribute(sem, idx)
                {
                    attrs.push(wgpu::VertexAttribute {
                        format,
                        offset,
                        shader_location: i.location,
                    });
                    continue;
                }
            }
            let (offset, format) = kind
                .attribute(&i.semantic, i.index)
                .ok_or_else(|| format!("vertex input {}{} not in a {kind:?} vertex", i.semantic, i.index))?;
            attrs.push(wgpu::VertexAttribute {
                format,
                offset,
                shader_location: i.location,
            });
        }
        let mut streams = vec![wgpu::VertexBufferLayout {
            array_stride: kind.stride(),
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &attrs,
        }];
        if !layer_attrs.is_empty() {
            streams.push(wgpu::VertexBufferLayout {
                array_stride: layer_stride,
                step_mode: wgpu::VertexStepMode::Vertex,
                attributes: &layer_attrs,
            });
        }
        let dev = &self.gpu.device;
        let scope = dev.push_error_scope(wgpu::ErrorFilter::Validation);
        crate::loadtime::seen(0, &vs.wgsl);
        crate::loadtime::seen(1, &ps.wgsl);
        let tm = crate::loadtime::timer(1);
        let module = |label: &str, text: &str| -> wgpu::ShaderModule {
            if !modules_shared() {
                return dev.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some(label), source: wgpu::ShaderSource::Wgsl(text.into()) });
            }
            let mut m = self.modules.lock().unwrap_or_else(|e| e.into_inner());
            if let Some(found) = m.get(text) {
                return found.clone();
            }
            let made = dev.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some(label), source: wgpu::ShaderSource::Wgsl(text.into()) });
            m.insert(text.to_string(), made.clone());
            made
        };
        let vm = module(vs_name, &vs.wgsl);
        let pm = module(ps_name, &ps.wgsl);
        drop(tm);
        // The per-object block takes a dynamic offset (the object); a shadow
        // target's frame blocks too (the view: a cascade or a tile).
        let dynamic_slots: &[u32] = if shadow { &[bind::PER_PRIM, bind::PER_OBJECT, bind::STABLE] } else { &[bind::PER_PRIM] };
        let layout = crate::layout::pass_layout(&self.gpu, &vs.reflection, &ps.reflection, dynamic_slots);
        if layout.dynamic_count > dev.limits().max_dynamic_uniform_buffers_per_pipeline_layout {
            return Err(format!("{vs_name} / {ps_name}: {} dynamic constant buffers", layout.dynamic_count));
        }
        let mut dynamic: Vec<(u32, u32)> = layout
            .cbuffers
            .iter()
            .filter(|c| dynamic_slots.contains(&c.1))
            .map(|c| (c.0, c.1))
            .collect();
        dynamic.sort_unstable();
        let writes = if ps.reflection.targets.contains(&0) {
            state.color_writes()
        } else {
            wgpu::ColorWrites::empty()
        };
        let targets = [Some(if shadow {
            wgpu::ColorTargetState {
                format: SHADOW_COLOUR,
                blend: None,
                write_mask: wgpu::ColorWrites::empty(),
            }
        } else {
            wgpu::ColorTargetState {
                format: SCENE_COLOUR,
                blend: state.blend_state(),
                write_mask: writes,
            }
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
                front_face: match kind {
                    VertexKind::World => wgpu::FrontFace::Cw,
                    VertexKind::Model => wgpu::FrontFace::Ccw,
                },
                cull_mode: if shadow { None } else { state.cull_mode() },
                ..Default::default()
            },
            depth_stencil: Some(if shadow {
                // (The shadow map's depth runs from the sun: 0 nearest.)
                wgpu::DepthStencilState {
                    format: SHADOW_DEPTH,
                    depth_write_enabled: Some(true),
                    depth_compare: Some(wgpu::CompareFunction::LessEqual),
                    stencil: Default::default(),
                    bias: if target == Target::SpotShadow { crate::state::spot_shadow_bias() } else { state.shadow_bias() },
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
            cache: self.pipeline_cache.as_ref().map(|c| &c.0),
        });
        // The instanced form (`instancing`): a vertex program reading its per-primitive block by
        // instance, when the pixel program does not read that block. It is a second pipeline from
        // its own module and layout, independent of the first: built beside it on another thread
        // (`BO2MP_IW6_PIPE_THREADS=1` builds one after the other; error scopes are per thread).
        let limits = dev.limits();
        let storage_align = u64::from(limits.min_storage_buffer_offset_alignment);
        let build_inst = || (crate::instancing::on()
            && limits.max_storage_buffers_per_shader_stage > 0
            && !ps.reflection.cbuffers.iter().any(|c| c.slot == bind::PER_PRIM))
        .then(|| vs.reflection.cbuffers.iter().find(|c| c.slot == bind::PER_PRIM))
        .flatten()
        .and_then(|c| {
            let stride = (u64::from(c.registers.max(1)) * 16).next_multiple_of(self.arena.align);
            if stride % storage_align != 0 {
                return None;
            }
            let wgsl = crate::instancing::patch_vertex(&vs.wgsl, bind::PER_PRIM, stride)?;
            let scope = dev.push_error_scope(wgpu::ErrorFilter::Validation);
            let vm = dev.create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some(vs_name),
                source: wgpu::ShaderSource::Wgsl(wgsl.into()),
            });
            let il = crate::layout::pass_layout_with(&self.gpu, &vs.reflection, &ps.reflection, dynamic_slots, Some(bind::PER_PRIM));
            let p = make(&vm, &il.pipeline_layout);
            match pollster::block_on(scope.pop()) {
                None => Some((p, il, stride)),
                Some(e) => {
                    eprintln!("note: iw6 instancing: {vs_name} / {ps_name} not instanced: {e}");
                    None
                }
            }
        });
        let tp = crate::loadtime::timer(2);
        let (pipeline, inst) = if crate::loadtime::pipe_threads() > 1 {
            std::thread::scope(|sc| {
                let side = sc.spawn(&build_inst);
                let p = make(&vm, &layout.pipeline_layout);
                (p, side.join().unwrap_or(None))
            })
        } else {
            (make(&vm, &layout.pipeline_layout), None)
        };
        drop(tp);
        if let Some(e) = pollster::block_on(scope.pop()) {
            return Err(format!("pipeline {vs_name} / {ps_name} ({kind:?}): {e}"));
        }
        let inst = if crate::loadtime::pipe_threads() > 1 { inst } else { build_inst() };
        let mut regs: BTreeMap<u32, u32> = BTreeMap::new();
        for c in vs.reflection.cbuffers.iter().chain(&ps.reflection.cbuffers) {
            let e = regs.entry(c.slot).or_insert(0);
            *e = (*e).max(c.registers);
        }
        Ok(Pipeline {
            pipeline,
            id: 0,
            dynamic,
            layered: streams.len() > 1,
            layout,
            regs,
            inst,
            discards: ps.wgsl.contains("discard"),
        })
    }

    pub fn image_tex(&mut self, name: &str) -> Option<&Tex> {
        let k = key(name);
        if !self.tex.contains_key(&k) {
            let t = match self.scene.images.image(&k) {
                Some((px, faces)) => match gpu::upload(&self.gpu, &k, &px, faces) {
                    Ok(t) => {
                        if let crate::bake::State::Writing(_, w, n) = &mut self.bake {
                            w.push(crate::bake::picture(&k, &px, faces));
                            *n += 1;
                        }
                        Some(t)
                    }
                    Err(e) => {
                        self.report.missing_images.insert(k.clone(), e);
                        None
                    }
                },
                None => {
                    self.report.missing_images.insert(k.clone(), "not read (its pak is not there)".into());
                    None
                }
            };
            self.tex.insert(k.clone(), t);
        }
        self.tex.get(&k).and_then(Option::as_ref)
    }

    /// Pictures loaded and missing so far.
    pub fn image_counts(&self) -> (usize, usize) {
        let ok = self.tex.values().filter(|t| t.is_some()).count();
        (ok, self.tex.len() - ok)
    }

    fn default_tex(&mut self, what: &str, dim: wgpu::TextureViewDimension, kind: TextureKind) -> &Tex {
        let k = format!("{what}/{dim:?}/{kind:?}");
        if !self.defaults.contains_key(&k) {
            let t = if kind == TextureKind::Depth {
                gpu::solid_depth(&self.gpu, 0.0, dim)
            } else {
                let rgba = match what {
                    "white" => [255, 255, 255, 255],
                    // BC5's two channels: a flat normal is (0.5, 0.5).
                    "normal" => [128, 128, 255, 255],
                    // A picture that isn't loaded: mid grey, alpha 0 (an
                    // alpha-tested cut-out or a decal that would show it
                    // draws nothing; opaque surfaces don't read its alpha).
                    "grey" => [140, 140, 140, 0],
                    "grey_opaque" => [140, 140, 140, 255],
                    _ => [0, 0, 0, 0],
                };
                gpu::solid(&self.gpu, rgba, dim)
            };
            self.defaults.insert(k.clone(), t);
        }
        &self.defaults[&k]
    }

    fn sampler(&mut self, s: SampSource, comparison: bool) -> wgpu::Sampler {
        let k = format!("{s:?}/{comparison}/{}", self.anisotropy);
        if let Some(x) = self.samplers.get(&k) {
            return x.clone();
        }
        let x = if comparison {
            shadow_sampler(&self.gpu)
        } else {
            match s {
                SampSource::Material(state) => material_sampler(&self.gpu, state, self.anisotropy),
                _ => gpu::linear_clamp(&self.gpu),
            }
        };
        self.samplers.insert(k, x.clone());
        x
    }
}

/// A material's sampler from its state byte (`render_h1::gpu::material_sampler`'s
/// reading) with its anisotropy capped at `max_aniso` (the graphics
/// settings' texture filtering; 16 leaves it as the material says).
fn material_sampler(gpu: &Gpu, state: u8, max_aniso: u16) -> wgpu::Sampler {
    let filter = state & 7;
    let mip = state & 0x18;
    let clamp = |bit: u8| {
        if state & bit != 0 {
            wgpu::AddressMode::ClampToEdge
        } else {
            wgpu::AddressMode::Repeat
        }
    };
    let lin = !matches!(filter, 1);
    let f = if lin { wgpu::FilterMode::Linear } else { wgpu::FilterMode::Nearest };
    let m = match mip {
        0x10 => wgpu::MipmapFilterMode::Linear,
        _ => wgpu::MipmapFilterMode::Nearest,
    };
    let aniso: u16 = match filter {
        3 => 2,
        4 => 4,
        6 => 16,
        _ => 1,
    };
    gpu.device.create_sampler(&wgpu::SamplerDescriptor {
        label: Some("material"),
        address_mode_u: clamp(0x20),
        address_mode_v: clamp(0x40),
        address_mode_w: clamp(0x80),
        mag_filter: f,
        min_filter: f,
        mipmap_filter: m,
        lod_max_clamp: if mip == 0 { 0.0 } else { 32.0 },
        anisotropy_clamp: if lin && mip == 0x10 { aniso.min(max_aniso.max(1)) } else { 1 },
        ..Default::default()
    })
}

/// The sun shadow map's comparison sampler: lit where the receiver's depth
/// is at or before the caster's (the map's depth runs from the sun),
/// linear (bilinear compares), a white border (outside the map is lit;
/// the shaders fade to the baked visibility there anyway).
pub fn shadow_sampler(gpu: &Gpu) -> wgpu::Sampler {
    let border = gpu.device.features().contains(wgpu::Features::ADDRESS_MODE_CLAMP_TO_BORDER);
    let mode = if border { wgpu::AddressMode::ClampToBorder } else { wgpu::AddressMode::ClampToEdge };
    gpu.device.create_sampler(&wgpu::SamplerDescriptor {
        label: Some("iw6_shadow"),
        address_mode_u: mode,
        address_mode_v: mode,
        address_mode_w: mode,
        mag_filter: wgpu::FilterMode::Linear,
        min_filter: wgpu::FilterMode::Linear,
        compare: Some(wgpu::CompareFunction::LessEqual),
        border_color: border.then_some(wgpu::SamplerBorderColor::OpaqueWhite),
        ..Default::default()
    })
}

fn vertex_buffer(gpu: &Gpu, bytes: &[u8]) -> wgpu::Buffer {
    wgpu::util::DeviceExt::create_buffer_init(
        &gpu.device,
        &wgpu::util::BufferInitDescriptor {
            label: Some("iw6_vb"),
            contents: bytes,
            usage: wgpu::BufferUsages::VERTEX,
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn idx() -> Indices {
        Indices {
            wvp: code_index("TRANSPOSE_WORLD_VIEW_PROJECTION_MATRIX0"),
            inv_wv: code_index("INVERSE_TRANSPOSE_WORLD_VIEW_MATRIX0"),
            world: code_index("TRANSPOSE_WORLD_MATRIX0"),
            lookup: code_index("LIGHTING_LOOKUP_SCALE"),
        }
    }

    fn code_write(index: u16, dest: u16, rows: u8) -> CbWrite {
        CbWrite {
            buffer: bind::PER_PRIM,
            dest,
            src: CbSource::Code {
                index,
                first_row: 0,
                rows: rows.into(),
            },
        }
    }

    /// The rows an object owns, a world surface's identity and the lookup
    /// are written once; the view's matrices and frame values follow.
    #[test]
    fn what_follows_the_view() {
        let i = idx();
        let world = glam::Mat4::from_translation(glam::Vec3::new(10.0, 0.0, 0.0));
        let obj = Object {
            rows: vec![(i.world, (0..4).map(|r| world.row(r).to_array()).collect())],
            world,
            tile: None,
        };
        assert!(!follows(&code_write(i.world, 0, 4), Some(&obj), i));
        assert!(!follows(&code_write(i.world, 0, 4), None, i));
        assert!(!follows(&code_write(i.lookup, 0, 1), Some(&obj), i));
        assert!(follows(&code_write(i.wvp, 0, 4), Some(&obj), i));
        assert!(follows(&code_write(i.inv_wv, 0, 4), None, i));
        assert!(follows(&code_write(code_index("EYEOFFSET"), 0, 1), Some(&obj), i));
        let lit = CbWrite {
            buffer: bind::PER_PRIM,
            dest: 0,
            src: CbSource::Literal([1.0; 4]),
        };
        assert!(!follows(&lit, Some(&obj), i));
    }

    /// `write_rows` gives what the per-frame fill gave: the object's own
    /// rows, its view-projection made from the frame's, the frame's values.
    #[test]
    fn rows_written() {
        let i = idx();
        let world = glam::Mat4::from_translation(glam::Vec3::new(10.0, 2.0, 3.0));
        let obj = Object {
            rows: vec![(i.world, (0..4).map(|r| world.row(r).to_array()).collect())],
            world,
            tile: None,
        };
        let mut code = CodeValues {
            view_projection: glam::Mat4::from_scale(glam::Vec3::splat(2.0)),
            eye: glam::Vec3::new(1.0, 0.0, 0.0),
            ..CodeValues::default()
        };
        code.set_vec("EYEOFFSET", [1.0, 0.0, 0.0, 1.0]);
        let mut out = Vec::new();
        write_rows(&code_write(i.world, 0, 4), Some(&obj), &code, [0.0; 4], i, &mut out);
        assert_eq!(out[0], world.row(0).to_array());
        out.clear();
        write_rows(&code_write(i.wvp, 0, 4), Some(&obj), &code, [0.0; 4], i, &mut out);
        let want = code.view_projection * glam::Mat4::from_translation(-code.eye) * world;
        assert_eq!(out, (0..4).map(|r| want.row(r).to_array()).collect::<Vec<_>>());
        out.clear();
        write_rows(&code_write(code_index("EYEOFFSET"), 0, 1), None, &code, [0.0; 4], i, &mut out);
        assert_eq!(out, vec![[1.0, 0.0, 0.0, 1.0]]);
        let mut a = Vec::new();
        let mut t = Vec::new();
        fill_into(&[code_write(i.wvp, 2, 4)], bind::PER_PRIM, 3, Some(&obj), &code, [0.0; 4], i, &mut a, &mut t);
        assert_eq!(a.len(), 6);
        assert_eq!(a[2], want.row(0).to_array());
    }

    /// Every view of a shadow target has its own constants side by side.
    #[test]
    fn views_per_target() {
        assert_eq!(views_for(Target::Scene), 1);
        assert_eq!(views_for(Target::Shadow), 2);
        assert_eq!(views_for(Target::SpotShadow), crate::spot_shadow::SPOT_LIMIT as u32);
    }
}

#[cfg(test)]
mod twin_tests {
    use super::*;

    #[test]
    fn displacement_only_unlit_add_set_finds_its_plain_twin() {
        let mut pool = Pool::default();
        pool.techsets.insert("wc_unlit_add_lin_ndw_bl_blmod_nfwpf".into(), vec![(7, "effect_add_lin_ndw_blml_blmod".into())]);
        assert_eq!(
            plain_twin(&pool, "w_unlit_add_lin_bl_blmod_z1_nfwpf").as_deref(),
            Some("wc_unlit_add_lin_ndw_bl_blmod_nfwpf")
        );
        assert_eq!(plain_twin(&pool, "w_l_sm_r0c0z0n0s0l0_z1_nfwpf"), None);
        assert_eq!(plain_twin(&Pool::default(), "w_unlit_add_lin_bl_blmod_z1_nfwpf"), None);
    }
}

/// `BO2MP_IW6_MODULE_SHARE=0`: every pipeline makes its own shader modules.
fn modules_shared() -> bool {
    static S: crate::switch::Switch = crate::switch::Switch::on("BO2MP_IW6_MODULE_SHARE");
    S.get()
}
