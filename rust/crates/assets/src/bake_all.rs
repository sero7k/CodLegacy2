//! bo2mp (bake all): the games Options > BAKE ALL and the `bake_all`
//! example can bake ahead. The loop is `map_bake::ahead`; this is its table,
//! one [`Game`] per game: which maps (from the folder set in GAME FOLDERS)
//! and the function that bakes one map into the files its first load
//! writes. A new game is one more entry here, not a second runner.
//!
//! Gameplay data (stats, hitboxes, collision) is never baked.

use std::path::{Path, PathBuf};

pub use map_bake::ahead::{Eta, Game, Item, Left, Now, Summary, eta_left, now, plan, run, run_timed, running, stop, take_finished};
/// The bake job the row shows (game, map, step, counts, why).
pub use map_bake::status::current as current_job;

/// Every game BAKE ALL can bake.
pub const GAMES: &[Game] = &[
    Game { key: "bo3", code: "t7", maps: bo3_maps, bake: bo3_bake },
    Game { key: "bo2", code: "t6", maps: bo2_maps, bake: bo2_bake },
    // bo2mp (bake all): the guest games (pictures + programs, as their first
    // load writes them: `crate::bake_guest`).
    Game { key: "bo1", code: "t5", maps: guest_maps, bake: crate::bake_guest::bake_ahead },
    Game { key: "mw2", code: "iw4", maps: guest_maps, bake: crate::bake_guest::bake_ahead },
    Game { key: "mw3", code: "iw5", maps: guest_maps, bake: crate::bake_guest::bake_ahead },
    Game { key: "cod4", code: "iw3", maps: guest_maps, bake: crate::bake_guest::bake_ahead },
    Game { key: "waw", code: "t4", maps: guest_maps, bake: crate::bake_guest::bake_ahead },
];

/// The game with folder key `key` ("bo3").
pub fn game(key: &str) -> Option<Game> {
    GAMES.iter().copied().find(|g| g.key.eq_ignore_ascii_case(key))
}

/// Every map of every game set in GAME FOLDERS.
pub fn plan_from_folders() -> Vec<Item> {
    map_bake::ahead::plan(GAMES, |g| asset_transport::game_paths::get(g.key))
}

/// What the files depend on besides the game files and this program: a
/// change makes PREPARE MAPS AT LAUNCH check every map again.
fn set_stamp() {
    let native = frame::settings::T7_NATIVE_RENDER.load(std::sync::atomic::Ordering::Relaxed);
    map_bake::ahead::set_stamp_extra(format!("t7 cap {} native {native}", render_t7::bake_ahead_cap()));
}

/// Every map of the games set in GAME FOLDERS whose key `skip` leaves out.
fn plan_without(skip: &dyn Fn(&str) -> bool) -> Vec<Item> {
    plan_from_folders().into_iter().filter(|(g, _)| !skip(g.key)).collect()
}

/// Options > PREPARE MAPS: the games set in GAME FOLDERS (less those `skip`
/// leaves out, the menu's unchecked games), on a thread of its own (each map
/// shows on the bar: game, map, step, counts). `resume`: maps already made
/// by this version and settings are skipped. `Ok`: how many maps (0: none).
pub fn start(resume: bool, skip: &dyn Fn(&str) -> bool) -> Result<usize, String> {
    set_stamp();
    map_bake::ahead::start(plan_without(skip), |line| diag::info!(Zone, "bake all: {line}"), resume)
}

/// Each game set in GAME FOLDERS (in [`GAMES`] order): its maps still to make
/// and its measured time per map (reads the folders: not every frame).
pub fn left_by_game() -> Vec<Left> {
    set_stamp();
    map_bake::ahead::left(&plan_from_folders())
}

/// The map files in `p`: `p` itself when it is a file, else the folder's
/// files `keep` accepts (sorted).
pub fn files_in(p: &Path, keep: fn(&Path) -> bool) -> Vec<PathBuf> {
    if p.is_file() {
        return vec![p.to_path_buf()];
    }
    let mut found: Vec<PathBuf> = std::fs::read_dir(p).map(|d| d.flatten().map(|e| e.path()).filter(|f| keep(f)).collect()).unwrap_or_else(|_| Vec::new());
    found.sort();
    found
}

/// Black Ops III: the multiplayer maps' zones (`mp_*.ff`, not the shared or
/// patch zones), in the install's `zone` folder or the folder given.
fn bo3_maps(folder: &Path) -> Vec<PathBuf> {
    let zone = folder.join("zone");
    files_in(if zone.is_dir() { &zone } else { folder }, is_bo3_map)
}

fn is_bo3_map(f: &Path) -> bool {
    let Some(stem) = f.file_stem().and_then(|s| s.to_str()) else { return false };
    let stem = stem.to_ascii_lowercase();
    f.extension().is_some_and(|x| x.eq_ignore_ascii_case("ff")) && stem.starts_with("mp_") && !stem.ends_with("_patch") && !stem.starts_with("mp_common") && stem != "mp_patch"
}

