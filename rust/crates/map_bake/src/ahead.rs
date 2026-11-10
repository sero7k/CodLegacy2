//! bo2mp (bake all): every game's maps baked ahead, one after another, into
//! the files their first loads would write, so even a first load reads one
//! file. Options > BAKE ALL runs it on a thread of its own ([`start`]); the
//! `assets` example `bake_all` runs it from the command line ([`run`]).
//!
//! The games it can bake are a table of [`Game`]s in `assets::bake_all`
//! (one entry per game: which maps, and the function that bakes one). Each
//! bake reports into [`crate::status`] like a load's bake does (game, map,
//! step, counts, why, cancel), so the row reads the same everywhere.
//!
//! Gameplay data (stats, hitboxes, collision) is never baked.
//!
//! PREPARE ALL MAPS (the same run, at launch or from Options): each map a
//! run finishes is written to a record in the bake's root ([`RECORD`]),
//! with the game file's size and time and a stamp of this program and the
//! settings the files depend on ([`set_stamp_extra`]). A resumed run
//! ([`start`] with `resume`) skips the maps in the record at once, so a run
//! stopped with Esc goes on from where it stopped next launch; a changed
//! program, setting or map file checks that map again.
//!
//! Times: each map a run makes is timed, and each game's time per map (a
//! running average of the latest runs on this PC) is kept beside the record
//! ([`TIMES`]). [`left`] gives each game's maps still to do and
//! [`eta_left`] the running job's time still to go, from those times.

use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

/// One game the runner can bake.
#[derive(Clone, Copy, Debug)]
pub struct Game {
    /// Its key in the game folders ("bo3").
    pub key: &'static str,
    /// Its code in the bake folder and on the row ("t7").
    pub code: &'static str,
    /// The maps to bake, from the folder set for the game (its install
    /// folder, a folder of map files, or one map file).
    pub maps: fn(&Path) -> Vec<PathBuf>,
    /// Bakes one map into the files its first load writes, reporting into
    /// [`crate::status`] (begin, set_why, add, is_canceled, end). A map whose
    /// files are already good is left alone. `Ok`: the line to show;
    /// `Err`: why it was not baked (the run goes on).
    pub bake: fn(&Path) -> Result<String, String>,
}

/// One map to bake.
pub type Item = (Game, PathBuf);

static RUNNING: AtomicBool = AtomicBool::new(false);
static STOP: AtomicBool = AtomicBool::new(false);
static NOW: Mutex<Option<Now>> = Mutex::new(None);
static FINISHED: Mutex<Option<String>> = Mutex::new(None);
static STAMP_EXTRA: Mutex<String> = Mutex::new(String::new());

/// The record of prepared maps, in the bake's root.
pub const RECORD: &str = "prepared.txt";

/// Each game's measured time per map, in the bake's root
/// ("<code>\t<maps counted>\t<seconds per map>" a line).
pub const TIMES: &str = "prepare_times.txt";

/// How many of the latest maps a game's time follows (a running average:
/// older runs count less once there are more).
const TIMES_WEIGHT: f64 = 20.0;

/// The running job's place for its time still to go.
struct RunEta {
    /// Seconds per map by game code, read at the start and updated by the run.
    times: std::collections::HashMap<String, (f64, f64)>,
    /// The codes of the maps still to make after the one being made.
    todo: std::collections::VecDeque<&'static str>,
    /// The map being made: its code and when it started.
    current: Option<(&'static str, std::time::Instant)>,
}

static ETA: Mutex<Option<RunEta>> = Mutex::new(None);

/// Time still to go.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Eta {
    /// Seconds, from the measured times.
    pub secs: f64,
    /// Maps of games with no time measured yet (not in `secs`).
    pub untimed: usize,
}

