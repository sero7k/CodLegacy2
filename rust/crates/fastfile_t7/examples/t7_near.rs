//! Static models near a point: `t7_near <map.ff> x y radius` prints each
//! placed model within `radius` (xy) with its materials' technique sets.
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let map = fastfile_t7::map::T7Map::load(&fastfile_t7::map::MapSources::beside(std::path::Path::new(&a[1]))).expect("map");
    let (x, y, r): (f32, f32, f32) = (a[2].parse().unwrap(), a[3].parse().unwrap(), a[4].parse().unwrap());
    let mut seen = std::collections::BTreeMap::new();
    for sm in &map.static_models {
        let d = ((sm.origin[0] - x).powi(2) + (sm.origin[1] - y).powi(2)).sqrt();
        if d <= r {
            let m = &map.models[sm.model];
            let e = seen.entry(m.name.clone()).or_insert((0, sm.origin));
            e.0 += 1;
        }
    }
    let mut world = std::collections::BTreeMap::new();
    for s in &map.surfaces {
        let c = [(s.mins[0] + s.maxs[0]) * 0.5, (s.mins[1] + s.maxs[1]) * 0.5];
        if ((c[0] - x).powi(2) + (c[1] - y).powi(2)).sqrt() <= r
            && let Some(i) = s.material
        {
            let e = world.entry(format!("{} [{}]", map.materials[i].name, map.materials[i].technique_set)).or_insert((0, s.mins, s.maxs));
            e.0 += s.tri_count as usize;
        }
    }
    for (n, (t, lo, hi)) in world {
        println!("world {n}: {t} tris {:?}..{:?}", lo, hi);
    }
    for (n, (k, o)) in seen {
        let m = map.models.iter().find(|m| m.name == n).unwrap();
        let mats: std::collections::BTreeSet<String> =
            m.surfaces.iter().filter_map(|s| s.material).map(|i| format!("{} [{}]", map.materials[i].name, map.materials[i].technique_set)).collect();
        println!("{n} x{k} at {:?}", o);
        for s in mats {
            println!("    {s}");
        }
    }
}
