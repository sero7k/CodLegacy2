//! Lists a Ghosts zone's animations and the models that carry bones with a word:
//! `iw6_anim_probe <zone.ff> [word]`.
fn main() {
    let mut a = std::env::args().skip(1);
    let path = a.next().expect("zone");
    let word = a.next().unwrap_or_default().to_ascii_lowercase();
    let zone = fastfile_iw6::Zone::open(std::path::Path::new(&path)).expect("open");
    let (cap, stop) = fastfile_iw6::walk::walk_zone(&zone);
    println!("walked {}/{} {:?}", cap.walked, zone.assets.assets.len(), stop.map(|s| s.error));
    let s = |i: u32| zone.assets.script_strings.get(i as usize).cloned().flatten().unwrap_or_default();
    for x in cap.xanims.iter().filter(|x| x.name.to_ascii_lowercase().contains(&word)) {
        let names: Vec<String> = x.names.iter().map(|&i| s(i)).collect();
        let notes: Vec<String> = x.notify.iter().map(|&(i, t)| format!("{}@{t:.3}", s(i))).collect();
        println!(
            "xanim {} frames {} rate {} freq {} flags {:#x} counts {:?} bones {names:?} notes {notes:?} delta {}",
            x.name, x.numframes, x.framerate, x.frequency, x.flags, x.bone_count, x.delta.is_some()
        );
    }
    for m in cap.xmodels.iter().filter(|m| m.name.to_ascii_lowercase().contains(&word)) {
        let names: Vec<String> = m.bone_names.iter().map(|&i| s(i)).collect();
        println!("xmodel {} bones {} roots {} {names:?} lods {}", m.name, m.num_bones, m.num_root_bones, m.lods.len());
        if m.num_bones > 1 || std::env::var("SURFS").is_ok() {
            for (li, l) in m.lods.iter().enumerate().take(1) {
                let Some(ms) = l.surfs.and_then(|s| cap.model_surfs.get(s.index as usize)) else { continue };
                for (si, sf) in ms.surfs.iter().enumerate() {
                    println!("   lod{li} surf{si}: verts {} tris {} blend {:?} rigid {:?}", sf.vert_count, sf.tri_count, sf.blend_vert_counts, sf.rigid);
                    if let Some(v) = sf.verts {
                        let b = v.get(&zone.image);
                        let (mut lo, mut hi) = ([f32::MAX; 3], [f32::MIN; 3]);
                        for c in b.chunks_exact(32) {
                            for k in 0..3 {
                                let f = f32::from_le_bytes([c[4 * k], c[4 * k + 1], c[4 * k + 2], c[4 * k + 3]]);
                                lo[k] = lo[k].min(f);
                                hi[k] = hi[k].max(f);
                            }
                        }
                        println!("      vertex bounds {lo:.1?} .. {hi:.1?}");
                    }
                }
            }
            let mats: Vec<String> = m.materials.iter().map(|x| x.map(|id| cap.name_of(id).to_owned()).unwrap_or_default()).collect();
            println!("   materials {mats:?}");
            println!("   tag_angles {:?} tag_positions {:?}", m.tag_angles, m.tag_positions);
            println!("   parents {:?} base0 {:?}", m.parent_list, m.base_mat.iter().map(|b| format!("{:.2?}", b)).collect::<Vec<_>>());
        }
    }
    // Script models of the map that name an animation or are animated props.
    if std::env::var("ENTS").is_ok() {
        for me in &cap.map_ents {
            for b in fastfile_iw6::maplogic::blocks(&me.entity_string) {
                let get = |k: &str| b.iter().find(|(kk, _)| kk == k).map(|(_, v)| v.as_str());
                let class = get("classname").unwrap_or("");
                let model = get("model").unwrap_or("");
                let has_anim = b.iter().any(|(k, _)| k == "iw6_3571");
                let w = word.as_str();
                if (class == "script_model" || class == "script_brushmodel") && (has_anim || model.contains(w) || get("targetname").is_some_and(|t| t.contains(w))) {
                    println!("ent {b:?}");
                }
            }
        }
    }
}
