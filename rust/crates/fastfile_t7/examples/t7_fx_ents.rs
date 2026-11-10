//! The map's placed effects (`classname` `fx` blocks of its entity string)
//! and the keys they carry: `t7_fx_ents <map.ff>`.
use std::collections::BTreeMap;

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let z = fastfile_t7::Zone::open(std::path::Path::new(&a[1])).expect("zone");
    let w = fastfile_t7::walk::walk(&z);
    let src = fastfile_t7::map::MapSources::beside(std::path::Path::new(&a[1]));
    let map = fastfile_t7::map::T7Map::load(&src).expect("map");
    let mut keys: BTreeMap<String, usize> = BTreeMap::new();
    let mut names: BTreeMap<String, usize> = BTreeMap::new();
    let mut shown = 0;
    for block in map.entities.split('}') {
        if !block.contains("\"classname\" \"fx\"") {
            continue;
        }
        if shown < 3 {
            println!("{}", block.trim());
            shown += 1;
        }
        for l in block.lines() {
            if let Some((k, v)) = l.trim().strip_prefix('"').and_then(|l| l.split_once("\" \"")) {
                *keys.entry(k.to_owned()).or_default() += 1;
                if k == "fxdef" || k == "fxid" || k == "script_fxid" {
                    *names.entry(v.trim_end_matches('"').to_owned()).or_default() += 1;
                }
            }
        }
    }
    println!("keys: {keys:?}");
    println!("{} effect names:", names.len());
    let have: std::collections::HashSet<&str> = w.capture.fx.iter().map(|f| f.name.trim_start_matches(',')).collect();
    for (n, c) in names.iter().take(400) {
        println!("  {c:4} {n} {}", if have.contains(n.as_str()) { "(in zone)" } else { "" });
    }
    println!("zone effects: {}", w.capture.fx.len());
}
