//! bo2mp: Call of Duty: Modern Warfare III (2023, `jup`) multiplayer maps.
//!
//! `fastfile_jup::map` reads the map zone (`IWffa100`, inflated through the
//! game's own Oodle) into plain data: the world's surfaces, the entities.
//! This lays them out as a Black Ops III map read (`T7Map`) and builds the
//! match's world through the Black Ops III lane's builder: the world mesh
//! (IW3 builder), the zone's own collision (`col_map`; the drawn world where it has none),
//! entities with the spawn points the map needs (Modern Warfare III keeps
//! them out of its entities: `fastfile_jup::map::ents` makes them over the
//! map's path nodes), so Black Ops II's rules play it as an Infinity Ward
//! map.

use std::path::Path;

use asset_material::MaterialCatalog;
use asset_transport::progress::{LoadProgress, StageId};
use asset_world::WorldDrawPolicy;
use fastfile_jup::map::materials::Packages;
use fastfile_jup::map::{JupMap, MapSources, ents};
use fastfile_t7::map::{MapMaterial, MapModel, ModelSurface, StaticModel, T7Map, WorldSurface, WorldVertex};

/// A placed model is drawn out to its last LOD's distance, but never culled
/// nearer than this (a small prop's coarsest LOD can end at a few metres).
const PROP_NEAREST_CULL: f32 = 1500.0;
use fastfile_t7::texture::ImageLevels;

use super::LoadedWorld;
use crate::lane_capability::PreparedCapability;

/// Smooth normals for a triangle list (area-weighted). The front faces
/// wind clockwise seen from the front (the sea's sheets all do seen from
/// above).
fn normals_of(positions: &[[f32; 3]], tris: &[[u16; 3]]) -> Vec<[f32; 3]> {
    let mut n = vec![[0.0f32; 3]; positions.len()];
    for t in tris {
        let [a, b, c] = t.map(|i| positions[usize::from(i)]);
        let u = [c[0] - a[0], c[1] - a[1], c[2] - a[2]];
        let v = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
        let f = [
            u[1] * v[2] - u[2] * v[1],
            u[2] * v[0] - u[0] * v[2],
            u[0] * v[1] - u[1] * v[0],
        ];
        for &i in t {
            for k in 0..3 {
                n[usize::from(i)][k] += f[k];
            }
        }
    }
    n.into_iter()
        .map(|v| {
            let l = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
            if l > 1e-12 {
                v.map(|x| x / l)
            } else {
                [0.0, 0.0, 1.0]
            }
        })
        .collect()
}

/// A tangent for a normal (any vector across it).
fn tangent_of(n: [f32; 3]) -> [f32; 3] {
    let a = if n[2].abs() < 0.9 {
        [0.0, 0.0, 1.0]
    } else {
        [1.0, 0.0, 0.0]
    };
    let t = [
        a[1] * n[2] - a[2] * n[1],
        a[2] * n[0] - a[0] * n[2],
        a[0] * n[1] - a[1] * n[0],
    ];
    let l = (t[0] * t[0] + t[1] * t[1] + t[2] * t[2]).sqrt().max(1e-6);
    t.map(|x| x / l)
}

/// Where the map is played: a circle (middle, radius) on the ground plan.
#[derive(Clone, Copy, Debug)]
struct PlayArea {
    mid: [f32; 2],
    radius: f32,
}

impl PlayArea {
    /// Does a box (on the ground plan) come within the area?
    fn reaches(&self, lo: [f32; 3], hi: [f32; 3]) -> bool {
        let dx = (self.mid[0] - self.mid[0].clamp(lo[0], hi[0])).abs();
        let dy = (self.mid[1] - self.mid[1].clamp(lo[1], hi[1])).abs();
        dx * dx + dy * dy <= self.radius * self.radius
    }
}

/// A surface too big to stand on as one (a sea sheet, a far vista's
/// ground) spans more than this on the ground plan; it is not drawn.
const SHEET: f32 = 16384.0;

/// Past this many units from its origin a placed model's mesh is a far
/// vista cluster (as `render_jup::scene`'s).
const FAR_VISTA_REACH: f32 = 12_000.0;

/// A clear daylight sky as a cube (RGBA8 sRGB, faces +x -x +y -y +z -z,
/// z up): the zenith's blue down to a pale haze at the horizon, the sea's
/// haze below it, a soft glow toward the sun. (It stands in for the map's
/// own sky, [`super::jup_sky`], where that cannot be read.)
fn daylight_sky(size: u32, sun: [f32; 3]) -> ImageLevels {
    let zenith = [0.33f32, 0.55, 0.86];
    let horizon = [0.80f32, 0.86, 0.91];
    let below = [0.42f32, 0.52, 0.58];
    let glow = [1.0f32, 0.96, 0.86];
    let n = size as usize;
    let mut data = Vec::with_capacity(6 * n * n * 4);
    for face in 0..6 {
        for y in 0..n {
            for x in 0..n {
                let s = (x as f32 + 0.5) / size as f32 * 2.0 - 1.0;
                let t = (y as f32 + 0.5) / size as f32 * 2.0 - 1.0;
                let d = match face {
                    0 => [1.0, -t, -s],
                    1 => [-1.0, -t, s],
                    2 => [s, 1.0, t],
                    3 => [s, -1.0, -t],
                    4 => [s, -t, 1.0],
                    _ => [-s, -t, -1.0],
                };
                let l = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt();
                let d = d.map(|v| v / l);
                let up = d[2];
                let mut c: [f32; 3] = if up >= 0.0 {
                    let k = up.powf(0.45);
                    std::array::from_fn(|i| horizon[i] + (zenith[i] - horizon[i]) * k)
                } else {
                    let k = (-up).powf(0.35);
                    std::array::from_fn(|i| horizon[i] + (below[i] - horizon[i]) * k)
                };
                let toward = (d[0] * sun[0] + d[1] * sun[1] + d[2] * sun[2]).max(0.0);
                let g = toward.powf(24.0) * 0.6 + toward.powf(400.0) * 0.4;
                for i in 0..3 {
                    c[i] += (glow[i] - c[i]) * g.min(1.0);
                }
                data.extend(c.map(|v| (v.clamp(0.0, 1.0) * 255.0).round() as u8));
                data.push(255);
            }
        }
    }
    ImageLevels {
        format: 29,
        width: size,
        height: size,
        levels: 1,
        data,
        part: None,
    }
}

/// The sky drawn where the map's own cannot be read: a daylight cube, the
/// glow where the Black Ops III builder's default sun stands (60 degrees
/// up, toward +x +y).
fn default_sky() -> ImageLevels {
    daylight_sky(256, [0.3536, 0.3536, 0.866])
}

/// How far past the map's bounds the sea is drawn.
const SEA_MARGIN: f32 = 16384.0;

/// What lies wholly this far under the sea collides with nothing (a placed
/// model whose origin lies four times as deep).
const UNDERWATER: f32 = 48.0;

/// A flat sheet's triangles cut to a box on the ground plan (each
/// triangle clipped against the box's four sides, UVs carried along).
#[allow(clippy::type_complexity)]
fn clip_sheet(
    positions: &[[f32; 3]],
    uvs: &[[f32; 2]],
    tris: &[[u16; 3]],
    lo: [f32; 2],
    hi: [f32; 2],
) -> (Vec<[f32; 3]>, Vec<[f32; 2]>, Vec<[u16; 3]>) {
    type V = ([f32; 3], [f32; 2]);
    let (mut ps, mut ts, mut out) = (Vec::new(), Vec::new(), Vec::new());
    for t in tris {
        let mut poly: Vec<V> = t
            .iter()
            .map(|&i| (positions[usize::from(i)], uvs.get(usize::from(i)).copied().unwrap_or_default()))
            .collect();
        // Planes: inside when d >= 0.
        for (axis, bound, sign) in [(0, lo[0], 1.0f32), (0, hi[0], -1.0), (1, lo[1], 1.0), (1, hi[1], -1.0)] {
            let d = |v: &V| sign * (v.0[axis] - bound);
            let mut next = Vec::with_capacity(poly.len() + 2);
            for k in 0..poly.len() {
                let (a, b) = (poly[k], poly[(k + 1) % poly.len()]);
                let (da, db) = (d(&a), d(&b));
                if da >= 0.0 {
                    next.push(a);
                }
                if (da >= 0.0) != (db >= 0.0) {
                    let f = da / (da - db);
                    next.push((
                        std::array::from_fn(|c| a.0[c] + (b.0[c] - a.0[c]) * f),
                        std::array::from_fn(|c| a.1[c] + (b.1[c] - a.1[c]) * f),
                    ));
                }
            }
            poly = next;
            if poly.len() < 3 {
                break;
            }
        }
        if poly.len() < 3 || ps.len() + poly.len() > usize::from(u16::MAX) {
            continue;
        }
        let base = ps.len() as u16;
        for v in &poly {
            ps.push(v.0);
            ts.push(v.1);
        }
        for k in 1..poly.len() as u16 - 1 {
            out.push([base, base + k, base + k + 1]);
        }
    }
    (ps, ts, out)
}

