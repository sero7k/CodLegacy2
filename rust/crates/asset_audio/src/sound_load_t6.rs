//! bo2zm: Black Ops II sound.
//!
//! A T6 zone carries sound *banks* (`SndBank`): alias names with their
//! variants (volume, pitch, distances, falloff curves, the file each one
//! plays, by the hash of its name). The audio itself lives beside the
//! zones in `sound/*.sabl` (loaded, 16-bit PCM) and `*.sabs` (streamed,
//! FLAC) files, read by `sab_t6`. Aliases become `CapturedSound`s whose
//! variants name their clip as streamed `(bank file, entry id)`; the clip
//! store decodes a clip on first use through `decode_t6_clip`.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex, OnceLock};

use asset_core::ZoneOwner;
use asset_transport::ZoneGame;

use crate::{CapturedAlias, CapturedSndCurve, CapturedSound, SoundCatalog, VoicePriority};

/// Every bank file a set of bank names reads, indexed by entry id.
#[derive(Default)]
pub struct T6BankIndex {
    by_id: HashMap<u32, (Arc<sab_t6::BankFile>, sab_t6::BankEntry)>,
    pub files: Vec<PathBuf>,
}

impl T6BankIndex {
    /// Open every `<bank>.<language>.sab*` in `sound_dir` whose name starts
    /// with one of `banks` (and the languages `all` / `english`). Earlier
    /// banks win an id they share with a later one.
    pub fn open(sound_dir: &Path, banks: &[&str]) -> (Self, Vec<String>) {
        Self::open_banks(sound_dir, banks, None)
    }

    /// `open` for one map's banks: the banks the last map read and this one
    /// does not name leave memory (lane B, B2). The menu's, the music's and
    /// the weapons' banks (`open`) stay.
    pub fn open_for_map(sound_dir: &Path, banks: &[&str], map: &str) -> (Self, Vec<String>) {
        Self::open_banks(sound_dir, banks, Some(map))
    }

    fn open_banks(sound_dir: &Path, banks: &[&str], map: Option<&str>) -> (Self, Vec<String>) {
        let mut index = Self::default();
        let mut report = Vec::new();
        let Ok(read) = std::fs::read_dir(sound_dir) else {
            report.push(format!("t6 sound: no sound folder at {}", sound_dir.display()));
            return (index, report);
        };
        let mut paths: Vec<PathBuf> = read.filter_map(|e| e.ok().map(|e| e.path())).collect();
        paths.sort();
        // bo2zm: then the extra folders' banks (`CODLEGACY_T6_EXTRA`).
        paths.extend(asset_transport::t6_extra::extra_files("sabl"));
        paths.extend(asset_transport::t6_extra::extra_files("sabs"));
        // bo2mp: then the mod's (`CODLEGACY_T6_MOD`: `leetin_pt1.all.sabl`, ...).
        paths.extend(asset_transport::t6_mod::files("sabl"));
        paths.extend(asset_transport::t6_mod::files("sabs"));
        for bank in banks {
            for path in &paths {
                let Some(file) = path.file_name().and_then(|n| n.to_str()) else {
                    continue;
                };
                let lower = file.to_ascii_lowercase();
                let wanted = [".all.sabl", ".all.sabs", ".english.sabl", ".english.sabs"]
                    .iter()
                    .any(|tail| lower == format!("{bank}{tail}"));
                if !wanted {
                    continue;
                }
                match sab_t6::BankFile::open(path) {
                    Ok(file) => {
                        let file = Arc::new(file);
                        for e in &file.entries {
                            index
                                .by_id
                                .entry(e.id)
                                .or_insert_with(|| (Arc::clone(&file), *e));
                        }
                        index.files.push(path.clone());
                    }
                    Err(e) => report.push(format!("t6 sound bank gap: {e}")),
                }
            }
        }
        report.push(format!(
            "t6 sound banks: {} files, {} clips",
            index.files.len(),
            index.by_id.len()
        ));
        preload_t6_banks(&index.files, map);
        (index, report)
    }

    pub fn get(&self, id: u32) -> Option<&(Arc<sab_t6::BankFile>, sab_t6::BankEntry)> {
        self.by_id.get(&id)
    }

