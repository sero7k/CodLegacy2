//! Ambient light for places that have no reflection probe, and stand-in
//! colours for materials that have no picture.
//!
//! Only a few of a map's reflection probes can be read (the others live in
//! streamed packages that are not always present: Highrise 6 of 56, Shipment
//! 3 of 22, Farm 18 4 of 107), and the ones that load differ from each other
//! by a hundred times (a probe beside a lamp reads 300, one in a corner
//! 0.3). Taken as they are, the nearest one lit the whole map: Shipment came
//! out pale white, Farm 18 flat grey. Three parts replace that:
//!
//! - [`sky_light`]: the ambient light of the open sky as the map's sky cube
//!   (or the vision set's gradient) and the sun give it, one colour each for a
//!   surface facing up, sideways and down. This sets the map's overall level.
//! - [`HeightGrid`]: the highest geometry in each cell of the ground plan. The
//!   lighting pass marches it in eight directions to find how much sky a point
//!   sees (a street between towers is darker than a roof), so the ambient
//!   differs from place to place with no probe at all.
//! - [`normalise_probes`]: the probes that did load keep their direction
//!   and colour, but their level is moved to the sky's, and they apply only
//!   near themselves (`Env::probe_reach`).

use crate::scene::Probe;
use crate::sky::SkyCube;

/// The ambient light of a surface facing up, sideways (the mean of four
/// directions) and down, in the units the lighting pass uses (a surface of
/// albedo 1 under it shows this colour).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct SkyLight {
    pub up: [f32; 3],
    pub side: [f32; 3],
    pub down: [f32; 3],
}

fn lum(c: [f32; 3]) -> f32 {
    0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2]
}

fn srgb_to_linear(v: u8) -> f32 {
    let c = f32::from(v) / 255.0;
    if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) }
}

/// The sky cube's colour along a direction (z up): the cube's faces are written
/// for the world direction itself (`sky::build`), so this is its inverse.
#[must_use]
pub fn cube_lookup(cube: &SkyCube, d: [f32; 3]) -> [f32; 3] {
    let (ax, ay, az) = (d[0].abs(), d[1].abs(), d[2].abs());
    let (face, s, t) = if ax >= ay && ax >= az {
        if d[0] > 0.0 { (0usize, -d[2] / ax, -d[1] / ax) } else { (1, d[2] / ax, -d[1] / ax) }
    } else if ay >= az {
        if d[1] > 0.0 { (2, d[0] / ay, d[2] / ay) } else { (3, d[0] / ay, -d[2] / ay) }
    } else if d[2] > 0.0 {
        (4, d[0] / az, -d[1] / az)
    } else {
        (5, -d[0] / az, -d[1] / az)
    };
    let n = cube.size as usize;
    let px = (((s + 1.0) * 0.5 * n as f32) as isize).clamp(0, n as isize - 1) as usize;
    let py = (((t + 1.0) * 0.5 * n as f32) as isize).clamp(0, n as isize - 1) as usize;
    let at = ((face * n + py) * n + px) * 4;
    match cube.rgba.get(at..at + 3) {
        Some(p) => [srgb_to_linear(p[0]), srgb_to_linear(p[1]), srgb_to_linear(p[2])],
        None => [0.0; 3],
    }
}

/// The map's gradient sky (as the lighting pass draws it without a cube).
#[must_use]
pub fn gradient_sky(horizon: [f32; 3], zenith: [f32; 3], d: [f32; 3]) -> [f32; 3] {
    let up = d[2].clamp(0.0, 1.0).powf(0.45);
    std::array::from_fn(|k| horizon[k] + (zenith[k] - horizon[k]) * up)
}

/// Directions spread evenly over the sphere (a Fibonacci spiral).
fn sphere(n: usize) -> Vec<[f32; 3]> {
    let golden = std::f32::consts::PI * (3.0 - 5.0f32.sqrt());
    (0..n)
        .map(|i| {
            let z = 1.0 - 2.0 * (i as f32 + 0.5) / n as f32;
            let r = (1.0 - z * z).max(0.0).sqrt();
            let phi = golden * i as f32;
            [r * phi.cos(), r * phi.sin(), z]
        })
        .collect()
}

