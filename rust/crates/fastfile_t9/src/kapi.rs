//! `.xpak` package index, `KAPI` version 0x10 (MWIII's is 0x17; see `fastfile_jup::xsub`).
//!
//! ```text
//! 0x000 "KAPI", u16 4, u16 0x10, u64 1, u32 2, u32 7, u64 file length, u64 hash,
//! 0x028 u64 part count (core.xpak: 74 -> core-00000.xsub .. core-00073.xsub)
//! 0x7a0 u64 row count, u64 table offset (0x8000), u64 table size (= rows * 16)
//! 0x800 .. table: 11-byte (u64 key, u24 index) rows, probably a bucket index (not used)
//! table rows: u64 key (sorted ascending), u64 packed location
//! ```
//! The packed word's top byte takes as many distinct values as there are parts. How the rest
//! splits into offset and size is not pinned yet: that needs one `.xsub` part to check against.

use crate::{Error, Result, u32le, u64le};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Header {
    pub version: u16,
    pub file_len: u64,
    pub parts: u64,
    pub rows: u64,
    pub table_at: u64,
}

pub fn header(b: &[u8]) -> Result<Header> {
    if b.get(..4) != Some(b"KAPI") {
        return Err(Error::Format("not KAPI".into()));
    }
    let version = (u32le(b, 4)? >> 16) as u16;
    let h = Header { version, file_len: u64le(b, 0x18)?, parts: u64le(b, 0x28)?, rows: u64le(b, 0x7a0)?, table_at: u64le(b, 0x7a8)? };
    if u64le(b, 0x7b0)? != h.rows * 16 {
        return Err(Error::Format("KAPI table size is not rows * 16".into()));
    }
    Ok(h)
}

/// `(key, packed)` rows, checked sorted.
pub fn rows(b: &[u8]) -> Result<Vec<(u64, u64)>> {
    let h = header(b)?;
    let mut v = Vec::with_capacity(h.rows as usize);
    for i in 0..h.rows as usize {
        let o = h.table_at as usize + i * 16;
        v.push((u64le(b, o)?, u64le(b, o + 8)?));
    }
    if v.windows(2).any(|w| w[0].0 > w[1].0) {
        return Err(Error::Format("KAPI keys not sorted".into()));
    }
    Ok(v)
}

/// Binary search for one key's packed location.
pub fn find(rows: &[(u64, u64)], key: u64) -> Option<u64> {
    rows.binary_search_by_key(&key, |r| r.0).ok().map(|i| rows[i].1)
}
