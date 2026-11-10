//! bo2mp: a small copy of the install, holding only what the rebuild reads
//! of it for some purpose (its guns, their sounds and pictures), so the
//! owner can keep that without the whole game.
//!
//! While [`record_start`] is on, every zone read ([`crate::read_zone`]) and
//! every package entry decoded ([`crate::xsub::decode_entry`]), and every
//! level of a picture up to the largest one asked for
//! ([`crate::map::materials::Packages::level`]), is noted. [`write_slim`]
//! then writes, under the copy's `zone` folder:
//!
//! - each zone noted, copied whole (a zone is read whole);
//! - for each zone `.xpak` with entries noted, a new `.xpak` and one part
//!   (`<name>-00000.xsub`) holding those entries alone, byte for byte (an
//!   entry's segment offsets are its own, so it moves as it is); the part is
//!   a package with its own table too, as the sound reader finds streams in
//!   any `.xsub`;
//! - each standalone `.xsub` with entries noted, cut down the same way;
//!
//! and the game's Oodle DLL beside it. Readers open the copy exactly as the
//! install: nothing reads it differently.

use std::collections::{BTreeSet, HashMap, HashSet};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use crate::xsub::{self, ENTRY_ALIGN, HASH_ENTRY_LEN, HEADER_LEN, TOC_OFFSET, XSubEntry};

#[derive(Default)]
struct Recorded {
    keys: HashSet<u64>,
    zones: BTreeSet<PathBuf>,
    /// Entries read at a place known to the reader (a sound's row says
    /// its package, offset and size): (file, offset, size, key).
    ranges: Vec<(PathBuf, u64, u32, u64)>,
}

static REC: Mutex<Option<Recorded>> = Mutex::new(None);

/// Starts noting what is read (what was noted before is dropped).
pub fn record_start() {
    if let Ok(mut r) = REC.lock() {
        *r = Some(Recorded::default());
    }
}

/// Stops noting: the entries' keys and the zones read since the start.
pub fn record_stop() -> (HashSet<u64>, BTreeSet<PathBuf>) {
    REC.lock().ok().and_then(|mut r| r.take()).map(|r| (r.keys, r.zones)).unwrap_or_default()
}

/// Notes a package entry's key (while recording).
pub fn note_key(key: u64) {
    if let Ok(mut r) = REC.lock()
        && let Some(r) = r.as_mut()
    {
        r.keys.insert(key);
    }
}

/// Notes an entry read straight from `file` at `offset` (while recording).
pub fn note_range(file: &Path, offset: u64, size: u32, key: u64) {
    if let Ok(mut r) = REC.lock()
        && let Some(r) = r.as_mut()
    {
        r.ranges.push((file.to_path_buf(), offset, size, key));
    }
}

/// The entries noted by [`note_range`] so far (call before [`record_stop`]).
pub fn recorded_ranges() -> Vec<(PathBuf, u64, u32, u64)> {
    REC.lock().ok().and_then(|r| r.as_ref().map(|r| r.ranges.clone())).unwrap_or_default()
}

/// Notes a zone file (while recording).
pub fn note_zone(path: &Path) {
    if let Ok(mut r) = REC.lock()
        && let Some(r) = r.as_mut()
    {
        r.zones.insert(path.to_path_buf());
    }
}

/// Whether anything is being noted (so callers can skip work otherwise).
pub fn recording() -> bool {
    REC.lock().is_ok_and(|r| r.is_some())
}

/// What a copy came to.
#[derive(Debug, Default, Clone)]
pub struct SlimReport {
    pub zones: usize,
    pub packages: usize,
    pub entries: usize,
    pub bytes: u64,
}

fn u64_at(b: &[u8], at: usize) -> u64 {
    u64::from_le_bytes(b[at..at + 8].try_into().unwrap())
}

fn put_u64(b: &mut [u8], at: usize, v: u64) {
    b[at..at + 8].copy_from_slice(&v.to_le_bytes());
}

