//! Black Ops II image packs on disk: open their indexes, find a GfxImage's
//! entry, read and decode it. Packs are only read.

use std::fs::File;
use std::path::{Path, PathBuf};

use ipak_t6::{IndexEntry, decode_entry, parse_header, parse_index};

use crate::capture::ImageRef;

pub struct Pack {
    pub name: String,
    pub path: PathBuf,
    data_offset: u64,
    index: Vec<IndexEntry>,
    /// Read by position only (no shared cursor). Windows still runs one
    /// synchronous handle's reads one at a time, so `stored_many` (the batch
    /// reads, several threads at once) opens its own.
    file: File,
    /// This pack file's build (name, size, write time), the start of its
    /// entries' keys in the artifact cache.
    cache_id: u64,
    /// Entries a bundle read ahead (`PackSet::preload`), by offset and size:
    /// each handed out once.
    preloaded: std::sync::Mutex<std::collections::HashMap<(u32, u32), Vec<u8>>>,
}

/// The artifact cache kind for bundles: many entries' stored bytes in one file.
const BUNDLE_CACHE_KIND: &str = "t6bundle";

/// Bump when what a `t6bundle` holds changes.
const BUNDLE_CACHE_FORMAT: u32 = 1;

/// The artifact cache kind for each bundle label's index: the key of the
/// label's bundle and its entries, in order (`encode_bundle_index`).
const BUNDLE_INDEX_KIND: &str = "t6bundleidx";

/// A map's bundle file (`PackSet::preload_file`): this magic, the format,
/// the count, then each entry's pack build, offset and size, then their
/// stored bytes in that order.
const BUNDLE_FILE_MAGIC: &[u8; 4] = b"T6MB";

/// Bump when what a map's bundle file holds changes.
const BUNDLE_FILE_FORMAT: u32 = 1;

/// The most of one pack `write_bundle_file` holds in memory at once.
const BUNDLE_FILE_CHUNK: u64 = 32 << 20;

fn bundle_file_head_len(n: usize) -> u64 {
    12 + 16 * n as u64
}

fn encode_bundle_file_head(held: &[BundleEntry]) -> Vec<u8> {
    let mut out = Vec::with_capacity(bundle_file_head_len(held.len()) as usize);
    out.extend_from_slice(BUNDLE_FILE_MAGIC);
    out.extend_from_slice(&BUNDLE_FILE_FORMAT.to_le_bytes());
    out.extend_from_slice(&(held.len() as u32).to_le_bytes());
    for &(id, offset, size) in held {
        out.extend_from_slice(&id.to_le_bytes());
        out.extend_from_slice(&offset.to_le_bytes());
        out.extend_from_slice(&size.to_le_bytes());
    }
    out
}

/// The entries a bundle file holds, its reader left at their bytes.
fn read_bundle_file_head(file: &mut impl std::io::Read) -> Option<Vec<BundleEntry>> {
    let mut head = [0u8; 12];
    file.read_exact(&mut head).ok()?;
    if &head[..4] != BUNDLE_FILE_MAGIC || head[4..8] != BUNDLE_FILE_FORMAT.to_le_bytes() {
        return None;
    }
    let n = u32::from_le_bytes([head[8], head[9], head[10], head[11]]) as usize;
    // (A broken count: no map has that many pictures.)
    if n > 1 << 20 {
        return None;
    }
    let mut index = vec![0u8; 16 * n];
    file.read_exact(&mut index).ok()?;
    Some(
        index
            .chunks_exact(16)
            .map(|c| {
                let u32_at = |at: usize| u32::from_le_bytes([c[at], c[at + 1], c[at + 2], c[at + 3]]);
                (u64::from(u32_at(0)) | (u64::from(u32_at(4)) << 32), u32_at(8), u32_at(12))
            })
            .collect(),
    )
}

/// The artifact cache kind for pack entries' stored bytes.
const PACK_CACHE_KIND: &str = "t6pak";

/// Bump when what a `t6pak` entry holds changes.
const PACK_CACHE_FORMAT: u32 = 1;

/// lane B (B1): the packs sit with the install, often on a hard drive, where
/// the first match read ~1,100 pictures one seek each (73 s of a 96 s
/// Nuketown load). Each entry's stored bytes are copied once to the artifact
/// cache (the system drive) and read from there ever after, so only the very
/// first read of a picture touches the pack. `CODLEGACY_T6_PACK_CACHE=0`
/// turns it off.
fn pack_cache_on() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| {
        codlegacy_env::var("CODLEGACY_T6_PACK_CACHE").map_or(true, |v| v.trim() != "0")
    })
}

/// Whether `Pack::warm_many` reads the cached copies it finds (see there).
fn warm_cached_on() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| codlegacy_env::var("CODLEGACY_T6_WARM_CACHED").map_or(true, |v| v.trim() != "0"))
}

fn pack_cache_id(name: &str, len: u64, modified: u64) -> u64 {
    let id = asset_transport::fnv1a64(name.to_ascii_lowercase().as_bytes());
    let id = asset_transport::fnv1a64_more(id, &len.to_le_bytes());
    asset_transport::fnv1a64_more(id, &modified.to_le_bytes())
}

fn pack_cache_key(cache_id: u64, entry: IndexEntry) -> String {
    format!(
        "{PACK_CACHE_FORMAT:02x}-{cache_id:016x}-{:016x}-{:x}-{:x}",
        entry.key, entry.offset, entry.size
    )
}

impl Pack {
    pub fn open(path: &Path) -> Result<Pack, String> {
        let file = File::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
        let head = read_at(&file, 0, 256)?;
        let header = parse_header(&head).map_err(|e| format!("{}: {e}", path.display()))?;
        let bytes = read_at(
            &file,
            header.index.offset as u64,
            header.index.item_count as usize * ipak_t6::INDEX_ENTRY_LEN,
        )?;
        let index = parse_index(&bytes, header.index.item_count as usize)
            .map_err(|e| format!("{}: {e}", path.display()))?;
        let name = path
            .file_stem()
            .map_or_else(String::new, |s| s.to_string_lossy().into_owned());
        let meta = file.metadata().map_err(|e| format!("{}: {e}", path.display()))?;
        let modified = meta
            .modified()
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map_or(0, |d| d.as_secs());
        Ok(Pack {
            cache_id: pack_cache_id(&name, meta.len(), modified),
            name,
            path: path.to_owned(),
            data_offset: header.data.offset as u64,
            index,
            file,
            preloaded: Default::default(),
        })
    }

    pub fn len(&self) -> usize {
        self.index.len()
    }

    pub fn is_empty(&self) -> bool {
        self.index.is_empty()
    }

    pub fn find(&self, name_hash: u32, data_hash: u32) -> Option<IndexEntry> {
        let key = IndexEntry::key_of(name_hash, data_hash);
        self.index
            .binary_search_by_key(&key, |e| e.key)
            .ok()
            .map(|i| self.index[i])
    }

    /// Every entry stored under an image name's hash (any data hash).
    pub fn find_name(&self, name_hash: u32) -> Vec<IndexEntry> {
        let lo = IndexEntry::key_of(name_hash, 0);
        let start = self.index.partition_point(|e| e.key < lo);
        self.index[start..].iter().take_while(|e| e.name_hash() == name_hash).copied().collect()
    }

