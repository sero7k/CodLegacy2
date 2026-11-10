//! Full-screen passes with Ghosts' own post techniques (`postopaquefx_*`
//! fog, `postfx_macroclut` tone map and grade, the bloom passes): the
//! technique's programs and argument table, a triangle over the screen,
//! the engine's textures (`RESOLVED_SCENE`, `FLOATZ`, `MACROCLUT`,
//! `DYNAMIC_IRIS_EXPOSURE`) and values. The engine's own small passes
//! that have no technique (the depth copy into `FLOATZ`, the luminance
//! reduction and the iris step) are here too, in WGSL.

use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

use dxbc_sm5::{Options, Stage, TextureKind};
use fastfile_iw6::AssetType;
use render_h1::gpu::{self, Gpu};

use crate::bind::{self, CbSource, MaterialView, SampSource, TexSource};
use crate::code::{CodeValues, code_texture_name};
use crate::pool::Pool;

struct Pipe {
    pipeline: wgpu::RenderPipeline,
    layout: gpu::PassLayout,
    regs: BTreeMap<u32, u32>,
    /// The pass's bindings (resolved once: they depend on the technique alone).
    binds: bind::PassBinds,
    /// The code texture each of the layout's texture slots reads.
    names: Vec<&'static str>,
}

/// A pass's bind groups for one set of textures, its constant buffers
/// rewritten each frame (made once per textures bound, not per frame).
struct Bound {
    views: Vec<wgpu::TextureView>,
    bufs: BTreeMap<u32, wgpu::Buffer>,
    groups: [wgpu::BindGroup; 3],
    /// The constants last written.
    last: BTreeMap<u32, Vec<[f32; 4]>>,
}

/// One technique's pipeline for a target format and blend, the groups made
/// for it and the buffers its constants are built in (kept: a frame builds
/// them again without allocating).
struct Slot {
    format: wgpu::TextureFormat,
    add: bool,
    pipe: Pipe,
    bound: Vec<Bound>,
    scratch: BTreeMap<u32, Vec<[f32; 4]>>,
}

pub struct Post {
    pub gpu: Gpu,
    pub pool: Arc<Pool>,
    /// By technique name; a name holds a slot per format and blend.
    slots: HashMap<String, Vec<Slot>>,
    /// The views the pass being recorded reads (kept between passes).
    views: Vec<wgpu::TextureView>,
    /// Constant bytes written since the profile last took them.
    pub bytes: u64,
    quad: wgpu::Buffer,
    linear: wgpu::Sampler,
    point: wgpu::Sampler,
    /// Unfiltered and wrapped (`RANDOM_ROTATIONS`).
    repeat: wgpu::Sampler,
}

/// A triangle over the screen: clip position (x, y, z, w), then uv
/// (top-left 0).
const QUAD: [f32; 18] = [
    -1.0, -1.0, 0.0, 1.0, 0.0, 1.0, //
    3.0, -1.0, 0.0, 1.0, 2.0, 1.0, //
    -1.0, 3.0, 0.0, 1.0, 0.0, -1.0,
];

