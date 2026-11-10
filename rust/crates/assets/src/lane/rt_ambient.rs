//! bo2mp: real-time lighting for the Black Ops III look on a Black Ops II
//! map (`super::bo3_look`): the ambient light rebuilt at load from our own
//! sun and sky, so it follows the time of day we draw, not the one the
//! lightmaps were baked for.
//!
//! Over the play area (the spawn points' box and a margin) a coarse grid of
//! probes is traced against the map's own triangles (the same blockers the
//! sun map is baked from). Per probe a sphere of rays: a ray that leaves the
//! map sees the sky (a zenith and a horizon colour, from the look's sky and
//! fog); a ray that hits a surface sees that surface lit once by the sun
//! (shadow ray included) at an albedo of about 0.3, plus a little of the
//! sky. The radiance is folded into an ambient cube (six faces, cosine
//! weighted), the sun itself left out (it is drawn live). Probes buried in
//! geometry are filled from their neighbours.
//!
//! The grid rides to the shader as rows of the colour-table slot's texture
//! below the sun map (`pack`), and light row `ROW` says where (`write_rows`):
//! `rt_ambient` in `geometry_diagnostic.wgsl` filters it trilinearly and
//! `t7_shade` blends it with the lightmap's ambient. Local lights (the map's
//! spot and omni lights, which the lightmap's baked page used to carry) are
//! switched on in the same row.
//!
//! Settings (all read at load): `BO2MP_RT_AMBIENT` (0 off, 1 on; default on
//! for Carrier and for maps drawn in the Black Ops III look), `_MIX` (0..1,
//! the share of the rebake against the lightmap, default 0.85), `_CELL`
//! (units, default 64), `_RAYS` (per probe, default 64), `_SKY` (the sky's
//! luminance over the sun's, default 0.3), `_BOUNCE` (albedo, default 0.3),
//! `BO2MP_RT_LOCAL` (0 leaves the map's local lights and effect lights out
//! of the branch).

use asset_world::WorldDraw;
use fastfile_t7::bake::{Tracer, parallel_map};

/// The light-table row (vec4s 1000..1003) of the grid; below the look row.
pub(super) const ROW: usize = 250;

/// What the caller gives: the sky colours (zenith, horizon; any scale, they
/// are normalised to unit luminance) when the map takes the rebake, and
/// whether local lights light the branch.
#[derive(Clone, Copy, Debug)]
pub(super) struct Options {
    pub sky: Option<([f32; 3], [f32; 3])>,
    pub local: bool,
}

impl Options {
    /// Nothing changes (maps with their own shaders).
    pub const OFF: Options = Options {
        sky: None,
        local: false,
    };
}

fn env_f32(name: &str, default: f32) -> f32 {
    codlegacy_env::var(name)
        .ok()
        .and_then(|v| v.trim().parse().ok())
        .filter(|v: &f32| v.is_finite() && *v >= 0.0)
        .unwrap_or(default)
}

fn lum(c: [f32; 3]) -> f32 {
    0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2]
}

/// The sky's radiance by direction, in the sun colour's unit.
#[derive(Clone, Copy, Debug)]
pub(super) struct Sky {
    pub zenith: [f32; 3],
    pub horizon: [f32; 3],
    pub ground: [f32; 3],
}

impl Sky {
    /// From two colours and a luminance.
    pub fn new(zenith: [f32; 3], horizon: [f32; 3], level: f32) -> Sky {
        let norm = |c: [f32; 3]| {
            let l = lum(c).max(1e-4);
            c.map(|v| v.max(0.0) / l * level)
        };
        let horizon = norm(horizon);
        Sky {
            zenith: norm(zenith),
            horizon,
            ground: horizon.map(|v| v * 0.25),
        }
    }

    pub fn radiance(&self, d: [f32; 3]) -> [f32; 3] {
        let up = d[2].clamp(-1.0, 1.0);
        if up >= 0.0 {
            let t = up.sqrt();
            std::array::from_fn(|k| self.horizon[k] + (self.zenith[k] - self.horizon[k]) * t)
        } else {
            let t = (-up).sqrt();
            std::array::from_fn(|k| self.horizon[k] + (self.ground[k] - self.horizon[k]) * t)
        }
    }

