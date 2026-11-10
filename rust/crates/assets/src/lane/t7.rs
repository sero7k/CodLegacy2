//! bo2mp: Call of Duty: Black Ops III's guns as guest guns in a Black Ops
//! II match (`bo3_<gun>_mp`) and in Zombies (`<gun>_upgraded` for
//! Pack-a-Punch).
//!
//! Every Black Ops III gun is in `zone/core_common.ff` (campaign `_cp`,
//! multiplayer `_mp` and zombies `_zm` variants sharing models, animations
//! and pictures); streamed meshes and pictures are in the `.xpak` packages
//! beside it. `fastfile_t7`'s walk reads the zone (it stops near the end,
//! at the character customization tables, after every gun); this turns
//! what it kept into the engine's catalogs the way Black Ops II's own guns
//! go in, Black Ops III being Black Ops II's engine grown:
//!
//! - models: the densest LOD (the model's list runs coarsest first), its
//!   vertices as Black Ops II's packed ones, its skin weights as BO2's
//!   blend buckets (vertices ordered one to four bones), its skeleton (the
//!   rest pose's quaternions are stored as halves in Black Ops III);
//! - materials: the colour map, the normal map (BC7 with x, y in red and
//!   green: turned into BC5 for BO2's draw), the specular colour with the
//!   gloss in its alpha (one BC3: BO2's specular map);
//! - animations: Black Ops II's part types and packing, but every
//!   quaternion stored as four halves (turned into BO2's 16-bit
//!   fractions), the bone counts 16-bit; notetracks are (kind, parameter)
//!   pairs (`sound`, `fly_arak_mag_out`), turned into Black Ops II's
//!   inline sound and rumble notes;
//! - weapons: Black Ops III's numbers copied into Black Ops II's weapon
//!   structs by field name (`t7_fields`) and read by Black Ops II's own
//!   capture (`WeaponCatalog::capture_t6`), the fire type and player
//!   animation type renumbered.
//!
//! Effects and tracers are Black Ops II's (the weapon registry gives each
//! Black Ops III gun a Black Ops II gun's of its class); sounds are Black
//! Ops III's own (`asset_audio::bo3_weapon_sounds`). Arms are Black Ops
//! II's.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use asset_anim::XAnimBuild;
use asset_core::{AssetEdge, AssetNamespace, AssetRef, ZoneOwner};
use asset_game::{WeaponBuild, WeaponCatalog};
use asset_material::{
    AuthoredImage, AuthoredMaterial, MaterialCatalog, MaterialTextureBinding, TS_COLOR_MAP, TS_NORMAL_MAP,
    TS_SPECULAR_MAP,
};
use asset_model::{FpvMeshBuild, WorldWeaponBuild};
use bo2_profile::guest::WorkshopPack;
use fastfile_t6::layout::{WeaponDef as d6, WeaponVariantDef as v6};
use fastfile_t7::texture::{self, ImageLevels};
use fastfile_t7::walk::capture::{AnimRec, Capture, FieldValue, ModelRec, WeaponRec};
use fastfile_t7::xpak::XpakSet;

use super::t7_fields::T7_TO_T6;
use super::t7_map::{f16_bits, t6_unit_vec, t7_draw};

const NS: AssetNamespace = AssetNamespace::T7;

/// `R_HashString` of the texture slots Black Ops II's lit model draw reads
/// (Black Ops III names its colour and normal maps the same way).
const COLOR_MAP_HASH: u32 = 0xa0ab_1041;
const NORMAL_MAP_HASH: u32 = 0x59d3_0d0f;
const SPECULAR_MAP_HASH: u32 = 0x34ec_ccb3;
/// Black Ops III's specular colour and gloss maps (`i_..._s`, `i_..._g`).
const T7_SPECULAR_HASH: u32 = 0xec44_3804;
const T7_GLOSS_HASH: u32 = 0x6d0a_6c98;
/// Black Ops III's emissive map (a lit-emissive tritium dot's picture).
const T7_EMISSIVE_HASH: u32 = 0x3461_4347;

/// The largest level kept of a gun's pictures (first person), and of the
/// third-person models' (`BO2MP_BO3_TEXTURE=<size>`, or `BO2MP_GUN_TEXTURE`,
/// changes the first). Black Ops III authors its gun bodies (receiver,
/// metal and plastic) at 2048 and a viewmodel fills the screen: at 1024 they
/// were soft. Measured on core_common: the first-person pictures of all 30
/// guns come to about +10% memory at 2048 over 1024.
const FIRST_PERSON_MAX: u32 = 2048;
/// bo2mp: the same for the Workshop packs' guns (`BO2MP_WORKSHOP_TEXTURE`):
/// the six packs' 242 guns are about 8400 pictures, which at 1024 took
/// 8.8 GB to build (measured, `t7_pack_guns --build`); at 512 a view model
/// shares its world model's pictures.
const WORKSHOP_FIRST_PERSON_MAX: u32 = 512;

/// (Nanoseconds spent reading pictures' levels, on every thread together;
/// and making the pictures `link_image` found not made ahead.)
static PROBE_LEVELS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
static PROBE_MAKE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
const WORLD_MAX: u32 = 512;

/// Black Ops III's multiplayer guns (`<gun>_mp` in core_common), without
/// the dual-optic, dual-wield and left-hand variants, the specialist
/// weapons, scorestreaks and equipment. Create-a-Class's names are in
/// `bo2_profile::guest::BO3`.
pub(crate) const BO3_GUNS: &[&str] = &[
    // Assault rifles.
    "ar_standard", "ar_accurate", "ar_fastburst", "ar_cqb", "ar_damage", "ar_longburst", "ar_marksman",
    // Submachine guns.
    "smg_standard", "smg_versatile", "smg_fastfire", "smg_longrange", "smg_burst", "smg_capacity",
    // Light machine guns.
    "lmg_light", "lmg_heavy", "lmg_cqb", "lmg_slowfire",
    // Sniper rifles.
    "sniper_powerbolt", "sniper_fastbolt", "sniper_fastsemi", "sniper_chargeshot",
    // Shotguns.
    "shotgun_pump", "shotgun_semiauto", "shotgun_precision", "shotgun_fullauto",
    // Pistols.
    "pistol_standard", "pistol_burst", "pistol_fullauto",
    // Launchers.
    "launcher_standard", "launcher_lockonly",
];

/// Black Ops III's guns, ready to join the match's catalogs.
pub(crate) struct T7Guests {
    pub weapons: WeaponBuild,
    pub fpv: FpvMeshBuild,
    pub world_guns: WorldWeaponBuild,
    pub xanims: XAnimBuild,
    pub material_seed: MaterialCatalog,
    pub report: Vec<String>,
}

impl T7Guests {
    pub(crate) fn empty(material_seed: MaterialCatalog, report: Vec<String>) -> Self {
        Self {
            weapons: WeaponBuild::default(),
            fpv: FpvMeshBuild::default(),
            world_guns: WorldWeaponBuild::default(),
            xanims: XAnimBuild::default(),
            material_seed,
            report,
        }
    }
}

/// Black Ops III's `zone` folder (the one with `core_common.ff`), if it is
/// installed.
pub(crate) fn bo3_zone_dir() -> Result<PathBuf, String> {
    let root = asset_transport::games_root_from_env()?;
    let zone = asset_transport::find_zone_file_version(&root, "core_common", asset_transport::T7_ZONE_VERSION)
        .map_err(|e| format!("Black Ops III is not installed (no core_common.ff in its format: {e})"))?;
    zone.path.parent().map(Path::to_path_buf).ok_or_else(|| "core_common.ff has no folder".into())
}

/// core_common, read ahead of the build.
pub(crate) enum T7Prefetch {
    Off(Vec<String>),
    Ready(Box<T7Zone>),
}

pub(crate) struct T7Zone {
    dir: PathBuf,
    /// The zone read (core_common, or a Workshop pack's).
    path: PathBuf,
    /// bo2mp: the Workshop weapon pack it is, `None` for core_common.
    pack: Option<&'static WorkshopPack>,
    zone: fastfile_t7::Zone,
    cap: Capture,
    report: Vec<String>,
    started: std::time::Instant,
}

/// bo2mp: reads Black Ops III's core_common for its guns (`wanted`: Black
/// Ops II rules with guest guns on), off the match's critical path.
pub(crate) fn prefetch_t7_guest_guns(wanted: bool) -> T7Prefetch {
    if !wanted {
        return T7Prefetch::Off(vec!["t7 guest guns: off".into()]);
    }
    let dir = match bo3_zone_dir() {
        Ok(dir) => dir,
        Err(e) => return T7Prefetch::Off(vec![format!("t7 guest guns: {e}")]),
    };
    let started = std::time::Instant::now();
    let path = dir.join("core_common.ff");
    let zone = match fastfile_t7::Zone::open(&path) {
        Ok(z) => z,
        Err(e) => return T7Prefetch::Off(vec![format!("t7 guest guns: {}: {e}", path.display())]),
    };
    let w = fastfile_t7::walk::walk_with(&zone, fastfile_t7::walk::Options { keep_anims: true, ..Default::default() });
    let report = vec![format!(
        "t7 guest guns: walked {} assets of {} ({} weapons, {} models, {} meshes, {} animations, {} materials, {} images; {}) [{:.1} s]",
        w.walked.len(),
        path.display(),
        w.capture.weapons.len(),
        w.capture.models.len(),
        w.capture.meshes.len(),
        w.capture.anims.len(),
        w.capture.materials.len(),
        w.capture.images.len(),
        match &w.stop {
            fastfile_t7::walk::Stop::Done { .. } => "to its end".to_owned(),
            other => format!("stopped at {other:?}"),
        },
        started.elapsed().as_secs_f64()
    )];
    T7Prefetch::Ready(Box::new(T7Zone { dir, path, pack: None, zone, cap: w.capture, report, started }))
}

/// bo2mp: the Black Ops III Workshop weapon packs to read (`wanted`: guest
/// guns on): those installed, with Black Ops III (their guns pose its arms
/// and hang its stock attachments, whose streamed data is in its packages).
/// `BO2MP_WORKSHOP_GUNS=0` reads none; `BO2MP_WORKSHOP_GUNS=cw,ol2` only
/// the packs named (by their tag, the roster's first word, or Workshop id).
fn workshop_packs_wanted(wanted: bool, filter: Option<&str>) -> (Vec<(&'static WorkshopPack, PathBuf)>, Vec<String>) {
    let mut report = Vec::new();
    if !wanted {
        return (Vec::new(), report);
    }
    let setting = filter.map_or_else(|| std::env::var("BO2MP_WORKSHOP_GUNS").unwrap_or_default(), str::to_owned);
    let setting = setting.trim().to_ascii_lowercase();
    if setting == "0" {
        report.push("t7 workshop guns: off (BO2MP_WORKSHOP_GUNS=0)".into());
        return (Vec::new(), report);
    }
    if let Err(e) = bo3_zone_dir() {
        report.push(format!("t7 workshop guns: none, they need Black Ops III: {e}"));
        return (Vec::new(), report);
    }
    let mut out = Vec::new();
    for pack in &bo2_profile::guest::WORKSHOP_PACKS {
        let tag = pack.guns.first().and_then(|g| g.gun.split('_').next()).unwrap_or("");
        if !setting.is_empty() && !setting.split(',').map(str::trim).any(|s| s == tag || s == pack.workshop_id || s == pack.game) {
            continue;
        }
        match asset_transport::workshop::bo3_mod(pack.workshop_id).and_then(|m| m.zone(pack.zone)) {
            Some(zone) => out.push((pack, zone.to_path_buf())),
            None => report.push(format!("t7 workshop guns: {} not installed (Workshop item {}, {}.ff)", pack.title, pack.workshop_id, pack.zone)),
        }
    }
    (out, report)
}

/// bo2mp: reads the Workshop weapon packs' zones (`workshop_packs_wanted`),
/// each walked as core_common is; a pack zone is a few hundred MB, its
/// streamed data (up to 3 GB) is read only by the entries the guns name.
pub(crate) fn prefetch_t7_pack_guns(wanted: bool) -> (Vec<T7Prefetch>, Vec<String>) {
    prefetch_packs(wanted, None)
}

/// `prefetch_t7_pack_guns` with the packs named by `filter` (else by
/// `BO2MP_WORKSHOP_GUNS`).
fn prefetch_packs(wanted: bool, filter: Option<&str>) -> (Vec<T7Prefetch>, Vec<String>) {
    let (packs, mut report) = workshop_packs_wanted(wanted, filter);
    let mut out = Vec::new();
    for (pack, path) in packs {
        let started = std::time::Instant::now();
        let zone = match fastfile_t7::Zone::open(&path) {
            Ok(z) => z,
            Err(e) => {
                report.push(format!("t7 workshop guns: {}: {}: {e}", pack.title, path.display()));
                continue;
            }
        };
        let w = fastfile_t7::walk::walk_with(&zone, fastfile_t7::walk::Options { keep_anims: true, ..Default::default() });
        let walked = format!(
            "{}: walked {} assets of {} ({} weapons, {} models, {} animations, {} materials, {} images; {}) [{:.1} s]",
            pack_label(pack),
            w.walked.len(),
            path.display(),
            w.capture.weapons.len(),
            w.capture.models.len(),
            w.capture.anims.len(),
            w.capture.materials.len(),
            w.capture.images.len(),
            match &w.stop {
                fastfile_t7::walk::Stop::Done { .. } => "to its end".to_owned(),
                other => format!("stopped at {other:?}"),
            },
            started.elapsed().as_secs_f64()
        );
        let dir = path.parent().map(Path::to_path_buf).unwrap_or_default();
        out.push(T7Prefetch::Ready(Box::new(T7Zone { dir, path, pack: Some(pack), zone, cap: w.capture, report: vec![walked], started })));
    }
    (out, report)
}

/// The report lines' name for a pack's build (`t7 workshop guns (Cold War)`).
fn pack_label(pack: &WorkshopPack) -> String {
    format!("t7 workshop guns ({})", pack.title)
}

/// bo2mp: `BO2MP_T7_PACK_FIX=0` restores the pack guns' old behaviour (the
/// aiming overlay models and the `_sco_` scopes of CoD Online treated as any
/// attachment, part bits least-significant first).
fn pack_fix_on() -> bool {
    std::env::var("BO2MP_T7_PACK_FIX").map_or(true, |v| v.trim() != "0")
}

/// bo2mp: the bones a model's hide `tags` take with them when posed: each
/// tag's bone and every bone under it (`parent_list` holds each non-root
/// bone's step back to its parent, parents first). By index.
fn hidden_bone_set(bone_names: &[String], roots: usize, parent_list: &[u8], tags: &[String]) -> Vec<u16> {
    let n = bone_names.len();
    let mut hidden = vec![false; n];
    for b in 0..n {
        let own = tags.iter().any(|t| t.eq_ignore_ascii_case(&bone_names[b]));
        let parent = (b >= roots)
            .then(|| parent_list.get(b - roots).copied())
            .flatten()
            .filter(|&step| step > 0)
            .and_then(|step| b.checked_sub(usize::from(step)));
        hidden[b] = own || parent.is_some_and(|p| hidden[p]);
    }
    (0..n).filter(|&b| hidden[b]).map(|b| b as u16).collect()
}

/// bo2mp: `BO2MP_T7_HIDE_GUARD=0` keeps a gun's hide tags even where they
/// would collapse most of its vertices.
fn hide_guard_on() -> bool {
    std::env::var("BO2MP_T7_HIDE_GUARD").map_or(true, |v| v.trim() != "0")
}

/// A bone's bit in a surface's part bits: Black Ops II counts a bone from
/// the top bit of its word (`xmodel_no_scale_bit`, `set_hide_part_bit`);
/// `msb` false is the old bottom-up order (`BO2MP_T7_PACK_FIX=0`).
fn part_bit(bone: u16, msb: bool) -> u32 {
    if msb { 0x8000_0000u32 >> (bone % 32) } else { 1 << (bone % 32) }
}

/// bo2mp: whether an attached view model is the gun's aiming overlay: the
/// model Black Ops III shows through its scope while aimed (a ballistic
/// reticle, a lens cover, a stencil disc, all sized to fill the eye's view
/// from a hand's length), which a Black Ops II match replaces with its own
/// scope. `_ads_view` is Black Ops III's own name for it; a Workshop pack's
/// (`pack`) names it `<gun>_ads` (CoD Online's `codol_sco_sniper_cheytac_ads`:
/// 866 vertices in three surfaces, the reticle disc and the stencil 17
/// vertices each). Left on the gun it drew as the huge flat octagon in front
/// of the camera.
fn is_ads_overlay(name: &str, pack: bool) -> bool {
    name.ends_with("_ads_view") || (pack && pack_fix_on() && name.ends_with("_ads"))
}

/// bo2mp: whether an attached view model is a sniper's own scope: Black Ops
/// III's and Modern Warfare's are `..._scope`, CoD Online's `codol_sco_<gun>`.
fn is_own_scope_model(name: &str, pack: bool) -> bool {
    name.contains("_scope") || (pack && pack_fix_on() && name.contains("_sco_"))
}

/// bo2mp: the Workshop packs' guns from what `prefetch_t7_pack_guns` read,
/// one build per pack, their materials linked into `material_seed` (handed
/// back with the builds). The packs share one set of packages: theirs, then
/// Black Ops III's.
pub(crate) fn build_t7_pack_guns(
    prefetch: (Vec<T7Prefetch>, Vec<String>),
    material_seed: MaterialCatalog,
    progress: &asset_transport::LoadProgress,
) -> (Vec<T7Guests>, MaterialCatalog, Vec<String>) {
    let (zones, mut report) = prefetch;
    let zones: Vec<T7Zone> = zones
        .into_iter()
        .filter_map(|p| match p {
            T7Prefetch::Ready(z) => Some(*z),
            T7Prefetch::Off(r) => {
                report.extend(r);
                None
            }
        })
        .collect();
    if zones.is_empty() {
        return (Vec::new(), material_seed, report);
    }
    let stage = progress.begin_scoped(asset_transport::StageId::CommonAssets, "t7_workshop_weapons", None);
    let mut paks = XpakSet::default();
    for z in &zones {
        let Some(m) = z.pack.and_then(|p| asset_transport::workshop::bo3_mod(p.workshop_id)) else { continue };
        for x in &m.xpaks {
            match fastfile_t7::xpak::Xpak::open(x) {
                Ok(p) => paks.paks.push(p),
                Err(e) => report.push(format!("t7 workshop guns: {}: {e}", x.display())),
            }
        }
    }
    match bo3_zone_dir().map_err(|e| e.to_string()).and_then(|d| XpakSet::open_dir(&d).map_err(|e| e.to_string())) {
        Ok((bo3, bad)) => {
            if !bad.is_empty() {
                report.push(format!("t7 workshop guns: {} of Black Ops III's packages unreadable", bad.len()));
            }
            paks.paks.extend(bo3.paks);
        }
        Err(e) => report.push(format!("t7 workshop guns: Black Ops III's packages: {e}")),
    }
    let mut seed = material_seed;
    let mut out = Vec::new();
    for z in zones {
        let (mut guests, back) = load_t7_guests(z, seed, Some(paks));
        paks = back;
        seed = std::mem::take(&mut guests.material_seed);
        out.push(guests);
    }
    stage.done();
    (out, seed, report)
}

/// bo2mp: the match's Black Ops III guest guns from what
/// `prefetch_t7_guest_guns` read, their materials linked into
/// `material_seed`.
pub(crate) fn build_t7_guest_guns(
    prefetch: T7Prefetch,
    material_seed: MaterialCatalog,
    progress: &asset_transport::LoadProgress,
) -> T7Guests {
    let zone = match prefetch {
        T7Prefetch::Off(report) => return T7Guests::empty(material_seed, report),
        T7Prefetch::Ready(zone) => zone,
    };
    let stage = progress.begin_scoped(asset_transport::StageId::CommonAssets, "t7_guest_weapons", None);
    let (guests, _) = load_t7_guests(*zone, material_seed, None);
    stage.done();
    guests
}

/// bo2mp: read and built in one go (`CodLegacy check-guns`).
pub(crate) fn walk_t7_guest_guns(
    wanted: bool,
    material_seed: MaterialCatalog,
    progress: &asset_transport::LoadProgress,
) -> T7Guests {
    build_t7_guest_guns(prefetch_t7_guest_guns(wanted), material_seed, progress)
}

/// bo2mp: Black Ops III's world materials as a substitution pool
/// (`bo3_subst`): its shared zones that hold surface materials
/// (`mp_common`, `core_common`) are read and walked, every material whose
/// name or colour map's name has a surface class and whose draw is a plain
/// opaque lit one becomes a candidate (with its colour map's stats), and
/// `f` chooses among them; the ones it asks to link are made through the
/// same `Builder` the guns use (colour, normal as BC5, specular with gloss
/// as BC3) into `catalog`. Returns the catalog and `f`'s result with where
/// the pool came from, or why there is none (no install, no zone read).
pub(crate) fn bo3_pool_run<R>(
    catalog: MaterialCatalog,
    max: u32,
    f: impl FnOnce(&mut dyn super::bo3_subst::Pool) -> R,
) -> (MaterialCatalog, Result<(R, String), String>) {
    use super::bo3_subst::{self, PoolItem};
    let dir = match bo3_zone_dir() {
        Ok(dir) => dir,
        Err(e) => return (catalog, Err(e)),
    };
    let started = std::time::Instant::now();
    let mut loaded: Vec<(String, PathBuf, fastfile_t7::Zone, Capture)> = Vec::new();
    let mut notes = Vec::new();
    // (The world's own materials live in the maps' zones; the shared zones
    // hold props, weapons and characters: on the install measured, 3 of
    // 10344 materials of mp_common and core_common had a surface class.)
    let stems: Vec<String> = std::fs::read_dir(&dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| {
            let p = e.path();
            (p.extension().is_some_and(|x| x == "ff")).then(|| p.file_stem()?.to_str().map(str::to_owned))?
        })
        .collect();
    let budget = std::env::var("BO2MP_BO3_MAT_BUDGET").ok().and_then(|v| v.trim().parse::<f32>().ok()).unwrap_or(150.0);
    for name in pool_zones(&stems, std::env::var("BO2MP_BO3_MAT_ZONES").ok().as_deref()) {
        if !loaded.is_empty() && started.elapsed().as_secs_f32() > budget {
            notes.push(format!("{name}: left out, the {budget:.0}s budget (BO2MP_BO3_MAT_BUDGET) was used"));
            continue;
        }
        let path = dir.join(format!("{name}.ff"));
        match fastfile_t7::Zone::open(&path) {
            Ok(zone) => {
                let w = fastfile_t7::walk::walk(&zone);
                loaded.push((name.to_owned(), path, zone, w.capture));
            }
            Err(e) => notes.push(format!("{name}: {e}")),
        }
    }
    if loaded.is_empty() {
        return (catalog, Err(format!("no mp_common.ff or core_common.ff could be read in {} {notes:?}", dir.display())));
    }
    struct Pool<'a> {
        builders: Vec<Builder<'a>>,
        items: Vec<PoolItem>,
        origin: Vec<(usize, String)>,
        catalog: MaterialCatalog,
        max: u32,
    }
    impl bo3_subst::Pool for Pool<'_> {
        fn items(&self) -> &[PoolItem] {
            &self.items
        }
        fn link(&mut self, i: usize) -> Option<Vec<MaterialTextureBinding>> {
            let (bi, name) = self.origin.get(i)?.clone();
            let b = &mut self.builders[bi];
            // (One catalog for all the zones' builders, handed to each in turn.)
            b.catalog = std::mem::take(&mut self.catalog);
            let slot = b.material(&name, self.max);
            self.catalog = std::mem::take(&mut b.catalog);
            self.catalog.materials.get(slot?).map(|m| m.textures.clone())
        }
    }
    let mut pool = Pool {
        builders: loaded
            .iter()
            .map(|(_, path, zone, cap)| Builder {
                zone,
                cap,
                paks: XpakSet::open_dir(&dir).map(|(p, _)| p).unwrap_or_default(),
                catalog: MaterialCatalog::default(),
                owner: ZoneOwner::from_zone_path(path),
                materials: HashMap::new(),
                images: HashMap::new(),
                image_failures: Vec::new(),
                decoded: 0,
                ready: HashMap::new(),
                pic_cache: None,
                pic_cache_hits: 0,
            })
            .collect(),
        items: Vec::new(),
        origin: Vec::new(),
        catalog,
        max,
    };
    let mut seen = BTreeSet::new();
    let mut per_zone = Vec::new();
    let cache_on = super::bo3_look::game_on("BO2MP_BO3_MAT_CACHE", true);
    for (bi, (zone_name, _, zone, cap)) in loaded.iter().enumerate() {
        let before = pool.items.len();
        // (The colour statistics of the zone's candidates, kept on disk
        // from an earlier start: only what it lacks is measured.)
        let cache_key = cache_on.then(|| super::bo3_pool_cache::key_for(&dir, zone_name)).flatten();
        let mut kept = cache_key.as_deref().map(super::bo3_pool_cache::load).unwrap_or_default();
        let (hits, mut measured) = (kept.len(), 0usize);
        // (Why a material is not a candidate: no colour map, no class, a draw
        // that cannot take another texture.)
        let (mut no_colour, mut no_class, mut not_plain) = (0usize, 0usize, 0usize);
        let mut unread = BTreeMap::<String, usize>::new();
        // (The map's collision says what each material's surface is.)
        let surfaces: HashMap<String, u32> = cap
            .world
            .clip_maps
            .first()
            .map(|c| c.materials.iter().map(|(n, flags, _)| (n.trim_start_matches(',').to_ascii_lowercase(), (*flags as u32 >> 20) & 0x3f)).collect())
            .unwrap_or_default();
        for m in &cap.materials {
            if m.name.starts_with(',') || !seen.insert(m.name.to_ascii_lowercase()) {
                continue;
            }
            let colour = m.textures.iter().filter(|(_, n)| !n.starts_with(',') && !n.starts_with('$')).find(|(h, n)| slot_kind(*h, n) == Some(Kind::Colour)).map(|(_, n)| n.clone());
            let Some(colour) = colour else {
                no_colour += 1;
                continue;
            };
            if !bo3_subst::generic_world(&m.name, &colour) {
                no_class += 1;
                continue;
            }
            let surface = surfaces.get(&m.name.to_ascii_lowercase()).copied();
            let Some((class, name_conf)) = bo3_subst::class_of_bo3(surface, &[&m.name, &colour], 0.5) else {
                no_class += 1;
                continue;
            };
            // (Only plain opaque lit materials: a decal, a blend or a
            // cut-out brings its own geometry's UVs.)
            let draw = t7_draw(&m.name, m.techset.as_deref().unwrap_or("").trim_start_matches(','), true);
            if draw.unlit || draw.alpha_test || draw.effect || draw.cloud || draw.water || draw.polygon_offset > 0 || draw.blend != asset_core::T6Blend::Opaque {
                not_plain += 1;
                continue;
            }
            let item = match kept.get(&m.name) {
                Some(item) => item.clone(),
                None => {
                    measured += 1;
                    let item = match levels_of(cap, &zone.image, &pool.builders[bi].paks, &colour, 32) {
                        Ok(levels) => match bo3_subst::stats_of_chain(levels.format, levels.width, levels.height, levels.levels, &levels.data) {
                            Some(stats) => Some(PoolItem { name: m.name.clone(), class, name_conf, stats }),
                            None => {
                                *unread.entry(format!("format {} not decoded", levels.format)).or_default() += 1;
                                None
                            }
                        },
                        Err(e) => {
                            *unread.entry(e.chars().take(70).collect::<String>()).or_default() += 1;
                            None
                        }
                    };
                    kept.insert(m.name.clone(), item.clone());
                    item
                }
            };
            let Some(item) = item else { continue };
            pool.items.push(item);
            pool.origin.push((bi, m.name.clone()));
        }
        let cached = match (&cache_key, measured) {
            (Some(key), n) if n > 0 => match super::bo3_pool_cache::store(key, &kept) {
                Ok(()) => format!(", stats cache: {hits} read, {n} measured and kept"),
                Err(e) => format!(", stats cache: {hits} read, {n} measured, not kept ({e})"),
            },
            (Some(_), _) => format!(", stats cache: all {hits} read"),
            (None, _) => ", stats cache off".to_owned(),
        };
        per_zone.push(format!("{zone_name} {} of {} materials ({} with a collision surface{cached}; left out: {no_colour} without a colour map, {no_class} without a class, {not_plain} not plain opaque lit; colour map not read: {:?})", pool.items.len() - before, cap.materials.len(), surfaces.len(), unread.iter().take(4).collect::<Vec<_>>()));
    }
    per_zone.extend(notes);
    let read_secs = started.elapsed().as_secs_f32();
    let result = f(&mut pool);
    let linked: usize = pool.builders.iter().map(|b| b.materials.values().filter(|m| m.is_some()).count()).sum();
    let source = format!("Black Ops III {} [{} linked, read {read_secs:.1}s, total {:.1}s]", per_zone.join(", "), linked, started.elapsed().as_secs_f32());
    let Pool { catalog, .. } = pool;
    (catalog, Ok((result, source)))
}

