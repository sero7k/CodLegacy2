//! `t7_image_sources <names.txt> <map.ff> <zone.ff>...`: for each image
//! name, where its pixels are: in a zone (inline, any of the zones given),
//! or only in `.xpak` packages (the packages' index bits, which entry key).
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let names: Vec<String> = std::fs::read_to_string(&a[1]).expect("names").lines().map(str::to_owned).collect();
    let mut zones = Vec::new();
    for p in &a[2..] {
        let Ok(z) = fastfile_t7::Zone::open(std::path::Path::new(p)) else { continue };
        let w = fastfile_t7::walk::walk(&z);
        let short = std::path::Path::new(p).file_name().unwrap().to_string_lossy().into_owned();
        zones.push((short, w.capture));
    }
    let mut by_pak: std::collections::BTreeMap<String, Vec<String>> = Default::default();
    for n in &names {
        let mut inline = None;
        let mut xpak = None;
        for (zn, c) in &zones {
            for im in c.images.iter().filter(|i| i.name.trim_start_matches(',') == n) {
                if im.xpak.is_empty() && im.pixels.is_some() {
                    inline.get_or_insert(zn.clone());
                } else if let Some(e) = im.xpak.first() {
                    xpak.get_or_insert((zn.clone(), e.bits, im.xpak.len(), im.fallback.is_some()));
                }
            }
        }
        let key = match (inline, xpak) {
            (Some(z), _) => format!("in zone {z}"),
            (None, Some((z, idx, parts, fb))) => format!("xpak bits {idx:#x} (named by {z}; {parts} parts{})", if fb { ", lowest mips in the zone" } else { "" }),
            _ => "no record in any zone given".into(),
        };
        by_pak.entry(key).or_default().push(n.clone());
    }
    for (k, v) in &by_pak {
        println!("{k}: {} images", v.len());
        for n in v {
            println!("    {n}");
        }
    }
}
