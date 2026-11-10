//! bo2mp: what a guest map's load reads from the caches, in one file per
//! kind, read start to end (the one-time map bake, lane D).
//!
//! A converted map (Modern Warfare 2 and 3, Black Ops, Call of Duty 4,
//! World at War) takes its pictures' mip chains from the mip cache and its
//! shader programs from the program cache: one small file each. Terminal
//! reads about 5,000 pictures and 10,000 programs, and on a hard drive those
//! scattered reads cost minutes of disk time spread over the loading threads
//! (the programs alone 94 s for 4,105 hits). The bake keeps what one map's
//! load asks for in one file per kind, in the order the load asked for it, so
//! the next load of that map reads that file start to end instead.
//!
//! Where: `<root>\<game>\<map>\<kind>.pak` (the entries, end to end) and
//! `<kind>.idx` (what is where, written last: a pak without its index is a
//! miss). The folder is `map_bake::map_dir` (`CODLEGACY_BAKE_DIR`, else
//! `D:\codlegacy-bake`; with neither, nothing is baked).
//!
//! Every entry keeps its own cache key (a picture's covers the source
//! picture's checksum, size and name and the chain's format; a program's
//! covers its bytecode and the translator's code), so a stale pak can never
//! hand out the wrong entry: what it does not have, or has under another
//! key, comes from the cache as before. When too much of a load is missed
//! that way, the pak is dropped and the next load bakes it again.
//!
//! `CODLEGACY_BAKE=0` turns all baking off; `CODLEGACY_BAKE_MIPS=0` only the
//! pictures, `CODLEGACY_BAKE_PROGRAMS=0` only the programs. CoD4 and WaW
//! maps bake pictures only (they make no programs);
//! `CODLEGACY_BAKE_LEGACY_PROGRAMS=1` bakes their programs too.
//!
//! Format 2 (7 Nov 2026) packs every entry with lz4 (one block each, the
//! index holds both lengths). The hard drive read of the pak was the whole
//! wait of a warm load, and the entries shrink well: mip chains to about
//! two thirds (Black Ops Nuked's 651 MB, MW2 Terminal's 368 MB), programs
//! to a fifteenth (Terminal's 230 MB to 16 MB). The one reading thread
//! stays on the disk; the thread that takes an entry unpacks it (a kept
//! kind unpacks once and keeps the unpacked bytes). A pak made by format 1
//! is a miss and is baked again.

use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::io::{BufReader, BufWriter, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, SyncSender, TrySendError};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::time::{Duration, Instant};

const IDX_MAGIC: &[u8; 8] = b"BO2WMIX1";
const END: &[u8; 8] = b"BO2WEND1";
/// The layout's number (raised when what is written changes).
const FORMAT: u32 = 2;
/// Read and write buffer: large sequential transfers on a hard drive.
const BUF: usize = 8 << 20;
/// Entries read ahead and not yet taken: the reader waits past this.
const READ_AHEAD: usize = 768 << 20;
/// Entries handed to the writer and not yet on disk: past this the bake is
/// given up for this load (the load never waits on the disk).
const WRITE_BEHIND: usize = 512 << 20;

/// One kind of baked cache entry.
pub struct Pak {
    /// File and log name (`mips`: `mips.pak`, `mips.idx`).
    name: &'static str,
    /// Its own off switch (`=0`).
    switch: &'static str,
    /// An entry stays to be taken again (programs: small, and asked for by
    /// more than one draw); otherwise it is handed out once (pictures).
    keep_taken: bool,
    active: Mutex<Option<Session>>,
}

/// Pictures' mip chains (the mip cache's entries).
pub static MIPS: Pak = Pak::new("mips", "CODLEGACY_BAKE_MIPS", false);
/// Shader programs translated to WGSL (the program cache's entries).
pub static PROGRAMS: Pak = Pak::new("programs", "CODLEGACY_BAKE_PROGRAMS", true);

static PAKS: [&Pak; 2] = [&MIPS, &PROGRAMS];

/// A map load starts (`load`: its request number). For a guest map, each
/// kind with a whole pak starts reading it; one without starts baking it.
pub fn begin(load: u64, zone_ff: &Path) {
    let row = game_of(zone_ff).map(|game| Row::new(game, &map_name(zone_ff)));
    for pak in PAKS {
        pak.begin(load, zone_ff, row.as_ref());
    }
}

fn map_name(zone_ff: &Path) -> String {
    zone_ff.file_stem().map_or_else(String::new, |s| s.to_string_lossy().to_ascii_lowercase())
}

/// The map load `load` is over: `ok` when it finished (a bake is kept only
/// then).
pub fn finish(load: u64, ok: bool) {
    for pak in PAKS {
        drop(pak.finish(load, ok));
    }
}

