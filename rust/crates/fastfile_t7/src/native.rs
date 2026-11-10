//! bo2mp: a Black Ops III map's sun as the game's own renderer takes it
//! (translated from the exe; `docs/T7_NATIVE.md`).
//!
//! A `GfxSunVolume` (0x11b8) holds, from 0x4c, four `GfxGlobalLightSettings`
//! (0xf4 each, one per lighting state 0..3), copied whole by the exe. When a
//! view is set up the exe takes the current state's settings and writes the
//! view's sun config (the struct the sun-constants builder reads):
//!
//! * colour `(c.r, c.g, c.b) * intensity`, the intensity, and settings[6];
//! * the direction from two angles in degrees (pitch = settings[0], yaw =
//!   settings[1]): `(-cos p·cos y, -cos p·sin y, sin p)`;
//! * a second direction the same way from `settings[59..60] + settings[0..1]`;
//! * a shadow mode: 0 when the settings' shadow byte (0x60) is clear, else 3
//!   (2 when the view is a secondary one);
//! * further fields copied straight (settings 0x61, 0x74, 0xb8, 0xbc, 0xc0,
//!   0xc4..0xd3, 0xd4, 0xd8 and the sun volume's 0x10..0x2c).

/// Size of one `GfxGlobalLightSettings` (the exe's own table).
pub const GLOBAL_LIGHT_SETTINGS: usize = 0xf4;
/// Where the four settings sit in a `GfxSunVolume`.
pub const SETTINGS_AT: usize = 0x4c;
const DEG: f32 = 0.017_453_292;

/// One lighting state's settings, as 61 raw words.
#[derive(Clone, Debug, PartialEq)]
pub struct GlobalLightSettings {
    pub words: [u32; 61],
}

impl GlobalLightSettings {
    pub fn f(&self, i: usize) -> f32 {
        f32::from_bits(self.words[i])
    }
    pub fn byte(&self, at: usize) -> u8 {
        self.words[at / 4].to_le_bytes()[at % 4]
    }
}

fn u32_at(b: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(b.get(at..at + 4)?.try_into().ok()?))
}

/// The settings of lighting state `state` (0..3) of the sun volume at
/// `volume` (its offset in the inflated zone).
pub fn global_light_settings(image: &[u8], volume: usize, state: usize) -> Option<GlobalLightSettings> {
    let base = volume + SETTINGS_AT + GLOBAL_LIGHT_SETTINGS * state.min(3);
    let mut words = [0u32; 61];
    for (i, w) in words.iter_mut().enumerate() {
        *w = u32_at(image, base + 4 * i)?;
    }
    Some(GlobalLightSettings { words })
}

/// The view's sun config fields the sun-constants builder reads (offsets
/// relative to the config, as the exe lays it out).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ViewSun {
    /// 0x00: colour × intensity.
    pub colour: [f32; 3],
    /// 0x0c.
    pub intensity: f32,
    /// 0x10 (× the builder's colour scale gives sunConstants[7]).
    pub scale: f32,
    /// 0x14: direction.
    pub direction: [f32; 3],
    /// 0x2c: shadow mode (0 = no sun shadow; 3 = cascades).
    pub shadow_mode: i32,
    /// 0x6c: the second direction.
    pub direction2: [f32; 3],
    /// The whole config 0x00..0x6f as words (`141ddd1b0`): besides the
    /// fields above, 0x20/0x24/0x28/0x34/0x38/0x3c from the sun volume
    /// record (+0x24/+0x28/+0x2c/+0x10/+0x14/+0x18, set by
    /// [`ViewSun::with_volume`]), 0x30 settings word 7, 0x40 word 29,
    /// 0x44 word 46, 0x48 word 47, 0x4c word 48, 0x50 word 53, 0x54..0x60
    /// words 49..52, 0x64 word 54, 0x68 the byte at 0x61. (0x34 can be
    /// overridden by the client; not here.)
    pub config: [u32; 28],
}

impl ViewSun {
    pub fn f(&self, at: usize) -> f32 {
        f32::from_bits(self.config[at / 4])
    }

