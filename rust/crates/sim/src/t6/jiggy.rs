//! bo2mp: Jiggy Free For All (Custom Games only): Free For All with the
//! Jiggy 4.3 mod menu's two compiled scripts linked over Black Ops II's own
//! `maps/mp/_ambientpackage` and `maps/mp/_development_dvars`, as a Plutonium
//! server does with a replaced script. Its `init()` runs once the game type
//! has started (the menu opens from there; `onplayerconnect` and
//! `onplayerspawned` follow).
//!
//! Jiggy's `main.gsc` (source: the stock `_rank` with the menu's welcome
//! text and messages in its `init`) is compiled here and replaces
//! `maps/mp/gametypes/_rank`; its `init` runs just before the menu's so the
//! menu finds its settings.
//!
//! The scripts are the author's compiled work and are not part of this
//! repository: they are read at match start from a `jiggy` folder holding
//! `_ambientpackage.gsc` and `_development_dvars.gsc` (also accepted under
//! `maps/mp/`), looked up in `CODLEGACY_JIGGY`, then next to `CodLegacy.exe`,
//! then the working directory. Without them (or if they do not read or link)
//! the match plays as plain Free For All and the log says why.
//!
//! The compiled files carry their compiler's own name inside
//! (`maps/mp/_imcsx_gsc_studio.gsc`), so each is renamed to the stock script
//! it replaces before linking: the linker keeps the later object of a name.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use gsc_t6::ScriptObject;

/// The stock scripts the two files replace.
const SCRIPTS: [&str; 2] = ["maps/mp/_ambientpackage", "maps/mp/_development_dvars"];

/// The script whose `init` starts the menu.
pub(super) const MENU: &str = "maps/mp/_development_dvars";

static ON: AtomicBool = AtomicBool::new(false);
static ACTIVE: AtomicBool = AtomicBool::new(false);
static RANK: AtomicBool = AtomicBool::new(false);

/// The stock script Jiggy's `main.gsc` replaces.
pub(super) const RANK_SCRIPT: &str = "maps/mp/gametypes/_rank";

/// Whether the running match links Jiggy's `_rank` (its `init` must run).
pub(crate) fn rank_active() -> bool {
    RANK.load(Ordering::Relaxed)
}

/// bo2mp: the lobby's JIGGY FREE FOR ALL for the next match (Custom Games
/// only: a public match never sets it).
pub fn set_jiggy(on: bool) {
    ON.store(on, Ordering::Relaxed);
}

/// Whether this match wants the Jiggy scripts: the lobby's choice, or
/// `BO2MP_JIGGY=0/1`.
pub(super) fn enabled() -> bool {
    match codlegacy_env::var("BO2MP_JIGGY") {
        Ok(v) => v.trim() == "1",
        Err(_) => ON.load(Ordering::Relaxed),
    }
}

/// Whether the running match links the Jiggy scripts (set once they are read).
pub(crate) fn active() -> bool {
    ACTIVE.load(Ordering::Relaxed)
}

/// The scripts did not link: the match goes on without them.
pub(super) fn deactivate() {
    ACTIVE.store(false, Ordering::Relaxed);
    RANK.store(false, Ordering::Relaxed);
}

fn folders() -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Ok(p) = codlegacy_env::var("CODLEGACY_JIGGY") {
        out.push(PathBuf::from(p.trim()));
    }
    if let Some(dir) = std::env::current_exe()
        .ok()
        .and_then(|e| e.parent().map(Path::to_path_buf))
    {
        out.push(dir.join("jiggy"));
    }
    out.push(PathBuf::from("jiggy"));
    out
}

/// One script's bytes from `dir`: `<dir>/maps/mp/<file>.gsc`, else `<dir>/<file>.gsc`.
fn read_script(dir: &Path, name: &str) -> Option<Vec<u8>> {
    let file = name.rsplit('/').next()?;
    let nested = dir.join(format!("{name}.gsc"));
    let flat = dir.join(format!("{file}.gsc"));
    std::fs::read(nested).or_else(|_| std::fs::read(flat)).ok()
}

/// The scripts, parsed and named as the stock scripts they replace: Jiggy's
/// `main.gsc` (compiled here) as `_rank`, and the menu's two compiled files.
/// Empty (and the log says why) when none is there or they do not read.
pub(super) fn objects() -> Vec<ScriptObject> {
    deactivate();
    let places = folders();
    let Some(dir) = places.iter().find(|d| {
        d.join("main.gsc").is_file() || SCRIPTS.iter().all(|n| read_script(d, n).is_some())
    }) else {
        diag::warn!(
            Sim,
            "jiggy: main.gsc, _ambientpackage.gsc and _development_dvars.gsc not found (looked in {:?}); playing plain Free For All",
            places
        );
        return Vec::new();
    };
    let mut out = Vec::new();
    match std::fs::read(dir.join("main.gsc")) {
        Ok(bytes) => match gsc_t6::compile(RANK_SCRIPT, &String::from_utf8_lossy(&bytes)) {
            Ok(object) => {
                out.push(object);
                RANK.store(true, Ordering::Relaxed);
            }
            Err(e) => diag::warn!(
                Sim,
                "jiggy: main.gsc does not compile ({}); the stock _rank stays",
                e
            ),
        },
        Err(_) => diag::warn!(
            Sim,
            "jiggy: main.gsc not found in {}; the stock _rank stays",
            dir.display()
        ),
    }
    let mut menu = Vec::new();
    for name in SCRIPTS {
        let Some(bytes) = read_script(dir, name) else {
            diag::warn!(
                Sim,
                "jiggy: {} not found in {}; no menu",
                name,
                dir.display()
            );
            menu.clear();
            break;
        };
        match ScriptObject::parse(&bytes) {
            Ok(mut object) => {
                object.name = name.to_owned();
                menu.push(object);
            }
            Err(e) => {
                diag::warn!(Sim, "jiggy: {} does not read ({}); no menu", name, e);
                menu.clear();
                break;
            }
        }
    }
    if !menu.is_empty() {
        ACTIVE.store(true, Ordering::Relaxed);
    }
    out.extend(menu);
    if out.is_empty() {
        return out;
    }
    diag::info!(
        Sim,
        "jiggy: linked {} scripts from {} (rank {}, menu {})",
        out.len(),
        dir.display(),
        rank_active(),
        active()
    );
    out
}
