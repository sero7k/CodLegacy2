//! The post chain recorded as BlackOps3.exe records it after the lit scene
//! (exe 0x141d66110, then the orchestrator 0x141d62ee0): bloom constants;
//! `bloom_downsample_l0` (scene → 0x2d) and `_l1` (→ 0x35); the colour
//! grade LUT (`create_lut2dv_compute`); `_lx` down to 0x39 (one more per
//! 1080 rows above 1080); `_ev` (0x39 → the luminance target 0x23); the
//! blurs x (mips 5..1) and y (5..1, each adding the level below);
//! `exposure_compute`; `bloom_apply_lut` (bloom + exposure + grade, into
//! 0x48 and the luma target 0x4a); `resample_final_bm_null` into the
//! final image (display-encoded, sRGB curve; RGBA8).
//!
//! Gaps (listed in [`Post::report`] too): the LUT volume mask byte (cg
//! +0x1a6a42) is taken as the lighting state's bit and the slice blend as
//! settled (no transition between volumes); the profile setting
//! `r_sceneBrightness` that `resample_final` adds (·16/255) is taken as 0
//! (its default lives in the profile data, not read); the bloom
//! and colour-vision modifiers and boxes are not blended (the map's base
//! records for lighting state 0); AA: SMAA 1x (`crate::post_aa`).

#[allow(unused_imports)]
use crate::profile::{ComputeExt as _, EncExt as _, PassExt as _, QueueExt as _};
use std::collections::HashMap;
use std::path::Path;

use render_h1::code::Camera;
use render_h1::gpu::Gpu;

use crate::compute::{CodeShader, Program, Res};
use crate::gpu::Tex;
use crate::post::{ExposurePass, PostFxConsts};
use crate::post_bloom::{self, BloomSettings};
use crate::post_exposure::{ExposureBase, ExposureGrids, ExposureState, ExposureVolume};
use crate::post_pass::{CodeBuffers, CodePass, CodeTech};
use crate::scene::Scene;

/// The final image's format (the exe's back buffer).
pub const FINAL_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;
const LUT_SIZE: u32 = 32;
const HISTORY: (u32, u32) = (16, 8);

const TECHS: &[&str] = &[
    "bloom_downsample_l0",
    "bloom_downsample_l1",
    // (SSR on: in `_l1`'s place, also writing the quarter-res scene the
    // next frame's SSR permutations read, exe `141d61d40`.)
    "bloom_downsample_l1_scene",
    "bloom_downsample_lx",
    "bloom_downsample_ev",
    "bloom_blur_mip_x_l5",
    "bloom_blur_mip_x_l4",
    "bloom_blur_mip_x_l3",
    "bloom_blur_mip_x_l2",
    "bloom_blur_mip_x_l1",
    "bloom_blur_mip_y_l5",
    "bloom_blur_mip_y_l4",
    "bloom_blur_mip_y_l3",
    "bloom_blur_mip_y_l2",
    "bloom_blur_mip_y_l1",
    "bloom_apply_lut",
    "resample_final_bm_null",
];

/// Render target ids beyond the bloom levels.
const RT_APPLY: u16 = 0x48;
const RT_LUMA: u16 = 0x4a;
const RT_LUM: u16 = 0x23;
const RT_SCENE: u16 = 0xffff;
const RT_LUT: u16 = 0xfffe;
const RT_FINAL: u16 = 0xfffd;

/// One draw: technique, targets, code textures 0x1a.. by render target id.
struct Draw {
    tech: &'static str,
    targets: Vec<u16>,
    textures: Vec<(u32, u16)>,
    consts: usize,
}

struct Sized {
    width: u32,
    height: u32,
    rts: HashMap<u16, Tex>,
    lut3d: Tex,
    history: Tex,
    draws: Vec<Draw>,
    /// Per draw: its bind groups (rebuilt when the scene view changes).
    groups: Vec<[wgpu::BindGroup; 3]>,
    per_scene: HashMap<(u32, u32), (wgpu::Buffer, wgpu::Buffer)>,
    lut_groups: Vec<wgpu::BindGroup>,
}

pub struct Post {
    gpu: Gpu,
    passes: HashMap<&'static str, CodePass>,
    lut: Program,
    exposure: ExposurePass,
    buffers: CodeBuffers,
    samplers: HashMap<u16, wgpu::Sampler>,
    lut_image: Tex,
    lut_height: u32,
    /// The four post-FX constant buffers in the order the exe changes them:
    /// LUT, bloom passes, exposure, final resample.
    cbs: [wgpu::Buffer; 4],
    bloom: BloomSettings,
    vision: Vec<f32>,
    lut_world: (fastfile_t7::walk::world::Span, fastfile_t7::walk::world::Span),
    vision_world: (usize, fastfile_t7::walk::world::Span, [fastfile_t7::walk::world::Span; 2]),
    /// The bloom lists (`GfxWorld` 0x1b40/0x1b50/0x1b60), blended per view.
    bloom_world: (usize, fastfile_t7::walk::world::Span, [fastfile_t7::walk::world::Span; 2]),
    /// The records the last view's bloom took (for the log on change).
    bloom_used: Vec<(usize, f32)>,
    /// The vukko lists (`GfxWorld` 0x1610/0x1620/0x1630) for `skyMx`/`sunMx`.
    vukko_world: (usize, fastfile_t7::walk::world::Span, [fastfile_t7::walk::world::Span; 2]),
    /// The records the lists above are into, copied out of the map zone
    /// ([`frame_records`]).
    kept: std::sync::Arc<fastfile_t7::kept::KeptBytes>,
    exposure_base: ExposureBase,
    exposure_volumes: Vec<ExposureVolume>,
    exposure_planes: Vec<[f32; 4]>,
    exposure_state: ExposureState,
    exposure_grids: ExposureGrids,
    hdr_format: wgpu::TextureFormat,
    sized: Option<Sized>,
    scene_key: Option<wgpu::TextureView>,
    /// bo2mp: with SSR, the lighting stage's `gDownsampledScene`, which
    /// `bloom_downsample_l1_scene` writes in `_l1`'s place.
    ssr_scene: Option<wgpu::TextureView>,
    frame: u32,
    /// The last view's CPU exposure (metered luminance, EV, scale).
    pub last_exposure: (f32, f32, f32),
    /// This view's CPU exposure, worked out by [`Post::begin_view`].
    view_exposure: Option<(crate::post_exposure::ViewExposure, bool)>,
    pub report: Vec<String>,
    /// bo2mp: Black Ops III's out-of-bounds filter (`crate::post_oob`),
    /// drawn while `post_oob::active()`.
    oob: Option<crate::post_oob::OobFilter>,
    /// bo2mp: the exe's AA after `bloom_apply_lut` (`crate::post_aa`).
    smaa: Option<crate::post_aa::Smaa>,
    /// bo2mp: the exe's camera motion blur before the bloom
    /// (`crate::post_mblur`), and this frame's blur velocity (None: the
    /// view does not blur).
    mblur: Option<crate::post_mblur::MotionBlur>,
    mblur_velocity: Option<wgpu::TextureView>,
    /// bo2mp: the exe's ADS blur after the bloom's `_l1` (`crate::post_ads`),
    /// this frame's ADS view and the frame's depth (None: not aiming),
    /// and `bloom_apply_lut`'s bind groups reading the blurred scene.
    ads: Option<crate::post_ads::AdsBlur>,
    ads_view: Option<(crate::post_ads::AdsView, wgpu::TextureView)>,
    ads_apply: Option<(wgpu::TextureView, [wgpu::BindGroup; 3])>,
    ads_logged: bool,
}

