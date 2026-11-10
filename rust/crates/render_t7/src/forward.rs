//! The forward lists as BlackOps3.exe records them after the deferred
//! lighting (`141d4e220`): every surface whose material has no G-buffer
//! technique is drawn with its technique set's forward technique (slot 6,
//! the view's default technique byte: `scene::FORWARD_TECHNIQUE`) into the
//! lit scene (code texture 0xa) over the G-buffer's depth, with the
//! technique's own states, programs and arguments. That covers the sky,
//! emissive surfaces, glass, water and the transparent and blended ones.
//!
//! The exe draws these from several lists (`142ed1220`: lists 7–10, 12–17,
//! 22, 23 take the default technique) in the order 14, 7, 8, (effects 18),
//! 9, 10, 20, 21, 22. Each list is sorted by the draw surfaces' sort keys
//! (`141d8d9b0`); the material's key template is MaterialInfo +0x10. Which
//! list a surface goes into is decided where the scene is traversed (the
//! surface buckets of `141d8cc70`), not decoded yet: here every forward
//! surface is drawn in one sequence in its key order (the keys' high bits
//! put emissive before glass before transparent, as the lists do). The
//! lists the exe sorts back to front (0x10, 0x11, 0x16: `141d8fc40`) are
//! not told apart from the others yet.
//!
//! Each argument is bound as the exe binds it (see `gbuffer`); a code
//! texture, buffer or constant buffer is the lighting stage's resource of
//! the same name (`Lighting::named`). A draw whose bindings cannot all be
//! made (a code texture the frame does not make yet: `resolvedScene` for
//! refraction, the material texture atlases, ...) is left out and listed.

#[allow(unused_imports)]
use crate::profile::{ComputeExt as _, EncExt as _, PassExt as _, QueueExt as _};
use std::collections::HashMap;
use std::sync::Arc;

use dxbc_sm5::{TextureDim, TextureKind};
use render_h1::gpu::Gpu;

use crate::code::CodeConsts;
use crate::compute::Res;
use crate::gbuffer::{make_pipeline_for, GBuffer, Pipeline, Targets, CODE_BUFFER_BONES, CODE_BUFFER_INSTANCES, CODE_TEXTURE_RESOLVED_NORMAL, DEPTH};
use crate::layout;
use crate::lighting::{Lighting, SCENE_FORMAT};
use crate::rdef;
use crate::scene::{Scene, Technique};

#[path = "fx_draw.rs"]
pub mod fx_draw;


struct Ready {
    draw: usize,
    pipeline: Arc<Pipeline>,
    groups: [wgpu::BindGroup; 3],
    key: u64,
    list: u8,
    /// Reads the scene depth (`floatZ`): read through a copy.
    reads_depth: bool,
    /// Reads `resolvedPostSun`.
    reads_resolved: bool,
}

/// The view lists the frame draws after the lighting, in the exe's order
/// (`141d4e220`; 18 is the effects', not drawn here). 16, 17 and 22 are
/// walked back to front (`141d8fc40`).
const LIST_ORDER: [u8; 8] = [14, 7, 8, 9, 10, 20, 21, 22];
const BACK_TO_FRONT: [u8; 3] = [16, 17, 22];

#[derive(Default, Debug)]
pub struct Report {
    pub pipelines_made: usize,
    pub pipelines_failed: Vec<String>,
    pub draws_ready: usize,
    pub draws_total: usize,
    pub draws_skipped: Vec<String>,
    /// Every forward draw left out: (draw, reason).
    pub skipped_draws: Vec<(usize, String)>,
    /// Missing pictures bound as a stand-in (as `gbuffer::Report::stand_ins`).
    pub stand_ins: Vec<String>,
}

pub struct Forward {
    gpu: Gpu,
    scene: Arc<Scene>,
    ready: Vec<Ready>,
    /// Code constant buffers by technique and binding: (buffer, size, the
    /// arguments filling it, what was last written).
    code_bufs: HashMap<(usize, u32), (wgpu::Buffer, usize, Vec<fastfile_t7::walk::capture::ShaderArg>, Vec<u8>)>,
    pipelines: HashMap<(usize, u8), Option<Arc<Pipeline>>>,
    /// The light, probe and decal lists the forward programs walk, empty:
    /// light culling is not built yet, so the frame has no lights, local
    /// probes or decals in its lists (as the deferred lighting's cull bits
    /// are empty); keyed by the binding's dimension and kind.
    empty_textures: HashMap<String, crate::gpu::Tex>,
    empty_buffer: Option<wgpu::Buffer>,
    /// The light culling (`light_culling_lights` / `_probes`) that fills
    /// the forward programs' per-cluster light and probe masks.
    cull: Option<Cull>,
    /// The quarter-resolution depth the water reads (`floatZDownscaled`).
    water_depth: Option<WaterDepth>,
    /// `resolvedPostSun` (code texture 0x9): the lit scene copied before
    /// view lists 20 and 21 when they have draws (`141d4b470` ->
    /// `141d4b2a0`: the scene target's colour into the resolve target, then
    /// bound), for the refracting glass and water.
    resolved: Option<crate::gpu::Tex>,
    /// The scene depth for the draws that read `floatZ` while drawing with
    /// depth: copied before their pass (wgpu cannot sample the depth a pass
    /// draws into), its depth aspect.
    depth_copy: Option<(wgpu::Texture, wgpu::TextureView)>,
    /// `floatZ` in the exe's depth encoding, made from the depth copy
    /// (what the forward programs and the water's downsample read).
    float_z: Option<FloatZ>,
    pub report: Report,
    /// The effects' sprites (view list 18, `fx_draw`).
    pub fx: fx_draw::FxDraw,
    /// The lit fog volumes' passes (`crate::volumetric_pass`), when the map
    /// has lit fog volumes.
    pub volumetrics: Option<crate::volumetric_pass::VolumetricPass>,
    /// The eye this frame (world), for the water's from-below check.
    eye: [f32; 3],
    /// The view's side planes this frame (`crate::cull::planes`): a draw
    /// whose bounds (a world surface) or every placement (an instanced
    /// model surface) is outside them draws no pixels and is not issued
    /// (`T7_NO_FORWARD_CULL=1`: every draw).
    planes: Option<[[f32; 4]; 4]>,
    /// The code constant buffers' bytes as built each frame (reused).
    code_scratch: Vec<u8>,
    /// The last verdict logged per water draw (`CODLEGACY_T7WATERLOG=1`).
    water_seen: std::sync::Mutex<HashMap<usize, u8>>,
}

/// `resolvedPostSun`.
const CODE_TEXTURE_RESOLVED_POST_SUN: u32 = 0x9;

/// `floatZ` (code texture 0x10) as the exe's programs read it. The exe's
/// depth is not ours: they decode `floatZ` as `z = 63 / (64·d)` below
/// `63/64` (the world) and `z = 1 / (64·d − 63)` above (the view model),
/// while our depth is `near / z` (reversed, infinite: `crate::motion`). Read
/// raw, the scene behind a forward surface came out `near·64/63` (about 4)
/// times nearer than it is: the water's depth test (`scene z > water z`)
/// failed and its depth fade saw no water under it, so every water body
/// drew as its refraction alone (the bed through clear glass) and its
/// downsample (`floatZDownscaled`) carried the same error. This writes
/// `d · 63 / (64·near)` (the world range) into an R32Float target from
/// the depth copy each time the copy is taken.
struct FloatZ {
    pipeline: wgpu::RenderPipeline,
    k: wgpu::Buffer,
    out: crate::gpu::Tex,
    group: wgpu::BindGroup,
}

const FLOATZ_CONVERT: &str = r#"
@group(0) @binding(0) var depth: texture_depth_2d;
struct K { k: vec4<f32> }
@group(0) @binding(1) var<uniform> k: K;
@vertex fn vs(@builtin(vertex_index) i: u32) -> @builtin(position) vec4<f32> {
    let p = vec2<f32>(f32((i << 1u) & 2u), f32(i & 2u));
    return vec4<f32>(p * 2.0 - 1.0, 0.0, 1.0);
}
@fragment fn fs(@builtin(position) p: vec4<f32>) -> @location(0) vec4<f32> {
    let d = textureLoad(depth, vec2<i32>(p.xy), 0);
    return vec4<f32>(d * k.k.x, 0.0, 0.0, 0.0);
}
"#;

