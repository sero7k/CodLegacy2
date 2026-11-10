//! The sun shadow cascades as BlackOps3.exe fits them (CPU side).
//!
//! The view setup 0x141e069a0 builds a camera record and calls the fit
//! 0x141d42820 (ported here from its disassembly: Ghidra loses the helper
//! arguments), which fills the 800-byte shadow state the view keeps at
//! +0x1440: three cascades over the view frustum, each an orthographic
//! view of the sun's space (the map's sun shadow info matrix) snapped to its
//! 32-texel tiles, with the streamed sun shadow tree level each reads. From
//! the state come the `depth_fill_from_sst` constants (DrawSST, 0x141e079b0),
//! the lighting's sun constants (0x141d43c20) and `minMaxConstants`
//! (0x141db8530), and the per-cascade view records the casters draw with.
//!
//! Matrices are row-vector (`v' = v·M`, translation in row 3), as the exe
//! keeps them.

#[allow(unused_imports)]
use crate::profile::{ComputeExt as _, EncExt as _, PassExt as _, QueueExt as _};
use fastfile_t7::native::ViewSun;
use render_h1::code::Camera;

pub type M4 = [[f32; 4]; 4];

/// The sun shadow maps' size (render target 5: 1024², six layers of D16,
/// three cascades per GPU; `0x141dcda90`). The fit takes it in 32-texel
/// tiles (pool +0x40e2 >> 5).
pub const MAP_SIZE: u32 = 1024;
pub const TILES: i32 = (MAP_SIZE / 32) as i32;
pub const CASCADES: usize = 3;

/// `DAT_142f87020`.
const EPS: f32 = 1.192_092_9e-7;
const SQRT2: f32 = 1.414_213_5;
const FLT_MAX: f32 = f32::MAX;

/// The map's sun shadow info (sun volume +1056, 112 bytes: what the view's
/// +0x350 points at) and the tree record's resident level count.
#[derive(Clone, Debug)]
pub struct ShadowInfo {
    /// +0x00: the first resident level, then each level's first word in the
    /// streamed tree.
    pub ints: [i32; 4],
    /// +0x10: world to sun space.
    pub matrix: M4,
    /// +0x50: the tree's size in tiles (128 cells each) at the coarsest
    /// level.
    pub size: [f32; 2],
    /// +0x58: a cell's size (world units) at the coarsest level.
    pub cell: f32,
    /// +0x5c: the height scale the tree's words are read with.
    pub height: f32,
    /// +0x60, +0x64 (copied, not read by the fit).
    pub w60: [u32; 2],
    /// The tree record's +0x5c (camera record +0x78): levels not resident.
    pub streamed: i32,
}

impl ShadowInfo {
    pub fn read(info: &[u8], record: &[u8]) -> Option<Self> {
        let w = |o: usize| info.get(o..o + 4).map(|b| u32::from_le_bytes(b.try_into().unwrap()));
        let f = |o: usize| w(o).map(f32::from_bits);
        let mut matrix = [[0f32; 4]; 4];
        for (r, row) in matrix.iter_mut().enumerate() {
            for (c, v) in row.iter_mut().enumerate() {
                *v = f(0x10 + 16 * r + 4 * c)?;
            }
        }
        Some(Self {
            ints: [w(0)? as i32, w(4)? as i32, w(8)? as i32, w(12)? as i32],
            matrix,
            size: [f(0x50)?, f(0x54)?],
            cell: f(0x58)?,
            height: f(0x5c)?,
            w60: [w(0x60)?, w(0x64)?],
            streamed: record.get(0x5c..0x60).map_or(0, |b| i32::from_le_bytes(b.try_into().unwrap())),
        })
    }
}

/// The min/max tree's record (sun volume +1504, 48 bytes: view +0x358).
#[derive(Clone, Copy, Debug, Default)]
pub struct MinMax {
    pub width: i32,
    pub height: i32,
    pub scale: [f32; 2],
}

impl MinMax {
    pub fn read(rec: &[u8]) -> Option<Self> {
        let w = |o: usize| rec.get(o..o + 4).map(|b| u32::from_le_bytes(b.try_into().unwrap()));
        Some(Self { width: w(0)? as i32, height: w(4)? as i32, scale: [f32::from_bits(w(8)?), f32::from_bits(w(12)?)] })
    }
}

/// The camera record 0x141e069a0 builds for the fit.
#[derive(Clone, Debug)]
pub struct FitCamera {
    /// +0x00: world to view (x right, y up, z forward).
    pub view: M4,
    /// +0x40: the eye; +0x4c: the forward axis.
    pub origin: [f32; 3],
    pub forward: [f32; 3],
    /// +0x58 / +0x5c: tangents of the half fields of view (view +0x280,
    /// +0x284).
    pub tan: [f32; 2],
    /// +0x60: the maps' size in 32-texel tiles.
    pub tiles: i32,
    /// +0x64: the GPU index (multi-GPU alternate frames; 0).
    pub gpu: i32,
    /// +0x68 / +0x6c: view +0x360 / +0x364 (copied into the state; their
    /// source, the client's `+0x1a6bc8` record, is not read: zero).
    pub w68: f32,
    pub v6c: [f32; 3],
}

impl FitCamera {
    pub fn new(camera: &Camera) -> Self {
        let rot = camera.rotation;
        let e = camera.eye;
        // World to view, absolute: rotation · translate(−eye).
        let m = rot * glam::Mat4::from_translation(-e);
        let view: M4 = std::array::from_fn(|i| m.col(i).to_array());
        Self {
            view,
            origin: e.to_array(),
            forward: [rot.col(0).z, rot.col(1).z, rot.col(2).z],
            tan: [camera.tan_x, camera.tan_y],
            tiles: TILES,
            gpu: 0,
            w68: 0.0,
            v6c: [0.0; 3],
        }
    }
}

