//! Static model LOD selection and the cull of placements too small on
//! screen, as BlackOps3.exe does them for its views (translated from the
//! exe; addresses in `docs/exe_notes/T7_RENDER_GAPS.md`).
//!
//! Each frame the exe fills a 48-byte LOD context per client: the eye, a
//! kind (0 for the ordinary view), a distance scale (a float dvar, default
//! 1, 1..4, times a scene factor) and bias (default 0, -1000..0), a size
//! factor, a LOD limit (an int dvar, default 0, 0..2: that many of the
//! finest LODs never used), the smallest radius on screen drawn (a float
//! dvar, default 1 pixel), and two pixel scales made from the view's
//! height `h` and the tangents of its half fields of view:
//! `px = sqrt(floor(h * 8 / 9) * (h >> 1) / (tan_x * tan_y))` and
//! `lod = sqrt(px² / 4 + 1 / pi)` (4: a float dvar, 1..100).
//!
//! The model header keeps the LOD count (+0x40), a size (+0x44), a smallest
//! radius on screen (+0x48), a size and a radius factor per kind (+0x4c,
//! +0x58), one distance per LOD (+0x64, largest first: the LOD meshes run
//! from the coarsest at index 0 to the finest last) and the finest LOD a
//! shadow view (+0x84) or the main view (+0x85) may take. A placement
//! (`GfxStaticModelDrawInst`, 0x98 bytes) gives its scale (+0x30), the
//! point distances are taken from (+0x34) and its radius (+0x84).
//!
//! [`pick`] is the exe's selection for one placement: `None` = not drawn.
//! The GPU culling runs the same steps (`crate::cull`); this one is for
//! the CPU counts and the checks.

/// LODs a model can have (the distances' room in the header).
pub const MAX_LODS: usize = 8;

/// The main view's flags (the finest LOD capped at the header's +0x85).
pub const FLAGS_VIEW: u32 = 2;
/// A shadow view's flags (capped at +0x84).
pub const FLAGS_SHADOW: u32 = 4;

/// One model's LOD numbers, read off its `XModel` header.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ModelLods {
    pub count: u8,
    pub size: f32,
    pub min_radius: f32,
    pub size_kind: [f32; 3],
    pub radius_kind: [f32; 3],
    pub dist: [f32; MAX_LODS],
    pub shadow_finest: i8,
    pub view_finest: i8,
}

fn f32_at(b: &[u8], o: usize) -> Option<f32> {
    Some(f32::from_le_bytes(b.get(o..o + 4)?.try_into().ok()?))
}

impl ModelLods {
    /// From the zone image at the model header `h`.
    pub fn read(img: &[u8], h: usize) -> Option<Self> {
        let count = *img.get(h + 0x40)?;
        let f = |o: usize| f32_at(img, h + o);
        let mut dist = [0.0; MAX_LODS];
        for (k, d) in dist.iter_mut().enumerate() {
            *d = f(0x64 + 4 * k)?;
        }
        Some(Self {
            count: count.min(MAX_LODS as u8),
            size: f(0x44)?,
            min_radius: f(0x48)?,
            size_kind: [f(0x4c)?, f(0x50)?, f(0x54)?],
            radius_kind: [f(0x58)?, f(0x5c)?, f(0x60)?],
            dist,
            shadow_finest: *img.get(h + 0x84)? as i8,
            view_finest: *img.get(h + 0x85)? as i8,
        })
    }
}

/// One view's LOD context (the exe's per-client 48 bytes, past the eye).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct View {
    pub kind: usize,
    pub scale: f32,
    pub bias: f32,
    pub size: f32,
    pub limit: i32,
    /// The smallest radius on screen drawn, pixels (negative: no floor).
    pub min_radius: f32,
    pub px: f32,
    pub lod_px: f32,
}

