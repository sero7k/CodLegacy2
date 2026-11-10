//! Map assets: path nodes, ComWorld, FxWorld, GfxWorld, MapEnts,
//! GlassWorld, PhysWorld and clipMap_t, each in the order x64-zt's H1
//! writers lay their data out (`zonetool/h1/assets/*.cpp`,
//! `zonetool/shared/assets/clipmap.hpp`).
//!
//! Started from Ghosts' walkers (`fastfile_iw6::walk::world`); what H1
//! changed is noted where it is read. RUNTIME arrays take no file bytes
//! (they only move that block), so they are listed for the record but
//! cannot drift the walk.

use super::{AssetId, Walker};
use crate::asset_type::AssetType;
use crate::cursor::{ReadError, Result};
use crate::layout as L;
use crate::ptr::Ptr;
use crate::stream::{RUNTIME, TEMP, VIRTUAL};
use crate::walk::{
    ClipInfoRec, ClipMapRec, ComWorldRec, FxGlassDefRec, FxGlassRec, GfxWorldRec, GlassWorldRec, MapEntsRec, Span,
};

fn span(pos: Option<usize>, len: usize) -> Option<Span> {
    pos.map(|pos| Span { pos, len })
}

// ---- PathData ------------------------------------------------------------

/// A path node tree's children (`pathnode_tree_t`, 24 bytes): a leaf
/// (`axis < 0`) holds its node indices, an inner node its two children,
/// each a tree that follows (aligned 4) with its own contents after it.
fn node_tree_contents(w: &mut Walker, t: usize, depth: usize) -> Result<()> {
    use L::pathnode_tree_t as T;
    if depth > 64 {
        return Err(ReadError::Invalid("path node tree depth", t));
    }
    if w.s.i32_at(t + T::axis)? < 0 {
        let n = w.s.i32_at(t + T::u__s__nodeCount)?.max(0) as usize;
        w.blob(t + T::u__s__nodes, 2 * n, 1)?;
    } else {
        for k in 0..2 {
            if let Some(child) = w.inline_array(t + T::u__child + 8 * k, T::SIZE, 3)? {
                node_tree_contents(w, child, depth + 1)?;
            }
        }
    }
    Ok(())
}

fn node_trees(w: &mut Walker, field: usize, count: usize) -> Result<()> {
    let ts = L::pathnode_tree_t::SIZE;
    if let Some(at) = w.inline_array(field, ts * count, 3)? {
        for i in 0..count {
            node_tree_contents(w, at + ts * i, 0)?;
        }
    }
    Ok(())
}

/// `PathData` (AI path nodes; x64-zt's `path_data::write`).
pub(super) fn path_data(w: &mut Walker, h: usize) -> Result<AssetId> {
    use L::PathData as P;
    w.s.push(VIRTUAL);
    let name = w.xstr(h + P::name)?.unwrap_or_else(String::new);
    let n = w.s.u32_at(h + P::nodeCount)? as usize;
    let ns = L::pathnode_t::SIZE;
    if let Some(at) = w.inline_array(h + P::nodes, ns * n, 3)? {
        for i in 0..n {
            let node = at + ns * i;
            let links = w.s.u16_at(node + L::pathnode_t::constant__totalLinkCount)? as usize;
            w.blob(
                node + L::pathnode_t::constant__Links,
                L::pathlink_s::SIZE * links,
                3,
            )?;
        }
    }
    let bytes = |w: &Walker, o: usize| w.s.i32_at(h + o).map(|v| v.max(0) as usize);
    let vis = bytes(w, P::visBytes)?;
    w.blob(h + P::pathVis, vis, 0)?;
    let trees = bytes(w, P::nodeTreeCount)?;
    node_trees(w, h + P::nodeTree, trees)?;
    let groups = bytes(w, P::dynamicNodeGroupCount)?;
    let gs = L::PathDynamicNodeGroup::SIZE;
    if let Some(at) = w.inline_array(h + P::dynamicNodeGroups, gs * groups, 3)? {
        for i in 0..groups {
            let g = at + gs * i;
            let n =
                w.s.i32_at(g + L::PathDynamicNodeGroup::nodeTreeCount)?
                    .max(0) as usize;
            node_trees(w, g + L::PathDynamicNodeGroup::nodeTree, n)?;
        }
    }
    for (data, size) in [
        (P::pathExposure, P::exposureBytes),
        (P::pathNoPeekVis, P::noPeekVisBytes),
        (P::pathZones, P::zonesBytes),
        (P::pathDynStates, P::dynStatesBytes),
    ] {
        let n = bytes(w, size)?;
        w.blob(h + data, n, 0)?;
    }
    w.s.pop();
    Ok(w.name_only(AssetType::PathData, name))
}

// ---- ComWorld --------------------------------------------------------------

pub(super) fn com_world(w: &mut Walker, h: usize) -> Result<AssetId> {
    use L::ComWorld as C;
    w.s.push(VIRTUAL);
    let name = w.xstr(h + C::name)?.unwrap_or_else(String::new);
    let n = w.s.u32_at(h + C::primaryLightCount)? as usize;
    let pl = L::ComPrimaryLight::SIZE;
    let mut primary_lights = None;
    if let Some(at) = w.inline_array(h + C::primaryLights, pl * n, 3)? {
        primary_lights = span(Some(at), pl * n);
        for i in 0..n {
            w.xstr(at + pl * i + L::ComPrimaryLight::defName)?;
        }
    }
    let e = w.s.u32_at(h + C::primaryLightEnvCount)? as usize;
    let envs = w.blob(h + C::primaryLightEnvs, L::ComPrimaryLightEnv::SIZE * e, 3)?;
    w.s.pop();
    w.cap.com_worlds.push(ComWorldRec {
        name,
        record: Span {
            pos: h,
            len: C::SIZE,
        },
        primary_lights,
        primary_light_count: n as u32,
        primary_light_envs: span(envs, L::ComPrimaryLightEnv::SIZE * e),
        primary_light_env_count: e as u32,
    });
    Ok(AssetId {
        ty: AssetType::ComWorld,
        index: (w.cap.com_worlds.len() - 1) as u32,
    })
}

// ---- FxWorld ---------------------------------------------------------------

