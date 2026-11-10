//! The game's pipeline list for a map and what of it is made: each program
//! pair ([`crate::mapping`]) is lowered (`dxil_sm6::wgsl::translate`), linked
//! ([`crate::library::link`]) and its binding table read; on a device the pair
//! is also made into a render pipeline under a validation error scope.
//! Lines go to the log as `jup native: ...`. The probe example
//! (`examples/jup_native_check.rs`) runs the same list offline and validates
//! every module with naga.

use std::collections::BTreeMap;
use std::num::NonZeroU32;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use render_h1::gpu::Gpu;

use crate::library::{Decl, DeclKind, Library, Linked, link};
use crate::mapping::{Pair, check_pairs};

/// What building one pair gave.
#[derive(Clone, Debug, Default)]
pub struct PairReport {
    pub pair: Option<Pair>,
    pub lowered: bool,
    pub error: Option<String>,
    pub vertex_bytes: usize,
    pub pixel_bytes: usize,
    pub groups: usize,
    pub bindings: usize,
    /// `(count, kind)` of the bindless arrays.
    pub bindless: Vec<(u32, String)>,
    pub unlinked: Vec<String>,
    pub shifted: usize,
    pub targets: usize,
}

/// Lowers and links one pair.
pub fn build_pair(lib: &Library, pair: Pair) -> (PairReport, Option<Linked>) {
    let mut r = PairReport { pair: Some(pair), ..Default::default() };
    let lowered = lib.lower(pair.vertex).and_then(|v| lib.lower(pair.pixel).map(|p| (v, p)));
    let (vs, ps) = match lowered {
        Ok(x) => x,
        Err(e) => {
            r.error = Some(e);
            return (r, None);
        }
    };
    r.lowered = true;
    r.targets = ps.outputs.iter().filter(|o| o.name.eq_ignore_ascii_case("SV_Target")).count();
    let linked = link(&vs, &ps);
    r.vertex_bytes = linked.vertex_wgsl.len();
    r.pixel_bytes = linked.pixel_wgsl.len();
    let all: Vec<&Decl> = linked.vertex_decls.iter().chain(linked.pixel_decls.iter()).collect();
    r.bindings = all.len();
    r.groups = all.iter().map(|d| d.group as usize + 1).max().unwrap_or(0);
    for d in &all {
        if let DeclKind::Array { of, count } = &d.kind {
            let kind = match &**of {
                DeclKind::Texture { dim, sample } => format!("texture_{dim}<{sample}>"),
                other => format!("{other:?}"),
            };
            if !r.bindless.iter().any(|(c, k)| c == count && *k == kind) {
                r.bindless.push((*count, kind));
            }
        }
    }
    r.unlinked.clone_from(&linked.unlinked);
    r.shifted = linked.shifted;
    (r, Some(linked))
}

/// One line for the log.
pub fn describe(r: &PairReport) -> String {
    let Some(p) = r.pair else { return "no pair".into() };
    if let Some(e) = &r.error {
        return format!("pair vs {} ps {}: not lowered: {e}", p.vertex, p.pixel);
    }
    format!(
        "pair vs {} ps {}: lowered ({} + {} B of WGSL), {} bindings in {} groups, bindless {}, {} pixel targets, {} bindings moved off the vertex stage's registers, pixel inputs the vertex stage does not write: {}",
        p.vertex,
        p.pixel,
        r.vertex_bytes,
        r.pixel_bytes,
        r.bindings,
        r.groups,
        if r.bindless.is_empty() { "none".to_owned() } else { r.bindless.iter().map(|(c, k)| format!("{k} x {c}")).collect::<Vec<_>>().join(", ") },
        r.targets,
        r.shifted,
        if r.unlinked.is_empty() { "none".to_owned() } else { r.unlinked.join(", ") }
    )
}