/// Which Black Ops III zones the substitution pool reads, from the `.ff`
/// stems in the install's zone folder: `BO2MP_BO3_MAT_ZONES` (comma list)
/// when set; else up to three multiplayer map zones, the ones in `FAVOURITES`
/// first (a mix of settings), then any other `mp_*` map; with no map zone,
/// the shared ones.
fn pool_zones(stems: &[String], wanted: Option<&str>) -> Vec<String> {
    const FAVOURITES: &[&str] = &["mp_biodome", "mp_stronghold", "mp_sector", "mp_apartments", "mp_spire", "mp_metro", "mp_ethiopia", "mp_havoc", "mp_chinatown", "mp_infection", "mp_redwood", "mp_veiled"];
    let have = |n: &str| stems.iter().any(|s| s.eq_ignore_ascii_case(n));
    if let Some(list) = wanted.map(str::trim).filter(|l| !l.is_empty()) {
        return list.split(',').map(|s| s.trim().to_ascii_lowercase()).filter(|s| !s.is_empty() && have(s)).collect();
    }
    let mut out: Vec<String> = FAVOURITES.iter().filter(|n| have(n)).map(|n| (*n).to_owned()).collect();
    let mut others: Vec<String> = stems
        .iter()
        .map(|s| s.to_ascii_lowercase())
        .filter(|s| s.starts_with("mp_") && !matches!(s.as_str(), "mp_common" | "mp_patch") && !s.ends_with("_patch") && !s.contains("_common") && !out.contains(s))
        .collect();
    others.sort();
    out.extend(others);
    out.truncate(3);
    if out.is_empty() {
        out = ["mp_common", "core_common"].into_iter().filter(|n| have(n)).map(str::to_owned).collect();
    }
    out
}

/// What a texture slot of a Black Ops III material is, for Black Ops II's
/// draw.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum Kind {
    Colour,
    Normal,
    /// The specular colour map (its gloss map beside it).
    Specular,
}

/// The slot of a material texture, by the slot's name hash, else by the
/// picture's name (`_c`, `_n`, `_s`, `_g`).
fn slot_kind(hash: u32, image: &str) -> Option<Kind> {
    match hash {
        COLOR_MAP_HASH => return Some(Kind::Colour),
        NORMAL_MAP_HASH => return Some(Kind::Normal),
        T7_SPECULAR_HASH => return Some(Kind::Specular),
        _ => {}
    }
    let n = image.to_ascii_lowercase();
    if n.ends_with("_c") || n.ends_with("_col") {
        Some(Kind::Colour)
    } else if n.ends_with("_n") || n.ends_with("_nml") {
        Some(Kind::Normal)
    } else {
        None
    }
}

/// A Black Ops III vertex's binormal sign from its packed tangent's top two
/// bits: 3 where cross(normal, tangent) runs along the UVs' v (+1), 0 where
/// it runs against (-1). (Measured on `wpn_t7_arak_view`: of its triangles'
/// UV bitangents, 22074 run against cross(n, t) with 0, 8173 along with 3,
/// a handful either way.)
fn binormal_sign(w: u8) -> f32 {
    if w & 2 != 0 { 1.0 } else { -1.0 }
}

/// One vertex as Black Ops II's packed one (32 bytes: position, binormal
/// sign, colour, UV halves, normal, tangent).
fn t6_vertex(p: [f32; 3], sign: f32, colour: [u8; 4], uv: [f32; 2], n: [f32; 3], t: [f32; 3]) -> [u8; 32] {
    let mut v = [0u8; 32];
    for k in 0..3 {
        v[4 * k..4 * k + 4].copy_from_slice(&p[k].to_le_bytes());
    }
    v[12..16].copy_from_slice(&sign.to_le_bytes());
    v[16..20].copy_from_slice(&colour);
    let tex = u32::from(f16_bits(uv[0])) | u32::from(f16_bits(uv[1])) << 16;
    v[20..24].copy_from_slice(&tex.to_le_bytes());
    v[24..28].copy_from_slice(&t6_unit_vec(n).to_le_bytes());
    v[28..32].copy_from_slice(&t6_unit_vec(t).to_le_bytes());
    v
}

/// A quaternion component as Black Ops II's 16-bit fraction.
fn frac16(x: f32) -> i16 {
    (x.clamp(-1.0, 1.0) * 32767.0).round() as i16
}

/// A half's bits as Black Ops II's 16-bit fraction's bits.
fn half_to_frac_bits(h: u16) -> u16 {
    frac16(fastfile_t7::mesh::f16(h)) as u16
}

/// Black Ops III's animation data in Black Ops II's packing: every
/// quaternion component (the rotation sections' keyed values in
/// `randomDataShort`, their constants in `dataShort`) from a half to a
/// 16-bit fraction. The sections are walked in the order the decoder reads
/// them (`xmodel_runtime::AnimClip::from_parts`): per keyed bone its key
/// count in `dataShort`, its key frames (bytes when `numframes < 256`,
/// else `dataShort`, or `indices` with `dataShort` checkpoints for 64 keys
/// and more), then its values. `None` when the data runs short.
fn t7_quats_to_t6(r: &mut asset_t6::XAnimRef) -> Option<()> {
    let byte_indices = r.numframes < 256;
    let (mut ds, mut db, mut idx, mut rs) = (0usize, 0usize, 0usize, 0usize);
    for (section, per) in [(1usize, 2usize), (2, 4)] {
        for _ in 0..r.bone_count[section] {
            let size = usize::from(*r.data_short.get(ds)?);
            ds += 1;
            let count = size + 1;
            if byte_indices {
                db += count;
            } else if size >= 64 {
                idx += count;
                ds += (count - 2) / 256 + 2;
            } else {
                ds += count;
            }
            for v in r.random_data_short.get_mut(rs..rs + count * per)? {
                *v = half_to_frac_bits(*v);
            }
            rs += count * per;
        }
    }
    let _ = (db, idx);
    for (section, per) in [(3usize, 2usize), (4, 4)] {
        for _ in 0..r.bone_count[section] {
            for v in r.data_short.get_mut(ds..ds + per)? {
                *v = half_to_frac_bits(*v);
            }
            ds += per;
        }
    }
    Some(())
}

/// The colour (linear) of a material with no picture to colour it: see
/// `Builder::stand_in_colour`. `placeholder` is its colour slot's
/// placeholder (`,$black_color`, `,$white_diffuse`).
fn solid_colour(name: &str, placeholder: &str, consts: Option<[f32; 8]>) -> [f32; 3] {
    let tint = consts.map(|c| [c[0], c[1], c[2]]);
    let spec = consts.map(|c| [c[4], c[5], c[6]]);
    // (A colour is 0 to 1 on every channel: the constants of a tiled or
    // emissive technique hold tile counts and glow steps in these slots,
    // `[0, 13, 0]` on an ISR's sight glow, which drew bright green.)
    let lit = |c: &[f32; 3]| c.iter().all(|x| x.is_finite() && *x <= 1.0) && c.iter().any(|&x| x > 0.004);
    let white = |c: &[f32; 3]| c.iter().all(|&x| x > 0.99);
    if placeholder.contains("black")
        && let Some(s) = spec.filter(lit)
    {
        return s;
    }
    // (A tint is a lamp's own colour (a tritium dot, an LED); on a body or
    // metal part it is a paint-shop or camo constant, which painted a whole
    // Modern Warfare 2019 gun flat red.)
    let lamp = ["tritium", "dot", "led", "glow", "emis", "lamp", "light", "reticle", "ember"].iter().any(|w| name.contains(w));
    if lamp
        && let Some(t) = tint.filter(|t| lit(t) && !white(t))
    {
        return t;
    }
    // (A tritium dot without constants: its colour by its name.)
    for (word, rgb) in [
        ("red", [0.9, 0.02, 0.02]),
        ("grn", [0.05, 0.9, 0.1]),
        ("green", [0.05, 0.9, 0.1]),
        ("ylw", [0.9, 0.75, 0.05]),
        ("yellow", [0.9, 0.75, 0.05]),
        ("tritium", [0.05, 0.9, 0.1]),
    ] {
        if name.split(['_', '/']).any(|w| w == word) {
            return rgb;
        }
    }
    [0.03, 0.03, 0.03]
}

/// One colour as a 4x4 sRGB picture (DXGI R8G8B8A8_UNORM_SRGB).
fn solid_levels(rgb: [f32; 3]) -> ImageLevels {
    let srgb = |x: f32| {
        let x = x.clamp(0.0, 1.0);
        let s = if x <= 0.003_130_8 { 12.92 * x } else { 1.055 * x.powf(1.0 / 2.4) - 0.055 };
        (s * 255.0).round() as u8
    };
    let px = [srgb(rgb[0]), srgb(rgb[1]), srgb(rgb[2]), 255];
    ImageLevels { format: 29, width: 4, height: 4, levels: 1, data: px.repeat(16), part: None }
}

/// Black Ops III's animation slots -> Black Ops II's (`szXAnims`, the slots
/// `WeaponCatalog::capture_t6` reads). Black Ops III's ADS clips move
/// `tag_torso` as Black Ops II's do: `adsUpAnim` (`vm_<gun>_ads_base_up`) from
/// the hip to the sights, `adsDownAnim` back (they looked like still poses the
/// other way round while every clip looped: the scrub's end wrapped to frame
/// 0), so each goes in its own slot.
const T7_TO_T6_ANIMS: &[(usize, usize)] = &[
    (1, 0x01), (2, 0x02), (3, 0x03), (4, 0x04), (7, 0x05), (10, 0x06), (11, 0x08), (12, 0x09), (14, 0x0D),
    (20, 0x0E), (40, 0x10), (41, 0x12), (42, 0x13), (43, 0x14), (44, 0x15), (45, 0x16), (50, 0x17), (51, 0x18),
    (52, 0x19), (53, 0x1A), (54, 0x1B), (57, 0x1C), (58, 0x1D), (59, 0x1E), (60, 0x1F), (72, 0x20), (73, 0x21),
    (74, 0x22), (75, 0x23), (76, 0x24), (77, 0x25), (84, 0x2C), (85, 0x2D), (86, 0x2E), (87, 0x2F), (88, 0x30),
    (89, 0x31), (90, 0x32), (91, 0x33), (92, 0x34), (93, 0x35), (94, 0x36), (95, 0x37), (103, 0x3A), (104, 0x3D),
    (106, 0x3E), (107, 0x3F), (108, 0x40), (173, 0x55), (174, 0x56),
];

/// Black Ops II's `szXAnims` slot count.
const T6_ANIM_SLOTS: usize = 88;

/// Black Ops III's `weapFireType_t` (full auto, single shot, burst, auto
/// burst, stacked, all locked, minigun, charge shot, ...) as Black Ops II's
/// (full auto, single, burst 2..5, stacked, minigun, charge shot):
/// bursts by their `burstCount`.
fn t7_fire_type(raw: i32, burst: i32) -> i32 {
    match raw {
        0 => 0,
        2 | 3 => 2 + (burst.clamp(2, 5) - 2),
        4 => 6,
        6 => 7,
        7..=9 => 8,
        _ => 1,
    }
}

/// Black Ops III's `playerAnimType` as Black Ops II's: the first entries
/// (none, default, other, sniper) and hold .. remote control are the same;
/// the rest are drawn as the default rifle's.
fn t7_player_anim_type(raw: i32) -> i32 {
    match raw {
        0..=3 | 5..=10 => raw,
        _ => 1,
    }
}

/// bo2mp: what `Builder::model_ref` found in one model's mesh (the
/// `bo2mp t7 pack gun:` line reports it): the box of its vertices (from the
/// gun's `tag_weapon` origin), vertices whose skin weight names a bone the
/// model lacks (put on bone 0), vertices with no weight at all, positions
/// that are not finite, the largest bone index, and per surface the bones
/// its vertices are on.
#[derive(Clone, Debug, Default)]
struct MeshFacts {
    lods: usize,
    verts: usize,
    tris: usize,
    mins: [f32; 3],
    maxs: [f32; 3],
    stray: usize,
    unweighted: usize,
    nonfinite: usize,
    max_bone: u16,
    units: bool,
    surface_bones: Vec<Vec<u16>>,
    /// Per bone, the vertices with a weight on it.
    bone_verts: Vec<usize>,
}

/// The facts of the models built in the current load, by model name.
static MESH_FACTS: std::sync::Mutex<Vec<(String, MeshFacts)>> = std::sync::Mutex::new(Vec::new());

fn mesh_facts(name: &str) -> Option<MeshFacts> {
    MESH_FACTS.lock().ok()?.iter().find(|(n, _)| n == name).map(|(_, f)| f.clone())
}

struct Builder<'a> {
    zone: &'a fastfile_t7::Zone,
    cap: &'a Capture,
    paks: XpakSet,
    catalog: MaterialCatalog,
    owner: ZoneOwner,
    /// (material name, size cap) -> catalog material.
    materials: HashMap<(String, u32), Option<usize>>,
    /// (picture key, size cap) -> catalog image slot.
    images: HashMap<(String, u32), Option<usize>>,
    image_failures: Vec<String>,
    decoded: usize,
    /// Pictures made ahead, on several threads ([`Builder::make_ahead`]),
    /// by (key, size cap): `link_image` takes its picture from here.
    ready: HashMap<(String, u32), Result<Made, String>>,
    /// The disk cache's key for the converted pictures
    /// (`t7_gun_cache`), and how many came from it.
    pic_cache: Option<String>,
    pic_cache_hits: usize,
}

/// A picture made: its levels and their GPU form.
type Made = (fastfile_iw6::image::ImagePixels, bevy::prelude::Image);

/// How a picture a material may want is made (see `Builder::material`).
enum Recipe {
    /// A picture's levels as they are (a colour map, or a stand-in's).
    Levels(String),
    /// A normal map as BC5.
    Normal(String),
    /// A specular colour map with its gloss in alpha, as BC3.
    SpecGloss(String, String),
}

/// A picture's levels at most `max` on a side.
fn levels_of(cap: &Capture, zone: &[u8], paks: &XpakSet, name: &str, max: u32) -> Result<ImageLevels, String> {
    let t = std::time::Instant::now();
    let im = cap.image(name).ok_or_else(|| format!("{name}: not in core_common"))?;
    let out = texture::image_levels_max(im, zone, paks, max).map_err(|e| format!("{name}: {e}"));
    PROBE_LEVELS.fetch_add(t.elapsed().as_nanos() as u64, std::sync::atomic::Ordering::Relaxed);
    out
}

/// A picture made by its recipe (its levels, converted), and its GPU form
/// (`colour`: read as sRGB).
fn make_picture(cap: &Capture, zone: &[u8], paks: &XpakSet, recipe: &Recipe, max: u32, colour: bool) -> Result<Made, String> {
    let levels = match recipe {
        Recipe::Levels(name) => levels_of(cap, zone, paks, name, max),
        Recipe::Normal(normal) => {
            let l = levels_of(cap, zone, paks, normal, max)?;
            texture::normal_map_bc5(&l).ok_or_else(|| format!("{normal}: format {} not decoded", l.format))
        }
        Recipe::SpecGloss(spec, gloss) => {
            let s = levels_of(cap, zone, paks, spec, max)?;
            // (The gloss map at least as big as the specular map.)
            let g = levels_of(cap, zone, paks, gloss, max.max(s.width.max(s.height)))?;
            texture::specular_gloss_bc3(&s, &g).ok_or_else(|| format!("{spec} + {gloss}: formats {} and {} not paired", s.format, g.format))
        }
    }?;
    let px = fastfile_iw6::image::ImagePixels {
        width: levels.width,
        height: levels.height,
        levels: levels.levels,
        dxgi_format: levels.format,
        data: levels.data,
    };
    super::iw6::gpu_image(&px, colour).map(|img| (px, img))
}

