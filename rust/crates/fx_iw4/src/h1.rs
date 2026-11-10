//! bo2mp: Modern Warfare Remastered's particle rules, as its executable runs
//! them (static analysis of the owner's `h1_mp64_zt.exe`; see
//! `docs/H1_NATIVE.md`, "Effects"). The element maths is the IW family's
//! (spawn origin and offsets, the velocity graph, gravity, the fades, the
//! sample interpolation all have the shape of `fx_iw4`'s), but MWR changed
//! what goes in:
//!
//! * its own random table (16391 floats, seeds taken mod 16361, channels as
//!   IW4's plus scale 28 and pivot 29/30), and the element seed
//!   `(sequence * 10111 + effect seed + element def seed + spawn msec) mod
//!   16361`, where IW4 had `(effect seed + sequence * 296 + msec) mod 479`
//!   and no per-definition seed;
//! * float visual samples (`FxElemVisStateSample`, 112 bytes: colour rgba,
//!   emissive rgb, rotation delta / total, size x2, scale, pivot x2, as base
//!   and second half), where IW4 kept colour bytes;
//! * colour and emissive both take the colour channel's random, as a lerp
//!   between the sample's two halves; size, scale, pivot and rotation are
//!   `base + random x second half`;
//! * the lit colour: in the default (HDR) mode `rgb x (emissive +
//!   hdrLightingFrac x light)` for lit types 1..3 (3: the light's luminance),
//!   `rgb x emissive` for lit type 0; in the other mode the LDR rule
//!   `rgb x (1 + (2 light - 1) lightingFrac / 255)`;
//! * a particle is not drawn when its distance fade, or its colour's alpha
//!   times the fade, is at or below a threshold (`0/255` by default).
//!
//! Everything here is a pure function of the definition's bytes, the
//! element's spawn and the time, as `render_t7::fx` is for Black Ops III.

/// The random table's seed range (`seed mod 16361`).
pub const H1_RAND_MOD: u32 = 16361;
/// Floats in the table: the seed range plus the highest channel (30).
pub const H1_RANDOM_TABLE_FLOATS: usize = 16391;
/// The sequence's weight in an element's seed.
pub const H1_SEED_SEQUENCE_MUL: u32 = 10111;

const H1_RANDOM_TABLE: &[u8] = include_bytes!("../data/h1_fx_random.bin");

/// Random channels (index added to the seed).
pub mod ch {
    pub const VEL_X: u32 = 0;
    pub const ANG_VEL: u32 = 3;
    pub const ORIGIN_X: u32 = 6;
    pub const OFFSET_YAW: u32 = 9;
    pub const OFFSET_HEIGHT: u32 = 10;
    pub const OFFSET_RADIUS: u32 = 11;
    pub const SPAWN_ANGLES: u32 = 12;
    pub const GRAVITY: u32 = 15;
    pub const REFLECTION: u32 = 16;
    pub const LIFE: u32 = 17;
    pub const DELAY: u32 = 18;
    pub const ONESHOT_COUNT: u32 = 19;
    pub const EMIT_DIST: u32 = 20;
    pub const VISUAL: u32 = 21;
    pub const ATLAS: u32 = 22;
    pub const COLOR: u32 = 23;
    pub const INITIAL_ROTATION: u32 = 24;
    pub const ROTATION: u32 = 25;
    pub const SIZE0: u32 = 26;
    pub const SIZE1: u32 = 27;
    pub const SCALE: u32 = 28;
    pub const PIVOT_X: u32 = 29;
    pub const PIVOT_Y: u32 = 30;
}