/// The ambient light from a sky and a ground: `sky` gives the radiance of a
/// direction above the horizon, `ground` the radiance of everything below it.
/// Each result is the cosine weighted mean over the sphere about its normal.
#[must_use]
pub fn sky_light(sky: &dyn Fn([f32; 3]) -> [f32; 3], ground: [f32; 3]) -> SkyLight {
    let dirs = sphere(768);
    let rad: Vec<[f32; 3]> = dirs.iter().map(|d| if d[2] >= 0.0 { sky(*d) } else { ground }).collect();
    let mean = |n: [f32; 3]| -> [f32; 3] {
        let (mut acc, mut w) = ([0.0f32; 3], 0.0f32);
        for (d, l) in dirs.iter().zip(&rad) {
            let c = (d[0] * n[0] + d[1] * n[1] + d[2] * n[2]).max(0.0);
            for k in 0..3 {
                acc[k] += l[k] * c;
            }
            w += c;
        }
        acc.map(|v| v / w.max(1e-6))
    };
    let sides = [[1.0, 0.0, 0.0], [-1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, -1.0, 0.0]].map(mean);
    SkyLight {
        up: mean([0.0, 0.0, 1.0]),
        down: mean([0.0, 0.0, -1.0]),
        side: std::array::from_fn(|k| sides.iter().map(|s| s[k]).sum::<f32>() / 4.0),
    }
}

/// The bounce off the ground: a grey of albedo 0.28 lit by the sun and the sky above.
#[must_use]
pub fn ground_bounce(sun_dir_z: f32, sun_colour: [f32; 3], sky_up: [f32; 3]) -> [f32; 3] {
    let sz = sun_dir_z.clamp(0.1, 1.0);
    std::array::from_fn(|k| 0.28 * (sun_colour[k] * sz + sky_up[k]))
}

/// Moves the probes' level to the sky's: the probes' own scale is the median
/// of their mean luminance, so a map whose probes all read dim or bright keeps
/// the differences between its probes (an interior against a street), then each
/// probe is held to between a third and twice the sky's level. The radiance
/// pictures follow the same scale (and stay under 3).
pub fn normalise_probes(probes: &mut [Probe], level: f32) {
    if probes.is_empty() || level <= 1e-5 {
        return;
    }
    let mean_lum = |p: &Probe| lum([0, 1, 2].map(|k| (p.up[k] + p.side[k] + p.down[k]) / 3.0));
    let mut lums: Vec<f32> = probes.iter().map(mean_lum).collect();
    lums.sort_by(f32::total_cmp);
    let median = lums[lums.len() / 2].max(1e-4);
    let k = level / median;
    for p in probes.iter_mut() {
        let own = mean_lum(p) * k;
        let held = own.clamp(level / 3.0, level * 2.0);
        let scale = if own > 1e-6 { k * held / own } else { k };
        for v in [&mut p.up, &mut p.side, &mut p.down] {
            *v = v.map(|c| c * scale);
        }
        for r in &mut p.radiance {
            *r = r.map(|c| (c * scale).min(3.0));
        }
    }
}

/// A stand-in albedo for a material with no colour picture: a muted palette of
/// the greys, sands and browns a map's surfaces are, picked by the material's
/// hash so neighbouring materials differ.
#[must_use]
pub fn fallback_tint(hash: u64) -> [f32; 3] {
    const PALETTE: [[f32; 3]; 10] = [
        [0.30, 0.29, 0.27], // pale concrete
        [0.20, 0.20, 0.20], // grey concrete
        [0.34, 0.28, 0.20], // sand
        [0.27, 0.16, 0.12], // brick
        [0.10, 0.10, 0.11], // asphalt
        [0.19, 0.22, 0.26], // painted blue-grey
        [0.20, 0.20, 0.14], // dust and olive
        [0.24, 0.25, 0.26], // metal
        [0.26, 0.24, 0.20], // plaster
        [0.15, 0.17, 0.15], // weathered green-grey
    ];
    let mut h = hash ^ (hash >> 29);
    h = h.wrapping_mul(0xbf58_476d_1ce4_e5b9);
    h ^= h >> 32;
    let base = PALETTE[(h % PALETTE.len() as u64) as usize];
    let jitter = 0.8 + 0.4 * ((h >> 8) & 0xff) as f32 / 255.0;
    base.map(|c| c * jitter)
}

/// The highest geometry in each cell of the ground plan (row major, `cols` by
/// `rows`, cell `cell` units, the corner of cell 0 at `origin`); [`NO_HEIGHT`]
/// where nothing stands.
#[derive(Clone, Debug)]
pub struct HeightGrid {
    pub origin: [f32; 2],
    pub cell: f32,
    pub cols: u32,
    pub rows: u32,
    pub heights: Vec<f32>,
}

/// A cell nothing stands in.
pub const NO_HEIGHT: f32 = -1.0e9;

impl HeightGrid {
    /// A grid over `lo..hi` with cells of at least `min_cell` and at most `max_side` to a side.
    #[must_use]
    pub fn new(lo: [f32; 2], hi: [f32; 2], min_cell: f32, max_side: u32) -> Self {
        let span = (hi[0] - lo[0]).max(hi[1] - lo[1]).max(1.0);
        let cell = (span / max_side as f32).max(min_cell);
        let cols = (((hi[0] - lo[0]) / cell).ceil() as u32).clamp(1, max_side);
        let rows = (((hi[1] - lo[1]) / cell).ceil() as u32).clamp(1, max_side);
        Self { origin: lo, cell, cols, rows, heights: vec![NO_HEIGHT; (cols * rows) as usize] }
    }

