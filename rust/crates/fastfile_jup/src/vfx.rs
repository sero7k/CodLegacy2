//! Modern Warfare III's effects ("vfx" assets) as a zone holds them: where
//! they are, their header, and the module lists of their emitters. The
//! modules' numbers (curves, spawn, size, colour) are NOT read: see
//! `docs/JUP.md`, "Effects (vfx)".
//!
//! ```text
//! 0x00 u64 name hash   FNV of the path without ".vfx" (`vfx/iw8/veh/.../x`)
//! 0x08 ptr -2          the effect follows
//! 0x10 u32 flags       0x0f 0x17 0x1f 0x20 0x24 ...
//! 0x14 u32 emitters    1 ..= 64
//! 0x18 u16, u16        0, then 4 / 0x0c / 0x10 ... (kinds)
//! 0x1c i32 -1
//! 0x20 f32 x3          0, then LOD distances (-1 -1, or 350 500), 0
//! 0x30 f32             100000
//! 0x80 ptr -2          the emitters follow (0x88)
//! 0x88 emitter[n]      0xC0 bytes each, a pointer at +0xB8 to its detail
//! then per emitter, depth first:
//!   detail (0x48): +0x18 ptr module list, +0x20 u32 count,
//!                  +0x28 ptr parameter list, +0x30 u32 count
//!   entries (0x20): ptr, u64, u16, u16 id (+0x12), ...
//!     a module's id is 0..=37, a parameter group's 42..=62
//!     ptr -2 / -1: the entry's data follows (after the list)
//!     ptr 0x0f<<32 | offset+1: the same data stored earlier in block 0x0f
//! ```
//!
//! An effect whose one emitter has only back-references is exactly
//! `0x10 + 0x78 + 0xC0 + 0x48 + 0x20 * (modules + parameters)` bytes
//! ([`Vfx::extent`]), which is how the layout was checked: 2,0xx effects of
//! `ingame_shared_mp` end exactly where the next one begins.
#![forbid(unsafe_code)]

use crate::ptr::{FOLLOW, INSERT};

pub const ROOT_END: usize = 0x88;
pub const EMITTER_LEN: usize = 0xC0;
pub const DETAIL_LEN: usize = 0x48;
pub const ENTRY_LEN: usize = 0x20;

/// Where an entry's data is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Storage {
    /// Follows in the stream (`-2`).
    Inline,
    /// Follows in the stream and may be back-referenced (`-1`).
    InlineShared,
    /// Stored earlier in block `0x0f` at this offset.
    Back(u32),
    /// Some other pointer value.
    Other(u64),
}

/// One entry of a module list or parameter list.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Entry {
    pub id: u16,
    pub storage: Storage,
}

/// An effect found in a zone image.
#[derive(Clone, Debug)]
pub struct Vfx {
    /// Offset of the name hash.
    pub at: usize,
    pub hash: u64,
    pub flags: u32,
    pub emitters: u32,
    /// The two distances at +0x24 / +0x28 (-1, -1 when unset).
    pub lod: [f32; 2],
    /// The first emitter's modules.
    pub modules: Vec<Entry>,
    /// The first emitter's parameter groups.
    pub params: Vec<Entry>,
}

impl Vfx {
    /// The effect's size in the stream, known when it has one emitter
    /// and all its entries are back-references.
    #[must_use]
    pub fn extent(&self) -> Option<usize> {
        let shared = |e: &Entry| matches!(e.storage, Storage::Back(_));
        (self.emitters == 1 && self.modules.iter().chain(&self.params).all(shared))
            .then(|| 0x10 + 0x78 + EMITTER_LEN + DETAIL_LEN + ENTRY_LEN * (self.modules.len() + self.params.len()))
    }
}

fn u16_at(b: &[u8], o: usize) -> Option<u16> {
    Some(u16::from_le_bytes(b.get(o..o + 2)?.try_into().ok()?))
}
fn u32_at(b: &[u8], o: usize) -> Option<u32> {
    Some(u32::from_le_bytes(b.get(o..o + 4)?.try_into().ok()?))
}
fn u64_at(b: &[u8], o: usize) -> Option<u64> {
    Some(u64::from_le_bytes(b.get(o..o + 8)?.try_into().ok()?))
}
fn f32_at(b: &[u8], o: usize) -> Option<f32> {
    Some(f32::from_bits(u32_at(b, o)?))
}

fn entry(b: &[u8], o: usize) -> Option<Entry> {
    let p = u64_at(b, o)?;
    let storage = match p {
        FOLLOW => Storage::Inline,
        INSERT => Storage::InlineShared,
        p if p >> 32 == 0x0f && p & 0xffff_ffff != 0 => Storage::Back((p & 0xffff_ffff) as u32 - 1),
        p => Storage::Other(p),
    };
    Some(Entry { id: u16_at(b, o + 0x12)?, storage })
}

