//! Lists a map zone's scriptables: each def with its instance count, parts, states, health and
//! events (`--full` prints every event), and the dynamic entity kinds:
//! `cargo run -p fastfile_h1 --release --example h1_scriptables -- <zone.ff> [--full] [name-part]`

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let path = std::path::PathBuf::from(args.first().expect("zone path"));
    let full = args.iter().any(|a| a == "--full");
    let want = args.iter().skip(1).find(|a| !a.starts_with("--")).cloned();
    let zone = fastfile_h1::Zone::open(&path).expect("zone");
    let (map, cap) = fastfile_h1::map::IwMap::load(&zone);
    let Some(clip) = &map.clip else {
        println!("no clip map");
        return;
    };
    let strings = &zone.assets.script_strings;
    let sname = |n: u32| strings.get(n as usize).and_then(|s| s.clone()).unwrap_or_else(|| format!("#{n}"));
    let mut counts: std::collections::BTreeMap<&str, usize> = Default::default();
    for i in &clip.scriptables {
        *counts.entry(i.def.as_str()).or_default() += 1;
    }
    println!("{} scriptable instances of {} defs ({} defs captured)", clip.scriptables.len(), counts.len(), cap.scriptable_defs.len());
    for (name, n) in &counts {
        if want.as_ref().is_some_and(|w| !name.contains(w.as_str())) {
            continue;
        }
        let Some(d) = cap.scriptable_defs.iter().find(|d| d.name == *name) else {
            println!("{name} x{n}: def not in this zone");
            continue;
        };
        println!(
            "{name} x{n}: model {:?} kind {} flags {:#x} parts {} stream {} consts {}",
            d.base_model,
            d.kind,
            d.flags,
            d.parts.len(),
            d.event_stream_size,
            d.event_constants_size
        );
        for p in &d.parts {
            println!("  part {} flags {:#x}", sname(p.name), p.flags);
            for (si, s) in p.states.iter().enumerate() {
                println!(
                    "    state {si} {} tag {} health {} dmgflags {:#x} events {}",
                    sname(s.name),
                    sname(s.tag),
                    s.max_health,
                    s.damage_flags,
                    s.events.len()
                );
                if full {
                    for e in &s.events {
                        println!("      {e:?}");
                    }
                }
            }
        }
    }
    let mut kinds: std::collections::BTreeMap<i32, usize> = Default::default();
    for d in &clip.dyn_ents {
        *kinds.entry(d.kind).or_default() += 1;
    }
    println!("dynents by kind: {kinds:?}");
}
