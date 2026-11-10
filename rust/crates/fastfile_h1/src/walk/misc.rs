//! Small assets: names, raw files, string tables, physics, tracers,
//! vehicles.

use super::{AssetId, Walker};
use crate::asset_type::AssetType;
use crate::cursor::{ReadError, Result};
use crate::layout as L;
use crate::stream::{SCRIPT, VIRTUAL};
use crate::walk::{OtherRec, RawFileRec, Span};

pub(super) fn named_only(
    w: &mut Walker,
    ty: AssetType,
    h: usize,
    name_off: usize,
) -> Result<AssetId> {
    w.s.push(VIRTUAL);
    let name = w.xstr(h + name_off)?.unwrap_or_default();
    w.s.pop();
    Ok(w.name_only(ty, name))
}

/// `PhysWaterPreset`: a name and seven effects (splashes, wakes), as
/// x64-zt's S1 layout (`code_post_gfx_mp` has them before its effects).
pub(super) fn phys_water_preset(w: &mut Walker, h: usize) -> Result<AssetId> {
    use L::PhysWaterPreset as P;
    w.s.push(VIRTUAL);
    let name = w.xstr(h + P::name)?.unwrap_or_default();
    for f in [P::fx0, P::fx1, P::fx2, P::fx3, P::fx4, P::fx5, P::fx6] {
        w.asset(AssetType::Fx, h + f)?;
    }
    w.s.pop();
    Ok(w.name_only(AssetType::PhysWaterPreset, name))
}

/// `Clut` (a colour lookup volume): width x height x depth RGBA8 texels,
/// then its name (x64-zt's S1 member order).
pub(super) fn clut(w: &mut Walker, h: usize) -> Result<AssetId> {
    use L::Clut as C;
    w.s.push(VIRTUAL);
    let dims = [C::width, C::height, C::depth]
        .iter()
        .map(|&f| w.s.u32_at(h + f))
        .collect::<Result<Vec<_>>>()?;
    let n = dims.iter().map(|&d| d as usize).product::<usize>();
    let at = w.blob(h + C::pixels, n * 4, 0)?;
    let name = w.xstr(h + C::name)?.unwrap_or_default();
    w.s.pop();
    if !name.starts_with(',') {
        w.cap.cluts.push((
            name.clone(),
            [dims[0], dims[1], dims[2]],
            at.map(|pos| super::capture::Span { pos, len: n * 4 }),
        ));
    }
    Ok(w.name_only(AssetType::Clut, name))
}

/// bo2mp: a header that names only itself: every pointer after the name
/// (`ptrs`, field offsets) null. Anything more is refused (its layout is
/// not read here): `code_post_gfx_mp`'s empty `ScriptableDef` and its
/// `AnimClass` (88 bytes, measured) are of this kind.
pub(super) fn name_only_header(
    w: &mut Walker,
    ty: AssetType,
    h: usize,
    ptrs: &[usize],
) -> Result<AssetId> {
    for &p in ptrs {
        if w.s.u64_at(h + p)? != 0 {
            return Err(ReadError::Invalid(ty.name(), h + p));
        }
    }
    named_only(w, ty, h, 0)
}

pub(super) fn net_const_strings(w: &mut Walker, h: usize) -> Result<AssetId> {
    use L::NetConstStrings as N;
    w.s.push(VIRTUAL);
    let name = w.xstr(h + N::name)?.unwrap_or_default();
    let count = w.s.u32_at(h + N::entryCount)? as usize;
    if let Some(table) = w.inline_array(h + N::stringList, 8 * count, 7)? {
        for i in 0..count {
            w.xstr(table + 8 * i)?;
        }
    }
    w.s.pop();
    Ok(w.name_only(AssetType::NetConstStrings, name))
}

pub(super) fn raw_file(w: &mut Walker, h: usize) -> Result<AssetId> {
    use L::RawFile as R;
    w.s.push(VIRTUAL);
    let name = w.xstr(h + R::name)?.unwrap_or_default();
    let compressed_len = w.s.u32_at(h + R::compressedLen)?;
    let len = w.s.u32_at(h + R::len)?;
    let n = if compressed_len != 0 {
        compressed_len as usize
    } else {
        len as usize + 1
    };
    let data = w.blob(h + R::buffer, n, 0)?.map(|pos| Span { pos, len: n });
    w.s.pop();
    w.cap.raw_files.push(RawFileRec {
        name,
        compressed_len,
        len,
        data,
    });
    Ok(AssetId {
        ty: AssetType::RawFile,
        index: (w.cap.raw_files.len() - 1) as u32,
    })
}

pub(super) fn script_file(w: &mut Walker, h: usize) -> Result<AssetId> {
    use L::ScriptFile as S;
    w.s.push(VIRTUAL);
    let name = w.xstr(h + S::name)?.unwrap_or_default();
    w.s.push(SCRIPT);
    let compressed = w.s.u32_at(h + S::compressedLen)? as usize;
    let bytecode = w.s.u32_at(h + S::bytecodeLen)? as usize;
    let len = w.s.u32_at(h + S::len)?;
    let buffer = w.blob(h + S::buffer, compressed, 0)?.map(|pos| Span { pos, len: compressed });
    let code = w.blob(h + S::bytecode, bytecode, 0)?.map(|pos| Span { pos, len: bytecode });
    w.s.pop();
    w.s.pop();
    w.cap.script_files.push(crate::walk::ScriptFileRec {
        name: name.clone(),
        compressed_len: compressed as u32,
        len,
        bytecode_len: bytecode as u32,
        buffer,
        bytecode: code,
    });
    Ok(w.name_only(AssetType::ScriptFile, name))
}