fn entry_type(kind: &DeclKind) -> Result<(wgpu::BindingType, Option<NonZeroU32>), String> {
    use wgpu::{BindingType as B, TextureSampleType as S, TextureViewDimension as D};
    Ok(match kind {
        DeclKind::Uniform { rows } => (
            B::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: wgpu::BufferSize::new(u64::from(*rows) * 16) },
            None,
        ),
        DeclKind::RawBuffer => (B::Buffer { ty: wgpu::BufferBindingType::Storage { read_only: true }, has_dynamic_offset: false, min_binding_size: None }, None),
        DeclKind::Texture { dim, sample } => {
            let view_dimension = match dim.as_str() {
                "1d" => D::D1,
                "2d" | "depth_2d" => D::D2,
                "2d_array" | "depth_2d_array" => D::D2Array,
                "3d" => D::D3,
                "cube" | "depth_cube" => D::Cube,
                other => return Err(format!("texture dimension {other}")),
            };
            let sample_type = match sample.as_str() {
                "f32" => S::Float { filterable: true },
                "u32" => S::Uint,
                "i32" => S::Sint,
                "depth" => S::Depth,
                other => return Err(format!("texture sample type {other}")),
            };
            (B::Texture { sample_type, view_dimension, multisampled: false }, None)
        }
        DeclKind::Sampler { comparison } => (B::Sampler(if *comparison { wgpu::SamplerBindingType::Comparison } else { wgpu::SamplerBindingType::Filtering }), None),
        DeclKind::StorageTexture(t) => return Err(format!("storage texture {t}")),
        DeclKind::Array { of, count } => (entry_type(of)?.0, NonZeroU32::new(*count)),
    })
}

/// Makes a linked pair into a render pipeline on `gpu` under a validation
/// error scope: the bind group layouts from the declarations, the pixel
/// stage's targets as `Rgba16Float`, reversed depth.
pub fn try_pipeline(gpu: &Gpu, linked: &Linked, targets: usize) -> Result<(), String> {
    let device = &gpu.device;
    let limits = device.limits();
    let groups = linked.vertex_decls.iter().chain(&linked.pixel_decls).map(|d| d.group + 1).max().unwrap_or(0);
    if groups > limits.max_bind_groups {
        return Err(format!("needs {groups} bind groups, the device allows {}", limits.max_bind_groups));
    }
    let bindless = linked.vertex_decls.iter().chain(&linked.pixel_decls).filter_map(|d| if let DeclKind::Array { count, .. } = d.kind { Some(count) } else { None }).max().unwrap_or(0);
    if bindless > limits.max_binding_array_elements_per_shader_stage {
        return Err(format!("needs binding arrays of {bindless}, the device allows {}", limits.max_binding_array_elements_per_shader_stage));
    }
    let mut entries: BTreeMap<(u32, u32), wgpu::BindGroupLayoutEntry> = BTreeMap::new();
    for (decls, vis) in [(&linked.vertex_decls, wgpu::ShaderStages::VERTEX), (&linked.pixel_decls, wgpu::ShaderStages::FRAGMENT)] {
        for d in decls {
            let (ty, count) = entry_type(&d.kind).map_err(|e| format!("{}: {e}", d.name))?;
            entries
                .entry((d.group, d.binding))
                .and_modify(|e| e.visibility |= vis)
                .or_insert(wgpu::BindGroupLayoutEntry { binding: d.binding, visibility: vis, ty, count });
        }
    }
    let scope = device.push_error_scope(wgpu::ErrorFilter::Validation);
    let layouts: Vec<wgpu::BindGroupLayout> = (0..groups)
        .map(|g| {
            let list: Vec<_> = entries.range((g, 0)..(g + 1, 0)).map(|(_, e)| *e).collect();
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor { label: Some("jup_program"), entries: &list })
        })
        .collect();
    let refs: Vec<Option<&wgpu::BindGroupLayout>> = layouts.iter().map(Some).collect();
    let pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: Some("jup_program"), bind_group_layouts: &refs, immediate_size: 0 });
    let vs = device.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some("jup_program_vs"), source: wgpu::ShaderSource::Wgsl(linked.vertex_wgsl.as_str().into()) });
    let fs = device.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some("jup_program_ps"), source: wgpu::ShaderSource::Wgsl(linked.pixel_wgsl.as_str().into()) });
    let color: Vec<Option<wgpu::ColorTargetState>> = (0..targets).map(|_| Some(wgpu::ColorTargetState { format: wgpu::TextureFormat::Rgba16Float, blend: None, write_mask: wgpu::ColorWrites::ALL })).collect();
    let _pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("jup_program"),
        layout: Some(&pl),
        vertex: wgpu::VertexState { module: &vs, entry_point: Some("main"), compilation_options: Default::default(), buffers: &[] },
        fragment: Some(wgpu::FragmentState { module: &fs, entry_point: Some("main"), compilation_options: Default::default(), targets: &color }),
        primitive: wgpu::PrimitiveState::default(),
        depth_stencil: Some(wgpu::DepthStencilState {
            format: wgpu::TextureFormat::Depth32Float,
            depth_write_enabled: Some(true),
            depth_compare: Some(wgpu::CompareFunction::Greater),
            stencil: Default::default(),
            bias: Default::default(),
        }),
        multisample: Default::default(),
        multiview_mask: None,
        cache: None,
    });
    match pollster::block_on(scope.pop()) {
        Some(e) => Err(e.to_string()),
        None => Ok(()),
    }
}

