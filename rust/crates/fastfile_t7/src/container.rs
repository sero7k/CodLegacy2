//! The `.ff` envelope of Black Ops III (PC): a fixed `XFile` header, then
//! the zone image as zlib blocks.
//!
//! ```text
//! 0x000 magic        "TAff0000"
//! 0x008 version      u32 = 0x251
//! 0x00c server u8, compression u8 (1 = zlib), platform u8, encrypted u8
//! 0x010 timestamp    u64
//! 0x018 changelist   u32
//! 0x01c checksum     [u32; 4]
//! 0x02c builder      char[32]   ("CODBUILD15-764")
//! 0x04c u32, then 64 bytes (zero in every retail zone read)
//! 0x090 image size   u64        (= the inflated size, exactly)
//! 0x098 u64, u64     (zero)
//! 0x0a8 block sizes  [u64; 10]  (XFILE_BLOCK_*: TEMP, RUNTIME_VIRTUAL,
//!                    RUNTIME_PHYSICAL, DELAY_VIRTUAL, DELAY_PHYSICAL,
//!                    VIRTUAL, PHYSICAL, STREAMER_RESERVE, STREAMER, MEMMAPPED)
//! 0x0f8 zone name    char[64]
//! 0x138 signature    [u8; 256]
//! 0x238 16 bytes     (zero)
//! 0x248 blocks       { u32 compressed; u32 raw; u32 stored; u32 at; data[stored] }
//! ```
//!
//! Each block is a 16-byte header and a zlib stream (with the zlib header):
//! `compressed` bytes of stream, `stored` = `compressed` rounded up to 4
//! (the next block's header follows the stored bytes), `at` = the file
//! offset of the block's own header, `raw` = its inflated size (0x381b0 for
//! every block but the last). A header with `raw == 0` is not a block: the
//! writer does not let a block cross an 8 MiB boundary of the file, and
//! leaves the rest of the 8 MiB as filler behind such a header; the next
//! block starts at the boundary. An all-zero header ends the data. (Read on
//! retail `mp_patch` and `mp_common`: the blocks inflate to exactly the
//! image size of the header.)

pub const MAGIC: &[u8; 8] = b"TAff0000";
/// Black Ops III PC.
pub const ZONE_VERSION: u32 = 0x251;
pub const HEADER_LEN: usize = 0x248;
pub const BLOCK_HEAD_LEN: usize = 16;
pub const BLOCK_COUNT: usize = 10;
/// Blocks never cross this boundary of the file.
pub const CHUNK: usize = 0x80_0000;
pub const COMPRESSION_ZLIB: u8 = 1;

#[derive(Debug)]
pub enum ContainerError {
    Io(std::io::Error),
    Truncated,
    BadMagic([u8; 8]),
    WrongVersion(u32),
    Encrypted,
    Compression(u8),
    Inflate(String),
}

impl std::fmt::Display for ContainerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(e) => write!(f, "reading zone: {e}"),
            Self::Truncated => write!(f, "zone file is shorter than its header"),
            Self::BadMagic(m) => write!(f, "not a Black Ops III zone (magic {:02x?})", m),
            Self::WrongVersion(v) => write!(f, "zone version {v:#x} is not Black Ops III PC ({ZONE_VERSION:#x})"),
            Self::Encrypted => write!(f, "encrypted zone (not supported)"),
            Self::Compression(c) => write!(f, "zone compression {c} is not zlib"),
            Self::Inflate(e) => write!(f, "zone data does not decompress: {e}"),
        }
    }
}

impl std::error::Error for ContainerError {}

impl From<std::io::Error> for ContainerError {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileHeader {
    pub version: u32,
    pub server: u8,
    pub compression: u8,
    pub platform: u8,
    pub encrypted: u8,
    pub timestamp: u64,
    pub changelist: u32,
    pub builder: String,
    pub name: String,
    /// The inflated image's size.
    pub size: u64,
    pub block_sizes: [u64; BLOCK_COUNT],
}

fn u32_at(b: &[u8], o: usize) -> u32 {
    u32::from_le_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]])
}
fn u64_at(b: &[u8], o: usize) -> u64 {
    let mut a = [0u8; 8];
    a.copy_from_slice(&b[o..o + 8]);
    u64::from_le_bytes(a)
}
fn fixed_str(b: &[u8]) -> String {
    let n = b.iter().position(|&c| c == 0).unwrap_or(b.len());
    String::from_utf8_lossy(&b[..n]).into_owned()
}

/// Is this the start of a Black Ops III PC zone?
pub fn is_t7_zone(head: &[u8]) -> bool {
    head.len() >= 12 && &head[..8] == MAGIC && u32_at(head, 8) == ZONE_VERSION
}

