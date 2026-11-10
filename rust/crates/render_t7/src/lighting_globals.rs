//! `LightingGlobals` (944 bytes) and the lighting programs' `PerSceneConsts`
//! as the exe fills them for a primary view (`141db8530`, sun constants
//! `141d43c20`; layouts from the programs' RDEF). See `docs/T7_NATIVE.md`,
//! "The lighting pass as the exe records it".
//!
//! What is not filled (zero) and why is listed in [`GAPS`].

use fastfile_t7::native::ViewSun;
use render_h1::code::Camera;

use crate::code::{rows, CodeConsts};

pub const SIZE: usize = 944;

/// Fields left at zero because what fills them is not decoded or not drawn
/// yet (each also changes which permutations classify picks).
pub const GAPS: &[&str] = &[
    "map lights: packed, up to four shadowed a view (crate::spot_shadow); their animation (flicker scale 1) is not drawn; the lighting state is taken as 0",
    "local reflection probes and their blends packed (0x141db8530); visibility by the frustum only (no umbra test)",
    "sun shadow maps (when the map's sun shadow tree is not read, or T7_NO_SUN_SHADOW): a map with sun shadows (mode 3) is lit as the exe lights a view whose shadow flag gives mode 2 (sun on, no cascades: bands FLT_MAX, classify_swizzle); the shadow state's sun constants (3, 37, 40-63) zero (8-11 are the global probe's words 0..3, filled)",
    "viewmodelSunShadowRaw, enableDitheredShadow (a dvar) zero",
    "cloud shadows: the constants are built and their texture is layer (config 0x50 - 1) of the loaded cookie array (GfxWorld +0x260); no staged map's sun has a cloud layer, so the drawn cloud is not seen yet (the array is bound only with opt-in T7_CLOUD_SHADOWS=1)",
    "AO: the exe's default (3, temporal denoisers over the motion vectors); no sub-pixel jitter (no TAA yet); enableAO stays 0 (no lighting permutation reads it)",
];

/// The sun constants (64 floats at 0x110), `141d43c20`, for shadow mode
/// `mode` (0: no sun; 2: sun without shadow maps; 3: cascaded shadow
/// maps, not drawn yet, so never passed here). The shadow state's fields
/// (3, 37, 40-63) are zero: no shadow map is set up; 8-11 are the global
/// probe's config words 0..3 (see [`build`]).
pub fn sun_constants(sun: &ViewSun, mode: i32, time: f32) -> [f32; 64] {
    let mut f = [0f32; 64];
    // Cloud shadows: 24 the layer count (an integer), 25/26 config
    // 0x40/0x64, 28-35 the cloud plane transform: the sun basis (columns
    // w = f x u, u = a unit vector perpendicular to f, f = -direction;
    // `142389480` with angle 0) times the 2-D rotation by
    // `(t·cfg0x4c + cfg0x48)` degrees scaled by cfg0x44, plus the scroll
    // `t·(cfg0x5c, cfg0x60) + (cfg0x54, cfg0x58)`; `t` the view's time.
    f[24] = f32::from_bits(sun.config[0x50 / 4]);
    f[25] = sun.f(0x40);
    f[26] = sun.f(0x64);
    let fwd = [-sun.direction[0], -sun.direction[1], -sun.direction[2]];
    let u = perpendicular(fwd);
    let w = [fwd[1] * u[2] - fwd[2] * u[1], fwd[2] * u[0] - fwd[0] * u[2], fwd[0] * u[1] - fwd[1] * u[0]];
    let angle = (time * sun.f(0x4c) + sun.f(0x48)) * 0.017_453_292;
    let (sn, cs) = (angle.sin() * sun.f(0x44), angle.cos() * sun.f(0x44));
    for k in 0..3 {
        f[28 + k] = cs * w[k] + sn * u[k];
        f[32 + k] = sn * w[k] - cs * u[k];
    }
    f[31] = time * sun.f(0x5c) + sun.f(0x54);
    f[35] = time * sun.f(0x60) + sun.f(0x58);
    f[0..3].copy_from_slice(&sun.direction);
    f[4..7].copy_from_slice(&sun.colour);
    // The builder's colour scale argument is 1.0 (`DAT_142f875ac`).
    f[7] = sun.scale;
    f[36] = sun.intensity;
    match mode {
        0 => f[27] = 1.0,
        // No cascades: their bands are FLT_MAX (`DAT_142f895c0`).
        3 => {}
        _ => f[12..24].fill(f32::MAX),
    }
    f
}

