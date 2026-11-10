//! `music.cfg` and the user's music folders.
//!
//! Where things are looked for:
//! * the music folder: `CODLEGACY_MUSIC_DIR`, else `<exe dir>/music`; its
//!   `menu/`, `lobby/`, `match/` and `zombies/` subfolders feed those
//!   playlists (`.mp3`, `.ogg`, `.wav`, `.flac`); `zombies/` is the Zombies
//!   front end's (never mixed with the menu's);
//! * the config: `CODLEGACY_MUSIC_CFG`, else `<exe dir>/music.cfg`, else
//!   `<music folder>/music.cfg`. A commented template is written beside the
//!   exe when none exists.
//!
//! The config is `key = value` lines (`//` comments, or `#` at a line's start):
//! ```text
//! enabled = 1            // 0: no director music at all
//! shuffle = 1            // 0: play each playlist in order
//! menu_level = 1.0       // per-context level, under music_volume
//! lobby_level = 1.0
//! match_level = 1.0      // a trim on in-match music's own low base gain
//! zombies_level = 1.0    // the Zombies front end's music
//! fade_in = 2.0          // seconds
//! fade_out = 1.5
//! crossfade = 4.0
//! menu = defaults        // a playlist line; any line for a context replaces
//! menu = music/menu      // its defaults. An entry is `defaults`, a game
//! menu = t6:mus_mp_frontend  // alias (`t6:`, `t5:`, `iw4:`, `iw5:`, `ghosts:`), a
//! lobby = D:/Music/x.mp3 // folder, or a file (relative to the exe dir).
//! match = none           // an empty playlist
//! zombies = defaults     // the Zombies front end (never the menu's tracks)
//! intro_level = 1.0      // the map intro track (a short track at join), a trim on in-match music's base gain
//! intro_seconds = 50     // it is cut to this length and fades out
//! intro = t5:mp_nuked music/nuketown.mp3   // one map's intro (`game:map`, then entries)
//! ```

use std::path::{Path, PathBuf};

use asset_core::AssetNamespace;

use super::logic::{FadeTimings, MusicContext};
use super::stream::MUSIC_EXTENSIONS;

/// The game tracks the code knows how to load: each game's main-menu music,
/// from the front end's bank (BO2's own; BO1's, MW2's and MW3's are taken
/// in by `asset_audio::menu_music_guests`).
pub const GAME_TRACKS: [(AssetNamespace, &str, &str); 4] = [
    (AssetNamespace::T6, "mus_mp_frontend", "Black Ops II"),
    (AssetNamespace::T5, "mus_mp_frontend", "Black Ops"),
    (AssetNamespace::Iw4, "music_mainmenu_mp", "Modern Warfare 2"),
    (AssetNamespace::Iw5, "music_mainmenu_mp", "Modern Warfare 3"),
];

/// Where a game's Zombies main-menu music is read from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ZombiesFile {
    /// A Black Ops II sound bank in the install's `sound` folder
    /// (`<bank>.all.sabs`) and the entry id its alias plays (`SND_HashName`
    /// of the alias's file name, `sab_t6::sound_hash`).
    T6Bank { bank: &'static str, entry: u32 },
    /// A file in the game's IWDs (`sound/<path>`).
    Iwd(&'static str),
}

/// One game's Zombies main-menu music, known by name.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ZombiesMenuTrack {
    pub ns: AssetNamespace,
    /// The alias the game plays it through.
    pub alias: &'static str,
    pub file: ZombiesFile,
    /// Why this is that game's Zombies menu music (for the log).
    pub why: &'static str,
}

/// Black Ops II's Zombies menu music: alias `mus_mp_frontend` in the Zombies
/// front end's own bank `zmb_code_post_gfx` (zone `code_post_gfx_zm`), file
/// `raw\sound\mus\zmb\damned_100ae.SL65.pc.snd` — "Damned 100AE", a 192 s
/// stereo FLAC loop, the only music stream over 60 s in that bank (the
/// others are the Nuketown and TranZit load beds, 43.5 s and 22.1 s). Read
/// from his `code_post_gfx_zm.ff` and `zmb_code_post_gfx.all.sabs`
/// (2026-10-26). It shares its alias name with the multiplayer front end's
/// track, which lives in a different bank (`mpl_code_post_gfx`).
pub const T6_ZOMBIES_ENTRY: u32 = 0xc0e6_24ad;