fn align(v: u64) -> u64 {
    v.div_ceil(ENTRY_ALIGN) * ENTRY_ALIGN
}

fn header_of(path: &Path) -> std::io::Result<Vec<u8>> {
    let mut h = vec![0u8; HEADER_LEN];
    std::fs::File::open(path)?.read_exact(&mut h)?;
    Ok(h)
}

/// One row of a hash table, as stored.
fn row(key: u64, offset: u64, size: u32, decompressed: u32, compressed: bool) -> [u8; HASH_ENTRY_LEN] {
    let mut r = [0u8; HASH_ENTRY_LEN];
    r[..8].copy_from_slice(&key.to_le_bytes());
    let packed = ((offset / ENTRY_ALIGN) << 32) | (u64::from(size & 0x7fff_ffff) << 1) | u64::from(compressed);
    r[8..16].copy_from_slice(&packed.to_le_bytes());
    r[16..20].copy_from_slice(&decompressed.to_le_bytes());
    r
}

/// Writes a package of `entries` (each its table row and its stored bytes)
/// at `path`, its header shaped on `template` (an original's): the data
/// from the template's data offset, each entry 128-aligned, the table
/// after. Returns where each entry went and the file's length.
fn write_package(path: &Path, template: &[u8], entries: &[(XSubEntry, Vec<u8>)]) -> std::io::Result<(Vec<u64>, u64)> {
    let mut h = template.to_vec();
    let data_offset = u64_at(&h, TOC_OFFSET + 8).max(HEADER_LEN as u64);
    let mut at = data_offset;
    let mut placed = Vec::with_capacity(entries.len());
    let mut f = std::io::BufWriter::new(std::fs::File::create(path)?);
    f.write_all(&vec![0u8; data_offset as usize])?;
    for (_, blob) in entries {
        placed.push(at);
        f.write_all(blob)?;
        let end = at + blob.len() as u64;
        let pad = align(end) - end;
        f.write_all(&vec![0u8; pad as usize])?;
        at = end + pad;
    }
    let data_size = at - data_offset;
    // (The table sorted by key, as readers binary-search it.)
    let mut rows: Vec<[u8; HASH_ENTRY_LEN]> = entries
        .iter()
        .zip(&placed)
        .map(|((e, blob), &off)| row(e.key, off, blob.len() as u32, e.decompressed, e.compressed))
        .collect();
    rows.sort_by_key(|r| u64_at(r, 0));
    let hash_offset = at;
    for r in &rows {
        f.write_all(r)?;
    }
    let file_len = hash_offset + (rows.len() * HASH_ENTRY_LEN) as u64;
    put_u64(&mut h, 0x18, file_len);
    put_u64(&mut h, TOC_OFFSET, rows.len() as u64);
    put_u64(&mut h, TOC_OFFSET + 8, data_offset);
    put_u64(&mut h, TOC_OFFSET + 16, data_size);
    put_u64(&mut h, TOC_OFFSET + 24, rows.len() as u64);
    put_u64(&mut h, TOC_OFFSET + 32, hash_offset);
    put_u64(&mut h, TOC_OFFSET + 40, (rows.len() * HASH_ENTRY_LEN) as u64);
    f.flush()?;
    let mut file = f.into_inner().map_err(|e| e.into_error())?;
    file.seek(SeekFrom::Start(0))?;
    file.write_all(&h)?;
    Ok((placed, file_len))
}

