//! A whole MWR frame recorded into a host's encoder, the way MWR draws
//! one: the opaque surfaces (depth pre-pass, then lit) into an HDR target
//! and the host's depth buffer, the post-opaque fog, the blended surfaces
//! (fogged in their own shaders) over the fogged scene, then MWR's tone map
//! and film into the host's display target.
//!
//! The host owns the device, the depth buffer (reversed infinite, any
//! depth format with a depth aspect that can be sampled) and the display
//! target; this owns the map and everything made for it.

use std::collections::HashMap;
use std::path::PathBuf;

use crate::code::{Camera, CodeValues, world_view};
use crate::gpu::Gpu;
use crate::look::{self, Settings};
use crate::post::{self, FloatZ, Post};
use crate::scene::Scene;
use crate::sun_shadow;
use crate::world::{Frame, Part, Renderer, Target, Targets, slot};

/// The technique slots MWR's lit pass tries without a sun shadow map:
/// LIT_DIR (the sun with the lightmap's baked visibility).
pub const LIT_SLOTS: [usize; 4] = [slot::LIT_DIR, slot::LIT, slot::EMISSIVE, slot::UNLIT];

/// With the sun shadow map: LIT_DIR_SHADOW (the sun with its cascaded
/// shadow map, faded to the baked visibility) first.
pub const LIT_SLOTS_SHADOW: [usize; 5] = [
    slot::LIT_DIR_SHADOW,
    slot::LIT_DIR,
    slot::LIT,
    slot::EMISSIVE,
    slot::UNLIT,
];

/// The sun shadow map: its depth atlas (cascades stacked in V), the
/// stand-in colour target of the caster pass, the translucent casters'
/// colour map, the settings, the world's shadow bounds (the depth range),
/// the gate and the cached tiles' bookkeeping.
struct SunShadow {
    depth: wgpu::TextureView,
    colour: wgpu::TextureView,
    trans: wgpu::TextureView,
    settings: sun_shadow::Settings,
    bounds: (glam::Vec3, glam::Vec3),
    gate: sun_shadow::Gate,
    /// Our one stand-in cell: visible, and sun-lit when some surface draws
    /// with the sun shadow (we keep no cells or portal visibility).
    sun_lit: Vec<u32>,
    /// Per-cascade caster culling (`BO2MP_H1_SUN_SHADOW_CULL=0`: every
    /// caster into every cascade).
    cull: bool,
    cache: std::sync::Mutex<sun_shadow::TileCache>,
}

