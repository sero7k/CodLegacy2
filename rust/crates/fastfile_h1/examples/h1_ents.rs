//! Prints a map zone's entity string, or the entities whose fields match a word:
//! `cargo run -p fastfile_h1 --release --example h1_ents -- <zone.ff> [word]`.

fn main() {
    let mut args = std::env::args_os().skip(1);
    let path: std::path::PathBuf = args.next().expect("zone path").into();
    let word = args.next().map(|w| w.to_string_lossy().to_lowercase());
    let zone = fastfile_h1::Zone::open(&path).expect("zone");
    let (map, _cap) = fastfile_h1::map::IwMap::load(&zone);
    let text = map.entity_string();
    let mut n = 0;
    for block in text.split('}') {
        let b = block.trim().trim_start_matches('{').trim();
        if b.is_empty() {
            continue;
        }
        n += 1;
        if word
            .as_ref()
            .is_none_or(|w| b.to_lowercase().contains(w.as_str()))
        {
            println!("{{\n{b}\n}}");
        }
    }
    eprintln!("{n} entities");
}
