//! bo2mp: the shared zones (`core_*`, `mp_common`, ...) read and walked once
//! per process and kept (with `BO2MP_T7_NATIVE=1`): every Black Ops III map uses them, and the map
//! load, the material layouts and the native renderer all need them. A map
//! zone is not kept here (it is read with its map).

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};

use crate::walk::capture::Capture;

/// A zone, inflated and walked.
pub struct WalkedZone {
    pub name: String,
    pub image: Arc<Vec<u8>>,
    pub capture: Arc<Capture>,
    /// Whether the walk reached the zone's end.
    pub complete: bool,
    pub assets: usize,
}

/// The shared zone `name` (`core_common`) beside `dir`, read and walked the
/// first time it is asked for (None: not there or not readable; that too is
/// remembered).
type Slot = Arc<OnceLock<Option<Arc<WalkedZone>>>>;
static CACHE: OnceLock<Mutex<HashMap<PathBuf, Slot>>> = OnceLock::new();

/// Lets go of the kept zones but `core_patch` (what a stage still holds of
/// one stays): once a map's renderer is made nothing asks for the others
/// again until the next map; the code programs are asked for again when
/// the frame is resized.
pub fn release() {
    release_keeping(&[]);
}

/// As `release`, also keeping the zones in `names`.
pub fn release_keeping(names: &[&str]) {
    if let Some(c) = CACHE.get()
        && let Ok(mut c) = c.lock()
    {
        c.retain(|p, _| p.file_stem().is_some_and(|n| n == "core_patch" || names.iter().any(|k| n == *k)));
    }
}

pub fn zone(dir: &Path, name: &str) -> Option<Arc<WalkedZone>> {
    let path = dir.join(format!("{name}.ff"));
    let read = || {
        crate::Zone::open(&path).ok().map(|z| {
            let w = crate::walk::walk(&z);
            Arc::new(WalkedZone {
                name: name.to_owned(),
                complete: matches!(w.stop, crate::walk::Stop::Done { .. }),
                assets: w.walked.len(),
                capture: Arc::new(w.capture),
                image: Arc::new(z.image),
            })
        })
    };
    // (Kept for the native renderer, which is on unless `BO2MP_T7_NATIVE=0`
    // or the option is off ([`keep`]); otherwise the caller takes what it
    // needs and the zone is freed. A zone two threads ask for at once is
    // read once: the second waits for the first.)
    if !keeping() {
        return read();
    }
    let slot = {
        let mut c = CACHE.get_or_init(Default::default).lock().ok()?;
        c.entry(path.clone()).or_default().clone()
    };
    slot.get_or_init(read).clone()
}

/// The shared zone `name` beside `dir` if it is kept now (never reads it).
pub fn held(dir: &Path, name: &str) -> Option<Arc<WalkedZone>> {
    let c = CACHE.get()?.lock().ok()?;
    c.get(&dir.join(format!("{name}.ff")))?.get()?.clone()
}

static KEEP: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(true);

/// Whether the shared zones are kept after they are read (the native
/// renderer is on; on by default). Set by the map loader from the option.
pub fn keep(on: bool) {
    KEEP.store(on, std::sync::atomic::Ordering::Relaxed);
}

fn keeping() -> bool {
    KEEP.load(std::sync::atomic::Ordering::Relaxed) && !std::env::var("BO2MP_T7_NATIVE").is_ok_and(|v| v == "0")
}

/// Reads and walks the shared zones `names` side by side (one thread
/// each, a few at a time), so the first `zone` call for each finds it
/// cached. Without keeping there is nothing to keep them in, so it does
/// nothing.
pub fn preload(dir: &Path, names: &[&str]) {
    if !keeping() {
        return;
    }
    let next = std::sync::atomic::AtomicUsize::new(0);
    let workers = std::thread::available_parallelism().map_or(2, usize::from).clamp(2, 6).min(names.len());
    std::thread::scope(|s| {
        for _ in 0..workers {
            s.spawn(|| {
                loop {
                    let i = next.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    let Some(n) = names.get(i) else { break };
                    let _ = zone(dir, n);
                }
            });
        }
    });
}
