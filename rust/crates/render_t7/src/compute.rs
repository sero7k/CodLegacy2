//! Black Ops III's code compute shaders (`core_patch`'s compute shader
//! sets, `walk::compute`) as wgpu compute pipelines: the generic D3D11
//! variant (path 2) translated by `dxbc_sm5`, its bind group layouts from
//! the translation's reflection, each binding named by the program's RDEF.
//!
//! A dispatch is given in threads, as the exe gives it, and divided by the
//! shader's thread group size, rounded up (`141ded7f0`).

use std::collections::HashMap;

use dxbc_sm5::wgsl::UavKind;
use dxbc_sm5::{TextureDim, TextureKind};
use render_h1::gpu::Gpu;

use crate::rdef;

/// A compute shader as the zone stores it (variant path 2).
#[derive(Clone, Debug)]
pub struct CodeShader {
    pub name: String,
    pub program: Vec<u8>,
    /// Threads per group, as the zone records it.
    pub groups: [u32; 3],
    pub args: Vec<fastfile_t7::walk::capture::ShaderArg>,
}

/// The code compute shaders of a zone, by name (`core_patch`).
pub fn code_shaders(image: &[u8], capture: &fastfile_t7::walk::capture::Capture) -> HashMap<String, CodeShader> {
    let mut out = HashMap::new();
    for c in &capture.compute_shaders {
        let Some(v) = c.variants.iter().find(|v| v.path == 2 && v.program.is_some()) else { continue };
        let (at, n) = v.program.unwrap();
        let Some(program) = image.get(at..at + n) else { continue };
        out.entry(c.name.clone()).or_insert_with(|| CodeShader { name: c.name.clone(), program: program.to_vec(), groups: v.groups, args: v.args.clone() });
    }
    out
}

