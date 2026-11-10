//! Eye exposure as BlackOps3.exe works it out on the CPU each view, before
//! `exposure_compute` runs (`docs/T7_NATIVE.md`, "Post chain").
//!
//! * The current lighting state's `GfxGlobalLightSettings` give the base
//!   values: +0xb0 the exposure bias (EV), +0xb4 the lowest and +0xb8 the
//!   highest metered exposure (EV).
//! * Exposure volumes (`GfxExposureVolume`, 0x6c; `GfxWorld` +0x128 count,
//!   +0x130 records, +0x140 planes) the eye is in override them by weight
//!   ([`volume_weights`]); each per-state value is a u16 `v / 1024 - 32` EV
//!   (0xffff: not set), lerped `q·(1 - w) + w·value` in the volumes' sort
//!   order (record +0, ascending), with a per-state adaptation scale at
//!   +0x58.
//! * The metered luminance at the eye (the sun volume's exposure grids; its
//!   evaluation is not ported yet: [`ExposureState::update`] takes it) times
//!   `2^bias` is clamped to `0.18·2^(min-3) ..= 0.18·2^(max-3)`, eased
//!   toward over time and averaged over the last eight views; the result
//!   is the view's exposure EV `log2(avg / 0.18) + 3` and scale `2^(EV-3)`.

/// The per-state exposure values of one lighting state.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ExposureBase {
    pub bias: f32,
    pub min_ev: f32,
    pub max_ev: f32,
}

impl ExposureBase {
    /// From the sun volume at the lighting state (`GfxSunVolume` + 0xf4·state:
    /// +0xb0, +0xb4, +0xb8, as the exe reads them in 0x141dd6d10).
    pub fn from_settings(rec: &[u8]) -> Option<Self> {
        Some(Self { bias: f32_at(rec, 0xb0)?, min_ev: f32_at(rec, 0xb4)?, max_ev: f32_at(rec, 0xb8)? })
    }
}

fn f32_at(b: &[u8], at: usize) -> Option<f32> {
    b.get(at..at + 4).map(|x| f32::from_le_bytes(x.try_into().unwrap()))
}
fn u16_at(b: &[u8], at: usize) -> Option<u16> {
    b.get(at..at + 2).map(|x| u16::from_le_bytes(x.try_into().unwrap()))
}
fn i32_at(b: &[u8], at: usize) -> Option<i32> {
    b.get(at..at + 4).map(|x| i32::from_le_bytes(x.try_into().unwrap()))
}

/// One `GfxExposureVolume` (0x6c bytes) as the exe reads it.
#[derive(Clone, Debug, PartialEq)]
pub struct ExposureVolume {
    pub sort: i32,
    pub angles: [f32; 3],
    pub first_plane: usize,
    pub plane_count: usize,
    /// Falloff per box axis for planes facing against (`neg`) and along
    /// (`pos`) it.
    pub falloff_neg: [f32; 3],
    pub falloff_pos: [f32; 3],
    /// Per lighting state: bias (+0x40), min (+0x30), max (+0x38) as stored.
    pub bias_q: [u16; 4],
    pub min_q: [u16; 4],
    pub max_q: [u16; 4],
    pub adapt: [f32; 4],
    pub mask: u8,
}

pub const EXPOSURE_VOLUME: usize = 0x6c;

impl ExposureVolume {
    pub fn parse(r: &[u8]) -> Option<Self> {
        let f3 = |at: usize| Some([f32_at(r, at)?, f32_at(r, at + 4)?, f32_at(r, at + 8)?]);
        let q4 = |at: usize| Some([u16_at(r, at)?, u16_at(r, at + 2)?, u16_at(r, at + 4)?, u16_at(r, at + 6)?]);
        Some(Self {
            sort: i32_at(r, 0)?,
            angles: f3(4)?,
            first_plane: i32_at(r, 0x10)?.max(0) as usize,
            plane_count: i32_at(r, 0x14)?.max(0) as usize,
            falloff_neg: f3(0x18)?,
            falloff_pos: f3(0x24)?,
            min_q: q4(0x30)?,
            max_q: q4(0x38)?,
            bias_q: q4(0x40)?,
            adapt: [f32_at(r, 0x58)?, f32_at(r, 0x5c)?, f32_at(r, 0x60)?, f32_at(r, 0x64)?],
            mask: *r.get(0x68)?,
        })
    }
}