/// The target formats of a post chain code pass.
pub(crate) fn pass_formats(n: &str, hdr_format: wgpu::TextureFormat) -> Vec<wgpu::TextureFormat> {
    match n {
        "bloom_downsample_ev" => vec![wgpu::TextureFormat::R32Float],
        // (With AA, a storage-capable 0x48: the AA passes write it.)
        "bloom_apply_lut" => vec![crate::post_aa::apply_format(hdr_format), wgpu::TextureFormat::R8Unorm],
        "resample_final_bm_null" => vec![FINAL_FORMAT],
        n if n.starts_with("bloom_blur_mip_x") || n == "bloom_downsample_l1" || n == "bloom_downsample_lx" => vec![wgpu::TextureFormat::Rgba16Float],
        // (Targets 0 and 2; 1 is not written: `gDownsampledScene`, RGBA16F.)
        "bloom_downsample_l1_scene" => vec![wgpu::TextureFormat::Rgba16Float; 3],
        _ => vec![hdr_format],
    }
}

/// The post chain's code passes, in the order the chain makes them.
pub(crate) const fn techs() -> &'static [&'static str] {
    TECHS
}

impl Post {
    pub fn new(gpu: &Gpu, scene: &Scene, zone_dir: &Path) -> Result<Self, String> {
        let mut report = Vec::new();
        let patch = fastfile_t7::shared::zone(zone_dir, "core_patch").ok_or("core_patch.ff not read")?;
        let techs: HashMap<String, CodeTech> = crate::post_pass::code_techs(&patch.image, &patch.capture, TECHS)?;
        // colorMatrixR/G/B: left unset because nothing reads them (the grade
        // is the LUT); counted here so a zone that did would show.
        {
            let mut n = crate::code::readers(&patch.capture, &crate::code::COLOR_MATRIX);
            if let Some((_, cap)) = scene.map.zone_data() {
                n += crate::code::readers(&cap, &crate::code::COLOR_MATRIX);
            }
            if let Some(c) = fastfile_t7::shared::zone(zone_dir, "core_common") {
                n += crate::code::readers(&c.capture, &crate::code::COLOR_MATRIX);
            }
            report.push(format!("t7 colorMatrix: {n} technique arguments in core_patch, core_common and the map read colorMatrixR/G/B; {}", if n == 0 { "left unset (the exe grades through the LUT)" } else { "UNSET but read: their writer is not found" }));
        }
        let shaders: HashMap<String, CodeShader> = crate::compute::code_shaders(&patch.image, &patch.capture);
        let rg11 = gpu.device.features().contains(wgpu::Features::RG11B10UFLOAT_RENDERABLE);
        let hdr_format = if rg11 { wgpu::TextureFormat::Rg11b10Ufloat } else { wgpu::TextureFormat::Rgba16Float };
        if !rg11 {
            report.push("post: R11G11B10 targets are RGBA16F here (the device cannot render to R11G11B10)".into());
        }
        // (Side by side; the load may have made them already.)
        let built = crate::par::map(TECHS, |&n| CodePass::new(gpu, &techs[n], &pass_formats(n, hdr_format), &[0x1d]));
        let mut passes = HashMap::new();
        for (&n, r) in TECHS.iter().zip(built) {
            passes.insert(n, r?);
        }
        let lut = Program::new(gpu, shaders.get("create_lut2dv_compute").ok_or("create_lut2dv_compute not in core_patch")?, &[])?;
        let exposure_cs = shaders.get("exposure_compute").ok_or("exposure_compute not in core_patch")?;
        let exposure = ExposurePass::new(gpu, &exposure_cs.program)?;
        let mut samplers = HashMap::new();
        let border = gpu.device.features().contains(wgpu::Features::ADDRESS_MODE_CLAMP_TO_BORDER);
        for p in passes.values() {
            for s in p.sampler_states() {
                if let std::collections::hash_map::Entry::Vacant(e) = samplers.entry(s) {
                    e.insert(gpu.device.create_sampler(&crate::sampler::descriptor(s, crate::sampler::ANISO_QUALITY, border)?));
                }
            }
        }
        // The map's world settings.
        let map = &scene.map;
        let (zone_image, cap) = map.zone_data().ok_or("the map's zone data is not kept")?;
        let world = cap.world.gfx_worlds.first().ok_or("no GfxWorld")?;
        let bloom = post_bloom::base_settings(&zone_image, world.header_at, 0).ok_or("bloom settings past the zone")?;
        // The grade LUT image: the world's LUT material (header +0x530),
        // else the code material's (`identity_lut_win`).
        // (A texture of that material that is no image in the zones, as
        // mp_western's (Outlaw) `create_lut2dv#da7372ff`, is passed over
        // for the code material's: failing here left the post chain unmade
        // and the raw HDR scene on screen, which is white.)
        let wanted = world
            .header_materials
            .get(2)
            .cloned()
            .flatten()
            .and_then(|m| cap.material(m.trim_start_matches(',')).and_then(|mm| mm.textures.first().map(|(_, n)| n.trim_start_matches(',').to_owned())));
        let (lut_name, im) = match wanted.as_deref().map(|n| (n, map.image(n, 4096))) {
            Some((n, Ok(im))) => (n.to_owned(), im),
            other => {
                if let Some((n, Err(e))) = other {
                    report.push(format!("post: the world's grade LUT {n} is no image ({e}): identity_lut_win taken, as for a map without one"));
                }
                let n = "identity_lut_win".to_owned();
                let im = map.image(&n, 4096).map_err(|e| format!("LUT image {n}: {e}"))?;
                (n, im)
            }
        };
        let lut_height = im.height;
        let lut_image = crate::gpu::upload(gpu, &lut_name, &im)?;
        report.push(format!("post: grade LUT image {lut_name} ({}x{}, DXGI {})", im.width, im.height, im.format));
        // The client's naked vision: `default` (vision/default.vision).
        let vision = ["core_common", "mp_common", "core_patch"]
            .iter()
            .find_map(|z| fastfile_t7::shared::zone(zone_dir, z).and_then(|z| crate::post_lut::vision_words(&z.image, "vision/default.vision")))
            .ok_or("vision/default.vision not found in the shared zones")?;
        if std::env::var("T7_POST_DEBUG").is_ok() {
            report.push(format!("post debug: vision {vision:?}"));
            for (i, c) in crate::post_lut::controls(&vision).iter().enumerate() {
                report.push(format!("post debug: control{i} {c:?}"));
            }
        }
        // Exposure: the first sun volume's state-0 settings and the
        // world's exposure volumes.
        // (The exe reads them at the sun volume + 0xf4·state + 0xb0.)
        let sun = *world.sun_volumes.first().ok_or("no sun volume")?;
        let exposure_base = zone_image.get(sun..sun + 0xbc).and_then(ExposureBase::from_settings).ok_or("exposure settings")?;
        report.push(format!("post: exposure base {exposure_base:?}"));
        let exposure_volumes: Vec<ExposureVolume> = world.exposure_volumes.map_or(Vec::new(), |(at, n)| (0..n).filter_map(|i| zone_image.get(at + 0x6c * i..at + 0x6c * (i + 1)).and_then(ExposureVolume::parse)).collect());
        report.push(format!("post: {} LUT volumes", world.lut_volumes.map_or(0, |v| v.1)));
        let lighting = world.sun_lighting.first();
        let exposure_grids = ExposureGrids::new(&zone_image, world.sun_volumes[0], 0, lighting.and_then(|l| l.exposure_grids.first().copied().flatten()), lighting.and_then(|l| l.exposure_grid_parts.first()).map_or(&[][..], Vec::as_slice));
        report.push(format!("post: exposure grids (state 0): {} probes, {} where none covers the eye", lighting.and_then(|l| l.exposure_grids.first().copied().flatten()).map_or(0, |g| g.1), exposure_grids.default));
        let (kept, lut_world, vision_world, bloom_world, vukko_world) = frame_records(&zone_image, world);
        let cb = |label| crate::audit::buffer(&gpu.device, &wgpu::BufferDescriptor { label: Some(label), size: PostFxConsts::BYTES, usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST, mapped_at_creation: false });
        let oob = match crate::post_oob::OobFilter::new(gpu, zone_dir, crate::post_aa::apply_format(hdr_format)) {
            Ok(f) => {
                report.push("t7 oob filter: generic_filter_out_of_bounds (out_of_bounds#da7372ff, core_common) ready".into());
                Some(f)
            }
            Err(e) => {
                report.push(format!("t7 oob filter: not made ({e}); out of bounds draws no filter"));
                None
            }
        };
        // The AA (exe 0x141d636c0 after the orchestrator; `crate::post_aa`).
        let aa_mode = crate::post_aa::mode();
        let smaa = if crate::post_aa::active() {
            let im = map.image("$smaa_tex", 4096).or_else(|e| {
                patch.capture.images.iter().find(|i| i.name == "$smaa_tex").ok_or(e).and_then(|r| fastfile_t7::texture::zone_levels(r, &patch.image).map_err(|e| format!("$smaa_tex: {e}")))
            });
            match im.and_then(|im| crate::post_aa::Smaa::new(gpu, &shaders, &im)) {
                Ok(s) => {
                    report.push(format!("t7 aa: mode {aa_mode} ({}) asked (exe default 5); runs as {}: smaa_pass1_compute_hq, the morphological work queues, smaa_pass2v/h_compute (indirect) over rt 0x48 before resample_final{}", crate::post_aa::mode_name(aa_mode), crate::post_aa::mode_name(crate::post_aa::effective(aa_mode)), if aa_mode >= 3 { "; the temporal/filmic passes need the projection jitter, not drawn (the exe's own non-temporal fallback)" } else { "" }));
                    Some(s)
                }
                Err(e) => {
                    report.push(format!("t7 aa: SMAA not made ({e}); no AA"));
                    None
                }
            }
        } else {
            report.push(format!("t7 aa: mode {aa_mode} ({}): no AA (BO2MP_T7_AA)", crate::post_aa::mode_name(aa_mode)));
            None
        };
        // Motion blur (exe 0x141d66110's start; `crate::post_mblur`).
        let mb_mode = crate::post_mblur::mode();
        let mblur = if mb_mode == crate::post_mblur::OFF {
            report.push("t7 motion blur: off (BO2MP_T7_MOTION_BLUR=off)".into());
            None
        } else {
            let q = crate::post_mblur::quality();
            match crate::post_pass::code_techs(&patch.image, &patch.capture, &crate::post_mblur::MotionBlur::pass_names()).and_then(|t| crate::post_mblur::MotionBlur::new(gpu, &t, wgpu::TextureFormat::Rgba16Float, q)) {
                Ok(m) => {
                    report.push(format!("t7 motion blur: {} (the exe's default Auto: blurs while the game sets a blur amount, now {}), quality {} ({}); motion_vector_static_with_mb, mblur_tile_downsample_0/1, mblur_tile_neighbor, {} before the bloom", if mb_mode == crate::post_mblur::ON { "On" } else { "Auto" }, crate::post_mblur::amount(), q, ["Low", "Medium", "High"][q], ["mblur_low", "mblur_medium", "mblur_high"][q]));
                    Some(m)
                }
                Err(e) => {
                    report.push(format!("t7 motion blur: not made ({e})"));
                    None
                }
            }
        };
        // The ADS blur (exe 141d61d40 / 141d63100; `crate::post_ads`).
        let ads = if crate::post_ads::enabled() {
            match crate::post_pass::code_techs(&patch.image, &patch.capture, &crate::post_ads::PASSES).and_then(|t| crate::post_ads::AdsBlur::new(gpu, &t, &shaders, wgpu::TextureFormat::Rgba16Float)) {
                Ok(a) => {
                    report.push("t7 ads blur: on (BO2MP_T7_ADS_BLUR=0 off): while aiming, bloom_downsample_l1_adsblur (CoC from downsample_depth_with_uv_4x_compute's downsampledDepth), adsblur_large_x/y, adsblur_small, adsblur_update before the grade; postFxControl0/1 from the exe's physical camera (focal length from the FOV, focus 500, f-stop 108)".into());
                    Some(a)
                }
                Err(e) => {
                    report.push(format!("t7 ads blur: not made ({e})"));
                    None
                }
            }
        } else {
            report.push("t7 ads blur: off (BO2MP_T7_ADS_BLUR=0)".into());
            None
        };
        Ok(Self {
            gpu: gpu.clone(),
            passes,
            lut,
            exposure,
            buffers: CodeBuffers::new(gpu),
            samplers,
            lut_image,
            lut_height,
            cbs: [cb("t7 postfx lut"), cb("t7 postfx bloom"), cb("t7 postfx exposure"), cb("t7 postfx final")],
            bloom,
            vision,
            lut_world,
            vision_world,
            bloom_world,
            bloom_used: Vec::new(),
            vukko_world,
            kept: std::sync::Arc::new(kept),
            exposure_base,
            exposure_volumes,
            exposure_planes: world.exposure_planes.map_or(Vec::new(), |(at, n)| (0..n).map(|i| std::array::from_fn(|k| zone_image.get(at + 16 * i + 4 * k..at + 16 * i + 4 * k + 4).map_or(0.0, |b| f32::from_le_bytes(b.try_into().unwrap())))).collect()),
            exposure_state: ExposureState::default(),
            exposure_grids,
            hdr_format,
            sized: None,
            scene_key: None,
            ssr_scene: None,
            frame: 0,
            last_exposure: (0.0, 0.0, 0.0),
            view_exposure: None,
            report,
            oob,
            smaa,
            mblur,
            mblur_velocity: None,
            ads,
            ads_view: None,
            ads_apply: None,
            ads_logged: false,
        })
    }

