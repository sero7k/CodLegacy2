//! bo2mp: Call of Duty: Ghosts' particle, weapon-effect and createfx rules,
//! as `docs/exe_notes/IW6_VFX_AUDIO.md` (part A) sets them down from a static
//! reading of the game's executable. The element maths is the family's (as
//! `fx_iw4` and `fx_iw4::h1`); what Ghosts brings:
//!
//! * the element seed `(sequence * 10111 + effect seed + element def seed +
//!   spawn msec) mod 16361`, every random a lookup in a table of uniform
//!   floats on its own channel. Ghosts' own table was not extracted; any
//!   uniform table serves (the spec's note), so Modern Warfare Remastered's
//!   (`fx_iw4::h1`, the same seed range) stands in, on IW4's channels;
//! * the spawn delay and life drawn from that seed (`base + ((rand16 x
//!   (amplitude + 1)) >> 16)`), a sound element dropped when it would start
//!   more than 200 ms late, an expired element skipped unless it carries
//!   child effects;
//! * the 80-byte visual sample (colour rgba, unlit HDR scale, rotation delta
//!   and total, size x2, scale; base then amplitude): colour and unlit HDR
//!   scale lerp from base to amplitude with the colour random; size, scale
//!   and rotation are `base + random x amplitude` (the spec reads every
//!   channel as a lerp; Ghosts' data says otherwise for sizes and scales:
//!   on `mp_frag` 1747 of 2196 samples hold a size amplitude of 0 under a
//!   non-zero base, and every scale amplitude is 0, while colour and unlit
//!   scale amplitudes repeat their base, `docs/IW6.md`);
//! * the distance fades and draw threshold of the family (`h1`'s);
//! * the lit colour of lit types 3 and 4: `rgb x ((1 - b) x unlit x scene +
//!   b x light x litHDRScalar)`;
//! * a weapon's effect override chosen by how many of its attachments are
//!   on the gun, the flash and eject effect kinds by view and shot;
//! * a createfx placement loops or plays once by its placement, not by its
//!   effect: `playLoopedFx`'s repeat is its seconds in whole milliseconds,
//!   at least 1.
//!
//! Every function here is pure: the definition's bytes, the element's spawn
//! and the time in, the result out.

use crate::h1;

pub use crate::h1::{H1_RAND_MOD as IW6_RAND_MOD, distance_fade, display_rgb, draw_age, drawn};

/// The sequence's weight in an element's seed.
pub const IW6_SEED_SEQUENCE_MUL: u32 = 0x277f;

/// Random channels (index added to the seed; IW4's numbering, as
/// `h1::ch`: the channels are independent slots of the one table).
pub use crate::h1::ch;

/// `FxElemDef` (312 bytes) fields the rules read, by offset.
pub mod elem {
    pub const SIZE: usize = 312;
    pub const FLAGS: usize = 0;
    pub const FLAGS2: usize = 4;
    /// Looping: interval msec; one-shot: count base.
    pub const SPAWN_A: usize = 8;
    /// Looping: count; one-shot: count amplitude.
    pub const SPAWN_B: usize = 12;
    pub const FADE_IN: usize = 24;
    pub const FADE_OUT: usize = 32;
    pub const DELAY: usize = 44;
    pub const LIFE: usize = 52;
    pub const INITIAL_ROTATION: usize = 148;
    pub const GRAVITY: usize = 156;
    pub const ELEM_TYPE: usize = 180;
    pub const LIT_TYPE: usize = 181;
    pub const VISUAL_COUNT: usize = 182;
    pub const VEL_INTERVALS: usize = 183;
    pub const VIS_INTERVALS: usize = 184;
    pub const SORT_ORDER: usize = 288;
    pub const LIGHTING_FRAC: usize = 289;
    pub const RANDOM_SEED: usize = 292;
    pub const LIT_MAX_COLOR_CHANGE_PER_SEC: usize = 296;
    pub const UNLIT_HDR_SCALAR: usize = 300;
    pub const LIT_HDR_SCALAR: usize = 304;
    pub const LIT_UNLIT_BLEND: usize = 308;
}

/// `FxElemVisStateSample` (80 bytes): base, then amplitude at 40.
pub mod vis {
    pub const SIZE: usize = 80;
    pub const HALF: usize = 40;
    pub const COLOR: usize = 0;
    pub const UNLIT_HDR_SCALE: usize = 16;
    pub const ROT_DELTA: usize = 20;
    pub const ROT_TOTAL: usize = 24;
    pub const SIZE0: usize = 28;
    pub const SIZE1: usize = 32;
    pub const SCALE: usize = 36;
}

/// Ghosts' element types (before the catalog's conversion).
pub mod elem_type {
    pub const SPRITE_BILLBOARD: u8 = 0;
    pub const SPRITE_ORIENTED: u8 = 1;
    pub const TAIL: u8 = 2;
    pub const TRAIL: u8 = 3;
    pub const FLARE: u8 = 4;
    pub const PARTICLE_SIM: u8 = 5;
    pub const CLOUD_A: u8 = 6;
    pub const CLOUD_B: u8 = 7;
    pub const CLOUD: u8 = 8;
    pub const SPARK_CLOUD: u8 = 9;
    pub const SPARK_FOUNTAIN: u8 = 10;
    pub const MODEL: u8 = 11;
    pub const OMNI_LIGHT: u8 = 12;
    pub const SPOT_LIGHT: u8 = 13;
    pub const SOUND: u8 = 14;
    pub const DECAL: u8 = 15;
    pub const RUNNER: u8 = 16;
    pub const VECTOR_FIELD: u8 = 17;
}