/// `T7_RAW_FLOATZ=1`: the forward lists read our depth as it is (the old
/// way, for before/after checks).
fn raw_floatz() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var("T7_RAW_FLOATZ").is_ok_and(|v| !v.is_empty() && v != "0"))
}

/// `floatZDownscaled` (code texture 0x11, the water's
/// `ifloatZSceneDownsampled`), as the exe makes it after view lists 9, 10
/// and 20 and before 21 and 22 (`141d4ced0` -> `141d4c090`):
/// `downsample_depth_4x_compute` reads `floatZ` (code texture 0x10: the
/// scene depth, view 0 of its target) and writes each 4x4 block's depth
/// (min / max by a checkerboard) to an R32_FLOAT target, with
/// `postFxControl7` = (4, 0, 0, 0) and `postFxConst00` = (1 / source
/// width, 1 / source height, target width, target height), dispatched in
/// target pixels. The exe runs it only when the view has water to draw.
struct WaterDepth {
    program: crate::compute::Program,
    out: crate::gpu::Tex,
    groups: Vec<wgpu::BindGroup>,
    size: [u32; 2],
}

/// The exe's light culling before the forward lists (`141d5de20`): per 64
/// pixel tile and 16 depth slices of 128 units (the last one to 131072),
/// a 32-bit mask per 32 lights (layer = slice * 16 + chunk) in
/// `visibleLights` (UAV u6) and `visibleProbes` (u7), from the cull
/// constants (`gCullConstants`, t11: lights first, then probes),
/// `PerSceneConsts` and `LightingGlobals`; dispatched in threads (tiles x,
/// tiles y, 16 x chunks of 32), and only when there is something to cull.
/// Decals (`light_culling_decals`, u0) are not packed yet, so not culled.
struct Cull {
    programs: [crate::compute::Program; 2],
    masks: [crate::gpu::Tex; 2],
    groups: [Vec<wgpu::BindGroup>; 2],
    tiles: [u32; 2],
    /// Its own `PerSceneConsts`, filled by the two programs' arguments
    /// (the inverse view matrix and the target size the lighting's
    /// programs do not take).
    per_scene: wgpu::Buffer,
    args: Vec<fastfile_t7::walk::capture::ShaderArg>,
}

/// Mask layers: 16 depth slices x 16 chunks of 32 lights.
const CULL_LAYERS: u32 = 256;
const CULL_TILE: u32 = 64;

/// The code resources bound empty until light culling fills them (see
/// `Forward::empty_textures`): the forward programs' visible light, probe
/// and decal lists and the light, probe and decal constants they index (the
/// lists light culling fills are not built).
/// The material texture arrays (`materialTexBC3/BC7/BC7linear/Mask`) are
/// read only through the decal list, so they are empty with it.
const EMPTY_UNTIL_LIGHT_CULLING: [&str; 6] = [
    "visibleDecals",
    "forwardDecalConstants",
    "materialTexBC3",
    "materialTexBC7",
    "materialTexBC7linear",
    "materialTexMask",
];

fn stage(ty: u16) -> u16 {
    ty & 0xff
}

/// `t7 sky material:` lines, once per map: the sky material (the sky box
/// draws' and the sun volume's), its texture slots and whether each picture
/// resolved (size and format as uploaded) or is missing.
pub(crate) fn sky_material_lines(g: &GBuffer, ready: &[usize]) -> Vec<String> {
    let sc = &g.scene;
    let mut mats: Vec<usize> = sc.draws.iter().filter(|d| d.source == u32::MAX && d.group == 1).map(|d| d.material).collect();
    if let Some(m) = sc.map.light_refs().map(|l| l.sky_material.clone()).filter(|m| !m.is_empty()) {
        if let Some(i) = sc.materials.iter().position(|x| x.name == m || x.name.ends_with(&format!("/{m}"))) {
            mats.push(i);
        }
    }
    mats.sort_unstable();
    mats.dedup();
    let picture = sc.map.light_refs().and_then(|l| l.sky.as_ref().map(|s| s.image.clone()));
    let mut out = Vec::new();
    if mats.is_empty() {
        out.push(format!("t7 sky material: none (no sky box draw, sky material {:?}, sky picture {picture:?})", sc.map.light_refs().map(|l| l.sky_material.clone())));
    }
    for i in mats {
        let m = &sc.materials[i];
        let drawn = ready.iter().filter(|&&d| sc.draws[d].material == i).count();
        let t = m.forward.as_ref().or(m.technique.as_ref());
        let mut slots = Vec::new();
        for (hash, img) in &m.textures {
            let slot = t.and_then(|t| t.args.iter().find(|a| a.ty & 0xff00 == 0x200 && a.u[0] == *hash).map(|a| format!("t{}", a.location))).unwrap_or_else(|| "t?".into());
            let state = match g.images.get(img) {
                Some(Some(tex)) => {
                    let s = tex.texture.size();
                    format!("resolved {}x{} {:?} {} levels", s.width, s.height, tex.texture.format(), tex.texture.mip_level_count())
                }
                Some(None) => "MISSING".into(),
                None => "not bound".into(),
            };
            slots.push(format!("{slot} {hash:08x} {img}: {state}"));
        }
        out.push(format!(
            "t7 sky material: {} ({}), {drawn} ready draws, forward technique {}; sky picture {}; textures: {}",
            m.name,
            m.techset,
            if m.forward.is_some() { "yes" } else { "no" },
            picture.as_deref().unwrap_or("none"),
            if slots.is_empty() { "none".into() } else { slots.join(", ") }
        ));
    }
    out
}

/// `floatZ` (code texture 0x10: the scene depth, read as floats).
const CODE_TEXTURE_FLOATZ: u32 = 0x10;

/// The pixel-stage slots a technique binds `floatZ` to (unfilterable).
pub(crate) fn floatz_slots(t: &Technique) -> Vec<u32> {
    t.args.iter().filter(|a| a.ty & 0xff00 == 0x500 && stage(a.ty) == 1 && a.u[0] == CODE_TEXTURE_FLOATZ).map(|a| u32::from(a.location)).collect()
}

impl Forward {
    /// The draws the forward lists record (indices into the scene's draws).
    /// One `t7 water:` line per map: the water surfaces (technique sets
    /// with `water`, drawn by the exe's own water programs: refraction of
    /// `resolvedPostSun`, depth fade from `floatZ` / `floatZDownscaled`,
    /// fresnel and sky reflection, animated normals from the game time),
    /// how many are drawn, why the others are not, and whether the depth
    /// they read is in the exe's encoding.
    pub fn water_line(&self) -> String {
        let s = &self.scene;
        let ready: std::collections::HashSet<usize> = self.ready.iter().map(|r| r.draw).collect();
        let mut sets: std::collections::BTreeMap<&str, (usize, usize)> = std::collections::BTreeMap::new();
        let mut why: std::collections::BTreeMap<String, usize> = std::collections::BTreeMap::new();
        for (di, d) in s.draws.iter().enumerate() {
            let m = &s.materials[d.material];
            if !m.techset.contains("water") {
                continue;
            }
            let e = sets.entry(m.techset.as_str()).or_default();
            e.0 += 1;
            if ready.contains(&di) {
                e.1 += 1;
            } else {
                let r = self.report.skipped_draws.iter().find(|(i, _)| *i == di).map_or_else(|| "not a forward draw".to_owned(), |(_, r)| r.clone());
                *why.entry(r).or_default() += 1;
            }
        }
        let (n, drawn) = sets.values().fold((0, 0), |a, b| (a.0 + b.0, a.1 + b.1));
        // Every map, no env switch: the water draws' placement (largest
        // first), so a sheet over a floor can be told from the log alone
        // (a flat sheet at the floor's height, a brush box at the origin,
        // a vista sheet above the eye). `range none` / source past the
        // world's ranges are brush-model surfaces.
        let mut ws: Vec<(f64, usize)> = s.draws.iter().enumerate().filter(|(_, d)| d.group == 3 && s.materials[d.material].techset.contains("water")).map(|(i, d)| (f64::from(d.maxs[0] - d.mins[0]) * f64::from(d.maxs[1] - d.mins[1]), i)).collect();
        ws.sort_by(|a, b| b.0.total_cmp(&a.0));
        for (area, di) in ws.iter().take(12) {
            let d = &s.draws[*di];
            eprintln!(
                "t7 water draw: surface #{} {} range {} list {} z {:.0}..{:.0} x {:.0}..{:.0} y {:.0}..{:.0} (bounds area {area:.3e}), {}",
                d.source,
                s.materials[d.material].name,
                ["A", "B", "C", "D", "none"].get(d.range as usize).unwrap_or(&"?"),
                d.list,
                d.mins[2],
                d.maxs[2],
                d.mins[0],
                d.maxs[0],
                d.mins[1],
                d.maxs[1],
                if ready.contains(di) { "forward draw" } else { "not a forward draw" },
            );
        }
        format!(
            "t7 water: {drawn} of {n} water draws drawn by the exe's water programs ({}); not drawn: {why:?}; floatZ {} ; floatZDownscaled {}",
            sets.iter().map(|(k, (a, b))| format!("{k} {b}/{a}")).collect::<Vec<_>>().join(", "),
            if self.float_z.is_some() { "in the exe's encoding" } else { "raw (T7_RAW_FLOATZ)" },
            if self.water_depth.is_some() { "made" } else { "not made" },
        )
    }

