use std::collections::HashSet;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Condvar, Mutex, OnceLock};

use crate::discover::ensure_artifacts_dir;

fn cache_budget_bytes() -> u64 {
    static BUDGET: OnceLock<u64> = OnceLock::new();
    *BUDGET.get_or_init(|| {
        codlegacy_env::var("CODLEGACY_CACHE_BUDGET_MIB")
            .ok()
            .and_then(|value| value.trim().parse::<u64>().ok())
            // (8 GiB: the pictures' mip chains and the shaders fill 4, and a
            // converted Modern Warfare III map with its collision is
            // several hundred MiB more.)
            .unwrap_or(8192)
            << 20
    })
}

pub fn cache_get(kind: &str, key: &str) -> Option<Vec<u8>> {
    let path = cache_path(kind, key).ok()?;
    fs::read(path).ok()
}

pub fn cache_open(kind: &str, key: &str) -> Option<fs::File> {
    fs::File::open(cache_path(kind, key).ok()?).ok()
}

/// Removes the entries of `kind` whose key starts with `prefix`, but
/// `keep` (an older build of the same thing, which a newer key replaces).
pub fn cache_prune(kind: &str, prefix: &str, keep: &str) {
    let Ok(path) = cache_path(kind, keep) else {
        return;
    };
    let Some(dir) = path.parent() else {
        return;
    };
    let Ok(listing) = fs::read_dir(dir) else {
        return;
    };
    for entry in listing.flatten() {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name.starts_with(prefix) && name != keep && !name.ends_with(".tmp") {
            let _ = fs::remove_file(entry.path());
        }
    }
}

pub fn cache_put(kind: &str, key: &str, bytes: &[u8]) -> Result<(), String> {
    cache_put_with(kind, key, |file| file.write_all(bytes))
}

pub fn cache_put_with(
    kind: &str,
    key: &str,
    write: impl FnOnce(&mut fs::File) -> std::io::Result<()>,
) -> Result<(), String> {
    let path = cache_path(kind, key)?;
    let Some(parent) = path.parent() else {
        return Err(format!("cache path has no directory: {}", path.display()));
    };
    fs::create_dir_all(parent)
        .map_err(|error| format!("cache mkdir {}: {error}", parent.display()))?;

    let tmp = unique_temp(parent, key)?;
    let write = (|| -> std::io::Result<()> {
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&tmp)?;
        write(&mut file)
    })();
    if let Err(error) = write {
        let _ = fs::remove_file(&tmp);
        return Err(format!("cache write {}: {error}", tmp.display()));
    }

    match fs::rename(&tmp, &path) {
        Ok(()) => {}

        Err(_) if path.exists() => {
            let _ = fs::remove_file(&tmp);
        }
        Err(error) => {
            let _ = fs::remove_file(&tmp);
            return Err(format!(
                "cache rename {} -> {}: {error}",
                tmp.display(),
                path.display()
            ));
        }
    }
    request_sweep(kind);
    Ok(())
}

fn unique_temp(parent: &Path, key: &str) -> Result<PathBuf, String> {
    static SEQUENCE: AtomicU64 = AtomicU64::new(0);
    let n = SEQUENCE.fetch_add(1, Ordering::Relaxed);
    Ok(parent.join(format!("{key}.{}.{n:x}.tmp", std::process::id())))
}

pub struct CacheFlight {
    key: String,
}

fn inflight() -> &'static (Mutex<HashSet<String>>, Condvar) {
    static INFLIGHT: OnceLock<(Mutex<HashSet<String>>, Condvar)> = OnceLock::new();
    INFLIGHT.get_or_init(|| (Mutex::new(HashSet::new()), Condvar::new()))
}

