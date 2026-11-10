//! Finds the installed games without asking (the automatic scan).
//!
//! Where it looks, in order (the first folder found for a game wins, and a
//! folder told by its program beats one told by its zones, which beats one
//! told by its name only):
//! - every Steam library: Steam's folder from the registry (`SteamPath`,
//!   `InstallPath`) or its usual places, each library in
//!   `steamapps/libraryfolders.vdf`, then `steamapps/common/*`;
//! - the launchers' folders: Battle.net's `Call of Duty*`, Epic Games, the
//!   Xbox app's `XboxGames`, `Program Files` and `Program Files (x86)`;
//! - `Games` and `SteamLibrary` at the root of every drive A-Z;
//! - his Documents, Desktop and Downloads (and OneDrive's);
//! - the `CODLEGACY_GAMES` folder and its folders;
//! - then every drive's folders, three levels down.
//!
//! Folders are searched at most three levels below each of these, never
//! into Windows' own, the recycle bin, `node_modules`, `target` and the
//! like. A folder is a game's when its program is there (`t6mp.exe`,
//! `iw4mp.exe`, ...), or its zones are laid out as that game's (`zone/all/`
//! for Black Ops II, a map of its own in `zone/`), or its name says so
//! (`bo2_game_files`, `Black Ops II`) with a `zone` folder or fast files in
//! it: a folder of "game files" holding only the files the rebuild reads
//! counts.
//!
//! [`fill_missing`] writes what it found into `game_paths.cfg` for the
//! games with no good folder set, keeping every folder he set himself.
//! `CODLEGACY_NO_DETECT` turns it off.

use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::thread::JoinHandle;

use crate::game_paths::{self, GAMES, MOD_KEYS};

/// The programs that mark each game's install: (key, any of these files in
/// the folder).
pub const SIGNATURE_EXES: [(&str, &[&str]); 15] = [
    ("bo2", &["t6mp.exe", "t6zm.exe", "t6sp.exe"]),
    ("bo1", &["BlackOpsMP.exe", "BlackOps.exe"]),
    ("mw2", &["iw4mp.exe", "iw4x.exe", "iw4sp.exe"]),
    ("mw3", &["iw5mp.exe", "iw5sp.exe"]),
    ("bo3", &["BlackOps3.exe"]),
    ("aw", &["s1_mp64_ship.exe", "s1_sp64_ship.exe"]),
    ("jup", &["cod23-cod.exe", "sp23-cod.exe"]),
    ("waw", &["CoDWaWmp.exe", "CoDWaW.exe"]),
    ("cod4", &["iw3mp.exe", "iw3sp.exe"]),
    ("ghosts", &["iw6mp64_ship.exe", "iw6sp64_ship.exe"]),
    ("iw", &["iw7_ship.exe"]),
    ("mwr", &["h1_mp64_ship.exe", "h1_sp64_ship.exe"]),
    ("cw", &["BlackOpsColdWar.exe"]),
    ("vg", &["Vanguard.exe"]),
    // bo2mp: Battlefield 1 (with its `Data` and `Patch`, see `identify`).
    ("bf1", &["bf1.exe"]),
];

/// Fast files only that game has (its own maps, mostly), found in `zone/`,
/// one folder under it or beside the program: a folder holding one is that
/// game's even without its program. Names are lower case.
const SIGNATURE_ZONES: [(&str, &[&str]); 10] = [
    ("bo2", &["mp_nuketown_2020.ff", "mp_raid.ff", "mp_hijacked.ff", "mp_carrier.ff", "zm_transit.ff"]),
    ("bo1", &["mp_nuked.ff", "mp_firingrange.ff", "mp_array.ff", "mp_cracked.ff", "zombie_theater.ff"]),
    ("mw2", &["mp_rust.ff", "mp_afghan.ff", "mp_highrise.ff", "mp_favela.ff", "mp_quarry.ff"]),
    ("mw3", &["mp_seatown.ff", "mp_bootleg.ff", "mp_plaza2.ff", "mp_hardhat.ff", "mp_mogadishu.ff"]),
    ("waw", &["mp_castle.ff", "mp_airfield.ff", "mp_makin.ff", "mp_asylum.ff"]),
    ("cod4", &["mp_backlot.ff", "mp_bloc.ff", "mp_crossfire.ff", "mp_citystreets.ff"]),
    ("bo3", &["core_common.ff", "mp_biodome.ff", "mp_sector.ff", "mp_ethiopia.ff"]),
    ("ghosts", &["mp_prisonbreak.ff", "mp_chasm.ff", "mp_strikezone.ff", "mp_sovereign.ff"]),
    ("aw", &["mp_recovery.ff", "mp_lab2.ff", "mp_venus.ff", "mp_instinct.ff"]),
    ("iw", &["cp_zmb.ff", "mp_metropolis.ff", "mp_breakneck.ff", "mp_frontier.ff"]),
];