    pub fn set_eye(&mut self, eye: [f32; 3]) {
        self.eye = eye;
    }

    /// The view the forward draws are culled against this frame.
    pub fn set_cull_view(&mut self, world_to_clip: glam::Mat4) {
        static OFF: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
        let off = *OFF.get_or_init(|| std::env::var("T7_NO_FORWARD_CULL").is_ok_and(|v| v.trim() != "0"));
        self.planes = (!off).then(|| crate::cull::planes(world_to_clip));
    }

    /// Whether ready draw `r` is drawn this frame (not a water sheet left
    /// out, in the view).
    fn drawn(&self, r: &Ready) -> bool {
        let d = &self.scene.draws[r.draw];
        // (A coarser LOD's draw: the forward lists draw the finest over
        // every placement, as before.)
        !self.scene.lod_alternate(d) && self.water_hidden(d).is_none() && self.in_view(d)
    }

    /// Whether draw `d` can put a pixel in this view: its own bounds (a
    /// world surface) or any of its placements' spheres (an instanced
    /// model surface) touch the view's side planes; anything without
    /// bounds (sky, effect pools) always.
    fn in_view(&self, d: &crate::scene::Draw) -> bool {
        let Some(p) = &self.planes else { return true };
        if d.cull != crate::scene::NO_CULL
            && let Some(r) = self.scene.cull_ranges.get(d.cull as usize)
        {
            return r.spheres.iter().any(|s| crate::cull::visible(p, *s));
        }
        crate::cull::visible(p, crate::cull::draw_sphere(d))
    }

