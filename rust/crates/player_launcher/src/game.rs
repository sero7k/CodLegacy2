//! Finding the player's Black Ops II folder and starting CodLegacy.
//! The launcher only ever reads the player's folder to point the game at it.

use std::path::{Path, PathBuf};
use std::process::Command;

use asset_transport::{game_paths, game_scan};

use crate::error::{Error, Fault};
use crate::settings::Settings;

pub const GAME_EXE: &str = if cfg!(windows) { "CodLegacy.exe" } else { "CodLegacy" };

/// The folder the player chose if it still looks like Black Ops II, else whatever the scan finds.
pub fn find_bo2(s: &Settings) -> Result<PathBuf, Error> {
    let pack = PathBuf::from(s.pack_dir.trim());
    if !s.pack_dir.trim().is_empty() && asset_transport::call_of_claude::is_pack(&pack) {
        let bo2 = pack.join("bo2");
        if bo2.is_dir() {
            return Ok(bo2);
        }
    }
    let chosen = s.game_dir.trim();
    if !chosen.is_empty() {
        let p = PathBuf::from(chosen);
        if game_paths::looks_valid_for("bo2", &p) {
            return Ok(p);
        }
    }
    game_scan::scan()
        .into_iter()
        .find(|(k, _)| k == "bo2")
        .map(|(_, p)| p)
        .ok_or_else(|| Error::new(Fault::GameNotFound, "Could not find Black Ops II. Pick its folder in Settings."))
}

/// Checks a folder the player picked by hand.
pub fn check_bo2_folder(p: &Path) -> Result<(), Error> {
    if game_paths::looks_valid_for("bo2", p) {
        Ok(())
    } else {
        Err(Error::new(Fault::GameNotFound, "That folder does not look like Black Ops II (no t6mp.exe or zone files)."))
    }
}

/// The command that starts the game. The ticket goes in the environment, not the command
/// line, so it does not show up in process lists. (How the game reads these is the game
/// side's job; see docs/LAUNCHER_ACCOUNTS.md.)
pub fn command(install: &Path, bo2: &Path, api_url: &str, ticket: &str) -> Command {
    let mut c = Command::new(install.join(GAME_EXE));
    c.current_dir(install)
        .env("CODLEGACY_TICKET", ticket)
        .env("CODLEGACY_GAME_DIR", bo2)
        .env("CODLEGACY_API", api_url)
        // The long-lived key must never reach the game.
        .env_remove("CODLEGACY_KEY");
    c
}

pub fn start(install: &Path, bo2: &Path, api_url: &str, ticket: &str) -> Result<std::process::Child, Error> {
    if !install.join(GAME_EXE).is_file() {
        return Err(Error::other("CodLegacy is not installed yet. Press Update first."));
    }
    command(install, bo2, api_url, ticket).spawn().map_err(|e| Error::other(format!("Could not start the game: {e}")))
}

/// The games the CodLegacy content check looks for (game_paths keys): Black Ops II (required to play),
/// then the games whose maps and guns it still uses. Left out: Infinite Warfare, Battlefield 1, Cold War and
/// Vanguard (`NOT_PACKED`), and the MW2 and MW3 remasters, the Advanced Warfare maps and the two Black Ops II
/// mods, which CodLegacy supplies (`SUPPLIED`). Modern Warfare Remastered stays: the player needs their own.
pub const CHECKED: [&str; 8] = ["bo2", "bo1", "waw", "cod4", "bo3", "jup", "ghosts", "mwr"];

/// Points `game_paths` at the community projects and mods CodLegacy's own build carries (`projects/...` and
/// `mods/...` in the install folder), so the player never has to supply them. One the build does not hold is
/// left alone. Returns the keys it set.
pub fn link_supplied_mods(install: &Path) -> Vec<&'static str> {
    let mut set = Vec::new();
    for (key, rel) in call_of_claude::pack::SUPPLIED {
        let dir = install.join(rel);
        if dir.is_dir() && game_paths::set(key, Some(dir)).is_ok() {
            set.push(key);
        }
    }
    set
}

/// The result of taking a folder the player already has (a trimmed "Call of Claude" folder).
#[derive(Debug, Default)]
pub struct ExistingPack {
    /// Games the folder holds that CodLegacy will use.
    pub games: Vec<String>,
    /// Games in it that are ignored (parked or not used).
    pub ignored: Vec<String>,
    /// Files that failed the hash check; empty if it passed or there was nothing to check against.
    pub bad: Vec<String>,
    /// Whether the folder had a manifest to check against.
    pub checked: bool,
}