#[must_use]
pub fn cache_flight(kind: &str, key: &str) -> CacheFlight {
    let key = format!("{kind}/{key}");
    let (lock, signal) = inflight();
    let mut live = lock
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let waited = live.contains(&key).then(std::time::Instant::now);
    while live.contains(&key) {
        live = signal
            .wait(live)
            .unwrap_or_else(std::sync::PoisonError::into_inner);
    }
    live.insert(key.clone());
    drop(live);
    // Load speed: a thread that waited long on another's write of the same
    // file is named (a background-priority thread on a busy disk can hold
    // one for a long time).
    if let Some(from) = waited.filter(|from| from.elapsed().as_secs_f64() >= 1.0) {
        diag::info!(
            Launch,
            "cache: {} waited {:.1} s for another thread's {key}",
            std::thread::current().name().unwrap_or("a thread"),
            from.elapsed().as_secs_f64()
        );
    }
    CacheFlight { key }
}

impl Drop for CacheFlight {
    fn drop(&mut self) {
        let (lock, signal) = inflight();
        lock.lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .remove(&self.key);
        signal.notify_all();
    }
}

fn cache_path(kind: &str, key: &str) -> Result<PathBuf, String> {
    if !kind_ok(kind) || !key_ok(key) {
        return Err("cache kind/key must be ascii [0-9a-z._-]".into());
    }
    Ok(root_for(kind)?
        .join(kind)
        .join(&key[..2.min(key.len())])
        .join(key))
}

/// Where the prepared-artifact cache lives.
///
/// The cache is thousands of small files (mips, WGSL) read by every load
/// worker at once; next to an install on a hard drive each one was a seek, and
/// a cache in the game folder was lost with the run folder. So by default it
/// sits in `%LOCALAPPDATA%\iw4l\cache-<install id>` (the system drive, where
/// games keep their shader caches), one folder per install so two builds never
/// read each other's entries. `CODLEGACY_CACHE_DIR` names any folder instead; an
/// `codlegacy-artifacts/cache` that is a link (a test harness's shared warm cache)
/// is still followed; with no `LOCALAPPDATA` it stays in
/// `codlegacy-artifacts/cache`.
pub fn cache_root() -> Result<PathBuf, String> {
    roots().clone().map(|(own, _)| own)
}

/// bo2mp: kinds that are plain copies of the install's own files (Black Ops
/// II's picture pack entries, its sound banks, Black Ops' soldier maps), keyed by that file's name,
/// size and time, so every build would keep the same bytes. They live in one
/// folder all installs share: a new build folder starts with them warm (each
/// new one read Nuketown's pictures off the hard disk again, 100 s), and the
/// system drive holds one copy of them, not one per build. The decoded ones
/// join them: MW2/MW3 IWD mips (keyed by the entry's crc, size and name),
/// Black Ops' decoded xWMA sounds (keyed by the packets themselves), the
/// localized strings (keyed by the zone's path, size and time) and other
/// games' menu pictures (keyed by name and a hash of the code that makes
/// them), and the bundles of Black Ops II's menu pictures (keyed by the
/// packs' builds and entries): each key carries its format, so a build that
/// changes one misses. The bots' walk graphs join them too (keyed by the map's
/// content, its collision and the bake's schema): a new build folder paid
/// Skyjacked's 6.1 s bake again before admission.
/// The other games' shaders translated to WGSL join them as `wgsl_shared` (keyed by the programs, the
/// translation's inputs and a hash of the translator's own source): a new
/// build folder translated MW3 Terminal's 11,623 programs again (26.8 s).
const SHARED_KINDS: &[&str] = &["t6pak", "t6bank", "t5soldier", "mips", "xwma_pcm", "localize", "ui_picture", "t6bundle", "navshared", "wgsl_shared"];

fn root_for(kind: &str) -> Result<PathBuf, String> {
    if SHARED_KINDS.contains(&kind) {
        roots().clone().map(|(_, shared)| shared)
    } else {
        cache_root()
    }
}

/// The shared kinds' folder beside the per-install ones:
/// `%LOCALAPPDATA%\iw4l\cache-shared`.
fn shared_beside(own: &Path) -> PathBuf {
    own.with_file_name("cache-shared")
}

