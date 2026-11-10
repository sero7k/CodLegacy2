//! MWR's sun shadow map: the cascades, their projections and the constants
//! the lit (`LIT_DIR_SHADOW`, `_sm_ld1`) shaders read, as the game's
//! executable and shaders give them (`docs/H1_NATIVE.md`, "Sun shadows").
//! The pass itself (the depth atlas drawn with each caster's
//! `build_shadowmap*` technique) is in `host.rs`.
//!
//! Read (exe): the sun axes, the fitting of the view frustum into the
//! first cascade (the eye's texel and the cascade's view depth), the
//! cascades' texel sizes (x4 each), their snapped centres, each cascade's
//! projection, the depth range (the world's shadow bounds along the sun,
//! the sun side 1: a reversed depth), the lookup matrix and its view-depth
//! column, the switch partitions, scale, UV offset and distance blend, the
//! pixel adjust, V clamp and near/far constants, the casters' depth bias
//! (constant -128, slope -4) and the comparison sampler ("greater", border
//! 0); the map size per shadow quality and the light set's defaults where
//! a key is missing (`Quality`, `Settings`); the per-cascade caster
//! culling planes (`caster_planes`); the "sun-lit cell visible" gate
//! (`view_has_sun_shadow`); the cached cascades' world-anchored tiles
//! (`cache_tiles`, `TileCache`).

use glam::{Mat4, Vec2, Vec3, Vec4};

use crate::code::CodeValues;

/// `SHADOWMAP_DISTANCE_BIAS` per used cascade (the engine's immediate).
pub const CASCADE_DISTANCE_BIAS: f32 = 8.0;
/// The engine's shadow map resolution scale is clamped to at least this
/// (the light set's `sunshadowscale`).
pub const MIN_SHADOW_SCALE: f32 = 0.25;
/// Each cascade's extent over the first's (the engine's table 1, 4, 16,
/// 64): its texel size and view depth both grow by 4 a cascade.
pub const CASCADE_STEP: [f32; 4] = [1.0, 4.0, 16.0, 64.0];
/// The cascades the lit shaders test (the switch partitions name two
/// switches, the distance bias three cascades).
pub const MAX_CASCADES: u32 = 3;
/// Rows of the stacked map: 3 (4 with the cached cascades, which we do not
/// draw).
pub const ROWS: u32 = 3;
/// The casters' rasteriser depth bias (polygon offset 3, the shadow map
/// mode): the engine negates its two shadow dvars, whose defaults are 128
/// and 4, and their clamp (default 0). Negative pushes a caster away from
/// the sun in the reversed shadow depth.
pub const CASTER_DEPTH_BIAS: i32 = -128;
pub const CASTER_SLOPE_BIAS: f32 = -4.0;
pub const CASTER_BIAS_CLAMP: f32 = 0.0;
/// Below this |sun z| the sun axes are built around world up, else
/// around world x.
pub const AXIS_UP_LIMIT: f32 = 0.9;

/// The cascade source block the engine fills before copying it into the
/// view's constants (`SHADOWMAP_*`).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct CascadeSource {
    /// One vec4 per cascade switch: the first cascade's UV to cascade
    /// `i + 1`'s, `uv * w + xy`; `z` a "not cached" flag.
    pub switch_partition: [[f32; 4]; 2],
    pub scale: [f32; 4],
    pub uv_offset: [f32; 4],
    pub distance_blend: [f32; 4],
    /// The cascades used.
    pub cascades: i32,
}

/// The seven `SHADOWMAP_*` constants (code constants 196..202) as the
/// engine copies them into the view: the switch partitions, scale, UV
/// offset and blend as they are, the distance bias 8 for each used
/// cascade of the first three (its `.w` is never written; 0 here), and the
/// cascade mask `(1, 1, n >= 3, n >= 4)`.
pub fn block_constants(src: &CascadeSource) -> [(&'static str, [f32; 4]); 7] {
    let n = src.cascades;
    let on = |k: i32| if n >= k { 1.0 } else { 0.0 };
    [
        (
            "SHADOWMAP_SWITCH_PARTITION_ARRAY_0",
            src.switch_partition[0],
        ),
        (
            "SHADOWMAP_SWITCH_PARTITION_ARRAY_1",
            src.switch_partition[1],
        ),
        (
            "SHADOWMAP_DISTANCE_BIAS",
            [
                CASCADE_DISTANCE_BIAS * on(1),
                CASCADE_DISTANCE_BIAS * on(2),
                CASCADE_DISTANCE_BIAS * on(3),
                0.0,
            ],
        ),
        ("SHADOWMAP_SCALE", src.scale),
        ("SHADOWMAP_PARTITION_UV_OFFSET", src.uv_offset),
        ("SHADOWMAP_CASCADE_MASK", [1.0, 1.0, on(3), on(4)]),
        ("SHADOWMAP_DISTANCE_BLEND", src.distance_blend),
    ]
}

/// `SHADOWMAP_SCALE` and `SHADOWMAP_PARTITION_UV_OFFSET`, `f` the shadow
/// resolution scale and `rows` the cascades stacked in the map:
/// `(16 / f, 16 rows / f, -8 / f, -24 / f)` and `(0, 2 / rows, 0, 0)`.
pub fn scale_and_uv_offset(f: f32, rows: u32) -> ([f32; 4], [f32; 4]) {
    let r = rows as f32;
    (
        [16.0 / f, 16.0 * r / f, -8.0 / f, -24.0 / f],
        [0.0, 2.0 / r, 0.0, 0.0],
    )
}

/// The engine's shadow resolution scale from the light set's
/// `sunshadowscale` (a "full scale" override dvar, off by default, is not
/// modelled): clamped to [0.25, 1].
pub fn resolution_scale(sun_shadow_scale: f32) -> f32 {
    sun_shadow_scale.clamp(MIN_SHADOW_SCALE, 1.0)
}

/// The used size of a band: `2 ceil(res f / 2)`.
pub fn used_size(res: u32, f: f32) -> u32 {
    2 * ((res as f32 * f) / 2.0).ceil() as u32
}

/// The border left of (and above) the used part of a band:
/// `(res - used) / 2`, in whole texels.
pub fn border(res: u32, used: u32) -> u32 {
    res.saturating_sub(used) / 2
}

/// The shadow lookup matrix: the first cascade's view projection `m` (one
/// row per input component x, y, z, w) mapped to the map's texture space,
/// u = (x + w) / 2, v = (w - y) / (2 rows), z kept, and the last column
/// `plane`, the view-depth plane (the view's forward axis over the first
/// cascade's view depth).
pub fn lookup_matrix(m: [[f32; 4]; 4], rows: u32, plane: [f32; 4]) -> [[f32; 4]; 4] {
    let a = -0.5 / rows as f32;
    let b = 0.5 / rows as f32;
    std::array::from_fn(|r| {
        [
            0.5 * m[r][0] + 0.5 * m[r][3],
            a * m[r][1] + b * m[r][3],
            m[r][2],
            plane[r],
        ]
    })
}

/// A cascade's centre snapped to its texel size `t` so the map does not
/// crawl as the eye moves: `(floor(x / t) + 0.5) t` (the engine's case
/// with its fixed-centre dvars at their defaults).
pub fn snap_to_texel(x: f32, t: f32) -> f32 {
    ((x / t).floor() + 0.5) * t
}

/// The shadow axes for a sun direction (toward the sun): `x = up × s`,
/// `y = s × x`, with `up` world z unless the sun is within about 25
/// degrees of vertical (|s.z| > 0.9), then world x. The view looks down
/// `-s` (away from the sun), so `x × y = s`.
pub fn axes(to_sun: Vec3) -> (Vec3, Vec3, Vec3) {
    let s = to_sun.normalize_or(Vec3::Z);
    let up = if s.z.abs() <= AXIS_UP_LIMIT {
        Vec3::Z
    } else {
        Vec3::X
    };
    let x = up.cross(s).normalize();
    let y = s.cross(x);
    (x, y, s)
}

/// The depth range of the shadow map: the box `(lo, hi)` projected on the
/// direction away from the sun, `(near, far)` = (min, max) of `-s · p`
/// (`SUN_SHADOWMAP_NEAR_FAR_PLANE.xy`).
pub fn depth_range(to_sun: Vec3, lo: Vec3, hi: Vec3) -> (f32, f32) {
    let n = -to_sun;
    let c = (lo + hi) * 0.5;
    let h = (hi - lo) * 0.5;
    let mid = n.dot(c);
    let ext = n.abs().dot(h.abs());
    (mid - ext, mid + ext)
}

/// The four rays through the view's corners (unit length), from its
/// forward, right and up axes and the tangents of its half fields of view.
pub fn corner_rays(forward: Vec3, right: Vec3, up: Vec3, tan_x: f32, tan_y: f32) -> [Vec3; 4] {
    [(-1.0, 1.0), (1.0, 1.0), (1.0, -1.0), (-1.0, -1.0)]
        .map(|(a, b)| (forward + right * (a * tan_x) + up * (b * tan_y)).normalize())
}

/// Fits the view frustum into the first cascade along one shadow axis.
/// `lo`, `hi`: the corner rays' extent along the axis (the eye, 0,
/// included); `used`: the band's used size in texels. Returns the eye's
/// texel position in the used part and the texels per unit of ray length
/// (`(U - 2) / (hi - lo)`, re-fitted so that the eye lands on a texel
/// centre with the rays inside `[0.5, U - 0.5]`).
pub fn fit_axis(lo: f32, hi: f32, used: u32) -> (f32, f32) {
    let u = used as f32;
    let span = (hi - lo).max(1e-6);
    let mut k = (u - 2.0) / span;
    let mut at = (0.5 - k * lo).ceil() + 0.5;
    let end = (u - 0.5 - k * hi).floor();
    if at != end - 0.5 {
        if hi <= -lo {
            // The rays reach further on the low side: keep the eye's texel
            // one lower and scale to reach texel 1 at `lo`.
            at -= 1.0;
            k = (1.0 - at) / lo.min(-1e-6);
        } else {
            at = end - 0.5 + 1.0;
            k = (u - 1.0 - at) / hi.max(1e-6);
        }
    }
    (at, k)
}

/// The view a frame's cascades are fitted to.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct View {
    pub eye: Vec3,
    pub forward: Vec3,
    pub right: Vec3,
    pub up: Vec3,
    pub tan_x: f32,
    pub tan_y: f32,
    /// The near plane's view depth (the first cascade's near culling
    /// plane when the view looks into the sun).
    pub near: f32,
}

