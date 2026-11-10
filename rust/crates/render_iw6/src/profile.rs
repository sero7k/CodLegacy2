//! The opt-in frame profile (`BO2MP_IW6_PROFILE=1`): per phase of a frame
//! (each shadow view, the opaque and blended scene, the post) the CPU time
//! split into preparing (first use only), the constants written, and the
//! commands encoded, with the draws, the draws culled, the state changes and
//! the bytes uploaded; and the GPU's time per phase from timestamp queries
//! where the device has them (written between passes, read back a few frames
//! later). Offline (`iw6_shot`) a line per frame; in game one every few
//! seconds with the means.

use std::sync::Arc;
use std::sync::atomic::{AtomicU8, Ordering};

/// `BO2MP_IW6_PROFILE=1`: the profile is kept and printed.
pub fn on() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var("BO2MP_IW6_PROFILE").is_ok_and(|v| matches!(v.trim(), "1" | "2")) || render_h1::gpu_timer::shared_switch())
}

/// `BO2MP_IW6_PROFILE=2`: the long one-line form with every counter of
/// every phase, besides the table.
pub fn verbose() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var("BO2MP_IW6_PROFILE").is_ok_and(|v| v.trim() == "2"))
}

/// What one `Renderer::record` (or a post step) did.
#[derive(Clone, Copy, Debug, Default)]
pub struct Counts {
    pub passes: u32,
    pub draws: u32,
    pub culled: u32,
    /// Of those, culled by the cells and portals.
    pub portal_culled: u32,
    pub pipelines: u32,
    pub groups: [u32; 3],
    pub vertex_buffers: u32,
    pub index_buffers: u32,
    pub bytes: u64,
    /// Draw calls issued (visible draws less those merged into a run).
    pub calls: u32,
    pub merged: u32,
    /// Draws that went into a multi-draw buffer.
    pub multi_records: u32,
    /// Draw calls that repeat the previous call but for its per-object
    /// block (same pipeline, groups, mesh and index range): what
    /// instancing would fold into the previous call.
    pub instanceable: u32,
    /// Draws folded into an instanced call (`instancing`).
    pub instanced: u32,
    /// Submissions of their own (outside the frame's encoder).
    pub submits: u32,
    /// Triangles drawn (instances counted).
    pub tris: u64,
    /// Draws left out because the occlusion buffer hides them.
    pub occluded: u32,
    /// Draws left out because they span too few pixels.
    pub small: u32,
    /// Of the entries drawn: those of the depth pre-pass and their
    /// triangles; sky; decals; placed models and their triangles; draws
    /// lit by a spot or omni light (their own technique).
    pub prepass: u32,
    pub prepass_tris: u64,
    pub sky: u32,
    pub decals: u32,
    pub models: u32,
    pub model_tris: u64,
    pub local_lit: u32,
}

impl std::ops::AddAssign for Counts {
    fn add_assign(&mut self, o: Self) {
        self.passes += o.passes;
        self.draws += o.draws;
        self.culled += o.culled;
        self.portal_culled += o.portal_culled;
        self.tris += o.tris;
        self.occluded += o.occluded;
        self.small += o.small;
        self.prepass += o.prepass;
        self.prepass_tris += o.prepass_tris;
        self.sky += o.sky;
        self.decals += o.decals;
        self.models += o.models;
        self.model_tris += o.model_tris;
        self.local_lit += o.local_lit;
        self.instanceable += o.instanceable;
        self.multi_records += o.multi_records;
        self.instanced += o.instanced;
        self.pipelines += o.pipelines;
        for k in 0..3 {
            self.groups[k] += o.groups[k];
        }
        self.vertex_buffers += o.vertex_buffers;
        self.index_buffers += o.index_buffers;
        self.bytes += o.bytes;
        self.calls += o.calls;
        self.merged += o.merged;
        self.submits += o.submits;
    }
}

/// CPU milliseconds of one phase, split, and what it did.
#[derive(Clone, Copy, Debug, Default)]
pub struct Phase {
    pub total: f64,
    pub prepare: f64,
    pub constants: f64,
    pub encode: f64,
    /// Of `encode`, the culling (which entries a view sees).
    pub cull: f64,
    pub counts: Counts,
}