/// Element flag: the second size channel sizes the height.
pub const FLAG_NON_UNIFORM_SIZE: u32 = 0x1000_0000;
/// Element flags 0x1c0: where the element runs.
pub const FLAG_RUN_MASK: u32 = 0x1c0;
/// Element flags 0x30: the spawn offset's shape.
pub const FLAG_SPAWN_OFFSET_MASK: u32 = 0x30;
/// Element flag: the scene's light scale is not applied (1.0).
pub const FLAG_NO_SCENE_SCALE: u32 = 0x80_0000;

/// A sound element further behind its start than this is dropped.
pub const SOUND_LATE_MSEC: i32 = 200;

/// Where an element runs (flags & 0x1c0).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RunFrame {
    World,
    RelativeToSpawn,
    RelativeToEffect,
    RelativeToOffset,
    RelativeToCamera,
    Other(u32),
}

pub fn run_frame(flags: u32) -> RunFrame {
    match flags & FLAG_RUN_MASK {
        0 => RunFrame::World,
        0x40 => RunFrame::RelativeToSpawn,
        0x80 => RunFrame::RelativeToEffect,
        0xc0 => RunFrame::RelativeToOffset,
        0x100 => RunFrame::RelativeToCamera,
        x => RunFrame::Other(x),
    }
}

/// The spawn offset's shape (flags & 0x30).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SpawnOffset {
    None,
    Sphere,
    Cylinder,
}

pub fn spawn_offset(flags: u32) -> SpawnOffset {
    match flags & FLAG_SPAWN_OFFSET_MASK {
        0x10 => SpawnOffset::Sphere,
        0x20 => SpawnOffset::Cylinder,
        _ => SpawnOffset::None,
    }
}

/// An element's seed.
#[inline]
pub fn elem_seed(sequence: u8, effect_seed: u16, def_seed: i32, msec: i32) -> u32 {
    u32::from(sequence)
        .wrapping_mul(IW6_SEED_SEQUENCE_MUL)
        .wrapping_add(u32::from(effect_seed))
        .wrapping_add(def_seed as u32)
        .wrapping_add(msec as u32)
        % IW6_RAND_MOD
}

/// The table's float for `seed` on `channel` (0..1).
#[inline]
pub fn rand_f32(seed: u32, channel: u32) -> f32 {
    h1::rand_f32(seed, channel)
}

/// The integer random for counts and times.
#[inline]
pub fn rand_u16(seed: u32, channel: u32) -> u16 {
    h1::rand_u16(seed, channel)
}

/// `base + ((rand16 x (amplitude + 1)) >> 16)`; amplitude 0 is no random.
#[inline]
pub fn int_range(base: i32, amplitude: i32, rand16: u16) -> i32 {
    h1::int_range(base, amplitude, rand16)
}

fn rd_f32(b: &[u8], o: usize) -> f32 {
    b.get(o..o + 4)
        .map_or(0.0, |s| f32::from_le_bytes([s[0], s[1], s[2], s[3]]))
}
fn rd_i32(b: &[u8], o: usize) -> i32 {
    b.get(o..o + 4)
        .map_or(0, |s| i32::from_le_bytes([s[0], s[1], s[2], s[3]]))
}
fn rd_u8(b: &[u8], o: usize) -> u8 {
    b.get(o).copied().unwrap_or(0)
}

/// The fields of one Ghosts element definition the rules use.
#[derive(Clone, Copy, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Iw6ElemRules {
    pub flags: u32,
    pub flags2: u32,
    pub elem_type: u8,
    pub lit_type: u8,
    pub visual_count: u8,
    pub vel_intervals: u8,
    pub vis_intervals: u8,
    pub spawn_a: i32,
    pub spawn_b: i32,
    pub fade_in: [f32; 2],
    pub fade_out: [f32; 2],
    pub delay: [i32; 2],
    pub life: [i32; 2],
    pub initial_rotation: [f32; 2],
    pub gravity: [f32; 2],
    pub sort_order: u8,
    pub lighting_frac: u8,
    pub random_seed: i32,
    pub lit_max_color_change_per_sec: f32,
    pub unlit_hdr_scalar: f32,
    pub lit_hdr_scalar: f32,
    pub lit_unlit_blend: f32,
}

impl Iw6ElemRules {
    /// From the 312-byte record.
    pub fn read(r: &[u8]) -> Self {
        let pair_i = |o: usize| [rd_i32(r, o), rd_i32(r, o + 4)];
        let pair_f = |o: usize| [rd_f32(r, o), rd_f32(r, o + 4)];
        Self {
            flags: rd_i32(r, elem::FLAGS) as u32,
            flags2: rd_i32(r, elem::FLAGS2) as u32,
            elem_type: rd_u8(r, elem::ELEM_TYPE),
            lit_type: rd_u8(r, elem::LIT_TYPE),
            visual_count: rd_u8(r, elem::VISUAL_COUNT),
            vel_intervals: rd_u8(r, elem::VEL_INTERVALS),
            vis_intervals: rd_u8(r, elem::VIS_INTERVALS),
            spawn_a: rd_i32(r, elem::SPAWN_A),
            spawn_b: rd_i32(r, elem::SPAWN_B),
            fade_in: pair_f(elem::FADE_IN),
            fade_out: pair_f(elem::FADE_OUT),
            delay: pair_i(elem::DELAY),
            life: pair_i(elem::LIFE),
            initial_rotation: pair_f(elem::INITIAL_ROTATION),
            gravity: pair_f(elem::GRAVITY),
            sort_order: rd_u8(r, elem::SORT_ORDER),
            lighting_frac: rd_u8(r, elem::LIGHTING_FRAC),
            random_seed: rd_i32(r, elem::RANDOM_SEED),
            lit_max_color_change_per_sec: rd_f32(r, elem::LIT_MAX_COLOR_CHANGE_PER_SEC),
            unlit_hdr_scalar: rd_f32(r, elem::UNLIT_HDR_SCALAR),
            lit_hdr_scalar: rd_f32(r, elem::LIT_HDR_SCALAR),
            lit_unlit_blend: rd_f32(r, elem::LIT_UNLIT_BLEND),
        }
    }

