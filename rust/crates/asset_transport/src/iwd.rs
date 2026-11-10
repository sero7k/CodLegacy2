use std::collections::HashMap;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock, RwLock};

#[derive(Clone, Debug)]
pub struct IwdFile {
    archive: PathBuf,
    entry: String,
    crc32: u32,
    size: u64,
    /// Where the entry starts in its archive.
    offset: u64,
}

impl IwdFile {
    pub fn entry(&self) -> &str {
        &self.entry
    }

    /// lane B (B3): the archive and where in it the entry starts, so a batch
    /// of reads can go front to back through each archive.
    pub fn disk_position(&self) -> (&Path, u64) {
        (&self.archive, self.offset)
    }

    pub fn crc32(&self) -> u32 {
        self.crc32
    }

    pub fn size(&self) -> u64 {
        self.size
    }

    pub fn read(&self) -> Result<Vec<u8>, String> {
        ENTRY_PAYLOAD_READS.fetch_add(1, Ordering::Relaxed);
        read_indexed_entry(self, None)
    }

    /// Inflate at most `limit` bytes of the entry.
    ///
    /// A header is a fixed prefix, and deflate stops where the reader stops:
    /// asking whether an image is a cubemap costs the first block of it, not
    /// the megabyte behind it.
    pub fn read_header(&self, limit: usize) -> Result<Vec<u8>, String> {
        ENTRY_HEADER_READS.fetch_add(1, Ordering::Relaxed);
        read_indexed_entry(self, Some(limit))
    }
}

#[derive(Debug, Default)]
pub struct IwdIndex {
    images: HashMap<String, Vec<IwdFile>>,
    archives: usize,
}

fn index_cache() -> &'static RwLock<HashMap<PathBuf, Arc<IwdIndex>>> {
    static CACHE: OnceLock<RwLock<HashMap<PathBuf, Arc<IwdIndex>>>> = OnceLock::new();
    CACHE.get_or_init(|| RwLock::new(HashMap::new()))
}

type IndexBuild = Arc<OnceLock<Result<Arc<IwdIndex>, String>>>;

fn index_builds() -> &'static Mutex<HashMap<PathBuf, IndexBuild>> {
    static BUILDS: OnceLock<Mutex<HashMap<PathBuf, IndexBuild>>> = OnceLock::new();
    BUILDS.get_or_init(|| Mutex::new(HashMap::new()))
}

impl IwdIndex {
    pub fn open(directory: &Path) -> Result<Arc<Self>, String> {
        let key = directory.to_path_buf();
        if let Some(hit) = index_cache()
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .get(&key)
        {
            return Ok(Arc::clone(hit));
        }

        let slot = {
            let mut builds = index_builds()
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            Arc::clone(builds.entry(key.clone()).or_default())
        };

        let built = slot
            .get_or_init(|| Self::open_uncached(directory).map(Arc::new))
            .clone();
        {
            let mut builds = index_builds()
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if builds
                .get(&key)
                .is_some_and(|live| Arc::ptr_eq(live, &slot))
            {
                builds.remove(&key);
            }
        }
        let built = built?;
        let mut cache = index_cache()
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        Ok(Arc::clone(
            cache.entry(key).or_insert_with(|| Arc::clone(&built)),
        ))
    }

    pub fn is_cached(directory: &Path) -> bool {
        index_cache()
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .contains_key(directory)
    }

    pub fn archive_count(&self) -> usize {
        self.archives
    }

    /// Every image name the archives hold (lower case, no extension).
    pub fn image_names(&self) -> impl Iterator<Item = &str> {
        self.images.keys().map(String::as_str)
    }

    pub fn image_candidates(&self, name: &str) -> Option<&[IwdFile]> {
        self.images
            .get(&name.to_ascii_lowercase())
            .map(Vec::as_slice)
    }

    fn open_uncached(directory: &Path) -> Result<Self, String> {
        let mut archives = std::fs::read_dir(directory)
            .map_err(|error| format!("cannot read IWD directory {}: {error}", directory.display()))?
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|path| {
                path.extension()
                    .is_some_and(|extension| extension.eq_ignore_ascii_case("iwd"))
            })
            .collect::<Vec<_>>();
        archives.sort();
        // bo2mp: the usermaps' archives (`<main>/../usermaps/<map>/*.iwd`)
        // come after the game's own, so a stock image keeps its stock file;
        // one that does not index is left out instead of failing the game.
        let stock = archives.len();
        // (bo2mp: IW4x's DLC map packs beside MW2's own archives.)
        archives.extend(crate::discover::iw4x_iwds_for_main(directory));
        archives.extend(crate::discover::usermap_iwds_for_main(directory));

