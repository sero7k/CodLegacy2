//! Where a retail map's materials and pictures are: `cargo run -p
//! fastfile_h1 --release --example h1_retail -- <map.ff> [zone.ff ...]`.
//! Walks the map, then each other zone in turn (dropped after), and prints
//! per zone what it holds of what the map names: materials with a
//! technique set, pictures with pixels (inline or streamed), and their
//! size; then what of the map's world and placed models' materials is
//! held nowhere, and the names that look like the map's compass.
//!
//! `H1_KEYS=1`: instead, the map's numbered entity keys with sample values
//! (how `fastfile_h1::map::H1_KEYS` was read). `H1_PROBE_POINTS="x,y,z;.."`:
//! instead, the drawn world's heights under each point and the placed
//! models around it (how Shipment's spawn points were found to stand in
//! the air above the drawn ground).

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::PathBuf;

fn key(n: &str) -> String {
    fastfile_h1::shared::image_key(n)
}

fn main() {
    let paths: Vec<PathBuf> = std::env::args_os().skip(1).map(PathBuf::from).collect();
    let Some(map_path) = paths.first() else {
        eprintln!("usage: h1_retail <map.ff> [zone.ff ...]");
        std::process::exit(2);
    };
    let t = std::time::Instant::now();
    let zone = fastfile_h1::Zone::open(map_path).expect("map zone");
    let (cap, stop) = fastfile_h1::walk::walk_zone(&zone);
    let map = fastfile_h1::map::read_map(&cap, &zone.image, &zone.assets.script_strings);
    println!(
        "{}: inflated {} MB, {} assets, walk stop {:?} [{:.1}s]",
        map_path.display(),
        zone.image.len() >> 20,
        zone.assets.assets.len(),
        stop.map(|s| s.asset_index),
        t.elapsed().as_secs_f32()
    );
    // What the map's drawn world and placed models name.
    let mut wanted_mats: BTreeSet<String> = BTreeSet::new();
    if let Some(w) = &map.world {
        for s in &w.surfaces {
            if let Some(m) = s.material {
                wanted_mats.insert(key(&cap.materials[m.index as usize].name));
            }
        }
        for sm in &w.static_models {
            let Some(id) = sm.model else { continue };
            let Some(x) = cap.xmodels.get(id.index as usize) else {
                continue;
            };
            for m in x.materials.iter().flatten() {
                wanted_mats.insert(key(&cap.materials[m.index as usize].name));
            }
        }
        println!(
            "  lightmaps {:?}",
            w.lightmaps
                .iter()
                .map(|(a, b)| {
                    let f = |id: Option<fastfile_h1::walk::AssetId>| {
                        id.and_then(|i| cap.images.get(i.index as usize)).map(|r| {
                            (
                                r.name.clone(),
                                r.format,
                                r.width,
                                r.height,
                                r.streamed,
                                r.pixels.is_some(),
                            )
                        })
                    };
                    (f(*a), f(*b))
                })
                .collect::<Vec<_>>()
        );
    }
    let full_here = cap.materials.iter().filter(|m| m.techset.is_some()).count();
    println!(
        "  materials {} ({} with a technique set), the world and models name {}",
        cap.materials.len(),
        full_here,
        wanted_mats.len()
    );
    let (mut inline, mut streamed, mut refs, mut inline_bytes) = (0, 0, 0, 0usize);
    let mut map_images: HashMap<String, (bool, bool)> = HashMap::new();
    for i in &cap.images {
        if fastfile_h1::shared::is_reference(i) {
            refs += 1;
        } else if i.streamed {
            streamed += 1;
        } else {
            inline += 1;
            inline_bytes += i.pixels.map_or(0, |p| p.len);
        }
        map_images.insert(
            key(&i.name),
            (fastfile_h1::shared::is_reference(i), i.streamed),
        );
    }
    println!(
        "  images {}: {inline} inline ({} MB), {streamed} streamed, {refs} name only",
        cap.images.len(),
        inline_bytes >> 20
    );
    for i in cap.images.iter().filter(|i| {
        let n = i.name.to_ascii_lowercase();
        n.contains("compass") || n.contains("minimap")
    }) {
        println!(
            "  compass-like image {} (ref {})",
            i.name,
            fastfile_h1::shared::is_reference(i)
        );
    }
    for m in cap.materials.iter().filter(|m| {
        let n = m.name.to_ascii_lowercase();
        n.contains("compass") || n.contains("minimap")
    }) {
        println!("  compass-like material {}", m.name);
    }
    let corners = map.entities.matches("minimap_corner").count();
    println!("  entity text: {} minimap_corner", corners);
    for (at, _) in map.entities.match_indices("minimap_corner") {
        let a = map.entities[..at].rfind('{').unwrap_or(0);
        let b = map.entities[at..]
            .find('}')
            .map_or(map.entities.len(), |e| at + e + 1);
        println!("  corner entity: {:?}", &map.entities[a..b]);
    }
    if let Some(pts) = std::env::var_os("H1_PROBE_POINTS") {
        // "x,y,z;x,y,z": the drawn world's triangles under each point (their
        // height at the point and material), and the placed models near it.
        let w = map.world.as_ref().expect("world");
        for p in pts.to_string_lossy().split(';') {
            let v: Vec<f32> = p.split(',').filter_map(|x| x.parse().ok()).collect();
            let (x, y, z) = (v[0], v[1], v[2]);
            let mut hits = Vec::new();
            for s in &w.surfaces {
                if x < s.mins[0] || x > s.maxs[0] || y < s.mins[1] || y > s.maxs[1] {
                    continue;
                }
                let ix = &w.indices
                    [s.base_index as usize..s.base_index as usize + 3 * s.tri_count as usize];
                for t in ix.chunks_exact(3) {
                    let q = [t[0], t[1], t[2]]
                        .map(|i| w.vertices[(s.first_vertex + u32::from(i)) as usize].xyz);
                    let d = (q[1][1] - q[2][1]) * (q[0][0] - q[2][0])
                        + (q[2][0] - q[1][0]) * (q[0][1] - q[2][1]);
                    if d.abs() < 1e-6 {
                        continue;
                    }
                    let a = ((q[1][1] - q[2][1]) * (x - q[2][0])
                        + (q[2][0] - q[1][0]) * (y - q[2][1]))
                        / d;
                    let b = ((q[2][1] - q[0][1]) * (x - q[2][0])
                        + (q[0][0] - q[2][0]) * (y - q[2][1]))
                        / d;
                    let c = 1.0 - a - b;
                    if a >= 0.0 && b >= 0.0 && c >= 0.0 {
                        let h = a * q[0][2] + b * q[1][2] + c * q[2][2];
                        let m = s.material.map_or(String::new(), |m| {
                            cap.materials[m.index as usize].name.clone()
                        });
                        hits.push((h as i32, m));
                    }
                }
            }
            hits.sort();
            hits.dedup();
            println!("  point {x} {y} {z}: world under/over {hits:?}");
            for sm in &w.static_models {
                let dx = sm.origin[0] - x;
                let dy = sm.origin[1] - y;
                let name = sm.model.map_or(String::new(), |m| {
                    cap.xmodels[m.index as usize].name.clone()
                });
                let r = sm
                    .model
                    .map_or(0.0, |m| cap.xmodels[m.index as usize].radius)
                    * sm.scale;
                if (dx * dx + dy * dy).sqrt() < r {
                    println!(
                        "    model {name} at {:?} radius {r}",
                        sm.origin.map(|v| v as i32)
                    );
                }
            }
        }
        return;
    }
    if std::env::var_os("H1_KEYS").is_some() {
        let mut keys: BTreeMap<u32, (usize, BTreeSet<String>)> = BTreeMap::new();
        for line in map.entities.lines() {
            if let Some((k, v)) = line.trim().split_once(' ')
                && let Ok(k) = k.parse::<u32>()
            {
                let e = keys.entry(k).or_default();
                e.0 += 1;
                if e.1.len() < 6 {
                    e.1.insert(v.chars().take(40).collect());
                }
            }
        }
        for (k, (n, v)) in &keys {
            println!("  key {k}: {n} x {v:?}");
        }
        return;
    }
    drop(map);

    // Materials by name, first zone that defines it wins.
    let mut found_mats: BTreeMap<String, (String, String, Vec<String>)> = BTreeMap::new();
    for m in &cap.materials {
        if let Some(ts) = m.techset {
            let tex = m
                .textures
                .iter()
                .filter_map(|t| Some(cap.images.get(t.image?.index as usize)?.name.clone()))
                .collect();
            found_mats.insert(
                key(&m.name),
                ("map".into(), cap.name_of(ts).to_owned(), tex),
            );
        }
    }
    let mut image_where: HashMap<String, String> = HashMap::new();
    for (k, (r, s)) in &map_images {
        if !r {
            image_where.insert(
                k.clone(),
                if *s {
                    "map(streamed)".into()
                } else {
                    "map(inline)".into()
                },
            );
        }
    }
    drop(cap);
    drop(zone);
    for p in &paths[1..] {
        let t = std::time::Instant::now();
        let zone = match fastfile_h1::Zone::open(p) {
            Ok(z) => z,
            Err(e) => {
                println!("{}: {e}", p.display());
                continue;
            }
        };
        let (cap, stop) = fastfile_h1::walk::walk_zone(&zone);
        let name = p.file_stem().unwrap().to_string_lossy().into_owned();
        let mut mats_new = 0;
        let mut mats_full = 0;
        for m in &cap.materials {
            let Some(ts) = m.techset else { continue };
            mats_full += 1;
            let k = key(&m.name);
            if wanted_mats.contains(&k) && !found_mats.contains_key(&k) {
                mats_new += 1;
                let tex = m
                    .textures
                    .iter()
                    .filter_map(|t| Some(cap.images.get(t.image?.index as usize)?.name.clone()))
                    .collect();
                found_mats.insert(k, (name.clone(), cap.name_of(ts).to_owned(), tex));
            }
        }
        let (mut im_inline, mut im_stream, mut im_ref, mut bytes) = (0, 0, 0, 0usize);
        let mut bytes_big = 0usize;
        for i in &cap.images {
            if fastfile_h1::shared::is_reference(i) {
                im_ref += 1;
                continue;
            }
            if i.streamed {
                im_stream += 1;
            } else {
                im_inline += 1;
                let l = i.pixels.map_or(0, |p| p.len);
                bytes += l;
                if l > 400 * 1024 {
                    bytes_big += l;
                }
            }
            image_where.entry(key(&i.name)).or_insert_with(|| {
                format!("{name}({})", if i.streamed { "streamed" } else { "inline" })
            });
        }
        for i in cap.images.iter().filter(|i| {
            let n = i.name.to_ascii_lowercase();
            n.contains("compass_map") || n.contains("minimap_")
        }) {
            println!(
                "  {name}: image {} ref {} streamed {}",
                i.name,
                fastfile_h1::shared::is_reference(i),
                i.streamed
            );
        }
        for m in cap
            .materials
            .iter()
            .filter(|m| m.name.to_ascii_lowercase().contains("compass_map"))
        {
            println!(
                "  {name}: material {} techset {}",
                m.name,
                m.techset.is_some()
            );
        }
        println!(
            "{name}: inflated {} MB, {} assets, stop {:?}, materials {} ({mats_full} full, {mats_new} new of the map's), images inline {im_inline} ({} MB, {} MB over 400 KB) streamed {im_stream} name-only {im_ref} [{:.1}s]",
            zone.image.len() >> 20,
            zone.assets.assets.len(),
            stop.map(|s| (s.asset_index, s.error)),
            cap.materials.len(),
            bytes >> 20,
            bytes_big >> 20,
            t.elapsed().as_secs_f32()
        );
    }
    // The map's materials, and their colour maps.
    let mut missing = Vec::new();
    let mut by_zone: BTreeMap<String, usize> = BTreeMap::new();
    let mut img_by_zone: BTreeMap<String, usize> = BTreeMap::new();
    let mut img_missing = BTreeSet::new();
    for k in &wanted_mats {
        match found_mats.get(k) {
            None => missing.push(k.clone()),
            Some((z, _, tex)) => {
                *by_zone.entry(z.clone()).or_default() += 1;
                for t in tex {
                    match image_where.get(&key(t)) {
                        Some(w) => *img_by_zone.entry(w.clone()).or_default() += 1,
                        None if key(t).starts_with('$') => {}
                        None => {
                            img_missing.insert(key(t));
                        }
                    }
                }
            }
        }
    }
    println!("materials by defining zone: {by_zone:?}");
    println!(
        "missing materials ({}): {:?}",
        missing.len(),
        &missing[..missing.len().min(40)]
    );
    println!("their pictures by zone: {img_by_zone:?}");
    println!(
        "pictures held nowhere ({}): {:?}",
        img_missing.len(),
        img_missing.iter().take(40).collect::<Vec<_>>()
    );
    for (k, v) in found_mats
        .iter()
        .filter(|(k, _)| wanted_mats.contains(*k))
        .take(6)
    {
        println!("  e.g. {k}: {v:?}");
    }
}
