//! Offline probe: for a Workshop pack zone, each gun's view model LODs
//! (vertices, triangles, inline or streamed, skinned vertices, bones,
//! materials with their colour maps).
//! `cargo run --release -p assets --example t7_pack_lods -- <zone.ff> [name filter]`
use fastfile_t7::walk::capture::FieldValue;
fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let zone = fastfile_t7::Zone::open(std::path::Path::new(&args[0])).expect("zone");
    let filter = args.get(1).cloned().unwrap_or_default();
    if std::env::var("IDLE").is_ok() {
        // (Each comma-separated name of the filter: a weapon's idle clip tracks on its view model's bones.)
        let guns: Vec<String> = filter.split(',').map(str::to_owned).collect();
        for line in assets::t7_pack_gun_idle_tracks(std::path::Path::new(&args[0]), &guns) {
            println!("{line}");
        }
        return;
    }
    let w = fastfile_t7::walk::walk_with(&zone, fastfile_t7::walk::Options { keep_anims: false, ..Default::default() });
    let cap = &w.capture;
    println!("walked {} assets, {} weapons, {} models, {} meshes; stop {:?}", w.walked.len(), cap.weapons.len(), cap.models.len(), cap.meshes.len(), w.stop);
    for wp in &cap.weapons {
        if !filter.is_empty() && !wp.name.contains(&filter) {
            continue;
        }
        {
            let tags: Vec<String> = wp.hide_tags.iter().map(|&t| zone.assets.script_strings.get(t as usize).cloned().flatten().unwrap_or_default()).collect();
            let at: Vec<String> = wp.fields.iter().filter(|(f, _)| f.starts_with("attachViewModelTag[")).map(|(f, v)| format!("{f}={}", match v { FieldValue::Str(s) => s.clone(), FieldValue::Asset { name, .. } => name.clone() })).collect();
            println!("{} hide_tags {:?} {:?}", wp.name, tags, at);
            if std::env::var("FIELDS").is_ok() {
                // (Every field the walk read, and the animation table.)
                println!("{} base {:?} def_at {:?} shared {:?}", wp.name, wp.base_name, wp.def_at, wp.def_shared_with);
                for (f, v) in &wp.fields {
                    println!("      field {f} = {}", match v { FieldValue::Str(s) => s.clone(), FieldValue::Asset { name, .. } => format!("asset {name}") });
                }
                for (slot, name, anim) in &wp.anims {
                    println!("      anim[{slot}] {name} = {anim} {}", if cap.anim(anim).is_some() { "" } else { "(not kept)" });
                }
            }
        }
        if std::env::var("HIDESIM").is_ok() {
            // (Per surface the bones of its rigid lists stand in for its influences.)
            if let Some(FieldValue::Asset { name, .. }) = wp.field("gunModel")
                && let Some(rec) = cap.model(name)
                && let Some(l) = rec.lods.iter().max_by_key(|l| cap.mesh(&l.mesh).map_or(0, |m| m.vert_count))
                && let Some(m) = cap.mesh(&l.mesh)
            {
                let names: Vec<String> = rec.skeleton.bone_names.iter().map(|&n| zone.assets.script_strings.get(n as usize).cloned().flatten().unwrap_or_default()).collect();
                let hide: Vec<usize> = (0..names.len()).filter(|&i| wp.hide_tags.iter().any(|&t| zone.assets.script_strings.get(t as usize).cloned().flatten().is_some_and(|x| x.eq_ignore_ascii_case(&names[i])))).collect();
                let mirror = |h: usize| (h / 32) * 32 + 31 - h % 32;
                let (mut bug, mut any, mut all) = (Vec::new(), Vec::new(), Vec::new());
                for (si, sf) in m.surfaces.iter().enumerate() {
                    let bones: Vec<usize> = sf.rigid.iter().map(|r| r[0] as usize).collect();
                    if bones.iter().any(|b| hide.iter().any(|&h| mirror(h) == *b)) { bug.push(si); }
                    if bones.iter().any(|b| hide.contains(b)) { any.push(si); }
                    if !bones.is_empty() && bones.iter().all(|b| hide.contains(b)) { all.push(si); }
                }
                println!("{} hidesim: surfaces {} hide bones {:?} | buggy-any hides {:?} | correct-any hides {:?} | correct-all hides {:?}", wp.name, m.surfaces.len(), hide, bug, any, all);
            }
        }
        let mut fields: Vec<String> = wp
            .fields
            .iter()
            .filter(|(f, v)| (*f == "gunModel" || *f == "worldModel" || f.starts_with("attachViewModel[") || f.starts_with("attachWorldModel[")) && matches!(v, FieldValue::Asset { .. }))
            .map(|(f, _)| f.clone())
            .collect();
        fields.dedup();
        for f in fields.iter().map(String::as_str) {
            let Some(FieldValue::Asset { name, .. }) = wp.field(f) else { continue };
            if name.is_empty() || name.starts_with(',') || name.starts_with("<ref") {
                continue;
            }
            let Some(rec) = cap.model(name) else { println!("{} {f} {name}: not in zone", wp.name); continue };
            println!("{} {f} {} bones {} skel names {} lods {}", wp.name, rec.name, rec.bones, rec.skeleton.bone_names.len(), rec.lods.len());
            if std::env::var("BONES").is_ok() {
                let nm = |i: usize| rec.skeleton.bone_names.get(i).and_then(|&n| zone.assets.script_strings.get(n as usize).cloned().flatten()).unwrap_or_default();
                for (i, m) in rec.skeleton.base_mat.iter().enumerate() {
                    println!("      bone {i:2} {:24} class {} t [{:.3},{:.3},{:.3}] q [{:.3},{:.3},{:.3},{:.3}]", nm(i), rec.skeleton.part_classification.get(i).copied().unwrap_or(0), m[4], m[5], m[6], m[0], m[1], m[2], m[3]);
                }
            }
            if std::env::var("BONES").is_ok() {
                for (i, (t, q)) in rec.skeleton.trans.iter().zip(&rec.skeleton.quats).enumerate().take(6) {
                    println!("      local[{i}] (bone {}) t [{:.3},{:.3},{:.3}] q [{:.3},{:.3},{:.3},{:.3}]", i + rec.skeleton.num_root_bones as usize, t[0], t[1], t[2], q[0], q[1], q[2], q[3]);
                }
                println!("      roots {} parent_list {:?}", rec.skeleton.num_root_bones, rec.skeleton.parent_list);
            }
            for (i, l) in rec.lods.iter().enumerate() {
                let Some(m) = cap.mesh(&l.mesh) else { println!("   lod{i} {} mesh not read", l.mesh); continue };
                let mats: Vec<String> = l.materials.iter().map(|mn| {
                    let c = cap.material(mn).map(|m| m.textures.iter().map(|(_, n)| n.clone()).collect::<Vec<_>>()).unwrap_or_default();
                    let have = c.iter().filter(|n| cap.image(n).is_some()).count();
                    format!("{mn}[{have}/{} imgs]", c.len())
                }).collect();
                if std::env::var("SURFS").is_ok() {
                    for (si, sf) in m.surfaces.iter().enumerate() {
                        println!("      surf {si} {:?} verts {} tris {} base_vert {} base_index {} flags {:#x} rigid {:?}", sf.name, sf.vert_count, sf.tri_count, sf.base_vert, sf.base_index, sf.flags, sf.rigid);
                    }
                }
                if std::env::var("MATS").is_ok() {
                    for mn in &l.materials {
                        if let Some(mat) = cap.material(mn) {
                            println!("      mat {mn} techset {:?} consts {:?}", mat.techset, mat.colour_consts);
                            for (h, n) in &mat.textures {
                                println!("         tex {h:#010x} {n} {}", if cap.image(n).is_some() { "defined" } else { "MISSING" });
                            }
                        }
                    }
                }
                let maxb = m.surfaces.iter().map(|s| s.rigid.len()).sum::<usize>();
                println!("   lod{i} {} verts {} tris {} skinverts {} surfs {} rigid {} {} flags {:#x} offs {:?} datasize {} mats {:?}",
                    l.mesh, m.vert_count, m.index_count / 3, m.skin_vert_count, m.surfaces.len(), maxb,
                    if m.data.is_some() { "INLINE" } else if m.xpak.is_some() { "STREAMED" } else { "NODATA" }, m.shared_flags, m.offsets, m.data_size, mats);
            }
        }
    }
}
