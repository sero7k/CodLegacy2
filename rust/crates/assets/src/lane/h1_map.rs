//! bo2mp: Call of Duty: Modern Warfare Remastered (H1) multiplayer maps:
//! its own and Horizon MW's usermaps (MW2's, MW3's and others' maps rebuilt
//! for it with the zone tools).
//!
//! A zone of H1's version (0x42) is walked by `fastfile_h1`'s exact walk
//! and read into plain data by `fastfile_h1::map`; this turns it into the
//! match's world as the Ghosts lane does (`super::iw6_map`; H1's world is
//! Ghosts' with the differences `docs/H1.md` lists): the world goes to the
//! IW3 world builder, the collision to the IW3 clip builder (brushes,
//! terrain triangles under the AABB forest, brush models) with the placed
//! models kept for the model trace (bullets and sight only, as the game
//! does: `docs/H1_NATIVE.md` *Collision*), the lightmaps converted to
//! CoD4's form (`fastfile_h1::map::iw3_lightmap`), the entities named (H1's
//! keys already are) and its trigger models kept for the scripts.
//! Materials and placed models go through the H1 builder (`super::h1`):
//! pictures from the zone, its own `<map>.pak`, the game's
//! `imagefile<N>.pak`s, and what the map only names from `common_mp`.
//! Black Ops II's rules then add everything else (`attach_t6_rules`).
//!
//! No ZoneGame of its own: the map walk sends a zone of H1's version here
//! (`asset_transport::legacy_game_for_path` says `h1`).

use std::collections::HashMap;
use std::path::Path;

use asset_core::AssetNamespace;
use asset_material::MaterialCatalog;
use asset_transport::progress::{LoadProgress, StageId};
use asset_world::WorldDrawPolicy;
use fastfile_h1::map::IwMap;
use fastfile_h1::walk::Capture;

use super::LoadedWorld;
use super::h1::{ColourMap, H1Builder};
use super::iw6_map::{ghosts_fog, iw3_colour, iw3_unit_vec, place_point, rotate, spawns_of};
use crate::lane_capability::PreparedCapability;
use crate::session_load::PreparedWorld;

/// The largest level kept of a world surface's pictures and of a placed
/// model's (`BO2MP_H1_MAP_TEXTURE=<size>` changes both).
const WORLD_TEXTURE_MAX: u32 = 512;

fn texture_max() -> u32 {
    std::env::var("BO2MP_H1_MAP_TEXTURE")
        .ok()
        .and_then(|v| v.trim().parse().ok())
        .filter(|&v: &u32| (64..=4096).contains(&v))
        .unwrap_or(WORLD_TEXTURE_MAX)
}

/// Whether Modern Warfare Remastered's maps are drawn with Black Ops III's
/// look (`super::bo3_look`, as Nuk3town): its baked light (the lightmaps,
/// linear, without the sun) as the ambient, the sun over a sun map baked at
/// load, GGX highlights, the probe's reflections and the filmic tone curve.
/// `BO2MP_BO3_LOOK_H1=0` (or the older `BO2MP_H1_LOOK=0`): CoD4's lightmap
/// draw, as before; `BO2MP_BO3_LOOK_H1`, where set, decides.
pub(super) fn look_on() -> bool {
    super::bo3_look::game_on("BO2MP_BO3_LOOK_H1", std::env::var("BO2MP_H1_LOOK").map_or(true, |v| v.trim() != "0"))
}

fn env_f(name: &str, default: f32) -> f32 {
    codlegacy_env::var(name)
        .ok()
        .and_then(|v| v.trim().parse().ok())
        .filter(|v: &f32| v.is_finite() && *v >= 0.0)
        .unwrap_or(default)
}

/// The look's light levels (linear, into the tone curve at exposure 1): the
/// mean baked light of the shaded texels (`BO2MP_H1_SHADE`), and the sun's
/// over its colour (`BO2MP_H1_SUN`); the tone curve's exposure
/// (`BO2MP_H1_EXPOSURE`) and the placed models' light over their ground
/// light (`BO2MP_H1_MODEL`). Chosen against pictures of the game: a shaded
/// wall a third of the way up the tone curve, a sunlit floor three
/// quarters.
const LOOK_SHADE: f32 = 0.4;
const LOOK_SUN: f32 = 2.6;
const LOOK_EXPOSURE: f32 = 1.0;
const LOOK_MODEL: f32 = 1.0;

/// A baked light whose sunlit texels are this much brighter than its shaded
/// ones holds the sun's own light: the look then draws no sun of its own.
const SUN_IN_BAKE: f32 = 2.5;

/// A brush model an entity places: its surfaces, origin and axes.
type PlacedBrush = (std::ops::Range<usize>, [f32; 3], [[f32; 3]; 3]);

/// The panic's message.
fn panic_text(panic: &(dyn std::any::Any + Send)) -> String {
    panic
        .downcast_ref::<&str>()
        .map(|s| (*s).to_owned())
        .or_else(|| panic.downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "unknown error".to_owned())
}

/// Loads a Modern Warfare Remastered map zone into the match's world. A
/// zone that cannot be read becomes a gap with the reason, never a crash.
pub(crate) fn load_h1_world(
    path: &Path,
    progress: &LoadProgress,
    material_seed: MaterialCatalog,
) -> LoadedWorld {
    load_h1_world_audited(path, progress, material_seed, &mut HashMap::new(), false)
}

/// `load_h1_world` for a match: the old converted world (its meshes,
/// materials, pictures and look) is not built, MWR's own renderer
/// (`render_h1`) draws the map; collision, spawns, entities and scripts
/// are read as before.
pub(crate) fn load_h1_world_native(
    path: &Path,
    progress: &LoadProgress,
    material_seed: MaterialCatalog,
) -> LoadedWorld {
    load_h1_world_audited(path, progress, material_seed, &mut HashMap::new(), true)
}

/// `load_h1_world`, also giving per catalog material whether its colour
/// map is drawn (`H1Builder::colour`).
fn load_h1_world_audited(
    path: &Path,
    progress: &LoadProgress,
    material_seed: MaterialCatalog,
    colour: &mut HashMap<usize, ColourMap>,
    native: bool,
) -> LoadedWorld {
    let mut report = vec![format!(
        "game: H1 / Modern Warfare Remastered ({})",
        path.display()
    )];
    let stage = progress.begin_scoped(StageId::MapAssets, "mwr zone", None);
    let started = std::time::Instant::now();
    let read = std::panic::catch_unwind(|| -> Result<_, String> {
        let zone = fastfile_h1::Zone::open(path).map_err(|e| e.to_string())?;
        let (cap, stop) = fastfile_h1::walk::walk_zone(&zone);
        Ok((zone, cap, stop))
    });
    let (zone, cap, stop) = match read {
        Ok(Ok(z)) => {
            stage.done();
            z
        }
        Ok(Err(error)) => {
            stage.fail();
            diag::warn!(World, "bo2mp h1: {}: {error}", path.display());
            return LoadedWorld::with_gap(
                WorldDrawPolicy::t5(),
                PreparedCapability::PreparedWorld,
                format!("Modern Warfare Remastered zone: {error}"),
                Some("assets::lane::h1_map::load_world/zone"),
            );
        }
        Err(panic) => {
            stage.fail();
            let what = panic_text(panic.as_ref());
            diag::warn!(
                World,
                "bo2mp h1: {}: reader panicked: {what}",
                path.display()
            );
            return LoadedWorld::with_gap(
                WorldDrawPolicy::t5(),
                PreparedCapability::PreparedWorld,
                format!("Modern Warfare Remastered zone could not be read: {what}"),
                Some("assets::lane::h1_map::load_world/panic"),
            );
        }
    };
    report.push(format!(
        "h1 zone: walked {} of {} assets [{:.1}s]{}",
        cap.walked,
        zone.assets.assets.len(),
        started.elapsed().as_secs_f32(),
        stop.as_ref()
            .map(|s| format!(
                ", stopped at {} ({:?}): {}",
                s.asset_index, s.asset_type, s.error
            ))
            .unwrap_or_else(String::new)
    ));
    let map = fastfile_h1::map::read_map(&cap, &zone.image, &zone.assets.script_strings);
    report.extend(map.notes.iter().cloned());
    let built = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        build_h1_world(
            path,
            &zone,
            &cap,
            &map,
            report,
            progress,
            material_seed,
            colour,
            native,
        )
    }));
    let loaded = match built {
        Ok(loaded) => loaded,
        Err(panic) => {
            let what = panic_text(panic.as_ref());
            diag::warn!(
                World,
                "bo2mp h1: {}: building the world panicked: {what}",
                path.display()
            );
            return LoadedWorld::with_gap(
                WorldDrawPolicy::t5(),
                PreparedCapability::PreparedWorld,
                format!("Modern Warfare Remastered world could not be built: {what}"),
                Some("assets::lane::h1_map::load_world/build_panic"),
            );
        }
    };
    diag::info!(
        World,
        "bo2mp h1: {} loaded: {}",
        path.display(),
        loaded
            .report
            .iter()
            .filter(|l| l.starts_with("h1 ") || l.starts_with("world mesh"))
            .cloned()
            .collect::<Vec<_>>()
            .join("; ")
    );
    loaded
}

/// bo2mp: Modern Warfare Remastered's breakable glass is not ported yet (its
/// zone's glass data is read and dropped), so a glass brush or model is a
/// wall nobody can break. For now the player walks through it: a piece
/// with the glass bit keeps only that bit (shots still hit it, nothing
/// blocks the player). `BO2MP_H1_GLASS_SOLID=1` keeps the glass solid.
pub(super) fn walk_through_glass(contents: u32) -> u32 {
    const GLASS: u32 = 0x10;
    if contents & GLASS != 0 && !std::env::var("BO2MP_H1_GLASS_SOLID").is_ok_and(|v| v.trim() == "1") {
        GLASS
    } else {
        contents
    }
}

/// H1's collision as CoD4's `clipMap_t` (what the IW3 clip builder reads).
fn iw3_clip(clip: &fastfile_h1::map::Clip) -> iw3::zone::ClipMap {
    iw3::zone::ClipMap {
        name: clip.name.clone(),
        planes: clip
            .planes
            .iter()
            .map(|p| iw3::zone::Plane {
                normal: p.normal,
                dist: p.dist,
            })
            .collect(),
        static_models: Vec::new(),
        materials: clip
            .materials
            .iter()
            .map(
                |(name, surface_flags, content_flags)| iw3::zone::ClipMaterial {
                    name: name.clone(),
                    surface_flags: *surface_flags,
                    content_flags: *content_flags,
                },
            )
            .collect(),
        brushes: clip
            .brushes
            .iter()
            .map(|b| iw3::zone::Brush {
                mins: b.mins,
                maxs: b.maxs,
                contents: walk_through_glass(b.contents as u32) as i32,
                side_planes: b.sides.iter().map(|s| s.0).collect(),
                side_materials: b.sides.iter().map(|s| u32::from(s.1)).collect(),
                axial_materials: b.axial_materials,
            })
            .collect(),
        verts: clip.verts.clone(),
        tri_indices: clip.tri_indices.clone(),
        partitions: clip
            .partitions
            .iter()
            .map(|&(tri_count, first_tri, _)| iw3::zone::CollisionPartition {
                tri_count,
                first_tri,
            })
            .collect(),
        aabb_trees: clip
            .aabb_trees
            .iter()
            .map(|t| iw3::zone::CollisionAabbTree {
                origin: t.origin,
                half_size: t.half_size,
                material_index: t.material_index,
                child_count: t.child_count,
                index: t.index,
            })
            .collect(),
        cmodels: clip
            .cmodels
            .iter()
            .map(|m| iw3::zone::CModel {
                mins: m.mins,
                maxs: m.maxs,
                radius: m.radius,
                leaf_brush_node: m.leaf_brush_node,
                first_coll_aabb: m.first_coll_aabb,
                coll_aabb_count: m.coll_aabb_count,
            })
            .collect(),
        map_ents: None,
        dyn_ent_counts: [0, 0],
        dyn_ents: Vec::new(),
        leaf_brush_nodes: clip
            .leaf_brush_nodes
            .iter()
            .map(|n| iw3::zone::LeafBrushNode {
                leaf_brush_count: n.leaf_brush_count,
                brushes: n.brushes.clone(),
                child_offsets: n.child_offsets,
            })
            .collect(),
    }
}

/// The inverse of a 3x3 matrix (`None` if singular).
pub(super) fn invert3(m: [[f32; 3]; 3]) -> Option<[[f32; 3]; 3]> {
    let c =
        |r0: usize, c0: usize, r1: usize, c1: usize| m[r0][c0] * m[r1][c1] - m[r0][c1] * m[r1][c0];
    let det = m[0][0] * c(1, 1, 2, 2) - m[0][1] * c(1, 0, 2, 2) + m[0][2] * c(1, 0, 2, 1);
    if det.abs() <= 1e-12 || !det.is_finite() {
        return None;
    }
    let inv = 1.0 / det;
    Some([
        [
            c(1, 1, 2, 2) * inv,
            -c(0, 1, 2, 2) * inv,
            c(0, 1, 1, 2) * inv,
        ],
        [
            -c(1, 0, 2, 2) * inv,
            c(0, 0, 2, 2) * inv,
            -c(0, 0, 1, 2) * inv,
        ],
        [
            c(1, 0, 2, 1) * inv,
            -c(0, 0, 2, 1) * inv,
            c(0, 0, 1, 1) * inv,
        ],
    ])
}

/// A triangle as the model trace takes it (`XModelCollTri`'s form): its
/// plane, front the side the game's test hits from (the normal is
/// `(c - a) x (b - a)`: clockwise seen from the front), and the two
/// barycentric rows (`s = svec.p - svec.w` along `b - a`, `t` along
/// `c - a`). `None` when degenerate.
pub(super) fn coll_tri(a: [f32; 3], b: [f32; 3], c: [f32; 3]) -> Option<clipmap_iw4::XModelCollTri> {
    let sub = |p: [f32; 3], q: [f32; 3]| [p[0] - q[0], p[1] - q[1], p[2] - q[2]];
    let cross = |u: [f32; 3], v: [f32; 3]| {
        [
            u[1] * v[2] - u[2] * v[1],
            u[2] * v[0] - u[0] * v[2],
            u[0] * v[1] - u[1] * v[0],
        ]
    };
    let dot = |u: [f32; 3], v: [f32; 3]| u[0] * v[0] + u[1] * v[1] + u[2] * v[2];
    let (e1, e2) = (sub(b, a), sub(c, a));
    let n = cross(e2, e1);
    let len = dot(n, n).sqrt();
    if !(len > 1e-6) || !len.is_finite() {
        return None;
    }
    let n = n.map(|v| v / len);
    let row = |dual: [f32; 3], edge: [f32; 3]| -> Option<[f32; 4]> {
        let d = dot(dual, edge);
        if d.abs() <= 1e-12 {
            return None;
        }
        let v = dual.map(|x| x / d);
        Some([v[0], v[1], v[2], dot(v, a)])
    };
    Some(clipmap_iw4::XModelCollTri {
        plane: [n[0], n[1], n[2], dot(n, a)],
        svec: row(cross(e2, n), e1)?,
        tvec: row(cross(n, e1), e2)?,
    })
}

/// A model's collision for the trace (`clipmap_iw4::XModelColl`), as the
/// game reads it: every rigid vertex list of every surface of the
/// collision LOD, in order, paired with the model's next
/// `XModelCollSurf_s` (bounds, bone, contents, surface flags); its
/// triangles (the list's run of `triIndices2`, which index the surface's
/// vertices). `None` when the model has no collision LOD. Also: lists
/// without a collision surface (left out, as the game does).
fn xmodel_coll(
    cap: &Capture,
    image: &[u8],
    model: &fastfile_h1::walk::XModelRec,
) -> Option<(clipmap_iw4::XModelColl, usize)> {
    if model.coll_lod < 0 {
        return None;
    }
    let surfs = model
        .lods
        .get(model.coll_lod as usize)
        .and_then(|l| l.surfs)
        .and_then(|s| cap.model_surfs.get(s.index as usize))?;
    let mut out = clipmap_iw4::XModelColl {
        trace_offset: H1_MODEL_TRACE_OFFSET,
        coll_lod: i16::from(model.coll_lod),
        contents: walk_through_glass(model.contents as u32),
        surfs: Default::default(),
    };
    let mut surfs_out: Vec<clipmap_iw4::XModelCollSurf> = Vec::new();
    let mut unpaired = 0usize;
    for s in &surfs.surfs {
        let verts = s.verts.map_or(&[][..], |sp| sp.get(image));
        let tris = s.tris2.or(s.tris).map_or(&[][..], |sp| sp.get(image));
        let pos = |i: u16| -> Option<[f32; 3]> {
            let at = 32 * usize::from(i);
            let v = verts.get(at..at + 12)?;
            Some([0, 4, 8].map(|o| f32::from_le_bytes([v[o], v[o + 1], v[o + 2], v[o + 3]])))
        };
        for &(_, _, tri_offset, tri_count) in &s.rigid {
            let Some(cs) = model.coll_surfs.get(surfs_out.len()) else {
                unpaired += 1;
                continue;
            };
            let first = usize::from(tri_offset);
            let coll = (first..first + usize::from(tri_count))
                .filter_map(|t| {
                    let at = 6 * t;
                    let ix = tris.get(at..at + 6)?;
                    let ix = [0, 2, 4].map(|o| u16::from_le_bytes([ix[o], ix[o + 1]]));
                    coll_tri(pos(ix[0])?, pos(ix[1])?, pos(ix[2])?)
                })
                .collect();
            surfs_out.push(clipmap_iw4::XModelCollSurf {
                tris: coll,
                midpoint: cs.mid,
                half_size: cs.half,
                bone_idx: cs.bone,
                contents: walk_through_glass(cs.contents),
                surf_flags: cs.surf_flags,
            });
        }
    }
    out.surfs = surfs_out.into();
    Some((out, unpaired))
}

/// The model trace's back-off of a triangle hit: none (the game takes the
/// plane crossing exactly; IW4's is 0.125).
const H1_MODEL_TRACE_OFFSET: f32 = 0.0;

/// `BO2MP_H1_COLLISION=soup` brings back the collision before 30 Oct
/// (placed models' triangles in the world mesh, met by the player too);
/// anything else, or unset, is the game's (placed models for line traces
/// only).
fn soup_collision() -> bool {
    std::env::var("BO2MP_H1_COLLISION").is_ok_and(|v| v.eq_ignore_ascii_case("soup"))
}

/// What the clip map's placed models give the collision.
#[derive(Default)]
struct ModelCollision {
    models: Vec<asset_world::ClipPlacedStaticModel>,
    /// Placements whose model has no collision LOD (`collLod` < 0).
    no_lod: usize,
    /// Placements without their model, or over the alternate level.
    skipped: usize,
    /// Placements whose layer byte is not 0 (kept; see `ClipStaticModel`).
    layered: usize,
    /// Triangles over all placements.
    tris: usize,
    /// Rigid vertex lists with no collision surface of their own.
    unpaired: usize,
}