    /// The sky's light on a surface facing `n` (up: the zenith's, down: the
    /// ground's): a cheap stand-in for the irradiance.
    fn irradiance(&self, n: [f32; 3]) -> [f32; 3] {
        let t = 0.5 + 0.5 * n[2].clamp(-1.0, 1.0);
        std::array::from_fn(|k| {
            self.ground[k] + (self.zenith[k] * 0.5 + self.horizon[k] * 0.5 - self.ground[k]) * t
        })
    }
}

pub(super) struct Params {
    pub sky: Sky,
    /// Toward the sun (unit) and its light.
    pub sun: [f32; 3],
    pub sun_colour: [f32; 3],
    pub albedo: f32,
    pub cell: f32,
    pub max_cells: usize,
    pub rays: usize,
    /// How far a ray looks (units); past it, the sky.
    pub reach: f32,
    /// Seconds the bake may take (0: no limit), kept by growing the cell.
    pub budget: f32,
}

/// The fewest probes the budget may cut the grid down to.
const MIN_CELLS: usize = 3000;

/// The ambient cubes: per cell the light a surface facing +x, -x, +y, -y,
/// +z, -z gets (the unit the lightmap's ambient is in).
pub(super) struct Grid {
    /// The first cell's centre.
    pub origin: [f32; 3],
    pub cell: f32,
    pub dims: [usize; 3],
    pub cubes: Vec<[[f32; 3]; 6]>,
    /// Cells filled from their neighbours (buried in geometry).
    pub filled: usize,
}

const FACES: [[f32; 3]; 6] = [
    [1.0, 0.0, 0.0],
    [-1.0, 0.0, 0.0],
    [0.0, 1.0, 0.0],
    [0.0, -1.0, 0.0],
    [0.0, 0.0, 1.0],
    [0.0, 0.0, -1.0],
];

fn hash(a: usize) -> f32 {
    let mut x = (a as u32).wrapping_mul(0x9E37_79B1) ^ 0x85EB_CA6B;
    x ^= x >> 15;
    x = x.wrapping_mul(0x2C1B_3C6D);
    x ^= x >> 12;
    x = x.wrapping_mul(0x297A_2D39);
    x ^= x >> 15;
    (x >> 8) as f32 / (1u32 << 24) as f32
}

/// `n` directions over the sphere (a Fibonacci spiral).
fn directions(n: usize) -> Vec<[f32; 3]> {
    (0..n)
        .map(|i| {
            let z = 1.0 - 2.0 * (i as f32 + 0.5) / n as f32;
            let r = (1.0 - z * z).max(0.0).sqrt();
            let phi = i as f32 * 2.399_963;
            [r * phi.cos(), r * phi.sin(), z]
        })
        .collect()
}

