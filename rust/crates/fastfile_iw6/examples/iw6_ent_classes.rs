//! Counts the entity classnames in a zone's MapEnts: `iw6_ent_classes <zone.ff> [classname-substring]`
use std::collections::BTreeMap;
fn main() {
    let mut a = std::env::args().skip(1);
    let path = a.next().expect("zone");
    let filt = a.next();
    let zone = fastfile_iw6::Zone::open(std::path::Path::new(&path)).expect("open");
    let (cap, _) = fastfile_iw6::walk::walk_zone(&zone);
    for me in &cap.map_ents {
        let mut m: BTreeMap<String, usize> = BTreeMap::new();
        let blocks = fastfile_iw6::maplogic::blocks(&me.entity_string);
        for b in &blocks {
            let c = b.iter().find(|(k, _)| k == "classname").map(|(_, v)| v.as_str()).unwrap_or("?").to_string();
            *m.entry(c).or_default() += 1;
        }
        println!("{}: {} ents, entity string {} bytes", me.name, blocks.len(), me.entity_string.len());
        for (k, v) in &m {
            println!("  {k}: {v}");
        }
        // Entities whose values name an effect (ambient effect entities).
        let mut fxy: BTreeMap<String, usize> = BTreeMap::new();
        for b in &blocks {
            if b.iter().any(|(_, v)| v.contains("vfx") || v.starts_with("fx/")) {
                let c = b.iter().find(|(k, _)| k == "classname").map(|(_, v)| v.as_str()).unwrap_or("?");
                *fxy.entry(c.to_owned()).or_default() += 1;
            }
        }
        println!("  entities naming an effect: {fxy:?}");
        if let Some(f) = &filt {
            let mut shown: BTreeMap<String, usize> = BTreeMap::new();
            for b in &blocks {
                if let Some((_, c)) = b.iter().find(|(k, v)| k == "classname" && v.contains(f.as_str())) {
                    let n = shown.entry(c.clone()).or_default();
                    if *n < 3 {
                        println!("--- {:?}", b);
                    }
                    *n += 1;
                }
            }
        }
    }
}