/// How a folder was told to be a game's (lower is surer).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Evidence {
    /// Its program with a `zone` folder or fast files beside it.
    ProgramAndZones,
    /// Its zones' layout.
    Zones,
    /// Its program alone.
    Program,
    /// Its name, with a `zone` folder or fast files in it.
    Name,
}

/// Folders never searched (system and tool folders), lower case.
const SKIP_DIRS: [&str; 16] = [
    "windows", "$recycle.bin", "system volume information", "programdata", "appdata", "node_modules",
    ".git", "msocache", "recovery", "perflogs", "target", "windowsapps", "winsxs", "$windows.~bt",
    "$windows.~ws", "config.msi",
];

/// Whether the scan steps into a folder of this name.
fn skipped(name: &str) -> bool {
    let n = name.to_ascii_lowercase();
    SKIP_DIRS.contains(&n.as_str()) || n.starts_with('.') || n.starts_with('$')
}

/// Every fast file's name (lower case) in `dir`, `dir/zone` and the
/// folders one level under `dir/zone`.
fn zone_names(dir: &Path) -> Vec<String> {
    fn read(d: &Path, names: &mut Vec<String>, subdirs: Option<&mut Vec<PathBuf>>) {
        let Ok(entries) = std::fs::read_dir(d) else { return };
        let mut subdirs = subdirs;
        for e in entries.flatten() {
            let name = e.file_name().to_string_lossy().to_ascii_lowercase();
            if name.ends_with(".ff") {
                names.push(name);
            } else if let Some(s) = subdirs.as_mut()
                && e.file_type().is_ok_and(|t| t.is_dir())
            {
                s.push(e.path());
            }
        }
    }
    let mut names = Vec::new();
    read(dir, &mut names, None);
    let zone = dir.join("zone");
    if zone.is_dir() {
        let mut subs = Vec::new();
        read(&zone, &mut names, Some(&mut subs));
        for s in subs {
            read(&s, &mut names, None);
        }
    }
    names
}

/// The game a folder's zones are laid out as, if any.
fn game_from_zones(dir: &Path) -> Option<&'static str> {
    // Black Ops II keeps its zones in `zone/all` (patch_mp.ff there).
    let all = dir.join("zone").join("all");
    if all.join("patch_mp.ff").is_file() || all.join("common_mp.ff").is_file() {
        return Some("bo2");
    }
    let names = zone_names(dir);
    if names.is_empty() {
        return None;
    }
    // Modern Warfare III (2023): `mp_jup_*` zones.
    if names.iter().any(|n| n.starts_with("mp_jup_") || n.contains("_jup_")) {
        return Some("jup");
    }
    // Modern Warfare Remastered: Horizon MW's folders beside its zones.
    if dir.join("hmw-mod").is_dir() || dir.join("hmw-usermaps").is_dir() {
        return Some("mwr");
    }
    SIGNATURE_ZONES.iter().find(|(_, ffs)| ffs.iter().any(|f| names.iter().any(|n| n == f))).map(|(k, _)| *k)
}

/// The game a folder holds and how it was told, from the files in it and
/// its name. Unit-testable on any folder tree.
pub fn identify(dir: &Path) -> Option<(&'static str, Evidence)> {
    // (Battlefield 1 has no zones: its `Data` + `Patch` folders and program.)
    if dir.join("bf1.exe").is_file() && game_paths::looks_valid_for(game_paths::BF1_KEY, dir) {
        return Some((game_paths::BF1_KEY, Evidence::ProgramAndZones));
    }
    let zones = game_paths::looks_valid(dir);
    let by_exe = SIGNATURE_EXES.iter().find(|(_, exes)| exes.iter().any(|f| dir.join(f).is_file())).map(|(k, _)| *k);
    // (Modern Warfare III's `cod.exe` is shared by the newer games: only
    // with its zones.)
    if let Some(key) = by_exe
        && zones
    {
        return Some((key, Evidence::ProgramAndZones));
    }
    if let Some(key) = game_from_zones(dir) {
        // (A "mwr" or "cod4" folder told apart by its name: their maps
        // share names.)
        if key == "cod4"
            && let Some("mwr") = game_from_name(dir)
        {
            return Some(("mwr", Evidence::Zones));
        }
        return Some((key, Evidence::Zones));
    }
    if let Some(key) = by_exe {
        return Some((key, Evidence::Program));
    }
    if zones && let Some(key) = game_from_name(dir) {
        return Some((key, Evidence::Name));
    }
    None
}

