//! A BO3 map's entity string: `t7_ents <map.ff>` prints it.
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let map = fastfile_t7::map::T7Map::load(&fastfile_t7::map::MapSources::beside(std::path::Path::new(&a[1]))).expect("map");
    print!("{}", map.entities);
}
