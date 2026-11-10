//! The map's lights packed for the deferred lighting as the exe packs them
//! each frame (`141db6e10` and its helpers): per light a cull constant
//! (`gCullConstants`, 0x50 bytes, 14 words used; `141db5a70`'s tail) and a
//! 240-byte record in `LightingPackCBuffer` (`141db6bb0` from the
//! intermediate light `141d3fae0` + `141d40a40`).
//!
//! The view's light list is the client's scene lights; here it is the map's
//! primary lights (`ComPrimaryLight`, 564 bytes, the same layout as the
//! renderer's 568-byte light up to +0x234). Not done (listed in the report):
//! the static shadow slots (a shadowed light is packed the way the exe packs
//! one without a static slot: shadow index 0xffff, state 1; its per-view
//! shadow is `crate::spot_shadow`'s, named by the flags word), light
//! animation (flicker/pulse scale taken as 1), the umbra visibility test (lights are culled by the
//! layer mask, colour, distance fade and the view frustum only; a light
//! the exe would cull by umbra lights no pixel the exe lights, since every
//! tile tests the light's own bounds).

use glam::Vec3;
use render_h1::code::Camera;

pub const RECORD: usize = 0xf0;
pub const CULL: usize = 0x50;
/// `LightingPackCBuffer` holds 273 records; the exe keeps at most 0x101.
pub const MAX_LIGHTS: usize = 0x101;
const LIGHT: usize = 564;

pub struct MapLights {
    records: Vec<Vec<u8>>,
}

fn f(r: &[u8], at: usize) -> f32 {
    f32::from_le_bytes(r[at..at + 4].try_into().unwrap())
}
fn u(r: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(r[at..at + 4].try_into().unwrap())
}

/// One frame's packing.
pub struct Packed {
    pub count: u32,
    /// `count` records of 240 bytes.
    pub records: Vec<u8>,
    /// `count` cull constants of 80 bytes (to go first in gCullConstants).
    pub cull: Vec<u8>,
    /// The volumetric lights (byte +0x164 set) by their place in the list,
    /// 32 to a word: spots (`volumetricSpotLightBits`) and omnis
    /// (`volumetricOmniLightBits`), as the packer writes its two bit
    /// buffers.
    pub volumetric_bits: [Vec<u32>; 2],
    /// `LightingGlobals` words 200..203: the first and last volumetric
    /// spot, the first and last volumetric omni (3000, 0 when none, the
    /// packer's starting values).
    pub volumetric_range: [u32; 4],
}

/// The volumetric bit words and range of a list: `(type, volumetric)` per
/// listed light.
pub fn volumetric_bits(list: impl Iterator<Item = (u32, bool)>) -> ([Vec<u32>; 2], [u32; 4]) {
    let mut bits = [Vec::new(), Vec::new()];
    let mut range = [3000, 0, 3000, 0];
    for (i, (ty, vol)) in list.enumerate() {
        let w = i / 32;
        for b in &mut bits {
            if b.len() <= w {
                b.resize(w + 1, 0);
            }
        }
        let k = match ty {
            2 if vol => 0,
            4 if vol => 1,
            _ => continue,
        };
        bits[k][w] |= 1 << (i % 32);
        range[2 * k] = range[2 * k].min(i as u32);
        range[2 * k + 1] = i as u32;
    }
    (bits, range)
}

impl MapLights {
    /// The map zone's primary lights (all `ComWorld`s).
    pub fn read(image: &[u8], capture: &fastfile_t7::walk::capture::Capture) -> Self {
        let mut records = Vec::new();
        for c in &capture.world.com_worlds {
            let Some((at, n)) = c.lights else { continue };
            for i in 0..n {
                if let Some(r) = image.get(at + LIGHT * i..at + LIGHT * (i + 1)) {
                    records.push(r.to_vec());
                }
            }
        }
        Self { records }
    }

    pub fn len(&self) -> usize {
        self.records.len()
    }