    /// The decoded entry: an IWI v27 file.
    pub fn read(&self, entry: IndexEntry) -> Result<Vec<u8>, String> {
        let at = self.data_offset + entry.offset as u64;
        let stored = self.stored(entry)?;
        let mut out = Vec::new();
        decode_entry(&stored, at, &mut out).map_err(|e| format!("{}: {e}", self.name))?;
        Ok(out)
    }

    /// Makes a later `read` fast (the boot prewarm): the entry copied to the
    /// artifact cache if it is not there yet (or, with that cache off, its
    /// bytes read and dropped so the system's file cache holds them).
    /// Returns the entry's size.
    pub fn warm(&self, entry: IndexEntry) -> Result<usize, String> {
        if self.preloaded.lock().is_ok_and(|m| m.contains_key(&(entry.offset, entry.size))) {
            return Ok(entry.size as usize);
        }
        if pack_cache_on() {
            let key = pack_cache_key(self.cache_id, entry);
            if asset_transport::cache_open(PACK_CACHE_KIND, &key).is_some() {
                return Ok(entry.size as usize);
            }
        }
        Ok(self.stored(entry)?.len())
    }

    /// `read` for many entries at once (any order; results in the same
    /// order). The ones not in the artifact cache are read from the pack in
    /// disk order, near neighbours together in one read (see `RUN_GAP`), so a
    /// cold map's pictures cost a few long reads instead of a seek each.
    pub fn read_many(&self, entries: &[IndexEntry]) -> Vec<Result<Vec<u8>, String>> {
        self.stored_many(entries, true)
            .into_iter()
            .zip(entries)
            .map(|(stored, entry)| {
                let stored = stored?;
                let mut out = Vec::new();
                decode_entry(&stored, self.data_offset + entry.offset as u64, &mut out)
                    .map_err(|e| format!("{}: {e}", self.name))?;
                Ok(out)
            })
            .collect()
    }

    /// `warm` for many entries at once, read as `read_many` reads them.
    /// Returns the bytes they hold.
    ///
    /// Load speed (the menu's and the lobby's prewarm): an entry already in
    /// the artifact cache was only opened, never read, so the match load
    /// still read the lobby's map pictures cold from the system drive, as a
    /// map nothing warmed does (Nuketown's 999, 179 MB: materials 3.4 s, 27 s
    /// of reads summed on 8 threads, on a busy drive). The cached copies are
    /// now read too, so the load finds them in the system's file cache.
    /// `CODLEGACY_T6_WARM_CACHED=0` only opens them (the old way).
    pub fn warm_many(&self, entries: &[IndexEntry]) -> usize {
        let all = self.warm_many_cold(entries).0;
        if pack_cache_on() && warm_cached_on() {
            for &entry in entries {
                if let Some(mut file) = asset_transport::cache_open(PACK_CACHE_KIND, &pack_cache_key(self.cache_id, entry)) {
                    let _ = std::io::copy(&mut file, &mut std::io::sink());
                }
            }
        }
        all
    }

    /// `warm_many`, with the bytes of the entries the artifact cache did not
    /// hold, read from the pack: (bytes, of them from the pack).
    pub fn warm_many_cold(&self, entries: &[IndexEntry]) -> (usize, usize) {
        let cold: Vec<IndexEntry> = if pack_cache_on() {
            entries
                .iter()
                .copied()
                .filter(|&e| {
                    !self.preloaded.lock().is_ok_and(|m| m.contains_key(&(e.offset, e.size)))
                        && asset_transport::cache_open(PACK_CACHE_KIND, &pack_cache_key(self.cache_id, e)).is_none()
                })
                .collect()
        } else {
            entries.to_vec()
        };
        self.stored_many(&cold, true);
        let sum = |l: &[IndexEntry]| l.iter().map(|e| e.size as usize).sum::<usize>();
        (sum(entries), sum(&cold))
    }

    /// The entries' stored bytes, as `stored` gives them: cache hits first,
    /// then the rest from the pack in runs of near neighbours, each put in the
    /// artifact cache when `cache_each` (a bundle being written holds them
    /// itself).
    fn stored_many(&self, entries: &[IndexEntry], cache_each: bool) -> Vec<Result<Vec<u8>, String>> {
        self.stored_many_within(entries, cache_each, RUN_GAP)
    }

    /// `stored_many` with entries `gap` apart or nearer read together.
    fn stored_many_within(&self, entries: &[IndexEntry], cache_each: bool, gap: u64) -> Vec<Result<Vec<u8>, String>> {
        let mut out: Vec<Option<Result<Vec<u8>, String>>> = entries
            .iter()
            .map(|&entry| {
                let preloaded = self.preloaded.lock().ok().and_then(|mut m| m.remove(&(entry.offset, entry.size)));
                preloaded
                    .or_else(|| {
                        pack_cache_on()
                            .then(|| asset_transport::cache_get(PACK_CACHE_KIND, &pack_cache_key(self.cache_id, entry)))
                            .flatten()
                    })
                    .filter(|bytes| bytes.len() == entry.size as usize)
                    .map(Ok)
            })
            .collect();
        let mut cold: Vec<usize> = (0..entries.len()).filter(|&i| out[i].is_none()).collect();
        cold.sort_by_key(|&i| entries[i].offset);
        // lane B: a batch reads through its own handle, so batches on several
        // threads overlap on the disk instead of queueing on `file` (8
        // threads, 1,359 scattered entries on a hard drive: 122 s through one
        // handle, 10 s through one each).
        let runs = runs_within(&cold, gap, |i| entries[i]);
        // Busy PC: a bundle being written (`cache_each` false) keeps a few
        // reads in flight, each through its own handle, handed out in disk
        // order: one read at a time waits its turn behind every other
        // program's reads on a shared hard drive (the front end's pictures
        // took 62.0 s cold), and with several queued the drive sorts them
        // along its sweep. `CODLEGACY_HUD_BUNDLE_READERS=1` reads one at a time.
        let readers = if cache_each { 1 } else { bundle_readers().min(runs.len()) };
        if readers > 1 {
            let next = std::sync::atomic::AtomicUsize::new(0);
            let done: Vec<Vec<RunRead>> = std::thread::scope(|s| {
                let workers: Vec<_> = (0..readers)
                    .map(|_| {
                        s.spawn(|| {
                            let own = File::open(&self.path).ok();
                            let file = own.as_ref().unwrap_or(&self.file);
                            let mut got = Vec::new();
                            loop {
                                let n = next.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                                let Some(run) = runs.get(n) else { break };
                                got.extend(self.read_run(file, entries, run, cache_each));
                            }
                            got
                        })
                    })
                    .collect();
                // (A reader that panicked leaves its entries "not read": no bundle.)
                workers.into_iter().map(|w| w.join().unwrap_or(Vec::new())).collect()
            });
            for (i, r) in done.into_iter().flatten() {
                out[i] = Some(r);
            }
        } else {
            let own = (!cold.is_empty()).then(|| File::open(&self.path).ok()).flatten();
            let file = own.as_ref().unwrap_or(&self.file);
            for run in &runs {
                for (i, r) in self.read_run(file, entries, run, cache_each) {
                    out[i] = Some(r);
                }
            }
        }
        out.into_iter().map(|r| r.unwrap_or_else(|| Err("not read".to_owned()))).collect()
    }

