//! Script bundles and the other small game-data assets.
//!
//! ```text
//! ScriptBundle 0x30: 0x00 char* name; 0x08 u32 type (script string); 0x0c u32 bundleType;
//!              0x10 u32 kvpCount; 0x18 ScriptBundleKVP* kvps;
//!              0x20 u32 objectCount; 0x28 ScriptBundleObject* objects
//! ScriptBundleObject 16: 0x00 u32 kvpCount; 0x08 ScriptBundleKVP* kvps
//! ScriptBundleKVP    24: 0x00 u32 key, 0x04 u32 value (script strings); 0x08 u32 type;
//!                    0x0c u32 data (int/float); 0x10 the asset `type` names (or null)
//! ```
//! (The layout as atian-cod-tools reads it from the running game, the
//! stream order read on retail `mp_common`: name, the bundle's pairs then
//! each pair's asset, the objects, then per object its pairs and their
//! assets.)

use super::capture::FieldValue;
use crate::asset_type::AssetType;
use crate::cursor::Result;
use crate::stream::Stream;

const KVP: usize = 24;
const OBJECT: usize = 16;

/// The asset type a script bundle pair of `kind` loads, as the exe's pair
/// loader does: 3/4 animation, 6 model, 0xa surface effect table, 0xd
/// camera, 0xf image. Other kinds' words are names or numbers: nothing
/// loads (kinds 1, 2 and 0x12, and a pair whose value word is 0, never
/// load).
pub fn kvp_asset(kind: u32) -> Option<AssetType> {
    use AssetType as T;
    Some(match kind {
        3 | 4 => T::XAnimParts,
        6 => T::XModel,
        0xa => T::SurfaceFxTable,
        0xd => T::XCam,
        0xf => T::Image,
        _ => return None,
    })
}

/// `count` pairs at `field`, then each pair's asset; bo2mp: each pair as a
/// link `<prefix>#<key script string>` = its value (the asset's name, else
/// `ss:<value script string>`, the number for an int or float).
fn kvps(s: &mut Stream, field: usize, count: usize, prefix: &str, links: &mut Vec<(String, FieldValue)>) -> Result<()> {
    if let Some(t) = s.records(field, count, KVP, 7)? {
        for i in 0..count {
            let p = t + KVP * i;
            let kind = s.u32_at(p + 8)?;
            let key = s.u32_at(p)?;
            let label = format!("{prefix}#{key}#{kind}");
            let loads = s.u32_at(p + 4)? != 0 && !matches!(kind, 1 | 2 | 0x12);
            match kvp_asset(kind).filter(|_| loads) {
                Some(ty) => match s.asset_named(ty, p + 0x10)? {
                    Some(n) => links.push((label, FieldValue::Asset { ty, name: n })),
                    None => links.push((label, FieldValue::Str(format!("ss:{} (no asset)", s.u32_at(p + 4)?)))),
                },
                None => {
                    let v = s.u32_at(p + 4)?;
                    let d = s.u32_at(p + 0xc)?;
                    links.push((label, FieldValue::Str(format!("ss:{v} data:{d:#x}"))));
                }
            }
        }
    }
    Ok(())
}

pub fn script_bundle(s: &mut Stream, h: usize) -> Result<Option<String>> {
    // (The exe's loader reads the name, the pairs and the objects; the
    // words at 8..0x18 are script strings and counts.)
    let name = s.string(h)?;
    let mut links = Vec::new();
    kvps(s, h + 0x18, s.u32_at(h + 0x10)? as usize, "bundle", &mut links)?;
    let n = s.u32_at(h + 0x20)? as usize;
    if let Some(t) = s.records(h + 0x28, n, OBJECT, 7)? {
        for i in 0..n {
            let o = t + OBJECT * i;
            kvps(s, o + 8, s.u32_at(o)? as usize, &format!("object{i}"), &mut links)?;
        }
    }
    if let Some(n) = &name {
        s.capture.add_links(AssetType::ScriptBundle, n, links);
    }
    Ok(name)
}

const ACV_VARIANT: usize = 0xe0;
const ACV_MODEL: usize = 48;

