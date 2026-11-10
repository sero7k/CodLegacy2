//! Frame costs (`BO2MP_IW7_PROFILE=1`, and the offline shot's
//! `IW7_SHOT_PROFILE=<frames>`): CPU time per phase of
//! [`crate::world::Host::record_into`], the draw, pass, pipeline and
//! bind-group counts, bytes uploaded, and GPU time per phase from
//! timestamp queries where the adapter has them (read back a frame or two
//! late, never waited on).

use std::sync::Arc;
use std::sync::atomic::{AtomicU8, Ordering};

/// Whether frames are profiled (`BO2MP_IW7_PROFILE=1`).
pub fn on() -> bool {
    std::env::var("BO2MP_IW7_PROFILE").is_ok_and(|v| v.trim() == "1")
}

/// The CPU phases, in order.
pub const CPU_PHASES: [&str; 6] = ["cull", "lights", "constants", "encode shadows+screen", "encode world", "encode post"];
/// The GPU spans, between consecutive timestamps.
pub const GPU_SPANS: [&str; 4] = ["shadows+screen", "volumetric", "world", "post"];

/// One frame's costs.
#[derive(Clone, Debug, Default)]
pub struct FrameStats {
    pub cpu_ms: [f32; 6],
    pub draws: u32,
    pub draws_total: u32,
    pub occluders: [u32; 3],
    pub occluders_total: u32,
    pub passes: u32,
    pub pipeline_switches: u32,
    pub group_switches: u32,
    pub bytes_uploaded: u64,
    /// The latest GPU times read back (ms per span).
    pub gpu_ms: Option<[f32; 4]>,
}

impl FrameStats {
    pub fn cpu_total(&self) -> f32 {
        self.cpu_ms.iter().sum()
    }

    /// One line.
    pub fn line(&self) -> String {
        let cpu: Vec<String> = CPU_PHASES.iter().zip(self.cpu_ms).map(|(n, t)| format!("{n} {t:.2}")).collect();
        let gpu = self.gpu_ms.map_or_else(|| "gpu n/a".to_owned(), |g| format!("gpu {:.2} ms ({})", g.iter().sum::<f32>(), GPU_SPANS.iter().zip(g).map(|(n, t)| format!("{n} {t:.2}")).collect::<Vec<_>>().join(", ")));
        format!(
            "cpu {:.2} ms ({}); draws {} of {}, occluders {:?} of {}, passes {}, pipeline switches {}, bind-group switches {}, uploaded {} B; {gpu}",
            self.cpu_total(),
            cpu.join(", "),
            self.draws,
            self.draws_total,
            self.occluders,
            self.occluders_total,
            self.passes,
            self.pipeline_switches,
            self.group_switches,
            self.bytes_uploaded
        )
    }
}

/// Averages frames' stats.
#[derive(Default)]
pub struct Average {
    sum: FrameStats,
    gpu: [f32; 4],
    gpu_n: u32,
    n: u32,
}

impl Average {
    pub fn add(&mut self, s: &FrameStats) {
        for (a, b) in self.sum.cpu_ms.iter_mut().zip(s.cpu_ms) {
            *a += b;
        }
        self.sum.draws += s.draws;
        self.sum.draws_total = s.draws_total;
        for (a, b) in self.sum.occluders.iter_mut().zip(s.occluders) {
            *a += b;
        }
        self.sum.occluders_total = s.occluders_total;
        self.sum.passes += s.passes;
        self.sum.pipeline_switches += s.pipeline_switches;
        self.sum.group_switches += s.group_switches;
        self.sum.bytes_uploaded += s.bytes_uploaded;
        if let Some(g) = s.gpu_ms {
            for (a, b) in self.gpu.iter_mut().zip(g) {
                *a += b;
            }
            self.gpu_n += 1;
        }
        self.n += 1;
    }

    pub fn frames(&self) -> u32 {
        self.n
    }

    pub fn mean(&self) -> FrameStats {
        let n = self.n.max(1);
        let nf = n as f32;
        let mut m = self.sum.clone();
        for t in &mut m.cpu_ms {
            *t /= nf;
        }
        m.draws /= n;
        for o in &mut m.occluders {
            *o /= n;
        }
        m.passes /= n;
        m.pipeline_switches /= n;
        m.group_switches /= n;
        m.bytes_uploaded /= u64::from(n);
        m.gpu_ms = (self.gpu_n > 0).then(|| self.gpu.map(|g| g / self.gpu_n as f32));
        m
    }
}

