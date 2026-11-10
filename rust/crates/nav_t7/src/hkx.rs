//! Havok packfiles (binary `.hkx`, file version 11, `hk_2014.2.0-r1`, the
//! 64-bit little-endian layout) as Black Ops III's zones carry them: the
//! nav mesh and nav volume assets each hold a few, unpatched.
//!
//! Layout (read on `zm_zod.ff`, 2026-10-27): a 0x40-byte header (magic
//! `57 E0 E0 57 10 C0 C0 10`, user tag, file version, layout rules, section
//! count, contents section/offset, class name section/offset, the version
//! string at 0x28, flags, then the predicate size at 0x3e and that many
//! bytes), then per section 0x40 bytes: its tag (`__classnames__`,
//! `__types__`, `__data__`), the absolute data start and, relative to it,
//! the local, global and virtual fixup tables, exports, imports, end.
//! Local fixups are (source, target) offsets in the section: the pointer
//! at the source points at the target. Virtual fixups are (offset, class
//! name section, class name offset): an object of that class starts there.

use std::collections::HashMap;

pub const MAGIC: [u8; 8] = [0x57, 0xe0, 0xe0, 0x57, 0x10, 0xc0, 0xc0, 0x10];

#[derive(Clone, Debug)]
pub struct Section {
    pub tag: String,
    /// The section's data, from the packfile's start.
    pub start: usize,
    pub end: usize,
    /// Pointer source -> target, offsets in the section.
    pub local: HashMap<usize, usize>,
    /// (offset, class name) of each object.
    pub objects: Vec<(usize, String)>,
}

#[derive(Clone, Debug)]
pub struct Packfile<'a> {
    pub bytes: &'a [u8],
    pub version: String,
    pub sections: Vec<Section>,
    /// Total size (the last section's end).
    pub size: usize,
}

fn i32_at(b: &[u8], o: usize) -> Result<i32, String> {
    b.get(o..o + 4).map(|s| i32::from_le_bytes([s[0], s[1], s[2], s[3]])).ok_or_else(|| format!("i32 at {o:#x} past the end"))
}

fn cstr(b: &[u8], o: usize) -> String {
    b.get(o..).unwrap_or(&[]).iter().take_while(|&&c| c != 0).map(|&c| char::from(c)).collect()
}

impl<'a> Packfile<'a> {
    pub fn parse(b: &'a [u8]) -> Result<Self, String> {
        if b.get(..8) != Some(MAGIC.as_slice()) {
            return Err("not a Havok packfile".into());
        }
        let file_version = i32_at(b, 0x0c)?;
        if file_version != 11 || b.get(0x10..0x14) != Some([8, 1, 0, 1].as_slice()) {
            return Err(format!("packfile version {file_version}, layout {:?}: not the 64-bit 2014 layout", b.get(0x10..0x14)));
        }
        let n = i32_at(b, 0x14)?;
        let version = cstr(b, 0x28);
        let pred = u16::from_le_bytes([b[0x3e], b[0x3f]]) as usize;
        let mut off = 0x40 + pred;
        let mut sections = Vec::new();
        for _ in 0..n {
            let tag = cstr(b, off);
            let start = i32_at(b, off + 20)? as usize;
            let rel = |k: usize| -> Result<usize, String> { Ok(start + i32_at(b, off + 24 + 4 * k)? as usize) };
            let (lf, gf, vf, ex, end) = (rel(0)?, rel(1)?, rel(2)?, rel(3)?, rel(5)?);
            off += 0x40;
            let mut local = HashMap::new();
            let mut o = lf;
            while o + 8 <= gf {
                let src = i32_at(b, o)?;
                if src >= 0 {
                    local.insert(src as usize, i32_at(b, o + 4)? as usize);
                }
                o += 8;
            }
            sections.push(Section { tag, start, end, local, objects: Vec::new() });
            // (Virtual fixups name classes in another section: filled below.)
            sections.last_mut().unwrap().objects = {
                let mut v = Vec::new();
                let mut o = vf;
                while o + 12 <= ex {
                    let at = i32_at(b, o)?;
                    if at >= 0 {
                        v.push((at as usize, format!("{}:{}", i32_at(b, o + 4)?, i32_at(b, o + 8)?)));
                    }
                    o += 12;
                }
                v
            };
        }
        // Resolve class names.
        let starts: Vec<usize> = sections.iter().map(|s| s.start).collect();
        for s in &mut sections {
            for (_, name) in &mut s.objects {
                let (sec, at) = name.split_once(':').unwrap_or(("0", "0"));
                let (sec, at): (usize, usize) = (sec.parse().unwrap_or(0), at.parse().unwrap_or(0));
                *name = cstr(b, starts.get(sec).copied().unwrap_or(0) + at);
            }
        }
        let size = sections.iter().map(|s| s.end).max().unwrap_or(0);
        if size > b.len() {
            return Err(format!("packfile of {size} bytes, {} there", b.len()));
        }
        Ok(Self { bytes: &b[..size], version, sections, size })
    }

    /// The data section.
    pub fn data(&self) -> Option<&Section> {
        self.sections.iter().find(|s| s.tag == "__data__")
    }

    /// The offsets (in the data section) of the objects of a class.
    pub fn objects_of(&self, class: &str) -> Vec<usize> {
        let Some(d) = self.data() else { return Vec::new() };
        d.objects.iter().filter(|o| o.1 == class).map(|o| o.0).collect()
    }

    /// An `hkArray` at `at` in the data section: (its data's offset in the
    /// section, its size), the data found through the local fixups.
    pub fn array(&self, at: usize) -> Option<(usize, usize)> {
        let d = self.data()?;
        let size = i32_at(self.bytes, d.start + at + 8).ok()?;
        if size == 0 {
            return Some((0, 0));
        }
        Some((*d.local.get(&at)?, size as usize))
    }

    /// Bytes of the data section from `at`.
    pub fn data_bytes(&self, at: usize, len: usize) -> Option<&'a [u8]> {
        let d = self.data()?;
        self.bytes.get(d.start + at..d.start + at + len)
    }
}

/// Each packfile in a buffer (a zone image), by its magic.
pub fn find_all(image: &[u8]) -> Vec<Packfile<'_>> {
    let mut out = Vec::new();
    let mut i = 0;
    while let Some(p) = image[i..].windows(8).position(|w| w == MAGIC) {
        let at = i + p;
        if let Ok(pf) = Packfile::parse(&image[at..]) {
            i = at + pf.size.max(8);
            out.push(pf);
        } else {
            i = at + 8;
        }
    }
    out
}