    fn rt<'a>(&self, s: &'a Sized, id: u16) -> Option<&'a wgpu::TextureView> {
        match id {
            RT_LUT => Some(&s.lut3d.view),
            _ => s.rts.get(&id).map(|t| &t.view),
        }
    }

    fn make_sized(&self, w: u32, h: u32) -> Sized {
        let gpu = &self.gpu;
        let mut rts = HashMap::new();
        for l in post_bloom::levels(w, h) {
            let format = if l.format == wgpu::TextureFormat::Rg11b10Ufloat { self.hdr_format } else { l.format };
            let extra = wgpu::TextureUsages::COPY_SRC | if l.id == RT_LUM { wgpu::TextureUsages::STORAGE_BINDING } else { wgpu::TextureUsages::empty() };
            rts.insert(l.id, crate::gpu::target(gpu, &format!("t7 rt {:#x}", l.id), l.width, l.height, format, extra));
        }
        let aa = if self.smaa.is_some() { wgpu::TextureUsages::STORAGE_BINDING } else { wgpu::TextureUsages::empty() };
        rts.insert(RT_APPLY, crate::gpu::target(gpu, "t7 rt 0x48", w, h, crate::post_aa::apply_format(self.hdr_format), wgpu::TextureUsages::COPY_SRC | aa));
        rts.insert(RT_LUMA, crate::gpu::target(gpu, "t7 rt 0x4a", w, h, wgpu::TextureFormat::R8Unorm, wgpu::TextureUsages::empty()));
        rts.insert(RT_FINAL, crate::gpu::target(gpu, "t7 final", w, h, FINAL_FORMAT, wgpu::TextureUsages::COPY_SRC));
        let lut3d_t = crate::audit::texture(&gpu, &wgpu::TextureDescriptor {
            label: Some("t7 lut3D"),
            size: wgpu::Extent3d { width: LUT_SIZE, height: LUT_SIZE, depth_or_array_layers: LUT_SIZE },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D3,
            format: wgpu::TextureFormat::Rgba16Float,
            usage: wgpu::TextureUsages::STORAGE_BINDING | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let lut3d = Tex { view: lut3d_t.create_view(&Default::default()), texture: lut3d_t };
        let hist_t = crate::audit::texture(&gpu, &wgpu::TextureDescriptor {
            label: Some("t7 exposure history"),
            size: wgpu::Extent3d { width: HISTORY.0, height: HISTORY.1, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::R32Float,
            usage: wgpu::TextureUsages::STORAGE_BINDING,
            view_formats: &[],
        });
        let history = Tex { view: hist_t.create_view(&Default::default()), texture: hist_t };
        // The draws (the extra `_lx` steps above 1080 and 2160 rows move
        // the blurs one level up: inferred from `bloom_apply_lut` reading
        // 0x2f / 0x30 there).
        let shift: u16 = if h > 2160 { 2 } else if h > 1080 { 1 } else { 0 };
        let mut draws = Vec::new();
        let d = |tech: &'static str, targets: Vec<u16>, textures: Vec<(u32, u16)>, consts: usize| Draw { tech, targets, textures, consts };
        draws.push(d("bloom_downsample_l0", vec![0x2d], vec![(0x1a, RT_SCENE)], 1));
        draws.push(d("bloom_downsample_l1", vec![0x35], vec![(0x1a, 0x2d)], 1));
        for s in 0x35..0x39 + shift {
            draws.push(d("bloom_downsample_lx", vec![s + 1], vec![(0x1a, s)], 1));
        }
        draws.push(d("bloom_downsample_ev", vec![RT_LUM], vec![(0x1a, 0x39)], 1));
        for (k, lvl) in ["l5", "l4", "l3", "l2", "l1"].iter().enumerate() {
            let src = 0x39 + shift - k as u16;
            let tech: &'static str = Box::leak(format!("bloom_blur_mip_x_{lvl}").into_boxed_str());
            draws.push(d(tech, vec![src + 7], vec![(0x1a, src)], 1));
        }
        for (k, lvl) in ["l5", "l4", "l3", "l2", "l1"].iter().enumerate() {
            let x = 0x40 + shift - k as u16;
            let dst = 0x32 + shift - k as u16;
            let tech: &'static str = Box::leak(format!("bloom_blur_mip_y_{lvl}").into_boxed_str());
            let tex = if k == 0 { vec![(0x1a, x)] } else { vec![(0x1a, x), (0x1b, dst + 1)] };
            draws.push(d(tech, vec![dst], tex, 1));
        }
        draws.push(d("bloom_apply_lut", vec![RT_APPLY, RT_LUMA], vec![(0x1a, 0x2e + shift), (0x1b, RT_LUT), (0x1c, RT_SCENE), (0x1d, RT_LUM), (0x1e, 0x2d)], 1));
        draws.push(d("resample_final_bm_null", vec![RT_FINAL], vec![(0x1a, RT_APPLY)], 3));
        // (Last, outside the chain's order: drawn in `_l1`'s place with SSR.)
        draws.push(d("bloom_downsample_l1_scene", vec![0x35, 0x35, 0x35], vec![(0x1a, 0x2d)], 1));
        Sized { width: w, height: h, rts, lut3d, history, draws, groups: Vec::new(), per_scene: HashMap::new(), lut_groups: Vec::new() }
    }

    fn bind(&self, s: &mut Sized, scene: &wgpu::TextureView) -> Result<(), String> {
        use wgpu::util::DeviceExt;
        let mut groups = Vec::new();
        let mut per_scene = std::mem::take(&mut s.per_scene);
        for dr in &s.draws {
            let p = &self.passes[dr.tech];
            let t0 = s.rts.get(&dr.targets[0]).map(|t| t.texture.size()).ok_or_else(|| format!("{}: target {:#x} not made", dr.tech, dr.targets[0]))?;
            let key = (t0.width, t0.height);
            let (ps, _) = per_scene.entry(key).or_insert_with(|| {
                let dev = &self.gpu.device;
                let ps = dev.create_buffer_init(&wgpu::util::BufferInitDescriptor { label: Some("t7 code PerSceneConsts"), contents: bytemuck::cast_slice(&crate::post_pass::per_scene(key.0, key.1)), usage: wgpu::BufferUsages::UNIFORM });
                let vb = dev.create_buffer_init(&wgpu::util::BufferInitDescriptor { label: Some("t7 code triangle"), contents: &crate::post_pass::triangle(key.0, key.1), usage: wgpu::BufferUsages::VERTEX });
                (ps, vb)
            });
            let tex = |id: u32| -> Option<&wgpu::TextureView> {
                let rt = dr.textures.iter().find(|t| t.0 == id)?.1;
                if rt == RT_SCENE { Some(scene) } else { self.rt(s, rt) }
            };
            groups.push(p.bind(&self.gpu, ps, &self.cbs[dr.consts], &self.buffers, tex, |st| self.samplers.get(&st))?);
        }
        s.lut_groups = self.lut.bind(&self.gpu, |slot| match (slot.group, slot.binding) {
            (0, _) => Some(Res::Buffer(&self.cbs[0])),
            (1, 160) => Some(Res::View(&s.lut3d.view)),
            (1, _) => Some(Res::View(&self.lut_image.view)),
            _ => None,
        })?;
        s.per_scene = per_scene;
        s.groups = groups;
        Ok(())
    }

    /// The constants for this frame (the camera for the colour-vision
    /// blend and exposure volumes).
    fn update(&mut self, camera: &Camera, w: u32, h: u32) {
        let kept = self.kept.clone();
        let img = kept.bytes();
        let eye = [camera.eye.x, camera.eye.y, camera.eye.z];
        // LUT.
        let mut c = PostFxConsts::default();
        for (i, v) in crate::post_lut::controls(&self.vision).iter().enumerate() {
            c.control(i, *v);
        }
        // (The LUT volume mask is taken as the lighting state's bit, as the
        // colour-vision and exposure volumes' are.)
        let vol = crate::post_lut::lut_volume(&img, self.lut_world.0, self.lut_world.1, eye, 1 << 4);
        let (slice, [w0, w1]) = crate::post_lut::settled_slice(&img, self.lut_world.0, vol);
        let [sl, wt] = crate::post_lut::slices(slice, slice, [w0, w1, 0.0, 0.0], self.lut_height);
        {
            // (Logged when it changes: which grade volume and LUT band the
            // eye is in, for a colour cast to be traced to the grade.)
            use std::sync::atomic::{AtomicI64, Ordering};
            static LAST: AtomicI64 = AtomicI64::new(-2);
            let key = vol.map_or(-1, |v| v.0 as i64 * 64 + i64::from(slice));
            if LAST.swap(key, Ordering::Relaxed) != key {
                self.report.push(format!("t7 grade: eye in LUT volume {:?}, LUT band {} of {} ({} rows), weights {w0:.2}/{w1:.2}", vol.map(|v| (v.0, v.1)), sl[1], self.lut_height >> 5, self.lut_height));
            }
        }
        c.control(14, sl);
        c.control(15, wt);
        let f = camera.rotation.row(2);
        let (hdr, mods, boxes) = self.vision_world;
        let slots = fastfile_t7::native::vision_slots(&img, hdr, 0, 0, mods, boxes, eye, [f.x, f.y, f.z]);
        let blocks = crate::post_lut::vision_blocks(&img, &slots);
        for (i, v) in blocks.iter().enumerate() {
            c.constant(i, *v);
        }
        // The view's bloom settings (the exe's view setup blends the map's
        // bloom modifiers, boxes and base record at the eye).
        let (bh, bm, bb) = self.bloom_world;
        let (bloom, used) = post_bloom::view_settings(&img, bh, 0, 0, bm, bb, eye, [f.x, f.y, f.z]);
        self.bloom = bloom;
        let records: Vec<usize> = used.iter().map(|u| u.0).collect();
        if records != self.bloom_used.iter().map(|u| u.0).collect::<Vec<_>>() {
            let names: Vec<String> = used.iter().map(|&(at, wt)| format!("{} {wt:.3}", if at == bh + 0x1640 { "base (state 0)".to_owned() } else { format!("record at {at:#x}") })).collect();
            self.report.push(format!("t7 bloom: view settings from {} (map: {} modifiers, {} + {} boxes); postFxBloom00 {:?}", names.join(", "), bm.map_or(0, |m| m.1), bb[0].map_or(0, |b| b.1), bb[1].map_or(0, |b| b.1), post_bloom::bloom_constants(&self.bloom, w, h)[0]));
            self.bloom_used = used;
        }
        for (i, v) in post_bloom::bloom_constants(&self.bloom, w, h).iter().enumerate() {
            c.bloom(i, *v);
        }
        let q = self.gpu.queue.clone();
        q.write_buffer_p(&self.cbs[0], 0, &c.bytes());
        q.write_buffer_p(&self.cbs[1], 0, &c.bytes());
        // The ADS blur's postFxControl0/1 (exe 141d60f10) over the bloom
        // constants (its `_l1` reads both).
        if let (Some(a), Some((v, _))) = (self.ads.as_ref(), self.ads_view.as_ref()) {
            let mut ca = c.clone();
            let [c0, c1] = crate::post_ads::controls(v, camera.near, h);
            ca.control(0, c0);
            ca.control(1, c1);
            q.write_buffer_p(&a.fx, 0, &ca.bytes());
        }
        // Exposure.
        let (e, adaptive) = match self.view_exposure.take() {
            Some(v) => v,
            None => {
                self.begin_view(camera);
                self.view_exposure.take().unwrap()
            }
        };
        let mut ce = c.clone();
        ce.set_exposure(&e, 1.0, adaptive, 0);
        q.write_buffer_p(&self.cbs[2], 0, &ce.bytes());
        // Final resample (exe 0x141d66110's tail): sRGB encoding.
        let mut cf = ce;
        let n = noise(self.frame);
        cf.control(0, [w as f32 * 0.001_953_125, h as f32 * 0.001_953_125, n.0, n.1]);
        let brightness = 0.0f32 * 0.062_745_1;
        cf.control(1, [0.0, brightness, brightness - 0.055, 0.003_130_8]);
        cf.control(2, [12.92, 0.416_666_66, 1.055, -0.055]);
        q.write_buffer_p(&self.cbs[3], 0, &cf.bytes());
    }

    /// The view's eye exposure, worked out on the CPU before the scene is
    /// lit (exe 0x141dd6d10 on the client, then the view setup 0x141dddc20:
    /// S+0x1d44 the EV, S+0x1d48 = 1 / 2^(EV-3)). Returns the view's
    /// `relHDRExposure` code constant `(2^(EV-3), 2^-(EV-3), 0, 0)`, which
    /// the lighting programs scale the lit scene by (its `.y`) — the post
    /// chain expects the scene so pre-divided. Call it before lighting the
    /// frame; [`Post::run`] calls it itself if not.
    pub fn begin_view(&mut self, camera: &Camera) -> [f32; 4] {
        let eye = [camera.eye.x, camera.eye.y, camera.eye.z];
        let f = camera.rotation.row(2);
        let mask = 1u8 << 4;
        let planes: Vec<[f32; 4]> = self.exposure_planes.clone();
        let inside = crate::post_exposure::volume_weights(eye, &self.exposure_volumes, &planes, mask);
        let metered = self.exposure_grids.meter(eye, [f.x, f.y, f.z]);
        // (The first view is a cut: the history snaps to it.)
        let adaptive = self.frame > 0;
        let e = self.exposure_state.update(self.exposure_base, &self.exposure_volumes, &inside, 0, metered, adaptive);
        self.last_exposure = (metered, e.ev, e.scale);
        let s = 2f32.powf(e.ev - 3.0);
        self.view_exposure = Some((e, adaptive));
        [s, 1.0 / s, 0.0, 0.0]
    }

    /// `skyMxR/G/B`, `sunMxR/G/B` (code constants 0x2d..0x32) of the view,
    /// from the map's vukko records blended at the camera as the exe's view
    /// setup does (`141dd8b90`; `fastfile_t7::native::sky_matrices`), with
    /// the records used and their weights. `sky_rotation_rad`: the angle
    /// `skyRotationTransition` is made from.
    pub fn sky_matrices(&self, camera: &Camera, sky_rotation_rad: f32) -> ([[f32; 4]; 6], Vec<(usize, f32)>) {
        let (hdr, mods, boxes) = self.vukko_world;
        let eye = [camera.eye.x, camera.eye.y, camera.eye.z];
        let f = camera.rotation.row(2);
        fastfile_t7::native::sky_matrices(self.kept.bytes(), hdr, 0, 0, mods, boxes, eye, [f.x, f.y, f.z], sky_rotation_rad)
    }

    /// The `hdrControl0/1` code constants of the view (exe 0x141dddc20):
    /// `(2^-(EV-3), 0, 2^(EV-3), 2^(EV-3))`, `(1, 0, 0, 0)`.
    pub fn hdr_controls(rel_hdr_exposure: [f32; 4]) -> [[f32; 4]; 2] {
        let (s, inv) = (rel_hdr_exposure[0], rel_hdr_exposure[1]);
        [[inv, 0.0, s, s], [1.0, 0.0, 0.0, 0.0]]
    }

    /// Records the chain for one frame: `scene` the lit scene (the frame's
    /// size `width` × `height`). Returns the final image.
    pub fn run(&mut self, enc: &mut wgpu::CommandEncoder, scene: &wgpu::TextureView, width: u32, height: u32, camera: &Camera) -> Result<&wgpu::TextureView, String> {
        // Motion blur first (exe 0x141d66110): the chain then reads the
        // blurred scene.
        let blurred = match (self.mblur.as_mut(), self.mblur_velocity.as_ref()) {
            (Some(m), Some(v)) => match m.record(enc, scene, v, width, height) {
                Ok(b) => Some(b.clone()),
                Err(e) => {
                    self.report.push(format!("t7 motion blur: not run ({e}); off"));
                    self.mblur = None;
                    None
                }
            },
            _ => None,
        };
        if blurred.is_some() {
            crate::timing::mark(enc, "motion blur");
        }
        let scene = blurred.as_ref().unwrap_or(scene);
        if self.sized.as_ref().is_none_or(|s| s.width != width || s.height != height) {
            self.sized = Some(self.make_sized(width, height));
            self.scene_key = None;
        }
        if self.scene_key.as_ref() != Some(scene) {
            let mut s = self.sized.take().unwrap();
            let r = self.bind(&mut s, scene);
            self.sized = Some(s);
            r?;
            self.scene_key = Some(scene.clone());
        }
        // The ADS blur's targets and bindings (it reads the l0 level and the
        // scene the chain reads), then `bloom_apply_lut` reading its output.
        let mut ads_on = false;
        if let (Some(a), Some((v, depth)), Some(s)) = (self.ads.as_mut(), self.ads_view.as_ref(), self.sized.as_ref())
            && v.active()
        {
            let q = s.rts[&0x35].texture.size();
            let inputs = crate::post_ads::Inputs { scene, l0: &s.rts[&0x2d].view, depth, quarter: (q.width, q.height) };
            match a.prepare(width, height, &inputs) {
                Ok(()) => ads_on = true,
                Err(e) => {
                    self.report.push(format!("t7 ads blur: not run ({e}); off"));
                    self.ads = None;
                }
            }
        }
        if ads_on {
            let out = self.ads.as_ref().and_then(|a| a.output()).cloned();
            if let (Some(out), Some(s)) = (out, self.sized.as_ref())
                && self.ads_apply.as_ref().is_none_or(|(v, _)| *v != out)
            {
                let k = s.draws.iter().position(|d| d.tech == "bloom_apply_lut").unwrap();
                let dr = &s.draws[k];
                let size = s.rts[&dr.targets[0]].texture.size();
                let ps = &s.per_scene[&(size.width, size.height)].0;
                let tex = |id: u32| -> Option<&wgpu::TextureView> {
                    let rt = dr.textures.iter().find(|t| t.0 == id)?.1;
                    if rt == RT_SCENE { Some(&out) } else { self.rt(s, rt) }
                };
                match self.passes[dr.tech].bind(&self.gpu, ps, &self.cbs[dr.consts], &self.buffers, tex, |st| self.samplers.get(&st)) {
                    Ok(g) => self.ads_apply = Some((out.clone(), g)),
                    Err(e) => {
                        self.report.push(format!("t7 ads blur: apply not bound ({e}); off"));
                        self.ads = None;
                        ads_on = false;
                    }
                }
            }
            if !self.ads_logged {
                self.ads_logged = true;
                if let Some((v, _)) = self.ads_view.as_ref() {
                    let [c0, c1] = crate::post_ads::controls(v, camera.near, height);
                    self.report.push(format!("t7 ads blur: first aimed frame: amount {:.2}, focal length {:.1} mm, focus {}, f-stop {}; postFxControl0 {c0:?}, postFxControl1 {c1:?}", v.amount, v.focal_mm, v.focus, v.fstop));
                }
            }
        }
        self.update(camera, width, height);
        self.frame = self.frame.wrapping_add(1);
        let mut smaa = self.smaa.take();
        let mut oob = self.oob.take().filter(|_| crate::post_oob::active());
        let oob_idle = if oob.is_none() { self.oob.take() } else { None };
        let mut oob_error = None;
        let s = self.sized.as_ref().unwrap();
        // l0, l1 (before the orchestrator), then the LUT.
        let mut i = 0;
        let draw = |enc: &mut wgpu::CommandEncoder, i: usize| {
            let dr = &s.draws[i];
            let targets: Vec<&wgpu::TextureView> = dr.targets.iter().map(|t| &s.rts[t].view).collect();
            let size = s.rts[&dr.targets[0]].texture.size();
            let vb = &s.per_scene[&(size.width, size.height)].1;
            self.passes[dr.tech].draw(enc, &targets, &s.groups[i], vb);
        };
        draw(enc, 0);
        let ads = self.ads.as_ref().filter(|_| ads_on);
        match &self.ssr_scene {
            // (Aiming: `_l1` as `bloom_downsample_l1_adsblur`, then the
            // exe's adsblur passes, 141d63100.)
            _ if ads.is_some() => {
                let a = ads.unwrap();
                a.record_l1(enc, camera.near, &s.rts[&0x35].view, self.ssr_scene.as_ref());
                a.record_blur(enc);
                crate::timing::mark(enc, "ads blur");
            }
            Some(down) => {
                let k = s.draws.len() - 1;
                let dr = &s.draws[k];
                let t0 = &s.rts[&0x35];
                let size = t0.texture.size();
                let vb = &s.per_scene[&(size.width, size.height)].1;
                self.passes[dr.tech].draw(enc, &[&t0.view, &t0.view, down], &s.groups[k], vb);
            }
            None => draw(enc, 1),
        }
        {
            let mut pass = enc.compute_pass_p(&wgpu::ComputePassDescriptor { label: Some("t7 create_lut2dv_compute"), timestamp_writes: None });
            pass.set_pipeline_p(&self.lut.pipeline);
            for (k, g) in s.lut_groups.iter().enumerate() {
                pass.set_bind_group_p(k as u32, g, &[]);
            }
            let [x, y, z] = self.lut.groups_for([LUT_SIZE; 3]);
            pass.dispatch_p(x, y, z);
        }
        i += 2;
        crate::timing::mark(enc, "post lut");
        let apply = s.draws.iter().position(|d| d.tech == "bloom_apply_lut").unwrap();
        // (Bloom off, the shared `bloom` row: the blurs are skipped and the
        // level `bloom_apply_lut` adds is cleared to black; the downsamples
        // the exposure meters stay.)
        let bloom = crate::quality::get().bloom;
        while i < apply {
            if bloom || !s.draws[i].tech.starts_with("bloom_blur_mip") {
                draw(enc, i);
            }
            i += 1;
        }
        if !bloom && let Some(t) = s.draws[apply].textures.iter().find(|t| t.0 == 0x1a).and_then(|t| s.rts.get(&t.1)) {
            enc.render_pass_p(&wgpu::RenderPassDescriptor {
                label: Some("t7 bloom off: clear the bloom level"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment { view: &t.view, depth_slice: None, resolve_target: None, ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT), store: wgpu::StoreOp::Store } })],
                ..Default::default()
            });
        }
        crate::timing::mark(enc, "bloom");
        self.exposure.record_with(&self.gpu, enc, &self.cbs[2], &s.rts[&RT_LUM].view, &s.history.view);
        crate::timing::mark(enc, "exposure");
        match self.ads_apply.as_ref().filter(|_| ads.is_some()) {
            Some((_, g)) => {
                let dr = &s.draws[apply];
                let targets: Vec<&wgpu::TextureView> = dr.targets.iter().map(|t| &s.rts[t].view).collect();
                let size = s.rts[&dr.targets[0]].texture.size();
                self.passes[dr.tech].draw(enc, &targets, g, &s.per_scene[&(size.width, size.height)].1);
            }
            None => draw(enc, apply),
        }
        crate::timing::mark(enc, "apply");
        // The AA over the graded image (exe 0x141d636c0, after the
        // orchestrator, before the final resample).
        let mut aa_error = None;
        if let Some(a) = smaa.as_mut()
            && let Err(e) = a.record(enc, &s.rts[&RT_APPLY].view, &s.rts[&RT_LUMA].view, width, height)
        {
            aa_error = Some(e);
        }
        crate::timing::mark(enc, "aa");
        // bo2mp: the out-of-bounds filter over the graded image, before the
        // final resample (`crate::post_oob`).
        if let Some(f) = oob.as_mut()
            && let Err(e) = f.draw(&self.gpu, enc, &s.rts[&RT_APPLY])
        {
            oob_error = Some(e);
        }
        draw(enc, apply + 1);
        crate::timing::mark(enc, "apply+final");
        match aa_error {
            Some(e) => self.report.push(format!("t7 aa: not run ({e}); off")),
            None => self.smaa = smaa,
        }
        if let Some(e) = oob_error {
            self.report.push(format!("t7 oob filter: draw failed ({e}); off"));
        } else {
            self.oob = oob.or(oob_idle);
        }
        let s = self.sized.as_ref().unwrap();
        Ok(&s.rts[&RT_FINAL].view)
    }

    /// bo2mp: this frame's motion blur velocity (the lighting stage's
    /// `blur_velocity`; None: no blur).
    pub fn set_motion_blur(&mut self, velocity: Option<wgpu::TextureView>) {
        self.mblur_velocity = velocity;
    }

    /// bo2mp: this frame's ADS view and the frame's depth (depth aspect),
    /// for the ADS blur (None or amount 0: not aiming).
    pub fn set_ads(&mut self, view: Option<(crate::post_ads::AdsView, wgpu::TextureView)>) {
        self.ads_view = view.filter(|(v, _)| v.active());
    }

    /// bo2mp: the SSR set's `gDownsampledScene` to write this frame (None:
    /// the plain `_l1`).
    pub fn set_ssr_scene(&mut self, view: Option<wgpu::TextureView>) {
        self.ssr_scene = view;
    }

    /// A render target of the chain by the exe's id (offline checks).
    pub fn target(&self, id: u16) -> Option<&wgpu::Texture> {
        self.sized.as_ref()?.rts.get(&id).map(|t| &t.texture)
    }

    /// The final image of the last [`Post::run`].
    pub fn final_image(&self) -> Option<&wgpu::TextureView> {
        self.sized.as_ref().map(|s| &s.rts[&RT_FINAL].view)
    }
}

