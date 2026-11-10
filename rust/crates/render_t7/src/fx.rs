//! Black Ops III's map effects as `render_t7` draws them: the map's placed
//! effects (`"classname" "fx"`), their `FxEffectDef`/`FxElemDef`s read off
//! the zones (`fastfile_t7::walk::fx`), evaluated each frame and packed into
//! the 64-byte sprite records the effect techniques' vertex programs read.
//!
//! The record (layout group 8, one instance per sprite, the quad's corner
//! from `SV_VertexID & 3`), as `ei/effect_*`'s vertex program reads it
//! (`t7_fx_techs` dumps the programs; see docs/T7_VISUAL_PARITY.md):
//!
//! ```text
//! 0x00 f32 x3   centre (world), w unused          POSITION
//! 0x10 rgba8    colour (red first)                COLOR0
//! 0x14 u32      atlas: cell | next cell << 8 | blend << 16 | column bits << 24 | row bits << 28 (0: whole picture)
//! 0x18 u32      view-angle fade: two u16 angles in degrees (0, 0: none)
//! 0x1c u32      emissive scale (f16, low half) | sprite light map layer << 16
//! 0x20 unorm10_10_10_2 axis A, 0x24 axis B (used with flag 4)       NORMAL, TEXCOORD1
//! 0x28 f32 x4   pivot (in half sizes), half width, half height       TEXCOORD2
//! 0x38 u16 x4   alpha mode (256: plain), soft depth * 16, flags (4: own axes, 2: z-up, 1: face the eye), rotation (65536 = 2 pi)
//! ```
//!
//! The quad: `centre + U * halfW * (pivotX + cy) + V' * halfH * (pivotY + cx)`
//! with `U = A sin r + B cos r`, `V = B sin r - A cos r` (A, B: the view's
//! up and right without flags). Looping elements are evaluated without
//! state: a looping element spawns `count` every `interval` ms from the
//! effect's start, each one lives its sampled life span, so the elements
//! alive at a time are found from the time alone (as every frame of the
//! exe's would show them, not its pool and update order).

use std::collections::HashMap;

use fx_iw4::FxElemVec3Range;

/// Element types (`FxElemDef` +0xc8).
pub mod elem_type {
    pub const BILLBOARD: u8 = 0;
    pub const ORIENTED: u8 = 1;
    pub const ROTATED: u8 = 2;
    pub const TAIL: u8 = 3;
    pub const LINE: u8 = 4;
    pub const TRAIL: u8 = 5;
    pub const CLOUD: u8 = 6;
    pub const MODEL: u8 = 7;
    pub const LIGHT: u8 = 8;
    pub const LENS_FLARE: u8 = 12;
    pub const DECAL: u8 = 13;
    pub const NAMED: u8 = 14;
}

/// Tails and lines: on unless `BO2MP_T7_FX_TAILS=0` (offline, Evac's
/// `fx_water_stream_sm_apt` draws its falling stream since their quads wind
/// as the billboards' do; not compared with the game).
pub fn tails_on() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| !std::env::var("BO2MP_T7_FX_TAILS").is_ok_and(|v| v.trim() == "0") || cfg!(test))
}

/// Model elements (7: an XModel per particle, drawn through the G-buffer's
/// model instances, `Scene::read_effects`) only with `BO2MP_T7_FX_MODELS=1`
/// (unverified on the GPU).
pub fn models_on() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var("BO2MP_T7_FX_MODELS").is_ok_and(|v| v.trim() == "1") || cfg!(test))
}

/// The sprite record's size.
pub const SPRITE_RECORD: usize = 64;
/// The most sprites one element of one placed effect draws in a frame.
const MAX_PER_ELEM: usize = 256;
/// The most sprites a frame draws.
pub const MAX_SPRITES: usize = 32768;

const T7_VIS: usize = 80;
const IW4_VIS: usize = 48;
/// `FxElemDef` flag: the second size channel sizes the height.
const NON_UNIFORM_SIZE: i32 = 0x1000_0000;

fn f32_at(b: &[u8], o: usize) -> f32 {
    b.get(o..o + 4).map_or(0.0, |s| f32::from_le_bytes(s.try_into().unwrap()))
}
fn i32_at(b: &[u8], o: usize) -> i32 {
    b.get(o..o + 4).map_or(0, |s| i32::from_le_bytes(s.try_into().unwrap()))
}
fn i16_at(b: &[u8], o: usize) -> i32 {
    b.get(o..o + 2).map_or(0, |s| i32::from(i16::from_le_bytes(s.try_into().unwrap())))
}

/// One element as the frame evaluates it.
#[derive(Clone, Debug, Default)]
pub struct Elem {
    pub ty: u8,
    pub flags: i32,
    /// Looping: (interval ms, count); one-shot: (count base, amplitude).
    pub spawn: (i32, i32),
    pub spawn_range: [f32; 2],
    pub fade_out: [f32; 2],
    pub fade_in: [f32; 2],
    pub delay: (i32, i32),
    pub life: (i32, i32),
    pub spawn_origin: [[f32; 2]; 3],
    pub offset_radius: [f32; 2],
    pub offset_height: [f32; 2],
    pub spawn_angles: [[f32; 2]; 3],
    pub angular_velocity: [[f32; 2]; 3],
    pub initial_rotation: [f32; 2],
    pub gravity: [f32; 2],
    /// IW4's 8 atlas bytes (behaviour, index, fps, loops, column bits, row bits, entries).
    pub atlas: [u8; 8],
    /// Black Ops III's own atlas bytes (+0xbc), as [`atlas_word`] reads them.
    pub atlas_t7: [u8; 8],
    pub vel_local: Vec<FxElemVec3Range>,
    pub vel_world: Vec<FxElemVec3Range>,
    /// The visual samples as IW4's 48-byte ones (colour kept red first).
    pub vis: Vec<u8>,
    /// Per visual sample, word 6 of each half (base, far end): the
    /// sprite's emissive scale.
    pub emissive: Vec<[f32; 2]>,
    pub vis_intervals: u8,
    /// The scene material per visual (`None`: not drawable here).
    pub materials: Vec<Option<usize>>,
    pub sort: u8,
    /// A runner's (type 14) effects, per visual (indices into the scene's
    /// defs; `None`: not found).
    pub runs: Vec<Option<usize>>,
    /// The effect played where each particle dies (+0x1a0).
    pub death: Option<usize>,
    /// The effect names the element plays, by visual (runners) and at
    /// death, as read (resolved by `FxScene::build`).
    pub run_names: Vec<String>,
    pub death_name: Option<String>,
    /// A model element's (7) XModel names per visual, and their indices
    /// into [`FxScene::model_names`].
    pub model_names: Vec<String>,
    pub models: Vec<Option<usize>>,
    /// The sprite record's words the exe takes from the element def
    /// (`FUN_141573090`): pivot (+0x228, +0x22c), alpha scale (+0x230;
    /// record 0x38 = 256 / it), soft depth (+0x234; record 0x3a = 16 x it),
    /// view-angle fade (+0x238 | +0x23c << 16; record 0x18).
    pub pivot: [f32; 2],
    pub alpha_scale: f32,
    pub soft: f32,
    pub angle_fade: u32,
    /// The axis a cylinder spawn offset goes round (+0x6c: 0 forward, 1
    /// left, 2 up; `FUN_1415d47a0`).
    pub cylinder_axis: u8,
    /// How far the sprite is pulled toward the eye (+0x1f0, a byte of
    /// units; `FUN_14157cc80`).
    pub camera_offset: f32,
}

impl Elem {
    fn sprite(&self) -> bool {
        matches!(self.ty, elem_type::BILLBOARD | elem_type::ORIENTED | elem_type::ROTATED | elem_type::CLOUD) || matches!(self.ty, elem_type::TAIL | elem_type::LINE) && tails_on()
    }
}

#[derive(Clone, Debug, Default)]
pub struct Def {
    pub name: String,
    pub looping: usize,
    pub one_shot: usize,
    pub msec_looping_life: i32,
    pub elems: Vec<Elem>,
    /// How long the effect plays from its start, ms (`i32::MAX`: a looping
    /// effect with no looping life never ends).
    pub duration: i32,
}

impl Def {
    fn duration(&self) -> i32 {
        let mut d = 0i32;
        for (i, e) in self.elems.iter().enumerate() {
            let one = (e.delay.0 + e.delay.1.max(0)).saturating_add(e.life.0 + e.life.1.max(0));
            if i < self.looping {
                if self.msec_looping_life <= 0 {
                    return i32::MAX;
                }
                d = d.max(self.msec_looping_life.saturating_add(one));
            } else {
                d = d.max(one);
            }
        }
        d
    }
}

/// A placed effect.
#[derive(Clone, Copy, Debug)]
pub struct Placed {
    pub def: usize,
    pub origin: [f32; 3],
    pub axis: [[f32; 3]; 3],
    /// When it started, ms on the frame clock (its `delay`: negative = it
    /// has run that long when the map starts).
    pub start_ms: i32,
    /// Its seed, 0 .. 808 (the exe's effect +0xb2).
    pub seed: u16,
    /// Its time scale (the entity's `timescale`; the exe's effect +0xd4:
    /// a particle's age is the time since it began times it).
    pub timescale: f32,
}

