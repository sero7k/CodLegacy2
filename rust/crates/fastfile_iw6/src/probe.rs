//! bo2mp: a Ghosts map's reflection probes as plain data: each probe's
//! capture position, its influence volumes and its cube's texels, and the
//! reference positions that name a probe (`walk::GfxWorldRec::reflection_probes`).
//!
//! **Cube texels.** A probe's image is a cube (map type 5) stored inline in
//! the zone: six faces one after another, each with its whole mip chain
//! largest first (mp_frag: each face's top level is a coherent picture under
//! this order; the smaller levels are blurred more than a 2x2 mean, 17 to
//! 42 % from it, so they are prefiltered for rough surfaces). Formats seen:
//! DXGI 10 (`R16G16B16A16_FLOAT`, linear light) for the captured probes and
//! DXGI 28 (`R8G8B8A8_UNORM`) for probe 0, the one at the world origin,
//! which is a flat 1.0 everywhere (a neutral default). The half-float alpha
//! is 1 on the top level and 0.43..0.57 on the others, the same on every
//! probe and face whatever the picture (meaning unknown; not used). Face
//! order and orientation are D3D's (+X, -X, +Y, -Y, +Z, -Z) sampled with the
//! world direction itself, z up: on mp_frag +Z is the sky, -Z the snow, and
//! a side face's top row looks up for -Y and down for +Y as D3D addresses
//! them.
//!
//! **Volumes.** 96 bytes each: six planes `(normal, d)`; a point is inside
//! when `normal . p + d >= 0` for every plane (mp_frag: probe 8's box holds
//! its own origin under this sign and is empty under the other; some
//! volumes carry a slanted plane). A volume need not hold its probe's
//! origin (probes 13, 14, 20 on mp_frag): it marks the region that probe
//! serves.
//!
//! **Which probe.** World surfaces carry their probe index in the zone
//! (`GfxSurface`'s lighting fields), and placed models in theirs
//! (`GfxStaticModelDrawInst::reflectionProbeIndex`); those are used as
//! stored. For anything else (players, guns, script models) the game's own
//! selection was not found; a stand-in, measured against the stored indices
//! on mp_frag: the stored index of the nearest placed model or world surface
//! ([`NearestSamples`]; the nearest other placed model's index is the
//! model's own for 92 % of the 8779 placed models), else
//! [`ProbeSet::pick`]: the tightest volume holding the point, else the
//! nearest probe origin other than probe 0 (that geometric rule agrees with
//! 50 % of the models' stored indices; the nearest reference position's
//! probe with 12 %, so the references are kept as data but not used).

use crate::layout as L;
use crate::walk::{AssetId, Capture, ImageRec};

/// One reflection probe.
#[derive(Clone, Debug, Default)]
pub struct ReflectionProbe {
    pub origin: [f32; 3],
    /// The record's fourth origin float (0 on every probe seen).
    pub origin_w: f32,
    pub image: Option<AssetId>,
    /// Six planes `(nx, ny, nz, d)` per volume.
    pub volumes: Vec<[[f32; 4]; 6]>,
}

/// A map's probes and the reference positions that name one.
#[derive(Clone, Debug, Default)]
pub struct ProbeSet {
    pub probes: Vec<ReflectionProbe>,
    pub references: Vec<([f32; 3], u8)>,
}

fn f32_at(image: &[u8], at: usize) -> Option<f32> {
    Some(f32::from_le_bytes(image.get(at..at + 4)?.try_into().ok()?))
}

