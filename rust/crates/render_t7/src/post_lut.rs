//! The colour-grade LUT build's constants as BlackOps3.exe records them
//! (exe 0x141d5e9b0, before `create_lut2dv_compute`: one 32³ RGBA16F
//! dispatch of 8 × 8 × 8 groups).
//!
//! Its inputs are the view's post-FX settings S:
//! * S+0x24..0xa8: the client's active vision (`vision/<name>.vision`, its
//!   words 0..0x84: `vkTT` colour temperature, `vkTS` saturation, `vkTC`
//!   tint and brightness, `vkTO` offset, then the tone curve `vkRGB0..4`,
//!   `vkL*`, `vkM*`, `vkRM`), copied each frame from the client's vision
//!   slot (exe 0x1411ea860: cg +0x3260f8 + 0xf0·slot into the refdef
//!   +0xd8; the refdef into the scene +0x88, 0x141de37a0; the scene into S
//!   +0x24, 0x141dd8140). The client's naked vision starts as `default`
//!   (0x1411e12f0); a missing vision file falls back to
//!   `vision/default.vision` (0x1411d6f10).
//! * S+0xac/0xae: the LUT slices of the camera's LUT volume (previous and
//!   current) and S+0xb4/0xb8 their blend (0x141dd8140); with the camera in
//!   no LUT volume both are slice 0, blended 0.5 / 0.5.
//! * the four colour-vision blocks (S+0x18cc + 0x110·k, 15 registers each
//!   into `postFxConst00..59`) and their weights (`postFxConst60`).
//!
//! `postFxControl0..2` are the columns of the colour matrix: the colour
//! temperature's chromatic adaptation (0x14238afa0: Bradford, from 6500 K,
//! each entry rounded to 1/1024) × saturation (Rec. 709 weights) × the
//! tint (sRGB → linear) times `2^brightness`, plus the offset;
//! `postFxControl3..13` the tone curve's segments.

/// Krystek's Planckian locus (exe 0x14238ae70): the XYZ (Y = 1) of a
/// black body at `t` kelvin.
fn kelvin_xyz(t: f32) -> [f32; 4] {
    let t = f64::from(t);
    let t2 = t * t * 1e-7;
    let u = (t * 1e-4 * 1.541_182_54 + 0.860_117_757 + t2 * 1.286_412_12) / (t * 1e-4 * 8.424_202_35 + 1.0 + t2 * 7.081_451_63);
    let v = (t * t * 1e-8 * 4.204_816_91 + t * 1e-5 * 4.228_062_45 + 0.317_398_726) / ((1.0 - t * 1e-5 * 2.897_418_16) + t2 * 1.614_560_53);
    let d = ((u + u) - v * 8.0) + 4.0;
    let y = (v + v) / d;
    let x = (u * 3.0) / d;
    [(x / y) as f32, 1.0, (((1.0 - x) - y) / y) as f32, 0.0]
}

type M4 = [[f32; 4]; 4];

/// Row-vector 4×4 product `a · b` (exe 0x142388870).
fn mul(a: &M4, b: &M4) -> M4 {
    std::array::from_fn(|i| std::array::from_fn(|j| a[i][0] * b[0][j] + a[i][1] * b[1][j] + a[i][2] * b[2][j] + a[i][3] * b[3][j]))
}

/// The exe's matrices (row-vector): XYZ → LMS (Bradford), its inverse,
/// linear sRGB → XYZ and back (0x143388030, 0x143387ff0, 0x143388070,
/// 0x143387fb0).
const BRADFORD: M4 = [[0.8951, -0.7502, 0.0389, 0.0], [0.2664, 1.7135, -0.0685, 0.0], [-0.1614, 0.0367, 1.0296, 0.0], [0.0, 0.0, 0.0, 1.0]];
const BRADFORD_INV: M4 = [[0.986_993, 0.432_305, -0.008_529, 0.0], [-0.147_054, 0.518_36, 0.040_043, 0.0], [0.159_963, 0.049_291, 0.968_487, 0.0], [0.0, 0.0, 0.0, 1.0]];
const RGB_TO_XYZ: M4 = [[0.4124, 0.2126, 0.0193, 0.0], [0.3576, 0.7152, 0.1192, 0.0], [0.1805, 0.0722, 0.9505, 0.0], [0.0, 0.0, 0.0, 1.0]];
const XYZ_TO_RGB: M4 = [[3.240_71, -0.969_258, 0.055_635_2, 0.0], [-1.537_26, 1.875_99, -0.203_996, 0.0], [-0.498_571, 0.041_555_7, 1.057_07, 0.0], [0.0, 0.0, 0.0, 1.0]];