/// `FxWorld`: the breakable-glass system. H1 writes the initial geometry
/// before the initial piece states (Ghosts: states first). A glass def's
/// physics preset comes before its two materials (mp_creek is the first map
/// to define its own, inline).
pub(super) fn fx_world(w: &mut Walker, h: usize) -> Result<AssetId> {
    use L::FxGlassSystem as G;
    use L::FxWorld as F;
    w.s.push(VIRTUAL);
    let name = w.xstr(h + F::name)?.unwrap_or_else(String::new);
    let g = h + F::glassSys;
    let u = |w: &Walker, off: usize| -> Result<usize> { Ok(w.s.u32_at(g + off)? as usize) };
    let defs = u(w, G::defCount)?;
    let piece_limit = u(w, G::pieceLimit)?;
    let words = u(w, G::pieceWordCount)?;
    let init_pieces = u(w, G::initPieceCount)?;
    let cells = u(w, G::cellCount)?;
    let geo_limit = u(w, G::geoDataLimit)?;
    let init_geo = u(w, G::initGeoDataCount)?;
    let d = L::FxGlassDef::SIZE;
    let mut rec = FxGlassRec {
        name: name.clone(),
        init_piece_count: init_pieces,
        init_geo_count: init_geo,
        ..Default::default()
    };
    if let Some(at) = w.inline_array(g + G::defs, d * defs, 3)? {
        for i in 0..defs {
            let def = at + d * i;
            use L::FxGlassDef as D;
            let mut dr = FxGlassDefRec {
                half_thickness: w.s.f32_at(def + D::halfThickness)?,
                tex_vecs: [
                    [w.s.f32_at(def + D::texVecs)?, w.s.f32_at(def + D::texVecs + 4)?],
                    [w.s.f32_at(def + D::texVecs + 8)?, w.s.f32_at(def + D::texVecs + 12)?],
                ],
                color: [
                    w.s.u8_at(def + D::color)?,
                    w.s.u8_at(def + D::color + 1)?,
                    w.s.u8_at(def + D::color + 2)?,
                    w.s.u8_at(def + D::color + 3)?,
                ],
                is_opaque: w.s.u32_at(def + D::isOpaque)? != 0,
                ..Default::default()
            };
            for (ty, f) in [
                (AssetType::PhysPreset, D::physPreset),
                (AssetType::Material, D::material),
                (AssetType::Material, D::materialShattered),
                (AssetType::Fx, D::pieceBreakEffect),
                (AssetType::Fx, D::shatterEffect),
                (AssetType::Fx, D::shatterSmallEffect),
                (AssetType::Fx, D::crackDecalEffect),
            ] {
                let id = w.asset(ty, def + f)?;
                if f == D::material {
                    dr.material = id;
                } else if f == D::materialShattered {
                    dr.material_shattered = id;
                }
            }
            rec.defs.push(dr);
            for snd in [D::damagedSound, D::destroyedSound, D::destroyedQuietSound] {
                super::misc::sound_custom(w, def + snd)?;
            }
        }
    }
    w.s.push(RUNTIME);
    w.blob(
        g + G::piecePlaces,
        L::FxGlassPiecePlace::SIZE * piece_limit,
        3,
    )?;
    w.blob(
        g + G::pieceStates,
        L::FxGlassPieceState::SIZE * piece_limit,
        3,
    )?;
    w.blob(
        g + G::pieceDynamics,
        L::FxGlassPieceDynamics::SIZE * piece_limit,
        3,
    )?;
    w.blob(g + G::geoData, L::FxGlassGeometryData::SIZE * geo_limit, 3)?;
    w.blob(g + G::isInUse, 4 * words, 3)?;
    w.blob(g + G::cellBits, 4 * words * cells, 3)?;
    w.blob(g + G::visData, (piece_limit + 15) & !15, 15)?;
    w.blob(g + G::linkOrg, 12 * piece_limit, 3)?;
    w.blob(g + G::halfThickness, 4 * ((piece_limit + 3) & !3), 15)?;
    w.s.pop();
    w.blob(g + G::lightingHandles, 2 * init_pieces, 1)?;
    let geo_len = L::FxGlassGeometryData::SIZE * init_geo;
    rec.init_geo = span(w.blob(g + G::initGeoData, geo_len, 3)?, geo_len);
    let piece_len = L::FxGlassInitPieceState::SIZE * init_pieces;
    rec.init_pieces = span(w.blob(g + G::initPieceStates, piece_len, 3)?, piece_len);
    w.s.pop();
    w.cap.fx_glass.push(rec);
    Ok(w.name_only(AssetType::FxWorld, name))
}

// ---- GfxWorld --------------------------------------------------------------

