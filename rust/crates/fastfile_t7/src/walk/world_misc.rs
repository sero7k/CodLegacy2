//! Asset types map zones load besides the world assets themselves (read
//! on retail map zones, `mp_nuketown_x` first; x64-zt has no T7 layout for
//! these): camera paths (`XCam`), ...
//!
//! Each walker reads its pointer fields in offset order, an array then
//! what its records point to, the way the other walkers do; layouts are
//! measured on the zones (the comments say where).

use crate::cursor::Result;
use crate::stream::Stream;

fn count(s: &Stream, at: usize) -> Result<usize> {
    Ok(s.u32_at(at)? as usize)
}

/// `XCam` (0xa8; `mp_nuketown_x`'s `ui_cam_endgame_mp_nuketown`):
///
/// ```text
/// 0x00 char* name; 0x08 char* (empty on the shipped cameras)
/// 0x18 u32 count A; 0x1c u32 frames; 0x20 u32 rate; 0x24 u32 count C
/// 0x48 A* (16: { u32; u32 n; frame28* })  frames: origin + quaternion
/// 0x50 B* (32: { u32; u32; u32 n; u32; frame48*; f32; u32 })
/// 0x60 C* (8 each)
/// 0x68 F* (16 each, count at 0x70: `zm_castle`'s and `zm_island`'s outro
///      cameras, two each; null on the MP cameras)
/// 0x78 D* (16 each, count at 0x80), 0x88 E* (16 each, count at 0x90)
/// ```
pub fn xcam(s: &mut Stream, h: usize) -> Result<Option<String>> {
    s.no_loads(h, crate::AssetType::XCam.header_size(), &[0, 8, 0x48, 0x50, 0x60, 0x68, 0x78, 0x88])?;
    let name = s.string(h)?;
    s.string(h + 8)?;
    let n = count(s, h + 0x18)?;
    if let Some(t) = s.records(h + 0x48, n, 16, 7)? {
        for i in 0..n {
            let a = t + 16 * i;
            let frames = count(s, a + 4)?;
            s.records(a + 8, frames, 28, 3)?;
        }
    }
    // (The count is the byte at 0x14 on `mp_nuketown_x`'s cameras, at 0x15
    // on `zm_factory`'s intro camera.)
    // (`zm_stalingrad`'s `c_sta_outro_3rd_sh200_camera` has both bytes 0
    // and its count, one, in the u32 at 0x18.)
    let n = match (s.u8_at(h + 0x14)?, s.u8_at(h + 0x15)?) {
        (0, 0) => count(s, h + 0x18)?,
        (0, n) | (n, _) => n as usize,
    };
    if let Some(t) = s.records(h + 0x50, n, 32, 7)? {
        for i in 0..n {
            let b = t + 32 * i;
            let frames = count(s, b + 8)?;
            s.records(b + 0x10, frames, 48, 3)?;
        }
    }
    let n = count(s, h + 0x24)?;
    s.records(h + 0x60, n, 8, 3)?;
    // (`zm_castle`'s `c_cas_01_outro_3rd_sh010_cam`: two records of u32 ids
    // and counts between its frames and the list at 0x78.)
    let n = count(s, h + 0x70)?;
    s.records(h + 0x68, n, 16, 3)?;
    // Two lists of 16-byte records (u32 ids and counts; `zm_factory`'s
    // intro camera: five at 0x78, count at 0x80, three at 0x88, count at
    // 0x90).
    let n = count(s, h + 0x80)?;
    s.records(h + 0x78, n, 16, 3)?;
    let n = count(s, h + 0x90)?;
    s.records(h + 0x88, n, 16, 3)?;
    Ok(name)
}

/// `Glass` (136, `mp_nuketown_x`): 0x00 `GlassDef*`, 0x08 index, 0x0c brush
/// model, origin, angles, bounds, 0x40 planar, 0x41 outline vertex count,
/// 0x48 outline (8 each), axis, ...
pub const GLASS: usize = 136;
/// `GlassDef` (0x68): name, sizes, three materials (0x28..0x38), two
/// effect names (0x40, 0x48: strings), and references at 0x50..0x60.
pub const GLASS_DEF: usize = 0x68;

