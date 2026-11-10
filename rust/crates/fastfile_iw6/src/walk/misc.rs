//! Small assets: names, raw files, string tables, physics, tracers,
//! vehicles.

use super::{AssetId, Walker};
use crate::asset_type::AssetType;
use crate::cursor::{ReadError, Result};
use crate::layout as L;
use crate::stream::{SCRIPT, VIRTUAL};
use crate::walk::{LightDefRec, OtherRec, RawFileRec, Span, ToneMappingRec};

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

/// bo2mp: a sound curve (`SndCurve`, 152 bytes: isDefault, name at 8,
/// knot count at 16, up to 16 (x, y) knots at 20); its knots are kept by
/// its index in `others`.
pub(super) fn snd_curve(w: &mut Walker, ty: AssetType, h: usize) -> Result<AssetId> {
    w.s.push(VIRTUAL);
    let name = w.xstr(h + 8)?.unwrap_or_default();
    w.s.pop();
    let mut rec = vec![0u8; crate::sound::SND_CURVE_LEN];
    for (k, b) in rec.iter_mut().enumerate() {
        *b = w.s.u8_at(h + k)?;
    }
    let knots = crate::sound::curve_knots(&rec);
    let id = w.name_only(ty, name.clone());
    // (A curve another zone holds loads as its name alone, `,`-prefixed:
    // no knots here.)
    w.cap.snd_curves.insert(id.index, crate::sound::SndCurve { name, knots: knots.unwrap_or_default() });
    Ok(id)
}

/// bo2mp: a `DopplerPreset` (name, speed of sound, player velocity scale,
/// min pitch, max pitch, smoothing), kept by its index in `others`.
pub(super) fn doppler_preset(w: &mut Walker, h: usize) -> Result<AssetId> {
    use L::DopplerPreset as D;
    w.s.push(VIRTUAL);
    let name = w.xstr(h + D::name)?.unwrap_or_default();
    w.s.pop();
    let preset = crate::sound::DopplerPreset {
        name: name.clone(),
        speed_of_sound: w.s.f32_at(h + D::speedOfSound)?,
        player_velocity_scale: w.s.f32_at(h + D::playerVelocityScale)?,
        min_pitch: w.s.f32_at(h + D::minPitch)?,
        max_pitch: w.s.f32_at(h + D::maxPitch)?,
        smoothing: w.s.f32_at(h + D::smoothing)?,
    };
    let id = w.name_only(AssetType::DopplerPreset, name);
    w.cap.doppler_presets.insert(id.index, preset);
    Ok(id)
}