#[derive(Default)]
pub struct FxScene {
    pub defs: Vec<Def>,
    pub placed: Vec<Placed>,
    pub report: Vec<String>,
    /// The XModels model elements name (`Elem::models` index this).
    pub model_names: Vec<String>,
    /// Per def, the eye distance from a placement's origin past which none
    /// of its sprites can be drawn (`f32::INFINITY`: any; empty: no
    /// culling): each sprite element's far fade (or spawn range) end plus
    /// how far its sprites reach from the origin (`fx_check`'s bound). A
    /// placed effect that far away is not evaluated (`T7_FX_NO_CULL=1`:
    /// every effect every frame).
    pub cull_far: Vec<FarCut>,
}

/// [`FxScene::cull_far`] of one def: the whole def's and each element's
/// (non-sprite elements: 0; infinite: never left out).
#[derive(Clone, Debug, Default)]
pub struct FarCut {
    pub def: f32,
    pub elems: Vec<f32>,
}

/// One model element particle of a frame: the model (into
/// [`FxScene::model_names`]), its distance and its placement as a bone
/// matrix (rows: rotation | translation, as a static model's).
#[derive(Clone, Copy, Debug)]
pub struct ModelInst {
    pub model: usize,
    pub dist: f32,
    pub bone: [f32; 16],
}

/// Where one frame's particles go.
struct Sink<'a> {
    sprites: &'a mut Vec<Sprite>,
    models: Option<&'a mut Vec<ModelInst>>,
}

/// The sprite sort's vectors, kept between frames.
#[derive(Default)]
pub struct SortScratch {
    order: Vec<u32>,
    sorted: Vec<Sprite>,
}

/// One sprite of a frame: its scene material, distance and record.
#[derive(Clone, Copy)]
pub struct Sprite {
    pub material: usize,
    pub dist: f32,
    pub sort: u8,
    pub record: [u8; SPRITE_RECORD],
    /// The effect def and element it came from (checks).
    pub def: u32,
    pub elem: u16,
}

/// The 80-byte Black Ops III visual sample as IW4's 48-byte one (colour
/// bytes as they are, rotation delta and total, size 0/1, scale), and its
/// emissive words.
fn vis_sample(s: &[u8]) -> ([u8; IW4_VIS], [f32; 2]) {
    let mut out = [0u8; IW4_VIS];
    for half in 0..2 {
        let (src, o) = (half * T7_VIS / 2, half * IW4_VIS / 2);
        out[o..o + 4].copy_from_slice(s.get(src..src + 4).unwrap_or(&[0; 4]));
        // (Rotation delta, total, size 0, size 1, scale: the scale at +0x14,
        // where the exe's model draw `FUN_141575ca0` reads it.)
        for (k, at) in [4usize, 8, 12, 16, 20].into_iter().enumerate() {
            out[o + 4 + 4 * k..o + 8 + 4 * k].copy_from_slice(&f32_at(s, src + at).to_le_bytes());
        }
    }
    (out, [f32_at(s, 24), f32_at(s, T7_VIS / 2 + 24)])
}

/// Black Ops III's atlas (+0xbc: behaviour, index, fps, loops, column
/// bits, row bits) as IW4's 8 bytes.
fn atlas(r: &[u8]) -> [u8; 8] {
    let b = |o: usize| r.get(0xbc + o).copied().unwrap_or(0);
    let (col, row) = (b(4) & 7, b(5) & 7);
    let cells: u16 = 1 << (col + row);
    let [lo, hi] = cells.to_le_bytes();
    [b(0), b(1), b(2), b(3).max(1), col, row, lo, hi]
}

fn vel(samples: &[u8], n: usize, world: bool) -> Vec<FxElemVec3Range> {
    let base = if world { 0x30 } else { 0 };
    (0..n)
        .map_while(|i| {
            let at = i * 96 + base;
            samples.get(at..at + 24).map(|r| FxElemVec3Range { base: core::array::from_fn(|a| f32_at(r, a * 4)), amplitude: core::array::from_fn(|a| f32_at(r, 12 + a * 4)) })
        })
        .collect()
}

/// One element from its walked record; `material` resolves a visual's
/// material name to a scene material.
pub fn elem(e: &fastfile_t7::walk::capture::FxElemRec, material: &mut dyn FnMut(&str) -> Option<usize>) -> Elem {
    let r = &e.raw;
    let f = |o: usize| f32_at(r, o);
    let range = |o: usize| [f(o), f(o + 4)];
    let ty = r.get(0xc8).copied().unwrap_or(0xff);
    let vel_n = usize::from(r.get(0xca).copied().unwrap_or(0)) + 1;
    let vis_intervals = r.get(0xcd).copied().unwrap_or(0);
    let mut vis = Vec::new();
    let mut emissive = Vec::new();
    for s in e.samples[2].chunks_exact(T7_VIS).take(usize::from(vis_intervals) + 1) {
        let (v, w) = vis_sample(s);
        vis.extend_from_slice(&v);
        emissive.push(w);
    }
    // (One sample (no intervals): the exe reads it and the next with weight
    // 0; it is drawn, as its only sample, not dropped.)
    if vis.len() == IW4_VIS {
        vis.extend_from_within(..IW4_VIS);
        emissive.push(emissive[0]);
    }
    let mut el = Elem {
        ty,
        flags: i32_at(r, 0),
        spawn: (i32_at(r, 0x08), i32_at(r, 0x0c)),
        spawn_range: range(0x18),
        fade_out: range(0x20),
        fade_in: range(0x28),
        delay: (i32_at(r, 0x34), i32_at(r, 0x38)),
        life: (i32_at(r, 0x3c), i32_at(r, 0x40)),
        spawn_origin: core::array::from_fn(|a| range(0x44 + 8 * a)),
        offset_radius: range(0x5c),
        offset_height: range(0x64),
        spawn_angles: core::array::from_fn(|a| range(0x70 + 8 * a)),
        angular_velocity: core::array::from_fn(|a| range(0x88 + 8 * a)),
        initial_rotation: range(0xa0),
        gravity: range(0xac),
        atlas: atlas(r),
        atlas_t7: core::array::from_fn(|k| r.get(0xbc + k).copied().unwrap_or(0)),
        vel_local: vel(&e.samples[0], vel_n, false),
        vel_world: vel(&e.samples[0], vel_n, true),
        vis,
        emissive,
        vis_intervals,
        materials: Vec::new(),
        sort: r.get(0x1f0).copied().unwrap_or(0),
        pivot: [f(0x228), f(0x22c)],
        alpha_scale: if r.len() >= 0x234 { f(0x230) } else { 1.0 },
        soft: f(0x234),
        angle_fade: (i32_at(r, 0x238) as u32 & 0xffff) | (i32_at(r, 0x23c) as u32) << 16,
        cylinder_axis: r.get(0x6c).copied().unwrap_or(0),
        camera_offset: f32::from(r.get(0x1f0).copied().unwrap_or(0)),
        ..Default::default()
    };
    if el.sprite() {
        el.materials = e.visuals.iter().map(|v| material(v.trim_start_matches(','))).collect();
    }
    if el.ty == elem_type::MODEL {
        el.model_names = e.visuals.iter().map(|v| v.trim_start_matches(',').to_owned()).filter(|v| !v.is_empty()).collect();
    }
    if el.ty == elem_type::NAMED {
        el.run_names = e.visuals.iter().map(|v| v.trim_start_matches(',').to_owned()).collect();
    }
    el.death_name = e.effects[1].as_ref().map(|n| n.trim_start_matches(',').to_owned()).filter(|n| !n.is_empty());
    el
}

/// How deep runners and death effects nest (an effect a runner plays that
/// runs another ...).
const MAX_DEPTH: u8 = 3;
/// The most children one element of one effect has playing at a time.
const MAX_CHILDREN: usize = 16;

/// Reads an effect def and (to `MAX_DEPTH`) the effects its runners and
/// death effects play.
fn load_def(map: &fastfile_t7::map::T7Map, name: &str, s: &mut FxScene, by_name: &mut HashMap<String, Option<usize>>, material: &mut dyn FnMut(&str) -> Option<usize>, depth: u8) -> Option<usize> {
    if let Some(d) = by_name.get(name) {
        return *d;
    }
    let fx = map.effect(name);
    let Some(fx) = fx else {
        by_name.insert(name.to_owned(), None);
        return None;
    };
    let h = &fx.header;
    let looping = i16_at(h, 0x0c).max(0) as usize;
    let one_shot = i16_at(h, 0x0e).max(0) as usize;
    let elems: Vec<Elem> = fx.elems.iter().map(|e| elem(e, material)).collect();
    s.defs.push(Def { name: name.to_owned(), looping, one_shot, msec_looping_life: i32_at(h, 0x18), elems, duration: 0 });
    let i = s.defs.len() - 1;
    s.defs[i].duration = s.defs[i].duration();
    by_name.insert(name.to_owned(), Some(i));
    if depth < MAX_DEPTH {
        for ei in 0..s.defs[i].elems.len() {
            let names = s.defs[i].elems[ei].run_names.clone();
            let runs: Vec<Option<usize>> = names.iter().map(|n| load_def(map, n, s, by_name, material, depth + 1)).collect();
            let death = s.defs[i].elems[ei].death_name.clone().and_then(|n| load_def(map, &n, s, by_name, material, depth + 1));
            let e = &mut s.defs[i].elems[ei];
            e.runs = runs;
            e.death = death;
        }
    }
    Some(i)
}

