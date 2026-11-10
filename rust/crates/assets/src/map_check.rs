//! bo2mp `CodLegacy check-maps`: one map from another game read the way a match
//! reads it - its zone opened by its own lane (MW2 and its usermaps,
//! Call of Duty 4, World at War), its world draw, collision and spawn
//! points built - without starting a match. One line per map: `OK:` with
//! what was built, or `FAIL` with the first reason a match would refuse it.

use std::path::Path;

use asset_transport::progress::LoadProgress;

use crate::lane::{LoadedWorld, lane};

/// The world built for one map, as a line.
fn summary(loaded: &LoadedWorld) -> Result<String, String> {
    let draw = loaded.world.draw.as_ref();
    let surfaces = draw.map_or(0, |d| d.stats.surfaces);
    let batches = draw.map_or(0, |d| d.batches.len());
    let triangles = draw.map_or(0, |d| d.stats.triangles);
    let gaps: Vec<String> = loaded.gaps.iter().map(|g| g.reason.clone()).collect();
    let brushes = loaded.collision.as_ref().map(|c| c.brushes.len());
    let line = format!(
        "world {surfaces} surfaces in {batches} batches ({triangles} triangles); collision {}; {} spawn points",
        brushes.map_or_else(|| "none".to_owned(), |b| format!("{b} brushes")),
        loaded.spawns.len()
    );
    if draw.is_none() || batches == 0 {
        return Err(format!(
            "no drawable world{}",
            if gaps.is_empty() { String::new() } else { format!(": {}", gaps.join("; ")) }
        ));
    }
    if loaded.collision.is_none() {
        return Err(format!("no collision: {}; {line}", gaps.join("; ")));
    }
    if loaded.spawns.is_empty() {
        return Err(format!("no spawn points; {line}"));
    }
    if gaps.is_empty() {
        Ok(line)
    } else {
        Ok(format!("{line}; gaps: {}", gaps.join("; ")))
    }
}

/// One map, by its game prefix (`iw4`, `iw3`, `t4`) and zone file. MW2
/// zones (stock, IW4x's packs, usermaps) also get the picture census of
/// [`crate::check_iw4_map`].
pub fn check_map(game: &str, path: &Path) -> String {
    let loaded = match load_for_check(game, path) {
        Ok(loaded) => loaded,
        Err(line) => return line,
    };
    let mut line = check_loaded(game, path, &loaded);
    // bo2mp: `BO2MP_CHECK_REPORT=<substring>`: the load report's lines
    // holding it, after the summary (a Zombies map's layout: `zm`).
    if let Ok(want) = std::env::var("BO2MP_CHECK_REPORT") {
        for l in loaded.report.iter().filter(|l| l.contains(want.as_str())) {
            line.push_str("\n    ");
            line.push_str(l);
        }
    }
    line
}