impl Post {
    pub fn new(gpu: Gpu, pool: Arc<Pool>) -> Self {
        let quad = wgpu::util::DeviceExt::create_buffer_init(
            &gpu.device,
            &wgpu::util::BufferInitDescriptor {
                label: Some("iw6_post_quad"),
                contents: bytemuck::cast_slice(&QUAD),
                usage: wgpu::BufferUsages::VERTEX,
            },
        );
        let point = gpu.device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("iw6_point"),
            ..Default::default()
        });
        let repeat = gpu.device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("iw6_repeat"),
            address_mode_u: wgpu::AddressMode::Repeat,
            address_mode_v: wgpu::AddressMode::Repeat,
            address_mode_w: wgpu::AddressMode::Repeat,
            ..Default::default()
        });
        Self {
            slots: HashMap::new(),
            views: Vec::new(),
            bytes: 0,
            quad,
            linear: gpu::linear_clamp(&gpu),
            repeat,
            point,
            gpu,
            pool,
        }
    }

    /// Whether the zones hold a technique of this name.
    pub fn has(&self, technique: &str) -> bool {
        self.pool.technique(technique).is_some()
    }

    fn make(&self, technique: &str, format: wgpu::TextureFormat, add: bool) -> Result<Pipe, String> {
        let tech = self
            .pool
            .technique(technique)
            .ok_or_else(|| format!("technique {technique} not loaded"))?;
        let pass = tech.passes.first().ok_or("no pass")?.clone();
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
        let mut ps = dxbc_sm5::translate(ps_b, &po).map_err(|e| format!("{ps_name}: {e:?}"))?;
        // IW6_DBG_PSOUT=<technique>|<wgsl vec4>: that pass writes the
        // expression instead (debug).
        if let Ok(v) = std::env::var("IW6_DBG_PSOUT")
            && let Some((t, e)) = v.split_once('|')
            && t == technique
        {
            ps.wgsl = ps
                .wgsl
                .replace("output.sv_t0 = bitcast<vec4<f32>>(o0);", &format!("output.sv_t0 = {e};"));
        }
        let mut vo = Options::for_stage(Stage::Vertex);
        vo.varyings = varyings;
        vo.output_interp = ps
            .reflection
            .inputs
            .iter()
            .map(|i| (i.semantic.to_ascii_uppercase(), i.index, i.interp))
            .collect();
        let vs = dxbc_sm5::translate(vs_b, &vo).map_err(|e| format!("{vs_name}: {e:?}"))?;
        if let Ok(dir) = std::env::var("IW6_DUMP_WGSL")
            && std::env::var("IW6_DUMP_MATCH").is_ok_and(|m| technique.contains(m.as_str()))
        {
            let _ = std::fs::write(format!("{dir}/post_vs_{technique}.wgsl"), &vs.wgsl);
            let _ = std::fs::write(format!("{dir}/post_ps_{technique}.wgsl"), &ps.wgsl);
        }
        let mut attrs = Vec::new();
        for i in &vs.reflection.inputs {
            let (offset, format) = match (i.semantic.to_ascii_uppercase().as_str(), i.index) {
                ("POSITION" | "SV_POSITION", 0) => (0, wgpu::VertexFormat::Float32x4),
                ("TEXCOORD", 0) => (16, wgpu::VertexFormat::Float32x2),
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
                    array_stride: 24,
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
                targets: &[Some(if add {
                    // Added over what the target holds, the source's alpha
                    // darkening it (the bloom's apply, its alpha is the sun's
                    // blind; with alpha 0 it is a plain add. Its blend state
                    // is not read: INFERRED).
                    wgpu::ColorTargetState {
                        format,
                        blend: Some(wgpu::BlendState {
                            color: wgpu::BlendComponent {
                                src_factor: wgpu::BlendFactor::One,
                                dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
                                operation: wgpu::BlendOperation::Add,
                            },
                            alpha: wgpu::BlendComponent {
                                src_factor: wgpu::BlendFactor::Zero,
                                dst_factor: wgpu::BlendFactor::One,
                                operation: wgpu::BlendOperation::Add,
                            },
                        }),
                        write_mask: wgpu::ColorWrites::ALL,
                    }
                } else {
                    wgpu::ColorTargetState {
                        format,
                        blend: None,
                        write_mask: wgpu::ColorWrites::ALL,
                    }
                })],
            }),
            multiview_mask: None,
            cache: None,
        });
        if let Some(e) = pollster::block_on(scope.pop()) {
            return Err(format!("{technique}: {e}"));
        }
        let mut regs = BTreeMap::new();
        for c in vs.reflection.cbuffers.iter().chain(&ps.reflection.cbuffers) {
            let e = regs.entry(c.slot).or_insert(0);
            *e = (*e).max(c.registers);
        }
        let binds = bind::resolve(
            &pass,
            &MaterialView {
                textures: &[],
                constants: &[],
            },
        );
        let names = layout
            .textures
            .iter()
            .map(|&(_binding, slot, _dim, _kind, is_ps)| {
                let src = if is_ps { binds.ps.textures.get(&slot) } else { binds.vs.textures.get(&slot) };
                match src {
                    Some(TexSource::Code(c)) => code_texture_name(*c),
                    _ => "",
                }
            })
            .collect();
        Ok(Pipe {
            pipeline,
            layout,
            regs,
            binds,
            names,
        })
    }

    /// Makes `technique`'s pipeline now (at load); false if it does not
    /// build (the frame reports it then).
    pub fn warm(&mut self, technique: &str, format: wgpu::TextureFormat, add: bool) -> bool {
        self.slot(technique, format, add).is_ok()
    }

    /// The index of the slot for `technique` in `format` / `add`, made if it
    /// is not there yet.
    fn slot(&mut self, technique: &str, format: wgpu::TextureFormat, add: bool) -> Result<usize, String> {
        if let Some(i) = self.slots.get(technique).and_then(|l| l.iter().position(|s| s.format == format && s.add == add)) {
            return Ok(i);
        }
        let pipe = self.make(technique, format, add)?;
        let scratch = pipe.regs.iter().map(|(s, n)| (*s, vec![[0.0; 4]; *n as usize])).collect();
        let list = self.slots.entry(technique.to_string()).or_default();
        list.push(Slot { format, add, pipe, bound: Vec::new(), scratch });
        Ok(list.len() - 1)
    }

    /// Records `technique` over the whole of `target`. `textures`: the
    /// code textures it reads, by name.
    pub fn record(
        &mut self,
        enc: &mut wgpu::CommandEncoder,
        technique: &str,
        textures: &[(&str, &wgpu::TextureView)],
        code: &CodeValues,
        target: &wgpu::TextureView,
        format: wgpu::TextureFormat,
    ) -> Result<(), String> {
        self.record_with(enc, technique, textures, code, target, format, false)
    }

    /// `record`; `add`: added over the target's contents (no clear).
    #[allow(clippy::too_many_arguments)]
    pub fn record_with(
        &mut self,
        enc: &mut wgpu::CommandEncoder,
        technique: &str,
        textures: &[(&str, &wgpu::TextureView)],
        code: &CodeValues,
        target: &wgpu::TextureView,
        format: wgpu::TextureFormat,
        add: bool,
    ) -> Result<(), String> {
        let si = self.slot(technique, format, add)?;
        let Self { gpu, slots, views, bytes, quad, linear, point, repeat, .. } = self;
        let slot = &mut slots.get_mut(technique).ok_or("slot")?[si];
        let Slot { pipe: p, bound, scratch: bufs, .. } = slot;
        // The constants this pass reads, built in the slot's own buffers.
        for rows in bufs.values_mut() {
            rows.fill([0.0; 4]);
        }
        for w in &p.binds.cbuffer {
            let b = bufs.entry(w.buffer).or_default();
            let dest = w.dest as usize;
            match &w.src {
                CbSource::Code { index, first_row, rows } => {
                    let end = dest + *rows as usize;
                    if b.len() < end {
                        b.resize(end, [0.0; 4]);
                    }
                    for r in 0..*rows as usize {
                        b[dest + r] = code.row(*index, *first_row as usize + r);
                    }
                }
                CbSource::Literal(v) | CbSource::Material(v) => {
                    if b.len() < dest + 1 {
                        b.resize(dest + 1, [0.0; 4]);
                    }
                    b[dest] = *v;
                }
                CbSource::MissingMaterial(_) => {
                    if b.len() < dest + 1 {
                        b.resize(dest + 1, [0.0; 4]);
                    }
                    b[dest] = [0.0; 4];
                }
            }
        }
        views.clear();
        for (&(_binding, slot_no, _dim, kind, _is_ps), name) in p.layout.textures.iter().zip(&p.names) {
            let v = textures
                .iter()
                .find(|(n, _)| n == name)
                .map(|(_, v)| *v)
                .ok_or_else(|| format!("{technique}: no code texture {name} (t{slot_no})"))?;
            if kind == TextureKind::Depth {
                return Err(format!("{technique}: depth texture input"));
            }
            views.push(v.clone());
        }
        let at = match bound.iter().position(|b| b.views == *views) {
            Some(i) => i,
            None => {
                let binds = &p.binds;
                let ubufs: BTreeMap<u32, wgpu::Buffer> = bufs
                    .iter()
                    .map(|(s, r)| {
                        let n = (r.len().max(1) * 16) as u64;
                        let b = gpu.device.create_buffer(&wgpu::BufferDescriptor {
                            label: Some("iw6_post_cb"),
                            size: n,
                            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                            mapped_at_creation: false,
                        });
                        (*s, b)
                    })
                    .collect();
                let cb_entries: Vec<wgpu::BindGroupEntry> = p
                    .layout
                    .cbuffers
                    .iter()
                    .filter_map(|&(binding, slot, _)| {
                        ubufs.get(&slot).map(|b| wgpu::BindGroupEntry {
                            binding,
                            resource: b.as_entire_binding(),
                        })
                    })
                    .collect();
                let samps: Vec<(u32, wgpu::Sampler)> = p
                    .layout
                    .samplers
                    .iter()
                    .map(|&(binding, slot, is_ps)| {
                        let src = if is_ps { binds.ps.samplers.get(&slot) } else { binds.vs.samplers.get(&slot) };
                        let s = match src {
                            Some(SampSource::Code(c)) if matches!(code_texture_name(*c), "FLOATZ" | "DYNAMIC_IRIS_EXPOSURE") => point.clone(),
                            Some(SampSource::Code(c)) if code_texture_name(*c) == "RANDOM_ROTATIONS" => repeat.clone(),
                            _ => linear.clone(),
                        };
                        (binding, s)
                    })
                    .collect();
                let dev = &gpu.device;
                let g0 = dev.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("iw6_post_cb"),
                    layout: &p.layout.groups[0],
                    entries: &cb_entries,
                });
                let g1 = dev.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("iw6_post_tex"),
                    layout: &p.layout.groups[1],
                    entries: &p
                        .layout
                        .textures
                        .iter()
                        .zip(views.iter())
                        .map(|(t, v)| wgpu::BindGroupEntry {
                            binding: t.0,
                            resource: wgpu::BindingResource::TextureView(v),
                        })
                        .collect::<Vec<_>>(),
                });
                let g2 = dev.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("iw6_post_samp"),
                    layout: &p.layout.groups[2],
                    entries: &samps
                        .iter()
                        .map(|(b, s)| wgpu::BindGroupEntry {
                            binding: *b,
                            resource: wgpu::BindingResource::Sampler(s),
                        })
                        .collect::<Vec<_>>(),
                });
                // (Targets re-made at another size leave old sets behind:
                // the oldest goes.)
                if bound.len() >= 4 {
                    bound.remove(0);
                }
                bound.push(Bound {
                    views: views.clone(),
                    bufs: ubufs,
                    groups: [g0, g1, g2],
                    last: BTreeMap::new(),
                });
                bound.len() - 1
            }
        };
        let b = &mut bound[at];
        for (slot_no, rows) in bufs.iter() {
            if b.last.get(slot_no) != Some(rows)
                && let Some(buf) = b.bufs.get(slot_no)
            {
                let data: &[u8] = bytemuck::cast_slice(rows);
                if !data.is_empty() {
                    gpu.queue.write_buffer(buf, 0, data);
                    *bytes += data.len() as u64;
                }
                match b.last.get_mut(slot_no) {
                    Some(l) => {
                        l.clear();
                        l.extend_from_slice(rows);
                    }
                    None => {
                        b.last.insert(*slot_no, rows.clone());
                    }
                }
            }
        }
        let [g0, g1, g2] = &b.groups;
        let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some(technique),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: target,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: if add { wgpu::LoadOp::Load } else { wgpu::LoadOp::Clear(wgpu::Color::BLACK) },
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
        pass.set_bind_group(0, g0, &[]);
        pass.set_bind_group(1, g1, &[]);
        pass.set_bind_group(2, g2, &[]);
        pass.set_vertex_buffer(0, quad.slice(..));
        pass.draw(0..3, 0..1);
        Ok(())
    }
}

