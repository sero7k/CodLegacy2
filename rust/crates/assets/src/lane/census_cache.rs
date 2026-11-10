//! The common-zone walk (`ZoneLane::load_common_mp`) kept beside the map bake.
//!
//! IW4, T5 and IW5 each return guns together with scene models, scripts,
//! team settings and an image plan. The file holds that census. A hit builds
//! a new image plan from the restored materials (the plan is a live task)
//! and fills the sound claim with the catalog the walk deposited.
//! Black Ops II's common stub does not come here (`caches_common_weapons`).

use std::cell::RefCell;
use std::path::Path;

use asset_anim::{MULTIPLAYER_ANIMTREE_PATH, PLAYERANIM_SCRIPT_PATH, PLAYERANIM_TYPES_PATH};
use asset_core::ZoneGame;
use asset_material::MaterialCatalog;
use asset_transport::{LoadProgress, StageId, ZoneImage};

use super::finished_gun_cache::{self, MatAt, Parts, Sources};
use super::CommonCensus;

std::thread_local! {
    static SOUND: RefCell<Option<Result<asset_audio::SoundCatalog, String>>> = const { RefCell::new(None) };
}

pub(crate) fn note_sound(saved: Result<asset_audio::SoundCatalog, String>) {
    SOUND.with(|slot| *slot.borrow_mut() = Some(saved));
}

fn take_sound() -> Option<Result<asset_audio::SoundCatalog, String>> {
    SOUND.with(|slot| slot.borrow_mut().take())
}

pub(crate) fn load(
    game: ZoneGame,
    path: &Path,
    image: &ZoneImage,
    progress: &LoadProgress,
    decode: bool,
    seed: MaterialCatalog,
    walk: impl FnOnce(&Path, &ZoneImage, &LoadProgress, bool, MaterialCatalog) -> CommonCensus,
) -> CommonCensus {
    let files = [path.to_path_buf()];
    let code = [
        include_str!("finished_gun_cache.rs"),
        include_str!("census_cache.rs"),
        include_str!("iw4.rs"),
        include_str!("t5.rs"),
        include_str!("iw5.rs"),
    ];
    let stem = path
        .file_stem()
        .map_or_else(|| "common_mp".to_owned(), |s| s.to_string_lossy().into_owned());
    let names = vec![stem.clone()];
    let lane = game.prefix();
    let src = Sources {
        lane,
        name: &stem,
        files: &files,
        caps: &[("decode", u32::from(decode))],
        code: &code,
        zones: &names,
        note: "",
        missing_ok: false,
    };
    let _ = take_sound();
    let at = MatAt::of(&seed);
    if let Some(body) = finished_gun_cache::read_body(&src) {
        match unpack(&body, seed, path, progress, decode, game) {
            Ok(census) => {
                if let Some(dir) = map_bake::map_dir(lane, &stem) {
                    diag::info!(
                        World,
                        "{lane} guest guns: {stem} reads {}",
                        dir.join("finished_guns.pak").display()
                    );
                }
                return census;
            }
            Err(seed) => {
                diag::warn!(World, "{lane} guest guns: cache unreadable, built again");
                let census = walk(path, image, progress, decode, seed);
                return finish(census, &src, at);
            }
        }
    }
    let census = walk(path, image, progress, decode, seed);
    finish(census, &src, at)
}

fn finish(census: CommonCensus, src: &Sources<'_>, at: MatAt) -> CommonCensus {
    let sound = take_sound();
    match pack(&census, at, sound.as_ref()) {
        Ok(bytes) => finished_gun_cache::write_logged(src, &bytes),
        Err(why) => diag::warn!(World, "{} guest guns: not cached ({why})", src.lane),
    }
    census
}

fn pack(
    census: &CommonCensus,
    at: MatAt,
    sound: Option<&Result<asset_audio::SoundCatalog, String>>,
) -> Result<Vec<u8>, String> {
    let side = pack_side(census, sound)?;
    finished_gun_cache::pack_parts(&Parts {
        weapons: &census.weapons,
        fpv: &census.fpv,
        world: &census.world_weapons,
        projectiles: &census.projectile_meshes,
        xanims: &census.xanims,
        fx: &census.fx,
        fx_models: &census.fx_models,
        tracers: &census.tracers,
        impact: census.impact_fx.as_ref(),
        materials: &census.material_population,
        mat_at: at,
        reports: &census.report,
        extra: &side,
    })
}