/// The per-install folder and the shared kinds' folder (the same folder when
/// the cache is in a fixed one: the override, the harness link).
fn roots() -> &'static Result<(PathBuf, PathBuf), String> {
    static ROOTS: OnceLock<Result<(PathBuf, PathBuf), String>> = OnceLock::new();
    ROOTS.get_or_init(|| {
        let override_dir = codlegacy_env::var_os("CODLEGACY_CACHE_DIR");
        if let Some(dir) = override_dir.as_deref().filter(|dir| !dir.is_empty()) {
            return Ok((PathBuf::from(dir), PathBuf::from(dir)));
        }
        let artifacts = ensure_artifacts_dir()?;
        let install = fs::canonicalize(&artifacts).unwrap_or_else(|_| artifacts.clone());
        let legacy = artifacts.join("cache");
        let legacy_is_link =
            fs::symlink_metadata(&legacy).is_ok_and(|meta| meta.file_type().is_symlink());
        let local = std::env::var_os("LOCALAPPDATA");
        match pick_cache_root(
            override_dir.as_deref(),
            &legacy,
            legacy_is_link,
            local.as_deref(),
            &install,
        ) {
            CacheRoot::Local(dir) if fs::create_dir_all(&dir).is_ok() => {
                let _ = fs::write(dir.join("install.txt"), install.to_string_lossy().as_bytes());
                diag::info!(Zone, "artifact cache: {} (for {})", dir.display(), install.display());
                let shared = shared_beside(&dir);
                let shared = if fs::create_dir_all(&shared).is_ok() { shared } else { dir.clone() };
                Ok((dir, shared))
            }
            CacheRoot::Local(_) => Ok((legacy.clone(), legacy)),
            CacheRoot::Fixed(dir) => Ok((dir.clone(), dir)),
        }
    })
}

#[derive(Debug, PartialEq, Eq)]
enum CacheRoot {
    /// Used as it is: the override, the harness link or the folder in the install.
    Fixed(PathBuf),
    /// The per-install folder under the local app data, made on first use.
    Local(PathBuf),
}

/// The choice behind [`cache_root`], from what the environment holds.
fn pick_cache_root(
    override_dir: Option<&std::ffi::OsStr>,
    legacy: &Path,
    legacy_is_link: bool,
    local_app_data: Option<&std::ffi::OsStr>,
    install: &Path,
) -> CacheRoot {
    if let Some(dir) = override_dir.filter(|dir| !dir.is_empty()) {
        return CacheRoot::Fixed(PathBuf::from(dir));
    }
    if legacy_is_link {
        return CacheRoot::Fixed(legacy.to_path_buf());
    }
    let Some(local) = local_app_data.filter(|dir| !dir.is_empty()) else {
        return CacheRoot::Fixed(legacy.to_path_buf());
    };
    let id = fnv1a64(install.to_string_lossy().to_lowercase().as_bytes());
    CacheRoot::Local(PathBuf::from(local).join("iw4l").join(format!("cache-{id:016x}")))
}

pub(crate) fn kind_ok(kind: &str) -> bool {
    !kind.is_empty() && kind.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
}

fn key_ok(key: &str) -> bool {
    !key.is_empty()
        && key.len() < 200
        && key
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'))
}

/// Once a run for each folder: the per-install one and the shared kinds'.
fn request_sweep(kind: &str) {
    static OWN: std::sync::Once = std::sync::Once::new();
    static SHARED: std::sync::Once = std::sync::Once::new();
    let once = if SHARED_KINDS.contains(&kind) { &SHARED } else { &OWN };
    once.call_once(|| {
        let Ok(root) = root_for(kind) else {
            return;
        };

        let _ = std::thread::Builder::new()
            .name("codlegacy cache sweep".to_owned())
            .spawn(move || sweep(&root, cache_budget_bytes()));
    });
}

