//! The material records the game's pixel programs read.
//!
//! A family 12 pixel program's `GPU_PIPELINE_ID` input is the byte offset of
//! the material's record in one raw buffer (register `t39`, space 1) and it
//! reads the record with `bufferLoad(buffer, id + constant)` (`docs/JUP.md`):
//!
//! - constants at +36, +40 ... +412 in steps of 4: packed half pairs and
//!   shifted fields (texture-coordinate scale and rotation, colour scales,
//!   blend ranges); computed by the engine from the material's constant rows;
//! - flags at +484 (and +500): feature bits that switch layers on and off;
//! - picture indices at +516 to +692 in steps of 4: 45 entries into the
//!   bindless `Texture2D` ranges, a value above 270335 meaning none. They are
//!   very probably the material's slot pairs' pictures in the pairs' order.
//!
//! This builds the record from what the zone gives (the entries; the flags
//! from the layer count; constants zero until their meaning is decoded) for
//! the day a program can be driven.

/// Bytes between two records (the engine's stride is not known: the last
/// entry ends at +696, rounded up to 16).
pub const RECORD_BYTES: usize = 704;
/// The first constant, and the last.
pub const CONSTANTS_AT: usize = 36;
pub const CONSTANTS_END: usize = 416;
/// The feature bits' dword.
pub const FLAGS_AT: usize = 484;
/// The first picture-index entry.
pub const TEXTURE_BASE: usize = 516;
/// Picture-index entries a record holds.
pub const TEXTURE_SLOTS: usize = 45;
/// A picture index meaning none (above the bindless range's last index).
pub const NONE: u32 = 0xFFFF_FFFF;
/// The last index of the bindless ranges.
pub const BINDLESS_LAST: u32 = 270_335;

/// One material's pictures as bindless indices, by record entry.
#[derive(Clone, Debug, Default)]
pub struct MaterialPictures {
    pub entries: Vec<Option<u32>>,
}

/// The records of `materials` in one buffer; the byte offset of each is
/// [`offset_of`].
pub fn build(materials: &[MaterialPictures]) -> Vec<u8> {
    let mut out = vec![0u8; RECORD_BYTES * materials.len().max(1)];
    for (m, pics) in materials.iter().enumerate() {
        let rec = &mut out[m * RECORD_BYTES..(m + 1) * RECORD_BYTES];
        for s in 0..TEXTURE_SLOTS {
            let v = pics.entries.get(s).copied().flatten().unwrap_or(NONE);
            rec[TEXTURE_BASE + 4 * s..TEXTURE_BASE + 4 * s + 4].copy_from_slice(&v.to_le_bytes());
        }
    }
    out
}

/// A material's `GPU_PIPELINE_ID`: its record's byte offset.
pub fn offset_of(material: usize) -> u32 {
    (material * RECORD_BYTES) as u32
}
