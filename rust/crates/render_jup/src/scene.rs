//! A Modern Warfare III / Modern Warfare II map read into the plain data the
//! host draws: the world's surfaces (the map zone and the zones beside it:
//! its streamed tiles), the placed models with one instance per placement,
//! each surface's material and the pictures that material has (the colour
//! picture and the packed normal map, with their mip chains from the
//! install's `.xpak` packages), the zone's own light (reflection probes,
//! the vision set's fog and sun) and what is missing.
//!
//! Read on a thread of the caller's: [`Scene::load`] takes the map zone's
//! path (`<install>/zone/<map>.ff`) and needs the game's Oodle.

use std::path::{Path, PathBuf};

use fastfile_jup::image::{self, ImageRec};
use fastfile_jup::map::materials::{MapImage, Packages};
use fastfile_jup::map::{JupMap, MapSources};

/// A vertex of the world or of a model: position, smooth normal, picture
/// coordinates, and the layer blend weights the world's vertices carry (the
/// surface's extra stream: byte k is layer k's weight, byte 0 always 255;
/// all zero where a vertex has none, and the layers then blend by their masks
/// alone).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, bytemuck::Pod, bytemuck::Zeroable)]
pub struct Vertex {
    pub pos: [f32; 3],
    pub normal: [f32; 3],
    pub uv: [f32; 2],
    pub blend: [u8; 4],
    /// The second texture coordinate set (the first again where there is none).
    pub uv2: [f32; 2],
}

/// Where one placement stands: the model's axes (rows: forward, left, up)
/// with the scale in `a0.w`, and its origin. The world's own surfaces use the
/// identity.
#[repr(C)]
#[derive(Clone, Copy, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub struct Instance {
    pub a0: [f32; 4],
    pub a1: [f32; 4],
    pub a2: [f32; 4],
    pub origin: [f32; 4],
}

impl Instance {
    pub const IDENTITY: Self = Self { a0: [1.0, 0.0, 0.0, 1.0], a1: [0.0, 1.0, 0.0, 0.0], a2: [0.0, 0.0, 1.0, 0.0], origin: [0.0; 4] };
}

/// One indexed draw: the triangles of one material of one mesh, over a run
/// of instances.
#[derive(Clone, Copy, Debug)]
pub struct Draw {
    /// Index into [`Scene::materials`].
    pub material: u32,
    pub first_index: u32,
    pub index_count: u32,
    pub first_instance: u32,
    pub instances: u32,
    /// World box of everything the draw covers (all its instances), for culling.
    pub mins: [f32; 3],
    pub maxs: [f32; 3],
    /// The placed model drawn (an index into [`Scene::model_names`]), `u32::MAX` for the world.
    pub model: u32,
}

/// Side of the cells world surfaces are grouped into, so a draw covers a
/// small area the host can cull.
const CELL: f32 = 3072.0;

/// A world triangle whose texture mapping stretches one way by less than this
/// share of the other (the smaller principal stretch over the larger) is mapped by
/// its plane instead: the zone gives world surfaces coordinates on a line or
/// sheared (their material makes the real ones from the position).
const PLANAR_RATIO: f32 = 0.4;
/// Texture units per world unit for a material with too few sound triangles to
/// measure (a picture across about 100 units), and the range a measured one is held to.
const DEFAULT_DENSITY: f32 = 1.0 / 96.0;
const MIN_DENSITY: f32 = 1.0 / 1024.0;
const MAX_DENSITY: f32 = 1.0 / 16.0;

/// A picture with its mip chain (largest first), blocks as stored.
#[derive(Clone, Debug)]
pub struct SceneImage {
    pub hash: u64,
    /// DXGI format (71 BC1, 98 BC7 UNORM).
    pub dxgi: u32,
    pub width: u32,
    pub height: u32,
    pub levels: Vec<Vec<u8>>,
}

/// The paint a placed model's name states (`..._yellow`, `..._grey`,
/// `red::...`), as a linear albedo multiplier for its near-white tint base.
/// The game takes the paint from the material's constants (packed colours in
/// its constant rows, not decoded far enough to tell which row a program
/// reads); the model names say it for the variants Highrise places
/// (`crane_towerpeak_01_yellow`, `crane_pendant_01_grey`,
/// `pipe_metal_painted_straight_64_thin_gray`, `red::pipe_metal_...`).
/// Without a colour word: a mid-grey painted steel, not the base's white.
pub fn paint_of(name: &str) -> [f32; 3] {
    let n = name.to_ascii_lowercase();
    let words: &[(&str, [f32; 3])] = &[
        ("yellow", [0.62, 0.43, 0.05]),
        ("orange", [0.62, 0.24, 0.04]),
        ("red", [0.42, 0.06, 0.04]),
        ("blue", [0.07, 0.16, 0.38]),
        ("green", [0.10, 0.22, 0.09]),
        ("black", [0.06, 0.06, 0.06]),
        ("white", [0.72, 0.72, 0.70]),
        ("grey", [0.30, 0.30, 0.30]),
        ("gray", [0.30, 0.30, 0.30]),
        ("tan", [0.45, 0.36, 0.24]),
    ];
    // (A word of the name: between separators, or before `::`.)
    let parts: Vec<&str> = n.split(|c: char| c == '_' || c == '/' || c == ':').collect();
    for (w, c) in words {
        if parts.iter().any(|p| p == w) {
            return *c;
        }
    }
    [0.62, 0.62, 0.62]
}

/// The mean of a picture's brightness (display-encoded, at about 64 texels a side).
pub fn mean_brightness(img: &SceneImage) -> Option<f32> {
    let (_, _, l) = small_luma(img)?;
    Some(l.iter().sum::<f32>() / l.len().max(1) as f32)
}

/// Above this mean brightness a model's colour picture is a tint base (paint
/// over white: the colour comes from the material).
const TINT_BASE: f32 = 0.70;

/// Past this many units from its origin a model's mesh is a far vista cluster.
const FAR_VISTA_REACH: f32 = 12_000.0;

/// A picture's level of about 64 texels a side as luminance, (width, height, values).
fn small_luma(img: &SceneImage) -> Option<(usize, usize, Vec<f32>)> {
    let (w, h, c) = small_rgb(img)?;
    Some((w, h, c.iter().map(|p| 0.3 * p[0] + 0.59 * p[1] + 0.11 * p[2]).collect()))
}

/// The mean colour of a picture (display-encoded).
pub fn mean_rgb(img: &SceneImage) -> Option<[f32; 3]> {
    let (_, _, c) = small_rgb(img)?;
    let n = c.len().max(1) as f32;
    Some([0, 1, 2].map(|k| c.iter().map(|p| p[k]).sum::<f32>() / n))
}

/// Past this spread between its mean channels a colour picture is not an albedo
/// (Highrise material 65's usage-0x32 picture: green with yellow flecks, a packed
/// mask; drawn as colour it made the bright green ledges).
const NOT_ALBEDO_CHROMA: f32 = 0.45;

/// A picture's level of about 64 texels a side as display-encoded colour.
fn small_rgb(img: &SceneImage) -> Option<(usize, usize, Vec<[f32; 3]>)> {
    let bs = match img.dxgi {
        71 => 8,
        98 => 16,
        _ => return None,
    };
    let mut k = 0usize;
    let (mut w, mut h) = (img.width as usize, img.height as usize);
    while w.min(h) > 64 && k + 1 < img.levels.len() {
        k += 1;
        w = (w / 2).max(1);
        h = (h / 2).max(1);
    }
    if w < 16 || h < 16 {
        return None;
    }
    let data = img.levels.get(k)?;
    let mut out = vec![[0f32; 3]; w * h];
    let mut tmp = [0u8; 64];
    for by in 0..h.div_ceil(4) {
        for bx in 0..w.div_ceil(4) {
            let at = (by * w.div_ceil(4) + bx) * bs;
            let src = data.get(at..at + bs)?;
            if bs == 8 {
                bcdec_rs::bc1(src, &mut tmp, 16);
            } else {
                bcdec_rs::bc7(src, &mut tmp, 16);
            }
            for y in 0..4 {
                for x in 0..4 {
                    let (qx, qy) = (bx * 4 + x, by * 4 + y);
                    if qx < w && qy < h {
                        let p = &tmp[y * 16 + x * 4..][..3];
                        out[qy * w + qx] = [f32::from(p[0]) / 255.0, f32::from(p[1]) / 255.0, f32::from(p[2]) / 255.0];
                    }
                }
            }
        }
    }
    Some((w, h, out))
}

/// How much a picture's brightness varies at large scale: the deviation of the
/// means of its 8 x 8 blocks (at about 64 texels a side). A picture made to
/// repeat over a wall or a floor varies little at that scale (Highrise's
/// concrete 0.008, paper litter 0.012, painted plaster 0.022); an atlas of
/// separate things does not (a boot 0.062). `None` for a picture this cannot read.
pub fn block_deviation(img: &SceneImage) -> Option<f32> {
    let (w, h, l) = small_luma(img)?;
    let (bw, bh) = (w / 8, h / 8);
    let means: Vec<f32> = (0..64)
        .map(|b| {
            let (bx, by) = (b % 8, b / 8);
            let mut s = 0.0;
            for y in by * bh..(by + 1) * bh {
                for x in bx * bw..(bx + 1) * bw {
                    s += l[y * w + x];
                }
            }
            s / (bw * bh).max(1) as f32
        })
        .collect();
    let mean = means.iter().sum::<f32>() / 64.0;
    Some((means.iter().map(|m| (m - mean) * (m - mean)).sum::<f32>() / 64.0).sqrt())
}

/// Past this block deviation an upper layer's colour picture is an atlas.
const ATLAS_DEVIATION: f32 = 0.045;

/// A material as the fallback shader draws it.
#[derive(Clone, Debug, Default)]
pub struct SceneMaterial {
    pub hash: u64,
    /// Indices into [`Scene::images`]: the four layers' colour pictures and
    /// packed normal maps, and the blend masks of layers 1 to 3.
    pub colour: [Option<usize>; 4],
    pub normal: [Option<usize>; 4],
    pub mask: [Option<usize>; 3],
    /// Layers drawn (1 to 4).
    pub count: u8,
    /// The colour picture cuts holes (BC1 punch-through): alpha-tested.
    pub cutout: bool,
    /// The colour picture's alpha is partly translucent (BC7 blocks whose
    /// alpha endpoints are both well under opaque): drawn blended, sorted.
    pub blend: bool,
    /// The sea (a flat sheet at worldspawn's `sealevel`).
    pub water: bool,
    /// A tool material (a level designer's `Caulk Shadow` and its kin: a
    /// small flat-coloured picture with a caption): geometry that is there to
    /// cast shadows or block, never to be seen. Drawn into the shadow map
    /// only ([`tool_picture`]).
    pub tool: bool,
    /// The albedo a layer with no colour picture shows ([`crate::skylight::fallback_tint`]).
    pub tint: [f32; 3],
    /// A paint colour the colour picture is multiplied by (a placed model's
    /// near-white tint base, [`paint_of`]), where it has one.
    pub paint: Option<[f32; 3]>,
    /// Layers sampled with the second texture coordinate set.
    pub second_uv: [bool; 4],
    /// A colour each layer's picture is multiplied by (linear; 1 where none).
    pub layer_tint: [[f32; 3]; 4],
}

