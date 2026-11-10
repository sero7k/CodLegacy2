//! Every picture and material with `camo` in its name in the Ghosts zones
//! given, one line each. For cataloguing Ghosts' camos so they can join the
//! camo layer every gun wears.
//!
//! `cargo run -p fastfile_iw6 --example iw6_camo_names -- <zone.ff>... > camos.txt`
use std::collections::BTreeSet;

fn main() {
    let paths: Vec<String> = std::env::args().skip(1).collect();
    if paths.is_empty() {
        eprintln!("usage: iw6_camo_names <zone.ff>...");
        std::process::exit(2);
    }
    let mut seen = BTreeSet::new();
    for path in &paths {
        let zone = match fastfile_iw6::Zone::open(std::path::Path::new(path)) {
            Ok(z) => z,
            Err(e) => {
                eprintln!("{path}: {e}");
                continue;
            }
        };
        let (cap, _) = fastfile_iw6::walk::walk_zone(&zone);
        let zone_name = std::path::Path::new(path).file_stem().map_or_else(String::new, |s| s.to_string_lossy().into_owned());
        for rec in &cap.images {
            if rec.name.to_ascii_lowercase().contains("camo") && seen.insert(rec.name.clone()) {
                println!("picture\t{}\t{}", rec.name, zone_name);
            }
        }
        for m in &cap.materials {
            if m.name.to_ascii_lowercase().contains("camo") && seen.insert(format!("m:{}", m.name)) {
                println!("material\t{}\t{}", m.name, zone_name);
            }
        }
    }
}