/// Floor points when the map has no path nodes: the middles of the
/// world's upward triangles of a walkable size (not the sea's or a far
/// vista's huge sheets), around where most of them are (a big map's
/// streamed backdrop tiles out), above the lowest few (the sea bed); and
/// the play area they make.
fn floor_points(map: &JupMap) -> (Vec<[f32; 3]>, Option<PlayArea>) {
    let mut pts = Vec::new();
    for s in &map.surfaces {
        for t in &s.tris {
            let [a, b, c] = t.map(|i| s.positions[usize::from(i)]);
            let u = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
            let v = [c[0] - a[0], c[1] - a[1], c[2] - a[2]];
            let f = [
                u[1] * v[2] - u[2] * v[1],
                u[2] * v[0] - u[0] * v[2],
                u[0] * v[1] - u[1] * v[0],
            ];
            let area2 = (f[0] * f[0] + f[1] * f[1] + f[2] * f[2]).sqrt();
            if area2 > 2.0 * 32.0 * 32.0 && area2 < 2.0 * 512.0 * 512.0 && f[2] / area2 > 0.85 {
                pts.push(std::array::from_fn(|k| (a[k] + b[k] + c[k]) / 3.0));
            }
        }
    }
    if pts.len() < 10 {
        return (pts, None);
    }
    let median = |k: usize, pts: &[[f32; 3]]| {
        let mut v: Vec<f32> = pts.iter().map(|p| p[k]).collect();
        v.sort_by(f32::total_cmp);
        v[v.len() / 2]
    };
    // Around the middle: within 1.5 times the distance six in ten are in.
    let mid = [median(0, &pts), median(1, &pts)];
    let d2 = |p: &[f32; 3]| (p[0] - mid[0]).powi(2) + (p[1] - mid[1]).powi(2);
    let mut d: Vec<f32> = pts.iter().map(d2).collect();
    d.sort_by(f32::total_cmp);
    let reach = d[d.len() * 6 / 10] * 1.5 * 1.5;
    pts.retain(|p| d2(p) <= reach);
    // Above the lowest twentieth.
    let mut z: Vec<f32> = pts.iter().map(|p| p[2]).collect();
    z.sort_by(f32::total_cmp);
    let floor = z[z.len() / 20];
    pts.retain(|p| p[2] > floor);
    let area = PlayArea {
        mid,
        // (A little past the floor's edge: what borders it is solid too.)
        radius: reach.sqrt() * 1.25,
    };
    (pts, Some(area))
}

/// The map's own reflection probes (Modern Warfare III's maps carry no
/// lightmaps: the probes are their ambient light), each as the light it has
/// up, to the side and down. Only those whose picture is whole in the zone
/// or the packages; none when `BO2MP_JUP_PROBES=0`.
fn reflection_probes(
    map: &JupMap,
    oodle: &dyn fastfile_jup::oodle::Decompressor,
    report: &mut Vec<String>,
) -> Vec<([f32; 3], fastfile_jup::map::lighting::ProbeAmbient)> {
    if std::env::var("BO2MP_JUP_PROBES").is_ok_and(|v| v == "0") {
        return Vec::new();
    }
    let out: Vec<_> = map
        .lighting
        .probes
        .iter()
        .filter_map(|p| Some((p.origin, p.load(map.packages.as_deref(), 64, oodle)?.ambient())))
        .collect();
    report.push(format!(
        "jup probes: {} of {} reflection probes read for the ambient light",
        out.len(),
        map.lighting.probes.len()
    ));
    out
}