    /// The map's lights the packer may shadow (`spot_shadow::eligible`).
    pub fn shadowed(&self) -> usize {
        self.records.iter().filter(|r| r[0x15c] == 0 && crate::spot_shadow::eligible(r)).count()
    }

    pub fn is_empty(&self) -> bool {
        self.records.is_empty()
    }

    /// The volumetric lights (byte +0x164 set: the packer's volumetric
    /// spot / omni bits): `(spots, omnis)`.
    pub fn volumetric_counts(&self) -> (usize, usize) {
        let n = |t: u32| self.records.iter().filter(|r| u(r, 0x40) == t && r[0x164] != 0).count();
        (n(2), n(4))
    }

    /// The spot lights with a cookie: `(word +0x230, origin, direction)`
    /// (checks: a view to put one in).
    pub fn cookie_spots(&self) -> Vec<(u32, [f32; 3], [f32; 3])> {
        self.records.iter().filter(|r| u(r, 0x40) == 2 && u(r, 0x230) != 0).map(|r| (u(r, 0x230), [f(r, 0x48), f(r, 0x4c), f(r, 0x50)], [f(r, 0x9c), f(r, 0xa0), f(r, 0xa4)])).collect()
    }

    /// The spot lights' cookie layers (word +0x230), sorted and counted:
    /// `(layer, spot lights)`, for the `t7 cloud shadows` line.
    pub fn cookie_layers(&self) -> Vec<(u32, usize)> {
        let mut m = std::collections::BTreeMap::new();
        for r in self.records.iter().filter(|r| u(r, 0x40) == 2) {
            *m.entry(u(r, 0x230)).or_insert(0) += 1;
        }
        m.into_iter().collect()
    }