/// The engine's `AnglesToAxis` (degrees: pitch, yaw, roll): forward, left,
/// up.
pub fn angles_to_axis(a: [f32; 3]) -> [[f32; 3]; 3] {
    let (sp, cp) = a[0].to_radians().sin_cos();
    let (sy, cy) = a[1].to_radians().sin_cos();
    let (sr, cr) = a[2].to_radians().sin_cos();
    let forward = [cp * cy, cp * sy, -sp];
    let right = [-sr * sp * cy + cr * sy, -sr * sp * sy - cr * cy, -sr * cp];
    let up = [cr * sp * cy + sr * sy, cr * sp * sy - sr * cy, cr * cp];
    [forward, [-right[0], -right[1], -right[2]], up]
}

fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

/// The volumes the eye is in, with their weights (0 fully inside, 1 at the
/// outer edge of the falloff), at most 16, in the exe's sort order. `mask`
/// is `1 << (4 + lighting state)` as the exe passes it.
pub fn volume_weights(eye: [f32; 3], volumes: &[ExposureVolume], planes: &[[f32; 4]], mask: u8) -> Vec<(usize, f32)> {
    let mut out = Vec::new();
    for (i, v) in volumes.iter().enumerate() {
        if v.mask & mask == 0 {
            continue;
        }
        let axis = angles_to_axis(v.angles);
        let mut worst = -1.0f32;
        for p in planes.iter().skip(v.first_plane).take(v.plane_count) {
            let n = [p[0], p[1], p[2]];
            let d = dot(eye, n) + p[3];
            let mut w = d;
            if d > 0.0 {
                w = 1.0;
                if v.plane_count == 6 {
                    w = d;
                    for k in 0..3 {
                        let a = dot(n, axis[k]);
                        let fall = if a > 0.9 {
                            v.falloff_pos[k]
                        } else if a < -0.9 {
                            v.falloff_neg[k]
                        } else {
                            continue;
                        };
                        w /= fall.max(0.001);
                    }
                }
            }
            worst = worst.max(w);
        }
        if v.plane_count > 0 && worst > 1.0 {
            continue;
        }
        out.push((i, worst.clamp(0.0, 1.0)));
        if out.len() == 16 {
            break;
        }
    }
    out.sort_by_key(|&(i, _)| volumes[i].sort);
    out
}

/// What the exe's view setup takes into its post-FX settings.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ViewExposure {
    pub ev: f32,
    pub scale: f32,
    pub bias: f32,
    pub min_ev: f32,
    pub max_ev: f32,
}

/// The eye exposure kept from view to view (the client's 8-view history).
#[derive(Clone, Debug)]
pub struct ExposureState {
    history: [f32; 8],
    next: usize,
    smoothed: f32,
}

impl Default for ExposureState {
    fn default() -> Self {
        Self { history: [0.18; 8], next: 0, smoothed: 0.18 }
    }
}

fn dequant(q: u16) -> Option<f32> {
    (q != 0xffff).then(|| f32::from(q) * (1.0 / 1024.0) - 32.0)
}