/// The luminance chain's format: (sum, pixel count).
const LUM_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rg32Float;

/// The engine's luminance reduction and iris step: the scene's mean
/// Rec. 709 luminance (an arithmetic mean, as `avg_luminance_*`), summed
/// exactly over blocks and then 2 x 2 down to one texel with the pixel
/// counts (every scene pixel counts once whatever the sizes; a pixel that
/// is not finite is left out), then `update_dynamic_iris_adaptation`'s
/// step into the one-texel `DYNAMIC_IRIS_EXPOSURE`
/// (`docs/exe_notes/IW6_SHADERS.md` 10.5).
pub struct Iris {
    first: wgpu::RenderPipeline,
    down: wgpu::RenderPipeline,
    step: wgpu::RenderPipeline,
    /// Ping-pong exposures (R32Float, 1 x 1).
    exposure: [wgpu::Texture; 2],
    pub current: usize,
    chain: Vec<wgpu::Texture>,
    size: (u32, u32),
    sampler: wgpu::Sampler,
    params: wgpu::Buffer,
    /// The first level's constants.
    first_params: wgpu::Buffer,
    /// The chain's and the exposures' views, and each step's bind group
    /// for the scene view they read and each exposure as the previous one
    /// (made once per scene target).
    views: Vec<wgpu::TextureView>,
    exposure_views: [wgpu::TextureView; 2],
    groups: Option<(wgpu::TextureView, [Vec<wgpu::BindGroup>; 2])>,
}

