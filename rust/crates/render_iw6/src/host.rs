//! A whole frame the way Ghosts draws one: the opaque world and placed
//! models into the HDR scene (with the depth), the depth into `FLOATZ`,
//! Ghosts' post-opaque fog (`postopaquefx_dfog` / `_fog`) over it, the
//! blended surfaces on top, the luminance reduction and iris step into
//! `DYNAMIC_IRIS_EXPOSURE`, then Ghosts' final pass (`postfx_macroclut`:
//! exposure, Hable's curve, white scale, the colour table, sRGB, black
//! level) into the display.
//!
//! A frame can be drawn in its three stages (`record_scene`, `record_fog`,
//! `record_post`) so a host draws its own players, view gun and effects into
//! the HDR scene in between and Ghosts' fog, colorization, bloom, iris and
//! final pass grade them as they grade the world.

use std::path::PathBuf;

use glam::{Mat4, Vec3};
use render_h1::code::Camera;
use render_h1::gpu::{Gpu, Tex};

use crate::code::CodeValues;
use crate::post::{Iris, Post};
use crate::scene::Scene;
use crate::world::{Frame, Part, Renderer, SCENE_COLOUR, Target, Targets};

struct Sized {
    width: u32,
    height: u32,
    scene: wgpu::TextureView,
    scene_texture: wgpu::Texture,
    floatz: wgpu::TextureView,
    fogged: wgpu::TextureView,
    fogged_texture: wgpu::Texture,
    /// The bloom's two half-size targets (setup / blur ping-pong).
    bloom: [wgpu::TextureView; 2],
    bloom_size: (u32, u32),
    /// The ambient occlusion's two one-channel targets (made when it is
    /// first on, at its own size).
    ssao: Option<crate::ssao::Targets>,
    /// The depth of field's quarter-size targets (made when it first runs).
    dof: Option<crate::dof::Targets>,
}

/// What a host sets per frame besides the camera.
#[derive(Clone, Copy, Debug)]
pub struct Look {
    pub fog: bool,
    /// The depth band the host's scene is drawn in (reversed; the game
    /// keeps the top band for the view model). None: 0..1.
    pub depth_range: Option<(f32, f32)>,
    /// Seconds since the last frame (the iris adapts by it); None: settle
    /// at once (offline).
    pub dt: Option<f32>,
}

impl Default for Look {
    fn default() -> Self {
        Self {
            fog: true,
            depth_range: None,
            dt: None,
        }
    }
}

/// One line of coverage for the log.
#[derive(Clone, Debug, Default)]
pub struct Coverage {
    pub techniques_made: usize,
    pub techniques_failed: usize,
    pub draws: usize,
    pub drawn: usize,
    pub skipped: usize,
    pub images_loaded: usize,
    pub images_missing: usize,
    pub images_reduced: usize,
    pub post: Vec<String>,
}

/// The sun shadow atlas (`sun_shadow`): its depth and the casters' stand-in
/// colour target.
struct Shadow {
    res: u32,
    depth: wgpu::TextureView,
    colour: wgpu::TextureView,
    /// The atlas itself (the copy's target) and, while players and the gun
    /// cast into it (`caster`), the world's cascades alone.
    live: wgpu::Texture,
    world: Option<(wgpu::Texture, wgpu::TextureView)>,
}

/// `BO2MP_IW6_CASTER_SKIP=0`: the proxies' atlas copy and draws every frame.
fn caster_skip_on() -> bool {
    static S: crate::switch::Switch = crate::switch::Switch::on("BO2MP_IW6_CASTER_SKIP");
    S.get()
}

/// `BO2MP_IW6_BLOOM=0`: no material bloom.
pub fn bloom_on() -> bool {
    static S: crate::switch::Switch = crate::switch::Switch::on("BO2MP_IW6_BLOOM");
    S.get()
}

/// `BO2MP_IW6_SUN_SHADOW=0`: no sun shadow map (the baked visibility, `ld0`).
pub fn sun_shadow_on() -> bool {
    static S: crate::switch::Switch = crate::switch::Switch::on("BO2MP_IW6_SUN_SHADOW");
    S.get()
}

pub struct Host {
    pub renderer: Renderer,
    shadow: Option<Shadow>,
    /// The spot shadow atlas (`spot_shadow`): four tiles.
    spot: Option<Shadow>,
    post: Post,
    floatz: render_h1::post::FloatZ,
    iris: Iris,
    macroclut: Tex,
    sized: Option<Sized>,
    pub zones: Vec<PathBuf>,
    started: std::time::Instant,
    /// Post passes that failed (kept off after), with why.
    pub post_errors: Vec<String>,
    frames: u64,
    /// `BO2MP_IW6_PROFILE=1`: the frame profile.
    pub profile: Option<crate::profile::Profile>,
    /// The graphics settings applied (`native_quality`), the revision
    /// read, and lines about changes for the log.
    pub quality: native_quality::Quality,
    quality_rev: u64,
    pub quality_notes: Vec<String>,
    /// What the shadow atlases hold (`shadow_key`), and a count bumped when
    /// they are made again.
    sun_drawn: [Option<Vec<u32>>; 2],
    spot_drawn: Option<Vec<u32>>,
    /// Per tile of the spot atlas: the light it holds and the key it was drawn under.
    spot_tiles: [Option<(u16, Vec<u32>)>; 4],
    shadow_gen: u64,
    /// The far sun cascade's centre along the light's axes while it is
    /// held (`sun_cache`), and the pass that clears one cascade's quarter.
    sun_far: Option<glam::Vec2>,
    sun_clear: Option<crate::sun_cache::Clear>,
    /// The players' and gun's sun shadow proxies (`caster`): this frame's
    /// list, the pass, and whether the cascades were last drawn with them.
    casters: [crate::caster::Caster; crate::caster::MAX],
    caster_n: usize,
    caster_pass: Option<crate::caster::Casters>,
    caster_mode: bool,
    /// The proxies the live atlas holds (their bits), while it is current.
    caster_drawn: Option<Vec<u32>>,
    /// The frame between its stages (`record_scene` .. `record_post`).
    frame: Option<FrameState>,
    /// The film baked into `MACROCLUT` (for effects graded on the CPU).
    film: crate::look::Film,
    /// The iris exposure read back from the GPU a few frames late.
    readback: ExposureReadback,
    /// The sun's blind and glare (`sun_fx`): its state, the visibility
    /// pass and that pass's late read.
    sun_fx: Option<crate::sun_fx::SunFx>,
    sun_probe: Option<crate::sun_fx::Probe>,
    sun_read: ExposureReadback,
    /// The ambient occlusion's values (the vision's strength and power,
    /// the dvars' defaults) and its rotation picture.
    ssao: crate::ssao::Params,
    rotations: Tex,
    /// The depth of field a script or weapon set (`set_dof`); None: none.
    dof: Option<crate::dof::Dof>,
    /// The camera motion blur (`render_h1`'s pass; INFERRED) and the view it
    /// was made for.
    mblur: Option<(render_h1::screenfx::ScreenFx, wgpu::TextureView)>,
    /// Ghosts' velocity-buffer blur (made at its first frame).
    mb_hq: Option<crate::mblur::Hq>,
    /// Effect lights (`fx_light`): the pass, and the frame's lights as the
    /// host set them (a fixed list: nothing is allocated per frame).
    fx_light: crate::fx_light::Pass,
    fx_lights: [crate::fx_light::Light; crate::fx_light::MAX],
    fx_light_count: usize,
    /// Code-value sets and plane lists the frame copies into (a frame makes
    /// a dozen copies of its values: the passes' own constants over them).
    code_pool: Vec<CodeValues>,
    plane_pool: Vec<Vec<glam::Vec4>>,
    /// The fog technique's name and the AO form of it, kept while the
    /// colorization switch is as it was made (`fog_names`).
    fog_names: Option<(bool, std::sync::Arc<str>, std::sync::Arc<str>, bool)>,
    /// Whether the zones hold the AO techniques (looked up once).
    ssao_has: std::sync::atomic::AtomicU8,
    decals: crate::decal::Decals,
    distort: crate::distort::Distort,
}

/// Where a frame drawn in stages is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stage {
    /// `record_scene` ran: the opaque scene is in `scene_view`.
    Scene,
    /// `record_fog` ran: the fogged scene and the blended world surfaces
    /// are in `fogged_view`.
    Fogged,
}

/// What the later stages of a frame need of its first.
struct FrameState {
    code: CodeValues,
    shadowed: bool,
    planes: Vec<glam::Vec4>,
    culled: bool,
    viewport: Option<[f32; 6]>,
    camera: Camera,
    look: Look,
    width: u32,
    height: u32,
    started: std::time::Instant,
    stage: Stage,
    /// The camera's occlusion buffer and the too-small angle (the scene's
    /// views).
    occlusion: Option<std::sync::Arc<native_cull::Occlusion>>,
    small: Option<f32>,
    /// The sampled depth `record_fog` was given (the motion blur reads it).
    depth: Option<wgpu::TextureView>,
}

const RB_IDLE: u8 = 0;
const RB_COPIED: u8 = 1;
const RB_MAPPING: u8 = 2;
const RB_READY: u8 = 3;

/// One texel of the iris read back (copied one frame, mapped the next,
/// read when the map is done), at most one in flight.
struct ExposureReadback {
    buffer: wgpu::Buffer,
    state: std::sync::Arc<std::sync::atomic::AtomicU8>,
    last: Option<f32>,
    /// A read of 0 counts (the sun's visibility); else only values above 0.
    allow_zero: bool,
}

impl ExposureReadback {
    fn new(gpu: &Gpu, allow_zero: bool) -> Self {
        Self {
            allow_zero,
            buffer: gpu.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("iw6_exposure_readback"),
                size: 256,
                usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }),
            state: Default::default(),
            last: None,
        }
    }

    fn is_idle(&self) -> bool {
        self.state.load(std::sync::atomic::Ordering::Acquire) == RB_IDLE
    }

    /// Takes in a finished read and maps last frame's copy.
    fn collect(&mut self) {
        use std::sync::atomic::Ordering;
        match self.state.load(Ordering::Acquire) {
            RB_READY => {
                {
                    let data = self.buffer.slice(..4).get_mapped_range();
                    let v = f32::from_le_bytes([data[0], data[1], data[2], data[3]]);
                    if v.is_finite() && (v > 0.0 || (self.allow_zero && v == 0.0)) {
                        self.last = Some(v);
                    }
                }
                self.buffer.unmap();
                self.state.store(RB_IDLE, Ordering::Release);
            }
            RB_COPIED => {
                self.state.store(RB_MAPPING, Ordering::Release);
                let st = self.state.clone();
                self.buffer.slice(..).map_async(wgpu::MapMode::Read, move |r| {
                    st.store(if r.is_ok() { RB_READY } else { RB_IDLE }, Ordering::Release);
                });
            }
            _ => {}
        }
    }

    /// Copies the exposure texel when nothing is in flight.
    fn copy(&mut self, enc: &mut wgpu::CommandEncoder, exposure: &wgpu::Texture) {
        use std::sync::atomic::Ordering;
        if self.state.load(Ordering::Acquire) != RB_IDLE {
            return;
        }
        enc.copy_texture_to_buffer(
            exposure.as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &self.buffer,
                layout: wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(256), rows_per_image: Some(1) },
            },
            wgpu::Extent3d { width: 1, height: 1, depth_or_array_layers: 1 },
        );
        self.state.store(RB_COPIED, Ordering::Release);
    }
}

impl Host {
    pub fn load(gpu: Gpu, zones: &[PathBuf], depth_format: wgpu::TextureFormat) -> Result<Self, String> {
        let scene = Scene::load(zones)?;
        Ok(Self::new(gpu, scene, depth_format, zones.to_vec()))
    }

