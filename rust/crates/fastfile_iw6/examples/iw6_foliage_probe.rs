//! Lists the technique sets of the materials the placed models of a Ghosts map
//! zone use, with their placement counts, state bits and colour map formats:
//! `iw6_foliage_probe <mp_map.ff>` (`PROBE_GREP=leaf,tree` filters by model or
//! material name).
use std::collections::BTreeMap;

fn main() {
    let grep: Vec<String> = std::env::var("PROBE_GREP")
        .map(|v| v.split(',').map(str::to_owned).collect())
        .unwrap_or_default();
    let path = std::env::args().nth(1).expect("zone");
    let zone = fastfile_iw6::Zone::open(std::path::Path::new(&path)).expect("open");
    let (cap, _) = fastfile_iw6::walk::walk_zone(&zone);
    let map = fastfile_iw6::map::read_map(&cap, &zone.image, &zone.assets.script_strings);
    let mut by: BTreeMap<(String, String), (usize, usize, Vec<String>, [u64; 4])> = BTreeMap::new();
    let mut counted: BTreeMap<u32, usize> = BTreeMap::new();
    if let Some(w) = &map.world {
        for s in &w.static_models {
            if let Some(m) = s.model {
                *counted.entry(m.index).or_default() += 1;
            }
        }
    }
    for (&mi, &n) in &counted {
        let Some(m) = cap.xmodels.get(mi as usize) else { continue };
        let Some(lod) = m.lods.first() else { continue };
        let Some(sid) = lod.surfs else { continue };
        let Some(surfs) = cap.model_surfs.get(sid.index as usize) else { continue };
        for (i, s) in surfs.surfs.iter().enumerate() {
            let Some(mid) = m.materials.get(usize::from(lod.surf_index) + i).copied().flatten() else { continue };
            let Some(mr) = cap.materials.get(mid.index as usize) else { continue };
            let ts = mr.techset.map(|t| cap.name_of(t).to_owned()).unwrap_or_default();
            let hay = format!("{} {}", m.name, mr.name);
            if std::env::var_os("PROBE_MODELS").is_some() {
                println!("MODEL {} x{n} | {} | {ts}", m.name, mr.name);
                continue;
            }
            if !grep.is_empty() && !grep.iter().any(|g| hay.contains(g.as_str())) {
                continue;
            }
            let e = by.entry((ts, mr.name.clone())).or_default();
            e.0 += n;
            e.1 += s.vert_count as usize;
            if let Some(sp) = s.verts {
                for v in sp.get(&zone.image).chunks_exact(32) {
                    for k in 0..4 {
                        e.3[k] += u64::from(v[16 + k]);
                    }
                }
            }
            if e.2.len() < 2 && !e.2.contains(&m.name) {
                e.2.push(m.name.clone());
            }
        }
    }
    for ((ts, mat), (n, v, ms, col)) in &by {
        let avg = col.map(|c| c / (*v as u64).max(1));
        let mr = cap.materials.iter().find(|m| &m.name == mat).unwrap();
        let tex: Vec<String> = mr
            .textures
            .iter()
            .filter_map(|t| {
                t.image.map(|i| {
                    let im = cap.images.get(i.index as usize);
                    format!(
                        "{:08x}:{}:fmt{}{}{}x{}",
                        t.name_hash,
                        cap.name_of(i),
                        im.map_or(0, |x| x.format),
                        if im.is_some_and(|x| x.streamed) { " streamed " } else { " inline " },
                        im.map_or(0, |x| x.width),
                        im.map_or(0, |x| x.height)
                    )
                })
            })
            .collect();
        let sb: Vec<String> = mr
            .state_bits
            .iter()
            .map(|b| {
                let w = |o: usize| u32::from_le_bytes(b[o..o + 4].try_into().unwrap());
                format!("{:08x}/{:08x}/{:08x} z{} d{} b{} r{}", w(0), w(4), w(8), b[12], b[13], b[24], b[25])
            })
            .collect();
        println!(
            "x{n:<5} v{v:<7} col{avg:?} {ts} | {mat} | {ms:?} | sort{} flags{} | tex {tex:?} | sb {sb:?}",
            mr.sort_key, mr.state_flags
        );
    }
}