/// The map as a Black Ops III map read.
fn as_t7(
    map: &JupMap,
    probes: Vec<([f32; 3], fastfile_jup::map::lighting::ProbeAmbient)>,
    report: &mut Vec<String>,
) -> T7Map {
    let mut t7 = T7Map::default();
    t7.name = map.name.clone();
    let mut sheets = 0usize;
    let mut sea = 0usize;
    let mut sea_surfaces = Vec::new();
    // The sea's level (its sheets' height), where it has one.
    let mut sea_z = f32::MIN;
    // The bounds of the surfaces that are not sheets (where the sea is cut).
    let play = {
        let (mut lo, mut hi) = ([f32::MAX; 3], [f32::MIN; 3]);
        for s in &map.surfaces {
            let (mut slo, mut shi) = ([f32::MAX; 3], [f32::MIN; 3]);
            for p in &s.positions {
                for k in 0..3 {
                    slo[k] = slo[k].min(p[k]);
                    shi[k] = shi[k].max(p[k]);
                }
            }
            if shi[0] - slo[0] > SHEET || shi[1] - slo[1] > SHEET || s.positions.is_empty() {
                continue;
            }
            for k in 0..3 {
                lo[k] = lo[k].min(slo[k]);
                hi[k] = hi[k].max(shi[k]);
            }
        }
        (lo[0] <= hi[0]).then_some((lo, hi))
    };
    for s in &map.surfaces {
        if s.tris.is_empty()
            || s.positions.is_empty()
            || s.positions.len() > usize::from(u16::MAX)
            || s.tris.len() > usize::from(u16::MAX)
        {
            continue;
        }
        // A sheet wider than the play area (the sea, the sea bed, a far
        // vista's ground) is left out: it would stretch the world's bounds
        // (and the view's far plane with them) tens of times past the map
        // and lie over it in the depth buffer.
        let extent = |k: usize| {
            let (lo, hi) = s
                .positions
                .iter()
                .fold((f32::MAX, f32::MIN), |(lo, hi), p| {
                    (lo.min(p[k]), hi.max(p[k]))
                });
            hi - lo
        };
        // The sea is flat sheets at sea level: drawn, cut to the play
        // area's bounds and a margin; the sea bed and far ground are not.
        let clipped;
        let (positions, uvs, tris) = if extent(0) > SHEET || extent(1) > SHEET {
            if extent(2) > 1.0 || play.is_none() {
                sheets += 1;
                continue;
            }
            let (lo, hi) = play.unwrap_or_default();
            clipped = clip_sheet(&s.positions, &s.uvs, &s.tris, [lo[0] - SEA_MARGIN, lo[1] - SEA_MARGIN], [hi[0] + SEA_MARGIN, hi[1] + SEA_MARGIN]);
            if clipped.2.is_empty() || clipped.0.len() > usize::from(u16::MAX) {
                continue;
            }
            sea += 1;
            sea_surfaces.push(t7.surfaces.len());
            sea_z = sea_z.max(s.positions[0][2]);
            (&clipped.0, &clipped.1, &clipped.2)
        } else {
            (&s.positions, &s.uvs, &s.tris)
        };
        let normals = normals_of(positions, tris);
        let first_vertex = t7.vertices.len() as u32;
        let base_index = t7.indices.len() as u32;
        let mut lo = [f32::MAX; 3];
        let mut hi = [f32::MIN; 3];
        for (i, p) in positions.iter().enumerate() {
            for k in 0..3 {
                lo[k] = lo[k].min(p[k]);
                hi[k] = hi[k].max(p[k]);
            }
            let n = normals[i];
            t7.vertices.push(WorldVertex {
                xyz: *p,
                color: 0xFFFF_FFFF,
                uv: uvs.get(i).copied().unwrap_or_default(),
                normal: n,
                tangent: tangent_of(n),
            });
        }
        t7.indices.extend(tris.iter().flatten());
        t7.surfaces.push(WorldSurface {
            first_vertex,
            vertex_count: positions.len() as u32,
            tri_count: tris.len() as u16,
            base_index,
            material: s.material,
            flags: 0,
            mins: lo,
            maxs: hi,
        });
    }
    // The terrain (the ground between the buildings), drawn with the
    // material after the map's own (its virtual texture's picture, added
    // with the pictures).
    if let Some(terrain) = &map.terrain {
        let material = (!map.materials.is_empty()).then_some(map.materials.len());
        let added = super::jup_terrain::add_mesh(&mut t7, terrain, map.virtual_texture.as_ref(), material);
        report.push(format!(
            "jup terrain: {} surfaces over {}x{} heights {} units apart",
            added.len(),
            terrain.size,
            terrain.size,
            terrain.cell
        ));
    }
    // The sky: a daylight cube, the glow where the Black Ops III builder's
    // default sun stands (60 degrees up, toward +x +y).
    t7.sky_cube = Some(default_sky());
    // The placed models (first LODs), drawn out to their last LOD's
    // distance.
    for m in &map.prop_models {
        t7.models.push(MapModel {
            name: m.name.clone(),
            surfaces: m
                .surfaces
                .iter()
                .map(|s| ModelSurface {
                    normals: normals_of(&s.positions, &s.tris),
                    positions: s.positions.clone(),
                    uvs: s.uvs.clone(),
                    tris: s.tris.clone(),
                    material: s.material,
                })
                .collect(),
            coarse: m
                .coarse
                .iter()
                .map(|s| ModelSurface {
                    normals: Vec::new(),
                    positions: s.positions.clone(),
                    uvs: Vec::new(),
                    tris: s.tris.clone(),
                    material: None,
                })
                .collect(),
        });
    }
    // bo2mp (2026-10-28, as `render_jup::scene`): a model whose mesh reaches
    // past 12,000 units from its origin is a far vista cluster (Highrise's
    // `us_periph_vista_cluster01_*`, `jup_us_vista_highrise_skidrow_far_01`:
    // 17,000 to 30,000); placed where the zone places them they bury the
    // rooftops under brown boxes. The game draws them far off by a means
    // not read here; they are left out (`BO2MP_JUP_FAR_VISTAS=1` keeps them).
    let keep_far = std::env::var("BO2MP_JUP_FAR_VISTAS").is_ok_and(|v| v.trim() == "1");
    let far_vista: Vec<bool> = map
        .prop_models
        .iter()
        .map(|m| !keep_far && m.surfaces.iter().flat_map(|s| s.positions.iter()).any(|p| p[0] * p[0] + p[1] * p[1] + p[2] * p[2] > FAR_VISTA_REACH * FAR_VISTA_REACH))
        .collect();
    let mut far_left = 0usize;
    for p in &map.props {
        if far_vista.get(p.model).copied().unwrap_or(false) {
            far_left += 1;
            continue;
        }
        let far = map.prop_models[p.model].draw_distance * p.scale.max(1.0);
        t7.static_models.push(StaticModel {
            model: p.model,
            origin: p.origin,
            axis: p.axis,
            scale: p.scale,
            cull_dist: if far > 0.0 { far.max(PROP_NEAREST_CULL) } else { 0.0 },
        });
    }
    report.push(format!(
        "jup static models: {} models, {} placed ({far_left} placements of {} far vista models left out: mesh reach over {FAR_VISTA_REACH} units; BO2MP_JUP_FAR_VISTAS=1 keeps them)",
        t7.models.len(),
        t7.static_models.len(),
        far_vista.iter().filter(|&&f| f).count()
    ));
    // The bounds of what is drawn.
    let (mut lo, mut hi) = ([f32::MAX; 3], [f32::MIN; 3]);
    for s in &t7.surfaces {
        for k in 0..3 {
            lo[k] = lo[k].min(s.mins[k]);
            hi[k] = hi[k].max(s.maxs[k]);
        }
    }
    (t7.mins, t7.maxs) = if t7.surfaces.is_empty() {
        (map.mins, map.maxs)
    } else {
        (lo, hi)
    };
    report.push(format!(
        "jup world drawn: {} surfaces ({sheets} sheets wider than {SHEET} left out, {sea} sea sheets cut to the map), bounds {:?} {:?}",
        t7.surfaces.len(),
        t7.mins,
        t7.maxs
    ));
    let (ground, area) = floor_points(map);
    // Collision only where the map is played: not the backdrop beyond the
    // play area (a big map's far tiles).
    if let Some(area) = area {
        t7.no_collision = t7
            .surfaces
            .iter()
            .map(|s| !area.reaches(s.mins, s.maxs))
            .collect();
        let left = t7.no_collision.iter().filter(|&&n| n).count();
        // The sea's surface is not walked on.
        for &i in &sea_surfaces {
            if let Some(n) = t7.no_collision.get_mut(i) {
                *n = true;
            }
        }
        report.push(format!(
            "jup collision: play area {:.0} around ({:.0}, {:.0}); {left} of {} surfaces drawn only",
            area.radius,
            area.mid[0],
            area.mid[1],
            t7.surfaces.len()
        ));
    }
    if t7.no_collision.is_empty() && !sea_surfaces.is_empty() {
        t7.no_collision = (0..t7.surfaces.len()).map(|i| sea_surfaces.contains(&i)).collect();
    }
    // Under the sea nothing is stood on: what lies wholly below it (the sea
    // floor, sunken rocks) is drawn only, so one who falls in sinks to the
    // fall kill plane just under the island instead of walking off along
    // the bottom.
    let underwater = sea_z > f32::MIN;
    if underwater {
        let below = sea_z - UNDERWATER;
        let mut sunk = 0usize;
        for (i, s) in t7.surfaces.iter().enumerate() {
            if s.maxs[2] < below
                && let Some(n) = t7.no_collision.get_mut(i)
                && !*n
            {
                *n = true;
                sunk += 1;
            }
        }
        report.push(format!(
            "jup collision: sea at {sea_z:.0}; {sunk} surfaces wholly under it drawn only"
        ));
    }
    // Placed models collide where the map is played, but not foliage,
    // decals and effects' models; each as its coarse LOD.
    t7.static_no_collision = t7
        .static_models
        .iter()
        .map(|sm| {
            let name = &t7.models[sm.model].name;
            area.is_some_and(|a| !a.reaches(sm.origin, sm.origin))
                || (underwater && sm.origin[2] < sea_z - 4.0 * UNDERWATER)
                || ["foliage", "grass", "decal", "vfx", "fx_", "leaves", "weeds"]
                    .iter()
                    .any(|w| name.contains(w))
        })
        .collect();
    let colliding: usize = t7
        .static_models
        .iter()
        .zip(&t7.static_no_collision)
        .filter(|(_, n)| !**n)
        .map(|(sm, _)| {
            let m = &t7.models[sm.model];
            let s = if m.coarse.is_empty() { &m.surfaces } else { &m.coarse };
            s.iter().map(|s| s.tris.len()).sum::<usize>()
        })
        .sum();
    report.push(format!(
        "jup collision: {} of {} placed models may collide ({colliding} triangles before the size cut)",
        t7.static_no_collision.iter().filter(|&&n| !n).count(),
        t7.static_models.len()
    ));
    let mut floor = ents::path_nodes(&map.entities);
    // Only the nodes in the play area: the game's maps keep nodes of other
    // modes' stand-ins far out (Das Haus's and Afghan's at one corner, over
    // no ground at all), which no spawn should be made on.
    if let Some(area) = area {
        let all = floor.len();
        floor.retain(|n| area.reaches([n[0], n[1], n[2]], [n[0], n[1], n[2]]));
        if floor.len() < all {
            report.push(format!("jup spawns: {} of {all} path nodes lie outside the play area and are left out", all - floor.len()));
        }
    }
    let source = if floor.is_empty() {
        floor = ground;
        "the world's floor"
    } else {
        "the path nodes"
    };
    let spawns = ents::spawn_entities(&floor);
    report.push(format!(
        "jup spawns: made over {source} ({} points)",
        floor.len()
    ));
    let entities = if map.entities.trim().is_empty() {
        "{\n\"classname\" \"worldspawn\"\n}\n".to_owned()
    } else {
        map.entities.clone()
    };
    // The light: the map's sun (worldspawn's, else a late-morning one),
    // dimmed to leave room for the sky's, baked into the vertex colours.
    // (Most maps keep no sun on worldspawn: their lighting state holds it.
    // The vision set's sun-fog direction stands in for it there, else a
    // late-morning one.)
    let world_sun = worldspawn_vec(&entities, "sundirection");
    let vision_sun = map.atmosphere.as_ref().and_then(|a| a.toward_sun());
    let angles = world_sun
        .or_else(|| vision_sun.map(sun_angles))
        .unwrap_or(DEFAULT_SUN_ANGLES);
    report.push(format!(
        "jup sun: {} ({:.1} up, yaw {:.1})",
        if world_sun.is_some() {
            "worldspawn's"
        } else if vision_sun.is_some() {
            "the vision set's sun-fog direction"
        } else {
            "a default"
        },
        -angles[0],
        angles[1]
    ));
    let colour = worldspawn_vec(&entities, "suncolor").unwrap_or([1.0, 0.96, 0.9]);
    // A night map (worldspawn's `sunlight` 0: Shipment, a storm at night):
    // no sun, a dim cool moon, the storm's dark sky, and its lamps' warm
    // pools baked in colour.
    let night = is_night(&entities);
    let sun = if night { NIGHT_MOON } else { colour.map(|c| (c * SUN_SCALE).clamp(0.0, 2.0)) };
    let (pitch, yaw) = (angles[0].to_radians(), angles[1].to_radians());
    let toward_sun = [pitch.cos() * yaw.cos(), pitch.cos() * yaw.sin(), -pitch.sin()];
    let skip: Vec<bool> = (0..t7.surfaces.len()).map(|i| sea_surfaces.contains(&i)).collect();
    let lamps = if night { lamps_of(&t7) } else { Vec::new() };
    if night {
        let mut kinds: std::collections::BTreeMap<&str, usize> = Default::default();
        for sm in &t7.static_models {
            let n = t7.models[sm.model].name.as_str();
            if lamp_kind(n).is_some() {
                *kinds.entry(n).or_default() += 1;
            }
        }
        report.push(format!("jup lamps: {} from {:?}", lamps.len(), kinds));
    }
    super::jup_light::bake(
        &mut t7,
        &super::jup_light::BakeLight {
            toward_sun,
            sun,
            ambient: if night { NIGHT_AMBIENT } else { AMBIENT },
            lamps,
            night_page: night.then_some(NIGHT_PAGE),
            probes,
        },
        &skip,
        report,
    );
    t7.baked_ambient = Some(if night { NIGHT_PAGE } else { AMBIENT });
    if night {
        t7.sky_cube = Some(night_sky(256));
    }
    let mut entities = with_worldspawn_sun(&entities, angles, sun);
    // The vision set's fog rides on worldspawn (so the converted map's
    // cache holds it).
    if let Some(f) = map.atmosphere.as_ref().and_then(|a| a.fog).filter(|f| f.opacity > 0.01 && f.half > 1.0) {
        let v = [
            f.start, f.half, f.opacity, f.normal_scale, f.colour[0], f.colour[1], f.colour[2], f.sun_colour[0],
            f.sun_colour[1], f.sun_colour[2], f.sun_dir[0], f.sun_dir[1], f.sun_dir[2], f.sun_begin_deg,
            f.sun_end_deg, f.base_height, f.half_height,
        ];
        let text: Vec<String> = v.iter().map(|x| format!("{x}")).collect();
        if let Some(end) = entities.find('}') {
            entities.insert_str(end, &format!("\"{FOG_KEY}\" \"{}\"\n", text.join(" ")));
        }
    }
    // The sea's height on worldspawn: Modern Warfare III's movement swims in it.
    if sea_z > f32::MIN
        && let Some(end) = entities.find('}')
    {
        entities.insert_str(end, &format!("\"sealevel\" \"{sea_z}\"\n"));
    }
    t7.entities = format!("{entities}{spawns}");
    t7
}

