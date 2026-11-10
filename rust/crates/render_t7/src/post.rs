//! BO3's post chain as BlackOps3.exe records it after the lit scene
//! (`docs/T7_NATIVE.md`, "Post chain"). The orchestrator (exe 0x141d62ee0)
//! runs, in order: the colour-grade LUT build (`create_lut2dv`), the bloom
//! downsample chain (`bloom_downsample_l0`, `_l1*`, `_lx` ..., `_ev` into
//! the luminance target), the bloom blurs (x then y, mips 5..1), eye
//! exposure (`exposure_compute`), then `bloom_apply_lut` (bloom + grade);
//! `resample_final` follows after AA.
//!
//! Built so far: the `PostFxCBuffer` the post programs read
//! ([`PostFxConsts`]) and the eye-exposure pass ([`ExposurePass`]) with its
//! CPU inputs (`post_exposure`). The LUT build, bloom chain,
//! `bloom_apply_lut` and `resample_final` are listed in the doc as not done.

#[allow(unused_imports)]
use crate::profile::{ComputeExt as _, EncExt as _, PassExt as _, QueueExt as _};
use render_h1::gpu::Gpu;

use crate::post_exposure::{ViewExposure, exposure_controls};

pub use crate::post_run::{FINAL_FORMAT, Post};

/// `PostFxCBuffer` (2016 bytes): postFxControl0..F at 0x000, postFxConst00..63
/// at 0x100, postFxBloom00..25 at 0x500, filterTap at 0x6a0 (8 vec4),
/// the post view/projection matrices at 0x720.
#[derive(Clone)]
pub struct PostFxConsts(pub [[f32; 4]; 126]);

impl Default for PostFxConsts {
    fn default() -> Self {
        Self([[0.0; 4]; 126])
    }
}

impl PostFxConsts {
    pub const BYTES: u64 = 2016;
    pub fn control(&mut self, i: usize, v: [f32; 4]) {
        self.0[i] = v;
    }
    pub fn constant(&mut self, i: usize, v: [f32; 4]) {
        self.0[16 + i] = v;
    }
    pub fn bloom(&mut self, i: usize, v: [f32; 4]) {
        self.0[80 + i] = v;
    }
    pub fn bytes(&self) -> Vec<u8> {
        bytemuck::cast_slice(&self.0).to_vec()
    }
    /// The exposure controls the exe sets before `exposure_compute`.
    pub fn set_exposure(&mut self, e: &ViewExposure, meter_scale: f32, adaptive: bool, view_index: u32) {
        let [c0, c1] = exposure_controls(e, meter_scale, adaptive, view_index);
        self.control(0, c0);
        self.control(1, c1);
    }
}

/// Translates one of the zone's compute programs (`core_patch`, the
/// generic D3D11 variant) with the binding plan the post passes use:
/// constant buffer `b8` at group 0 binding 40, UAVs `u0..` at group 1
/// binding 160+.
pub fn translate_compute(program: &[u8], uav_formats: &[(u32, &str)]) -> Result<String, String> {
    let c = dxbc_sm5::Container::parse(program).map_err(|e| format!("{e:?}"))?;
    let p = dxbc_sm5::decode(c.shader).map_err(|e| format!("{e:?}"))?;
    let mut o = dxbc_sm5::Options::for_stage(p.stage);
    o.uav_formats = uav_formats.iter().map(|(s, f)| (*s, (*f).to_owned())).collect();
    Ok(dxbc_sm5::translate_program(&p, &c, &o).map_err(|e| format!("{e:?}"))?.wgsl)
}

/// `exposure_compute` as the exe records it (0x141d62430): one 8×8 group
/// (`Dispatch(1, 1, 1)`) over two RGBA16F UAVs, u0 the luminance target the
/// bloom chain's `_ev` step wrote and u1 this view's exposure history, with
/// `postFxControl0/1` from the CPU exposure.
pub struct ExposurePass {
    pipeline: wgpu::ComputePipeline,
    consts: wgpu::Buffer,
}

