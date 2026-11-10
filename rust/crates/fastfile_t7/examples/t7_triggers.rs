//! A map's trigger models' hulls: `t7_triggers <map.ff> [first] [last]`.
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let map = fastfile_t7::map::T7Map::load(&fastfile_t7::map::MapSources::beside(std::path::Path::new(&a[1]))).expect("map");
    let first: usize = a.get(2).and_then(|v| v.parse().ok()).unwrap_or(0);
    let last: usize = a.get(3).and_then(|v| v.parse().ok()).unwrap_or(map.trigger_models.len());
    for (i, hulls) in map.trigger_models.iter().enumerate().take(last + 1).skip(first) {
        for (k, (m, h)) in hulls.iter().enumerate() {
            println!("?{i}: middle {:?} half {:?}", m.map(|v| v.round()), h.map(|v| v.round()));
            for (d, mid, half) in map.trigger_slabs.get(i).and_then(|t| t.get(k)).into_iter().flatten() {
                println!("    slab dir {:?} middle {mid:.0} half {half:.0}", d.map(|v| (v * 1000.0).round() / 1000.0));
            }
        }
    }
}
