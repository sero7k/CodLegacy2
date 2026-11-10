//! Full-screen passes with MWR's own post techniques (fog, tone map,
//! film): the technique's shaders and argument table, a triangle over the
//! screen, the engine's textures (`RESOLVED_SCENE`, `FLOATZ`) and values.

use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

use dxbc_sm5::{Options, Stage, TextureKind};
use fastfile_h1::AssetType;

use crate::bind::{self, CbSource, MaterialView, SampSource, TexSource};
use crate::code::CodeValues;
use crate::gpu::{self, Gpu};
use crate::pool::Pool;

struct Pipe {
    pipeline: wgpu::RenderPipeline,
    layout: gpu::PassLayout,
    regs: BTreeMap<u32, u32>,
    pass: fastfile_h1::walk::PassRec,
}

pub struct Post {
    pub gpu: Gpu,
    pub pool: Arc<Pool>,
    pipes: HashMap<(String, wgpu::TextureFormat), Pipe>,
    quad: wgpu::Buffer,
    sampler: wgpu::Sampler,
    point: wgpu::Sampler,
}

/// A triangle over the screen: clip position, then uv (top-left 0).
const QUAD: [f32; 15] = [
    -1.0, -1.0, 0.0, 0.0, 1.0, 3.0, -1.0, 0.0, 2.0, 1.0, -1.0, 3.0, 0.0, 0.0, -1.0,
];

