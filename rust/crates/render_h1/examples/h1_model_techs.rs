//! Which techniques a map's static models draw with, per slot, and what
//! their passes take. `h1_model_techs <map.ff> <zone.ff>...`
use render_h1::pool::{Pool, key};
use std::collections::BTreeMap;
fn main() {
    let paths: Vec<String> = std::env::args().skip(1).collect();
    let zones: Vec<_> = paths
        .iter()
        .map(|p| {
            let z = fastfile_h1::Zone::open(std::path::Path::new(p)).expect("zone");
            let (c, _) = fastfile_h1::walk::walk_zone(&z);
            (z, c)
        })
        .collect();
    let mut pool = Pool::default();
    for (z, c) in zones.iter().skip(1).chain(zones.iter().take(1)) {
        pool.add(c, &z.image);
    }
    let (z, cap) = &zones[0];
    let map = fastfile_h1::map::read_map(cap, &z.image, &z.assets.script_strings);
    let w = map.world.expect("world");
    let mut techsets: BTreeMap<String, usize> = BTreeMap::new();
    for sm in &w.static_models {
        let Some(m) = sm.model else { continue };
        let xm = &cap.xmodels[m.index as usize];
        for mat in xm.materials.iter().flatten() {
            let mr = &cap.materials[mat.index as usize];
            let ts = mr
                .techset
                .map(|t| cap.name_of(t).to_string())
                .unwrap_or_else(|| format!("(ref {})", mr.name));
            *techsets.entry(ts).or_default() += 1;
        }
    }
    println!("{} static models; flags histogram:", w.static_models.len());
    let mut fl: BTreeMap<u16, usize> = BTreeMap::new();
    for sm in &w.static_models {
        *fl.entry(sm.flags).or_default() += 1;
    }
    println!("{fl:?}");
    for (ts, n) in techsets.iter().take(12) {
        println!("== techset {ts} ({n} model materials)");
        if let Some(set) = pool.techsets.get(&key(ts)) {
            for (slot, name) in set
                .iter()
                .filter(|(s, _)| [13usize, 14, 15, 73, 74, 75, 193, 194, 195].contains(s))
            {
                let t = pool.technique(name);
                let p = t.and_then(|t| t.pass_info.first());
                println!(
                    "  slot {slot:3} {name} vs {:?} ps {:?} decl {:?}",
                    p.and_then(|p| p.vertex_shader.clone()),
                    p.and_then(|p| p.pixel_shader.clone()),
                    p.and_then(|p| p.vertex_decl.clone())
                );
                if let Some(p) = p {
                    let names: Vec<String> = p
                        .args
                        .iter()
                        .map(|a| match a.kind {
                            0 => fastfile_h1::code_source::code_const_name(a.code_const().0)
                                .to_string(),
                            1 | 2 => format!(
                                "tex:{}",
                                fastfile_h1::code_source::code_texture_name(a.word())
                            ),
                            k => format!("k{k}@{}", a.dest),
                        })
                        .collect();
                    println!("      args {names:?} csf {:#x}", p.custom_sampler_flags);
                }
            }
        } else {
            println!("  (set not loaded)");
        }
    }
}