/// `Glasses` (0x58): `{ name; u32 numGlasses; Glass* glasses; u8*
/// workMemory; u32 workMemorySize; u8* smallAllocatorBlocks; u32
/// smallAllocatorSize; limits (u32s) }`. Order: name, the glasses (each:
/// its def, then its outline), the work memory, the small allocator
/// blocks (both filled with a byte pattern in the zone).
pub fn glasses(s: &mut Stream, h: usize) -> Result<Option<String>> {
    s.no_loads(h, crate::AssetType::Glasses.header_size(), &[0, 0x10, 0x18, 0x28])?;
    let name = s.string(h)?;
    let n = count(s, h + 8)?;
    if let Some(t) = s.records(h + 0x10, n, GLASS, 7)? {
        for i in 0..n {
            let g = t + GLASS * i;
            s.no_loads(g, GLASS, &[0, 0x48])?;
            if let Some(d) = s.array(g, GLASS_DEF, 7)? {
                s.no_loads(d, GLASS_DEF, &[0, 0x28, 0x30, 0x38, 0x40, 0x48, 0x50, 0x58, 0x60])?;
                s.string(d)?;
                for f in [0x28, 0x30, 0x38] {
                    s.asset(crate::AssetType::Material, d + f)?;
                }
                s.string(d + 0x40)?;
                s.string(d + 0x48)?;
                for f in [0x50, 0x58, 0x60] {
                    if s.ptr_at(d + f)?.is_inline() {
                        return Err(crate::cursor::ReadError::Invalid("glass def field (not read yet)", d + f));
                    }
                }
            }
            let verts = s.u8_at(g + 0x41)? as usize;
            s.records(g + 0x48, verts, 8, 3)?;
        }
    }
    // (The work memory is not VIRTUAL: the clip map's references after it
    // land only without it.)
    let k = count(s, h + 0x20)?;
    s.push(crate::stream::PHYSICAL);
    let r = s.array(h + 0x18, k, 15);
    s.pop();
    r?;
    let k = count(s, h + 0x30)?;
    s.array(h + 0x28, k, 15)?;
    Ok(name)
}

/// `SAnim` (siege animation, 0x70; `mp_freerun_02`'s
/// `p7_fxanim_gp_flag_freerun_01_sanim`): 0x00 name, 0x08
/// `StreamWrappedBuffer*` (its name a reference, its data resident or in
/// the `.xpak`), 0x18 records (28 each, count u16 at 0x64), bounds and
/// counts in between, 0x20 a second list (28 each: 16 bytes and three
/// u32s; count u16 at 0x6c: `mp_banzai`'s chains have two). Loaded in
/// that order.
///
/// bo2mp: the mod tools' maps (Steam Workshop items: `mp_mountain`'s
/// wires and tarps) also keep siege animations resident: 0x08 null and
/// 0x10 an `SAnimData*` instead (`sanim_resident`).
pub fn sanim(s: &mut Stream, h: usize) -> Result<Option<String>> {
    if s.ptr_at(h + 0x10)?.is_inline() {
        return sanim_resident(s, h);
    }
    s.no_loads(h, crate::AssetType::SAnim.header_size(), &[0, 8, 0x18, 0x20])?;
    let name = s.string(h)?;
    let stream = super::gfxworld::stream_wrapped(s, h + 8)?;
    // (The first list's count: 3 on every MP siege animation read, where
    // the u16 at 0x66 is 1; `zm_zod`'s fungus pods carry 2 and 1, and their
    // list holds three. The list is the two counts' sum when the first is
    // under three; a 3 already counts the 1.)
    let (a, b) = (s.u16_at(h + 0x64)? as usize, s.u16_at(h + 0x66)? as usize);
    let n = if a < 3 { a + b } else { a };
    let shots = s.records(h + 0x18, n, 28, 3)?;
    let n = s.u16_at(h + 0x6c)? as usize;
    s.records(h + 0x20, n, 28, 3)?;
    s.capture.world.sanims.push(SAnimRec { name: name.clone().unwrap_or_default(), header_at: h, stream, data: None, shots });
    Ok(name)
}

/// bo2mp: what the walk keeps of a siege animation (`crate::siege` reads
/// it): its header's file offset, its `StreamWrappedBuffer`'s (a retail
/// map's: the compressed animation streams from the packages), or its
/// resident `GfxSiegeAnimCompressed`'s six arrays (the mod tools' maps),
/// and its shot list's.
#[derive(Clone, Debug, Default)]
pub struct SAnimRec {
    pub name: String,
    pub header_at: usize,
    pub stream: Option<usize>,
    /// Parts, rotation key times, translation key times, rotation keys,
    /// translation keys, pivots: (file offset, count).
    pub data: Option<[Option<(usize, usize)>; 6]>,
    pub shots: Option<usize>,
}