/// `GfxWorld`: the drawn world, in x64-zt's H1 `gfx_world::write` order.
///
/// Beyond Ghosts: portal groups and portal distance anchors after the
/// cells, reflection probe volumes carry plane lists, displacement
/// parameters after the indices, three light grid trees, the static
/// visibility's 27 + 27 unknown and 4 + 4 umbra arrays (RUNTIME, except
/// the two `unknownSModelVisData` arrays which are in the file), static
/// model lighting split out of the draw instances
/// (`smodelLightingInsts`, vertex lighting values only for vertex-lit
/// models), subdivision vertex lighting, MDAO volumes and the build info
/// strings.
pub(super) fn gfx_world(w: &mut Walker, h: usize) -> Result<AssetId> {
    use L::GfxWorld as G;
    let u32_ = |w: &Walker, off: usize| -> Result<usize> { Ok(w.s.u32_at(h + off)? as usize) };
    w.s.push(VIRTUAL);
    let name = w.xstr(h + G::name)?.unwrap_or_else(String::new);
    w.xstr(h + G::baseName)?;
    let plane_count = u32_(w, G::planeCount)?;
    let node_count = u32_(w, G::nodeCount)?;
    let surface_count = u32_(w, G::surfaceCount)?;
    let sky_count = u32_(w, G::skyCount)?;
    let cell_count = u32_(w, G::dpvsPlanes__cellCount)?;
    let primary_lights = u32_(w, G::primaryLightCount)?;
    let last_sun = u32_(w, G::lastSunPrimaryLightIndex)?;
    let mut rec = GfxWorldRec {
        name: name.clone(),
        header: h,
        surface_count: surface_count as u32,
        cell_count: cell_count as u32,
        ..Default::default()
    };

    // Skies.
    let sk = L::GfxSky::SIZE;
    if let Some(at) = w.inline_array(h + G::skies, sk * sky_count, 3)? {
        for i in 0..sky_count {
            let s = at + sk * i;
            let n = w.s.u32_at(s + L::GfxSky::skySurfCount)? as usize;
            let surfs = w.blob(s + L::GfxSky::skyStartSurfs, 4 * n, 3)?;
            rec.sky_surfaces.push(span(surfs, 4 * n));
            let img = w.asset(AssetType::Image, s + L::GfxSky::skyImage)?;
            rec.sky_images.push(img);
        }
    }
    // Planes and nodes.
    w.blob(
        h + G::dpvsPlanes__planes,
        L::cplane_s::SIZE * plane_count,
        3,
    )?;
    w.blob(h + G::dpvsPlanes__nodes, 2 * node_count, 1)?;
    w.s.push(RUNTIME);
    w.blob(
        h + G::dpvsPlanes__sceneEntCellBits,
        4 * (cell_count << 9),
        3,
    )?;
    w.s.pop();
    // Cell trees.
    let counts = w.blob(h + G::aabbTreeCounts, 4 * cell_count, 3)?;
    if let Some(trees) = w.inline_array(h + G::aabbTrees, 8 * cell_count, 127)? {
        for i in 0..cell_count {
            let n = match counts {
                Some(c) => w.s.u32_at(c + 4 * i)? as usize,
                None => 0,
            };
            let a = L::GfxAabbTree::SIZE;
            if let Some(at) = w.inline_array(trees + 8 * i, a * n, 3)? {
                for k in 0..n {
                    let t = at + a * k;
                    let m = w.s.u16_at(t + L::GfxAabbTree::smodelIndexCount)? as usize;
                    w.blob(t + L::GfxAabbTree::smodelIndexes, 2 * m, 1)?;
                }
            }
        }
    }
    // Cells and portals.
    let c = L::GfxCell::SIZE;
    rec.cells = w.inline_array(h + G::cells, c * cell_count, 3)?;
    if let Some(at) = rec.cells {
        for i in 0..cell_count {
            let cell = at + c * i;
            let portals = w.s.u16_at(cell + L::GfxCell::portalCount)? as usize;
            let p = L::GfxPortal::SIZE;
            if let Some(pa) = w.inline_array(cell + L::GfxCell::portals, p * portals, 3)? {
                for k in 0..portals {
                    let portal = pa + p * k;
                    let v = w.s.u8_at(portal + L::GfxPortal::vertexCount)? as usize;
                    w.blob(portal + L::GfxPortal::vertices, 12 * v, 3)?;
                }
            }
            let probes = w.s.u8_at(cell + L::GfxCell::reflectionProbeCount)? as usize;
            w.blob(cell + L::GfxCell::reflectionProbes, probes, 0)?;
            let refs =
                w.s.u8_at(cell + L::GfxCell::reflectionProbeReferenceCount)? as usize;
            w.blob(cell + L::GfxCell::reflectionProbeReferences, refs, 0)?;
        }
    }
    // Portal groups (H1).
    let groups = u32_(w, G::portalGroupCount)?;
    let pg = L::GfxPortalGroup::SIZE;
    if let Some(at) = w.inline_array(h + G::portalGroup, pg * groups, 3)? {
        for i in 0..groups {
            let g = at + pg * i;
            w.xstr(g + L::GfxPortalGroup::targetName)?;
            let n = w.s.u16_at(g + L::GfxPortalGroup::numPortals)? as usize;
            w.blob(g + L::GfxPortalGroup::gfxPortalArray, 4 * n, 3)?;
        }
    }
    let anchors = u32_(w, G::portalDistanceAnchorCount)?;
    w.blob(
        h + G::portalDistanceAnchorsAndCloseDistSquared,
        16 * anchors,
        3,
    )?;
    // Reflection probes.
    let probes = u32_(w, G::draw__reflectionProbeCount)?;
    if let Some(at) = w.inline_array(h + G::draw__reflectionProbes, 8 * probes, 7)? {
        for i in 0..probes {
            let img = w.asset(AssetType::Image, at + 8 * i)?;
            rec.reflection_probes.push(img);
        }
    }
    let rp = L::GfxReflectionProbe::SIZE;
    rec.reflection_probe_origins =
        w.inline_array(h + G::draw__reflectionProbeOrigins, rp * probes, 3)?;
    if let Some(at) = rec.reflection_probe_origins {
        for i in 0..probes {
            let o = at + rp * i;
            let n = w.s.u32_at(o + L::GfxReflectionProbe::probeVolumeCount)? as usize;
            let pv = L::GfxReflectionProbeVolume::SIZE;
            if let Some(va) = w.inline_array(o + L::GfxReflectionProbe::probeVolumes, pv * n, 3)? {
                for k in 0..n {
                    let v = va + pv * k;
                    let planes =
                        w.s.u32_at(v + L::GfxReflectionProbeVolume::probeVolumePlaneCount)?
                            as usize;
                    w.blob(
                        v + L::GfxReflectionProbeVolume::probeVolumePlanes,
                        2 * planes,
                        1,
                    )?;
                }
            }
        }
    }
    w.s.push(RUNTIME);
    w.blob(
        h + G::draw__reflectionProbeTextures,
        L::GfxTexture::SIZE * probes,
        3,
    )?;
    w.s.pop();
    let probe_refs = u32_(w, G::draw__reflectionProbeReferenceCount)?;
    w.blob(
        h + G::draw__reflectionProbeReferenceOrigins,
        12 * probe_refs,
        3,
    )?;
    w.blob(h + G::draw__reflectionProbeReferences, probe_refs, 0)?;
    // Lightmaps.
    let lightmaps = u32_(w, G::draw__lightmapCount)?;
    let lm = L::GfxLightmapArray::SIZE;
    if let Some(at) = w.inline_array(h + G::draw__lightmaps, lm * lightmaps, 3)? {
        for i in 0..lightmaps {
            let a = w.asset(AssetType::Image, at + lm * i + L::GfxLightmapArray::primary)?;
            let b = w.asset(
                AssetType::Image,
                at + lm * i + L::GfxLightmapArray::secondary,
            )?;
            rec.lightmaps.push((a, b));
        }
    }
    w.s.push(RUNTIME);
    w.blob(
        h + G::draw__lightmapPrimaryTextures,
        L::GfxTexture::SIZE * lightmaps,
        3,
    )?;
    w.blob(
        h + G::draw__lightmapSecondaryTextures,
        L::GfxTexture::SIZE * lightmaps,
        3,
    )?;
    w.s.pop();
    w.asset(AssetType::Image, h + G::draw__lightmapOverridePrimary)?;
    w.asset(AssetType::Image, h + G::draw__lightmapOverrideSecondary)?;
    // Geometry.
    let vertex_count = u32_(w, G::draw__vertexCount)?;
    rec.vertex_count = vertex_count as u32;
    rec.vertices = w.blob(
        h + G::draw__vd__vertices,
        L::GfxWorldVertex::SIZE * vertex_count,
        3,
    )?;
    let layer = u32_(w, G::draw__vertexLayerDataSize)?;
    rec.vertex_layer_data = span(w.blob(h + G::draw__vld__data, layer, 3)?, layer);
    let index_count = u32_(w, G::draw__indexCount)?;
    rec.index_count = index_count as u32;
    rec.indices = w.blob(h + G::draw__indices, 2 * index_count, 3)?;
    let disp = u32_(w, G::draw__displacementParmsCount)?;
    w.blob(
        h + G::draw__displacementParms,
        L::GfxDisplacementParms::SIZE * disp,
        63,
    )?;
    // Light grid.
    let row_axis = u32_(w, G::lightGrid__rowAxis)?.min(2);
    let mins = w.s.u16_at(h + G::lightGrid__mins + 2 * row_axis)? as usize;
    let maxs = w.s.u16_at(h + G::lightGrid__maxs + 2 * row_axis)? as usize;
    let rows = 2 * (maxs.wrapping_sub(mins).wrapping_add(1) & 0xffff);
    rec.light_grid_row_start = span(w.blob(h + G::lightGrid__rowDataStart, rows, 1)?, rows);
    let raw = u32_(w, G::lightGrid__rawRowDataSize)?;
    rec.light_grid_rows = span(w.blob(h + G::lightGrid__rawRowData, raw, 0)?, raw);
    let entries = u32_(w, G::lightGrid__entryCount)?;
    rec.light_grid_entries = span(
        w.blob(
            h + G::lightGrid__entries,
            L::GfxLightGridEntry::SIZE * entries,
            3,
        )?,
        L::GfxLightGridEntry::SIZE * entries,
    );
    let colors = u32_(w, G::lightGrid__colorCount)?;
    rec.light_grid_colors = span(
        w.blob(
            h + G::lightGrid__colors,
            L::GfxLightGridColors::SIZE * colors,
            3,
        )?,
        L::GfxLightGridColors::SIZE * colors,
    );
    let stages = w.s.u8_at(h + G::lightGrid__stageCount)? as usize;
    w.blob(h + G::lightGrid__stageLightingContrastGain, 4 * stages, 3)?;
    let palette = u32_(w, G::lightGrid__paletteEntryCount)?;
    rec.light_grid_palette = span(
        w.blob(h + G::lightGrid__paletteEntryAddress, 4 * palette, 3)?,
        4 * palette,
    );
    let bits = u32_(w, G::lightGrid__paletteBitstreamSize)?;
    rec.light_grid_bitstream = span(w.blob(h + G::lightGrid__paletteBitstream, bits, 0)?, bits);
    if std::env::var("H1_GRID_DEBUG").is_ok() {
        eprintln!(
            "light grid: rows {rows} raw {raw} entries {entries} colours {colors} stages {stages} palette {palette} bitstream {bits}"
        );
    }
    for k in 0..3 {
        use L::GfxLightGridTree as T;
        let t = h + G::lightGrid__tree + T::SIZE * k;
        let nodes = w.s.u32_at(t + T::nodeCount)? as usize;
        if std::env::var("H1_GRID_DEBUG").is_ok() {
            eprintln!(
                "light grid tree {k}: nodes {nodes} leaves {}",
                w.s.u32_at(t + T::leafTableSize)?
            );
        }
        w.blob(t + T::p_nodeTable, 4 * nodes, 3)?;
        let leaves = w.s.u32_at(t + T::leafTableSize)? as usize;
        w.blob(t + T::p_leafTable, leaves, 0)?;
    }
    // Brush models, material memory, sun, outdoor image.
    let models = u32_(w, G::modelCount)?;
    rec.model_count = models as u32;
    rec.models = w.blob(h + G::models, L::GfxBrushModel::SIZE * models, 3)?;
    let mm = u32_(w, G::materialMemoryCount)?;
    if let Some(at) = w.inline_array(h + G::materialMemory, L::MaterialMemory::SIZE * mm, 3)? {
        for i in 0..mm {
            w.asset(
                AssetType::Material,
                at + L::MaterialMemory::SIZE * i + L::MaterialMemory::material,
            )?;
        }
    }
    rec.sun_sprite = w.asset(AssetType::Material, h + G::sun__spriteMaterial)?;
    rec.sun_flare = w.asset(AssetType::Material, h + G::sun__flareMaterial)?;
    rec.outdoor_image = w.asset(AssetType::Image, h + G::outdoorImage)?;
    // Runtime visibility.
    let dyn0 = u32_(w, G::dpvsDyn__dynEntClientCount)?;
    let dyn1 = u32_(w, G::dpvsDyn__dynEntClientCount + 4)?;
    let non_sun = primary_lights.saturating_sub(last_sun + 1);
    w.s.push(RUNTIME);
    w.blob(
        h + G::cellCasterBits,
        4 * cell_count * cell_count.div_ceil(32),
        3,
    )?;
    w.blob(
        h + G::cellHasSunLitSurfsBits,
        4 * cell_count.div_ceil(32),
        3,
    )?;
    w.blob(h + G::sceneDynModel, L::GfxSceneDynModel::SIZE * dyn0, 3)?;
    w.blob(h + G::sceneDynBrush, L::GfxSceneDynBrush::SIZE * dyn1, 3)?;
    w.blob(h + G::primaryLightEntityShadowVis, 4 * (non_sun << 13), 3)?;
    w.blob(h + G::primaryLightDynEntShadowVis, 4 * dyn0 * non_sun, 3)?;
    w.blob(
        h + G::primaryLightDynEntShadowVis + 8,
        4 * dyn1 * non_sun,
        3,
    )?;
    w.blob(h + G::nonSunPrimaryLightForModelDynEnt, 2 * dyn0, 1)?;
    w.s.pop();
    // Shadows and light regions.
    for field in [G::shadowGeom, G::shadowGeomOptimized] {
        let sg = L::GfxShadowGeometry::SIZE;
        if let Some(at) = w.inline_array(h + field, sg * primary_lights, 3)? {
            for i in 0..primary_lights {
                let g = at + sg * i;
                let s = w.s.u16_at(g + L::GfxShadowGeometry::surfaceCount)? as usize;
                let m = w.s.u16_at(g + L::GfxShadowGeometry::smodelCount)? as usize;
                w.blob(g + L::GfxShadowGeometry::sortedSurfIndex, 4 * s, 3)?;
                w.blob(g + L::GfxShadowGeometry::smodelIndex, 2 * m, 1)?;
            }
        }
    }
    let lr = L::GfxLightRegion::SIZE;
    if let Some(at) = w.inline_array(h + G::lightRegion, lr * primary_lights, 3)? {
        for i in 0..primary_lights {
            let r = at + lr * i;
            let n = w.s.u32_at(r + L::GfxLightRegion::hullCount)? as usize;
            let hl = L::GfxLightRegionHull::SIZE;
            if let Some(ha) = w.inline_array(r + L::GfxLightRegion::hulls, hl * n, 3)? {
                for k in 0..n {
                    let hull = ha + hl * k;
                    let axes = w.s.u32_at(hull + L::GfxLightRegionHull::axisCount)? as usize;
                    w.blob(
                        hull + L::GfxLightRegionHull::axis,
                        L::GfxLightRegionAxis::SIZE * axes,
                        3,
                    )?;
                }
            }
        }
    }
    // Static visibility.
    let smodels = u32_(w, G::dpvs__smodelCount)?;
    let subdiv_infos = u32_(w, G::dpvs__subdivVertexLightingInfoCount)?;
    let static_surfs = u32_(w, G::dpvs__staticSurfaceCount)?;
    let smodel_vis = u32_(w, G::dpvs__smodelVisDataCount)?;
    let surface_vis = u32_(w, G::dpvs__surfaceVisDataCount)?;
    let sm_vis = |k: usize| h + G::dpvs__smodelVisData + 8 * k;
    let su_vis = |k: usize| h + G::dpvs__surfaceVisData + 8 * k;
    let sm_unk = |k: usize| h + G::dpvs__smodelUnknownVisData + 8 * k;
    let su_unk = |k: usize| h + G::dpvs__surfaceUnknownVisData + 8 * k;
    let sm_umbra = |k: usize| h + G::dpvs__smodelUmbraVisData + 8 * k;
    let su_umbra = |k: usize| h + G::dpvs__surfaceUmbraVisData + 8 * k;
    w.s.push(RUNTIME);
    for k in 0..3 {
        w.blob(sm_vis(k), 4 * smodel_vis, 127)?;
    }
    for k in 0..3 {
        w.blob(su_vis(k), 4 * surface_vis, 127)?;
    }
    w.s.pop();
    w.blob(h + G::dpvs__unknownSModelVisData1, 4 * smodel_vis, 127)?;
    w.blob(h + G::dpvs__unknownSModelVisData2, 8 * smodel_vis, 127)?;
    w.s.push(RUNTIME);
    w.blob(sm_vis(3), 4 * smodel_vis, 127)?;
    w.blob(su_vis(3), 4 * surface_vis, 127)?;
    for (lo, hi) in [(0, 9), (9, 18), (18, 27)] {
        for k in lo..hi {
            w.blob(sm_unk(k), 4 * smodel_vis, 127)?;
        }
        for k in lo..hi {
            w.blob(su_unk(k), 4 * surface_vis, 127)?;
        }
    }
    for k in 0..3 {
        w.blob(sm_umbra(k), 4 * (smodel_vis + 1), 127)?;
    }
    for k in 0..3 {
        w.blob(su_umbra(k), 4 * surface_vis, 127)?;
    }
    w.blob(sm_umbra(3), 4 * (smodel_vis + 1), 127)?;
    w.blob(su_umbra(3), 4 * surface_vis, 127)?;
    w.s.pop();
    w.blob(h + G::dpvs__lodData, 4 * (smodels + 1), 3)?;
    w.s.push(RUNTIME);
    w.blob(h + G::dpvs__tessellationCutoffVisData, 4 * surface_vis, 127)?;
    w.s.pop();
    rec.sorted_surfs = w.blob(h + G::dpvs__sortedSurfIndex, 4 * static_surfs, 3)?;
    rec.static_surface_count = static_surfs as u32;
    rec.smodel_insts = w.blob(
        h + G::dpvs__smodelInsts,
        L::GfxStaticModelInst::SIZE * smodels,
        3,
    )?;
    let gs = L::GfxSurface::SIZE;
    rec.surfaces = w.inline_array(h + G::dpvs__surfaces, gs * surface_count, 3)?;
    if let Some(at) = rec.surfaces {
        for i in 0..surface_count {
            let m = w.asset(AssetType::Material, at + gs * i + L::GfxSurface::material)?;
            rec.surface_materials.push(m);
        }
    }
    rec.surface_bounds = w.blob(
        h + G::dpvs__surfacesBounds,
        L::GfxSurfaceBounds::SIZE * surface_count,
        3,
    )?;
    let sd = L::GfxStaticModelDrawInst::SIZE;
    rec.smodel_count = smodels as u32;
    rec.smodels = w.inline_array(h + G::dpvs__smodelDrawInsts, sd * smodels, 3)?;
    if let Some(at) = rec.smodels {
        for i in 0..smodels {
            let m = w.asset(
                AssetType::XModel,
                at + sd * i + L::GfxStaticModelDrawInst::model,
            )?;
            rec.smodel_models.push(m);
        }
    }
    // Static model lighting: a union per model; vertex lighting values are
    // stored for vertex-lit models (draw flag 0x80) only.
    let sl = L::GfxStaticModelLighting::SIZE;
    rec.smodel_lighting = w.inline_array(h + G::dpvs__smodelLightingInsts, sl * smodels, 3)?;
    if let Some(at) = rec.smodel_lighting {
        use L::GfxStaticModelLighting as S;
        for i in 0..smodels {
            let flags = match rec.smodels {
                Some(d) => w.s.u16_at(d + sd * i + L::GfxStaticModelDrawInst::flags)?,
                None => 0,
            };
            let l = at + sl * i;
            let mut values = None;
            if flags & 0x80 != 0 {
                let n =
                    w.s.i32_at(l + S::vertexLightingInfo__numLightingValues)?
                        .max(0) as usize;
                let len = L::GfxStaticModelVertexLighting::SIZE * n;
                values = span(
                    w.blob(l + S::vertexLightingInfo__lightingValues, len, 3)?,
                    len,
                );
            }
            rec.smodel_vertex_lighting.push(values);
        }
    }
    w.blob(
        h + G::dpvs__subdivVertexLighting,
        L::GfxSubdivVertexLightingInfo::SIZE * subdiv_infos,
        3,
    )?;
    let sun_opt = u32_(w, G::dpvs__sunShadowOptCount)?;
    let sun_vis = u32_(w, G::dpvs__sunSurfVisDataCount)?;
    w.s.push(RUNTIME);
    w.blob(
        h + G::dpvs__surfaceMaterials,
        L::GfxDrawSurf::SIZE * surface_count,
        7,
    )?;
    w.blob(h + G::dpvs__surfaceCastsSunShadow, 4 * surface_vis, 127)?;
    w.blob(
        h + G::dpvs__surfaceCastsSunShadowOpt,
        4 * sun_opt * sun_vis,
        127,
    )?;
    w.blob(
        h + G::dpvs__surfaceDeptAndSurf,
        L::GfxDepthAndSurf::SIZE * static_surfs,
        3,
    )?;
    w.blob(h + G::dpvs__constantBuffersLit, 8 * smodels, 7)?;
    w.blob(h + G::dpvs__constantBuffersAmbient, 8 * smodels, 7)?;
    let words0 = u32_(w, G::dpvsDyn__dynEntClientWordCount)?;
    let words1 = u32_(w, G::dpvsDyn__dynEntClientWordCount + 4)?;
    w.blob(h + G::dpvsDyn__dynEntCellBits, 4 * words0 * cell_count, 3)?;
    w.blob(
        h + G::dpvsDyn__dynEntCellBits + 8,
        4 * words1 * cell_count,
        3,
    )?;
    for (k, words) in [(0usize, words0), (1, words1)] {
        for j in 0..4 {
            w.blob(
                h + G::dpvsDyn__dynEntVisData + 32 * k + 8 * j,
                32 * words,
                15,
            )?;
        }
    }
    w.s.pop();
    let heroes = u32_(w, G::heroOnlyLightCount)?;
    let hl = L::GfxHeroOnlyLight::SIZE * heroes;
    rec.hero_only_lights = span(w.blob(h + G::heroOnlyLights, hl, 3)?, hl);
    let tome = u32_(w, G::umbraTomeSize)?;
    w.blob(h + G::umbraTomeData, tome, 15)?;
    let mdao = u32_(w, G::mdaoVolumesCount)?;
    let mv = L::MdaoVolume::SIZE;
    if let Some(at) = w.inline_array(h + G::mdaoVolumes, mv * mdao, 3)? {
        for i in 0..mdao {
            w.asset(AssetType::Image, at + mv * i + L::MdaoVolume::volumeData)?;
        }
    }
    for f in [
        G::buildInfo__bspCommandline,
        G::buildInfo__lightCommandline,
        G::buildInfo__bspTimestamp,
        G::buildInfo__lightTimestamp,
    ] {
        w.xstr(h + f)?;
    }
    w.s.pop();
    w.cap.gfx_worlds.push(rec);
    Ok(AssetId {
        ty: AssetType::GfxWorld,
        index: (w.cap.gfx_worlds.len() - 1) as u32,
    })
}