        type IndexedArchive = (usize, Result<Vec<(String, IwdFile)>, String>);
        let next = std::sync::atomic::AtomicUsize::new(0);
        let done: Mutex<Vec<IndexedArchive>> = Mutex::new(Vec::with_capacity(archives.len()));
        let lanes = archives.len().min(index_lane_width());
        std::thread::scope(|scope| {
            for _ in 0..lanes {
                scope.spawn(|| {
                    loop {
                        let slot = next.fetch_add(1, Ordering::Relaxed);
                        let Some(path) = archives.get(slot) else {
                            return;
                        };
                        let indexed = index_image_entries(path);
                        done.lock()
                            .unwrap_or_else(std::sync::PoisonError::into_inner)
                            .push((slot, indexed));
                    }
                });
            }
        });
        let mut done = done
            .into_inner()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        done.sort_by_key(|(slot, _)| *slot);
        let mut per_archive = Vec::with_capacity(done.len());
        for (slot, indexed) in done {
            match indexed {
                Ok(entries) => {
                    if slot >= stock {
                        diag::info!(
                            Zone,
                            "usermap IWD {}: {} images",
                            archives[slot].display(),
                            entries.len()
                        );
                    }
                    per_archive.push(entries);
                }
                Err(error) if slot >= stock => {
                    diag::warn!(Zone, "usermap IWD skipped: {error}");
                }
                Err(error) => return Err(error),
            }
        }

        let mut images: HashMap<String, Vec<IwdFile>> = HashMap::new();
        for entries in per_archive {
            for (name, file) in entries {
                images.entry(name).or_default().push(file);
            }
        }
        Ok(Self {
            images,
            archives: archives.len(),
        })
    }
}

/// One file's line in an archive's central directory: its name, CRC,
/// unpacked size and where its header starts, which is all the image index
/// keeps.
struct DirectoryEntry {
    name: String,
    crc32: u32,
    size: u64,
    offset: u64,
}

/// An archive's central directory in two reads (its tail, then the
/// directory itself). `ZipArchive::by_index` seeks to every file's own
/// header too, one random read per file: on a cold hard disk MW2's
/// archives and IW4x's took minutes that way. `None` for anything this
/// does not read plainly (Zip64, data before the archive, a name that is
/// not UTF-8): the zip crate reads those.
fn read_central_directory(path: &Path) -> Option<Vec<DirectoryEntry>> {
    use std::io::{Seek, SeekFrom};
    fn u16_at(bytes: &[u8], at: usize) -> Option<u16> {
        Some(u16::from_le_bytes(bytes.get(at..at + 2)?.try_into().ok()?))
    }
    fn u32_at(bytes: &[u8], at: usize) -> Option<u32> {
        Some(u32::from_le_bytes(bytes.get(at..at + 4)?.try_into().ok()?))
    }
    let mut file = std::fs::File::open(path).ok()?;
    let len = file.metadata().ok()?.len();
    // The end record is 22 bytes plus a comment of up to 64 KiB.
    let tail_len = len.min(22 + 0xFFFF);
    file.seek(SeekFrom::Start(len - tail_len)).ok()?;
    let mut tail = vec![0; tail_len as usize];
    file.read_exact(&mut tail).ok()?;
    let end = (0..tail.len().checked_sub(21)?)
        .rev()
        .find(|&at| u32_at(&tail, at) == Some(0x0605_4b50))?;
    let count = u16_at(&tail, end + 10)?;
    let dir_len = u32_at(&tail, end + 12)?;
    let dir_at = u32_at(&tail, end + 16)?;
    if count == 0xFFFF || dir_len == u32::MAX || dir_at == u32::MAX {
        return None;
    }
    if u64::from(dir_at) + u64::from(dir_len) != len - tail_len + end as u64 {
        return None;
    }
    file.seek(SeekFrom::Start(u64::from(dir_at))).ok()?;
    let mut dir = vec![0; dir_len as usize];
    file.read_exact(&mut dir).ok()?;
    let mut entries = Vec::with_capacity(usize::from(count));
    let mut at = 0;
    for _ in 0..count {
        if u32_at(&dir, at)? != 0x0201_4b50 {
            return None;
        }
        let crc32 = u32_at(&dir, at + 16)?;
        let size = u32_at(&dir, at + 24)?;
        let name_len = usize::from(u16_at(&dir, at + 28)?);
        let extra_len = usize::from(u16_at(&dir, at + 30)?);
        let comment_len = usize::from(u16_at(&dir, at + 32)?);
        // (The local header's offset: the same as the zip crate's
        // `header_start`, as no data comes before the archive.)
        let offset = u32_at(&dir, at + 42)?;
        if size == u32::MAX || offset == u32::MAX {
            return None;
        }
        let name = std::str::from_utf8(dir.get(at + 46..at + 46 + name_len)?).ok()?;
        entries.push(DirectoryEntry {
            name: name.to_owned(),
            crc32,
            size: u64::from(size),
            offset: u64::from(offset),
        });
        at += 46 + name_len + extra_len + comment_len;
    }
    (at == dir.len()).then_some(entries)
}

