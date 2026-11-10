//! GPU time per phase of a frame (opt-in: `H1_PROFILE=1` or
//! `BO2MP_H1_PROFILE=1`), from timestamp queries written between the
//! phases in the frame's own encoder, resolved at the end and read back a
//! few frames later. Needs `TIMESTAMP_QUERY` and
//! `TIMESTAMP_QUERY_INSIDE_ENCODERS` on the device; without them the
//! timer does nothing (CPU times only). The game's device may not ask for
//! those features; the offline `h1_native` device does where the adapter has
//! them.

use crate::gpu::Gpu;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

const MAX_MARKS: u32 = 16;
const RING: usize = 4;

pub fn wanted() -> bool {
    ["H1_PROFILE", "BO2MP_H1_PROFILE"]
        .iter()
        .any(|k| codlegacy_env::var(k).is_ok_and(|v| v.trim() != "0"))
        || shared_switch()
}

/// `BO2MP_GPU_TIMING=1`: the one switch that turns the per-pass GPU timing
/// table on in every renderer (MWR, BO3, IW7, Ghosts, BO2), besides each
/// game's own switch. Read once.
pub fn shared_switch() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| codlegacy_env::var("BO2MP_GPU_TIMING").is_ok_and(|v| !v.trim().is_empty() && v.trim() != "0"))
}

/// Seconds between two table lines (`BO2MP_GPU_TIMING_EVERY`, default 5).
pub fn shared_every() -> f32 {
    static EVERY: std::sync::OnceLock<f32> = std::sync::OnceLock::new();
    *EVERY.get_or_init(|| codlegacy_env::var("BO2MP_GPU_TIMING_EVERY").ok().and_then(|v| v.trim().parse().ok()).filter(|v: &f32| *v > 0.0).unwrap_or(5.0))
}

/// The per-frame counters every renderer reports beside its pass table.
/// `None`: this renderer does not have that stage (or does not count it);
/// the line prints `n/a`, never a made-up zero.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FrameCounters {
    /// Draw calls of the scene passes (instances counted).
    pub draws: Option<u64>,
    pub tris: Option<u64>,
    /// Quads of the particle / effect systems that were drawn.
    pub fx_quads: Option<u64>,
    /// Local lights that shaded something this frame.
    pub lights: Option<u64>,
    /// Draws in the shadow passes (every cascade and spot).
    pub shadow_draws: Option<u64>,
}

impl FrameCounters {
    /// The sums of `n` counted frames as the sums of `m` frames (the
    /// timings are read back a few frames after the frames they time).
    pub fn rescaled(&self, n: u32, m: u32) -> FrameCounters {
        let f = |v: Option<u64>| v.map(|v| (v as f64 * f64::from(m) / f64::from(n.max(1))).round() as u64);
        FrameCounters { draws: f(self.draws), tris: f(self.tris), fx_quads: f(self.fx_quads), lights: f(self.lights), shadow_draws: f(self.shadow_draws) }
    }

    /// Adds `o` (a frame's counters) to this sum.
    pub fn add(&mut self, o: &FrameCounters) {
        fn a(x: &mut Option<u64>, y: Option<u64>) {
            if let Some(y) = y {
                *x = Some(x.unwrap_or(0) + y);
            }
        }
        a(&mut self.draws, o.draws);
        a(&mut self.tris, o.tris);
        a(&mut self.fx_quads, o.fx_quads);
        a(&mut self.lights, o.lights);
        a(&mut self.shadow_draws, o.shadow_draws);
    }
}