// ---- MapEnts ---------------------------------------------------------------

/// A `MapTriggers`' arrays: (models, hulls, slabs) as file spans.
fn map_triggers(w: &mut Walker, t: usize) -> Result<[Option<Span>; 3]> {
    use L::MapTriggers as T;
    let count = w.s.u32_at(t + T::count)? as usize;
    let hulls = w.s.u32_at(t + T::hullCount)? as usize;
    let slabs = w.s.u32_at(t + T::slabCount)? as usize;
    let (ms, hs, ss) = (
        L::TriggerModel::SIZE * count,
        L::TriggerHull::SIZE * hulls,
        L::TriggerSlab::SIZE * slabs,
    );
    let m = w.blob(t + T::models, ms, 3)?;
    let h = w.blob(t + T::hulls, hs, 3)?;
    let s = w.blob(t + T::slabs, ss, 3)?;
    Ok([span(m, ms), span(h, hs), span(s, ss)])
}

/// `MapEnts`: the entity string, triggers, client triggers, spawns and
/// splines (x64-zt's H1 `map_ents::write`: H1 adds light set, colour
/// lookup, context, water and one unknown client trigger table).
pub(super) fn map_ents(w: &mut Walker, h: usize) -> Result<AssetId> {
    use L::MapEnts as M;
    w.s.push(VIRTUAL);
    let name = w.xstr(h + M::name)?.unwrap_or_else(String::new);
    let chars = w.s.i32_at(h + M::numEntityChars)?.max(0) as usize;
    let ents = w.blob(h + M::entityString, chars, 0)?;
    let entity_string = match ents {
        Some(at) => {
            let b = w.s.slice(at, chars)?;
            String::from_utf8_lossy(b.split(|&c| c == 0).next().unwrap_or(&[])).into_owned()
        }
        None => String::new(),
    };
    let triggers = map_triggers(w, h + M::trigger)?;
    map_triggers(w, h + M::clientTrigger__trigger)?;
    let ct = h + M::clientTrigger;
    use L::ClientTriggers as C;
    let count = w.s.u32_at(ct + C::trigger__count)? as usize;
    let nodes = w.s.u16_at(ct + C::numClientTriggerNodes)? as usize;
    w.blob(
        ct + C::clientTriggerAabbTree,
        L::ClientTriggerAabbNode::SIZE * nodes,
        3,
    )?;
    let len = w.s.u32_at(ct + C::triggerStringLength)? as usize;
    w.blob(ct + C::triggerString, len, 0)?;
    for f in [
        C::visionSetTriggers,
        C::lightSetTriggers,
        C::clutTriggers,
        C::triggerType,
    ] {
        w.blob(ct + f, 2 * count, 1)?;
    }
    w.blob(ct + C::origins, 12 * count, 3)?;
    w.blob(ct + C::scriptDelay, 4 * count, 3)?;
    // (The writer's order, not the struct's: `unkTriggers` third.)
    for f in [
        C::audioTriggers,
        C::blendLookup,
        C::unkTriggers,
        C::npcTriggers,
        C::contextTriggers,
        C::waterTriggers,
    ] {
        w.blob(ct + f, 2 * count, 1)?;
    }
    let blends =
        w.s.u16_at(h + M::clientTriggerBlend__numClientTriggerBlendNodes)? as usize;
    w.blob(
        h + M::clientTriggerBlend__blendNodes,
        L::ClientTriggerBlendNode::SIZE * blends,
        3,
    )?;
    let spawns = w.s.u16_at(h + M::spawnList__spawnsCount)? as usize;
    let sl = L::SpawnPointEntityRecord::SIZE * spawns;
    let spawn_at = w.blob(h + M::spawnList__spawns, sl, 3)?;
    let splines = w.s.u16_at(h + M::splineList__splineCount)? as usize;
    let sr = L::SplinePointRecordList::SIZE;
    if let Some(at) = w.inline_array(h + M::splineList__splines, sr * splines, 3)? {
        for i in 0..splines {
            let s = at + sr * i;
            let n = w.s.u16_at(s + L::SplinePointRecordList::splinePointCount)? as usize;
            let pr = L::SplinePointEntityRecord::SIZE;
            if let Some(pa) =
                w.inline_array(s + L::SplinePointRecordList::splinePoints, pr * n, 3)?
            {
                for k in 0..n {
                    let p = pa + pr * k;
                    w.blob(p + L::SplinePointEntityRecord::splineNodeLabel, 64, 0)?;
                    w.blob(p + L::SplinePointEntityRecord::positionCubic, 48, 3)?;
                    w.blob(p + L::SplinePointEntityRecord::tangentQuadratic, 36, 3)?;
                }
            }
        }
    }
    w.s.pop();
    w.cap.map_ents.push(MapEntsRec {
        name,
        entity_string,
        spawns: span(spawn_at, sl),
        triggers,
    });
    Ok(AssetId {
        ty: AssetType::MapEnts,
        index: (w.cap.map_ents.len() - 1) as u32,
    })
}

