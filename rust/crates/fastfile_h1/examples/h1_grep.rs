//! Lists a zone's raw files and prints lines matching a word in any of them:
//! `cargo run -p fastfile_h1 --release --example h1_grep -- <zone.ff> <word>`.

fn main() {
    let mut args = std::env::args_os().skip(1);
    let path: std::path::PathBuf = args.next().expect("zone path").into();
    let word = args
        .next()
        .map(|w| w.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    let zone = fastfile_h1::Zone::open(&path).expect("zone");
    let (_map, cap) = fastfile_h1::map::IwMap::load(&zone);
    for r in &cap.raw_files {
        let name = r.name.clone();
        let Some(bytes) = fastfile_h1::map::raw_file(&cap, &zone.image, &name) else {
            println!("-- {name}: unreadable");
            continue;
        };
        let text = String::from_utf8_lossy(&bytes);
        let hits: Vec<&str> = text
            .lines()
            .filter(|l| l.to_lowercase().contains(&word))
            .collect();
        println!("-- {name} ({} bytes, {} hits)", bytes.len(), hits.len());
        for h in hits.iter().take(20) {
            println!("   {}", h.trim());
        }
    }
}
