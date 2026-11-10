//! bo2mp: the compiled scripts (and string tables) of Ghosts zones, for the sim's script
//! VM (`gsc_iw6_vm`), and the map's entity blocks with their raw numeric keys.
//!
//! A `ScriptFile` asset holds the *stream* (zlib) and the *code*; both are copied out here.
//! The scripts come first in a zone's asset order, so a walk that stops late (the 124 MB
//! `common` stops at asset 4819 of 4871) still has all of them.

use std::io::Read;
use std::path::Path;

use crate::walk::Capture;

/// One compiled script: the asset name (a decimal file id, or `maps/mp/mp_frag`), its
/// inflated string stream and its bytecode.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ZoneScript {
    pub name: String,
    pub stream: Vec<u8>,
    pub code: Vec<u8>,
}

/// A string table (`mp/...csv`) of a zone.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ZoneTable {
    pub name: String,
    pub columns: usize,
    pub rows: usize,
    pub cells: Vec<String>,
}

/// What one zone holds for the script VM.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ZoneScripts {
    pub scripts: Vec<ZoneScript>,
    pub tables: Vec<ZoneTable>,
}

/// The scripts and string tables of a walked zone.
pub fn from_capture(cap: &Capture, image: &[u8]) -> ZoneScripts {
    let mut out = ZoneScripts::default();
    for sf in &cap.script_files {
        let (Some(b), Some(c)) = (sf.buffer, sf.bytecode) else { continue };
        let mut stream = Vec::new();
        if flate2::read::ZlibDecoder::new(b.get(image)).read_to_end(&mut stream).is_err() {
            continue;
        }
        out.scripts.push(ZoneScript { name: sf.name.clone(), stream, code: c.get(image).to_vec() });
    }
    for t in &cap.string_tables {
        out.tables.push(ZoneTable { name: t.name.clone(), columns: t.columns, rows: t.rows, cells: t.cells.clone() });
    }
    out
}

/// Opens and walks the zone at `path` and takes its scripts. `Err` when it cannot be opened.
pub fn read_zone(path: &Path) -> Result<ZoneScripts, String> {
    let zone = crate::Zone::open(path).map_err(|e| e.to_string())?;
    let (cap, _stop) = crate::walk::walk_zone(&zone);
    Ok(from_capture(&cap, &zone.image))
}

// ---- a cache of one zone's scripts (a second launch skips the walk) -------------------------

const MAGIC: &[u8; 8] = b"IW6SCR01";

fn put_u32(out: &mut Vec<u8>, v: usize) {
    out.extend_from_slice(&(v as u32).to_le_bytes());
}
fn put_bytes(out: &mut Vec<u8>, b: &[u8]) {
    put_u32(out, b.len());
    out.extend_from_slice(b);
}

/// The cache file's bytes.
pub fn encode(z: &ZoneScripts) -> Vec<u8> {
    let mut out = MAGIC.to_vec();
    put_u32(&mut out, z.scripts.len());
    for s in &z.scripts {
        put_bytes(&mut out, s.name.as_bytes());
        put_bytes(&mut out, &s.stream);
        put_bytes(&mut out, &s.code);
    }
    put_u32(&mut out, z.tables.len());
    for t in &z.tables {
        put_bytes(&mut out, t.name.as_bytes());
        put_u32(&mut out, t.columns);
        put_u32(&mut out, t.rows);
        put_u32(&mut out, t.cells.len());
        for c in &t.cells {
            put_bytes(&mut out, c.as_bytes());
        }
    }
    out
}

struct Rd<'a>(&'a [u8]);
impl<'a> Rd<'a> {
    fn u32(&mut self) -> Option<usize> {
        let (h, t) = self.0.split_at_checked(4)?;
        self.0 = t;
        Some(u32::from_le_bytes(h.try_into().ok()?) as usize)
    }
    fn bytes(&mut self) -> Option<&'a [u8]> {
        let n = self.u32()?;
        let (h, t) = self.0.split_at_checked(n)?;
        self.0 = t;
        Some(h)
    }
    fn string(&mut self) -> Option<String> {
        Some(String::from_utf8_lossy(self.bytes()?).into_owned())
    }
}