    pub fn new(gpu: Gpu, scene: Scene, depth_format: wgpu::TextureFormat, zones: Vec<PathBuf>) -> Self {
        crate::loadtime::stage("(scene built)");
        let post = Post::new(gpu.clone(), scene.pool.clone());
        crate::loadtime::stage("post new");
        let floatz = render_h1::post::FloatZ::new(&gpu);
        let mut iris = Iris::new(&gpu);
        let e = scene.tone.exposure;
        iris.set(&gpu, 0.5 * (e[0] + e[1]));
        // The film baked into the final pass's colour table
        // (`BO2MP_IW6_FILM=0`: the identity table).
        let mut film = crate::look::Film::from_vision(&scene.vision);
        if std::env::var("BO2MP_IW6_FILM").is_ok_and(|v| v.trim() == "0") {
            film.on = false;
        }
        eprintln!(
            "note: iw6 film: {} (tints dark {:?} / light {:?}, desaturation {} / {}, bias {:.3}); BLACK_LEVEL (0, 1): r_blacklevel 0, the vision sets none",
            if film.on { "baked into MACROCLUT" } else { "off, identity MACROCLUT" },
            film.base, film.base.iter().zip(film.delta).zip(film.quad).map(|((b, d), q)| b + d + q).collect::<Vec<_>>(),
            film.desat_dark, film.desat, film.bias,
        );
        // The final pass's colour table: the colorization set's low / mid /
        // high grade, as the engine bakes it (READ; no Ghosts technique and
        // no part of that bake applies the vision's film). Without the
        // three grades, or with `BO2MP_IW6_LMH=0`, the film baked as before
        // (INFERRED, MWR's curve).
        let lmh = scene
            .colorization
            .as_ref()
            .and_then(|c| c.lmh.as_ref())
            .filter(|_| !std::env::var("BO2MP_IW6_LMH").is_ok_and(|v| v.trim() == "0"));
        let macroclut = match lmh {
            Some(l) => {
                eprintln!("note: iw6 macroclut: the colorization set's low / mid / high grade (the film is not applied)");
                crate::post::macroclut_lmh(&gpu, l)
            }
            None => crate::post::macroclut(&gpu, &film),
        };
        let readback = ExposureReadback::new(&gpu, false);
        let sun_read = ExposureReadback::new(&gpu, true);
        let sun_rec = match scene.sun_flare {
            Some(r) if crate::sun_fx::from_dvars() => Some(crate::sun_fx::dvar_record(r.position)),
            r => r,
        };
        let sun_fx = crate::sun_fx::SunFx::new_opt(sun_rec);
        match (&scene.sun_flare, &sun_fx) {
            (Some(r), fx) => eprintln!("note: iw6 sun fx: {} (valid {}, sprite {}, blind dot {:.3}..{:.3} darken {}, fade {}/{} ms; glare dot {:.3}..{:.3} lighten {}, fade {}/{} ms; dir {:?})", if fx.is_some() { "on" } else { "no blind or glare" }, r.has_valid_data, r.sprite_size, r.blind_min_dot, r.blind_max_dot, r.blind_max_darken, r.blind_fade_in, r.blind_fade_out, r.glare_min_dot, r.glare_max_dot, r.glare_max_lighten, r.glare_fade_in, r.glare_fade_out, r.position),
            (None, _) => eprintln!("note: iw6 sun fx: no record"),
        }
        let sun_probe = sun_fx.is_some().then(|| crate::sun_fx::Probe::new(&gpu));
        let vision = scene.vision.clone();
        let host_gpu = gpu.clone();
        let fx_light = crate::fx_light::Pass::new(&gpu, SCENE_COLOUR);
        crate::loadtime::stage("host pre-renderer (macroclut etc.)");
        let renderer = Renderer::new(gpu, scene, depth_format);
        crate::loadtime::stage("Renderer::new (buffers)");
        let quality = native_quality::current();
        let mut host = Self {
            renderer,
            sun_read,
            sun_fx,
            sun_probe,
            shadow: None,
            spot: None,
            post,
            floatz,
            iris,
            macroclut,
            sized: None,
            zones,
            started: std::time::Instant::now(),
            post_errors: Vec::new(),
            frames: 0,
            profile: crate::profile::on().then(Default::default),
            quality,
            quality_rev: native_quality::revision(),
            quality_notes: Vec::new(),
            sun_drawn: [None, None],
            spot_drawn: None,
            spot_tiles: [None, None, None, None],
            shadow_gen: 0,
            sun_far: None,
            sun_clear: None,
            casters: Default::default(),
            caster_n: 0,
            caster_pass: None,
            caster_mode: false,
            caster_drawn: None,
            frame: None,
            film,
            readback,
            ssao: crate::ssao::Params::from_vision(&vision),
            rotations: crate::ssao::rotations(&host_gpu),
            dof: None,
            mblur: None,
            mb_hq: None,
            fx_light,
            fx_lights: [crate::fx_light::Light::default(); crate::fx_light::MAX],
            fx_light_count: 0,
            code_pool: Vec::new(),
            plane_pool: Vec::new(),
            fog_names: None,
            ssao_has: std::sync::atomic::AtomicU8::new(0),
            decals: Default::default(),
            distort: Default::default(),
        };
        crate::loadtime::stage("host fields, renderer");
        host.apply_quality(quality, true);
        crate::loadtime::stage("apply_quality");
        // Every list a frame draws, made now (no hitch at the first view
        // of anything): pipelines, groups, constant blocks.
        let (sun, spot) = (host.shadow.is_some(), host.spot.is_some());
        if let Err(e) = host.renderer.warm(sun, spot) {
            host.quality_notes.push(format!("iw6 fps: lists not made ahead: {e}"));
        }
        crate::loadtime::stage("renderer warm (lists, pipelines)");
        host.renderer.save_pipeline_cache();
        crate::loadtime::stage("pipeline cache saved");
        for (tech, add) in [("material_bloom_setup_alpha_intensity", false), ("material_bloom_hq_blur", false), ("material_bloom_apply", true)] {
            host.post.warm(tech, SCENE_COLOUR, add);
        }
        host.warm_post();
        crate::loadtime::stage("post warm");
        crate::loadtime::totals();
        host
    }

    /// The depth of field for the next frames (the eight distances a
    /// script or the weapon's ADS sets); None clears it.
    pub fn set_dof(&mut self, dof: Option<[f32; 8]>) {
        self.dof = dof.map(crate::dof::Dof::from_array);
    }

    /// Whether the depth of field runs this frame: the option row, the
    /// switch, a range in use and the techniques in the zones.
    fn dof_active(&self) -> Option<crate::dof::Dof> {
        let d = self.dof.filter(crate::dof::Dof::active)?;
        if !crate::dof::on() || !self.quality.depth_of_field {
            return None;
        }
        ["dof_downsample", "dof_near_coc", "postfx_dof", "material_bloom_hq_blur"]
            .iter()
            .all(|t| self.post.has(t))
            .then_some(d)
    }

    /// The post passes the graphics settings may turn on, made now (no
    /// hitch when an option is switched on mid-match).
    fn warm_post(&mut self) {
        // (The one-cascade clear pipeline, made lazily at the first frame that needs it: a hitch.)
        if self.shadow.is_some() && self.sun_clear.is_none() {
            self.sun_clear = Some(crate::sun_cache::Clear::new(&self.renderer.gpu.device));
        }
        if self.quality.depth_of_field && crate::dof::on() {
            for t in ["dof_downsample", "dof_near_coc", "material_bloom_hq_blur"] {
                self.post.warm(t, SCENE_COLOUR, false);
            }
            self.post.warm("postfx_dof", SCENE_COLOUR, false);
        }
        if self.quality.motion_blur && crate::mblur::on() && crate::mblur::mode() == crate::mblur::Mode::Velocity {
            for t in [crate::mblur::TECH_TILE, crate::mblur::TECH_NEIGHBOUR] {
                self.post.warm(t, crate::mblur::VEL_FORMAT, false);
            }
            self.post.warm(crate::mblur::TECH_APPLY, SCENE_COLOUR, false);
        }
        if self.ssao_half().is_some() {
            let f = crate::ssao::FORMAT;
            self.post.warm(crate::ssao::calc_technique(), f, false);
            self.post.warm("ssao_vblur_lq", f, false);
            let name = crate::ssao::composite_technique(&self.fog_technique());
            self.post.warm(&name, SCENE_COLOUR, false);
        }
    }

    /// Whether the ambient occlusion draws this frame, and if so whether
    /// its targets are half size: the preset (HIGH half, ULTRA the render
    /// size), the option rows, `BO2MP_IW6_SSAO`, a vision strength above
    /// zero and the techniques in the zones.
    fn ssao_half(&self) -> Option<bool> {
        if !crate::ssao::on() || !self.quality.ambient_occlusion || !self.ssao.active() {
            return None;
        }
        use std::sync::atomic::Ordering::Relaxed;
        // (0 not asked yet, 1 no, 2 yes.)
        let has = match self.ssao_has.load(Relaxed) {
            0 => {
                let h = self.post.has(crate::ssao::calc_technique()) && self.post.has("ssao_vblur_lq");
                self.ssao_has.store(if h { 2 } else { 1 }, Relaxed);
                h
            }
            v => v == 2,
        };
        has.then_some(self.quality.preset != native_quality::Preset::Ultra)
    }

    /// `fog_technique` and its AO form, made once (again when the
    /// colorization switch changes).
    fn fog_names(&mut self) -> (std::sync::Arc<str>, std::sync::Arc<str>, bool) {
        let on = self.colorization().is_some();
        if self.fog_names.as_ref().is_none_or(|(o, ..)| *o != on) {
            let plain = self.fog_technique();
            let ao = crate::ssao::composite_technique(&plain);
            let has = self.post.has(&ao);
            self.fog_names = Some((on, plain.into(), ao.into(), has));
        }
        let (_, a, b, has) = self.fog_names.as_ref().expect("made above");
        (a.clone(), b.clone(), *has)
    }

    /// The post-opaque technique of the map's fog and colorization.
    fn fog_technique(&self) -> String {
        let fog = &self.renderer.scene.fog;
        let base = if fog.sun_color != fog.color { "postopaquefx_dfog" } else { "postopaquefx_fog" };
        match self.colorization() {
            Some(c) if c.alternate.is_some() => format!("{base}_color_altd"),
            Some(_) => format!("{base}_color"),
            None => base.to_string(),
        }
    }

    /// The vision's colorization when it is used (`BO2MP_IW6_COLORIZATION=0` off).
    fn colorization(&self) -> Option<&crate::look::Colorization> {
        self.renderer
            .scene
            .colorization
            .as_ref()
            .filter(|_| {
                static S: crate::switch::Switch = crate::switch::Switch::on("BO2MP_IW6_COLORIZATION");
                S.get()
            })
    }