/// The probes of the zone's first GfxWorld.
pub fn read_probes(cap: &Capture, image: &[u8]) -> ProbeSet {
    let mut out = ProbeSet::default();
    let Some(g) = cap.gfx_worlds.first() else {
        return out;
    };
    let vs = L::GfxReflectionProbeVolumeData::SIZE;
    for p in &g.reflection_probes {
        let mut volumes = Vec::new();
        if let Some(at) = p.volumes {
            for k in 0..p.volume_count as usize {
                let base = at + vs * k + L::GfxReflectionProbeVolumeData::volumePlanes;
                let mut planes = [[0f32; 4]; 6];
                let mut ok = true;
                for (j, plane) in planes.iter_mut().enumerate() {
                    for (c, v) in plane.iter_mut().enumerate() {
                        match f32_at(image, base + 16 * j + 4 * c) {
                            Some(x) if x.is_finite() => *v = x,
                            _ => ok = false,
                        }
                    }
                }
                if ok {
                    volumes.push(planes);
                }
            }
        }
        out.probes.push(ReflectionProbe {
            origin: [p.origin[0], p.origin[1], p.origin[2]],
            origin_w: p.origin[3],
            image: p.image,
            volumes,
        });
    }
    if let (Some(o), Some(r)) = (
        g.reflection_probe_reference_origins,
        g.reflection_probe_references,
    ) {
        for i in 0..g.reflection_probe_reference_count as usize {
            let at = o + L::GfxReflectionProbeReferenceOrigin::SIZE * i;
            let (Some(x), Some(y), Some(z)) = (
                f32_at(image, at),
                f32_at(image, at + 4),
                f32_at(image, at + 8),
            ) else {
                break;
            };
            let Some(&probe) = image.get(r + L::GfxReflectionProbeReference::SIZE * i) else {
                break;
            };
            out.references.push(([x, y, z], probe));
        }
    }
    out
}

/// Whether `p` is inside a volume's six planes.
pub fn volume_contains(planes: &[[f32; 4]; 6], p: [f32; 3]) -> bool {
    planes
        .iter()
        .all(|n| n[0] * p[0] + n[1] * p[1] + n[2] * p[2] + n[3] >= 0.0)
}

/// A rough size of a volume (the product of its three axis-plane spans
/// where it has them), to prefer the tighter of two volumes holding a point.
fn volume_size(planes: &[[f32; 4]; 6]) -> f32 {
    let mut lo = [f32::NEG_INFINITY; 3];
    let mut hi = [f32::INFINITY; 3];
    for n in planes {
        for k in 0..3 {
            let others = (0..3).filter(|&j| j != k).all(|j| n[j] == 0.0);
            if others && n[k] > 0.0 {
                lo[k] = lo[k].max(-n[3] / n[k]);
            } else if others && n[k] < 0.0 {
                hi[k] = hi[k].min(n[3] / -n[k]);
            }
        }
    }
    (0..3)
        .map(|k| {
            if hi[k] > lo[k] && (hi[k] - lo[k]).is_finite() {
                hi[k] - lo[k]
            } else {
                1.0e5
            }
        })
        .product()
}

fn dist2(a: [f32; 3], b: [f32; 3]) -> f32 {
    (0..3).map(|k| (a[k] - b[k]) * (a[k] - b[k])).sum()
}

/// How [`ProbeSet::pick`] chose.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PickedBy {
    Volume,
    Nearest,
}

impl ProbeSet {
    /// The probe for a point without a stored index (see the module notes:
    /// a stand-in rule). `None` when the map has no probes.
    pub fn pick(&self, p: [f32; 3]) -> Option<(usize, PickedBy)> {
        let n = self.probes.len();
        if n == 0 {
            return None;
        }
        let in_volume = self
            .probes
            .iter()
            .enumerate()
            .flat_map(|(i, pr)| pr.volumes.iter().map(move |v| (i, v)))
            .filter(|(_, v)| volume_contains(v, p))
            .min_by(|a, b| volume_size(a.1).total_cmp(&volume_size(b.1)));
        if let Some((i, _)) = in_volume {
            return Some((i, PickedBy::Volume));
        }
        self.probes
            .iter()
            .enumerate()
            .skip(usize::from(n > 1))
            .min_by(|a, b| dist2(a.1.origin, p).total_cmp(&dist2(b.1.origin, p)))
            .map(|(i, _)| (i, PickedBy::Nearest))
    }
}

/// Points that carry a stored probe index (placed models, world surface
/// centres), bucketed for a nearest-point query.
#[derive(Clone, Debug, Default)]
pub struct NearestSamples {
    lo: [f32; 3],
    cell: f32,
    dims: [usize; 3],
    buckets: Vec<Vec<([f32; 3], u8)>>,
}