/// Which game a folder holds by its name alone (extracted "game files"
/// folders with no program in them: `H:\mwr_game_files`, `F:\mw3`, `Game
/// Files\Infinite Warfare`, `Documents\bo2_game_files`). A `<key>_game_files`
/// folder is that key's; otherwise names are compared letters and digits
/// only, the first row with a matching word winning (the longer names come
/// first). A "mw3" folder is Modern Warfare III (2023) when its zones are
/// that game's (`jup` in a fast file's name), else Modern Warfare 3 (2011).
pub(crate) fn game_from_name(dir: &Path) -> Option<&'static str> {
    const NAMES: [(&str, &[&str]); 13] = [
        ("bo3", &["blackopsiii", "blackops3", "bo3"]),
        ("bo2", &["blackopsii", "blackops2", "bo2"]),
        ("cw", &["coldwar"]),
        ("bo1", &["blackops", "bo1"]),
        ("mwr", &["modernwarfareremastered", "mwr", "horizonmw"]),
        ("mw3", &["modernwarfareiii", "modernwarfare3", "mw3"]),
        ("mw2", &["modernwarfareii", "modernwarfare2", "mw2"]),
        ("iw", &["infinitewarfare"]),
        ("aw", &["advancedwarfare"]),
        ("ghosts", &["ghosts"]),
        ("waw", &["worldatwar", "waw"]),
        ("vg", &["vanguard"]),
        ("cod4", &["callofduty4", "cod4"]),
    ];
    let raw = dir.file_name()?.to_string_lossy().to_ascii_lowercase();
    if let Some(prefix) = raw.strip_suffix("_game_files").or_else(|| raw.strip_suffix(" game files"))
        && let Some((key, _, _)) = GAMES.iter().find(|(k, _, _)| *k == prefix && !MOD_KEYS.contains(k))
        && *key != "mw3"
    {
        return Some(key);
    }
    let name: String = raw.chars().filter(char::is_ascii_alphanumeric).collect();
    let (key, _) = NAMES.iter().find(|(_, words)| words.iter().any(|w| name.contains(w)))?;
    if *key == "mw3" {
        let zone = dir.join("zone");
        let jup = [dir.to_path_buf(), zone].iter().any(|d| {
            std::fs::read_dir(d).is_ok_and(|r| r.flatten().any(|e| e.file_name().to_string_lossy().to_ascii_lowercase().contains("jup")))
        });
        return Some(if jup { "jup" } else { "mw3" });
    }
    Some(key)
}

/// A `reg query` value (Windows): `None` when absent.
#[cfg(windows)]
fn reg_value(key: &str, value: &str) -> Option<PathBuf> {
    use std::os::windows::process::CommandExt;
    // (No console window flashing up.)
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let out = std::process::Command::new("reg")
        .args(["query", key, "/v", value])
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .ok()?;
    let text = String::from_utf8_lossy(&out.stdout);
    text.lines().find_map(|l| {
        let (_, v) = l.split_once("REG_SZ")?;
        let v = v.trim();
        (!v.is_empty()).then(|| PathBuf::from(v.replace('/', "\\")))
    })
}

/// The library folders in a Steam `libraryfolders.vdf`.
pub fn vdf_libraries(text: &str) -> Vec<PathBuf> {
    let mut libraries = Vec::new();
    for line in text.lines() {
        let mut parts = line.split('"').filter(|p| !p.trim().is_empty());
        if parts.next().is_some_and(|k| k.eq_ignore_ascii_case("path"))
            && let Some(v) = parts.next()
        {
            libraries.push(PathBuf::from(v.replace("\\\\", "\\")));
        }
    }
    libraries
}

/// The drives there are (`C:\`, ...; `/` off Windows).
fn drives() -> Vec<PathBuf> {
    if cfg!(windows) {
        (b'A'..=b'Z').map(|d| PathBuf::from(format!("{}:\\", d as char))).filter(|p| p.is_dir()).collect()
    } else {
        vec![PathBuf::from("/")]
    }
}

/// Every Steam library folder there is (each holds `steamapps/`): Steam's
/// own folder (registry, then the usual places) and every library its
/// `steamapps/libraryfolders.vdf` lists, each once.
pub fn steam_libraries() -> Vec<PathBuf> {
    steam_libraries_on(&drives())
}