/// bo2mp: the map's collision mesh traced the way a match traces it
/// (`map_check_probe` with `BO2MP_CHECK_TRACES=1`): sight lines between
/// spawn points at eye height, long bullet lines out of each spawn, and a
/// player's box stepping and falling at each. Mesh only (brushes are not
/// traced); a line per kind: time and work per trace.
pub fn trace_bench(game: &str, path: &Path) -> String {
    let loaded = match load_for_check(game, path) {
        Ok(loaded) => loaded,
        Err(line) => return line,
    };
    let Some(clip) = loaded.collision.as_ref() else {
        return "no collision".to_owned();
    };
    let mesh = clip.mesh.as_ref().as_ref();
    let spawns: Vec<[f32; 3]> = loaded.spawns.iter().map(|s| s.origin).collect();
    if spawns.is_empty() || mesh.tri_indices.len() < 3 {
        return format!("nothing to trace ({} spawns, {} triangles)", spawns.len(), mesh.tri_indices.len() / 3);
    }
    let n = spawns.len();
    let eye = |p: [f32; 3]| [p[0], p[1], p[2] + 60.0];
    let mut kinds: Vec<(&str, Vec<clipmap_iw4::TraceExtents>)> = vec![("sight", Vec::new()), ("bullet", Vec::new()), ("step", Vec::new()), ("fall", Vec::new())];
    for i in 0..400usize {
        let a = spawns[(i * 7) % n];
        let b = spawns[(i * 13 + 5) % n];
        kinds[0].1.push(clipmap_iw4::TraceExtents::new(eye(a), eye(b), [0.0; 3], [0.0; 3], 1));
        let yaw = (i as f32) * 0.618_034 * std::f32::consts::TAU;
        let pitch = ((i % 9) as f32 - 4.0) * 0.08;
        let dir = [yaw.cos() * pitch.cos(), yaw.sin() * pitch.cos(), pitch.sin()];
        let e = eye(a);
        kinds[1].1.push(clipmap_iw4::TraceExtents::new(e, std::array::from_fn(|k| e[k] + dir[k] * 8192.0), [0.0; 3], [0.0; 3], 1));
        let up = [a[0], a[1], a[2] + 18.0];
        kinds[2].1.push(clipmap_iw4::TraceExtents::new(up, [up[0] + dir[0] * 24.0, up[1] + dir[1] * 24.0, up[2]], [-15.0, -15.0, 0.0], [15.0, 15.0, 52.0], 1));
        kinds[3].1.push(clipmap_iw4::TraceExtents::new(up, [up[0], up[1], up[2] - 256.0], [-15.0, -15.0, 0.0], [15.0, 15.0, 52.0], 1));
    }
    let mut lines = vec![format!("{} triangles, {} nodes, {} spawns", mesh.tri_indices.len() / 3, mesh.aabb_trees.len(), n)];
    for (name, list) in &kinds {
        let started = std::time::Instant::now();
        let (mut tris, mut nodes, mut hits) = (0u64, 0u64, 0u32);
        for ext in list {
            let (t, census) = clipmap_iw4::trace_through_mesh_with_census(&mesh, ext);
            tris += u64::from(census.tris_tested);
            nodes += u64::from(census.aabb_nodes_visited);
            hits += u32::from(t.fraction < 1.0);
        }
        let k = list.len().max(1) as f64;
        lines.push(format!(
            "{name}: {:.1} us a trace, {:.0} triangles and {:.0} nodes tested, {hits}/{} hit",
            started.elapsed().as_secs_f64() * 1e6 / k,
            tris as f64 / k,
            nodes as f64 / k,
            list.len()
        ));
    }
    lines.join("; ")
}

/// bo2mp: what the shared look's offline check (`bo2_shared_look_check`)
/// draws of a Black Ops II map: the world's triangles as the match builds
/// them, the map's sun, its spawn points and bounds.
pub struct LookScene {
    pub positions: Vec<[f32; 3]>,
    pub normals: Vec<[f32; 3]>,
    pub indices: Vec<u32>,
    /// Toward the sun (unit) and its colour.
    pub sun_dir: [f32; 3],
    pub sun_colour: [f32; 3],
    /// Origin and yaw (degrees).
    pub spawns: Vec<([f32; 3], f32)>,
    pub min: [f32; 3],
    pub max: [f32; 3],
    pub report: Vec<String>,
}

/// bo2mp: a Black Ops II map's [`LookScene`], read the way a match reads it.
pub fn bo2_look_scene(path: &Path) -> Result<LookScene, String> {
    let image = asset_transport::open_zone(path).map_err(|e| format!("zone does not open: {e}"))?;
    if image.game != asset_core::ZoneGame::T6 {
        return Err(format!("not a Black Ops II zone ({:?})", image.game));
    }
    let progress = LoadProgress::new(0);
    let mut visions = std::collections::BTreeMap::new();
    let loaded = lane(image.game).load_world(path, &image, &progress, Default::default(), Default::default(), &mut visions);
    let draw = loaded.world.draw.as_ref().ok_or("no drawable world")?;
    let sun = draw
        .t5_sun_light
        .map(|s| (s.direction, [s.diffuse[0], s.diffuse[1], s.diffuse[2]]))
        .or_else(|| draw.primary_lights.iter().find(|l| l.is_sun).map(|l| (l.direction, l.color)))
        .ok_or("no sun")?;
    let indices: Vec<u32> = draw
        .surface_index_ranges
        .iter()
        .flat_map(|&(start, count)| draw.packed_indices.get(start as usize..(start + count) as usize).unwrap_or(&[]).iter().copied())
        .collect();
    Ok(LookScene {
        positions: draw.positions.clone(),
        normals: draw.normals.clone(),
        indices,
        sun_dir: sun.0,
        sun_colour: sun.1,
        spawns: loaded.spawns.iter().filter(|s| s.classname.contains("_spawn")).map(|s| (s.origin, s.angles[1])).collect(),
        min: draw.stats.min,
        max: draw.stats.max,
        report: loaded.report.clone(),
    })
}