/// The colour temperature matrix (exe 0x14238afa0): white balance from
/// 6500 K to `t`, in linear sRGB.
pub fn temperature_matrix(t: f32) -> M4 {
    let lms = |xyz: [f32; 4]| -> [f32; 3] { std::array::from_fn(|c| xyz[1] * BRADFORD[1][c] + xyz[3] * BRADFORD[3][c] + xyz[0] * BRADFORD[0][c] + xyz[2] * BRADFORD[2][c]) };
    let (r, t) = (lms(kelvin_xyz(6500.0)), lms(kelvin_xyz(t)));
    let d: M4 = [[t[0] / r[0], 0.0, 0.0, 0.0], [0.0, t[1] / r[1], 0.0, 0.0], [0.0, 0.0, t[2] / r[2], 0.0], [0.0, 0.0, 0.0, 1.0]];
    let m = mul(&mul(&RGB_TO_XYZ, &mul(&mul(&BRADFORD, &d), &BRADFORD_INV)), &XYZ_TO_RGB);
    let mut m = m;
    for row in m.iter_mut().take(3) {
        for v in row.iter_mut().take(3) {
            *v = ((*v + 0.000_488_281_25) * 1024.0).floor() * 0.000_976_562_5;
        }
    }
    m
}

fn srgb_to_linear(c: f32) -> f32 {
    if 0.040_45 < c { ((c + 0.055) / 1.055).powf(2.4) } else { c / 12.92 }
}

