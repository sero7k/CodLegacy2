//! bo2mp: the one-time map bake.
//!
//! A guest game's map takes most of its load reading and converting what
//! comes out the same every time for the same install: Black Ops III's
//! Skyjacked spent 28.5 s of his 43.7 s load reading 953 MB of pictures from
//! about 2000 scattered places in the game's packages on a hard drive. The
//! bake keeps that output in one file per map, ready for the graphics card
//! and in the order the load wants it, so the next load reads it start to
//! end instead.
//!
//! Where: `CODLEGACY_BAKE_DIR`, else `D:\codlegacy-bake` when there is a
//! `D:` drive; with neither, nothing is baked (never the system drive
//! without the owner's word). One folder per game and map:
//! `<root>\<game>\<map>\` holding `pictures.pak` and `manifest.json`.
//! One game's folder can sit somewhere else: `CODLEGACY_BAKE_DIR_T7`
//! (the game's name in capitals) is that game's root, so Black Ops III's
//! big files (20-30 GB) can go on the solid-state drive (the owner's word,
//! 7 Nov 2026: `C:\codlegacy-bake\t7`) while the rest stays on the data
//! drive. The maps go in `<that root>\<map>\`, no game folder between.
//!
//! An entry is found by a key over everything it was made from (the source
//! files' names, sizes and times written, the settings that change the
//! output, the format's number and the converting code's own text); a
//! different key, or a file that does not read back whole, is a miss and
//! the map loads the old way (and bakes again).
//!
//! `CODLEGACY_BAKE=0` turns all of it off.

use std::path::{Path, PathBuf};

pub mod ahead;
pub mod clear;
pub mod map_file;
pub mod pictures;
pub mod status;
pub mod why;

/// Each read ahead's number: a newer one stops the older (another map).
static READ_AHEAD: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// Reads `path` start to end into the system's file cache on a thread of
/// its own and throws the bytes away, so the load that wants it later reads
/// it from memory: started as a load begins, the hard drive streams the
/// map's baked file while the load does its other work. A newer call stops
/// an older one. `on_end` hears how it went (one line). Nothing when the
/// file is missing or `CODLEGACY_BAKE_READ_AHEAD=0`.
pub fn read_ahead(path: PathBuf, on_end: impl FnOnce(String) + Send + 'static) {
    use std::io::Read;
    use std::sync::atomic::Ordering;
    let n = READ_AHEAD.fetch_add(1, Ordering::Relaxed) + 1;
    if codlegacy_env::var("CODLEGACY_BAKE_READ_AHEAD").is_ok_and(|v| v.trim() == "0") || !path.is_file() {
        return;
    }
    let _ = std::thread::Builder::new().name("bake_read_ahead".into()).spawn(move || {
        let t = std::time::Instant::now();
        let Ok(mut f) = std::fs::File::open(&path) else { return };
        let mut buf = vec![0u8; 8 << 20];
        let mut total = 0u64;
        loop {
            if READ_AHEAD.load(Ordering::Relaxed) != n {
                on_end(format!("bake: read ahead of {} stopped after {} MB (another map)", path.display(), total >> 20));
                return;
            }
            match f.read(&mut buf) {
                Ok(0) => break,
                Ok(k) => total += k as u64,
                Err(e) => {
                    on_end(format!("bake: read ahead of {} stopped: {e}", path.display()));
                    return;
                }
            }
        }
        on_end(format!("bake: {} read ahead ({} MB in {:.1}s)", path.display(), total >> 20, t.elapsed().as_secs_f32()));
    });
}

/// Whether baked maps are read and written (`CODLEGACY_BAKE=0`: no).
pub fn enabled() -> bool {
    !codlegacy_env::var("CODLEGACY_BAKE").is_ok_and(|v| v.trim() == "0")
}

/// The folder baked maps go in, if any (see the module notes).
pub fn root() -> Option<PathBuf> {
    if !enabled() {
        return None;
    }
    if let Some(dir) = codlegacy_env::var_os("CODLEGACY_BAKE_DIR").filter(|d| !d.is_empty()) {
        return Some(PathBuf::from(dir));
    }
    default_root()
}

#[cfg(windows)]
fn default_root() -> Option<PathBuf> {
    Path::new("D:\\").is_dir().then(|| PathBuf::from("D:\\codlegacy-bake"))
}

#[cfg(not(windows))]
fn default_root() -> Option<PathBuf> {
    None
}

