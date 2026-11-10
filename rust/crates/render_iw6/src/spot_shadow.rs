//! Spot light shadow maps for the `ls1` techniques (`SHADOWMAP_SPOT`), as
//! their pixel shaders read them (translated from the shadowed spot light
//! programs, `lmo_sm_l0s_l0ss_*` / `lpo_sm_l0s_l0ss_*`):
//!
//! ```text
//! sc   = (p - eye, 1) * TRANSPOSE_SHADOW_LOOKUP_MATRIX      // four rows: projective
//! tap  = sampleCmpLevelZero(shadowmapSpot, (sc +- sc.w * PIX.xy|zw).xy / w, sc.z / w)
//! sh   = mean of the four taps                            // PIX = SPOT_SHADOWMAP_PIXEL_ADJUST
//! vis  = baked + LIGHT_SPOTFACTORS.w * (sh - baked)        // baked: the lightmap primary (world)
//! ```
//!
//! So `LIGHT_SPOTFACTORS.w` is the shadow's fade (0: the baked visibility,
//! exactly what `ls0` draws; 1: the shadow map). Every spot light draws with
//! `ls1`; the lights given a shadow map this frame get the fade 1, the
//! others 0 (the engine fades a light's shadow in and out over
//! `sm_spotShadowFadeTime`, 1 s).
//!
//! Up to `sm_spotLimit` (4) lights get a shadow map. INFERRED: which ones
//! (the engine scores lights by intensity, radius and position; the
//! `sm_lightScore_*` dvars name the points it measures between: 64 units in
//! front of the eye and an eighth of the light's radius along its
//! direction; the score itself is not read): here the nearest by that
//! distance among lights whose radius reaches the point. The maps are four
//! square tiles of one atlas, each the light's own perspective from its
//! origin along the direction it shines (`-LIGHT_SPOTDIR`) over its outer
//! cone, depth 0 at the near plane and 1 at its radius (INFERRED: the tile
//! size, the near plane, the tap spacing of a quarter and half texel on a
//! rotated grid, after the sun's `SUN_SHADOWMAP_PIXEL_ADJUST`).

use glam::{Mat4, Vec3, Vec4};

use crate::scene::PrimaryLight;

/// `sm_spotLimit` (and `sm_maxLightsWithShadows`): 4.
pub const SPOT_LIMIT: usize = 4;

/// `BO2MP_IW6_SPOT_SHADOW=0`: no spot shadow maps (`ls0`, the baked
/// visibility).
pub fn on() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| !std::env::var("BO2MP_IW6_SPOT_SHADOW").is_ok_and(|v| v.trim() == "0"))
}

/// One light's shadow map this frame.
#[derive(Clone, Debug)]
pub struct SpotShadow {
    pub light: u16,
    /// Eye-relative world to the light's clip space.
    pub view_projection: Mat4,
    /// The atlas viewport (x, y, size).
    pub viewport: [f32; 3],
    /// Culling planes in world space (`n . p + w >= 0` inside).
    pub planes: Vec<Vec4>,
    /// `TRANSPOSE_SHADOW_LOOKUP_MATRIX`: eye-relative world to atlas
    /// (u w, v w, depth w, w).
    pub lookup: Mat4,
    pub pixel_adjust: [f32; 4],
}

/// The spot lights that get a shadow map for a view at `eye` looking along
/// `forward` (by index in `lights`), nearest first.
pub fn choose(lights: &[PrimaryLight], eye: Vec3, forward: Vec3) -> Vec<u16> {
    let probe = eye + forward.normalize_or_zero() * 64.0;
    let mut c: Vec<(f32, u16)> = lights
        .iter()
        .enumerate()
        .filter(|(_, l)| l.ty == 2 && l.radius > 0.0)
        .filter_map(|(i, l)| {
            let at = Vec3::from(l.origin) - Vec3::from(l.dir) * (0.125 * l.radius);
            let d = at.distance(probe);
            (d < l.radius * 2.0).then_some((d / l.radius, i as u16))
        })
        .collect();
    c.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
    c.into_iter().take(SPOT_LIMIT).map(|(_, i)| i).collect()
}