/// The sea's height from worldspawn's `sealevel`, where the map has a sea.
fn sea_level_of(entities: &str) -> Option<f32> {
    let block = entities.split('}').next()?;
    let line = block.lines().find(|l| l.trim().starts_with("\"sealevel\""))?;
    line.trim().split('"').nth(3)?.trim().parse::<f32>().ok().filter(|z| z.is_finite())
}

/// Is the map lit at night (worldspawn's `sunlight` 0)?
fn is_night(entities: &str) -> bool {
    let Some(block) = entities.split('}').next() else { return false };
    block
        .lines()
        .find(|l| l.trim().starts_with("\"sunlight\""))
        .and_then(|l| l.trim().split('"').nth(3).and_then(|v| v.trim().parse::<f32>().ok()))
        .is_some_and(|v| v <= 0.05)
}

/// A night map's sky light (gamma space): the storm's dim blue-grey.
const NIGHT_AMBIENT: [f32; 3] = [0.13, 0.15, 0.19];
/// A night map's moon: faint and cool.
const NIGHT_MOON: [f32; 3] = [0.07, 0.08, 0.10];
/// The brightest light a night map's vertex colours carry (the page they
/// are a share of): a lamp's pool tops out here.
const NIGHT_PAGE: [f32; 3] = [1.0, 1.0, 1.0];

/// A lamp model's light: warm for the security and string lamps, cool white
/// for fluorescent tubes, a reach by its kind.
fn lamp_kind(name: &str) -> Option<([f32; 3], f32)> {
    let n = name.to_ascii_lowercase();
    if n.contains("_off") || n.contains("broken") || n.contains("for_lighting") || n.contains("pole") && !n.contains("lamp") {
        return None;
    }
    // (Only fittings: a model named for its lighting set but not a lamp,
    // a cable or a bracket, gives none.)
    if ["cable", "wire", "bracket", "mount", "box", "conduit", "switch", "panel", "base", "cover", "clamp", "veh", "heli"].iter().any(|w| n.contains(w)) {
        return None;
    }
    if n.contains("fluorescent") || n.contains("tube") {
        Some(([0.55, 0.62, 0.62], 380.0))
    } else if n.contains("security_lamp") || n.contains("lamp_withcage") || n.contains("cage") && n.contains("light") {
        Some(([0.95, 0.62, 0.30], 420.0))
    } else if n.contains("string_light") || n.contains("bulb") {
        Some(([0.80, 0.55, 0.30], 220.0))
    } else if n.contains("flood") || n.contains("spotlight") || n.contains("stadium") {
        Some(([0.85, 0.85, 0.80], 900.0))
    } else if n.contains("lighting_") || n.contains("light_fixture") || n.contains("lamp") || n.contains("_light_") {
        Some(([0.90, 0.66, 0.38], 360.0))
    } else {
        None
    }
}

/// The lamps among a night map's placed models: each lamp model's light
/// from just under its middle.
fn lamps_of(t7: &T7Map) -> Vec<super::jup_light::Lamp> {
    let kinds: Vec<Option<([f32; 3], f32, [f32; 3])>> = t7
        .models
        .iter()
        .map(|m| {
            let (colour, reach) = lamp_kind(&m.name)?;
            let (mut lo, mut hi) = ([f32::MAX; 3], [f32::MIN; 3]);
            for p in m.surfaces.iter().flat_map(|s| s.positions.iter()) {
                for k in 0..3 {
                    lo[k] = lo[k].min(p[k]);
                    hi[k] = hi[k].max(p[k]);
                }
            }
            let mid = if lo[0] > hi[0] { [0.0; 3] } else { std::array::from_fn(|k| 0.5 * (lo[k] + hi[k])) };
            Some((colour, reach, mid))
        })
        .collect();
    t7.static_models
        .iter()
        .filter_map(|sm| {
            let (colour, reach, mid) = kinds.get(sm.model).copied().flatten()?;
            let at: [f32; 3] = std::array::from_fn(|k| {
                sm.origin[k] + sm.scale * (mid[0] * sm.axis[0][k] + mid[1] * sm.axis[1][k] + mid[2] * sm.axis[2][k])
            });
            // (Just under the fitting, so its own housing does not shade it.)
            Some(super::jup_light::Lamp { at: [at[0], at[1], at[2] - 6.0], colour, reach: reach * sm.scale.max(0.5) })
        })
        .fold(Vec::<super::jup_light::Lamp>::new(), |mut kept, l| {
            // (Fittings within a hand's breadth of one another are one lamp:
            // a caged lamp is its cage, bulb and glass.)
            let near = kept.iter().any(|k| (0..3).map(|i| (k.at[i] - l.at[i]).powi(2)).sum::<f32>() < 40.0 * 40.0);
            if !near {
                kept.push(l);
            }
            kept
        })
}

/// A stormy night sky as a cube (RGBA8 sRGB, faces +x -x +y -y +z -z,
/// z up): heavy cloud, dark blue-grey, a little lighter at the horizon
/// (Modern Warfare III's Shipment at night in the rain).
fn night_sky(size: u32) -> ImageLevels {
    let zenith = [0.10f32, 0.12, 0.16];
    let horizon = [0.24f32, 0.28, 0.33];
    let below = [0.06f32, 0.08, 0.10];
    let n = size as usize;
    let mut data = Vec::with_capacity(6 * n * n * 4);
    // Cloud: a few octaves of value noise over the direction.
    let hash = |x: i32, y: i32, z: i32| -> f32 {
        let mut h = (x as u32).wrapping_mul(73_856_093) ^ (y as u32).wrapping_mul(19_349_663) ^ (z as u32).wrapping_mul(83_492_791);
        h ^= h >> 13;
        h = h.wrapping_mul(0x5bd1_e995);
        h ^= h >> 15;
        (h & 0xffff) as f32 / 65535.0
    };
    let noise = |p: [f32; 3]| -> f32 {
        let f = p.map(f32::floor);
        let t: [f32; 3] = std::array::from_fn(|k| {
            let u = p[k] - f[k];
            u * u * (3.0 - 2.0 * u)
        });
        let c = |dx: i32, dy: i32, dz: i32| hash(f[0] as i32 + dx, f[1] as i32 + dy, f[2] as i32 + dz);
        let l = |a: f32, b: f32, u: f32| a + (b - a) * u;
        l(
            l(l(c(0, 0, 0), c(1, 0, 0), t[0]), l(c(0, 1, 0), c(1, 1, 0), t[0]), t[1]),
            l(l(c(0, 0, 1), c(1, 0, 1), t[0]), l(c(0, 1, 1), c(1, 1, 1), t[0]), t[1]),
            t[2],
        )
    };
    for face in 0..6 {
        for y in 0..n {
            for x in 0..n {
                let s = (x as f32 + 0.5) / size as f32 * 2.0 - 1.0;
                let t = (y as f32 + 0.5) / size as f32 * 2.0 - 1.0;
                let d = match face {
                    0 => [1.0, -t, -s],
                    1 => [-1.0, -t, s],
                    2 => [s, 1.0, t],
                    3 => [s, -1.0, -t],
                    4 => [s, -t, 1.0],
                    _ => [-s, -t, -1.0],
                };
                let l = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt();
                let d = d.map(|v| v / l);
                let up = d[2];
                let mut c: [f32; 3] = if up >= 0.0 {
                    let k = up.powf(0.6);
                    std::array::from_fn(|i| horizon[i] + (zenith[i] - horizon[i]) * k)
                } else {
                    let k = (-up).powf(0.4);
                    std::array::from_fn(|i| horizon[i] + (below[i] - horizon[i]) * k)
                };
                if up > -0.05 {
                    // Cloud banks: lighter billows and darker gaps.
                    let p = [d[0] * 3.0, d[1] * 3.0, d[2] * 6.0];
                    let mut v = 0.0;
                    let mut a = 0.55;
                    let mut q = p;
                    for _ in 0..5 {
                        v += a * noise(q);
                        q = q.map(|x| x * 2.03 + 0.37);
                        a *= 0.5;
                    }
                    let cloud = (v - 0.45) * 0.35;
                    for ch in &mut c {
                        *ch = (*ch + cloud).max(0.02);
                    }
                }
                data.extend(c.map(|v| (v.clamp(0.0, 1.0) * 255.0).round() as u8));
                data.push(255);
            }
        }
    }
    ImageLevels { format: 29, width: size, height: size, levels: 1, data, part: None }
}