/// The image's name in the index (`images/<name>.iwi`, lower case), or
/// `None` for any other file.
fn image_name_of(exact: &str) -> Option<String> {
    let normalized = exact.replace('\\', "/");
    let name = normalized
        .strip_prefix("images/")
        .and_then(|name| name.strip_suffix(".iwi"))?;
    (!name.is_empty() && !name.contains('/')).then(|| name.to_ascii_lowercase())
}

fn index_image_entries(path: &Path) -> Result<Vec<(String, IwdFile)>, String> {
    if let Some(directory) = read_central_directory(path) {
        return Ok(directory
            .into_iter()
            .filter_map(|entry| {
                let name = image_name_of(&entry.name)?;
                Some((
                    name,
                    IwdFile {
                        archive: path.to_path_buf(),
                        entry: entry.name,
                        crc32: entry.crc32,
                        size: entry.size,
                        offset: entry.offset,
                    },
                ))
            })
            .collect());
    }
    let file = std::fs::File::open(path)
        .map_err(|error| format!("cannot open {}: {error}", path.display()))?;
    let mut archive = zip::ZipArchive::new(file)
        .map_err(|error| format!("cannot index {}: {error}", path.display()))?;
    let mut entries = Vec::new();
    for entry_index in 0..archive.len() {
        // (Only an image's own header is read: the name comes from the
        // central directory.)
        if archive.name_for_index(entry_index).and_then(image_name_of).is_none() {
            continue;
        }
        let entry = archive
            .by_index(entry_index)
            .map_err(|error| format!("cannot read {}: {error}", path.display()))?;
        let exact = entry.name().to_owned();
        let Some(name) = image_name_of(&exact) else {
            continue;
        };
        entries.push((
            name,
            IwdFile {
                archive: path.to_path_buf(),
                entry: exact,
                crc32: entry.crc32(),
                size: entry.size(),
                offset: entry.header_start(),
            },
        ));
    }
    Ok(entries)
}

type OpenArchive = zip::ZipArchive<std::io::BufReader<std::fs::File>>;

static PARKED: OnceLock<Mutex<HashMap<PathBuf, Vec<OpenArchive>>>> = OnceLock::new();

const PARKED_LIMIT: usize = 256;

static PARKED_N: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

fn index_lane_width() -> usize {
    static WIDTH: OnceLock<usize> = OnceLock::new();
    *WIDTH.get_or_init(|| {
        codlegacy_env::var("CODLEGACY_IWD_INDEX_LANES")
            .ok()
            .and_then(|value| value.trim().parse::<usize>().ok())
            .filter(|lanes| *lanes > 0)
            .unwrap_or(4)
    })
}

fn parked() -> &'static Mutex<HashMap<PathBuf, Vec<OpenArchive>>> {
    PARKED.get_or_init(|| Mutex::new(HashMap::new()))
}

struct ArchiveLease {
    path: PathBuf,
    archive: Option<OpenArchive>,
}

impl ArchiveLease {
    fn take(path: &Path) -> Result<Self, String> {
        let parked = parked()
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .get_mut(path)
            .and_then(Vec::pop);
        if let Some(archive) = parked {
            PARKED_N.fetch_sub(1, Ordering::Relaxed);
            return Ok(Self {
                path: path.to_path_buf(),
                archive: Some(archive),
            });
        }
        let opened_at = std::time::Instant::now();
        let disk = std::fs::File::open(path)
            .map_err(|error| format!("cannot open {}: {error}", path.display()))?;
        let archive = zip::ZipArchive::new(std::io::BufReader::new(disk))
            .map_err(|error| format!("cannot read {}: {error}", path.display()))?;
        IWD_DIRECTORY_NS.fetch_add(opened_at.elapsed().as_nanos() as u64, Ordering::Relaxed);
        IWD_DIRECTORY_OPENS.fetch_add(1, Ordering::Relaxed);
        Ok(Self {
            path: path.to_path_buf(),
            archive: Some(archive),
        })
    }
}

