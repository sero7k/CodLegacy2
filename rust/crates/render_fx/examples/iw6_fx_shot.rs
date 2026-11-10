//! Offline Ghosts effect harness: plays a Ghosts map's effects through the
//! game's own effect system (`fx::FxSystemHost`, `render_fx::present::build_fx_verts`
//! with the Ghosts rules, the converted catalog of `assets`' createfx lane) and
//! writes PNGs and a text dump, headlessly.
//!
//! `cargo run -p render_fx --example iw6_fx_shot -- <zone dir> <map> [options]`
//!
//! * `--effect <part>` (repeatable): the effects whose path holds the part. None:
//!   every effect the map's createfx places.
//! * `--paths <file>`: also the effects named in the file (the scripts' literals).
//! * `--isolated` (default): one effect at its origin, frames at `--times`
//!   (default `0.1,0.5,1,2,4` s); `--placed`: every createfx placement played
//!   together (a frame at each time).
//! * `--world`: draws the map under the effects (render_iw6's native frame, its
//!   depth testing the sprites), at the effect's first placement (`--dist`
//!   metres of units behind it); otherwise a flat backdrop at the origin.
//! * `--cam x y z pitch yaw`: a fixed camera. `--size WxH`, `--out <dir>`,
//!   `--text-only`: no pictures.
//!
//! The sprites are composited on the CPU in display values over the picture, by
//! the material's own blend (add, alpha, premultiplied) in sort-key then
//! far-to-near order, with the effect's picture where the pak is staged and a
//! soft disc where it is not (`FALLBACK` in the dump). This is the shape and
//! placement of what the game draws, not its shading (the game's own pass
//! draws these quads through the converted material).
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::PathBuf;

use asset_audio::CreateFxOneshot;
use asset_core::AssetNamespace;
use asset_game::{FxCatalog, FxName};
use fastfile_iw6::fxview::{OwnedFx, OwnedVisual};
use fastfile_iw6::image::{ImagePaks, image_pixels};
use fastfile_iw6::shared::{SHARED_ZONES, SharedImages, image_key};
use glam::{Mat4, Vec3};
use render_fx::present::{FxDrawCull, FxElemInfoCache, boot_createfx_effects, build_fx_verts, tick_fx_non_dependent, tick_fx_remaining};

/// The lane's per-material shading (`fx_iw4::iw6::material`) for a material
/// of the harness, registered as the game's lane registers it.
fn register_fx_material(name: &str, techset: &str, constants: &[fastfile_iw6::walk::MaterialConstant]) {
    let fx = fx_iw4::iw6::material::read(techset, |want| {
        let want = &want[..want.len().min(12)];
        constants.iter().find(|c| c.name.as_str().get(..c.name.len().min(12)) == Some(want)).map(|c| c.literal)
    });
    asset_game::set_iw6_fx_material(name, fx);
}

struct Mat {
    index: usize,
    techset: String,
    state: Vec<[u8; 28]>,
    sort_key: u8,
    colour: Option<String>,
}

#[derive(Clone)]
struct Tex {
    w: usize,
    h: usize,
    rgba: Vec<u8>,
}

fn decode(px: &fastfile_iw6::image::ImagePixels) -> Option<Tex> {
    let (w, h) = (px.width as usize, px.height as usize);
    let mut out = vec![0u8; w * h * 4];
    let d = &px.data;
    match px.dxgi_format {
        28 | 29 => out.copy_from_slice(d.get(..w * h * 4)?),
        87 | 91 => {
            for i in 0..w * h {
                let s = d.get(i * 4..i * 4 + 4)?;
                out[i * 4..i * 4 + 4].copy_from_slice(&[s[2], s[1], s[0], s[3]]);
            }
        }
        65 => {
            for i in 0..w * h {
                let a = *d.get(i)?;
                out[i * 4..i * 4 + 4].copy_from_slice(&[255, 255, 255, a]);
            }
        }
        61 => {
            for i in 0..w * h {
                let a = *d.get(i)?;
                out[i * 4..i * 4 + 4].copy_from_slice(&[a, a, a, 255]);
            }
        }
        70..=72 | 73..=75 | 76..=78 | 79..=81 | 97..=99 => {
            let block = if matches!(px.dxgi_format, 70..=72 | 79..=81) { 8 } else { 16 };
            let (bw, bh) = (w.div_ceil(4), h.div_ceil(4));
            if d.len() < bw * bh * block {
                return None;
            }
            let mut tmp = vec![0u8; bw * 4 * bh * 4 * 4];
            for by in 0..bh {
                for bx in 0..bw {
                    let b = &d[(by * bw + bx) * block..(by * bw + bx) * block + block];
                    let at = (by * 4 * bw * 4 + bx * 4) * 4;
                    let pitch = bw * 4 * 4;
                    match px.dxgi_format {
                        70..=72 => bcdec_rs::bc1(b, &mut tmp[at..], pitch),
                        73..=75 => bcdec_rs::bc2(b, &mut tmp[at..], pitch),
                        76..=78 => bcdec_rs::bc3(b, &mut tmp[at..], pitch),
                        97..=99 => bcdec_rs::bc7(b, &mut tmp[at..], pitch),
                        _ => {
                            let mut grey = [0u8; 16];
                            bcdec_rs::bc4(b, &mut grey, 4, px.dxgi_format == 81);
                            for y in 0..4 {
                                for x in 0..4 {
                                    let g = grey[y * 4 + x];
                                    let o = at + y * pitch + x * 4;
                                    tmp[o..o + 4].copy_from_slice(&[g, g, g, 255]);
                                }
                            }
                        }
                    }
                }
            }
            for y in 0..h {
                let src = &tmp[y * bw * 16..y * bw * 16 + w * 4];
                out[y * w * 4..(y + 1) * w * 4].copy_from_slice(src);
            }
        }
        _ => return None,
    }
    Some(Tex { w, h, rgba: out })
}