pub fn parse_file_header(file: &[u8]) -> Result<FileHeader, ContainerError> {
    if file.len() < HEADER_LEN {
        return Err(ContainerError::Truncated);
    }
    let mut magic = [0u8; 8];
    magic.copy_from_slice(&file[..8]);
    if &magic != MAGIC {
        return Err(ContainerError::BadMagic(magic));
    }
    let version = u32_at(file, 8);
    if version != ZONE_VERSION {
        return Err(ContainerError::WrongVersion(version));
    }
    let mut block_sizes = [0u64; BLOCK_COUNT];
    for (i, b) in block_sizes.iter_mut().enumerate() {
        *b = u64_at(file, 0xa8 + 8 * i);
    }
    Ok(FileHeader {
        version,
        server: file[0xc],
        compression: file[0xd],
        platform: file[0xe],
        encrypted: file[0xf],
        timestamp: u64_at(file, 0x10),
        changelist: u32_at(file, 0x18),
        builder: fixed_str(&file[0x2c..0x4c]),
        name: fixed_str(&file[0xf8..0x138]),
        size: u64_at(file, 0x90),
        block_sizes,
    })
}

/// Inflates the zlib blocks from `HEADER_LEN` on into the image of `size` bytes.
///
/// bo2mp: each block is a zlib stream of its own, so a zone of more than a
/// few is inflated on several threads at once, each block straight into its
/// place in the image (one thread took core_common's 271 MB, the last shared
/// zone a Skyjacked load waited for). `BO2MP_T7_INFLATE_PAR=0`: one block
/// after another (the old way).
pub fn inflate_blocks(file: &[u8], size: u64) -> Result<Vec<u8>, ContainerError> {
    let workers = if codlegacy_env::var("BO2MP_T7_INFLATE_PAR").is_ok_and(|v| v.trim() == "0") {
        1
    } else {
        std::thread::available_parallelism().map_or(1, usize::from).min(8)
    };
    inflate_blocks_on(file, size, workers)
}

/// One zlib block: where it starts, its packed bytes and its inflated size.
struct Block {
    at: usize,
    src: std::ops::Range<usize>,
    raw: usize,
}

/// The image's size, checked.
fn image_len(size: u64) -> Result<usize, ContainerError> {
    let total = usize::try_from(size).map_err(|_| ContainerError::Truncated)?;
    if total > 1 << 34 {
        return Err(ContainerError::Inflate(format!("image size {total:#x}")));
    }
    Ok(total)
}

/// The blocks from `HEADER_LEN` on (the checks [`inflate_serial`] makes,
/// without inflating): their sizes must add up to `total`.
fn scan_blocks(file: &[u8], total: usize) -> Result<Vec<Block>, ContainerError> {
    let mut blocks = Vec::new();
    let mut sum = 0usize;
    let mut p = HEADER_LEN;
    loop {
        let head = file
            .get(p..p + BLOCK_HEAD_LEN)
            .ok_or_else(|| ContainerError::Inflate(format!("data ends at {p:#x} with {sum:#x} of {total:#x} bytes")))?;
        let packed = u32_at(head, 0) as usize;
        let raw = u32_at(head, 4) as usize;
        let stored = u32_at(head, 8) as usize;
        let at = u32_at(head, 12) as usize;
        if packed == 0 && raw == 0 {
            break;
        }
        if at != p & 0xffff_ffff {
            return Err(ContainerError::Inflate(format!("block at {p:#x} names offset {at:#x}")));
        }
        if raw == 0 {
            // Filler up to the next 8 MiB boundary.
            p = (p / CHUNK + 1) * CHUNK;
            continue;
        }
        if stored < packed || raw > 1 << 24 {
            return Err(ContainerError::Inflate(format!("block at {p:#x}: {packed:#x} -> {raw:#x} (stored {stored:#x})")));
        }
        let src = p + BLOCK_HEAD_LEN..p + BLOCK_HEAD_LEN + packed;
        if file.get(src.clone()).is_none() {
            return Err(ContainerError::Truncated);
        }
        sum += raw;
        if sum > total {
            return Err(ContainerError::Inflate(format!("blocks inflate to more than the header's {total:#x} (block at {p:#x})")));
        }
        blocks.push(Block { at: p, src, raw });
        p += BLOCK_HEAD_LEN + stored;
    }
    if sum != total {
        return Err(ContainerError::Inflate(format!("blocks inflate to {sum:#x}, header says {total:#x}")));
    }
    Ok(blocks)
}