impl Drop for ArchiveLease {
    fn drop(&mut self) {
        let Some(archive) = self.archive.take() else {
            return;
        };

        if PARKED_N.fetch_add(1, Ordering::Relaxed) >= PARKED_LIMIT {
            PARKED_N.fetch_sub(1, Ordering::Relaxed);
            return;
        }
        parked()
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .entry(self.path.clone())
            .or_default()
            .push(archive);
    }
}

static ENTRY_PAYLOAD_READS: AtomicU64 = AtomicU64::new(0);
static ENTRY_HEADER_READS: AtomicU64 = AtomicU64::new(0);

static IWD_DIRECTORY_NS: AtomicU64 = AtomicU64::new(0);
static IWD_DIRECTORY_OPENS: AtomicU64 = AtomicU64::new(0);
static IWD_INFLATE_NS: AtomicU64 = AtomicU64::new(0);

fn read_indexed_entry(file: &IwdFile, limit: Option<usize>) -> Result<Vec<u8>, String> {
    read_pooled_entry(&file.archive, &file.entry, limit)
}

fn read_pooled_entry(
    archive_path: &Path,
    entry_name: &str,
    limit: Option<usize>,
) -> Result<Vec<u8>, String> {
    let mut lease = ArchiveLease::take(archive_path)?;
    let archive = lease
        .archive
        .as_mut()
        .expect("lease holds its reader until drop");
    let inflate_at = std::time::Instant::now();
    let mut entry = archive
        .by_name(entry_name)
        .map_err(|error| format!("cannot read {entry_name}: {error}"))?;
    let want = limit.map_or(entry.size() as usize, |limit| {
        limit.min(entry.size() as usize)
    });
    let mut bytes = Vec::with_capacity(want);
    let read = match limit {
        Some(limit) => entry.take(limit as u64).read_to_end(&mut bytes),
        None => entry.read_to_end(&mut bytes),
    };
    read.map_err(|error| format!("cannot read {entry_name}: {error}"))?;
    IWD_INFLATE_NS.fetch_add(inflate_at.elapsed().as_nanos() as u64, Ordering::Relaxed);
    Ok(bytes)
}

pub fn cached_iwd_dirs() -> Vec<PathBuf> {
    let mut dirs = index_cache()
        .read()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .keys()
        .cloned()
        .collect::<Vec<_>>();
    dirs.sort();
    dirs
}

/// Entries inflated whole, and entries inflated only far enough to read a
/// header.
pub fn iwd_entry_reads() -> (u64, u64) {
    (
        ENTRY_PAYLOAD_READS.load(Ordering::Relaxed),
        ENTRY_HEADER_READS.load(Ordering::Relaxed),
    )
}

pub fn iwd_read_cost() -> (f64, u64, f64) {
    (
        IWD_DIRECTORY_NS.load(Ordering::Relaxed) as f64 / 1.0e6,
        IWD_DIRECTORY_OPENS.load(Ordering::Relaxed),
        IWD_INFLATE_NS.load(Ordering::Relaxed) as f64 / 1.0e6,
    )
}

pub fn game_main_for_zone(zone_ff: &Path) -> Result<PathBuf, String> {
    for ancestor in zone_ff.ancestors() {
        let main = ancestor.join("main");
        if main.is_dir() {
            return Ok(main);
        }
    }
    // bo2zm: a map zone from an extra folder (`CODLEGACY_T6_EXTRA`) uses the
    // install's game tree.
    let install = crate::t6_extra::install_zone_dir(zone_ff);
    if let Some(main) = install
        .ancestors()
        .map(|a| a.join("main"))
        .find(|m| m.is_dir())
    {
        return Ok(main);
    }
    Err(format!(
        "no game tree with a `main/` directory above {}",
        zone_ff.display()
    ))
}

pub fn game_mains_under(games_root: &Path) -> Vec<PathBuf> {
    let mut mains = Vec::new();
    let mut stack = crate::discover::search_roots(games_root);
    while let Some(dir) = stack.pop() {
        let main = dir.join("main");
        if main.is_dir() {
            mains.push(main);
        }
        if let Ok(entries) = std::fs::read_dir(&dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    stack.push(path);
                }
            }
        }
    }
    mains.sort();
    mains
}