impl ExposurePass {
    pub fn new(gpu: &Gpu, program: &[u8]) -> Result<Self, String> {
        let wgsl = translate_compute(program, &[(0, "r32float"), (1, "r32float")])?;
        let module = gpu.device.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some("t7 exposure_compute"), source: wgpu::ShaderSource::Wgsl(wgsl.into()) });
        let pipeline = gpu.device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("t7 exposure_compute"),
            layout: None,
            module: &module,
            entry_point: Some("main"),
            compilation_options: Default::default(),
            cache: crate::pipecache::get(&gpu.device).as_ref(),
        });
        let consts = crate::audit::buffer(&gpu.device, &wgpu::BufferDescriptor {
            label: Some("t7 PostFxCBuffer"),
            size: PostFxConsts::BYTES,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        Ok(Self { pipeline, consts })
    }

    /// Records the pass: `luminance` and `history` are R32F storage views
    /// (the luminance target 0x23 is R32_FLOAT in the exe).
    pub fn record(&self, gpu: &Gpu, enc: &mut wgpu::CommandEncoder, consts: &PostFxConsts, luminance: &wgpu::TextureView, history: &wgpu::TextureView) {
        gpu.queue.write_buffer_p(&self.consts, 0, &consts.bytes());
        self.record_with(gpu, enc, &self.consts, luminance, history);
    }

    /// As `record`, with the constants already in `consts`.
    pub fn record_with(&self, gpu: &Gpu, enc: &mut wgpu::CommandEncoder, consts: &wgpu::Buffer, luminance: &wgpu::TextureView, history: &wgpu::TextureView) {
        let g0 = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("t7 exposure consts"),
            layout: &self.pipeline.get_bind_group_layout(0),
            entries: &[wgpu::BindGroupEntry { binding: 40, resource: consts.as_entire_binding() }],
        });
        let g1 = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("t7 exposure uavs"),
            layout: &self.pipeline.get_bind_group_layout(1),
            entries: &[
                wgpu::BindGroupEntry { binding: 160, resource: wgpu::BindingResource::TextureView(luminance) },
                wgpu::BindGroupEntry { binding: 161, resource: wgpu::BindingResource::TextureView(history) },
            ],
        });
        let mut pass = enc.compute_pass_p(&wgpu::ComputePassDescriptor { label: Some("t7 exposure_compute"), timestamp_writes: None });
        pass.set_pipeline_p(&self.pipeline);
        pass.set_bind_group_p(0, &g0, &[]);
        pass.set_bind_group_p(1, &g1, &[]);
        pass.dispatch_p(1, 1, 1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_cbuffer_is_the_programs_size_and_layout() {
        let mut c = PostFxConsts::default();
        c.control(1, [1.0, 2.0, 3.0, 4.0]);
        c.constant(60, [5.0; 4]);
        c.bloom(25, [6.0; 4]);
        let b = c.bytes();
        assert_eq!(b.len() as u64, PostFxConsts::BYTES);
        let f = |at: usize| f32::from_le_bytes(b[at..at + 4].try_into().unwrap());
        assert_eq!(f(0x10), 1.0);
        assert_eq!(f(0x4c0), 5.0);
        assert_eq!(f(0x690), 6.0);
    }
}

/// Offline: `T7_CORE_PATCH=<core_patch.ff> cargo test -p render_t7 --lib
/// exposure_pass_runs -- --ignored`: the zone's own `exposure_compute`
/// translates, its pipeline creates and validates and one dispatch runs on
/// a synthetic luminance target.
#[cfg(test)]
mod offline {
    use super::*;