impl SunShadow {
    fn new(renderer: &mut Renderer) -> Self {
        let settings = sun_shadow::Settings::from_lightset(&renderer.scene.lightset);
        let scene = &renderer.scene;
        let lit = scene.draws.iter().any(|d| {
            crate::world::pick(
                &scene.pool,
                &scene.materials[d.material].techset,
                &[slot::LIT_DIR_SHADOW],
            )
            .is_some()
        });
        let sun_lit = sun_shadow::pack_cell_bits([lit]);
        let gpu = &renderer.gpu;
        let make = |label: &str, format| {
            gpu.device.create_texture(&wgpu::TextureDescriptor {
                label: Some(label),
                size: wgpu::Extent3d {
                    width: settings.res,
                    height: settings.res * sun_shadow::ROWS,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                    | wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            })
        };
        let depth_t = make("h1_shadowmap_sun", crate::world::SHADOW_DEPTH);
        let colour = make("h1_shadowmap_colour", crate::world::SHADOW_COLOUR)
            .create_view(&Default::default());
        let depth = depth_t.create_view(&Default::default());
        // `SHADOWMAP_SUN`: the lit shaders' depth texture.
        renderer.code_textures.insert(
            "SHADOWMAP_SUN".into(),
            crate::gpu::Tex {
                view: depth.clone(),
                texture: depth_t,
                dim: wgpu::TextureViewDimension::D2,
            },
        );
        // `TRANS_SHADOWMAP_SUN`: the translucent casters' tint, as large
        // as the depth atlas (read), cleared to white (no tint; inferred).
        let trans_t = make("h1_trans_shadowmap_sun", crate::world::TRANS_SHADOW_COLOUR);
        let trans = trans_t.create_view(&Default::default());
        renderer.code_textures.insert(
            "TRANS_SHADOWMAP_SUN".into(),
            crate::gpu::Tex {
                view: trans.clone(),
                texture: trans_t,
                dim: wgpu::TextureViewDimension::D2,
            },
        );
        let bounds = sun_shadow::caster_bounds(&renderer.scene);
        let gate = sun_shadow::Gate::from_env();
        let cull = !std::env::var("BO2MP_H1_SUN_SHADOW_CULL").is_ok_and(|v| v == "0");
        renderer.scene.notes.push(format!(
            "h1 sun shadow: {settings:?}, casters within {bounds:?}, gate {gate:?} \
             (sun-lit surfaces: {lit}), per-cascade culling {cull}"
        ));
        Self {
            depth,
            colour,
            trans,
            settings,
            bounds,
            gate,
            sun_lit,
            cull,
            cache: std::sync::Mutex::new(sun_shadow::TileCache::new(
                9 * sun_shadow::MAX_CASCADES as usize,
            )),
        }
    }

    /// Whether this frame's main view draws the sun shadow (the gate over
    /// our one always-visible stand-in cell).
    fn drawn(&self, sun: &crate::code::SunLight) -> bool {
        let sun_on = sun.diffuse.max_element() > 0.0 || sun.specular.max_element() > 0.0;
        sun_shadow::view_has_sun_shadow(
            self.gate,
            sun_on,
            1,
            &sun_shadow::pack_cell_bits([true]),
            &self.sun_lit,
        )
    }
}

/// Per-size targets: the HDR scene, `FLOATZ`, the fogged scene, the tone
/// mapped scene.
struct Sized {
    width: u32,
    height: u32,
    scene: wgpu::TextureView,
    floatz: wgpu::TextureView,
    fogged: wgpu::TextureView,
    fogged_texture: wgpu::Texture,
    toned: wgpu::TextureView,
    /// The film pass's display values, for `present` (float: kept exact).
    film: wgpu::TextureView,
}

/// What a host sets per frame besides the camera.
#[derive(Clone, Copy, Debug)]
pub struct Look {
    /// The tone map's exposure (linear scale). With the map's filmic tone
    /// map (light set `tonemap` 2) it scales the light set's own exposure.
    pub exposure: f32,
    pub fog: bool,
    pub film: bool,
    /// The depth range the host's scene is drawn in (reversed: the game
    /// keeps the top band for the view model). None: 0..1.
    pub depth_range: Option<(f32, f32)>,
    /// The engine's automatic exposure (`meter`) instead of `exposure`.
    pub auto_exposure: bool,
}

impl Default for Look {
    fn default() -> Self {
        Self {
            exposure: 1.0,
            fog: true,
            film: true,
            depth_range: None,
            auto_exposure: false,
        }
    }
}

pub struct Host {
    pub renderer: Renderer,
    post: Post,
    floatz: FloatZ,
    sized: Option<Sized>,
    /// The hand-off into the host's target (`present`), for its format.
    present: Option<crate::present::Present>,
    /// The zones the map was loaded from.
    pub zones: Vec<PathBuf>,
    /// When the host began: the shaders' game time (`GAMETIME`) counts from it.
    started: std::time::Instant,
    /// The sun shadow map (`BO2MP_H1_SUN_SHADOW=0`: none, the baked look).
    shadow: Option<SunShadow>,
    /// The automatic exposure's luminance read back.
    meter: crate::meter::Meter,
    /// The bloom (veil) chain and its plan, for the current size
    /// (`BO2MP_H1_VEIL=1`).
    veil: Option<(crate::veil::Chain, Vec<crate::veil::Pass>)>,
    /// Set when a veil pass failed (a material or texture the zones do
    /// not hold): the veil stays off after that.
    veil_off: bool,
    /// The graphics quality applied (`native_quality`).
    pub quality: native_quality::Quality,
    /// How many local lights the last frame drew.
    pub local_drawn: usize,
    /// Frames recorded, and the last frame's scene draws and CPU time (ms).
    pub frames: u64,
    /// Diagnostic lines for the game's own log (drained by the caller).
    pub pending_log: Vec<String>,
    /// GPU time per phase (opt-in: `H1_PROFILE` / `BO2MP_H1_PROFILE`).
    pub timer: crate::gpu_timer::GpuTimer,
    timing: bool,
    pub last_draws: usize,
    pub last_cpu_ms: f32,
    /// The glass revision last applied (`glass::broken`).
    glass_rev: u64,
    /// Ambient occlusion, depth of field and motion blur.
    screenfx: crate::screenfx::ScreenFx,
}

impl Host {
    /// Loads the map (`zones[0]`, with the zones it draws on) for drawing
    /// with `gpu` against a depth buffer of `depth_format`.
    pub fn load(
        gpu: Gpu,
        zones: &[PathBuf],
        depth_format: wgpu::TextureFormat,
    ) -> Result<Self, String> {
        let scene = Scene::load(zones)?;
        Ok(Self::new(gpu, scene, depth_format, zones.to_vec()))
    }

    pub fn new(
        gpu: Gpu,
        scene: Scene,
        depth_format: wgpu::TextureFormat,
        zones: Vec<PathBuf>,
    ) -> Self {
        let gpu_for_fx = gpu.clone();
        let post = Post::new(gpu.clone(), scene.pool.clone());
        let floatz = FloatZ::new(&gpu);
        let meter = crate::meter::Meter::new(&gpu);
        let mut renderer = Renderer::new(gpu, scene, depth_format);
        let shadow =
            sun_shadow::Settings::enabled_from_env().then(|| SunShadow::new(&mut renderer));
        Self {
            renderer,
            post,
            floatz,
            sized: None,
            present: None,
            zones,
            started: std::time::Instant::now(),
            shadow,
            meter,
            veil: None,
            veil_off: false,
            quality: native_quality::current(),
            local_drawn: 0,
            frames: 0,
            pending_log: Vec::new(),
            timer: Default::default(),
            timing: crate::gpu_timer::wanted(),
            last_draws: 0,
            last_cpu_ms: 0.0,
            glass_rev: 0,
            screenfx: crate::screenfx::ScreenFx::new(&gpu_for_fx),
        }
    }

