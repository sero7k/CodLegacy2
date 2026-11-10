//! bo2mp shell: Options - graphics, sound, mouse and keyboard, controller,
//! account - as left / right selectors on his settings
//! (`frame::GameSettings`). The screens only queue a change; `sync_settings`
//! (the one system that writes them) carries it out, and the systems that
//! watch his settings apply it: the window (`ui::options`), shadows, depth
//! of field, bloom and the master volume (`console::user_settings`), the
//! brightness (`film_vision_view`), the music (`audio::music`), the mouse
//! and the pad (`console`), the vibration (`audio::rumble`); the settings
//! file (`settings.cfg`) is saved by `console::user_settings` whenever they
//! change. Unlock All lives in his stats file (`bridge`).
//!
//! The same rows serve the front end and the pause menu in a match (which
//! leaves ACCOUNT out: its rows need the front end's scripts).

use frame::{DisplayResolution, GameSettings};

use super::{Act, Row, Setting};

pub(crate) const TABS: [&str; 6] = ["GRAPHICS", "SOUND", "MOUSE & KEYBOARD", "CONTROLLER", "ACCOUNT", "GAME FOLDERS"];

/// bo2mp: the front end's OPTIONS page, as Ghosts': the category panel at the
/// left (a category a tab of ours, in `TABS`' order), its settings card at
/// the right.
pub(crate) const CATEGORIES: [&str; 6] = ["VIDEO OPTIONS", "AUDIO OPTIONS", "MOUSE & KEYBOARD", "CONTROLLER OPTIONS", "ACCOUNT", "GAME FOLDERS"];

/// VIDEO OPTIONS' card: the first `VIDEO_MAIN` GRAPHICS rows (resolution,
/// display mode, v-sync, field of view, brightness), then the ADVANCED VIDEO
/// row that opens the rest on their own page (`ADVANCED_PAGE`, a `page`
/// past the tabs).
pub(crate) const VIDEO_MAIN: usize = 5;
pub(crate) const ADVANCED_PAGE: usize = TABS.len();
pub(crate) const ADVANCED_TITLE: &str = "ADVANCED VIDEO";

/// The tab a front-end page's rows belong to (ADVANCED VIDEO: GRAPHICS).
pub(crate) fn tab_of(page: usize) -> usize {
    match page {
        ADVANCED_PAGE => 0,
        super::keys::KEYS_PAGE => 2,
        PREPARE_PAGE => FOLDERS_TAB,
        p => p.min(TABS.len() - 1),
    }
}

/// MOUSE & KEYBOARD's card: its rows, then the KEY BINDINGS row that opens
/// its page (`keys::KEYS_PAGE`).
pub(crate) const KEYS_ROW: usize = 2;

/// The front end's rows of a page: a tab's own rows, VIDEO OPTIONS split into
/// its card and ADVANCED VIDEO (the `k` in each row's act is still the row's
/// number in its tab, so `change` and the pause menu's tabs are as before).
pub(crate) fn page_rows(page: usize, s: &GameSettings) -> Vec<Row> {
    match page {
        0 => {
            let mut r: Vec<Row> = rows(0, s).into_iter().take(VIDEO_MAIN).collect();
            r.push(Row {
                label: ADVANCED_TITLE.to_owned(),
                sub: "Shadows, depth of field, bloom, texture quality, player models, soldiers, animated cards, the Black Ops III renderer and upscaling (FSR 1).".to_owned(),
                value: None,
                act: Act::SubPage(ADVANCED_PAGE),
            });
            r
        }
        ADVANCED_PAGE => rows(0, s).into_iter().skip(VIDEO_MAIN).collect(),
        2 => {
            let mut r = rows(2, s);
            r.push(Row {
                label: super::keys::KEYS_TITLE.to_owned(),
                sub: "Black Ops II's PC controls: a key for each action (movement, combat, interface).".to_owned(),
                value: None,
                act: Act::SubPage(super::keys::KEYS_PAGE),
            });
            r
        }
        super::keys::KEYS_PAGE => super::keys::rows(&super::keys::snapshot()),
        PREPARE_PAGE => prepare_rows(s),
        p => rows(p, s),
    }
}

/// The rows a list `max` rows high shows with `focus` in it: `first..last`
/// (the focus kept near the middle, the window inside the list).
pub(crate) fn scroll_window(n: usize, focus: usize, max: usize) -> (usize, usize) {
    if max == 0 || n <= max {
        return (0, n);
    }
    let first = focus.min(n - 1).saturating_sub(max / 2).min(n - max);
    (first, first + max)
}

/// bo2mp: OPTIONS' lists: the rows a list `max` rows high shows, `first..last`,
/// from where it was (`first`) moved just enough to show `focus`. The mouse
/// focuses only a row it can see, so pointing never scrolls the list (a list
/// that re-centred on the row under the mouse ran to its foot: F2); the wheel
/// and the scroll bar move `first` (`opt_view::options_scroll`).
pub(crate) fn list_window(first: usize, n: usize, focus: usize, max: usize) -> (usize, usize) {
    if max == 0 || n <= max {
        return (0, n);
    }
    let focus = focus.min(n - 1);
    let mut first = first.min(n - max);
    if focus < first {
        first = focus;
    }
    if focus >= first + max {
        first = focus + 1 - max;
    }
    (first, first + max)
}

/// bo2mp: the sensitivity rows, on a 1-10 scale laid in a straight line over
/// the setting's own range (`lo..=hi`): MOUSE SENSITIVITY (0.5-30), LOOK
/// SENSITIVITY (Black Ops II's steps 1-14) and AIM SENSITIVITY (50-150%).
/// The settings file keeps the setting itself, so old files read as before.
pub(crate) fn scale_range(tab: usize, k: u8) -> Option<(f32, f32)> {
    match (tab, k) {
        (2, 0) => Some((0.5, 30.0)),
        (3, 0) => Some((1.0, 14.0)),
        (3, 1) => Some((0.5, 1.5)),
        _ => None,
    }
}

pub(crate) const SCALE_MIN: f32 = 1.0;
pub(crate) const SCALE_MAX: f32 = 10.0;

/// A setting's value on the 1-10 scale.
pub(crate) fn to_scale(raw: f32, (lo, hi): (f32, f32)) -> f32 {
    (SCALE_MIN + (raw - lo) / (hi - lo) * (SCALE_MAX - SCALE_MIN)).clamp(SCALE_MIN, SCALE_MAX)
}

/// The setting for a 1-10 value (kept in the scale).
pub(crate) fn from_scale(v: f32, (lo, hi): (f32, f32)) -> f32 {
    let v = if v.is_finite() { v.clamp(SCALE_MIN, SCALE_MAX) } else { SCALE_MIN };
    lo + (v - SCALE_MIN) / (SCALE_MAX - SCALE_MIN) * (hi - lo)
}

/// A 1-10 value as a row shows it.
pub(crate) fn scale_text(v: f32) -> String {
    format!("{v:.1}")
}

/// A typed 1-10 value: decimals (a comma for the point too); nothing for
/// what is not a number.
pub(crate) fn parse_scale(text: &str) -> Option<f32> {
    let v: f32 = text.trim().replace(',', ".").parse().ok()?;
    v.is_finite().then(|| v.clamp(SCALE_MIN, SCALE_MAX))
}

fn scaled(tab: usize, k: u8, raw: f32) -> String {
    scale_range(tab, k).map_or_else(|| format!("{raw:.1}"), |r| scale_text(to_scale(raw, r)))
}

