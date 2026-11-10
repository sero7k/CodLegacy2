//! bo2mp: a Modern Warfare III map's own collision (`col_map`, read by
//! `fastfile_jup::map::clip`) as the clip tables the match traces.
//!
//! The zone's convex hulls become brushes and its mesh shapes' triangles
//! the mesh, through the Black Ops III lane's clip builder (brushes under a
//! k-d tree; triangles as its "extra" soup). The drawn world stands in only
//! where the zone has no collision, or (see [`best_collision`]) joins the
//! zone's own where that leaves spawn points without a floor.

use asset_world::ClipCollision;
use asset_world::world_iw7::{IW7_SOLID, SoupTri};
use asset_world::world_t7::{T7Brush, T7ClipInput, T7Cmodel, build_t7_clip_collision};
use fastfile_jup::map::clip::WorldClip;
use fastfile_t7::map::T7Map;

/// Brushes a clip table indexes (leaf lists hold `u16`s).
const MAX_BRUSHES: usize = u16::MAX as usize - 1;

/// The collision of `clip`, moved by `offset` (the zone's shapes to the
/// drawn world), with what was made said in `report`.
///
/// `sea_z` is the sea's height where the map has one (worldspawn's
/// `sealevel`): what lies wholly under it is ground in water, so it carries
/// the `water` surface type (footsteps wade, a landing splashes). `extra`
/// is more solid triangles for the mesh (the drawn world's, where it joins).
pub(super) fn collision_of(
    clip: &WorldClip,
    offset: [f32; 3],
    sea_z: Option<f32>,
    extra: &[SoupTri],
    report: &mut Vec<String>,
) -> ClipCollision {
    // (The surface type is bits 20 and up of the flags; `water` is 20.)
    const WATER: u32 = 20 << 20;
    let under = |top: f32| sea_z.is_some_and(|z| top + offset[2] < z);
    let mut wet = 0usize;
    let moved = |p: [f32; 3]| -> [f32; 3] { std::array::from_fn(|k| p[k] + offset[k]) };
    let mut soup: Vec<SoupTri> = clip
        .tris
        .iter()
        .map(|t| {
            let flags = if under(t.iter().map(|p| p[2]).fold(f32::MIN, f32::max)) {
                wet += 1;
                WATER
            } else {
                0
            };
            SoupTri { p: t.map(moved), surface_flags: flags, contents: IW7_SOLID }
        })
        .collect();
    soup.extend_from_slice(extra);
    let kept = clip.brushes.len().min(MAX_BRUSHES);
    let brushes: Vec<T7Brush> = clip.brushes[..kept]
        .iter()
        .map(|b| {
            // (n . x = d on a face: moving the brush by o moves d by n . o.)
            let planes: Vec<[f32; 4]> = b
                .planes
                .iter()
                .map(|p| [p[0], p[1], p[2], p[3] + p[0] * offset[0] + p[1] * offset[1] + p[2] * offset[2]])
                .collect();
            // (Only the top face is stood on, but a wet hull is wet all round.)
            let flags = if under(b.maxs[2]) {
                wet += 1;
                WATER
            } else {
                0
            };
            T7Brush { contents: IW7_SOLID, plane_surface_flags: vec![flags; planes.len()], planes }
        })
        .collect();
    // (Hulls past what a table indexes would be hollow in a soup: left
    // out and said.)
    let left_out = clip.brushes.len() - kept;
    let input = T7ClipInput {
        materials: vec![("jup_solid".to_owned(), 0, IW7_SOLID)],
        leaf_brushes: vec![(0..kept as u16).collect()],
        cmodels: vec![T7Cmodel { mins: moved(clip.mins), maxs: moved(clip.maxs), radius: 0.0, brushes: Vec::new() }],
        brushes,
        ..T7ClipInput::default()
    };
    let (collision, stats) = build_t7_clip_collision(&input, &soup);
    soup.clear();
    report.push(format!(
        "jup clip: {} brushes, {} mesh triangles ({} of them the drawn world's) from {} mesh and {} compound shapes ({} hulls unusable, {left_out} past the table); {} leaves; {wet} under the sea{}",
        stats.brushes,
        stats.extra_tris,
        extra.len(),
        clip.shapes.0,
        clip.shapes.1,
        clip.bad_hulls,
        stats.leaves,
        sea_z.map_or(String::new(), |z| format!(" at {z:.0}"))
    ));
    collision
}

/// The drawn world's collision triangles (the surfaces the converter left
/// collidable), as solid soup.
pub(super) fn drawn_soup(drawn: &T7Map) -> Vec<SoupTri> {
    let mut out = Vec::new();
    for (i, s) in drawn.surfaces.iter().enumerate() {
        if drawn.no_collision.get(i).copied().unwrap_or(false) {
            continue;
        }
        for t in 0..s.tri_count as usize {
            let p = surface_tri(drawn, s, t);
            out.push(SoupTri { p, surface_flags: 0, contents: IW7_SOLID });
        }
    }
    out
}

/// Triangle `t` of a drawn surface.
fn surface_tri(drawn: &T7Map, s: &fastfile_t7::map::WorldSurface, t: usize) -> [[f32; 3]; 3] {
    std::array::from_fn(|k| {
        let ix = drawn.indices.get(s.base_index as usize + 3 * t + k).copied().unwrap_or(0);
        drawn.vertices.get((s.first_vertex + u32::from(ix)) as usize).map_or([0.0; 3], |v| v.xyz)
    })
}

/// The collision for a map: the zone's own shapes (`clip`), and the drawn
/// world's triangles added where that leaves spawn points without a floor.
///
/// Each is counted by dropping the map's spawn points onto it
/// ([`asset_world::world_iw3::spawn_ground_census`]). Where a tenth of the
/// spawns or more have no floor in the zone's shapes (fall further than they
/// may, or nothing below), the drawn world's collidable surfaces join the
/// mesh, and stay if that gives more spawns ground. The counts are said in
/// `report` (`BO2MP_JUP_CLIP=drawn` uses the drawn world alone).
pub(super) fn best_collision(clip: &WorldClip, drawn: &T7Map, sea_z: Option<f32>, report: &mut Vec<String>) -> ClipCollision {
    use asset_world::world_iw3::spawn_ground_census;
    let spawns: Vec<[f32; 3]> = super::t7_map::spawn_origins(&drawn.entities);
    let mut collision = collision_of(clip, [0.0; 3], sea_z, &[], report);
    let mut got = spawn_ground_census(&collision, &spawns);
    report.push(format!("jup clip: spawns on the zone's shapes: {got}"));
    // Spawns without floor (a tenth of them or more): the drawn world's
    // triangles join the mesh, where that gives more of them ground.
    let lacking = got.dropped_far + got.no_floor;
    if got.spawns > 0 && lacking * 10 >= got.spawns {
        let soup = drawn_soup(drawn);
        let merged = collision_of(clip, [0.0; 3], sea_z, &soup, &mut Vec::new());
        let c = spawn_ground_census(&merged, &spawns);
        report.push(format!(
            "jup clip: {lacking} of {} spawns have no floor of their own in the zone's collision; with the drawn world's {} triangles joined: {c}",
            got.spawns,
            soup.len()
        ));
        if c.on_ground > got.on_ground {
            report.push("jup clip: the drawn world's collision joined the zone's".to_owned());
            collision = merged;
            got = c;
        } else {
            report.push("jup clip: the zone's collision kept alone (the drawn world gave no more ground)".to_owned());
        }
    }
    report.push(format!("jup clip: final spawn census: {got}"));
    collision
}
