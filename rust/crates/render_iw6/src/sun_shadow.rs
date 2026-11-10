//! The sun shadow map the `..._ld1` techniques read (`SHADOWMAP_SUN`), as
//! their pixel shaders use it (`docs/exe_notes/IW6_SHADERS.md` 6): two
//! cascades of one atlas, one lookup matrix for the near cascade
//! (`TRANSPOSE_SHADOW_LOOKUP_MATRIX`: eye-relative position to atlas u, v
//! and depth), the far cascade's coordinates made from the near's
//! (`farUV = nearUV * PART.w + PART.xy`, `SHADOWMAP_SWITCH_PARTITION`), the
//! same depth in both, each faded out over its last eighth
//! (`saturate(8 - max|uv * SCALE.xx|yy + SCALE.zz|zw|)`, `SHADOWMAP_SCALE`),
//! 12 compare taps `SUN_SHADOWMAP_PIXEL_ADJUST` apart, the baked visibility
//! past both.
//!
//! Those maths fix the layout up to a choice: the fade uses one scale and
//! one offset for a cascade's u and v and the same offset for both
//! cascades' u, so each cascade is a square of the atlas and the two share
//! their u range. Here: a 2R x 2R atlas, the near cascade in its top-left
//! quarter, the far one below it (the right half is unused). READ from the
//! dvars: the near cascade's texel, `sm_sunSampleSizeNear` 0.25 units
//! (`near_texel`; `BO2MP_IW6_SHADOW_DVARS=0` returns to the earlier
//! inferred 0.5). INFERRED (the engine's own atlas layout and split are not
//! read): cascades centred on the eye, the far one `far_ratio` times as
//! large (READ: 4 times, `far_ratio`); depth along the sun over the world's bounds; casters drawn with
//! their `build_shadowmap_*` technique (slot 1) and an offset for this depth
//! range (`state::PassState::shadow_bias`: the engine's
//! `sm_polygonOffset*` 128 / 2 gave acne here).

use glam::{Mat4, Vec2, Vec3, Vec4};

/// `sm_sunSampleSizeNear`: world units per texel of the near cascade.
pub const SAMPLE_SIZE_NEAR: f32 = 0.25;
/// The earlier inferred near texel (`BO2MP_IW6_SHADOW_DVARS=0`).
pub const INFERRED_NEAR_TEXEL: f32 = 0.5;

/// `BO2MP_IW6_SHADOW_DVARS=0`: the earlier inferred cascade size instead
/// of `sm_sunSampleSizeNear`.
pub fn dvars_on() -> bool {
    static S: crate::switch::Switch = crate::switch::Switch::on("BO2MP_IW6_SHADOW_DVARS");
    S.get()
}

/// World units per texel of the near cascade.
pub fn near_texel() -> f32 {
    if dvars_on() { SAMPLE_SIZE_NEAR } else { INFERRED_NEAR_TEXEL }
}
/// The far cascade's texel over the near's. READ (the engine's sun
/// shadow setup writes the partitions' texel sizes as `sampleSize` and
/// `4 x sampleSize`): 4. The earlier inferred ratio was 8.
pub const EXE_FAR_RATIO: f32 = 4.0;
/// The earlier inferred far ratio (`BO2MP_IW6_SHADOW_DVARS=0`).
pub const INFERRED_FAR_RATIO: f32 = 8.0;

/// The far cascade's size over the near's: `EXE_FAR_RATIO`
/// (`BO2MP_IW6_SHADOW_FAR_RATIO=n` sets it, 1 to 16; `BO2MP_IW6_SHADOW_DVARS=0`
/// returns to the inferred 8).
pub fn far_ratio() -> f32 {
    static R: std::sync::OnceLock<f32> = std::sync::OnceLock::new();
    *R.get_or_init(|| {
        std::env::var("BO2MP_IW6_SHADOW_FAR_RATIO")
            .ok()
            .and_then(|v| v.trim().parse::<f32>().ok())
            .filter(|r| (1.0..=16.0).contains(r))
            .unwrap_or(if dvars_on() { EXE_FAR_RATIO } else { INFERRED_FAR_RATIO })
    })
}