    /// A shadow atlas `2 res` a side bound as code texture `name`.
    fn shadow_atlas(renderer: &mut Renderer, name: &str, res: u32) -> Shadow {
        let g = &renderer.gpu;
        let depth = g.device.create_texture(&wgpu::TextureDescriptor {
            label: Some(name),
            size: wgpu::Extent3d { width: 2 * res, height: 2 * res, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: crate::world::SHADOW_DEPTH,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                | wgpu::TextureUsages::TEXTURE_BINDING
                | wgpu::TextureUsages::COPY_SRC
                | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let colour = render_h1::post::target(g, name, 2 * res, 2 * res, crate::world::SHADOW_COLOUR);
        let sampled = depth.create_view(&wgpu::TextureViewDescriptor {
            aspect: wgpu::TextureAspect::DepthOnly,
            ..Default::default()
        });
        let view = depth.create_view(&Default::default());
        let live = depth.clone();
        renderer.code_textures.insert(
            name.to_string(),
            Tex {
                texture: depth,
                view: sampled,
                dim: wgpu::TextureViewDimension::D2,
            },
        );
        Shadow {
            res,
            depth: view,
            colour: colour.create_view(&Default::default()),
            live,
            world: None,
        }
    }

    /// The graphics settings (`native_quality`) applied: the sun shadow
    /// map (on, its size), the spot shadow atlas (how many lights, its
    /// size), spot and omni lights, texture filtering, bloom, LOD and draw
    /// distances. The `BO2MP_IW6_*` switches still win. Lists the changes
    /// touch are made again at once (pipelines are kept); the picture size
    /// cap waits for the next map.
    pub fn apply_quality(&mut self, mut q: native_quality::Quality, first: bool) {
        // Ghosts has no real-time lighting setting: its spot and omni lights
        // are the map's own lighting, so the REAL-TIME LIGHTING option (the
        // Black Ops III look's) does not reach them; only the preset does.
        if q.options_off & native_quality::OFF_LOCAL_LIGHTS != 0 {
            let base = native_quality::Quality::preset(q.preset);
            q.local_lights = base.local_lights;
            q.spot_shadows = if q.options_off & native_quality::OFF_SHADOWS != 0 { 0 } else { base.spot_shadows };
            q.options_off &= !native_quality::OFF_LOCAL_LIGHTS;
        }
        let old = self.quality;
        self.quality = q;
        let env_res = |k: &str, lo: u32, hi: u32| {
            codlegacy_env::var(k)
                .ok()
                .and_then(|v| v.trim().parse::<u32>().ok())
                .filter(|n| n.is_power_of_two() && (lo..=hi).contains(n))
        };
        let r = &mut self.renderer;
        let mut remake = false;
        // The sun: 2R a side (`sun_shadow`); `BO2MP_IW6_SHADOW_RES=n` sets R.
        let sun = (sun_shadow_on() && q.sun_shadow).then(|| env_res("BO2MP_IW6_SHADOW_RES", 256, 4096).unwrap_or(q.sun_shadow_res));
        if first || sun != self.shadow.as_ref().map(|s| s.res) {
            r.code_textures.remove("SHADOWMAP_SUN");
            self.shadow = sun.map(|res| Self::shadow_atlas(r, "SHADOWMAP_SUN", res));
            remake = true;
        }
        // The spot lights: four R tiles of one 2R atlas
        // (`BO2MP_IW6_SPOT_SHADOW_RES=n` sets R; INFERRED 512).
        let local = crate::world::local_lights_on() && q.local_lights;
        let spot = (crate::spot_shadow::on() && local && q.spot_shadows > 0)
            .then(|| env_res("BO2MP_IW6_SPOT_SHADOW_RES", 128, 2048).unwrap_or(q.spot_shadow_res));
        if first || spot != self.spot.as_ref().map(|s| s.res) {
            r.code_textures.remove("SHADOWMAP_SPOT");
            self.spot = spot.map(|res| Self::shadow_atlas(r, "SHADOWMAP_SPOT", res));
            remake = true;
        }
        let technique = spot.is_some();
        if r.local_lights != local || r.spot_shadow_technique != technique || r.anisotropy != q.anisotropy {
            r.local_lights = local;
            r.spot_shadow_technique = technique;
            r.anisotropy = q.anisotropy;
            remake = true;
        }
        r.lod_scale = q.lod_distance;
        r.draw_scale = q.draw_distance;
        if remake {
            self.shadow_gen += 1;
        }
        if remake && !first {
            r.invalidate_lists();
            let (sun, spot) = (self.shadow.is_some(), self.spot.is_some());
            if let Err(e) = r.warm(sun, spot) {
                self.quality_notes.push(format!("iw6 fps: lists not made again: {e}"));
            }
        }
        if !first {
            // (An option switched on now: its passes made now, no hitch.)
            self.warm_post();
            let wait = if old.max_texture != q.max_texture { "; picture size from the next map" } else { "" };
            self.quality_notes.push(format!("iw6 fps: quality {}{wait}", q.summary()));
        }
    }

    /// The `iw6 fps:` line: the quality applied, the lists (draws,
    /// pipelines, bind group sets, constant plans, frame blocks), the last
    /// frame's draws after culling and the runs merged.
    pub fn fps_line(&self) -> String {
        let r = &self.renderer;
        let list = |sun: bool, t: Target, v: u8| {
            r.list_counts(sun, t, v)
                .map_or("not made".to_string(), |(d, p, g, pl, b)| format!("{d} draws, {p} pipelines, {g} groups, {pl} plans, {b} frame blocks"))
        };
        format!(
            "iw6 fps: {}: quality {}; scene list {}; sun shadow list {}; spot shadow list {}; last frame {} scene draws after culling (+{} into the last shadow view); techniques {}",
            r.scene.name,
            self.quality.summary(),
            list(self.shadow.is_some(), Target::Scene, 0),
            list(false, Target::Shadow, 1),
            list(false, Target::SpotShadow, 3),
            r.report.drawn,
            r.report.shadow_drawn,
            r.report.techniques,
        )
    }

    /// bo2mp: the frame's grade for effects drawn after the final pass
    /// (`fx_iw4::iw6::grade`): the iris exposure as last read back from the
    /// GPU (before the first read, the iris's starting value), the map's
    /// tone curve and its film.
    /// This frame's effect lights (`fx_light`; the nearest first). Taken
    /// at the next `record_fog`, under the graphics preset's cap; none
    /// clears them.
    pub fn set_fx_lights(&mut self, lights: &[crate::fx_light::Light]) {
        let n = lights.len().min(crate::fx_light::MAX);
        self.fx_lights[..n].copy_from_slice(&lights[..n]);
        self.fx_light_count = n;
    }

    /// A Ghosts decal (effect element type 15) cut out of the map's world
    /// where it falls (`decal`; `BO2MP_IW6_DECALS=0`: none). False when no
    /// mark was made.
    pub fn add_decal(&mut self, spawn: &crate::decal::Spawn<'_>) -> bool {
        let now = self.started.elapsed().as_millis() as u64;
        self.decals.spawn(&mut self.renderer, spawn, now)
    }

    /// This frame's sun shadow proxies for the players and the gun
    /// (`caster`; `BO2MP_IW6_SUN_CASTERS=0`: none); call before `record_scene`.
    pub fn set_sun_casters(&mut self, list: &[crate::caster::Caster]) {
        let n = if crate::caster::on() { list.len().min(crate::caster::MAX) } else { 0 };
        self.casters[..n].copy_from_slice(&list[..n]);
        self.caster_n = n;
    }

    /// This frame's distortion sprites (`distort`; `BO2MP_IW6_DISTORTION=0`:
    /// none), as `(material name, quad)`; call before `record_scene`. The
    /// scene is copied for them in `record_fog` only when one is in view.
    pub fn set_distortion(&mut self, quads: &[(&str, crate::distort::Quad)]) {
        self.distort.set(&mut self.renderer, quads);
        if self.distort.visible > 0 {
            // (The copy exists before the lists that bind it are made.)
            let size = self.sized.as_ref().map(|s| (s.width, s.height));
            if let Some(size) = size
                && self.distort.ensure_copy(&mut self.renderer, size, SCENE_COLOUR)
            {
                self.distort.dirty = true;
            }
        }
        if self.distort.dirty {
            self.distort.dirty = false;
            self.remake_lists("distortion");
        }
    }

    /// Drops and makes again every draw list (a ring was added, or a code
    /// texture the lists bind was made).
    fn remake_lists(&mut self, what: &str) {
        self.renderer.invalidate_lists();
        let (sun, spot) = (self.shadow.is_some(), self.spot.is_some());
        if let Err(e) = self.renderer.warm(sun, spot) {
            self.quality_notes.push(format!("iw6 {what}: lists not made again: {e}"));
        }
    }

    /// The decals' fades and removals, and the lists made again after a new
    /// ring (once, before the frame's lists are used).
    fn update_decals(&mut self) {
        let now = self.started.elapsed().as_millis() as u64;
        self.decals.update(&self.renderer, now);
        if self.decals.dirty {
            self.decals.dirty = false;
            self.remake_lists("decals");
        }
    }

    pub fn fx_grade(&self) -> fx_iw4::iw6::grade::Grade {
        let t = &self.renderer.scene.tone;
        let start = 0.5 * (t.exposure[0] + t.exposure[1]);
        grade_of(t, &self.film, self.readback.last.unwrap_or(start))
    }

    /// Whether the grade's exposure has been read back from the GPU.
    pub fn exposure_read(&self) -> bool {
        self.readback.last.is_some()
    }

    /// Moves the scene's mover `key` (a door, the hopper) to `origin` and
    /// `angles` (pitch, yaw, roll in degrees); false when the scene has no
    /// such mover.
    pub fn set_mover_pose(&mut self, key: &crate::scene::MoverKey, origin: [f32; 3], angles: [f32; 3]) -> bool {
        self.set_mover_state(key, origin, angles, false)
    }

    /// `set_mover_pose`, and `hidden` (the scripts hid or deleted it) draws it at no size.
    pub fn set_mover_state(&mut self, key: &crate::scene::MoverKey, origin: [f32; 3], angles: [f32; 3], hidden: bool) -> bool {
        let objects: Vec<u32> = self
            .renderer
            .scene
            .movers
            .iter()
            .filter(|m| m.key.matches(key))
            .flat_map(|m| m.objects.iter().copied())
            .collect();
        let (_, world) = crate::scene::placement(origin, fastfile_iw6::map::axis_of(angles), if hidden { 0.0 } else { 1.0 });
        for &o in &objects {
            self.renderer.set_object_world(o, world);
        }
        !objects.is_empty()
    }

    /// `set_mover_state` for a model that plays an animation: each part (bone) of the mover is
    /// placed by the entity's pose and the bone's skinning pose (`pose * bind^-1`, model space; one
    /// per bone of the skeleton, in its order). A mover drawn whole is placed as before.
    pub fn set_mover_skinned(&mut self, key: &crate::scene::MoverKey, origin: [f32; 3], angles: [f32; 3], hidden: bool, skin: &[iw6_pose::Bone]) -> bool {
        let movers: Vec<(Vec<u32>, Vec<(u32, u16)>)> = self
            .renderer
            .scene
            .movers
            .iter()
            .filter(|m| m.key.matches(key))
            .map(|m| (m.objects.clone(), m.bones.clone()))
            .collect();
        let scale = if hidden { 0.0 } else { 1.0 };
        let axes = fastfile_iw6::map::axis_of(angles);
        let (_, entity) = crate::scene::placement(origin, axes, scale);
        let mut any = false;
        for (objects, bones) in movers {
            for &o in &objects {
                let world = match bones.iter().find(|b| b.0 == o) {
                    Some(&(_, bone)) => {
                        let (q, t) = skin.get(usize::from(bone)).copied().unwrap_or((iw6_pose::IDENTITY, [0.0; 3]));
                        let (_, m) = crate::scene::placement(t, iw6_pose::axes(q), 1.0);
                        entity * m
                    }
                    None => entity,
                };
                self.renderer.set_object_world(o, world);
                any = true;
            }
        }
        any
    }

    pub fn scene(&self) -> &Scene {
        &self.renderer.scene
    }

    pub fn coverage(&self) -> Coverage {
        let r = &self.renderer.report;
        let (ok, missing) = self.renderer.image_counts();
        Coverage {
            techniques_made: r.techniques,
            techniques_failed: r.failed.len(),
            draws: r.draws,
            drawn: r.drawn,
            skipped: r.skipped.values().sum(),
            images_loaded: ok,
            images_missing: missing,
            images_reduced: self.renderer.scene.images.reduced.len(),
            post: self.post_errors.clone(),
        }
    }

    /// The `iw6 native:` log line.
    pub fn coverage_line(&self) -> String {
        let c = self.coverage();
        let r = &self.renderer.report;
        format!(
            "iw6 native: {}: techniques made {} (failed {}), draws {} of {} drawn ({} skipped), slots {:?}, pictures {} loaded / {} missing ({} at a smaller size; bind groups on a fallback picture: {} opaque grey, {} alpha-0 grey), opaque draws with no depth pre-pass writing their own depth {} (BO2MP_IW6_OWN_DEPTH=0 leaves them without), post {}",
            self.renderer.scene.name,
            c.techniques_made,
            c.techniques_failed,
            c.drawn,
            c.draws,
            c.skipped,
            r.by_slot,
            c.images_loaded,
            c.images_missing,
            c.images_reduced,
            r.fallback_groups[0],
            r.fallback_groups[1],
            r.own_depth,
            if c.post.is_empty() { "all Ghosts' own".to_string() } else { format!("fallbacks: {}", c.post.join("; ")) }
        )
    }

    fn sized(&mut self, w: u32, h: u32) {
        if self.sized.as_ref().is_some_and(|s| s.width == w && s.height == h) {
            return;
        }
        self.fx_light.invalidate();
        let g = &self.renderer.gpu;
        let scene = render_h1::post::target(g, "iw6_scene", w, h, SCENE_COLOUR);
        let floatz = render_h1::post::target(g, "iw6_floatz", w, h, wgpu::TextureFormat::R32Float);
        let fogged = render_h1::post::target(g, "iw6_fogged", w, h, SCENE_COLOUR);
        let bloom_size = ((w / 2).max(1), (h / 2).max(1));
        let bloom = [0, 1].map(|_| {
            render_h1::post::target(g, "iw6_bloom", bloom_size.0, bloom_size.1, SCENE_COLOUR).create_view(&Default::default())
        });
        self.sized = Some(Sized {
            bloom,
            bloom_size,
            ssao: None,
            dof: None,
            width: w,
            height: h,
            scene: scene.create_view(&Default::default()),
            scene_texture: scene,
            floatz: floatz.create_view(&Default::default()),
            fogged: fogged.create_view(&Default::default()),
            fogged_texture: fogged,
        });
    }

    /// The frame's code values: the camera, the sun, the fog, the sky.
    pub fn code(&self, camera: &Camera, look: &Look) -> CodeValues {
        let mut c = CodeValues::default();
        self.code_into(&mut c, camera, look);
        c
    }

    /// `code`, into `c` (its allocations reused: every constant this sets
    /// is set each frame).
    pub fn code_into(&self, c: &mut CodeValues, camera: &Camera, look: &Look) {
        let s = &self.renderer.scene;
        c.view_projection = camera.view_projection;
        c.eye = camera.eye;
        c.rotation = camera.rotation;
        c.clear_missing();
        let eye = camera.eye;
        c.set_matrix("TRANSPOSE_VIEW_PROJECTION_MATRIX", camera.view_projection);
        c.set_matrix("VIEW_PROJECTION_MATRIX", camera.view_projection);
        c.set_matrix("TRANSPOSE_PROJECTION_MATRIX", camera.view_projection * camera.rotation.inverse());
        c.set_matrix("TRANSPOSE_VIEW_MATRIX", camera.rotation);
        c.set_vec("EYEOFFSET", [eye.x, eye.y, eye.z, 1.0]);
        c.set_vec("MATERIAL_COLOR", [1.0; 4]);
        // The displacement techniques drawn without tessellation
        // (`tessellation_consts`).
        let (tp, tp2) = tessellation_consts();
        c.set_vec("TESSELLATION_PARMS", tp);
        c.set_vec("TESSELLATION_PARMS2", tp2);
        c.set_vec("MSAA_ALPHA_TEST_OFFSETS_PS", [0.0; 4]);
        crate::scope::set_idle_constants(c, None);
        // (Offline, no frame time: the clock held at 0, so animated
        // materials draw the same every run.)
        let t = if look.dt.is_some() { self.started.elapsed().as_secs_f32() } else { 0.0 };
        let frac = t - t.floor();
        let a = frac * std::f32::consts::TAU;
        c.set_vec("GAMETIME", [a.sin(), a.cos(), frac, t % 43200.0]);
        let sun = s.sun;
        c.set_vec("LIGHT_POSITION", [sun.dir.x, sun.dir.y, sun.dir.z, 0.0]);
        c.set_vec("LIGHT_DIFFUSE", [sun.diffuse.x, sun.diffuse.y, sun.diffuse.z, 1.0]);
        c.set_vec("LIGHT_SPECULAR", [sun.specular.x, sun.specular.y, sun.specular.z, 1.0]);
        let f = if look.fog { s.fog } else { crate::look::FogConsts::none() };
        c.set_vec("FOG", f.fog);
        c.set_vec("FOG_COLOR_LINEAR", f.color);
        c.set_vec("FOG_SUN_CONSTS", f.sun_consts);
        c.set_vec("FOG_SUN_COLOR_LINEAR", f.sun_color);
        // (The sky shader dots it with a camera-relative world direction.)
        c.set_vec("FOG_SUN_DIR", f.sun_dir);
        c.set_vec("FOG_HEIGHT", f.height);
        c.set_vec("FOG_DEPTHHACK", f.fog);
        c.set_vec("HDR_SKY_INTENSITY", s.sky_intensity);
        // No sun shadow map: every cascade weight zero (the baked
        // visibility decides).
        c.set_matrix("TRANSPOSE_SHADOW_LOOKUP_MATRIX", Mat4::ZERO);
        c.set_vec("SHADOWMAP_SWITCH_PARTITION", [0.0; 4]);
        c.set_vec("SHADOWMAP_SCALE", [0.0, 0.0, 100.0, 100.0]);
        c.set_vec("SUN_SHADOWMAP_PIXEL_ADJUST", [0.0; 4]);
        // The depth band and the post constants.
        let (lo, hi) = look.depth_range.unwrap_or((0.0, 1.0));
        let band = (hi - lo).max(1e-6);
        // FLOATZ is our reversed depth: near / distance in the band.
        // Ghosts' fog reads `distance = ZNEAR * 63/64 / floatz`.
        c.set_vec("ZNEAR", [camera.near * band * 64.0 / 63.0, 0.0, 0.0, 0.0]);
        c.set_vec("INV_SCENE_PROJECTION", [camera.tan_x, 0.0, camera.tan_y, 0.0]);
        c.set_matrix("TRANSPOSE_WORLD_VIEW_PROJECTION_MATRIX0", Mat4::IDENTITY);
        let tone = &s.tone;
        // Hable's six groups as the shader combines them (`tone_consts`).
        for (name, val) in tone_consts(tone) {
            c.set_vec(name, val);
        }
        c.set_vec("TONE_MAPPING_EXPOSURE", [tone.exposure[0], tone.exposure[1], tone.exposure[2], 0.0]);
        c.set_vec("TONE_MAPPING_WHITE_ADAPTATION_RATES", [tone.rates[0], tone.rates[1], look.dt.unwrap_or(1.0), 0.0]);
        // `r_blacklevel` (default 0): `saturate((g - level) / (1 - level))`.
        c.set_vec("BLACK_LEVEL", [0.0, 1.0, 0.0, 0.0]);
        c.set_vec("COLORIZATION_POWER_AND_DESATURATION", [1.0, 1.0, 1.0, 0.0]);
        c.set_vec("COLORIZATION_SCALE", [1.0; 4]);
        c.set_vec("COLORIZATION_BIAS", [0.0; 4]);
    }

    /// Draws a frame: the scene into `depth` (the host's, reversed) and
    /// the finished picture into `display`. The three stages below in one
    /// go: `record_scene`, `record_fog`, `record_post`.
    #[allow(clippy::too_many_arguments)]
    pub fn record(
        &mut self,
        enc: &mut wgpu::CommandEncoder,
        depth: &wgpu::TextureView,
        depth_sampled: &wgpu::TextureView,
        display: &wgpu::TextureView,
        display_format: wgpu::TextureFormat,
        width: u32,
        height: u32,
        camera: &Camera,
        look: &Look,
    ) -> Result<(), String> {
        self.record_scene(enc, depth, width, height, camera, look)?;
        self.record_fog(enc, depth, depth_sampled)?;
        self.record_post(enc, display, display_format)
    }

    /// The first stage of a frame: the shadow maps, then the opaque world
    /// and models into the HDR scene target (`scene_view`) and `depth`.
    /// A host drawing its own opaque things (players, the view gun) into
    /// the same scene does it between this and `record_fog`, so Ghosts'
    /// fog, colorization, bloom, iris and final pass apply to them too.
    pub fn record_scene(
        &mut self,
        enc: &mut wgpu::CommandEncoder,
        depth: &wgpu::TextureView,
        width: u32,
        height: u32,
        camera: &Camera,
        look: &Look,
    ) -> Result<(), String> {
        let started = std::time::Instant::now();
        let profile_on = self.profile.is_some();
        if let Some(f) = self.frame.take() {
            self.code_pool.push(f.code);
            self.plane_pool.push(f.planes);
        }
        self.update_decals();
        self.renderer.begin_frame();
        // The graphics settings changed: applied before this frame.
        let rev = native_quality::revision();
        if rev != self.quality_rev {
            self.quality_rev = rev;
            let q = native_quality::current();
            if q != self.quality {
                self.apply_quality(q, false);
            }
        }
        if let Some(p) = &mut self.profile {
            p.begin(&self.renderer.gpu, enc);
        }
        self.sized(width, height);
        let mut code = self.code_pool.pop().unwrap_or_default();
        self.code_into(&mut code, camera, look);
        let t = self.sized.as_ref().ok_or("no targets")?;
        let viewport = look
            .depth_range
            .map(|(lo, hi)| [0.0, 0.0, width as f32, height as f32, lo, hi]);
        if let Some(p) = &mut self.profile {
            p.phase("setup", phase_since(started));
        }
        // The sun shadow cascades, each into its quarter of the atlas.
        // Proxies in the sun shadow: the world's cascades go to an atlas of
        // their own and are copied over the live one each frame, then the
        // proxies are drawn into that (`caster`).
        let with_casters = self.caster_n > 0 && self.shadow.is_some();
        if with_casters {
            let device = &self.renderer.gpu.device;
            if self.caster_pass.is_none() {
                self.caster_pass = Some(crate::caster::Casters::new(device));
            }
            if let Some(sh) = &mut self.shadow
                && sh.world.is_none()
            {
                let t = device.create_texture(&wgpu::TextureDescriptor {
                    label: Some("iw6_sun_world"),
                    size: wgpu::Extent3d { width: 2 * sh.res, height: 2 * sh.res, depth_or_array_layers: 1 },
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format: crate::world::SHADOW_DEPTH,
                    usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
                    view_formats: &[],
                });
                let v = t.create_view(&Default::default());
                sh.world = Some((t, v));
                self.sun_drawn = [None, None];
                self.caster_drawn = None;
            }
        }
        if with_casters != self.caster_mode {
            // (The cascades go to the other atlas, or come back: both redrawn.)
            self.caster_mode = with_casters;
            self.sun_drawn = [None, None];
            self.caster_drawn = None;
        }
        let shadowed = if let Some(sh) = &self.shadow {
            let dst: &wgpu::TextureView = match (&sh.world, with_casters) {
                (Some((_, v)), true) => v,
                _ => &sh.depth,
            };
            let s = &self.renderer.scene;
            // Each cascade on a texel of its own grid (`BO2MP_IW6_SUN_SNAP=0`:
            // on the eye), the far one held while the eye stays near its
            // centre (`sun_cache`, `BO2MP_IW6_SUN_CACHE=0`: not held).
            let snap = crate::sun_shadow::snap_on();
            let per_cascade = snap && crate::sun_cache::on();
            let centres = snap.then(|| {
                let at = crate::sun_shadow::eye_on_light(camera.eye, s.sun.dir);
                let c = crate::sun_cache::centres(at, self.sun_far, per_cascade);
                self.sun_far = Some(c[1]);
                c
            });
            let setup = crate::sun_shadow::setup_at(camera.eye, s.sun.dir, s.bounds, sh.res, centres);
            // The cascades hold only the world's static casters: with the same
            // projections, the same view and nothing moved, last frame's atlas
            // is this frame's. (The view counts too: the displacement casters'
            // techniques read the camera's rotation and projection, so a turn
            // changes a few of their texels.) Per cascade: the near one as
            // that; the far one by its texels in the world alone (a turn or
            // a step leaves it; its few view-dependent texels lag until it
            // is next drawn).
            let extra = self.renderer.moved + self.shadow_gen;
            let keys = if per_cascade {
                [
                    shadow_key_with(&setup.world[0], [camera.view_projection, camera.rotation].into_iter(), extra),
                    shadow_key_with(&setup.world[1], std::iter::empty(), extra),
                ]
            } else {
                let key = shadow_key(
                    setup.view_projection.iter().copied().chain([camera.view_projection, camera.rotation]),
                    extra,
                );
                [key.clone(), key]
            };
            let draw = [0, 1].map(|k| !(shadow_cache_on() && self.sun_drawn[k].as_ref() == Some(&keys[k])));
            let cached = !draw[0] && !draw[1];
            if cached {
                if let Some(p) = &mut self.profile {
                    p.phase("sun cached", phase_since(std::time::Instant::now()));
                }
            } else if draw[0] != draw[1] && let Some(p) = &mut self.profile {
                p.phase(if draw[0] { "sun far cached" } else { "sun near cached" }, phase_since(std::time::Instant::now()));
            }
            self.sun_drawn = keys.map(Some);
            // One cascade alone: its quarter cleared, the other's kept.
            let alone = draw[0] != draw[1];
            if alone && self.sun_clear.is_none() {
                self.sun_clear = Some(crate::sun_cache::Clear::new(&self.renderer.gpu.device));
            }
            for k in (0..2).filter(|&k| draw[k]) {
                if alone && let Some(c) = &self.sun_clear {
                    c.record(enc, &sh.colour, dst, setup.viewport[k]);
                }
                let mut ck = self.code_pool.pop().unwrap_or_default();
                ck.clone_from(&code);
                ck.view_projection = setup.view_projection[k];
                ck.set_matrix("TRANSPOSE_VIEW_PROJECTION_MATRIX", setup.view_projection[k]);
                let [x, y, size] = setup.viewport[k];
                // (The far cascade without casters under one of its texels.)
                self.renderer.min_caster = (k == 1 && crate::world::far_min_on()).then(|| {
                    let (right, up, _) = crate::sun_shadow::axes(self.renderer.scene.sun.dir);
                    (right, up, crate::sun_shadow::texel(1))
                });
                let done = self.renderer.record(
                    enc,
                    &Targets {
                        colour: &sh.colour,
                        depth: dst,
                        clear: (k == 0 && !alone).then_some([0.0; 4]),
                        depth_clear: 1.0,
                        viewport: Some([x, y, size, size, 0.0, 1.0]),
                    },
                    &Frame {
                        code: &ck,
                        sun_shadow: false,
                        part: Part::Opaque,
                        target: Target::Shadow,
                        variant: 1,
                        cull: Some(&setup.planes[k]),
                        view: k as u32,
                        occlusion: None,
                        small_angle: None,
                    },
                );
                self.renderer.min_caster = None;
                self.code_pool.push(ck);
                done?;
                if let Some(p) = &mut self.profile {
                    let label = if k == 0 { "sun near" } else { "sun far" };
                    p.phase(label, self.renderer.stats);
                    p.mark(enc, label);
                }
            }
            // (With both cascades held and the same proxies as last frame, the
            // live atlas already is this frame's: no copy, no upload, no draws.
            // `BO2MP_IW6_CASTER_SKIP=0`: always.)
            let bits: Vec<u32> = if with_casters && cached && caster_skip_on() {
                self.casters[..self.caster_n]
                    .iter()
                    .flat_map(|c| [c.a[0], c.a[1], c.a[2], c.b[0], c.b[1], c.b[2], c.radius].map(f32::to_bits))
                    .collect()
            } else {
                Vec::new()
            };
            let same = !bits.is_empty() && self.caster_drawn.as_deref() == Some(&bits[..]);
            if !with_casters || !cached || bits.is_empty() {
                self.caster_drawn = None;
            }
            if same {
                if let Some(p) = &mut self.profile {
                    p.phase("sun casters cached", phase_since(std::time::Instant::now()));
                }
            } else if with_casters && let (Some((world, _)), Some(pass)) = (&sh.world, &self.caster_pass) {
                if !bits.is_empty() {
                    self.caster_drawn = Some(bits);
                }
                let size = wgpu::Extent3d { width: 2 * sh.res, height: 2 * sh.res, depth_or_array_layers: 1 };
                enc.copy_texture_to_texture(world.as_image_copy(), sh.live.as_image_copy(), size);
                pass.upload(&self.renderer.gpu.queue, camera.eye.to_array(), setup.view_projection, &self.casters[..self.caster_n]);
                for k in 0..2 {
                    pass.record(enc, &sh.depth, k, setup.viewport[k], self.caster_n);
                }
                if let Some(p) = &mut self.profile {
                    p.mark(enc, "sun casters");
                }
            }
            code.set_matrix("TRANSPOSE_SHADOW_LOOKUP_MATRIX", setup.lookup);
            code.set_vec("SHADOWMAP_SWITCH_PARTITION", setup.partition);
            code.set_vec("SHADOWMAP_SCALE", setup.scale);
            code.set_vec("SUN_SHADOWMAP_PIXEL_ADJUST", setup.pixel_adjust);
            true
        } else {
            false
        };
        // The spot lights' shadow maps, each into its tile of the atlas.
        self.renderer.spot_shadows.clear();
        if let Some(sp) = &self.spot {
            let forward = camera.rotation.row(2).truncate();
            let mut chosen = crate::spot_shadow::choose(&self.renderer.scene.lights, camera.eye, forward);
            chosen.truncate(usize::from(self.quality.spot_shadows));
            // Each light keeps its tile while it stays chosen (a new one takes
            // a tile no chosen light holds), so a tile's depth map is made
            // again only when its own light changed (`BO2MP_IW6_SPOT_TILES=0`:
            // all four together, on the whole view's key as before).
            let per_tile = spot_tiles_on() && shadow_cache_on();
            let mut slot_of: Vec<usize> = Vec::with_capacity(chosen.len());
            if per_tile {
                for &li in &chosen {
                    let held = self.spot_tiles.iter().position(|t| t.as_ref().is_some_and(|t| t.0 == li));
                    slot_of.push(held.unwrap_or(usize::MAX));
                }
                for i in 0..chosen.len() {
                    if slot_of[i] == usize::MAX {
                        // (A tile no chosen light holds, an empty one first.)
                        let free = (0..4)
                            .filter(|t| !slot_of.contains(t))
                            .min_by_key(|&t| self.spot_tiles[t].is_some());
                        slot_of[i] = free.unwrap_or(0);
                    }
                }
            } else {
                slot_of.extend(0..chosen.len());
            }
            let setups: Vec<crate::spot_shadow::SpotShadow> = chosen
                .iter()
                .zip(&slot_of)
                .map(|(&li, &tile)| crate::spot_shadow::setup(li, &self.renderer.scene.lights[usize::from(li)], camera.eye, tile, sp.res))
                .collect();
            // The same lights in the same tiles with the same projections, the
            // same view and nothing moved: last frame's tiles.
            let key = shadow_key(
                setups.iter().map(|s| s.view_projection).chain([camera.view_projection, camera.rotation]),
                self.renderer.moved + self.shadow_gen + chosen.iter().map(|&l| u64::from(l) << 20).sum::<u64>(),
            );
            let all_cached = !per_tile && shadow_cache_on() && self.spot_drawn.as_ref() == Some(&key);
            self.spot_drawn = Some(key);
            // Per tile: the light's own numbers (world space; the eye is not
            // in them) and the world's generation. The view is in the key only
            // with `BO2MP_IW6_SPOT_VIEW_KEY=1` (the few displacement casters
            // that read the camera then redraw the tile, as the exact frame).
            let dirty: Vec<bool> = if per_tile {
                chosen
                    .iter()
                    .zip(&slot_of)
                    .map(|(&li, &tile)| {
                        let l = &self.renderer.scene.lights[usize::from(li)];
                        let vals = [l.origin[0], l.origin[1], l.origin[2], l.dir[0], l.dir[1], l.dir[2], l.radius, l.cos_half_fov_outer];
                        let views = spot_view_key().then_some([camera.view_projection, camera.rotation]).into_iter().flatten();
                        let key = shadow_key_with(&vals, views, self.renderer.moved + self.shadow_gen + sp.res as u64);
                        let same = self.spot_tiles[tile].as_ref().is_some_and(|t| t.0 == li && t.1 == key);
                        self.spot_tiles[tile] = Some((li, key));
                        !same
                    })
                    .collect()
            } else {
                vec![!all_cached; chosen.len()]
            };
            let alone = per_tile && dirty.iter().any(|&d| !d);
            if alone && self.sun_clear.is_none() {
                self.sun_clear = Some(crate::sun_cache::Clear::new(&self.renderer.gpu.device));
            }
            let mut first = true;
            for (n, (&li, s)) in chosen.iter().zip(setups).enumerate() {
                if !dirty[n] {
                    self.renderer.spot_shadows.insert(li, s);
                    continue;
                }
                let tile_started = std::time::Instant::now();
                let k = slot_of[n];
                if alone && let Some(c) = &self.sun_clear {
                    c.record(enc, &sp.colour, &sp.depth, s.viewport);
                }
                let mut ck = self.code_pool.pop().unwrap_or_default();
                ck.clone_from(&code);
                ck.view_projection = s.view_projection;
                ck.set_matrix("TRANSPOSE_VIEW_PROJECTION_MATRIX", s.view_projection);
                let [x, y, size] = s.viewport;
                // (One list for every tile, each tile a view of it with its
                // own constants: all four in the frame's encoder.)
                let done = self.renderer.record(
                    enc,
                    &Targets {
                        colour: &sp.colour,
                        depth: &sp.depth,
                        clear: (first && !alone && !per_tile_partial(&dirty)).then_some([0.0; 4]),
                        depth_clear: 1.0,
                        viewport: Some([x, y, size, size, 0.0, 1.0]),
                    },
                    &Frame {
                        code: &ck,
                        sun_shadow: false,
                        part: Part::Opaque,
                        target: Target::SpotShadow,
                        variant: 3,
                        cull: Some(&s.planes),
                        view: k as u32,
                        occlusion: None,
                        small_angle: None,
                    },
                );
                first = false;
                self.code_pool.push(ck);
                done?;
                self.renderer.spot_shadows.insert(li, s);
                if let Some(p) = &mut self.profile {
                    let mut st = self.renderer.stats;
                    st.total = tile_started.elapsed().as_secs_f64() * 1000.0;
                    p.phase("spot", st);
                }
            }
            if let Some(p) = &mut self.profile {
                let kept = dirty.iter().filter(|&&d| !d).count() as u32;
                p.figure("spot tiles redrawn", (dirty.len() as u32 - kept) as f64);
                p.figure("spot tiles kept", f64::from(kept));
                if kept > 0 {
                    // (Tiles kept from earlier frames: the passes column.)
                    p.phase("spot kept", crate::profile::Phase { counts: crate::profile::Counts { passes: kept, ..Default::default() }, ..Default::default() });
                }
                p.mark(enc, "spot");
            }
        }
        let mut planes = self.plane_pool.pop().unwrap_or_default();
        frustum_planes_into(camera, &mut planes);
        // The cells seen through the portals from the camera's cell.
        let forward = camera.view_projection.row(3).truncate();
        self.renderer.update_cells(camera.eye, forward, camera.near, &planes);
        // IW6_DBG_NOCULL=1: every draw, no view culling (debug).
        static NOCULL: crate::switch::Switch = crate::switch::Switch::off("IW6_DBG_NOCULL");
        let culled = !NOCULL.get();
        let cull = culled.then_some(&planes[..]);
        // What the big opaque surfaces hide, and what is too small to show
        // (the scene's draws only; not with the view culling off).
        let t_occ = profile_on.then(std::time::Instant::now);
        let occlusion = if culled { self.renderer.occlusion(camera) } else { None };
        let small = if culled { crate::world::small_angle(self.quality.preset) } else { None };
        if let (Some(p), Some(t)) = (&mut self.profile, t_occ) {
            p.phase("occlusion", phase_since(t));
        }
        // The opaque world and models.
        self.renderer.record(
            enc,
            &Targets {
                colour: &t.scene,
                depth,
                clear: Some([0.0; 4]),
                depth_clear: 0.0,
                viewport,
            },
            &Frame {
                code: &code,
                sun_shadow: shadowed,
                part: Part::Opaque,
                target: Target::Scene,
                variant: 0,
                cull,
                view: 0,
                occlusion: occlusion.as_deref(),
                small_angle: small,
            },
        )?;
        if let Some(p) = &mut self.profile {
            p.phase("opaque", self.renderer.stats);
            p.mark(enc, "opaque");
        }
        self.frame = Some(FrameState {
            code,
            shadowed,
            planes,
            culled,
            viewport,
            camera: *camera,
            look: *look,
            width,
            height,
            started,
            stage: Stage::Scene,
            occlusion,
            small,
            depth: None,
        });
        Ok(())
    }

    /// The second stage: the depth (with whatever the host drew into it
    /// since `record_scene`) into `FLOATZ`, Ghosts' post-opaque fog and
    /// colorization from the scene target into the fogged target
    /// (`fogged_view`), then the blended world surfaces over it. A host's
    /// own blended things (effects) go into the fogged target after this.
    pub fn record_fog(
        &mut self,
        enc: &mut wgpu::CommandEncoder,
        depth: &wgpu::TextureView,
        depth_sampled: &wgpu::TextureView,
    ) -> Result<(), String> {
        let f = self.frame.take().ok_or("record_fog without record_scene")?;
        if f.stage != Stage::Scene {
            self.frame = Some(f);
            return Err("record_fog: the frame is past its fog".into());
        }
        let (width, height) = (f.width, f.height);
        let camera = &f.camera;
        let code = &f.code;
        let (mut fog_name, fog_ao, fog_ao_has) = self.fog_names();
        let t = self.sized.as_ref().ok_or("no targets")?;
        let fog_started = std::time::Instant::now();
        // The depth into FLOATZ, then the post-opaque fog.
        self.floatz.record(&self.renderer.gpu, enc, depth_sampled, &t.floatz);
        if let Some(p) = &mut self.profile {
            p.mark(enc, "floatz");
        }
        // Effect lights over the opaque scene, before the fog (the preset's
        // cap; nothing is recorded with none).
        let fx_n = self.fx_light_count.min(usize::from(self.quality.fx_lights));
        if fx_n > 0 && crate::fx_light::on() {
            let u = crate::fx_light::Uniforms::build(camera, f.look.depth_range, &self.fx_lights[..fx_n], crate::fx_light::gain());
            self.fx_light.record(&self.renderer.gpu, enc, &t.floatz, &t.scene, &u, (width, height));
            if let Some(p) = &mut self.profile {
                p.mark(enc, "fx lights");
            }
        }
        if let Some(p) = &mut self.profile {
            p.figure("fx lights", fx_n as f64);
            p.figure("scene lights", self.renderer.scene.lights.len() as f64);
        }
        let mut post_code = self.code_pool.pop().unwrap_or_default();
        post_code.clone_from(code);
        // (The fog pass dots it with the view-space ray.)
        let d = camera.rotation.transform_vector3(Vec3::from_slice(&self.renderer.scene.fog.sun_dir[..3]));
        post_code.set_vec("FOG_SUN_DIR", [d.x, d.y, d.z, 0.0]);
        // With the vision's colorization: the same pass's `_color` form
        // (fog, then the HDR colorization), `_color_altd` with an
        // alternate grade blended by distance (`BO2MP_IW6_COLORIZATION=0`:
        // fog only).
        if let Some(c) = self.colorization() {
            c.write(&mut post_code);
        }
        // The ambient occlusion (`ssao`): Ghosts' calc pass over `FLOATZ`,
        // its bilateral blur along y, then the post-opaque pass's AO form
        // (which blurs along x and darkens). Off: byte for byte the pass
        // without it.
        let mut ssao_view: Option<wgpu::TextureView> = None;
        if let Some(half) = self.ssao_half() {
            let want = crate::ssao::target_size(width, height, half);
            let gpu = self.renderer.gpu.clone();
            if let Some(s) = self.sized.as_mut()
                && s.ssao.as_ref().map(|t| t.size) != Some(want)
            {
                s.ssao = Some(crate::ssao::Targets::new(&gpu, want.0, want.1));
            }
            let comp = fog_ao;
            let t = self.sized.as_ref().ok_or("no targets")?;
            if let Some(ao) = t.ssao.as_ref().filter(|_| fog_ao_has) {
                let ssao_started = std::time::Instant::now();
                self.ssao.calc_consts(want.0, want.1, &mut post_code);
                let calc = [("RANDOM_ROTATIONS", &self.rotations.view), ("FLOATZ", &t.floatz)];
                let blur = [("SSAO", &ao.ao[0]), ("FLOATZ", &t.floatz)];
                let fmt = crate::ssao::FORMAT;
                let r = self
                    .post
                    .record(enc, crate::ssao::calc_technique(), &calc, &post_code, &ao.ao[0], fmt)
                    .and_then(|()| self.post.record(enc, "ssao_vblur_lq", &blur, &post_code, &ao.ao[1], fmt));
                match r {
                    Ok(()) => {
                        self.ssao.composite_consts(want.0, &mut post_code);
                        ssao_view = Some(ao.ao[1].clone());
                        fog_name = comp;
                    }
                    Err(e) => {
                        let msg = format!("ssao: {e} (no ambient occlusion)");
                        if !self.post_errors.contains(&msg) {
                            self.post_errors.push(msg);
                        }
                    }
                }
                if let Some(p) = &mut self.profile {
                    p.phase("ssao", phase_since(ssao_started));
                    p.mark(enc, "ssao");
                }
            }
        }
        let t = self.sized.as_ref().ok_or("no targets")?;
        let fog_tech: &str = &fog_name;
        let mut tex = [("RESOLVED_SCENE", &t.scene), ("FLOATZ", &t.floatz), ("SSAO", &t.scene)];
        let mut tex_n = 2;
        if let Some(v) = ssao_view.as_ref() {
            tex[2] = ("SSAO", v);
            tex_n = 3;
        }
        if let Err(e) = self.post.record(enc, fog_tech, &tex[..tex_n], &post_code, &t.fogged, SCENE_COLOUR) {
            let msg = format!("{fog_tech}: {e} (unfogged copy)");
            if !self.post_errors.contains(&msg) {
                self.post_errors.push(msg);
            }
            enc.copy_texture_to_texture(
                t.scene_texture.as_image_copy(),
                t.fogged_texture.as_image_copy(),
                wgpu::Extent3d {
                    width,
                    height,
                    depth_or_array_layers: 1,
                },
            );
        }
        if let Some(p) = &mut self.profile {
            p.phase("fog", phase_since(fog_started));
            p.mark(enc, "fog");
        }
        // The scene behind the distortion sprites: a copy of the fogged scene
        // (`RESOLVED_POST_SUN`), only while one is in view.
        if self.distort.visible > 0
            && let (Some(t), Some(copy)) = (self.sized.as_ref(), self.distort.copy_texture())
            && copy.width() == width
            && copy.height() == height
        {
            enc.copy_texture_to_texture(
                t.fogged_texture.as_image_copy(),
                copy.as_image_copy(),
                wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
            );
        }
        // The blended surfaces over the fogged scene.
        let t = self.sized.as_ref().ok_or("no targets")?;
        let cull = f.culled.then_some(&f.planes[..]);
        let result = self.renderer.record(
            enc,
            &Targets {
                colour: &t.fogged,
                depth,
                clear: None,
                depth_clear: 0.0,
                viewport: f.viewport,
            },
            &Frame {
                code,
                sun_shadow: f.shadowed,
                part: Part::Blended,
                target: Target::Scene,
                variant: 0,
                cull,
                view: 0,
                occlusion: f.occlusion.as_deref(),
                small_angle: f.small,
            },
        );
        if let Some(p) = &mut self.profile {
            p.phase("blended", self.renderer.stats);
            p.mark(enc, "blended");
        }
        self.frame = Some(FrameState { stage: Stage::Fogged, depth: Some(depth_sampled.clone()), ..f });
        self.code_pool.push(post_code);
        result
    }

    /// The last stage: material bloom over the fogged target, the iris,
    /// and Ghosts' final pass into `display`. After `record_fog`.
    pub fn record_post(
        &mut self,
        enc: &mut wgpu::CommandEncoder,
        display: &wgpu::TextureView,
        display_format: wgpu::TextureFormat,
    ) -> Result<(), String> {
        let f = self.frame.take().ok_or("record_post without record_scene")?;
        if f.stage != Stage::Fogged {
            self.frame = Some(f);
            return Err("record_post before record_fog".into());
        }
        // (The host's own draws between the stages (characters, effects) end
        // here: the time since the blended mark is theirs.)
        if let Some(p) = &mut self.profile {
            p.mark(enc, "host fx+chars");
        }
        let (width, height, started) = (f.width, f.height, f.started);
        let code = &f.code;
        let look = &f.look;
        let dof = self.dof_active();
        if dof.is_some() {
            let gpu = self.renderer.gpu.clone();
            if let Some(sz) = self.sized.as_mut() {
                sz.dof.get_or_insert_with(|| crate::dof::Targets::new(&gpu, width, height, SCENE_COLOUR));
            }
        }
        let t = self.sized.as_ref().ok_or("no targets")?;
        // Depth of field: the fogged scene into `scene` (free after the
        // fog), which the rest of the post then reads.
        let mut dof_done = false;
        if let (Some(d), Some(dt)) = (dof, t.dof.as_ref()) {
            let started = std::time::Instant::now();
            dof_done = Self::record_dof(&mut self.post, &mut self.post_errors, &mut self.code_pool, enc, &d, dt, t, code, f.camera.near, (width, height));
            if let Some(p) = &mut self.profile {
                p.phase("dof", phase_since(started));
                p.mark(enc, "dof");
            }
        }
        let mut main = if dof_done { &t.scene } else { &t.fogged };
        // Camera motion blur over what the depth of field left: the pass
        // writes into its scratch, which the rest of the post then reads.
        let mb_on = self.quality.motion_blur && crate::mblur::on();
        let hq_mode = crate::mblur::mode() == crate::mblur::Mode::Velocity && crate::mblur::Hq::available(&self.post);
        if mb_on && hq_mode {
            // Ghosts' own chain: velocity picture, tile and neighbour
            // maxima, the apply pass (an unmoved view draws nothing).
            let started = std::time::Instant::now();
            let gpu = self.renderer.gpu.clone();
            let hq = self.mb_hq.get_or_insert_with(|| crate::mblur::Hq::new(&gpu));
            let mut c = self.code_pool.pop().unwrap_or_default();
            if let Some(out) = hq.record(&gpu, &mut self.post, &mut self.post_errors, enc, main, &t.floatz, code, &mut c, &f.camera, (width, height), SCENE_COLOUR) {
                main = out;
            }
            self.code_pool.push(c);
            if let Some(p) = &mut self.profile {
                p.phase("mblur", phase_since(started));
                p.mark(enc, "mblur");
            }
        } else if let Some(hq) = self.mb_hq.as_mut() {
            hq.remember(f.camera.view_projection, f.camera.eye);
        }
        if mb_on && !hq_mode && let Some(depth) = f.depth.as_ref() {
            let started = std::time::Instant::now();
            let stale = self.mblur.as_ref().is_none_or(|(_, v)| v != main);
            if stale {
                self.mblur = Some((render_h1::screenfx::ScreenFx::new(&self.renderer.gpu), main.clone()));
            }
            if let Some((fx, _)) = self.mblur.as_mut() {
                fx.dt_override = Some(look.dt.unwrap_or(1.0 / 60.0).max(1e-4));
                // (A view that has not moved has no velocity to blur;
                // `BO2MP_IW6_MBLUR_STILL=1` draws the pass anyway.)
                static ALWAYS: crate::switch::Switch = crate::switch::Switch::off("BO2MP_IW6_MBLUR_STILL");
                fx.skip_still = !ALWAYS.get();
                let on = render_h1::screenfx::Enabled { ao: false, dof: false, motion_blur: true };
                let range = look.depth_range.unwrap_or((0.0, 1.0));
                if fx.record(enc, main, depth, &f.camera, range, (width, height), on, false)
                    && let Some(s) = fx.scratch_view()
                {
                    main = s;
                }
            }
            if let Some(p) = &mut self.profile {
                p.phase("mblur", phase_since(started));
                p.mark(enc, "mblur");
            }
        } else if let Some((fx, _)) = self.mblur.as_mut() {
            // (Off: the view is remembered so a switch on starts clean.)
            fx.skip(&f.camera);
        }
        let bloom_started = std::time::Instant::now();
        // Material bloom (`material_bloom_*`): the scene's glow by its
        // alpha (the bloom mask), blurred at half size along x then y,
        // added back over the scene (INFERRED: the target size, the two
        // blur passes and the additive blend; the passes' maths is
        // Ghosts').
        let (darken, lighten) = Self::sun_state(
            &mut self.sun_fx,
            &mut self.sun_probe,
            &mut self.sun_read,
            self.quality.preset != native_quality::Preset::Low,
            enc,
            &f.camera,
            look,
            (width, height),
            &t.floatz,
        );
        let intensity = self.renderer.scene.vision.f("r_materialBloomIntensity", 0.0);
        if bloom_on() && self.quality.bloom {
            let mut bc = self.code_pool.pop().unwrap_or_default();
            bc.clone_from(code);
            crate::look::bloom_consts(&self.renderer.scene.vision, (width, height), &mut bc);
            crate::sun_fx::glow_apply(&mut bc, darken, lighten, intensity);
            let steps: [(&str, &wgpu::TextureView, &wgpu::TextureView, &str); 3] = [
                ("material_bloom_setup_alpha_intensity", main, &t.bloom[0], "RESOLVED_SCENE"),
                ("material_bloom_hq_blur", &t.bloom[0], &t.bloom[1], "FEEDBACK"),
                ("material_bloom_hq_blur", &t.bloom[1], &t.bloom[0], "FEEDBACK"),
            ];
            let mut ok = true;
            for (k, (tech, src, dst, name)) in steps.iter().enumerate() {
                let tex = [(*name, *src)];
                // (The blur runs once along each axis.)
                if k > 0 {
                    let r = crate::look::bloom_radius(&self.renderer.scene.vision, t.bloom_size, k == 1);
                    bc.set_vec("BLOOM_HQ_RADIUS", r);
                }
                if let Err(e) = self.post.record(enc, tech, &tex, &bc, dst, SCENE_COLOUR) {
                    let msg = format!("{tech}: {e} (no bloom)");
                    if !self.post_errors.contains(&msg) {
                        self.post_errors.push(msg);
                    }
                    ok = false;
                    break;
                }
            }
            if ok {
                let tex = [("FEEDBACK", &t.bloom[0])];
                if let Err(e) = self.post.record_with(enc, "material_bloom_apply", &tex, &bc, main, SCENE_COLOUR, true) {
                    let msg = format!("material_bloom_apply: {e} (no bloom)");
                    if !self.post_errors.contains(&msg) {
                        self.post_errors.push(msg);
                    }
                }
            }
            self.code_pool.push(bc);
        }
        else if darken > 0.0 || lighten > 0.0 {
            // No bloom: the apply pass alone, with the glow's weight 0.
            let mut bc = self.code_pool.pop().unwrap_or_default();
            bc.clone_from(code);
            crate::look::bloom_consts(&self.renderer.scene.vision, (width, height), &mut bc);
            crate::sun_fx::glow_apply(&mut bc, darken, lighten, 0.0);
            let tex = [("FEEDBACK", &t.bloom[0])];
            if let Err(e) = self.post.record_with(enc, "material_bloom_apply", &tex, &bc, main, SCENE_COLOUR, true) {
                let msg = format!("material_bloom_apply: {e} (no sun blind)");
                if !self.post_errors.contains(&msg) {
                    self.post_errors.push(msg);
                }
            }
            self.code_pool.push(bc);
        }
        if let Some(p) = &mut self.profile {
            p.phase("bloom", phase_since(bloom_started));
            p.mark(enc, "bloom");
        }
        let iris_started = std::time::Instant::now();
        // The iris: offline it settles (a long step).
        let tone = &self.renderer.scene.tone;
        let dt = look.dt.unwrap_or(60.0);
        self.iris.record(
            &self.renderer.gpu,
            enc,
            main,
            (width, height),
            tone.exposure,
            tone.rates,
            dt,
            !tone.adaptive(),
        );
        if let Some(p) = &mut self.profile {
            p.phase("iris", phase_since(iris_started));
            p.mark(enc, "iris");
        }
        let final_started = std::time::Instant::now();
        // (The exposure for effects graded on the CPU, read back late.)
        self.readback.collect();
        self.readback.copy(enc, self.iris.texture());
        let iris = self.iris.view();
        let tex = [
            ("RESOLVED_SCENE", main),
            ("MACROCLUT", &self.macroclut.view),
            ("DYNAMIC_IRIS_EXPOSURE", &iris),
        ];
        if let Err(e) = self.post.record(enc, "postfx_macroclut", &tex, code, display, display_format) {
            self.note_post(format!("postfx_macroclut: {e}"));
            return Err(e);
        }
        let post_bytes = std::mem::take(&mut self.post.bytes);
        if let Some(p) = &mut self.profile {
            let mut f = phase_since(final_started);
            f.counts.bytes = post_bytes;
            p.phase("final", f);
            p.mark(enc, "final");
            p.end(enc, started.elapsed().as_secs_f64() * 1000.0);
        }
        self.frames += 1;
        self.code_pool.push(f.code);
        self.plane_pool.push(f.planes);
        Ok(())
    }

    /// The depth of field chain (`dof`): downsample, blur, near spread and
    /// the final pass from the fogged target into the scene target. False
    /// (and a note) when a pass fails: the frame goes on without it.
    #[allow(clippy::too_many_arguments)]
    fn record_dof(
        post: &mut Post,
        errors: &mut Vec<String>,
        pool: &mut Vec<CodeValues>,
        enc: &mut wgpu::CommandEncoder,
        d: &crate::dof::Dof,
        dt: &crate::dof::Targets,
        t: &Sized,
        code: &CodeValues,
        near: f32,
        size: (u32, u32),
    ) -> bool {
        let (w, h) = (size.0 as f32, size.1 as f32);
        let mut c = pool.pop().unwrap_or_default();
        c.clone_from(code);
        d.consts(near, h, crate::dof::BIAS, &mut c);
        c.set_vec("RENDER_TARGET_SIZE", [w, h, 1.0 / w, 1.0 / h]);
        let r = d.blur_radius(h).max(0.25);
        let (qw, qh) = (dt.size.0 as f32, dt.size.1 as f32);
        let mut run = |tech: &str, tex: &[(&str, &wgpu::TextureView)], c: &CodeValues, dst: &wgpu::TextureView| -> bool {
            match post.record(enc, tech, tex, c, dst, SCENE_COLOUR) {
                Ok(()) => true,
                Err(e) => {
                    let msg = format!("{tech}: {e} (no depth of field)");
                    if !errors.contains(&msg) {
                        errors.push(msg);
                    }
                    false
                }
            }
        };
        let mut bc = pool.pop().unwrap_or_default();
        let done = 'chain: {
            if !run("dof_downsample", &[("RESOLVED_SCENE", &t.fogged), ("FLOATZ", &t.floatz)], &c, &dt.down) {
                break 'chain false;
            }
            bc.clone_from(&c);
            bc.set_vec("BLOOM_HQ_RADIUS", [r / qw, 0.0, 0.0, 0.0]);
            if !run("material_bloom_hq_blur", &[("FEEDBACK", &dt.down)], &bc, &dt.scratch) {
                break 'chain false;
            }
            bc.set_vec("BLOOM_HQ_RADIUS", [0.0, r / qh, 0.0, 0.0]);
            if !run("material_bloom_hq_blur", &[("FEEDBACK", &dt.scratch)], &bc, &dt.blur) {
                break 'chain false;
            }
            if !run("dof_near_coc", &[("POST_EFFECT_0", &dt.blur), ("POST_EFFECT_1", &dt.down)], &c, &dt.near) {
                break 'chain false;
            }
            run(
                "postfx_dof",
                &[("RESOLVED_SCENE", &t.fogged), ("POST_EFFECT_1", &dt.near), ("POST_EFFECT_0", &dt.blur), ("FLOATZ", &t.floatz)],
                &c,
                &t.scene,
            )
        };
        pool.push(bc);
        pool.push(c);
        done
    }

    /// The sun's blind and glare for this frame: (darken, lighten), both 0
    /// with the effect off, on LOW, away from the sun or without the
    /// map's data. Runs the visibility pass when the sun is near the view
    /// and takes the pass's reading from a few frames back.
    #[allow(clippy::too_many_arguments)]
    fn sun_state(
        fx: &mut Option<crate::sun_fx::SunFx>,
        probe: &mut Option<crate::sun_fx::Probe>,
        read: &mut ExposureReadback,
        preset_on: bool,
        enc: &mut wgpu::CommandEncoder,
        camera: &Camera,
        look: &Look,
        size: (u32, u32),
        floatz: &wgpu::TextureView,
    ) -> (f32, f32) {
        let (Some(fx), Some(probe)) = (fx.as_mut(), probe.as_mut()) else { return (0.0, 0.0) };
        if !preset_on || !crate::sun_fx::on() {
            fx.blind = 0.0;
            fx.glare = 0.0;
            return (0.0, 0.0);
        }
        let forward = camera.rotation.inverse().transform_vector3(glam::Vec3::Z);
        let dot = fx.dot(forward);
        // (Offline there is no late read: `IW6_SHOT_SUN_VIS` is the visibility.)
        fx.forced = look.dt.is_none().then(|| {
            std::env::var("IW6_SHOT_SUN_VIS").ok().and_then(|v| v.trim().parse::<f32>().ok()).unwrap_or(0.0)
        });
        read.collect();
        if fx.near(dot) && fx.forced.is_none() {
            if let Some((c, r)) = fx.disc(&camera.view_projection, size) {
                let (lo, hi) = look.depth_range.unwrap_or((0.0, 1.0));
                let thresh = camera.near * (hi - lo).max(1e-6) / crate::sun_fx::FAR_DISTANCE;
                if read.is_idle() {
                    probe.record(enc, floatz, c, r, thresh);
                    read.copy(enc, &probe.target);
                }
            }
        }
        let secs = look.dt.map_or(1.0e4, |d| d.clamp(0.0, 1.0));
        let out = fx.update(dot, read.last.unwrap_or(0.0), secs * 1000.0);
        static DBG_SUN: crate::switch::Switch = crate::switch::Switch::off("IW6_DBG_SUN");
        if DBG_SUN.get() {
            eprintln!("iw6 sun fx: dot {dot:.3} near {} forced {:?} read {:?} -> darken {:.3} lighten {:.3}", fx.near(dot), fx.forced, read.last, out.0, out.1);
        }
        out
    }

    /// The frame's effect sprites, for the profile table
    /// (`BO2MP_IW6_PROFILE=1`); the host that built them calls it once a
    /// frame before `record_post`.
    pub fn profile_fx(&mut self, f: crate::profile::FxFigures) {
        if let Some(p) = &mut self.profile {
            p.fx(f);
        }
    }

    /// The profile's table when `every` seconds have passed (0: each
    /// frame); None with the profile off.
    pub fn profile_line(&mut self, every: f32) -> Option<String> {
        let p = self.profile.as_mut()?;
        p.collect();
        p.take_line(every)
    }

    fn note_post(&mut self, e: String) {
        if !self.post_errors.contains(&e) {
            self.post_errors.push(e);
        }
    }

    /// Which stage the current frame is at: `None` before `record_scene`
    /// and after `record_post`.
    pub fn stage(&self) -> Option<Stage> {
        self.frame.as_ref().map(|f| f.stage)
    }

    /// The HDR scene target (`SCENE_COLOUR`) of the frame being drawn: what
    /// `record_scene` drew into and `record_fog` reads.
    pub fn scene_view(&self) -> Option<&wgpu::TextureView> {
        self.sized.as_ref().map(|s| &s.scene)
    }

    /// The fogged HDR target (`SCENE_COLOUR`): what `record_fog` wrote and
    /// `record_post` reads.
    pub fn fogged_view(&self) -> Option<&wgpu::TextureView> {
        self.sized.as_ref().map(|s| &s.fogged)
    }

    /// The fogged HDR scene of the last frame (before the tone map).
    pub fn fogged(&self) -> Option<&wgpu::Texture> {
        self.sized.as_ref().map(|s| &s.fogged_texture)
    }

    /// The exposure texture (1 x 1, R16Float).
    pub fn exposure_texture(&self) -> &wgpu::Texture {
        self.iris.texture()
    }
}

/// `BO2MP_IW6_SHADOW_CACHE=0`: the shadow atlases are drawn every frame.
pub fn shadow_cache_on() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| !std::env::var("BO2MP_IW6_SHADOW_CACHE").is_ok_and(|v| v.trim() == "0"))
}

