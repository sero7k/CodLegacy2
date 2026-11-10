//! A map's brush sub-models (`*<n>`): bounds, brush count and the brushes'
//! contents, with the entity that names each and its origin:
//! `t7_cmodels <map.ff> [--planes]` (`--planes`: also a `B *<n> <brush>
//! <contents> nx,ny,nz,d ...` line per brush, for `sim`'s headless T7 test,
//! and a `T ?<n> mx,my,mz,hx,hy,hz ...` line per map trigger model: its
//! hulls' middles and half sizes, about its entity).
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let map = fastfile_t7::map::T7Map::load(&fastfile_t7::map::MapSources::beside(std::path::Path::new(&a[1]))).expect("map");
    let clip = map.clip.as_ref().expect("clip");
    let mut names: std::collections::HashMap<String, (String, String, String)> = Default::default();
    for block in map.entities.split('{') {
        let v = |k: &str| {
            let at = block.find(&format!("\"{k}\" \""))? + k.len() + 4;
            let r = &block[at..];
            Some(r[..r.find('"')?].to_owned())
        };
        if let Some(m) = v("model").filter(|m| m.starts_with('*')) {
            names.insert(m, (v("classname").unwrap_or_default(), v("targetname").unwrap_or_default(), v("origin").unwrap_or_default()));
        }
    }
    for (i, m) in clip.models.iter().enumerate() {
        let mut contents: std::collections::BTreeMap<u32, usize> = Default::default();
        for &b in &m.leaf.brushes {
            if let Some(br) = clip.brushes.get(b as usize) {
                *contents.entry(br.contents).or_default() += 1;
            }
        }
        let e = names.get(&format!("*{i}")).cloned().unwrap_or_default();
        let first: Vec<[f32; 4]> = m.leaf.brushes.first().and_then(|&b| clip.brushes.get(b as usize)).map(|b| b.planes.iter().take(6).copied().collect()).unwrap_or_default();
        println!("*{i} first brush planes {first:?}");
        println!("*{i} {:?} {:?}..{:?} brushes {} contents {:x?} | {} {} origin {}", m.radius, m.mins, m.maxs, m.leaf.brushes.len(), contents, e.0, e.1, e.2);
        if a.iter().any(|x| x == "--planes") {
            for &b in &m.leaf.brushes {
                let Some(br) = clip.brushes.get(b as usize) else { continue };
                let planes: Vec<String> = br.planes.iter().map(|p| format!("{},{},{},{}", p[0], p[1], p[2], p[3])).collect();
                println!("B *{i} {b} {:x} {}", br.contents, planes.join(" "));
            }
        }
    }
    if a.iter().any(|x| x == "--planes") {
        for (i, hulls) in map.trigger_models.iter().enumerate() {
            let h: Vec<String> = hulls.iter().map(|(m, h)| format!("{},{},{},{},{},{}", m[0], m[1], m[2], h[0], h[1], h[2])).collect();
            println!("T ?{i} {}", h.join(" "));
        }
    }
}