    /// Packs the lights for a view: `layer` the lighting state's bit (`1 <<
    /// state`), `time` the view's time (s), `fade_scale` the refdef's
    /// +0x7c (clamped to 0.4775909 as the exe does outside its special
    /// mode).
    /// `scene`: the frame's scene lights (`fastfile_t7::scene_light`, the
    /// effects' lights), taken after the map's as the exe does
    /// (`141db6e10`'s second loop).
    /// `shadows`: the spot shadow plan and how many lights it may shadow
    /// (`crate::spot_shadow`): the shadowed lights' records name their
    /// shadow record, appended after the `count` light records.
    pub fn pack(&self, camera: &Camera, frustum: &[[f32; 4]; 6], layer: u32, time: f32, fade_scale: f32, scene: &[Vec<u8>], shadows: Option<(&mut crate::spot_shadow::Plan, usize)>) -> Packed {
        let all: Vec<&[u8]> = self.records.iter().map(|r| r.as_slice()).chain(scene.iter().filter(|r| r.len() >= fastfile_t7::scene_light::LIGHT).map(|r| r.as_slice())).collect();
        let eye = camera.eye;
        let fade_scale = fade_scale.min(0.477_590_9);
        let mut list: Vec<(f32, usize)> = Vec::new();
        for (i, r) in all.iter().copied().enumerate() {
            let ty = u(r, 0x40);
            if ty == 0 || u(r, 0x158) & layer == 0 || r[0x15c] != 0 {
                continue;
            }
            let col = [f(r, 0x58), f(r, 0x5c), f(r, 0x60)];
            if col[0] * col[0] + col[1] * col[1] + col[2] * col[2] < 0.001 {
                continue;
            }
            let fade = distance_fade(r, eye, fade_scale);
            if fade == 0.0 {
                continue;
            }
            let (lo, hi) = ([f(r, 0x1f0), f(r, 0x1f4), f(r, 0x1f8)], [f(r, 0x1fc), f(r, 0x200), f(r, 0x204)]);
            if !box_visible(frustum, lo, hi) {
                continue;
            }
            // The sort key (`141db81e0`): 1, or radius / distance beyond it.
            let o = Vec3::new(f(r, 0x48), f(r, 0x4c), f(r, 0x50));
            let d = (o - eye).length();
            let radius = f(r, 0x14c);
            let key = if radius < d { radius / d } else { 1.0 };
            list.push((key, i));
        }
        list.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
        list.truncate(MAX_LIGHTS);
        let mut records = Vec::with_capacity(list.len() * RECORD);
        let mut cull = Vec::with_capacity(list.len() * CULL);
        let mut shadow_records = Vec::new();
        for &(_, i) in &list {
            let r = all[i];
            let fade = distance_fade(r, eye, fade_scale);
            let w = intermediate(r, eye, time, fade);
            let rec = pack(&w);
            for v in rec {
                records.extend_from_slice(&v.to_le_bytes());
            }
            for v in cull_constant(r, eye) {
                cull.extend_from_slice(&v.to_bits().to_le_bytes());
            }
            cull.extend_from_slice(&[0u8; CULL - 14 * 4]);
        }
        if let Some((plan, max)) = shadows {
            let mut cands = Vec::new();
            if max > 0 {
                let inv = camera.view_projection.inverse();
                for (pos, &(_, i)) in list.iter().enumerate() {
                    let r = all[i];
                    if i >= self.records.len() || !crate::spot_shadow::eligible(r) {
                        continue;
                    }
                    let (lo, hi) = crate::spot_shadow::light_box(r);
                    let o = Vec3::new(f(r, 0x48), f(r, 0x4c), f(r, 0x50));
                    let far = (o - eye).length() + (hi - lo).length();
                    let Some((bl, bh)) = crate::spot_shadow::view_box(&inv, eye, camera.near, far, lo, hi) else { continue };
                    let priority = crate::spot_shadow::priority(r, attenuation(r), bl, bh);
                    cands.push(crate::spot_shadow::Candidate { pos, key: i as u32, record: r, priority, dist2: (o - eye).length_squared(), view_box: (bl, bh) });
                }
            }
            let chosen = plan.choose(cands, max, eye);
            for (j, (pos, rec)) in chosen.iter().enumerate() {
                let at = pos * RECORD + 7 * 4;
                let w = u32::from_le_bytes(records[at..at + 4].try_into().unwrap()) | ((j as u32 + 1) & 0xf) << 20;
                records[at..at + 4].copy_from_slice(&w.to_le_bytes());
                for v in rec {
                    shadow_records.extend_from_slice(&v.to_le_bytes());
                }
            }
        }
        records.extend_from_slice(&shadow_records);
        let (volumetric_bits, volumetric_range) = volumetric_bits(list.iter().map(|&(_, i)| (u(all[i], 0x40), all[i][0x164] != 0 || crate::volumetric::test_mode())));
        Packed { count: list.len() as u32, records, cull, volumetric_bits, volumetric_range }
    }
}

/// The intermediate's attenuation words 17..20 (near and far smoothstep
/// along the axis), as [`intermediate`] makes them.
fn attenuation(r: &[u8]) -> [f32; 4] {
    let base = f(r, 0x12c);
    let far = base + f(r, 0x6c);
    let near = base + f(r, 0x68);
    let span = (far - near).max(0.001);
    let a70 = f(r, 0x70);
    let a70 = if a70 > 0.001 { a70.min(1.0) } else { 0.001 };
    let a74 = f(r, 0x74);
    let a74 = if a74 > 0.0 { a74.min(0.999) } else { 0.0 };
    let s = (1.0 - a74) * span;
    [1.0 / (a70 * span), -near / (a70 * span), -1.0 / s, far / s]
}

/// `141db56e0`: the light's distance fade (1 when it has none).
fn distance_fade(r: &[u8], eye: Vec3, scale: f32) -> f32 {
    let start = f(r, 0x80);
    if start <= 0.0 {
        return 1.0;
    }
    let d = (Vec3::new(f(r, 0x48), f(r, 0x4c), f(r, 0x50)) - eye).length();
    let size = if r[0x78] == 0 { f(r, 0x14c) } else { f(r, 0x6c) };
    let mut v = size / (d * scale);
    if u(r, 0x40) == 2 {
        v *= 0.5;
    }
    if v < start {
        return 0.0;
    }
    let t = ((v - start) / (f(r, 0x7c) - start)).clamp(0.0, 1.0);
    (3.0 - 2.0 * t) * t * t
}