/// `FxElemDef` (320 bytes) fields the rules read, by offset.
pub mod elem {
    pub const FLAGS: usize = 0x00;
    pub const FLAGS2: usize = 0x04;
    /// Looping: interval msec; one-shot: count base.
    pub const SPAWN_A: usize = 0x08;
    /// Looping: how many it spawns (`0x7fffffff`: no end); one-shot: count
    /// amplitude.
    pub const SPAWN_B: usize = 0x0c;
    pub const FADE_IN: usize = 0x18;
    pub const FADE_OUT: usize = 0x20;
    pub const DELAY: usize = 0x2c;
    pub const LIFE: usize = 0x34;
    pub const SPAWN_ORIGIN: usize = 0x3c;
    pub const OFFSET_RADIUS: usize = 0x54;
    pub const OFFSET_HEIGHT: usize = 0x5c;
    pub const INITIAL_ROTATION: usize = 0x94;
    pub const GRAVITY: usize = 0x9c;
    pub const ELEM_TYPE: usize = 0xb4;
    pub const LIT_TYPE: usize = 0xb5;
    pub const VISUAL_COUNT: usize = 0xb6;
    pub const VEL_INTERVALS: usize = 0xb7;
    pub const VIS_INTERVALS: usize = 0xb8;
    pub const LIGHTING_FRAC: usize = 0x121;
    pub const RANDOM_SEED: usize = 0x128;
    pub const EMISSIVE_SCALE_SCALE: usize = 0x12c;
    pub const HDR_LIGHTING_FRAC: usize = 0x130;
    pub const SIZE: usize = 0x140;
}

/// `FxEffectDef` (64 bytes) fields.
pub mod effect {
    pub const LOOPING_LIFE: usize = 0x10;
    pub const LOOPING: usize = 0x14;
    pub const ONE_SHOT: usize = 0x18;
    pub const EMISSION: usize = 0x1c;
}

/// Element flag: the second size channel sizes the height.
pub const FLAG_NON_UNIFORM_SIZE: u32 = 0x1000_0000;
/// Element flag: velocity graph in the effect's axes.
pub const FLAG_LOCAL_VELOCITY: u32 = 0x0100_0000;
/// Element flag: velocity graph in world axes.
pub const FLAG_WORLD_VELOCITY: u32 = 0x0200_0000;
/// Element flags 0x1c0: where the element runs (0 world, 0x40 relative to
/// the effect when it spawned, 0x80 relative to the effect now, 0xc0 to its
/// spawn offset, 0x100 to the view).
pub const FLAG_RUN_MASK: u32 = 0x1c0;

/// `FxElemVisStateSample` (112 bytes) layout: two halves of 56.
pub mod vis {
    pub const SIZE: usize = 112;
    pub const HALF: usize = 56;
    pub const COLOR: usize = 0;
    pub const EMISSIVE: usize = 16;
    pub const ROT_DELTA: usize = 28;
    pub const ROT_TOTAL: usize = 32;
    pub const SIZE0: usize = 36;
    pub const SIZE1: usize = 40;
    pub const SCALE: usize = 44;
    pub const PIVOT: usize = 48;
}

fn table_f32(idx: usize) -> f32 {
    let idx = idx % H1_RANDOM_TABLE_FLOATS;
    let o = idx * 4;
    f32::from_le_bytes([
        H1_RANDOM_TABLE[o],
        H1_RANDOM_TABLE[o + 1],
        H1_RANDOM_TABLE[o + 2],
        H1_RANDOM_TABLE[o + 3],
    ])
}

/// The table's float for `seed` on channel `channel` (0..1).
#[inline]
pub fn rand_f32(seed: u32, channel: u32) -> f32 {
    table_f32((seed % H1_RAND_MOD + channel) as usize)
}

/// The integer random the game uses for counts and times: the low half of
/// the same float's bits.
#[inline]
pub fn rand_u16(seed: u32, channel: u32) -> u16 {
    (table_f32((seed % H1_RAND_MOD + channel) as usize).to_bits() & 0xffff) as u16
}

/// `base + ((rand16 x (amplitude + 1)) >> 16)`, the game's integer range.
#[inline]
pub fn int_range(base: i32, amplitude: i32, rand16: u16) -> i32 {
    base.wrapping_add((i64::from(rand16) * i64::from(amplitude.wrapping_add(1)) >> 16) as i32)
}

/// An effect's seed from the C runtime's `rand()` (as IW4's, mod 16361).
#[inline]
pub fn effect_seed_from_rand(r: i32) -> u16 {
    let p = r.wrapping_mul(H1_RAND_MOD as i32);
    (p.wrapping_add((p >> 31) & 0x7fff) >> 15) as u16
}

