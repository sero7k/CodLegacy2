mod artifact_cache;
pub mod bake_all;
mod asset_graph;
mod gltf_export;
mod iwd;
mod lane;
mod lane_capability;
pub mod loading_screen;
mod map_load_process;
// bo2mp `CodLegacy check-maps`: every foreign map read as a match reads it.
mod map_check;
pub use map_check::{LookScene, bo2_look_scene, check_map, trace_bench};
mod map_scripts;
pub mod bake_guest;
mod match_load;
pub use match_load::match_common_mp;
pub mod plugin;
pub mod prepared;
pub mod session_load;
mod teardown;

pub use artifact_cache::{cache_flight, cache_get, cache_put, fnv1a64, fnv1a64_more};
pub(crate) use asset_graph::stamp_match_destructible_death;
pub use asset_graph::{
    AssetEdge, AssetEdgeCensus, AssetEdgeReason, AssetGraphCensus, DESTRUCTIBLE_DEATH_HINTS,
    DeathClipEdge, DeathHuskEdge, DestructibleDeathHint, DestructibleDeathRow, FpvMeshIndex,
    FpvMeshSpace, FxIndex, FxModelIndex, FxModelSpace, FxSpace, LoadedSoundIndex, LoadedSoundSpace,
    MapXModelIndex, MapXModelSpace, MaterialIndex, MaterialSpace, ProjectileModelIndex,
    ProjectileModelSpace, SoundAliasIndex, SoundAliasSpace, TechniqueSetIndex, TechniqueSetSpace,
    TracerIndex, TracerSpace, WorldWeaponIndex, WorldWeaponSpace, XAnimIndex, XAnimSpace,
    resolve_after_absorb,
};
pub use fastfile_iw4::GlyphCapture;
pub use gltf_export::{GltfExportSummary, export_prepared_world_gltf};
pub use iwd::{IwdSoundIndex, NamespaceSoundIwd, NamespaceTree, NamespaceTrees};
#[doc(hidden)]
pub use lane::ghosts_collision_for_tests;
pub use lane::{check_iw4_map, CommonCensus, LANE_GAPS, LaneGap, LoadedWorld, ZoneLane, lane};
// bo2mp lane C: the multiplayer front end (no map).
pub use lane::t6_frontend::{T6Frontend, load_t6_frontend, load_t6_zm_pictures, plain_rgba8, prefetch_own_image_at_launch, prewarm_t6_mp_map, prewarm_t6_mp_next, prewarm_t6_mp_pick, prefetch_guest_guns, prewarm_t6_mp_shared, stop_t6_prewarm, t6_match_ended, t6_menu_up};
// bo2zm M4: the Zombies front end (no map), a Resource kept for the run;
// renamed here so it sits beside the multiplayer front end.
pub use lane::{T6Frontend as T6ZmFrontend, load_t6_frontend as load_t6_zm_frontend};
/// bo2mp: the mod's Create-a-Class pictures (`CODLEGACY_T6_MOD`).
pub use lane::mod_picture_rgba as t6_mod_picture_rgba;
pub use lane::note_t6_card_held;
pub use lane::iw6_fx_harness_views;
#[cfg(feature = "jup")]
pub use lane::{JupPreview, jup_preview}; // bo2mp: an MW3 map converted as the BO3 look draws it (offline checks)
pub use lane_capability::{LaneStatus, PreparedCapability, lane_status};
pub use lighting_iw4::MODEL_LIGHTING_TILE_BYTES;
pub use loading_screen::{LoadingPreviewSource, LoadingScreen};
pub use map_load_process::MapLoadProcess;
pub use match_load::{
    MapLoadApproval, MatchLoadAbort, MatchLoadAccepted, MatchLoadBusy, MatchLoadDispatch,
    MatchLoadRequest, PreparedMatchReady, PreparedMatchSound,
};
pub use plugin::AssetPlugin;
pub use prepared::{
    MapFacts, MatchMaterials, MatchType10SoundHints, PreparedBodies, PreparedBodyClips,
    PreparedDestructibleDeath, PreparedFpvMeshes, PreparedGaps, PreparedLocalizedStrings,
    PreparedMap, PreparedProjectileMeshes, PreparedWeapons, PreparedWorldWeapons, PreparedXAnims,
    PreparedXModelWalkCensus, SessionCompass, T6HudFont, T6HudFonts, T6HudIcons, T6ScreenFilter,
    T6ScreenFilters, T6Ui,
};
pub use session_load::{
    check_guest_guns, MatchLoadOutcome, MatchMaterialSeed, PreparedMatch, PreparedWorld, ShellCommon,
    apply_match_material_map, load_match_material_catalog, load_match_material_seed, load_pool,
    load_prepared_match, load_shell_common, prefetch_match_common, prefetch_status, load_workers, note_gpu_resident, publish_process_cpus,
    ready_match_common,
    release_resident_cpu_images, gpu_resident, release_common_cpu_images, forget_stripped_common, rebuild_common_image,
};