/// One game's own root, if set (`CODLEGACY_BAKE_DIR_<GAME>`); its maps go
/// straight under it.
pub fn game_root(game: &str) -> Option<PathBuf> {
    if !enabled() {
        return None;
    }
    codlegacy_env::var_os(format!("CODLEGACY_BAKE_DIR_{}", clean(game).to_ascii_uppercase())).filter(|d| !d.is_empty()).map(PathBuf::from)
}

/// The folder of one game's map: `<root>\<game>\<map>` (not made yet), or
/// `<the game's own root>\<map>` when that game has one.
pub fn map_dir(game: &str, map: &str) -> Option<PathBuf> {
    if let Some(own) = game_root(game) {
        return Some(own.join(clean(map)));
    }
    Some(root()?.join(clean(game)).join(clean(map)))
}

/// The games that may have a root of their own (for CLEAR CACHE).
pub const GAMES: [&str; 12] = ["t4", "t5", "t6", "t7", "iw3", "iw4", "iw5", "iw6", "s1", "h1", "iw7", "jup"];

/// A name safe as one folder name.
fn clean(name: &str) -> String {
    let s: String = name.chars().map(|c| if c.is_ascii_alphanumeric() || c == '_' || c == '-' || c == '.' { c } else { '_' }).collect();
    if s.is_empty() || s.chars().all(|c| c == '.') { "_".into() } else { s }
}

/// A 64-bit FNV-1a key over everything an entry was made from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Key(u64);

impl Default for Key {
    fn default() -> Self {
        Self::new()
    }
}

impl Key {
    pub fn new() -> Self {
        Self(0xcbf2_9ce4_8422_2325)
    }

    pub fn value(&self) -> u64 {
        self.0
    }

    pub fn bytes(&mut self, b: &[u8]) -> &mut Self {
        for x in b {
            self.0 ^= u64::from(*x);
            self.0 = self.0.wrapping_mul(0x0100_0000_01b3);
        }
        self
    }

    /// A string, ended so that `"ab" + "c"` and `"a" + "bc"` differ.
    pub fn str(&mut self, s: &str) -> &mut Self {
        self.bytes(s.as_bytes()).bytes(&[0])
    }

    pub fn u64(&mut self, v: u64) -> &mut Self {
        self.bytes(&v.to_le_bytes())
    }

    /// A source file: its name, size and time written (a missing file
    /// counts as missing).
    pub fn file(&mut self, path: &Path) -> &mut Self {
        self.meta(path, std::fs::metadata(path).ok())
    }

    fn meta(&mut self, path: &Path, m: Option<std::fs::Metadata>) -> &mut Self {
        let name = path.file_name().map_or(String::new(), |n| n.to_string_lossy().to_ascii_lowercase());
        let len = m.as_ref().map_or(u64::MAX, |m| m.len());
        let at = m.and_then(|m| m.modified().ok()).and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map_or(0, |d| d.as_nanos());
        self.str(&name).u64(len).bytes(&at.to_le_bytes())
    }

    /// Every file in `dir` whose extension is one of `exts` (any case),
    /// sorted by name.
    pub fn dir_files(&mut self, dir: &Path, exts: &[&str]) -> &mut Self {
        // (The folder listing's own sizes and times: no extra call per file.)
        let mut files: Vec<(PathBuf, Option<std::fs::Metadata>)> = std::fs::read_dir(dir)
            .into_iter()
            .flatten()
            .flatten()
            .map(|e| (e.path(), e.metadata().ok()))
            .filter(|(p, _)| p.extension().is_some_and(|x| exts.iter().any(|e| x.eq_ignore_ascii_case(e))))
            .collect();
        files.sort_by(|a, b| a.0.cmp(&b.0));
        self.str(&dir.to_string_lossy().to_ascii_lowercase()).u64(files.len() as u64);
        for (f, m) in files {
            self.meta(&f, m);
        }
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_separates_strings() {
        let mut a = Key::new();
        a.str("ab").str("c");
        let mut b = Key::new();
        b.str("a").str("bc");
        assert_ne!(a, b);
    }

    #[test]
    fn folder_names_are_clean() {
        assert_eq!(clean("mp_skyjacked"), "mp_skyjacked");
        assert_eq!(clean("..\\x:y"), ".._x_y");
        assert_eq!(clean(".."), "_");
    }
}