// ---- GlassWorld, PhysWorld -------------------------------------------------

pub(super) fn glass_world(w: &mut Walker, h: usize) -> Result<AssetId> {
    use L::G_GlassData as D;
    w.s.push(VIRTUAL);
    let name = w.xstr(h + L::GlassWorld::name)?.unwrap_or_else(String::new);
    let mut rec = GlassWorldRec {
        name,
        ..Default::default()
    };
    if let Some(d) = w.inline_array(h + L::GlassWorld::g_glassData, D::SIZE, 3)? {
        let pieces = w.s.u32_at(d + D::pieceCount)? as usize;
        let len = L::G_GlassPiece::SIZE * pieces;
        rec.pieces = span(w.blob(d + D::glassPieces, len, 3)?, len);
        let names = w.s.u32_at(d + D::glassNameCount)? as usize;
        let gn = L::G_GlassName::SIZE;
        if let Some(at) = w.inline_array(d + D::glassNames, gn * names, 3)? {
            for i in 0..names {
                let n = at + gn * i;
                let s = w
                    .xstr(n + L::G_GlassName::nameStr)?
                    .unwrap_or_else(String::new);
                let c = w.s.u16_at(n + L::G_GlassName::pieceCount)? as usize;
                let idx = span(w.blob(n + L::G_GlassName::pieceIndices, 2 * c, 1)?, 2 * c);
                rec.names.push((s, idx));
            }
        }
    }
    w.s.pop();
    w.cap.glass_worlds.push(rec);
    Ok(AssetId {
        ty: AssetType::GlassWorld,
        index: (w.cap.glass_worlds.len() - 1) as u32,
    })
}