const SLOTS: usize = 3;
const STAMPS: u32 = 5;
const FREE: u8 = 0;
const MAPPING: u8 = 1;
const READY: u8 = 2;

/// GPU timestamps round the frame's phases, read back without waiting.
pub struct GpuTimer {
    queries: wgpu::QuerySet,
    resolve: wgpu::Buffer,
    readback: Vec<(wgpu::Buffer, Arc<AtomicU8>)>,
    period_ns: f32,
    frame: usize,
    /// The slot written by the last frame, mapped once that frame has been
    /// submitted (at the next frame's start).
    pending: Option<usize>,
    pub latest: Option<[f32; 4]>,
}

impl GpuTimer {
    /// None when the device can't write timestamps inside encoders.
    pub fn new(device: &wgpu::Device, queue: &wgpu::Queue) -> Option<Self> {
        let need = wgpu::Features::TIMESTAMP_QUERY | wgpu::Features::TIMESTAMP_QUERY_INSIDE_ENCODERS;
        if !device.features().contains(need) {
            return None;
        }
        let queries = device.create_query_set(&wgpu::QuerySetDescriptor { label: Some("iw7 timer"), ty: wgpu::QueryType::Timestamp, count: STAMPS * SLOTS as u32 });
        let size = u64::from(STAMPS) * 8;
        let resolve = device.create_buffer(&wgpu::BufferDescriptor { label: Some("iw7 timer resolve"), size: wgpu::QUERY_RESOLVE_BUFFER_ALIGNMENT * SLOTS as u64, usage: wgpu::BufferUsages::QUERY_RESOLVE | wgpu::BufferUsages::COPY_SRC, mapped_at_creation: false });
        let readback = (0..SLOTS)
            .map(|_| (device.create_buffer(&wgpu::BufferDescriptor { label: Some("iw7 timer readback"), size, usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST, mapped_at_creation: false }), Arc::new(AtomicU8::new(FREE))))
            .collect();
        Some(Self { queries, resolve, readback, period_ns: queue.get_timestamp_period(), frame: 0, pending: None, latest: None })
    }

    /// The slot this frame writes (None: all slots still being read).
    pub fn begin(&mut self) -> Option<usize> {
        if let Some(p) = self.pending.take() {
            self.map(p);
        }
        self.collect();
        let slot = self.frame % SLOTS;
        self.frame += 1;
        (self.readback[slot].1.load(Ordering::Acquire) == FREE).then_some(slot)
    }

    /// Stamp `k` (0 to 4) of `slot`.
    pub fn stamp(&self, enc: &mut wgpu::CommandEncoder, slot: usize, k: u32) {
        enc.write_timestamp(&self.queries, slot as u32 * STAMPS + k);
    }

    /// Resolves `slot`'s stamps and asks for them back.
    pub fn end(&mut self, enc: &mut wgpu::CommandEncoder, slot: usize) {
        self.pending = Some(slot);
        let first = slot as u32 * STAMPS;
        let at = slot as u64 * wgpu::QUERY_RESOLVE_BUFFER_ALIGNMENT;
        enc.resolve_query_set(&self.queries, first..first + STAMPS, &self.resolve, at);
        enc.copy_buffer_to_buffer(&self.resolve, at, &self.readback[slot].0, 0, u64::from(STAMPS) * 8);
    }

    /// Maps `slot` (its frame submitted) to read when it lands.
    fn map(&self, slot: usize) {
        let (buf, state) = &self.readback[slot];
        state.store(MAPPING, Ordering::Release);
        let s = state.clone();
        buf.slice(..).map_async(wgpu::MapMode::Read, move |r| {
            s.store(if r.is_ok() { READY } else { FREE }, Ordering::Release);
        });
    }

    fn collect(&mut self) {
        for (buf, state) in &self.readback {
            if state.load(Ordering::Acquire) != READY {
                continue;
            }
            {
                let data = buf.slice(..).get_mapped_range();
                let t: &[u64] = bytemuck::cast_slice(&data);
                if t.len() >= STAMPS as usize && t[STAMPS as usize - 1] >= t[0] {
                    let ms = |a: usize, b: usize| (t[b].saturating_sub(t[a])) as f32 * self.period_ns / 1e6;
                    self.latest = Some([ms(0, 1), ms(1, 2), ms(2, 3), ms(3, 4)]);
                }
            }
            buf.unmap();
            state.store(FREE, Ordering::Release);
        }
    }
}