/// A named file from any IWD under the games root (`mp/basemaps.arena`,
/// `default_xboxlive.cfg`). The first launch walks every IWD zip to find it
/// (seconds on a cold disk); the find is then kept in the bake folder
/// (`<bake>\iwd\<name>.note`: which IWD, its size and time, the bytes) and
/// a later launch checks that one IWD's size and time instead. A changed or
/// missing IWD walks again. `CODLEGACY_BAKE=0` turns the note off.
pub fn read_iwd_named(games_root: &Path, want: &str) -> Option<Vec<u8>> {
    let want = want.replace('\\', "/");
    let note = iwd_note_path(games_root, &want);
    if let Some(bytes) = note.as_deref().and_then(iwd_note_read) {
        return Some(bytes);
    }
    let (archive, bytes) = read_iwd_named_walk(games_root, &want)?;
    if let Some(note) = note {
        iwd_note_write(&note, &archive, &bytes);
    }
    Some(bytes)
}

const IWD_NOTE_MAGIC: &[u8; 4] = b"IWDN";
const IWD_NOTE_FORMAT: u32 = 1;

fn iwd_note_path(games_root: &Path, want: &str) -> Option<PathBuf> {
    let root = map_bake::root()?;
    let name: String = want
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '_' || c == '-' || c == '.' { c } else { '_' })
        .collect();
    let mut key = map_bake::Key::new();
    key.str(&games_root.to_string_lossy().to_ascii_lowercase()).str(want);
    Some(root.join("iwd").join(format!("{name}.{:016x}.note", key.value())))
}

fn iwd_note_key(archive: &Path) -> u64 {
    let mut key = map_bake::Key::new();
    key.str(&archive.to_string_lossy().to_ascii_lowercase()).file(archive);
    key.value()
}

fn iwd_note_read(note: &Path) -> Option<Vec<u8>> {
    let data = std::fs::read(note).ok()?;
    let mut at = 0usize;
    let take = |at: &mut usize, n: usize| -> Option<&[u8]> {
        let out = data.get(*at..*at + n)?;
        *at += n;
        Some(out)
    };
    let u64_at = |at: &mut usize| -> Option<u64> {
        Some(u64::from_le_bytes(take(at, 8)?.try_into().ok()?))
    };
    if take(&mut at, 4)? != IWD_NOTE_MAGIC
        || u32::from_le_bytes(take(&mut at, 4)?.try_into().ok()?) != IWD_NOTE_FORMAT
    {
        return None;
    }
    let path_len = u64_at(&mut at)? as usize;
    let archive = PathBuf::from(std::str::from_utf8(take(&mut at, path_len)?).ok()?);
    let key = u64_at(&mut at)?;
    if !archive.is_file() || iwd_note_key(&archive) != key {
        let _ = std::fs::remove_file(note);
        return None;
    }
    let len = u64_at(&mut at)? as usize;
    Some(take(&mut at, len)?.to_vec())
}

fn iwd_note_write(note: &Path, archive: &Path, bytes: &[u8]) {
    let Some(dir) = note.parent() else { return };
    if std::fs::create_dir_all(dir).is_err() {
        return;
    }
    let path = archive.to_string_lossy();
    let mut out = Vec::with_capacity(bytes.len() + path.len() + 40);
    out.extend_from_slice(IWD_NOTE_MAGIC);
    out.extend_from_slice(&IWD_NOTE_FORMAT.to_le_bytes());
    out.extend_from_slice(&(path.len() as u64).to_le_bytes());
    out.extend_from_slice(path.as_bytes());
    out.extend_from_slice(&iwd_note_key(archive).to_le_bytes());
    out.extend_from_slice(&(bytes.len() as u64).to_le_bytes());
    out.extend_from_slice(bytes);
    let part = note.with_extension("part");
    if std::fs::write(&part, &out).is_ok() && std::fs::rename(&part, note).is_err() {
        let _ = std::fs::remove_file(&part);
    }
}