impl View {
    /// From a camera (its rotation's rows: right, up, forward).
    pub fn from_camera(c: &crate::code::Camera) -> Self {
        let r = c.rotation;
        Self {
            eye: c.eye,
            right: r.row(0).truncate().normalize_or(Vec3::X),
            up: r.row(1).truncate().normalize_or(Vec3::Z),
            forward: r.row(2).truncate().normalize_or(Vec3::Y),
            tan_x: c.tan_x,
            tan_y: c.tan_y,
            near: c.near,
        }
    }
}

/// The frustum fitted into the first cascade: the eye's texel in the used
/// part of each band (x, y), the first cascade's view depth (the split
/// distance; cascade `k` reaches `CASCADE_STEP[k]` times it).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Fit {
    pub eye_texel: [f32; 2],
    pub depth: f32,
}

/// Fits the view's frustum to the first cascade: the corner rays'
/// extents along the shadow axes (the eye included), [`fit_axis`] on each,
/// the ray length the smaller scale fits (`min(k) t0`) and the view depth
/// at that length along the widest corner ray.
pub fn fit(view: &View, x: Vec3, y: Vec3, used: u32, t0: f32) -> Fit {
    let rays = corner_rays(view.forward, view.right, view.up, view.tan_x, view.tan_y);
    let (mut lo, mut hi) = ([0.0f32; 2], [0.0f32; 2]);
    let mut min_cos = 1.0f32;
    for r in rays {
        for (j, a) in [x, y].into_iter().enumerate() {
            let p = r.dot(a);
            lo[j] = lo[j].min(p);
            hi[j] = hi[j].max(p);
        }
        min_cos = min_cos.min(view.forward.dot(r));
    }
    let (ax, kx) = fit_axis(lo[0], hi[0], used);
    let (ay, ky) = fit_axis(lo[1], hi[1], used);
    Fit {
        eye_texel: [ax, ay],
        depth: min_cos * kx.min(ky) * t0,
    }
}

/// The shadow quality setting (an enum dvar, default `Auto`) that sizes
/// the shadow maps when the renderer starts (read).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Quality {
    #[default]
    Auto,
    Normal,
    High,
    Extra,
}

impl Quality {
    /// `BO2MP_H1_SUN_SHADOW_QUALITY` = `auto` (default), `normal`, `high`
    /// or `extra`.
    pub fn from_env() -> Self {
        match std::env::var("BO2MP_H1_SUN_SHADOW_QUALITY")
            .unwrap_or_else(|_| String::new())
            .to_ascii_lowercase()
            .as_str()
        {
            "normal" => Self::Normal,
            "high" => Self::High,
            "extra" => Self::Extra,
            _ => Self::Auto,
        }
    }

    /// The sun shadow map's width and the small map's (read): `Auto` and
    /// `Normal` 1024 and 512, `High` 2048 and 1024, `Extra` 3072 and 1536.
    /// The sun map is that wide and 3 bands high, the small one 4 bands.
    pub fn map_sizes(self) -> (u32, u32) {
        match self {
            Self::Auto | Self::Normal => (1024, 512),
            Self::High => (2048, 1024),
            Self::Extra => (3072, 1536),
        }
    }
}

/// The cached sun shadow setting (an enum dvar: "Disabled", "Enabled",
/// "Enabled Force Cache"; default "Disabled"; read).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum CacheMode {
    #[default]
    Disabled,
    Enabled,
    /// Every tile rebuilt each frame.
    Force,
}

impl CacheMode {
    /// `BO2MP_H1_SUN_SHADOW_CACHE` = `0` (default), `1` or `force`.
    pub fn from_env() -> Self {
        match std::env::var("BO2MP_H1_SUN_SHADOW_CACHE")
            .unwrap_or_else(|_| String::new())
            .to_ascii_lowercase()
            .as_str()
        {
            "1" | "on" | "enabled" => Self::Enabled,
            "2" | "force" => Self::Force,
            _ => Self::Disabled,
        }
    }
}

/// What the pass is set up with. The light set keys come from the map's
/// light set; a key the map's light set lacks keeps the engine's row
/// default (read: the light set rows start from their override dvars'
/// defaults). The retail zones here carry none of these keys.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Settings {
    /// The map's size per band (read: the shadow quality, 1024 at its
    /// default `Auto`).
    pub res: u32,
    /// `sunshadowscale` (default 1, range 0.25..1).
    pub scale: f32,
    /// `usedsuncascadecount`, 1..=3 (default 2).
    pub cascades: u32,
    /// `sunsamplesizenear`: the first cascade's world units per texel at
    /// full scale (default 0.25, range 1/64..32).
    pub sample_size: f32,
    /// `sunfilterradius`: the PCF radius in texels (default 4, range
    /// 0..64).
    pub filter_radius: f32,
    /// The cached cascades (default off).
    pub cache: CacheMode,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            res: Quality::Auto.map_sizes().0,
            scale: 1.0,
            cascades: 2,
            sample_size: 0.25,
            filter_radius: 4.0,
            cache: CacheMode::Disabled,
        }
    }
}

impl Settings {
    /// From the map's light set (keys present override the defaults),
    /// `BO2MP_H1_SUN_SHADOW_QUALITY` (map size), `BO2MP_H1_SUN_SHADOW_RES`
    /// (map size, overriding the quality) and `BO2MP_H1_SUN_SHADOW_CACHE`.
    pub fn from_lightset(ls: &crate::look::Settings) -> Self {
        let d = Self::default();
        let res = std::env::var("BO2MP_H1_SUN_SHADOW_RES")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(Quality::from_env().map_sizes().0)
            .clamp(256, 8192);
        Self {
            res,
            scale: ls.f("sunshadowscale", d.scale),
            cascades: (ls.f("usedsuncascadecount", d.cascades as f32) as u32)
                .clamp(1, MAX_CASCADES),
            sample_size: ls.f("sunsamplesizenear", d.sample_size).max(1e-3),
            filter_radius: ls.f("sunfilterradius", d.filter_radius).max(0.0),
            cache: CacheMode::from_env(),
        }
    }

