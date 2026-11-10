//! What the G-buffer and forward passes need that depends on the scene but
//! not on the frame's size: every technique's pipeline and every picture the
//! draws bind, made side by side on worker threads (`crate::par`) off the
//! render thread; `GBuffer::adopt` and `Forward::adopt` then take them, and
//! `prepare` only makes the bindings.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use render_h1::gpu::Gpu;

use crate::gbuffer::{Pipeline, make_pipeline, make_pipeline_for, DEPTH};
use crate::gpu::Tex;
use crate::lighting::SCENE_FORMAT;
use crate::scene::{Material, Scene, Technique};

type Pipelines = HashMap<(usize, u8), Option<Arc<Pipeline>>>;

#[derive(Default)]
pub struct Warm {
    pub gbuffer: Pipelines,
    pub forward: Pipelines,
    pub images: HashMap<String, Option<Tex>>,
    pub failed: Vec<String>,
    pub forward_failed: Vec<String>,
    pub forward_made: usize,
    pub missing: Vec<String>,
    /// Seconds: pipeline creation (CPU time over all workers), decoding,
    /// uploading; and the wall time of each step.
    pub t_pipeline: f32,
    pub t_decode: f32,
    pub t_upload: f32,
    pub image_bytes: u64,
    pub images_loaded: usize,
    pub wall_pipelines: f32,
    pub wall_images: f32,
    pub lines: Vec<String>,
}

/// The pictures a technique's draws bind (by the pipeline's own texture
/// list, as the bindings are made), for `material`.
pub(crate) fn needed_images(p: &Pipeline, t: &Technique, m: &Material, out: &mut Vec<String>) {
    for &(_, slot, _, _, ps) in &p.layout.textures {
        let st = u16::from(ps);
        let Some(a) = t.args.iter().find(|a| matches!(a.ty & 0xff00, 0x200 | 0x500 | 0x600) && (a.ty & 0xff) == st && u32::from(a.location) == slot) else {
            // (The forward programs' named textures: the glass BRDF table.)
            if p.binds[st as usize].iter().any(|b| b.kind != crate::rdef::CBUFFER && b.kind != crate::rdef::SAMPLER && b.point == slot && b.name == "gEnvBRDFGlass") {
                out.push("$env_brdf_glass".into());
            }
            continue;
        };
        match (a.ty & 0xff00, a.u[0]) {
            (0x200, hash) => {
                if let Some((_, n)) = m.textures.iter().find(|(h, _)| *h == hash)
                    && !n.starts_with('<')
                {
                    out.push(n.clone());
                }
            }
            (0x500, c) if crate::weather::CODE_TEXTURES.contains(&c) => {
                out.push(crate::weather::DEFAULT_IMAGES[crate::weather::CODE_TEXTURES.iter().position(|&x| x == c).unwrap_or(0)].to_owned());
            }
            _ => {}
        }
    }
}

/// Whether the warm also makes the effect sprites' pipelines and loads
/// their pictures (on unless `BO2MP_T7_FX_WARM=0`: then
/// `Forward::prepare_fx` makes them on the render thread, the old way).
fn fx_warm_on() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| !std::env::var("BO2MP_T7_FX_WARM").is_ok_and(|v| v.trim() == "0"))
}

/// The effect sprites' materials (by index, once each), as
/// `Forward::prepare_fx` reads them; none when the effects are off or
/// `BO2MP_T7_FX_WARM=0`.
fn fx_materials(scene: &Scene) -> Vec<usize> {
    if !fx_warm_on() || !crate::forward::fx_draw::enabled() {
        return Vec::new();
    }
    let mut mats: Vec<usize> = scene.fx.defs.iter().flat_map(|d| &d.elems).flat_map(|e| e.materials.iter().flatten().copied()).collect();
    mats.sort_unstable();
    mats.dedup();
    mats
}