/// `postFxControl0..13` from the vision words 0..0x84 (`v[i]` = S+0x24+4i).
pub fn controls(v: &[f32]) -> [[f32; 4]; 14] {
    let s = |off: usize| v.get((off - 0x24) / 4).copied().unwrap_or(0.0);
    let mut c = [[0f32; 4]; 14];
    // Colour matrix.
    let temp = temperature_matrix(s(0x24));
    let k = s(0x28) + 1.0;
    let q = 1.0 - k;
    let (a, b, d) = (q * 0.2126, q * 0.7152, q * 0.0722);
    let sat: M4 = [[a + k, a, a, 0.0], [b, b + k, b, 0.0], [d, d, d + k, 0.0], [0.0, 0.0, 0.0, 1.0]];
    let ts = mul(&temp, &sat);
    let bright = 2f32.powf(s(0x38));
    let gain: M4 = [[srgb_to_linear(s(0x2c)) * bright, 0.0, 0.0, 0.0], [0.0, srgb_to_linear(s(0x30)) * bright, 0.0, 0.0], [0.0, 0.0, srgb_to_linear(s(0x34)) * bright, 0.0], [0.0; 4]];
    let mut m = mul(&ts, &gain);
    m[3][0] += s(0x3c);
    m[3][1] += s(0x40);
    m[3][2] += s(0x44);
    for (k, col) in c.iter_mut().take(3).enumerate() {
        *col = [m[0][k], m[1][k], m[2][k], m[3][k]];
    }
    // The tone curve (transcribed from the recorder's order of operations).
    let eps = 0.000_122_070_31f32;
    let (rgb0, l0, m0) = ([s(0x48), s(0x4c), s(0x50)], s(0x54), s(0x58));
    let (rgb1, l1, m1) = ([s(0x5c), s(0x60), s(0x64)], s(0x68), s(0x6c));
    let (rgb2, l2) = ([s(0x70), s(0x74), s(0x78)], s(0x7c));
    let (rgb3, l3, m3) = ([s(0x80), s(0x84), s(0x88)], s(0x8c), s(0x90));
    let (rgb4, l4, m4) = ([s(0x94), s(0x98), s(0x9c)], s(0xa0), s(0xa4));
    let rm = s(0xa8);
    let mut lo2 = l2 - eps;
    let mut hi2 = l2 + eps;
    let hi3 = if hi2 <= l3 { l3 } else { hi2 };
    let lo1 = if l1 <= lo2 { l1 } else { lo2 };
    let x = (hi3 - l2) * m3 + l2;
    let y = (l2 - lo1) * m1 + lo1;
    if hi2 <= x {
        hi2 = x;
    }
    if y <= lo2 {
        lo2 = y;
    }
    let p3 = if hi2 + eps <= hi3 { hi3 } else { hi2 + eps };
    let p1 = if lo1 <= lo2 - eps { lo1 } else { lo2 - eps };
    let mut p4 = p3 + eps;
    let mut p0 = p1 - eps;
    let lo0 = if l0 <= p0 { l0 } else { p0 };
    let hi4 = if p4 <= l4 { l4 } else { p4 };
    let x = (p1 - lo0) * m0 + lo0;
    let y = (hi4 - p3) * m4 + p3;
    if x <= p0 {
        p0 = x;
    }
    if p4 <= y {
        p4 = y;
    }
    let pm = if lo0 <= p0 - eps { lo0 } else { p0 - eps };
    let r0 = 1.0 / (p0 - pm);
    let r1 = 1.0 / (p1 - p0);
    let r2 = 1.0 / (lo2 - p1);
    let r3 = 1.0 / (l2 - lo2);
    c[3] = [r0, r1, r2, r3];
    c[4] = [-pm * r0, -p0 * r1, -p1 * r2, -lo2 * r3];
    let r4 = 1.0 / (hi2 - l2);
    let r5 = 1.0 / (p3 - hi2);
    let r6 = 1.0 / (p4 - p3);
    let p5 = if p4 + eps <= hi4 { hi4 } else { p4 + eps };
    let r7 = 1.0 / (p5 - p4);
    c[5] = [r4, r5, r6, r7];
    c[6] = [-l2 * r4, -hi2 * r5, -p3 * r6, -p4 * r7];
    let h = 0.5f32;
    let mid = |a: f32, b: f32| (b - a) * h + a;
    c[7] = [rgb0[0], rgb0[1], rgb0[2], rm];
    let (r01, r12, r23, r34) = (mid(rgb0[0], rgb1[0]), mid(rgb1[0], rgb2[0]), mid(rgb2[0], rgb3[0]), mid(rgb3[0], rgb4[0]));
    c[8] = [r01 - rgb0[0], rgb1[0] - r01, r12 - rgb1[0], rgb2[0] - r12];
    c[9] = [r23 - rgb2[0], rgb3[0] - r23, r34 - rgb3[0], rgb4[0] - r34];
    let (g01, g12, g23, g34) = (mid(rgb0[1], rgb1[1]), mid(rgb1[1], rgb2[1]), mid(rgb2[1], rgb3[1]), mid(rgb3[1], rgb4[1]));
    c[10] = [g01 - rgb0[1], rgb1[1] - g01, g12 - rgb1[1], rgb2[1] - g12];
    c[11] = [g23 - rgb2[1], rgb3[1] - g23, g34 - rgb3[1], rgb4[1] - g34];
    let (b01, b12, b23, b34) = (mid(rgb0[2], rgb1[2]), mid(rgb1[2], rgb2[2]), mid(rgb2[2], rgb3[2]), mid(rgb3[2], rgb4[2]));
    c[12] = [b01 - rgb0[2], rgb1[2] - b01, b12 - rgb1[2], rgb2[2] - b12];
    c[13] = [b23 - rgb2[2], rgb3[2] - b23, b34 - rgb3[2], rgb4[2] - b34];
    c
}

/// `postFxControl14/15`: the LUT slices (S+0xac, S+0xae, clamped to the
/// LUT image's `height / 32 - 1`) and their weights (S+0xb4..0xc0).
pub fn slices(prev: u16, cur: u16, weights: [f32; 4], lut_height: u32) -> [[f32; 4]; 2] {
    let last = (lut_height >> 5).max(1) as u16 - 1;
    let c = |s: u16| f32::from(s.min(last));
    [[c(prev), c(cur), c(cur), c(cur)], weights]
}

/// The four colour-vision blocks (15 registers each) and their weights,
/// `postFxConst00..60`, from the vision records' file offsets and weights
/// (`fastfile_t7::native::vision_slots`).
pub fn vision_blocks(image: &[u8], slots: &[(usize, f32)]) -> [[f32; 4]; 61] {
    let mut c = [[0f32; 4]; 61];
    let mut w = [0f32; 4];
    for (k, &(at, weight)) in slots.iter().take(4).enumerate() {
        for r in 0..15 {
            for j in 0..4 {
                let o = at + 16 * r + 4 * j;
                c[15 * k + r][j] = image.get(o..o + 4).map_or(0.0, |b| f32::from_le_bytes(b.try_into().unwrap()));
            }
        }
        w[k] = weight;
    }
    c[60] = w;
    c
}

