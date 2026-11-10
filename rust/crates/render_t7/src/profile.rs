//! `BO2MP_T7_PROFILE=1`: what a frame of the native renderer costs on the
//! CPU and what it hands the GPU, as one `t7 profile:` line every few
//! seconds (the mean per frame over the frames since the last line):
//!
//! * CPU ms per phase (the same phases as the frame timing's `CPU
//!   recording` part: constants, cull, each pass's encoding, post), and the
//!   frame's whole recording time;
//! * draws (direct, indirect and fullscreen; a G-buffer bundle counts the
//!   draws it holds), compute dispatches, render and compute passes,
//!   pipeline and bind group switches;
//! * bytes written to buffers and textures through the queue;
//! * the draws left after the G-buffer's GPU culling, counted on the CPU
//!   with the same spheres and planes (once per line, not per frame).
//!
//! The GPU side per pass is the frame timing's `frame ms (GPU ...)` line
//! (`crate::timing`, needs timestamp queries inside encoders).
//!
//! Off (the default) every counter is one relaxed atomic load and a branch.
//! The counted calls go through [`PassExt`], [`ComputeExt`], [`EncExt`] and
//! [`QueueExt`]: the same wgpu calls with a count beside them.

use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

static ON: AtomicBool = AtomicBool::new(false);
static INIT: std::sync::Once = std::sync::Once::new();

/// Whether the profile is on (`BO2MP_T7_PROFILE=1`, or [`force`]).
#[inline]
pub fn on() -> bool {
    INIT.call_once(|| {
        if std::env::var("BO2MP_T7_PROFILE").is_ok_and(|v| !v.is_empty() && v != "0") {
            ON.store(true, Ordering::Relaxed);
        }
    });
    ON.load(Ordering::Relaxed)
}

/// Turns the profile on from code (the offline tools' `--profile`).
pub fn force(v: bool) {
    INIT.call_once(|| {});
    ON.store(v, Ordering::Relaxed);
}

#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
pub struct Counts {
    pub draws: u64,
    pub dispatches: u64,
    pub passes: u64,
    pub pipelines: u64,
    pub binds: u64,
    pub bytes: u64,
}

impl Counts {
    pub fn minus(self, o: Counts) -> Counts {
        Counts {
            draws: self.draws.saturating_sub(o.draws),
            dispatches: self.dispatches.saturating_sub(o.dispatches),
            passes: self.passes.saturating_sub(o.passes),
            pipelines: self.pipelines.saturating_sub(o.pipelines),
            binds: self.binds.saturating_sub(o.binds),
            bytes: self.bytes.saturating_sub(o.bytes),
        }
    }
}

static DRAWS: AtomicU64 = AtomicU64::new(0);
static DISPATCHES: AtomicU64 = AtomicU64::new(0);
static PASSES: AtomicU64 = AtomicU64::new(0);
static PIPELINES: AtomicU64 = AtomicU64::new(0);
static BINDS: AtomicU64 = AtomicU64::new(0);
static BYTES: AtomicU64 = AtomicU64::new(0);

#[inline]
fn add(c: &AtomicU64, n: u64) {
    if on() || crate::timing::counting() {
        c.fetch_add(n, Ordering::Relaxed);
    }
}

pub fn draws(n: u64) {
    add(&DRAWS, n);
}
pub fn dispatch() {
    add(&DISPATCHES, 1);
}
pub fn pass() {
    add(&PASSES, 1);
}
pub fn pipeline() {
    add(&PIPELINES, 1);
}
pub fn bind() {
    add(&BINDS, 1);
}
pub fn bytes(n: u64) {
    add(&BYTES, n);
}

/// The counters as they stand (a bundle's own counts are the difference
/// before and after recording it).
pub fn now() -> Counts {
    Counts {
        draws: DRAWS.load(Ordering::Relaxed),
        dispatches: DISPATCHES.load(Ordering::Relaxed),
        passes: PASSES.load(Ordering::Relaxed),
        pipelines: PIPELINES.load(Ordering::Relaxed),
        binds: BINDS.load(Ordering::Relaxed),
        bytes: BYTES.load(Ordering::Relaxed),
    }
}

thread_local! {
    static BUNDLE: std::cell::Cell<Counts> = const { std::cell::Cell::new(Counts { draws: 0, dispatches: 0, passes: 0, pipelines: 0, binds: 0, bytes: 0 }) };
}

/// Counts a call recorded into a render bundle (on this thread).
pub fn bundle(f: impl FnOnce(&mut Counts)) {
    BUNDLE.with(|b| {
        let mut c = b.get();
        f(&mut c);
        b.set(c);
    });
}

/// This thread's bundle counts since the last call (zeroed).
pub fn bundle_take() -> Counts {
    BUNDLE.with(|b| b.replace(Counts::default()))
}