/// The bar a row draws under or for its value (0..1): the volumes and the
/// brightness (a slider with the number), the 1-10 scales and the dead zones.
pub(crate) fn bar(tab: usize, k: u8, s: &GameSettings) -> Option<f32> {
    let f = match (tab, k) {
        (0, 4) => (s.brightness + 0.2) / 0.4,
        (0, 15) => s.upscale_sharpness,
        (1, 0) => s.master_volume,
        (1, 1) => s.music_volume,
        (2, 0) => scale_range(2, 0).map_or(0.0, |r| (to_scale(s.sensitivity, r) - 1.0) / 9.0),
        (3, 0) => scale_range(3, 0).map_or(0.0, |r| (to_scale(s.pad_sensitivity, r) - 1.0) / 9.0),
        (3, 1) => scale_range(3, 1).map_or(0.0, |r| (to_scale(s.pad_ads_sensitivity, r) - 1.0) / 9.0),
        (3, 6) => s.pad_deadzone_left / 0.4,
        (3, 7) => s.pad_deadzone_right / 0.4,
        _ => return None,
    };
    Some(f.clamp(0.0, 1.0))
}

/// The raw setting behind a 1-10 row.
fn scaled_field(tab: usize, k: u8, s: &mut GameSettings) -> Option<&mut f32> {
    match (tab, k) {
        (2, 0) => Some(&mut s.sensitivity),
        (3, 0) => Some(&mut s.pad_sensitivity),
        (3, 1) => Some(&mut s.pad_ads_sensitivity),
        _ => None,
    }
}

/// A 1-10 row `by` half steps on (not snapped to the half steps, so a
/// typed 3.7 steps to 4.2 and back to 3.7).
fn step_scaled(tab: usize, k: u8, by: i32, s: &mut GameSettings) -> bool {
    let Some(r) = scale_range(tab, k) else { return false };
    let Some(v) = scaled_field(tab, k, s) else { return false };
    *v = from_scale(to_scale(*v, r) + by as f32 * 0.5, r);
    true
}

/// A 1-10 value typed on a row (`v` on the scale).
pub(crate) fn set_scaled(tab: usize, k: u8, v: f32, s: &mut GameSettings) {
    let Some(r) = scale_range(tab, k) else { return };
    let Some(f) = scaled_field(tab, k, s) else { return };
    *f = from_scale(v, r);
    s.sanitize();
    s.touch();
}

/// The tabs a match's pause menu shows (all but ACCOUNT).
pub(crate) const MATCH_TABS: usize = 4;

/// Unlock All (every item free in Create-a-Class), as his stats hold it.
pub(crate) static UNLOCK_ALL: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// ACCOUNT's rows: his name (typed in), Unlock All.
pub(crate) const ACCOUNT_TAB: usize = 4;

/// bo2mp: GAME FOLDERS: a row a game (`game_paths::GAMES`), its folder's
/// state; Enter or a click picks the folder, Delete clears it (F10 opens
/// the tab from anywhere in the front end); SCAN FOR GAMES last.
pub(crate) const FOLDERS_TAB: usize = 5;

/// A game's name as a row shows it: no "Call of Duty: " in front.
pub(crate) fn folder_label(title: &str) -> String {
    let t = title.strip_prefix("Call of Duty: ").or_else(|| title.strip_prefix("the ")).unwrap_or(title);
    t.to_uppercase()
}

/// GAME FOLDERS' rows.
pub(crate) fn folder_rows(s: &GameSettings) -> Vec<Row> {
    use asset_transport::game_paths;
    let paths = game_paths::load();
    game_paths::GAMES
        .iter()
        .enumerate()
        .map(|(i, (key, title, needed))| {
            let path = paths.iter().find(|(k, _)| k == key).map(|(_, p)| p.clone());
            let modded = game_paths::MOD_KEYS.contains(key);
            let value = match &path {
                Some(p) if modded || game_paths::looks_valid_for(key, p) => "SET",
                Some(_) if *key == game_paths::BF1_KEY => "NO DATA/PATCH",
                Some(_) => "NO ZONE FOLDER",
                None if *needed => "NEEDED",
                None => "NOT SET",
            };
            // bo2mp: Modern Warfare III's row: its small copy (`jup_pack`).
            let small = path.as_ref().is_some_and(|p| p.join(asset_audio::PACK_MARK).is_file());
            let value = if small { "SMALL COPY" } else { value };
            let extra = if *key == crate::jup_cards::PREFIX {
                if crate::jup_pack::running() {
                    "\n\nMaking a small copy: its progress shows at the bottom."
                } else if small {
                    "\n\nA small copy: the guns, their sounds and pictures, the calling cards and emblems. The maps and Warzone need the full game's folder."
                } else {
                    "\n\nX: make a small copy of the guns (Modern Warfare III's and II's), their sounds and pictures, and the calling cards and emblems, to keep without the whole game."
                }
            } else {
                ""
            };
            let at = path.map_or_else(|| "Not set.".to_owned(), |p| p.display().to_string());
            Row {
                label: folder_label(title),
                sub: format!("{at}\n\nEnter or click: choose the folder (the one the game's .exe is in). Delete: clear it. A change loads after a restart.{extra}"),
                value: Some(value.to_owned()),
                act: Act::PickFolder(i),
            }
        })
        .chain([scan_row(), scan_folder_row(), prepare_at_launch_row(s), bake_all_row(), clear_cache_row()])
        .collect()
}

/// GAME FOLDERS' last row: SCAN FOR GAMES (F5 anywhere on the page) looks
/// for the games on this PC again and sets the ones with no folder.
fn scan_row() -> Row {
    let value = if crate::game_folders::scanning() { "SCANNING" } else { "F5" };
    Row {
        label: "SCAN FOR GAMES".to_owned(),
        sub: "Enter, a click or F5: look for the games on this PC (Steam libraries, Battle.net, Epic, Program Files, Games and SteamLibrary on every drive, Documents, Desktop, Downloads, the games folder, the folders beside the ones set) and set every game with no folder. Folders you set are kept. A change loads after a restart.".to_owned(),
        value: Some(value.to_owned()),
        act: Act::ScanGames,
    }
}

/// GAME FOLDERS' foot: SCAN A FOLDER, for games kept together (F1: every
/// game in `D:\COD`, one `<key>_game_files` each): he picks the folder or
/// drive, and every game in it fills its row.
fn scan_folder_row() -> Row {
    let value = if crate::game_folders::scanning() { "SCANNING" } else { "PICK" };
    Row {
        label: "SCAN A FOLDER".to_owned(),
        sub: "Enter or click: pick the folder (or drive) your games are in. Every game inside, up to three folders down, fills its row: each is told by its .exe and zone files, not the folder's name. Folders you set that still work are kept. A change loads after a restart.".to_owned(),
        value: Some(value.to_owned()),
        act: Act::ScanFolder,
    }
}

/// GAME FOLDERS: PREPARE MAPS AT LAUNCH (off by default): opens the
/// PREPARE MAPS page (the switch, a checkbox and time a game, the total).
fn prepare_at_launch_row(s: &GameSettings) -> Row {
    Row {
        label: "PREPARE MAPS AT LAUNCH".to_owned(),
        sub: "Enter or click: choose the games and turn it on or off. On: every time the game starts, it goes through every map of the checked games and makes any fast-load file that is missing or out of date, so every map loads quickly, even the first time. A bar at the bottom of the menus shows the map it is on and the time left; Esc stops it, and the next start goes on from where it stopped.".to_owned(),
        value: Some(on_off(s.prepare_maps_at_launch)),
        act: Act::SubPage(PREPARE_PAGE),
    }
}