    pub fn len(&self) -> usize {
        self.by_id.len()
    }

    pub fn is_empty(&self) -> bool {
        self.by_id.is_empty()
    }
}

/// Opened bank files by path, shared by every clip decode.
fn open_banks() -> &'static Mutex<HashMap<String, Arc<sab_t6::BankFile>>> {
    static BANKS: OnceLock<Mutex<HashMap<String, Arc<sab_t6::BankFile>>>> = OnceLock::new();
    BANKS.get_or_init(Default::default)
}

/// Decode one clip: `bank` is the bank file's path, `id` the entry id as
/// eight hex digits (what a T6 alias variant names as its streamed clip).
pub fn decode_t6_clip(bank: &str, id: &str) -> Result<sab_t6::Pcm16, String> {
    let id = u32::from_str_radix(id, 16).map_err(|e| format!("clip id `{id}`: {e}"))?;
    let file = {
        let mut banks = open_banks().lock().map_err(|_| "bank cache poisoned".to_owned())?;
        match banks.get(bank) {
            Some(file) => Arc::clone(file),
            None => {
                let file = Arc::new(sab_t6::BankFile::open(Path::new(bank))?);
                banks.insert(bank.to_owned(), Arc::clone(&file));
                file
            }
        }
    };
    let entry = file
        .entries
        .iter()
        .find(|e| e.id == id)
        .copied()
        .ok_or_else(|| format!("{bank}: no clip {id:08x}"))?;
    // bo2mp: from the bank's map when it has one.
    let resident = resident_banks()
        .lock()
        .ok()
        .and_then(|r| r.get(bank).cloned().flatten());
    if let Some(bytes) = resident {
        let start = usize::try_from(entry.offset).unwrap_or(usize::MAX);
        let end = start.saturating_add(entry.size as usize);
        if let Some(data) = bytes.get(start..end) {
            return sab_t6::bank::decode_entry(&entry, data);
        }
    }
    file.decode(&entry)
}

/// bo2mp: whole bank files mapped into memory, so a clip first asked for
/// mid-match costs no hard-drive seek (a 10-13 match read 270 clips one seek
/// at a time and its sound cut in and out). The map is of the bank's copy on
/// the system drive (`map_bank`): its pages are the system's file cache, read
/// when a clip is first played and given back when memory runs short, not the
/// game's own memory (the whole-bank reads held 279 MB at the menu). Keyed like
/// `open_banks`; `None` while the map is being made.
type ResidentBanks = HashMap<String, Option<Arc<memmap2::Mmap>>>;

fn resident_banks() -> &'static Mutex<ResidentBanks> {
    static RESIDENT: OnceLock<Mutex<ResidentBanks>> = OnceLock::new();
    RESIDENT.get_or_init(Default::default)
}

/// Most bank bytes `preload_t6_banks` holds in memory at once.
const PRELOAD_CAP: u64 = 512 << 20;

/// Bank bytes held against `PRELOAD_CAP` now.
static PRELOADED: AtomicU64 = AtomicU64::new(0);

/// lane B (B2): each resident bank's map (`None`: read for the menu, the
/// music or the weapons, kept for good) and the bytes it holds against
/// `PRELOAD_CAP`. Keyed like `resident_banks`.
type BankOwners = HashMap<String, (Option<String>, u64)>;

fn bank_owners() -> &'static Mutex<BankOwners> {
    static OWNERS: OnceLock<Mutex<BankOwners>> = OnceLock::new();
    OWNERS.get_or_init(Default::default)
}

