//! bo2mp: where Infinite Warfare's (IW7) multiplayer zones are, for its
//! guns as guest guns: the folder with its `global_core_mp.ff` (the weapon
//! definitions), `common_core_mp.ff` (their animations and sounds), the
//! technique-set zones and each gun's `mp_weapon_view_iw7_<gun>_tr.ff` /
//! `mp_weapon_world_iw7_<gun>_tr.ff`. The install names them as they are;
//! a copy beside other games' zones carries an `iw7_` prefix
//! (`fastfile_iw7::zone_path` takes either).

use std::path::{Path, PathBuf};

fn has_guns(dir: &Path) -> bool {
    fastfile_iw7::zone_path(dir, "global_core_mp").is_some()
}

/// The folder of Infinite Warfare's multiplayer zones: `CODLEGACY_IW7_ZONE`,
/// else the Infinite Warfare folder set in Game Folders (`iw`, or its
/// `zone/`), else wherever the games' folders hold its `global_core_mp`.
pub fn iw7_zone_dir() -> Option<PathBuf> {
    if let Some(dir) = codlegacy_env::var_os("CODLEGACY_IW7_ZONE")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        && has_guns(&dir)
    {
        return Some(dir);
    }
    if let Some(set) = asset_transport::game_paths::get("iw")
        && let Some(dir) = [set.clone(), set.join("zone")]
            .into_iter()
            .find(|d| has_guns(d))
    {
        return Some(dir);
    }
    let root = asset_transport::games_root_from_env().ok()?;
    ["global_core_mp", "iw7_global_core_mp"]
        .into_iter()
        .find_map(|z| {
            asset_transport::find_zone_file_version(&root, z, asset_transport::IW7_ZONE_VERSION)
                .ok()
        })
        .and_then(|z| z.path.parent().map(Path::to_path_buf))
}