pub(super) fn string_table(w: &mut Walker, h: usize) -> Result<AssetId> {
    use L::StringTable as S;
    w.s.push(VIRTUAL);
    let name = w.xstr(h + S::name)?.unwrap_or_default();
    let columns = w.s.u32_at(h + S::columnCount)? as usize;
    let n = columns * (w.s.u32_at(h + S::rowCount)? as usize);
    let mut cells = Vec::with_capacity(n);
    if let Some(values) = w.inline_array(h + S::values, L::StringTableCell::SIZE * n, 3)? {
        for i in 0..n {
            cells.push(
                w.xstr(values + L::StringTableCell::SIZE * i + L::StringTableCell::string)?
                    .unwrap_or_default(),
            );
        }
    }
    w.s.pop();
    if !name.starts_with(',') {
        w.cap.string_tables.push((name.clone(), columns, cells));
    }
    Ok(w.name_only(AssetType::StringTable, name))
}

pub(super) fn lua_file(w: &mut Walker, h: usize) -> Result<AssetId> {
    use L::LuaFile as F;
    w.s.push(VIRTUAL);
    let name = w.xstr(h + F::name)?.unwrap_or_default();
    let len = w.s.u32_at(h + F::len)? as usize;
    w.blob(h + F::buffer, len, 15)?;
    w.s.pop();
    Ok(w.name_only(AssetType::LuaFile, name))
}

pub(super) fn localize(w: &mut Walker, h: usize) -> Result<AssetId> {
    use L::LocalizeEntry as E;
    w.s.push(VIRTUAL);
    let value = w.xstr(h + E::value)?;
    let name = w.xstr(h + E::name)?.unwrap_or_default();
    w.s.pop();
    if let Some(value) = value {
        w.cap.localize.insert(name.clone(), value);
    }
    Ok(w.name_only(AssetType::LocalizeEntry, name))
}

pub(super) fn phys_preset(w: &mut Walker, h: usize) -> Result<AssetId> {
    use L::PhysPreset as P;
    w.s.push(VIRTUAL);
    let name = w.xstr(h + P::name)?.unwrap_or_default();
    w.xstr(h + P::sndAliasPrefix)?;
    w.s.pop();
    Ok(w.name_only(AssetType::PhysPreset, name))
}

/// A `dmPolytopeData` (a physics collision hull), in x64-zt's order.
fn polytope(w: &mut Walker, field: usize) -> Result<()> {
    let Some(d) = w.inline_array(field, L::dmPolytopeData::SIZE, 3)? else {
        return Ok(());
    };
    polytope_contents(w, d)
}

/// A `dmPolytopeData`'s arrays (the record itself read at `d`).
pub(super) fn polytope_contents(w: &mut Walker, d: usize) -> Result<()> {
    use L::dmPolytopeData as D;
    let verts = w.s.u32_at(d + D::m_vertexCount)? as usize;
    let faces = w.s.u32_at(d + D::m_faceCount)? as usize;
    let edges = w.s.u32_at(d + D::m_subEdgeCount)? as usize;
    w.blob(d + D::m_aVertices, 16 * verts, 15)?;
    w.blob(d + D::m_aPlanes, 16 * faces, 15)?;
    w.blob(d + D::m_aSubEdges, 4 * edges, 3)?;
    w.blob(d + D::m_aFaceSubEdges, faces, 0)?;
    w.blob(d + D::m_surfaceTypes, 2 * faces, 1)?;
    w.blob(d + D::m_vertexMaterials, 2 * verts, 1)?;
    Ok(())
}

pub(super) fn phys_collmap(w: &mut Walker, h: usize) -> Result<AssetId> {
    use L::PhysCollmap as P;
    w.s.push(VIRTUAL);
    let name = w.xstr(h + P::name)?.unwrap_or_default();
    let count = w.s.u32_at(h + P::count)? as usize;
    if let Some(geoms) = w.inline_array(h + P::geoms, 8 * count, 3)? {
        for i in 0..count {
            polytope(w, geoms + 8 * i)?;
        }
    }
    w.s.pop();
    Ok(w.name_only(AssetType::PhysCollmap, name))
}

pub(super) fn tracer(w: &mut Walker, h: usize) -> Result<AssetId> {
    use L::TracerDef as T;
    w.s.push(VIRTUAL);
    let name = w.xstr(h + T::name)?.unwrap_or_default();
    let material = w.asset(AssetType::Material, h + T::material)?;
    w.s.pop();
    // bo2mp: kept for the guest guns' tracers.
    w.cap.tracers.push(super::TracerRec {
        name: name.clone(),
        material,
        record: Span {
            pos: h,
            len: T::SIZE,
        },
    });
    Ok(w.name_only(AssetType::Tracer, name))
}

pub(super) fn light_def(w: &mut Walker, h: usize) -> Result<AssetId> {
    use L::GfxLightDef as D;
    w.s.push(VIRTUAL);
    let name = w.xstr(h + D::name)?.unwrap_or_default();
    w.asset(AssetType::Image, h + D::attenuation__image)?;
    w.asset(AssetType::Image, h + D::cucoloris__image)?;
    w.s.pop();
    Ok(w.name_only(AssetType::LightDef, name))
}