/// GAME FOLDERS' row that opens PREPARE MAPS (where its back goes).
pub(crate) fn prepare_row() -> usize {
    asset_transport::game_paths::GAMES.len() + 2
}

/// GAME FOLDERS > PREPARE MAPS AT LAUNCH: its page, past KEY BINDINGS.
pub(crate) const PREPARE_PAGE: usize = super::keys::KEYS_PAGE + 1;
pub(crate) const PREPARE_TITLE: &str = "PREPARE MAPS";

/// PREPARE MAPS: a game's checkbox row is `k` = this + its place in
/// `assets::bake_all::GAMES`.
const PREPARE_GAME_K: u8 = 10;

/// PREPARE MAPS: game `key` is left out (unchecked) in `off`.
pub(crate) fn game_off(off: &str, key: &str) -> bool {
    off.split(',').any(|k| k.trim().eq_ignore_ascii_case(key))
}

/// `off` with game `key` checked or unchecked.
fn toggle_game(off: &str, key: &str) -> String {
    let mut keys: Vec<&str> = off.split(',').map(str::trim).filter(|k| !k.is_empty() && !k.eq_ignore_ascii_case(key)).collect();
    if !game_off(off, key) {
        keys.push(key);
    }
    keys.join(",")
}

/// A checkbox row's state (`None`: not a checkbox).
pub(crate) fn checkbox(tab: usize, k: u8, s: &GameSettings) -> Option<bool> {
    if tab != FOLDERS_TAB || k < PREPARE_GAME_K {
        return None;
    }
    let g = assets::bake_all::GAMES.get((k - PREPARE_GAME_K) as usize)?;
    Some(!game_off(&s.prepare_games_off, g.key))
}

/// The time a run takes, as a row shows it ("about 12 min").
pub(crate) fn eta_text(secs: f64) -> String {
    let m = (secs / 60.0).ceil() as u64;
    if secs < 60.0 {
        "under 1 min".to_owned()
    } else if m < 60 {
        format!("about {m} min")
    } else {
        format!("about {} h {} min", m / 60, m % 60)
    }
}

/// The maps still to make a game, read off the main thread (the folders,
/// the record and each map file's date): the last count, and how fresh.
struct Counts {
    left: Vec<assets::bake_all::Left>,
    at: Option<std::time::Instant>,
    busy: bool,
    rev: u32,
}

static COUNTS: std::sync::Mutex<Counts> = std::sync::Mutex::new(Counts { left: Vec::new(), at: None, busy: false, rev: 0 });

/// How long a count is good for (a run in the background makes maps).
const COUNT_AGE: std::time::Duration = std::time::Duration::from_secs(3);

/// The last count of each game's maps still to make (`None`: not counted
/// yet), counting again in the background when it is old.
fn counts() -> Option<Vec<assets::bake_all::Left>> {
    let mut c = COUNTS.lock().ok()?;
    if !c.busy && c.at.is_none_or(|t| t.elapsed() > COUNT_AGE) {
        c.busy = true;
        let spawned = std::thread::Builder::new()
            .name("prepare-count".into())
            .spawn(|| {
                let left = assets::bake_all::left_by_game();
                if let Ok(mut c) = COUNTS.lock() {
                    if c.at.is_none() || c.left != left {
                        c.rev = c.rev.wrapping_add(1);
                    }
                    c.left = left;
                    c.at = Some(std::time::Instant::now());
                    c.busy = false;
                }
            })
            .is_ok();
        if !spawned {
            c.busy = false;
        }
    }
    c.at.map(|_| c.left.clone())
}

/// Counts now, on this thread (the preview pictures: no wait for the
/// background count).
pub(crate) fn count_now() {
    let left = assets::bake_all::left_by_game();
    if let Ok(mut c) = COUNTS.lock() {
        c.rev = c.rev.wrapping_add(1);
        c.left = left;
        c.at = Some(std::time::Instant::now());
    }
}

/// For the view's redraw key: a new count.
pub(crate) fn counts_rev() -> u32 {
    COUNTS.lock().map_or(0, |c| c.rev)
}

/// PREPARE MAPS' rows: AT LAUNCH, a checkbox a game set in GAME FOLDERS
/// (its maps still to make and their time on this PC), the checked games'
/// total, PREPARE NOW.
fn prepare_rows(s: &GameSettings) -> Vec<Row> {
    let counts = counts();
    let mut out = vec![row(
        "AT LAUNCH",
        on_off(s.prepare_maps_at_launch),
        "On: every time the game starts, the checked games' maps that are missing or out of date are prepared, with a bar at the bottom of the menus (Esc stops it; the next start goes on from there). Maps already done are skipped at once.",
        0,
    )];
    let (mut secs, mut untimed, mut maps, mut counted) = (0.0, 0usize, 0usize, true);
    for (i, g) in assets::bake_all::GAMES.iter().enumerate() {
        if asset_transport::game_paths::get(g.key).is_none() {
            continue;
        }
        let title = asset_transport::game_paths::GAMES.iter().find(|(k, _, _)| *k == g.key).map_or_else(|| g.key.to_uppercase(), |(_, t, _)| folder_label(t));
        let checked = !game_off(&s.prepare_games_off, g.key);
        let left = counts.as_ref().map(|c| c.iter().find(|l| l.key == g.key));
        let (value, sub) = match left {
            None => {
                counted = false;
                ("COUNTING...".to_owned(), "Counting the maps still to prepare...".to_owned())
            }
            Some(None) => ("NO MAPS".to_owned(), "No maps found in this game's folder.".to_owned()),
            Some(Some(l)) => {
                if checked {
                    maps += l.left;
                    match l.secs() {
                        Some(t) => secs += t,
                        None => untimed += l.left,
                    }
                }
                let per = match l.secs_per_map {
                    Some(t) => format!("Measured on this PC: {t:.0} s a map."),
                    None => "Not timed on this PC yet: its time shows after its first prepared map.".to_owned(),
                };
                let value = match (l.left, l.secs()) {
                    (0, _) => "DONE".to_owned(),
                    (n, Some(t)) => format!("{n} of {} · {}", l.total, eta_text(t).trim_start_matches("about ")),
                    (n, None) => format!("{n} of {} · not timed", l.total),
                };
                (value, format!("{} of its {} maps still to prepare. {per}", l.left, l.total))
            }
        };
        out.push(Row {
            label: title,
            sub: format!("Enter or click: check or uncheck. Checked games are prepared (at launch and with PREPARE NOW). {sub}"),
            value: Some(value),
            act: Act::Cycle(Setting::Option(PREPARE_GAME_K + i as u8)),
        });
    }
    let total = if !counted {
        "COUNTING...".to_owned()
    } else if maps == 0 {
        "DONE".to_owned()
    } else if untimed == maps {
        format!("{maps} maps · not timed")
    } else if untimed > 0 {
        format!("{maps} maps · {} + {untimed} not timed", eta_text(secs))
    } else {
        format!("{maps} maps · {}", eta_text(secs))
    };
    out.push(Row {
        label: "TOTAL (CHECKED)".to_owned(),
        sub: "The checked games' maps still to prepare, and their time from the times measured on this PC (each prepared map updates its game's time).".to_owned(),
        value: Some(total),
        act: Act::Info,
    });
    let mut now = bake_all_row();
    now.label = "PREPARE NOW".to_owned();
    out.push(now);
    out
}