impl NearestSamples {
    /// Buckets `samples` in cubes of `cell` units (at most 64 a side).
    pub fn new(samples: &[([f32; 3], u8)], cell: f32) -> Self {
        let samples: Vec<_> = samples
            .iter()
            .copied()
            .filter(|s| s.0.iter().all(|v| v.is_finite()))
            .collect();
        if samples.is_empty() {
            return Self::default();
        }
        let mut lo = [f32::INFINITY; 3];
        let mut hi = [f32::NEG_INFINITY; 3];
        for s in &samples {
            for k in 0..3 {
                lo[k] = lo[k].min(s.0[k]);
                hi[k] = hi[k].max(s.0[k]);
            }
        }
        let span = (0..3).map(|k| hi[k] - lo[k]).fold(0f32, f32::max);
        let cell = cell.max(span / 64.0).max(1.0);
        let dims: [usize; 3] = std::array::from_fn(|k| ((hi[k] - lo[k]) / cell) as usize + 1);
        let mut buckets = vec![Vec::new(); dims[0] * dims[1] * dims[2]];
        let mut out = Self {
            lo,
            cell,
            dims,
            buckets: Vec::new(),
        };
        for s in samples {
            let c = out.cell_of(s.0);
            buckets[out.index(c)].push(s);
        }
        out.buckets = buckets;
        out
    }

    fn cell_of(&self, p: [f32; 3]) -> [usize; 3] {
        std::array::from_fn(|k| {
            (((p[k] - self.lo[k]) / self.cell).max(0.0) as usize).min(self.dims[k] - 1)
        })
    }

    fn index(&self, c: [usize; 3]) -> usize {
        (c[2] * self.dims[1] + c[1]) * self.dims[0] + c[0]
    }

    pub fn is_empty(&self) -> bool {
        self.buckets.is_empty()
    }

    /// The stored index of the sample nearest `p` (a point outside the
    /// samples' box is brought to its edge first).
    pub fn nearest(&self, p: [f32; 3]) -> Option<u8> {
        if self.buckets.is_empty() {
            return None;
        }
        let q: [f32; 3] = std::array::from_fn(|k| {
            p[k].clamp(self.lo[k], self.lo[k] + self.cell * self.dims[k] as f32)
        });
        let c = self.cell_of(q);
        let mut best: Option<(f32, u8)> = None;
        let max_r = self.dims.iter().copied().max().unwrap_or(1);
        for r in 0..=max_r {
            // Every sample in ring r is at least (r - 1) cells away.
            if let Some((d, _)) = best {
                let reach = (r as f32 - 1.0).max(0.0) * self.cell;
                if reach * reach > d {
                    break;
                }
            }
            let lo: [isize; 3] = std::array::from_fn(|k| c[k] as isize - r as isize);
            let hi: [isize; 3] = std::array::from_fn(|k| c[k] as isize + r as isize);
            for z in lo[2].max(0)..=hi[2].min(self.dims[2] as isize - 1) {
                for y in lo[1].max(0)..=hi[1].min(self.dims[1] as isize - 1) {
                    for x in lo[0].max(0)..=hi[0].min(self.dims[0] as isize - 1) {
                        let on_ring = [x, y, z]
                            .iter()
                            .zip(lo.iter().zip(hi.iter()))
                            .any(|(v, (a, b))| v == a || v == b);
                        if !on_ring {
                            continue;
                        }
                        for s in &self.buckets[self.index([x as usize, y as usize, z as usize])] {
                            let d = dist2(s.0, q);
                            if best.is_none_or(|b| d < b.0) {
                                best = Some((d, s.1));
                            }
                        }
                    }
                }
            }
        }
        best.map(|b| b.1)
    }
}

/// A probe's cube: `size` on a side, `levels` levels; texels linear RGB,
/// face by face (+X, -X, +Y, -Y, +Z, -Z), each face's levels largest first.
#[derive(Clone, Debug, Default)]
pub struct ProbeCube {
    pub size: u32,
    pub levels: u32,
    pub dxgi_format: u32,
    pub texels: Vec<[f32; 3]>,
}

impl ProbeCube {
    /// Texels in one face (all its levels).
    pub fn face_texels(&self) -> usize {
        (0..self.levels)
            .map(|k| ((self.size >> k).max(1) as usize).pow(2))
            .sum()
    }

    /// The texels of one face's level (row-major, `size >> level` wide).
    pub fn level(&self, face: usize, level: u32) -> &[[f32; 3]] {
        let before: usize = (0..level)
            .map(|k| ((self.size >> k).max(1) as usize).pow(2))
            .sum();
        let d = (self.size >> level).max(1) as usize;
        let at = face * self.face_texels() + before;
        self.texels.get(at..at + d * d).unwrap_or(&[])
    }
}