/// A sound named by a weapon or vehicle (`SndAliasCustom`): an 8-byte
/// name slot (VIRTUAL, aligned 8) then the name.
pub(super) fn sound_custom(w: &mut Walker, field: usize) -> Result<Option<String>> {
    match w.s.ptr_at(field)? {
        crate::Ptr::Follow | crate::Ptr::Insert => {
            if w.s.ptr_at(field)? == crate::Ptr::Insert {
                w.s.insert_slot();
            }
            let slot = w.st(8, 7)?;
            w.xstr(slot)
        }
        crate::Ptr::Ref {
            block: VIRTUAL,
            offset,
        } => {
            // A reference to an earlier name slot: its name.
            let Some(at) = w.vmap.file_pos(offset as u64) else {
                return Ok(None);
            };
            match w.s.ptr_at(at)? {
                crate::Ptr::Ref {
                    block: VIRTUAL,
                    offset,
                } => Ok(w.string_at(offset as u64)),
                _ => Ok(w.string_at(offset as u64 + 8)),
            }
        }
        _ => Ok(None),
    }
}

pub(super) fn vehicle(w: &mut Walker, h: usize) -> Result<AssetId> {
    use L::VehicleDef as V;
    w.s.push(VIRTUAL);
    let name = w.xstr(h + V::name)?.unwrap_or_default();
    w.xstr(h + V::useHintString)?;
    w.xstr(h + V::vehPhysDef__physPresetName)?;
    w.asset(AssetType::PhysPreset, h + V::vehPhysDef__physPreset)?;
    w.xstr(h + V::vehPhysDef__physMassName)?;
    w.xstr(h + V::vehPhysDef__accelGraphName)?;
    // (Tread, hand brake and boost effects sit before the helicopter's.)
    for f in [
        V::treadDefaultFx,
        V::handBrakeDefaultFx,
        V::handBrakeLeftFx,
        V::handBrakeRightFx,
        V::boostFx,
    ] {
        w.asset(AssetType::Fx, h + f)?;
    }
    w.asset(AssetType::Fx, h + V::vehHelicopterGroundFx)?;
    w.asset(AssetType::Fx, h + V::vehHelicopterGroundWaterFx)?;
    w.xstr(h + V::vehDiveboatSteeringGraphName)?;
    w.xstr(h + V::turretWeaponName)?;
    // (The zone tools never write the turret's weapon; retail vehicles
    // carry it inline.)
    w.asset(AssetType::Weapon, h + V::turretWeapon)?;
    sound_custom(w, h + V::turretSpinSnd)?;
    sound_custom(w, h + V::turretStopSnd)?;
    w.asset(AssetType::Fx, h + V::trophyExplodeFx)?;
    w.asset(AssetType::Fx, h + V::trophyFlashFx)?;
    for f in [
        V::compassFriendlyIcon,
        V::compassEnemyIcon,
        V::compassFriendlyAltIcon,
        V::compassEnemyAltIcon,
    ] {
        w.asset(AssetType::Material, h + f)?;
    }
    for f in [
        V::idleLowSnd,
        V::idleHighSnd,
        V::engineLowSnd,
        V::engineHighSnd,
        V::idleLowSndAlt,
        V::idleHighSndAlt,
        V::engineLowSndAlt,
        V::engineHighSndAlt,
        V::turretSpinSndAlt,
        V::turretStopSndAlt,
        V::engineStartUpSnd,
        V::engineShutdownSnd,
        V::engineIdleSnd,
        V::engineSustainSnd,
        V::engineRampUpSnd,
        V::engineRampDownSnd,
        V::suspensionSoftSnd,
        V::suspensionHardSnd,
        V::collisionSnd,
        V::speedSnd,
    ] {
        sound_custom(w, h + f)?;
    }
    w.xstr(h + V::surfaceSndPrefix)?;
    for i in 0..31 {
        sound_custom(w, h + V::surfaceSnds + 8 * i)?;
    }
    w.xstr(h + V::soundTriggerOverrideZone)?;
    w.s.pop();
    Ok(w.name_only(AssetType::Vehicle, name))
}

#[allow(dead_code)]
fn _unused(_: OtherRec) {}

/// `FxImpactTable` { name; FxImpactEntry* table } with 15 entries of 38
/// effects each (Ghosts' surface types plus flesh; measured on retail
/// common_mp: 570 effect pointers between the name and the next asset).
pub(super) const IMPACT_FX_EFFECTS: usize = 19 * 60;

/// Debug override of an effect table's size (`S1_FXTABLE_<ty>=<n>`).
pub(super) fn table_size(ty: AssetType, default: usize) -> usize {
    #[cfg(test)]
    if let Ok(v) = std::env::var(format!("S1_FXTABLE_{}", ty.name())) {
        return v.parse().unwrap_or(default);
    }
    let _ = ty;
    default
}

/// `SurfaceFxTable` { name; SurfaceFxEntry* table }: 6 entries of 31
/// effects (186 pointers in retail common_mp).
pub(super) const SURFACE_FX_EFFECTS: usize = 6 * 53;