/// The 800-byte shadow state (0x141d42820's output; the view's +0x1440).
#[derive(Clone, Debug, Default)]
pub struct ShadowState {
    /// 0x000: per cascade, world to the cascade's sun view.
    pub view: [M4; CASCADES],
    /// 0x0c0: per cascade, its orthographic projection (reversed depth over
    /// 16384 units).
    pub proj: [M4; CASCADES],
    /// 0x180: the cascade's top-left corner in tree tiles.
    pub uv: [[f32; 2]; CASCADES],
    /// 0x198: the top-left corner in sun space.
    pub corner: [[f32; 2]; CASCADES],
    /// 0x1e0: the tree level each reads (0 finest).
    pub level: [i32; CASCADES],
    /// 0x1ec: texel size (world units).
    pub texel: [f32; CASCADES],
    /// 0x1f8: each level's first word in the streamed tree.
    pub level_offset: [i32; 3],
    /// 0x204: the cascades' distances.
    pub distance: [f32; CASCADES],
    /// 0x210: settings +0x34 (shadow distance).
    pub shadow_distance: f32,
    /// 0x214: the sun-space depth the projections start at.
    pub z_base: f32,
    /// 0x218 / 0x21c: settings +0x38 / +0x30.
    pub s38: f32,
    pub s30: f32,
    /// 0x220: levels not resident (max of the camera's and the tree's).
    pub streamed: i32,
    /// 0x224: tiles a side; 0x228: the first cascade's layer.
    pub tiles: i32,
    pub layer: i32,
    /// 0x22c: the frustum planes in sun space (first fit pass).
    pub planes: [[f32; 4]; 6],
    /// 0x28c: the eye in sun space; 0x294: the frustum's mean direction;
    /// 0x29c: its half-angle cosine less ε (−1 when wider than that).
    pub eye: [f32; 2],
    pub axis: [f32; 2],
    pub cone: f32,
    /// 0x2a0..0x2ff: the info's ints, matrix, size, cell, height, +0x60.
    pub info_ints: [i32; 4],
    pub info_matrix: M4,
    pub info_size: [f32; 2],
    pub info_cell: f32,
    pub info_height: f32,
    pub info_w60: [u32; 2],
    /// 0x308 / 0x314: the camera's +0x6c / +0x68.
    pub v308: [f32; 3],
    pub w314: f32,
    /// The eye the fit ran from (the camera record's +0x40). Not part of
    /// the 800-byte state: the sun builder's caller (`141db98e0`) hands it
    /// the view's eye as a translation matrix, see [`ShadowState::lookup`].
    pub origin: [f32; 3],
}

fn mul(a: &M4, b: &M4) -> M4 {
    std::array::from_fn(|i| std::array::from_fn(|j| a[i][0] * b[0][j] + a[i][1] * b[1][j] + a[i][2] * b[2][j] + a[i][3] * b[3][j]))
}

fn inverse(m: &M4) -> M4 {
    let g = glam::Mat4::from_cols_array_2d(m).inverse();
    g.to_cols_array_2d()
}

/// `v·M` for a point (w = 1).
fn point(m: &M4, v: [f32; 3]) -> [f32; 4] {
    std::array::from_fn(|j| v[0] * m[0][j] + v[1] * m[1][j] + v[2] * m[2][j] + m[3][j])
}

/// `0x141d44520`: the fit's perspective projection (reversed depth).
fn projection(tan: [f32; 2], near: f32, far: f32) -> M4 {
    let n = near.max(1.0);
    let f = (n + 1.0).max(far);
    let mut p = [[0f32; 4]; 4];
    p[0][0] = 1.0 / tan[0];
    p[1][1] = 1.0 / tan[1];
    p[2][2] = -n / (f - n);
    p[2][3] = 1.0;
    p[3][2] = n * f / (f - n);
    p
}

/// `0x141d44950`: six planes `(n, d)` with `n·x = d` on the plane (the
/// clip planes from the columns, normals negated, normalised).
fn planes(m: &M4) -> [[f32; 4]; 6] {
    let col = |j: usize| [m[0][j], m[1][j], m[2][j], m[3][j]];
    let (c0, c1, c2, c3) = (col(0), col(1), col(2), col(3));
    let add = |a: [f32; 4], b: [f32; 4]| std::array::from_fn::<f32, 4, _>(|k| a[k] + b[k]);
    let sub = |a: [f32; 4], b: [f32; 4]| std::array::from_fn::<f32, 4, _>(|k| a[k] - b[k]);
    let mut p = [add(c3, c0), sub(c3, c0), sub(c3, c1), add(c3, c1), sub(c3, c2), c2];
    for q in &mut p {
        let l = (q[0] * q[0] + q[1] * q[1] + q[2] * q[2]).sqrt();
        let s = if l <= 1e-7 { 0.0 } else { 1.0 / l };
        q[0] *= -s;
        q[1] *= -s;
        q[2] *= -s;
        q[3] *= s;
    }
    p
}

/// `0x14238a0a0` (in doubles): the point on three planes.
fn intersect(a: [f32; 4], b: [f32; 4], c: [f32; 4]) -> [f32; 3] {
    let d = |p: [f32; 4]| [p[0] as f64, p[1] as f64, p[2] as f64, p[3] as f64];
    let (a, b, c) = (d(a), d(b), d(c));
    let cross = |u: [f64; 4], v: [f64; 4]| [u[1] * v[2] - u[2] * v[1], u[2] * v[0] - u[0] * v[2], u[0] * v[1] - u[1] * v[0]];
    let (bc, ca, ab) = (cross(b, c), cross(c, a), cross(a, b));
    let det = a[0] * bc[0] + a[1] * bc[1] + a[2] * bc[2];
    if det.abs() < 0.001 {
        return [0.0; 3];
    }
    std::array::from_fn(|k| ((bc[k] * a[3] + ca[k] * b[3] + ab[k] * c[3]) / det) as f32)
}