/// The sky's light where the map's lightmaps are not read (gamma space).
const AMBIENT: [f32; 3] = [0.40, 0.43, 0.50];
/// The sun's share of its colour (the sky lights the rest).
const SUN_SCALE: f32 = 0.78;
/// A sun for a map whose worldspawn has none: 50 degrees up, from the
/// south-west (pitch, yaw, roll).
const DEFAULT_SUN_ANGLES: [f32; 3] = [-50.0, 35.0, 0.0];

/// The worldspawn key that carries the vision set's fog (17 numbers).
const FOG_KEY: &str = "jup_fog";

/// The (pitch, yaw, 0) of a direction toward the sun.
fn sun_angles(d: [f32; 3]) -> [f32; 3] {
    [-d[2].clamp(-1.0, 1.0).asin().to_degrees(), d[1].atan2(d[0]).to_degrees(), 0.0]
}

/// The map's fog from worldspawn's [`FOG_KEY`], as the exponential fog the
/// renderer draws (`BO2MP_JUP_FOG=0` draws none).
fn fog_of(entities: &str) -> Option<asset_world::ExpFog> {
    if std::env::var("BO2MP_JUP_FOG").is_ok_and(|v| v == "0") {
        return None;
    }
    let block = entities.split('}').next()?;
    let line = block.lines().find(|l| l.trim().starts_with(&format!("\"{FOG_KEY}\"")))?;
    let v: Vec<f32> = line.trim().split('"').nth(3)?.split_whitespace().filter_map(|x| x.parse().ok()).collect();
    if v.len() != 17 || v.iter().any(|x| !x.is_finite()) {
        return None;
    }
    Some(asset_world::ExpFog {
        start_dist: v[0].max(0.0),
        halfway_dist: v[1],
        color_rgb: [v[4], v[5], v[6]].map(|c| c.clamp(0.0, 1.0)),
        max_opacity: v[2].clamp(0.0, 1.0),
        transition_time: 0.0,
        sun: Some(asset_world::SunFog {
            color_rgb: [v[7], v[8], v[9]].map(|c| c.clamp(0.0, 1.0)),
            sun_dir: [v[10], v[11], v[12]],
            begin_angle_deg: v[13],
            end_angle_deg: v[14],
            scale: v[3].clamp(0.0, 1.0),
        }),
        volumetric: (v[16] > 1.0).then_some(asset_world::VolFog {
            halfway_height: v[16],
            base_height: v[15],
            color_scale: 1.0,
        }),
    })
}

/// A vector key of the entity text's worldspawn (its first block).
fn worldspawn_vec(text: &str, key: &str) -> Option<[f32; 3]> {
    let block = text.split('}').next()?;
    let line = block.lines().find(|l| l.trim().starts_with(&format!("\"{key}\"")))?;
    let v: Vec<f32> = line
        .trim()
        .split('"')
        .nth(3)?
        .split_whitespace()
        .filter_map(|x| x.parse().ok())
        .collect();
    (v.len() == 3 && v.iter().all(|x| x.is_finite())).then(|| [v[0], v[1], v[2]])
}

/// The entity text with worldspawn's sun set to the one drawn.
fn with_worldspawn_sun(text: &str, angles: [f32; 3], colour: [f32; 3]) -> String {
    let Some(end) = text.find('}') else {
        return text.to_owned();
    };
    let (head, rest) = text.split_at(end);
    let mut out: String = head
        .lines()
        .filter(|l| {
            let t = l.trim();
            !t.starts_with("\"sundirection\"") && !t.starts_with("\"suncolor\"")
        })
        .map(|l| format!("{l}\n"))
        .collect();
    out.push_str(&format!(
        "\"sundirection\" \"{} {} {}\"\n\"suncolor\" \"{} {} {}\"\n",
        angles[0], angles[1], angles[2], colour[0], colour[1], colour[2]
    ));
    out.push_str(rest);
    out
}

/// The largest a picture is drawn at.
const PICTURE_MAX: u32 = 1024;
/// The terrain texture's finest quadtree level drawn (Rebirth: 16 tiles a
/// side, 3968 px, about 9 units a pixel).
const TERRAIN_LEVEL: u32 = 4;

/// Does a BC1 picture cut holes (a tenth of its blocks in the
/// punch-through mode)? Its material is then drawn alpha-tested.
fn bc1_cuts(dxgi: u32, data: &[u8]) -> bool {
    if dxgi != 71 || data.len() < 8 {
        return false;
    }
    let blocks = data.chunks_exact(8);
    let n = blocks.len();
    let holes = blocks
        .filter(|b| u16::from_le_bytes([b[0], b[1]]) <= u16::from_le_bytes([b[2], b[3]]))
        .count();
    holes * 10 >= n
}

/// What a map's materials are and the pictures they name (from the zone,
/// or from the converted map's cache).
struct Pictures<'a> {
    materials: &'a [fastfile_jup::map::materials::MapMaterial],
    images: &'a [fastfile_jup::map::materials::MapImage],
}