/// bo2mp (bake all): is every kind of the map in `zone_ff` baked whole
/// already (`None`: not a guest map, or the bake is off)?
pub fn baked(zone_ff: &Path) -> Option<bool> {
    let game = game_of(zone_ff)?;
    let dir = map_bake::map_dir(game, &map_name(zone_ff))?;
    let key = pak_key(zone_ff);
    let on: Vec<&Pak> = PAKS.into_iter().filter(|p| p.enabled_for(game)).collect();
    (!on.is_empty()).then(|| on.iter().all(|p| p.open_pak(&dir, key).is_some()))
}

/// bo2mp (bake all): a load made now only to bake the map (no game, no
/// card): like [`begin`], but entries wait for the disk instead of giving
/// the bake up when it falls behind.
pub fn begin_ahead(load: u64, zone_ff: &Path) {
    begin(load, zone_ff);
    for pak in PAKS {
        if let Some(rec) = pak.active().as_mut().filter(|s| s.load == load).and_then(|s| s.recording.as_mut()) {
            rec.ahead = true;
        }
    }
}

/// bo2mp (bake all): the bake-only load `load` is over; waits for each
/// kind's file to be written (or thrown away) and gives its line.
pub fn finish_ahead(load: u64, ok: bool) -> Vec<String> {
    let writers: Vec<_> = PAKS.into_iter().filter_map(|pak| pak.finish(load, ok)).collect();
    writers.into_iter().filter_map(|w| w.join().ok()).collect()
}

/// The short name of a guest game's zone (`None`: Black Ops II, whose loads
/// are not baked, or a zone not recognised).
fn game_of(zone_ff: &Path) -> Option<&'static str> {
    use asset_core::ZoneGame;
    match asset_transport::zone_game_for_path(zone_ff) {
        Some(ZoneGame::Iw4) => Some("iw4"),
        Some(ZoneGame::T5) => Some("t5"),
        Some(ZoneGame::Iw5) => Some("iw5"),
        Some(_) => None,
        None => asset_transport::legacy_game_for_path(zone_ff),
    }
}

/// The key a map's pak is made under: the format and the map's zone file
/// (name, size, time written). Entries carry their own keys besides.
fn pak_key(zone_ff: &Path) -> u64 {
    let mut h = Fnv::new();
    h.bytes(&FORMAT.to_le_bytes());
    h.bytes(zone_ff.to_string_lossy().to_ascii_lowercase().as_bytes());
    if let Ok(meta) = std::fs::metadata(zone_ff) {
        h.bytes(&meta.len().to_le_bytes());
        let at = meta
            .modified()
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map_or(0, |d| d.as_nanos());
        h.bytes(&at.to_le_bytes());
    }
    h.0
}

struct Fnv(u64);

impl Fnv {
    fn new() -> Self {
        Self(0xcbf2_9ce4_8422_2325)
    }
    fn bytes(&mut self, b: &[u8]) {
        for x in b {
            self.0 ^= u64::from(*x);
            self.0 = self.0.wrapping_mul(0x0100_0000_01b3);
        }
    }
}

// ------------------------------------------------------------- the index

struct IndexEntry {
    key: String,
    /// The entry's bytes once unpacked.
    len: u64,
    /// Its bytes in the pak (the lz4 block).
    packed: u64,
}

/// An entry read from the pak and not taken yet.
enum Ready {
    /// As the file holds it, with its unpacked length.
    Packed { len: usize, block: Vec<u8> },
    /// Unpacked (a kept kind, after its first take).
    Plain(Vec<u8>),
}

impl Ready {
    /// Bytes held in memory.
    fn held(&self) -> usize {
        match self {
            Ready::Packed { block, .. } => block.len(),
            Ready::Plain(b) => b.len(),
        }
    }

    /// The entry's bytes; `None` when the block does not unpack to what
    /// the index says (the pak is not what was written).
    fn unpack(self) -> Option<Vec<u8>> {
        match self {
            Ready::Packed { len, block } => lz4_flex::block::decompress(&block, len).ok().filter(|b| b.len() == len),
            Ready::Plain(b) => Some(b),
        }
    }
}

fn encode_index(key: u64, entries: &[IndexEntry]) -> Vec<u8> {
    let mut out = Vec::with_capacity(32 + entries.len() * 64);
    out.extend_from_slice(IDX_MAGIC);
    out.extend_from_slice(&FORMAT.to_le_bytes());
    out.extend_from_slice(&key.to_le_bytes());
    out.extend_from_slice(&(entries.len() as u64).to_le_bytes());
    // (The total is the pak's length: the packed bytes.)
    let total: u64 = entries.iter().map(|e| e.packed).sum();
    out.extend_from_slice(&total.to_le_bytes());
    for e in entries {
        let k = e.key.as_bytes();
        out.extend_from_slice(&(k.len() as u16).to_le_bytes());
        out.extend_from_slice(k);
        out.extend_from_slice(&e.len.to_le_bytes());
        out.extend_from_slice(&e.packed.to_le_bytes());
    }
    out.extend_from_slice(END);
    out
}

