//! bo2mp: Call of Duty: Ghosts (IW6) multiplayer maps.
//!
//! A Ghosts map zone (`IWffu100`, version 0x235) is walked by
//! `fastfile_iw6`'s exact walk and read into plain data by
//! `fastfile_iw6::map`; this turns it into the match's world the way the
//! Call of Duty 4 / World at War lane does (`super::cod4`): the world is
//! handed to the IW3 world builder (Ghosts' world vertex is CoD4's layout),
//! the collision to the IW3 clip builder (brushes, terrain triangles under
//! the AABB forest, brush models), the lightmaps converted to CoD4's form
//! (`fastfile_iw6::map::iw3_lightmap`), the entities named (Ghosts stores
//! their keys as numbers and its spawn points beside them) and its trigger
//! models (`"model" "?N"`) kept for the scripts. Materials and placed
//! models go through the Ghosts guest-gun builder (`super::iw6::Builder`):
//! colour maps and layers from the zone or its `imagefile<N>.pak`s.
//! Black Ops II's rules then add everything else (`attach_t6_rules`).
//!
//! No ZoneGame of its own: the map walk sends a zone of Ghosts' version
//! here (`asset_transport::legacy_game_for_path` says `iw6`).

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use asset_core::AssetNamespace;
use asset_material::MaterialCatalog;
use asset_transport::progress::{LoadProgress, StageId};
use asset_world::WorldDrawPolicy;
use fastfile_iw6::map::IwMap;

use super::LoadedWorld;
use crate::lane_capability::PreparedCapability;
use crate::session_load::PreparedWorld;

/// The largest level kept of a world surface's pictures and of a placed
/// model's (`BO2MP_GH_MAP_TEXTURE=<size>` changes both).
const WORLD_TEXTURE_MAX: u32 = 512;

fn texture_max() -> u32 {
    std::env::var("BO2MP_GH_MAP_TEXTURE")
        .ok()
        .and_then(|v| v.trim().parse().ok())
        .filter(|&v: &u32| (64..=4096).contains(&v))
        .unwrap_or(WORLD_TEXTURE_MAX)
}

/// A model's first LOD as a mesh, its materials linked in `b`'s catalog
/// (a placed model's vertex colour is no tint in Ghosts: its foliage keeps
/// wind and reactive-motion weights there, which tinted ferns and grass
/// red, so it is set white).
fn model_mesh(
    b: &mut super::iw6::Builder<'_>,
    model: &fastfile_iw6::walk::XModelRec,
    max: u32,
) -> Option<asset_world::ModelMesh> {
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
    let mesh = asset_world::build_t6_model_mesh(&xref, |k| linked.get(&k.index).copied());
    // (What the builder leaves out of a model: surfaces with no vertices,
    // no triangles or an index past their vertices; counted for the report.)
    let built = mesh.as_ref().map_or(0, |m| m.lod_surfaces[0].len());
    let lost = xref.lod0.len().saturating_sub(built);
    if lost > 0 {
        use std::sync::atomic::Ordering::Relaxed;
        DROPPED_SURFACES.fetch_add(lost, Relaxed);
        DROPPED_MODELS.fetch_add(1, Relaxed);
        if mesh.is_none() {
            EMPTY_MODELS.fetch_add(1, Relaxed);
        }
    }
    mesh
}

static DROPPED_SURFACES: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
static DROPPED_MODELS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
static EMPTY_MODELS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

/// Meshes of models (lower case names) the shared zones hold, appended to
/// `meshes`: each zone beside the map is walked once for this (a second or
/// two) and only when a map names a model it does not hold; the models'
/// materials link into `b`'s catalog. Name -> index into `meshes`.
fn shared_model_meshes(
    zone_dir: &Path,
    names: &[String],
    b: &mut super::iw6::Builder<'_>,
    max: u32,
    meshes: &mut Vec<asset_world::ModelMesh>,
) -> HashMap<String, usize> {
    let mut found: HashMap<String, usize> = HashMap::new();
    for z in fastfile_iw6::shared::lane_zones() {
        if names.iter().all(|n| found.contains_key(n)) {
            break;
        }
        let p = zone_dir.join(format!("{z}.ff"));
        if !p.is_file() {
            continue;
        }
        let walked = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let zone = fastfile_iw6::Zone::open(&p).ok()?;
            let (cap2, _) = fastfile_iw6::walk::walk_zone(&zone);
            Some((zone, cap2))
        }));
        let Ok(Some((zone, cap2))) = walked else {
            diag::warn!(World, "bo2mp iw6: shared models: reading {} failed", p.display());
            continue;
        };
        let mut b2 = super::iw6::Builder::new(&cap2, &zone, &p, std::mem::take(&mut b.catalog));
        b2.shared = b.shared.clone();
        for (i, m) in cap2.xmodels.iter().enumerate() {
            let key = m.name.trim_start_matches(',').to_ascii_lowercase();
            if !names.contains(&key)
                || found.contains_key(&key)
                || !fastfile_iw6::map::model_has_geometry(&cap2, i as u32)
            {
                continue;
            }
            if let Some(mesh) = model_mesh(&mut b2, m, max) {
                meshes.push(mesh);
                found.insert(key, meshes.len() - 1);
            }
        }
        b.catalog = std::mem::take(&mut b2.catalog);
    }
    found
}

/// The images and materials Ghosts' map zones name but do not hold
/// (`,name`): those of `common_mp` and `common_core_mp` (and their patches)
/// beside the map, read once per process (two walks of a second or two;
/// image records, inline pixels and material texture lists kept, streamed
/// pixels read from the paks when a map wants them).
pub(super) fn shared_images(zone_dir: &Path) -> Option<std::sync::Arc<fastfile_iw6::shared::SharedImages>> {
    use std::sync::{Arc, Mutex, OnceLock};
    static CACHE: OnceLock<
        Mutex<HashMap<std::path::PathBuf, Arc<fastfile_iw6::shared::SharedImages>>>,
    > = OnceLock::new();
    let cache = CACHE.get_or_init(Default::default);
    let mut guard = cache.lock().ok()?;
    if let Some(hit) = guard.get(zone_dir) {
        return Some(hit.clone());
    }
    let mut shared = fastfile_iw6::shared::SharedImages::default();
    for z in fastfile_iw6::shared::lane_zones() {
        let p = zone_dir.join(format!("{z}.ff"));
        if !p.is_file() {
            continue;
        }
        let read = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| shared.add_zone(&p)));
        match read {
            Ok(Ok(_)) => {}
            Ok(Err(e)) => diag::warn!(World, "bo2mp iw6: shared images: {e}"),
            Err(_) => diag::warn!(
                World,
                "bo2mp iw6: shared images: reading {} panicked",
                p.display()
            ),
        }
    }
    let shared = Arc::new(shared);
    guard.insert(zone_dir.to_path_buf(), shared.clone());
    Some(shared)
}

pub(super) fn rotate(v: [f32; 3], axis: &[[f32; 3]; 3]) -> [f32; 3] {
    std::array::from_fn(|k| axis[0][k] * v[0] + axis[1][k] * v[1] + axis[2][k] * v[2])
}

pub(super) fn place_point(v: [f32; 3], origin: [f32; 3], axis: &[[f32; 3]; 3]) -> [f32; 3] {
    let r = rotate(v, axis);
    std::array::from_fn(|k| origin[k] + r[k])
}

/// CoD4's packed unit vector (as `asset_model::unpack_unit_vec` reads it,
/// scale byte 63: 1/127 a step).
pub(super) fn iw3_unit_vec(v: [f32; 3]) -> u32 {
    let b = |x: f32| {
        (x.clamp(-1.0, 1.0) * 127.0 + 127.0)
            .round()
            .clamp(0.0, 255.0) as u32
    };
    b(v[0]) | b(v[1]) << 8 | b(v[2]) << 16 | 63 << 24
}

/// bo2mp: whether Ghosts maps take Black Ops III's look (`super::bo3_look`,
/// as Carrier): `BO2MP_BO3_LOOK_IW6=0` keeps CoD4's lightmap draw under the
/// map's own exposure. On by default.
fn look_on() -> bool {
    super::bo3_look::game_on("BO2MP_BO3_LOOK_IW6", true)
}

fn env_f(name: &str, default: f32) -> f32 {
    codlegacy_env::var(name)
        .ok()
        .and_then(|v| v.trim().parse().ok())
        .filter(|v: &f32| v.is_finite() && *v >= 0.0)
        .unwrap_or(default)
}

/// The ground light of the lit models round `origin`: the inverse-square
/// distance mean of the `NEIGHBOURS` nearest (height counts double: a tree's
/// top is not near a crate under it), so a prop in a shaded corner and one
/// in the open no longer share one light. `None` with no lit model at all.
fn neighbour_light(origin: [f32; 3], lit: &[([f32; 3], [f32; 4])]) -> Option<[f32; 4]> {
    const NEIGHBOURS: usize = 4;
    let mut near: [(f32, usize); NEIGHBOURS] = [(f32::MAX, 0); NEIGHBOURS];
    for (i, (o, _)) in lit.iter().enumerate() {
        let (dx, dy, dz) = (o[0] - origin[0], o[1] - origin[1], 2.0 * (o[2] - origin[2]));
        let d = dx * dx + dy * dy + dz * dz;
        if d < near[NEIGHBOURS - 1].0 {
            near[NEIGHBOURS - 1] = (d, i);
            near.sort_by(|a, b| a.0.total_cmp(&b.0));
        }
    }
    let (mut sum, mut weight) = ([0.0f32; 4], 0.0f32);
    for &(d, i) in near.iter().filter(|n| n.0 < f32::MAX) {
        // (A floor of 32 units keeps a model on top of a lit one its own.)
        let w = 1.0 / d.max(1024.0);
        for k in 0..4 {
            sum[k] += lit[i].1[k] * w;
        }
        weight += w;
    }
    (weight > 0.0).then(|| sum.map(|v| v / weight))
}

/// The look's light levels, as Modern Warfare Remastered's (`h1_map`): the
/// baked light's shaded texels' mean (`BO2MP_IW6_SHADE`), the sun's light
/// over its colour (`BO2MP_IW6_SUN`), the tone curve's exposure
/// (`BO2MP_IW6_EXPOSURE`) and the placed models' light over their ground
/// light (`BO2MP_IW6_MODEL`), all in the baked light's linear units.
const LOOK_SHADE: f32 = 0.4;
/// `LOOK_SHADE` as the whole shaded light (`A + 0.8 B`) of Prison Break's
/// baked pages, where the upper page's shaded mean (0.09) was set to 0.4:
/// 0.09 + 0.8 x 0.204 = 0.253, times 0.4 / 0.09.
const LOOK_SHADE_TOTAL: f32 = 1.126;
const LOOK_SUN: f32 = 2.6;
const LOOK_EXPOSURE: f32 = 0.8;
const LOOK_MODEL: f32 = 1.0;
/// The least the placed models' light is scaled to where the sun is live.
const MODEL_K_MIN: f32 = 0.15;

/// The factor on the placed models' ground light that brings their median
/// (`median`, the look's units) to the world's shaded level (`level`); never
/// brightens, never below `MODEL_K_MIN`.
fn model_light_k(level: f32, median: f32) -> f32 {
    if median > 0.0 && level.is_finite() { (level / median).clamp(MODEL_K_MIN, 1.0) } else { 1.0 }
}

/// The display value a mid-grey in the sun is solved to (`bo3_look::solve_exposure`):
/// the middle of the 0.5..0.85 band the look check calls right.
const AUTO_GREY: f32 = 0.65;

/// A baked light whose sunlit texels are this much brighter than its shaded
/// ones holds the sun's own light (none is drawn over it).
const SUN_IN_BAKE: f32 = 2.5;

/// bo2mp: whether Ghosts' maps are drawn by Ghosts' own renderer
/// (`render_iw6`, the only renderer on them, as MWR's and Black Ops III's
/// maps have theirs). `BO2MP_IW6_NATIVE=0` returns to the converted world
/// under Black Ops II's renderer (for comparison).
pub(crate) fn iw6_native_on() -> bool {
    !std::env::var("BO2MP_IW6_NATIVE").is_ok_and(|v| v.trim() == "0")
}

/// bo2mp: the zones Ghosts' own renderer draws a map from: the map, its
/// technique set zone and patch, the shared technique set zones (which
/// hold every technique and program; the map's technique sets are names)
/// and the shared zones its materials and pictures come from, where
/// present beside it. `None` when the native renderer is off.
pub(crate) fn native_zones(map_path: &Path) -> Option<Vec<PathBuf>> {
    if !iw6_native_on() {
        return None;
    }
    let name = map_path.file_stem()?.to_string_lossy().into_owned();
    let dir = map_path.parent()?;
    let mut out = vec![map_path.to_path_buf()];
    let mut names = vec![
        format!("techsets_{name}.ff"),
        format!("patch_{name}.ff"),
        "techsets_code_post_gfx_mp.ff".to_owned(),
        "techsets_common_core_mp.ff".to_owned(),
        "techsets_common_mp.ff".to_owned(),
        "techsets_common.ff".to_owned(),
    ];
    names.extend(fastfile_iw6::shared::SHARED_ZONES.iter().map(|z| format!("{z}.ff")));
    for z in names {
        let p = dir.join(&z);
        if p.is_file() && !out.contains(&p) {
            out.push(p);
        }
    }
    Some(out)
}

/// Loads a Ghosts map zone into the match's world. A zone that cannot be
/// read becomes a gap with the reason, never a crash.
pub(crate) fn load_iw6_world(
    path: &Path,
    progress: &LoadProgress,
    material_seed: MaterialCatalog,
) -> LoadedWorld {
    load_iw6_world_for(path, progress, material_seed, false)
}

/// Test support: a Ghosts map zone's collision and spawn origins, loaded the way a match loads
/// them (no materials, no drawing). `None` when the zone cannot be read.
#[doc(hidden)]
pub fn collision_for_tests(path: &Path) -> Option<(asset_world::ClipCollision, Vec<[f32; 3]>)> {
    let loaded = load_iw6_world_for(path, &LoadProgress::default(), MaterialCatalog::default(), true);
    let spawns = loaded.spawns.iter().map(|s| s.origin).collect();
    loaded.collision.map(|c| (c, spawns))
}

/// The light row the fallback pass reads on a Ghosts map's frame drawn in
/// stages (`IW6_ROW` in `geometry_diagnostic.wgsl`). Its v0 is the carrier
/// look's (`carrier_look::LOOK_ROW`, the same row), which a Ghosts map never
/// turns on: v0 stays zero.
pub(crate) const NATIVE_HDR_ROW: usize = 251;