/// Each game's Zombies main-menu music, by name (picked first; the pattern
/// search, `zombies_fallback`, only when a game's own file is not there).
pub const ZOMBIES_MENU_TRACKS: [ZombiesMenuTrack; 3] = [
    ZombiesMenuTrack {
        ns: AssetNamespace::T6,
        alias: "mus_mp_frontend",
        file: ZombiesFile::T6Bank { bank: "zmb_code_post_gfx", entry: T6_ZOMBIES_ENTRY },
        why: "the Zombies front end's own menu loop (zmb_code_post_gfx, damned_100ae)",
    },
    ZombiesMenuTrack {
        ns: AssetNamespace::T5,
        alias: "mus_zombietime",
        file: ZombiesFile::Iwd("mus/zombie_theatre/mus_theatre_underscore_l.wav"),
        why: "the main menu's Zombies music (frontend.ff alias mus_zombietime, its only Zombies music loop)",
    },
    ZombiesMenuTrack {
        ns: AssetNamespace::T4,
        alias: "music_mainmenu",
        file: ZombiesFile::Iwd("stream/music/mission/sniper/final/mx_brave_soldat.wav"),
        why: "Nazi Zombies has no menu of its own; it is started from the main menu, whose loop this is (ui.ff soundloop music_mainmenu)",
    },
];

/// The Zombies menu music the director looks for, for the "nothing to
/// play" warning.
pub const ZOMBIES_WANTED: &str = "Black Ops II `mus_mp_frontend` in zmb_code_post_gfx (damned_100ae), \
Black Ops `mus_zombietime` (mus/zombie_theatre/mus_theatre_underscore_l.wav), \
World at War `music_mainmenu` (mx_brave_soldat.wav), music/zombies";

/// File names of Zombies music that is not menu music: round, wave and
/// game-over stingers, map-start splashes, deaths, easter eggs.
const ZOMBIES_STINGERS: [&str; 12] = [
    "splash", "round", "wave", "game_over", "gameover", "death", "stg", "sting", "_end", "egg", "laststand", "intro",
];

fn is_stinger(stem: &str) -> bool {
    ZOMBIES_STINGERS.iter().any(|k| stem.contains(k))
}

/// A game's IWD sound path that may be Zombies menu music (the fallback
/// search): a music file (under `mus/`, `music/` or `stream/music/`, or a
/// `mus_` / `mx_` / `music_` file) whose path names Zombies (`zombie`,
/// `zmb`, `nazi`), whose file name names the menus (`frontend`, `menu`,
/// `lobby`, `title`) and is no stinger (`splash`, `round`, `game_over`, ...).
pub fn is_zombies_menu_path(path: &str) -> bool {
    let p = path.replace('\\', "/").to_ascii_lowercase();
    let file = p.rsplit('/').next().unwrap_or(&p);
    let stem = file.rsplit_once('.').map_or(file, |(s, _)| s);
    let music = p.starts_with("mus/")
        || p.starts_with("music/")
        || p.starts_with("stream/music/")
        || stem.starts_with("mus_")
        || stem.starts_with("mx_")
        || stem.starts_with("music_");
    let zombies = ["zombie", "zmb", "nazi"].iter().any(|k| p.contains(k));
    let menu = ["frontend", "menu", "lobby", "title"].iter().any(|k| stem.contains(k));
    music && zombies && menu && !is_stinger(stem)
}

/// A front-end bank alias that may be Zombies menu music (the fallback
/// search): a music alias (`mus_`, `music_`, `mx_`) naming Zombies
/// (`zombie`, `zmb`, `zm_`, `_zm`, `nazi`) and the menus (`frontend`,
/// `menu`, `lobby`, `title`), and no stinger.
pub fn is_zombies_menu_alias(name: &str) -> bool {
    let n = name.to_ascii_lowercase();
    let music = n.starts_with("mus_") || n.starts_with("music_") || n.starts_with("mx_");
    let zombies = ["zombie", "zmb", "zm_", "_zm", "nazi"].iter().any(|k| n.contains(k));
    let menu = ["frontend", "menu", "lobby", "title"].iter().any(|k| n.contains(k));
    music && zombies && menu && !is_stinger(&n)
}

/// What a game's Zombies menu music came to.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ZombiesChoice {
    /// Its own track, known by name, is in the game's files.
    Known,
    /// Its own track is not there; the first fallback hit (sorted) instead.
    Fallback(String),
    /// Neither.
    Nothing,
}

