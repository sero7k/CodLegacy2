//! Placed models whose materials hold a word: each LOD-0 surface's counts,
//! rigid lists and the first vertices' positions:
//! `cargo run -p fastfile_h1 --release --example h1_model_dump -- <zone.ff> <material word>`.

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let zone = fastfile_h1::Zone::open(std::path::Path::new(&args[0])).expect("zone");
    let word = &args[1];
    let (map, cap) = fastfile_h1::map::IwMap::load(&zone);
    let world = map.world.as_ref();
    for xm in &cap.xmodels {
        let mats: Vec<String> = xm.materials.iter().flatten().map(|m| cap.name_of(*m).to_string()).collect();
        if !mats.iter().any(|m| m.contains(word.as_str())) {
            continue;
        }
        let placed = world.map_or(0, |w| {
            w.static_models
                .iter()
                .filter(|s| s.model.is_some_and(|m| cap.name_of(m) == xm.name))
                .count()
        });
        println!("{} bones {} lods {} placed {placed} mats {mats:?}", xm.name, xm.num_bones, xm.lods.len());
        let Some(lod) = xm.lods.first() else { continue };
        let Some(ms) = lod.surfs.and_then(|s| cap.model_surfs.get(s.index as usize)) else {
            println!("  lod0 surfs missing");
            continue;
        };
        for (j, s) in ms.surfs.iter().enumerate() {
            println!(
                "  surf {j}: flags {:#x} verts {} tris {} blend {:?} rigid {:?} verts {:?} tris {:?}",
                s.flags, s.vert_count, s.tri_count, s.blend_vert_counts, s.rigid, s.verts.map(|v| v.len), s.tris.map(|t| t.len)
            );
            if let Some(v) = s.verts {
                let b = v.get(&zone.image);
                for k in 0..3.min(s.vert_count as usize) {
                    let p: Vec<f32> = (0..4).map(|i| f32::from_le_bytes(b[32 * k + 4 * i..32 * k + 4 * i + 4].try_into().unwrap())).collect();
                    println!("    v{k} {p:?}");
                }
            }
            if let Some(t) = s.tris {
                let b = t.get(&zone.image);
                let max = b.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).max();
                println!("    max index {max:?}");
            }
        }
    }
}
