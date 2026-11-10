//! Local index files `Data/data/BBVVVVVVVV.idx` (version 7).
//!
//! ```text
//! u32 header length, u32 header hash,
//! u16 version 7, u8 bucket, u8 extra, u8 size bytes (4), u8 offset bytes (5),
//! u8 key bytes (9), u8 archive-offset bits (30), u64 archive total size
//! (pad to 16) u32 entries length, u32 entries hash
//! entries: key[9], offset[5] big-endian (archive << bits | offset), size[4] little-endian
//! ```
//! The launcher keeps old generations next to the current one; only the highest version per
//! bucket (`BB`) counts ([`current_files`]).

use crate::{Result, fmt_err};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Where one encoded file is: `data.{archive:03}` at `offset`, `size` bytes including the
/// 30-byte entry header. A `size` of 30 is a header with no data (not resident).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Location {
    pub archive: u32,
    pub offset: u64,
    pub size: u32,
}

/// Length of the per-entry header in `data.NNN` (reversed EKey, size, flags, checksums).
pub const ENTRY_HEADER: u32 = 30;

impl Location {
    pub fn resident(&self) -> bool {
        self.size > ENTRY_HEADER
    }
}

/// Key prefix (9 bytes) -> location.
pub type Index = HashMap<[u8; 9], Location>;

/// Parse one `.idx` file into `out`.
pub fn parse(b: &[u8], out: &mut Index) -> Result<()> {
    let u32le = |o: usize| -> Option<u32> { Some(u32::from_le_bytes(b.get(o..o + 4)?.try_into().ok()?)) };
    let hl = u32le(0).ok_or_else(|| crate::Error::Format("idx too short".into()))? as usize;
    let h = b.get(8..16).ok_or_else(|| crate::Error::Format("idx header".into()))?;
    let version = u16::from_le_bytes([h[0], h[1]]);
    let (ss, os, ks, bits) = (h[4] as usize, h[5] as usize, h[6] as usize, h[7] as u32);
    if version != 7 || ss != 4 || os != 5 || ks != 9 || bits != 30 {
        return fmt_err(format!("idx v{version} spec {ss}/{os}/{ks}/{bits} (want 7, 4/5/9/30)"));
    }
    let pos = (8 + hl + 15) & !15;
    let len = u32le(pos).ok_or_else(|| crate::Error::Format("idx entries header".into()))? as usize;
    let es = ss + os + ks;
    let entries = b
        .get(pos + 8..pos + 8 + len)
        .ok_or_else(|| crate::Error::Format("idx entries past end".into()))?;
    for e in entries.chunks_exact(es) {
        let mut key = [0u8; 9];
        key.copy_from_slice(&e[..9]);
        let mut o = 0u64;
        for &x in &e[9..14] {
            o = (o << 8) | x as u64;
        }
        let size = u32::from_le_bytes(e[14..18].try_into().unwrap());
        out.insert(
            key,
            Location { archive: (o >> bits) as u32, offset: o & ((1 << bits) - 1), size },
        );
    }
    Ok(())
}

/// The current `.idx` of each bucket in `dir` (highest version wins).
pub fn current_files(dir: &Path) -> Result<Vec<PathBuf>> {
    let mut best: HashMap<u8, (u32, PathBuf)> = HashMap::new();
    for e in std::fs::read_dir(dir)? {
        let p = e?.path();
        let Some(name) = p.file_name().and_then(|n| n.to_str()) else { continue };
        let Some(stem) = name.strip_suffix(".idx") else { continue };
        if stem.len() != 10 {
            continue;
        }
        let (Ok(bucket), Ok(ver)) = (u8::from_str_radix(&stem[..2], 16), u32::from_str_radix(&stem[2..], 16)) else {
            continue;
        };
        if best.get(&bucket).is_none_or(|(v, _)| ver > *v) {
            best.insert(bucket, (ver, p));
        }
    }
    let mut v: Vec<_> = best.into_iter().collect();
    v.sort_by_key(|(b, _)| *b);
    Ok(v.into_iter().map(|(_, (_, p))| p).collect())
}

/// Read all current indexes of `Data/data`.
pub fn load_dir(dir: &Path) -> Result<Index> {
    let mut out = Index::new();
    for p in current_files(dir)? {
        parse(&std::fs::read(&p)?, &mut out)?;
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn synth(entries: &[([u8; 9], u32, u64, u32)]) -> Vec<u8> {
        let mut b = vec![0u8; 0x28];
        b[0..4].copy_from_slice(&0x10u32.to_le_bytes());
        b[8..10].copy_from_slice(&7u16.to_le_bytes());
        b[10] = 3;
        b[12..16].copy_from_slice(&[4, 5, 9, 30]);
        b.truncate(0x20);
        b.extend_from_slice(&((entries.len() * 18) as u32).to_le_bytes());
        b.extend_from_slice(&[0; 4]);
        for (k, a, o, s) in entries {
            b.extend_from_slice(k);
            let v = ((*a as u64) << 30) | o;
            b.extend_from_slice(&v.to_be_bytes()[3..]);
            b.extend_from_slice(&s.to_le_bytes());
        }
        b
    }

    #[test]
    fn parses_v7_entries() {
        // The encoding manifest's real location in the Cold War install.
        let k = [0x37, 0x24, 0xaa, 0x3e, 0xd6, 0xa0, 0x88, 0x4d, 0x1f];
        let b = synth(&[(k, 0, 480, 413_153), ([1; 9], 97, 0x3fff_ffff, 30)]);
        let mut ix = Index::new();
        parse(&b, &mut ix).unwrap();
        assert_eq!(ix[&k], Location { archive: 0, offset: 480, size: 413_153 });
        let l = ix[&[1; 9]];
        assert_eq!((l.archive, l.offset, l.resident()), (97, 0x3fff_ffff, false));
    }
}
