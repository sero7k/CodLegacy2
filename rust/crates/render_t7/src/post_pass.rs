//! The post chain's full-screen code draws as BlackOps3.exe makes them
//! (exe 0x141d50790): a code material's technique 5 (`core_patch`, the
//! technique sets `<name>#da7372ff`), drawn as one triangle over the target
//! in pixels — corners (0, 0), (2W, 0), (0, 2H), texture coordinates (0, 0),
//! (2, 0), (0, 2), colour 0xffffffff, `TEXCOORD15` the packed word
//! 0x3ffe7f7f — through the code constants the programs read
//! (`viewProjectionMatrix` the target's pixel-to-clip transform, `eyeOffset`
//! zero), every vertex on bone 0 (identity).
//!
//! Bindings come from the technique's arguments: code constants
//! (0x400 | stage) into the buffer they name (1 `PerSceneConsts`, 8
//! `PostFxCBuffer`), code textures (0x500 | stage: the render target the
//! exe's view role gives that slot), code buffers (0x600 | stage: 6 bone
//! matrices, 7 model instances) and the material's samplers
//! (0x300 | stage, by name hash).

#[allow(unused_imports)]
use crate::profile::{ComputeExt as _, EncExt as _, PassExt as _, QueueExt as _};
use std::collections::HashMap;

use render_h1::gpu::Gpu;

use crate::state::StateMap;

/// The technique the exe draws code materials with.
pub const CODE_TECHNIQUE: usize = 5;
const CODE_BUFFER_BONES: u32 = 6;
const CODE_BUFFER_INSTANCES: u32 = 7;
/// `PerSceneConsts` registers the code draws read: `viewProjectionMatrix`
/// rows at 8..11, `eyeOffset` at 24.
const PER_SCENE_REGISTERS: usize = 25;
/// Model instance records (24 bytes): enough for any instance index the
/// packed `TEXCOORD15` word can give (its low 16 bits).
const INSTANCES: u64 = 0x10000;

/// A code material: its technique 5 programs, arguments, state and sampler
/// table.
#[derive(Clone, Debug)]
pub struct CodeTech {
    pub name: String,
    pub vs: Vec<u8>,
    pub ps: Vec<u8>,
    pub args: Vec<fastfile_t7::walk::capture::ShaderArg>,
    pub state: StateMap,
    /// (name hash, sampler state) from the material.
    pub samplers: Vec<(u32, u16)>,
}

/// The code materials `names` from a zone (`core_patch`).
pub fn code_techs(image: &[u8], cap: &fastfile_t7::walk::capture::Capture, names: &[&str]) -> Result<HashMap<String, CodeTech>, String> {
    let mut out = HashMap::new();
    for &n in names {
        let ts = cap.techsets.iter().find(|t| t.name.trim_start_matches(',').split('#').next() == Some(n)).ok_or_else(|| format!("{n}: no technique set in core_patch"))?;
        let ti = ts.techniques[CODE_TECHNIQUE].as_ref().ok_or_else(|| format!("{n}: no technique {CODE_TECHNIQUE}"))?;
        let prog = |k: usize| -> Result<Vec<u8>, String> {
            let (at, len) = ti.programs[k].ok_or_else(|| format!("{n}: no program {k}"))?;
            image.get(at..at + len).map(<[u8]>::to_vec).ok_or_else(|| format!("{n}: program past the zone"))
        };
        let state = ti.state_map.and_then(|s| image.get(s..s + 112)).and_then(StateMap::read).ok_or_else(|| format!("{n}: no state map"))?;
        let samplers = cap
            .materials
            .iter()
            .find(|m| m.name == n)
            .map(|m| m.sampler_defs.iter().map(|d| (u32::from_le_bytes(d[0..4].try_into().unwrap()), u16::from_le_bytes([d[4], d[5]]) & 0x3fff)).collect())
            .unwrap_or_default();
        out.insert(n.to_owned(), CodeTech { name: n.to_owned(), vs: prog(0)?, ps: prog(1)?, args: ti.args.clone(), state, samplers });
    }
    Ok(out)
}

/// The buffers every code draw shares: model instances (all on bone 0) and
/// bone matrices (bone 0 the identity).
pub struct CodeBuffers {
    pub instances: wgpu::Buffer,
    pub bones: wgpu::Buffer,
}

