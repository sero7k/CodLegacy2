//! bo2mp: emptying the bake (the Options "clear cache" calls this through
//! `asset_transport::clear_cache::clear_all`).
//!
//! Only the files the bake itself writes are deleted, by their exact names
//! ([`MADE`], and any of them half-written: `<name>.part<pid>`), in the folders
//! it makes (`<root>\<game>\<map>`); a folder left empty after that goes too.
//! Anything else found there (a file someone put in the bake folder) stays,
//! and so does the root itself, which may be a folder the owner named.

use std::path::Path;

/// Black Ops II: a map's pictures as stored in the packs, in one file
/// (`asset_t6::images::PackSet::preload_file`).
pub const T6_PICTURES: &str = "t6pictures.pak";

/// The files a map's folder holds, by exact name: Black Ops III's pictures
/// (`pictures`), Black Ops II's (`T6_PICTURES`) and the other games'
/// `asset_material::map_pak` kinds (`<kind>.pak` + `<kind>.idx`). (A new
/// baked file adds its name here, or "clear cache" leaves it behind.)
pub const MADE: &[&str] = &[crate::pictures::FILE, T6_PICTURES, "manifest.json", "mips.pak", "mips.idx", "programs.pak", "programs.idx", "finished_guns.pak"];

/// A half-written file: one of [`MADE`], `.part` and the writer's process id.
fn is_part(name: &str) -> bool {
    MADE.iter().any(|m| name.strip_prefix(m).and_then(|r| r.strip_prefix(".part")).is_some_and(|pid| !pid.is_empty() && pid.bytes().all(|b| b.is_ascii_digit())))
}

/// What one clear did: files deleted and their bytes, and a line for each
/// thing left (in use, a stranger's file, a folder skipped).
#[derive(Debug, Default)]
pub struct Cleared {
    pub files: u64,
    pub bytes: u64,
    pub notes: Vec<String>,
}

impl Cleared {
    /// Deletes one file, counted (or noted when it cannot go: the game
    /// still has it open).
    pub fn remove_file(&mut self, path: &Path) {
        let len = std::fs::metadata(path).map_or(0, |m| m.len());
        match std::fs::remove_file(path) {
            Ok(()) => {
                self.files += 1;
                self.bytes += len;
            }
            Err(e) => self.notes.push(format!("kept {}: {e}", path.display())),
        }
    }

    /// One line for the log and the Options screen.
    pub fn summary(&self) -> String {
        let mb = self.bytes as f64 / 1_048_576.0;
        if self.notes.is_empty() {
            format!("cleared {} files ({mb:.0} MB)", self.files)
        } else {
            format!("cleared {} files ({mb:.0} MB); {} left (see the log)", self.files, self.notes.len())
        }
    }
}

/// Empties the bake under its root (`CODLEGACY_BAKE_DIR`, else
/// `D:\codlegacy-bake`) and under every game's own root
/// (`CODLEGACY_BAKE_DIR_<GAME>`), also when baking is off
/// (`CODLEGACY_BAKE=0`).
pub fn clear(out: &mut Cleared) {
    let root = codlegacy_env::var_os("CODLEGACY_BAKE_DIR").filter(|d| !d.is_empty()).map(std::path::PathBuf::from).or_else(crate::default_root);
    if let Some(root) = root {
        clear_in(&root, out);
    }
    for game in crate::GAMES {
        let own = codlegacy_env::var_os(format!("CODLEGACY_BAKE_DIR_{}", game.to_ascii_uppercase())).filter(|d| !d.is_empty()).map(std::path::PathBuf::from);
        if let Some(own) = own {
            // (A game's own root holds its maps directly: the root stands
            // in for the game folder.)
            clear_game(&own, out);
            let _ = std::fs::remove_dir(&own);
        }
    }
}

/// Empties the baked files of every map folder under `game`.
fn clear_game(game: &Path, out: &mut Cleared) {
    for map in dirs(game) {
        for e in std::fs::read_dir(&map).into_iter().flatten().flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            let path = e.path();
            if !path.is_file() {
                continue;
            }
            if MADE.contains(&name.as_str()) || is_part(&name) {
                out.remove_file(&path);
            } else {
                out.notes.push(format!("kept {} (not a baked file)", path.display()));
            }
        }
        // (Only goes when empty.)
        let _ = std::fs::remove_dir(&map);
    }
}

/// Empties the bake under `root` (see the module notes).
pub fn clear_in(root: &Path, out: &mut Cleared) {
    // (PREPARE ALL MAPS' record of what it made: with the files gone, every
    // map is to be made again.)
    let record = root.join(crate::ahead::RECORD);
    if record.is_file() {
        out.remove_file(&record);
    }
    for game in dirs(root) {
        clear_game(&game, out);
        let _ = std::fs::remove_dir(&game);
    }
}

/// The folders directly in `dir` (not links).
fn dirs(dir: &Path) -> Vec<std::path::PathBuf> {
    std::fs::read_dir(dir).into_iter().flatten().flatten().filter(|e| e.file_type().is_ok_and(|t| t.is_dir())).map(|e| e.path()).collect()
}