/// What a shadow atlas was drawn with: its views' projections, bit for bit,
/// and a count of what else changes it (objects moved, atlas made again,
/// which lights).
fn shadow_key(views: impl Iterator<Item = Mat4>, extra: u64) -> Vec<u32> {
    shadow_key_with(&[], views, extra)
}

/// `shadow_key` with `values` first (a cascade's texels in the world).
fn shadow_key_with(values: &[f32], views: impl Iterator<Item = Mat4>, extra: u64) -> Vec<u32> {
    let mut k: Vec<u32> = values.iter().map(|v| v.to_bits()).collect();
    k.extend(views.flat_map(|m| m.to_cols_array().map(f32::to_bits)));
    k.extend([extra as u32, (extra >> 32) as u32]);
    k
}

/// `BO2MP_IW6_SPOT_TILES=0`: the spot atlas is drawn whole or not at all.
fn spot_tiles_on() -> bool {
    static S: crate::switch::Switch = crate::switch::Switch::on("BO2MP_IW6_SPOT_TILES");
    S.get()
}

/// `BO2MP_IW6_SPOT_VIEW_KEY=1`: a tile's key holds the camera's projection and rotation.
fn spot_view_key() -> bool {
    static S: crate::switch::Switch = crate::switch::Switch::off("BO2MP_IW6_SPOT_VIEW_KEY");
    S.get()
}