/// A unit vector perpendicular to `p` (`142387d20`): the axis of `p`'s
/// largest component made perpendicular to it, normalised.
fn perpendicular(p: [f32; 3]) -> [f32; 3] {
    let sq = [p[0] * p[0], p[1] * p[1], p[2] * p[2]];
    let mut m = usize::from(sq[1] < sq[0]);
    if sq[2] < sq[m] {
        m = 2;
    }
    let k = -p[m];
    let mut v = [p[0] * k, p[1] * k, p[2] * k];
    v[m] += 1.0;
    let l = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
    if l > 0.0 { v.map(|x| x / l) } else { v }
}

/// `globalProbeConstants` (0x230..0x25f) from the global probe's 328-byte
/// config (`141db8530`): its box centre (words 29-31) relative to the eye,
/// `0.5 / max(1e-4, (w23..25 + w26..28)·0.5)`, and words 4-9 copied as
/// they are.
pub fn global_probe(config: &[u8], eye: [f32; 3]) -> [u32; 12] {
    let w = |k: usize| config.get(4 * k..4 * k + 4).map_or(0, |b| u32::from_le_bytes(b.try_into().unwrap()));
    let f = |k: usize| f32::from_bits(w(k));
    let mut o = [0u32; 12];
    for i in 0..3 {
        o[i] = (f(29 + i) - eye[i]).to_bits();
        let e = ((f(23 + i) + f(26 + i)) * 0.5).max(1e-4);
        o[3 + i] = (0.5 / e).to_bits();
    }
    for i in 0..6 {
        o[6 + i] = w(4 + i);
    }
    o
}

#[derive(Clone, Copy, Debug)]
pub struct Counts {
    pub lights: u32,
    pub ref_probes: u32,
    pub ref_probe_blends: u32,
    pub override_probes: u32,
    /// Words 200..203 (0x320): the first and last volumetric spot and omni
    /// light (`crate::lights::Packed::volumetric_range`).
    pub volumetric_lights: [u32; 4],
    /// 0x330/0x334: the first and last volumetric probe
    /// (`crate::probes::Packed::volumetric_range`).
    pub volumetric_probes: [u32; 2],
}

impl Default for Counts {
    fn default() -> Self {
        Self { lights: 0, ref_probes: 0, ref_probe_blends: 0, override_probes: 0, volumetric_lights: [3000, 0, 3000, 0], volumetric_probes: [0xfa, 0] }
    }
}

