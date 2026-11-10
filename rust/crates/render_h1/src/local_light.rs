//! A spot or omni light's lighting constants, as the game's per-light
//! setter fills them (docs/H1_NATIVE.md, "Spot and omni lights"). Light
//! slot `n` writes `LIGHT_*` for `n` 0 and `LIGHT_*<n>` for 1..3.
//!
//! This only builds the values. Nothing draws with them yet: the technique
//! slots of the spot and omni lit passes are not read (the exe holds no
//! names for them), so the passes cannot be chosen without a GPU to check.
//!
//! * READ: position is eye-relative origin with `1 / radius` in w; diffuse
//!   and specular are the colour times the same scales as the sun; the
//!   physical size is the bulb length, the two values after it, then the
//!   bulb radius; fade offset is (x, y, 0, 0); the spot direction carries
//!   the shadow slot in w; the spot factors are `1 / (cosInner - cosOuter)`,
//!   `-cosOuter / (cosInner - cosOuter)`, the exponent, and a per-draw value.
//! * INFERRED: the w of diffuse and specular is 1 (as the sun's), the
//!   shadow slot is 0 (no shadow), the per-draw value is 0, and a cone with
//!   equal cosines divides by a tiny value instead of zero.

use fastfile_h1::map::PrimaryLight;
use glam::Vec3;

use crate::code::CodeValues;
use crate::light::ColorScales;

/// The constant names of light slot `n` (0..=3).
pub fn names(n: usize) -> [String; 6] {
    let sfx = if n == 0 { String::new() } else { n.to_string() };
    ["POSITION", "DIFFUSE", "SPECULAR", "PHYSICALSIZE", "FADEOFFSET", "SPOTDIR"]
        .map(|k| format!("LIGHT_{k}{sfx}"))
}

/// `LIGHT_SPOTFACTORS<n>`.
pub fn spot_factors_name(n: usize) -> String {
    if n == 0 { "LIGHT_SPOTFACTORS".into() } else { format!("LIGHT_SPOTFACTORS{n}") }
}

/// The values of one spot (type 2) or omni (type 3) light for an eye.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LocalLight {
    pub position: [f32; 4],
    pub diffuse: [f32; 4],
    pub specular: [f32; 4],
    pub physical_size: [f32; 4],
    pub fade_offset: [f32; 4],
    pub spot_dir: [f32; 4],
    pub spot_factors: [f32; 4],
}

impl LocalLight {
    /// None for a sun or the empty slot 0, or a light with no radius.
    pub fn new(l: &PrimaryLight, eye: Vec3, scales: ColorScales) -> Option<Self> {
        if !matches!(l.ty, 2 | 3) || l.radius <= 0.0 {
            return None;
        }
        let o = Vec3::from(l.origin) - eye;
        let c = Vec3::from(l.color);
        let range = (l.cos_half_fov_inner - l.cos_half_fov_outer).max(1e-6);
        Some(Self {
            position: [o.x, o.y, o.z, 1.0 / l.radius],
            diffuse: [c.x * scales.diffuse, c.y * scales.diffuse, c.z * scales.diffuse, 1.0],
            specular: [c.x * scales.specular, c.y * scales.specular, c.z * scales.specular, 1.0],
            physical_size: [l.bulb_length[0], l.bulb_length[1], l.bulb_length[2], l.bulb_radius],
            fade_offset: [l.fade_offset[0], l.fade_offset[1], 0.0, 0.0],
            spot_dir: [l.dir[0], l.dir[1], l.dir[2], 0.0],
            spot_factors: [
                1.0 / range,
                -l.cos_half_fov_outer / range,
                f32::from(l.exponent),
                0.0,
            ],
        })
    }

    /// Writes it into light slot `n` of the code constants.
    pub fn write(&self, n: usize, code: &mut CodeValues) {
        let [p, d, s, ps, fo, sd] = names(n);
        code.set_vec(&p, self.position);
        code.set_vec(&d, self.diffuse);
        code.set_vec(&s, self.specular);
        code.set_vec(&ps, self.physical_size);
        code.set_vec(&fo, self.fade_offset);
        code.set_vec(&sd, self.spot_dir);
        code.set_vec(&spot_factors_name(n), self.spot_factors);
    }
}

/// The six planes (`n . p + w >= 0` inside) of the box around a light's
/// sphere: a draw wholly outside any of them cannot be lit by it.
pub fn box_planes(origin: [f32; 3], radius: f32) -> [glam::Vec4; 6] {
    let [x, y, z] = origin;
    [
        glam::Vec4::new(-1.0, 0.0, 0.0, x + radius),
        glam::Vec4::new(1.0, 0.0, 0.0, -(x - radius)),
        glam::Vec4::new(0.0, -1.0, 0.0, y + radius),
        glam::Vec4::new(0.0, 1.0, 0.0, -(y - radius)),
        glam::Vec4::new(0.0, 0.0, -1.0, z + radius),
        glam::Vec4::new(0.0, 0.0, 1.0, -(z - radius)),
    ]
}

/// The most local lights drawn in a frame (each is a pass over the draws
/// inside its radius): from the shared quality's `local_lights`, and
/// `BO2MP_H1_LOCAL_LIGHTS=n` (0: none). INFERRED cap: the game's own is the
/// primary light environment's four per surface, not read as a frame count.
pub fn frame_cap(local_lights_on: bool) -> usize {
    std::env::var("BO2MP_H1_LOCAL_LIGHTS")
        .ok()
        .and_then(|v| v.trim().parse::<usize>().ok())
        .unwrap_or(if local_lights_on { 4 } else { 0 })
        .min(MAX_VARIANTS)
}