/// One probe's ambient cube and whether it is buried in geometry (most of
/// its rays hit within a quarter cell).
fn trace_cell(
    tracer: &Tracer,
    p: &Params,
    dirs: &[[f32; 3]],
    origin: [f32; 3],
    cell: f32,
    dims: [usize; 3],
    i: usize,
) -> ([[f32; 3]; 6], bool) {
    let (x, y, z) = (
        i % dims[0],
        (i / dims[0]) % dims[1],
        i / (dims[0] * dims[1]),
    );
    let c = [
        origin[0] + x as f32 * cell,
        origin[1] + y as f32 * cell,
        origin[2] + z as f32 * cell,
    ];
    let yaw = hash(i) * std::f32::consts::TAU;
    let (sy, cy) = yaw.sin_cos();
    let mut cube = [[0f32; 3]; 6];
    let mut near = 0usize;
    for d0 in dirs {
        let d = [d0[0] * cy - d0[1] * sy, d0[0] * sy + d0[1] * cy, d0[2]];
        let radiance = match tracer.nearest(c, d, p.reach) {
            None => p.sky.radiance(d),
            Some((t, n)) => {
                if t < cell * 0.25 {
                    near += 1;
                }
                let h: [f32; 3] = std::array::from_fn(|k| c[k] + d[k] * t + n[k] * 1.5);
                let ndl = (n[0] * p.sun[0] + n[1] * p.sun[1] + n[2] * p.sun[2]).max(0.0);
                let lit = if ndl > 0.0 && !tracer.occluded(h, p.sun, p.reach) {
                    ndl
                } else {
                    0.0
                };
                let sky = p.sky.irradiance(n);
                std::array::from_fn(|k| p.albedo * (p.sun_colour[k] * lit + sky[k] * BOUNCE_SKY))
            }
        };
        for (f, face) in FACES.iter().enumerate() {
            let w = d[0] * face[0] + d[1] * face[1] + d[2] * face[2];
            if w > 0.0 {
                for k in 0..3 {
                    cube[f][k] += radiance[k] * w;
                }
            }
        }
    }
    // The cosine-weighted mean over the sphere: 4 / N times the sum.
    let scale = 4.0 / dirs.len() as f32;
    for f in cube.iter_mut() {
        for v in f.iter_mut() {
            *v *= scale;
        }
    }
    (cube, near as f32 > 0.6 * dirs.len() as f32)
}

/// How much of a hit's own sky light the bounce keeps (it is seen from an
/// unknown spot: about a third of what the open sky would give).
const BOUNCE_SKY: f32 = 0.35;

/// Traces the grid over the box `lo..hi` against the solid triangles. The
/// cell is `p.cell` unless that is more cells than `p.max_cells` or than the
/// time budget (`p.budget` seconds) allows: a pilot of a few hundred probes
/// is timed first, and the cell grows until the whole grid would fit.
pub(super) fn bake(tracer: &Tracer, lo: [f32; 3], hi: [f32; 3], p: &Params) -> Grid {
    let mut cell = p.cell.max(8.0);
    let count = |cell: f32| -> [usize; 3] {
        std::array::from_fn(|k| (((hi[k] - lo[k]) / cell).ceil() as usize).max(1) + 1)
    };
    let cells_of = |d: [usize; 3]| d[0] * d[1] * d[2];
    let mut dims = count(cell);
    while cells_of(dims) > p.max_cells {
        cell *= 1.1;
        dims = count(cell);
    }
    let dirs = directions(p.rays.max(8));
    // The pilot: probes spread over the grid, timed on every core.
    if p.budget > 0.0 {
        const PILOT: usize = 192;
        let total = cells_of(dims);
        let t = std::time::Instant::now();
        let _ = parallel_map(PILOT, |k| {
            trace_cell(
                tracer,
                p,
                &dirs,
                lo,
                cell,
                dims,
                (k * 7919 + 13) % total.max(1),
            )
            .1 as u8
        });
        let per_cell = t.elapsed().as_secs_f32() / PILOT as f32;
        let fit = (p.budget / per_cell.max(1e-7)) as usize;
        while cells_of(dims) > fit.max(MIN_CELLS) && cell < 1024.0 {
            cell *= 1.1;
            dims = count(cell);
        }
    }
    let origin = lo;
    let total = cells_of(dims);
    // (Cube, buried?) per cell.
    let cells: Vec<([[f32; 3]; 6], bool)> = parallel_map(total, |i| {
        trace_cell(tracer, p, &dirs, origin, cell, dims, i)
    });
    let mut cubes: Vec<[[f32; 3]; 6]> = cells.iter().map(|c| c.0).collect();
    let mut valid: Vec<bool> = cells.iter().map(|c| !c.1).collect();
    let filled = valid.iter().filter(|v| !**v).count();
    // Fill the buried cells from valid neighbours, a few sweeps.
    let at = |x: usize, y: usize, z: usize| x + dims[0] * (y + dims[1] * z);
    for _ in 0..6 {
        if valid.iter().all(|v| *v) {
            break;
        }
        let (c0, v0) = (cubes.clone(), valid.clone());
        for z in 0..dims[2] {
            for y in 0..dims[1] {
                for x in 0..dims[0] {
                    let i = at(x, y, z);
                    if v0[i] {
                        continue;
                    }
                    let mut sum = [[0f32; 3]; 6];
                    let mut n = 0.0f32;
                    for (dx, dy, dz) in [
                        (1i32, 0i32, 0i32),
                        (-1, 0, 0),
                        (0, 1, 0),
                        (0, -1, 0),
                        (0, 0, 1),
                        (0, 0, -1),
                    ] {
                        let (nx, ny, nz) = (x as i32 + dx, y as i32 + dy, z as i32 + dz);
                        if nx < 0
                            || ny < 0
                            || nz < 0
                            || nx >= dims[0] as i32
                            || ny >= dims[1] as i32
                            || nz >= dims[2] as i32
                        {
                            continue;
                        }
                        let j = at(nx as usize, ny as usize, nz as usize);
                        if v0[j] {
                            for f in 0..6 {
                                for k in 0..3 {
                                    sum[f][k] += c0[j][f][k];
                                }
                            }
                            n += 1.0;
                        }
                    }
                    if n > 0.0 {
                        for f in 0..6 {
                            for k in 0..3 {
                                cubes[i][f][k] = sum[f][k] / n;
                            }
                        }
                        valid[i] = true;
                    }
                }
            }
        }
    }
    // Whatever is left (a solid block): the mean of the valid cells.
    if valid.iter().any(|v| !*v) {
        let mut mean = [[0f32; 3]; 6];
        let mut n = 0.0f32;
        for (c, ok) in cubes.iter().zip(&valid) {
            if *ok {
                for f in 0..6 {
                    for k in 0..3 {
                        mean[f][k] += c[f][k];
                    }
                }
                n += 1.0;
            }
        }
        for (c, ok) in cubes.iter_mut().zip(&valid) {
            if !*ok {
                for f in 0..6 {
                    for k in 0..3 {
                        c[f][k] = mean[f][k] / n.max(1.0);
                    }
                }
            }
        }
    }
    Grid {
        origin,
        cell,
        dims,
        cubes,
        filled,
    }
}