fn load_for_check(game: &str, path: &Path) -> Result<LoadedWorld, String> {
    let progress = LoadProgress::new(0);
    Ok(match game {
        "iw3" => crate::lane::load_iw3_world(path, &progress, Default::default()),
        "t4" => crate::lane::load_t4_world(path, &progress, Default::default()),
        // bo2mp: Ghosts' maps.
        "iw6" => crate::lane::load_iw6_world(path, &progress, Default::default()),
        // bo2mp: Infinite Warfare's maps.
        "iw7" => crate::lane::load_iw7_world(path, &progress, Default::default()),
        // bo2mp: Modern Warfare Remastered's maps and Horizon MW's usermaps.
        "h1" => crate::lane::load_h1_world(path, &progress, Default::default()),
        // bo2mp: Black Ops III's maps.
        // (`BO2MP_CHECK_NATIVE=1`: as the game loads it for the native
        // renderer, offline timing.)
        "t7" => crate::lane::load_t7_world_for(path, &progress, Default::default(), std::env::var("BO2MP_CHECK_NATIVE").is_ok_and(|v| v == "1")),
        // bo2mp: Modern Warfare III's (2023) maps.
        "jup" => crate::lane::load_jup_world(path, &progress, Default::default(), false),
        "iw4" => {
            let image = match asset_transport::open_zone(path) {
                Ok(image) => image,
                Err(e) => return Err(format!("FAIL zone does not open: {e}")),
            };
            let mut visions = std::collections::BTreeMap::new();
            lane(image.game).load_world(
                path,
                &image,
                &progress,
                Default::default(),
                Default::default(),
                &mut visions,
            )
        }
        other => return Err(format!("FAIL no map check for `{other}` zones")),
    })
}

fn check_loaded(game: &str, path: &Path, loaded: &LoadedWorld) -> String {
    let world = summary(loaded);
    match (game, world) {
        ("iw4", Ok(line)) => {
            let pictures = crate::check_iw4_map(path);
            if pictures.starts_with("OK:") {
                format!("OK: {line}; {}", pictures.trim_start_matches("OK: "))
            } else {
                format!("{pictures}; {line}")
            }
        }
        (_, Ok(line)) => {
            let materials = loaded
                .report
                .iter()
                .filter(|l| l.starts_with(&format!("{game} materials")) || l.starts_with(&format!("{game} lightmaps")) || l.starts_with(&format!("{game} fog")))
                .cloned()
                .collect::<Vec<_>>()
                .join("; ");
            format!("OK: {line}; {materials}; {}; {}", legacy_texture_census(&loaded), props_census(&loaded))
        }
        (_, Err(why)) => format!("FAIL {why}"),
    }
}

