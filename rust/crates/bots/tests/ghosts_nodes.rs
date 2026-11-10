//! bo2mp: mp_frag's own path nodes as the bots' route graph, over the map's real collision.
//! Skips when no Ghosts zones are staged (`BO2MP_IW6_ZONES`, else the owner's upload folder).

use bots::{BakeTraces, EXPAND_CAP, HullTrace, TraversalKind, find_path, from_path_nodes};

struct Map(asset_world::ClipCollision);

impl Map {
    fn hull(&self, start: [f32; 3], end: [f32; 3]) -> HullTrace {
        let (fraction, normal, startsolid) = asset_world::world_iw3::trace_clip_collision_hit(
            &self.0,
            start,
            end,
            [-15.0, -15.0, 0.0],
            [15.0, 15.0, 70.0],
            asset_world::MASK_PLAYER_SOLID,
        );
        HullTrace { fraction, normal, endpos: std::array::from_fn(|k| start[k] + (end[k] - start[k]) * fraction), startsolid }
    }
}

impl BakeTraces for Map {
    fn bake_hull(&self, start: [f32; 3], end: [f32; 3]) -> HullTrace {
        self.hull(start, end)
    }
    fn bake_navigation(&self, start: [f32; 3], end: [f32; 3]) -> HullTrace {
        self.hull(start, end)
    }
}

/// The load hands the path nodes over through a process-wide slot: the maps are checked one at a time.
static ONE_AT_A_TIME: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[test]
fn mp_frag_routes_over_its_own_nodes() {
    routes_over_its_own_nodes("mp_frag", 1902);
}

#[test]
fn mp_snow_routes_over_its_own_nodes() {
    routes_over_its_own_nodes("mp_snow", 2079);
}