/// An effect table (`ImpactFx`, `SurfaceFx`): name, then a pointer to
/// `effects` effect pointers.
pub(super) fn fx_table(w: &mut Walker, ty: AssetType, h: usize, effects: usize) -> Result<AssetId> {
    let effects = table_size(ty, effects);
    w.s.push(VIRTUAL);
    let name = w.xstr(h)?.unwrap_or_default();
    let mut cells = Vec::new();
    if let Some(table) = w.inline_array(h + 8, 8 * effects, 3)? {
        for i in 0..effects {
            #[cfg(test)]
            if matches!(w.s.raw_ptr_at(table + 8 * i)?, crate::Ptr::Other(_)) {
                eprintln!("FXTABLE {} entry {i} is not a pointer", ty.name());
            }
            cells.push(w.asset(AssetType::Fx, table + 8 * i)?);
        }
    }
    w.s.pop();
    // bo2mp: the impact table's cells, for the guest guns' own impacts.
    if ty == AssetType::ImpactFx {
        w.cap.impact_tables.push(super::FxTableRec {
            name: name.clone(),
            cells,
        });
    }
    Ok(w.name_only(ty, name))
}

/// `EquipmentSndTable` (no reference layout; measured on retail common_mp,
/// `soundaliases/equipment_snd.def`): an 88-byte header { name; three
/// counts; eight pointers }, then in VIRTUAL its name, a list of
/// `count0` names (`vestlight` ...), `count1` { name, float } (`small` 5.0
/// ...), then float tables (pointer arrays and `float[6][3]` blocks, all
/// whole multiples of 8 bytes) up to the next asset. Those tables are taken
/// as one run: it ends where the next table asset's header starts (found by
/// its shape: a name pointer that follows, its name string after the
/// header).
pub(super) fn equipment_snd_table(w: &mut Walker, h: usize) -> Result<AssetId> {
    // AW's table (88-byte header, name only filled in) is not IW6's: after
    // the name its data is taken as one run up to the next asset, found by
    // that asset's shape.
    use L::EquipmentSoundTable as E;
    w.s.push(VIRTUAL);
    let name = w.xstr(h)?.unwrap_or_default();
    // Retail common_mp's table is only its name: no counts, every pointer
    // null, and the next asset follows the string.
    let mut empty = true;
    for f in [
        E::clothTypes,
        E::weaponRattleTypes,
        E::chancesPLR,
        E::chancesNPC,
        E::mvmtClothSoundSets,
        E::mvmtRattleSoundSets,
        E::mvmtAccentSoundSets,
        E::mvmtMantleSoundSets,
    ] {
        empty &= w.s.u64_at(h + f)? == 0;
    }
    if empty {
        w.s.pop();
        return Ok(w.name_only(AssetType::EquipmentSndTable, name));
    }
    let start = w.s.pos();
    let end = find_next_asset(w, start)
        .or_else(|| find_next_techset(w, start))
        .ok_or(ReadError::Invalid("equipment sound table end", start))?;
    // (On retail common_mp the next asset follows the name directly, with
    // no VIRTUAL alignment between them.)
    if end > start {
        w.s.align(7);
        w.take(end - start)?;
    }
    w.s.pop();
    Ok(w.name_only(AssetType::EquipmentSndTable, name))
}

/// Where the table asset after the current one starts, at or after
/// `from`: the first offset from which the following run of raw files
/// (the assets after the equipment table in retail common_mp) parses as
/// raw file headers and lands on the next asset's name pointer.
fn find_next_asset(w: &Walker, from: usize) -> Option<usize> {
    const FOLLOW: [u8; 8] = [0xff, 0xff, 0xff, 0xff, 0xff, 0xfd, 0xfd, 0xfd];
    let data = w.s.data();
    let next = w.cap.walked + 1;
    let run = w.list.assets[next..]
        .iter()
        .take_while(|a| a.asset_type() == Some(AssetType::RawFile))
        .count();
    if run == 0 {
        return None;
    }
    let raw_file_at = |q: usize| -> Option<usize> {
        let h = data.get(q..q + 24)?;
        if h[0..8] != FOLLOW || !(h[16..24] == FOLLOW || h[16..24] == [0; 8]) {
            return None;
        }
        let compressed = u32::from_le_bytes(h[8..12].try_into().ok()?) as usize;
        let len = u32::from_le_bytes(h[12..16].try_into().ok()?) as usize;
        if len > 1 << 24 || compressed > len + 64 {
            return None;
        }
        let name = data
            .get(q + 24..)?
            .iter()
            .take(4096)
            .position(|&b| b == 0)?;
        let body = if h[16..24] == [0; 8] {
            0
        } else if compressed != 0 {
            compressed
        } else {
            len + 1
        };
        Some(q + 24 + name + 1 + body)
    };
    let limit = data.len().min(from + (1 << 20));
    (from..limit).find(|&p| {
        let mut q = p;
        for _ in 0..run {
            match raw_file_at(q) {
                Some(n) => q = n,
                None => return false,
            }
        }
        data.get(q..q + 8) == Some(&FOLLOW[..])
    })
}

pub(super) fn skeleton_script(w: &mut Walker, h: usize) -> Result<AssetId> {
    use L::SkeletonScript as S;
    w.s.push(VIRTUAL);
    let name = w.xstr(h + S::name)?.unwrap_or_default();
    let n = |w: &Walker, o: usize| w.s.u8_at(h + o).map(|v| v as usize);
    let solvers = n(w, S::ikData__numSolvers)?;
    let floats = n(w, S::ikData__numFloatParams)?;
    let ints = n(w, S::ikData__numIntParams)?;
    let strings = n(w, S::ikData__numStringParams)?;
    w.blob(h + S::ikData__solverIDs, solvers, 0)?;
    w.blob(h + S::ikData__floatParams, 4 * floats, 3)?;
    w.blob(h + S::ikData__intParams, 4 * ints, 3)?;
    w.blob(h + S::ikData__stringParams, 4 * strings, 3)?;
    // (x64-zt's writer leaves the program out.)
    let code = w.s.u16_at(h + S::codeSize)? as usize;
    w.blob(h + S::code, 4 * code, 3)?;
    w.s.pop();
    Ok(w.name_only(AssetType::SkeletonScript, name))
}

