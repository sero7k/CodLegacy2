//! `t7_sky_wgsl <map.ff>`: the sky material's (or `T7_SET=<substring>`: those technique sets') forward technique translated
//! (vertex and pixel WGSL to stdout), to read how the sky is placed.
fn main() {
    let zone = std::path::PathBuf::from(std::env::args().nth(1).expect("map zone"));
    let scene = render_t7::scene::Scene::load(&zone).expect("scene");
    println!("// sky_rotation (deg, z) {:?}", scene.map.light_refs().map(|l| l.sky_rotation));
    if let Some(pr) = scene.map.light_refs().and_then(|l| l.probes.as_ref()) {
        let w: Vec<f32> = pr.configs[..328].chunks_exact(4).map(|c| f32::from_le_bytes(c.try_into().unwrap())).collect();
        println!("// probe0 words: {:?}", &w[..40]);
        let w1: Vec<f32> = pr.configs[328..656].chunks_exact(4).map(|c| f32::from_le_bytes(c.try_into().unwrap())).collect();
        println!("// probe1 words: {:?}", &w1[..12]);
    }
    let want = std::env::var("T7_SET").unwrap_or_else(|_| "sky".into());
    let mut seen = std::collections::HashSet::new();
    for m in scene.materials.iter().filter(|m| m.techset.contains(&want) && seen.insert(m.techset.clone())) {
        println!("// material {} techset {} key {:016x}", m.name, m.techset, m.sort_key);
        for (i, n, d) in &m.cbuffers {
            let f: Vec<f32> = d.chunks_exact(4).map(|c| f32::from_le_bytes(c.try_into().unwrap())).collect();
            println!("// cbuffer idx {i} {n}: {f:?}");
        }
        println!("// textures {:?}", m.textures);
        // (`T7_GBUFFER=1`: the G-buffer technique when there is no forward one.)
        let gb = std::env::var("T7_GBUFFER").is_ok();
        let Some(t) = m.forward.as_ref().or(m.technique.as_ref().filter(|_| gb)) else { continue };
        println!("// slot {} buffer_set {}", t.slot, t.buffer_set);
        for st in 0..2 { for b in render_t7::rdef::bindings(if st == 0 { &t.vs } else { &t.ps }) { println!("// rdef stage {st}: {b:?}"); } }
        for a in &t.args {
            println!("// arg ty {:#x} loc {} size {} buf {} u {:x?} lit {:?}", a.ty, a.location, a.size, a.buffer, a.u, a.literal);
        }
        match render_t7::gbuffer::translate(t) {
            Ok((vs, ps)) => println!("// VS\n{}\n// PS\n{}", vs.wgsl, ps.wgsl),
            Err(e) => println!("// {e}"),
        }
    }
}