/// The clip map's placed models (`cStaticModel_s`) for the model trace,
/// the game's way (read from the H1 executable, `docs/H1_NATIVE.md`
/// *Collision*): a placed model is met only by line traces (bullets,
/// sight, `sim::bullet_collision`); the player's box (a capsule) goes
/// through brushes, terrain and brush models only, so nothing is added to
/// the mesh. Each keeps its placement (origin, inverse scaled axes, the
/// clip map's bounds) and its model's collision LOD (`xmodel_coll`).
fn model_collision(
    cap: &Capture,
    image: &[u8],
    clip: &fastfile_h1::map::Clip,
    level_cut: Option<f32>,
) -> ModelCollision {
    let mut out = ModelCollision::default();
    let mut cache: HashMap<u32, Option<(clipmap_iw4::XModelColl, usize)>> = HashMap::new();
    for (index, sm) in clip.static_models.iter().enumerate() {
        let Some(id) = sm.model else {
            out.skipped += 1;
            continue;
        };
        let Some(model) = cap.xmodels.get(id.index as usize) else {
            out.skipped += 1;
            continue;
        };
        if level_cut.is_some_and(|c| sm.origin[2] > c) {
            out.skipped += 1;
            continue;
        }
        let Some((coll, unpaired)) = cache
            .entry(id.index)
            .or_insert_with(|| xmodel_coll(cap, image, model))
            .clone()
        else {
            out.no_lod += 1;
            continue;
        };
        if sm.layer != 0 {
            out.layered += 1;
        }
        out.unpaired += unpaired;
        out.tris += coll.surfs.iter().map(|s| s.tris.len()).sum::<usize>();
        out.models.push(asset_world::ClipPlacedStaticModel {
            index: index as u32,
            name: model.name.clone(),
            model: clipmap_iw4::ClipStaticModel {
                origin: sm.origin,
                inv_scaled_axis: sm.inv_scaled_axis,
                bounds_mid: std::array::from_fn(|k| (sm.mins[k] + sm.maxs[k]) * 0.5),
                bounds_half: std::array::from_fn(|k| ((sm.maxs[k] - sm.mins[k]) * 0.5).max(0.0)),
                coll,
            },
        });
    }
    out
}

/// The collision before 30 Oct (`BO2MP_H1_COLLISION=soup`, for
/// comparison): each placed model's collision LOD as triangles in the world
/// mesh, met by the player's box too (not what the game does).
mod soup {
    use super::{Capture, HashMap, invert3};

    /// A triangle's corners.
    type Tri = [[f32; 3]; 3];

    /// A placed model smaller than this (radius times scale) adds no collision
    /// (cans, bottles, debris: the player steps over them in H1 too).
    const COLLIDE_MIN_RADIUS: f32 = 12.0;

    /// At most this many triangles of placed models' collision are added; past
    /// it, the coarsest LOD of each model is used instead of its collision LOD.
    const MODEL_COLLISION_BUDGET: usize = 1_500_000;

    /// A model's triangles (model space) at one of its LODs, from the zone.
    pub(super) fn lod_triangles(
        cap: &Capture,
        image: &[u8],
        model: &fastfile_h1::walk::XModelRec,
        lod: usize,
    ) -> Vec<[[f32; 3]; 3]> {
        let Some(surfs) = model
            .lods
            .get(lod)
            .and_then(|l| l.surfs)
            .and_then(|s| cap.model_surfs.get(s.index as usize))
        else {
            return Vec::new();
        };
        let mut out = Vec::new();
        for s in &surfs.surfs {
            let verts = s.verts.map_or(&[][..], |sp| sp.get(image));
            let tris = s.tris.map_or(&[][..], |sp| sp.get(image));
            let pos = |i: u16| -> Option<[f32; 3]> {
                let at = 32 * usize::from(i);
                let v = verts.get(at..at + 12)?;
                Some([0, 4, 8].map(|o| f32::from_le_bytes([v[o], v[o + 1], v[o + 2], v[o + 3]])))
            };
            for t in tris.chunks_exact(6) {
                let ix = [0, 2, 4].map(|o| u16::from_le_bytes([t[o], t[o + 1]]));
                if let (Some(a), Some(b), Some(c)) = (pos(ix[0]), pos(ix[1]), pos(ix[2])) {
                    out.push([a, b, c]);
                }
            }
        }
        out
    }

    /// The LOD a model collides with: its collision LOD (`collLod`), or with
    /// `coarse` its last LOD with surfaces. `None` when it collides with none.
    fn collision_lod(model: &fastfile_h1::walk::XModelRec, coarse: bool) -> Option<usize> {
        if model.coll_lod < 0 {
            return None;
        }
        let with_surfs = |l: usize| {
            model
                .lods
                .get(l)
                .is_some_and(|l| l.surfs.is_some() && l.numsurfs > 0)
        };
        if coarse {
            return (0..usize::from(model.num_lods).max(1))
                .rev()
                .find(|&l| with_surfs(l));
        }
        let l = usize::from(model.coll_lod as u8);
        with_surfs(l)
            .then_some(l)
            .or_else(|| (0..=l).rev().find(|&l| with_surfs(l)))
    }

    /// What placed models add to the collision.
    #[derive(Default)]
    pub(super) struct ModelCollision {
        pub(super) tris: Vec<asset_world::world_iw7::SoupTri>,
        pub(super) models: usize,
        pub(super) small: usize,
        pub(super) no_lod: usize,
        pub(super) coarse: bool,
    }

    /// The clip map's placed models (`cStaticModel_s`: the ones that collide)
    /// as world-space triangles of their collision LOD, with the model's own
    /// contents (solid, foliage, glass). Models under `COLLIDE_MIN_RADIUS` are
    /// left out.
    pub(super) fn model_collision(
        cap: &Capture,
        image: &[u8],
        clip: &fastfile_h1::map::Clip,
        level_cut: Option<f32>,
    ) -> ModelCollision {
        let gather = |coarse: bool| -> ModelCollision {
            let mut out = ModelCollision {
                coarse,
                ..Default::default()
            };
            let mut cache: HashMap<(u32, usize), std::rc::Rc<Vec<Tri>>> = HashMap::new();
            for sm in &clip.static_models {
                let Some(id) = sm.model else { continue };
                let Some(model) = cap.xmodels.get(id.index as usize) else {
                    continue;
                };
                if level_cut.is_some_and(|c| sm.origin[2] > c) {
                    continue;
                }
                let Some(m) = invert3(sm.inv_scaled_axis) else {
                    continue;
                };
                let scale = (0..3)
                    .map(|k| m[k].iter().map(|v| v * v).sum::<f32>().sqrt())
                    .fold(0.0f32, f32::max);
                if model.radius * scale < COLLIDE_MIN_RADIUS {
                    out.small += 1;
                    continue;
                }
                let Some(lod) = collision_lod(model, coarse) else {
                    out.no_lod += 1;
                    continue;
                };
                let tris = cache
                    .entry((id.index, lod))
                    .or_insert_with(|| std::rc::Rc::new(lod_triangles(cap, image, model, lod)))
                    .clone();
                if tris.is_empty() {
                    out.no_lod += 1;
                    continue;
                }
                out.models += 1;
                let contents = if model.contents == 0 {
                    asset_world::world_iw7::IW7_SOLID
                } else {
                    super::walk_through_glass(model.contents as u32)
                };
                let place = |p: [f32; 3]| -> [f32; 3] {
                    std::array::from_fn(|k| {
                        sm.origin[k] + p[0] * m[0][k] + p[1] * m[1][k] + p[2] * m[2][k]
                    })
                };
                out.tris
                    .extend(tris.iter().map(|t| asset_world::world_iw7::SoupTri {
                        p: t.map(place),
                        surface_flags: 0,
                        contents,
                    }));
            }
            out
        };
        let fine = gather(false);
        if fine.tris.len() <= MODEL_COLLISION_BUDGET {
            return fine;
        }
        gather(true)
    }

    /// Leaves out the placed models' triangles that reach into a spawn point's
    /// player box (lifted off its floor): H1's own placed-model collision is
    /// only traced by bullets there, so a prop's LOD laid over a spawn would
    /// start the player stuck. How many went.
    pub(super) fn clear_spawn_boxes(
        tris: &mut Vec<asset_world::world_iw7::SoupTri>,
        spawns: &[fastfile_h1::map::SpawnRecord],
    ) -> usize {
        const HALF: [f32; 3] = [16.0, 16.0, 36.0];
        const CELL: f32 = 256.0;
        let cell = |x: f32, y: f32| ((x / CELL).floor() as i32, (y / CELL).floor() as i32);
        let mut cells: HashMap<(i32, i32), Vec<[f32; 3]>> = HashMap::new();
        for s in spawns {
            let mid = [s.origin[0], s.origin[1], s.origin[2] + 1.0 + HALF[2]];
            cells.entry(cell(mid[0], mid[1])).or_default().push(mid);
        }
        let before = tris.len();
        tris.retain(|t| {
            let lo = [0, 1].map(|k| t.p.iter().map(|p| p[k]).fold(f32::MAX, f32::min) - HALF[k]);
            let hi = [0, 1].map(|k| t.p.iter().map(|p| p[k]).fold(f32::MIN, f32::max) + HALF[k]);
            let (c0, c1) = (cell(lo[0], lo[1]), cell(hi[0], hi[1]));
            let hit = if (c1.0 - c0.0 + 1) * (c1.1 - c0.1 + 1) > 64 {
                cells
                    .values()
                    .flatten()
                    .any(|&mid| super::super::tri_overlaps_box(&t.p, mid, HALF))
            } else {
                (c0.0..=c1.0).any(|cx| {
                    (c0.1..=c1.1).any(|cy| {
                        cells.get(&(cx, cy)).is_some_and(|l| {
                            l.iter()
                                .any(|&mid| super::super::tri_overlaps_box(&t.p, mid, HALF))
                        })
                    })
                })
            };
            !hit
        });
        before - tris.len()
    }

    /// Adds `extra` triangles to a built collision's mesh: partitions of their
    /// own (each with its vertices in a segment of its own) under one tree,
    /// whose root joins the mesh's roots. The triangles added, or why none
    /// were.
    pub(super) fn append_soup(
        clip: &mut asset_world::ClipCollision,
        extra: &[asset_world::world_iw7::SoupTri],
    ) -> Result<usize, String> {
        const PART_TRIS: usize = 32;
        const FANOUT: usize = 8;
        let keep: Vec<&asset_world::world_iw7::SoupTri> = extra
            .iter()
            .filter(|t| {
                let [a, b, c] = t.p;
                let e1 = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
                let e2 = [c[0] - a[0], c[1] - a[1], c[2] - a[2]];
                let n = [
                    e1[1] * e2[2] - e1[2] * e2[1],
                    e1[2] * e2[0] - e1[0] * e2[2],
                    e1[0] * e2[1] - e1[1] * e2[0],
                ];
                n[0] * n[0] + n[1] * n[1] + n[2] * n[2] > 1e-6
                    && t.p.iter().flatten().all(|v| v.is_finite())
            })
            .collect();
        if keep.is_empty() {
            return Ok(0);
        }
        // Neighbours together: a Morton order of the triangles' middles.
        let mid = |t: &asset_world::world_iw7::SoupTri| -> [f32; 3] {
            std::array::from_fn(|k| (t.p[0][k] + t.p[1][k] + t.p[2][k]) / 3.0)
        };
        let (mut lo, mut hi) = ([f32::MAX; 3], [f32::MIN; 3]);
        for t in &keep {
            let m = mid(t);
            for k in 0..3 {
                lo[k] = lo[k].min(m[k]);
                hi[k] = hi[k].max(m[k]);
            }
        }
        let spread = |mut v: u64| {
            v &= 0x1f_ffff;
            v = (v | (v << 32)) & 0x1f_0000_0000_ffff;
            v = (v | (v << 16)) & 0x1f_0000_ff00_00ff;
            v = (v | (v << 8)) & 0x100f_00f0_0f00_f00f;
            v = (v | (v << 4)) & 0x10c3_0c30_c30c_30c3;
            (v | (v << 2)) & 0x1249_2492_4924_9249
        };
        let key = |t: &asset_world::world_iw7::SoupTri| {
            let m = mid(t);
            let q = |k: usize| (((m[k] - lo[k]) / (hi[k] - lo[k]).max(1.0)) * 2_097_151.0) as u64;
            spread(q(0)) | (spread(q(1)) << 1) | (spread(q(2)) << 2)
        };
        let mut order: Vec<usize> = (0..keep.len()).collect();
        order.sort_by_cached_key(|&i| key(keep[i]));
        let mesh = std::sync::Arc::make_mut(&mut clip.mesh);
        let parts = keep.len().div_ceil(PART_TRIS);
        // The new nodes: leaves, then each level above, laid out root first.
        let levels = {
            let mut n = parts;
            let mut l = 1;
            while n > 1 {
                n = n.div_ceil(FANOUT);
                l += 1;
            }
            l
        };
        let mut counts = vec![parts];
        while counts.last().copied().unwrap_or(0) > 1 {
            let up = counts.last().copied().unwrap_or(0).div_ceil(FANOUT);
            counts.push(up);
        }
        debug_assert_eq!(counts.len(), levels);
        let total: usize = counts.iter().sum();
        let base = mesh.aabb_trees.len();
        if base + total >= i32::MAX as usize {
            return Err(format!("{base} tree nodes already"));
        }
        // Offsets of each level (root level first in memory: the root is
        // named by u16).
        let mut offsets = vec![0usize; counts.len()];
        let mut at = base;
        for d in (0..counts.len()).rev() {
            offsets[d] = at;
            at += counts[d];
        }
        if offsets[counts.len() - 1] > usize::from(u16::MAX) {
            return Err("the tree's root would not be indexable".to_owned());
        }
        let seg_size = clipmap_iw4::VERTS_PER_SEGMENT;
        let material = clip.materials.len() as u16;
        clip.materials.push(asset_world::ClipMapMaterial {
            name: "h1_model_collision".to_owned(),
            surface_flags: 0,
            content_flags: asset_world::world_iw7::IW7_SOLID,
        });
        let mut leaf_nodes: Vec<clipmap_iw4::ClipAabbNode> = Vec::with_capacity(parts);
        let bounds_of = |pts: &mut dyn Iterator<Item = [f32; 3]>| {
            let (mut lo, mut hi) = ([f32::MAX; 3], [f32::MIN; 3]);
            for p in pts {
                for k in 0..3 {
                    lo[k] = lo[k].min(p[k]);
                    hi[k] = hi[k].max(p[k]);
                }
            }
            (lo, hi)
        };
        let node = |lo: [f32; 3], hi: [f32; 3], child_count: u16, u: i32| clipmap_iw4::ClipAabbNode {
            origin: std::array::from_fn(|k| (lo[k] + hi[k]) * 0.5),
            half_size: std::array::from_fn(|k| (hi[k] - lo[k]) * 0.5 + 0.5),
            material_index: material,
            child_count,
            u,
        };
        for chunk in order.chunks(PART_TRIS) {
            let mut start = mesh.verts.len();
            if start % seg_size + 3 * chunk.len() > seg_size {
                start = start.div_ceil(seg_size) * seg_size;
                mesh.verts.resize(start, [0.0; 3]);
            }
            let seg = start / seg_size;
            let seg_base = seg * seg_size;
            let first_tri = mesh.tri_indices.len() / 3;
            for &i in chunk {
                let t = keep[i];
                for p in t.p {
                    mesh.tri_indices.push((mesh.verts.len() - seg_base) as u16);
                    mesh.verts.push(p);
                }
                mesh.tri_surface_flags.push(t.surface_flags);
                mesh.tri_content_flags.push(super::walk_through_glass(t.contents));
                clip.tri_material_index.push(material);
            }
            let part = mesh.partitions.len();
            mesh.partitions.push(clipmap_iw4::ClipPartition {
                tri_count: chunk.len() as u8,
                first_tri: first_tri as i32,
                first_vert_segment: seg.min(usize::from(u16::MAX)) as u16,
                border_count: 0,
                first_border: 0,
            });
            let (l, h) = bounds_of(&mut chunk.iter().flat_map(|&i| keep[i].p));
            leaf_nodes.push(node(l, h, 0, part as i32));
        }
        // (The mesh's other triangles have no flags here when the zone's had
        // none: keep the tables the same length.)
        let tri_total = mesh.tri_indices.len() / 3;
        mesh.tri_surface_flags.resize(tri_total, 0);
        mesh.tri_content_flags
            .resize(tri_total, asset_world::world_iw7::IW7_SOLID);
        clip.tri_material_index.resize(tri_total, 0);
        let mut levels_nodes = vec![leaf_nodes];
        for d in 1..counts.len() {
            let below = &levels_nodes[d - 1];
            let mut up = Vec::with_capacity(counts[d]);
            for (g, group) in below.chunks(FANOUT).enumerate() {
                let (l, h) = bounds_of(&mut group.iter().flat_map(|n| {
                    let a: [f32; 3] = std::array::from_fn(|k| n.origin[k] - n.half_size[k]);
                    let b: [f32; 3] = std::array::from_fn(|k| n.origin[k] + n.half_size[k]);
                    [a, b]
                }));
                up.push(node(
                    l,
                    h,
                    group.len() as u16,
                    (offsets[d - 1] + g * FANOUT) as i32,
                ));
            }
            levels_nodes.push(up);
        }
        for d in (0..levels_nodes.len()).rev() {
            mesh.aabb_trees.extend_from_slice(&levels_nodes[d]);
        }
        mesh.aabb_roots.push(offsets[counts.len() - 1] as u16);
        Ok(keep.len())
    }

    #[cfg(test)]
    mod tests {
        /// Triangles added to an empty collision are traced: a floor under a
        /// point stops a box dropped onto it.
        #[test]
        fn appended_triangles_are_traced() {
            let mut clip = asset_world::ClipCollision::default();
            let s = 512.0;
            let floor = [
                // (Clockwise from above: the mesh's front faces.)
                asset_world::world_iw7::SoupTri {
                    p: [[-s, -s, 0.0], [s, s, 0.0], [s, -s, 0.0]],
                    surface_flags: 0,
                    contents: 1,
                },
                asset_world::world_iw7::SoupTri {
                    p: [[-s, -s, 0.0], [-s, s, 0.0], [s, s, 0.0]],
                    surface_flags: 0,
                    contents: 1,
                },
            ];
            let many: Vec<_> = (0..300).flat_map(|_| floor).collect();
            assert_eq!(super::append_soup(&mut clip, &many).unwrap(), 600);
            let (frac, _) = asset_world::world_iw3::trace_clip_collision(
                &clip,
                [0.0, 0.0, 64.0],
                [0.0, 0.0, -64.0],
                [-15.0, -15.0, 0.0],
                [15.0, 15.0, 70.0],
                1,
            );
            assert!(frac < 0.6 && frac > 0.4, "{frac}");
            let (miss, _) = asset_world::world_iw3::trace_clip_collision(
                &clip,
                [2000.0, 0.0, 64.0],
                [2000.0, 0.0, -64.0],
                [-15.0, -15.0, 0.0],
                [15.0, 15.0, 70.0],
                1,
            );
            assert_eq!(miss, 1.0);
        }
    }
}

