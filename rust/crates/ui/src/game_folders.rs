//! bo2mp: the front end's Game Folders panel (F10, or Back + Y on a pad).
//!
//! Lists every game the rebuild reads, where it is set to (game_paths.cfg)
//! and whether that folder looks right. Up / Down picks a game; Enter (A)
//! picks its folder with a folder picker; Delete (X) clears it; F10 or
//! Esc (B) closes. The zone search reads the file as it goes, but what was
//! already loaded (the front end, the map list) changes after a restart,
//! which the panel says.

use std::path::PathBuf;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

use asset_transport::game_paths;
use bevy::input::gamepad::{Gamepad, GamepadButton};
use bevy::prelude::*;
use frame::ClientSet;

use crate::layers::{GameUiFont, game_text_font};

static OPEN: AtomicBool = AtomicBool::new(false);

/// The panel is up (BO2's menus take no keys meanwhile).
pub(crate) fn is_open() -> bool {
    OPEN.load(Ordering::Relaxed)
}

/// A folder picker running off the main thread: (game key, its answer).
static PICKED: Mutex<Option<(String, Option<PathBuf>)>> = Mutex::new(None);
static PICKING: AtomicBool = AtomicBool::new(false);

/// bo2mp: F10 pressed with the shell up: it opens Options > GAME FOLDERS
/// (`take_open_request`) instead of this panel.
static WANT_SHELL: AtomicBool = AtomicBool::new(false);

/// The shell's F10: true once per press.
pub(crate) fn take_open_request() -> bool {
    WANT_SHELL.swap(false, Ordering::Relaxed)
}

/// A folder picker is open.
pub(crate) fn picking() -> bool {
    PICKING.load(Ordering::Relaxed)
}

/// Opens the folder picker for `key` (a [`game_paths::GAMES`] key) off the
/// main thread; [`take_picked`] has its answer. Returns the line to show,
/// or None while another picker is still open.
pub(crate) fn start_pick(key: &str, title: &str) -> Option<String> {
    if PICKING.swap(true, Ordering::Relaxed) {
        return None;
    }
    let (key, owned) = (key.to_owned(), title.to_owned());
    std::thread::Builder::new()
        .name("game-folder-picker".into())
        .spawn(move || {
            let folder = game_paths::pick_folder(&format!("Where is {owned} installed?"));
            if let Ok(mut p) = PICKED.lock() {
                *p = Some((key, folder));
            }
        })
        .ok();
    Some(format!("Pick {title}'s folder in the window that opened."))
}

/// The picker's answer once it closed, set in game_paths.cfg: the line
/// about it.
pub(crate) fn take_picked() -> Option<String> {
    let (k, folder) = PICKED.lock().ok()?.take()?;
    PICKING.store(false, Ordering::Relaxed);
    Some(match folder {
        Some(folder) if game_paths::looks_valid_for(&k, &folder) || game_paths::MOD_KEYS.contains(&k.as_str()) => {
            match game_paths::set(&k, Some(folder.clone())) {
                Ok(()) => format!("Set to {}. Restart the game to load it.", folder.display()),
                Err(e) => e,
            }
        }
        Some(folder) if k == game_paths::BF1_KEY => format!("{} has no Data\\layout.toc and Patch folder: pick the folder bf1.exe is in.", folder.display()),
        Some(folder) => format!("{} has no `zone` folder or fast files: pick the folder the game's .exe is in.", folder.display()),
        None => "No folder picked.".to_owned(),
    })
}

/// SCAN FOR GAMES: the game scan running off the main thread, and the
/// line about what it found.
static SCANNING: AtomicBool = AtomicBool::new(false);
static SCANNED: Mutex<Option<String>> = Mutex::new(None);

/// A scan is running.
pub(crate) fn scanning() -> bool {
    SCANNING.load(Ordering::Relaxed)
}

/// Starts the game scan (`asset_transport::game_scan::rescan`) off the
/// main thread; [`take_scanned`] has what it found. The line to show.
pub(crate) fn start_scan() -> String {
    if SCANNING.swap(true, Ordering::Relaxed) {
        return "Already scanning for games.".to_owned();
    }
    let spawned = std::thread::Builder::new()
        .name("game-rescan".into())
        .spawn(|| {
            let found = asset_transport::game_scan::rescan();
            if let Ok(mut s) = SCANNED.lock() {
                *s = Some(scan_line(found.found, &found.set));
            }
        })
        .is_ok();
    if !spawned {
        SCANNING.store(false, Ordering::Relaxed);
        return "Could not start the scan.".to_owned();
    }
    "Scanning this PC for games...".to_owned()
}

