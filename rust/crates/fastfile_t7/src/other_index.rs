//! bo2mp: the other zones' picture index (`map::other_zones_image`) saved
//! to a small file, so a Black Ops III map after another game's map (which
//! lets the index go, the memory rule) reads it back instead of walking every
//! zone of the folder again (136 zones, 4.98 GB: ~30 s warm, minutes on a
//! busy drive). The file is keyed by the folder's zone list (names, sizes,
//! change times): any zone added, removed or changed builds it again.
//! `BO2MP_T7_OTHER_INDEX_CACHE=0` turns it off (every BO3 map after another
//! game's walks the folder, the old behaviour).

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::walk::capture::ImageRec;
use crate::walk::image::XPakEntry;

/// name -> (record, the zone file that defines it).
pub type Index = HashMap<String, (ImageRec, PathBuf)>;

const MAGIC: &[u8; 8] = b"T7OIDX01";

pub fn enabled() -> bool {
    !codlegacy_env::var("BO2MP_T7_OTHER_INDEX_CACHE").is_ok_and(|v| v.trim() == "0")
}

fn fnv(h: &mut u64, data: &[u8]) {
    for &b in data {
        *h ^= u64::from(b);
        *h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
}

/// The folder's `.ff` files, sorted, and a key over their names, sizes and
/// change times (one directory listing, no file is opened).
pub fn zones_and_key(dir: &Path) -> (Vec<PathBuf>, u64) {
    let mut zones: Vec<(PathBuf, u64, u128)> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| e.path().extension().is_some_and(|x| x == "ff"))
        .map(|e| {
            let m = e.metadata().ok();
            let len = m.as_ref().map_or(0, |m| m.len());
            let at = m
                .and_then(|m| m.modified().ok())
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map_or(0, |d| d.as_nanos());
            (e.path(), len, at)
        })
        .collect();
    zones.sort();
    let mut h = 0xcbf2_9ce4_8422_2325u64;
    for (p, len, at) in &zones {
        let name = p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or(String::new());
        fnv(&mut h, name.as_bytes());
        fnv(&mut h, &[0]);
        fnv(&mut h, &len.to_le_bytes());
        fnv(&mut h, &at.to_le_bytes());
    }
    (zones.into_iter().map(|(p, _, _)| p).collect(), h)
}

/// Where the folder's saved index goes: `CODLEGACY_CACHE_DIR`, else
/// `%LOCALAPPDATA%\iw4l\t7index`, else the XDG/HOME cache; one file per
/// folder (named by a hash of its path).
pub fn file_for(dir: &Path) -> Option<PathBuf> {
    let var = |k: &str| std::env::var_os(k).filter(|v| !v.is_empty()).map(PathBuf::from);
    let root = codlegacy_env::var_os("CODLEGACY_CACHE_DIR")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .or_else(|| var("LOCALAPPDATA").map(|d| d.join("iw4l").join("t7index")))
        .or_else(|| var("XDG_CACHE_HOME").or_else(|| var("HOME").map(|h| h.join(".cache"))).map(|d| d.join("iw4l")))?;
    let mut h = 0xcbf2_9ce4_8422_2325u64;
    fnv(&mut h, dir.to_string_lossy().to_lowercase().as_bytes());
    Some(root.join(format!("t7_other_index_{h:016x}.bin")))
}

fn put_str(out: &mut Vec<u8>, s: &str) {
    out.extend_from_slice(&(s.len() as u32).to_le_bytes());
    out.extend_from_slice(s.as_bytes());
}

fn put_opt(out: &mut Vec<u8>, v: Option<(usize, usize)>) {
    match v {
        Some((a, n)) => {
            out.push(1);
            out.extend_from_slice(&(a as u64).to_le_bytes());
            out.extend_from_slice(&(n as u64).to_le_bytes());
        }
        None => out.push(0),
    }
}

/// The index as bytes: zones by file name (they are all in `dir`).
pub fn encode(index: &Index, key: u64) -> Vec<u8> {
    let mut out = Vec::with_capacity(64 + index.len() * 96);
    out.extend_from_slice(MAGIC);
    out.extend_from_slice(&key.to_le_bytes());
    out.extend_from_slice(&(index.len() as u32).to_le_bytes());
    let mut names: Vec<&String> = index.keys().collect();
    names.sort();
    for name in names {
        let (im, zone) = &index[name];
        put_str(&mut out, name);
        put_str(&mut out, &im.name);
        put_str(&mut out, &zone.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or(String::new()));
        out.extend_from_slice(&im.width.to_le_bytes());
        out.extend_from_slice(&im.height.to_le_bytes());
        out.extend_from_slice(&im.depth.to_le_bytes());
        out.push(im.levels);
        out.extend_from_slice(&im.format.to_le_bytes());
        put_opt(&mut out, im.pixels);
        put_opt(&mut out, im.fallback);
        out.extend_from_slice(&(im.pixels_len as u64).to_le_bytes());
        out.extend_from_slice(&(im.xpak.len() as u32).to_le_bytes());
        for x in &im.xpak {
            out.extend_from_slice(&x.key.to_le_bytes());
            out.extend_from_slice(&x.offset.to_le_bytes());
            out.extend_from_slice(&x.size.to_le_bytes());
            out.extend_from_slice(&x.bits.to_le_bytes());
            out.extend_from_slice(&x.width.to_le_bytes());
            out.extend_from_slice(&x.height.to_le_bytes());
            out.push(x.levels);
        }
    }
    out
}

