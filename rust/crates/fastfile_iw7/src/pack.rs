//! Range packs: the `.pak` ranges a map reads (its shared streams, its
//! pictures at the size drawn, the common zones' pictures it names),
//! recorded while the game loads it and written beside nothing else in a
//! few chunk files, so the map can be read again where the multi-gigabyte
//! paks are not (an offline machine): [`crate::pak::read_range`] looks a
//! missing pak's range up in the packs of the pak's folder.
//!
//! On disk, per map: `iw7pack_<map>.idx` (text: one line per range,
//! `<pak file name> <offset> <end> <chunk> <position>`) and
//! `iw7pack_<map>.<chunk>` (the ranges end to end, each chunk at most
//! [`CHUNK_LIMIT`] bytes).

use crate::container::StreamFile;
use std::collections::HashMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

/// The largest chunk file.
pub const CHUNK_LIMIT: u64 = 350 << 20;

type Key = (String, u64, u64);

struct Recorder {
    seen: std::collections::HashSet<Key>,
    tx: std::sync::mpsc::Sender<(Key, Vec<u8>)>,
}

static RECORDER: Mutex<Option<Recorder>> = Mutex::new(None);

/// Starts recording every pak range read into `dir`'s pack for `map`
/// (written on a thread as the ranges come). Nothing when that pack exists.
/// Returns whether it started.
pub fn start_recording(dir: &Path, map: &str) -> bool {
    let idx = dir.join(format!("iw7pack_{map}.idx"));
    if idx.is_file() || std::fs::create_dir_all(dir).is_err() {
        return false;
    }
    let (tx, rx) = std::sync::mpsc::channel::<(Key, Vec<u8>)>();
    let (dir, map) = (dir.to_owned(), map.to_owned());
    std::thread::Builder::new()
        .name("iw7 pack writer".into())
        .spawn(move || {
            let _ = write_pack(&dir, &map, &rx);
        })
        .ok();
    if let Ok(mut r) = RECORDER.lock() {
        *r = Some(Recorder { seen: Default::default(), tx });
    }
    true
}

/// Stops recording (the writer finishes and writes the index).
pub fn stop_recording() {
    if let Ok(mut r) = RECORDER.lock() {
        *r = None;
    }
}

fn write_pack(dir: &Path, map: &str, rx: &std::sync::mpsc::Receiver<(Key, Vec<u8>)>) -> std::io::Result<()> {
    let (mut chunk, mut pos) = (0u32, 0u64);
    let open = |k: u32| std::fs::File::create(dir.join(format!("iw7pack_{map}.{k}"))).map(std::io::BufWriter::new);
    let mut out = open(chunk)?;
    let mut index = String::new();
    for ((pak, offset, end), bytes) in rx {
        if pos > 0 && pos + bytes.len() as u64 > CHUNK_LIMIT {
            out.flush()?;
            chunk += 1;
            pos = 0;
            out = open(chunk)?;
        }
        out.write_all(&bytes)?;
        index.push_str(&format!("{pak} {offset} {end} {chunk} {pos}\n"));
        pos += bytes.len() as u64;
    }
    out.flush()?;
    // (The index last: a pack with an index is whole.)
    let tmp = dir.join(format!("iw7pack_{map}.idx.part"));
    std::fs::write(&tmp, index)?;
    std::fs::rename(tmp, dir.join(format!("iw7pack_{map}.idx")))
}

/// Records a range just read (when recording).
pub(crate) fn record(path: &Path, f: &StreamFile, bytes: &[u8]) {
    let Ok(mut g) = RECORDER.lock() else { return };
    let Some(r) = g.as_mut() else { return };
    let name = path.file_name().map_or_else(String::new, |n| n.to_string_lossy().into_owned());
    let key = (name, f.offset, f.offset_end);
    if r.seen.insert(key.clone()) {
        let _ = r.tx.send((key, bytes.to_vec()));
    }
}

/// The packs of a folder: range -> (chunk file, position).
type Index = HashMap<Key, (PathBuf, u64)>;

fn index_of(dir: &Path) -> &'static Index {
    static INDEXES: OnceLock<Mutex<HashMap<PathBuf, &'static Index>>> = OnceLock::new();
    let m = INDEXES.get_or_init(Default::default);
    let mut g = m.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    if let Some(i) = g.get(dir) {
        return i;
    }
    let mut index = Index::new();
    for e in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let p = e.path();
        let Some(name) = p.file_name().map(|n| n.to_string_lossy().into_owned()) else { continue };
        let Some(map) = name.strip_prefix("iw7pack_").and_then(|n| n.strip_suffix(".idx")) else { continue };
        let Ok(text) = std::fs::read_to_string(&p) else { continue };
        for line in text.lines() {
            let v: Vec<&str> = line.split(' ').collect();
            if let [pak, o, e, c, at] = v[..]
                && let (Ok(o), Ok(e), Ok(at)) = (o.parse(), e.parse(), at.parse())
            {
                index.entry((pak.to_owned(), o, e)).or_insert((dir.join(format!("iw7pack_{map}.{c}")), at));
            }
        }
    }
    let leaked: &'static Index = Box::leak(Box::new(index));
    g.insert(dir.to_owned(), leaked);
    leaked
}

/// A range from the packs beside `path` (a pak that isn't there).
pub(crate) fn lookup(path: &Path, f: &StreamFile) -> Option<Vec<u8>> {
    use std::io::{Read, Seek, SeekFrom};
    let dir = path.parent()?;
    let name = path.file_name()?.to_string_lossy().into_owned();
    let (chunk, at) = index_of(dir).get(&(name, f.offset, f.offset_end))?;
    let mut file = std::fs::File::open(chunk).ok()?;
    file.seek(SeekFrom::Start(*at)).ok()?;
    let mut buf = vec![0u8; usize::try_from(f.offset_end - f.offset).ok()?];
    file.read_exact(&mut buf).ok()?;
    Some(buf)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_recorded_range_reads_back_from_the_pack() {
        let dir = std::env::temp_dir().join(format!("iw7pack_test_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let pak = dir.join("imagefile7.pak");
        let f = StreamFile { offset: 100, offset_end: 104, file_index: 7, localized: false };
        assert!(start_recording(&dir, "mp_test"));
        record(&pak, &f, &[1, 2, 3, 4]);
        record(&pak, &f, &[1, 2, 3, 4]);
        stop_recording();
        let idx = dir.join("iw7pack_mp_test.idx");
        for _ in 0..200 {
            if idx.is_file() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert_eq!(lookup(&pak, &f), Some(vec![1, 2, 3, 4]));
        // (Already there: not again.)
        assert!(!start_recording(&dir, "mp_test"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
