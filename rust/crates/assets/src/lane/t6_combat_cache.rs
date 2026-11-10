//! Black Ops II's combat load (`load_t6_combat`) kept beside the map bake.
//!
//! The walk also builds scripts, HUD pictures, sounds, visions and the
//! camo tables. Those travel in the same file as the guns. A hit does not
//! read the zones again. It still registers the stored visions and installs
//! the stored camos, which the live walk does as it goes.
//!
//! Byte runs (pictures, the sound catalog, scripts) go in the file's blob
//! section by number (`finished_gun_cache::Blobs`); the JSON side holds
//! only the small facts.

use asset_material::MaterialCatalog;

use super::finished_gun_cache::{self, Blobs, BlobsIn, GpuDto, MatAt, Parts};
use super::t6_m2::{T6Camos, T6Combat};

#[derive(serde::Serialize, serde::Deserialize)]
struct Side {
    sound: Option<u32>,
    sound_err: Option<String>,
    footsteps: Vec<asset_t6::FootstepTableRef>,
    room_tone: Option<String>,
    scripts: u32,
    icons: Vec<(String, GpuDto)>,
    font_sheet: Option<GpuDto>,
    fonts: Vec<crate::T6HudFont>,
    ui_scripts: Vec<(String, u32)>,
    ui_strings: Vec<(String, String)>,
    ui_tables: Vec<(String, usize, usize, Vec<String>)>,
    ui_configs: Vec<(String, String)>,
    ui_settings: Vec<(String, f32)>,
    pen: Option<[[f32; 32]; 4]>,
    visions: Vec<(String, u32)>,
    filters: Vec<FilterDto>,
    camos: CamoDto,
}

#[derive(serde::Serialize, serde::Deserialize)]
struct FilterDto {
    material: String,
    images: Vec<(String, GpuDto)>,
    constants: Vec<(String, [f32; 4])>,
}

/// A catalog material or picture by (namespace, name): catalog indices
/// are not the same from one catalog to the next, names are.
type Name = (String, String);

/// The camo tables as installed (`T6Camos`), every catalog index a name.
#[derive(serde::Serialize, serde::Deserialize, Default)]
struct CamoDto {
    /// Whether the map had a camo table at all.
    on: bool,
    /// option, gun material, camo material, tile.
    swaps: Vec<(u32, Name, Name, f32)>,
    /// option, gun material, pattern picture, scale, full, style.
    patterns: Vec<(u32, Name, Name, f32, bool, u16)>,
    options: Vec<(u32, String)>,
    /// camo number, pattern picture, scale, full, style.
    extra: Vec<(u32, Name, f32, bool, u16)>,
    /// option, camo material, tile.
    pap: Option<(u32, Name, f32)>,
}

fn material_name(catalog: &MaterialCatalog, i: u32) -> Result<Name, String> {
    catalog
        .materials
        .get(i as usize)
        .map(|m| (m.namespace.as_str().to_owned(), m.name.as_str().to_owned()))
        .ok_or_else(|| format!("camo material {i} is not in the catalog"))
}

fn image_name(catalog: &MaterialCatalog, i: u32) -> Result<Name, String> {
    catalog
        .images
        .get(i as usize)
        .map(|m| (m.namespace.as_str().to_owned(), m.name.as_str().to_owned()))
        .ok_or_else(|| format!("camo picture {i} is not in the catalog"))
}

fn material_at(catalog: &MaterialCatalog, name: &Name) -> Option<u32> {
    let ns = finished_gun_cache::ns_of(&name.0)?;
    catalog.material_index_by_ns(ns, &name.1).map(|i| i.order() as u32)
}

fn image_at(catalog: &MaterialCatalog, name: &Name) -> Option<u32> {
    let ns = finished_gun_cache::ns_of(&name.0)?;
    catalog.image_index_by_key(ns, &name.1).map(|i| i as u32)
}

fn camos_out(camos: &T6Camos, catalog: &MaterialCatalog) -> Result<CamoDto, String> {
    let mut dto = CamoDto::default();
    if let Some(table) = &camos.table {
        dto.on = true;
        for (&(option, base), camo) in &table.swaps {
            dto.swaps.push((option, material_name(catalog, base)?, material_name(catalog, camo.material)?, camo.tile));
        }
        for (&(option, base), p) in &table.patterns {
            dto.patterns.push((option, material_name(catalog, base)?, image_name(catalog, p.image)?, p.scale, p.full, p.style));
        }
        dto.options = table.options.iter().map(|(&o, n)| (o, n.clone())).collect();
        dto.swaps.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.cmp(&b.1)));
        dto.patterns.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.cmp(&b.1)));
        dto.options.sort();
    }
    for (&n, p) in &camos.extra {
        dto.extra.push((n, image_name(catalog, p.image)?, p.scale, p.full, p.style));
    }
    dto.extra.sort_by(|a, b| a.0.cmp(&b.0));
    if let Some((option, camo)) = camos.pap {
        dto.pap = Some((option, material_name(catalog, camo.material)?, camo.tile));
    }
    Ok(dto)
}

