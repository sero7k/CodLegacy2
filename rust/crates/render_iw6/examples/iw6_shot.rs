//! Draws a Ghosts map with Ghosts' own techniques and post, offline, to a
//! PNG.
//!
//! `cargo run -p render_iw6 --release --example iw6_shot -- <map.ff> [zone.ff...] [--out pic.png] [--size 1280x720] [--cam x y z pitch yaw] [--fov 80]`
//!
//! With only the map given, the zones beside it are found
//! (`Scene::zones_for`). The camera defaults to the map's intermission
//! view. Prints what was drawn and what fell back.
//!
//! `IW6_SHOT_PLACEHOLDER=graded|old` puts a grey box (a stand-in for a
//! player or the view gun: the tool spawns no characters) in front of the
//! camera, lit as the game's fallback pass lights a model on a frame drawn
//! in stages (`iw6_lit` in `geometry_diagnostic.wgsl`, diffuse part): a
//! texel of 0.5 (albedo 0.25 after the shaders' squaring) times the light
//! grid's mean light at the box (Ghosts' units, `lightgrid::mean_light`)
//! plus the map's sun by N.L (no shadow): `graded` draws it into the HDR
//! scene between `Host::record_scene` and `Host::record_fog` (the game's
//! staged frame, `BO2MP_IW6_CHAR_GRADE`), so Ghosts' fog, colorization,
//! bloom, iris and final pass grade it; `old` draws the same light over the
//! finished picture, sRGB-encoded (the order before, no exposure or curve).
//! `IW6_SHOT_ANIM="model,clip,secs[,x,y,z,yaw]"` poses a script model's animation: the model's
//! placed instance (the first of that name, or the one at x,y,z) skinned by `clip` at `secs`
//! (`end` for the clip's length), as the sim's animator would publish it. Two runs with `0` and
//! `end` show a gate shut and open.
//! `IW6_SHOT_CARRY="carrier,clip,secs,cx,cy,cz,cyaw,tag,child,hx,hy,hz"` poses a model that is linked to a
//! tag of an animated carrier (the iron gate's halves ride `generic_prop_raven`'s `j_prop_1` / `j_prop_2`):
//! the child, placed at hx,hy,hz when the clip starts, goes where the tag carries it at `secs`.
//! `IW6_SHOT_PLACEHOLDER_AT="dist,size,down"` (default 160,48,24).
//! `IW6_SHOT_VARIANTS="name:KEY=val,KEY=val;name2:..."`: every camera drawn once per variant in the
//! one process (the map is read once): the variant's environment switches set (and put back after),
//! `quality=low|medium|high|ultra` applied as the graphics preset. Each picture goes to
//! `<out stem>_<variant>_<k>.png`; the first variant is the reference and each other is compared to
//! it (`variant vs ref: n pixels differ, max d levels` or `byte-identical`).
//! `IW6_SHOT_DOF="vs/ve/ns/ne/fs/fe/nb/fb"` (commas also work outside a variant) sets a depth of field (`Host::set_dof`, the exe's order).
//! `IW6_SHOT_SUN_VIS=v` holds the sun's visibility at v (0..1; the blind and glare settle at once offline).
//! The motion blur needs frames that move: `IW6_SHOT_MOVE` / `IW6_SHOT_WALK`.

use std::path::PathBuf;

