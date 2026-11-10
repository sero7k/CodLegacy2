//! `pictures.pak`: a map's pictures, ready for the graphics card (each one's
//! compressed mip chain exactly as it is uploaded), in the order the load
//! finished them, read start to end in large reads.
//!
//! Layout (little-endian): `BO2WPIC1`, the format number (u32), the key
//! (u64); then each picture: tag 1, name length (u16), name, format, width,
//! height, levels (u32 each), part (u64, `u64::MAX`: none), data length
//! (u64), packed length (u64), the data packed with lz4 (a block, no size
//! in front); then the end: tag 0, picture count (u64), data bytes (u64,
//! unpacked), `BO2WEND1`. A file without its end (cut short) is a miss.
//!
//! Format 2 (7 Nov 2026) packs each picture: the card's block formats pack
//! to about two thirds (Stronghold: 1.6 GB to 1.1 GB), and a hard drive
//! read of the file was the whole wait of a warm load. Unpacking is far
//! faster than the disk (1.7 GB/s a thread), and the reader hands the
//! packed bytes over ([`Packed`]) so the one reading thread stays on the
//! disk and the upload threads unpack.
//!
//! The file is written beside its final name and renamed when whole, on a
//! thread of its own so the load never waits on the disk.

use std::fs::File;
use std::io::{BufReader, BufWriter, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, SyncSender};

use crate::Key;

const MAGIC: &[u8; 8] = b"BO2WPIC1";
const END: &[u8; 8] = b"BO2WEND1";
/// The layout's number (raised when what is written changes).
pub const FORMAT: u32 = 2;
/// The file's name in a map's folder.
pub const FILE: &str = "pictures.pak";
const HEAD: u64 = 8 + 4 + 8;
const TAIL: u64 = 1 + 8 + 8 + 8;
/// Read and write buffer: large sequential transfers on a hard drive.
const BUF: usize = 8 << 20;

/// One picture as the graphics card takes it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Picture {
    pub name: String,
    pub format: u32,
    pub width: u32,
    pub height: u32,
    pub levels: u32,
    pub part: Option<u64>,
    pub data: Vec<u8>,
}

/// One picture still packed as the file holds it: [`Self::unpack`] on any
/// thread.
#[derive(Clone, Debug)]
pub struct Packed {
    pub name: String,
    pub format: u32,
    pub width: u32,
    pub height: u32,
    pub levels: u32,
    pub part: Option<u64>,
    /// The picture's bytes once unpacked.
    pub len: u64,
    bytes: Vec<u8>,
}

impl Packed {
    /// The picture; an error when the packed bytes are not what was written.
    pub fn unpack(self) -> std::io::Result<Picture> {
        let data = lz4_flex::block::decompress(&self.bytes, usize::try_from(self.len).unwrap_or(usize::MAX))
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, format!("pictures.pak: {}: {e}", self.name)))?;
        Ok(Picture { name: self.name, format: self.format, width: self.width, height: self.height, levels: self.levels, part: self.part, data })
    }

    /// Bytes on disk.
    pub fn packed_len(&self) -> usize {
        self.bytes.len()
    }
}

/// A whole `pictures.pak` with the expected key, opened for reading.
pub struct Reader {
    file: BufReader<File>,
    /// The file's length (an entry cannot be longer).
    len: u64,
    /// Pictures and data bytes the end says the file holds.
    pub count: u64,
    pub bytes: u64,
    read: u64,
}

impl Reader {
    /// The pak at `path` if it is whole and made with `key`.
    pub fn open(path: &Path, key: Key) -> Option<Self> {
        let mut f = File::open(path).ok()?;
        let len = f.metadata().ok()?.len();
        if len < HEAD + TAIL {
            return None;
        }
        let mut head = [0u8; HEAD as usize];
        f.read_exact(&mut head).ok()?;
        if &head[0..8] != MAGIC || u32_le(&head[8..12]) != FORMAT || u64_le(&head[12..20]) != key.value() {
            return None;
        }
        f.seek(SeekFrom::Start(len - TAIL)).ok()?;
        let mut tail = [0u8; TAIL as usize];
        f.read_exact(&mut tail).ok()?;
        if tail[0] != 0 || &tail[17..25] != END {
            return None;
        }
        let (count, bytes) = (u64_le(&tail[1..9]), u64_le(&tail[9..17]));
        f.seek(SeekFrom::Start(HEAD)).ok()?;
        Some(Self { file: BufReader::with_capacity(BUF, f), len, count, bytes, read: 0 })
    }