fn routes_over_its_own_nodes(map_name: &str, node_count: usize) {
    let _guard = ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner());
    let dir = std::env::var_os("BO2MP_IW6_ZONES")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| "/mnt/user-data/uploads/ghosts_game_files/zone".into());
    let zone = dir.join(format!("{map_name}.ff"));
    if !zone.exists() {
        return;
    }
    let Some((clip, spawns)) = assets::ghosts_collision_for_tests(&zone) else {
        panic!("{map_name} collision");
    };
    let nodes = asset_world::iw6_path_nodes().expect("the load hands over the map's path nodes");
    assert_eq!(nodes.len(), node_count);
    // The map's own path zone gave the links when it is installed beside the map.
    let own = dir.join(format!("{map_name}_path.ff")).is_file();
    assert_eq!(nodes.iter().any(|n| !n.links.is_empty()), own);
    let map = Map(clip);
    if std::env::var_os("GHOSTS_NODES_DIAG").is_some() {
        let (mut solid, mut grounded, mut pairs, mut clear2, mut clear18) = (0, 0, 0, 0, 0);
        for n in nodes.iter() {
            let o = n.origin;
            let t = map.hull([o[0], o[1], o[2] + 18.0], [o[0], o[1], o[2] - 24.0]);
            solid += usize::from(t.startsolid);
            grounded += usize::from(!t.startsolid && t.fraction < 1.0 && t.normal[2] >= 0.7);
        }
        for (i, a) in nodes.iter().enumerate().step_by(7) {
            for b in nodes.iter().skip(i + 1) {
                let d = ((a.origin[0] - b.origin[0]).powi(2) + (a.origin[1] - b.origin[1]).powi(2)).sqrt();
                if d > 70.0 || (a.origin[2] - b.origin[2]).abs() > 18.0 {
                    continue;
                }
                pairs += 1;
                for (lift, c) in [(2.0, &mut clear2), (18.0, &mut clear18)] {
                    let t = map.hull([a.origin[0], a.origin[1], a.origin[2] + lift], [b.origin[0], b.origin[1], b.origin[2] + lift]);
                    *c += usize::from(!t.startsolid && t.fraction >= 1.0);
                }
            }
        }
        let mut hist = std::collections::BTreeMap::new();
        for n in nodes.iter().filter(|n| n.ty == 1) {
            let o = n.origin;
            // from just above the node down: the floor under it
            let t = map.hull([o[0], o[1], o[2] + 4.0], [o[0], o[1], o[2] - 200.0]);
            let key = if t.startsolid { "solid".to_string() } else if t.fraction >= 1.0 { "none".to_string() } else { format!("{}", ((o[2] + 4.0 - t.endpos[2]) / 8.0).round() as i32 * 8) };
            *hist.entry(key).or_insert(0usize) += 1;
        }
        eprintln!("diag: path-node height above the floor under it (hull, from +4): {hist:?}");
        eprintln!("diag: {} nodes, startsolid-at-top {solid}, grounded {grounded}; near pairs {pairs}: clear at +2 {clear2}, at +18 {clear18}", nodes.len());
    }
    let g = from_path_nodes(&map, &nodes, 1);
    let edges: usize = g.adj.iter().map(Vec::len).sum();
    let walks: usize = g.adj.iter().flatten().filter(|e| e.kind == TraversalKind::Walk).count();
    let jumps: usize = g.adj.iter().flatten().filter(|e| e.kind == TraversalKind::Negotiate).count();
    let mut sizes = std::collections::HashMap::new();
    for c in &g.component {
        *sizes.entry(*c).or_insert(0usize) += 1;
    }
    let (main, main_size) = sizes.iter().max_by_key(|(_, n)| **n).map(|(c, n)| (*c, *n)).unwrap();
    // Symmetry of the walk links (the test is made from both ends).
    let symmetric = g
        .adj
        .iter()
        .enumerate()
        .flat_map(|(i, es)| es.iter().filter(|e| e.kind == TraversalKind::Walk).map(move |e| (i, e.to as usize)))
        .filter(|&(i, j)| g.adj[j].iter().any(|e| e.to as usize == i))
        .count();
    eprintln!(
        "{map_name}: {} nodes, {edges} links ({walks} walks, {jumps} traversals), {} components, largest {main_size}, walk symmetry {symmetric}/{walks}",
        g.nodes.len(),
        sizes.len()
    );
    assert!(jumps >= 137 || map_name != "mp_frag");
    if own {
        // The engine's links: every node is in one piece (its zone nodes are not part of the graph),
        // and every link of the asset is an edge.
        let want: usize = nodes.iter().map(|n| n.links.len()).sum();
        assert_eq!(edges, want);
        assert_eq!(sizes.len(), 1, "{map_name}: the map's links join every node");
    }
    assert!(walks > 3 * g.nodes.len());
    assert!(symmetric * 10 >= walks * 9, "walk links should mostly be mutual");
    let mut by_size: Vec<usize> = sizes.values().copied().collect();
    by_size.sort_unstable_by(|a, b| b.cmp(a));
    eprintln!("component sizes: {:?}, {} single nodes", &by_size[..by_size.len().min(8)], by_size.iter().filter(|n| **n == 1).count());
    assert!(main_size * 10 >= g.nodes.len() * 7, "most nodes should share one component");
    // Bots spawn in the main component and can route between spawns.
    let mut routed = 0;
    let mut tried = 0;
    let near = |p: [f32; 3]| bots_nearest(&g, p);
    for a in spawns.iter().step_by(3) {
        for b in spawns.iter().step_by(5) {
            if a == b {
                continue;
            }
            let (Some(i), Some(j)) = (near(*a), near(*b)) else { continue };
            if g.component[i] != main || g.component[j] != main {
                continue;
            }
            tried += 1;
            let mut budget = EXPAND_CAP * 8;
            if find_path(&g, g.nodes[i], g.nodes[j], &mut budget).is_ok() {
                routed += 1;
            }
        }
    }
    let attached = spawns
        .iter()
        .filter(|s| bots::nearest_within(&g, **s, 256.0).is_some_and(|n| g.component[n as usize] == main))
        .count();
    eprintln!("spawn routes (between the spawns\' nearest main-component nodes): {routed}/{tried}; spawns whose nearest node is in the main component: {attached}/{}", spawns.len());
    assert!(attached * 10 >= spawns.len() * 8);
    // A `jump_down` is one way, so a few pockets drop in and do not climb out.
    assert!(tried > 10 && routed * 100 >= tried * 90);
}