/// The walk itself: every IWD under every search root, in name order.
fn read_iwd_named_walk(games_root: &Path, want: &str) -> Option<(PathBuf, Vec<u8>)> {
    let mut stack = crate::discover::search_roots(games_root);
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        let mut paths = entries
            .flatten()
            .map(|entry| entry.path())
            .collect::<Vec<_>>();
        paths.sort();
        for path in paths {
            if path.is_dir() {
                stack.push(path);
                continue;
            }
            if !path
                .extension()
                .is_some_and(|extension| extension.eq_ignore_ascii_case("iwd"))
            {
                continue;
            }
            let Ok(file) = std::fs::File::open(&path) else {
                continue;
            };
            let Ok(mut archive) = zip::ZipArchive::new(file) else {
                continue;
            };
            // (The names come from the central directory: no file's own
            // header is read until the one wanted.)
            let Some(name) = archive
                .file_names()
                .find(|n| n.replace('\\', "/").eq_ignore_ascii_case(want))
                .map(str::to_owned)
            else {
                continue;
            };
            if let Ok(mut entry) = archive.by_name(&name) {
                let mut bytes = Vec::new();
                if entry.read_to_end(&mut bytes).is_ok() {
                    return Some((path, bytes));
                }
            }
        }
    }
    None
}

/// bo2mp: one file from a game's own IWDs (`<main>/*.iwd`, the game's
/// load order: a later archive's copy wins), by its path inside them
/// (`vision/mp_shipment.vision`); `None` when none holds it.
pub fn read_main_iwd_file(main: &Path, want: &str) -> Option<Vec<u8>> {
    let want = want.replace('\\', "/");
    let mut iwds: Vec<PathBuf> = std::fs::read_dir(main)
        .ok()?
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x.eq_ignore_ascii_case("iwd")))
        .collect();
    iwds.sort_by_key(|p| p.file_name().map(|n| n.to_ascii_lowercase()));
    for path in iwds.iter().rev() {
        let Ok(file) = std::fs::File::open(path) else { continue };
        let Ok(mut archive) = zip::ZipArchive::new(file) else { continue };
        // (The names come from the central directory: no entry is opened
        // until the one wanted.)
        let Some(name) = archive
            .file_names()
            .find(|n| n.replace('\\', "/").eq_ignore_ascii_case(&want))
            .map(str::to_owned)
        else {
            continue;
        };
        let Ok(mut entry) = archive.by_name(&name) else { continue };
        let mut bytes = Vec::new();
        if entry.read_to_end(&mut bytes).is_ok() {
            return Some(bytes);
        }
    }
    None
}

pub fn read_text(path: &Path) -> Result<String, String> {
    std::fs::read_to_string(path).map_err(|error| format!("read {}: {error}", path.display()))
}

pub fn inflate_zlib(data: &[u8]) -> Result<Vec<u8>, String> {
    miniz_oxide::inflate::decompress_to_vec_zlib(data)
        .map_err(|error| format!("zlib inflate: {error:?}"))
}

#[derive(Debug, Default, Clone)]
pub struct IwdSoundIndex {
    dir: PathBuf,

    // (Shared: the match's open takes the boot's index, `opened_indexes`.)
    sounds: std::sync::Arc<HashMap<String, (PathBuf, String)>>,
}

/// Load speed: each archive folder's sound index, once per run. The boot
/// opens the same folders the match does, and the match's second open (a
/// directory listing, a stat per archive and the cached index read back)
/// took over 20 s on a busy hard drive while AudioReady waited on it.
/// `CODLEGACY_IWD_INDEX_MEMO=0` opens every time, as before.
fn opened_indexes() -> Option<&'static std::sync::Mutex<HashMap<PathBuf, IwdSoundIndex>>> {
    static MEMO: std::sync::OnceLock<Option<std::sync::Mutex<HashMap<PathBuf, IwdSoundIndex>>>> =
        std::sync::OnceLock::new();
    MEMO.get_or_init(|| {
        let off = cfg!(test)
            || codlegacy_env::var_os("CODLEGACY_IWD_INDEX_MEMO").is_some_and(|v| v == "0");
        (!off).then(|| std::sync::Mutex::new(HashMap::new()))
    })
    .as_ref()
}

fn sound_key(relative: &str) -> String {
    relative.replace('\\', "/").to_ascii_lowercase()
}

fn sound_rel_from_zip_name(name: &str, language: Option<&str>) -> Option<String> {
    let lower = name.replace('\\', "/").to_ascii_lowercase();
    let lower = language
        .and_then(|language| lower.strip_prefix(language)?.strip_prefix('/'))
        .unwrap_or(&lower);
    let rel = lower.strip_prefix("sound/")?;
    if rel.is_empty() {
        return None;
    }
    Some(rel.to_owned())
}

/// bo2mp: a sound index saved once and read back in milliseconds while its
/// folder's archives (names, sizes, times) are unchanged: each archive's
/// listing was a seek on his hard drive, for every game at every launch.
const SOUND_CACHE: &str = "iwd-sounds";