fn steam_libraries_on(drives: &[PathBuf]) -> Vec<PathBuf> {
    let mut steams: Vec<PathBuf> = Vec::new();
    #[cfg(windows)]
    {
        for (key, value) in [
            ("HKCU\\Software\\Valve\\Steam", "SteamPath"),
            ("HKLM\\SOFTWARE\\WOW6432Node\\Valve\\Steam", "InstallPath"),
            ("HKLM\\SOFTWARE\\Valve\\Steam", "InstallPath"),
        ] {
            steams.extend(reg_value(key, value));
        }
    }
    steams.extend(["ProgramFiles(x86)", "ProgramFiles"].iter().filter_map(std::env::var_os).map(|p| PathBuf::from(p).join("Steam")));
    if let Some(home) = std::env::var_os("HOME") {
        let home = PathBuf::from(home);
        steams.push(home.join(".steam").join("steam"));
        steams.push(home.join(".local").join("share").join("Steam"));
    }
    for d in drives {
        for s in ["Steam", "SteamLibrary", "Program Files (x86)/Steam", "Program Files/Steam", "Games/Steam", "Games/SteamLibrary"] {
            steams.push(d.join(s));
        }
    }
    let mut libraries: Vec<PathBuf> = Vec::new();
    for steam in steams.iter().filter(|s| s.is_dir()) {
        libraries.push(steam.clone());
        if let Ok(text) = std::fs::read_to_string(steam.join("steamapps").join("libraryfolders.vdf")) {
            libraries.extend(vdf_libraries(&text));
        }
    }
    let mut seen: Vec<PathBuf> = Vec::new();
    libraries.retain(|l| {
        let key = PathBuf::from(l.to_string_lossy().to_ascii_lowercase().trim_end_matches(['\\', '/']));
        if seen.contains(&key) {
            return false;
        }
        seen.push(key);
        true
    });
    libraries
}

/// Where the games may be, in the order searched: (folder, levels to go
/// down).
fn scan_roots() -> Vec<(PathBuf, usize)> {
    let drives = drives();
    let mut roots: Vec<(PathBuf, usize)> = Vec::new();
    let add = |p: PathBuf, depth: usize, roots: &mut Vec<(PathBuf, usize)>| {
        if p.is_dir() && !roots.iter().any(|(r, _)| *r == p) {
            roots.push((p, depth));
        }
    };
    // Steam: every library.
    let libraries = steam_libraries_on(&drives);
    for lib in libraries {
        add(lib.join("steamapps").join("common"), 1, &mut roots);
    }
    // The launchers' folders.
    let program_files: Vec<PathBuf> = ["ProgramFiles(x86)", "ProgramFiles", "ProgramW6432"]
        .iter()
        .filter_map(std::env::var_os)
        .map(PathBuf::from)
        .chain(drives.iter().flat_map(|d| [d.join("Program Files (x86)"), d.join("Program Files")]))
        .collect();
    for pf in &program_files {
        // Battle.net's `Call of Duty*`, Epic's folder.
        if let Ok(entries) = std::fs::read_dir(pf) {
            for e in entries.flatten() {
                let name = e.file_name().to_string_lossy().to_ascii_lowercase();
                if name.starts_with("call of duty") {
                    add(e.path(), 2, &mut roots);
                }
            }
        }
        add(pf.join("Epic Games"), 2, &mut roots);
    }
    for d in &drives {
        for s in ["Games", "SteamLibrary", "XboxGames", "Epic Games", "Battle.net", "Call of Duty"] {
            add(d.join(s), 3, &mut roots);
        }
    }
    // His own folders.
    let homes: Vec<PathBuf> = ["USERPROFILE", "HOME"].iter().filter_map(std::env::var_os).map(PathBuf::from).collect();
    for home in &homes {
        for sub in ["Documents", "Desktop", "Downloads", "OneDrive/Documents", "OneDrive/Desktop"] {
            add(home.join(sub), 3, &mut roots);
        }
    }
    // The games root and its folders.
    if let Some(games) = codlegacy_env::var_os("CODLEGACY_GAMES").filter(|g| !g.is_empty()) {
        add(
            PathBuf::from(games.to_string_lossy().trim_matches('"')),
            3,
            &mut roots,
        );
    }
    // The folders beside the ones set (F1: all his games in `D:\COD`, one
    // `<key>_game_files` each).
    for parent in set_parents(&game_paths::load()) {
        add(parent, 2, &mut roots);
    }
    // Program Files' other folders, then every drive.
    for pf in program_files {
        add(pf, 2, &mut roots);
    }
    if cfg!(windows) {
        for d in drives {
            add(d, 3, &mut roots);
        }
    } else {
        for home in homes {
            add(home, 3, &mut roots);
        }
        for m in ["/mnt", "/media"] {
            add(PathBuf::from(m), 3, &mut roots);
        }
    }
    roots
}

/// The folders holding the game folders set, each once (not a drive's
/// root: every drive is searched anyway).
fn set_parents(paths: &[(String, PathBuf)]) -> Vec<PathBuf> {
    let mut parents: Vec<PathBuf> = Vec::new();
    for (_, folder) in paths {
        if let Some(parent) = folder.parent()
            && parent.parent().is_some()
            && !parents.iter().any(|p| p == parent)
        {
            parents.push(parent.to_path_buf());
        }
    }
    parents
}