pub mod image_handles;

/// bo2mp: a match the Black Ops II front end starts plays by Black Ops II's
/// rules on any map: a Modern Warfare 2, Modern Warfare 3 or Black Ops map
/// brings its world (drawing, collision, entities, ambience) and Black Ops
/// II brings the rest (guns, scripts, HUD, factions, sound).
static T6_RULES: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(true);

pub fn set_t6_rules(on: bool) {
    T6_RULES.store(on, std::sync::atomic::Ordering::Relaxed);
}

pub fn t6_rules() -> bool {
    T6_RULES.load(std::sync::atomic::Ordering::Relaxed)
}

/// bo2mp: which game the loaded match's map comes from (its own namespace:
/// Iw4 MW2, Iw5 MW3, T5 Black Ops, T6 Black Ops II, Iw3 CoD4, T4 World at
/// War), set by the match load; the players' bodies follow it.
static MAP_GAME: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(0);

const MAP_GAMES: [asset_core::AssetNamespace; 7] = [
    asset_core::AssetNamespace::Iw4,
    asset_core::AssetNamespace::T5,
    asset_core::AssetNamespace::Iw5,
    asset_core::AssetNamespace::T6,
    asset_core::AssetNamespace::Iw6,
    asset_core::AssetNamespace::T4,
    asset_core::AssetNamespace::Iw3,
];

pub fn set_map_game(game: Option<asset_core::AssetNamespace>) {
    let code = game.and_then(|g| MAP_GAMES.iter().position(|m| *m == g)).map_or(0, |i| i as u8 + 1);
    MAP_GAME.store(code, std::sync::atomic::Ordering::Relaxed);
    // (A Modern Warfare Remastered map turns its automatic exposure back on
    // as it loads.)
    asset_world::set_mwr_auto_exposure(None);
    asset_world::set_h1_eye_lighting(Vec::new());
    asset_world::set_iw6_doors(Vec::new());
    asset_world::set_iw6_scripts(None);
    asset_world::set_iw6_path_nodes(None);
    asset_world::set_iw6_mover_poses(Vec::new());
    release_native_maps();
    if game != Some(asset_core::AssetNamespace::T7) {
        // (Kept for a Black Ops III map after a Black Ops III map only.)
        fastfile_t7::map::release_folder_caches();
    }
}

/// bo2mp: no other game's renderer draws a map: each lets go of its scene,
/// its pictures and its buffers (as when any map loads; the next map makes
/// its own anyway). Back at the menu after a Black Ops III match this is
/// most of what the match held.
pub fn release_native_maps() {
    asset_world::set_h1_native_zones(None);
    asset_world::set_iw6_native_zones(None);
    asset_world::set_t7_native_zone(None);
    asset_world::set_jup_native_zone(None);
    asset_world::set_iw7_native_zone(None);
    frame::settings::t7_native_load::set(frame::settings::t7_native_load::IDLE);
    frame::settings::t7_native_fx::set_drawing(false);
}

/// bo2mp: Graphics > Player Models: another game's map shows its own
/// soldiers (true) or Black Ops II's characters.
static MAP_GAME_BODIES: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

pub fn set_map_game_bodies(on: bool) {
    MAP_GAME_BODIES.store(on, std::sync::atomic::Ordering::Relaxed);
}

/// bo2mp: whose soldiers the players are (Options: SOLDIERS): Black Ops'
/// (`lane::t5_soldiers`), Black Ops II's, or Modern Warfare III's (2023)
/// operators (`lane::jup_ops`). Black Ops II's where the chosen game's are
/// not installed.
static SOLDIERS: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(0);

/// bo2mp: reads and builds every Modern Warfare III (2023) guest gun as a
/// match does (what `CodLegacy check-guns` does): for a small copy of the
/// install (`fastfile_jup::pack`), whose recorder notes what is read.
/// Returns its report lines.
pub fn touch_jup_guns() -> Vec<String> {
    lane::jup_check()
}

/// bo2mp: the offline audit of every guest gun's first-person fit
/// (`examples/jup_gun_audit.rs`): a Markdown report from the part zones
/// in `dir` (decompressed `<zone>.zone` files, or the install's `.ff`).
#[cfg(feature = "jup")]
pub fn jup_gun_audit(dir: &std::path::Path, filter: Option<&str>) -> String {
    lane::jup_gun_audit(dir, filter)
}

/// bo2mp: the Black Ops III Workshop weapon packs' guns as the lane adds
/// them, and what of each resolves (`examples/t7_pack_guns.rs`); `build`:
/// also built as a match builds them.
pub fn workshop_gun_check(build: bool, filter: Option<&str>) -> Vec<String> {
    lane::workshop_gun_check(build, filter)
}