    /// The config words the exe takes from the sun volume record itself
    /// (`volume`: its offset in the inflated zone).
    pub fn with_volume(mut self, image: &[u8], volume: usize) -> Self {
        for (cfg, rec) in [(0x20, 0x24), (0x24, 0x28), (0x28, 0x2c), (0x34, 0x10), (0x38, 0x14), (0x3c, 0x18)] {
            if let Some(w) = u32_at(image, volume + rec) {
                self.config[cfg / 4] = w;
            }
        }
        self
    }
}

fn angles_dir(pitch_deg: f32, yaw_deg: f32) -> [f32; 3] {
    let (p, y) = (pitch_deg * DEG, yaw_deg * DEG);
    [-(p.cos() * y.cos()), -(p.cos() * y.sin()), p.sin()]
}

/// The exe's view sun setup for one lighting state (a primary view).
pub fn view_sun(s: &GlobalLightSettings) -> ViewSun {
    let intensity = s.f(5);
    let mut config = [0u32; 28];
    let mut put = |at: usize, v: f32| config[at / 4] = v.to_bits();
    put(0x00, s.f(2) * intensity);
    put(0x04, s.f(3) * intensity);
    put(0x08, s.f(4) * intensity);
    put(0x0c, intensity);
    put(0x10, s.f(6));
    let d = angles_dir(s.f(0), s.f(1));
    put(0x14, d[0]);
    put(0x18, d[1]);
    put(0x1c, d[2]);
    let d2 = angles_dir(s.f(59) + s.f(0), s.f(60) + s.f(1));
    put(0x6c, d2[0]);
    for (at, w) in [(0x30, 7), (0x40, 29), (0x44, 46), (0x48, 47), (0x4c, 48), (0x50, 53), (0x54, 49), (0x58, 50), (0x5c, 51), (0x60, 52), (0x64, 54)] {
        config[at / 4] = s.words[w];
    }
    config[0x2c / 4] = if s.byte(0x60) != 0 { 3 } else { 0 };
    config[0x68 / 4] = u32::from(s.byte(0x61));
    ViewSun {
        config,
        colour: [s.f(2) * intensity, s.f(3) * intensity, s.f(4) * intensity],
        intensity,
        scale: s.f(6),
        direction: angles_dir(s.f(0), s.f(1)),
        shadow_mode: if s.byte(0x60) != 0 { 3 } else { 0 },
        direction2: angles_dir(s.f(59) + s.f(0), s.f(60) + s.f(1)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_sun_points_where_its_angles_say() {
        let mut words = [0u32; 61];
        let set = |w: &mut [u32; 61], i: usize, v: f32| w[i] = v.to_bits();
        set(&mut words, 0, 90.0);
        set(&mut words, 2, 1.0);
        set(&mut words, 3, 0.5);
        set(&mut words, 4, 0.25);
        set(&mut words, 5, 4.0);
        words[0x60 / 4] = 1;
        let v = view_sun(&GlobalLightSettings { words });
        assert!((v.direction[2] - 1.0).abs() < 1e-6 && v.direction[0].abs() < 1e-6);
        assert_eq!(v.colour, [4.0, 2.0, 1.0]);
        assert_eq!(v.shadow_mode, 3);
        let mut img = vec![0u8; SETTINGS_AT + GLOBAL_LIGHT_SETTINGS * 4];
        img[SETTINGS_AT + GLOBAL_LIGHT_SETTINGS..SETTINGS_AT + GLOBAL_LIGHT_SETTINGS + 4].copy_from_slice(&7u32.to_le_bytes());
        assert_eq!(global_light_settings(&img, 0, 1).unwrap().words[0], 7);
    }
}

/// A vision file's fields as the exe's parser lays them out (its 36-row
/// table: name, offset in the 0xf0-byte struct, kind: 0 bool, 1 float,
/// 2 vec3, 3 vec4, 4 image name). Keys match case-insensitively; values
/// are quoted. (The table has no `vkM2`.)
pub const VISION_FIELDS: [(&str, usize, u8); 36] = [
    ("vkTT", 0x0, 1), ("vkTS", 0x4, 1), ("vkTC", 0x8, 3), ("vkTO", 0x18, 2),
    ("vkRGB0", 0x24, 2), ("vkL0", 0x30, 1), ("vkM0", 0x34, 1),
    ("vkRGB1", 0x38, 2), ("vkL1", 0x44, 1), ("vkM1", 0x48, 1),
    ("vkRGB2", 0x4c, 2), ("vkL2", 0x58, 1),
    ("vkRGB3", 0x5c, 2), ("vkL3", 0x68, 1), ("vkM3", 0x6c, 1),
    ("vkRGB4", 0x70, 2), ("vkL4", 0x7c, 1), ("vkM4", 0x80, 1), ("vkRM", 0x84, 1),
    ("r_filmLut", 0x88, 4), ("r_reviveFX_enable", 0x8c, 0),
    ("r_reviveFX_edgeColorTemp", 0x90, 1), ("r_reviveFX_edgeSaturation", 0x94, 1),
    ("r_reviveFX_edgeScale", 0x98, 2), ("r_reviveFX_edgeContrast", 0xa4, 2),
    ("r_reviveFX_edgeOffset", 0xb0, 2), ("r_reviveFX_edgeMaskAdjust", 0xbc, 1),
    ("r_reviveFX_edgeAmount", 0xc0, 1), ("snd_masterRingmod", 0xc4, 1),
    ("snd_reverbRingmod", 0xc8, 1), ("snd_hiFilter", 0xcc, 1), ("snd_lowFilter", 0xd0, 1),
    ("r_sunFlareTint", 0xd4, 3), ("r_atmoBlurNS", 0xe4, 1), ("r_atmoBlurHD", 0xe8, 1),
    ("r_atmoBlurBA", 0xec, 1),
];

/// A parsed vision file: the struct's 60 words (floats, a bool as 0/1)
/// and the film LUT's image name (kind 4 keeps the name, not a word).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Vision {
    pub words: Vec<f32>,
    pub film_lut: Option<String>,
    /// Keys the exe's table does not know (it skips them).
    pub unknown: Vec<String>,
}

impl Vision {
    /// Field `name`'s floats (its kind's count).
    pub fn get(&self, name: &str) -> Option<&[f32]> {
        let (_, at, kind) = VISION_FIELDS.iter().find(|f| f.0.eq_ignore_ascii_case(name))?;
        let n = match kind {
            2 => 3,
            3 => 4,
            4 => return None,
            _ => 1,
        };
        self.words.get(at / 4..at / 4 + n)
    }
}

/// Parses a vision file's text (`key "v v v"` per line).
pub fn parse_vision(text: &str) -> Vision {
    let mut v = Vision { words: vec![0.0; 0xf0 / 4], ..Vision::default() };
    for line in text.lines() {
        let line = line.trim();
        let Some((key, rest)) = line.split_once(char::is_whitespace) else { continue };
        let value = rest.trim().trim_matches('"');
        let Some(&(_, at, kind)) = VISION_FIELDS.iter().find(|f| f.0.eq_ignore_ascii_case(key)) else {
            if !key.is_empty() {
                v.unknown.push(key.to_owned());
            }
            continue;
        };
        if kind == 4 {
            v.film_lut = (!value.is_empty()).then(|| value.to_owned());
            continue;
        }
        let n = match kind {
            2 => 3,
            3 => 4,
            _ => 1,
        };
        for (k, part) in value.split_whitespace().take(n).enumerate() {
            let x = if kind == 0 { f32::from(u8::from(part != "0")) } else { part.parse().unwrap_or(0.0) };
            v.words[at / 4 + k] = x;
        }
    }
    v
}

#[cfg(test)]
mod vision_tests {
    #[test]
    fn a_vision_file_reads_into_the_exes_layout() {
        let v = super::parse_vision("vkTT \"6500.000000\"\nvkTC \"1 0.5 0.25 0\"\nVKL4 \"1.0\"\nr_filmLut \"lut_x\"\nfoo \"1\"\n");
        assert_eq!(v.get("vktt"), Some(&[6500.0][..]));
        assert_eq!(v.get("vkTC"), Some(&[1.0, 0.5, 0.25, 0.0][..]));
        assert_eq!(v.words[0x7c / 4], 1.0);
        assert_eq!(v.film_lut.as_deref(), Some("lut_x"));
        assert_eq!(v.unknown, vec!["foo"]);
    }
}

/// The colour-vision blend of a view, as the exe's view setup does it: up to
/// four vision blocks (each the 0xf0-byte vision struct, here their file
/// offsets) and their weights, which become the post-FX settings' four slots
/// (`S + 0x18cc + 0x110·k`) and weights (`S + 0x1d0c`) that `create_lut2dv`
/// reads.
///
/// * the mask is `1 << (4 + lighting state)` or-ed with the client's extra
///   bits (0 unless script set them);
/// * modifiers (336 bytes: origin 0..0xc, flags 0xc, a direction 0x10..0x1c,
///   distance scale/bias 0x20/0x24, cone scale/bias 0x28/0x2c, strength
///   0x1c, vision at 0x40) whose `flags & mask == mask` weigh
///   `smoothstep(clamp(d·scale + bias))` × the cone term (flag bit 8) × the
///   view-direction term (flag bit 9); the sum is scaled to at most 1 and
///   each slot gets `w × scale × strength`;
/// * then each box list (688 bytes: six planes 0..0x60, per-plane falloff
///   0x60..0x78, strength 0x78, mask 0x7c, vision at 0x90) fills what is
///   left of 1, the box weight being `smoothstep(Π clamp(plane·eye ×
///   falloff))`;
/// * the rest of 1 goes to the world's base vision for the lighting state
///   (`GfxWorld + 0x1bb0 + 0x110·state`);
/// * the weights are then divided by their sum. With the camera in no box and
///   no modifier, that is the base vision alone with weight 1.
pub fn vision_slots(
    image: &[u8],
    world_header: usize,
    lighting_state: usize,
    extra_mask: u32,
    mods: Option<(usize, usize)>,
    boxes: [Option<(usize, usize)>; 2],
    eye: [f32; 3],
    view_dir: [f32; 3],
) -> Vec<(usize, f32)> {
    const MOD: usize = 0x150;
    const BOX: usize = 0x2b0;
    let f = |at: usize| u32_at(image, at).map_or(0.0, f32::from_bits);
    let mask = (1u32 << (4 + lighting_state.min(3))) | extra_mask;
    let mut slots: Vec<(usize, f32)> = Vec::new();
    let push = |slots: &mut Vec<(usize, f32)>, at: usize, w: f32| {
        if slots.len() < 4 {
            slots.push((at, w));
        }
    };
    let mut total = 0.0f64;
    // Modifiers.
    if let Some((at, n)) = mods {
        let mut ws = Vec::with_capacity(n);
        let mut sum = 0.0f64;
        for i in 0..n {
            let r = at + MOD * i;
            let w = modifier_weight(image, r, mask, eye, view_dir);
            sum += w as f64;
            ws.push(w);
        }
        if sum > 0.0 {
            let scale = if sum > 1.0 { 1.0 / sum } else { 1.0 };
            for (i, w) in ws.iter().enumerate() {
                let r = at + MOD * i;
                let s = (*w as f64 * scale) as f32 * f(r + 0x1c);
                if s > 0.0 {
                    push(&mut slots, r + 0x40, s);
                    total += s as f64;
                }
            }
            total = total.min(1.0);
            if total >= 1.0 {
                return normalise(slots);
            }
        }
    }
    // Box lists.
    for list in boxes.into_iter().flatten() {
        if total >= 1.0 {
            break;
        }
        let (at, n) = list;
        let mut ws = Vec::with_capacity(n);
        let mut sum = 0.0f64;
        for i in 0..n {
            let r = at + BOX * i;
            let w = box_weight(image, r, mask, eye);
            sum += w as f64;
            ws.push(w);
        }
        if sum > 0.0 {
            let mut left = 1.0 - total;
            if sum > 1.0 {
                left *= 1.0 / sum;
            }
            for (i, w) in ws.iter().enumerate() {
                let r = at + BOX * i;
                let s = (*w as f64 * left) as f32 * f(r + 0x78);
                if s > 0.0 {
                    push(&mut slots, r + 0x90, s);
                    total += s as f64;
                }
            }
            total = total.min(1.0);
        }
    }
    if total < 1.0 {
        push(&mut slots, world_header + 0x1bb0 + 0x110 * lighting_state.min(3), (1.0 - total) as f32);
    }
    normalise(slots)
}

/// The weight of one post-effect modifier (exe `141dd84e0`; the colour
/// vision's 336-byte and the vukko's 64-byte records share its first 0x30
/// bytes): 0 unless `flags & mask == mask`, else
/// `smoothstep(clamp(d·scale + bias))` × the cone term (flag bit 8) × the
/// view-direction term (flag bit 9).
pub fn modifier_weight(image: &[u8], r: usize, mask: u32, eye: [f32; 3], view_dir: [f32; 3]) -> f32 {
    let f = |at: usize| u32_at(image, at).map_or(0.0, f32::from_bits);
    let clamp01 = |v: f32| v.clamp(0.0, 1.0);
    let flags = u32_at(image, r + 0xc).unwrap_or(0);
    if flags & mask != mask {
        return 0.0;
    }
    let d = [f(r) - eye[0], f(r + 4) - eye[1], f(r + 8) - eye[2]];
    let len = (d[0] as f64 * d[0] as f64 + d[1] as f64 * d[1] as f64 + d[2] as f64 * d[2] as f64).sqrt();
    let inv = 1.0 / len;
    let mut v = clamp01(len as f32 * f(r + 0x20) + f(r + 0x24));
    let n3 = [(inv * d[0] as f64) as f32, (inv * d[1] as f64) as f32, (inv * d[2] as f64) as f32];
    if flags >> 8 & 1 != 0 {
        let c = clamp01(n3[0] * f(r + 0x10) + n3[1] * f(r + 0x14) + n3[2] * f(r + 0x18));
        v *= clamp01(c * f(r + 0x28) + f(r + 0x2c));
    }
    if flags >> 9 & 1 != 0 {
        v *= clamp01(n3[0] * view_dir[0] + n3[1] * view_dir[1] + n3[2] * view_dir[2]);
    }
    (3.0 - (v + v)) * v * v
}

/// The weight of one post-effect box (exe `141dd86f0`: six planes at 0,
/// per-plane falloff at 0x60, mask at 0x7c): 0 unless the mask matches,
/// else `smoothstep(Π clamp(plane·eye × falloff))`.
pub fn box_weight(image: &[u8], r: usize, mask: u32, eye: [f32; 3]) -> f32 {
    let f = |at: usize| u32_at(image, at).map_or(0.0, f32::from_bits);
    if u32_at(image, r + 0x7c).unwrap_or(0) & mask != mask {
        return 0.0;
    }
    let mut p = 1.0f32;
    for k in 0..6 {
        let q = r + 16 * k;
        p *= ((eye[1] * f(q + 4) + eye[0] * f(q) + eye[2] * f(q + 8) + f(q + 12)) * f(r + 0x60 + 4 * k)).clamp(0.0, 1.0);
    }
    (3.0 - (p + p)) * p * p
}

/// The vukko blend of a view and the six code constants it gives,
/// `skyMxR/G/B` and `sunMxR/G/B` (0x2d..0x32), as the exe's view setup
/// writes them (`141dd8b90`, into the view's code constants at 0x2d0..0x32c).
///
/// The map keeps 32 vukko records inline at `GfxWorld + 0x600` (0x80 bytes,
/// 32 floats `v`); record 0 is the default. The blend sums records with
/// weights exactly as the colour-vision blend weighs its volumes, with the
/// vision's mask (`1 << (4 + lighting state)` | the client's bits), but
/// without its four-slot limit or a final normalisation:
///
/// * modifiers (64 bytes, `GfxWorld` 0x1610; record index `u16` at 0xe
///   & 0x1f, strength 0x1c): `w = modifier_weight`, the sum capped to 1,
///   each record gets `w × scale × strength`; a total ≥ 1 ends the blend;
/// * the two box lists (144 bytes, 0x1620 and 0x1630; index `u16` at 0x7e
///   & 0x1f, strength 0x78): fill what is left of 1 as the vision boxes do;
/// * what is left of 1 goes to record 0.
///
/// (A script vision override — client weight `+0x1a6a3c`, record `+0x1a6a40`
/// — lerps the sum toward one record; not driven here: weight 0.)
///
/// From the blended `v`: sun direction 1 `D1 = (cos(-v7)cos(-v3),
/// sin(-v7)cos(-v3), sin(-v3))`; direction 2 from pitch `-v19` and yaw
/// `-(v23 + a)` (`a` the sky rotation in radians, as `skyRotationTransition`
/// has it); `t = smoothstep(clamp(clamp(D2·forward) × v27 + v31))`; per-row
/// factors `lerp(1, v[k], t)` for k = 24, 25, 26 (sky rows) and 28, 29, 30
/// (sun rows). Rows:
/// `skyMxR = (v0..2 × f24, D1.x)`, `skyMxG = (v4..6 × f25, D1.y)`,
/// `skyMxB = (v8..10 × f26, D1.z)`, `sunMxR = (v12..14 × f28, v11)`,
/// `sunMxG = (v16..18 × f29, v15)`, `sunMxB = (v20..22 × f30, 1)`.
#[allow(clippy::too_many_arguments)]
pub fn sky_matrices(
    image: &[u8],
    world_header: usize,
    lighting_state: usize,
    extra_mask: u32,
    mods: Option<(usize, usize)>,
    boxes: [Option<(usize, usize)>; 2],
    eye: [f32; 3],
    forward: [f32; 3],
    sky_rotation_rad: f32,
) -> ([[f32; 4]; 6], Vec<(usize, f32)>) {
    const MOD: usize = 0x40;
    const BOX: usize = 0x90;
    let f = |at: usize| u32_at(image, at).map_or(0.0, f32::from_bits);
    let u16_at = |at: usize| image.get(at..at + 2).map_or(0, |b| u16::from_le_bytes([b[0], b[1]]));
    let record = |idx: u16| world_header + 0x600 + 0x80 * (idx as usize & 0x1f);
    let mask = (1u32 << (4 + lighting_state.min(3))) | extra_mask;
    let mut used: Vec<(usize, f32)> = Vec::new();
    let mut total = 0.0f64;
    let mut done = false;
    if let Some((at, n)) = mods {
        let ws: Vec<f32> = (0..n).map(|i| modifier_weight(image, at + MOD * i, mask, eye, forward)).collect();
        let sum: f64 = ws.iter().map(|&w| w as f64).sum();
        if sum > 0.0 {
            let scale = if sum > 1.0 { 1.0 / sum } else { 1.0 };
            let mut acc = 0.0f64;
            for (i, w) in ws.iter().enumerate() {
                let r = at + MOD * i;
                let s = (*w as f64 * scale) as f32 * f(r + 0x1c);
                if s > 0.0 {
                    used.push((record(u16_at(r + 0xe)), s));
                    acc += s as f64;
                }
            }
            total = acc.min(1.0);
            done = total >= 1.0;
        }
    }
    if !done {
        for (k, list) in boxes.into_iter().enumerate() {
            if k > 0 && total >= 1.0 {
                break;
            }
            let Some((at, n)) = list else { continue };
            let ws: Vec<f32> = (0..n).map(|i| box_weight(image, at + BOX * i, mask, eye)).collect();
            let sum: f64 = ws.iter().map(|&w| w as f64).sum();
            if sum > 0.0 {
                let mut left = 1.0 - total;
                if sum > 1.0 {
                    left *= 1.0 / sum;
                }
                for (i, w) in ws.iter().enumerate() {
                    let r = at + BOX * i;
                    let s = (*w as f64 * left) as f32 * f(r + 0x78);
                    if s > 0.0 {
                        used.push((record(u16_at(r + 0x7e)), s));
                        total += s as f64;
                    }
                }
                total = total.min(1.0);
            }
        }
        if total < 1.0 {
            used.push((record(0), (1.0 - total) as f32));
        }
    }
    let mut v = [0.0f32; 32];
    for &(r, w) in &used {
        for (k, x) in v.iter_mut().enumerate() {
            *x += w * f(r + 4 * k);
        }
    }
    let (p1, y1) = (-v[3], -v[7]);
    let d1 = [y1.cos() * p1.cos(), y1.sin() * p1.cos(), p1.sin()];
    let (p2, y2) = (-v[19], -v[23] - sky_rotation_rad);
    let d2 = [y2.cos() * p2.cos(), y2.sin() * p2.cos(), p2.sin()];
    let dot = (d2[0] * forward[0] + d2[1] * forward[1] + d2[2] * forward[2]).clamp(0.0, 1.0);
    let t = (v[27] * dot + v[31]).clamp(0.0, 1.0);
    let t = t * (3.0 - (t + t)) * t;
    let k = |i: usize| v[i] * t + (1.0 - t);
    let row = |b: usize, s: f32, w: f32| [v[b] * s, v[b + 1] * s, v[b + 2] * s, w];
    let rows = [
        row(0, k(24), d1[0]),
        row(4, k(25), d1[1]),
        row(8, k(26), d1[2]),
        row(12, k(28), v[11]),
        row(16, k(29), v[15]),
        row(20, k(30), 1.0),
    ];
    (rows, used)
}

fn normalise(mut slots: Vec<(usize, f32)>) -> Vec<(usize, f32)> {
    let sum: f32 = slots.iter().map(|s| s.1).sum();
    if sum > 0.0 {
        for s in &mut slots {
            s.1 /= sum;
        }
    }
    slots
}

#[cfg(test)]
mod vision_blend_tests {
    use super::*;

    #[test]
    fn sky_matrices_from_the_default_record_alone() {
        let mut img = vec![0u8; 0x2000];
        // Record 0: identity rows, scale/bias 0 (t = 0: factors 1), sun row ws.
        for (i, x) in [1.0f32, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.25, 2.0, 0.0, 0.0, 0.5].into_iter().enumerate() {
            put(&mut img, 0x600 + 4 * i, x);
        }
        put(&mut img, 0x600 + 4 * 16, 3.0);
        let (m, used) = sky_matrices(&img, 0, 0, 0, None, [None, None], [0.0; 3], [1.0, 0.0, 0.0], 0.0);
        assert_eq!(used, vec![(0x600, 1.0)]);
        assert_eq!(m[0], [1.0, 0.0, 0.0, 1.0]);
        assert_eq!(m[1], [0.0, 1.0, 0.0, 0.0]);
        assert_eq!(m[2], [0.0, 0.0, 1.0, 0.0]);
        assert_eq!(m[3], [2.0, 0.0, 0.0, 0.25]);
        assert_eq!(m[4], [3.0, 0.0, 0.0, 0.5]);
        assert_eq!(m[5], [0.0, 0.0, 0.0, 1.0]);
    }

    fn put(img: &mut [u8], at: usize, v: f32) {
        img[at..at + 4].copy_from_slice(&v.to_le_bytes());
    }

    #[test]
    fn no_box_or_modifier_leaves_the_base_vision_alone() {
        let img = vec![0u8; 0x3000];
        let s = vision_slots(&img, 0, 2, 0, None, [None, None], [0.0; 3], [1.0, 0.0, 0.0]);
        assert_eq!(s, vec![(0x1bb0 + 0x110 * 2, 1.0)]);
    }

    #[test]
    fn a_box_the_eye_is_deep_inside_takes_its_strength_and_the_base_the_rest() {
        let mut img = vec![0u8; 0x4000];
        let b = 0x3000;
        // Six planes all facing the eye at distance 10, falloff 1: each term clamps to 1.
        for k in 0..6 {
            put(&mut img, b + 16 * k + 12, 10.0);
            put(&mut img, b + 0x60 + 4 * k, 1.0);
        }
        put(&mut img, b + 0x78, 0.25);
        img[b + 0x7c..b + 0x80].copy_from_slice(&(1u32 << 4).to_le_bytes());
        let s = vision_slots(&img, 0, 0, 0, None, [Some((b, 1)), None], [0.0; 3], [1.0, 0.0, 0.0]);
        assert_eq!(s.len(), 2);
        assert_eq!(s[0].0, b + 0x90);
        assert!((s[0].1 - 0.25).abs() < 1e-6 && (s[1].1 - 0.75).abs() < 1e-6, "{s:?}");
        // Not for another lighting state (mask bit 5).
        let s = vision_slots(&img, 0, 1, 0, None, [Some((b, 1)), None], [0.0; 3], [1.0, 0.0, 0.0]);
        assert_eq!(s, vec![(0x1bb0 + 0x110, 1.0)]);
    }
}