pub fn box_visible(planes: &[[f32; 4]; 6], lo: [f32; 3], hi: [f32; 3]) -> bool {
    planes.iter().all(|p| {
        let x = if p[0] >= 0.0 { hi[0] } else { lo[0] };
        let y = if p[1] >= 0.0 { hi[1] } else { lo[1] };
        let z = if p[2] >= 0.0 { hi[2] } else { lo[2] };
        p[0] * x + p[1] * y + p[2] * z + p[3] >= 0.0
    })
}

/// The view frustum's planes (inward, world space) from the camera.
pub fn frustum(camera: &Camera) -> [[f32; 4]; 6] {
    // Eye-relative clip from world: view_projection is eye-relative.
    let m = camera.view_projection * glam::Mat4::from_translation(-camera.eye);
    let r = |i: usize| m.row(i);
    // Reversed depth: near is z = w, far at infinity (z >= 0).
    let ps = [r(3) + r(0), r(3) - r(0), r(3) + r(1), r(3) - r(1), r(3) - r(2), r(2)];
    ps.map(|p| {
        let l = p.truncate().length().max(1e-9);
        [p.x / l, p.y / l, p.z / l, p.w / l]
    })
}

/// The cull constant (`141db5a70`'s tail): box centre and half size (eye
/// relative), the light's offset origin, then for a spot its direction and
/// cone (cos, sin), else its origin and -radius.
fn cull_constant(r: &[u8], eye: Vec3) -> [f32; 14] {
    let lo = Vec3::new(f(r, 0x1f0), f(r, 0x1f4), f(r, 0x1f8));
    let hi = Vec3::new(f(r, 0x1fc), f(r, 0x200), f(r, 0x204));
    let c = (lo + hi) * 0.5 - eye;
    let h = (hi - lo) * 0.5;
    let o = Vec3::new(f(r, 0x48), f(r, 0x4c), f(r, 0x50));
    let dir = Vec3::new(f(r, 0x9c), f(r, 0xa0), f(r, 0xa4));
    let off = o + dir * f(r, 0x12c) - eye;
    let (v, a, b) = if u(r, 0x40) == 2 {
        let cs = f(r, 0x110);
        (dir, cs, (1.0 - cs * cs).max(0.0).sqrt())
    } else {
        (o - eye, -f(r, 0x14c), 0.0)
    };
    [c.x, c.y, c.z, h.x, h.y, h.z, off.x, off.y, off.z, v.x, v.y, v.z, a, b]
}

