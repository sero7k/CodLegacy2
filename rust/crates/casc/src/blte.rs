//! BLTE: `"BLTE"`, u32 BE header size (0 = one chunk), then (if not 0) u8 flags, u24 BE chunk
//! count, per chunk {u32 BE compressed, u32 BE decoded, 16-byte MD5}; chunks start at the header
//! size. Each chunk starts with a mode byte: `N` raw, `Z` zlib, `E` encrypted (u8 key name length,
//! key name, ...).
//!
//! Every file in the Cold War install is `N` or `Z` (the encoding manifest's specs have no `e:`).
//! An `E` chunk is reported as [`Error::Encrypted`](crate::Error::Encrypted) and never decoded.

use crate::{Error, Result, fmt_err};
use std::io::Read;

/// Chunk modes seen while decoding, for surveys.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Modes {
    pub raw: u32,
    pub zlib: u32,
    pub encrypted: u32,
}

/// `(compressed, decoded)` per chunk.
pub type ChunkTable = Vec<(usize, Option<usize>)>;

/// The `(compressed, decoded)` chunk sizes of a BLTE stream (`decoded` is `None` for a
/// single-chunk stream without a table).
pub fn chunks(b: &[u8]) -> Result<(usize, ChunkTable)> {
    if b.get(..4) != Some(b"BLTE") {
        return fmt_err("not BLTE");
    }
    let hs = u32::from_be_bytes(b[4..8].try_into().unwrap()) as usize;
    if hs == 0 {
        return Ok((8, vec![(b.len() - 8, None)]));
    }
    let n = b.get(9..12).map(|x| u32::from_be_bytes([0, x[0], x[1], x[2]]) as usize);
    let n = n.ok_or_else(|| Error::Format("BLTE table".into()))?;
    let mut v = Vec::with_capacity(n);
    for i in 0..n {
        let e = b.get(12 + i * 24..20 + i * 24).ok_or_else(|| Error::Format("BLTE table past end".into()))?;
        v.push((
            u32::from_be_bytes(e[0..4].try_into().unwrap()) as usize,
            Some(u32::from_be_bytes(e[4..8].try_into().unwrap()) as usize),
        ));
    }
    Ok((hs, v))
}

/// Decode a whole BLTE stream.
pub fn decode(b: &[u8], modes: &mut Modes) -> Result<Vec<u8>> {
    let (mut q, table) = chunks(b)?;
    let mut out = Vec::with_capacity(table.iter().map(|c| c.1.unwrap_or(c.0)).sum());
    for (cs, ds) in table {
        let c = b.get(q..q + cs).ok_or_else(|| Error::Format("BLTE chunk past end".into()))?;
        q += cs;
        let start = out.len();
        match c.first() {
            Some(b'N') => {
                modes.raw += 1;
                out.extend_from_slice(&c[1..]);
            }
            Some(b'Z') => {
                modes.zlib += 1;
                flate2::read::ZlibDecoder::new(&c[1..]).read_to_end(&mut out)?;
            }
            Some(b'E') => {
                modes.encrypted += 1;
                let kl = *c.get(1).unwrap_or(&0) as usize;
                let mut key = c.get(2..2 + kl).unwrap_or(&[]).to_vec();
                key.reverse();
                return Err(Error::Encrypted(crate::hex(&key)));
            }
            m => return fmt_err(format!("BLTE chunk mode {m:?}")),
        }
        if let Some(ds) = ds
            && out.len() - start != ds
        {
            return fmt_err(format!("BLTE chunk decoded {} bytes, table says {ds}", out.len() - start));
        }
    }
    Ok(out)
}

/// Decode a `data.NNN` entry (30-byte header + BLTE).
pub fn decode_entry(entry: &[u8], modes: &mut Modes) -> Result<Vec<u8>> {
    decode(entry.get(crate::idx::ENTRY_HEADER as usize..).unwrap_or(&[]), modes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn blte(chunks: &[Vec<u8>], decoded: &[usize]) -> Vec<u8> {
        let hs = 12 + 24 * chunks.len();
        let mut b = b"BLTE".to_vec();
        b.extend_from_slice(&(hs as u32).to_be_bytes());
        b.push(0x0f);
        b.extend_from_slice(&(chunks.len() as u32).to_be_bytes()[1..]);
        for (c, d) in chunks.iter().zip(decoded) {
            b.extend_from_slice(&(c.len() as u32).to_be_bytes());
            b.extend_from_slice(&(*d as u32).to_be_bytes());
            b.extend_from_slice(&[0; 16]);
        }
        for c in chunks {
            b.extend_from_slice(c);
        }
        b
    }

    #[test]
    fn raw_and_zlib_chunks() {
        let mut z = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
        z.write_all(b"world").unwrap();
        let mut zc = vec![b'Z'];
        zc.extend(z.finish().unwrap());
        let b = blte(&[b"Nhello ".to_vec(), zc], &[6, 5]);
        let mut m = Modes::default();
        assert_eq!(decode(&b, &mut m).unwrap(), b"hello world");
        assert_eq!((m.raw, m.zlib, m.encrypted), (1, 1, 0));
    }

    #[test]
    fn encrypted_chunk_is_reported_not_decoded() {
        let b = blte(&[vec![b'E', 2, 0xaa, 0xbb, 1, 2, 3]], &[3]);
        let mut m = Modes::default();
        assert!(matches!(decode(&b, &mut m), Err(Error::Encrypted(k)) if k == "bbaa"));
        assert_eq!(m.encrypted, 1);
    }
}