    /// Hides the glass panes the match has broken and shows the rest (the
    /// set comes from `glass::set_broken`). A hidden pane's object is
    /// collapsed to nothing, which the per-frame constants carry to the
    /// GPU. Cheap when nothing changed. Returns how many panes are hidden
    /// when the set changed.
    pub fn sync_glass(&mut self) -> Option<usize> {
        let (rev, ids) = crate::glass::broken();
        if rev == self.glass_rev {
            return None;
        }
        self.glass_rev = rev;
        let hidden: std::collections::HashSet<u32> = ids.into_iter().collect();
        let sc = &mut self.renderer.scene;
        let mut count = 0usize;
        for n in 0..sc.glass_objects.len() {
            let Some(o) = sc.glass_objects[n] else { continue };
            let Some(object) = sc.objects.get_mut(o as usize) else { continue };
            let gone = hidden.contains(&(n as u32));
            count += usize::from(gone);
            for (_, rows) in &mut object.rows {
                for (r, row) in rows.iter_mut().enumerate() {
                    *row = if gone {
                        [0.0; 4]
                    } else {
                        std::array::from_fn(|c| if c == r { 1.0 } else { 0.0 })
                    };
                }
            }
        }
        Some(count)
    }

    /// The one-line summary logged with the first frame of a map.
    pub fn fps_line(&self) -> String {
        let sc = &self.renderer.scene;
        format!(
            "mwr fps: {}; {} draws in the last pass, {} local lights of {} drawn, {} frame ms CPU; AO {}, DOF {}, motion blur {}",
            self.quality.preset.label(),
            self.last_draws,
            self.local_drawn,
            sc.local_lights.len(),
            self.last_cpu_ms,
            if crate::screenfx::Enabled::from_quality(&self.quality).ao { "on" } else { "off" },
            if crate::screenfx::Enabled::from_quality(&self.quality).dof { "on" } else { "off" },
            if crate::screenfx::Enabled::from_quality(&self.quality).motion_blur { "on" } else { "off" },
        )
    }

    pub fn constants_memory(&self) -> (usize, usize, usize) {
        self.renderer.constants_memory()
    }

    /// The latest measured log-average scene luminance (automatic exposure).
    pub fn measured_luminance(&self) -> Option<f32> {
        self.meter.luminance
    }

    pub fn scene(&self) -> &Scene {
        &self.renderer.scene
    }

    /// The frame's sun shadow cascades, when the pass is on and the gate
    /// lets the view draw it.
    pub fn sun_shadow(&self, camera: &Camera) -> Option<sun_shadow::Setup> {
        let s = self.shadow.as_ref()?;
        if !self.quality.sun_shadow || !s.drawn(&self.renderer.scene.sun) {
            return None;
        }
        Some(sun_shadow::Setup::new(
            s.settings,
            self.renderer.scene.sun.dir,
            sun_shadow::View::from_camera(camera),
            s.bounds,
        ))
    }

    /// The code values of a frame seen by `camera`.
    pub fn code(&self, camera: &Camera, look: &Look) -> CodeValues {
        let scene = &self.renderer.scene;
        let mut code = world_view(camera, &scene.sun);
        // The eye's stage sets the sky's and effects' HDR scale.
        code.set_vec(
            "HDR_STAGE_PARMS",
            crate::stage::hdr_stage_parms(
                scene.stages.radiometric_scale_at(camera.eye),
                crate::stage::StageMath::from_env(),
            ),
        );
        // (Water scrolls and flags and foliage wave by the game time.)
        // (`H1_GAMETIME=<seconds>`: a fixed time, for comparing pictures.)
        let time = std::env::var("H1_GAMETIME")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or_else(|| self.started.elapsed().as_secs_f32());
        crate::code::set_game_time(&mut code, time);
        let none = Settings::default();
        look::fog(if look.fog { &scene.fog } else { &none }, camera, &mut code);
        // The exe's fog builder (default; `BO2MP_H1_FOG=fitted`: the fitted).
        if look.fog && look::legacy_fog_on(&scene.fog) {
            // Legacy (distance) fog: the builder's other branch.
            look::fog_legacy(
                &scene.fog,
                camera,
                scene.stages.radiometric_scale_at(camera.eye),
                &mut code,
            );
        } else if look.fog && !std::env::var("BO2MP_H1_FOG").is_ok_and(|v| v == "fitted") {
            look::fog_exact(
                &scene.fog,
                camera,
                scene.stages.radiometric_scale_at(camera.eye),
                &mut code,
            );
        }
        // With the HDR film on, MWR grades in HDR (r_filmHDR*) and the LDR
        // film tints are not applied.
        let hdr_film = scene.vision.f("r_filmHDREnabled", 0.0) != 0.0;
        match (look.film, hdr_film) {
            (true, true) => look::film_hdr(&scene.vision, &mut code),
            (true, false) => look::film(&scene.vision, &mut code),
            _ => look::film(&none, &mut code),
        }
        if let Some(s) = self.sun_shadow(camera) {
            s.apply(&mut code);
        }
        code
    }