/// `AttachmentCosmeticVariant` (0x18: `{ char* name; Variant* variants;
/// u32 count }`), `Variant` 0xe0: four `{ XModel* model; XModel*
/// adsModel; char* tag; vec3 position, rotation }` (48), then `char*
/// shortDisplayName, longDisplayName, description; GfxImage* icon`
/// (HydraX's layout; the icon is an image, read on `core_common`'s
/// `acv_extbarrel_smg_standard`). Order: name, the variants, then per variant its
/// models' fields and the four pointers, in field order.
pub fn attachment_cosmetic_variant(s: &mut Stream, h: usize) -> Result<Option<String>> {
    s.no_loads(h, AssetType::AttachmentCosmeticVariant.header_size(), &[0, 8])?;
    let name = s.string(h)?;
    let n = s.u32_at(h + 0x10)? as usize;
    let mut links = Vec::new();
    if let Some(t) = s.records(h + 8, n, ACV_VARIANT, 7)? {
        for i in 0..n {
            let v = t + ACV_VARIANT * i;
            let mut known: Vec<usize> = (0..4).flat_map(|m| [ACV_MODEL * m, ACV_MODEL * m + 8, ACV_MODEL * m + 16]).collect();
            known.extend([0xc0, 0xc8, 0xd0, 0xd8]);
            s.no_loads(v, ACV_VARIANT, &known)?;
            for m in 0..4 {
                let vm = v + ACV_MODEL * m;
                for (f, label) in [(0, "model"), (8, "adsModel")] {
                    if let Some(n) = s.asset_named(AssetType::XModel, vm + f)? {
                        links.push((format!("variant[{i}].{label}[{m}]"), FieldValue::Asset { ty: AssetType::XModel, name: n }));
                    }
                }
                if let Some(t) = s.string_named(vm + 16)?.filter(|t| !t.is_empty()) {
                    links.push((format!("variant[{i}].tag[{m}]"), FieldValue::Str(t)));
                }
            }
            for (f, label) in [(0xc0, "shortDisplayName"), (0xc8, "longDisplayName"), (0xd0, "description")] {
                if let Some(t) = s.string_named(v + f)?.filter(|t| !t.is_empty()) {
                    links.push((format!("variant[{i}].{label}"), FieldValue::Str(t)));
                }
            }
            if let Some(n) = s.asset_named(AssetType::Image, v + 0xd8)? {
                links.push((format!("variant[{i}].icon"), FieldValue::Asset { ty: AssetType::Image, name: n }));
            }
        }
    }
    if let Some(n) = &name {
        s.capture.add_links(AssetType::AttachmentCosmeticVariant, n, links);
    }
    Ok(name)
}

const RUMBLE_GRAPH: usize = 0x90;

/// `RumbleInfo` (0x40: `{ char* name; i32 duration; f32 range;
/// RumbleGraph* high, low; ...ints and floats }`), `RumbleGraph` 0x90:
/// `{ char* name; i32 knotCount; vec2 knots[16] }` (HydraX's fields).
/// Order: name, then each graph's struct and name.
pub fn rumble(s: &mut Stream, h: usize) -> Result<Option<String>> {
    s.no_loads(h, AssetType::Rumble.header_size(), &[0, 0x10, 0x18])?;
    let name = s.string(h)?;
    for f in [0x10, 0x18] {
        if let Some(g) = s.array(h + f, RUMBLE_GRAPH, 7)? {
            s.no_loads(g, RUMBLE_GRAPH, &[0])?;
            s.string(g)?;
        }
    }
    Ok(name)
}

const CAMO: usize = 16;
const CAMO_MATERIAL: usize = 0x138;
const CAMO_BASE: usize = 16;

