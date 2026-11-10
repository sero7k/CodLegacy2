//! Infinite Warfare sound banks (`<zone>.sabl` loaded, `<zone>.sabs`
//! streamed, beside the zones): Black Ops II's `2UX#` bank, IW7's version.
//!
//! ```text
//! header (2048)  0x00 "2UX#"; 0x04 u32 version (4); 0x08 u32 entry size (44);
//!                0x0c u32 (16); 0x10 u32 dependency name size (64); 0x14 u32 entry count;
//!                0x18 u32; 0x1c u32; 0x20 u64 file size; 0x28 u64 entry table offset;
//!                0x30 u64 second table offset; 0x38 16 bytes (checksum)
//! entry (44)     0x00 u32 id (the alias's `assetId`); 0x04 u32 data size; 0x08 u32 seek
//!                table size; 0x0c u32 frame count; 0x10 u32 flags; 0x14 u64 offset; 0x1c u32 frame rate;
//!                0x20 u8 channels; 0x21 u8 looping; 0x22 u8 format (8: FLAC)
//! sound          at its offset: its seek table (18-byte points, little endian:
//!                u64 sample, u64 byte offset, u16 frame samples), then its FLAC
//!                frames, with no `fLaC` stream header
//! ```
//! (Read on retail `common_core_mp`, `patch_common_core_mp` and
//! `patch_common_mp` banks: every entry FLAC, 16-bit, 44.1 or 48 kHz, the
//! first at 0x1000.)

use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

pub const MAGIC: &[u8; 4] = b"2UX#";
pub const VERSION: u32 = 4;
pub const HEADER_SIZE: usize = 2048;
pub const ENTRY_SIZE: usize = 44;
pub const FORMAT_FLAC: u8 = 8;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SabEntry {
    pub id: u32,
    pub size: u32,
    pub seek_size: u32,
    pub frames: u32,
    /// The word after the frame count (bit 11 set on most title-update
    /// sounds; its meaning is not known).
    pub flags: u32,
    pub offset: u64,
    pub rate: u32,
    pub channels: u8,
    pub looping: bool,
    pub format: u8,
}

impl SabEntry {
    /// Where its FLAC frames start, and how many bytes they are.
    pub fn frames_range(&self) -> (u64, u32) {
        (self.offset + u64::from(self.seek_size), self.size)
    }
}

/// One opened bank: its entries sorted by id; the sounds' bytes are read
/// on demand.
#[derive(Clone, Debug, Default)]
pub struct SabFile {
    pub path: PathBuf,
    pub file_size: u64,
    pub entries: Vec<SabEntry>,
}

fn u32_at(b: &[u8], at: usize) -> u32 {
    u32::from_le_bytes([b[at], b[at + 1], b[at + 2], b[at + 3]])
}

fn u64_at(b: &[u8], at: usize) -> u64 {
    u64::from(u32_at(b, at)) | (u64::from(u32_at(b, at + 4)) << 32)
}

pub fn parse_entry(e: &[u8]) -> SabEntry {
    SabEntry {
        id: u32_at(e, 0),
        size: u32_at(e, 4),
        seek_size: u32_at(e, 8),
        frames: u32_at(e, 0x0c),
        flags: u32_at(e, 0x10),
        offset: u64_at(e, 0x14),
        rate: u32_at(e, 0x1c),
        channels: e[0x20],
        looping: e[0x21] != 0,
        format: e[0x22],
    }
}

impl SabFile {
    pub fn open(path: &Path) -> Result<Self, String> {
        let err = |e: String| format!("{}: {e}", path.display());
        let mut f = std::fs::File::open(path).map_err(|e| err(e.to_string()))?;
        let mut head = vec![0u8; HEADER_SIZE];
        f.read_exact(&mut head).map_err(|e| err(format!("header: {e}")))?;
        if &head[..4] != MAGIC {
            return Err(err("not a sound bank".into()));
        }
        let version = u32_at(&head, 4);
        let entry_size = u32_at(&head, 8) as usize;
        if version != VERSION || entry_size != ENTRY_SIZE {
            return Err(err(format!("bank version {version} entry size {entry_size}, want {VERSION} / {ENTRY_SIZE}")));
        }
        let count = u32_at(&head, 0x14) as usize;
        let file_size = u64_at(&head, 0x20);
        let mut table = vec![0u8; count * ENTRY_SIZE];
        if count > 0 {
            f.seek(SeekFrom::Start(u64_at(&head, 0x28))).map_err(|e| err(e.to_string()))?;
            f.read_exact(&mut table).map_err(|e| err(format!("entry table: {e}")))?;
        }
        let mut entries: Vec<SabEntry> = table.chunks_exact(ENTRY_SIZE).map(parse_entry).collect();
        entries.sort_by_key(|e| e.id);
        Ok(Self { path: path.to_path_buf(), file_size, entries })
    }

    pub fn find(&self, id: u32) -> Option<&SabEntry> {
        self.entries.binary_search_by_key(&id, |e| e.id).ok().map(|i| &self.entries[i])
    }
}

/// The bank files of `zone` in `dir` (`<zone>.sabl`, `.sabs`, or the
/// `iw7_`-prefixed copies) that are there.
pub fn bank_paths(dir: &Path, zone: &str) -> Vec<PathBuf> {
    ["sabl", "sabs"]
        .iter()
        .filter_map(|ext| {
            [format!("{zone}.{ext}"), format!("iw7_{zone}.{ext}")].into_iter().map(|n| dir.join(n)).find(|p| p.is_file())
        })
        .collect()
}