/// SCAN A FOLDER: he picks a folder or drive (off the main thread), then
/// every game in it fills its row (`asset_transport::game_scan::rescan_in`);
/// [`take_scanned`] has what it found. The line to show.
pub(crate) fn start_folder_scan() -> String {
    if SCANNING.swap(true, Ordering::Relaxed) {
        return "Already scanning for games.".to_owned();
    }
    let spawned = std::thread::Builder::new()
        .name("game-folder-scan".into())
        .spawn(|| {
            let line = match game_paths::pick_folder("Pick the folder (or drive) your games are in") {
                Some(folder) => {
                    let found = asset_transport::game_scan::rescan_in(&folder);
                    scan_line(found.found, &found.set)
                }
                None => "No folder picked.".to_owned(),
            };
            if let Ok(mut s) = SCANNED.lock() {
                *s = Some(line);
            }
        })
        .is_ok();
    if !spawned {
        SCANNING.store(false, Ordering::Relaxed);
        return "Could not start the scan.".to_owned();
    }
    "Pick the folder your games are in...".to_owned()
}

/// The line about a scan: how many games it found and which it set.
pub(crate) fn scan_line(found: usize, set: &[String]) -> String {
    let games = if found == 1 { "game" } else { "games" };
    if set.is_empty() {
        format!("Found {found} {games}. No new folders: every game found was already set.")
    } else {
        format!("Found {found} {games}. New: {}. Restart the game to load them.", set.join(", "))
    }
}

/// CLEAR CACHE's first press: when (the second within `CONFIRM` clears).
static CLEAR_ARMED: Mutex<Option<std::time::Instant>> = Mutex::new(None);
const CONFIRM: std::time::Duration = std::time::Duration::from_secs(5);

/// CLEAR CACHE was pressed once and waits for the second press.
pub(crate) fn clear_armed() -> bool {
    CLEAR_ARMED.lock().ok().and_then(|a| *a).is_some_and(|t| t.elapsed() < CONFIRM)
}

/// CLEAR CACHE: the first press asks again; the second empties every cache
/// (`asset_transport::clear_cache::clear_all`) off the main thread, and
/// [`take_scanned`] has the line about it. The line to show now.
pub(crate) fn clear_cache() -> String {
    let Ok(mut armed) = CLEAR_ARMED.lock() else { return "Could not clear the cache.".to_owned() };
    if !armed.is_some_and(|t| t.elapsed() < CONFIRM) {
        *armed = Some(std::time::Instant::now());
        return "Press CLEAR CACHE again to delete the fast-load copies. The next load of each map is slower.".to_owned();
    }
    *armed = None;
    drop(armed);
    if assets::bake_all::running() {
        return "Cancel BAKE ALL first (it is writing to the cache).".to_owned();
    }
    if SCANNING.swap(true, Ordering::Relaxed) {
        return "Wait for the scan to finish.".to_owned();
    }
    let spawned = std::thread::Builder::new()
        .name("clear-cache".into())
        .spawn(|| {
            let done = asset_transport::clear_cache::clear_all();
            if let Ok(mut s) = SCANNED.lock() {
                *s = Some(format!("Cache {}.", done.summary()));
            }
        })
        .is_ok();
    if !spawned {
        SCANNING.store(false, Ordering::Relaxed);
        return "Could not clear the cache.".to_owned();
    }
    "Clearing the cache...".to_owned()
}

/// PREPARE ALL MAPS NOW: starts it (the checked games, those `off` does
/// not list, in the background; maps already made by this version and
/// settings are skipped, as PREPARE MAPS' times count them), or stops it
/// while it runs. The line to show now.
pub(crate) fn bake_all(off: &str) -> String {
    if assets::bake_all::running() {
        return stop_preparing();
    }
    match assets::bake_all::start(true, &|k| crate::shell::options::game_off(off, k)) {
        Ok(0) => "No maps to prepare: check a game in PREPARE MAPS AT LAUNCH, or set a game folder first.".to_owned(),
        Ok(n) => format!("Preparing {n} maps in the background. The bar shows each map; Esc stops it."),
        Err(e) => e,
    }
}

