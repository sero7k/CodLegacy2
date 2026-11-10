//! Black Ops III's effect random table and the particle rules that read it,
//! as the exe has them (static analysis of the reference exe; see
//! docs/T7_VISUAL_PARITY.md "The particle rules read off the exe"):
//!
//! * the table: 844 floats at 0x142ec7630 (`data/t7_fx_random.bin`, copied
//!   out of the exe), a seed taken modulo 809 (IW4's: 479 others), each
//!   channel `table[seed + channel]` (a u16 channel: the float's low half);
//! * a particle's seed: `(sequence * 500 + effect seed + spawn msec) % 809`
//!   (`FUN_14157cc80`; IW4's: `* 296 % 479`);
//! * the channels are IW4's: velocity 0..2, angular velocity 3..5, spawn
//!   origin 6..8, offset yaw 9, height 10, radius 11, spawn angles 12..14,
//!   gravity 15, life 17, delay 18, one-shot count 19, visual 21, atlas 22,
//!   colour 23, initial rotation 24, rotation 25, size 26 / 27, model scale
//!   28, emissive 29 (each read where the exe reads it);
//! * spawn offsets (`FUN_1415d47a0`): a sphere as IW4's, a cylinder round
//!   one of the effect's three axes (def byte +0x6c: 0 forward, 1 left, 2
//!   up; IW4's always forward);
//! * gravity (`FUN_1415bca50`): `(base + r15 x amplitude) x 800` units/s².

use fx_iw4::FxElemVec3Range;

static TABLE: &[u8] = include_bytes!("../data/t7_fx_random.bin");

/// The seed's modulus (`% 0x329` in the exe).
pub const MOD: u32 = 809;

/// Channels (offsets into the table from the seed).
pub mod ch {
    pub const ANG_VEL: u32 = 3;
    pub const ORIGIN: u32 = 6;
    pub const OFFSET_YAW: u32 = 9;
    pub const OFFSET_HEIGHT: u32 = 10;
    pub const OFFSET_RADIUS: u32 = 11;
    pub const ANGLES: u32 = 12;
    pub const GRAVITY: u32 = 15;
    pub const LIFE: u32 = 17;
    pub const DELAY: u32 = 18;
    pub const COUNT: u32 = 19;
    pub const VISUAL: u32 = 21;
    pub const ATLAS: u32 = 22;
    pub const COLOUR: u32 = 23;
    pub const INITIAL_ROTATION: u32 = 24;
    pub const ROTATION: u32 = 25;
    pub const SIZE0: u32 = 26;
    pub const SIZE1: u32 = 27;
    pub const SCALE: u32 = 28;
    pub const EMISSIVE: u32 = 29;
}

fn at(seed: u32, channel: u32) -> usize {
    ((seed % MOD + channel) as usize * 4).min(TABLE.len() - 4)
}

/// The table's float for a seed and channel (0 .. 1).
pub fn f(seed: u32, channel: u32) -> f32 {
    let o = at(seed, channel);
    f32::from_le_bytes(TABLE[o..o + 4].try_into().unwrap())
}

/// The low half of the table's float for a seed and channel.
pub fn u16(seed: u32, channel: u32) -> u16 {
    let o = at(seed, channel);
    u16::from_le_bytes([TABLE[o], TABLE[o + 1]])
}

/// A particle's seed from its effect's seed, its sequence byte and the msec
/// it spawned at (unsigned arithmetic, as the exe's).
pub fn elem_seed(effect_seed: u16, sequence: u8, spawn_msec: i32) -> u32 {
    u32::from(sequence).wrapping_mul(500).wrapping_add(u32::from(effect_seed)).wrapping_add(spawn_msec as u32) % MOD
}

/// Which of `count` visuals a particle draws.
pub fn visual_index(count: usize, seed: u32) -> usize {
    if count <= 1 {
        return 0;
    }
    ((count as u32).wrapping_mul(u32::from(u16(seed, ch::VISUAL))) >> 16) as usize
}

/// A life span: `base + (rand16 x (amplitude + 1)) >> 16`.
pub fn life(base: i32, amplitude: i32, seed: u32) -> i32 {
    base.wrapping_add((i32::from(u16(seed, ch::LIFE)).wrapping_mul(amplitude.wrapping_add(1))) >> 16)
}

