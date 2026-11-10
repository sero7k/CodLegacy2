//! `t7_brush_models <map.ff>`: the map's brush sub-models (`*<n>`): their
//! world surfaces, materials and bounds, the entities that name them
//! (classname, targetname, origin, angles) and what the map read made of
//! their surfaces (`fastfile_t7::map::brush_places`: moved to a drawn
//! entity, or not drawn).
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let zone = std::path::PathBuf::from(a.get(1).expect("map zone"));
    let map = fastfile_t7::map::T7Map::load(&fastfile_t7::map::MapSources::beside(&zone)).expect("map");
    for l in map.report.iter().filter(|l| l.contains("brush")) {
        println!("map: {l}");
    }
    let blocks = fastfile_t7::spawn::blocks(&map.entities);
    for (k, b) in map.brush_models.iter().enumerate() {
        let names: Vec<String> = blocks
            .iter()
            .filter(|(_, keys)| keys.iter().any(|(n, v)| n == "model" && v == &format!("*{k}")))
            .map(|(_, keys)| {
                let g = |n: &str| keys.iter().find(|(a, _)| a == n).map_or("", |(_, v)| v.as_str()).to_owned();
                format!("{} tn={} origin={} angles={}", g("classname"), g("targetname"), g("origin"), g("angles"))
            })
            .collect();
        let range = || (b.first_surface..b.first_surface.saturating_add(b.surface_count)).map(|i| i as usize);
        let mats: std::collections::BTreeSet<String> = range().filter_map(|i| map.surfaces.get(i)).filter_map(|s| s.material.map(|m| map.materials[m].name.clone())).collect();
        let place = range().next().and_then(|i| map.brush_places.get(i)).map(|p| format!("{p:?}")).unwrap_or_default();
        println!("*{k}: surfaces {}+{} bounds {:?}..{:?} materials {mats:?} entities {names:?} -> {place}", b.first_surface, b.surface_count, b.mins, b.maxs);
    }
}
