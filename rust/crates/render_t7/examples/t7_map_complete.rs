//! `t7_map_complete [map.ff ...]`: is each Black Ops III map complete?
//! (`render_t7::completeness`). Without arguments every BO3 map zone staged
//! under `T7_MAPS_ROOT` (default `/mnt/user-data/uploads`): `mp_*.ff` /
//! `zm_*.ff` beside BO3's shared zones or in a Workshop item folder.
//!
//! Per map: surfaces in the map and drawn (G-buffer, forward), the undrawn
//! by reason and technique set family (materials named), and the hole test:
//! rays from up to `T7_HOLE_EYES` (32) spawns in `T7_HOLE_DIRS` (384)
//! directions out to 4000 units; a ray whose first solid collision has no
//! drawn surface within `SLACK` of it is a hole, put down to the undrawn
//! draw there (its reason) or to the collision with nothing on it.
//! Ends with `| map | ... |` rows for `docs/T7_MAP_COMPLETENESS.md`.
//!
//! `T7_GAME_ZONE=<dir>`: BO3's zone folder for Workshop maps (else the
//! first staged folder with `core_common.ff`). `T7_HOLE_EYES=0`: no hole
//! test.
use std::path::{Path, PathBuf};

fn find_maps(root: &Path, out: &mut Vec<PathBuf>, depth: usize) {
    let Ok(rd) = std::fs::read_dir(root) else { return };
    let mut entries: Vec<_> = rd.flatten().map(|e| e.path()).collect();
    entries.sort();
    for p in entries {
        let name = p.file_name().map_or_else(String::new, |n| n.to_string_lossy().into_owned());
        if p.is_dir() && depth < 4 {
            find_maps(&p, out, depth + 1);
        } else if (name.starts_with("mp_") || name.starts_with("zm_")) && name.ends_with(".ff") && !name.contains("_patch") && !name.contains("_load") && !name.starts_with("mp_common") && !name.starts_with("mp_weapon") {
            let dir = p.parent().unwrap();
            // A BO3 map: beside BO3's shared zones, or a Workshop item.
            if dir.join("core_common.ff").is_file() || dir.join("workshop.json").is_file() {
                out.push(p);
            }
        }
    }
}