/// H1's drawn world as CoD4's `GfxWorld` (materials and lightmaps go
/// beside it, not in it).
fn iw3_world(
    w: &fastfile_h1::map::World,
    sun: Option<fastfile_h1::map::Sun>,
    exposure: f32,
) -> iw3::zone::GfxWorld {
    let range = |(a, b): (u32, u32)| a.min(b)..b;
    let lit = w.ranges[0].0.min(w.ranges[2].0)..w.ranges[0].1.max(w.ranges[2].1);
    let (angles, colour) = match sun {
        Some(s) => {
            let d = s.dir;
            let yaw = d[1].atan2(d[0]).to_degrees();
            let pitch = (-d[2]).clamp(-1.0, 1.0).asin().to_degrees();
            let peak = s.color.iter().copied().fold(0.0f32, f32::max).max(1e-6);
            ([pitch, yaw, 0.0], (s.color.map(|c| c / peak), peak))
        }
        None => ([-60.0, 45.0, 0.0], ([1.0; 3], 1.0)),
    };
    iw3::zone::GfxWorld {
        name: w.name.clone(),
        base_name: w.name.clone(),
        indices: w.indices.clone(),
        vertices: w
            .vertices
            .iter()
            .map(|v| iw3::zone::WorldVertex {
                xyz: v.xyz,
                binormal_sign: v.binormal_sign,
                color: iw3_colour(v.color),
                tex_coord: v.tex_coord,
                lmap_coord: v.lmap_coord,
                normal: iw3_unit_vec(v.normal),
                tangent: iw3_unit_vec(v.tangent),
            })
            .collect(),
        surfaces: w
            .surfaces
            .iter()
            .map(|s| iw3::zone::GfxSurface {
                first_vertex: s.first_vertex as i32,
                vertex_count: s.vertex_count,
                tri_count: s.tri_count,
                base_index: s.base_index as i32,
                material: None,
                lightmap_index: s.lightmap_index,
                reflection_probe_index: s.reflection_probe_index,
                primary_light_index: 0,
                flags: s.flags,
                bounds: [s.mins, s.maxs],
            })
            .collect(),
        static_models: Vec::new(),
        sky_start_surfs: w.sky_surfaces.iter().map(|&s| s as i32).collect(),
        sky_image: None,
        sun: iw3::zone::SunParse {
            name: String::new(),
            ambient_scale: 0.0,
            ambient_color: [0.0; 3],
            diffuse_fraction: 0.0,
            sun_light: fastfile_h1::map::iw3_sun_gamma(colour.1, exposure),
            sun_color: colour.0,
            diffuse_color: colour.0,
            angles,
        },
        sun_flare: Default::default(),
        sun_color_from_bsp: colour.0,
        lightmaps: Vec::new(),
        reflection_probes: Vec::new(),
        mins: w.mins,
        maxs: w.maxs,
        lit_surfs: lit,
        decal_surfs: range(w.ranges[1]),
        emissive_surfs: range(w.ranges[3]),
        models: w
            .brush_models
            .iter()
            .map(|m| iw3::zone::GfxBrushModel {
                bounds: [m.mins, m.maxs],
                surface_count: m.surface_count.min(u32::from(u16::MAX)) as u16,
                start_surf_index: m.start_surf.min(u32::from(u16::MAX)) as u16,
            })
            .collect(),
        light_grid: Default::default(),
        dpvs: None,
    }
}

/// Why a world surface is not drawn, by its technique set's name: the
/// sky's (the sky pass draws behind them), tool, shadow-only, distortion
/// and exposure-portal surfaces.
fn hidden_kind(techset: &str) -> Option<&'static str> {
    let words: Vec<&str> = techset.split('_').collect();
    if words.iter().any(|w| w.starts_with("sky")) {
        Some("sky")
    } else if words.contains(&"tools") {
        Some("tools")
    } else if words.contains(&"shadowcaster") || words.contains(&"onlycast") {
        Some("shadow only")
    } else if words.contains(&"distortion") {
        Some("distortion")
    } else if words.contains(&"distfalloff") {
        Some("exposure portal")
    } else {
        None
    }
}

/// The materials a map's drawn world and placed models use (their parts'
/// too), and the material named `extra` (the compass), by capture index.
fn used_materials(
    cap: &Capture,
    world: &fastfile_h1::map::World,
    extra: &str,
) -> std::collections::BTreeSet<usize> {
    let mut out: std::collections::BTreeSet<usize> = world
        .surfaces
        .iter()
        .filter_map(|s| s.material.map(|m| m.index as usize))
        .collect();
    let mut models: Vec<u32> = world
        .static_models
        .iter()
        .filter_map(|sm| sm.model.map(|m| m.index))
        .collect();
    models.sort_unstable();
    models.dedup();
    let mut seen = std::collections::HashSet::new();
    while let Some(i) = models.pop() {
        if !seen.insert(i) {
            continue;
        }
        let Some(x) = cap.xmodels.get(i as usize) else {
            continue;
        };
        out.extend(x.materials.iter().flatten().map(|m| m.index as usize));
        models.extend(x.composite.iter().flatten().map(|m| m.index));
    }
    let key = fastfile_h1::shared::image_key(extra);
    out.extend(
        cap.materials
            .iter()
            .position(|m| fastfile_h1::shared::image_key(&m.name) == key),
    );
    out
}

/// Where a map's spawn points stand: on their floor (within
/// `SPAWN_FLOOR_REACH`), placed above it (the drawn world has nothing
/// between the point and the floor the collision has: the map's authors
/// put it in the air, and Black Ops II's `placespawnpoint` drops it there,
/// as H1's does), or over a drawn floor the collision lacks (the player
/// would fall through it).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct SpawnFloors {
    spawns: usize,
    on_ground: usize,
    above: usize,
    /// The highest a point placed above its floor stands over it.
    above_max: u32,
    floor_missing: usize,
    no_floor: usize,
}

impl std::fmt::Display for SpawnFloors {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{} spawns: {} on their floor, {} placed above it (up to {} units, nothing drawn between: dropped to it as the game does), {} over a drawn floor the collision lacks, {} over nothing",
            self.spawns,
            self.on_ground,
            self.above,
            self.above_max,
            self.floor_missing,
            self.no_floor
        )
    }
}

fn spawn_floors(
    clip: &asset_world::ClipCollision,
    world: &iw3::zone::GfxWorld,
    hidden: &[bool],
    spawns: &[fastfile_h1::map::SpawnRecord],
) -> SpawnFloors {
    const MINS: [f32; 3] = [-15.0, -15.0, 0.0];
    const MAXS: [f32; 3] = [15.0, 15.0, 70.0];
    const DROP: f32 = 2048.0;
    let reach = asset_world::world_iw3::SPAWN_FLOOR_REACH;
    // Whether a drawn triangle crosses the vertical at (x, y) between
    // heights `lo` and `hi`.
    let drawn_between = |x: f32, y: f32, lo: f32, hi: f32| -> bool {
        world.surfaces.iter().enumerate().any(|(i, s)| {
            if hidden.get(i).copied().unwrap_or(false)
                || x < s.bounds[0][0]
                || x > s.bounds[1][0]
                || y < s.bounds[0][1]
                || y > s.bounds[1][1]
                || s.bounds[1][2] < lo
                || s.bounds[0][2] > hi
            {
                return false;
            }
            let first = s.first_vertex.max(0) as usize;
            let base = s.base_index.max(0) as usize;
            let tris = world
                .indices
                .get(base..base + 3 * usize::from(s.tri_count))
                .unwrap_or(&[]);
            tris.chunks_exact(3).any(|t| {
                let Some(q) = [t[0], t[1], t[2]]
                    .iter()
                    .map(|&ix| world.vertices.get(first + usize::from(ix)).map(|v| v.xyz))
                    .collect::<Option<Vec<_>>>()
                else {
                    return false;
                };
                let d = (q[1][1] - q[2][1]) * (q[0][0] - q[2][0])
                    + (q[2][0] - q[1][0]) * (q[0][1] - q[2][1]);
                if d.abs() < 1e-6 {
                    return false;
                }
                let a =
                    ((q[1][1] - q[2][1]) * (x - q[2][0]) + (q[2][0] - q[1][0]) * (y - q[2][1])) / d;
                let b =
                    ((q[2][1] - q[0][1]) * (x - q[2][0]) + (q[0][0] - q[2][0]) * (y - q[2][1])) / d;
                let c = 1.0 - a - b;
                if a < 0.0 || b < 0.0 || c < 0.0 {
                    return false;
                }
                let h = a * q[0][2] + b * q[1][2] + c * q[2][2];
                h > lo && h < hi
            })
        })
    };
    let mut out = SpawnFloors {
        spawns: spawns.len(),
        ..Default::default()
    };
    for s in spawns {
        let o = s.origin;
        let (down, _) = asset_world::world_iw3::trace_clip_collision(
            clip,
            [o[0], o[1], o[2] + 1.0],
            [o[0], o[1], o[2] + 1.0 - DROP],
            MINS,
            MAXS,
            asset_world::MASK_PLAYER_SOLID,
        );
        if down >= 1.0 {
            out.no_floor += 1;
            continue;
        }
        let fall = DROP * down - 1.0;
        if fall <= reach {
            out.on_ground += 1;
        } else if drawn_between(o[0], o[1], o[2] - fall + 8.0, o[2] + 1.0) {
            out.floor_missing += 1;
        } else {
            out.above += 1;
            out.above_max = out.above_max.max(fall.round() as u32);
        }
    }
    out
}