/// An element's seed (the spawn routine's formula; unsigned arithmetic).
#[inline]
pub fn elem_seed(sequence: u8, effect_seed: u16, def_seed: i32, msec: i32) -> u32 {
    u32::from(sequence)
        .wrapping_mul(H1_SEED_SEQUENCE_MUL)
        .wrapping_add(u32::from(effect_seed))
        .wrapping_add(def_seed as u32)
        .wrapping_add(msec as u32)
        % H1_RAND_MOD
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

/// The fields of one element definition the rules use.
#[derive(Clone, Copy, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct H1ElemRules {
    pub flags: u32,
    pub flags2: u32,
    pub elem_type: u8,
    pub lit_type: u8,
    pub visual_count: u8,
    pub vis_intervals: u8,
    pub spawn_a: i32,
    pub spawn_b: i32,
    pub fade_in: [f32; 2],
    pub fade_out: [f32; 2],
    pub delay: [i32; 2],
    pub life: [i32; 2],
    pub initial_rotation: [f32; 2],
    pub gravity: [f32; 2],
    pub lighting_frac: u8,
    pub random_seed: i32,
    pub emissive_scale_scale: f32,
    pub hdr_lighting_frac: f32,
}

impl H1ElemRules {
    /// From the 320-byte record.
    pub fn read(r: &[u8]) -> Self {
        Self {
            flags: rd_i32(r, elem::FLAGS) as u32,
            flags2: rd_i32(r, elem::FLAGS2) as u32,
            elem_type: rd_u8(r, elem::ELEM_TYPE),
            lit_type: rd_u8(r, elem::LIT_TYPE),
            visual_count: rd_u8(r, elem::VISUAL_COUNT),
            vis_intervals: rd_u8(r, elem::VIS_INTERVALS),
            spawn_a: rd_i32(r, elem::SPAWN_A),
            spawn_b: rd_i32(r, elem::SPAWN_B),
            fade_in: [rd_f32(r, elem::FADE_IN), rd_f32(r, elem::FADE_IN + 4)],
            fade_out: [rd_f32(r, elem::FADE_OUT), rd_f32(r, elem::FADE_OUT + 4)],
            delay: [rd_i32(r, elem::DELAY), rd_i32(r, elem::DELAY + 4)],
            life: [rd_i32(r, elem::LIFE), rd_i32(r, elem::LIFE + 4)],
            initial_rotation: [
                rd_f32(r, elem::INITIAL_ROTATION),
                rd_f32(r, elem::INITIAL_ROTATION + 4),
            ],
            gravity: [rd_f32(r, elem::GRAVITY), rd_f32(r, elem::GRAVITY + 4)],
            lighting_frac: rd_u8(r, elem::LIGHTING_FRAC),
            random_seed: rd_i32(r, elem::RANDOM_SEED),
            emissive_scale_scale: rd_f32(r, elem::EMISSIVE_SCALE_SCALE),
            hdr_lighting_frac: rd_f32(r, elem::HDR_LIGHTING_FRAC),
        }
    }

    /// When an element spawned at `msec` with `sequence` begins and its
    /// seed: the delay is drawn with the seed of the undelayed time plus
    /// the delay's base, then the seed is taken again at the begin time.
    pub fn begin(&self, sequence: u8, effect_seed: u16, msec: i32) -> (i32, u32) {
        let mut begin = msec.wrapping_add(self.delay[0]);
        if self.delay[1] != 0 {
            let s0 = elem_seed(sequence, effect_seed, self.random_seed, begin);
            begin = begin.wrapping_add(int_range(0, self.delay[1], rand_u16(s0, ch::DELAY)));
        }
        (begin, elem_seed(sequence, effect_seed, self.random_seed, begin))
    }

    /// The element's life span (msec) for its seed.
    pub fn life_msec(&self, seed: u32) -> i32 {
        int_range(self.life[0], self.life[1], rand_u16(seed, ch::LIFE))
    }

    /// A one-shot element's count for the effect's seed.
    pub fn one_shot_count(&self, effect_seed: u16) -> i32 {
        if self.spawn_b == 0 {
            self.spawn_a
        } else {
            int_range(self.spawn_a, self.spawn_b, rand_u16(u32::from(effect_seed), ch::ONESHOT_COUNT))
        }
    }