    /// The next picture, unpacked here, `None` at the end; an error when the
    /// file does not read back as written.
    pub fn next_picture(&mut self) -> std::io::Result<Option<Picture>> {
        self.next_packed()?.map(Packed::unpack).transpose()
    }

    /// The next picture as the file holds it (packed), `None` at the end; an
    /// error when the file does not read back as written.
    pub fn next_packed(&mut self) -> std::io::Result<Option<Packed>> {
        let bad = |what: &str| std::io::Error::new(std::io::ErrorKind::InvalidData, format!("pictures.pak: {what}"));
        let mut tag = [0u8; 1];
        self.file.read_exact(&mut tag)?;
        match tag[0] {
            0 => {
                return if self.read == self.count { Ok(None) } else { Err(bad("ends early")) };
            }
            1 => {}
            _ => return Err(bad("bad entry")),
        }
        let mut n = [0u8; 2];
        self.file.read_exact(&mut n)?;
        let mut name = vec![0u8; usize::from(u16::from_le_bytes(n))];
        self.file.read_exact(&mut name)?;
        let name = String::from_utf8(name).map_err(|_| bad("name"))?;
        let mut h = [0u8; 4 * 4 + 8 + 8 + 8];
        self.file.read_exact(&mut h)?;
        let part = u64_le(&h[16..24]);
        let len = u64_le(&h[24..32]);
        let packed = u64_le(&h[32..40]);
        if len > self.bytes || packed > self.len {
            return Err(bad("entry larger than the file"));
        }
        let mut bytes = vec![0u8; usize::try_from(packed).map_err(|_| bad("size"))?];
        self.file.read_exact(&mut bytes)?;
        self.read += 1;
        Ok(Some(Packed {
            name,
            format: u32_le(&h[0..4]),
            width: u32_le(&h[4..8]),
            height: u32_le(&h[8..12]),
            levels: u32_le(&h[12..16]),
            part: (part != u64::MAX).then_some(part),
            len,
            bytes,
        }))
    }
}

enum Msg {
    Picture(Picture),
    /// The load is over: keep the file (`true`) or drop it.
    Done(bool, String),
}

/// A `pictures.pak` being written on its own thread: [`Self::push`] each
/// picture as the load finishes it, then [`Self::finish`]. Dropped without
/// `finish(true, ..)`, nothing is kept.
pub struct Writer {
    tx: Option<SyncSender<Msg>>,
}

impl Writer {
    /// Starts writing `dir\pictures.pak` for `key`; `None` when the folder
    /// cannot be made. `on_end` hears how it went (one line).
    pub fn start(dir: &Path, key: Key, on_end: impl FnOnce(String) + Send + 'static) -> Option<Self> {
        std::fs::create_dir_all(dir).ok()?;
        // (Any older half-written file: a load that ended mid-write.)
        for e in std::fs::read_dir(dir).ok()?.flatten() {
            if e.file_name().to_string_lossy().starts_with("pictures.pak.part") {
                let _ = std::fs::remove_file(e.path());
            }
        }
        let tmp = dir.join(format!("pictures.pak.part{}", std::process::id()));
        let file = File::create(&tmp).ok()?;
        // (A few dozen pictures in flight: the load waits on the disk only
        // when it falls that far behind.)
        let (tx, rx) = std::sync::mpsc::sync_channel::<Msg>(64);
        let dir = dir.to_owned();
        std::thread::Builder::new()
            .name("bake_pictures".into())
            .spawn(move || {
                let line = write_all(file, &tmp, &dir, key, rx);
                if line.is_err() {
                    let _ = std::fs::remove_file(&tmp);
                }
                on_end(line.unwrap_or_else(|e| format!("bake: pictures not kept ({e})")));
            })
            .ok()?;
        Some(Self { tx: Some(tx) })
    }