/// The particle's spawn place in the world (`FUN_1415d4bd0`, then the
/// offset of `FUN_1415d47a0`): `axis` rows forward, left, up.
#[allow(clippy::too_many_arguments)]
pub fn spawn_origin(origin: [f32; 3], axis: [[f32; 3]; 3], spawn: [[f32; 2]; 3], flags: i32, radius: [f32; 2], height: [f32; 2], cylinder_axis: u8, seed: u32) -> [f32; 3] {
    let local: [f32; 3] = core::array::from_fn(|a| spawn[a][0] + f(seed, ch::ORIGIN + a as u32) * spawn[a][1]);
    let mut p = origin;
    for a in 0..3 {
        p[a] += if flags & 2 != 0 { local[0] * axis[0][a] + local[1] * axis[1][a] + local[2] * axis[2][a] } else { local[a] };
    }
    match flags & 0x30 {
        0 => {}
        0x10 => {
            let h = f(seed, ch::OFFSET_HEIGHT) + f(seed, ch::OFFSET_HEIGHT) - 1.0;
            let horiz = (1.0 - h * h).max(0.0).sqrt();
            let yaw = f(seed, ch::OFFSET_YAW) * std::f32::consts::TAU;
            let r = f(seed, ch::OFFSET_RADIUS) * radius[1] + radius[0];
            p[0] += yaw.cos() * horiz * r;
            p[1] += yaw.sin() * horiz * r;
            p[2] += r * h;
        }
        _ => {
            // (Round the axis `cylinder_axis` names: the radius in the
            // other two, the height along it.)
            let (sin_dir, cos_dir, along, sign) = match cylinder_axis {
                1 => (axis[2], axis[0], axis[1], -1.0),
                2 => (axis[1], axis[0], axis[2], 1.0),
                _ => (axis[2], axis[1], axis[0], 1.0),
            };
            let yaw = f(seed, ch::OFFSET_YAW) * std::f32::consts::TAU;
            let r = f(seed, ch::OFFSET_RADIUS) * radius[1] + radius[0];
            let (s, c) = (yaw.sin() * r, yaw.cos() * r * sign);
            let hgt = f(seed, ch::OFFSET_HEIGHT) * height[1] + height[0];
            for a in 0..3 {
                p[a] += c * cos_dir[a] + s * sin_dir[a] + hgt * along[a];
            }
        }
    }
    p
}

/// The particle's angles (radians) `age_ms` into its life: spawn angles
/// plus angular velocity (per msec).
pub fn angles(spawn: [[f32; 2]; 3], vel: [[f32; 2]; 3], seed: u32, age_ms: f32) -> [f32; 3] {
    core::array::from_fn(|i| spawn[i][0] + f(seed, ch::ANGLES + i as u32) * spawn[i][1] + age_ms * (vel[i][0] + f(seed, ch::ANG_VEL + i as u32) * vel[i][1]))
}

/// Gravity's acceleration, units/s² downwards.
pub fn gravity(range: [f32; 2], seed: u32) -> f32 {
    (f(seed, ch::GRAVITY) * range[1] + range[0]) * 800.0
}

fn vel_at(samples: &[FxElemVec3Range], t: f32, seed: u32) -> [f32; 3] {
    let last = samples.len() - 1;
    let s = t.clamp(0.0, 1.0) * last as f32;
    let i = (s.floor() as usize).min(last);
    let j = (i + 1).min(last);
    let k = if i == j { 0.0 } else { s - i as f32 };
    core::array::from_fn(|a| {
        let v = |r: &FxElemVec3Range| r.base[a] + r.amplitude[a] * f(seed, a as u32);
        v(&samples[i]) + (v(&samples[j]) - v(&samples[i])) * k
    })
}

/// How far a velocity graph carries the
/// particle from its spawn to `t` (0 .. 1 of its life): the graph's
/// integral, piecewise linear as the graph is.
pub fn travel(samples: &[FxElemVec3Range], t: f32, life_ms: f32, seed: u32) -> [f32; 3] {
    let mut d = [0.0f32; 3];
    if samples.len() < 2 || life_ms <= 0.0 || t <= 0.0 {
        return d;
    }
    let n = samples.len() - 1;
    let t = t.min(1.0);
    for seg in 0..n {
        let (lo, hi) = (seg as f32 / n as f32, ((seg + 1) as f32 / n as f32).min(t));
        if hi <= lo {
            break;
        }
        let (v0, v1) = (vel_at(samples, lo, seed), vel_at(samples, hi, seed));
        // (The samples hold velocity per interval: x the interval count, as
        // `fx_iw4::integrate_velocity_graph` reads IW4's.)
        let w = 0.5 * (hi - lo) * life_ms * n as f32;
        for a in 0..3 {
            d[a] += (v0[a] + v1[a]) * w;
        }
    }
    d
}