// ---- allocation counting (IW6_SHOT_ALLOC=1 prints the heap allocations of each frame's record and
// finish; IW6_SHOT_ALLOC_TRACE=1 also names where the last frame's record allocated, by the first
// frame of the call stack inside the renderer crates). Costs a relaxed counter per allocation.
mod alloc_count {
    use std::alloc::{GlobalAlloc, Layout, System};
    use std::cell::Cell;
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicBool, AtomicU64, Ordering::Relaxed};
    pub static ALLOCS: AtomicU64 = AtomicU64::new(0);
    pub static BYTES: AtomicU64 = AtomicU64::new(0);
    pub static TRACE: AtomicBool = AtomicBool::new(false);
    pub static STACKS: Mutex<Vec<(std::backtrace::Backtrace, usize)>> = Mutex::new(Vec::new());
    thread_local! { static IN: Cell<bool> = const { Cell::new(false) }; }
    pub struct Counting;
    unsafe impl GlobalAlloc for Counting {
        unsafe fn alloc(&self, l: Layout) -> *mut u8 {
            ALLOCS.fetch_add(1, Relaxed);
            BYTES.fetch_add(l.size() as u64, Relaxed);
            if TRACE.load(Relaxed) && !IN.with(|c| c.replace(true)) {
                let bt = std::backtrace::Backtrace::force_capture();
                if let Ok(mut v) = STACKS.lock() {
                    v.push((bt, l.size()));
                }
                IN.with(|c| c.set(false));
            }
            unsafe { System.alloc(l) }
        }
        unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
            unsafe { System.dealloc(p, l) }
        }
        unsafe fn realloc(&self, p: *mut u8, l: Layout, n: usize) -> *mut u8 {
            ALLOCS.fetch_add(1, Relaxed);
            BYTES.fetch_add(n as u64, Relaxed);
            if TRACE.load(Relaxed) && !IN.with(|c| c.replace(true)) {
                let bt = std::backtrace::Backtrace::force_capture();
                if let Ok(mut v) = STACKS.lock() {
                    v.push((bt, n));
                }
                IN.with(|c| c.set(false));
            }
            unsafe { System.realloc(p, l, n) }
        }
    }
    pub fn now() -> (u64, u64) {
        (ALLOCS.load(Relaxed), BYTES.load(Relaxed))
    }
    /// Prints the traced allocations grouped by the first frames that name a renderer crate.
    pub fn report() {
        let v = std::mem::take(&mut *STACKS.lock().unwrap());
        let mut by: std::collections::BTreeMap<String, (u32, usize)> = Default::default();
        let depth: usize = std::env::var("IW6_SHOT_ALLOC_DEPTH").ok().and_then(|v| v.parse().ok()).unwrap_or(2);
        for (bt, size) in &v {
            let text = bt.to_string();
            let mut picked = Vec::new();
            for line in text.lines() {
                let t = line.trim();
                if let Some((_, sym)) = t.split_once(": ")
                    && (sym.contains("render_iw6") || sym.contains("render_gpu") || sym.contains("render_h1") || sym.contains("native_cull"))
                    && !sym.contains("alloc_count")
                {
                    picked.push(sym.split("::h").next().unwrap_or(sym).to_string());
                    if picked.len() == depth {
                        break;
                    }
                }
            }
            let key = if picked.is_empty() { "(outside the renderer crates)".to_string() } else { picked.join(" <- ") };
            if *size > 64 * 1024 {
                eprintln!("alloc trace: big {size} bytes  {key}");
            }
            let e = by.entry(key).or_default();
            e.0 += 1;
            e.1 += size;
        }
        let mut rows: Vec<_> = by.into_iter().collect();
        rows.sort_by_key(|r| std::cmp::Reverse(r.1.0));
        for (k, (n, b)) in rows.iter().take(40) {
            eprintln!("alloc trace: {n:>6} allocs {b:>9} bytes  {k}");
        }
    }
}
#[global_allocator]
static GLOBAL: alloc_count::Counting = alloc_count::Counting;

use glam::Vec3;
use render_h1::code::{Camera, View};
use render_h1::gpu::Gpu;
use render_iw6::host::{Host, Look};
use render_iw6::scene::Scene;