/// Size of `SAnimData` (`sanim_resident`).
pub(crate) const SANIM_DATA: usize = 0x40;

/// bo2mp: a siege animation kept in the zone (the BO3 mod tools' maps;
/// measured on Steam Workshop `mp_mountain`, whose six resident ones end
/// each exactly where the next asset's header starts). Header as `sanim`
/// but 0x08 null and 0x10 `SAnimData*` (0x40):
///
/// ```text
/// 0x00..0x30 six pointers, loaded in order:
///   part table (12 each: u32 first rotation key, u32 first translation
///     key, u16 rotation keys, u16 translation keys), one per part
///   rotation key times (u16 each), translation key times (u16 each)
///   rotation keys (48 each: three quaternions, a spline's points)
///   translation keys (36 each: three vec3s)
///   per part a vec4 (16 each)
/// 0x30 u32 parts; 0x34 u32 rotation keys; 0x38 u32 translation keys
/// ```
///
/// then 0x18 loads one 12-byte record (`u16 0, u16 frames, u32 0, u32`;
/// the frame count is the header's at 0x58), and 0x20 its list as `sanim`.
pub fn sanim_resident(s: &mut Stream, h: usize) -> Result<Option<String>> {
    s.no_loads(h, crate::AssetType::SAnim.header_size(), &[0, 0x10, 0x18, 0x20])?;
    let name = s.string(h)?;
    let mut data = None;
    if let Some(d) = s.array(h + 0x10, SANIM_DATA, 7)? {
        s.no_loads(d, SANIM_DATA, &[0, 8, 0x10, 0x18, 0x20, 0x28])?;
        let (parts, rot, trans) = (count(s, d + 0x30)?, count(s, d + 0x34)?, count(s, d + 0x38)?);
        let a = s.records(d, parts, 12, 3)?.map(|o| (o, parts));
        let b = s.records(d + 8, rot, 2, 1)?.map(|o| (o, rot));
        let c = s.records(d + 0x10, trans, 2, 1)?.map(|o| (o, trans));
        let e = s.records(d + 0x18, rot, 48, 3)?.map(|o| (o, rot));
        let f = s.records(d + 0x20, trans, 36, 3)?.map(|o| (o, trans));
        data = Some([a, b, c, e, f, None]);
        let g = s.records(d + 0x28, parts, 16, 15)?.map(|o| (o, parts));
        if let Some(x) = data.as_mut() {
            x[5] = g;
        }
    }
    let shots = s.array(h + 0x18, 12, 3)?;
    let n = s.u16_at(h + 0x6c)? as usize;
    s.records(h + 0x20, n, 28, 3)?;
    s.capture.world.sanims.push(SAnimRec { name: name.clone().unwrap_or_default(), header_at: h, stream: None, data, shots });
    Ok(name)
}

/// `StreamerHint` (0x28; read on `mp_sector`, `mp_crucible`): 0x00 name,
/// 0x08 u32 count, 0x10 hints (24 each: a name, a u32 kind, 8 bytes), 0x18
/// u32 count, 0x20 a second list (64 each). Order: the name, the hints,
/// each hint's name (pictures' and models' names, or references to earlier
/// ones), then the second list and its two arrays.
pub fn streamer_hint(s: &mut Stream, h: usize) -> Result<Option<String>> {
    s.no_loads(h, crate::AssetType::StreamerHint.header_size(), &[0, 0x10, 0x20])?;
    let name = s.string(h)?;
    let n = count(s, h + 8)?;
    if let Some(t) = s.records(h + 0x10, n, 24, 7)? {
        for i in 0..n {
            s.string(t + 24 * i)?;
        }
    }
    // The second list (64 each; `mp_sector`, `mp_crucible`: one): 0x00 -1
    // (no pointer: nothing loads for it), 0x20 u32 count and 0x28 u16s,
    // 0x30 u32 count and 0x38 (u32, u32) pairs.
    let n = count(s, h + 0x18)?;
    if let Some(t) = s.records(h + 0x20, n, 64, 7)? {
        for i in 0..n {
            let r = t + 64 * i;
            let k = count(s, r + 0x20)?;
            s.records(r + 0x28, k, 2, 1)?;
            let k = count(s, r + 0x30)?;
            s.records(r + 0x38, k, 8, 3)?;
        }
    }
    Ok(name)
}