/// `PhysWorld` (H1: the world's physics brush models, hulls and meshes).
pub(super) fn phys_world(w: &mut Walker, h: usize) -> Result<AssetId> {
    use L::PhysWorld as P;
    w.s.push(VIRTUAL);
    let name = w.xstr(h + P::name)?.unwrap_or_else(String::new);
    let n = |w: &Walker, o: usize| w.s.u32_at(h + o).map(|v| v as usize);
    let brushes = n(w, P::brushModelCount)?;
    w.blob(h + P::brushModels, L::PhysBrushModel::SIZE * brushes, 3)?;
    let polys = n(w, P::polytopeCount)?;
    let pd = L::dmPolytopeData::SIZE;
    if let Some(at) = w.inline_array(h + P::polytopeDatas, pd * polys, 3)? {
        for i in 0..polys {
            super::misc::polytope_contents(w, at + pd * i)?;
        }
    }
    let meshes = n(w, P::meshDataCount)?;
    let md = L::dmMeshData::SIZE;
    if let Some(at) = w.inline_array(h + P::meshDatas, md * meshes, 3)? {
        use L::dmMeshData as M;
        for i in 0..meshes {
            let m = at + md * i;
            let u = |w: &Walker, o: usize| w.s.u32_at(m + o).map(|v| v as usize);
            let nodes = u(w, M::m_nodeCount)?;
            let verts = u(w, M::m_vertexCount)?;
            let tris = u(w, M::m_triangleCount)?;
            w.blob(m + M::m_pRoot, L::dmMeshNode::SIZE * nodes, 15)?;
            w.blob(m + M::m_aVertices, 16 * verts, 15)?;
            w.blob(m + M::m_aTriangles, L::dmMeshTriangle::SIZE * tris, 3)?;
        }
    }
    let water = n(w, P::waterVolumeDefCount)?;
    let wv = L::PhysWaterVolumeDef::SIZE;
    if let Some(at) = w.inline_array(h + P::waterVolumeDefs, wv * water, 3)? {
        for i in 0..water {
            w.asset(
                AssetType::PhysWaterPreset,
                at + wv * i + L::PhysWaterVolumeDef::physWaterPreset,
            )?;
        }
    }
    w.s.pop();
    Ok(w.name_only(AssetType::PhysWorld, name))
}

/// `AddonMapEnts` (a map's patch zone: extra entities and their brush
/// models). Read in the order x64-zt's H1 writer saves it: name, entity
/// string, triggers, the clip info, sub-models (each with its own info),
/// brush models, physics models, polytopes, meshes. Before this was read
/// the walk stopped here and lost every later asset of `patch_mp_*` zones
/// (their createart fog scripts among them).
pub(super) fn addon_map_ents(w: &mut Walker, h: usize) -> Result<AssetId> {
    use L::AddonMapEnts as M;
    w.s.push(VIRTUAL);
    let name = w.xstr(h + M::name)?.unwrap_or_else(String::new);
    let chars = w.s.i32_at(h + M::numEntityChars)?.max(0) as usize;
    w.blob(h + M::entityString, chars, 0)?;
    map_triggers(w, h + M::trigger)?;
    if let Some(at) = w.inline_array(h + M::info, L::ClipInfo::SIZE, 3)? {
        clip_info(w, at)?;
    }
    let subs = w.s.u32_at(h + M::numSubModels)? as usize;
    let cmo = L::cmodel_t::SIZE;
    if let Some(at) = w.inline_array(h + M::cmodels, cmo * subs, 3)? {
        for i in 0..subs {
            if let Some(info) =
                w.inline_array(at + cmo * i + L::cmodel_t::info, L::ClipInfo::SIZE, 3)?
            {
                clip_info(w, info)?;
            }
        }
    }
    w.blob(h + M::models, L::GfxBrushModel::SIZE * subs, 3)?;
    w.blob(h + M::physModels, L::PhysBrushModel::SIZE * subs, 3)?;
    let polys = w.s.u32_at(h + M::polytopeCount)? as usize;
    let pd = L::dmPolytopeData::SIZE;
    if let Some(at) = w.inline_array(h + M::polytope, pd * polys, 3)? {
        for i in 0..polys {
            super::misc::polytope_contents(w, at + pd * i)?;
        }
    }
    let meshes = w.s.u32_at(h + M::meshDataCount)? as usize;
    let md = L::dmMeshData::SIZE;
    if let Some(at) = w.inline_array(h + M::meshData, md * meshes, 3)? {
        use L::dmMeshData as D;
        for i in 0..meshes {
            let m = at + md * i;
            let u = |w: &Walker, o: usize| w.s.u32_at(m + o).map(|v| v as usize);
            let nodes = u(w, D::m_nodeCount)?;
            let verts = u(w, D::m_vertexCount)?;
            let tris = u(w, D::m_triangleCount)?;
            w.blob(m + D::m_pRoot, L::dmMeshNode::SIZE * nodes, 15)?;
            w.blob(m + D::m_aVertices, 16 * verts, 15)?;
            w.blob(m + D::m_aTriangles, L::dmMeshTriangle::SIZE * tris, 3)?;
        }
    }
    w.s.pop();
    Ok(w.name_only(AssetType::AddonMapEnts, name))
}