impl<'a> Builder<'a> {
    fn string(&self, index: u32) -> String {
        self.zone
            .assets
            .script_strings
            .get(index as usize)
            .cloned()
            .flatten()
            .unwrap_or_default()
    }

    /// The densest LOD's mesh bytes (in the zone or its `.xpak` entry).
    fn mesh_bytes(&self, mesh: &fastfile_t7::walk::capture::MeshRec) -> Result<Vec<u8>, String> {
        match (mesh.data, mesh.xpak) {
            (Some((at, len)), _) => self.zone.image.get(at..at + len).map(<[u8]>::to_vec).ok_or_else(|| "mesh data past the zone".into()),
            (None, Some(x)) => self.paks.read(x.key).map_err(|e| e.to_string()),
            _ => Err("mesh has no data".into()),
        }
    }

    /// A model in Black Ops II's captured form (its densest LOD) and the
    /// material name of each surface.
    fn model_ref(&self, rec: &ModelRec) -> Result<(asset_t6::XModelRef, Vec<Option<String>>), String> {
        let lod = rec
            .lods
            .iter()
            .max_by_key(|l| self.cap.mesh(&l.mesh).map_or(0, |m| m.vert_count))
            .ok_or("no LODs")?;
        let mesh = self.cap.mesh(&lod.mesh).ok_or_else(|| format!("mesh {} not read", lod.mesh))?;
        let data = self.mesh_bytes(mesh)?;
        let g = fastfile_t7::mesh::decode(mesh, &data).map_err(|e| e.to_string())?;
        let k = &rec.skeleton;
        let n_bones = rec.bones;
        if n_bones == 0 || n_bones > 255 || k.base_mat.len() != n_bones || k.bone_names.len() != n_bones {
            return Err(format!("skeleton not read ({n_bones} bones)"));
        }
        // (Skin weights name bones by index; a few models count them in
        // 64-byte bone units as Black Ops II does.)
        let max_bone = g.weights.iter().flatten().map(|w| w.0).max().unwrap_or(0);
        let units = usize::from(max_bone) >= n_bones && g.weights.iter().flatten().all(|w| w.0 % 64 == 0);
        let bone_of = |b: u16| -> u16 {
            let b = if units { b / 64 } else { b };
            if usize::from(b) < n_bones { b } else { 0 }
        };
        let (mut mins, mut maxs) = ([f32::MAX; 3], [f32::MIN; 3]);
        let mut lod0 = Vec::with_capacity(mesh.surfaces.len());
        let mut materials = Vec::with_capacity(mesh.surfaces.len());
        let mut facts = MeshFacts {
            lods: rec.lods.len(),
            verts: mesh.vert_count as usize,
            tris: mesh.index_count as usize / 3,
            max_bone,
            units,
            bone_verts: vec![0; n_bones],
            ..Default::default()
        };
        for (si, sf) in mesh.surfaces.iter().enumerate() {
            let base = sf.base_vert as usize;
            let n = usize::from(sf.vert_count);
            if base + n > g.positions.len() {
                return Err(format!("surface {si} past the vertices"));
            }
            for v in base..base + n {
                match g.weights[v].iter().filter(|w| w.1 > 0.0).map(|w| if units { w.0 / 64 } else { w.0 }).max() {
                    None => facts.unweighted += 1,
                    Some(b) if usize::from(b) >= n_bones => facts.stray += 1,
                    _ => {}
                }
                if !g.positions[v].iter().all(|c| c.is_finite()) {
                    facts.nonfinite += 1;
                }
            }
            // Each vertex's influences, heaviest first, at most four.
            let infl: Vec<Vec<(u16, f32)>> = (base..base + n)
                .map(|v| {
                    let mut w: Vec<(u16, f32)> = g.weights[v].iter().map(|&(b, x)| (bone_of(b), x)).filter(|x| x.1 > 0.0).collect();
                    w.sort_by(|a, b| b.1.total_cmp(&a.1));
                    w.truncate(4);
                    if w.is_empty() {
                        w.push((0, 1.0));
                    }
                    let sum: f32 = w.iter().map(|x| x.1).sum();
                    for x in &mut w {
                        x.1 /= sum.max(1e-6);
                    }
                    w
                })
                .collect();
            // Black Ops II orders a surface's vertices by influence count.
            let mut order: Vec<usize> = (0..n).collect();
            order.sort_by_key(|&i| infl[i].len());
            let mut new_of = vec![0u16; n];
            for (new, &old) in order.iter().enumerate() {
                new_of[old] = new as u16;
            }
            let mut verts = Vec::with_capacity(n * 32);
            let mut part_bits = [0u32; 6];
            let mut blend_counts = [0i16; 4];
            let mut blend = Vec::new();
            for &old in &order {
                let v = base + old;
                let p = g.positions[v];
                for c in 0..3 {
                    mins[c] = mins[c].min(p[c]);
                    maxs[c] = maxs[c].max(p[c]);
                }
                verts.extend_from_slice(&t6_vertex(
                    p,
                    binormal_sign(g.tangent_w.get(v).copied().unwrap_or(0)),
                    g.colors.get(v).copied().unwrap_or([255; 4]),
                    g.uvs[v],
                    g.normals[v],
                    g.tangents.get(v).copied().unwrap_or([1.0, 0.0, 0.0]),
                ));
                let w = &infl[old];
                for &(bone, _) in w {
                    if let Some(word) = part_bits.get_mut(usize::from(bone) / 32) {
                        // bo2mp: Black Ops II's part bits count a bone from the top
                        // bit (`xmodel_no_scale_bit`, the hide words): set from the
                        // bottom they named the mirror bone (31 - bone % 32) of every
                        // surface, so a hidden tag hid the surfaces of another bone.
                        *word |= part_bit(bone, pack_fix_on());
                    }
                }
                blend_counts[w.len() - 1] += 1;
                blend.push(w[0].0 * 64);
                for &(b, x) in &w[1..] {
                    blend.push(b * 64);
                    blend.push((x * 65536.0).round().clamp(0.0, 65535.0) as u16);
                }
            }
            let indices = g.surfaces[si]
                .indices
                .iter()
                .map(|&i| new_of.get((i as usize).wrapping_sub(base)).copied().unwrap_or(0))
                .collect();
            let mut used: Vec<u16> = infl.iter().flatten().map(|x| x.0).collect();
            used.sort_unstable();
            used.dedup();
            facts.surface_bones.push(used);
            for w in &infl {
                for &(bone, _) in w {
                    if let Some(n) = facts.bone_verts.get_mut(usize::from(bone)) {
                        *n += 1;
                    }
                }
            }
            lod0.push(asset_t6::XSurfaceRef {
                verts,
                vert_count: n as u16,
                indices,
                vert_lists: Vec::new(),
                flags: 0,
                material: None,
                blend_counts,
                blend,
                part_bits,
            });
            materials.push(lod.materials.get(si).cloned().filter(|m| !m.is_empty()));
        }
        if lod0.is_empty() {
            return Err("no surfaces".into());
        }
        let radius = (0..3).map(|c| mins[c].abs().max(maxs[c].abs()).powi(2)).sum::<f32>().sqrt();
        let roots = k.num_root_bones.max(1);
        facts.mins = mins;
        facts.maxs = maxs;
        if let Ok(mut all) = MESH_FACTS.lock() {
            all.retain(|(n, _)| n != &rec.name);
            all.push((rec.name.clone(), facts));
        }
        Ok((
            asset_t6::XModelRef {
                name: rec.name.clone(),
                numsurfs: lod0.len().min(255) as u8,
                materials: Vec::new(),
                num_bones: n_bones as u8,
                num_lods: 1,
                mins,
                maxs,
                radius,
                lighting_origin_offset: [0.0; 3],
                lighting_origin_range: 0.0,
                lods: vec![(0.0, lod0.len() as u16, 0)],
                lod0,
                bone_names: k.bone_names.iter().map(|&s| self.string(s)).collect(),
                num_root_bones: roots,
                parent_list: k.parent_list.clone(),
                quats: k.quats.iter().map(|q| q.map(frac16)).collect(),
                trans: k.trans.clone(),
                base_mat: k.base_mat.iter().map(|m| ([m[0], m[1], m[2], m[3]], [m[4], m[5], m[6]])).collect(),
                part_classification: k.part_classification.clone(),
                bone_info: vec![None; n_bones],
                contents: 0,
                coll_lod: -1,
                coll_surfs: Vec::new(),
                phys_preset: None,
            },
            materials,
        ))
    }

    /// A picture's levels at most `max` on a side.
    fn levels(&self, name: &str, max: u32) -> Result<ImageLevels, String> {
        levels_of(self.cap, &self.zone.image, &self.paks, name, max)
    }

    /// Makes ahead, on the machine's threads, the pictures these
    /// materials will ask `link_image` for (as `material` and
    /// `stand_in_colour` ask for them), so the build links them without
    /// reading and converting one at a time.
    fn make_ahead(&mut self, materials: &[(String, u32)]) {
        let mut jobs: Vec<((String, u32), Recipe, bool)> = Vec::new();
        let mut seen: std::collections::HashSet<(String, u32)> = std::collections::HashSet::new();
        let mut want = |key: String, max: u32, recipe: Recipe, colour: bool, jobs: &mut Vec<_>| {
            if seen.insert((key.clone(), max)) {
                jobs.push(((key, max), recipe, colour));
            }
        };
        let real = |n: &String| !n.starts_with(',') && !n.starts_with('$');
        for (name, max) in materials {
            let Some(m) = self.cap.material(name).filter(|_| !name.starts_with(',')) else { continue };
            let technique = m.techset.clone().unwrap_or_default();
            let draw = t7_draw(&m.name, technique.trim_start_matches(','), true);
            let mut picks: HashMap<Kind, String> = HashMap::new();
            for (hash, image) in m.textures.iter().filter(|(_, n)| real(n)) {
                if let Some(kind) = slot_kind(*hash, image) {
                    picks.entry(kind).or_insert_with(|| image.clone());
                }
            }
            match picks.get(&Kind::Colour) {
                Some(colour) => want(colour.clone(), *max, Recipe::Levels(colour.clone()), true, &mut jobs),
                // (A stand-in's: its specular colour map, else its
                // emissive picture.)
                None => {
                    let t = technique.trim_start_matches(',');
                    if let Some(spec) = picks.get(&Kind::Specular) {
                        want(spec.clone(), *max, Recipe::Levels(spec.clone()), true, &mut jobs);
                    }
                    if t.contains("emissive")
                        && let Some((_, glow)) = m.textures.iter().find(|(h, n)| *h == T7_EMISSIVE_HASH && real(n))
                    {
                        want(glow.clone(), *max, Recipe::Levels(glow.clone()), true, &mut jobs);
                    }
                }
            }
            if !draw.unlit {
                if let Some(normal) = picks.get(&Kind::Normal) {
                    want(format!("{normal}#bc5"), *max, Recipe::Normal(normal.clone()), false, &mut jobs);
                }
                let gloss = m.textures.iter().find(|(h, n)| *h == T7_GLOSS_HASH && real(n)).map(|(_, n)| n.clone());
                if let (Some(spec), Some(gloss)) = (picks.get(&Kind::Specular), gloss) {
                    want(format!("{spec}+{gloss}"), *max, Recipe::SpecGloss(spec.clone(), gloss), false, &mut jobs);
                }
            }
        }
        // (Those the disk cache holds are taken from it: only their GPU
        // form is made again.)
        let mut kept = self.pic_cache.as_deref().map(super::t7_gun_cache::load).unwrap_or_default();
        let had = kept.len();
        jobs.retain(|(key, _, colour)| match kept.get(key) {
            Some(px) => match super::iw6::gpu_image(px, *colour) {
                Ok(img) => {
                    self.ready.insert(key.clone(), Ok((px.clone(), img)));
                    self.pic_cache_hits += 1;
                    false
                }
                Err(_) => true,
            },
            None => true,
        });
        let (cap, zone, paks) = (self.cap, &self.zone.image[..], &self.paks);
        let next = std::sync::atomic::AtomicUsize::new(0);
        let threads = std::thread::available_parallelism().map_or(4, |n| n.get()).clamp(2, 16);
        let made: Vec<Vec<((String, u32), Result<Made, String>)>> = std::thread::scope(|s| {
            let workers: Vec<_> = (0..threads)
                .map(|_| {
                    s.spawn(|| {
                        let mut out = Vec::new();
                        loop {
                            let k = next.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                            let Some((key, recipe, colour)) = jobs.get(k) else { break };
                            out.push((key.clone(), make_picture(cap, zone, paks, recipe, key.1, *colour)));
                        }
                        out
                    })
                })
                .collect();
            workers.into_iter().map(|w| w.join().unwrap_or_default()).collect()
        });
        let mut new_ones = 0usize;
        for (key, result) in made.into_iter().flatten() {
            if let Ok((px, _)) = &result {
                kept.insert(key.clone(), px.clone());
                new_ones += 1;
            }
            self.ready.insert(key, result);
        }
        if let Some(cache_key) = self.pic_cache.as_deref()
            && new_ones > 0
        {
            let total = kept.len();
            let cache_key = cache_key.to_owned();
            // (Written off the load's path: the next start reads it.)
            std::thread::Builder::new()
                .name("t7-gun-cache".into())
                .spawn(move || match super::t7_gun_cache::store(&cache_key, &kept) {
                    Ok(()) => diag::info!(World, "t7 guest guns: {total} converted pictures kept in the cache ({had} were)"),
                    Err(e) => diag::warn!(World, "t7 guest guns: pictures not kept in the cache: {e}"),
                })
                .ok();
        }
    }

    /// One picture into the catalog (`key` names the converted form).
    fn link_image(&mut self, key: String, max: u32, semantic: u8, make: impl FnOnce(&Self) -> Result<ImageLevels, String>) -> Option<usize> {
        if let Some(slot) = self.images.get(&(key.clone(), max)) {
            return *slot;
        }
        let colour = semantic == TS_COLOR_MAP;
        let t = std::time::Instant::now();
        let made = match self.ready.remove(&(key.clone(), max)) {
            Some(made) => made,
            None => make(self).and_then(|l| {
                let px = fastfile_iw6::image::ImagePixels {
                    width: l.width,
                    height: l.height,
                    levels: l.levels,
                    dxgi_format: l.format,
                    data: l.data,
                };
                super::iw6::gpu_image(&px, colour).map(|img| (px, img))
            }),
        };
        PROBE_MAKE.fetch_add(t.elapsed().as_nanos() as u64, std::sync::atomic::Ordering::Relaxed);
        let slot = match made {
            Ok((px, gpu)) => {
                self.decoded += 1;
                Some(self.catalog.link_image(AuthoredImage {
                    namespace: NS,
                    name: AssetRef::Real(format!("{key}#{max}")),
                    map_type: 0,
                    semantic,
                    category: 0,
                    use_srgb_reads: colour,
                    width: px.width as u16,
                    height: px.height as u16,
                    depth: 1,
                    level_count: px.levels as u8,
                    format: 0,
                    payload: Arc::new(Vec::new()),
                    decoded: Some(Arc::new(gpu)),
                    common_owned: false,
                    decoded_variant: None,
                    decoded_by: None,
                    pending_decode: None,
                }))
            }
            Err(e) => {
                if self.image_failures.len() < 64 {
                    self.image_failures.push(e);
                }
                None
            }
        };
        self.images.insert((key, max), slot);
        slot
    }

    /// One material into the catalog: its colour map, its normal map (as
    /// BC5), its specular colour with its gloss in alpha (as BC3).
    fn material(&mut self, name: &str, max: u32) -> Option<usize> {
        if let Some(&m) = self.materials.get(&(name.to_owned(), max)) {
            return m;
        }
        let Some(m) = self.cap.material(name).filter(|_| !name.starts_with(',')) else {
            self.materials.insert((name.to_owned(), max), None);
            return None;
        };
        let technique = m.techset.clone().unwrap_or_default();
        let draw = t7_draw(&m.name, technique.trim_start_matches(','), true);
        let real = |n: &String| !n.starts_with(',') && !n.starts_with('$');
        let mut picks: HashMap<Kind, String> = HashMap::new();
        for (hash, image) in m.textures.iter().filter(|(_, n)| real(n)) {
            if let Some(kind) = slot_kind(*hash, image) {
                picks.entry(kind).or_insert_with(|| image.clone());
            }
        }
        let gloss = m.textures.iter().find(|(h, n)| *h == T7_GLOSS_HASH && real(n)).map(|(_, n)| n.clone());
        let mut textures = Vec::new();
        let colour_slot = match picks.get(&Kind::Colour).cloned() {
            Some(colour) => self.link_image(colour.clone(), max, TS_COLOR_MAP, |b| b.levels(&colour, max)),
            None => self.stand_in_colour(m, &technique, picks.get(&Kind::Specular).cloned(), max),
        };
        if let Some(slot) = colour_slot {
            textures.push(MaterialTextureBinding {
                name_hash: COLOR_MAP_HASH,
                name_start: 0,
                name_end: 0,
                sampler_state: 0x13,
                semantic: TS_COLOR_MAP,
                image: Some(slot),
            });
        }
        if !draw.unlit {
            if let Some(normal) = picks.get(&Kind::Normal).cloned()
                && let Some(slot) = self.link_image(format!("{normal}#bc5"), max, TS_NORMAL_MAP, |b| {
                    let l = b.levels(&normal, max)?;
                    texture::normal_map_bc5(&l).ok_or_else(|| format!("{normal}: format {} not decoded", l.format))
                })
            {
                textures.push(MaterialTextureBinding {
                    name_hash: NORMAL_MAP_HASH,
                    name_start: 0,
                    name_end: 0,
                    sampler_state: 0x13,
                    semantic: TS_NORMAL_MAP,
                    image: Some(slot),
                });
            }
            if let (Some(spec), Some(gloss)) = (picks.get(&Kind::Specular).cloned(), gloss)
                && let Some(slot) = self.link_image(format!("{spec}+{gloss}"), max, TS_SPECULAR_MAP, |b| {
                    let s = b.levels(&spec, max)?;
                    // (The gloss map at least as big as the specular map.)
                    let g = b.levels(&gloss, max.max(s.width.max(s.height)))?;
                    texture::specular_gloss_bc3(&s, &g).ok_or_else(|| format!("{spec} + {gloss}: formats {} and {} not paired", s.format, g.format))
                })
            {
                textures.push(MaterialTextureBinding {
                    name_hash: SPECULAR_MAP_HASH,
                    name_start: 0,
                    name_end: 0,
                    sampler_state: 0x13,
                    semantic: TS_SPECULAR_MAP,
                    image: Some(slot),
                });
            }
        }
        let index = self.catalog.link_material(AuthoredMaterial {
            name: AssetRef::Real(format!("{}#{max}", m.name)),
            namespace: NS,
            technique_set: AssetRef::default(),
            technique_set_edge: AssetEdge::Absent,
            draw_surf: 0,
            sort_key: draw.sort_key,
            info_game_flags: 0,
            texture_atlas: None,
            surface_type_bits: None,
            t5_layered_surface_types: None,
            state_flags: 0,
            camera_region: 0,
            state_bits: Vec::new(),
            state_bits_entry: None,
            t5_state_bits_entry: None,
            iw5_state_bits_entry: None,
            technique_table: None,
            route: None,
            textures,
            constants: Vec::new(),
            zone: self.owner,
            t6_draw: Some(draw),
        });
        self.materials.insert((name.to_owned(), max), Some(index));
        Some(index)
    }

    /// bo2mp: a scene prop's emissive material (Nuk3town's derez cubes,
    /// `lit_emissive_scroll`): drawn unlit from its emissive map times its
    /// emissive tint (the lit buffer's floats 76-78: orange), twice over
    /// (BO3 lifts it by `hdrScale`); added when transparent. `None` for a
    /// material that is not emissive.
    fn emissive_prop_material(&mut self, name: &str) -> Option<usize> {
        const EMISSIVE_MAP_HASH: u32 = 0x3461_4347;
        let m = self.cap.material(name).filter(|_| !name.starts_with(','))?;
        let technique = m.techset.clone().unwrap_or_default();
        if !technique.contains("emissive") {
            return None;
        }
        let em = m.textures.iter().find(|(h, n)| *h == EMISSIVE_MAP_HASH && !n.starts_with('$') && !n.starts_with(",$")).map(|(_, n)| n.trim_start_matches(',').to_owned())?;
        let tint: [f32; 3] = m
            .globals
            .iter()
            .filter(|(_, g)| g.len() >= 80 * 4)
            .map(|(_, g)| std::array::from_fn(|k| f32::from_le_bytes(g[(76 + k) * 4..(77 + k) * 4].try_into().unwrap_or([0; 4]))))
            .find(|t: &[f32; 3]| t.iter().any(|v| *v > 0.0) && t.iter().all(|v| v.is_finite() && *v >= 0.0))
            .unwrap_or([1.0; 3]);
        // (An opaque cube's body is its lit colour placeholder, the glow
        // added over it: Nuk3town's grey cubes sit cream in the sun with
        // orange caps; drawn from the glow alone they were dark grey. A
        // black one's body stays dark.)
        let transparent = technique.contains("transparent");
        let base = if transparent || m.name.contains("black") { 0.0 } else { OPAQUE_CUBE_BODY };
        let key = format!("{em}*glow{:.3},{:.3},{:.3}+{base}", tint[0], tint[1], tint[2]);
        let slot = self.link_image(key, 512, TS_COLOR_MAP, |b| {
            let l = b.levels(&em, 512)?;
            let (w, h, mut px) = texture::level_rgba(&l, 0).ok_or_else(|| format!("{em}: format {} not decoded", l.format))?;
            for p in px.chunks_exact_mut(4) {
                for k in 0..3 {
                    let lin = base + fastfile_t7::look::to_linear(p[k]) * tint[k] * 2.0;
                    p[k] = fastfile_t7::look::to_srgb(lin);
                }
            }
            fastfile_t7::look::encode_colour(w, h, px, false).ok_or_else(|| format!("{em}: {w}x{h} not encoded"))
        })?;
        let mut draw = t7_draw(&m.name, technique.trim_start_matches(','), true);
        draw.unlit = true;
        draw.unlit_scale_exp = 1;
        if technique.contains("transparent") {
            draw.blend = asset_core::T6Blend::Add;
            draw.unlit_scale_exp = 0;
        }
        let index = self.catalog.link_material(AuthoredMaterial {
            name: AssetRef::Real(format!("{}#glow", m.name)),
            namespace: AssetNamespace::T6,
            technique_set: AssetRef::default(),
            technique_set_edge: AssetEdge::Absent,
            draw_surf: 0,
            sort_key: draw.sort_key,
            info_game_flags: 0,
            texture_atlas: None,
            surface_type_bits: None,
            t5_layered_surface_types: None,
            state_flags: 0,
            camera_region: 0,
            state_bits: Vec::new(),
            state_bits_entry: None,
            t5_state_bits_entry: None,
            iw5_state_bits_entry: None,
            technique_table: None,
            route: None,
            textures: vec![MaterialTextureBinding { name_hash: COLOR_MAP_HASH, name_start: 0, name_end: 0, sampler_state: 0x13, semantic: TS_COLOR_MAP, image: Some(slot) }],
            constants: Vec::new(),
            zone: self.owner,
            t6_draw: Some(draw),
        });
        Some(index)
    }