    /// Whether the pass is on: on by default as in the game (31 Oct, checked
    /// on Shipment on the GPU); `BO2MP_H1_SUN_SHADOW=0` keeps the baked
    /// `LIT_DIR` look.
    pub fn enabled_from_env() -> bool {
        std::env::var("BO2MP_H1_SUN_SHADOW").map_or(true, |v| v != "0")
    }

    /// The resolution scale `f`.
    pub fn f(&self) -> f32 {
        resolution_scale(self.scale)
    }

    /// The used size of a band.
    pub fn used(&self) -> u32 {
        used_size(self.res, self.f()).min(self.res)
    }

    /// The border left of the used part of a band, in texels.
    pub fn border_texels(&self) -> u32 {
        border(self.res, self.used())
    }

    /// Cascade `k`'s texel size: the sample size over `f`, x4 a cascade.
    pub fn texel(&self, k: usize) -> f32 {
        self.sample_size / self.f() * CASCADE_STEP[k]
    }
}

/// One frame's cascades.
#[derive(Clone, Debug)]
pub struct Setup {
    pub settings: Settings,
    /// The shadow axes: x, y across the map, `s` toward the sun.
    pub x: Vec3,
    pub y: Vec3,
    pub s: Vec3,
    /// The frustum's fit into the first cascade.
    pub fit: Fit,
    /// Each cascade's snapped centre (along x, y) and texel size.
    pub centre: Vec<[f32; 2]>,
    pub texel: Vec<f32>,
    /// The depth range: min and max of `-s · p` over the shadow bounds.
    pub near_far: (f32, f32),
    pub view: View,
}

impl Setup {
    /// The cascades for `view`, the sun toward `to_sun`, the depth over
    /// the world's shadow bounds `bounds`.
    pub fn new(settings: Settings, to_sun: Vec3, view: View, bounds: (Vec3, Vec3)) -> Self {
        let (x, y, s) = axes(to_sun);
        let used = settings.used();
        let fit = fit(&view, x, y, used, settings.texel(0));
        let e = [view.eye.dot(x), view.eye.dot(y)];
        let texel: Vec<f32> = (0..settings.cascades as usize)
            .map(|k| settings.texel(k))
            .collect();
        let centre = texel
            .iter()
            .map(|&t| [snap_to_texel(e[0], t), snap_to_texel(e[1], t)])
            .collect();
        let (n, f) = depth_range(s, bounds.0, bounds.1);
        Self {
            settings,
            x,
            y,
            s,
            fit,
            centre,
            texel,
            near_far: (n, f.max(n + 1.0)),
            view,
        }
    }

    /// The cascades drawn.
    pub fn cascades(&self) -> usize {
        self.texel.len()
    }

    /// The eye's texel in the full band: the border plus its texel in the
    /// used part.
    fn eye_texel(&self) -> [f32; 2] {
        let b = border(self.settings.res, self.settings.used()) as f32;
        [b + self.fit.eye_texel[0], b + self.fit.eye_texel[1]]
    }

    /// Cascade `k`'s projection of world positions: clip x, y over the
    /// band (`2 (a · p - c) / (res t) + 2 e / res - 1`, `e` the eye's
    /// texel), z the reversed depth `(s · p + far) / (far - near)` (1 at the
    /// sun side of the shadow bounds), w 1.
    pub fn world_projection(&self, k: usize) -> Mat4 {
        let res = self.settings.res as f32;
        let sc = 2.0 / (res * self.texel[k]);
        let e = self.eye_texel();
        let [cx, cy] = self.centre[k];
        let (n, f) = self.near_far;
        let row = |a: Vec3, c: f32, e: f32| {
            Vec4::new(a.x * sc, a.y * sc, a.z * sc, -c * sc + 2.0 * e / res - 1.0)
        };
        let z = self.s / (f - n);
        Mat4::from_cols_array_2d(&[
            row(self.x, cx, e[0]).to_array(),
            row(self.y, cy, e[1]).to_array(),
            [z.x, z.y, z.z, f / (f - n)],
            [0.0, 0.0, 0.0, 1.0],
        ])
        .transpose()
    }

    /// Cascade `k`'s projection of eye-relative positions (as the caster
    /// shaders take them: world minus `WORLD_MATRIX_EYE_OFFSET`).
    pub fn view_projection(&self, k: usize) -> Mat4 {
        self.world_projection(k) * Mat4::from_translation(self.view.eye)
    }

    /// The view-depth plane for eye-relative positions: the forward axis
    /// over the first cascade's view depth (the lookup's last column).
    pub fn depth_plane(&self) -> Vec4 {
        (self.view.forward / self.fit.depth).extend(0.0)
    }

    /// The lookup matrix (`TRANSPOSE_SHADOW_LOOKUP_MATRIX`, column-vector
    /// form): eye-relative position to (u, v, depth, view depth over the
    /// first cascade's).
    pub fn lookup(&self) -> Mat4 {
        let m = self.view_projection(0).to_cols_array_2d();
        Mat4::from_cols_array_2d(&lookup_matrix(m, ROWS, self.depth_plane().to_array()))
    }

    /// Switch partition `k`: the first cascade's UV to cascade `k + 1`'s,
    /// `uv * w + xy`, `w = 1 / 4^(k + 1)`, `z` 1 ("not cached": we draw no
    /// cached cascades).
    pub fn switch_partition(&self, k: usize) -> [f32; 4] {
        let Some(&t) = self.texel.get(k + 1) else {
            return [0.0; 4];
        };
        let res = self.settings.res as f32;
        let r = 1.0 / CASCADE_STEP[k + 1];
        let e = self.eye_texel();
        let d = [
            self.centre[0][0] - self.centre[k + 1][0],
            self.centre[0][1] - self.centre[k + 1][1],
        ];
        let u = ((1.0 - r) * e[0] + d[0] / t) / res;
        let v = ((k as f32 + 2.0 - r) - ((1.0 - r) * e[1] + d[1] / t) / res) / ROWS as f32;
        [u, v, 1.0, r]
    }

    /// The cascade source block.
    pub fn source(&self) -> CascadeSource {
        let (scale, uv_offset) = scale_and_uv_offset(self.settings.f(), ROWS);
        let n = self.cascades();
        CascadeSource {
            switch_partition: [self.switch_partition(0), self.switch_partition(1)],
            scale,
            uv_offset,
            distance_blend: distance_blend(self.fit.depth, n, self.settings.res, self.texel[n - 1]),
            cascades: n as i32,
        }
    }

    /// Writes every sun shadow constant the lit shaders read.
    pub fn apply(&self, code: &mut CodeValues) {
        code.set_matrix("TRANSPOSE_SHADOW_LOOKUP_MATRIX", self.lookup());
        for (n, v) in block_constants(&self.source()) {
            code.set_vec(n, v);
        }
        let p = pixel_adjust(self.settings.res, self.settings.filter_radius);
        for (k, n) in [
            "SUN_SHADOWMAP_PIXEL_ADJUST_ARRAY_0",
            "SUN_SHADOWMAP_PIXEL_ADJUST_ARRAY_1",
            "SUN_SHADOWMAP_PIXEL_ADJUST_ARRAY_2",
        ]
        .into_iter()
        .enumerate()
        {
            code.set_vec(n, p[k]);
        }
        code.set_vec("SUN_SHADOWMAP_CASCADE_V_CLAMP", v_clamp(&p));
        code.set_vec(
            "SUN_SHADOWMAP_NEAR_FAR_PLANE",
            near_far_plane(self.near_far),
        );
    }
}