/// `resample_final`'s dither noise for a frame (exe 0x141d66110: the
/// frame number's bits de-interleaved, hashed).
/// A world's post-effect lists: the header and the modifiers and boxes.
type Lists = (usize, fastfile_t7::walk::world::Span, [fastfile_t7::walk::world::Span; 2]);

/// bo2mp: the records [`Post`] reads each view (the world header, the grade
/// volumes and their planes, the colour-vision, bloom and vukko modifiers
/// and boxes, at the sizes their readers step by), copied out of the map
/// zone so the zone can be let go (`T7Map::release_zone`); with the grade
/// lists and the vision, bloom and vukko lists at their offsets in the copy.
pub(crate) fn frame_records(image: &[u8], world: &fastfile_t7::walk::gfxworld::GfxWorldRec) -> (fastfile_t7::kept::KeptBytes, (fastfile_t7::walk::world::Span, fastfile_t7::walk::world::Span), Lists, Lists, Lists) {
    use fastfile_t7::walk::world::Span;
    let records = |s: Span, size: usize| s.map(|(at, n)| (at, size.saturating_mul(n)));
    let lists = |mods: Span, boxes: [Span; 2], m: usize, b: usize| [records(mods, m), records(boxes[0], b), records(boxes[1], b)];
    let mut spans = vec![Some((world.header_at, 0x2040)), records(world.lut_volumes, 0x3c), records(world.lut_planes, 16)];
    spans.extend(lists(world.vision_mods, world.vision_boxes, 0x150, 0x2b0));
    spans.extend(lists(world.bloom_mods, world.bloom_boxes, 0x180, 0x1d0));
    spans.extend(lists(world.vukko_mods, world.vukko_boxes, 0x40, 0x90));
    let kept = fastfile_t7::kept::KeptBytes::new(image, spans.into_iter().flatten());
    let rebased = |mods: Span, boxes: [Span; 2]| (kept.rebase(world.header_at), kept.span(mods), boxes.map(|b| kept.span(b)));
    let lut = (kept.span(world.lut_volumes), kept.span(world.lut_planes));
    let (vision, bloom, vukko) = (rebased(world.vision_mods, world.vision_boxes), rebased(world.bloom_mods, world.bloom_boxes), rebased(world.vukko_mods, world.vukko_boxes));
    (kept, lut, vision, bloom, vukko)
}

