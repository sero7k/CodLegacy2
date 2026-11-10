//! The frame's automatic exposure, as MWR's frame code does it
//! (`docs/H1_NATIVE.md`, "Exposure"): the log-average Rec. 709 luminance of
//! the HDR scene (each sample clamped to 6.1e-5..65504), the target grey of
//! the light set over it, eased toward by `tonemapadaptspeed` a frame when
//! `tonemapblend` is on, then `tonemapexposureadjust` stops and the
//! `tonemapmaxexposure` cap.
//!
//! The average is measured on the GPU over the scene box-filtered down to
//! the game's metering size (halved until it fits 160x90; the exe's
//! reduction source, then `cs_logaccumfromtex2d` on it), one partial sum a
//! 16x16 group and read back without waiting: a copy is
//! recorded one frame, mapped the next, read when the GPU is done. The
//! exposure lags the scene by those frames, as a read back in the engine
//! does.

use std::sync::Arc;
use std::sync::atomic::{AtomicU8, Ordering};

use crate::gpu::Gpu;
use crate::look::Settings;
use crate::tonemap;

const WGSL: &str = r"
@group(0) @binding(0) var src: texture_2d<f32>;
@group(0) @binding(1) var<storage, read_write> partials: array<vec2<f32>>;
var<workgroup> acc: array<vec2<f32>, 256>;

@compute @workgroup_size(16, 16)
fn main(
    @builtin(local_invocation_index) li: u32,
    @builtin(workgroup_id) wg: vec3<u32>,
    @builtin(num_workgroups) nw: vec3<u32>,
    @builtin(global_invocation_id) gid: vec3<u32>,
) {
    let dims = textureDimensions(src);
    // The game halves the scene until it fits 160x90 (box filtering),
    // then meters that picture: one texel here is a k x k block.
    var k = 1u;
    loop {
        if (dims.x / k <= 160u && dims.y / k <= 90u) { break; }
        k = k * 2u;
    }
    let small = dims / k;
    var v = vec2<f32>(0.0, 0.0);
    if (gid.x < small.x && gid.y < small.y) {
        var c = vec3<f32>(0.0);
        for (var y = 0u; y < k; y = y + 1u) {
            for (var x = 0u; x < k; x = x + 1u) {
                let s = textureLoad(src, vec2<i32>(gid.xy * k + vec2<u32>(x, y)), 0).rgb;
                c = c + select(vec3<f32>(0.0), s, s == s);
            }
        }
        c = c / f32(k * k);
        let l = dot(c, vec3<f32>(0.2126, 0.7152, 0.0722));
        v = vec2<f32>(log(clamp(l, 6.1e-5, 65504.0)), 1.0);
    }
    acc[li] = v;
    workgroupBarrier();
    var s = 128u;
    loop {
        if (s == 0u) { break; }
        if (li < s) { acc[li] = acc[li] + acc[li + s]; }
        workgroupBarrier();
        s = s >> 1u;
    }
    if (li == 0u) {
        partials[wg.y * nw.x + wg.x] = acc[0];
    }
}
";

/// The size the game meters a `w` x `h` scene at: halved (both ways) until
/// it fits 160x90 (`docs/H1_NATIVE.md`, "Exposure": the reduction source).
pub fn metered_size(w: u32, h: u32) -> (u32, u32) {
    let mut k = 1;
    while w / k > 160 || h / k > 90 {
        k *= 2;
    }
    ((w / k).max(1), (h / k).max(1))
}

const IDLE: u8 = 0;
const COPIED: u8 = 1;
const MAPPING: u8 = 2;
const READY: u8 = 3;

struct Slot {
    buffer: wgpu::Buffer,
    state: Arc<AtomicU8>,
    groups: u32,
}

pub struct Meter {
    pipeline: wgpu::ComputePipeline,
    layout: wgpu::BindGroupLayout,
    partials: Option<(wgpu::Buffer, u32)>,
    slots: Vec<Slot>,
    /// The latest measured log-average luminance.
    pub luminance: Option<f32>,
    /// ln of the adapted automatic exposure.
    ln_exposure: Option<f32>,
}