/// The entries of an index made under `key`, and the pak bytes they add up
/// to; `None` when it is not whole or is another key's.
fn decode_index(b: &[u8], key: u64) -> Option<(Vec<IndexEntry>, u64)> {
    let mut at = 0usize;
    let mut take = |n: usize| -> Option<&[u8]> {
        let s = b.get(at..at.checked_add(n)?)?;
        at += n;
        Some(s)
    };
    if take(8)? != IDX_MAGIC {
        return None;
    }
    let u64_at = |s: &[u8]| -> Option<u64> { Some(u64::from_le_bytes(s.try_into().ok()?)) };
    let format = u32::from_le_bytes(take(4)?.try_into().ok()?);
    if format != FORMAT || u64_at(take(8)?)? != key {
        return None;
    }
    let count = usize::try_from(u64_at(take(8)?)?).ok()?;
    let total = u64_at(take(8)?)?;
    let mut entries = Vec::with_capacity(count.min(1 << 20));
    let mut sum = 0u64;
    for _ in 0..count {
        let n = usize::from(u16::from_le_bytes(take(2)?.try_into().ok()?));
        let k = String::from_utf8(take(n)?.to_vec()).ok()?;
        let len = u64_at(take(8)?)?;
        let packed = u64_at(take(8)?)?;
        sum = sum.checked_add(packed)?;
        entries.push(IndexEntry { key: k, len, packed });
    }
    (take(8)? == END && at == b.len() && sum == total).then_some((entries, total))
}

// ------------------------------------------------------------ the session

/// What a map load's reads of the pak share.
#[derive(Default)]
struct Reading {
    /// Every key in the pak, by its place in the file.
    place: HashMap<String, usize>,
    /// Entries read and not taken yet (or kept to be taken again).
    ready: HashMap<String, Ready>,
    ready_bytes: usize,
    /// How many entries the reader has gone past.
    cursor: usize,
    /// The reader waits for entries to be taken (or has stopped).
    stalled: bool,
    done: bool,
    closed: bool,
    read_bytes: u64,
    read_secs: f32,
}

struct Recording {
    tx: SyncSender<Msg>,
    seen: HashSet<String>,
    /// Bytes sent and not yet written (shared with the writer).
    behind: Arc<Mutex<usize>>,
    failed: bool,
    /// The loading screen's row, shared by every kind baking for this load.
    row: Arc<Row>,
    /// A bake-only load: entries wait for the disk rather than drop the bake.
    ahead: bool,
    writer: Option<std::thread::JoinHandle<String>>,
}

enum Msg {
    Entry(String, Vec<u8>),
    Done(bool),
}

/// One loading-screen row for a load's bakes ("Black Ops - Nuked -
/// pictures + programs 412"): one job however many kinds are baking, so
/// the row does not flip between them, and Cancel stops them all.
pub struct Row {
    game: &'static str,
    map: String,
    /// The kinds baking (step name, why), in the order they started.
    writers: Mutex<Vec<(&'static str, String)>>,
    job: std::sync::OnceLock<Arc<map_bake::status::Job>>,
    live: std::sync::atomic::AtomicUsize,
    kept: std::sync::atomic::AtomicBool,
}

impl Row {
    fn new(game: &'static str, map: &str) -> Arc<Self> {
        Arc::new(Self {
            game,
            map: map.to_owned(),
            writers: Mutex::new(Vec::new()),
            job: std::sync::OnceLock::new(),
            live: std::sync::atomic::AtomicUsize::new(0),
            kept: std::sync::atomic::AtomicBool::new(true),
        })
    }

    fn add_writer(&self, step: &'static str, why: String) {
        self.live.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        self.writers.lock().unwrap_or_else(std::sync::PoisonError::into_inner).push((step, why));
    }

    /// The row, opened by the first entry any kind records.
    fn job(&self) -> &Arc<map_bake::status::Job> {
        self.job.get_or_init(|| {
            let writers = self.writers.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
            let steps: Vec<&str> = writers.iter().map(|(s, _)| *s).collect();
            let mut whys: Vec<&str> = Vec::new();
            for (_, w) in writers.iter() {
                if !whys.contains(&w.as_str()) {
                    whys.push(w);
                }
            }
            let job = map_bake::status::begin(self.game, &self.map, &steps.join(" + "), 0);
            job.set_why(&whys.join(", "));
            job
        })
    }

    fn canceled(&self) -> bool {
        self.job.get().is_some_and(|j| j.is_canceled())
    }

    /// One kind's writer is over (`None`: it had nothing to bake). The last
    /// one closes the row: kept only if every kind that baked kept its file.
    fn writer_done(&self, kept: Option<bool>) {
        use std::sync::atomic::Ordering;
        if kept == Some(false) {
            self.kept.store(false, Ordering::SeqCst);
        }
        if self.live.fetch_sub(1, Ordering::SeqCst) == 1
            && let Some(job) = self.job.get()
        {
            job.end(self.kept.load(Ordering::SeqCst));
        }
    }
}

struct Session {
    load: u64,
    map: String,
    dir: PathBuf,
    reading: Option<Arc<(Mutex<Reading>, Condvar)>>,
    recording: Option<Recording>,
    served: u64,
    missed: u64,
    waited: Duration,
    started: Instant,
}

impl Pak {
    const fn new(name: &'static str, switch: &'static str, keep_taken: bool) -> Self {
        Self { name, switch, keep_taken, active: Mutex::new(None) }
    }