/// `ZBarrierDef` (0x300; `zm_factory`'s `zm_factory_debris`): the name,
/// the board count (u32 at 0x38), then six boards (0x70 each from 0x40)
/// whose models, effects and sounds are references to assets loaded
/// earlier on the maps read, and whose two animations (+0x38 the tear,
/// +0x40 the board's) point at the name of an animation loaded earlier.
/// Inline pointers past the name are not read yet.
pub fn zbarrier(s: &mut Stream, h: usize) -> Result<Option<String>> {
    let size = crate::AssetType::ZBarrier.header_size();
    s.no_loads(h, size, &[0])?;
    let name = s.string(h)?;
    // bo2mp: the boards as the game builds a barrier from its classname's
    // def (not from the map entity's `zbarrierboardmodel<i>` keys, which
    // a prefab can leave stale: Shadows of Evil's scissor gates carry a
    // magic box's), in `Capture::impact_fields` by offset: 0x38 the board
    // count (as text), each board's model (+0x20), upgraded model (+0x30),
    // tear animation (+0x38) and board animation (+0x40).
    if let Some(n) = name.as_ref().filter(|n| !n.starts_with(',')) {
        let count = s.u32_at(h + 0x38)?.min(ZBARRIER_BOARDS as u32);
        let mut kept = vec![(0x38, count.to_string())];
        for i in 0..ZBARRIER_BOARDS {
            let board = h + 0x40 + ZBARRIER_BOARD * i;
            for (at, by_name) in [(0x20, false), (0x30, false), (0x38, true), (0x40, true)] {
                let Ok(crate::ptr::Ptr::Ref { offset, .. }) = s.ptr_at(board + at) else { continue };
                let found = if by_name {
                    s.string_at(offset).map(|b| String::from_utf8_lossy(b).into_owned())
                } else {
                    s.named_at(offset).map(|(_, r)| r.trim_start_matches(',').to_owned())
                };
                if let Some(r) = found.filter(|r| !r.is_empty()) {
                    kept.push((0x40 + ZBARRIER_BOARD * i + at, r));
                }
            }
        }
        s.capture.impact_fields.insert((crate::AssetType::ZBarrier, n.clone()), kept);
    }
    Ok(name)
}

/// A `ZBarrierDef`'s boards: six of 0x70 bytes from 0x40.
pub const ZBARRIER_BOARDS: usize = 6;
pub const ZBARRIER_BOARD: usize = 0x70;

#[cfg(test)]
mod sanim_tests {
    use crate::AssetType;
    use crate::stream::Stream;

    /// A resident siege animation (the mod tools' layout, `sanim_resident`)
    /// walks to its end: 2 parts, 3 rotation keys, 1 translation key.
    #[test]
    fn resident_sanim_walks_to_its_end() {
        let mut z: Vec<u8> = vec![0; 0x70];
        let follow = |z: &mut Vec<u8>, at: usize| z[at..at + 8].copy_from_slice(&[0xff; 8]);
        for f in [0, 0x10, 0x18] {
            follow(&mut z, f);
        }
        z[0x58..0x5c].copy_from_slice(&300u32.to_le_bytes());
        z[0x64..0x66].copy_from_slice(&5u16.to_le_bytes());
        z[0x66..0x68].copy_from_slice(&1u16.to_le_bytes());
        z.extend_from_slice(b"p7_fxanim_test_sanim\0");
        // (The data struct follows the name in the file, as on the zones.)
        let d = z.len();
        z.extend(std::iter::repeat_n(0u8, super::SANIM_DATA));
        for f in 0..6 {
            follow(&mut z, d + 8 * f);
        }
        for (at, n) in [(0x30, 2u32), (0x34, 3), (0x38, 1)] {
            z[d + at..d + at + 4].copy_from_slice(&n.to_le_bytes());
        }
        let data = 2 * 12 + 3 * 2 + 2 + 3 * 48 + 36 + 2 * 16 + 12;
        z.extend(std::iter::repeat_n(0x11u8, data));
        let end = z.len();
        z.extend_from_slice(&[0xff; 16]);
        let mut s = Stream::new(&z);
        assert_eq!(s.load_asset(AssetType::SAnim).unwrap().as_deref(), Some("p7_fxanim_test_sanim"));
        assert_eq!(s.pos(), end);
        // A field the layout does not know still stops the walk.
        let mut bad = z.clone();
        bad[0x08..0x10].copy_from_slice(&[0xff; 8]);
        assert!(Stream::new(&bad).load_asset(AssetType::SAnim).is_err());
    }
}