/// A reflection probe's ambient light (linear).
#[derive(Clone, Debug)]
pub struct Probe {
    pub origin: [f32; 3],
    pub up: [f32; 3],
    pub side: [f32; 3],
    pub down: [f32; 3],
    /// The probe's octahedral radiance, [`PROBE_SIZE`] square (linear rgb).
    pub radiance: Vec<[f32; 3]>,
}

/// Side of the probe radiance pictures the host keeps.
pub const PROBE_SIZE: usize = 64;

/// The map's fog (the vision set's, linear colours).
#[derive(Clone, Copy, Debug)]
pub struct SceneFog {
    pub start: f32,
    pub half: f32,
    pub opacity: f32,
    pub normal_scale: f32,
    pub colour: [f32; 3],
    pub sun_colour: [f32; 3],
    pub sun_dir: [f32; 3],
    pub sun_begin_deg: f32,
    pub sun_end_deg: f32,
    pub base_height: f32,
    pub half_height: f32,
}

/// The map's light and air.
#[derive(Clone, Debug)]
pub struct Env {
    /// Unit vector toward the sun (z up).
    pub sun_dir: [f32; 3],
    /// Linear sun colour times its intensity.
    pub sun_colour: [f32; 3],
    /// The sun's colour and intensity are worldspawn's (else a stand-in colour, which the map's
    /// sky picture then tints).
    pub sun_given: bool,
    pub night: bool,
    pub fog: Option<SceneFog>,
    /// The ambient light where no probe is read (linear).
    pub ambient: [f32; 3],
    pub sky_zenith: [f32; 3],
    pub sky_horizon: [f32; 3],
    /// The ambient light of the open sky for a surface facing up, sideways and
    /// down ([`crate::skylight::sky_light`]).
    pub amb_up: [f32; 3],
    pub amb_side: [f32; 3],
    pub amb_down: [f32; 3],
    /// A probe applies fully within `probe_reach[0]` of itself and not at all past
    /// `probe_reach[1]`, with at most `probe_weight` of the ambient light.
    pub probe_reach: [f32; 2],
    pub probe_weight: f32,
}

/// The map, read.
pub struct Scene {
    pub name: String,
    pub install: PathBuf,
    pub vertices: Vec<Vertex>,
    pub indices: Vec<u32>,
    pub instances: Vec<Instance>,
    pub draws: Vec<Draw>,
    pub materials: Vec<SceneMaterial>,
    pub images: Vec<SceneImage>,
    pub probes: Vec<Probe>,
    pub env: Env,
    /// The map's own sky pictures as a cube, where they could be read.
    pub sky: Option<crate::sky::SkyCube>,
    /// The highest geometry over the ground plan, for the sky visibility.
    pub heights: Option<crate::skylight::HeightGrid>,
    /// The map's entity text.
    pub entities: String,
    /// The map's colour grade (a 32^3 volume, 16-bit unorm rgba), where it has one.
    pub grade: Option<Vec<u16>>,
    pub mins: [f32; 3],
    pub maxs: [f32; 3],
    /// Surfaces, placed models and triangles, for the log.
    pub world_surfaces: usize,
    pub model_draws: usize,
    /// The placed models' names, by [`Draw::model`].
    pub model_names: Vec<String>,
    pub placements: usize,
    pub report: Vec<String>,
}

/// How much of the open sky's light the ambient takes (the sky cube is drawn
/// as the sky itself). Against a sun of [`DAY_SUN`]: sunlit faces about six
/// times their shade, a low late-day sun's contrast (it was 0.8 against a sun
/// of 1.2, which left shade and sun two to one, the flat, muddy look).
const AMBIENT_SCALE: f32 = 0.48;

/// How much of the sky picture's sun-disc colour the sun takes (`BO2MP_JUP_SUN_TINT`).
const SUN_TINT: f32 = 0.6;

/// The sun's intensity where worldspawn gives none (`BO2MP_JUP_SUN` sets it).
const DAY_SUN: f32 = 3.0;

/// The highest geometry over the playable ground plan: the world's surfaces
/// and the placed models, cells of 24 units or more, at most 1024 a side.
/// A surface every map's list ends with: a box 1116 x 320 x 272 units at the origin (72 triangles,
/// or two of 36 on Farm 18) whose record is out of the list's order and names a material at
/// random (Derail's showed a magazine and cereal box atlas on it, Elevated's spawn stands inside
/// it). Highrise, Afghan, Das Haus, Derail, Elevated and Farm 18 have it; Shipment does not. It is
/// not part of the map and is left out.
fn reference_box(s: &fastfile_jup::map::world::WorldSurface) -> bool {
    if s.positions.is_empty() || !matches!(s.tris.len(), 36 | 72) {
        return false;
    }
    let ext = |a: usize| s.positions.iter().fold((f32::MAX, f32::MIN), |(l, h), p| (l.min(p[a]), h.max(p[a])));
    ext(0) == (-456.0, 660.0) && ext(1) == (-148.0, 172.0)
}

fn build_heights(map: &JupMap, report: &mut Vec<String>) -> Option<crate::skylight::HeightGrid> {
    let t0 = std::time::Instant::now();
    let wide = |s: &fastfile_jup::map::world::WorldSurface| {
        let ext = |a: usize| s.positions.iter().fold((f32::MAX, f32::MIN), |(l, h), p| (l.min(p[a]), h.max(p[a])));
        let (ex, ey, ez) = (ext(0), ext(1), ext(2));
        (ex.1 - ex.0 > SHEET || ey.1 - ey.0 > SHEET) && ez.1 - ez.0 > 1.0
    };
    let mut cx: Vec<f32> = Vec::new();
    let mut cy: Vec<f32> = Vec::new();
    let mut keep: Vec<&fastfile_jup::map::world::WorldSurface> = Vec::new();
    for s in &map.surfaces {
        if s.tris.is_empty() || s.positions.is_empty() || wide(s) || reference_box(s) {
            continue;
        }
        let ext = |a: usize| s.positions.iter().fold((f32::MAX, f32::MIN), |(l, h), p| (l.min(p[a]), h.max(p[a])));
        let (ex, ey) = (ext(0), ext(1));
        cx.push((ex.0 + ex.1) * 0.5);
        cy.push((ey.0 + ey.1) * 0.5);
        keep.push(s);
    }
    if cx.len() < 8 {
        return None;
    }
    cx.sort_by(f32::total_cmp);
    cy.sort_by(f32::total_cmp);
    let pct = |v: &[f32], q: usize| v[(v.len() * q / 100).min(v.len() - 1)];
    let (x0, x1, y0, y1) = (pct(&cx, 3), pct(&cx, 97), pct(&cy, 3), pct(&cy, 97));
    let pad = 0.1 * (x1 - x0).max(y1 - y0) + 600.0;
    let mut grid = crate::skylight::HeightGrid::new([x0 - pad, y0 - pad], [x1 + pad, y1 + pad], 24.0, 1024);
    for s in &keep {
        for t in &s.tris {
            if t.iter().all(|&i| usize::from(i) < s.positions.len()) {
                grid.triangle(s.positions[usize::from(t[0])], s.positions[usize::from(t[1])], s.positions[usize::from(t[2])]);
            }
        }
    }
    let mut prop_tris = 0usize;
    for p in &map.props {
        let Some(model) = map.prop_models.get(p.model) else { continue };
        let sc = p.scale;
        let place = |v: [f32; 3]| -> [f32; 3] {
            std::array::from_fn(|k| p.origin[k] + (v[0] * p.axis[0][k] + v[1] * p.axis[1][k] + v[2] * p.axis[2][k]) * sc)
        };
        for s in &model.surfaces {
            for t in &s.tris {
                if t.iter().all(|&i| usize::from(i) < s.positions.len()) {
                    grid.triangle(place(s.positions[usize::from(t[0])]), place(s.positions[usize::from(t[1])]), place(s.positions[usize::from(t[2])]));
                    prop_tris += 1;
                }
            }
        }
    }
    report.push(format!(
        "sky visibility grid: {} x {} cells of {:.0} units over {:.0},{:.0} .. {:.0},{:.0} from {} surfaces and {prop_tris} model triangles in {:.1}s",
        grid.cols,
        grid.rows,
        grid.cell,
        grid.origin[0],
        grid.origin[1],
        grid.origin[0] + grid.cols as f32 * grid.cell,
        grid.origin[1] + grid.rows as f32 * grid.cell,
        keep.len(),
        t0.elapsed().as_secs_f32()
    ));
    Some(grid)
}

/// The largest a picture is read at.
pub const COLOUR_MAX: u32 = 1024;
pub const NORMAL_MAX: u32 = 512;
pub const MASK_MAX: u32 = 256;
/// A surface wider than this on the ground plan is a sheet (the sea, a far
/// vista's ground): drawn only when it is flat.
const SHEET: f32 = 16384.0;

fn srgb_to_linear(c: f32) -> f32 {
    if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) }
}

/// Smooth normals for a triangle list (area-weighted).
fn normals_of(positions: &[[f32; 3]], tris: &[[u16; 3]]) -> Vec<[f32; 3]> {
    let mut n = vec![[0.0f32; 3]; positions.len()];
    for t in tris {
        if t.iter().any(|&i| usize::from(i) >= positions.len()) {
            continue;
        }
        let [a, b, c] = t.map(|i| positions[usize::from(i)]);
        let u = [c[0] - a[0], c[1] - a[1], c[2] - a[2]];
        let v = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
        let f = [u[1] * v[2] - u[2] * v[1], u[2] * v[0] - u[0] * v[2], u[0] * v[1] - u[1] * v[0]];
        for &i in t {
            for k in 0..3 {
                n[usize::from(i)][k] += f[k];
            }
        }
    }
    n.into_iter()
        .map(|v| {
            let l = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
            if l > 1e-12 { v.map(|x| x / l) } else { [0.0, 0.0, 1.0] }
        })
        .collect()
}