/// Choose a game's Zombies menu music: its known track when the game has
/// it, else the first of the fallback search's hits (paths or aliases)
/// that pass `is_zombies_menu_path` / `is_zombies_menu_alias`, else none.
pub fn choose_zombies_track(known_found: bool, hits: &[String]) -> ZombiesChoice {
    if known_found {
        return ZombiesChoice::Known;
    }
    let mut hits: Vec<&String> =
        hits.iter().filter(|h| is_zombies_menu_path(h) || is_zombies_menu_alias(h)).collect();
    hits.sort();
    hits.first().map_or(ZombiesChoice::Nothing, |h| ZombiesChoice::Fallback((*h).clone()))
}

/// The known Zombies menu track of a game.
#[cfg(test)]
pub fn zombies_menu_track(ns: AssetNamespace) -> Option<&'static ZombiesMenuTrack> {
    ZOMBIES_MENU_TRACKS.iter().find(|t| t.ns == ns)
}

pub fn game_label(ns: AssetNamespace) -> &'static str {
    match ns {
        AssetNamespace::T6 => "Black Ops II",
        AssetNamespace::T5 => "Black Ops",
        AssetNamespace::Iw4 => "Modern Warfare 2",
        AssetNamespace::Iw5 => "Modern Warfare 3",
        AssetNamespace::Iw6 => "Ghosts",
        AssetNamespace::T4 => "World at War",
        AssetNamespace::Iw3 => "Call of Duty 4",
        AssetNamespace::S1 => "Advanced Warfare",
        AssetNamespace::T7 => "Black Ops III",
        AssetNamespace::Jup => "Modern Warfare III (2023)",
        AssetNamespace::H1 => "Modern Warfare Remastered",
        AssetNamespace::Iw7 => "Infinite Warfare",
    }
}

fn ns_of_prefix(prefix: &str) -> Option<AssetNamespace> {
    match prefix.to_ascii_lowercase().as_str() {
        "t6" | "bo2" => Some(AssetNamespace::T6),
        "t5" | "bo1" => Some(AssetNamespace::T5),
        "iw4" | "mw2" => Some(AssetNamespace::Iw4),
        "iw5" | "mw3" => Some(AssetNamespace::Iw5),
        _ => None,
    }
}

/// One playlist line.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Entry {
    Defaults,
    None,
    Game(AssetNamespace, String),
    /// A Ghosts music alias (`ghosts:music_mainmenu_squad`); see `ghosts.rs`.
    Ghosts(String),
    Path(PathBuf),
}

#[derive(Clone, Debug, PartialEq)]
pub struct MusicConfig {
    pub enabled: bool,
    pub shuffle: bool,
    pub levels: [f32; 4],
    /// The map intro's level and its longest length in seconds.
    pub intro_level: f32,
    pub intro_seconds: f64,
    /// `intro =` lines: (game prefix, zone name, entries), over the table.
    pub intro_maps: Vec<(String, String, Vec<Entry>)>,
    pub timings: FadeTimings,
    /// Per context (menu, lobby, match, zombies): `None` = the defaults.
    pub playlists: [Option<Vec<Entry>>; 4],
    /// Where it was read from, if anywhere.
    pub source: Option<PathBuf>,
    pub warnings: Vec<String>,
}

impl Default for MusicConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            shuffle: true,
            levels: [1.0, 1.0, 1.0, 1.0],
            intro_level: super::map_intro::DEFAULT_LEVEL,
            intro_seconds: super::map_intro::DEFAULT_SECONDS,
            intro_maps: Vec::new(),
            timings: FadeTimings::default(),
            playlists: [None, None, None, None],
            source: None,
            warnings: Vec::new(),
        }
    }
}

fn parse_bool(v: &str) -> Option<bool> {
    match v.to_ascii_lowercase().as_str() {
        "1" | "true" | "yes" | "on" => Some(true),
        "0" | "false" | "no" | "off" => Some(false),
        _ => None,
    }
}