/// The map's placed effects (as the client's radiant loader keeps them,
/// see docs/T7_NATIVE.md "Effects"): `fxstate1` gates, mode-bound and
/// exploder effects wait for scripts and are left out.
pub fn placements(entities: &str) -> Vec<(String, [f32; 3], [f32; 3], f32, f32)> {
    let mut out = Vec::new();
    for block in entities.split('{') {
        let value = |key: &str| {
            let at = block.find(&format!("\"{key}\" \""))? + key.len() + 4;
            let rest = &block[at..];
            Some(&rest[..rest.find('"')?])
        };
        let floats = |s: &str| -> Option<[f32; 3]> {
            let v: Vec<f32> = s.split_whitespace().filter_map(|w| w.parse().ok()).collect();
            (v.len() == 3).then(|| [v[0], v[1], v[2]])
        };
        if value("classname") != Some("fx") || value("script_gameobjectname").is_some_and(|v| !v.is_empty()) {
            continue;
        }
        if value("fxstate1").is_some_and(|v| v.trim().parse::<i32>().unwrap_or(0) == 0) || value("exploder_count").is_some_and(|v| v.trim().parse::<i32>().unwrap_or(0) > 0) && !exploders_fired() {
            continue;
        }
        let (Some(def), Some(origin)) = (value("fxdef"), value("origin").and_then(floats)) else { continue };
        let angles = value("angles").and_then(floats).unwrap_or_default();
        let delay = value("delay").and_then(|d| d.trim().parse::<f32>().ok()).unwrap_or(0.0);
        let timescale = value("timescale").and_then(|d| d.trim().parse::<f32>().ok()).filter(|t| t.is_finite() && *t > 0.0).unwrap_or(1.0);
        out.push((def.to_owned(), origin, angles, delay, timescale));
    }
    out
}

/// Effects that wait for an exploder played as if their exploder fired at
/// the map's start (`BO2MP_T7_FX_EXPLODERS=1`): the map's script, which
/// fires them in the game, is not run here.
pub fn exploders_fired() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var("BO2MP_T7_FX_EXPLODERS").is_ok_and(|v| v.trim() == "1"))
}

/// The map's effect entities left out of [`placements`]: (fxstate1 off,
/// bound to a game mode, waiting for an exploder). Exploder effects start
/// only when the map's script fires their exploder; the script is not run
/// here, so they are counted, not played.
pub fn left_out(entities: &str) -> (usize, usize, usize) {
    let (mut state, mut mode, mut exploder) = (0, 0, 0);
    for block in entities.split('{').filter(|b| b.contains("\"classname\" \"fx\"")) {
        let value = |key: &str| {
            let at = block.find(&format!("\"{key}\" \""))? + key.len() + 4;
            let rest = &block[at..];
            Some(&rest[..rest.find('"')?])
        };
        if value("script_gameobjectname").is_some_and(|v| !v.is_empty()) {
            mode += 1;
        } else if value("exploder_count").is_some_and(|v| v.trim().parse::<i32>().unwrap_or(0) > 0) {
            exploder += 1;
        } else if value("fxstate1").is_some_and(|v| v.trim().parse::<i32>().unwrap_or(0) == 0) {
            state += 1;
        }
    }
    (state, mode, exploder)
}

/// IW angles (pitch, yaw, roll; degrees) as the axis rows (forward, left, up).
pub fn axis_from_angles(deg: [f32; 3]) -> [[f32; 3]; 3] {
    fx_iw4::angles_to_axis_radians(deg.map(f32::to_radians))
}

impl FxScene {
    /// Reads the effects the placements name (and only those: runners'
    /// children are counted, not played yet).
    pub fn build(map: &fastfile_t7::map::T7Map, material: &mut dyn FnMut(&str) -> Option<usize>) -> Self {
        let mut s = FxScene::default();
        let mut by_name: HashMap<String, Option<usize>> = HashMap::new();
        let mut missing = 0;
        for (name, origin, angles, delay, timescale) in placements(&map.entities) {
            let bare = name.trim_start_matches(',').to_owned();
            let def = load_def(map, &bare, &mut s, &mut by_name, material, 0);
            let Some(def) = def else {
                missing += 1;
                continue;
            };
            let seed = (origin.iter().fold(0x811c_9dc5u32, |h, v| (h ^ v.to_bits()).wrapping_mul(0x0100_0193)) % crate::fx_rand::MOD) as u16;
            s.placed.push(Placed { def, origin, axis: axis_from_angles(angles), start_ms: delay as i32, seed, timescale });
        }
        // The model elements' XModels, each named once.
        let mut model_names: Vec<String> = Vec::new();
        for e in s.defs.iter_mut().flat_map(|d| d.elems.iter_mut()) {
            e.models = e
                .model_names
                .iter()
                .map(|n| {
                    Some(model_names.iter().position(|m| m == n).unwrap_or_else(|| {
                        model_names.push(n.clone());
                        model_names.len() - 1
                    }))
                })
                .collect();
        }
        s.model_names = model_names;
        let mut types: std::collections::BTreeMap<u8, usize> = Default::default();
        let (mut elems, mut drawable, mut mats, mut mats_ok) = (0, 0, 0, 0);
        for d in &s.defs {
            for e in &d.elems {
                elems += 1;
                *types.entry(e.ty).or_default() += 1;
                if e.sprite() {
                    mats += e.materials.len();
                    mats_ok += e.materials.iter().flatten().count();
                    if e.materials.iter().any(Option::is_some) {
                        drawable += 1;
                    }
                }
            }
        }
        let runners = s.defs.iter().flat_map(|d| &d.elems).filter(|e| e.runs.iter().any(Option::is_some)).count();
        let deaths = s.defs.iter().flat_map(|d| &d.elems).filter(|e| e.death.is_some()).count();
        let children: usize = s.defs.iter().flat_map(|d| &d.elems).map(|e| e.run_names.len() + usize::from(e.death_name.is_some())).sum();
        let found: usize = s.defs.iter().flat_map(|d| &d.elems).map(|e| e.runs.iter().flatten().count() + usize::from(e.death.is_some())).sum();
        let (state, mode, exploder) = left_out(&map.entities);
        s.report.push(format!(
            "t7 fx: exploders: {exploder} placed effects wait for an exploder ({}), {mode} bound to a game mode, {state} off in lighting state 1",
            if exploders_fired() { "played from the start: BO2MP_T7_FX_EXPLODERS=1" } else { "their script fires it; not played unless BO2MP_T7_FX_EXPLODERS=1" }
        ));
        let n = |t: u8| types.get(&t).copied().unwrap_or(0);
        s.report.push(format!(
            "t7 fx: tails: {} tail and {} line elements {} (IW4's tail geometry, wound as the billboards; BO2MP_T7_FX_TAILS=0 turns them off); trails: {} not drawn (their FxTrailDef strips are not built)",
            n(elem_type::TAIL),
            n(elem_type::LINE),
            if tails_on() { "drawn as velocity-aligned quads" } else { "not drawn" },
            n(elem_type::TRAIL)
        ));
        s.report.push(format!(
            "t7 fx: models: {} model elements naming {} XModels {}",
            n(elem_type::MODEL),
            s.model_names.len(),
            if models_on() { "drawn as G-buffer model instances (BO2MP_T7_FX_MODELS=1, unverified)" } else { "not drawn (BO2MP_T7_FX_MODELS=1 draws them, unverified)" }
        ));
        s.report.push(format!("t7 fx: lens flares: {} lens-flare elements not drawn (the manager's per-frame source list is not decompiled); decals: {}", n(elem_type::LENS_FLARE), n(elem_type::DECAL)));
        s.report.push(format!("t7 fx: lighting: lit sprites take lmapTex = 1.0 unless BO2MP_T7_FX_LIGHT=1 (fx_compute_sprite_light_shadow); {} light elements go to the scene's lights", n(elem_type::LIGHT)));
        s.report.push(format!("t7 fx: runners: {runners} runner elements and {deaths} with a death effect play child effects ({found} of {children} named effects found, nested to {MAX_DEPTH})"));
        s.report.push(format!(
            "t7 fx: {} placed effects ({} effect names not found), {} effect defs, {elems} elements (by type {types:?}), {drawable} sprite elements drawable ({mats_ok} of {mats} sprite visuals have a material with a forward technique); trails, models, decals and lens flares not drawn by render_t7 yet",
            s.placed.len(),
            missing,
            s.defs.len()
        ));
        s.cull_far = (0..s.defs.len()).map(|d| def_far(&s, d)).collect();
        let culled = s.cull_far.iter().filter(|f| f.def.is_finite()).count();
        s.report.push(format!("t7 fx: {culled} of {} effect defs are left out beyond their sprites' far fades (T7_FX_NO_CULL=1: never)", s.defs.len()));
        s
    }

