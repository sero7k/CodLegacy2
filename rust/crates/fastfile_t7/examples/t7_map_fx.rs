//! A map's placed effects (`"classname" "fx"` entities) against what the
//! zones define: `t7_map_fx <map.ff>` prints each effect used (placements,
//! found or not), its sprite materials and whether their colour maps
//! decode, and runs down the effects they run.
use std::collections::{BTreeMap, BTreeSet};

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let map = fastfile_t7::map::T7Map::load(&fastfile_t7::map::MapSources::beside(std::path::Path::new(&a[1]))).expect("map");
    let mut used: BTreeMap<String, usize> = BTreeMap::new();
    for block in map.entities.split('{') {
        if !block.contains("\"classname\" \"fx\"") {
            continue;
        }
        if let Some(i) = block.find("\"fxdef\" \"") {
            let rest = &block[i + 9..];
            let name = &rest[..rest.find('"').unwrap_or(0)];
            *used.entry(name.to_owned()).or_default() += 1;
        }
    }
    let mut todo: Vec<String> = used.keys().cloned().collect();
    let mut seen = BTreeSet::new();
    let (mut mats_ok, mut mats_bad) = (BTreeSet::new(), BTreeMap::new());
    while let Some(n) = todo.pop() {
        if !seen.insert(n.clone()) {
            continue;
        }
        let Some(fx) = map.effect(&n) else {
            println!("{n}: NOT FOUND ({} placed)", used.get(&n).copied().unwrap_or(0));
            continue;
        };
        println!("{n}: {} elements ({} placed)", fx.elems.len(), used.get(&n).copied().unwrap_or(0));
        for e in &fx.elems {
            let t = e.raw[0xc8];
            if t == 14 {
                todo.extend(e.visuals.iter().filter(|v| map.effect(v).is_some()).cloned());
            }
            todo.extend(e.effects.iter().flatten().cloned());
            if t > 6 {
                continue;
            }
            for v in &e.visuals {
                if mats_ok.contains(v) || mats_bad.contains_key(v) {
                    continue;
                }
                let r = match map.effect_material(v) {
                    None => Err("material not found".to_owned()),
                    Some(m) => match m.textures.iter().find(|(h, _)| *h == 0xa0ab_1041) {
                        None => Err(format!("no colour map ({:?})", m.techset)),
                        Some((_, img)) => map.image(img, 256).map(|_| ()).map_err(|e| format!("{img}: {e}")),
                    },
                };
                match r {
                    Ok(()) => {
                        mats_ok.insert(v.clone());
                    }
                    Err(e) => {
                        mats_bad.insert(v.clone(), e);
                    }
                }
            }
        }
    }
    println!("\n{} effects, {} materials drawable, {} not:", seen.len(), mats_ok.len(), mats_bad.len());
    for (m, e) in &mats_bad {
        println!("  {m}: {e}");
    }
}