/// Writes a zone `.xpak` index over a single part of `len` bytes, its rows
/// pointing at `placed` (the part's offsets), shaped on `template`.
fn write_index(path: &Path, template: &[u8], entries: &[(XSubEntry, Vec<u8>)], placed: &[u64]) -> std::io::Result<()> {
    let mut h = template.to_vec();
    let data_offset = HEADER_LEN as u64;
    let mut rows: Vec<[u8; HASH_ENTRY_LEN]> = entries
        .iter()
        .zip(placed)
        .map(|((e, blob), &off)| row(e.key, off, blob.len() as u32, e.decompressed, e.compressed))
        .collect();
    rows.sort_by_key(|r| u64_at(r, 0));
    let hash_offset = data_offset;
    let file_len = hash_offset + (rows.len() * HASH_ENTRY_LEN) as u64;
    put_u64(&mut h, 0x18, file_len);
    put_u64(&mut h, 0x28, 1);
    put_u64(&mut h, TOC_OFFSET, rows.len() as u64);
    put_u64(&mut h, TOC_OFFSET + 8, data_offset);
    put_u64(&mut h, TOC_OFFSET + 16, 0);
    put_u64(&mut h, TOC_OFFSET + 24, rows.len() as u64);
    put_u64(&mut h, TOC_OFFSET + 32, hash_offset);
    put_u64(&mut h, TOC_OFFSET + 40, (rows.len() * HASH_ENTRY_LEN) as u64);
    let mut f = std::io::BufWriter::new(std::fs::File::create(path)?);
    f.write_all(&h)?;
    for r in &rows {
        f.write_all(r)?;
    }
    f.flush()
}

/// An entry's stored bytes from a file.
fn read_blob(path: &Path, offset: u64, size: u32) -> std::io::Result<Vec<u8>> {
    let mut f = std::fs::File::open(path)?;
    let mut blob = vec![0u8; size as usize];
    f.seek(SeekFrom::Start(offset))?;
    f.read_exact(&mut blob)?;
    Ok(blob)
}

/// `<stem>-NNNNN.xsub` -> `<stem>`.
fn part_stem(name: &str) -> Option<&str> {
    let stem = name.strip_suffix(".xsub")?;
    let (s, n) = stem.rsplit_once('-')?;
    (n.len() == 5 && n.chars().all(|c| c.is_ascii_digit())).then_some(s)
}