    /// A copy of the defs and placements (checks: the same scene with
    /// another cut).
    pub fn clone_for_check(&self) -> Self {
        Self { defs: self.defs.clone(), placed: self.placed.clone(), report: Vec::new(), model_names: self.model_names.clone(), cull_far: self.cull_far.clone() }
    }

    /// Whether placed effect `p` can draw a sprite seen from `eye`.
    fn in_reach(&self, p: &Placed, eye: [f32; 3]) -> bool {
        if fx_no_cull() {
            return true;
        }
        let Some(far) = self.cull_far.get(p.def).map(|c| c.def) else { return true };
        if !far.is_finite() {
            return true;
        }
        let d2 = (0..3).map(|k| (p.origin[k] - eye[k]).powi(2)).sum::<f32>();
        d2 <= far * far
    }

    /// Whether element `ei` of placed effect `p` can draw a sprite seen
    /// from `eye` ([`FarCut::elems`]).
    fn elem_in_reach(&self, p: &Placed, ei: usize, eye: [f32; 3]) -> bool {
        let Some(&far) = self.cull_far.get(p.def).and_then(|c| c.elems.get(ei)) else { return true };
        if !far.is_finite() || fx_no_cull() {
            return true;
        }
        let d2 = (0..3).map(|k| (p.origin[k] - eye[k]).powi(2)).sum::<f32>();
        d2 <= far * far
    }

    /// The sprites alive at `now_ms` seen from `eye`, back to front.
    pub fn sprites(&self, now_ms: i32, eye: [f32; 3], out: &mut Vec<Sprite>) {
        self.sprites_into(now_ms, eye, out, &mut SortScratch::default());
    }

    /// [`Self::sprites`] with the sort's scratch kept by the caller (no
    /// allocation once the vectors have grown).
    pub fn sprites_into(&self, now_ms: i32, eye: [f32; 3], out: &mut Vec<Sprite>, scratch: &mut SortScratch) {
        out.clear();
        let mut sink = Sink { sprites: out, models: None };
        for p in &self.placed {
            if !self.in_reach(p, eye) {
                continue;
            }
            self.effect(p, now_ms, eye, &mut sink, 0);
            if sink.sprites.len() >= MAX_SPRITES {
                break;
            }
        }
        out.truncate(MAX_SPRITES);
        // (Back to front, then by sort byte; ties kept in evaluation order:
        // the order a stable sort gives, by index.)
        let order = &mut scratch.order;
        order.clear();
        order.extend(0..out.len() as u32);
        order.sort_unstable_by(|&a, &b| {
            let (x, y) = (&out[a as usize], &out[b as usize]);
            y.dist.total_cmp(&x.dist).then(x.sort.cmp(&y.sort)).then(a.cmp(&b))
        });
        scratch.sorted.clear();
        scratch.sorted.extend(order.iter().map(|&i| out[i as usize]));
        std::mem::swap(out, &mut scratch.sorted);
    }

    /// One placed effect's sprites (and its children's) at `now_ms`, in
    /// evaluation order (checks: `fx_check`).
    pub fn placed_sprites(&self, placed: usize, now_ms: i32, eye: [f32; 3], out: &mut Vec<Sprite>) {
        out.clear();
        let mut sink = Sink { sprites: out, models: None };
        self.effect(&self.placed[placed], now_ms, eye, &mut sink, 0);
    }

    /// The model element particles alive at `now_ms` seen from `eye`,
    /// nearest first (sprites are not evaluated).
    pub fn models(&self, now_ms: i32, eye: [f32; 3], out: &mut Vec<ModelInst>) {
        out.clear();
        if self.model_names.is_empty() {
            return;
        }
        let mut none = Vec::new();
        let mut sink = Sink { sprites: &mut none, models: Some(out) };
        for p in &self.placed {
            self.effect(p, now_ms, eye, &mut sink, 0);
        }
        out.sort_by(|a, b| a.dist.total_cmp(&b.dist));
    }

    /// One playing effect's sprites, and those of the effects its runners
    /// and dying particles started (to `MAX_DEPTH`).
    fn effect(&self, p: &Placed, now_ms: i32, eye: [f32; 3], sink: &mut Sink, depth: u8) {
        let def = &self.defs[p.def];
        // (A particle's age is the time since it began times the effect's
        // time scale, `FUN_14157cc80`.)
        let age = (f64::from(now_ms - p.start_ms) * f64::from(p.timescale)) as i32;
        if age < 0 || age >= def.duration {
            return;
        }
        for (ei, e) in def.elems.iter().enumerate() {
            if sink.sprites.len() >= MAX_SPRITES {
                return;
            }
            if ei >= def.looping + def.one_shot {
                continue;
            }
            if sink.models.is_none() && e.sprite() && e.materials.iter().any(Option::is_some) && self.elem_in_reach(p, ei, eye) {
                let out = &mut *sink.sprites;
                if ei < def.looping {
                    each_looping(def, ei, e, age, |k, at| particle(p, ei, e, k, at, age, eye, out));
                } else {
                    for k in 0..one_shot_count(p, e) {
                        particle(p, ei, e, k, 0, age, eye, out);
                    }
                }
            }
            if let Some(models) = sink.models.as_deref_mut()
                && e.ty == elem_type::MODEL
                && e.models.iter().any(Option::is_some)
            {
                if ei < def.looping {
                    each_looping(def, ei, e, age, |k, at| model_particle(p, ei, e, k, at, age, eye, models));
                } else {
                    for k in 0..one_shot_count(p, e) {
                        model_particle(p, ei, e, k, 0, age, eye, models);
                    }
                }
            }
            if depth < MAX_DEPTH && (e.runs.iter().any(Option::is_some) || e.death.is_some()) {
                self.children(p, def, ei, e, age, now_ms, eye, sink, depth);
            }
        }
    }

    /// The effects an element's particles started that still play: a
    /// runner (type 14) plays one of its effects where and when each
    /// particle spawns, in the particle's orientation (IW4's runner); any
    /// element with a death effect plays it where each particle dies.
    #[allow(clippy::too_many_arguments)]
    fn children(&self, p: &Placed, def: &Def, ei: usize, e: &Elem, age: i32, now_ms: i32, eye: [f32; 3], out: &mut Sink, depth: u8) {
        let child_max = e.runs.iter().flatten().chain(e.death.iter()).map(|&d| self.defs[d].duration).max().unwrap_or(0);
        let window = (e.life.0 + e.life.1.max(0)).saturating_add(e.delay.0 + e.delay.1.max(0)).saturating_add(child_max);
        let mut spawns: Vec<(u32, i32)> = Vec::new();
        if ei < def.looping {
            let (interval, count) = (e.spawn.0.max(1), e.spawn.1.max(1));
            let life_stop = if def.msec_looping_life > 0 { def.msec_looping_life / interval } else { i32::MAX };
            let last = (age / interval).min(life_stop).min(count - 1);
            let first = if window == i32::MAX { 0 } else { ((age - window) / interval).max(0) };
            for k in (first..=last).rev().take(MAX_CHILDREN) {
                spawns.push((k as u32, k * interval));
            }
        } else {
            spawns.extend((0..one_shot_count(p, e).min(MAX_CHILDREN as u32)).map(|k| (k, 0)));
        }
        for (seq, spawn_ms) in spawns {
            let (seed, delay, life) = particle_life(p, ei, e, seq, spawn_ms);
            let child_seed = ((u32::from(p.seed) * 31 + seed * 7 + ei as u32) % crate::fx_rand::MOD) as u16;
            let at = |ms: i32| p.start_ms + (ms as f32 / p.timescale) as i32;
            if let Some(Some(d)) = e.runs.get(crate::fx_rand::visual_index(e.runs.len(), seed)) {
                let (origin, axis) = particle_at(p, e, seed, 0, life);
                let child = Placed { def: *d, origin, axis, start_ms: at(spawn_ms + delay), seed: child_seed, timescale: p.timescale };
                self.effect(&child, now_ms, eye, out, depth + 1);
            }
            if let Some(d) = e.death
                && age >= spawn_ms + delay + life
            {
                let (origin, axis) = particle_at(p, e, seed, life, life);
                let child = Placed { def: d, origin, axis, start_ms: at(spawn_ms + delay + life), seed: (child_seed ^ 0x55) % crate::fx_rand::MOD as u16, timescale: p.timescale };
                self.effect(&child, now_ms, eye, out, depth + 1);
            }
        }
    }
}

/// How many particles a one-shot element spawns.
fn one_shot_count(p: &Placed, e: &Elem) -> u32 {
    let n = e.spawn.0 + ((crate::fx_rand::f(u32::from(p.seed), crate::fx_rand::ch::COUNT) * (e.spawn.1 + 1) as f32) as i32);
    n.clamp(0, MAX_PER_ELEM as i32) as u32
}