/// GAME FOLDERS: PREPARE ALL MAPS NOW makes every map's fast-load copy now
/// (`assets::bake_all`), checking every map, so even a map's first load is
/// quick; while it runs the row shows the map and its step, and a press
/// stops it.
fn bake_all_row() -> Row {
    let running = assets::bake_all::running();
    let sub = match (running, assets::bake_all::now()) {
        (true, Some(now)) => {
            let step = assets::bake_all::current_job().filter(|j| j.outcome().is_none() && j.label().starts_with(&now.map)).map(|j| {
                let (done, total) = j.counts();
                let why = j.why();
                let why = if why.is_empty() { String::new() } else { format!(" ({why})") };
                if total > 0 { format!("{}{why} {done}/{total}", j.label()) } else { format!("{}{why} {done}", j.label()) }
            });
            format!("Map {} of {}: {}. Enter, click or Esc: stop (the half-made copy is thrown away; the maps done so far stay, and the next run goes on from here).", now.index, now.of, step.unwrap_or(now.map))
        }
        (true, None) => "Starting... Enter, click or Esc: stop.".to_owned(),
        _ => "Enter or click: make the fast-load files of every map of the checked games (PREPARE MAPS AT LAUNCH chooses them) now, so even a map's first load is quick. Files that are missing or out of date (an older game update, setting or version of this game) are made again; maps already done are skipped. It runs in the background (a match loading stops it) with a bar and the time left; it uses disk space next to the cache.".to_owned(),
    };
    Row {
        label: "PREPARE ALL MAPS NOW".to_owned(),
        sub,
        value: Some(if running { "STOP" } else { "PREPARE" }.to_owned()),
        act: Act::BakeAll,
    }
}

/// GAME FOLDERS' last row: CLEAR CACHE empties every cache the game keeps
/// (`asset_transport::clear_cache`): the next loads build them again.
fn clear_cache_row() -> Row {
    let value = if crate::game_folders::scanning() { "BUSY" } else if crate::game_folders::clear_armed() { "PRESS AGAIN" } else { "CLEAR" };
    Row {
        label: "CLEAR CACHE".to_owned(),
        sub: "Enter or click twice: delete the fast-load copies the game made from your game files (the baked maps, the prepared pictures, sounds and shaders, the graphics card's pipelines). Your game files, settings and stats are not touched. The next load of each map is slower while it builds them again.".to_owned(),
        value: Some(value.to_owned()),
        act: Act::ClearCache,
    }
}

pub(crate) const RESOLUTIONS: [(u32, u32); 8] = [(1280, 720), (1366, 768), (1600, 900), (1920, 1080), (2560, 1080), (2560, 1440), (3440, 1440), (3840, 2160)];

pub(crate) const PAD_LAYOUTS: [&str; 5] = ["DEFAULT", "TACTICAL", "LEFTY", "NUMBER", "CHARLIE"];
pub(crate) const STICK_LAYOUTS: [&str; 4] = ["DEFAULT", "SOUTHPAW", "LEGACY", "LEGACY SOUTHPAW"];

fn on_off(v: bool) -> String {
    if v { "ON" } else { "OFF" }.to_owned()
}

/// A selector row: its label, what it shows, and a line about it.
fn row(label: &str, value: String, sub: &str, k: u8) -> Row {
    Row { label: label.to_owned(), sub: sub.to_owned(), value: Some(value), act: Act::Cycle(Setting::Option(k)) }
}

/// DRAW FPS's value: OFF, SIMPLE (the frame rate) or FULL (the telemetry panel).
fn fps_mode_label(s: &GameSettings) -> &'static str {
    match (s.draw_fps, s.fps_full) {
        (false, _) => "OFF",
        (true, false) => "SIMPLE",
        (true, true) => "FULL",
    }
}

/// A 0..1 level as a percentage.
pub(crate) fn percent(v: f32) -> String {
    format!("{:.0}%", v * 100.0)
}

/// The button layout's name (one he bound himself is CUSTOM).
pub(crate) fn pad_layout_name(layout: u8) -> String {
    if layout == GameSettings::PAD_LAYOUT_CUSTOM { "CUSTOM".to_owned() } else { PAD_LAYOUTS[layout.min(4) as usize].to_owned() }
}