impl ExposureState {
    /// One view: `base` the lighting state's values, `inside` the volumes
    /// and weights from [`volume_weights`], `state` the lighting state,
    /// `metered` the luminance metered at the eye, `adaptive` the eye
    /// adapting (else the history snaps to this view's value).
    pub fn update(&mut self, base: ExposureBase, volumes: &[ExposureVolume], inside: &[(usize, f32)], state: usize, metered: f32, adaptive: bool) -> ViewExposure {
        let s = state.min(3);
        let (mut bias, mut min_ev, mut max_ev, mut adapt) = (base.bias, base.min_ev, base.max_ev, 1.0f32);
        for &(i, w) in inside {
            let v = &volumes[i];
            if let Some(x) = dequant(v.bias_q[s]) {
                bias = x * (1.0 - w) + w * bias;
            }
            if let Some(x) = dequant(v.min_q[s]) {
                min_ev = x * (1.0 - w) + w * min_ev;
            }
            if let Some(x) = dequant(v.max_q[s]) {
                max_ev = x * (1.0 - w) + w * max_ev;
            }
            adapt = (1.0 - w) * v.adapt[s] + w * adapt;
        }
        let lum = metered * 2f32.powf(bias);
        let lo = 2f32.powf(min_ev - 3.0) * 0.18;
        let hi = 2f32.powf(max_ev - 3.0) * 0.18;
        let target = if lo >= lum { lo } else { lum.min(hi) };
        if adaptive {
            let prev = self.smoothed;
            let t = 0.04 / (target + 0.04);
            let k = (-(1.0 / 60.0) / ((1.0 - t) * 0.1 + t * 0.4)).exp();
            let eased = (((1.0 - k) * (target - prev) + prev) - target) * adapt + target;
            let ev = (target / 0.18).log2();
            let lo2 = 2f32.powf(ev - 5.0);
            let hi2 = 2f32.powf(ev + 10.0);
            let v = if lo2 >= eased { lo2 } else { eased.min(hi2) };
            self.history[self.next] = v;
            self.next = (self.next + 1) & 7;
        } else {
            self.history = [target; 8];
        }
        let avg = self.history.iter().sum::<f32>() * 0.125;
        self.smoothed = avg;
        let ev = (avg / 0.18).log2() + 3.0;
        ViewExposure { ev, scale: 2f32.powf(ev - 3.0), bias, min_ev, max_ev }
    }
}

/// `postFxControl0` and `postFxControl1` as the exe sets them for
/// `exposure_compute` (`view_index` from 0; `meter_scale` the setting the
/// exe puts at 0x1d5c: 1 normally).
pub fn exposure_controls(e: &ViewExposure, meter_scale: f32, adaptive: bool, view_index: u32) -> [[f32; 4]; 2] {
    [[e.min_ev, e.max_ev, e.bias, meter_scale], [e.ev, e.scale, if adaptive { 1.0 } else { 0.0 }, (view_index + 1) as f32]]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base() -> ExposureBase {
        ExposureBase { bias: 0.0, min_ev: 1.0, max_ev: 12.0 }
    }

    #[test]
    fn a_settled_view_lands_on_its_metered_ev() {
        let mut s = ExposureState::default();
        let e = s.update(base(), &[], &[], 0, 0.18 * 8.0, false);
        // 0.18·8 is EV 3 above mid grey: log2(8) + 3 = 6.
        assert!((e.ev - 6.0).abs() < 1e-4, "{e:?}");
        assert!((e.scale - 8.0).abs() < 1e-3);
    }

    #[test]
    fn metering_is_clamped_to_the_states_range() {
        let mut s = ExposureState::default();
        let e = s.update(base(), &[], &[], 0, 1e-6, false);
        assert!((e.ev - 1.0).abs() < 1e-4, "{e:?}");
        let e = s.update(base(), &[], &[], 0, 1e6, false);
        assert!((e.ev - 12.0).abs() < 1e-4, "{e:?}");
    }

    #[test]
    fn an_adapting_eye_moves_toward_the_target_over_views() {
        let mut s = ExposureState::default();
        let first = s.update(base(), &[], &[], 0, 0.18 * 64.0, true).ev;
        let mut last = first;
        for _ in 0..30 {
            last = s.update(base(), &[], &[], 0, 0.18 * 64.0, true).ev;
        }
        assert!(last > first && last <= 9.0 + 1e-3, "{first} {last}");
    }

    #[test]
    fn a_volume_overrides_by_weight_and_box_falloff() {
        let q = |ev: f32| ((ev + 32.0) * 1024.0) as u16;
        let v = ExposureVolume {
            sort: 0,
            angles: [0.0; 3],
            first_plane: 0,
            plane_count: 6,
            falloff_neg: [10.0; 3],
            falloff_pos: [10.0; 3],
            bias_q: [q(2.0); 4],
            min_q: [0xffff; 4],
            max_q: [0xffff; 4],
            adapt: [1.0; 4],
            mask: 0x10,
        };
        // A box 0..100 on each axis, planes facing out.
        let planes = [[1.0, 0.0, 0.0, -100.0], [-1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, -100.0], [0.0, -1.0, 0.0, 0.0], [0.0, 0.0, 1.0, -100.0], [0.0, 0.0, -1.0, 0.0]];
        let inside = volume_weights([50.0, 50.0, 50.0], std::slice::from_ref(&v), &planes, 0x10);
        assert_eq!(inside, vec![(0, 0.0)]);
        let edge = volume_weights([105.0, 50.0, 50.0], std::slice::from_ref(&v), &planes, 0x10);
        assert!((edge[0].1 - 0.5).abs() < 1e-5, "{edge:?}");
        assert!(volume_weights([150.0, 50.0, 50.0], std::slice::from_ref(&v), &planes, 0x10).is_empty());
        assert!(volume_weights([50.0, 50.0, 50.0], std::slice::from_ref(&v), &planes, 0x20).is_empty());
        let mut s = ExposureState::default();
        let e = s.update(base(), std::slice::from_ref(&v), &inside, 0, 0.18 * 8.0, false);
        assert!((e.bias - 2.0).abs() < 1e-3 && (e.ev - 8.0).abs() < 1e-3, "{e:?}");
    }
}