    /// One more picture (waits only when the writer is far behind).
    pub fn push(&self, p: Picture) {
        if let Some(tx) = &self.tx {
            let _ = tx.send(Msg::Picture(p));
        }
    }

    /// The load is over: `keep` (every picture it wants was pushed, at the
    /// size the key says) makes the file the map's; `manifest` is written
    /// beside it.
    pub fn finish(mut self, keep: bool, manifest: String) {
        if let Some(tx) = self.tx.take() {
            let _ = tx.send(Msg::Done(keep, manifest));
        }
    }
}

fn write_all(file: File, tmp: &Path, dir: &Path, key: Key, rx: Receiver<Msg>) -> Result<String, String> {
    let t0 = std::time::Instant::now();
    let mut w = BufWriter::with_capacity(BUF, file);
    let io = |e: std::io::Error| e.to_string();
    w.write_all(MAGIC).map_err(io)?;
    w.write_all(&FORMAT.to_le_bytes()).map_err(io)?;
    w.write_all(&key.value().to_le_bytes()).map_err(io)?;
    let (mut count, mut bytes, mut packed) = (0u64, 0u64, 0u64);
    let manifest = loop {
        match rx.recv() {
            Ok(Msg::Picture(p)) => {
                let name = p.name.as_bytes();
                let n = u16::try_from(name.len()).map_err(|_| format!("name too long: {}", p.name))?;
                w.write_all(&[1]).map_err(io)?;
                w.write_all(&n.to_le_bytes()).map_err(io)?;
                w.write_all(name).map_err(io)?;
                for v in [p.format, p.width, p.height, p.levels] {
                    w.write_all(&v.to_le_bytes()).map_err(io)?;
                }
                w.write_all(&p.part.unwrap_or(u64::MAX).to_le_bytes()).map_err(io)?;
                let block = lz4_flex::block::compress(&p.data);
                w.write_all(&(p.data.len() as u64).to_le_bytes()).map_err(io)?;
                w.write_all(&(block.len() as u64).to_le_bytes()).map_err(io)?;
                w.write_all(&block).map_err(io)?;
                count += 1;
                bytes += p.data.len() as u64;
                packed += block.len() as u64;
            }
            Ok(Msg::Done(true, manifest)) => break manifest,
            Ok(Msg::Done(false, why)) => return Err(why),
            Err(_) => return Err("the load ended before the pictures did".into()),
        }
    };
    w.write_all(&[0]).map_err(io)?;
    w.write_all(&count.to_le_bytes()).map_err(io)?;
    w.write_all(&bytes.to_le_bytes()).map_err(io)?;
    w.write_all(END).map_err(io)?;
    let f = w.into_inner().map_err(|e| e.error().to_string())?;
    f.sync_all().map_err(io)?;
    drop(f);
    let to: PathBuf = dir.join(FILE);
    let _ = std::fs::remove_file(&to);
    std::fs::rename(tmp, &to).map_err(io)?;
    let _ = std::fs::write(dir.join("manifest.json"), manifest);
    Ok(format!("bake: {count} pictures ({:.0} MB, {:.0} MB packed) kept in {} ({:.1}s)", bytes as f64 / 1048576.0, packed as f64 / 1048576.0, to.display(), t0.elapsed().as_secs_f32()))
}

fn u32_le(b: &[u8]) -> u32 {
    let mut a = [0u8; 4];
    a.copy_from_slice(&b[..4]);
    u32::from_le_bytes(a)
}

fn u64_le(b: &[u8]) -> u64 {
    let mut a = [0u8; 8];
    a.copy_from_slice(&b[..8]);
    u64::from_le_bytes(a)
}

/// A JSON string (for the manifests).
pub fn json_str(s: &str) -> String {
    let mut o = String::with_capacity(s.len() + 2);
    o.push('"');
    for c in s.chars() {
        match c {
            '"' => o.push_str("\\\""),
            '\\' => o.push_str("\\\\"),
            c if (c as u32) < 0x20 => o.push_str(&format!("\\u{:04x}", c as u32)),
            c => o.push(c),
        }
    }
    o.push('"');
    o
}
