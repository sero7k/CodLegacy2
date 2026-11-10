//! bo2mp (load speed and memory): which names each shared zone has, kept
//! on disk, so a scene read opens only the shared zones that have what it
//! looks for. Without it a material, technique set or model the map zone
//! holds only as a reference opens the shared zones one at a time, whole,
//! until one has it, and a name none of them has opens every one
//! (`core_frontend`, `core_ui`, the patches: a 3.8 GB peak on Skyjacked).
//! A zone's names are written the first time it is opened; after that a
//! lookup the index says a zone cannot answer skips that zone (a name no
//! zone has opens nothing), and one it can answer opens that zone only.
//! One file per zone, `<cache dir>/t7_zone_index/<zone>.idx`: 8 bytes of
//! checksum, then a stamp line (the zone's and the program's size and
//! time: a new zone or a new build reads its zones again), then one name a
//! line, its kind first. Read once per process (`pipecache::cache_dir`).
//! `BO2MP_T7_ZONE_INDEX=0` turns it off.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};

const VERSION: &str = "t7zi 1";

/// What a name is looked up as.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Kind {
    /// A material with a technique set (by its name as looked up).
    Material,
    /// A technique set with a technique or programs (without its `,`).
    Techset,
    /// A technique read whole: `<slot>:<technique set>`.
    Technique,
    /// A model with a mesh that has data (without its `,`).
    Model,
}

impl Kind {
    const ALL: [Kind; 4] = [Kind::Material, Kind::Techset, Kind::Technique, Kind::Model];
    fn tag(self) -> char {
        match self {
            Kind::Material => 'm',
            Kind::Techset => 's',
            Kind::Technique => 't',
            Kind::Model => 'x',
        }
    }
}

/// The names one zone answers.
#[derive(Default, Debug, PartialEq)]
pub struct Names(HashSet<(Kind, String)>);

