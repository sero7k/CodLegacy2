//! bo2mp: one read of a map zone for two readers. The converted lane reads
//! the map (`JupMap::load`) for the collision and the entities; when the
//! native renderer (`render_jup`) draws the map it needs the same read, so the
//! lane keeps it here instead of the renderer reading the zone again.
//!
//! `expect` announces a load; it ends in `hand_over` or `cancel` (a cached
//! conversion has no map to hand over); `wait` blocks until then.

use std::path::{Path, PathBuf};
use std::sync::{Condvar, Mutex};
use std::time::{Duration, Instant};

use super::JupMap;

struct Handed {
    expected: Option<PathBuf>,
    map: Option<(PathBuf, JupMap)>,
}

static HANDED: Mutex<Handed> = Mutex::new(Handed { expected: None, map: None });
static CV: Condvar = Condvar::new();

/// A load for `zone` has begun and will end in [`hand_over`] or [`cancel`].
pub fn expect(zone: &Path) {
    if let Ok(mut h) = HANDED.lock() {
        h.expected = Some(zone.to_owned());
        h.map = None;
    }
    CV.notify_all();
}

/// The load of `zone` ended without a map to hand over.
pub fn cancel(zone: &Path) {
    if let Ok(mut h) = HANDED.lock()
        && h.expected.as_deref() == Some(zone)
    {
        h.expected = None;
    }
    CV.notify_all();
}

/// Keeps `map` (read from `zone`) for the renderer's scene read.
pub fn hand_over(zone: &Path, map: JupMap) {
    if let Ok(mut h) = HANDED.lock() {
        h.map = Some((zone.to_owned(), map));
        h.expected = None;
    }
    CV.notify_all();
}

/// The map handed over for `zone`, waiting (up to `limit`) while a load of
/// it is announced and not over. `None`: none was kept; read the zone.
pub fn wait(zone: &Path, limit: Duration) -> Option<JupMap> {
    let end = Instant::now() + limit;
    let mut h = HANDED.lock().ok()?;
    loop {
        if h.map.as_ref().is_some_and(|(z, _)| z == zone) {
            return h.map.take().map(|(_, m)| m);
        }
        if h.expected.as_deref() != Some(zone) {
            return None;
        }
        let left = end.checked_duration_since(Instant::now())?;
        h = CV.wait_timeout(h, left).ok()?.0;
    }
}
