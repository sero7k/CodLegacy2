//! The vertex inputs IW7's techniques ask for: `iw7_vs_inputs <techsets.ff>
//! [technique substring]` counts the distinct input signatures (semantic,
//! index, mask) of the passes' vertex programs, with the vertex declaration's
//! routing (source -> destination) of one pass of each.
use std::collections::BTreeMap;

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let zone = fastfile_iw7::Zone::open(std::path::Path::new(&a[1])).expect("zone");
    let want = a.get(2).cloned().unwrap_or_default();
    let (w, _) = fastfile_iw7::walk::walk_fitted(&zone);
    let mut sigs: BTreeMap<Vec<(String, u32, u8)>, (usize, String)> = BTreeMap::new();
    for ts in &w.cap.techsets {
        for (tech, passes) in ts.techniques.iter().zip(&ts.passes) {
            if !tech.1.contains(want.as_str()) {
                continue;
            }
            for p in passes {
                let Some(s) = p.vertex_shader.and_then(|id| w.cap.shader_programs.get(&(fastfile_iw7::AssetType::VertexShader, id.index))) else { continue };
                let Ok(c) = dxbc_sm5::Container::parse(&zone.image[s.pos..s.pos + s.len]) else { continue };
                let sig: Vec<(String, u32, u8)> = c.inputs.iter().filter(|e| e.system_value == 0).map(|e| (e.semantic.clone(), e.semantic_index, e.mask)).collect();
                let e = sigs.entry(sig).or_insert((0, ts.name.clone()));
                e.0 += 1;
            }
        }
    }
    let mut v: Vec<_> = sigs.into_iter().collect();
    v.sort_by(|a, b| b.1.0.cmp(&a.1.0));
    for (sig, (n, set)) in v.iter().take(14) {
        let s: Vec<String> = sig.iter().map(|(n, i, m)| format!("{n}{i}:{m:x}")).collect();
        println!("{n:4} passes (e.g. {set}): {}", s.join(" "));
    }
}
