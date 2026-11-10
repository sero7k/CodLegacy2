//! Bloom as BlackOps3.exe sets it up each view (`docs/T7_NATIVE.md`, "Post
//! chain").
//!
//! * The view's bloom settings (80 floats, post-FX settings S+0x1774) are
//!   a weighted blend of the map's bloom records, made each view
//!   ([`view_settings`]): bloom modifiers, then two bloom box lists, then
//!   the map's base record for the current lighting state (`GfxWorld`
//!   +0x1640 + 0x140·state, [`base_settings`]) for what is left of 1. With
//!   the camera in no bloom modifier or box that is the base record alone
//!   (its first 72 floats: the blend adds only those).
//! * `postFxBloom00..25` are built from them (exe 0x141d611a0,
//!   [`bloom_constants`]).
//! * The render targets the chain draws into (exe's render-target table and
//!   its size classes, [`Level`]).

/// The bloom settings record (80 floats).
pub type BloomSettings = [f32; 80];

/// The map's base bloom record for a lighting state, from the zone image
/// and the `GfxWorld` header's file offset.
pub fn base_settings(image: &[u8], gfx_world_at: usize, state: usize) -> Option<BloomSettings> {
    let at = gfx_world_at + 0x1640 + 0x140 * state.min(3);
    let b = image.get(at..at + 0x140)?;
    let mut s = [0f32; 80];
    for (i, v) in s.iter_mut().enumerate() {
        *v = f32::from_le_bytes(b[i * 4..i * 4 + 4].try_into().ok()?);
    }
    Some(s)
}

/// Floats of a record the view's blend adds (the rest stay zero).
pub const BLENDED: usize = 72;

/// The view's bloom settings, blended as the exe's view setup does: the
/// bloom modifiers (`mods`, 384-byte records: the colour vision's weight by
/// distance, cone and view direction, mask at +0xc, strength at +0x1c,
/// settings at +0x40), capped to a total of 1; then each bloom box list
/// (`boxes`, 464-byte records: six planes, falloffs at +0x60, strength at
/// +0x78, mask at +0x7c, settings at +0x90) filling what is left of 1; then
/// the base record of lighting state `state` for the rest. The mask is the
/// lighting state's bit (`1 << (4 + state)`) with `extra_mask` (the
/// client's). Returns the settings and the records used with their weights.
#[allow(clippy::too_many_arguments)]
pub fn view_settings(image: &[u8], world_header: usize, state: usize, extra_mask: u32, mods: Option<(usize, usize)>, boxes: [Option<(usize, usize)>; 2], eye: [f32; 3], view_dir: [f32; 3]) -> (BloomSettings, Vec<(usize, f32)>) {
    const MOD: usize = 0x180;
    const BOX: usize = 0x1d0;
    let f = |at: usize| image.get(at..at + 4).map_or(0.0, |b| f32::from_le_bytes(b.try_into().unwrap()));
    let mask = (1u32 << (4 + state.min(3))) | extra_mask;
    let mut out = [0f32; 80];
    let mut used = Vec::new();
    let mut add = |out: &mut BloomSettings, at: usize, w: f32| {
        for (k, v) in out.iter_mut().take(BLENDED).enumerate() {
            *v += w * f(at + 4 * k);
        }
        used.push((at, w));
    };
    let mut total = 0.0f64;
    if let Some((at, n)) = mods.filter(|m| m.1 > 0) {
        let ws: Vec<f32> = (0..n).map(|i| fastfile_t7::native::modifier_weight(image, at + MOD * i, mask, eye, view_dir)).collect();
        let sum: f64 = ws.iter().map(|&w| f64::from(w)).sum();
        if sum > 0.0 {
            let scale = if 1.0 < sum { 1.0 / sum } else { 1.0 };
            let mut acc = 0.0f64;
            for (i, &w) in ws.iter().enumerate() {
                let r = at + MOD * i;
                let s = (f64::from(w) * scale) as f32 * f(r + 0x1c);
                if 0.0 < s {
                    add(&mut out, r + 0x40, s);
                    acc += f64::from(s);
                }
            }
            total = acc.min(1.0);
            if 1.0 <= total {
                return (out, used);
            }
        }
    }
    for (k, list) in boxes.into_iter().enumerate() {
        if k == 1 && 1.0 <= total {
            break;
        }
        let Some((at, n)) = list.filter(|l| l.1 > 0) else { continue };
        let ws: Vec<f32> = (0..n).map(|i| fastfile_t7::native::box_weight(image, at + BOX * i, mask, eye)).collect();
        let sum: f64 = ws.iter().map(|&w| f64::from(w)).sum();
        if 0.0 < sum {
            let mut left = 1.0 - total;
            if 1.0 < sum {
                left *= 1.0 / sum;
            }
            for (i, &w) in ws.iter().enumerate() {
                let r = at + BOX * i;
                let s = (f64::from(w) * left) as f32 * f(r + 0x78);
                total += f64::from(s);
                if 0.0 < s {
                    add(&mut out, r + 0x90, s);
                }
            }
            total = total.min(1.0);
        }
    }
    if total < 1.0 {
        add(&mut out, world_header + 0x1640 + 0x140 * state.min(3), (1.0 - total) as f32);
    }
    (out, used)
}