impl CodeBuffers {
    pub fn new(gpu: &Gpu) -> Self {
        use wgpu::util::DeviceExt;
        let dev = &gpu.device;
        let instances = crate::audit::buffer(&dev, &wgpu::BufferDescriptor { label: Some("t7 code instances"), size: INSTANCES * 24, usage: wgpu::BufferUsages::STORAGE, mapped_at_creation: false });
        let bone: [f32; 16] = [1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0];
        let bones = dev.create_buffer_init(&wgpu::util::BufferInitDescriptor { label: Some("t7 code bones"), contents: bytemuck::cast_slice(&bone), usage: wgpu::BufferUsages::STORAGE });
        Self { instances, bones }
    }
}

/// `PerSceneConsts` for a code draw into a `w` × `h` target.
pub fn per_scene(w: u32, h: u32) -> [[f32; 4]; PER_SCENE_REGISTERS] {
    let mut r = [[0f32; 4]; PER_SCENE_REGISTERS];
    r[8] = [2.0 / w as f32, 0.0, 0.0, 0.0];
    r[9] = [0.0, -2.0 / h as f32, 0.0, 0.0];
    r[10] = [0.0, 0.0, 1.0, 0.0];
    r[11] = [-1.0, 1.0, 0.0, 1.0];
    r
}

/// The exe's code-draw triangle for a `w` × `h` target (32-byte vertices:
/// position xyzw, colour, texture coordinate, packed `TEXCOORD15`).
pub fn triangle(w: u32, h: u32) -> Vec<u8> {
    let mut v = Vec::with_capacity(96);
    for (x, y, s, t) in [(0.0, 0.0, 0.0f32, 0.0f32), (2.0 * w as f32, 0.0, 2.0, 0.0), (0.0, 2.0 * h as f32, 0.0, 2.0)] {
        for f in [x, y, 0.0, 1.0] {
            v.extend_from_slice(&f32::to_le_bytes(f));
        }
        v.extend_from_slice(&0xffff_ffffu32.to_le_bytes());
        v.extend_from_slice(&s.to_le_bytes());
        v.extend_from_slice(&t.to_le_bytes());
        v.extend_from_slice(&0x3ffe_7f7fu32.to_le_bytes());
    }
    v
}

fn vertex_attribute(semantic: &str, index: u32, component_type: u32, location: u32) -> Result<wgpu::VertexAttribute, String> {
    use wgpu::VertexFormat as F;
    let (offset, format) = match (semantic.to_ascii_uppercase().as_str(), index) {
        ("POSITION", 0) => (0, F::Float32x4),
        ("COLOR", 0) => (16, F::Unorm8x4),
        ("TEXCOORD", 0) => (20, F::Float32x2),
        ("TEXCOORD", 15) => (28, if component_type == 3 { F::Unorm8x4 } else if component_type == 2 { F::Sint8x4 } else { F::Uint8x4 }),
        (s, i) => return Err(format!("vertex input {s}{i} not in the code vertex")),
    };
    Ok(wgpu::VertexAttribute { format, offset, shader_location: location })
}

fn translate(t: &CodeTech) -> Result<(dxbc_sm5::Translation, dxbc_sm5::Translation), String> {
    use dxbc_sm5::{Options, Stage};
    let vs_c = dxbc_sm5::Container::parse(&t.vs).map_err(|e| format!("{} vs: {e:?}", t.name))?;
    let ps_c = dxbc_sm5::Container::parse(&t.ps).map_err(|e| format!("{} ps: {e:?}", t.name))?;
    let vs_p = dxbc_sm5::decode(vs_c.shader).map_err(|e| format!("{} vs: {e:?}", t.name))?;
    let ps_p = dxbc_sm5::decode(ps_c.shader).map_err(|e| format!("{} ps: {e:?}", t.name))?;
    let mut po = Options::for_stage(Stage::Pixel);
    po.varyings = dxbc_sm5::link(&vs_c.outputs, &ps_c.inputs);
    let ps = dxbc_sm5::translate_program(&ps_p, &ps_c, &po).map_err(|e| format!("{} ps: {e:?}", t.name))?;
    let mut vo = Options::for_stage(Stage::Vertex);
    vo.varyings = dxbc_sm5::link(&vs_c.outputs, &ps_c.inputs);
    vo.output_interp = ps.reflection.inputs.iter().map(|i| (i.semantic.to_ascii_uppercase(), i.index, i.interp)).collect();
    let vs = dxbc_sm5::translate_program(&vs_p, &vs_c, &vo).map_err(|e| format!("{} vs: {e:?}", t.name))?;
    Ok((vs, ps))
}

