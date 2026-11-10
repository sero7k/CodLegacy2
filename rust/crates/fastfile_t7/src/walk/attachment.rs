//! Weapon attachments: `WeaponAttachment` (the `attachment` asset: one per
//! attachment kind, "acog", "extbarrel", ...) and `WeaponAttachmentUnique`
//! (the `attachmentunique` asset: an attachment on one weapon, with its
//! models, animations and overrides). Read on retail `core_common`, where
//! the first weapons load them where the weapon's lists point.
//!
//! ```text
//! WeaponAttachment 0x220: 0x00 char* name ("acog"); 0x08 char* displayName ("WEAPON_ACOG");
//!         0x1d8 char* (a UI3D reticle name, "MagnifierReticle_UI3D"); the rest numbers
//!         (type, flags, multipliers)
//! ```

use super::capture::FieldValue;
use super::weapon::ANIMS;
use crate::asset_type::AssetType;
use crate::cursor::Result;
use crate::stream::Stream;

pub fn attachment(s: &mut Stream, h: usize) -> Result<Option<String>> {
    s.no_loads(h, AssetType::Attachment.header_size(), &[0, 8, 0x1d8])?;
    let name = s.string(h)?;
    s.string(h + 8)?;
    s.string(h + 0x1d8)?;
    Ok(name)
}

/// `WeaponAttachmentUnique` pointer fields, in the exe loader's order.
#[derive(Clone, Copy)]
enum U {
    Str,
    /// An asset of this type.
    A(AssetType),
    /// `n` u32 script strings (no stream data), 4-aligned.
    ScriptStrings(usize),
    /// `n` f32s, 4-aligned.
    Floats(usize),
    /// 197 animation names.
    Anims,
    /// Five pointers to assets of this type (an array, 8-aligned).
    Assets5(AssetType),
    /// Five strings (an array, 8-aligned).
    Strs5,
}

const MODEL: U = U::A(AssetType::XModel);
const IMAGE: U = U::A(AssetType::Image);
const ACV: U = U::A(AssetType::AttachmentCosmeticVariant);

/// The exe's loader (stream order): name, two strings, two images, two
/// strings, 32 script strings at 0x60, two inline 0xf0 blocks at 0x68 and
/// 0x158 (each: a cosmetic variant at +8, two `{model, model, tag}` pairs of
/// 0x30 at +0x10 and +0x70, three strings at +0xd0, an image at +0xe8),
/// five models at 0x248 and 0x250, five strings at 0x258 and 0x260, the camo
/// 0x358, images 0x380 and 0x388, 197 animation names 0x3b8, 21 floats
/// 0x3e0, strings 0x420..0x4b8, 0x500..0x518, then 0x4c0..0x4f8, flash
/// effects 0x528/0x530, tracers 0x550/0x558, lasers 0x560/0x568.
fn unique_fields() -> Vec<(usize, U, String)> {
    let mut f: Vec<(usize, U, String)> = vec![
        (0x0, U::Str, "name".into()),
        (0x8, U::Str, "+0x8".into()),
        (0x10, U::Str, "+0x10".into()),
        (0x18, IMAGE, "hudIcon".into()),
        (0x20, IMAGE, "icon".into()),
        (0x40, U::Str, "displayName".into()),
        (0x50, U::Str, "+0x50".into()),
        (0x60, U::ScriptStrings(32), "+0x60".into()),
    ];
    for (k, base) in [0x68usize, 0x158].into_iter().enumerate() {
        f.push((base + 8, ACV, format!("acv{k}")));
        for (j, pair) in [0x10usize, 0x40, 0x70, 0xa0].into_iter().enumerate() {
            let m = 4 * k + j;
            f.push((base + pair, MODEL, format!("model{m}")));
            f.push((base + pair + 8, MODEL, format!("model{m}b")));
            f.push((base + pair + 0x10, U::Str, format!("tag{m}")));
        }
        for o in [0xd0usize, 0xd8, 0xe0] {
            f.push((base + o, U::Str, format!("+{:#x}", base + o)));
        }
        f.push((base + 0xe8, IMAGE, format!("+{:#x} image", base + 0xe8)));
    }
    f.push((0x248, U::Assets5(AssetType::XModel), "models5a".into()));
    f.push((0x250, U::Assets5(AssetType::XModel), "models5b".into()));
    f.push((0x258, U::Strs5, "+0x258".into()));
    f.push((0x260, U::Strs5, "+0x260".into()));
    f.push((0x358, U::A(AssetType::WeaponCamo), "camo".into()));
    f.push((0x380, IMAGE, "+0x380 image".into()));
    f.push((0x388, IMAGE, "+0x388 image".into()));
    f.push((0x3b8, U::Anims, "anims".into()));
    f.push((0x3e0, U::Floats(21), "locationDamageMultipliers".into()));
    for o in (0x420..=0x4b8).step_by(8).chain((0x500..=0x518).step_by(8)).chain((0x4c0..=0x4f8).step_by(8)) {
        f.push((o, U::Str, format!("+{o:#x}")));
    }
    f.push((0x528, U::A(AssetType::Fx), "viewFlashEffect".into()));
    f.push((0x530, U::A(AssetType::Fx), "worldFlashEffect".into()));
    f.push((0x550, U::A(AssetType::Tracer), "viewTracer".into()));
    f.push((0x558, U::A(AssetType::Tracer), "worldTracer".into()));
    f.push((0x560, U::A(AssetType::Laser), "viewLaser".into()));
    f.push((0x568, U::A(AssetType::Laser), "worldLaser".into()));
    f
}