/// `postFxBloom00..25` for a view (`height`/`width` the view's size).
pub fn bloom_constants(r: &BloomSettings, width: u32, height: u32) -> [[f32; 4]; 26] {
    let q = |i: usize| [r[i], r[i + 1], r[i + 2], r[i + 3]];
    let mut c = [[0f32; 4]; 26];
    c[0] = [r[3], r[7], r[11], r[15]];
    c[1] = [r[19], r[23], 1.0 / r[19], 1.0 / r[23]];
    c[2] = [r[0], r[1], r[2], r[3]];
    c[3] = q(4);
    c[4] = q(8);
    c[5] = q(12);
    c[6] = [r[16], r[17], r[18], r[19]];
    c[7] = q(20);
    c[8] = q(24);
    c[9] = q(28);
    c[10] = q(32);
    c[11] = q(36);
    let eps = 6.103_515_6e-5;
    let (lo_a, mut hi_a) = (r[40], r[44]);
    if hi_a <= lo_a {
        hi_a += eps;
    }
    let (lo_b, mut hi_b) = (r[41], r[45]);
    if hi_b <= lo_b {
        hi_b += eps;
    }
    let a = 1.0 / (hi_a - lo_a);
    let b = 1.0 / (hi_b - lo_b);
    c[12] = [a, b, b, 0.0];
    c[13] = [-lo_a * a, -lo_b * b, -lo_b * b, 0.0];
    c[14] = q(48);
    c[15] = [r[56] - r[52], r[57] - r[53], r[57] - r[53], 0.0];
    c[16] = [r[52], r[53], r[53], 0.0];
    c[17] = q(60);
    c[18] = q(64);
    c[19] = q(68);
    // The blur radii, normalised to a 1080-row screen, and the texel width
    // of each bloom level there.
    let aspect = height as f32 / width.max(1) as f32;
    let n = (((1080.0 / aspect) as i64 >> 1) & 0x7fff_ffff) as u64 >> 1;
    let texel = |s: u32| 1.0 / (n >> s) as f32;
    for (k, (ri, tail)) in [(43usize, 0.003_703_703_6f32), (47, 0.007_407_407_3), (51, 0.014_925_373), (55, 0.030_303_031), (59, 0.0625)].into_iter().enumerate() {
        let rad = r[ri] / 1080.0;
        c[20 + k] = [rad * aspect, rad, texel(k as u32), tail];
    }
    c[25] = [1.0 / r[23], r[23], 0.18 / (r[11] + r[7] + r[15]), 0.0];
    c
}

/// A render target of the bloom chain: the exe's id, DXGI format and size
/// (from the view's render size `w` × `h`, by the exe's size classes).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Level {
    pub id: u16,
    pub format: wgpu::TextureFormat,
    pub width: u32,
    pub height: u32,
}

