//! Per-frame draw counters of the Black Ops II / IW4-port paths and the
//! one-line GPU pass table they go with (`BO2MP_GPU_TIMING=1`).
//!
//! The recording code adds to a few relaxed atomics (one load when the
//! table is off); the main-world frame system takes them once per frame
//! and hands them, with the GPU span times bevy's render diagnostics
//! deliver a few frames later, to `render_h1::gpu_timer::pass_line`, the
//! log format every renderer shares (`<game> gpu: pass ms ...`).

use std::sync::atomic::{AtomicU64, Ordering};

use render_h1::gpu_timer::FrameCounters;

static DRAWS: AtomicU64 = AtomicU64::new(0);
static TRIS: AtomicU64 = AtomicU64::new(0);
static SHADOW_DRAWS: AtomicU64 = AtomicU64::new(0);
static LIGHTS: AtomicU64 = AtomicU64::new(0);

/// Whether the counters are wanted (`BO2MP_GPU_TIMING`, read once).
#[inline]
pub fn on() -> bool {
    render_h1::gpu_timer::shared_switch()
}

/// One scene draw of `index_count` indices over `instances` instances.
#[inline]
pub fn draw(index_count: u32, instances: u32) {
    if on() {
        DRAWS.fetch_add(1, Ordering::Relaxed);
        TRIS.fetch_add(u64::from(index_count / 3) * u64::from(instances.max(1)), Ordering::Relaxed);
    }
}

/// One draw of a shadow pass.
#[inline]
pub fn shadow_draw() {
    if on() {
        SHADOW_DRAWS.fetch_add(1, Ordering::Relaxed);
    }
}

/// The lights (spot shadow slots) the frame has.
#[inline]
pub fn lights(n: u32) {
    if on() {
        LIGHTS.store(u64::from(n), Ordering::Relaxed);
    }
}

/// The frame's counters, and the counters reset. `fx_quads` is `None`:
/// Black Ops II's effects are drawn by the dynamic lists, which do not
/// separate their quads.
pub fn take() -> FrameCounters {
    FrameCounters {
        draws: Some(DRAWS.swap(0, Ordering::Relaxed)),
        tris: Some(TRIS.swap(0, Ordering::Relaxed)),
        fx_quads: None,
        lights: Some(LIGHTS.swap(0, Ordering::Relaxed)),
        shadow_draws: Some(SHADOW_DRAWS.swap(0, Ordering::Relaxed)),
    }
}

static TABLE: std::sync::Mutex<Option<Table>> = std::sync::Mutex::new(None);

/// One main frame of the table: its counters, the GPU batch when a fresh
/// one was delivered, and the table's line when it is due.
pub fn frame(batch: Option<(&[(&'static str, Option<f32>)], Option<f32>)>) -> Option<String> {
    if !on() {
        return None;
    }
    let c = take();
    let mut g = TABLE.lock().ok()?;
    let t = g.get_or_insert_with(Table::default);
    t.count(&c);
    if let Some((spans, total)) = batch {
        t.passes(spans, total);
    }
    t.take_line()
}

/// The sums of the table since its last line.
#[derive(Default)]
pub struct Table {
    sums: Vec<(&'static str, f64)>,
    frames: u32,
    counters: FrameCounters,
    counted: u32,
    last: Option<std::time::Instant>,
}

impl Table {
    /// Adds one main frame's counters.
    pub fn count(&mut self, c: &FrameCounters) {
        self.counters.add(c);
        self.counted += 1;
    }

    /// Adds one delivered GPU batch (the passes that have a time, ms).
    pub fn passes(&mut self, spans: &[(&'static str, Option<f32>)], total: Option<f32>) {
        let mut any = false;
        for (label, ms) in spans {
            if let Some(ms) = ms {
                any = true;
                match self.sums.iter_mut().find(|(l, _)| l == label) {
                    Some((_, s)) => *s += f64::from(*ms),
                    None => self.sums.push((label, f64::from(*ms))),
                }
            }
        }
        if let Some(t) = total {
            any = true;
            match self.sums.iter_mut().find(|(l, _)| *l == "total") {
                Some((_, s)) => *s += f64::from(t),
                None => self.sums.push(("total", f64::from(t))),
            }
        }
        if any {
            self.frames += 1;
        }
    }

    /// The line when `every` seconds have passed and a batch has arrived.
    pub fn take_line(&mut self) -> Option<String> {
        let t = self.last.get_or_insert_with(std::time::Instant::now);
        if self.frames == 0 || t.elapsed().as_secs_f32() < render_h1::gpu_timer::shared_every() {
            return None;
        }
        let line = render_h1::gpu_timer::pass_line("bo2", &self.sums, self.frames, &self.counters.rescaled(self.counted, self.frames));
        *self = Table { last: Some(std::time::Instant::now()), ..Table::default() };
        Some(line)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_table_prints_the_shared_format() {
        let mut t = Table::default();
        t.passes(&[("colour", Some(2.0)), ("sun", None)], Some(3.0));
        t.passes(&[("colour", Some(4.0)), ("sun", Some(1.0))], Some(5.0));
        t.count(&FrameCounters { draws: Some(10), tris: Some(100), shadow_draws: Some(4), lights: Some(1), fx_quads: None });
        t.count(&FrameCounters { draws: Some(30), tris: Some(300), shadow_draws: Some(6), lights: Some(1), fx_quads: None });
        t.last = Some(std::time::Instant::now() - std::time::Duration::from_secs(60));
        let l = t.take_line().expect("a line");
        assert_eq!(l, "bo2 gpu: pass ms colour 3.00, sun 0.50, total 4.00 over 2 frames; draws 20, tris 200, fx quads n/a, lights 1, shadow draws 5");
        assert!(t.take_line().is_none());
    }
}