/// Whether some tiles are kept (the drawn ones then clear their own tile).
fn per_tile_partial(dirty: &[bool]) -> bool {
    dirty.iter().any(|&d| !d)
}

/// A post step's CPU time as a profile phase.
fn phase_since(t: std::time::Instant) -> crate::profile::Phase {
    let ms = t.elapsed().as_secs_f64() * 1000.0;
    crate::profile::Phase {
        total: ms,
        encode: ms,
        counts: crate::profile::Counts {
            passes: 1,
            ..Default::default()
        },
        ..Default::default()
    }
}

/// bo2mp: a tone mapping, film and exposure as the CPU grade of effects.
pub fn grade_of(t: &crate::look::Tone, f: &crate::look::Film, exposure: f32) -> fx_iw4::iw6::grade::Grade {
    fx_iw4::iw6::grade::Grade {
        exposure,
        curve: std::array::from_fn(|c| [t.a[c], t.b[c], t.c[c], t.d[c], t.e[c], t.f[c]]),
        white_scale: t.white_scale(),
        film_on: f.on,
        film_base: f.base,
        film_delta: f.delta,
        film_quad: f.quad,
        film_bias: f.bias,
        film_desat_dark: f.desat_dark,
        film_desat: f.desat,
    }
}

/// Hable's groups as the final pass's constants: the curve
/// `(A x^2 + C B x + D E) / (A x^2 + B x + D F) - E / F` as the shader
/// evaluates it (read from its translation): `B x` once, the numerator
/// `LINEAR_ANGLE * (B x) + A x^2 + TOE_NUMERATOR`, the denominator
/// `A x^2 + B x + TOE_DENOMINATOR`, minus `TOE_ANGLE`, times the white
/// `TESSELLATION_PARMS` and `TESSELLATION_PARMS2` as the engine sets them
/// from its dvars (READ, `docs/exe_notes/IW6_SHADERS.md` 13): (factor,
/// LOD bias, height scale, hybrid) with the height scale times -1/256 when
/// `r_tessellationHeightAuto` is on (the shaders then divide the height by
/// the vertex's texture stretch, `|position.w|`); (subdomain limit, cutoff
/// distance less the falloff, 1 / falloff, eye scale), the cutoff pair 0
/// with tessellation off, as here: the techniques without a hull shader are
/// the ones the engine draws then. Defaults: factor 60, LOD bias 1, height
/// scale 1 (auto), hybrid 1, subdomain limit 3, eye scale 0.0625.
/// Off by default (`BO2MP_IW6_TESS_PARMS=1` turns them on): with them the
/// displaced ground moves by its height over the texture stretch, and
/// offline (the displacement pictures are in paks not staged in the cloud,
/// drawn as the mid grey fallback) parts of mp_frag's yard came apart, so
/// they wait for a check on the owner's install. Without: the neutral values
/// used before (height scale 1, no stretch division).
pub fn tessellation_consts() -> ([f32; 4], [f32; 4]) {
    // (Read once: the frame asks for it every time.)
    static MODE: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    let mode = MODE.get_or_init(|| std::env::var("BO2MP_IW6_TESS_PARMS").unwrap_or_default());
    if mode.trim() != "1" {
        // Default (2026-11-04): no displacement at all (height scale 0: the
        // no_tess vertex programs compute `s = w0 * |z|`, so every vertex
        // stays where the map put it and neighbouring ground surfaces
        // share their edges). The displaced ground opened black cracks
        // between surfaces (the owner's snow-map screenshot; mp_frag's
        // gravel at `--cam -193 1650 500 30 90`): the layer stream's `w0`
        // is not read reliably (INFERRED layouts). `BO2MP_IW6_TESS_PARMS=neutral`
        // is the old height scale 1.
        let z = if mode.trim() == "neutral" { 1.0 } else { 0.0 };
        return ([1.0, 0.0, z, 0.0], [0.0, 0.0, 0.0, 1.0]);
    }
    let (factor, lod_bias, height, hybrid) = (60.0, 1.0, 1.0f32, 1.0);
    let height_auto = true;
    let z = if height_auto { height * -0.00390625 } else { height };
    ([factor, lod_bias, z, hybrid], [3.0, 0.0, 0.0, 0.0625])
}

