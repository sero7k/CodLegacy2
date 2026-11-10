//! What a game's renderer holds to draw a Black Ops III map every frame:
//! the scene's GPU side and the frame's render targets, recorded into the
//! host's encoder.

use std::sync::Arc;

use render_h1::code::Camera;
use render_h1::gpu::Gpu;

use crate::gbuffer::{GBuffer, Targets};
use crate::lighting::Lighting;
use crate::post_run::Post;
use crate::scene::Scene;

/// What the load thread makes: the lighting stage and post chain, and the
/// pipelines and pictures of the passes ([`crate::warm`]).
type Stages = (Result<(Lighting, Result<Post, String>), String>, crate::warm::Warm);
type LightingRx = std::sync::Mutex<std::sync::mpsc::Receiver<Stages>>;

/// bo2mp: `BO2MP_T7_HEAP_TRIM=1` hands the allocator's freed pages back to
/// the system when a BO3 map's load ends: on the load thread before it exits,
/// and on the main thread once the map is ready. A thread that exits leaves
/// its pages with the allocator, so what the load let go of could stay in
/// the working set through the match. Off by default (the old behaviour)
/// until a counting-alloc run shows the heap well below the working set.
pub fn heap_trim() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var_os("BO2MP_T7_HEAP_TRIM").is_some_and(|v| v == "1"))
}

pub struct Host {
    gpu: Gpu,
    pub gbuffer: GBuffer,
    pub targets: Option<Targets>,
    /// Lines for the log, taken once with [`Host::take_report`].
    report: Vec<String>,
    frames: u64,
    /// The lighting stage, made on a thread of its own (68 programs) and
    /// taken when ready; its frames recorded after it is bound.
    lighting: Option<Lighting>,
    /// The post chain (made with the lighting stage): exposure, LUT,
    /// bloom, the final resample.
    post: Option<Post>,
    lighting_rx: Option<LightingRx>,
    lit_frames: u64,
    /// The lighting stage was just taken (bound on the next frame).
    fresh: bool,
    /// The non-G-buffer surfaces (technique slot 6), drawn into the lit
    /// scene after the lighting, before the post chain, as the exe's frame
    /// does; made and bound with the lighting stage.
    forward: Option<crate::forward::Forward>,
    started: std::time::Instant,
    /// The `t7 coverage:` audit has run.
    coverage_done: bool,
    /// bo2mp: the local player's ADS amount (`ps.fWeaponPosFrac`, 0 while
    /// a scope overlay is up) for the ADS blur, and the frame depth's
    /// depth-aspect view it reads (made with the targets).
    ads_amount: f32,
    depth_only: Option<wgpu::TextureView>,
    /// The shared graphics setting's revision last logged
    /// (`crate::quality`).
    quality_rev: Option<u64>,
}

/// The G-buffer and the lighting stage are recorded every frame and the
/// frame handed to the screen (the post chain's output once it is in;
/// until then the lit scene as it is, before exposure). Off: the G-buffer
/// and lighting are recorded once, to check them on the device.
pub const LIGHTING_BUILT: bool = true;

/// `BO2MP_T7_NATIVE_DEBUG=gbuffer`: the G-buffer's diffuse target is
/// handed to the screen as the image (to check the hand-off before the
/// lighting and post chain are built). Never on by default.
pub fn debug_gbuffer() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var("BO2MP_T7_NATIVE_DEBUG").is_ok_and(|v| v.eq_ignore_ascii_case("gbuffer")))
}

/// `BO2MP_T7_NATIVE_DEBUG=lit`: the lit scene (before the post chain) is
/// handed to the screen.
pub fn debug_lit() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var("BO2MP_T7_NATIVE_DEBUG").is_ok_and(|v| v.eq_ignore_ascii_case("lit")))
}

