//! A BO2 map zone's path nodes and spawn points as text: `t6_nav_dump
//! <zone.ff>` prints `node x y z type links` and `spawn class x y z yaw`.
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let c = asset_t6::capture_zone(std::path::Path::new(&a[1])).expect("zone");
    for n in &c.path_nodes {
        println!(
            "node {} {} {} {} {}",
            n.origin[0],
            n.origin[1],
            n.origin[2],
            n.ty,
            n.links.len()
        );
    }
    for ents in &c.map_ents {
        let mut class = String::new();
        let mut origin = String::new();
        let mut angles = String::new();
        for line in ents.lines() {
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
}