fn main() {
    let mut zones_p: Vec<PathBuf> = Vec::new();
    let mut out = PathBuf::from("iw6_shot.png");
    let (mut w, mut h) = (1280u32, 720u32);
    let mut cams: Vec<(Vec3, Vec3)> = Vec::new();
    let mut fov_x = 80.0f32;
    let a: Vec<String> = std::env::args().skip(1).collect();
    let mut i = 0;
    while i < a.len() {
        match a[i].as_str() {
            "--out" => {
                out = a[i + 1].clone().into();
                i += 1;
            }
            "--size" => {
                let (x, y) = a[i + 1].split_once('x').expect("WxH");
                w = x.parse().expect("w");
                h = y.parse().expect("h");
                i += 1;
            }
            "--fov" => {
                fov_x = a[i + 1].parse().expect("fov");
                i += 1;
            }
            "--cam" => {
                let v: Vec<f32> = a[i + 1..i + 6].iter().map(|s| s.parse().expect("cam")).collect();
                cams.push((Vec3::new(v[0], v[1], v[2]), Vec3::new(v[3], v[4], 0.0)));
                i += 5;
            }
            p => zones_p.push(p.into()),
        }
        i += 1;
    }
    if zones_p.len() == 1 {
        zones_p = Scene::zones_for(&zones_p[0]);
    }
    eprintln!("zones: {zones_p:?}");
    let t0 = std::time::Instant::now();
    let scene = Scene::load(&zones_p).expect("load");
    for n in &scene.notes {
        eprintln!("note: {n}");
    }
    eprintln!("read in {:.1}s: {} draws, {} materials", t0.elapsed().as_secs_f32(), scene.draws.len(), scene.materials.len());
    // IW6_SHOT_LMROW="page,row[,from,count]": a lightmap secondary's texels
    // on one row (debug: where the light defs' falloff ramps are).
    if let Ok(v) = std::env::var("IW6_SHOT_LMROW") {
        let n: Vec<usize> = v.split(',').filter_map(|s| s.trim().parse().ok()).collect();
        let mut scene = scene;
        if let Some(Some(name)) = scene.lightmaps.get(n[0]).map(|l| l.1.clone())
            && let Some((px, _)) = scene.images.image(&name)
        {
            eprintln!("lightmap secondary {name}: {}x{} format {:?}", px.width, px.height, px.dxgi_format);
            let w = px.width as usize;
            let (from, count) = (n.get(2).copied().unwrap_or(0), n.get(3).copied().unwrap_or(w));
            let row = n[1];
            for x in from..(from + count).min(w) {
                let at = (row * w + x) * 8;
                let h = |i: usize| render_iw6::model_lighting::unhalf(u16::from_le_bytes([px.data[at + 2 * i], px.data[at + 2 * i + 1]]));
                eprintln!("  {x},{row}: {:.4} {:.4} {:.4} {:.4}", h(0), h(1), h(2), h(3));
            }
        }
        return;
    }
    if cams.is_empty() {
        cams.push(scene.intermission().unwrap_or((Vec3::new(0.0, 0.0, 200.0), Vec3::ZERO)));
    }
    let gpu = Gpu::headless().expect("gpu");
    // IW6_SHOT_OPEN=1: every door switch's pieces at their far end (the
    // pose a script would move them to).
    let open: Vec<(render_iw6::scene::MoverKey, [f32; 3], [f32; 3])> = if std::env::var("IW6_SHOT_OPEN").is_ok_and(|v| v == "1") {
        scene
            .logic
            .doors
            .iter()
            .flat_map(|d| d.pieces.iter())
            .map(|p| {
                let key = match (p.brush, &p.model) {
                    (Some(n), _) => render_iw6::scene::MoverKey::Brush(n),
                    (None, m) => render_iw6::scene::MoverKey::Model {
                        name: m.clone().unwrap_or_default(),
                        origin: p.start,
                    },
                };
                (key, p.end, p.end_angles)
            })
            .collect()
    } else {
        Vec::new()
    };
    // IW6_SHOT_MOVERS=1: each mover's world bounds as placed.
    if std::env::var("IW6_SHOT_MOVERS").is_ok_and(|v| v == "1") {
        let b = render_iw6::world::draw_bounds(&scene);
        for m in &scene.movers {
            let (mut lo, mut hi) = (Vec3::splat(f32::MAX), Vec3::splat(f32::MIN));
            for (d, bb) in scene.draws.iter().zip(&b) {
                if let (Some(o), Some((a, c))) = (d.object, bb)
                    && m.objects.contains(&o)
                {
                    lo = lo.min(*a);
                    hi = hi.max(*c);
                }
            }
            eprintln!("mover {:?}: {lo:.0} .. {hi:.0}", m.key);
        }
    }
    let mut host = Host::new(gpu.clone(), scene, wgpu::TextureFormat::Depth32Float, zones_p.clone());
    let moved = open.iter().filter(|(k, o, a)| host.set_mover_pose(k, *o, *a)).count();
    if !open.is_empty() {
        eprintln!("iw6_shot: {moved} of {} door pieces moved to their far end", open.len());
    }
    if let Ok(v) = std::env::var("IW6_SHOT_ANIM") {
        pose_animation(&mut host, &zones_p, &v);
    }
    if let Ok(v) = std::env::var("IW6_SHOT_CARRY") {
        pose_carried(&mut host, &zones_p, &v);
    }
    let depth = render_h1::post::target(&gpu, "depth", w, h, wgpu::TextureFormat::Depth32Float);
    let display = render_h1::post::target(&gpu, "display", w, h, wgpu::TextureFormat::Rgba8Unorm);
    let dv = depth.create_view(&Default::default());
    let ds = depth.create_view(&wgpu::TextureViewDescriptor {
        aspect: wgpu::TextureAspect::DepthOnly,
        ..Default::default()
    });
    let ov = display.create_view(&Default::default());
    let placeholder = std::env::var("IW6_SHOT_PLACEHOLDER").ok().map(|v| {
        let at: Vec<f32> = std::env::var("IW6_SHOT_PLACEHOLDER_AT")
            .ok()
            .map(|v| v.split(',').filter_map(|s| s.trim().parse().ok()).collect())
            .unwrap_or_default();
        let graded = v.trim() != "old";
        eprintln!("iw6_shot: placeholder box drawn {}", if graded { "into the HDR scene (graded)" } else { "over the finished picture (old order)" });
        Placeholder::new(
            &gpu,
            graded,
            [at.first().copied().unwrap_or(160.0), at.get(1).copied().unwrap_or(48.0), at.get(2).copied().unwrap_or(24.0)],
        )
    });
    let sun = {
        let s = &host.scene().sun;
        (s.dir, s.diffuse)
    };
    // The light grid, for the box's ambient light.
    let zones = host.scene().zones.clone();
    let grid_zone = &zones[0];
    let grid = grid_zone
        .cap
        .gfx_worlds
        .first()
        .and_then(|g| g.light_grid.as_ref())
        .and_then(|r| fastfile_iw6::lightgrid::LightGrid::new(&grid_zone.zone.image, r));
    // IW6_SHOT_VARIANTS: (name, env switches, preset).
    type Variant = (String, Vec<(String, String)>, Option<String>);
    let variants: Vec<Variant> = match std::env::var("IW6_SHOT_VARIANTS") {
        Ok(v) => v
            .split(';')
            .filter(|s| !s.trim().is_empty())
            .map(|item| {
                let (name, rest) = item.split_once(':').unwrap_or((item, ""));
                let mut env = Vec::new();
                let mut preset = None;
                for kv in rest.split(',').filter(|s| !s.trim().is_empty()) {
                    let (k, val) = kv.split_once('=').unwrap_or((kv, ""));
                    if k.trim() == "quality" {
                        preset = Some(val.trim().to_string());
                    } else {
                        env.push((k.trim().to_string(), val.trim().to_string()));
                    }
                }
                (name.trim().to_string(), env, preset)
            })
            .collect(),
        Err(_) => vec![(String::new(), Vec::new(), None)],
    };
    let mut reference: std::collections::HashMap<usize, Vec<u8>> = std::collections::HashMap::new();
    for (variant, env, preset) in &variants {
      let saved: Vec<(String, Option<String>)> = env.iter().map(|(k, _)| (k.clone(), std::env::var(k).ok())).collect();
      for (k, val) in env {
          // SAFETY: the tool is single-threaded at this point (the GPU's own threads read no switch).
          unsafe { std::env::set_var(k, val) };
      }
      // (Switches keep what they read: read again.)
      render_iw6::switch::refresh();
      if let Some(p) = preset {
          let preset = match p.as_str() {
              "low" => native_quality::Preset::Low,
              "medium" => native_quality::Preset::Medium,
              "high" => native_quality::Preset::High,
              _ => native_quality::Preset::Ultra,
          };
          host.apply_quality(native_quality::Quality::preset(preset), false);
      }
      eprintln!("iw6_shot: variant {:?} {env:?} {preset:?}", variant);
      // IW6_SHOT_DOF="vs,ve,ns,ne,fs,fe,nb,fb": the depth of field for this variant's pictures.
      let dof: Option<[f32; 8]> = std::env::var("IW6_SHOT_DOF").ok().and_then(|v| {
          let n: Vec<f32> = v.split(|c: char| c == ',' || c == '/' || c == ' ').filter_map(|x| x.trim().parse().ok()).collect();
          <[f32; 8]>::try_from(n).ok()
      });
      host.set_dof(dof);
    for (k, (eye, angles)) in cams.iter().enumerate() {
        let view = View {
            eye: *eye,
            angles: *angles,
            fov_x,
            width: w,
            height: h,
            near: 4.0,
        };
        let camera = Camera::from(&view);
        // IW6_SHOT_MOVE=deg: the yaw turns this much a frame (the profile's
        // constants with a moving view), back to the shot's angle for the
        // last frame.
        let turn: f32 = std::env::var("IW6_SHOT_MOVE").ok().and_then(|v| v.parse().ok()).unwrap_or(0.0);
        // IW6_SHOT_WALK="x,y,z": the eye steps this far a frame (units),
        // arriving at the shot's position on the last frame (a moving
        // camera: the sun cascades' cache).
        let walk = std::env::var("IW6_SHOT_WALK")
            .ok()
            .map(|v| v.split(',').filter_map(|s| s.trim().parse::<f32>().ok()).collect::<Vec<_>>())
            .filter(|v| v.len() == 3)
            .map_or(Vec3::ZERO, |v| Vec3::new(v[0], v[1], v[2]));
        let t = std::time::Instant::now();
        // Two frames: the iris settles on the first (`IW6_SHOT_FRAMES=n`:
        // more, for the profile's steady state).
        let frames = std::env::var("IW6_SHOT_FRAMES").ok().and_then(|v| v.parse::<u32>().ok()).unwrap_or(2).max(2);
        for f in 0..frames {
            let camera = if (turn != 0.0 || walk != Vec3::ZERO) && f + 1 < frames {
                let back = (frames - 1 - f) as f32;
                Camera::from(&View {
                    eye: *eye - walk * back,
                    angles: *angles + Vec3::new(0.0, turn * back, 0.0),
                    ..view
                })
            } else {
                camera
            };
            // IW6_SHOT_CASTER=1: one standing proxy 150 units ahead of the camera.
            if std::env::var("IW6_SHOT_CASTER").is_ok_and(|v| v == "1") {
                let fwd = camera.rotation.row(2).truncate();
                let feet = camera.eye + Vec3::new(fwd.x, fwd.y, 0.0).normalize_or_zero() * 150.0 - Vec3::new(0.0, 0.0, 60.0);
                host.set_sun_casters(&[render_iw6::caster::Caster::standing(feet.to_array(), 70.0, 15.0)]);
            }
            let mut enc = gpu.device.create_command_encoder(&Default::default());
            let tr = std::time::Instant::now();
            let a0 = alloc_count::now();
            let trace_now = f + 1 == frames && std::env::var("IW6_SHOT_ALLOC_TRACE").is_ok_and(|v| v == "1");
            alloc_count::TRACE.store(trace_now, std::sync::atomic::Ordering::Relaxed);
            match placeholder.as_ref() {
                Some(p) if p.graded => {
                    host.record_scene(&mut enc, &dv, w, h, &camera, &Look::default()).expect("scene");
                    p.draw(&gpu, &mut enc, host.scene_view().expect("scene target"), &dv, &camera, sun, grid.as_ref());
                    host.record_fog(&mut enc, &dv, &ds).expect("fog");
                    host.record_post(&mut enc, &ov, wgpu::TextureFormat::Rgba8Unorm).expect("post");
                }
                _ => {
                    host.record(&mut enc, &dv, &ds, &ov, wgpu::TextureFormat::Rgba8Unorm, w, h, &camera, &Look::default())
                        .expect("frame");
                    if let Some(p) = placeholder.as_ref() {
                        p.draw(&gpu, &mut enc, &ov, &dv, &camera, sun, grid.as_ref());
                    }
                }
            }
            let record_ms = tr.elapsed().as_secs_f64() * 1000.0;
            alloc_count::TRACE.store(false, std::sync::atomic::Ordering::Relaxed);
            let a1 = alloc_count::now();
            if trace_now {
                alloc_count::report();
            }
            // (wgpu encodes every command for the driver at `finish`: the
            // per-draw validation and state tracking are counted there, not
            // in the record.)
            let tf = std::time::Instant::now();
            let cb = enc.finish();
            let finish_ms = tf.elapsed().as_secs_f64() * 1000.0;
            let a2 = alloc_count::now();
            if std::env::var("IW6_SHOT_ALLOC").is_ok_and(|v| v == "1") {
                eprintln!(
                    "view {k} frame {f}: heap allocations: record {} ({} KB), finish {} ({} KB)",
                    a1.0 - a0.0,
                    (a1.1 - a0.1) / 1024,
                    a2.0 - a1.0,
                    (a2.1 - a1.1) / 1024
                );
            }
            let ts = std::time::Instant::now();
            gpu.queue.submit([cb]);
            let submit_ms = ts.elapsed().as_secs_f64() * 1000.0;
            if host.profile.is_some() {
                // (Waits for the frame: the GPU's time on a software
                // adapter is this wait.)
                let tw = std::time::Instant::now();
                let _ = gpu.device.poll(wgpu::PollType::wait_indefinitely());
                let wait_ms = tw.elapsed().as_secs_f64() * 1000.0;
                if let Some(l) = host.profile_line(0.0) {
                    eprintln!("view {k} frame {f}: record {record_ms:.2} ms, finish {finish_ms:.2} ms, submit {submit_ms:.2} ms, wait {wait_ms:.2} ms; {l}");
                }
            }
        }
        if let Some(p) = &mut host.profile {
            for n in p.notes.drain(..) {
                eprintln!("{n}");
            }
        }
        if let Some(f) = host.fogged() {
            let hdr = render_h1::world::read_rgba16f(&gpu, f, w, h);
            let mut lums: Vec<f32> = hdr
                .iter()
                .map(|c| 0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2])
                .collect();
            let nan = lums.iter().filter(|v| !v.is_finite()).count();
            // (Where the first few non-finite pixels are.)
            for (i, c) in hdr.iter().enumerate().filter(|(_, c)| c.iter().any(|v| !v.is_finite())).take(4) {
                eprintln!("not finite at {},{}: {c:?}", i as u32 % w, i as u32 / w);
            }
            lums.retain(|v| v.is_finite());
            lums.sort_by(f32::total_cmp);
            let q = |p: f32| lums.get(((lums.len() as f32 - 1.0) * p) as usize).copied().unwrap_or(0.0);
            let mean = lums.iter().sum::<f32>() / lums.len().max(1) as f32;
            // IW6_SHOT_PIXEL="x,y;x,y": the HDR scene at those pixels.
            if let Ok(v) = std::env::var("IW6_SHOT_PIXEL") {
                for xy in v.split(';') {
                    if let Some((x, y)) = xy.split_once(',')
                        && let (Ok(x), Ok(y)) = (x.trim().parse::<u32>(), y.trim().parse::<u32>())
                        && x < w
                        && y < h
                    {
                        eprintln!("hdr at {x},{y}: {:?}", hdr[(y * w + x) as usize]);
                    }
                }
            }
            eprintln!(
                "hdr luminance: mean {mean:.3}, p10 {:.3} p50 {:.3} p90 {:.3} max {:.3}, {nan} not finite; centre {:?}",
                q(0.1),
                q(0.5),
                q(0.9),
                q(1.0),
                hdr[(h / 2 * w + w / 2) as usize]
            );
        }
        // IW6_SHOT_SPOTMAP=1: the spot shadow atlas beside the picture
        // (depth as grey, near dark).
        if std::env::var("IW6_SHOT_SPOTMAP").is_ok()
            && let Some(t) = host.renderer.code_textures.get("SHADOWMAP_SPOT")
        {
            let (sw, sh) = (t.texture.width(), t.texture.height());
            let d = read_depth(&gpu, &t.texture, sw, sh);
            let lo = d.iter().copied().filter(|v| *v < 1.0).fold(1.0f32, f32::min);
            let g: Vec<u8> = d
                .iter()
                .map(|v| if *v >= 1.0 { 255 } else { (((v - lo) / (1.0 - lo).max(1e-6)).sqrt() * 230.0) as u8 })
                .collect();
            let p = out.with_file_name(format!("{}_spotmap_{k}.png", out.file_stem().unwrap_or_default().to_string_lossy()));
            let f = std::fs::File::create(&p).expect("png");
            let mut e = png::Encoder::new(std::io::BufWriter::new(f), sw, sh);
            e.set_color(png::ColorType::Grayscale);
            e.set_depth(png::BitDepth::Eight);
            e.write_header().expect("png").write_image_data(&g).expect("png");
            eprintln!("spot shadow atlas: {} (lights {:?})", p.display(), host.renderer.spot_shadows.keys().collect::<Vec<_>>());
        }
        let mut px = read_rgba8(&gpu, &display, w, h);
        // (The final pass writes the bloom mask to alpha: the picture is opaque.)
        for p in px.chunks_exact_mut(4) {
            p[3] = 255;
        }
        let path = {
            let stem = out.file_stem().unwrap_or_default().to_string_lossy().into_owned();
            match (cams.len() == 1, variant.is_empty()) {
                (true, true) => out.clone(),
                (false, true) => out.with_file_name(format!("{stem}_{k}.png")),
                (_, false) => out.with_file_name(format!("{stem}_{variant}_{k}.png")),
            }
        };
        match reference.get(&k) {
            None => {
                reference.insert(k, px.clone());
            }
            Some(r) => {
                let (mut n, mut max) = (0usize, 0i32);
                for (a, b) in r.chunks_exact(4).zip(px.chunks_exact(4)) {
                    let d = (0..3).map(|c| (i32::from(a[c]) - i32::from(b[c])).abs()).max().unwrap_or(0);
                    if d > 0 {
                        n += 1;
                        max = max.max(d);
                    }
                }
                if n == 0 && r == &px {
                    eprintln!("variant {variant:?} vs ref, view {k}: byte-identical");
                } else {
                    eprintln!("variant {variant:?} vs ref, view {k}: {n} pixels differ ({:.2}%), max {max} levels", 100.0 * n as f64 / (w * h) as f64);
                }
            }
        }
        let f = std::fs::File::create(&path).expect("png");
        let mut enc = png::Encoder::new(std::io::BufWriter::new(f), w, h);
        enc.set_color(png::ColorType::Rgba);
        enc.set_depth(png::BitDepth::Eight);
        enc.write_header().expect("png").write_image_data(&px).expect("png");
        let exposure = read_half(&gpu, host.exposure_texture());
        eprintln!("{}: camera {eye:?} {angles:?}, exposure {exposure:.3}, {:.1}s", path.display(), t.elapsed().as_secs_f32());
    }
      for (k, old) in saved {
          // SAFETY: as above.
          unsafe {
              match old {
                  Some(v) => std::env::set_var(&k, v),
                  None => std::env::remove_var(&k),
              }
          }
      }
      render_iw6::switch::refresh();
    }
    for n in host.quality_notes.drain(..) {
        eprintln!("{n}");
    }
    eprintln!("{}", host.fps_line());
    let r = &host.renderer.report;
    eprintln!("{}", host.coverage_line());
    for (k, n) in &r.skipped {
        eprintln!("skipped {n}: {k}");
    }
    for (k, e) in r.failed.iter().take(20) {
        eprintln!("failed {k}: {e}");
    }
    eprintln!("missing code constants: {:?}", r.missing_code);
    let missing: Vec<_> = r.missing_images.iter().take(8).collect();
    eprintln!("missing pictures ({}): {missing:?}", r.missing_images.len());
    for e in &host.post_errors {
        eprintln!("post: {e}");
    }
}