/// bo2mp: what a CoD4 / World at War map draws untextured: every material
/// a drawn world batch or a placed prop uses, by its colour binding as the
/// renderer will see it (no colour slot, no image, or the lane's 1x1 grey
/// stand-in for a picture it did not find). `textures missing: world N/M
/// materials (T tris), props P/Q [names]`.
fn legacy_texture_census(loaded: &LoadedWorld) -> String {
    use std::collections::BTreeMap;
    let cat = &loaded.materials;
    let untextured = |m: usize| -> Option<String> {
        let mat = cat.materials.get(m)?;
        let name = mat.name.as_str().to_owned();
        // (The shadow-only and sky surfaces draw no colour.)
        if mat.t6_draw.is_some_and(|d| matches!(d.blend, asset_core::T6Blend::ShadowOnly | asset_core::T6Blend::Sky)) {
            return None;
        }
        let colour = mat.textures.iter().find(|t| t.semantic == asset_material::TS_COLOR_MAP);
        let image = colour.and_then(|t| t.image).and_then(|i| cat.images.get(i));
        match image {
            None => Some(format!("{name} (no colour map)")),
            // (A 1x1 picture is the grey stand-in, unless it is a water
            // colour the lane made.)
            Some(img) if img.width <= 1 && img.height <= 1 && !img.name.as_str().ends_with("#water") => {
                Some(format!("{name} [{}]", img.name.as_str()))
            }
            // The editor's placeholder (`case64blue`) drawn as a picture.
            Some(img) if img.name.as_str().contains("case64") => Some(format!("{name} [{} placeholder]", img.name.as_str())),
            Some(img) if img.decoded.is_none() && img.payload.is_empty() && img.pending_decode.is_none() => {
                Some(format!("{name} [{}: no pixels]", img.name.as_str()))
            }
            Some(_) => None,
        }
    };
    let mut world: BTreeMap<usize, u64> = BTreeMap::new();
    if let Some(draw) = &loaded.world.draw {
        for b in &draw.batches {
            if let Some(m) = b.material {
                *world.entry(m).or_default() += (b.packed_indices.len() / 3) as u64;
            }
        }
    }
    let mut props: BTreeMap<usize, ()> = BTreeMap::new();
    for mesh in &loaded.world.static_model_meshes {
        for s in mesh.lod_surfaces.iter().flatten() {
            if let Some(m) = s.material {
                props.insert(m, ());
            }
        }
    }
    let mut names = Vec::new();
    let (mut wm, mut wt) = (0usize, 0u64);
    for (&m, &tris) in &world {
        if let Some(n) = untextured(m) {
            wm += 1;
            wt += tris;
            names.push(format!("world {n} {tris} tris"));
        }
    }
    let mut pm = 0usize;
    for &m in props.keys() {
        if let Some(n) = untextured(m) {
            pm += 1;
            names.push(format!("prop {n}"));
        }
    }
    format!(
        "textures missing: world {wm}/{} materials ({wt} tris), props {pm}/{} {names:?}",
        world.len(),
        props.len()
    )
}

/// bo2mp: the placed props as the renderer's props pass takes them
/// (`SmodelGpuPlan::t6_fallback_props` draws only a placement that carries a
/// light): instances placed, with a light (drawn), their first-LOD surfaces
/// (drawn / with a colour map / with no material), and the models behind them.
fn props_census(loaded: &LoadedWorld) -> String {
    let cat = &loaded.materials;
    let meshes = &loaded.world.static_model_meshes;
    let placed = loaded.world.static_model_instances.iter().flatten().count();
    let (mut drawn, mut surfaces, mut textured, mut no_material) = (0usize, 0usize, 0usize, 0usize);
    let (mut cutout, mut cutout_lit, mut unlit) = (0usize, 0usize, 0usize);
    let mut seen = std::collections::HashSet::new();
    for p in loaded.world.static_model_instances.iter().flatten() {
        if p.packed_lighting.is_none() {
            continue;
        }
        drawn += 1;
        let Some(mesh) = meshes.get(p.mesh) else { continue };
        seen.insert(p.mesh);
        for s in &mesh.lod_surfaces[0] {
            surfaces += 1;
            match s.material.and_then(|m| cat.materials.get(m)) {
                None => no_material += 1,
                Some(mat) => {
                    let has = mat
                        .textures
                        .iter()
                        .any(|t| t.semantic == asset_material::TS_COLOR_MAP && t.image.is_some());
                    textured += usize::from(has);
                    if let Some(d) = &mat.t6_draw {
                        // (A cut-out draws two-sided: Black Ops II's `cull` none.)
                        cutout += usize::from(d.alpha_test && d.cull == asset_core::T6Cull::None);
                        cutout_lit += usize::from(d.alpha_test && !d.unlit);
                        unlit += usize::from(d.unlit);
                    }
                }
            }
        }
    }
    format!(
        "props: {placed} placed, {drawn} with a light (the props pass draws these) from {} of {} models, {surfaces} surfaces drawn ({textured} with a colour map, {no_material} no material; {cutout} alpha tested and two-sided, {cutout_lit} alpha tested and lit, {unlit} unlit); catalog {} materials, {} images",
        seen.len(),
        meshes.len(),
        cat.materials.len(),
        cat.images.len()
    )
}