/// PREPARE MAPS AT LAUNCH: the start's run, skipping the maps a run already
/// made with this version and these settings (so a stopped run goes on
/// where it stopped). The line to show, if any.
pub(crate) fn prepare_at_launch(off: &str) -> Option<String> {
    if assets::bake_all::running() {
        return None;
    }
    match assets::bake_all::start(true, &|k| crate::shell::options::game_off(off, k)) {
        Ok(0) => None,
        Ok(n) => Some(format!("Preparing {n} maps (PREPARE MAPS AT LAUNCH). Esc stops it; the next start goes on from there.")),
        Err(e) => Some(e),
    }
}

/// Stops PREPARE ALL MAPS. The line to show now.
pub(crate) fn stop_preparing() -> String {
    assets::bake_all::stop();
    "Preparing maps stopped. The next run goes on from here.".to_owned()
}

/// What the scan found once it is done.
pub(crate) fn take_scanned() -> Option<String> {
    let line = SCANNED.lock().ok()?.take()?;
    SCANNING.store(false, Ordering::Relaxed);
    Some(line)
}

/// Clears `key`'s folder: the line about it.
pub(crate) fn clear(key: &str, title: &str) -> String {
    match game_paths::set(key, None) {
        Ok(()) => format!("{title} cleared. Restart the game for it to take effect."),
        Err(e) => e,
    }
}

#[derive(Component)]
struct PanelRoot;

#[derive(Component)]
struct HintRoot;

#[derive(Default)]
struct PanelState {
    selected: usize,
    /// What the panel shows: (paths as set, a line about the last change).
    drawn: Option<(Vec<(String, PathBuf)>, usize, String, bool)>,
    note: String,
}

