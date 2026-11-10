//! A placed model's bounds: `t7_model_bounds <map.ff> <name substring>`.
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let map = fastfile_t7::map::T7Map::load(&fastfile_t7::map::MapSources::beside(std::path::Path::new(&a[1]))).expect("map");
    for m in map.models.iter().filter(|m| m.name.contains(&a[2])) {
        let (mut lo, mut hi) = ([f32::MAX; 3], [f32::MIN; 3]);
        for p in m.surfaces.iter().flat_map(|s| s.positions.iter()) {
            for k in 0..3 {
                lo[k] = lo[k].min(p[k]);
                hi[k] = hi[k].max(p[k]);
            }
        }
        println!("{} {:?}..{:?}", m.name, lo, hi);
    }
    for sm in &map.static_models {
        if map.models[sm.model].name.contains(&a[2]) {
            println!("  at {:?} scale {} cull {}", sm.origin, sm.scale, sm.cull_dist);
        }
    }
}