impl Host {
    pub fn new(gpu: Gpu, scene: Scene) -> Self {
        let mut report: Vec<String> = scene.map.report.iter().map(|l| format!("map: {l}")).collect();
        report.extend(scene.report.iter().map(|l| format!("scene: {l}")));
        report.push(format!("scene: {} materials, {} draws", scene.materials.len(), scene.draws.len()));
        let mut gbuffer = GBuffer::new(gpu.clone(), Arc::new(scene));
        // `T7_STAND_IN=1` (offline audits): a picture missing from the
        // packages at hand is bound as a 1x1 mid grey (listed in the log)
        // instead of its draw being skipped, so every draw of the map goes
        // through its pipeline. Off in the game: a grey opaque stand-in
        // would turn masked and blended surfaces into solid cards.
        gbuffer.check_bindings_without_images = std::env::var("T7_STAND_IN").is_ok_and(|v| v != "0");
        // (A new map: its pictures at the quality's size again.)
        crate::gbuffer::reset_oom_cap();
        let (tx, rx) = std::sync::mpsc::channel();
        let (lgpu, lscene) = (gpu.clone(), gbuffer.scene.clone());
        let spawned = std::thread::Builder::new().name("t7_native_lighting".into()).spawn(move || {
            let t = std::time::Instant::now();
            // The passes' pipelines and pictures first (side by side), then
            // the lighting stage and post chain (their programs may be
            // made already: `crate::prewarm`).
            crate::prewarm::note(&format!("load thread: warm begins ({} draws)", lscene.draws.len()));
            let warm = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| crate::warm::run(&lgpu, &lscene))).unwrap_or_default();
            let t_warm = t.elapsed().as_secs_f32();
            crate::prewarm::note(&format!("load thread: warm done in {t_warm:.1}s; lighting stage begins"));
            // (Out of video memory while the stages are made: the map is
            // drawn without them, with the reason in the log, instead of the
            // device's uncaptured error ending the game; the scope is this
            // thread's, where the stages' textures and buffers are made.)
            let oom_scope = lgpu.device.push_error_scope(wgpu::ErrorFilter::OutOfMemory);
            let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                let t = std::time::Instant::now();
                let mut l = Lighting::new(&lgpu, &lscene, &lscene.zone_dir)?;
                let t_l = t.elapsed().as_secs_f32();
                crate::prewarm::note(&format!("load thread: lighting stage made in {t_l:.1}s; post chain begins"));
                let p = Post::new(&lgpu, &lscene, &lscene.zone_dir);
                fastfile_t7::shared::release();
                // (The stages keep copies of the records they read each
                // view: the inflated map zone goes now, but for the pixels
                // of the pictures kept in it.)
                if let Some((held, kept)) = lscene.map.release_zone() {
                    l.report.push(format!("memory: the map zone let go once the stages were made ({:.0} MB; {:.1} MB of its pictures' pixels kept)", held as f64 / 1048576.0, kept as f64 / 1048576.0));
                }
                // (So do the shared zones' picture copies: the pictures the
                // passes and stages load up front are on the card.)
                let common_px = lscene.map.release_common_pixels();
                if common_px > 0 {
                    l.report.push(format!("memory: the shared zones' picture copies let go once the stages were made ({:.0} MB; one asked for later reads its zone again)", common_px as f64 / 1048576.0));
                }
                l.report.push(format!(
                    "timing: scene passes warm {t_warm:.2}s; lighting stage made in {t_l:.2}s, post chain {:.2}s (compute programs so far: translating {:.2}s of work, shader modules + pipelines {:.2}s of work)",
                    t.elapsed().as_secs_f32() - t_l,
                    crate::compute::CS_TRANSLATE_NS.load(std::sync::atomic::Ordering::Relaxed) as f32 * 1e-9,
                    crate::compute::CS_PIPELINE_NS.load(std::sync::atomic::Ordering::Relaxed) as f32 * 1e-9
                ));
                Ok::<_, String>((l, p))
            }))
            .unwrap_or_else(|_| Err("the lighting stage or the post chain panicked while being made".into()));
            let r = match pollster::block_on(oom_scope.pop()) {
                Some(e) => {
                    crate::prewarm::note(&format!("t7 native: out of video memory making the lighting stage and post chain ({e}; {:.0} MB of pictures loaded{}); the map is drawn unlit", warm.image_bytes as f32 / 1048576.0, crate::gbuffer::oom_cap().map_or(String::new(), |c| format!(", pictures capped at {c} px"))));
                    Err(format!("out of video memory ({e})"))
                }
                None => r,
            };
            crate::prewarm::note("load thread: stages sent");
            let _ = tx.send((r, warm));
            // (Every pipeline of the load is made: the cache is written back
            // for the next load, after the stages are on their way.)
            crate::pipecache::save(&lgpu.device);
            // (This thread's freed pages go back before it exits, instead of
            // staying with the allocator: `heap_trim`.)
            if heap_trim() {
                let ms = diag::release_freed_heap().as_secs_f32() * 1000.0;
                crate::prewarm::note(&format!("memory: load thread heap trimmed before it exits ({ms:.0} ms)"));
            }
        });
        let lighting_rx = spawned.is_ok().then(|| std::sync::Mutex::new(rx));
        Self { gpu, gbuffer, targets: None, report, frames: 0, lighting: None, post: None, lighting_rx, lit_frames: 0, fresh: false, forward: None, started: std::time::Instant::now(), coverage_done: false, ads_amount: 0.0, depth_only: None, quality_rev: None }
    }

    /// The frame clock: the time since the host was made, or with
    /// `T7_FIXED_CLOCK_MS=<ms>` (offline comparisons) that time on every
    /// frame, so effects, wind and the AO and blur ring land the same in two
    /// runs.
    fn clock(&self) -> std::time::Duration {
        static FIXED: std::sync::OnceLock<Option<u64>> = std::sync::OnceLock::new();
        match *FIXED.get_or_init(|| std::env::var("T7_FIXED_CLOCK_MS").ok().and_then(|v| v.trim().parse().ok())) {
            Some(ms) => std::time::Duration::from_millis(ms),
            None => self.started.elapsed(),
        }
    }

    /// The log lines gathered since the last call.
    /// Whether this host draws the map's placed effects (`forward::fx_draw`).
    pub fn fx_drawing(&self) -> bool {
        self.forward.as_ref().is_some_and(crate::forward::Forward::fx_ready)
    }

    pub fn take_report(&mut self) -> Vec<String> {
        let mut r = std::mem::take(&mut self.report);
        r.extend(crate::prewarm::take_report());
        r.extend(crate::timing::take_notes());
        r.extend(crate::timing::take_line(self.frames));
        r
    }

    /// Ends the frame's timing marks (after everything the frame recorded
    /// into `enc`, the hand-off to the screen too); `cpu_ms`: the time the
    /// CPU spent recording it.
    pub fn end_frame(&mut self, enc: &mut wgpu::CommandEncoder, cpu_ms: f64) {
        crate::timing::end(enc, cpu_ms);
        if crate::profile::on() && self.lit_frames > 0 {
            // (The draws the GPU culling leaves, counted on the CPU once
            // per line.)
            if crate::profile::line_due() {
                crate::profile::set_culled(self.gbuffer.culled_count());
            }
            if let Some(l) = crate::profile::end_frame(cpu_ms) {
                self.report.push(l);
            }
        }
    }

    /// Records the frame. `Ok(true)`: the frame is complete (the world is in
    /// the scene); `Ok(false)`: recorded as far as the chain goes.
    pub fn record(&mut self, enc: &mut wgpu::CommandEncoder, width: u32, height: u32, camera: &Camera) -> Result<bool, String> {
        // Nothing is recorded until the load thread has made the passes'
        // pipelines and pictures and the lighting stage (the frame would be
        // incomplete anyway, and making them here would stall the frame).
        let t_begin = std::time::Instant::now();
        self.poll_stages();
        if self.lighting_rx.is_some() {
            return Ok(false);
        }
        crate::timing::begin(&self.gpu, enc);
        crate::timing::cpu("frame begin (timing readback)", t_begin);
        // The game's graphics setting changed: say what applies when.
        let rev = crate::quality::revision();
        if self.quality_rev != Some(rev) {
            if self.quality_rev.is_some() {
                self.report.push(format!("t7 quality: now {}; local lights and the motion blur amount from this frame, SSAO, sun shadows and the picture size cap from the next map", crate::quality::get().line()));
            }
            self.quality_rev = Some(rev);
        }
        let resized = self.targets.as_ref().is_none_or(|t| t.width != width || t.height != height);
        if resized {
            let first = self.targets.is_none();
            let t = Targets::new(&self.gpu, width, height);
            let t_prep = std::time::Instant::now();
            let tr0 = crate::gbuffer::TRANSLATE_NS.load(std::sync::atomic::Ordering::Relaxed);
            self.gbuffer.prepare(&t);
            if first {
                let r = &self.gbuffer.report;
                let tr = (crate::gbuffer::TRANSLATE_NS.load(std::sync::atomic::Ordering::Relaxed) - tr0) as f32 * 1e-9;
                self.report.push(format!(
                    "timing: gbuffer prepare {:.2}s (pipelines {:.2}s of which translating {tr:.2}s, {} pictures {:.0} MB: decode {:.2}s, upload {:.2}s)",
                    t_prep.elapsed().as_secs_f32(), r.t_pipeline, r.images_loaded, r.image_bytes as f32 / 1048576.0, r.t_decode, r.t_upload
                ));
            }
            self.depth_only = Some(t.depth.texture.create_view(&wgpu::TextureViewDescriptor { aspect: wgpu::TextureAspect::DepthOnly, ..Default::default() }));
            self.targets = Some(t);
            if first {
                let r = &self.gbuffer.report;
                self.report.push(format!("gbuffer: {} pipelines made, {} failed; {} draws ready of {} ({} of them opaque forward surfaces' depth prepass)", r.pipelines_made, r.pipelines_failed.len(), r.draws_ready, self.gbuffer.scene.draws.len(), r.forward_depth));
                self.report.push(self.gbuffer.cull_line());
                self.report.push(format!("t7 wind: {} G-buffer bindings take the frame's LightingGlobals (vegetation sway from the grime volume's wind)", r.lighting_globals_binds));
                {
                    let sc = &self.gbuffer.scene;
                    let is_vd = |d: usize| sc.draws[d].source == u32::MAX - 1;
                    let ready: Vec<usize> = self.gbuffer.ready_draws().into_iter().filter(|&d| is_vd(d)).collect();
                    let total = sc.draws.iter().filter(|d| d.source == u32::MAX - 1).count();
                    if total > 0 {
                        let skipped: Vec<String> = r.skipped_draws.iter().filter(|(d, _)| is_vd(*d)).map(|(d, e)| format!("{}: {e}", sc.materials[sc.draws[*d].material].name)).collect();
                        let decals: u32 = ready.iter().map(|&d| sc.draws[d].instances).sum();
                        self.report.push(format!("t7 voldecals: {} of {total} instanced box draws ready ({decals} decals of {}){}", ready.len(), sc.volume_decals.len(), if skipped.is_empty() { String::new() } else { format!("; skipped: {}", skipped.join("; ")) }));
                    }
                }
                self.report.extend(r.pipelines_failed.iter().map(|f| format!("gbuffer: pipeline failed: {f}")));
                self.report.extend(r.draws_skipped.iter().map(|f| format!("gbuffer: skipped: {f}")));
                let mut im = r.images_missing.clone();
                im.sort();
                im.dedup();
                self.report.push(format!("gbuffer: {} images missing", im.len()));
                self.report.extend(im.iter().map(|i| format!("gbuffer: image missing: {i}")));
            }
        }
        let missing_before = self.gbuffer.report.code_constants_missing.len();
        // (The view's whole set, as the lighting stage takes it: the volume
        // decals' pixel programs read the inverse view/projection,
        // renderTargetSize, subpixelOffset and zNear.)
        let t_cpu = std::time::Instant::now();
        let consts = crate::lighting_globals::view_consts(camera, width, height, self.clock().as_secs_f32(), self.frames);
        self.gbuffer.update(&consts);
        // (The draws culled to this view: `crate::cull`.)
        let q = crate::quality::get();
        let lod_view = crate::lod::View::new(height, camera.tan_x, camera.tan_y, q.lod_scale, q.lod_min_radius);
        self.gbuffer.set_cull_view(camera.view_projection * glam::Mat4::from_translation(-camera.eye), camera.eye.to_array(), lod_view);
        // The effects' model elements (`BO2MP_T7_FX_MODELS=1`).
        if !self.gbuffer.scene.fx_models.is_empty() {
            let now = self.clock().as_millis().min(i32::MAX as u128) as i32;
            let (placed, alive) = self.gbuffer.update_fx_models(now, camera.eye.to_array());
            if self.frames == 0 || self.frames.is_power_of_two() && (64..=4096).contains(&self.frames) {
                self.report.push(format!("t7 fx: models: frame {} at {now} ms: {placed} placed of {alive} alive", self.frames));
            }
        }
        // The static models' siege animations (vines, flags, cables).
        let playing = self.gbuffer.update_siege(self.clock().as_secs_f32());
        if self.frames == 0 && playing > 0 {
            self.report.push(format!("t7 siege: {playing} static models animated per frame"));
        }
        crate::timing::cpu("gbuffer constants", t_cpu);
        if self.gbuffer.report.code_constants_missing.len() != missing_before {
            let lines: Vec<String> = self.gbuffer.report.code_constants_missing.iter().map(|c| format!("gbuffer: code constant missing: {c}")).collect();
            self.report.extend(lines);
        }
        let targets = self.targets.as_ref().ok_or("no targets")?;
        // `t7 probe:` lines (see `probe`): only with `CODLEGACY_T7_DEBUG_PROBE=1`
        // or `CODLEGACY_T7_PROBE_FRAMES` (each one stalls the frame on readbacks).
        if crate::probe::enabled()
            && crate::probe::due(self.frames)
            && !crate::surf_debug::get().no_probe
            && self.lighting.is_some()
        {
            let g: std::collections::HashSet<usize> =
                self.gbuffer.ready_draws().into_iter().collect();
            let f: std::collections::HashSet<usize> = self
                .forward
                .as_ref()
                .map(|f| f.ready_draws())
                .unwrap_or_default()
                .into_iter()
                .collect();
            let lines = crate::probe::run(&self.gpu, &self.gbuffer.scene, targets, camera, &g, &f);
            self.report.extend(lines);
            // (The first time, with `CODLEGACY_T7_DEBUG_COVERAGE=1` only: every
            // G-buffer draw's coverage on its own. Thousands of passes and
            // occlusion queries in one submit: never in normal play; on by
            // default it ended his Skyjacked match, 2026-10-26.)
            if !self.coverage_done && crate::gbuffer::coverage_wanted() && !crate::surf_debug::get().no_coverage {
                self.coverage_done = true;
                let lines = self.gbuffer.coverage_audit(targets, camera.view_projection * glam::Mat4::from_translation(-camera.eye));
                self.report.extend(lines);
            }
        }
        // The G-buffer is recorded every frame once the lighting stage that
        // reads it is in (with LIGHTING_BUILT), else once to check it.
        let gbuffer_recorded = (LIGHTING_BUILT && self.lighting.is_some()) || debug_gbuffer() || self.frames == 0;
        if gbuffer_recorded {
            let t_cpu = std::time::Instant::now();
            // The view's LightingGlobals for the G-buffer programs that read
            // it (vegetation wind): the lighting stage's, as it writes it
            // this frame (queued writes land before this encoder runs).
            if let Some(l) = self.lighting.as_ref() {
                enc.copy_buffer_to_buffer(l.globals_buffer(), 0, &self.gbuffer.lighting_globals, 0, crate::lighting_globals::SIZE as u64);
            }
            self.gbuffer.record(enc, targets);
            crate::timing::cpu("gbuffer record", t_cpu);
            crate::timing::mark(enc, "gbuffer");
            if self.frames == 0 {
                self.report.push("gbuffer: first frame recorded".into());
            }
        }
        self.frames += 1;
        let lit = self.record_lighting(enc, camera, resized, gbuffer_recorded)?;
        // (The earlier frames are submitted by now; this one is not.)
        if crate::debug_views::wants_readback() && self.lit_frames == u64::from(crate::debug_views::readback_frame()) {
            if let Some(l) = self.lighting.as_ref() {
                let lines = if crate::debug_views::mode() == "empty" || crate::debug_views::seams_on() {
                    crate::debug_views::empty_report(&self.gpu, l, self.targets.as_ref().unwrap())
                } else {
                    crate::debug_views::report(&self.gpu, l)
                };
                self.report.extend(lines);
            }
        }
        Ok((LIGHTING_BUILT && lit) || debug_gbuffer())
    }

    /// Takes what the load thread made, once it is done.
    /// Takes the load thread's stages when they are made (called every
    /// render tick by the owner, not only from a frame: a loading screen
    /// may have no view to record). True once they are taken.
    pub fn poll(&mut self) -> bool {
        self.poll_stages();
        self.lighting_rx.is_none()
    }

    fn poll_stages(&mut self) {
        let received = self.lighting_rx.as_ref().and_then(|rx| rx.lock().ok().and_then(|rx| rx.try_recv().ok()));
        let Some((r, mut warm)) = received else { return };
        self.lighting_rx = None;
        crate::prewarm::note("load thread's stages taken on the render side");
        // (Everything that reads the shared zones is made: let them go.)
        fastfile_t7::shared::release();
        self.report.append(&mut warm.lines);
        self.gbuffer.adopt(&mut warm);
        let mut f = crate::forward::Forward::new(self.gpu.clone(), self.gbuffer.scene.clone());
        f.adopt(&mut warm);
        self.forward = Some(f);
        match r {
            Ok((mut l, p)) => {
                self.report.append(&mut l.report);
                self.lighting = Some(l);
                match p {
                    Ok(mut p) => {
                        self.report.append(&mut p.report);
                        self.post = Some(p);
                    }
                    Err(e) => self.report.push(format!("post: not made: {e}; the lit scene is shown as it is")),
                }
                self.fresh = true;
            }
            Err(e) => self.report.push(format!("lighting: not made: {e}")),
        }
    }

    /// The lighting stage after the G-buffer, once it is made: bound to the
    /// targets (again on a resize), its constants updated and recorded
    /// (every frame once the chain is built; until then its first frame
    /// only, as the G-buffer's, to check it on this device).
    /// `Ok(true)`: the lit scene was recorded this frame.
    fn record_lighting(&mut self, enc: &mut wgpu::CommandEncoder, camera: &Camera, resized: bool, gbuffer_recorded: bool) -> Result<bool, String> {
        let fresh = std::mem::take(&mut self.fresh);
        let now = self.clock().as_millis().min(i32::MAX as u128) as i32;
        let (Some(l), Some(targets)) = (self.lighting.as_mut(), self.targets.as_ref()) else { return Ok(false) };
        if fresh || resized {
            if let Err(e) = l.prepare(targets) {
                self.report.push(format!("lighting: not bound: {e}"));
                self.lighting = None;
                return Ok(false);
            }
            let t_f = std::time::Instant::now();
            let f = self.forward.get_or_insert_with(|| crate::forward::Forward::new(self.gpu.clone(), self.gbuffer.scene.clone()));
            f.prepare(&mut self.gbuffer, targets, l);
            if fresh {
                self.report.push(format!("timing: forward prepare {:.2}s", t_f.elapsed().as_secs_f32()));
            }
            if fresh {
                let r = &f.report;
                self.report.push(format!("forward: {} pipelines made, {} failed; {} draws ready of {}", r.pipelines_made, r.pipelines_failed.len(), r.draws_ready, r.draws_total));
                // The one-line summary of what the frame costs to issue.
                {
                    let (bundles, bc) = self.gbuffer.bundle_stats();
                    let (left, of, il, iof) = self.gbuffer.culled_count();
                    self.report.push(format!(
                        "t7 fps: {}; G-buffer {of} draws ({left} after culling this view, {il} of {iof} instances) in {bundles} bundles: {} pipeline and {} bind group switches, {} pipelines; forward {} draws, {} pipelines; every pipeline made at load",
                        crate::quality::get().line(),
                        bc.pipelines,
                        bc.binds,
                        self.gbuffer.report.pipelines_made,
                        r.draws_ready,
                        r.pipelines_made
                    ));
                }
                let ready = f.ready_draws();
                let sc = &self.gbuffer.scene;
                let sky = ready.iter().filter(|&&d| sc.draws[d].source == u32::MAX && sc.draws[d].group == 1).count();
                let fx = ready.iter().filter(|&&d| { let t = &sc.materials[sc.draws[d].material].techset; t.contains("emissive") || t.contains("transparent") || t.contains("glass") || t.contains("fxanim") }).count();
                self.report.push(format!("t7 sky/fx draws: {sky} sky box draws issued per frame, {fx} emissive/transparent/glass/fxanim forward draws, {} forward draws in all", ready.len()));
                self.report.extend(crate::forward::sky_material_lines(&self.gbuffer, &ready));
                self.report.push(f.water_line());
                self.report.extend(r.pipelines_failed.iter().map(|e| format!("forward: pipeline failed: {e}")));
                self.report.extend(r.draws_skipped.iter().map(|e| format!("forward: skipped: {e}")));
                // What every draw did, once per map (`t7 surfaces:`).
                let (gr, fr) = (self.gbuffer.ready_draws(), f.ready_draws());
                let g = &self.gbuffer.report;
                let lines = crate::surf_debug::summary(&self.gbuffer.scene, (&gr, &g.skipped_draws, &g.stand_ins), (&fr, &r.skipped_draws, &r.stand_ins));
                self.report.extend(lines);
                // `CODLEGACY_T7_LOG_MODEL=<text>[,<text>]`: every material of the
                // static models whose name contains the text (a pickup
                // truck drawn see-through white, a prop drawn flat): its
                // technique set, which passes it has (G-buffer, forward,
                // prepass), whether each draw was ready in either, the
                // pictures (and whether each was loaded), the forward
                // blend word and the constant buffers' count.
                if let Ok(v) = codlegacy_env::var("CODLEGACY_T7_LOG_MODEL") {
                    let wants: Vec<String> = v
                        .split(',')
                        .map(|t| t.trim().to_ascii_lowercase())
                        .filter(|t| !t.is_empty())
                        .collect();
                    let sc = self.gbuffer.scene.clone();
                    let (gset, fset): (std::collections::HashSet<usize>, std::collections::HashSet<usize>) = (gr.iter().copied().collect(), fr.iter().copied().collect());
                    let mut seen = std::collections::HashSet::new();
                    for (di, d) in sc.draws.iter().enumerate() {
                        if d.group != 1 || d.source as usize >= sc.model_names.len() {
                            continue;
                        }
                        let model = &sc.model_names[d.source as usize];
                        let lm = model.to_ascii_lowercase();
                        if !wants.iter().any(|w| lm.contains(w.as_str())) || !seen.insert((d.source, d.material)) {
                            continue;
                        }
                        let m = &sc.materials[d.material];
                        let imgs: Vec<String> = m.textures.iter().map(|(h, n)| format!("{h:08x}={n}{}", if self.gbuffer.has_image(n) { "" } else { " (NOT LOADED)" })).collect();
                        let fb = m.forward.as_ref().map(|t| format!("{:08x}", t.state.blend[0]));
                        self.report.push(format!(
                            "t7 model: {model} draw {di}: {} ({}): gbuffer {} forward {} prepass {}; ready gbuffer {} forward {}; forward blend0 {fb:?}; {} cbuffers; undrawn {:?}; pictures [{}]",
                            m.name, m.techset, m.technique.is_some(), m.forward.is_some(), m.prepass.is_some(), gset.contains(&di), fset.contains(&di), m.cbuffers.len(), m.undrawn, imgs.join(", ")
                        ));
                    }
                }
            }
        }
        // The view's eye exposure before the lighting: the lit scene is
        // scaled by relHDRExposure (exe 0x141dddc20).
        let t_cpu = std::time::Instant::now();
        if let Some(p) = self.post.as_mut() {
            l.rel_hdr_exposure = p.begin_view(camera);
        }
        crate::timing::cpu("exposure (begin view)", t_cpu);
        let t_cpu = std::time::Instant::now();
        l.update(camera, now, self.lit_frames == 0);
        crate::timing::cpu("lighting update", t_cpu);
        if !l.report.is_empty() {
            self.report.append(&mut l.report);
        }
        let recorded = LIGHTING_BUILT || self.lit_frames == 0;
        if recorded {
            // (The G-buffer it reads is recorded this frame too.)
            if !gbuffer_recorded {
                self.gbuffer.record(enc, targets);
            }
            let t_cpu = std::time::Instant::now();
            l.record_spot_shadows(enc, &self.gbuffer);
            crate::timing::cpu("spot shadows", t_cpu);
            let t_cpu = std::time::Instant::now();
            l.record(enc);
            crate::timing::cpu("lighting record", t_cpu);
            crate::timing::mark(enc, "lighting");
            // (`empty`: the lit lists alone, to check what the lighting leaves in pixels with no surface.)
            if (crate::debug_views::mode() != "empty" || crate::debug_views::keep_forward()) && !(crate::debug_views::seams_on() && !crate::debug_views::keep_forward()) && let (Some(f), Some(scene), Some(scene_texture)) = (self.forward.as_mut(), l.scene_target(), l.scene_texture()) {
                // (The lighting stage's view constants: the target size and
                // relHDRExposure as the lighting has them.)
                let t_cpu = std::time::Instant::now();
                let mut vc = l.view_consts(camera, now);
                // skyMx/sunMx from the map's vukko blend (exe 141dd8b90).
                if let Some(p) = self.post.as_ref() {
                    let deg = self.gbuffer.scene.map.light_refs().map_or(0.0, |l| l.sky_rotation.0);
                    let (rows, used) = p.sky_matrices(camera, deg.to_radians());
                    for (k, row) in rows.into_iter().enumerate() {
                        vc.set(crate::code::SKY_MX_R + k as u16, vec![row]);
                    }
                    if self.lit_frames == 0 {
                        self.report.push(format!("t7 sky matrices: vukko records {used:?}; skyMx {:?} {:?} {:?}, sunMx {:?} {:?} {:?}", rows[0], rows[1], rows[2], rows[3], rows[4], rows[5]));
                    }
                }
                f.update(&vc);
                f.set_eye(camera.eye.to_array());
                f.set_cull_view(camera.view_projection * glam::Mat4::from_translation(-camera.eye));
                crate::timing::cpu("forward constants", t_cpu);
                let t_cpu = std::time::Instant::now();
                let lines = f.update_volumetrics(l, camera, now);
                self.report.extend(lines);
                crate::timing::cpu("fog update", t_cpu);
                // The effects' sprites at this frame's time (`forward::fx_draw`).
                let t_cpu = std::time::Instant::now();
                f.update_fx(now, camera.eye.to_array());
                self.report.extend(f.take_fx_lines());
                crate::timing::cpu("effects sim+upload", t_cpu);
                let t_cpu = std::time::Instant::now();
                f.record(enc, &self.gbuffer, targets, scene, scene_texture, &l.counts);
                crate::timing::cpu("forward record", t_cpu);
            }
            crate::timing::mark(enc, "forward");
            if self.lit_frames == 0 {
                self.report.push("lighting: first frame recorded".into());
            }
            let t_cpu = std::time::Instant::now();
            if let (Some(p), Some(scene)) = (self.post.as_mut(), l.scene_target()) {
                // (SSR: the post chain writes the next frame's gDownsampledScene.)
                p.set_ssr_scene(l.downsampled_scene().cloned());
                // (Motion blur: the view's blur velocity, when it blurs.)
                p.set_motion_blur(l.blur_velocity().cloned());
                // (The ADS blur while aiming: `crate::post_ads`.)
                p.set_ads(self.depth_only.clone().filter(|_| self.ads_amount > 0.0).map(|d| (crate::post_ads::AdsView::new(self.ads_amount, camera.tan_x), d)));
                if let Err(e) = p.run(enc, scene, targets.width, targets.height, camera) {
                    self.report.push(format!("post: not run: {e}; the lit scene is shown as it is"));
                    self.post = None;
                } else if !p.report.is_empty() {
                    self.report.append(&mut p.report);
                }
            }
            crate::timing::cpu("post", t_cpu);
        }
        self.lit_frames += 1;
        Ok(recorded)
    }

    /// bo2mp: the local player's ADS amount this frame (Black Ops II's
    /// `ps.fWeaponPosFrac`, 0..1; 0 while a scope overlay is up), which
    /// drives Black Ops III's ADS blur (`crate::post_ads`).
    pub fn set_ads_amount(&mut self, amount: f32) {
        self.ads_amount = if amount.is_finite() { amount.clamp(0.0, 1.0) } else { 0.0 };
    }

    /// The lit scene (code texture 0xa, RGBA16F), the frame's size, once the
    /// lighting stage is made and bound.
    pub fn scene_target(&self) -> Option<&wgpu::TextureView> {
        self.lighting.as_ref()?.scene_target()
    }

    /// The finished image to hand to the screen, the frame's size: with
    /// [`debug_gbuffer`], the G-buffer's diffuse target; with
    /// `LIGHTING_BUILT`, the lit scene (until the post chain is in: before
    /// exposure, so bright); else none. The post chain's output is
    /// `crate::post::Post::run(enc, scene_target, w, h, camera)` (also
    /// `Post::final_image()`; RGBA8, display-encoded by `resample_final`;
    /// `t7_post_check` runs the whole path offline): once lighting is
    /// built, this must return that.
    pub fn final_image(&self) -> Option<&wgpu::TextureView> {
        let targets = self.targets.as_ref()?;
        if debug_gbuffer() {
            return Some(&targets.colour[0].view);
        }
        if !LIGHTING_BUILT {
            return None;
        }
        // The post chain's finished image; the lit scene as it is only when
        // the post chain isn't made (or `BO2MP_T7_NATIVE_DEBUG=lit`).
        // (A merge had dropped this back to the lit scene, which put the
        // unexposed HDR scene on screen: white blow-outs and black.)
        if !debug_lit() {
            if let Some(img) = self.post.as_ref().and_then(|p| p.final_image()) {
                return Some(img);
            }
        }
        self.scene_target()
    }

    /// The scene depth (reversed: 1 near, 0 at infinity) as a depth-aspect
    /// view, for the hand-off.
    pub fn scene_depth(&self) -> Option<wgpu::TextureView> {
        // (The one made with the targets: a new view every frame made the
        // hand-off's binding again every frame.)
        self.targets.as_ref()?;
        self.depth_only.clone()
    }
}