    /// Whether this bake is on (`CODLEGACY_BAKE=0` or its own switch `=0`: no).
    pub fn enabled(&self) -> bool {
        map_bake::enabled() && !codlegacy_env::var(self.switch).is_ok_and(|v| v.trim() == "0")
    }

    /// On for a map of `game`. CoD4 and WaW maps make no shader programs
    /// (all 44 baked offline on 2026-11-06 made 0), so a programs bake there
    /// was never kept and the map counted as "not baked" on every load.
    /// `CODLEGACY_BAKE_LEGACY_PROGRAMS=1` bakes their programs again.
    fn enabled_for(&self, game: &str) -> bool {
        let legacy = matches!(game, "iw3" | "t4");
        self.enabled()
            && !(legacy
                && std::ptr::eq(self, &PROGRAMS)
                && !codlegacy_env::var("CODLEGACY_BAKE_LEGACY_PROGRAMS").is_ok_and(|v| v.trim() == "1"))
    }

    fn active(&self) -> MutexGuard<'_, Option<Session>> {
        self.active.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    fn pak_file(&self) -> String {
        format!("{}.pak", self.name)
    }

    fn idx_file(&self) -> String {
        format!("{}.idx", self.name)
    }

    fn begin(&self, load: u64, zone_ff: &Path, row: Option<&Arc<Row>>) {
        drop(self.end_session(self.active().take(), false));
        let Some(game) = game_of(zone_ff) else { return };
        if !self.enabled_for(game) {
            return;
        }
        let map = map_name(zone_ff);
        let Some(dir) = map_bake::map_dir(game, &map) else { return };
        let key = pak_key(zone_ff);
        let mut session = Session {
            load,
            map: format!("{game}:{map}"),
            dir: dir.clone(),
            reading: None,
            recording: None,
            served: 0,
            missed: 0,
            waited: Duration::ZERO,
            started: Instant::now(),
        };
        match self.open_pak(&dir, key) {
            Some((entries, file)) => {
                let shared = Arc::new((
                    Mutex::new(Reading {
                        place: entries.iter().enumerate().map(|(i, e)| (e.key.clone(), i)).collect(),
                        ..Default::default()
                    }),
                    Condvar::new(),
                ));
                let reader = Arc::clone(&shared);
                let spawned = std::thread::Builder::new()
                    .name(format!("bake_{}_read", self.name))
                    .spawn(move || read_pak(file, entries, &reader));
                if spawned.is_ok() {
                    diag::info!(
                        World,
                        "bake {}: {} reads {}",
                        self.name,
                        session.map,
                        dir.join(self.pak_file()).display()
                    );
                    session.reading = Some(shared);
                }
            }
            None => {
                let row = row.map_or_else(|| Row::new(game, &map), Arc::clone);
                session.recording = self.start_writer(&dir, key, &session.map, row);
            }
        }
        *self.active() = Some(session);
    }

    /// Ends load `load`'s session; its bake's writer, if it was baking.
    fn finish(&self, load: u64, ok: bool) -> Option<std::thread::JoinHandle<String>> {
        let mut slot = self.active();
        if slot.as_ref().is_some_and(|s| s.load == load) {
            let s = slot.take();
            drop(slot);
            return self.end_session(s, ok);
        }
        None
    }

    fn end_session(&self, session: Option<Session>, ok: bool) -> Option<std::thread::JoinHandle<String>> {
        let mut s = session?;
        let mut line = format!(
            "bake {}: {} done in {:.1}s, {} entries from the pak, {} not, waited {:.2}s",
            self.name,
            s.map,
            s.started.elapsed().as_secs_f32(),
            s.served,
            s.missed,
            s.waited.as_secs_f32()
        );
        if let Some(shared) = s.reading.take() {
            let (lock, signal) = &*shared;
            let mut r = lock.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
            r.closed = true;
            r.ready.clear();
            r.ready_bytes = 0;
            line.push_str(&format!(
                "; pak read {:.0} MB in {:.1}s{}",
                r.read_bytes as f64 / 1_048_576.0,
                r.read_secs,
                if r.done { "" } else { " (not to the end)" }
            ));
            drop(r);
            signal.notify_all();
            // (Too much of the load came from elsewhere: the entries changed
            // or the load asks for others now. Bake it again next time.)
            if ok && s.missed > 32.max(s.served / 20) {
                let _ = std::fs::remove_file(s.dir.join(self.idx_file()));
                line.push_str("; too many misses, it is baked again on the next load");
            }
        }
        let mut writer = None;
        if let Some(mut rec) = s.recording.take() {
            writer = rec.writer.take();
            // (A cancelled load's bake is thrown away; the next load bakes again.)
            let keep = ok && !rec.failed && !rec.row.canceled() && !rec.seen.is_empty();
            let _ = rec.tx.send(Msg::Done(keep));
            line.push_str(&format!(
                "; baking {} entries: {}",
                rec.seen.len(),
                if keep { "kept" } else { "dropped" }
            ));
        }
        diag::info!(World, "{line}");
        writer
    }