/// One game's maps still to make.
#[derive(Clone, Debug, PartialEq)]
pub struct Left {
    pub key: &'static str,
    pub code: &'static str,
    /// Maps not made yet by this version and settings.
    pub left: usize,
    /// All its maps.
    pub total: usize,
    /// Measured seconds per map on this PC (`None`: never timed).
    pub secs_per_map: Option<f64>,
}

impl Left {
    /// The time its maps still take (`None`: never timed and some left).
    pub fn secs(&self) -> Option<f64> {
        if self.left == 0 {
            return Some(0.0);
        }
        self.secs_per_map.map(|s| s * self.left as f64)
    }
}

/// The map being baked and where it is in the list.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Now {
    /// 1-based.
    pub index: usize,
    pub of: usize,
    /// "Black Ops 3 - Skyjacked" (the start of its row's label).
    pub map: String,
}

/// How a run went.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Summary {
    pub of: usize,
    pub ok: usize,
    pub failed: usize,
    pub stopped: bool,
    /// Of `ok`: maps the record already had (nothing to do).
    pub ready: usize,
}

impl Summary {
    /// "Prepared 37 maps." / "Preparing stopped after 3 of 37 maps." for
    /// the menu.
    pub fn line(&self) -> String {
        let done = self.ok + self.failed;
        let mut not = Vec::new();
        if self.ready > 0 {
            not.push(format!("{} were ready", self.ready));
        }
        if self.failed > 0 {
            not.push(format!("{} could not be prepared", self.failed));
        }
        let not = if not.is_empty() { String::new() } else { format!(" ({})", not.join(", ")) };
        if self.of == 0 {
            "No maps to prepare: set a game folder first.".to_owned()
        } else if self.stopped {
            format!("Preparing stopped after {done} of {} maps{not}; the next run goes on from there.", self.of)
        } else {
            format!("Prepared {} maps{not}.", self.of)
        }
    }
}

/// A run is going.
pub fn running() -> bool {
    RUNNING.load(Ordering::Relaxed)
}

/// A stop was asked for (a map's bake can end early between its steps).
pub fn stopping() -> bool {
    STOP.load(Ordering::Relaxed)
}

/// The settings the baked files depend on, as text (picture size, renderer,
/// quality): part of each record line's stamp, so a changed setting checks
/// every map again.
pub fn set_stamp_extra(extra: String) {
    if let Ok(mut e) = STAMP_EXTRA.lock() {
        *e = extra;
    }
}

/// This program (its file's size and time) and the settings, as one value.
fn stamp() -> u64 {
    let mut k = crate::Key::new();
    k.str("prepared 1");
    if let Ok(exe) = std::env::current_exe() {
        k.file(&exe);
    }
    k.str(&STAMP_EXTRA.lock().map(|e| e.clone()).unwrap_or_default());
    k.value()
}

/// A map's line in the record: the stamp, the game, the map file's size and
/// time, and its path.
fn record_line(stamp: u64, game: &Game, map: &Path) -> String {
    let mut k = crate::Key::new();
    k.file(map);
    format!("{stamp:016x}\t{}\t{:016x}\t{}", game.code, k.value(), map.display())
}

/// The record's lines (none when there is no record).
fn read_record(path: &Path) -> std::collections::HashSet<String> {
    std::fs::read_to_string(path).map(|t| t.lines().map(str::to_owned).collect()).unwrap_or_default()
}

/// Adds one line to the record (a record that cannot be written only means
/// the next run checks that map again).
fn add_record(path: &Path, line: &str) {
    use std::io::Write;
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
        let _ = writeln!(f, "{line}");
    }
}

/// The record's place: the bake's root (`None`: the bake is off).
pub fn record_path() -> Option<PathBuf> {
    crate::root().map(|r| r.join(RECORD))
}

/// The times' place: the bake's root.
pub fn times_path() -> Option<PathBuf> {
    crate::root().map(|r| r.join(TIMES))
}