/// lane B (B2): `files` are `map`'s banks (`None`: kept for good). A bank
/// another map read that `files` does not name is unmapped, and its bytes
/// go back to `PRELOAD_CAP` (a clip still asked for from it reads the disk).
fn release_other_maps(files: &[PathBuf], map: Option<&str>) {
    let (Ok(mut resident), Ok(mut owners)) = (resident_banks().lock(), bank_owners().lock()) else {
        return;
    };
    let named: std::collections::HashSet<String> =
        files.iter().map(|p| p.to_string_lossy().into_owned()).collect();
    for key in &named {
        if let Some((owner, _)) = owners.get_mut(key) {
            if owner.is_some() {
                *owner = map.map(str::to_owned);
            }
        }
    }
    let Some(map) = map else { return };
    let old: Vec<String> = owners
        .iter()
        .filter(|(key, (owner, _))| owner.as_deref().is_some_and(|o| o != map) && !named.contains(*key))
        .map(|(key, _)| key.clone())
        .collect();
    for key in old {
        resident.remove(&key);
        if let Some((owner, bytes)) = owners.remove(&key) {
            PRELOADED.fetch_sub(bytes, Ordering::Relaxed);
            let name = Path::new(&key)
                .file_name()
                .map_or_else(|| key.clone(), |n| n.to_string_lossy().into_owned());
            diag::info!(
                Audio,
                "t6 sound bank released: {name} ({} MB, `{}`'s)",
                bytes >> 20,
                owner.unwrap_or_default()
            );
        }
    }
}

/// Map the loaded (`.sabl`) banks among `files` on one thread at background
/// I/O priority, so the load's own reads go first. A file already mapped, or
/// being mapped, is skipped. `map` names the map they are for (see
/// `release_other_maps`).
/// `CODLEGACY_T6_BANKS_ON_DISK=1` turns this off.
pub fn preload_t6_banks(files: &[PathBuf], map: Option<&str>) {
    if codlegacy_env::var_os("CODLEGACY_T6_BANKS_ON_DISK").is_some() {
        return;
    }
    // (A map's own banks are wanted for its match now.)
    if map.is_some() {
        t6_bank_copies_may_start();
    }
    release_other_maps(files, map);
    let mut todo: Vec<(String, PathBuf)> = {
        let Ok(mut resident) = resident_banks().lock() else {
            return;
        };
        files
            .iter()
            .filter_map(|path| {
                let key = path.to_string_lossy().into_owned();
                if resident.contains_key(&key) {
                    return None;
                }
                resident.insert(key.clone(), None);
                Some((key, path.clone()))
            })
            .collect()
    };
    if todo.is_empty() {
        return;
    }
    // Only the loaded (`.sabl`) banks, where streamed clips live, and at most
    // `PRELOAD_CAP` bytes at once (a released bank's bytes come back): each
    // one is a copy on the system drive.
    todo.retain(|(key, path)| {
        let size = std::fs::metadata(path).map_or(u64::MAX, |m| m.len());
        let keep = key.to_ascii_lowercase().ends_with(".sabl")
            && PRELOADED
                .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |used| {
                    used.checked_add(size).filter(|&total| total <= PRELOAD_CAP)
                })
                .is_ok();
        if !keep {
            if let Ok(mut resident) = resident_banks().lock() {
                resident.remove(key);
            }
        } else if let Ok(mut owners) = bank_owners().lock() {
            owners.insert(key.clone(), (map.map(str::to_owned), size));
        }
        keep
    });
    if todo.is_empty() {
        return;
    }
    let spawned = std::thread::Builder::new()
        .name("t6 bank preload".into())
        .spawn(move || {
            background_io(true);
            for (key, path) in todo {
                let at = std::time::Instant::now();
                let name = path
                    .file_name()
                    .map_or_else(|| "?".to_owned(), |n| n.to_string_lossy().into_owned());
                // (A bank the next map released before its turn is not mapped.)
                if !resident_banks().lock().is_ok_and(|r| r.contains_key(&key)) {
                    continue;
                }
                match map_bank(&path) {
                    Ok((map, from)) => {
                        let mb = map.len() >> 20;
                        if let Ok(mut resident) = resident_banks().lock() {
                            if let Some(slot) = resident.get_mut(&key) {
                                *slot = Some(Arc::new(map));
                            }
                        }
                        diag::info!(
                            Audio,
                            "t6 sound bank mapped: {name} ({mb} MB, {from}, in {} ms)",
                            at.elapsed().as_millis()
                        );
                    }
                    Err(e) => {
                        if let Ok(mut resident) = resident_banks().lock() {
                            resident.remove(&key);
                        }
                        if let Some((_, bytes)) = bank_owners().lock().ok().and_then(|mut o| o.remove(&key)) {
                            PRELOADED.fetch_sub(bytes, Ordering::Relaxed);
                        }
                        diag::warn!(Audio, "t6 sound bank preload `{name}`: {e}");
                    }
                }
            }
            background_io(false);
        });
    if let Err(e) = spawned {
        diag::warn!(Audio, "t6 sound bank preload thread: {e}");
    }
}

