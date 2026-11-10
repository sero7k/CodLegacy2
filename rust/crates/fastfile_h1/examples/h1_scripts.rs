//! Lists a zone's compiled scripts and, with `--dump <dir>`, writes each one's
//! inflated buffer and bytecode there:
//! `cargo run -p fastfile_h1 --example h1_scripts -- <zone.ff> [--dump <dir>]`.

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let path = std::path::PathBuf::from(args.first().expect("zone path"));
    let dump = args.iter().position(|a| a == "--dump").and_then(|i| args.get(i + 1)).map(std::path::PathBuf::from);
    let zone = fastfile_h1::Zone::open(&path).expect("zone");
    let (_map, cap) = fastfile_h1::map::IwMap::load(&zone);
    for s in &cap.script_files {
        println!("{} compressed {} len {} bytecode {}", s.name, s.compressed_len, s.len, s.bytecode_len);
        let buffer = s.buffer.map(|b| b.get(&zone.image).to_vec()).unwrap_or_default();
        let inflated = miniz_oxide::inflate::decompress_to_vec_zlib(&buffer).ok();
        println!("  buffer inflates: {:?}", inflated.as_ref().map(|v| v.len()));
        if let Some(dir) = &dump {
            std::fs::create_dir_all(dir).expect("dir");
            let stem = s.name.replace(['/', '\\'], "_");
            std::fs::write(dir.join(format!("{stem}.buffer")), inflated.unwrap_or(buffer)).expect("write");
            std::fs::write(dir.join(format!("{stem}.bytecode")), s.bytecode.map(|b| b.get(&zone.image).to_vec()).unwrap_or_default()).expect("write");
        }
    }
}