#[derive(serde::Serialize, serde::Deserialize)]
struct Side {
    scripts: Vec<u8>,
    scene: Vec<SceneDto>,
    phys: [usize; 3],
    shared: Vec<SharedDto>,
    anim_atr: Option<Vec<u8>>,
    anim_script: Option<Vec<u8>>,
    anim_types: Option<Vec<u8>>,
    anim_errors: Vec<String>,
    lights: Vec<LightDto>,
    pen: [[f32; 32]; 4],
    pen_loaded: bool,
    lochit: Option<Vec<f32>>,
    walked: usize,
    unclassified: usize,
    projectile_names: Vec<String>,
    s1_bytes: usize,
    teams: Vec<(String, asset_game::MapTeamSettings)>,
    visions: Vec<VisionDto>,
    tables: Vec<asset_game::CapturedStringTable>,
    sound: Option<Vec<u8>>,
    sound_err: Option<String>,
}

#[derive(serde::Serialize, serde::Deserialize)]
struct SceneDto {
    name: String,
    kind: u8,
    reason: String,
    zone: String,
    skel: Option<Vec<u8>>,
}

#[derive(serde::Serialize, serde::Deserialize)]
struct SharedDto {
    name: String,
    lod: usize,
    skel: Vec<u8>,
}

#[derive(serde::Serialize, serde::Deserialize)]
struct LightDto {
    ns: String,
    real: bool,
    name: String,
    image: Option<String>,
    width: Option<u16>,
    sampler: u8,
    lmap: i32,
}

#[derive(serde::Serialize, serde::Deserialize)]
struct VisionDto {
    name: String,
    ok: Option<FilmDto>,
    err: Option<String>,
}

#[derive(serde::Serialize, serde::Deserialize)]
struct FilmDto {
    enable: bool,
    contrast: f32,
    brightness: f32,
    desaturation: f32,
    desaturation_dark: f32,
    invert: bool,
    light_tint: [f32; 3],
    medium_tint: [f32; 3],
    dark_tint: [f32; 3],
    glow_enable: bool,
    glow_radius: f32,
    glow_bloom_cutoff: f32,
    glow_bloom_desaturation: f32,
    glow_bloom_intensity: f32,
}

fn pack_side(
    census: &CommonCensus,
    sound: Option<&Result<asset_audio::SoundCatalog, String>>,
) -> Result<Vec<u8>, String> {
    let scripts = census.scripts.export_common()?;
    let (sound, sound_err) = match sound {
        None => (None, None),
        Some(Ok(catalog)) => (Some(catalog.export_finished()?), None),
        Some(Err(error)) => (None, Some(error.clone())),
    };
    let (anim_atr, anim_script, anim_types, errors) = census.player_anim_sources.finished_sources();
    let scene = census
        .scene_models
        .export_finished()
        .into_iter()
        .map(|row| {
            let skel = row
                .skel
                .as_deref()
                .map(|skel| finished_gun_cache::encode_skel(&census.material_population, skel))
                .transpose()?;
            Ok(SceneDto {
                name: row.name,
                kind: row.kind,
                reason: row.reason,
                zone: row.zone,
                skel,
            })
        })
        .collect::<Result<Vec<_>, String>>()?;
    let shared = census
        .shared_surfaces
        .0
        .iter()
        .map(|(name, (skel, lod))| {
            Ok(SharedDto {
                name: name.clone(),
                lod: *lod,
                skel: finished_gun_cache::encode_skel(&census.material_population, skel)?,
            })
        })
        .collect::<Result<Vec<_>, String>>()?;
    let lights = census
        .light_defs
        .iter()
        .map(|light| {
            let (real, name) = match &light.name {
                asset_core::AssetRef::Real(name) => (true, name.clone()),
                asset_core::AssetRef::Reference(name) => (false, name.clone()),
            };
            LightDto {
                ns: light.namespace.as_str().to_owned(),
                real,
                name,
                image: light.attenuation_image_name.clone(),
                width: light.attenuation_width,
                sampler: light.attenuation_sampler,
                lmap: light.lmap_lookup_start,
            }
        })
        .collect();
    let visions = census
        .film_visions
        .iter()
        .map(|(name, vision)| VisionDto {
            name: name.clone(),
            ok: vision.as_ref().ok().map(film_out),
            err: vision.as_ref().err().map(|e| format!("{e:?}")),
        })
        .collect();
    serde_json::to_vec(&Side {
        scripts,
        scene,
        phys: {
            let (a, b, c) = census.scene_models.phys_counts();
            [a, b, c]
        },
        shared,
        anim_atr,
        anim_script,
        anim_types,
        anim_errors: errors.into_iter().map(str::to_owned).collect(),
        lights,
        pen: census.pen_table.rows(),
        pen_loaded: census.pen_table_loaded,
        lochit: census.lochit_table.map(|row| row.to_vec()),
        walked: census.xmodel_walk.walked_n,
        unclassified: census.xmodel_walk.unclassified_n,
        projectile_names: census.xmodel_walk.projectile_names.clone(),
        s1_bytes: census.s1_common_bytes,
        teams: census.teamsets.iter().map(|(k, v)| (k.clone(), v.clone())).collect(),
        visions,
        tables: census.cac_tables.clone(),
        sound,
        sound_err,
    })
    .map_err(|e| e.to_string())
}