fn sound_cache_key(directory: &Path, archives: &[PathBuf]) -> String {
    // FNV-1a: the same key from build to build.
    fn eat(hash: u64, bytes: &[u8]) -> u64 {
        bytes
            .iter()
            .fold(hash, |h, b| (h ^ u64::from(*b)).wrapping_mul(0x0100_0000_01b3))
    }
    let mut hash = eat(0xcbf2_9ce4_8422_2325, directory.to_string_lossy().as_bytes());
    for path in archives {
        hash = eat(hash, path.to_string_lossy().as_bytes());
        if let Ok(meta) = std::fs::metadata(path) {
            hash = eat(hash, &meta.len().to_le_bytes());
            let time = meta
                .modified()
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map_or(0, |t| t.as_nanos());
            hash = eat(hash, &time.to_le_bytes());
        }
    }
    format!("v1-{hash:016x}.txt")
}

/// The file: each archive once (`a<TAB>path`), then each sound
/// (`s<TAB>archive number<TAB>relative path<TAB>entry name`).
fn write_sound_cache(key: &str, sounds: &HashMap<String, (PathBuf, String)>) {
    if cfg!(test) {
        return;
    }
    let mut archives: Vec<&PathBuf> = sounds.values().map(|(path, _)| path).collect();
    archives.sort();
    archives.dedup();
    let mut text = String::new();
    for path in &archives {
        text.push_str(&format!("a\t{}\n", path.to_string_lossy()));
    }
    for (rel, (path, name)) in sounds {
        let Ok(slot) = archives.binary_search(&path) else {
            continue;
        };
        text.push_str(&format!("s\t{slot}\t{rel}\t{name}\n"));
    }
    let _ = crate::artifact_cache::cache_put(SOUND_CACHE, key, text.as_bytes());
}

fn read_sound_cache(key: &str) -> Option<HashMap<String, (PathBuf, String)>> {
    if cfg!(test) {
        return None;
    }
    let bytes = crate::artifact_cache::cache_get(SOUND_CACHE, key)?;
    let text = String::from_utf8(bytes).ok()?;
    let mut archives = Vec::new();
    let mut sounds = HashMap::new();
    for line in text.lines() {
        let mut parts = line.split('\t');
        match parts.next()? {
            "a" => archives.push(PathBuf::from(parts.next()?)),
            "s" => {
                let path = archives.get(parts.next()?.parse::<usize>().ok()?)?;
                let rel = parts.next()?;
                let name = parts.next()?;
                sounds.insert(rel.to_owned(), (path.clone(), name.to_owned()));
            }
            _ => return None,
        }
    }
    Some(sounds)
}

fn archive_language(path: &Path) -> Option<String> {
    let stem = path.file_stem()?.to_str()?.to_ascii_lowercase();
    let (language, _) = stem.strip_prefix("localized_")?.rsplit_once('_')?;
    Some(language.to_owned())
}

impl IwdSoundIndex {
    pub fn open(directory: impl AsRef<Path>) -> Result<Self, String> {
        let directory = directory.as_ref();
        if let Some(index) = opened_indexes()
            .and_then(|memo| memo.lock().ok()?.get(directory).cloned())
        {
            return Ok(index);
        }
        let index = Self::open_uncached(directory)?;
        if let Some(Ok(mut memo)) = opened_indexes().map(|memo| memo.lock()) {
            memo.insert(directory.to_path_buf(), index.clone());
        }
        Ok(index)
    }

    fn open_uncached(directory: &Path) -> Result<Self, String> {
        let mut archives = std::fs::read_dir(directory)
            .map_err(|e| format!("cannot read {}: {e}", directory.display()))?
            .filter_map(std::result::Result::ok)
            .map(|e| e.path())
            .filter(|p| {
                p.extension()
                    .is_some_and(|ext| ext.eq_ignore_ascii_case("iwd"))
            })
            .collect::<Vec<_>>();
        archives.sort();
        // bo2mp: the usermaps' archives after the game's own (a stock sound
        // keeps its stock file); one that does not open is left out.
        let stock = archives.len();
        // (bo2mp: IW4x's DLC map packs beside MW2's own archives.)
        archives.extend(crate::discover::iw4x_iwds_for_main(directory));
        archives.extend(crate::discover::usermap_iwds_for_main(directory));

        let key = sound_cache_key(directory, &archives);
        if let Some(sounds) = read_sound_cache(&key) {
            return Ok(Self {
                dir: directory.to_path_buf(),
                sounds: std::sync::Arc::new(sounds),
            });
        }
        let mut sounds = HashMap::new();
        for (slot, path) in archives.into_iter().enumerate() {
            let opened = std::fs::File::open(&path)
                .map_err(|e| format!("cannot open {}: {e}", path.display()))
                .and_then(|file| {
                    zip::ZipArchive::new(std::io::BufReader::new(file))
                        .map_err(|e| format!("zip {}: {e}", path.display()))
                });
            let archive = match opened {
                Ok(archive) => archive,
                Err(error) if slot >= stock => {
                    diag::warn!(Zone, "usermap IWD sounds skipped: {error}");
                    continue;
                }
                Err(error) => return Err(error),
            };
            let language = archive_language(&path);
            for name in archive.file_names() {
                let Some(rel) = sound_rel_from_zip_name(name, language.as_deref()) else {
                    continue;
                };
                sounds
                    .entry(rel)
                    .or_insert_with(|| (path.clone(), name.to_owned()));
            }
        }
        write_sound_cache(&key, &sounds);
        Ok(Self {
            dir: directory.to_path_buf(),
            sounds: std::sync::Arc::new(sounds),
        })
    }