    /// When an element spawned at `msec` with `sequence` begins, and its
    /// seed there.
    pub fn begin(&self, sequence: u8, effect_seed: u16, msec: i32) -> (i32, u32) {
        spawn_begin(self.delay, self.random_seed, sequence, effect_seed, msec)
    }

    /// The element's life span (msec) for its seed.
    pub fn life_msec(&self, seed: u32) -> i32 {
        life_msec(self.life, seed)
    }
}

/// The begin time of an element spawned at `msec`: `msec + delay.base +
/// ((rand16 x (delay.amp + 1)) >> 16)`, the random taken with the seed at
/// `msec + delay.base`; and the element's seed at its begin time.
pub fn spawn_begin(delay: [i32; 2], def_seed: i32, sequence: u8, effect_seed: u16, msec: i32) -> (i32, u32) {
    let mut begin = msec.wrapping_add(delay[0]);
    if delay[1] != 0 {
        let s0 = elem_seed(sequence, effect_seed, def_seed, begin);
        begin = begin.wrapping_add(int_range(0, delay[1], rand_u16(s0, ch::DELAY)));
    }
    (begin, elem_seed(sequence, effect_seed, def_seed, begin))
}

/// An element's life span for its seed.
pub fn life_msec(life: [i32; 2], seed: u32) -> i32 {
    int_range(life[0], life[1], rand_u16(seed, ch::LIFE))
}

/// A one-shot element's count, drawn from Ghosts' element seed of the
/// effect's spawn (sequence 0) on the count channel: `spawn.base + ((rand16
/// x (spawn.amplitude + 1)) >> 16)`. The channel and the seed's time are
/// inferred (the spec gives the integer range, not the draw).
pub fn oneshot_count(spawn: [i32; 2], def_seed: i32, effect_seed: u16, msec: i32) -> i32 {
    int_range(spawn[0], spawn[1], rand_u16(elem_seed(0, effect_seed, def_seed, msec), ch::ONESHOT_COUNT))
}

/// Whether an element is queued at `now`: not when its life has already run
/// out, unless it carries an impact, death or emitted effect.
pub fn queued(begin: i32, life: i32, now: i32, has_child_effects: bool) -> bool {
    has_child_effects || now.wrapping_sub(begin.wrapping_add(life)) < 0
}

/// Whether a sound element beginning at `begin` still plays at `now`.
pub fn sound_fires(begin: i32, now: i32) -> bool {
    now.wrapping_sub(begin) <= SOUND_LATE_MSEC
}

/// Whether a bolted effect stays on its bolt: when one of its elements is a
/// trail (3) or of type 7, or runs relative to the effect. Otherwise it
/// plays in the world where it started. `elems`: (Ghosts element type,
/// flags).
pub fn keeps_bolt(elems: impl IntoIterator<Item = (u8, u32)>) -> bool {
    elems.into_iter().any(|(t, flags)| {
        t == elem_type::TRAIL || t == elem_type::CLOUD_B || run_frame(flags) == RunFrame::RelativeToEffect
    })
}

/// The draw list an element type goes on: 0 for types 0..7, 1 for 10 and
/// up, 2 for the clouds 8..9.
pub fn draw_list(elem_type: u8) -> u8 {
    match elem_type {
        0..=7 => 0,
        8 | 9 => 2,
        _ => 1,
    }
}

/// Whether the generic draw dispatch draws an element type (sounds and
/// vector fields are never drawn).
pub fn drawn_type(elem_type: u8) -> bool {
    elem_type != elem_type::SOUND && elem_type != elem_type::VECTOR_FIELD
}

/// The visual sample pair and weight for a normalised time.
pub fn vis_setup(intervals: u8, norm: f32) -> h1::VisSetup {
    h1::vis_setup(intervals, norm)
}

/// One particle's visual state.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Iw6Visual {
    /// Colour, linear floats, alpha last (fades not applied).
    pub rgba: [f32; 4],
    /// The unlit HDR scale (the sample's).
    pub unlit_scale: f32,
    /// Half sizes (width, height).
    pub size: [f32; 2],
    pub scale: f32,
    /// Rotation (radians), the initial rotation included.
    pub rotation: f32,
}