/// The check thread: opens the library, builds the pairs and says what came
/// of them, as `jup native:` lines in `out`.
pub fn spawn_check(gpu: Gpu, install: PathBuf, map: String, materials: usize, out: Arc<Mutex<Vec<String>>>) {
    let say = move |out: &Arc<Mutex<Vec<String>>>, line: String| {
        if let Ok(mut o) = out.lock() {
            o.push(line);
        }
    };
    if std::env::var("BO2MP_JUP_PROGRAM_CHECK").is_ok_and(|v| v.trim() == "0") {
        return;
    }
    let result = std::thread::Builder::new().name("jup_native_check".into()).spawn(move || {
        let started = std::time::Instant::now();
        let run = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let oodle = fastfile_jup::oodle::load(&install).map_err(|e| format!("Oodle: {e}"))?;
            let lib = Library::load(&install, oodle.as_ref())?;
            let graphics = (0..lib.programs.len()).filter(|&i| lib.is_graphics(i)).count();
            say(&out, format!("programs: {} in {} ({graphics} vertex or pixel), read in {:.1}s", lib.programs.len(), crate::library::ZONE, started.elapsed().as_secs_f32()));
            let pairs = check_pairs(&lib, 4);
            say(
                &out,
                format!(
                    "{map}: {materials} materials, none mapped to a program yet (the material to program mapping is not decoded), so every surface is drawn by the fallback shader; the game's {} material pixel programs and {} distinct families are lowered below as the pipeline check",
                    crate::library::material_pairs(&lib).len(),
                    pairs.len()
                ),
            );
            for pair in pairs {
                let (report, linked) = build_pair(&lib, pair);
                say(&out, describe(&report));
                if let Some(linked) = linked {
                    let t = std::time::Instant::now();
                    match try_pipeline(&gpu, &linked, report.targets.max(1)) {
                        Ok(()) => say(&out, format!("pair vs {} ps {}: pipeline made on this device in {:.1}s", pair.vertex, pair.pixel, t.elapsed().as_secs_f32())),
                        Err(e) => say(&out, format!("pair vs {} ps {}: pipeline not made on this device: {}", pair.vertex, pair.pixel, e.chars().take(300).collect::<String>())),
                    }
                }
            }
            Ok::<(), String>(())
        }));
        match run {
            Ok(Ok(())) => {}
            Ok(Err(e)) => say(&out, format!("program check did not run: {e}")),
            Err(_) => say(&out, "program check panicked".to_owned()),
        }
    });
    if result.is_err() {
        eprintln!("jup native: the program check thread could not start");
    }
}