/// `BO2MP_RT_DUMP=<file>`: writes the grid (dims, cell, then the cubes as
/// f32) for comparing two bakes.
pub(super) fn dump(g: &Grid) {
    let Ok(path) = std::env::var("BO2MP_RT_DUMP") else { return };
    let mut out = Vec::new();
    for d in g.dims {
        out.extend_from_slice(&(d as u32).to_le_bytes());
    }
    out.extend_from_slice(&g.cell.to_le_bytes());
    for v in g.cubes.iter().flat_map(|c| c.iter().flat_map(|f| f.iter())) {
        out.extend_from_slice(&v.to_le_bytes());
    }
    let _ = std::fs::write(path, out);
}

impl Grid {
    /// The light at `p` for a surface facing `n` (unit): the cubes filtered
    /// trilinearly, the faces weighted by n squared (the shader's `rt_ambient`).
    #[cfg(test)]
    pub fn sample(&self, p: [f32; 3], n: [f32; 3]) -> [f32; 3] {
        let g: [f32; 3] = std::array::from_fn(|k| {
            ((p[k] - self.origin[k]) / self.cell).clamp(0.0, (self.dims[k] - 1) as f32)
        });
        let i0: [usize; 3] = std::array::from_fn(|k| (g[k].floor() as usize).min(self.dims[k] - 1));
        let i1: [usize; 3] = std::array::from_fn(|k| (i0[k] + 1).min(self.dims[k] - 1));
        let f: [f32; 3] = std::array::from_fn(|k| g[k] - i0[k] as f32);
        let at = |x: usize, y: usize, z: usize| x + self.dims[0] * (y + self.dims[1] * z);
        let faces = [
            if n[0] >= 0.0 { 0 } else { 1 },
            if n[1] >= 0.0 { 2 } else { 3 },
            if n[2] >= 0.0 { 4 } else { 5 },
        ];
        let mut out = [0f32; 3];
        for (axis, &face) in faces.iter().enumerate() {
            let w = n[axis] * n[axis];
            for corner in 0..8usize {
                let (cx, cy, cz) = (corner & 1, (corner >> 1) & 1, (corner >> 2) & 1);
                let wt = (if cx == 1 { f[0] } else { 1.0 - f[0] })
                    * (if cy == 1 { f[1] } else { 1.0 - f[1] })
                    * (if cz == 1 { f[2] } else { 1.0 - f[2] });
                let c = &self.cubes[at(
                    if cx == 1 { i1[0] } else { i0[0] },
                    if cy == 1 { i1[1] } else { i0[1] },
                    if cz == 1 { i1[2] } else { i0[2] },
                )][face];
                for k in 0..3 {
                    out[k] += c[k] * w * wt;
                }
            }
        }
        out
    }