/// The visual state at normalised time `norm` for an element with `seed`.
/// `samples` holds `intervals + 1` 80-byte samples; `life_msec` scales the
/// rotation. `None` when the samples are short.
pub fn visual_state(rules: &Iw6ElemRules, samples: &[u8], norm: f32, seed: u32, life_msec: f32) -> Option<Iw6Visual> {
    if rules.vis_intervals == 0 {
        return None;
    }
    let s = vis_setup(rules.vis_intervals, norm);
    let a = s.index * vis::SIZE;
    if samples.len() < a + 2 * vis::SIZE {
        return None;
    }
    let s0 = &samples[a..a + vis::SIZE];
    let s1 = &samples[a + vis::SIZE..a + 2 * vis::SIZE];
    let (f, g) = (s.frac, 1.0 - s.frac);
    // base + random x amplitude, sample i weighted by 1 - frac.
    let range = |off: usize, ch: u32| {
        let r = rand_f32(seed, ch);
        (r * rd_f32(s1, vis::HALF + off) + rd_f32(s1, off)) * f + (r * rd_f32(s0, vis::HALF + off) + rd_f32(s0, off)) * g
    };
    // lerp(base, amplitude, colour random).
    let c = rand_f32(seed, ch::COLOR);
    let lerp = |off: usize| {
        (c * rd_f32(s1, vis::HALF + off) + (1.0 - c) * rd_f32(s1, off)) * f
            + (c * rd_f32(s0, vis::HALF + off) + (1.0 - c) * rd_f32(s0, off)) * g
    };
    let rgba = core::array::from_fn(|k| lerp(vis::COLOR + 4 * k));
    let size0 = range(vis::SIZE0, ch::SIZE0);
    let size1 = if rules.flags & FLAG_NON_UNIFORM_SIZE != 0 {
        range(vis::SIZE1, ch::SIZE1)
    } else {
        size0
    };
    // initialRot + life x (delta_i (f - f^2/2) + total_i + delta_i+1 f^2/2).
    let r = rand_f32(seed, ch::ROTATION);
    let half_sq = f * f * 0.5;
    let initial = rand_f32(seed, ch::INITIAL_ROTATION) * rules.initial_rotation[1] + rules.initial_rotation[0];
    let rot = ((r * rd_f32(s0, vis::HALF + vis::ROT_DELTA) + rd_f32(s0, vis::ROT_DELTA)) * (f - half_sq)
        + r * rd_f32(s0, vis::HALF + vis::ROT_TOTAL)
        + rd_f32(s0, vis::ROT_TOTAL)
        + (r * rd_f32(s1, vis::HALF + vis::ROT_DELTA) + rd_f32(s1, vis::ROT_DELTA)) * half_sq)
        * life_msec
        + initial;
    Some(Iw6Visual {
        rgba,
        unlit_scale: lerp(vis::UNLIT_HDR_SCALE),
        size: [size0, size1],
        scale: range(vis::SCALE, ch::SCALE),
        rotation: rot,
    })
}

/// One effect light element's state (types 12 and 13).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Iw6Light {
    /// Linear colour times the light's scale (red first).
    pub rgb: [f32; 3],
    pub radius: f32,
}

/// The light element's state at normalised time `norm`: the exe's omni
/// handler takes the origin, the radius from the first size channel and
/// the colour times the scale (`docs/exe_notes/IW6_FX_EXE.md`), and culls
/// by the sphere of that radius. The scale is the sample's scale channel;
/// INFERRED: a light whose scale channel is 0 takes the colour's HDR scale
/// instead (retail light elements were not compared). `None`: no radius, a
/// dark colour, or the samples are short.
pub fn light_state(rules: &Iw6ElemRules, samples: &[u8], norm: f32, seed: u32, life_msec: f32) -> Option<Iw6Light> {
    let v = visual_state(rules, samples, norm, seed, life_msec)?;
    let radius = v.size[0];
    if !(radius > 0.0) || !radius.is_finite() {
        return None;
    }
    let scale = if v.scale > 0.0 { v.scale } else { v.unlit_scale.max(0.0) };
    let rgb: [f32; 3] = core::array::from_fn(|k| (v.rgba[k] * scale).max(0.0));
    if !rgb.iter().any(|c| *c > 0.0 && c.is_finite()) {
        return None;
    }
    Some(Iw6Light { rgb, radius })
}

/// Whether an element's lit type takes the light: 3 (the light once per
/// effect) and 4 (per vertex).
pub fn takes_light(lit_type: u8) -> bool {
    matches!(lit_type, 3 | 4)
}

/// The scene's light scale for an element: 1 with flag 0x800000, else the
/// effect's.
pub fn scene_scale(flags: u32, effect_scale: f32) -> f32 {
    if flags & FLAG_NO_SCENE_SCALE != 0 { 1.0 } else { effect_scale }
}

/// The lit colour of a lit type 3 or 4 element: `rgb x ((1 - b) x unlit x
/// scene + b x light x litHDRScalar)`, `b` the element's lit/unlit blend.
pub fn lit_rgb(rules: &Iw6ElemRules, rgb: [f32; 3], unlit_scale: f32, light: [f32; 3], scene: f32) -> [f32; 3] {
    let b = rules.lit_unlit_blend;
    core::array::from_fn(|k| rgb[k] * ((1.0 - b) * unlit_scale * scene + b * light[k] * rules.lit_hdr_scalar))
}

/// The unlit colour: `rgb x unlit x scene` (the blend's unlit side alone).
pub fn unlit_rgb(rgb: [f32; 3], unlit_scale: f32, scene: f32) -> [f32; 3] {
    core::array::from_fn(|k| rgb[k] * unlit_scale * scene)
}

/// A light sampled once per effect, moved towards `target` by at most
/// `max_per_sec x dt` per channel.
pub fn approach_light(prev: [f32; 3], target: [f32; 3], max_per_sec: f32, dt_sec: f32) -> [f32; 3] {
    let step = (max_per_sec * dt_sec).max(0.0);
    core::array::from_fn(|k| prev[k] + (target[k] - prev[k]).clamp(-step, step))
}