/// A bank's FLAC frames (`offset`, `length` in `path`) as a FLAC stream a
/// decoder opens: a `fLaC` header and a STREAMINFO block (its rate,
/// channels, bit depth and block size from the first frame's header) in
/// front of them.
pub fn read_flac(path: &Path, offset: u64, length: u32, frames: u64, rate: u32) -> Result<Vec<u8>, String> {
    let mut f = std::fs::File::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
    f.seek(SeekFrom::Start(offset)).map_err(|e| e.to_string())?;
    let mut body = vec![0u8; length as usize];
    f.read_exact(&mut body).map_err(|e| format!("{}: {offset:#x}+{length}: {e}", path.display()))?;
    let mut out = flac_header(&body, frames, rate).ok_or_else(|| format!("{}: {offset:#x}: not a FLAC frame", path.display()))?;
    out.extend_from_slice(&body);
    Ok(out)
}

/// `fLaC` and a STREAMINFO block for frames starting with `body` (no frame
/// sizes and no MD5: unknown); `rate` when the frame's header gives its own
/// at its end.
pub fn flac_header(body: &[u8], frames: u64, rate: u32) -> Option<Vec<u8>> {
    if body.len() < 6 || body[0] != 0xff || body[1] & 0xfe != 0xf8 {
        return None;
    }
    let rate: u32 = match body[2] & 0x0f {
        1 => 88200,
        2 => 176400,
        3 => 192000,
        4 => 8000,
        5 => 16000,
        6 => 22050,
        7 => 24000,
        8 => 32000,
        9 => 44100,
        10 => 48000,
        11 => 96000,
        12..=14 if rate > 0 => rate,
        _ => return None,
    };
    let block: u32 = match body[2] >> 4 {
        1 => 192,
        n @ 2..=5 => 576 << (n - 2),
        n @ 8..=15 => 256 << (n - 8),
        // (Read from the end of the frame header: the largest kept.)
        _ => 65535,
    };
    let channels = match body[3] >> 4 {
        n @ 0..=7 => u32::from(n) + 1,
        8..=10 => 2,
        _ => return None,
    };
    let bits: u32 = match (body[3] >> 1) & 7 {
        1 => 8,
        2 => 12,
        4 => 16,
        5 => 20,
        6 => 24,
        _ => 16,
    };
    let mut out = Vec::with_capacity(42);
    out.extend_from_slice(b"fLaC");
    out.extend_from_slice(&[0x80, 0, 0, 34]);
    // (A fixed block size is given as both the least and the most: a
    // decoder reads a stream whose two differ as of variable size.)
    let block = block.clamp(16, 65535) as u16;
    let least = if body[1] & 1 == 0 { block } else { 16 };
    out.extend_from_slice(&least.to_be_bytes());
    out.extend_from_slice(&block.to_be_bytes());
    out.extend_from_slice(&[0; 6]);
    let packed: u64 = (u64::from(rate) << 44) | (u64::from(channels - 1) << 41) | (u64::from(bits - 1) << 36) | (frames & 0xf_ffff_ffff);
    out.extend_from_slice(&packed.to_be_bytes());
    out.extend_from_slice(&[0; 16]);
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entry_and_header() {
        let mut e = [0u8; ENTRY_SIZE];
        e[0..4].copy_from_slice(&0x712d_08bdu32.to_le_bytes());
        e[4..8].copy_from_slice(&0xd8f6u32.to_le_bytes());
        e[8..12].copy_from_slice(&18u32.to_le_bytes());
        e[0x0c..0x10].copy_from_slice(&0x5ed5u32.to_le_bytes());
        e[0x10..0x14].copy_from_slice(&0x800u32.to_le_bytes());
        e[0x14..0x1c].copy_from_slice(&0x6122u64.to_le_bytes());
        e[0x1c..0x20].copy_from_slice(&44100u32.to_le_bytes());
        e[0x20] = 2;
        e[0x22] = 8;
        let p = parse_entry(&e);
        assert_eq!((p.id, p.frames, p.rate, p.channels, p.format), (0x712d_08bd, 0x5ed5, 44100, 2, FORMAT_FLAC));
        assert_eq!(p.frames_range(), (0x6134, 0xd8f6));
        // A 1024-sample, 44.1 kHz, left/side stereo, 16-bit frame.
        let h = flac_header(&[0xff, 0xf8, 0xa9, 0xa8, 0, 0], 11375, 0).expect("frame");
        assert_eq!(&h[..8], b"fLaC\x80\x00\x00\x22");
        assert_eq!((u16::from_be_bytes([h[8], h[9]]), u16::from_be_bytes([h[10], h[11]])), (1024, 1024));
        let packed = u64::from_be_bytes(h[18..26].try_into().unwrap());
        assert_eq!((packed >> 44, (packed >> 41) & 7, (packed >> 36) & 31, packed & 0xf_ffff_ffff), (44100, 1, 15, 11375));
        // A frame whose rate is at the end of its header takes the bank's.
        let h = flac_header(&[0xff, 0xf8, 0xac, 0xa8, 0, 0], 1, 48000).expect("frame");
        assert_eq!(u64::from_be_bytes(h[18..26].try_into().unwrap()) >> 44, 48000);
    }
}