// ---- clipMap_t -------------------------------------------------------------

/// A `ClipInfo`'s arrays (x64-zt's shared `clip_map::write_info`, H1's
/// layout: H1 moved terrain, partitions, AABB trees and static models
/// into the info).
fn clip_info(w: &mut Walker, c: usize) -> Result<ClipInfoRec> {
    use L::ClipInfo as I;
    let mut rec = ClipInfoRec::default();
    let n = |w: &Walker, off: usize| -> Result<usize> { Ok(w.s.u32_at(c + off)? as usize) };
    let planes = n(w, I::planeCount)?;
    rec.plane_count = planes as u32;
    rec.planes = w.blob(c + I::planes, L::cplane_s::SIZE * planes, 3)?;
    let mats = n(w, I::numMaterials)?;
    let cm = L::ClipMaterial::SIZE;
    match w.inline_array(c + I::materials, cm * mats, 3)? {
        Some(at) => {
            for i in 0..mats {
                let m = at + cm * i;
                let name = w
                    .xstr(m + L::ClipMaterial::name)?
                    .unwrap_or_else(String::new);
                let flags = w.s.i32_at(m + L::ClipMaterial::surfaceFlags)?;
                let contents = w.s.i32_at(m + L::ClipMaterial::contents)?;
                rec.materials.push((name, flags, contents));
            }
        }
        None => {
            if let Some(at) = ref_pos(w, c + I::materials)? {
                for i in 0..mats {
                    let m = at + cm * i;
                    let name =
                        w.s.ptr_at(m)?
                            .virtual_offset()
                            .and_then(|v| w.string_at(v))
                            .unwrap_or_else(String::new);
                    rec.materials.push((
                        name,
                        w.s.i32_at(m + L::ClipMaterial::surfaceFlags)?,
                        w.s.i32_at(m + L::ClipMaterial::contents)?,
                    ));
                }
            }
        }
    }
    let lbn = n(w, I::bCollisionTree__leafbrushNodesCount)?;
    rec.leaf_brush_node_count = lbn as u32;
    let ln = L::cLeafBrushNode_s::SIZE;
    rec.leaf_brush_nodes = w.inline_array(c + I::bCollisionTree__leafbrushNodes, ln * lbn, 3)?;
    if let Some(at) = rec.leaf_brush_nodes {
        for i in 0..lbn {
            let node = at + ln * i;
            let count = w.s.u16_at(node + L::cLeafBrushNode_s::leafBrushCount)? as i16;
            rec.leaf_brush_lists.push(if count > 0 {
                w.blob(
                    node + L::cLeafBrushNode_s::data__leaf__brushes,
                    2 * count as usize,
                    1,
                )?
            } else {
                None
            });
        }
    } else if let Some(at) = ref_pos(w, c + I::bCollisionTree__leafbrushNodes)? {
        rec.leaf_brush_nodes = Some(at);
    }
    let leaf_brushes = n(w, I::bCollisionTree__numLeafBrushes)?;
    w.blob(c + I::bCollisionTree__leafbrushes, 2 * leaf_brushes, 1)?;
    let trees = n(w, I::pCollisionTree__aabbTreeCount)?;
    rec.aabb_tree_count = trees as u32;
    rec.aabb_trees = w.blob(
        c + I::pCollisionTree__aabbTrees,
        L::CollisionAabbTree::SIZE * trees,
        15,
    )?;
    let smn = w.s.u16_at(c + I::sCollisionTree__smodelNodeCount)? as usize;
    w.blob(
        c + I::sCollisionTree__smodelNodes,
        L::SModelAabbNode::SIZE * smn,
        3,
    )?;
    let sides = n(w, I::bCollisionData__numBrushSides)?;
    let bs = L::cbrushside_t::SIZE;
    rec.brush_side_count = sides as u32;
    rec.brush_sides = w.blob(c + I::bCollisionData__brushSides, bs * sides, 3)?;
    let edges = n(w, I::bCollisionData__numBrushEdges)?;
    w.blob(c + I::bCollisionData__brushEdges, edges, 0)?;
    let brushes = n(w, I::bCollisionData__numBrushes)?;
    rec.brush_count = brushes as u32;
    let b = L::cbrush_t::SIZE;
    rec.brushes = w.inline_array(c + I::bCollisionData__brushes, b * brushes, 127)?;
    if let Some(at) = rec.brushes {
        for i in 0..brushes {
            let br = at + b * i;
            rec.brush_first_sides
                .push(w.blob(br + L::cbrush_t::sides, bs, 3)?);
            w.blob(br + L::cbrush_t::baseAdjacentSide, 1, 0)?;
        }
    }
    rec.brush_bounds = w.blob(c + I::bCollisionData__brushBounds, 24 * brushes, 127)?;
    rec.brush_contents = w.blob(c + I::bCollisionData__brushContents, 4 * brushes, 3)?;
    let verts = n(w, I::pCollisionData__vertCount)?;
    rec.vert_count = verts as u32;
    rec.verts = w.blob(c + I::pCollisionData__verts, 12 * verts, 3)?;
    let tris = n(w, I::pCollisionData__triCount)?;
    rec.tri_count = tris as u32;
    rec.tri_indices = w.blob(c + I::pCollisionData__triIndices, 6 * tris, 1)?;
    w.blob(
        c + I::pCollisionData__triEdgeIsWalkable,
        ((3 * tris + 31) >> 3) & !3,
        0,
    )?;
    let borders = n(w, I::pCollisionData__borderCount)?;
    w.blob(
        c + I::pCollisionData__borders,
        L::CollisionBorder::SIZE * borders,
        3,
    )?;
    let parts = n(w, I::pCollisionData__partitionCount)?;
    let cp = L::CollisionPartition::SIZE;
    rec.partition_count = parts as u32;
    rec.partitions = w.inline_array(c + I::pCollisionData__partitions, cp * parts, 3)?;
    match rec.partitions {
        Some(at) => {
            for i in 0..parts {
                w.blob(
                    at + cp * i + L::CollisionPartition::borders,
                    L::CollisionBorder::SIZE,
                    3,
                )?;
            }
        }
        None => rec.partitions = ref_pos(w, c + I::pCollisionData__partitions)?,
    }
    let smodels = n(w, I::sCollisionData__numStaticModels)?;
    let sm = L::cStaticModel_s::SIZE;
    rec.static_models = w.inline_array(c + I::sCollisionData__staticModelList, sm * smodels, 3)?;
    if let Some(at) = rec.static_models {
        for i in 0..smodels {
            let m = w.asset(AssetType::XModel, at + sm * i + L::cStaticModel_s::xmodel)?;
            rec.static_model_models.push(m);
        }
    }
    Ok(rec)
}

/// The file offset a pointer field names when it refers to VIRTUAL data
/// read earlier.
fn ref_pos(w: &Walker, field: usize) -> Result<Option<usize>> {
    Ok(match w.s.raw_ptr_at(field)? {
        Ptr::Ref {
            block: VIRTUAL,
            offset,
        } => w.vmap.file_pos(offset as u64),
        _ => None,
    })
}