    /// The grid as rows of an RGBA8 picture `width` wide: one tile of
    /// dims x by dims y per (z slice, face), slice-major, tiles left to right
    /// then down; each value the square root of the light over `scale`.
    /// Returns (texels, rows, tiles per row, scale).
    pub fn pack(&self, width: usize) -> Option<(Vec<u8>, usize, usize, f32)> {
        let [nx, ny, nz] = self.dims;
        let per_row = width / nx;
        if per_row == 0 {
            return None;
        }
        let tiles = nz * 6;
        let rows = tiles.div_ceil(per_row) * ny;
        let peak = self
            .cubes
            .iter()
            .flat_map(|c| c.iter().flat_map(|f| f.iter()))
            .fold(0f32, |a, v| a.max(*v))
            .max(1e-3);
        let mut px = vec![0u8; width * rows * 4];
        for z in 0..nz {
            for (face, _) in FACES.iter().enumerate() {
                let t = z * 6 + face;
                let (ox, oy) = ((t % per_row) * nx, (t / per_row) * ny);
                for y in 0..ny {
                    for x in 0..nx {
                        let c = &self.cubes[x + nx * (y + ny * z)][face];
                        let o = ((oy + y) * width + ox + x) * 4;
                        for k in 0..3 {
                            px[o + k] = ((c[k].max(0.0) / peak).sqrt() * 255.0)
                                .round()
                                .clamp(0.0, 255.0) as u8;
                        }
                        px[o + 3] = 255;
                    }
                }
            }
        }
        Some((px, rows, per_row, peak))
    }
}

/// Whether the rebake is asked for: the map offers a sky and the setting
/// (`BO2MP_RT_AMBIENT`, else Options > REAL-TIME LIGHTING) is on.
pub(super) fn wanted(opts: &Options) -> bool {
    opts.sky.is_some() && setting("BO2MP_RT_AMBIENT")
}

/// The Options > REAL-TIME LIGHTING choice, unless the environment variable
/// is set (it wins: 0 off, anything else on).
fn setting(env: &str) -> bool {
    match codlegacy_env::var(env) {
        Ok(v) => v.trim() != "0",
        Err(_) => frame::settings::RT_LIGHTING.load(std::sync::atomic::Ordering::Relaxed),
    }
}

pub(super) fn local_wanted(opts: &Options) -> bool {
    opts.local && setting("BO2MP_RT_LOCAL")
}

/// The bake's parameters from the settings.
pub(super) fn params(opts: &Options, sun: [f32; 3], sun_colour: [f32; 3]) -> Option<Params> {
    let (zenith, horizon) = opts.sky?;
    let level = env_f32("BO2MP_RT_SKY", 0.3) * lum(sun_colour).max(0.05);
    Some(Params {
        sky: Sky::new(zenith, horizon, level),
        sun,
        sun_colour,
        albedo: env_f32("BO2MP_RT_BOUNCE", 0.3).min(1.0),
        cell: env_f32("BO2MP_RT_CELL", 64.0),
        max_cells: 120_000,
        rays: env_f32("BO2MP_RT_RAYS", 32.0) as usize,
        reach: env_f32("BO2MP_RT_REACH", 4096.0).max(256.0),
        budget: env_f32("BO2MP_RT_BUDGET", 6.0),
    })
}