    /// Applies the shared graphics quality (`native_quality`): the sun shadow,
    /// the bloom and the screen effects take effect on the next frame; the
    /// picture size cap and anisotropy were read as the scene was made.
    /// Returns the log line.
    pub fn apply_quality(&mut self, q: native_quality::Quality) -> String {
        let reload = self.quality.needs_reload(&q);
        self.quality = q;
        let fx = crate::screenfx::Enabled::from_quality(&q);
        format!(
            "mwr quality: {}; draw distance {} in, anisotropy cap {}x, sun shadow {}, bloom {}, ambient occlusion {}, depth of field {}, motion blur {}{}",
            q.summary(),
            if q.draw_distance < 0.999 { format!("{:.0}", crate::screenfx::DRAW_DISTANCE_BASE * q.draw_distance) } else { "unlimited".to_owned() },
            crate::gpu::anisotropy_cap(),
            if q.sun_shadow && self.shadow.is_some() { "on" } else { "off" },
            if q.bloom { "on" } else { "off" },
            if fx.ao { "on" } else { "off" },
            if fx.dof { "on" } else { "off" },
            if fx.motion_blur { "on" } else { "off" },
            if reload { "; the texture size and anisotropy change waits for the next map" } else { "" }
        )
    }

    /// The eye and the farthest distance a draw may start at, from the shared
    /// quality's draw distance (None at 1.0).
    fn max_distance(&self, camera: &Camera) -> Option<(glam::Vec3, f32)> {
        let k = self.quality.draw_distance;
        (k < 0.999).then(|| (camera.eye, crate::screenfx::DRAW_DISTANCE_BASE * k.max(0.05)))
    }

    /// The eye and the LOD distance scale of the main view (None: first
    /// levels only, `BO2MP_H1_LODS=0`).
    fn lod_scale(&self, camera: &Camera) -> Option<(glam::Vec3, f32)> {
        Some((camera.eye, self.quality.lod_distance.max(0.05)))
    }

    fn sized(&mut self, width: u32, height: u32) {
        if self
            .sized
            .as_ref()
            .is_some_and(|s| s.width == width && s.height == height)
        {
            return;
        }
        let gpu = &self.renderer.gpu;
        let view = |label: &str, format| {
            post::target(gpu, label, width, height, format).create_view(&Default::default())
        };
        let fogged_texture = post::target(
            gpu,
            "h1_fogged",
            width,
            height,
            wgpu::TextureFormat::Rgba16Float,
        );
        self.veil = None;
        self.sized = Some(Sized {
            width,
            height,
            scene: view("h1_scene", wgpu::TextureFormat::Rgba16Float),
            floatz: view("h1_floatz", wgpu::TextureFormat::R32Float),
            fogged: fogged_texture.create_view(&Default::default()),
            fogged_texture,
            toned: view("h1_toned", wgpu::TextureFormat::Rgba16Float),
            film: view("h1_film", wgpu::TextureFormat::Rgba16Float),
        });
    }

