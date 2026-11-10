//! bo2mp: a small copy of Modern Warfare III (2023), made from the owner's
//! install so he can keep its guns without the whole game (Options > GAME
//! FOLDERS, X on its row): every played gun (Modern Warfare III's and II's)
//! with its parts, pictures and sounds, the guns' and camos' Create-a-Class
//! pictures, and every calling card and emblem with its frames.
//!
//! The guns are read as a match reads them while `fastfile_jup::pack`
//! notes each zone and package entry used; the gun sounds' entries come
//! from their list (a sound is decoded only when played); then the copy is
//! written (`fastfile_jup::pack::write_slim`), the calling cards and
//! emblems saved decoded (`jup_cards::export_looks`, their pictures in the
//! game are far larger than shown), the copy marked
//! (`asset_audio::PACK_MARK`) and set as Modern Warfare III's folder. The
//! copy stays on his PC; nothing of it is part of the rebuild.
//!
//! What the copy does not hold: the maps, Warzone (Resurgence) and its
//! sounds, the operators: those need the full install.

use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

use asset_transport::game_paths;

static RUNNING: AtomicBool = AtomicBool::new(false);
/// Lines for the screen, oldest first (taken by [`take_notes`]); the last
/// says it is done or why not.
static NOTES: Mutex<Vec<(String, bool)>> = Mutex::new(Vec::new());

fn say(line: &str, last: bool) {
    diag::info!(Ui, "bo2mp: Modern Warfare III small copy: {line}");
    if let Ok(mut n) = NOTES.lock() {
        n.push((line.to_owned(), last));
    }
}

/// The lines said since the last look: (text, whether it is the last).
pub(crate) fn take_notes() -> Vec<(String, bool)> {
    NOTES.lock().map(|mut n| std::mem::take(&mut *n)).unwrap_or_default()
}

/// A copy is being made.
pub(crate) fn running() -> bool {
    RUNNING.load(Ordering::Relaxed)
}

/// Starts a copy: asks where to put it (a folder picker), then makes it off
/// the main thread. The line to show now.
pub(crate) fn start() -> String {
    let Some(install) = asset_audio::jup_install() else {
        return "Set Modern Warfare III's folder first (the full game): the small copy is made from it.".to_owned();
    };
    if install.join(asset_audio::PACK_MARK).is_file() {
        return "Modern Warfare III's folder is already a small copy: set the full game's folder to make another.".to_owned();
    }
    if RUNNING.swap(true, Ordering::Relaxed) {
        return "A small copy is already being made.".to_owned();
    }
    let spawned = std::thread::Builder::new().name("jup-small-copy".into()).spawn(move || {
        let dest = game_paths::pick_folder("Where should the small copy of Modern Warfare III go? (an empty folder)");
        match dest {
            None => say("No folder picked: no copy made.", true),
            Some(dest) => match make(&install, &dest) {
                Ok(line) => say(&line, true),
                Err(e) => say(&format!("The small copy failed: {e}"), true),
            },
        }
        RUNNING.store(false, Ordering::Relaxed);
    });
    if spawned.is_err() {
        RUNNING.store(false, Ordering::Relaxed);
        return "The small copy could not start.".to_owned();
    }
    "Pick a folder for the small copy in the window that opened.".to_owned()
}

fn gb(bytes: u64) -> String {
    format!("{:.1} GB", bytes as f64 / 1_073_741_824.0)
}

/// bo2mp `CodLegacy export-mw3 <folder>`: the copy made from the command line
/// (the lines it says printed as it goes).
pub fn export_from_command_line(dest: &Path) -> Result<String, String> {
    let install = asset_audio::jup_install().ok_or("Modern Warfare III's folder is not set (game_paths.cfg's jup line, or BO2_JUP_INSTALL)")?;
    if install.join(asset_audio::PACK_MARK).is_file() {
        return Err("Modern Warfare III's folder is already a small copy".to_owned());
    }
    RUNNING.store(true, Ordering::Relaxed);
    let printer = std::thread::spawn(|| {
        while RUNNING.load(Ordering::Relaxed) {
            for (line, _) in take_notes() {
                println!("{line}");
            }
            std::thread::sleep(std::time::Duration::from_millis(250));
        }
    });
    let out = make(&install, dest);
    RUNNING.store(false, Ordering::Relaxed);
    let _ = printer.join();
    for (line, _) in take_notes() {
        println!("{line}");
    }
    out
}

/// Makes the copy of `install` in `dest` (see the module docs).
fn make(install: &Path, dest: &Path) -> Result<String, String> {
    let same = |a: &Path, b: &Path| a.canonicalize().ok().zip(b.canonicalize().ok()).is_some_and(|(a, b)| a == b || b.starts_with(&a));
    if same(install, dest) {
        return Err("pick a folder outside the game's own".to_owned());
    }
    std::fs::create_dir_all(dest).map_err(|e| format!("{}: {e}", dest.display()))?;
    let t = std::time::Instant::now();
    say("Reading every Modern Warfare III gun as a match does (a few minutes)...", false);
    fastfile_jup::pack::record_start();
    let guns = assets::touch_jup_guns();
    say("Reading the guns' and camos' pictures...", false);
    let icons = crate::jup_hud::touch_gun_icons();
    let (mut keys, mut zones) = fastfile_jup::pack::record_stop();
    say("Listing the guns' sounds...", false);
    keys.extend(asset_audio::jup_gun_sound_keys(install)?);
    zones.extend(fastfile_jup::pack::zone_paths(install, asset_audio::JUP_GUN_ZONES));
    zones.extend(fastfile_jup::pack::zone_paths(install, &["global_stream_mp"]));
    // (Only the install's own zones: a match read while this ran may have
    // read another game's.)
    let zone_dir = install.join("zone");
    zones.retain(|z| z.file_name().is_some_and(|n| zone_dir.join(n).is_file()));
    say(&format!("Writing {} zones and the packages' {} entries the guns use...", zones.len(), keys.len()), false);
    let report = fastfile_jup::pack::write_slim(install, dest, &keys, &zones, &mut |l| say(l, false))?;
    say("Saving the calling cards and emblems...", false);
    let (looks, look_bytes) = crate::jup_cards::export_looks(dest, &mut |l| say(l, false))?;
    let gun_lines = guns.iter().filter(|l| l.contains("first person")).count();
    let summary = format!(
        "A small copy of Modern Warfare III (2023), made by the BO2 rebuild from {}.\nGuns: {gun_lines}; gun pictures: {icons}; calling cards and emblems: {looks}.\nZones: {}; packages: {} ({} entries).\nSize: {}.\nIt holds the guns, their sounds and pictures, and the calling cards and emblems; the maps, Warzone and the operators need the full game.\n",
        install.display(),
        report.zones,
        report.packages,
        report.entries,
        gb(report.bytes + look_bytes),
    );
    std::fs::write(dest.join(asset_audio::PACK_MARK), summary).map_err(|e| format!("{}: {e}", asset_audio::PACK_MARK))?;
    game_paths::set(crate::jup_cards::PREFIX, Some(PathBuf::from(dest)))?;
    Ok(format!(
        "Small copy done in {:.0} min: {} at {}. Modern Warfare III now loads from it (restart the game). Its maps and Warzone need the full game's folder set again.",
        t.elapsed().as_secs_f64() / 60.0,
        gb(report.bytes + look_bytes),
        dest.display()
    ))
}