/// The exe's dvar defaults: distance scale 1 (1..4), bias 0, size divisor
/// 4 (1..100), smallest radius 1 pixel (0..10000), LOD limit 0 (0..2).
pub const SCALE_DEFAULT: f32 = 1.0;
pub const MIN_RADIUS_DEFAULT: f32 = 1.0;
pub const SIZE_DIVISOR_DEFAULT: f32 = 4.0;

impl View {
    /// For a view `height` pixels high with half-field tangents `tan_x`,
    /// `tan_y`, the distance `scale` (clamped to the dvar's 1..4) and the
    /// smallest radius drawn (`min_radius`, pixels).
    pub fn new(height: u32, tan_x: f32, tan_y: f32, scale: f32, min_radius: f32) -> Self {
        let h = height.min(0xffff);
        let half_w = ((u64::from(h) * 0x3_8e38_e390) >> 0x22) as u32;
        let area = half_w.wrapping_mul(h >> 1) as f32 * (1.0 / tan_y) * (1.0 / tan_x);
        let px = if area < 0.0 || !area.is_finite() {
            0.0
        } else {
            area.sqrt()
        };
        let lod = area / SIZE_DIVISOR_DEFAULT + std::f32::consts::FRAC_1_PI;
        Self {
            kind: 0,
            scale: scale.clamp(1.0, 4.0),
            bias: 0.0,
            size: 1.0,
            limit: 0,
            min_radius: min_radius.clamp(0.0, 10000.0),
            px,
            lod_px: if lod.is_finite() && lod > 0.0 {
                lod.sqrt()
            } else {
                0.0
            },
        }
    }
}

/// The exe's inner pick: the LOD index (coarsest 0) for distance `d`,
/// `-1` when the placement is too small on screen.
fn inner(v: &View, d: f32, cull: f32, r: f32, sz: f32, count: u8, dist: &[f32; MAX_LODS]) -> i32 {
    let last = i32::from(count) - 1;
    let x = d - r;
    if 0.0 - x >= 0.0 || x <= 0.0 || v.px == 0.0 {
        return last;
    }
    let floor = if v.min_radius < 0.0 {
        0.0
    } else if cull - v.min_radius >= 0.0 {
        cull
    } else {
        v.min_radius
    };
    if floor * d > v.px * r {
        return -1;
    }
    let k2 = v.lod_px * v.lod_px;
    let x2 = x * x;
    let mut i = last;
    while i >= 0 {
        if sz * dist[i as usize] * k2 > x2 {
            break;
        }
        i -= 1;
    }
    i.min(last).max(0)
}

/// The LOD index (0 the coarsest) the exe draws a placement at, `None` when
/// it is not drawn: `r` its radius (+0x84), `dist` the eye's distance to
/// its point (+0x34), `s` its scale (+0x30), `flags` [`FLAGS_VIEW`] /
/// [`FLAGS_SHADOW`] (bit 0: never left out).
pub fn pick(v: &View, m: &ModelLods, r: f32, dist: f32, s: f32, flags: u32) -> Option<usize> {
    if m.count == 0 {
        return None;
    }
    let k = v.kind.min(2);
    let d = dist * v.scale + v.bias;
    let cull = m.radius_kind[k] * m.min_radius;
    let sz = m.size_kind[k] * m.size * (s * v.size);
    let mut i = inner(v, d, cull, r, sz, m.count, &m.dist);
    if i == -1 && d != dist {
        i = inner(v, dist, cull, r, sz, m.count, &m.dist);
    }
    let mut c = i as i8;
    if flags & 1 != 0 && c < 0 {
        c = 0;
    }
    if flags & 2 != 0 && c > m.view_finest {
        c = m.view_finest;
    }
    if flags & 4 != 0 && c > m.shadow_finest {
        c = m.shadow_finest;
    }
    let cap = ((m.count as i8).wrapping_sub(1) as i32 - v.limit).max(0);
    let c = i32::from(c).min(cap);
    usize::try_from(c).ok()
}