fn read_rgba8(gpu: &Gpu, tex: &wgpu::Texture, w: u32, h: u32) -> Vec<u8> {
    let row = (w * 4).div_ceil(256) * 256;
    let buf = gpu.device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("readback"),
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
    let mut out = Vec::with_capacity((w * h * 4) as usize);
    for y in 0..h {
        let at = (y * row) as usize;
        out.extend_from_slice(&data[at..at + (w * 4) as usize]);
    }
    out
}

fn read_depth(gpu: &Gpu, tex: &wgpu::Texture, w: u32, h: u32) -> Vec<f32> {
    let row = (w * 4).div_ceil(256) * 256;
    let buf = gpu.device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("readback_d"),
        size: u64::from(row * h),
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut enc = gpu.device.create_command_encoder(&Default::default());
    enc.copy_texture_to_buffer(
        wgpu::TexelCopyTextureInfo {
            texture: tex,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::DepthOnly,
        },
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
        let at = (y * row) as usize;
        out.extend(data[at..at + (w * 4) as usize].chunks_exact(4).map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]])));
    }
    out
}

fn read_half(gpu: &Gpu, tex: &wgpu::Texture) -> f32 {
    let buf = gpu.device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("readback_e"),
        size: 256,
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
                bytes_per_row: Some(256),
                rows_per_image: Some(1),
            },
        },
        wgpu::Extent3d {
            width: 1,
            height: 1,
            depth_or_array_layers: 1,
        },
    );
    gpu.queue.submit([enc.finish()]);
    let slice = buf.slice(..);
    slice.map_async(wgpu::MapMode::Read, |_| {});
    let _ = gpu.device.poll(wgpu::PollType::wait_indefinitely());
    let d = slice.get_mapped_range();
    let h = u16::from_le_bytes([d[0], d[1]]);
    let e = (h >> 10) & 0x1f;
    let m = f32::from(h & 0x3ff);
    if e == 0 { m * 2f32.powi(-24) } else { (1.0 + m / 1024.0) * 2f32.powi(i32::from(e) - 15) }
}