/// Constants variants reserved for the light passes (one per light).
pub const MAX_VARIANTS: usize = 16;
/// The first variant a light pass uses (the sun shadow's cascades and the
/// main view take the ones below).
pub const FIRST_VARIANT: u8 = 8;

/// Which lights to draw: those whose sphere reaches the view (`planes`, as
/// `frustum_planes`) and the eye's neighbourhood, nearest first, up to `cap`.
pub fn pick(lights: &[PrimaryLight], eye: Vec3, planes: Option<&[glam::Vec4]>, cap: usize) -> Vec<usize> {
    let mut v: Vec<(f32, usize)> = lights
        .iter()
        .enumerate()
        .filter(|(_, l)| {
            let (lo, hi) = (Vec3::from(l.origin) - Vec3::splat(l.radius), Vec3::from(l.origin) + Vec3::splat(l.radius));
            planes.is_none_or(|p| crate::sun_shadow::box_inside(p, lo, hi))
        })
        .map(|(i, l)| ((Vec3::from(l.origin) - eye).length() - l.radius, i))
        .collect();
    v.sort_by(|a, b| a.0.total_cmp(&b.0));
    v.into_iter().take(cap).map(|(_, i)| i).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spot() -> PrimaryLight {
        PrimaryLight {
            ty: 2,
            color: [2.0, 1.0, 0.5],
            dir: [0.0, 0.0, -1.0],
            origin: [110.0, 20.0, 30.0],
            radius: 200.0,
            cos_half_fov_outer: 0.5,
            cos_half_fov_inner: 0.9,
            exponent: 1,
            bulb_radius: 3.0,
            bulb_length: [4.0, 5.0, 6.0],
            fade_offset: [0.25, 0.75],
            ..PrimaryLight::default()
        }
    }

    #[test]
    fn the_box_planes_keep_a_point_inside_and_drop_one_outside() {
        let pl = box_planes([10.0, 0.0, 0.0], 5.0);
        let inside = |p: glam::Vec3| pl.iter().all(|q| q.truncate().dot(p) + q.w >= 0.0);
        assert!(inside(glam::Vec3::new(12.0, 3.0, -4.0)));
        assert!(!inside(glam::Vec3::new(16.0, 0.0, 0.0)));
        assert!(!inside(glam::Vec3::new(10.0, 0.0, 6.0)));
    }

    #[test]
    fn the_nearest_lights_in_view_are_picked_first() {
        let mk = |x: f32| PrimaryLight { ty: 3, radius: 10.0, origin: [x, 0.0, 0.0], ..PrimaryLight::default() };
        let lights = [mk(100.0), mk(30.0), mk(60.0)];
        assert_eq!(pick(&lights, Vec3::ZERO, None, 2), vec![1, 2]);
        assert!(pick(&lights, Vec3::ZERO, None, 0).is_empty());
    }

    #[test]
    fn a_spot_light_fills_the_documented_values() {
        let s = ColorScales { diffuse: 2.0, specular: 0.5 };
        let l = LocalLight::new(&spot(), Vec3::new(10.0, 20.0, 30.0), s).unwrap();
        assert_eq!(l.position, [100.0, 0.0, 0.0, 1.0 / 200.0]);
        assert_eq!(l.diffuse, [4.0, 2.0, 1.0, 1.0]);
        assert_eq!(l.specular, [1.0, 0.5, 0.25, 1.0]);
        assert_eq!(l.physical_size, [4.0, 5.0, 6.0, 3.0]);
        assert_eq!(l.fade_offset, [0.25, 0.75, 0.0, 0.0]);
        // 1 / 0.4 = 2.5; -0.5 / 0.4 = -1.25.
        assert!((l.spot_factors[0] - 2.5).abs() < 1e-5);
        assert!((l.spot_factors[1] + 1.25).abs() < 1e-5);
        assert_eq!(l.spot_factors[2], 1.0);
        // Inside the cone's edge the factor is 0 at the outer cosine, 1 at the inner.
        let at = |cos: f32| l.spot_factors[0] * cos + l.spot_factors[1];
        assert!(at(0.5).abs() < 1e-5 && (at(0.9) - 1.0).abs() < 1e-5);
    }

    #[test]
    fn suns_empty_slots_and_radiusless_lights_make_nothing() {
        let s = ColorScales { diffuse: 1.0, specular: 1.0 };
        let mut l = spot();
        l.ty = 1;
        assert!(LocalLight::new(&l, Vec3::ZERO, s).is_none());
        l.ty = 3;
        l.radius = 0.0;
        assert!(LocalLight::new(&l, Vec3::ZERO, s).is_none());
        l.radius = 50.0;
        // An omni light has no cone but keeps its position and colour.
        assert!(LocalLight::new(&l, Vec3::ZERO, s).is_some());
    }

    #[test]
    fn writes_into_the_numbered_slot() {
        let s = ColorScales { diffuse: 1.0, specular: 1.0 };
        let l = LocalLight::new(&spot(), Vec3::ZERO, s).unwrap();
        let mut c = CodeValues::default();
        l.write(2, &mut c);
        assert_eq!(c.row(crate::code::code_index("LIGHT_POSITION2"), 0), l.position);
        assert_eq!(c.row(crate::code::code_index("LIGHT_SPOTFACTORS2"), 0), l.spot_factors);
        assert_eq!(names(0)[0], "LIGHT_POSITION");
    }
}