/// Normals at each corner of each triangle: the area-weighted mean of the
/// faces that meet at the vertex and lie within 35 degrees of this face (a
/// crease beyond that stays sharp: the vertex streams carry no usable normals).
fn corner_normals(positions: &[[f32; 3]], tris: &[[u16; 3]]) -> Vec<[[f32; 3]; 3]> {
    let raw: Vec<[f32; 3]> = tris
        .iter()
        .map(|t| {
            if t.iter().any(|&i| usize::from(i) >= positions.len()) {
                return [0.0; 3];
            }
            let [a, b, c] = t.map(|i| positions[usize::from(i)]);
            let u = [c[0] - a[0], c[1] - a[1], c[2] - a[2]];
            let v = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
            [u[1] * v[2] - u[2] * v[1], u[2] * v[0] - u[0] * v[2], u[0] * v[1] - u[1] * v[0]]
        })
        .collect();
    let unit = |v: [f32; 3]| {
        let l = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
        if l > 1e-12 { v.map(|x| x / l) } else { [0.0; 3] }
    };
    let faces: Vec<[f32; 3]> = raw.iter().map(|&v| unit(v)).collect();
    let mut incident: Vec<Vec<u32>> = vec![Vec::new(); positions.len()];
    for (f, t) in tris.iter().enumerate() {
        if t.iter().all(|&i| usize::from(i) < positions.len()) {
            for &i in t {
                incident[usize::from(i)].push(f as u32);
            }
        }
    }
    const COS: f32 = 0.82;
    tris.iter()
        .enumerate()
        .map(|(f, t)| {
            std::array::from_fn(|c| {
                let Some(list) = incident.get(usize::from(t[c])) else { return [0.0, 0.0, 1.0] };
                let mut n = [0.0f32; 3];
                for &g in list {
                    let g = g as usize;
                    if faces[g][0] * faces[f][0] + faces[g][1] * faces[f][1] + faces[g][2] * faces[f][2] > COS {
                        for k in 0..3 {
                            n[k] += raw[g][k];
                        }
                    }
                }
                let n = unit(n);
                if n == [0.0; 3] { [0.0, 0.0, 1.0] } else { n }
            })
        })
        .collect()
}

/// The largest and smallest stretch of a triangle's mapping into texture space
/// (texture units per world unit along its two principal directions) and its
/// world area. A small ratio is a mapping that lies on a line.
fn uv_singular(positions: &[[f32; 3]], uvs: &[[f32; 2]], t: &[u16; 3]) -> Option<(f32, f32, f32)> {
    if t.iter().any(|&i| usize::from(i) >= positions.len() || usize::from(i) >= uvs.len()) {
        return None;
    }
    let [q0, q1, q2] = t.map(|i| positions[usize::from(i)]);
    let [p0, p1, p2] = t.map(|i| uvs[usize::from(i)]);
    let e1 = [q1[0] - q0[0], q1[1] - q0[1], q1[2] - q0[2]];
    let e2 = [q2[0] - q0[0], q2[1] - q0[1], q2[2] - q0[2]];
    let dot = |a: [f32; 3], b: [f32; 3]| a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
    let a = dot(e1, e1).sqrt();
    if a < 1e-6 {
        return None;
    }
    let b = dot(e1, e2) / a;
    let c2 = dot(e2, e2) - b * b;
    if c2 <= 1e-12 {
        return None;
    }
    let c = c2.sqrt();
    let grad = |d1: f32, d2: f32| (d1 / a, (d2 - b * d1 / a) / c);
    let gu = grad(p1[0] - p0[0], p2[0] - p0[0]);
    let gv = grad(p1[1] - p0[1], p2[1] - p0[1]);
    let (p, q, r) = (gu.0 * gu.0 + gu.1 * gu.1, gv.0 * gv.0 + gv.1 * gv.1, gu.0 * gv.0 + gu.1 * gv.1);
    let root = ((p - q) * (p - q) + 4.0 * r * r).sqrt();
    Some((((p + q + root) * 0.5).max(0.0).sqrt(), ((p + q - root) * 0.5).max(0.0).sqrt(), 0.5 * a * c))
}

/// The area of a triangle in texture space and in the world (None where the
/// indices run off the vertices).
fn tri_areas(positions: &[[f32; 3]], uvs: &[[f32; 2]], t: &[u16; 3]) -> Option<(f32, f32)> {
    if t.iter().any(|&i| usize::from(i) >= positions.len() || usize::from(i) >= uvs.len()) {
        return None;
    }
    let [p0, p1, p2] = t.map(|i| uvs[usize::from(i)]);
    let uva = ((p1[0] - p0[0]) * (p2[1] - p0[1]) - (p1[1] - p0[1]) * (p2[0] - p0[0])).abs();
    let [q0, q1, q2] = t.map(|i| positions[usize::from(i)]);
    let u = [q1[0] - q0[0], q1[1] - q0[1], q1[2] - q0[2]];
    let v = [q2[0] - q0[0], q2[1] - q0[1], q2[2] - q0[2]];
    let c = [u[1] * v[2] - u[2] * v[1], u[2] * v[0] - u[0] * v[2], u[0] * v[1] - u[1] * v[0]];
    Some((uva, (c[0] * c[0] + c[1] * c[1] + c[2] * c[2]).sqrt()))
}

/// Does a BC1 picture cut holes (a tenth of its blocks in the punch-through mode)?
/// How far the average of a packed normal map (from its smallest level's first
/// block) leans from flat, in the tangent plane (0 flat, 1 on its side).
fn mean_tilt(img: &SceneImage) -> f32 {
    let Some(last) = img.levels.last() else { return 0.0 };
    let mut px = [0u8; 64];
    match img.dxgi {
        71 if last.len() >= 8 => bcdec_rs::bc1(&last[..8], &mut px, 16),
        98 if last.len() >= 16 => bcdec_rs::bc7(&last[..16], &mut px, 16),
        _ => return 0.0,
    }
    let (mut g, mut a) = (0.0f32, 0.0f32);
    for p in px.chunks_exact(4) {
        g += f32::from(p[1]);
        a += f32::from(p[3]);
    }
    let (p, q) = (g / (16.0 * 127.5) - 1.0, a / (16.0 * 127.5) - 1.0);
    (((p + q) * 0.5).powi(2) + ((p - q) * 0.5).powi(2)).sqrt()
}

/// Whether a picture is a level designer's tool texture (`Caulk Shadow`,
/// `Caulk`, `Clip` and the like: a small opaque picture of one flat colour
/// with a black caption). The game never draws these (they are shadow casters
/// and blockers); drawn, they were the flat olive-brown floors, ceilings and
/// faceted hulls of Highrise in the first in-game frames. At most 256 a side,
/// BC1 or BC7, over half its pixels within a few steps of one light colour and
/// a twentieth of them near black.
pub fn tool_picture(img: &SceneImage) -> bool {
    if img.width > 256 || img.height > 256 || img.width < 16 || img.height < 16 || !matches!(img.dxgi, 71 | 98) {
        return false;
    }
    let Some(level0) = img.levels.first() else { return false };
    let (w, h) = (img.width as usize, img.height as usize);
    let block = if img.dxgi == 71 { 8 } else { 16 };
    let (bw, bh) = (w.div_ceil(4), h.div_ceil(4));
    if level0.len() < bw * bh * block {
        return false;
    }
    let mut px: Vec<[u8; 4]> = Vec::with_capacity(w * h);
    let mut tmp = [0u8; 64];
    px.resize(w * h, [0; 4]);
    for by in 0..bh {
        for bx in 0..bw {
            let at = (by * bw + bx) * block;
            if img.dxgi == 71 {
                bcdec_rs::bc1(&level0[at..at + 8], &mut tmp, 16);
            } else {
                bcdec_rs::bc7(&level0[at..at + 16], &mut tmp, 16);
            }
            for y in 0..4 {
                for x in 0..4 {
                    let (qx, qy) = (bx * 4 + x, by * 4 + y);
                    if qx < w && qy < h {
                        px[qy * w + qx] = [tmp[y * 16 + x * 4], tmp[y * 16 + x * 4 + 1], tmp[y * 16 + x * 4 + 2], tmp[y * 16 + x * 4 + 3]];
                    }
                }
            }
        }
    }
    if px.iter().any(|p| p[3] < 250) {
        return false;
    }
    // The commonest colour (5 bits a channel).
    let mut hist: std::collections::HashMap<u16, usize> = std::collections::HashMap::new();
    for p in &px {
        *hist.entry((u16::from(p[0] >> 3) << 10) | (u16::from(p[1] >> 3) << 5) | u16::from(p[2] >> 3)).or_default() += 1;
    }
    let Some((&mode, _)) = hist.iter().max_by_key(|(_, n)| **n) else { return false };
    let bg = [f32::from((mode >> 10) as u8 & 31) * 8.0 + 4.0, f32::from((mode >> 5) as u8 & 31) * 8.0 + 4.0, f32::from(mode as u8 & 31) * 8.0 + 4.0];
    let luma = |c: [f32; 3]| (0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2]) / 255.0;
    if luma(bg) < 0.25 {
        return false;
    }
    // (The tool pictures' ground is warm, khaki or maroon, red well over blue: a pale blue
    // pictogram on Derail passed everything else.)
    if bg[0] < bg[2] + 40.0 {
        return false;
    }
    let (mut near_bg, mut dark) = (0usize, 0usize);
    for p in &px {
        let c = [f32::from(p[0]), f32::from(p[1]), f32::from(p[2])];
        if (0..3).all(|k| (c[k] - bg[k]).abs() <= 28.0) {
            near_bg += 1;
        } else if luma(c) < 0.12 {
            dark += 1;
        }
    }
    let n = px.len();
    near_bg * 2 > n && near_bg * 100 < n * 97 && dark * 20 > n
}

fn bc1_cuts(dxgi: u32, data: &[u8]) -> bool {
    if dxgi != 71 || data.len() < 8 {
        return false;
    }
    let blocks = data.chunks_exact(8);
    let n = blocks.len();
    // (A block is cut when it is in the three-colour mode and uses index 3,
    // the transparent one; flat blocks with equal endpoints never do.)
    let holes = blocks
        .filter(|b| {
            u16::from_le_bytes([b[0], b[1]]) <= u16::from_le_bytes([b[2], b[3]]) && (0..4).any(|r| (0..4).any(|c| (b[4 + r] >> (2 * c)) & 3 == 3))
        })
        .count();
    holes * 10 >= n
}