#[allow(clippy::too_many_arguments)]
fn build_h1_world(
    path: &Path,
    zone: &fastfile_h1::Zone,
    cap: &Capture,
    map: &IwMap,
    mut report: Vec<String>,
    progress: &LoadProgress,
    material_seed: MaterialCatalog,
    colour: &mut HashMap<usize, ColourMap>,
    native: bool,
) -> LoadedWorld {
    super::bo2_look::set_pending_grade(None);
    let map_name = path
        .file_stem()
        .map_or_else(String::new, |s| s.to_string_lossy().into_owned());
    let lower = map_name.to_ascii_lowercase();
    let zone_dir = super::h1::h1_zone_dir(path);
    report.push(format!(
        "h1 install: {}",
        zone_dir.as_ref().map_or_else(
            || "not found (the map's own pictures only)".to_owned(),
            |d| d.display().to_string()
        )
    ));
    let level_cut = fastfile_h1::map::alternate_level_cut(&map.spawns);

    // Collision: the clip map, and the placed models for the model trace.
    let stage = progress.begin_scoped(StageId::MapAssets, "mwr collision", None);
    let collision = match map
        .clip
        .as_ref()
        .map(|c| asset_world::world_iw3::build_iw3_clip_collision(&iw3_clip(c)))
    {
        Some(Ok(mut clip)) => {
            if let Some(src) = &map.clip {
                let mesh = std::sync::Arc::make_mut(&mut clip.mesh);
                for (p, &(_, _, segment)) in mesh.partitions.iter_mut().zip(&src.partitions) {
                    p.first_vert_segment = segment.into();
                }
                if soup_collision() {
                    let mut models = soup::model_collision(cap, &zone.image, src, level_cut);
                    let cleared = soup::clear_spawn_boxes(&mut models.tris, &map.spawns);
                    match soup::append_soup(&mut clip, &models.tris) {
                        Ok(n) => report.push(format!(
                            "h1 model collision (BO2MP_H1_COLLISION=soup, the old way): {n} triangles from {} placed models ({} LODs; {} small left out, {} with no collision LOD; {cleared} in a spawn point's player box left out)",
                            models.models,
                            if models.coarse { "coarsest" } else { "collision" },
                            models.small,
                            models.no_lod
                        )),
                        Err(e) => report.push(format!("h1 model collision gap: {e}")),
                    }
                } else {
                let models = model_collision(cap, &zone.image, src, level_cut);
                report.push(format!(
                    "h1 model collision: {} placed models for the model trace (line traces only: bullets, sight), {} triangles; {} with no collision LOD, {} skipped (no model, or over the alternate level), {} with a layer byte, {} rigid lists without a collision surface",
                    models.models.len(),
                    models.tris,
                    models.no_lod,
                    models.skipped,
                    models.layered,
                    models.unpaired
                ));
                clip.static_models = models.models;
                }
                // The breakable glass: each glass brush takes the pane in it,
                // so a shot on the brush can break that pane.
                if render_h1::glass::enabled() && !cap.fx_glass.is_empty() {
                    let panes: Vec<render_h1::glass::Pane> = cap
                        .fx_glass
                        .iter()
                        .flat_map(|rec| render_h1::glass::panes(rec, &zone.image))
                        .collect();
                    let centers: Vec<[f32; 3]> = panes.iter().map(|p| p.center).collect();
                    let bounds: Vec<Option<([f32; 3], [f32; 3])>> = src
                        .brushes
                        .iter()
                        .map(|b| (b.contents as u32 & 0x10 != 0).then_some((b.mins, b.maxs)))
                        .collect();
                    let tags = render_h1::glass::tag_brushes(&centers, &bounds, 2.0);
                    let mut tagged = 0usize;
                    for (brush, &tag) in clip.brushes.iter_mut().zip(&tags) {
                        if tag != 0 {
                            brush.glass_encoded = tag;
                            tagged += 1;
                        }
                    }
                    clip.glass_panes = panes.iter().map(|p| (p.origin, p.axes[0], p.axes[1])).collect();
                    report.push(format!(
                        "h1 glass: {} panes, {} of {} glass brushes tied to a pane (a shot or blast on them breaks it)",
                        panes.len(),
                        tagged,
                        bounds.iter().flatten().count()
                    ));
                }
            }
            clip.trigger_models = map
                .trigger_models
                .iter()
                .map(|hulls| {
                    hulls
                        .iter()
                        .map(|h| asset_world::MapTriggerHull {
                            mid: h.mid,
                            half: h.half,
                            slabs: h.slabs.clone(),
                        })
                        .collect()
                })
                .collect();
            let origins: Vec<[f32; 3]> = map.spawns.iter().map(|s| s.origin).collect();
            let census = asset_world::world_iw3::spawn_ground_census(&clip, &origins);
            report.push(format!(
                "h1 clip: {} brushes ({} the world's), {} mesh tris in {} trees, {} models, {} trigger models; {census}",
                clip.brushes.len(),
                clip.cmodels.first().map_or(0, |m| m.num_brushes),
                clip.mesh.tri_indices.len() / 3,
                clip.mesh.aabb_roots.len(),
                clip.cmodels.len(),
                clip.trigger_models.len(),
            ));
            Some(clip)
        }
        Some(Err(error)) => {
            report.push(format!("h1 clip gap: {error}"));
            None
        }
        None => {
            report.push("h1 clip gap: zone has no clipMap".to_owned());
            None
        }
    };
    stage.done();

    // Entities and spawns. Spawn records over nothing are left out (as
    // Ghosts' maps, a mode's spawns can be parked in a corner).
    let mut spawn_records = map.spawns.clone();
    if let Some(cut) = level_cut {
        let before = spawn_records.len();
        spawn_records.retain(|s| s.origin[2] < cut);
        report.push(format!(
            "h1 second level above z {cut:.0} left out: {} of {before} spawn records there",
            before - spawn_records.len()
        ));
    }
    let mut parked = 0usize;
    if let Some(clip) = &collision {
        const MINS: [f32; 3] = [-15.0, -15.0, 0.0];
        const MAXS: [f32; 3] = [15.0, 15.0, 70.0];
        spawn_records.retain(|s| {
            let o = s.origin;
            let (down, _) = asset_world::world_iw3::trace_clip_collision(
                clip,
                [o[0], o[1], o[2] + 16.0],
                [o[0], o[1], o[2] - 1024.0],
                MINS,
                MAXS,
                asset_world::MASK_PLAYER_SOLID,
            );
            let keep = down < 1.0;
            parked += usize::from(!keep);
            keep
        });
    }
    // (The text's own spawn entities over nothing go too: the records name
    // the same points.)
    let kept: std::collections::HashSet<[i64; 3]> = spawn_records
        .iter()
        .map(|s| s.origin.map(|v| (v * 8.0).round() as i64))
        .collect();
    let dropped: std::collections::HashSet<[i64; 3]> = map
        .spawns
        .iter()
        .map(|s| s.origin.map(|v| (v * 8.0).round() as i64))
        .filter(|o| !kept.contains(o))
        .collect();
    let mut entity_string = IwMap {
        spawns: spawn_records,
        entities: map.entities.clone(),
        ..Default::default()
    }
    .entity_string();
    // bo2mp: the blocks the game's map load spawns, as it spawns them
    // (`fastfile_h1::spawn`, read from the exe: vehicle nodes and actors
    // make no entity, a class takes its table class by the exe's match),
    // and what the map's start scripts delete in an ordinary match (Old
    // School pickups, `delete_on_load`, hidden exploder models: inferred,
    // the scripts are not in the staged zones). `BO2MP_H1_MAP_LOGIC=0`
    // keeps the text as read.
    if std::env::var("BO2MP_H1_MAP_LOGIC").map_or(true, |v| v.trim() != "0") {
        let (text, logic) = fastfile_h1::spawn::as_spawned(&entity_string, true);
        entity_string = text;
        report.push(logic.line());
    }
    let mut entities = iw3::ents::parse(&entity_string);
    if !dropped.is_empty() {
        entities.retain(|e| {
            let c = e.classname();
            let spawn = c.starts_with("mp_") && c.contains("spawn");
            !(spawn
                && e.origin()
                    .is_some_and(|o| dropped.contains(&o.map(|v| (v * 8.0).round() as i64))))
        });
    }
    let placed_bmodels: Vec<PlacedBrush> = fastfile_h1::map::placed_brush_models(map, level_cut)
        .into_iter()
        .map(|b| (b.surfaces, b.origin, b.axis))
        .collect();
    let spawns = spawns_of(&entities);
    // The map's intermission camera (`mp_global_intermission`): where the
    // game holds the view before a team is picked (HorizonMW's reference
    // shots are taken from it).
    let intermission_view = entities
        .iter()
        .find(|e| e.classname() == "mp_global_intermission")
        .and_then(|e| e.origin().map(|origin| asset_world::IntermissionView { origin, angles: e.angles() }));
    if let Some(v) = &intermission_view {
        report.push(format!("h1 intermission: {:?} angles {:?}", v.origin, v.angles));
    }
    report.push(format!(
        "h1 entities: {}, spawn points {} ({} spawn records, {parked} over nothing left out), {} trigger models",
        entities.len(),
        spawns.len(),
        map.spawns.len(),
        map.trigger_models.len()
    ));
    let mut scripts = crate::ScriptSources::default();
    scripts.set_entities(entity_string);
    let corners: Vec<[f32; 2]> = entities
        .iter()
        .filter(|e| e.get("targetname") == Some("minimap_corner"))
        .filter_map(|e| e.origin().map(|o| [o[0], o[1]]))
        .collect();
    let mut facts = crate::MapFacts::default();
    if let [a, b] = corners[..] {
        facts.minimap_corners = Some(asset_world::MinimapCorners { a, b });
    }

    let Some(world) = map.world.as_ref() else {
        let mut loaded = LoadedWorld::with_gap(
            WorldDrawPolicy::t5(),
            PreparedCapability::PreparedWorld,
            "Modern Warfare Remastered zone has no GfxWorld",
            Some("assets::lane::h1_map::load_world/no_world"),
        );
        loaded.scripts = scripts;
        loaded.collision = collision;
        loaded.spawns = spawns;
        loaded.report.extend(report);
        return loaded;
    };

    // Materials: world surfaces, then the placed models'.
    let stage = progress.begin_scoped(StageId::MapAssets, "mwr materials", None);
    let max = texture_max();
    let mut materials = material_seed;
    materials.set_capture_ns(AssetNamespace::T6);
    let mut b = H1Builder::new(cap, zone, path, zone_dir.as_deref(), materials);
    b.world_shine = look_on();
    // bo2mp: the raw files a retail map keeps in its patch zones (the
    // createfx, fog and sun scripts in `patch_<map>`, the film vision in
    // `patch_common_mp`); a usermap holds its own.
    let raws = zone_dir.as_deref().map_or_else(Default::default, |dir| {
        let r = fastfile_h1::map::PatchRaws::read(dir, &lower, &super::h1_fx::patch_names(&lower), cap, &zone.image);
        if !r.zones.is_empty() {
            report.push(format!("h1 patch zones read for the map's scripts: {}", r.zones.join(", ")));
        }
        r
    });
    // The shared zones only when the map names something it does not hold
    // (both usermaps seen hold everything; a retail map names common_mp's).
    let names_shared = cap.images.iter().any(fastfile_h1::shared::is_reference)
        || cap
            .materials
            .iter()
            .any(|m| m.techset.is_none() && m.textures.is_empty());
    if names_shared {
        match zone_dir.as_deref() {
            Some(dir) => {
                let started = std::time::Instant::now();
                let wanted = fastfile_h1::shared::Wanted::of_map_materials(
                    cap,
                    &used_materials(cap, world, &format!("compass_map_{lower}")),
                );
                let asked = (wanted.materials.len(), wanted.images.len());
                let shared = super::h1::h1_shared(dir, &lower, wanted);
                report.push(format!(
                    "h1 shared zones: the map names {} materials and {} pictures it does not hold; {} materials and {} pictures ({} KB inline) kept from {}; {} materials and {} pictures in none{} [{:.1}s]",
                    asked.0,
                    asked.1,
                    shared.images.material_count(),
                    shared.images.len(),
                    shared.images.inline_bytes / 1024,
                    shared.lines.join(", "),
                    shared.unfound.0,
                    shared.unfound.1,
                    if shared.missing.is_empty() {
                        String::new()
                    } else {
                        format!("; missing {:?}", shared.missing)
                    },
                    started.elapsed().as_secs_f32()
                ));
                b.shared = Some(shared);
            }
            None => report.push(
                "h1 shared zones: none (no install found): what the map only names draws grey"
                    .to_owned(),
            ),
        }
    }
    let mut hidden = vec![false; world.surfaces.len()];
    let mut surface_materials = Vec::with_capacity(world.surfaces.len());
    let mut hidden_kinds: HashMap<&'static str, usize> = HashMap::new();
    let decal_materials: std::collections::HashSet<u32> = {
        let (a, e) = world.ranges[1];
        world.surfaces
            [(a.min(e) as usize).min(world.surfaces.len())..(e as usize).min(world.surfaces.len())]
            .iter()
            .filter_map(|s| s.material.map(|m| m.index))
            .collect()
    };
    // bo2mp: in a match MWR's own renderer (`render_h1`) draws the map and
    // reads its own pictures, so the converted world keeps no surface, no
    // material and no placed model mesh (it held GBs of pictures next to the
    // native ones); what stays is what the game's other passes take from it:
    // the light for the players and the gun, collision, spawns, entities,
    // scripts, the compass. (Black Ops III's native lane does the same.)
    // `BO2MP_H1_CONVERTED_WORLD=1` builds it all anyway.
    let native = native && !std::env::var("BO2MP_H1_CONVERTED_WORLD").is_ok_and(|v| v.trim() == "1");
    if native {
        report.push("h1 world: drawn by the native renderer; the converted world keeps only its light".to_owned());
    }
    for (i, s) in world.surfaces.iter().enumerate() {
        let Some(m) = s.material else {
            surface_materials.push(None);
            continue;
        };
        if native {
            hidden[i] = true;
            surface_materials.push(None);
            continue;
        }
        // (A retail map's materials are names: the technique set is the
        // definition's, in `techsets_<map>`.)
        let techset = b.techset_name(m.index as usize);
        let kind = if level_cut.is_some_and(|cut| s.mins[2] > cut) {
            Some("second level")
        } else {
            hidden_kind(&techset)
        };
        if let Some(kind) = kind {
            hidden[i] = true;
            *hidden_kinds.entry(kind).or_default() += 1;
            surface_materials.push(None);
            continue;
        }
        surface_materials.push(b.world_material(
            m.index as usize,
            max,
            decal_materials.contains(&m.index),
        ));
    }
    // (A sky stored as half floats (BC6H, DXGI 95/96) holds its radiance
    // already; the global factor below is for the UNORM pictures.)
    let mut sky_hdr = false;
    let sky_line = match b.sky_material(&world.sky_images) {
        Ok((name, _, [w, h, format])) => {
            sky_hdr = matches!(format, 95 | 96);
            format!("h1 sky: {name} (cube {w}x{h}, format {format})")
        }
        Err(e) => format!("h1 sky: {e}"),
    };
    report.push(sky_line);
    report.push(format!(
        "h1 materials: {} world materials linked, {} pictures decoded (at most {max}px; {} from {:?} or built in; {} materials from the shared zones, {} named but in none{}), {} failed{}; not drawn: {:?}",
        b.linked(),
        b.decoded,
        b.from_shared,
        b.shared.as_ref().map_or_else(Vec::new, |s| s.images.zones.clone()),
        b.shared_materials,
        b.unresolved_materials.len(),
        if b.unresolved_materials.is_empty() { String::new() } else { format!(" {:?}", &b.unresolved_materials[..b.unresolved_materials.len().min(4)]) },
        b.image_failures.len(),
        if b.image_failures.is_empty() { String::new() } else { format!(" (first: {})", b.image_failures.iter().take(4).cloned().collect::<Vec<_>>().join("; ")) },
        hidden_kinds
    ));

    // The map's exposure, from its baked light.
    let light_key = fastfile_h1::map::lightmap_key(cap, &zone.image, &world.lightmaps, &mut b.paks);
    let mut exposure = fastfile_h1::map::light_exposure(light_key);
    report.push(format!(
        "h1 exposure: {exposure:.3} (baked light 90th percentile {})",
        light_key.map_or_else(|| "unmeasured".to_owned(), |k| format!("{k:.3}"))
    ));
    // Black Ops III's look: the baked light scaled so its shaded texels'
    // mean is `LOOK_SHADE`; whether it holds the sun (then no sun is drawn
    // over it).
    let look = look_on().then(|| {
        let splits: Vec<[f64; 4]> = world
            .lightmaps
            .iter()
            .filter_map(|&pair| fastfile_h1::map::iw3_lightmap(cap, &zone.image, pair, 1.0, true, &mut b.paks).ok())
            .map(|l| l.split)
            .collect();
        let (mut shade, mut n) = (0.0f64, 0.0f64);
        for sp in &splits {
            shade += sp[2] * sp[3];
            n += sp[3];
        }
        let shade = if n > 0.0 { (shade / n) as f32 } else { light_key.unwrap_or(0.3) * 0.5 };
        let ratio = fastfile_h1::map::sun_in_lightmap(&splits);
        let sun_baked = ratio.is_some_and(|r| r >= SUN_IN_BAKE);
        // (Under MWR's own shaders with its automatic exposure the bake is
        // drawn raw, as the game does: the exposure finds the level;
        // `BO2MP_H1_RAW=0` keeps the old scaling to a shaded mean.)
        // (Raw also under a fixed exposure, `BO2MP_H1_AUTO=0` with
        // `BO2MP_H1_EXPOSURE`: the game's light in its own units.)
        let raw = mwr_on() && std::env::var("BO2MP_H1_RAW").map_or(true, |v| v.trim() != "0");
        let scale = if raw { 1.0 } else { (env_f("BO2MP_H1_SHADE", LOOK_SHADE) / shade.max(1e-4)).clamp(0.01, 64.0) };
        report.push(format!(
            "h1 look: baked light's shaded mean {shade:.3}, sunlit over shaded {}, scaled x{scale:.3}; {}",
            ratio.map_or_else(|| "unmeasured".to_owned(), |r| format!("{r:.2}")),
            if sun_baked { "the sun is in the bake (none drawn over it)" } else { "the sun drawn over a sun map" }
        ));
        (scale, sun_baked)
    });
    if let Some((scale, _)) = look {
        exposure = scale;
    }

    // The placed models.
    let (prop_meshes, mut prop_placements) = {
        use bevy::prelude::{Mat3, Quat, Transform, Vec3};
        let mut meshes = Vec::new();
        let mut shared: HashMap<u32, Option<usize>> = HashMap::new();
        let mut placements = vec![None; world.static_models.len()];
        let (mut unreadable, mut ground_lit) = (Vec::new(), 0usize);
        let failures_before = b.image_failures.len();
        // A model lit by H1's light grid (most are) takes the middle light
        // of the ground-lit ones; with none, a neutral one at the map's
        // exposure key.
        let pack = |g: [f32; 4]| -> [u8; 4] {
            let p = |v: f32| {
                ((v.max(0.0) / 32.0).sqrt() * 255.0)
                    .round()
                    .clamp(1.0, 255.0) as u8
            };
            [p(g[0]), p(g[1]), p(g[2]), 0]
        };
        let lit_of = |sm: &fastfile_h1::map::StaticModel| -> Option<[f32; 4]> {
            let model_k = if look.is_some() { env_f("BO2MP_H1_MODEL", LOOK_MODEL) } else { 1.0 };
            let g = sm
                .ground_lighting
                .map(|v| if v.is_finite() { v * exposure * model_k } else { 0.0 });
            (sm.flags & 0x20 != 0 && g[0] + g[1] + g[2] > 0.0).then_some(g)
        };
        let fallback = {
            let mut lums: Vec<[f32; 4]> = world.static_models.iter().filter_map(lit_of).collect();
            lums.sort_by(|a, b| (a[0] + a[1] + a[2]).total_cmp(&(b[0] + b[1] + b[2])));
            lums.get(lums.len() / 2)
                .copied()
                .unwrap_or([fastfile_h1::map::EXPOSURE_KEY; 4])
        };
        for (i, sm) in world.static_models.iter().enumerate() {
            let Some(id) = sm.model else { continue };
            if native {
                continue;
            }
            if level_cut.is_some_and(|cut| sm.origin[2] > cut) {
                continue;
            }
            let entry = *shared.entry(id.index).or_insert_with(|| {
                let model = cap.xmodels.get(id.index as usize)?;
                let Some((mut xref, keys)) = b.model_ref(model) else {
                    if unreadable.len() < 8 {
                        unreadable.push(model.name.clone());
                    }
                    return None;
                };
                // (A placed model's vertex colour is wind and motion
                // weights, as Ghosts': not a tint.)
                for surf in &mut xref.lod0 {
                    for v in surf.verts.chunks_exact_mut(32) {
                        v[16..20].fill(0xff);
                    }
                }
                let linked: HashMap<usize, usize> = keys
                    .into_iter()
                    .filter_map(|k| Some((k, b.material(k, max)?)))
                    .collect();
                let mesh =
                    asset_world::build_t6_model_mesh(&xref, |k| linked.get(&k.index).copied())?;
                meshes.push(mesh);
                Some(meshes.len() - 1)
            });
            let Some(mesh) = entry else { continue };
            let g = match lit_of(sm) {
                Some(g) => {
                    ground_lit += 1;
                    g
                }
                None => fallback,
            };
            let basis = Mat3::from_cols(
                Vec3::from_array(sm.axis[0]),
                Vec3::from_array(sm.axis[1]),
                Vec3::from_array(sm.axis[2]),
            );
            placements[i] = Some(asset_world::StaticModelPlacement {
                mesh,
                transform: Transform {
                    translation: Vec3::from_array(sm.origin),
                    rotation: Quat::from_mat3(&basis),
                    scale: Vec3::splat(sm.scale),
                },
                origin: sm.origin,
                axis: sm.axis,
                scale: sm.scale,
                cull_dist: sm.cull_dist.clamp(0.0, f32::from(u16::MAX)) as u16,
                reflection_probe_index: 0,
                primary_light_index: 0,
                flags: sm.flags as u8,
                packed_lighting: Some(pack(g)),
                t6_primary: [0, 0],
            });
        }
        // The map's script models that stand as scenery (Crash's downed
        // helicopter, its burnt technicals): `script_model` entities the
        // game modes do not own (no `script_gameobjectname`, not a pickup),
        // drawn as placed models (`BO2MP_H1_SCRIPT_MODELS=0` leaves them out).
        let mut script_placed = 0usize;
        if std::env::var("BO2MP_H1_SCRIPT_MODELS").map_or(true, |v| v.trim() != "0") {
            let by_name: HashMap<&str, u32> = cap
                .xmodels
                .iter()
                .enumerate()
                .map(|(i, x)| (x.name.trim_start_matches(',').trim(), i as u32))
                .collect();
            for e in &entities {
                if e.classname() != "script_model" || e.get("script_gameobjectname").is_some() {
                    continue;
                }
                let target = e.get("targetname").unwrap_or("");
                if target.contains("pickup") || target.contains("bomb") || target.contains("flag") {
                    continue;
                }
                let (Some(name), Some(origin)) = (e.get("model"), e.origin()) else { continue };
                let Some(&xi) = by_name.get(name) else { continue };
                let entry = *shared.entry(xi).or_insert_with(|| {
                    let model = cap.xmodels.get(xi as usize)?;
                    let (mut xref, keys) = b.model_ref(model)?;
                    for surf in &mut xref.lod0 {
                        for v in surf.verts.chunks_exact_mut(32) {
                            v[16..20].fill(0xff);
                        }
                    }
                    let linked: HashMap<usize, usize> = keys
                        .into_iter()
                        .filter_map(|k| Some((k, b.material(k, max)?)))
                        .collect();
                    let mesh = asset_world::build_t6_model_mesh(&xref, |k| linked.get(&k.index).copied())?;
                    meshes.push(mesh);
                    Some(meshes.len() - 1)
                });
                let Some(mesh) = entry else { continue };
                let [pitch, yaw, roll] = e.angles().map(f32::to_radians);
                let rotation = Quat::from_rotation_z(yaw) * Quat::from_rotation_y(pitch) * Quat::from_rotation_x(roll);
                let m = Mat3::from_quat(rotation);
                let axis = [m.x_axis.to_array(), m.y_axis.to_array(), m.z_axis.to_array()];
                placements.push(Some(asset_world::StaticModelPlacement {
                    mesh,
                    transform: Transform { translation: Vec3::from_array(origin), rotation, scale: Vec3::ONE },
                    origin,
                    axis,
                    scale: 1.0,
                    cull_dist: 0,
                    reflection_probe_index: 0,
                    primary_light_index: 0,
                    flags: 0,
                    packed_lighting: Some(pack(fallback)),
                    t6_primary: [0, 0],
                }));
                script_placed += 1;
            }
        }
        report.push(format!("h1 script models drawn as scenery: {script_placed}"));
        report.push(format!(
            "h1 static models: {} placed of {}, {} distinct models built ({} unreadable {:?}), {ground_lit} with their own ground light (the rest the middle one), {} more picture failures",
            placements.iter().filter(|p| p.is_some()).count(),
            world.static_models.len(),
            meshes.len(),
            unreadable.len(),
            unreadable,
            b.image_failures.len() - failures_before
        ));
        (meshes, placements)
    };

    // The baked light, in CoD4's form.
    let mut lightmap_gaps = Vec::new();
    // (Each page's secondary as converted, kept to light placed models from
    // the ground under them where the light grid is not read.)
    let mut lightmap_cpu: Vec<Option<(usize, usize, Vec<u8>)>> = Vec::new();
    let lightmaps: Vec<Option<asset_world::WorldLightmap>> = world
        .lightmaps
        .iter()
        .enumerate()
        .map(|(i, &pair)| {
            let names = (
                pair.0
                    .map_or_else(String::new, |id| cap.name_of(id).to_owned()),
                pair.1
                    .map_or_else(String::new, |id| cap.name_of(id).to_owned()),
            );
            let page =
                match fastfile_h1::map::iw3_lightmap(cap, &zone.image, pair, exposure, look.is_some(), &mut b.paks)
                {
                    Ok(page) => page,
                    Err(e) => {
                        lightmap_gaps.push(format!("page {i}: {e}"));
                        return None;
                    }
                };
            let (pw, ph, p) = &page.primary;
            let (sw, sh, s) = &page.secondary;
            lightmap_cpu.resize(i + 1, None);
            lightmap_cpu[i] = Some((usize::from(*sw), usize::from(*sh), s.clone()));
            match asset_world::world_iw3::iw3_lightmap(
                (
                    *pw,
                    *ph,
                    asset_world::world_iw3::IW3_LIGHTMAP_PRIMARY_FORMAT,
                    p,
                ),
                (
                    *sw,
                    *sh,
                    asset_world::world_iw3::IW3_LIGHTMAP_SECONDARY_FORMAT,
                    s,
                ),
                (&names.0, &names.1),
            ) {
                Ok(page) => Some(page),
                Err(e) => {
                    lightmap_gaps.push(format!("page {i}: {e:?}"));
                    None
                }
            }
        })
        .collect();
    report.push(format!(
        "h1 lightmaps: {}/{} pages converted{}",
        lightmaps.iter().filter(|p| p.is_some()).count(),
        lightmaps.len(),
        if lightmap_gaps.is_empty() {
            String::new()
        } else {
            format!(" ({})", lightmap_gaps.join("; "))
        }
    ));
    {
        let (mut decoded, mut absent, mut missing) = (0usize, 0usize, Vec::new());
        let mut paks = std::collections::BTreeSet::new();
        for (&m, state) in &b.colour {
            match state {
                ColourMap::Decoded => decoded += 1,
                ColourMap::PakAbsent(p) => {
                    absent += 1;
                    paks.insert(
                        Path::new(p)
                            .file_name()
                            .map_or_else(String::new, |f| f.to_string_lossy().into_owned()),
                    );
                }
                ColourMap::Missing(why) => missing.push(format!(
                    "{}: {why}",
                    b.catalog.materials.get(m).map_or("?", |m| m.name.as_str())
                )),
            }
        }
        missing.sort();
        report.push(format!(
            "h1 colour maps: {decoded} drawn, {absent} in image paks not installed ({paks:?}), {} missing{}",
            missing.len(),
            if missing.is_empty() {
                String::new()
            } else {
                format!(" (first: {})", missing[..missing.len().min(6)].join("; "))
            }
        ));
    }
    // The compass: Black Ops II's HUD takes the picture by the name its
    // script's `setminimap` gives (`compass_map_<map>`), as a Black Ops III
    // map's (`super::t7_fx`). H1's `compass_map_<map>` material names it
    // (`h1_og_compass_map_<map>` on the retail maps).
    let compass_name = format!("compass_map_{lower}");
    super::t7_fx::set_pending_compass(None);
    let compass_picture = match b.material_picture(&compass_name, 2048) {
        Ok((picture, (w, h, px))) => {
            super::t7_fx::set_pending_compass(Some((compass_name.clone(), w, h, px)));
            facts.compass.material = Some(compass_name.clone());
            facts.compass.image = Some(compass_name.clone());
            format!("{compass_name} ({picture} {w}x{h})")
        }
        Err(e) => format!("missing ({e})"),
    };
    report.push(format!(
        "h1 compass: corners {}, picture {compass_picture}",
        if facts.minimap_corners.is_some() {
            "found"
        } else {
            "missing"
        },
    ));
    // bo2mp: the map's createfx (rain, mist, dust, lights), its sprites
    // linked into the same material catalog.
    super::h1_fx::prepare(&mut b, &raws, &lower, path, zone_dir.as_deref(), &mut report);
    *colour = std::mem::take(&mut b.colour);
    let reveal_materials = std::mem::take(&mut b.reveal);
    let mut materials = std::mem::take(&mut b.catalog);
    drop(b);
    stage.done();

    // The map's film vision (`vision/<map>.vision`).
    let vision_name = format!("vision/{lower}.vision");
    let film_vision = match raws.get(cap, &zone.image, &vision_name) {
        Some(bytes) => match asset_world::parse_film_vision_rawfile(&vision_name, &bytes, false) {
            Ok(Some(v)) => {
                report.push(format!(
                    "film vision: READY source=zone enable={} contrast={:.4} brightness={:.4} desaturation={:.5}",
                    v.enable, v.contrast, v.brightness, v.desaturation
                ));
                Some(v)
            }
            Ok(None) => None,
            Err(e) => {
                report.push(format!("film vision: {vision_name} unreadable ({e:?})"));
                None
            }
        },
        None => {
            report.push(format!("film vision: {vision_name} not in the zone"));
            None
        }
    };
    // (Black Ops III's look grades through its own tone curve: the map's
    // film vision, made for H1's own output, over it saturated Seatown
    // and Rust to yellow and orange. `BO2MP_H1_VISION=1` keeps it.)
    let film_vision = if look_on() && std::env::var("BO2MP_H1_VISION").map_or(true, |v| v.trim() != "1") {
        if film_vision.is_some() {
            report.push("film vision: left out under Black Ops III's look".to_owned());
        }
        None
    } else {
        film_vision
    };

    // The sun's bearing. Horizon MW's usermaps (converted Modern Warfare 2
    // maps): which axes of the stored direction the real game's shadows
    // disagree with is being measured against its pictures
    // (`BO2MP_H1_SUN_FLIP`: "x", "y", "xy", "r90", "r270" or "0"; default
    // "r90" on a usermap, a quarter turn, which matches Rust's real
    // shadows; none on a retail map).
    let flip = std::env::var("BO2MP_H1_SUN_FLIP").map(|v| v.trim().to_ascii_lowercase()).unwrap_or_else(|_| {
        if path.to_string_lossy().to_ascii_lowercase().contains("hmw-usermaps") { "r90".to_owned() } else { "0".to_owned() }
    });
    let sun = map.sun.map(|mut s| {
        // ("r90": the bearing turned a quarter round, x y to -y x; "r270"
        // the other way.)
        if flip == "r90" {
            s.dir = [-s.dir[1], s.dir[0], s.dir[2]];
            return s;
        }
        if flip == "r270" {
            s.dir = [s.dir[1], -s.dir[0], s.dir[2]];
            return s;
        }
        if flip.contains('x') {
            s.dir[0] = -s.dir[0];
        }
        if flip.contains('y') {
            s.dir[1] = -s.dir[1];
        }
        s
    });
    if flip.contains('x') || flip.contains('y') || flip.starts_with('r') {
        report.push(format!("h1 sun: stored direction's {flip} turned (a Horizon MW usermap's)"));
    }
    let mut iw3w = iw3_world(world, sun, exposure);
    // The brush models their entities place (doors, walls, care package
    // pallets): drawn where the entity stands. H1's surfaces share their
    // block's vertex range, so only the vertices a surface's triangles use
    // are moved.
    {
        let n = iw3w.surfaces.len();
        let listed = |i: usize| {
            let i = i as u32;
            [&iw3w.lit_surfs, &iw3w.decal_surfs, &iw3w.emissive_surfs]
                .iter()
                .any(|r| r.contains(&i))
        };
        let listed: Vec<bool> = (0..n).map(listed).collect();
        let mut placed = vec![false; n];
        let mut moved = vec![false; iw3w.vertices.len()];
        for (range, origin, axis) in &placed_bmodels {
            for i in range.clone().filter(|&i| i < n) {
                if listed[i] || placed[i] {
                    continue;
                }
                placed[i] = true;
                let surf = &iw3w.surfaces[i];
                let first = surf.first_vertex.max(0) as usize;
                let base = surf.base_index.max(0) as usize;
                let used: Vec<usize> = iw3w
                    .indices
                    .get(base..base + 3 * usize::from(surf.tri_count))
                    .unwrap_or(&[])
                    .iter()
                    .map(|&ix| first + usize::from(ix))
                    .filter(|&v| v < iw3w.vertices.len())
                    .collect();
                let (mut lo, mut hi) = ([f32::MAX; 3], [f32::MIN; 3]);
                for &vi in &used {
                    let v = &mut iw3w.vertices[vi];
                    if !std::mem::replace(&mut moved[vi], true) {
                        v.xyz = place_point(v.xyz, *origin, axis);
                        v.normal =
                            iw3_unit_vec(rotate(asset_model::unpack_unit_vec(v.normal), axis));
                        v.tangent =
                            iw3_unit_vec(rotate(asset_model::unpack_unit_vec(v.tangent), axis));
                    }
                    for k in 0..3 {
                        lo[k] = lo[k].min(v.xyz[k]);
                        hi[k] = hi[k].max(v.xyz[k]);
                    }
                }
                if lo[0] <= hi[0] {
                    iw3w.surfaces[i].bounds = [lo, hi];
                }
            }
        }
        for i in 0..n {
            if !listed[i] && !placed[i] {
                hidden[i] = true;
            }
        }
        iw3w.lit_surfs = 0..n as u32;
        report.push(format!(
            "h1 brush models: {} placed by their entities ({} surfaces)",
            placed_bmodels.len(),
            placed.iter().filter(|&&p| p).count()
        ));
    }
    if let Some(clip) = &collision {
        let floors = spawn_floors(clip, &iw3w, &hidden, &map.spawns);
        report.push(format!("h1 spawn floors: {floors}"));
        if floors.floor_missing * 4 > floors.spawns {
            diag::warn!(
                World,
                "bo2mp h1: {map_name}: spawn points over a drawn floor the collision lacks ({floors})"
            );
        }
    }
    let (pitch, yaw) = (
        iw3w.sun.angles[0].to_radians(),
        iw3w.sun.angles[1].to_radians(),
    );
    let dir = [
        pitch.cos() * yaw.cos(),
        pitch.cos() * yaw.sin(),
        -pitch.sin(),
    ];
    let colour = [
        iw3w.sun.sun_color[0] * iw3w.sun.sun_light,
        iw3w.sun.sun_color[1] * iw3w.sun.sun_light,
        iw3w.sun.sun_color[2] * iw3w.sun.sun_light,
        1.0,
    ];
    report.push(format!(
        "h1 sun: {} toward {dir:?} colour {colour:?}",
        if map.sun.is_some() {
            "from the ComWorld"
        } else {
            "none in the ComWorld (a default)"
        }
    ));
    // (Black Ops III's look: the sun's hue, its light the look's row.)
    let colour = if look.is_some() {
        let hue = iw3w.sun.sun_color;
        [hue[0], hue[1], hue[2], 1.0]
    } else {
        colour
    };
    let lighting = asset_world::T6WorldLighting {
        sun: asset_world::WorldSunLight {
            direction: dir,
            diffuse: colour,
            specular: colour,
        },
        exposure_scale: 1.0,
        // (Under the game's own shader the sky as its `sky_cso_hss` draws
        // it: the cube times its material's scale (Rust's 20) times a
        // global the code sets (not read; `BO2MP_H1_SKY_G`, set against the
        // game's pictures), in the bake's units.)
        sky: [0.0, 1.0, 0.0, match look {
            Some((scale, _)) if mwr_on() => {
                let m = sky_scale(cap).unwrap_or(1.0);
                let g = if sky_hdr { env_f("BO2MP_H1_SKY_G_HDR", 1.0) } else { env_f("BO2MP_H1_SKY_G", 0.034) };
                let k = m * g * scale;
                report.push(format!("h1 look: sky x{k:.2} (its material's scale {m}, the global x{g}{})", if sky_hdr { ", a half-float sky" } else { "" }));
                k.max(1e-4)
            }
            _ => 1.0,
        }],
        sky_parms: [1.0, 0.0],
        saturation: None,
        grade: None,
    };
    report.push(format!(
        "h1 world: {} surfaces ({} not drawn), {} sky surfaces, ranges {:?}",
        world.surfaces.len(),
        hidden.iter().filter(|&&h| h).count(),
        world.sky_surfaces.len(),
        world.ranges
    ));
    match asset_world::world_iw3::build_iw3_world_draw(
        &iw3w,
        &surface_materials,
        &hidden,
        lightmaps,
        Some(lighting),
    ) {
        Ok(mut draw) => {
            // The map's HDR fog first (`<map>_art.gsc` runs it when the
            // game draws in HDR, as Modern Warfare Remastered always does:
            // Rust's is a warm dust from 172 units; its plain `_fog.gsc`,
            // the fallback, starts at 17346), then the plain one.
            let (fog_name, fog) = [format!("maps/createart/{lower}_fog_hdr.gsc"), format!("maps/createart/{lower}_fog.gsc")]
                .into_iter()
                .find_map(|name| {
                    let f = raws.get(cap, &zone.image, &name).and_then(|b| ghosts_fog(&String::from_utf8_lossy(&b)))?;
                    Some((name, Some(f)))
                })
                .unwrap_or_else(|| (format!("maps/createart/{lower}_fog.gsc"), None));
            let fog = fog.map(|mut f| {
                f.opacity = (f.opacity * env_f("BO2MP_H1_FOG", 1.0)).clamp(0.0, 1.0);
                f
            });
            match &fog {
                Some(f) => {
                    report.push(format!(
                        "h1 fog: READY {fog_name} start={:.1} half={:.1} opacity={:.3} sun={}",
                        f.start,
                        f.half,
                        f.opacity,
                        f.sun.is_some()
                    ));
                    draw.t6_lights = super::cod4::legacy_fog_table(f);
                    // An HDR fog's colours are linear, in the bake's units
                    // (`HDRColorIntensity` times): under the look, times the
                    // bake's scale (Rust's dust, 4.5x its shade, the bright
                    // beige of the game's distance), not squared as a
                    // CoD4 fog's gamma colour.
                    if fog_name.ends_with("_hdr.gsc")
                        && let Some((scale, _)) = look
                    {
                        let text = raws.get(cap, &zone.image, &fog_name).map(|b| String::from_utf8_lossy(&b).into_owned()).unwrap_or_default();
                        let k = |key: &str| -> f32 {
                            text.lines()
                                .find_map(|l| {
                                    let (a, b) = l.trim().strip_prefix("ent.")?.split_once('=')?;
                                    (a.trim().eq_ignore_ascii_case(key)).then(|| b.trim().trim_end_matches(';').trim().parse::<f32>().ok()).flatten()
                                })
                                .unwrap_or(1.0)
                        };
                        let row = &mut draw.t6_lights[super::t6::FOG_SLOT];
                        let c = scale * k("HDRColorIntensity") * env_f("BO2MP_H1_FOG_LIGHT", 1.0);
                        for ch in 0..3 {
                            row[4 + ch] = f.colour[ch] * c;
                        }
                        if f.sun.is_some() {
                            let cs = scale * k("HDRSunColorIntensity") * env_f("BO2MP_H1_FOG_LIGHT", 1.0);
                            for ch in 0..3 {
                                row[8 + ch] = f.sun.as_ref().map_or(0.0, |s| s.color_rgb[ch]) * cs;
                            }
                        }
                        report.push(format!("h1 fog: HDR colour, linear, x{c:.2}"));
                    }
                }
                None => report.push(format!("h1 fog: none ({fog_name} sets none)")),
            }
            if let Some((_, sun_baked)) = look {
                let places: Vec<[f32; 3]> = map.spawns.iter().map(|s| s.origin).collect();
                super::bo3_look::apply(
                    &mut draw,
                    &materials,
                    &prop_meshes,
                    &prop_placements,
                    &places,
                    dir,
                    [colour[0], colour[1], colour[2]],
                    None,
                    &super::rt_ambient::Options::OFF,
                    &mut report,
                );
                if draw.t7_light {
                    // Reveal decals: each vertex's alpha packed with the
                    // soft edge for the shader's `t7_reveal` (the reveal
                    // map rides in the colour map's alpha).
                    let soft = env_f("BO2MP_H1_REVEAL_SOFT", 0.12);
                    let q = (soft * 64.0).round().clamp(1.0, 64.0);
                    let mut packed = vec![false; draw.colors.len()];
                    let mut revealed = 0usize;
                    for (s, &(start, count)) in draw.surface_index_ranges.iter().enumerate() {
                        if !draw.surface_materials.get(s).copied().flatten().is_some_and(|m| reveal_materials.contains(&m)) {
                            continue;
                        }
                        revealed += 1;
                        for &v in &draw.packed_indices[start as usize..(start + count) as usize] {
                            let v = v as usize;
                            if v < packed.len() && !std::mem::replace(&mut packed[v], true) {
                                let a = draw.colors[v][3].clamp(0.0, 1.0);
                                draw.colors[v][3] = -(2.0 + 2.0 * q + 0.999 * a);
                            }
                        }
                    }
                    report.push(format!("h1 look: {revealed} reveal decal surfaces ({} materials), soft edge {soft:.2}", reveal_materials.len()));
                    // The map's own reflection probes, in the baked light's
                    // scale: each surface its own (else the nearest kept),
                    // each placed model the nearest.
                    // (Under Modern Warfare Remastered's own shader the
                    // probes stay as stored, about one on average: it
                    // re-lights them by the baked light toward the
                    // reflection; else in the bake's scale.)
                    let bake_scale = look.map_or(1.0, |l| l.0);
                    let scale = if mwr_on() { 1.0 } else { bake_scale } * env_f("BO2MP_H1_PROBE", 1.0);
                    let kept = link_probes(cap, &zone.image, world, scale, &mut materials, asset_core::ZoneOwner::from_zone_path(path), &lower, &mut report);
                    let model_scale = if mwr_on() { bake_scale } else { 1.0 };
                    if !kept.is_empty() {
                        let nearest = |p: [f32; 3]| -> u8 {
                            kept.iter()
                                .enumerate()
                                .skip(usize::from(kept.len() > 1 && kept[0].1 == [0.0; 3]))
                                .min_by(|a, b| {
                                    let d = |o: [f32; 3]| (0..3).map(|k| (o[k] - p[k]).powi(2)).sum::<f32>();
                                    d(a.1.1).total_cmp(&d(b.1.1))
                                })
                                .map_or(0, |(slot, _)| slot as u8)
                        };
                        for (s, probe) in draw.surface_reflection_probes.iter_mut().enumerate() {
                            *probe = match kept.iter().position(|k| k.0 == usize::from(*probe)) {
                                Some(slot) if kept[slot].1 != [0.0; 3] || kept.len() == 1 => slot as u8,
                                _ => iw3w.surfaces.get(s).map_or(0, |sf| nearest(std::array::from_fn(|k| (sf.bounds[0][k] + sf.bounds[1][k]) * 0.5))),
                            };
                        }
                        // A placed model's light: its probe's all round
                        // (the bake holds no light for models; its own
                        // ground light, where it has one, read pink and
                        // too bright), less over a map whose sun is drawn.
                        let model_k = env_f("BO2MP_H1_MODEL", 1.0) * if sun_baked { 0.8 } else { 0.45 };
                        let pack = |v: f32| ((v.max(0.0) / 32.0).sqrt() * 255.0).round().clamp(1.0, 255.0) as u8;
                        // Under the game's own shader: a model's light is
                        // the light grid's where it stands (the 56 samples'
                        // mean, in the bake's units), as the game lights
                        // models; else its probe's.
                        let mut from_grid = 0usize;
                        let mut from_ground = 0usize;
                        let mut ground_sum = 0f32;
                        let probe_mean = kept.iter().map(|k| (k.2[0] + k.2[1] + k.2[2]) / 3.0).sum::<f32>() / kept.len().max(1) as f32;
                        let ground = (mwr_on() && env_f("BO2MP_H1_GROUND", 1.0) > 0.0 && world.light_grid.is_none())
                            .then(|| GroundLight::new(&iw3w, &lightmap_cpu, exposure));
                        for pl in prop_placements.iter_mut().flatten() {
                            let slot = nearest(pl.origin);
                            pl.reflection_probe_index = slot;
                            let grid = (mwr_on() && env_f("BO2MP_H1_GRID", 1.0) > 0.0)
                                .then(|| world.light_grid.as_ref()?.sample(&world.light_grid_colors, [pl.origin[0], pl.origin[1], pl.origin[2] + 16.0]))
                                .flatten();
                            let (a, k) = match grid {
                                Some(c) => {
                                    from_grid += 1;
                                    let mut m = [0f32; 3];
                                    for t in &c {
                                        for ch in 0..3 {
                                            m[ch] += t[ch] / 56.0;
                                        }
                                    }
                                    (m, bake_scale * env_f("BO2MP_H1_MODEL", 1.0))
                                }
                                None => match ground.as_ref().and_then(|g| g.at(pl.origin)) {
                                    // (Else, without a grid this lane reads,
                                    // the baked light of the ground under
                                    // it: A + B / 2, all round.)
                                    Some(c) => {
                                        from_ground += 1;
                                        ground_sum += (c[0] + c[1] + c[2]) / 3.0;
                                        (c, bake_scale * env_f("BO2MP_H1_MODEL", 1.0))
                                    }
                                    None => (kept[usize::from(slot)].2, model_k * model_scale),
                                },
                            };
                            pl.packed_lighting = Some([pack(a[0] * k), pack(a[1] * k), pack(a[2] * k), 0]);
                        }
                        // The first-person gun and arms: the scene reads no
                        // light grid here, so they take the nearest probe's
                        // light as a placed model without one does (else
                        // they get the sun alone and blow out).
                        if mwr_on() {
                            asset_world::set_h1_eye_lighting(
                                kept.iter()
                                    .map(|(_, o, c)| (*o, [pack(c[0] * model_k * model_scale), pack(c[1] * model_k * model_scale), pack(c[2] * model_k * model_scale), 0]))
                                    .collect(),
                            );
                        }
                        report.push(format!("h1 look: {from_grid} placed models lit from the light grid, {from_ground} from the baked light of the ground under them (mean {:.3}; the probes' {probe_mean:.3})", ground_sum / from_ground.max(1) as f32));
                        draw.t6_probes = kept.iter().map(|(_, o, _)| [[1.0, o[0], o[1], o[2]], [0.0, 0.0, 0.0, 1.0], [0.0; 4]]).collect();
                        asset_world::install_t6_probe_origins(kept.iter().map(|(_, o, _)| *o).collect());
                    } else if mwr_on() {
                        // A map with no reflection probe (a converted custom
                        // map often has none): the first-person gun and arms
                        // would take no ambient and show the sun alone, red
                        // or white. INFERRED: a neutral mid light, the middle
                        // of the Rust map's shaded mean (0.13) and its probes'
                        // mean (0.70), at the models' scale.
                        let k = env_f("BO2MP_H1_MODEL", 1.0) * if sun_baked { 0.8 } else { 0.45 } * model_scale;
                        let pk = |v: f32| ((v.max(0.0) / 32.0).sqrt() * 255.0).round().clamp(1.0, 255.0) as u8;
                        let v = pk(env_f("BO2MP_H1_EYE_FALLBACK", 0.4) * k);
                        asset_world::set_h1_eye_lighting(vec![([0.0; 3], [v, v, v, 0])]);
                        report.push("h1 look: no reflection probe; the gun and arms take a neutral ambient (BO2MP_H1_EYE_FALLBACK)".to_owned());
                    }
                    // The rows `bo3_look` wrote, as this map's: the tone
                    // curve's exposure, the sun's light over its colour
                    // (none over a bake that holds it), and the pages'
                    // packing (CoD4's, linear: `t7_iw_pages` in the shader).
                    let row = &mut draw.t6_lights[asset_world::T7_LIGHT_ROW];
                    row[0] = env_f("BO2MP_H1_EXPOSURE", LOOK_EXPOSURE).max(0.01);
                    // The sun: under the game's own shader its own light
                    // (the map's, in the bake's units: times the bake's
                    // scale, `BO2MP_H1_SUN` over it); none over a bake that
                    // holds it.
                    row[1] = if sun_baked {
                        0.0
                    } else if mwr_on() {
                        // (Its colour here is its hue; its strength the
                        // ComWorld's brightest channel: Rust's 1.5,
                        // Crash's 34.)
                        let peak = map.sun.map_or(1.0, |s| s.color.iter().copied().fold(0.0f32, f32::max).max(1e-3));
                        bake_scale * peak * env_f("BO2MP_H1_SUN", SUN_PI)
                    } else {
                        env_f("BO2MP_H1_SUN", LOOK_SUN)
                    };
                    draw.t6_lights[asset_world::T7_LIGHT_ROW + 1][9] = 1.0;
                    draw.t6_lights[asset_world::T7_LIGHT_ROW + 1][10] = if mwr_on() { 1.0 } else { 0.0 };
                    draw.t6_lights[asset_world::T7_LIGHT_ROW + 1][12] = if std::env::var("BO2MP_H1_HEIGHT_SHADOW").map_or(false, |v| v.trim() == "1") { 1.0 } else { 0.0 };
                    // (Not in a match, where the native renderer has its own meter: this
                    // old loop measured the empty converted scene and lifted the
                    // viewmodel to white.)
                    // Under the game's own shaders its automatic exposure
                    // (`r_tonemapAuto` 1): the scene's log-average
                    // brightness held at the key the real game's pictures
                    // show (`BO2MP_H1_AUTO_KEY`; `BO2MP_H1_AUTO=0` off).
                    let auto = mwr_on() && !native && std::env::var("BO2MP_H1_AUTO").map_or(true, |v| v.trim() != "0");
                    // (Kept in the light rows, so a packed copy of the
                    // map turns it on too: `session_load`.)
                    // (Each map's key fitted to HorizonMW's own picture of
                    // it, `look_keys.txt`: the game does not hold every map
                    // at one brightness; a map not in it takes AUTO_KEY.)
                    let fitted = map_look_key(&lower);
                    let key = env_f("BO2MP_H1_AUTO_KEY", fitted.unwrap_or(AUTO_KEY));
                    draw.t6_lights[asset_world::T7_LIGHT_ROW + 1][11] = if auto { key } else { 0.0 };
                    if auto {
                        report.push(format!(
                            "h1 look: automatic exposure, key {key:.3} ({})",
                            if fitted.is_some() { "fitted to HorizonMW's picture" } else { "the default" }
                        ));
                        // The game's own key (docs/H1_NATIVE.md): 0.18 while the
                        // light set locks the adjust (its default), with no
                        // lightset read here; the fitted key stays in force.
                        report.push(format!(
                            "h1 vision: {lower}: BO3 look; auto key {key:.3} against the game's 0.180 (0.18 x 2^autoexposureadjust, locked); the game's tone map is not drawn on this path (docs/H1_NATIVE.md)"
                        ));
                    }
                    // The sun's haze and shafts (`bo3_look`'s fog rows), as
                    // the map's vision sets them: on with
                    // `r_volumeLightScatter`, in its colour
                    // (`r_volumeLightScatterColor`: Rust's warm dust), at
                    // the strength the look gives the sun's; off without
                    // it, and over a bake that holds the sun (no sun drawn:
                    // it was a milky veil).
                    let scatter = raws.get(cap, &zone.image, &vision_name)
                        .map(|b| String::from_utf8_lossy(&b).into_owned())
                        .and_then(|t| volume_scatter(&t));
                    let fog_row = &mut draw.t6_lights[super::t7_light::T7_FOG_ROW];
                    let sun_haze = [fog_row[4], fog_row[5], fog_row[6]];
                    let strength = sun_haze[0] * 0.2126 + sun_haze[1] * 0.7152 + sun_haze[2] * 0.0722;
                    let haze = match scatter {
                        Some(c) if !sun_baked => {
                            let l = (c[0] * 0.2126 + c[1] * 0.7152 + c[2] * 0.0722).max(1e-4);
                            let k = env_f("BO2MP_H1_HAZE", 1.0);
                            for ch in 0..3 {
                                fog_row[4 + ch] = c[ch] / l * strength * k;
                            }
                            format!("{c:.2?} x{k}")
                        }
                        _ => {
                            fog_row[4..7].fill(0.0);
                            "off".to_owned()
                        }
                    };
                    report.push(format!("h1 look: sun haze and shafts {haze} (the vision's volume light scatter)"));
                    // The map's atmospheric fog (`atmosFog*` in its HDR fog
                    // script; the game's `postopaquefx_fog_hf`: transmittance
                    // by distance past the start, halving every half-plane
                    // distance, and in-scattered haze, Rayleigh plus a Mie
                    // glow toward the sun) into this renderer's fog of the
                    // same form (`t7_fog_core`), its light in the bake's
                    // units; it replaces the plain fog (Rust's is thick from
                    // 172 units; the game's view is clear to mid range).
                    if mwr_on()
                        && let Some(a) = raws.get(cap, &zone.image, &format!("maps/createart/{lower}_fog_hdr.gsc"))
                            .map(|b| String::from_utf8_lossy(&b).into_owned())
                            .and_then(|t| atmos_fog(&t))
                    {
                        let light = bake_scale * env_f("BO2MP_H1_ATMOS", 1.0);
                        // (The in-scatter's Rayleigh phase averaged over the
                        // view, 0.0597 (1 + mu^2): 0.0796.)
                        let ray = a.inscatter * 0.0796 * a.haze_strength * light;
                        let mie = a.inscatter * a.haze_strength * light;
                        // (The shader's transmittance is 2^(length * row 2):
                        // halving every half-plane distance.)
                        let d = 1.0 / a.half.max(1.0);
                        // (Toward the sun: as `bo3_look` wrote it.)
                        // (The fog's own sun when it names one: Rust's
                        // glow stands apart from the light's sun.)
                        let toward = a.sun_dir.unwrap_or_else(|| {
                            let r = &draw.t6_lights[super::t7_light::T7_FOG_ROW];
                            [r[12], r[13], r[14]]
                        });
                        let height = if a.height_on { 1.0 / a.height_half.max(1.0) } else { 0.0 };
                        let rows: [[f32; 4]; 8] = [
                            [a.haze[0] * ray, a.haze[1] * ray, a.haze[2] * ray, a.start],
                            [a.sun_fog[0] * mie, a.sun_fog[1] * mie, a.sun_fog[2] * mie, 1.0 / 2000.0],
                            [-d, -d, -d, a.sky_distance.clamp(2000.0, 200000.0)],
                            [toward[0], toward[1], toward[2], 0.6],
                            [a.extinction, 1.0, 0.0, 1.0],
                            // (The falloff's angles at a fifth: the real
                            // game's Rust shows its clouds down to a few
                            // degrees up, where the script's 8 + 40 would
                            // still fog them over; the shader's own use of
                            // them is not read yet.)
                            [a.start, a.height_base, height, env_f("BO2MP_H1_SKY_FALL_START", a.sky_fall_start * SKY_FALL_SCALE)],
                            [a.start, a.height_base, height, env_f("BO2MP_H1_SKY_FALL_RANGE", a.sky_fall_range * SKY_FALL_SCALE)],
                            {
                                let r = &draw.t6_lights[super::t7_light::T7_FOG_ROW + 1];
                                [r[12], r[13], r[14], r[15]]
                            },
                        ];
                        for (i, r) in rows.iter().enumerate() {
                            let row = super::t7_light::T7_FOG_ROW + i / 4;
                            draw.t6_lights[row][4 * (i % 4)..4 * (i % 4) + 4].copy_from_slice(r);
                        }
                        // (The plain fog off: `fog_apply`'s on flag.)
                        draw.t6_lights[super::t6::FOG_SLOT][3] = 0.0;
                        report.push(format!(
                            "h1 look: atmospheric fog from {lower}_fog_hdr.gsc: start {:.0}, half {:.0}, haze {:.3?}, sun fog {:.3?}, strength {:.2}, in-scatter {:.1}, extinction {:.2}, sky at {:.0} (the plain fog off)",
                            a.start, a.half, a.haze, a.sun_fog, a.haze_strength, a.inscatter, a.extinction, a.sky_distance
                        ));
                    }
                    // The map's own grade, from its vision's HDR film
                    // (Rust's: warm tints over shadows, midtones and
                    // highlights, a little more saturation), with a light
                    // bloom (as Nuk3town's look sets its own): ungraded,
                    // Black Ops II's intro vision's bloom veiled the
                    // bright maps.
                    let mut vision = asset_world::T6Vision::default();
                    let vision_text = raws.get(cap, &zone.image, &vision_name).map(|b| String::from_utf8_lossy(&b).into_owned());
let film_lut = vision_text.as_deref().and_then(film_of).filter(|f| f.enable && env_f("BO2MP_H1_FILM", 1.0) > 0.0).map(|mut f| {
                        // (Test knobs while the film is fitted to the real
                        // game: `BO2MP_H1_FILM_BRIGHT`, `_FILM_CONTRAST`,
                        // `_FILM_TINT` scale those terms.)
                        f.brightness *= env_f("BO2MP_H1_FILM_BRIGHT", 1.0);
                        f.contrast = 1.0 + (f.contrast - 1.0) * env_f("BO2MP_H1_FILM_CONTRAST", 1.0);
                        let t = env_f("BO2MP_H1_FILM_TINT", 1.0);
                        for c in [&mut f.dark, &mut f.medium, &mut f.light] {
                            for k in c.iter_mut() {
                                *k = 1.0 + (*k - 1.0) * t;
                            }
                        }
                        report.push(format!(
                            "h1 look: film from {vision_name} (the game's postfx_color2): dark {:.2?} medium {:.2?} light {:.2?}, desaturation {:.2}/{:.2} dark, contrast {:.2}, brightness {:.2}",
                            f.dark, f.medium, f.light, f.desaturation, f.desaturation_dark, f.contrast, f.brightness
                        ));
                        film_lut(&f)
                    });
                    if film_lut.is_none() {
                        report.push(format!("h1 look: no film in {vision_name}: neutral grade"));
                    }
                    let bloom = env_f("BO2MP_H1_BLOOM", 0.35);
                    vision.bloom_rgb_hi = [0.35 * bloom, 0.35 * bloom, 0.35 * bloom, 0.7];
                    vision.bloom_rgb_lo = [0.45 * bloom, 0.45 * bloom, 0.45 * bloom, 3.0];
                    // (No film: the same grade with every term neutral, the picture
                    // as lit; the default vision's own LUT drew black.)
                    let lut = film_lut.unwrap_or_else(|| {
                        self::film_lut(&Film {
                            enable: true,
                            contrast: 1.0,
                            brightness: 0.0,
                            desaturation: 0.0,
                            desaturation_dark: 0.0,
                            dark: [1.0; 3],
                            medium: [1.0; 3],
                            light: [1.0; 3],
                        })
                    });
                    super::bo2_look::set_pending_grade(Some(asset_world::T6Grade { lut, vision }));
                    report.push(format!(
                        "h1 look: on (BO2MP_BO3_LOOK_H1=0 turns it off): exposure {:.2}, sun x{:.2}",
                        draw.t6_lights[asset_world::T7_LIGHT_ROW][0],
                        draw.t6_lights[asset_world::T7_LIGHT_ROW][1]
                    ));
                }
            }
            report.push(format!(
                "world mesh: {} vertices, {} triangles, {} surfaces (Modern Warfare Remastered, fallback draw with colour maps{})",
                draw.stats.vertices,
                draw.stats.triangles,
                draw.stats.surfaces,
                if draw.iw3_lightmaps { " and its lightmaps" } else { "" }
            ));
            let (min, max) = (draw.stats.min, draw.stats.max);
            materials.set_capture_ns(AssetNamespace::T6);
            LoadedWorld {
                scripts,
                world: PreparedWorld {
                    draw: Some(draw),
                    min,
                    max,
                    film_vision,
                    policy: WorldDrawPolicy::t5(),
                    static_model_meshes: prop_meshes,
                    static_model_instances: prop_placements,
                    intermission_view,
                    ..Default::default()
                },
                materials,
                collision,
                spawns,
                facts,
                report,
                ..Default::default()
            }
        }
        Err(error) => {
            let mut loaded = LoadedWorld::with_gap(
                WorldDrawPolicy::t5(),
                PreparedCapability::PreparedWorld,
                format!("Modern Warfare Remastered world mesh: {error}"),
                Some("assets::lane::h1_map::load_world/world_mesh"),
            );
            loaded.scripts = scripts;
            loaded.collision = collision;
            loaded.spawns = spawns;
            loaded.materials = materials;
            loaded.facts = facts;
            loaded.report.extend(report);
            loaded
        }
    }
}

