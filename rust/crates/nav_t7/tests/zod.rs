//! Shadows of Evil's nav mesh: read from the map zone, and zombies' paths
//! from the start zone's spawners to the players' start, around the
//! walls. Reads `zm_zod.ff` from `T7_ZONES` (else
//! `/mnt/user-data/uploads/zone`); skipped when it is not there.

use std::path::PathBuf;

use nav_t7::NavMesh;

fn zone() -> Option<PathBuf> {
    let dir = std::env::var_os("T7_ZONES").map_or_else(|| PathBuf::from("/mnt/user-data/uploads/zone"), PathBuf::from);
    let p = dir.join("zm_zod.ff");
    p.is_file().then_some(p)
}

#[test]
fn zod_nav_mesh_paths_from_spawners_to_the_start() {
    let Some(path) = zone() else {
        eprintln!("no zm_zod.ff; skipped");
        return;
    };
    let z = fastfile_t7::Zone::open(&path).expect("zone");
    let mesh = NavMesh::from_image(&z.image).expect("nav mesh").expect("a nav mesh");
    drop(z);
    eprintln!("nav mesh: {} faces, {} edges, {} vertices", mesh.faces.len(), mesh.edges.len(), mesh.verts.len());
    assert!(mesh.faces.len() > 3000);
    let map = fastfile_t7::map::T7Map::load(&fastfile_t7::map::MapSources::beside(&path)).expect("map");
    let ents = iw3::ents::parse(&map.entities);
    let named = |n: &str| ents.iter().filter(move |e| e.get("targetname") == Some(n)).collect::<Vec<_>>();
    let starts: Vec<[f32; 3]> = named("initial_spawn_points").iter().filter_map(|e| e.origin()).collect();
    assert!(!starts.is_empty());
    let start = starts[0];
    let mut mesh = mesh;
    let before = mesh.reachable(mesh.locate(start, 64.0, 32.0).expect("start on the mesh")).iter().filter(|r| **r).count();
    let mut linked = 0;
    let mut traversals = 0;
    for b in ents.iter().filter(|e| e.classname() == "node_negotiation_begin") {
        let end = b.get("target").map_or_else(Vec::new, named).first().and_then(|e| e.origin());
        if let (Some(o), Some(e)) = (b.origin(), end) {
            traversals += 1;
            if mesh.link_traversal(o, e, b.get("animscript").unwrap_or("")).is_some() {
                linked += 1;
            }
        }
    }
    // Windows: from the outside goal in front of the boards, through the
    // barrier, to where the zombie lands inside.
    let mut windows = (0, 0);
    for g in named("exterior_goal") {
        let barrier = g.get("target").map_or_else(Vec::new, named).into_iter().find(|e| e.classname().starts_with("zbarrier")).and_then(|e| e.origin());
        let (Some(o), Some(b)) = (g.origin(), barrier) else { continue };
        windows.0 += 1;
        let d = [b[0] - o[0], b[1] - o[1]];
        let l = d[0].hypot(d[1]).max(1.0);
        let inside = [b[0] + d[0] / l * 48.0, b[1] + d[1] / l * 48.0, o[2]];
        if mesh.link_traversal(o, inside, "zbarrier").is_some() {
            windows.1 += 1;
        }
    }
    eprintln!("traversals: {linked} of {traversals} linked, windows {} of {}; reachable from the start without them: {before} faces", windows.1, windows.0);
    let sf = mesh.locate(start, 64.0, 32.0).expect("the start is over the mesh");
    let reach = mesh.reachable(sf);
    eprintln!("faces reachable from the start: {} of {}", reach.iter().filter(|r| **r).count(), mesh.faces.len());
    // The start zone's spawners: the structs the start zone's volumes target.
    let mut spawners = Vec::new();
    for v in ents.iter().filter(|e| e.classname() == "info_volume" && e.get("script_noteworthy") == Some("player_volume")) {
        let Some(zone) = v.get("targetname") else { continue };
        for s in v.get("target").map_or_else(Vec::new, named) {
            if let Some(o) = s.origin() {
                spawners.push((zone.to_owned(), s.get("script_noteworthy").unwrap_or("").to_owned(), o));
            }
        }
    }
    eprintln!("{} zone spawn structs", spawners.len());
    let start_zone: Vec<_> = spawners.iter().filter(|s| s.0 == "zone_start").collect();
    let near: Vec<_> = if start_zone.is_empty() { spawners.iter().collect() } else { start_zone };
    let (mut found, mut tried, mut bent, mut through_links) = (0, 0, 0, 0);
    for (zone, kind, o) in &near {
        if kind == "dog_location" {
            continue;
        }
        tried += 1;
        let Some(p) = mesh.find_path(*o, start) else {
            eprintln!("no path from {zone} {kind} {o:?}");
            continue;
        };
        found += 1;
        // Every stretch of the way is over the mesh.
        let mut from = *o;
        for w in &p {
            let steps = (((w.point[0] - from[0]).hypot(w.point[1] - from[1])) / 16.0).ceil().max(1.0) as usize;
            for k in 1..steps {
                let t = k as f32 / steps as f32;
                let q = [from[0] + (w.point[0] - from[0]) * t, from[1] + (w.point[1] - from[1]) * t, from[2] + (w.point[2] - from[2]) * t];
                assert!(mesh.locate(q, 160.0, 160.0).is_some(), "{zone} {kind} {o:?}: {q:?} on the way is off the mesh ({p:?})");
            }
            // (A traversal starts here: the walk goes on from where it lands.)
            from = w.link.map_or(w.point, |l| mesh.links[l as usize].to);
        }
        if p.len() > 1 {
            bent += 1;
        }
        if p.iter().any(|w| w.link.is_some()) {
            through_links += 1;
        }
    }
    eprintln!("paths: {found} of {tried} start-zone spawners; {bent} turn corners, {through_links} come through a window or traversal");
    assert!(tried > 0 && found * 10 >= tried * 8, "{found} of {tried}");
    // Map-wide (doors open): the spawn structs of every zone.
    let all = spawners.iter().filter(|s| s.1 != "dog_location").count();
    let ok = spawners.iter().filter(|s| s.1 != "dog_location" && mesh.find_path(s.2, start).is_some()).count();
    let reach = mesh.reachable(sf).iter().filter(|r| **r).count();
    eprintln!("map-wide: {ok} of {all} spawn structs have a path to the start; {reach} of {} faces reachable", mesh.faces.len());
    assert!(ok * 10 >= all * 7, "{ok} of {all}");
    assert!(bent > 0, "no path turned a corner");
}