    pub fn indexed_dir(&self) -> &Path {
        &self.dir
    }

    pub fn sound_count(&self) -> usize {
        self.sounds.len()
    }

    /// bo2mp: every sound path it holds (relative to `sound/`, lower case),
    /// for finding a game's music by name (the Zombies menu music).
    pub fn sound_paths(&self) -> impl Iterator<Item = &str> {
        self.sounds.keys().map(String::as_str)
    }

    pub fn read_sound(&self, relative: &str) -> Option<Result<Vec<u8>, String>> {
        let (archive, entry) = self.sounds.get(&sound_key(relative))?;
        Some(read_pooled_entry(archive, entry, None))
    }
}

#[cfg(test)]
mod main_iwd_tests {
    use super::read_main_iwd_file;
    use std::io::Write;

    fn iwd(path: &std::path::Path, name: &str, text: &str) {
        let mut z = zip::ZipWriter::new(std::fs::File::create(path).unwrap());
        let opts = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
        z.start_file(name, opts).unwrap();
        z.write_all(text.as_bytes()).unwrap();
        z.finish().unwrap();
    }

    #[test]
    fn the_later_archive_wins_and_names_ignore_case() {
        let dir = std::env::temp_dir().join(format!("iwdtest{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        iwd(&dir.join("iw_00.iwd"), "vision/mp_x.vision", "old");
        iwd(&dir.join("iw_05.iwd"), "Vision/MP_X.vision", "new");
        assert_eq!(read_main_iwd_file(&dir, "vision/mp_x.vision").as_deref(), Some(&b"new"[..]));
        assert!(read_main_iwd_file(&dir, "vision/mp_y.vision").is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }
}

#[cfg(test)]
mod image_index_tests {
    use super::{index_image_entries, read_central_directory};
    use std::io::Write;

    #[test]
    fn the_central_directory_matches_the_zip_crate() {
        let dir = std::env::temp_dir().join(format!("iwdcd{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("iw_00.iwd");
        let mut z = zip::ZipWriter::new(std::fs::File::create(&path).unwrap());
        let stored = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
        let packed = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
        z.start_file("images/Gun_A.iwi", stored).unwrap();
        z.write_all(b"pixels a").unwrap();
        z.start_file("images/sub/skip.iwi", stored).unwrap();
        z.write_all(b"in a folder").unwrap();
        z.start_file("sound/shot.wav", packed).unwrap();
        z.write_all(&[7; 4096]).unwrap();
        z.start_file("images/gun_b.iwi", packed).unwrap();
        z.write_all(&[3; 9000]).unwrap();
        z.set_comment("an archive comment");
        z.finish().unwrap();

        let directory = read_central_directory(&path).expect("plain archive");
        let mut archive = zip::ZipArchive::new(std::fs::File::open(&path).unwrap()).unwrap();
        assert_eq!(directory.len(), archive.len());
        for (index, entry) in directory.iter().enumerate() {
            let file = archive.by_index(index).unwrap();
            assert_eq!(entry.name, file.name());
            assert_eq!(entry.crc32, file.crc32());
            assert_eq!(entry.size, file.size());
        }
        let images = index_image_entries(&path).unwrap();
        let names: Vec<_> = images.iter().map(|(name, file)| (name.as_str(), file.entry(), file.size())).collect();
        assert_eq!(names, [("gun_a", "images/Gun_A.iwi", 8), ("gun_b", "images/gun_b.iwi", 9000)]);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