/// What a binding is bound to.
pub enum Res<'a> {
    Buffer(&'a wgpu::Buffer),
    View(&'a wgpu::TextureView),
    Sampler(&'a wgpu::Sampler),
}

/// One binding of a program: its group and binding in the WGSL, its D3D
/// slot, its RDEF name and what it is.
#[derive(Clone, Debug)]
pub struct Slot {
    pub group: u32,
    pub binding: u32,
    pub slot: u32,
    pub name: String,
    pub kind: SlotKind,
}

#[derive(Clone, Debug, PartialEq)]
pub enum SlotKind {
    Uniform { size: u64 },
    Texture { dim: TextureDim, kind: TextureKind },
    /// A structured or byte-address buffer read by the shader.
    ReadBuffer,
    Sampler { comparison: bool },
    StorageTexture { format: wgpu::TextureFormat, dim: TextureDim },
    StorageBuffer,
}

/// Nanoseconds spent translating compute programs / making their shader
/// modules and pipelines (all threads), for the timing lines.
pub static CS_TRANSLATE_NS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
pub static CS_PIPELINE_NS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

#[derive(Clone)]
pub struct Program {
    pub name: String,
    pub pipeline: wgpu::ComputePipeline,
    pub layouts: Vec<wgpu::BindGroupLayout>,
    pub slots: Vec<Slot>,
    pub threads: [u32; 3],
    pub args: Vec<fastfile_t7::walk::capture::ShaderArg>,
    /// Kept to make the pieces of a split dispatch ([`Program::piece`]).
    module: wgpu::ShaderModule,
    pipeline_layout: wgpu::PipelineLayout,
    /// Made with `t7_group_offset` (see [`Program::new_split`]).
    splittable: bool,
}

fn storage_format(f: &str) -> Option<wgpu::TextureFormat> {
    use wgpu::TextureFormat as F;
    Some(match f {
        "rgba16float" => F::Rgba16Float,
        "rgba32float" => F::Rgba32Float,
        "r32float" => F::R32Float,
        "r32uint" => F::R32Uint,
        "r32sint" => F::R32Sint,
        "rgba8unorm" => F::Rgba8Unorm,
        "rgba32uint" => F::Rgba32Uint,
        "rg32float" => F::Rg32Float,
        _ => return None,
    })
}

fn view_dim(d: TextureDim) -> wgpu::TextureViewDimension {
    use wgpu::TextureViewDimension as V;
    match d {
        TextureDim::D1 | TextureDim::Buffer => V::D1,
        TextureDim::D2 | TextureDim::D2Ms | TextureDim::Raw => V::D2,
        TextureDim::D2Array => V::D2Array,
        TextureDim::D3 => V::D3,
        TextureDim::Cube => V::Cube,
        TextureDim::CubeArray => V::CubeArray,
    }
}

impl Program {
    /// Translates and makes the pipeline. `uav_formats`: storage formats by
    /// UAV slot where the default (from the return type) is not the
    /// target's.
    pub fn new(gpu: &Gpu, cs: &CodeShader, uav_formats: &[(u32, &str)]) -> Result<Self, String> {
        Self::new_with(gpu, cs, uav_formats, &[])
    }

    /// As `new`; `unfilterable`: texture slots bound to textures that can
    /// only be loaded, not filtered (a depth target read with `Load`).
    /// Typed buffer UAVs are storage buffers of 32-bit words (they hold
    /// indirect dispatch arguments).
    pub fn new_with(gpu: &Gpu, cs: &CodeShader, uav_formats: &[(u32, &str)], unfilterable: &[u32]) -> Result<Self, String> {
        Self::new_opts(gpu, cs, uav_formats, unfilterable, false)
    }

    /// As `new_with`; `nonfiltering`: its samplers are bound as
    /// non-filtering (a program that gathers from an unfilterable texture).
    pub fn new_opts(gpu: &Gpu, cs: &CodeShader, uav_formats: &[(u32, &str)], unfilterable: &[u32], nonfiltering: bool) -> Result<Self, String> {
        Self::build(gpu, cs, uav_formats, unfilterable, nonfiltering, false)
    }

    /// As `new_with`, for a program whose dispatch may need more thread
    /// groups than the device takes in one dimension (the permuted lighting
    /// at 4K: one group a tile): its pipeline carries `t7_group_offset`, an
    /// override added to the group id, and [`Program::piece`] makes the
    /// pipeline of each piece.
    pub fn new_split(gpu: &Gpu, cs: &CodeShader, uav_formats: &[(u32, &str)], unfilterable: &[u32]) -> Result<Self, String> {
        Self::build(gpu, cs, uav_formats, unfilterable, false, true)
    }

    /// Made once per device (and kept): the load makes the programs early
    /// (`crate::prewarm`) and the stages that need them take them from here.
    fn build(gpu: &Gpu, cs: &CodeShader, uav_formats: &[(u32, &str)], unfilterable: &[u32], nonfiltering: bool, split: bool) -> Result<Self, String> {
        let key = format!("cs:{}:{uav_formats:?}:{unfilterable:?}:{nonfiltering}:{split}:{}", cs.name, cs.program.len());
        crate::memo::get(&gpu.device, key, || Self::build_uncached(gpu, cs, uav_formats, unfilterable, nonfiltering, split))
    }

    fn build_uncached(gpu: &Gpu, cs: &CodeShader, uav_formats: &[(u32, &str)], unfilterable: &[u32], nonfiltering: bool, split: bool) -> Result<Self, String> {
        use dxbc_sm5::{Options, Stage};
        let c = dxbc_sm5::Container::parse(&cs.program).map_err(|e| format!("{}: {e:?}", cs.name))?;
        let p = dxbc_sm5::decode(c.shader).map_err(|e| format!("{}: {e:?}", cs.name))?;
        let mut o = Options::for_stage(Stage::Compute);
        o.uav_formats = uav_formats.iter().map(|(s, f)| (*s, (*f).to_owned())).collect();
        o.typed_buffer_uavs_as_words = true;
        o.group_offset_override = split;
        let audit = crate::audit::bounds_audit();
        o.bounds_audit = audit;
        let t_tr = std::time::Instant::now();
        let t = dxbc_sm5::translate_program(&p, &c, &o).map_err(|e| format!("{}: {e:?}", cs.name))?;
        CS_TRANSLATE_NS.fetch_add(t_tr.elapsed().as_nanos() as u64, std::sync::atomic::Ordering::Relaxed);
        let r = &t.reflection;
        let b = o.bindings;
        let names = rdef::bindings(&cs.program);
        let name_of = |kinds: &[u32], slot: u32| names.iter().find(|n| kinds.contains(&n.kind) && n.point == slot).map_or_else(|| format!("?{slot}"), |n| n.name.clone());
        let mut slots = Vec::new();
        for cb in &r.cbuffers {
            slots.push(Slot { group: b.cbuffer_group, binding: cb.binding, slot: cb.slot, name: name_of(&[rdef::CBUFFER], cb.slot), kind: SlotKind::Uniform { size: u64::from(cb.registers.max(1)) * 16 } });
        }
        for tx in &r.textures {
            let kind = if tx.kind == TextureKind::RawBuffer { SlotKind::ReadBuffer } else { SlotKind::Texture { dim: tx.dim, kind: tx.kind } };
            slots.push(Slot { group: b.texture_group, binding: tx.binding, slot: tx.slot, name: name_of(&[2, 5, 7], tx.slot), kind });
        }
        for s in &r.samplers {
            slots.push(Slot { group: b.sampler_group, binding: s.binding, slot: s.slot, name: name_of(&[rdef::SAMPLER], s.slot), kind: SlotKind::Sampler { comparison: s.comparison } });
        }
        for u in &r.uavs {
            let kind = match u.kind {
                UavKind::Typed(dim) => SlotKind::StorageTexture { format: storage_format(&u.format).ok_or_else(|| format!("{}: u{} storage format {}", cs.name, u.slot, u.format))?, dim },
                UavKind::Raw | UavKind::Structured(_) => SlotKind::StorageBuffer,
            };
            slots.push(Slot { group: b.texture_group, binding: u.binding, slot: u.slot, name: name_of(&[4, 6, 8, 9, 10, 11], u.slot), kind });
        }
        if audit {
            slots.push(Slot { group: b.cbuffer_group, binding: dxbc_sm5::wgsl::OOB_BINDING, slot: 0, name: "t7_oob".into(), kind: SlotKind::StorageBuffer });
        }
        let ngroups = 3usize;
        let mut entries: Vec<Vec<wgpu::BindGroupLayoutEntry>> = vec![Vec::new(); ngroups];
        for s in &slots {
            let ty = match &s.kind {
                SlotKind::Uniform { .. } => wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: None },
                SlotKind::ReadBuffer => wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Storage { read_only: true }, has_dynamic_offset: false, min_binding_size: None },
                SlotKind::StorageBuffer => wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Storage { read_only: false }, has_dynamic_offset: false, min_binding_size: None },
                SlotKind::Texture { dim, kind } => wgpu::BindingType::Texture {
                    sample_type: match kind {
                        TextureKind::Float => wgpu::TextureSampleType::Float { filterable: !unfilterable.contains(&s.slot) },
                        TextureKind::Sint => wgpu::TextureSampleType::Sint,
                        TextureKind::Uint => wgpu::TextureSampleType::Uint,
                        TextureKind::Depth => wgpu::TextureSampleType::Depth,
                        TextureKind::RawBuffer => wgpu::TextureSampleType::Float { filterable: false },
                    },
                    view_dimension: view_dim(*dim),
                    multisampled: *dim == TextureDim::D2Ms,
                },
                SlotKind::Sampler { comparison } => wgpu::BindingType::Sampler(if *comparison {
                    wgpu::SamplerBindingType::Comparison
                } else if nonfiltering {
                    wgpu::SamplerBindingType::NonFiltering
                } else {
                    wgpu::SamplerBindingType::Filtering
                }),
                SlotKind::StorageTexture { format, dim } => wgpu::BindingType::StorageTexture { access: wgpu::StorageTextureAccess::WriteOnly, format: *format, view_dimension: view_dim(*dim) },
            };
            entries[s.group as usize].push(wgpu::BindGroupLayoutEntry { binding: s.binding, visibility: wgpu::ShaderStages::COMPUTE, ty, count: None });
        }
        let dev = &gpu.device;
        let scope = dev.push_error_scope(wgpu::ErrorFilter::Validation);
        let layouts: Vec<wgpu::BindGroupLayout> = entries.iter().enumerate().map(|(g, e)| dev.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor { label: Some(&format!("{} g{g}", cs.name)), entries: e })).collect();
        let refs: Vec<Option<&wgpu::BindGroupLayout>> = layouts.iter().map(Some).collect();
        let pl = dev.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: Some(&cs.name), bind_group_layouts: &refs, immediate_size: 0 });
        // `T7_CS_OVERRIDE=<dir>`: a `<name>.wgsl` there replaces the
        // translation (debugging: the same bindings, edited code).
        let wgsl = std::env::var("T7_CS_OVERRIDE").ok().and_then(|d| std::fs::read_to_string(format!("{d}/{}.wgsl", cs.name)).ok()).unwrap_or_else(|| t.wgsl.clone());
        let wgsl = if audit { format!("{wgsl}\nconst T7_OOB_BASE: u32 = {}u;\n", crate::audit::oob_base(&cs.name)) } else { wgsl };
        let t_pl = std::time::Instant::now();
        let module = dev.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some(&cs.name), source: wgpu::ShaderSource::Wgsl(wgsl.into()) });
        let pipeline = dev.create_compute_pipeline(&wgpu::ComputePipelineDescriptor { label: Some(&cs.name), layout: Some(&pl), module: &module, entry_point: Some("main"), compilation_options: Default::default(), cache: crate::pipecache::get(&dev).as_ref() });
        CS_PIPELINE_NS.fetch_add(t_pl.elapsed().as_nanos() as u64, std::sync::atomic::Ordering::Relaxed);
        if let Some(e) = pollster::block_on(scope.pop()) {
            return Err(format!("{}: {e}", cs.name));
        }
        let threads = if r.thread_group.iter().all(|&g| g > 0) { r.thread_group } else { cs.groups };
        Ok(Self { name: cs.name.clone(), pipeline, layouts, slots, threads, args: cs.args.clone(), module, pipeline_layout: pl, splittable: split })
    }

    /// The pipeline of piece `k` of a split dispatch: the same program with
    /// `t7_group_offset` = `k * piece` groups (`piece`: the groups a piece
    /// dispatches at most, the device's limit). Piece 0 is `pipeline`.
    pub fn piece(&self, gpu: &Gpu, k: u32, piece: u32) -> Result<wgpu::ComputePipeline, String> {
        if !self.splittable {
            return Err(format!("{}: not made with new_split", self.name));
        }
        let dev = &gpu.device;
        let scope = dev.push_error_scope(wgpu::ErrorFilter::Validation);
        let constants = [("t7_group_offset", f64::from(k) * f64::from(piece))];
        let pipeline = dev.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some(&self.name),
            layout: Some(&self.pipeline_layout),
            module: &self.module,
            entry_point: Some("main"),
            compilation_options: wgpu::PipelineCompilationOptions { constants: &constants, ..Default::default() },
            cache: crate::pipecache::get(&dev).as_ref(),
        });
        if let Some(e) = pollster::block_on(scope.pop()) {
            return Err(format!("{} piece {k}: {e}", self.name));
        }
        Ok(pipeline)
    }

    /// The bind groups, each binding resolved by `find(slot)`; a binding it
    /// cannot resolve is an error naming it.
    pub fn bind<'a>(&self, gpu: &Gpu, mut find: impl FnMut(&Slot) -> Option<Res<'a>>) -> Result<Vec<wgpu::BindGroup>, String> {
        let mut per: Vec<Vec<wgpu::BindGroupEntry>> = vec![Vec::new(); self.layouts.len()];
        let oob = crate::audit::bounds_audit().then(|| crate::audit::oob_buffer(&gpu.device));
        for s in &self.slots {
            if s.name == "t7_oob" {
                if let Some(o) = oob.as_ref() {
                    per[s.group as usize].push(wgpu::BindGroupEntry { binding: s.binding, resource: o.as_entire_binding() });
                    continue;
                }
            }
            let r = find(s).ok_or_else(|| format!("{}: {} ({:?} slot {}) not made", self.name, s.name, s.kind, s.slot))?;
            let resource = match r {
                Res::Buffer(b) => b.as_entire_binding(),
                Res::View(v) => wgpu::BindingResource::TextureView(v),
                Res::Sampler(x) => wgpu::BindingResource::Sampler(x),
            };
            per[s.group as usize].push(wgpu::BindGroupEntry { binding: s.binding, resource });
        }
        let dev = &gpu.device;
        let scope = dev.push_error_scope(wgpu::ErrorFilter::Validation);
        let groups = per.iter().zip(&self.layouts).map(|(e, l)| dev.create_bind_group(&wgpu::BindGroupDescriptor { label: Some(&self.name), layout: l, entries: e })).collect();
        if let Some(e) = pollster::block_on(scope.pop()) {
            return Err(format!("{}: bind groups: {e}", self.name));
        }
        Ok(groups)
    }

    /// Thread groups for a dispatch of `threads` threads (the exe's
    /// rounding up).
    pub fn groups_for(&self, threads: [u32; 3]) -> [u32; 3] {
        std::array::from_fn(|k| if threads[k] == 0 { 0 } else { threads[k].div_ceil(self.threads[k].max(1)) })
    }
}
