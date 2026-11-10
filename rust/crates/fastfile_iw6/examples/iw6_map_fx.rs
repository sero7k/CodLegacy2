//! A Ghosts map zone's ambient effects (its createfx script): what is placed, what the zone
//! holds of it: `iw6_map_fx <mp_map.ff>... [--list] [--types]`.
use std::collections::BTreeMap;

fn base(name: &str) -> &str {
    name.trim_start_matches(',')
        .rsplit('/')
        .next()
        .unwrap_or(name)
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let list = args.iter().any(|a| a == "--list");
    let types = args.iter().any(|a| a == "--types");
    for path in args.iter().filter(|a| !a.starts_with("--")) {
        let zone = fastfile_iw6::Zone::open(std::path::Path::new(path)).expect("zone");
        let (cap, stop) = fastfile_iw6::walk::walk_zone(&zone);
        println!(
            "{path}: walked {}/{}{}; {} zone effects",
            cap.walked,
            zone.assets.assets.len(),
            stop.map(|s| format!(" STOPPED {}", s.error))
                .unwrap_or_default(),
            cap.fx.len()
        );
        let Some(fx) = fastfile_iw6::createfx::read(&cap, &zone.image) else {
            println!("  no createfx script ({} scripts)", cap.script_files.len());
            continue;
        };
        for n in &fx.notes {
            println!("  note: {n}");
        }
        let mut by_base: BTreeMap<&str, &fastfile_iw6::walk::FxRec> = BTreeMap::new();
        for f in &cap.fx {
            if !f.name.starts_with(',') && !f.elems.is_empty() {
                by_base.insert(base(&f.name), f);
            }
        }
        let mut names: BTreeMap<&str, usize> = BTreeMap::new();
        for p in &fx.placed {
            *names.entry(p.fxid.as_str()).or_default() += 1;
        }
        let (mut in_zone, mut total) = (0, 0);
        for (name, n) in &names {
            total += n;
            let path = fx.paths.get(*name).map_or(*name, |p| p.as_str());
            let found = by_base.contains_key(base(path));
            if found {
                in_zone += n;
            }
            if list || !found {
                println!(
                    "  {:4} x {name} -> {path} {}",
                    n,
                    if found { "in zone" } else { "NOT IN ZONE" }
                );
            }
        }
        println!(
            "  createfx {}: {} placed ({} kinds, {} in the map zone), {} exploder, {} sound, {} paths",
            fx.script,
            total,
            names.len(),
            in_zone,
            fx.exploders,
            fx.sounds,
            fx.paths.len()
        );
        if types {
            let mut t: BTreeMap<u8, usize> = BTreeMap::new();
            let mut tech: BTreeMap<String, usize> = BTreeMap::new();
            let mut looping = 0;
            for f in by_base.values() {
                looping += usize::from(f.elem_counts.0 > 0);
                for e in &f.elems {
                    *t.entry(e.elem_type).or_default() += 1;
                    for m in &e.materials {
                        let tn = cap.materials[m.index as usize]
                            .techset
                            .map(|t| cap.name_of(t).to_owned())
                            .unwrap_or_default();
                        *tech.entry(tn).or_default() += 1;
                    }
                }
            }
            println!("  zone fx looping {looping}; elem types {t:?}; techsets {tech:?}");
        }
    }
}
