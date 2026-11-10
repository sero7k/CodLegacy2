//! Lists the sound aliases, materials and images of a zone whose names
//! contain a word: `cargo run -p fastfile_h1 --release --example h1_names -- <zone.ff> <word>...`.

fn main() {
    let mut args = std::env::args_os().skip(1);
    let path: std::path::PathBuf = args.next().expect("zone path").into();
    let words: Vec<String> = args.map(|w| w.to_string_lossy().to_lowercase()).collect();
    let zone = fastfile_h1::Zone::open(&path).expect("zone");
    let (cap, stop) = fastfile_h1::walk::walk_zone(&zone);
    println!("stop: {stop:?}");
    let hit = |n: &str| words.is_empty() || words.iter().any(|w| n.to_lowercase().contains(w));
    for l in &cap.sounds {
        for a in &l.aliases {
            if hit(&a.name) {
                let f = a.file.and_then(|v| cap.sound_files.get(&v));
                println!("sound {} (list {}) kind {:?} loaded {} secondary {:?} chain {:?}", a.name, l.name, f.map(|f| f.kind), f.is_some_and(|f| f.loaded.is_some()), a.secondary, a.chain);
            }
        }
    }
    for m in &cap.materials {
        if hit(&m.name) {
            println!("material {}", m.name);
        }
    }
    for i in &cap.images {
        if hit(&i.name) {
            println!("image {}", i.name);
        }
    }
}