/// The sun volume's exposure grids for one lighting state (sun volume
/// +0x830 + 0x10·state: count, the luminance used where no grid covers
/// the eye, the probes of 0x50), as the client meters them each view (exe
/// 0x141dd5c30, from 0x141dd6d10 with the eye and the view's forward
/// axis).
///
/// A probe: origin (+0), half extents (+0xc), angles (+0x18), scale
/// (+0x24), voxel counts (u16 +0x28/0x2a/0x2c), voxels (six half-float
/// planes, ±x ±y ±z), blends (u16 count +0x38, 0x50 each), a flag (+0x48:
/// set, it overrides the unflagged probes). A blend: origin (+0), box half
/// extents (+0xc), falloffs below (+0x18) and above (+0x24) (at least 0.1),
/// angles (+0x30), faces (count +0x3c, planes of 16: then the weight is the
/// product of the clamped plane distances, not the box's), a flag (+0x48:
/// set, it cuts the probe out).
pub struct ExposureGrids {
    /// The probes' records, blends, faces and voxels, copied out of the
    /// zone (the probes' offsets are into this).
    image: fastfile_t7::kept::KeptBytes,
    probes: Vec<(usize, fastfile_t7::walk::gfxworld::ExposureGridParts)>,
    pub default: f32,
}

fn half(h: u16) -> f32 {
    let sign = u32::from(h & 0x8000) << 16;
    let e = u32::from(h >> 10 & 0x1f);
    let m = u32::from(h & 0x3ff);
    let bits = if e == 0 { if m != 0 { sign | 0x3880_0000 } else { sign } } else { (e * 0x80_0000 + 0x3800_0000) | m << 13 | sign };
    f32::from_bits(bits)
}

impl ExposureGrids {
    /// From the zone image, the sun volume's file offset, the walk's grid
    /// list (probes) and parts for `state`.
    pub fn new(image: &[u8], sun_volume: usize, state: usize, grids: fastfile_t7::walk::world::Span, parts: &[fastfile_t7::walk::gfxworld::ExposureGridParts]) -> Self {
        let default = f32_at(image, sun_volume + 0x830 + 0x10 * state.min(3) + 4).unwrap_or(0.0);
        let probes: Vec<(usize, fastfile_t7::walk::gfxworld::ExposureGridParts)> = grids.map_or(Vec::new(), |(at, n)| (0..n).zip(parts).map(|(i, p)| (at + 0x50 * i, p.clone())).collect());
        // (What [`Self::meter`] reads, as it reads it: the zone is let go
        // once the stages are made.)
        let mut spans = Vec::new();
        for (g, p) in &probes {
            spans.push((*g, 0x50));
            let nb = u16_at(image, g + 0x38).unwrap_or(0) as usize;
            if let Some(b) = p.blends {
                spans.push((b, 0x50usize.saturating_mul(nb)));
                for (j, fa) in p.faces.iter().enumerate().take(nb) {
                    let faces = i32_at(image, b + 0x50 * j + 0x3c).unwrap_or(0).max(0) as usize;
                    spans.extend(fa.map(|fa| (fa, 16usize.saturating_mul(faces))));
                }
            }
            if let Some(v) = p.voxels {
                // (Six planes of half floats; a grid with no voxels still
                // reads its first.)
                let total = [0x28, 0x2a, 0x2c].iter().map(|&o| u16_at(image, g + o).unwrap_or(1) as usize).product::<usize>();
                spans.push((v, 12usize.saturating_mul(total).max(2)));
            }
        }
        let image = fastfile_t7::kept::KeptBytes::new(image, spans);
        let probes = probes
            .into_iter()
            .map(|(g, p)| {
                let parts = fastfile_t7::walk::gfxworld::ExposureGridParts {
                    voxels: p.voxels.map(|v| image.rebase(v)),
                    blends: p.blends.map(|b| image.rebase(b)),
                    faces: p.faces.iter().map(|f| f.map(|f| image.rebase(f))).collect(),
                };
                (image.rebase(g), parts)
            })
            .collect();
        Self { image, probes, default }
    }