struct Lookup {
    map_fx: HashMap<String, OwnedFx>,
    map_base: HashMap<String, String>,
    shared: SharedImages,
}
impl Lookup {
    fn effect(&self, name: &str) -> Option<OwnedFx> {
        let key = name.trim_start_matches(',').to_ascii_lowercase();
        if let Some(f) = self.map_fx.get(&key) {
            return Some(f.clone());
        }
        let base = key.rsplit('/').next().unwrap_or(&key);
        if let Some(f) = self.map_base.get(base).and_then(|k| self.map_fx.get(k)) {
            return Some(f.clone());
        }
        self.shared.effect(name).cloned()
    }
}

fn blend_of(state: &[[u8; 28]]) -> &'static str {
    // The last technique slot's state is the lit/final pass; effects have one.
    let Some(b) = state.first() else { return "alpha" };
    let w2 = u32::from_le_bytes([b[8], b[9], b[10], b[11]]);
    let (s, d, op) = (w2 & 0xf, (w2 >> 4) & 0xf, (w2 >> 8) & 7);
    match (op, s, d) {
        (0, _, _) => "opaque",
        (_, 2, 2) => "add",
        (_, 5, 2) => "add",
        // INV_DEST_COLOR / ONE: screen (the colour times its alpha, where the frame is dark).
        (_, 10, 2) => "screen",
        (_, 5, 6) => "alpha",
        (_, 2, 6) => "premult",
        _ => "alpha",
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let dir = PathBuf::from(&args[0]);
    let map = args[1].clone();
    let mut want: Vec<String> = Vec::new();
    let mut paths_file: Option<PathBuf> = None;
    let mut times: Vec<f32> = vec![0.1, 0.5, 1.0, 2.0, 4.0];
    let (mut placed_mode, mut world, mut text_only) = (false, false, false);
    let (mut w, mut h) = (960u32, 540u32);
    let mut out_dir = PathBuf::from("fxshots");
    let mut cam: Option<(Vec3, Vec3)> = None;
    let mut dist = 220.0f32;
    let mut k = 2;
    while k < args.len() {
        match args[k].as_str() {
            "--effect" => {
                want.push(args[k + 1].to_ascii_lowercase());
                k += 1;
            }
            "--paths" => {
                paths_file = Some(args[k + 1].clone().into());
                k += 1;
            }
            "--times" => {
                times = args[k + 1].split(',').filter_map(|s| s.parse().ok()).collect();
                k += 1;
            }
            "--placed" => placed_mode = true,
            "--isolated" => placed_mode = false,
            "--world" => world = true,
            "--text-only" => text_only = true,
            "--dist" => {
                dist = args[k + 1].parse().expect("dist");
                k += 1;
            }
            "--size" => {
                let (a, b) = args[k + 1].split_once('x').expect("WxH");
                w = a.parse().expect("w");
                h = b.parse().expect("h");
                k += 1;
            }
            "--out" => {
                out_dir = args[k + 1].clone().into();
                k += 1;
            }
            "--cam" => {
                let v: Vec<f32> = args[k + 1..k + 6].iter().map(|s| s.parse().expect("cam")).collect();
                cam = Some((Vec3::new(v[0], v[1], v[2]), Vec3::new(v[3], v[4], 0.0)));
                k += 5;
            }
            _ => {}
        }
        k += 1;
    }
    std::fs::create_dir_all(&out_dir).ok();

    // --- zones ---
    let zone_path = dir.join(format!("{map}.ff"));
    let zone = fastfile_iw6::Zone::open(&zone_path).expect("zone");
    let (cap, _) = fastfile_iw6::walk::walk_zone(&zone);
    let cfx = fastfile_iw6::createfx::read(&cap, &zone.image).expect("createfx");
    let mut shared = SharedImages::default();
    for z in SHARED_ZONES {
        let p = dir.join(format!("{z}.ff"));
        if p.is_file() {
            let _ = shared.add_zone(&p);
        }
    }
    let mut map_fx: HashMap<String, OwnedFx> = HashMap::new();
    let mut map_base: HashMap<String, String> = HashMap::new();
    for fr in &cap.fx {
        if fr.name.starts_with(',') || fr.elems.is_empty() {
            continue;
        }
        let key = fr.name.to_ascii_lowercase();
        map_base.entry(key.rsplit('/').next().unwrap_or(&key).to_owned()).or_insert_with(|| key.clone());
        map_fx.entry(key).or_insert_with(|| fastfile_iw6::fxview::own(fr, &cap, &zone.image));
    }
    let lk = Lookup { map_fx, map_base, shared };

    // --- the effect set: placed, scripted, wanted, and what they run ---
    let mut roots: Vec<(String, String)> = Vec::new();
    let mut placements: BTreeMap<String, Vec<([f32; 3], [f32; 3])>> = BTreeMap::new();
    for p in &cfx.placed {
        let path = cfx.paths.get(&p.fxid).cloned().unwrap_or_else(|| p.fxid.clone());
        placements.entry(path.to_ascii_lowercase()).or_default().push((p.origin, p.angles));
    }
    if std::env::var("IW6_SHOT_LIST").is_ok() {
        // Every createfx placement: effect, origin, angles (for choosing cameras).
        for (path, v) in &placements {
            for (o, a) in v {
                println!("PLACEMENT {path} {:.0} {:.0} {:.0} angles {:.0} {:.0} {:.0}", o[0], o[1], o[2], a[0], a[1], a[2]);
            }
        }
    }
    if want.is_empty() || placed_mode {
        for path in placements.keys() {
            roots.push((path.clone(), "createfx".into()));
        }
    }
    if let Some(pf) = &paths_file {
        for l in std::fs::read_to_string(pf).expect("paths").lines() {
            let l = l.trim();
            if l.starts_with("fx/") || l.starts_with("vfx/") {
                roots.push((l.to_ascii_lowercase(), "script".into()));
            }
        }
    }
    let mut found: BTreeMap<String, OwnedFx> = BTreeMap::new();
    let mut queue: Vec<String> = roots.iter().map(|r| r.0.clone()).collect();
    // `--effect` parts select among everything the zones hold.
    for part in &want {
        let mut names: Vec<String> = lk.map_fx.keys().filter(|k| k.contains(part.as_str())).cloned().collect();
        names.extend(placements.keys().filter(|k| k.contains(part.as_str())).cloned());
        queue.extend(names);
    }
    while let Some(n) = queue.pop() {
        let Some(fx) = lk.effect(&n) else { continue };
        let key = fx.name.to_ascii_lowercase();
        if found.contains_key(&key) {
            continue;
        }
        for el in &fx.elems {
            for c in el
                .visuals
                .iter()
                .filter_map(|v| if let OwnedVisual::Effect(n) = v { Some(n) } else { None })
                .chain([&el.effect_on_impact, &el.effect_on_death, &el.effect_emitted].into_iter().flatten())
            {
                queue.push(c.clone());
            }
        }
        found.insert(key, fx);
    }
    eprintln!("{} effects in the set", found.len());

    // --- materials ---
    let mut mats: HashMap<String, Mat> = HashMap::new();
    let mut order: Vec<String> = Vec::new();
    for m in &cap.materials {
        let Some(ts) = m.techset else { continue };
        let key = image_key(&m.name);
        if mats.contains_key(&key) {
            continue;
        }
        if std::env::var("IW6_SHOT_TEXLIST").map_or(false, |v| m.name.contains(&v)) {
            for t in &m.textures {
                eprintln!("material {} texture semantic {} hash {:08x} image {:?}", m.name, t.semantic, t.name_hash, t.image.and_then(|i| cap.images.get(i.index as usize)).map(|im| im.name.clone()));
            }
        }
        let colour = m
            .textures
            .iter()
            // The `colorMap` sampler (as the game lane picks it); a material can carry
            // more than one colour-class picture (a vector field, a gradient).
            .find(|t| t.name_hash == 0xa0ab_1041 && t.image.is_some())
            .or_else(|| m.textures.iter().find(|t| t.semantic == 2 && t.image.is_some()))
            .and_then(|t| cap.images.get(t.image?.index as usize))
            .map(|im| im.name.clone());
        register_fx_material(&m.name, cap.name_of(ts), &m.constants);
        mats.insert(key.clone(), Mat { index: order.len(), techset: cap.name_of(ts).trim_start_matches(',').to_owned(), state: m.state_bits.clone(), sort_key: m.sort_key, colour });
        order.push(key);
    }
    let mut paks = ImagePaks::new(&dir);
    let mut tex_cache: HashMap<String, Option<Tex>> = HashMap::new();
    let mut tex_of = |mats: &HashMap<String, Mat>, key: &str, paks: &mut ImagePaks| -> (Option<Tex>, String) {
        let Some(pic) = mats.get(key).and_then(|m| m.colour.clone()) else { return (None, "no colour picture".into()) };
        let pk = image_key(&pic);
        if let Some(t) = tex_cache.get(&pk) {
            return (t.clone(), if t.is_some() { "loaded".into() } else { "FALLBACK".into() });
        }
        let from_map = cap.images.iter().find(|im| image_key(&im.name) == pk && !fastfile_iw6::shared::is_reference(im));
        let r = if let Some(rec) = from_map {
            image_pixels(rec, &zone.image, paks, 512).ok()
        } else if lk.shared.get(&pk).is_some() {
            lk.shared.pixels(&pk, paks, 512).ok()
        } else {
            fastfile_iw6::shared::builtin_pixels(&pk)
        };
        if std::env::var("IW6_SHOT_DUMPTEX").is_ok() {
            if let Some(px) = r.as_ref() {
                eprintln!("image {pk}: {}x{} dxgi {} bytes {}", px.width, px.height, px.dxgi_format, px.data.len());
            }
        }
        let t = r.as_ref().and_then(decode);
        tex_cache.insert(pk, t.clone());
        let status = if t.is_some() { "loaded" } else { "FALLBACK" };
        (t, status.into())
    };
    let fx_list: Vec<OwnedFx> = found.values().cloned().collect();
    let mut linked = |name: &str| -> Option<String> {
        let key = image_key(name);
        if mats.contains_key(&key) {
            Some(name.trim_start_matches(',').to_owned())
        } else if let Some(sm) = lk.shared.material(name) {
            // A shared material: add it too.
            let idx = order.len();
            let colour = sm.textures.iter().find(|t| t.2 == 2).map(|t| t.3.clone());
            register_fx_material(&sm.name, &sm.techset, &sm.constants);
            mats.insert(key.clone(), Mat { index: idx, techset: sm.techset.clone(), state: Vec::new(), sort_key: sm.sort_key, colour });
            order.push(key);
            Some(name.trim_start_matches(',').to_owned())
        } else {
            None
        }
    };
    if std::env::var("IW6_SHOT_ELEMS").is_ok() {
        // Per element: the raw type, flare bytes, visuals (what the walker kept).
        for f in &fx_list {
            for (i, el) in f.elems.iter().enumerate() {
                let t = el.record.get(fastfile_iw6::layout::FxElemDef::elemType).copied().unwrap_or(255);
                println!("ELEM {} e{i}: type {t}, flare bytes {}, extended {}, visuals {:?}", f.name, el.flare.len(), el.extended.len(), el.visuals);
            }
        }
    }
    let (views, tally) = assets::iw6_fx_harness_views(&fx_list, &mut linked);
    let mut catalog = FxCatalog::default();
    catalog.set_capture_ns(AssetNamespace::T6);
    let mut captured = 0;
    for v in &views {
        if catalog.capture_s1_as(AssetNamespace::T6, AssetNamespace::Iw6, v) {
            captured += 1;
        }
    }
    let mut defs = catalog.publish();
    defs.set_map_namespace(AssetNamespace::T6);
    defs.resolve_nested_edges();
    let by_key: HashMap<String, usize> = order.iter().enumerate().map(|(i, k)| (k.clone(), i)).collect();
    defs.bind_materials_with(|name| by_key.get(&image_key(name)).copied());
    eprintln!("{captured} of {} effects captured", views.len());
    let mut cache = FxElemInfoCache::default();
    cache.sync(&defs);

    // --- the world, when asked ---
    let mut world_frame = if world && !text_only { Some(WorldFrame::new(&dir, &map, w, h)) } else { None };

    // --- play ---
    let targets: Vec<String> = if want.is_empty() || placed_mode {
        found.keys().cloned().collect()
    } else {
        found.keys().filter(|k| want.iter().any(|p| k.contains(p.as_str()))).cloned().collect()
    };
    let jobs: Vec<(String, Vec<([f32; 3], [f32; 3])>)> = if placed_mode {
        vec![("placed".into(), placements.values().flatten().cloned().collect::<Vec<_>>())]
            .into_iter()
            .collect()
    } else {
        targets
            .iter()
            .map(|k| {
                let place = placements.get(k).and_then(|v| v.first().copied());
                let o = if world { place.unwrap_or(([0.0; 3], [270.0, 0.0, 0.0])) } else { ([0.0, 0.0, 0.0], place.map_or([270.0, 0.0, 0.0], |p| p.1)) };
                (k.clone(), vec![o])
            })
            .collect()
    };
    let mut sum_rows: Vec<String> = Vec::new();
    for (name, spots) in jobs {
        let mut host = fx::FxSystemHost::new();
        let start = 1000i32;
        host.set_msec_now(start);
        let (centre, angles) = spots[0];
        let eye_cam = cam.unwrap_or_else(|| {
            // Behind the effect's centre, looking at it from the side, a little above.
            let eye = Vec3::from_array(centre) + Vec3::new(-dist, 0.0, 40.0);
            (eye, Vec3::new(8.0, 0.0, 0.0))
        });
        let cam_pos = eye_cam.0.to_array();
        if placed_mode {
            let shots: Vec<CreateFxOneshot> = cfx
                .placed
                .iter()
                .filter_map(|p| {
                    let path = cfx.paths.get(&p.fxid).cloned().unwrap_or_else(|| p.fxid.clone());
                    let fx = lk.effect(&path)?;
                    Some(CreateFxOneshot { fxid: fx.name.clone(), origin_inches: p.origin, angles_deg: p.angles, delay: 0.0, repeat_secs: 0.0 })
                })
                .collect();
            let census = boot_createfx_effects(&mut host, &defs, &cache, &shots, start, None);
            eprintln!("placed: {} named, {} held, {} failed {:?}", census.named, census.held, census.failed.len(), census.failed.iter().take(3).collect::<Vec<_>>());
        } else {
            let fxn = found[&name].name.clone();
            let shot = CreateFxOneshot { fxid: fxn, origin_inches: centre, angles_deg: angles, delay: 0.0, repeat_secs: 0.0 };
            let census = boot_createfx_effects(&mut host, &defs, &cache, &[shot], start, None);
            if census.named == 0 {
                println!("== {name}: not in the catalog (nothing to play)");
                continue;
            }
        }
        println!("== {name} at {centre:?} angles {angles:?}, camera {:?}", eye_cam);
        let mut now = start;
        for &t in &times {
            let target = start + (t * 1000.0) as i32;
            let (mut tick_us, mut ticks) = (0u128, 0u32);
            while now < target {
                now = (now + 16).min(target);
                let t0 = std::time::Instant::now();
                tick_fx_non_dependent(&mut host, &defs, &cache, now, cam_pos, None, None);
                tick_fx_remaining(&mut host, &defs, &cache, now, cam_pos, None, None);
                tick_us += t0.elapsed().as_micros();
                ticks += 1;
            }
            let b0 = std::time::Instant::now();
            // The view's frustum planes (unit normals, inside positive), as the game hands the
            // draw cull; `IW6_SHOT_CULL=0` leaves them out.
            let planes: Vec<[f32; 4]> = if std::env::var("IW6_SHOT_CULL").is_ok_and(|v| v == "0") {
                Vec::new()
            } else {
                let (eye, ang) = eye_cam;
                let view = render_h1::code::View { eye, angles: ang, fov_x: 80.0, width: w, height: h, near: 4.0 };
                let cam = render_h1::code::Camera::from(&view);
                let (r, u, f) = (cam.rotation.row(0).truncate(), cam.rotation.row(1).truncate(), cam.rotation.row(2).truncate());
                [r + f * cam.tan_x, -r + f * cam.tan_x, -u + f * cam.tan_y, u + f * cam.tan_y, f]
                    .iter()
                    .map(|n| {
                        let n = n.normalize();
                        [n.x, n.y, n.z, -n.dot(eye)]
                    })
                    .collect()
            };
            {
                // The camera the flare handler places screen-space quads by.
                let (eye, ang) = eye_cam;
                let view = render_h1::code::View { eye, angles: ang, fov_x: 80.0, width: w, height: h, near: 4.0 };
                let cam = render_h1::code::Camera::from(&view);
                let (r, u, f) = (cam.rotation.row(0).truncate(), cam.rotation.row(1).truncate(), cam.rotation.row(2).truncate());
                asset_world::set_iw6_fx_view(Some(asset_world::Iw6FxView {
                    eye: eye.to_array(),
                    right: r.to_array(),
                    up: u.to_array(),
                    forward: f.to_array(),
                    tan_x: cam.tan_x,
                    tan_y: cam.tan_y,
                }));
            }
            let (out, gaps) = build_fx_verts(&mut host, &defs, FxDrawCull { elem_draw: true, planes: &planes }, cam_pos);
            if std::env::var("IW6_SHOT_TIME").is_ok() {
                let quad_area: f32 = out.sprites.iter().map(|s| 4.0 * s.size0 * if s.size1 > 0.0 { s.size1 } else { s.size0 }).sum();
                println!(
                    "  TIME t={t:.2}s: sim {:.2} ms/tick over {ticks} ticks, build {:.2} ms for {} sprites ({} verts), quad area {:.3e} u2",
                    tick_us as f64 / 1000.0 / f64::from(ticks.max(1)), b0.elapsed().as_secs_f64() * 1000.0, out.sprites.len(), out.sprites.len() * 4, quad_area
                );
            }
            // ---- text ----
            println!(
                "  t={t:.2}s: {} sprites, {} trails, {} clouds, {} spark clouds, {} fountains, {} omni, {} spot, {} models; skipped dormant {} null {} unsupported {} cloud {} spark-cloud {} fountain {} model {} omni {} spot {}; gaps iw6 drawn {} not drawn {} culled {}, no material {}",
                out.sprites.len(), out.trail_meshes.len(), out.clouds.len(), out.spark_clouds.len(), out.fountains.len(), out.omni_lights.len(), out.spot_lights.len(), out.models.len(),
                out.skipped_dormant, out.skipped_null_handler, out.skipped_unsupported_type, out.skipped_cloud, out.skipped_spark_cloud, out.skipped_spark_fountain, out.skipped_model, out.skipped_omni_light, out.skipped_spot_light,
                gaps.iw6_drawn, gaps.iw6_not_drawn, gaps.iw6_culled, gaps.no_material_visual
            );
            let mut per: BTreeMap<(String, u8), Vec<&fx::FxSpriteInstance>> = BTreeMap::new();
            for s in &out.sprites {
                per.entry((s.def_name.to_string(), s.def_index)).or_default().push(s);
            }
            for ((dn, di), v) in &per {
                let n = v.len() as f32;
                let mean = |f: &dyn Fn(&fx::FxSpriteInstance) -> f32| v.iter().map(|s| f(s)).sum::<f32>() / n;
                let (mn, mx) = v.iter().fold((f32::MAX, 0f32), |a, s| (a.0.min(s.size0), a.1.max(s.size0)));
                let mat = &v[0].material_name;
                let key = image_key(mat);
                let blend = mats.get(&key).map_or("?", |m| blend_of(&m.state));
                println!(
                    "    {dn} e{di} type {} x{} half-size {mn:.1}..{mx:.1} (size1 {:.1}) rgba mean [{:.0} {:.0} {:.0} {:.0}] flags {:#x} mat {mat} ({blend}, sort {})",
                    v[0].elem_type, v.len(), mean(&|s| s.size1),
                    mean(&|s| f32::from(s.color_rgba[0])), mean(&|s| f32::from(s.color_rgba[1])), mean(&|s| f32::from(s.color_rgba[2])), mean(&|s| f32::from(s.color_rgba[3])),
                    v[0].flags, mats.get(&key).map_or(0, |m| m.sort_key)
                );
            }
            if std::env::var("IW6_SHOT_AXES").is_ok() {
                for s in out.sprites.iter().filter(|s| s.elem_type == 1).take(6) {
                    let angles = defs.def_at(s.catalog_index as usize).and_then(|d| d.elems.get(s.def_index as usize)).map(|e| (e.view.spawn_angles, e.view.angular_velocity));
                    println!(
                        "      oriented {} e{} origin {:.0?} axis {:.2?} {:.2?} {:.2?} rot {:.2} half {:.0}x{:.0} spawnAngles/angVel {:?}",
                        s.def_name, s.def_index, s.origin, s.axis[0], s.axis[1], s.axis[2], s.rotation_rad, s.size0, s.size1, angles
                    );
                }
            }
            if std::env::var("IW6_SHOT_TAILS").is_ok() {
                for s in out.sprites.iter().filter(|s| s.elem_type == 2).take(8) {
                    println!(
                        "      tail {} e{} origin {:.0?} vel_dir {:.2?} half {:.1}x{:.1} axis0 {:.2?}",
                        s.def_name, s.def_index, s.origin, s.vel_dir, s.size0, s.size1, s.axis[0]
                    );
                }
            }
            // Sanity: quads whose extent or aspect leave the bound (a billboard or
            // oriented sprite is 2*size0 by 2*size1; a tail 2*size0 by 4*size1).
            {
                let bound: f32 = std::env::var("IW6_SHOT_BOUND").ok().and_then(|v| v.parse().ok()).unwrap_or(600.0);
                let mut seen: BTreeSet<(String, u8)> = BTreeSet::new();
                for s in &out.sprites {
                    let h = if s.elem_type == 2 { s.size1 * 2.0 } else if s.size1 > 0.0 { s.size1 } else { s.size0 };
                    let (w, l) = (s.size0 * 2.0, h * 2.0);
                    let zero_dir = s.elem_type == 2 && s.vel_dir == [0.0; 3];
                    if (w.max(l) > bound || (w.min(l) > 0.0 && w.max(l) / w.min(l) > 12.0) || zero_dir) && seen.insert((s.def_name.to_string(), s.def_index)) {
                        println!(
                            "    SANITY {} e{} type {} quad {w:.0}x{l:.0} at {:.0?} zero_vel_tail {zero_dir} alpha {}",
                            s.def_name, s.def_index, s.elem_type, s.origin, s.color_rgba[3]
                        );
                    }
                }
            }
            for c in &out.clouds {
                println!("    cloud {} e{}", c.def_name, c.def_index);
            }
            for l in out.omni_lights.iter().chain(out.spot_lights.iter()) {
                println!("    light {} e{} r{:.0} rgb {:.2?}", l.def_name, l.def_index, l.radius, l.color_bgr);
            }
            for m in &out.models {
                println!("    model {} e{} #{} scale {:.2}", m.def_name, m.def_index, m.model_index, m.scale);
            }
            if t == times[0] {
                sum_rows.push(format!("{name}: {} sprites at {t}s", out.sprites.len()));
            }
            // ---- picture ----
            if text_only {
                continue;
            }
            let (eye, angles_cam) = eye_cam;
            let (mut rgb, depth, near) = match world_frame.as_mut() {
                Some(wf) => wf.shot(eye, angles_cam),
                None => (vec![[0.11f32, 0.12, 0.14]; (w * h) as usize], Vec::new(), 4.0),
            };
            let view = render_h1::code::View { eye, angles: angles_cam, fov_x: 80.0, width: w, height: h, near };
            let camera = render_h1::code::Camera::from(&view);
            // Quads: (sort key, distance, corners, colour, atlas, material key)
            struct Q {
                sort: u8,
                dist: f32,
                corners: [Vec3; 4],
                rgba: [f32; 4],
                atlas: [f32; 4],
                key: String,
            }
            let right = camera.rotation.row(0).truncate();
            let up = camera.rotation.row(1).truncate();
            let mut quads: Vec<Q> = Vec::new();
            for s in &out.sprites {
                let o = Vec3::from_array(s.origin);
                let height = if s.size1 > 0.0 { s.size1 } else { s.size0 };
                let (ax, ay) = if s.elem_type == 0 {
                    let (sn, cs) = s.rotation_rad.sin_cos();
                    ((right * cs + up * sn) * s.size0, (up * cs - right * sn) * height)
                } else if s.elem_type == 2 {
                    let anchor = fx_iw4::tail_anchor_origin(s.origin, s.vel_dir, s.size1);
                    let Some(axes) = fx_iw4::tail_sprite_axes(s.vel_dir, cam_pos, anchor) else { continue };
                    let r = Vec3::from_array(axes[0]).normalize_or_zero();
                    let u = Vec3::from_array(axes[1]).normalize_or_zero();
                    let c = Vec3::from_array(anchor);
                    let (hw, hh) = (fx_iw4::tail_sprite_full_extent(s.size0) * 0.5, fx_iw4::tail_sprite_full_extent(s.size1) * 0.5);
                    let corners = [c - r * hw - u * hh, c + r * hw - u * hh, c + r * hw + u * hh, c - r * hw + u * hh];
                    quads.push(Q { sort: 0, dist: (c - eye).length(), corners, rgba: s.color_rgba.map(|v| f32::from(v) / 255.0), atlas: [s.atlas.s0, s.atlas.ds, s.atlas.t0, s.atlas.dt], key: image_key(&s.material_name) });
                    continue;
                } else {
                    let r = Vec3::from_array(s.axis[1]).normalize_or_zero();
                    let u = Vec3::from_array(s.axis[2]).normalize_or_zero();
                    let (sn, cs) = s.rotation_rad.sin_cos();
                    ((r * cs + u * sn) * s.size0, (u * cs - r * sn) * height)
                };
                let corners = [o - ax - ay, o + ax - ay, o + ax + ay, o - ax + ay];
                quads.push(Q { sort: 0, dist: (o - eye).length(), corners, rgba: s.color_rgba.map(|v| f32::from(v) / 255.0), atlas: [s.atlas.s0, s.atlas.ds, s.atlas.t0, s.atlas.dt], key: image_key(&s.material_name) });
            }
            for q in &mut quads {
                q.sort = mats.get(&q.key).map_or(0, |m| m.sort_key);
            }
            quads.sort_by(|a, b| a.sort.cmp(&b.sort).then(b.dist.total_cmp(&a.dist)));
            if std::env::var("IW6_SHOT_PERF").is_ok() {
                // Cost proxies at this camera: draws (runs of one material and blend in
                // sort order), blend switches, and the screen area the quads cover over
                // the screen's (overdraw a fill-bound GPU pays).
                let (mut area_sum, mut big, mut visible) = (0f64, 0u32, 0u32);
                let (mut draws, mut switches) = (0u32, 0u32);
                let mut last: Option<(&str, &str)> = None;
                let mut last_blend: Option<&str> = None;
                let mut used: BTreeSet<&str> = BTreeSet::new();
                let mut biggest: Vec<(f32, &str, f32)> = Vec::new();
                for q in &quads {
                    let pr: Vec<Option<(f32, f32)>> = q.corners.iter().map(|p| {
                        let v = camera.rotation.transform_vector3(*p - eye);
                        (v.z >= near).then(|| ((v.x / (v.z * camera.tan_x) * 0.5 + 0.5) * w as f32, (0.5 - v.y / (v.z * camera.tan_y) * 0.5) * h as f32))
                    }).collect();
                    if pr.iter().any(Option::is_none) {
                        continue;
                    }
                    let p: Vec<(f32, f32)> = pr.into_iter().flatten().collect();
                    let sh: f32 = (0..4).map(|i| p[i].0 * p[(i + 1) % 4].1 - p[(i + 1) % 4].0 * p[i].1).sum::<f32>().abs() * 0.5;
                    let (x0, x1) = (p.iter().map(|q| q.0).fold(f32::MAX, f32::min), p.iter().map(|q| q.0).fold(f32::MIN, f32::max));
                    let (y0, y1) = (p.iter().map(|q| q.1).fold(f32::MAX, f32::min), p.iter().map(|q| q.1).fold(f32::MIN, f32::max));
                    let bb = ((x1 - x0) * (y1 - y0)).max(1e-3);
                    let vis = ((x1.min(w as f32) - x0.max(0.0)).max(0.0)) * ((y1.min(h as f32) - y0.max(0.0)).max(0.0));
                    let area = sh * (vis / bb);
                    if area <= 0.0 {
                        continue;
                    }
                    let blend = mats.get(&q.key).map_or("alpha", |m| blend_of(&m.state));
                    visible += 1;
                    area_sum += f64::from(area);
                    if area > 0.25 * (w * h) as f32 {
                        big += 1;
                    }
                    if last != Some((q.key.as_str(), blend)) {
                        draws += 1;
                        last = Some((q.key.as_str(), blend));
                    }
                    if last_blend != Some(blend) {
                        switches += 1;
                        last_blend = Some(blend);
                    }
                    used.insert(q.key.as_str());
                    biggest.push((area, q.key.as_str(), q.dist));
                }
                println!(
                    "  PERF t={t:.2}s: {} sprites, {visible} on screen, {draws} draws ({} materials), {switches} blend switches, overdraw {:.2}x of {w}x{h}, {big} quads over a quarter of the screen",
                    out.sprites.len(), used.len(), area_sum / f64::from(w * h)
                );
                biggest.sort_by(|a, b| b.0.total_cmp(&a.0));
                for r in biggest.iter().take(3) {
                    println!("    biggest {} at {:.0}: {:.0} px ({:.0}% of the screen)", r.1, r.2, r.0, 100.0 * r.0 / (w * h) as f32);
                }
            }
            let mut drawn = 0usize;
            for q in &quads {
                let blend = mats.get(&q.key).map_or("alpha", |m| blend_of(&m.state));
                let (tex, _) = tex_of(&mats, &q.key, &mut paks);
                if let (Ok(pat), Some(t)) = (std::env::var("IW6_SHOT_DUMPTEX"), &tex) {
                    // Writes the sampled image once as rgb (left) and alpha (right).
                    if q.key.contains(&pat) && !std::path::Path::new(&out_dir).join(format!("tex_{}.png", q.key)).exists() {
                        let mut px = Vec::with_capacity(t.w * t.h * 6);
                        for y in 0..t.h {
                            for x in 0..t.w {
                                px.extend_from_slice(&t.rgba[(y * t.w + x) * 4..(y * t.w + x) * 4 + 3]);
                            }
                            for x in 0..t.w {
                                let a = t.rgba[(y * t.w + x) * 4 + 3];
                                px.extend_from_slice(&[a, a, a]);
                            }
                        }
                        let f = std::fs::File::create(out_dir.join(format!("tex_{}.png", q.key))).expect("png");
                        let mut enc = png::Encoder::new(std::io::BufWriter::new(f), (t.w * 2) as u32, t.h as u32);
                        enc.set_color(png::ColorType::Rgb);
                        enc.set_depth(png::BitDepth::Eight);
                        enc.write_header().expect("png").write_image_data(&px).expect("png");
                        println!("    texture {} {}x{} dumped", q.key, t.w, t.h);
                    }
                }
                // Project the corners.
                let proj = |p: Vec3| -> Option<(f32, f32, f32)> {
                    let v = camera.rotation.transform_vector3(p - eye);
                    if v.z < near {
                        return None;
                    }
                    Some(((v.x / (v.z * camera.tan_x) * 0.5 + 0.5) * w as f32, (0.5 - v.y / (v.z * camera.tan_y) * 0.5) * h as f32, v.z))
                };
                let pr: Vec<_> = q.corners.iter().map(|c| proj(*c)).collect();
                if pr.iter().any(Option::is_none) {
                    continue;
                }
                let pr: Vec<(f32, f32, f32)> = pr.into_iter().flatten().collect();
                let (x0, x1) = (pr.iter().map(|p| p.0).fold(f32::MAX, f32::min).max(0.0), pr.iter().map(|p| p.0).fold(f32::MIN, f32::max).min(w as f32 - 1.0));
                let (y0, y1) = (pr.iter().map(|p| p.1).fold(f32::MAX, f32::min).max(0.0), pr.iter().map(|p| p.1).fold(f32::MIN, f32::max).min(h as f32 - 1.0));
                if x0 >= x1 || y0 >= y1 {
                    continue;
                }
                drawn += 1;
                let zq = pr.iter().map(|p| p.2).sum::<f32>() / 4.0;
                // Inverse bilinear on the screen quad is overkill: map by the
                // quad's own axes in view space (a planar quad).
                let c0 = q.corners[0];
                let ex = q.corners[1] - q.corners[0];
                let ey = q.corners[3] - q.corners[0];
                for py in y0 as u32..=y1 as u32 {
                    for px in x0 as u32..=x1 as u32 {
                        if !depth.is_empty() {
                            let d = depth[(py * w + px) as usize];
                            let zw = if d > 0.0 { near / d } else { f32::MAX };
                            if zq > zw {
                                continue;
                            }
                        }
                        // Ray through the pixel meets the quad's plane.
                        let nx = ((px as f32 + 0.5) / w as f32 - 0.5) * 2.0 * camera.tan_x;
                        let ny = (0.5 - (py as f32 + 0.5) / h as f32) * 2.0 * camera.tan_y;
                        let dir_world = camera.rotation.transpose().transform_vector3(Vec3::new(nx, ny, 1.0));
                        let n = ex.cross(ey);
                        let denom = n.dot(dir_world);
                        if denom.abs() < 1e-9 {
                            continue;
                        }
                        let tt = n.dot(c0 - eye) / denom;
                        if tt <= 0.0 {
                            continue;
                        }
                        let hit = eye + dir_world * tt - c0;
                        let (u, v) = (hit.dot(ex) / ex.length_squared(), hit.dot(ey) / ey.length_squared());
                        if !(0.0..=1.0).contains(&u) || !(0.0..=1.0).contains(&v) {
                            continue;
                        }
                        let (us, vs) = (q.atlas[0] + u * q.atlas[1], q.atlas[2] + (1.0 - v) * q.atlas[3]);
                        let texel = match &tex {
                            Some(t) => {
                                let tx = ((us.fract() * t.w as f32) as usize).min(t.w - 1);
                                let ty = ((vs.fract() * t.h as f32) as usize).min(t.h - 1);
                                let p = &t.rgba[(ty * t.w + tx) * 4..(ty * t.w + tx) * 4 + 4];
                                [f32::from(p[0]) / 255.0, f32::from(p[1]) / 255.0, f32::from(p[2]) / 255.0, f32::from(p[3]) / 255.0]
                            }
                            None => {
                                let (dx, dy) = (u - 0.5, v - 0.5);
                                let a = (1.0 - (dx * dx + dy * dy).sqrt() * 2.0).clamp(0.0, 1.0);
                                [1.0, 0.0, 1.0, a * a]
                            }
                        };
                        let a = q.rgba[3] * texel[3];
                        let src = [q.rgba[0] * texel[0], q.rgba[1] * texel[1], q.rgba[2] * texel[2]];
                        let dst = &mut rgb[(py * w + px) as usize];
                        for c in 0..3 {
                            dst[c] = match blend {
                                "add" => (dst[c] + src[c] * a).min(1.0),
                                "screen" => (dst[c] + src[c] * a * (1.0 - dst[c])).min(1.0),
                                "premult" => (dst[c] * (1.0 - a) + src[c]).min(1.0),
                                _ => dst[c] * (1.0 - a) + src[c] * a,
                            };
                        }
                    }
                }
            }
            let safe: String = name.chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '_' }).collect();
            let path = out_dir.join(format!("{safe}_t{:.2}.png", t));
            let f = std::fs::File::create(&path).expect("png");
            let mut enc = png::Encoder::new(std::io::BufWriter::new(f), w, h);
            enc.set_color(png::ColorType::Rgb);
            enc.set_depth(png::BitDepth::Eight);
            let bytes: Vec<u8> = rgb.iter().flat_map(|c| c.map(|v| (v.clamp(0.0, 1.0) * 255.0).round() as u8)).collect();
            enc.write_header().expect("png").write_image_data(&bytes).expect("png");
            println!("    -> {} ({} quads, {} drawn)", path.display(), quads.len(), drawn);
        }
    }
    let _ = (BTreeSet::<u8>::new(), Mat4::IDENTITY, tally, FxName::engine("x"));
}