    fn device() -> Option<Gpu> {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env());
        let adapter = pollster::block_on(instance.request_adapter(&Default::default())).ok()?;
        let f = wgpu::Features::TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES;
        let want = if adapter.features().contains(f) { f } else { wgpu::Features::empty() };
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor { required_features: want, required_limits: adapter.limits(), ..Default::default() })).ok()?;
        Some(Gpu::from_parts(device, queue))
    }

    #[test]
    #[ignore = "needs T7_CORE_PATCH"]
    fn exposure_pass_runs() {
        let path = std::env::var("T7_CORE_PATCH").expect("T7_CORE_PATCH");
        let z = fastfile_t7::Zone::open(std::path::Path::new(&path)).expect("zone");
        let w = fastfile_t7::walk::walk(&z);
        let cs = w.capture.compute_shaders.iter().find(|c| c.name == "exposure_compute").expect("exposure_compute");
        let v = cs.variants.iter().find(|v| v.path == 2 && v.program.is_some()).or_else(|| cs.variants.iter().find(|v| v.program.is_some())).expect("variant");
        let (at, n) = v.program.unwrap();
        let program = &z.image[at..at + n];
        let gpu = device().expect("device");
        let scope = gpu.device.push_error_scope(wgpu::ErrorFilter::Validation);
        let pass = ExposurePass::new(&gpu, program).expect("translate");
        let tex = |label| {
            gpu.device.create_texture(&wgpu::TextureDescriptor {
                label: Some(label),
                size: wgpu::Extent3d { width: 8, height: 8, depth_or_array_layers: 1 },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::R32Float,
                usage: wgpu::TextureUsages::STORAGE_BINDING | wgpu::TextureUsages::COPY_SRC,
                view_formats: &[],
            })
        };
        let (lum, hist) = (tex("lum"), tex("hist"));
        let mut c = PostFxConsts::default();
        let e = crate::post_exposure::ExposureState::default().update(
            crate::post_exposure::ExposureBase { bias: 0.0, min_ev: 1.0, max_ev: 12.0 },
            &[],
            &[],
            0,
            1.44,
            true,
        );
        c.set_exposure(&e, 1.0, true, 0);
        let mut enc = gpu.device.create_command_encoder(&Default::default());
        pass.record(&gpu, &mut enc, &c, &lum.create_view(&Default::default()), &hist.create_view(&Default::default()));
        gpu.queue.submit([enc.finish()]);
        let err = pollster::block_on(scope.pop());
        assert!(err.is_none(), "{err:?}");
    }
}

#[cfg(test)]
mod probe {
    #[test]
    #[ignore = "probe"]
    fn print_code_technique_vs() {
        let path = std::env::var("T7_CORE_PATCH").unwrap();
        let z = fastfile_t7::Zone::open(std::path::Path::new(&path)).unwrap();
        let w = fastfile_t7::walk::walk(&z);
        for n in std::env::var("PROBE").unwrap().split(',') {
            let t = w.capture.techsets.iter().find(|t| t.name.split('#').next() == Some(n)).unwrap();
            let ti = t.techniques[5].as_ref().unwrap();
            for (k, p) in ti.programs.iter().enumerate() {
                let (at, len) = p.unwrap();
                let b = &z.image[at..at + len];
                let c = dxbc_sm5::Container::parse(b).unwrap();
                let pr = dxbc_sm5::decode(c.shader).unwrap();
                let t = dxbc_sm5::translate_program(&pr, &c, &dxbc_sm5::Options::for_stage(pr.stage)).unwrap();
                let decl: Vec<String> = if std::env::var("FULL").is_ok() { t.wgsl.lines().map(str::to_owned).collect() } else { t.wgsl.lines().filter(|l| l.contains("@group") || l.contains("@location") || l.contains("fn main")).map(str::to_owned).collect() };
                println!("== {n} {k} inputs {:?}\n{}", c.inputs.iter().map(|i| (&i.semantic, i.semantic_index)).collect::<Vec<_>>(), decl.join("\n"));
                println!("{:?}", crate::rdef::bindings(b));
            }
        }
    }
}