    /// One run of `stored_many_within` (indices into `entries`, in offset
    /// order) read in one request: each entry's stored bytes.
    fn read_run(&self, file: &File, entries: &[IndexEntry], run: &[usize], cache_each: bool) -> Vec<RunRead> {
        let start = entries[run[0]].offset as u64;
        let end = run.iter().map(|&i| entries[i].offset as u64 + entries[i].size as u64).max().unwrap_or(start);
        match read_at(file, self.data_offset + start, (end - start) as usize) {
            Ok(bytes) => run
                .iter()
                .map(|&i| {
                    let entry = entries[i];
                    let from = (entry.offset as u64 - start) as usize;
                    let stored = bytes.get(from..from + entry.size as usize);
                    let r = match stored {
                        Some(stored) => {
                            if cache_each && pack_cache_on() {
                                let key = pack_cache_key(self.cache_id, entry);
                                let _ = asset_transport::cache_put(PACK_CACHE_KIND, &key, stored);
                            }
                            Ok(stored.to_vec())
                        }
                        // (Past the pack's end: read alone, as `read` would.)
                        None => self.stored(entry),
                    };
                    (i, r)
                })
                .collect(),
            Err(e) => run.iter().map(|&i| (i, Err(e.clone()))).collect(),
        }
    }

    /// The entry's stored bytes: from the artifact cache, else from the pack
    /// (and then put in the cache).
    fn stored(&self, entry: IndexEntry) -> Result<Vec<u8>, String> {
        let at = self.data_offset + entry.offset as u64;
        if let Some(bytes) = self.preloaded.lock().ok().and_then(|mut m| m.remove(&(entry.offset, entry.size))) {
            return Ok(bytes);
        }
        if !pack_cache_on() {
            return read_at(&self.file, at, entry.size as usize);
        }
        let key = pack_cache_key(self.cache_id, entry);
        let cached = |key: &str| {
            asset_transport::cache_get(PACK_CACHE_KIND, key)
                .filter(|bytes| bytes.len() == entry.size as usize)
        };
        if let Some(bytes) = cached(&key) {
            return Ok(bytes);
        }
        let _flight = asset_transport::cache_flight(PACK_CACHE_KIND, &key);
        if let Some(bytes) = cached(&key) {
            return Ok(bytes);
        }
        let stored = read_at(&self.file, at, entry.size as usize)?;
        // (A short read is the pack's end, not a whole entry: never kept.)
        if stored.len() == entry.size as usize {
            let _ = asset_transport::cache_put(PACK_CACHE_KIND, &key, &stored);
        }
        Ok(stored)
    }
}

/// One entry `Pack::read_run` read: its index and stored bytes.
type RunRead = (usize, Result<Vec<u8>, String>);

/// An entry in a bundle: its pack's build, offset and stored size.
type BundleEntry = (u64, u32, u32);

/// Load speed: a match's HUD-icon bundle, written the first time on each
/// map, holds ~1,700 of the front end's pictures. They are taken from the
/// front end's bundle (read at boot, one file in the artifact cache) and only
/// the rest read from the packs (the first map read 187.6 MB of packs for
/// 18.3 MB in 19.7 s cold). `CODLEGACY_HUD_BUNDLE_REUSE=0` reads them all
/// from the packs.
fn bundle_reuse_on() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| codlegacy_env::var("CODLEGACY_HUD_BUNDLE_REUSE").map_or(true, |v| v.trim() != "0"))
}

/// The bundles read or written this run (the last few), by cache key.
static BUNDLES_SEEN: std::sync::Mutex<Vec<(String, Vec<BundleEntry>)>> = std::sync::Mutex::new(Vec::new());

/// How many entries the last bundle written took from earlier bundles.
static LAST_REUSED: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

fn remember_bundle(key: &str, held: Vec<BundleEntry>) {
    if !bundle_reuse_on() {
        return;
    }
    if let Ok(mut seen) = BUNDLES_SEEN.lock() {
        seen.retain(|(k, _)| k != key);
        seen.push((key.to_string(), held));
        if seen.len() > 4 {
            seen.remove(0);
        }
    }
}

/// `ids`' stored bytes not yet in `out` from the bundles seen this run,
/// where one holds them.
fn reused_from_bundles(ids: &[BundleEntry], out: &mut [Option<Vec<u8>>]) {
    if !bundle_reuse_on() {
        return;
    }
    let seen = BUNDLES_SEEN.lock().map(|s| s.clone()).unwrap_or_default();
    for (key, held) in seen.iter().rev() {
        let set: std::collections::HashSet<&BundleEntry> = held.iter().collect();
        if !ids.iter().zip(out.iter()).any(|(id, got)| got.is_none() && set.contains(id)) {
            continue;
        }
        if let Some(bundle) = asset_transport::cache_get(BUNDLE_CACHE_KIND, key) {
            take_from_bundle(held, &bundle, ids, out);
        }
    }
}

/// Busy PC: a bundle's key changes with any change in its entry list (a
/// build that adds or renames one picture, a patched pack), and the new
/// bundle then read all ~1,700 pictures from the packs again (62.0 s on a
/// busy hard drive, the menu waiting) while the old bundle, one file holding
/// nearly all of them, was deleted after. Each label's index (its bundle's
/// key and entries) is kept beside it, so a new bundle takes what the
/// previous one of the same label holds (one sequential read) and reads
/// only the new entries from the packs. `CODLEGACY_HUD_BUNDLE_KEEP_OLD=0`
/// reads them all from the packs.
fn bundle_keep_old_on() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| codlegacy_env::var("CODLEGACY_HUD_BUNDLE_KEEP_OLD").map_or(true, |v| v.trim() != "0"))
}

const BUNDLE_INDEX_MAGIC: &[u8; 4] = b"TBI1";

/// A label's index: magic, the bundle's key (length first), the entry
/// count, then each entry (pack build, offset, size), little-endian.
fn encode_bundle_index(key: &str, held: &[BundleEntry]) -> Vec<u8> {
    let mut out = Vec::with_capacity(12 + key.len() + held.len() * 16);
    out.extend_from_slice(BUNDLE_INDEX_MAGIC);
    out.extend_from_slice(&(key.len() as u32).to_le_bytes());
    out.extend_from_slice(key.as_bytes());
    out.extend_from_slice(&(held.len() as u32).to_le_bytes());
    for (pack, offset, size) in held {
        out.extend_from_slice(&pack.to_le_bytes());
        out.extend_from_slice(&offset.to_le_bytes());
        out.extend_from_slice(&size.to_le_bytes());
    }
    out
}