impl std::ops::AddAssign for Phase {
    fn add_assign(&mut self, o: Self) {
        self.total += o.total;
        self.prepare += o.prepare;
        self.constants += o.constants;
        self.encode += o.encode;
        self.cull += o.cull;
        self.counts += o.counts;
    }
}

const MAX_MARKS: u32 = 32;
/// Readback buffers: enough for the frames in flight plus the map's delay.
const RING: usize = 8;
/// `Slot::mapped`: waiting, mapped, the map failed (the slot is let go).
const WAITING: u8 = 0;
const MAPPED: u8 = 1;
const FAILED: u8 = 2;

/// One readback buffer of the ring. Its life: free (`labels` None) ->
/// written by a frame's encoder (`labels`, `written` that frame) -> asked
/// (`map_async`, only once a later frame has begun, so the encoder that
/// copies into it has been submitted: a buffer waiting for its map must not
/// be in a submission) -> mapped (the callback) -> read and unmapped -> free.
struct Slot {
    buffer: wgpu::Buffer,
    mapped: Arc<AtomicU8>,
    labels: Option<Vec<&'static str>>,
    written: u64,
    asked: bool,
}

struct Stamps {
    device: wgpu::Device,
    set: wgpu::QuerySet,
    resolve: wgpu::Buffer,
    period_ns: f32,
    slots: Vec<Slot>,
    marks: Vec<&'static str>,
}