/// A `ToneMapping` asset: Ghosts' filmic curve (Hable's: shoulder `A`,
/// linear strength `B`, linear angle `C`, toe strength `D`, toe angle
/// numerator `E` and denominator `F`, white point, exposure, adaptation
/// rates). The header is 8 groups of three floats and the rates (`layout`
/// `ToneMapping`); the values are kept under the asset's name.
pub(super) fn tone_mapping(w: &mut Walker, h: usize) -> Result<AssetId> {
    use L::ToneMapping as T;
    w.s.push(VIRTUAL);
    let name = w.xstr(h + T::name)?.unwrap_or_default();
    let mut v = [0f32; 26];
    for (k, slot) in v.iter_mut().enumerate().take(26) {
        *slot = w.s.f32_at(h + 4 * k).unwrap_or(f32::NAN);
    }
    w.s.pop();
    w.cap.tone_mappings.insert(name.to_ascii_lowercase(), ToneMappingRec { name: name.clone(), v });
    Ok(w.name_only(AssetType::ToneMapping, name))
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

/// bo2mp: the AI path data of a map (`<map>_path.ff`): the nodes (176 bytes each, unaligned in
/// the file), each node's links, the visibility bit matrix, the nearest-node tree (a flat array
/// whose children are references into itself; the leaves' node lists follow in array order), then
/// the exposure, no-peek, zone and dynamic-state blobs. READ from the data: the sizes add up to
/// the next asset exactly on mp_frag and mp_snow.
/// A flat `pathnode_tree_t` array (`count` entries behind the pointer at `field`) whose children
/// are references into itself, then the node lists of its leaves in array order.
fn path_tree(w: &mut Walker, field: usize, count: usize) -> Result<Vec<crate::walk::PathTreeRec>> {
    use crate::ptr::Ptr;
    use crate::walk::PathTreeRec;
    const TREE: usize = L::pathnode_tree_t::SIZE;
    let mut out = Vec::new();
    if !matches!(w.s.ptr_at(field)?, Ptr::Follow | Ptr::Insert) {
        return Ok(out);
    }
    w.s.align(7);
    let base = w.s.offset(VIRTUAL);
    let Some(at) = w.inline_array(field, TREE * count, 7)? else { return Ok(out) };
    let child = |w: &Walker, f: usize| -> Result<Option<u32>> {
        Ok(match w.s.ptr_at(f)? {
            Ptr::Ref { block: VIRTUAL, offset } if (offset as u64) >= base => Some(((offset as u64 - base) / TREE as u64) as u32),
            _ => None,
        })
    };
    for i in 0..count {
        let e = at + i * TREE;
        let axis = w.s.i32_at(e + L::pathnode_tree_t::axis)?;
        let dist = w.s.f32_at(e + L::pathnode_tree_t::dist)?;
        let children = if axis >= 0 { [child(w, e + L::pathnode_tree_t::u__child)?, child(w, e + L::pathnode_tree_t::u__child + 8)?] } else { [None, None] };
        out.push(PathTreeRec { axis, dist, children, leaf: Vec::new() });
    }
    for i in 0..count {
        let e = at + i * TREE;
        if out[i].axis >= 0 {
            continue;
        }
        let n = w.s.u32_at(e + L::pathnode_tree_t::u__s__nodeCount)? as usize;
        if let Some(pos) = w.inline_array(e + L::pathnode_tree_t::u__s__nodes, n * 2, 1)? {
            for k in 0..n {
                out[i].leaf.push(u16::from_le_bytes([w.s.data()[pos + k * 2], w.s.data()[pos + k * 2 + 1]]));
            }
        }
    }
    Ok(out)
}

pub(super) fn path_data(w: &mut Walker, h: usize) -> Result<AssetId> {
    use L::PathData as P;
    use crate::walk::PathDataRec;
    const NODE: usize = L::pathnode_t::SIZE;
    const LINK: usize = L::pathlink_s::SIZE;
    w.s.push(VIRTUAL);
    let name = w.xstr(h + P::name)?.unwrap_or_default();
    let n = w.s.u32_at(h + P::nodeCount)? as usize;
    if n > 0x2000 {
        w.s.pop();
        return Err(ReadError::Invalid("path data: more than 8192 nodes", h));
    }
    let mut rec = PathDataRec { name: name.clone(), version: w.s.u16_at(h + P::version)?, zone_count: w.s.u32_at(h + P::zoneCount)?, ..Default::default() };
    rec.dyn_group_count = w.s.u32_at(h + P::dynamicNodeGroupCount)?;
    let nodes = w.inline_array(h + P::nodes, NODE * n, 7)?;
    if let Some(at) = nodes {
        for i in 0..n {
            rec.nodes.push(Span { pos: at + i * NODE, len: NODE });
        }
        for i in 0..n {
            let field = at + i * NODE;
            let count = w.s.u16_at(field + L::pathnode_t::constant__totalLinkCount)? as usize;
            let links = w.inline_array(field + L::pathnode_t::constant__Links, LINK * count, 3)?.map(|pos| Span { pos, len: LINK * count });
            rec.links.push((links, count));
        }
    }
    let vis_bytes = w.s.u32_at(h + P::visBytes)? as usize;
    rec.vis = w.blob(h + P::pathVis, vis_bytes, 0)?.map(|pos| Span { pos, len: vis_bytes });
    let tree_count = w.s.u32_at(h + P::nodeTreeCount)? as usize;
    rec.tree = path_tree(w, h + P::nodeTree, tree_count)?;
    if rec.dyn_group_count != 0 {
        // PathDynamicNodeGroup (16 bytes): parentIndex, nodeTreeCount, nodeTree; each group's
        // tree follows the array of groups in order.
        let groups = rec.dyn_group_count as usize;
        if let Some(at) = w.inline_array(h + P::dynamicNodeGroups, L::PathDynamicNodeGroup::SIZE * groups, 7)? {
            for g in 0..groups {
                let e = at + g * L::PathDynamicNodeGroup::SIZE;
                let parent = w.s.u32_at(e + L::PathDynamicNodeGroup::parentIndex)?;
                let count = w.s.u32_at(e + L::PathDynamicNodeGroup::nodeTreeCount)? as usize;
                let tree = path_tree(w, e + L::PathDynamicNodeGroup::nodeTree, count)?;
                rec.dyn_groups.push((parent, tree));
            }
        }
    }
    let blob = |w: &mut Walker, field: usize, len: usize| -> Result<Option<Span>> { Ok(w.blob(field, len, 0)?.map(|pos| Span { pos, len })) };
    let exposure = w.s.u32_at(h + P::exposureBytes)? as usize;
    rec.exposure = blob(w, h + P::pathExposure, exposure)?;
    let no_peek = w.s.u32_at(h + P::noPeekVisBytes)? as usize;
    rec.no_peek_vis = blob(w, h + P::pathNoPeekVis, no_peek)?;
    let zones = w.s.u32_at(h + P::zonesBytes)? as usize;
    rec.zones = blob(w, h + P::pathZones, zones)?;
    let dyn_states = w.s.u32_at(h + P::dynStatesBytes)? as usize;
    rec.dyn_states = blob(w, h + P::pathDynStates, dyn_states)?;
    w.s.pop();
    w.cap.path_data.push(rec);
    Ok(w.name_only(AssetType::PathData, name))
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
    let rows = w.s.u32_at(h + S::rowCount)? as usize;
    let n = columns * rows;
    let mut cells = Vec::new();
    if let Some(values) = w.inline_array(h + S::values, L::StringTableCell::SIZE * n, 3)? {
        cells.reserve(n);
        for i in 0..n {
            let field = values + L::StringTableCell::SIZE * i + L::StringTableCell::string;
            // (A shared cell names an earlier string; its text is that one's.)
            let shared = match w.s.ptr_at(field)? {
                crate::ptr::Ptr::Ref { block: VIRTUAL, offset } => Some(w.string_at(offset as u64)),
                _ => None,
            };
            let text = w.xstr(field)?;
            cells.push(match shared {
                Some(s) => s.unwrap_or_default(),
                None => text.unwrap_or_default(),
            });
        }
    }
    w.s.pop();
    w.cap.string_tables.push(crate::walk::StringTableRec { name: name.clone(), columns, rows, cells });
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
    let value = w.xstr(h + E::value)?.unwrap_or_default();
    let name = w.xstr(h + E::name)?.unwrap_or_default();
    w.s.pop();
    w.cap.localized.push((name.clone(), value));
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

/// A `BrushWrapper` (physics collision brush).
fn brush_wrapper(w: &mut Walker, field: usize) -> Result<()> {
    use L::BrushWrapper as B;
    let Some(b) = w.inline_array(field, B::SIZE, 3)? else {
        return Ok(());
    };
    let numsides = w.s.u16_at(b + B::brush__numsides)? as usize;
    if let Some(sides) = w.inline_array(b + B::brush__sides, L::cbrushside_t::SIZE * numsides, 0)? {
        for i in 0..numsides {
            let plane = sides + L::cbrushside_t::SIZE * i + L::cbrushside_t::plane;
            w.blob(plane, L::cplane_s::SIZE, 3)?;
        }
    }
    let edges = w.s.u32_at(b + B::totalEdgeCount)? as usize;
    w.blob(b + B::brush__baseAdjacentSide, edges, 0)?;
    w.blob(b + B::planes, L::cplane_s::SIZE * numsides, 3)?;
    Ok(())
}

pub(super) fn phys_collmap(w: &mut Walker, h: usize) -> Result<AssetId> {
    use L::PhysCollmap as P;
    w.s.push(VIRTUAL);
    let name = w.xstr(h + P::name)?.unwrap_or_default();
    let count = w.s.u32_at(h + P::count)? as usize;
    if let Some(geoms) = w.inline_array(h + P::geoms, L::PhysGeomInfo::SIZE * count, 3)? {
        for i in 0..count {
            brush_wrapper(
                w,
                geoms + L::PhysGeomInfo::SIZE * i + L::PhysGeomInfo::brushWrapper,
            )?;
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
    let f = |w: &Walker, o: usize| w.s.f32_at(h + o).unwrap_or(0.0);
    let rec = crate::walk::TracerRec {
        name: name.clone(),
        material: material.map(|id| w.cap.name_of(id).to_owned()).filter(|n| !n.is_empty()),
        draw_interval: w.s.u32_at(h + T::drawInterval).unwrap_or(0) as i32,
        speed: f(w, T::speed),
        beam_length: f(w, T::beamLength),
        beam_width: f(w, T::beamWidth),
        screw_radius: f(w, T::screwRadius),
        screw_dist: f(w, T::screwDist),
        colors: std::array::from_fn(|c| std::array::from_fn(|k| f(w, T::colors + 16 * c + 4 * k))),
    };
    w.s.pop();
    w.cap.tracers.insert(name.to_ascii_lowercase(), rec);
    Ok(w.name_only(AssetType::Tracer, name))
}

/// bo2mp: a `Colorization`'s values (power and desaturation, scale, bias).
pub(super) fn colorization(w: &mut Walker, h: usize) -> Result<AssetId> {
    use L::Colorization as C;
    w.s.push(VIRTUAL);
    let name = w.xstr(h + C::name)?.unwrap_or_default();
    let f = |w: &Walker, o: usize| w.s.f32_at(h + o).unwrap_or(f32::NAN);
    let rec = crate::walk::ColorizationRec {
        name: name.clone(),
        power_and_desaturation: std::array::from_fn(|k| f(w, C::powerAndDesaturation + 4 * k)),
        scale: std::array::from_fn(|k| f(w, C::scale + 4 * k)),
        bias: std::array::from_fn(|k| f(w, C::bias + 4 * k)),
    };
    w.s.pop();
    w.cap.colorizations.insert(name.to_ascii_lowercase(), rec);
    Ok(w.name_only(AssetType::Colorization, name))
}

pub(super) fn colorization_set(w: &mut Walker, h: usize) -> Result<AssetId> {
    use L::ColorizationSet as C;
    w.s.push(VIRTUAL);
    let name = w.xstr(h + C::name)?.unwrap_or_default();
    let f = |w: &Walker, o: usize| w.s.f32_at(h + o).unwrap_or(f32::NAN);
    let mut rec = crate::walk::ColorizationSetRec {
        name: name.clone(),
        low_and_high_end_points: std::array::from_fn(|k| f(w, C::lowAndHighEndPoints + 4 * k)),
        mid_end_points: std::array::from_fn(|k| f(w, C::midEndPoints + 4 * k)),
        alternate_depth_scale_and_bias: std::array::from_fn(|k| f(w, C::alternateDepthScaleAndBias + 4 * k)),
        flags: w.s.u32_at(h + C::flags).unwrap_or(0),
        ..Default::default()
    };
    for (k, fld) in [
        C::colorization,
        C::colorizationAlternate,
        C::colorizationLow,
        C::colorizationMid,
        C::colorizationHigh,
    ]
    .into_iter()
    .enumerate()
    {
        let id = w.asset(AssetType::Colorization, h + fld)?;
        rec.colorizations[k] = id.map(|id| w.cap.name_of(id).trim_start_matches(',').to_ascii_lowercase());
    }
    w.asset(AssetType::Material, h + C::alternateMaskMaterial)?;
    w.s.pop();
    w.cap.colorization_sets.insert(name.to_ascii_lowercase(), rec);
    Ok(w.name_only(AssetType::ColorizationSet, name))
}

pub(super) fn light_def(w: &mut Walker, h: usize) -> Result<AssetId> {
    use L::GfxLightDef as D;
    w.s.push(VIRTUAL);
    let name = w.xstr(h + D::name)?.unwrap_or_default();
    let att = w.asset(AssetType::Image, h + D::attenuation__image)?;
    let cuc = w.asset(AssetType::Image, h + D::cucoloris__image)?;
    let lmap_lookup_start = w.s.i32_at(h + D::lmapLookupStart)?;
    w.s.pop();
    let nm = |id: Option<AssetId>| id.map(|id| w.cap.name_of(id).trim_start_matches(',').to_string());
    let rec = LightDefRec {
        name: name.clone(),
        attenuation: nm(att),
        cucoloris: nm(cuc),
        lmap_lookup_start,
    };
    w.cap.light_defs.insert(name.trim_start_matches(',').to_ascii_lowercase(), rec);
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
    w.xstr(h + V::vehPhysDef__accelGraphName)?;
    w.asset(AssetType::Fx, h + V::vehHelicopterGroundFx)?;
    w.asset(AssetType::Fx, h + V::vehHelicopterGroundWaterFx)?;
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
pub(super) const IMPACT_FX_EFFECTS: usize = 15 * 38;

/// `SurfaceFxTable` { name; SurfaceFxEntry* table }: 6 entries of 31
/// effects (186 pointers in retail common_mp).
pub(super) const SURFACE_FX_EFFECTS: usize = 6 * 31;

/// An effect table (`ImpactFx`, `SurfaceFx`): name, then a pointer to
/// `effects` effect pointers, aligned to 4 (retail mp_snow's and
/// mp_skeleton's own impact tables, named "", sit 4 bytes past an 8-byte
/// boundary: their later references name them so).
pub(super) fn fx_table(w: &mut Walker, ty: AssetType, h: usize, effects: usize) -> Result<AssetId> {
    w.s.push(VIRTUAL);
    let name = w.xstr(h)?.unwrap_or_default();
    let trace = std::env::var_os("IW6_TRACE_FXTABLE").is_some();
    let mut cells: Vec<Option<String>> = Vec::new();
    if let Some(table) = w.inline_array(h + 8, 8 * effects, 3)? {
        for i in 0..effects {
            let p = w.s.ptr_at(table + 8 * i)?;
            let before = w.s.pos();
            let id = w.asset(AssetType::Fx, table + 8 * i)?;
            cells.push(id.map(|id| w.cap.name_of(id).to_owned()).filter(|n| !n.is_empty()));
            if trace && p != crate::Ptr::Null {
                eprintln!("FXTABLE {name:?} {i} ({}, {}) field virt {:?} {p:?} -> {:?} pos {before:#x}->{:#x} unresolved {}", i / 38, i % 38, w.vmap.virt_of(table + 8 * i), id.map(|id| w.cap.name_of(id).to_owned()), w.s.pos(), w.unresolved.get());
            }
        }
    }
    w.s.pop();
    if ty == AssetType::ImpactFx && !cells.is_empty() {
        w.cap.impact_tables.push(crate::walk::ImpactTableRec { name: name.clone(), cells });
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
    w.s.push(VIRTUAL);
    let name = w.xstr(h)?.unwrap_or_default();
    let names = w.s.u32_at(h + 8)? as usize;
    let rattles = w.s.u32_at(h + 12)? as usize;
    if let Some(list) = w.inline_array(h + 24, 8 * names, 7)? {
        for i in 0..names {
            w.xstr(list + 8 * i)?;
        }
    }
    if let Some(list) = w.inline_array(h + 32, 16 * rattles, 7)? {
        for i in 0..rattles {
            w.xstr(list + 16 * i)?;
        }
    }
    let start = w.s.pos();
    let end =
        find_next_asset(w, start).ok_or(ReadError::Invalid("equipment sound table end", start))?;
    w.s.align(7);
    w.take(end - start)?;
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

/// `Font_s` (x64-zt `fontdef.cpp`): name, materials, glyphs (aligned 4).
pub(super) fn font(w: &mut Walker, h: usize) -> Result<AssetId> {
    use L::Font_s as F;
    w.s.push(VIRTUAL);
    let name = w.xstr(h + F::fontName)?.unwrap_or_default();
    w.asset(AssetType::Material, h + F::material)?;
    w.asset(AssetType::Material, h + F::glowMaterial)?;
    let n = w.s.i32_at(h + F::glyphCount)?.max(0) as usize;
    w.blob(h + F::glyphs, L::Glyph::SIZE * n, 3)?;
    w.s.pop();
    Ok(w.name_only(AssetType::Font, name))
}