/// One block into `dst` (exactly its inflated size).
fn inflate_one(file: &[u8], b: &Block, dst: &mut [u8]) -> Result<(), ContainerError> {
    let mut z = flate2::Decompress::new(true);
    let st = z
        .decompress(&file[b.src.clone()], dst, flate2::FlushDecompress::Finish)
        .map_err(|e| ContainerError::Inflate(format!("block at {:#x}: {e}", b.at)))?;
    if st != flate2::Status::StreamEnd || z.total_out() != b.raw as u64 {
        return Err(ContainerError::Inflate(format!("block at {:#x} does not inflate to {:#x} ({:#x} out, {st:?})", b.at, b.raw, z.total_out())));
    }
    Ok(())
}

/// A block and its slice of the image, until a thread takes it.
type Job<'a> = std::sync::Mutex<Option<(&'a Block, &'a mut [u8])>>;

/// [`inflate_blocks`] on up to `workers` threads (1: [`inflate_serial`]).
fn inflate_blocks_on(file: &[u8], size: u64, workers: usize) -> Result<Vec<u8>, ContainerError> {
    use std::sync::atomic::{AtomicUsize, Ordering};
    if workers <= 1 {
        return inflate_serial(file, size);
    }
    let total = image_len(size)?;
    let blocks = scan_blocks(file, total)?;
    if blocks.len() < 4 {
        return inflate_serial(file, size);
    }
    let mut out = vec![0u8; total];
    // (Each block's own slice of the image, taken once by whichever thread
    // gets to it first.)
    let mut jobs: Vec<Job> = Vec::with_capacity(blocks.len());
    let mut rest: &mut [u8] = &mut out;
    for b in &blocks {
        let (dst, tail) = std::mem::take(&mut rest).split_at_mut(b.raw);
        jobs.push(std::sync::Mutex::new(Some((b, dst))));
        rest = tail;
    }
    let next = AtomicUsize::new(0);
    let failed: std::sync::Mutex<Option<(usize, ContainerError)>> = std::sync::Mutex::new(None);
    std::thread::scope(|s| {
        for _ in 0..workers.min(blocks.len()) {
            s.spawn(|| {
                loop {
                    let i = next.fetch_add(1, Ordering::Relaxed);
                    let Some(job) = jobs.get(i) else { break };
                    let Some((b, dst)) = job.lock().ok().and_then(|mut j| j.take()) else { continue };
                    if let Err(e) = inflate_one(file, b, dst)
                        && let Ok(mut f) = failed.lock()
                    {
                        // (The first block's error, as one thread would say.)
                        if f.as_ref().is_none_or(|(k, _)| i < *k) {
                            *f = Some((i, e));
                        }
                        next.store(jobs.len(), Ordering::Relaxed);
                    }
                }
            });
        }
    });
    if let Some((_, e)) = failed.into_inner().ok().flatten() {
        return Err(e);
    }
    Ok(out)
}

/// One block after another on this thread (the old way).
fn inflate_serial(file: &[u8], size: u64) -> Result<Vec<u8>, ContainerError> {
    use std::io::Read;
    let total = usize::try_from(size).map_err(|_| ContainerError::Truncated)?;
    if total > 1 << 34 {
        return Err(ContainerError::Inflate(format!("image size {total:#x}")));
    }
    let mut out = Vec::with_capacity(total);
    let mut p = HEADER_LEN;
    loop {
        let head = file
            .get(p..p + BLOCK_HEAD_LEN)
            .ok_or_else(|| ContainerError::Inflate(format!("data ends at {p:#x} with {:#x} of {total:#x} bytes", out.len())))?;
        let packed = u32_at(head, 0) as usize;
        let raw = u32_at(head, 4) as usize;
        let stored = u32_at(head, 8) as usize;
        let at = u32_at(head, 12) as usize;
        if packed == 0 && raw == 0 {
            break;
        }
        if at != p & 0xffff_ffff {
            return Err(ContainerError::Inflate(format!("block at {p:#x} names offset {at:#x}")));
        }
        if raw == 0 {
            // Filler up to the next 8 MiB boundary.
            p = (p / CHUNK + 1) * CHUNK;
            continue;
        }
        if stored < packed || raw > 1 << 24 {
            return Err(ContainerError::Inflate(format!("block at {p:#x}: {packed:#x} -> {raw:#x} (stored {stored:#x})")));
        }
        let src = file
            .get(p + BLOCK_HEAD_LEN..p + BLOCK_HEAD_LEN + packed)
            .ok_or(ContainerError::Truncated)?;
        let start = out.len();
        let mut z = flate2::read::ZlibDecoder::new(src);
        z.read_to_end(&mut out)
            .map_err(|e| ContainerError::Inflate(format!("block at {p:#x}: {e}")))?;
        if out.len() - start != raw {
            return Err(ContainerError::Inflate(format!(
                "block at {p:#x} inflates to {:#x}, not {raw:#x}",
                out.len() - start
            )));
        }
        p += BLOCK_HEAD_LEN + stored;
    }
    if out.len() != total {
        return Err(ContainerError::Inflate(format!("blocks inflate to {:#x}, header says {total:#x}", out.len())));
    }
    Ok(out)
}