/// That row: v1 the sun's diffuse light as Ghosts' model shaders take it
/// (the ComWorld sun's linear colour times `r_diffuseColorScale`, 1; as
/// `render_iw6::scene`) and the factor from this lane's light units (the
/// moving things' grid and the placed models' light are scaled by
/// `exposure`, the look's scale) back to Ghosts' own; v2 the sun's specular
/// light (`r_specularColorScale` 2.5) and the factor a display value of the
/// effects is taken back by: one over the exposure the map's iris settles
/// at (INFERRED: Black Ops II's effects are authored as display values,
/// Ghosts' as light; 2 without a tone mapping asset).
pub(crate) fn native_hdr_row(sun: Option<&fastfile_iw6::map::Sun>, exposure: f32, settle: Option<f32>) -> [f32; 16] {
    let colour = sun.map_or([1.0; 3], |s| s.color);
    let diffuse = env_f("BO2MP_IW6_DIFFUSE_SCALE", 1.0);
    let specular = env_f("BO2MP_IW6_SPECULAR_SCALE", 2.5);
    let mut row = [0f32; 16];
    for k in 0..3 {
        row[4 + k] = colour[k] * diffuse;
        row[8 + k] = colour[k] * specular;
    }
    row[7] = 1.0 / exposure.max(1e-4);
    row[11] = 1.0 / settle.filter(|e| e.is_finite() && *e > 0.0).unwrap_or(2.0);
    row
}

/// `load_iw6_world` for a match: with `native` the converted world (its
/// surfaces, placed model meshes, materials, pictures and sky) is not
/// built, Ghosts' own renderer (`render_iw6`) draws the map; collision,
/// spawns, entities, scripts and the map's sounds are read as before.
/// bo2mp: the map's own script data (`fastfile_iw6::maplogic`): its door
/// switches handed to the sim (`asset_world::set_iw6_doors`), one line for
/// the report.
fn iw6_map_logic(path: &Path, zone: &fastfile_iw6::Zone, cap: &fastfile_iw6::walk::Capture, map: &IwMap) -> String {
    let name = path.file_stem().map_or_else(String::new, |s| s.to_string_lossy().into_owned());
    let src = fastfile_iw6::map::raw_file(cap, &zone.image, &format!("maps/createfx/{name}_fx.gsc"))
        .map(|b| String::from_utf8_lossy(&b).into_owned());
    let logic = fastfile_iw6::maplogic::MapLogic::read(&map.entities, src.as_deref());
    let sound = |p: &fastfile_iw6::maplogic::MoverPiece, k: &str| p.parameter(k).map(str::to_owned);
    asset_world::set_iw6_doors(
        logic
            .doors
            .iter()
            .map(|d| asset_world::Iw6DoorGroup {
                name: d.name.clone(),
                switches: d.switches.clone(),
                button_sound: d.parameters.iter().find(|(k, _)| k == "button_sound").map(|(_, v)| v.clone()),
                pieces: d
                    .pieces
                    .iter()
                    .map(|p| asset_world::Iw6DoorPiece {
                        brush: p.brush,
                        model: p.model.clone(),
                        start: p.start,
                        start_angles: p.start_angles,
                        end: p.end,
                        end_angles: p.end_angles,
                        start_sound: sound(p, "start_sound"),
                        stop_sound: sound(p, "stop_sound"),
                        loop_sound: sound(p, "loop_sound"),
                    })
                    .collect(),
            })
            .collect(),
    );
    let scripts_line = iw6_script_pack(path, &name, zone, cap, map, &logic);
    format!("iw6 map logic: {name}: {}; {scripts_line}", logic.summary())
}

/// bo2mp: `BO2MP_IW6_SCRIPTS=0` runs the old inferred door driver instead of the map's real
/// compiled scripts.
fn iw6_scripts_on() -> bool {
    !std::env::var("BO2MP_IW6_SCRIPTS").is_ok_and(|v| v.trim() == "0")
}

/// The zones whose scripts a Ghosts map runs, in load order (a later zone's script replaces an
/// earlier one of the same name): the shared ones, then the map's own and its patch.
/// (`common_bots_mp` has the agent scripts: the teleport and nuke scripts call them.)
const SCRIPT_ZONES: [&str; 7] =
    ["common", "common_mp", "common_core_mp", "common_bots_mp", "patch_common", "patch_common_mp", "patch_common_core_mp"];

/// bo2mp: collects the compiled scripts of the shared zones and the map's (and its patch's), the
/// string tables, the map's entity blocks with their numeric keys and the exploder placements,
/// and hands them to the sim (`asset_world::set_iw6_scripts`). The shared zones come from a cache
/// in the temp directory when it has them (the 124 MB `common` takes seconds to walk).
fn iw6_script_pack(
    path: &Path,
    name: &str,
    zone: &fastfile_iw6::Zone,
    cap: &fastfile_iw6::walk::Capture,
    map: &IwMap,
    logic: &fastfile_iw6::maplogic::MapLogic,
) -> String {
    if !iw6_scripts_on() {
        asset_world::set_iw6_scripts(None);
        return "scripts: off (BO2MP_IW6_SCRIPTS=0)".to_owned();
    }
    let started = std::time::Instant::now();
    let dir = path.parent().unwrap_or(Path::new("."));
    let cache_dir = std::env::temp_dir();
    let mut pack = asset_world::Iw6ScriptPack { map: name.to_owned(), ..Default::default() };
    let mut notes: Vec<String> = Vec::new();
    fn add(
        pack: &mut asset_world::Iw6ScriptPack,
        notes: &mut Vec<String>,
        label: &str,
        z: fastfile_iw6::scripts::ZoneScripts,
    ) {
        notes.push(format!("{label} {}", z.scripts.len()));
        pack.zones.push(label.to_owned());
        for s in z.scripts {
            pack.scripts.push((s.name, s.stream, s.code));
        }
        for t in z.tables {
            pack.tables.push((t.name, t.columns, t.rows, t.cells));
        }
    }
    for z in SCRIPT_ZONES {
        let p = dir.join(format!("{z}.ff"));
        if !p.is_file() {
            continue;
        }
        let t = std::time::Instant::now();
        match fastfile_iw6::scripts::read_zone_cached(&p, &cache_dir) {
            Ok((scripts, cached)) => {
                let label = format!("{z}{} [{:.1}s]", if cached { " (cached)" } else { "" }, t.elapsed().as_secs_f32());
                add(&mut pack, &mut notes, &label, scripts);
            }
            Err(e) => notes.push(format!("{z}: {e}")),
        }
    }
    add(&mut pack, &mut notes, name, fastfile_iw6::scripts::from_capture(cap, &zone.image));
    let patch = dir.join(format!("patch_{name}.ff"));
    if patch.is_file() {
        match fastfile_iw6::scripts::read_zone(&patch) {
            Ok(scripts) => add(&mut pack, &mut notes, &format!("patch_{name}"), scripts),
            Err(e) => notes.push(format!("patch_{name}: {e}")),
        }
    }
    pack.entities = fastfile_iw6::scripts::raw_blocks(&map.entities);
    pack.exploders = logic
        .exploders
        .iter()
        .map(|e| asset_world::Iw6Exploder {
            id: e.id.clone(),
            fxid: e.fxid.clone(),
            effect: String::new(),
            origin: e.origin,
            angles: e.angles,
            delay: e.delay,
            soundalias: e.soundalias.clone(),
            loopsound: e.loopsound.clone(),
        })
        .collect();
    let reach = script_reach(&pack, name, cap, &zone.assets.script_strings);
    pack.models = reach.models;
    pack.pool_models = reach.pool_models;
    // The animations script models play and the skeletons of the multi-bone models.
    let (clips, bad) = fastfile_iw6::anim::clips(cap, &zone.image, &zone.assets.script_strings);
    pack.clips = clips;
    pack.skeletons = cap
        .xmodels
        .iter()
        .filter(|m| m.num_bones > 1 && !m.name.starts_with(','))
        .filter_map(|m| fastfile_iw6::anim::skeleton(m, &zone.assets.script_strings))
        .collect();
    if let Some(w) = &map.world {
        pack.brush_bounds = w.brush_models.iter().enumerate().skip(1).map(|(n, b)| (n as u32, b.mins, b.maxs)).collect();
    }
    // The English strings the scripts name (door hints): the localized zones beside the map's.
    for z in [format!("eng_{name}"), format!("eng_patch_{name}"), "eng_common_mp".into(), "eng_common_core_mp".into(), "eng_patch_common_mp".into()] {
        let p = dir.join("english").join(format!("{z}.ff"));
        if !p.is_file() {
            continue;
        }
        if let Ok(zone) = fastfile_iw6::Zone::open(&p) {
            let (cap, _) = fastfile_iw6::walk::walk_zone(&zone);
            pack.localized.extend(cap.localized);
        }
    }
    let anim_note = format!(
        "{} animations ({} refused by the decoder{}), {} skeletons, {} localized strings",
        pack.clips.len(),
        bad.len(),
        if bad.is_empty() { String::new() } else { format!(": {}", bad.iter().take(2).cloned().collect::<Vec<_>>().join("; ")) },
        pack.skeletons.len(),
        pack.localized.len()
    );
    pack.script_fx = reach.fx.into_iter().map(|p| (p, String::new())).collect();
    let line = format!(
        "scripts: {} ({} scripts, {} string tables, {} entity blocks, {} exploder placements) [{:.1}s]",
        notes.join(", "),
        pack.scripts.len(),
        pack.tables.len(),
        pack.entities.len(),
        pack.exploders.len(),
        started.elapsed().as_secs_f32()
    );
    let line = format!(
        "{line}; script reach: {} effect paths named, {} models the scripts may spawn or swap in, {} models with bounds, {anim_note}",
        pack.script_fx.len(),
        pack.pool_models.len(),
        pack.models.len()
    );
    asset_world::set_iw6_scripts(Some(pack));
    line
}

/// Effect paths scripts name: `fx/...` (the game's) and `vfx/...` (the maps').
fn is_fx_path(s: &str) -> bool {
    let l = s.to_ascii_lowercase();
    l.starts_with("fx/") || l.starts_with("vfx/")
}

/// What the scripts reachable from the map's `main` (and every function of the map's own
/// scripts) name: effect paths, models, and the map zone's models with their bounds and tags.
struct Reach {
    fx: Vec<String>,
    pool_models: Vec<String>,
    models: Vec<asset_world::Iw6Model>,
}

fn script_reach(
    pack: &asset_world::Iw6ScriptPack,
    map: &str,
    cap: &fastfile_iw6::walk::Capture,
    strings: &[Option<String>],
) -> Reach {
    use gsc_iw6_vm::{file_name, FuncRef, Name, ScriptSet};
    let mut set = ScriptSet::new();
    for (n, stream, code) in &pack.scripts {
        let _ = set.add_bytes(file_name(n), stream, code);
    }
    let mut entries: Vec<FuncRef> = Vec::new();
    if let Some(m) = set.resolve(&Name::Text(format!("maps/mp/{map}")), &Name::Id(20445)) {
        entries.push(m);
    }
    // The map's own scripts: every function (callbacks are reached through pointers).
    for (i, p) in set.iter() {
        let n = p.name.to_string();
        if n.starts_with(&format!("maps/mp/{map}")) || n.starts_with(&format!("maps/createfx/{map}")) {
            for f in 0..p.script.functions.len() {
                entries.push(FuncRef { script: i, func: f as u32 });
            }
        }
    }
    let reach = gsc_iw6_vm::reachable(&set, &entries);
    let lits = gsc_iw6_vm::string_literals(&set, Some(&reach));
    let mut fx: Vec<String> = lits
        .iter()
        .filter(|l| is_fx_path(l))
        .map(|l| l.to_ascii_lowercase())
        .collect();
    fx.sort();
    fx.dedup();
    let mut models = Vec::new();
    let mut pool: Vec<String> = Vec::new();
    for xm in &cap.xmodels {
        let name = xm.name.trim_start_matches(',').to_ascii_lowercase();
        if name.is_empty() {
            continue;
        }
        let tags = xm
            .bone_names
            .iter()
            .zip(&xm.base_mat)
            .filter_map(|(&ix, m)| {
                let n = strings.get(ix as usize)?.as_ref()?.to_ascii_lowercase();
                Some((n, [m[4], m[5], m[6]]))
            })
            .collect();
        if lits.iter().any(|l| l.eq_ignore_ascii_case(&name)) && !pool.contains(&name) {
            pool.push(name.clone());
        }
        models.push(asset_world::Iw6Model { name, mins: xm.bounds.0, maxs: xm.bounds.1, tags });
    }
    Reach { fx, pool_models: pool, models }
}

pub(crate) fn load_iw6_world_for(
    path: &Path,
    progress: &LoadProgress,
    material_seed: MaterialCatalog,
    native: bool,
) -> LoadedWorld {
    let mut report = vec![format!("game: IW6 / Ghosts ({})", path.display())];
    let stage = progress.begin_scoped(StageId::MapAssets, "ghosts zone", None);
    let started = std::time::Instant::now();
    let read = std::panic::catch_unwind(|| -> Result<_, String> {
        let zone = fastfile_iw6::Zone::open(path).map_err(|e| e.to_string())?;
        let (cap, stop) = fastfile_iw6::walk::walk_zone(&zone);
        Ok((zone, cap, stop))
    });
    let (zone, cap, stop) = match read {
        Ok(Ok(z)) => {
            stage.done();
            z
        }
        Ok(Err(error)) => {
            stage.fail();
            diag::warn!(World, "bo2mp iw6: {}: {error}", path.display());
            return LoadedWorld::with_gap(
                WorldDrawPolicy::t5(),
                PreparedCapability::PreparedWorld,
                format!("Ghosts zone: {error}"),
                Some("assets::lane::iw6_map::load_world/zone"),
            );
        }
        Err(panic) => {
            stage.fail();
            let what = panic
                .downcast_ref::<&str>()
                .map(|s| (*s).to_owned())
                .or_else(|| panic.downcast_ref::<String>().cloned())
                .unwrap_or_else(|| "unknown error".to_owned());
            diag::warn!(
                World,
                "bo2mp iw6: {}: reader panicked: {what}",
                path.display()
            );
            return LoadedWorld::with_gap(
                WorldDrawPolicy::t5(),
                PreparedCapability::PreparedWorld,
                format!("Ghosts zone could not be read: {what}"),
                Some("assets::lane::iw6_map::load_world/panic"),
            );
        }
    };
    report.push(format!(
        "iw6 zone: walked {} of {} assets [{:.1}s]{}",
        cap.walked,
        zone.assets.assets.len(),
        started.elapsed().as_secs_f32(),
        stop.as_ref()
            .map(|s| format!(
                ", stopped at {} ({:?}): {}",
                s.asset_index, s.asset_type, s.error
            ))
            .unwrap_or_default()
    ));
    let map = fastfile_iw6::map::read_map(&cap, &zone.image, &zone.assets.script_strings);
    report.extend(map.notes.iter().cloned());
    report.push(iw6_map_logic(path, &zone, &cap, &map));
    let mut loaded = build_iw6_world(path, &zone, &cap, &map, report, progress, material_seed, native);
    attach_map_sound(&mut loaded, path, &zone, &cap);
    diag::info!(
        World,
        "bo2mp iw6: {} loaded: {}",
        path.display(),
        loaded
            .report
            .iter()
            .filter(|l| l.starts_with("iw6 ") || l.starts_with("world mesh"))
            .cloned()
            .collect::<Vec<_>>()
            .join("; ")
    );
    loaded
}

