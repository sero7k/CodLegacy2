//! Offline check of the Black Ops III Workshop weapon packs' guns
//! (`bo2_profile::guest::WORKSHOP_PACKS`, docs/WORKSHOP_GUNS.md):
//!
//! ```text
//! cargo run --release -p assets --example t7_pack_guns -- [--build] [packs]
//! ```
//!
//! For each installed pack (its Workshop folder found as the game finds it,
//! with Black Ops III's `zone` folder from `game_paths.cfg` or `CODLEGACY_GAMES`),
//! every roster gun: its Create-a-Class group, whether the pack's zone has
//! it, its view and world models with their mesh data and colour maps, and
//! its animations. `--build` also builds the guns as a match does (models,
//! pictures, animations, weapon rows) and reports what came out. `packs`:
//! a comma list of pack tags (`cw,vg,mw19,cod2,ol,ol2`), games or ids.
fn main() {
    let mut build = false;
    let mut filter = None;
    for a in std::env::args().skip(1) {
        if a == "--build" {
            build = true;
        } else {
            filter = Some(a);
        }
    }
    for line in assets::workshop_gun_check(build, filter.as_deref()) {
        println!("{line}");
    }
}