impl Post {
    pub fn new(gpu: Gpu, pool: Arc<Pool>) -> Self {
        let quad = wgpu::util::DeviceExt::create_buffer_init(
            &gpu.device,
            &wgpu::util::BufferInitDescriptor {
                label: Some("post_quad"),
                contents: bytemuck::cast_slice(&QUAD),
                usage: wgpu::BufferUsages::VERTEX,
            },
        );
        let point = gpu.device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("point"),
            ..Default::default()
        });
        Self {
            pipes: HashMap::new(),
            quad,
            sampler: gpu::linear_clamp(&gpu),
            point,
            gpu,
            pool,
        }
    }

    fn pipe(&mut self, technique: &str, format: wgpu::TextureFormat) -> Result<&Pipe, String> {
        let k = (technique.to_string(), format);
        if !self.pipes.contains_key(&k) {
            let p = self.make(technique, format)?;
            self.pipes.insert(k.clone(), p);
        }
        Ok(&self.pipes[&k])
    }

    fn make(&self, technique: &str, format: wgpu::TextureFormat) -> Result<Pipe, String> {
        let tech = self
            .pool
            .technique(technique)
            .ok_or_else(|| format!("technique {technique} not loaded"))?;
        let pass = tech.pass_info.first().ok_or("no pass")?.clone();
        let vs_name = pass.vertex_shader.clone().ok_or("no vs")?;
        let ps_name = pass.pixel_shader.clone().ok_or("no ps")?;
        let vs_b = self
            .pool
            .program(AssetType::VertexShader, &vs_name)
            .ok_or_else(|| format!("{vs_name} not loaded"))?;
        let ps_b = self
            .pool
            .program(AssetType::PixelShader, &ps_name)
            .ok_or_else(|| format!("{ps_name} not loaded"))?;
        let vs_c = dxbc_sm5::Container::parse(vs_b).map_err(|e| format!("{e:?}"))?;
        let ps_c = dxbc_sm5::Container::parse(ps_b).map_err(|e| format!("{e:?}"))?;
        let varyings = dxbc_sm5::link(&vs_c.outputs, &ps_c.inputs);
        let mut po = Options::for_stage(Stage::Pixel);
        po.varyings = varyings.clone();
        let ps = dxbc_sm5::translate(ps_b, &po).map_err(|e| format!("{ps_name}: {e:?}"))?;
        let mut vo = Options::for_stage(Stage::Vertex);
        vo.varyings = varyings;
        vo.output_interp = ps
            .reflection
            .inputs
            .iter()
            .map(|i| (i.semantic.to_ascii_uppercase(), i.index, i.interp))
            .collect();
        let vs = dxbc_sm5::translate(vs_b, &vo).map_err(|e| format!("{vs_name}: {e:?}"))?;
        let mut attrs = Vec::new();
        for i in &vs.reflection.inputs {
            let (offset, format) = match (i.semantic.to_ascii_uppercase().as_str(), i.index) {
                ("POSITION" | "SV_POSITION", 0) => (0, wgpu::VertexFormat::Float32x3),
                ("TEXCOORD", 0) => (12, wgpu::VertexFormat::Float32x2),
                (s, n) => return Err(format!("{technique}: vertex input {s}{n}")),
            };
            attrs.push(wgpu::VertexAttribute {
                format,
                offset,
                shader_location: i.location,
            });
        }
        let dev = &self.gpu.device;
        let scope = dev.push_error_scope(wgpu::ErrorFilter::Validation);
        let vm = dev.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some(&vs_name),
            source: wgpu::ShaderSource::Wgsl(vs.wgsl.into()),
        });
        let pm = dev.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some(&ps_name),
            source: wgpu::ShaderSource::Wgsl(ps.wgsl.into()),
        });
        let layout = gpu::pass_layout(&self.gpu, &vs.reflection, &ps.reflection, None);
        let pipeline = dev.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some(technique),
            layout: Some(&layout.pipeline_layout),
            vertex: wgpu::VertexState {
                module: &vm,
                entry_point: Some("main"),
                compilation_options: Default::default(),
                buffers: &[wgpu::VertexBufferLayout {
                    array_stride: 20,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &attrs,
                }],
            },
            primitive: Default::default(),
            depth_stencil: None,
            multisample: Default::default(),
            fragment: Some(wgpu::FragmentState {
                module: &pm,
                entry_point: Some("main"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        });
        if let Some(e) = pollster::block_on(scope.pop()) {
            return Err(format!("{technique}: {e}"));
        }
        let mut regs = BTreeMap::new();
        for (c, is_ps) in vs
            .reflection
            .cbuffers
            .iter()
            .map(|c| (c, false))
            .chain(ps.reflection.cbuffers.iter().map(|c| (c, true)))
        {
            let e = regs.entry(bind::buffer_key(c.slot, is_ps)).or_insert(0);
            *e = (*e).max(c.registers);
        }
        Ok(Pipe {
            pipeline,
            layout,
            regs,
            pass,
        })
    }

    /// Runs `technique` over the whole of `target`.
    ///
    /// `textures`: the code textures it reads, by name (`RESOLVED_SCENE`,
    /// `FLOATZ`, ...). `custom`: constant buffers the engine fills itself
    /// rather than through the argument table (the tone map's `b4`).
    pub fn run(
        &mut self,
        technique: &str,
        textures: &HashMap<&str, &wgpu::TextureView>,
        code: &CodeValues,
        custom: &[(u32, Vec<[f32; 4]>)],
        target: &wgpu::TextureView,
        format: wgpu::TextureFormat,
    ) -> Result<(), String> {
        let mut enc = self.gpu.device.create_command_encoder(&Default::default());
        self.record(&mut enc, technique, textures, code, custom, target, format)?;
        self.gpu.queue.submit([enc.finish()]);
        Ok(())
    }

    /// `run`, recorded into `enc`.
    #[allow(clippy::too_many_arguments)]
    pub fn record(
        &mut self,
        enc: &mut wgpu::CommandEncoder,
        technique: &str,
        textures: &HashMap<&str, &wgpu::TextureView>,
        code: &CodeValues,
        custom: &[(u32, Vec<[f32; 4]>)],
        target: &wgpu::TextureView,
        format: wgpu::TextureFormat,
    ) -> Result<(), String> {
        self.pipe(technique, format)?;
        let p = &self.pipes[&(technique.to_string(), format)];
        let mv = MaterialView {
            textures: &[],
            constants: &[],
        };
        let binds = bind::resolve(&p.pass, &mv);
        // Constant buffers: the argument table's, then the custom ones.
        let mut bufs: BTreeMap<u32, Vec<[f32; 4]>> = p
            .regs
            .iter()
            .map(|(s, n)| (*s, vec![[0.0; 4]; *n as usize]))
            .collect();
        for w in &binds.cbuffer {
            let rows: Vec<[f32; 4]> = match &w.src {
                CbSource::Code {
                    index,
                    first_row,
                    rows,
                } => (0..*rows as usize)
                    .map(|r| code.row(*index, *first_row as usize + r))
                    .collect(),
                CbSource::Literal(v) | CbSource::Material(v) => vec![*v],
                CbSource::MissingMaterial(_) => vec![[0.0; 4]],
            };
            let b = bufs.entry(w.buffer).or_default();
            let end = w.dest as usize + rows.len();
            if b.len() < end {
                b.resize(end, [0.0; 4]);
            }
            b[w.dest as usize..end].copy_from_slice(&rows);
        }
        // (The custom buffers are the pixel stage's.)
        for (slot, rows) in custom {
            let b = bufs.entry(bind::buffer_key(*slot, true)).or_default();
            if b.len() < rows.len() {
                b.resize(rows.len(), [0.0; 4]);
            }
            b[..rows.len()].copy_from_slice(rows);
        }
        let gpu = &self.gpu;
        let ubufs: BTreeMap<u32, wgpu::Buffer> = bufs
            .iter()
            .map(|(s, r)| (*s, gpu::uniform(gpu, r)))
            .collect();
        let cb_entries: Vec<wgpu::BindGroupEntry> = p
            .layout
            .cbuffers
            .iter()
            .filter_map(|&(binding, slot, _)| {
                ubufs.get(&bind::buffer_key(slot, binding >= 32)).map(|b| wgpu::BindGroupEntry {
                    binding,
                    resource: b.as_entire_binding(),
                })
            })
            .collect();
        let mut views = Vec::new();
        for &(binding, slot, _dim, kind, is_ps) in &p.layout.textures {
            let src = if is_ps {
                binds.ps.textures.get(&slot)
            } else {
                binds.vs.textures.get(&slot)
            };
            let name = match src {
                Some(TexSource::Code(c)) => fastfile_h1::code_source::code_texture_name(*c),
                _ => "",
            };
            let v = textures
                .get(name)
                .ok_or_else(|| format!("{technique}: no code texture {name} (t{slot})"))?;
            if kind == TextureKind::Depth {
                return Err(format!("{technique}: depth texture input"));
            }
            views.push((binding, (*v).clone()));
        }
        let samps: Vec<(u32, wgpu::Sampler)> = p
            .layout
            .samplers
            .iter()
            .map(|&(binding, slot, is_ps)| {
                let src = if is_ps {
                    binds.ps.samplers.get(&slot)
                } else {
                    binds.vs.samplers.get(&slot)
                };
                let s = match src {
                    Some(SampSource::Code(c))
                        if fastfile_h1::code_source::code_texture_name(*c) == "FLOATZ" =>
                    {
                        self.point.clone()
                    }
                    _ => self.sampler.clone(),
                };
                (binding, s)
            })
            .collect();
        let dev = &gpu.device;
        let g0 = dev.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("post_cb"),
            layout: &p.layout.groups[0],
            entries: &cb_entries,
        });
        let g1 = dev.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("post_tex"),
            layout: &p.layout.groups[1],
            entries: &views
                .iter()
                .map(|(b, v)| wgpu::BindGroupEntry {
                    binding: *b,
                    resource: wgpu::BindingResource::TextureView(v),
                })
                .collect::<Vec<_>>(),
        });
        let g2 = dev.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("post_samp"),
            layout: &p.layout.groups[2],
            entries: &samps
                .iter()
                .map(|(b, s)| wgpu::BindGroupEntry {
                    binding: *b,
                    resource: wgpu::BindingResource::Sampler(s),
                })
                .collect::<Vec<_>>(),
        });
        {
            let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some(technique),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: target,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                    depth_slice: None,
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_pipeline(&p.pipeline);
            pass.set_bind_group(0, &g0, &[]);
            pass.set_bind_group(1, &g1, &[]);
            pass.set_bind_group(2, &g2, &[]);
            pass.set_vertex_buffer(0, self.quad.slice(..));
            pass.draw(0..3, 0..1);
        }
        Ok(())
    }
}

