//! The local reflection probes packed for the deferred lighting as the exe
//! packs them each frame (0x141db8530): per visible probe a cull constant
//! (0x141d41c30/0x141d41d10 + 0x141db1670; after the lights in
//! `gCullConstants`) and a 224-byte `RefProbePackCBuffer` record (0x141d415b0
//! + 0x141db11f0). Probe 0 is the map-wide probe (globalProbeConstants), the
//! others are local: a box (axes, half sizes, centre) with up to six planes.
//!
//! The probe blends (`RefProbeBlendPackCBuffer`, 0x60 each, 0x141d41160)
//! follow each probe's blend list in two passes. Not done (listed): the
//! umbra visibility test (frustum only, which keeps every probe the exe
//! keeps).

use glam::Vec3;
use render_h1::code::Camera;

pub const RECORD: usize = 0xe0;
pub const BLEND: usize = 0x60;
const BLEND_SRC: usize = 0x25c;
/// `RefProbeBlendPackCBuffer` holds 682.
pub const MAX_BLENDS: usize = 65472 / BLEND;
pub const MAX_PROBES: usize = 0x111;
const PROBE: usize = 0x148;

fn f(r: &[u8], at: usize) -> f32 {
    f32::from_le_bytes(r[at..at + 4].try_into().unwrap())
}
fn u(r: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(r[at..at + 4].try_into().unwrap())
}
fn v3(r: &[u8], at: usize) -> Vec3 {
    Vec3::new(f(r, at), f(r, at + 4), f(r, at + 8))
}

pub struct Packed {
    pub count: u32,
    pub overrides: u32,
    pub records: Vec<u8>,
    pub cull: Vec<u8>,
    pub blend_count: u32,
    pub blends: Vec<u8>,
    /// The volumetric probes (byte +0x121 set) by their place in the list,
    /// 32 to a word (`volumetricProbeBits`).
    pub volumetric_bits: Vec<u32>,
    /// `LightingGlobals` +0x330/+0x334: the first and last volumetric probe
    /// (250, 0 when none, the packer's starting values).
    pub volumetric_range: [u32; 2],
}

/// The box of probe record `r` (B = +0x2c): axes, half sizes, centre and its
/// world AABB half extents.
fn probe_box(r: &[u8]) -> ([Vec3; 3], Vec3, Vec3, Vec3) {
    let axes = [v3(r, 0x38), v3(r, 0x44), v3(r, 0x50)];
    let h = (v3(r, 0x5c) + v3(r, 0x68)) * 0.5;
    let centre = v3(r, 0x74);
    let ext = Vec3::new(
        axes[0].x.abs() * h.x + axes[1].x.abs() * h.y + axes[2].x.abs() * h.z,
        axes[0].y.abs() * h.x + axes[1].y.abs() * h.y + axes[2].y.abs() * h.z,
        axes[0].z.abs() * h.x + axes[1].z.abs() * h.y + axes[2].z.abs() * h.z,
    );
    (axes, h, centre, ext)
}