/// The materials as the Black Ops III builder reads them, each with its
/// colour picture's pixels (fetched from the install's packages, a few
/// threads at once). Materials keep the map's order, so a surface's
/// material index stays.
///
/// bo2mp (2026-10-28): a material whose picture is a tool picture (Caulk
/// Shadow and kin, `jup_texsub::tool_picture`) casts the sun's shadow but is
/// not drawn (its name gets `_shadowcaster`); one with no picture, one the
/// install cannot read and one read only at a small level take a stand-in
/// from Modern Warfare Remastered or Modern Warfare 2 where one is found
/// (`super::jup_texsub`, `BO2MP_JUP_SUB=0` off).
fn materials_into(
    zone: &Path,
    map: &Pictures<'_>,
    t7: &mut T7Map,
    paks: &Packages,
    oodle: &dyn fastfile_jup::oodle::Decompressor,
    report: &mut Vec<String>,
) {
    if map.materials.is_empty() {
        return;
    }
    let started = std::time::Instant::now();
    let wanted: Vec<usize> = map.materials.iter().filter_map(|m| m.colour).collect();
    let mut wanted_sorted = wanted.clone();
    wanted_sorted.sort_unstable();
    wanted_sorted.dedup();
    let next = std::sync::atomic::AtomicUsize::new(0);
    let results: std::sync::Mutex<
        Vec<(usize, Result<fastfile_jup::map::materials::Level, String>)>,
    > = std::sync::Mutex::new(Vec::new());
    let threads = std::thread::available_parallelism().map_or(4, |n| n.get().clamp(2, 8));
    std::thread::scope(|s| {
        for _ in 0..threads {
            s.spawn(|| {
                loop {
                    let k = next.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    let Some(&img) = wanted_sorted.get(k) else {
                        break;
                    };
                    let level = paks.level(&map.images[img], PICTURE_MAX, oodle);
                    if let Ok(mut r) = results.lock() {
                        r.push((img, level));
                    }
                }
            });
        }
    });
    let mut decoded = std::collections::HashMap::new();
    let mut failures = Vec::new();
    for (img, level) in results.into_inner().unwrap_or_default() {
        match level {
            Ok(l) => {
                decoded.insert(img, l);
            }
            Err(e) => failures.push(e),
        }
    }
    // Which materials the world or a placed model draws (only those want a
    // stand-in).
    let mut used = vec![false; map.materials.len()];
    for s in &t7.surfaces {
        if let Some(u) = s.material.and_then(|m| used.get_mut(m)) {
            *u = true;
        }
    }
    // (The world's own materials: their first layer's colour row tints them.)
    let world_used = used.clone();
    // bo2mp (2026-10-28, as `render_jup::scene`): a world material's first
    // colour row's second colour, where on, multiplies its picture (linear,
    // byte / 255): MW3's tint bases are near white and take their colour so
    // (the roof concrete's `81 7c 72`). `BO2MP_JUP_TINT=0` off.
    let tint_rows = std::env::var("BO2MP_JUP_TINT").map_or(true, |v| v.trim() != "0");
    let mut tinted = 0usize;
    for m in &t7.models {
        for s in &m.surfaces {
            if let Some(u) = s.material.and_then(|m| used.get_mut(m)) {
                *u = true;
            }
        }
    }
    let sub_on = super::jup_texsub::on();
    let low = super::jup_texsub::low_side();
    let mut needs = Vec::new();
    let (mut cut, mut tools) = (0usize, 0usize);
    let mut below: std::collections::BTreeMap<(u32, u32), usize> = Default::default();
    for (mi, m) in map.materials.iter().enumerate() {
        let mut mat = MapMaterial {
            name: format!("jup_{:016x}", m.hash),
            technique_set: String::new(),
            textures: Vec::new(),
            constants: Default::default(),
            bound: Vec::new(),
        };
        let row_tint = m
            .colour_rows
            .first()
            .filter(|r| tint_rows && world_used[mi] && r.3)
            .map(|r| r.2.map(|c| f32::from(c) / 255.0))
            .filter(|t| t.iter().any(|c| (c - 1.0).abs() > 0.03));
        if let Some((c, l)) = m.colour.and_then(|c| decoded.get(&c).map(|l| (c, l))) {
            let pic = ImageLevels {
                format: l.dxgi,
                width: l.width,
                height: l.height,
                levels: 1,
                data: l.data.clone(),
                part: None,
            };
            let full = u32::from(map.images[c].width.max(map.images[c].height));
            let have = l.width.max(l.height);
            if have < full {
                *below.entry((have, full)).or_default() += 1;
            }
            let small = have <= 256;
            let low_one = sub_on && used[mi] && have <= low && have < full;
            let rgba = if small || low_one { fastfile_t7::texture::level_rgba(&pic, 0) } else { None };
            if let Some((w, h, px)) = rgba.as_ref()
                && super::jup_texsub::tool_picture(*w, *h, px)
            {
                // (Drawn into the sun's shadow only, as the game does.)
                mat.name.push_str("_shadowcaster");
                tools += 1;
            } else {
                if bc1_cuts(l.dxgi, &l.data) {
                    mat.name.push_str("_alphatest");
                    cut += 1;
                }
                if low_one {
                    needs.push(super::jup_texsub::Need {
                        material: mi,
                        name: mat.name.clone(),
                        why: super::jup_texsub::Why::Low { have, full },
                        // (Its own colour as drawn: tinted by its row.)
                        // (Kept small: only its colour is read.)
                        own: rgba.map(|(w, h, px)| super::jup_texsub::shrink(w, h, px, 64)).map(|(w, h, mut px)| {
                            if let Some(t) = row_tint {
                                for p in px.chunks_exact_mut(4) {
                                    for k in 0..3 {
                                        p[k] = fastfile_t7::look::to_srgb(fastfile_t7::look::to_linear(p[k]) * t[k]);
                                    }
                                }
                            }
                            (w, h, px)
                        }),
                        tint: row_tint,
                    });
                }
            }
            let mut name = format!("jup_img_{:016x}", map.images[c].hash);
            let pic = match row_tint.filter(|_| !mat.name.ends_with("_shadowcaster")) {
                Some(t) => {
                    tinted += 1;
                    name.push_str(&format!("_m{mi}"));
                    super::jup_texsub::tint_levels(&pic, t)
                }
                None => pic,
            };
            t7.add_image(name.clone(), pic);
            mat.textures.push((0, name));
        } else if sub_on && used[mi] {
            let why = if m.colour.is_some() { super::jup_texsub::Why::NotRead } else { super::jup_texsub::Why::NoPicture };
            needs.push(super::jup_texsub::Need { material: mi, name: mat.name.clone(), why, own: None, tint: row_tint });
        }
        t7.materials.push(mat);
    }
    report.push(format!(
        "jup pictures: {} of {} colour pictures from {} packages in {:.1}s ({cut} materials alpha-tested, {tools} tool pictures drawn as shadow casters only, {tinted} world materials tinted by their colour row){}",
        decoded.len(),
        wanted_sorted.len(),
        paks.len(),
        started.elapsed().as_secs_f32(),
        failures
            .first()
            .map(|e| format!("; first missing: {e}"))
            .unwrap_or_default()
    ));
    if !below.is_empty() {
        report.push(format!(
            "jup pictures read below their full size (have, full: materials): {below:?} (a stand-in is looked for at {low} or less, BO2MP_JUP_SUB_LOW)"
        ));
    }
    // The stand-ins.
    // (A reader of another game's zone that fails takes the stand-ins with
    // it, never the map.)
    let subs = {
        let t7_ref: &T7Map = t7;
        let mut lines = Vec::new();
        let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| super::jup_texsub::substitute(zone, t7_ref, &needs, &mut lines)));
        report.append(&mut lines);
        r.unwrap_or_else(|_| {
            report.push("jup sub: reading the stand-ins failed; MW3's own pictures kept".to_owned());
            Vec::new()
        })
    };
    for sub in subs {
        let Some(mat) = t7.materials.get_mut(sub.material) else { continue };
        // (A stand-in may cut holes: its own alpha decides.)
        if !mat.name.ends_with("_alphatest") && sub.levels.format == 71 && bc1_cuts(71, &sub.levels.data) {
            mat.name.push_str("_alphatest");
        }
        mat.textures.clear();
        mat.textures.push((0, sub.key.clone()));
        t7.add_image(sub.key, sub.levels);
    }
}

/// bo2mp (2026-10-28, as `render_jup::scene`): a placed model's material
/// whose picture is a near-white tint base (mean brightness over
/// `jup_texsub::TINT_BASE`) is painted the colour the model's name states
/// (`jup_texsub::paint_of`; a mid grey without one): one material per
/// (material, paint), the model's surfaces moved onto it. Drawn white they
/// were the cream cranes, pipes and housings of the first renders.
/// `BO2MP_JUP_PAINT=0` off.
fn paint_models(t7: &mut T7Map, report: &mut Vec<String>) {
    if std::env::var("BO2MP_JUP_PAINT").is_ok_and(|v| v.trim() == "0") {
        return;
    }
    let mut white: std::collections::HashMap<String, bool> = Default::default();
    let mut painted: std::collections::HashMap<(usize, [u32; 3]), usize> = Default::default();
    let mut surfaces = 0usize;
    for mi in 0..t7.models.len() {
        let paint = super::jup_texsub::paint_of(&t7.models[mi].name);
        for si in 0..t7.models[mi].surfaces.len() {
            let Some(m) = t7.models[mi].surfaces[si].material else { continue };
            let Some(mat) = t7.materials.get(m) else { continue };
            if mat.name.ends_with("_shadowcaster") {
                continue;
            }
            let Some((_, key)) = mat.textures.first().cloned() else { continue };
            let is_white = *white.entry(key.clone()).or_insert_with(|| {
                t7.image(&key, PICTURE_MAX).ok().and_then(|l| super::jup_texsub::mean_brightness(&l)).is_some_and(|b| b > super::jup_texsub::TINT_BASE)
            });
            if !is_white {
                continue;
            }
            let k = (m, paint.map(f32::to_bits));
            let to = match painted.get(&k) {
                Some(&i) => i,
                None => {
                    let Ok(l) = t7.image(&key, PICTURE_MAX) else { continue };
                    let pkey = format!("{key}_paint{}", painted.len());
                    t7.add_image(pkey.clone(), super::jup_texsub::tint_levels(&l, paint));
                    let mut nm = t7.materials[m].clone();
                    nm.textures = vec![(0, pkey)];
                    t7.materials.push(nm);
                    let i = t7.materials.len() - 1;
                    painted.insert(k, i);
                    i
                }
            };
            t7.models[mi].surfaces[si].material = Some(to);
            surfaces += 1;
        }
    }
    report.push(format!(
        "jup paint: {} placed-model materials painted by the model's name ({surfaces} model surfaces; a near-white tint base, mean brightness over {}; BO2MP_JUP_PAINT=0 off)",
        painted.len(),
        super::jup_texsub::TINT_BASE
    ));
}

/// A zone that cannot be read: the match's gap, with the reason.
fn zone_gap(path: &Path, why: String, at: &'static str) -> LoadedWorld {
    diag::warn!(World, "bo2mp jup: {}: {why}", path.display());
    LoadedWorld::with_gap(WorldDrawPolicy::t5(), PreparedCapability::PreparedWorld, why, Some(at))
}