const IRIS_WGSL: &str = r"
@group(0) @binding(0) var src: texture_2d<f32>;
@group(0) @binding(1) var smp: sampler;
struct P { e: vec4<f32>, a: vec4<f32> };
@group(0) @binding(2) var<uniform> prm: P;
@group(0) @binding(3) var prev_e: texture_2d<f32>;

@vertex fn vs(@builtin(vertex_index) i: u32) -> @builtin(position) vec4<f32> {
    let p = vec2<f32>(f32(i == 1u) * 4.0 - 1.0, f32(i == 2u) * 4.0 - 1.0);
    return vec4<f32>(p, 0.0, 1.0);
}

// The first level: each texel the sum of the Rec. 709 luminance of the
// scene pixels its block covers, and how many (a pixel that is not finite
// counts for neither).
@fragment fn first(@builtin(position) p: vec4<f32>) -> @location(0) vec4<f32> {
    let sd = vec2<u32>(textureDimensions(src));
    let od = vec2<u32>(prm.a.zw);
    let o = vec2<u32>(p.xy);
    let lo = (o * sd) / od;
    let hi = min(((o + vec2<u32>(1u)) * sd) / od, sd);
    var sum = 0.0;
    var n = 0.0;
    for (var y = lo.y; y < hi.y; y = y + 1u) {
        for (var x = lo.x; x < hi.x; x = x + 1u) {
            let c = textureLoad(src, vec2<i32>(i32(x), i32(y)), 0).rgb;
            let l = dot(max(c, vec3<f32>(0.0)), vec3<f32>(0.2126, 0.7152, 0.0722));
            if (l == l && l < 3.0e38) {
                sum = sum + l;
                n = n + 1.0;
            }
        }
    }
    return vec4<f32>(sum, n, 0.0, 1.0);
}