fn film_out(film: &asset_world::FilmVision) -> FilmDto {
    FilmDto {
        enable: film.enable,
        contrast: film.contrast,
        brightness: film.brightness,
        desaturation: film.desaturation,
        desaturation_dark: film.desaturation_dark,
        invert: film.invert,
        light_tint: film.light_tint,
        medium_tint: film.medium_tint,
        dark_tint: film.dark_tint,
        glow_enable: film.glow_enable,
        glow_radius: film.glow_radius,
        glow_bloom_cutoff: film.glow_bloom_cutoff,
        glow_bloom_desaturation: film.glow_bloom_desaturation,
        glow_bloom_intensity: film.glow_bloom_intensity,
    }
}

fn unpack(
    body: &[u8],
    seed: MaterialCatalog,
    path: &Path,
    progress: &LoadProgress,
    decode: bool,
    game: ZoneGame,
) -> Result<CommonCensus, MaterialCatalog> {
    let saved = seed.clone();
    let rest = match finished_gun_cache::unpack_parts(body, seed) {
        Ok(rest) => rest,
        Err(seed) => return Err(seed),
    };
    let Some(parsed) = serde_json::from_slice::<Side>(&rest.extra).ok() else {
        return Err(saved);
    };
    if let Some(bytes) = &parsed.sound {
        match asset_audio::SoundCatalog::import_finished(bytes) {
            Some(catalog) => asset_audio::deposit_saved(path, game, "common census", Ok(catalog)),
            None => return Err(saved),
        }
    } else if let Some(error) = &parsed.sound_err {
        asset_audio::deposit_saved(path, game, "common census", Err(error.clone()));
    }
    let Some(side) = side_ok(&parsed, &rest.materials) else {
        return Err(saved);
    };
    let mut census = side.census(rest);
    if decode {
        replan(&mut census, path, progress);
    }
    Ok(census)
}

struct BuiltSide {
    scripts: crate::ScriptSources,
    scene: asset_world::MapXModelSceneCatalog,
    shared: asset_model::SharedXModelSurfaces,
    anim: asset_anim::PlayerAnimSources,
    lights: Vec<asset_world::CapturedLightDef>,
    pen: weapon_iw4::PenetrationDepthTable,
    pen_loaded: bool,
    lochit: Option<[f32; weapon_iw4::HITLOC_COUNT]>,
    walk: crate::PreparedXModelWalkCensus,
    s1_bytes: usize,
    teams: std::collections::HashMap<String, asset_game::MapTeamSettings>,
    visions: std::collections::BTreeMap<String, Result<asset_world::FilmVision, asset_world::FilmVisionParseError>>,
    tables: Vec<asset_game::CapturedStringTable>,
}