/// `WeaponCamo` (0x18: `{ char* name; Camo* camos; u32 count }`); `Camo`
/// 16: `{ u32 count; CamoMaterial* materials }`; `CamoMaterial` 0x138:
/// `{ u32 baseCount; BaseMaterial* base (16: Material*, GfxImage* mask);
/// 0x10 ?; 0x18 flags; 0x20 Material*; transforms; 0x48 GfxImage*
/// detailNormalMap; ... }` (HydraX). Order: name, the camos, per camo its
/// materials, per material its base list (each base's material and
/// mask), then its material and detail normal map.
pub fn weapon_camo(s: &mut Stream, h: usize) -> Result<Option<String>> {
    s.no_loads(h, AssetType::WeaponCamo.header_size(), &[0, 8])?;
    let name = s.string(h)?;
    let n = s.u32_at(h + 0x10)? as usize;
    let mut links = Vec::new();
    let keep = |links: &mut Vec<(String, FieldValue)>, label: String, ty: AssetType, n: Option<String>| {
        if let Some(n) = n {
            links.push((label, FieldValue::Asset { ty, name: n }));
        }
    };
    if let Some(t) = s.records(h + 8, n, CAMO, 7)? {
        for i in 0..n {
            let c = t + CAMO * i;
            let m = s.u32_at(c)? as usize;
            if let Some(mt) = s.records(c + 8, m, CAMO_MATERIAL, 7)? {
                for k in 0..m {
                    let cm = mt + CAMO_MATERIAL * k;
                    s.no_loads(cm, CAMO_MATERIAL, &[8, 0x20, 0x48])?;
                    let b = s.u32_at(cm)? as usize;
                    if let Some(bt) = s.records(cm + 8, b, CAMO_BASE, 7)? {
                        for j in 0..b {
                            let v = s.asset_named(AssetType::Material, bt + CAMO_BASE * j)?;
                            keep(&mut links, format!("camo[{i}][{k}].base[{j}].material"), AssetType::Material, v);
                            let v = s.asset_named(AssetType::Image, bt + CAMO_BASE * j + 8)?;
                            keep(&mut links, format!("camo[{i}][{k}].base[{j}].mask"), AssetType::Image, v);
                        }
                    }
                    let v = s.asset_named(AssetType::Material, cm + 0x20)?;
                    keep(&mut links, format!("camo[{i}][{k}].material"), AssetType::Material, v);
                    let v = s.asset_named(AssetType::Image, cm + 0x48)?;
                    keep(&mut links, format!("camo[{i}][{k}].detailNormalMap"), AssetType::Image, v);
                }
            }
        }
    }
    if let Some(n) = &name {
        s.capture.add_links(AssetType::WeaponCamo, n, links);
    }
    Ok(name)
}

const TAG_FX_ITEM: usize = 0x70;

/// `TagFx` (0x18: `{ char* name; u32 count; TagFxItem* items }`, HydraX),
/// `TagFxItem` 0x70: `{ FxEffectDef* fx; ...; u32 tag, delay, bolted }`.
/// Order: name, the items, each item's effect.
pub fn tag_fx(s: &mut Stream, h: usize) -> Result<Option<String>> {
    s.no_loads(h, AssetType::TagFx.header_size(), &[0, 0x10])?;
    let name = s.string(h)?;
    let n = s.u32_at(h + 8)? as usize;
    if let Some(t) = s.records(h + 0x10, n, TAG_FX_ITEM, 7)? {
        for i in 0..n {
            s.no_loads(t + TAG_FX_ITEM * i, TAG_FX_ITEM, &[0])?;
            s.asset(AssetType::Fx, t + TAG_FX_ITEM * i)?;
        }
    }
    Ok(name)
}

/// `MedalTable` (0x18: `{ char* name; u32 count; Medal** medals }`, read
/// on mp_common): name, the medal pointers, then each medal (most are
/// references to the table's medals, some load here).
pub fn medal_table(s: &mut Stream, h: usize) -> Result<Option<String>> {
    s.no_loads(h, AssetType::MedalTable.header_size(), &[0, 0x10])?;
    let name = s.string(h)?;
    let n = s.u32_at(h + 8)? as usize;
    if let Some(t) = s.records(h + 0x10, n, 8, 7)? {
        for i in 0..n {
            s.asset(AssetType::Medal, t + 8 * i)?;
        }
    }
    Ok(name)
}

const LB_COLUMN: usize = 0x40;

/// `LeaderboardDef` (0x30, Black Ops II's fields on x64: `{ char* name; u32
/// id; i32 columnCount, dwColumnCount, xpColId, prestigeColId;
/// LbColumnDef* columns; updateType; trackTypes }`), `LbColumnDef` 0x40:
/// `{ char* name; i32 colId, dwColIndex; bool hidden; char* statName;
/// type, precision, agg; char* localization; i32 uiCalColX, Y }`. Order:
/// name, the columns, each column's three strings.
pub fn leaderboard(s: &mut Stream, h: usize) -> Result<Option<String>> {
    s.no_loads(h, AssetType::Leaderboard.header_size(), &[0, 0x20])?;
    let name = s.string(h)?;
    let n = s.i32_at(h + 0xc)?.max(0) as usize;
    if let Some(t) = s.records(h + 0x20, n, LB_COLUMN, 7)? {
        for i in 0..n {
            let c = t + LB_COLUMN * i;
            s.no_loads(c, LB_COLUMN, &[0, 0x18, 0x30])?;
            for f in [0, 0x18, 0x30] {
                s.string(c + f)?;
            }
        }
    }
    Ok(name)
}

