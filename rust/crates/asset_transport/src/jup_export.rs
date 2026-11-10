//! bo2mp: where the exported Modern Warfare III (2023) assets are
//! (`CodLegacy export-jup <folder>`, `docs/JUP.md` "The exported guns"): the guns,
//! their materials and pictures, the camos, the calling cards and emblems
//! and the gun sounds in our own converted formats, so they can be used
//! without the game installed.
//!
//! The folder is `$BO2_JUP_EXPORT`, else the first of the default places
//! that holds an export (a folder with `manifest.json`):
//! `MW3_export` beside the running program, in the working directory, in
//! the user's Documents (and OneDrive's), and beside the folder set for
//! Modern Warfare III (2023) (`Documents\MW3` -> `Documents\MW3_export`).
//! A full install, when there is one, is read first; the export stands in
//! when there is not.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

/// Names the export folder.
pub const ENV: &str = "BO2_JUP_EXPORT";
/// The folder every default place is looked in under this name.
pub const DEFAULT_NAME: &str = "MW3_export";
/// The file that marks an export.
pub const MANIFEST: &str = "manifest.json";
/// The folders of an export.
pub const GUNS_DIR: &str = "guns";
pub const SOUNDS_DIR: &str = "sounds";
pub const ICONS_DIR: &str = "icons";
/// A gun's file under `guns` (both its models; its materials and pictures
/// are in `materials` and `textures`).
pub const GUN_EXT: &str = "jgun";

/// Whether a folder is an export.
#[must_use]
pub fn is_export(dir: &Path) -> bool {
    dir.join(MANIFEST).is_file() && dir.join(GUNS_DIR).is_dir()
}

/// The places an export is looked for, best first.
#[must_use]
pub fn candidates() -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = Vec::new();
    if let Some(set) = std::env::var_os(ENV).filter(|p| !p.is_empty()) {
        v.push(PathBuf::from(set));
    }
    if let Some(exe_dir) = std::env::current_exe().ok().and_then(|e| e.parent().map(Path::to_path_buf)) {
        v.push(exe_dir.join(DEFAULT_NAME));
    }
    v.push(PathBuf::from(DEFAULT_NAME));
    let homes = ["USERPROFILE", "HOME"].into_iter().filter_map(|k| std::env::var_os(k).filter(|p| !p.is_empty())).map(PathBuf::from);
    for home in homes {
        v.push(home.join("Documents").join(DEFAULT_NAME));
        v.push(home.join("OneDrive").join("Documents").join(DEFAULT_NAME));
        v.push(home.join(DEFAULT_NAME));
    }
    if let Some(game) = crate::game_paths::get("jup")
        && let Some(parent) = game.parent()
    {
        v.push(parent.join(DEFAULT_NAME));
        v.push(game.join(DEFAULT_NAME));
    }
    v
}

/// The export folder: `$BO2_JUP_EXPORT` when it holds one, else the first
/// default place that does. Looked for once per process.
#[must_use]
pub fn dir() -> Option<PathBuf> {
    static DIR: OnceLock<Option<PathBuf>> = OnceLock::new();
    DIR.get_or_init(|| candidates().into_iter().find(|d| is_export(d))).clone()
}

/// The guns the export holds (the internal names without `jup_`, as
/// `bo2_profile::jup_names`), sorted.
#[must_use]
pub fn guns(dir: &Path) -> Vec<String> {
    let Ok(rd) = std::fs::read_dir(dir.join(GUNS_DIR)) else {
        return Vec::new();
    };
    let mut v: Vec<String> = rd
        .flatten()
        .filter_map(|e| {
            let p = e.path();
            (p.extension().is_some_and(|x| x == GUN_EXT)).then(|| p.file_stem().map(|s| s.to_string_lossy().into_owned()))?
        })
        .collect();
    v.sort();
    v
}

/// The most a pack file holds (a file to copy by hand stays small).
pub const PACK_BYTES: usize = 24 * 1024 * 1024;

/// Where one blob is: a pack file of the folder, from `offset`, `len` long.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Blob {
    pub file: String,
    pub offset: u64,
    pub len: u64,
}