/// What a binding of a code draw is bound to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Bind {
    PerScene,
    PostFx,
    Texture(u32),
    Bones,
    Instances,
    Sampler(u16),
    /// A constant buffer other than the code ones (`b7`: the motion vector
    /// parameters), bound by [`CodePass::bind_with`].
    Extra(u32),
    /// A code buffer other than the bones and instances (0xc
    /// `gLitFogVolumes`), bound by [`CodePass::bind_full`].
    CodeBuffer(u32),
    /// A sampler no material argument binds (the renderer's own, as
    /// `ShadowSamplerComparisonState` at s13): by its RDEF name
    /// (`CodePass::named_samplers`), bound by [`CodePass::bind_full`].
    NamedSampler(usize),
}

/// One code material made into a pipeline for given target formats.
#[derive(Clone)]
pub struct CodePass {
    pub name: String,
    pipeline: wgpu::RenderPipeline,
    layouts: [wgpu::BindGroupLayout; 3],
    /// (group, binding, what).
    binds: Vec<(u32, u32, Bind)>,
    /// The code textures it reads.
    pub code_textures: Vec<u32>,
    /// The samplers bound by name ([`Bind::NamedSampler`]).
    pub named_samplers: Vec<String>,
    /// bo2mp: per colour target, whether the pixel program writes it (a
    /// target it skips, as `bloom_downsample_l1_scene`'s `SV_Target1`, is
    /// left out of the pipeline and the pass).
    written: Vec<bool>,
}

impl CodePass {
    /// `unfilterable`: code textures bound to formats that can only be
    /// loaded (R32F).
    pub fn new(gpu: &Gpu, t: &CodeTech, formats: &[wgpu::TextureFormat], unfilterable: &[u32]) -> Result<Self, String> {
        // (Made once per device: the load makes them early, `crate::prewarm`.)
        let key = format!("pass:{}:{formats:?}:{unfilterable:?}:{}:{}", t.name, t.vs.len(), t.ps.len());
        crate::memo::get(&gpu.device, key, || Self::new_uncached(gpu, t, formats, unfilterable))
    }

    fn new_uncached(gpu: &Gpu, t: &CodeTech, formats: &[wgpu::TextureFormat], unfilterable: &[u32]) -> Result<Self, String> {
        Self::new_ext(gpu, t, formats, None, unfilterable, false)
    }