/// The LOD drawn for pick `want` when only the LODs in `built` (bit k: LOD
/// k has a mesh here) exist: the nearest finer one, else the nearest
/// coarser (the order the exe falls back in while a LOD streams in).
pub fn built_near(want: usize, built: u32, count: u8) -> Option<usize> {
    let n = usize::from(count).min(MAX_LODS);
    if built & (1 << want) != 0 {
        return Some(want);
    }
    (want + 1..n)
        .find(|&k| built & (1 << k) != 0)
        .or_else(|| (0..want).rev().find(|&k| built & (1 << k) != 0))
}

/// How static models pick their LOD here (`BO2MP_T7_LOD`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    /// The finest LOD at every distance, nothing left out for size (the
    /// renderer before this; ultra and high by default).
    Finest,
    /// The exe's selection and size cull (medium and low by default).
    Exe,
}

/// `BO2MP_T7_LOD=exe|finest`, else what the settings want.
pub fn mode() -> Mode {
    match std::env::var("BO2MP_T7_LOD").map(|v| v.trim().to_ascii_lowercase()) {
        Ok(v) if v == "exe" || v == "1" || v == "on" => Mode::Exe,
        Ok(v) if v == "finest" || v == "0" || v == "off" => Mode::Finest,
        _ => {
            if crate::quality::get().model_lods {
                Mode::Exe
            } else {
                Mode::Finest
            }
        }
    }
}

/// A placement's LOD inputs: (point, radius) and scale.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Placement {
    pub point: [f32; 3],
    pub radius: f32,
    pub scale: f32,
}

impl Placement {
    /// From a `GfxStaticModelDrawInst` at `d` in the zone image.
    pub fn read(img: &[u8], d: usize) -> Self {
        let f = |o: usize| f32_at(img, d + o).unwrap_or(0.0);
        Self {
            point: [f(0x34), f(0x38), f(0x3c)],
            radius: f(0x84),
            scale: f(0x30),
        }
    }

    pub fn distance(&self, eye: [f32; 3]) -> f32 {
        let d = [
            eye[0] - self.point[0],
            eye[1] - self.point[1],
            eye[2] - self.point[2],
        ];
        let s = d[2] * d[2] + (d[0] * d[0] + d[1] * d[1]);
        s.max(0.0).sqrt()
    }
}

/// A cull range's LOD data: the model's numbers, which LODs have a mesh,
/// the finest built, and each placement's inputs (in the range's order).
#[derive(Clone, Debug, Default)]
pub struct RangeLod {
    pub model: ModelLods,
    pub built: u32,
    pub finest: u8,
    pub placements: Vec<Placement>,
}