fn bots_nearest(g: &bots::NavGraph, p: [f32; 3]) -> Option<usize> {
    g.nodes
        .iter()
        .enumerate()
        .filter(|(_, n)| (n[2] - p[2]).abs() < 64.0)
        .min_by(|a, b| {
            let d = |n: &[f32; 3]| (n[0] - p[0]).powi(2) + (n[1] - p[1]).powi(2);
            d(a.1).total_cmp(&d(b.1))
        })
        .filter(|(_, n)| ((n[0] - p[0]).powi(2) + (n[1] - p[1]).powi(2)).sqrt() < 256.0)
        .map(|(i, _)| i)
}

/// The doors the scripts close (`disconnectpaths` on a brush model) cut the links through their
/// volume; `connectpaths` gives them back.
#[test]
fn mp_frag_doors_close_and_open_routes() {
    let _guard = ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner());
    let dir = std::env::var_os("BO2MP_IW6_ZONES")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| "/mnt/user-data/uploads/ghosts_game_files/zone".into());
    let zone = dir.join("mp_frag.ff");
    if !zone.exists() {
        return;
    }
    let Some((clip, _)) = assets::ghosts_collision_for_tests(&zone) else {
        panic!("mp_frag collision");
    };
    let nodes = asset_world::iw6_path_nodes().expect("path nodes");
    let pack = asset_world::iw6_scripts().expect("the load hands over the scripts");
    let mut g = from_path_nodes(&Map(clip), &nodes, 1);
    let full: usize = g.adj.iter().map(Vec::len).sum();
    // The doors the scripts shut at the start (entity origin, brush model number), from the
    // script run (`mp_frag_events_run_from_its_scripts` in the sim): the pair that opens again.
    let doors = [(51u32, [451.0f32, 1542.0, 240.0]), (58, [447.0, 1476.0, 240.0])];
    let mut closed = 0;
    for (n, origin) in doors {
        let (_, mins, maxs) = *pack.brush_bounds.iter().find(|b| b.0 == n).expect("the brush model's bounds");
        let (lo, hi): ([f32; 3], [f32; 3]) = (std::array::from_fn(|k| origin[k] + mins[k]), std::array::from_fn(|k| origin[k] + maxs[k]));
        eprintln!("door *{n}: local {mins:?}..{maxs:?}, world {lo:?}..{hi:?}");
        assert!((0..3).all(|k| hi[k] - lo[k] > 0.5 && hi[k] - lo[k] < 600.0), "a door-sized box");
        assert!(g.set_gate(n, lo, hi, true));
        let now: usize = g.adj.iter().map(Vec::len).sum();
        assert!(now < full - closed, "the door at *{n} holds some links shut");
        closed = full - now;
    }
    let held: Vec<(usize, usize)> = g
        .gates
        .boxes
        .iter()
        .flat_map(|b| b.links.iter().map(|&(a, n)| (a as usize, g.gates.full_adj[a as usize][n as usize].to as usize)))
        .collect();
    eprintln!("doors closed: {} links held of {full}", held.len());
    // For the nodes either side of a shut door, the direct step is gone while it is shut and back
    // once it opens.
    let (a, b) = held[0];
    assert!(!g.adj[a].iter().any(|e| e.to as usize == b));
    for (n, origin) in doors {
        let (_, mins, maxs) = *pack.brush_bounds.iter().find(|b| b.0 == n).unwrap();
        let (lo, hi) = (std::array::from_fn(|k| origin[k] + mins[k]), std::array::from_fn(|k| origin[k] + maxs[k]));
        assert!(g.set_gate(n, lo, hi, false));
    }
    assert!(g.adj[a].iter().any(|e| e.to as usize == b), "opened again");
    assert_eq!(g.adj.iter().map(Vec::len).sum::<usize>(), full);
}