/// A grey box in front of the camera: the stand-in for a player or the view
/// gun (`IW6_SHOT_PLACEHOLDER`).
struct Placeholder {
    graded: bool,
    at: [f32; 3],
    pipeline: wgpu::RenderPipeline,
    layout: wgpu::BindGroupLayout,
}

const PLACEHOLDER_WGSL: &str = r#"
struct U {
    view_projection: mat4x4<f32>,
    centre: vec4<f32>,      // eye-relative centre, w half size
    sun_dir: vec4<f32>,     // toward the sun, w: 1 to sRGB-encode
    sun_colour: vec4<f32>,
    ambient: vec4<f32>,
};
@group(0) @binding(0) var<uniform> u: U;

struct V {
    @builtin(position) clip: vec4<f32>,
    @location(0) rel: vec3<f32>,
};

@vertex
fn vs(@builtin(vertex_index) i: u32) -> V {
    // 12 triangles of a unit cube.
    var idx = array<u32, 36>(0u,2u,1u, 1u,2u,3u, 4u,5u,6u, 5u,7u,6u, 0u,1u,4u, 1u,5u,4u,
                             2u,6u,3u, 3u,6u,7u, 0u,4u,2u, 2u,4u,6u, 1u,3u,5u, 3u,7u,5u);
    let c = idx[i];
    let p = vec3<f32>(f32(c & 1u), f32((c >> 1u) & 1u), f32((c >> 2u) & 1u)) * 2.0 - 1.0;
    let rel = u.centre.xyz + p * u.centre.w;
    var o: V;
    o.clip = u.view_projection * vec4<f32>(rel, 1.0);
    o.rel = rel;
    return o;
}