/// The intermediate light (0x53 words): `141d3fae0` then `141d40a40`.
fn intermediate(r: &[u8], eye: Vec3, time: f32, fade: f32) -> [u32; 0x53] {
    let mut w = [0u32; 0x53];
    let fb = |v: f32| v.to_bits();
    let ty = u(r, 0x40);
    w[0] = ty;
    // No shadow slot: state 1 (`params + 0x7c`).
    w[1] = 1;
    w[2] = u32::from(r[0x164]);
    w[3] = u32::from(r[0x165]);
    let o = Vec3::new(f(r, 0x48), f(r, 0x4c), f(r, 0x50));
    let rel = o - eye;
    w[10] = fb(rel.x);
    w[11] = fb(rel.y);
    w[12] = fb(rel.z);
    w[21] = u(r, 0x54);
    let k = fade * f(r, 0x160);
    w[13] = fb(f(r, 0x58) * k);
    w[14] = fb(f(r, 0x5c) * k);
    w[15] = fb(f(r, 0x60) * k);
    w[16] = fb(f(r, 0x64));
    if ty == 2 {
        w[0x51] = u(r, 0x168);
        w[0x52] = u(r, 0x16c);
    } else if ty == 4 {
        w[0x4f] = u(r, 0x168);
        w[0x50] = u(r, 0x16c);
    }
    let base = f(r, 0x12c);
    let far = base + f(r, 0x6c);
    let near = base + f(r, 0x68);
    let span = (far - near).max(0.001);
    let a70 = f(r, 0x70);
    let a70 = if a70 > 0.001 { a70.min(1.0) } else { 0.001 };
    w[17] = fb(1.0 / (a70 * span));
    w[18] = fb(-near / (a70 * span));
    let a74 = f(r, 0x74);
    let a74 = if a74 > 0.0 { a74.min(0.999) } else { 0.0 };
    let s = (1.0 - a74) * span;
    w[19] = fb(-1.0 / s);
    w[20] = fb(far / s);
    // Shadow index: none (0xffff, `params + 0x6c`).
    w[9] = 0xffff;
    // Shadow params (`params + 0x5c..0x68`): no shadow.
    let mut f94 = f(r, 0x94);
    match ty {
        1 => f94 /= 1024.0,
        2 | 4 => f94 /= 2048.0,
        _ => {}
    }
    w[22] = fb(f94);
    let dir = Vec3::new(f(r, 0x9c), f(r, 0xa0), f(r, 0xa4));
    w[6] = fb(dir.x);
    w[7] = fb(dir.y);
    w[8] = fb(dir.z);
    let p = o + dir * base - eye;
    w[23] = fb(p.x);
    w[24] = fb(p.y);
    w[25] = fb(p.z);

    // `141d40a40`.
    if ty == 1 || ty == 2 {
        // S x L, S the translation by +eye (row vectors), stored transposed.
        let l = |row: usize, col: usize| f(r, 16 * row + 4 * col);
        let s = [[1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0], [eye.x, eye.y, eye.z, 1.0]];
        let mut m = [[0f32; 4]; 4];
        for (i, mr) in m.iter_mut().enumerate() {
            for (j, mv) in mr.iter_mut().enumerate() {
                *mv = (0..4).map(|k2| s[i][k2] * l(k2, j)).sum();
            }
        }
        for j in 0..4 {
            for i in 0..4 {
                w[26 + 4 * j + i] = fb(m[i][j]);
            }
        }
    }
    if ty == 2 {
        let (a0, a1) = (f(r, 0x118), f(r, 0x11c));
        let (b0, b1) = (f(r, 0x120), f(r, 0x124));
        let m0 = a1.max(a0);
        let m1 = b1.max(b0);
        let (x0, x1) = (a0 / m0, a1 / m0);
        let (y0, y1) = (b0 / m1, b1 / m1);
        let c = f(r, 0x114);
        w[0x33] = fb(c);
        w[0x34] = fb(x0);
        w[0x35] = fb(x1);
        w[0x36] = fb(y0);
        w[0x37] = fb(y1);
        if c >= 0.000_488_281_25 {
            w[0x38] = fb(y0 * x0);
            w[0x39] = fb(y1 * x1);
            w[0x3a] = fb(2.0 / c);
            w[0x3b] = fb(-c * 0.5);
        } else {
            let d1 = (x1 - x0).max(0.001);
            let d2 = (y1 - y0).max(0.001);
            w[0x38] = fb(-1.0 / d1);
            w[0x39] = fb(-1.0 / d2);
            w[0x3a] = fb(-x1 * (-1.0 / d1));
            w[0x3b] = fb(-y1 * (-1.0 / d2));
        }
        // The cookie's rotation and scroll.
        let ang = time * f(r, 0x1d4) + f(r, 0x1d0);
        let (sn, cs) = ang.sin_cos();
        let (s0, s1) = (f(r, 0x1d8), f(r, 0x1dc));
        w[0x41] = fb(s0 * 0.5 * cs);
        w[0x42] = fb(s1 * 0.5 * sn);
        w[0x43] = 0;
        w[0x45] = fb(s0 * -0.5 * -sn);
        w[0x46] = fb(s1 * -0.5 * cs);
        w[0x47] = 0;
        let scroll = |speed: f32, base: f32| {
            let sign = if speed < 0.0 { -1.0 } else if speed == 0.0 { 0.0 } else { 1.0 };
            let x = speed.abs() * time;
            ((x - x.trunc()).abs() * sign + base + 0.5).clamp(-65504.0, 65504.0)
        };
        w[0x44] = fb(scroll(f(r, 0x1e0), f(r, 0x1e8)));
        w[0x48] = fb(scroll(f(r, 0x1e4), f(r, 0x1ec)));
        // The cookie's layer in `gCookieArray`: the light's own word at
        // +0x230 (the light packer passes it down to the intermediate as
        // the fourth float of its parameters), not its place in the list.
        w[0x40] = u(r, 0x230);
    } else if ty == 4 {
        w[0x2d] = u(r, 0x38);
        w[0x2e] = u(r, 0x28);
        let k2 = f(r, 0x144);
        w[0x2f] = fb(f(r, 0x138) * k2);
        w[0x30] = fb(f(r, 0x13c) * k2);
        w[0x31] = fb(f(r, 0x140) * k2);
        w[0x32] = fb(k2 * k2);
        w[0x2a] = fb(rel.x);
        w[0x2b] = fb(rel.y);
        w[0x2c] = fb(rel.z);
    }
    w
}