/// The native frame's grade on an effect colour: what Ghosts' final pass
/// (`postfx_macroclut`) does to the HDR scene, so an effect drawn after
/// that pass looks as the world around it. `exposure x hdr`, Hable's curve
/// per channel times its white scale, sRGB, then the film (the colour
/// table's tint, contrast and desaturation in display space).
pub mod grade {
    /// One map's grade.
    #[derive(Clone, Copy, Debug, PartialEq)]
    pub struct Grade {
        /// The iris exposure (`DYNAMIC_IRIS_EXPOSURE`).
        pub exposure: f32,
        /// Hable's curve per channel: a, b, c, d, e, f.
        pub curve: [[f32; 6]; 3],
        /// `1 / f(white)` per channel.
        pub white_scale: [f32; 3],
        /// The film: on, base, delta, quad, bias, desaturation dark, light.
        pub film_on: bool,
        pub film_base: [f32; 3],
        pub film_delta: [f32; 3],
        pub film_quad: [f32; 3],
        pub film_bias: f32,
        pub film_desat_dark: f32,
        pub film_desat: f32,
    }

    fn hable(k: &[f32; 6], x: f32) -> f32 {
        let [a, b, c, d, e, f] = *k;
        let den = a * x * x + b * x + d * f;
        if den.abs() < 1e-12 {
            return 0.0;
        }
        (a * x * x + c * b * x + d * e) / den - e / f
    }

    fn linear_to_srgb(x: f32) -> f32 {
        if x <= 0.003_130_8 { x * 12.92 } else { 1.055 * libm::powf(x, 1.0 / 2.4) - 0.055 }
    }

    impl Grade {
        /// An HDR colour as the frame shows it (0..1, display space).
        pub fn apply(&self, hdr: [f32; 3]) -> [f32; 3] {
            let d: [f32; 3] = core::array::from_fn(|c| {
                let x = (hdr[c] * self.exposure).max(0.0);
                linear_to_srgb((hable(&self.curve[c], x) * self.white_scale[c]).clamp(0.0, 1.0))
            });
            if !self.film_on {
                return d;
            }
            let l = 0.2125 * d[0] + 0.7154 * d[1] + 0.0721 * d[2];
            let k = (self.film_desat_dark + (self.film_desat - self.film_desat_dark) * l).clamp(0.0, 1.0);
            core::array::from_fn(|c| {
                let x = d[c] + (l - d[c]) * k;
                (x * (self.film_base[c] + self.film_delta[c] * l + self.film_quad[c] * l * l) + self.film_bias).clamp(0.0, 1.0)
            })
        }
    }
}

/// Weapon effects: the attachment override and the kind of effect a shot
/// plays.
pub mod weapon {
    /// `FXOverrideEntry::fxType`.
    pub const VIEW_FLASH: u32 = 1;
    pub const WORLD_FLASH: u32 = 2;
    pub const VIEW_SHELL_EJECT: u32 = 3;
    pub const WORLD_SHELL_EJECT: u32 = 4;
    pub const VIEW_LAST_SHOT_EJECT: u32 = 5;
    pub const WORLD_LAST_SHOT_EJECT: u32 = 6;
    pub const VIEW_MAG_EJECT: u32 = 7;
    pub const VIEW_FLASH_ADS: u32 = 8;

    /// One override entry: two attachment ids (0: none), its kind.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct OverrideKey {
        pub attachment1: u32,
        pub attachment2: u32,
        pub fx_type: u32,
    }

    /// The entry that replaces the gun's effect of `fx_type` with
    /// `equipped` on: the most of its non-zero attachments equipped wins,
    /// the first on a tie; none with no match.
    pub fn best_override(entries: impl IntoIterator<Item = OverrideKey>, fx_type: u32, equipped: &[u32]) -> Option<usize> {
        let mut best = None;
        let mut best_score = 0u8;
        for (i, e) in entries.into_iter().enumerate() {
            if e.fx_type != fx_type {
                continue;
            }
            let score = u8::from(e.attachment1 != 0 && equipped.contains(&e.attachment1))
                + u8::from(e.attachment2 != 0 && equipped.contains(&e.attachment2));
            if score > best_score {
                best = Some(i);
                best_score = score;
            }
        }
        best
    }

    /// The flash kinds a shot tries, in order: in the view the ADS flash
    /// first when fully aimed (and the gun allows it), else the view flash;
    /// in the world the world flash.
    pub fn flash_types(first_person: bool, ads_frac: f32, no_ads_flash: bool) -> &'static [u32] {
        if !first_person {
            &[WORLD_FLASH]
        } else if ads_frac == 1.0 && !no_ads_flash {
            &[VIEW_FLASH_ADS, VIEW_FLASH]
        } else {
            &[VIEW_FLASH]
        }
    }

    /// The shell eject kind: the last-shot one for a last shot when the gun
    /// has one.
    pub fn eject_type(first_person: bool, last_shot: bool, has_last_shot_eject: bool) -> u32 {
        match (first_person, last_shot && has_last_shot_eject) {
            (true, true) => VIEW_LAST_SHOT_EJECT,
            (true, false) => VIEW_SHELL_EJECT,
            (false, true) => WORLD_LAST_SHOT_EJECT,
            (false, false) => WORLD_SHELL_EJECT,
        }
    }

    /// The tag the flash plays on.
    pub fn flash_tag(use_tag_flash_silenced: bool, silenced: bool) -> &'static str {
        if use_tag_flash_silenced && silenced { "tag_flash_silenced" } else { "tag_flash" }
    }

    /// Cells in one row of Ghosts' impact table: its 31 surfaces, then
    /// seven flesh cells.
    pub const IMPACT_COLUMNS: usize = 38;
    /// The first flesh cell (body, body fatal, head, head fatal follow).
    pub const IMPACT_FLESH_COLUMN: usize = 31;

    /// The impact table rows of a Ghosts weapon impact type: the bullet
    /// going in, then the alternate row (the bullet coming out). `None` for
    /// a type without a row (0) or an unknown one.
    pub fn impact_rows(impact_type: i32) -> Option<(usize, usize)> {
        Some(match impact_type {
            1 => (0, 1),
            2 => (2, 3),
            3 => (8, 9),
            4 => (4, 4),
            5 => (5, 6),
            6 => (7, 7),
            7 => (10, 10),
            8 => (12, 12),
            9 => (13, 13),
            10 => (14, 14),
            _ => return None,
        })
    }

    /// Whether a Ghosts impact type is a bullet's (its row is a hit's, not
    /// a footstep's or a projectile's explosion).
    pub fn is_bullet_impact(impact_type: i32) -> bool {
        matches!(impact_type, 1..=6 | 10)
    }

    /// The column of Ghosts' impact table a Black Ops II surface type
    /// (`weapon_iw4::T6_SURFACE_TYPE_NAMES`) reads. Ghosts' list is Black
    /// Ops II's to metal, then a second metal (14), `mud` .. `plastic` one
    /// above, then rubber (whose cells hold the cloth effects; Black Ops
    /// II's cushion takes them too), fruit, painted metal, riot shield,
    /// slush. Black Ops II's `player` reads flesh and `tallgrass` grass.
    pub fn impact_column(t6_surface: usize) -> usize {
        match t6_surface {
            0..=13 => t6_surface,
            14..=24 => t6_surface + 1,
            25 | 26 => 26,
            27 | 28 => t6_surface,
            29 => 7,
            30 => 10,
            31 => 29,
            _ => 0,
        }
    }
}