/// MWR's `FLOATZ`: near / view distance, sky (infinity) 0. Our reversed
/// infinite depth already holds exactly that, so it is a copy to R32F.
pub fn floatz(gpu: &Gpu, depth: &wgpu::Texture, w: u32, h: u32) -> wgpu::Texture {
    let out = target(gpu, "floatz", w, h, wgpu::TextureFormat::R32Float);
    let dv = depth.create_view(&wgpu::TextureViewDescriptor {
        aspect: wgpu::TextureAspect::DepthOnly,
        ..Default::default()
    });
    let ov = out.create_view(&Default::default());
    let mut enc = gpu.device.create_command_encoder(&Default::default());
    FloatZ::new(gpu).record(gpu, &mut enc, &dv, &ov);
    gpu.queue.submit([enc.finish()]);
    out
}

/// The copy of a depth buffer into `FLOATZ` (R32Float).
pub struct FloatZ {
    pipeline: wgpu::RenderPipeline,
    /// The bind group over the depth view of the last copy (made again when
    /// the view is another).
    group: std::sync::Mutex<Option<(wgpu::TextureView, wgpu::BindGroup)>>,
}

impl FloatZ {
    pub fn new(gpu: &Gpu) -> Self {
        let dev = &gpu.device;
        let module = dev.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("floatz"),
            source: wgpu::ShaderSource::Wgsl(
                "@group(0) @binding(0) var d: texture_depth_2d;\n\
                 @vertex fn vs(@builtin(vertex_index) i: u32) -> @builtin(position) vec4<f32> {\n\
                     let p = vec2<f32>(f32(i == 1u) * 4.0 - 1.0, f32(i == 2u) * 4.0 - 1.0);\n\
                     return vec4<f32>(p, 0.0, 1.0);\n\
                 }\n\
                 @fragment fn fs(@builtin(position) p: vec4<f32>) -> @location(0) vec4<f32> {\n\
                     return vec4<f32>(textureLoad(d, vec2<i32>(p.xy), 0), 0.0, 0.0, 1.0);\n\
                 }\n"
                .into(),
            ),
        });
        let pipeline = dev.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("floatz"),
            layout: None,
            vertex: wgpu::VertexState {
                module: &module,
                entry_point: Some("vs"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            primitive: Default::default(),
            depth_stencil: None,
            multisample: Default::default(),
            fragment: Some(wgpu::FragmentState {
                module: &module,
                entry_point: Some("fs"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::TextureFormat::R32Float.into())],
            }),
            multiview_mask: None,
            cache: None,
        });
        Self { pipeline, group: std::sync::Mutex::new(None) }
    }

    /// `depth`: a depth-only view of the depth buffer.
    pub fn record(
        &self,
        gpu: &Gpu,
        enc: &mut wgpu::CommandEncoder,
        depth: &wgpu::TextureView,
        out: &wgpu::TextureView,
    ) {
        let mut cached = self.group.lock().unwrap_or_else(|e| e.into_inner());
        if cached.as_ref().is_none_or(|(v, _)| v != depth) {
            let bg = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("floatz"),
                layout: &self.pipeline.get_bind_group_layout(0),
                entries: &[wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(depth),
                }],
            });
            *cached = Some((depth.clone(), bg));
        }
        let Some((_, bg)) = cached.as_ref() else { return };
        let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("floatz"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: out,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                    store: wgpu::StoreOp::Store,
                },
                depth_slice: None,
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, bg, &[]);
        pass.draw(0..3, 0..1);
    }
}