/// Searches `roots` (folder, levels to go down) for the games: each game's
/// surest folder, in the order found. A folder told as a game is not gone
/// into. `budget` caps the folders looked at.
pub fn scan_in(roots: &[(PathBuf, usize)], mut budget: usize) -> Vec<(String, PathBuf)> {
    let mut found: Vec<(&'static str, PathBuf, Evidence)> = Vec::new();
    let mut seen: std::collections::HashSet<PathBuf> = std::collections::HashSet::new();
    for (root, depth) in roots {
        let mut stack = vec![(root.clone(), 0usize)];
        while let Some((dir, level)) = stack.pop() {
            if budget == 0 {
                break;
            }
            budget -= 1;
            if !seen.insert(dir.clone()) {
                continue;
            }
            // (A drive or search root itself is told by its files only.)
            let id = identify(&dir).filter(|(_, ev)| level > 0 || *ev != Evidence::Name);
            if let Some((key, ev)) = id {
                match found.iter_mut().find(|(k, _, _)| *k == key) {
                    Some(slot) if ev < slot.2 => *slot = (key, dir.clone(), ev),
                    Some(_) => {}
                    None => found.push((key, dir.clone(), ev)),
                }
                if ev != Evidence::Name {
                    continue;
                }
            }
            if level >= *depth {
                continue;
            }
            let Ok(entries) = std::fs::read_dir(&dir) else { continue };
            let mut subs: Vec<PathBuf> = entries
                .flatten()
                .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()) && !skipped(&e.file_name().to_string_lossy()))
                .map(|e| e.path())
                .collect();
            // (Popped in name order.)
            subs.sort();
            subs.reverse();
            stack.extend(subs.into_iter().map(|p| (p, level + 1)));
        }
    }
    found.into_iter().map(|(k, p, _)| (k.to_owned(), p)).collect()
}

/// Finds every game's folder it can on this PC. Games not found are absent.
pub fn scan() -> Vec<(String, PathBuf)> {
    scan_in(&scan_roots(), 300_000)
}

/// Whether a set folder still counts (a mod's folder needs only to be
/// there).
fn still_good(key: &str, folder: &Path) -> bool {
    if MOD_KEYS.contains(&key) { folder.is_dir() } else { game_paths::looks_valid_for(key, folder) }
}

/// The games (not mods) with no good folder set.
pub fn missing() -> Vec<&'static str> {
    let paths = game_paths::load();
    GAMES
        .iter()
        .map(|(k, _, _)| *k)
        .filter(|k| !MOD_KEYS.contains(k))
        .filter(|k| !paths.iter().any(|(pk, p)| pk == k && still_good(k, p)))
        .collect()
}

/// Merges `found` into `paths` for the games in `missing`, never replacing
/// a folder he set that is still there: (the games set, the new paths).
pub fn merge(paths: &[(String, PathBuf)], found: &[(String, PathBuf)], missing: &[&str]) -> (Vec<String>, Vec<(String, PathBuf)>) {
    let mut paths = paths.to_vec();
    let mut set = Vec::new();
    for (key, folder) in found {
        if !missing.contains(&key.as_str()) {
            continue;
        }
        // Launch speed: only a folder the game can be loaded from (its zones,
        // or a banner-only game's program). A copy of Modern Warfare
        // Remastered's program on his desktop made the desktop its folder:
        // the next launch scanned for it again, and the map list walked the
        // whole desktop, the console's startup waiting on it for 20+ minutes.
        if !still_good(key, folder) {
            continue;
        }
        // (A folder of his that no longer looks like the game but is still
        // there is his to change.)
        if paths.iter().any(|(k, p)| k == key && p.is_dir()) {
            continue;
        }
        paths.retain(|(k, _)| k != key);
        paths.push((key.clone(), folder.clone()));
        set.push(key.clone());
    }
    (set, paths)
}

/// Scans for the games with no good folder set and writes what it found to
/// `game_paths.cfg` (a log line per game); returns the games set.
/// `CODLEGACY_NO_DETECT` turns it off.
///
/// Launch speed: a game the file has an empty line for (skipped, or not
/// found by an earlier launch's scan) is not scanned for again; the Game
/// Folders page's SCAN FOR GAMES still looks for it. Every launch scanned
/// every drive for the five games not installed, and the boot waited:
/// 342 to 736 s of a cold launch. A scan writes every game's line, so a
/// game it did not find is not scanned for on the next launch.
/// `CODLEGACY_SCAN_SKIPPED_GAMES=1` scans for them on every launch.
pub fn fill_missing() -> Vec<String> {
    if codlegacy_env::var_os("CODLEGACY_NO_DETECT").is_some() {
        return Vec::new();
    }
    let mut missing = missing();
    if !codlegacy_env::var_os("CODLEGACY_SCAN_SKIPPED_GAMES").is_some_and(|v| v == "1") {
        let skipped = game_paths::skipped();
        let before = missing.len();
        missing.retain(|key| !skipped.iter().any(|s| s == key));
        if missing.len() < before {
            diag::info!(
                Zone,
                "game scan: {} games not found before are not scanned for (Game Folders: SCAN FOR GAMES looks again)",
                before - missing.len()
            );
        }
    }
    if missing.is_empty() {
        return Vec::new();
    }
    let started = std::time::Instant::now();
    let found = scan();
    let (set, paths) = merge(&game_paths::load(), &found, &missing);
    for key in &set {
        if let Some((_, p)) = paths.iter().find(|(k, _)| k == key) {
            diag::info!(Zone, "game scan: found {key} at {}", p.display());
        }
    }
    diag::info!(
        Zone,
        "game scan: {} of {} missing games found in {:.1} s",
        set.len(),
        missing.len(),
        started.elapsed().as_secs_f64()
    );
    // (Every game's line written, found or not: see above.)
    if let Err(error) = game_paths::save(&paths) {
        diag::warn!(Zone, "game scan: {error}");
    }
    set
}

