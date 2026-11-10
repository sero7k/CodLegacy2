//! Prints raw files of a zone whose names contain a pattern:
//! `cargo run -p fastfile_h1 --release --example h1_rawfile -- <zone.ff> [<name>...]`
//! (no names: lists them).

fn main() {
    let mut args = std::env::args_os().skip(1);
    let path: std::path::PathBuf = args.next().expect("zone path").into();
    let zone = fastfile_h1::Zone::open(&path).expect("zone");
    let (_map, cap) = fastfile_h1::map::IwMap::load(&zone);
    let names: Vec<_> = args.collect();
    // (No names: every raw file of the zone, by name.)
    if names.is_empty() {
        for r in &cap.raw_files {
            println!("{} ({} bytes{})", r.name, r.data.map_or(0, |d| d.get(&zone.image).len()), if r.compressed_len == 0 { "" } else { ", zlib" });
        }
    }
    for name in names {
        let name = name.to_string_lossy().into_owned();
        match fastfile_h1::map::raw_file(&cap, &zone.image, &name) {
            Some(b) => println!("==== {name}\n{}", String::from_utf8_lossy(&b)),
            None => println!("==== {name}: not in the zone"),
        }
    }
}