    /// (For the tests: the grids over the whole zone, as they read it
    /// before the zone was let go.)
    #[cfg(test)]
    pub(crate) fn over_whole_zone(image: &[u8], sun_volume: usize, state: usize, grids: fastfile_t7::walk::world::Span, parts: &[fastfile_t7::walk::gfxworld::ExposureGridParts]) -> Self {
        let default = f32_at(image, sun_volume + 0x830 + 0x10 * state.min(3) + 4).unwrap_or(0.0);
        let probes = grids.map_or(Vec::new(), |(at, n)| (0..n).zip(parts).map(|(i, p)| (at + 0x50 * i, p.clone())).collect());
        Self { image: fastfile_t7::kept::KeptBytes::new(image, [(0, image.len())]), probes, default }
    }

    fn f(&self, at: usize) -> f32 {
        f32_at(self.image.bytes(), at).unwrap_or(0.0)
    }
    fn v3(&self, at: usize) -> [f32; 3] {
        [self.f(at), self.f(at + 4), self.f(at + 8)]
    }

    /// The metered luminance at `eye` looking along `dir`.
    pub fn meter(&self, eye: [f32; 3], dir: [f32; 3]) -> f32 {
        let one = 1.0f32;
        let (mut aw, mut av, mut bw, mut bv) = (0f32, 0f32, 0f32, 0f32);
        for (g, parts) in &self.probes {
            let g = *g;
            let o = self.v3(g);
            let h = self.v3(g + 0xc);
            let m = angles_to_axis(self.v3(g + 0x18));
            let d = [eye[0] - o[0], eye[1] - o[1], eye[2] - o[2]];
            // (The exe's grid transform: local = d · M, rows forward, left, up.)
            let l: [f32; 3] = std::array::from_fn(|k| m[0][k] * d[0] + m[1][k] * d[1] + m[2][k] * d[2]);
            let u: [f32; 3] = std::array::from_fn(|k| ((h[k] + l[k]) / h[k]) * 0.5);
            if !u.iter().all(|&x| (0.0..=one).contains(&x)) {
                continue;
            }
            let (mut inc, mut exc) = (0f32, 0f32);
            let nb = u16_at(self.image.bytes(), g + 0x38).unwrap_or(0) as usize;
            for j in 0..nb {
                let Some(b) = parts.blends.map(|b| b + 0x50 * j) else { break };
                let bo = self.v3(b);
                let bm = angles_to_axis(self.v3(b + 0x30));
                let d = [eye[0] - bo[0], eye[1] - bo[1], eye[2] - bo[2]];
                // (Blends use the transposed product: local = M · d.)
                let bl: [f32; 3] = std::array::from_fn(|k| bm[k][0] * d[0] + bm[k][1] * d[1] + bm[k][2] * d[2]);
                let faces = i32_at(self.image.bytes(), b + 0x3c).unwrap_or(0).max(0) as usize;
                let w = if faces == 0 {
                    let he = self.v3(b + 0xc);
                    let up = self.v3(b + 0x24);
                    let lo = self.v3(b + 0x18);
                    let c01 = |x: f32| x.clamp(0.0, 1.0);
                    let mut w = 1.0f32;
                    for k in 0..3 {
                        let p = c01((bl[k] - he[k]) / up[k].max(0.1));
                        let n = c01((-bl[k] - he[k]) / lo[k].max(0.1));
                        w *= (one - (p + n).abs()).clamp(0.0, 1.0);
                    }
                    w
                } else {
                    let mut w = one;
                    if let Some(Some(fa)) = parts.faces.get(j) {
                        for q in 0..faces {
                            let p = self.v3(fa + 16 * q);
                            let pw = self.f(fa + 16 * q + 12);
                            w *= (p[0] * bl[0] + p[1] * bl[1] + p[2] * bl[2] + pw).clamp(0.0, 1.0);
                        }
                    }
                    w
                };
                if self.image.bytes().get(b + 0x48).copied().unwrap_or(0) == 0 {
                    inc = inc.max(w);
                } else {
                    exc = exc.max(w);
                }
            }
            let w = inc * (one - exc);
            if w <= 0.0 {
                continue;
            }
            let Some(vox) = parts.voxels else { continue };
            let dims = [u16_at(self.image.bytes(), g + 0x28).unwrap_or(1), u16_at(self.image.bytes(), g + 0x2a).unwrap_or(1), u16_at(self.image.bytes(), g + 0x2c).unwrap_or(1)];
            let total = dims.iter().map(|&x| x as usize).product::<usize>();
            let dr: [f32; 3] = std::array::from_fn(|k| m[0][k] * dir[0] + m[1][k] * dir[1] + m[2][k] * dir[2]);
            let sel = |x: f32| if x < 0.0 { -1.0 } else if x == 0.0 { 0.0 } else { 1.0 };
            let signs = [sel(dr[0]), sel(dr[1]), sel(dr[2])];
            let sample = |x: i32, y: i32, z: i32| -> f32 {
                let c = |v: i32, n: u16| v.min(i32::from(n) - 1).max(0) as usize;
                let idx = (c(z, dims[2]) * dims[1] as usize + c(y, dims[1])) * dims[0] as usize + c(x, dims[0]);
                let comp = |k: usize, pos: bool| -> f32 {
                    let plane = if pos { k } else { k + 3 };
                    u16_at(self.image.bytes(), vox + (plane * total + idx) * 2).map_or(0.0, half)
                };
                dr[0] * dr[0] * comp(0, 0.0 < signs[0]) + dr[1] * dr[1] * comp(1, 0.0 < signs[1]) + dr[2] * dr[2] * comp(2, 0.0 < signs[2])
            };
            let fx = (i32::from(dims[0]) - 1) as f32 * u[0];
            let fy = (i32::from(dims[1]) - 1) as f32 * u[1];
            let fz = (i32::from(dims[2]) - 1) as f32 * u[2];
            let (ix, iy, iz) = (fx as i32, fy as i32, fz as i32);
            let (ax, ay, az) = (fx - ix as f32, fy - iy as f32, fz - iz as f32);
            let (bx, by) = (one - ax, one - ay);
            let lo_z = (sample(ix + 1, iy + 1, iz) * ax + sample(ix, iy + 1, iz) * bx) * ay + (sample(ix + 1, iy, iz) * ax + sample(ix, iy, iz) * bx) * by;
            let hi_z = (sample(ix + 1, iy, iz + 1) * ax + sample(ix, iy, iz + 1) * bx) * by + (sample(ix, iy + 1, iz + 1) * bx + sample(ix + 1, iy + 1, iz + 1) * ax) * ay;
            let v = (lo_z * (one - az) + hi_z * az) * self.f(g + 0x24) * w;
            if self.image.bytes().get(g + 0x48).copied().unwrap_or(0) == 0 {
                aw += w;
                av += v;
            } else {
                bw += w;
                bv += v;
            }
        }
        let b = bw.clamp(0.0, 1.0);
        let v = (one - b) * av + bv;
        let w = (one - b) * aw + bw;
        if one <= w { v / w } else { v + (one - w) * self.default }
    }
}