fn noise(frame: u32) -> (f32, f32) {
    let compact = |v: u32| {
        let mut v = v & 0x5555_5555;
        v = (v >> 1 ^ v) & 0x3333_3333;
        v = (v >> 2 ^ v) & 0x0f0f_0f0f;
        v = (v >> 4 ^ v) & 0x00ff_00ff;
        v = (v >> 8 ^ v) & 0xffff;
        v >> 13 ^ v
    };
    let h = |u: u32| 1.0 - f64::from(u.wrapping_mul(u).wrapping_mul(0xec4d).wrapping_add(0x0131_071f).wrapping_mul(u).wrapping_add(0xd208_dd0d) & 0x7fff_ffff) * 9.313_225_746_154_785e-10;
    (h(compact(frame)) as f32, h(compact(frame >> 1)) as f32)
}

#[cfg(test)]
mod kept_reads_tests {
    use fastfile_t7::walk::gfxworld::{ExposureGridParts, GfxWorldRec};

    /// A zone with every record the stages read each view, in noise (floats
    /// of -64..64), with the counts and indices set; between the records,
    /// noise the copies must never read.
    fn zone() -> (Vec<u8>, GfxWorldRec, fastfile_t7::fog::FogSources, (Option<(usize, usize)>, Option<(usize, usize)>), Option<(usize, usize)>, Vec<ExposureGridParts>, usize) {
        let mut seed = 0x2545_f491u32;
        let mut img: Vec<u8> = (0..0x14000 / 4)
            .flat_map(|_| {
                seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                ((seed >> 8) as f32 / (1u32 << 24) as f32 * 128.0 - 64.0).to_le_bytes()
            })
            .collect();
        let put = |img: &mut Vec<u8>, at: usize, b: &[u8]| img[at..at + b.len()].copy_from_slice(b);
        let u32s = |img: &mut Vec<u8>, at: usize, v: u32| put(img, at, &v.to_le_bytes());
        let f32s = |img: &mut Vec<u8>, at: usize, v: [f32; 3]| (0..3).for_each(|k| put(img, at + 4 * k, &v[k].to_le_bytes()));
        let h = 0x100;
        let world = GfxWorldRec {
            header_at: h,
            lut_volumes: Some((0x3000, 6)),
            lut_planes: Some((0x3400, 8)),
            vision_mods: Some((0x4000, 4)),
            vision_boxes: [Some((0x5000, 3)), Some((0x6000, 2))],
            bloom_mods: Some((0x7000, 3)),
            bloom_boxes: [Some((0x8000, 2)), Some((0x8800, 2))],
            vukko_mods: Some((0x9000, 4)),
            vukko_boxes: [Some((0x9400, 3)), Some((0x9800, 2))],
            ..Default::default()
        };
        // Grade volumes: all in the state's mask, their planes in and past the list.
        for (i, (first, last)) in [(0u16, 2u8), (3, 5), (6, 9), (0, 7), (5, 4), (1, 1)].into_iter().enumerate() {
            let r = 0x3000 + 0x3c * i;
            img[r + 0xc] = 0xff;
            img[r + 0xd] = last;
            put(&mut img, r + 0xe, &first.to_le_bytes());
        }
        // Fog: volumes naming planes by start and count (the third past the
        // list), modifiers by their word (the third past it too).
        let fog = fastfile_t7::fog::FogSources { world_header: h, fog_volumes: Some((0xa000, 4)), fog_planes: Some((0xb000, 6)), modifiers: Some((0xc000, 3)), modifier_planes: Some((0xc400, 4)) };
        for (i, (start, count)) in [(0, 3), (2, 4), (5, 6), (0, 0)].into_iter().enumerate() {
            u32s(&mut img, 0xa000 + 0x270 * i, start);
            u32s(&mut img, 0xa000 + 0x270 * i + 4, count);
        }
        for (i, (start, count)) in [(0u32, 3u32), (2, 2), (3, 5)].into_iter().enumerate() {
            u32s(&mut img, 0xc000 + 0x30 * i, start << 16 | (count - 1) << 8 | 0xff);
        }
        // Lit fog: two volumes with six planes of their own (one past the list).
        let lit_fog = (Some((0xd000, 4)), Some((0xe000, 8)));
        for (i, first) in [(0, 0u32), (2, 4)] {
            u32s(&mut img, 0xd000 + 0x168 * i, first);
            u32s(&mut img, 0xd000 + 0x168 * i + 4, 6);
        }
        // Both stand round the middle eyes, shown in bank 0 in lighting
        // state 0 with the sun's flag (the third local lights too): the
        // first bounded by its own six planes, the third by its box (its
        // planes run past the list).
        let boxed = [(0usize, [-50f32; 3], [50f32; 3], 0x1021u32), (2, [-30.0, -50.0, -50.0], [70.0, 50.0, 50.0], 0x1061)];
        for (i, lo, hi, flags) in boxed {
            let r = 0xd000 + 0x168 * i;
            f32s(&mut img, r + 8, lo);
            f32s(&mut img, r + 20, hi);
            u32s(&mut img, r + 0x2c, flags);
        }
        let planes = [[0.0, 0.0, -1.0, 50.0], [0.0, 0.0, 1.0, 50.0], [0.0, 1.0, 0.0, 50.0], [1.0, 0.0, 0.0, 50.0], [0.0, -1.0, 0.0, 50.0], [-1.0, 0.0, 0.0, 50.0f32]];
        for (k, p) in planes.into_iter().enumerate() {
            (0..4).for_each(|j| put(&mut img, 0xe000 + 16 * k + 4 * j, &p[j].to_le_bytes()));
        }
        // Exposure: a probe round the origin with three blends (the second
        // cut by two faces, the third cutting the probe out), and one with
        // no voxels along x.
        let grids = Some((0xf000, 2));
        let dims = |img: &mut Vec<u8>, g: usize, d: [u16; 3], nb: u16| {
            (0..3).for_each(|k| put(img, g + 0x28 + 2 * k, &d[k].to_le_bytes()));
            put(img, g + 0x38, &nb.to_le_bytes());
            img[g + 0x48] = 0;
        };
        let g = 0xf000;
        f32s(&mut img, g, [0.0; 3]);
        f32s(&mut img, g + 0xc, [64.0; 3]);
        f32s(&mut img, g + 0x18, [0.0; 3]);
        put(&mut img, g + 0x24, &1f32.to_le_bytes());
        dims(&mut img, g, [2, 3, 2], 3);
        for (j, (faces, flag)) in [(0i32, 0u8), (2, 0), (0, 1)].into_iter().enumerate() {
            let b = 0xf400 + 0x50 * j;
            f32s(&mut img, b, [8.0 * j as f32; 3]);
            f32s(&mut img, b + 0xc, [32.0; 3]);
            f32s(&mut img, b + 0x18, [8.0; 3]);
            f32s(&mut img, b + 0x24, [8.0; 3]);
            f32s(&mut img, b + 0x30, [0.0; 3]);
            put(&mut img, b + 0x3c, &faces.to_le_bytes());
            img[b + 0x48] = flag;
        }
        f32s(&mut img, g + 0x50, [16.0; 3]);
        f32s(&mut img, g + 0x50 + 0xc, [48.0; 3]);
        f32s(&mut img, g + 0x50 + 0x18, [0.0; 3]);
        dims(&mut img, g + 0x50, [0, 4, 4], 0);
        let parts = vec![
            ExposureGridParts { voxels: Some(0x10000), blends: Some(0xf400), faces: vec![None, Some(0xf800), None] },
            ExposureGridParts { voxels: Some(0x10400), blends: None, faces: Vec::new() },
        ];
        (img, world, fog, lit_fog, grids, parts, 0x11000)
    }