/// The one log format of every game's per-pass GPU table:
/// `<game> gpu: pass ms <pass> <ms>, ..., total <ms> over <n> frames; draws
/// <n>, tris <n>, fx quads <n>, lights <n>, shadow draws <n>`. `passes`
/// are sums over `frames` frames (the mean is printed; a `total` entry is
/// printed last), `counters` the sums of the same frames. Pure, tested.
pub fn pass_line(game: &str, passes: &[(&str, f64)], frames: u32, counters: &FrameCounters) -> String {
    let n = f64::from(frames.max(1));
    let mut parts: Vec<String> = passes.iter().filter(|(l, _)| *l != "total").map(|(l, v)| format!("{l} {:.2}", v / n)).collect();
    if let Some((_, t)) = passes.iter().find(|(l, _)| *l == "total") {
        parts.push(format!("total {:.2}", t / n));
    }
    let c = |v: Option<u64>| v.map_or_else(|| "n/a".to_owned(), |v| format!("{:.0}", v as f64 / n));
    format!(
        "{game} gpu: pass ms {} over {frames} frames; draws {}, tris {}, fx quads {}, lights {}, shadow draws {}",
        if parts.is_empty() { "n/a".to_owned() } else { parts.join(", ") },
        c(counters.draws),
        c(counters.tris),
        c(counters.fx_quads),
        c(counters.lights),
        c(counters.shadow_draws)
    )
}

struct Slot {
    buffer: wgpu::Buffer,
    mapped: Arc<AtomicBool>,
    labels: Option<Vec<&'static str>>,
    asked: bool,
}

struct Stamps {
    set: wgpu::QuerySet,
    resolve: wgpu::Buffer,
    period_ns: f32,
    slots: Vec<Slot>,
    marks: Vec<&'static str>,
}