/// Makes the pipelines of the scene's techniques and loads the pictures
/// they bind.
pub fn run(gpu: &Gpu, scene: &Scene) -> Warm {
    let mut w = Warm::default();
    let t0 = std::time::Instant::now();
    // The distinct (technique, vertex source) pairs of the G-buffer and
    // forward lists.
    let mut seen = HashSet::new();
    let mut g_jobs: Vec<(&Arc<Technique>, u8)> = Vec::new();
    let mut f_jobs: Vec<(&Arc<Technique>, u8)> = Vec::new();
    for d in &scene.draws {
        let m = &scene.materials[d.material];
        if let Some(t) = m.gbuffer_pass()
            && seen.insert((Arc::as_ptr(t) as usize, d.group, false))
        {
            g_jobs.push((t, d.group));
        }
        if let Some(t) = &m.forward
            && seen.insert((Arc::as_ptr(t) as usize, d.group, true))
        {
            f_jobs.push((t, d.group));
        }
    }
    // bo2mp: the effect sprites' pipelines too (their own vertex group),
    // which `Forward::prepare_fx` made one by one on the render thread
    // (Skyjacked's first load: 0.97 s of its 1.02 s forward prepare).
    let fx = fx_materials(scene);
    for &m in &fx {
        if let Some(t) = &scene.materials[m].forward
            && seen.insert((Arc::as_ptr(t) as usize, crate::forward::fx_draw::FX_GROUP, true))
        {
            f_jobs.push((t, crate::forward::fx_draw::FX_GROUP));
        }
    }
    // (Forward ones second: the G-buffer's are the ones the first frame
    // needs; the order only matters to which finishes first.)
    let timed = |f: &dyn Fn() -> Result<Pipeline, String>| {
        let t = std::time::Instant::now();
        (f(), t.elapsed().as_secs_f32())
    };
    crate::prewarm::note(&format!("warm: {} gbuffer and {} forward pipelines to make on {} threads", g_jobs.len(), f_jobs.len(), crate::par::workers()));
    let g_made = crate::par::map(&g_jobs, |(t, g)| timed(&|| make_pipeline(gpu, t, *g)));
    crate::prewarm::note(&format!("warm: gbuffer pipelines made in {:.1}s", t0.elapsed().as_secs_f32()));
    for ((t, g), (r, secs)) in g_jobs.iter().zip(g_made) {
        w.t_pipeline += secs;
        let key = (Arc::as_ptr(t) as usize, *g);
        match r {
            Ok(p) => {
                w.gbuffer.insert(key, Some(Arc::new(p)));
            }
            Err(e) => {
                w.failed.push(format!("{} (group {g}): {e}", t.techset));
                w.gbuffer.insert(key, None);
            }
        }
    }
    let f_made = crate::par::map(&f_jobs, |(t, g)| timed(&|| make_pipeline_for(gpu, t, *g, &[SCENE_FORMAT], DEPTH, &crate::forward::floatz_slots(t))));
    for ((t, g), (r, secs)) in f_jobs.iter().zip(f_made) {
        w.t_pipeline += secs;
        let key = (Arc::as_ptr(t) as usize, *g);
        match r {
            Ok(p) => {
                w.forward_made += 1;
                w.forward.insert(key, Some(Arc::new(p)));
            }
            Err(e) => {
                w.forward_failed.push(format!("{} (group {g}): {e}", t.techset));
                w.forward.insert(key, None);
            }
        }
    }
    crate::prewarm::note(&format!("warm: forward pipelines made in {:.1}s", t0.elapsed().as_secs_f32()));
    w.wall_pipelines = t0.elapsed().as_secs_f32();

    // The pictures every draw binds, once each.
    let t1 = std::time::Instant::now();
    let mut names: Vec<String> = Vec::new();
    let mut done: HashSet<(usize, u8, usize, bool)> = HashSet::new();
    for d in &scene.draws {
        let m = &scene.materials[d.material];
        for (t, pipes, fwd) in [(&m.gbuffer_pass().cloned(), &w.gbuffer, false), (&m.forward, &w.forward, true)] {
            let Some(t) = t else { continue };
            let Some(Some(p)) = pipes.get(&(Arc::as_ptr(t) as usize, d.group)) else { continue };
            if done.insert((Arc::as_ptr(t) as usize, d.group, d.material, fwd)) {
                needed_images(p, t, m, &mut names);
            }
        }
    }
    // (And the effect sprites' pictures, loaded with the rest.)
    for &mi in &fx {
        let m = &scene.materials[mi];
        let Some(t) = &m.forward else { continue };
        let Some(Some(p)) = w.forward.get(&(Arc::as_ptr(t) as usize, crate::forward::fx_draw::FX_GROUP)) else { continue };
        needed_images(p, t, m, &mut names);
    }
    // The lighting's own pictures are loaded by the lighting stage.
    let mut uniq = HashSet::new();
    names.retain(|n| uniq.insert(n.clone()));
    crate::prewarm::note(&format!("warm: {} pictures to load", names.len()));
    // (`BO2MP_T7_TIMING`: each progress line also says where the pictures'
    // time went so far, as work over every thread: the package reads, their
    // inflating, the rest of the decode, the uploads and their error scopes'
    // wait.)
    let timing = std::env::var_os("BO2MP_T7_TIMING").is_some();
    let (pak0, scope0) = (fastfile_t7::xpak::read_stats(), crate::gbuffer::scope_wait());
    let count = std::sync::atomic::AtomicUsize::new(0);
    let (dec_ns, up_ns) = (std::sync::atomic::AtomicU64::new(0), std::sync::atomic::AtomicU64::new(0));
    let laps = |k: usize| {
        let pak = fastfile_t7::xpak::read_stats().since(pak0);
        let dec = dec_ns.load(std::sync::atomic::Ordering::Relaxed) as f64 / 1e9;
        format!(
            "packages read {:.1}s ({} reads, {:.0} MB), inflated {:.1}s, rest of the decode {:.1}s, upload {:.1}s (error scopes {:.1}s) of work on {} threads; {:.1} pictures a second",
            pak.read,
            pak.reads,
            pak.bytes as f64 / 1048576.0,
            pak.inflate,
            (dec - pak.read - pak.inflate).max(0.0),
            up_ns.load(std::sync::atomic::Ordering::Relaxed) as f64 / 1e9,
            crate::gbuffer::scope_wait() - scope0,
            crate::par::workers(),
            k as f32 / t1.elapsed().as_secs_f32().max(1e-3)
        )
    };
    // bo2mp (map bake): the pictures from this map's baked file when it has
    // one (read start to end on one thread, uploaded on the others); the
    // rest the old way, and with no file yet, the old way writing one.
    let bake = if crate::gbuffer::oom_cap().is_none() { crate::bake::Pictures::new(&scene.map, crate::gbuffer::image_max_dim()) } else { None };
    #[allow(clippy::type_complexity)]
    let mut slots: Vec<Option<Result<(Tex, u64, f32, f32), (String, f32)>>> = names.iter().map(|_| None).collect();
    let pak = bake.as_ref().and_then(|b| b.open());
    let had_pak = pak.is_some();
    if let (Some(b), Some(reader)) = (&bake, pak) {
        let index: HashMap<&str, usize> = names.iter().enumerate().map(|(i, n)| (n.as_str(), i)).collect();
        let (got, err) = crate::bake::upload_all(gpu, reader, &index, &names);
        let n = got.len();
        for (i, l) in got {
            slots[i] = Some(Ok(l));
        }
        crate::prewarm::note(&format!("bake: {n} of {} pictures read from {} ({:.1}s)", names.len(), b.dir().display(), t1.elapsed().as_secs_f32()));
        if let Some(e) = err {
            crate::prewarm::note(&format!("bake: the pictures file stopped reading back ({e}); it is made again on the next load"));
            let _ = std::fs::remove_file(b.dir().join(map_bake::pictures::FILE));
        }
    }
    // (Why, said before the writer starts over the old manifest.)
    let why = if had_pak { String::new() } else { bake.as_ref().map(|b| b.why()).unwrap_or_else(String::new) };
    let writer = if had_pak { None } else { bake.as_ref().and_then(|b| b.writer()) };
    if let (Some(b), Some(_)) = (&bake, &writer) {
        crate::prewarm::note(&format!("bake: no usable pictures file for this map ({why}); writing one in {} as the pictures load", b.dir().display()));
    }
    let rest: Vec<usize> = (0..names.len()).filter(|&i| slots[i].is_none()).collect();
    // (The loading screen's row for it; a cancelled load throws the file away.)
    let job = writer.as_ref().map(|_| map_bake::status::begin("t7", &scene.map.name, "pictures", rest.len() as u64));
    if let Some(j) = &job {
        j.set_why(&why);
    }
    let keep = |n: &str, l: &fastfile_t7::texture::ImageLevels| {
        if let Some(w) = &writer
            && !job.as_ref().is_some_and(|j| j.is_canceled())
        {
            w.push(crate::bake::picture(n, l));
        }
    };
    let keep: Option<&(dyn Fn(&str, &fastfile_t7::texture::ImageLevels) + Sync)> = if writer.is_some() { Some(&keep) } else { None };
    let loaded = crate::par::map(&rest, |&i| {
        let n = &names[i];
        let r = crate::gbuffer::load_image_kept(gpu, &scene.map, n, keep);
        let (dec, up) = match &r {
            Ok((_, _, dec, up)) => (*dec, *up),
            Err((_, dec)) => (*dec, 0.0),
        };
        dec_ns.fetch_add((dec * 1e9) as u64, std::sync::atomic::Ordering::Relaxed);
        up_ns.fetch_add((up * 1e9) as u64, std::sync::atomic::Ordering::Relaxed);
        let k = count.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1;
        if let Some(j) = &job {
            j.add(1);
        }
        if k % 250 == 0 {
            let lap = if timing { format!("; {}", laps(k)) } else { String::new() };
            crate::prewarm::note(&format!("warm: {k} of {} pictures loaded ({:.1}s){lap}", names.len(), t1.elapsed().as_secs_f32()));
        }
        r
    });
    if timing {
        crate::prewarm::note(&format!("warm: all {} pictures loaded ({:.1}s); {}", names.len(), t1.elapsed().as_secs_f32(), laps(names.len())));
    }
    let kept = loaded.iter().filter(|r| r.is_ok()).count();
    for (&i, r) in rest.iter().zip(loaded) {
        slots[i] = Some(r);
    }
    if let (Some(b), Some(w)) = (&bake, writer) {
        // (A step-down for the card's memory: the pictures are not the size
        // the key says.)
        let whole = crate::gbuffer::oom_cap().is_none();
        let canceled = job.as_ref().is_some_and(|j| j.is_canceled());
        let why = if canceled { "the load was cancelled" } else { "the card ran out of video memory during the load" };
        w.finish(whole && !canceled, if whole && !canceled { b.manifest(kept) } else { why.into() });
        if let Some(j) = &job {
            j.end(whole && !canceled);
        }
    }
    for (n, r) in names.iter().zip(slots) {
        let Some(r) = r else { continue };
        match r {
            Ok((tex, bytes, dec, up)) => {
                w.t_decode += dec;
                w.t_upload += up;
                w.image_bytes += bytes;
                w.images_loaded += 1;
                w.images.insert(n.clone(), Some(tex));
            }
            Err((e, dec)) => {
                w.t_decode += dec;
                w.missing.push(e);
                w.images.insert(n.clone(), None);
            }
        }
    }
    w.wall_images = t1.elapsed().as_secs_f32();
    w.lines.push(format!(
        "timing: warm: {} gbuffer + {} forward pipelines in {:.2}s wall ({:.2}s of work on {} threads); {} pictures {:.0} MB in {:.2}s wall (decode {:.2}s + upload {:.2}s of work)",
        w.gbuffer.len(),
        w.forward.len(),
        w.wall_pipelines,
        w.t_pipeline,
        crate::par::workers(),
        w.images_loaded,
        w.image_bytes as f32 / 1048576.0,
        w.wall_images,
        w.t_decode,
        w.t_upload
    ));
    if let Some(cap) = crate::gbuffer::oom_cap() {
        let lost = w.missing.iter().filter(|e| e.contains("out of video memory")).count();
        w.lines.push(format!("t7 native: the card ran out of video memory during the picture load: pictures after that at most {cap} px, {lost} not loaded"));
    }
    w
}
