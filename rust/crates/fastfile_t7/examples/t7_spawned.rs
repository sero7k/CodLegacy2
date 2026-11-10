//! Which of a BO3 map's entity blocks the game spawns (`fastfile_t7::spawn`):
//! `t7_spawned <map.ff>` prints the count by kind and the classes left out.
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let path = std::path::Path::new(&a[1]);
    let map = fastfile_t7::map::T7Map::load(&fastfile_t7::map::MapSources::beside(path)).expect("map");
    let mp = !map.name.starts_with("zm_") && !map.name.starts_with("cp_");
    let mut kinds: std::collections::BTreeMap<String, usize> = std::collections::BTreeMap::new();
    for (_, keys) in fastfile_t7::spawn::blocks(&map.entities) {
        *kinds.entry(format!("{:?}", fastfile_t7::spawn::spawn_class(&keys, mp))).or_default() += 1;
    }
    println!("{kinds:?}");
    let (_, dropped) = fastfile_t7::spawn::as_spawned(&map.entities, mp);
    println!("left out: {dropped:?}");
}
