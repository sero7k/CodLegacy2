//! `TAFF` v9 fastfile envelope.
//!
//! ```text
//! 0x000 "TAFF", u32 9, u32 0x39cb44f1, u32 4, u32 0x64, u32 0x2c2381cf, u32 6, ...
//! 0x046 builder string, e.g. "TA-BM-SV-T9-12" (64 bytes)
//! 0x182 u64 decoded size of the whole zone (sum of the blocks)
//! 0x46a zone name (64 bytes)
//! 0x4aa 256-byte signature (not checked)
//! 0x5ba blocks: u32 compressed, u32 decoded, u32 aligned size, u32 this header's own offset,
//!       then the Oodle stream; the next header is at +16 + aligned; a zero header ends the chain
//! ```
//! Checked on `mp_sm_central.ff` (29 blocks, 30,178,741 bytes) and `en_mp_sm_central.ff`
//! (1 block): every block is Kraken (`8C 06`), and the readable literals show the data is not
//! encrypted.

use crate::{Error, Result, cstr, u32le, u64le};
use fastfile_jup::oodle::Decompressor;

pub const MAGIC: &[u8; 4] = b"TAFF";
pub const VERSION: u32 = 9;
pub const BLOCKS_AT: usize = 0x5ba;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Header {
    pub builder: String,
    pub zone: String,
    pub decoded_size: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Block {
    /// File offset of the Oodle stream (after the 16-byte block header).
    pub data: usize,
    pub compressed: u32,
    pub decoded: u32,
}

pub fn header(b: &[u8]) -> Result<Header> {
    if b.get(..4) != Some(MAGIC) {
        return Err(Error::Format("not a TAFF fastfile".into()));
    }
    let v = u32le(b, 4)?;
    if v != VERSION {
        return Err(Error::Format(format!("TAFF version {v}, expected {VERSION}")));
    }
    let s = |o: usize| b.get(o..o + 64).map(cstr).ok_or_else(|| Error::Format("TAFF header short".into()));
    Ok(Header { builder: s(0x46)?, zone: s(0x46a)?, decoded_size: u64le(b, 0x182)? })
}

/// Where the first block header is: `BLOCKS_AT` on the zones seen so far; otherwise the first
/// offset past it whose header names its own offset (an unverified fallback for zones such as
/// `mp_common`, which fail at `BLOCKS_AT`).
pub fn first_block(b: &[u8]) -> Option<usize> {
    let ok = |p: usize| {
        let r = |o| u32le(b, p + o).ok();
        matches!((r(0), r(8), r(12)), (Some(cs), Some(al), Some(own))
            if cs > 0 && al >= cs && own as usize == p && p + 16 + cs as usize <= b.len())
    };
    (BLOCKS_AT..b.len().min(1 << 20)).find(|&p| ok(p))
}

/// The block chain, checked (self offsets, bounds, sizes adding up to the header's total).
pub fn blocks(b: &[u8]) -> Result<Vec<Block>> {
    let h = header(b)?;
    let mut p = first_block(b).ok_or_else(|| {
        Error::Format(format!("bad block header at {BLOCKS_AT:#x} and no self-addressed header after it"))
    })?;
    let mut v = Vec::new();
    let mut total = 0u64;
    loop {
        let cs = u32le(b, p)?;
        if cs == 0 {
            break;
        }
        let (ds, al, own) = (u32le(b, p + 4)?, u32le(b, p + 8)?, u32le(b, p + 12)?);
        if own as usize != p || al < cs || p + 16 + cs as usize > b.len() {
            return Err(Error::Format(format!("bad block header at {p:#x}")));
        }
        v.push(Block { data: p + 16, compressed: cs, decoded: ds });
        total += ds as u64;
        p += 16 + al as usize;
    }
    if total != h.decoded_size {
        return Err(Error::Format(format!("blocks decode to {total}, header says {}", h.decoded_size)));
    }
    Ok(v)
}

/// The zone image: every block decoded in order.
pub fn decode(b: &[u8], oodle: &dyn Decompressor) -> Result<Vec<u8>> {
    let bl = blocks(b)?;
    let mut out = vec![0u8; bl.iter().map(|x| x.decoded as usize).sum()];
    let mut o = 0;
    for x in bl {
        let src = &b[x.data..x.data + x.compressed as usize];
        oodle.decompress(src, &mut out[o..o + x.decoded as usize]).map_err(Error::Oodle)?;
        o += x.decoded as usize;
    }
    Ok(out)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use fastfile_jup::oodle::StoredOnly;

    /// A TAFF with stored Oodle chunks holding `parts`.
    pub(crate) fn synth(zone: &str, parts: &[&[u8]]) -> Vec<u8> {
        let mut b = vec![0u8; BLOCKS_AT];
        b[..4].copy_from_slice(MAGIC);
        b[4..8].copy_from_slice(&VERSION.to_le_bytes());
        b[0x46..0x46 + 14].copy_from_slice(b"TA-BM-SV-T9-12");
        b[0x46a..0x46a + zone.len()].copy_from_slice(zone.as_bytes());
        let total: usize = parts.iter().map(|p| p.len()).sum();
        b[0x182..0x18a].copy_from_slice(&(total as u64).to_le_bytes());
        for p in parts {
            let mut s = vec![0xCC, 0x06];
            s.extend_from_slice(p);
            let al = (s.len() + 3) & !3;
            let own = b.len() as u32;
            for v in [s.len() as u32, p.len() as u32, al as u32, own] {
                b.extend_from_slice(&v.to_le_bytes());
            }
            s.resize(al, 0);
            b.extend(s);
        }
        b.extend_from_slice(&[0; 16]);
        b
    }

    #[test]
    fn walks_and_decodes_the_chain() {
        let b = synth("mp_sm_central", &[b"zone ", b"image"]);
        let h = header(&b).unwrap();
        assert_eq!((h.zone.as_str(), h.builder.as_str(), h.decoded_size), ("mp_sm_central", "TA-BM-SV-T9-12", 10));
        assert_eq!(blocks(&b).unwrap().len(), 2);
        assert_eq!(decode(&b, &StoredOnly).unwrap(), b"zone image");
    }

    #[test]
    fn rejects_a_wrong_self_offset() {
        let mut b = synth("x", &[b"abc", b"def"]);
        let second = BLOCKS_AT + 16 + 8;
        b[second + 12] ^= 1;
        assert!(blocks(&b).is_err());
    }

    #[test]
    fn finds_a_moved_first_block() {
        let mut b = synth("x", &[b"abc"]);
        // Shift the chain 0x40 bytes later and fix its self offset.
        b.splice(BLOCKS_AT..BLOCKS_AT, [0u8; 0x40]);
        let p = BLOCKS_AT + 0x40;
        b[p + 12..p + 16].copy_from_slice(&(p as u32).to_le_bytes());
        assert_eq!(first_block(&b), Some(p));
        assert_eq!(decode(&b, &StoredOnly).unwrap(), b"abc");
    }
}