/// Each looping particle that may be alive at `age`, newest first, to
/// `f(sequence, spawn ms)` (true: drawn; at most `MAX_PER_ELEM` drawn).
fn each_looping(def: &Def, ei: usize, e: &Elem, age: i32, mut f: impl FnMut(u32, i32) -> bool) {
    let _ = ei;
    // (IW4's `FxSpawnDefLooping`: one element every `interval` ms, `count`
    // in all, `i32::MAX` for no end; Nuk3town's are all `i32::MAX`.)
    let (interval, count) = (e.spawn.0.max(1), e.spawn.1.max(1));
    let life_max = (e.life.0 + e.life.1).max(1);
    let last = age / interval;
    let first = ((age - life_max - e.delay.0 - e.delay.1) / interval).max(0);
    let life_stop = if def.msec_looping_life > 0 { def.msec_looping_life / interval } else { i32::MAX };
    let stop = life_stop.min(count - 1);
    let mut drawn = 0;
    let first = first.max(last.min(stop) - 4 * MAX_PER_ELEM as i32);
    for k in (first..=last.min(stop)).rev() {
        if drawn >= MAX_PER_ELEM {
            return;
        }
        if f(k as u32, k * interval) {
            drawn += 1;
        }
    }
}

/// The record's atlas word (0x14: cell | next cell << 8 | blend << 16 |
/// column bits << 24 | row bits << 28) from the def's atlas bytes (+0xbc:
/// behaviour, index, fps, loops, column bits, row bits, range count) as the
/// exe's `FUN_14156fce0` makes it: behaviour bit 0x80 on (else 0, the whole
/// picture); start (bits 0..1) fixed, random (`rand_cell`: a 16-bit random
/// times the cells >> 16), by sequence, or 3: a range of byte 6 cells from
/// the index; 0x04 plays the cells over the life, else byte 2 frames a
/// second; 0x08 stops at the last cell after byte 3 loops; 0x10 blends to
/// the next cell; 0x20 plays backwards.
fn atlas_word(a: &[u8; 8], rand_cell: u32, seq: u8, age_ms: f32, t: f32) -> u32 {
    let b = a[0];
    if b & 0x80 == 0 {
        return 0;
    }
    let cells = 1u32 << (u32::from(a[4]) + u32::from(a[5])).min(31);
    let mask = cells.wrapping_sub(1);
    let mut count = cells;
    let start = match b & 3 {
        0 => u32::from(a[1]),
        1 => (rand_cell * cells) >> 16,
        3 => {
            count = u32::from(a[6]);
            0
        }
        _ => u32::from(seq) & mask,
    };
    let mut frame = start as f32;
    if b & 4 != 0 {
        frame += count as f32 * t;
    } else if a[2] != 0 {
        frame += f32::from(a[2]) * age_ms / 1000.0;
    }
    let fixed = (frame * 256.0) as i32;
    let mut blend = (fixed & 0xff) as u8;
    let (mut lo, mut hi) = (fixed >> 8, (fixed + 0xff) >> 8);
    if b & 8 != 0 {
        let end = i32::from(a[3]) * count as i32;
        if lo < end {
            if end <= hi {
                hi = count as i32 - 1;
            }
        } else {
            lo = count as i32 - 1;
            hi = lo;
            blend = 0;
        }
    }
    if b & 3 == 3 && count > 0 {
        lo = lo.rem_euclid(count as i32) + i32::from(a[1]);
        hi = hi.rem_euclid(count as i32) + i32::from(a[1]);
    }
    if b & 0x10 == 0 {
        blend = 0;
    }
    let mut c0 = (lo as u32 & mask) as u8;
    let mut c1 = (if b & 0x10 != 0 { hi } else { lo } as u32 & mask) as u8;
    if b & 0x20 != 0 {
        blend = !blend;
        (c0, c1) = ((cells as u8).wrapping_sub(c1).wrapping_sub(1), (cells as u8).wrapping_sub(c0).wrapping_sub(1));
    }
    u32::from(c0) | u32::from(c1) << 8 | u32::from(blend) << 16 | u32::from(a[4] & 0xf) << 24 | u32::from(a[5]) << 28
}

/// A particle's visual state at `t` (0 .. 1 of its life) as the exe
/// evaluates it (`FUN_14157cc80`, `FUN_141578bd0`, `FUN_141573090`): the
/// interval `i = min(floor(n t), n - 1)` and `frac`; colour per byte
/// `lerp(base, far, r23)` of both samples; sizes `base + r26 (r27) x
/// amplitude`, the second only with def flag 0x10000000; rotation (r24,
/// r25); emissive `base + r29 x amplitude`; model scale (+0x14, r28).
#[derive(Clone, Copy, Debug)]
pub struct VisState {
    pub rgba: [u8; 4],
    pub size: [f32; 2],
    pub rotation: f32,
    pub emissive: f32,
    pub scale: f32,
}

pub fn vis_state(e: &Elem, t: f32, seed: u32, life: f32) -> Option<VisState> {
    use crate::fx_rand::{ch, f};
    let (i, frac) = crate::fx_rand::sample_point(e.vis_intervals, t);
    let s0 = e.vis.get(i * IW4_VIS..(i + 1) * IW4_VIS)?;
    let s1 = e.vis.get((i + 1) * IW4_VIS..(i + 2) * IW4_VIS).unwrap_or(s0);
    let g = |s: &[u8], o: usize| f32_at(s, o);
    let lerp = |o: usize, r: f32| (g(s0, o) + r * g(s0, o + 24)) * (1.0 - frac) + (g(s1, o) + r * g(s1, o + 24)) * frac;
    let rc = f(seed, ch::COLOUR);
    let rgba: [u8; 4] = core::array::from_fn(|c| {
        let v = |s: &[u8]| f32::from(s[c]) * (1.0 - rc) + f32::from(s[24 + c]) * rc;
        (v(s0) * (1.0 - frac) + v(s1) * frac).round().clamp(0.0, 255.0) as u8
    });
    let size0 = lerp(12, f(seed, ch::SIZE0));
    let size1 = if e.flags & NON_UNIFORM_SIZE != 0 || matches!(e.ty, elem_type::TAIL | elem_type::LINE) { lerp(16, f(seed, ch::SIZE1)) } else { size0 };
    let rotation = crate::fx_rand::rotation(s0, s1, frac, life, e.initial_rotation, seed);
    let re = f(seed, ch::EMISSIVE);
    let em = |k: usize| e.emissive.get(k.min(e.emissive.len().saturating_sub(1))).map_or(0.0, |w| w[0] + w[1] * re);
    let emissive = em(i) * (1.0 - frac) + em(i + 1) * frac;
    let scale = lerp(20, f(seed, ch::SCALE));
    Some(VisState { rgba, size: [size0, size1], rotation, emissive, scale })
}

fn pack_axis(v: [f32; 3]) -> u32 {
    // (The program reads `x * 2.00196 - 1.00196`: unorm 10 bits.)
    let q = |x: f32| (((x + 1.00196) / 2.00196).clamp(0.0, 1.0) * 1023.0).round() as u32;
    q(v[0]) | q(v[1]) << 10 | q(v[2]) << 20
}

/// A particle's random seed, delay and life span (ms).
///
/// (The seed as the exe's `FUN_14157cc80`: sequence byte x 500 + the
/// effect's seed + the msec it spawned at, mod 809; the spawn msec on the
/// effect's own clock plus the element's index (elements of one effect
/// spawning together keep apart; the exe's absolute game time differs per
/// run anyway).)
fn particle_life(p: &Placed, ei: usize, e: &Elem, seq: u32, spawn_ms: i32) -> (u32, i32, i32) {
    use crate::fx_rand::{ch, f};
    let seed = crate::fx_rand::elem_seed(p.seed, (seq & 0xff) as u8, p.start_ms.wrapping_add(spawn_ms).wrapping_add(ei as i32 * 97).wrapping_add((seq >> 8) as i32));
    let delay = e.delay.0 + ((f(seed, ch::DELAY) * e.delay.1 as f32) as i32);
    let life = crate::fx_rand::life(e.life.0, e.life.1, seed).max(1);
    (seed, delay, life)
}

/// Where a particle is `elem_age` ms into its life, and its orientation:
/// its spawn place, then its velocity graphs and gravity.
fn particle_at(p: &Placed, e: &Elem, seed: u32, elem_age: i32, life: i32) -> ([f32; 3], [[f32; 3]; 3]) {
    let t = elem_age as f32 / life as f32;
    let mut pos = crate::fx_rand::spawn_origin(p.origin, p.axis, e.spawn_origin, e.flags, e.offset_radius, e.offset_height, e.cylinder_axis, seed);
    let angles = crate::fx_rand::angles(e.spawn_angles, e.angular_velocity, seed, elem_age as f32);
    let axis = fx_iw4::mat3_mul(fx_iw4::angles_to_axis_radians(angles), p.axis);
    if e.flags & 0x0100_0000 != 0 && e.vel_local.len() >= 2 {
        let d = crate::fx_rand::travel(&e.vel_local, t, life as f32, seed);
        for a in 0..3 {
            pos[a] += axis[0][a] * d[0] + axis[1][a] * d[1] + axis[2][a] * d[2];
        }
    }
    if e.flags & 0x0200_0000 != 0 && e.vel_world.len() >= 2 {
        let d = crate::fx_rand::travel(&e.vel_world, t, life as f32, seed);
        for a in 0..3 {
            pos[a] += d[a];
        }
    }
    let secs = elem_age as f32 * 0.001;
    pos[2] -= 0.5 * crate::fx_rand::gravity(e.gravity, seed) * secs * secs;
    (pos, axis)
}