pub fn parse_entry(value: &str, base: &Path) -> Entry {
    let v = value.trim().trim_matches('"');
    if v.eq_ignore_ascii_case("defaults") {
        return Entry::Defaults;
    }
    if v.eq_ignore_ascii_case("none") || v.is_empty() {
        return Entry::None;
    }
    // `ghosts:alias` / `iw6:alias`.
    if let Some((prefix, alias)) = v.split_once(':')
        && matches!(prefix.to_ascii_lowercase().as_str(), "ghosts" | "iw6")
        && !alias.is_empty()
    {
        return Entry::Ghosts(alias.to_owned());
    }
    // `t6:alias` (but not a Windows drive, `C:\...`).
    if let Some((prefix, alias)) = v.split_once(':')
        && prefix.len() > 1
        && let Some(ns) = ns_of_prefix(prefix)
        && !alias.is_empty()
    {
        return Entry::Game(ns, alias.to_owned());
    }
    let path = PathBuf::from(v);
    Entry::Path(if path.is_absolute() { path } else { base.join(path) })
}

impl MusicConfig {
    /// Parses `text`; relative paths are taken from `base`.
    pub fn parse(text: &str, base: &Path) -> Self {
        let mut cfg = Self::default();
        for (n, raw) in text.lines().enumerate() {
            let line = raw.split("//").next().unwrap_or("").trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let Some((key, value)) = line.split_once('=') else {
                cfg.warnings.push(format!("line {}: no `=` in `{line}`", n + 1));
                continue;
            };
            let (key, value) = (key.trim().to_ascii_lowercase(), value.trim());
            let num = value.parse::<f64>().ok().filter(|v| v.is_finite() && *v >= 0.0);
            let mut bad = false;
            match key.as_str() {
                "enabled" => match parse_bool(value) {
                    Some(b) => cfg.enabled = b,
                    None => bad = true,
                },
                "shuffle" => match parse_bool(value) {
                    Some(b) => cfg.shuffle = b,
                    None => bad = true,
                },
                "menu_level" | "lobby_level" | "match_level" | "zombies_level" => match num {
                    Some(v) => {
                        let slot = match key.as_str() {
                            "menu_level" => 0,
                            "lobby_level" => 1,
                            "match_level" => 2,
                            _ => 3,
                        };
                        cfg.levels[slot] = v.min(1.0) as f32;
                    }
                    None => bad = true,
                },
                "intro_level" => match num {
                    Some(v) => cfg.intro_level = v.min(1.0) as f32,
                    None => bad = true,
                },
                "intro_seconds" => match num {
                    Some(v) => cfg.intro_seconds = v.min(600.0),
                    None => bad = true,
                },
                "intro" => {
                    let (map, rest) = value.split_once(char::is_whitespace).unwrap_or((value, ""));
                    let (game, stem) = super::map_intro::split_zone(map);
                    match game {
                        Some(game) if !rest.trim().is_empty() => {
                            let entries = rest.split(',').map(|e| parse_entry(e, base)).collect();
                            cfg.intro_maps.push((game, stem, entries));
                        }
                        _ => bad = true,
                    }
                }
                "fade_in" | "fade_out" | "crossfade" | "prefetch" => match num {
                    Some(v) => {
                        let v = v.min(60.0);
                        match key.as_str() {
                            "fade_in" => cfg.timings.fade_in = v,
                            "fade_out" => cfg.timings.fade_out = v,
                            "crossfade" => cfg.timings.crossfade = v,
                            _ => cfg.timings.prefetch = v.max(5.0),
                        }
                    }
                    None => bad = true,
                },
                "menu" | "lobby" | "match" | "zombies" => {
                    let slot = match key.as_str() {
                        "menu" => 0,
                        "lobby" => 1,
                        "match" => 2,
                        _ => 3,
                    };
                    cfg.playlists[slot]
                        .get_or_insert_with(Vec::new)
                        .push(parse_entry(value, base));
                }
                _ => cfg.warnings.push(format!("line {}: unknown key `{key}`", n + 1)),
            }
            if bad {
                cfg.warnings
                    .push(format!("line {}: bad value `{value}` for `{key}`", n + 1));
            }
        }
        cfg
    }

    pub fn level(&self, context: MusicContext) -> f32 {
        context.slot().map_or(0.0, |s| self.levels[s])
    }
}

/// The folder the exe is in (the working directory when unknown).
pub fn exe_dir() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(Path::to_path_buf))
        .unwrap_or_else(|| PathBuf::from("."))
}

pub fn music_root(exe_dir: &Path) -> PathBuf {
    codlegacy_env::var_os("CODLEGACY_MUSIC_DIR")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| exe_dir.join("music"))
}