fn main() {
    let args: Vec<PathBuf> = std::env::args().skip(1).map(PathBuf::from).collect();
    let root = PathBuf::from(std::env::var("T7_MAPS_ROOT").unwrap_or_else(|_| "/mnt/user-data/uploads".into()));
    let mut maps = args.clone();
    if maps.is_empty() {
        find_maps(&root, &mut maps, 0);
    }
    // One of each map name (the same zone staged twice).
    let mut seen = std::collections::HashSet::new();
    maps.retain(|p| seen.insert(std::fs::canonicalize(p).ok().and_then(|c| c.file_name().map(|n| n.to_owned())).or_else(|| p.file_name().map(|n| n.to_owned()))));
    let game_zone = std::env::var("T7_GAME_ZONE").map(PathBuf::from).ok().or_else(|| maps.iter().filter_map(|m| m.parent()).find(|d| d.join("core_common.ff").is_file()).map(Path::to_path_buf)).or_else(|| {
        let mut all = Vec::new();
        find_maps(&root, &mut all, 0);
        all.iter().filter_map(|m| m.parent()).find(|d| d.join("core_common.ff").is_file()).map(Path::to_path_buf)
    });
    fastfile_t7::map::set_game_zone_dir(game_zone.clone());
    println!("maps: {} ({:?}); BO3 zone folder {:?}", maps.len(), maps.iter().filter_map(|m| m.file_stem()).collect::<Vec<_>>(), game_zone);
    let eyes_max: usize = std::env::var("T7_HOLE_EYES").ok().and_then(|v| v.parse().ok()).unwrap_or(32);
    let ndirs: usize = std::env::var("T7_HOLE_DIRS").ok().and_then(|v| v.parse().ok()).unwrap_or(384);
    let dirs = render_t7::completeness::directions(ndirs);
    let mut rows = Vec::new();
    for zone in &maps {
        let t0 = std::time::Instant::now();
        let scene = match render_t7::scene::Scene::load(zone) {
            Ok(s) => s,
            Err(e) => {
                println!("== {}: not read: {e}", zone.display());
                continue;
            }
        };
        let name = scene.name.clone();
        println!("== {name} ({}; read in {:.1}s)", zone.display(), t0.elapsed().as_secs_f32());
        for l in scene.report.iter().filter(|l| l.contains("completed from a technique") || l.contains("constant buffers the material records") || l.contains("looked up after the walk")) {
            println!("scene: {}", l.chars().take(600).collect::<String>());
        }
        // `T7_MODEL_CHECK=<model>`: its drawn placements and its collision
        // placements side by side (a hole put down to "collision outside its
        // drawn model's mesh").
        if let Ok(want) = std::env::var("T7_MODEL_CHECK") {
            if let Some(src) = scene.model_names.iter().position(|n| *n == want) {
                for d in scene.draws.iter().filter(|d| d.group == 1 && d.source as usize == src).take(1) {
                    println!("check {want}: drawn bounds of the largest placement {:?}..{:?}, {} placements", d.mins, d.maxs, d.instances);
                    for i in d.instance..(d.instance + d.instances).min(d.instance + 4) {
                        let b = scene.bones[scene.instances[i as usize][3] as usize];
                        println!("  drawn placement: translation ({:.1} {:.1} {:.1}) row0 ({:.3} {:.3} {:.3})", b[3], b[7], b[11], b[0], b[1], b[2]);
                    }
                }
            }
            if let Some(clip) = &scene.map.clip {
                for sm in clip.static_models.iter().filter(|s| s.name == want).take(4) {
                    println!("  collision placement: origin {:?} axis0 {:?} scale {} bones {}", sm.origin, sm.axis[0], sm.scale, sm.bones.len());
                }
                if let Some(tris) = clip.model_collision.get(&want) {
                    let (mut lo, mut hi) = ([f32::MAX; 3], [f32::MIN; 3]);
                    for t in tris {
                        for c in t.corners {
                            for k in 0..3 {
                                lo[k] = lo[k].min(c[k]);
                                hi[k] = hi[k].max(c[k]);
                            }
                        }
                    }
                    println!("  collision triangles {} in model/bone space {lo:?}..{hi:?}; bones used {:?}", tris.len(), tris.iter().map(|t| t.bone).collect::<std::collections::BTreeSet<_>>());
                }
            }
        }
        let t = render_t7::completeness::tally(&scene);
        println!("{}", t.line(&name));
        let mut groups: Vec<_> = t.undrawn.iter().collect();
        groups.sort_by_key(|g| std::cmp::Reverse(g.1.0.total()));
        for ((reason, fam), (n, mats)) in &groups {
            println!("  undrawn {:>6} ({} world, {} model) {reason} [{fam}]: {}{}", n.total(), n.world, n.models, mats.iter().take(4).cloned().collect::<Vec<_>>().join(", "), if mats.len() > 4 { format!(" (+{})", mats.len() - 4) } else { String::new() });
        }
        // `T7_GPU=1`: the G-buffer pipelines and bindings of the draws whose
        // technique was completed from a donor or whose constant buffer was
        // made up (`T7_GPU=all`: every draw; `world`: the world's), made on whatever adapter wgpu
        // finds; the hole test is not run.
        if let Ok(mode) = std::env::var("T7_GPU") {
            let mut scene = scene;
            let donated: std::collections::HashSet<String> = scene.donated.iter().map(|(n, _)| n.clone()).collect();
            let filled: std::collections::HashSet<String> = scene.cbuffers_filled.iter().filter(|l| !l.ends_with("not made")).filter_map(|l| l.split(' ').next().map(str::to_owned)).collect();
            let mats = &scene.materials;
            if mode == "world" {
                // (`T7_GPU=world`: the world surfaces only, less memory.)
                scene.draws.retain(|d| d.group == 3);
            } else if mode != "all" {
                scene.draws.retain(|d| {
                    let m = &mats[d.material];
                    m.technique.as_ref().is_some_and(|t| donated.contains(&t.techset)) || filled.contains(&m.name)
                });
            }
            let n = scene.draws.len();
            let gpu = render_h1::gpu::Gpu::headless().expect("gpu");
            let targets = render_t7::gbuffer::Targets::new(&gpu, 320, 180);
            let mut g = render_t7::gbuffer::GBuffer::new(gpu.clone(), std::sync::Arc::new(scene));
            g.check_bindings_without_images = true;
            let scope = gpu.device.push_error_scope(wgpu::ErrorFilter::Validation);
            g.prepare(&targets);
            let err = pollster::block_on(scope.pop());
            let r = &g.report;
            let forward_only = g.scene.draws.iter().filter(|d| g.scene.materials[d.material].technique.is_none()).count();
            println!("  gpu: {n} draws checked ({forward_only} forward, not made here): {} G-buffer ready; pipelines {} made, {} failed; validation {}", r.draws_ready, r.pipelines_made, r.pipelines_failed.len(), err.map_or("none".to_owned(), |e| e.to_string()));
            for f in r.pipelines_failed.iter().take(8) {
                println!("    pipeline failed: {f}");
            }
            for d in r.draws_skipped.iter().take(8) {
                println!("    skipped: {d}");
            }
            continue;
        }
        let mut hole_line = "-".to_owned();
        if eyes_max > 0 {
            let t1 = std::time::Instant::now();
            let tris = render_t7::completeness::scene_triangles(&scene);
            match render_t7::completeness::Collision::read(&scene) {
                None => println!("  holes: no collision in the zone"),
                Some(coll) => {
                    let eyes = render_t7::completeness::spawn_eyes(&scene, eyes_max);
                    let h = render_t7::completeness::find_holes(&scene, &tris, &coll, &eyes, &dirs, 4000.0);
                    println!(
                        "  holes: {} of {} rays meeting solid collision ({:.2}%), {} rays from {} spawns; {} scene triangles, {} collision triangles ({:.1}s)",
                        h.total(),
                        h.solid,
                        100.0 * h.total() as f32 / h.solid.max(1) as f32,
                        h.rays,
                        eyes.len(),
                        tris.meshes.iter().map(|m| m.tris.len()).sum::<usize>(),
                        coll.mesh.tris.len(),
                        t1.elapsed().as_secs_f32()
                    );
                    for (k, n) in h.by_kind() {
                        println!("    holes by kind {n:>5}: {k}");
                    }
                    let exp: usize = h.expected.values().sum();
                    println!("    not holes: {exp} rays meet collision invisible by design ({})", h.expected.iter().map(|(k, n)| format!("{k} {n}")).collect::<Vec<_>>().join(", "));
                    let mut c: Vec<_> = h.by_cause.iter().collect();
                    c.sort_by(|a, b| b.1.cmp(a.1));
                    for (cause, n) in c.iter().take(16) {
                        println!("    hole {n:>5}: {cause}");
                    }
                    for (eye, d, tc, cause) in h.examples.iter().take(8) {
                        println!("    e.g. eye ({:.0} {:.0} {:.0}) dir ({:.2} {:.2} {:.2}) collision at {tc:.0}: {cause}", eye.x, eye.y, eye.z, d.x, d.y, d.z);
                    }
                    let kinds = h.by_kind();
                    hole_line = format!("{} / {} ({:.2}%){}", h.total(), h.solid, 100.0 * h.total() as f32 / h.solid.max(1) as f32, kinds.first().map_or_else(String::new, |(k, n)| format!("; most: {k} {n}")));
                }
            }
        }
        let u = t.undrawn_total();
        let top: Vec<String> = t.by_reason().iter().take(3).map(|(r, n)| format!("{r} {n}")).collect();
        rows.push(format!("| {name} | {} | {} | {} | {} ({:.1}%) | {} | {} |", t.total.total(), t.gbuffer.total(), t.forward.total(), u.total(), 100.0 * u.total() as f32 / t.total.total().max(1) as f32, hole_line, top.join("; ")));
    }
    println!("\n| map | surfaces | G-buffer | forward | not drawn | holes (rays) | top reasons |\n|---|---|---|---|---|---|---|");
    for r in rows {
        println!("{r}");
    }
}