    /// The per-view reads over the copies the stages keep give what they
    /// gave over the whole zone, bit for bit (offsets aside).
    #[test]
    fn kept_copies_read_as_the_whole_zone() {
        let (img, world, fog, lit_fog, grids, parts, sun) = zone();
        let (post, lut, vision, bloom, vukko) = super::frame_records(&img, &world);
        let (light, fog_k, lit_fog_k) = crate::lighting::frame_records(&img, Some(fog), lit_fog);
        let fog_k = fog_k.unwrap();
        assert!(post.len() + light.len() < img.len() / 2, "the copies are the records, not the zone");
        let whole = crate::post_exposure::ExposureGrids::over_whole_zone(&img, sun, 0, grids, &parts);
        let kept = crate::post_exposure::ExposureGrids::new(&img, sun, 0, grids, &parts);
        let (p, l) = (post.bytes(), light.bytes());
        let (wv, kv) = (crate::volumetric::Volumes { image: &img, volumes: lit_fog.0.unwrap(), planes: lit_fog.1 }, crate::volumetric::Volumes { image: l, volumes: lit_fog_k.0.unwrap(), planes: lit_fog_k.1 });
        let (wbs, kbs) = (crate::volumetric::BankStates::new(&wv), crate::volumetric::BankStates::new(&kv));
        assert_eq!(wbs.0, kbs.0, "lit fog bank states");
        let (mut wf, mut kf) = (fastfile_t7::fog::FogTransitions::default(), fastfile_t7::fog::FogTransitions::default());
        let weights = |s: &[(usize, f32)]| s.iter().map(|u| u.1.to_bits()).collect::<Vec<_>>();
        let mut now = 0;
        let mut seen = [0usize; 4];
        for x in (-80..=80).step_by(20) {
            for y in (-80..=80).step_by(20) {
                for z in (-80..=80).step_by(20) {
                    let eye = [x as f32 + 0.5, y as f32 - 0.25, z as f32 + 0.125];
                    let dir = [0.6f32, -0.48, 0.64];
                    let at = format!("eye {eye:?}");
                    // Post: grade, colour vision, bloom, sky.
                    let (wl, kl) = (crate::post_lut::lut_volume(&img, world.lut_volumes, world.lut_planes, eye, 1 << 4), crate::post_lut::lut_volume(p, lut.0, lut.1, eye, 1 << 4));
                    assert_eq!(format!("{wl:?}"), format!("{kl:?}"), "grade volume, {at}");
                    assert_eq!(crate::post_lut::settled_slice(&img, world.lut_volumes, wl), crate::post_lut::settled_slice(p, lut.0, kl), "grade slice, {at}");
                    let ws = fastfile_t7::native::vision_slots(&img, world.header_at, 0, 0, world.vision_mods, world.vision_boxes, eye, dir);
                    let ks = fastfile_t7::native::vision_slots(p, vision.0, 0, 0, vision.1, vision.2, eye, dir);
                    assert_eq!(weights(&ws), weights(&ks), "vision weights, {at}");
                    let bits = |b: [[f32; 4]; 61]| b.iter().flatten().map(|v| v.to_bits()).collect::<Vec<_>>();
                    assert_eq!(bits(crate::post_lut::vision_blocks(&img, &ws)), bits(crate::post_lut::vision_blocks(p, &ks)), "vision blocks, {at}");
                    let (wb, wu) = crate::post_bloom::view_settings(&img, world.header_at, 0, 0, world.bloom_mods, world.bloom_boxes, eye, dir);
                    let (kb, ku) = crate::post_bloom::view_settings(p, bloom.0, 0, 0, bloom.1, bloom.2, eye, dir);
                    assert_eq!((format!("{wb:?}"), weights(&wu)), (format!("{kb:?}"), weights(&ku)), "bloom, {at}");
                    let (wm, wu) = fastfile_t7::native::sky_matrices(&img, world.header_at, 0, 0, world.vukko_mods, world.vukko_boxes, eye, dir, 0.3);
                    let (km, ku) = fastfile_t7::native::sky_matrices(p, vukko.0, 0, 0, vukko.1, vukko.2, eye, dir, 0.3);
                    assert_eq!((format!("{wm:?}"), weights(&wu)), (format!("{km:?}"), weights(&ku)), "sky, {at}");
                    // Lighting: fog and lit fog.
                    let (w, k) = (wf.update(&img, &fog, 0, 1, eye, now, now == 0), kf.update(l, &fog_k, 0, 1, eye, now, now == 0));
                    assert_eq!(format!("{w:?}"), format!("{k:?}"), "fog, {at}");
                    let view = crate::volumetric::View { eye, frustum: &[], sun_colour: [1.0, 0.9, 0.8], sun_intensity: 1.0, sun_mode: 1, lighting_state: 0 };
                    let (wp, kp) = (crate::volumetric::pack(&wv, &wbs, &view), crate::volumetric::pack(&kv, &kbs, &view));
                    assert_eq!(wp, kp, "lit fog, {at}");
                    // Exposure.
                    let (we, ke) = (whole.meter(eye, dir), kept.meter(eye, dir));
                    assert_eq!(we.to_bits(), ke.to_bits(), "exposure, {at}");
                    seen[0] += usize::from(wl.is_some());
                    seen[1] += usize::from(!ws.is_empty());
                    seen[2] += usize::from(wp.count > 0);
                    seen[3] += usize::from(we != whole.default);
                    now += 16;
                }
            }
        }
        eprintln!("eyes in a grade volume / vision record / packed lit fog / metered probe: {seen:?}");
        assert!(seen.iter().all(|&n| n > 0), "every read met a record somewhere: {seen:?}");
    }
}