pub(super) fn laser(w: &mut Walker, h: usize) -> Result<AssetId> {
    use L::LaserDef as D;
    w.s.push(VIRTUAL);
    let name = w.xstr(h + D::name)?.unwrap_or_default();
    w.asset(AssetType::Material, h + D::laserMaterial)?;
    w.asset(AssetType::Material, h + D::laserLightMaterial)?;
    w.asset(AssetType::Fx, h + D::laserEndEffect)?;
    w.asset(AssetType::Laser, h + D::friendlyTeamLaser)?;
    w.s.pop();
    Ok(w.name_only(AssetType::Laser, name))
}

/// `VectorField` (H1's layout, read on retail common_mp): a 48-byte header
/// { name; fields; bounds; i32 count; }, then `count` 80-byte sub-fields
/// (aligned 16) each with its `vec4` grid (x*y*z entries, aligned 4).
pub(super) fn vector_field(w: &mut Walker, h: usize) -> Result<AssetId> {
    w.s.push(VIRTUAL);
    let name = w.xstr(h)?.unwrap_or_default();
    let n = w.s.i32_at(h + 40)?.max(0) as usize;
    if let Some(at) = w.inline_array(h + 8, 80 * n, 15)? {
        for i in 0..n {
            let f = at + 80 * i;
            let cells = (0..3).try_fold(1usize, |acc, k| {
                w.s.u32_at(f + 44 + 4 * k).map(|v| acc * v as usize)
            })?;
            w.blob(f, 16 * cells, 3)?;
        }
    }
    w.s.pop();
    Ok(w.name_only(AssetType::VectorField, name))
}

/// Where the next asset starts when it is a technique set: a 0x5B0-byte
/// header whose name pointer and 180 technique pointers are all pointers.
fn find_next_techset(w: &Walker, from: usize) -> Option<usize> {
    let next = w.cap.walked + 1;
    if w.list.assets.get(next)?.asset_type() != Some(AssetType::TechniqueSet) {
        return None;
    }
    let data = w.s.data();
    let ptr_ok = |q: usize| -> Option<bool> {
        let b = data.get(q..q + 8)?;
        Some(
            b == [0; 8] || (b[5..8] == [0xfd, 0xfd, 0xfd] && (b[4] == 0xfd || b[4] & 0xf0 == 0xf0)),
        )
    };
    let limit = data.len().min(from + (1 << 22));
    (from..limit).find(|&p| {
        let name = data.get(p..p + 8);
        name == Some(&[0xff, 0xff, 0xff, 0xff, 0xff, 0xfd, 0xfd, 0xfd][..])
            && data.get(p + 12..p + 16) == Some(&[0; 4][..])
            && (0..180).all(|i| ptr_ok(p + 16 + 8 * i) == Some(true))
            && (0..180).any(|i| data.get(p + 16 + 8 * i..p + 24 + 8 * i) != Some(&[0; 8][..]))
    })
}

/// `LeaderboardDef`: its name, then its columns (each a name and a stat
/// name), as x64-zt's H1 `leaderboard::write`.
pub(super) fn leaderboard(w: &mut Walker, h: usize) -> Result<AssetId> {
    use L::LeaderboardDef as D;
    w.s.push(VIRTUAL);
    let name = w.xstr(h + D::name)?.unwrap_or_else(String::new);
    let n = w.s.i32_at(h + D::columnCount)?.max(0) as usize;
    let c = L::LbColumnDef::SIZE;
    if let Some(at) = w.inline_array(h + D::columns, c * n, 3)? {
        for i in 0..n {
            w.xstr(at + c * i + L::LbColumnDef::name)?;
            w.xstr(at + c * i + L::LbColumnDef::statName)?;
        }
    }
    w.s.pop();
    Ok(w.name_only(AssetType::Leaderboard, name))
}

/// `VirtualLeaderboardDef`: its name, the leaderboard's name, then the
/// columns (24 bytes each: a name pointer and an id), each name following in
/// order. Measured on hmw_patch_code_post_gfx_mp (no reference layout).
pub(super) fn virtual_leaderboard(w: &mut Walker, h: usize) -> Result<AssetId> {
    use L::VirtualLeaderboardDef as D;
    w.s.push(VIRTUAL);
    let name = w.xstr(h + D::name)?.unwrap_or_else(String::new);
    w.xstr(h + D::leaderboardName)?;
    let n = w.s.i32_at(h + D::columnCount)?.max(0) as usize;
    if let Some(at) = w.inline_array(h + D::columns, 24 * n, 3)? {
        for i in 0..n {
            w.xstr(at + 24 * i)?;
        }
    }
    w.s.pop();
    Ok(w.name_only(AssetType::VirtualLeaderboard, name))
}