/// Inflates a whole `.ff` file to the zone image.
pub fn inflate_zone(file: &[u8]) -> Result<(FileHeader, Vec<u8>), ContainerError> {
    let header = parse_file_header(file)?;
    if header.encrypted != 0 {
        return Err(ContainerError::Encrypted);
    }
    if header.compression != COMPRESSION_ZLIB {
        return Err(ContainerError::Compression(header.compression));
    }
    let image = inflate_blocks(file, header.size)?;
    Ok((header, image))
}

/// Reads and inflates the zone at `path`.
pub fn open_zone_file(path: &std::path::Path) -> Result<(FileHeader, Vec<u8>), ContainerError> {
    let file = std::fs::read(path)?;
    inflate_zone(&file)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn zlib(data: &[u8]) -> Vec<u8> {
        let mut e = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
        e.write_all(data).unwrap();
        e.finish().unwrap()
    }

    #[test]
    fn inflates_chained_blocks() {
        let mut file = vec![0u8; HEADER_LEN];
        file[..8].copy_from_slice(MAGIC);
        file[8..12].copy_from_slice(&ZONE_VERSION.to_le_bytes());
        file[0xd] = COMPRESSION_ZLIB;
        let parts: [&[u8]; 2] = [b"hello, ", b"zone"];
        for part in parts {
            let z = zlib(part);
            let stored = (z.len() + 3) & !3;
            let at = file.len() as u32;
            for v in [z.len() as u32, part.len() as u32, stored as u32, at] {
                file.extend_from_slice(&v.to_le_bytes());
            }
            file.extend_from_slice(&z);
            file.resize(file.len() + stored - z.len(), 0);
        }
        file.extend_from_slice(&[0; 16]);
        file[0x90..0x98].copy_from_slice(&11u64.to_le_bytes());
        let (h, img) = inflate_zone(&file).unwrap();
        assert_eq!(h.size, 11);
        assert_eq!(img, b"hello, zone");
    }

    /// A zone of `parts` blocks, with a filler block after the second.
    fn zone_of(parts: &[Vec<u8>]) -> (Vec<u8>, u64) {
        let mut file = vec![0u8; HEADER_LEN];
        let mut size = 0u64;
        for (i, part) in parts.iter().enumerate() {
            let z = zlib(part);
            let stored = (z.len() + 3) & !3;
            let at = file.len() as u32;
            for v in [z.len() as u32, part.len() as u32, stored as u32, at] {
                file.extend_from_slice(&v.to_le_bytes());
            }
            file.extend_from_slice(&z);
            file.resize(file.len() + stored - z.len(), 0);
            size += part.len() as u64;
            if i == 1 {
                // Filler (packed bytes but no inflated size; 0 and 0 would
                // end the list): the next block starts at the next 8 MiB
                // boundary.
                let at = file.len() as u32;
                for v in [1u32, 0, 0, at] {
                    file.extend_from_slice(&v.to_le_bytes());
                }
                file.resize(CHUNK, 0);
            }
        }
        file.extend_from_slice(&[0; 16]);
        (file, size)
    }

    #[test]
    fn threads_inflate_the_same_image() {
        let parts: Vec<Vec<u8>> = (0..23u32).map(|i| (0..(1000 + i * 337)).map(|k| (k.wrapping_mul(i + 7) % 251) as u8).collect()).collect();
        let (file, size) = zone_of(&parts);
        let one = inflate_blocks_on(&file, size, 1).unwrap();
        assert_eq!(one, parts.concat());
        for workers in [2, 3, 8] {
            assert_eq!(inflate_blocks_on(&file, size, workers).unwrap(), one);
        }
        // A size the blocks do not add up to, and a broken block, fail
        // both ways.
        assert!(inflate_blocks_on(&file, size + 1, 4).is_err());
        assert!(inflate_blocks_on(&file, size + 1, 1).is_err());
        let mut bad = file.clone();
        // (The second block's zlib header.)
        let second = HEADER_LEN + BLOCK_HEAD_LEN + ((zlib(&parts[0]).len() + 3) & !3);
        bad[second + BLOCK_HEAD_LEN] ^= 0xff;
        assert!(inflate_blocks_on(&bad, size, 4).is_err());
        assert!(inflate_blocks_on(&bad, size, 1).is_err());
    }
}
