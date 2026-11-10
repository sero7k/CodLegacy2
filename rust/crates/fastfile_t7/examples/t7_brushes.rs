//! A map's collision brushes by contents and side surface flags, and the
//! static models that collide by model: `t7_brushes <map.ff>`.
use std::collections::BTreeMap;
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let map = fastfile_t7::map::T7Map::load(&fastfile_t7::map::MapSources::beside(
        std::path::Path::new(&a[1]),
    ))
    .expect("map");
    let clip = map.clip.as_ref().expect("clip");
    let owned: std::collections::HashSet<u16> = clip
        .models
        .iter()
        .skip(1)
        .flat_map(|m| m.leaf.brushes.iter().copied())
        .collect();
    let mut by: BTreeMap<(u32, Vec<u32>), (usize, usize)> = BTreeMap::new();
    for (i, b) in clip.brushes.iter().enumerate() {
        let mut f = b.plane_surface_flags.clone();
        f.sort_unstable();
        f.dedup();
        let e = by.entry((b.contents, f)).or_default();
        if owned.contains(&(i as u16)) {
            e.1 += 1
        } else {
            e.0 += 1
        }
    }
    println!("brushes (world, sub-model) by contents and side surface flags:");
    for ((c, f), (w, s)) in &by {
        println!("  contents {c:#x} sides {f:x?}: {w} world, {s} sub-model");
    }
    let mut sm: BTreeMap<(String, u32), usize> = BTreeMap::new();
    for m in &clip.static_models {
        *sm.entry((m.name.clone(), m.contents)).or_default() += 1;
    }
    println!("colliding static models:");
    for ((n, c), k) in sm {
        println!("  {n} contents {c:#x}: {k}");
    }
    for (i, (n, s, c)) in clip.materials.iter().enumerate() {
        println!("material {i}: {n} surface {s:#x} contents {c:#x}");
    }
}