#[cfg(test)]
mod maps {
    //! Opt-in checks on real zones: `H1_MAPS=a.ff:b.ff cargo test -p assets
    //! --release h1_maps_load -- --ignored --nocapture`. Each map is loaded
    //! through the lane and checked: a drawable world with no non-finite
    //! vertex, every drawn surface with a colour map that has pixels, the
    //! lightmaps converted, placed models placed, and every spawn point
    //! inside the collision's bounds with a floor under it.

    #[test]
    #[ignore]
    fn h1_maps_load() {
        let Some(paths) = std::env::var_os("H1_MAPS") else {
            return;
        };
        for path in std::env::split_paths(&paths) {
            let started = std::time::Instant::now();
            let progress = asset_transport::progress::LoadProgress::new(0);
            let mut colour = std::collections::HashMap::new();
            let loaded =
                super::load_h1_world_audited(&path, &progress, Default::default(), &mut colour, false);
            println!(
                "== {} [{:.1}s]",
                path.display(),
                started.elapsed().as_secs_f32()
            );
            for line in &loaded.report {
                println!("  {line}");
            }
            assert!(
                loaded.gaps.is_empty(),
                "gaps: {:?}",
                loaded.gaps.iter().map(|g| &g.reason).collect::<Vec<_>>()
            );
            let draw = loaded.world.draw.as_ref().expect("a drawable world");
            let bad = draw
                .positions
                .iter()
                .flatten()
                .filter(|v| !v.is_finite())
                .count();
            assert_eq!(bad, 0, "non-finite vertex coordinates");
            let size: Vec<f32> = (0..3)
                .map(|k| draw.stats.max[k] - draw.stats.min[k])
                .collect();
            assert!(
                size.iter().all(|s| *s > 256.0 && *s < 1.0e6),
                "world size {size:?}"
            );
            // Every drawn surface's material has a colour map: with pixels,
            // or bound to a picture whose retail `imagefile<N>.pak` is not
            // installed (those draw grey here). A material or colour map in
            // no zone, or pixels that do not decode, is a bug.
            let cat = &loaded.materials;
            let (mut drawn, mut decoded) = (0usize, 0usize);
            let (mut absent, mut missing) = (Vec::new(), Vec::new());
            for (i, &(_, count)) in draw.surface_index_ranges.iter().enumerate() {
                if count == 0 {
                    continue;
                }
                drawn += 1;
                let Some(m) = draw.surface_materials[i] else {
                    missing.push(format!("surface {i}: no material"));
                    continue;
                };
                let name = cat
                    .materials
                    .get(m)
                    .map_or_else(|| format!("material {m}"), |m| m.name.as_str().to_owned());
                let has_pixels = cat
                    .materials
                    .get(m)
                    .and_then(|m| {
                        m.textures
                            .iter()
                            .find(|t| t.semantic == asset_material::TS_COLOR_MAP)
                    })
                    .and_then(|t| t.image)
                    .and_then(|i| cat.images.get(i))
                    .is_some_and(|img| img.decoded.is_some());
                match colour.get(&m) {
                    _ if has_pixels => decoded += 1,
                    Some(super::ColourMap::PakAbsent(pak)) => absent.push((name, pak.clone())),
                    Some(super::ColourMap::Missing(why)) => missing.push(format!("{name}: {why}")),
                    Some(super::ColourMap::Decoded) | None => {
                        missing.push(format!("{name}: no colour map bound"))
                    }
                }
            }
            missing.sort();
            missing.dedup();
            absent.sort();
            absent.dedup();
            let paks: std::collections::BTreeSet<&str> =
                absent.iter().map(|(_, p)| p.as_str()).collect();
            println!(
                "  check: {drawn} drawn surfaces: {decoded} with pixels, {} materials whose colour map is in an absent pak {paks:?}, missing {missing:?}",
                absent.len()
            );
            assert!(missing.is_empty(), "untextured: {missing:?}");
            let pages = draw
                .lightmap
                .as_ref()
                .map_or(0, |p| p.iter().flatten().count());
            assert!(pages > 0, "no lightmap page converted");
            let placed: Vec<_> = loaded
                .world
                .static_model_instances
                .iter()
                .flatten()
                .collect();
            assert!(!placed.is_empty(), "no placed models");
            assert!(placed.iter().all(|p| p.origin.iter().all(|v| v.is_finite())
                && p.scale.is_finite()
                && p.mesh < loaded.world.static_model_meshes.len()));
            // Spawns: inside the collision's bounds, a floor under each.
            let clip = loaded.collision.as_ref().expect("collision");
            let world = clip.cmodels.first().expect("the world model");
            assert!(!loaded.spawns.is_empty());
            let (mut outside, mut no_floor) = (Vec::new(), Vec::new());
            for s in &loaded.spawns {
                let o = s.origin;
                if !(0..3).all(|k| o[k] >= world.mins[k] && o[k] <= world.maxs[k]) {
                    outside.push((s.classname.clone(), o));
                }
                let (down, _) = asset_world::world_iw3::trace_clip_collision(
                    clip,
                    [o[0], o[1], o[2] + 16.0],
                    [o[0], o[1], o[2] - 256.0],
                    [0.0; 3],
                    [0.0; 3],
                    asset_world::MASK_PLAYER_SOLID,
                );
                if down >= 1.0 {
                    no_floor.push((s.classname.clone(), o));
                }
            }
            println!(
                "  check: {} spawns, {} outside the collision bounds, {} with no floor within 256 below; collision {} brushes {} triangles",
                loaded.spawns.len(),
                outside.len(),
                no_floor.len(),
                clip.brushes.len(),
                clip.mesh.tri_count()
            );
            assert!(outside.is_empty(), "{outside:?}");
            assert!(no_floor.is_empty(), "{no_floor:?}");
            // (A spawn point placed in the air is the map's own; one over a
            // drawn floor the collision lacks is a bug.)
            let floors = loaded
                .report
                .iter()
                .find(|l| l.starts_with("h1 spawn floors"))
                .expect("the spawn floors line");
            assert!(
                floors.contains(" 0 over a drawn floor the collision lacks"),
                "{floors}"
            );
        }
    }