/// scale. So the constants are D E, D F, E / F, B, C and A.
pub fn tone_consts(t: &crate::look::Tone) -> Vec<(&'static str, [f32; 4])> {
    let m = |a: [f32; 3], b: [f32; 3]| [a[0] * b[0], a[1] * b[1], a[2] * b[2], 0.0];
    let ef = [t.e[0] / t.f[0], t.e[1] / t.f[1], t.e[2] / t.f[2], 0.0];
    let w = t.white_scale();
    vec![
        ("TONE_MAPPING_TOE_STRENGTH_TOE_NUMERATOR", m(t.d, t.e)),
        ("TONE_MAPPING_TOE_STRENGTH_TOE_DENOMINATOR", m(t.d, t.f)),
        ("TONE_MAPPING_TOE_ANGLE", ef),
        ("TONE_MAPPING_LINEAR_STRENGTH", [t.b[0], t.b[1], t.b[2], 0.0]),
        ("TONE_MAPPING_LINEAR_ANGLE", [t.c[0], t.c[1], t.c[2], 0.0]),
        ("TONE_MAPPING_TOE_SHOULDER_STRENGTH", [t.a[0], t.a[1], t.a[2], 0.0]),
        ("TONE_MAPPING_WHITE_POINT", [w[0], w[1], w[2], 0.0]),
    ]
}