@fragment
fn fs(v: V) -> @location(0) vec4<f32> {
    let n = normalize(cross(dpdx(v.rel), dpdy(v.rel)));
    let sun = max(dot(n, normalize(u.sun_dir.xyz)), 0.0);
    var c = vec3<f32>(0.25) * (u.sun_colour.xyz * sun + u.ambient.xyz);
    if (u.sun_dir.w > 0.5) {
        c = select(1.055 * pow(c, vec3<f32>(1.0 / 2.4)) - 0.055, c * 12.92, c <= vec3<f32>(0.0031308));
    }
    return vec4<f32>(c, 0.0);
}
"#;

impl Placeholder {
    fn new(gpu: &Gpu, graded: bool, at: [f32; 3]) -> Self {
        let module = gpu.device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("placeholder"),
            source: wgpu::ShaderSource::Wgsl(PLACEHOLDER_WGSL.into()),
        });
        let layout = gpu.device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("placeholder"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });
        let pl = gpu.device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("placeholder"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let format = if graded { render_iw6::world::SCENE_COLOUR } else { wgpu::TextureFormat::Rgba8Unorm };
        let pipeline = gpu.device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("placeholder"),
            layout: Some(&pl),
            vertex: wgpu::VertexState {
                module: &module,
                entry_point: Some("vs"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            fragment: Some(wgpu::FragmentState {
                module: &module,
                entry_point: Some("fs"),
                compilation_options: Default::default(),
                // (As the game's exact pass into the HDR scene: the world's
                // alpha, its bloom mask, is left as it is.)
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: None,
                    write_mask: wgpu::ColorWrites::COLOR,
                })],
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: Some(wgpu::DepthStencilState {
                format: wgpu::TextureFormat::Depth32Float,
                depth_write_enabled: Some(true),
                depth_compare: Some(wgpu::CompareFunction::GreaterEqual),
                stencil: Default::default(),
                bias: Default::default(),
            }),
            multisample: Default::default(),
            multiview_mask: None,
            cache: None,
        });
        Self { graded, at, pipeline, layout }
    }

    #[allow(clippy::too_many_arguments)]
    fn draw(&self, gpu: &Gpu, enc: &mut wgpu::CommandEncoder, colour: &wgpu::TextureView, depth: &wgpu::TextureView, camera: &Camera, sun: (Vec3, Vec3), grid: Option<&fastfile_iw6::lightgrid::LightGrid>) {
        let [dist, size, down] = self.at;
        let fwd = camera.rotation.row(2).truncate().normalize();
        let centre = fwd * dist - Vec3::Z * down;
        let world = camera.eye + centre;
        let ambient = grid
            .and_then(|g| g.sample(world.to_array()))
            .map_or([0.3; 3], |sh| fastfile_iw6::lightgrid::mean_light(&sh));
        eprintln!("iw6_shot: placeholder at {world:?}: grid mean light {ambient:?}, sun {:?}", sun.1);
        let mut data: Vec<f32> = camera.view_projection.to_cols_array().to_vec();
        data.extend([centre.x, centre.y, centre.z, size * 0.5]);
        data.extend([sun.0.x, sun.0.y, sun.0.z, if self.graded { 0.0 } else { 1.0 }]);
        data.extend([sun.1.x, sun.1.y, sun.1.z, 0.0]);
        data.extend([ambient[0], ambient[1], ambient[2], 0.0]);
        let buf = gpu.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("placeholder"),
            size: (data.len() * 4) as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        gpu.queue.write_buffer(&buf, 0, bytemuck::cast_slice(&data));
        let bg = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("placeholder"),
            layout: &self.layout,
            entries: &[wgpu::BindGroupEntry { binding: 0, resource: buf.as_entire_binding() }],
        });
        let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("placeholder"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: colour,
                resolve_target: None,
                depth_slice: None,
                ops: wgpu::Operations { load: wgpu::LoadOp::Load, store: wgpu::StoreOp::Store },
            })],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: depth,
                depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Load, store: wgpu::StoreOp::Store }),
                stencil_ops: None,
            }),
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &bg, &[]);
        pass.draw(0..36, 0..1);
    }
}