/// A guest game (Black Ops, MW2, MW3, CoD4, WaW): its multiplayer maps'
/// zones in the install's `zone` folder and the folders in it (`Common`,
/// `english`, `dlc`), or the folder given; the zones the bake takes only.
fn guest_maps(folder: &Path) -> Vec<PathBuf> {
    let zone = folder.join("zone");
    let root = if zone.is_dir() { zone } else { folder.to_path_buf() };
    let mut found = files_in(&root, is_guest_map_name);
    if root.is_dir() {
        let mut subs: Vec<PathBuf> = std::fs::read_dir(&root).map(|d| d.flatten().map(|e| e.path()).filter(|p| p.is_dir()).collect()).unwrap_or_else(|_| Vec::new());
        subs.sort();
        for sub in subs {
            found.extend(files_in(&sub, is_guest_map_name));
        }
    }
    found.retain(|f| crate::bake_guest::is_guest_map(f));
    found
}

/// A guest map's own zone by its name: `mp_*.ff`, not its loading screen
/// (`_load`), a patch or the shared zones.
fn is_guest_map_name(f: &Path) -> bool {
    let Some(stem) = f.file_stem().and_then(|s| s.to_str()) else { return false };
    let stem = stem.to_ascii_lowercase();
    f.extension().is_some_and(|x| x.eq_ignore_ascii_case("ff"))
        && stem.starts_with("mp_")
        && !stem.ends_with("_load")
        && !stem.ends_with("_patch")
        && !stem.starts_with("mp_common")
        && stem != "mp_patch"
}

/// Black Ops III: the map's pictures at the size the load uses (another
/// size would be a file no load reads), then its sun map (`map.pak`), from
/// one read of the zone, so the map's first load finds the ground lit from
/// disk.
fn bo3_bake(zone: &Path) -> Result<String, String> {
    let map = fastfile_t7::map::T7Map::load(&fastfile_t7::map::MapSources::beside(zone))?;
    let pictures = render_t7::bake_ahead_of(&map, render_t7::bake_ahead_cap())?;
    if map_bake::ahead::stopping() {
        return Ok(pictures);
    }
    match crate::lane::t7_sun_map_ahead(zone, &map) {
        Ok(sun) => Ok(format!("{pictures}; {sun}")),
        Err(e) => Err(format!("{pictures}; {e}")),
    }
}

/// Black Ops II: the multiplayer maps' zones (`mp_*.ff`), in the install's
/// `zone\all` folder or the folder given.
fn bo2_maps(folder: &Path) -> Vec<PathBuf> {
    let all = folder.join("zone").join("all");
    let zone = folder.join("all");
    files_in(
        if all.is_dir() {
            &all
        } else if zone.is_dir() {
            &zone
        } else {
            folder
        },
        is_bo2_map,
    )
}

fn is_bo2_map(f: &Path) -> bool {
    let Some(stem) = f.file_stem().and_then(|s| s.to_str()) else { return false };
    let stem = stem.to_ascii_lowercase();
    f.extension().is_some_and(|x| x.eq_ignore_ascii_case("ff")) && stem.starts_with("mp_") && !stem.ends_with("_patch")
}

/// Black Ops II: the map's pictures, as the packs store them, in one file
/// (every picture its materials name; a load reads the ones it links).
fn bo2_bake(zone: &Path) -> Result<String, String> {
    crate::lane::t6_bake_map_bundle(zone)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bo2_map_zones_only() {
        for (f, map) in [
            ("mp_nuketown_2020.ff", true),
            ("MP_Raid.FF", true),
            ("common_mp.ff", false),
            ("patch_mp.ff", false),
            ("mp_raid_patch.ff", false),
            ("frontend.ff", false),
            ("zm_transit.ff", false),
            ("mp_raid.ipak", false),
        ] {
            assert_eq!(is_bo2_map(Path::new(f)), map, "{f}");
        }
        assert_eq!(game("bo2").map(|g| g.code), Some("t6"));
    }

    #[test]
    fn bo3_map_zones_only() {
        for (f, map) in [
            ("mp_skyjacked.ff", true),
            ("MP_Nuketown_X.FF", true),
            ("mp_common.ff", false),
            ("mp_common_patch.ff", false),
            ("mp_patch.ff", false),
            ("mp_skyjacked_patch.ff", false),
            ("mp_skyjacked.xpak", false),
            ("zm_zod.ff", false),
        ] {
            assert_eq!(is_bo3_map(Path::new(f)), map, "{f}");
        }
        assert_eq!(game("BO3").map(|g| g.code), Some("t7"));
        assert!(game("nope").is_none());
    }

    #[test]
    fn guest_map_zones_only() {
        for (f, map) in [
            ("mp_nuked.ff", true),
            ("MP_Terminal_CLS.FF", true),
            ("mp_terminal_cls_load.ff", false),
            ("mp_rust_long_load.ff", false),
            ("mp_common.ff", false),
            ("mp_patch.ff", false),
            ("mp_nuked_patch.ff", false),
            ("common_mp.ff", false),
            ("mp_nuked.iwd", false),
        ] {
            assert_eq!(is_guest_map_name(Path::new(f)), map, "{f}");
        }
        for (key, code) in [("bo1", "t5"), ("mw2", "iw4"), ("mw3", "iw5"), ("cod4", "iw3"), ("waw", "t4")] {
            assert_eq!(game(key).map(|g| g.code), Some(code), "{key}");
        }
    }
}