/// IEEE half to f32.
fn half(h: u16) -> f32 {
    crate::map::half_to_f32(h)
}

/// Decodes a probe image's cube (inline pixels only; probes are not
/// streamed).
pub fn probe_cube(rec: &ImageRec, zone_image: &[u8]) -> Result<ProbeCube, String> {
    if rec.map_type != 5 {
        return Err(format!(
            "{}: map type {} is not a cube",
            rec.name, rec.map_type
        ));
    }
    if rec.width != rec.height || rec.width == 0 {
        return Err(format!(
            "{}: {}x{} is not a square cube",
            rec.name, rec.width, rec.height
        ));
    }
    let size = u32::from(rec.width);
    let levels = u32::from(rec.level_count.max(1));
    let bpp = match rec.format {
        10 => 8,
        27..=29 => 4,
        f => {
            return Err(format!(
                "{}: DXGI format {f} is not read for a probe",
                rec.name
            ));
        }
    };
    let span = rec
        .pixels
        .ok_or_else(|| format!("{}: no pixels in the zone", rec.name))?;
    let raw = span.get(zone_image);
    let per_face: usize = (0..levels)
        .map(|k| ((size >> k).max(1) as usize).pow(2))
        .sum();
    let want = per_face * 6 * bpp;
    let raw = raw.get(..want).ok_or_else(|| {
        format!(
            "{}: {} bytes, {want} wanted for six faces of {levels} levels",
            rec.name,
            raw.len()
        )
    })?;
    let srgb = rec.format == 29;
    let texels = raw
        .chunks_exact(bpp)
        .map(|t| {
            if bpp == 8 {
                [0, 1, 2].map(|k| {
                    let v = half(u16::from_le_bytes([t[2 * k], t[2 * k + 1]]));
                    if v.is_finite() { v.max(0.0) } else { 0.0 }
                })
            } else {
                [0, 1, 2].map(|k| {
                    let x = f32::from(t[k]) / 255.0;
                    if !srgb {
                        x
                    } else if x <= 0.04045 {
                        x / 12.92
                    } else {
                        ((x + 0.055) / 1.055).powf(2.4)
                    }
                })
            }
        })
        .collect();
    Ok(ProbeCube {
        size,
        levels,
        dxgi_format: rec.format,
        texels,
    })
}

/// Mean luminance over the sphere of a cube's top level (each texel
/// weighted by its solid angle).
pub fn mean_luminance(cube: &ProbeCube) -> f32 {
    let n = cube.size as usize;
    let (mut sum, mut weight) = (0.0f64, 0.0f64);
    for face in 0..6 {
        let level = cube.level(face, 0);
        if level.len() != n * n {
            return 0.0;
        }
        for y in 0..n {
            for x in 0..n {
                let (sc, tc) = (
                    (x as f32 + 0.5) / n as f32 * 2.0 - 1.0,
                    (y as f32 + 0.5) / n as f32 * 2.0 - 1.0,
                );
                let w = f64::from((1.0 + sc * sc + tc * tc).powf(-1.5));
                let c = level[y * n + x];
                sum += w * f64::from(0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2]);
                weight += w;
            }
        }
    }
    (sum / weight.max(1e-9)) as f32
}

#[cfg(test)]
mod tests {
    use super::*;

    fn boxed(lo: [f32; 3], hi: [f32; 3]) -> [[f32; 4]; 6] {
        [
            [-1.0, 0.0, 0.0, hi[0]],
            [1.0, 0.0, 0.0, -lo[0]],
            [0.0, -1.0, 0.0, hi[1]],
            [0.0, 1.0, 0.0, -lo[1]],
            [0.0, 0.0, -1.0, hi[2]],
            [0.0, 0.0, 1.0, -lo[2]],
        ]
    }

    #[test]
    fn a_volume_holds_what_is_inside_its_planes() {
        // mp_frag's probe 8 box, as stored.
        let v = [
            [-1.0, 0.0, 0.0, 1208.0],
            [1.0, 0.0, 0.0, -776.0],
            [0.0, -1.0, 0.0, 1928.5],
            [0.0, 1.0, 0.0, -1297.5],
            [0.0, 0.0, -1.0, 920.0],
            [0.0, 0.0, 1.0, -128.0],
        ];
        assert!(volume_contains(&v, [992.0, 1600.0, 200.0]));
        assert!(!volume_contains(&v, [700.0, 1600.0, 200.0]));
        assert!(!volume_contains(&v, [992.0, 1600.0, 1000.0]));
        assert!((volume_size(&v) - 432.0 * 631.0 * 792.0).abs() < 1.0);
    }