/// `DDLRoot`: its name, then the chain of `DDLDef` versions. Written in
/// the order of x64-zt's `ddl::write` (structs with their members and hash
/// tables, enums with their member names and hash table, then `next`);
/// checked by the walk running on through the rest of the zone.
pub(super) fn ddl(w: &mut Walker, h: usize) -> Result<AssetId> {
    use L::DDLRoot as R;
    w.s.push(VIRTUAL);
    let name = w.xstr(h + R::name)?.unwrap_or_else(String::new);
    let mut field = h + R::ddlDef;
    while let Some(def) = w.inline_array(field, L::DDLDef::SIZE, 7)? {
        use L::DDLDef as D;
        w.xstr(def + D::name)?;
        let n = w.s.i32_at(def + D::structCount)?.max(0) as usize;
        if let Some(at) = w.inline_array(def + D::structList, L::DDLStruct::SIZE * n, 7)? {
            for i in 0..n {
                ddl_struct(w, at + L::DDLStruct::SIZE * i)?;
            }
        }
        let n = w.s.i32_at(def + D::enumCount)?.max(0) as usize;
        if let Some(at) = w.inline_array(def + D::enumList, L::DDLEnum::SIZE * n, 7)? {
            for i in 0..n {
                let e = at + L::DDLEnum::SIZE * i;
                w.xstr(e + L::DDLEnum::name)?;
                let m = w.s.i32_at(e + L::DDLEnum::memberCount)?.max(0) as usize;
                if let Some(list) = w.inline_array(e + L::DDLEnum::members, 8 * m, 7)? {
                    for k in 0..m {
                        w.xstr(list + 8 * k)?;
                    }
                }
                let c = w.s.i32_at(e + L::DDLEnum::hashTable__count)?.max(0) as usize;
                w.inline_array(e + L::DDLEnum::hashTable__list, L::DDLHash::SIZE * c, 3)?;
            }
        }
        field = def + D::next;
    }
    w.s.pop();
    Ok(w.name_only(AssetType::Ddl, name))
}

fn ddl_struct(w: &mut Walker, s: usize) -> Result<()> {
    use L::DDLStruct as S;
    w.xstr(s + S::name)?;
    let m = w.s.i32_at(s + S::memberCount)?.max(0) as usize;
    if let Some(at) = w.inline_array(s + S::members, L::DDLMember::SIZE * m, 7)? {
        for k in 0..m {
            w.xstr(at + L::DDLMember::SIZE * k + L::DDLMember::name)?;
            w.xstr(at + L::DDLMember::SIZE * k + L::DDLMember::parent)?;
        }
    }
    let c = w.s.i32_at(s + S::hashTableUpper__count)?.max(0) as usize;
    w.inline_array(s + S::hashTableUpper__list, L::DDLHash::SIZE * c, 3)?;
    let c = w.s.i32_at(s + S::hashTableLower__count)?.max(0) as usize;
    w.inline_array(s + S::hashTableLower__list, L::DDLHash::SIZE * c, 3)?;
    Ok(())
}

/// `StructuredDataDefSet`: its name, then each version's enums (with their
/// entries, no pointers inside), structs (with their properties), indexed
/// and enumed arrays, in field order. Measured on hmw_patch_code_post_gfx_mp.
pub(super) fn structured_data_def(w: &mut Walker, h: usize) -> Result<AssetId> {
    use L::StructuredDataDefSet as Set;
    w.s.push(VIRTUAL);
    let name = w.xstr(h + Set::name)?.unwrap_or_else(String::new);
    let n = w.s.i32_at(h + Set::defCount)?.max(0) as usize;
    let d = L::StructuredDataDef::SIZE;
    if let Some(defs) = w.inline_array(h + Set::defs, d * n, 7)? {
        for i in 0..n {
            use L::StructuredDataDef as D;
            let def = defs + d * i;
            let c = w.s.i32_at(def + D::enumCount)?.max(0) as usize;
            if let Some(at) = w.inline_array(def + D::enums, L::StructuredDataEnum::SIZE * c, 7)? {
                for k in 0..c {
                    use L::StructuredDataEnum as E;
                    let e = at + L::StructuredDataEnum::SIZE * k;
                    let m = w.s.i32_at(e + E::entryCount)?.max(0) as usize;
                    // The entries hold a string offset and an index, no pointers.
                    w.inline_array(e + E::entries, L::StructuredDataEnumEntry::SIZE * m, 3)?;
                }
            }
            let c = w.s.i32_at(def + D::structCount)?.max(0) as usize;
            if let Some(at) = w.inline_array(def + D::structs, L::StructuredDataStruct::SIZE * c, 7)? {
                for k in 0..c {
                    use L::StructuredDataStruct as S;
                    let st = at + L::StructuredDataStruct::SIZE * k;
                    let m = w.s.i32_at(st + S::propertyCount)?.max(0) as usize;
                    let ps = L::StructuredDataStructProperty::SIZE;
                    w.inline_array(st + S::properties, ps * m, 3)?;
                }
            }
            let c = w.s.i32_at(def + D::indexedArrayCount)?.max(0) as usize;
            w.inline_array(def + D::indexedArrays, L::StructuredDataIndexedArray::SIZE * c, 3)?;
            let c = w.s.i32_at(def + D::enumedArrayCount)?.max(0) as usize;
            w.inline_array(def + D::enumedArrays, L::StructuredDataEnumedArray::SIZE * c, 3)?;
        }
    }
    w.s.pop();
    Ok(w.name_only(AssetType::StructuredDataDef, name))
}