/// The whole buffer.
pub fn build(fog: &[f32; 56], sun: &ViewSun, mode: i32, time: f32, probe0: Option<&[u8]>, eye: [f32; 3], width: u32, height: u32, permute_stride: u32, counts: &Counts) -> Vec<u8> {
    let mut b = vec![0u8; SIZE];
    let mut put = |at: usize, v: u32| b[at..at + 4].copy_from_slice(&v.to_le_bytes());
    put(0x0, counts.lights);
    put(0x4, counts.ref_probes);
    put(0x8, counts.ref_probe_blends);
    for (i, v) in fog.iter().enumerate() {
        put(0x30 + 4 * i, v.to_bits());
    }
    for (i, v) in sun_constants(sun, mode, time).iter().enumerate() {
        put(0x110 + 4 * i, v.to_bits());
    }
    if let Some(c) = probe0 {
        for (i, v) in global_probe(c, eye).iter().enumerate() {
            put(0x230 + 4 * i, *v);
        }
        // sunConstants 8..11 (0x130..0x13f): the global probe's config words
        // 0..3 (the probe units and its tint), not shadow state. The view
        // setup (`141ddd1b0`) takes them from the first float block of the
        // lighting state's reflection probe configs (`*(*(scene +
        // 0x1a6bc8) + 0x10)`, the config array of the sun volume's probe
        // set = probe 0, the global probe) into the view (+0x360..+0x36c,
        // and +0x1748..+0x1754), and the sun builder (`141d43c20`) copies
        // them from the shadow state's +0x314 / +0x308..+0x310 (written by
        // `141d42240` from the view in every shadow mode) into floats 8
        // and 9..11. Every deferred permutation multiplies its probe volume
        // weights by float 8 and takes the specular occlusion's upper
        // bound from it: left at zero the indirect diffuse and specular
        // light was black.
        let w = |k: usize| c.get(4 * k..4 * k + 4).map_or(0, |b| u32::from_le_bytes(b.try_into().unwrap()));
        for k in 0..4 {
            put(0x130 + 4 * k, w(k));
        }
    }
    // `BO2MP_T7_NATIVE_DEBUG` (docs/T7_NATIVE.md, "Debug views"): `ambient`
    // and `probe` drop the sun (floats 4..6, its colour), `sunonly` and
    // `shadow` drop the probe volume's diffuse light (float 8, the weight
    // every permutation multiplies it by).
    match crate::debug_views::mode() {
        "ambient" | "probe" => (4..7).for_each(|i| put(0x110 + 4 * i, 0)),
        "sunonly" | "shadow" => put(0x130, 0),
        _ => {}
    }
    // The light and probe packers' volumetric ranges (the volumetric
    // march and cull read them in 32-light words).
    for (k, v) in counts.volumetric_lights.iter().enumerate() {
        put(0x320 + 4 * k, *v);
    }
    put(0x330, counts.volumetric_probes[0]);
    put(0x334, counts.volumetric_probes[1]);
    put(0x374, (width + 63) >> 6);
    put(0x378, (height + 63) >> 6);
    put(0x390, permute_stride);
    put(0x3a0, counts.override_probes);
    b
}

pub const RENDER_TARGET_SIZE: u16 = 0x1;
pub const GAME_TICK: u16 = 0x4;
pub const GAME_TIME: u16 = 0x3;
pub const SUBPIXEL_OFFSET: u16 = 0x5;
pub const REL_HDR_EXPOSURE: u16 = 0x7;
pub const VIEW_SPACE_SCALE_BIAS: u16 = 0x6;
/// `zNear`.
pub const Z_NEAR: u16 = 0x15;
pub const HDR_CONTROL0: u16 = 0xa9;
pub const HDR_CONTROL1: u16 = 0xaa;
pub const VIEW_MATRIX: u16 = 0xd5;
pub const INVERSE_VIEW_MATRIX: u16 = 0xd6;
pub const PROJECTION_MATRIX: u16 = 0xd9;
pub const INVERSE_VIEW_PROJECTION_MATRIX: u16 = 0xde;
pub const INVERSE_PROJECTION_MATRIX: u16 = 0xda;

/// The code constants the lighting programs take, for a view of `width` x
/// `height`: the G-buffer's (eye-relative view-projection, eye offset) and
/// the view and projection matrices and their inverses (eye-relative),
/// `renderTargetSize` `(w, h, 1/w, 1/h)`, `relHDRExposure` `(1, 1, 0, 0)` as
/// the exe sets it every frame, `subpixelOffset` zero (no jitter), and the
/// time constants ([`set_time`]).
pub fn view_consts(camera: &Camera, width: u32, height: u32, time: f32, frame: u64) -> CodeConsts {
    let mut c = CodeConsts::view(camera);
    set_time(&mut c, time, frame);
    let view = camera.rotation;
    let proj = camera.view_projection * view.inverse();
    c.set(VIEW_MATRIX, rows(view));
    c.set(INVERSE_VIEW_MATRIX, rows(view.inverse()));
    c.set(PROJECTION_MATRIX, rows(proj));
    c.set(INVERSE_PROJECTION_MATRIX, rows(proj.inverse()));
    c.set(INVERSE_VIEW_PROJECTION_MATRIX, rows(camera.view_projection.inverse()));
    let (w, h) = (width.max(1) as f32, height.max(1) as f32);
    c.set(RENDER_TARGET_SIZE, vec![[w, h, 1.0 / w, 1.0 / h]]);
    c.set(SUBPIXEL_OFFSET, vec![[0.0; 4]]);
    c.set(REL_HDR_EXPOSURE, vec![[1.0, 1.0, 0.0, 0.0]]);
    // zNear (0x15): x the view's near plane (the forward lists' floatZ
    // conversion, `forward::FloatZ`, and the volumetric march read it).
    c.set(Z_NEAR, vec![[camera.near, 0.0, 0.0, 0.0]]);
    // viewSpaceScaleBias (0x6), as the view setup writes it (`141dddc20`,
    // `crate::ao`): (2·a·t, −2·t, a·t, −t), t = tan(fov_y / 2), a the
    // aspect (a·t = tan(fov_x / 2)). Forward programs read it (`mc/skin`):
    // it was left zero there (`t7_code_consts_audit`).
    c.set(VIEW_SPACE_SCALE_BIAS, vec![[2.0 * camera.tan_x, -2.0 * camera.tan_y, camera.tan_x, -camera.tan_y]]);
    c
}