    /// Why a water draw is left out this frame (`CODLEGACY_T7WATER=all` draws
    /// every one, `off` none): a flat sheet (a vista one whatever its
    /// height) whose top is above the eye is seen from below, where the
    /// exe's water programs (refraction of the lit scene, sky reflection)
    /// paint a mirrored slab over the sky; the camera is not under water in
    /// these maps.
    fn water_hidden(&self, d: &crate::scene::Draw) -> Option<&'static str> {
        static MODE: std::sync::OnceLock<String> = std::sync::OnceLock::new();
        let mode = MODE.get_or_init(|| {
            codlegacy_env::var("CODLEGACY_T7WATER")
                .unwrap_or_default()
                .to_ascii_lowercase()
        });
        let m = &self.scene.materials[d.material];
        if d.group != 3 || !m.techset.contains("water") {
            return None;
        }
        match mode.as_str() {
            "off" => return Some("CODLEGACY_T7WATER=off"),
            "all" => return None,
            _ => {}
        }
        let flat = d.maxs[2] - d.mins[2] < 4.0;
        let vista = m.name.contains("vista");
        (self.eye[2] < d.maxs[2] && (flat || vista)).then_some("eye below the sheet")
    }

    fn log_water(&self, ready: &[Ready]) {
        static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
        if !*ON.get_or_init(|| codlegacy_env::var("CODLEGACY_T7WATERLOG").is_ok_and(|v| v != "0")) {
            return;
        }
        let Ok(mut seen) = self.water_seen.lock() else { return };
        for r in ready {
            let d = &self.scene.draws[r.draw];
            let m = &self.scene.materials[d.material];
            if d.group != 3 || !m.techset.contains("water") {
                continue;
            }
            let why = self.water_hidden(d);
            let v = u8::from(why.is_none());
            if seen.insert(r.draw, v) == Some(v) {
                continue;
            }
            let area = (d.maxs[0] - d.mins[0]) as f64 * (d.maxs[1] - d.mins[1]) as f64;
            eprintln!(
                "t7 waterlog: surface #{} {} ({}) list {} range {}: z {:.0}..{:.0}, xy extent {:.0} x {:.0} (bounds area {:.3e}), eye ({:.0}, {:.0}, {:.0}), source resolvedPostSun, {}",
                d.source,
                m.name,
                m.techset,
                r.list,
                ["A", "B", "C", "D", "none"].get(d.range as usize).unwrap_or(&"?"),
                d.mins[2],
                d.maxs[2],
                d.maxs[0] - d.mins[0],
                d.maxs[1] - d.mins[1],
                area,
                self.eye[0],
                self.eye[1],
                self.eye[2],
                why.map_or("drawn".to_owned(), |w| format!("skipped: {w}")),
            );
        }
    }

    pub fn ready_draws(&self) -> Vec<usize> {
        self.ready.iter().map(|r| r.draw).collect()
    }

    pub fn new(gpu: Gpu, scene: Arc<Scene>) -> Self {
        Self { gpu, scene, ready: Vec::new(), code_bufs: HashMap::new(), pipelines: HashMap::new(), empty_textures: HashMap::new(), empty_buffer: None, cull: None, water_depth: None, resolved: None, depth_copy: None, float_z: None, report: Report::default(), fx: fx_draw::FxDraw::default(), volumetrics: None, eye: [0.0; 3], planes: None, code_scratch: Vec::new(), water_seen: Default::default() }
    }

    fn make_cull(&self, targets: &Targets, lighting: &Lighting) -> Result<Cull, String> {
        let patch = fastfile_t7::shared::zone(&self.scene.zone_dir, "core_patch").ok_or("core_patch.ff not read")?;
        let shaders = crate::compute::code_shaders(&patch.image, &patch.capture);
        let make = |n: &str| crate::compute::Program::new(&self.gpu, shaders.get(n).ok_or_else(|| format!("{n} not in core_patch"))?, &[]);
        let programs = [make("light_culling_lights")?, make("light_culling_probes")?];
        let tiles = [targets.width.div_ceil(CULL_TILE), targets.height.div_ceil(CULL_TILE)];
        let mask = |label: &str| {
            let texture = crate::audit::texture(&self.gpu, &wgpu::TextureDescriptor {
                label: Some(label),
                size: wgpu::Extent3d { width: tiles[0], height: tiles[1], depth_or_array_layers: CULL_LAYERS },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::R32Uint,
                usage: wgpu::TextureUsages::STORAGE_BINDING | wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_SRC,
                view_formats: &[],
            });
            let view = texture.create_view(&wgpu::TextureViewDescriptor { dimension: Some(wgpu::TextureViewDimension::D2Array), ..Default::default() });
            crate::gpu::Tex { texture, view }
        };
        let masks = [mask("visibleLights"), mask("visibleProbes")];
        let per_scene = crate::audit::buffer(&self.gpu.device, &wgpu::BufferDescriptor { label: Some("t7 light culling PerSceneConsts"), size: 1152, usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST, mapped_at_creation: false });
        let args: Vec<_> = programs.iter().flat_map(|p| p.args.iter().filter(|a| a.ty & 0xff00 == 0x400 && a.buffer == 1).cloned()).collect();
        let mut groups: [Vec<wgpu::BindGroup>; 2] = [Vec::new(), Vec::new()];
        for (i, p) in programs.iter().enumerate() {
            groups[i] = p.bind(&self.gpu, |slot| match slot.name.as_str() {
                "visibleLights" | "visibleProbes" => Some(Res::View(&masks[i].view)),
                "PerSceneConsts" => Some(Res::Buffer(&per_scene)),
                n => lighting.named(n, targets),
            })?;
        }
        Ok(Cull { programs, masks, groups, tiles, per_scene, args })
    }

    fn make_float_z(&self, targets: &Targets, depth: &wgpu::TextureView) -> FloatZ {
        let dev = &self.gpu.device;
        let m = dev.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some("floatZ (exe encoding, forward)"), source: wgpu::ShaderSource::Wgsl(FLOATZ_CONVERT.into()) });
        let pipeline = dev.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("floatZ (exe encoding, forward)"),
            layout: None,
            vertex: wgpu::VertexState { module: &m, entry_point: Some("vs"), compilation_options: Default::default(), buffers: &[] },
            primitive: Default::default(),
            depth_stencil: None,
            multisample: Default::default(),
            fragment: Some(wgpu::FragmentState { module: &m, entry_point: Some("fs"), compilation_options: Default::default(), targets: &[Some(wgpu::TextureFormat::R32Float.into())] }),
            multiview_mask: None,
            cache: crate::pipecache::get(&dev).as_ref(),
        });
        let k = crate::audit::buffer(dev, &wgpu::BufferDescriptor { label: Some("floatZ scale (forward)"), size: 16, usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST, mapped_at_creation: false });
        self.gpu.queue.write_buffer_p(&k, 0, bytemuck::cast_slice(&[63.0 / (64.0 * 4.0), 0.0, 0.0, 0.0f32]));
        let out = crate::gpu::target(&self.gpu, "floatZ (exe encoding, forward)", targets.width.max(1), targets.height.max(1), wgpu::TextureFormat::R32Float, wgpu::TextureUsages::TEXTURE_BINDING);
        let group = dev.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("floatZ (exe encoding, forward)"),
            layout: &pipeline.get_bind_group_layout(0),
            entries: &[wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(depth) }, wgpu::BindGroupEntry { binding: 1, resource: k.as_entire_binding() }],
        });
        FloatZ { pipeline, k, out, group }
    }

    /// The scene depth as it stands copied for the draws that read it
    /// (`floatZ`), and converted to the exe's encoding.
    pub(crate) fn copy_depth(&self, enc: &mut wgpu::CommandEncoder, targets: &Targets) {
        let Some((dc, _)) = self.depth_copy.as_ref() else { return };
        enc.copy_texture_to_texture(targets.depth.texture.as_image_copy(), dc.as_image_copy(), dc.size());
        let Some(f) = self.float_z.as_ref() else { return };
        let mut p = enc.render_pass_p(&wgpu::RenderPassDescriptor {
            label: Some("floatZ (exe encoding, forward)"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment { view: &f.out.view, resolve_target: None, depth_slice: None, ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT), store: wgpu::StoreOp::Store } })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        p.set_pipeline_p(&f.pipeline);
        p.set_bind_group_p(0, &f.group, &[]);
        p.draw_p(0..3, 0..1);
    }

    fn make_water_depth(&self, targets: &Targets, lighting: &Lighting) -> Result<WaterDepth, String> {
        let patch = fastfile_t7::shared::zone(&self.scene.zone_dir, "core_patch").ok_or("core_patch.ff not read")?;
        let shaders = crate::compute::code_shaders(&patch.image, &patch.capture);
        let cs = shaders.get("downsample_depth_4x_compute").ok_or("downsample_depth_4x_compute not in core_patch")?;
        // (The depth is read as an unfilterable float; the output is the
        // exe's R32_FLOAT target.)
        let program = crate::compute::Program::new_opts(&self.gpu, cs, &[(0, "r32float")], &[0], true)?;
        let (w, h) = (targets.width, targets.height);
        let size = [w.div_ceil(4), h.div_ceil(4)];
        let out = crate::gpu::target(&self.gpu, "floatZDownscaled", size[0], size[1], wgpu::TextureFormat::R32Float, wgpu::TextureUsages::STORAGE_BINDING | wgpu::TextureUsages::TEXTURE_BINDING);
        let mut consts = [[0f32; 4]; 17];
        consts[7] = [4.0, 0.0, 0.0, 0.0];
        consts[16] = [1.0 / w as f32, 1.0 / h as f32, size[0] as f32, size[1] as f32];
        let cb = crate::gpu::buffer(&self.gpu, "t7 floatZDownscaled PostFxCBuffer", bytemuck::cast_slice(&consts), wgpu::BufferUsages::UNIFORM);
        let sampler = self.gpu.device.create_sampler(&wgpu::SamplerDescriptor { label: Some("depthSampler"), ..Default::default() });
        let groups = program.bind(&self.gpu, |slot| match slot.name.as_str() {
            "PostFxCBuffer" => Some(Res::Buffer(&cb)),
            // (floatZ in the exe's encoding, as the frame has it.)
            "floatZSampler" => match self.float_z.as_ref() {
                Some(f) => Some(Res::View(&f.out.view)),
                None => lighting.named("GBufferDepth", targets),
            },
            "depthSampler" => Some(Res::Sampler(&sampler)),
            "postEffect1" => Some(Res::View(&out.view)),
            _ => None,
        })?;
        Ok(WaterDepth { program, out, groups, size })
    }

    fn record_water_depth(&self, enc: &mut wgpu::CommandEncoder) {
        let Some(w) = self.water_depth.as_ref() else { return };
        let mut pass = enc.compute_pass_p(&wgpu::ComputePassDescriptor { label: Some("t7 floatZDownscaled"), timestamp_writes: None });
        pass.set_pipeline_p(&w.program.pipeline);
        for (k, g) in w.groups.iter().enumerate() {
            pass.set_bind_group_p(k as u32, g, &[]);
        }
        let [x, y, z] = w.program.groups_for([w.size[0], w.size[1], 1]);
        pass.dispatch_p(x, y, z);
    }

    /// The light and probe masks the culling writes (checks).
    pub fn cull_masks(&self) -> Option<[&wgpu::Texture; 2]> {
        self.cull.as_ref().map(|c| [&c.masks[0].texture, &c.masks[1].texture])
    }

    /// The light culling, before the forward draws: lights and probes as
    /// the lighting stage packed them this frame.
    fn record_cull(&self, enc: &mut wgpu::CommandEncoder, counts: &crate::lighting_globals::Counts) {
        let Some(c) = self.cull.as_ref() else { return };
        let mut pass = enc.compute_pass_p(&wgpu::ComputePassDescriptor { label: Some("t7 light culling"), timestamp_writes: None });
        // Probes, then lights (`141d5de20`).
        for (i, n) in [(1, counts.ref_probes), (0, counts.lights)] {
            if n == 0 {
                continue;
            }
            let p = &c.programs[i];
            pass.set_pipeline_p(&p.pipeline);
            for (k, g) in c.groups[i].iter().enumerate() {
                pass.set_bind_group_p(k as u32, g, &[]);
            }
            let [x, y, z] = p.groups_for([c.tiles[0], c.tiles[1], ((n + 31) & !31) << 4 >> 5]);
            pass.dispatch_p(x, y, z);
        }
    }

    fn empty_texture(&mut self, dim: TextureDim, kind: TextureKind) {
        if self.empty_textures.contains_key(&format!("{dim:?}{kind:?}")) {
            return;
        }
        let format = match kind {
            TextureKind::Uint => wgpu::TextureFormat::R32Uint,
            TextureKind::Sint => wgpu::TextureFormat::R32Sint,
            TextureKind::Depth => wgpu::TextureFormat::Depth32Float,
            _ => wgpu::TextureFormat::R32Float,
        };
        let (dimension, layers, view) = match dim {
            TextureDim::D1 => (wgpu::TextureDimension::D1, 1, wgpu::TextureViewDimension::D1),
            TextureDim::D2Array => (wgpu::TextureDimension::D2, 1, wgpu::TextureViewDimension::D2Array),
            TextureDim::D3 => (wgpu::TextureDimension::D3, 1, wgpu::TextureViewDimension::D3),
            TextureDim::Cube => (wgpu::TextureDimension::D2, 6, wgpu::TextureViewDimension::Cube),
            TextureDim::CubeArray => (wgpu::TextureDimension::D2, 6, wgpu::TextureViewDimension::CubeArray),
            _ => (wgpu::TextureDimension::D2, 1, wgpu::TextureViewDimension::D2),
        };
        let texture = crate::audit::texture(&self.gpu, &wgpu::TextureDescriptor {
            label: Some("t7 forward empty list"),
            size: wgpu::Extent3d { width: 1, height: 1, depth_or_array_layers: layers },
            mip_level_count: 1,
            sample_count: 1,
            dimension,
            format,
            usage: wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor { dimension: Some(view), ..Default::default() });
        self.empty_textures.insert(format!("{dim:?}{kind:?}"), crate::gpu::Tex { texture, view });
    }

    /// Takes the pipelines [`crate::warm::run`] made.
    pub fn adopt(&mut self, w: &mut crate::warm::Warm) {
        self.report.pipelines_made += w.forward_made;
        self.report.pipelines_failed.append(&mut w.forward_failed);
        for (k, v) in w.forward.drain() {
            self.pipelines.insert(k, v);
        }
    }

    fn pipeline(&mut self, t: &Arc<Technique>, group: u8) -> Option<Arc<Pipeline>> {
        let key = (Arc::as_ptr(t) as usize, group);
        if let Some(p) = self.pipelines.get(&key) {
            return p.clone();
        }
        if std::env::var_os("T7_FORWARD_TRACE").is_some() {
            eprintln!("forward: making {} (group {group})", t.techset);
        }
        let p = match make_pipeline_for(&self.gpu, t, group, &[SCENE_FORMAT], DEPTH, &floatz_slots(t)) {
            Ok(p) => {
                self.report.pipelines_made += 1;
                Some(Arc::new(p))
            }
            Err(e) => {
                self.report.pipelines_failed.push(format!("{} (group {group}): {e}", t.techset));
                None
            }
        };
        self.pipelines.insert(key, p.clone());
        p
    }

    /// Makes every forward draw's pipeline and bindings; again after the
    /// lighting stage is (re)sized, since its resources are bound.
    pub fn prepare(&mut self, g: &mut GBuffer, targets: &Targets, lighting: &Lighting) {
        // (Where the time goes, for the game log: the first prepare of a
        // Skyjacked load took 1.97 s.)
        let t0 = std::time::Instant::now();
        let made0 = self.report.pipelines_made;
        self.cull = match self.make_cull(targets, lighting) {
            Ok(c) => Some(c),
            Err(e) => {
                self.report.pipelines_failed.push(format!("light culling: {e} (the forward draws reading it are left out)"));
                None
            }
        };
        let dc = crate::audit::texture(&self.gpu, &wgpu::TextureDescriptor {
            label: Some("floatZ (copy)"),
            size: wgpu::Extent3d { width: targets.width.max(1), height: targets.height.max(1), depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: DEPTH,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let dv = dc.create_view(&wgpu::TextureViewDescriptor { aspect: wgpu::TextureAspect::DepthOnly, ..Default::default() });
        let t_cull = t0.elapsed();
        self.float_z = if raw_floatz() { None } else { Some(self.make_float_z(targets, &dv)) };
        self.depth_copy = Some((dc, dv));
        self.water_depth = match self.make_water_depth(targets, lighting) {
            Ok(w) => Some(w),
            Err(e) => {
                self.report.pipelines_failed.push(format!("floatZDownscaled: {e} (the water reading it is left out)"));
                None
            }
        };
        let t_depth = t0.elapsed();
        // (A map with no lit fog volumes makes none of the passes; a view
        // whose list is empty records nothing. On by default,
        // `T7_VOLUMETRIC_GPU=0` off (`crate::volumetric::gpu_on`).)
        self.volumetrics = if lighting.lit_fog_volume_count() == 0 || !crate::volumetric::gpu_on_for(self.scene.workshop) {
            None
        } else {
            let visible = self.cull.as_ref().map(|c| [&c.masks[0].view, &c.masks[1].view]);
            match crate::volumetric_pass::VolumetricPass::new(&self.gpu, &self.scene.zone_dir, targets, lighting, visible) {
                Ok(v) => Some(v),
                Err(e) => {
                    self.report.pipelines_failed.push(format!("t7 volumetrics: not made: {e} (the lit fog volumes are not drawn)"));
                    None
                }
            }
        };
        let t_vol = t0.elapsed();
        self.resolved = Some(crate::gpu::target(&self.gpu, "resolvedPostSun", targets.width, targets.height, SCENE_FORMAT, wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST));
        self.ready.clear();
        self.report.draws_skipped.clear();
        let scene = self.scene.clone();
        self.report.skipped_draws.clear();
        self.report.stand_ins.clear();
        let mut skipped: HashMap<String, usize> = HashMap::new();
        let mut total = 0;
        let no_forward = crate::surf_debug::get().no_forward;
        let mut t_pipes = std::time::Duration::ZERO;
        for (di, d) in scene.draws.iter().enumerate() {
            let m = &scene.materials[d.material];
            let Some(t) = m.forward.clone() else { continue };
            total += 1;
            if no_forward {
                self.report
                    .skipped_draws
                    .push((di, "left out by CODLEGACY_T7_DEBUG_SURF=no_forward".into()));
                continue;
            }
            let t_p = std::time::Instant::now();
            let p = self.pipeline(&t, d.group);
            t_pipes += t_p.elapsed();
            let Some(p) = p else {
                self.report.skipped_draws.push((di, format!("pipeline failed ({})", t.techset)));
                continue;
            };
            match self.bind(g, &p, &t, d.material, targets, lighting) {
                Ok(groups) => self.ready.push(Ready { draw: di, pipeline: p, groups, key: m.sort_key, list: d.list, reads_depth: !floatz_slots(&t).is_empty(), reads_resolved: t.args.iter().any(|a| a.ty & 0xff00 == 0x500 && a.u[0] == CODE_TEXTURE_RESOLVED_POST_SUN) }),
                Err(e) => {
                    *skipped.entry(format!("{}: {e}", m.techset)).or_default() += 1;
                    self.report.skipped_draws.push((di, e));
                }
            }
        }
        let mut sk: Vec<_> = skipped.into_iter().collect();
        sk.sort();
        self.report.draws_skipped = sk.into_iter().map(|(e, n)| format!("{n} draws of {e}")).collect();
        // The lists in the frame's order, each by sort key (stable: the
        // zone's order within a key); a draw in a list the frame does not
        // draw after the lighting is reported.
        let mut off_list: HashMap<u8, usize> = HashMap::new();
        let mut off_draws = Vec::new();
        self.ready.retain(|r| {
            let keep = LIST_ORDER.contains(&r.list);
            if !keep {
                *off_list.entry(r.list).or_default() += 1;
                off_draws.push((r.draw, format!("forward surface in view list {} (drawn before the lighting): its technique set has no G-buffer technique", r.list)));
            }
            keep
        });
        self.report.skipped_draws.extend(off_draws);
        for (l, n) in off_list {
            self.report.draws_skipped.push(format!("{n} draws in view list {l}, which is drawn before the lighting (G-buffer): their technique set has no G-buffer technique"));
        }
        self.ready.sort_by_key(|r| (LIST_ORDER.iter().position(|l| *l == r.list), r.key));
        // Back to front: the exe sorts these lists by key, then reverses
        // them.
        for l in BACK_TO_FRONT {
            let (a, b) = (self.ready.iter().position(|r| r.list == l), self.ready.iter().rposition(|r| r.list == l));
            if let (Some(a), Some(b)) = (a, b) {
                self.ready[a..=b].reverse();
            }
        }
        // (An opaque forward material whose forward draw is not ready must
        // not leave its depth prepass in the G-buffer: a black hole.)
        {
            let ready: std::collections::HashSet<usize> = self.ready.iter().map(|r| r.draw).collect();
            let (dropped, names) = g.drop_orphan_forward_depth(&ready);
            if dropped > 0 {
                self.report.draws_skipped.push(format!("{dropped} depth prepass draws of opaque forward materials left out, their forward draws not being ready (they drew black holes): {}", names.join(", ")));
            }
        }
        self.report.draws_ready = self.ready.len();
        self.report.draws_total = total;
        let t_draws = t0.elapsed();
        let made_draws = self.report.pipelines_made - made0;
        self.prepare_fx(g, targets, lighting);
        let t_all = t0.elapsed();
        let s = |d: std::time::Duration| d.as_secs_f32();
        diag::info!(
            World,
            "t7 native: timing: forward prepare {:.2}s: light culling {:.2}s, depth copies {:.2}s, volumetrics {:.2}s, {total} draws {:.2}s (making {made_draws} pipelines {:.2}s, binding the rest), effects {:.2}s ({} more pipelines)",
            s(t_all),
            s(t_cull),
            s(t_depth - t_cull),
            s(t_vol - t_depth),
            s(t_draws - t_vol),
            s(t_pipes),
            s(t_all - t_draws),
            self.report.pipelines_made - made0 - made_draws
        );
    }

    #[allow(clippy::type_complexity)]
    fn bind(&mut self, g: &mut GBuffer, p: &Pipeline, t: &Technique, material: usize, targets: &Targets, lighting: &Lighting) -> Result<[wgpu::BindGroup; 3], String> {
        let scene = self.scene.clone();
        let m = &scene.materials[material];
        let dev = self.gpu.device.clone();
        enum Cb<'a> {
            Own(wgpu::Buffer, u64),
            Code(&'a wgpu::Buffer),
        }
        let mut cbs: Vec<(u32, Cb)> = Vec::new();
        for &(binding, slot, registers) in &p.layout.cbuffers {
            let st: u16 = if binding >= 32 { 1 } else { 0 };
            let size = (registers.max(1) * 16) as usize;
            let args: Vec<_> = t.args.iter().filter(|a| a.ty & 0xff00 == 0x400 && stage(a.ty) == st && u32::from(a.buffer) == slot).cloned().collect();
            if !args.is_empty() {
                // (One buffer per technique and binding: its contents are
                // the view's, the same for every draw of the technique.)
                let key = (t as *const Technique as usize, binding);
                let b = self
                    .code_bufs
                    .entry(key)
                    .or_insert_with(|| {
                        let buffer = crate::audit::buffer(&dev, &wgpu::BufferDescriptor { label: Some("t7 forward code constants"), size: size as u64, usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST, mapped_at_creation: false });
                        (buffer, size, args, Vec::new())
                    })
                    .0
                    .clone();
                cbs.push((binding, Cb::Own(b, size as u64)));
                continue;
            }
            let name = rdef::cbuffer_name(&p.binds[st as usize], slot).ok_or_else(|| format!("b{slot}: no name in RDEF"))?;
            let index = (t.slot * 6) as u8 + t.buffer_set + if st == 0 { 2 } else { 4 };
            if let Some(data) = m.cbuffers.iter().find(|(i, n, _)| *i == index && n == name).map(|(_, _, d)| d.clone()) {
                let mut bytes = data;
                bytes.resize(size.max(bytes.len()), 0);
                let len = bytes.len() as u64;
                cbs.push((binding, Cb::Own(crate::gpu::buffer(&self.gpu, name, &bytes, wgpu::BufferUsages::UNIFORM), len)));
                continue;
            }
            match lighting.named(name, targets) {
                Some(Res::Buffer(b)) => cbs.push((binding, Cb::Code(b))),
                _ => {
                    // A material constant buffer of another name in its slot.
                    let data = m.cbuffers.iter().find(|(i, _, _)| *i == index).map(|(_, _, d)| d.clone()).ok_or_else(|| format!("constant buffer {name} (b{slot}) neither the material's nor made by the frame"))?;
                    let mut bytes = data;
                    bytes.resize(size.max(bytes.len()), 0);
                    let len = bytes.len() as u64;
                    cbs.push((binding, Cb::Own(crate::gpu::buffer(&self.gpu, name, &bytes, wgpu::BufferUsages::UNIFORM), len)));
                }
            }
        }
        enum Tx<'a> {
            Image(String),
            Check,
            StandIn([u8; 4]),
            View(&'a wgpu::TextureView),
            Buffer(&'a wgpu::Buffer),
            Bones,
            Instances,
            EmptyTexture(TextureDim, TextureKind),
            Mask(usize),
            WaterDepth,
            Resolved,
            DepthCopy,
            EmptyBuffer,
            Lmap,
            Image2(&'static str),
        }
        let mut txs: Vec<(u32, Tx)> = Vec::new();
        for &(binding, slot, dim, kind, ps) in &p.layout.textures {
            let st: u16 = u16::from(ps);
            let a = t.args.iter().find(|a| matches!(a.ty & 0xff00, 0x200 | 0x500 | 0x600) && stage(a.ty) == st && u32::from(a.location) == slot);
            let name = p.binds[st as usize].iter().find(|b| b.kind != rdef::CBUFFER && b.kind != rdef::SAMPLER && b.point == slot).map(|b| b.name.clone()).unwrap_or_default();
            let r = match a.map(|a| (a.ty & 0xff00, a.u[0])) {
                Some((0x200, hash)) => {
                    let img = m.textures.iter().find(|(h, _)| *h == hash).map(|(_, n)| n.clone()).ok_or_else(|| format!("t{slot}: texture {hash:08x} not in the material's table"))?;
                    if img.starts_with('<') {
                        return Err(format!("t{slot}: texture {img} not named in the zone"));
                    }
                    if g.image(&img) {
                        Tx::Image(img)
                    } else if g.check_bindings_without_images {
                        Tx::Check
                    } else {
                        match crate::surf_debug::stand_in(&name, &img, p.masked_or_blended) {
                            Some((rgba, what)) => {
                                self.report.stand_ins.push(format!("{} ({}): t{slot} {name} <- {img}: {what}", m.name, m.techset));
                                g.stand_in(rgba);
                                Tx::StandIn(rgba)
                            }
                            None => return Err(format!("t{slot}: image {img} missing ({name}{})", if p.masked_or_blended { ", masked or blended surface" } else { "" })),
                        }
                    }
                }
                Some((0x500, CODE_TEXTURE_RESOLVED_NORMAL)) => Tx::View(&targets.resolved_normal.view),
                Some((0x500, c)) if crate::weather::CODE_TEXTURES.contains(&c) => {
                    // (The weather grime maps' neutral defaults, as in the G-buffer.)
                    let img = crate::weather::DEFAULT_IMAGES[crate::weather::CODE_TEXTURES.iter().position(|&x| x == c).unwrap_or(0)];
                    if g.image(img) {
                        Tx::Image(img.to_owned())
                    } else if g.check_bindings_without_images {
                        Tx::Check
                    } else {
                        return Err(format!("t{slot}: weather image {img} missing"));
                    }
                }
                Some((0x600, CODE_BUFFER_BONES)) => Tx::Bones,
                Some((0x600, CODE_BUFFER_INSTANCES)) => Tx::Instances,
                _ if name == "gEnvBRDFGlass" => {
                    // The glass BRDF table, an image of core_gfx as the
                    // generic one the lighting loads.
                    if !g.image("$env_brdf_glass") {
                        return Err("$env_brdf_glass missing".into());
                    }
                    Tx::Image2("$env_brdf_glass")
                }
                Some((0x500, CODE_TEXTURE_RESOLVED_POST_SUN)) => Tx::Resolved,
                // floatZ: the scene depth (view 0 of the scene target).
                Some((0x500, CODE_TEXTURE_FLOATZ)) => Tx::DepthCopy,
                _ if name == "ifloatZSceneDownsampled" || name == "floatZDownscaledSampler" => {
                    if self.water_depth.is_none() {
                        return Err(format!("{name}: floatZDownscaled not made"));
                    }
                    Tx::WaterDepth
                }
                _ if matches!(name.as_str(), "visibleLights" | "visibleProbes") => {
                    if self.cull.is_none() {
                        return Err(format!("{name}: light culling not made"));
                    }
                    Tx::Mask(usize::from(name == "visibleProbes"))
                }
                _ if matches!(name.as_str(), "lightConstants" | "refProbeConstants" | "refProbeAttenuationConstants") => {
                    // The lighting stage's light and probe packs (the
                    // deferred lighting's constant buffers), read here as
                    // structured buffers.
                    let pack = match name.as_str() {
                        "lightConstants" => "LightingPackCBuffer",
                        "refProbeConstants" => "RefProbePackCBuffer",
                        _ => "RefProbeBlendPackCBuffer",
                    };
                    match lighting.named(pack, targets) {
                        Some(Res::Buffer(b)) => Tx::Buffer(b),
                        _ => return Err(format!("{name}: the lighting's {pack} not made")),
                    }
                }
                // shaderConstantSetBuffer (code buffer 15; the draw state's
                // +0x1cd8 from the backend's +0x96380, `141df0cf0`): 384-byte
                // shader constant sets, picked by the draw's instance record
                // (word 1). The frame uploads sets 0 .. count from the
                // backend's array (`141d4e220`); effects and entities add
                // sets through the allocator (`141dea080`, `141575ca0`),
                // whose count restarts at 1 every frame (`141dcb3c0`), so set
                // 0, the one the world and static models carry, is never
                // written: zero.
                Some((0x600, 15)) => {
                    if self.empty_buffer.is_none() {
                        self.empty_buffer = Some(crate::audit::buffer(&self.gpu.device, &wgpu::BufferDescriptor { label: Some("t7 forward empty list"), size: 1 << 16, usage: wgpu::BufferUsages::STORAGE, mapped_at_creation: false }));
                    }
                    Tx::EmptyBuffer
                }
                // The effects' sprite light map (`fx_draw`).
                _ if name == "lmapTex" && self.fx.lmap.is_some() => Tx::Lmap,
                _ if EMPTY_UNTIL_LIGHT_CULLING.contains(&name.as_str()) => {
                    if matches!(dim, TextureDim::Buffer | TextureDim::Raw) || kind == TextureKind::RawBuffer {
                        if self.empty_buffer.is_none() {
                            self.empty_buffer = Some(crate::audit::buffer(&self.gpu.device, &wgpu::BufferDescriptor { label: Some("t7 forward empty list"), size: 1 << 16, usage: wgpu::BufferUsages::STORAGE, mapped_at_creation: false }));
                        }
                        Tx::EmptyBuffer
                    } else {
                        self.empty_texture(dim, kind);
                        Tx::EmptyTexture(dim, kind)
                    }
                }
                other => match lighting.named(&name, targets) {
                    Some(Res::View(v)) => Tx::View(v),
                    Some(Res::Buffer(b)) => Tx::Buffer(b),
                    _ => {
                        let what = match other {
                            Some((0x500, i)) => format!("code texture {i:#x} {name}"),
                            Some((0x600, i)) => format!("code buffer {i} {name}"),
                            _ => format!("t{slot} {name} (no argument)"),
                        };
                        return Err(format!("{what} not made by the frame yet"));
                    }
                },
            };
            txs.push((binding, r));
        }
        if txs.iter().any(|(_, r)| matches!(r, Tx::Check)) && g.check_image.is_none() {
            g.check_image = Some(crate::gpu::target(&self.gpu, "check-only", 1, 1, wgpu::TextureFormat::Rgba8Unorm, wgpu::TextureUsages::empty()));
        }
        enum Sm<'a> {
            Material(u16),
            Code(&'a wgpu::Sampler),
        }
        let mut sms: Vec<(u32, Sm)> = Vec::new();
        for &(binding, slot, ps) in &p.layout.samplers {
            let st: u16 = u16::from(ps);
            match t.args.iter().find(|a| a.ty & 0xff00 == 0x300 && stage(a.ty) == st && u32::from(a.location) == slot) {
                Some(a) => {
                    let state = m.samplers.iter().find(|(h, _)| *h == a.u[0]).map(|(_, s)| *s).ok_or_else(|| format!("s{slot}: sampler {:08x} not in the material's table", a.u[0]))?;
                    g.sampler(state)?;
                    sms.push((binding, Sm::Material(state)));
                }
                None => {
                    let name = p.binds[st as usize].iter().find(|b| b.kind == rdef::SAMPLER && b.point == slot).map(|b| b.name.clone()).unwrap_or_default();
                    let s = lighting.named_sampler(&name).ok_or_else(|| format!("code sampler s{slot} {name} not made by the frame yet"))?;
                    sms.push((binding, Sm::Code(s)));
                }
            }
        }
        let g0: Vec<wgpu::BindGroupEntry> = cbs
            .iter()
            .map(|(b, c)| wgpu::BindGroupEntry {
                binding: *b,
                resource: match c {
                    Cb::Own(buf, len) => wgpu::BindingResource::Buffer(wgpu::BufferBinding { buffer: buf, offset: 0, size: wgpu::BufferSize::new(*len) }),
                    Cb::Code(buf) => buf.as_entire_binding(),
                },
            })
            .collect();
        let g1: Vec<wgpu::BindGroupEntry> = txs
            .iter()
            .map(|(b, r)| wgpu::BindGroupEntry {
                binding: *b,
                resource: match r {
                    Tx::Image(n) => wgpu::BindingResource::TextureView(&g.images[n].as_ref().unwrap().view),
                    Tx::Check => wgpu::BindingResource::TextureView(&g.check_image.as_ref().unwrap().view),
                    Tx::StandIn(rgba) => wgpu::BindingResource::TextureView(&g.stand_in_images[rgba].view),
                    Tx::View(v) => wgpu::BindingResource::TextureView(v),
                    Tx::Buffer(buf) => buf.as_entire_binding(),
                    Tx::Bones => g.bones.as_entire_binding(),
                    Tx::EmptyTexture(d, k) => wgpu::BindingResource::TextureView(&self.empty_textures[&format!("{d:?}{k:?}")].view),
                    Tx::DepthCopy => match self.float_z.as_ref() {
                        Some(f) => wgpu::BindingResource::TextureView(&f.out.view),
                        None => wgpu::BindingResource::TextureView(&self.depth_copy.as_ref().unwrap().1),
                    },
                    Tx::Resolved => wgpu::BindingResource::TextureView(&self.resolved.as_ref().unwrap().view),
                    Tx::WaterDepth => wgpu::BindingResource::TextureView(&self.water_depth.as_ref().unwrap().out.view),
                    Tx::Mask(i) => wgpu::BindingResource::TextureView(&self.cull.as_ref().unwrap().masks[*i].view),
                    Tx::EmptyBuffer => self.empty_buffer.as_ref().unwrap().as_entire_binding(),
                    Tx::Lmap => wgpu::BindingResource::TextureView(&self.fx.lmap.as_ref().unwrap().view),
                    Tx::Image2(n) => wgpu::BindingResource::TextureView(&g.images[*n].as_ref().unwrap().view),
                    Tx::Instances => g.instances.as_entire_binding(),
                },
            })
            .collect();
        let g2: Vec<wgpu::BindGroupEntry> = sms
            .iter()
            .map(|(b, s)| wgpu::BindGroupEntry {
                binding: *b,
                resource: wgpu::BindingResource::Sampler(match s {
                    Sm::Material(st) => &g.samplers[st],
                    Sm::Code(s) => s,
                }),
            })
            .collect();
        let scope = dev.push_error_scope(wgpu::ErrorFilter::Validation);
        let groups = [
            dev.create_bind_group(&wgpu::BindGroupDescriptor { label: Some("t7 fwd cb"), layout: &p.layout.groups[0], entries: &g0 }),
            dev.create_bind_group(&wgpu::BindGroupDescriptor { label: Some("t7 fwd tex"), layout: &p.layout.groups[1], entries: &g1 }),
            dev.create_bind_group(&wgpu::BindGroupDescriptor { label: Some("t7 fwd samp"), layout: &p.layout.groups[2], entries: &g2 }),
        ];
        if let Some(e) = pollster::block_on(scope.pop()) {
            return Err(format!("bind groups: {e}"));
        }
        Ok(groups)
    }

    /// Fills the code constant buffers for this view.
    pub fn update(&mut self, consts: &CodeConsts) {
        // skyRotationTransition (code constant 0x29), as the exe's view
        // setup writes it (`141dd8b90`): (sin a, cos a, the sky box's +0xc,
        // a dvar (default 0)), a = (the sky box's rotation + a rotation
        // dvar (default 0)) in radians + atan2 of a 2-D vector of the view
        // (backend view +0x2ec / +0x2f0, the client refdef's +0x2ec /
        // +0x2f0: written only by two script builtins, `140412770` /
        // `14040ac90`, each setting one component from its float argument;
        // zero until a map's script calls them, so the term is 0: no
        // script sky rotation is driven here).
        let mut consts = consts.clone();
        let (deg, z) = self.scene.map.light_refs().map_or((0.0, 0.0), |l| l.sky_rotation);
        let a = deg.to_radians();
        consts.set(crate::code::SKY_ROTATION_TRANSITION, vec![[a.sin(), a.cos(), z, 0.0]]);
        // skyMxR..sunMxB (0x2d..0x32): the host sets them from the map's
        // vukko blend (exe `141dd8b90`, `Post::sky_matrices`). Without the
        // post chain (no zone data): identity rows, D1 = +x, sunMxB.w = 1
        // (what Nuketown's default record gives).
        for (k, row) in [[1.0, 0.0, 0.0, 1.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0], [1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 1.0]].into_iter().enumerate() {
            if consts.get(crate::code::SKY_MX_R + k as u16).is_none() {
                consts.set(crate::code::SKY_MX_R + k as u16, vec![row]);
            }
        }
        if let Some(f) = &self.float_z {
            let near = consts.get(crate::lighting_globals::Z_NEAR).and_then(|r| r.first()).map_or(4.0, |r| r[0]).max(1e-3);
            self.gpu.queue.write_buffer_p(&f.k, 0, bytemuck::cast_slice(&[63.0 / (64.0 * near), 0.0, 0.0, 0.0f32]));
        }
        let consts = &consts;
        self.fx_consts(consts);
        if let Some(c) = &self.cull {
            let mut bytes = vec![0u8; 1152];
            for a in &c.args {
                let Some(rows) = consts.get((a.u[0] & 0xff) as u16) else { continue };
                let src: &[u8] = bytemuck::cast_slice(rows);
                let at = a.location as usize;
                let n = (a.size as usize).min(src.len()).min(bytes.len().saturating_sub(at));
                bytes[at..at + n].copy_from_slice(&src[..n]);
            }
            self.gpu.queue.write_buffer_p(&c.per_scene, 0, &bytes);
        }
        // (One scratch for the bytes, kept from frame to frame.)
        let mut bytes = std::mem::take(&mut self.code_scratch);
        for (buf, size, args, last) in self.code_bufs.values_mut() {
            bytes.clear();
            bytes.resize(*size, 0);
            for a in args.iter() {
                let index = (a.u[0] & 0xff) as u16;
                let Some(rows) = consts.get(index) else { continue };
                let src: &[u8] = bytemuck::cast_slice(rows);
                let at = a.location as usize;
                let n = (a.size as usize).min(src.len()).min(size.saturating_sub(at));
                bytes[at..at + n].copy_from_slice(&src[..n]);
            }
            if bytes != *last {
                self.gpu.queue.write_buffer_p(buf, 0, &bytes);
                last.clear();
                last.extend_from_slice(&bytes);
            }
        }
        self.code_scratch = bytes;
    }

    /// Records the forward draws into the lit scene `scene` over the
    /// G-buffer's depth.
    pub fn record(&self, enc: &mut wgpu::CommandEncoder, g: &GBuffer, targets: &Targets, scene: &wgpu::TextureView, scene_texture: &wgpu::Texture, counts: &crate::lighting_globals::Counts) {
        // (`fwd ...` marks: the forward stage's parts in the frame timing;
        // the host's `forward` mark is what follows the last.)
        self.record_cull(enc, counts);
        crate::timing::mark(enc, "fwd light cull");
        // The volumetric depth pre-pass before the opaque lists (`141d4dca0`).
        if let Some(v) = &self.volumetrics {
            v.record_prepass(enc);
            crate::timing::mark(enc, "fwd fog prepass");
        }
        // As the frame (`141d4e220`): lists 14, 7, 8 (bound to the resolve
        // target as it stands: `141d90660` / `141d907a0`); before lists 9,
        // 10 (`141d90700`), 20 and 21 (`141d4b2a0`) the scene copied into
        // it when the list has draws that read it; floatZDownscaled between
        // 20 and 21; list 22.
        let part = |l: &[u8]| -> &[Ready] {
            let a = self.ready.iter().position(|r| l.contains(&r.list));
            let b = self.ready.iter().rposition(|r| l.contains(&r.list));
            match (a, b) {
                (Some(a), Some(b)) => &self.ready[a..=b],
                _ => &[],
            }
        };
        self.log_water(&self.ready);
        self.draw(enc, g, targets, scene, part(&[14, 7, 8]));
        crate::timing::mark(enc, "fwd lists 14/7/8");
        self.draw_fx(enc, targets, scene);
        crate::timing::mark(enc, "fwd effects");
        self.resolve(enc, scene_texture, part(&[9]));
        self.draw(enc, g, targets, scene, part(&[9]));
        self.resolve(enc, scene_texture, part(&[10]));
        self.draw(enc, g, targets, scene, part(&[10]));
        self.resolve(enc, scene_texture, part(&[20]));
        self.draw(enc, g, targets, scene, part(&[20]));
        // (floatZDownscaled from floatZ as it stands after list 20.)
        if self.water_depth.is_some() && self.float_z.is_some() {
            self.copy_depth(enc, targets);
        }
        self.record_water_depth(enc);
        crate::timing::mark(enc, "fwd lists 9/10/20");
        // The lit fog volumes' march, blur and apply (`141d4ced0`,
        // `141d4d3c0`): after list 20 (and the effects), before 21.
        if let Some(v) = &self.volumetrics {
            v.record(enc, scene, scene_texture);
            crate::timing::mark(enc, "fwd fog");
        }
        self.resolve(enc, scene_texture, part(&[21]));
        self.draw(enc, g, targets, scene, part(&[21]));
        self.draw(enc, g, targets, scene, part(&[22]));
    }

    /// The lit fog volumes' list and constants for the frame (after the
    /// lighting's update); their `t7 volumetrics` lines.
    pub fn update_volumetrics(&mut self, lighting: &Lighting, camera: &render_h1::code::Camera, now: i32) -> Vec<String> {
        let Some(v) = self.volumetrics.as_mut() else { return Vec::new() };
        v.update(lighting, camera, now);
        std::mem::take(&mut v.report)
    }

    /// The scene copied into `resolvedPostSun` before a list that has
    /// draws.
    fn resolve(&self, enc: &mut wgpu::CommandEncoder, scene: &wgpu::Texture, list: &[Ready]) {
        let Some(r) = self.resolved.as_ref() else { return };
        // (Only when a draw that reads it is drawn this frame.)
        if !list.iter().any(|r| r.reads_resolved && self.drawn(r)) {
            return;
        }
        enc.copy_texture_to_texture(scene.as_image_copy(), r.texture.as_image_copy(), r.texture.size());
    }

    fn draw(&self, enc: &mut wgpu::CommandEncoder, g: &GBuffer, targets: &Targets, scene: &wgpu::TextureView, ready: &[Ready]) {
        // (A list with nothing drawn this frame records no pass.)
        if !ready.iter().any(|r| self.drawn(r)) {
            return;
        }
        // The depth as it stands, for the draws drawn that read it.
        if ready.iter().any(|r| r.reads_depth && self.drawn(r)) {
            self.copy_depth(enc, targets);
        }
        let ops = Some(wgpu::Operations { load: wgpu::LoadOp::Load, store: wgpu::StoreOp::Store });
        let mut rp = enc.render_pass_p(&wgpu::RenderPassDescriptor {
            label: Some("t7 forward"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment { view: scene, depth_slice: None, resolve_target: None, ops: wgpu::Operations { load: wgpu::LoadOp::Load, store: wgpu::StoreOp::Store } })],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: &targets.depth.view,
                depth_ops: ops,
                stencil_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Load, store: wgpu::StoreOp::Store }),
            }),
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        // (State the last draw set is not set again: the draws stay in the
        // exe's order, so only runs of one pipeline, binding or vertex
        // source save anything.)
        let mut last_pipeline: Option<&Arc<Pipeline>> = None;
        let mut last_groups: [Option<&wgpu::BindGroup>; 3] = [None; 3];
        let mut last_stencil = u32::MAX;
        let mut last_group = u8::MAX;
        for r in ready {
            let d = &self.scene.draws[r.draw];
            if !self.drawn(r) {
                continue;
            }
            let t = self.scene.materials[d.material].forward.as_ref().unwrap();
            let same_pipeline = last_pipeline.is_some_and(|p| Arc::ptr_eq(p, &r.pipeline));
            if !same_pipeline {
                rp.set_pipeline_p(&r.pipeline.pipeline);
                last_pipeline = Some(&r.pipeline);
                last_groups = [None; 3];
                last_group = u8::MAX;
            }
            let stencil = u32::from(t.state.stencil_ref);
            if stencil != last_stencil {
                rp.set_stencil_reference(stencil);
                last_stencil = stencil;
            }
            for (gi, bg) in r.groups.iter().enumerate() {
                if last_groups[gi] != Some(bg) {
                    rp.set_bind_group_p(gi as u32, bg, &[]);
                    last_groups[gi] = Some(bg);
                }
            }
            if d.group != last_group {
                last_group = d.group;
                for (k, &slot) in r.pipeline.slots.iter().enumerate() {
                    let buf = match (d.group, slot) {
                        (_, layout::INSTANCE_SLOT) => &g.instance_ids,
                        (3, 0) => &g.world[0],
                        (3, 1) => &g.world[1],
                        (1, 0) => &g.models[0],
                        (1, 1) => &g.models[1],
                        (1, 2) => &g.models[2],
                        _ => continue,
                    };
                    rp.set_vertex_buffer(k as u32, buf.slice(..));
                }
                let ib = if d.group == 3 { &g.world[2] } else { &g.models[3] };
                rp.set_index_buffer(ib.slice(..), wgpu::IndexFormat::Uint16);
            }
            rp.draw_indexed_p(d.first_index..d.first_index + d.index_count, d.base_vertex, d.instance..d.instance + d.instances);
        }
    }
}