/// The rows of a tab (`k`: the row's number in the tab).
pub(crate) fn rows(tab: usize, s: &GameSettings) -> Vec<Row> {
    match tab {
        FOLDERS_TAB => folder_rows(s),
        0 => vec![
            row("RESOLUTION", s.resolution.to_string(), "The window's size (fullscreen fills the monitor at its own size).", 0),
            row("DISPLAY MODE", if s.fullscreen { "FULLSCREEN" } else { "WINDOWED" }.to_owned(), "Borderless fullscreen on the monitor the game is on, or a window.", 1),
            row("V-SYNC", on_off(s.vsync), "Lock the frame rate to the monitor's.", 2),
            row("FIELD OF VIEW", format!("{:.0}", s.fov), "How wide you see in a match (65 is Black Ops II's).", 3),
            row("BRIGHTNESS", format!("{:+.0}", s.brightness * 100.0), "Brighter or darker in a match (0 is the map's own).", 4),
            row("SHADOWS", on_off(s.shadows), "Shadow maps in a match.", 5),
            row("DEPTH OF FIELD", on_off(s.depth_of_field), "Blur behind the sights when aiming.", 6),
            row("BLOOM", on_off(s.bloom), "Glow around bright lights.", 7),
            row(
                "TEXTURE QUALITY",
                if s.bo2_fast { "PERFORMANCE" } else { "FULL" }.to_owned(),
                "Performance halves texture sizes and draws less far on Black Ops II maps (textures from the next map on).",
                8,
            ),
            row(
                "PLAYER MODELS",
                if s.map_game_bodies { "MAP'S GAME" } else { "BLACK OPS II" }.to_owned(),
                "On another game's map: that game's soldiers and animations, or Black Ops II's characters.",
                9,
            ),
            row(
                "SOLDIERS",
                s.soldiers.label().to_owned(),
                "Black Ops' CIA against its Spetsnaz, Black Ops II's soldiers, or Task Force 141 against Makarov's men (Modern Warfare III's operators); from the next match.",
                10,
            ),
            row("ANIMATED CARDS", on_off(s.animated_cards), "Animated calling cards and emblems move: in the menus, in a match and in the killcam.", 11),
            row("BO3 NATIVE RENDERER", on_off(s.t7_native), "Black Ops III maps are drawn the way Black Ops III draws them. Off draws them with the converted world instead (from the next map on).", 12),
            row(
                "UPSCALING",
                s.upscaler.label().to_owned(),
                "Draws the match smaller and enlarges it: more frames. FSR 1 is AMD's spatial upscaler (any graphics card). FSR 3.1 is AMD's temporal one: sharper, any graphics card, needs the VULKAN graphics API. DLSS is NVIDIA's: RTX cards, VULKAN, a build made with DLSS. A choice this machine cannot run draws as the next best (the label says so). Menus and the HUD stay sharp. On a Radeon, AMD Fluid Motion Frames (Radeon software) adds frames to any game.",
                13,
            ),
            row(
                "RENDER QUALITY",
                s.upscale_quality.label().to_owned(),
                "How big the match is drawn before UPSCALING enlarges it: a smaller share is faster and softer. Does nothing while UPSCALING is off.",
                14,
            ),
            row(
                "SHARPNESS",
                format!("{:.0}%", s.upscale_sharpness * 100.0),
                "How much FSR 1 and FSR 3.1 sharpen the enlarged picture (80% is close to AMD's default; DLSS has its own). Does nothing while UPSCALING is off.",
                15,
            ),
            row(
                "GRAPHICS API",
                s.graphics_api.label().to_owned(),
                "Which graphics API the renderer starts with: AUTO lets wgpu choose, or VULKAN, DIRECTX 12, OPENGL. Applies on the next launch; if it fails to start, the game falls back to AUTO. FSR 3.1 and DLSS need VULKAN.",
                16,
            ),
            row(
                "REAL-TIME LIGHTING",
                on_off(s.rt_lighting),
                "The Black Ops III look's own ambient light, traced when the map loads, and the map's lamps and effect lights. Off draws the maps as before. Call of Duty: Ghosts maps are not affected: they use only their own baked lighting. Takes effect from the next map load.",
                17,
            ),
            row("MENU STYLE", if s.classic_menu { "CLASSIC" } else { "CURRENT" }.to_owned(), "The main menu: CURRENT, or the earlier CLASSIC layout (CALL OF DUTY logo; ZOMBIES, WARZONE, SPEC OPS). Kept for the next launch.", 18),
            row(
                "GRAPHICS QUALITY",
                s.graphics_quality.label().to_owned(),
                "LOW, MEDIUM, HIGH or ULTRA (everything on) for maps drawn by their own game's renderer (Ghosts, Call of Duty 4, Black Ops III, ...): shadow maps and their size, lamps, texture size and filtering, bloom, draw distance. SHADOWS, BLOOM and TEXTURE QUALITY still switch their part off. Shadows, lamps, bloom and draw distance change at once; textures from the next map.",
                19,
            ),
            row("MOTION BLUR", on_off(s.motion_blur), "Blur from turning and moving fast, on maps drawn by a renderer that has it (Infinite Warfare's). Also off at LOW and MEDIUM GRAPHICS QUALITY.", 20),
            row("DRAW FPS", fps_mode_label(s).to_owned(), "The frame rate counter in a corner of the screen. SIMPLE shows the frames per second; FULL adds the frame time, a graph and the 1% low.", 21),
            row("FPS POSITION", if s.fps_left { "LEFT" } else { "RIGHT" }.to_owned(), "Which top corner the frame rate counter (DRAW FPS) sits in.", 22),
        ],
        1 => vec![
            row("MASTER VOLUME", percent(s.master_volume), "Every sound in the game.", 0),
            row("MUSIC VOLUME", percent(s.music_volume), "Menu, lobby and match music.", 1),
            row("MUSIC IN MATCHES", on_off(s.match_music), "Quiet ambient music during matches.", 2),
            row("MAP INTRO MUSIC", on_off(s.map_intro_music), "A ~50 second track chosen for the map plays when you join a match, then fades out.", 3),
            row("CHAT TEXT-TO-SPEECH", on_off(s.chat_tts), "Chat lines are read aloud in R.E.P.O.'s voice (needs R.E.P.O. installed on this PC).", 4),
        ],
        2 => vec![
            row("MOUSE SENSITIVITY", scaled(2, 0, s.sensitivity), "How fast the mouse turns you (1-10). Enter or click: type an exact value.", 0),
            row("INVERT MOUSE", on_off(s.invert_mouse), "Mouse up looks down.", 1),
        ],
        3 => vec![
            row("LOOK SENSITIVITY", scaled(3, 0, s.pad_sensitivity), "How fast the stick turns you: 1 is slow, 10 the fastest (Black Ops II's 14 steps). Enter or click: type an exact value.", 0),
            row("AIM SENSITIVITY", scaled(3, 1, s.pad_ads_sensitivity), "Turning while aiming down sights: 1 is half, 10 one and a half times the look speed. Enter or click: type an exact value.", 1),
            row("INVERT LOOK", on_off(s.pad_invert), "Stick up looks down.", 2),
            row("VIBRATION", on_off(s.pad_vibration), "The controller rumbles.", 3),
            row("BUTTON LAYOUT", pad_layout_name(s.pad_layout), "Black Ops II's button layouts (CUSTOM: buttons you bound yourself).", 4),
            row("STICK LAYOUT", STICK_LAYOUTS[s.pad_stick_layout.min(3) as usize].to_owned(), "Which stick moves and which looks.", 5),
            row("LEFT STICK DEADZONE", percent(s.pad_deadzone_left), "How far the stick goes before it moves you.", 6),
            row("RIGHT STICK DEADZONE", percent(s.pad_deadzone_right), "How far the stick goes before it turns you.", 7),
        ],
        _ => vec![
            Row {
                label: "PLAYER NAME".to_owned(),
                sub: "Press Enter to type a new name (16 letters at most), Enter again to keep it.".to_owned(),
                value: Some(s.player_name.clone()),
                act: Act::TypeName,
            },
            row(
                "UNLOCK ALL",
                on_off(UNLOCK_ALL.load(std::sync::atomic::Ordering::Relaxed)),
                "Every weapon, attachment, perk and wildcard free in Create-a-Class, whatever your level.",
                1,
            ),
            Row {
                label: "FRIEND CODE".to_owned(),
                sub: "Give this to a friend so they can add you (FRIENDS on the main menu).".to_owned(),
                value: Some(GameSettings::friend_code_text(s.friend_code)),
                act: Act::Info,
            },
        ],
    }
}

/// `v` moved `by` steps of `d`, on the grid of `d`, kept in `lo..=hi`.
pub(crate) fn step(v: f32, by: i32, d: f32, lo: f32, hi: f32) -> f32 {
    ((v / d).round() + by as f32).mul_add(d, 0.0).clamp(lo, hi)
}

/// The resolution `by` steps from `now` in the list (one not in it starts
/// from the nearest).
pub(crate) fn next_resolution(now: DisplayResolution, by: i32) -> DisplayResolution {
    let pixels = |(w, h): (u32, u32)| i64::from(w) * i64::from(h);
    let mine = pixels((now.width, now.height));
    let at = RESOLUTIONS.iter().position(|(w, h)| *w == now.width && *h == now.height);
    let i = match at {
        Some(i) => (i as i32 + by).rem_euclid(RESOLUTIONS.len() as i32) as usize,
        None => {
            let near = RESOLUTIONS.iter().enumerate().min_by_key(|(_, r)| (pixels(**r) - mine).abs()).map_or(0, |(i, _)| i);
            if by > 0 && pixels(RESOLUTIONS[near]) <= mine {
                (near + 1) % RESOLUTIONS.len()
            } else if by < 0 && pixels(RESOLUTIONS[near]) >= mine {
                (near + RESOLUTIONS.len() - 1) % RESOLUTIONS.len()
            } else {
                near
            }
        }
    };
    let (w, h) = RESOLUTIONS[i];
    DisplayResolution::new(w, h)
}