/// A BC7 picture whose alpha is translucent over a fifth of its blocks. Only
/// the modes that carry alpha are read (4, 5 and 6, whose alpha endpoints sit
/// at fixed bits; 4 and 5 only with no channel rotation); a block is
/// translucent when both endpoints are under 200.
fn bc7_translucent(dxgi: u32, data: &[u8]) -> bool {
    if dxgi != 98 || data.len() < 16 {
        return false;
    }
    let (mut n, mut seen) = (0usize, 0usize);
    for b in data.chunks_exact(16) {
        n += 1;
        let v = u128::from_le_bytes(b.try_into().unwrap_or([0; 16]));
        let mode = (v as u8).trailing_zeros();
        // (Modes 4 and 5 carry a rotation that swaps a colour channel into the alpha
        // slot, which an encoder does to opaque pictures; those blocks say nothing.)
        let (a0, a1) = match mode {
            4 if (v >> 5) & 3 == 0 => (((v >> 38) & 0x3f) as u32 * 255 / 63, ((v >> 44) & 0x3f) as u32 * 255 / 63),
            5 if (v >> 6) & 3 == 0 => (((v >> 50) & 0xff) as u32, ((v >> 58) & 0xff) as u32),
            6 => (((v >> 49) & 0x7f) as u32 * 255 / 127, ((v >> 56) & 0x7f) as u32 * 255 / 127),
            _ => continue,
        };
        if a0 < 200 && a1 < 200 {
            seen += 1;
        }
    }
    seen * 5 >= n
}

/// A picture's mip chain no larger than `max_dim`: every level of every
/// streamed group (fetched once each) that fits, down as far as the chain is
/// whole. The pixels in the zone (a small picture) are its whole chain.
/// The format byte of the one-channel blend masks (usage 0x33), and their DXGI format (BC4).
const FORMAT_MASK: u8 = 0x2a;
const DXGI_BC4: u32 = 80;

pub fn mip_chain(paks: &Packages, img: &MapImage, max_dim: u32, oodle: &dyn fastfile_jup::oodle::Decompressor) -> Result<SceneImage, String> {
    // (Format byte 0x2a is the one-channel blend mask: BC4, the same eight bytes a block
    // as BC1. Read as BC1 it is noise, which put random layers into every blended material.)
    let dxgi = if img.format == FORMAT_MASK { Some(DXGI_BC4) } else { image::dxgi(img.format) }.ok_or_else(|| format!("format {:#x}", img.format))?;
    let rec = ImageRec {
        at: 0,
        hash: img.hash,
        flags: 0,
        data_size: 0,
        width: img.width,
        height: img.height,
        depth: 1,
        array: 1,
        format: img.format,
        usage: 0,
        levels: 0,
        groups: img.groups.clone(),
        inline: None,
        end: 0,
    };
    let mut got: Vec<(u32, u32, Vec<u8>)> = Vec::new();
    for g in 0..img.groups.len() {
        let Some(levels) = image::group_levels(&rec, g, dxgi) else { continue };
        let fits: Vec<_> = levels.into_iter().filter(|l| l.1.max(l.2) <= max_dim).collect();
        if fits.is_empty() {
            continue;
        }
        let Some(bytes) = paks.fetch(img.groups[g].key, oodle) else { continue };
        for (_, w, h, off) in fits {
            let n = image::level_bytes(dxgi, w, h).unwrap_or(0);
            if let Some(d) = bytes.get(off..off + n) {
                got.push((w, h, d.to_vec()));
            }
        }
    }
    if got.is_empty()
        && let Some(px) = &img.inline
    {
        let (mut w, mut h) = (u32::from(img.width), u32::from(img.height));
        let mut off = 0usize;
        while w > 0 && h > 0 {
            let n = image::level_bytes(dxgi, w, h).unwrap_or(0);
            if n == 0 || off + n > px.len() {
                break;
            }
            if w.max(h) <= max_dim {
                got.push((w, h, px[off..off + n].to_vec()));
            }
            off += n;
            if w == 1 && h == 1 {
                break;
            }
            (w, h) = ((w / 2).max(1), (h / 2).max(1));
        }
    }
    got.sort_by_key(|l| std::cmp::Reverse(l.0.max(l.1)));
    got.dedup_by_key(|l| (l.0, l.1));
    let Some(top) = got.first().cloned() else {
        return Err(format!("{:016x}: no group of it in the install", img.hash));
    };
    // The chain: each level half the one above it, no gap.
    let mut chain = vec![top.clone()];
    for l in got.into_iter().skip(1) {
        let prev = chain.last().map_or((1, 1), |p| (p.0, p.1));
        if l.0 == (prev.0 / 2).max(1) && l.1 == (prev.1 / 2).max(1) {
            chain.push(l);
        }
    }
    Ok(SceneImage { hash: img.hash, dxgi, width: top.0, height: top.1, levels: chain.into_iter().map(|l| l.2).collect() })
}

fn worldspawn_value(text: &str, key: &str) -> Option<String> {
    let block = text.split('}').next()?;
    let line = block.lines().find(|l| l.trim().starts_with(&format!("\"{key}\"")))?;
    line.trim().split('"').nth(3).map(str::to_owned)
}

fn worldspawn_vec(text: &str, key: &str) -> Option<[f32; 3]> {
    let v: Vec<f32> = worldspawn_value(text, key)?.split_whitespace().filter_map(|x| x.parse().ok()).collect();
    (v.len() == 3 && v.iter().all(|x| x.is_finite())).then(|| [v[0], v[1], v[2]])
}

/// The light and air of a read map.
fn environment(map: &JupMap, report: &mut Vec<String>) -> Env {
    let sunlight = worldspawn_value(&map.entities, "sunlight").and_then(|v| v.trim().parse::<f32>().ok());
    // (A `sunlight` of 0 is a map that leaves the sun to its vision set, not a night: Shipment, a
    // day map, was drawn as one with a 0.05 sun.)
    let night = sunlight.is_some_and(|v| v > 0.0 && v <= 0.05);
    let toward = map.atmosphere.as_ref().and_then(|a| a.toward_sun()).or_else(|| {
        worldspawn_vec(&map.entities, "sundirection").map(|a| {
            let (p, y) = (a[0].to_radians(), a[1].to_radians());
            [p.cos() * y.cos(), p.cos() * y.sin(), -p.sin()]
        })
    });
    let toward = toward.unwrap_or_else(|| {
        let (p, y) = ((-50.0f32).to_radians(), 35.0f32.to_radians());
        [p.cos() * y.cos(), p.cos() * y.sin(), -p.sin()]
    });
    // (Most maps keep their sun in the vision set and not on worldspawn; their stand-in is nearly
    // white, tinted afterwards by the sky picture. A warm default made every one of them orange.)
    let sun_given = worldspawn_vec(&map.entities, "suncolor").is_some();
    let colour = worldspawn_vec(&map.entities, "suncolor").unwrap_or([1.0, 0.99, 0.97]).map(|c| srgb_to_linear(c.clamp(0.0, 1.0)));
    let intensity = if night { 0.05 } else { sunlight.filter(|v| *v > 0.0).unwrap_or(DAY_SUN) };
    let intensity = std::env::var("BO2MP_JUP_SUN").ok().and_then(|v| v.trim().parse::<f32>().ok()).filter(|v| *v > 0.0).unwrap_or(intensity);
    let fog_scale = std::env::var("BO2MP_JUP_FOG").ok().and_then(|v| v.parse::<f32>().ok()).unwrap_or(1.0);
    let fog = map.atmosphere.as_ref().and_then(|a| a.fog).filter(|_| fog_scale > 0.0).map(|f| SceneFog {
        start: f.start.max(0.0),
        half: f.half.max(1.0),
        opacity: (f.opacity * fog_scale).clamp(0.0, 1.0),
        normal_scale: f.normal_scale.clamp(0.0, 1.0),
        colour: f.colour.map(|c| srgb_to_linear(c.clamp(0.0, 1.0))),
        sun_colour: f.sun_colour.map(|c| srgb_to_linear(c.clamp(0.0, 1.0))),
        sun_dir: f.sun_dir,
        sun_begin_deg: f.sun_begin_deg,
        sun_end_deg: f.sun_end_deg,
        base_height: f.base_height,
        half_height: f.half_height,
    });
    let horizon = fog.map_or([0.55, 0.62, 0.70], |f| f.colour);
    let (zenith, ambient) = if night { ([0.004, 0.006, 0.012], [0.012, 0.014, 0.02]) } else { ([0.12, 0.25, 0.55], [0.10, 0.12, 0.15]) };
    report.push(format!(
        "sun {:.2} {:.2} {:.2} colour {:.2} {:.2} {:.2} x {intensity:.2}{}; fog {}",
        toward[0],
        toward[1],
        toward[2],
        colour[0],
        colour[1],
        colour[2],
        if night { " (night)" } else { "" },
        fog.map_or("none".to_owned(), |f| format!("start {} half {} opacity {} normal scale {:.2} height {} half {} sun {} to {} deg, colour {:.2} {:.2} {:.2} toward the sun {:.2} {:.2} {:.2}", f.start, f.half, f.opacity, f.normal_scale, f.base_height, f.half_height, f.sun_begin_deg, f.sun_end_deg, f.colour[0], f.colour[1], f.colour[2], f.sun_colour[0], f.sun_colour[1], f.sun_colour[2]))
    ));
    Env {
        sun_dir: toward,
        sun_colour: colour.map(|c| c * intensity),
        sun_given,
        night,
        fog,
        ambient,
        sky_zenith: zenith,
        sky_horizon: horizon,
        amb_up: ambient,
        amb_side: ambient,
        amb_down: ambient,
        probe_reach: [600.0, 2400.0],
        probe_weight: 0.7,
    }
}

impl Scene {
    /// Reads the map the zone `zone` names (`<install>/zone/<map>.ff`).
    pub fn load(zone: &Path) -> Result<Self, String> {
        let src = MapSources::beside(zone);
        let oodle = fastfile_jup::oodle::load(&src.install).map_err(|e| format!("Modern Warfare III's Oodle: {e}"))?;
        Self::load_with(zone, oodle)
    }