/// Packs probes 1.. of `configs` (0x148 each) for a view.
pub fn pack(configs: &[u8], blend_src: &[u8], camera: &Camera, frustum: &[[f32; 4]; 6]) -> Packed {
    let eye = camera.eye;
    let n = configs.len() / PROBE;
    let mut list: Vec<(f32, usize)> = Vec::new();
    for i in 1..n {
        let r = &configs[PROBE * i..PROBE * (i + 1)];
        // Skipped for this view (+0x140 + view index).
        if r[0x140] != 0 {
            continue;
        }
        let (_, _, centre, ext) = probe_box(r);
        let (lo, hi) = (centre - ext, centre + ext);
        if !crate::lights::box_visible(frustum, lo.to_array(), hi.to_array()) {
            continue;
        }
        list.push((if r[0x122] != 0 { 0.0 } else { 1.0 }, i));
    }
    list.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
    list.truncate(MAX_PROBES);
    let mut records = Vec::with_capacity(list.len() * RECORD);
    let mut cull = Vec::with_capacity(list.len() * crate::lights::CULL);
    let mut overrides = 0;
    let mut blends: Vec<u8> = Vec::new();
    let mut blend_total = 0u32;
    for &(_, i) in &list {
        let r = &configs[PROBE * i..PROBE * (i + 1)];
        if r[0x122] != 0 {
            overrides += 1;
        }
        let (axes, h, centre, ext) = probe_box(r);
        // Cull constant: centre - eye, AABB extents, half sizes, axes.
        let c = centre - eye;
        let words = [c.x, c.y, c.z, ext.x, ext.y, ext.z, h.x, h.y, h.z, axes[0].x, axes[0].y, axes[0].z, axes[1].x, axes[1].y, axes[1].z, axes[2].x, axes[2].y, axes[2].z];
        for v in words {
            cull.extend_from_slice(&v.to_le_bytes());
        }
        cull.extend_from_slice(&[0u8; crate::lights::CULL - 18 * 4]);
        // The blends (two passes over the probe's list: records whose byte
        // 0x258 is the pass).
        let start = blend_total;
        let mut bits = 0u32;
        let mut count = 0u32;
        let (first, n) = (usize::from(u16::from_le_bytes([r[0x28], r[0x29]])), usize::from(u16::from_le_bytes([r[0x2a], r[0x2b]])));
        for pass in 0..2u8 {
            for j in 0..n {
                let Some(b) = blend_src.get(BLEND_SRC * (first + j)..BLEND_SRC * (first + j + 1)) else { continue };
                if b[0x258] != pass {
                    continue;
                }
                let mut sub = 0usize;
                loop {
                    if blend_total as usize >= MAX_BLENDS {
                        break;
                    }
                    let (rows, more) = blend(r, b, sub);
                    if more {
                        bits = bits.wrapping_add(1 << ((2 * count) & 31));
                    } else if pass != 0 && j > 0 {
                        bits = bits.wrapping_add(1 << ((2 * count + 1) & 31));
                    }
                    for v in rows {
                        blends.extend_from_slice(&v.to_le_bytes());
                    }
                    count += 1;
                    blend_total += 1;
                    sub += 1;
                    if !more {
                        break;
                    }
                }
            }
        }
        for w in record(r, i as u32, eye, axes, h, centre, (count, start, bits)) {
            records.extend_from_slice(&w.to_le_bytes());
        }
    }
    let mut volumetric_bits = vec![0u32; list.len().div_ceil(32)];
    let mut volumetric_range = [0xfa, 0];
    for (k, &(_, i)) in list.iter().enumerate() {
        if configs[PROBE * i + 0x121] != 0 || crate::volumetric::test_mode() {
            volumetric_bits[k / 32] |= 1 << (k % 32);
            volumetric_range[0] = volumetric_range[0].min(k as u32);
            volumetric_range[1] = k as u32;
        }
    }
    Packed { count: list.len() as u32, overrides, records, cull, blend_count: blend_total, blends, volumetric_bits, volumetric_range }
}