/// What a rescan from the menu found: every game found (new or moved)
/// written for the games with no good folder, the rest left alone.
pub struct Rescan {
    /// Games found on this PC.
    pub found: usize,
    /// Games whose folder it set (titles).
    pub set: Vec<String>,
}

/// Rescans (the Game Folders page's SCAN FOR GAMES): sets every missing
/// game it finds; folders he set are kept.
pub fn rescan() -> Rescan {
    let missing = missing();
    let found = scan();
    apply(game_paths::load(), &found, &missing)
}

/// Scans the one folder (or drive) he picked, three levels down (SCAN A
/// FOLDER): every game in it with no good folder set gets its folder,
/// told by its program and zones, not the folder's name. A folder he set
/// that no longer looks like the game is replaced too: he pointed here.
pub fn rescan_in(folder: &Path) -> Rescan {
    let missing = missing();
    let found = scan_in(&[(folder.to_path_buf(), 3)], 300_000);
    let mut paths = game_paths::load();
    paths.retain(|(k, _)| !missing.contains(&k.as_str()) || !found.iter().any(|(f, _)| f == k));
    apply(paths, &found, &missing)
}

/// Writes what a rescan found for the missing games (a log line each).
fn apply(paths: Vec<(String, PathBuf)>, found: &[(String, PathBuf)], missing: &[&str]) -> Rescan {
    let (set, paths) = merge(&paths, found, missing);
    for key in &set {
        if let Some((_, p)) = paths.iter().find(|(k, _)| k == key) {
            diag::info!(Zone, "game scan: found {key} at {}", p.display());
        }
    }
    if !set.is_empty()
        && let Err(error) = game_paths::save(&paths)
    {
        diag::warn!(Zone, "game scan: {error}");
    }
    let titles = set.iter().map(|k| GAMES.iter().find(|(g, _, _)| g == k).map_or(k.clone(), |(_, t, _)| (*t).to_owned())).collect();
    Rescan { found: found.len(), set: titles }
}

/// The launch's scan: not started, running in the background, or done
/// with the games it set.
enum Launch {
    Idle,
    Running(JoinHandle<Vec<String>>),
    Done(Vec<String>),
}

static LAUNCH: Mutex<Launch> = Mutex::new(Launch::Idle);
/// What `game_paths.cfg` had lines for before the launch's scan ran.
static ASKED_BEFORE: Mutex<Option<Vec<String>>> = Mutex::new(None);

fn note_asked() {
    if let Ok(mut asked) = ASKED_BEFORE.lock()
        && asked.is_none()
    {
        *asked = Some(game_paths::asked());
    }
}

/// Starts the launch's scan on its own thread (once).
pub fn start_background() {
    let Ok(mut launch) = LAUNCH.lock() else { return };
    if !matches!(*launch, Launch::Idle) {
        return;
    }
    note_asked();
    *launch = match std::thread::Builder::new().name("game-scan".into()).spawn(fill_missing) {
        Ok(handle) => Launch::Running(handle),
        Err(_) => Launch::Done(fill_missing()),
    };
}

/// Whether the launch's scan is running on its thread right now.
pub fn still_scanning() -> bool {
    LAUNCH
        .lock()
        .is_ok_and(|launch| matches!(&*launch, Launch::Running(handle) if !handle.is_finished()))
}

/// Waits for a running scan; `run` also runs it here when it never
/// started. The games it set.
fn finish(run: bool) -> Vec<String> {
    let Ok(mut launch) = LAUNCH.lock() else { return Vec::new() };
    let state = std::mem::replace(&mut *launch, Launch::Idle);
    let set = match state {
        Launch::Running(handle) => handle.join().unwrap_or_default(),
        Launch::Done(set) => set,
        Launch::Idle if run => {
            note_asked();
            fill_missing()
        }
        Launch::Idle => return Vec::new(),
    };
    *launch = Launch::Done(set.clone());
    set
}

