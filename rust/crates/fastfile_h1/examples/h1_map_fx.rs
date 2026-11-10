//! A Modern Warfare Remastered map's createfx: the placements its scripts hold
//! and whether the map zone holds the effects they name:
//! `cargo run -p fastfile_h1 --release --example h1_map_fx -- <map.ff> [<zone dir>]`
//! (the zone dir has the retail `patch_<map>.ff`; default: beside the map).

use std::collections::BTreeMap;

fn main() {
    let mut args = std::env::args_os().skip(1);
    let path: std::path::PathBuf = args.next().expect("zone path").into();
    let dir: std::path::PathBuf = args.next().map(Into::into).unwrap_or_else(|| {
        path.parent()
            .map(std::path::Path::to_path_buf)
            .unwrap_or_default()
    });
    let map = path
        .file_stem()
        .expect("name")
        .to_string_lossy()
        .to_ascii_lowercase();
    let zone = fastfile_h1::Zone::open(&path).expect("zone");
    let (_m, cap) = fastfile_h1::map::IwMap::load(&zone);
    let names = vec![
        format!("maps/createfx/{map}_fx.gsc"),
        format!("maps/mp/{map}_fx.gsc"),
        format!("maps/createart/{map}_fog_hdr.gsc"),
        format!("vision/{map}.vision"),
    ];
    let raws = fastfile_h1::map::PatchRaws::read(&dir, &map, &names, &cap, &zone.image);
    println!("patch zones read: {:?}", raws.zones);
    for n in &names {
        println!(
            "{n}: {}",
            raws.get(&cap, &zone.image, n)
                .map_or("absent".to_owned(), |b| format!("{} bytes", b.len()))
        );
    }
    let text = |n: &str| {
        raws.get(&cap, &zone.image, n)
            .map(|b| String::from_utf8_lossy(&b).into_owned())
            .unwrap_or_default()
    };
    let fx = fastfile_h1::createfx::parse(&text(&names[0]), &text(&names[1]));
    let zone_fx: Vec<&str> = cap
        .fx
        .iter()
        .filter(|f| !f.name.starts_with(','))
        .map(|f| f.name.as_str())
        .collect();
    println!(
        "{} placements ({} ambient), {} effect ids, {} effects in the zone",
        fx.placed.len(),
        fx.ambient().count(),
        fx.paths.len(),
        zone_fx.len()
    );
    let mut by: BTreeMap<&str, (usize, bool, Option<&str>)> = BTreeMap::new();
    for p in fx.ambient() {
        let path = fx.paths.get(&p.fxid).map(String::as_str);
        let found = path.is_some_and(|pa| {
            zone_fx.iter().any(|z| {
                z.eq_ignore_ascii_case(pa)
                    || z.eq_ignore_ascii_case(
                        pa.trim_start_matches("fx/").trim_start_matches("vfx/"),
                    )
            })
        });
        let e = by.entry(p.fxid.as_str()).or_insert((0, found, path));
        e.0 += 1;
    }
    for (id, (n, found, path)) in by {
        println!("{n:4} x {id:40} -> {path:?} in zone: {found}");
    }
    if std::env::var_os("LIST_ZONE_FX").is_some() {
        for z in zone_fx {
            println!("zone fx: {z}");
        }
    }
}