    /// Why the pak in `dir` is made again, in `map_bake::why`'s words (the
    /// key is the pak format plus the game file's name, size and date).
    fn why(&self, dir: &Path, key: u64) -> String {
        let Ok(index) = std::fs::read(dir.join(self.idx_file())) else {
            return "first load".into();
        };
        let header = |at: usize, n: usize| index.get(at..at + n);
        let format = header(8, 4).and_then(|b| b.try_into().ok()).map(u32::from_le_bytes);
        let old_key = header(12, 8).and_then(|b| b.try_into().ok()).map(u64::from_le_bytes);
        if header(0, 8) != Some(IDX_MAGIC.as_slice()) || format != Some(FORMAT) {
            "made by an older version".into()
        } else if old_key != Some(key) {
            "game files changed".into()
        } else {
            "the last copy did not read back whole".into()
        }
    }

    fn open_pak(&self, dir: &Path, key: u64) -> Option<(Vec<IndexEntry>, File)> {
        let index = std::fs::read(dir.join(self.idx_file())).ok()?;
        let (entries, total) = decode_index(&index, key)?;
        let file = File::open(dir.join(self.pak_file())).ok()?;
        (file.metadata().ok()?.len() == total).then_some((entries, file))
    }

    /// The entry under cache `key` from the map's pak, if it has it (waits
    /// while the reader is on its way to it).
    pub fn take(&self, key: &str) -> Option<Vec<u8>> {
        let shared = {
            let slot = self.active();
            Arc::clone(slot.as_ref()?.reading.as_ref()?)
        };
        let t0 = Instant::now();
        let (lock, signal) = &*shared;
        let mut r = lock.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let got = loop {
            if self.keep_taken {
                // (Unpacked on the first take, kept unpacked: the kept kinds
                // are small.)
                if let Some(entry) = r.ready.remove(key) {
                    r.ready_bytes -= entry.held();
                    let Some(data) = entry.unpack() else {
                        diag::warn!(World, "bake {}: {key} did not unpack, read from the cache instead", self.name);
                        break None;
                    };
                    r.ready_bytes += data.len();
                    r.ready.insert(key.to_owned(), Ready::Plain(data.clone()));
                    break Some(Ready::Plain(data));
                }
            } else if let Some(entry) = r.ready.remove(key) {
                r.ready_bytes -= entry.held();
                signal.notify_all();
                break Some(entry);
            }
            let ahead = r.place.get(key).is_some_and(|&at| at >= r.cursor);
            if !ahead || r.stalled || r.done || r.closed {
                break None;
            }
            r = signal
                .wait_timeout(r, Duration::from_millis(200))
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .0;
        };
        drop(r);
        // (Unpacked by the taking thread, outside the lock: the reader stays
        // on the disk and the loading threads share the unpacking.)
        let got = got.and_then(|entry| {
            let data = entry.unpack();
            if data.is_none() {
                diag::warn!(World, "bake {}: {key} did not unpack, read from the cache instead", self.name);
            }
            data
        });
        if let Some(s) = self.active().as_mut() {
            s.waited += t0.elapsed();
            if got.is_some() {
                s.served += 1;
            } else {
                s.missed += 1;
            }
        }
        got
    }

    /// An entry this load used (from the cache, or just made), for the bake
    /// being made; dropped when none is.
    pub fn record(&self, key: &str, entry: Vec<u8>) {
        let mut slot = self.active();
        let Some(rec) = slot.as_mut().and_then(|s| s.recording.as_mut()) else { return };
        if rec.failed || rec.row.canceled() || !rec.seen.insert(key.to_owned()) {
            return;
        }
        let len = entry.len();
        {
            let mut behind = rec.behind.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
            // (Baking ahead, nothing waits on the load: the disk sets the pace.)
            while rec.ahead && *behind > 0 && *behind + len > WRITE_BEHIND {
                drop(behind);
                std::thread::sleep(Duration::from_millis(5));
                behind = rec.behind.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
            }
            if *behind + len > WRITE_BEHIND {
                rec.failed = true;
                diag::warn!(World, "bake {}: the disk fell behind, this load's bake is given up", self.name);
                return;
            }
            *behind += len;
        }
        match rec.tx.try_send(Msg::Entry(key.to_owned(), entry)) {
            Ok(()) => rec.row.job().add(1),
            Err(TrySendError::Full(_) | TrySendError::Disconnected(_)) => {
                rec.failed = true;
                let mut behind = rec.behind.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
                *behind = behind.saturating_sub(len);
            }
        }
    }