/// The games the launch's scan set (waits for it; runs it here when it
/// was not started).
pub fn wait_background() -> Vec<String> {
    finish(true)
}

/// Waits for the launch's scan if [`start_background`] started it (never
/// scans itself: tools and tests that read the games root do not).
pub fn wait_if_started() {
    let _ = finish(false);
}

/// What `game_paths.cfg` had lines for before the launch's scan wrote it.
pub fn asked_before_scan() -> Vec<String> {
    ASKED_BEFORE.lock().ok().and_then(|a| a.clone()).unwrap_or_else(game_paths::asked)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("bo2mp-scan-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn touch(p: &Path) {
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, b"").unwrap();
    }

    #[test]
    fn one_parent_folder_of_games_fills_every_row() {
        // (F1: `D:\COD\<key>_game_files`, told by program and zones.)
        let root = tmp("parent");
        let cod = root.join("COD");
        touch(&cod.join("waw_game_files").join("CoDWaWmp.exe"));
        touch(&cod.join("waw_game_files").join("zone").join("english").join("common.ff"));
        touch(&cod.join("cod4_game_files").join("iw3mp.exe"));
        touch(&cod.join("cod4_game_files").join("zone").join("english").join("common.ff"));
        touch(&cod.join("ghosts_game_files").join("iw6mp64_ship.exe"));
        touch(&cod.join("ghosts_game_files").join("zone").join("common.ff"));
        // (An empty one is no game; an unknown game's is skipped.)
        std::fs::create_dir_all(cod.join("iw_game_files")).unwrap();
        touch(&cod.join("cod1_game_files").join("CoDMP.exe"));
        let mut found: Vec<String> = scan_in(&[(cod.clone(), 3)], 10_000).into_iter().map(|(k, _)| k).collect();
        found.sort();
        assert_eq!(found, ["cod4", "ghosts", "waw"]);
        // (Picking the drive above finds them too.)
        assert_eq!(scan_in(&[(root.clone(), 3)], 10_000).len(), 3);
        // F5: the folder beside one set is searched.
        let set = vec![("bo1".to_owned(), cod.join("bo1_game_files"))];
        assert_eq!(set_parents(&set), [cod.clone()]);
        assert!(set_parents(&[("bo1".to_owned(), PathBuf::from("D:\\"))]).is_empty());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn folders_are_told_by_their_names() {
        let tmp = tmp("names");
        let mk = |n: &str| {
            let d = tmp.join(n);
            std::fs::create_dir_all(d.join("zone")).unwrap();
            d
        };
        assert_eq!(game_from_name(&mk("mwr_game_files")), Some("mwr"));
        assert_eq!(game_from_name(&mk("iw_game_files")), Some("iw"));
        assert_eq!(game_from_name(&mk("aw_game_files")), Some("aw"));
        assert_eq!(game_from_name(&mk("Infinite Warfare")), Some("iw"));
        assert_eq!(game_from_name(&mk("Black Ops II")), Some("bo2"));
        assert_eq!(game_from_name(&mk("Call of Duty Black Ops III")), Some("bo3"));
        assert_eq!(game_from_name(&mk("Black Ops Cold War")), Some("cw"));
        assert_eq!(game_from_name(&mk("Black Ops")), Some("bo1"));
        let m = mk("mw3");
        assert_eq!(game_from_name(&m), Some("mw3"));
        touch(&m.join("zone").join("mp_jup_rebirth.ff"));
        assert_eq!(game_from_name(&m), Some("jup"));
        assert_eq!(game_from_name(&mk("Documents")), None);
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn games_are_told_by_their_files() {
        let t = tmp("identify");
        // Black Ops II: its program and zone folder.
        let bo2 = t.join("Call of Duty Black Ops II");
        touch(&bo2.join("t6mp.exe"));
        touch(&bo2.join("zone").join("all").join("patch_mp.ff"));
        assert_eq!(identify(&bo2), Some(("bo2", Evidence::ProgramAndZones)));
        // Only the needed files, under any name.
        let only = t.join("stuff");
        touch(&only.join("zone").join("all").join("patch_mp.ff"));
        assert_eq!(identify(&only), Some(("bo2", Evidence::Zones)));
        // Its program alone.
        let exe = t.join("x");
        touch(&exe.join("iw4mp.exe"));
        assert_eq!(identify(&exe), Some(("mw2", Evidence::Program)));
        // Maps of its own.
        let mw3 = t.join("y");
        touch(&mw3.join("zone").join("english").join("mp_seatown.ff"));
        assert_eq!(identify(&mw3), Some(("mw3", Evidence::Zones)));
        let jup = t.join("z");
        touch(&jup.join("zone").join("mp_jup_shipment.ff"));
        assert_eq!(identify(&jup), Some(("jup", Evidence::Zones)));
        // MWR and CoD4 share map names: the name tells them apart.
        let mwr = t.join("mwr_game_files");
        touch(&mwr.join("zone").join("mp_backlot.ff"));
        assert_eq!(identify(&mwr), Some(("mwr", Evidence::Zones)));
        let cod4 = t.join("w");
        touch(&cod4.join("zone").join("english").join("mp_backlot.ff"));
        assert_eq!(identify(&cod4), Some(("cod4", Evidence::Zones)));
        // A banner-only game's program.
        let cw = t.join("cw");
        touch(&cw.join("BlackOpsColdWar.exe"));
        assert_eq!(identify(&cw).map(|(k, _)| k), Some("cw"));
        // Name only, with a zone folder.
        let ghosts = t.join("ghosts_game_files");
        touch(&ghosts.join("zone").join("common.ff"));
        assert_eq!(identify(&ghosts), Some(("ghosts", Evidence::Name)));
        // Nothing.
        let none = t.join("Documents");
        std::fs::create_dir_all(&none).unwrap();
        assert_eq!(identify(&none), None);
        let _ = std::fs::remove_dir_all(&t);
    }

    #[test]
    fn scan_finds_games_three_levels_down_and_prefers_the_surest() {
        let t = tmp("walk");
        // A Steam library and a documents folder.
        let common = t.join("SteamLibrary").join("steamapps").join("common");
        let bo2 = common.join("Call of Duty Black Ops II");
        touch(&bo2.join("t6mp.exe"));
        touch(&bo2.join("zone").join("all").join("patch_mp.ff"));
        let docs = t.join("Documents");
        // (A name-only bo2 copy: the Steam one is surer.)
        touch(&docs.join("bo2_game_files").join("zone").join("readme.ff"));
        touch(&docs.join("a").join("b").join("mw2_game_files").join("zone").join("english").join("mp_rust.ff"));
        // Too deep.
        touch(&docs.join("a").join("b").join("c").join("d").join("iw3mp.exe"));
        // Skipped.
        touch(&docs.join("node_modules").join("aw").join("s1_mp64_ship.exe"));
        let found = scan_in(&[(docs.clone(), 3), (common.clone(), 1)], 10_000);
        let get = |k: &str| found.iter().find(|(g, _)| g == k).map(|(_, p)| p.clone());
        assert_eq!(get("bo2"), Some(bo2));
        assert_eq!(get("mw2"), Some(docs.join("a").join("b").join("mw2_game_files")));
        assert_eq!(get("cod4"), None);
        assert_eq!(get("aw"), None);
        let _ = std::fs::remove_dir_all(&t);
    }

    #[test]
    fn merge_keeps_his_folders() {
        let t = tmp("merge");
        let his = t.join("his");
        std::fs::create_dir_all(&his).unwrap();
        let paths = vec![("bo2".to_owned(), his.clone()), ("mw2".to_owned(), t.join("gone"))];
        let found = vec![
            ("bo2".to_owned(), t.join("found_bo2")),
            ("mw2".to_owned(), t.join("found_mw2")),
            ("mw3".to_owned(), t.join("found_mw3")),
            ("bo1".to_owned(), t.join("found_bo1")),
            // A copy of the program alone (his desktop): not a folder to load from.
            ("mwr".to_owned(), t.join("desktop")),
        ];
        for d in ["found_bo2", "found_mw2", "found_mw3", "found_bo1"] {
            std::fs::create_dir_all(t.join(d).join("zone")).unwrap();
        }
        touch(&t.join("desktop").join("h1_mp64_ship.exe"));
        let (set, out) = merge(&paths, &found, &["bo2", "mw2", "mw3", "mwr"]);
        assert_eq!(set, vec!["mw2".to_owned(), "mw3".to_owned()]);
        assert!(out.contains(&("bo2".to_owned(), his)));
        assert!(out.contains(&("mw2".to_owned(), t.join("found_mw2"))));
        assert!(!out.iter().any(|(k, _)| k == "bo1"));
        assert!(!out.iter().any(|(k, _)| k == "mwr"));
        let _ = std::fs::remove_dir_all(&t);
    }

    #[test]
    fn steam_library_list_is_read() {
        let vdf = "\"libraryfolders\"\n{\n\t\"0\"\n\t{\n\t\t\"path\"\t\t\"C:\\\\Program Files (x86)\\\\Steam\"\n\t}\n\t\"1\"\n\t{\n\t\t\"path\"\t\t\"D:\\\\SteamLibrary\"\n\t}\n}\n";
        assert_eq!(vdf_libraries(vdf), vec![PathBuf::from("C:\\Program Files (x86)\\Steam"), PathBuf::from("D:\\SteamLibrary")]);
    }
}