/// A replayed bundle's counts, added each time it is executed.
pub fn replay(c: &Counts) {
    if on() {
        DRAWS.fetch_add(c.draws, Ordering::Relaxed);
        PIPELINES.fetch_add(c.pipelines, Ordering::Relaxed);
        BINDS.fetch_add(c.binds, Ordering::Relaxed);
    }
}

struct Window {
    start: Option<std::time::Instant>,
    frames: u64,
    at_start: Counts,
    cpu: Vec<(&'static str, f64)>,
    record_ms: f64,
    submit_ms: f64,
    submits: u64,
    worst_record: f64,
    /// Load-time facts for the line (`t7 fps:` summary inputs).
    culled: Option<(usize, usize, u64, u64)>,
}

static WINDOW: Mutex<Window> = Mutex::new(Window {
    start: None,
    frames: 0,
    at_start: Counts {
        draws: 0,
        dispatches: 0,
        passes: 0,
        pipelines: 0,
        binds: 0,
        bytes: 0,
    },
    cpu: Vec::new(),
    record_ms: 0.0,
    submit_ms: 0.0,
    submits: 0,
    worst_record: 0.0,
    culled: None,
});

/// A CPU phase's time this frame (`crate::timing::cpu` passes it on).
pub fn cpu(label: &'static str, ms: f64) {
    if !on() {
        return;
    }
    if let Ok(mut w) = WINDOW.lock() {
        match w.cpu.iter_mut().find(|(l, _)| *l == label) {
            Some((_, t)) => *t += ms,
            None => w.cpu.push((label, ms)),
        }
    }
}

/// The time the owner spent submitting the frame's command buffers (the
/// offline tools; in the game the engine submits).
pub fn submit(ms: f64) {
    if !on() {
        return;
    }
    if let Ok(mut w) = WINDOW.lock() {
        w.submit_ms += ms;
        w.submits += 1;
    }
}

/// The G-buffer draws left after culling this view, of the ready ones
/// (counted on the CPU when a line is due).
pub fn set_culled(c: (usize, usize, u64, u64)) {
    if let Ok(mut w) = WINDOW.lock() {
        w.culled = Some(c);
    }
}

/// Whether a line is due at the end of this frame (the culled count is
/// then worth working out).
pub fn line_due() -> bool {
    on() && WINDOW.lock().is_ok_and(|w| {
        w.start
            .is_some_and(|s| s.elapsed().as_secs_f32() >= every())
    })
}

fn every() -> f32 {
    static EVERY: std::sync::OnceLock<f32> = std::sync::OnceLock::new();
    *EVERY.get_or_init(|| {
        std::env::var("T7_PROFILE_EVERY")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(5.0)
    })
}

/// Ends a frame (`record_ms`: the whole recording on the CPU). Returns the
/// line when one is due.
pub fn end_frame(record_ms: f64) -> Option<String> {
    if !on() {
        return None;
    }
    let mut w = WINDOW.lock().ok()?;
    let Some(start) = w.start else {
        // (The first frame binds and sizes everything: not counted.)
        w.start = Some(std::time::Instant::now());
        w.at_start = now();
        w.cpu.clear();
        w.submit_ms = 0.0;
        w.submits = 0;
        return None;
    };
    w.frames += 1;
    w.record_ms += record_ms;
    w.worst_record = w.worst_record.max(record_ms);
    if start.elapsed().as_secs_f32() < every() {
        return None;
    }
    let n = w.frames.max(1) as f64;
    let c = now().minus(w.at_start);
    let per = |v: u64| v as f64 / n;
    let phases: Vec<String> = w
        .cpu
        .iter()
        .map(|(l, t)| format!("{l} {:.3}", t / n))
        .collect();
    let submit = if w.submits > 0 {
        format!(", submit {:.3}", w.submit_ms / w.submits as f64)
    } else {
        String::new()
    };
    let culled = w.culled.map_or(String::new(), |(l, of, il, iof)| {
        format!("; G-buffer after culling: {l} of {of} draws, {il} of {iof} instances")
    });
    let line = format!(
        "t7 profile: {} frames over {:.1}s: CPU ms/frame: record {:.3} (worst {:.3}){submit}; phases: {}; per frame: draws {:.0}, dispatches {:.0}, passes {:.0}, pipeline switches {:.0}, bind group switches {:.0}, uploaded {:.1} KB{culled}",
        w.frames,
        start.elapsed().as_secs_f32(),
        w.record_ms / n,
        w.worst_record,
        phases.join(", "),
        per(c.draws),
        per(c.dispatches),
        per(c.passes),
        per(c.pipelines),
        per(c.binds),
        per(c.bytes) / 1024.0,
    );
    w.start = Some(std::time::Instant::now());
    w.at_start = now();
    w.frames = 0;
    w.cpu.clear();
    w.record_ms = 0.0;
    w.worst_record = 0.0;
    w.submit_ms = 0.0;
    w.submits = 0;
    Some(line)
}

/// Counted render pass calls.
pub trait PassExt {
    fn set_pipeline_p(&mut self, p: &wgpu::RenderPipeline);
    fn set_bind_group_p<'b>(
        &mut self,
        i: u32,
        g: impl Into<Option<&'b wgpu::BindGroup>>,
        offsets: &[u32],
    );
    fn draw_p(&mut self, v: std::ops::Range<u32>, i: std::ops::Range<u32>);
    fn draw_indexed_p(&mut self, idx: std::ops::Range<u32>, base: i32, i: std::ops::Range<u32>);
}