fn side_ok(side: &Side, materials: &MaterialCatalog) -> Option<BuiltSide> {
    let scripts = crate::ScriptSources::import_common(&side.scripts)?;
    let rows = side
        .scene
        .iter()
        .map(|row| {
            let skel = match &row.skel {
                None => None,
                Some(bytes) => Some(std::sync::Arc::new(finished_gun_cache::decode_skel(bytes, materials)?)),
            };
            Some(asset_world::FinishedSceneRow {
                name: row.name.clone(),
                kind: row.kind,
                reason: row.reason.clone(),
                skel,
                zone: row.zone.clone(),
            })
        })
        .collect::<Option<Vec<_>>>()?;
    let scene = asset_world::MapXModelSceneCatalog::import_finished(
        rows,
        (side.phys[0], side.phys[1], side.phys[2]),
    );
    let mut shared = asset_model::SharedXModelSurfaces::default();
    for row in &side.shared {
        let skel = finished_gun_cache::decode_skel(&row.skel, materials)?;
        shared.0.insert(row.name.clone(), (std::sync::Arc::new(skel), row.lod));
    }
    let errors = side
        .anim_errors
        .iter()
        .filter_map(|error| {
            Some(match error.as_str() {
                MULTIPLAYER_ANIMTREE_PATH => MULTIPLAYER_ANIMTREE_PATH,
                PLAYERANIM_SCRIPT_PATH => PLAYERANIM_SCRIPT_PATH,
                PLAYERANIM_TYPES_PATH => PLAYERANIM_TYPES_PATH,
                _ => return None,
            })
        })
        .collect();
    let anim = asset_anim::PlayerAnimSources::from_finished_sources(
        side.anim_atr.clone(),
        side.anim_script.clone(),
        side.anim_types.clone(),
        errors,
    );
    let lights = side
        .lights
        .iter()
        .map(|light| {
            let ns = match light.ns.as_str() {
                "iw4" => asset_core::AssetNamespace::Iw4,
                "t5" => asset_core::AssetNamespace::T5,
                "iw5" => asset_core::AssetNamespace::Iw5,
                "t6" => asset_core::AssetNamespace::T6,
                _ => return None,
            };
            Some(asset_world::CapturedLightDef {
                namespace: ns,
                name: if light.real {
                    asset_core::AssetRef::Real(light.name.clone())
                } else {
                    asset_core::AssetRef::Reference(light.name.clone())
                },
                attenuation_image_name: light.image.clone(),
                attenuation_width: light.width,
                attenuation_sampler: light.sampler,
                lmap_lookup_start: light.lmap,
            })
        })
        .collect::<Option<Vec<_>>>()?;
    let lochit = match &side.lochit {
        None => None,
        Some(row) => Some(row.as_slice().try_into().ok()?),
    };
    let mut visions = std::collections::BTreeMap::new();
    for vision in &side.visions {
        let value = match &vision.ok {
            Some(film) => Ok(film_in(film)),
            None => Err(asset_world::FilmVisionParseError::Decode),
        };
        visions.insert(vision.name.clone(), value);
    }
    Some(BuiltSide {
        scripts,
        scene,
        shared,
        anim,
        lights,
        pen: weapon_iw4::PenetrationDepthTable::from_depths(side.pen),
        pen_loaded: side.pen_loaded,
        lochit,
        walk: crate::PreparedXModelWalkCensus {
            walked_n: side.walked,
            unclassified_n: side.unclassified,
            projectile_names: side.projectile_names.clone(),
        },
        s1_bytes: side.s1_bytes,
        teams: side.teams.iter().cloned().collect(),
        visions,
        tables: side.tables.clone(),
    })
}

fn film_in(film: &FilmDto) -> asset_world::FilmVision {
    asset_world::FilmVision {
        enable: film.enable,
        contrast: film.contrast,
        brightness: film.brightness,
        desaturation: film.desaturation,
        desaturation_dark: film.desaturation_dark,
        invert: film.invert,
        light_tint: film.light_tint,
        medium_tint: film.medium_tint,
        dark_tint: film.dark_tint,
        glow_enable: film.glow_enable,
        glow_radius: film.glow_radius,
        glow_bloom_cutoff: film.glow_bloom_cutoff,
        glow_bloom_desaturation: film.glow_bloom_desaturation,
        glow_bloom_intensity: film.glow_bloom_intensity,
    }
}

impl BuiltSide {
    fn census(self, rest: finished_gun_cache::Restored) -> CommonCensus {
        CommonCensus {
            scripts: self.scripts,
            scene_models: self.scene,
            shared_surfaces: self.shared,
            weapons: rest.weapons,
            cac_tables: self.tables,
            fpv: rest.fpv,
            world_weapons: rest.world,
            projectile_meshes: rest.projectiles,
            xanims: rest.xanims,
            player_anim_sources: self.anim,
            fx: rest.fx,
            fx_models: rest.fx_models,
            tracers: rest.tracers,
            impact_fx: rest.impact,
            material_population: rest.materials,
            light_defs: self.lights,
            report: rest.reports,
            pen_table: self.pen,
            pen_table_loaded: self.pen_loaded,
            lochit_table: self.lochit,
            xmodel_walk: self.walk,
            s1_common_bytes: self.s1_bytes,
            teamsets: self.teams,
            film_visions: self.visions,
            pending_images: None,
        }
    }
}

fn replan(census: &mut CommonCensus, path: &Path, progress: &LoadProgress) {
    let stage = progress.begin_scoped(StageId::Images, "common_mp", None);
    let (_inline, mut plan) = asset_material::material_images::plan_material_color_maps(
        path,
        &mut census.material_population,
        &stage,
        crate::session_load::load_pool(),
    );
    let _ = asset_material::material_images::plan_color_or_2d_for_keys(
        &mut plan,
        &mut census.material_population,
        census.tracers.material_keys(),
        &stage,
        crate::session_load::load_pool(),
    );
    let _ = asset_material::material_images::plan_color_or_2d_for_keys(
        &mut plan,
        &mut census.material_population,
        census.fx.unique_material_keys(),
        &stage,
        crate::session_load::load_pool(),
    );
    stage.done();
    census.pending_images = Some(plan);
}