/// Change a row by one step (`by` -1 or +1).
pub(crate) fn change(tab: usize, k: u8, by: i32, s: &mut GameSettings) {
    if step_scaled(tab, k, by, s) {
        s.sanitize();
        s.touch();
        return;
    }
    match (tab, k) {
        (FOLDERS_TAB, 0) => s.prepare_maps_at_launch = !s.prepare_maps_at_launch,
        (FOLDERS_TAB, k) if k >= PREPARE_GAME_K => {
            if let Some(g) = assets::bake_all::GAMES.get((k - PREPARE_GAME_K) as usize) {
                s.prepare_games_off = toggle_game(&s.prepare_games_off, g.key);
            }
        }
        (0, 0) => s.resolution = next_resolution(s.resolution, by),
        (0, 1) => s.fullscreen = !s.fullscreen,
        (0, 2) => s.vsync = !s.vsync,
        (0, 3) => s.fov = step(s.fov, by, 5.0, GameSettings::FOV_MIN, GameSettings::FOV_MAX),
        (0, 4) => s.brightness = step(s.brightness, by, 0.04, -0.2, 0.2),
        (0, 5) => s.shadows = !s.shadows,
        (0, 6) => s.depth_of_field = !s.depth_of_field,
        (0, 7) => s.bloom = !s.bloom,
        (0, 8) => s.bo2_fast = !s.bo2_fast,
        (0, 9) => s.map_game_bodies = !s.map_game_bodies,
        (0, 10) => s.soldiers = s.soldiers.step(by),
        (0, 11) => s.animated_cards = !s.animated_cards,
        (0, 12) => s.t7_native = !s.t7_native,
        (0, 13) => s.upscaler = s.upscaler.step(by),
        (0, 14) => s.upscale_quality = s.upscale_quality.step(by),
        (0, 16) => s.graphics_api = s.graphics_api.step(by),
        (0, 15) => s.upscale_sharpness = step(s.upscale_sharpness, by, 0.1, 0.0, 1.0),
        (0, 17) => s.rt_lighting = !s.rt_lighting,
        (0, 18) => s.classic_menu = !s.classic_menu,
        (0, 19) => s.graphics_quality = s.graphics_quality.step(by),
        (0, 20) => s.motion_blur = !s.motion_blur,
        (0, 21) => {
            // OFF -> SIMPLE -> FULL, either way round.
            let at = if !s.draw_fps { 0 } else if s.fps_full { 2 } else { 1 };
            match (at + by).rem_euclid(3) {
                0 => (s.draw_fps, s.fps_full) = (false, false),
                1 => (s.draw_fps, s.fps_full) = (true, false),
                _ => (s.draw_fps, s.fps_full) = (true, true),
            }
        }
        (0, 22) => s.fps_left = !s.fps_left,
        (1, 0) => s.master_volume = step(s.master_volume, by, 0.05, 0.0, 1.0),
        (1, 1) => s.music_volume = step(s.music_volume, by, 0.05, 0.0, 1.0),
        (1, 2) => s.match_music = !s.match_music,
        (1, 3) => s.map_intro_music = !s.map_intro_music,
        (1, 4) => s.chat_tts = !s.chat_tts,
        (2, 1) => s.invert_mouse = !s.invert_mouse,
        (3, 2) => s.pad_invert = !s.pad_invert,
        (3, 3) => s.pad_vibration = !s.pad_vibration,
        // (From CUSTOM, right is DEFAULT and left CHARLIE.)
        (3, 4) => {
            let at = if s.pad_layout == GameSettings::PAD_LAYOUT_CUSTOM { if by > 0 { -1 } else { 0 } } else { i32::from(s.pad_layout.min(4)) };
            s.pad_layout = (at + by).rem_euclid(PAD_LAYOUTS.len() as i32) as u8;
        }
        (3, 5) => s.pad_stick_layout = ((s.pad_stick_layout.min(3) as i32 + by).rem_euclid(4)) as u8,
        (3, 6) => s.pad_deadzone_left = step(s.pad_deadzone_left, by, 0.02, 0.0, 0.4),
        (3, 7) => s.pad_deadzone_right = step(s.pad_deadzone_right, by, 0.02, 0.0, 0.4),
        (4, 1) => {
            let on = !UNLOCK_ALL.load(std::sync::atomic::Ordering::Relaxed);
            super::bridge::request(super::bridge::Request::UnlockAll(on));
            return;
        }
        _ => return,
    }
    s.sanitize();
    s.touch();
}

/// His settings as of this frame, and the changes the screens asked for
/// (carried out by `sync_settings`, the one system that writes them).
static NOW: std::sync::Mutex<Option<GameSettings>> = std::sync::Mutex::new(None);
static QUEUED: std::sync::Mutex<Vec<(usize, u8, i32)>> = std::sync::Mutex::new(Vec::new());

pub(crate) fn snapshot() -> GameSettings {
    NOW.lock().ok().and_then(|g| g.clone()).unwrap_or_default()
}

/// His saved settings have reached the menu (the first frame after the
/// settings file was read).
pub(crate) fn loaded() -> bool {
    NOW.lock().is_ok_and(|g| g.is_some())
}

pub(crate) fn queue(tab: usize, k: u8, by: i32) {
    if let Ok(mut q) = QUEUED.lock() {
        q.push((tab, k, by));
    }
}

/// A 1-10 value he typed on a row (tab, row, value).
static TYPED: std::sync::Mutex<Vec<(usize, u8, f32)>> = std::sync::Mutex::new(Vec::new());

pub(crate) fn queue_typed(tab: usize, k: u8, v: f32) {
    if let Ok(mut q) = TYPED.lock() {
        q.push((tab, k, v));
    }
}

/// A new name he typed.
static NAME: std::sync::Mutex<Option<String>> = std::sync::Mutex::new(None);

pub(crate) fn set_name(name: String) {
    if let Ok(mut n) = NAME.lock() {
        *n = Some(name);
    }
}