/// `0x141d44b80`: frustum corner `i` (0..3 on plane 4, 4..7 on plane 5).
fn corner(p: &[[f32; 4]; 6], i: usize) -> [f32; 3] {
    let (a, b, c) = match i {
        0 => (4, 2, 0),
        1 => (4, 2, 1),
        2 => (4, 3, 0),
        3 => (4, 3, 1),
        4 => (5, 2, 0),
        5 => (5, 2, 1),
        6 => (5, 3, 0),
        _ => (5, 3, 1),
    };
    intersect(p[a], p[b], p[c])
}

fn z_range(c: &[[f32; 3]; 8]) -> (f32, f32) {
    let (mut lo, mut hi) = (FLT_MAX, -FLT_MAX);
    for p in c {
        if p[2] - lo < 0.0 {
            lo = p[2];
        }
        if hi - p[2] < 0.0 {
            hi = p[2];
        }
    }
    (lo, hi)
}

fn normalize2(v: [f32; 2]) -> [f32; 2] {
    let l = v[0] * v[0] + v[1] * v[1];
    let s = if l <= 0.0 { 0.0 } else { 1.0 / l.sqrt() };
    [v[0] * s, v[1] * s]
}

fn dot2(a: [f32; 2], b: [f32; 2]) -> f32 {
    a[0] * b[0] + a[1] * b[1]
}

/// `0x141d420d0`: the cascades' distances and texel sizes for a map `res`
/// texels wide: `d = 2·(|z|·(k·p − c·p) + c·p)` (`z` the sun direction's
/// height, `p` the shadow distance, `c = √(0.4775909² + 0.7208867)`, `k =
/// 0.8490505`), cascades at d/8, d/2, d; the texel size made a multiple of
/// four texels over the map plus 32 texels.
fn splits(dir_z: f32, distance: f32, res: i32) -> ([f32; 3], [f32; 3]) {
    let c = (0.477_590_9f32 * 0.477_590_9 + 0.720_886_7f32).sqrt();
    let mut d = dir_z.abs() * (distance * 0.849_050_46 - c * distance) + c * distance;
    d += d;
    let res = res as f32;
    let t = d / res;
    let q = t * 4.0;
    let n = ((t * 32.0 + d) / q).ceil();
    let v = (n * q) / res;
    ([d * 0.125, d * 0.5, d], [v * 0.125, v * 0.5, v])
}

/// The fit `0x141d42820` for shadow mode 3 (`flag5`, `flag6`: the dvars
/// `0x1486bf028` / `0x1486bf6f0`, both off by default).
pub fn fit(sun: &ViewSun, info: &ShadowInfo, cam: &FitCamera, flag5: bool, flag6: bool) -> ShadowState {
    let mut st = ShadowState::default();
    let s30 = sun.f(0x30);
    let s34 = sun.f(0x34);
    let s38 = sun.f(0x38);
    // 0x141d42240: the info and settings words, the resident levels.
    st.info_ints = info.ints;
    st.info_matrix = info.matrix;
    st.info_size = info.size;
    st.info_cell = info.cell;
    st.info_height = info.height;
    st.info_w60 = info.w60;
    st.w314 = cam.w68;
    st.v308 = cam.v6c;
    st.s38 = s38;
    st.s30 = s30;
    st.origin = cam.origin;
    let p = info.ints;
    let streamed = cam_streamed(info).max(p[0]);
    st.streamed = streamed;
    let k = streamed - p[0];
    let at = |i: i32| p[(i.min(2) + 1) as usize];
    if k < 1 {
        st.level_offset = [p[1], p[2], p[3]];
    } else {
        st.level_offset[0] = 0;
        let mut base = at(p[0] + 1);
        if k > 1 {
            st.level_offset[1] = 0;
            base = at(p[0] + 2);
            st.level_offset[2] = if k > 2 { 0 } else { p[3] - base };
        } else {
            st.level_offset[1] = p[2] - base;
            st.level_offset[2] = p[3] - base;
        }
    }
    st.tiles = cam.tiles;
    st.layer = cam.gpu * 3;
    let res = cam.tiles << 5;
    let resf = res as f32;

    let s = info.matrix;
    let to_view = mul(&inverse(&s), &cam.view);
    st.shadow_distance = s34;
    let (dist, texel) = splits(sun.direction[2], s34, res);
    st.distance = dist;
    st.texel = texel;
    let far1 = resf * texel[2] * SQRT2;
    let frustum = |far: f32| -> ([[f32; 4]; 6], [[f32; 3]; 8]) {
        let pl = planes(&mul(&to_view, &projection(cam.tan, 1.0, far)));
        (pl, std::array::from_fn(|i| corner(&pl, i)))
    };
    let (planes1, corners1) = frustum(far1);
    let (lo, hi) = z_range(&corners1);
    let (_, corners2) = frustum(12287.75 / (hi - lo) * far1);
    let (lo2, _) = z_range(&corners2);

    // The frustum's spread in the sun plane: the edge directions from the
    // eye to the near corners, the most opposed pair, their mean.
    let e = point(&s, cam.origin);
    let ahead = point(&s, [cam.origin[0] + cam.forward[0], cam.origin[1] + cam.forward[1], cam.origin[2] + cam.forward[2]]);
    let fwd2 = [ahead[0] - e[0], ahead[1] - e[1]];
    let dirs: [[f32; 2]; 4] = std::array::from_fn(|i| normalize2([corners1[i][0] - e[0], corners1[i][1] - e[1]]));
    let (mut best, mut bi, mut bj) = (FLT_MAX, 0usize, 1usize);
    for i in 0..4 {
        for j in i + 1..4 {
            let d = dot2(dirs[i], dirs[j]);
            if best > d {
                best = d;
                bi = i;
                bj = j;
            }
        }
    }
    let axis = normalize2([(dirs[bi][0] + dirs[bj][0]) * 0.5, (dirs[bi][1] + dirs[bj][1]) * 0.5]);
    let d1 = dot2(axis, dirs[bj]);
    let mut m = dot2(dirs[bi], axis);
    if d1 - m < 0.0 {
        m = d1;
    }
    let thresh = m - EPS;
    st.cone = if dirs.iter().any(|d| thresh > dot2(axis, *d)) { -1.0 } else { thresh };
    st.planes = planes1;
    st.eye = [e[0], e[1]];
    st.axis = axis;

    // The tree levels' cell sizes (the first `streamed` not resident).
    let mut levels = [info.cell * 0.125, info.cell * 0.5, info.cell];
    for l in levels.iter_mut().take(streamed.clamp(0, 3) as usize) {
        *l = 0.0;
    }
    let z_base = ((lo2 - 0.250_003_8) / 4095.8125).floor() * 4095.8125;
    st.z_base = z_base;
    for i in 0..CASCADES {
        let r = texel[i];
        st.level[i] = if flag5 {
            levels.iter().position(|&l| l > 0.0).unwrap_or(3) as i32
        } else {
            let mut k = 2usize;
            while k > 0 {
                if r + EPS >= levels[k] || levels[k - 1] == 0.0 {
                    break;
                }
                k -= 1;
            }
            k as i32
        };
        let ext = resf * r;
        let half = ext * 0.5;
        let tile = r * 32.0;
        let along = (dist[i] + 1.0) * 0.5;
        let (x0, y0) = if !flag6 {
            let d = along - (tile + s30) * SQRT2;
            let c = point(&s, [cam.origin[0] + cam.forward[0] * d, cam.origin[1] + cam.forward[1] * d, cam.origin[2] + cam.forward[2] * d]);
            (((c[0] - half) / tile - 0.5).floor() * tile, ((c[1] + half) / tile + 0.5).ceil() * tile)
        } else {
            let c = point(&s, [cam.origin[0] + cam.forward[0] * along, cam.origin[1] + cam.forward[1] * along, cam.origin[2] + cam.forward[2] * along]);
            let ox = if 0.0 <= fwd2[0] { -1.0 } else { 1.0 };
            let oy = if 0.0 > fwd2[1] { 1.0 } else { -1.0 };
            let x = (c[0] - half) / tile + ox + 0.5;
            let y = (c[1] + half) / tile + oy + 0.5;
            ((x - x % 1.0) * tile, (y - y % 1.0) * tile)
        };
        let mut v = s;
        v[3][0] -= x0 + half;
        v[3][1] -= y0 - half;
        v[3][2] -= z_base;
        st.view[i] = v;
        let a = 2.0 / ext;
        st.proj[i] = [[a, 0.0, 0.0, 0.0], [0.0, a, 0.0, 0.0], [0.0, 0.0, f32::from_bits(0xb880_0000), 0.0], [0.0, 0.0, 1.0, 1.0]];
        let k = 1.0 / (info.cell * 128.0);
        st.uv[i] = [x0 * k + info.size[0] * 0.5, y0 * -k + info.size[1] * 0.5];
        st.corner[i] = [x0, y0];
    }
    st
}