/// Reads the map's zones and converts them (`as_t7`, the light baked).
fn convert(
    path: &Path,
    src: &MapSources,
    oodle: &dyn fastfile_jup::oodle::Decompressor,
    progress: &LoadProgress,
    native: bool,
) -> Result<(super::jup_cache::Converted, Option<std::sync::Arc<Packages>>), LoadedWorld> {
    let stage = progress.begin_scoped(StageId::MapAssets, "modern warfare iii zone", None);
    let started = std::time::Instant::now();
    let read = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| JupMap::load(src, oodle)));
    let mut map = match read {
        Ok(Ok(map)) => {
            stage.done();
            map
        }
        Ok(Err(error)) => {
            stage.fail();
            return Err(zone_gap(path, format!("Modern Warfare III zone: {error}"), "assets::lane::jup_map::load_world/zone"));
        }
        Err(panic) => {
            stage.fail();
            let what = panic
                .downcast_ref::<&str>()
                .map(|s| (*s).to_owned())
                .or_else(|| panic.downcast_ref::<String>().cloned())
                .unwrap_or_else(|| "unknown error".to_owned());
            return Err(zone_gap(
                path,
                format!("Modern Warfare III zone could not be read: {what}"),
                "assets::lane::jup_map::load_world/panic",
            ));
        }
    };
    let mut report = vec![format!("jup zone: read in {:.1}s", started.elapsed().as_secs_f32())];
    report.extend(map.report.iter().map(|l| format!("jup {l}")));
    let stage = progress.begin_scoped(StageId::MapAssets, "modern warfare iii light", None);
    let probes = reflection_probes(&map, oodle, &mut report);
    let t7 = as_t7(&map, probes, &mut report);
    stage.done();
    // bo2mp: drawn by `render_jup`: the read is handed to it whole (it reads
    // the zone no second time); this lane keeps copies of the small parts
    // it still names.
    if native {
        let conv = super::jup_cache::Converted {
            t7,
            materials: map.materials.clone(),
            images: map.images.clone(),
            sky_images: Vec::new(),
            virtual_texture: None,
            has_terrain: map.terrain.is_some(),
            report,
        };
        let packages = map.packages.clone();
        fastfile_jup::map::handover::hand_over(path, map);
        return Ok((conv, packages));
    }
    let packages = map.packages.take();
    Ok((
        super::jup_cache::Converted {
            t7,
            materials: std::mem::take(&mut map.materials),
            images: std::mem::take(&mut map.images),
            sky_images: std::mem::take(&mut map.sky_images),
            virtual_texture: map.virtual_texture.take(),
            has_terrain: map.terrain.is_some(),
            report,
        },
        packages,
    ))
}

/// The map's collision as the zone has it (`fastfile_jup::map::clip`),
/// the clip tables built over it; `None` (the drawn world stands in) when
/// the zone cannot be read again or holds no collision.
fn real_collision(
    path: &Path,
    oodle: &dyn fastfile_jup::oodle::Decompressor,
    drawn: &T7Map,
    sea_z: Option<f32>,
    report: &mut Vec<String>,
) -> Option<asset_world::ClipCollision> {
    let started = std::time::Instant::now();
    let image = match fastfile_jup::map::read_image(path, oodle) {
        Ok(image) => image,
        Err(error) => {
            report.push(format!("jup clip: zone not read again ({error}); collision from the drawn world"));
            return None;
        }
    };
    let Some(clip) = fastfile_jup::map::clip::read(&image.bytes) else {
        report.push("jup clip: no collision in the zone; collision from the drawn world".to_owned());
        return None;
    };
    drop(image);
    let collision = super::jup_clip::best_collision(&clip, drawn, sea_z, report);
    report.push(format!("jup clip: read and built in {:.1}s", started.elapsed().as_secs_f32()));
    Some(collision)
}

/// What `prepare` hands the map load: the converted map (pictures, stand-ins
/// and sky in), the report so far, the game's Oodle, the cache keys and a
/// cached collision.
struct Prepared {
    conv: super::jup_cache::Converted,
    report: Vec<String>,
    oodle: Box<dyn fastfile_jup::oodle::Decompressor>,
    keys: Option<(String, String)>,
    cached_clip: Option<asset_world::ClipCollision>,
}

/// The map read (or taken from the cache) and converted, its pictures,
/// their stand-ins and its sky in; for `load_jup_world` and `jup_preview`.
fn prepare(path: &Path, progress: &LoadProgress, native: bool) -> Result<Prepared, LoadedWorld> {
    use super::jup_cache::{CLIP_KIND, MAP_KIND};
    let src = MapSources::beside(path);

    let mut report = vec![format!(
        "game: JUP / Modern Warfare III ({})",
        path.display()
    )];
    let oodle = match std::panic::catch_unwind(|| fastfile_jup::oodle::load(&src.install).map_err(|e| e.to_string())) {
        Ok(Ok(oodle)) => oodle,
        Ok(Err(error)) => {
            if native {
                fastfile_jup::map::handover::cancel(path);
            }
            return Err(zone_gap(path, format!("Modern Warfare III zone: {error}"), "assets::lane::jup_map::load_world/zone"));
        }
        Err(_) => {
            if native {
                fastfile_jup::map::handover::cancel(path);
            }
            return Err(zone_gap(path, "Modern Warfare III's Oodle could not be loaded".to_owned(), "assets::lane::jup_map::load_world/panic"));
        }
    };
    // (The key tells a conversion for MW3's own renderer, which keeps no
    // pictures, sky or terrain picture, from one for the converted world.)
    let keys = super::jup_cache::keys(path).map(|(m, c)| if native { (format!("{m}-native"), c) } else { (m, c) });
    let stage = progress.begin_scoped(StageId::MapAssets, "modern warfare iii cache", None);
    let started = std::time::Instant::now();
    let cached = keys
        .as_ref()
        .and_then(|(key, _)| super::jup_cache::read(MAP_KIND, key))
        .and_then(|raw| super::jup_cache::decode(&raw));
    // (Collision is gameplay data: read from the map's own files every load
    // unless `BO2MP_JUP_CLIP_CACHE=1` asks for the old cache.)
    let cached_clip = keys
        .as_ref()
        .filter(|_| clip_cache_on())
        .and_then(|(_, key)| super::jup_cache::read(CLIP_KIND, key))
        .and_then(|raw| super::jup_cache::decode_clip(&raw));
    stage.done();
    let (mut conv, packages) = match cached {
        Some(mut conv) => {
            // (No map is read, so none is handed to the native renderer, which reads its own.)
            if native {
                fastfile_jup::map::handover::cancel(path);
            }
            report.push(format!(
                "jup cache: the converted map read in {:.1}s ({} vertices, {} placed models){}",
                started.elapsed().as_secs_f32(),
                conv.t7.vertices.len(),
                conv.t7.static_models.len(),
                if cached_clip.is_some() { ", with its collision" } else { "" }
            ));
            conv.t7.sky_cube = Some(if is_night(&conv.t7.entities) { night_sky(256) } else { default_sky() });
            (conv, None)
        }
        None => {
            let (conv, packages) = match convert(path, &src, oodle.as_ref(), progress, native) {
                Ok(read) => read,
                Err(gap) => {
                    if native {
                        fastfile_jup::map::handover::cancel(path);
                    }
                    return Err(gap);
                }
            };
            if let Some((key, _)) = &keys {
                // (Encoded here, before the pictures change the map;
                // packed and written on a thread of its own.)
                let raw = super::jup_cache::encode(&conv);
                super::jup_cache::write_later(MAP_KIND, key.clone(), move || raw);
            }
            (conv, packages)
        }
    };
    report.append(&mut conv.report);
    // bo2mp: effects the map's entities name take a stand-in (Black Ops III's
    // or Modern Warfare Remastered's) or are logged and skipped.
    super::fx_standin::entity_effects("jup map", &conv.t7.entities);
    report.extend(super::fx_standin::report_lines());
    let stage = progress.begin_scoped(StageId::MapAssets, "modern warfare iii pictures", None);
    let packages = packages.unwrap_or_else(|| {
        let opened = std::time::Instant::now();
        let p = Packages::shared(&src.install);
        report.push(format!("jup packages: {} opened in {:.1}s", p.len(), opened.elapsed().as_secs_f32()));
        p
    });
    // bo2mp: Black Ops III's look (`super::bo3_look`, `BO2MP_BO3_LOOK_JUP=0`
    // off): the vertex colours hold the light baked here (the share of sun
    // and sky reaching each vertex), which the look takes from the sun map
    // and the real-time ambient instead; they go white. A night map's lamps
    // are baked in colour and stay as they are (no look).
    if super::bo3_look::game_on("BO2MP_BO3_LOOK_JUP", true) && conv.t7.baked_ambient.is_some() {
        if is_night(&conv.t7.entities) {
            report.push("jup look: off (a night map: its lamps' pools are baked into the vertex colours)".to_owned());
        } else {
            conv.t7.bo3_look = true;
            for v in &mut conv.t7.vertices {
                v.color = 0xFFFF_FFFF;
            }
        }
    }
    let pictures = Pictures { materials: &conv.materials, images: &conv.images };
    let t7 = &mut conv.t7;
    // (Drawn by `render_jup`: it reads the pictures itself.)
    if !native {
        materials_into(path, &pictures, t7, &packages, oodle.as_ref(), &mut report);
    }
    // The terrain's material (index: after the map's own, see `as_t7`).
    if !native && conv.has_terrain && !conv.materials.is_empty() {
        let mut mat = MapMaterial {
            name: "jup_terrain".to_owned(),
            technique_set: String::new(),
            textures: Vec::new(),
            constants: Default::default(),
            bound: Vec::new(),
        };
        if let Some(vt) = conv.virtual_texture.as_ref()
            && let Some(picture) = super::jup_terrain::picture(vt, &packages, oodle.as_ref(), TERRAIN_LEVEL, &mut report)
        {
            t7.add_image("jup_terrain_colour".to_owned(), picture);
            mat.textures.push((0, "jup_terrain_colour".to_owned()));
        }
        t7.materials.push(mat);
    }
    if !native {
        paint_models(t7, &mut report);
    }
    // The map's own sky, where its pictures are in the install.
    // (Square sky pictures are the native renderer's; this lane reads the two-to-one ones.)
    let wide_skies: Vec<_> = conv.sky_images.iter().filter(|i| u32::from(i.width) == 2 * u32::from(i.height)).cloned().collect();
    if !native && !wide_skies.is_empty() {
        let angles = worldspawn_vec(&t7.entities, "sundirection").unwrap_or(DEFAULT_SUN_ANGLES);
        let (pitch, yaw) = (angles[0].to_radians(), angles[1].to_radians());
        let toward_sun = [pitch.cos() * yaw.cos(), pitch.cos() * yaw.sin(), -pitch.sin()];
        if let Some(cube) = super::jup_sky::sky_cube(&wide_skies, &packages, oodle.as_ref(), toward_sun, 512, &mut report) {
            t7.sky_cube = Some(cube);
        }
    }
    stage.done();
    Ok(Prepared { conv, report, oodle, keys, cached_clip })
}