#[derive(Default)]
pub struct GpuTimer {
    stamps: Option<Stamps>,
    tried: bool,
    /// Milliseconds per label summed since the last line, and the frames.
    sums: Vec<(&'static str, f64)>,
    frames: u32,
    last_line: Option<std::time::Instant>,
    /// The counters of the frames begun since the last line.
    counters: FrameCounters,
    counted: u32,
}

/// Mean ms per frame by label from the summed values (pure, tested).
pub fn format_line(sums: &[(&'static str, f64)], frames: u32, counters: &FrameCounters) -> String {
    pass_line("mwr", sums, frames, counters)
}

impl GpuTimer {
    /// Starts the frame's marks and collects the frames read back.
    pub fn begin(&mut self, gpu: &Gpu, enc: &mut wgpu::CommandEncoder) {
        if self.stamps.is_none() && !self.tried {
            self.tried = true;
            let f = gpu.device.features();
            if f.contains(wgpu::Features::TIMESTAMP_QUERY) && f.contains(wgpu::Features::TIMESTAMP_QUERY_INSIDE_ENCODERS) {
                let size = u64::from(MAX_MARKS) * 8;
                self.stamps = Some(Stamps {
                    set: gpu.device.create_query_set(&wgpu::QuerySetDescriptor { label: Some("h1 timestamps"), ty: wgpu::QueryType::Timestamp, count: MAX_MARKS }),
                    resolve: gpu.device.create_buffer(&wgpu::BufferDescriptor { label: Some("h1 resolve"), size, usage: wgpu::BufferUsages::QUERY_RESOLVE | wgpu::BufferUsages::COPY_SRC, mapped_at_creation: false }),
                    period_ns: gpu.queue.get_timestamp_period(),
                    slots: (0..RING)
                        .map(|_| Slot {
                            buffer: gpu.device.create_buffer(&wgpu::BufferDescriptor { label: Some("h1 read"), size, usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ, mapped_at_creation: false }),
                            mapped: Arc::new(AtomicBool::new(false)),
                            labels: None,
                            asked: false,
                        })
                        .collect(),
                    marks: Vec::new(),
                });
            } else {
                eprintln!("mwr gpu timer: no timestamp queries inside encoders on this device (CPU times only)");
            }
        }
        self.collect();
        if let Some(st) = &mut self.stamps {
            st.marks.clear();
            st.marks.push("start");
            enc.write_timestamp(&st.set, 0);
        }
    }

    fn collect(&mut self) {
        let Some(st) = &mut self.stamps else { return };
        for slot in &mut st.slots {
            if slot.labels.is_some() && !slot.asked {
                slot.asked = true;
                let flag = slot.mapped.clone();
                slot.buffer.slice(..).map_async(wgpu::MapMode::Read, move |r| {
                    if r.is_ok() {
                        flag.store(true, Ordering::Release);
                    }
                });
            }
        }
        for slot in &mut st.slots {
            if !(slot.asked && slot.mapped.load(Ordering::Acquire)) {
                continue;
            }
            let labels = slot.labels.take().unwrap_or_default();
            {
                let data = slot.buffer.slice(..).get_mapped_range();
                let ticks: Vec<u64> = data
                    .chunks_exact(8)
                    .take(labels.len())
                    .map(|c| u64::from_le_bytes([c[0], c[1], c[2], c[3], c[4], c[5], c[6], c[7]]))
                    .collect();
                let ms = |a: u64, b: u64| b.saturating_sub(a) as f64 * f64::from(st.period_ns) * 1e-6;
                let add = |label: &'static str, t: f64, sums: &mut Vec<(&'static str, f64)>| match sums.iter_mut().find(|(l, _)| *l == label) {
                    Some((_, s)) => *s += t,
                    None => sums.push((label, t)),
                };
                for i in 1..ticks.len() {
                    add(labels[i], ms(ticks[i - 1], ticks[i]), &mut self.sums);
                }
                if ticks.len() > 1 {
                    add("total", ms(ticks[0], ticks[ticks.len() - 1]), &mut self.sums);
                    self.frames += 1;
                }
            }
            slot.buffer.unmap();
            slot.mapped.store(false, Ordering::Release);
            slot.asked = false;
        }
    }

    /// The GPU's work for `label` ends here.
    pub fn mark(&mut self, enc: &mut wgpu::CommandEncoder, label: &'static str) {
        let Some(st) = &mut self.stamps else { return };
        let i = st.marks.len() as u32;
        if i < MAX_MARKS {
            st.marks.push(label);
            enc.write_timestamp(&st.set, i);
        }
    }

    /// Ends the frame: resolves its marks for a later read.
    pub fn end(&mut self, enc: &mut wgpu::CommandEncoder) {
        let Some(st) = &mut self.stamps else { return };
        let n = st.marks.len() as u32;
        if let Some(slot) = st.slots.iter_mut().find(|s| s.labels.is_none()) {
            enc.resolve_query_set(&st.set, 0..n, &st.resolve, 0);
            enc.copy_buffer_to_buffer(&st.resolve, 0, &slot.buffer, 0, u64::from(n) * 8);
            slot.labels = Some(std::mem::take(&mut st.marks));
        }
    }

    /// The line (and the sums reset) when `every` seconds have passed.
    pub fn take_line(&mut self, every: f32) -> Option<String> {
        let every = if shared_switch() { shared_every() } else { every };
        let t = self.last_line.get_or_insert_with(std::time::Instant::now);
        if self.frames == 0 || t.elapsed().as_secs_f32() < every {
            return None;
        }
        let line = format_line(&self.sums, self.frames, &self.counters.rescaled(self.counted, self.frames));
        self.counters = FrameCounters::default();
        self.counted = 0;
        self.sums.clear();
        self.frames = 0;
        self.last_line = Some(std::time::Instant::now());
        Some(line)
    }

    /// Adds this frame's counters to the line's sums.
    pub fn count(&mut self, c: &FrameCounters) {
        self.counters.add(c);
        self.counted += 1;
    }

    /// Whether timestamps are in use.
    pub fn active(&self) -> bool {
        self.stamps.is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_line_gives_means_per_frame() {
        let c = FrameCounters { draws: Some(30), tris: Some(3000), shadow_draws: Some(6), ..Default::default() };
        let l = format_line(&[("opaque", 6.0), ("total", 9.0)], 3, &c);
        assert_eq!(l, "mwr gpu: pass ms opaque 2.00, total 3.00 over 3 frames; draws 10, tris 1000, fx quads n/a, lights n/a, shadow draws 2");
    }

    #[test]
    fn counters_add_only_what_is_counted() {
        let mut s = FrameCounters::default();
        s.add(&FrameCounters { draws: Some(2), ..Default::default() });
        s.add(&FrameCounters { draws: Some(3), lights: Some(1), ..Default::default() });
        assert_eq!((s.draws, s.lights, s.tris), (Some(5), Some(1), None));
    }
}