/// Map placements (createfx).
pub mod createfx {
    /// How a placement plays.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub enum Placement {
        /// Plays once.
        Oneshot,
        /// Replayed every so many milliseconds (`playLoopedFx`).
        Loop { repeat_msec: i32 },
    }

    /// `playLoopedFx`'s repeat: the seconds in whole milliseconds, at least
    /// 1.
    pub fn loop_repeat_msec(repeat_secs: f32) -> i32 {
        let ms = libm::roundf(repeat_secs * 1000.0);
        if ms.is_finite() { (ms as i32).max(1) } else { 1 }
    }

    /// A compiled placement's kind from its stored delay, the placement
    /// call itself not being readable: a negative (or zero) delay is the
    /// one-shot placement's default (`-15`), a positive one the loop's
    /// repeat.
    pub fn placement_of_delay(delay_secs: f32) -> Placement {
        if delay_secs.is_finite() && delay_secs > 0.0 {
            Placement::Loop { repeat_msec: loop_repeat_msec(delay_secs) }
        } else {
            Placement::Oneshot
        }
    }
}

/// What Ghosts' effect techniques do to a sprite that Black Ops II's effect
/// shaders do not (`docs/exe_notes/IW6_FX_EXE.md`, "Technique families":
/// decoded from the DXBC of `effect_*falloff*`, `zfeather` and `eyeoffset`).
pub mod material {
    /// How a falloff technique combines the faded colour (its vertex shader's
    /// last lines differ between the families).
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub enum Combine {
        /// `effect_add_falloff*`: `rgb' = f * vertex_rgb * lerp(end, begin, f)`.
        Add,
        /// `*_blend_falloff*`, `*_screen_falloff*`: `rgb' = lerp(end, begin, f) * vertex_rgb`.
        Blend,
    }

    /// The view-angle falloff: `f = saturate((N . V)^2 * scale + bias)`, with
    /// `N` the quad's world normal and `V` the unit vector from the eye to
    /// the vertex; the material's `falloffParms.zw` are `scale` and `bias`,
    /// `falloffBeginColor` the colour at `f = 1` (face on), `falloffEndColor`
    /// the colour at `f = 0` (edge on); alpha is multiplied by `f` too.
    #[derive(Clone, Copy, Debug, PartialEq)]
    pub struct Falloff {
        pub scale: f32,
        pub bias: f32,
        pub begin: [f32; 3],
        pub end: [f32; 3],
        pub combine: Combine,
    }

    /// A Ghosts effect material's own shading, read from its technique set's
    /// name and its constants.
    #[derive(Clone, Copy, Debug, Default, PartialEq)]
    pub struct MaterialFx {
        pub falloff: Option<Falloff>,
        /// `featherParms.x` of a `zfeather` technique: the vertex shader ramps
        /// alpha in with `saturate(view depth * scale)` (0: no ramp).
        pub near_scale: f32,
        /// `eyeOffsetPar.x`: the vertex shader moves the quad this many units
        /// toward the eye (0: none).
        pub eye_offset: f32,
    }

    impl MaterialFx {
        pub fn is_plain(&self) -> bool {
            self.falloff.is_none() && self.near_scale <= 0.0 && self.eye_offset == 0.0
        }
    }

    /// `f` for a quad seen at `cos` (N . V, either sign).
    pub fn falloff_factor(cos: f32, f: &Falloff) -> f32 {
        (cos * cos * f.scale + f.bias).clamp(0.0, 1.0)
    }