    /// A brush as the trace sees it (its six bounds planes, then its
    /// sides): the corners of the solid they enclose. Empty when the
    /// planes enclose nothing.
    pub(super) fn brush_corners(planes: &[[f32; 4]]) -> Vec<[f32; 3]> {
        let n = planes.len();
        let mut out = Vec::new();
        for i in 0..n {
            for j in i + 1..n {
                for k in j + 1..n {
                    let (a, b, c) = (planes[i], planes[j], planes[k]);
                    let cross = |u: [f32; 4], v: [f32; 4]| -> [f64; 3] {
                        let (u, v) = (u.map(f64::from), v.map(f64::from));
                        [u[1] * v[2] - u[2] * v[1], u[2] * v[0] - u[0] * v[2], u[0] * v[1] - u[1] * v[0]]
                    };
                    let bc = cross(b, c);
                    let det = f64::from(a[0]) * bc[0] + f64::from(a[1]) * bc[1] + f64::from(a[2]) * bc[2];
                    if det.abs() < 1e-9 {
                        continue;
                    }
                    let (ca, ab) = (cross(c, a), cross(a, b));
                    let p: [f32; 3] = std::array::from_fn(|q| {
                        ((f64::from(a[3]) * bc[q] + f64::from(b[3]) * ca[q] + f64::from(c[3]) * ab[q]) / det) as f32
                    });
                    if planes.iter().all(|pl| pl[0] * p[0] + pl[1] * p[1] + pl[2] * p[2] - pl[3] <= 0.05) {
                        out.push(p);
                    }
                }
            }
        }
        out
    }