/// The camos back by name. A name the restored catalog does not have
/// (it cannot happen for a file written from this catalog) drops that
/// one entry rather than the whole file.
fn camos_in(dto: CamoDto, catalog: &MaterialCatalog) -> T6Camos {
    use asset_core::weapon_camos::{CamoTable, MaterialCamo, PatternCamo};
    let table = dto.on.then(|| {
        let mut table = CamoTable::default();
        for (option, base, camo, tile) in dto.swaps {
            if let (Some(b), Some(c)) = (material_at(catalog, &base), material_at(catalog, &camo)) {
                table.swaps.insert((option, b), MaterialCamo { material: c, tile });
            }
        }
        for (option, base, image, scale, full, style) in dto.patterns {
            if let (Some(b), Some(i)) = (material_at(catalog, &base), image_at(catalog, &image)) {
                table.patterns.insert((option, b), PatternCamo { image: i, scale, full, style });
            }
        }
        table.options = dto.options.into_iter().collect();
        table
    });
    let extra = dto
        .extra
        .into_iter()
        .filter_map(|(n, image, scale, full, style)| Some((n, PatternCamo { image: image_at(catalog, &image)?, scale, full, style })))
        .collect();
    let pap = dto.pap.and_then(|(option, camo, tile)| Some((option, MaterialCamo { material: material_at(catalog, &camo)?, tile })));
    T6Camos { table, extra, pap }
}

pub(crate) fn pack(combat: &T6Combat, materials: &MaterialCatalog, at: MatAt) -> Result<Vec<u8>, String> {
    let mut blobs = Blobs::default();
    let (sound, sound_err) = match &combat.sound {
        Ok(catalog) => (Some(blobs.put(catalog.export_finished()?)), None),
        Err(error) => (None, Some(error.clone())),
    };
    let icons = combat
        .hud_icons
        .0
        .iter()
        .map(|(name, image)| Ok((name.clone(), finished_gun_cache::gpu_out(image, &mut blobs)?)))
        .collect::<Result<Vec<_>, String>>()?;
    let font_sheet = combat
        .hud_fonts
        .sheet
        .as_deref()
        .map(|image| finished_gun_cache::gpu_out(image, &mut blobs))
        .transpose()?;
    let filters = combat
        .screen_filters
        .0
        .iter()
        .map(|filter| {
            let images = filter
                .images
                .iter()
                .map(|(name, image)| Ok((name.clone(), finished_gun_cache::gpu_out(image, &mut blobs)?)))
                .collect::<Result<Vec<_>, String>>()?;
            Ok(FilterDto {
                material: filter.material.clone(),
                images,
                constants: filter.constants.clone(),
            })
        })
        .collect::<Result<Vec<_>, String>>()?;
    let scripts = blobs.put(combat.scripts.export_finished()?);
    let ui_scripts = combat
        .ui
        .scripts
        .iter()
        .map(|(name, bytes)| (name.clone(), blobs.put(bytes.as_ref().to_vec())))
        .collect();
    let visions = combat
        .visions
        .iter()
        .map(|(name, bytes)| (name.clone(), blobs.put(bytes.clone())))
        .collect();
    let side = serde_json::to_vec(&Side {
        sound,
        sound_err,
        footsteps: combat.footsteps.clone(),
        room_tone: combat.room_tone.clone(),
        scripts,
        icons,
        font_sheet,
        fonts: combat.hud_fonts.fonts.clone(),
        ui_scripts,
        ui_strings: combat.ui.strings.as_ref().clone(),
        ui_tables: combat.ui.tables.as_ref().clone(),
        ui_configs: combat.ui.configs.as_ref().clone(),
        ui_settings: combat.ui.game_settings.as_ref().clone(),
        pen: combat.pen_table.map(|table| table.rows()),
        visions,
        filters,
        camos: camos_out(&combat.camos, materials)?,
    })
    .map_err(|e| e.to_string())?;
    let projectiles = asset_model::ProjectileMeshBuild::default();
    finished_gun_cache::pack_parts_with(
        &Parts {
            weapons: &combat.weapons,
            fpv: &combat.fpv,
            world: &combat.world_weapons,
            projectiles: &projectiles,
            xanims: &combat.xanims,
            fx: &combat.fx,
            fx_models: &combat.fx_models,
            tracers: &combat.tracers,
            impact: combat.impact_fx.as_ref(),
            materials,
            mat_at: at,
            reports: &combat.report,
            extra: &side,
        },
        blobs,
    )
}