/// bo2mp: a Ghosts map's own sounds (`BO2MP_IW6_MAP_SOUNDS`, on): every
/// alias list of the map zone under the Ghosts namespace, and its ambient
/// bed (`ambient_<map>`) as the map's ambient loop. One log line per map
/// with the Ghosts sound switches.
fn attach_map_sound(loaded: &mut LoadedWorld, path: &Path, zone: &fastfile_iw6::Zone, cap: &fastfile_iw6::walk::Capture) {
    let switches = asset_audio::iw6_rules::Iw6Switches::get();
    let stem = path.file_stem().map(|s| s.to_string_lossy().to_ascii_lowercase()).unwrap_or_default();
    if !switches.map_sounds {
        diag::info!(Audio, "iw6 sound: {stem}: the map's own sounds are off (BO2MP_IW6_MAP_SOUNDS=0); {}", switches.summary());
        return;
    }
    let catalog = asset_audio::ghosts_map_catalog(path, zone, cap);
    let ambient = asset_audio::ghosts_ambient_alias(cap, &stem);
    let tables = asset_audio::iw6_rules::map_zones().map_or((0, 0), |z| (z.zones.len(), z.reverbs.len()));
    let line = format!(
        "iw6 sound: {stem}: {} alias lists, {} loaded clips, {} curves of its own, ambient {}, sound tables: {} zones and {} reverbs (the map's and the localized zones', `iw6 sound tables:`); {}",
        catalog.sounds.len(),
        catalog.loaded.len(),
        catalog.curves.len(),
        ambient.as_deref().unwrap_or("none"),
        tables.0,
        tables.1,
        switches.summary()
    );
    diag::info!(Audio, "{line}");
    loaded.report.push(line);
    if let Some(alias) = ambient {
        loaded.facts.script_sound.ambient_alias = Some(format!("iw6:{alias}"));
    }
    loaded.sound = Some(match loaded.sound.take() {
        Some(Ok(mut other)) => {
            other.absorb_missing_aliases_unresolved(catalog);
            Ok(other)
        }
        _ => Ok(catalog),
    });
}

/// The spawn points in the map's entities (`mp_*_spawn*`).
pub(super) fn spawns_of(entities: &[iw3::ents::Entity]) -> Vec<asset_world::SpawnPoint> {
    entities
        .iter()
        .filter(|e| {
            let c = e.classname();
            c.starts_with("mp_") && c.contains("spawn") || c == "info_player_start"
        })
        .filter_map(|e| {
            Some(asset_world::SpawnPoint {
                classname: e.classname().to_owned(),
                origin: e.origin()?,
                angles: e.angles(),
                script_linkto: e.get("script_linkto").unwrap_or_default().to_owned(),
                script_destructable_area: e
                    .get("script_destructable_area")
                    .unwrap_or_default()
                    .to_owned(),
            })
        })
        .collect()
}

/// Ghosts' collision as CoD4's `clipMap_t` (what the IW3 clip builder reads).
fn iw3_clip(clip: &fastfile_iw6::map::Clip) -> iw3::zone::ClipMap {
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
                contents: b.contents,
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

/// Ghosts' HDR sun light (linear, its brightest channel) as CoD4's
/// gamma-space `sunlight`, under the exposure the map's lightmaps take
/// (`fastfile_iw6::map::light_exposure`), at most
/// `fastfile_iw6::map::SUN_GAMMA_MAX`.
fn sun_gamma(peak: f32, exposure: f32) -> f32 {
    fastfile_iw6::map::iw3_sun_gamma(peak, exposure)
}

/// Ghosts' world vertex colour (bytes R, G, B, A: a layered surface's
/// weights in green for layer 1 and blue for layer 2) as CoD4's D3DCOLOR
/// (bytes B, G, R, A), which the IW3 world builder reads. Read as it was,
/// layer 2's weight came from the red byte (near 255 everywhere: the second
/// layer covered every two-layer surface) and tints swapped red and blue.
pub(super) fn iw3_colour(rgba: u32) -> u32 {
    let [r, g, b, a] = rgba.to_le_bytes();
    u32::from_le_bytes([b, g, r, a])
}

/// Ghosts' drawn world as CoD4's `GfxWorld` (materials and lightmaps go
/// beside it, not in it).
fn iw3_world(
    w: &fastfile_iw6::map::World,
    sun: Option<fastfile_iw6::map::Sun>,
    exposure: f32,
) -> iw3::zone::GfxWorld {
    let range = |(a, b): (u32, u32)| a.min(b)..b;
    // Lit opaque, decal and transparent run on from one another.
    let lit = w.ranges[0].0.min(w.ranges[2].0)..w.ranges[0].1.max(w.ranges[2].1);
    // The sun's angles from its direction (toward the sun): pitch down.
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
            // (Ghosts' sun colour is HDR linear light, 9 on Prison Break,
            // its brightest channel the light and the rest the colour. The
            // fallback draw adds the sun in CoD4's gamma space beside the
            // lightmap's sqrt(light), both under one exposure: `sun_gamma`.)
            sun_light: sun_gamma(colour.1, exposure),
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

/// Ghosts' film keys left at their "unset" value (-1e8): the medium tint is
/// then the mean of the light and dark tints, the dark desaturation the
/// desaturation (the engine's vision loader).
fn film_unset_values(mut v: asset_world::FilmVision) -> asset_world::FilmVision {
    const UNSET: f32 = -1.0e7;
    if v.medium_tint.iter().any(|c| *c <= UNSET) {
        v.medium_tint = std::array::from_fn(|i| 0.5 * (v.light_tint[i] + v.dark_tint[i]));
    }
    if v.desaturation_dark <= UNSET {
        v.desaturation_dark = v.desaturation;
    }
    v
}

/// What the report says of the map's film vision. Ghosts' film constants
/// (bias `b + 0.5 - c / 2`, contrast, the light, medium and dark tints as a
/// quadratic Bezier, desaturation from its dark value to its full one) are
/// those `asset_world::iw_film_colour` draws, black level 0.
fn film_rule(film: Option<&asset_world::FilmVision>, look: bool) -> String {
    match (film, look) {
        (Some(f), _) if f.enable => format!(
            "the vision's (contrast {:.3}, brightness {:.3}, desaturation {:.3} / dark {:.3}, tints {:?} / {:?} / {:?}), the engine's maths",
            f.contrast, f.brightness, f.desaturation, f.desaturation_dark, f.light_tint, f.medium_tint, f.dark_tint
        ),
        (Some(_), _) => "the vision's is off (r_filmEnable 0)".to_owned(),
        (None, true) => "left out under Black Ops III's look (BO2MP_IW6_VISION=1 keeps it)".to_owned(),
        (None, false) => "none".to_owned(),
    }
}

/// The createart fog of a Ghosts map (`maps/createart/<map>_fog.gsc`): its
/// first `create_vision_set_fog` block's `ent.<field> = value;` lines.
pub(super) fn ghosts_fog(script: &str) -> Option<super::cod4::LegacyFog> {
    ghosts_fog_block(script.split("create_vision_set_fog").nth(1)?).map(|f| f.fog)
}

/// A fog block with what `ghosts_fog` leaves out: the block's vision name
/// and the HDR intensities of its colour and its sun colour (1 when unset).
pub(super) struct GhostsFog {
    pub name: String,
    pub fog: super::cod4::LegacyFog,
    pub colour_intensity: f32,
    pub sun_intensity: f32,
}

/// The fog by the engine's rules (`BO2MP_IW6_FOG_HDR`, on by default): the HDR block
/// (`<map>_hdr`) when the script has one, else its first.
pub(super) fn ghosts_fog_hdr(script: &str) -> Option<GhostsFog> {
    let blocks: Vec<GhostsFog> = script.split("create_vision_set_fog").skip(1).filter_map(ghosts_fog_block).collect();
    let hdr = blocks.iter().position(|b| b.name.to_ascii_lowercase().ends_with("_hdr"));
    let mut blocks = blocks;
    match hdr {
        Some(i) => Some(blocks.swap_remove(i)),
        None => blocks.into_iter().next(),
    }
}

/// The fog row's switch value (w) that has the world shader hold the fog's
/// transmittance at its floor (`fog_amount` in the shader) instead of scaling
/// the fog by its opacity; 1 is Black Ops II's law, 0 no fog.
pub(super) const FOG_LAW_GHOSTS: f32 = 2.0;

/// A colour from its stored (sRGB) value to linear, by the exact curve.
pub(super) fn srgb_to_linear(c: f32) -> f32 {
    let c = c.clamp(0.0, 1.0);
    if c <= 0.03928 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) }
}

/// The fog row of the light table (`cod4::legacy_fog_table`) with the HDR
/// fog's colours as the engine makes them: the stored colour to linear by
/// the sRGB curve, times its HDR intensity, times the map's light scale
/// `scale` (the fog is mixed in before the exposure, in the scene's light).
pub(super) fn hdr_fog_colours(row: &mut [f32; 16], f: &GhostsFog, scale: f32) {
    for ch in 0..3 {
        row[4 + ch] = srgb_to_linear(f.fog.colour[ch]) * f.colour_intensity * scale;
    }
    if let Some(sun) = f.fog.sun.as_ref() {
        for ch in 0..3 {
            row[8 + ch] = srgb_to_linear(sun.color_rgb[ch]) * f.sun_intensity * scale;
        }
    } else {
        let c = [row[4], row[5], row[6]];
        row[8..11].copy_from_slice(&c);
    }
}

fn ghosts_fog_block(block: &str) -> Option<GhostsFog> {
    let first = block;
    let name = first.split('"').nth(1).unwrap_or("").trim().to_owned();
    let mut num: HashMap<String, f32> = HashMap::new();
    let mut vec3: HashMap<String, [f32; 3]> = HashMap::new();
    for line in first.lines() {
        let Some((key, value)) = line
            .trim()
            .strip_prefix("ent.")
            .and_then(|l| l.split_once('='))
        else {
            continue;
        };
        let key = key.trim().to_ascii_lowercase();
        let value = value.trim().trim_end_matches(';').trim();
        if let Some(inner) = value.strip_prefix('(').and_then(|v| v.strip_suffix(')')) {
            let v: Vec<f32> = inner
                .split(',')
                .filter_map(|x| x.trim().parse().ok())
                .collect();
            if v.len() == 3 {
                vec3.insert(key, [v[0], v[1], v[2]]);
            }
        } else if let Ok(v) = value.parse::<f32>() {
            num.insert(key, v);
        }
    }
    let n = |k: &str| num.get(k).copied();
    let half = n("halfwaydist")?;
    if half <= 0.0 {
        return None;
    }
    let colour = [n("red")?, n("green")?, n("blue")?].map(|c| c.clamp(0.0, 1.0));
    let sun = (n("sunfogenabled").unwrap_or(0.0) > 0.0)
        .then(|| {
            Some(asset_world::SunFog {
                color_rgb: [n("sunred")?, n("sungreen")?, n("sunblue")?].map(|c| c.clamp(0.0, 1.0)),
                sun_dir: *vec3.get("sundir")?,
                begin_angle_deg: n("sunbeginfadeangle").unwrap_or(0.0),
                end_angle_deg: n("sunendfadeangle").unwrap_or(90.0),
                scale: n("normalfogscale").unwrap_or(1.0),
            })
        })
        .flatten();
    let intensity = |k: &str| n(k).filter(|v| v.is_finite() && *v >= 0.0).unwrap_or(1.0);
    Some(GhostsFog {
        name,
        fog: super::cod4::LegacyFog {
            start: n("startdist").unwrap_or(0.0).max(0.0),
            half,
            opacity: n("maxopacity").unwrap_or(1.0).clamp(0.0, 1.0),
            colour,
            sun,
            base_height: 1.0e7,
            half_height: 1.0e9,
        },
        colour_intensity: intensity("hdrcolorintensity"),
        sun_intensity: intensity("hdrsuncolorintensity"),
    })
}

/// Whether the map's own path data is used for the bots (`BO2MP_IW6_PATHDATA=0` turns it off).
fn pathdata_on() -> bool {
    !std::env::var("BO2MP_IW6_PATHDATA").is_ok_and(|v| matches!(v.trim(), "0" | "off" | "false"))
}

/// The nodes of `<dir>/<map>_path.ff` as the bots read them, when the zone is there and agrees with
/// the map's entities (the same node count, the same types and places in the same order).
pub(crate) fn path_data_nodes(
    dir: &Path,
    map_name: &str,
    entities: &fastfile_iw6::pathnode::PathNodes,
    report: &mut Vec<String>,
) -> Option<Vec<asset_world::Iw6PathNode>> {
    if !dir.join(format!("{map_name}_path.ff")).is_file() {
        report.push(format!("iw6 path data: {map_name}_path.ff is not installed; links rebuilt from the collision"));
        return None;
    }
    let data = match fastfile_iw6::pathdata::PathData::open_map(dir, map_name) {
        Ok(d) => d,
        Err(e) => {
            report.push(format!("iw6 path data: {e}"));
            return None;
        }
    };
    if let Err(e) = data.check() {
        report.push(format!("iw6 path data: {e}; not used"));
        return None;
    }
    let n = entities.nodes.len();
    let agrees = data.tree_nodes() == n
        && data.nodes.len() >= n
        && entities.nodes.iter().zip(&data.nodes).all(|(e, d)| e.ty == d.ty && (0..2).all(|k| (e.origin[k] - d.origin[k]).abs() < 0.5));
    if !agrees {
        report.push(format!("iw6 path data: {} nodes in the tree against {n} entities; not used", data.tree_nodes()));
        return None;
    }
    // (The nodes past the entities' are the AI zones' pseudo nodes: not places to stand.)
    let nodes: Vec<asset_world::Iw6PathNode> = data.nodes[..n]
        .iter()
        .map(|d| {
            let links: Vec<asset_world::Iw6PathLink> = d
                .links
                .iter()
                .filter(|l| usize::from(l.to) < n && l.disconnect_count == 0)
                .map(|l| asset_world::Iw6PathLink { to: u32::from(l.to), cost: l.dist, negotiation: l.negotiation })
                .collect();
            let end = links.iter().find(|l| l.negotiation && matches!(d.ty.0, 17 | 27)).map(|l| l.to);
            asset_world::Iw6PathNode { ty: d.ty.0, origin: d.origin, yaw: d.yaw, negotiation_end: end, animscript: d.animscript.clone(), links }
        })
        .collect();
    let links: usize = nodes.iter().map(|d| d.links.len()).sum();
    report.push(format!("iw6 path data: {map_name}_path.ff: {n} nodes, {links} links ({} traversals)", nodes.iter().flat_map(|d| &d.links).filter(|l| l.negotiation).count()));
    Some(nodes)
}