// 2 x 2: the sums and counts of the four texels (those past the edge of an
// odd-sized level add nothing).
@fragment fn down(@builtin(position) p: vec4<f32>) -> @location(0) vec4<f32> {
    let sd = vec2<i32>(textureDimensions(src));
    let o = vec2<i32>(p.xy) * 2;
    var acc = vec2<f32>(0.0);
    for (var k = 0; k < 4; k = k + 1) {
        let q = o + vec2<i32>(k & 1, k >> 1u);
        if (q.x < sd.x && q.y < sd.y) {
            acc = acc + textureLoad(src, q, 0).rg;
        }
    }
    return vec4<f32>(acc, 0.0, 1.0);
}

// E = (min, max, key, held: 1 when the exposure does not adapt),
// A = (down rate, up rate, dt, _); the previous exposure is `prev_e`.
@fragment fn step(@builtin(position) p: vec4<f32>) -> @location(0) vec4<f32> {
    let s = textureLoad(src, vec2<i32>(0, 0), 0).rg;
    let l = s.x / max(s.y, 1.0);
    let prev = textureLoad(prev_e, vec2<i32>(0, 0), 0).r;
    if (prm.e.w > 0.5) {
        return vec4<f32>(0.5 * (prm.e.x + prm.e.y), 0.0, 0.0, 1.0);
    }
    let target_e = prm.e.z / max(l, 1e-4);
    let d = target_e - prev;
    let st = select(max(d, -prm.a.x * prm.a.z), min(d, prm.a.y * prm.a.z), d > 0.0);
    return vec4<f32>(clamp(prev + st, prm.e.x, prm.e.y), 0.0, 0.0, 1.0);
}
";

