//! bo2mp (bake all): bakes maps ahead into the files their first loads
//! write (`map_bake`: `CODLEGACY_BAKE_DIR`, else `D:\codlegacy-bake`), so
//! even a first load reads one file. No graphics card and no game.
//!
//! `bake_all`: every game set in GAME FOLDERS (as Options > BAKE ALL).
//! `bake_all <game> <folder or map file>...`: that game's maps there
//! (`bake_all bo3 D:\COD\bo3_game_files\zone`). Games: `assets::bake_all::GAMES`.
fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let items = match args.split_first() {
        None => assets::bake_all::plan_from_folders(),
        Some((key, paths)) => {
            let Some(game) = assets::bake_all::game(key) else {
                let keys: Vec<&str> = assets::bake_all::GAMES.iter().map(|g| g.key).collect();
                eprintln!("usage: bake_all [<game> <folder or map file>...]; games: {}", keys.join(", "));
                std::process::exit(2);
            };
            paths.iter().flat_map(|p| (game.maps)(std::path::Path::new(p)).into_iter().map(move |m| (game, m))).collect()
        }
    };
    println!("{} maps", items.len());
    let t = std::time::Instant::now();
    // (Timed: each game's time per map goes beside the bake, for the
    // PREPARE MAPS menu's times.)
    match assets::bake_all::run_timed(&items, |line| println!("{line}")) {
        Ok(s) => println!("{} ({:.0}s)", s.line(), t.elapsed().as_secs_f32()),
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(1);
        }
    }
}