/// Starts using a Call of Claude folder the player already has, instead of building one from installed
/// games. It must be a pack with Black Ops II in it. If it carries a hash manifest every file is
/// checked and a mismatch refuses the folder; if not, it is accepted but marked as unchecked.
/// Games CodLegacy does not use (Infinite Warfare, Battlefield 1, ...) are ignored, not pointed at.
pub fn use_existing_pack(dir: &Path, progress: &mut dyn FnMut(usize, usize)) -> Result<ExistingPack, Error> {
    let other = |m: String| Error::other(m);
    if !asset_transport::call_of_claude::is_pack(dir) {
        return Err(other(format!("{} is not a Call of Claude folder (it has no {}).", dir.display(), asset_transport::call_of_claude::MARKER)));
    }
    let held = asset_transport::call_of_claude::games_in(dir);
    let (use_, ignored): (Vec<_>, Vec<_>) = held.into_iter().partition(|(k, _)| call_of_claude::pack::is_packed(k));
    if !use_.iter().any(|(k, _)| k == "bo2") {
        return Err(other("That folder has no Black Ops II (bo2), which CodLegacy needs.".into()));
    }
    let mut out = ExistingPack::default();
    if dir.join(call_of_claude::pack::MANIFEST).is_file() {
        out.checked = true;
        out.bad = call_of_claude::pack::verify(dir, progress).map_err(other)?;
        if !out.bad.is_empty() {
            return Err(other(format!("{} files in that folder do not match its manifest (first: {}), so it was not used.", out.bad.len(), out.bad[0])));
        }
    }
    game_paths::save(&use_).map_err(other)?;
    out.games = use_.into_iter().map(|(k, _)| k).collect();
    out.ignored = ignored.into_iter().map(|(k, _)| k).collect();
    Ok(out)
}