/// The default vision's words 0..0x84 (`vision/default.vision` as the
/// client parses it) from the shared zones' rawfile text: the file named
/// `name` (`vision/default.vision`) found in `image`, else None.
pub fn vision_words(image: &[u8], name: &str) -> Option<Vec<f32>> {
    let mut key = name.as_bytes().to_vec();
    key.push(0);
    let mut from = 0;
    while let Some(p) = image[from..].windows(key.len()).position(|w| w == key.as_slice()) {
        let at = from + p + key.len();
        let end = image[at..].iter().position(|&b| b == 0).map_or(image.len(), |e| at + e);
        let text = std::str::from_utf8(&image[at..end]).unwrap_or("");
        if text.contains("vkTT") {
            let v = fastfile_t7::native::parse_vision(text);
            return Some(v.words[..34].to_vec());
        }
        from = at;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_reference_temperature_is_white() {
        let m = temperature_matrix(6500.0);
        for i in 0..3 {
            for j in 0..3 {
                let want = if i == j { 1.0 } else { 0.0 };
                assert!((m[i][j] - want).abs() <= 0.002, "{m:?}");
            }
        }
    }

    #[test]
    fn a_neutral_vision_gives_an_identity_matrix() {
        // default.vision: 6500 K, no saturation change, white tint, no offset.
        let mut v = vec![0f32; 34];
        v[0] = 6500.0;
        v[2] = 1.0;
        v[3] = 1.0;
        v[4] = 1.0;
        let c = controls(&v);
        assert!((c[0][0] - 1.0).abs() < 0.002 && c[0][1].abs() < 0.002 && c[0][3] == 0.0, "{c:?}");
        assert!((c[2][2] - 1.0).abs() < 0.002);
    }
}

/// The LUT volume the eye is in (exe 0x141dd7f30): `GfxWorld` +0x188
/// count, +0x190 records of 0x3c (mask byte +0xc, last plane +0xd, first
/// plane u16 +0xe, per-plane falloffs from +0x1c, LUT slice `u16 +0x38 &
/// 0x1f`), planes at +0x1a0 (16 bytes). The first volume (in order) whose
/// mask meets `mask` and whose planes all keep the eye inside (a plane
/// in front within its falloff fades it; past six planes or with no
/// falloff, out) wins: its index and fade, else None.
pub fn lut_volume(image: &[u8], volumes: Option<(usize, usize)>, planes: Option<(usize, usize)>, eye: [f32; 3], mask: u8) -> Option<(usize, f32)> {
    let f = |at: usize| image.get(at..at + 4).map_or(0.0, |b| f32::from_le_bytes(b.try_into().unwrap()));
    let (at, n) = volumes?;
    let (pa, pn) = planes.unwrap_or((0, 0));
    'vol: for i in 0..n {
        let r = at + 0x3c * i;
        if image.get(r + 0xc).copied().unwrap_or(0) & mask == 0 {
            continue;
        }
        let last = usize::from(image.get(r + 0xd).copied().unwrap_or(0));
        let first = image.get(r + 0xe..r + 0x10).map_or(0, |b| usize::from(u16::from_le_bytes([b[0], b[1]])));
        let mut w = 1.0f32;
        for (k, p) in (first..=last).enumerate() {
            if p >= pn {
                continue 'vol;
            }
            let q = pa + 16 * p;
            let d = f(q) * eye[0] + f(q + 4) * eye[1] + f(q + 8) * eye[2] + f(q + 12);
            if 0.0 < d {
                let fall = f(r + 0x1c + 4 * k);
                if k > 5 || fall <= 0.0 {
                    continue 'vol;
                }
                w = w.min(1.0 - (d / fall).clamp(0.0, 1.0));
            }
            if w <= 0.0 {
                continue 'vol;
            }
        }
        return Some((i, w));
    }
    None
}

/// The LUT slices and their blend with the eye settled in `volume` (exe
/// 0x141dd8140: the previous and current volume the same, its slice
/// weighted 1): (slice, (S+0xb4, S+0xb8)).
pub fn settled_slice(image: &[u8], volumes: Option<(usize, usize)>, volume: Option<(usize, f32)>) -> (u16, [f32; 2]) {
    let (slice, w) = match (volume, volumes) {
        (Some((i, w)), Some((at, _))) => (image.get(at + 0x3c * i + 0x38..at + 0x3c * i + 0x3a).map_or(0, |b| u16::from_le_bytes([b[0], b[1]]) & 0x1f), w),
        _ => (0, 1.0),
    };
    let t = 1.0 / (w + 1.0);
    let s = (3.0 - (t + t)) * t * t;
    (slice, [s, 1.0 - s])
}