/// The measured times: code -> (maps counted, seconds per map).
fn read_times(path: Option<&Path>) -> std::collections::HashMap<String, (f64, f64)> {
    let Some(text) = path.and_then(|p| std::fs::read_to_string(p).ok()) else {
        return Default::default();
    };
    text.lines()
        .filter_map(|l| {
            let mut f = l.split('\t');
            let code = f.next()?.to_owned();
            let n = f.next()?.parse().ok()?;
            let secs: f64 = f.next()?.parse().ok()?;
            (secs.is_finite() && secs >= 0.0).then_some((code, (n, secs)))
        })
        .collect()
}

/// Adds one map's time to its game's running average.
fn add_time(times: &mut std::collections::HashMap<String, (f64, f64)>, code: &str, secs: f64) {
    let (n, avg) = times.entry(code.to_owned()).or_insert((0.0, 0.0));
    *n = (*n + 1.0).min(TIMES_WEIGHT);
    *avg += (secs - *avg) / *n;
}

fn write_times(path: &Path, times: &std::collections::HashMap<String, (f64, f64)>) {
    let mut lines: Vec<String> = times.iter().map(|(c, (n, s))| format!("{c}\t{n}\t{s:.2}")).collect();
    lines.sort();
    let _ = std::fs::write(path, lines.join("\n") + "\n");
}

/// Each game of `items` (in order): its maps still to make by this version
/// and settings (the record), and its measured time per map.
pub fn left(items: &[Item]) -> Vec<Left> {
    left_in(items, record_path().as_deref(), times_path().as_deref())
}

fn left_in(items: &[Item], record: Option<&Path>, times: Option<&Path>) -> Vec<Left> {
    let stamp = stamp();
    let done = record.map(read_record).unwrap_or_default();
    let times = read_times(times);
    let mut out: Vec<Left> = Vec::new();
    for (game, map) in items {
        let i = match out.iter().position(|l| l.key == game.key) {
            Some(i) => i,
            None => {
                out.push(Left { key: game.key, code: game.code, left: 0, total: 0, secs_per_map: times.get(game.code).map(|t| t.1) });
                out.len() - 1
            }
        };
        out[i].total += 1;
        if !done.contains(&record_line(stamp, game, map)) {
            out[i].left += 1;
        }
    }
    out
}

/// The running job's time still to go (`None`: no job running).
pub fn eta_left() -> Option<Eta> {
    let g = ETA.lock().ok()?;
    let r = g.as_ref()?;
    let mut e = Eta::default();
    let mut add = |code: &str, spent: f64| match r.times.get(code) {
        Some((_, s)) => e.secs += (s - spent).max(0.0),
        None => e.untimed += 1,
    };
    if let Some((code, t)) = r.current {
        add(code, t.elapsed().as_secs_f64());
    }
    for code in &r.todo {
        add(code, 0.0);
    }
    Some(e)
}

/// The map a run is on.
pub fn now() -> Option<Now> {
    NOW.lock().ok().and_then(|n| n.clone())
}

/// The finished run's line, once.
pub fn take_finished() -> Option<String> {
    FINISHED.lock().ok()?.take()
}

/// Every map of `games`, each from `folder(game)` (a game with none is left out).
pub fn plan(games: &[Game], folder: impl Fn(&Game) -> Option<PathBuf>) -> Vec<Item> {
    games
        .iter()
        .filter_map(|g| folder(g).map(|f| (*g, f)))
        .flat_map(|(g, f)| (g.maps)(&f).into_iter().map(move |m| (g, m)))
        .collect()
}

/// The start of a map's row label ("Black Ops 3 - Skyjacked").
fn row_prefix(game: &Game, map: &Path) -> String {
    let stem = map.file_stem().and_then(|s| s.to_str()).unwrap_or("").to_ascii_lowercase();
    format!("{} - {}", crate::status::game_title(game.code), crate::status::map_title(&stem))
}