/// The config's path: the override, else beside the exe, else in the music
/// folder; `(path, exists)`.
pub fn config_path(exe_dir: &Path, root: &Path) -> (PathBuf, bool) {
    if let Some(p) = codlegacy_env::var_os("CODLEGACY_MUSIC_CFG").filter(|v| !v.is_empty()) {
        let p = PathBuf::from(p);
        let exists = p.is_file();
        return (p, exists);
    }
    let beside = exe_dir.join("music.cfg");
    if beside.is_file() {
        return (beside, true);
    }
    let inside = root.join("music.cfg");
    if inside.is_file() {
        return (inside, true);
    }
    (beside, false)
}

pub const TEMPLATE: &str = "\
// Music director (bo2mp). Lines are `key = value`; `//` starts a comment.
// Drop your own music (.mp3 .ogg .wav .flac) into music/menu, music/lobby,
// music/match and music/zombies beside the exe (or set CODLEGACY_MUSIC_DIR); they
// join the playlists below. Any `menu =`, `lobby =`, `match =` or `zombies =`
// line replaces that playlist's defaults; list `defaults` to keep them too.
// The Zombies front end plays only the zombies playlist (the Zombies menu
// music of Black Ops II, Black Ops and World at War found in your games, and
// music/zombies), never the menu's; with nothing there it is silent.
enabled = 1
shuffle = 1
menu_level = 1.0
lobby_level = 1.0
// match_level trims in-match music, which already sits far under the menus
match_level = 1.0
zombies_level = 1.0
fade_in = 2.0
fade_out = 1.5
crossfade = 4.0
// menu = defaults
// menu = t6:mus_mp_frontend
// menu = ghosts:music_mainmenu_squad
// lobby = music/lobby
// match = none
// zombies = defaults
// zombies = music/zombies
// Map intro: a short track when you join a match, chosen per map (see
// map_intro.cfg in the source; intro = ... lines here override it). Your own
// files named after a map (mp_nuked.mp3) in music/intro are played first.
// intro_level = 1.0
// intro_seconds = 50
// intro = t5:mp_nuked music/nuketown.mp3
";

/// Is `path` a music file this build reads?
pub fn is_music_file(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| MUSIC_EXTENSIONS.iter().any(|m| e.eq_ignore_ascii_case(m)))
}

/// The music files in `dir` (not recursive), sorted; and the files there
/// that are not music, so the manifest can say so.
pub fn scan_dir(dir: &Path) -> (Vec<PathBuf>, Vec<PathBuf>) {
    let Ok(read) = std::fs::read_dir(dir) else {
        return (Vec::new(), Vec::new());
    };
    let mut music = Vec::new();
    let mut other = Vec::new();
    for entry in read.flatten() {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        if path.file_name().is_some_and(|n| n == "music.cfg" || n.to_string_lossy().starts_with('.')) {
            continue;
        }
        if is_music_file(&path) {
            music.push(path);
        } else {
            other.push(path);
        }
    }
    music.sort();
    other.sort();
    (music, other)
}

/// Game folders whose formats this build cannot read, by folder name
/// (Ghosts is read now, for its menu music).
pub fn unreadable_game(name: &str) -> Option<&'static str> {
    let n = name.to_ascii_lowercase();
    let has = |s: &str| n.contains(s);
    if has("advanced warfare") || has("advanced_warfare") || n == "aw" || has("s1_mp") || n.ends_with(" aw") {
        Some("Advanced Warfare")
    } else if has("infinite warfare") || has("infinite_warfare") || n == "iw" || n == "iw7" || n.ends_with(" iw") {
        Some("Infinite Warfare")
    } else {
        None
    }
}