    /// The colour a material without a colour map of its own is drawn with
    /// (Black Ops III draws many gun parts from their specular colour, or
    /// from constants alone: a bullet's brass, a tritium dot):
    ///
    /// - its specular colour map (`_s`; metal parts' colour is their
    ///   reflectance, and BO2 draws a gun's metal from its colour map);
    /// - else its emissive picture (a lit-emissive tritium dot's);
    /// - else one colour from its constants: the specular colour where the
    ///   colour slot is Black Ops III's black placeholder (metals: brass),
    ///   else its tint (a tritium dot's red), else a dark grey.
    ///
    /// `None` for what Black Ops II's gun pass should not draw solid: a
    /// paint-shop overlay, a scope's stencil or glass, a reticle, a
    /// thermal screen, a 3D-UI panel.
    fn stand_in_colour(
        &mut self,
        m: &fastfile_t7::walk::capture::MaterialRec,
        technique: &str,
        specular: Option<String>,
        max: u32,
    ) -> Option<usize> {
        let t = technique.trim_start_matches(',');
        // (A scope lens with no picture: glass, not a dark grey disc.)
        let skip = ["paintshop", "stencil", "reticle", "thermal", "ui_3d", "transparent", "glass", "lens"];
        if skip.iter().any(|w| t.contains(w) || m.name.contains(w)) {
            return None;
        }
        if let Some(spec) = specular
            && let Some(slot) = self.link_image(spec.clone(), max, TS_COLOR_MAP, |b| b.levels(&spec, max))
        {
            return Some(slot);
        }
        let real = |n: &str| !n.starts_with(',') && !n.starts_with('$');
        if t.contains("emissive")
            && let Some((_, glow)) = m.textures.iter().find(|(h, n)| *h == T7_EMISSIVE_HASH && real(n))
        {
            let glow = glow.clone();
            if let Some(slot) = self.link_image(glow.clone(), max, TS_COLOR_MAP, |b| b.levels(&glow, max)) {
                return Some(slot);
            }
        }
        let placeholder = m
            .textures
            .iter()
            .find(|(h, _)| *h == COLOR_MAP_HASH)
            .map_or("", |(_, n)| n.as_str());
        let rgb = solid_colour(&m.name, placeholder, m.colour_consts);
        diag::info!(
            World,
            "t7 gun: material {} ({}) has no colour picture; drawn solid {:.2},{:.2},{:.2} (consts {:?})",
            m.name,
            t,
            rgb[0],
            rgb[1],
            rgb[2],
            m.colour_consts
        );
        let key = format!("$solid:{:.3},{:.3},{:.3}", rgb[0], rgb[1], rgb[2]);
        self.link_image(key, 4, TS_COLOR_MAP, |_| Ok(solid_levels(rgb)))
    }

    /// An animation in Black Ops II's captured form; `None` when its bone
    /// counts do not fit Black Ops II's bytes or its data is short.
    ///
    /// The header past the counts (measured on core_common's 6871): 0x24
    /// `bLoop` (the sprint, crawl and shotgun reload loops, walks), 0x26
    /// `bDelta`, 0x30 the ten bone counts (u16), 0x44 the kind (1 a whole
    /// pose, every gun's slots; 6 additive, the 1-bone walk and jump bobs;
    /// 2 and 7 delta), 0x50 the frame rate.
    fn anim_ref(&self, a: &AnimRec) -> Option<asset_t6::XAnimRef> {
        let h = &a.header;
        let u16_at = |o: usize| h.get(o..o + 2).map_or(0, |b| u16::from_le_bytes([b[0], b[1]]));
        let mut bone_count = [0u8; 10];
        for (k, c) in bone_count.iter_mut().enumerate() {
            *c = u8::try_from(u16_at(0x30 + 2 * k)).ok()?;
        }
        let framerate = h.get(0x50..0x54).map_or(30.0, |b| f32::from_le_bytes([b[0], b[1], b[2], b[3]]));
        let bytes = |s: Option<(usize, usize)>| s.and_then(|(at, len)| self.zone.image.get(at..at + len)).unwrap_or(&[]);
        let indices = if a.numframes >= 256 {
            super::iw6::le_u16s(bytes(a.indices))
        } else {
            bytes(a.indices).iter().map(|&b| u16::from(b)).collect()
        };
        let notifies = a
            .notifies
            .iter()
            .map(|&(kind, time, param, _)| {
                let (kind, param) = (self.string(kind), self.string(param));
                let name = match kind.as_str() {
                    "sound" if !param.is_empty() => format!("{}{param}", asset_game::T5_NOTE_SOUND_PREFIX),
                    "rumble" if !param.is_empty() => format!("{}{param}", asset_game::T5_NOTE_RUMBLE_PREFIX),
                    _ if !param.is_empty() => param,
                    _ => kind,
                };
                (name, time)
            })
            .collect();
        let mut r = asset_t6::XAnimRef {
            delta_quat: Vec::new(),
            name: a.name.clone(),
            numframes: a.numframes,
            framerate: if framerate.is_finite() && framerate > 0.0 { framerate } else { 30.0 },
            // (0x44, read here before, is 1 on every gun clip: every one
            // looped, so no reload, raise, drop or sprint in/out ever ended.)
            looping: h.get(0x24).is_some_and(|&b| b != 0),
            delta: false,
            delta_trans: Vec::new(),
            bone_count,
            names: a.names.iter().map(|&s| self.string(s)).collect(),
            notifies,
            data_byte: bytes(a.data_byte).to_vec(),
            data_short: super::iw6::le_u16s(bytes(a.data_short)),
            data_int: super::iw6::le_u32s(bytes(a.data_int)),
            random_data_byte: bytes(a.random_data_byte).to_vec(),
            random_data_short: super::iw6::le_u16s(bytes(a.random_data_short)),
            random_data_int: super::iw6::le_u32s(bytes(a.random_data_int)),
            indices,
        };
        t7_quats_to_t6(&mut r)?;
        Some(r)
    }

    /// bo2mp: the `bo2mp t7 pack gun:` line: a pack gun's view model as built
    /// (its bones, surfaces, vertices, the box of its vertices from the
    /// `tag_weapon` origin, the weights it could not place, what its hide
    /// tags take off) and each attached view model with its tag, its box and
    /// whether the aiming overlay filter dropped it.
    fn pack_gun_line(&self, name: &str, w: &WeaponRec, attached: &[(String, String)], dropped: &[String]) -> String {
        let size = |f: &MeshFacts| -> String {
            let d: Vec<String> = (0..3).map(|c| format!("{:.1}", f.maxs[c] - f.mins[c])).collect();
            let m: Vec<String> = (0..3).map(|c| format!("{:.1}", (f.maxs[c] + f.mins[c]) * 0.5)).collect();
            format!("box {} centre {}", d.join("x"), m.join(","))
        };
        let Some(view) = self.field_str(w, "gunModel") else { return format!("bo2mp t7 pack gun: {name}: no view model") };
        let mut out = format!("bo2mp t7 pack gun: {name} view {view}");
        match (self.cap.model(&view), mesh_facts(&view)) {
            (Some(rec), Some(f)) => {
                let bones: Vec<String> = rec.skeleton.bone_names.iter().map(|&b| self.string(b)).collect();
                let hide: Vec<u16> = (0..bones.len())
                    .filter(|&i| w.hide_tags.iter().any(|&t| self.string(t).eq_ignore_ascii_case(&bones[i])))
                    .map(|i| i as u16)
                    .collect();
                let any = f.surface_bones.iter().filter(|s| s.iter().any(|b| hide.contains(b))).count();
                let all = f.surface_bones.iter().filter(|s| !s.is_empty() && s.iter().all(|b| hide.contains(b))).count();
                let hidden: Vec<String> = hide.iter().map(|&i| format!("{}={i}", bones[usize::from(i)])).collect();
                let (collapsed, with) = self.hide_collapse(w, rec, &f);
                let heavy: Vec<String> = f.bone_verts.iter().enumerate().filter(|(_, n)| **n > 0).map(|(b, n)| format!("{}={n}", bones.get(b).map_or("?", String::as_str))).collect();
                out += &format!(
                    ": {} bones, {} lod(s), {} surfaces, {} vertices, {} triangles, {}, weights on bones the model lacks {}, none {}, positions not finite {}, largest bone {}{}, hide tags {:?} touch {any} surfaces, take {all} whole, collapse {collapsed} vertices ({:.0}%) with {with:?}, per-surface bones {:?}, vertices per bone {:?}",
                    bones.len(),
                    f.lods,
                    f.surface_bones.len(),
                    f.verts,
                    f.tris,
                    size(&f),
                    f.stray,
                    f.unweighted,
                    f.nonfinite,
                    f.max_bone,
                    if f.units { " (64-byte units)" } else { "" },
                    hidden,
                    100.0 * collapsed as f32 / f.verts.max(1) as f32,
                    f.surface_bones,
                    heavy
                );
            }
            _ => out += ": model not built",
        }
        for (m, tag) in attached {
            out += &format!(" | attach {m} at {tag}");
            if let Some(f) = mesh_facts(m) {
                out += &format!(" {} verts {}", f.verts, size(&f));
            }
            if dropped.contains(m) {
                out += " DROPPED (aiming overlay)";
            }
        }
        out
    }

    /// bo2mp: the bones a weapon's hide tags take off a model when the view
    /// is posed: each tag's bone and every bone under it (the rule the
    /// first-person hide words follow), by index.
    fn hidden_bones(&self, w: &WeaponRec, rec: &ModelRec) -> Vec<u16> {
        let k = &rec.skeleton;
        let names: Vec<String> = k.bone_names.iter().map(|&b| self.string(b)).collect();
        let tags: Vec<String> = w.hide_tags.iter().map(|&t| self.string(t)).collect();
        hidden_bone_set(&names, usize::from(k.num_root_bones.max(1)), &k.parent_list, &tags)
    }

    /// bo2mp: how many of a view model's vertices the weapon's hide tags
    /// collapse when posed (a vertex with any weight on a hidden bone), and
    /// the hidden bones' names.
    fn hide_collapse(&self, w: &WeaponRec, rec: &ModelRec, f: &MeshFacts) -> (usize, Vec<String>) {
        let hidden = self.hidden_bones(w, rec);
        let collapsed = hidden.iter().filter_map(|&b| f.bone_verts.get(usize::from(b))).sum();
        let names = hidden.iter().map(|&b| rec.skeleton.bone_names.get(usize::from(b)).map_or_else(String::new, |&s| self.string(s))).collect();
        (collapsed, names)
    }

    /// bo2mp: the name of a model's root bone when the weapon's view model has
    /// a bone of that name (where the engine hangs a model given no tag).
    fn own_root_tag(&self, w: &WeaponRec, model: &str) -> Option<String> {
        let root = self.string(*self.cap.model(model)?.skeleton.bone_names.first()?);
        let gun = self.cap.model(&self.field_str(w, "gunModel")?)?;
        (!root.is_empty() && gun.skeleton.bone_names.iter().any(|&n| self.string(n) == root)).then_some(root)
    }

    /// bo2mp: a multiplayer gun's (`gun`) attachments from its uniques
    /// (`au_<gun>_<attachment>`): their view models at their placements
    /// (a model, its tag, origin and angles, 0x30 apart from 0x78; the
    /// first two the view's, the others the world's), kept where the tag
    /// is a bone of the gun, and the animations they give it; with the
    /// animations' names to read.
    fn guest_attachments(&self, w: &WeaponRec, gun: &str) -> (Vec<asset_game::GuestAttachment>, Vec<String>) {
        use fastfile_t7::asset_type::AssetType;
        let class = bo2_profile::guest::BO3
            .iter()
            .find(|g| g.gun == gun)
            .map_or(super::guest_attach::GunClass::Rifle, |g| super::guest_attach::GunClass::from_group(g.group));
        let bones: Vec<String> = self
            .field_str(w, "gunModel")
            .and_then(|m| self.cap.model(&m))
            .map(|m| m.skeleton.bone_names.iter().map(|&b| self.string(b)).collect())
            .unwrap_or_default();
        let img = &self.zone.image;
        let f32_at = |at: usize| img.get(at..at + 4).map_or(0.0, |b| f32::from_le_bytes([b[0], b[1], b[2], b[3]]));
        let vec3 = |at: usize| [f32_at(at), f32_at(at + 4), f32_at(at + 8)];
        let prefix = format!("au_{gun}_");
        let mut offered: Vec<asset_game::GuestAttachment> = Vec::new();
        let mut names = Vec::new();
        for (f, v) in &w.fields {
            let (true, FieldValue::Asset { name: unique, .. }) = (f.starts_with("attachmentUniques["), v) else {
                continue;
            };
            let Some(kind) = unique
                .strip_prefix(&prefix)
                .filter(|k| !k.contains('+'))
                .and_then(|k| super::guest_attach::t7_kind(k, class))
            else {
                continue;
            };
            if offered.iter().any(|o| o.word == kind.word) {
                continue;
            }
            let links = self.cap.links(AssetType::AttachmentUnique, unique);
            let link = |key: &str| {
                links.iter().find(|(k, _)| k == key).and_then(|(_, v)| match v {
                    FieldValue::Asset { name, .. } | FieldValue::Str(name) => Some(name.clone()),
                })
            };
            let header = self.cap.unique_headers.get(unique).copied();
            // (Models that take the place of the gun's own parts, its
            // upper, handguard, stock or grip, are left off: the gun keeps
            // its own. A magazine takes the place of the gun's.)
            let magazine = matches!(kind.word, "extclip" | "dualclip");
            let modelled = kind.sight || magazine || matches!(kind.word, "grip" | "steadyaim" | "silencer" | "extbarrel");
            let mut views = Vec::new();
            for i in 0..2 {
                if !modelled {
                    break;
                }
                let (Some(model), Some(h)) = (link(&format!("model{i}")), header) else {
                    continue;
                };
                let tag = link(&format!("tag{i}")).unwrap_or_else(|| "tag_weapon".to_owned());
                if model.starts_with(',') || self.cap.model(&model).is_none() || !bones.iter().any(|b| b.eq_ignore_ascii_case(&tag)) {
                    continue;
                }
                let at = h + 0x78 + 0x30 * i;
                // (A spare magazine behind the gun is the reload's, held by
                // its animation, not hung on the gun.)
                if f32_at(at + 0x18) < 0.0 {
                    continue;
                }
                views.push((
                    model,
                    Some(asset_model::FpvMountOffset { tag, offset: vec3(at + 0x18), rotation: vec3(at + 0x24), on_arms: false }),
                ));
            }
            if kind.sight && views.is_empty() {
                continue;
            }
            let mut anims = Vec::new();
            for (k, v) in links {
                let (Some(slot), FieldValue::Str(anim)) = (k.strip_prefix("anim[").and_then(|k| k.strip_suffix(']')), v) else {
                    continue;
                };
                let Some(&(_, t6)) = slot.parse::<usize>().ok().and_then(|s| T7_TO_T6_ANIMS.iter().find(|(t7, _)| *t7 == s)) else {
                    continue;
                };
                if let Some(to) = asset_game::t6_anim_slot(t6) {
                    names.push(anim.clone());
                    anims.push((to, anim.to_ascii_lowercase()));
                }
            }
            let mut a = super::guest_attach::guest_attachment(kind, class, None, None, anims, Vec::new());
            a.view_models = views;
            if magazine && !a.view_models.is_empty() {
                a.replaces = (0..5)
                    .filter_map(|i| self.field_str(w, &format!("attachViewModel[{i}]")))
                    .filter(|m| m.contains("_mag_"))
                    .collect();
            }
            offered.push(a);
        }
        (offered, names)
    }

    /// The weapon's `WeaponDef` bytes (its own, or the variant's whose it shares).
    fn def_at(&self, w: &WeaponRec) -> Option<usize> {
        w.def_at.or_else(|| {
            let owner = w.def_shared_with.as_deref()?;
            self.cap.weapons.iter().find(|x| x.name == owner)?.def_at
        })
    }

    /// A string field of the weapon, else of another variant of the same
    /// gun that loaded the list (`attachViewModelTag[4]`).
    fn field_str(&self, w: &WeaponRec, field: &str) -> Option<String> {
        let own = |r: &WeaponRec| match r.field(field) {
            Some(FieldValue::Str(s)) => Some(s.clone()),
            Some(FieldValue::Asset { name, .. }) => Some(name.clone()),
            None => None,
        };
        // (Its own field, else the variant whose WeaponDef it shares, else
        // another variant of the same gun.)
        own(w)
            .or_else(|| {
                let shared = w.def_shared_with.as_deref()?;
                self.cap.weapons.iter().filter(|r| r.name == shared && r.name != w.name).find_map(own)
            })
            .or_else(|| {
                self.cap
                    .weapons
                    .iter()
                    .filter(|r| r.base_name == w.base_name && r.name != w.name)
                    .find_map(own)
            })
        .filter(|s| !s.is_empty() && !s.starts_with("<ref"))
    }