    /// Which visual of `visual_count` an element draws.
    pub fn visual_index(&self, seed: u32) -> usize {
        if self.visual_count <= 1 {
            0
        } else {
            ((u32::from(rand_u16(seed, ch::VISUAL)) * u32::from(self.visual_count)) >> 16) as usize
        }
    }

    /// Gravity (units / s^2, positive down) for the seed: `(base + random x
    /// amplitude) x 800`.
    pub fn gravity(&self, seed: u32) -> f32 {
        (rand_f32(seed, ch::GRAVITY) * self.gravity[1] + self.gravity[0]) * 800.0
    }
}

/// When a looping element spawns between `from` (exclusive of what was
/// already spawned) and `to` (inclusive): the first at the effect's start
/// with sequence 0, then one every `interval` msec, `count` in all (the
/// spawn loop: `n = (from - start) / interval + 1`, `msec = start + n
/// interval`, while `msec <= to` and `n < count`). The sequence is `n`
/// (the seed takes its low byte).
pub fn looping_spawns(
    interval: i32,
    count: i32,
    start: i32,
    to: i32,
    mut each: impl FnMut(i32, u32),
) {
    if to < start || count <= 0 {
        return;
    }
    each(start, 0);
    if interval <= 0 {
        return;
    }
    let mut n: i64 = 1;
    loop {
        let msec = i64::from(start) + n * i64::from(interval);
        if msec > i64::from(to) || n >= i64::from(count) {
            break;
        }
        each(msec as i32, n as u32);
        n += 1;
    }
}

/// An element's age clamped as the draw does: `(age, norm)`; at or past its
/// life the age is held at 0.99 of it and the normalised time at 1.
pub fn draw_age(age_msec: i32, life_msec: i32) -> (f32, f32) {
    let life = life_msec as f32;
    let age = age_msec as f32;
    if age - life >= 0.0 {
        (life * 0.99, 1.0)
    } else {
        (age, age / life)
    }
}

/// The sample pair and weights for a normalised time (the exe's setup:
/// `i = min(floor(n t), n - 1)`, `frac = n t - i`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct VisSetup {
    pub index: usize,
    pub frac: f32,
}

pub fn vis_setup(intervals: u8, norm: f32) -> VisSetup {
    let n = f32::from(intervals);
    let x = n * norm;
    let i = (libm::floorf(x) as i32).min(i32::from(intervals) - 1).max(0);
    VisSetup {
        index: i as usize,
        frac: x - i as f32,
    }
}

/// One particle's visual state (the draw's channel evaluators).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct H1Visual {
    /// Colour, linear floats, alpha last (fades not applied).
    pub rgba: [f32; 4],
    /// Emissive scale per channel.
    pub emissive: [f32; 3],
    /// Half sizes (width, height).
    pub size: [f32; 2],
    pub scale: f32,
    pub pivot: [f32; 2],
    /// Rotation (radians), the initial rotation included.
    pub rotation: f32,
}