/// Writes named blobs into pack files `<prefix>_000.pack`, `_001.pack` ... of
/// at most [`PACK_BYTES`] each (a blob larger than that has a file of its
/// own) and their list, `<prefix>.idx` (`name<TAB>file<TAB>offset<TAB>len` a
/// line). Many small pictures in few files: the folder is copied by hand.
pub struct PackWriter {
    dir: PathBuf,
    prefix: String,
    buf: Vec<u8>,
    number: usize,
    index: String,
    pub bytes: u64,
    pub count: usize,
}

impl PackWriter {
    /// A writer into `dir` (made if missing).
    pub fn new(dir: &Path, prefix: &str) -> Result<Self, String> {
        std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        Ok(Self { dir: dir.to_path_buf(), prefix: prefix.to_owned(), buf: Vec::new(), number: 0, index: String::new(), bytes: 0, count: 0 })
    }

    fn flush(&mut self) -> Result<(), String> {
        if self.buf.is_empty() {
            return Ok(());
        }
        let name = format!("{}_{:03}.pack", self.prefix, self.number);
        self.number += 1;
        let path = self.dir.join(&name);
        std::fs::write(&path, &self.buf).map_err(|e| format!("{}: {e}", path.display()))?;
        self.buf.clear();
        Ok(())
    }

    /// Adds a blob under `name` (no tab or line break in it).
    pub fn add(&mut self, name: &str, bytes: &[u8]) -> Result<(), String> {
        if !self.buf.is_empty() && self.buf.len() + bytes.len() > PACK_BYTES {
            self.flush()?;
        }
        let file = format!("{}_{:03}.pack", self.prefix, self.number);
        let clean = name.replace(['\t', '\n', '\r'], " ");
        self.index.push_str(&format!("{clean}\t{file}\t{}\t{}\n", self.buf.len(), bytes.len()));
        self.buf.extend_from_slice(bytes);
        self.bytes += bytes.len() as u64;
        self.count += 1;
        Ok(())
    }

    /// Writes the last pack file and the list.
    pub fn finish(mut self) -> Result<(), String> {
        self.flush()?;
        let path = self.dir.join(format!("{}.idx", self.prefix));
        std::fs::write(&path, &self.index).map_err(|e| format!("{}: {e}", path.display()))
    }
}

/// A folder's packs of one prefix, read back: the list at once, a blob when
/// asked for.
pub struct PackIndex {
    dir: PathBuf,
    blobs: std::collections::HashMap<String, Blob>,
    /// The names in the order they were written.
    order: Vec<String>,
}

impl PackIndex {
    /// The list of `<prefix>.idx` in `dir`, or `None` without one.
    #[must_use]
    pub fn open(dir: &Path, prefix: &str) -> Option<Self> {
        let text = std::fs::read_to_string(dir.join(format!("{prefix}.idx"))).ok()?;
        let mut blobs = std::collections::HashMap::new();
        let mut order = Vec::new();
        for line in text.lines() {
            let f: Vec<&str> = line.split('\t').collect();
            let [name, file, offset, len] = f[..] else { continue };
            let (Ok(offset), Ok(len)) = (offset.parse(), len.parse()) else { continue };
            order.push(name.to_owned());
            blobs.insert(name.to_owned(), Blob { file: file.to_owned(), offset, len });
        }
        Some(Self { dir: dir.to_path_buf(), blobs, order })
    }

    /// The names, in the order they were written.
    #[must_use]
    pub fn names(&self) -> &[String] {
        &self.order
    }

    #[must_use]
    pub fn has(&self, name: &str) -> bool {
        self.blobs.contains_key(name)
    }

    /// The blob called `name`.
    #[must_use]
    pub fn get(&self, name: &str) -> Option<Vec<u8>> {
        read_blob(&self.dir, self.blobs.get(name)?)
    }
}

/// A blob of a pack file in `dir`.
#[must_use]
pub fn read_blob(dir: &Path, blob: &Blob) -> Option<Vec<u8>> {
    use std::io::{Read, Seek, SeekFrom};
    let mut f = std::fs::File::open(dir.join(&blob.file)).ok()?;
    f.seek(SeekFrom::Start(blob.offset)).ok()?;
    let mut out = vec![0u8; usize::try_from(blob.len).ok()?];
    f.read_exact(&mut out).ok()?;
    Some(out)
}

/// What one part of an export holds, for the manifest and the report.
#[derive(Clone, Debug, Default)]
pub struct Section {
    pub name: String,
    pub count: usize,
    pub bytes: u64,
}