impl Iris {
    pub fn new(gpu: &Gpu) -> Self {
        let dev = &gpu.device;
        let module = dev.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("iw6_iris"),
            source: wgpu::ShaderSource::Wgsl(IRIS_WGSL.into()),
        });
        let bgl = dev.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("iw6_iris"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: false },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::NonFiltering),
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 3,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: false },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
            ],
        });
        let pl = dev.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("iw6_iris"),
            bind_group_layouts: &[Some(&bgl)],
            immediate_size: 0,
        });
        let mk = |entry: &str, format: wgpu::TextureFormat| {
            dev.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(entry),
                layout: Some(&pl),
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
                    entry_point: Some(entry),
                    compilation_options: Default::default(),
                    targets: &[Some(format.into())],
                }),
                multiview_mask: None,
                cache: None,
            })
        };
        // (R16Float: filterable everywhere, as the tone map's vertex
        // shader samples the exposure.)
        // (Sums and pixel counts: exact in f32, so the reduction gives the
        // same mean every run and from every block layout.)
        let first = mk("first", LUM_FORMAT);
        let down = mk("down", LUM_FORMAT);
        let step = mk("step", wgpu::TextureFormat::R16Float);
        let one = |label| {
            dev.create_texture(&wgpu::TextureDescriptor {
                label: Some(label),
                size: wgpu::Extent3d {
                    width: 1,
                    height: 1,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::R16Float,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                    | wgpu::TextureUsages::TEXTURE_BINDING
                    | wgpu::TextureUsages::COPY_SRC
                    | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            })
        };
        let params = dev.create_buffer(&wgpu::BufferDescriptor {
            label: Some("iw6_iris_params"),
            size: 32,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let exposure = [one("iw6_iris_a"), one("iw6_iris_b")];
        let exposure_views = [0, 1].map(|i| exposure[i].create_view(&Default::default()));
        let first_params = dev.create_buffer(&wgpu::BufferDescriptor {
            label: Some("iw6_iris_first"),
            size: 32,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        Self {
            first,
            down,
            step,
            first_params,
            views: Vec::new(),
            exposure_views,
            groups: None,
            exposure,
            current: 0,
            chain: Vec::new(),
            size: (0, 0),
            sampler: gpu.device.create_sampler(&wgpu::SamplerDescriptor {
                label: Some("iw6_iris"),
                ..Default::default()
            }),
            params,
        }
    }

    /// Sets the exposure (held, or the start of the adaptation).
    pub fn set(&mut self, gpu: &Gpu, exposure: f32) {
        gpu.queue.write_texture(
            self.exposure[self.current].as_image_copy(),
            bytemuck::bytes_of(&crate::model_lighting::half(exposure)),
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(2),
                rows_per_image: Some(1),
            },
            wgpu::Extent3d {
                width: 1,
                height: 1,
                depth_or_array_layers: 1,
            },
        );
    }

    pub fn view(&self) -> wgpu::TextureView {
        self.exposure_views[self.current].clone()
    }

    pub fn texture(&self) -> &wgpu::Texture {
        &self.exposure[self.current]
    }

    /// Measures `scene` and steps the exposure: `e` = (min, max, key),
    /// `rates` = (down, up), `dt` seconds; `held`: the exposure does not
    /// adapt (the middle of its bounds).
    #[allow(clippy::too_many_arguments)]
    pub fn record(
        &mut self,
        gpu: &Gpu,
        enc: &mut wgpu::CommandEncoder,
        scene: &wgpu::TextureView,
        scene_size: (u32, u32),
        e: [f32; 3],
        rates: [f32; 2],
        dt: f32,
        held: bool,
    ) {
        if self.size != scene_size {
            self.size = scene_size;
            self.chain.clear();
            // The first level: the scene halved until it fits 256.
            let (mut w, mut h) = scene_size;
            while w > 256 || h > 256 {
                w = w.div_ceil(2);
                h = h.div_ceil(2);
            }
            loop {
                self.chain
                    .push(render_h1::post::target(gpu, "iw6_lum", w.max(1), h.max(1), LUM_FORMAT));
                if w <= 1 && h <= 1 {
                    break;
                }
                w = w.div_ceil(2);
                h = h.div_ceil(2);
            }
        }
        if self.views.len() != self.chain.len() {
            self.views = self.chain.iter().map(|t| t.create_view(&Default::default())).collect();
            self.groups = None;
        }
        let first_size = (self.chain[0].width(), self.chain[0].height());
        // (The first pass reads the first level's size from `a.zw`.)
        let scale = [first_size.0 as f32, first_size.1 as f32];
        let held = if held { 1.0 } else { 0.0 };
        let p = [e[0], e[1], e[2], held, rates[0], rates[1], dt, 0.0];
        gpu.queue.write_buffer(&self.params, 0, bytemuck::cast_slice(&p));
        let f = [e[0], e[1], e[2], held, rates[0], rates[1], scale[0], scale[1]];
        gpu.queue.write_buffer(&self.first_params, 0, bytemuck::cast_slice(&f));
        if self.groups.as_ref().is_none_or(|(v, _)| v != scene) {
            let group = |pipe: &wgpu::RenderPipeline, src: &wgpu::TextureView, buf: &wgpu::Buffer, prev: &wgpu::TextureView| {
                gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("iw6_iris"),
                    layout: &pipe.get_bind_group_layout(0),
                    entries: &[
                        wgpu::BindGroupEntry {
                            binding: 0,
                            resource: wgpu::BindingResource::TextureView(src),
                        },
                        wgpu::BindGroupEntry {
                            binding: 1,
                            resource: wgpu::BindingResource::Sampler(&self.sampler),
                        },
                        wgpu::BindGroupEntry {
                            binding: 2,
                            resource: buf.as_entire_binding(),
                        },
                        wgpu::BindGroupEntry {
                            binding: 3,
                            resource: wgpu::BindingResource::TextureView(prev),
                        },
                    ],
                })
            };
            let sets = [0, 1].map(|cur| {
                let prev = &self.exposure_views[cur];
                let mut g = vec![group(&self.first, scene, &self.first_params, prev)];
                for k in 1..self.views.len() {
                    g.push(group(&self.down, &self.views[k - 1], &self.params, prev));
                }
                g.push(group(&self.step, &self.views[self.views.len() - 1], &self.params, prev));
                g
            });
            self.groups = Some((scene.clone(), sets));
        }
        let Some((_, sets)) = &self.groups else { return };
        let groups = &sets[self.current];
        let next = 1 - self.current;
        let n = self.views.len();
        for (k, g) in groups.iter().enumerate() {
            let (pipe, dst) = match k {
                0 => (&self.first, &self.views[0]),
                k if k < n => (&self.down, &self.views[k]),
                _ => (&self.step, &self.exposure_views[next]),
            };
            let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("iw6_iris"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: dst,
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
            pass.set_pipeline(pipe);
            pass.set_bind_group(0, g, &[]);
            pass.draw(0..3, 0..1);
        }
        self.current = next;
    }
}

/// `MACROCLUT` (16^3, linear in and out, `postfx_macroclut` samples it at
/// `t * 15/16 + 1/32` with the tone-mapped linear value `t`): the vision's
/// film (`look::Film`) baked in, applied in display space; the identity
/// when the film is off.
/// `MACROCLUT` as the engine bakes it from the colorization set's low / mid
/// / high grade (`look::Lmh`): 16^3, 8-bit unorm, linear in and out.
pub fn macroclut_lmh(gpu: &Gpu, lmh: &crate::look::Lmh) -> render_h1::gpu::Tex {
    let n = 16u32;
    let mut data: Vec<u8> = Vec::with_capacity((n * n * n * 4) as usize);
    for b in 0..n {
        for g in 0..n {
            for r in 0..n {
                let t = [r, g, b].map(|c| c as f32 * 0.06666667);
                for c in lmh.apply(t) {
                    data.push((c * 255.0 + 0.5) as u8);
                }
                data.push(0);
            }
        }
    }
    clut_texture(gpu, &data, wgpu::TextureFormat::Rgba8Unorm)
}

fn clut_texture(gpu: &Gpu, data: &[u8], format: wgpu::TextureFormat) -> render_h1::gpu::Tex {
    let n = 16u32;
    let texture = wgpu::util::DeviceExt::create_texture_with_data(
        &gpu.device,
        &gpu.queue,
        &wgpu::TextureDescriptor {
            label: Some("iw6_macroclut"),
            size: wgpu::Extent3d {
                width: n,
                height: n,
                depth_or_array_layers: n,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D3,
            format,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        },
        wgpu::util::TextureDataOrder::LayerMajor,
        data,
    );
    let view = texture.create_view(&wgpu::TextureViewDescriptor {
        dimension: Some(wgpu::TextureViewDimension::D3),
        ..Default::default()
    });
    render_h1::gpu::Tex {
        texture,
        view,
        dim: wgpu::TextureViewDimension::D3,
    }
}

pub fn macroclut(gpu: &Gpu, film: &crate::look::Film) -> render_h1::gpu::Tex {
    use crate::look::{linear_to_srgb, srgb_to_linear};
    let n = 16u32;
    let mut data: Vec<u16> = Vec::with_capacity((n * n * n * 4) as usize);
    for b in 0..n {
        for g in 0..n {
            for r in 0..n {
                let t = [r, g, b].map(|c| c as f32 / (n - 1) as f32);
                let d = film.apply(t.map(linear_to_srgb));
                for c in d.map(srgb_to_linear) {
                    data.push(crate::model_lighting::half(c));
                }
                data.push(crate::model_lighting::half(1.0));
            }
        }
    }
    let data: Vec<u8> = bytemuck::cast_slice(&data).to_vec();
    let texture = wgpu::util::DeviceExt::create_texture_with_data(
        &gpu.device,
        &gpu.queue,
        &wgpu::TextureDescriptor {
            label: Some("iw6_macroclut"),
            size: wgpu::Extent3d {
                width: n,
                height: n,
                depth_or_array_layers: n,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D3,
            format: wgpu::TextureFormat::Rgba16Float,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        },
        wgpu::util::TextureDataOrder::LayerMajor,
        &data,
    );
    let view = texture.create_view(&wgpu::TextureViewDescriptor {
        dimension: Some(wgpu::TextureViewDimension::D3),
        ..Default::default()
    });
    render_h1::gpu::Tex {
        texture,
        view,
        dim: wgpu::TextureViewDimension::D3,
    }
}