    fn cell_of(&self, x: f32, y: f32) -> Option<(usize, usize)> {
        let (fx, fy) = ((x - self.origin[0]) / self.cell, (y - self.origin[1]) / self.cell);
        if fx < 0.0 || fy < 0.0 || fx >= self.cols as f32 || fy >= self.rows as f32 {
            return None;
        }
        Some((fx as usize, fy as usize))
    }

    /// Raises the cell under a point to its height.
    pub fn splat(&mut self, p: [f32; 3]) {
        if let Some((ix, iy)) = self.cell_of(p[0], p[1]) {
            let h = &mut self.heights[iy * self.cols as usize + ix];
            if p[2] > *h {
                *h = p[2];
            }
        }
    }

    /// Adds a triangle: its corners, its edges and the cells whose centre it covers.
    pub fn triangle(&mut self, a: [f32; 3], b: [f32; 3], c: [f32; 3]) {
        for p in [a, b, c] {
            self.splat(p);
        }
        let step = self.cell * 0.5;
        for (p, q) in [(a, b), (b, c), (c, a)] {
            let len = ((q[0] - p[0]).powi(2) + (q[1] - p[1]).powi(2)).sqrt();
            let n = (len / step).ceil() as usize;
            for i in 1..n {
                let t = i as f32 / n as f32;
                self.splat([p[0] + (q[0] - p[0]) * t, p[1] + (q[1] - p[1]) * t, p[2] + (q[2] - p[2]) * t]);
            }
        }
        let (min_x, max_x) = (a[0].min(b[0]).min(c[0]), a[0].max(b[0]).max(c[0]));
        let (min_y, max_y) = (a[1].min(b[1]).min(c[1]), a[1].max(b[1]).max(c[1]));
        if max_x - min_x < self.cell && max_y - min_y < self.cell {
            return;
        }
        let det = (b[1] - c[1]) * (a[0] - c[0]) + (c[0] - b[0]) * (a[1] - c[1]);
        if det.abs() < 1e-6 {
            return;
        }
        let x0 = (((min_x - self.origin[0]) / self.cell).floor().max(0.0)) as usize;
        let x1 = (((max_x - self.origin[0]) / self.cell).floor().max(-1.0) + 1.0).min(self.cols as f32) as usize;
        let y0 = (((min_y - self.origin[1]) / self.cell).floor().max(0.0)) as usize;
        let y1 = (((max_y - self.origin[1]) / self.cell).floor().max(-1.0) + 1.0).min(self.rows as f32) as usize;
        for iy in y0..y1 {
            let py = self.origin[1] + (iy as f32 + 0.5) * self.cell;
            for ix in x0..x1 {
                let px = self.origin[0] + (ix as f32 + 0.5) * self.cell;
                let l1 = ((b[1] - c[1]) * (px - c[0]) + (c[0] - b[0]) * (py - c[1])) / det;
                let l2 = ((c[1] - a[1]) * (px - c[0]) + (a[0] - c[0]) * (py - c[1])) / det;
                let l3 = 1.0 - l1 - l2;
                if l1 >= 0.0 && l2 >= 0.0 && l3 >= 0.0 {
                    let z = l1 * a[2] + l2 * b[2] + l3 * c[2];
                    let h = &mut self.heights[iy * self.cols as usize + ix];
                    if z > *h {
                        *h = z;
                    }
                }
            }
        }
    }

    /// The share of the sky a point sees, 0 to 1, by the same march the lighting
    /// pass makes (eight directions, eight steps growing by 1.75 from 36 units;
    /// `sin^2` of the highest horizon over the points, averaged, at most 0.7 of the light taken). For tests and
    /// the log.
    #[must_use]
    pub fn sky_visibility(&self, p: [f32; 3]) -> f32 {
        let z0 = p[2] + 24.0;
        let mut acc = 0.0;
        for a in 0..8 {
            let ang = a as f32 * std::f32::consts::FRAC_PI_4;
            let dir = [ang.cos(), ang.sin()];
            let mut worst = 0.0f32;
            let mut d = 36.0f32;
            for _ in 0..8 {
                if let Some((ix, iy)) = self.cell_of(p[0] + dir[0] * d, p[1] + dir[1] * d) {
                    let dz = self.heights[iy * self.cols as usize + ix] - z0;
                    if dz > 0.0 {
                        worst = worst.max(dz * dz / (d * d + dz * dz));
                    }
                }
                d *= 1.75;
            }
            acc += worst;
        }
        1.0 - 0.7 * acc / 8.0
    }
}