    /// bo2mp: every brush checked, and the headless drop test over MWR maps
    /// (`H1_TEST_MAP=<zone.ff>[,...] cargo test -p assets --release --lib
    /// drop_test_h1 -- --ignored --nocapture`). Each map goes through the
    /// lane (`load_h1_world_audited`). Brushes: every clip plane a unit
    /// normal, and every brush's planes enclose a solid that reaches its
    /// bounds (within 0.25 on every face; a misread side or plane cuts it
    /// short or empties it). Then `t7_map`'s drop test on the collision: the
    /// player box dropped at every spawn and over a grid (`T7_GRID`, 96) of
    /// the drawn world's floors where a player can get, the match's pmove
    /// run from `T7_PMOVE_DROPS` (600) of them, and every world player clip
    /// walked into. Drawn floors are the world's surfaces the lane draws
    /// (not placed models: H1's player box does not meet them). `T7_STRICT=1`
    /// fails on any failure.
    #[test]
    #[ignore]
    fn drop_test_h1() {
        let Ok(paths) = std::env::var("H1_TEST_MAP") else {
            return;
        };
        let env = |k: &str, d: usize| {
            codlegacy_env::var(k)
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(d)
        };
        let mut seed = 0x9e37_79b9_7f4a_7c15u64;
        let mut rand = move || {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            (seed >> 11) as f32 / (1u64 << 53) as f32
        };
        let (mut summary, mut total) = (Vec::new(), 0usize);
        for path in paths.split(',') {
            let path = std::path::Path::new(path);
            let map_name = path
                .file_stem()
                .map_or_else(String::new, |s| s.to_string_lossy().into_owned());
            let started = std::time::Instant::now();
            let progress = asset_transport::progress::LoadProgress::new(0);
            let mut colour = std::collections::HashMap::new();
            let loaded =
                super::load_h1_world_audited(path, &progress, Default::default(), &mut colour, false);
            println!("== {map_name} [{:.1}s]", started.elapsed().as_secs_f32());
            for line in loaded.report.iter().filter(|l| {
                l.starts_with("h1 clip") || l.starts_with("h1 model") || l.starts_with("h1 spawn")
            }) {
                println!("  {line}");
            }
            let Some(full) = loaded.collision.as_ref() else {
                summary.push(format!("{map_name}: no collision"));
                total += 1;
                continue;
            };
            // Every brush: unit planes, and a solid reaching its bounds.
            let mut bad_planes = 0usize;
            let (mut empty, mut short) = (Vec::new(), Vec::new());
            let mut centres = Vec::with_capacity(full.brushes.len());
            for (bi, b) in full.brushes.iter().enumerate() {
                for p in &b.planes {
                    let len = (p[0] * p[0] + p[1] * p[1] + p[2] * p[2]).sqrt();
                    if (len - 1.0).abs() > 1e-3 || !p[3].is_finite() {
                        bad_planes += 1;
                    }
                }
                if b.planes.len() < 6 {
                    empty.push(bi);
                    centres.push(None);
                    continue;
                }
                let corners = brush_corners(&b.planes);
                if corners.is_empty() {
                    empty.push(bi);
                    centres.push(None);
                    continue;
                }
                let g = |i: usize| b.planes[i][3];
                let (blo, bhi) = ([-g(1), -g(3), -g(5)], [g(0), g(2), g(4)]);
                let lo: [f32; 3] =
                    std::array::from_fn(|k| corners.iter().map(|c| c[k]).fold(f32::MAX, f32::min));
                let hi: [f32; 3] =
                    std::array::from_fn(|k| corners.iter().map(|c| c[k]).fold(f32::MIN, f32::max));
                let gap = (0..3)
                    .map(|k| (lo[k] - blo[k]).max(bhi[k] - hi[k]))
                    .fold(0.0f32, f32::max);
                if gap > 0.25 {
                    short.push((bi, gap, b.contents));
                }
                let n = corners.len() as f32;
                centres.push(Some(std::array::from_fn(|k| {
                    corners.iter().map(|c| c[k]).sum::<f32>() / n
                })));
            }
            short.sort_by(|a, b| b.1.total_cmp(&a.1));
            let brush_line = format!(
                "{map_name}: {} brushes checked: {bad_planes} planes not unit, {} enclose nothing, {} short of their bounds by over 0.25{}",
                full.brushes.len(),
                empty.len(),
                short.len(),
                if short.is_empty() {
                    String::new()
                } else {
                    format!(
                        " (worst {:?})",
                        short
                            .iter()
                            .take(5)
                            .map(|&(i, g, c)| format!("#{i} {g:.2} contents {c:#x}"))
                            .collect::<Vec<_>>()
                    )
                }
            );
            println!("{brush_line}");
            total += bad_planes + empty.len() + short.len();
            summary.push(brush_line);
            // The drawn world's floors (what the lane draws: hidden
            // surfaces have no material).
            let mut floors = super::super::t7_map::tests::DrawnFloors::empty();
            floors.names.push(format!("world of {map_name}"));
            if let Some(draw) = loaded.world.draw.as_ref() {
                for (i, &(start, count)) in draw.surface_index_ranges.iter().enumerate() {
                    if draw.surface_materials.get(i).copied().flatten().is_none() {
                        continue;
                    }
                    let (start, end) = (start as usize, start as usize + count as usize);
                    for t in draw
                        .packed_indices
                        .get(start..end.min(draw.packed_indices.len()))
                        .unwrap_or(&[])
                        .chunks_exact(3)
                    {
                        let v = |k: usize| draw.positions.get(t[k] as usize).copied();
                        if let (Some(a), Some(b), Some(c)) = (v(0), v(1), v(2)) {
                            floors.add([a, b, c], 0);
                        }
                    }
                }
            }
            // (H1 places many spawn records up to 96 over their floor, and
            // the game drops the player to it: the floor is where it starts.)
            let spawns: Vec<[f32; 3]> = loaded
                .spawns
                .iter()
                .map(|s| {
                    let o = s.origin;
                    let z = floors
                        .at(o[0], o[1], o[2] - 128.0, o[2] + 2.0)
                        .first()
                        .map_or(o[2], |f| f.0);
                    [o[0], o[1], z]
                })
                .collect();
            if spawns.is_empty() {
                summary.push(format!("{map_name}: no spawns"));
                continue;
            }
            let (line, failures) = super::super::t7_map::tests::drop_test_world(
                &map_name,
                full,
                &spawns,
                &floors,
                &centres,
                env("T7_GRID", 96) as f32,
                env("T7_PMOVE_DROPS", 600),
                env("T7_REACH_NODES", 60000),
                &mut rand,
            );
            total += failures;
            summary.push(line);
        }
        println!("== drop test summary");
        for l in &summary {
            println!("{l}");
        }
        if std::env::var("T7_STRICT").is_ok() {
            assert_eq!(total, 0, "drop test failures");
        }
    }

    /// The brush check's solver: a box's corners are its corners; a side
    /// cutting it off leaves it short of its bounds.
    #[test]
    fn brush_corners_of_a_box() {
        let b = [
            [1.0, 0.0, 0.0, 10.0],
            [-1.0, 0.0, 0.0, 0.0],
            [0.0, 1.0, 0.0, 10.0],
            [0.0, -1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 10.0],
            [0.0, 0.0, -1.0, 0.0],
        ];
        assert_eq!(brush_corners(&b).len(), 8);
        let mut cut = b.to_vec();
        let s = std::f32::consts::FRAC_1_SQRT_2;
        cut.push([s, s, 0.0, 10.0 * s]);
        let c = brush_corners(&cut);
        assert!(!c.is_empty());
        let hi_x = c.iter().map(|p| p[0]).fold(f32::MIN, f32::max);
        assert!((hi_x - 10.0).abs() < 1e-3, "{hi_x}");
        cut.push([0.0, 0.0, 1.0, -1.0]);
        assert!(brush_corners(&cut).is_empty());
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn matrices_invert() {
        let m = [[2.0, 0.0, 0.0], [0.0, 0.0, 3.0], [0.0, -1.0, 0.0]];
        let inv = super::invert3(m).expect("invertible");
        for (r, row) in m.iter().enumerate() {
            for c in 0..3 {
                let v: f32 = row.iter().zip(&inv).map(|(a, col)| a * col[c]).sum();
                assert!(
                    (v - f32::from(u8::from(r == c))).abs() < 1e-5,
                    "{r} {c} {v}"
                );
            }
        }
        assert!(super::invert3([[1.0, 2.0, 3.0], [2.0, 4.0, 6.0], [0.0, 0.0, 1.0]]).is_none());
    }

    /// A placed model's triangle as the model trace takes it: a ray onto
    /// its front (clockwise seen from there) hits at its plane through the
    /// placement's inverse scaled axes; from behind or beside, nothing.
    #[test]
    fn placed_model_triangle_is_traced_from_its_front() {
        let t = super::coll_tri([0.0, 0.0, 0.0], [0.0, 10.0, 0.0], [10.0, 0.0, 0.0]).unwrap();
        assert!((t.plane[2] - 1.0).abs() < 1e-6 && t.plane[3].abs() < 1e-6);
        assert!(super::coll_tri([0.0; 3], [1.0, 1.0, 1.0], [2.0, 2.0, 2.0]).is_none());
        // Rotated 90 degrees about z and scaled 2, at (100, 0, 0).
        let axes = [[0.0, 2.0, 0.0], [-2.0, 0.0, 0.0], [0.0, 0.0, 2.0]];
        let origin = [100.0, 0.0, 0.0];
        let sm = clipmap_iw4::ClipStaticModel {
            origin,
            inv_scaled_axis: super::invert3(axes).unwrap(),
            bounds_mid: [90.0, 10.0, 0.0],
            bounds_half: [21.0, 21.0, 1.0],
            coll: clipmap_iw4::XModelColl {
                trace_offset: super::H1_MODEL_TRACE_OFFSET,
                coll_lod: 0,
                contents: 1,
                surfs: vec![clipmap_iw4::XModelCollSurf {
                    tris: vec![t],
                    midpoint: [5.0, 5.0, 0.0],
                    half_size: [5.0, 5.0, 0.5],
                    bone_idx: 0,
                    contents: 1,
                    surf_flags: 0,
                }]
                .into(),
            },
        };
        // The triangle's middle, placed.
        let l = [10.0 / 3.0, 10.0 / 3.0, 0.0];
        let p: [f32; 3] =
            std::array::from_fn(|k| origin[k] + l[0] * axes[0][k] + l[1] * axes[1][k] + l[2] * axes[2][k]);
        let shoot = |from: f32, to: f32, x: f32| {
            let mut tr = trace_iw4::Trace {
                fraction: 1.0,
                ..Default::default()
            };
            let hit = clipmap_iw4::point_trace_static_models(
                [&sm],
                [x, p[1], from],
                [x, p[1], to],
                1,
                &mut tr,
            );
            (hit.is_some(), tr.fraction, tr.normal)
        };
        let (hit, frac, normal) = shoot(50.0, -50.0, p[0]);
        assert!(hit && (frac - 0.5).abs() < 0.01, "{frac}");
        assert!(normal[2] > 0.99, "{normal:?}");
        assert!(!shoot(-50.0, 50.0, p[0]).0, "from behind");
        assert!(!shoot(50.0, -50.0, p[0] + 30.0).0, "beside");
    }

    #[test]
    fn hidden_surfaces_by_technique() {
        assert_eq!(super::hidden_kind("wc_sky_cso_nfwpf"), Some("sky"));
        assert_eq!(super::hidden_kind("wc_tools_r0c0ct0"), Some("tools"));
        assert_eq!(super::hidden_kind("wc_shadowcaster"), Some("shadow only"));
        assert_eq!(
            super::hidden_kind("wc_unlit_distfalloff_replace_ndw_cltrans"),
            Some("exposure portal")
        );
        assert_eq!(super::hidden_kind("wc_l_sm_r0c0n0sd0_nfwpf"), None);
    }

}

/// A vision file's volume light scatter colour, when it is on.
fn volume_scatter(text: &str) -> Option<[f32; 3]> {
    let get = |key: &str| -> Option<Vec<f32>> {
        text.lines().find_map(|l| {
            let (n, rest) = l.trim().split_once(char::is_whitespace)?;
            n.eq_ignore_ascii_case(key).then(|| rest.trim().trim_matches('"').split_whitespace().filter_map(|x| x.parse().ok()).collect())
        })
    };
    if get("r_volumeLightScatter")?.first().copied().unwrap_or(0.0) < 0.5 {
        return None;
    }
    let c = get("r_volumeLightScatterColor")?;
    (c.len() == 3).then(|| [c[0], c[1], c[2]])
}

/// The sky material's scale (its first constant's x: `sky_cso_hss`'s
/// `texel * c.x + c.y`).
fn sky_scale(cap: &Capture) -> Option<f32> {
    cap.materials
        .iter()
        .filter(|m| m.techset.is_some_and(|t| cap.name_of(t).contains("_sky")))
        .find_map(|m| m.constants.first().map(|c| c.literal[0]))
        .filter(|v| v.is_finite() && *v > 0.0)
}

/// A fog script's atmospheric fog (`ent.atmosFog*`), when it is on.
struct AtmosFog {
    start: f32,
    half: f32,
    haze: [f32; 3],
    sun_fog: [f32; 3],
    haze_strength: f32,
    inscatter: f32,
    extinction: f32,
    sky_distance: f32,
    height_on: bool,
    height_base: f32,
    height_half: f32,
    /// The sky's fog fading out above the horizon: from this many degrees
    /// up, over `sky_fall_range` more (`atmosFogSkyAngularFalloff*`; range
    /// 0: none).
    sky_fall_start: f32,
    sky_fall_range: f32,
    /// The fog's own sun (`atmosFogSunDirection`, toward it): where its
    /// glow sits.
    sun_dir: Option<[f32; 3]>,
}

fn atmos_fog(script: &str) -> Option<AtmosFog> {
    let mut num: std::collections::HashMap<String, f32> = std::collections::HashMap::new();
    let mut vec3: std::collections::HashMap<String, [f32; 3]> = std::collections::HashMap::new();
    for line in script.lines() {
        let Some((key, value)) = line.trim().strip_prefix("ent.").and_then(|l| l.split_once('=')) else { continue };
        let key = key.trim().to_ascii_lowercase();
        let value = value.trim().trim_end_matches(';').trim();
        if let Some(inner) = value.strip_prefix('(').and_then(|v| v.strip_suffix(')')) {
            let v: Vec<f32> = inner.split(',').filter_map(|x| x.trim().parse().ok()).collect();
            if v.len() == 3 {
                vec3.insert(key, [v[0], v[1], v[2]]);
            }
        } else if let Ok(v) = value.parse::<f32>() {
            num.insert(key, v);
        }
    }
    let n = |k: &str, d: f32| num.get(k).copied().unwrap_or(d);
    if n("atmosfogenabled", 0.0) < 0.5 {
        return None;
    }
    let scale = n("atmosfogdistancescale", 1.0).max(1e-3);
    Some(AtmosFog {
        start: n("atmosfogstartdistance", 0.0).max(0.0),
        half: n("atmosfoghalfplanedistance", 8000.0) / scale,
        haze: vec3.get("atmosfoghazecolor").copied().unwrap_or([0.5; 3]),
        sun_fog: vec3.get("atmosfogsunfogcolor").copied().unwrap_or([0.5; 3]),
        haze_strength: n("atmosfoghazestrength", 0.5).max(0.0),
        inscatter: n("atmosfoginscatterstrength", 1.0).max(0.0),
        extinction: n("atmosfogextinctionstrength", 1.0).clamp(0.0, 2.0),
        sky_distance: n("atmosfogskydistance", 30000.0),
        height_on: n("atmosfogheightfogenabled", 0.0) > 0.5,
        height_base: n("atmosfogheightfogbaseheight", 0.0),
        height_half: n("atmosfogheightfoghalfplanedistance", 1000.0),
        sky_fall_start: n("atmosfogskyfalloffstartangle", 0.0),
        sky_fall_range: if n("atmosfogskyangularfalloffenabled", 0.0) > 0.5 { n("atmosfogskyfalloffanglerange", 0.0).max(0.0) } else { 0.0 },
        sun_dir: vec3.get("atmosfogsundirection").copied().filter(|d| d.iter().any(|c| *c != 0.0)).map(|d| {
            let l = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt().max(1e-6);
            [d[0] / l, d[1] / l, d[2] / l]
        }),
    })
}

/// A vision's film (`r_film*`): Modern Warfare Remastered's grade
/// (`postfx_color2`).
struct Film {
    enable: bool,
    contrast: f32,
    brightness: f32,
    desaturation: f32,
    desaturation_dark: f32,
    dark: [f32; 3],
    medium: [f32; 3],
    light: [f32; 3],
}

fn film_of(text: &str) -> Option<Film> {
    let mut v: std::collections::HashMap<String, Vec<f32>> = std::collections::HashMap::new();
    for line in text.lines() {
        let Some((name, rest)) = line.trim().split_once(char::is_whitespace) else { continue };
        let nums: Vec<f32> = rest.trim().trim_matches('"').split_whitespace().filter_map(|x| x.parse().ok()).collect();
        v.insert(name.to_ascii_lowercase(), nums);
    }
    let one = |k: &str, d: f32| v.get(k).and_then(|x| x.first()).copied().unwrap_or(d);
    let three = |k: &str| v.get(k).filter(|t| t.len() == 3).map(|t| [t[0], t[1], t[2]]);
    let light = three("r_filmlighttint")?;
    let dark = three("r_filmdarktint")?;
    let medium = three("r_filmmediumtint").unwrap_or(std::array::from_fn(|k| (light[k] + dark[k]) * 0.5));
    let desaturation = one("r_filmdesaturation", 0.0);
    Some(Film {
        enable: one("r_filmenable", 0.0) > 0.5,
        contrast: one("r_filmcontrast", 1.0),
        brightness: one("r_filmbrightness", 0.0),
        desaturation,
        desaturation_dark: one("r_filmdesaturationdark", desaturation),
        dark,
        medium,
        light,
    })
}

/// The film as the game's `postfx_color2` draws it (decompiled;
/// docs/mwr_engine/post.md), as the grade's 32^3 table: the tone-mapped
/// colour in sRGB, its luma L (Rec. 601); toward L by the desaturation
/// (the dark one at L = 0, the light at 1); times the tint (the quadratic
/// through the dark tint at 0, the medium at one half, the light at 1) and
/// the contrast about one half, plus the brightness. (Which film settings
/// feed which of the pass's constants is the game's code, not read: this
/// mapping is ours.) The table's input and output are this renderer's
/// gamma values (the square root of linear).
fn film_lut(f: &Film) -> Vec<u8> {
    let n = asset_world::T6_LUT_SIZE;
    let enc = |x: f32| if x <= 0.0031308 { x * 12.92 } else { 1.055 * x.powf(1.0 / 2.4) - 0.055 };
    let dec = |x: f32| if x <= 0.04045 { x / 12.92 } else { ((x + 0.055) / 1.055).powf(2.4) };
    // The tint's quadratic t(L) = c0 + c1 L + c2 L^2 through (0, dark),
    // (1/2, medium), (1, light).
    let c0 = f.dark;
    let c1: [f32; 3] = std::array::from_fn(|k| 4.0 * f.medium[k] - 3.0 * f.dark[k] - f.light[k]);
    let c2: [f32; 3] = std::array::from_fn(|k| 2.0 * f.light[k] + 2.0 * f.dark[k] - 4.0 * f.medium[k]);
    let mut out = Vec::with_capacity(n * n * n * 4);
    for b in 0..n {
        for g in 0..n {
            for r in 0..n {
                let gam = [r, g, b].map(|i| i as f32 / (n - 1) as f32);
                let mut c = gam.map(|x| enc((x * x).clamp(0.0, 1.0)));
                let l = c[0] * 0.299 + c[1] * 0.587 + c[2] * 0.114;
                let desat = f.desaturation_dark + (f.desaturation - f.desaturation_dark) * l;
                let lc = l.clamp(0.0, 1.0);
                for k in 0..3 {
                    c[k] += desat * (lc - c[k]);
                    let tint = c0[k] + c1[k] * lc + c2[k] * lc * lc;
                    c[k] = (c[k] * tint - 0.5) * f.contrast + 0.5 + f.brightness;
                }
                for k in 0..3 {
                    out.push((dec(c[k].clamp(0.0, 1.0)).sqrt() * 255.0).round() as u8);
                }
                out.push(255);
            }
        }
    }
    out
}

/// The baked light of the world's ground, read under a point: the
/// highest lightmapped triangle below it (within 64 units above it), its
/// lightmap texel's A + B / 2 (the converted pages: CoD4's square root of
/// a quarter of the light, under `exposure`).
struct GroundLight<'a> {
    world: &'a iw3::zone::GfxWorld,
    pages: &'a [Option<(usize, usize, Vec<u8>)>],
    exposure: f32,
    cells: std::collections::HashMap<(i32, i32), Vec<(u32, u32)>>,
}