/// Light `l`'s shadow map in tile `tile` (0..4) of an atlas `2 * res` a
/// side, for a view at `eye`.
pub fn setup(index: u16, l: &PrimaryLight, eye: Vec3, tile: usize, res: u32) -> SpotShadow {
    let origin = Vec3::from(l.origin) - eye;
    let shine = -Vec3::from(l.dir).normalize_or(Vec3::Z);
    let up = if shine.z.abs() > 0.99 { Vec3::X } else { Vec3::Z };
    // The outer cone, a little wider so its edge taps stay in the tile.
    let half = l.cos_half_fov_outer.clamp(-0.99, 0.999).acos().min(1.45) + 0.02;
    let near = (l.radius / 64.0).max(4.0);
    let far = l.radius.max(near + 1.0);
    let view = Mat4::look_to_rh(origin, shine, up);
    let proj = Mat4::perspective_rh(2.0 * half, 1.0, near, far);
    let view_projection = proj * view;
    let (tx, ty) = ((tile % 2) as f32, (tile / 2) as f32);
    // Clip x, y in [-1, 1] to the tile: u = (tx + 0.5 + 0.5 x) / 2.
    let to_uv = Mat4::from_cols(
        Vec4::new(0.25, 0.0, 0.0, 0.0),
        Vec4::new(0.0, -0.25, 0.0, 0.0),
        Vec4::new(0.0, 0.0, 1.0, 0.0),
        Vec4::new(0.5 * tx + 0.25, 0.5 * ty + 0.25, 0.0, 1.0),
    );
    let texel = 1.0 / (2 * res) as f32;
    // The frustum's planes in world space: rows of the clip matrix.
    let m = view_projection * Mat4::from_translation(-eye);
    let (r0, r1, r2, r3) = (m.row(0), m.row(1), m.row(2), m.row(3));
    let planes = [r3 + r0, r3 - r0, r3 + r1, r3 - r1, r2, r3 - r2]
        .into_iter()
        .map(|p| p / p.truncate().length().max(1e-12))
        .collect();
    SpotShadow {
        light: index,
        view_projection,
        viewport: [tx * res as f32, ty * res as f32, res as f32],
        planes,
        lookup: to_uv * view_projection,
        pixel_adjust: [0.25 * texel, 0.5 * texel, 0.5 * texel, -0.25 * texel],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spot() -> PrimaryLight {
        PrimaryLight {
            ty: 2,
            exponent: 1,
            color: [1.0; 3],
            // Toward the light: it shines down.
            dir: [0.0, 0.0, 1.0],
            origin: [100.0, 0.0, 300.0],
            radius: 400.0,
            cos_half_fov_outer: 0.5,
            cos_half_fov_inner: 0.9,
            ..PrimaryLight::default()
        }
    }

    #[test]
    fn a_point_under_the_light_lands_in_its_tile() {
        let eye = Vec3::new(10.0, 20.0, 30.0);
        let s = setup(0, &spot(), eye, 3, 512);
        let p = Vec3::new(100.0, 0.0, 100.0) - eye;
        let sc = s.lookup * p.extend(1.0);
        let (u, v, z) = (sc.x / sc.w, sc.y / sc.w, sc.z / sc.w);
        assert!((u - 0.75).abs() < 1e-4 && (v - 0.75).abs() < 1e-4, "{u} {v}");
        assert!((0.0..1.0).contains(&z));
        // Nearer the light: a smaller depth.
        let q = Vec3::new(100.0, 0.0, 200.0) - eye;
        let sq = s.lookup * q.extend(1.0);
        assert!(sq.z / sq.w < z);
        // Inside every culling plane.
        let w = Vec3::new(100.0, 0.0, 100.0);
        assert!(s.planes.iter().all(|pl| pl.truncate().dot(w) + pl.w >= 0.0));
    }

    #[test]
    fn the_nearest_spots_are_chosen() {
        let mut far = spot();
        far.origin = [5000.0, 0.0, 300.0];
        let mut omni = spot();
        omni.ty = 3;
        let c = choose(&[far, spot(), omni], Vec3::new(100.0, 0.0, 100.0), Vec3::X);
        assert_eq!(c, vec![1]);
    }
}