/// The artifact cache kind for whole loaded banks.
const BANK_CACHE_KIND: &str = "t6bank";

/// lane B (B1): a loaded bank mapped from its copy in the artifact cache (the
/// system drive), which the first launch makes from the install a megabyte at
/// a time. `cmn_root.all.sabl` alone is 254 MB, 25 s to 72 s off a hard drive
/// beside the menu's pictures; after the first launch nothing is read until a
/// clip plays. `CODLEGACY_T6_PACK_CACHE=0` (or a cache that cannot be written)
/// maps the install, read through once so the system's file cache holds it.
/// Returns where the map came from, for the log.
fn map_bank(path: &Path) -> std::io::Result<(memmap2::Mmap, &'static str)> {
    use std::io::Read;
    let off = codlegacy_env::var("CODLEGACY_T6_PACK_CACHE").is_ok_and(|v| v.trim() == "0");
    let meta = std::fs::metadata(path)?;
    let cached = |key: &str| {
        asset_transport::cache_open(BANK_CACHE_KIND, key)
            .filter(|file| file.metadata().is_ok_and(|m| m.len() == meta.len()))
    };
    if let Some(key) = bank_cache_key(path, &meta).filter(|_| !off) {
        if let Some(file) = cached(&key) {
            return Ok((map_file(&file)?, "from the cache"));
        }
        let waited = wait_to_copy();
        if waited.as_secs_f32() >= 0.1 {
            diag::info!(
                Audio,
                "t6 sound bank `{}`: its copy waited {:.1} s for the menu",
                path.file_name().map_or_else(|| "?".into(), |n| n.to_string_lossy()),
                waited.as_secs_f32()
            );
        }
        let copied = asset_transport::cache_put_with(BANK_CACHE_KIND, &key, |out| {
            let mut install = std::io::BufReader::with_capacity(1 << 20, std::fs::File::open(path)?);
            let len = std::io::copy(&mut install, out)?;
            if len == meta.len() {
                Ok(())
            } else {
                Err(std::io::Error::other("the bank changed while it was copied"))
            }
        });
        match copied.ok().and_then(|()| cached(&key)) {
            Some(file) => return Ok((map_file(&file)?, "copied to the cache")),
            None => diag::warn!(Audio, "t6 sound bank `{}`: not copied to the cache", path.display()),
        }
    }
    let mut file = std::fs::File::open(path)?;
    let mut chunk = vec![0u8; 1 << 20];
    while file.read(&mut chunk)? > 0 {}
    Ok((map_file(&file)?, "from the install"))
}

/// bo2mp: a bank's first copy into the cache waits for the menu. The copy
/// (254 MB at a megabyte a second off his hard drive) and the system's
/// write-back of it fought the menu's first frame for the disk: the 11-05
/// cold launch put the menu up at 275 s, the moment `cmn_root`'s copy ended
/// (4.8 s warm, with nothing to copy). A clip asked for before then reads the
/// install. The menu coming up, START MATCH or a map's own banks let the
/// copies start; ten minutes if nothing does.
/// `CODLEGACY_T6_BANK_COPY_AT_BOOT=1` copies at once, as before.
static COPIES_MAY_START: (Mutex<bool>, Condvar) = (Mutex::new(false), Condvar::new());

/// The menu is up, or a match is loading: the banks' copies may start.
pub fn t6_bank_copies_may_start() {
    if let Ok(mut go) = COPIES_MAY_START.0.lock() {
        *go = true;
        COPIES_MAY_START.1.notify_all();
    }
}

/// Until the copies may start; how long that took.
fn wait_to_copy() -> std::time::Duration {
    let at = std::time::Instant::now();
    if codlegacy_env::var("CODLEGACY_T6_BANK_COPY_AT_BOOT").is_ok_and(|v| v.trim() == "1") {
        return at.elapsed();
    }
    if let Ok(go) = COPIES_MAY_START.0.lock() {
        let _ = COPIES_MAY_START.1.wait_timeout_while(go, std::time::Duration::from_secs(600), |go| !*go);
    }
    at.elapsed()
}