impl RangeLod {
    /// The LOD drawn for placement `i` seen from `eye` (None: not drawn).
    pub fn drawn(&self, v: &View, eye: [f32; 3], i: usize) -> Option<usize> {
        let p = self.placements.get(i)?;
        let want = pick(
            v,
            &self.model,
            p.radius,
            p.distance(eye),
            p.scale,
            FLAGS_VIEW,
        )?;
        built_near(want, self.built, self.model.count)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cypress() -> ModelLods {
        // p7_foliage_tree_cypress_italian_thin (Nuk3town), as t7_lods
        // prints it: 7 LODs, size 0.176777, no smallest radius.
        ModelLods {
            count: 7,
            size: 0.176777,
            min_radius: 0.0,
            size_kind: [1.0; 3],
            radius_kind: [1.0; 3],
            dist: [
                742.7327, 335.5352, 172.4762, 76.46336, 38.96219, 25.84311, 19.42458, 0.0,
            ],
            shadow_finest: 6,
            view_finest: 6,
        }
    }

    fn view() -> View {
        // 1920x1080, BO3's 65 degree vertical field.
        let ty = (65f32.to_radians() * 0.5).tan();
        View::new(1080, ty * 16.0 / 9.0, ty, 1.0, 1.0)
    }

    #[test]
    fn the_pixel_scales_follow_the_exe() {
        let v = view();
        let area = (960u32 * 540) as f32 / v_t();
        assert!((v.px - area.sqrt()).abs() < 1e-2);
        assert!(
            (v.lod_px * v.lod_px - (area / 4.0 + std::f32::consts::FRAC_1_PI)).abs()
                / v.lod_px.powi(2)
                < 1e-5
        );
    }

    fn v_t() -> f32 {
        let ty = (65f32.to_radians() * 0.5).tan();
        ty * 16.0 / 9.0 * ty
    }

    #[test]
    fn near_is_finest_far_is_coarser_tiny_is_left_out() {
        let (v, m) = (view(), cypress());
        assert_eq!(
            pick(&v, &m, 120.0, 50.0, 1.0, FLAGS_VIEW),
            Some(6),
            "inside its sphere"
        );
        assert_eq!(pick(&v, &m, 120.0, 300.0, 1.0, FLAGS_VIEW), Some(6));
        let mut last = 6;
        for d in (500..40000).step_by(250) {
            match pick(&v, &m, 120.0, d as f32, 1.0, FLAGS_VIEW) {
                Some(k) => {
                    assert!(
                        k <= last,
                        "LODs only get coarser with distance ({d}: {k} after {last})"
                    );
                    last = k;
                }
                None => {
                    // Left out once its radius covers less than a pixel.
                    assert!(v.px * 120.0 < d as f32 * 1.0 + 1.0);
                    last = 0;
                }
            }
        }
        assert_eq!(last, 0);
        assert_eq!(pick(&v, &m, 120.0, 1.0e6, 1.0, FLAGS_VIEW), None);
        assert_eq!(
            pick(&v, &m, 120.0, 1.0e6, 1.0, FLAGS_VIEW | 1),
            Some(0),
            "flag 1: never left out"
        );
    }

    #[test]
    fn caps_limit_scale_and_built_fallback() {
        let (v, mut m) = (view(), cypress());
        m.view_finest = 4;
        assert_eq!(pick(&v, &m, 120.0, 50.0, 1.0, FLAGS_VIEW), Some(4));
        assert_eq!(pick(&v, &m, 120.0, 50.0, 1.0, FLAGS_SHADOW), Some(6));
        let lim = View { limit: 2, ..v };
        assert_eq!(
            pick(&lim, &cypress(), 120.0, 50.0, 1.0, FLAGS_VIEW),
            Some(4)
        );
        // A larger scale reaches a coarser LOD sooner.
        let far = View { scale: 4.0, ..v };
        let a = pick(&v, &cypress(), 120.0, 3000.0, 1.0, FLAGS_VIEW).unwrap();
        let b = pick(&far, &cypress(), 120.0, 3000.0, 1.0, FLAGS_VIEW).unwrap();
        assert!(b < a, "{b} < {a}");
        assert_eq!(built_near(3, 0b1000000, 7), Some(6), "finer first");
        assert_eq!(built_near(3, 0b0000011, 7), Some(1), "then coarser");
        assert_eq!(built_near(3, 0, 7), None);
    }

    #[test]
    fn reads_the_header() {
        let mut img = vec![0u8; 0x100];
        img[0x40] = 7;
        for (k, d) in cypress().dist.iter().enumerate() {
            img[0x64 + 4 * k..0x68 + 4 * k].copy_from_slice(&d.to_le_bytes());
        }
        img[0x44..0x48].copy_from_slice(&0.176777f32.to_le_bytes());
        for o in [0x4c, 0x50, 0x54, 0x58, 0x5c, 0x60] {
            img[o..o + 4].copy_from_slice(&1f32.to_le_bytes());
        }
        img[0x84] = 6;
        img[0x85] = 6;
        assert_eq!(ModelLods::read(&img, 0), Some(cypress()));
    }
}