    /// The weapon as Black Ops II's `WeaponRef` (`catalog`: its name in the
    /// catalog, `ar_standard`; an upgraded Zombies gun `ar_standard_upgraded`).
    fn weapon_ref(&self, w: &WeaponRec, catalog: &str, display: &str) -> Option<asset_t6::WeaponRef> {
        let img = &self.zone.image;
        let variant7 = img.get(w.header_at..w.header_at + 0x300)?;
        let def7 = self.def_at(w).and_then(|d| img.get(d..d + 0x15f0))?;
        let mut variant = vec![0u8; v6::SIZE];
        let mut def = vec![0u8; d6::SIZE];
        for &(dst, at6, at7, n) in T7_TO_T6 {
            let (at6, at7, n) = (usize::from(at6), usize::from(at7), usize::from(n));
            let src = if at7 < 0x300 { variant7.get(at7..at7 + n) } else { def7.get(at7 - 0x300..at7 - 0x300 + n) };
            let out = if dst == 0 { &mut variant } else { &mut def };
            if let (Some(src), Some(out)) = (src, out.get_mut(at6..at6 + n)) {
                out.copy_from_slice(src);
            }
        }
        let i32_7 = |at7: usize| def7.get(at7 - 0x300..at7 - 0x300 + 4).map_or(0, |b| i32::from_le_bytes([b[0], b[1], b[2], b[3]]));
        let put = |buf: &mut Vec<u8>, at: usize, v: i32| {
            if let Some(b) = buf.get_mut(at..at + 4) {
                b.copy_from_slice(&v.to_le_bytes());
            }
        };
        // (HydraX's offsets: fireType 0x380, burstCount 0x11f8,
        // playerAnimType 0x368.)
        put(&mut def, d6::fireType, t7_fire_type(i32_7(0x380), i32_7(0x11f8)));
        put(&mut def, d6::playerAnimType, t7_player_anim_type(i32_7(0x368)));
        // The sprint's rhythm: Black Ops III's sprint and dive cycle scales
        // (0xa5c after sprintRot, 0.95 on the guns, 0.53 the minigun; 0xaa8
        // after dtpRot), its crouched one (0xa40) 0, as are the bob pairs it
        // has none of. Left 0, the step rhythm stopped while he sprinted (no
        // footsteps, no bob) and the gun's sprint bob was nothing: Black Ops
        // II's 1 for those.
        let scale = |at7: usize| {
            let s = f32::from_bits(i32_7(at7) as u32);
            if s.is_finite() && s > 0.0 { s } else { 1.0 }
        };
        let sprint = scale(0xa5c).to_bits() as i32;
        put(&mut def, d6::fSprintCycleScale, sprint);
        put(&mut def, d6::fDuckedSprintCycleScale, sprint);
        put(&mut def, d6::fDtpCycleScale, scale(0xaa8).to_bits() as i32);
        for bob in [d6::vSprintBob, d6::vDuckedSprintBob, d6::vDtpBob] {
            put(&mut def, bob, 1.0f32.to_bits() as i32);
            put(&mut def, bob + 4, 1.0f32.to_bits() as i32);
        }
        // Animations onto Black Ops II's slots.
        let mut xanims = vec![String::new(); T6_ANIM_SLOTS];
        for (slot, _, anim) in &w.anims {
            if let Some(&(_, t6)) = T7_TO_T6_ANIMS.iter().find(|(t7, _)| t7 == slot) {
                xanims[t6] = anim.to_ascii_lowercase();
            }
        }
        // Sound aliases onto Black Ops II's string fields (by name; Black
        // Ops III's looping fire is Black Ops II's per-shot fire when the gun
        // has no fire sound of its own).
        let s = |f: &str| self.field_str(w, f);
        let mut strings: Vec<(usize, String)> = Vec::new();
        let mut set = |off: usize, v: Option<String>| {
            if let Some(v) = v {
                strings.push((off, v));
            }
        };
        // (A gun whose fire sounds are not read from its def, the RK5 among
        // them, takes Black Ops III's own names for them: every gun's fire
        // lists are `wpn_<gun>_fire_npc` / `_plr` in mp_common.)
        let gun_name = if w.base_name.is_empty() { w.name.trim_end_matches("_mp") } else { w.base_name.as_str() };
        let by_name = |kind: &str| Some(format!("wpn_{gun_name}_fire_{kind}"));
        set(d6::fireSound, s("fireSound").or_else(|| s("loopFireSound")).or_else(|| by_name("npc")));
        set(d6::fireSoundPlayer, s("fireSoundPlayer").or_else(|| s("loopFireSoundPlayer")).or_else(|| by_name("plr")));
        set(d6::fireLastSound, s("lastShotSound").or_else(|| s("fireSound")).or_else(|| by_name("npc")));
        set(d6::fireLastSoundPlayer, s("lastShotSoundPlayer").or_else(|| s("fireSoundPlayer")).or_else(|| by_name("plr")));
        set(d6::emptyFireSound, s("emptyFireSound"));
        set(d6::emptyFireSoundPlayer, s("emptyFireSoundPlayer"));
        for (off, f) in [
            (d6::reloadSound, "reloadSound"),
            (d6::reloadSoundPlayer, "reloadSoundPlayer"),
            (d6::reloadEmptySound, "reloadEmptySound"),
            (d6::reloadEmptySoundPlayer, "reloadEmptySoundPlayer"),
            (d6::reloadStartSound, "reloadStartSound"),
            (d6::reloadStartSoundPlayer, "reloadStartSoundPlayer"),
            (d6::reloadEndSound, "reloadEndSound"),
            (d6::reloadEndSoundPlayer, "reloadEndSoundPlayer"),
            (d6::rechamberSound, "rechamberSound"),
            (d6::rechamberSoundPlayer, "rechamberSoundPlayer"),
            (d6::pullbackSound, "pullbackSound"),
            (d6::pullbackSoundPlayer, "pullbackSoundPlayer"),
            (d6::adsZoomSound, "adsZoomSound"),
            (d6::projExplosionSound, "projExplosionSound"),
            (d6::projectileSound, "projectileSound"),
            (d6::projIgnitionSound, "projIgnitionSound"),
        ] {
            set(off, s(f));
        }
        // The attached models (a magazine, a scope): view and world, with
        // their tags (Black Ops III leaves most empty: the gun's root),
        // offsets and rotations (HydraX: 0x78.., 0xb4.., 0xf0.., 0x12c..).
        let f32_v = |at: usize| variant7.get(at..at + 4).map_or(0.0, |b| f32::from_le_bytes([b[0], b[1], b[2], b[3]]));
        let vec3 = |at: usize| [f32_v(at), f32_v(at + 4), f32_v(at + 8)];
        let attached = |models: &str, tags: &str, ofs: usize, rot: usize| -> Vec<(usize, String, String, [f32; 3], [f32; 3])> {
            (0..5)
                .filter_map(|i| {
                    let model = s(&format!("{models}[{i}]"))?;
                    if model.starts_with(',') {
                        return None;
                    }
                    let tag = s(&format!("{tags}[{i}]")).unwrap_or_else(|| "tag_weapon".to_owned());
                    Some((i, model, tag, vec3(ofs + 12 * i), vec3(rot + 12 * i)))
                })
                .collect()
        };
        let location_damage = w.location_damage_at.and_then(|at| {
            let b = img.get(at..at + 21 * 4)?;
            Some(b.chunks_exact(4).map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]])).collect())
        });
        let model = |f: &str| s(f).filter(|m| !m.starts_with(','));
        Some(asset_t6::WeaponRef {
            name: catalog.to_owned(),
            variant,
            def,
            display_name: display.to_owned(),
            alt_weapon_name: String::new(),
            ammo_name: s("ammoName").unwrap_or_default(),
            clip_name: s("clipName").unwrap_or_default(),
            xanims,
            hide_tags: w.hide_tags.iter().map(|&t| self.string(t)).filter(|t| !t.is_empty()).collect(),
            attach_view_models: attached("attachViewModel", "attachViewModelTag", 0x78, 0xf0),
            attach_world_models: attached("attachWorldModel", "attachWorldModelTag", 0xb4, 0x12c),
            gun_models: model("gunModel").into_iter().collect(),
            world_models: model("worldModel").into_iter().collect(),
            strings,
            fx: Vec::new(),
            materials: Vec::new(),
            models: Vec::new(),
            tracers: Vec::new(),
            overlay_material: String::new(),
            overlay_material_low: String::new(),
            dpad_icon: String::new(),
            notetrack_sounds: Vec::new(),
            bounce_sounds: Vec::new(),
            parallel_bounce: None,
            perpendicular_bounce: None,
            location_damage,
            camo: String::new(),
        })
    }
}

/// The guns this match takes: each multiplayer gun (`<gun>_mp`, catalog
/// name `<gun>`) and, for Pack-a-Punch, its upgraded Zombies copy when
/// core_common has one (`<gun>_upgraded_zm`, catalog name
/// `<gun>_upgraded`).
fn chosen<'a>(cap: &'a Capture) -> Vec<(String, &'a WeaponRec)> {
    let mut out = Vec::new();
    for gun in BO3_GUNS {
        if let Some(w) = cap.weapons.iter().find(|w| w.name == format!("{gun}_mp")) {
            out.push(((*gun).to_owned(), w));
        }
        if let Some(w) = cap.weapons.iter().find(|w| w.name == format!("{gun}_upgraded_zm")) {
            out.push((format!("{gun}_upgraded"), w));
        }
    }
    out
}

/// bo2mp: a Workshop pack's guns: each roster gun the pack's zone has
/// (`WorkshopPack::zone_weapon`), catalog name the roster's (`cw_ak47`).
fn pack_chosen<'a>(cap: &'a Capture, pack: &WorkshopPack) -> Vec<(String, &'a WeaponRec)> {
    pack.guns
        .iter()
        .filter_map(|g| {
            let want = pack.zone_weapon(g.gun);
            cap.weapons.iter().find(|w| w.name.eq_ignore_ascii_case(&want)).map(|w| (g.gun.to_owned(), w))
        })
        .collect()
}

/// bo2mp: the animation slots a pack gun's own animations are found for
/// by name when its table's names do not name animations of the zone
/// (Black Ops III's slot, the endings tried in order). CoD Online 2's
/// tables read as garbage: its zone has a stretch the walk steps over, and
/// the names after it are references the walk places by fitted deltas,
/// which land wrong. Its guns' own animations are in the zone under
/// `vm_<gun>_<ending>` (`vm_g37h_idle`, `vm_g37h_runing`).
const ANIM_ENDINGS: &[(usize, &[&str])] = &[
    (1, &["idle"]),
    (2, &["idle_empty", "idle"]),
    (4, &["fire"]),
    (10, &["fire_last", "fire"]),
    (40, &["reload", "reloadstart"]),
    (41, &["reload_empty", "reload"]),
    (50, &["pullout"]),
    (51, &["pullout_first", "first_pullout", "pullout"]),
    (52, &["putaway"]),
    (57, &["pullout_quick", "pullout"]),
    (58, &["putaway"]),
    (72, &["runstart"]),
    (73, &["runing", "running", "run"]),
    (74, &["runover"]),
    (84, &["pronestart"]),
    (85, &["prone", "prone_f", "pronemoveon"]),
    (89, &["proneover"]),
    (104, &["fireads", "adsfire", "fire_ads"]),
    (106, &["fireads", "adsfire", "fire_ads"]),
    (109, &["slidestart", "silde"]),
    (111, &["slideing", "sildeing"]),
    (112, &["slideover", "sildeover"]),
    (173, &["adsup"]),
    (174, &["adsdown"]),
];

/// bo2mp: a pack gun with its animation slots named again where its table's
/// names are not animations of the zone (`ANIM_ENDINGS`): the gun's stem
/// from its view model's name (`g37h_vmgun`, `vmgun_famas`, `vm_spas12`,
/// `ak47_ol_vmgun`: the first of its words, joined or alone, for which
/// `vm_<stem>_idle` is in the zone). Returns the gun and how many slots
/// were named again.
fn repaired_anims(cap: &Capture, w: &WeaponRec) -> (WeaponRec, usize) {
    let mut out = w.clone();
    if out.anims.iter().any(|a| a.0 == 1 && cap.anim(&a.2).is_some()) {
        return (out, 0);
    }
    let Some(view) = (match w.field("gunModel") {
        Some(FieldValue::Asset { name, .. }) => Some(name.clone()),
        _ => None,
    }) else {
        return (out, 0);
    };
    let words: Vec<&str> = view.split('_').filter(|t| !matches!(*t, "vm" | "vmgun" | "wmgun" | "camo" | "")).collect();
    let mut stems: Vec<String> = Vec::new();
    for n in (1..=words.len()).rev() {
        for s in words.windows(n) {
            stems.push(s.join("_"));
        }
    }
    // (Every gun idle of the zone: `vm_<stem>_idle`, `viewmodel_<stem>_idle`.)
    let idles: Vec<(&str, &str)> = cap
        .anims
        .iter()
        .filter_map(|a| {
            let rest = a.name.strip_suffix("_idle")?;
            ["vm_", "viewmodel_"].into_iter().find_map(|p| rest.strip_prefix(p).map(|s| (p, s)))
        })
        .collect();
    // (Its stem, else one a word of the model's name starts or ends with, or
    // that starts with one: `csg12recluse_vmgun` plays `vm_csg12_*`,
    // `ak47_ol_vmgun` `vm_47_*`, `aa12_vmgun` `vm_aa12_grip_*`.)
    let exact = stems.iter().find_map(|s| idles.iter().find(|(_, i)| i == s));
    let near = || {
        idles
            .iter()
            .filter(|(_, i)| i.len() >= 2 && words.iter().any(|w| w.starts_with(i) || w.ends_with(i) || i.starts_with(&format!("{w}_"))))
            .max_by_key(|(_, i)| i.len())
    };
    let Some(&(prefix, stem)) = exact.or_else(near) else {
        return (out, 0);
    };
    let mut named = 0;
    out.anims.retain(|a| cap.anim(&a.2).is_some());
    for &(slot, endings) in ANIM_ENDINGS {
        if out.anims.iter().any(|a| a.0 == slot) {
            continue;
        }
        if let Some(anim) = endings.iter().map(|e| format!("{prefix}{stem}_{e}")).find(|n| cap.anim(n).is_some()) {
            let name = fastfile_t7::walk::weapon_fields::ANIM_NAMES.iter().find(|n| n.0 == slot).map_or_else(|| format!("anim{slot}"), |n| n.1.to_owned());
            out.anims.push((slot, name, anim));
            named += 1;
        }
    }
    out.anims.sort_by_key(|a| a.0);
    (out, named)
}

/// A gun's Create-a-Class group (`weapon_sniper`): its roster's, Black Ops
/// III's or the pack's.
fn gun_group(pack: Option<&WorkshopPack>, name: &str) -> &'static str {
    let roster = pack.map_or(bo2_profile::guest::BO3, |p| p.guns);
    roster.iter().find(|g| g.gun == name).map_or("weapon_assault", |g| g.group)
}

/// The most a Workshop pack gun's first bullet does in a Black Ops II match,
/// by its group: about Black Ops III's multiplayer guns of the class (the
/// KN-44's 42, the Kuda's 36, a sniper's 99 that kills with BO2's chest
/// multiplier). The packs' guns are their Zombies copies, whose numbers are
/// made for zombies (the Cold War AK-47's 230, the M82's 1000).
fn mp_damage_cap(group: &str) -> i32 {
    match group {
        "weapon_smg" => 36,
        "weapon_lmg" => 45,
        "weapon_sniper" => 100,
        "weapon_cqb" => 50,
        "weapon_pistol" => 38,
        _ => 42,
    }
}

/// bo2mp: a Workshop pack gun's Zombies numbers made multiplayer's, in
/// Black Ops II's `WeaponDef` (`def`) and `WeaponVariantDef` (`variant`):
///
/// - bullet damage: where the first (closest) step is past `mp_damage_cap`,
///   every step is scaled by the same factor (the fall-off keeps its shape),
///   none below 1; `minPlayerDamage` and `playerDamage` with it;
/// - explosions (launchers): inner damage at most 200, outer scaled with it;
/// - a direct hit's damage at most 300 (a rocket's 5000);
/// - the magazine 1 to 200 rounds (CoD Online 2's AK-47 says 960), the
///   start and reserve ammunition non-negative and the reserve at most
///   999;
/// - melee at most 135 (Black Ops III's knife-swipe guns).
///
/// Returns what changed, for the report.
fn normalise_zombies_numbers(def: &mut [u8], variant: &mut [u8], group: &str) -> Vec<String> {
    let get = |b: &[u8], at: usize| b.get(at..at + 4).map_or(0, |x| i32::from_le_bytes([x[0], x[1], x[2], x[3]]));
    let put = |b: &mut [u8], at: usize, v: i32| {
        if let Some(x) = b.get_mut(at..at + 4) {
            x.copy_from_slice(&v.to_le_bytes());
        }
    };
    let mut changed = Vec::new();
    let launcher = group == "weapon_launcher";
    let first = get(def, d6::damage);
    let cap = if launcher { 300 } else { mp_damage_cap(group) };
    if first > cap {
        let k = cap as f32 / first as f32;
        let scale = |v: i32| if v > 0 { ((v as f32 * k).round() as i32).max(1) } else { v };
        for i in 0..6 {
            let at = d6::damage + 4 * i;
            put(def, at, scale(get(def, at)));
        }
        for at in [d6::minPlayerDamage, d6::playerDamage] {
            put(def, at, scale(get(def, at)));
        }
        changed.push(format!("damage {first}->{cap}"));
    }
    let inner = get(def, d6::iExplosionInnerDamage);
    if inner > 200 {
        let k = 200.0 / inner as f32;
        put(def, d6::iExplosionInnerDamage, 200);
        put(def, d6::iExplosionOuterDamage, (get(def, d6::iExplosionOuterDamage) as f32 * k).round() as i32);
        changed.push(format!("explosion {inner}->200"));
    }
    let clip = get(variant, v6::iClipSize);
    if !(1..=200).contains(&clip) {
        put(variant, v6::iClipSize, clip.clamp(1, 200));
        changed.push(format!("magazine {clip}->{}", clip.clamp(1, 200)));
    }
    put(def, d6::iStartAmmo, get(def, d6::iStartAmmo).max(0));
    put(def, d6::iMaxAmmo, get(def, d6::iMaxAmmo).clamp(0, 999));
    let melee = get(def, d6::iMeleeDamage);
    if melee > 135 {
        put(def, d6::iMeleeDamage, 135);
    }
    changed
}

/// Black Ops III's guns from core_common (`z.pack` `None`), or a Workshop
/// pack's from its zone, their materials linked into `material_seed`.
/// `shared_paks`: the packages to read streamed data from (the packs'
/// shared set), else core_common's folder's; handed back.
fn load_t7_guests(z: T7Zone, material_seed: MaterialCatalog, shared_paks: Option<XpakSet>) -> (T7Guests, XpakSet) {
    let fp = match z.pack {
        None => super::helpers::gun_texture_cap("BO2MP_BO3_TEXTURE", FIRST_PERSON_MAX),
        Some(_) => super::helpers::gun_texture_cap("BO2MP_WORKSHOP_TEXTURE", WORKSHOP_FIRST_PERSON_MAX),
    };
    let caps = [("fp", fp), ("world", WORLD_MAX), ("camo", CAMO_MAX)];
    let code = [include_str!("finished_gun_cache.rs"), include_str!("t7.rs"), include_str!("t7_gun_cache.rs")];
    let files = t7_source_files(&z.dir, &z.path);
    let zone_name = z.path.file_stem().map_or_else(|| "core_common".to_owned(), |s| s.to_string_lossy().into_owned());
    let names = vec![zone_name.clone()];
    let src = super::finished_gun_cache::Sources {
        lane: "t7",
        name: &zone_name,
        files: &files,
        caps: &caps,
        code: &code,
        zones: &names,
        note: "",
        missing_ok: false,
    };
    let at = super::finished_gun_cache::MatAt::of(&material_seed);
    if let Some(body) = super::finished_gun_cache::read_body(&src) {
        match unpack_t7(&body, material_seed) {
            Ok(guests) => {
                if let Some(dir) = map_bake::map_dir("t7", &zone_name) {
                    let path = dir.join("finished_guns.pak");
                    for zone in &names {
                        diag::info!(World, "t7 guest guns: {zone} reads {}", path.display());
                    }
                }
                let paks = match shared_paks {
                    Some(paks) => paks,
                    None => XpakSet::open_dir(&z.dir).map(|(paks, _)| paks).unwrap_or_default(),
                };
                return (guests, paks);
            }
            Err(seed) => return finish_t7(load_t7_guests_now(z, seed, shared_paks), &src, at),
        }
    }
    finish_t7(load_t7_guests_now(z, material_seed, shared_paks), &src, at)
}

fn finish_t7(built: (T7Guests, XpakSet), src: &super::finished_gun_cache::Sources<'_>, at: super::finished_gun_cache::MatAt) -> (T7Guests, XpakSet) {
    match pack_t7(&built.0, at) {
        Ok(bytes) => super::finished_gun_cache::write_logged(src, &bytes),
        Err(why) => diag::warn!(World, "t7 guest guns: not cached ({why})"),
    }
    built
}

fn t7_source_files(dir: &Path, zone: &Path) -> Vec<PathBuf> {
    let mut files = vec![zone.to_path_buf()];
    if let Ok(read) = std::fs::read_dir(dir) {
        for entry in read.flatten() {
            let name = entry.file_name().to_string_lossy().to_ascii_lowercase();
            if name.ends_with(".xpak") {
                files.push(entry.path());
            }
        }
    }
    files
}

fn pack_t7(guests: &T7Guests, at: super::finished_gun_cache::MatAt) -> Result<Vec<u8>, String> {
    let projectiles = asset_model::ProjectileMeshBuild::default();
    let fx = asset_game::FxCatalog::default();
    let fx_models = asset_game::FxModelCatalog::default();
    let tracers = asset_game::TracerCatalog::default();
    super::finished_gun_cache::pack_parts(&super::finished_gun_cache::Parts {
        weapons: &guests.weapons,
        fpv: &guests.fpv,
        world: &guests.world_guns,
        projectiles: &projectiles,
        xanims: &guests.xanims,
        fx: &fx,
        fx_models: &fx_models,
        tracers: &tracers,
        impact: None,
        materials: &guests.material_seed,
        mat_at: at,
        reports: &guests.report,
        extra: &[],
    })
}

fn unpack_t7(bytes: &[u8], seed: MaterialCatalog) -> Result<T7Guests, MaterialCatalog> {
    match super::finished_gun_cache::unpack_parts(bytes, seed) {
        Ok(rest) => Ok(T7Guests {
            weapons: rest.weapons,
            fpv: rest.fpv,
            world_guns: rest.world,
            xanims: rest.xanims,
            material_seed: rest.materials,
            report: rest.reports,
        }),
        Err(seed) => Err(seed),
    }
}