    /// [`Self::load`] with the Oodle decoder given (the offline probes use a
    /// Linux one).
    pub fn load_with(zone: &Path, oodle: Box<dyn fastfile_jup::oodle::Decompressor>) -> Result<Self, String> {
        let started = std::time::Instant::now();
        let src = MapSources::beside(zone);
        // (The converted lane's read, when it is loading this zone: the zone is read once.)
        let map = match fastfile_jup::map::handover::wait(zone, std::time::Duration::from_secs(900)) {
            Some(m) => m,
            None => JupMap::load(&src, oodle.as_ref())?,
        };
        let mut report: Vec<String> = map.report.clone();
        report.push(format!("zone read in {:.1}s", started.elapsed().as_secs_f32()));
        let paks = map.packages.clone().unwrap_or_else(|| Packages::shared(&src.install));

        // Materials and pictures.
        let t_pics = std::time::Instant::now();
        let mut want: Vec<(usize, u32)> = Vec::new(); // (map image index, max dim)
        let mut layer_stats = (0usize, 0usize, 0usize); // as expected, other usage, layered materials
        let mut materials: Vec<SceneMaterial> = map
            .materials
            .iter()
            .enumerate()
            .map(|(mi, m)| {
                let l = crate::layers::classify(m, &map.image_usage, &map.images);
                // (`JUP_DBG_MAT=<material>[,...]`: the material's pictures in slot order, and the layers made of them.)
                if std::env::var("JUP_DBG_MAT").is_ok_and(|v| v.split(',').any(|x| x.trim().parse() == Ok(mi))) {
                    let pics: Vec<String> = m
                        .entries
                        .iter()
                        .map(|e| e.and_then(|i| map.images.get(i).map(|im| format!("{i}:{:016x}/{:#x}/{}x{}", im.hash, map.image_usage.get(i).copied().unwrap_or(0), im.width, im.height))).unwrap_or_else(|| "-".into()))
                        .collect();
                    report.push(format!("material {mi} {:016x}: entries [{}] -> {l:?}", m.hash, pics.join(" ")));
                    for i in m.entries.iter().flatten() {
                        let users: Vec<usize> = map.materials.iter().enumerate().filter(|(_, o)| o.entries.contains(&Some(*i))).map(|(k, _)| k).collect();
                        let world = map.surfaces.iter().filter(|s| s.material.is_some_and(|x| users.contains(&x))).count();
                        report.push(format!("  picture {i}: used by materials {users:?} ({world} world surfaces among them)"));
                    }
                }
                layer_stats.0 += usize::from(l.as_expected);
                layer_stats.1 += usize::from(l.other_usage);
                layer_stats.2 += usize::from(l.count > 1);
                for c in l.colour.iter().flatten() {
                    want.push((*c, COLOUR_MAX));
                }
                for n in l.normal.iter().flatten() {
                    want.push((*n, NORMAL_MAX));
                }
                for k in l.mask.iter().flatten() {
                    want.push((*k, MASK_MAX));
                }
                SceneMaterial { hash: m.hash, colour: l.colour, normal: l.normal, mask: l.mask, count: l.count, cutout: false, blend: false, water: false, tool: false, tint: crate::skylight::fallback_tint(m.hash), paint: None, second_uv: [false; 4], layer_tint: [[1.0; 3]; 4] }
            })
            .collect();
        // (A surface with no material of the map's: one grey one at the end.)
        if let Ok(v) = std::env::var("JUP_DBG_MAT") {
            for mi in v.split(',').filter_map(|x| x.trim().parse::<usize>().ok()) {
                let (mut n, mut with) = (0usize, 0usize);
                let mut hist = [[0usize; 8]; 4];
                for s in map.surfaces.iter().filter(|s| s.material == Some(mi)) {
                    n += 1;
                    with += usize::from(!s.extra.is_empty());
                    for w in &s.extra {
                        for (b, h) in hist.iter_mut().enumerate() {
                            h[((w >> (8 * b)) & 0xff) as usize / 32] += 1;
                        }
                    }
                }
                report.push(format!("material {mi}: {n} surfaces, {with} with the extra stream; its bytes in eighths {hist:?}"));
            }
        }
        let default_material = materials.len() as u32;
        materials.push(SceneMaterial { tint: [0.22; 3], ..Default::default() });
        want.sort_unstable();
        want.dedup_by_key(|w| w.0);
        {
            let mut hist: std::collections::BTreeMap<(u8, u8), usize> = std::collections::BTreeMap::new();
            for w in &want {
                *hist.entry((map.image_usage.get(w.0).copied().unwrap_or(0), map.images[w.0].format)).or_default() += 1;
            }
            report.push(format!("picture (usage, format) counts: {}", hist.iter().map(|((u, f), n)| format!("{u:#x}/{f:#x} {n}")).collect::<Vec<_>>().join(", ")));
        }
        let next = std::sync::atomic::AtomicUsize::new(0);
        let results: std::sync::Mutex<Vec<(usize, Result<SceneImage, String>)>> = std::sync::Mutex::new(Vec::new());
        let threads = std::thread::available_parallelism().map_or(4, |n| n.get().clamp(2, 8));
        std::thread::scope(|s| {
            for _ in 0..threads {
                s.spawn(|| {
                    loop {
                        let k = next.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                        let Some(&(img, max)) = want.get(k) else { break };
                        let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| mip_chain(&paks, &map.images[img], max, oodle.as_ref())))
                            .unwrap_or_else(|_| Err("the picture read panicked".into()));
                        if let Ok(mut out) = results.lock() {
                            out.push((img, r));
                        }
                    }
                });
            }
        });
        let mut images: Vec<SceneImage> = Vec::new();
        let mut by_map: std::collections::HashMap<usize, usize> = std::collections::HashMap::new();
        let mut failures = Vec::new();
        let mut fail_hist: std::collections::BTreeMap<(u8, u8, &str), usize> = std::collections::BTreeMap::new();
        let mut done = results.into_inner().unwrap_or_default();
        done.sort_by_key(|d| d.0);
        for (img, r) in done {
            match r {
                Ok(i) => {
                    by_map.insert(img, images.len());
                    images.push(i);
                }
                Err(e) => {
                    let mi = &map.images[img];
                    let why = if e.contains("no group") { "no data in the install" } else { "format not decoded" };
                    *fail_hist.entry((map.image_usage.get(img).copied().unwrap_or(0), mi.format, why)).or_default() += 1;
                    failures.push(e);
                }
            }
        }
        let blend_off = std::env::var("BO2MP_JUP_BLEND").is_ok_and(|v| v == "0");
        // (`BO2MP_JUP_TOOLS=1` draws the tool materials too, to compare.)
        let tools_visible = std::env::var("BO2MP_JUP_TOOLS").is_ok_and(|v| v == "1");
        let mut missing_colour = 0usize;
        let mut missing_normal = 0usize;
        let mut tilted = 0usize;
        let mut grey_read_failed = 0usize;
        let mut atlas_layers = 0usize;
        let mut not_albedo = 0usize;
        let second_set: std::collections::HashSet<usize> = map.surfaces.iter().filter(|s| !s.uvs2.is_empty()).filter_map(|s| s.material).collect();
        // (The k-th colour row's second colour, where it is on, multiplies layer k of
        // a world material. UNVERIFIED which colour of a row tints what: the roof
        // concrete's `81 7c 72` is the same on every material that layers it, and
        // with it a view matched to the reference screenshot measures as the
        // reference does. `BO2MP_JUP_TINT=0` off.)
        let world_set: std::collections::HashSet<usize> = map.surfaces.iter().filter_map(|s| s.material).collect();
        let tint_rows = std::env::var("BO2MP_JUP_TINT").map_or(true, |v| v != "0");
        let mut tinted = 0usize;
        let mut atlas_second = 0usize;
        for (mi, m) in materials.iter_mut().enumerate() {
            if tint_rows
                && world_set.contains(&mi)
                && let Some(mm) = map.materials.get(mi)
            {
                for (l, row) in mm.colour_rows.iter().take(4).enumerate() {
                    if row.3 {
                        m.layer_tint[l] = row.2.map(|c| f32::from(c) / 255.0);
                        tinted += 1;
                    }
                }
            }
            let had_colour = m.colour[0].is_some();
            for c in m.colour.iter_mut().chain(m.normal.iter_mut()).chain(m.mask.iter_mut()) {
                *c = c.and_then(|k| by_map.get(&k).copied());
            }
            grey_read_failed += usize::from(had_colour && m.colour[0].is_none());
            // (A colour picture far more saturated on average than any surface is a
            // packed mask under a colour usage: the stand-in albedo shows instead.)
            for l in 0..4 {
                if let Some(c) = m.colour[l]
                    && mean_rgb(&images[c]).is_some_and(|c| c[0].max(c[1]).max(c[2]) - c[0].min(c[1]).min(c[2]) > NOT_ALBEDO_CHROMA)
                {
                    m.colour[l] = if l == 0 { None } else { m.colour[0] };
                    not_albedo += 1;
                }
            }
            // (A packed normal map averages out to flat. One whose average leans far
            // over is another kind of picture under a normal map's usage byte.)
            for n in m.normal.iter_mut() {
                if n.is_some_and(|k| mean_tilt(&images[k]) > 0.25) {
                    *n = None;
                    tilted += 1;
                }
            }
            // (An upper layer whose colour is an atlas of separate things, a boot,
            // litter, repeats across a roof with the first texture coordinate set.
            // With the second (`BO2MP_JUP_UV2=1`, where the material's surfaces have
            // one) Highrise's boot atlas lands as boot-sole prints, but about ten
            // times a real print's size: a scale the game applies is not read. By
            // default the layer shows its base.)
            for l in 1..4 {
                if let Some(c) = m.colour[l]
                    && block_deviation(&images[c]).is_some_and(|d| d > ATLAS_DEVIATION)
                {
                    if second_set.contains(&mi) && std::env::var("BO2MP_JUP_UV2").is_ok_and(|v| v == "1") {
                        m.second_uv[l] = true;
                        atlas_second += 1;
                    } else {
                        m.colour[l] = m.colour[0];
                        m.normal[l] = m.normal[0];
                        atlas_layers += 1;
                    }
                }
            }
            if let Some(c) = m.colour[0] {
                let top = &images[c];
                let level0 = top.levels.first().map_or(&[][..], Vec::as_slice);
                m.cutout = bc1_cuts(top.dxgi, level0);
                m.blend = !blend_off && bc7_translucent(top.dxgi, level0);
                // (A tool material has one picture and no normal map or mask.)
                m.tool = !tools_visible && m.count <= 1 && m.normal.iter().all(Option::is_none) && m.mask.iter().all(Option::is_none) && tool_picture(top);
            }
        }
        if std::env::var("JUP_DBG").is_ok() {
            let mut sig:std::collections::BTreeMap<String, (usize, u64)> = std::collections::BTreeMap::new();
            for (mi, m) in materials.iter().take(map.materials.len()).enumerate() {
                if m.colour[0].is_some() {
                    continue;
                }
                let mm = &map.materials[mi];
                let mut parts: Vec<String> = mm.entries.iter().flatten().filter(|&&i| i < map.images.len() && u32::from(map.images[i].width) * u32::from(map.images[i].height) > 16).map(|&i| format!("{:x}/{:x}", map.image_usage.get(i).copied().unwrap_or(0), map.images[i].format)).collect();
                parts.dedup();
                let e = sig.entry(format!("{} colour {}", parts.join(" "), mm.colour.is_some())).or_insert((0, mm.hash));
                e.0 += 1;
            }
            let mut v: Vec<_> = sig.into_iter().collect();
            v.sort_by_key(|x| std::cmp::Reverse(x.1.0));
            for (k, (n, h)) in v.iter().take(25) {
                report.push(format!("grey signature: {n} x [{k}] e.g. {h:016x}"));
            }
        }
        for m in materials.iter().take(map.materials.len()) {
            missing_colour += usize::from(m.colour[0].is_none());
            missing_normal += usize::from(m.normal[0].is_none());
        }
        report.push(format!("materials grey because their colour picture could not be read: {grey_read_failed}"));
        report.push(format!("pictures not read (usage/format, why): {}", fail_hist.iter().map(|((u, f, w), n)| format!("{u:#x}/{f:#x} {w} {n}")).collect::<Vec<_>>().join(", ")));
        report.push(format!("normal maps dropped for leaning far from flat on average: {tilted}"));
        report.push(format!("colour pictures too saturated on average to be an albedo (a packed mask), not drawn as colour: {not_albedo}"));
        report.push(format!("upper layers whose colour is an atlas (block deviation over {ATLAS_DEVIATION}): {atlas_second} drawn with the second texture coordinate set, {atlas_layers} (no second set) shown as their base"));
        if tint_rows {
            report.push(format!("layer tints from the colour rows: {tinted} layers"));
        }
        report.push(format!("world: {} surfaces have a second texture coordinate set", map.surfaces.iter().filter(|s| !s.uvs2.is_empty()).count()));
        report.push(format!("blended materials (BC7 colour with translucent alpha): {}", materials.iter().filter(|m| m.blend).count()));
        report.push(format!(
            "tool materials (a flat picture with a caption, e.g. Caulk Shadow; shadow casters only, not drawn into the view): {} [{}]",
            materials.iter().filter(|m| m.tool).count(),
            materials.iter().filter(|m| m.tool).filter_map(|m| m.colour[0]).map(|c| format!("{:016x}", images[c].hash)).collect::<std::collections::BTreeSet<_>>().into_iter().collect::<Vec<_>>().join(" ")
        ));
        report.push(format!(
            "layers: {} slots held the expected picture usage, {} held another; {} materials blend more than one layer",
            layer_stats.0, layer_stats.1, layer_stats.2
        ));
        report.push(format!(
            "pictures: {} read ({} materials, {missing_colour} without a colour picture, {missing_normal} without a normal map) from {} packages in {:.1}s{}",
            images.len(),
            map.materials.len(),
            paks.len(),
            t_pics.elapsed().as_secs_f32(),
            failures.first().map(|e| format!("; first missing: {e}")).unwrap_or_default()
        ));

        // The world.
        let mut vertices: Vec<Vertex> = Vec::new();
        let mut indices: Vec<u32> = Vec::new();
        let mut draws: Vec<Draw> = Vec::new();
        let mut instances = vec![Instance::IDENTITY];
        let (mut lo, mut hi) = ([f32::MAX; 3], [f32::MIN; 3]);
        let mut buckets: std::collections::BTreeMap<(u32, i32, i32), Vec<usize>> = std::collections::BTreeMap::new();
        let mut sheets = 0usize;
                let mut water_surfaces: Vec<usize> = Vec::new();
        let mut boxes = 0usize;
        for (k, s) in map.surfaces.iter().enumerate() {
            if s.tris.is_empty() || s.positions.is_empty() {
                continue;
            }
            if reference_box(s) {
                boxes += 1;
                continue;
            }
            let extent = |a: usize| s.positions.iter().fold((f32::MAX, f32::MIN), |(l, h), p| (l.min(p[a]), h.max(p[a])));
            let (ex, ey, ez) = (extent(0), extent(1), extent(2));
            if (ex.1 - ex.0 > SHEET || ey.1 - ey.0 > SHEET) && ez.1 - ez.0 > 1.0 {
                sheets += 1;
                if std::env::var("JUP_DBG").is_ok() {
                    eprintln!("far sheet {k}: x {:.0}..{:.0} y {:.0}..{:.0} z {:.0}..{:.0}, {} tris, material {:?}", ex.0, ex.1, ey.0, ey.1, ez.0, ez.1, s.tris.len(), s.material);
                }
                if std::env::var("BO2MP_JUP_SHEETS").is_err() {
                    continue;
                }
            }
            let wide = ex.1 - ex.0 > SHEET || ey.1 - ey.0 > SHEET;
            if wide && map.has_water {
                water_surfaces.push(k);
                continue;
            }
            let cell = |a: (f32, f32)| (((a.0 + a.1) * 0.5) / CELL).floor() as i32;
            buckets.entry((s.material.map_or(default_material, |m| m as u32), cell(ex), cell(ey))).or_default().push(k);
        }
        // How many texture coordinate units a world unit takes on each material
        // (the median over its triangles that have an area in both). Triangles whose
        // coordinates lie on a line (the zone's second coordinate set is not read) are
        // mapped by their plane at that density instead of streaking one texel row.
        let mut samples: std::collections::HashMap<u32, Vec<f32>> = Default::default();
        for s in &map.surfaces {
            let Some(m) = s.material else { continue };
            for t in s.tris.iter().step_by(3) {
                // (Only triangles whose mapping is sound set the density: a sliver or a
                // sheared one reads far too small, and put a whole wall on one texel.)
                if let Some((uva, wa)) = tri_areas(&s.positions, &s.uvs, t)
                    && uva > 1e-9
                    && wa > 100.0
                    && uv_singular(&s.positions, &s.uvs, t).is_some_and(|(hi, lo, _)| lo >= PLANAR_RATIO * hi)
                {
                    samples.entry(m as u32).or_default().push((uva / wa).sqrt());
                }
            }
        }
        let density: std::collections::HashMap<u32, f32> = samples
            .into_iter()
            .filter(|(_, v)| v.len() >= 3)
            .map(|(m, mut v)| {
                v.sort_by(f32::total_cmp);
                (m, v[v.len() / 2].clamp(MIN_DENSITY, MAX_DENSITY))
            })
            .collect();
        if std::env::var("JUP_DBG_DENSITY").is_ok() {
            // World units a texture repeat spans, per material, and its picture's size.
            let mut rows: Vec<(f32, u32)> = density.iter().map(|(m, d)| (1.0 / d, *m)).collect();
            rows.sort_by(|a, b| a.0.total_cmp(&b.0));
            let q = |f: f32| rows.get(((rows.len() as f32 - 1.0) * f) as usize).map_or(0.0, |r| r.0);
            report.push(format!("density: world units per texture repeat over {} materials: 10% {:.0}, 25% {:.0}, median {:.0}, 75% {:.0}, 90% {:.0}", rows.len(), q(0.1), q(0.25), q(0.5), q(0.75), q(0.9)));
            for (u, m) in rows.iter().filter(|r| [543u32, 382, 155, 435, 1528].contains(&r.1)) {
                report.push(format!("density: material {m}: {u:.1} units per repeat"));
            }
            // Per surface of a few materials: the units per repeat of each coordinate set.
            for (k, s) in map.surfaces.iter().enumerate().filter(|(_, s)| s.material.is_some_and(|m| [543usize, 382, 155].contains(&m))) {
                let per = |uvs: &[[f32; 2]]| {
                    let mut v: Vec<f32> = s.tris.iter().filter_map(|t| tri_areas(&s.positions, uvs, t)).filter(|(ua, wa)| *ua > 1e-9 && *wa > 100.0).map(|(ua, wa)| (wa / ua).sqrt()).collect();
                    v.sort_by(f32::total_cmp);
                    v.get(v.len() / 2).copied().unwrap_or(0.0)
                };
                report.push(format!("density: surface {k} material {:?}: {} verts, uv {:.1}, second set {:.1} units per repeat ({} second-set coords)", s.material, s.positions.len(), per(&s.uvs), if s.lightmap_uvs.len() == s.positions.len() { per(&s.lightmap_uvs) } else { 0.0 }, s.lightmap_uvs.len()));
            }
        }
        let mut planar_tris = 0usize;
        let skip_planar = std::env::var("BO2MP_JUP_PLANAR").is_ok_and(|v| v == "0");
        // (`BO2MP_JUP_VERTEX_BLEND=0`: the layers blend by their masks alone, as before.)
        let no_vertex_blend = std::env::var("BO2MP_JUP_VERTEX_BLEND").is_ok_and(|v| v == "0");
        for ((material, _, _), list) in &buckets {
            let first_index = indices.len() as u32;
            let (mut blo, mut bhi) = ([f32::MAX; 3], [f32::MIN; 3]);
            for &k in list {
                let s = &map.surfaces[k];
                let corner = corner_normals(&s.positions, &s.tris);
                for (t, tri) in s.tris.iter().enumerate() {
                    if !tri.iter().all(|&i| usize::from(i) < s.positions.len()) {
                        continue;
                    }
                    let planar = uv_singular(&s.positions, &s.uvs, tri).is_some_and(|(hi, lo, wa)| wa > 100.0 && lo < PLANAR_RATIO * hi.max(1e-9));
                    planar_tris += usize::from(planar);
                    // (A decal or glass triangle with its coordinates on a line has nothing
                    // to show: its plane mapping would stretch a strip over the floor.)
                    let overlay = materials.get(*material as usize).is_some_and(|m| m.cutout || m.blend);
                    if planar && (skip_planar || overlay) {
                        continue;
                    }
                    let per_unit = density.get(material).copied().unwrap_or(DEFAULT_DENSITY);
                    for (c, &i) in tri.iter().enumerate() {
                        let p = s.positions[usize::from(i)];
                        for a in 0..3 {
                            lo[a] = lo[a].min(p[a]);
                            hi[a] = hi[a].max(p[a]);
                            blo[a] = blo[a].min(p[a]);
                            bhi[a] = bhi[a].max(p[a]);
                        }
                        indices.push(vertices.len() as u32);
                        let n = corner[t][c];
                        let uv = if planar {
                            let ax = if n[0].abs() >= n[1].abs() && n[0].abs() >= n[2].abs() { 0 } else if n[1].abs() >= n[2].abs() { 1 } else { 2 };
                            let (a, b) = [(1, 2), (0, 2), (0, 1)][ax];
                            [p[a] * per_unit, p[b] * per_unit]
                        } else {
                            s.uvs.get(usize::from(i)).copied().unwrap_or_default()
                        };
                        // (The extra stream's word: byte 0 is 255 on every vertex that has
                        // one, so a word without it is not blend weights.)
                        let blend = s.extra.get(usize::from(i)).map(|w| w.to_le_bytes()).filter(|b| b[0] == 255 && !no_vertex_blend).unwrap_or_default();
                        let uv2 = if planar { uv } else { s.uvs2.get(usize::from(i)).copied().unwrap_or(uv) };
                        vertices.push(Vertex { pos: p, normal: n, uv, blend, uv2 });
                    }
                }
            }
            let index_count = indices.len() as u32 - first_index;
            if index_count > 0 {
                draws.push(Draw { material: *material, first_index, index_count, first_instance: 0, instances: 1, mins: blo, maxs: bhi, model: u32::MAX });
            }
        }
        report.push(format!("world: {planar_tris} triangles with no area in texture space mapped by their plane"));
        if !water_surfaces.is_empty() {
            let wm = materials.len() as u32;
            materials.push(SceneMaterial { hash: 0x7e44_a100_0000_0002, count: 1, water: true, tint: [0.2; 3], ..Default::default() });
            let first_index = indices.len() as u32;
            let (mut blo, mut bhi) = ([f32::MAX; 3], [f32::MIN; 3]);
            for &k in &water_surfaces {
                let s = &map.surfaces[k];
                let base = vertices.len() as u32;
                for (i, p) in s.positions.iter().enumerate() {
                    for a in 0..3 {
                        blo[a] = blo[a].min(p[a]);
                        bhi[a] = bhi[a].max(p[a]);
                    }
                    let uv = s.uvs.get(i).copied().unwrap_or_default();
                    vertices.push(Vertex { pos: *p, normal: [0.0, 0.0, 1.0], uv, blend: [0; 4], uv2: uv });
                }
                for t in &s.tris {
                    if t.iter().all(|&i| usize::from(i) < s.positions.len()) {
                        indices.extend(t.iter().map(|&i| base + u32::from(i)));
                    }
                }
            }
            let index_count = indices.len() as u32 - first_index;
            if index_count > 0 {
                draws.push(Draw { material: wm, first_index, index_count, first_instance: 0, instances: 1, mins: blo, maxs: bhi, model: u32::MAX });
            }
            report.push(format!("water: {} flat sheet surfaces (the sea) drawn with the water shader", water_surfaces.len()));
        } else if !map.has_water {
            report.push("water: the map has no water asset; flat far sheets are ground".to_owned());
        }
        let world_surfaces = buckets.values().map(Vec::len).sum();
        let world_draws = draws.len();
        report.push(format!(
            "world: {world_surfaces} surfaces in {} draws (cells of {CELL} units, culled by the host), {} vertices, {} triangles ({sheets} far sheets and {boxes} reference boxes left out)",
            world_draws,
            vertices.len(),
            indices.len() / 3
        ));

        // The placed models: each mesh once, an instance per placement.
        let t_models = std::time::Instant::now();
        let mut per_model: Vec<Vec<usize>> = vec![Vec::new(); map.prop_models.len()];
        for (k, p) in map.props.iter().enumerate() {
            if let Some(v) = per_model.get_mut(p.model) {
                v.push(k);
            }
        }
        let mut model_draws = 0usize;
        let mut placements = 0usize;
        // (`BO2MP_JUP_HIDE_MODELS=a,b`: leave out models whose name contains any of them.)
        let hide: Vec<String> = std::env::var("BO2MP_JUP_HIDE_MODELS").map(|v| v.split(',').map(str::to_owned).filter(|s| !s.is_empty()).collect()).unwrap_or_default();
        let mut too_big: Vec<String> = Vec::new();
        let mut white_base: std::collections::HashMap<usize, bool> = Default::default();
        let mut painted: std::collections::HashMap<(u32, [u32; 3]), u32> = Default::default();
        // (`BO2MP_JUP_PAINT=0`: models' tint bases stay white.)
        let no_paint = std::env::var("BO2MP_JUP_PAINT").is_ok_and(|v| v == "0");
        for (mi, model) in map.prop_models.iter().enumerate() {
            let list = &per_model[mi];
            if list.is_empty() || hide.iter().any(|h| model.name.contains(h.as_str())) {
                continue;
            }
            // (A mesh reaching past 12,000 units from its origin is a far vista
            // cluster: Highrise's `us_periph_vista_cluster01_*` and
            // `vista_highrise_skidrow_far` reach 17,000 to 30,000 and, placed where
            // the zone places them, bury the rooftops under brown boxes. The game
            // draws them far off by a means not read here; they are left out.)
            let mesh_reach = model.surfaces.iter().flat_map(|s| s.positions.iter()).fold(0f32, |r, p| r.max((p[0] * p[0] + p[1] * p[1] + p[2] * p[2]).sqrt()));
            if mesh_reach > FAR_VISTA_REACH && std::env::var("BO2MP_JUP_FAR_VISTAS").is_err() {
                too_big.push(format!("{} ({mesh_reach:.0})", model.name));
                continue;
            }
            let first_instance = instances.len() as u32;
            let (mut plo, mut phi) = ([f32::MAX; 3], [f32::MIN; 3]);
            let mut reach = 0.0f32;
            for s in &model.surfaces {
                for p in &s.positions {
                    reach = reach.max((p[0] * p[0] + p[1] * p[1] + p[2] * p[2]).sqrt());
                }
            }
            for &k in list {
                let p = &map.props[k];
                for a in 0..3 {
                    plo[a] = plo[a].min(p.origin[a] - reach * p.scale.abs());
                    phi[a] = phi[a].max(p.origin[a] + reach * p.scale.abs());
                }
                instances.push(Instance {
                    a0: [p.axis[0][0], p.axis[0][1], p.axis[0][2], p.scale],
                    a1: [p.axis[1][0], p.axis[1][1], p.axis[1][2], 0.0],
                    a2: [p.axis[2][0], p.axis[2][1], p.axis[2][2], 0.0],
                    origin: [p.origin[0], p.origin[1], p.origin[2], 0.0],
                });
                for a in 0..3 {
                    lo[a] = lo[a].min(p.origin[a]);
                    hi[a] = hi[a].max(p.origin[a]);
                }
            }
            placements += list.len();
            let paint = paint_of(&model.name);
            for s in &model.surfaces {
                if s.tris.is_empty() || s.positions.is_empty() {
                    continue;
                }
                let base = vertices.len() as u32;
                let normals = normals_of(&s.positions, &s.tris);
                for (i, p) in s.positions.iter().enumerate() {
                    let uv = s.uvs.get(i).copied().unwrap_or_default();
                    vertices.push(Vertex { pos: *p, normal: normals[i], uv, blend: [0; 4], uv2: uv });
                }
                let first_index = indices.len() as u32;
                for t in &s.tris {
                    if t.iter().all(|&i| usize::from(i) < s.positions.len()) {
                        indices.extend(t.iter().map(|&i| base + u32::from(i)));
                    }
                }
                let index_count = indices.len() as u32 - first_index;
                if index_count > 0 {
                    // (A near-white tint base is painted: one material per (material, paint).)
                    let mut material = s.material.map_or(default_material, |m| m as u32);
                    let base_white = materials.get(material as usize).and_then(|m| m.colour[0]).is_some_and(|c| *white_base.entry(c).or_insert_with(|| mean_brightness(&images[c]).is_some_and(|b| b > TINT_BASE)));
                    if base_white && !no_paint {
                        let key = (material, paint.map(f32::to_bits));
                        material = *painted.entry(key).or_insert_with(|| {
                            let mut m = materials[material as usize].clone();
                            m.paint = Some(paint);
                            materials.push(m);
                            materials.len() as u32 - 1
                        });
                    }
                    draws.push(Draw { material, first_index, index_count, first_instance, instances: list.len() as u32, mins: plo, maxs: phi, model: mi as u32 });
                    model_draws += 1;
                }
            }
        }
        // (`JUP_DBG_FIND=a,b`: every placement of the models whose name contains one of them.)
        if let Ok(v) = std::env::var("JUP_DBG_FIND") {
            for (mi, model) in map.prop_models.iter().enumerate() {
                if v.split(',').any(|w| !w.is_empty() && model.name.contains(w)) {
                    for &k in &per_model[mi] {
                        let p = &map.props[k];
                        report.push(format!("find: {} at {:.0} {:.0} {:.0} scale {:.2} forward {:.2?} up {:.2?}", model.name, p.origin[0], p.origin[1], p.origin[2], p.scale, p.axis[0], p.axis[2]));
                    }
                }
            }
        }
        if std::env::var("JUP_DBG_MODELS").is_ok() {
            let mut rows: Vec<(f32, String)> = Vec::new();
            for (mi, model) in map.prop_models.iter().enumerate() {
                let list = &per_model[mi];
                if list.is_empty() {
                    continue;
                }
                let reach = model.surfaces.iter().flat_map(|s| s.positions.iter()).fold(0f32, |r, p| r.max((p[0] * p[0] + p[1] * p[1] + p[2] * p[2]).sqrt()));
                let scale = list.iter().map(|&k| map.props[k].scale.abs()).fold(0f32, f32::max);
                let tris: usize = model.surfaces.iter().map(|s| s.tris.len()).sum();
                let colour = model.surfaces.iter().filter(|s| s.material.and_then(|m| materials.get(m)).is_some_and(|m| m.colour[0].is_some())).count();
                let o = map.props[list[0]].origin;
                rows.push((reach * scale, format!("model {mi} {} reach {reach:.0} scale {scale:.2} x{} tris {tris} surfaces {} with colour {colour} at {:.0} {:.0} {:.0}", model.name, list.len(), model.surfaces.len(), o[0], o[1], o[2])));
            }
            rows.sort_by(|a, b| b.0.total_cmp(&a.0));
            for (_, r) in rows.iter().take(60) {
                report.push(r.clone());
            }
            // Placements whose mesh centre is far above the roofs over the play area.
            let mut floating = 0usize;
            for (mi, model) in map.prop_models.iter().enumerate() {
                let (mut lo, mut hi) = ([f32::MAX; 3], [f32::MIN; 3]);
                for p in model.surfaces.iter().flat_map(|s| s.positions.iter()) {
                    for a in 0..3 {
                        lo[a] = lo[a].min(p[a]);
                        hi[a] = hi[a].max(p[a]);
                    }
                }
                let c = [(lo[0] + hi[0]) * 0.5, (lo[1] + hi[1]) * 0.5, (lo[2] + hi[2]) * 0.5];
                for &k in &per_model[mi] {
                    let p = &map.props[k];
                    let w: Vec<f32> = (0..3).map(|a| p.origin[a] + p.scale * (c[0] * p.axis[0][a] + c[1] * p.axis[1][a] + c[2] * p.axis[2][a])).collect();
                    if w[2] > 4300.0 && w[0].abs() < 4500.0 && w[1].abs() < 4500.0 {
                        floating += 1;
                        if floating <= 40 {
                            report.push(format!(
                                "floating: model {mi} {} centre {c:.0?} at {:.0?} -> {:.0?} axes {:.2?} scale {:.2}",
                                model.name, p.origin, w, p.axis, p.scale
                            ));
                        }
                    }
                }
            }
            report.push(format!("floating placements: {floating}"));
        }
        if !too_big.is_empty() {
            report.push(format!("models left out as far vistas (mesh reach over {FAR_VISTA_REACH} units; BO2MP_JUP_FAR_VISTAS=1 draws them): {}", too_big.join(", ")));
        }
        report.push(format!("models: {} materials painted by the model's name (a near-white tint base, mean brightness over {TINT_BASE})", painted.len()));
        report.push(format!(
            "models: {} models placed {placements} times in {model_draws} draws (read in {:.1}s)",
            per_model.iter().filter(|l| !l.is_empty()).count(),
            t_models.elapsed().as_secs_f32()
        ));
        if let Some(terrain) = &map.terrain {
            let t_terrain = std::time::Instant::now();
            let vt = map.virtual_texture.as_ref();
            let picture = vt.and_then(|v| crate::terrain::picture(v, &paks, oodle.as_ref(), 4, &mut report));
            let mut material = SceneMaterial { hash: 0x7e44_a100_0000_0001, count: 1, tint: crate::skylight::fallback_tint(0x7e44_a100_0000_0001), ..Default::default() };
            if let Some(pic) = picture {
                material.colour[0] = Some(images.len());
                images.push(pic);
            }
            let material_index = materials.len() as u32;
            materials.push(material);
            let chunks = crate::terrain::add_mesh(&mut vertices, &mut indices, terrain, vt);
            for c in &chunks {
                for a in 0..3 {
                    lo[a] = lo[a].min(c.mins[a]);
                    hi[a] = hi[a].max(c.maxs[a]);
                }
                draws.push(Draw { material: material_index, first_index: c.first_index, index_count: c.index_count, first_instance: 0, instances: 1, mins: c.mins, maxs: c.maxs, model: u32::MAX });
            }
            report.push(format!("terrain: {} chunks over {} x {} heights {} units apart in {:.1}s", chunks.len(), terrain.size, terrain.size, terrain.cell, t_terrain.elapsed().as_secs_f32()));
        }

        // Light.
        let mut probes: Vec<Probe> = if std::env::var("BO2MP_JUP_PROBES").is_ok_and(|v| v == "0") {
            Vec::new()
        } else {
            map.lighting
                .probes
                .iter()
                .filter_map(|p| {
                    let m = p.load(map.packages.as_deref(), PROBE_SIZE as u32, oodle.as_ref())?;
                    let a = m.ambient();
                    // (Resampled to the host's fixed size.)
                    let radiance: Vec<[f32; 3]> = (0..PROBE_SIZE * PROBE_SIZE)
                        .map(|k| {
                            let (x, y) = (k % PROBE_SIZE, k / PROBE_SIZE);
                            let (sx, sy) = (x * m.size / PROBE_SIZE, y * m.size / PROBE_SIZE);
                            m.rgb.get(sy * m.size + sx).copied().unwrap_or([0.0; 3])
                        })
                        .collect();
                    Some(Probe { origin: p.origin, up: a.up, side: a.side, down: a.down, radiance })
                })
                .collect()
        };
        report.push(format!("probes: {} of {} reflection probes read for the ambient light", probes.len(), map.lighting.probes.len()));
        if std::env::var("JUP_DBG").is_ok() {
            for p in &probes {
                report.push(format!("probe at {:?}: up {:?} side {:?} down {:?}", p.origin, p.up, p.side, p.down));
            }
            report.push(format!("worldspawn: {}", map.entities.split('}').next().unwrap_or("").replace('\n', " ")));
        }
        let mut env = environment(&map, &mut report);
        let sky = if std::env::var("BO2MP_JUP_SKY").is_ok_and(|v| v == "0") || map.sky_images.is_empty() {
            None
        } else {
            crate::sky::build(&map.sky_images, &paks, oodle.as_ref(), env.sun_dir, 256, &mut report)
        };
        if lo[0] > hi[0] {
            (lo, hi) = ([0.0; 3], [0.0; 3]);
        }
        // A sun colour worldspawn does not give: nearly white, warmed or cooled by the colour of the
        // sky picture's sun disc (a compact one: an overcast sky has none) at 40 per cent.
        if !env.sun_given {
            if let Some(c) = sky.as_ref().filter(|c| c.sun_compact) {
                let intensity = env.sun_colour.iter().copied().fold(0.0f32, f32::max);
                let mix = std::env::var("BO2MP_JUP_SUN_TINT").ok().and_then(|v| v.trim().parse::<f32>().ok()).unwrap_or(SUN_TINT);
                let tint = c.sun_tint.map(|t| 1.0 - mix + mix * t);
                let top = tint[0].max(tint[1]).max(tint[2]);
                env.sun_colour = tint.map(|t| t / top * intensity);
                report.push(format!("sun colour from the sky's disc: {:.2} {:.2} {:.2}", env.sun_colour[0] / intensity, env.sun_colour[1] / intensity, env.sun_colour[2] / intensity));
            }
        }
        // Ambient light: the sky's and the sun's, then the probes brought to its level.
        {
            let (horizon, zenith) = (env.sky_horizon, env.sky_zenith);
            let sky_fn = |d: [f32; 3]| match &sky {
                Some(c) => crate::skylight::cube_lookup(c, d),
                // (The drawn gradient is saturated blue; as light it is a third as much so.)
                None => {
                    let c = crate::skylight::gradient_sky(horizon, zenith, d);
                    let l = 0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2];
                    c.map(|v| l + (v - l) * 0.35)
                }
            };
            // The sky picture's blue as light is not so blue: the ground and the buildings
            // bounce the sun back neutral (Elevated's ambient was 0.16 red to 0.75 blue).
            let sky_fn = |d: [f32; 3]| {
                let c = sky_fn(d);
                if sky.is_some() {
                    let l = 0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2];
                    c.map(|v| l + (v - l) * 0.4)
                } else {
                    c
                }
            };
            let open = crate::skylight::sky_light(&sky_fn, [0.0; 3]);
            let ground = crate::skylight::ground_bounce(env.sun_dir[2], env.sun_colour, open.up);
            let lit = crate::skylight::sky_light(&sky_fn, ground);
            // (The gradient is darker than a map's own sky picture; its ambient is lifted to the level of one.)
            let amb_scale = if sky.is_some() { AMBIENT_SCALE } else { AMBIENT_SCALE * 1.4 };
            let amb_scale = amb_scale * std::env::var("BO2MP_JUP_AMBIENT").ok().and_then(|v| v.trim().parse::<f32>().ok()).filter(|v| *v > 0.0).unwrap_or(1.0);
            let scaled = |c: [f32; 3]| c.map(|v| v * amb_scale);
            env.amb_up = scaled(lit.up);
            env.amb_side = scaled(lit.side);
            env.amb_down = scaled(lit.down);
            env.ambient = std::array::from_fn(|k| (env.amb_up[k] + env.amb_side[k] * 2.0 + env.amb_down[k]) * 0.25);
            let level = 0.2126 * env.ambient[0] + 0.7152 * env.ambient[1] + 0.0722 * env.ambient[2];
            if std::env::var("BO2MP_JUP_PROBE_RAW").is_err() {
                crate::skylight::normalise_probes(&mut probes, level);
            }
            let span = (hi[0] - lo[0]).max(hi[1] - lo[1]).max(1.0);
            env.probe_reach = [(0.02 * span).clamp(400.0, 1200.0), (0.1 * span).clamp(1500.0, 4000.0)];
            report.push(format!(
                "ambient: sky {} (up {:.2} {:.2} {:.2}, side {:.2} {:.2} {:.2}, down {:.2} {:.2} {:.2}); {} probes brought to level {level:.2}, reach {:.0} to {:.0}",
                if sky.is_some() { "cube" } else { "gradient" },
                env.amb_up[0], env.amb_up[1], env.amb_up[2],
                env.amb_side[0], env.amb_side[1], env.amb_side[2],
                env.amb_down[0], env.amb_down[1], env.amb_down[2],
                probes.len(), env.probe_reach[0], env.probe_reach[1]
            ));
        }
        let heights = if std::env::var("BO2MP_JUP_SKYVIS").is_ok_and(|v| v == "0") { None } else { build_heights(&map, &mut report) };
        report.push(match &map.grade {
            Some(g) => {
                // What the volume does to greys (encoded 0.25, 0.4, 0.55, 0.7): rgb out.
                let at = |t: f32| {
                    let i = (t * 31.0).round() as usize;
                    let o = ((i * 32 + i) * 32 + i) * 4;
                    [0, 1, 2].map(|c| g.rgba.get(o + c).map_or(0.0, |v| f32::from(*v) / 65535.0))
                };
                let grey: Vec<String> = [0.25f32, 0.4, 0.55, 0.7].iter().map(|&t| { let a = at(t); format!("{t:.2}->{:.2} {:.2} {:.2}", a[0], a[1], a[2]) }).collect();
                format!("grade: the map's colour grade volume {:016x} (32^3, log-encoded lookup); greys {}", g.hash, grey.join(" | "))
            }
            None => "grade: the map has no colour grade volume".to_owned(),
        });
        report.push(format!("scene ready in {:.1}s", started.elapsed().as_secs_f32()));
        Ok(Self {
            name: map.name.clone(),
            install: src.install,
            vertices,
            indices,
            instances,
            draws,
            materials,
            images,
            probes,
            env,
            sky,
            heights,
            entities: map.entities.clone(),
            grade: if std::env::var("BO2MP_JUP_GRADE").is_ok_and(|v| v == "0") { None } else { map.grade.as_ref().map(|g| g.rgba.clone()) },
            mins: lo,
            maxs: hi,
            world_surfaces,
            model_draws,
            model_names: map.prop_models.iter().map(|m| m.name.clone()).collect(),
            placements,
            report,
        })
    }
}