    /// Records the frame. `depth`: the host's depth buffer (attachment
    /// view); `depth_sampled`: a depth-only view of it; `display`: where
    /// the film pass writes display (sRGB-encoded) values. The depth is
    /// cleared and left holding the world's.
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
        let prof = std::env::var("H1_PROFILE").is_ok();
        let t0 = std::time::Instant::now();
        self.frames += 1;
        self.renderer.report.frame = crate::gpu_timer::FrameCounters::default();
        self.sized(width, height);
        if crate::present::enabled() && self.present.as_ref().is_none_or(|p| p.format() != display_format) {
            self.present = Some(crate::present::Present::new(&self.renderer.gpu, display_format));
            self.pending_log.push(format!(
                "h1 present: host target {display_format:?}, film output decoded: {}",
                crate::present::decodes(display_format)
            ));
        }
        let code = self.code(camera, look);
        if self.timing {
            let gpu = self.renderer.gpu.clone();
            self.timer.begin(&gpu, enc);
        }
        let t1 = std::time::Instant::now();
        // The sun shadow map first: each cascade's casters drawn with their
        // shadow technique into its band of the atlas, culled by the
        // cascade's planes; then the translucent casters' tint.
        let setup = self.sun_shadow(camera);
        if prof {
            eprintln!(
                "host: code {:.2} ms, shadow setup {:.2} ms",
                (t1 - t0).as_secs_f32() * 1e3,
                t1.elapsed().as_secs_f32() * 1e3
            );
        }
        if let (Some(s), Some(sh)) = (&setup, &self.shadow) {
            if sh.settings.cache != sun_shadow::CacheMode::Disabled {
                // The cached tiles' bookkeeping (which world-anchored tiles
                // the engine would draw this frame). The tiles themselves
                // are not drawn: every caster still goes into each cascade.
                let keys = s.tile_keys();
                let force = sh.settings.cache == sun_shadow::CacheMode::Force;
                let redraw = sh.cache.lock().map_or(0, |mut c| c.update(&keys, force).len());
                if std::env::var("H1_PROFILE").is_ok() {
                    eprintln!("sun shadow tiles: {} needed, {redraw} to draw", keys.len());
                }
            }
            // No translucent caster: no tint (white; inferred).
            enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("h1_trans_shadow_clear"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &sh.trans,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::WHITE),
                        store: wgpu::StoreOp::Store,
                    },
                    depth_slice: None,
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            for k in 0..s.cascades() {
                let planes = if sh.cull {
                    Some(s.caster_planes(k))
                } else {
                    None
                };
                let mut c = code.clone();
                let vp = s.view_projection(k);
                c.view_projection = vp;
                c.set_matrix("TRANSPOSE_VIEW_PROJECTION_MATRIX", vp);
                c.set_matrix(
                    "TRANSPOSE_WORLD_VIEW_PROJECTION_MATRIX0",
                    vp * glam::Mat4::from_translation(-camera.eye),
                );
                let [x, y, w, h] = sun_shadow::viewport(sh.settings.res, k);
                self.renderer.record(
                    enc,
                    &Targets {
                        colour: &sh.colour,
                        depth: &sh.depth,
                        clear: (k == 0).then_some([0.0; 4]),
                        // The shadow depth is reversed (the sun side 1).
                        depth_clear: 0.0,
                        viewport: Some([x, y, w, h, 0.0, 1.0]),
                    },
                    &Frame {
                        code: &c,
                        slots: &[slot::SHADOW_CASTER],
                        prepass_slots: &[],
                        part: Part::Opaque,
                        target: Target::Shadow,
                        variant: k as u8,
                        cull: planes.as_deref(),
                        max_distance: None,
                        need_bounds: false,
                        lod: None,
                        occlusion: None,
                        small_angle: None,
                    },
                )?;
                // The translucent casters tint `TRANS_SHADOWMAP_SUN` over
                // the band, tested against the opaque casters' depth.
                self.renderer.record(
                    enc,
                    &Targets {
                        colour: &sh.trans,
                        depth: &sh.depth,
                        clear: None,
                        depth_clear: 0.0,
                        viewport: Some([x, y, w, h, 0.0, 1.0]),
                    },
                    &Frame {
                        code: &c,
                        slots: &[slot::TRANS_SHADOW_CASTER],
                        prepass_slots: &[],
                        part: Part::All,
                        target: Target::TransShadow,
                        variant: k as u8,
                        cull: planes.as_deref(),
                        max_distance: None,
                        need_bounds: false,
                        lod: None,
                        occlusion: None,
                        small_angle: None,
                    },
                )?;
            }
        }
        if self.timing {
            self.timer.mark(enc, "shadow");
        }
        let Some(t) = self.sized.as_ref() else {
            return Ok(());
        };
        // Draws wholly outside the view are left out, as the engine culls
        // by its cells and bounds (`H1_NO_VIEW_CULL=1`: every draw).
        let view_planes =
            (std::env::var("H1_NO_VIEW_CULL").is_err()).then(|| frustum_planes(camera));
        let t_occ = std::time::Instant::now();
        let occlusion = if crate::world::occlusion_on() {
            self.renderer.occlusion(camera)
        } else {
            None
        };
        if std::env::var("H1_PROFILE").is_ok() {
            eprintln!("occlusion buffer {:.2} ms", t_occ.elapsed().as_secs_f32() * 1e3);
        }
        let frame = Frame {
            code: &code,
            slots: if setup.is_some() {
                &LIT_SLOTS_SHADOW
            } else {
                &LIT_SLOTS
            },
            prepass_slots: if std::env::var("H1_DBG_NOPREPASS").is_ok() {
                &[]
            } else {
                &[slot::ZPREPASS]
            },
            part: Part::Opaque,
            target: Target::Scene,
            variant: 0,
            cull: view_planes.as_deref(),
            max_distance: self.max_distance(camera),
            need_bounds: false,
            lod: self.lod_scale(camera),
            occlusion: occlusion.as_ref(),
            small_angle: crate::world::small_angle(self.quality.preset).map(|a| (camera.eye, a)),
        };
        self.renderer.record(
            enc,
            &Targets {
                colour: &t.scene,
                depth,
                clear: Some([0.0, 0.0, 0.0, 1.0]),
                depth_clear: 0.0,
                viewport: look
                    .depth_range
                    .map(|(a, b)| [0.0, 0.0, width as f32, height as f32, a, b]),
            },
            &frame,
        )?;
        if self.timing {
            self.timer.mark(enc, "opaque");
        }
        // The spot and omni lights: for each picked light, the draws inside
        // its sphere drawn again with the material's own `l_spot` / `l_omni`
        // technique over the lit scene (before the fog, as the lit passes
        // are). The pass's code values carry the light in slot 0.
        let scales = crate::light::ColorScales::read(&self.renderer.scene.lightset, &self.renderer.scene.vision);
        let picks = crate::local_light::pick(
            &self.renderer.scene.local_lights,
            camera.eye,
            view_planes.as_deref(),
            crate::local_light::frame_cap(self.quality.local_lights),
        );
        self.local_drawn = picks.len();
        for (n, &i) in picks.iter().enumerate() {
            let l = self.renderer.scene.local_lights[i].clone();
            let Some(ll) = crate::local_light::LocalLight::new(&l, camera.eye, scales) else { continue };
            let mut c = code.clone();
            ll.write(0, &mut c);
            let planes = crate::local_light::box_planes(l.origin, l.radius);
            let slots: &[usize] = if l.ty == 2 { &[slot::LIGHT_SPOT] } else { &[slot::LIGHT_OMNI] };
            self.renderer.record(
                enc,
                &Targets {
                    colour: &t.scene,
                    depth,
                    clear: None,
                    depth_clear: 0.0,
                    viewport: look
                        .depth_range
                        .map(|(a, b)| [0.0, 0.0, width as f32, height as f32, a, b]),
                },
                &Frame {
                    code: &c,
                    slots,
                    prepass_slots: &[],
                    // (The light techniques blend additively, so the
                    // draw list files them under the blended part.)
                    part: Part::All,
                    target: Target::Scene,
                    variant: crate::local_light::FIRST_VARIANT + n as u8,
                    cull: Some(&planes),
                    max_distance: self.max_distance(camera),
                    need_bounds: true,
                    lod: self.lod_scale(camera),
                    occlusion: occlusion.as_ref(),
                    small_angle: crate::world::small_angle(self.quality.preset).map(|a| (camera.eye, a)),
                },
            )?;
        }
        if self.timing {
            self.timer.mark(enc, "lights");
        }
        self.floatz
            .record(&self.renderer.gpu, enc, depth_sampled, &t.floatz);
        // Full-screen passes take the clip position as given.
        let mut post_code = code.clone();
        post_code.set_matrix(
            "TRANSPOSE_WORLD_VIEW_PROJECTION_MATRIX0",
            glam::Mat4::IDENTITY,
        );
        post_code.set_vec("TEXCOORD_CLAMP_0", [0.0, 0.0, 1.0, 1.0]);
        let hdr = wgpu::TextureFormat::Rgba16Float;
        let tex: HashMap<&str, &wgpu::TextureView> =
            [("RESOLVED_SCENE", &t.scene), ("FLOATZ", &t.floatz)].into();
        // The exe's pick of the post-opaque material (fog with or without
        // height fog and sky falloff); with no fog it draws none, which is
        // the height-fog pass at zero opacity here (it copies the scene).
        // `BO2MP_H1_POST_OPAQUE=hf`: always the height-fog pass (before).
        let none = Settings::default();
        let fog = if look.fog { &self.renderer.scene.fog } else { &none };
        let post_opaque = look::post_opaque_technique(fog, 0)
            .filter(|_| !std::env::var("BO2MP_H1_POST_OPAQUE").is_ok_and(|v| v == "hf"))
            .unwrap_or_else(|| "postopaquefx_fog_hf".into());
        self.post.record(
            enc,
            &post_opaque,
            &tex,
            &post_code,
            &[],
            &t.fogged,
            hdr,
        )?;
        let frame = Frame {
            part: Part::Blended,
            ..frame
        };
        self.renderer.record(
            enc,
            &Targets {
                colour: &t.fogged,
                depth,
                clear: None,
                depth_clear: 0.0,
                viewport: look
                    .depth_range
                    .map(|(a, b)| [0.0, 0.0, width as f32, height as f32, a, b]),
            },
            &frame,
        )?;
        if self.timing {
            self.timer.mark(enc, "fog_blended");
        }
        let on = crate::screenfx::Enabled::from_quality(&self.quality);
        let in_scratch = self.screenfx.record(
            enc,
            &t.fogged,
            depth_sampled,
            camera,
            look.depth_range.unwrap_or((0.0, 1.0)),
            (width, height),
            on,
            crate::tonemap::is_auto(&self.renderer.scene.lightset),
        );
        let scene_view: &wgpu::TextureView = match (in_scratch, self.screenfx.scratch_view()) {
            (true, Some(v)) => v,
            _ => &t.fogged,
        };
        if self.timing {
            self.timer.mark(enc, "screenfx");
        }
        let exposure = if look.auto_exposure {
            let gpu = self.renderer.gpu.clone();
            self.meter.record(&gpu, enc, scene_view, width, height);
            self.meter.exposure(&self.renderer.scene.lightset)
        } else {
            look.exposure
        };
        let look = &Look { exposure, ..*look };
        // Once the automatic exposure has settled (a few seconds in), one
        // line of what drives the map's brightness, for reports from maps
        // that cannot be rendered offline.
        if self.frames == 240 {
            let sc = &self.renderer.scene;
            let line = format!(
                "mwr look: {}: tone map {}, {} exposure, scene luminance {}, exposure x{:.3}, key boost x{:.2}, shade x{:.2}, diffuse scale {:.2}, sun {:?}",
                sc.name,
                sc.lightset.f("tonemap", -1.0),
                if look.auto_exposure { "automatic" } else { "fixed" },
                self.meter.luminance.map_or("n/a".into(), |l| format!("{l:.4}")),
                exposure,
                crate::tonemap::key_boost_in_use(&sc.lightset),
                crate::light::secondary_scale(&sc.lightset, &sc.vision),
                crate::light::ColorScales::read(&sc.lightset, &sc.vision).diffuse,
                sc.sun.diffuse,
            );
            eprintln!("{line}");
            self.pending_log.push(line);
        }
        if self.frames == 240 {
            let r = &self.renderer.report;
            let first: Vec<String> = r
                .missing_images
                .iter()
                .take(24)
                .map(|(k, e)| format!("{k} ({e})"))
                .collect();
            let line = format!(
                "mwr pictures: {} not loaded {first:?}; draws left out: {:?}",
                r.missing_images.len(),
                r.skipped
            );
            eprintln!("{line}");
            self.pending_log.push(line);
            // Placed models (props): what lights them and how they draw,
            // for reports of washed-out or see-through props.
            let p = &self.renderer.report.props;
            let blended: Vec<String> = p
                .blended
                .iter()
                .take(16)
                .map(|(m, (n, dw, b))| format!("{m} x{n} (depth write {dw}, blend {b})"))
                .collect();
            let defaulted: Vec<String> = p.defaulted.iter().take(16).map(|(m, n)| format!("{m} x{n}")).collect();
            let lines = [
                self.renderer.scene.props_line.clone(),
                format!(
                    "bo2mp h1 props: {} opaque model draws, {} blended model draws over {} materials {blended:?}; model draws with a picture not loaded (drawn grey; opaque passes solid, blended alpha 0) {defaulted:?}; opaque draws with no depth pre-pass writing their own depth {} (BO2MP_H1_OWN_DEPTH=0 leaves them without)",
                    p.opaque,
                    p.blended.values().map(|b| b.0).sum::<usize>(),
                    p.blended.len(),
                    self.renderer.report.own_depth,
                ),
            ];
            for line in lines {
                eprintln!("{line}");
                self.pending_log.push(line);
            }
        }
        let mut tex: HashMap<&str, &wgpu::TextureView> = [("RESOLVED_SCENE", scene_view)].into();
        let filmic = self.renderer.scene.lightset.f("tonemap", 0.0) == 2.0;
        // The bloom (veil): the chain over the fogged scene, then the
        // veil colour pass in place of the plain tone map.
        let mut veil_pass = false;
        if self.quality.bloom
            && crate::veil::enabled(self.renderer.scene.lightset.f("veil", 0.0) == 1.0)
            && !self.veil_off
        {
            if self.veil.is_none() {
                let env_i = |k: &str| {
                    codlegacy_env::var(k)
                        .ok()
                        .and_then(|v| v.parse::<i32>().ok())
                };
                let mut vs = crate::veil::Settings::default();
                // Test switches: the first-pass kind (0 linear, 1 log, 2
                // karis), the mode, the gaussian size.
                if let Some(i) = env_i("BO2MP_H1_VEIL_KIND") {
                    vs.downsample = crate::veil::Downsample::from_index(i);
                }
                if let Some(i) = env_i("BO2MP_H1_VEIL_MODE") {
                    vs.mode = crate::veil::Mode::from_index(i);
                }
                if let Some(i) = env_i("BO2MP_H1_VEIL_QUALITY") {
                    vs.quality = crate::veil::Quality::from_index(i);
                }
                let mut plan = crate::veil::plan(
                    &vs,
                    width,
                    height,
                    1.0,
                    8,
                );
                // `BO2MP_H1_VEIL_STOP=n`: only the first n passes (a test:
                // the colour pass then shows that pass's target).
                if let Some(n) = env_i("BO2MP_H1_VEIL_STOP") {
                    plan.truncate(n.max(1) as usize);
                }
                let chain = crate::veil::Chain::new(&self.renderer.gpu, width, height, &plan);
                self.renderer.scene.notes.push(format!(
                    "h1 veil: {} passes at {width}x{height}",
                    plan.len()
                ));
                self.veil = Some((chain, plan));
            }
            // The veil colour pass's mix: `veil * x + scene * y` (READ from
            // the zone's `tonemap_veil_*` shaders: `PERCEPTUAL_PARMS.xy`),
            // the light set's `veilstrength` and `veilbackgroundstrength`
            // (INFERRED defaults 0 and 1: no veil, the scene as it is).
            // `TEXCOORD_CLAMP_1` clamps the veil texture's coordinates
            // (the colour pass reads it for `FEEDBACK`): the whole picture.
            post_code.set_vec("TEXCOORD_CLAMP_1", [0.0, 0.0, 1.0, 1.0]);
            let ls = &self.renderer.scene.lightset;
            post_code.set_vec(
                "PERCEPTUAL_PARMS",
                [
                    // `BO2MP_H1_VEIL_STRENGTH`: a test value (the maps' own
                    // light sets carry the real ones).
                    std::env::var("BO2MP_H1_VEIL_STRENGTH")
                        .ok()
                        .and_then(|v| v.parse().ok())
                        .unwrap_or_else(|| ls.f("veilstrength", 0.0)),
                    std::env::var("BO2MP_H1_VEIL_BG")
                        .ok()
                        .and_then(|v| v.parse().ok())
                        .unwrap_or_else(|| ls.f("veilbackgroundstrength", 1.0)),
                    0.0,
                    0.0,
                ],
            );
            if let Some((chain, plan)) = &self.veil {
                match chain.record(&mut self.post, enc, scene_view, &post_code, plan) {
                    Ok(()) => {
                        if let Some(v) = chain.result() {
                            tex.insert("FEEDBACK", v);
                            veil_pass = true;
                        }
                    }
                    Err(e) => {
                        eprintln!("h1 veil: {e}; drawn without");
                        self.veil_off = true;
                    }
                }
            }
        }
        let (technique, buffer) = if filmic {
            (
                "tonemap_filmic",
                crate::tonemap::filmic_buffer_for(
                    crate::tonemap::ToneMath::from_env(),
                    &self.renderer.scene.lightset,
                    look.exposure,
                    look.auto_exposure,
                ),
            )
        } else {
            ("tonemap_linear", look::tonemap_buffer(look.exposure, 1.0))
        };
        let tone = [(4, buffer)];
        let veil_technique = crate::veil::tonemap_technique(filmic);
        let done = veil_pass
            && match self
                .post
                .record(enc, veil_technique, &tex, &post_code, &tone, &t.toned, hdr)
            {
                Ok(()) => true,
                Err(e) => {
                    eprintln!("h1 veil: {e}; plain tone map");
                    false
                }
            };
        if !done {
            self.post
                .record(enc, technique, &tex, &post_code, &tone, &t.toned, hdr)?;
        }
        let tex: HashMap<&str, &wgpu::TextureView> = [("RESOLVED_SCENE", &t.toned)].into();
        match self.present.as_ref().filter(|_| crate::present::enabled()) {
            // The film encodes to sRGB itself (read): into its own float
            // texture, then decoded into the host's target.
            Some(present) => {
                self.post.record(
                    enc,
                    "postfx_color2",
                    &tex,
                    &post_code,
                    &[],
                    &t.film,
                    wgpu::TextureFormat::Rgba16Float,
                )?;
                present.record(&self.renderer.gpu, enc, &t.film, display);
            }
            None => self.post.record(
                enc,
                "postfx_color2",
                &tex,
                &post_code,
                &[],
                display,
                display_format,
            )?,
        }
        if self.timing {
            self.timer.mark(enc, "tonemap_film");
            self.timer.end(enc);
            let mut c = self.renderer.report.frame;
            c.lights = Some(self.local_drawn as u64);
            self.timer.count(&c);
            if let Some(l) = self.timer.take_line(std::env::var("H1_PROFILE_EVERY").ok().and_then(|v| v.parse().ok()).unwrap_or(5.0)) {
                eprintln!("{l}");
            }
        }
        self.last_cpu_ms = (t0.elapsed().as_secs_f32() * 1000.0 * 100.0).round() / 100.0;
        self.last_draws = self.renderer.report.drawn;
        Ok(())
    }

    /// The fogged HDR scene of the last frame (Rgba16Float), before the
    /// tone map: what exposure is measured on.
    pub fn fogged(&self) -> Option<&wgpu::Texture> {
        self.sized.as_ref().map(|s| &s.fogged_texture)
    }
}

/// The view's four side planes in world space (`n . p + w >= 0` inside),
/// from the eye-relative clip matrix (reversed infinite depth: no near or
/// far plane is needed).
fn frustum_planes(camera: &Camera) -> Vec<glam::Vec4> {
    let m = camera.view_projection;
    let (r0, r1, r3) = (m.row(0), m.row(1), m.row(3));
    [r3 + r0, r3 - r0, r3 + r1, r3 - r1]
        .into_iter()
        .map(|p| {
            let n = p.truncate();
            let len = n.length().max(1e-12);
            // Eye-relative to world: n . (p - eye) + w.
            (n / len).extend((p.w - n.dot(camera.eye)) / len)
        })
        .collect()
}
