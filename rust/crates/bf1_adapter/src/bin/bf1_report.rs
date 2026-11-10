//! `bf1_report [<folder with Data and Patch>] [mp_verdun]`: lists the BF1
//! maps and prints the world report of one. The folder defaults to
//! `BF1_FILES` / `BF1_GAME_DIR`. Read-only.

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (folder, map) = match args.as_slice() {
        [] => (None, "mp_verdun".to_owned()),
        [a] if a.contains(['/', '\\', ':']) || std::path::Path::new(a).is_dir() => (Some(std::path::PathBuf::from(a)), "mp_verdun".to_owned()),
        [a] => (None, a.clone()),
        [a, b, ..] => (Some(std::path::PathBuf::from(a)), b.clone()),
    };
    let root = match bf1_adapter::config::resolve_from_env(folder.as_deref()) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(2);
        }
    };
    println!("Battlefield 1 install: {}", root.display());
    for m in bf1_adapter::catalog::catalog(Some(&root)) {
        println!("  bf1:{}  {}", m.id, m.label());
    }
    let map = match bf1_adapter::catalog::parse_spec(&map) {
        Ok(m) => m,
        Err(e) => {
            eprintln!("{e} (only the maps tagged `reader only` can be reported)");
            std::process::exit(2);
        }
    };
    match bf1_adapter::report::open_and_report(&root, map) {
        Ok(r) => r.lines().iter().for_each(|l| println!("{l}")),
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(1);
        }
    }
}