/// The view's four side planes in world space (`n . p + w >= 0` inside).
pub fn frustum_planes(camera: &Camera) -> Vec<glam::Vec4> {
    let mut v = Vec::new();
    frustum_planes_into(camera, &mut v);
    v
}

/// `frustum_planes` into `out` (cleared first).
pub fn frustum_planes_into(camera: &Camera, out: &mut Vec<glam::Vec4>) {
    out.clear();
    let m = camera.view_projection;
    let (r0, r1, r3) = (m.row(0), m.row(1), m.row(3));
    out.extend([r3 + r0, r3 - r0, r3 + r1, r3 - r1].into_iter().map(|p| {
        let n = p.truncate();
        let len = n.length().max(1e-12);
        (n / len).extend((p.w - n.dot(camera.eye)) / len)
    }));
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The final pass's arithmetic (as translated) with `tone_consts`
    /// gives the asset's curve.
    #[test]
    fn the_effect_grade_follows_the_tone_curve_and_film() {
        let t = crate::look::Tone::published();
        let v = crate::look::Vision::parse("r_filmEnable 1\nr_filmContrast 1.2\nr_filmBrightness 0\nr_filmDarkTint 0.9 0.95 1.1\nr_filmLightTint 1 1 1\n");
        let f = crate::look::Film::from_vision(&v);
        let g = grade_of(&t, &f, 1.5);
        for x in [0.05f32, 0.18, 0.7, 3.0] {
            let want: [f32; 3] = std::array::from_fn(|c| {
                crate::look::linear_to_srgb((t.curve(x * 1.5, c) * t.white_scale()[c]).clamp(0.0, 1.0))
            });
            let want = f.apply(want);
            let got = g.apply([x; 3]);
            for c in 0..3 {
                assert!((got[c] - want[c]).abs() < 1e-4, "{x} {c}: {got:?} {want:?}");
            }
        }
    }

    #[test]
    fn tone_consts_reproduce_the_curve() {
        let mut v = [0f32; 26];
        let groups: [[f32; 3]; 8] = [[0.8; 3], [-2.0; 3], [10.0; 3], [3.0; 3], [0.5; 3], [1.0; 3], [16.0; 3], [0.4, 2.2, 1.1]];
        for (i, g) in groups.iter().enumerate() {
            v[3 * i..3 * i + 3].copy_from_slice(g);
        }
        let t = crate::look::Tone::from_asset(&fastfile_iw6::walk::ToneMappingRec { name: "t".into(), v });
        let c: std::collections::HashMap<&str, [f32; 4]> = tone_consts(&t).into_iter().collect();
        let g = |n: &str| c[&format!("TONE_MAPPING_{n}")[..]][0];
        for x in [0.1f32, 0.6, 1.5, 4.0] {
            let bx = x * g("LINEAR_STRENGTH");
            let a2 = x * x * g("TOE_SHOULDER_STRENGTH");
            let num = g("LINEAR_ANGLE") * bx + a2 + g("TOE_STRENGTH_TOE_NUMERATOR");
            let den = a2 + bx + g("TOE_STRENGTH_TOE_DENOMINATOR");
            let shader = (num / den - g("TOE_ANGLE")) * g("WHITE_POINT");
            let want = t.curve(x, 0) * t.white_scale()[0];
            assert!((shader - want).abs() < 1e-5, "{x}: {shader} {want}");
        }
    }
}