#[derive(Clone, Debug)]
pub struct Setup {
    /// Per cascade: eye-relative world to clip (x, y in [-1, 1], depth 0 at
    /// the sun's side).
    pub view_projection: [Mat4; 2],
    /// Per cascade: the atlas viewport (x, y, size).
    pub viewport: [[f32; 3]; 2],
    /// Per cascade: culling planes in world space (`n . p + w >= 0`).
    pub planes: [Vec<Vec4>; 2],
    pub lookup: Mat4,
    pub partition: [f32; 4],
    pub scale: [f32; 4],
    pub pixel_adjust: [f32; 4],
    /// Per cascade, what its texels are in the world (not relative to the
    /// eye): centre along right and up, half size, the depth range's start
    /// and length, the sun. Two cascades with equal values hold the same
    /// texels (the key of the cascade cache).
    pub world: [[f32; 8]; 2],
}

/// The light's axes: right, up and forward (from the sun into the world).
pub fn axes(sun: Vec3) -> (Vec3, Vec3, Vec3) {
    let l = sun.normalize_or(Vec3::Z);
    let fwd = -l;
    let up = if fwd.z.abs() > 0.99 { Vec3::X } else { Vec3::Z };
    let right = fwd.cross(up).normalize();
    let up = right.cross(fwd).normalize();
    (right, up, fwd)
}

/// World units per texel of cascade `k` (0 near, 1 far).
pub fn texel(k: usize) -> f32 {
    near_texel() * if k == 0 { 1.0 } else { far_ratio() }
}

/// `BO2MP_IW6_SUN_SNAP=0`: the cascades centred on the eye itself (as
/// before), not on a texel of their own grid.
pub fn snap_on() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| !std::env::var("BO2MP_IW6_SUN_SNAP").is_ok_and(|v| v.trim() == "0"))
}

/// The eye's position along the light's right and up axes.
pub fn eye_on_light(eye: Vec3, sun: Vec3) -> Vec2 {
    let (right, up, _) = axes(sun);
    Vec2::new(right.dot(eye), up.dot(eye))
}

/// READ (the engine's sun shadow setup): each cascade's centre along the
/// light's right and up axes is the eye's, put on the middle of the texel
/// it falls in, `(floor(e / texel) + 0.5) * texel`, so a cascade's texels
/// stay where they are in the world as the eye moves (no crawling edges)
/// and a cascade drawn at one centre holds the same texels as one drawn
/// at a neighbouring centre.
pub fn snapped(at: Vec2, k: usize) -> Vec2 {
    let t = texel(k);
    ((at / t).floor() + Vec2::splat(0.5)) * t
}

/// The cascades for a view at `eye`, a sun toward `sun`, a world inside
/// `bounds`, an atlas of `2 * res` a side, centred on the eye.
pub fn setup(eye: Vec3, sun: Vec3, bounds: (Vec3, Vec3), res: u32) -> Setup {
    setup_at(eye, sun, bounds, res, None)
}