impl Names {
    pub fn add(&mut self, kind: Kind, name: String) {
        self.0.insert((kind, name));
    }
    pub fn has(&self, kind: Kind, name: &str) -> bool {
        self.0.contains(&(kind, name.to_owned()))
    }
    pub fn len(&self) -> usize {
        self.0.len()
    }
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

pub fn on() -> bool {
    static ON: OnceLock<bool> = OnceLock::new();
    *ON.get_or_init(|| std::env::var("BO2MP_T7_ZONE_INDEX").map_or(true, |v| v.trim() != "0"))
}

/// A file's size and time, for the stamp.
fn file_stamp(path: &Path) -> Option<String> {
    let m = std::fs::metadata(path).ok()?;
    let t = m.modified().ok()?.duration_since(std::time::UNIX_EPOCH).ok()?;
    Some(format!("{} {}", m.len(), t.as_nanos()))
}

/// The stamp of the zone at `zone`: it and the program that reads it.
fn stamp(zone: &Path) -> Option<String> {
    let exe = std::env::current_exe().ok().and_then(|e| file_stamp(&e))?;
    Some(format!("{VERSION} {} {exe}", file_stamp(zone)?))
}

fn encode(stamp: &str, names: &Names) -> Option<Vec<u8>> {
    let mut lines: Vec<String> = names.0.iter().map(|(k, n)| format!("{}{n}", k.tag())).collect();
    // (A name with a line break in it cannot be written: no file, so the
    // zone is opened as before.)
    if lines.iter().any(|l| l.contains(['\n', '\r'])) || stamp.contains(['\n', '\r']) {
        return None;
    }
    lines.sort();
    let mut body = stamp.to_owned();
    for l in lines {
        body.push('\n');
        body.push_str(&l);
    }
    let mut raw = crate::pipecache::checksum(body.as_bytes()).to_le_bytes().to_vec();
    raw.extend_from_slice(body.as_bytes());
    Some(raw)
}

/// The names in a file's bytes (None: damaged or from another stamp).
fn decode(raw: &[u8], stamp: &str) -> Option<Names> {
    let (head, body) = raw.split_at_checked(8)?;
    if u64::from_le_bytes(head.try_into().ok()?) != crate::pipecache::checksum(body) {
        return None;
    }
    let body = std::str::from_utf8(body).ok()?;
    let mut lines = body.split('\n');
    if lines.next()? != stamp {
        return None;
    }
    let mut names = Names::default();
    for l in lines {
        let mut c = l.chars();
        let tag = c.next()?;
        let kind = Kind::ALL.into_iter().find(|k| k.tag() == tag)?;
        names.add(kind, c.as_str().to_owned());
    }
    Some(names)
}

fn file_for(dir: &Path, zone: &Path) -> Option<PathBuf> {
    Some(dir.join("t7_zone_index").join(format!("{}.idx", zone.file_stem()?.to_string_lossy())))
}

fn save(file: &Path, stamp: &str, names: &Names) -> bool {
    let Some(raw) = encode(stamp, names) else { return false };
    if let Some(d) = file.parent() {
        let _ = std::fs::create_dir_all(d);
    }
    let tmp = file.with_extension("tmp");
    if std::fs::write(&tmp, raw).is_ok() && std::fs::rename(&tmp, file).is_ok() {
        return true;
    }
    let _ = std::fs::remove_file(&tmp);
    false
}

fn load(file: &Path, stamp: &str) -> Option<Names> {
    decode(&std::fs::read(file).ok()?, stamp)
}

/// Each zone's names as read this process (None: not known).
static KNOWN: OnceLock<Mutex<HashMap<PathBuf, Option<Arc<Names>>>>> = OnceLock::new();

/// The names of the zone at `zone`, from its file the first time (None:
/// off, no file, or the file is from another zone or build).
pub fn names(zone: &Path) -> Option<Arc<Names>> {
    if !on() {
        return None;
    }
    let mut known = KNOWN.get_or_init(Default::default).lock().ok()?;
    known
        .entry(zone.to_owned())
        .or_insert_with(|| {
            let file = file_for(&crate::pipecache::cache_dir()?, zone)?;
            load(&file, &stamp(zone)?).map(Arc::new)
        })
        .clone()
}

/// Whether the zone at `zone` is known not to answer `name` as `kind`
/// (false when it is not known).
pub fn lacks(zone: &Path, kind: Kind, name: &str) -> bool {
    names(zone).is_some_and(|n| !n.has(kind, name))
}

/// Writes the names of the zone at `zone`, just opened, unless they are
/// known already (`make` lists them: called only then).
pub fn note(zone: &Path, make: impl FnOnce() -> Names) {
    if !on() || names(zone).is_some() {
        return;
    }
    let (Some(dir), Some(st)) = (crate::pipecache::cache_dir(), stamp(zone)) else { return };
    let Some(file) = file_for(&dir, zone) else { return };
    let n = make();
    if save(&file, &st, &n)
        && let Ok(mut known) = KNOWN.get_or_init(Default::default).lock()
    {
        known.insert(zone.to_owned(), Some(Arc::new(n)));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Names {
        let mut n = Names::default();
        n.add(Kind::Material, "mc/mtl_robot_head".into());
        n.add(Kind::Techset, "wc/lit#882572b8".into());
        n.add(Kind::Technique, "3:wc/lit#882572b8".into());
        n.add(Kind::Model, "c_t7_mp_robot_head".into());
        n
    }

    #[test]
    fn round_trip_keeps_every_name() {
        let raw = encode("t7zi 1 10 20 30 40", &sample()).unwrap();
        let back = decode(&raw, "t7zi 1 10 20 30 40").unwrap();
        assert_eq!(back, sample());
        assert!(back.has(Kind::Model, "c_t7_mp_robot_head"));
        assert!(!back.has(Kind::Material, "c_t7_mp_robot_head"));
    }

    #[test]
    fn another_stamp_or_damage_is_not_read() {
        let mut raw = encode("t7zi 1 10 20 30 40", &sample()).unwrap();
        assert!(decode(&raw, "t7zi 1 10 21 30 40").is_none());
        let last = raw.len() - 1;
        raw[last] ^= 1;
        assert!(decode(&raw, "t7zi 1 10 20 30 40").is_none());
        assert!(decode(&raw[..4], "t7zi 1 10 20 30 40").is_none());
    }

    #[test]
    fn a_name_with_a_line_break_writes_no_file() {
        let mut n = sample();
        n.add(Kind::Material, "a\nb".into());
        assert!(encode("t7zi 1", &n).is_none());
    }

    #[test]
    fn saved_file_loads_back() {
        let dir = std::env::temp_dir().join(format!("t7_zone_index_test_{}", std::process::id()));
        let file = file_for(&dir, Path::new("x/core_common.ff")).unwrap();
        assert!(file.ends_with("t7_zone_index/core_common.idx"));
        assert!(save(&file, "t7zi 1 s", &sample()));
        assert_eq!(load(&file, "t7zi 1 s"), Some(sample()));
        assert!(load(&file, "t7zi 1 t").is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