/// Writes the small copy of `install` at `dest` (see the module docs):
/// the zones `zones` (paths in the install's `zone` folder, or names
/// there), the entries `keys` of every package, and the Oodle DLL.
/// `say` hears how far it is.
pub fn write_slim(
    install: &Path,
    dest: &Path,
    keys: &HashSet<u64>,
    zones: &BTreeSet<PathBuf>,
    say: &mut dyn FnMut(&str),
) -> Result<SlimReport, String> {
    let src = install.join("zone");
    let out = dest.join("zone");
    std::fs::create_dir_all(&out).map_err(|e| format!("{}: {e}", out.display()))?;
    let mut report = SlimReport::default();
    // The zones, whole.
    for z in zones {
        let Some(name) = z.file_name() else { continue };
        let from = src.join(name);
        let to = out.join(name);
        let len = std::fs::metadata(&from).map_err(|e| format!("{}: {e}", from.display()))?.len();
        if std::fs::metadata(&to).is_ok_and(|m| m.len() == len) {
            report.zones += 1;
            report.bytes += len;
            continue;
        }
        say(&format!("copying zone {}", name.to_string_lossy()));
        std::fs::copy(&from, &to).map_err(|e| format!("{}: {e}", from.display()))?;
        report.zones += 1;
        report.bytes += len;
    }
    // The DLL.
    let dll = crate::oodle::dll_path(install);
    if let Some(name) = dll.file_name() {
        std::fs::copy(&dll, dest.join(name)).map_err(|e| format!("{}: {e}", dll.display()))?;
    }
    // The packages.
    let names: Vec<String> = std::fs::read_dir(&src)
        .map_err(|e| format!("{}: {e}", src.display()))?
        .filter_map(|e| e.ok()?.file_name().into_string().ok())
        .collect();
    let xpaks: HashSet<&str> = names.iter().filter_map(|n| n.strip_suffix(".xpak")).collect();
    let mut taken: HashSet<u64> = HashSet::new();
    for stem in &xpaks {
        let path = src.join(format!("{stem}.xpak"));
        let Ok(pak) = xsub::XPak::open(&path) else { continue };
        let mut entries: Vec<(XSubEntry, Vec<u8>)> = Vec::new();
        for e in pak.entries.iter().filter(|e| keys.contains(&e.key)) {
            let Some((part, local)) = pak.locate(e) else { continue };
            let blob = read_blob(&pak.parts[part], local.offset, local.size).map_err(|err| format!("{}: {err}", pak.parts[part].display()))?;
            entries.push((local, blob));
        }
        if entries.is_empty() {
            continue;
        }
        say(&format!("writing package {stem} ({} entries)", entries.len()));
        let part_template = header_of(&pak.parts[0]).map_err(|e| format!("{}: {e}", pak.parts[0].display()))?;
        let part = out.join(format!("{stem}-00000.xsub"));
        let (placed, len) = write_package(&part, &part_template, &entries).map_err(|e| format!("{}: {e}", part.display()))?;
        let index_template = header_of(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        write_index(&out.join(format!("{stem}.xpak")), &index_template, &entries, &placed).map_err(|e| format!("{stem}.xpak: {e}"))?;
        taken.extend(entries.iter().map(|(e, _)| e.key));
        report.packages += 1;
        report.entries += entries.len();
        report.bytes += len;
    }
    // Packages on their own (not a zone `.xpak`'s part): the sound reader
    // finds streams in these too.
    for name in names.iter().filter(|n| n.ends_with(".xsub")) {
        if part_stem(name).is_some_and(|s| xpaks.contains(s)) {
            continue;
        }
        let path = src.join(name);
        let Ok((_, rows)) = xsub::read_index(&mut std::fs::File::open(&path).map_err(|e| format!("{}: {e}", path.display()))?) else {
            continue;
        };
        let mut entries = Vec::new();
        for e in rows.iter().filter(|e| keys.contains(&e.key) && !taken.contains(&e.key)) {
            entries.push((*e, read_blob(&path, e.offset, e.size).map_err(|err| format!("{}: {err}", path.display()))?));
        }
        if entries.is_empty() {
            continue;
        }
        say(&format!("writing package {name} ({} entries)", entries.len()));
        let template = header_of(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        let (_, len) = write_package(&out.join(name), &template, &entries).map_err(|e| format!("{name}: {e}"))?;
        report.packages += 1;
        report.entries += entries.len();
        report.bytes += len;
    }
    Ok(report)
}

/// Where one noted entry's stored bytes are: a file of the install's `zone`
/// folder, the offset in it and the length.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlanRow {
    pub file: String,
    pub offset: u64,
    pub size: u32,
    pub key: u64,
}

/// The ranges of the entries `keys` in the install's packages, as
/// [`write_slim`] would copy them, without reading a byte of any part (the
/// zone `.xpak` indexes are enough; the parts only need to exist), and the
/// keys found in none. Lets the ranges be cut out on the owner's machine.
pub fn plan(install: &Path, keys: &HashSet<u64>, direct: &[(PathBuf, u64, u32, u64)]) -> Result<(Vec<PlanRow>, usize), String> {
    let src = install.join("zone");
    let mut names: Vec<String> = std::fs::read_dir(&src)
        .map_err(|e| format!("{}: {e}", src.display()))?
        .filter_map(|e| e.ok()?.file_name().into_string().ok())
        .filter(|n| n.ends_with(".xpak"))
        .collect();
    names.sort();
    let mut rows = Vec::new();
    let mut found: HashSet<u64> = HashSet::new();
    for (file, offset, size, key) in direct {
        if found.insert(*key) {
            let file = file.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            rows.push(PlanRow { file, offset: *offset, size: *size, key: *key });
        }
    }
    for name in &names {
        let Ok(pak) = xsub::XPak::open(&src.join(name)) else { continue };
        for e in pak.entries.iter().filter(|e| keys.contains(&e.key)) {
            if found.contains(&e.key) {
                continue;
            }
            let Some((part, local)) = pak.locate(e) else { continue };
            let file = pak.parts[part].file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            found.insert(e.key);
            rows.push(PlanRow { file, offset: local.offset, size: local.size, key: e.key });
        }
    }
    rows.sort_by(|a, b| (&a.file, a.offset).cmp(&(&b.file, b.offset)));
    Ok((rows, keys.iter().filter(|k| !found.contains(k)).count()))
}

/// The zones of `names` in the install's `zone` folder (`ingame` ->
/// `<install>/zone/ingame.ff`).
pub fn zone_paths(install: &Path, names: &[&str]) -> BTreeSet<PathBuf> {
    names.iter().map(|n| install.join("zone").join(format!("{n}.ff"))).collect()
}

/// Every entry key a table holds (a test aid; keys by package).
pub fn keys_of(path: &Path) -> Result<HashMap<u64, XSubEntry>, String> {
    let (_, rows) = xsub::read_index(&mut std::fs::File::open(path).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
    Ok(rows.into_iter().map(|e| (e.key, e)).collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::oodle::StoredOnly;

    /// A package of stored entries, cut down to some of them, reads back
    /// those entries' bytes; an index over it locates them in its part.
    #[test]
    fn a_cut_down_package_reads_back() {
        let dir = std::env::temp_dir().join(format!("jup_pack_test_{}", std::process::id()));
        let src = dir.join("src/zone");
        std::fs::create_dir_all(&src).unwrap();
        // A template header, as an original's.
        let mut template = vec![0u8; HEADER_LEN];
        template[..4].copy_from_slice(xsub::MAGIC);
        template[4..6].copy_from_slice(&1u16.to_le_bytes());
        template[6..8].copy_from_slice(&xsub::VERSION.to_le_bytes());
        put_u64(&mut template, TOC_OFFSET + 8, 0x8000);
        let blobs: Vec<(XSubEntry, Vec<u8>)> = (0..5u64)
            .map(|k| {
                let b = vec![k as u8 + 1; 100 + k as usize * 300];
                (XSubEntry { key: 0x1000 + k, offset: 0, size: b.len() as u32, decompressed: b.len() as u32, compressed: false }, b)
            })
            .collect();
        let whole = src.join("pkg-00000.xsub");
        let (placed, _) = write_package(&whole, &template, &blobs).unwrap();
        let mut idx = template.clone();
        idx[4..6].copy_from_slice(&2u16.to_le_bytes());
        write_index(&src.join("pkg.xpak"), &idx, &blobs, &placed).unwrap();
        // (A stand-in for the game's DLL, which the copy takes along.)
        std::fs::write(crate::oodle::dll_path(&dir.join("src")), b"dll").unwrap();
        // Cut down to two of them.
        let keep: HashSet<u64> = [0x1001, 0x1003].into_iter().collect();
        let dest = dir.join("dest");
        let r = write_slim(&dir.join("src"), &dest, &keep, &BTreeSet::new(), &mut |_| {}).unwrap();
        assert_eq!((r.packages, r.entries), (1, 2));
        let out = dest.join("zone");
        let small = xsub::XPak::open(&out.join("pkg.xpak")).unwrap();
        assert_eq!(small.entries.len(), 2);
        for k in [0x1001u64, 0x1003] {
            let e = small.find(k).unwrap();
            let bytes = small.read(e, &StoredOnly).unwrap();
            assert_eq!(bytes, blobs[(k - 0x1000) as usize].1);
        }
        // The part is a package of its own (the sound reader's way).
        let mut part = xsub::XSub::open(&out.join("pkg-00000.xsub")).unwrap();
        let e = *part.find(0x1003).unwrap();
        assert_eq!(part.read(&e, &StoredOnly).unwrap(), blobs[3].1);
        assert!(part.find(0x1000).is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