/// `ObjectiveList` (0x18: `{ char* name; u32 count; Objective** objectives }`,
/// read on mp_common): like the medal table.
pub fn objective_list(s: &mut Stream, h: usize) -> Result<Option<String>> {
    s.no_loads(h, AssetType::ObjectiveList.header_size(), &[0, 0x10])?;
    let name = s.string(h)?;
    let n = s.u32_at(h + 8)? as usize;
    if let Some(t) = s.records(h + 0x10, n, 8, 7)? {
        for i in 0..n {
            s.asset(AssetType::Objective, t + 8 * i)?;
        }
    }
    Ok(name)
}

const BG_CACHE_ENTRY: usize = 0x84;

/// `BgCache` (0x18, read on mp_common: `{ char* name; Entry* entries; u32
/// count }`), entries 0x84: `{ u32 type; char name[128] }` (the names are
/// inline: vehicles, weapons, models the server caches). Name, entries.
pub fn bg_cache(s: &mut Stream, h: usize) -> Result<Option<String>> {
    s.no_loads(h, AssetType::BgCache.header_size(), &[0, 8])?;
    let name = s.string(h)?;
    let n = s.u32_at(h + 0x10)? as usize;
    s.records(h + 8, n, BG_CACHE_ENTRY, 3)?;
    Ok(name)
}

/// `ScriptBundleList` (0x20, read on retail `core_pre_gfx`): 0x00 char*
/// name; 0x0c u32 script string; 0x10 u32 count; 0x18
/// ScriptBundle** bundles (count asset pointers).
pub fn script_bundle_list(s: &mut Stream, h: usize) -> Result<Option<String>> {
    s.no_loads(h, AssetType::ScriptBundleList.header_size(), &[0, 0x18])?;
    let name = s.string(h)?;
    let n = s.u32_at(h + COUNT_AT)? as usize;
    if let Some(t) = s.records(h + 0x18, n, 8, 7)? {
        for i in 0..n {
            s.asset_named(AssetType::ScriptBundle, t + 8 * i)?;
        }
    }
    Ok(name)
}
const COUNT_AT: usize = 0x10;

/// bo2mp: assets whose retail copies (the default ones in `core_pre_gfx`)
/// are a header only: the name, every other pointer null or a reference to
/// data loaded earlier (sizes from the exe's struct size table:
/// PlayerSoundsTable 0x448, PlayerFXTable 0x98, CustomizationTable_FEImages
/// 0x18, MapTableLoadingImages 0x18, DDLRoot 0x10). A copy with data of its
/// own stops the walk (its layout is not read yet).
fn header_only(s: &mut Stream, h: usize, ty: AssetType) -> Result<Option<String>> {
    s.no_loads(h, ty.header_size(), &[0])?;
    s.string(h)
}
pub fn player_sounds(s: &mut Stream, h: usize) -> Result<Option<String>> {
    header_only(s, h, AssetType::PlayerSounds)
}
pub fn player_fx(s: &mut Stream, h: usize) -> Result<Option<String>> {
    header_only(s, h, AssetType::PlayerFx)
}
pub fn customization_table_fe_images(s: &mut Stream, h: usize) -> Result<Option<String>> {
    header_only(s, h, AssetType::CustomizationTableFeImages)
}
/// `MapTableLoadingImages` (0x18), as the exe's loader reads it: name;
/// u32 count at 8; count image pointers at 0x10 (aligned 8).
pub fn map_table_loading_images(s: &mut Stream, h: usize) -> Result<Option<String>> {
    let name = s.string(h)?;
    let n = s.u32_at(h + 8)? as usize;
    if let Some(t) = s.records(h + 0x10, n, 8, 7)? {
        for i in 0..n {
            s.asset_named(AssetType::Image, t + 8 * i)?;
        }
    }
    Ok(name)
}
/// `DDLRoot` (0x10), as the exe's loader reads it: name; a `DDLDef` chain
/// (0x60 each, aligned 8): name; `DDLStruct`[u32 0x28] at 0x20 (0x38: name;
/// `DDLMember`[u32 +0xc] at +0x10 (0x48: name; one byte at +0x10); two hash
/// tables `{ptr, u32 count}` of 8-byte entries at +0x18 and +0x28, aligned
/// 4; members hold -1 ints the loader leaves alone); `DDLEnum`[u32 0x38] at 0x30 (0x28: name; char*[u32 +8] at +0x10; a
/// hash table at +0x18); the next def at 0x40.
pub fn ddl(s: &mut Stream, h: usize) -> Result<Option<String>> {
    s.no_loads(h, AssetType::Ddl.header_size(), &[0, 8])?;
    let name = s.string(h)?;
    let mut field = h + 8;
    while let Some(d) = s.array(field, 0x60, 7)? {
        s.string(d)?;
        let n = s.u32_at(d + 0x28)? as usize;
        if let Some(t) = s.records(d + 0x20, n, 0x38, 7)? {
            for i in 0..n {
                let r = t + 0x38 * i;
                s.string(r)?;
                let m = s.u32_at(r + 0xc)? as usize;
                if let Some(mt) = s.records(r + 0x10, m, 0x48, 7)? {
                    for k in 0..m {
                        let e = mt + 0x48 * k;
                        s.string(e)?;
                        s.array(e + 0x10, 1, 0)?;
                    }
                }
                for ht in [0x18, 0x28] {
                    let c = s.u32_at(r + ht + 8)? as usize;
                    s.array(r + ht, 8 * c, 3)?;
                }
            }
        }
        let n = s.u32_at(d + 0x38)? as usize;
        if let Some(t) = s.records(d + 0x30, n, 0x28, 7)? {
            for i in 0..n {
                let e = t + 0x28 * i;
                s.string(e)?;
                let m = s.u32_at(e + 8)? as usize;
                if let Some(mt) = s.records(e + 0x10, m, 8, 7)? {
                    for k in 0..m {
                        s.string(mt + 8 * k)?;
                    }
                }
                let c = s.u32_at(e + 0x20)? as usize;
                s.array(e + 0x18, 8 * c, 3)?;
            }
        }
        field = d + 0x40;
    }
    Ok(name)
}