/// `pos` moved `by` units toward the eye.
fn toward_eye(pos: [f32; 3], eye: [f32; 3], by: f32) -> [f32; 3] {
    if by == 0.0 {
        return pos;
    }
    let d = glam::Vec3::from(eye) - glam::Vec3::from(pos);
    let len = d.length();
    let k = if len > 0.0 { by / len } else { by };
    (glam::Vec3::from(pos) + d * k).to_array()
}

fn fx_no_cull() -> bool {
    static OFF: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *OFF.get_or_init(|| std::env::var("T7_FX_NO_CULL").is_ok_and(|v| !v.is_empty() && v != "0"))
}

/// [`FxScene::cull_far`] of def `d`: the largest of its sprite elements'
/// far ends (fade out end, else spawn range end) plus their reach; infinite
/// when an element has no far end, plays child effects (runners, death
/// effects) or the def is nested too deep to tell.
fn def_far(s: &FxScene, d: usize) -> FarCut {
    let Some(def) = s.defs.get(d) else { return FarCut { def: f32::INFINITY, elems: Vec::new() } };
    let elems: Vec<f32> = def.elems.iter().map(elem_far).collect();
    let def_far = def.elems.iter().enumerate().filter(|(ei, _)| *ei < def.looping + def.one_shot).map(|(ei, _)| elems[ei]).fold(0.0f32, f32::max);
    FarCut { def: def_far, elems }
}

/// One element's far cut: 0 when it draws no sprite and starts no effect;
/// infinite when it starts effects or has no far end.
fn elem_far(e: &Elem) -> f32 {
    if e.runs.iter().any(Option::is_some) || e.death.is_some() {
        return f32::INFINITY;
    }
    if !(e.sprite() && e.materials.iter().any(Option::is_some)) {
        return 0.0;
    }
    let end = if e.fade_out[1] > 0.0 && e.fade_out[0] >= 0.0 {
        e.fade_out[0] + e.fade_out[1]
    } else if e.spawn_range[1] > 0.0 {
        e.spawn_range[0] + e.spawn_range[1]
    } else {
        return f32::INFINITY;
    };
    let reach = crate::fx_check::elem_reach(e);
    if !end.is_finite() || !reach.is_finite() {
        return f32::INFINITY;
    }
    // (A little over: the reach is a bound on the sim's own numbers.)
    (end + reach) * 1.05 + 64.0
}

/// The distance fades as `FUN_14157cc80` takes them: fade out (+0x20
/// start, +0x24 length: 1 nearer than the start, 0 past start + length),
/// fade in (+0x28, +0x2c: 0 nearer, 1 past), each only when its length is
/// not 0, the smaller of the two.
pub fn distance_fade(e: &Elem, dist: f32) -> f32 {
    if e.fade_out[1] == 0.0 && e.fade_in[1] == 0.0 {
        return 1.0;
    }
    let mut out = 1.0f32;
    if e.fade_out[1] != 0.0 {
        let d = dist - e.fade_out[0];
        if d >= 0.0 {
            out = if d < e.fade_out[1] { 1.0 - d / e.fade_out[1] } else { 0.0 };
        }
    }
    let mut inn = 1.0f32;
    if e.fade_in[1] != 0.0 {
        let d = dist - e.fade_in[0];
        let far = if d < 0.0 { 1.0 } else if d < e.fade_in[1] { 1.0 - d / e.fade_in[1] } else { 0.0 };
        inn = 1.0 - far;
    }
    out.min(inn)
}

/// One particle, spawned `spawn_ms` after the effect began; true when drawn.
#[allow(clippy::too_many_arguments)]
fn particle(p: &Placed, ei: usize, e: &Elem, seq: u32, spawn_ms: i32, age: i32, eye: [f32; 3], out: &mut Vec<Sprite>) -> bool {
    let (seed, delay, life) = particle_life(p, ei, e, seq, spawn_ms);
    let elem_age = age - spawn_ms - delay;
    if elem_age < 0 || elem_age >= life {
        return false;
    }
    let Some(material) = e.materials.get(crate::fx_rand::visual_index(e.materials.len(), seed)).copied().flatten() else { return false };
    let t = elem_age as f32 / life as f32;
    let (mut pos, axis) = particle_at(p, e, seed, elem_age, life);
    // (Pulled toward the eye by the def's camera offset, +0x1f0, before the
    // fades measure it: `FUN_14157cc80`.)
    pos = toward_eye(pos, eye, e.camera_offset);
    let dist = ((pos[0] - eye[0]).powi(2) + (pos[1] - eye[1]).powi(2) + (pos[2] - eye[2]).powi(2)).sqrt();
    // The spawn range and the near / far fades.
    if e.spawn_range[1] > 0.0 && (dist < e.spawn_range[0] || dist > e.spawn_range[0] + e.spawn_range[1]) {
        return false;
    }
    let fade = distance_fade(e, dist);
    // (The dispatcher `FUN_141578d80` draws a particle only when its fade is
    // at least 4/255; the colour's alpha is not tested.)
    if fade < 4.0 / 255.0 {
        return false;
    }
    let Some(vs) = vis_state(e, t, seed, life as f32) else { return false };
    let mut rgba = vs.rgba;
    rgba[3] = (f32::from(rgba[3]) * fade).round() as u8;
    let [size0, size1] = vs.size;
    if size0 <= 0.0 && size1 <= 0.0 {
        return false;
    }
    let rotation = vs.rotation;
    // (A sample's emissive word can be slightly negative from its amplitude: no light is taken away.)
    let emissive = vs.emissive.max(0.0);
    // The atlas cell.
    let rand_cell = u32::from(crate::fx_rand::u16(seed, crate::fx_rand::ch::ATLAS));
    let atlas_word = atlas_word(&e.atlas_t7, rand_cell, (seq & 0xff) as u8, elem_age as f32, t);
    // A tail (3) is a quad along the particle's velocity, size0 wide and
    // 2 x size1 long, trailing behind it; a line (4) runs ahead of it (IW4's
    // and Black Ops II's tail, `fx_iw4::tail_*`; not read off Black Ops
    // III's exe). Drawn with the own-axes flag: width across the velocity
    // and the view, length along the velocity.
    let mut pos = pos;
    // (The exe's per-type draw table at 0x142ec75b0: billboards (0) pass no
    // axes, the record's flags then being the def's bits 0x100000 (1: face
    // the eye) and 0x80000 (2: z-up); oriented sprites (1) pass the
    // particle's axes, rotated sprites (2) the axes of the def's spawn
    // angles alone (`FUN_141575410` -> `FUN_1415d56a0`), both with flag 4.)
    let (flags, a, b) = match e.ty {
        // (A = axis 2, B = axis 1: width along the particle's left, height
        // along its up, as the exe packs them (record 0x20 = axis row 2,
        // 0x24 = row 1), and the quad's normal U x V = -axis 0, which faces
        // away from an eye in front of it as a billboard's does.)
        elem_type::ORIENTED => (4u16, axis[2], axis[1]),
        elem_type::ROTATED => {
            let (_, spawn_axis) = particle_at(p, e, seed, 0, life);
            (4u16, spawn_axis[2], spawn_axis[1])
        }
        elem_type::TAIL | elem_type::LINE => {
            let (p0, _) = particle_at(p, e, seed, (elem_age - 8).max(0), life);
            let (p1, _) = particle_at(p, e, seed, (elem_age + 8).min(life), life);
            let v = glam::Vec3::from(p1) - glam::Vec3::from(p0);
            let at = glam::Vec3::from(pos);
            let Some(dir) = v.try_normalize() else { return false };
            // (Across = to-eye x velocity, so U x V = (across x dir) points away
            // from the eye as a billboard's does: the quad's winding then
            // matches the billboards' and survives the materials' culling.)
            let Some(across) = (glam::Vec3::from(eye) - at).cross(dir).try_normalize() else { return false };
            let ahead = if e.ty == elem_type::LINE { 1.0 } else { -1.0 };
            pos = (at + dir * size1 * ahead).to_array();
            (4u16, (-dir).to_array(), across.to_array())
        }
        _ => (u16::from(e.flags & 0x10_0000 != 0) | if e.flags & 0x8_0000 != 0 { 2 } else { 0 }, [0.0; 3], [0.0; 3]),
    };
    let mut r = [0u8; SPRITE_RECORD];
    let put = |r: &mut [u8; SPRITE_RECORD], at: usize, b: &[u8]| r[at..at + b.len()].copy_from_slice(b);
    for (k, v) in pos.iter().chain([1.0f32].iter()).enumerate() {
        put(&mut r, 4 * k, &v.to_le_bytes());
    }
    // (The colour bytes as authored: no program decodes them and the named
    // colours read right only so, see docs/T7_VISUAL_PARITY.md.)
    put(&mut r, 0x10, &rgba);
    put(&mut r, 0x14, &atlas_word.to_le_bytes());
    put(&mut r, 0x18, &e.angle_fade.to_le_bytes());
    put(&mut r, 0x1c, &u32::from(half(emissive)).to_le_bytes());
    put(&mut r, 0x20, &pack_axis(a).to_le_bytes());
    put(&mut r, 0x24, &pack_axis(b).to_le_bytes());
    let pivot = if matches!(e.ty, elem_type::TAIL | elem_type::LINE) { [0.0; 2] } else { e.pivot };
    for (k, v) in [pivot[0], pivot[1], size0, size1].into_iter().enumerate() {
        put(&mut r, 0x28 + 4 * k, &v.to_le_bytes());
    }
    let rot = ((rotation / std::f32::consts::TAU).rem_euclid(1.0) * 65536.0) as u32 as u16;
    // (Alpha mode 256 / the def's alpha scale (+0x230, at least 1e-4; 256:
    // plain alpha, more: the pixel program's alpha threshold), soft depth 16
    // x +0x234, both clamped to 0xffff as the exe's writer does.)
    let alpha_mode = (256.0 / e.alpha_scale.max(1e-4)).min(65535.0) as u16;
    let soft = (e.soft * 16.0).clamp(0.0, 65535.0) as u16;
    for (k, v) in [alpha_mode, soft, flags, rot].into_iter().enumerate() {
        put(&mut r, 0x38 + 2 * k, &v.to_le_bytes());
    }
    out.push(Sprite { material, dist, sort: e.sort, record: r, def: p.def as u32, elem: ei as u16 });
    true
}