/// The visual state at normalised time `norm` for an element with `seed`.
/// `samples` holds `intervals + 1` 112-byte samples; `life_msec` scales the
/// rotation as the exe's colour step does. `None` when the samples are short.
pub fn visual_state(
    rules: &H1ElemRules,
    samples: &[u8],
    norm: f32,
    seed: u32,
    life_msec: f32,
) -> Option<H1Visual> {
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
    // base + random x second half, sample i weighted by 1 - frac.
    let range = |off: usize, ch: u32| {
        let r = rand_f32(seed, ch);
        (r * rd_f32(s1, vis::HALF + off) + rd_f32(s1, off)) * f
            + (r * rd_f32(s0, vis::HALF + off) + rd_f32(s0, off)) * g
    };
    // lerp(base, second half, colour random), for colour and emissive.
    let c = rand_f32(seed, ch::COLOR);
    let lerp = |off: usize| {
        (c * rd_f32(s1, vis::HALF + off) + (1.0 - c) * rd_f32(s1, off)) * f
            + (c * rd_f32(s0, vis::HALF + off) + (1.0 - c) * rd_f32(s0, off)) * g
    };
    let rgba = core::array::from_fn(|k| lerp(vis::COLOR + 4 * k));
    let emissive = core::array::from_fn(|k| lerp(vis::EMISSIVE + 4 * k));
    let size0 = range(vis::SIZE0, ch::SIZE0);
    let size1 = if rules.flags & FLAG_NON_UNIFORM_SIZE != 0 {
        range(vis::SIZE1, ch::SIZE1)
    } else {
        size0
    };
    // Rotation: initial + (delta_i (f - f^2/2) + total_i + delta_i+1 f^2/2)
    // x life, all with the rotation channel's random.
    let r = rand_f32(seed, ch::ROTATION);
    let half_sq = f * f * 0.5;
    let initial = rand_f32(seed, ch::INITIAL_ROTATION) * rules.initial_rotation[1]
        + rules.initial_rotation[0];
    let rot = ((r * rd_f32(s0, vis::HALF + vis::ROT_DELTA) + rd_f32(s0, vis::ROT_DELTA))
        * (f - half_sq)
        + r * rd_f32(s0, vis::HALF + vis::ROT_TOTAL)
        + rd_f32(s0, vis::ROT_TOTAL)
        + (r * rd_f32(s1, vis::HALF + vis::ROT_DELTA) + rd_f32(s1, vis::ROT_DELTA)) * half_sq)
        * life_msec
        + initial;
    Some(H1Visual {
        rgba,
        emissive,
        size: [size0, size1],
        scale: range(vis::SCALE, ch::SCALE),
        pivot: [range(vis::PIVOT, ch::PIVOT_X), range(vis::PIVOT + 4, ch::PIVOT_Y)],
        rotation: rot,
    })
}

/// The distance fade: `fadeIn` (start, length) takes the particle from 1 at
/// its start to 0 at start + length, `fadeOut` from 0 to 1; the smaller
/// counts. A zero length leaves its side at 1.
pub fn distance_fade(dist: f32, fade_in: [f32; 2], fade_out: [f32; 2]) -> f32 {
    let side = |r: [f32; 2]| {
        let x = dist - r[0];
        if x < 0.0 {
            1.0
        } else if x - r[1] < 0.0 {
            1.0 - x / r[1]
        } else {
            0.0
        }
    };
    let a = if fade_in[1] != 0.0 { side(fade_in) } else { 1.0 };
    let b = if fade_out[1] != 0.0 { 1.0 - side(fade_out) } else { 1.0 };
    a.min(b)
}

/// Whether a particle is drawn: its distance fade and its colour's alpha
/// times the fade above `threshold / 255` (the game's default threshold is
/// 0).
pub fn drawn(alpha: f32, fade: f32, threshold: u8) -> bool {
    let t = f32::from(threshold) / 255.0;
    fade > t && alpha * fade > t
}

/// Rec. 709 luminance (the lit type 3 light).
pub fn luminance(rgb: [f32; 3]) -> f32 {
    0.2126 * rgb[0] + 0.7152 * rgb[1] + 0.0722 * rgb[2]
}

/// The lit colour (rgb). `light` is the particle's light (linear, scene
/// units): its ambient, plus the sun when the element asks for it.
/// `hdr` is the renderer's HDR mode (on by default in the game).
pub fn lit_rgb(rules: &H1ElemRules, rgb: [f32; 3], emissive: [f32; 3], light: [f32; 3], hdr: bool) -> [f32; 3] {
    let lt = rules.lit_type;
    if hdr {
        match lt {
            0 => core::array::from_fn(|k| rgb[k] * emissive[k]),
            1..=3 => {
                let l = if lt == 3 { [luminance(light); 3] } else { light };
                let h = rules.hdr_lighting_frac;
                core::array::from_fn(|k| {
                    let lit = if h > 0.0 { h * l[k] * rgb[k] } else { 0.0 };
                    lit + rgb[k] * emissive[k]
                })
            }
            _ => rgb,
        }
    } else if (1..=3).contains(&lt) && rules.lighting_frac != 0 {
        let k = f32::from(rules.lighting_frac) / 255.0;
        core::array::from_fn(|i| rgb[i] * (2.0 * ((light[i] - 0.5) * k) + 1.0))
    } else {
        rgb
    }
}