/// `IW6_SHOT_ANIM`: skins a placed script model by an animation of the map zone.
fn pose_animation(host: &mut Host, zones: &[PathBuf], spec: &str) {
    let p: Vec<&str> = spec.split(',').collect();
    let (model, clip_name) = (p[0].to_ascii_lowercase(), p[1].to_ascii_lowercase());
    let at: Option<[f32; 3]> = (p.len() >= 6).then(|| [p[3].parse().unwrap_or(0.0), p[4].parse().unwrap_or(0.0), p[5].parse().unwrap_or(0.0)]);
    let yaw: f32 = p.get(6).and_then(|v| v.parse().ok()).unwrap_or(0.0);
    for z in zones.iter().filter(|z| z.file_name().is_some_and(|n| n.to_string_lossy().starts_with("mp_"))) {
        let Ok(zone) = fastfile_iw6::Zone::open(z) else { continue };
        let (cap, _) = fastfile_iw6::walk::walk_zone(&zone);
        let strings = &zone.assets.script_strings;
        let (clips, _) = fastfile_iw6::anim::clips(&cap, &zone.image, strings);
        let Some(clip) = clips.iter().find(|c| c.name.eq_ignore_ascii_case(&clip_name)) else { continue };
        let Some(skel) = cap
            .xmodels
            .iter()
            .filter(|m| m.name.eq_ignore_ascii_case(&model))
            .find_map(|m| fastfile_iw6::anim::skeleton(m, strings))
        else {
            eprintln!("iw6_shot: no skeleton for {model}");
            return;
        };
        let secs = if p[2] == "end" { clip.length_ms() as f32 / 1000.0 } else { p[2].parse().unwrap_or(0.0) };
        let posed = iw6_pose::pose(&skel, Some(clip), secs);
        let skin = iw6_pose::skin(&skel, &posed);
        let keys: Vec<render_iw6::scene::MoverKey> = host
            .renderer
            .scene
            .movers
            .iter()
            .map(|m| m.key.clone())
            .filter(|k| matches!(k, render_iw6::scene::MoverKey::Model { name, origin } if name.eq_ignore_ascii_case(&model) && at.is_none_or(|a| (0..3).all(|i| (a[i] - origin[i]).abs() < 1.0))))
            .collect();
        for k in &keys {
            let render_iw6::scene::MoverKey::Model { origin, .. } = k else { continue };
            let done = host.set_mover_skinned(k, *origin, [0.0, yaw, 0.0], false, &skin);
            eprintln!("iw6_shot: {model} at {origin:?} skinned by {} at {secs:.2}s ({} bones): {done}", clip.name, skin.len());
        }
        if keys.is_empty() {
            eprintln!("iw6_shot: no placed {model}");
        }
        return;
    }
    eprintln!("iw6_shot: no animation {clip_name} in the map zone");
}