/// The side data back from its JSON and the blobs (None: the file is not
/// this code's).
fn side_in(side: Side, blobs: &BlobsIn, catalog: &MaterialCatalog) -> Option<SideIn> {
    let sound = if let Some(id) = side.sound {
        Ok(asset_audio::SoundCatalog::import_finished(blobs.get(id)?)?)
    } else {
        Err(side.sound_err?)
    };
    let icons = side
        .icons
        .into_iter()
        .map(|(name, gpu)| Some((name, finished_gun_cache::gpu_in(gpu, blobs)?)))
        .collect::<Option<Vec<_>>>()?;
    let font_sheet = match side.font_sheet {
        None => None,
        Some(gpu) => Some(finished_gun_cache::gpu_in(gpu, blobs)?),
    };
    let filters = side
        .filters
        .into_iter()
        .map(|filter| {
            let images = filter
                .images
                .into_iter()
                .map(|(name, gpu)| Some((name, finished_gun_cache::gpu_in(gpu, blobs)?)))
                .collect::<Option<Vec<_>>>()?;
            Some(crate::T6ScreenFilter {
                material: filter.material,
                images,
                constants: filter.constants,
            })
        })
        .collect::<Option<Vec<_>>>()?;
    let scripts = crate::T6ScriptSet::import_finished(blobs.get(side.scripts)?)?;
    let ui_scripts = side
        .ui_scripts
        .into_iter()
        .map(|(name, id)| Some((name, blobs.owned(id)?.into())))
        .collect::<Option<Vec<_>>>()?;
    let visions = side
        .visions
        .into_iter()
        .map(|(name, id)| Some((name, blobs.owned(id)?)))
        .collect::<Option<Vec<_>>>()?;
    Some(SideIn {
        sound,
        footsteps: side.footsteps,
        room_tone: side.room_tone,
        scripts,
        hud_icons: crate::T6HudIcons(icons),
        hud_fonts: crate::T6HudFonts {
            sheet: font_sheet,
            fonts: side.fonts,
        },
        ui: crate::T6Ui {
            scripts: ui_scripts,
            strings: std::sync::Arc::new(side.ui_strings),
            tables: std::sync::Arc::new(side.ui_tables),
            configs: std::sync::Arc::new(side.ui_configs),
            game_settings: std::sync::Arc::new(side.ui_settings),
        },
        pen_table: side.pen.map(weapon_iw4::PenetrationDepthTable::from_depths),
        visions,
        screen_filters: crate::T6ScreenFilters(filters),
        camos: camos_in(side.camos, catalog),
    })
}

struct SideIn {
    sound: Result<asset_audio::SoundCatalog, String>,
    footsteps: Vec<asset_t6::FootstepTableRef>,
    room_tone: Option<String>,
    scripts: crate::T6ScriptSet,
    hud_icons: crate::T6HudIcons,
    hud_fonts: crate::T6HudFonts,
    ui: crate::T6Ui,
    pen_table: Option<weapon_iw4::PenetrationDepthTable>,
    visions: Vec<(String, Vec<u8>)>,
    screen_filters: crate::T6ScreenFilters,
    camos: T6Camos,
}

pub(crate) fn unpack(body: &[u8], materials: &mut MaterialCatalog) -> Option<T6Combat> {
    let seed = std::mem::take(materials);
    let rest = match finished_gun_cache::unpack_parts(body, seed) {
        Ok(rest) => rest,
        Err(seed) => {
            *materials = seed;
            return None;
        }
    };
    let side = serde_json::from_slice::<Side>(&rest.extra)
        .ok()
        .and_then(|side| side_in(side, &rest.blobs, &rest.materials));
    *materials = rest.materials;
    let side = side?;
    Some(T6Combat {
        weapons: rest.weapons,
        fpv: rest.fpv,
        world_weapons: rest.world,
        xanims: rest.xanims,
        fx: rest.fx,
        fx_models: rest.fx_models,
        tracers: rest.tracers,
        impact_fx: rest.impact,
        sound: side.sound,
        footsteps: side.footsteps,
        room_tone: side.room_tone,
        scripts: side.scripts,
        hud_icons: side.hud_icons,
        hud_fonts: side.hud_fonts,
        ui: side.ui,
        pen_table: side.pen_table,
        visions: side.visions,
        screen_filters: side.screen_filters,
        camos: side.camos,
        report: rest.reports,
    })
}