/// `encode_bundle_index` read back; `None` for anything else.
fn decode_bundle_index(bytes: &[u8]) -> Option<(String, Vec<BundleEntry>)> {
    fn u32_at(b: &[u8], at: usize) -> Option<u32> {
        b.get(at..at + 4).map(|x| u32::from_le_bytes([x[0], x[1], x[2], x[3]]))
    }
    let rest = bytes.strip_prefix(BUNDLE_INDEX_MAGIC.as_slice())?;
    let key_len = u32_at(rest, 0)? as usize;
    let key = std::str::from_utf8(rest.get(4..4 + key_len)?).ok()?.to_string();
    let rest = &rest[4 + key_len..];
    let count = u32_at(rest, 0)? as usize;
    let body = rest.get(4..)?;
    if body.len() != count.checked_mul(16)? {
        return None;
    }
    let held = body
        .chunks_exact(16)
        .map(|c| {
            let mut pack = [0u8; 8];
            pack.copy_from_slice(&c[0..8]);
            (u64::from_le_bytes(pack), u32_at(c, 8).unwrap_or(0), u32_at(c, 12).unwrap_or(0))
        })
        .collect();
    Some((key, held))
}

/// Writes `label`'s index unless it already names `key` and `held`.
fn keep_bundle_index(label: &str, key: &str, held: &[BundleEntry]) {
    if !bundle_keep_old_on() {
        return;
    }
    let same = asset_transport::cache_get(BUNDLE_INDEX_KIND, label)
        .and_then(|b| decode_bundle_index(&b))
        .is_some_and(|(k, h)| k == key && h.as_slice() == held);
    if !same {
        let _ = asset_transport::cache_put(BUNDLE_INDEX_KIND, label, &encode_bundle_index(key, held));
    }
}

/// `ids`' stored bytes not yet in `out` from `label`'s previous bundle on
/// disk (the one its index names, when not `key` itself).
fn reused_from_old_bundle(label: &str, key: &str, ids: &[BundleEntry], out: &mut [Option<Vec<u8>>]) {
    if !bundle_keep_old_on() {
        return;
    }
    let Some((old_key, held)) = asset_transport::cache_get(BUNDLE_INDEX_KIND, label).and_then(|b| decode_bundle_index(&b)) else {
        return;
    };
    if old_key == key {
        return;
    }
    if let Some(bundle) = asset_transport::cache_get(BUNDLE_CACHE_KIND, &old_key) {
        take_from_bundle(&held, &bundle, ids, out);
    }
}

/// The entries of `ids` not yet in `out` that `held` (a bundle's entries in
/// order, `bundle` their bytes end to end) holds; nothing when the bundle's
/// size does not match its entries.
fn take_from_bundle(held: &[BundleEntry], bundle: &[u8], ids: &[BundleEntry], out: &mut [Option<Vec<u8>>]) {
    let mut place = std::collections::HashMap::with_capacity(held.len());
    let mut at = 0usize;
    for id in held {
        place.entry(*id).or_insert(at);
        at += id.2 as usize;
    }
    if at != bundle.len() {
        return;
    }
    for (id, got) in ids.iter().zip(out.iter_mut()) {
        if got.is_none() {
            if let Some(&start) = place.get(id) {
                *got = Some(bundle[start..start + id.2 as usize].to_vec());
            }
        }
    }
}

/// How many entries the last `PackSet::preload` that wrote a bundle took
/// from an earlier bundle of this run instead of the packs.
pub fn bundle_entries_reused() -> usize {
    LAST_REUSED.load(std::sync::atomic::Ordering::Relaxed)
}

/// lane B (B3): entries this close on disk are read together, the gap
/// read and dropped: on a hard disk a seek costs about as long as reading
/// a megabyte.
const RUN_GAP: u64 = 1 << 20;

/// The longest one read of a run.
pub const RUN_MAX: u64 = 16 << 20;

/// Load speed: a bundle being written reads its entries with this gap
/// instead of `RUN_GAP`: the front end's 18.4 MB of pictures were read as
/// 178.9 MB in 62.0 s on a cold boot, the drive shared with the boot's other
/// readers. Only near-adjoining entries are joined; the rest are read on
/// their own, in pack order, in one pass. `CODLEGACY_HUD_BUNDLE_EXACT_READ=0`
/// reads them with `RUN_GAP`.
const BUNDLE_RUN_GAP: u64 = 64 << 10;

/// Whether a bundle being written reads with `BUNDLE_RUN_GAP`.
pub fn bundle_exact_read_on() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| codlegacy_env::var("CODLEGACY_HUD_BUNDLE_EXACT_READ").map_or(true, |v| v.trim() != "0"))
}

/// How many reads a bundle being written keeps in flight (see
/// `stored_many_within`): 4, or `CODLEGACY_HUD_BUNDLE_READERS` (1 to 8; 1 =
/// one at a time, the old way).
fn bundle_readers() -> usize {
    static N: std::sync::OnceLock<usize> = std::sync::OnceLock::new();
    *N.get_or_init(|| {
        codlegacy_env::var("CODLEGACY_HUD_BUNDLE_READERS")
            .ok()
            .and_then(|v| v.trim().parse::<usize>().ok())
            .map_or(4, |n| n.clamp(1, 8))
    })
}

/// How many image packs `PackSet::open_dir` opens at once: 4, or
/// `CODLEGACY_T6_PACK_OPENERS` (1 to 8; 1 = one after another, the old way).
fn pack_openers() -> usize {
    static N: std::sync::OnceLock<usize> = std::sync::OnceLock::new();
    *N.get_or_init(|| {
        codlegacy_env::var("CODLEGACY_T6_PACK_OPENERS")
            .ok()
            .and_then(|v| v.trim().parse::<usize>().ok())
            .map_or(4, |n| n.clamp(1, 8))
    })
}

/// `f` over `items` on up to `threads` threads at once, each thread taking
/// the next item in `order` (indices into `items`), each result at its
/// item's place; `None` for an item not in `order` or whose thread stopped.
/// One thread (or one item) runs them here, in `order`.
pub fn on_threads<T: Sync, R: Send>(items: &[T], order: &[usize], threads: usize, f: impl Fn(&T) -> R + Sync) -> Vec<Option<R>> {
    let mut out: Vec<Option<R>> = (0..items.len()).map(|_| None).collect();
    let threads = threads.min(order.len());
    if threads <= 1 {
        for &i in order {
            out[i] = Some(f(&items[i]));
        }
        return out;
    }
    let next = std::sync::atomic::AtomicUsize::new(0);
    let f = &f;
    let done: Vec<Vec<(usize, R)>> = std::thread::scope(|s| {
        let workers: Vec<_> = (0..threads)
            .map(|_| {
                s.spawn(|| {
                    let mut got = Vec::new();
                    while let Some(&i) = order.get(next.fetch_add(1, std::sync::atomic::Ordering::Relaxed)) {
                        got.push((i, f(&items[i])));
                    }
                    got
                })
            })
            .collect();
        workers.into_iter().map(|w| w.join().unwrap_or(Vec::new())).collect()
    });
    for (i, r) in done.into_iter().flatten() {
        out[i] = Some(r);
    }
    out
}

fn bundle_run_gap() -> u64 {
    if bundle_exact_read_on() { BUNDLE_RUN_GAP } else { RUN_GAP }
}

/// `items` (in offset order) split into runs: each next item starts within
/// `RUN_GAP` of the run's end and the run stays within `RUN_MAX`.
#[cfg(test)]
fn runs<T: Copy>(items: &[T], entry: impl Fn(T) -> IndexEntry) -> Vec<&[T]> {
    runs_within(items, RUN_GAP, entry)
}