fn load_t7_guests_now(z: T7Zone, material_seed: MaterialCatalog, shared_paks: Option<XpakSet>) -> (T7Guests, XpakSet) {
    let T7Zone { dir, path, pack, zone, cap, mut report, started } = z;
    let label = pack.map_or_else(|| "t7 guest guns".to_owned(), pack_label);
    if let Ok(mut all) = MESH_FACTS.lock() {
        all.clear();
    }
    let build_started = std::time::Instant::now();
    let mut phases: Vec<(&str, f32)> = Vec::new();
    let paks = match shared_paks {
        Some(paks) => paks,
        None => match XpakSet::open_dir(&dir) {
            Ok((paks, bad)) => {
                if !bad.is_empty() {
                    report.push(format!("{label}: {} packages unreadable", bad.len()));
                }
                paks
            }
            Err(e) => {
                report.push(format!("{label}: packages: {e} (streamed meshes and pictures left out)"));
                XpakSet::default()
            }
        },
    };
    let mut b = Builder {
        zone: &zone,
        cap: &cap,
        paks,
        catalog: material_seed,
        owner: ZoneOwner::from_zone_path(&path),
        materials: HashMap::new(),
        images: HashMap::new(),
        image_failures: Vec::new(),
        decoded: 0,
        ready: HashMap::new(),
        pic_cache: None,
        pic_cache_hits: 0,
    };
    // (A pack's guns are many - Cold War's 52 - each with several 2048
    // pictures: at Black Ops III's size they would take gigabytes. 512 by
    // default, `BO2MP_WORKSHOP_TEXTURE=<size>`.)
    let first_person_max = match pack {
        None => super::helpers::gun_texture_cap("BO2MP_BO3_TEXTURE", FIRST_PERSON_MAX),
        Some(_) => super::helpers::gun_texture_cap("BO2MP_WORKSHOP_TEXTURE", WORKSHOP_FIRST_PERSON_MAX),
    };
    b.pic_cache = if std::env::var("BO2MP_BO3_GUN_CACHE").map_or(true, |v| v.trim() != "0") {
        match pack {
            None => super::t7_gun_cache::key_for(&dir, first_person_max),
            Some(p) => {
                let mut files = vec![path.clone()];
                files.extend(asset_transport::workshop::bo3_mod(pack.map_or("", |p| p.workshop_id)).map(|m| m.xpaks.clone()).unwrap_or_default());
                super::t7_gun_cache::key_for_files(p.game, &files, first_person_max)
            }
        }
    } else {
        None
    };
    // (A pack gun whose animation table's names are not the zone's, CoD
    // Online 2's, has its own found by name: `repaired_anims`.)
    let repaired: Vec<(String, WeaponRec, usize)> = pack
        .map(|p| {
            pack_chosen(&cap, p)
                .into_iter()
                .map(|(n, w)| {
                    let (w, k) = repaired_anims(&cap, w);
                    (n, w, k)
                })
                .collect()
        })
        .unwrap_or_default();
    let guns: Vec<(String, &WeaponRec)> = match pack {
        None => chosen(&cap),
        Some(_) => repaired.iter().map(|(n, w, _)| (n.clone(), w)).collect(),
    };
    if let Some(p) = pack {
        let missing: Vec<&str> = p.guns.iter().map(|g| g.gun).filter(|g| !guns.iter().any(|(n, _)| n == g)).collect();
        report.push(format!("{label}: {} of {} roster guns in {} (not there: {missing:?})", guns.len(), p.guns.len(), path.display()));
        let renamed: Vec<String> = repaired.iter().filter(|r| r.2 > 0).map(|(n, _, k)| format!("{n} {k}")).collect();
        if !renamed.is_empty() {
            report.push(format!("{label}: animation slots found by name on {} guns (their table's names unread): {}", renamed.len(), renamed.join(", ")));
        }
    }
    // bo2mp: each multiplayer gun's attachments (its uniques), their view
    // models and the animations they give it. (A pack's guns name Black Ops
    // III's stock attachments, the AP9's: none are offered.)
    let attachments: Vec<(String, Vec<asset_game::GuestAttachment>, Vec<String>)> = guns
        .iter()
        .filter(|(name, _)| pack.is_none() && !name.ends_with("_upgraded"))
        .map(|(name, w)| {
            let (offered, anims) = b.guest_attachments(w, name);
            (name.clone(), offered, anims)
        })
        .collect();

    // Models: each gun's view and world model and the models it hangs on
    // them (its magazine), and its attachments' view models.
    let mut first_person: BTreeSet<String> = BTreeSet::new();
    let mut third_person: BTreeSet<String> = BTreeSet::new();
    for (_, offered, _) in &attachments {
        first_person.extend(offered.iter().flat_map(|a| a.view_models.iter().map(|(m, _)| m.clone())));
    }
    for (_, w) in &guns {
        for (f, v) in &w.fields {
            let FieldValue::Asset { name, .. } = v else { continue };
            // (A scope's model for aiming through it in Black Ops III: the
            // gun shows Black Ops II's scope instead.)
            if name.starts_with(',') || name.starts_with("<ref") || is_ads_overlay(name, pack.is_some()) {
                continue;
            }
            if f == "gunModel" || f.starts_with("attachViewModel[") {
                first_person.insert(name.clone());
            } else if f == "worldModel" || f.starts_with("attachWorldModel[") {
                third_person.insert(name.clone());
            }
        }
    }
    phases.push(("packages", build_started.elapsed().as_secs_f32()));
    // The models' meshes (each its densest LOD, as `model_ref` reads it)
    // and their materials' pictures, read and converted on several
    // threads; then linked in order, one at a time, below.
    let sets = [(&first_person, first_person_max, true), (&third_person, WORLD_MAX, false)];
    let names: Vec<&String> = sets.iter().flat_map(|(set, ..)| set.iter()).collect();
    let mut refs: HashMap<&str, Result<(asset_t6::XModelRef, Vec<Option<String>>), String>> = {
        let next = std::sync::atomic::AtomicUsize::new(0);
        let threads = std::thread::available_parallelism().map_or(4, |n| n.get()).clamp(2, 16);
        let b = &b;
        let cap = &cap;
        std::thread::scope(|s| {
            let workers: Vec<_> = (0..threads)
                .map(|_| {
                    s.spawn(|| {
                        let mut out = Vec::new();
                        loop {
                            let k = next.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                            let Some(name) = names.get(k) else { break };
                            if let Some(rec) = cap.model(name) {
                                out.push((name.as_str(), b.model_ref(rec)));
                            }
                        }
                        out
                    })
                })
                .collect();
            workers.into_iter().flat_map(|w| w.join().unwrap_or_default()).collect()
        })
    };
    let mut wanted_materials: Vec<(String, u32)> = Vec::new();
    for (set, max, _) in &sets {
        for name in set.iter() {
            if let Some(Ok((_, mats))) = refs.get(name.as_str()) {
                wanted_materials.extend(mats.iter().flatten().map(|m| (m.clone(), *max)));
            }
        }
    }
    b.make_ahead(&wanted_materials);
    phases.push(("made ahead", build_started.elapsed().as_secs_f32()));
    let mut fpv = FpvMeshBuild::default();
    fpv.set_capture_ns(NS);
    let mut world_guns = WorldWeaponBuild::default();
    world_guns.set_capture_ns(NS);
    let mut unreadable = Vec::new();
    for (set, max, first) in sets {
        for name in set {
            let Some(rec) = cap.model(name) else {
                unreadable.push(format!("{name} (not in {})", path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()));
                continue;
            };
            // (A model in both sets is read again the second time.)
            let read = match refs.remove(name.as_str()) {
                Some(r) => r,
                None => b.model_ref(rec),
            };
            let (xref, mats) = match read {
                Ok(x) => x,
                Err(e) => {
                    unreadable.push(format!("{name} ({e})"));
                    continue;
                }
            };
            // Surface materials: local key = surface index.
            let linked: Vec<Option<usize>> = mats.iter().map(|m| m.as_deref().and_then(|m| b.material(m, max))).collect();
            let mut xref = xref;
            let key = |k: usize| asset_t6::AssetKey { ty: fastfile_t6::AssetType::Material, index: k };
            for (i, s) in xref.lod0.iter_mut().enumerate() {
                s.material = linked.get(i).copied().flatten().map(|_| key(i));
            }
            xref.materials = xref.lod0.iter().map(|s| s.material).collect();
            match asset_model::capture_model_skel_t6(&xref, |k| linked.get(k.index).copied().flatten()) {
                Some(skel) if first => fpv.insert_in(NS, skel, Some(&b.catalog)),
                Some(skel) => world_guns.insert_in(NS, skel, Some(&b.catalog)),
                None => unreadable.push(format!("{name} (unreadable)")),
            }
        }
    }
    // Black Ops III's arm skeleton, which the guns' view animations pose:
    // the first person view carries those poses onto Black Ops II's arms
    // (`asset_game::FpvArmsRetarget`). Its mesh is never drawn. (A pack's
    // guns pose the same arms: core_common's build puts them in.)
    let reference = cap
        .models
        .iter()
        .filter(|m| pack.is_none() && m.name.starts_with("c_hro_player_male_") && m.name.ends_with("_viewhands"))
        .find_map(|rec| {
            let (mut xref, _) = b.model_ref(rec).ok()?;
            for s in &mut xref.lod0 {
                s.material = None;
            }
            xref.materials.clear();
            xref.name = asset_game::T7_REFERENCE_ARMS.to_owned();
            let skel = asset_model::capture_model_skel_t6(&xref, |_| None)?;
            let names = &skel.bone_names;
            names.iter().any(|n| n == "tag_weapon_right").then_some((rec.name.clone(), skel))
        });
    match reference {
        Some((from, skel)) => {
            fpv.insert_in(NS, skel, None);
            report.push(format!("{label}: arms skeleton from {from}"));
        }
        None if pack.is_some() => {}
        None => report.push(format!("{label}: no Black Ops III arms skeleton (view animations stay on BO2's arms as they are)")),
    }
    report.push(format!(
        "{label}: models: {} first person, {} third person built; {} not {:?}",
        fpv.len(),
        world_guns.len(),
        unreadable.len(),
        &unreadable[..unreadable.len().min(8)]
    ));
    report.push(format!(
        "{label}: materials: {} linked, {} pictures decoded ({} from the cache), {} failed {:?}",
        b.materials.values().filter(|m| m.is_some()).count(),
        b.decoded,
        b.pic_cache_hits,
        b.image_failures.len(),
        &b.image_failures[..b.image_failures.len().min(6)]
    ));

    phases.push(("models", build_started.elapsed().as_secs_f32()));
    // Animations every gun names.
    let mut xanims = XAnimBuild::default();
    xanims.set_capture_ns(NS);
    let mut wanted: BTreeSet<&str> = guns.iter().flat_map(|(_, w)| w.anims.iter().map(|a| a.2.as_str())).collect();
    wanted.extend(attachments.iter().flat_map(|(_, _, anims)| anims.iter().map(String::as_str)));
    let (mut anim_n, mut anim_refused) = (0usize, Vec::new());
    for name in &wanted {
        match cap.anim(name).map(|a| (a, b.anim_ref(a))) {
            Some((_, Some(r))) => anim_n += usize::from(xanims.insert_t6_as(NS, &r)),
            Some((a, None)) => anim_refused.push(format!("{} ({} bones)", a.name, a.names.len())),
            None => anim_refused.push(format!("{name} (not kept)")),
        }
    }
    report.push(format!(
        "{label}: animations: {anim_n} of {} named kept ({} gaps; {} left out {:?})",
        wanted.len(),
        xanims.capture_gaps,
        anim_refused.len(),
        &anim_refused[..anim_refused.len().min(6)]
    ));

    phases.push(("animations", build_started.elapsed().as_secs_f32()));
    // The weapon catalog, through Black Ops II's own capture.
    let mut catalog = WeaponCatalog::default();
    catalog.set_capture_ns(NS);
    let mut no_def = Vec::new();
    let mut scopes = Vec::new();
    let mut normalised = Vec::new();
    let mut hide_guarded = Vec::new();
    for (name, w) in &guns {
        let base = name.strip_suffix("_upgraded").unwrap_or(name);
        let display = format!(
            "WEAPON_BO3_{}{}",
            base.to_ascii_uppercase(),
            if name.ends_with("_upgraded") { "_UPGRADED" } else { "" }
        );
        match b.weapon_ref(w, name, &display) {
            Some(mut r) => {
                let before: Vec<(String, String)> = r.attach_view_models.iter().map(|(_, m, tag, ..)| (m.clone(), tag.clone())).collect();
                r.attach_view_models.retain(|(_, m, ..)| !is_ads_overlay(m, pack.is_some()));
                let dropped: Vec<String> = before.iter().filter(|(m, _)| is_ads_overlay(m, pack.is_some())).map(|(m, _)| m.clone()).collect();
                if pack.is_some() {
                    diag::info!(World, "{}", b.pack_gun_line(name, w, &before, &dropped));
                }
                // bo2mp: hide tags that would collapse most of the gun (a tag
                // that is an ancestor of the body's bones, or body vertices
                // weighted to it) are left off: the gun shows whole with its
                // unlit sight rather than vanish (`BO2MP_T7_HIDE_GUARD=0` keeps
                // them). Logged either way; the first report of an invisible
                // pack gun (the RAM-7) has no other cause left in the zone.
                if let Some((view, rec, f)) = b.field_str(w, "gunModel").and_then(|v| Some((v.clone(), b.cap.model(&v)?, mesh_facts(&v)?))) {
                    let (collapsed, with) = b.hide_collapse(w, rec, &f);
                    if hide_guard_on() && f.verts > 0 && collapsed * 2 > f.verts {
                        diag::warn!(World, "bo2mp t7 gun {name}: hide tags {:?} would collapse {collapsed} of {} vertices of {view} (bones {with:?}): left off", r.hide_tags, f.verts);
                        hide_guarded.push(name.clone());
                        r.hide_tags.clear();
                    }
                }
                let group = gun_group(pack, name);
                // bo2mp: a pack gun's Zombies numbers as multiplayer's.
                if pack.is_some() {
                    let changed = normalise_zombies_numbers(&mut r.def, &mut r.variant, group);
                    if !changed.is_empty() {
                        normalised.push(format!("{name} {}", changed.join(" ")));
                    }
                }
                // bo2mp: a sniper's own scope comes off with a sight; aimed,
                // the gun shows Black Ops II's sniper scope.
                if group == "weapon_sniper" {
                    let scope: Vec<(String, asset_model::FpvMountOffset)> = r
                        .attach_view_models
                        .iter()
                        .filter(|(_, m, ..)| is_own_scope_model(m, pack.is_some()))
                        .map(|(_, m, tag, offset, rotation)| {
                            // bo2mp: a scope the weapon names no tag for was put on
                            // `tag_weapon`, the gun's origin; the engine hangs a model
                            // given no tag on its own root bone where the gun has a bone
                            // of that name (the CheyTac's scope: root `tag_scope`, 4.0
                            // back and 3.8 up of the origin).
                            let tag = if pack.is_some() && pack_fix_on() && tag == "tag_weapon" {
                                b.own_root_tag(w, m).unwrap_or_else(|| tag.clone())
                            } else {
                                tag.clone()
                            };
                            let at = asset_model::FpvMountOffset { tag, offset: *offset, rotation: *rotation, on_arms: false };
                            (m.clone(), at)
                        })
                        .collect();
                    r.attach_view_models.retain(|(_, m, ..)| !is_own_scope_model(m, pack.is_some()));
                    scopes.push((name.clone(), super::guest_attach::own_scope_placed(scope)));
                }
                catalog.capture_t6(&r, None, &HashMap::new());
            }
            None => no_def.push(w.name.clone()),
        }
    }
    let captured = catalog.len();
    let mut weapons = catalog.into_build();
    weapons.stamp_namespace(NS);
    for (gun, scope) in scopes {
        if let Some(id) = weapons.resolve_index(&gun).ok().flatten().filter(|&i| i != 0) {
            weapons.give_guest_scope(id, scope);
        }
    }
    for (gun, offered, _) in attachments {
        let Some(id) = weapons.resolve_index(&gun).ok().flatten().filter(|&i| i != 0) else {
            continue;
        };
        report.push(format!("{label}: {gun} attachments: {}", super::guest_attach::words(&offered)));
        weapons.offer_guest_attachments(id, offered);
    }
    weapons.resolve_sz_xanim_edges(&xanims);
    weapons.resolve_fpv_mesh_edges(&fpv);
    weapons.resolve_world_model_edges(&world_guns);
    if !normalised.is_empty() {
        report.push(format!("{label}: Zombies numbers made multiplayer's on {} guns: {}", normalised.len(), normalised.join("; ")));
    }
    if !hide_guarded.is_empty() {
        report.push(format!("{label}: hide tags left off {} guns they would have collapsed: {}", hide_guarded.len(), hide_guarded.join(", ")));
    }
    report.push(format!(
        "{label}: {captured} weapons captured, {} in the catalog ({} without a WeaponDef {:?}) [{:.1} s]",
        weapons.len(),
        no_def.len(),
        &no_def[..no_def.len().min(6)],
        started.elapsed().as_secs_f64()
    ));
    phases.push(("weapons", build_started.elapsed().as_secs_f32()));
    let ns = |a: &std::sync::atomic::AtomicU64| a.load(std::sync::atomic::Ordering::Relaxed) as f64 / 1e9;
    report.push(format!(
        "{label}: built in {:.1} s ({}); pictures: levels read {:.1} s (all threads), {:.1} s making those not made ahead",
        build_started.elapsed().as_secs_f32(),
        phases.iter().map(|(p, at)| format!("{p} at {at:.1} s")).collect::<Vec<_>>().join(", "),
        ns(&PROBE_LEVELS),
        ns(&PROBE_MAKE),
    ));
    // (Black Ops III's camos are core_common's, put in once.)
    if pack.is_none() {
        t7_camos(&mut b, &mut report);
    }
    let Builder { catalog: material_seed, paks, .. } = b;
    (T7Guests { weapons, fpv, world_guns, xanims, material_seed, report }, paks)
}

/// The largest level kept of a Black Ops III camo's picture (a pattern laid
/// over a first-person gun, tiled twice): 512 by default, now that the 27
/// pictures reach the GPU with every map (`BO2MP_T7_CAMO_TEXTURE=<size>`;
/// 1024 was the size while they never got there).
const CAMO_MAX: u32 = 512;

fn camo_max() -> u32 {
    codlegacy_env::var("BO2MP_T7_CAMO_TEXTURE")
        .ok()
        .and_then(|v| v.trim().parse::<u32>().ok())
        .filter(|v| (64..=4096).contains(v))
        .unwrap_or(CAMO_MAX)
}

/// bo2mp: the picture a Black Ops III camo material is drawn with as a
/// pattern, when `T7_CAMOS` names none it has: its colour map (`_c`), else
/// a metal camo's specular colour (`_s`: Red Hex, Etching, whose colour map
/// is `$black_color`), else its glow (`_e*`: Dark Matter).
fn camo_picture(cap: &Capture, material: &str) -> Option<String> {
    let m = cap.material(material)?;
    let real: Vec<&str> = m
        .textures
        .iter()
        .map(|(_, n)| n.trim_start_matches(','))
        .filter(|n| !n.starts_with('$') && cap.image(n).is_some())
        .collect();
    ["_c", "_s", "_e2", "_e"]
        .iter()
        .find_map(|end| real.iter().find(|n| n.ends_with(end)))
        .map(|n| (*n).to_owned())
}

/// bo2mp: Black Ops III's camos (`asset_game::weapon_camo_models::T7_CAMOS`),
/// each its material's picture laid over a whole gun material, on every
/// game's guns (`asset_core::weapon_camos::T7_FIRST`); the pictures read on
/// the machine's threads, then linked.
fn t7_camos(b: &mut Builder<'_>, report: &mut Vec<String>) {
    let list: Vec<(u32, String)> = asset_game::weapon_camo_models::T7_CAMOS
        .iter()
        .filter_map(|(n, _, _, material, picture)| {
            let picture = b.cap.image(picture).map(|_| (*picture).to_owned()).or_else(|| camo_picture(b.cap, material))?;
            Some((asset_core::weapon_camos::T7_FIRST + u32::from(*n), picture))
        })
        .collect();
    // (Diamond's and Ritual's glow pictures too, for the special camos that
    // move them: the first of their material's `_e` pictures.)
    let mut list = list;
    for (n, material) in [(16u32, "mc/mtl_t7_camo_diamond_inlay"), (26, "mc/mtl_t7_camo_zod_pap")] {
        let glow = b.cap.material(material).and_then(|m| {
            m.textures
                .iter()
                .map(|(_, name)| name.trim_start_matches(','))
                .find(|name| !name.starts_with('$') && name.ends_with("_e") && b.cap.image(name).is_some())
                .map(str::to_owned)
        });
        if let Some(glow) = glow {
            list.push((asset_core::weapon_camos::T7_GLOW_FIRST + n, glow));
        }
    }
    let key = |p: &str| format!("t7_camo:{p}");
    let mut jobs: Vec<&String> = list.iter().map(|(_, p)| p).collect();
    jobs.sort();
    jobs.dedup();
    let (cap, zone, paks) = (b.cap, &b.zone.image[..], &b.paks);
    let camo_max = camo_max();
    let next = std::sync::atomic::AtomicUsize::new(0);
    let threads = std::thread::available_parallelism().map_or(4, |n| n.get()).clamp(2, 8);
    let made: Vec<(String, Result<Made, String>)> = std::thread::scope(|s| {
        let workers: Vec<_> = (0..threads)
            .map(|_| {
                s.spawn(|| {
                    let mut out = Vec::new();
                    while let Some(p) = jobs.get(next.fetch_add(1, std::sync::atomic::Ordering::Relaxed)) {
                        out.push(((*p).clone(), make_picture(cap, zone, paks, &Recipe::Levels((*p).clone()), camo_max, true)));
                    }
                    out
                })
            })
            .collect();
        workers.into_iter().flat_map(|w| w.join().unwrap_or_default()).collect()
    });
    for (p, r) in made {
        b.ready.insert((key(&p), camo_max), r);
    }
    let mut patterns = HashMap::new();
    for (option, picture) in &list {
        if let Some(slot) = b.link_image(key(picture), camo_max, TS_COLOR_MAP, |b| b.levels(picture, camo_max)) {
            patterns.insert(*option, asset_core::weapon_camos::PatternCamo::still(slot as u32, 2.0, true));
        }
    }
    report.push(format!(
        "t7 camos: {} of {} drawn on every gun ({} without a picture in core_common)",
        patterns.len(),
        asset_game::weapon_camo_models::T7_CAMOS.len(),
        asset_game::weapon_camo_models::T7_CAMOS.len() - list.len()
    ));
    asset_core::weapon_camos::install_t7_patterns(patterns);
}

/// bo2mp: a Black Ops III map's scene props (its `scriptbundle_scene`
/// fxanims: Nuk3town's derezzing buildings), in Black Ops II's form under
/// the T6 namespace, which the script models' draw and pose look up: each
/// model with its skeleton, each animation and its timing row.
pub(crate) struct T7MapProps {
    pub fpv: FpvMeshBuild,
    pub xanims: XAnimBuild,
    pub anims: Vec<crate::T6AnimFacts>,
    /// Per scene bundle asked for, its (model, animation).
    pub props: Vec<(String, String, String)>,
    /// bo2mp: the AI types asked for, each its characters (`t7_cast`).
    pub cast: Vec<crate::T7AiCast>,
    pub material_seed: MaterialCatalog,
    pub report: Vec<String>,
}

/// bo2mp: the zombies' own animations a Black Ops III Zombies map's actors
/// play (`sim::t6::t7_ai::ZOMBIE_CLIPS` picks among them): its base
/// archetype's locomotion, attacks, board tears, window traversals,
/// deaths and taunts. (The animation state machine's selector tables pick
/// these by blackboard in the game; the sim picks by name.)
fn zombie_clip(name: &str) -> bool {
    let Some(rest) = name.strip_prefix("ai_zombie_base_").or_else(|| name.strip_prefix("bo3_ai_zombie_")) else { return false };
    const KEEP: &[&str] = &[
        "walk_ad_v", "run_ad_v", "sprint_ad_v", "supersprint_ad_v", "idle_ad_v", "crawl_v", "crawl_sprint_v", "crawl_idle_v",
        "ad_attack_v", "walk_ad_attack_v", "run_ad_attack_v", "crawl_attack_v", "boardtear_aligned_m_1_", "crawl_boardtear_aligned_m_1_",
        "win_attack_arm_", "walk_ad_win_trav_m_v1", "run_ad_win_trav_m_v1", "sprint_ad_win_trav_m_v1", "crawl_win_trav_m_v1",
        "death_v", "crawl_death_v", "taunts_", "traverse_ground_", "ground_rise",
    ];
    // (A `_v` pattern only its numbered variants: `walk_ad_v2`, not
    // `walk_ad_v2_pain_chest_f`.)
    KEEP.iter().any(|k| match rest.strip_prefix(k) {
        Some(tail) if k.ends_with("_v") => !tail.is_empty() && tail.bytes().all(|b| b.is_ascii_digit()),
        Some(_) => true,
        None => false,
    })
}