/// Ends a run after (and cancels) the map it is baking; the half-made file
/// is thrown away. A load's own bake of another map is not touched.
pub fn stop() {
    if !running() {
        return;
    }
    STOP.store(true, Ordering::Relaxed);
    if let Some(now) = now()
        && let Some(job) = crate::status::current()
        && job.outcome().is_none()
        && job.label().starts_with(&format!("{} - ", now.map))
    {
        job.cancel();
    }
}

/// Bakes `items` on this thread, map after map; `out` gets one line per
/// map. `Err`: a run is already going.
pub fn run(items: &[Item], out: impl FnMut(&str)) -> Result<Summary, String> {
    if RUNNING.swap(true, Ordering::AcqRel) {
        return Err("A bake is already running.".to_owned());
    }
    let s = run_claimed(items, out, None, None, false);
    RUNNING.store(false, Ordering::Release);
    Ok(s)
}

/// [`run`], timing each map into the bake's times file (the `bake_all`
/// example: measures this PC for the menu's times; no record is written,
/// so the game still checks each map once).
pub fn run_timed(items: &[Item], out: impl FnMut(&str)) -> Result<Summary, String> {
    if RUNNING.swap(true, Ordering::AcqRel) {
        return Err("A bake is already running.".to_owned());
    }
    let times = times_path();
    let s = run_claimed(items, out, None, times.as_deref(), false);
    RUNNING.store(false, Ordering::Release);
    Ok(s)
}

/// The run itself. `record`: where finished maps are written down;
/// `times`: where each game's time per map is kept; `resume`: maps already
/// in the record are skipped (else every map is checked).
fn run_claimed(items: &[Item], mut out: impl FnMut(&str), record: Option<&Path>, times: Option<&Path>, resume: bool) -> Summary {
    STOP.store(false, Ordering::Relaxed);
    let mut s = Summary { of: items.len(), ..Summary::default() };
    let stamp = stamp();
    let mut done = record.map(read_record).unwrap_or_default();
    let todo = items.iter().filter(|(g, m)| !(resume && done.contains(&record_line(stamp, g, m)))).map(|(g, _)| g.code).collect();
    if let Ok(mut e) = ETA.lock() {
        *e = Some(RunEta { times: read_times(times), todo, current: None });
    }
    for (i, (game, map)) in items.iter().enumerate() {
        if STOP.load(Ordering::Relaxed) {
            s.stopped = true;
            break;
        }
        let line = record_line(stamp, game, map);
        if resume && done.contains(&line) {
            s.ok += 1;
            s.ready += 1;
            continue;
        }
        let now = Now { index: i + 1, of: items.len(), map: row_prefix(game, map) };
        if let Ok(mut n) = NOW.lock() {
            *n = Some(now);
        }
        let t0 = std::time::Instant::now();
        if let Ok(mut e) = ETA.lock()
            && let Some(e) = e.as_mut()
        {
            e.todo.pop_front();
            e.current = Some((game.code, t0));
        }
        let made = (game.bake)(map);
        // (A whole map's time, made or failed, into its game's time per map;
        // a map stopped part way says nothing about it.)
        if !STOP.load(Ordering::Relaxed)
            && let Ok(mut e) = ETA.lock()
            && let Some(e) = e.as_mut()
        {
            e.current = None;
            add_time(&mut e.times, game.code, t0.elapsed().as_secs_f64());
            if let Some(t) = times {
                write_times(t, &e.times);
            }
        }
        match made {
            Ok(l) => {
                s.ok += 1;
                out(&format!("[{}/{}] {l}", i + 1, items.len()));
            }
            Err(e) => {
                s.failed += 1;
                out(&format!("[{}/{}] {}: not baked ({e})", i + 1, items.len(), map.display()));
            }
        }
        // (A map stopped part way is not written down: the next run makes
        // it. One that failed is, so a launch does not retry it each time.)
        if !STOP.load(Ordering::Relaxed)
            && let Some(r) = record
            && done.insert(line.clone())
        {
            add_record(r, &line);
        }
    }
    // (Stopped during the last map.)
    s.stopped |= STOP.load(Ordering::Relaxed);
    if let Ok(mut n) = NOW.lock() {
        *n = None;
    }
    if let Ok(mut e) = ETA.lock() {
        *e = None;
    }
    s
}

