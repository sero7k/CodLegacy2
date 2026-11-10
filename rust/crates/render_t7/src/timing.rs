//! Per-pass frame timing: GPU timestamps written between the frame's
//! stages, read back a few frames later, and a log line every few seconds
//! with the mean of each stage (`t7 native: frame ms: gbuffer x, lighting y,
//! ...`), so a PC run shows where the time goes. Off when the device has no
//! timestamp queries inside encoders (or `T7_NO_TIMING=1`).

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

const MAX_MARKS: u32 = 48;
const RING: usize = 4;

struct Slot {
    buffer: wgpu::Buffer,
    /// Set by the map callback when the data can be read.
    mapped: Arc<AtomicBool>,
    /// The labels of the marks written into it (None: free).
    labels: Option<Vec<&'static str>>,
    asked: bool,
}

struct Stamps {
    set: wgpu::QuerySet,
    resolve: wgpu::Buffer,
    period_ns: f32,
    slots: Vec<Slot>,
    /// This frame's marks.
    marks: Vec<&'static str>,
    sums: Vec<(&'static str, f64)>,
    frames: u32,
    cpu_ms: f64,
    last_log: std::time::Instant,
    first_log: bool,
    /// Frames begun since the last line (all of them, read back or not).
    begun: u32,
    /// The slowest frame's GPU total since the last line, ms.
    worst: f64,
    /// The counters of the frames ended since the last line.
    counters: render_h1::gpu_timer::FrameCounters,
    counted: u32,
    /// The profile's draw count when this frame began.
    draws_at_begin: u64,
}

static STAMPS: Mutex<Option<Stamps>> = Mutex::new(None);
static ACTIVE: AtomicBool = AtomicBool::new(false);
/// The table is being made (the profile's draw counters then run too).
static COUNTING: AtomicBool = AtomicBool::new(false);

/// Whether the per-frame counters of the timing table are counted.
#[inline]
pub fn counting() -> bool {
    COUNTING.load(Ordering::Relaxed)
}

// The counters the profile does not have, summed over the frame in progress
// (taken at its end by `end`).
static LIGHTS: AtomicU64 = AtomicU64::new(0);
static FX_QUADS: AtomicU64 = AtomicU64::new(0);
static SHADOW_DRAWS: AtomicU64 = AtomicU64::new(0);

/// Lights packed for this frame's view.
pub fn count_lights(n: u64) {
    if counting() {
        LIGHTS.store(n, Ordering::Relaxed);
    }
}
/// Effect quads (sprites) drawn this frame.
pub fn count_fx_quads(n: u64) {
    if counting() {
        FX_QUADS.store(n, Ordering::Relaxed);
    }
}
/// Draws in the shadow passes.
pub fn count_shadow_draws(n: u64) {
    if counting() {
        SHADOW_DRAWS.fetch_add(n, Ordering::Relaxed);
    }
}

/// Whether marks are being written this frame.
pub fn active() -> bool {
    ACTIVE.load(Ordering::Relaxed)
}

/// On unless `T7_NO_TIMING=1` (or `BO2MP_GPU_TIMING=0`, the shared switch
/// of every renderer's table).
fn wanted() -> bool {
    !std::env::var("T7_NO_TIMING").is_ok_and(|v| v != "0") && !std::env::var("BO2MP_GPU_TIMING").is_ok_and(|v| v.trim() == "0")
}

/// Starts a frame. Makes the query set the first time (None if the device
/// cannot).
pub fn begin(gpu: &render_h1::gpu::Gpu, enc: &mut wgpu::CommandEncoder) {
    let Ok(mut g) = STAMPS.lock() else { return };
    if g.is_none() {
        let f = gpu.device.features();
        if !wanted() || !f.contains(wgpu::Features::TIMESTAMP_QUERY) || !f.contains(wgpu::Features::TIMESTAMP_QUERY_INSIDE_ENCODERS) {
            ACTIVE.store(false, Ordering::Relaxed);
            *g = None;
            // (Remembered by a set that is never made: try once.)
            static TRIED: AtomicBool = AtomicBool::new(false);
            if !TRIED.swap(true, Ordering::Relaxed) {
                drop(g);
                push_line(format!("frame timing: not available on this device (timestamp queries inside encoders: {})", f.contains(wgpu::Features::TIMESTAMP_QUERY_INSIDE_ENCODERS)));
            }
            return;
        }
        let set = gpu.device.create_query_set(&wgpu::QuerySetDescriptor { label: Some("t7 frame timestamps"), ty: wgpu::QueryType::Timestamp, count: MAX_MARKS });
        let size = u64::from(MAX_MARKS) * 8;
        let resolve = gpu.device.create_buffer(&wgpu::BufferDescriptor { label: Some("t7 timestamps resolve"), size, usage: wgpu::BufferUsages::QUERY_RESOLVE | wgpu::BufferUsages::COPY_SRC, mapped_at_creation: false });
        let slots = (0..RING)
            .map(|_| Slot {
                buffer: gpu.device.create_buffer(&wgpu::BufferDescriptor { label: Some("t7 timestamps read"), size, usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ, mapped_at_creation: false }),
                mapped: Arc::new(AtomicBool::new(false)),
                labels: None,
                asked: false,
            })
            .collect();
        *g = Some(Stamps {
            set,
            resolve,
            period_ns: gpu.queue.get_timestamp_period(),
            slots,
            marks: Vec::new(),
            sums: Vec::new(),
            frames: 0,
            cpu_ms: 0.0,
            last_log: std::time::Instant::now(),
            first_log: true,
            begun: 0,
            worst: 0.0,
            counters: Default::default(),
            counted: 0,
            draws_at_begin: 0,
        });
        COUNTING.store(true, Ordering::Relaxed);
    }
    let Some(st) = g.as_mut() else { return };
    // Frames recorded earlier have been submitted by now: ask for their
    // data (not before: a buffer mapped under a pending copy is an error).
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
    // Collect the frames whose data came back.
    for slot in &mut st.slots {
        if slot.asked && slot.mapped.load(Ordering::Acquire) {
            let labels = slot.labels.take().unwrap_or_default();
            {
                let data = slot.buffer.slice(..).get_mapped_range();
                let ticks: Vec<u64> = data.chunks_exact(8).take(labels.len()).map(|c| u64::from_le_bytes(c.try_into().unwrap())).collect();
                for i in 1..ticks.len() {
                    let ms = ticks[i].saturating_sub(ticks[i - 1]) as f64 * f64::from(st.period_ns) * 1e-6;
                    match st.sums.iter_mut().find(|(l, _)| *l == labels[i]) {
                        Some((_, t)) => *t += ms,
                        None => st.sums.push((labels[i], ms)),
                    }
                }
                if ticks.len() > 1 {
                    let total = ticks[ticks.len() - 1].saturating_sub(ticks[0]) as f64 * f64::from(st.period_ns) * 1e-6;
                    match st.sums.iter_mut().find(|(l, _)| *l == "total") {
                        Some((_, t)) => *t += total,
                        None => st.sums.push(("total", total)),
                    }
                    st.worst = st.worst.max(total);
                    st.frames += 1;
                }
            }
            slot.buffer.unmap();
            slot.mapped.store(false, Ordering::Release);
            slot.asked = false;
        }
    }
    st.marks.clear();
    st.begun += 1;
    st.draws_at_begin = crate::profile::now().draws;
    LIGHTS.store(0, Ordering::Relaxed);
    FX_QUADS.store(0, Ordering::Relaxed);
    SHADOW_DRAWS.store(0, Ordering::Relaxed);
    ACTIVE.store(true, Ordering::Relaxed);
    st.marks.push("start");
    enc.write_timestamp(&st.set, 0);
}

/// A stage ends here: the time since the last mark is `label`'s.
pub fn mark(enc: &mut wgpu::CommandEncoder, label: &'static str) {
    if !active() {
        return;
    }
    let Ok(mut g) = STAMPS.lock() else { return };
    let Some(st) = g.as_mut() else { return };
    let i = st.marks.len() as u32;
    if i >= MAX_MARKS {
        return;
    }
    st.marks.push(label);
    enc.write_timestamp(&st.set, i);
}

/// Ends the frame's marks: resolves them for reading. `cpu_ms`: the time the
/// CPU spent recording the frame.
pub fn end(enc: &mut wgpu::CommandEncoder, cpu_ms: f64) {
    if !active() {
        return;
    }
    let Ok(mut g) = STAMPS.lock() else { return };
    let Some(st) = g.as_mut() else { return };
    st.cpu_ms += cpu_ms;
    // (Draw calls of every pass, the full-screen ones too; the G-buffer's
    // GPU-culled indirect draws count as the calls they are, so no triangle
    // count: it would be the uncull'd one.)
    let shadow = SHADOW_DRAWS.load(Ordering::Relaxed);
    let draws = crate::profile::now().draws.saturating_sub(st.draws_at_begin).saturating_sub(shadow);
    st.counters.add(&render_h1::gpu_timer::FrameCounters { draws: Some(draws), tris: None, fx_quads: Some(FX_QUADS.load(Ordering::Relaxed)), lights: Some(LIGHTS.load(Ordering::Relaxed)), shadow_draws: Some(shadow) });
    st.counted += 1;
    let n = st.marks.len() as u32;
    // A free slot, else this frame is not read.
    if let Some(slot) = st.slots.iter_mut().find(|s| s.labels.is_none()) {
        enc.resolve_query_set(&st.set, 0..n, &st.resolve, 0);
        enc.copy_buffer_to_buffer(&st.resolve, 0, &slot.buffer, 0, u64::from(n) * 8);
        slot.labels = Some(std::mem::take(&mut st.marks));
        // (Asked for at the next frame's start, after the submit.)
    }
    ACTIVE.store(false, Ordering::Relaxed);
}

static CPU: Mutex<Vec<(&'static str, f64)>> = Mutex::new(Vec::new());

/// Adds `since.elapsed()` to the CPU time of `label` (the stages' recording
/// on the CPU, apart from the GPU's).
pub fn cpu(label: &'static str, since: std::time::Instant) {
    let ms = since.elapsed().as_secs_f64() * 1000.0;
    crate::profile::cpu(label, ms);
    if let Ok(mut c) = CPU.lock() {
        match c.iter_mut().find(|(l, _)| *l == label) {
            Some((_, t)) => *t += ms,
            None => c.push((label, ms)),
        }
    }
}

/// The log line when a few seconds have passed since the last: the mean of
/// each stage over the frames read back, the CPU recording time, the frame
/// rate.
pub fn take_line(frames_total: u64) -> Option<String> {
    let mut g = STAMPS.lock().ok()?;
    let st = g.as_mut()?;
    // (`T7_TIMING_EVERY=seconds`: another interval, offline checks.)
    static EVERY: std::sync::OnceLock<Option<f32>> = std::sync::OnceLock::new();
    let every = EVERY.get_or_init(|| std::env::var("T7_TIMING_EVERY").ok().and_then(|v| v.parse().ok())).unwrap_or(if render_h1::gpu_timer::shared_switch() { render_h1::gpu_timer::shared_every() } else if st.first_log { 3.0 } else { 5.0 });
    if st.last_log.elapsed().as_secs_f32() < every || st.frames == 0 {
        return None;
    }
    let secs = st.last_log.elapsed().as_secs_f32();
    let n = f64::from(st.frames);
    let total = st.sums.iter().find(|(l, _)| *l == "total").map_or(0.0, |(_, t)| t / n);
    let cpu: Vec<String> = CPU.lock().map(|mut c| std::mem::take(&mut *c)).unwrap_or_default().into_iter().map(|(l, t)| format!("{l} {:.2}", t / n)).collect();
    // The wall time a frame took against the GPU's own: GPU total near the
    // wall time is a GPU-bound frame; every pass slower than usual with the
    // same work (a fixed-size pass such as `post lut`) is a GPU shared with
    // something else (capture, overlay, another game).
    let wall = f64::from(secs) * 1000.0 / f64::from(st.begun.max(1));
    let line = render_h1::gpu_timer::pass_line("t7", &st.sums, st.frames, &st.counters.rescaled(st.counted, st.frames));
    push_line(format!(
        "t7 gpu: frame (mean of {} frames over {secs:.1}s): worst {:.2} ms; wall {wall:.2} ms/frame ({:.0} fps), GPU {total:.2} ms = {:.0}% of it; CPU recording {:.2} ms/frame ({}); {} frames so far",
        st.frames,
        st.worst,
        1000.0 / wall.max(1e-3),
        100.0 * total / wall.max(1e-3),
        st.cpu_ms / n,
        cpu.join(", "),
        frames_total
    ));
    st.counters = Default::default();
    st.counted = 0;
    st.begun = 0;
    st.worst = 0.0;
    st.sums.clear();
    st.frames = 0;
    st.cpu_ms = 0.0;
    st.last_log = std::time::Instant::now();
    st.first_log = false;
    Some(line)
}

fn push_line(l: String) {
    if let Ok(mut g) = LINES.lock() {
        g.push(l);
    }
}

static LINES: Mutex<Vec<String>> = Mutex::new(Vec::new());

/// Lines about the timing itself (not available, ...), since the last call.
pub fn take_notes() -> Vec<String> {
    LINES.lock().map(|mut l| std::mem::take(&mut *l)).unwrap_or_default()
}