/// `141db6bb0`: the 240-byte record (60 words) from the intermediate.
fn pack(p: &[u32; 0x53]) -> [u32; 60] {
    let mut o = [0u32; 60];
    o[..7].copy_from_slice(&p[10..17]);
    o[7] = (((((p[3] * 2 | p[2] & 1) << 3 | p[1] & 7) << 3 | p[0] & 7) << 4 | p[5] & 0xf) << 4 | p[4] & 0xf) << 16 | (p[9] & 0xffff);
    o[8..17].copy_from_slice(&p[0x11..0x1a]);
    if p[0] != 2 {
        o[0x11..0x1a].copy_from_slice(&p[0x2a..0x33]);
        o[0x1a] = p[0x50];
        o[0x1b] = p[0x4f] & 0x1f;
        return o;
    }
    o[0x11..0x21].copy_from_slice(&p[0x1a..0x2a]);
    o[0x21..0x25].copy_from_slice(&p[0x3c..0x40]);
    o[0x25..0x2b].copy_from_slice(&p[0x49..0x4f]);
    o[0x2b..0x34].copy_from_slice(&p[0x33..0x3c]);
    o[0x34] = p[0x41];
    o[0x35] = p[0x42];
    o[0x36] = p[0x44];
    o[0x37] = p[0x45];
    o[0x38] = p[0x46];
    o[0x39] = p[0x48];
    o[0x3a] = p[0x52];
    o[0x3b] = (p[0x51] & 0x1f) << 8 | (p[0x40] & 0xff);
    o
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn volumetric_bits_and_range() {
        let (b, r) = volumetric_bits([(2, false), (2, true), (4, true), (1, true), (4, false)].into_iter().chain((0..30).map(|_| (2, false))).chain([(2, true)]));
        assert_eq!(b[0], vec![0b10, 0b1000]);
        assert_eq!(b[1], vec![0b100, 0]);
        assert_eq!(r, [1, 35, 2, 2]);
        let (b, r) = volumetric_bits((0..33).map(|i| (2, i == 34 || i == 32)));
        assert_eq!(b[0], vec![0, 1]);
        assert_eq!(r, [32, 32, 3000, 0]);
        assert_eq!(volumetric_bits(std::iter::empty()).1, [3000, 0, 3000, 0]);
    }

    #[test]
    fn the_flags_word_packs_as_the_exe_does() {
        let mut p = [0u32; 0x53];
        p[0] = 2;
        p[1] = 1;
        p[2] = 1;
        p[3] = 1;
        p[9] = 0xffff;
        let o = pack(&p);
        assert_eq!(o[7], ((((3u32 << 3 | 1) << 3 | 2) << 8) << 16) | 0xffff);
    }
}