/// `SHADOWMAP_DISTANCE_BLEND`: `((last step x first depth)^2, 1 / (res x
/// last texel / 16)^2, 0, 0)`: the squared view depth the last cascade
/// reaches and one over a squared fade width.
pub fn distance_blend(depth: f32, cascades: usize, res: u32, last_texel: f32) -> [f32; 4] {
    let far = CASCADE_STEP[cascades - 1] * depth;
    let w = res as f32 * last_texel * 0.0625;
    [far * far, 1.0 / (w * w), 0.0, 0.0]
}

/// `SUN_SHADOWMAP_PIXEL_ADJUST_ARRAY_0..2`: the filter radius in the
/// stacked map's UV, `(r / res, r / (rows res), 0, 0)` for the first
/// cascade, scaled by its texel size over each cascade's (1/4, 1/16).
pub fn pixel_adjust(res: u32, radius: f32) -> [[f32; 4]; 3] {
    let u = radius / res as f32;
    let v = radius / (ROWS * res) as f32;
    std::array::from_fn(|k| {
        let r = CASCADE_STEP[0] / CASCADE_STEP[k];
        [u * r, v * r, 0.0, 0.0]
    })
}

/// `SUN_SHADOWMAP_CASCADE_V_CLAMP`: the band edges less the filter:
/// `(1 / rows - p0.v, 1 / rows + p1.v, 2 / rows - p1.v, 0)` (cascade 0's
/// V clamped below `.x`, cascade 1's into [`.y`, `.z`]).
pub fn v_clamp(p: &[[f32; 4]; 3]) -> [f32; 4] {
    let r = ROWS as f32;
    [1.0 / r - p[0][1], 1.0 / r + p[1][1], 2.0 / r - p[1][1], 0.0]
}

/// `SUN_SHADOWMAP_NEAR_FAR_PLANE`: `(near, far, far - near, 1 / (far -
/// near))` of the depth range (world units along the sun).
pub fn near_far_plane((n, f): (f32, f32)) -> [f32; 4] {
    [n, f, f - n, 1.0 / (f - n)]
}

/// Where cascade `k`'s band is drawn: (x, y, width, height) in the map.
pub fn viewport(res: u32, k: usize) -> [f32; 4] {
    [0.0, (res as usize * k) as f32, res as f32, res as f32]
}

/// The box the shadow map's depth range is fitted to: the world's shadow
/// bounds (`GfxWorld::shadowBounds`; a degenerate box falls back to
/// +-16384 units, which holds every retail MWR map).
pub fn caster_bounds(scene: &crate::scene::Scene) -> (Vec3, Vec3) {
    let (lo, hi) = scene.shadow_bounds;
    if (hi - lo).min_element() > 0.0 {
        (lo, hi)
    } else {
        (Vec3::splat(-16384.0), Vec3::splat(16384.0))
    }
}

/// The engine's tolerance in the caster culling (read): a projected face
/// or ray within it of zero counts as zero.
pub const CULL_EPSILON: f32 = 0.001;

/// A caster culling plane (world space): a caster is kept when some point
/// of it has `xyz · p + w >= 0`.
pub type Plane = Vec4;

/// The view frustum's four side faces as the engine pairs the corner rays
/// (`corner_rays` order): (3, 0), (0, 1), (1, 2), (2, 3).
pub const FACES: [(usize, usize); 4] = [(3, 0), (0, 1), (1, 2), (2, 3)];

/// How each side face of the view meets the sun: `(r_a x r_b) · s`
/// oriented so that positive means the face's inward normal leans toward
/// the sun (light leaves the frustum through it). The engine computes the
/// same as the 2D cross of the two rays projected on the shadow axes, of
/// the opposite sign; a face is "toward the sun" past `CULL_EPSILON`.
pub fn face_sun(view: &View, s: Vec3) -> [f32; 4] {
    let r = corner_rays(view.forward, view.right, view.up, view.tan_x, view.tan_y);
    FACES.map(|(a, b)| {
        let n = r[a].cross(r[b]);
        let inward = if n.dot(view.forward) < 0.0 { -n } else { n };
        inward.dot(s)
    })
}

/// The view's side planes (through the eye, unit inward normals) whose
/// face leans toward the sun: a caster outside such a face casts away from
/// the view (read: added to every cascade).
pub fn side_planes(view: &View, s: Vec3) -> Vec<Plane> {
    let r = corner_rays(view.forward, view.right, view.up, view.tan_x, view.tan_y);
    let f = face_sun(view, s);
    FACES
        .iter()
        .zip(f)
        .filter(|&(_, c)| c > CULL_EPSILON)
        .map(|(&(a, b), _)| {
            let mut n = r[a].cross(r[b]).normalize();
            if n.dot(view.forward) < 0.0 {
                n = -n;
            }
            n.extend(-n.dot(view.eye))
        })
        .collect()
}

/// The view wedge's silhouette as seen from the sun: the two corner rays
/// where the faces on either side switch between leaning toward and away
/// from the sun (read). Each comes back as its unit direction in the
/// shadow plane `(x · r, y · r)` with the unit normal (in the same plane)
/// that points into the wedge. None when the wedge has no such pair (the
/// sun within the view's cone or behind it).
pub fn silhouette(view: &View, x: Vec3, y: Vec3, s: Vec3) -> Option<[(Vec2, Vec2); 2]> {
    let r = corner_rays(view.forward, view.right, view.up, view.tan_x, view.tan_y);
    let f = face_sun(view, s);
    let (lo, hi) = f
        .iter()
        .fold((f32::MAX, f32::MIN), |(a, b), &c| (a.min(c), b.max(c)));
    if hi.min(-lo) < CULL_EPSILON {
        return None;
    }
    let p: [Vec2; 4] = r.map(|r| Vec2::new(r.dot(x), r.dot(y)));
    // Ray i sits between face (i - 1, i) (index i in `FACES`) and face
    // (i, i + 1) (index i + 1).
    let mut found = Vec::new();
    for i in 0..4 {
        let before = f[i] > CULL_EPSILON;
        let after = f[(i + 1) % 4] > CULL_EPSILON;
        if before != after {
            found.push(i);
        }
    }
    if found.len() != 2 {
        return None;
    }
    let inside: Vec2 = p.iter().map(|q| q.normalize_or_zero()).sum();
    Some([found[0], found[1]].map(|i| {
        let d = p[i].normalize_or(Vec2::X);
        let mut n = d.perp();
        if n.dot(inside) < 0.0 {
            n = -n;
        }
        (d, n)
    }))
}

/// Whether a box (min, max) is kept by every plane.
pub fn box_inside(planes: &[Plane], lo: Vec3, hi: Vec3) -> bool {
    let c = (lo + hi) * 0.5;
    let h = (hi - lo) * 0.5;
    planes
        .iter()
        .all(|p| p.truncate().dot(c) + p.truncate().abs().dot(h) + p.w >= 0.0)
}

impl Setup {
    /// Cascade `k`'s band (its used part) as four planes along the shadow
    /// axes (read): `x · p` in `[c - e t, c + (U - e) t]`, `e` the eye's
    /// texel in the used part, `U` the used size, the same along `y`.
    pub fn band_planes(&self, k: usize) -> [Plane; 4] {
        let t = self.texel[k];
        let u = self.settings.used() as f32;
        let mut out = [Vec4::ZERO; 4];
        for (j, a) in [self.x, self.y].into_iter().enumerate() {
            let lo = self.centre[k][j] - self.fit.eye_texel[j] * t;
            out[2 * j] = a.extend(-lo);
            out[2 * j + 1] = (-a).extend(lo + u * t);
        }
        out
    }