const GROUND_CELL: f32 = 256.0;

impl<'a> GroundLight<'a> {
    fn new(world: &'a iw3::zone::GfxWorld, pages: &'a [Option<(usize, usize, Vec<u8>)>], exposure: f32) -> Self {
        let mut cells: std::collections::HashMap<(i32, i32), Vec<(u32, u32)>> = std::collections::HashMap::new();
        for (si, s) in world.surfaces.iter().enumerate() {
            if pages.get(usize::from(s.lightmap_index)).and_then(|p| p.as_ref()).is_none() {
                continue;
            }
            for t in 0..u32::from(s.tri_count) {
                let Some(tri) = Self::tri(world, s, t) else { continue };
                let lo = [0, 1].map(|k| tri.iter().map(|v| v.xyz[k]).fold(f32::MAX, f32::min));
                let hi = [0, 1].map(|k| tri.iter().map(|v| v.xyz[k]).fold(f32::MIN, f32::max));
                let c0 = lo.map(|v| (v / GROUND_CELL).floor() as i32);
                let c1 = hi.map(|v| (v / GROUND_CELL).floor() as i32);
                if (c1[0] - c0[0] + 1) * (c1[1] - c0[1] + 1) > 64 {
                    continue;
                }
                for x in c0[0]..=c1[0] {
                    for y in c0[1]..=c1[1] {
                        cells.entry((x, y)).or_default().push((si as u32, t));
                    }
                }
            }
        }
        Self { world, pages, exposure, cells }
    }

    fn tri<'w>(world: &'w iw3::zone::GfxWorld, s: &iw3::zone::GfxSurface, t: u32) -> Option<[&'w iw3::zone::WorldVertex; 3]> {
        let first = s.first_vertex.max(0) as usize;
        let base = s.base_index.max(0) as usize + 3 * t as usize;
        let ix = world.indices.get(base..base + 3)?;
        Some([world.vertices.get(first + usize::from(ix[0]))?, world.vertices.get(first + usize::from(ix[1]))?, world.vertices.get(first + usize::from(ix[2]))?])
    }

    fn at(&self, p: [f32; 3]) -> Option<[f32; 3]> {
        let key = ((p[0] / GROUND_CELL).floor() as i32, (p[1] / GROUND_CELL).floor() as i32);
        let mut best: Option<(f32, usize, [f32; 2])> = None;
        for &(si, t) in self.cells.get(&key)? {
            let s = &self.world.surfaces[si as usize];
            let Some([a, b, c]) = Self::tri(self.world, s, t) else { continue };
            // Barycentric in xy.
            let d = (b.xyz[1] - c.xyz[1]) * (a.xyz[0] - c.xyz[0]) + (c.xyz[0] - b.xyz[0]) * (a.xyz[1] - c.xyz[1]);
            if d.abs() < 1e-6 {
                continue;
            }
            let w0 = ((b.xyz[1] - c.xyz[1]) * (p[0] - c.xyz[0]) + (c.xyz[0] - b.xyz[0]) * (p[1] - c.xyz[1])) / d;
            let w1 = ((c.xyz[1] - a.xyz[1]) * (p[0] - c.xyz[0]) + (a.xyz[0] - c.xyz[0]) * (p[1] - c.xyz[1])) / d;
            let w2 = 1.0 - w0 - w1;
            if w0 < -1e-4 || w1 < -1e-4 || w2 < -1e-4 {
                continue;
            }
            let z = w0 * a.xyz[2] + w1 * b.xyz[2] + w2 * c.xyz[2];
            if z > p[2] + 64.0 || best.is_some_and(|(bz, ..)| bz >= z) {
                continue;
            }
            let uv = [0, 1].map(|k| w0 * a.lmap_coord[k] + w1 * b.lmap_coord[k] + w2 * c.lmap_coord[k]);
            best = Some((z, usize::from(s.lightmap_index), uv));
        }
        let (_, page, uv) = best?;
        let (w, h, data) = self.pages.get(page)?.as_ref()?;
        let half = h / 2;
        let x = ((uv[0].fract() + 1.0).fract() * *w as f32) as usize % w.max(&1);
        let y = ((uv[1].clamp(0.0, 0.9999)) * half as f32) as usize;
        let texel = |row: usize| -> [f32; 3] {
            let i = (row * w + x) * 4;
            let q = |o: usize| {
                let v = f32::from(*data.get(i + o).unwrap_or(&0)) / 255.0;
                v * v * 4.0 / self.exposure.max(1e-6)
            };
            // (Stored blue, green, red.)
            [q(2), q(1), q(0)]
        };
        let a = texel(y);
        let b = texel(half + y);
        Some(std::array::from_fn(|k| a[k] + 0.5 * b[k]))
    }
}

/// The log-average of the squared brightness, before bloom and the HUD,
/// that leaves our finished pictures where the real game's are (Rust's
/// three references, HUD and all: 0.21, 0.21, 0.29; ours read 0.31 at a
/// key of 0.22).
const AUTO_KEY: f32 = 0.15;

/// The automatic exposure's key for a map, fitted to HorizonMW's own
/// intermission picture of it (`look_keys.txt`: `mp_name key` lines).
fn map_look_key(map: &str) -> Option<f32> {
    const KEYS: &str = include_str!("look_keys.txt");
    KEYS.lines()
        .filter_map(|l| {
            let mut it = l.split_whitespace();
            Some((it.next()?, it.next()?.parse::<f32>().ok()?))
        })
        .find(|(m, _)| m.eq_ignore_ascii_case(map))
        .map(|(_, k)| k)
        .filter(|k| k.is_finite() && *k > 0.0)
}

/// The sun against the bake: pi (the bake's texels hold radiance, light
/// over pi; the sun's colour is irradiance): Rust's real sunlit sand
/// stands 7 times its shaded sand, ours stood 2.7 times without it.
const SUN_PI: f32 = std::f32::consts::PI;

/// The sky fog falloff's angles as drawn, against the fog script's
/// (fitted to Rust's real sky).
const SKY_FALL_SCALE: f32 = 0.2;

/// Modern Warfare Remastered's own lit world shader (`mwr_shade`) under
/// the look (`BO2MP_H1_MWR=0`: Black Ops III's).
fn mwr_on() -> bool {
    std::env::var("BO2MP_H1_MWR").map_or(true, |v| v.trim() != "0")
}

/// One channel of a packed small float (`R11G11B10_FLOAT`): `m` mantissa
/// bits, five exponent bits, no sign.
fn small_float(v: u32, m: u32) -> f32 {
    let mant = v & ((1 << m) - 1);
    let exp = (v >> m) & 31;
    match exp {
        0 => mant as f32 / (1 << m) as f32 * 2f32.powi(-14),
        31 => 0.0,
        e => 2f32.powi(e as i32 - 15) * (1.0 + mant as f32 / (1 << m) as f32),
    }
}

/// The map's reflection probes for Black Ops III's look: each probe's cube
/// (inline in the zone, `R11G11B10_FLOAT` or `R16G16B16A16_FLOAT`, faces
/// each with their levels) times the baked light's `scale`, as one cube
/// array (half floats) linked as the map's probes. The first 32 of one
/// size; returns their origins (index = the array's layer).
fn link_probes(
    cap: &Capture,
    image: &[u8],
    world: &fastfile_h1::map::World,
    scale: f32,
    catalog: &mut MaterialCatalog,
    owner: asset_core::ZoneOwner,
    name: &str,
    report: &mut Vec<String>,
) -> Vec<(usize, [f32; 3], [f32; 3])> {
    use bevy::asset::RenderAssetUsages;
    use bevy::prelude::Image;
    use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat, TextureViewDescriptor, TextureViewDimension};
    let mut kept: Vec<(usize, [f32; 3], [f32; 3])> = Vec::new();
    let mut size_levels: Option<(u32, u32)> = None;
    let mut data: Vec<u8> = Vec::new();
    for (i, (id, origin)) in world.reflection_probes.iter().enumerate() {
        if kept.len() >= 32 {
            break;
        }
        let Some(rec) = id.and_then(|id| cap.images.get(id.index as usize)) else { continue };
        let Some(span) = rec.pixels else { continue };
        let (s, stored) = (u32::from(rec.width), u32::from(rec.level_count.max(1)));
        if rec.map_type != 5 || rec.width != rec.height || s == 0 {
            continue;
        }
        // (A probe stored with its top level only, as the usermaps': its
        // smaller levels box-filtered here; the game's shader blurs by them.)
        let lv = if stored == 1 { 32 - s.leading_zeros() } else { stored };
        if size_levels.is_some_and(|sl| sl != (s, lv)) {
            continue;
        }
        let texels: usize = (0..lv).map(|k| ((s >> k).max(1) * (s >> k).max(1)) as usize).sum();
        // Its texels (faces, each with its levels), linear: R11G11B10 and
        // half floats as written; RGBA8 and BC3 (the usermaps' probes, MW2's
        // own: low range, as the game's shader samples them) and BC6H.
        let Some(mut colours) = probe_texels(rec.format, span.get(image), s as usize, stored as usize) else { continue };
        if stored == 1 && lv > 1 {
            let n = (s * s) as usize;
            if colours.len() != n * 6 {
                continue;
            }
            let mut full = Vec::with_capacity(texels * 6);
            for f in 0..6 {
                let mut cur = colours[f * n..(f + 1) * n].to_vec();
                let mut d = s as usize;
                full.extend_from_slice(&cur);
                while d > 1 {
                    let h = d / 2;
                    let mut next = vec![[0f32; 3]; h * h];
                    for y in 0..h {
                        for x in 0..h {
                            for c in 0..3 {
                                next[y * h + x][c] = (cur[2 * y * d + 2 * x][c] + cur[2 * y * d + 2 * x + 1][c] + cur[(2 * y + 1) * d + 2 * x][c] + cur[(2 * y + 1) * d + 2 * x + 1][c]) * 0.25;
                            }
                        }
                    }
                    full.extend_from_slice(&next);
                    cur = next;
                    d = h;
                }
            }
            colours = full;
        }
        if colours.len() != texels * 6 {
            continue;
        }
        size_levels = Some((s, lv));
        let first = data.len();
        for c in colours {
            for v in c {
                let v = if v.is_finite() { (v * scale).clamp(0.0, 60000.0) } else { 0.0 };
                data.extend_from_slice(&super::t7_map::f16_bits(v).to_le_bytes());
            }
            data.extend_from_slice(&super::t7_map::f16_bits(1.0).to_le_bytes());
        }
        // Its light all round: the mean of each face's smallest level
        // (already scaled, half floats as written).
        let mut amb = [0f32; 3];
        for f in 0..6 {
            let at = first + (f * texels + texels - 1) * 8;
            for k in 0..3 {
                amb[k] += fastfile_h1::map::half_to_f32(u16::from_le_bytes([data[at + 2 * k], data[at + 2 * k + 1]])) / 6.0;
            }
        }
        kept.push((i, *origin, amb));
    }
    let Some((s, lv)) = size_levels else {
        report.push("h1 look: no reflection probes read".to_owned());
        return kept;
    };
    let mut cube = Image::new_uninit(
        Extent3d { width: s, height: s, depth_or_array_layers: 6 * kept.len() as u32 },
        TextureDimension::D2,
        TextureFormat::Rgba16Float,
        RenderAssetUsages::RENDER_WORLD,
    );
    cube.texture_descriptor.mip_level_count = lv;
    cube.texture_view_descriptor = Some(TextureViewDescriptor { dimension: Some(TextureViewDimension::CubeArray), ..Default::default() });
    cube.data = Some(data);
    cube.sampler = super::t7_light::clamped();
    let semantic = super::t6_materials::T6_PROBES_SEMANTIC;
    let slot = super::t7_light::link_image(catalog, format!("h1:{name}#probes"), s, lv, semantic, cube);
    catalog.link_material(super::t7_light::material(super::t6_materials::T6_PROBES_MATERIAL.to_owned(), semantic, slot, owner, None));
    report.push(format!("h1 look: {} of {} reflection probes ({s}px, {lv} levels)", kept.len(), world.reflection_probes.len()));
    kept
}

/// A cube probe's texels (six faces, each with its `levels` from `size`
/// down), linear RGB, by its DXGI format.
fn probe_texels(format: u32, raw: &[u8], size: usize, levels: usize) -> Option<Vec<[f32; 3]>> {
    let dims = |k: usize| (size >> k).max(1);
    let mut out = Vec::new();
    match format {
        26 | 10 | 28 | 29 => {
            let bpp = if format == 10 { 8 } else { 4 };
            let n: usize = (0..levels).map(|k| dims(k) * dims(k)).sum::<usize>() * 6;
            for t in raw.get(..n * bpp)?.chunks_exact(bpp) {
                out.push(match format {
                    26 => {
                        let v = u32::from_le_bytes([t[0], t[1], t[2], t[3]]);
                        [small_float(v & 0x7ff, 6), small_float((v >> 11) & 0x7ff, 6), small_float(v >> 22, 5)]
                    }
                    10 => {
                        let h = |k: usize| fastfile_h1::map::half_to_f32(u16::from_le_bytes([t[2 * k], t[2 * k + 1]]));
                        [h(0), h(1), h(2)]
                    }
                    29 => [0, 1, 2].map(|k| {
                        let x = f32::from(t[k]) / 255.0;
                        if x <= 0.04045 { x / 12.92 } else { ((x + 0.055) / 1.055).powf(2.4) }
                    }),
                    _ => [0, 1, 2].map(|k| f32::from(t[k]) / 255.0),
                });
            }
        }
        76..=78 | 94..=96 => {
            let mut at = 0usize;
            for _face in 0..6 {
                for k in 0..levels {
                    let d = dims(k);
                    let bw = d.div_ceil(4);
                    let (pw, ph) = (bw * 4, bw * 4);
                    let mut level = vec![[0f32; 3]; pw * ph];
                    for by in 0..bw {
                        for bx in 0..bw {
                            let block = raw.get(at..at + 16)?;
                            at += 16;
                            let mut px = [[0f32; 3]; 16];
                            if format <= 78 {
                                let mut b = [0u8; 64];
                                bcdec_rs::bc3(block, &mut b, 16);
                                let srgb = format == 78;
                                for i in 0..16 {
                                    px[i] = [0, 1, 2].map(|c| {
                                        let x = f32::from(b[i * 4 + c]) / 255.0;
                                        if srgb { if x <= 0.04045 { x / 12.92 } else { ((x + 0.055) / 1.055).powf(2.4) } } else { x }
                                    });
                                }
                            } else {
                                let mut f = [0f32; 48];
                                bcdec_rs::bc6h_float(block, &mut f, 12, format == 96);
                                for i in 0..16 {
                                    px[i] = [f[i * 3], f[i * 3 + 1], f[i * 3 + 2]];
                                }
                            }
                            for y in 0..4 {
                                for x in 0..4 {
                                    level[(by * 4 + y) * pw + bx * 4 + x] = px[y * 4 + x];
                                }
                            }
                        }
                    }
                    for y in 0..d {
                        out.extend_from_slice(&level[y * pw..y * pw + d]);
                    }
                }
            }
        }
        _ => return None,
    }
    Some(out)
}

#[cfg(test)]
mod small_float_tests {
    use super::{film_lut, film_of, Film};
    #[test]
    fn film_table() {
        let neutral = Film { enable: true, contrast: 1.0, brightness: 0.0, desaturation: 0.0, desaturation_dark: 0.0, dark: [1.0; 3], medium: [1.0; 3], light: [1.0; 3] };
        let lut = film_lut(&neutral);
        let n = asset_world::T6_LUT_SIZE;
        for (i, px) in lut.chunks_exact(4).enumerate() {
            let want = [i % n, (i / n) % n, i / (n * n)].map(|v| (v as f32 / (n - 1) as f32 * 255.0).round());
            for k in 0..3 {
                assert!((f32::from(px[k]) - want[k]).abs() <= 1.0, "texel {i} channel {k}: {} for {}", px[k], want[k]);
            }
        }
        // Rust's film warms a mid grey (more red than blue).
        let text = "r_filmEnable \"1\"\nr_filmContrast \"1.05\"\nr_filmDesaturation \"-0.1\"\nr_filmDesaturationDark \"-0.15\"\nr_filmLightTint \"1 1 1\"\nr_filmMediumTint \"1.21772 1.01266 0.704429\"\nr_filmDarkTint \"1.19494 0.946567 0.789566\"";
        let lut = film_lut(&film_of(text).unwrap());
        let mid = n / 2;
        let at = ((mid * n + mid) * n + mid) * 4;
        assert!(lut[at] > lut[at + 2] + 20, "grey to {:?}", &lut[at..at + 3]);
    }

    #[test]
    fn packed_floats_read() {
        // 1.0: exponent 15, mantissa 0.
        assert_eq!(super::small_float(15 << 6, 6), 1.0);
        assert_eq!(super::small_float(15 << 5, 5), 1.0);
        assert_eq!(super::small_float((16 << 6) | 32, 6), 3.0);
        assert_eq!(super::small_float(0, 6), 0.0);
    }
}