/// 0x141d415b0's temporary (0x39 words) then 0x141db11f0's 56 words.
fn record(r: &[u8], index: u32, eye: Vec3, axes: [Vec3; 3], h: Vec3, centre: Vec3, blend: (u32, u32, u32)) -> [u32; 56] {
    let mut t = [0u32; 0x39];
    let fb = |v: f32| v.to_bits();
    let c = centre - eye;
    t[0] = fb(c.x);
    t[1] = fb(c.y);
    t[2] = fb(c.z);
    for k in 0..3 {
        t[3 + 3 * k] = fb(axes[k].x);
        t[4 + 3 * k] = fb(axes[k].y);
        t[5 + 3 * k] = fb(axes[k].z);
    }
    let hh = [h.x, h.y, h.z];
    for k in 0..3 {
        t[12 + k] = fb(0.5 / hh[k].max(1e-4));
    }
    let b0 = v3(r, 0x2c);
    let d = centre - b0;
    for (k, v) in [d.x, d.y, d.z].into_iter().enumerate() {
        t[15 + k] = fb(v.min(65504.0).max(-65504.0));
    }
    t[18] = index;
    t[19] = u(r, 0x10);
    t[20] = u(r, 0x14);
    t[21] = u(r, 0x18);
    t[22] = u(r, 0x1c);
    t[23] = u(r, 0x20);
    t[24] = u(r, 0x24);
    // The record's blend word (o[30]) is `count << 25 | index << 16 | start` (read off the deferred
    // programs: low 16 bits the probe's first blend, bits 25..30 how many); `blend` is (count,
    // start, bits). These two were swapped, which gave every local probe zero blends and so full
    // weight over every pixel of every tile whose cull bits held it (8x8 seams).
    t[25] = blend.1; // start
    t[26] = blend.0; // count
    t[27] = blend.2;
    t[28] = u(r, 0x138);
    t[29] = u(r, 0x04);
    t[30] = u(r, 0x08);
    t[31] = u(r, 0x0c);
    t[32] = u(r, 0x00);
    let planes = u(r, 0xb8) as usize;
    for k in 0..6 {
        let at = 33 + 4 * k;
        if planes == 0 {
            let (ax, hk) = (axes[k / 2], hh[k / 2]);
            let s = if k % 2 == 0 { 1.0 } else { -1.0 };
            t[at] = fb(ax.x * s);
            t[at + 1] = fb(ax.y * s);
            t[at + 2] = fb(ax.z * s);
            t[at + 3] = fb(-hk);
        } else if k < planes {
            let p = 0xbc + 16 * k;
            let n = v3(r, p);
            let cl = |v: f32| v.min(1.0).max(-1.0);
            t[at] = fb(cl(n.x));
            t[at + 1] = fb(cl(n.y));
            t[at + 2] = fb(cl(n.z));
            t[at + 3] = fb((f(r, p + 12) - centre.dot(n)).min(65504.0).max(-65504.0));
        } else {
            t[at + 3] = fb(-65504.0);
        }
    }
    let mut o = [0u32; 56];
    o[..4].copy_from_slice(&t[..4]);
    o[4] = t[6];
    o[5] = t[9];
    o[6] = t[4];
    o[7] = t[7];
    o[8] = t[10];
    o[9] = t[5];
    o[10] = t[8];
    o[11..18].copy_from_slice(&t[11..18]);
    o[18..24].copy_from_slice(&t[19..25]);
    o[24..30].copy_from_slice(&t[27..33]);
    o[30] = ((t[26] & 0x3f) << 9 | t[18] & 0x1ff) << 16 | (t[25] & 0xffff);
    o[31..55].copy_from_slice(&t[33..57]);
    o
}

/// One blend (0x141d41160): six planes (24 floats) from blend record `b`
/// (0x25c) for probe `r`, sub-blend `sub`; and whether planes remain for
/// another (`6·sub + 6 < plane count`).
fn blend(r: &[u8], b: &[u8], sub: usize) -> ([f32; 24], bool) {
    let centre = v3(r, 0x74);
    let d = v3(b, 0x00) - centre;
    let count = u(b, 0x60) as usize;
    let mut o = [0f32; 24];
    let k_min = 0.01f32;
    if count == 0 {
        for k in 0..3 {
            let a = v3(b, 0x0c + 12 * k);
            let (r30, r3c, r48, r54) = (f(b, 0x30 + 4 * k), f(b, 0x3c + 4 * k), f(b, 0x48 + 4 * k), f(b, 0x54 + 4 * k));
            let s1 = 1.0 / r54.max(k_min);
            let s2 = 1.0 / r48.max(k_min);
            let n = [-a.x, -a.y, -a.z, r3c + r54 + a.dot(d)];
            let p = [a.x, a.y, a.z, r30 + r48 - a.dot(d)];
            for c in 0..4 {
                o[4 * k + c] = n[c] * s1;
                o[12 + 4 * k + c] = p[c] * s2;
            }
        }
        return (o, false);
    }
    let axes = [v3(b, 0x0c), v3(b, 0x18), v3(b, 0x24)];
    for j in 0..6 {
        let q = 6 * sub + j;
        if q < count {
            let n = v3(b, 0x64 + 16 * q);
            let nw = axes[0] * n.x + axes[1] * n.y + axes[2] * n.z;
            let fall = f(b, 0x1e4 + 4 * q);
            let base = if b[0x258] == 0 { fall } else { 0.0 };
            let w = (base - f(b, 0x70 + 16 * q)) - d.dot(nw);
            let s = 1.0 / fall.max(0.1);
            o[4 * j] = nw.x * s;
            o[4 * j + 1] = nw.y * s;
            o[4 * j + 2] = nw.z * s;
            o[4 * j + 3] = w * s;
        } else {
            o[4 * j] = 1.0;
            o[4 * j + 3] = 2.0 * f(r, 0x2c + 0x54) - d.x;
        }
    }
    (o, 6 * sub + 6 < count)
}