/// An HDR or display target the size of the view.
pub fn target(
    gpu: &Gpu,
    label: &str,
    w: u32,
    h: u32,
    format: wgpu::TextureFormat,
) -> wgpu::Texture {
    gpu.device.create_texture(&wgpu::TextureDescriptor {
        label: Some(label),
        size: wgpu::Extent3d {
            width: w,
            height: h,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT
            | wgpu::TextureUsages::TEXTURE_BINDING
            | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    })
}

/// Reads back an R32Float texture (checks and tools).
pub fn read_r32f(gpu: &Gpu, tex: &wgpu::Texture, w: u32, h: u32) -> Vec<f32> {
    let row = (w * 4).div_ceil(256) * 256;
    let buf = gpu.device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("readback_r32f"),
        size: u64::from(row * h),
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut enc = gpu.device.create_command_encoder(&Default::default());
    enc.copy_texture_to_buffer(
        tex.as_image_copy(),
        wgpu::TexelCopyBufferInfo {
            buffer: &buf,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(row),
                rows_per_image: Some(h),
            },
        },
        wgpu::Extent3d {
            width: w,
            height: h,
            depth_or_array_layers: 1,
        },
    );
    gpu.queue.submit([enc.finish()]);
    let slice = buf.slice(..);
    slice.map_async(wgpu::MapMode::Read, |_| {});
    let _ = gpu.device.poll(wgpu::PollType::wait_indefinitely());
    let data = slice.get_mapped_range();
    let mut out = Vec::with_capacity((w * h) as usize);
    for y in 0..h {
        for x in 0..w {
            let at = (y * row + x * 4) as usize;
            out.push(f32::from_le_bytes([
                data[at],
                data[at + 1],
                data[at + 2],
                data[at + 3],
            ]));
        }
    }
    out
}