/// bo2mp (ours, not the game's): an HDR particle colour as the sprite pass
/// takes it (0..1, drawn after MWR's tone map): a colour brighter than
/// white is scaled down whole, keeping its hue, as the effect conversion
/// did before.
pub fn display_rgb(rgb: [f32; 3]) -> [f32; 3] {
    let peak = rgb[0].max(rgb[1]).max(rgb[2]);
    let k = if peak > 1.0 && peak.is_finite() { 1.0 / peak } else { 1.0 };
    core::array::from_fn(|i| (rgb[i] * k).clamp(0.0, 1.0))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(color: [f32; 4], color2: [f32; 4], emissive: [f32; 3], size: f32, size_amp: f32) -> [u8; 112] {
        let mut b = [0u8; 112];
        let mut put = |o: usize, x: f32| b[o..o + 4].copy_from_slice(&x.to_le_bytes());
        for k in 0..4 {
            put(vis::COLOR + 4 * k, color[k]);
            put(vis::HALF + vis::COLOR + 4 * k, color2[k]);
        }
        for k in 0..3 {
            put(vis::EMISSIVE + 4 * k, emissive[k]);
            put(vis::HALF + vis::EMISSIVE + 4 * k, emissive[k]);
        }
        put(vis::SIZE0, size);
        put(vis::HALF + vis::SIZE0, size_amp);
        put(vis::SIZE1, 2.0 * size);
        put(vis::SCALE, 1.0);
        b
    }

    #[test]
    fn the_table_is_the_games() {
        assert_eq!(H1_RANDOM_TABLE.len(), 4 * H1_RANDOM_TABLE_FLOATS);
        // Its first values (read off the exe) and its spread.
        assert!((rand_f32(0, 0) - 0.236_532_72).abs() < 1e-7);
        assert!((rand_f32(0, 1) - 0.185_675_8).abs() < 1e-7);
        let (mut lo, mut hi, mut sum) = (1.0f32, 0.0f32, 0.0f64);
        for i in 0..H1_RANDOM_TABLE_FLOATS {
            let x = table_f32(i);
            lo = lo.min(x);
            hi = hi.max(x);
            sum += f64::from(x);
        }
        assert!(lo >= 0.0 && hi < 1.0);
        assert!((sum / H1_RANDOM_TABLE_FLOATS as f64 - 0.5).abs() < 0.01);
        // The integer random is the float's low half.
        assert_eq!(rand_u16(0, ch::LIFE), (table_f32(17).to_bits() & 0xffff) as u16);
    }

    #[test]
    fn seeds_follow_the_spawn_formula() {
        assert_eq!(elem_seed(0, 0, 0, 0), 0);
        assert_eq!(elem_seed(1, 0, 0, 0), 10111);
        assert_eq!(elem_seed(2, 5, 3, 100), (2 * 10111 + 5 + 3 + 100) % 16361);
        // Unsigned wrap for a negative time.
        assert_eq!(elem_seed(0, 0, 0, -1), (u32::MAX) % 16361);
        assert!(effect_seed_from_rand(0x7fff) < 16361);
        assert_eq!(effect_seed_from_rand(0), 0);
    }

    #[test]
    fn delay_and_life_draw_from_their_channels() {
        let r = H1ElemRules { delay: [100, 50], life: [1000, 500], random_seed: 7, ..Default::default() };
        let (begin, seed) = r.begin(3, 11, 2000);
        let s0 = elem_seed(3, 11, 7, 2100);
        let want_begin = 2100 + ((u32::from(rand_u16(s0, ch::DELAY)) * 51) >> 16) as i32;
        assert_eq!(begin, want_begin);
        assert_eq!(seed, elem_seed(3, 11, 7, want_begin));
        let life = r.life_msec(seed);
        assert!((1000..=1500).contains(&life));
        assert_eq!(life, 1000 + ((u32::from(rand_u16(seed, ch::LIFE)) * 501) >> 16) as i32);
        // No amplitude: no random.
        let fixed = H1ElemRules { delay: [100, 0], life: [800, 0], ..Default::default() };
        assert_eq!(fixed.begin(0, 0, 0).0, 100);
        assert_eq!(fixed.life_msec(1234), 800);
    }

    #[test]
    fn looping_elements_spawn_on_the_interval_up_to_their_count() {
        let mut got = [(0i32, 0u32); 8];
        let mut n = 0;
        looping_spawns(100, 3, 1000, 2000, |m, s| {
            got[n] = (m, s);
            n += 1;
        });
        assert_eq!(&got[..n], &[(1000, 0), (1100, 1), (1200, 2)]);
        n = 0;
        looping_spawns(250, i32::MAX, 0, 600, |m, s| {
            got[n] = (m, s);
            n += 1;
        });
        assert_eq!(&got[..n], &[(0, 0), (250, 1), (500, 2)]);
    }

    #[test]
    fn samples_interpolate_as_the_draw_does() {
        let s = vis_setup(4, 0.6);
        assert_eq!(s.index, 2);
        assert!((s.frac - 0.4).abs() < 1e-6);
        // The last interval holds at its end.
        let e = vis_setup(4, 1.0);
        assert_eq!(e.index, 3);
        assert!((e.frac - 1.0).abs() < 1e-6);
        let a = draw_age(500, 1000);
        assert_eq!(a, (500.0, 0.5));
        assert_eq!(draw_age(1000, 1000), (990.0, 1.0));
    }

    #[test]
    fn colour_and_emissive_lerp_with_the_colour_random_and_sizes_add_theirs() {
        let rules = H1ElemRules { vis_intervals: 1, ..Default::default() };
        let mut samples = [0u8; 224];
        samples[..112].copy_from_slice(&sample([1.0, 0.0, 0.0, 1.0], [0.0, 0.0, 1.0, 0.0], [2.0; 3], 10.0, 4.0));
        samples[112..].copy_from_slice(&sample([0.0, 1.0, 0.0, 1.0], [0.0, 1.0, 0.0, 1.0], [4.0; 3], 20.0, 0.0));
        let seed = 77;
        let v = visual_state(&rules, &samples, 0.25, seed, 1000.0).unwrap();
        let c = rand_f32(seed, ch::COLOR);
        let r_size = rand_f32(seed, ch::SIZE0);
        // Red: sample 0's lerp (1 -> 0) weighted 0.75.
        assert!((v.rgba[0] - (1.0 - c) * 0.75).abs() < 1e-5);
        assert!((v.rgba[1] - 0.25).abs() < 1e-5);
        assert!((v.emissive[0] - (2.0 * 0.75 + 4.0 * 0.25)).abs() < 1e-5);
        assert!((v.size[0] - ((10.0 + 4.0 * r_size) * 0.75 + 20.0 * 0.25)).abs() < 1e-4);
        // Uniform size unless the element asks for its second channel.
        assert_eq!(v.size[1], v.size[0]);
        let wide = H1ElemRules { flags: FLAG_NON_UNIFORM_SIZE, ..rules };
        let w = visual_state(&wide, &samples, 0.25, seed, 1000.0).unwrap();
        assert!((w.size[1] - (20.0 * 0.75 + 40.0 * 0.25)).abs() < 1e-4);
        assert!((v.scale - 1.0).abs() < 1e-6);
        // Short samples: none.
        assert!(visual_state(&rules, &samples[..200], 0.5, seed, 1000.0).is_none());
    }

    #[test]
    fn rotation_integrates_its_delta_over_life() {
        let rules = H1ElemRules { vis_intervals: 1, initial_rotation: [0.5, 0.0], ..Default::default() };
        let mut samples = [0u8; 224];
        for s in 0..2 {
            let o = s * 112 + vis::ROT_DELTA;
            samples[o..o + 4].copy_from_slice(&0.001f32.to_le_bytes());
        }
        // A constant delta: 0.5 + 0.001 x t x life.
        let v = visual_state(&rules, &samples, 0.5, 9, 2000.0).unwrap();
        assert!((v.rotation - (0.5 + 0.001 * 0.5 * 2000.0)).abs() < 1e-4);
    }

    #[test]
    fn fades_take_the_smaller_side() {
        assert_eq!(distance_fade(50.0, [0.0, 0.0], [0.0, 0.0]), 1.0);
        // Fade in: 1 before 100, 0 after 300.
        assert_eq!(distance_fade(50.0, [100.0, 200.0], [0.0, 0.0]), 1.0);
        assert!((distance_fade(200.0, [100.0, 200.0], [0.0, 0.0]) - 0.5).abs() < 1e-6);
        assert_eq!(distance_fade(400.0, [100.0, 200.0], [0.0, 0.0]), 0.0);
        // Fade out: 0 near, rising to 1 by 20.
        assert_eq!(distance_fade(5.0, [0.0, 0.0], [10.0, 10.0]), 0.0);
        assert!((distance_fade(15.0, [0.0, 0.0], [10.0, 10.0]) - 0.5).abs() < 1e-6);
        assert!((distance_fade(15.0, [12.0, 10.0], [10.0, 10.0]) - 0.5).abs() < 1e-6);
        assert!(!drawn(1.0, 0.0, 0));
        assert!(!drawn(0.0, 1.0, 0));
        assert!(drawn(0.01, 0.5, 0));
    }

    #[test]
    fn lit_colour_follows_the_lit_type() {
        let mut r = H1ElemRules { hdr_lighting_frac: 2.0, lighting_frac: 255, ..Default::default() };
        let rgb = [0.5, 0.5, 0.5];
        // Lit type 0: emissive alone.
        assert_eq!(lit_rgb(&r, rgb, [3.0; 3], [9.0; 3], true), [1.5; 3]);
        // Lit 1: emissive plus the light times hdrLightingFrac.
        r.lit_type = 1;
        assert_eq!(lit_rgb(&r, rgb, [0.0; 3], [1.0, 2.0, 0.0], true), [1.0, 2.0, 0.0]);
        // Lit 3: the light's luminance.
        r.lit_type = 3;
        let l = luminance([1.0, 2.0, 0.0]);
        assert!((lit_rgb(&r, rgb, [0.0; 3], [1.0, 2.0, 0.0], true)[2] - 0.5 * 2.0 * l).abs() < 1e-6);
        // Lit 4 and up: as sampled.
        r.lit_type = 4;
        assert_eq!(lit_rgb(&r, rgb, [7.0; 3], [1.0; 3], true), rgb);
        // The LDR mode: 1 + (2 light - 1) x frac.
        r.lit_type = 1;
        let out = lit_rgb(&r, rgb, [1.0; 3], [0.0, 0.5, 1.0], false);
        assert!((out[0] - 0.0).abs() < 1e-6 && (out[1] - 0.5).abs() < 1e-6 && (out[2] - 1.0).abs() < 1e-6);
        assert_eq!(display_rgb([4.0, 2.0, 1.0]), [1.0, 0.5, 0.25]);
    }

    #[test]
    fn rules_read_their_offsets() {
        let mut r = [0u8; elem::SIZE];
        r[elem::ELEM_TYPE] = 2;
        r[elem::LIT_TYPE] = 3;
        r[elem::VIS_INTERVALS] = 4;
        r[elem::LIGHTING_FRAC] = 76;
        r[elem::RANDOM_SEED..elem::RANDOM_SEED + 4].copy_from_slice(&55i32.to_le_bytes());
        r[elem::HDR_LIGHTING_FRAC..elem::HDR_LIGHTING_FRAC + 4].copy_from_slice(&60.0f32.to_le_bytes());
        r[elem::LIFE..elem::LIFE + 4].copy_from_slice(&1500i32.to_le_bytes());
        let x = H1ElemRules::read(&r);
        assert_eq!((x.elem_type, x.lit_type, x.vis_intervals, x.lighting_frac), (2, 3, 4, 76));
        assert_eq!((x.random_seed, x.hdr_lighting_frac, x.life), (55, 60.0, [1500, 0]));
        assert_eq!(x.visual_index(5), 0);
        let many = H1ElemRules { visual_count: 4, ..x };
        assert!(many.visual_index(5) < 4);
        assert!((x.gravity(3) - 0.0).abs() < 1e-6);
    }
}