    /// As [`Self::new`], with a depth target (its state from the
    /// technique's state map) and, `nonfiltering`, samplers bound as
    /// non-filtering (a code texture that can only be loaded).
    pub fn new_ext(gpu: &Gpu, t: &CodeTech, formats: &[wgpu::TextureFormat], depth: Option<wgpu::TextureFormat>, unfilterable: &[u32], nonfiltering: bool) -> Result<Self, String> {
        let (vs, ps) = translate(t)?;
        let stage = |ty: u16| ty & 0xff;
        let arg = |kind: u16, st: u16, slot: u32| t.args.iter().find(|a| a.ty & 0xff00 == kind && stage(a.ty) == st && u32::from(a.location) == slot);
        let mut binds = Vec::new();
        let mut entries: [Vec<wgpu::BindGroupLayoutEntry>; 3] = Default::default();
        let mut code_textures = Vec::new();
        let mut named_samplers = Vec::new();
        let rdef_names = [crate::rdef::bindings(&t.vs), crate::rdef::bindings(&t.ps)];
        for (st, r, vis) in [(0u16, &vs.reflection, wgpu::ShaderStages::VERTEX), (1, &ps.reflection, wgpu::ShaderStages::FRAGMENT)] {
            for c in &r.cbuffers {
                let what = match c.slot {
                    1 => Bind::PerScene,
                    8 => Bind::PostFx,
                    s => Bind::Extra(s),
                };
                if let Some(e) = entries[0].iter_mut().find(|e| e.binding == c.binding) {
                    e.visibility |= vis;
                    continue;
                }
                entries[0].push(wgpu::BindGroupLayoutEntry { binding: c.binding, visibility: vis, ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: None }, count: None });
                binds.push((0, c.binding, what));
            }
            for x in &r.textures {
                let what = if let Some(a) = arg(0x500, st, x.slot) {
                    code_textures.push(a.u[0]);
                    Bind::Texture(a.u[0])
                } else if let Some(a) = arg(0x200, st, x.slot) {
                    // bo2mp: a material texture (a filter's own pictures:
                    // `generic_filter_out_of_bounds`), asked of the caller's
                    // `texture` by its name hash.
                    code_textures.push(a.u[0]);
                    Bind::Texture(a.u[0])
                } else if let Some(a) = arg(0x600, st, x.slot) {
                    match a.u[0] {
                        CODE_BUFFER_BONES => Bind::Bones,
                        CODE_BUFFER_INSTANCES => Bind::Instances,
                        b => Bind::CodeBuffer(b),
                    }
                } else {
                    return Err(format!("{}: t{} (stage {st}) not bound by a code argument", t.name, x.slot));
                };
                let ty = match (what, x.kind) {
                    (Bind::Bones | Bind::Instances | Bind::CodeBuffer(_), _) | (_, dxbc_sm5::TextureKind::RawBuffer) => wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Storage { read_only: true }, has_dynamic_offset: false, min_binding_size: None },
                    (Bind::Texture(id), k) => wgpu::BindingType::Texture {
                        sample_type: match k {
                            dxbc_sm5::TextureKind::Sint => wgpu::TextureSampleType::Sint,
                            dxbc_sm5::TextureKind::Uint => wgpu::TextureSampleType::Uint,
                            dxbc_sm5::TextureKind::Depth => wgpu::TextureSampleType::Depth,
                            _ => wgpu::TextureSampleType::Float { filterable: !unfilterable.contains(&id) },
                        },
                        view_dimension: match x.dim {
                            dxbc_sm5::TextureDim::D3 => wgpu::TextureViewDimension::D3,
                            dxbc_sm5::TextureDim::Cube => wgpu::TextureViewDimension::Cube,
                            dxbc_sm5::TextureDim::D2Array => wgpu::TextureViewDimension::D2Array,
                            _ => wgpu::TextureViewDimension::D2,
                        },
                        multisampled: false,
                    },
                    _ => unreachable!(),
                };
                entries[1].push(wgpu::BindGroupLayoutEntry { binding: x.binding, visibility: vis, ty, count: None });
                binds.push((1, x.binding, what));
            }
            for s in &r.samplers {
                let what = match arg(0x300, st, s.slot) {
                    Some(a) => Bind::Sampler(t.samplers.iter().find(|(h, _)| *h == a.u[0]).map(|(_, s)| *s).ok_or_else(|| format!("{}: sampler {:08x} not in the material", t.name, a.u[0]))?),
                    // (No material argument: the renderer's own sampler at
                    // that slot, by the program's name for it.)
                    None => {
                        let n = rdef_names[st as usize].iter().find(|n| n.kind == crate::rdef::SAMPLER && n.point == s.slot).map(|n| n.name.clone()).ok_or_else(|| format!("{}: s{} (stage {st}) not bound", t.name, s.slot))?;
                        named_samplers.push(n);
                        Bind::NamedSampler(named_samplers.len() - 1)
                    }
                };
                let ty = if s.comparison {
                    wgpu::SamplerBindingType::Comparison
                } else if nonfiltering {
                    wgpu::SamplerBindingType::NonFiltering
                } else {
                    wgpu::SamplerBindingType::Filtering
                };
                entries[2].push(wgpu::BindGroupLayoutEntry { binding: s.binding, visibility: vis, ty: wgpu::BindingType::Sampler(ty), count: None });
                binds.push((2, s.binding, what));
            }
        }
        let dev = &gpu.device;
        let scope = dev.push_error_scope(wgpu::ErrorFilter::Validation);
        let mk = |g: usize| dev.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor { label: Some(&t.name), entries: &entries[g] });
        let layouts = [mk(0), mk(1), mk(2)];
        let pl = dev.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: Some(&t.name), bind_group_layouts: &[Some(&layouts[0]), Some(&layouts[1]), Some(&layouts[2])], immediate_size: 0 });
        let attrs: Vec<wgpu::VertexAttribute> = vs.reflection.inputs.iter().map(|i| vertex_attribute(&i.semantic, i.index, i.component_type, i.location)).collect::<Result<_, _>>()?;
        let vm = dev.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some(&t.name), source: wgpu::ShaderSource::Wgsl(vs.wgsl.clone().into()) });
        let pm = dev.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some(&t.name), source: wgpu::ShaderSource::Wgsl(ps.wgsl.clone().into()) });
        let mut targets = crate::state::targets(&t.state, formats)?;
        let written: Vec<bool> = (0..targets.len()).map(|i| ps.reflection.targets.is_empty() || ps.reflection.targets.iter().any(|&o| o as usize == i)).collect();
        for (t, &w) in targets.iter_mut().zip(&written) {
            if !w {
                *t = None;
            }
        }
        let pipeline = dev.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some(&t.name),
            layout: Some(&pl),
            vertex: wgpu::VertexState { module: &vm, entry_point: Some("main"), compilation_options: Default::default(), buffers: &[wgpu::VertexBufferLayout { array_stride: 32, step_mode: wgpu::VertexStepMode::Vertex, attributes: &attrs }] },
            primitive: crate::state::primitive(&t.state)?,
            depth_stencil: depth.map(|f| crate::state::depth_stencil(&t.state, f)).transpose()?,
            multisample: Default::default(),
            fragment: Some(wgpu::FragmentState { module: &pm, entry_point: Some("main"), compilation_options: Default::default(), targets: &targets }),
            multiview_mask: None,
            cache: crate::pipecache::get(&dev).as_ref(),
        });
        if let Some(e) = pollster::block_on(scope.pop()) {
            return Err(format!("{}: {e}", t.name));
        }
        Ok(Self { name: t.name.clone(), pipeline, layouts, binds, code_textures, named_samplers, written })
    }

    /// The bind groups for one draw: `per_scene` and `post_fx` the constant
    /// buffers, `texture(id)` the view for a code texture, `sampler(state)`.
    pub fn bind<'a>(
        &self,
        gpu: &Gpu,
        per_scene: &wgpu::Buffer,
        post_fx: &wgpu::Buffer,
        buffers: &CodeBuffers,
        texture: impl FnMut(u32) -> Option<&'a wgpu::TextureView>,
        sampler: impl FnMut(u16) -> Option<&'a wgpu::Sampler>,
    ) -> Result<[wgpu::BindGroup; 3], String> {
        self.bind_with(gpu, per_scene, post_fx, &[], buffers, texture, sampler)
    }

    /// [`Self::bind`] with other constant buffers by slot (`extra`).
    #[allow(clippy::too_many_arguments)]
    pub fn bind_with<'a>(
        &self,
        gpu: &Gpu,
        per_scene: &wgpu::Buffer,
        post_fx: &wgpu::Buffer,
        extra: &[(u32, &wgpu::Buffer)],
        buffers: &CodeBuffers,
        texture: impl FnMut(u32) -> Option<&'a wgpu::TextureView>,
        sampler: impl FnMut(u16) -> Option<&'a wgpu::Sampler>,
    ) -> Result<[wgpu::BindGroup; 3], String> {
        self.bind_full(gpu, per_scene, post_fx, extra, &[], buffers, texture, sampler, |_| None)
    }

    /// [`Self::bind_with`] with code buffers by id (`code_buffers`) and the
    /// samplers bound by name (`named`).
    #[allow(clippy::too_many_arguments)]
    pub fn bind_full<'a>(
        &self,
        gpu: &Gpu,
        per_scene: &wgpu::Buffer,
        post_fx: &wgpu::Buffer,
        extra: &[(u32, &wgpu::Buffer)],
        code_buffers: &[(u32, &wgpu::Buffer)],
        buffers: &CodeBuffers,
        mut texture: impl FnMut(u32) -> Option<&'a wgpu::TextureView>,
        mut sampler: impl FnMut(u16) -> Option<&'a wgpu::Sampler>,
        mut named: impl FnMut(&str) -> Option<&'a wgpu::Sampler>,
    ) -> Result<[wgpu::BindGroup; 3], String> {
        let mut per: [Vec<wgpu::BindGroupEntry>; 3] = Default::default();
        for &(g, binding, what) in &self.binds {
            let resource = match what {
                Bind::PerScene => per_scene.as_entire_binding(),
                Bind::PostFx => post_fx.as_entire_binding(),
                Bind::Extra(slot) => extra.iter().find(|(s, _)| *s == slot).map(|(_, b)| b.as_entire_binding()).ok_or_else(|| format!("{}: constant buffer b{slot} not given", self.name))?,
                Bind::Bones => buffers.bones.as_entire_binding(),
                Bind::Instances => buffers.instances.as_entire_binding(),
                Bind::CodeBuffer(id) => code_buffers.iter().find(|(b, _)| *b == id).map(|(_, b)| b.as_entire_binding()).ok_or_else(|| format!("{}: code buffer {id:#x} not given", self.name))?,
                Bind::NamedSampler(k) => wgpu::BindingResource::Sampler(named(&self.named_samplers[k]).ok_or_else(|| format!("{}: sampler {} not made", self.name, self.named_samplers[k]))?),
                Bind::Texture(id) => wgpu::BindingResource::TextureView(texture(id).ok_or_else(|| format!("{}: code texture {id:#x} not made", self.name))?),
                Bind::Sampler(s) => wgpu::BindingResource::Sampler(sampler(s).ok_or_else(|| format!("{}: sampler state {s:#x} not made", self.name))?),
            };
            per[g as usize].push(wgpu::BindGroupEntry { binding, resource });
        }
        let dev = &gpu.device;
        let scope = dev.push_error_scope(wgpu::ErrorFilter::Validation);
        let groups = std::array::from_fn(|g| dev.create_bind_group(&wgpu::BindGroupDescriptor { label: Some(&self.name), layout: &self.layouts[g], entries: &per[g] }));
        if let Some(e) = pollster::block_on(scope.pop()) {
            return Err(format!("{}: bind groups: {e}", self.name));
        }
        Ok(groups)
    }

    /// The sampler states it binds.
    pub fn sampler_states(&self) -> Vec<u16> {
        self.binds.iter().filter_map(|b| if let Bind::Sampler(s) = b.2 { Some(s) } else { None }).collect()
    }

    /// Records the draw into `targets` (the exe's triangle `vertices` for
    /// the targets' size).
    pub fn draw(&self, enc: &mut wgpu::CommandEncoder, targets: &[&wgpu::TextureView], groups: &[wgpu::BindGroup; 3], vertices: &wgpu::Buffer) {
        let att: Vec<Option<wgpu::RenderPassColorAttachment>> = targets
            .iter()
            .enumerate()
            .map(|(i, v)| self.written.get(i).copied().unwrap_or(true).then(|| wgpu::RenderPassColorAttachment { view: v, resolve_target: None, depth_slice: None, ops: wgpu::Operations { load: wgpu::LoadOp::Load, store: wgpu::StoreOp::Store } }))
            .collect();
        let mut pass = enc.render_pass_p(&wgpu::RenderPassDescriptor { label: Some(&self.name), color_attachments: &att, depth_stencil_attachment: None, timestamp_writes: None, occlusion_query_set: None, multiview_mask: None });
        pass.set_pipeline_p(&self.pipeline);
        for (i, g) in groups.iter().enumerate() {
            pass.set_bind_group_p(i as u32, g, &[]);
        }
        pass.set_vertex_buffer(0, vertices.slice(..));
        pass.draw_p(0..3, 0..1);
    }

    /// Records the draw into a depth target only (`load`: keep its
    /// contents, else clear to `clear`).
    pub fn draw_depth(&self, enc: &mut wgpu::CommandEncoder, depth: &wgpu::TextureView, clear: Option<f32>, groups: &[wgpu::BindGroup; 3], vertices: &wgpu::Buffer) {
        let load = clear.map_or(wgpu::LoadOp::Load, wgpu::LoadOp::Clear);
        let mut pass = enc.render_pass_p(&wgpu::RenderPassDescriptor {
            label: Some(&self.name),
            color_attachments: &[],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment { view: depth, depth_ops: Some(wgpu::Operations { load, store: wgpu::StoreOp::Store }), stencil_ops: None }),
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        pass.set_pipeline_p(&self.pipeline);
        for (i, g) in groups.iter().enumerate() {
            pass.set_bind_group_p(i as u32, g, &[]);
        }
        pass.set_vertex_buffer(0, vertices.slice(..));
        pass.draw_p(0..3, 0..1);
    }
}
