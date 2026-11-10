//! The technique sets a map's placed effects draw their sprites with:
//! `t7_fx_techs <map.ff>` prints per effect material its technique set, the
//! slots that have programs, and per slot the vertex declaration's
//! component ids and the vertex program's input semantics (`ISGN`), so the
//! effect draw's vertex data can be laid out as the program reads it.
use std::collections::{BTreeMap, BTreeSet};

fn isgn(p: &[u8]) -> Vec<String> {
    let Some(c) = fastfile_t7::dxbc::chunk(p, b"ISGN") else { return vec![] };
    let rd = |o: usize| c.get(o..o + 4).map_or(0, |b| u32::from_le_bytes(b.try_into().unwrap())) as usize;
    let n = rd(0);
    (0..n)
        .map(|i| {
            let e = 8 + 24 * i;
            let name_at = rd(e);
            let name: String = c.get(name_at..).unwrap_or(&[]).iter().take_while(|&&b| b != 0).map(|&b| b as char).collect();
            let mask = c.get(e + 20).copied().unwrap_or(0);
            format!("{name}{} r{} m{mask:x}", rd(e + 4), rd(e + 16))
        })
        .collect()
}

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let path = std::path::Path::new(&a[1]);
    let map = fastfile_t7::map::T7Map::load(&fastfile_t7::map::MapSources::beside(path)).expect("map");
    let mut used = BTreeSet::new();
    for block in map.entities.split('{').filter(|b| b.contains("\"classname\" \"fx\"")) {
        if let Some(i) = block.find("\"fxdef\" \"") {
            let rest = &block[i + 9..];
            used.insert(rest[..rest.find('"').unwrap_or(0)].to_owned());
        }
    }
    let mut todo: Vec<String> = used.into_iter().collect();
    let mut seen = BTreeSet::new();
    let mut techsets: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut types: BTreeMap<u8, usize> = BTreeMap::new();
    while let Some(n) = todo.pop() {
        if !seen.insert(n.clone()) {
            continue;
        }
        let Some(fx) = map.effect(&n) else { continue };
        for e in &fx.elems {
            let t = e.raw[0xc8];
            *types.entry(t).or_default() += 1;
            if t == 14 {
                todo.extend(e.visuals.iter().filter(|v| map.effect(v).is_some()).cloned());
            }
            todo.extend(e.effects.iter().flatten().cloned());
            if t > 6 {
                continue;
            }
            for v in &e.visuals {
                let ts = map.effect_material(v).and_then(|m| m.techset.clone()).unwrap_or_else(|| "?".into());
                techsets.entry(ts.trim_start_matches(',').to_owned()).or_default().push(v.clone());
            }
        }
    }
    println!("element types: {types:?}");
    let dir = path.parent().unwrap();
    let zones: Vec<_> = [path.file_stem().unwrap().to_str().unwrap(), "mp_common", "core_common", "core_post_gfx", "core_pre_gfx", "core_gfx", "core_patch"].iter().filter_map(|z| fastfile_t7::shared::zone(dir, z)).collect();
    for (ts, mats) in &techsets {
        let mats: BTreeSet<_> = mats.iter().collect();
        println!("{ts}: {} materials ({})", mats.len(), mats.iter().take(3).map(|s| s.as_str()).collect::<Vec<_>>().join(", "));
        let Some((img, rec)) = zones.iter().find_map(|z| z.capture.techset(ts).filter(|t| t.techniques.iter().any(Option::is_some)).map(|t| (&z.image, t))) else {
            println!("    technique set not found");
            continue;
        };
        for (slot, t) in rec.techniques.iter().enumerate() {
            let Some(t) = t else { continue };
            let Some(m) = t.methods[0].as_ref() else { continue };
            let decl = m.decl.and_then(|d| img.get(d..d + 0x78)).map(|d| d[..1 + d[0] as usize].to_vec()).unwrap_or_default();
            let vs = m.programs[0].and_then(|(at, n)| img.get(at..at + n)).map(isgn).unwrap_or_default();
            let refl = m.programs[0].and_then(|(at, n)| img.get(at..at + n)).and_then(fastfile_t7::dxbc::reflect);
            let bufs: Vec<String> = refl.map(|r| r.textures.iter().map(|t| t.0.clone()).chain(r.buffers.iter().map(|b| b.name.clone())).collect()).unwrap_or_default();
            if let Ok(d) = std::env::var("T7_DUMP") {
                for (k, st) in ["vs", "ps"].iter().enumerate() {
                    if let Some(b) = m.programs[k].and_then(|(at, n)| img.get(at..at + n)) {
                        let _ = std::fs::write(format!("{d}/{}_{slot}.{st}.dxbc", ts.replace(['/', '#'], "_")), b);
                    }
                }
            }
            println!("    slot {slot}: decl {decl:?} vs in {vs:?} vs binds {bufs:?}");
        }
    }
}