/// True when light row `ROW` holds no primary light.
pub(super) fn row_free(draw: &WorldDraw) -> bool {
    draw.t6_lights.len() <= ROW || draw.t6_lights[ROW].iter().all(|v| *v == 0.0)
}

/// Writes light row `ROW` (vec4s 1000..1003): v0 the first cell's centre and
/// the cell; v1 the dims and the mix (w, 0 = no grid); v2 the picture's first
/// row, tiles per row, the scale and the picture's width; v3 x: local lights
/// on, y: effect lights on.
pub(super) fn write_rows(
    draw: &mut WorldDraw,
    grid: Option<(&Grid, usize, usize, f32, usize)>,
    mix: f32,
    local: bool,
) {
    if draw.t6_lights.len() <= ROW {
        draw.t6_lights.resize(ROW + 1, [0.0; 16]);
    }
    let mut r = [0f32; 16];
    if let Some((g, y0, per_row, scale, width)) = grid {
        r[0..3].copy_from_slice(&g.origin);
        r[3] = g.cell;
        r[4] = g.dims[0] as f32;
        r[5] = g.dims[1] as f32;
        r[6] = g.dims[2] as f32;
        r[7] = mix;
        r[8] = y0 as f32;
        r[9] = per_row as f32;
        r[10] = scale;
        r[11] = width as f32;
    }
    if local {
        r[12] = 1.0;
        r[13] = 1.0;
    }
    draw.t6_lights[ROW] = r;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn quad(a: [f32; 3], b: [f32; 3], c: [f32; 3], d: [f32; 3]) -> [[[f32; 3]; 3]; 2] {
        [[a, b, c], [a, c, d]]
    }

    fn scene() -> Vec<[[f32; 3]; 3]> {
        let mut t = Vec::new();
        // Floor, z 0.
        t.extend(quad(
            [-1000.0, -1000.0, 0.0],
            [1000.0, -1000.0, 0.0],
            [1000.0, 1000.0, 0.0],
            [-1000.0, 1000.0, 0.0],
        ));
        // A roof, z 120, over x, y -200..200.
        t.extend(quad(
            [-200.0, -200.0, 120.0],
            [200.0, -200.0, 120.0],
            [200.0, 200.0, 120.0],
            [-200.0, 200.0, 120.0],
        ));
        // A wall at x -600, facing +x, y -600..600, z 0..400.
        t.extend(quad(
            [-600.0, -600.0, 0.0],
            [-600.0, 600.0, 0.0],
            [-600.0, 600.0, 400.0],
            [-600.0, -600.0, 400.0],
        ));
        t
    }

    fn params(sky: Sky, sun: [f32; 3]) -> Params {
        Params {
            sky,
            sun,
            sun_colour: [1.0, 0.8, 0.5],
            albedo: 0.3,
            cell: 50.0,
            max_cells: 100_000,
            rays: 96,
            reach: 65536.0,
            budget: 0.0,
        }
    }

    fn mean(c: [f32; 3]) -> f32 {
        (c[0] + c[1] + c[2]) / 3.0
    }

    #[test]
    fn the_floor_under_a_roof_is_darker_than_the_open_floor() {
        let tracer = Tracer::new(&scene());
        let sky = Sky::new([0.5, 0.6, 0.8], [0.6, 0.6, 0.6], 0.3);
        let s = {
            let l = (0.3f32 * 0.3 + 0.9 * 0.9 + 0.3 * 0.3).sqrt();
            [0.3 / l, 0.0, 0.9 / l]
        };
        let g = bake(
            &tracer,
            [-400.0, -400.0, 0.0],
            [400.0, 400.0, 250.0],
            &params(sky, s),
        );
        let under = mean(g.sample([0.0, 0.0, 10.0], [0.0, 0.0, 1.0]));
        let open = mean(g.sample([350.0, 350.0, 10.0], [0.0, 0.0, 1.0]));
        assert!(under < open * 0.7, "under {under} open {open}");
        assert!(under > 0.0);
    }

    #[test]
    fn a_sunlit_wall_bounces_the_sun() {
        let tracer = Tracer::new(&scene());
        // No sky at all: only the bounce is left.
        let sky = Sky {
            zenith: [0.0; 3],
            horizon: [0.0; 3],
            ground: [0.0; 3],
        };
        // The sun toward +x and up: it lights the wall at x -600 (facing +x).
        let s = {
            let l = (0.8f32 * 0.8 + 0.6 * 0.6).sqrt();
            [0.8 / l, 0.0, 0.6 / l]
        };
        let g = bake(
            &tracer,
            [-400.0, -400.0, 0.0],
            [400.0, 400.0, 250.0],
            &params(sky, s),
        );
        // Near the wall, facing it, against facing away from it.
        let toward = mean(g.sample([-400.0, 0.0, 150.0], [-1.0, 0.0, 0.0]));
        let away = mean(g.sample([-400.0, 0.0, 150.0], [1.0, 0.0, 0.0]));
        assert!(toward > 0.02, "toward {toward}");
        assert!(toward > away * 1.5, "toward {toward} away {away}");
    }

    #[test]
    fn the_pack_round_trips() {
        let tracer = Tracer::new(&scene());
        let sky = Sky::new([0.5, 0.6, 0.8], [0.6, 0.6, 0.6], 0.3);
        let g = bake(
            &tracer,
            [-200.0, -200.0, 0.0],
            [200.0, 200.0, 200.0],
            &params(sky, [0.0, 0.0, 1.0]),
        );
        let (px, rows, per_row, scale) = g.pack(512).unwrap();
        assert_eq!(px.len(), 512 * rows * 4);
        let [nx, ny, nz] = g.dims;
        assert!(per_row >= 1 && rows >= ny && nz >= 1);
        // A cell, face +z (4), slice 1.
        let (x, y, z) = (2.min(nx - 1), 3.min(ny - 1), 1.min(nz - 1));
        let t = z * 6 + 4;
        let o = (((t / per_row) * ny + y) * 512 + (t % per_row) * nx + x) * 4;
        let want = g.cubes[x + nx * (y + ny * z)][4];
        for k in 0..3 {
            let got = (px[o + k] as f32 / 255.0).powi(2) * scale;
            assert!(
                (got - want[k]).abs() < 0.03 * scale.max(0.1),
                "{got} {}",
                want[k]
            );
        }
    }

    #[test]
    fn the_cell_count_is_capped() {
        let tracer = Tracer::new(&scene());
        let sky = Sky::new([0.5; 3], [0.5; 3], 0.3);
        let mut p = params(sky, [0.0, 0.0, 1.0]);
        p.max_cells = 500;
        p.rays = 16;
        let g = bake(&tracer, [-900.0, -900.0, 0.0], [900.0, 900.0, 300.0], &p);
        assert!(g.dims[0] * g.dims[1] * g.dims[2] <= 500);
    }

    #[test]
    fn a_tiny_budget_coarsens_the_grid() {
        let tracer = Tracer::new(&scene());
        let sky = Sky::new([0.5; 3], [0.5; 3], 0.3);
        let mut fine = params(sky, [0.0, 0.0, 1.0]);
        fine.cell = 16.0;
        fine.rays = 64;
        let a = bake(&tracer, [-900.0, -900.0, 0.0], [900.0, 900.0, 300.0], &fine);
        // A budget no grid of this size can meet: the cell grows to the
        // floor on the probe count.
        let mut tight = params(sky, [0.0, 0.0, 1.0]);
        tight.cell = 16.0;
        tight.rays = 64;
        tight.budget = 1e-6;
        let b = bake(
            &tracer,
            [-900.0, -900.0, 0.0],
            [900.0, 900.0, 300.0],
            &tight,
        );
        assert!(b.cell > a.cell, "{} {}", b.cell, a.cell);
        assert!(b.dims[0] * b.dims[1] * b.dims[2] < a.dims[0] * a.dims[1] * a.dims[2]);
    }
}
