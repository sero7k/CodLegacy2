//! bo2mp: Black Ops III's scene lights, the lights added for one frame on
//! top of the map's primary lights, and the effect light elements that add
//! them, as BlackOps3.exe does it.
//!
//! * An effect's light element (type 8) holds a light description
//!   (`walk::tables::light_description`, 0x240 bytes: at 0x8 a 0x230-byte
//!   renderer light). Each frame the element draws (`1415760a0`), the exe
//!   copies that light and fills it from the element: colour from the
//!   element's colour bytes (sRGB to linear, red first) times the visual
//!   state's word 0x18, the light's size (0x6c) from word 0x1c, a cone angle
//!   from word 0x20 (90 for a type 4 light) as cos(angle / 2) at 0x10c
//!   (1e-6 for an angle of 0), the description's vector at 0x138 turned by
//!   the element's orientation, the direction (0x9c) the element's forward
//!   axis negated, and the byte at 0x8a set. Each word is sampled as
//!   `base + random * amplitude` over the element's two visual samples
//!   either side of its life fraction; the random table index is the one
//!   the sprites' scale uses (word 0x18 one past it, 0x1c at it, 0x20 three
//!   past it).
//! * `141d40280` then sets the light up: a spot light's cone terms
//!   (`141d3ff20`: 0x12c, 0x130, 0x110), the projection a type 3 or 4 light
//!   keeps at 0x00 (`141d3fa00`/`141d44520`: near and far from 0x68, 0x6c,
//!   0x12c and 0x144), and the radius at 0x14c (a spot's size over its
//!   cone's cosine, at most 65504; a type 4 light's size plus half of 0x144;
//!   otherwise its size).
//! * The light is dropped when its sphere is behind one of the effect
//!   system's cull planes, then added (`141de5fd0`, at most 0x100 a frame):
//!   origin at 0x48, the direction at 0x9c, the description's word 0x238
//!   at 0x230, 0x234 set, -1 at 0x44, 0x34000 at 0x208, and its bounds at
//!   0x1f0 (origin +/- radius; a spot light's the exe takes from the
//!   corners of its frustum, here the sphere's box, which holds them).
//! * The renderer copies the frame's scene lights after the map's visible
//!   primary lights (`141dddc20`) and packs them with them (`141db6e10`).
//!
//! Not done: the per-client brightness an element with flag word 0x400 on
//! multiplies in (taken as 1), the exclusion volumes an element with flag
//! 0x8000 skips its light inside (`1415d54c0`), a spot light's frustum
//! matrix at 0x00 (the packer reads only a type 4 light's).

use std::sync::Mutex;

/// The renderer light.
pub const LIGHT: usize = 0x238;
/// A light description.
pub const DESCRIPTION: usize = 0x240;
/// The scene's cap (`141de5fd0`).
pub const MAX: usize = 0x100;
/// The random table index the light's words are sampled with, past the
/// element's seed (the sprites' scale channel).
pub const RAND_CHANNEL: u32 = 28;

static FRAME: Mutex<Vec<Vec<u8>>> = Mutex::new(Vec::new());

/// The effects' scene lights for the frame just drawn (replaces the last).
pub fn publish(mut lights: Vec<Vec<u8>>) {
    lights.truncate(MAX);
    if let Ok(mut f) = FRAME.lock() {
        *f = lights;
    }
}

/// The latest frame's scene lights.
pub fn current() -> Vec<Vec<u8>> {
    FRAME.lock().map(|f| f.clone()).unwrap_or_default()
}

fn f(r: &[u8], at: usize) -> f32 {
    f32::from_le_bytes(r[at..at + 4].try_into().unwrap())
}
fn u(r: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(r[at..at + 4].try_into().unwrap())
}
fn put(r: &mut [u8], at: usize, v: f32) {
    r[at..at + 4].copy_from_slice(&v.to_le_bytes());
}
fn put_u(r: &mut [u8], at: usize, v: u32) {
    r[at..at + 4].copy_from_slice(&v.to_le_bytes());
}

/// One element's light inputs for this frame.
#[derive(Clone, Copy, Debug)]
pub struct ElemLight {
    pub origin: [f32; 3],
    /// Forward, left, up.
    pub axis: [[f32; 3]; 3],
    /// The element's colour bytes, red first.
    pub colour: [u8; 3],
    /// Visual state word 0x18.
    pub intensity: f32,
    /// Word 0x1c.
    pub size: f32,
    /// Word 0x20 (degrees).
    pub angle: f32,
}

/// A visual sample's light word (0x18, 0x1c or 0x20) at a sample point: the
/// two 80-byte samples (base 40, amplitude 40) `floor` and `floor + 1`
/// blended by `frac`, each `base + random * amplitude`.
pub fn sample_word(samples: &[u8], floor: usize, frac: f32, word: usize, random: f32) -> Option<f32> {
    let at = |i: usize| -> Option<f32> {
        let s = samples.get(i * 80..i * 80 + 80)?;
        Some(f(s, word) + random * f(s, word + 40))
    };
    let v0 = at(floor)?;
    let v1 = at(floor + 1).unwrap_or(v0);
    Some(v1 * frac + v0 * (1.0 - frac))
}