/// PREPARE ALL MAPS: bakes `items` on a thread of its own, one line per
/// map to `out`; when it is done [`take_finished`] has its line. `resume`:
/// maps the record already has are skipped (the launch's run); without it
/// every map is checked (Options > PREPARE ALL MAPS NOW). `Ok`: how many
/// maps (0: nothing started).
pub fn start(items: Vec<Item>, out: fn(&str), resume: bool) -> Result<usize, String> {
    if items.is_empty() {
        return Ok(0);
    }
    if RUNNING.swap(true, Ordering::AcqRel) {
        return Err("A bake is already running.".to_owned());
    }
    let n = items.len();
    let spawned = std::thread::Builder::new().name("bake-all".into()).spawn(move || {
        let record = record_path();
        let times = times_path();
        let s = run_claimed(&items, out, record.as_deref(), times.as_deref(), resume);
        RUNNING.store(false, Ordering::Release);
        if let Ok(mut f) = FINISHED.lock() {
            *f = Some(s.line());
        }
    });
    if spawned.is_err() {
        RUNNING.store(false, Ordering::Release);
        return Err("Could not start the bake.".to_owned());
    }
    Ok(n)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn maps(f: &Path) -> Vec<PathBuf> {
        ["mp_a.ff", "mp_b.ff", "mp_c.ff"].iter().map(|m| f.join(m)).collect()
    }

    fn bake(m: &Path) -> Result<String, String> {
        let stem = m.file_stem().and_then(|s| s.to_str()).unwrap_or("");
        match stem {
            "mp_b" => Err("broken".into()),
            _ => Ok(format!("{stem} baked")),
        }
    }

    fn bake_and_stop(m: &Path) -> Result<String, String> {
        // (Cancel pressed while the second map bakes.)
        if m.ends_with("mp_b.ff") {
            let job = crate::status::begin("t7", "mp_b", "pictures", 10);
            stop();
            assert!(job.is_canceled(), "the map being baked is cancelled");
        }
        Ok("ok".into())
    }

    /// (One test: the run state is the process's.)
    #[test]
    fn runs_every_map_and_stops() {
        let game = Game { key: "bo3", code: "t7", maps, bake };
        let none = Game { key: "x", ..game };
        let items = plan(&[game, none], |g| (g.key == "bo3").then(|| PathBuf::from("z")));
        assert_eq!(items.len(), 3);
        let mut lines = Vec::new();
        let s = run(&items, |l| lines.push(l.to_owned())).expect("not running");
        assert_eq!(s, Summary { of: 3, ok: 2, failed: 1, stopped: false, ready: 0 });
        assert_eq!(lines[0], "[1/3] mp_a baked");
        assert!(lines[1].ends_with("not baked (broken)"));
        assert_eq!(s.line(), "Prepared 3 maps (1 could not be prepared).");
        assert!(!running() && now().is_none());

        let stopping = Game { bake: bake_and_stop, ..game };
        let items = plan(&[stopping], |_| Some(PathBuf::from("z")));
        let s = run(&items, |_| {}).expect("not running");
        assert_eq!(s, Summary { of: 3, ok: 2, failed: 0, stopped: true, ready: 0 });
        assert_eq!(s.line(), "Preparing stopped after 2 of 3 maps; the next run goes on from there.");
        assert_eq!(Summary::default().line(), "No maps to prepare: set a game folder first.");
        assert_eq!(start(Vec::new(), |_| {}, true), Ok(0));

        resumes_from_the_record(game);
    }

    static BAKED: Mutex<Vec<String>> = Mutex::new(Vec::new());
    static STOPPED_ONCE: AtomicBool = AtomicBool::new(false);

    fn bake_count(m: &Path) -> Result<String, String> {
        let stem = m.file_stem().and_then(|s| s.to_str()).unwrap_or("").to_owned();
        let mut b = BAKED.lock().unwrap();
        b.push(stem.clone());
        if stem == "mp_b" && !STOPPED_ONCE.swap(true, Ordering::Relaxed) {
            // (Esc on the second map, the first time only.)
            STOP.store(true, Ordering::Relaxed);
        }
        Ok("ok".into())
    }

    fn baked() -> Vec<String> {
        std::mem::take(&mut *BAKED.lock().unwrap())
    }

    /// Stopped on the second map: the next run starts there; a changed
    /// setting or map file checks the map again; a full check checks all.
    fn resumes_from_the_record(game: Game) {
        let dir = std::env::temp_dir().join(format!("map_bake_ahead_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        for m in ["mp_a.ff", "mp_b.ff", "mp_c.ff"] {
            std::fs::write(dir.join(m), m).unwrap();
        }
        let record = dir.join(RECORD);
        let g = Game { bake: bake_count, ..game };
        let items = plan(&[g], |_| Some(dir.clone()));

        let times = dir.join(TIMES);
        let l = left_in(&items, Some(&record), Some(&times));
        assert_eq!((l.len(), l[0].left, l[0].total, l[0].secs_per_map, l[0].secs()), (1, 3, 3, None, None), "nothing made or timed yet");
        let s = run_claimed(&items, |_| {}, Some(&record), Some(&times), true);
        assert!(s.stopped);
        assert_eq!(baked(), ["mp_a", "mp_b"]);
        assert_eq!(read_record(&record).len(), 1, "only the finished map is written down");
        let l = left_in(&items, Some(&record), Some(&times));
        assert_eq!(l[0].left, 2);
        let per = l[0].secs_per_map.expect("the finished map was timed");
        assert!(per >= 0.0 && l[0].secs() == Some(per * 2.0));
        assert_eq!(read_times(Some(&times)).get("t7").map(|t| t.0), Some(1.0), "the stopped map is not timed");

        let s = run_claimed(&items, |_| {}, Some(&record), Some(&times), true);
        assert_eq!(baked(), ["mp_b", "mp_c"], "goes on from the stopped map");
        assert_eq!(left_in(&items, Some(&record), Some(&times))[0].secs(), Some(0.0), "done");
        assert!(eta_left().is_none(), "no job, no time to go");
        assert_eq!(s, Summary { of: 3, ok: 3, failed: 0, stopped: false, ready: 1 });
        assert_eq!(s.line(), "Prepared 3 maps (1 were ready).");

        let s = run_claimed(&items, |_| {}, Some(&record), None, true);
        assert!(baked().is_empty(), "all ready");
        assert_eq!(s.ready, 3);

        std::fs::write(dir.join("mp_c.ff"), "a newer mp_c, longer").unwrap();
        run_claimed(&items, |_| {}, Some(&record), None, true);
        assert_eq!(baked(), ["mp_c"], "a changed map file is checked again");

        set_stamp_extra("t7 cap 2048".into());
        run_claimed(&items, |_| {}, Some(&record), None, true);
        assert_eq!(baked(), ["mp_a", "mp_b", "mp_c"], "a changed setting checks every map");
        set_stamp_extra(String::new());

        run_claimed(&items, |_| {}, Some(&record), None, false);
        assert_eq!(baked(), ["mp_a", "mp_b", "mp_c"], "a full check checks every map");

        // (The running average: a game's time follows its latest maps.)
        let mut t = std::collections::HashMap::new();
        add_time(&mut t, "t6", 10.0);
        add_time(&mut t, "t6", 20.0);
        assert_eq!(t["t6"], (2.0, 15.0));
        for _ in 0..300 {
            add_time(&mut t, "t6", 4.0);
        }
        assert!((t["t6"].1 - 4.0).abs() < 0.01 && t["t6"].0 == TIMES_WEIGHT);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