/// The rotation (radians) at `frac` into a visual interval, as the exe's
/// writer (`FUN_141573090`): `(delta0 (f - f²/2) + total0 + delta1 f²/2) x
/// life + initial`, each `base + r25 x amplitude`, the initial
/// `base + r24 x amplitude`. `s0`, `s1`: the interval's two 48-byte samples
/// (IW4's layout: base half then amplitude half; delta +4, total +8).
pub fn rotation(s0: &[u8], s1: &[u8], frac: f32, life_ms: f32, initial: [f32; 2], seed: u32) -> f32 {
    let g = |s: &[u8], o: usize| s.get(o..o + 4).map_or(0.0, |b| f32::from_le_bytes(b.try_into().unwrap()));
    let r = f(seed, ch::ROTATION);
    let half_sq = frac * frac * 0.5;
    let delta0 = g(s0, 4) + r * g(s0, 28);
    let total0 = g(s0, 8) + r * g(s0, 32);
    let delta1 = g(s1, 4) + r * g(s1, 28);
    (delta0 * (frac - half_sq) + total0 + delta1 * half_sq) * life_ms + initial[0] + f(seed, ch::INITIAL_ROTATION) * initial[1]
}

/// The visual interval a normalised age falls in and how far into it
/// (`FUN_14157cc80`: `i = min(floor(n t), n - 1)`, at least 0; `frac = n t - i`).
pub fn sample_point(intervals: u8, t: f32) -> (usize, f32) {
    let n = f32::from(intervals);
    let i = ((n * t).floor().max(0.0) as usize).min(usize::from(intervals.saturating_sub(1)));
    (i, n * t - i as f32)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_table_is_the_exes() {
        assert_eq!(TABLE.len(), 844 * 4);
        assert!((f(0, 0) - 0.073_574_86).abs() < 1e-6);
        assert!((f(0, 2) - 0.124_257_04).abs() < 1e-6);
        for s in 0..MOD {
            for c in 0..=ch::EMISSIVE {
                let v = f(s, c);
                assert!((0.0..=1.0).contains(&v), "seed {s} channel {c}: {v}");
            }
        }
        // (Seed 808 + channel 29 is the last float the exe reads, 837 < 844.)
        assert_eq!(elem_seed(0, 0, 809), 0);
        assert_eq!(elem_seed(3, 2, 10), (1000 + 3 + 10) % 809);
        // Negative spawn times wrap as the exe's unsigned sum does.
        assert_eq!(elem_seed(0, 0, -1), u32::MAX % 809);
    }

    #[test]
    fn a_cylinder_lies_round_the_axis_its_def_names() {
        let axis = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
        // Radius 10, no height: round up (2), the point stays at z 0.
        let p = spawn_origin([0.0; 3], axis, [[0.0; 2]; 3], 0x20, [10.0, 0.0], [0.0; 2], 2, 5);
        assert!(p[2].abs() < 1e-5 && ((p[0] * p[0] + p[1] * p[1]).sqrt() - 10.0).abs() < 1e-3, "{p:?}");
        // Round forward (0): x stays 0.
        let p = spawn_origin([0.0; 3], axis, [[0.0; 2]; 3], 0x20, [10.0, 0.0], [5.0, 0.0], 0, 5);
        assert!((p[0] - 5.0).abs() < 1e-5 && ((p[1] * p[1] + p[2] * p[2]).sqrt() - 10.0).abs() < 1e-3, "{p:?}");
    }

    #[test]
    fn velocity_travel_integrates_the_graph() {
        let s = vec![FxElemVec3Range { base: [0.1, 0.0, 0.0], amplitude: [0.0; 3] }, FxElemVec3Range { base: [0.1, 0.0, 0.0], amplitude: [0.0; 3] }];
        let d = travel(&s, 0.5, 1000.0, 1);
        assert!((d[0] - 50.0).abs() < 1e-3);
        assert_eq!(sample_point(0, 0.7), (0, 0.0));
        assert_eq!(sample_point(4, 0.5), (2, 0.0));
    }
}