/// The element colour as the sprites take it: per channel the sample's
/// base blended to its amplitude by `random`, the two samples by `frac`.
pub fn sample_colour(samples: &[u8], floor: usize, frac: f32, random: f32) -> Option<[u8; 3]> {
    let s0 = samples.get(floor * 80..floor * 80 + 80)?;
    let s1 = samples.get((floor + 1) * 80..(floor + 1) * 80 + 80).unwrap_or(s0);
    let mut out = [0u8; 3];
    for c in 0..3 {
        let v0 = s0[c] as f32 * (1.0 - random) + s0[40 + c] as f32 * random;
        let v1 = s1[c] as f32 * (1.0 - random) + s1[40 + c] as f32 * random;
        out[c] = (v0 * (1.0 - frac) + v1 * frac).round().clamp(0.0, 255.0) as u8;
    }
    Some(out)
}

fn linear(c: u8) -> f32 {
    let c = c as f32 / 255.0;
    if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) }
}

fn rotate(axis: &[[f32; 3]; 3], v: [f32; 3]) -> [f32; 3] {
    let mut o = [0.0; 3];
    for k in 0..3 {
        o[k] = v[0] * axis[0][k] + v[1] * axis[1][k] + v[2] * axis[2][k];
    }
    o
}

/// `141d3ff20`: a spot light's cone terms.
fn spot_terms(l: &mut [u8]) {
    if u(l, 0x40) != 2 {
        return;
    }
    let mut c = f(l, 0x10c);
    put(l, 0x12c, 0.0);
    put(l, 0x130, c);
    if f(l, 0x128) > 0.0 {
        let size = f(l, 0x6c);
        let p = f(l, 0x128).powf(4.0);
        let room = (16384.0 - size).max(0.0);
        put(l, 0x12c, p * room);
        let k = size / (p * room + size);
        let v = ((1.0 - c * c) / (c * c)) * k * k + 1.0;
        c = 1.0 / v.sqrt();
        put(l, 0x130, c);
    }
    let t = 1.0 / c;
    let g = (1.0 - f(l, 0x114)) * t;
    put(l, 0x110, 1.0 / (g * g + t * t).sqrt());
}

/// `141d3fa00` with `141d44520`: the projection a light other than a sun
/// or spot keeps at 0x00.
fn projection(l: &mut [u8]) {
    let (near, span) = if u(l, 0x40) == 4 {
        let n = f(l, 0x68);
        (n.max(1.0), (f(l, 0x6c) - n) + f(l, 0x144) * 0.5)
    } else {
        let n = f(l, 0x12c) + f(l, 0x68);
        (n.max(1.0), (f(l, 0x12c) + f(l, 0x6c)) - n)
    };
    let far = (near + span).max(near + 1.0);
    for i in 0..16 {
        put(l, i * 4, 0.0);
    }
    put(l, 0x00, 1.0);
    put(l, 0x14, 1.0);
    put(l, 0x28, -near / (far - near));
    put(l, 0x2c, 1.0);
    put(l, 0x38, near * far / (far - near));
}

/// `1415760a0`, `141d40280` and `141de5fd0`: the scene light an effect
/// light element adds this frame, before the cull planes (see
/// [`behind_planes`]). None for a description of the wrong size or a light
/// with no radius.
pub fn elem_light(description: &[u8], e: &ElemLight) -> Option<Vec<u8>> {
    if description.len() < DESCRIPTION {
        return None;
    }
    let mut l = vec![0u8; LIGHT];
    l[..0x230].copy_from_slice(&description[8..0x238]);
    let ty = u(&l, 0x40);
    put(&mut l, 0x58, linear(e.colour[0]) * e.intensity);
    put(&mut l, 0x5c, linear(e.colour[1]) * e.intensity);
    put(&mut l, 0x60, linear(e.colour[2]) * e.intensity);
    put(&mut l, 0x50, e.origin[2]);
    put(&mut l, 0x6c, e.size);
    let angle = if ty == 4 { 90.0 } else { e.angle };
    put(&mut l, 0x10c, if angle != 0.0 { (angle * 0.017453292 * 0.5).cos() } else { 1e-6 });
    let v = rotate(&e.axis, [f(description, 0x140), f(description, 0x144), f(description, 0x148)]);
    put(&mut l, 0x138, v[0]);
    put(&mut l, 0x13c, v[1]);
    put(&mut l, 0x140, v[2]);
    put(&mut l, 0x144, f(description, 0x14c));
    l[0x8a] = 1;
    // 141d40280
    spot_terms(&mut l);
    if !(ty == 1 || ty == 2) {
        projection(&mut l);
    }
    let radius = match ty {
        2 if f(&l, 0x10c) > 0.0 => (f(&l, 0x6c) / f(&l, 0x10c)).clamp(0.0, 65504.0),
        2 => f(&l, 0x14c),
        4 => f(&l, 0x144) * 0.5 + f(&l, 0x6c),
        _ => f(&l, 0x6c),
    };
    put(&mut l, 0x14c, radius);
    if radius <= 0.0 {
        return None;
    }
    // 141de5fd0
    put_u(&mut l, 0x230, u(description, 0x238));
    l[0x234] = 1;
    for k in 0..3 {
        put(&mut l, 0x48 + 4 * k, e.origin[k]);
        put(&mut l, 0x9c + 4 * k, -e.axis[0][k]);
        put(&mut l, 0x1f0 + 4 * k, e.origin[k] - radius);
        put(&mut l, 0x1fc + 4 * k, e.origin[k] + radius);
    }
    put_u(&mut l, 0x44, 0xffff_ffff);
    put_u(&mut l, 0x208, 0x34000);
    Some(l)
}