#[allow(clippy::too_many_arguments)]
fn game_folders_panel(
    mut commands: Commands,
    keys: Res<ButtonInput<KeyCode>>,
    pads: Query<&Gamepad>,
    front: Option<Res<crate::lui_frontend::Bo2mpFrontend>>,
    font: Option<Res<GameUiFont>>,
    panels: Query<Entity, With<PanelRoot>>,
    hints: Query<Entity, With<HintRoot>>,
    mut state: Local<PanelState>,
) {
    let Some(font) = font else { return };
    let in_front = front.is_some();
    let pressed = |k: KeyCode| keys.just_pressed(k);
    let pad = |b: GamepadButton| pads.iter().any(|p| p.just_pressed(b));
    let pad_held = |b: GamepadButton| pads.iter().any(|p| p.pressed(b));
    if !in_front {
        OPEN.store(false, Ordering::Relaxed);
    } else if (pressed(KeyCode::F10) || (pad_held(GamepadButton::Select) && pad(GamepadButton::North))) && crate::shell::enabled() && !is_open() {
        // bo2mp: the shell shows the folders as an Options tab.
        WANT_SHELL.store(true, Ordering::Relaxed);
    } else if pressed(KeyCode::F10) || (pad_held(GamepadButton::Select) && pad(GamepadButton::North)) {
        OPEN.store(!is_open(), Ordering::Relaxed);
        state.note.clear();
    } else if is_open() && (pressed(KeyCode::Escape) || pad(GamepadButton::East)) {
        OPEN.store(false, Ordering::Relaxed);
    }
    // The hint in the front end's corner.
    // (The shell's footer says it instead.)
    let hint_wanted = in_front && !is_open() && !crate::shell::enabled();
    if hint_wanted && hints.is_empty() {
        commands
            .spawn((
                HintRoot,
                GlobalZIndex(20_000),
                Node {
                    position_type: PositionType::Absolute,
                    right: Val::Percent(1.5),
                    top: Val::Percent(1.5),
                    ..default()
                },
            ))
            .with_child((
                Text::new("F10  GAME FOLDERS"),
                game_text_font(&font.0, 15.0),
                TextColor(Color::srgba(1.0, 1.0, 1.0, 0.55)),
            ));
    } else if !hint_wanted {
        for e in &hints {
            commands.entity(e).despawn();
        }
    }
    if !is_open() {
        for e in &panels {
            commands.entity(e).despawn();
        }
        state.drawn = None;
        return;
    }
    let games = game_paths::GAMES;
    if pressed(KeyCode::ArrowUp) || pad(GamepadButton::DPadUp) {
        state.selected = (state.selected + games.len() - 1) % games.len();
    }
    if pressed(KeyCode::ArrowDown) || pad(GamepadButton::DPadDown) {
        state.selected = (state.selected + 1) % games.len();
    }
    let (key, title, _) = games[state.selected];
    // A folder he picked comes back from the picker.
    if let Some(note) = take_picked() {
        state.note = note;
    }
    let picking = picking();
    if !picking
        && (pressed(KeyCode::Enter) || pad(GamepadButton::South))
        && let Some(note) = start_pick(key, title)
    {
        state.note = note;
    }
    if !picking && (pressed(KeyCode::Delete) || pressed(KeyCode::Backspace) || pad(GamepadButton::West)) {
        state.note = clear(key, title);
    }
    let picking = self::picking();
    let paths = game_paths::load();
    let drawn = (paths.clone(), state.selected, state.note.clone(), picking);
    if state.drawn.as_ref() == Some(&drawn) && !panels.is_empty() {
        return;
    }
    state.drawn = Some(drawn);
    for e in &panels {
        commands.entity(e).despawn();
    }
    let f = |px: f32| game_text_font(&font.0, px);
    commands
        .spawn((
            PanelRoot,
            GlobalZIndex(20_001),
            BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.7)),
            Node {
                position_type: PositionType::Absolute,
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                ..default()
            },
        ))
        .with_children(|root| {
            root.spawn((
                BackgroundColor(Color::srgba(0.05, 0.06, 0.07, 0.95)),
                Node {
                    width: Val::Px(1100.0),
                    flex_direction: FlexDirection::Column,
                    row_gap: Val::Px(6.0),
                    padding: UiRect::all(Val::Px(28.0)),
                    ..default()
                },
            ))
            .with_children(|p| {
                p.spawn((Text::new("GAME FOLDERS"), f(34.0), TextColor(Color::WHITE)));
                p.spawn((
                    Text::new("Where each game is installed. Black Ops II is needed; the others add their maps."),
                    f(16.0),
                    TextColor(Color::srgba(1.0, 1.0, 1.0, 0.6)),
                ));
                for (i, (key, title, needed)) in games.iter().enumerate() {
                    let path = paths.iter().find(|(k, _)| k == key).map(|(_, p)| p.clone());
                    let (status, color) = match &path {
                        Some(p) if game_paths::looks_valid_for(key, p) => ("OK", Color::srgb(0.35, 0.85, 0.45)),
                        Some(_) if *key == game_paths::BF1_KEY => ("NO DATA/PATCH", Color::srgb(0.95, 0.35, 0.3)),
                        Some(_) => ("NO ZONE FOLDER", Color::srgb(0.95, 0.35, 0.3)),
                        None if *needed => ("NEEDED", Color::srgb(0.95, 0.35, 0.3)),
                        None => ("NOT SET", Color::srgba(1.0, 1.0, 1.0, 0.45)),
                    };
                    let selected = i == state.selected;
                    p.spawn((
                        BackgroundColor(if selected {
                            Color::srgba(0.95, 0.6, 0.1, 0.22)
                        } else {
                            Color::srgba(1.0, 1.0, 1.0, 0.04)
                        }),
                        Node {
                            align_items: AlignItems::Center,
                            column_gap: Val::Px(16.0),
                            padding: UiRect::axes(Val::Px(14.0), Val::Px(6.0)),
                            ..default()
                        },
                    ))
                    .with_children(|line| {
                        // (One line a game: sixteen of them fit on a 1080 screen.)
                        line.spawn((
                            Text::new(title.to_uppercase()),
                            f(18.0),
                            TextColor(if selected { Color::srgb(1.0, 0.75, 0.25) } else { Color::WHITE }),
                            Node { width: Val::Px(470.0), flex_shrink: 0.0, ..default() },
                        ));
                        line.spawn((
                            Text::new(path.map_or_else(|| "-".to_owned(), |p| p.display().to_string())),
                            f(14.0),
                            TextColor(Color::srgba(1.0, 1.0, 1.0, 0.6)),
                            Node { flex_grow: 1.0, overflow: Overflow::clip(), ..default() },
                        ));
                        line.spawn((Text::new(status), f(15.0), TextColor(color)));
                    });
                }
                if !state.note.is_empty() {
                    p.spawn((Text::new(state.note.clone()), f(16.0), TextColor(Color::srgb(1.0, 0.8, 0.4))));
                }
                p.spawn((
                    Text::new("UP/DOWN choose   ENTER (A) pick folder   DELETE (X) clear   F10 / ESC (B) close"),
                    f(15.0),
                    TextColor(Color::srgba(1.0, 1.0, 1.0, 0.5)),
                ));
            });
        });
}

pub(crate) fn register_game_folders(app: &mut App) {
    app.add_systems(Update, game_folders_panel.in_set(ClientSet::Ui));
}