pub struct Profile {
    /// Phases summed since the last line, in first-seen order.
    phases: Vec<(&'static str, Phase)>,
    gpu: Vec<(&'static str, f64)>,
    gpu_frames: u32,
    frames: u32,
    cpu: f64,
    worst_cpu: f64,
    last_line: std::time::Instant,
    stamps: Option<Stamps>,
    tried: bool,
    /// Frames begun (a slot written in frame `n` is asked from `n + 1`).
    frame: u64,
    /// Frames whose marks had no free slot (every slot still in flight).
    dropped: u32,
    pub notes: Vec<String>,
    /// Per-frame figures summed since the last line (name, sum), and the
    /// frames they cover.
    figures: Vec<(&'static str, f64)>,
    /// The frame's effect sprites (set by the host that built them).
    fx: Option<FxFigures>,
    fx_sum: [f64; 4],
    fx_frames: u32,
}

/// The frame's effect sprites, as the effect system built them.
#[derive(Clone, Copy, Debug, Default)]
pub struct FxFigures {
    pub sprites: u32,
    pub visible: u32,
    /// Sum of the sprites' screen coverage in screens (an estimate).
    pub overdraw: f32,
    /// The biggest sprite's share of the screen.
    pub biggest: f32,
}

impl Default for Profile {
    fn default() -> Self {
        Self {
            phases: Vec::new(),
            gpu: Vec::new(),
            gpu_frames: 0,
            frames: 0,
            cpu: 0.0,
            worst_cpu: 0.0,
            last_line: std::time::Instant::now(),
            stamps: None,
            tried: false,
            frame: 0,
            dropped: 0,
            notes: Vec::new(),
            figures: Vec::new(),
            fx: None,
            fx_sum: [0.0; 4],
            fx_frames: 0,
        }
    }
}

impl Profile {
    /// Starts a frame's GPU marks (when the device has timestamps inside
    /// encoders) and collects the frames read back.
    pub fn begin(&mut self, gpu: &render_h1::gpu::Gpu, enc: &mut wgpu::CommandEncoder) {
        if self.stamps.is_none() && !self.tried {
            self.tried = true;
            let f = gpu.device.features();
            if f.contains(wgpu::Features::TIMESTAMP_QUERY) && f.contains(wgpu::Features::TIMESTAMP_QUERY_INSIDE_ENCODERS) {
                let size = u64::from(MAX_MARKS) * 8;
                self.stamps = Some(Stamps {
                    device: gpu.device.clone(),
                    set: gpu.device.create_query_set(&wgpu::QuerySetDescriptor {
                        label: Some("iw6 profile timestamps"),
                        ty: wgpu::QueryType::Timestamp,
                        count: MAX_MARKS,
                    }),
                    resolve: gpu.device.create_buffer(&wgpu::BufferDescriptor {
                        label: Some("iw6 profile resolve"),
                        size,
                        usage: wgpu::BufferUsages::QUERY_RESOLVE | wgpu::BufferUsages::COPY_SRC,
                        mapped_at_creation: false,
                    }),
                    period_ns: gpu.queue.get_timestamp_period(),
                    slots: (0..RING)
                        .map(|_| Slot {
                            buffer: gpu.device.create_buffer(&wgpu::BufferDescriptor {
                                label: Some("iw6 profile read"),
                                size,
                                usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
                                mapped_at_creation: false,
                            }),
                            mapped: Arc::new(AtomicU8::new(WAITING)),
                            labels: None,
                            written: 0,
                            asked: false,
                        })
                        .collect(),
                    marks: Vec::new(),
                });
            } else {
                self.notes.push("iw6 profile: no GPU timestamps on this device (CPU times only)".into());
            }
        }
        // (Every frame before this one has been submitted.)
        self.frame += 1;
        self.fx = None;
        self.collect();
        if let Some(st) = &mut self.stamps {
            st.marks.clear();
            st.marks.push("start");
            enc.write_timestamp(&st.set, 0);
        }
    }

    /// Asks for the frames submitted since (those written before the frame
    /// now being recorded) and reads those that came back.
    pub fn collect(&mut self) {
        let Some(st) = &mut self.stamps else { return };
        // (Runs the map callbacks of buffers the GPU is done with; does not
        // wait.)
        let _ = st.device.poll(wgpu::PollType::Poll);
        for slot in &mut st.slots {
            if slot.labels.is_some() && !slot.asked && slot.written < self.frame {
                slot.asked = true;
                let flag = slot.mapped.clone();
                slot.buffer.slice(..).map_async(wgpu::MapMode::Read, move |r| {
                    flag.store(if r.is_ok() { MAPPED } else { FAILED }, Ordering::Release);
                });
            }
        }
        for slot in &mut st.slots {
            let state = slot.mapped.load(Ordering::Acquire);
            if !slot.asked || state == WAITING {
                continue;
            }
            if state == FAILED {
                slot.labels = None;
                slot.mapped.store(WAITING, Ordering::Release);
                slot.asked = false;
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
                for i in 1..ticks.len() {
                    let t = ms(ticks[i - 1], ticks[i]);
                    match self.gpu.iter_mut().find(|(l, _)| *l == labels[i]) {
                        Some((_, s)) => *s += t,
                        None => self.gpu.push((labels[i], t)),
                    }
                }
                if ticks.len() > 1 {
                    let t = ms(ticks[0], ticks[ticks.len() - 1]);
                    match self.gpu.iter_mut().find(|(l, _)| *l == "total") {
                        Some((_, s)) => *s += t,
                        None => self.gpu.push(("total", t)),
                    }
                    self.gpu_frames += 1;
                }
            }
            slot.buffer.unmap();
            slot.mapped.store(WAITING, Ordering::Release);
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

    pub fn phase(&mut self, label: &'static str, p: Phase) {
        match self.phases.iter_mut().find(|(l, _)| *l == label) {
            Some((_, s)) => *s += p,
            None => self.phases.push((label, p)),
        }
    }

    /// Ends the frame: resolves its marks; `cpu_ms` the whole record.
    pub fn end(&mut self, enc: &mut wgpu::CommandEncoder, cpu_ms: f64) {
        self.frames += 1;
        if let Some(f) = self.fx.take() {
            self.fx_frames += 1;
            self.fx_sum[0] += f64::from(f.sprites);
            self.fx_sum[1] += f64::from(f.visible);
            self.fx_sum[2] += f64::from(f.overdraw);
            self.fx_sum[3] += f64::from(f.biggest);
        }
        self.cpu += cpu_ms;
        self.worst_cpu = self.worst_cpu.max(cpu_ms);
        let Some(st) = &mut self.stamps else { return };
        let n = st.marks.len() as u32;
        if let Some(slot) = st.slots.iter_mut().find(|s| s.labels.is_none() && !s.asked) {
            enc.resolve_query_set(&st.set, 0..n, &st.resolve, 0);
            enc.copy_buffer_to_buffer(&st.resolve, 0, &slot.buffer, 0, u64::from(n) * 8);
            slot.labels = Some(std::mem::take(&mut st.marks));
            slot.written = self.frame;
        } else {
            self.dropped += 1;
        }
    }

    /// A per-frame figure (lights, tiles, ...): summed, shown as the mean.
    pub fn figure(&mut self, label: &'static str, v: f64) {
        match self.figures.iter_mut().find(|(l, _)| *l == label) {
            Some((_, s)) => *s += v,
            None => self.figures.push((label, v)),
        }
    }

    /// This frame's effect sprites.
    pub fn fx(&mut self, f: FxFigures) {
        self.fx = Some(f);
    }

    /// The table: one row per pass (the GPU's time from timestamps where
    /// the device has them, the CPU's, and what the pass drew), then the
    /// frame's figures. Pure over the sums, so it is tested.
    fn table(&self) -> String {
        let n = f64::from(self.frames.max(1));
        let g = f64::from(self.gpu_frames.max(1));
        let mut rows: Vec<String> = Vec::new();
        rows.push(format!(
            "{:<16} {:>7} {:>7} {:>6} {:>6} {:>7} {:>8}",
            "pass", "gpu ms", "cpu ms", "draws", "calls", "culled", "ktris"
        ));
        let mut labels: Vec<&'static str> = self.phases.iter().map(|(l, _)| *l).collect();
        for (l, _) in &self.gpu {
            if !labels.contains(l) {
                labels.push(l);
            }
        }
        // (The total goes last.)
        labels.retain(|l| *l != "total");
        for l in labels {
            let p = self.phases.iter().find(|(k, _)| *k == l).map(|(_, p)| *p);
            let gpu = self.gpu.iter().find(|(k, _)| *k == l).map(|(_, t)| *t);
            let c = p.map(|p| p.counts).unwrap_or_default();
            rows.push(format!(
                "{:<16} {:>7} {:>7} {:>6} {:>6} {:>7} {:>8}",
                l,
                gpu.map_or("-".to_string(), |t| format!("{:.2}", t / g)),
                p.map_or("-".to_string(), |p| format!("{:.2}", p.total / n)),
                if c.draws + c.culled > 0 { (c.draws / self.frames.max(1)).to_string() } else { "-".into() },
                if c.draws + c.culled > 0 { (c.calls / self.frames.max(1)).to_string() } else { "-".into() },
                if c.draws + c.culled > 0 { (c.culled / self.frames.max(1)).to_string() } else { "-".into() },
                if c.draws > 0 { format!("{:.1}", c.tris as f64 / n / 1000.0) } else { "-".into() },
            ));
        }
        if let Some((_, t)) = self.gpu.iter().find(|(l, _)| *l == "total") {
            rows.push(format!("{:<16} {:>7}", "GPU total", format!("{:.2}", t / g)));
        }
        // The scene's makeup (opaque pass, summed over the frames).
        if let Some((_, p)) = self.phases.iter().find(|(l, _)| *l == "opaque") {
            let c = p.counts;
            let f = self.frames.max(1);
            rows.push(format!(
                "opaque list: {} entries drawn, of them pre-pass {} ({:.0}k tris), models {} ({:.0}k tris), decals {}, sky {}, lit by a spot/omni light {}; {} occluded, {} too small, {} by portals",
                c.draws / f,
                c.prepass / f,
                c.prepass_tris as f64 / n / 1000.0,
                c.models / f,
                c.model_tris as f64 / n / 1000.0,
                c.decals / f,
                c.sky / f,
                c.local_lit / f,
                c.occluded / f,
                c.small / f,
                c.portal_culled / f,
            ));
        }
        if !self.figures.is_empty() {
            rows.push(format!(
                "figures: {}",
                self.figures.iter().map(|(l, v)| format!("{l} {:.1}", v / n)).collect::<Vec<_>>().join(", ")
            ));
        }
        if self.fx_frames > 0 {
            let f = f64::from(self.fx_frames);
            rows.push(format!(
                "effect sprites: {:.0} built, {:.0} on screen, covering {:.2} screens (overdraw estimate), biggest {:.0}% of the screen",
                self.fx_sum[0] / f,
                self.fx_sum[1] / f,
                self.fx_sum[2] / f,
                self.fx_sum[3] / f * 100.0
            ));
        }
        rows.join("\n")
    }

    /// The line when `every` seconds have passed since the last (0: every
    /// frame): means per frame.
    pub fn take_line(&mut self, every: f32) -> Option<String> {
        if self.frames == 0 || self.last_line.elapsed().as_secs_f32() < every {
            return None;
        }
        let n = f64::from(self.frames);
        let mut sum = Phase::default();
        let parts: Vec<String> = self
            .phases
            .iter()
            .map(|(l, p)| {
                sum += *p;
                let c = p.counts;
                let mut s = format!("{l} {:.2}", p.total / n);
                if p.prepare + p.constants + p.encode > 0.0 {
                    s += &format!(" (prep {:.2} const {:.2} enc {:.2}", p.prepare / n, p.constants / n, p.encode / n);
                    if p.cull > 0.0 {
                        s += &format!(" of which cull {:.2}", p.cull / n);
                    }
                    s += ")";
                }
                if c.draws + c.culled > 0 {
                    s += &format!(
                        " [{} draws ({} calls, {} instanced, {} instanceable), {} culled ({} occluded, {} small), {} tris, pipe {}, bg {}/{}/{}, vb {}, ib {}]",
                        c.draws / self.frames,
                        c.calls / self.frames,
                        c.instanced / self.frames,
                        c.instanceable / self.frames,
                        c.culled / self.frames,
                        c.occluded / self.frames,
                        c.small / self.frames,
                        c.tris / u64::from(self.frames),
                        c.pipelines / self.frames,
                        c.groups[0] / self.frames,
                        c.groups[1] / self.frames,
                        c.groups[2] / self.frames,
                        c.vertex_buffers / self.frames,
                        c.index_buffers / self.frames,
                    );
                }
                s
            })
            .collect();
        let c = sum.counts;
        let gpu = if self.gpu_frames > 0 {
            let g = f64::from(self.gpu_frames);
            format!(
                "; GPU ms (mean of {}{}): {}",
                self.gpu_frames,
                if self.dropped > 0 { format!(", {} frames unmeasured: every readback in flight", self.dropped) } else { String::new() },
                self.gpu.iter().map(|(l, t)| format!("{l} {:.2}", t / g)).collect::<Vec<_>>().join(", ")
            )
        } else {
            String::new()
        };
        let head = format!(
            "iw6 profile ({} frames): CPU record {:.2} ms/frame (worst {:.2}), {} draws in {} calls, {} triangles, {} pipeline and {} bind group switches{}",
            self.frames,
            self.cpu / n,
            self.worst_cpu,
            c.draws / self.frames,
            c.calls / self.frames,
            c.tris / u64::from(self.frames),
            c.pipelines / self.frames,
            (c.groups[0] + c.groups[1] + c.groups[2]) / self.frames,
            if self.gpu_frames > 0 { String::new() } else { "; no GPU timestamps on this device".to_string() },
        );
        // The one log format of every game (`render_h1::gpu_timer::pass_line`).
        let sum_of = |names: &[&str]| {
            self.phases
                .iter()
                .filter(|(l, _)| names.contains(l))
                .fold((0u64, 0u64), |(d, t), (_, p)| (d + u64::from(p.counts.draws), t + p.counts.tris))
        };
        let (scene_draws, scene_tris) = sum_of(&["opaque", "blended"]);
        let (shadow_draws, _) = sum_of(&["sun near", "sun far", "spot"]);
        let local_lit = self.phases.iter().find(|(l, _)| *l == "opaque").map_or(0, |(_, p)| u64::from(p.counts.local_lit));
        let counters = render_h1::gpu_timer::FrameCounters {
            draws: Some(scene_draws),
            tris: Some(scene_tris),
            fx_quads: (self.fx_frames > 0).then(|| (self.fx_sum[0] * f64::from(self.frames) / f64::from(self.fx_frames)).round() as u64),
            lights: Some(local_lit),
            shadow_draws: Some(shadow_draws),
        };
        let gpu_pass: Vec<(&str, f64)> = self.gpu.iter().map(|(l, t)| (*l, *t)).collect();
        let shared = if self.gpu_frames > 0 {
            // (Counters are sums over the frames recorded; the timings' mean is over the frames read back.)
            let c = counters.rescaled(self.frames, self.gpu_frames);
            format!("{}\n", render_h1::gpu_timer::pass_line("iw6", &gpu_pass, self.gpu_frames, &c))
        } else {
            String::new()
        };
        let table = format!("{shared}{head}\n{}", self.table());
        let line = format!(
            "iw6 profile ({} frames): CPU ms/frame {:.2} (worst {:.2}): {}; totals: prep {:.2} const {:.2} enc {:.2} (cull {:.2}), {} passes, {} draws, {} calls ({} merged, {} instanced, {} instanceable), {} culled ({} by the portals, {} occluded, {} small), {} triangles, {} pipeline / {} bind group switches, {} KB uploaded, {} own submits{gpu}",
            self.frames,
            self.cpu / n,
            self.worst_cpu,
            parts.join(", "),
            sum.prepare / n,
            sum.constants / n,
            sum.encode / n,
            sum.cull / n,
            c.passes / self.frames,
            c.draws / self.frames,
            c.calls / self.frames,
            c.merged / self.frames,
            c.instanced / self.frames,
            c.instanceable / self.frames,
            c.culled / self.frames,
            c.portal_culled / self.frames,
            c.occluded / self.frames,
            c.small / self.frames,
            c.tris / u64::from(self.frames),
            c.pipelines / self.frames,
            (c.groups[0] + c.groups[1] + c.groups[2]) / self.frames,
            c.bytes / u64::from(self.frames) / 1024,
            c.submits / self.frames,
        );
        self.phases.clear();
        self.gpu.clear();
        self.figures.clear();
        self.fx_sum = [0.0; 4];
        self.fx_frames = 0;
        self.gpu_frames = 0;
        self.dropped = 0;
        self.frames = 0;
        self.cpu = 0.0;
        self.worst_cpu = 0.0;
        self.last_line = std::time::Instant::now();
        Some(if verbose() { format!("{table}\n{line}") } else { table })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_table_joins_cpu_gpu_and_counts_by_pass() {
        let mut p = Profile::default();
        let counts = Counts { draws: 200, calls: 120, culled: 50, tris: 40_000, prepass: 80, prepass_tris: 9_000, models: 30, model_tris: 6_000, ..Default::default() };
        for _ in 0..2 {
            p.phase("opaque", Phase { total: 4.0, counts, ..Default::default() });
            p.frames += 1;
        }
        p.gpu = vec![("opaque", 6.0), ("floatz", 0.5), ("total", 7.0)];
        p.gpu_frames = 2;
        p.figure("fx lights", 6.0);
        p.figure("fx lights", 2.0);
        p.fx_sum = [100.0, 60.0, 3.0, 0.5];
        p.fx_frames = 2;
        let t = p.table();
        assert!(t.contains("pass"), "{t}");
        let row = t.lines().find(|l| l.starts_with("opaque ")).expect("an opaque row");
        // GPU 3.00 per frame, CPU 4.00, 200 draws, 120 calls, 50 culled, 40.0 ktris.
        for want in ["3.00", "4.00", "200", "120", "50", "40.0"] {
            assert!(row.contains(want), "{want} in {row}");
        }
        assert!(t.lines().any(|l| l.starts_with("floatz") && l.contains("0.25")), "{t}");
        assert!(t.contains("GPU total") && t.contains("3.50"), "{t}");
        assert!(t.contains("pre-pass 80") && t.contains("models 30"), "{t}");
        assert!(t.contains("fx lights 4.0"), "{t}");
        assert!(t.contains("effect sprites: 50 built, 30 on screen, covering 1.50 screens"), "{t}");
    }
}