/// As `setup`, with each cascade's centre along the light's right and up
/// axes given (`centres`: near, far; `snapped`), or on the eye (`None`,
/// the maths as before).
pub fn setup_at(eye: Vec3, sun: Vec3, bounds: (Vec3, Vec3), res: u32, centres: Option<[Vec2; 2]>) -> Setup {
    let (right, up, fwd) = axes(sun);
    // Depth range: the world's bounds along the sun (eye-relative).
    let (lo, hi) = bounds;
    let mut dmin = f32::MAX;
    let mut dmax = f32::MIN;
    // (With given centres, the range is taken in the world and then made
    // eye-relative, so it does not drift with the eye.)
    let origin = if centres.is_some() { Vec3::ZERO } else { eye };
    for c in 0..8 {
        let p = Vec3::new(
            if c & 1 == 0 { lo.x } else { hi.x },
            if c & 2 == 0 { lo.y } else { hi.y },
            if c & 4 == 0 { lo.z } else { hi.z },
        ) - origin;
        let d = p.dot(fwd);
        dmin = dmin.min(d);
        dmax = dmax.max(d);
    }
    let (dmin, dmax) = (dmin - 64.0, dmax + 64.0);
    let range = (dmax - dmin).max(1.0);
    let start = dmin;
    let dmin = if centres.is_some() { dmin - fwd.dot(eye) } else { dmin };
    let half_near = near_texel() * res as f32 * 0.5;
    let halves = [half_near, half_near * far_ratio()];
    // Each cascade's centre less the eye's, along right and up.
    let at = eye_on_light(eye, sun);
    let shift = centres.map_or([Vec2::ZERO; 2], |c| [c[0] - at, c[1] - at]);
    let given = centres.is_some();
    let make = |h: f32, s: Vec2| {
        let t = |v: f32| if given { -v / h } else { 0.0 };
        // Rows: x = (right . p - s.x) / h, y = (up . p - s.y) / h,
        // z = (fwd . p - dmin) / range.
        Mat4::from_cols(
            Vec4::new(right.x / h, up.x / h, fwd.x / range, 0.0),
            Vec4::new(right.y / h, up.y / h, fwd.y / range, 0.0),
            Vec4::new(right.z / h, up.z / h, fwd.z / range, 0.0),
            Vec4::new(t(s.x), t(s.y), -dmin / range, 1.0),
        )
    };
    let vp = [make(halves[0], shift[0]), make(halves[1], shift[1])];
    // Clip x, y in [-1, 1] to the near cascade's quarter: u = 0.25 + 0.25 x,
    // v = 0.25 - 0.25 y.
    let to_uv = Mat4::from_cols(
        Vec4::new(0.25, 0.0, 0.0, 0.0),
        Vec4::new(0.0, -0.25, 0.0, 0.0),
        Vec4::new(0.0, 0.0, 1.0, 0.0),
        Vec4::new(0.25, 0.25, 0.0, 1.0),
    );
    let lookup = to_uv * vp[0];
    let k = halves[0] / halves[1];
    // The far cascade's clip from the near's: x1 = k x0 + d, d the near
    // centre less the far one over the far half size.
    let d = (shift[0] - shift[1]) / halves[1];
    let partition = if centres.is_some() {
        [0.25 - 0.25 * k + 0.25 * d.x, 0.75 - 0.25 * k - 0.25 * d.y, 0.0, k]
    } else {
        [0.25 - 0.25 * k, 0.75 - 0.25 * k, 0.0, k]
    };
    let scale = [32.0, 32.0, -8.0, -24.0];
    let texel = 1.0 / (2 * res) as f32;
    let pixel_adjust = [texel, texel, 0.0, 0.0];
    let planes = [0, 1].map(|i| {
        let h = halves[i];
        let c = eye + right * shift[i].x + up * shift[i].y;
        let mut p = Vec::new();
        for (n, d) in [(right, h), (-right, h), (up, h), (-up, h)] {
            // Inside when n . (x - c) <= d: -n . x + (n . c + d) >= 0.
            p.push((-n).extend(n.dot(c) + d));
        }
        p
    });
    Setup {
        view_projection: vp,
        viewport: [[0.0, 0.0, res as f32], [0.0, res as f32, res as f32]],
        planes,
        lookup,
        partition,
        scale,
        pixel_adjust,
        world: [0, 1].map(|i| {
            let c = centres.map_or(at, |c| c[i]);
            [c.x, c.y, halves[i], start, range, sun.x, sun.y, sun.z]
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn far_coordinates_and_fades_follow_the_shader() {
        let s = setup(Vec3::new(100.0, 50.0, 10.0), Vec3::new(0.7, 0.5, 0.5), (Vec3::splat(-4000.0), Vec3::splat(4000.0)), 1024);
        // A point 100 units right of the eye: inside the near cascade.
        let l = Vec3::new(0.7, 0.5, 0.5).normalize();
        let right = (-l).cross(Vec3::Z).normalize();
        let p = right * 100.0;
        let sc = s.lookup * p.extend(1.0);
        let (u, v) = (sc.x, sc.y);
        assert!((0.0..0.5).contains(&u) && (0.0..0.5).contains(&v), "{u} {v}");
        assert!((0.0..=1.0).contains(&sc.z));
        // The far cascade's coordinates by the partition match its own
        // projection placed in the lower quarter.
        let fu = u * s.partition[3] + s.partition[0];
        let fv = v * s.partition[3] + s.partition[1];
        let c = s.view_projection[1] * p.extend(1.0);
        assert!((fu - (0.25 + 0.25 * c.x)).abs() < 1e-4);
        assert!((fv - (0.75 - 0.25 * c.y)).abs() < 1e-4);
        // The fade: full inside, zero at the near cascade's edge.
        let e = |uv: f32, s0: f32, o: f32| (8.0 - (uv * s0 + o).abs()).clamp(0.0, 1.0);
        assert_eq!(e(0.25, s.scale[0], s.scale[2]), 1.0);
        assert_eq!(e(0.5, s.scale[0], s.scale[2]), 0.0);
        assert_eq!(e(0.75, s.scale[1], s.scale[3]), 1.0);
        assert_eq!(e(1.0, s.scale[1], s.scale[3]), 0.0);
    }

    /// With given centres: the near lookup, the partition and the far
    /// projection agree, and a held far cascade puts a world point on the
    /// same texel from two eyes.
    #[test]
    fn snapped_cascades_stay_on_their_texels() {
        let sun = Vec3::new(0.3, -0.6, 0.7);
        let bounds = (Vec3::splat(-4000.0), Vec3::splat(4000.0));
        let res = 1024;
        let eye_a = Vec3::new(103.7, 51.2, 60.0);
        let eye_b = eye_a + Vec3::new(37.9, -21.4, 3.0);
        let at_a = eye_on_light(eye_a, sun);
        let far = snapped(at_a, 1);
        let a = setup_at(eye_a, sun, bounds, res, Some([snapped(at_a, 0), far]));
        let at_b = eye_on_light(eye_b, sun);
        let b = setup_at(eye_b, sun, bounds, res, Some([snapped(at_b, 0), far]));
        assert_eq!(a.world[1], b.world[1]);
        let p = Vec3::new(400.0, -250.0, 20.0);
        for (s, eye) in [(&a, eye_a), (&b, eye_b)] {
            let rel = (p - eye).extend(1.0);
            let near = s.lookup * rel;
            let fu = near.x * s.partition[3] + s.partition[0];
            let fv = near.y * s.partition[3] + s.partition[1];
            let c = s.view_projection[1] * rel;
            assert!((fu - (0.25 + 0.25 * c.x)).abs() < 1e-5, "{fu} {}", 0.25 + 0.25 * c.x);
            assert!((fv - (0.75 - 0.25 * c.y)).abs() < 1e-5);
        }
        let texel_of = |s: &Setup, eye: Vec3| {
            let c = s.view_projection[1] * (p - eye).extend(1.0);
            ((c.x * 0.5 + 0.5) * res as f32, (c.y * 0.5 + 0.5) * res as f32, c.z)
        };
        let (ta, tb) = (texel_of(&a, eye_a), texel_of(&b, eye_b));
        assert!((ta.0 - tb.0).abs() < 1e-2 && (ta.1 - tb.1).abs() < 1e-2, "{ta:?} {tb:?}");
        assert!((ta.2 - tb.2).abs() < 1e-5);
        // A snapped centre is the middle of a texel.
        let n = snapped(at_a, 0);
        let f = (n / texel(0)) - Vec2::splat(0.5);
        assert!((f - f.round()).abs().max_element() < 1e-3);
        // Eye-centred (`None`) is the maths as before.
        let old = setup(eye_a, sun, bounds, res);
        assert_eq!(old.partition, [0.25 - 0.25 / far_ratio(), 0.75 - 0.25 / far_ratio(), 0.0, 1.0 / far_ratio()]);
    }
}
