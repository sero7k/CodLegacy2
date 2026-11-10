//! bo2mp: the start-up guard of the upscalers. While an upscaler draws, a
//! "running" marker file sits in the profile folder; a clean exit removes it.
//! A marker found at start-up means the last run ended with an upscaler on
//! and did not exit cleanly (a crash, a lost device, a killed process): the
//! game then starts with upscaling Off (and saves that), so a crash caused by
//! an upscaler is never repeated on the next launch.
//!
//! `BO2MP_UPSCALE_SAFE=1` forces Off for the whole run, for diagnosis, without
//! touching the saved setting.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// The marker's file name, in the profile folder (beside `settings.cfg`).
pub const MARKER_FILE: &str = "upscaler-running.marker";

/// The text last written, so an unchanged state does not touch the disk again.
static LAST: Mutex<Option<String>> = Mutex::new(None);

/// What start-up found.
#[derive(Debug, PartialEq, Eq)]
pub enum Startup {
    /// No marker, no override: upscaling is as the settings say.
    Clean,
    /// `BO2MP_UPSCALE_SAFE=1`: Off for this run, the setting kept.
    SafeMode,
    /// A marker of a run that did not exit cleanly (its text: which
    /// upscaler, size and process); upscaling goes Off, and the setting too.
    Crashed(String),
}

/// A switch that is on: set, not empty, not `0` or `false`.
pub fn truthy(value: &str) -> bool {
    let v = value.trim();
    !v.is_empty() && v != "0" && !v.eq_ignore_ascii_case("false") && !v.eq_ignore_ascii_case("off")
}

/// `BO2MP_UPSCALE_SAFE` is on.
pub fn safe_env() -> bool {
    std::env::var("BO2MP_UPSCALE_SAFE").is_ok_and(|v| truthy(&v))
}

pub fn marker_path(dir: &Path) -> PathBuf {
    dir.join(MARKER_FILE)
}

/// The process id a marker was written by (its first line: `pid=<n> ...`).
fn marker_pid(text: &str) -> Option<u32> {
    text.split_whitespace()
        .find_map(|word| word.strip_prefix("pid="))
        .and_then(|n| n.parse().ok())
}

/// Whether another game process, still running, owns a marker (two games on
/// one profile folder: its marker is not a crash). Only known on Linux.
fn other_process_alive(pid: u32) -> bool {
    #[cfg(target_os = "linux")]
    {
        pid != std::process::id() && Path::new(&format!("/proc/{pid}")).exists()
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = pid;
        false
    }
}

/// Reads (and removes) the marker the last run left. `safe` is
/// `safe_env()`. Call once, before the renderer first asks for the setting.
pub fn startup(dir: &Path, safe: bool) -> Startup {
    let path = marker_path(dir);
    let text = std::fs::read_to_string(&path).ok();
    let crashed = text
        .as_deref()
        .filter(|t| !marker_pid(t).is_some_and(other_process_alive))
        .map(|t| t.trim().to_owned());
    if crashed.is_some() {
        let _ = std::fs::remove_file(&path);
    }
    if safe {
        return Startup::SafeMode;
    }
    match crashed {
        Some(text) => Startup::Crashed(text),
        None => Startup::Clean,
    }
}

/// An upscaler is drawing (`what`: which, at what size): writes the marker,
/// if that text is not what is there already. Errors are ignored; the guard
/// is a convenience, never a reason to stop the game.
pub fn begin(dir: &Path, what: &str) {
    let text = format!("pid={} {what}", std::process::id());
    let Ok(mut last) = LAST.lock() else {
        return;
    };
    if last.as_deref() == Some(text.as_str()) {
        return;
    }
    let _ = std::fs::create_dir_all(dir);
    if std::fs::write(marker_path(dir), text.as_bytes()).is_ok() {
        *last = Some(text);
    }
}

/// No upscaler draws any more, or the game is leaving cleanly: removes the
/// marker.
pub fn end(dir: &Path) {
    let Ok(mut last) = LAST.lock() else {
        return;
    };
    if last.take().is_some() {
        let _ = std::fs::remove_file(marker_path(dir));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("bo2mp_upscale_guard_{name}_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn switches_read_as_expected() {
        for on in ["1", "true", "yes", " 1 "] {
            assert!(truthy(on), "{on:?}");
        }
        for off in ["", "0", "false", "FALSE", "off", "  "] {
            assert!(!truthy(off), "{off:?}");
        }
    }

    #[test]
    fn a_marker_left_behind_turns_the_next_start_off_once() {
        let dir = scratch("crash");
        assert_eq!(startup(&dir, false), Startup::Clean);

        // A run that drew with an upscaler and died: the marker stays. (A
        // pid of a process that is gone, as the next launch would see.)
        std::fs::write(marker_path(&dir), "pid=4000000 FSR 3.1 1715x965").unwrap();
        match startup(&dir, false) {
            Startup::Crashed(text) => assert!(text.contains("FSR 3.1"), "{text}"),
            other => panic!("{other:?}"),
        }
        // (Read once: the launch after that starts as the settings say.)
        assert!(!marker_path(&dir).exists());
        assert_eq!(startup(&dir, false), Startup::Clean);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_safe_switch_wins_and_still_clears_the_marker() {
        let dir = scratch("safe");
        assert_eq!(startup(&dir, true), Startup::SafeMode);
        std::fs::write(marker_path(&dir), "pid=4000000 FSR 1").unwrap();
        assert_eq!(startup(&dir, true), Startup::SafeMode);
        assert!(!marker_path(&dir).exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_clean_run_leaves_no_marker() {
        let dir = scratch("clean");
        begin(&dir, "FSR 1 1280x720");
        let text = std::fs::read_to_string(marker_path(&dir)).unwrap();
        assert!(text.contains("FSR 1 1280x720"));
        assert_eq!(marker_pid(&text), Some(std::process::id()));
        // (The same text again does not write again.)
        std::fs::remove_file(marker_path(&dir)).unwrap();
        begin(&dir, "FSR 1 1280x720");
        assert!(!marker_path(&dir).exists());
        begin(&dir, "FSR 3.1 1280x720");
        assert!(marker_path(&dir).exists());
        end(&dir);
        assert!(!marker_path(&dir).exists());
        assert_eq!(startup(&dir, false), Startup::Clean);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn the_marker_of_a_game_still_running_is_not_a_crash() {
        let dir = scratch("other");
        // pid 1 is alive and is not us.
        std::fs::write(marker_path(&dir), "pid=1 FSR 1").unwrap();
        assert_eq!(startup(&dir, false), Startup::Clean);
        assert!(marker_path(&dir).exists());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