/// `IW6_SHOT_CARRY`: a model linked to a tag of an animated carrier.
fn pose_carried(host: &mut Host, zones: &[PathBuf], spec: &str) {
    let p: Vec<&str> = spec.split(',').collect();
    let num = |i: usize| p.get(i).and_then(|v| v.parse::<f32>().ok()).unwrap_or(0.0);
    let (carrier, clip_name, tag, child) = (p[0].to_ascii_lowercase(), p[1].to_ascii_lowercase(), p[7].to_ascii_lowercase(), p[8].to_ascii_lowercase());
    let at = ([num(3), num(4), num(5)], [0.0, num(6), 0.0]);
    let home = [num(9), num(10), num(11)];
    for z in zones.iter().filter(|z| z.file_name().is_some_and(|n| n.to_string_lossy().starts_with("mp_"))) {
        let Ok(zone) = fastfile_iw6::Zone::open(z) else { continue };
        let (cap, _) = fastfile_iw6::walk::walk_zone(&zone);
        let strings = &zone.assets.script_strings;
        let (clips, _) = fastfile_iw6::anim::clips(&cap, &zone.image, strings);
        let Some(clip) = clips.iter().find(|c| c.name.eq_ignore_ascii_case(&clip_name)) else { continue };
        let Some(skel) = cap.xmodels.iter().filter(|m| m.name.eq_ignore_ascii_case(&carrier)).find_map(|m| fastfile_iw6::anim::skeleton(m, strings)) else {
            eprintln!("iw6_shot: no skeleton for {carrier}");
            return;
        };
        let Some(b) = skel.bones.iter().position(|n| n.eq_ignore_ascii_case(&tag)) else {
            eprintln!("iw6_shot: {carrier} has no tag {tag}");
            return;
        };
        let secs = if p[2] == "end" { clip.length_ms() as f32 / 1000.0 } else { p[2].parse().unwrap_or(0.0) };
        let root = (iw6_pose::quat_of_axes(iw6_pose::axes_of_angles(at.1)), at.0);
        let bone = |t: f32| iw6_pose::compose(root, iw6_pose::pose(&skel, Some(clip), t)[b]);
        let child_at = (iw6_pose::quat_of_axes(iw6_pose::axes_of_angles([0.0; 3])), home);
        let local = iw6_pose::compose(iw6_pose::invert(bone(0.0)), child_at);
        let w = iw6_pose::compose(bone(secs), local);
        let angles = iw6_pose::angles_of_axes(iw6_pose::axes(w.0));
        let key = render_iw6::scene::MoverKey::Model { name: child.clone(), origin: home };
        let done = host.set_mover_pose(&key, w.1, angles);
        eprintln!("iw6_shot: {child} on {carrier}.{tag} at {secs:.2}s: origin {:?} angles {angles:?}: {done}", w.1);
        return;
    }
    eprintln!("iw6_shot: no animation {clip_name} in the map zone");
}