/// A read-only map of a whole file.
fn map_file(file: &std::fs::File) -> std::io::Result<memmap2::Mmap> {
    // SAFETY: bank files are only read: the install's are the game's own, and
    // a cache copy is written once to a temporary name and renamed into place
    // before it is opened here.
    unsafe { memmap2::Mmap::map(file) }
}

/// A bank file's cache key: its name, size and write time.
fn bank_cache_key(path: &Path, meta: &std::fs::Metadata) -> Option<String> {
    let name = path.file_name()?.to_string_lossy().to_ascii_lowercase();
    let modified = meta.modified().ok()?.duration_since(std::time::UNIX_EPOCH).ok()?.as_secs();
    let id = asset_transport::fnv1a64(path.to_string_lossy().to_ascii_lowercase().as_bytes());
    let id = asset_transport::fnv1a64_more(id, &meta.len().to_le_bytes());
    let id = asset_transport::fnv1a64_more(id, &modified.to_le_bytes());
    let stem: String = name
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '.' || c == '_' { c } else { '-' })
        .take(96)
        .collect();
    Some(format!("1-{id:016x}-{stem}"))
}

/// bo2mp: this thread's disk reads wait behind everyone else's (Windows
/// background mode), or go back to normal.
pub fn background_io(on: bool) {
    #[cfg(windows)]
    {
        unsafe extern "system" {
            fn GetCurrentThread() -> isize;
            fn SetThreadPriority(thread: isize, priority: i32) -> i32;
        }
        const THREAD_MODE_BACKGROUND_BEGIN: i32 = 0x0001_0000;
        const THREAD_MODE_BACKGROUND_END: i32 = 0x0002_0000;
        let mode = if on {
            THREAD_MODE_BACKGROUND_BEGIN
        } else {
            THREAD_MODE_BACKGROUND_END
        };
        // SAFETY: the current thread's pseudo handle.
        unsafe {
            SetThreadPriority(GetCurrentThread(), mode);
        }
    }
    #[cfg(not(windows))]
    let _ = on;
}

/// The curve name a falloff curve index stands for in the driver globals.
fn curve(globals: Option<&asset_t6::SndDriverGlobalsRef>, index: u32) -> Option<CapturedSndCurve> {
    let c = globals?.curves.get(index as usize)?;
    // Curves end at the first point whose x falls back below the last.
    let mut knots: Vec<(f32, f32)> = Vec::with_capacity(8);
    for p in c.points {
        if knots.last().is_some_and(|&(x, _)| p[0] < x) {
            break;
        }
        knots.push((p[0], p[1]));
    }
    Some(CapturedSndCurve {
        name: format!("t6/curve/{}", c.name),
        knots,
    })
}

fn alias_variant(
    a: &asset_t6::SndAliasRef,
    list: &str,
    index: &T6BankIndex,
    globals: Option<&asset_t6::SndDriverGlobalsRef>,
) -> CapturedAlias {
    let (vol_min, vol_max) = a.volume();
    let (pitch_min, pitch_max) = a.pitch();
    let streamed = index
        .get(a.asset_id)
        .map(|(file, _)| (file.path.to_string_lossy().into_owned(), format!("{:08x}", a.asset_id)));
    CapturedAlias {
        alias_name: list.to_owned(),
        subtitle: (!a.subtitle.is_empty()).then(|| a.subtitle.clone()),
        secondary: (!a.secondary.is_empty()).then(|| a.secondary.clone()),
        file_type: Some(2),
        file_exists: Some(u8::from(streamed.is_some())),
        file_name: (!a.asset_file.is_empty()).then(|| a.asset_file.clone()),
        streamed,
        vol_min,
        vol_max,
        pitch_min: if pitch_min > 0.0 { pitch_min } else { 1.0 },
        pitch_max: if pitch_max > 0.0 { pitch_max } else { 1.0 },
        dist_min: f32::from(a.dist_min),
        dist_max: f32::from(a.dist_max),
        flags: Some(a.flags[0]),
        probability: f32::from(a.probability) / 255.0,
        start_delay: i32::from(a.start_delay),
        volume_falloff: curve(globals, a.volume_falloff_curve()),
        near_falloff: curve(globals, a.volume_min_falloff_curve()),
        voice_priority: Some(VoicePriority {
            thresholds: [a.min_priority_threshold, a.max_priority_threshold],
            values: [a.min_priority, a.max_priority],
            distance_max: f32::from(a.dist_max),
        }),
        envelop_min: f32::from(a.envelop_min),
        envelop_max: f32::from(a.envelop_max),
        envelop_percentage: f32::from(a.envelop_percentage) / 65535.0,
        reverb_send: f32::from(a.reverb_send) / 65535.0,
        limit_count: Some(a.limit_count),
        entity_limit_count: Some(a.entity_limit_count),
        ..CapturedAlias::default()
    }
}

