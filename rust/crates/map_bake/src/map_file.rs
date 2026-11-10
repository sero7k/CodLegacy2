//! One finished file per map (`map.pak`): the container every converted
//! section of a map lives in (`docs/MAP_CONVERTER.md`).
//!
//! ```text
//! file      = MAGIC "BO2MAP\0\0" + FORMAT u32 + file key u64 + count u32 + directory + sections
//! directory = per section: tag [u8; 8], section key u64, offset u64, packed len u64, raw len u64
//! section   = lz4 (size prepended) of the section's own layout
//! ```
//!
//! The file key says which zone (name, size, time) and converter the file was
//! made from; a file with the wrong key is ignored and removed. Each section
//! carries its own key too, so a changed rule rebuilds one section, not the
//! file. Sections are read on demand: opening the file reads the directory
//! only. A section's body is its own written layout with its own reader
//! (never a dump of a Rust struct), so a struct change never changes a file
//! silently.

use std::io::{self, Read as _, Seek as _, SeekFrom, Write as _};
use std::path::{Path, PathBuf};

const MAGIC: &[u8; 8] = b"BO2MAP\0\0";
const FORMAT: u32 = 1;
const HEADER: u64 = 8 + 4 + 8 + 4;
const ENTRY: u64 = 8 + 8 + 8 + 8 + 8;
/// A section may be this big raw (a sun map is 64 MiB; pictures more).
const MAX_RAW: u64 = 1 << 32;

/// The name of the finished file inside a map's bake folder.
pub const FILE: &str = "map.pak";

/// A section's tag: up to 8 ASCII bytes, zero padded (`b"SUNMAP"`).
pub type Tag = [u8; 8];

pub const fn tag(name: &str) -> Tag {
    let b = name.as_bytes();
    assert!(b.len() <= 8, "a tag is at most 8 bytes");
    let mut t = [0u8; 8];
    let mut i = 0;
    while i < b.len() {
        t[i] = b[i];
        i += 1;
    }
    t
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub tag: Tag,
    pub key: u64,
    offset: u64,
    packed: u64,
    pub raw: u64,
}

/// A finished file open for reading: the directory, the sections on demand.
pub struct Reader {
    path: PathBuf,
    file: std::fs::File,
    key: u64,
    entries: Vec<Entry>,
}

impl Reader {
    /// Opens `path` when it carries `key`. A file with another key, another
    /// format, or a broken directory is removed and `None` comes back.
    pub fn open(path: &Path, key: u64) -> Option<Reader> {
        let file = std::fs::File::open(path).ok()?;
        match Self::read_dir(file, key) {
            Ok(Some((file, entries))) => Some(Reader { path: path.to_path_buf(), file, key, entries }),
            _ => {
                let _ = std::fs::remove_file(path);
                None
            }
        }
    }