    /// The colour factor (per channel) and the alpha factor of the falloff at
    /// `cos`; `near` is the ramp `saturate(depth * near_scale)` (1 when none).
    pub fn falloff_factors(cos: f32, f: &Falloff) -> ([f32; 3], f32) {
        let k = falloff_factor(cos, f);
        let lerp = |i: usize| f.end[i] + k * (f.begin[i] - f.end[i]);
        let rgb = match f.combine {
            Combine::Add => [k * lerp(0), k * lerp(1), k * lerp(2)],
            Combine::Blend => [lerp(0), lerp(1), lerp(2)],
        };
        (rgb, k)
    }

    /// The vertex shader's alpha ramp near the eye.
    pub fn near_ramp(view_depth: f32, near_scale: f32) -> f32 {
        if near_scale <= 0.0 { 1.0 } else { (view_depth * near_scale).clamp(0.0, 1.0) }
    }

    /// The technique set's name tokens: `(has falloff, has zfeather, combine)`.
    pub fn tokens(technique_set: &str) -> (bool, bool, Combine) {
        let name = technique_set.trim_start_matches(',');
        let has = |w: &str| name.split('_').any(|t| t == w);
        let combine = if has("add") { Combine::Add } else { Combine::Blend };
        (has("falloff"), has("zfeather"), combine)
    }

    /// The material's shading from its technique set name and constants
    /// (`(name, value)`; names as the zone spells them, any case).
    pub fn read(technique_set: &str, constant: impl Fn(&str) -> Option<[f32; 4]>) -> MaterialFx {
        let (falloff, feather, combine) = tokens(technique_set);
        let name = technique_set.trim_start_matches(',');
        let falloff = if falloff {
            match (constant("falloffParms"), constant("falloffBegin"), constant("falloffEndCo")) {
                (Some(p), Some(b), Some(e)) => Some(Falloff {
                    scale: p[2],
                    bias: p[3],
                    begin: [b[0], b[1], b[2]],
                    end: [e[0], e[1], e[2]],
                    combine,
                }),
                _ => None,
            }
        } else {
            None
        };
        let near_scale = if feather { constant("featherParms").map_or(0.0, |v| v[0]) } else { 0.0 };
        let eye_offset = if name.split('_').any(|t| t == "eyeoffset") { constant("eyeOffsetPar").map_or(0.0, |v| v[0]) } else { 0.0 };
        MaterialFx { falloff, near_scale, eye_offset }
    }
}

#[cfg(test)]
mod tests {
    use super::weapon::*;

    /// A single-interval light element: two identical samples (colour
    /// rgb, unlit scale, size0, scale).
    fn light_samples(rgb: [f32; 3], unlit: f32, size: f32, scale: f32) -> [u8; 2 * super::vis::SIZE] {
        let mut one = [0u8; super::vis::SIZE];
        // (Colour and unlit scale lerp base to amplitude by the colour
        // random: the amplitude half repeats the base, as retail data does.)
        let mut put = |at: usize, v: f32| {
            one[at..at + 4].copy_from_slice(&v.to_le_bytes());
            one[super::vis::HALF + at..super::vis::HALF + at + 4].copy_from_slice(&v.to_le_bytes());
        };
        for k in 0..3 {
            put(super::vis::COLOR + 4 * k, rgb[k]);
        }
        put(super::vis::COLOR + 12, 1.0);
        put(super::vis::UNLIT_HDR_SCALE, unlit);
        // (Size and scale are base + random x amplitude: amplitude 0.)
        one[super::vis::HALF + super::vis::SIZE0..super::vis::HALF + super::vis::SIZE0 + 4].fill(0);
        one[super::vis::HALF + super::vis::SCALE..super::vis::HALF + super::vis::SCALE + 4].fill(0);
        let mut put = |at: usize, v: f32| one[at..at + 4].copy_from_slice(&v.to_le_bytes());
        put(super::vis::SIZE0, size);
        put(super::vis::SCALE, scale);
        let mut out = [0u8; 2 * super::vis::SIZE];
        out[..super::vis::SIZE].copy_from_slice(&one);
        out[super::vis::SIZE..].copy_from_slice(&one);
        out
    }

    #[test]
    fn a_light_is_colour_times_scale_with_the_size_as_radius() {
        let rules = super::Iw6ElemRules { vis_intervals: 1, ..Default::default() };
        let l = super::light_state(&rules, &light_samples([1.0, 0.5, 0.25], 9.0, 120.0, 4.0), 0.3, 77, 1000.0).unwrap();
        assert_eq!(l.radius, 120.0);
        assert_eq!(l.rgb, [4.0, 2.0, 1.0]);
        // A zero scale channel takes the colour's HDR scale.
        let l = super::light_state(&rules, &light_samples([1.0, 1.0, 1.0], 2.0, 50.0, 0.0), 0.3, 77, 1000.0).unwrap();
        assert_eq!(l.rgb, [2.0; 3]);
        // No radius or no colour: no light.
        assert!(super::light_state(&rules, &light_samples([1.0; 3], 1.0, 0.0, 1.0), 0.3, 77, 1000.0).is_none());
        assert!(super::light_state(&rules, &light_samples([0.0; 3], 1.0, 50.0, 1.0), 0.3, 77, 1000.0).is_none());
        // Short samples: none.
        assert!(super::light_state(&rules, &[0u8; 10], 0.3, 77, 1000.0).is_none());
    }