/// One model element particle (7): its visual's XModel at the particle's
/// place in its orientation, scaled by the visual samples' scale (+0x14,
/// r28) times the effect's (1 here), not drawn when that is 0 or less (the
/// exe's model draw `FUN_141575ca0`); true when alive.
#[allow(clippy::too_many_arguments)]
fn model_particle(p: &Placed, ei: usize, e: &Elem, seq: u32, spawn_ms: i32, age: i32, eye: [f32; 3], out: &mut Vec<ModelInst>) -> bool {
    let (seed, delay, life) = particle_life(p, ei, e, seq, spawn_ms);
    let elem_age = age - spawn_ms - delay;
    if elem_age < 0 || elem_age >= life {
        return false;
    }
    let Some(model) = e.models.get(crate::fx_rand::visual_index(e.models.len(), seed)).copied().flatten() else { return false };
    let (pos, axis) = particle_at(p, e, seed, elem_age, life);
    let dist = ((pos[0] - eye[0]).powi(2) + (pos[1] - eye[1]).powi(2) + (pos[2] - eye[2]).powi(2)).sqrt();
    if e.spawn_range[1] > 0.0 && (dist < e.spawn_range[0] || dist > e.spawn_range[0] + e.spawn_range[1]) {
        return false;
    }
    let scale = vis_state(e, elem_age as f32 / life as f32, seed, life as f32).map_or(1.0, |v| v.scale);
    if !(scale > 0.0) {
        return false;
    }
    let mut bone = [0.0f32; 16];
    for row in 0..3 {
        for col in 0..3 {
            bone[4 * row + col] = axis[col][row] * scale;
        }
        bone[4 * row + 3] = pos[row];
    }
    out.push(ModelInst { model, dist, bone });
    true
}