    fn read_dir(mut file: std::fs::File, key: u64) -> io::Result<Option<(std::fs::File, Vec<Entry>)>> {
        let len = file.metadata()?.len();
        let mut head = [0u8; HEADER as usize];
        if file.read_exact(&mut head).is_err() || &head[..8] != MAGIC {
            return Ok(None);
        }
        if u32::from_le_bytes(head[8..12].try_into().unwrap()) != FORMAT || u64::from_le_bytes(head[12..20].try_into().unwrap()) != key {
            return Ok(None);
        }
        let count = u32::from_le_bytes(head[20..24].try_into().unwrap()) as u64;
        if count > 4096 || HEADER + count * ENTRY > len {
            return Ok(None);
        }
        let mut dir = vec![0u8; (count * ENTRY) as usize];
        file.read_exact(&mut dir)?;
        let mut entries = Vec::with_capacity(count as usize);
        for e in dir.chunks_exact(ENTRY as usize) {
            let u = |at: usize| u64::from_le_bytes(e[at..at + 8].try_into().unwrap());
            let entry = Entry { tag: e[..8].try_into().unwrap(), key: u(8), offset: u(16), packed: u(24), raw: u(32) };
            let end = entry.offset.checked_add(entry.packed);
            if end.is_none_or(|end| end > len) || entry.raw > MAX_RAW || entries.iter().any(|x: &Entry| x.tag == entry.tag) {
                return Ok(None);
            }
            entries.push(entry);
        }
        Ok(Some((file, entries)))
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn key(&self) -> u64 {
        self.key
    }

    pub fn entries(&self) -> &[Entry] {
        &self.entries
    }

    pub fn entry(&self, tag: Tag) -> Option<&Entry> {
        self.entries.iter().find(|e| e.tag == tag)
    }

    /// The section's body, when it is there under `key`. A section under
    /// another key reads as missing (its writer rebuilds it); a damaged
    /// section is `None` too.
    pub fn section(&mut self, tag: Tag, key: u64) -> Option<Vec<u8>> {
        let e = self.entry(tag)?.clone();
        if e.key != key {
            return None;
        }
        let packed = self.packed(&e).ok()?;
        let body = lz4_flex::decompress_size_prepended(&packed).ok()?;
        (body.len() as u64 == e.raw).then_some(body)
    }

    fn packed(&mut self, e: &Entry) -> io::Result<Vec<u8>> {
        self.file.seek(SeekFrom::Start(e.offset))?;
        let mut packed = vec![0u8; e.packed as usize];
        self.file.read_exact(&mut packed)?;
        Ok(packed)
    }

    /// A writer holding every section of this file as it is (still packed),
    /// to add or replace sections and write the file afresh.
    pub fn edit(mut self) -> io::Result<Writer> {
        let mut w = Writer::new(self.key);
        for e in self.entries.clone() {
            let packed = self.packed(&e)?;
            w.sections.push(Packed { tag: e.tag, key: e.key, raw: e.raw, packed });
        }
        Ok(w)
    }
}

struct Packed {
    tag: Tag,
    key: u64,
    raw: u64,
    packed: Vec<u8>,
}

/// A finished file being put together.
pub struct Writer {
    key: u64,
    sections: Vec<Packed>,
}

impl Writer {
    pub fn new(key: u64) -> Writer {
        Writer { key, sections: Vec::new() }
    }

    /// Adds `body` as section `tag` under `key` (replacing a section of that tag).
    pub fn section(&mut self, tag: Tag, key: u64, body: &[u8]) -> &mut Self {
        self.sections.retain(|s| s.tag != tag);
        self.sections.push(Packed { tag, key, raw: body.len() as u64, packed: lz4_flex::compress_prepend_size(body) });
        self
    }

    pub fn has(&self, tag: Tag) -> bool {
        self.sections.iter().any(|s| s.tag == tag)
    }

    /// Writes the file: a temp file beside it, then one rename, so a crash
    /// mid-write never leaves a half file under the name. Returns the bytes written.
    pub fn write(&self, path: &Path) -> io::Result<u64> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let part = path.with_extension(format!("pak.part{}", std::process::id()));
        let written = (|| {
            let mut f = io::BufWriter::new(std::fs::File::create(&part)?);
            let mut head = Vec::with_capacity(HEADER as usize + self.sections.len() * ENTRY as usize);
            head.extend_from_slice(MAGIC);
            head.extend_from_slice(&FORMAT.to_le_bytes());
            head.extend_from_slice(&self.key.to_le_bytes());
            head.extend_from_slice(&(self.sections.len() as u32).to_le_bytes());
            let mut at = HEADER + self.sections.len() as u64 * ENTRY;
            for s in &self.sections {
                head.extend_from_slice(&s.tag);
                head.extend_from_slice(&s.key.to_le_bytes());
                head.extend_from_slice(&at.to_le_bytes());
                head.extend_from_slice(&(s.packed.len() as u64).to_le_bytes());
                head.extend_from_slice(&s.raw.to_le_bytes());
                at += s.packed.len() as u64;
            }
            f.write_all(&head)?;
            for s in &self.sections {
                f.write_all(&s.packed)?;
            }
            f.flush()?;
            Ok::<u64, io::Error>(at)
        })();
        match written {
            Ok(n) => match std::fs::rename(&part, path) {
                Ok(()) => Ok(n),
                Err(e) => {
                    let _ = std::fs::remove_file(&part);
                    Err(e)
                }
            },
            Err(e) => {
                let _ = std::fs::remove_file(&part);
                Err(e)
            }
        }
    }
}

