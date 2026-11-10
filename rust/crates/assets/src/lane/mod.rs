mod helpers;
mod iw4;
pub use iw4::check_iw4_map;
mod iw5;
// bo2mp: Call of Duty 4 maps (no ZoneGame of their own).
mod bo2_look;
mod bo3_look;
mod bo3_subst; // bo2mp: foreign materials swapped for Black Ops III's own (pool read at load)
mod bo3_vfx; // bo2mp: foreign ambient effects played as Black Ops III's where it has a match
mod carrier_look; // bo2mp: Carrier's dusk look
mod sea_filter; // bo2mp: the sea's specular and distance filtering
mod rt_ambient; // bo2mp: real-time ambient rebake for the BO3 look
mod rt_cache; // bo2mp: the BO3 look's bake, kept on disk
mod cod4;
// bo2mp: World at War maps, through the CoD4 lane's builders.
mod waw;
// bo2mp: Ghosts' guns as guest guns.
mod iw6;
#[allow(dead_code)] // the reticle, model-slot, dual-view and thermal parts wait for a renderer hook
mod iw6_attach; // bo2mp: Ghosts' attachment match codes, reticle and model picks, dual-view scope, thermal (pure)
// bo2mp: Ghosts' multiplayer maps (no ZoneGame of their own).
mod iw6_map;
#[doc(hidden)]
pub use iw6_map::collision_for_tests as ghosts_collision_for_tests;
mod iw6_model_collision; // bo2mp: Ghosts placed models as line-trace collision (bullets, sight)
mod iw6_gun_fx; // bo2mp: Ghosts guns' own impacts (common_mp's impact table) and tracers
mod iw6_fx; // bo2mp: Ghosts maps' createfx (waterfalls, mist, fire, dust)
pub use iw6_fx::harness_views as iw6_fx_harness_views;
mod iw6_grid; // bo2mp: Ghosts maps' own light grid as the light of what has no baked light (switches, off by default)
mod iw6_look; // bo2mp: Ghosts maps under Black Ops III's look: a sky probe, the sky's colours
mod iw6_tone; // bo2mp: Ghosts' own tone mapping (Hable's curve, read from the zone) as the targets of the look
pub(crate) use iw6_map::load_iw6_world;
pub(crate) use iw6_map::{load_iw6_world_for, native_zones as iw6_native_zones};
// bo2mp: Modern Warfare Remastered's materials and models, and its maps
// (no ZoneGame of their own).
mod h1;
mod h1_map;
mod h1_fx; // bo2mp: Remastered maps' createfx (rain, mist, dust, lights)
pub(crate) use h1::native_zones as h1_native_zones;
pub(crate) use h1_map::{load_h1_world, load_h1_world_native};
// bo2mp: Infinite Warfare's maps (no ZoneGame of their own).
#[cfg(feature = "iw7")]
mod iw7;
#[cfg(feature = "iw7")]
pub(crate) use iw7::{iw7_native_on, load_iw7_world, load_iw7_world_for};
// bo2mp: Infinite Warfare's emissive materials' glow.
#[cfg(feature = "iw7")]
mod iw7_glow;
// bo2mp: stand-ins for the games left out of the build (`iw7`, `jup`).
mod dropped;
#[cfg(not(feature = "iw7"))]
pub(crate) use dropped::iw7::*;
#[cfg(not(feature = "jup"))]
pub(crate) use dropped::jup::*;
// bo2mp: Black Ops III's maps (no ZoneGame of their own).
mod t7_light;
mod t7_light_cache;
mod t7_look;
mod t7_map;
mod t7_fx; // bo2mp: Black Ops III maps' placed effects
pub(crate) use t7_map::{load_t7_world, load_t7_world_for, sun_map_ahead as t7_sun_map_ahead};
// bo2mp: Modern Warfare III's (2023) maps, through the Black Ops III
// builder.
mod geom; // bo2mp: triangle/box tests shared by the lanes
pub(crate) use geom::tri_overlaps_box;
mod disk_cache; // bo2mp: the packed cache files every map cache uses
#[cfg(feature = "jup")]
mod jup_cache;
#[cfg(feature = "jup")]
mod jup_clip;
#[cfg(feature = "jup")]
mod jup_light;
#[cfg(feature = "jup")]
mod jup_map;
#[cfg(feature = "jup")]
mod jup_sky;
#[cfg(feature = "jup")]
mod jup_terrain;
#[cfg(feature = "jup")]
mod jup_texsub; // bo2mp: Modern Warfare Remastered / Modern Warfare 2 pictures standing in for MW3's missing ones
#[cfg(feature = "jup")]
pub(crate) use jup_map::{jup_native_opted_in, load_jup_world};
#[cfg(feature = "jup")]
pub use jup_map::{JupPreview, jup_preview};
// bo2mp: Infinite Warfare's Zombies maps as Black Ops II Zombies.
#[cfg(feature = "iw7")]
mod iw7_zombies;
#[cfg(feature = "iw7")]
#[allow(unused_imports)] // (register_foreign_zm_map: for the Black Ops III lane)
pub(crate) use iw7_zombies::{attach_t6_zm_rules, is_iw7_zombies_map, register_foreign_zm_map};
mod t7_zm_scripts; // bo2mp: Black Ops III Zombies maps' own scripts
pub(crate) use iw6::{
    Iw6Guests, build_iw6_guest_guns, iw6_anim_check, iw6_sound_check, prefetch_iw6_guest_guns, walk_iw6_guest_guns,
};
// bo2mp: guest guns' attachments as Create-a-Class offers them.
mod guest_attach;
// bo2mp: Advanced Warfare's guns as guest guns.
mod s1;
pub(crate) use s1::{S1Guests, build_s1_guest_guns, prefetch_s1_guest_guns, s1_anim_check, walk_s1_guest_guns};
// bo2mp: Horizon MW's guns (MW2's remastered for MWR) as guest guns.
mod census_cache;
mod finished_gun_cache;
mod t6_combat_cache;
mod hmw;
mod hmw_fields;
pub(crate) use hmw::{HmwGuests, build_hmw_guest_guns, hmw_anim_check, prefetch_hmw_guest_guns, walk_hmw_guest_guns};
// bo2mp: Infinite Warfare's guns as guest guns.
#[cfg(feature = "iw7")]
mod iw7_gun;
#[cfg(feature = "iw7")]
mod iw7_gun_fields;
#[cfg(feature = "iw7")]
pub(crate) use iw7_gun::{Iw7Guests, build_iw7_guest_guns, iw7_anim_check, iw7_guest_sounds, prefetch_iw7_guest_guns, walk_iw7_guest_guns};
// bo2mp: Call of Duty 4's and World at War's guns as guest guns.
mod legacy_guns;
// bo2mp: Black Ops III's guns as guest guns.
mod t7;
mod t7_gun_cache; // bo2mp: the BO3 guns' converted pictures kept on disk
mod bo3_pool_cache; // bo2mp: the BO3 material pool's colour statistics kept on disk
mod t7_fields;
pub(crate) use t7::{T7Guests, build_t7_guest_guns, build_t7_pack_guns, prefetch_t7_guest_guns, prefetch_t7_pack_guns, t7_anim_check, t7_sound_check, walk_t7_guest_guns, workshop_gun_check};
pub(crate) use t7::pack_gun_idle_tracks as t7_pack_gun_idle_tracks;
// bo2mp: one Modern Warfare III (2023) gun as a guest gun.
mod fx_standin; // bo2mp: Black Ops III's and Modern Warfare Remastered's effects standing in for Modern Warfare III's
#[cfg(feature = "jup")]
mod jup_gun;
#[cfg(feature = "jup")]
mod jup_gun_audit; // bo2mp: offline audit of the guest guns' first-person fit
#[cfg(feature = "jup")]
pub(crate) use jup_gun_audit::run as jup_gun_audit;
#[cfg(feature = "jup")]
pub(crate) mod jup_ops;
// bo2mp: Black Ops' soldiers as the players.
pub(crate) mod t5_soldiers;
#[cfg(feature = "jup")]
pub(crate) use jup_gun::{JupGuests, build_jup_guest_guns, jup_check, jup_guest_rows, prefetch_jup_guest_guns};
#[cfg(feature = "jup")]
pub(crate) use jup_gun::{check_jup_export, export_jup_guns, png_rgba}; // bo2mp: `CodLegacy export-jup`
pub(crate) use legacy_guns::{
    Legacy, LegacyGuests, build_legacy_guest_guns, legacy_common_mp, load_legacy_guests, prefetch_legacy_guest_guns,
};
mod sink;
mod t5;
// bo2zm: Black Ops II.
mod t6;
mod t6_materials;
// bo2mp: BAKE ALL's Black Ops II entry (a map's pictures in one file).
pub(crate) use t6_materials::bake_map_bundle as t6_bake_map_bundle;
// bo2zm M2: guns, effects and sound.
mod t6_m2;
// bo2mp lane C: the multiplayer front end's UI (no map).
pub mod t6_frontend;
mod t6_scripts;

