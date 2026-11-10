//! TVFS root (`vfs-root` in the build config).
//!
//! ```text
//! "TVFS", u8 version 1, u8 header size, u8 EKey size, u8 patch key size, u32 BE flags,
//! u32 BE path table offset, size, VFS table offset, size, CFT table offset, size,
//! u16 BE max depth, [flags & 2: u32 BE ESpec table offset, size]
//! ```
//! Path table: a prefix tree of name fragments. A node is: optional `0x00` (separator before),
//! `len, bytes` (a fragment, absent when the next byte is `0xFF`), optional `0x00` (separator
//! after), then either `0xFF` + u32 BE value or another fragment continuing the same name.
//! A value with the top bit set is a folder whose children take `(value & 0x7fffffff) - 4`
//! bytes. Otherwise it is an offset into the VFS table: u8 span count, then per span u32 BE
//! offset in the file, u32 BE span size, and a big-endian CFT offset (1..4 bytes, by CFT table
//! size). The CFT entry starts with the EKey and a u32 BE encoded size.
//!
//! Cold War: version 1, flags 0x0f, EKey 9 bytes, one span per file, 6,415 paths.

use crate::{Error, Result, fmt_err};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Span {
    /// The first `ekey_size` bytes of the encoded key (9 in Cold War; an `.idx` key).
    pub ekey: Vec<u8>,
    /// Decoded size of this span.
    pub size: u32,
    /// Encoded (BLTE) size.
    pub encoded_size: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct File {
    pub path: String,
    pub spans: Vec<Span>,
}

impl File {
    pub fn size(&self) -> u64 {
        self.spans.iter().map(|s| s.size as u64).sum()
    }
}

struct Hdr {
    ekey: usize,
    path: (usize, usize),
    vfs: usize,
    cft: (usize, usize),
}

fn be32(b: &[u8], o: usize) -> Result<u32> {
    b.get(o..o + 4)
        .map(|x| u32::from_be_bytes(x.try_into().unwrap()))
        .ok_or_else(|| Error::Format(format!("TVFS read past end at {o}")))
}

fn header(b: &[u8]) -> Result<Hdr> {
    if b.get(..4) != Some(b"TVFS") {
        return fmt_err("not TVFS");
    }
    if b[4] != 1 {
        return fmt_err(format!("TVFS version {}", b[4]));
    }
    Ok(Hdr {
        ekey: b[6] as usize,
        path: (be32(b, 12)? as usize, be32(b, 16)? as usize),
        vfs: be32(b, 20)? as usize,
        cft: (be32(b, 28)? as usize, be32(b, 32)? as usize),
    })
}

/// Every file of a decoded TVFS root, in path-table order (sorted).
pub fn parse(b: &[u8]) -> Result<Vec<File>> {
    let h = header(b)?;
    let mut leaves = Vec::new();
    walk(b, h.path.0, h.path.0 + h.path.1, &mut String::new(), &mut leaves, 0)?;
    let osz = match h.cft.1 {
        n if n > 0xff_ffff => 4,
        n if n > 0xffff => 3,
        n if n > 0xff => 2,
        _ => 1,
    };
    leaves
        .into_iter()
        .map(|(path, v)| {
            let mut q = h.vfs + v as usize;
            let n = *b.get(q).ok_or_else(|| Error::Format("VFS entry past end".into()))? as usize;
            q += 1;
            let mut spans = Vec::with_capacity(n);
            for _ in 0..n {
                let size = be32(b, q + 4)?;
                let c = b.get(q + 8..q + 8 + osz).ok_or_else(|| Error::Format("CFT offset".into()))?;
                let c = c.iter().fold(0usize, |a, &x| (a << 8) | x as usize);
                q += 8 + osz;
                let e = h.cft.0 + c;
                let ekey = b.get(e..e + h.ekey).ok_or_else(|| Error::Format("CFT entry".into()))?.to_vec();
                spans.push(Span { ekey, size, encoded_size: be32(b, e + h.ekey)? });
            }
            Ok(File { path, spans })
        })
        .collect()
}

fn walk(b: &[u8], mut p: usize, end: usize, base: &mut String, out: &mut Vec<(String, u32)>, depth: u32) -> Result<()> {
    if depth > 64 || end > b.len() {
        return fmt_err("TVFS path table malformed");
    }
    let keep = base.len();
    while p < end {
        if b[p] == 0 {
            base.push('/');
            p += 1;
        }
        if p < end && b[p] != 0xff {
            let l = b[p] as usize;
            let frag = b.get(p + 1..p + 1 + l).ok_or_else(|| Error::Format("TVFS fragment".into()))?;
            base.push_str(&String::from_utf8_lossy(frag));
            p += 1 + l;
        }
        if p < end && b[p] == 0 {
            base.push('/');
            p += 1;
        }
        if p < end && b[p] == 0xff {
            let v = be32(b, p + 1)?;
            p += 5;
            if v & 0x8000_0000 != 0 {
                let len = ((v & 0x7fff_ffff) as usize).checked_sub(4).ok_or_else(|| Error::Format("folder size".into()))?;
                walk(b, p, p + len, base, out, depth + 1)?;
                p += len;
            } else {
                out.push((base.clone(), v));
            }
            base.truncate(keep);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A two-file tree: `a.dll` and `zone/b.ff`, hand-assembled.
    #[test]
    fn walks_fragments_and_folders() {
        let mut path = vec![0xff];
        let mut root_children = Vec::new();
        root_children.extend_from_slice(&[5, b'a', b'.', b'd', b'l', b'l', 0xff, 0, 0, 0, 0]);
        let mut zone = vec![4, b'z', b'o', b'n', b'e', 0, 0xff];
        let sub = [4u8, b'b', b'.', b'f', b'f', 0xff, 0, 0, 0, 10];
        zone.extend_from_slice(&(0x8000_0000u32 | (sub.len() as u32 + 4)).to_be_bytes());
        zone.extend_from_slice(&sub);
        root_children.extend_from_slice(&zone);
        path.extend_from_slice(&(0x8000_0000u32 | (root_children.len() as u32 + 4)).to_be_bytes());
        path.extend_from_slice(&root_children);
        let hs = 38usize;
        let vfs: Vec<u8> = [
            vec![1, 0, 0, 0, 0, 0, 0, 0, 7, 0],
            vec![1, 0, 0, 0, 0, 0, 0, 0, 9, 6],
        ]
        .concat();
        let cft: Vec<u8> = [vec![0xaa; 2], 7u32.to_be_bytes().to_vec(), vec![0xbb; 2], 9u32.to_be_bytes().to_vec()].concat();
        let mut b = b"TVFS".to_vec();
        b.extend_from_slice(&[1, hs as u8, 2, 0, 0, 0, 0, 0]);
        let po = hs;
        let vo = po + path.len();
        let co = vo + vfs.len();
        for v in [po, path.len(), vo, vfs.len(), co, cft.len()] {
            b.extend_from_slice(&(v as u32).to_be_bytes());
        }
        b.extend_from_slice(&[0, 4]);
        assert_eq!(b.len(), hs);
        b.extend(path);
        b.extend(vfs);
        b.extend(cft);
        let f = parse(&b).unwrap();
        assert_eq!(f.len(), 2);
        assert_eq!((f[0].path.as_str(), f[0].spans[0].ekey.as_slice(), f[0].size()), ("a.dll", &[0xaa, 0xaa][..], 7));
        assert_eq!((f[1].path.as_str(), f[1].spans[0].ekey.as_slice(), f[1].size()), ("zone/b.ff", &[0xbb, 0xbb][..], 9));
    }
}
