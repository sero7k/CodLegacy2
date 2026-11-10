//! `t7_gun_mats <zone.ff> <model substring>`: each matching model's materials with
//! their texture slots (hash, picture, where it is) and tint/spec constants.
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let zone = fastfile_t7::Zone::open(std::path::Path::new(&a[1])).expect("zone");
    let w = fastfile_t7::walk::walk(&zone);
    let cap = &w.capture;
    println!("stop {:?}; {} models, {} materials, {} images", w.stop, cap.models.len(), cap.materials.len(), cap.images.len());
    for m in cap.models.iter().filter(|m| m.name.contains(&a[2])) {
        println!("{}", m.name);
        let mut seen = std::collections::BTreeSet::new();
        for l in m.lods.iter().take(1) {
            for mat in &l.materials {
                if !seen.insert(mat.clone()) { continue; }
                let Some(r) = cap.materials.iter().find(|r| &r.name == mat) else { println!("  {mat}: NOT CAPTURED"); continue };
                println!("  {} [{:?}] consts {:?}", r.name, r.techset, r.colour_consts);
                for (h, n) in &r.textures {
                    let img = cap.images.iter().find(|i| &i.name == n);
                    let place = img.map_or("no record".to_owned(), |i| if i.pixels.is_some() { "zone".into() } else if !i.xpak.is_empty() { format!("xpak {}x{} fmt {}", i.width, i.height, i.format) } else { "record only".into() });
                    println!("      {h:#010x} {n} ({place})");
                }
            }
        }
    }
}