/// `TTFDef` (a font file): its name, then `fileLen + 1` bytes.
pub(super) fn ttf(w: &mut Walker, h: usize) -> Result<AssetId> {
    use L::TTFDef as D;
    w.s.push(VIRTUAL);
    let name = w.xstr(h + D::name)?.unwrap_or_else(String::new);
    let n = w.s.u32_at(h + D::fileLen)? as usize;
    w.blob(h + D::file, n + 1, 0)?;
    w.s.pop();
    Ok(w.name_only(AssetType::Ttf, name))
}

/// `ScriptableDef` (x64-zt h1 `scriptabledef.cpp`): name, base model,
/// collision brush names, parts (each part's states, each state's
/// on-enter events) and notetracks. Event data by type: models and effects
/// are asset references, a sound is its alias name following, an
/// animation / light / shake / translate / rotate a string.
pub(super) fn scriptable_def(w: &mut Walker, h: usize) -> Result<AssetId> {
    use crate::scriptable as S;
    use L::ScriptableDef as D;
    use L::ScriptableEventDef as E;
    w.s.push(VIRTUAL);
    let name = w.xstr(h + D::name)?.unwrap_or_default();
    let base_model = w.asset(AssetType::XModel, h + D::baseModel)?;
    let base_collision_brush = w.xstr(h + D::baseCollisionBrush)?;
    let destroyed_collision_brush = w.xstr(h + D::destroyedCollisionBrush)?;
    let mut rec = S::Def {
        name: name.clone(),
        base_model: base_model.map(|m| w.cap.name_of(m).trim_start_matches(',').to_owned()),
        base_collision_brush,
        destroyed_collision_brush,
        kind: w.s.i32_at(h + D::r#type)?,
        flags: w.s.u8_at(h + D::flags)?,
        event_stream_size: w.s.u8_at(h + D::eventStreamSize)?,
        event_constants_size: w.s.u8_at(h + D::eventConstantsSize)?,
        ..Default::default()
    };
    let parts = w.s.u8_at(h + D::partCount)? as usize;
    let ps = L::ScriptablePartDef::SIZE;
    if let Some(pa) = w.inline_array(h + D::parts, ps * parts, 3)? {
        for i in 0..parts {
            let part = pa + ps * i;
            let states = w.s.u8_at(part + L::ScriptablePartDef::stateCount)? as usize;
            let mut prec = S::Part {
                name: w.s.u32_at(part + L::ScriptablePartDef::name)?,
                flags: w.s.u8_at(part + L::ScriptablePartDef::flags)?,
                states: Vec::new(),
            };
            let ss = L::ScriptableStateDef::SIZE;
            let Some(sa) = w.inline_array(part + L::ScriptablePartDef::states, ss * states, 3)?
            else {
                rec.parts.push(prec);
                continue;
            };
            for j in 0..states {
                let state = sa + ss * j;
                use L::ScriptableStateDef as T;
                let events = w.s.u8_at(state + T::onEnterEventCount)? as usize;
                let mut srec = S::State {
                    name: w.s.u32_at(state + T::name)?,
                    tag: w.s.u32_at(state + T::tagName)?,
                    damage_flags: w.s.u8_at(state + T::damageFlags)?,
                    parent_transfer_rate: w.s.u8_at(state + T::damageParentTransferRate)?,
                    parent_receive_rate: w.s.u8_at(state + T::damageParentReceiveRate)?,
                    max_health: w.s.u32_at(state + T::maxHealth)?,
                    events: Vec::new(),
                };
                let es = E::SIZE;
                let Some(ea) = w.inline_array(state + T::onEnterEvents, es * events, 3)? else {
                    prec.states.push(srec);
                    continue;
                };
                for k in 0..events {
                    let ev = ea + es * k;
                    let ty = w.s.i32_at(ev + E::r#type)?;
                    let named = |w: &Walker, id: Option<AssetId>| {
                        id.map(|m| w.cap.name_of(m).trim_start_matches(',').to_owned())
                    };
                    let i16_at = |w: &Walker, o: usize| -> Result<i16> { Ok(w.s.u16_at(ev + o)? as i16) };
                    let event = match ty {
                        0 => {
                            let m = w.asset(AssetType::XModel, ev + E::data__setModel__model)?;
                            S::Event::SetModel { model: named(w, m) }
                        }
                        1 => {
                            let f = w.asset(AssetType::Fx, ev + E::data__playFx__handle)?;
                            S::Event::PlayFx {
                                fx: named(w, f),
                                tag: w.s.u32_at(ev + E::data__playFx__tagName)?,
                                loop_time: w.s.u16_at(ev + E::data__playFx__loopTime)?,
                                use_angles: w.s.u8_at(ev + E::data__playFx__tagUseAngles)? != 0,
                            }
                        }
                        2 => {
                            let f = w.asset(AssetType::Fx, ev + E::data__stopFx__handle)?;
                            S::Event::StopFx {
                                fx: named(w, f),
                                tag: w.s.u32_at(ev + E::data__stopFx__tagName)?,
                            }
                        }
                        3 => {
                            let alias = sound_custom(w, ev + E::data__playSound__alias)?;
                            S::Event::PlaySound {
                                alias,
                                looping: w.s.u8_at(ev + E::data__playSound__looping)? != 0,
                            }
                        }
                        4 => {
                            let n = w.xstr(ev + E::data__playAnim__animName)?;
                            S::Event::PlayAnim {
                                name: n,
                                stateful: w.s.u8_at(ev + E::data__playAnim__stateful)? != 0,
                                rate_min: w.s.u16_at(ev + E::data__playAnim__playbackRateMin)?,
                                rate_max: w.s.u16_at(ev + E::data__playAnim__playbackRateMax)?,
                            }
                        }
                        5 => S::Event::Explosion {
                            force_min: w.s.u16_at(ev + E::data__doExplosion__forceMin)?,
                            force_max: w.s.u16_at(ev + E::data__doExplosion__forceMax)?,
                            radius: w.s.u16_at(ev + E::data__doExplosion__radius)?,
                            damage_min: w.s.u16_at(ev + E::data__doExplosion__damageMin)?,
                            damage_max: w.s.u16_at(ev + E::data__doExplosion__damageMax)?,
                            ai_avoid: w.s.u8_at(ev + E::data__doExplosion__aiAvoid)? != 0,
                        },
                        6 => S::Event::HealthDrain {
                            amount: i16_at(w, E::data__healthDrain__amount)?,
                            interval: w.s.u16_at(ev + E::data__healthDrain__interval)?,
                            bad_place_radius: w.s.u16_at(ev + E::data__healthDrain__badPlaceRadius)?,
                        },
                        7 => {
                            let m = w.asset(AssetType::XModel, ev + E::data__physicsLaunch__model)?;
                            S::Event::PhysicsLaunch {
                                model: named(w, m),
                                dir: [
                                    w.s.u8_at(ev + E::data__physicsLaunch__launchDirX)? as i8,
                                    w.s.u8_at(ev + E::data__physicsLaunch__launchDirY)? as i8,
                                    w.s.u8_at(ev + E::data__physicsLaunch__launchDirZ)? as i8,
                                ],
                                expl_force_scale: w.s.u16_at(ev + E::data__physicsLaunch__explForceScale)?,
                                bullet_force_scale: w.s.u16_at(ev + E::data__physicsLaunch__bulletForceScale)?,
                            }
                        }
                        8 => S::Event::Light {
                            noteworthy: w.xstr(ev + E::data__lightSettings__noteworthy)?,
                        },
                        9 => S::Event::Sunlight,
                        10 => {
                            let r = w.xstr(ev + E::data__shake__rumbleName)?;
                            S::Event::Shake {
                                rumble: r,
                                duration: w.s.u16_at(ev + E::data__shake__duration)?,
                                radius: w.s.u16_at(ev + E::data__shake__radius)?,
                            }
                        }
                        11 => {
                            w.xstr(ev + E::data__translate__noteworthy)?;
                            S::Event::Translate {
                                min: [
                                    i16_at(w, E::data__translate__translationMinX)?,
                                    i16_at(w, E::data__translate__translationMinY)?,
                                    i16_at(w, E::data__translate__translationMinZ)?,
                                ],
                                max: [
                                    i16_at(w, E::data__translate__translationMaxX)?,
                                    i16_at(w, E::data__translate__translationMaxY)?,
                                    i16_at(w, E::data__translate__translationMaxZ)?,
                                ],
                                time_min: w.s.u16_at(ev + E::data__translate__transitionTimeMin)?,
                                time_max: w.s.u16_at(ev + E::data__translate__transitionTimeMax)?,
                                flags: w.s.u16_at(ev + E::data__translate__flags)?,
                            }
                        }
                        12 => {
                            w.xstr(ev + E::data__rotate__noteworthy)?;
                            S::Event::Rotate {
                                min: [
                                    i16_at(w, E::data__rotate__rotationMinPitch)?,
                                    i16_at(w, E::data__rotate__rotationMinYaw)?,
                                    i16_at(w, E::data__rotate__rotationMinRoll)?,
                                ],
                                max: [
                                    i16_at(w, E::data__rotate__rotationMaxPitch)?,
                                    i16_at(w, E::data__rotate__rotationMaxYaw)?,
                                    i16_at(w, E::data__rotate__rotationMaxRoll)?,
                                ],
                                time_min: w.s.u16_at(ev + E::data__rotate__transitionTimeMin)?,
                                time_max: w.s.u16_at(ev + E::data__rotate__transitionTimeMax)?,
                                flags: w.s.u16_at(ev + E::data__rotate__flags)?,
                            }
                        }
                        13 => S::Event::StateChange {
                            target: w.s.u8_at(ev + E::data__stateChange__targetIndex)?,
                            delay_min: w.s.u16_at(ev + E::data__stateChange__delayMin)?,
                            delay_max: w.s.u16_at(ev + E::data__stateChange__delayMax)?,
                        },
                        other => S::Event::Other(other),
                    };
                    srec.events.push(event);
                }
                prec.states.push(srec);
            }
            rec.parts.push(prec);
        }
    }
    let notes = w.s.u8_at(h + D::notetrackCount)? as usize;
    let ns = L::ScriptableNotetrackDef::SIZE;
    if let Some(na) = w.inline_array(h + D::notetracks, ns * notes, 3)? {
        use L::ScriptableNotetrackDef as N;
        for i in 0..notes {
            let nt = na + ns * i;
            let ty = w.s.i32_at(nt + N::r#type)?;
            rec.notetracks.push((w.s.u32_at(nt + N::name)?, ty));
            if ty == 1 {
                sound_custom(w, nt + N::data__playSound__alias)?;
            } else {
                w.asset(AssetType::Fx, nt + N::data__playFx__handle)?;
            }
        }
    }
    w.cap.scriptable_defs.push(rec);
    w.s.pop();
    Ok(w.name_only(AssetType::Scriptable, name))
}