/// Where each checked game's folder is, from a content check: what the pack builder reads.
/// Games inside the Call of Claude folder are not sources.
pub fn pack_sources(list: &[GameStatus], pack_dir: &Path, include: &dyn Fn(&str) -> bool) -> Vec<(String, PathBuf)> {
    list.iter()
        .filter(|g| include(g.key))
        .filter_map(|g| g.path.clone().map(|p| (g.key.to_string(), p)))
        .filter(|(_, p)| !p.starts_with(pack_dir))
        .collect()
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GameStatus {
    pub key: &'static str,
    pub title: String,
    /// Where the game's files were found on this PC, if they were.
    pub path: Option<PathBuf>,
    /// Whether that folder is a Steam install (a Steam `appmanifest` names the folder). Files on disk
    /// show the game is installed here; they do not prove who bought it, and the launcher says so.
    pub steam: bool,
}

/// Whether `dir` is a Steam-installed game: it sits in `steamapps/common/<name>` and a Steam
/// `appmanifest_*.acf` in `steamapps` lists that `<name>` as its install folder.
pub fn steam_installed(dir: &Path) -> bool {
    let Some(name) = dir.file_name().map(|n| n.to_string_lossy().to_ascii_lowercase()) else { return false };
    let Some(common) = dir.parent() else { return false };
    let Some(steamapps) = common.parent().filter(|_| common.file_name().is_some_and(|n| n.eq_ignore_ascii_case("common"))) else {
        return false;
    };
    let Ok(entries) = std::fs::read_dir(steamapps) else { return false };
    entries.flatten().any(|e| {
        let f = e.file_name().to_string_lossy().to_ascii_lowercase();
        f.starts_with("appmanifest_") && f.ends_with(".acf") && std::fs::read_to_string(e.path()).is_ok_and(|t| acf_install_dir(&t).is_some_and(|d| d.to_ascii_lowercase() == name))
    })
}

/// The `installdir` value of a Steam `appmanifest` file.
pub fn acf_install_dir(text: &str) -> Option<String> {
    text.lines().find_map(|l| {
        let mut parts = l.split('"').filter(|p| !p.trim().is_empty());
        (parts.next()?.eq_ignore_ascii_case("installdir")).then(|| parts.next().map(str::to_string))?
    })
}

/// Looks the checked games up on this PC (one scan). The player's own chosen Black Ops II folder wins.
pub fn content_check(s: &Settings) -> Vec<GameStatus> {
    let mut found = game_scan::scan();
    let pack = PathBuf::from(s.pack_dir.trim());
    let packed: Vec<(String, PathBuf)> = if !s.pack_dir.trim().is_empty() && asset_transport::call_of_claude::is_pack(&pack) {
        asset_transport::call_of_claude::games_in(&pack)
    } else {
        Vec::new()
    };
    found.retain(|(k, _)| !packed.iter().any(|(p, _)| p == k));
    found.extend(packed);
    CHECKED
        .iter()
        .map(|&key| {
            let title = game_paths::GAMES.iter().find(|(k, _, _)| *k == key).map_or_else(|| key.to_string(), |(_, t, _)| (*t).to_string());
            let mut path = found.iter().find(|(k, _)| k == key).map(|(_, p)| p.clone());
            if key == "bo2" && let Ok(p) = find_bo2(s) {
                path = Some(p);
            }
            let steam = path.as_deref().is_some_and(steam_installed);
            GameStatus { key, title, path, steam }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_ticket_reaches_the_game_but_the_key_does_not() {
        let c = command(Path::new("/i"), Path::new("/bo2"), "https://x.test", "tkt");
        let envs: Vec<_> = c.get_envs().map(|(k, v)| (k.to_string_lossy().into_owned(), v.map(|v| v.to_string_lossy().into_owned()))).collect();
        assert!(envs.contains(&("CODLEGACY_TICKET".into(), Some("tkt".into()))));
        assert!(envs.contains(&("CODLEGACY_GAME_DIR".into(), Some("/bo2".into()))));
        assert!(envs.contains(&("CODLEGACY_KEY".into(), None)), "key is explicitly removed");
        assert!(c.get_args().next().is_none(), "nothing on the command line");
    }

    #[test]
    fn steam_installs_are_recognised_by_their_appmanifest() {
        let lib = tempfile::tempdir().unwrap();
        let steamapps = lib.path().join("steamapps");
        let game = steamapps.join("common").join("Call of Duty Black Ops II");
        std::fs::create_dir_all(&game).unwrap();
        assert!(!steam_installed(&game), "no manifest yet");
        std::fs::write(steamapps.join("appmanifest_202970.acf"), "\"AppState\"\n{\n\t\"appid\"\t\t\"202970\"\n\t\"installdir\"\t\t\"Call of Duty Black Ops II\"\n}\n").unwrap();
        assert!(steam_installed(&game));
        let elsewhere = lib.path().join("Games").join("Call of Duty Black Ops II");
        std::fs::create_dir_all(&elsewhere).unwrap();
        assert!(!steam_installed(&elsewhere), "not under steamapps/common");
    }

    #[test]
    fn an_existing_pack_is_checked_and_parked_games_are_ignored() {
        let t = tempfile::tempdir().unwrap();
        let src = t.path().join("g");
        std::fs::create_dir_all(src.join("zone/all")).unwrap();
        std::fs::write(src.join("zone/all/mp_a.ff"), "data").unwrap();
        let dest = t.path().join("pack");
        let plan = call_of_claude::pack::plan(&[("bo2".into(), src.clone()), ("iw".into(), src.clone())]);
        call_of_claude::pack::build(&plan, &dest, &mut |_| {}).unwrap();
        // Not a pack: refused. (Only the refusal paths run here: using a pack writes game_paths.cfg.)
        assert!(use_existing_pack(t.path(), &mut |_, _| {}).is_err());
        std::fs::write(dest.join("bo2/zone/all/mp_a.ff"), "evil").unwrap();
        let e = use_existing_pack(&dest, &mut |_, _| {}).unwrap_err();
        assert!(e.message.contains("do not match"), "{e}");
    }

    #[test]
    fn acf_install_dir_reads_the_value() {
        assert_eq!(acf_install_dir("\t\"installdir\"\t\t\"Ghosts\"").as_deref(), Some("Ghosts"));
        assert_eq!(acf_install_dir("\"name\" \"x\""), None);
    }

    #[test]
    fn starting_without_an_install_is_a_clear_error() {
        let dir = tempfile::tempdir().unwrap();
        assert!(start(dir.path(), dir.path(), "https://x.test", "t").is_err());
    }
}