pub(crate) fn sync_settings(settings: Option<bevy::prelude::ResMut<GameSettings>>, mut fast: bevy::prelude::Local<Option<bool>>) {
    let Some(mut s) = settings else { return };
    if let Some(name) = NAME.lock().ok().and_then(|mut n| n.take()) {
        s.player_name = name;
        s.sanitize();
        s.touch();
        diag::info!(Ui, "bo2mp shell: player name now `{}`", s.player_name);
    }
    // (Friends added or removed on the FRIENDS screen.)
    if super::friends::apply_edits(&mut s) {
        s.sanitize();
        s.touch();
    }
    let queued = QUEUED.lock().map(|mut q| std::mem::take(&mut *q)).unwrap_or_default();
    for (tab, k, by) in queued {
        change(tab, k, by, &mut s);
    }
    let typed = TYPED.lock().map(|mut q| std::mem::take(&mut *q)).unwrap_or_default();
    for (tab, k, v) in typed {
        set_scaled(tab, k, v, &mut s);
    }
    // (Texture quality: the Black Ops II renderer reads it as it draws and
    // loads; the settings file only sets it at start.)
    if *fast != Some(s.bo2_fast) {
        if fast.is_some() {
            asset_core::t6_set_fast(s.bo2_fast);
            diag::info!(Ui, "bo2mp shell: Black Ops II quality {}", if s.bo2_fast { "fast" } else { "full" });
        }
        *fast = Some(s.bo2_fast);
    }
    // (Player models: the body path reads it as it draws.)
    assets::set_map_game_bodies(s.map_game_bodies);
    assets::set_soldiers(s.soldiers);
    crate::hud_cards::set_animated(s.animated_cards);
    super::set_classic(s.classic_menu);
    // (Who he is to his friends, and whom he looks for, for the master.)
    net::set_friend_identity(s.friend_code, &s.player_name);
    net::set_friends(&s.friends);
    if let Ok(mut g) = NOW.lock()
        && g.as_ref().is_none_or(|old| old.revision != s.revision || old != &*s)
    {
        *g = Some((*s).clone());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn value(tab: usize, k: usize, s: &GameSettings) -> String {
        rows(tab, s)[k].value.clone().unwrap_or_default()
    }

    /// Every selector row of every tab changes something (none is only
    /// drawn), and a step there and back leaves it as it was.
    #[test]
    fn every_row_works_and_steps_back() {
        for tab in 0..ACCOUNT_TAB {
            let base = GameSettings::default();
            for (k, r) in rows(tab, &base).iter().enumerate() {
                let Act::Cycle(Setting::Option(id)) = r.act else { panic!("{} is not a selector", r.label) };
                assert_eq!(id as usize, k, "{}: row number", r.label);
                // (A level at its top by default steps down instead.)
                let by = {
                    let mut t = base.clone();
                    change(tab, id, 1, &mut t);
                    if value(tab, k, &t) == value(tab, k, &base) { -1 } else { 1 }
                };
                let mut s = base.clone();
                change(tab, id, by, &mut s);
                assert_ne!(s, base, "{} does nothing", r.label);
                assert_ne!(value(tab, k, &s), value(tab, k, &base), "{} shows no change", r.label);
                assert!(s.revision > base.revision, "{} is not marked changed (not saved)", r.label);
                change(tab, id, -by, &mut s);
                let shown = |s: &GameSettings| rows(tab, s).into_iter().map(|r| r.value).collect::<Vec<_>>();
                assert_eq!(shown(&s), shown(&base), "{} does not step back", r.label);
            }
        }
    }

    #[test]
    fn values_read_his_settings() {
        let mut s = GameSettings::default();
        s.resolution = DisplayResolution::new(2560, 1440);
        s.fullscreen = true;
        s.fov = 90.0;
        s.brightness = -0.08;
        s.master_volume = 0.35;
        s.music_volume = 0.0;
        s.match_music = false;
        s.sensitivity = 7.5;
        s.pad_deadzone_left = 0.2;
        s.pad_stick_layout = 1;
        s.player_name = "sero".to_owned();
        assert_eq!(value(0, 0, &s), "2560x1440");
        assert_eq!(value(0, 1, &s), "FULLSCREEN");
        assert_eq!(value(0, 3, &s), "90");
        assert_eq!(value(0, 4, &s), "-8");
        assert_eq!(value(1, 0, &s), "35%");
        assert_eq!(value(1, 1, &s), "0%");
        assert_eq!(value(1, 2, &s), "OFF");
        // (Sensitivities on the 1-10 scale: 7.5 of 0.5-30, 100% of 50-150%.)
        assert_eq!(value(2, 0, &s), "3.1");
        s.pad_ads_sensitivity = 1.0;
        assert_eq!(value(3, 1, &s), "5.5");
        assert_eq!(value(3, 5, &s), "SOUTHPAW");
        assert_eq!(value(3, 6, &s), "20%");
        assert_eq!(value(4, 0, &s), "sero");
        assert_eq!(rows(4, &s)[0].act, Act::TypeName);
    }

    #[test]
    fn steps_stay_in_range() {
        let mut s = GameSettings { master_volume: 1.0, ..GameSettings::default() };
        change(1, 0, 1, &mut s);
        assert_eq!(s.master_volume, 1.0);
        for _ in 0..40 {
            change(1, 0, -1, &mut s);
        }
        assert_eq!(s.master_volume, 0.0);
        for _ in 0..30 {
            change(0, 3, 1, &mut s);
            change(0, 4, 1, &mut s);
            change(3, 0, 1, &mut s);
        }
        assert_eq!(s.fov, GameSettings::FOV_MAX);
        assert!((s.brightness - 0.2).abs() < 1e-6);
        assert_eq!(s.pad_sensitivity, 14.0);
        // (Off the grid - an old file's 0.333 - lands back on it.)
        assert!((step(0.333, 1, 0.05, 0.0, 1.0) - 0.4).abs() < 1e-6);
        assert!((step(0.333, -1, 0.05, 0.0, 1.0) - 0.3).abs() < 1e-6);
    }

    #[test]
    fn resolutions_cycle_from_any() {
        assert_eq!(next_resolution(DisplayResolution::new(1920, 1080), 1), DisplayResolution::new(2560, 1080));
        assert_eq!(next_resolution(DisplayResolution::new(3840, 2160), 1), DisplayResolution::new(1280, 720));
        assert_eq!(next_resolution(DisplayResolution::new(1280, 720), -1), DisplayResolution::new(3840, 2160));
        // (His own 1680x1050, not listed: the next bigger / smaller.)
        assert_eq!(next_resolution(DisplayResolution::new(1680, 1050), 1), DisplayResolution::new(1920, 1080));
        assert_eq!(next_resolution(DisplayResolution::new(1680, 1050), -1), DisplayResolution::new(1600, 900));
    }

    #[test]
    fn custom_button_layout() {
        let mut s = GameSettings { pad_layout: GameSettings::PAD_LAYOUT_CUSTOM, ..GameSettings::default() };
        assert_eq!(value(3, 4, &s), "CUSTOM");
        change(3, 4, 1, &mut s);
        assert_eq!(s.pad_layout, 0);
        s.pad_layout = GameSettings::PAD_LAYOUT_CUSTOM;
        change(3, 4, -1, &mut s);
        assert_eq!(s.pad_layout, 4);
    }

    /// The ACCOUNT rows are not settings-file ones: the name is typed, and
    /// Unlock All goes to his stats (no settings change).
    #[test]
    fn account_rows() {
        let base = GameSettings::default();
        let mut s = base.clone();
        change(4, 0, 1, &mut s);
        assert_eq!(s, base);
        assert_eq!(MATCH_TABS, ACCOUNT_TAB);
    }

    /// F2: GAME FOLDERS (18 rows, 11 shown): pointing at any row it shows
    /// never moves the list; the keys move it only past its ends.
    #[test]
    fn pointing_at_a_shown_row_never_scrolls() {
        for first in 0..=7 {
            for focus in first..first + 11 {
                assert_eq!(list_window(first, 18, focus, 11), (first, first + 11), "first {first} focus {focus}");
            }
        }
        assert_eq!(list_window(0, 18, 11, 11), (1, 12));
        assert_eq!(list_window(5, 18, 2, 11), (2, 13));
        assert_eq!(list_window(9, 18, 17, 11), (7, 18));
        assert_eq!(list_window(3, 5, 4, 11), (0, 5));
    }

    #[test]
    fn scroll_window_keeps_focus_in_view() {
        assert_eq!(scroll_window(5, 4, 11), (0, 5));
        assert_eq!(scroll_window(16, 0, 11), (0, 11));
        assert_eq!(scroll_window(16, 8, 11), (3, 14));
        assert_eq!(scroll_window(16, 15, 11), (5, 16));
        assert_eq!(scroll_window(16, 99, 11), (5, 16));
        for n in 0..30 {
            for f in 0..n {
                let (a, b) = scroll_window(n, f, 9);
                assert!(a <= f && f < b && b <= n && b - a == n.min(9), "{n} {f}");
            }
        }
    }

    /// Every row of every tab sits on exactly one front-end page, under the
    /// category of its own tab, with its own row number; VIDEO OPTIONS adds
    /// only the ADVANCED VIDEO row.
    #[test]
    fn categories_hold_every_row() {
        let s = GameSettings::default();
        assert_eq!(CATEGORIES.len(), TABS.len());
        for tab in 0..TABS.len() {
            // (A row that opens a page of its own, like PREPARE MAPS AT
            // LAUNCH, is a link, not a setting.)
            let all: Vec<Row> = rows(tab, &s).into_iter().filter(|r| !matches!(r.act, Act::SubPage(_))).collect();
            let pages: Vec<usize> = if tab == 0 { vec![0, ADVANCED_PAGE] } else { vec![tab] };
            let shown: Vec<Row> = pages.iter().flat_map(|&p| page_rows(p, &s)).filter(|r| !matches!(r.act, Act::SubPage(_))).collect();
            assert_eq!(shown, all, "tab {tab}");
            for &p in &pages {
                assert_eq!(tab_of(p), tab);
            }
        }
        let video = page_rows(0, &s);
        assert_eq!(video.len(), VIDEO_MAIN + 1);
        assert_eq!(video[VIDEO_MAIN].act, Act::SubPage(ADVANCED_PAGE));
        assert!(page_rows(ADVANCED_PAGE, &s).iter().any(|r| r.label == "BO3 NATIVE RENDERER"));
        // (MOUSE & KEYBOARD adds only the KEY BINDINGS row, which opens its
        // page, under MOUSE & KEYBOARD.)
        let mk = page_rows(2, &s);
        assert_eq!(mk.len(), KEYS_ROW + 1);
        assert_eq!(mk[KEYS_ROW].act, Act::SubPage(super::super::keys::KEYS_PAGE));
        assert_eq!(tab_of(super::super::keys::KEYS_PAGE), 2);
        assert!(page_rows(super::super::keys::KEYS_PAGE, &s).iter().all(|r| matches!(r.act, Act::Rebind(_) | Act::ResetKeys)));
    }

    #[test]
    fn draw_fps_rows_cycle_and_show_in_graphics() {
        let mut s = GameSettings::default();
        let value = |s: &GameSettings, label: &str| rows(0, s).into_iter().find(|r| r.label == label).and_then(|r| r.value);
        // (On by default, as the bare frame rate.)
        assert_eq!(value(&s, "DRAW FPS").as_deref(), Some("SIMPLE"));
        change(0, 21, 1, &mut s);
        change(0, 21, 1, &mut s);
        assert!(!s.draw_fps);
        assert_eq!(value(&s, "DRAW FPS").as_deref(), Some("OFF"));
        change(0, 21, 1, &mut s);
        assert!(s.draw_fps && !s.fps_full);
        assert_eq!(value(&s, "DRAW FPS").as_deref(), Some("SIMPLE"));
        change(0, 21, 1, &mut s);
        assert!(s.draw_fps && s.fps_full);
        assert_eq!(value(&s, "DRAW FPS").as_deref(), Some("FULL"));
        change(0, 21, 1, &mut s);
        assert!(!s.draw_fps && !s.fps_full);
        change(0, 21, -1, &mut s);
        assert!(s.draw_fps && s.fps_full);
        assert_eq!(value(&s, "FPS POSITION").as_deref(), Some("RIGHT"));
        change(0, 22, 1, &mut s);
        assert!(s.fps_left);
        assert_eq!(value(&s, "FPS POSITION").as_deref(), Some("LEFT"));
        assert!(page_rows(ADVANCED_PAGE, &s).iter().any(|r| r.label == "DRAW FPS"));
    }

    #[test]
    fn sensitivity_scale() {
        let r = scale_range(2, 0).unwrap();
        assert_eq!(from_scale(1.0, r), 0.5);
        assert_eq!(from_scale(10.0, r), 30.0);
        assert!((to_scale(from_scale(6.3, r), r) - 6.3).abs() < 1e-4);
        // (An old file's 5.0 reads as 2.4 and stays 5.0 until changed.)
        assert_eq!(scaled(2, 0, 5.0), "2.4");
        assert_eq!(parse_scale(" 7,25 "), Some(7.25));
        assert_eq!(parse_scale("42"), Some(10.0));
        assert_eq!(parse_scale("0"), Some(1.0));
        assert_eq!(parse_scale("abc"), None);
        let mut s = GameSettings::default();
        set_scaled(2, 0, 10.0, &mut s);
        assert_eq!(s.sensitivity, 30.0);
        set_scaled(3, 0, 1.0, &mut s);
        assert_eq!(s.pad_sensitivity, 1.0);
        assert_eq!(value(3, 0, &s), "1.0");
        change(3, 0, 1, &mut s);
        assert_eq!(value(3, 0, &s), "1.5");
        assert!(bar(1, 0, &s).is_some() && bar(0, 4, &s).is_some() && bar(0, 0, &s).is_none());
    }

    /// GAME FOLDERS: a row a game (Delete and X read the focus as the
    /// game's index), then SCAN FOR GAMES, SCAN A FOLDER, PREPARE MAPS AT
    /// LAUNCH, PREPARE ALL MAPS NOW and CLEAR CACHE.
    #[test]
    fn folders_end_with_scan_for_games() {
        let mut s = GameSettings::default();
        let r = folder_rows(&s);
        let games = asset_transport::game_paths::GAMES.len();
        assert_eq!(r.len(), games + 5);
        assert!(r[..games].iter().enumerate().all(|(i, row)| row.act == Act::PickFolder(i)));
        assert_eq!(r[games].act, Act::ScanGames);
        assert_eq!(r[games].label, "SCAN FOR GAMES");
        assert_eq!(r[games + 1].act, Act::ScanFolder);
        assert_eq!(r[games + 1].label, "SCAN A FOLDER");
        assert_eq!(r[games + 2].label, "PREPARE MAPS AT LAUNCH");
        assert_eq!(r[games + 2].value.as_deref(), Some("OFF"), "off by default");
        assert_eq!(r[games + 2].act, Act::SubPage(PREPARE_PAGE), "opens PREPARE MAPS");
        assert_eq!((prepare_row(), tab_of(PREPARE_PAGE)), (games + 2, FOLDERS_TAB));
        change(FOLDERS_TAB, 0, 1, &mut s);
        assert!(s.prepare_maps_at_launch && s.revision > GameSettings::default().revision, "turns on and is saved");
        assert_eq!(folder_rows(&s)[games + 2].value.as_deref(), Some("ON"));

        // PREPARE MAPS: AT LAUNCH first, a checkbox a game, TOTAL, PREPARE NOW.
        let p = page_rows(PREPARE_PAGE, &s);
        assert_eq!((p[0].label.as_str(), p[0].value.as_deref()), ("AT LAUNCH", Some("ON")));
        let n = p.len();
        assert_eq!((p[n - 2].label.as_str(), &p[n - 2].act), ("TOTAL (CHECKED)", &Act::Info));
        assert_eq!((p[n - 1].label.as_str(), &p[n - 1].act), ("PREPARE NOW", &Act::BakeAll));
        // (Every game starts checked; a box unchecks and checks again, saved.)
        let bo2 = assets::bake_all::GAMES.iter().position(|g| g.key == "bo2").unwrap() as u8 + PREPARE_GAME_K;
        assert_eq!(checkbox(FOLDERS_TAB, bo2, &s), Some(true));
        assert_eq!(checkbox(FOLDERS_TAB, 0, &s), None, "AT LAUNCH is no checkbox");
        let rev = s.revision;
        change(FOLDERS_TAB, bo2, 1, &mut s);
        assert_eq!((checkbox(FOLDERS_TAB, bo2, &s), s.prepare_games_off.as_str()), (Some(false), "bo2"));
        assert!(s.revision > rev, "saved");
        change(FOLDERS_TAB, bo2, 1, &mut s);
        assert_eq!((checkbox(FOLDERS_TAB, bo2, &s), s.prepare_games_off.as_str()), (Some(true), ""));
        assert_eq!(toggle_game("bo3, mw2", "BO3"), "mw2");
        assert!(game_off("bo3,mw2", "mw2") && !game_off("bo3,mw2", "mw3"));
        assert_eq!((eta_text(20.0), eta_text(61.0), eta_text(3600.0), eta_text(3700.0)), ("under 1 min".into(), "about 2 min".into(), "about 1 h 0 min".into(), "about 1 h 2 min".into()));
        assert_eq!(r[games + 3].act, Act::BakeAll);
        assert_eq!(r[games + 3].label, "PREPARE ALL MAPS NOW");
        assert_eq!(r[games + 4].act, Act::ClearCache);
        assert_eq!(crate::game_folders::scan_line(0, &[]), "Found 0 games. No new folders: every game found was already set.");
        assert!(crate::game_folders::scan_line(2, &["Call of Duty: Ghosts".to_owned()]).starts_with("Found 2 games. New: Call of Duty: Ghosts."));
    }
}