fn sweep(cache: &Path, budget: u64) {
    let mut entries = Vec::new();
    let mut total = 0u64;
    let stale_temp = std::time::SystemTime::now() - std::time::Duration::from_secs(3600);
    let mut stack = vec![cache.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(listing) = fs::read_dir(&dir) else {
            continue;
        };
        for entry in listing.filter_map(Result::ok) {
            let path = entry.path();
            let Ok(meta) = entry.metadata() else { continue };
            if meta.is_dir() {
                stack.push(path);
                continue;
            }
            let modified = meta.modified().unwrap_or(std::time::UNIX_EPOCH);

            if path.extension().is_some_and(|ext| ext == "tmp") {
                if modified < stale_temp {
                    let _ = fs::remove_file(&path);
                }
                continue;
            }
            total += meta.len();
            entries.push((modified, meta.len(), path));
        }
    }
    if total <= budget {
        return;
    }
    entries.sort_by_key(|(modified, _, _)| *modified);
    let mut freed = 0u64;
    let mut dropped = 0usize;
    for (_, len, path) in entries {
        if total - freed <= budget {
            break;
        }
        if fs::remove_file(&path).is_ok() {
            freed += len;
            dropped += 1;
        }
    }
    diag::info!(
        Zone,
        "artifact cache sweep: {} MiB over {} MiB budget — evicted {dropped} files ({} MiB)",
        total >> 20,
        budget >> 20,
        freed >> 20
    );
}

pub fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut hash = 0xcbf29ce484222325u64;
    for &byte in bytes {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x100_0000_01b3);
    }
    hash
}

pub fn fnv1a64_more(mut hash: u64, bytes: &[u8]) -> u64 {
    for &byte in bytes {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x100_0000_01b3);
    }
    hash
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsStr;

    const LEGACY: &str = "codlegacy-artifacts/cache";

    fn pick(over: Option<&str>, link: bool, local: Option<&str>, install: &str) -> CacheRoot {
        pick_cache_root(
            over.map(OsStr::new),
            Path::new(LEGACY),
            link,
            local.map(OsStr::new),
            Path::new(install),
        )
    }

    #[test]
    fn local_app_data_holds_one_folder_per_install() {
        let CacheRoot::Local(a) = pick(None, false, Some("C:/Users/x/AppData/Local"), "D:/game/codlegacy-artifacts") else {
            panic!("expected the local app data folder");
        };
        assert!(a.starts_with("C:/Users/x/AppData/Local/iw4l"));
        let name = a.file_name().unwrap().to_string_lossy().into_owned();
        assert!(name.starts_with("cache-") && name.len() == "cache-".len() + 16, "{name}");

        // Same install in another case is the same folder; another install is not.
        assert_eq!(
            pick(None, false, Some("C:/Users/x/AppData/Local"), "d:/GAME/codlegacy-artifacts"),
            CacheRoot::Local(a.clone())
        );
        assert_ne!(
            pick(None, false, Some("C:/Users/x/AppData/Local"), "E:/other/codlegacy-artifacts"),
            CacheRoot::Local(a)
        );
    }

    #[test]
    fn shared_kinds_sit_beside_the_install_folders() {
        let CacheRoot::Local(own) = pick(None, false, Some("C:/Users/x/AppData/Local"), "D:/game/codlegacy-artifacts") else {
            panic!("expected the local app data folder");
        };
        assert_eq!(shared_beside(&own), PathBuf::from("C:/Users/x/AppData/Local/iw4l/cache-shared"));
        let CacheRoot::Local(other) = pick(None, false, Some("C:/Users/x/AppData/Local"), "E:/other/codlegacy-artifacts") else {
            panic!("expected the local app data folder");
        };
        assert_eq!(shared_beside(&other), shared_beside(&own));
    }

    #[test]
    fn override_beats_link_and_local_app_data() {
        assert_eq!(
            pick(Some("E:/warm"), true, Some("C:/local"), "D:/game"),
            CacheRoot::Fixed(PathBuf::from("E:/warm"))
        );
        // An empty override counts as unset.
        assert!(matches!(pick(Some(""), false, Some("C:/local"), "D:/game"), CacheRoot::Local(_)));
    }

    #[test]
    fn harness_link_and_missing_local_app_data_keep_the_install_folder() {
        assert_eq!(pick(None, true, Some("C:/local"), "D:/game"), CacheRoot::Fixed(PathBuf::from(LEGACY)));
        assert_eq!(pick(None, false, None, "D:/game"), CacheRoot::Fixed(PathBuf::from(LEGACY)));
        assert_eq!(pick(None, false, Some(""), "D:/game"), CacheRoot::Fixed(PathBuf::from(LEGACY)));
    }
}