    #[test]
    fn pick_takes_the_tightest_volume_then_the_nearest() {
        let set = ProbeSet {
            probes: vec![
                ReflectionProbe::default(),
                ReflectionProbe {
                    origin: [100.0, 0.0, 0.0],
                    volumes: vec![boxed([-1000.0; 3], [1000.0; 3])],
                    ..Default::default()
                },
                ReflectionProbe {
                    origin: [900.0, 0.0, 0.0],
                    volumes: vec![boxed([-10.0; 3], [10.0; 3])],
                    ..Default::default()
                },
                ReflectionProbe {
                    origin: [5000.0, 0.0, 0.0],
                    ..Default::default()
                },
            ],
            references: vec![([4000.0, 0.0, 0.0], 1)],
        };
        assert_eq!(set.pick([0.0; 3]), Some((2, PickedBy::Volume)));
        assert_eq!(set.pick([500.0, 0.0, 0.0]), Some((1, PickedBy::Volume)));
        // (References are not used: nearest origin.)
        assert_eq!(set.pick([3000.0, 0.0, 0.0]), Some((3, PickedBy::Nearest)));
        // Probe 0 (at the origin) is never the nearest while another exists.
        assert_eq!(set.pick([0.0, 0.0, 5000.0]).map(|p| p.0), Some(1));
        assert_eq!(ProbeSet::default().pick([0.0; 3]), None);
    }

    #[test]
    fn nearest_samples_find_the_closest_stored_index() {
        let mut pts = Vec::new();
        let mut seed = 12345u32;
        let mut rnd = || {
            seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
            (seed >> 8) as f32 / (1 << 24) as f32 * 4000.0 - 2000.0
        };
        for i in 0..500 {
            pts.push(([rnd(), rnd(), rnd() * 0.2], (i % 7) as u8));
        }
        let ns = NearestSamples::new(&pts, 128.0);
        for _ in 0..200 {
            let p = [rnd() * 1.2, rnd() * 1.2, rnd() * 0.3];
            let q: [f32; 3] = std::array::from_fn(|k| {
                p[k].clamp(ns.lo[k], ns.lo[k] + ns.cell * ns.dims[k] as f32)
            });
            let want = pts
                .iter()
                .min_by(|a, b| dist2(a.0, q).total_cmp(&dist2(b.0, q)))
                .unwrap();
            let got = ns.nearest(p).unwrap();
            let best = pts
                .iter()
                .filter(|s| s.1 == got)
                .map(|s| dist2(s.0, q))
                .fold(f32::INFINITY, f32::min);
            assert!((best - dist2(want.0, q)).abs() < 1e-2, "{p:?}");
        }
        assert_eq!(NearestSamples::new(&[], 64.0).nearest([0.0; 3]), None);
    }

    #[test]
    fn half_float_cube_reads_face_by_face() {
        let mut rec = ImageRec {
            name: "*reflection_probe1".into(),
            format: 10,
            map_type: 5,
            width: 2,
            height: 2,
            level_count: 2,
            ..Default::default()
        };
        // Six faces of (2x2 + 1x1) texels; face f's texels all f + 1.
        let mut image = Vec::new();
        for f in 0..6u16 {
            for _ in 0..5 {
                let h = [0x3c00u16, 0x4000, 0x4200, 0x4400, 0x4500, 0x4600][f as usize];
                for _ in 0..3 {
                    image.extend_from_slice(&h.to_le_bytes());
                }
                image.extend_from_slice(&0x3c00u16.to_le_bytes());
            }
        }
        rec.pixels = Some(crate::walk::Span {
            pos: 0,
            len: image.len(),
        });
        let cube = probe_cube(&rec, &image).unwrap();
        assert_eq!(cube.face_texels(), 5);
        assert_eq!(cube.level(0, 0), &[[1.0; 3]; 4]);
        assert_eq!(cube.level(3, 1), &[[4.0; 3]]);
        assert_eq!(cube.level(5, 0)[3], [6.0; 3]);
        assert!(mean_luminance(&cube) > 1.0);
        rec.format = 95;
        assert!(probe_cube(&rec, &image).is_err());
    }
}