struct Reader<'a> {
    b: &'a [u8],
    at: usize,
}

impl<'a> Reader<'a> {
    fn take(&mut self, n: usize) -> Option<&'a [u8]> {
        let s = self.b.get(self.at..self.at.checked_add(n)?)?;
        self.at += n;
        Some(s)
    }
    fn u8(&mut self) -> Option<u8> {
        self.take(1).map(|s| s[0])
    }
    fn u16(&mut self) -> Option<u16> {
        self.take(2).map(|s| u16::from_le_bytes([s[0], s[1]]))
    }
    fn u32(&mut self) -> Option<u32> {
        self.take(4).and_then(|s| s.try_into().ok()).map(u32::from_le_bytes)
    }
    fn u64(&mut self) -> Option<u64> {
        self.take(8).and_then(|s| s.try_into().ok()).map(u64::from_le_bytes)
    }
    fn str(&mut self) -> Option<String> {
        let n = self.u32()? as usize;
        String::from_utf8(self.take(n)?.to_vec()).ok()
    }
    fn opt(&mut self) -> Option<Option<(usize, usize)>> {
        Some(match self.u8()? {
            0 => None,
            1 => Some((self.u64()? as usize, self.u64()? as usize)),
            _ => return None,
        })
    }
}

/// The index back from `encode`'s bytes, when they are whole and were made
/// for `key` (zones joined to `dir`).
pub fn decode(bytes: &[u8], key: u64, dir: &Path) -> Option<Index> {
    let mut r = Reader { b: bytes, at: 0 };
    if r.take(8)? != MAGIC || r.u64()? != key {
        return None;
    }
    let n = r.u32()? as usize;
    let mut index = Index::with_capacity(n.min(1 << 20));
    for _ in 0..n {
        let name = r.str()?;
        let mut im = ImageRec { name: r.str()?, ..Default::default() };
        let zone = dir.join(r.str()?);
        im.width = r.u16()?;
        im.height = r.u16()?;
        im.depth = r.u16()?;
        im.levels = r.u8()?;
        im.format = r.u32()?;
        im.pixels = r.opt()?;
        im.fallback = r.opt()?;
        im.pixels_len = r.u64()? as usize;
        let parts = r.u32()? as usize;
        if parts > 64 {
            return None;
        }
        for _ in 0..parts {
            im.xpak.push(XPakEntry {
                key: r.u64()?,
                offset: r.u64()? as i64,
                size: r.u64()?,
                bits: r.u64()?,
                width: r.u16()?,
                height: r.u16()?,
                levels: r.u8()?,
            });
        }
        index.insert(name, (im, zone));
    }
    (r.at == bytes.len()).then_some(index)
}

/// The saved index for `dir`, when its file matches the folder's zones now.
pub fn load(dir: &Path, key: u64) -> Option<Index> {
    let bytes = std::fs::read(file_for(dir)?).ok()?;
    decode(&bytes, key, dir)
}

/// Saves the index (a temporary file, then renamed over the old one).
pub fn save(dir: &Path, key: u64, index: &Index) {
    let Some(path) = file_for(dir) else { return };
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let tmp = path.with_extension(format!("tmp{}", std::process::id()));
    if std::fs::write(&tmp, encode(index, key)).is_ok() && std::fs::rename(&tmp, &path).is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn saved_index_reads_back_the_same() {
        let dir = Path::new("some/zone");
        let mut index = Index::new();
        index.insert(
            "streamed".into(),
            (
                ImageRec {
                    name: "streamed".into(),
                    width: 1024,
                    height: 512,
                    depth: 1,
                    levels: 11,
                    format: 71,
                    pixels: None,
                    fallback: Some((4096, 512)),
                    pixels_len: 700_000,
                    xpak: vec![XPakEntry { key: 0xdead_beef_1234, offset: -7, size: 99, bits: 0b110_0011, width: 1024, height: 512, levels: 3 }],
                },
                dir.join("zm_stalingrad.ff"),
            ),
        );
        index.insert(
            "inzone".into(),
            (ImageRec { name: "inzone".into(), width: 4, height: 4, depth: 1, levels: 1, format: 28, pixels: Some((123, 64)), pixels_len: 64, ..Default::default() }, dir.join("mp_a.ff")),
        );
        let bytes = encode(&index, 42);
        let back = decode(&bytes, 42, dir).expect("decodes");
        assert_eq!(back.len(), 2);
        for (name, (im, zone)) in &index {
            let (b, bz) = &back[name];
            assert_eq!(bz, zone);
            assert_eq!(format!("{im:?}"), format!("{b:?}"));
        }
        // Another folder's zones (another key) or a cut file: built again.
        assert!(decode(&bytes, 43, dir).is_none());
        assert!(decode(&bytes[..bytes.len() - 1], 42, dir).is_none());
    }
}