/// The camera record's +0x78: the tree record's levels not resident.
fn cam_streamed(info: &ShadowInfo) -> i32 {
    info.streamed
}

/// A tree level's cell scale (`DAT_142f87ab4` = 8 for level 0, 2, then 1).
/// Sun constants 8..11 (LightingGlobals 0x130..0x13f) in mode 3: the shadow
/// state's +0x314 / +0x308..+0x310, which `141d42240` copies from the view
/// (+0x360..+0x36c) in every shadow mode: the global probe's config words
/// 0..3 (probe units, tint), already written by `lighting_globals::build`.
/// The state here does not read that view record (`FitCamera::w68` / `v6c`
/// are zero), so its zeros overwrote them: every deferred permutation
/// multiplies the probe volume's light by float 8, so with the map's sun
/// shadows on (mode 3: the PC, with the map's packages) everything out of
/// the sun was pure black (Aquarium 2026-10-27: palms, planters, the
/// harbour water and the debris on it). Kept from `g` when the state has
/// none of its own. `CODLEGACY_T7_SHADOW_PROBE_WORDS=zero`: the old zeros (for
/// before/after checks).
fn keep_probe_words(g: &[u8], f: &mut [f32; 64]) {
    if codlegacy_env::var("CODLEGACY_T7_SHADOW_PROBE_WORDS").is_ok_and(|v| v.trim() == "zero") {
        return;
    }
    if f[8..12].iter().all(|v| *v == 0.0) {
        for k in 0..4 {
            let at = 0x130 + 4 * k;
            if let Some(b) = g.get(at..at + 4) {
                f[8 + k] = f32::from_le_bytes(b.try_into().unwrap());
            }
        }
    }
}

fn level_scale(level: i32) -> f32 {
    match level {
        0 => 8.0,
        1 => 2.0,
        _ => 1.0,
    }
}

impl ShadowState {
    /// Sun floats 44..59, the shadow lookup matrix: `141d43c20` multiplies
    /// its fourth argument by the tree's world-to-sun matrix
    /// (`142388870`). Its caller `141db98e0` (via `141db8530`'s fourth
    /// argument, the local at rsp+0x60) builds that argument as rows
    /// (1,0,0,0) (0,1,0,0) (0,0,1,0) (eye, 1), the eye from the view
    /// (+0x240..+0x248, also its +0x1a0 the global probe's centre is made
    /// relative to): the lighting reconstructs eye-relative positions
    /// (eye-relative inverse view-projection), so the lookup is
    /// `translate(eye) · S`. With the identity there (before 2026-10-28)
    /// every pixel read the cascades at its eye-relative position as if it
    /// were absolute: the shadows slid with the player and showed the
    /// wrong casters, shimmering as the camera moved.
    pub fn lookup(&self) -> M4 {
        let o = self.origin;
        let t: M4 = [[1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0], [o[0], o[1], o[2], 1.0]];
        mul(&t, &self.info_matrix)
    }

    /// A cascade's view-projection (the per-cascade view record +0xc0).
    pub fn view_projection(&self, c: usize) -> M4 {
        mul(&self.view[c], &self.proj[c])
    }