/// bo2mp: an AI type's characters (`aitype` 0x4b8.., eight slots), each
/// its body (`character` 0x60), its heads (0x70 a model, else 0x78 a model
/// alias's list) and its other attached models (0x88 the hat, 0x98).
fn ai_cast(cap: &fastfile_t7::walk::capture::Capture, aitype: &str) -> Option<crate::T7AiCast> {
    use fastfile_t7::asset_type::AssetType as T;
    let bare = |n: &str| n.trim_start_matches(',').to_owned();
    let fields = |ty: T, name: &str| cap.impact_fields.get(&(ty, name.to_owned())).or_else(|| cap.impact_fields.get(&(ty, format!(",{name}"))));
    let at = |f: &Vec<(usize, String)>, o: usize| f.iter().find(|x| x.0 == o).map(|x| bare(&x.1));
    let ai = fields(T::AiType, aitype)?;
    let mut characters = Vec::new();
    for o in (0x4b8..=0x4f0).step_by(8) {
        let Some(c) = at(ai, o) else { continue };
        let Some(cf) = fields(T::Character, &c) else { continue };
        let Some(body) = at(cf, 0x60) else { continue };
        let heads = match at(cf, 0x70) {
            Some(h) => vec![h],
            None => at(cf, 0x78).and_then(|a| fields(T::XModelAlias, &a)).map(|f| f.iter().map(|x| bare(&x.1)).collect()).unwrap_or_default(),
        };
        let attached = [0x88, 0x98].iter().filter_map(|&o| at(cf, o)).collect();
        characters.push(crate::T7Character { name: c, body, heads, attached });
    }
    Some(crate::T7AiCast { aitype: aitype.to_owned(), characters })
}

/// What [`map_props`] reads before it builds: the map zone again, walked
/// keeping its animations, and the packages beside it. (Lane B 11-05: read
/// ahead on a thread of its own beside the light and the materials,
/// `build_t7_world`; the same reads, so the same props.)
pub(crate) struct PropsRead {
    zone: fastfile_t7::Zone,
    walk: fastfile_t7::walk::Walk,
    paks: XpakSet,
    secs: f32,
    /// (Package entries read ahead, their bytes, the seconds it took.)
    warmed: (usize, usize, f32),
}

/// `bundles` and `plain`: the props whose package bytes to read ahead
/// ([`warm_props`]); empty, none.
pub(crate) fn props_read(zone_path: &Path, bundles: &[String], plain: &[String]) -> Result<PropsRead, String> {
    let started = std::time::Instant::now();
    let zone = fastfile_t7::Zone::open(zone_path).map_err(|e| format!("t7 scene props: {}: {e}", zone_path.display()))?;
    let walk = fastfile_t7::walk::walk_with(&zone, fastfile_t7::walk::Options { keep_anims: true, ..Default::default() });
    let dir = zone_path.parent().map(Path::to_path_buf).unwrap_or_default();
    let paks = XpakSet::open_dir(&dir).map(|(p, _)| p).unwrap_or_default();
    let secs = started.elapsed().as_secs_f32();
    let warm = std::time::Instant::now();
    let models = bundles.iter().flat_map(|b| bundle_objects(&zone, &walk.capture, b)).map(|(m, _)| m).chain(plain.iter().cloned());
    let (n, bytes) = warm_props(&walk.capture, &paks, models);
    Ok(PropsRead { zone, walk, paks, secs, warmed: (n, bytes, warm.elapsed().as_secs_f32()) })
}

/// The objects of the scene `bundle` with both a model and a main animation
/// (`walk::bundle`'s links: `object<i>#<key>#<kind>`, kind 6 a model, 3 an
/// animation), in object order.
fn bundle_objects(zone: &fastfile_t7::Zone, cap: &Capture, bundle: &str) -> Vec<(String, String)> {
    let links = cap.links(fastfile_t7::asset_type::AssetType::ScriptBundle, bundle);
    let mut objects: std::collections::BTreeMap<&str, (Option<String>, Option<String>)> = std::collections::BTreeMap::new();
    for (label, v) in links {
        let mut parts = label.split('#');
        let (who, kind) = (parts.next().unwrap_or(""), parts.nth(1).unwrap_or(""));
        if !who.starts_with("object") {
            continue;
        }
        // (The asset by name, else the pair's value: the script string
        // naming it, `ss:<index>`.)
        let name = match v {
            FieldValue::Asset { name, .. } => Some(name.trim_start_matches(',').to_owned()),
            FieldValue::Str(t) => t
                .strip_prefix("ss:")
                .and_then(|r| r.split(' ').next())
                .and_then(|i| i.parse::<usize>().ok())
                .and_then(|i| zone.assets.script_strings.get(i).cloned().flatten()),
            _ => None,
        };
        if let Some(name) = name.filter(|n| !n.is_empty()) {
            let e = objects.entry(who).or_default();
            match kind {
                "6" => e.0 = Some(name),
                "3" | "4" => e.1 = Some(name),
                _ => {}
            }
        }
    }
    objects.into_values().filter_map(|(m, a)| Some((m?, a?))).collect()
}

/// Reads the package bytes the props' build reads (each model's densest
/// mesh, its materials' colour, normal, specular and gloss pictures up to
/// the 512 the build takes) and drops them: the hard drive's reads done
/// ahead, beside the light, so the build finds them in the file cache.
/// (Lane B 11-05: Skyjacked's props built for about 7 s in the game.) The
/// build reads what it reads either way; nothing here reaches it. Returns
/// (entries read, bytes).
fn warm_props(cap: &Capture, paks: &XpakSet, models: impl Iterator<Item = String>) -> (usize, usize) {
    let (mut n, mut bytes) = (0usize, 0usize);
    let mut read = |key: u64| {
        if let Some((x, e)) = paks.find_all(key).into_iter().next()
            && let Ok(raw) = x.read_raw(e)
        {
            n += 1;
            bytes += raw.len();
        }
    };
    let (mut seen, mut mats) = (BTreeSet::new(), BTreeSet::new());
    for m in models {
        if !seen.insert(m.clone()) {
            continue;
        }
        let Some(rec) = cap.model(&m).or_else(|| cap.model(&format!(",{m}"))) else { continue };
        let Some(lod) = rec.lods.iter().max_by_key(|l| cap.mesh(&l.mesh).map_or(0, |x| x.vert_count)) else { continue };
        if let Some(mesh) = cap.mesh(&lod.mesh)
            && mesh.data.is_none()
            && let Some(x) = mesh.xpak
        {
            read(x.key);
        }
        mats.extend(lod.materials.iter().cloned());
    }
    let mut images = BTreeSet::new();
    for name in mats.iter().filter(|n| !n.starts_with(',')) {
        let Some(m) = cap.material(name) else { continue };
        for (hash, image) in &m.textures {
            if !image.starts_with(',') && !image.starts_with('$') && (slot_kind(*hash, image).is_some() || *hash == T7_GLOSS_HASH) {
                images.insert(image.clone());
            }
        }
    }
    for image in &images {
        let Some(im) = cap.image(image) else { continue };
        let dim = |k: usize| u32::from(im.xpak[k].width.max(im.xpak[k].height));
        if let Some(top) = (0..im.xpak.len()).rev().find(|&k| dim(k) <= 512).or((!im.xpak.is_empty()).then_some(0)) {
            for p in im.xpak.iter().take(top + 1) {
                read(p.key);
            }
        }
    }
    (n, bytes)
}

/// The props of the scene `bundles` (each object's `model` and `mainanim`)
/// in the map zone at `zone_path`, their materials linked into
/// `material_seed`. `ahead`: the zone already read ([`props_read`]); None
/// reads it here.
pub(crate) fn map_props(
    zone_path: &Path,
    ahead: Option<Result<PropsRead, String>>,
    bundles: &[String],
    plain: &[String],
    actors: &[String],
    material_seed: MaterialCatalog,
) -> T7MapProps {
    let mut out = T7MapProps {
        fpv: FpvMeshBuild::default(),
        xanims: XAnimBuild::default(),
        anims: Vec::new(),
        props: Vec::new(),
        cast: Vec::new(),
        material_seed,
        report: Vec::new(),
    };
    if bundles.is_empty() && plain.is_empty() && actors.is_empty() {
        return out;
    }
    let waited = std::time::Instant::now();
    let read_here = ahead.is_none();
    let PropsRead { zone, walk: w, paks, secs, warmed } = match ahead.unwrap_or_else(|| props_read(zone_path, &[], &[])) {
        Ok(r) => r,
        Err(e) => {
            out.report.push(e);
            return out;
        }
    };
    let (waited, built) = (waited.elapsed().as_secs_f32(), std::time::Instant::now());
    let cap = w.capture;
    // Each bundle object's model and main animation (`walk::bundle`'s
    // links: `object<i>#<key>#<kind>`, kind 6 a model, 3 an animation).
    let mut props: Vec<(String, String)> = Vec::new();
    for bundle in bundles {
        for (model, anim) in bundle_objects(&zone, &cap, bundle) {
            out.props.push((bundle.clone(), model.clone(), anim.clone()));
            props.push((model, anim));
        }
    }
    let mut b = Builder {
        zone: &zone,
        cap: &cap,
        paks,
        catalog: std::mem::take(&mut out.material_seed),
        owner: ZoneOwner::from_zone_path(zone_path),
        materials: HashMap::new(),
        images: HashMap::new(),
        image_failures: Vec::new(),
        decoded: 0,
        ready: HashMap::new(),
        pic_cache: None,
        pic_cache_hits: 0,
    };
    out.fpv.set_capture_ns(AssetNamespace::T6);
    out.xanims.set_capture_ns(AssetNamespace::T6);
    let (mut models, mut anims, mut failed) = (0usize, 0usize, Vec::new());
    let mut seen = BTreeSet::new();
    // (The map's plain script models, no animation: Nuk3town's destructible
    // mannequins and cars, whose clip stood there unseen without them.)
    let mut jobs: Vec<(String, Option<String>)> =
        props.iter().map(|(m, a)| (m.clone(), Some(a.clone()))).chain(plain.iter().map(|m| (m.clone(), None))).collect();
    // bo2mp: the actors' bodies, heads and hats (their AI types'
    // characters), and the zombies' own animations.
    let mut cast_anims: BTreeSet<String> = BTreeSet::new();
    if !actors.is_empty() {
        out.cast = actors.iter().filter_map(|a| ai_cast(&cap, a)).collect();
        for c in out.cast.iter().flat_map(|a| &a.characters) {
            for m in std::iter::once(&c.body).chain(&c.heads).chain(&c.attached) {
                jobs.push((m.clone(), None));
            }
        }
        cast_anims = cap.anims.iter().map(|a| a.name.trim_start_matches(',').to_owned()).filter(|n| zombie_clip(n)).collect();
    }
    for (model, anim) in &jobs {
        if seen.insert(model.clone()) {
            match cap.model(model).or_else(|| cap.model(&format!(",{model}"))).map(|rec| b.model_ref(rec)) {
                Some(Ok((mut xref, mats))) => {
                    let linked: Vec<Option<usize>> =
                        mats.iter().map(|m| m.as_deref().and_then(|m| b.emissive_prop_material(m).or_else(|| b.material(m, 512)))).collect();
                    let key = |k: usize| asset_t6::AssetKey { ty: fastfile_t6::AssetType::Material, index: k };
                    for (i, s) in xref.lod0.iter_mut().enumerate() {
                        s.material = linked.get(i).copied().flatten().map(|_| key(i));
                    }
                    xref.materials = xref.lod0.iter().map(|s| s.material).collect();
                    xref.name = model.clone();
                    match asset_model::capture_model_skel_t6(&xref, |k| linked.get(k.index).copied().flatten()) {
                        Some(skel) => {
                            out.fpv.insert_in(AssetNamespace::T6, skel, Some(&b.catalog));
                            models += 1;
                        }
                        None => failed.push(format!("{model} (no skeleton)")),
                    }
                }
                Some(Err(e)) => failed.push(format!("{model} ({e})")),
                None => failed.push(format!("{model} (not in the zone)")),
            }
        }
        let Some(anim) = anim else { continue };
        if seen.insert(anim.clone()) {
            match cap.anim(anim).or_else(|| cap.anim(&format!(",{anim}"))).and_then(|a| b.anim_ref(a)) {
                Some(mut r) => {
                    r.name = anim.clone();
                    // (A scene plays round, as its timing row says.)
                    r.looping = true;
                    if out.xanims.insert_t6_as(AssetNamespace::T6, &r) {
                        anims += 1;
                        out.anims.push(crate::T6AnimFacts {
                            name: anim.clone(),
                            numframes: r.numframes,
                            framerate: r.framerate,
                            looping: true,
                            delta_trans: Vec::new(),
                            delta_quat: Vec::new(),
                            notifies: r.notifies.clone(),
                        });
                    } else {
                        failed.push(format!("{anim} (bone counts)"));
                    }
                }
                None => failed.push(format!("{anim} (not read)")),
            }
        }
    }
    // bo2mp: a character's model the zone does not hold (Shadows of
    // Evil's `c_zom_zod_zombie_fem_body3_intestine`, a female zombie's
    // extra at 0x98, is only a reference there) leaves its composition: with it, the client found no
    // pose source for one model and drew the whole zombie in its bind pose.
    // (A comma-named record is only a reference to another zone's model:
    // no skeleton of its own.)
    let absent = |m: &String| {
        cap.model(m)
            .or_else(|| cap.model(&format!(",{m}")))
            .is_none_or(|r| r.bones == 0 || r.skeleton.bone_names.len() != r.bones)
    };
    let mut dropped = BTreeSet::new();
    for c in out.cast.iter_mut().flat_map(|a| a.characters.iter_mut()) {
        for m in c.heads.iter().chain(&c.attached).filter(|m| absent(m)) {
            dropped.insert(m.clone());
        }
        c.heads.retain(|m| !absent(m));
        c.attached.retain(|m| !absent(m));
    }
    if !dropped.is_empty() {
        out.report.push(format!("t7 actors: models not in the zone left off their characters: {dropped:?}"));
    }
    // bo2mp: the zombies' clips, each with its own looping flag (its
    // locomotion loops; an attack, a tear, a death plays once).
    let mut actor_anims = 0usize;
    for anim in &cast_anims {
        let rec = cap.anim(anim).or_else(|| cap.anim(&format!(",{anim}")));
        let Some(mut r) = rec.and_then(|a| b.anim_ref(a)) else {
            failed.push(format!("{anim} (not read)"));
            continue;
        };
        r.name = anim.clone();
        if out.xanims.insert_t6_as(AssetNamespace::T6, &r) {
            actor_anims += 1;
            out.anims.push(crate::T6AnimFacts {
                name: anim.clone(),
                numframes: r.numframes,
                framerate: r.framerate,
                looping: r.looping,
                // (The root motion: the ground a locomotion clip covers
                // is the zombie's speed, `t7_ai::speed`.)
                delta_trans: rec.map(|a| a.delta_trans.clone()).unwrap_or_default(),
                delta_quat: Vec::new(),
                notifies: r.notifies.clone(),
            });
        } else {
            failed.push(format!("{anim} (bone counts)"));
        }
    }
    // bo2mp: and on a Zombies map the timing and notetracks of every
    // animation its script bundles name (the scenes the scripts play by
    // name: Zetsubou's power plants wait on their water clip's
    // `water_drain_complete`), without building them.
    let mut scene_facts = 0usize;
    if !actors.is_empty() {
        let mut named: BTreeSet<String> = BTreeSet::new();
        for ((ty, _), links) in &cap.links {
            if *ty != fastfile_t7::asset_type::AssetType::ScriptBundle {
                continue;
            }
            for (label, v) in links {
                let kind = label.split('#').nth(2).unwrap_or("");
                if let (true, FieldValue::Asset { name, .. }) = (kind == "3" || kind == "4", v) {
                    named.insert(name.trim_start_matches(',').to_owned());
                }
            }
        }
        for anim in named {
            if seen.contains(&anim) || cast_anims.contains(&anim) {
                continue;
            }
            let Some(r) = cap.anim(&anim).or_else(|| cap.anim(&format!(",{anim}"))).and_then(|a| b.anim_ref(a)) else { continue };
            scene_facts += 1;
            out.anims.push(crate::T6AnimFacts {
                name: anim.clone(),
                numframes: r.numframes,
                framerate: r.framerate,
                looping: r.looping,
                delta_trans: Vec::new(),
                delta_quat: Vec::new(),
                notifies: r.notifies.clone(),
            });
        }
    }
    if !actors.is_empty() {
        let chars: usize = out.cast.iter().map(|c| c.characters.len()).sum();
        out.report.push(format!("t7 scene animation timing (not built): {scene_facts}"));
        out.report.push(format!("t7 actors: {} AI types, {chars} characters, {actor_anims} zombie animations of {}", out.cast.len(), cast_anims.len()));
    }
    out.report.push(format!(
        "t7 scene props: {models} models and {anims} animations of {} props from {} scenes, {} plain script models{}",
        props.len(),
        bundles.len(),
        plain.len(),
        if failed.is_empty() { String::new() } else { format!("; left out {:?}", &failed[..failed.len().min(8)]) }
    ));
    diag::info!(
        World,
        "t7 load lap: scene props: zone read {secs:.2}s ({}, {} MiB inflated), packages read ahead {} entries ({} MiB) in {:.2}s, waited {waited:.2}s, built in {:.2}s ({} pictures decoded)",
        if read_here { "here" } else { "ahead" },
        zone.image.len() >> 20,
        warmed.0,
        warmed.1 >> 20,
        warmed.2,
        built.elapsed().as_secs_f32(),
        b.decoded
    );
    let Builder { catalog, .. } = b;
    out.material_seed = catalog;
    out
}

/// bo2mp `CodLegacy check-guns`: each Black Ops III gun's fire sound and its
/// animations' notetrack sounds against mp_common's sound bank (an alias
/// is playable when it names a sound in the banks).
pub(crate) fn t7_sound_check(weapons: &asset_game::WeaponRegistry) -> Vec<String> {
    let Some(path) = asset_transport::games_root_from_env().ok().and_then(|r| asset_audio::bo3_mp_common(&r)) else {
        return vec!["t7 sounds: Black Ops III's mp_common is not installed".into()];
    };
    let bank = match asset_audio::bo3_weapon_sounds(&path) {
        Ok(b) => b,
        Err(e) => return vec![format!("t7 sounds: {e}")],
    };
    let playable = |alias: &str| {
        bank.sound_in(NS, alias)
            .is_some_and(|s| s.aliases.iter().any(|a| a.loaded.is_bound() || a.streamed.is_some()))
    };
    let (mut fire_ok, mut guns, mut nt_ok, mut nt_all) = (0, 0, 0, 0);
    let mut missing = Vec::new();
    for i in 1..=weapons.len() as u32 {
        if weapons.namespace_of(i) != Some(NS) {
            continue;
        }
        let Some(sounds) = weapons.sounds_of(i) else { continue };
        guns += 1;
        let fire = sounds.fire_player.as_deref().or(sounds.fire.as_deref());
        if fire.is_some_and(playable) {
            fire_ok += 1;
        } else {
            missing.push(format!("{} fire `{}`", weapons.name_of(i), fire.unwrap_or("-")));
        }
        for (_, alias) in &sounds.notetrack_sound_map {
            nt_all += 1;
            nt_ok += usize::from(playable(alias));
        }
    }
    vec![format!(
        "t7 sounds: {} alias lists; fire sound playable for {fire_ok} of {guns} weapons; notetrack sounds {nt_ok} of {nt_all}{}",
        bank.sounds.len(),
        if missing.is_empty() { String::new() } else { format!("; missing: {}", missing.iter().take(8).cloned().collect::<Vec<_>>().join(", ")) }
    )]
}

/// bo2mp `CodLegacy check-guns`: every Black Ops III gun animation decoded by
/// the engine's decoder, and whether its quaternions came out unit length.
pub(crate) fn t7_anim_check(xanims: &XAnimBuild) -> String {
    let (mut ok, mut bad) = (0usize, Vec::new());
    let names: Vec<String> = xanims.names_in(NS).map(str::to_owned).collect();
    for name in &names {
        match xanims.get(NS, name).map(|c| xmodel_runtime::AnimClip::from_parts(&c.parts)) {
            Some(Ok(_)) => ok += 1,
            Some(Err(e)) => bad.push(format!("{name}: {e:?}")),
            None => {}
        }
    }
    format!(
        "t7 animations: {ok} decode, {} refused{}",
        bad.len(),
        if bad.is_empty() { String::new() } else { format!(" ({})", bad.iter().take(3).cloned().collect::<Vec<_>>().join("; ")) }
    )
}