/// bo2mp: a Modern Warfare III map converted as the map load converts it for
/// the converted world in Black Ops III's look (pictures, stand-ins, sky;
/// no collision, no match): for offline checks (`examples/jup_bo3_shot`).
pub struct JupPreview {
    pub map: T7Map,
    pub report: Vec<String>,
}

/// bo2mp: [`JupPreview`] of the map zone `path` (`<install>/zone/<map>.ff`;
/// on another system than Windows `JUP_OODLE_LIB` names the decoder).
pub fn jup_preview(path: &Path) -> Result<JupPreview, String> {
    let progress = LoadProgress::new(0);
    match prepare(path, &progress, false) {
        Ok(p) => Ok(JupPreview { map: p.conv.t7, report: p.report }),
        Err(gap) => Err(gap.report.join("; ")),
    }
}

/// bo2mp: whether a map's collision is kept in and taken from the cache
/// (`BO2MP_JUP_CLIP_CACHE=1`, the old way). By default (2026-11-06) it is
/// not: collision, like every piece of gameplay data, is read from the
/// game's own files on every load.
fn clip_cache_on() -> bool {
    std::env::var("BO2MP_JUP_CLIP_CACHE").is_ok_and(|v| matches!(v.trim().to_ascii_lowercase().as_str(), "1" | "on" | "true" | "yes"))
}

/// bo2mp: whether MW3's own renderer (`render_jup`) draws MW3 maps:
/// `BO2MP_JUP_NATIVE=1` opts in; by default (2026-10-28) the converted
/// world draws them in Black Ops III's look.
pub(crate) fn jup_native_opted_in() -> bool {
    std::env::var("BO2MP_JUP_NATIVE").is_ok_and(|v| matches!(v.trim().to_ascii_lowercase().as_str(), "1" | "on" | "true" | "yes"))
}

/// Loads a Modern Warfare III map zone into the match's world. A zone that
/// cannot be read becomes a gap with the reason, never a crash.
///
/// The converted map comes from the game's cache when it was converted
/// before from the same files (`super::jup_cache`); else it is read and
/// converted, and written there for the next load. Its collision is read from
/// the zone every load (`BO2MP_JUP_CLIP_CACHE=1`: cached like the map).
pub(crate) fn load_jup_world(
    path: &Path,
    progress: &LoadProgress,
    material_seed: MaterialCatalog,
    native: bool,
) -> LoadedWorld {
    use super::jup_cache::CLIP_KIND;
    let Prepared { conv, mut report, oodle, keys, cached_clip } = match prepare(path, progress, native) {
        Ok(p) => p,
        Err(gap) => return gap,
    };
    // `BO2MP_JUP_CLIP=drawn`: the drawn world's collision, whatever the
    // zone holds or the cache has (and nothing written to the cache).
    let drawn_only = std::env::var("BO2MP_JUP_CLIP").is_ok_and(|v| v.eq_ignore_ascii_case("drawn"));
    let cached_clip = if drawn_only { None } else { cached_clip };
    let clip_was_cached = drawn_only || cached_clip.is_some();
    // The map's own collision (its `col_map`), read from the zone; the
    // drawn world stands in where the zone has none.
    let cached_clip = cached_clip.or_else(|| {
        if drawn_only {
            return None;
        }
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| real_collision(path, oodle.as_ref(), &conv.t7, sea_level_of(&conv.t7.entities), &mut report)))
            .unwrap_or_else(|_| {
                report.push("jup clip: reading the zone's collision failed; collision from the drawn world".to_owned());
                None
            })
    });
    let fog = fog_of(&conv.t7.entities);
    let mut loaded = super::t7_map::build_t7_world(path, &conv.t7, report, progress, material_seed, cached_clip, native);
    // The map's ambience beds (its own sounds, from MW3's packages).
    if let Some(zone) = path.file_stem().and_then(|s| s.to_str()) {
        loaded.facts.script_sound.beds = asset_audio::jup_map_beds(zone);
    }
    if let Some(f) = fog {
        loaded.world.exp_fog = Some(f);
        loaded.report.push(format!(
            "jup fog: start {} half {} opacity {} (BO2MP_JUP_FOG=0 turns it off)",
            f.start_dist, f.halfway_dist, f.max_opacity
        ));
    }
    if !clip_was_cached
        && clip_cache_on()
        && let (Some((_, key)), Some(clip)) = (&keys, loaded.collision.as_ref())
    {
        let clip = clip.clone();
        super::jup_cache::write_later(CLIP_KIND, key.clone(), move || super::jup_cache::encode_clip(&clip));
    }
    diag::info!(
        World,
        "bo2mp jup: {} loaded: {}",
        path.display(),
        loaded
            .report
            .iter()
            .filter(|l| l.starts_with("jup ")
                || l.starts_with("world mesh")
                || l.starts_with("t7 "))
            .cloned()
            .collect::<Vec<_>>()
            .join("; ")
    );
    loaded
}

#[cfg(test)]
mod tests {
    #[test]
    fn night_maps_and_their_lamps() {
        assert!(super::is_night("{\n\"classname\" \"worldspawn\"\n\"sunlight\" \"0\"\n}\n"));
        assert!(!super::is_night("{\n\"classname\" \"worldspawn\"\n\"sunlight\" \"1.2\"\n}\n"));
        assert!(!super::is_night("{\n\"classname\" \"worldspawn\"\n}\n"));
        assert!(super::lamp_kind("lighting_fixtures_security_lamp_withcage_01").is_some_and(|(c, _)| c[0] > c[2]));
        assert!(super::lamp_kind("lighting_fluorescent_tube_64_01_on_white").is_some_and(|(c, _)| c[2] >= c[0]));
        assert!(super::lamp_kind("lighting_fixtures_security_lamp_withcage_01_off_wz").is_none());
        assert!(super::lamp_kind("container_shipping_40ft_red").is_none());
        assert!(super::lamp_kind("tm/accessory_ship_metal_clamp_01").is_none());
        let sky = super::night_sky(16);
        assert_eq!(sky.data.len(), 6 * 16 * 16 * 4);
        // Dark: the zenith face's pixels average well under a quarter.
        let top = &sky.data[4 * 16 * 16 * 4..5 * 16 * 16 * 4];
        let mean = top.iter().step_by(4).map(|&b| f32::from(b)).sum::<f32>() / (16.0 * 16.0);
        assert!(mean < 64.0, "{mean}");
    }

    #[test]
    fn a_lamp_fades_out_at_its_reach() {
        use super::super::jup_light::lamp_falloff;
        assert!(lamp_falloff(0.0, 400.0, 1.0) > 0.99);
        assert!(lamp_falloff(200.0, 400.0, 1.0) > lamp_falloff(300.0, 400.0, 1.0));
        assert_eq!(lamp_falloff(400.0, 400.0, 1.0), 0.0);
        assert!(lamp_falloff(100.0, 400.0, -1.0) > 0.0);
    }
}
