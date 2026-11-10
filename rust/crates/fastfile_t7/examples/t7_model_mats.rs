//! The materials a map's models draw with: `t7_model_mats <map.ff> <model
//! substring>` prints each matching model and, per surface, its material
//! and technique set.
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let map = fastfile_t7::map::T7Map::load(&fastfile_t7::map::MapSources::beside(std::path::Path::new(&a[1]))).expect("map");
    let mut seen = std::collections::BTreeSet::new();
    for m in map.models.iter().filter(|m| m.name.contains(&a[2])) {
        if !seen.insert(m.name.clone()) {
            continue;
        }
        println!("{}", m.name);
        let mut mats = std::collections::BTreeSet::new();
        for s in &m.surfaces {
            if let Some(i) = s.material {
                let mm = &map.materials[i];
                mats.insert(format!("{} [{}]", mm.name, mm.technique_set));
            }
        }
        for x in mats {
            println!("    {x}");
        }
    }
}
