//! Every picture and material with `camo` in its name in the zones given,
//! one line each (name, size, streamed or inline). For cataloguing a game's
//! camos so they can join the camo layer every gun wears.
//!
//! `cargo run -p fastfile_h1 --example h1_camo_names -- <zone.ff>... > camos.txt`

use std::collections::BTreeSet;
use std::path::Path;

fn main() {
    let paths: Vec<String> = std::env::args().skip(1).collect();
    if paths.is_empty() {
        eprintln!("usage: h1_camo_names <zone.ff>...");
        std::process::exit(2);
    }
    let mut seen = BTreeSet::new();
    for path in &paths {
        let zone = match fastfile_h1::Zone::open(Path::new(path)) {
            Ok(z) => z,
            Err(e) => {
                eprintln!("{path}: {e}");
                continue;
            }
        };
        let (cap, _) = fastfile_h1::walk::walk_zone(&zone);
        let zone_name = Path::new(path).file_stem().map_or_else(String::new, |s| s.to_string_lossy().into_owned());
        for rec in &cap.images {
            if rec.name.to_ascii_lowercase().contains("camo") && seen.insert(rec.name.clone()) {
                println!(
                    "picture\t{}\t{}x{}\t{}\t{}",
                    rec.name,
                    rec.width,
                    rec.height,
                    if rec.streamed { "streamed" } else { "inline" },
                    zone_name
                );
            }
        }
        for m in &cap.materials {
            if m.name.to_ascii_lowercase().contains("camo") && seen.insert(format!("m:{}", m.name)) {
                println!("material\t{}\t\t\t{}", m.name, zone_name);
            }
        }
    }
}