/// What a T6 catalog build found.
#[derive(Clone, Debug, Default)]
pub struct T6SoundCensus {
    pub aliases: usize,
    pub variants: usize,
    pub with_clip: usize,
    pub without_clip: usize,
}

/// Build the engine's sound catalog from T6 banks (later banks override an
/// alias an earlier one defines). `index` says where each clip lives.
pub fn build_t6_sound_catalog(
    banks: &[&asset_t6::SndBankRef],
    globals: Option<&asset_t6::SndDriverGlobalsRef>,
    index: &T6BankIndex,
    zone: ZoneOwner,
) -> (SoundCatalog, T6SoundCensus) {
    let mut catalog = SoundCatalog::default();
    catalog.set_capture_game(ZoneGame::T6);
    catalog.set_capture_zone(zone);
    let mut census = T6SoundCensus::default();
    for bank in banks {
        for r in &bank.radverbs {
            catalog.radverbs.insert(r.name.clone(), r.values);
        }
        for list in &bank.aliases {
            if list.name.is_empty() {
                continue;
            }
            let aliases: Vec<CapturedAlias> = list
                .entries
                .iter()
                .map(|a| alias_variant(a, &list.name, index, globals))
                .collect();
            census.aliases += 1;
            census.variants += aliases.len();
            let with = aliases.iter().filter(|a| a.streamed.is_some()).count();
            census.with_clip += with;
            census.without_clip += aliases.len() - with;
            catalog.ingest_sound(CapturedSound {
                name: list.name.clone(),
                aliases,
                game: ZoneGame::T6,
                zone,
                namespace: None,
            });
        }
    }
    catalog.finalize();
    (catalog, census)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_mapped_bank_reads_back_its_bytes() {
        let dir = std::env::temp_dir().join(format!("t6-bank-map-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("test.all.sabl");
        let bytes: Vec<u8> = (0..300_000u32).map(|i| (i % 251) as u8).collect();
        std::fs::write(&path, &bytes).unwrap();
        let map = map_file(&std::fs::File::open(&path).unwrap()).unwrap();
        assert_eq!(&map[..], &bytes[..]);
        drop(map);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_new_map_releases_only_the_last_maps_own_banks() {
        let keys = ["lane-b-test/mpl_a.all.sabl", "lane-b-test/cmn.all.sabl", "lane-b-test/mpl_ab.all.sabl"];
        for (key, owner) in keys.iter().zip([Some("a"), None, Some("a")]) {
            resident_banks().lock().unwrap().insert((*key).to_owned(), None);
            bank_owners().lock().unwrap().insert((*key).to_owned(), (owner.map(str::to_owned), 4));
            PRELOADED.fetch_add(4, Ordering::Relaxed);
        }
        release_other_maps(&[PathBuf::from(keys[2])], Some("b"));
        {
            let resident = resident_banks().lock().unwrap();
            assert!(!resident.contains_key(keys[0]), "map a's own bank leaves");
            assert!(resident.contains_key(keys[1]), "a shared bank stays");
            assert!(resident.contains_key(keys[2]), "a bank map b names stays");
        }
        assert_eq!(bank_owners().lock().unwrap()[keys[2]].0.as_deref(), Some("b"));
    }
}
