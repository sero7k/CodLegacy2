//! Offline: `load_timing <game> <zone.ff>...` loads each map as `map_check_probe`
//! does, with the game's log open (`CODLEGACY_LOG`, else `logs/` under the
//! working directory), so its `load stage:` lines time every step; no window.
fn main() {
    let log = diag::init_log(std::path::Path::new("."));
    let mut args = std::env::args().skip(1);
    let game = args.next().expect("game prefix");
    for path in args {
        let started = std::time::Instant::now();
        let line = assets::check_map(&game, std::path::Path::new(&path));
        println!("{game}:{path} [{:.1}s] {line}", started.elapsed().as_secs_f32());
    }
    println!("log: {}", log.display());
}