/// The native frame of the map (render_iw6), read back as display values and
/// the depth.
struct WorldFrame {
    gpu: render_h1::gpu::Gpu,
    host: render_iw6::host::Host,
    depth: wgpu::Texture,
    display: wgpu::Texture,
    w: u32,
    h: u32,
}

impl WorldFrame {
    fn new(dir: &std::path::Path, map: &str, w: u32, h: u32) -> Self {
        let zones = render_iw6::scene::Scene::zones_for(&dir.join(format!("{map}.ff")));
        let scene = render_iw6::scene::Scene::load(&zones).expect("scene");
        let gpu = render_h1::gpu::Gpu::headless().expect("gpu");
        let host = render_iw6::host::Host::new(gpu.clone(), scene, wgpu::TextureFormat::Depth32Float, zones);
        let depth = render_h1::post::target(&gpu, "depth", w, h, wgpu::TextureFormat::Depth32Float);
        let display = render_h1::post::target(&gpu, "display", w, h, wgpu::TextureFormat::Rgba8Unorm);
        Self { gpu, host, depth, display, w, h }
    }

    fn shot(&mut self, eye: Vec3, angles: Vec3) -> (Vec<[f32; 3]>, Vec<f32>, f32) {
        let (w, h) = (self.w, self.h);
        let view = render_h1::code::View { eye, angles, fov_x: 80.0, width: w, height: h, near: 4.0 };
        let camera = render_h1::code::Camera::from(&view);
        let dv = self.depth.create_view(&Default::default());
        let ds = self.depth.create_view(&wgpu::TextureViewDescriptor { aspect: wgpu::TextureAspect::DepthOnly, ..Default::default() });
        let ov = self.display.create_view(&Default::default());
        for _ in 0..2 {
            let mut enc = self.gpu.device.create_command_encoder(&Default::default());
            self.host
                .record(&mut enc, &dv, &ds, &ov, wgpu::TextureFormat::Rgba8Unorm, w, h, &camera, &render_iw6::host::Look::default())
                .expect("frame");
            self.gpu.queue.submit([enc.finish()]);
        }
        let px = read(&self.gpu, &self.display, w, h, false);
        let rgb = px.chunks_exact(4).map(|p| [f32::from(p[0]) / 255.0, f32::from(p[1]) / 255.0, f32::from(p[2]) / 255.0]).collect();
        let d = read(&self.gpu, &self.depth, w, h, true);
        let depth = d.chunks_exact(4).map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]])).collect();
        (rgb, depth, 4.0)
    }
}

fn read(gpu: &render_h1::gpu::Gpu, tex: &wgpu::Texture, w: u32, h: u32, depth: bool) -> Vec<u8> {
    let row = (w * 4).div_ceil(256) * 256;
    let buf = gpu.device.create_buffer(&wgpu::BufferDescriptor { label: Some("readback"), size: u64::from(row * h), usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ, mapped_at_creation: false });
    let mut enc = gpu.device.create_command_encoder(&Default::default());
    let src = wgpu::TexelCopyTextureInfo { texture: tex, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: if depth { wgpu::TextureAspect::DepthOnly } else { wgpu::TextureAspect::All } };
    enc.copy_texture_to_buffer(
        src,
        wgpu::TexelCopyBufferInfo { buffer: &buf, layout: wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(row), rows_per_image: Some(h) } },
        wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
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