/// `gameTime` and `gameTick` as `141d909a0` sets them: `gameTime =
/// (sin 2πf, cos 2πf, f, t)` with `f` the fraction of the view's time `t`
/// (seconds); `gameTick = (frame & 1, 1, 1, 0)` (the anti-aliasing modes
/// 4/5, temporal, change the last three; not drawn here).
pub fn set_time(c: &mut CodeConsts, time: f32, frame: u64) {
    let f = time - time.floor();
    let a = f * std::f32::consts::TAU;
    c.set(GAME_TIME, vec![[a.sin(), a.cos(), f, time]]);
    c.set(GAME_TICK, vec![[(frame & 1) as f32, 1.0, 1.0, 0.0]]);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layout() {
        let sun = ViewSun { colour: [1.0, 2.0, 3.0], intensity: 4.0, scale: 5.0, direction: [0.0, 0.0, 1.0], shadow_mode: 3, direction2: [0.0; 3], ..Default::default() };
        let mut cfg = vec![0u8; 328];
        let mut set = |k: usize, v: f32| cfg[4 * k..4 * k + 4].copy_from_slice(&v.to_le_bytes());
        set(29, 10.0);
        set(23, 100.0);
        set(26, 300.0);
        set(4, 7.5);
        let b = build(&[0.25; 56], &sun, 0, 0.0, Some(&cfg), [1.0, 0.0, 0.0], 1920, 1080, 77, &Counts::default());
        let f = |at: usize| f32::from_le_bytes(b[at..at + 4].try_into().unwrap());
        let u = |at: usize| u32::from_le_bytes(b[at..at + 4].try_into().unwrap());
        assert_eq!(f(0x30), 0.25);
        assert_eq!(f(0x118), 1.0);
        assert_eq!(f(0x120), 1.0);
        assert_eq!(f(0x12c), 5.0);
        assert_eq!(f(0x110 + 4 * 27), 1.0);
        assert_eq!(f(0x110 + 4 * 36), 4.0);
        assert_eq!(f(0x230), 9.0);
        assert_eq!(f(0x23c), 0.5 / 200.0);
        assert_eq!(f(0x248), 7.5);
        // (The probe config's words 0..3 are sunConstants 8..11.)
        let mut cfg2 = cfg.clone();
        for (k, v) in [104.5f32, 1.0, 0.9, 1.6].into_iter().enumerate() {
            cfg2[4 * k..4 * k + 4].copy_from_slice(&v.to_le_bytes());
        }
        let b2 = build(&[0.25; 56], &sun, 0, 0.0, Some(&cfg2), [1.0, 0.0, 0.0], 1920, 1080, 77, &Counts::default());
        let f2 = |at: usize| f32::from_le_bytes(b2[at..at + 4].try_into().unwrap());
        assert_eq!((f2(0x130), f2(0x134), f2(0x138), f2(0x13c)), (104.5, 1.0, 0.9, 1.6));
        assert_eq!((u(0x374), u(0x378), u(0x390)), (30, 17, 77));
    }
}