// bo2zm M4: Black Ops II's front end (no map).
pub use t6_m2::{T6Frontend, load_t6_frontend};
pub use t6_materials::mod_picture_rgba;
pub use t6_materials::note_t6_card_held;
pub(crate) use t6_materials::rebuild_card_image;

use std::path::Path;

pub(crate) use helpers::{
    build_iw5_static_model_draw, build_static_model_draw, build_t5_static_model_draw,
};
pub(crate) use sink::{CommonWalkSink, MaterialPopulationSink, ZoneWalkSink};
pub(crate) use t6::{attach_t6_rules, read_t6_rules_zones_ahead, t6_surface_list};
pub(crate) use cod4::load_iw3_world;
pub(crate) use waw::load_t4_world;

use crate::{
    lane_capability::{LaneStatus, PreparedCapability},
    session_load::{PreparedWorld, WorldDrawPolicy},
};
use asset_anim::XAnimBuild;
use asset_core::ZoneGame;
use asset_game::WeaponBuild;
use asset_model::{BodyMeshBuild, FpvMeshBuild, WorldWeaponBuild};
use asset_transport::{LoadProgress, ZoneImage};

#[derive(Clone, Debug)]
pub struct LaneGap {
    pub capability: PreparedCapability,
    pub reason: String,
    pub addr: Option<&'static str>,
}