/// The bloom chain's targets for a `w` × `h` view: 0x2d (½, R11G11B10),
/// 0x35..0x3b (¼..1/256, RGBA16F), 0x3c..0x42 (¼..1/256, RGBA16F), 0x2e..0x34
/// (¼..1/256, R11G11B10) and 0x23 (the luminance target, R32F, one texel
/// per 8×8 quarter-size pixels).
pub fn levels(w: u32, h: u32) -> Vec<Level> {
    use wgpu::TextureFormat as F;
    let (wi, hi) = (w as i32, h as i32);
    let shr = |v: i32, s: i32| -> u32 {
        let x = v >> s;
        if s >= 7 && x < 2 { 1 } else { x.max(1) as u32 }
    };
    let mut out = vec![Level { id: 0x2d, format: F::Rg11b10Ufloat, width: shr(wi, 1), height: shr(hi, 1) }];
    for (k, s) in (2..=8).enumerate() {
        out.push(Level { id: 0x35 + k as u16, format: F::Rgba16Float, width: shr(wi, s), height: shr(hi, s) });
        out.push(Level { id: 0x3c + k as u16, format: F::Rgba16Float, width: shr(wi, s), height: shr(hi, s) });
        out.push(Level { id: 0x2e + k as u16, format: F::Rg11b10Ufloat, width: shr(wi, s), height: shr(hi, s) });
    }
    let q = |v: i32| ((v >> 2) + 7) >> 3;
    out.push(Level { id: 0x23, format: F::R32Float, width: q(wi).max(1) as u32, height: q(hi).max(1) as u32 });
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn view_blend_is_the_base_without_volumes_and_fills_from_boxes() {
        // A header with the state-0 base record (all 2.0) and one box
        // record whose planes put the eye well inside (weight 1) with
        // strength 0.25 and settings all 6.0.
        let hdr = 0;
        let bx = 0x2000;
        let mut img = vec![0u8; bx + 0x1d0];
        let mut put = |at: usize, v: f32| img[at..at + 4].copy_from_slice(&v.to_le_bytes());
        for k in 0..80 {
            put(hdr + 0x1640 + 4 * k, 2.0);
            put(bx + 0x90 + 4 * k, 6.0);
        }
        for k in 0..6 {
            // Plane (0, 0, 0, 1)·eye = 1, falloff 10: clamp(10) = 1.
            put(bx + 16 * k + 12, 1.0);
            put(bx + 0x60 + 4 * k, 10.0);
        }
        put(bx + 0x78, 0.25);
        img[bx + 0x7c..bx + 0x80].copy_from_slice(&0x10u32.to_le_bytes());
        let (s, used) = view_settings(&img, hdr, 0, 0, None, [None, None], [0.0; 3], [1.0, 0.0, 0.0]);
        assert_eq!((s[0], s[71], s[72]), (2.0, 2.0, 0.0));
        assert_eq!(used, vec![(0x1640, 1.0)]);
        let (s, used) = view_settings(&img, hdr, 0, 0, None, [Some((bx, 1)), None], [0.0; 3], [1.0, 0.0, 0.0]);
        assert_eq!(used, vec![(bx + 0x90, 0.25), (0x1640, 0.75)]);
        assert_eq!((s[5], s[79]), (0.25 * 6.0 + 0.75 * 2.0, 0.0));
    }

    #[test]
    fn constants_follow_the_settings() {
        let mut r = [1.0f32; 80];
        r[7] = 2.0;
        r[11] = 3.0;
        r[15] = 4.0;
        r[23] = 0.5;
        r[40] = 0.2;
        r[44] = 0.6;
        r[43] = 108.0;
        let c = bloom_constants(&r, 1920, 1080);
        assert_eq!(c[0], [1.0, 2.0, 3.0, 4.0]);
        assert_eq!(c[25][0], 2.0);
        assert!((c[25][2] - 0.18 / 9.0).abs() < 1e-7);
        assert!((c[12][0] - 2.5).abs() < 1e-5 && (c[13][0] + 0.5).abs() < 1e-5);
        // 1920×1080: the quarter-size level is 480 texels wide at 1080 rows.
        assert!((c[20][2] - 1.0 / 480.0).abs() < 1e-7);
        assert!((c[20][1] - 0.1).abs() < 1e-6 && (c[20][0] - 0.05625).abs() < 1e-6);
        assert!((c[24][2] - 1.0 / 30.0).abs() < 1e-6);
    }

    #[test]
    fn levels_are_the_exes_size_classes() {
        let l = levels(1920, 1080);
        let get = |id| l.iter().find(|x| x.id == id).copied().unwrap();
        assert_eq!((get(0x2d).width, get(0x2d).height), (960, 540));
        assert_eq!((get(0x35).width, get(0x35).height), (480, 270));
        assert_eq!((get(0x39).width, get(0x39).height), (30, 16));
        assert_eq!((get(0x3b).width, get(0x3b).height), (7, 4));
        assert_eq!((get(0x23).width, get(0x23).height), (60, 34));
        assert_eq!(get(0x2e).format, wgpu::TextureFormat::Rg11b10Ufloat);
    }

    /// Offline: `T7_BLOOM_ZONE=<map.ff> cargo test -p render_t7 --lib
    /// post_bloom -- --ignored --nocapture`: the view settings at each bloom
    /// box's centre (solved from its three opposite plane pairs).
    #[test]
    #[ignore]
    fn staged_zone() {
        let Ok(path) = std::env::var("T7_BLOOM_ZONE") else { return };
        let z = fastfile_t7::Zone::open(std::path::Path::new(&path)).expect("zone");
        let w = fastfile_t7::walk::walk(&z);
        let g = w.capture.world.gfx_worlds.first().expect("gfxworld");
        let f = |at: usize| f32::from_le_bytes(z.image[at..at + 4].try_into().unwrap());
        println!("t7 bloom: {path}: modifiers {:?}, boxes {:?}", g.bloom_mods, g.bloom_boxes);
        let base = view_settings(&z.image, g.header_at, 0, 0, g.bloom_mods, g.bloom_boxes, [0.0; 3], [1.0, 0.0, 0.0]);
        println!("  at the origin: {:?}; postFxBloom00 {:?}", base.1, bloom_constants(&base.0, 1920, 1080)[0]);
        for (at, n) in g.bloom_boxes.iter().flatten().copied() {
            for i in 0..n {
                let r = at + 0x1d0 * i;
                let pl: Vec<[f32; 4]> = (0..6).map(|k| std::array::from_fn(|j| f(r + 16 * k + 4 * j))).collect();
                let mut rows = Vec::new();
                for a in 0..6 {
                    if rows.len() == 3 {
                        break;
                    }
                    if let Some(b) = (0..6).find(|&b| b != a && (0..3).map(|j| pl[a][j] + pl[b][j]).map(f32::abs).sum::<f32>() < 1e-3) {
                        if !rows.iter().any(|&(x, _)| x == b) {
                            // n·p + d_a ≥ 0 and −n·p + d_b ≥ 0: the middle is n·p = (d_b − d_a) / 2.
                            rows.push((a, (pl[b][3] - pl[a][3]) * 0.5));
                        }
                    }
                }
                if rows.len() < 3 {
                    println!("  box {i}: planes not in opposite pairs");
                    continue;
                }
                let m = glam::Mat3::from_cols_array_2d(&[[pl[rows[0].0][0], pl[rows[1].0][0], pl[rows[2].0][0]], [pl[rows[0].0][1], pl[rows[1].0][1], pl[rows[2].0][1]], [pl[rows[0].0][2], pl[rows[1].0][2], pl[rows[2].0][2]]]);
                let p = m.inverse() * glam::Vec3::new(rows[0].1, rows[1].1, rows[2].1);
                let (s, used) = view_settings(&z.image, g.header_at, 0, 0, g.bloom_mods, g.bloom_boxes, p.to_array(), [1.0, 0.0, 0.0]);
                println!("  box {i} (mask {:#x}, strength {}): at its centre {p:?}: {used:?}; postFxBloom00 {:?}", u32::from_le_bytes(z.image[r + 0x7c..r + 0x80].try_into().unwrap()), f(r + 0x78), bloom_constants(&s, 1920, 1080)[0]);
            }
        }
    }
}
