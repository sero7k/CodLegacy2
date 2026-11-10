//! A BO3 map's spawn points: `t7_spawns <map.ff>` prints `spawn class x y z
//! angles`.
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let map = fastfile_t7::map::T7Map::load(&fastfile_t7::map::MapSources::beside(
        std::path::Path::new(&a[1]),
    ))
    .expect("map");
    let (mut class, mut origin, mut angles) = (String::new(), String::new(), String::new());
    for line in map.entities.lines() {
        let l = line.trim();
        if l == "{" {
            class.clear();
            origin.clear();
            angles.clear();
        } else if l == "}" {
            if class.contains("spawn") && !origin.is_empty() {
                println!("spawn {class} {origin} {angles}");
            }
        } else if let Some(r) = l.strip_prefix("\"classname\" ") {
            class = r.trim_matches('"').to_owned();
        } else if let Some(r) = l.strip_prefix("\"origin\" ") {
            origin = r.trim_matches('"').to_owned();
        } else if let Some(r) = l.strip_prefix("\"angles\" ") {
            angles = r.trim_matches('"').to_owned();
        }
    }
}