/// `runs` with its own gap.
fn runs_within<T: Copy>(items: &[T], gap: u64, entry: impl Fn(T) -> IndexEntry) -> Vec<&[T]> {
    let mut out = Vec::new();
    let mut first = 0;
    let (mut start, mut end) = (0u64, 0u64);
    for (n, &item) in items.iter().enumerate() {
        let e = entry(item);
        let (at, to) = (e.offset as u64, e.offset as u64 + e.size as u64);
        if n > first && (at > end + gap || to.max(end) - start > RUN_MAX) {
            out.push(&items[first..n]);
            first = n;
        }
        if n == first {
            (start, end) = (at, to);
        } else {
            end = end.max(to);
        }
    }
    if first < items.len() {
        out.push(&items[first..]);
    }
    out
}

fn read_at(file: &File, at: u64, len: usize) -> Result<Vec<u8>, String> {
    #[cfg(windows)]
    use std::os::windows::fs::FileExt;
    #[cfg(unix)]
    use std::os::unix::fs::FileExt;
    let mut buf = vec![0u8; len];
    let mut got = 0;
    while got < len {
        #[cfg(windows)]
        let read = file.seek_read(&mut buf[got..], at + got as u64);
        #[cfg(unix)]
        let read = file.read_at(&mut buf[got..], at + got as u64);
        match read {
            Ok(0) => break,
            Ok(n) => got += n,
            Err(e) => return Err(e.to_string()),
        }
    }
    buf.truncate(got);
    Ok(buf)
}

/// Every pack of an install's zone folder, searched in priority order:
/// patches and map packs before the shared ones.
pub struct PackSet {
    pub packs: Vec<Pack>,
}

/// Where an image's pixels are.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ImageSource {
    /// In a pack: which pack (index into `PackSet::packs`) and entry.
    Pack(usize, IndexEntry),
    /// In the zone itself.
    Embedded,
    /// Not streamed and not embedded: nothing to draw.
    Empty,
}