/// f32 to IEEE half (round to nearest, no NaN payloads).
pub(crate) fn half(x: f32) -> u16 {
    let b = x.to_bits();
    let sign = ((b >> 16) & 0x8000) as u16;
    let exp = ((b >> 23) & 0xff) as i32 - 127 + 15;
    if x.is_nan() {
        return 0x7e00;
    }
    if exp >= 31 {
        return sign | 0x7c00;
    }
    if exp <= 0 {
        return sign;
    }
    let mant = (b >> 13) & 0x3ff;
    let round = (b >> 12) & 1;
    (u32::from(sign) | (exp as u32) << 10 | mant).wrapping_add(round) as u16
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The staged Nuk3town's placed effects evaluated (no GPU):
    /// `cargo test -p render_t7 --lib fx::tests::staged -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn staged_nuketown_sprites() {
        let zone = std::path::Path::new("/mnt/user-data/uploads/bo3_game_files/zone/mp_nuketown_x.ff");
        let Ok(map) = fastfile_t7::map::T7Map::load(&fastfile_t7::map::MapSources::beside(zone)) else { return };
        let mut names: Vec<String> = Vec::new();
        let s = FxScene::build(&map, &mut |n| {
            Some(names.iter().position(|m| m == n).unwrap_or_else(|| {
                names.push(n.to_owned());
                names.len() - 1
            }))
        });
        for l in &s.report {
            println!("{l}");
        }
        let mut out = Vec::new();
        for t in [0, 1000, 5000, 20000] {
            s.sprites(t, [0.0, 0.0, 100.0], &mut out);
            let mats: std::collections::HashSet<usize> = out.iter().map(|x| x.material).collect();
            println!("t7 fx: at {t} ms: {} sprites, {} materials", out.len(), mats.len());
        }
        assert!(!out.is_empty());
    }

    fn test_sprite_elem(ty: u8) -> Elem {
        let mut vis = Vec::new();
        for _ in 0..2 {
            let mut s = [0u8; T7_VIS];
            s[..4].copy_from_slice(&[255, 128, 64, 255]);
            s[40..44].copy_from_slice(&[255, 128, 64, 255]);
            s[12..16].copy_from_slice(&8.0f32.to_le_bytes());
            s[16..20].copy_from_slice(&8.0f32.to_le_bytes());
            vis.extend_from_slice(&vis_sample(&s).0);
        }
        Elem { ty, life: (1000, 0), vis, emissive: vec![[0.0; 2]; 2], vis_intervals: 1, materials: vec![Some(3)], atlas: atlas(&[]), alpha_scale: 1.0, ..Default::default() }
    }

    #[test]
    fn the_record_takes_the_defs_draw_words_as_the_exe_writes_them() {
        // A rotated sprite (2) pitched 90 degrees by its spawn angles lies
        // flat (own axes, flag 4); pivot, alpha mode, soft depth and the
        // view-angle fade come from the def (`FUN_141573090`).
        let mut e = test_sprite_elem(elem_type::ROTATED);
        e.spawn = (1, 0);
        e.spawn_angles = [[std::f32::consts::FRAC_PI_2, 0.0], [0.0; 2], [0.0; 2]];
        e.pivot = [0.5, -0.25];
        e.alpha_scale = 0.5;
        e.soft = 20.0;
        e.angle_fade = 65 | 85 << 16;
        let mut def = Def { name: "r".into(), looping: 0, one_shot: 1, msec_looping_life: 0, elems: vec![e.clone()], duration: 0 };
        def.duration = def.duration();
        let s = FxScene { defs: vec![def], placed: vec![Placed { def: 0, origin: [0.0; 3], axis: axis_from_angles([0.0; 3]), start_ms: 0, seed: 1, timescale: 1.0 }], report: vec![], model_names: vec![], cull_far: vec![] };
        let mut out = Vec::new();
        s.sprites(500, [0.0, 0.0, 300.0], &mut out);
        assert_eq!(out.len(), 1);
        let r = &out[0].record;
        let h = |o: usize| u16::from_le_bytes([r[o], r[o + 1]]);
        assert_eq!((h(0x38), h(0x3a), h(0x3c)), (512, 320, 4));
        assert_eq!(u32::from_le_bytes(r[0x18..0x1c].try_into().unwrap()), 65 | 85 << 16);
        assert_eq!((f32_at(r, 0x28), f32_at(r, 0x2c)), (0.5, -0.25));
        // Both axes horizontal: the quad lies flat, facing up or down.
        let un = |v: u32| (v >> 20 & 1023) as f32 / 1023.0 * 2.00196 - 1.00196;
        for at in [0x20, 0x24] {
            assert!(un(u32::from_le_bytes(r[at..at + 4].try_into().unwrap())).abs() < 0.01, "axis z at {at:#x}");
        }
        // A billboard takes its flags from the def's bits.
        let mut b = test_sprite_elem(elem_type::BILLBOARD);
        b.spawn = (1, 0);
        b.flags = 0x10_0000 | 0x8_0000;
        let mut def = Def { name: "b".into(), looping: 0, one_shot: 1, msec_looping_life: 0, elems: vec![b], duration: 0 };
        def.duration = def.duration();
        let s = FxScene { defs: vec![def], placed: vec![Placed { def: 0, origin: [0.0; 3], axis: axis_from_angles([0.0; 3]), start_ms: 0, seed: 1, timescale: 1.0 }], report: vec![], model_names: vec![], cull_far: vec![] };
        s.sprites(500, [0.0, 0.0, 300.0], &mut out);
        assert_eq!(u16::from_le_bytes([out[0].record[0x3c], out[0].record[0x3d]]), 3);
    }

    #[test]
    fn a_runner_plays_its_effect_where_and_when_it_spawns() {
        // The child: one sprite living 1 s. The runner: one every 4 s.
        let mut child = Def { name: "child".into(), looping: 0, one_shot: 1, msec_looping_life: 0, elems: vec![Elem { spawn: (1, 0), ..test_sprite_elem(elem_type::BILLBOARD) }], duration: 0 };
        child.duration = child.duration();
        assert_eq!(child.duration, 1000);
        let runner = Elem { ty: elem_type::NAMED, spawn: (4000, i32::MAX), life: (1, 0), runs: vec![Some(1)], ..Default::default() };
        let mut def = Def { name: "runner".into(), looping: 1, one_shot: 0, msec_looping_life: 0, elems: vec![runner], duration: 0 };
        def.duration = def.duration();
        let s = FxScene { defs: vec![def, child], placed: vec![Placed { def: 0, origin: [10.0, 20.0, 30.0], axis: axis_from_angles([0.0; 3]), start_ms: 0, seed: 3, timescale: 1.0 }], report: vec![], model_names: vec![], cull_far: vec![] };
        let mut out = Vec::new();
        // 500 ms after the third spawn (8 s): its child is alive at the runner's place.
        s.sprites(8_500, [200.0, 0.0, 0.0], &mut out);
        assert_eq!(out.len(), 1);
        assert_eq!(&out[0].record[0..12], &[10.0f32, 20.0, 30.0].iter().flat_map(|v| v.to_le_bytes()).collect::<Vec<_>>()[..]);
        // 2 s after it: the child is gone and the next not spawned yet.
        s.sprites(10_000, [200.0, 0.0, 0.0], &mut out);
        assert!(out.is_empty());
    }

    #[test]
    fn a_tail_lies_along_its_velocity_behind_the_particle() {
        let mut e = test_sprite_elem(elem_type::TAIL);
        e.spawn = (1, 0);
        // World velocity +x, 100 units over its life.
        e.flags = 0x0200_0000;
        e.vel_world = vec![FxElemVec3Range { base: [100.0, 0.0, 0.0], amplitude: [0.0; 3] }, FxElemVec3Range { base: [100.0, 0.0, 0.0], amplitude: [0.0; 3] }];
        let mut def = Def { name: "t".into(), looping: 0, one_shot: 1, msec_looping_life: 0, elems: vec![e], duration: 0 };
        def.duration = def.duration();
        let s = FxScene { defs: vec![def], placed: vec![Placed { def: 0, origin: [0.0; 3], axis: axis_from_angles([0.0; 3]), start_ms: 0, seed: 1, timescale: 1.0 }], report: vec![], model_names: vec![], cull_far: vec![] };
        let mut out = Vec::new();
        s.sprites(500, [0.0, -300.0, 0.0], &mut out);
        assert_eq!(out.len(), 1);
        let r = &out[0].record;
        let un = |v: u32| (v & 1023) as f32 / 1023.0 * 2.00196 - 1.00196;
        let a = u32::from_le_bytes(r[0x20..0x24].try_into().unwrap());
        // Own axes; axis A against the velocity; the centre one size1 behind the head.
        assert_eq!(u16::from_le_bytes([r[0x3c], r[0x3d]]), 4);
        assert!((un(a) + 1.0).abs() < 0.01, "A.x {}", un(a));
        let head = f32_at(r, 0) + f32_at(r, 0x34);
        assert!(head > 0.0 && f32_at(r, 0) < head);
    }

    #[test]
    fn a_model_element_places_its_model_at_the_particle() {
        // One model particle living 1 s, the effect placed at (10, 20, 30)
        // turned 90 degrees in yaw: its bone's translation is the origin,
        // its rotation the placement's.
        let e = Elem { ty: elem_type::MODEL, spawn: (1, 0), life: (1000, 0), models: vec![Some(0)], model_names: vec!["m".into()], ..Default::default() };
        let mut def = Def { name: "t".into(), looping: 0, one_shot: 1, msec_looping_life: 0, elems: vec![e], duration: 0 };
        def.duration = def.duration();
        let s = FxScene { defs: vec![def], placed: vec![Placed { def: 0, origin: [10.0, 20.0, 30.0], axis: axis_from_angles([0.0, 90.0, 0.0]), start_ms: 0, seed: 1, timescale: 1.0 }], report: vec![], model_names: vec!["m".into()], cull_far: vec![] };
        let mut out = Vec::new();
        s.models(500, [0.0; 3], &mut out);
        assert_eq!(out.len(), 1);
        let b = out[0].bone;
        assert_eq!([b[3], b[7], b[11]], [10.0, 20.0, 30.0]);
        // Model +x (column 0) goes to world +y.
        assert!((b[0]).abs() < 1e-5 && (b[4] - 1.0).abs() < 1e-5, "{b:?}");
        // Gone after its life; no sprites from a model element.
        s.models(1500, [0.0; 3], &mut out);
        assert!(out.is_empty());
        let mut sprites = Vec::new();
        s.sprites(500, [0.0; 3], &mut sprites);
        assert!(sprites.is_empty());
    }

    #[test]
    fn atlas_words_follow_the_exes_writer() {
        // Nuk3town's de-rez flipbook: enabled, blend, stop after 1 loop, over
        // life, from cell 10 of 8 x 4.
        let a = [0x9c, 10, 0, 1, 3, 2, 22, 0];
        let w = atlas_word(&a, 0, 0, 0.0, 0.0);
        assert_eq!(w, 10 | 10 << 8 | 3 << 24 | 2 << 28);
        // Half way: 10 + 16 = 26, blending to 27 by 0 (whole cell).
        assert_eq!(atlas_word(&a, 0, 0, 0.0, 0.5) & 0xffffff, 26 | 26 << 8);
        // 10 + 32 x 0.3 = 19.6: cell 19, next 20, blend 0.6 x 256.
        let w = atlas_word(&a, 0, 0, 0.0, 0.3);
        assert_eq!((w & 0xff, w >> 8 & 0xff), (19, 20));
        assert!(((w >> 16 & 0xff) as i32 - 153).abs() <= 1, "{}", w >> 16 & 0xff);
        // Past the end: held on the last cell.
        assert_eq!(atlas_word(&a, 0, 0, 0.0, 0.9) & 0xffffff, 31 | 31 << 8);
        // Off without bit 0x80: the whole picture.
        assert_eq!(atlas_word(&[0x1c, 10, 0, 1, 3, 2, 22, 0], 0, 0, 0.0, 0.3), 0);
    }

    #[test]
    fn half_floats() {
        assert_eq!(half(0.0), 0);
        assert_eq!(half(1.0), 0x3c00);
        assert_eq!(half(-2.0), 0xc000);
        assert_eq!(half(0.5), 0x3800);
    }

    #[test]
    fn axis_packs_as_the_program_unpacks() {
        let w = pack_axis([1.0, 0.0, -1.0]);
        let un = |v: u32| (v & 1023) as f32 / 1023.0 * 2.00196 - 1.00196;
        assert!((un(w) - 1.0).abs() < 0.003);
        assert!(un(w >> 10).abs() < 0.003);
        assert!((un(w >> 20) + 1.0).abs() < 0.003);
    }

    #[test]
    fn a_looping_element_is_alive_from_time_alone() {
        // 4 per second, each living 1 s: about 4 alive at any time.
        let mut vis = Vec::new();
        for _ in 0..2 {
            let mut s = [0u8; T7_VIS];
            s[..4].copy_from_slice(&[255, 128, 64, 255]);
            s[40..44].copy_from_slice(&[255, 128, 64, 255]);
            s[12..16].copy_from_slice(&8.0f32.to_le_bytes());
            vis.extend_from_slice(&vis_sample(&s).0);
        }
        let e = Elem { ty: 0, spawn: (250, i32::MAX), life: (1000, 0), vis, emissive: vec![[0.0; 2]; 2], vis_intervals: 1, materials: vec![Some(3)], atlas: atlas(&[]), alpha_scale: 1.0, ..Default::default() };
        let mut def = Def { name: "t".into(), looping: 1, one_shot: 0, msec_looping_life: 0, elems: vec![e], duration: 0 };
        def.duration = def.duration();
        let s = FxScene { defs: vec![def], placed: vec![Placed { def: 0, origin: [0.0, 0.0, 0.0], axis: axis_from_angles([0.0; 3]), start_ms: -5000, seed: 7, timescale: 1.0 }], report: vec![], model_names: vec![], cull_far: vec![] };
        let mut out = Vec::new();
        s.sprites(10_000, [100.0, 0.0, 0.0], &mut out);
        assert_eq!(out.len(), 4, "alive: {}", out.len());
        let r = &out[0].record;
        assert_eq!(f32_at(r, 0x30), 8.0);
        assert_eq!(&r[0x38..0x3a], &256u16.to_le_bytes());
        assert_eq!(out[0].material, 3);
    }
}