/// Game folders this build reads, but whose music the director does not
/// load (no menu-music loader for them).
pub fn unwired_game(name: &str) -> Option<&'static str> {
    let n = name.to_ascii_lowercase();
    if n.contains("world at war") || n == "waw" || n == "t4" || n.contains("cod5") {
        Some("World at War")
    } else if n.contains("call of duty 4") || n == "cod4" || n == "iw3" || n.contains("cod4") {
        Some("Call of Duty 4")
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_settings_and_playlists() {
        let base = Path::new("/games/bo2mp");
        let cfg = MusicConfig::parse(
            "enabled = 1\nshuffle=0 // in order\nmatch_level = 0.1\ncrossfade = 6\n\
             menu = defaults\nmenu = t6:mus_mp_frontend\nmenu = music/extra\n\
             lobby = /abs/song.mp3\nmatch = none\nbogus = 1\nfade_in = x\n",
            base,
        );
        assert!(cfg.enabled);
        assert!(!cfg.shuffle);
        assert!((cfg.levels[2] - 0.1).abs() < 1e-6);
        assert_eq!(cfg.timings.crossfade, 6.0);
        assert_eq!(
            cfg.playlists[0],
            Some(vec![
                Entry::Defaults,
                Entry::Game(AssetNamespace::T6, "mus_mp_frontend".into()),
                Entry::Path(base.join("music/extra")),
            ])
        );
        assert_eq!(cfg.playlists[1], Some(vec![Entry::Path("/abs/song.mp3".into())]));
        assert_eq!(cfg.playlists[2], Some(vec![Entry::None]));
        assert_eq!(cfg.warnings.len(), 2, "{:?}", cfg.warnings);
        let cfg = MusicConfig::parse(
            "intro_level = 0.5\nintro_seconds = 40\nintro = BO1:mp_nuked t5:mus_mp_frontend, a.mp3\nintro = mp_x t5:y\n",
            base,
        );
        assert_eq!((cfg.intro_level, cfg.intro_seconds), (0.5, 40.0));
        assert_eq!(cfg.intro_maps.len(), 1);
        assert_eq!(cfg.intro_maps[0].0, "t5");
        assert_eq!(cfg.intro_maps[0].2.len(), 2);
        assert_eq!(cfg.warnings.len(), 1, "{:?}", cfg.warnings);
    }

    #[test]
    fn template_parses_cleanly() {
        let cfg = MusicConfig::parse(TEMPLATE, Path::new("."));
        assert!(cfg.warnings.is_empty(), "{:?}", cfg.warnings);
        assert_eq!(cfg.playlists, [None, None, None, None]);
    }

    #[test]
    fn zombies_playlist_and_level_parse() {
        let base = Path::new("/games/bo2mp");
        let cfg = MusicConfig::parse("zombies = defaults\nzombies = music/zombies\nzombies_level = 0.6\n", base);
        assert!(cfg.warnings.is_empty(), "{:?}", cfg.warnings);
        assert_eq!(cfg.playlists[3], Some(vec![Entry::Defaults, Entry::Path(base.join("music/zombies"))]));
        assert_eq!(cfg.playlists[0], None, "the menu playlist is untouched");
        assert!((cfg.level(MusicContext::Zombies) - 0.6).abs() < 1e-6);
        assert_eq!(MusicConfig::default().level(MusicContext::Zombies), 1.0);
    }

    #[test]
    fn zombies_menu_aliases_are_told_from_the_rest() {
        for yes in ["mus_zmb_frontend", "mus_zm_menu", "MUS_ZOMBIE_LOBBY", "mx_nazi_zombie_title"] {
            assert!(is_zombies_menu_alias(yes), "{yes}");
        }
        for no in [
            "mus_mp_frontend",
            "music_mainmenu_mp",
            "mus_zombie_splash_screen",
            "mus_zombie_round_start",
            "mus_zmb_game_over",
            "mus_zmb_menu_stg",
            "zmb_frontend_click",
            "uin_zm_menu_select",
        ] {
            assert!(!is_zombies_menu_alias(no), "{no}");
        }
    }

    #[test]
    fn zombies_menu_paths_are_told_from_the_rest() {
        assert!(is_zombies_menu_path("music/nazi_zombie/mx_menu_theme.wav"));
        assert!(is_zombies_menu_path("MUS\\Zombie\\mus_zombie_title.WAV"));
        assert!(is_zombies_menu_path("stream/music/zombie/mx_zombie_menu.wav"));
        // Stingers: the map-start splashes, rounds, game over.
        assert!(!is_zombies_menu_path("mus/zombie/zombie_global/mus_zombie_splash_screen.wav"));
        assert!(!is_zombies_menu_path("stream/music/mission/zombie/mx_splash_screen.wav"));
        assert!(!is_zombies_menu_path("mus/zombie/zombie_global/mus_zombie_round_start.wav"));
        assert!(!is_zombies_menu_path("stream/music/mission/zombie/mx_zombie_menu_game_over.wav"));
        // Not Zombies, or not music.
        assert!(!is_zombies_menu_path("mus/mp/mus_mp_frontend.wav"));
        assert!(!is_zombies_menu_path("vox/zombie/menu_vox_01.wav"));
    }

    #[test]
    fn each_game_has_its_known_zombies_menu_track() {
        let bo2 = zombies_menu_track(AssetNamespace::T6).expect("Black Ops II");
        assert_eq!(bo2.alias, "mus_mp_frontend");
        // The entry id is the hash of the file name the zone gives the alias.
        assert_eq!(
            bo2.file,
            ZombiesFile::T6Bank { bank: "zmb_code_post_gfx", entry: T6_ZOMBIES_ENTRY }
        );
        let bo1 = zombies_menu_track(AssetNamespace::T5).expect("Black Ops");
        assert_eq!(bo1.file, ZombiesFile::Iwd("mus/zombie_theatre/mus_theatre_underscore_l.wav"));
        let waw = zombies_menu_track(AssetNamespace::T4).expect("World at War");
        assert_eq!(waw.file, ZombiesFile::Iwd("stream/music/mission/sniper/final/mx_brave_soldat.wav"));
        // No stinger among them, and no multiplayer front-end file.
        for t in ZOMBIES_MENU_TRACKS {
            if let ZombiesFile::Iwd(path) = t.file {
                assert!(!path.contains("splash") && !path.contains("round") && !path.contains("_mp"), "{path}");
            }
        }
        assert!(zombies_menu_track(AssetNamespace::Iw4).is_none());
        // `SND_HashName` of the file name `code_post_gfx_zm` gives the alias.
        let hash = |name: &str| {
            name.bytes().fold(0x1505u32, |h, b| u32::from(b.to_ascii_lowercase()).wrapping_add(h.wrapping_mul(0x1003f)))
        };
        assert_eq!(hash(r"raw\sound\mus\zmb\damned_100ae.SL65.pc.snd"), T6_ZOMBIES_ENTRY);
    }

    #[test]
    fn known_track_first_pattern_only_as_fallback() {
        let hits = vec![
            "mus/zombie/zombie_global/mus_zombie_splash_screen.wav".to_owned(),
            "mus/zombie/zombie_global/mus_zombie_round_start.wav".to_owned(),
            "mus/zombie/zombie_global/mus_zombie_title.wav".to_owned(),
            "mus/zombie/zombie_global/mus_zombie_menu.wav".to_owned(),
        ];
        assert_eq!(choose_zombies_track(true, &hits), ZombiesChoice::Known);
        assert_eq!(
            choose_zombies_track(false, &hits),
            ZombiesChoice::Fallback("mus/zombie/zombie_global/mus_zombie_menu.wav".to_owned())
        );
        // Only stingers: nothing (never a splash or a round start).
        assert_eq!(choose_zombies_track(false, &hits[..2]), ZombiesChoice::Nothing);
        assert_eq!(choose_zombies_track(false, &[]), ZombiesChoice::Nothing);
        assert_eq!(
            choose_zombies_track(false, &["mus_zmb_frontend".to_owned(), "mus_mp_frontend".to_owned()]),
            ZombiesChoice::Fallback("mus_zmb_frontend".to_owned())
        );
    }

    #[test]
    fn drive_letters_are_paths() {
        assert!(matches!(parse_entry("C:/Music/a.mp3", Path::new(".")), Entry::Path(_)));
        assert_eq!(
            parse_entry("mw2:music_mainmenu_mp", Path::new(".")),
            Entry::Game(AssetNamespace::Iw4, "music_mainmenu_mp".into())
        );
        assert_eq!(
            parse_entry("ghosts:music_mainmenu_squad", Path::new(".")),
            Entry::Ghosts("music_mainmenu_squad".into())
        );
        assert_eq!(parse_entry("IW6:music_mainmenu_mp", Path::new(".")), Entry::Ghosts("music_mainmenu_mp".into()));
    }

    #[test]
    fn folder_names() {
        assert_eq!(unreadable_game("Call of Duty Advanced Warfare"), Some("Advanced Warfare"));
        assert_eq!(unreadable_game("Call of Duty Ghosts"), None);
        assert_eq!(unreadable_game("aw"), Some("Advanced Warfare"));
        assert_eq!(unreadable_game("Call of Duty Infinite Warfare"), Some("Infinite Warfare"));
        assert_eq!(unreadable_game("Call of Duty Black Ops II"), None);
        assert_eq!(unwired_game("Call of Duty World at War"), Some("World at War"));
        assert!(is_music_file(Path::new("x/Rising Tide.MP3")));
        assert!(!is_music_file(Path::new("x/cover.jpg")));
    }
}
