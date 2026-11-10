//! An AI type's characters and their models (`t7_characters <zone.ff>
//! [substring]`): each `aitype`, `character` and `xmodelalias` with the
//! assets its fields name, by field offset (and each `zbarrier` def's
//! boards). `--cast` instead prints the
//! zombies' test data (`sim::t6::t7_test`'s `T7_CAST`): per actor AI type
//! `C\taitype\tbody\thead,head\tattached,...` per character, and
//! `A\tname\tframes\tframerate\tlooping` per zombie clip
//! (`ai_zombie_base_*`, `bo3_ai_zombie_*`).
use fastfile_t7::asset_type::AssetType as T;

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let z = fastfile_t7::Zone::open(std::path::Path::new(&a[1])).expect("zone");
    let cast = a.iter().any(|x| x == "--cast");
    let want = a.get(2).filter(|s| !s.starts_with("--")).map(|s| s.to_ascii_lowercase()).unwrap_or_default();
    let w = fastfile_t7::walk::walk_with(&z, fastfile_t7::walk::Options { resync: true, keep_anims: cast || a.iter().any(|x| x == "--anims"), ..Default::default() });
    let cap = &w.capture;
    if cast {
        let bare = |n: &str| n.trim_start_matches(',').to_owned();
        let fields = |ty: T, name: &str| cap.impact_fields.get(&(ty, name.to_owned())).or_else(|| cap.impact_fields.get(&(ty, format!(",{name}"))));
        let at = |f: &Vec<(usize, String)>, o: usize| f.iter().find(|x| x.0 == o).map(|x| bare(&x.1));
        let mut types: Vec<&String> = cap.impact_fields.keys().filter(|k| k.0 == T::AiType).map(|k| &k.1).collect();
        types.sort();
        for t in types {
            let ai = &cap.impact_fields[&(T::AiType, t.clone())];
            for o in (0x4b8..=0x4f0).step_by(8) {
                let Some(c) = at(ai, o) else { continue };
                let Some(cf) = fields(T::Character, &c) else { continue };
                let Some(body) = at(cf, 0x60) else { continue };
                let heads: Vec<String> = match at(cf, 0x70) {
                    Some(h) => vec![h],
                    None => at(cf, 0x78).and_then(|a| fields(T::XModelAlias, &a)).map(|f| f.iter().map(|x| bare(&x.1)).collect()).unwrap_or_default(),
                };
                let attached: Vec<String> = [0x88, 0x98].iter().filter_map(|&o| at(cf, o)).collect();
                println!("C\t{}\t{body}\t{}\t{}", bare(t), heads.join(","), attached.join(","));
            }
        }
        // `--bind`: per character, whether its models are in the zone with
        // a skeleton, and how many of a walk clip's bones its body has (the
        // client poses a zombie by bone name; none bound draws it in its
        // bind pose).
        if a.iter().any(|x| x == "--bind") {
            let ss = |i: u32| z.assets.script_strings.get(i as usize).cloned().flatten().unwrap_or_default();
            let model = |n: &str| cap.models.iter().find(|m| bare(&m.name) == n && !m.name.starts_with(','));
            let walk = cap.anims.iter().find(|r| {
                let n = bare(&r.name);
                !r.name.starts_with(',') && (n.starts_with("ai_zombie_base_walk_ad_v") || n.starts_with("bo3_ai_zombie_walk_ad_v"))
            });
            let clip_bones: Vec<String> = walk.map(|r| r.names.iter().map(|&s| ss(s)).collect()).unwrap_or_default();
            println!("walk clip {:?}: {} bones", walk.map(|r| bare(&r.name)), clip_bones.len());
            let mut types: Vec<&String> = cap.impact_fields.keys().filter(|k| k.0 == T::AiType).map(|k| &k.1).collect();
            types.sort();
            for t in types {
                let ai = &cap.impact_fields[&(T::AiType, t.clone())];
                for o in (0x4b8..=0x4f0).step_by(8) {
                    let Some(c) = at(ai, o) else { continue };
                    let Some(cf) = fields(T::Character, &c) else { continue };
                    let Some(body) = at(cf, 0x60) else { continue };
                    let mut names = vec![body.clone()];
                    names.extend(at(cf, 0x70));
                    names.extend(at(cf, 0x78).and_then(|a| fields(T::XModelAlias, &a)).map(|f| f.iter().map(|x| bare(&x.1)).collect::<Vec<_>>()).unwrap_or_default());
                    names.extend([0x88, 0x98].iter().filter_map(|&o| at(cf, o)));
                    let state: Vec<String> = names
                        .iter()
                        .map(|n| match model(n) {
                            Some(m) => format!("{n}: {} bones, {} named, {} LODs", m.bones, m.skeleton.bone_names.len(), m.lods.len()),
                            None => format!("{n}: NOT IN THE ZONE"),
                        })
                        .collect();
                    let bound = model(&body).map_or(0, |m| {
                        let bb: std::collections::HashSet<String> = m.skeleton.bone_names.iter().map(|&s| ss(s).to_ascii_lowercase()).collect();
                        clip_bones.iter().filter(|b| bb.contains(&b.to_ascii_lowercase())).count()
                    });
                    println!("B\t{}\t{c}\t{bound} of {} walk bones on the body\t{}", bare(t), clip_bones.len(), state.join("; "));
                }
            }
            return;
        }
        let all = a.iter().any(|x| x == "--all-anims");
        for r in &cap.anims {
            let n = bare(&r.name);
            if all || n.starts_with("ai_zombie_base_") || n.starts_with("bo3_ai_zombie_") {
                let fr = r.header.get(0x50..0x54).map_or(30.0, |b| f32::from_le_bytes([b[0], b[1], b[2], b[3]]));
                let looping = r.header.get(0x24).is_some_and(|&b| b != 0);
                println!("A\t{n}\t{}\t{fr}\t{}", r.numframes, u8::from(looping));
                // (The root motion's translation keys: `D\tframe\tx\ty\tz`.)
                for (f, p) in &r.delta_trans {
                    println!("D\t{f}\t{}\t{}\t{}", p[0], p[1], p[2]);
                }
                // (Its notetracks: `N\tname\ttime`, the parameter's text
                // when it has one, else the kind's.)
                let ss = |i: u32| z.assets.script_strings.get(i as usize).cloned().flatten().unwrap_or_default();
                for &(kind, time, param, _) in &r.notifies {
                    let (k, p) = (ss(kind), ss(param));
                    let name = if p.is_empty() { k } else { p };
                    if !name.is_empty() {
                        println!("N\t{name}\t{time}");
                    }
                }
            }
        }
        return;
    }
    let mut rows: Vec<_> = cap.impact_fields.iter().collect();
    rows.sort_by(|x, y| (x.0.0 as u32, &x.0.1).cmp(&(y.0.0 as u32, &y.0.1)));
    for ((ty, name), fields) in rows {
        if !matches!(ty, T::AiType | T::Character | T::XModelAlias | T::ZBarrier) || !name.to_ascii_lowercase().contains(&want) {
            continue;
        }
        let f: Vec<String> = fields.iter().map(|(o, n)| format!("{o:#x}={n}")).collect();
        println!("{ty:?} {name}: {}", f.join(" "));
    }
}