impl Meter {
    pub fn new(gpu: &Gpu) -> Self {
        let dev = &gpu.device;
        let module = dev.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("h1_meter"),
            source: wgpu::ShaderSource::Wgsl(WGSL.into()),
        });
        let layout = dev.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("h1_meter"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: false },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Storage { read_only: false },
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
        });
        let pl = dev.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("h1_meter"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let pipeline = dev.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("h1_meter"),
            layout: Some(&pl),
            module: &module,
            entry_point: Some("main"),
            compilation_options: Default::default(),
            cache: None,
        });
        Self {
            pipeline,
            layout,
            partials: None,
            slots: Vec::new(),
            luminance: None,
            ln_exposure: None,
        }
    }

    /// Takes in finished read backs, maps last frame's copy and records a
    /// measurement of `scene` (an HDR target of `width` x `height`).
    pub fn record(
        &mut self,
        gpu: &Gpu,
        encoder: &mut wgpu::CommandEncoder,
        scene: &wgpu::TextureView,
        width: u32,
        height: u32,
    ) {
        self.collect();
        // Last frame's copies have been submitted: map them now.
        for s in &self.slots {
            if s.state.load(Ordering::Acquire) == COPIED {
                s.state.store(MAPPING, Ordering::Release);
                let st = s.state.clone();
                s.buffer
                    .slice(..)
                    .map_async(wgpu::MapMode::Read, move |r| {
                        st.store(if r.is_ok() { READY } else { IDLE }, Ordering::Release);
                    });
            }
        }
        let (sw, sh) = metered_size(width, height);
        let (gx, gy) = (sw.div_ceil(16).max(1), sh.div_ceil(16).max(1));
        let groups = gx * gy;
        let size = u64::from(groups) * 8;
        let dev = &gpu.device;
        if self.partials.as_ref().is_none_or(|(_, n)| *n < groups) {
            self.partials = Some((
                dev.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("h1_meter_partials"),
                    size,
                    usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
                    mapped_at_creation: false,
                }),
                groups,
            ));
            self.slots.clear();
        }
        let Some((partials, _)) = &self.partials else {
            return;
        };
        let slot = match self
            .slots
            .iter()
            .position(|s| s.state.load(Ordering::Acquire) == IDLE)
        {
            Some(i) => i,
            None if self.slots.len() < 3 => {
                self.slots.push(Slot {
                    buffer: dev.create_buffer(&wgpu::BufferDescriptor {
                        label: Some("h1_meter_readback"),
                        size,
                        usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
                        mapped_at_creation: false,
                    }),
                    state: Arc::new(AtomicU8::new(IDLE)),
                    groups,
                });
                self.slots.len() - 1
            }
            None => return,
        };
        let group = dev.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("h1_meter"),
            layout: &self.layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(scene),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: partials.as_entire_binding(),
                },
            ],
        });
        {
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("h1_meter"),
                timestamp_writes: None,
            });
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, &group, &[]);
            pass.dispatch_workgroups(gx, gy, 1);
        }
        let s = &mut self.slots[slot];
        s.groups = groups;
        encoder.copy_buffer_to_buffer(partials, 0, &s.buffer, 0, size);
        s.state.store(COPIED, Ordering::Release);
    }

    /// Reads every finished read back into `luminance`.
    fn collect(&mut self) {
        for s in &self.slots {
            if s.state.load(Ordering::Acquire) != READY {
                continue;
            }
            {
                let data = s.buffer.slice(..).get_mapped_range();
                let v: &[f32] = bytemuck::cast_slice(&data);
                let (mut sum, mut n) = (0.0f64, 0.0f64);
                for p in v.chunks_exact(2).take(s.groups as usize) {
                    if p[0].is_finite() && p[1].is_finite() {
                        sum += f64::from(p[0]);
                        n += f64::from(p[1]);
                    }
                }
                if n > 0.0 {
                    self.luminance = Some((sum / n).exp() as f32);
                }
            }
            s.buffer.unmap();
            s.state.store(IDLE, Ordering::Release);
        }
    }

    /// The exposure for this frame from the latest measurement and the
    /// light set; 1 until a measurement arrives.
    pub fn exposure(&mut self, ls: &Settings) -> f32 {
        let Some(lum) = self.luminance else {
            return 1.0;
        };
        let b = tonemap::HDR_SCALAR_DEFAULT;
        let auto = crate::tonemap::is_auto(ls);
        let target = if auto {
            tonemap::shown_key(ls, lum, b) / lum.max(tonemap::MIN_LUMINANCE)
        } else {
            tonemap::ev_to_exposure(ls.f("tonemapexposure", 10.0), b)
        };
        let ln_e = match self.ln_exposure {
            Some(prev) if auto && ls.f("tonemapblend", 1.0) != 0.0 => {
                tonemap::adapt(prev, target, lum, ls.f("tonemapadaptspeed", 0.02))
            }
            _ => target.max(tonemap::MIN_LUMINANCE).ln(),
        };
        self.ln_exposure = Some(ln_e);
        let e = ln_e.exp() * 2f32.powf(ls.f("tonemapexposureadjust", 0.0));
        e.min(tonemap::ev_to_exposure(ls.f("tonemapmaxexposure", -10.0), b))
    }
}