/// `SndPatch` (0x18), as the exe's loader reads it: name; u32 count at 8;
/// count u32s at 0x10 (aligned 4).
pub fn sound_patch(s: &mut Stream, h: usize) -> Result<Option<String>> {
    let name = s.string(h)?;
    let n = s.u32_at(h + 8)? as usize;
    s.array(h + 0x10, 4 * n, 3)?;
    Ok(name)
}

/// `GfxLightDef` (0x28): the exe's loader reads its name only.
pub fn light_def(s: &mut Stream, h: usize) -> Result<Option<String>> {
    s.string(h)
}

/// `SndDriverGlobals` (0x78), as the exe's loader reads it: name; seven
/// `{u32 count; ptr}` arrays at 0x08.. of 0x9c, 0x84, 0x60, 0x44, 0x498,
/// 0x14c, 0x84 bytes per entry (all aligned 4).
pub fn snd_driver_globals(s: &mut Stream, h: usize) -> Result<Option<String>> {
    let name = s.string(h)?;
    for (k, size) in [0x9cusize, 0x84, 0x60, 0x44, 0x498, 0x14c, 0x84].into_iter().enumerate() {
        let n = s.u32_at(h + 8 + 0x10 * k)? as usize;
        s.array(h + 0x10 + 0x10 * k, n * size, 3)?;
    }
    Ok(name)
}

/// `StructuredTable` (0x38), as the exe's loader reads it: name; u32
/// columns 0xc, rows 0x10; cells (0x18 each, a char* at +8) 0x18; one u32
/// per cell 0x20; column headers (0x10 each, a char* at 0) 0x28; one u32
/// per column 0x30.
pub fn structured_table(s: &mut Stream, h: usize) -> Result<Option<String>> {
    let name = s.string(h)?;
    let (cols, rows) = (s.u32_at(h + 0xc)? as usize, s.u32_at(h + 0x10)? as usize);
    if let Some(t) = s.records(h + 0x18, cols * rows, 0x18, 7)? {
        for i in 0..cols * rows {
            s.string(t + 0x18 * i + 8)?;
        }
    }
    s.array(h + 0x20, 4 * cols * rows, 3)?;
    if let Some(t) = s.records(h + 0x28, cols, 0x10, 7)? {
        for i in 0..cols {
            s.string(t + 0x10 * i)?;
        }
    }
    s.array(h + 0x30, 4 * cols, 3)?;
    Ok(name)
}