/// Reads section `tag` of the map's file under the file key `file_key` and
/// the section key `key`: one call for a load.
pub fn read_section(path: &Path, file_key: u64, tag: Tag, key: u64) -> Option<Vec<u8>> {
    Reader::open(path, file_key)?.section(tag, key)
}

/// Adds or replaces section `tag` in the map's file (made afresh under
/// `file_key` when there is none or its key differs). Returns the file's size.
pub fn write_section(path: &Path, file_key: u64, tag: Tag, key: u64, body: &[u8]) -> io::Result<u64> {
    let mut w = match Reader::open(path, file_key) {
        Some(r) => r.edit()?,
        None => Writer::new(file_key),
    };
    w.section(tag, key, body);
    w.write(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir() -> PathBuf {
        let d = std::env::temp_dir().join(format!("map_file_test_{}_{}", std::process::id(), rand_u64()));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn rand_u64() -> u64 {
        use std::hash::{BuildHasher, Hasher};
        std::collections::hash_map::RandomState::new().build_hasher().finish()
    }

    #[test]
    fn sections_round_trip_and_grow() {
        let d = dir();
        let p = d.join(FILE);
        let big: Vec<u8> = (0..100_000u32).map(|i| (i % 251) as u8).collect();
        write_section(&p, 7, tag("SUNMAP"), 1, &big).unwrap();
        write_section(&p, 7, tag("CLIP"), 2, b"clip body").unwrap();
        let mut r = Reader::open(&p, 7).expect("opens");
        assert_eq!(r.entries().len(), 2);
        assert_eq!(r.section(tag("SUNMAP"), 1).as_deref(), Some(big.as_slice()));
        assert_eq!(r.section(tag("CLIP"), 2).as_deref(), Some(b"clip body".as_slice()));
        assert!(r.section(tag("CLIP"), 3).is_none(), "a section under another key is missing");
        assert!(r.section(tag("ENTS"), 2).is_none());
        // replacing one section keeps the other
        write_section(&p, 7, tag("CLIP"), 3, b"new clip").unwrap();
        let mut r = Reader::open(&p, 7).unwrap();
        assert_eq!(r.section(tag("CLIP"), 3).as_deref(), Some(b"new clip".as_slice()));
        assert_eq!(r.section(tag("SUNMAP"), 1).unwrap().len(), big.len());
        assert!(d.join(FILE).with_extension(format!("pak.part{}", std::process::id())).exists().eq(&false));
        std::fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn wrong_file_key_or_damage_is_removed() {
        let d = dir();
        let p = d.join(FILE);
        write_section(&p, 7, tag("SUNMAP"), 1, b"sun").unwrap();
        assert!(Reader::open(&p, 8).is_none());
        assert!(!p.exists(), "a file with the wrong key is removed");
        // a new file under the new key starts empty
        write_section(&p, 8, tag("CLIP"), 1, b"clip").unwrap();
        let r = Reader::open(&p, 8).unwrap();
        assert_eq!(r.entries().len(), 1);
        drop(r);
        // a truncated section reads as missing, the directory still opens
        let bytes = std::fs::read(&p).unwrap();
        std::fs::write(&p, &bytes[..bytes.len() - 2]).unwrap();
        assert!(Reader::open(&p, 8).is_none());
        std::fs::write(&p, b"BO2MAP\0\0junk").unwrap();
        assert!(Reader::open(&p, 8).is_none());
        assert!(!p.exists());
        std::fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn tags() {
        assert_eq!(&tag("SUNMAP"), b"SUNMAP\0\0");
        assert_eq!(&tag("A"), b"A\0\0\0\0\0\0\0");
    }
}