    fn start_writer(&self, dir: &Path, key: u64, map: &str, row: Arc<Row>) -> Option<Recording> {
        let map = map.to_owned();
        let why = self.why(dir, key);
        std::fs::create_dir_all(dir).ok()?;
        let prefix = format!("{}.", self.name);
        for e in std::fs::read_dir(dir).ok()?.flatten() {
            let file = e.file_name().to_string_lossy().into_owned();
            if file.starts_with(&prefix) && file.contains(".part") {
                let _ = std::fs::remove_file(e.path());
            }
        }
        let tmp = dir.join(format!("{}.part{}", self.pak_file(), std::process::id()));
        let file = File::create(&tmp).ok()?;
        // (Entries are counted in bytes by `behind`; the channel only has to
        // hold more of them than that ever allows.)
        let (tx, rx) = std::sync::mpsc::sync_channel::<Msg>(1 << 16);
        let behind = Arc::new(Mutex::new(0usize));
        let writer_behind = Arc::clone(&behind);
        let dir = dir.to_owned();
        let name = self.name;
        let (pak, idx) = (self.pak_file(), self.idx_file());
        let writer_row = Arc::clone(&row);
        let writer = std::thread::Builder::new()
            .name(format!("bake_{name}_write"))
            .spawn(move || {
                let (done, wrote) = write_pak(file, &tmp, &dir.join(pak), &dir.join(idx), key, &rx, &writer_behind);
                writer_row.writer_done((wrote > 0).then_some(done.is_ok()));
                let line = match done {
                    Ok(line) => format!("bake {name}: {line}"),
                    Err(e) => {
                        let _ = std::fs::remove_file(&tmp);
                        format!("bake {name}: {map} not kept ({e})")
                    }
                };
                diag::info!(World, "{line}");
                line
            })
            .ok()?;
        row.add_writer(if name == "mips" { "pictures" } else { name }, why);
        Some(Recording { tx, seen: HashSet::new(), behind, failed: false, row, ahead: false, writer: Some(writer) })
    }
}

fn read_pak(file: File, entries: Vec<IndexEntry>, shared: &(Mutex<Reading>, Condvar)) {
    let (lock, signal) = shared;
    let t0 = Instant::now();
    let mut f = BufReader::with_capacity(BUF, file);
    for (i, e) in entries.into_iter().enumerate() {
        {
            let mut r = lock.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
            while !r.closed && r.ready_bytes > READ_AHEAD {
                r.stalled = true;
                signal.notify_all();
                r = signal.wait(r).unwrap_or_else(std::sync::PoisonError::into_inner);
            }
            r.stalled = false;
            if r.closed {
                return;
            }
        }
        let (Ok(len), Ok(packed)) = (usize::try_from(e.len), usize::try_from(e.packed)) else { break };
        let mut block = vec![0u8; packed];
        if f.read_exact(&mut block).is_err() {
            break;
        }
        let mut r = lock.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        if r.closed {
            return;
        }
        r.cursor = i + 1;
        r.read_bytes += e.packed;
        r.read_secs = t0.elapsed().as_secs_f32();
        r.ready_bytes += packed;
        r.ready.insert(e.key, Ready::Packed { len, block });
        drop(r);
        signal.notify_all();
    }
    let mut r = lock.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    r.done = true;
    r.stalled = true;
    r.read_secs = t0.elapsed().as_secs_f32();
    drop(r);
    signal.notify_all();
}

fn write_pak(
    file: File,
    tmp: &Path,
    pak: &Path,
    idx: &Path,
    key: u64,
    rx: &Receiver<Msg>,
    behind: &Mutex<usize>,
) -> (Result<String, String>, usize) {
    let t0 = Instant::now();
    let mut w = BufWriter::with_capacity(BUF, file);
    let mut entries = Vec::new();
    loop {
        match rx.recv() {
            Ok(Msg::Entry(k, data)) => {
                // (Packed here, on the writer's thread, not the load's.)
                let block = lz4_flex::block::compress(&data);
                let wrote = w.write_all(&block);
                *behind.lock().unwrap_or_else(std::sync::PoisonError::into_inner) -= data.len();
                if let Err(e) = wrote {
                    return (Err(e.to_string()), entries.len() + 1);
                }
                entries.push(IndexEntry { key: k, len: data.len() as u64, packed: block.len() as u64 });
            }
            Ok(Msg::Done(keep)) => {
                let done = if keep {
                    finish_pak(w, tmp, pak, idx, key, &entries, t0)
                } else {
                    Err("the load did not finish, was cancelled, or the disk fell behind".into())
                };
                return (done, entries.len());
            }
            Err(_) => return (Err("the load ended before the bake did".into()), entries.len()),
        }
    }
}

/// The bake is whole: the pak goes in place, then its index.
fn finish_pak(
    w: BufWriter<File>,
    tmp: &Path,
    pak: &Path,
    idx: &Path,
    key: u64,
    entries: &[IndexEntry],
    t0: Instant,
) -> Result<String, String> {
    let io = |e: std::io::Error| e.to_string();
    let f = w.into_inner().map_err(|e| e.error().to_string())?;
    f.sync_all().map_err(io)?;
    drop(f);
    // (Index last: a pak whose index is missing or another key's is a miss.)
    let _ = std::fs::remove_file(idx);
    let _ = std::fs::remove_file(pak);
    std::fs::rename(tmp, pak).map_err(io)?;
    let mut idx_tmp = idx.as_os_str().to_owned();
    idx_tmp.push(format!(".part{}", std::process::id()));
    let idx_tmp = PathBuf::from(idx_tmp);
    std::fs::write(&idx_tmp, encode_index(key, entries)).map_err(io)?;
    std::fs::rename(&idx_tmp, idx).map_err(io)?;
    let bytes: u64 = entries.iter().map(|e| e.len).sum();
    let packed: u64 = entries.iter().map(|e| e.packed).sum();
    Ok(format!(
        "{} entries ({:.0} MB, {:.0} MB packed) kept in {} ({:.1}s)",
        entries.len(),
        bytes as f64 / 1_048_576.0,
        packed as f64 / 1_048_576.0,
        pak.display(),
        t0.elapsed().as_secs_f32()
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cod4_and_waw_maps_bake_no_programs() {
        if codlegacy_env::var("CODLEGACY_BAKE_LEGACY_PROGRAMS").is_ok() {
            return;
        }
        assert!(!PROGRAMS.enabled_for("iw3") && !PROGRAMS.enabled_for("t4"));
        assert_eq!(PROGRAMS.enabled_for("t5"), PROGRAMS.enabled());
        assert_eq!(MIPS.enabled_for("iw3"), MIPS.enabled());
    }

    #[test]
    fn index_round_trips_and_rejects_other_keys() {
        let entries = vec![
            IndexEntry { key: "00000002-aa-10-ff".into(), len: 10, packed: 8 },
            IndexEntry { key: "00000002-bb-20-ee".into(), len: 0, packed: 1 },
            IndexEntry { key: "00000002-cc-30-dd".into(), len: 7, packed: 8 },
        ];
        let b = encode_index(42, &entries);
        let (back, total) = decode_index(&b, 42).expect("whole index");
        assert_eq!(total, 17, "the pak's length is the packed bytes");
        assert_eq!(back.len(), 3);
        assert_eq!(back[2].key, "00000002-cc-30-dd");
        assert_eq!((back[2].len, back[2].packed), (7, 8));
        assert!(decode_index(&b, 43).is_none());
        assert!(decode_index(&b[..b.len() - 1], 42).is_none());
    }

    fn session(load: u64, dir: &Path) -> Session {
        Session {
            load,
            map: "test:map".into(),
            dir: dir.to_owned(),
            reading: None,
            recording: None,
            served: 0,
            missed: 0,
            waited: Duration::ZERO,
            started: Instant::now(),
        }
    }

    /// Bakes `entries` into a test pak under `dir`, then serves them back.
    fn bake_and_serve(pak: &Pak, dir: &Path, entries: &[(String, Vec<u8>)]) -> Arc<(Mutex<Reading>, Condvar)> {
        let _ = std::fs::remove_dir_all(dir);
        let mut s = session(1, dir);
        s.recording = Some(pak.start_writer(dir, 7, "test:map", Row::new("test", "map")).expect("writer"));
        *pak.active() = Some(s);
        for (k, c) in entries {
            pak.record(k, c.clone());
            pak.record(k, c.clone()); // (a second use is not written twice)
        }
        pak.finish(1, true);
        // (The writer is on its own thread: wait for the index.)
        let t0 = Instant::now();
        while !dir.join(pak.idx_file()).exists() && t0.elapsed() < Duration::from_secs(10) {
            std::thread::sleep(Duration::from_millis(20));
        }
        let (index, file) = pak.open_pak(dir, 7).expect("whole pak");
        assert_eq!(index.len(), entries.len());
        let shared = Arc::new((
            Mutex::new(Reading {
                place: index.iter().enumerate().map(|(i, e)| (e.key.clone(), i)).collect(),
                ..Default::default()
            }),
            Condvar::new(),
        ));
        let reader = Arc::clone(&shared);
        std::thread::spawn(move || read_pak(file, index, &reader));
        let mut s = session(2, dir);
        s.reading = Some(Arc::clone(&shared));
        *pak.active() = Some(s);
        shared
    }

    #[test]
    fn a_pak_is_written_and_served_back_in_order() {
        static PAK: Pak = Pak::new("test_once", "CODLEGACY_BAKE_TEST_ONCE", false);
        let dir = std::env::temp_dir().join(format!("bake_once_test_{}", std::process::id()));
        let chains: Vec<(String, Vec<u8>)> =
            (0..50u8).map(|i| (format!("k{i}"), vec![i; usize::from(i) * 3 + 1])).collect();
        bake_and_serve(&PAK, &dir, &chains);
        for (k, c) in chains.iter().rev() {
            assert_eq!(PAK.take(k).as_deref(), Some(c.as_slice()), "{k}");
        }
        assert!(PAK.take("k0").is_none(), "a chain is handed out once");
        assert!(PAK.take("not-there").is_none());
        PAK.finish(2, true);
        assert!(dir.join("test_once.idx").exists(), "few misses: the pak stays");
        // (Packed: runs of one byte are far smaller on disk.)
        let raw: u64 = chains.iter().map(|(_, c)| c.len() as u64).sum();
        let on_disk = std::fs::metadata(dir.join("test_once.pak")).expect("pak").len();
        assert!(on_disk < raw, "the pak is packed ({on_disk} of {raw} bytes)");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_kept_entry_is_served_again() {
        static PAK: Pak = Pak::new("test_keep", "CODLEGACY_BAKE_TEST_KEEP", true);
        let dir = std::env::temp_dir().join(format!("bake_keep_test_{}", std::process::id()));
        let programs: Vec<(String, Vec<u8>)> =
            (0..20u8).map(|i| (format!("{i:016x}"), vec![i; 40])).collect();
        bake_and_serve(&PAK, &dir, &programs);
        for _ in 0..3 {
            for (k, c) in &programs {
                assert_eq!(PAK.take(k).as_deref(), Some(c.as_slice()), "{k}");
            }
        }
        PAK.finish(2, true);
        assert!(dir.join("test_keep.idx").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }
    #[test]
    fn the_row_says_why_a_pak_is_made_again() {
        static PAK: Pak = Pak::new("test_why", "CODLEGACY_BAKE_TEST_WHY", false);
        let dir = std::env::temp_dir().join(format!("bake_why_test_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("dir");
        assert_eq!(PAK.why(&dir, 7), "first load");
        std::fs::write(dir.join("test_why.idx"), encode_index(7, &[])).expect("idx");
        assert_eq!(PAK.why(&dir, 8), "game files changed");
        assert_eq!(PAK.why(&dir, 7), "the last copy did not read back whole");
        std::fs::write(dir.join("test_why.idx"), b"old").expect("idx");
        assert_eq!(PAK.why(&dir, 7), "made by an older version");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn two_kinds_share_one_row_that_ends_once() {
        static A: Pak = Pak::new("test_row_a", "CODLEGACY_BAKE_TEST_ROW_A", false);
        static B: Pak = Pak::new("test_row_b", "CODLEGACY_BAKE_TEST_ROW_B", false);
        let dir = std::env::temp_dir().join(format!("bake_row_test_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let row = Row::new("test", "map");
        for pak in [&A, &B] {
            let mut s = session(1, &dir);
            s.recording = Some(pak.start_writer(&dir, 7, "test:map", Arc::clone(&row)).expect("writer"));
            *pak.active() = Some(s);
        }
        A.record("a", vec![1; 10]);
        B.record("b", vec![2; 10]);
        B.record("c", vec![3; 10]);
        let job = row.job.get().cloned().expect("one row");
        assert_eq!(job.counts().0, 3);
        A.finish(1, true);
        std::thread::sleep(Duration::from_millis(200));
        assert_eq!(job.outcome(), None, "the row stays open while a kind is still baking");
        B.finish(1, true);
        let t0 = Instant::now();
        while job.outcome().is_none() && t0.elapsed() < Duration::from_secs(10) {
            std::thread::sleep(Duration::from_millis(20));
        }
        assert_eq!(job.outcome(), Some(true));
        assert!(dir.join("test_row_a.idx").exists() && dir.join("test_row_b.idx").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_cancelled_bake_is_thrown_away() {
        static PAK: Pak = Pak::new("test_cancel", "CODLEGACY_BAKE_TEST_CANCEL", false);
        let dir = std::env::temp_dir().join(format!("bake_cancel_test_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let mut s = session(1, &dir);
        s.recording = Some(PAK.start_writer(&dir, 7, "test:map", Row::new("test", "map")).expect("writer"));
        *PAK.active() = Some(s);
        PAK.record("a", vec![1; 10]);
        let job = PAK
            .active()
            .as_ref()
            .and_then(|s| s.recording.as_ref())
            .and_then(|r| r.row.job.get().cloned())
            .expect("the first entry opens the row");
        assert_eq!(job.counts().0, 1);
        job.cancel();
        PAK.record("b", vec![2; 10]);
        assert_eq!(job.counts().0, 1, "nothing is added after a cancel");
        PAK.finish(1, true);
        let t0 = Instant::now();
        while job.outcome().is_none() && t0.elapsed() < Duration::from_secs(10) {
            std::thread::sleep(Duration::from_millis(20));
        }
        assert_eq!(job.outcome(), Some(false));
        assert!(!dir.join("test_cancel.idx").exists());
        assert!(!dir.join("test_cancel.pak").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