    /// The DrawSST constants (`0x141e079b0`): three vec4 per cascade, as
    /// `depth_fill_from_sst` reads them (`cb4`).
    pub fn sst_constants(&self) -> [[u32; 4]; 9] {
        let mut out = [[0u32; 4]; 9];
        for c in 0..CASCADES {
            let r = self.texel[c];
            let scale = level_scale(self.level[c]);
            let bias = (self.s30 / r).max(1.0);
            let b = |x: f32| x.to_bits();
            out[3 * c] = [b(self.uv[c][0]), b(self.uv[c][1]), b(self.corner[c][0]), b(self.corner[c][1])];
            out[3 * c + 1] = [b(self.info_height), b((r / self.info_cell) * 0.007_812_5), b(r), b(scale * self.info_size[0])];
            let z = (self.z_base - (bias + 1.0) * r * SQRT2 * self.s38) * (1.0 / 16384.0) + 1.0;
            let off = self.level_offset[self.level[c].clamp(0, 2) as usize];
            out[3 * c + 2] = [b(z), off as u32, b(scale), self.layer as u32];
        }
        out
    }

    /// The shadow state's part of the sun constants (`0x141d43c20`, mode 3:
    /// floats 3, 8-23, 37, 40-63; the lookup matrix is the eye's
    /// translation times the tree's, [`ShadowState::lookup`]).
    pub fn sun_constants(&self, f: &mut [f32; 64]) {
        let level = self.streamed.max(2);
        f[3] = self.z_base;
        f[8] = self.w314;
        f[9..12].copy_from_slice(&self.v308);
        let res = (self.tiles << 5) as f32;
        let half = res * 0.5;
        for c in 0..CASCADES {
            let r = self.texel[c];
            let s = half * r;
            f[12 + 4 * c] = s + self.corner[c][0];
            f[13 + 4 * c] = self.corner[c][1] - s;
            f[14 + 4 * c] = 1.0 / s;
            f[15 + 4 * c] = (1.0 / res) * (self.s30 / r).max(1.0);
        }
        f[27] = 0.0;
        f[37] = f32::from_bits(self.layer as u32);
        f[40] = self.info_size[0];
        f[41] = self.info_size[1];
        f[42] = self.info_cell;
        f[43] = self.info_height;
        let lookup = self.lookup();
        for r in 0..4 {
            f[44 + 4 * r..48 + 4 * r].copy_from_slice(&lookup[r]);
        }
        f[60] = level_scale(level);
        f[61] = f32::from_bits(self.level_offset[level.clamp(0, 2) as usize] as u32);
        f[62] = 0.0;
        f[63] = 0.0;
    }

    /// `minMaxConstants` (LightingGlobals 0x210..0x22c, `0x141db8530`;
    /// 0x218 is a stack slot nothing writes: zero).
    pub fn min_max_constants(&self, mm: &MinMax) -> [f32; 8] {
        let c = self.info_cell;
        [
            mm.scale[0] * (1.0 / self.info_size[0]),
            mm.scale[1] * (1.0 / self.info_size[1]),
            0.0,
            0.0,
            0.5 / mm.width as f32,
            0.5 / mm.height as f32,
            1.0 / (c * 0.125 * 128.0),
            c * 3.0,
        ]
    }
}

/// The sun shadow maps on the GPU: the streamed tree, the cascades filled
/// from it each frame (`depth_fill_from_sst` into the tile cache, render
/// target 6, then `floatz_writedepth` into the cascade's layer of render
/// target 5; `0x141d6fba0`) and the min/max tree's image.
///
/// The exe fills only the tiles that scrolled in or changed
/// (`0x141e06020`, `depth_copy_in_place` moving the rest); every tile is
/// refilled here each frame, which writes the same depths. The cache is
/// R32F (the exe's is R16_UNORM: the maps are D16, so the depths written
/// are the same to the map's precision).
pub struct SunShadows {
    info: ShadowInfo,
    minmax: MinMax,
    /// The last fit (set by [`SunShadows::update`]).
    pub state: Option<ShadowState>,
    /// `gSunShadowTree` (also the lighting's).
    pub tree: wgpu::Buffer,
    consts: wgpu::Buffer,
    fill: crate::compute::Program,
    fill_groups: Vec<Vec<wgpu::BindGroup>>,
    _cache: wgpu::Texture,
    maps: wgpu::Texture,
    /// `gSunShadowmapArray` (D16, a layer per cascade).
    pub maps_view: wgpu::TextureView,
    layers: Vec<wgpu::TextureView>,
    floatz: crate::post_pass::CodePass,
    floatz_groups: [wgpu::BindGroup; 3],
    vertices: wgpu::Buffer,
    /// `gSunShadowMax`.
    pub max: crate::gpu::Tex,
    _buffers: crate::post_pass::CodeBuffers,
    _per_scene: wgpu::Buffer,
    _generics: wgpu::Buffer,
    /// What was made, for the lighting's report.
    pub report: Vec<String>,
    /// Per cascade, the DrawSST constants its layer was last filled with:
    /// the fill reads only them and the static tree, so a cascade whose
    /// constants did not change (the fit moves cascades in whole tiles and
    /// their depth base in steps of 4096 units) already holds the same
    /// depths and is not filled again (`T7_SHADOW_REFILL=1`: every frame).
    filled: std::sync::Mutex<[Option<[[u32; 4]; 3]>; CASCADES]>,
}

fn refill_always() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var("T7_SHADOW_REFILL").is_ok_and(|v| !v.is_empty() && v != "0"))
}

/// The tile ids `depth_fill_from_sst` reads (`gDispatchIDs`): every tile of
/// cascade `c`, `ty << 16 | tx << 2 | c` (`0x141e05cc0`).
fn tile_ids(c: u32) -> Vec<u8> {
    let n = TILES as u32;
    let mut v = Vec::with_capacity((n * n * 4) as usize);
    for x in 0..n {
        for y in 0..n {
            v.extend_from_slice(&((((y & 0x1f) << 14) | (x & 0x1f)) << 2 | c).to_le_bytes());
        }
    }
    v
}

