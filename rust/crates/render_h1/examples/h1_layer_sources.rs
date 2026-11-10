//! Per world vertex format: the layer-stream sources (routing source ids
//! from 5 up) the passes of its technique sets read, with their masks.
//!
//! `cargo run -p render_h1 --example h1_layer_sources -- <techsets.ff>...`

use std::collections::{BTreeMap, BTreeSet, HashMap};

fn main() {
    let mut decls: HashMap<String, Vec<(u8, u8, u8)>> = HashMap::new();
    let mut caps = Vec::new();
    for p in std::env::args().skip(1) {
        let zone = fastfile_h1::Zone::open(std::path::Path::new(&p)).expect("zone");
        let (cap, _) = fastfile_h1::walk::walk_zone(&zone);
        for d in &cap.vertex_decls {
            decls.insert(d.0.clone(), d.2.clone());
        }
        caps.push(cap);
    }
    let mut techniques: HashMap<String, Vec<Option<String>>> = HashMap::new();
    for cap in &caps {
        for t in &cap.techniques {
            techniques.insert(
                t.name.clone(),
                t.pass_info.iter().map(|p| p.vertex_decl.clone()).collect(),
            );
        }
    }
    let mut out: BTreeMap<u8, BTreeMap<String, usize>> = BTreeMap::new();
    for cap in &caps {
        for ts in &cap.techsets {
            let mut srcs: BTreeSet<(u8, u8)> = BTreeSet::new();
            for (_, tn, _) in &ts.techniques {
                for d in techniques.get(tn).into_iter().flatten().flatten() {
                    for &(s, _, m) in decls.get(d).into_iter().flatten() {
                        if s >= 5 {
                            srcs.insert((s, m));
                        }
                    }
                }
            }
            let k: Vec<String> = srcs.iter().map(|(s, m)| format!("{s}/{m:x}")).collect();
            *out.entry(ts.world_vert_format)
                .or_default()
                .entry(k.join(" "))
                .or_default() += 1;
        }
    }
    for (f, m) in out {
        println!("wvf {f}: {m:?}");
    }
}
