//! A map's materials whose names hold a substring: technique set, textures
//! (slot hash, picture, whether the picture is in the zone or a package)
//! and named constants: `t7_mat <map.ff> <substring>`.
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let map = fastfile_t7::map::T7Map::load(&fastfile_t7::map::MapSources::beside(std::path::Path::new(&a[1]))).expect("map");
    for m in map.materials.iter().filter(|m| m.name.contains(&a[2])) {
        println!("{} [{}]", m.name, m.technique_set);
        for (h, t) in &m.textures {
            let place = match map.image_info(t) {
                Some(i) if i.pixels.is_some() => "zone".to_owned(),
                Some(i) if !i.xpak.is_empty() => format!("xpak {}x{} fmt {}", i.width, i.height, i.format),
                Some(_) => "record only".to_owned(),
                None => "unknown".to_owned(),
            };
            println!("    {h:#010x} {t} ({place})");
        }
        let mut c: Vec<_> = m.constants.iter().filter(|(k, _)| !k.starts_with("debug")).collect();
        c.sort_by(|a, b| a.0.cmp(b.0));
        for (k, v) in c {
            println!("    {k} = {v:?}");
        }
    }
}
