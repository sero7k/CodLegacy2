//! bo2mp (bake all): a guest game's map (Black Ops, MW2, MW3, CoD4, WaW)
//! baked ahead of time, with no game and no card: the map is loaded as a
//! match load does it and `asset_material::map_pak` writes what it reads
//! (mips.pak) and the programs made from it (programs.pak) under the map's
//! bake folder. Nothing is drawn; stats, hitboxes and collision are never
//! baked.
//!
//! The programs are made by the renderer (`render_frontend`), which this
//! crate cannot see: it hands its maker over with [`set_program_maker`]
//! (its plugin does). With none, the map's pictures are baked alone.

use std::path::Path;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

use bevy::tasks::futures_lite::future::block_on;

use crate::session_load::{MatchLoadOutcome, PreparedMatch, load_prepared_match, ready_match_common};

/// Makes the programs of a prepared map as the world spawn makes them (CPU
/// only): `Ok(how many)`.
pub type ProgramMaker = fn(PreparedMatch) -> Result<usize, String>;

static PROGRAM_MAKER: OnceLock<ProgramMaker> = OnceLock::new();

/// The renderer's program maker, for [`bake_ahead`].
pub fn set_program_maker(maker: ProgramMaker) {
    let _ = PROGRAM_MAKER.set(maker);
}

/// Load ids for bake-only loads, far above a game's own requests.
static NEXT_LOAD: AtomicU64 = AtomicU64::new(1 << 62);

/// A guest game's map the bake takes: its zone is one `map_pak` bakes
/// (`None` there: Black Ops II, Black Ops III, or the bake is off).
pub fn is_guest_map(zone_ff: &Path) -> bool {
    asset_material::map_pak::baked(zone_ff).is_some()
}

/// Bakes the guest map in `zone_ff` (an installed map's `.ff`, named as the
/// game names it: the bake's key is the path). Same contract as
/// `render_t7::bake_ahead`: "<map> already baked" when its copy is good,
/// the bake's row while it bakes, and a cancelled bake throws its files away.
pub fn bake_ahead(zone_ff: &Path) -> Result<String, String> {
    // The game's main thread has an 8 MB stack (`crates/launcher/build.rs`
    // `/STACK`), where a map loads; a BAKE ALL thread (2 MB) or the offline
    // tool's main thread (1 MB) is too small for a BO1 map's recursive zone
    // walk. So the bake runs on a thread with the game's stack.
    let zone_ff = zone_ff.to_path_buf();
    let map = zone_ff.file_stem().map_or_else(String::new, |s| s.to_string_lossy().to_ascii_lowercase());
    std::thread::Builder::new()
        .name("bake-guest".into())
        .stack_size(8 << 20)
        .spawn(move || bake_on_this_thread(&zone_ff))
        .map_err(|e| format!("{map}: no bake thread: {e}"))?
        .join()
        .map_err(|_| format!("{map}: the bake panicked"))?
}

fn bake_on_this_thread(zone_ff: &Path) -> Result<String, String> {
    let map = zone_ff.file_stem().map_or_else(String::new, |s| s.to_string_lossy().to_ascii_lowercase());
    match asset_material::map_pak::baked(zone_ff) {
        None => return Err(format!("{map}: not a guest map, or the bake is off")),
        Some(true) => return Ok(format!("{map} already baked")),
        Some(false) => {}
    }
    let t0 = Instant::now();
    let games = asset_transport::games_root_from_env().map_err(|e| format!("{map}: {e}"))?;
    let common_mp = crate::match_common_mp(&games, zone_ff);
    // (The lobby readies the common set before a map loads; so does this,
    // so the map's copy holds its own pictures only.)
    let common = block_on(ready_match_common(zone_ff, common_mp.as_deref().ok()));
    let common_s = t0.elapsed().as_secs_f32();

    let load = NEXT_LOAD.fetch_add(1, Ordering::Relaxed);
    asset_material::map_pak::begin_ahead(load, zone_ff);
    let progress = asset_transport::progress::LoadProgress::new(load);
    let MatchLoadOutcome::Ready(prepared) = block_on(load_prepared_match(Ok(zone_ff.to_path_buf()), common_mp, progress)) else {
        asset_material::map_pak::finish_ahead(load, false);
        return Err(format!("{map}: the load was cancelled"));
    };
    let load_s = t0.elapsed().as_secs_f32() - common_s;

    let programs = match PROGRAM_MAKER.get() {
        Some(make) => make(prepared).map(|n| format!("{n} programs")),
        None => Ok("pictures only: no renderer in this run".to_owned()),
    };
    let lines = asset_material::map_pak::finish_ahead(load, programs.is_ok());
    let programs = programs.map_err(|e| format!("{map}: the programs were not made: {e}"))?;
    Ok(format!(
        "{map} in {:.1}s (common set {common} {common_s:.1}s, load {load_s:.1}s, {programs}){}",
        t0.elapsed().as_secs_f32(),
        lines.iter().map(|l| format!("; {l}")).collect::<String>()
    ))
}