impl PackSet {
    pub fn open_dir(dir: &Path) -> Result<PackSet, String> {
        let mut paths: Vec<PathBuf> = std::fs::read_dir(dir)
            .map_err(|e| format!("{}: {e}", dir.display()))?
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| {
                p.extension()
                    .is_some_and(|x| x.eq_ignore_ascii_case("ipak"))
            })
            .collect();
        paths.sort_by_key(|p| priority(p));
        // bo2zm: then the extra folders' packs (`CODLEGACY_T6_EXTRA`), after the install's.
        let mut extra = asset_transport::t6_extra::extra_files("ipak");
        extra.sort_by_key(|p| priority(p));
        paths.extend(extra);
        // Busy PC: each pack's header and index (two reads, a seek each)
        // were read one pack after another, 24 packs in his zone folder, each
        // read waiting its turn behind every other program's on a shared hard
        // drive. A few packs are opened at once (`pack_openers`).
        let opened = on_threads(&paths, &(0..paths.len()).collect::<Vec<_>>(), pack_openers(), |p| Pack::open(p));
        let packs = opened
            .into_iter()
            .zip(&paths)
            .map(|(r, p)| r.unwrap_or_else(|| Err(format!("{}: not opened (its thread stopped)", p.display()))))
            .collect::<Result<Vec<_>, _>>()?;
        Ok(PackSet { packs })
    }

    /// bo2mp: many small entries (the front end's 2,400 pictures) read as one
    /// file. Each entry has its own file in the artifact cache, and every
    /// boot opened 2,400 of them one after another (4,278 reads). The first
    /// time, the entries are written in the order given (the caller's disk
    /// order) to one bundle named `label` and keyed by the packs' builds and
    /// the entries; after that the one file is read and each entry handed to
    /// `read` from memory, once (`drop_preloaded` frees the rest). An older
    /// bundle of the same label is removed. Returns how many entries came
    /// from a bundle (0 when it was written now or the cache is off).
    pub fn preload(&self, label: &str, entries: &[(usize, IndexEntry)]) -> usize {
        if !pack_cache_on() {
            return 0;
        }
        let mut id = asset_transport::fnv1a64(label.as_bytes());
        for (i, e) in entries {
            let Some(p) = self.packs.get(*i) else { return 0 };
            id = asset_transport::fnv1a64_more(id, &p.cache_id.to_le_bytes());
            id = asset_transport::fnv1a64_more(id, &e.offset.to_le_bytes());
            id = asset_transport::fnv1a64_more(id, &e.size.to_le_bytes());
        }
        let prefix = format!("{label}-");
        let key = format!("{prefix}{BUNDLE_CACHE_FORMAT:02x}-{id:016x}-{:x}", entries.len());
        let total: usize = entries.iter().map(|(_, e)| e.size as usize).sum();
        let hand_out = |i: usize, e: IndexEntry, bytes: Vec<u8>| {
            if let Ok(mut m) = self.packs[i].preloaded.lock() {
                m.insert((e.offset, e.size), bytes);
            }
        };
        let ids: Vec<BundleEntry> = entries.iter().map(|&(i, e)| (self.packs[i].cache_id, e.offset, e.size)).collect();
        // Memory: each entry read from the file into its own buffer (a map's
        // bundle is ~180 MB; read whole and then split, it was held twice).
        if let Some(mut file) = asset_transport::cache_open(BUNDLE_CACHE_KIND, &key)
            .filter(|f| f.metadata().is_ok_and(|m| m.len() == total as u64))
        {
            let mut read = 0;
            for &(i, e) in entries {
                let mut bytes = vec![0; e.size as usize];
                if std::io::Read::read_exact(&mut file, &mut bytes).is_err() {
                    // (The rest are read as they would be without a bundle.)
                    return read;
                }
                hand_out(i, e, bytes);
                read += 1;
            }
            keep_bundle_index(label, &key, &ids);
            remember_bundle(&key, ids);
            return entries.len();
        }
        // The first time: each pack's entries read in runs of near neighbours
        // and not each copied to the cache as well (the bundle is their
        // copy). One read and one cache file per entry made a cold front
        // end's pictures 34 s on his hard drive.
        // Entries the previous bundle of this label (on disk) or an earlier
        // bundle of this run holds are taken from it.
        let mut parts = vec![None; ids.len()];
        reused_from_old_bundle(label, &key, &ids, &mut parts);
        reused_from_bundles(&ids, &mut parts);
        LAST_REUSED.store(parts.iter().filter(|p| p.is_some()).count(), std::sync::atomic::Ordering::Relaxed);
        let mut at = 0;
        while at < entries.len() {
            let i = entries[at].0;
            let n = entries[at..].iter().take_while(|(j, _)| *j == i).count();
            let missing: Vec<usize> = (at..at + n).filter(|&k| parts[k].is_none()).collect();
            let run: Vec<IndexEntry> = missing.iter().map(|&k| entries[k].1).collect();
            if !run.is_empty() {
                let read = self.packs[i].stored_many_within(&run, false, bundle_run_gap());
                for (bytes, (&k, e)) in read.into_iter().zip(missing.iter().zip(&run)) {
                    match bytes {
                        Ok(bytes) if bytes.len() == e.size as usize => parts[k] = Some(bytes),
                        // (A short or failed entry: no bundle; the reads go on one by one.)
                        _ => return 0,
                    }
                }
            }
            at += n;
        }
        let Some(parts) = parts.into_iter().collect::<Option<Vec<Vec<u8>>>>() else { return 0 };
        let written = asset_transport::cache_put_with(BUNDLE_CACHE_KIND, &key, |file| {
            parts.iter().try_for_each(|bytes| std::io::Write::write_all(file, bytes))
        });
        if written.is_ok() {
            keep_bundle_index(label, &key, &ids);
            asset_transport::cache_prune(BUNDLE_CACHE_KIND, &prefix, &key);
            remember_bundle(&key, ids);
        }
        for (&(i, e), bytes) in entries.iter().zip(parts) {
            hand_out(i, e, bytes);
        }
        0
    }

    /// bo2mp: `preload` for a map's pictures, with the bundle in a file of
    /// its own at `path` (the map bake's folder on the data drive, which
    /// CLEAR CACHE empties, not the artifact cache on the system drive). The
    /// file can hold more than `entries` (BAKE ALL writes every picture the
    /// map's materials name, `write_bundle_file`): each of `entries` it holds
    /// is read from it start to end and handed to `read` from memory, once;
    /// the rest are skipped. Returns how many came from it (0: no file, or
    /// one made from other packs or another format).
    pub fn preload_file(&self, path: &Path, entries: &[(usize, IndexEntry)]) -> usize {
        let wanted: std::collections::HashMap<BundleEntry, usize> = entries
            .iter()
            .filter_map(|&(i, e)| Some(((self.packs.get(i)?.cache_id, e.offset, e.size), i)))
            .collect();
        let Ok(file) = File::open(path) else { return 0 };
        let mut file = std::io::BufReader::with_capacity(1 << 20, file);
        let Some(held) = read_bundle_file_head(&mut file) else { return 0 };
        let mut got = 0;
        for id in held {
            let size = id.2 as usize;
            match wanted.get(&id) {
                Some(&i) => {
                    let mut bytes = vec![0; size];
                    // (Cut short: the rest are read as without a bundle.)
                    if std::io::Read::read_exact(&mut file, &mut bytes).is_err() {
                        return got;
                    }
                    if let Ok(mut m) = self.packs[i].preloaded.lock()
                        && m.insert((id.1, id.2), bytes).is_none()
                    {
                        got += 1;
                    }
                }
                None => {
                    if file.seek_relative(size as i64).is_err() {
                        return got;
                    }
                }
            }
        }
        got
    }

    /// Writes the bundle file `preload_file` reads at `path`: `entries`'
    /// stored bytes (once each, in disk order) from the artifact cache or the
    /// packs, up to `BUNDLE_FILE_CHUNK` of a pack in memory at a time,
    /// written whole under `<name>.part<pid>` and then renamed over the old
    /// one. The ones in `hand_out` are also handed to `read` as
    /// `preload_file` does. `Ok`: the bytes written.
    pub fn write_bundle_file(
        &self,
        path: &Path,
        entries: &[(usize, IndexEntry)],
        hand_out: &[(usize, IndexEntry)],
    ) -> Result<u64, String> {
        let mut entries: Vec<(usize, IndexEntry)> = entries.iter().copied().filter(|(i, _)| *i < self.packs.len()).collect();
        entries.sort_unstable_by_key(|&(i, e)| (i, e.offset, e.size));
        entries.dedup_by_key(|&mut (i, e)| (i, e.offset, e.size));
        let give: std::collections::HashSet<(usize, u32, u32)> = hand_out.iter().map(|&(i, e)| (i, e.offset, e.size)).collect();
        let Some(dir) = path.parent() else { return Err(format!("{}: no folder", path.display())) };
        std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        let name = path.file_name().map_or_else(String::new, |n| n.to_string_lossy().into_owned());
        let part = path.with_file_name(format!("{name}.part{}", std::process::id()));
        let io = |e: std::io::Error| format!("{}: {e}", part.display());
        let written = (|| -> Result<u64, String> {
            let file = File::create(&part).map_err(io)?;
            let mut out = std::io::BufWriter::with_capacity(1 << 20, file);
            let held: Vec<BundleEntry> = entries.iter().map(|&(i, e)| (self.packs[i].cache_id, e.offset, e.size)).collect();
            std::io::Write::write_all(&mut out, &encode_bundle_file_head(&held)).map_err(io)?;
            let mut bytes_out = 0u64;
            let mut at = 0;
            while at < entries.len() {
                let i = entries[at].0;
                let mut n = 0;
                let mut chunk = 0u64;
                while at + n < entries.len()
                    && entries[at + n].0 == i
                    && (n == 0 || chunk + u64::from(entries[at + n].1.size) <= BUNDLE_FILE_CHUNK)
                {
                    chunk += u64::from(entries[at + n].1.size);
                    n += 1;
                }
                let run: Vec<IndexEntry> = entries[at..at + n].iter().map(|&(_, e)| e).collect();
                for (bytes, e) in self.packs[i].stored_many_within(&run, false, bundle_run_gap()).into_iter().zip(&run) {
                    let bytes = bytes?;
                    if bytes.len() != e.size as usize {
                        return Err(format!("{}: entry at {:x} read short", self.packs[i].name, e.offset));
                    }
                    std::io::Write::write_all(&mut out, &bytes).map_err(io)?;
                    bytes_out += bytes.len() as u64;
                    if give.contains(&(i, e.offset, e.size))
                        && let Ok(mut m) = self.packs[i].preloaded.lock()
                    {
                        m.insert((e.offset, e.size), bytes);
                    }
                }
                at += n;
            }
            let file = out.into_inner().map_err(|e| io(e.into_error()))?;
            file.sync_all().map_err(io)?;
            Ok(bytes_out)
        })();
        match written {
            Ok(n) => match std::fs::rename(&part, path) {
                Ok(()) => Ok(n),
                Err(e) => {
                    let _ = std::fs::remove_file(&part);
                    Err(format!("{}: {e}", path.display()))
                }
            },
            Err(e) => {
                let _ = std::fs::remove_file(&part);
                Err(e)
            }
        }
    }

    /// Whether the bundle file at `path` is whole and holds every one of
    /// `entries` (made from these packs' builds, in this format).
    pub fn bundle_file_holds(&self, path: &Path, entries: &[(usize, IndexEntry)]) -> bool {
        let Ok(file) = File::open(path) else { return false };
        let Some(held) = read_bundle_file_head(&mut std::io::BufReader::new(file)) else { return false };
        let total: u64 = held.iter().map(|h| u64::from(h.2)).sum();
        let whole = std::fs::metadata(path).is_ok_and(|m| m.len() == bundle_file_head_len(held.len()) + total);
        let held: std::collections::HashSet<BundleEntry> = held.into_iter().collect();
        whole && entries.iter().all(|&(i, e)| self.packs.get(i).is_some_and(|p| held.contains(&(p.cache_id, e.offset, e.size))))
    }

    /// How `preload` reads `entries` when it writes their bundle: its reads
    /// and their bytes, the gaps between near neighbours included (load
    /// speed: a cold front end's pictures went from 34 s to 50 s), with
    /// `RUN_GAP` (`exact` false) or `BUNDLE_RUN_GAP`.
    pub fn bundle_reads(&self, entries: &[(usize, IndexEntry)], exact: bool) -> (usize, u64) {
        let gap = if exact { BUNDLE_RUN_GAP } else { RUN_GAP };
        let (mut reads, mut bytes) = (0usize, 0u64);
        let mut at = 0;
        while at < entries.len() {
            let i = entries[at].0;
            let n = entries[at..].iter().take_while(|(j, _)| *j == i).count();
            let mut run: Vec<IndexEntry> = entries[at..at + n].iter().map(|&(_, e)| e).collect();
            run.sort_by_key(|e| e.offset);
            for r in runs_within(&run, gap, |e| e) {
                let start = r[0].offset as u64;
                let end = r.iter().map(|e| e.offset as u64 + e.size as u64).max().unwrap_or(start);
                reads += 1;
                bytes += end - start;
            }
            at += n;
        }
        (reads, bytes)
    }

    /// Frees the entries a `preload` read that nobody asked for.
    pub fn drop_preloaded(&self) {
        for p in &self.packs {
            if let Ok(mut m) = p.preloaded.lock() {
                *m = Default::default();
            }
        }
    }

    /// An image by its name alone (a zone's `,name` reference to an image
    /// another zone defines): the first pack holding that name, in
    /// priority order, and its largest entry (the full mip chain).
    pub fn locate_name(&self, name: &str) -> Option<(usize, IndexEntry)> {
        self.locate_name_hash(ipak_t6::hash_name(name.trim_start_matches(',')))
    }

    pub fn locate(&self, image: &ImageRef) -> Option<ImageSource> {
        if image.embedded.is_some() {
            return Some(ImageSource::Embedded);
        }
        if image.streamed_parts == 0 {
            return Some(ImageSource::Empty);
        }
        self.packs
            .iter()
            .enumerate()
            .find_map(|(i, p)| {
                p.find(image.name_hash, image.part_hash)
                    .map(|e| ImageSource::Pack(i, e))
            })
            // A zone built against another build of the same picture (the
            // Zombies Declassified "DLC5" maps: about 60% of their streamed
            // images are in the install's packs under their name with
            // other data hashes): the picture by its name hash alone.
            .or_else(|| self.locate_name_hash(image.name_hash).map(|(i, e)| ImageSource::Pack(i, e)))
    }

    /// The first pack, in priority order, with an entry under `name_hash`,
    /// and its largest such entry (the full mip chain).
    fn locate_name_hash(&self, name_hash: u32) -> Option<(usize, IndexEntry)> {
        self.packs
            .iter()
            .enumerate()
            .find_map(|(i, p)| p.find_name(name_hash).into_iter().max_by_key(|e| e.size).map(|e| (i, e)))
    }
}