/// Read the effect whose name hash is at `at`, if the bytes there are one.
#[must_use]
pub fn read(z: &[u8], at: usize) -> Option<Vfx> {
    let hash = u64_at(z, at)?;
    if hash >> 63 != 0 || hash.count_ones() < 16 || u64_at(z, at + 8)? != FOLLOW {
        return None;
    }
    let flags = u32_at(z, at + 0x10)?;
    let emitters = u32_at(z, at + 0x14)?;
    if flags == 0 || flags > 0x40 || !(1..=64).contains(&emitters) {
        return None;
    }
    if u32_at(z, at + 0x1c)? != u32::MAX || u32_at(z, at + 0x20)? != 0 || u64_at(z, at + 0x80)? != FOLLOW {
        return None;
    }
    let (a, b) = (f32_at(z, at + 0x24)?, f32_at(z, at + 0x28)?);
    if !((a == -1.0 && b == -1.0) || (a == 0.0 && b == 0.0) || (a == 350.0 && b == 500.0)) {
        // (other pairs exist; not read until one is seen)
        return None;
    }
    let detail = at + 0x10 + 0x78 + EMITTER_LEN * emitters as usize;
    if u64_at(z, detail + 0x18)? != FOLLOW {
        return None;
    }
    let (c1, c2) = (u32_at(z, detail + 0x20)? as usize, u32_at(z, detail + 0x30)? as usize);
    if c1 > 64 || c2 > 64 {
        return None;
    }
    let list1 = detail + DETAIL_LEN;
    let modules = (0..c1).map(|k| entry(z, list1 + ENTRY_LEN * k)).collect::<Option<Vec<_>>>()?;
    // The parameter list follows the module list only when no module's
    // data does; otherwise its place is not known without the module sizes.
    let params = if modules.iter().all(|m| matches!(m.storage, Storage::Back(_))) && (c2 == 0 || u64_at(z, detail + 0x28)? == FOLLOW) {
        let list2 = list1 + ENTRY_LEN * c1;
        (0..c2).map(|k| entry(z, list2 + ENTRY_LEN * k)).collect::<Option<Vec<_>>>()?
    } else {
        Vec::new()
    };
    Some(Vfx { at, hash, flags, emitters, lod: [a, b], modules, params })
}

/// Every effect in `z[from..to]`, in stream order.
#[must_use]
pub fn scan(z: &[u8], from: usize, to: usize) -> Vec<Vfx> {
    let to = to.min(z.len());
    let mut out = Vec::new();
    let mut i = from;
    while i + 0x90 <= to {
        // The pointer at +8 is eight 0xff-ish bytes starting 0xfe.
        if z[i + 8] == 0xfe && z[i + 15] == 0xff && z[i + 7] < 0x80 {
            if let Some(v) = read(z, i) {
                out.push(v);
            }
        }
        i += 1;
    }
    out
}

/// The name hash of an effect path (`vfx\iw8\veh\scriptables\x.vfx`).
#[must_use]
pub fn path_hash(path: &str) -> u64 {
    crate::hash::name(path.strip_suffix(".vfx").unwrap_or(path))
}

/// Where an effect's name hash occurs in a zone outside the effect's own
/// header: a map's placement of the effect, a scriptable's reference, a
/// precache list. The stream is searched at every byte (the structs are
/// not aligned to the hash).
#[derive(Clone, Debug, PartialEq)]
pub struct Ref {
    pub at: usize,
    pub hash: u64,
    /// Three plausible coordinates just before the hash (an origin placed
    /// ahead of it), if there are.
    pub before: Option<[f32; 3]>,
    /// Three plausible coordinates just after the hash and its 8 bytes of
    /// pointer, if there are (`hash, ptr, x, y, z`).
    pub after: Option<[f32; 3]>,
}

fn coordinate_triple(z: &[u8], o: usize) -> Option<[f32; 3]> {
    let v = [f32_at(z, o)?, f32_at(z, o + 4)?, f32_at(z, o + 8)?];
    let ok = v.iter().all(|c| c.is_finite() && c.abs() < 200_000.0) && v.iter().any(|c| c.abs() >= 1.0);
    ok.then_some(v)
}

/// Every occurrence of one of `hashes` in `z`, except at the effects' own
/// headers (`own`: offsets from [`scan`]).
#[must_use]
pub fn references(z: &[u8], hashes: &std::collections::HashSet<u64>, own: &std::collections::HashSet<usize>) -> Vec<Ref> {
    let mut out = Vec::new();
    for at in 0..z.len().saturating_sub(7) {
        let Some(h) = u64_at(z, at) else { break };
        if h >> 63 != 0 || !hashes.contains(&h) || own.contains(&at) {
            continue;
        }
        out.push(Ref {
            at,
            hash: h,
            before: at.checked_sub(12).and_then(|o| coordinate_triple(z, o)),
            after: coordinate_triple(z, at + 16),
        });
    }
    out
}

#[cfg(test)]
mod ref_tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn finds_a_hash_outside_the_effects_and_its_neighbouring_origin() {
        let h = path_hash("vfx/iw8/env/fire/fire_small");
        let mut z = vec![0u8; 16];
        // (an origin, then the hash, a pointer, nothing plausible after)
        for c in [120.5f32, -33.0, 8.25] {
            z.extend_from_slice(&c.to_le_bytes());
        }
        let at = z.len();
        z.extend_from_slice(&h.to_le_bytes());
        z.extend_from_slice(&u64::MAX.to_le_bytes());
        z.extend_from_slice(&[0u8; 12]);
        // the effect's own header elsewhere (excluded)
        let own_at = z.len();
        z.extend_from_slice(&h.to_le_bytes());
        z.extend_from_slice(&[0u8; 8]);
        let hashes: HashSet<u64> = HashSet::from([h]);
        let own: HashSet<usize> = HashSet::from([own_at]);
        let refs = references(&z, &hashes, &own);
        assert_eq!(refs.len(), 1);
        assert_eq!(refs[0].at, at);
        assert_eq!(refs[0].before, Some([120.5, -33.0, 8.25]));
        assert_eq!(refs[0].after, None);
    }

    #[test]
    fn nothing_when_the_hash_is_absent() {
        let z = vec![0xabu8; 64];
        let hashes: HashSet<u64> = HashSet::from([path_hash("vfx/x")]);
        assert!(references(&z, &hashes, &HashSet::new()).is_empty());
    }
}