/// `clipMap_t`: collision, dynamic entities, scriptables and grapple
/// magnets (x64-zt's shared `clip_map::write` with H1's types).
pub(super) fn clip_map(w: &mut Walker, h: usize) -> Result<AssetId> {
    use L::clipMap_t as M;
    let n32 = |w: &Walker, off: usize| -> Result<usize> { Ok(w.s.u32_at(h + off)? as usize) };
    w.s.push(VIRTUAL);
    let name = w.xstr(h + M::name)?.unwrap_or_else(String::new);
    let info = clip_info(w, h + M::info)?;
    w.s.push(TEMP);
    if let Some(at) = w.inline_array(h + M::pInfo, L::ClipInfo::SIZE, 3)? {
        clip_info(w, at)?;
    }
    w.s.pop();
    let mut rec = ClipMapRec {
        name: name.clone(),
        header: h,
        verts: info.verts,
        vert_count: info.vert_count,
        tri_indices: info.tri_indices,
        tri_count: info.tri_count,
        partitions: info.partitions,
        partition_count: info.partition_count,
        aabb_trees: info.aabb_trees,
        aabb_tree_count: info.aabb_tree_count,
        static_models: info.static_models,
        static_model_models: info.static_model_models.clone(),
        info,
        ..Default::default()
    };
    let nodes = n32(w, M::numNodes)?;
    let nd = L::cNode_t::SIZE;
    if let Some(at) = w.inline_array(h + M::nodes, nd * nodes, 3)? {
        for i in 0..nodes {
            w.blob(at + nd * i + L::cNode_t::plane, L::cplane_s::SIZE, 3)?;
        }
    }
    let leafs = n32(w, M::numLeafs)?;
    w.blob(h + M::leafs, L::cLeaf_t::SIZE * leafs, 3)?;
    let subs = n32(w, M::numSubModels)?;
    let cmo = L::cmodel_t::SIZE;
    rec.cmodel_count = subs as u32;
    rec.cmodels = w.inline_array(h + M::cmodels, cmo * subs, 3)?;
    if let Some(at) = rec.cmodels {
        for i in 0..subs {
            // (The tools point every sub-model at the map's `pInfo`; a
            // sub-model with its own info would load it right here.)
            let own = w.inline_array(at + cmo * i + L::cmodel_t::info, L::ClipInfo::SIZE, 3)?;
            if let Some(info) = own {
                clip_info(w, info)?;
            }
            rec.cmodel_own_info.push(own.is_some());
        }
    }
    rec.map_ents = w.asset(AssetType::MapEnts, h + M::mapEnts)?;
    let stages = w.s.u8_at(h + M::stageCount)? as usize;
    let st = L::Stage::SIZE;
    rec.stage_count = stages as u32;
    rec.stages = w.inline_array(h + M::stages, st * stages, 3)?;
    if let Some(at) = rec.stages {
        for i in 0..stages {
            w.xstr(at + st * i + L::Stage::name)?;
        }
    }
    rec.stage_triggers = map_triggers(w, h + M::stageTrigger)?;
    let dyn_counts = [
        w.s.u16_at(h + M::dynEntCount)? as usize,
        w.s.u16_at(h + M::dynEntCount + 2)? as usize,
    ];
    let de = L::DynEntityDef::SIZE;
    for (k, &count) in dyn_counts.iter().enumerate() {
        rec.dyn_ent_counts[k] = count as u32;
        rec.dyn_ent_defs[k] = w.inline_array(h + M::dynEntDefList + 8 * k, de * count, 3)?;
        if let Some(at) = rec.dyn_ent_defs[k] {
            use L::DynEntityDef as D;
            for i in 0..count {
                let d = at + de * i;
                let m = w.asset(AssetType::XModel, d + D::baseModel)?;
                rec.dyn_ent_models[k].push(m);
                w.asset(AssetType::Fx, d + D::destroyFx)?;
                w.asset(AssetType::Sound, d + D::sound)?;
                w.asset(AssetType::PhysPreset, d + D::physPreset)?;
                w.blob(d + D::hinge, L::DynEntityHingeDef::SIZE, 3)?;
                w.blob(d + D::linkTo, L::DynEntityLinkToDef::SIZE, 3)?;
            }
        }
    }
    w.s.push(RUNTIME);
    for (base, size) in [
        (M::dynEntPoseList, L::DynEntityPose::SIZE),
        (M::dynEntClientList, L::DynEntityClient::SIZE),
        (M::dynEntCollList, L::DynEntityColl::SIZE),
    ] {
        for (k, &count) in dyn_counts.iter().enumerate() {
            w.blob(h + base + 8 * k, size * count, 3)?;
        }
    }
    w.s.pop();
    let anchors = n32(w, M::dynEntAnchorCount)?;
    w.blob(h + M::dynEntAnchorNames, 4 * anchors, 3)?;
    let instances = n32(w, M::scriptableMapEnts__instanceCount)?;
    let si = L::ScriptableInstance::SIZE;
    if let Some(at) = w.inline_array(h + M::scriptableMapEnts__instances, si * instances, 3)? {
        use L::ScriptableInstance as S;
        rec.scriptables = Some(at);
        rec.scriptable_count = instances as u32;
        for i in 0..instances {
            let inst = at + si * i;
            let def = w.asset(AssetType::Scriptable, inst + S::def)?;
            rec.scriptable_defs.push(def);
            let Some(def) = def else {
                continue;
            };
            let dh = w.header_of(def).ok_or(ReadError::Invalid(
                "scriptable instance def not found",
                inst,
            ))?;
            let constants = w.s.u8_at(dh + L::ScriptableDef::eventConstantsSize)? as usize;
            let parts = w.s.u8_at(dh + L::ScriptableDef::partCount)? as usize;
            let stream = w.s.u8_at(dh + L::ScriptableDef::eventStreamSize)? as usize;
            let targets = w.s.u8_at(inst + S::targetCount)? as usize;
            w.blob(inst + S::eventConstantsBuf, constants, 0)?;
            w.blob(
                inst + S::targetData,
                L::ScriptableInstanceTargetData::SIZE * targets,
                3,
            )?;
            w.s.push(RUNTIME);
            w.asset(AssetType::XModel, inst + S::currentModel)?;
            w.blob(
                inst + S::partStates,
                L::ScriptableInstancePartState::SIZE * parts,
                3,
            )?;
            w.blob(inst + S::eventStreamBuf, stream, 0)?;
            w.s.pop();
        }
    }
    let anims = n32(w, M::scriptableMapEnts__animEntryCount)?;
    let ae = L::ScriptableAnimationEntry::SIZE;
    if let Some(at) = w.inline_array(h + M::scriptableMapEnts__animEntries, ae * anims, 3)? {
        for i in 0..anims {
            w.xstr(at + ae * i + L::ScriptableAnimationEntry::animName)?;
        }
    }
    // Grapple magnets (H1).
    let trees = w.s.i32_at(h + M::grapple__magnetTree__treeCount)?.max(0) as usize;
    let ts = L::sphere_tree_t::SIZE;
    if let Some(at) = w.inline_array(h + M::grapple__magnetTree__tree, ts * trees, 3)? {
        for i in 0..trees {
            let t = at + ts * i;
            let n = w.s.i32_at(t + L::sphere_tree_t::numObjects)?.max(0) as usize;
            w.blob(t + L::sphere_tree_t::objIdx, 4 * n, 3)?;
        }
    }
    let objects = w.s.i32_at(h + M::grapple__magnetTree__numObjects)?.max(0) as usize;
    w.blob(
        h + M::grapple__magnetTree__objects,
        L::sphere_tree_obj_t::SIZE * objects,
        3,
    )?;
    let magnets = n32(w, M::grapple__magnetCount)?;
    w.blob(
        h + M::grapple__magnets,
        L::grapple_magnet_t::SIZE * magnets,
        3,
    )?;
    w.s.pop();
    w.cap.clip_maps.push(rec);
    Ok(AssetId {
        ty: AssetType::ClipMap,
        index: (w.cap.clip_maps.len() - 1) as u32,
    })
}