/// Reads the cache file's bytes; `None` when they are not a cache.
pub fn decode(b: &[u8]) -> Option<ZoneScripts> {
    let mut r = Rd(b.strip_prefix(MAGIC)?);
    let mut out = ZoneScripts::default();
    for _ in 0..r.u32()? {
        out.scripts.push(ZoneScript { name: r.string()?, stream: r.bytes()?.to_vec(), code: r.bytes()?.to_vec() });
    }
    for _ in 0..r.u32()? {
        let (name, columns, rows) = (r.string()?, r.u32()?, r.u32()?);
        let n = r.u32()?;
        let mut cells = Vec::with_capacity(n);
        for _ in 0..n {
            cells.push(r.string()?);
        }
        out.tables.push(ZoneTable { name, columns, rows, cells });
    }
    Some(out)
}

/// Where the cache of the zone file `path` lives (keyed by its size and modification time),
/// under `dir`.
pub fn cache_path(dir: &Path, path: &Path) -> Option<std::path::PathBuf> {
    let meta = std::fs::metadata(path).ok()?;
    let mtime = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map_or(0, |d| d.as_secs());
    let stem = path.file_stem()?.to_string_lossy().into_owned();
    Some(dir.join(format!("bo2mp_iw6_scripts_{stem}_{}_{mtime}.bin", meta.len())))
}

/// `read_zone` through the cache in `dir` (written when missing; a cache that cannot be read
/// or written is ignored). The second value is whether the cache answered.
pub fn read_zone_cached(path: &Path, dir: &Path) -> Result<(ZoneScripts, bool), String> {
    let cache = cache_path(dir, path);
    if let Some(c) = &cache
        && let Ok(bytes) = std::fs::read(c)
        && let Some(z) = decode(&bytes)
    {
        return Ok((z, true));
    }
    let z = read_zone(path)?;
    if let Some(c) = &cache {
        let tmp = c.with_extension("tmp");
        if std::fs::write(&tmp, encode(&z)).is_ok() {
            let _ = std::fs::rename(&tmp, c);
        }
    }
    Ok((z, false))
}

// ---- the map's entity blocks with their numeric keys ----------------------------------------

/// One entity block: its keys as the zone stores them (script field ids) and their text.
pub type RawBlock = Vec<(u32, String)>;

/// The entity text's blocks (the same blocks, in the same order, as `maplogic::blocks`, which
/// names the keys; empty blocks are left out). Values lose their quotes. A key that is not a
/// number (a line such as `0 "aomindist" "16"` is kept under key 0 with the rest of the line
/// after it) stays as it is.
pub fn raw_blocks(entities: &str) -> Vec<RawBlock> {
    let mut out = Vec::new();
    for block in entities.trim_end_matches('\0').split('{').skip(1) {
        let mut kv = Vec::new();
        for line in block.lines() {
            let t = line.trim();
            let Some((key, value)) = t.split_once(' ') else { continue };
            if key.is_empty() || !key.bytes().all(|b| b.is_ascii_digit()) {
                continue;
            }
            let id: u32 = key.parse().unwrap_or(u32::MAX);
            let v = value.trim();
            let v = v.strip_prefix('"').unwrap_or(v);
            let v = v.strip_suffix('"').unwrap_or(v);
            kv.push((id, v.to_owned()));
        }
        if !kv.is_empty() {
            out.push(kv);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cache_round_trip() {
        let z = ZoneScripts {
            scripts: vec![ZoneScript { name: "927".into(), stream: vec![1, 2, 3], code: vec![4, 5] }],
            tables: vec![ZoneTable { name: "t.csv".into(), columns: 2, rows: 1, cells: vec!["a".into(), "b".into()] }],
        };
        assert_eq!(decode(&encode(&z)), Some(z));
        assert_eq!(decode(b"nope"), None);
    }

    #[test]
    fn blocks_keep_numeric_keys() {
        let text = "{\n97 \"worldspawn\"\n}\n{\n430 \"1 2 3\"\n640 \"door\"\n27766 \"a=b;c=d\"\n}\n";
        let b = raw_blocks(text);
        assert_eq!(b.len(), 2);
        assert_eq!(b[1], vec![(430, "1 2 3".into()), (640, "door".into()), (27766, "a=b;c=d".into())]);
    }
}