#[derive(Default)]
pub struct LoadedWorld {
    pub scripts: crate::ScriptSources,
    pub world: PreparedWorld,
    /// What the map zone itself captured. The local material indices in
    /// `world` are indices into this pool until the match finalizes one.
    pub materials: asset_material::MaterialCatalog,
    pub collision: Option<asset_world::ClipCollision>,
    pub spawns: Vec<asset_world::SpawnPoint>,
    pub bodies: BodyMeshBuild,
    pub fpv_meshes: FpvMeshBuild,
    pub xanims: XAnimBuild,
    /// bo2zm: weapons the map zone itself defines (Black Ops II maps carry
    /// their own; other games take theirs from common zones).
    pub weapons: WeaponBuild,
    /// bo2zm: the map's own weapons' third-person models.
    pub world_weapons: WorldWeaponBuild,
    /// bo2zm: tracers the map zone itself defines.
    pub tracers: asset_game::TracerCatalog,
    /// bo2mp: the bullet penetration depths the map's zones carry (Black
    /// Ops II has no common zones read yet; its table comes this way).
    pub pen_table: Option<weapon_iw4::PenetrationDepthTable>,
    pub facts: crate::MapFacts,
    /// Bytes the zone arenas held while the walk read them. The arenas
    /// themselves die with the walk; only their size travels.
    pub arena_bytes: usize,
    pub sound: Option<Result<asset_audio::SoundCatalog, String>>,
    pub report: Vec<String>,
    pub gaps: Vec<LaneGap>,
}

impl LoadedWorld {
    pub fn with_gap(
        policy: WorldDrawPolicy,
        capability: PreparedCapability,
        reason: impl Into<String>,
        addr: Option<&'static str>,
    ) -> Self {
        let reason = reason.into();
        Self {
            world: PreparedWorld {
                policy,
                ..Default::default()
            },
            report: vec![reason.clone()],
            gaps: vec![LaneGap {
                capability,
                reason,
                addr,
            }],
            ..Default::default()
        }
    }

    pub fn push_gap(
        &mut self,
        capability: PreparedCapability,
        reason: impl Into<String>,
        addr: Option<&'static str>,
    ) {
        self.gaps.push(LaneGap {
            capability,
            reason: reason.into(),
            addr,
        });
    }
}

#[derive(Default)]
pub struct CommonCensus {
    pub scripts: crate::ScriptSources,
    pub scene_models: asset_world::MapXModelSceneCatalog,
    pub shared_surfaces: asset_model::SharedXModelSurfaces,
    pub weapons: WeaponBuild,

    pub cac_tables: Vec<asset_game::CapturedStringTable>,
    pub fpv: FpvMeshBuild,
    pub world_weapons: WorldWeaponBuild,

    pub projectile_meshes: asset_model::ProjectileMeshBuild,
    pub xanims: XAnimBuild,
    pub player_anim_sources: asset_anim::PlayerAnimSources,
    pub fx: asset_game::FxCatalog,
    pub fx_models: asset_game::FxModelCatalog,

    pub tracers: asset_game::TracerCatalog,
    pub impact_fx: Option<asset_game::OwnedFxImpactTable>,

    pub material_population: asset_material::MaterialCatalog,

    pub light_defs: Vec<asset_world::CapturedLightDef>,
    pub report: Vec<String>,