impl SunShadows {
    pub fn new(gpu: &render_h1::gpu::Gpu, map: &fastfile_t7::map::T7Map, patch_image: &[u8], patch: &fastfile_t7::walk::capture::Capture, shaders: &std::collections::HashMap<String, crate::compute::CodeShader>) -> Result<Self, String> {
        use wgpu::util::DeviceExt;
        let t = map.light_refs().and_then(|r| r.shadow_tree.clone()).ok_or("the map has no sun shadow tree")?;
        let info = ShadowInfo::read(&t.info, &t.record).ok_or("the sun shadow info is not read")?;
        let minmax = MinMax::read(&t.minmax_record).ok_or("the min/max tree record is not read")?;
        let data = map.paks().ok_or("no packages")?.read(t.key).map_err(|e| format!("the sun shadow tree's data: {e}"))?;
        let dev = &gpu.device;
        let limit = u64::from(dev.limits().max_storage_buffer_binding_size);
        if data.len() as u64 > limit {
            return Err(format!("the sun shadow tree ({} bytes) is past the device's storage binding limit ({limit})", data.len()));
        }
        let mut report = vec![format!("sun shadows: tree {} bytes, levels at words {:?}, {} x {} tiles of {} units", data.len(), &info.ints[1..], info.size[0], info.size[1], info.cell * 128.0)];
        let tree = dev.create_buffer_init(&wgpu::util::BufferInitDescriptor { label: Some("gSunShadowTree"), contents: &data, usage: wgpu::BufferUsages::STORAGE });
        drop(data);
        let consts = dev.create_buffer(&wgpu::BufferDescriptor { label: Some("DrawSST constants"), size: 144, usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST, mapped_at_creation: false });
        let ids: Vec<wgpu::Buffer> = (0..3).map(|c| dev.create_buffer_init(&wgpu::util::BufferInitDescriptor { label: Some("gDispatchIDs"), contents: &tile_ids(c), usage: wgpu::BufferUsages::STORAGE })).collect();
        let cs = shaders.get("depth_fill_from_sst").ok_or("depth_fill_from_sst: not in core_patch")?;
        let fill = crate::compute::Program::new(gpu, cs, &[(0, "r32float")])?;
        let size = wgpu::Extent3d { width: MAP_SIZE, height: MAP_SIZE, depth_or_array_layers: 1 };
        let cache = dev.create_texture(&wgpu::TextureDescriptor {
            label: Some("sun shadow tile cache (rt 6)"),
            size,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::R32Float,
            usage: wgpu::TextureUsages::STORAGE_BINDING | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let cache_store = cache.create_view(&wgpu::TextureViewDescriptor { dimension: Some(wgpu::TextureViewDimension::D2), ..Default::default() });
        let cache_read = cache.create_view(&wgpu::TextureViewDescriptor { dimension: Some(wgpu::TextureViewDimension::D2Array), ..Default::default() });
        let mut fill_groups = Vec::new();
        for c in 0..3 {
            fill_groups.push(fill.bind(gpu, |s| {
                Some(match (&s.kind, s.slot) {
                    (crate::compute::SlotKind::Uniform { .. }, _) => crate::compute::Res::Buffer(&consts),
                    (crate::compute::SlotKind::ReadBuffer, 0) => crate::compute::Res::Buffer(&tree),
                    (crate::compute::SlotKind::ReadBuffer, 1) => crate::compute::Res::Buffer(&ids[c]),
                    (crate::compute::SlotKind::StorageTexture { .. }, _) => crate::compute::Res::View(&cache_store),
                    _ => return None,
                })
            })?);
        }
        let maps = dev.create_texture(&wgpu::TextureDescriptor {
            label: Some("gSunShadowmapArray (rt 5)"),
            size: wgpu::Extent3d { depth_or_array_layers: CASCADES as u32, ..size },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Depth16Unorm,
            // (Copied out by the checks.)
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let maps_view = maps.create_view(&wgpu::TextureViewDescriptor { dimension: Some(wgpu::TextureViewDimension::D2Array), ..Default::default() });
        let layers = (0..CASCADES as u32).map(|l| maps.create_view(&wgpu::TextureViewDescriptor { dimension: Some(wgpu::TextureViewDimension::D2), base_array_layer: l, array_layer_count: Some(1), ..Default::default() })).collect();
        let techs = crate::post_pass::code_techs(patch_image, patch, &["floatz_writedepth"])?;
        let tech = &techs["floatz_writedepth"];
        report.push(format!("sun shadows: floatz_writedepth depth state {:#x}", tech.state.depth));
        let floatz = crate::post_pass::CodePass::new_ext(gpu, tech, &[], Some(wgpu::TextureFormat::Depth16Unorm), &tech.args.iter().filter(|a| a.ty & 0xff00 == 0x500).map(|a| a.u[0]).collect::<Vec<_>>(), true)?;
        let buffers = crate::post_pass::CodeBuffers::new(gpu);
        let per = crate::post_pass::per_scene(MAP_SIZE, MAP_SIZE);
        let per_scene = dev.create_buffer_init(&wgpu::util::BufferInitDescriptor { label: Some("floatz PerSceneConsts"), contents: bytemuck::cast_slice(&per), usage: wgpu::BufferUsages::UNIFORM });
        // GenericsCBuffer as 0x141d6fba0 sets it: (layer 0, the map's
        // size), then (1, 0, 1, 0): the unit square scaled to the map, its
        // texture coordinates unscaled.
        let generics: [f32; 8] = [0.0, MAP_SIZE as f32, 0.0, 0.0, 1.0, 0.0, 1.0, 0.0];
        let generics = dev.create_buffer_init(&wgpu::util::BufferInitDescriptor { label: Some("floatz GenericsCBuffer"), contents: bytemuck::cast_slice(&generics), usage: wgpu::BufferUsages::UNIFORM });
        let nearest = dev.create_sampler(&wgpu::SamplerDescriptor { label: Some("floatZWriteDepthSampler"), ..Default::default() });
        let floatz_groups = floatz.bind_with(gpu, &per_scene, &generics, &[(3, &generics)], &buffers, |_| Some(&cache_read), |_| Some(&nearest))?;
        let vertices = dev.create_buffer_init(&wgpu::util::BufferInitDescriptor { label: Some("floatz quad"), contents: &crate::post_pass::triangle(1, 1), usage: wgpu::BufferUsages::VERTEX });
        let name = t.minmax_image.clone().ok_or("the min/max tree has no image")?;
        let max = map.image(&name, 16384).map_err(|e| format!("{name}: {e}")).and_then(|im| {
            report.push(format!("sun shadows: gSunShadowMax {name} {}x{} DXGI {}", im.width, im.height, im.format));
            upload_max(gpu, &im)
        })?;
        Ok(Self { info, minmax, state: None, tree, consts, fill, fill_groups, _cache: cache, maps, maps_view, layers, floatz, floatz_groups, vertices, max, _buffers: buffers, _per_scene: per_scene, _generics: generics, report, filled: Default::default() })
    }

    /// Fits the cascades to `camera` and writes the DrawSST constants.
    pub fn update(&mut self, gpu: &render_h1::gpu::Gpu, sun: &ViewSun, camera: &Camera) {
        let st = fit(sun, &self.info, &FitCamera::new(camera), false, false);
        gpu.queue.write_buffer_p(&self.consts, 0, bytemuck::cast_slice(&st.sst_constants()));
        self.state = Some(st);
    }

    /// Takes a state made elsewhere (a check's own cascade placement) and
    /// writes its DrawSST constants, for [`SunShadows::record`].
    pub fn set_state(&mut self, gpu: &render_h1::gpu::Gpu, st: ShadowState) {
        gpu.queue.write_buffer_p(&self.consts, 0, bytemuck::cast_slice(&st.sst_constants()));
        self.state = Some(st);
    }

    /// The lighting constants the shadow state gives: the sun constants'
    /// shadow fields and `minMaxConstants`.
    pub fn patch_globals(&self, g: &mut [u8], sun: &ViewSun, time: f32) {
        let Some(st) = &self.state else { return };
        let mut f = crate::lighting_globals::sun_constants(sun, 3, time);
        st.sun_constants(&mut f);
        keep_probe_words(g, &mut f);
        for (i, v) in f.iter().enumerate() {
            g[0x110 + 4 * i..0x114 + 4 * i].copy_from_slice(&v.to_bits().to_le_bytes());
        }
        for (i, v) in st.min_max_constants(&self.minmax).iter().enumerate() {
            g[0x210 + 4 * i..0x214 + 4 * i].copy_from_slice(&v.to_bits().to_le_bytes());
        }
    }

    /// Per cascade whose constants changed since its last fill: every tile
    /// filled from the tree into the cache, then the cache written into the
    /// cascade's layer.
    pub fn record(&self, enc: &mut wgpu::CommandEncoder) {
        let Some(st) = &self.state else { return };
        crate::timing::mark(enc, "clears");
        let consts = st.sst_constants();
        let mut filled = self.filled.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let groups = (TILES * TILES) as u32 * 16;
        for c in 0..CASCADES {
            let mine: [[u32; 4]; 3] = [consts[3 * c], consts[3 * c + 1], consts[3 * c + 2]];
            if filled[c] == Some(mine) && !refill_always() {
                continue;
            }
            filled[c] = Some(mine);
            {
                let mut pass = enc.compute_pass_p(&wgpu::ComputePassDescriptor { label: Some("depth_fill_from_sst"), timestamp_writes: None });
                pass.set_pipeline_p(&self.fill.pipeline);
                for (i, g) in self.fill_groups[c].iter().enumerate() {
                    pass.set_bind_group_p(i as u32, g, &[]);
                }
                pass.dispatch_p(groups, 1, 1);
            }
            crate::timing::count_shadow_draws(1);
            self.floatz.draw_depth(enc, &self.layers[c], Some(0.0), &self.floatz_groups, &self.vertices);
        }
        crate::timing::mark(enc, "sun shadow");
    }

    /// The cascade maps (for checks).
    pub fn maps(&self) -> &wgpu::Texture {
        &self.maps
    }
}

/// `gSunShadowMax` (R16_UNORM): as stored when the device has 16-bit norm
/// textures, else as R32F (R16F without 32-bit float filtering), the same
/// values.
fn upload_max(gpu: &render_h1::gpu::Gpu, im: &fastfile_t7::texture::ImageLevels) -> Result<crate::gpu::Tex, String> {
    if render_h1::gpu::dxgi_format(im.format) != Some(wgpu::TextureFormat::R16Unorm) || gpu.device.features().contains(wgpu::Features::TEXTURE_FORMAT_16BIT_NORM) {
        return crate::gpu::upload(gpu, "gSunShadowMax", im);
    }
    let format = if gpu.float32_filterable { wgpu::TextureFormat::R32Float } else { wgpu::TextureFormat::R16Float };
    let levels = im.levels.max(1);
    let texture = gpu.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("gSunShadowMax"),
        size: wgpu::Extent3d { width: im.width, height: im.height, depth_or_array_layers: 1 },
        mip_level_count: levels,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    let mut at = 0usize;
    for l in 0..levels {
        let (w, h) = ((im.width >> l).max(1), (im.height >> l).max(1));
        let n = (w * h) as usize;
        let Some(src) = im.data.get(at * 2..(at + n) * 2) else { break };
        at += n;
        let v = src.chunks(2).map(|b| f32::from(u16::from_le_bytes([b[0], b[1]])) / 65535.0);
        let bytes: Vec<u8> = if format == wgpu::TextureFormat::R32Float { v.flat_map(f32::to_le_bytes).collect() } else { v.flat_map(|x| fastfile_t7::light::f16_bits(x).to_le_bytes()).collect() };
        let bpp = if format == wgpu::TextureFormat::R32Float { 4 } else { 2 };
        gpu.queue.write_texture_p(
            wgpu::TexelCopyTextureInfo { texture: &texture, mip_level: l, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
            &bytes,
            wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(w * bpp), rows_per_image: Some(h) },
            wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
        );
    }
    let view = texture.create_view(&Default::default());
    Ok(crate::gpu::Tex { texture, view })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_sun() -> (ViewSun, ShadowInfo) {
        let d = glam::Vec3::new(-0.499, -0.349, 0.793).normalize();
        let mut sun = ViewSun { direction: d.to_array(), shadow_mode: 3, ..Default::default() };
        sun.config[0x30 / 4] = 1.0f32.to_bits();
        sun.config[0x34 / 4] = 4000.0f32.to_bits();
        sun.config[0x38 / 4] = 1.0f32.to_bits();
        // World to sun space: x, y across the sun, z along it; offset.
        let u = d.any_orthonormal_vector();
        let w = d.cross(u);
        let mut m = [[0f32; 4]; 4];
        for k in 0..3 {
            m[k] = [u[k], w[k], d[k], 0.0];
        }
        m[3] = [1234.5, -987.25, 3000.0, 1.0];
        let info = ShadowInfo { ints: [0, 0, 100, 200], matrix: m, size: [421.0, 215.0], cell: 282.0 / 128.0 * 8.0, height: 1.0, w60: [0; 2], streamed: 0 };
        (sun, info)
    }

    fn cam(eye: glam::Vec3, pitch: f32, yaw: f32) -> FitCamera {
        let view = render_h1::code::View { eye, angles: glam::Vec3::new(pitch, yaw, 0.0), fov_x: 80.0, width: 160, height: 90, near: 4.0 };
        FitCamera::new(&render_h1::code::Camera::from(&view))
    }

    /// The lighting reads the cascades at eye-relative positions through
    /// sun floats 44..59 (`translate(eye) · S`); a fixed world point must
    /// land on the same sun-space point, and on the same place inside a
    /// texel of each cascade, however the camera moves (sub-texel and
    /// multi-texel steps, turning): the cascades only ever move by whole
    /// 32-texel tiles, so static shadows neither slide with the player nor
    /// swim.
    #[test]
    fn a_world_point_stays_put_in_the_cascades_as_the_camera_moves() {
        let (sun, info) = test_sun();
        let p = [1520.0f32, -640.0, 96.0];
        let base = glam::Vec3::new(1200.0, -900.0, 60.0);
        let want = point(&info.matrix, p);
        let mut first: Option<ShadowState> = None;
        for (i, (step, pitch, yaw)) in [(0.0, 5.0, 30.0), (0.013, 5.0, 30.0), (0.37, 5.0, 30.0), (1.9, 5.2, 31.0), (7.5, 4.0, 33.0), (40.0, 6.0, 25.0), (133.0, 5.0, 30.0)].into_iter().enumerate() {
            let eye = base + glam::Vec3::new(step, step * 0.61, step * 0.07);
            let st = fit(&sun, &info, &cam(eye, pitch, yaw), false, false);
            let mut f = [0f32; 64];
            st.sun_constants(&mut f);
            let lk: M4 = std::array::from_fn(|r| std::array::from_fn(|c| f[44 + 4 * r + c]));
            let rel = [p[0] - eye.x, p[1] - eye.y, p[2] - eye.z];
            let got = point(&lk, rel);
            for k in 0..3 {
                assert!((got[k] - want[k]).abs() < 0.02, "move {i}: sun-space {k} {} vs {}", got[k], want[k]);
            }
            if let Some(f0) = &first {
                for c in 0..CASCADES {
                    assert_eq!(st.texel[c], f0.texel[c]);
                    let tile = st.texel[c] * 32.0;
                    for a in 0..2 {
                        let n = (st.corner[c][a] - f0.corner[c][a]) / tile;
                        assert!((n - n.round()).abs() < 1e-3, "move {i} cascade {c}: corner moved {n} tiles");
                        // The point's place inside its texel.
                        let t0 = (want[a] - f0.corner[c][a]) / f0.texel[c];
                        let t1 = (got[a] - st.corner[c][a]) / st.texel[c];
                        let d = (t1 - t0) - (t1 - t0).round();
                        assert!(d.abs() < 0.01, "move {i} cascade {c}: texel phase moved {d}");
                    }
                }
            } else {
                first = Some(st);
            }
        }
    }

    /// The old lookup (the tree's matrix alone) read eye-relative positions
    /// as absolute: the same world point mapped somewhere else for every eye.
    #[test]
    fn the_lookup_is_not_the_bare_tree_matrix() {
        let (sun, info) = test_sun();
        let st = fit(&sun, &info, &cam(glam::Vec3::new(1200.0, -900.0, 60.0), 5.0, 30.0), false, false);
        assert_ne!(st.lookup()[3], info.matrix[3]);
        assert_eq!(st.lookup()[0], info.matrix[0]);
    }

    /// Mode 3 keeps the global probe's words (sun floats 8..11) that the
    /// globals builder wrote; a state with its own words keeps those.
    #[test]
    fn probe_words_survive_the_shadow_patch() {
        let mut g = vec![0u8; 0x400];
        for (k, v) in [531.178f32, 0.876, 1.021, 1.161].iter().enumerate() {
            g[0x130 + 4 * k..0x134 + 4 * k].copy_from_slice(&v.to_le_bytes());
        }
        let mut f = [0.0f32; 64];
        super::keep_probe_words(&g, &mut f);
        assert_eq!(&f[8..12], &[531.178, 0.876, 1.021, 1.161]);
        let mut own = [0.0f32; 64];
        own[8] = 2.0;
        super::keep_probe_words(&g, &mut own);
        assert_eq!(own[8], 2.0);
    }
}