/// bo2mp `examples/t7_pack_lods.rs` (`IDLE=1`): each named gun's idle clip
/// tracks that name a bone of its view model (or `tag_weapon*`), sampled at
/// the start and the middle, read from the zone alone (no package).
pub(crate) fn pack_gun_idle_tracks(zone_path: &Path, guns: &[String]) -> Vec<String> {
    let mut out = Vec::new();
    let zone = match fastfile_t7::Zone::open(zone_path) {
        Ok(z) => z,
        Err(e) => return vec![format!("{}: {e}", zone_path.display())],
    };
    let w = fastfile_t7::walk::walk_with(&zone, fastfile_t7::walk::Options { keep_anims: true, ..Default::default() });
    let cap = &w.capture;
    let b = Builder {
        zone: &zone,
        cap,
        paks: XpakSet::default(),
        catalog: MaterialCatalog::default(),
        owner: ZoneOwner::from_zone_path(zone_path),
        materials: HashMap::new(),
        images: HashMap::new(),
        image_failures: Vec::new(),
        decoded: 0,
        ready: HashMap::new(),
        pic_cache: None,
        pic_cache_hits: 0,
    };
    for gun in guns {
        let Some(wp) = cap.weapons.iter().find(|x| &x.name == gun) else {
            out.push(format!("{gun}: no weapon"));
            continue;
        };
        let Some(FieldValue::Asset { name: model, .. }) = wp.field("gunModel") else { continue };
        let Some(rec) = cap.model(model) else { continue };
        let bones: Vec<String> = rec.skeleton.bone_names.iter().map(|&s| b.string(s)).collect();
        for (slot, _, anim) in &wp.anims {
            if *slot != 1 {
                continue;
            }
            let Some(a) = cap.anim(anim) else {
                out.push(format!("{gun}: idle {anim} not kept"));
                continue;
            };
            let mut xa = XAnimBuild::default();
            xa.set_capture_ns(NS);
            let Some(r) = b.anim_ref(a) else {
                out.push(format!("{gun}: idle {anim} refused"));
                continue;
            };
            xa.insert_t6_as(NS, &r);
            let Some(c) = xa.get(NS, anim) else { continue };
            let Ok(clip) = xmodel_runtime::AnimClip::from_parts(&c.parts) else { continue };
            out.push(format!("{gun} idle {anim}: {} tracks, {} frames; gun bones {}", clip.tracks.len(), clip.numframes, bones.len()));
            for (i, t) in clip.tracks.iter().enumerate() {
                if bones.iter().any(|x| x == &t.name) || t.name.starts_with("tag_weapon") {
                    let (s0, s1) = (clip.sample_track(i, 0.0), clip.sample_track(i, clip.duration() * 0.5));
                    let f = |s: &Option<xmodel_runtime::SampledTrack>| {
                        s.as_ref().map_or("-".to_owned(), |s| {
                            let r3 = |v: [f32; 3]| v.map(|x| (x * 1000.0).round() / 1000.0);
                            let r4 = |v: [f32; 4]| v.map(|x| (x * 1000.0).round() / 1000.0);
                            format!("t {:?} q {:?}", s.translation.map(|v| r3(v)), s.rotation.map(|q| r4(q)))
                        })
                    };
                    out.push(format!("   track {:22} {} | {}", t.name, f(&s0), f(&s1)));
                }
            }
        }
    }
    out
}

/// bo2mp `examples/t7_pack_guns.rs`: for each installed Workshop weapon
/// pack, the guns the lane adds, each with its Create-a-Class group and
/// what resolves: its view and world models in the pack's zone with their
/// mesh data (in the zone or a package), its colour maps' pixels, its
/// animations. With `build`, the packs are also built as a match builds
/// them, the lane's report and each catalog gun's model and animation
/// edges after it. `filter`: only the packs whose tag, game or id is in it
/// (comma list).
pub(crate) fn workshop_gun_check(build: bool, filter: Option<&str>) -> Vec<String> {
    let mut out = Vec::new();
    let prefetch = prefetch_packs(true, filter);
    out.extend(prefetch.1.iter().cloned());
    let mut paks = XpakSet::default();
    for p in &prefetch.0 {
        let T7Prefetch::Ready(z) = p else { continue };
        if let Some(m) = z.pack.and_then(|p| asset_transport::workshop::bo3_mod(p.workshop_id)) {
            paks.paks.extend(m.xpaks.iter().filter_map(|x| fastfile_t7::xpak::Xpak::open(x).ok()));
        }
    }
    if let Ok(dir) = bo3_zone_dir()
        && let Ok((bo3, _)) = XpakSet::open_dir(&dir)
    {
        paks.paks.extend(bo3.paks);
    }
    let mut totals = Vec::new();
    for p in &prefetch.0 {
        let T7Prefetch::Ready(z) = p else { continue };
        let Some(pack) = z.pack else { continue };
        out.extend(z.report.iter().cloned());
        let cap = &z.cap;
        let repaired: Vec<(String, WeaponRec, usize)> = pack_chosen(cap, pack)
            .into_iter()
            .map(|(n, w)| {
                let (w, k) = repaired_anims(cap, w);
                (n, w, k)
            })
            .collect();
        let guns: Vec<(String, &WeaponRec)> = repaired.iter().map(|(n, w, _)| (n.clone(), w)).collect();
        let (mut full, mut anims_ok, mut models_ok, mut pics_ok) = (0usize, 0usize, 0usize, 0usize);
        out.push(format!("## {} ({} of {} roster guns in {})", pack.title, guns.len(), pack.guns.len(), z.path.display()));
        for g in pack.guns {
            let Some((_, w)) = guns.iter().find(|(n, _)| n == g.gun) else {
                out.push(format!("  {:<28} {:<16} MISSING ({} not in the zone)", g.gun, g.group, pack.zone_weapon(g.gun)));
                continue;
            };
            let field = |f: &str| match w.field(f) {
                Some(FieldValue::Asset { name, .. }) | Some(FieldValue::Str(name)) => Some(name.clone()),
                None => None,
            };
            let mesh_ok = |name: &str| -> Result<(usize, usize), String> {
                let rec = cap.model(name).ok_or("not in the zone")?;
                let lod = rec.lods.iter().max_by_key(|l| cap.mesh(&l.mesh).map_or(0, |m| m.vert_count)).ok_or("no LODs")?;
                let mesh = cap.mesh(&lod.mesh).ok_or("mesh not read")?;
                match (mesh.data, mesh.xpak) {
                    (Some(_), _) => {}
                    (None, Some(x)) if !paks.find_all(x.key).is_empty() => {}
                    (None, Some(_)) => return Err("mesh data not in any package".into()),
                    _ => return Err("mesh has no data".into()),
                }
                // (Its surfaces' colour maps with pixels somewhere.)
                let colours: Vec<String> = lod
                    .materials
                    .iter()
                    .filter_map(|m| cap.material(m))
                    .filter_map(|m| m.textures.iter().find(|(h, n)| slot_kind(*h, n) == Some(Kind::Colour) && !n.starts_with(',') && !n.starts_with('$')).map(|(_, n)| n.clone()))
                    .collect();
                let have = colours.iter().filter(|c| cap.image(c).is_some_and(|im| im.pixels.is_some() || im.xpak.iter().any(|x| !paks.find_all(x.key).is_empty()))).count();
                Ok((have, colours.len()))
            };
            let view = field("gunModel").filter(|m| !m.starts_with(','));
            let world = field("worldModel").filter(|m| !m.starts_with(','));
            let say = |m: &Option<String>| match m.as_deref().map(|n| (n, mesh_ok(n))) {
                Some((n, Ok((have, all)))) => (true, have == all, format!("{n} ok, colour maps {have}/{all}")),
                Some((n, Err(e))) => (false, false, format!("{n} {e}")),
                None => (false, false, "none".into()),
            };
            let (v_ok, v_pics, v) = say(&view);
            let (w_ok, _, wm) = say(&world);
            let slots: Vec<&str> = w.anims.iter().map(|a| a.2.as_str()).collect();
            let named = slots.iter().filter(|a| cap.anim(a).is_some()).count();
            let idle = w.anims.iter().any(|a| a.0 == 1 && cap.anim(&a.2).is_some());
            models_ok += usize::from(v_ok && w_ok);
            pics_ok += usize::from(v_pics);
            anims_ok += usize::from(idle);
            full += usize::from(v_ok && w_ok && v_pics && idle);
            let renamed = repaired.iter().find(|r| r.0 == g.gun).map_or(0, |r| r.2);
            out.push(format!(
                "  {:<28} {:<16} {:<22} view {v}; world {wm}; animations {named}/{} named in the zone{}{}",
                g.gun,
                g.group,
                format!("\"{}\"", g.name),
                slots.len(),
                if renamed > 0 { format!(" ({renamed} found by name)") } else { String::new() },
                if idle { "" } else { " (NO IDLE)" }
            ));
        }
        out.push(format!(
            "  => {}: {} guns, {models_ok} with both models, {pics_ok} with every view colour map, {anims_ok} with an idle animation, {full} complete",
            pack.title,
            guns.len()
        ));
        totals.push(format!("{} {}/{} (complete {full})", pack.tab, guns.len(), pack.guns.len()));
    }
    out.push(format!("totals: {}", totals.join(", ")));
    if build {
        let progress = asset_transport::LoadProgress::default();
        let (built, _, report) = build_t7_pack_guns(prefetch, MaterialCatalog::default(), &progress);
        out.extend(report);
        // (The packs' sound banks, as the match's sound load reads them.)
        let banks: Vec<(&str, Arc<asset_audio::SoundCatalog>)> = asset_audio::bo3_pack_sound_zones()
            .into_iter()
            .filter_map(|(tab, zone)| match asset_audio::bo3_weapon_sounds(&zone) {
                Ok(c) => Some((tab, c)),
                Err(e) => {
                    out.push(format!("t7 workshop sounds: {tab} {}: {e}", zone.display()));
                    None
                }
            })
            .collect();
        for g in &built {
            out.extend(g.report.iter().cloned());
            out.push(t7_anim_check(&g.xanims));
            let playable = |alias: &str| {
                banks.iter().any(|(_, b)| b.sound_in(NS, alias).is_some_and(|s| s.aliases.iter().any(|a| a.loaded.is_bound() || a.streamed.is_some())))
            };
            let (mut fire_ok, mut silent) = (0usize, Vec::new());
            for i in 1..=g.weapons.len() as u32 {
                let fire = g.weapons.sounds_of(i).and_then(|s| s.fire_player.clone().or_else(|| s.fire.clone()));
                if fire.as_deref().is_some_and(playable) {
                    fire_ok += 1;
                } else {
                    silent.push(format!("{} `{}`", g.weapons.name_of(i), fire.unwrap_or_default()));
                }
            }
            out.push(format!("  sounds: fire sound playable for {fire_ok} of {} (not: {:?})", g.weapons.len(), &silent[..silent.len().min(6)]));
            let mut no_model = Vec::new();
            for i in 1..=g.weapons.len() as u32 {
                let anims = g.weapons.sz_xanims_of(i).map_or(0, |a| a.iter().flatten().count());
                let (view, world) = (g.weapons.gun_xmodel_of(i), g.weapons.world_model_of(i));
                if view.is_none() || world.is_none() || anims == 0 {
                    no_model.push(format!("{} (view {} world {} anims {anims})", g.weapons.name_of(i), view.is_some(), world.is_some()));
                }
            }
            out.push(format!("  built: {} weapons; without a view or world model or animations: {no_model:?}", g.weapons.len()));
        }
    }
    out
}

/// bo2mp (test aid): a model's pose source from its skeleton alone, as
/// `Builder::model_ref` + `capture_model_skel_t6` make it (for a zone whose
/// meshes' xpak is not staged: the bones are in the zone).
#[cfg(test)]
pub(crate) fn skeleton_pose_src(zone: &fastfile_t7::Zone, rec: &ModelRec) -> Option<xmodel_runtime::ModelPoseSrc> {
    let k = &rec.skeleton;
    let n = rec.bones;
    if n == 0 || n > 255 || k.base_mat.len() != n || k.bone_names.len() != n {
        return None;
    }
    let ss = |i: u32| zone.assets.script_strings.get(i as usize).cloned().flatten().unwrap_or_default();
    Some(xmodel_runtime::ModelPoseSrc {
        name: rec.name.trim_start_matches(',').to_owned(),
        num_bones: n,
        num_root_bones: usize::from(k.num_root_bones.max(1)),
        scale: 1.0,
        no_scale_part_bits: [0; 6],
        bone_names: k.bone_names.iter().map(|&s| ss(s)).collect(),
        parent_list: k.parent_list.clone(),
        quats: k.quats.iter().map(|q| q.map(frac16)).collect(),
        trans: k.trans.clone(),
        base_mat: k.base_mat.iter().map(|m| (glam::Quat::from_xyzw(m[0], m[1], m[2], m[3]), glam::Vec3::new(m[4], m[5], m[6]))).collect(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_pool_reads_map_zones_not_the_shared_ones() {
        let stems = |v: &[&str]| v.iter().map(|s| (*s).to_owned()).collect::<Vec<_>>();
        let all = stems(&["core_common", "mp_common", "mp_patch", "mp_spire", "mp_biodome", "mp_custom_x", "zm_castle", "mp_biodome_patch", "mp_stronghold", "mp_sector"]);
        // Favourites first (their order), at most three; no shared zones, no patches, no zombies.
        assert_eq!(pool_zones(&all, None), ["mp_biodome", "mp_stronghold", "mp_sector"]);
        // A choice is taken as given, minus what is not installed.
        assert_eq!(pool_zones(&all, Some("MP_Spire, mp_missing,mp_custom_x")), ["mp_spire", "mp_custom_x"]);
        // Other mp_ maps when no favourite is there.
        assert_eq!(pool_zones(&stems(&["mp_zzz", "mp_aaa", "mp_common"]), None), ["mp_aaa", "mp_zzz"]);
        // No map zone: the shared ones.
        assert_eq!(pool_zones(&stems(&["core_common", "mp_common", "mp_patch"]), None), ["mp_common", "core_common"]);
    }

    /// `CODLEGACY_GAMES=<root with Black Ops III> cargo test -p assets bo3_guns
    /// -- --ignored --nocapture`: Black Ops III's guns read and built as a
    /// match does.
    #[test]
    #[ignore]
    fn bo3_guns() {
        let g = walk_t7_guest_guns(true, MaterialCatalog::default(), &asset_transport::LoadProgress::default());
        for line in &g.report {
            println!("{line}");
        }
        println!("{}", t7_anim_check(&g.xanims));
        for i in 1..=g.weapons.len() as u32 {
            let anims = g.weapons.sz_xanims_of(i).map_or(0, |a| a.iter().flatten().count());
            println!(
                "  {} gun {:?} world {:?} anims {anims}",
                g.weapons.name_of(i),
                g.weapons.gun_xmodel_of(i),
                g.weapons.world_model_of(i)
            );
        }
    }

    #[test]
    fn part_bits_count_a_bone_from_the_top() {
        // (The hide words: bone 0 is the top bit, bone 22 is bit 9.)
        assert_eq!(part_bit(0, true), 0x8000_0000);
        assert_eq!(part_bit(22, true), 1 << 9);
        assert_eq!(part_bit(32, true), 0x8000_0000);
        // Old order: bone 22 was bit 22, read back as bone 9.
        assert_eq!(part_bit(22, false), 1 << 22);
    }

    #[test]
    fn hidden_bones_take_their_children() {
        // The MW2019 RAM-7's shape: every bone a child of `tag_sling` under
        // `tag_weapon` (steps back: 1 to tag_weapon, then k - 1 to tag_sling).
        let names: Vec<String> = ["tag_weapon", "tag_sling", "j_trigger", "tag_sight_off", "tag_sight_on"].iter().map(|s| (*s).to_owned()).collect();
        let parents = [1u8, 1, 2, 3];
        let tags = |t: &[&str]| t.iter().map(|s| (*s).to_owned()).collect::<Vec<_>>();
        // Its own hide tag: the one bone.
        assert_eq!(hidden_bone_set(&names, 1, &parents, &tags(&["tag_sight_off"])), [3]);
        // A tag over the body takes the body with it.
        assert_eq!(hidden_bone_set(&names, 1, &parents, &tags(&["TAG_SLING"])), [1, 2, 3, 4]);
        assert!(hidden_bone_set(&names, 1, &parents, &tags(&["tag_none"])).is_empty());
        assert!(hidden_bone_set(&names, 1, &parents, &[]).is_empty());
    }

    #[test]
    fn a_packs_aiming_overlays_stay_off_the_gun() {
        // CoD Online's CheyTac: its scope and the overlay it aims through.
        assert!(is_ads_overlay("codol_sco_sniper_cheytac_ads", true));
        assert!(!is_ads_overlay("codol_sco_sniper_cheytac", true));
        assert!(is_ads_overlay("codol_wpn_sniper_as50hellwalker_scope_ads", true));
        // Black Ops III's own name is dropped for its own guns too, the `_ads` one only for a pack's.
        assert!(is_ads_overlay("wpn_t7_dingo_ads_view", false));
        assert!(!is_ads_overlay("codol_sco_sniper_cheytac_ads", false));
        // Modern Warfare's `_ads_scope` is the gun's scope, not the overlay.
        assert!(!is_ads_overlay("vm_iw8_ax50_ads_scope", true));
        assert!(is_own_scope_model("vm_iw8_ax50_hip_scope", true));
        assert!(is_own_scope_model("codol_sco_sniper_cheytac", true));
        assert!(!is_own_scope_model("codol_sco_sniper_cheytac", false));
        assert!(!is_own_scope_model("vm_iw8_ram7_stock", true));
    }

    #[test]
    fn zombies_numbers_become_multiplayer_ones() {
        let mut def = vec![0u8; d6::SIZE];
        let mut variant = vec![0u8; v6::SIZE];
        let put = |b: &mut Vec<u8>, at: usize, v: i32| b[at..at + 4].copy_from_slice(&v.to_le_bytes());
        let get = |b: &Vec<u8>, at: usize| i32::from_le_bytes(b[at..at + 4].try_into().unwrap());
        // (Cold War's AK-47: 230 falling to 100.)
        put(&mut def, d6::damage, 230);
        put(&mut def, d6::damage + 20, 100);
        put(&mut variant, v6::iClipSize, 960);
        put(&mut def, d6::iMeleeDamage, 150);
        let changed = normalise_zombies_numbers(&mut def, &mut variant, "weapon_assault");
        assert_eq!(changed.len(), 2, "{changed:?}");
        assert_eq!((get(&def, d6::damage), get(&def, d6::damage + 20), get(&def, d6::damage + 4)), (42, 18, 0));
        assert_eq!((get(&variant, v6::iClipSize), get(&def, d6::iMeleeDamage)), (200, 135));
        // A rocket: its explosion at most 200, the outer scaled with it.
        let mut def = vec![0u8; d6::SIZE];
        put(&mut def, d6::damage, 5000);
        put(&mut def, d6::iExplosionInnerDamage, 1500);
        put(&mut def, d6::iExplosionOuterDamage, 150);
        normalise_zombies_numbers(&mut def, &mut variant, "weapon_launcher");
        assert_eq!((get(&def, d6::damage), get(&def, d6::iExplosionInnerDamage), get(&def, d6::iExplosionOuterDamage)), (300, 200, 20));
        // A multiplayer-sized gun is left as it is.
        let mut def = vec![0u8; d6::SIZE];
        put(&mut def, d6::damage, 33);
        assert!(normalise_zombies_numbers(&mut def, &mut variant, "weapon_smg").is_empty());
    }

    #[test]
    fn fire_types_renumber() {
        assert_eq!(t7_fire_type(0, 0), 0);
        assert_eq!(t7_fire_type(1, 0), 1);
        assert_eq!(t7_fire_type(2, 3), 3);
        assert_eq!(t7_fire_type(3, 4), 4);
        assert_eq!(t7_fire_type(2, 9), 5);
    }

    #[test]
    fn colourless_materials_get_a_colour() {
        // A bullet case: black placeholder, brass specular.
        let brass = solid_colour("mc/mtl_wpn_t7_bullet_case", ",$black_color", Some([0.0, 0.0, 0.0, 0.0, 0.591, 0.527, 0.352, 1.0]));
        assert_eq!(brass, [0.591, 0.527, 0.352]);
        // A tritium dot: its tint.
        let red = solid_colour("mc/mtl_wpn_t7_sniper_locus_tritium_red", ",$white_diffuse", Some([0.905, 0.009, 0.009, 0.0, 0.5, 0.5, 0.5, 1.0]));
        assert_eq!(red, [0.905, 0.009, 0.009]);
        // White tint, no constants of use: by name, else dark.
        assert_eq!(solid_colour("mc/mtl_wpn_t7_xr2_tritium_red", ",$white_diffuse", None), [0.9, 0.02, 0.02]);
        assert_eq!(solid_colour("mc/mtl_x_base", "", Some([1.0, 1.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0])), [0.03, 0.03, 0.03]);
        assert_eq!(solid_levels([1.0, 0.0, 0.0]).data[..4], [255, 0, 0, 255]);
        // Constants past 1 are not colours (an emissive sight glow's steps, a
        // tiled material's counts): dark, not bright green.
        assert_eq!(solid_colour("mc/mtl_wpn_t7_ar_isr27_sights_base_glow", ",$black_color", Some([1.0, 1.0, 1.0, 1.0, 0.0, 13.0, 0.0, 3.0])), [0.03, 0.03, 0.03]);
        assert_eq!(solid_colour("mc/mtl_p7_zm_ctl_music_box_metal_01", ",$black_color", Some([0.54, 0.55, 0.55, 1.0, 0.0, 2.0, 1.0, 1.0])), [0.03, 0.03, 0.03]);
        assert_eq!(solid_colour("mc/mtl_wpn_t7_pistol_mr6_tritium_red", ",$white_diffuse", Some([0.69, 1.0, 0.0, 1.0, 0.0, 17.0, 0.0, 8.0])), [0.69, 1.0, 0.0]);
    }

    #[test]
    fn half_quats_become_fractions() {
        // 0x3c00 is 1.0, 0xb800 is -0.5.
        assert_eq!(half_to_frac_bits(0x3c00) as i16, 32767);
        assert_eq!(half_to_frac_bits(0xb800) as i16, -16384);
    }

    #[test]
    fn keyed_and_constant_quats_convert() {
        let mut r = asset_t6::XAnimRef {
            name: String::new(),
            numframes: 2,
            framerate: 30.0,
            looping: false,
            delta: false,
            delta_trans: Vec::new(),
            delta_quat: Vec::new(),
            bone_count: [0, 0, 1, 0, 1, 0, 0, 0, 2, 2],
            names: Vec::new(),
            notifies: Vec::new(),
            // Keyed full quat: size 1 (two keys); then a constant.
            data_short: vec![1, 0x3c00, 0, 0, 0],
            data_byte: vec![0, 1],
            data_int: Vec::new(),
            random_data_byte: Vec::new(),
            random_data_short: vec![0x3c00, 0, 0, 0, 0, 0x3c00, 0, 0],
            random_data_int: Vec::new(),
            indices: Vec::new(),
        };
        t7_quats_to_t6(&mut r).unwrap();
        assert_eq!(r.random_data_short[0] as i16, 32767);
        assert_eq!(r.random_data_short[5] as i16, 32767);
        assert_eq!(r.data_short[0], 1);
        assert_eq!(r.data_short[1] as i16, 32767);
    }
}

/// A scene prop's opaque emissive body (linear): its grey colour
/// placeholder as the sun lights it (`Builder::emissive_prop_material`).
const OPAQUE_CUBE_BODY: f32 = 0.42;