    pub pen_table: weapon_iw4::PenetrationDepthTable,
    pub pen_table_loaded: bool,
    pub lochit_table: Option<[f32; weapon_iw4::HITLOC_COUNT]>,

    pub xmodel_walk: crate::PreparedXModelWalkCensus,

    /// Bytes the common_mp arenas held while the walk read them; the arenas
    /// themselves do not outlive it.
    pub s1_common_bytes: usize,

    pub teamsets: std::collections::HashMap<String, asset_game::MapTeamSettings>,
    pub film_visions: std::collections::BTreeMap<
        String,
        Result<asset_world::FilmVision, asset_world::FilmVisionParseError>,
    >,

    pub pending_images: Option<asset_material::material_images::ImageDemandPlan>,
}

pub struct MaterialPopulation {
    pub materials: asset_material::MaterialCatalog,
    pub light_defs: Vec<asset_world::CapturedLightDef>,
    pub walked: usize,
    pub report: Vec<String>,
    pub cac_tables: Vec<asset_game::CapturedStringTable>,
    pub scripts: crate::ScriptSources,
}

impl Default for MaterialPopulation {
    fn default() -> Self {
        Self {
            materials: asset_material::MaterialCatalog::default(),
            light_defs: Vec::new(),
            walked: 0,
            report: Vec::new(),
            cac_tables: Vec::new(),
            scripts: crate::ScriptSources::default(),
        }
    }
}

pub trait ZoneLane: Send + Sync {
    fn game(&self) -> ZoneGame;
    fn capabilities(&self) -> &'static [(PreparedCapability, LaneStatus)];

    fn load_world(
        &self,
        path: &Path,
        image: &ZoneImage,
        progress: &LoadProgress,

        shared_surfaces: asset_model::SharedXModelSurfaces,

        material_seed: asset_material::MaterialCatalog,
        common_film_visions: &mut std::collections::BTreeMap<
            String,
            Result<asset_world::FilmVision, asset_world::FilmVisionParseError>,
        >,
    ) -> LoadedWorld;

    /// Black Ops II's common zone is still unread (`false`). Every other lane
    /// caches the census (`true`). A new `ZoneGame` has to pick one: the
    /// default is to cache, and `walk_common_mp` is the only walk.
    fn caches_common_weapons(&self) -> bool {
        true
    }

    fn walk_common_mp(
        &self,
        path: &Path,
        image: &ZoneImage,
        progress: &LoadProgress,
        decode_color_maps: bool,
        material_seed: asset_material::MaterialCatalog,
    ) -> CommonCensus;

    fn load_common_mp(
        &self,
        path: &Path,
        image: &ZoneImage,
        progress: &LoadProgress,
        decode_color_maps: bool,
        material_seed: asset_material::MaterialCatalog,
    ) -> CommonCensus {
        if !self.caches_common_weapons() {
            return self.walk_common_mp(path, image, progress, decode_color_maps, material_seed);
        }
        census_cache::load(
            self.game(),
            path,
            image,
            progress,
            decode_color_maps,
            material_seed,
            |path, image, progress, decode, seed| {
                self.walk_common_mp(path, image, progress, decode, seed)
            },
        )
    }

    fn load_material_population(
        &self,
        path: &Path,
        image: &ZoneImage,
        progress: &LoadProgress,
        material_seed: asset_material::MaterialCatalog,
    ) -> MaterialPopulation;
}

pub fn lane(game: ZoneGame) -> &'static dyn ZoneLane {
    match game {
        ZoneGame::Iw4 => &iw4::Iw4Lane,
        ZoneGame::T5 => &t5::T5Lane,
        ZoneGame::Iw5 => &iw5::Iw5Lane,
        ZoneGame::T6 => &t6::T6Lane,
    }
}

pub const LANE_GAPS: &[&str] = &[
    "assets::lane::iw4::load_world/zone_header",
    "assets::lane::iw4::load_world/zone_arenas",
    "assets::lane::iw4::load_world/no_gfx_world",
    "assets::lane::iw4::load_world/world_mesh",
    "assets::lane::t5::load_world/zone_header",
    "assets::lane::t5::load_world/zone_arenas",
    "assets::lane::t5::load_world/no_gfx_world",
    "assets::lane::t5::load_world/world_mesh",
    "assets::lane::iw5::load_world/zone_header",
    "assets::lane::iw5::load_world/zone_arenas",
    "assets::lane::iw5::load_world/no_gfx_world",
    "assets::lane::iw5::load_world/world_mesh",
    "assets::lane::t6::load_world/walk",
    "assets::lane::t6::load_world/no_world",
    "assets::lane::t6::load_world/world_mesh",
    "assets::session_load::load_prepared_match/zone_open",
];