impl PassExt for wgpu::RenderPass<'_> {
    #[inline]
    fn set_pipeline_p(&mut self, p: &wgpu::RenderPipeline) {
        pipeline();
        self.set_pipeline(p);
    }
    #[inline]
    fn set_bind_group_p<'b>(
        &mut self,
        i: u32,
        g: impl Into<Option<&'b wgpu::BindGroup>>,
        offsets: &[u32],
    ) {
        bind();
        let g: Option<&wgpu::BindGroup> = g.into();
        self.set_bind_group(i, g, offsets);
    }
    #[inline]
    fn draw_p(&mut self, v: std::ops::Range<u32>, i: std::ops::Range<u32>) {
        draws(1);
        self.draw(v, i);
    }
    #[inline]
    fn draw_indexed_p(&mut self, idx: std::ops::Range<u32>, base: i32, i: std::ops::Range<u32>) {
        draws(1);
        self.draw_indexed(idx, base, i);
    }
}

/// Counted compute pass calls.
pub trait ComputeExt {
    fn set_pipeline_p(&mut self, p: &wgpu::ComputePipeline);
    fn set_bind_group_p<'b>(
        &mut self,
        i: u32,
        g: impl Into<Option<&'b wgpu::BindGroup>>,
        offsets: &[u32],
    );
    fn dispatch_p(&mut self, x: u32, y: u32, z: u32);
}

impl ComputeExt for wgpu::ComputePass<'_> {
    #[inline]
    fn set_pipeline_p(&mut self, p: &wgpu::ComputePipeline) {
        pipeline();
        self.set_pipeline(p);
    }
    #[inline]
    fn set_bind_group_p<'b>(
        &mut self,
        i: u32,
        g: impl Into<Option<&'b wgpu::BindGroup>>,
        offsets: &[u32],
    ) {
        bind();
        let g: Option<&wgpu::BindGroup> = g.into();
        self.set_bind_group(i, g, offsets);
    }
    #[inline]
    fn dispatch_p(&mut self, x: u32, y: u32, z: u32) {
        dispatch();
        self.dispatch_workgroups(x, y, z);
    }
}

/// Counted passes.
pub trait EncExt {
    fn render_pass_p<'e>(&'e mut self, d: &wgpu::RenderPassDescriptor<'_>) -> wgpu::RenderPass<'e>;
    fn compute_pass_p<'e>(
        &'e mut self,
        d: &wgpu::ComputePassDescriptor<'_>,
    ) -> wgpu::ComputePass<'e>;
}

impl EncExt for wgpu::CommandEncoder {
    #[inline]
    fn render_pass_p<'e>(&'e mut self, d: &wgpu::RenderPassDescriptor<'_>) -> wgpu::RenderPass<'e> {
        pass();
        self.begin_render_pass(d)
    }
    #[inline]
    fn compute_pass_p<'e>(
        &'e mut self,
        d: &wgpu::ComputePassDescriptor<'_>,
    ) -> wgpu::ComputePass<'e> {
        pass();
        self.begin_compute_pass(d)
    }
}

/// Counted queue writes.
pub trait QueueExt {
    fn write_buffer_p(&self, b: &wgpu::Buffer, offset: u64, data: &[u8]);
    fn write_texture_p(
        &self,
        t: wgpu::TexelCopyTextureInfo<'_>,
        data: &[u8],
        layout: wgpu::TexelCopyBufferLayout,
        size: wgpu::Extent3d,
    );
}

impl QueueExt for wgpu::Queue {
    #[inline]
    fn write_buffer_p(&self, b: &wgpu::Buffer, offset: u64, data: &[u8]) {
        bytes(data.len() as u64);
        self.write_buffer(b, offset, data);
    }
    #[inline]
    fn write_texture_p(
        &self,
        t: wgpu::TexelCopyTextureInfo<'_>,
        data: &[u8],
        layout: wgpu::TexelCopyBufferLayout,
        size: wgpu::Extent3d,
    ) {
        bytes(data.len() as u64);
        self.write_texture(t, data, layout, size);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_subtract_without_wrapping() {
        let a = Counts {
            draws: 5,
            dispatches: 1,
            passes: 3,
            pipelines: 2,
            binds: 9,
            bytes: 100,
        };
        let b = Counts {
            draws: 7,
            ..Default::default()
        };
        assert_eq!(a.minus(b).draws, 0);
        assert_eq!(a.minus(b).binds, 9);
    }
}
