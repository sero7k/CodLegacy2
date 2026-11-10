//! `BO2MP_IW6_LOAD_TIMES=1`: one line per load stage with the time since
//! the previous stage (the map load profile; nothing is recorded otherwise).

use std::sync::Mutex;
use std::time::Instant;

static LAST: Mutex<Option<Instant>> = Mutex::new(None);

pub fn on() -> bool {
    static S: crate::switch::Switch = crate::switch::Switch::off("BO2MP_IW6_LOAD_TIMES");
    S.get()
}

/// Marks the end of `what`.
pub fn stage(what: &str) {
    if !on() {
        return;
    }
    let now = Instant::now();
    let mut g = LAST.lock().unwrap_or_else(|e| e.into_inner());
    let dt = g.map_or(0.0, |t| now.duration_since(t).as_secs_f64() * 1000.0);
    *g = Some(now);
    eprintln!("load stage: {what}: {dt:.0} ms");
}

use std::sync::atomic::{AtomicU64, Ordering::Relaxed};

static NS: [AtomicU64; 4] = [AtomicU64::new(0), AtomicU64::new(0), AtomicU64::new(0), AtomicU64::new(0)];
static N: [AtomicU64; 4] = [AtomicU64::new(0), AtomicU64::new(0), AtomicU64::new(0), AtomicU64::new(0)];
const NAMES: [&str; 4] = ["dxbc decode + translate", "shader modules (wgsl parse)", "render pipelines", "make_pipeline total"];

/// A running total for one of the pipeline steps (0 translate, 1 modules, 2 pipelines, 3 all).
pub struct Timer(Option<(usize, Instant)>);

pub fn timer(k: usize) -> Timer {
    Timer(on().then(|| (k, Instant::now())))
}

impl Drop for Timer {
    fn drop(&mut self) {
        if let Some((k, t)) = self.0 {
            NS[k].fetch_add(t.elapsed().as_nanos() as u64, Relaxed);
            N[k].fetch_add(1, Relaxed);
        }
    }
}

/// Prints the totals.
pub fn totals() {
    if !on() {
        return;
    }
    eprintln!("load total: distinct shader texts: {} vertex, {} pixel", distinct().0, distinct().1);
    for k in 0..4 {
        eprintln!("load total: {}: {:.0} ms in {} calls", NAMES[k], NS[k].load(Relaxed) as f64 / 1e6, N[k].load(Relaxed));
    }
}

static SEEN: Mutex<Option<std::collections::HashSet<u64>>> = Mutex::new(None);
static SEEN_N: [AtomicU64; 2] = [AtomicU64::new(0), AtomicU64::new(0)];

/// Counts a shader text (0 vertex, 1 pixel) and, in the totals, how many were distinct.
pub fn seen(kind: usize, text: &str) {
    if !on() {
        return;
    }
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    (kind, text).hash(&mut h);
    let mut g = SEEN.lock().unwrap_or_else(|e| e.into_inner());
    if g.get_or_insert_with(Default::default).insert(h.finish()) {
        SEEN_N[kind].fetch_add(1, Relaxed);
    }
}

pub fn distinct() -> (u64, u64) {
    (SEEN_N[0].load(Relaxed), SEEN_N[1].load(Relaxed))
}

/// `BO2MP_IW6_LOAD_SERIAL=1`: zones open one after the other.
pub fn serial() -> bool {
    static S: crate::switch::Switch = crate::switch::Switch::off("BO2MP_IW6_LOAD_SERIAL");
    S.get()
}

/// `BO2MP_IW6_PIPE_THREADS`: threads that build one technique's two pipelines (the plain and the
/// instanced form) beside each other; default 2 when the machine has two cores or more, `1` = one
/// after the other.
pub fn pipe_threads() -> usize {
    static N: crate::switch::Number = crate::switch::Number::new("BO2MP_IW6_PIPE_THREADS", 0.0);
    let cores = std::thread::available_parallelism().map_or(1, |n| n.get());
    match N.get() as usize {
        0 => cores.min(2),
        n => n,
    }
}
