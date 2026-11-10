//! The first launch asks where each game is installed (only the games the
//! automatic scan, `asset_transport::game_scan`, did not find).
//!
//! Black Ops II is needed; Black Ops, Modern Warfare 2, Modern Warfare 3,
//! World at War, Call of Duty 4, Ghosts and Advanced Warfare add their maps
//! when present. A game added later (Ghosts, Advanced Warfare) is asked for
//! once on the next launch, keeping the folders already set. Each game
//! gets a folder picker (Cancel skips it); a folder without a `zone` folder
//! is asked for again once. The answers go to `game_paths.cfg`
//! (`asset_transport::game_paths`) and can be changed later in the front
//! end's Game Folders panel.

use std::path::PathBuf;

use asset_transport::{call_of_claude, game_paths, game_scan};

/// Asks for the game folders when they were never set (the games the
/// scan found are not asked for).
pub(crate) fn first_run() {
    if codlegacy_env::var_os("CODLEGACY_NO_FOLDER_PROMPT").is_some() {
        // (The scan still sets what it finds.)
        game_scan::wait_background();
        return;
    }
    // Busy PC: the scan walks every drive for the games not set (a first
    // launch, or a game this build adds: 342-736 s on his busy hard drive).
    // With Black Ops II set the menu needs none of it, so it goes on and the
    // scan finishes behind it, writing game_paths.cfg for the next launch.
    // `CODLEGACY_GAME_SCAN_WAIT=1` waits here (the old way).
    let wait = codlegacy_env::var("CODLEGACY_GAME_SCAN_WAIT").is_ok_and(|v| v.trim() == "1");
    let bo2_set = game_paths::load().iter().any(|(k, p)| k == "bo2" && game_paths::looks_valid(p));
    if !wait && bo2_set && game_scan::still_scanning() {
        diag::info!(Zone, "game scan: still running; the menu goes on (what it finds is used from the next launch)");
        return;
    }
    // Every game found on this PC is set without asking (the launch's
    // scan, `game_scan`: Steam libraries, launchers, his folders, then each
    // drive's); only what is not found is asked for. (What was asked
    // before this launch: the scan writes every game's line.)
    let detected = game_scan::wait_background();
    let asked = game_scan::asked_before_scan();
    if !detected.is_empty() {
        diag::info!(Zone, "game folders: found {}", detected.join(", "));
    }
    let set: Vec<String> = game_paths::load()
        .into_iter()
        .filter(|(_, p)| game_paths::looks_valid(p))
        .map(|(k, _)| k)
        .collect();
    let new: Vec<_> = game_paths::GAMES
        .into_iter()
        .filter(|(k, _, _)| *k != game_paths::BF1_KEY && !asked.iter().any(|a| a == k) && !set.iter().any(|s| s == k))
        .collect();
    if new.is_empty() {
        return;
    }
    // The very first launch: the games installed, or only the Call of
    // Claude folder (the files the rebuild reads, copied out of them).
    if asked.is_empty() && !set.iter().any(|k| k == "bo2") {
        match ask_source() {
            Source::Pack => {
                if pick_pack() {
                    return;
                }
            }
            Source::Games => {}
        }
    }
    let mut chosen: Vec<(String, PathBuf)> = game_paths::load();
    for (key, title, needed) in new {
        let note = if needed { "needed" } else { "optional: Cancel to skip" };
        let mut tries = 0;
        while tries < 2 {
            tries += 1;
            let Some(folder) = game_paths::pick_folder(&format!("Where is {title} installed? ({note})")) else {
                break;
            };
            if game_paths::looks_valid(&folder) {
                chosen.push((key.to_owned(), folder));
                break;
            }
            message(&format!(
                "{} does not look like {title}'s folder (it has no `zone` folder). Pick the folder the game's .exe is in.",
                folder.display()
            ));
        }
    }
    match game_paths::save(&chosen) {
        Ok(()) => diag::info!(Zone, "game folders: {} set", chosen.len()),
        Err(error) => diag::warn!(Zone, "game folders: {error}"),
    }
}

/// Where the games' files are.
enum Source {
    /// Every game installed in its own folder.
    Games,
    /// Only the Call of Claude folder.
    Pack,
}

/// Asks which: the games installed (Yes) or only the required files (No).
/// Off Windows (no box to ask in), the games.
fn ask_source() -> Source {
    let text = format!(
        "Do you have the games installed?\n\n\
         Yes: I have Black Ops II and the other Call of Duty games installed. You will be asked where each one is.\n\n\
         No: I only have the required files (the \"{}\" folder). You will be asked where it is; without it, you will be asked for each game instead.",
        call_of_claude::FOLDER_NAME
    );
    match ask_yes_no(&text) {
        Some(false) => Source::Pack,
        _ => Source::Games,
    }
}

/// Asks for the Call of Claude folder and points every game into it; false
/// when he cancels or it is not one: then he is told each game will be
/// asked for instead.
fn pick_pack() -> bool {
    let found = pick_pack_folder();
    if !found {
        message(&format!(
            "No {} folder was chosen, so the game will ask for each game's folder instead: Black Ops II, Black Ops, Modern Warfare 2, Modern Warfare 3, Black Ops III, Advanced Warfare and Modern Warfare III (2023), then the optional ones. Press Cancel on any game you do not have.",
            call_of_claude::FOLDER_NAME
        ));
    }
    found
}

fn pick_pack_folder() -> bool {
    for _ in 0..2 {
        let Some(folder) = game_paths::pick_folder(&format!("Where is the {} folder? (Cancel if you do not have it)", call_of_claude::FOLDER_NAME)) else {
            return false;
        };
        match call_of_claude::adopt(&folder) {
            Ok(n) => {
                diag::info!(Zone, "game folders: {n} games from {}", folder.display());
                return true;
            }
            Err(error) => message(&error),
        }
    }
    false
}

/// A Yes / No box (Windows): `None` off Windows or when it cannot be shown.
fn ask_yes_no(text: &str) -> Option<bool> {
    #[cfg(windows)]
    {
        // (A single-quoted PowerShell string, each line break a [char]10.)
        let text = text.replace('\'', "''").replace('\n', "' + [char]10 + '");
        let script = format!(
            "Add-Type -AssemblyName System.Windows.Forms; \
             $r = [System.Windows.Forms.MessageBox]::Show(('{text}'), 'Call of Claude - first setup', 'YesNo', 'Question'); \
             [Console]::Out.Write($r)"
        );
        let out = std::process::Command::new("powershell.exe")
            .args(["-NoProfile", "-STA", "-ExecutionPolicy", "Bypass", "-Command", &script])
            .output()
            .ok()?;
        match String::from_utf8_lossy(&out.stdout).trim() {
            "Yes" => Some(true),
            "No" => Some(false),
            _ => None,
        }
    }
    #[cfg(not(windows))]
    {
        let _ = text;
        None
    }
}

/// A plain message box (Windows); the log elsewhere.
fn message(text: &str) {
    diag::warn!(Zone, "game folders: {text}");
    #[cfg(windows)]
    {
        let text = text.replace('\'', "''");
        let script = format!(
            "Add-Type -AssemblyName System.Windows.Forms; [System.Windows.Forms.MessageBox]::Show('{text}', 'Game folders') | Out-Null"
        );
        let _ = std::process::Command::new("powershell.exe")
            .args(["-NoProfile", "-STA", "-ExecutionPolicy", "Bypass", "-Command", &script])
            .status();
    }
}
