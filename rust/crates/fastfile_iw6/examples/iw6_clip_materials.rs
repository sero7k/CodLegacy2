//! A Ghosts map's collision materials with their surface type (the flags'
//! bits 20..24, Ghosts' list), and how many of its render materials have a
//! collision material of the same name: `iw6_clip_materials <mp_map.ff>`.
use std::collections::{BTreeMap, HashSet};

fn main() {
    let path = std::env::args().nth(1).expect("zone");
    let zone = fastfile_iw6::Zone::open(std::path::Path::new(&path)).expect("open");
    let (cap, _) = fastfile_iw6::walk::walk_zone(&zone);
    let map = fastfile_iw6::map::read_map(&cap, &zone.image, &zone.assets.script_strings);
    let clip = map.clip.as_ref().expect("clip");
    let w = map.world.as_ref().expect("world");
    let clip_names: BTreeMap<String, u32> = clip
        .materials
        .iter()
        .map(|(n, f, _)| (n.trim_start_matches(',').to_ascii_lowercase(), (*f as u32 >> 20) & 0x3f))
        .collect();
    let mut by_type: BTreeMap<u32, usize> = BTreeMap::new();
    for t in clip_names.values() {
        *by_type.entry(*t).or_default() += 1;
    }
    println!("{} clip materials, by surface type {by_type:?}", clip_names.len());
    let mut render: HashSet<u32> = HashSet::new();
    for s in &w.surfaces {
        if let Some(m) = s.material {
            render.insert(m.index);
        }
    }
    let (mut hit, mut miss) = (0, 0);
    let mut shown = 0;
    for mi in &render {
        let n = cap.materials[*mi as usize].name.trim_start_matches(',').to_ascii_lowercase();
        let bare = n.strip_prefix("wc/").unwrap_or(&n).to_owned();
        if clip_names.contains_key(&n) || clip_names.contains_key(&bare) {
            hit += 1;
        } else {
            miss += 1;
            if shown < 12 && !n.starts_with('*') {
                println!("  no clip material for {n}");
                shown += 1;
            }
        }
    }
    println!("render materials with a clip material of the same name: {hit}, without: {miss}");
    for (n, t) in clip_names.iter().take(25) {
        println!("  clip {n}: type {t}");
    }
}
