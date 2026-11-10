//! A Ghosts map's world materials by triangles drawn: material name, technique
//! set and the picture names its texture slots hold (`iw6_material_names
//! <mp_map.ff>`, `IW6_NAMES_TOP=n` how many, default 80).
use std::collections::BTreeMap;

fn main() {
    let top: usize = std::env::var("IW6_NAMES_TOP").ok().and_then(|v| v.parse().ok()).unwrap_or(80);
    let path = std::env::args().nth(1).expect("zone");
    let zone = fastfile_iw6::Zone::open(std::path::Path::new(&path)).expect("open");
    let (cap, _) = fastfile_iw6::walk::walk_zone(&zone);
    let map = fastfile_iw6::map::read_map(&cap, &zone.image, &zone.assets.script_strings);
    let w = map.world.expect("world");
    let mut use_: BTreeMap<u32, usize> = BTreeMap::new();
    for s in &w.surfaces {
        if let Some(m) = s.material {
            *use_.entry(m.index).or_default() += s.tri_count as usize;
        }
    }
    let mut v: Vec<_> = use_.into_iter().collect();
    v.sort_by_key(|&(_, t)| std::cmp::Reverse(t));
    for (mi, tris) in v.into_iter().take(top) {
        let m = &cap.materials[mi as usize];
        let ts = m.techset.map(|t| cap.name_of(t).trim_start_matches(',').to_owned()).unwrap_or_default();
        let tex: Vec<String> = m
            .textures
            .iter()
            .filter_map(|t| t.image.map(|i| format!("{:08x}={}", t.name_hash, cap.images[i.index as usize].name)))
            .collect();
        println!("{tris:>7} {} [{ts}] {}", m.name, tex.join(" "));
    }
}