    /// Cascade `k`'s caster culling planes, as the engine builds them each
    /// frame (read): its band's four sides; the two planes along the sun
    /// through the view wedge's silhouette rays, pushed out by `(1 +
    /// 4^k) / 2` (cascade 0: 1/2) of the first cascade's texel times
    /// `|d.x| + |d.y|` of the ray's direction; with no silhouette and every
    /// face toward the sun (looking into the sun), the near plane at the
    /// previous cascade's view depth (cascade 0: the view's near plane);
    /// and the view's side faces that lean toward the sun.
    pub fn caster_planes(&self, k: usize) -> Vec<Plane> {
        let mut out: Vec<Plane> = Vec::new();
        let view = &self.view;
        match silhouette(view, self.x, self.y, self.s) {
            Some(sil) => {
                let grow = if k == 0 { 1.0 } else { 1.0 + CASCADE_STEP[k] };
                for (d, n2) in sil {
                    let n = self.x * n2.x + self.y * n2.y;
                    let pad = (d.x.abs() + d.y.abs()) * self.texel[0] * 0.5 * grow;
                    out.push(n.extend(pad - n.dot(view.eye)));
                }
            }
            None => {
                if face_sun(view, self.s).iter().all(|&c| c > CULL_EPSILON) {
                    let near = if k == 0 {
                        view.near
                    } else {
                        CASCADE_STEP[k - 1] * self.fit.depth
                    };
                    out.push(view.forward.extend(-view.forward.dot(view.eye) - near));
                }
            }
        }
        out.extend(self.band_planes(k));
        out.extend(side_planes(view, self.s));
        out
    }
}

/// When a view draws the sun shadow (the view's flag, read): `Auto` (0)
/// when one of its visible cells has sun-lit surfaces, `Force` (1) always
/// (kept only while the sun is on), `Off` (2) never. Otherwise the
/// per-cascade shadow views are cleared and the lit shaders fall back to
/// the baked `LIT_DIR`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Gate {
    #[default]
    Auto,
    Force,
    Off,
}

impl Gate {
    /// `BO2MP_H1_SUN_SHADOW_GATE` = `auto` (default), `force` or `off`.
    pub fn from_env() -> Self {
        match std::env::var("BO2MP_H1_SUN_SHADOW_GATE")
            .unwrap_or_else(|_| String::new())
            .to_ascii_lowercase()
            .as_str()
        {
            "1" | "force" => Self::Force,
            "2" | "off" => Self::Off,
            _ => Self::Auto,
        }
    }
}

/// Bit `i` of a cell bit array as the engine lays it out: word `i / 32`,
/// most significant bit first.
pub fn cell_bit(bits: &[u32], i: usize) -> bool {
    bits.get(i >> 5)
        .is_some_and(|w| w & (0x8000_0000u32 >> (i & 31)) != 0)
}

/// Packs per-cell flags into the engine's bit layout (`cell_bit`).
pub fn pack_cell_bits(flags: impl IntoIterator<Item = bool>) -> Vec<u32> {
    let mut out = Vec::new();
    for (i, f) in flags.into_iter().enumerate() {
        if out.len() <= i >> 5 {
            out.push(0);
        }
        if f {
            out[i >> 5] |= 0x8000_0000u32 >> (i & 31);
        }
    }
    out
}

/// Whether the main view draws the sun shadow (read): the gate, the sun
/// being on, and in `Auto` a cell both visible to the view and marked as
/// having sun-lit surfaces (`GfxWorld::cellHasSunLitSurfsBits`).
pub fn view_has_sun_shadow(
    gate: Gate,
    sun_on: bool,
    cells: usize,
    visible: &[u32],
    sun_lit: &[u32],
) -> bool {
    match gate {
        Gate::Off => false,
        Gate::Force => sun_on,
        Gate::Auto => sun_on && (0..cells).any(|i| cell_bit(visible, i) && cell_bit(sun_lit, i)),
    }
}

/// A cached tile's size in texels: half the sun map's width (read).
pub fn cache_tile_texels(res: u32) -> u32 {
    res / 2
}

/// Floor division (the engine rounds the tile index toward minus
/// infinity).
fn div_floor(a: i32, b: i32) -> i32 {
    a.div_euclid(b)
}

/// The world-anchored tiles a cascade's drawn rectangle needs (read):
/// `x0`, `y0` the rectangle's first texel on the world texel grid (texel
/// `i` covers `[i t, (i + 1) t)` along the axis), `w`, `h` its size, `tile`
/// the tile size in texels. Tile `(i, j)` covers texels `[i T, (i + 1) T)`
/// by `[j T, (j + 1) T)`; the last index is taken at `x0 + w` (inclusive).
pub fn cache_tiles(x0: i32, y0: i32, w: i32, h: i32, tile: u32) -> Vec<(i32, i32)> {
    let t = tile.max(1) as i32;
    let (i0, i1) = (div_floor(x0, t), div_floor(x0 + w, t));
    let (j0, j1) = (div_floor(y0, t), div_floor(y0 + h, t));
    let mut out = Vec::new();
    for j in j0..=j1 {
        for i in i0..=i1 {
            out.push((i, j));
        }
    }
    out
}

impl Setup {
    /// Cascade `k`'s used band on the world texel grid: its first texel
    /// along x and y and its size (`U` by `U`).
    pub fn world_texel_rect(&self, k: usize) -> (i32, i32, i32, i32) {
        let t = self.texel[k];
        let u = self.settings.used() as i32;
        let x0 = ((self.centre[k][0] / t) + 0.5 - self.fit.eye_texel[0]).floor() as i32;
        let y0 = ((self.centre[k][1] / t) + 0.5 - self.fit.eye_texel[1]).floor() as i32;
        (x0, y0, u, u)
    }

    /// The cached tiles cascade `k` needs this frame.
    pub fn cascade_tiles(&self, k: usize) -> Vec<(i32, i32)> {
        let (x0, y0, w, h) = self.world_texel_rect(k);
        cache_tiles(x0, y0, w, h, cache_tile_texels(self.settings.res))
    }

    /// A cached tile's projection of world positions (read): x and y over
    /// the tile, `2 (a · p) / (T t) - 2 i - 1`, the same reversed depth as
    /// the cascades.
    pub fn tile_projection(&self, k: usize, (i, j): (i32, i32)) -> Mat4 {
        let size = cache_tile_texels(self.settings.res) as f32 * self.texel[k];
        let sc = 2.0 / size;
        let (n, f) = self.near_far;
        let z = self.s / (f - n);
        let row = |a: Vec3, i: i32| [a.x * sc, a.y * sc, a.z * sc, -2.0 * i as f32 - 1.0];
        Mat4::from_cols_array_2d(&[
            row(self.x, i),
            row(self.y, j),
            [z.x, z.y, z.z, f / (f - n)],
            [0.0, 0.0, 0.0, 1.0],
        ])
        .transpose()
    }
}

/// A cached tile's key: the cascade, the sun direction it was drawn for
/// (bits), its index and its world size (bits).
pub type TileKey = (usize, [u32; 3], (i32, i32), u32);

/// The cached tiles kept between frames (the engine keeps a fixed pool,
/// reusing the least recently used; capacity here: 9 a cascade, the
/// "sun shadow cached" views). A tile is drawn once (static casters only)
/// and reused while its key holds; `CacheMode::Force` redraws every tile
/// each frame.
#[derive(Clone, Debug, Default)]
pub struct TileCache {
    /// (key, last frame used)
    tiles: Vec<(TileKey, u64)>,
    frame: u64,
    pub capacity: usize,
}

impl TileCache {
    pub fn new(capacity: usize) -> Self {
        Self {
            capacity,
            ..Default::default()
        }
    }

    /// Marks `keys` used this frame and returns the ones to (re)draw:
    /// new keys (evicting the least recently used past capacity), or all of
    /// them when forced.
    pub fn update(&mut self, keys: &[TileKey], force: bool) -> Vec<TileKey> {
        self.frame += 1;
        let mut draw = Vec::new();
        for k in keys {
            if let Some(t) = self.tiles.iter_mut().find(|t| t.0 == *k) {
                t.1 = self.frame;
                if force {
                    draw.push(*k);
                }
                continue;
            }
            if self.tiles.len() >= self.capacity.max(1)
                && let Some(old) = self
                    .tiles
                    .iter()
                    .enumerate()
                    .filter(|(_, t)| t.1 != self.frame)
                    .min_by_key(|(_, t)| t.1)
                    .map(|(i, _)| i)
            {
                self.tiles.swap_remove(old);
            }
            self.tiles.push((*k, self.frame));
            draw.push(*k);
        }
        draw
    }