/// The exe's cull before the add: the light's sphere wholly behind one of
/// the planes (normal, distance).
pub fn behind_planes(light: &[u8], planes: &[[f32; 4]]) -> bool {
    let o = [f(light, 0x48), f(light, 0x4c), f(light, 0x50)];
    let r = f(light, 0x14c);
    planes.iter().any(|p| o[0] * p[0] + o[1] * p[1] + o[2] * p[2] - p[3] <= -r)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn description(ty: u32) -> Vec<u8> {
        let mut d = vec![0u8; DESCRIPTION];
        d[8 + 0x40..8 + 0x44].copy_from_slice(&ty.to_le_bytes());
        d[0x238..0x23c].copy_from_slice(&7u32.to_le_bytes());
        d
    }

    #[test]
    fn an_omni_element_light_as_the_exe_adds_it() {
        let e = ElemLight {
            origin: [10.0, 20.0, 30.0],
            axis: [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
            colour: [255, 0, 128],
            intensity: 2.0,
            size: 100.0,
            angle: 40.0,
        };
        let l = elem_light(&description(3), &e).unwrap();
        assert_eq!(f(&l, 0x58), 2.0);
        assert_eq!(f(&l, 0x5c), 0.0);
        assert!((f(&l, 0x60) - linear(128) * 2.0).abs() < 1e-6);
        assert_eq!(f(&l, 0x14c), 100.0);
        assert_eq!([f(&l, 0x48), f(&l, 0x4c), f(&l, 0x50)], [10.0, 20.0, 30.0]);
        assert_eq!([f(&l, 0x9c), f(&l, 0xa0), f(&l, 0xa4)], [-1.0, -0.0, -0.0]);
        assert_eq!(u(&l, 0x230), 7);
        assert_eq!(l[0x234], 1);
        assert_eq!(u(&l, 0x44), 0xffff_ffff);
        assert_eq!(u(&l, 0x208), 0x34000);
        assert_eq!([f(&l, 0x1f0), f(&l, 0x204)], [-90.0, 130.0]);
        assert!((f(&l, 0x10c) - (20.0f32.to_radians()).cos()).abs() < 1e-6);
        // Near 1 (0x68 and 0x12c zero), far near plus the size.
        assert!((f(&l, 0x28) - (-1.0 / 100.0)).abs() < 1e-6);
        assert!((f(&l, 0x38) - (101.0 / 100.0)).abs() < 1e-5);
        assert!(!behind_planes(&l, &[[1.0, 0.0, 0.0, 100.0]]));
        assert!(behind_planes(&l, &[[1.0, 0.0, 0.0, 111.0]]));
    }

    #[test]
    fn a_spot_radius_is_its_size_over_its_cone() {
        let e = ElemLight {
            origin: [0.0; 3],
            axis: [[0.0, 0.0, -1.0], [0.0, 1.0, 0.0], [1.0, 0.0, 0.0]],
            colour: [255; 3],
            intensity: 1.0,
            size: 50.0,
            angle: 120.0,
        };
        let l = elem_light(&description(2), &e).unwrap();
        assert!((f(&l, 0x14c) - 100.0).abs() < 1e-3);
        assert_eq!([f(&l, 0x9c), f(&l, 0xa0), f(&l, 0xa4)], [-0.0, -0.0, 1.0]);
        // A type 4 light takes a 90 degree cone.
        let l = elem_light(&description(4), &e).unwrap();
        assert!((f(&l, 0x10c) - 45.0f32.to_radians().cos()).abs() < 1e-6);
    }

    #[test]
    fn words_blend_base_plus_random_amplitude() {
        let mut s = vec![0u8; 160];
        s[0x18..0x1c].copy_from_slice(&1.0f32.to_le_bytes());
        s[0x40..0x44].copy_from_slice(&2.0f32.to_le_bytes());
        s[80 + 0x18..80 + 0x1c].copy_from_slice(&3.0f32.to_le_bytes());
        assert_eq!(sample_word(&s, 0, 0.5, 0x18, 0.5), Some(2.5));
    }
}
