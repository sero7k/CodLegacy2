//! Vertex declarations of every technique pass in the given zones: the
//! routing and the vertex shader's inputs, one line per distinct pair.
//!
//! `cargo run -p render_h1 --example h1_decls -- <zone.ff>...`

use std::collections::BTreeMap;

use fastfile_h1::AssetType;
use render_h1::pool::Pool;

fn main() {
    let mut pool = Pool::default();
    let mut decls = BTreeMap::new();
    let mut zones = Vec::new();
    for p in std::env::args().skip(1) {
        let zone = fastfile_h1::Zone::open(std::path::Path::new(&p)).expect("zone");
        let (cap, _) = fastfile_h1::walk::walk_zone(&zone);
        for d in &cap.vertex_decls {
            decls.insert(d.0.clone(), (d.1, d.2.clone()));
        }
        zones.push((zone, cap));
    }
    for (zone, cap) in &zones {
        pool.add(cap, &zone.image);
    }
    let mut seen: BTreeMap<(String, String), (usize, String)> = BTreeMap::new();
    for t in pool.techniques.values() {
        for pass in &t.pass_info {
            let (Some(decl), Some(vs)) = (&pass.vertex_decl, &pass.vertex_shader) else {
                continue;
            };
            let Some(b) = pool.program(AssetType::VertexShader, vs) else {
                continue;
            };
            let Ok(c) = dxbc_sm5::Container::parse(b) else {
                continue;
            };
            let inputs: Vec<String> = c
                .inputs
                .iter()
                .map(|i| format!("{}{}", i.semantic, i.semantic_index))
                .collect();
            let e = seen
                .entry((decl.clone(), inputs.join(" ")))
                .or_insert((0, t.name.clone()));
            e.0 += 1;
        }
    }
    for ((decl, inputs), (n, example)) in &seen {
        let routing = decls.get(decl).map_or(String::from("?"), |(c, r)| {
            format!(
                "{c}: {}",
                r.iter()
                    .map(|(s, d, m)| format!("{s}>{d}/{m:x}"))
                    .collect::<Vec<_>>()
                    .join(" ")
            )
        });
        println!("{decl} [{routing}] <- {inputs}  ({n}x, e.g. {example})");
    }
}
