//! Names a technique set's slots and says whether each technique (and its
//! shaders) is in the zones given: `iw6_techset <set>[,<set>] <zone.ff>...`
//! (`?word` for the set: every set whose name holds the word).
use fastfile_iw6::AssetType;
use render_iw6::pool::{Pool, key};

fn main() {
    let mut a = std::env::args().skip(1);
    let sets = a.next().expect("set names");
    let mut pool = Pool::default();
    for p in a {
        let z = fastfile_iw6::Zone::open(std::path::Path::new(&p)).expect("open");
        let (cap, _) = fastfile_iw6::walk::walk_zone(&z);
        pool.add(&cap, &z.image);
    }
    // `?word`: the sets whose name holds the word.
    let sets: String = match sets.strip_prefix('?') {
        Some(w) => {
            let mut names: Vec<&String> = pool.techsets.keys().filter(|k| k.contains(w)).collect();
            names.sort();
            names.into_iter().cloned().collect::<Vec<_>>().join(",")
        }
        None => sets,
    };
    for set in sets.split(',') {
        println!("set {set}:");
        let Some(slots) = pool.techsets.get(&key(set)) else {
            println!("  not in the zones");
            continue;
        };
        for (i, n) in slots {
            match pool.technique(n) {
                None => println!("  slot {i}: {n} -> TECHNIQUE MISSING"),
                Some(t) => {
                    let tess = t.passes.iter().any(|p| p.hull_shader.is_some());
                    let vs = t.passes.first().and_then(|p| p.vertex_shader.clone());
                    let ps = t.passes.first().and_then(|p| p.pixel_shader.clone());
                    let have = |ty, n: &Option<String>| n.as_ref().is_some_and(|n| pool.program(ty, n).is_some());
                    println!(
                        "  slot {i}: {n} passes {} tess {tess} vs {vs:?} ({}) ps {ps:?} ({})",
                        t.passes.len(),
                        have(AssetType::VertexShader, &vs),
                        have(AssetType::PixelShader, &ps)
                    );
                }
            }
        }
    }
}