/// Lower sorts first: zone patches, then map packs, then shared packs.
fn priority(path: &Path) -> (u8, String) {
    let stem = path
        .file_stem()
        .map_or_else(String::new, |s| s.to_string_lossy().to_ascii_lowercase());
    let rank = if stem.ends_with("_patch") {
        0
    } else if stem.starts_with("zm_") || stem.starts_with("mp_") {
        1
    } else if stem.starts_with("patch") {
        2
    } else if stem.starts_with("dlc") {
        3
    } else if stem == "base" {
        5
    } else {
        4
    };
    (rank, stem)
}

#[cfg(test)]
mod tests {
    use super::{
        BUNDLE_RUN_GAP, IndexEntry, Pack, on_threads, RUN_GAP, RUN_MAX, decode_bundle_index, encode_bundle_index, pack_cache_id, pack_cache_key, read_at,
        runs, runs_within, take_from_bundle, PackSet,
    };

    /// A map's bundle file: written from a pack, it gives back the entries
    /// asked (any subset), skips the rest, and says what it holds.
    #[test]
    fn bundle_file_round_trips() {
        let dir = std::env::temp_dir().join(format!("t6_bundle_file_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("pack.bin");
        let data: Vec<u8> = (0..300_000u32).map(|i| (i.wrapping_mul(31) ^ (i / 89)) as u8).collect();
        std::fs::write(&path, &data).unwrap();
        let pack = |cache_id: u64| Pack {
            name: "test".into(),
            path: path.clone(),
            data_offset: 16,
            index: Vec::new(),
            file: std::fs::File::open(&path).unwrap(),
            cache_id,
            preloaded: Default::default(),
        };
        let id = 0x7e57_b0d1_7e57_b0d1;
        let set = PackSet { packs: vec![pack(id)] };
        let all: Vec<(usize, IndexEntry)> = (0..30u32)
            .map(|i| (0, IndexEntry { key: u64::from(i), offset: (i * 9001) % 290_000, size: 200 + i * 5 }))
            .collect();
        let bundle = dir.join("t6").join("mp_test").join("t6pictures.pak");
        assert!(!set.bundle_file_holds(&bundle, &all));
        let n = set.write_bundle_file(&bundle, &all, &[]).unwrap();
        assert_eq!(n, all.iter().map(|(_, e)| u64::from(e.size)).sum::<u64>());
        assert!(set.bundle_file_holds(&bundle, &all));
        assert!(set.packs[0].preloaded.lock().unwrap().is_empty(), "nothing handed out");
        // A fresh load wants every third one.
        let load = PackSet { packs: vec![pack(id)] };
        let wanted: Vec<(usize, IndexEntry)> = all.iter().copied().step_by(3).collect();
        assert_eq!(load.preload_file(&bundle, &wanted), wanted.len());
        {
            let m = load.packs[0].preloaded.lock().unwrap();
            assert_eq!(m.len(), wanted.len());
            for (_, e) in &wanted {
                let at = 16 + e.offset as usize;
                assert_eq!(m[&(e.offset, e.size)], data[at..at + e.size as usize]);
            }
        }
        // Another pack build: not this file's.
        let other = PackSet { packs: vec![pack(1)] };
        assert_eq!(other.preload_file(&bundle, &wanted), 0);
        assert!(!other.bundle_file_holds(&bundle, &wanted));
        assert_eq!(std::fs::read_dir(bundle.parent().unwrap()).unwrap().count(), 1, "no part file left");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Work handed to several threads lands at each item's place, in any
    /// order asked; items not asked stay empty.
    #[test]
    fn on_threads_keeps_each_result_in_place() {
        let items: Vec<u32> = (0..50).collect();
        let order: Vec<usize> = (0..50).rev().filter(|i| i % 7 != 3).collect();
        for threads in [1, 3, 8] {
            let got = on_threads(&items, &order, threads, |x| x * 10);
            for (i, r) in got.iter().enumerate() {
                assert_eq!(*r, (i % 7 != 3).then_some(i as u32 * 10), "threads {threads} item {i}");
            }
        }
        assert!(on_threads(&items[..0], &[], 4, |x| *x).is_empty());
    }

    /// A bundle's reads on several threads give each entry its own bytes, in
    /// the order asked.
    #[test]
    fn bundle_reads_in_flight_match_one_at_a_time() {
        let path = std::env::temp_dir().join(format!("t6_bundle_readers_{}.bin", std::process::id()));
        let data: Vec<u8> = (0..400_000u32).map(|i| (i * 13 + i / 97) as u8).collect();
        std::fs::write(&path, &data).unwrap();
        let pack = Pack {
            name: "test".into(),
            path: path.clone(),
            data_offset: 16,
            index: Vec::new(),
            file: std::fs::File::open(&path).unwrap(),
            // (A build no real pack has: no cached entry is found.)
            cache_id: 0x7e57_7e57_7e57_7e57,
            preloaded: Default::default(),
        };
        // Scattered, out of order, two overlapping (none past the end: that read
        // goes through `stored`, which caches).
        let mut entries: Vec<IndexEntry> =
            (0..40u32).map(|i| IndexEntry { key: i as u64, offset: (i * 7919) % 390_000, size: 100 + i * 3 }).collect();
        entries.push(IndexEntry { key: 99, offset: 1000 + 100, size: 50 });
        entries.reverse();
        // (`cache_each` false: nothing is written to the artifact cache.)
        let many = pack.stored_many_within(&entries, false, 0);
        for (e, got) in entries.iter().zip(&many) {
            let a = got.as_ref().unwrap();
            let at = 16 + e.offset as usize;
            assert_eq!(a.as_slice(), &data[at..(at + e.size as usize).min(data.len())]);
        }
        drop(pack);
        let _ = std::fs::remove_file(&path);
    }

    /// A label's index reads back as written; a cut or foreign file does not.
    #[test]
    fn bundle_index_round_trips() {
        let held = vec![(0x1122_3344_5566_7788u64, 4096u32, 977u32), (7, 0, 1)];
        let bytes = encode_bundle_index("t6frontextra-01-00ff-2", &held);
        assert_eq!(decode_bundle_index(&bytes), Some(("t6frontextra-01-00ff-2".to_string(), held.clone())));
        assert_eq!(decode_bundle_index(&bytes[..bytes.len() - 1]), None);
        assert_eq!(decode_bundle_index(b"nope"), None);
        let empty = encode_bundle_index("k", &[]);
        assert_eq!(decode_bundle_index(&empty), Some(("k".to_string(), vec![])));
    }

    /// A new bundle takes the entries an earlier one holds, byte for byte,
    /// and leaves the rest (or all, if the earlier file is the wrong size).
    #[test]
    fn bundle_entries_taken_from_an_earlier_bundle() {
        let held = [(7u64, 0u32, 3u32), (7, 10, 2), (9, 0, 4)];
        let bundle = b"abcdeWXYZ";
        let ids = [(9u64, 0u32, 4u32), (8, 0, 1), (7, 10, 2)];
        let mut out = vec![None; 3];
        take_from_bundle(&held, bundle, &ids, &mut out);
        assert_eq!(out, vec![Some(b"WXYZ".to_vec()), None, Some(b"de".to_vec())]);
        let mut out = vec![None; 3];
        take_from_bundle(&held, b"abcdeWXY", &ids, &mut out);
        assert!(out.iter().all(Option::is_none));
    }

    /// A pack's cache keys are cache-safe names, and change with the pack's
    /// build and with the entry, so a patched pack never reads old bytes.
    #[test]
    fn pack_cache_keys_follow_the_pack_and_entry() {
        let id = pack_cache_id("common_mp", 1 << 30, 1_700_000_000);
        assert_eq!(id, pack_cache_id("COMMON_MP", 1 << 30, 1_700_000_000));
        assert_ne!(id, pack_cache_id("common_mp", (1 << 30) + 1, 1_700_000_000));
        assert_ne!(id, pack_cache_id("common_mp", 1 << 30, 1_700_000_001));
        let entry = IndexEntry { key: 0xdead_beef_0000_0001, offset: 4096, size: 977 };
        let key = pack_cache_key(id, entry);
        assert!(key.len() < 200);
        assert!(key.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b) || b == b'-'));
        assert_ne!(key, pack_cache_key(id, IndexEntry { offset: 8192, ..entry }));
        assert_ne!(key, pack_cache_key(id, IndexEntry { size: 978, ..entry }));
    }

    /// Near neighbours share a read; far or too-long ones start a new one.
    #[test]
    fn runs_join_near_neighbours_and_split_far_or_long_ones() {
        let e = |offset: u32, size: u32| IndexEntry { key: 0, offset, size };
        let near = [e(0, 100), e(200, 100), e(150, 10)];
        assert_eq!(runs(&near, |x| x).len(), 1);
        let far = [e(0, 100), e(100 + RUN_GAP as u32 + 1, 10)];
        assert_eq!(runs(&far, |x| x).len(), 2);
        let long = [e(0, RUN_MAX as u32), e(RUN_MAX as u32, 1)];
        assert_eq!(runs(&long, |x| x).len(), 2);
        assert!(runs(&[] as &[IndexEntry], |x| x).is_empty());
    }

    /// A bundle's own gap reads entries a few hundred KB apart on their own.
    #[test]
    fn a_bundle_gap_splits_what_the_shared_gap_joins() {
        let e = |offset: u32, size: u32| IndexEntry { key: 0, offset, size };
        let apart = [e(0, 100), e(100 + BUNDLE_RUN_GAP as u32 + 1, 10), e(400 << 10, 10)];
        assert_eq!(runs(&apart, |x| x).len(), 1);
        assert_eq!(runs_within(&apart, BUNDLE_RUN_GAP, |x| x).len(), 3);
        let adjoining = [e(0, 100), e(100, 100), e(300, 100)];
        assert_eq!(runs_within(&adjoining, BUNDLE_RUN_GAP, |x| x).len(), 1);
    }

    /// Positional reads give the same bytes whatever the order or the thread.
    #[test]
    fn read_at_matches_in_any_order_on_threads() {
        let path = std::env::temp_dir().join(format!("t6_read_at_{}.bin", std::process::id()));
        let data: Vec<u8> = (0..100_000u32).map(|i| (i * 7 + i / 251) as u8).collect();
        std::fs::write(&path, &data).unwrap();
        let file = std::fs::File::open(&path).unwrap();
        let spans: Vec<(u64, usize)> = (0..64).map(|i| ((i * 1500 + 3) as u64, 977 + i)).collect();
        let in_order: Vec<Vec<u8>> = spans.iter().map(|&(a, l)| read_at(&file, a, l).unwrap()).collect();
        std::thread::scope(|s| {
            for chunk in spans.chunks(8).rev() {
                let (file, in_order, spans, data, path) = (&file, &in_order, &spans, &data, &path);
                s.spawn(move || {
                    // (A thread's own handle, as `stored_many` opens, reads the same.)
                    let own = std::fs::File::open(path).unwrap();
                    for span in chunk.iter().rev() {
                        let k = spans.iter().position(|x| x == span).unwrap();
                        assert_eq!(read_at(file, span.0, span.1).unwrap(), in_order[k]);
                        assert_eq!(read_at(&own, span.0, span.1).unwrap(), in_order[k]);
                        let a = span.0 as usize;
                        assert_eq!(in_order[k], data[a..a + span.1]);
                    }
                });
            }
        });
        // Past the end: a short read, as before.
        assert_eq!(read_at(&file, 99_990, 50).unwrap().len(), 10);
        drop(file);
        let _ = std::fs::remove_file(&path);
    }
}