    #[test]
    fn the_grade_tone_maps_like_the_final_pass() {
        // Hable's published curve, exposure 1, no film: 0 stays 0, white
        // (11.2) reaches 1, mid grey lands in between, monotonic.
        let k = [0.15, 0.5, 0.1, 0.2, 0.02, 0.3];
        let w = 1.0 / {
            let x = 11.2f32;
            (k[0] * x * x + k[2] * k[1] * x + k[3] * k[4]) / (k[0] * x * x + k[1] * x + k[3] * k[5]) - k[4] / k[5]
        };
        let g = super::grade::Grade {
            exposure: 1.0,
            curve: [k; 3],
            white_scale: [w; 3],
            film_on: false,
            film_base: [1.0; 3],
            film_delta: [0.0; 3],
            film_quad: [0.0; 3],
            film_bias: 0.0,
            film_desat_dark: 0.0,
            film_desat: 0.0,
        };
        assert!(g.apply([0.0; 3])[0].abs() < 1e-4);
        assert!((g.apply([11.2; 3])[0] - 1.0).abs() < 1e-3);
        let (a, b) = (g.apply([0.18; 3])[0], g.apply([0.5; 3])[0]);
        assert!(a > 0.05 && a < b && b < 1.0, "{a} {b}");
        // Twice the exposure is brighter.
        let g2 = super::grade::Grade { exposure: 2.0, ..g };
        assert!(g2.apply([0.18; 3])[0] > a);
    }

    #[test]
    fn falloff_fades_edge_on_and_keeps_face_on() {
        use super::material::*;
        // gfx_godray_beam_win: falloffParms.zw = (1.4902906, ~0), begin 1, end 0.
        let f = Falloff { scale: 1.490_290_6, bias: 0.0, begin: [1.0; 3], end: [0.0; 3], combine: Combine::Add };
        assert_eq!(falloff_factor(1.0, &f), 1.0);
        assert_eq!(falloff_factor(0.0, &f), 0.0);
        // full strength from about 35 degrees off the normal to face on
        assert_eq!(falloff_factor(0.82, &f), 1.0);
        let (rgb, a) = falloff_factors(0.5, &f);
        let k = 0.25 * 1.490_290_6;
        assert!((a - k).abs() < 1e-5);
        // the add family multiplies the colour by f once more (begin 1, end 0: f * f)
        assert!((rgb[0] - k * k).abs() < 1e-5, "{rgb:?}");
        let fb = Falloff { combine: Combine::Blend, ..f };
        let (rgb, a) = falloff_factors(0.5, &fb);
        assert!((rgb[0] - k).abs() < 1e-5 && (a - k).abs() < 1e-5);
        assert_eq!(near_ramp(5.0, 0.1), 0.5);
        assert_eq!(near_ramp(50.0, 0.1), 1.0);
        assert_eq!(near_ramp(0.0, 0.0), 1.0);
    }

    #[test]
    fn material_shading_reads_names_and_constants() {
        use super::material::*;
        let c = |n: &str| match n {
            "falloffParms" => Some([0.005, -0.05, 1.49, 0.0]),
            "falloffBegin" => Some([1.0; 4]),
            "falloffEndCo" => Some([0.0, 0.0, 0.0, 0.5]),
            "featherParms" => Some([0.1, 10.0, 0.0, 0.0]),
            "eyeOffsetPar" => Some([12.0, 0.0, 0.0, 0.0]),
            _ => None,
        };
        let m = read("effect_zfeather_falloff_add_lin_nofog_eyeoffset_ndw_bl_blmod", c);
        assert!(m.falloff.is_some_and(|f| f.combine == Combine::Add && f.scale == 1.49));
        assert_eq!((m.near_scale, m.eye_offset), (0.1, 12.0));
        let m = read("effect_add_lin_ndw_bl_blmod", c);
        assert!(m.is_plain());
        let m = read(",effect_zfeather_falloff_blend_lin_ndw_bl_abl", c);
        assert!(m.falloff.is_some_and(|f| f.combine == Combine::Blend));
    }

    #[test]
    fn oneshot_count_stays_in_its_range() {
        for msec in 0..200 {
            let n = super::oneshot_count([3, 4], 77, 1234, msec * 37);
            assert!((3..=7).contains(&n), "{n}");
        }
        assert_eq!(super::oneshot_count([5, 0], 1, 2, 3), 5);
    }

    #[test]
    fn impact_rows_follow_the_weapon_impact_type() {
        // (Retail common_mp: row 0 small, 1 small exit, 2 large, 3 large exit,
        // 5 and 6 shotgun, 12 grenade, 13 rocket.)
        assert_eq!(impact_rows(1), Some((0, 1)));
        assert_eq!(impact_rows(2), Some((2, 3)));
        assert_eq!(impact_rows(5), Some((5, 6)));
        assert_eq!(impact_rows(9), Some((13, 13)));
        assert_eq!(impact_rows(0), None);
        assert!(is_bullet_impact(2) && !is_bullet_impact(8) && !is_bullet_impact(7));
    }

    #[test]
    fn black_ops_ii_surfaces_read_ghosts_columns() {
        // concrete, metal: the same; mud 14 -> 15; plastic 24 -> 25;
        // cushion -> the rubber column (cloth); tall grass -> grass.
        assert_eq!(impact_column(5), 5);
        assert_eq!(impact_column(13), 13);
        assert_eq!(impact_column(14), 15);
        assert_eq!(impact_column(24), 25);
        assert_eq!(impact_column(26), 26);
        assert_eq!(impact_column(28), 28);
        assert_eq!(impact_column(30), 10);
        assert_eq!(impact_column(31), 29);
    }
}