/// `WeaponAttachmentUnique` (0x840): the fields of `unique_fields`, read
/// as the exe's loader reads them; what they name is kept in
/// `Capture::links` (animations as `anim[slot]`).
pub fn attachment_unique(s: &mut Stream, h: usize) -> Result<Option<String>> {
    let fields = unique_fields();
    let mut known: Vec<usize> = fields.iter().map(|f| f.0).collect();
    // (Words of -1 ints, not pointers: the loader reads nothing there.)
    known.extend([0x5c0, 0x5d8, 0x5f0, 0x600, 0x608, 0x6b8, 0x728, 0x750, 0x780, 0x790]);
    s.no_loads(h, AssetType::AttachmentUnique.header_size(), &known)?;
    let mut name = None;
    let mut links = Vec::new();
    for (off, kind, label) in fields {
        match kind {
            U::Str if off == 0 => name = s.string(h)?,
            U::Str => {
                if let Some(v) = s.string_named(h + off)?.filter(|v| !v.is_empty()) {
                    links.push((label, FieldValue::Str(v)));
                }
            }
            U::A(ty) => {
                if let Some(n) = s.asset_named(ty, h + off)? {
                    links.push((label, FieldValue::Asset { ty, name: n }));
                }
            }
            U::ScriptStrings(n) | U::Floats(n) => {
                s.array(h + off, 4 * n, 3)?;
            }
            U::Assets5(ty) => {
                if let Some(t) = s.records(h + off, 5, 8, 7)? {
                    for i in 0..5 {
                        if let Some(n) = s.asset_named(ty, t + 8 * i)? {
                            links.push((format!("{label}[{i}]"), FieldValue::Asset { ty, name: n }));
                        }
                    }
                }
            }
            U::Strs5 => {
                if let Some(t) = s.records(h + off, 5, 8, 7)? {
                    for i in 0..5 {
                        s.string(t + 8 * i)?;
                    }
                }
            }
            U::Anims => {
                if let Some(t) = s.records(h + off, ANIMS, 8, 7)? {
                    for i in 0..ANIMS {
                        if let Some(a) = s.string_named(t + 8 * i)?.filter(|a| !a.is_empty()) {
                            links.push((format!("anim[{i}]"), FieldValue::Str(a)));
                        }
                    }
                }
            }
        }
    }
    if let Some(n) = &name {
        s.capture.add_links(AssetType::AttachmentUnique, n, links);
        s.capture.unique_headers.insert(n.clone(), h);
    }
    Ok(name)
}