/// bo2mp `examples/t7_pack_lods.rs` (`IDLE=1`): a Workshop pack gun's idle clip tracks on its view model's bones.
pub fn t7_pack_gun_idle_tracks(zone: &std::path::Path, guns: &[String]) -> Vec<String> {
    lane::t7_pack_gun_idle_tracks(zone, guns)
}

#[cfg(feature = "jup")]
mod jup_export; // bo2mp `CodLegacy export-jup`: the guns, camos, cards and sounds in a standalone folder
#[cfg(feature = "jup")]
pub use jup_export::{check_jup_export, export_jup, plan_jup};
/// bo2mp: `CodLegacy export-jup` without the `jup` feature: MW3's (2023)
/// guns are left out of the game, so there is nothing to export.
#[cfg(not(feature = "jup"))]
pub fn export_jup(_: &std::path::Path, _: &mut dyn FnMut(&str)) -> Result<String, String> {
    Err("Modern Warfare III's (2023) guns are not in this build".to_owned())
}

pub fn set_soldiers(soldiers: frame::settings::Soldiers) {
    let at = frame::settings::Soldiers::ALL.iter().position(|s| *s == soldiers).unwrap_or(0);
    SOLDIERS.store(at as u8, std::sync::atomic::Ordering::Relaxed);
}

pub fn soldiers() -> frame::settings::Soldiers {
    let at = usize::from(SOLDIERS.load(std::sync::atomic::Ordering::Relaxed));
    frame::settings::Soldiers::ALL.get(at).copied().unwrap_or_default()
}

pub fn mw3_operators() -> bool {
    soldiers() == frame::settings::Soldiers::Mw3
}

/// The model a player of a team is drawn as in place of Black Ops II's
/// soldier, when the match built the chosen game's.
pub fn soldier_for(axis: bool, client: u32) -> Option<String> {
    match soldiers() {
        frame::settings::Soldiers::BlackOps => lane::t5_soldiers::soldier_for(axis, client),
        frame::settings::Soldiers::Mw3 => lane::jup_ops::operator_for(axis, client),
        frame::settings::Soldiers::BlackOps2 => None,
    }
}

pub fn map_game_bodies() -> bool {
    MAP_GAME_BODIES.load(std::sync::atomic::Ordering::Relaxed)
}

pub fn map_game() -> Option<asset_core::AssetNamespace> {
    let code = MAP_GAME.load(std::sync::atomic::Ordering::Relaxed);
    MAP_GAMES.get(usize::from(code).checked_sub(1)?).copied()
}

/// bo2mp: other games' guns join Black Ops II's as extra weapons (off with
/// `BO2MP_GUEST_GUNS=0`).
pub fn guest_guns_for(game: &str) -> bool {
    guest_guns() && bo2_profile::guest::match_games() & bo2_profile::guest::game_bit(game) != 0
}

/// bo2mp: a match started without the front end (a map from the command
/// line, no START MATCH) read every installed game's guns, each game's whole
/// roster, though only the guns his classes hold can be used (the bots'
/// classes take Black Ops II's and the mod's): on a 4 GB card most of the
/// match's pictures were guns nobody could pick. Now it reads the games his
/// saved classes hold, as START MATCH does (none, with no stats file: the
/// default classes are Black Ops II's). `BO2MP_DIRECT_START_ALL_GUESTS=1`
/// reads every game's as before.
pub(crate) fn direct_start_guest_games() {
    if bo2_profile::guest::match_games_set() || !t6_rules() || !guest_guns() {
        return;
    }
    if std::env::var("BO2MP_DIRECT_START_ALL_GUESTS").is_ok_and(|v| v.trim() == "1") {
        return;
    }
    let path = std::env::var_os("BO2MP_STATS")
        .map_or_else(|| diag::profile_file("bo2mp_stats.txt"), std::path::PathBuf::from);
    let stats = bo2_profile::Profile::read_stats_file(&path);
    let mask = bo2_profile::guest::games_of_classes(&stats);
    bo2_profile::guest::set_match_games(mask);
    diag::info!(
        World,
        "bo2mp guest guns: a match started without the front end reads the guest guns of games {mask:#x} (his classes in {})",
        path.display()
    );
}

/// bo2mp: whether other games' guns join a Black Ops II match at all
/// (`BO2MP_GUEST_GUNS`); [`guest_guns_for`] says which games' a match reads.
pub fn guest_guns() -> bool {
    std::env::var("BO2MP_GUEST_GUNS").map_or(true, |v| v.trim() != "0")
}

mod script_sources;
pub use script_sources::{ScriptSources, ScriptTable, T6AnimFacts, T6ScriptSet, T7AiCast, T7Character};
mod server_pack_io; // bo2mp: dedicated server map packs
pub use server_pack_io::build_map_pack;