    pub fn len(&self) -> usize {
        self.tiles.len()
    }

    pub fn is_empty(&self) -> bool {
        self.tiles.is_empty()
    }
}

impl Setup {
    /// The cache keys of every tile this frame's cascades need.
    pub fn tile_keys(&self) -> Vec<TileKey> {
        let s = self.s.to_array().map(f32::to_bits);
        (0..self.cascades())
            .flat_map(|k| {
                let size = (cache_tile_texels(self.settings.res) as f32 * self.texel[k]).to_bits();
                self.cascade_tiles(k)
                    .into_iter()
                    .map(move |ij| (k, s, ij, size))
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn block_masks_follow_the_cascade_count() {
        let src = CascadeSource {
            cascades: 3,
            scale: [1.0, 2.0, 3.0, 4.0],
            ..Default::default()
        };
        let b = block_constants(&src);
        assert_eq!(b[2].1, [8.0, 8.0, 8.0, 0.0]);
        assert_eq!(b[3].1, [1.0, 2.0, 3.0, 4.0]);
        assert_eq!(b[5].1, [1.0, 1.0, 1.0, 0.0]);
        let b = block_constants(&CascadeSource { cascades: 1, ..src });
        assert_eq!(b[2].1, [8.0, 0.0, 0.0, 0.0]);
        assert_eq!(b[5].1, [1.0, 1.0, 0.0, 0.0]);
        let names: Vec<_> = b.iter().map(|(n, _)| *n).collect();
        let idx: Vec<_> = names
            .iter()
            .map(|n| {
                fastfile_h1::code_source::CODE_CONST
                    .iter()
                    .position(|c| c == n)
            })
            .collect();
        assert_eq!(
            idx,
            (196..203).map(Some).collect::<Vec<_>>(),
            "constants 196..202 in order"
        );
    }

    #[test]
    fn scale_and_offset() {
        let (s, o) = scale_and_uv_offset(resolution_scale(1.0), 3);
        assert_eq!(s, [16.0, 48.0, -8.0, -24.0]);
        assert_eq!(o, [0.0, 2.0 / 3.0, 0.0, 0.0]);
        assert_eq!(resolution_scale(0.1), 0.25);
        assert_eq!(resolution_scale(2.0), 1.0);
        assert_eq!(used_size(2048, 0.3), 616);
        assert_eq!(border(2048, 616), 716);
    }

    #[test]
    fn lookup_maps_clip_to_the_stacked_map() {
        // Identity "projection": clip = position.
        let id = [
            [1.0, 0.0, 0.0, 0.0],
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
            [0.0, 0.0, 0.0, 1.0],
        ];
        let l = lookup_matrix(id, 3, [0.0, 0.0, 0.5, -1.0]);
        // p = (x, y, z, 1) as a row vector times l.
        let at = |p: [f32; 4]| -> [f32; 4] {
            std::array::from_fn(|c| (0..4).map(|r| p[r] * l[r][c]).sum())
        };
        // Clip corner (-1, 1) is the top-left of the first cascade's band.
        assert_eq!(at([-1.0, 1.0, 0.25, 1.0]), [0.0, 0.0, 0.25, 0.125 - 1.0]);
        // (1, -1): u 1, v at the band's bottom (1/3).
        let q = at([1.0, -1.0, 0.0, 1.0]);
        assert!((q[0] - 1.0).abs() < 1e-6 && (q[1] - 1.0 / 3.0).abs() < 1e-6);
    }

    #[test]
    fn snapping() {
        assert_eq!(snap_to_texel(10.2, 4.0), 10.0);
        assert_eq!(snap_to_texel(-0.1, 4.0), -2.0);
    }

    #[test]
    fn axes_follow_the_sun() {
        let (x, y, s) = axes(Vec3::new(0.3, -0.4, 0.8));
        assert!((x.cross(y) - s).length() < 1e-5);
        assert!(x.z.abs() < 1e-6, "x level for a sun below the limit");
        // A near-vertical sun builds around world x.
        let (x, _, s) = axes(Vec3::new(0.01, 0.0, 1.0));
        assert!(x.dot(s).abs() < 1e-6 && x.x.abs() < 1e-5);
    }

    #[test]
    fn depth_range_is_the_box_along_the_light() {
        let (n, f) = depth_range(Vec3::Z, Vec3::splat(-100.0), Vec3::new(100.0, 100.0, 300.0));
        assert_eq!((n, f), (-300.0, 100.0));
        let s = Vec3::new(1.0, 0.0, 1.0).normalize();
        let (n, f) = depth_range(s, Vec3::splat(-1.0), Vec3::splat(1.0));
        assert!((f - 2f32.sqrt()).abs() < 1e-5 && (n + 2f32.sqrt()).abs() < 1e-5);
        assert_eq!(
            near_far_plane((-300.0, 100.0)),
            [-300.0, 100.0, 400.0, 1.0 / 400.0]
        );
    }

    #[test]
    fn axis_fit_keeps_the_rays_inside() {
        for (lo, hi) in [
            (-0.3, 1.2),
            (-1.0, 0.2),
            (-0.5, 0.5),
            (0.0, 1.0),
            (-0.01, 0.9),
        ] {
            let (at, k) = fit_axis(lo, hi, 2048);
            // The eye on a texel centre, every ray inside the band.
            assert_eq!(at.fract(), 0.5, "{lo} {hi}");
            let a = at + k * lo;
            let b = at + k * hi;
            assert!(a >= 0.5 - 1e-3 && b <= 2047.5 + 1e-3, "{lo} {hi}: {a} {b}");
            // Close to the plain fit (U - 2) / span.
            let k0 = 2046.0 / (hi - lo);
            assert!((k / k0 - 1.0).abs() < 0.01, "{lo} {hi}: {k} {k0}");
        }
    }

    fn view(eye: Vec3, forward: Vec3) -> View {
        let forward = forward.normalize();
        let right = forward.cross(Vec3::Z).normalize();
        View {
            eye,
            forward,
            right,
            up: right.cross(forward),
            tan_x: 1.0,
            tan_y: 0.6,
            near: 4.0,
        }
    }

    fn setup(eye: Vec3, forward: Vec3) -> Setup {
        Setup::new(
            Settings {
                res: 2048,
                cascades: 3,
                ..Settings::default()
            },
            Vec3::new(0.3, -0.4, 0.8),
            view(eye, forward),
            (Vec3::splat(-3000.0), Vec3::splat(3000.0)),
        )
    }

    /// Cascade `k`'s map UV and depth of an eye-relative point, from its
    /// projection and band.
    fn map_uv(s: &Setup, k: usize, p: Vec3) -> (f32, f32, f32) {
        let c = s.view_projection(k) * p.extend(1.0);
        let u = (c.x + 1.0) / 2.0;
        let v = (k as f32 + (1.0 - c.y) / 2.0) / ROWS as f32;
        (u, v, c.z)
    }

    #[test]
    fn switch_partitions_reach_each_cascade() {
        let s = setup(Vec3::new(123.4, -567.8, 90.0), Vec3::new(1.0, 0.3, -0.1));
        let l = s.lookup();
        for p in [
            Vec3::new(10.0, 20.0, -5.0),
            Vec3::new(-700.0, 300.0, 40.0),
            Vec3::new(2500.0, -1800.0, -200.0),
        ] {
            let q = l * p.extend(1.0);
            let (u0, v0, z0) = map_uv(&s, 0, p);
            assert!((q.x - u0).abs() < 1e-5 && (q.y - v0).abs() < 1e-5);
            assert!((q.z - z0).abs() < 1e-5);
            assert!((q.w - p.dot(s.view.forward) / s.fit.depth).abs() < 1e-4);
            for k in 0..2 {
                let sp = s.switch_partition(k);
                let (u, v, z) = map_uv(&s, k + 1, p);
                assert!((q.x * sp[3] + sp[0] - u).abs() < 1e-4, "u {k} {p}");
                assert!((q.y * sp[3] + sp[1] - v).abs() < 1e-4, "v {k} {p}");
                assert!((z - z0).abs() < 1e-6, "one depth for every cascade");
            }
        }
        assert_eq!(s.switch_partition(0)[3], 0.25);
        assert_eq!(s.switch_partition(1)[3], 1.0 / 16.0);
    }

    #[test]
    fn the_frustum_fits_the_first_cascade() {
        for fwd in [
            Vec3::new(1.0, 0.0, 0.0),
            Vec3::new(-0.3, 1.0, -0.2),
            Vec3::new(0.2, 0.1, -1.0),
        ] {
            let s = setup(Vec3::new(5.0, 7.0, 3.0), fwd);
            assert!(s.fit.depth > 0.0);
            // Every corner ray, out to the fitted length, lands inside the
            // used part of each cascade's band; the eye on its texel.
            let rays = corner_rays(
                s.view.forward,
                s.view.right,
                s.view.up,
                s.view.tan_x,
                s.view.tan_y,
            );
            let min_cos = rays
                .iter()
                .map(|r| r.dot(s.view.forward))
                .fold(1.0f32, f32::min);
            let len = s.fit.depth / min_cos;
            for k in 0..3 {
                for r in rays {
                    let (u, v, _) = map_uv(&s, k, r * len * CASCADE_STEP[k] * 0.999);
                    assert!((0.0..=1.0).contains(&u), "{fwd} {k} u {u}");
                    let band = v * ROWS as f32 - k as f32;
                    assert!((0.0..=1.0).contains(&band), "{fwd} {k} v {band}");
                }
                let (u, _, _) = map_uv(&s, k, Vec3::ZERO);
                let texel = u * 2048.0;
                assert!(
                    (texel - s.fit.eye_texel[0]).abs() <= 0.5 + 1e-2,
                    "{k} {texel}"
                );
            }
        }
        // Looking along a shadow axis puts the eye near one edge.
        let s = setup(Vec3::ZERO, Vec3::new(1.0, 0.0, 0.0));
        let (x, _, _) = axes(Vec3::new(0.3, -0.4, 0.8));
        let at = if x.x > 0.0 {
            s.fit.eye_texel[0]
        } else {
            2048.0 - s.fit.eye_texel[0]
        };
        assert!(at < 700.0, "eye texel {at}");
    }

    #[test]
    fn depth_is_reversed_toward_the_sun() {
        let s = setup(Vec3::new(5.0, 7.0, 3.0), Vec3::X);
        let toward = s.s * 100.0;
        let (_, _, z_sun) = map_uv(&s, 0, toward);
        let (_, _, z) = map_uv(&s, 0, Vec3::ZERO);
        assert!(z_sun > z && (0.0..1.0).contains(&z) && z_sun < 1.0);
        // The shadow bounds' sun-most corner is 1, the far one 0.
        let far = Vec3::splat(3000.0) * -s.s.signum() - s.view.eye;
        let near = Vec3::splat(3000.0) * s.s.signum() - s.view.eye;
        assert!((map_uv(&s, 0, near).2 - 1.0).abs() < 1e-4);
        assert!(map_uv(&s, 0, far).2.abs() < 1e-4);
        // Winding as before: x × y = s.
        assert!((s.x.cross(s.y) - s.s).length() < 1e-5);
    }

    #[test]
    fn centres_move_in_whole_texels() {
        let a = setup(Vec3::ZERO, Vec3::X);
        assert_eq!(a.texel, vec![0.25, 1.0, 4.0]);
        let t = a.texel[0];
        let c = setup(Vec3::new(3.0, 1.0, 2.0), Vec3::X);
        for k in 0..2 {
            let dx = (c.centre[0][k] - a.centre[0][k]) / t;
            assert!((dx - dx.round()).abs() < 1e-3);
        }
        // Within a texel the centre stays.
        let e = a.x * a.centre[0][0] + a.y * a.centre[0][1];
        let b = setup(e + a.x * 0.02, Vec3::X);
        let b2 = setup(e - a.x * 0.02, Vec3::X);
        assert_eq!(b.centre[0], b2.centre[0]);
    }

    #[test]
    fn pixel_adjust_and_clamp() {
        let p = pixel_adjust(2048, 2.0);
        assert_eq!(p[0], [2.0 / 2048.0, 2.0 / 6144.0, 0.0, 0.0]);
        assert_eq!(p[1][0], p[0][0] / 4.0);
        assert_eq!(p[2][1], p[0][1] / 16.0);
        let c = v_clamp(&p);
        assert_eq!(c[0], 1.0 / 3.0 - p[0][1]);
        assert_eq!(c[1], 1.0 / 3.0 + p[1][1]);
        assert_eq!(c[2], 2.0 / 3.0 - p[1][1]);
        let b = distance_blend(300.0, 3, 2048, 4.0);
        assert_eq!(b[0], (16.0f32 * 300.0).powi(2));
        assert_eq!(b[1], 1.0 / 512.0f32.powi(2));
    }

    #[test]
    fn constants_are_all_set() {
        let s = setup(Vec3::ZERO, Vec3::X);
        let mut c = CodeValues::default();
        s.apply(&mut c);
        for n in [
            "TRANSPOSE_SHADOW_LOOKUP_MATRIX",
            "SUN_SHADOWMAP_PIXEL_ADJUST_ARRAY_0",
            "SUN_SHADOWMAP_PIXEL_ADJUST_ARRAY_2",
            "SUN_SHADOWMAP_CASCADE_V_CLAMP",
            "SUN_SHADOWMAP_NEAR_FAR_PLANE",
            "SHADOWMAP_SCALE",
            "SHADOWMAP_DISTANCE_BLEND",
        ] {
            let i = crate::code::code_index(n);
            assert_ne!(c.row(i, 0), [0.0; 4], "{n}");
        }
        assert_eq!(viewport(2048, 2), [0.0, 4096.0, 2048.0, 2048.0]);
    }

    #[test]
    fn defaults_and_sizes() {
        let d = Settings::default();
        assert_eq!((d.res, d.cascades), (1024, 2));
        assert_eq!((d.sample_size, d.filter_radius, d.scale), (0.25, 4.0, 1.0));
        assert_eq!(d.cache, CacheMode::Disabled);
        assert_eq!(Quality::Auto.map_sizes(), Quality::Normal.map_sizes());
        assert_eq!(Quality::High.map_sizes(), (2048, 1024));
        assert_eq!(Quality::Extra.map_sizes(), (3072, 1536));
        assert_eq!(cache_tile_texels(2048), 1024);
    }

    /// Points of the view slab cascade `k` covers, and points moved toward
    /// the sun from them (their casters).
    fn slab_and_casters(s: &Setup, k: usize) -> Vec<Vec3> {
        let v = &s.view;
        let rays = corner_rays(v.forward, v.right, v.up, v.tan_x, v.tan_y);
        let near = if k == 0 {
            v.near
        } else {
            CASCADE_STEP[k - 1] * s.fit.depth
        };
        let far = CASCADE_STEP[k] * s.fit.depth * 0.98;
        let mut out = Vec::new();
        for r in rays {
            let cos = r.dot(v.forward);
            for depth in [near * 1.01, (near + far) * 0.5, far] {
                for a in [0.0, 0.5, 0.95] {
                    let q = v.eye
                        + (r * a + v.forward * (1.0 - a)).normalize() * (depth / cos.max(0.2));
                    // Inside the slab only if its view depth is in range.
                    let dz = (q - v.eye).dot(v.forward);
                    if dz < near || dz > CASCADE_STEP[k] * s.fit.depth {
                        continue;
                    }
                    // Only receivers the cascade's band holds.
                    let rel = q - v.eye;
                    let c = s.view_projection(k) * rel.extend(1.0);
                    if c.x.abs() > 0.999 || c.y.abs() > 0.999 {
                        continue;
                    }
                    for up in [0.0, 50.0, 400.0] {
                        out.push(q + s.s * up);
                    }
                }
            }
        }
        out
    }

    #[test]
    fn caster_planes_keep_every_caster_of_the_slab() {
        for fwd in [
            Vec3::new(1.0, 0.0, 0.0),
            Vec3::new(-0.3, 1.0, -0.2),
            Vec3::new(0.2, 0.1, -1.0),
            // Into the sun and away from it.
            Vec3::new(0.3, -0.4, 0.8),
            Vec3::new(-0.3, 0.4, -0.8),
        ] {
            let s = setup(Vec3::new(5.0, 7.0, 3.0), fwd);
            for k in 0..s.cascades() {
                let planes = s.caster_planes(k);
                assert!(planes.len() >= 4, "{fwd} {k}");
                let pts = slab_and_casters(&s, k);
                assert!(pts.len() >= 30, "{fwd} {k}: {} points", pts.len());
                for p in pts {
                    let e = Vec3::splat(0.01);
                    assert!(
                        box_inside(&planes, p - e, p + e),
                        "{fwd} cascade {k}: {p} culled"
                    );
                }
            }
        }
    }

    #[test]
    fn caster_planes_cull_what_cannot_shadow_the_view() {
        let s = setup(Vec3::ZERO, Vec3::new(1.0, 0.0, 0.0));
        let k = 0;
        let planes = s.caster_planes(k);
        // Far outside the cascade's band.
        let far = s.x * 1.0e5;
        assert!(!box_inside(&planes, far - 1.0, far + 1.0));
        // Behind the eye, away from the view.
        let back = Vec3::new(-300.0, 0.0, 0.0);
        let inside_band = {
            let c = s.view_projection(k) * back.extend(1.0);
            c.x.abs() < 1.0 && c.y.abs() < 1.0
        };
        if inside_band && silhouette(&s.view, s.x, s.y, s.s).is_some() {
            assert!(!box_inside(&planes, back - 1.0, back + 1.0), "{back}");
        }
        // A box straddling a plane is kept.
        assert!(box_inside(&planes, Vec3::splat(-1.0e5), Vec3::splat(1.0e5)));
    }

    #[test]
    fn faces_and_silhouette() {
        // Looking level with the sun high and to one side: two faces each
        // way, a silhouette pair whose normals point into the wedge.
        let v = view(Vec3::ZERO, Vec3::X);
        let sun = Vec3::new(0.3, -0.4, 0.8).normalize();
        let (x, y, s) = axes(sun);
        let f = face_sun(&v, s);
        assert!(f.iter().any(|&c| c > CULL_EPSILON) && f.iter().any(|&c| c < -CULL_EPSILON));
        let sil = silhouette(&v, x, y, s).expect("silhouette");
        let fwd = Vec2::new(v.forward.dot(x), v.forward.dot(y));
        for (d, n) in sil {
            assert!((d.length() - 1.0).abs() < 1e-5 && d.dot(n).abs() < 1e-5);
            assert!(n.dot(fwd) > 0.0, "normal into the wedge");
        }
        // Looking straight into the sun: every face toward it, no
        // silhouette, the near plane instead.
        let v = view(Vec3::ZERO, sun);
        assert!(face_sun(&v, s).iter().all(|&c| c > CULL_EPSILON));
        assert!(silhouette(&v, x, y, s).is_none());
        let st = setup(Vec3::ZERO, sun);
        let p = st.caster_planes(1);
        assert!(
            p.iter()
                .any(|q| (q.truncate() - st.view.forward).length() < 1e-5
                    && (q.w + st.fit.depth).abs() < 1e-3)
        );
        // Away from the sun: no face toward it, no side planes.
        let v = view(Vec3::ZERO, -sun);
        assert!(side_planes(&v, s).is_empty());
    }

    #[test]
    fn band_planes_match_the_projection() {
        let s = setup(Vec3::new(123.4, -567.8, 90.0), Vec3::new(1.0, 0.3, -0.1));
        let b = s.settings.border_texels() as f32;
        let res = s.settings.res as f32;
        for k in 0..3 {
            let planes = s.band_planes(k);
            // The used band's edges in clip space.
            let lo = 2.0 * b / res - 1.0;
            for (cx, cy, inside) in [
                (lo + 0.01, 0.0, true),
                (-lo - 0.01, 0.0, true),
                (lo - 0.01, 0.0, false),
                (0.0, -lo + 0.01, false),
            ] {
                // Back from clip to a world point on the band.
                let m = s.world_projection(k).inverse();
                let p = (m * Vec4::new(cx, cy, 0.5, 1.0)).truncate();
                let kept = planes.iter().all(|q| q.truncate().dot(p) + q.w >= 0.0);
                assert_eq!(kept, inside, "cascade {k} clip ({cx}, {cy})");
            }
        }
    }

    #[test]
    fn gate() {
        let vis = pack_cell_bits([false, true, false, true, true]);
        let lit = pack_cell_bits([true, false, false, true, false]);
        assert_eq!(vis, vec![0x5800_0000]);
        assert!(cell_bit(&vis, 1) && !cell_bit(&vis, 2) && !cell_bit(&vis, 40));
        assert!(view_has_sun_shadow(Gate::Auto, true, 5, &vis, &lit));
        let lit2 = pack_cell_bits([true, false, true, false, false]);
        assert!(!view_has_sun_shadow(Gate::Auto, true, 5, &vis, &lit2));
        assert!(!view_has_sun_shadow(Gate::Auto, false, 5, &vis, &lit));
        assert!(view_has_sun_shadow(Gate::Force, true, 0, &[], &[]));
        assert!(!view_has_sun_shadow(Gate::Force, false, 0, &[], &[]));
        assert!(!view_has_sun_shadow(Gate::Off, true, 5, &vis, &lit));
        let many = pack_cell_bits((0..70).map(|i| i == 33));
        assert_eq!(many, vec![0, 0x4000_0000, 0]);
    }

    #[test]
    fn cached_tiles() {
        assert_eq!(cache_tiles(0, 0, 1023, 0, 1024), vec![(0, 0)]);
        assert_eq!(cache_tiles(-1, 0, 1024, 0, 1024), vec![(-1, 0), (0, 0)]);
        assert_eq!(cache_tiles(10, 10, 2048, 2048, 1024).len(), 9);
        let s = setup(Vec3::new(3000.0, -2000.0, 50.0), Vec3::X);
        for k in 0..3 {
            let tiles = s.cascade_tiles(k);
            assert!(!tiles.is_empty() && tiles.len() <= 9, "{k} {}", tiles.len());
            // The tile's projection puts its own corner texel at clip -1
            // and agrees with the cascade's grid.
            let (i, j) = tiles[0];
            let size = cache_tile_texels(2048) as f32 * s.texel[k];
            let p = s.x * (i as f32 * size) + s.y * (j as f32 * size);
            let c = s.tile_projection(k, (i, j)) * p.extend(1.0);
            assert!((c.x + 1.0).abs() < 1e-3 && (c.y + 1.0).abs() < 1e-3);
            assert!((c.z - (s.world_projection(k) * p.extend(1.0)).z).abs() < 1e-5);
            // The cascade's first texel on the world grid is whole.
            let (x0, _, w, _) = s.world_texel_rect(k);
            let edge = s.world_projection(k)
                * (s.x * (x0 as f32 * s.texel[k]) + s.y * s.centre[k][1]).extend(1.0);
            let texel = (edge.x + 1.0) / 2.0 * 2048.0 - s.settings.border_texels() as f32;
            assert!(texel.abs() < 1e-2, "{k} {texel}");
            assert_eq!(w as u32, s.settings.used());
        }
        let mut c = TileCache::new(4);
        let key = |i| (0usize, [0u32; 3], (i, 0), 0u32);
        assert_eq!(c.update(&[key(0), key(1)], false).len(), 2);
        assert!(c.update(&[key(0), key(1)], false).is_empty());
        assert_eq!(c.update(&[key(0)], true), vec![key(0)]);
        // Past capacity the least recently used goes.
        c.update(&[key(2), key(3)], false);
        assert_eq!(c.update(&[key(4)], false), vec![key(4)]);
        assert_eq!(c.len(), 4);
        assert_eq!(
            c.update(&[key(1)], false),
            vec![key(1)],
            "key 1 was evicted"
        );
    }
}