fn build_iw6_world(
    path: &Path,
    zone: &fastfile_iw6::Zone,
    cap: &fastfile_iw6::walk::Capture,
    map: &IwMap,
    mut report: Vec<String>,
    progress: &LoadProgress,
    material_seed: MaterialCatalog,
    native: bool,
) -> LoadedWorld {
    super::bo2_look::set_pending_grade(None);
    super::iw6_grid::set_pending(None);
    let map_name = path
        .file_stem()
        .map_or_else(String::new, |s| s.to_string_lossy().into_owned());
    let lower = map_name.to_ascii_lowercase();
    // What of the engine's own lighting rules this map was given, for one
    // line of the report (`iw6 engine rules:`); each part names its switch.
    let mut rules: Vec<String> = Vec::new();
    // The map's own light grid (the palette and tree format), for what has
    // no baked light (`iw6_grid`).
    let light_grid = super::iw6_grid::grid_of(cap, &zone.image);
    let grid_k = super::iw6_grid::scale();

    // Collision.
    let collision = match map
        .clip
        .as_ref()
        .map(|c| asset_world::world_iw3::build_iw3_clip_collision(&iw3_clip(c)))
    {
        Some(Ok(mut clip)) => {
            // Each partition's vertex segment (CoD4's have none): its
            // triangles index the vertices from `segment * 1024`.
            if let Some(src) = &map.clip {
                let mesh = std::sync::Arc::make_mut(&mut clip.mesh);
                for (p, &(_, _, segment)) in mesh.partitions.iter_mut().zip(&src.partitions) {
                    p.first_vert_segment = segment.into();
                }
                if super::iw6_model_collision::enabled() {
                    let models = super::iw6_model_collision::model_collision(cap, &zone.image, src);
                    report.push(format!(
                        "iw6 model collision: {} placed models for line traces (bullets, sight), {} triangles; {} with no collision LOD, {} skipped (no model), {} rigid lists without a collision surface (BO2MP_IW6_MODEL_COLLISION=0 leaves them out)",
                        models.models.len(),
                        models.tris,
                        models.no_lod,
                        models.skipped,
                        models.unpaired
                    ));
                    clip.static_models = models.models;
                    // The props that are not static models (clutter,
                    // destructibles, scriptables, script models) at their
                    // starting pose, after the static models.
                    if super::iw6_model_collision::props_enabled() {
                        let cut = fastfile_iw6::map::alternate_level_cut(&map.spawns);
                        let props = fastfile_iw6::map::extra_props(map, cap, cut);
                        let first = src.static_models.len() as u32;
                        let pc = super::iw6_model_collision::prop_collision(cap, &zone.image, &props, first);
                        report.push(format!(
                            "iw6 prop collision: {} of {} props for line traces, {} triangles; {} with no collision LOD (BO2MP_IW6_PROP_COLLISION=0 leaves them out)",
                            pc.models.len(),
                            props.len(),
                            pc.tris,
                            pc.no_lod
                        ));
                        clip.static_models.extend(pc.models);
                    }
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
                "iw6 clip: {} brushes ({} the world's), {} mesh tris in {} trees, {} models, {} trigger models; {census}",
                clip.brushes.len(),
                clip.cmodels.first().map_or(0, |m| m.num_brushes),
                clip.mesh.tri_indices.len() / 3,
                clip.mesh.aabb_roots.len(),
                clip.cmodels.len(),
                clip.trigger_models.len(),
            ));
            if census.spawns > 0 && census.on_ground * 2 < census.spawns {
                diag::warn!(
                    World,
                    "bo2mp iw6: {map_name}: most spawn points have no floor of their own in the collision ({census})"
                );
            }
            Some(clip)
        }
        Some(Err(error)) => {
            report.push(format!("iw6 clip gap: {error}"));
            None
        }
        None => {
            report.push("iw6 clip gap: zone has no clipMap".to_owned());
            None
        }
    };

    // Entities and spawns. Spawn records over nothing are left out: Ghosts
    // parks the spawns of modes a map does not play in an empty corner
    // (Prison Break's Capture the Flag and Search and Rescue spawns stand
    // over a void); Black Ops II's rules copy those modes' spawns from the
    // map's others instead.
    // A second level built above the first (Strikezone's after-the-strike
    // map): its spawn points, geometry and placed models are left out.
    let level_cut = fastfile_iw6::map::alternate_level_cut(&map.spawns);
    let mut spawn_records = map.spawns.clone();
    if let Some(cut) = level_cut {
        let before = spawn_records.len();
        spawn_records.retain(|s| s.origin[2] < cut);
        report.push(format!(
            "iw6 second level above z {cut:.0} left out: {} of {before} spawn records there",
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
    let entity_string = IwMap {
        spawns: spawn_records,
        entities: map.entities.clone(),
        ..Default::default()
    }
    .entity_string();
    let entities = iw3::ents::parse(&entity_string);
    // The map's path nodes for the bots: the `<map>_path.ff` zone's PathData (nodes seated on the
    // floor, with the engine's own links) when it is installed beside the map, else the `node_*`
    // entities (`fastfile_iw6::pathnode`) with links rebuilt from the collision.
    // `BO2MP_IW6_PATHDATA=0` keeps the entities.
    {
        let nodes = fastfile_iw6::pathnode::PathNodes::from_entities(&map.entities);
        let mut end_of = std::collections::HashMap::new();
        for n in &nodes.negotiations {
            end_of.insert(n.begin, n.end);
        }
        report.push(format!(
            "iw6 path nodes: {} ({} traversals, {} unpaired)",
            nodes.nodes.len(),
            nodes.negotiations.len(),
            nodes.dangling_begins.len()
        ));
        let from_entities: Vec<asset_world::Iw6PathNode> = nodes
            .nodes
            .iter()
            .map(|n| asset_world::Iw6PathNode {
                ty: n.ty.0,
                origin: n.origin,
                yaw: n.angles[1],
                negotiation_end: end_of.get(&n.index).copied(),
                animscript: n.animscript.clone(),
                links: Vec::new(),
            })
            .collect();
        let from_data = if pathdata_on() { path.parent().and_then(|dir| path_data_nodes(dir, &map_name, &nodes, &mut report)) } else { None };
        asset_world::set_iw6_path_nodes(Some(from_data.unwrap_or(from_entities)));
    }
    // Brush models their entities place as the map starts (doors,
    // elevators, walls, water sheets; `fastfile_iw6::map::placed_brush_models`).
    let placed_bmodels: Vec<(std::ops::Range<usize>, [f32; 3], [[f32; 3]; 3])> =
        fastfile_iw6::map::placed_brush_models(map, level_cut)
            .into_iter()
            .map(|b| (b.surfaces, b.origin, b.axis))
            .collect();
    let spawns = spawns_of(&entities);
    report.push(format!(
        "iw6 entities: {}, spawn points {} ({} spawn records, {parked} over nothing left out), {} trigger models",
        entities.len(),
        spawns.len(),
        map.spawns.len(),
        map.trigger_models.len()
    ));
    let mut scripts = crate::ScriptSources::default();
    scripts.set_entities(entity_string);
    // The compass frame (the two `minimap_corner` origins) and picture.
    let corners: Vec<[f32; 2]> = entities
        .iter()
        .filter(|e| e.get("targetname") == Some("minimap_corner"))
        .filter_map(|e| e.origin().map(|o| [o[0], o[1]]))
        .collect();
    let mut facts = crate::MapFacts::default();
    if let [a, b] = corners[..] {
        facts.minimap_corners = Some(asset_world::MinimapCorners { a, b });
    }
    let compass_name = format!("compass_map_{lower}");
    if cap
        .images
        .iter()
        .any(|i| i.name.eq_ignore_ascii_case(&compass_name))
    {
        facts.compass.material = Some(compass_name.clone());
        facts.compass.image = Some(compass_name);
    }
    report.push(format!(
        "iw6 compass: corners {}, picture {}",
        if facts.minimap_corners.is_some() {
            "found"
        } else {
            "missing"
        },
        facts.compass.image.as_deref().unwrap_or("missing")
    ));

    let Some(world) = map.world.as_ref() else {
        let mut loaded = LoadedWorld::with_gap(
            WorldDrawPolicy::t5(),
            PreparedCapability::PreparedWorld,
            "Ghosts zone has no GfxWorld",
            Some("assets::lane::iw6_map::load_world/no_world"),
        );
        loaded.scripts = scripts;
        loaded.collision = collision;
        loaded.spawns = spawns;
        loaded.report.extend(report);
        return loaded;
    };

    // Materials: world surfaces, then the placed models'.
    let stage = progress.begin_scoped(StageId::MapAssets, "ghosts materials", None);
    let max = texture_max();
    let mut materials = material_seed;
    materials.set_capture_ns(AssetNamespace::T6);
    let mut b = super::iw6::Builder::new(cap, zone, path, materials);
    b.shared = path.parent().and_then(shared_images);
    b.world_shine = look_on();
    // Surfaces not drawn: the sky's (the sky pass draws behind them), tool,
    // shadow-only and distortion (refraction) surfaces.
    let mut hidden = vec![false; world.surfaces.len()];
    let mut surface_materials = Vec::with_capacity(world.surfaces.len());
    let mut hidden_kinds: HashMap<&'static str, usize> = HashMap::new();
    // Materials of the decal range (`dpvs.litDecalSurfs`).
    let decal_materials: std::collections::HashSet<u32> = {
        let (a, e) = world.ranges[1];
        world.surfaces
            [(a.min(e) as usize).min(world.surfaces.len())..(e as usize).min(world.surfaces.len())]
            .iter()
            .filter_map(|s| s.material.map(|m| m.index))
            .collect()
    };
    // bo2mp: in a match Ghosts' own renderer (`render_iw6`) draws the map
    // and reads its own pictures, so the converted world keeps no surface,
    // no material and no placed model mesh; what stays is what the game's
    // other passes take from it: the light for the players and the gun,
    // collision, spawns, entities, scripts, sounds, the compass (as the
    // Modern Warfare Remastered and Black Ops III lanes do).
    // `BO2MP_IW6_CONVERTED_WORLD=1` builds it all anyway.
    let native = native && !std::env::var("BO2MP_IW6_CONVERTED_WORLD").is_ok_and(|v| v.trim() == "1");
    if native {
        report.push("iw6 world: drawn by the native renderer (render_iw6); the converted world keeps only its light".to_owned());
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
        let techset = cap
            .materials
            .get(m.index as usize)
            .and_then(|mr| mr.techset)
            .map(|t| cap.name_of(t).trim_start_matches(',').to_owned())
            .unwrap_or_default();
        let words: Vec<&str> = techset.split('_').collect();
        let kind = if level_cut.is_some_and(|cut| s.mins[2] > cut) {
            Some("second level")
        } else if words.iter().any(|w| w.starts_with("sky")) {
            Some("sky")
        } else if words.contains(&"tools") {
            Some("tools")
        } else if words.contains(&"shadowcaster") || words.contains(&"onlycast") {
            Some("shadow only")
        } else if words.contains(&"distortion") {
            Some("distortion")
        } else if words.contains(&"distfalloff") {
            // (`hdrportal_lighten`: an exposure portal fading with distance,
            // its colour map the engine's `$white`; drawn, a white sheet.)
            Some("exposure portal")
        } else {
            None
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
    // bo2mp: the sky: its cube under a sky material (the sky surface itself
    // is not drawn; the sky pass draws the cube behind the world).
    let sky_line = match world.sky_images.iter().flatten().next() {
        Some(_) if native => "iw6 sky: drawn by the native renderer".to_owned(),
        Some(id) => match b.sky_material(id.index as usize, max.max(1024)) {
            Ok(_) => format!("iw6 sky: {} (cube)", cap.name_of(*id)),
            Err(e) => format!("iw6 sky: {e}"),
        },
        None => "iw6 sky: none in the world".to_owned(),
    };
    report.push(sky_line);
    report.push(format!(
        "iw6 materials: {} world materials linked, {} pictures decoded (at most {max}px; {} from {:?} or built in), {} failed{}; not drawn: {:?}",
        b.materials.len(),
        b.decoded,
        b.from_shared,
        b.shared.as_ref().map(|s| s.zones.clone()).unwrap_or_default(),
        b.image_failures.len(),
        if b.image_failures.is_empty() {
            String::new()
        } else {
            format!(" (first: {})", b.image_failures.iter().take(4).cloned().collect::<Vec<_>>().join("; "))
        },
        hidden_kinds
    ));

    // The map's exposure (Ghosts tone-maps HDR light per map): from its
    // baked light, for the lightmaps, the sun and the models' ground light.
    let light_key = fastfile_iw6::map::lightmap_key(cap, &zone.image, &world.lightmaps);
    let exposure = fastfile_iw6::map::light_exposure(light_key);
    report.push(format!(
        "iw6 exposure: {exposure:.3} (baked light 90th percentile {})",
        light_key.map_or_else(|| "unmeasured".to_owned(), |k| format!("{k:.3}"))
    ));
    // Black Ops III's look: the tone curve is the exposure, so the map's own
    // (above) is not applied as well. The baked light goes in linear, scaled
    // so its shaded texels' mean is `LOOK_SHADE` (the level the look's sun
    // and ambient are set against, `h1_map`); measured over up to six of the
    // pages. Whether it holds the sun (then none is drawn over it).
    let sun_lum = map.sun.map_or(0.0, |s| 0.2126 * s.color[0] + 0.7152 * s.color[1] + 0.0722 * s.color[2]);
    let look = look_on().then(|| {
        let step = (world.lightmaps.len() / 6).max(1);
        let pages: Vec<([f64; 4], [f64; 2])> = world
            .lightmaps
            .iter()
            .step_by(step)
            .filter_map(|&pair| fastfile_iw6::map::iw3_lightmap_look(cap, &zone.image, pair, 1.0, true).ok())
            .map(|l| (l.split, l.split_b))
            .collect();
        let splits: Vec<[f64; 4]> = pages.iter().map(|p| p.0).collect();
        let light = fastfile_iw6::map::baked_light(&pages);
        let shade_a = if light.shaded_a > 0.0 { light.shaded_a } else { light_key.unwrap_or(0.3) * 0.5 };
        let ratio = fastfile_iw6::map::sun_in_lightmap(&splits);
        // Ghosts' sun is drawn live over a baked visibility mask; its pages
        // hold sky and bounce light only (mp_frag: sunlit texels at 2.0
        // against a sun of 8.0). The ratio alone (as Modern Warfare
        // Remastered's) read mp_frag's open snow as a baked sun, so it drew
        // none and every shadow was gone. `BO2MP_IW6_SUN_BAKED=1` / `0` force it.
        let by_ratio = ratio.is_some_and(|r| r >= SUN_IN_BAKE);
        let by_numbers = by_ratio && fastfile_iw6::map::sun_is_baked(&light, sun_lum);
        let sun_baked = match std::env::var("BO2MP_IW6_SUN_BAKED").ok().as_deref().map(str::trim) {
            Some("1") => true,
            Some("0") => false,
            _ => by_numbers,
        };
        // The scale: the baked light's whole shaded level (A + 0.8 B) to the
        // look's (`LOOK_SHADE_TOTAL`, set where `LOOK_SHADE` was on the upper
        // page alone: Prison Break reads the same). Measured on the upper
        // page only, a map whose directional page is bright (mp_frag: B is
        // five times A, Prison Break's two) came out several times too
        // bright, and its props with it. `BO2MP_IW6_SHADE_BASIS=a` returns
        // to the upper page.
        let total_basis = std::env::var("BO2MP_IW6_SHADE_BASIS").map_or(true, |v| v.trim() != "a");
        let shade_total = if light.shaded_total > 0.0 { light.shaded_total } else { shade_a * LOOK_SHADE_TOTAL / LOOK_SHADE };
        let (shade, target) = if total_basis {
            (shade_total, env_f("BO2MP_IW6_SHADE", LOOK_SHADE) * LOOK_SHADE_TOTAL / LOOK_SHADE)
        } else {
            (shade_a, env_f("BO2MP_IW6_SHADE", LOOK_SHADE))
        };
        let scale = (target / shade.max(1e-4)).clamp(0.01, 64.0);
        report.push(format!(
            "iw6 look: on (BO2MP_BO3_LOOK_IW6=0 turns it off): baked light's shaded level {shade_total:.3} (upper page {shade_a:.3}) and sunlit {:.3} over {} of {} pages, sunlit over shaded {}, scaled x{scale:.3} on the {} (the map's own exposure {exposure:.3} not applied); {} (sun {sun_lum:.2} in the same units{})",
            light.sunlit_total,
            splits.len(),
            world.lightmaps.len(),
            ratio.map_or_else(|| "unmeasured".to_owned(), |r| format!("{r:.2}")),
            if total_basis { "whole shaded level" } else { "upper page" },
            if sun_baked { "the sun is in the bake (none drawn over it)" } else { "the sun is not in the bake: drawn live over a sun map" },
            if by_ratio && !by_numbers { "; the sunlit/shaded ratio alone would have read it as baked, but the sunlit light is far under the sun's" } else { "" }
        ));
        (scale, sun_baked, shade_total)
    });
    let exposure = look.map_or(exposure, |(scale, ..)| scale);

    let mut model_k_for_summary = 1.0f32;
    // The placed models (rocks, trees, vehicles, furniture).
    let (prop_meshes, mut prop_placements) = {
        use bevy::prelude::{Mat3, Quat, Transform, Vec3};
        let mut meshes = Vec::new();
        let mut shared: HashMap<u32, Option<usize>> = HashMap::new();
        // The world's static models, then the props the map shows that are
        // not (dynamic entities, scriptables, script_model entities).
        let extras = fastfile_iw6::map::extra_props(map, cap, level_cut);
        let mut extra_counts = std::collections::BTreeMap::<&str, usize>::new();
        let mut all_models: Vec<fastfile_iw6::map::StaticModel> = world.static_models.clone();
        for p in &extras {
            *extra_counts.entry(p.source).or_default() += 1;
            all_models.push(fastfile_iw6::map::StaticModel {
                model: Some(fastfile_iw6::walk::AssetId {
                    ty: fastfile_iw6::AssetType::XModel,
                    index: p.model,
                }),
                origin: p.origin,
                axis: p.axis,
                scale: 1.0,
                cull_dist: f32::from(u16::MAX),
                flags: 0,
                ground_lighting: [0.0; 4],
                // (None stored: the nearest stored one is taken, `probe::NearestSamples`.)
                reflection_probe_index: u8::MAX,
            });
        }
        // Models the zone only names (`,weapon_uav_control_unit`: an empty
        // record) are the shared zones' (read once per process by name).
        let mut wanted: Vec<(u32, String)> = Vec::new();
        for sm in &all_models {
            let Some(id) = sm.model else { continue };
            if level_cut.is_some_and(|cut| sm.origin[2] > cut)
                || fastfile_iw6::map::model_has_geometry(cap, id.index)
                || wanted.iter().any(|w| w.0 == id.index)
            {
                continue;
            }
            if let Some(m) = cap.xmodels.get(id.index as usize) {
                let n = m.name.trim_start_matches(',').to_ascii_lowercase();
                if !n.is_empty() && !n.starts_with("tag_") && !n.starts_with("prop_flag") {
                    wanted.push((id.index, n));
                }
            }
        }
        let mut shared_found = 0usize;
        if !wanted.is_empty()
            && !native
            && let Some(dir) = path.parent()
        {
            let names: Vec<String> = wanted.iter().map(|w| w.1.clone()).collect();
            let found = shared_model_meshes(dir, &names, &mut b, max, &mut meshes);
            for (index, name) in &wanted {
                if let Some(&mesh) = found.get(name) {
                    shared.insert(*index, Some(mesh));
                    shared_found += 1;
                }
            }
            report.push(format!(
                "iw6 shared models: {shared_found} of {} models the zone only names found in the shared zones (missing: {:?})",
                wanted.len(),
                wanted
                    .iter()
                    .filter(|w| !found.contains_key(&w.1))
                    .map(|w| w.1.as_str())
                    .take(8)
                    .collect::<Vec<_>>()
            ));
        }
        let mut placements = vec![None; all_models.len()];
        let (mut unreadable, mut lit) = (Vec::new(), 0usize);
        // (Ghosts' ground light is linear, under the map's exposure as
        // its lightmaps; the IW3 grid's packing is sqrt(light / 32).)
        // A placed model's ground light is the light grid's reading over the
        // ground it stands on, the map's own scale: on mp_frag its median came
        // to 2.5 in the look's units against a traced ambient of 0.4 and a
        // baked shaded level of 1.1 (sun 2.2 drawn over it), so with the sun
        // live as well every prop (the crane, the bare tree, the container
        // stacks) drew near white. Where the sun is drawn live the models'
        // light is scaled so the median of them equals the world's baked
        // shaded level; a baked sun already is in the grid and stays as it was.
        // `BO2MP_IW6_MODEL` sets the factor by hand.
        let model_k = match look {
            None => 1.0,
            Some(_) if std::env::var_os("BO2MP_IW6_MODEL").is_some() => env_f("BO2MP_IW6_MODEL", LOOK_MODEL),
            Some((scale, sun_baked, shade_total)) if !sun_baked && scale > 0.0 => {
                let mut lums: Vec<f32> = all_models
                    .iter()
                    .filter(|sm| !level_cut.is_some_and(|cut| sm.origin[2] > cut))
                    .filter_map(|sm| {
                        let g = sm.ground_lighting;
                        let l = (g[0] + g[1] + g[2]) / 3.0 * scale;
                        (g.iter().all(|v| v.is_finite()) && l > 0.0).then_some(l)
                    })
                    .collect();
                lums.sort_by(f32::total_cmp);
                match lums.get(lums.len() / 2) {
                    Some(&median) => model_light_k(shade_total * scale, median),
                    None => 1.0,
                }
            }
            Some(_) => LOOK_MODEL,
        };
        model_k_for_summary = model_k;
        let ground_of = |sm: &fastfile_iw6::map::StaticModel| -> Option<[f32; 4]> {
            let g = sm.ground_lighting.map(|v| if v.is_finite() { v * exposure * model_k } else { 0.0 });
            (g[0] + g[1] + g[2] > 0.0).then_some(g)
        };
        // Black Ops II's props pass draws only a placement that carries a
        // light (`SmodelGpuPlan::t6_fallback_props`), and most of Ghosts'
        // models have none of their own (the light grid lights them; 84 %
        // of Prison Break's, every extra prop). The grid is a palette
        // bitstream the reader does not decode, so such a model takes the
        // light of the ground-lit models round it (`neighbour_light`; the
        // middle light of them all, a neutral one at the map's key with
        // none, where nothing is near). Under Black Ops III's look the
        // pass reads the real-time probes at each pixel as well.
        let lit_models: Vec<([f32; 3], [f32; 4])> = all_models
            .iter()
            .filter(|sm| !level_cut.is_some_and(|cut| sm.origin[2] > cut))
            .filter_map(|sm| Some((sm.origin, ground_of(sm)?)))
            .collect();
        let fallback = {
            let mut lums: Vec<[f32; 4]> = lit_models.iter().map(|l| l.1).collect();
            lums.sort_by(|a, b| (a[0] + a[1] + a[2]).total_cmp(&(b[0] + b[1] + b[2])));
            lums.get(lums.len() / 2).copied().unwrap_or([fastfile_iw6::map::EXPOSURE_KEY; 4])
        };
        let luminance = |g: [f32; 4]| (g[0] + g[1] + g[2]) / 3.0;
        // (For the report: the light of each placement without its own.)
        let mut spread_after = Vec::new();
        // The grid against the models' own stored light (report only), and
        // the grid's light for the models without their own when asked
        // (`BO2MP_IW6_GRID_PROPS=1`; the map's light scale, as its pages).
        let grid_props = super::iw6_grid::props_on() && light_grid.is_some();
        let mut props_from_grid = 0usize;
        if let Some(g) = light_grid.as_ref() {
            let stored: Vec<([f32; 3], [f32; 4])> = all_models
                .iter()
                .filter(|sm| sm.ground_lighting.iter().all(|v| v.is_finite()) && sm.ground_lighting[..3].iter().sum::<f32>() > 0.0)
                .map(|sm| (sm.origin, sm.ground_lighting))
                .collect();
            rules.push(match super::iw6_grid::calibration(g, &stored) {
                Some((ratio, n)) => format!("grid against the models' stored light: median ratio {ratio:.2} over {n} models"),
                None => "grid against the models' stored light: nothing to compare".to_owned(),
            });
        }
        let failures_before = b.image_failures.len();
        for (i, sm) in all_models.iter().enumerate() {
            let Some(id) = sm.model else { continue };
            if native {
                continue;
            }
            if level_cut.is_some_and(|cut| sm.origin[2] > cut) {
                continue;
            }
            let entry = *shared.entry(id.index).or_insert_with(|| {
                let model = cap.xmodels.get(id.index as usize)?;
                let Some(mesh) = model_mesh(&mut b, model, max) else {
                    if unreadable.len() < 8 {
                        unreadable.push(model.name.clone());
                    }
                    return None;
                };
                meshes.push(mesh);
                Some(meshes.len() - 1)
            });
            let Some(mesh) = entry else { continue };
            let own = ground_of(sm);
            lit += usize::from(own.is_some());
            let from_grid = (own.is_none() && grid_props)
                .then(|| light_grid.as_ref().and_then(|lg| super::iw6_grid::model_light(lg, sm.origin)))
                .flatten()
                .map(|m| {
                    let k = exposure * grid_k;
                    [m[0] * k, m[1] * k, m[2] * k, 0.0]
                });
            props_from_grid += usize::from(from_grid.is_some());
            let g = own.or(from_grid).unwrap_or_else(|| neighbour_light(sm.origin, &lit_models).unwrap_or(fallback));
            if own.is_none() {
                spread_after.push(luminance(g));
            }
            let p = |v: f32| ((v.max(0.0) / 32.0).sqrt() * 255.0).round().clamp(1.0, 255.0) as u8;
            let packed_lighting = Some([p(g[0]), p(g[1]), p(g[2]), 0]);
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
                packed_lighting,
                t6_primary: [0, 0],
            });
        }
        {
            use std::sync::atomic::Ordering::Relaxed;
            report.push(format!(
                "iw6 model surfaces the mesh builder left out: {} surfaces of {} models ({} models left with none)",
                DROPPED_SURFACES.swap(0, Relaxed),
                DROPPED_MODELS.swap(0, Relaxed),
                EMPTY_MODELS.swap(0, Relaxed)
            ));
        }
        report.push(format!(
            "iw6 static models: {} placed of {} (the world's {} and {:?} more props), {} distinct models built ({} unreadable {:?}), {} with their own ground light (the rest the middle one), {} more picture failures",
            placements.iter().filter(|p| p.is_some()).count(),
            all_models.len(),
            world.static_models.len(),
            extra_counts,
            meshes.len(),
            unreadable.len(),
            unreadable,
            lit,
            b.image_failures.len() - failures_before
        ));
        rules.push(if grid_props {
            format!("placed models without their own light: {props_from_grid} from the grid x{:.2} (BO2MP_IW6_GRID_PROPS=1), the rest their neighbours'", exposure * grid_k)
        } else if light_grid.is_some() {
            "placed models without their own light: their neighbours' (BO2MP_IW6_GRID_PROPS=1 takes the grid's)".to_owned()
        } else {
            "placed models without their own light: their neighbours' (no grid in the palette and tree format)".to_owned()
        });
        spread_after.sort_by(f32::total_cmp);
        let at = |q: usize| spread_after.get(spread_after.len().saturating_sub(1) * q / 100).copied().unwrap_or(0.0);
        report.push(format!(
            "iw6 model light of the {} placements with none of their own (mean of rgb, the look's units): before all {:.3}; now min {:.3} / p10 {:.3} / median {:.3} / p90 {:.3} / max {:.3}",
            spread_after.len(),
            luminance(fallback),
            at(0),
            at(10),
            at(50),
            at(90),
            at(100)
        ));
        (meshes, placements)
    };
    // The foliage's draw, counted: cut-outs, and the two-sided ones, among
    // the model materials built.
    {
        let (mut atest, mut two_sided, mut lit_cutout) = (0usize, 0usize, 0usize);
        for &c in b.materials.values() {
            let Some(d) = b.catalog.materials.get(c).and_then(|m| m.t6_draw.as_ref()) else { continue };
            atest += usize::from(d.alpha_test);
            two_sided += usize::from(d.alpha_test && d.cull == asset_core::T6Cull::None);
            lit_cutout += usize::from(d.alpha_test && !d.unlit);
        }
        report.push(format!(
            "iw6 model materials: {atest} alpha tested ({two_sided} two-sided, {lit_cutout} lit), {} cast no shadow (`nocast`)",
            b.nocast.len()
        ));
    }
    let mut material_tints: Vec<(usize, [f32; 3])> = Vec::new();
    let mut subst_summary = String::from("swaps not run");
    // bo2mp: Black Ops III's own textures for the materials it has a good
    // equivalent of (`bo3_subst`), under its look (the normal and specular
    // maps it brings are the lit branch's forms).
    if look.is_some() {
        let mut mats: Vec<(String, usize)> = b
            .materials
            .iter()
            .filter(|(k, _)| **k < (1usize << 41))
            .filter_map(|(k, slot)| Some((cap.materials.get(*k & ((1usize << 40) - 1))?.name.trim_start_matches(',').to_owned(), *slot)))
            .collect();
        mats.sort();
        // The collision's surface type of each material (Ghosts' list read
        // in Black Ops II's): evidence of what a surface is beside its name.
        let surfaces: std::collections::HashMap<String, u32> = map
            .clip
            .as_ref()
            .map(|c| {
                c.materials
                    .iter()
                    .map(|(n, flags, _)| (super::bo3_subst::surface_key(n), (asset_world::iw6_surface_flags_to_t6(*flags as u32) >> 20) & 0x1f))
                    .collect()
            })
            .unwrap_or_default();
        (material_tints, subst_summary) = super::bo3_subst::run("iw6", &mut b.catalog, &mats, &surfaces, max, &mut report);
    } else {
        report.push("iw6 material substitution: off (the Black Ops III look is off)".to_owned());
    }
    let nocast = std::mem::take(&mut b.nocast);
    // bo2mp: the map's createfx (waterfalls, mist, fire, dust), its sprites
    // linked into the same material catalog.
    super::iw6_fx::prepare(&mut b, cap, &zone.image, &mut report);
    let mut materials = std::mem::take(&mut b.catalog);
    drop(b);
    stage.done();
    // bo2mp: Black Ops III's look needs a reflection probe for the lit
    // branch's normal and specular maps to be drawn at all (the draw pass
    // binds them only on a map with a probe cube array); Ghosts' own probes
    // are not read, so the map is lit by a probe of the sky it is traced
    // under (`iw6_look`).
    let fog_early = fastfile_iw6::map::raw_file(cap, &zone.image, &format!("maps/createart/{lower}_fog.gsc"))
        .and_then(|b| ghosts_fog(&String::from_utf8_lossy(&b)));
    let (sky_zenith, sky_horizon) = super::iw6_look::sky_colours(fog_early.as_ref().map(|f| f.colour));
    // bo2mp: the map's own reflection probes in its place
    // (`BO2MP_IW6_OWN_PROBES=1`, `iw6_look::link_map_probes`): world
    // surfaces and placed models keep the probe the zone gives them.
    let probe_set = fastfile_iw6::probe::read_probes(cap, &zone.image);
    let mut own_probes = None;
    let mut own_probe_gap = None;
    if look.is_some() && super::iw6_look::probe_on() && super::iw6_look::own_probes_on() {
        match super::iw6_look::link_map_probes(cap, &zone.image, &probe_set, &mut materials, asset_core::ZoneOwner::from_zone_path(path), &lower) {
            Ok(p) => own_probes = Some(p),
            Err(e) => own_probe_gap = Some(e),
        }
    }
    let sky_probe = if let Some(p) = &own_probes {
        report.push(p.line.clone());
        Some(p.table.clone())
    } else {
        (look.is_some() && super::iw6_look::probe_on()).then(|| {
            let (table, mean) = super::iw6_look::link_sky_probe(
                &mut materials,
                asset_core::ZoneOwner::from_zone_path(path),
                &lower,
                sky_zenith,
                sky_horizon,
            );
            report.push(format!(
                "iw6 probe: a sky probe ({}px, {} levels, mean 1; the sky's mean light {mean:.3}, zenith {sky_zenith:.2?}, horizon {sky_horizon:.2?}) as the map's one reflection probe: the normal and specular maps are drawn (BO2MP_IW6_PROBE=0 leaves it out){}",
                super::iw6_look::PROBE_SIZE,
                super::iw6_look::PROBE_LEVELS,
                own_probe_gap.as_ref().map_or_else(String::new, |e| format!("; the map's own probes not used: {e}"))
            ));
            table
        })
    };
    if let Some(p) = &own_probes {
        // Placed models: the world's keep their stored probe (they come
        // first in the placements); the extra props take the stored probe
        // of the nearest placed model or surface. What moves (players,
        // guns) the same, through the runtime's picker.
        let mut samples: Vec<([f32; 3], u8)> = world
            .static_models
            .iter()
            .filter(|m| usize::from(m.reflection_probe_index) < probe_set.probes.len())
            .map(|m| (m.origin, m.reflection_probe_index))
            .collect();
        samples.extend(
            world
                .surfaces
                .iter()
                .filter(|s| usize::from(s.reflection_probe_index) < probe_set.probes.len() && s.reflection_probe_index != 0)
                .map(|s| (std::array::from_fn(|k| (s.mins[k] + s.maxs[k]) * 0.5), s.reflection_probe_index)),
        );
        let near = fastfile_iw6::probe::NearestSamples::new(&samples, 256.0);
        let (mut stored, mut picked) = (0usize, 0usize);
        for (i, pl) in prop_placements.iter_mut().enumerate() {
            let Some(pl) = pl else { continue };
            let index = match world.static_models.get(i).map(|m| m.reflection_probe_index) {
                Some(k) if usize::from(k) < probe_set.probes.len() => {
                    stored += 1;
                    usize::from(k)
                }
                _ => {
                    picked += 1;
                    near.nearest(pl.origin).map_or(usize::MAX, usize::from)
                }
            };
            pl.reflection_probe_index = p.slot(index, &probe_set, pl.origin);
        }
        report.push(format!("iw6 probe placement: {stored} placed models with their stored probe, {picked} by the nearest stored one ({} samples)", samples.len()));
        asset_world::install_t6_probe_origins(p.origins.clone());
        let probes = p.clone();
        let set = probe_set.clone();
        asset_world::install_t6_probe_picker(Some(std::sync::Arc::new(move |at: [f32; 3]| {
            let index = near.nearest(at).map_or(usize::MAX, usize::from);
            Some(u32::from(probes.slot(index, &set, at)))
        })));
    } else {
        // (A picker an earlier Ghosts map set does not outlive it.)
        asset_world::install_t6_probe_picker(None);
    }

    // The map's film vision (`vision/<map>.vision` in the zone).
    let vision_name = format!("vision/{lower}.vision");
    let vision_keys = super::iw6_tone::Vision::parse(
        &fastfile_iw6::map::raw_file(cap, &zone.image, &vision_name).map(|b| String::from_utf8_lossy(&b).into_owned()).unwrap_or_default(),
    );
    let film_vision = match fastfile_iw6::map::raw_file(cap, &zone.image, &vision_name) {
        Some(bytes) => match asset_world::parse_film_vision_rawfile(&vision_name, &bytes, false).map(|v| v.map(film_unset_values)) {
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

    // (Black Ops III's look grades through its own tone curve; the map's
    // film vision, made for Ghosts' own tone mapping, over it double-graded.
    // `BO2MP_IW6_VISION=1` keeps it.)
    let film_vision = if look.is_some() && std::env::var("BO2MP_IW6_VISION").map_or(true, |v| v.trim() != "1") {
        if film_vision.is_some() {
            report.push("film vision: left out under Black Ops III's look".to_owned());
        }
        None
    } else {
        film_vision
    };

    // The baked light, in CoD4's form. `BO2MP_IW6_LM_DIR=1` weights the
    // directional page on flat surfaces as Ghosts' lit shaders do (the baked
    // direction's z, `fastfile_iw6::shading`), not CoD4's `rsqrt(1 + x^2 + y^2)`.
    let lm_dir = super::bo3_look::game_on("BO2MP_IW6_LM_DIR", false);
    if lm_dir {
        report.push("lightmaps: directional page weighted by the baked direction's z (BO2MP_IW6_LM_DIR=1)".to_owned());
    }
    let mut lightmap_gaps = Vec::new();
    let lightmaps: Vec<Option<asset_world::WorldLightmap>> = world
        .lightmaps
        .iter()
        .enumerate()
        .map(|(i, &pair)| {
            let names = (
                pair.0
                    .map(|id| cap.name_of(id).to_owned())
                    .unwrap_or_default(),
                pair.1
                    .map(|id| cap.name_of(id).to_owned())
                    .unwrap_or_default(),
            );
            let page =
                match fastfile_iw6::map::iw3_lightmap_look_dir(cap, &zone.image, pair, exposure, look.is_some(), lm_dir) {
                    Ok(page) => page,
                    Err(e) => {
                        lightmap_gaps.push(format!("page {i}: {e}"));
                        return None;
                    }
                };
            let (pw, ph, p) = &page.primary;
            let (sw, sh, s) = &page.secondary;
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
        "iw6 lightmaps: {}/{} pages converted{}",
        lightmaps.iter().filter(|p| p.is_some()).count(),
        lightmaps.len(),
        if lightmap_gaps.is_empty() {
            String::new()
        } else {
            format!(" ({})", lightmap_gaps.join("; "))
        }
    ));

    let mut iw3w = iw3_world(world, map.sun, exposure);
    // A swapped material's colour is toned toward the Ghosts one it replaces:
    // the vertex colour scales the texel (darkening only).
    if !material_tints.is_empty() {
        let tint: std::collections::HashMap<usize, [f32; 3]> = material_tints.iter().copied().collect();
        let mut done = vec![false; iw3w.vertices.len()];
        let mut touched = 0usize;
        for (i, s) in iw3w.surfaces.iter().enumerate() {
            let Some(t) = surface_materials.get(i).copied().flatten().and_then(|m| tint.get(&m)) else { continue };
            let first = s.first_vertex.max(0) as usize;
            for vi in first..(first + usize::from(s.vertex_count)).min(iw3w.vertices.len()) {
                if std::mem::replace(&mut done[vi], true) {
                    continue;
                }
                let [b0, g0, r0, a0] = iw3w.vertices[vi].color.to_le_bytes();
                let m = |c: u8, k: usize| (f32::from(c) * t[k]).round().clamp(0.0, 255.0) as u8;
                iw3w.vertices[vi].color = u32::from_le_bytes([m(b0, 2), m(g0, 1), m(r0, 0), a0]);
                touched += 1;
            }
        }
        report.push(format!("iw6 material tint: {} swapped materials, {touched} world vertices toned toward the Ghosts colour they replace (darkening only, floor {:.2})", tint.len(), super::bo3_subst::TINT_FLOOR));
    }
    if let Some(p) = &own_probes {
        // (The zone's probe number of each surface, as its slot.)
        for s in &mut iw3w.surfaces {
            let at = std::array::from_fn(|k| (s.bounds[0][k] + s.bounds[1][k]) * 0.5);
            s.reflection_probe_index = p.slot(usize::from(s.reflection_probe_index), &probe_set, at);
        }
    } else if sky_probe.is_some() {
        // (Ghosts' own probe numbers name probes that are not read.)
        for s in &mut iw3w.surfaces {
            s.reflection_probe_index = 0;
        }
    }
    // The brush models their entities place (doors, elevators, walls,
    // water sheets): drawn where the entity stands, as the map starts.
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
        // (A vertex is moved once, should two surfaces share it.)
        let mut moved = vec![false; iw3w.vertices.len()];
        for (range, origin, axis) in &placed_bmodels {
            for i in range.clone().filter(|&i| i < n) {
                if listed[i] || placed[i] {
                    continue;
                }
                placed[i] = true;
                let surf = &mut iw3w.surfaces[i];
                let first = surf.first_vertex.max(0) as usize;
                let verts =
                    first..(first + usize::from(surf.vertex_count)).min(iw3w.vertices.len());
                if verts.is_empty() {
                    continue;
                }
                let (mut lo, mut hi) = ([f32::MAX; 3], [f32::MIN; 3]);
                for (vi, v) in iw3w.vertices[verts.clone()].iter_mut().enumerate() {
                    if std::mem::replace(&mut moved[verts.start + vi], true) {
                        continue;
                    }
                    v.xyz = place_point(v.xyz, *origin, axis);
                    v.normal = iw3_unit_vec(rotate(asset_model::unpack_unit_vec(v.normal), axis));
                    v.tangent = iw3_unit_vec(rotate(asset_model::unpack_unit_vec(v.tangent), axis));
                }
                for v in &iw3w.vertices[verts] {
                    for k in 0..3 {
                        lo[k] = lo[k].min(v.xyz[k]);
                        hi[k] = hi[k].max(v.xyz[k]);
                    }
                }
                if lo[0] <= hi[0] {
                    surf.bounds = [lo, hi];
                }
            }
        }
        // Every surface is then listed; what was not (and is not placed)
        // stays undrawn.
        for i in 0..n {
            if !listed[i] && !placed[i] {
                hidden[i] = true;
            }
        }
        iw3w.lit_surfs = 0..n as u32;
        report.push(format!(
            "iw6 brush models: {} placed by their entities ({} surfaces)",
            placed_bmodels.len(),
            placed.iter().filter(|&&p| p).count()
        ));
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
        "iw6 sun: {} toward {dir:?} colour {colour:?}",
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
        sky: [0.0, 1.0, 0.0, 1.0],
        sky_parms: [1.0, 0.0],
        saturation: None,
        grade: None,
    };
    report.push(format!(
        "iw6 world: {} surfaces ({} not drawn), {} sky surfaces, ranges {:?}",
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
            let fog_name = format!("maps/createart/{lower}_fog.gsc");
            // The fog by the engine's rules (`BO2MP_IW6_FOG_HDR`, on by
            // default, `=0` off): the script's HDR block, its colours linear by the
            // sRGB curve times their HDR intensities and the map's light
            // scale (as the scene's light). Its distance law is the
            // engine's already: visibility halves every halfway distance
            // past the start (k = ln 2 / halfway, the shader's base-2 exp).
            let fog_hdr = super::bo3_look::game_on("BO2MP_IW6_FOG_HDR", true)
                .then(|| fastfile_iw6::map::raw_file(cap, &zone.image, &fog_name).and_then(|b| ghosts_fog_hdr(&String::from_utf8_lossy(&b))))
                .flatten();
            let fog = match &fog_hdr {
                Some(h) => Some(h.fog.clone()),
                None => fog_early,
            };
            // The engine's opacity law (`BO2MP_IW6_FOG_LAW`, on by default, `=0` off):
            // transmittance max(1 - max opacity, e^(-k (d - start))), where
            // Black Ops II's shader scales the fog by the opacity (nearer
            // fog then reads thinner than Ghosts draws it).
            let fog_law = super::bo3_look::game_on("BO2MP_IW6_FOG_LAW", true) && fog.is_some();
            rules.push(if fog_law {
                "fog opacity: held at its floor, the engine's law (BO2MP_IW6_FOG_LAW=0 scales it, as Black Ops II)".to_owned()
            } else {
                "fog opacity: scaled, Black Ops II's (BO2MP_IW6_FOG_LAW=0)".to_owned()
            });
            if let Some(table) = sky_probe {
                draw.t6_probes = table;
            }
            rules.push(match (&fog_hdr, &fog) {
                (Some(h), _) => format!(
                    "fog: the {} block, colours linear x{:.2} (HDR {:.2} x the map's scale {exposure:.2}), sun x{:.2} (BO2MP_IW6_FOG_HDR=0 takes the first block)",
                    h.name,
                    h.colour_intensity * exposure,
                    h.colour_intensity,
                    h.sun_intensity * exposure
                ),
                (None, Some(_)) => "fog: the first block, colours squared, unscaled (BO2MP_IW6_FOG_HDR=0, or no HDR block)".to_owned(),
                (None, None) => "fog: none".to_owned(),
            });
            match &fog {
                Some(f) => {
                    report.push(format!(
                        "iw6 fog: READY start={:.1} half={:.1} opacity={:.3} sun={}",
                        f.start,
                        f.half,
                        f.opacity,
                        f.sun.is_some()
                    ));
                    draw.t6_lights = super::cod4::legacy_fog_table(f);
                    if let Some(h) = &fog_hdr {
                        hdr_fog_colours(&mut draw.t6_lights[super::t6::FOG_SLOT], h, exposure);
                    }
                    if fog_law {
                        draw.t6_lights[super::t6::FOG_SLOT][3] = FOG_LAW_GHOSTS;
                    }
                }
                None => report.push(format!("iw6 fog: none ({fog_name} sets none)")),
            }
            // The exposure the map's iris settles at (its tone mapping asset),
            // for the native frame's effects (`native_hdr_row`).
            let mut settle_exposure: Option<f32> = None;
            if let Some((look_scale, sun_baked, shade_raw)) = look {
                let places: Vec<[f32; 3]> = spawns.iter().map(|s| s.origin).collect();
                // The sun drawn, in the baked light's units (none over a
                // bake that holds it); its light is the hue times that.
                // From the map's own numbers (`BO2MP_IW6_MAP_LIGHT=0` brings
                // the fixed constants back; `BO2MP_IW6_SUN` / `_EXPOSURE` set
                // either by hand): the sun's level from its HDR peak against
                // the baked light's shaded mean, the exposure solved from the
                // traced light.
                let map_light = super::bo3_look::game_on("BO2MP_IW6_MAP_LIGHT", true);
                let sun_peak = map.sun.map_or(0.0, |s| s.color.iter().copied().fold(0.0f32, f32::max));
                // The map's own tone mapping (its vision's `toneMapping`
                // asset: Hable's curve) as the targets of the sun and the
                // exposure (`iw6_tone`); `BO2MP_IW6_TONEMAP=bo3` keeps the
                // earlier rule.
                let tone_mode = super::iw6_tone::mode(super::iw6_tone::DEFAULT_MODE);
                let mut tone_curve = super::iw6_tone::curve_for(&vision_keys, &cap.tone_mappings, sun_peak + shade_raw, AUTO_GREY);
                // The exposure the map's iris settles at for a mid-grey in
                // its sun (`BO2MP_IW6_IRIS`, on by default): inside the asset's range when
                // it adapts, the middle of it when it does not; else the
                // range's first float, as before.
                let first_exposure = tone_curve.exposure;
                if super::iw6_tone::iris_on() {
                    tone_curve.exposure = tone_curve.iris_exposure(0.35 * (sun_peak + shade_raw));
                }
                settle_exposure = Some(tone_curve.exposure);
                rules.push(format!(
                    "exposure: {:.3} ({}; range {:.3}..{:.3}, rates {:?}, {})",
                    tone_curve.exposure,
                    if super::iw6_tone::iris_on() && tone_curve.from_zone {
                        format!("where the iris settles for a sunlit grey, BO2MP_IW6_IRIS=0 takes the first float; the first float {first_exposure:.3}")
                    } else if tone_curve.from_zone {
                        "the range's first float (BO2MP_IW6_IRIS=0)".to_owned()
                    } else {
                        "solved: no tone mapping asset".to_owned()
                    },
                    tone_curve.exposure_min,
                    tone_curve.exposure_max,
                    tone_curve.rates,
                    if tone_curve.adaptive() { "adapts" } else { "held" }
                ));
                // Each colour channel by its own curve (`BO2MP_IW6_TONE_RGB`, on by default):
                // the final pass's colour table moves each channel from the
                // one curve to its own (with the film vision after it, when
                // that is kept, `BO2MP_IW6_VISION=1`).
                let channels = super::iw6_tone::channels_for(&vision_keys, &cap.tone_mappings);
                match (&channels, super::iw6_tone::rgb_on()) {
                    (Some(ch), true) => {
                        let film = film_vision.as_ref().filter(|f| f.enable);
                        super::bo2_look::set_pending_grade(Some(asset_world::T6Grade {
                            lut: super::iw6_tone::channel_lut(&tone_curve, ch, tone_curve.exposure, film),
                            vision: asset_world::T6Vision::default(),
                        }));
                        let s = super::iw6_tone::channel_spread(&tone_curve, ch, tone_curve.exposure);
                        rules.push(format!(
                            "tone map per channel: a mid-grey of the one curve draws {:.3} / {:.3} / {:.3} (BO2MP_IW6_TONE_RGB=0 for one curve){}",
                            s[0],
                            s[1],
                            s[2],
                            if film.is_some() { ", the film vision after it" } else { "" }
                        ));
                    }
                    (Some(ch), false) => {
                        let s = super::iw6_tone::channel_spread(&tone_curve, ch, tone_curve.exposure);
                        rules.push(format!("tone map: one curve (BO2MP_IW6_TONE_RGB=0; per channel a mid-grey would draw {:.3} / {:.3} / {:.3})", s[0], s[1], s[2]));
                    }
                    (None, _) => rules.push("tone map: one curve (no tone mapping asset, so no channel of its own)".to_owned()),
                }
                let tone_solution = (tone_mode == super::iw6_tone::Mode::Hable && map_light && !sun_baked)
                    .then(|| super::iw6_tone::solve(&tone_curve, sun_peak, shade_raw, shade_raw * look_scale))
                    .flatten();
                let (sun_light, sun_source) = if sun_baked {
                    (0.0, "the sun is in the baked light".to_owned())
                } else if std::env::var_os("BO2MP_IW6_SUN").is_some() {
                    (env_f("BO2MP_IW6_SUN", LOOK_SUN), "BO2MP_IW6_SUN".to_owned())
                } else if let Some(s) = tone_solution.as_ref().filter(|_| sun_peak > 0.0) {
                    (
                        s.sun_light,
                        format!(
                            "the map's tone mapping: a mid-grey draws {:.2} in its sun and {:.2} in its shade under {}, the branch is set to match{}",
                            s.grey_lit,
                            s.grey_shade,
                            tone_curve.source,
                            if s.clamped { " (the sun held to 1.07..3.55 times the shade)" } else { "" }
                        ),
                    )
                } else if map_light && sun_peak > 0.0 {
                    let ratio = super::bo3_look::map_sun_ratio(sun_peak, shade_raw);
                    // (The shaded level as the pages are scaled to.)
                    let shade = shade_raw * look_scale;
                    (
                        ratio * shade,
                        format!("the map's: ComWorld sun peak {sun_peak:.2} over the baked shaded level {shade_raw:.3} gives {ratio:.2} x the scaled shaded level {shade:.2}"),
                    )
                } else {
                    (LOOK_SUN, if map_light { "fixed: the map has no sun record" } else { "fixed (BO2MP_IW6_MAP_LIGHT=0)" }.to_owned())
                };
                let grey_target = tone_solution.as_ref().map_or(AUTO_GREY, |s| s.grey_lit.clamp(0.5, 0.85));
                let auto_grey = (map_light && !sun_baked && std::env::var_os("BO2MP_IW6_EXPOSURE").is_none()).then_some(grey_target);
                // One line per map: what the vision and its tone mapping say
                // and what was done with it.
                report.push(format!(
                    "iw6 vision: {lower}: toneMapping {} ({}; mode {tone_mode:?}: {}); exposure {:.3}, white {:.2}, Hable A {:.3} B {:.3} C {:.3} D {:.3} E {:.3} F {:.3}; native mid-grey {:.2} in sun / {:.2} in shade; applied: grey target {grey_target:.2}, sun x{sun_light:.2}; read, not applied (meaning or double grade unverified): hdrSkyIntensity {}, litSurfaceHDRScalar {}, unlitSurfaceHDRScalar {}, film tints (light/medium/dark) {}, desaturation {}, glow bloom cutoff {} intensity {}, material bloom {}, colorizationSet {}",
                    vision_keys.tone_mapping.as_deref().unwrap_or("none"),
                    tone_curve.source,
                    if tone_solution.is_some() { "sun and exposure target set from it" } else { "earlier rule kept" },
                    tone_curve.exposure,
                    tone_curve.white,
                    tone_curve.a,
                    tone_curve.b,
                    tone_curve.c,
                    tone_curve.d,
                    tone_curve.e,
                    tone_curve.f,
                    tone_curve.display(0.35 * (sun_peak + shade_raw)),
                    tone_curve.display(0.35 * shade_raw),
                    vision_keys.hdr_sky_intensity.map_or("none".to_owned(), |v| format!("{v}")),
                    vision_keys.lit_surface_scalar.map_or("none".to_owned(), |v| format!("{v}")),
                    vision_keys.unlit_surface_scalar.map_or("none".to_owned(), |v| format!("{v}")),
                    vision_keys.film_tints.map_or("none".to_owned(), |t| format!("{t:?}")),
                    vision_keys.desaturation.map_or("none".to_owned(), |v| format!("{v}")),
                    vision_keys.bloom_cutoff.map_or("none".to_owned(), |v| format!("{v}")),
                    vision_keys.bloom_intensity.map_or("none".to_owned(), |v| format!("{v}")),
                    vision_keys.material_bloom_intensity.map_or("none".to_owned(), |v| format!("{v}")),
                    vision_keys.colorization_set.as_deref().unwrap_or("none"),
                ));
                let hue = [colour[0], colour[1], colour[2]];
                // The real-time ambient's sky: the fog's colour is the
                // horizon's (else a little bluer than the horizon), the
                // zenith a touch bluer still; its level is set against the
                // sun drawn, so the sun colour given is that light.
                let horizon = sky_horizon;
                let rt_options = super::rt_ambient::Options {
                    // (Not over a bake that holds the sun: its sky level is
                    // set against the sun drawn, here none, so the rebake
                    // came out near black and replaced 85 % of the baked
                    // light, sun and shadows with it. `BO2MP_IW6_RT_BAKED=1`
                    // restores that.)
                    sky: (!sun_baked || env_f("BO2MP_IW6_RT_BAKED", 0.0) > 0.5)
                        .then_some((sky_zenith, horizon)),
                    local: false,
                };
                super::bo3_look::apply_except(
                    &mut draw,
                    &materials,
                    &prop_meshes,
                    &prop_placements,
                    &places,
                    dir,
                    hue.map(|h| h * sun_light.max(0.05)),
                    Some(env_f("BO2MP_IW6_EXPOSURE", LOOK_EXPOSURE).max(0.01)),
                    auto_grey,
                    &rt_options,
                    &nocast,
                    &mut report,
                );
                if draw.t7_light {
                    // The rows `bo3_look` wrote, as this map's: the sun's
                    // light over its colour, and the pages' packing (CoD4's,
                    // linear: `t7_iw_pages` in the shader).
                    draw.t6_lights[asset_world::T7_LIGHT_ROW][1] = sun_light;
                    draw.t6_lights[asset_world::T7_LIGHT_ROW + 1][9] = 1.0;
                    // Ghosts' pictures are authored for its own tone mapping:
                    // paint, snow-covered steel and bare tree bark carry
                    // albedo near 1, which the Black Ops III curve draws as
                    // paper (the crane, the gun, the bare tree). Past 0.5 the
                    // albedo rolls off at a quarter, as Modern Warfare III's
                    // (`BO2MP_IW6_ROLLOFF=0` leaves it out).
                    let rolloff = super::bo3_look::game_on("BO2MP_IW6_ROLLOFF", true);
                    draw.t6_lights[asset_world::T7_LIGHT_ROW + 1][13] = if rolloff { 1.0 } else { 0.0 };
                    report.push(format!(
                        "iw6 light: sun x{sun_light:.2} ({sun_source}), exposure x{:.2} ({}), placed models x{:.2} of their ground light",
                        draw.t6_lights[asset_world::T7_LIGHT_ROW][0],
                        match (auto_grey, std::env::var_os("BO2MP_IW6_EXPOSURE")) {
                            (_, Some(_)) => "BO2MP_IW6_EXPOSURE".to_owned(),
                            (Some(g), None) => format!("solved from the traced light for a mid-grey in the sun at {g:.2}, else the fixed {LOOK_EXPOSURE}"),
                            (None, None) => format!("fixed: {}", if sun_baked { "the sun is in the baked light" } else { "BO2MP_IW6_MAP_LIGHT=0" }),
                        },
                        model_k_for_summary
                    ));
                    // One line per map with the numbers that decide how it looks.
                    report.push(format!(
                        "iw6 look summary: {lower}: baked light x{look_scale:.2}, sun {} x{sun_light:.2}, exposure x{:.2}, models x{:.2} of their ground light, albedo roll-off {}, ambient {}, materials: {subst_summary}",
                        if sun_baked { "baked" } else { "live" },
                        draw.t6_lights[asset_world::T7_LIGHT_ROW][0],
                        model_k_for_summary,
                        if rolloff { "on" } else { "off" },
                        if rt_options.sky.is_some() { "traced (sky probes over the baked light)" } else { "baked light only" },
                    ));
                } else {
                    report.push("iw6 look: not applied (bo3 look refused the light table)".to_owned());
                }
            }
            // Players, the gun, entities: the map's own grid in Black Ops
            // II's layout (`BO2MP_IW6_GRID`, on by default; `=0` the placed models'
            // light round them, as before).
            let iw6_light_grid = match (super::iw6_grid::moving_on(), light_grid.as_ref()) {
                (true, Some(g)) => {
                    let places: Vec<[f32; 3]> = spawns.iter().map(|s| s.origin).collect();
                    match super::iw6_grid::owned_grid(g, &places) {
                        Ok((grid, line)) => {
                            report.push(format!("iw6 light grid: {line}"));
                            super::iw6_grid::set_pending(Some((lower.clone(), exposure * grid_k)));
                            rules.push(format!("moving things: the grid x{:.2} (BO2MP_IW6_GRID=0 takes the placed models' light; theirs outside its box)", exposure * grid_k));
                            Some(grid)
                        }
                        Err(e) => {
                            rules.push(format!("moving things: the placed models' light round them (the grid could not be laid out: {e})"));
                            None
                        }
                    }
                }
                (true, None) => {
                    rules.push("moving things: the placed models' light round them (no grid in the palette and tree format)".to_owned());
                    None
                }
                (false, _) => {
                    rules.push("moving things: the placed models' light round them (BO2MP_IW6_GRID=0, or no grid)".to_owned());
                    None
                }
            };
            // The native frame drawn in stages: the gun, players and effects
            // into Ghosts' HDR scene, in Ghosts' units (`native_hdr_row`).
            if native {
                let row = native_hdr_row(map.sun.as_ref(), exposure, settle_exposure);
                if draw.t6_lights.len() <= NATIVE_HDR_ROW {
                    draw.t6_lights.resize(NATIVE_HDR_ROW + 1, [0.0; 16]);
                }
                draw.t6_lights[NATIVE_HDR_ROW] = row;
                rules.push(format!(
                    "native frame: moving things into Ghosts' HDR scene at x{:.3} of this lane's light (the grid in Ghosts' own units), the sun {:.2} / {:.2} / {:.2} (Ghosts' diffuse), effects x{:.3}",
                    row[7], row[4], row[5], row[6], row[11]
                ));
            }
            rules.push(format!("film: {}", film_rule(film_vision.as_ref(), look.is_some())));
            report.push(format!("iw6 engine rules: {lower}: {}", rules.join("; ")));
            report.push(format!(
                "world mesh: {} vertices, {} triangles, {} surfaces (Ghosts, fallback draw with colour maps{})",
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
                    light_grid: iw6_light_grid,
                    policy: WorldDrawPolicy::t5(),
                    static_model_meshes: prop_meshes,
                    static_model_instances: prop_placements,
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
                format!("Ghosts world mesh: {error}"),
                Some("assets::lane::iw6_map::load_world/world_mesh"),
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
mod tests {
    #[test]
    fn the_models_light_is_brought_to_the_worlds_shaded_level() {
        // mp_frag: median 2.53 over a shaded level of 1.13.
        assert!((super::model_light_k(1.126, 2.531) - 0.445).abs() < 0.01);
        // Already under the level: untouched; far over: floored.
        assert_eq!(super::model_light_k(1.126, 0.5), 1.0);
        assert_eq!(super::model_light_k(1.126, 100.0), super::MODEL_K_MIN);
        assert_eq!(super::model_light_k(1.126, 0.0), 1.0);
    }

    /// Dumps the zone's raw files (names, and the text of `GHOSTS_DUMP` matches).
    #[test]
    #[ignore]
    fn ghosts_dump_rawfiles() {
        let Ok(path) = std::env::var("GHOSTS_TEST_MAP") else { return };
        let pat = std::env::var("GHOSTS_DUMP").unwrap_or_default();
        let zone = fastfile_iw6::Zone::open(std::path::Path::new(&path)).unwrap();
        let (cap, _) = fastfile_iw6::walk::walk_zone(&zone);
        for t in cap.tone_mappings.values() {
            println!("TONE {} {:?}", t.name, t.v);
        }
        for r in &cap.raw_files {
            println!("RAW {} len {}", r.name, r.len);
            if !pat.is_empty() && r.name.contains(&pat) {
                if let Some(b) = fastfile_iw6::map::raw_file(&cap, &zone.image, &r.name) {
                    println!("-----\n{}\n-----", String::from_utf8_lossy(&b));
                }
            }
        }
    }

    /// Loads a staged Ghosts map and prints the look's report lines
    /// (`GHOSTS_TEST_MAP=<mp_x.ff> [BO2MP_BO3_LOOK_IW6=0] cargo test -p assets
    /// --lib ghosts_look_report -- --ignored --nocapture`).
    #[test]
    #[ignore]
    fn ghosts_look_report() {
        let Ok(path) = std::env::var("GHOSTS_TEST_MAP") else { return };
        let loaded = super::load_iw6_world(
            std::path::Path::new(&path),
            &asset_transport::progress::LoadProgress::default(),
            Default::default(),
        );
        // A fingerprint of what the look wrote (to compare a cold bake with a read one).
        if let Some(draw) = &loaded.world.draw {
            let mut h = 0xcbf2_9ce4_8422_2325u64;
            let mut feed = |b: &[u8]| b.iter().for_each(|&b| h = (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3));
            if let Some(lut) = &draw.t6_lut {
                feed(&lut.width.to_le_bytes());
                feed(&lut.rgba);
            }
            for row in [250usize, 252, 253, 254, 255] {
                for v in draw.t6_lights.get(row).into_iter().flatten() {
                    feed(&v.to_le_bytes());
                }
            }
            println!("rt fingerprint: {h:016x}");
        }
        // (The look's cache is written off the load's thread: let it finish.)
        std::thread::sleep(std::time::Duration::from_secs(4));
        for l in &loaded.report {
            if ["iw6 look", "iw6 vision", "iw6 engine rules", "iw6 light grid", "iw6 light", "iw6 probe", "iw6 sky", "iw6 tint", "iw6 material", "iw6 vfx", "iw6 createfx", "look check", "exposure:", "bo3 look", "rt ", "iw6 exposure", "iw6 fog", "iw6 sun", "film vision", "iw6 static", "iw6 model", "iw6 shared", "iw6 brush", "world mesh"].iter().any(|k| l.starts_with(k)) {
                println!("{l}");
            }
        }
    }

    #[test]
    fn axes_of_angles() {
        let a = fastfile_iw6::map::axis_of([0.0, 90.0, 0.0]);
        let close = |x: [f32; 3], y: [f32; 3]| x.iter().zip(y).all(|(a, b)| (a - b).abs() < 1e-5);
        assert!(
            close(a[0], [0.0, 1.0, 0.0])
                && close(a[1], [-1.0, 0.0, 0.0])
                && close(a[2], [0.0, 0.0, 1.0])
        );
        let p = super::place_point([10.0, 0.0, 5.0], [100.0, 0.0, 0.0], &a);
        assert!(close(p, [100.0, 10.0, 5.0]));
    }

    #[test]
    fn unit_vectors_pack_as_cod4() {
        for v in [[1.0, 0.0, 0.0], [0.0, -1.0, 0.0], [0.6, 0.0, 0.8]] {
            let back = asset_model::unpack_unit_vec(super::iw3_unit_vec(v));
            for k in 0..3 {
                assert!((back[k] - v[k]).abs() < 0.01, "{v:?} -> {back:?}");
            }
        }
    }

    #[test]
    fn unlit_props_take_their_neighbours_light() {
        let lit = [([0.0, 0.0, 0.0], [0.2; 4]), ([1000.0, 0.0, 0.0], [0.8; 4]), ([5000.0, 0.0, 0.0], [0.5; 4])];
        assert!(super::neighbour_light([0.0; 3], &[]).is_none());
        // On top of a lit model: its light; between two: in between; the
        // result stays within the lit ones' range.
        let on = super::neighbour_light([0.0; 3], &lit).unwrap();
        assert!((on[0] - 0.2).abs() < 0.05, "{on:?}");
        let mid = super::neighbour_light([500.0, 0.0, 0.0], &lit).unwrap();
        assert!(mid[0] > 0.3 && mid[0] < 0.7, "{mid:?}");
        let near_far = super::neighbour_light([950.0, 0.0, 0.0], &lit).unwrap();
        assert!(near_far[0] > mid[0], "{near_far:?}");
    }

    #[test]
    fn ghosts_fog_reads_its_first_block() {
        let script = "main()\n{\n\tent = maps\\mp\\_art::create_vision_set_fog( \"mp_x\" );\n\tent.startDist = 500;\n\tent.halfwayDist = 25000;\n\tent.red = 0.62;\n\tent.green = 0.78;\n\tent.blue = 0.86;\n\tent.maxOpacity = 0.50;\n\tent.sunFogEnabled = 1;\n\tent.sunRed = 0.55;\n\tent.sunGreen = 0.45;\n\tent.sunBlue = 0.31;\n\tent.sunDir = (0.99, 0.112215, -0.0120798);\n\tent.sunBeginFadeAngle = 10;\n\tent.sunEndFadeAngle = 92;\n\tent = maps\\mp\\_art::create_vision_set_fog( \"mp_x_hdr\" );\n\tent.startDist = 1;\n}";
        let f = super::ghosts_fog(script).expect("fog");
        assert_eq!((f.start, f.half, f.opacity), (500.0, 25000.0, 0.5));
        assert!(f.sun.is_some_and(|s| (s.sun_dir[0] - 0.99).abs() < 1e-6));
        // (The HDR block here sets no halfway distance: the first is kept.)
        let h = super::ghosts_fog_hdr(script).expect("fog");
        assert_eq!((h.name.as_str(), h.colour_intensity, h.sun_intensity), ("mp_x", 1.0, 1.0));
    }

    #[test]
    fn the_engines_fog_takes_the_hdr_block_linear_and_scaled() {
        let script = "ent = maps\\mp\\_art::create_vision_set_fog( \"mp_x\" );\n\tent.startDist = 500;\n\tent.halfwayDist = 25000;\n\tent.red = 0.5;\n\tent.green = 0.5;\n\tent.blue = 0.5;\n\tent.maxOpacity = 0.5;\n\tent = maps\\mp\\_art::create_vision_set_fog( \"mp_x_HDR\" );\n\tent.startDist = 300;\n\tent.halfwayDist = 12000;\n\tent.red = 0.5;\n\tent.green = 0.25;\n\tent.blue = 0.02;\n\tent.HDRColorIntensity = 2;\n\tent.maxOpacity = 0.8;\n\tent.sunFogEnabled = 1;\n\tent.sunRed = 1;\n\tent.sunGreen = 0.5;\n\tent.sunBlue = 0;\n\tent.HDRSunColorIntensity = 4;\n\tent.sunDir = (1, 0, 0);\n\tent.sunBeginFadeAngle = 0;\n\tent.sunEndFadeAngle = 60;\n";
        let h = super::ghosts_fog_hdr(script).expect("fog");
        assert_eq!(h.name, "mp_x_HDR");
        assert_eq!((h.fog.start, h.fog.half, h.fog.opacity), (300.0, 12000.0, 0.8));
        assert_eq!((h.colour_intensity, h.sun_intensity), (2.0, 4.0));
        let mut table = super::super::cod4::legacy_fog_table(&h.fog);
        let row = &mut table[super::super::t6::FOG_SLOT];
        super::hdr_fog_colours(row, &h, 3.0);
        // sRGB 0.5 is 0.2140 linear; 0.02 under the curve's knee is /12.92.
        assert!((row[4] - 0.21404 * 2.0 * 3.0).abs() < 1e-3, "{}", row[4]);
        assert!((row[6] - 0.02 / 12.92 * 6.0).abs() < 1e-5, "{}", row[6]);
        assert!((row[8] - 12.0).abs() < 1e-5 && row[10] == 0.0);
        // The distance law is the table's own: start and halfway distance;
        // Black Ops II's opacity law (w 1) unless the engine's is asked for.
        assert_eq!((row[0], row[1], row[2], row[3]), (300.0, 12000.0, 0.8, 1.0));
        const { assert!(super::FOG_LAW_GHOSTS > 1.5) };
        assert_eq!(super::srgb_to_linear(1.0), 1.0);
        assert_eq!(super::srgb_to_linear(0.0), 0.0);
    }

    /// The world shader's fog (`fog_at`, `fog_amount` in
    /// `geometry_diagnostic.wgsl`) under the engine's law keeps of the scene
    /// what Ghosts' own fog does (`fastfile_iw6::shading::fog_transmittance`);
    /// Black Ops II's law keeps more nearby.
    #[test]
    fn the_fog_law_keeps_what_ghosts_keeps() {
        let (start, half, opacity) = (1500.0f32, 6145.0f32, 0.25f32);
        let k = std::f32::consts::LN_2 / half;
        let ghosts = [0.0, 1.0 - opacity, -k, k * start];
        for d in [1500.0f32, 3000.0, 8000.0, 20000.0, 90000.0] {
            let fog = 1.0 - (2f32.powf(-(d - start) / half)).min(1.0);
            let keep_law = 1.0 - fog.min(opacity);
            let keep_bo2 = 1.0 - fog * opacity;
            let want = fastfile_iw6::shading::fog_transmittance(ghosts, d);
            assert!((keep_law - want).abs() < 1e-4, "{d}: {keep_law} against {want}");
            assert!(keep_bo2 >= keep_law - 1e-6);
        }
    }

    #[test]
    fn unset_film_keys_take_the_engines_values() {
        let v = asset_world::FilmVision {
            enable: true,
            desaturation: 0.2,
            desaturation_dark: -1.0e8,
            light_tint: [1.0, 1.0, 1.0],
            medium_tint: [-1.0e8; 3],
            dark_tint: [0.5, 0.5, 0.7],
            ..Default::default()
        };
        let v = super::film_unset_values(v);
        assert_eq!(v.medium_tint, [0.75, 0.75, 0.85]);
        assert_eq!(v.desaturation_dark, 0.2);
        let kept = super::film_unset_values(asset_world::FilmVision { medium_tint: [0.9; 3], desaturation_dark: 0.1, ..v });
        assert_eq!((kept.medium_tint, kept.desaturation_dark), ([0.9; 3], 0.1));
    }
}
