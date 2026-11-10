//! bo2mp: Call of Duty: Infinite Warfare's (IW7) guns as guest guns in a
//! Black Ops II match, under the IW7 namespace (`iw7_m4_mp`).
//!
//! `fastfile_iw7`'s exact walk reads what one gun needs from four places:
//! its definition in `global_core_mp`; its animation and sound packages
//! (and the animations) in `common_core_mp` (a patch zone's copy first);
//! its models in its own transient zones, `mp_weapon_view_iw7_<gun>_tr`
//! (first person) and `mp_weapon_world_iw7_<gun>_tr`; their materials in
//! the `techsets_*` zones, whose pictures stream from the `imagefile`
//! paks. A name starting with "," is a placeholder: the asset of that name
//! in another zone (`Core::resolve`).
//!
//! A gun's definition, complete definition and animation package timers
//! are laid out as Advanced Warfare's `WeaponDef` (`iw7_gun_fields`) and
//! captured by `WeaponCatalog::capture_s1`: Black Ops II's gunplay rules,
//! as AW's and Horizon MW's guns get. Its 260 animation slots go to AW's
//! (`ANIM_SLOTS`); IW7's animations pack as AW's, read as they are. A
//! model's vertices are Black Ops II's but for their unit vectors
//! (`super::iw7::iw7_vertex_to_t6`); its materials go through the IW7 map
//! lane's material builder (`super::iw7::Materials`), as a placed model's,
//! and a surface whose colour picture is in an absent pak draws with a
//! stand-in finish (`link_stand_in`: the first-person pass draws no surface
//! without a colour map). Arms are Black Ops II's: IW7's view animations
//! pose its own first-person rig (`IW7_RIG`, arms on a body), and Black Ops
//! II's arms and the gun's root take its hands' and `tag_weapon_right`'s
//! places (`asset_game::FpvArmsRetarget::build_iw7`).
//!
//! Its sounds are its own where their clips are in the sound banks here
//! (`asset_audio::iw7_sounds_from_banks`: its fire, handling and reload
//! notes), else a Black Ops II gun's of its kind (`DONORS`,
//! `iw7_guest_sounds`); its effects are a Black Ops II gun's (IW7's are
//! particle systems, not read). A guest gun is its base gun: IW7's
//! attachments are not read (a sniper aims through Black Ops II's sniper
//! scope).

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use asset_anim::XAnimBuild;
use asset_core::{AssetNamespace, ZoneOwner};
use asset_game::{S1WeaponView, WeaponBuild, WeaponCatalog};
use asset_material::{
    AuthoredImage, MaterialCatalog, MaterialTextureBinding, TS_COLOR_MAP, TS_SPECULAR_MAP,
};
use asset_model::{FpvMeshBuild, WorldWeaponBuild};
use bevy::math::{Quat, Vec3};
use fastfile_iw7::AssetType as T;
use fastfile_iw7::layout::{WeaponCompleteDef as C, WeaponDef as D};
use fastfile_iw7::map::{Iw7Map, MapMaterial};
use fastfile_iw7::shared::SharedZones;
use fastfile_iw7::walk::{AssetId, Capture, WeaponField, WeaponRec, XAnimRec, XModelRec};

use super::iw6::{fold_blend, le_u16s, le_u32s};
use super::iw7::{Materials, iw7_vertex_to_t6};

const NS: AssetNamespace = AssetNamespace::Iw7;

// bo2mp: the guns' own muzzle flashes, shell ejects and tracers.
#[path = "iw7_fx.rs"]
mod iw7_fx;

/// The largest level kept of a gun's pictures (first person), and of its
/// third-person model's.
/// Infinite Warfare authors its gun bodies at 2048 (163 of the 512 distinct
/// colour and normal pictures the roster's models name; 219 more at 1024):
/// capped at 1024 the receivers were soft. `BO2MP_IW7_TEXTURE=<size>` (or
/// `BO2MP_GUN_TEXTURE`) changes it.
const FIRST_PERSON_MAX: u32 = 2048;
const WORLD_MAX: u32 = 512;

/// Advanced Warfare's animation slots (`weapAnimFiles_t`).
const S1_ANIM_SLOTS: usize = 180;

/// IW7 `weapAnimFiles_t` slot -> the AW slot `capture_s1` reads it from,
/// and the endings IW7's names for that slot's animations have
/// (`vm_ar57_reload_empty`, `vm_m8_raise`, `vm_ar57_pullout`).
const ANIM_SLOTS: [(usize, usize, &[&str]); 35] = [
    (107, 1, &["idle"]),
    (108, 2, &["idle_empty", "empty_idle"]),
    (109, 3, &["fire"]),
    (110, 4, &["hold_fire", "fire_hold"]),
    (111, 5, &["fire_last", "lastshot", "last_shot"]),
    (112, 6, &["rechamber"]),
    (118, 9, &["melee_swipe", "melee", "melee_hit"]),
    (171, 24, &["reload"]),
    (172, 25, &["reload_empty"]),
    (173, 26, &["reload_start"]),
    (174, 27, &["reload_end"]),
    (175, 28, &["reload_fast"]),
    (176, 29, &["reload_empty_fast"]),
    (179, 36, &["raise", "pullout"]),
    (180, 37, &["raise_first", "first_raise", "pullout_first"]),
    (181, 38, &["raise_breach", "breach_raise"]),
    (182, 39, &["drop", "putaway"]),
    (183, 40, &["toggle_on", "alt_raise"]),
    (186, 41, &["toggle_off", "alt_drop"]),
    (193, 43, &["raise_quick", "pullout_quick", "quick_raise"]),
    (194, 44, &["drop_quick", "putaway_quick", "quick_drop"]),
    (195, 45, &["raise_empty", "empty_raise", "pullout_empty"]),
    (196, 46, &["drop_empty", "empty_drop", "putaway_empty"]),
    (197, 49, &["sprint_in"]),
    (199, 52, &["sprint_loop"]),
    (200, 53, &["sprint_out"]),
    (201, 57, &["stunned_start", "stun_in"]),
    (202, 58, &["stunned_loop", "stun_loop"]),
    (203, 59, &["stunned_end", "stun_out"]),
    (204, 61, &["detonate"]),
    (205, 62, &["nightvision_wear", "nvg_on"]),
    (207, 64, &["ads_fire"]),
    (208, 65, &["ads_fire_last", "ads_lastshot"]),
    (249, 128, &["ads_up"]),
    (250, 129, &["ads_down"]),
];

/// The most name stems (`vm_ar57`) a gun's animations are looked for by.
const ANIM_STEMS: usize = 4;

/// The zones a gun's definition, animations and sounds are looked for in,
/// in order (a patch before the zone it patches).
const CORE_ZONES: [&str; 5] = [
    "patch_common_core_mp",
    "common_core_mp",
    "common_mp",
    "patch_global_core_mp",
    "global_core_mp",
];

/// The zones only their materials, pictures and sound banks are taken
/// from.
const MATERIAL_ZONES: [&str; 4] = [
    "techsets_global_core_mp",
    "techsets_common_core_mp",
    "techsets_common_mp",
    "patch_common_mp",
];

/// The Black Ops II gun whose sounds and effects an IW7 gun plays.
const DONORS: &[(&str, &str)] = &[
    ("m4", "hk416_mp"),
    ("sdfar", "type95_mp"),
    ("ar57", "tar21_mp"),
    ("fmg", "tar21_mp"),
    ("ake", "scar_mp"),
    ("rvn", "an94_mp"),
    ("vr", "scar_mp"),
    ("gauss", "saritch_mp"),
    ("arclassic", "tar21_mp"),
    ("m1c", "sa58_mp"),
    ("erad", "qcw05_mp"),
    ("fhr", "pdw57_mp"),
    ("crb", "vector_mp"),
    ("ripper", "peacekeeper_mp"),
    ("crdb", "mp7_mp"),
    ("mp28", "insas_mp"),
    ("tacburst", "evoskorpion_mp"),
    ("ump45c", "insas_mp"),
    ("sdflmg", "mk48_mp"),
    ("mauler", "qbb95_mp"),
    ("lmg03", "lsat_mp"),
    ("minilmg", "mk48_mp"),
    ("unsalmg", "lsat_mp"),
    ("kbs", "ballista_mp"),
    ("m8", "svu_mp"),
    ("ba50cal", "as50_mp"),
    ("cheytacc", "dsr50_mp"),
    ("devastator", "saiga12_mp"),
    ("sdfshotty", "srm1216_mp"),
    ("sonic", "870mcs_mp"),
    ("mod2187", "ksg_mp"),
    ("longshot", "ksg_mp"),
    ("spasc", "870mcs_mp"),
    ("emc", "fnp45_mp"),
    ("nrg", "fiveseven_mp"),
    ("revolver", "judge_mp"),
    ("udm45", "beretta93r_mp"),
    ("mag", "kard_mp"),
    ("g18c", "beretta93r_mp"),
    ("lockon", "fhj18_mp"),
    ("chargeshot", "smaw_mp"),
];

/// The weapon a roster gun is in IW7's zones (`m4` is `iw7_m4_mp`).
pub(crate) fn iw7_weapon_name(gun: &str) -> String {
    format!("iw7_{gun}_mp")
}

/// The name a gun's transient zones carry (`mp_weapon_view_iw7_<name>_tr`):
/// a classic gun's are those of the gun it is a version of (`ump45c`'s are
/// `ump45`'s), the Reaver's are `dev`.
pub(crate) fn iw7_zone_gun(gun: &str) -> &str {
    match gun {
        "devastator" => "dev",
        "ump45c" | "cheytacc" | "m1c" | "spasc" | "g18c" => &gun[..gun.len() - 1],
        g => g,
    }
}

/// Infinite Warfare's guns, ready to join the match's catalogs.
pub(crate) struct Iw7Guests {
    pub weapons: WeaponBuild,
    pub fpv: FpvMeshBuild,
    pub world_guns: WorldWeaponBuild,
    pub xanims: XAnimBuild,
    pub material_seed: MaterialCatalog,
    pub report: Vec<String>,
    /// Per gun built: what it came to (for checks and reports).
    pub built: Vec<Iw7GunReport>,
    /// Every sound alias the zones' banks name.
    pub aliases: HashSet<String>,
    /// The guns' own effects, effect models and tracers (the guns' effect
    /// slots name them; `BO2MP_IW7_OWN_FX=0` leaves them empty).
    pub fx: asset_game::FxCatalog,
    pub fx_models: asset_game::FxModelCatalog,
    pub tracers: asset_game::TracerCatalog,
}

/// One gun as the lane built it.
#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
pub(crate) struct Iw7GunReport {
    pub gun: String,
    /// Its own effects: per slot, the elements read and the modules
    /// dropped (`iw7_fx`).
    pub fx: Vec<String>,
    pub view_model: String,
    pub world_model: String,
    /// (vertices, triangles, bones), first person and third person.
    pub view: Option<(usize, usize, usize)>,
    pub world: Option<(usize, usize, usize)>,
    /// The view model's root bone, and the bones its surfaces are skinned
    /// to that it lacks (none: every blend bone is one of its own).
    pub root: String,
    pub bad_blend: usize,
    /// The idle animation's bones: (in the gun model, the arms'), and the
    /// ones it drives on neither.
    pub idle_bones: (usize, usize),
    pub idle_missing: Vec<String>,
    /// Materials linked; of those, with no colour map because its picture
    /// is in an `imagefile` pak that is not there, and with none at all.
    pub materials: usize,
    pub grey_absent_pak: usize,
    pub grey_missing: usize,
    /// Those last: "material (technique set)".
    pub no_picture: Vec<String>,
    /// Materials the models name that no zone defines.
    pub undefined: Vec<String>,
    /// Animations kept; of their slots, those whose animation was found by
    /// its name (the package's reference read as another animation), and
    /// those left empty (the reference names no animation for the slot);
    /// placeholders not found in any zone.
    pub anims: usize,
    pub by_name: usize,
    pub dropped: usize,
    pub missing_anims: Vec<String>,
    /// The sound aliases its package names; of those, the ones it plays
    /// from IW7's own banks, and the ones whose clips are in banks that
    /// are not here (its Black Ops II donor's play for them).
    pub sounds: Vec<String>,
    pub sounds_own: Vec<String>,
    pub sounds_donor: Vec<String>,
    /// Its animations' sound notes (`weap_m4_clipout_plr`): those IW7's
    /// banks play, and those its donor's reload sound of the same kind
    /// plays for.
    pub notes_own: Vec<String>,
    pub notes_donor: Vec<String>,
    pub hud_icon: Option<String>,
    /// The view model posed by its idle on IW7's own first-person rig
    /// (`IW7_RIG`): per tag the gun could hang on, how far the hands are
    /// from the gun's hand tags (`tag_ik_loc_ri`, `tag_ik_loc_le`).
    pub grip: Vec<(String, f32, f32)>,
    /// The tag it hangs on (the nearest grip), and its bounds there in
    /// `tag_view`'s frame (x forward, y left, z up).
    pub hangs_on: String,
    pub view_box: Option<([f32; 3], [f32; 3])>,
    /// On arms of Black Ops II's make (`ARMS_STAND_IN`), the gun on their
    /// `tag_weapon`: how far the wrists are from the gun's hand tags, right
    /// and left, the idle played straight onto them and as the first
    /// person view plays it (`arms_fit`).
    pub arms_fit: Option<[(f32, f32); 2]>,
    /// Surfaces whose colour picture is named as another kind of map
    /// (normal, specular, gloss, occlusion) and their pictures.
    pub colour_not_albedo: Vec<String>,
    /// Surfaces drawn with the stand-in finish (their colour picture in an
    /// absent pak, or none).
    pub stand_in: usize,
    /// Skinned vertices whose weights add past one, surfaces left out for
    /// vertices that are not positions.
    pub weight_over: usize,
    pub garbled: usize,
    /// Its idle animation's name.
    pub idle: String,
    /// The view model's stance offsets as captured (AW's `duckedOfs`,
    /// `proneOfs`, `standMove`; the stances' bob, `proneMove` -160 3 -120,
    /// is a rate): the largest component.
    pub view_offset_max: f32,
    /// Each distinct colour and normal picture the models' materials name:
    /// (name, largest stream's side, the `imagefile` paks its streams are
    /// in).
    pub pictures: Vec<(String, u32, Vec<u16>)>,
    /// The animation slots (by their first ending) nothing was found for.
    pub empty_slots: Vec<String>,
    /// Materials with an emissive map that glows: "material (map, scale,
    /// technique set)".
    pub emissive: Vec<String>,
    /// Of those, the materials drawn with their glow (`iw7_glow::link_gun_glow`).
    pub glowing: usize,
}

impl Iw7Guests {
    fn empty(material_seed: MaterialCatalog, report: Vec<String>) -> Self {
        Self {
            weapons: WeaponBuild::default(),
            fpv: FpvMeshBuild::default(),
            world_guns: WorldWeaponBuild::default(),
            xanims: XAnimBuild::default(),
            material_seed,
            report,
            built: Vec::new(),
            aliases: HashSet::new(),
            fx: asset_game::FxCatalog::default(),
            fx_models: asset_game::FxModelCatalog::default(),
            tracers: asset_game::TracerCatalog::default(),
        }
    }
}

/// Where Infinite Warfare's guns are, found ahead of the build.
pub(crate) enum Iw7Prefetch {
    Off(Vec<String>),
    Ready(PathBuf),
}

/// bo2mp: finds Infinite Warfare's zones (`wanted`: Black Ops II rules with
/// guest guns on).
pub(crate) fn prefetch_iw7_guest_guns(wanted: bool) -> Iw7Prefetch {
    if !wanted {
        return Iw7Prefetch::Off(vec!["iw7 guest guns: off".into()]);
    }
    match asset_audio::iw7_zone_dir() {
        Some(dir) => Iw7Prefetch::Ready(dir),
        None => Iw7Prefetch::Off(vec![
            "iw7 guest guns: Infinite Warfare's global_core_mp not found (Game Folders: iw)".into(),
        ]),
    }
}

/// bo2mp: the match's Infinite Warfare guest guns, their materials linked
/// into `material_seed`.
pub(crate) fn build_iw7_guest_guns(
    prefetch: Iw7Prefetch,
    material_seed: MaterialCatalog,
    progress: &asset_transport::LoadProgress,
) -> Iw7Guests {
    let dir = match prefetch {
        Iw7Prefetch::Off(report) => return Iw7Guests::empty(material_seed, report),
        Iw7Prefetch::Ready(dir) => dir,
    };
    let stage = progress.begin_scoped(
        asset_transport::StageId::CommonAssets,
        "iw7_guest_weapons",
        None,
    );
    let guests = load_iw7_guests(&dir, material_seed);
    stage.done();
    guests
}

/// bo2mp: read and built in one go (`CodLegacy check-guns`).
pub(crate) fn walk_iw7_guest_guns(
    wanted: bool,
    material_seed: MaterialCatalog,
    progress: &asset_transport::LoadProgress,
) -> Iw7Guests {
    let mut g = build_iw7_guest_guns(prefetch_iw7_guest_guns(wanted), material_seed, progress);
    let lines: Vec<String> = g
        .built
        .iter()
        .map(|r| {
            format!(
                "iw7 {}: view {:?}, world {:?}, {} materials ({} pictures in absent paks, {} without), {} animations ({} slots found by name, {} left empty), idle drives {} gun / {} arms bones, {} of {} sound aliases in the banks",
                r.gun,
                r.view,
                r.world,
                r.materials,
                r.grey_absent_pak,
                r.grey_missing,
                r.anims,
                r.by_name,
                r.dropped,
                r.idle_bones.0,
                r.idle_bones.1,
                r.sounds.iter().filter(|a| g.aliases.contains(*a)).count(),
                r.sounds.len()
            )
        })
        .collect();
    g.report.extend(lines);
    g
}

/// One zone, walked.
struct Walked {
    name: String,
    zone: fastfile_iw7::Zone,
    cap: Capture,
    stop: Option<fastfile_iw7::walk::WalkStop>,
}

/// `name` in `dir` walked, as far as it reads (`common_mp` stops at its
/// equipment sound table: its sound bank is found after it).
fn walk(dir: &Path, name: &str) -> Result<Walked, String> {
    let path = fastfile_iw7::zone_path(dir, name)
        .ok_or_else(|| format!("{name}.ff not in {}", dir.display()))?;
    let zone = fastfile_iw7::Zone::open(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    let (mut cap, stop) = fastfile_iw7::walk::walk_zone(&zone);
    if let Some(s) = &stop
        && let Some(bank) = fastfile_iw7::walk::recover_sound_bank(&zone, s)
    {
        cap.sound_banks.push(bank);
    }
    Ok(Walked {
        name: name.to_owned(),
        zone,
        cap,
        stop,
    })
}

fn bare(name: &str) -> &str {
    name.trim_start_matches(',')
}

fn string(strings: &[Option<String>], index: u32) -> String {
    strings
        .get(index as usize)
        .cloned()
        .flatten()
        .unwrap_or_else(String::new)
}

/// Whether a capture holds the asset itself, not a placeholder for one in
/// another zone (IW7 also names real assets with a ",").
fn real(c: &Capture, id: AssetId) -> bool {
    let i = id.index as usize;
    match id.ty {
        T::XAnimParts => c.xanims.get(i).is_some_and(|a| {
            a.numframes > 0
                || a.bone_count[9] > 0
                || a.data_byte.is_some()
                || a.data_short.is_some()
        }),
        T::AnimPackage => c
            .anim_packages
            .get(i)
            .is_some_and(|p| !p.name.starts_with(',') || p.anims.iter().any(Option::is_some)),
        T::SfxPackage => c
            .sfx_packages
            .get(i)
            .is_some_and(|p| !p.name.starts_with(',') || !p.sounds.is_empty()),
        T::XModel => c
            .xmodels
            .get(i)
            .is_some_and(|m| m.lods.first().is_some_and(|l| l.surfs.is_some())),
        T::XModelSurfs => c.model_surfs.get(i).is_some_and(|s| !s.surfs.is_empty()),
        T::Weapon => c.weapons.get(i).is_some_and(|w| w.def.is_some()),
        _ => !c.name_of(id).starts_with(','),
    }
}

/// The core zones, and every real asset of theirs by (type, name).
struct Core {
    zones: Vec<Walked>,
    index: HashMap<(T, String), (usize, AssetId)>,
}

impl Core {
    fn new(zones: Vec<Walked>) -> Self {
        let mut index = HashMap::new();
        for (z, w) in zones.iter().enumerate() {
            let c = &w.cap;
            for (ty, n) in [
                (T::XAnimParts, c.xanims.len()),
                (T::AnimPackage, c.anim_packages.len()),
                (T::SfxPackage, c.sfx_packages.len()),
                (T::XModel, c.xmodels.len()),
                (T::XModelSurfs, c.model_surfs.len()),
                (T::Weapon, c.weapons.len()),
            ] {
                for i in 0..n {
                    let id = AssetId {
                        ty,
                        index: i as u32,
                    };
                    if real(c, id) {
                        index
                            .entry((ty, bare(c.name_of(id)).to_owned()))
                            .or_insert((z, id));
                    }
                }
            }
        }
        Self { zones, index }
    }

    /// The real asset `id` of zone `z` stands for.
    fn resolve(&self, z: usize, id: AssetId) -> Option<(usize, AssetId)> {
        let c = &self.zones.get(z)?.cap;
        if real(c, id) {
            return Some((z, id));
        }
        self.index
            .get(&(id.ty, bare(c.name_of(id)).to_owned()))
            .copied()
    }

    fn weapon(&self, name: &str) -> Option<(usize, &WeaponRec)> {
        let &(z, id) = self.index.get(&(T::Weapon, name.to_owned()))?;
        Some((z, self.zones[z].cap.weapons.get(id.index as usize)?))
    }
}

/// IW7's blend vertices as Black Ops II's (which `fold_blend` folds to
/// four bones): a vertex with `k + 1` bones is its first bone and `k`
/// (bone, weight) pairs in both, but IW7 pads each to an even count of
/// words (the pad word is zero) and keeps a bone as its index, Black Ops II
/// as its index times 64. Also how many bones are not among the model's
/// `bones`, and how many vertices' weights add past one.
fn s1_blend(counts: [u16; 8], words: &[u16], bones: usize) -> (Vec<u16>, usize, usize) {
    let mut out = Vec::with_capacity(words.len());
    let (mut at, mut bad, mut over) = (0usize, 0usize, 0usize);
    for (k, &count) in counts.iter().enumerate() {
        let per = 1 + 2 * k;
        for _ in 0..count {
            let Some(v) = words.get(at..at + per) else {
                return (out, bad, over);
            };
            let mut sum = 0u32;
            for (i, &w) in v.iter().enumerate() {
                let bone = i == 0 || i % 2 == 1;
                if bone && usize::from(w) >= bones {
                    bad += 1;
                }
                if !bone {
                    sum += u32::from(w);
                }
                out.push(if bone { w.saturating_mul(64) } else { w });
            }
            if sum > 0xffff {
                over += 1;
            }
            at += per + 1;
        }
    }
    (out, bad, over)
}

/// Whether a surface's vertices (IW7's 32-byte vertices) are positions: a
/// few with a non-finite or far-off position or texture coordinate are
/// garbage read as vertices.
fn vertices_sane(verts: &[u8]) -> bool {
    let n = verts.len() / 32;
    let bad = verts
        .chunks_exact(32)
        .filter(|v| {
            let f = |o: usize| f32::from_le_bytes([v[o], v[o + 1], v[o + 2], v[o + 3]]);
            let half_ok = |o: usize| (u16::from_le_bytes([v[o], v[o + 1]]) >> 10) & 31 != 31;
            ![f(0), f(4), f(8)]
                .iter()
                .all(|c| c.is_finite() && c.abs() < 1000.0)
                || !half_ok(20)
                || !half_ok(22)
        })
        .count();
    bad * 50 <= n
}

/// A model in Black Ops II's captured form.
struct ModelRef {
    xref: asset_t6::XModelRef,
    /// The material name each surface uses (`None`: none).
    materials: Vec<Option<String>>,
    /// How many blend bones it lacks, vertices whose weights add past
    /// one, surfaces left out for garbage vertices.
    bad_blend: usize,
    weight_over: usize,
    garbled: usize,
    /// What was left out of it, and why.
    left_out: Vec<String>,
}

/// A model in Black Ops II's captured form and the material names its
/// surfaces use, and how many blend bones it lacks.
fn model_ref(w: &Walked, rec: &XModelRec) -> Option<ModelRef> {
    let image = &w.zone.image[..];
    let strings = &w.zone.assets.script_strings;
    let names: Vec<String> = rec.bone_names.iter().map(|&s| string(strings, s)).collect();
    let n = names.len();
    if n == 0 || n > 255 || rec.base_mat.len() != n {
        return None;
    }
    let roots = usize::from(rec.num_root_bones).clamp(1, n);
    let lod = rec.lods.first()?;
    let surfs_id = lod.surfs?;
    // (A placeholder: the surfaces of that name in the same zone.)
    let surfs = if real(&w.cap, surfs_id) {
        w.cap.model_surfs.get(surfs_id.index as usize)?
    } else {
        let want = bare(w.cap.name_of(surfs_id));
        w.cap
            .model_surfs
            .iter()
            .find(|s| bare(&s.name) == want && !s.surfs.is_empty())?
    };
    let (mut mins, mut maxs) = ([f32::MAX; 3], [f32::MIN; 3]);
    let mut lod0 = Vec::new();
    let mut materials = Vec::new();
    let (mut bad_blend, mut weight_over, mut garbled) = (0usize, 0usize, 0usize);
    let mut left_out = Vec::new();
    for (i, s) in surfs.surfs.iter().enumerate() {
        let vc = usize::from(s.vert_count);
        let all_tris = le_u16s(s.tris.map_or(&[][..], |sp| sp.get(image)));
        // (A few surfaces end with a triangle naming a vertex past their
        // own: that triangle is left out.)
        let tris: Vec<u16> = all_tris
            .chunks_exact(3)
            .filter(|t| t.iter().all(|&v| usize::from(v) < vc))
            .flatten()
            .copied()
            .collect();
        if tris.len() < all_tris.len() / 3 * 3 {
            left_out.push(format!(
                "surface {i}: {} of its {} triangles past its vertices",
                (all_tris.len() - tris.len()) / 3,
                all_tris.len() / 3
            ));
        }
        let blend_n: usize = s
            .blend_vert_counts
            .iter()
            .map(|&c| usize::from(c))
            .sum::<usize>()
            + s.rigid.iter().map(|r| usize::from(r.1)).sum::<usize>();
        // (A surface whose vertices are not in the zone, or that names
        // vertices or bones it does not have, is left out.)
        let why = if s.verts.is_none_or(|sp| sp.len < 32 * vc) {
            Some("no vertices in the zone")
        } else if !vertices_sane(s.verts.map_or(&[][..], |sp| sp.get(image))) {
            garbled += 1;
            Some("its vertices are not positions")
        } else if blend_n != vc {
            Some("skinned vertex count off")
        } else if s.rigid.iter().any(|r| usize::from(r.0) >= n) {
            Some("a rigid bone past the skeleton")
        } else {
            None
        };
        if let Some(why) = why {
            left_out.push(format!("surface {i}: {why}"));
            continue;
        }
        let verts: Vec<u8> = s
            .verts
            .map_or(&[][..], |sp| sp.get(image))
            .chunks_exact(32)
            .flat_map(|v| {
                let mut a = [0u8; 32];
                a.copy_from_slice(v);
                iw7_vertex_to_t6(&a)
            })
            .collect();
        for v in verts.chunks_exact(32) {
            for k in 0..3 {
                let c = f32::from_le_bytes(v[4 * k..4 * k + 4].try_into().unwrap_or([0; 4]));
                mins[k] = mins[k].min(c);
                maxs[k] = maxs[k].max(c);
            }
        }
        let (words, bad, over) = s1_blend(
            s.blend_vert_counts,
            &le_u16s(s.blend_verts.map_or(&[][..], |sp| sp.get(image))),
            n,
        );
        bad_blend += bad;
        weight_over += over;
        let (blend_counts, blend) = fold_blend(s.blend_vert_counts, &words);
        let material = rec
            .materials
            .get(usize::from(lod.surf_index) + i)
            .copied()
            .flatten()
            .map(|id| bare(w.cap.name_of(id)).to_owned())
            .filter(|n| !n.is_empty());
        let mut part_bits = [0u32; 6];
        part_bits.copy_from_slice(&s.part_bits[..6]);
        lod0.push(asset_t6::XSurfaceRef {
            verts,
            vert_count: s.vert_count,
            indices: tris,
            vert_lists: s
                .rigid
                .iter()
                .map(|&(b, v, t, c)| (b.saturating_mul(64), v, t, c))
                .collect(),
            flags: 0,
            material: None,
            blend_counts,
            blend,
            part_bits,
        });
        materials.push(material);
    }
    if lod0.is_empty() {
        return None;
    }
    let nonroot = n - roots;
    let radius = (0..3)
        .map(|k| mins[k].abs().max(maxs[k].abs()).powi(2))
        .sum::<f32>()
        .sqrt();
    let mut part_classification = rec.part_classification.clone();
    part_classification.resize(n, 0);
    Some(ModelRef {
        xref: asset_t6::XModelRef {
            lighting_origin_offset: [0.0; 3],
            lighting_origin_range: 0.0,
            name: bare(&rec.name).to_owned(),
            numsurfs: lod0.len().min(255) as u8,
            materials: Vec::new(),
            num_bones: n as u8,
            num_lods: 1,
            mins,
            maxs,
            radius,
            lods: vec![(0.0, lod0.len() as u16, 0)],
            lod0,
            bone_names: names,
            num_root_bones: roots as u8,
            parent_list: (0..nonroot)
                .map(|k| rec.parent_list.get(k).copied().unwrap_or(0))
                .collect(),
            quats: (0..nonroot)
                .map(|k| rec.tag_angles.get(k).copied().unwrap_or([0, 0, 0, 0x7fff]))
                .collect(),
            trans: (0..nonroot)
                .map(|k| rec.tag_positions.get(k).copied().unwrap_or([0.0; 3]))
                .collect(),
            base_mat: rec
                .base_mat
                .iter()
                .map(|m| ([m[0], m[1], m[2], m[3]], [m[4], m[5], m[6]]))
                .collect(),
            part_classification,
            bone_info: vec![None; n],
            contents: 0,
            coll_lod: -1,
            coll_surfs: Vec::new(),
            phys_preset: None,
        },
        materials,
        bad_blend,
        weight_over,
        garbled,
        left_out,
    })
}

/// An animation in Black Ops II's captured form (IW7's parts pack as
/// Ghosts' and AW's).
fn anim_ref(w: &Walked, a: &XAnimRec) -> asset_t6::XAnimRef {
    let image = &w.zone.image[..];
    let strings = &w.zone.assets.script_strings;
    let bytes = |s: Option<fastfile_iw7::walk::Span>| s.map_or(&[][..], |s| s.get(image));
    let indices = if a.numframes >= 256 {
        le_u16s(bytes(a.indices))
    } else {
        bytes(a.indices).iter().map(|&b| u16::from(b)).collect()
    };
    asset_t6::XAnimRef {
        delta_quat: Vec::new(),
        name: bare(&a.name).to_owned(),
        numframes: a.numframes,
        framerate: a.framerate,
        looping: a.flags & 1 != 0,
        delta: false,
        delta_trans: Vec::new(),
        bone_count: a.bone_count,
        names: a.names.iter().map(|&s| string(strings, s)).collect(),
        notifies: a
            .notify
            .iter()
            .map(|&(s, t)| (string(strings, s), t))
            .collect(),
        data_byte: bytes(a.data_byte).to_vec(),
        data_short: le_u16s(bytes(a.data_short)),
        data_int: le_u32s(bytes(a.data_int)),
        random_data_byte: bytes(a.random_data_byte).to_vec(),
        random_data_short: le_u16s(bytes(a.random_data_short)),
        random_data_int: le_u32s(bytes(a.random_data_int)),
        indices,
    }
}

/// The first of `names` the core zones hold as `ty`, else `fallback`.
fn by_name_or(
    core: &Core,
    ty: T,
    names: &[String],
    fallback: Option<(usize, AssetId)>,
) -> Option<(usize, AssetId)> {
    names
        .iter()
        .find_map(|n| core.index.get(&(ty, n.clone())).copied())
        .or(fallback)
}

/// The stems a package's animations are named from (`vm_ar57` in
/// `vm_ar57_reload_empty`), most names first: each name's leading words
/// (from `vm_<word>` on) that two or more of its distinct names share.
fn anim_stems(names: &[String]) -> Vec<String> {
    let mut count: HashMap<String, usize> = HashMap::new();
    let distinct: HashSet<&String> = names.iter().collect();
    for n in distinct {
        if !n.starts_with("vm_") {
            continue;
        }
        let words: Vec<&str> = n.split('_').collect();
        for k in 2..words.len() {
            *count.entry(words[..k].join("_")).or_default() += 1;
        }
    }
    let mut stems: Vec<(String, usize)> = count.into_iter().filter(|(_, c)| *c >= 2).collect();
    stems.sort_by(|a, b| {
        b.1.cmp(&a.1)
            .then(a.0.len().cmp(&b.0.len()))
            .then(a.0.cmp(&b.0))
    });
    stems.into_iter().take(ANIM_STEMS).map(|(s, _)| s).collect()
}

/// Arms bones as Black Ops II's and Infinity Ward's arms name them.
fn is_arms_bone(name: &str) -> bool {
    let n = name.to_ascii_lowercase();
    [
        "j_shoulder",
        "j_elbow",
        "j_wrist",
        "j_clavicle",
        "j_thumb",
        "j_index",
        "j_mid",
        "j_ring",
        "j_pinky",
        "j_palm",
        "j_webbing",
        "j_sleeve",
        "j_myhand",
        "j_forearm",
        "j_upperarm",
    ]
    .iter()
    .any(|p| n.starts_with(p))
        || matches!(
            n.as_str(),
            "tag_view"
                | "tag_torso"
                | "tag_weapon"
                | "tag_weapon_right"
                | "tag_weapon_left"
                | "tag_cambone"
                | "tag_camera"
                | "tag_ik_hand_le"
                | "tag_ik_hand_ri"
        )
}

/// Where a gun's model is: a real one of that name in its transient zone,
/// else in the core zones.
fn find_model<'a>(
    name: &str,
    own: &'a Walked,
    core: &'a Core,
) -> Option<(&'a Walked, &'a XModelRec)> {
    if let Some(m) = own
        .cap
        .xmodels
        .iter()
        .find(|m| bare(&m.name) == name && m.lods.first().is_some_and(|l| l.surfs.is_some()))
    {
        return Some((own, m));
    }
    let &(z, id) = core.index.get(&(T::XModel, name.to_owned()))?;
    Some((
        &core.zones[z],
        core.zones[z].cap.xmodels.get(id.index as usize)?,
    ))
}

/// The builders' shared output, carried from gun to gun.
struct Out {
    catalog: WeaponCatalog,
    fpv: FpvMeshBuild,
    world_guns: WorldWeaponBuild,
    xanims: XAnimBuild,
    built: Vec<Iw7GunReport>,
    decoded: usize,
    failures: Vec<String>,
    scopes: Vec<String>,
    /// The stand-in finish's pictures (`link_stand_in`), once linked.
    stand_in: Option<(usize, usize)>,
}

/// Whether a picture's pixels are in an `imagefile` pak that is not in
/// `dir`.
fn in_absent_pak(map: &Iw7Map, picture: &str, dir: &Path) -> bool {
    map.image_info(picture).is_some_and(|im| {
        im.pixels.is_none()
            && im.stream_files.iter().flatten().any(|f| {
                f.offset_end > f.offset
                    && !dir.join(format!("imagefile{}.pak", f.file_index)).is_file()
            })
    })
}

/// IW7's first-person rig: arms on a body, `j_mainroot`, the spine and
/// the clavicles between `tag_torso` and the shoulders, which its guns'
/// view animations pose.
const IW7_RIG: &str = "viewmodel_base_viewhands_iw7";

/// A skeleton's bind pose: per bone its name, parent and place on it.
struct Rig {
    index: HashMap<String, usize>,
    parent: Vec<Option<usize>>,
    bind: Vec<(Quat, Vec3)>,
    rest: Vec<Vec3>,
}

fn rig_of(w: &Walked, rec: &XModelRec) -> Option<Rig> {
    let strings = &w.zone.assets.script_strings;
    let names: Vec<String> = rec
        .bone_names
        .iter()
        .map(|&s| string(strings, s).to_ascii_lowercase())
        .collect();
    let n = names.len();
    if n == 0 || rec.base_mat.len() < n {
        return None;
    }
    let roots = usize::from(rec.num_root_bones).clamp(1, n);
    let parent: Vec<Option<usize>> = (0..n)
        .map(|b| {
            let step = usize::from(*rec.parent_list.get(b.checked_sub(roots)?)?);
            b.checked_sub(step).filter(|_| step > 0)
        })
        .collect();
    let world: Vec<(Quat, Vec3)> = rec
        .base_mat
        .iter()
        .take(n)
        .map(|m| {
            (
                Quat::from_xyzw(m[0], m[1], m[2], m[3]).normalize(),
                Vec3::new(m[4], m[5], m[6]),
            )
        })
        .collect();
    let bind = (0..n)
        .map(|b| match parent[b] {
            Some(p) => {
                let inv = world[p].0.inverse();
                (
                    (inv * world[b].0).normalize(),
                    inv * (world[b].1 - world[p].1),
                )
            }
            None => world[b],
        })
        .collect();
    let rest = (0..n)
        .map(|b| {
            b.checked_sub(roots)
                .and_then(|k| rec.tag_positions.get(k))
                .map_or(Vec3::ZERO, |t| Vec3::from_array(*t))
        })
        .collect();
    Some(Rig {
        index: names.into_iter().enumerate().map(|(i, n)| (n, i)).collect(),
        parent,
        bind,
        rest,
    })
}

/// IW7's first-person rig as the first-person catalog's skeleton
/// ([`asset_game::IW7_REFERENCE_ARMS`]): the view animations pose it and
/// Black Ops II's arms take its hands' places (`asset_game::FpvArmsRetarget`).
/// Its mesh is not read (never drawn).
fn rig_skel(w: &Walked, rec: &XModelRec, name: &str) -> Option<asset_model::ModelSkel> {
    let strings = &w.zone.assets.script_strings;
    let names: Vec<String> = rec.bone_names.iter().map(|&s| string(strings, s)).collect();
    let n = names.len();
    if n == 0 || n > 255 || rec.base_mat.len() < n {
        return None;
    }
    let roots = usize::from(rec.num_root_bones).clamp(1, n);
    let nonroot = n - roots;
    let xref = asset_t6::XModelRef {
        lighting_origin_offset: [0.0; 3],
        lighting_origin_range: 0.0,
        name: name.to_owned(),
        numsurfs: 0,
        materials: Vec::new(),
        num_bones: n as u8,
        num_lods: 1,
        mins: [0.0; 3],
        maxs: [0.0; 3],
        radius: 0.0,
        lods: vec![(0.0, 0, 0)],
        lod0: Vec::new(),
        bone_names: names,
        num_root_bones: roots as u8,
        parent_list: (0..nonroot)
            .map(|k| rec.parent_list.get(k).copied().unwrap_or(0))
            .collect(),
        quats: (0..nonroot)
            .map(|k| rec.tag_angles.get(k).copied().unwrap_or([0, 0, 0, 0x7fff]))
            .collect(),
        trans: (0..nonroot)
            .map(|k| rec.tag_positions.get(k).copied().unwrap_or([0.0; 3]))
            .collect(),
        base_mat: rec
            .base_mat
            .iter()
            .take(n)
            .map(|m| ([m[0], m[1], m[2], m[3]], [m[4], m[5], m[6]]))
            .collect(),
        part_classification: vec![0; n],
        bone_info: vec![None; n],
        contents: 0,
        coll_lod: -1,
        coll_surfs: Vec::new(),
        phys_preset: None,
    };
    asset_model::capture_model_skel_t6(&xref, |_| None)
}

/// `rig` posed by `clip` at `time`: each bone's place in the model. A
/// keyed bone takes the clip's turn and its rest offset plus the clip's;
/// an unkeyed one rests at its bind place.
fn posed(rig: &Rig, clip: &xmodel_runtime::AnimClip, time: f32) -> Vec<(Quat, Vec3)> {
    let mut local = rig.bind.clone();
    for (k, t) in clip.tracks.iter().enumerate() {
        let Some(&b) = rig.index.get(&t.name.to_ascii_lowercase()) else {
            continue;
        };
        let Some(s) = clip.sample_track(k, time) else {
            continue;
        };
        let q = s
            .rotation
            .map_or(Quat::IDENTITY, |q| Quat::from_array(q).normalize());
        local[b] = (
            q,
            rig.rest[b] + s.translation.map_or(Vec3::ZERO, Vec3::from_array),
        );
    }
    let mut world: Vec<(Quat, Vec3)> = Vec::with_capacity(local.len());
    for (b, &(q, t)) in local.iter().enumerate() {
        world.push(match rig.parent[b] {
            Some(p) if p < b => (world[p].0 * q, world[p].0 * t + world[p].1),
            _ => (q, t),
        });
    }
    world
}

/// IW7's arms built as Black Ops II's are (`ARMS_STAND_IN`'s use).
const ARMS_STAND_IN: &str = "viewmodel_base_viewhands";

/// What the view model checks pose a gun on: IW7's rig, as itself and as
/// the first-person catalog's skeleton, and arms of Black Ops II's make.
struct Hands {
    rig: Rig,
    rig_pose: Option<xmodel_runtime::ModelPoseSrc>,
    arms: Option<xmodel_runtime::ModelPoseSrc>,
}

/// How far `arms`' wrists are from the gun's hand tags (`tag_ik_loc_ri`,
/// `_le`) with the gun on the arms' `tag_weapon` and `clip` played at its
/// start: straight onto the arms, and through the retarget onto IW7's rig
/// (`asset_game::FpvArmsRetarget::build_iw7`), as the first person view
/// plays it.
fn arms_fit(
    arms: &xmodel_runtime::ModelPoseSrc,
    rig: &xmodel_runtime::ModelPoseSrc,
    gun: &xmodel_runtime::ModelPoseSrc,
    clip: &xmodel_runtime::AnimClip,
) -> Option<[(f32, f32); 2]> {
    use xmodel_runtime::{AnimInstance, Attach, DObj};
    let dobj = DObj::build(&[
        (arms, None),
        (
            gun,
            Some(Attach {
                parent_model: 0,
                tag: "tag_weapon".into(),
            }),
        ),
    ])
    .ok()?;
    let retarget = asset_game::FpvArmsRetarget::build_iw7(rig, &dobj)?;
    let tracks = dobj.tracks_for(clip);
    let anims = [AnimInstance {
        clip,
        tracks: &tracks,
        time: 0.0,
        weight: 1.0,
        parts: None,
    }];
    let gun_base = dobj.models.get(1)?.base;
    let find_in = |base: usize, count: usize, name: &str| {
        (base..base + count).find(|&b| dobj.bones[b].name.eq_ignore_ascii_case(name))
    };
    let arms_n = dobj.models[0].bone_count;
    let gun_n = dobj.models[1].bone_count;
    let fit = |world: Vec<bevy::math::Mat4>| {
        let at = |b: Option<usize>| b.map(|b| world[b].w_axis.truncate());
        let d = |a: Option<Vec3>, b: Option<Vec3>| match (a, b) {
            (Some(a), Some(b)) => a.distance(b),
            _ => f32::NAN,
        };
        (
            d(
                at(find_in(0, arms_n, "j_wrist_ri")),
                at(find_in(gun_base, gun_n, "tag_ik_loc_ri")),
            ),
            d(
                at(find_in(0, arms_n, "j_wrist_le")),
                at(find_in(gun_base, gun_n, "tag_ik_loc_le")),
            ),
        )
    };
    let mut locals = dobj.calc_anim(&anims, &dobj.all_parts());
    let straight = fit(dobj.compose(&locals, bevy::math::Mat4::IDENTITY));
    retarget.apply(&dobj, &anims, &mut locals);
    Some([
        straight,
        fit(dobj.compose(&locals, bevy::math::Mat4::IDENTITY)),
    ])
}

/// A note that names a weapon sound alias (`weap_m4_clipout_plr`), not an
/// effect or a hand cue (`fx_playfxontag, ...`, `ik_in_start_left_hand`).
fn is_sound_note(note: &str) -> bool {
    (note.starts_with("weap_") || note.starts_with("wpn_")) && !note.contains([' ', ','])
}

/// The tags of IW7's rig a gun's root could hang on.
const GUN_TAGS: [&str; 4] = ["tag_weapon_right", "tag_weapon", "tag_weapon_left", "j_gun"];

/// What kind of map a picture is by its name: IW7 names a packed picture
/// by what it packs (`..._packed_cs`: colour and smoothness;
/// `..._packed_ng`, `_nog`: normal and gloss; `_packed_a`: opacity), others
/// by their last word (`_c`, `_n`, `_s`, `_sg`, `_g`, `_o`, `_e`).
fn picture_kind(name: &str) -> &'static str {
    let n = name.to_ascii_lowercase();
    let word = n
        .rsplit_once("_packed_")
        .map_or_else(|| n.rsplit('_').next().unwrap_or(""), |(_, w)| w);
    match word {
        w if w.starts_with('c') || w == "col" || w == "color" => "colour",
        w if w.starts_with('n') => "normal",
        "s" | "sg" | "spec" | "g" | "gloss" => "specular",
        "o" | "occ" | "ao" => "occlusion",
        "a" | "alpha" => "opacity",
        "e" => "emissive",
        _ if n.contains("normal") => "normal",
        _ if n.contains("spec") || n.contains("gloss") => "specular",
        _ => "colour",
    }
}

/// The stand-in finish of a gun surface whose colour picture is not here:
/// a gun-metal grey and a dull, dielectric specular (gloss in alpha), so
/// the surface draws as a plain gun part, not as a missing one or a mirror.
fn link_stand_in(catalog: &mut MaterialCatalog) -> (usize, usize) {
    use bevy::asset::RenderAssetUsages;
    use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
    let mut link = |name: &str, semantic: u8, px: [u8; 4], srgb: bool| {
        let mut image = bevy::prelude::Image::new_uninit(
            Extent3d {
                width: 4,
                height: 4,
                depth_or_array_layers: 1,
            },
            TextureDimension::D2,
            if srgb {
                TextureFormat::Rgba8UnormSrgb
            } else {
                TextureFormat::Rgba8Unorm
            },
            RenderAssetUsages::RENDER_WORLD,
        );
        image.data = Some(px.repeat(16));
        catalog.link_image(AuthoredImage {
            namespace: AssetNamespace::T6,
            name: asset_core::AssetRef::Real(name.to_owned()),
            map_type: 0,
            semantic,
            category: 0,
            use_srgb_reads: srgb,
            width: 4,
            height: 4,
            depth: 1,
            level_count: 1,
            format: 0,
            payload: Arc::new(Vec::new()),
            decoded: Some(Arc::new(image)),
            common_owned: false,
            decoded_variant: None,
            decoded_by: None,
            pending_decode: None,
        })
    };
    (
        link(
            "iw7:gun_stand_in#colour",
            TS_COLOR_MAP,
            [0x52, 0x54, 0x58, 0xff],
            true,
        ),
        link(
            "iw7:gun_stand_in#specular",
            TS_SPECULAR_MAP,
            [0x2c, 0x2c, 0x2c, 0x50],
            false,
        ),
    )
}

/// `slot`'s material drawn with the stand-in finish (`link_stand_in`)
/// when it has no colour picture.
fn give_stand_in(catalog: &mut MaterialCatalog, slot: usize, (colour, specular): (usize, usize)) {
    let textures = &mut catalog.materials[slot].textures;
    if textures
        .iter()
        .any(|t| t.semantic == TS_COLOR_MAP && t.image.is_some())
    {
        return;
    }
    textures.retain(|t| t.semantic != TS_COLOR_MAP && t.semantic != TS_SPECULAR_MAP);
    for (semantic, image, hash) in [
        (
            TS_COLOR_MAP,
            colour,
            fastfile_iw7::texture::name_hash("colorMap"),
        ),
        (TS_SPECULAR_MAP, specular, 0x34ec_ccb3),
    ] {
        textures.push(MaterialTextureBinding {
            name_hash: hash,
            name_start: 0,
            name_end: 0,
            sampler_state: 0x13,
            semantic,
            image: Some(image),
        });
    }
}

/// One gun into `out`.
fn build_gun(
    gun: &str,
    dir: &Path,
    core: &Core,
    shared: &Arc<SharedZones>,
    hands: Option<&Hands>,
    seed: &mut MaterialCatalog,
    out: &mut Out,
) -> Result<(), String> {
    let (wz, rec) = core
        .weapon(&iw7_weapon_name(gun))
        .ok_or_else(|| format!("{}: no weapon", iw7_weapon_name(gun)))?;
    let wcap = &core.zones[wz].cap;
    let wimage = &core.zones[wz].zone.image[..];
    let wstrings = &core.zones[wz].zone.assets.script_strings;
    let field = |f: WeaponField| rec.assets.iter().find(|(x, _)| *x == f).map(|(_, id)| *id);
    let named = |f: WeaponField| {
        field(f)
            .map(|id| bare(wcap.name_of(id)).to_owned())
            .filter(|n| !n.is_empty())
    };
    let mut r = Iw7GunReport {
        gun: gun.to_owned(),
        ..Iw7GunReport::default()
    };
    let zone_gun = iw7_zone_gun(gun);
    let view_zone = walk(dir, &format!("mp_weapon_view_iw7_{zone_gun}_tr"))?;
    let world_zone = walk(dir, &format!("mp_weapon_world_iw7_{zone_gun}_tr"))?;
    let colour = |catalog: &MaterialCatalog, slot: usize| {
        catalog.materials[slot]
            .textures
            .iter()
            .any(|t| t.semantic == TS_COLOR_MAP && t.image.is_some())
    };
    let owner = ZoneOwner::from_zone_path(&dir.join("global_core_mp.ff"));

    // Models, each with its materials.
    let mut view_bones: Vec<String> = Vec::new();
    let mut view_bind: Vec<(Quat, Vec3)> = Vec::new();
    let mut view_bounds: Option<(Vec3, Vec3)> = None;
    let mut view_pose: Option<xmodel_runtime::ModelPoseSrc> = None;
    let gun_model = named(WeaponField::Def(D::gunXModel));
    let world_model = named(WeaponField::Def(D::worldModel));
    let first_person_max = super::helpers::gun_texture_cap("BO2MP_IW7_TEXTURE", FIRST_PERSON_MAX);
    for (name, zone, max, first_person) in [
        (gun_model.clone(), &view_zone, first_person_max, true),
        (world_model.clone(), &world_zone, WORLD_MAX, false),
    ] {
        let Some(name) = name else { continue };
        let Some((w, m)) = find_model(&name, zone, core) else {
            out.failures.push(format!("{gun}: model {name} not found"));
            continue;
        };
        let Some(ModelRef {
            mut xref,
            materials: mut surface_materials,
            bad_blend,
            weight_over,
            garbled,
            left_out,
        }) = model_ref(w, m)
        else {
            out.failures.push(format!("{gun}: {name} (no surfaces)"));
            continue;
        };
        out.failures
            .extend(left_out.into_iter().map(|l| format!("{gun}: {name} {l}")));
        // (A scope's stencil, which only masks the scope's picture, is not
        // drawn.)
        let stencil = |m: &Option<String>| {
            m.as_deref()
                .and_then(|n| shared.material(n))
                .is_some_and(|m| m.technique_set.split('_').any(|w| w == "stencil"))
        };
        // (Maya's default material, left on a few stray faces of the
        // ARX-160's, Karma-45's and S-Ravage's models: 5 to 25 vertices,
        // drawn as grey lumps with the stand-in finish.)
        let stray = |m: &Option<String>| m.as_deref().is_some_and(|m| m.ends_with("lambert1"));
        let keep: Vec<bool> = surface_materials
            .iter()
            .map(|m| !stencil(m) && !stray(m))
            .collect();
        if keep.iter().any(|k| !k) {
            let mut k = keep.iter();
            xref.lod0.retain(|_| *k.next().unwrap_or(&true));
            let mut k = keep.iter();
            surface_materials.retain(|_| *k.next().unwrap_or(&true));
            xref.numsurfs = xref.lod0.len().min(255) as u8;
            xref.lods = vec![(0.0, xref.lod0.len() as u16, 0)];
        }
        let verts: usize = xref.lod0.iter().map(|s| usize::from(s.vert_count)).sum();
        let tris: usize = xref.lod0.iter().map(|s| s.indices.len() / 3).sum();
        let shape = (verts, tris, xref.bone_names.len());
        if first_person {
            r.view_model = xref.name.clone();
            r.root = xref.bone_names.first().cloned().unwrap_or_else(String::new);
            r.view = Some(shape);
            r.bad_blend = bad_blend;
            r.weight_over = weight_over;
            r.garbled = garbled;
            view_bones = xref.bone_names.clone();
            view_bind = xref
                .base_mat
                .iter()
                .map(|(q, t)| (Quat::from_array(*q).normalize(), Vec3::from_array(*t)))
                .collect();
            view_bounds = Some((Vec3::from_array(xref.mins), Vec3::from_array(xref.maxs)));
        } else {
            r.world_model = xref.name.clone();
            r.world = Some(shape);
        }
        // (The materials of this model, defined in the technique-set zones.)
        let mut names: Vec<String> = Vec::new();
        for m in surface_materials.iter().flatten() {
            if !names.contains(m) {
                names.push(m.clone());
            }
        }
        let defined: Vec<MapMaterial> = names
            .iter()
            .filter_map(|n| {
                let m = shared.material(n).cloned();
                if m.is_none() && !r.undefined.contains(n) {
                    r.undefined.push(n.clone());
                }
                m
            })
            .collect();
        let map = Iw7Map::for_materials(gun, defined, shared.clone());
        let mut b = Materials::for_gun(&map, std::mem::take(seed), owner, max);
        let by_name: HashMap<String, usize> = map
            .materials
            .iter()
            .enumerate()
            .map(|(i, m)| (m.name.to_ascii_lowercase(), i))
            .collect();
        let mut slots: HashMap<usize, usize> = HashMap::new();
        for (k, n) in names.iter().enumerate() {
            if let Some(slot) = by_name
                .get(&n.to_ascii_lowercase())
                .and_then(|&i| b.material(i, true))
            {
                slots.insert(k, slot);
                r.materials += 1;
                let m = &map.materials[by_name[&n.to_ascii_lowercase()]];
                if let Some(p) = m.texture("colorMap")
                    && picture_kind(p) != "colour"
                {
                    r.colour_not_albedo.push(format!("{} ({p})", m.name));
                }
                if let Some((e, scale)) = fastfile_iw7::glow::emissive(m) {
                    r.emissive
                        .push(format!("{} ({e}, x{scale}, {})", m.name, m.technique_set));
                }
                for slot_name in ["colorMap", "normalMap"] {
                    if let Some(p) = m.texture(slot_name).filter(|p| !p.starts_with('$'))
                        && !r.pictures.iter().any(|x| x.0 == p)
                        && let Some(im) = map.image_info(p)
                    {
                        let top = im
                            .streams
                            .iter()
                            .map(|s| u32::from(s.0.max(s.1)))
                            .max()
                            .unwrap_or(0)
                            .max(u32::from(im.width.max(im.height)));
                        let mut paks: Vec<u16> = im
                            .stream_files
                            .iter()
                            .flatten()
                            .filter(|f| f.offset_end > f.offset)
                            .map(|f| f.file_index)
                            .collect();
                        paks.sort_unstable();
                        paks.dedup();
                        r.pictures.push((p.to_owned(), top, paks));
                    }
                }
                if !colour(&b.catalog, slot) {
                    let stand_in = *out
                        .stand_in
                        .get_or_insert_with(|| link_stand_in(&mut b.catalog));
                    give_stand_in(&mut b.catalog, slot, stand_in);
                    r.stand_in += 1;
                    match m.texture("colorMap") {
                        Some(p) if in_absent_pak(&map, p, &map_pak_dir(shared)) => {
                            r.grey_absent_pak += 1
                        }
                        _ => {
                            r.grey_missing += 1;
                            r.no_picture
                                .push(format!("{} ({})", m.name, m.technique_set));
                        }
                    }
                }
            }
        }
        r.glowing += b.with_gun_glow;
        out.decoded += b.decoded;
        out.failures.extend(b.failures.iter().cloned());
        let key = |k: usize| asset_t6::AssetKey {
            ty: fastfile_t6::AssetType::Material,
            index: k,
        };
        let index_of =
            |m: &Option<String>| m.as_ref().and_then(|m| names.iter().position(|n| n == m));
        xref.materials = surface_materials
            .iter()
            .map(|m| index_of(m).map(key))
            .collect();
        for (s, m) in xref.lod0.iter_mut().zip(&surface_materials) {
            s.material = index_of(m).map(key);
        }
        let catalog = std::mem::take(&mut b.catalog);
        match asset_model::capture_model_skel_t6(&xref, |k| slots.get(&k.index).copied()) {
            Some(skel) if first_person => {
                view_pose = skel.pose.clone();
                out.fpv.insert_in(NS, skel, Some(&catalog));
            }
            Some(skel) => out.world_guns.insert_in(NS, skel, Some(&catalog)),
            None => out.failures.push(format!("{gun}: {name} (unreadable)")),
        }
        *seed = catalog;
    }

    // Animations: the package's slots as AW's. IW7's zones reach most
    // assets through the shared streams, which the walk places by fitted
    // offsets: a slot's reference can land on another of the gun's
    // animations. A slot keeps its animation when the name says it is one
    // for the slot (`vm_ar57_reload_empty` for the empty reload), else
    // takes the gun's animation named for the slot, else stays empty.
    let package_names = [
        format!("{}_package", iw7_weapon_name(gun)),
        format!("iw7_{zone_gun}_mp_package"),
    ];
    let package = by_name_or(
        core,
        T::AnimPackage,
        &package_names,
        field(WeaponField::Def(D::szXAnims)).and_then(|id| core.resolve(wz, id)),
    );
    let mut slots: Vec<String> = vec![String::new(); S1_ANIM_SLOTS];
    let mut notes: Vec<String> = Vec::new();
    let mut timers: Vec<u8> = Vec::new();
    if let Some((pz, pid)) = package {
        let pcap = &core.zones[pz].cap;
        let package = &pcap.anim_packages[pid.index as usize];
        timers = package
            .timers
            .map_or_else(Vec::new, |t| t.get(&core.zones[pz].zone.image).to_vec());
        let names: Vec<String> = package
            .anims
            .iter()
            .flatten()
            .map(|id| bare(pcap.name_of(*id)).to_ascii_lowercase())
            .collect();
        let stems = anim_stems(&names);
        if std::env::var_os("IW7_DUMP_ANIMS").is_some_and(|v| v.to_string_lossy() == gun) {
            for (i, a) in package.anims.iter().enumerate() {
                if let Some(a) = a {
                    eprintln!("ANIMDUMP {gun} {i} {}", bare(pcap.name_of(*a)));
                }
            }
            let mut all: Vec<&String> = core
                .index
                .keys()
                .filter(|(t, n)| {
                    *t == T::XAnimParts && stems.iter().any(|s| n.starts_with(s.as_str()))
                })
                .map(|(_, n)| n)
                .collect();
            all.sort();
            for n in all {
                eprintln!("ANIMALL {gun} {n}");
            }
        }
        for &(iw7, s1, endings) in &ANIM_SLOTS {
            let referenced = package.anims.get(iw7).copied().flatten();
            let named = referenced.map(|id| bare(pcap.name_of(id)).to_ascii_lowercase());
            let fits = |n: &str| {
                stems.iter().any(|stem| {
                    endings.iter().any(|e| {
                        n == format!("{stem}_{e}") || n.starts_with(&format!("{stem}_{e}_"))
                    })
                })
            };
            let hit = match (&named, referenced) {
                (Some(n), Some(id)) if fits(n) => {
                    let hit = core.resolve(pz, id);
                    if hit.is_none() {
                        r.missing_anims.push(n.clone());
                    }
                    hit
                }
                _ => {
                    let found = stems.iter().find_map(|stem| {
                        endings.iter().find_map(|e| {
                            core.index
                                .get(&(T::XAnimParts, format!("{stem}_{e}")))
                                .copied()
                        })
                    });
                    if found.is_some() {
                        r.by_name += 1;
                    } else if named.is_some() {
                        r.dropped += 1;
                    }
                    found
                }
            };
            let Some((az, aid)) = hit else {
                r.empty_slots.push(endings[0].to_owned());
                continue;
            };
            let a = &core.zones[az].cap.xanims[aid.index as usize];
            if a.numframes == 0 && a.names.is_empty() {
                continue;
            }
            let anim = anim_ref(&core.zones[az], a);
            if iw7 == 107 {
                let gun_bones: HashSet<&str> = view_bones.iter().map(String::as_str).collect();
                for bn in &anim.names {
                    if gun_bones.contains(bn.as_str()) {
                        r.idle_bones.0 += 1;
                    } else if is_arms_bone(bn) {
                        r.idle_bones.1 += 1;
                    } else {
                        r.idle_missing.push(bn.clone());
                    }
                }
            }
            for (note, _) in &anim.notifies {
                let n = note.to_ascii_lowercase();
                if is_sound_note(&n) && !notes.contains(&n) {
                    notes.push(n);
                }
            }
            if out.xanims.insert_t6_as(NS, &anim) {
                r.anims += 1;
            }
            slots[s1] = anim.name.to_ascii_lowercase();
        }
    }

    // Where the idle puts the gun in IW7's own hands.
    r.idle = slots[1].clone();
    if let (Some(hands), Some(clip)) = (
        hands,
        out.xanims
            .get(NS, &r.idle)
            .and_then(|c| xmodel_runtime::AnimClip::from_parts(&c.parts).ok()),
    ) {
        let rig = &hands.rig;
        if let (Some(arms), Some(rig_pose), Some(gun)) = (&hands.arms, &hands.rig_pose, &view_pose)
        {
            r.arms_fit = arms_fit(arms, rig_pose, gun, &clip);
        }
        let world = posed(rig, &clip, 0.0);
        let at = |name: &str| rig.index.get(name).map(|&b| world[b]);
        let root_inv = view_bind.first().map_or(Quat::IDENTITY, |b| b.0.inverse());
        let root_t = view_bind.first().map_or(Vec3::ZERO, |b| b.1);
        let in_gun = |name: &str| {
            view_bones
                .iter()
                .position(|b| b.eq_ignore_ascii_case(name))
                .and_then(|i| view_bind.get(i))
                .map(|b| root_inv * (b.1 - root_t))
        };
        let (grip_ri, grip_le) = (in_gun("tag_ik_loc_ri"), in_gun("tag_ik_loc_le"));
        let (wrist_ri, wrist_le) = (at("j_wrist_ri"), at("j_wrist_le"));
        let dist =
            |g: (Quat, Vec3), tag: Option<Vec3>, hand: Option<(Quat, Vec3)>| match (tag, hand) {
                (Some(tag), Some(hand)) => (g.0 * tag + g.1).distance(hand.1),
                _ => f32::NAN,
            };
        let mut best: Option<(f32, &str, (Quat, Vec3))> = None;
        for tag in GUN_TAGS {
            let Some(g) = at(tag) else { continue };
            let (dr, dl) = (dist(g, grip_ri, wrist_ri), dist(g, grip_le, wrist_le));
            r.grip.push((tag.to_owned(), dr, dl));
            if dr.is_finite() && best.is_none_or(|b| dr < b.0) {
                best = Some((dr, tag, g));
            }
        }
        if let (Some((_, tag, g)), Some((lo, hi)), Some(view)) = (best, view_bounds, at("tag_view"))
        {
            r.hangs_on = tag.to_owned();
            let inv = view.0.inverse();
            let (mut mins, mut maxs) = ([f32::MAX; 3], [f32::MIN; 3]);
            for k in 0..8 {
                let c = Vec3::new(
                    if k & 1 == 0 { lo.x } else { hi.x },
                    if k & 2 == 0 { lo.y } else { hi.y },
                    if k & 4 == 0 { lo.z } else { hi.z },
                );
                let p = inv * (g.0 * (root_inv * (c - root_t)) + g.1 - view.1);
                for (a, v) in [p.x, p.y, p.z].into_iter().enumerate() {
                    mins[a] = mins[a].min(v);
                    maxs[a] = maxs[a].max(v);
                }
            }
            r.view_box = Some((mins, maxs));
        }
    }

    // Sounds its package names (Black Ops II's play in the match:
    // `iw7_guest_sounds`).
    let mut sounds: Vec<(usize, String)> = Vec::new();
    let sfx_names = [
        format!("{}_sfxpackage", iw7_weapon_name(gun)),
        format!("iw7_{zone_gun}_mp_sfxpackage"),
    ];
    if let Some((sz, sid)) = by_name_or(
        core,
        T::SfxPackage,
        &sfx_names,
        field(WeaponField::Def(D::sfxPackage)).and_then(|id| core.resolve(wz, id)),
    ) {
        let p = &core.zones[sz].cap.sfx_packages[sid.index as usize];
        for &(iw7, s1) in super::iw7_gun_fields::SOUNDS {
            if let Some((_, alias)) = p
                .sounds
                .iter()
                .find(|(o, a)| *o == iw7 && a.len() >= 3 && a.bytes().all(|b| b.is_ascii_graphic()))
            {
                sounds.push((s1, alias.clone()));
            }
        }
    }
    r.sounds = sounds.iter().map(|(_, a)| a.clone()).collect();
    r.sounds.sort();
    r.sounds.dedup();
    // (An alias whose clips are all in absent banks plays its donor's.)
    let playable = |a: &str| asset_audio::iw7_alias_playable(dir, a);
    (r.sounds_own, r.sounds_donor) = r.sounds.iter().cloned().partition(|a| playable(a));
    sounds.retain(|(_, a)| playable(a));
    (r.notes_own, r.notes_donor) = notes.into_iter().partition(|n| playable(n));
    let notetrack_sounds: Vec<(String, String)> =
        r.notes_own.iter().map(|n| (n.clone(), n.clone())).collect();
    r.hud_icon = named(WeaponField::Def(D::hudIcon));

    // The weapon, as AW's capture reads it.
    use fastfile_s1::layout::WeaponDef as s;
    let def = super::iw7_gun_fields::s1_def(
        rec.def.map_or(&[][..], |d| d.get(wimage)),
        rec.complete.get(wimage),
        &timers,
    );
    r.view_offset_max = [s::duckedOfs, s::proneOfs, s::standMove]
        .iter()
        .flat_map(|&o| (0..3).map(move |k| o + 4 * k))
        .filter_map(|at| def.get(at..at + 4))
        .map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]]).abs())
        .fold(0.0, f32::max);

    let materials: Vec<(usize, String)> = [
        (WeaponField::Def(D::hudIcon), s::hudIcon),
        (WeaponField::Def(D::pickupIcon), s::pickupIcon),
        (WeaponField::Complete(C::killIcon), s::killIcon),
        (WeaponField::Complete(C::dpadIcon), s::dpadIcon),
        (WeaponField::Def(D::reticleCenter), s::reticleCenter),
        (WeaponField::Def(D::reticleSide), s::reticleSide),
        (WeaponField::Def(D::overlay__shader), s::overlay__shader),
        (
            WeaponField::Def(D::overlay__shaderLowRes),
            s::overlay__shaderLowRes,
        ),
    ]
    .into_iter()
    .filter_map(|(f, o)| Some((o, named(f)?)))
    .collect();
    let models: Vec<(usize, String)> = [
        (WeaponField::Def(D::projectileModel), s::projectileModel),
        (WeaponField::Def(D::rocketModel), s::rocketModel),
        (WeaponField::Def(D::knifeModel), s::knifeModel),
    ]
    .into_iter()
    .filter_map(|(f, o)| Some((o, named(f)?)))
    .collect();
    let view = S1WeaponView {
        // (Named by the roster: `iw7_m4_mp` is the IW7 namespace's `m4`,
        // which a Black Ops II match gives as `iw7_m4_mp`.)
        name: format!("{gun}_mp"),
        display_name: format!("WEAPON_IW7_{}", gun.to_ascii_uppercase()),
        def,
        gun_model: gun_model.filter(|_| r.view.is_some()),
        world_model: world_model.filter(|_| r.world.is_some()),
        xanims: slots,
        hide_tags: rec
            .hide_tags
            .iter()
            .map(|&h| string(wstrings, h))
            .filter(|h| !h.is_empty())
            .collect(),
        alt_weapon_name: String::new(),
        sounds,
        fx: Vec::new(),
        models,
        materials,
        notetrack_sounds,
        location_damage: None,
        tracer: None,
        impact_row: Vec::new(),
    };
    if out.catalog.capture_s1(&view) {
        // (No sprint clips in IW7's packages: a BO2-like sprint pose.)
        out.catalog.give_last_guest_sprint_pose();
    }
    out.built.push(r);
    Ok(())
}

/// Where the `imagefile` paks are (beside the zones).
fn map_pak_dir(shared: &SharedZones) -> PathBuf {
    shared.pak_dir.clone()
}

/// Infinite Warfare's guns from its zones in `dir`, their materials linked
/// into `material_seed`. A later start reads the finished guns back.
fn load_iw7_guests(dir: &Path, material_seed: MaterialCatalog) -> Iw7Guests {
    let fp = super::helpers::gun_texture_cap("BO2MP_IW7_TEXTURE", FIRST_PERSON_MAX);
    let caps = [("fp", fp), ("world", WORLD_MAX)];
    let code = [
        include_str!("finished_gun_cache.rs"),
        include_str!("iw7_gun.rs"),
    ];
    let files = iw7_source_files(dir);
    let names = vec!["guns".to_owned()];
    let src = super::finished_gun_cache::Sources {
        lane: "iw7",
        name: "guns",
        files: &files,
        caps: &caps,
        code: &code,
        zones: &names,
        note: "",
        missing_ok: false,
    };
    super::finished_gun_cache::load_or_build(
        &src,
        material_seed,
        |seed| load_iw7_guests_now(dir, seed),
        pack_iw7,
        unpack_iw7,
    )
}

fn iw7_source_files(dir: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    if let Ok(read) = std::fs::read_dir(dir) {
        for entry in read.flatten() {
            let name = entry.file_name().to_string_lossy().to_ascii_lowercase();
            if name.ends_with(".ff") || name.ends_with(".pak") {
                files.push(entry.path());
            }
        }
    }
    files
}

#[derive(serde::Serialize, serde::Deserialize)]
struct Iw7Extra {
    built: Vec<Iw7GunReport>,
    aliases: Vec<String>,
}

fn pack_iw7(guests: &Iw7Guests, at: super::finished_gun_cache::MatAt) -> Result<Vec<u8>, String> {
    let extra = serde_json::to_vec(&Iw7Extra {
        built: guests.built.clone(),
        aliases: guests.aliases.iter().cloned().collect(),
    })
    .map_err(|e| e.to_string())?;
    let projectiles = asset_model::ProjectileMeshBuild::default();
    super::finished_gun_cache::pack_parts(&super::finished_gun_cache::Parts {
        weapons: &guests.weapons,
        fpv: &guests.fpv,
        world: &guests.world_guns,
        projectiles: &projectiles,
        xanims: &guests.xanims,
        fx: &guests.fx,
        fx_models: &guests.fx_models,
        tracers: &guests.tracers,
        impact: None,
        materials: &guests.material_seed,
        mat_at: at,
        reports: &guests.report,
        extra: &extra,
    })
}

fn unpack_iw7(bytes: &[u8], seed: MaterialCatalog) -> Result<Iw7Guests, MaterialCatalog> {
    match super::finished_gun_cache::unpack_parts(bytes, seed) {
        Ok(rest) => {
            let extra: Iw7Extra = serde_json::from_slice(&rest.extra).unwrap_or(Iw7Extra {
                built: Vec::new(),
                aliases: Vec::new(),
            });
            Ok(Iw7Guests {
                weapons: rest.weapons,
                fpv: rest.fpv,
                world_guns: rest.world,
                xanims: rest.xanims,
                material_seed: rest.materials,
                report: rest.reports,
                built: extra.built,
                aliases: extra.aliases.into_iter().collect(),
                fx: rest.fx,
                fx_models: rest.fx_models,
                tracers: rest.tracers,
            })
        }
        Err(seed) => Err(seed),
    }
}

fn load_iw7_guests_now(dir: &Path, mut material_seed: MaterialCatalog) -> Iw7Guests {
    let started = std::time::Instant::now();
    let mut report = vec![format!(
        "iw7 guest guns: Infinite Warfare's zones in {}",
        dir.display()
    )];
    let mut zones = Vec::new();
    for name in CORE_ZONES {
        match walk(dir, name) {
            Ok(w) => {
                report.push(format!(
                    "iw7 guest guns: {name}: {} assets walked{}",
                    w.cap.walked,
                    w.stop.as_ref().map_or_else(String::new, |s| format!(
                        ", stopped at asset {} (what came before kept)",
                        s.asset_index
                    ))
                ));
                zones.push(w);
            }
            Err(e) => report.push(format!("iw7 guest guns: {e}")),
        }
    }
    let core = Core::new(zones);
    if !core.zones.iter().any(|z| z.name == "global_core_mp") {
        report.push("iw7 guest guns: no global_core_mp: no guns".into());
        return Iw7Guests::empty(material_seed, report);
    }
    // Materials and pictures by name: the technique-set zones' materials,
    // the core zones' pictures.
    let mut shared = SharedZones::default();
    shared.pak_dir = dir.to_path_buf();
    let mut aliases: HashSet<String> = HashSet::new();
    // (Every walked zone's sound bank, for IW7's own gun sounds.)
    let mut banks: Vec<(String, fastfile_iw7::walk::SoundBankRec)> = core
        .zones
        .iter()
        .flat_map(|w| {
            w.cap
                .sound_banks
                .iter()
                .map(|b| (w.name.clone(), b.clone()))
        })
        .collect();
    for w in &core.zones {
        let line = shared.add_walked(&w.name, &w.zone, &w.cap, w.stop.as_ref());
        shared.zones.push(line);
        aliases.extend(
            w.cap
                .sound_banks
                .iter()
                .flat_map(|b| b.aliases.iter().map(|a| a.name.clone())),
        );
    }
    for name in MATERIAL_ZONES {
        match walk(dir, name) {
            Ok(w) => {
                let line = shared.add_walked(&w.name, &w.zone, &w.cap, w.stop.as_ref());
                shared.zones.push(line);
                aliases.extend(
                    w.cap
                        .sound_banks
                        .iter()
                        .flat_map(|b| b.aliases.iter().map(|a| a.name.clone())),
                );
                banks.extend(
                    w.cap
                        .sound_banks
                        .iter()
                        .map(|b| (w.name.clone(), b.clone())),
                );
            }
            Err(e) => report.push(format!("iw7 guest guns: {e}")),
        }
    }
    // IW7's own gun sounds: the banks' aliases whose clips are in the
    // `.sabl`/`.sabs` files here (`asset_audio::iw7_sounds_from_banks`,
    // kept for the match's sound bank under the IW7 namespace).
    let ordered: Vec<(&str, &fastfile_iw7::walk::SoundBankRec)> = asset_audio::IW7_SOUND_ZONES
        .iter()
        .flat_map(|z| {
            banks
                .iter()
                .filter(move |(n, _)| n == z)
                .map(|(n, b)| (n.as_str(), b))
        })
        .collect();
    let (_, census) = asset_audio::iw7_sounds_from_banks(dir, &ordered);
    report.push(format!(
        "iw7 guest guns: own sounds: {} alias lists kept, {} records with their clip, {} whose clip's bank is not here; banks {:?}",
        census.lists,
        census.playable,
        census.absent,
        census.banks
    ));
    report.push(format!(
        "iw7 guest guns: {} materials and {} pictures by name, {} sound aliases [{:.1} s]",
        shared.material_count(),
        shared.image_count(),
        aliases.len(),
        started.elapsed().as_secs_f64()
    ));
    let shared = Arc::new(shared);
    let mut out = Out {
        catalog: WeaponCatalog::default(),
        fpv: FpvMeshBuild::default(),
        world_guns: WorldWeaponBuild::default(),
        xanims: XAnimBuild::default(),
        built: Vec::new(),
        decoded: 0,
        failures: Vec::new(),
        scopes: Vec::new(),
        stand_in: None,
    };
    out.catalog.set_capture_ns(NS);
    out.fpv.set_capture_ns(NS);
    out.world_guns.set_capture_ns(NS);
    out.xanims.set_capture_ns(NS);
    let rig_rec = core
        .index
        .get(&(T::XModel, IW7_RIG.to_owned()))
        .and_then(|&(z, id)| {
            Some((
                &core.zones[z],
                core.zones[z].cap.xmodels.get(id.index as usize)?,
            ))
        });
    let rig = rig_rec.and_then(|(w, rec)| rig_of(w, rec));
    // (Black Ops II's arms take its hands' places: without it the view
    // animations' arm turns, made for its spine and clavicles, throw the
    // hands off the gun.)
    let rig_model = rig_rec.and_then(|(w, rec)| rig_skel(w, rec, asset_game::IW7_REFERENCE_ARMS));
    // (IW7's own arms of Black Ops II's make, shoulders on `tag_torso`,
    // fingers from `_0`, palms and wrist twists by Black Ops II's names:
    // Black Ops II's arms' stand-in for the checks.)
    let arms = core
        .index
        .get(&(T::XModel, ARMS_STAND_IN.to_owned()))
        .and_then(|&(z, id)| {
            rig_skel(
                &core.zones[z],
                core.zones[z].cap.xmodels.get(id.index as usize)?,
                ARMS_STAND_IN,
            )
        })
        .and_then(|s| s.pose);
    let hands = rig.map(|rig| Hands {
        rig,
        rig_pose: rig_model.as_ref().and_then(|s| s.pose.clone()),
        arms,
    });
    let rig_in_catalog = match rig_model {
        Some(skel) => {
            out.fpv.insert_in(NS, skel, None);
            true
        }
        None => false,
    };
    report.push(format!(
        "iw7 guest guns: IW7's first-person rig {IW7_RIG}: {}",
        match (hands.is_some(), rig_in_catalog) {
            (true, true) => "found, Black Ops II's arms follow its hands",
            (true, false) => "found, not built (view animations stay on BO2's arms as they are)",
            _ => "not found (view animations stay on BO2's arms as they are)",
        }
    ));
    for g in bo2_profile::guest::IW7 {
        if let Err(e) = build_gun(
            g.gun,
            dir,
            &core,
            &shared,
            hands.as_ref(),
            &mut material_seed,
            &mut out,
        ) {
            out.failures.push(e);
        }
    }
    let missing: Vec<&str> = bo2_profile::guest::IW7
        .iter()
        .map(|g| g.gun)
        .filter(|g| !out.built.iter().any(|r| r.gun == *g))
        .collect();
    // (Pictures in `imagefile` paks that are not there are counted, not
    // listed.)
    let (absent, problems): (Vec<&String>, Vec<&String>) = out
        .failures
        .iter()
        .partition(|f| f.ends_with(".pak not found"));
    report.push(format!(
        "iw7 guest guns: models: {} first person, {} third person; {} pictures decoded, {} in absent imagefile paks; problems {:?}; not built {:?}",
        out.fpv.len(),
        out.world_guns.len(),
        out.decoded,
        absent.len(),
        &problems[..problems.len().min(8)],
        missing
    ));
    report.push(format!(
        "iw7 guest guns: animations {} kept ({} gaps)",
        out.xanims.len(),
        out.xanims.capture_gaps
    ));
    // The guns' own muzzle flashes, shell ejects and tracers.
    let mut own = if iw7_fx::own_enabled() {
        let owner = ZoneOwner::from_zone_path(&dir.join("global_core_mp.ff"));
        iw7_fx::build(&core, &shared, &mut material_seed, owner, &mut out.failures)
    } else {
        iw7_fx::OwnFx {
            report: vec![
                "iw7 guest guns: own effects off (BO2MP_IW7_OWN_FX=0): the donors' flashes, ejects and tracers"
                    .into(),
            ],
            ..iw7_fx::OwnFx::default()
        }
    };
    report.append(&mut own.report);
    let Out {
        catalog,
        fpv,
        world_guns,
        xanims,
        mut built,
        mut scopes,
        ..
    } = out;
    let captured = catalog.len();
    let mut weapons = catalog.into_build();
    weapons.stamp_namespace(NS);
    for g in &own.guns {
        if let Some(r) = built.iter_mut().find(|r| r.gun == g.gun) {
            r.fx = g.lines.clone();
        }
        let id = weapons
            .resolve_index(&format!("{}_mp", g.gun))
            .ok()
            .flatten()
            .filter(|&i| i != 0);
        if let (Some(id), false) = (id, g.slots.is_empty()) {
            weapons.set_guest_effects(
                id,
                asset_game::GuestEffects::new(
                    g.slots
                        .iter()
                        .map(|(bit, name)| (*bit, NS, name.clone()))
                        .collect(),
                    None,
                ),
            );
        }
    }
    for g in bo2_profile::guest::IW7
        .iter()
        .filter(|g| g.group == "weapon_sniper")
    {
        let class = super::guest_attach::GunClass::from_group(g.group);
        if let (Some(id), Some(scope)) = (
            weapons
                .resolve_index(&format!("{}_mp", g.gun))
                .ok()
                .flatten()
                .filter(|&i| i != 0),
            super::guest_attach::sniper_scope_or(None, class),
        ) {
            weapons.give_guest_scope(id, scope);
            scopes.push(g.gun.to_owned());
        }
    }
    report.push(format!(
        "iw7 guest guns: Black Ops II's sniper scope on {scopes:?}"
    ));
    weapons.resolve_sz_xanim_edges(&xanims);
    weapons.resolve_fpv_mesh_edges(&fpv);
    weapons.resolve_world_model_edges(&world_guns);
    report.push(format!(
        "iw7 guest guns: {captured} weapons captured, {} in the catalog [{:.1} s]",
        weapons.len(),
        started.elapsed().as_secs_f64()
    ));
    Iw7Guests {
        weapons,
        fpv,
        world_guns,
        xanims,
        material_seed,
        report,
        built,
        aliases,
        fx: own.fx,
        fx_models: own.fx_models,
        tracers: own.tracers,
    }
}

/// bo2mp: Infinite Warfare's guns play a Black Ops II gun's effects and,
/// where theirs are not here, its sounds (`DONORS`; a launcher fires its
/// projectile). After Black Ops II's own guns are in the catalog.
pub(crate) fn iw7_guest_sounds(weapons: &mut WeaponBuild, report: &mut Vec<String>) {
    let (mut done, mut missing) = (0usize, Vec::new());
    for &(gun, donor) in DONORS {
        let key =
            asset_core::AssetKey::new(NS, asset_core::AssetKind::Weapon, format!("{gun}_mp")).ok();
        let Some(id) = key
            .and_then(|k| weapons.resolve_key(&k).ok().flatten())
            .filter(|&i| i != 0)
        else {
            continue;
        };
        match weapons
            .resolve_index(donor)
            .ok()
            .flatten()
            .filter(|&i| i != 0)
        {
            Some(from) if weapons.borrow_sounds_and_effects(id, from) => done += 1,
            _ => missing.push(donor),
        }
    }
    if done > 0 || !missing.is_empty() {
        report.push(format!("iw7 guest guns: {done} play Black Ops II's sounds and effects of their kind; donors not found {missing:?}"));
    }
}

/// bo2mp `CodLegacy check-guns`: every Infinite Warfare gun animation decoded by
/// the engine's IW-family decoder.
pub(crate) fn iw7_anim_check(xanims: &XAnimBuild) -> String {
    let (mut ok, mut bad) = (0usize, Vec::new());
    let names: Vec<String> = xanims.names_in(NS).map(str::to_owned).collect();
    for name in &names {
        match xanims
            .get(NS, name)
            .map(|c| xmodel_runtime::AnimClip::from_parts(&c.parts))
        {
            Some(Ok(_)) => ok += 1,
            Some(Err(e)) => bad.push(format!("{name}: {e:?}")),
            None => {}
        }
    }
    format!(
        "iw7 animations: {ok} decode, {} refused{}",
        bad.len(),
        if bad.is_empty() {
            String::new()
        } else {
            format!(
                " ({})",
                bad.iter().take(3).cloned().collect::<Vec<_>>().join("; ")
            )
        }
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roster_names_zones_and_donors() {
        assert_eq!(iw7_weapon_name("m4"), "iw7_m4_mp");
        assert_eq!(iw7_zone_gun("ump45c"), "ump45");
        assert_eq!(iw7_zone_gun("g18c"), "g18");
        assert_eq!(iw7_zone_gun("devastator"), "dev");
        assert_eq!(iw7_zone_gun("m1"), "m1");
        assert_eq!(iw7_zone_gun("m8"), "m8");
        for g in bo2_profile::guest::IW7 {
            assert!(
                DONORS.iter().any(|(gun, _)| *gun == g.gun),
                "{} has no donor",
                g.gun
            );
        }
        assert_eq!(DONORS.len(), bo2_profile::guest::IW7.len());
        let mut s1: Vec<usize> = ANIM_SLOTS.iter().map(|s| s.1).collect();
        s1.sort_unstable();
        s1.dedup();
        assert_eq!(s1.len(), ANIM_SLOTS.len());
        assert!(
            ANIM_SLOTS
                .iter()
                .all(|&(iw7, s1, e)| iw7 < 260 && s1 < S1_ANIM_SLOTS && !e.is_empty())
        );
        let slot = |iw7: usize| ANIM_SLOTS.iter().find(|s| s.0 == iw7).map(|s| s.1);
        assert_eq!(
            (slot(107), slot(171), slot(249)),
            (Some(1), Some(24), Some(128))
        );
        // A package's stems: the words its animations share, most first.
        let names: Vec<String> = [
            "vm_g18_fire",
            "vm_g18_reload",
            "vm_g18_reload_empty",
            "vm_g18_sprint_loop_l",
            "vm_fmg_akimbo_switch_r",
        ]
        .iter()
        .map(|s| (*s).to_owned())
        .collect();
        assert_eq!(anim_stems(&names), ["vm_g18"]);
        // Pictures by what they pack.
        assert_eq!(
            picture_kind("weapon_kb_m4_mag_c_weapon_kb_m4_mag_sg_packed_cs"),
            "colour"
        );
        assert_eq!(
            picture_kind("weapon_kb_m4_mag_n_0596f503_packed_ng"),
            "normal"
        );
        assert_eq!(
            picture_kind("weapon_kb_m4_sight_n_2b0a8a6f_packed_nog"),
            "normal"
        );
        assert_eq!(picture_kind("blank_sg"), "specular");
        assert_eq!(picture_kind("blank_o"), "occlusion");
        assert_eq!(picture_kind("weapon_kb_m4_sight_e"), "emissive");
        // Blend words: one bone; two bones (bone, bone, weight), each with
        // its pad word.
        let (out, bad, over) = s1_blend([1, 1, 0, 0, 0, 0, 0, 0], &[3, 0, 2, 5, 0x8000, 0], 6);
        assert_eq!((out, bad, over), (vec![192, 128, 320, 0x8000], 0, 0));
        let (_, bad, over) = s1_blend(
            [0, 2, 0, 0, 0, 0, 0, 0],
            &[9, 1, 0xffff, 0, 1, 2, 0x10, 0],
            6,
        );
        assert_eq!((bad, over), (1, 0));
        let (_, _, over) = s1_blend([0, 0, 1, 0, 0, 0, 0, 0], &[0, 1, 0x9000, 2, 0x9000, 0], 6);
        assert_eq!(over, 1);
    }

    /// `CODLEGACY_IW7_ZONE=<iw> cargo test -p assets --release iw7_guns --
    /// --ignored --nocapture`: every Infinite Warfare gun built as a match
    /// builds it, its sounds looked up in the zones' banks, its HUD
    /// picture read; a table.
    #[test]
    #[ignore]
    fn iw7_guns() {
        let Some(dir) = asset_audio::iw7_zone_dir() else {
            println!("CODLEGACY_IW7_ZONE not set (or no global_core_mp.ff there)");
            return;
        };
        let g = walk_iw7_guest_guns(
            true,
            MaterialCatalog::default(),
            &asset_transport::LoadProgress::default(),
        );
        for line in &g.report {
            println!("{line}");
        }
        println!("{}", iw7_anim_check(&g.xanims));
        let mut catalog = g.weapons;
        catalog.set_guest_names(true);
        println!(
            "\n| gun | name | view model | view verts / tris / bones | world model | world verts / tris | root | idle bones gun / arms (other) | materials (absent pak / no picture) | anims (by name / dropped) | sounds in banks | captured | picture |"
        );
        println!("|---|---|---|---|---|---|---|---|---|---|---|---|---|");
        let (
            mut mats,
            mut absent,
            mut nopic,
            mut snd_ok,
            mut snd_all,
            mut caught,
            mut pics,
            mut anims,
        ) = (0, 0, 0, 0, 0, 0, 0, 0);
        for r in &g.built {
            let sounds_ok = r.sounds.iter().filter(|s| g.aliases.contains(*s)).count();
            let captured = asset_core::AssetKey::new(
                NS,
                asset_core::AssetKind::Weapon,
                format!("{}_mp", r.gun),
            )
            .ok()
            .and_then(|k| catalog.resolve_key(&k).ok().flatten());
            let script = captured.map_or_else(String::new, |i| catalog.script_name_of(i));
            let pic = r
                .hud_icon
                .as_ref()
                .and_then(|n| fastfile_iw7::ui_image::read_gun_icon(&dir, std::slice::from_ref(n)))
                .map(|(w, h, _)| format!("{w}x{h}"));
            let name = bo2_profile::guest::IW7
                .iter()
                .find(|x| x.gun == r.gun)
                .map_or("", |x| x.name);
            mats += r.materials;
            absent += r.grey_absent_pak;
            nopic += r.grey_missing;
            snd_ok += sounds_ok;
            snd_all += r.sounds.len();
            caught += usize::from(captured.is_some());
            pics += usize::from(pic.is_some());
            anims += r.anims;
            let shape = |s: Option<(usize, usize, usize)>, bones: bool| {
                s.map_or("-".to_owned(), |(v, t, b)| {
                    if bones {
                        format!("{v} / {t} / {b}")
                    } else {
                        format!("{v} / {t}")
                    }
                })
            };
            println!(
                "| {} | {} | {} | {} | {} | {} | {} | {} / {}{} | {} ({} / {}) | {} ({} / {}) | {} / {} | {} | {} |",
                r.gun,
                name,
                r.view_model,
                shape(r.view, true),
                r.world_model,
                shape(r.world, false),
                r.root,
                r.idle_bones.0,
                r.idle_bones.1,
                if r.idle_missing.is_empty() {
                    String::new()
                } else {
                    format!(" ({})", r.idle_missing.len())
                },
                r.materials,
                r.grey_absent_pak,
                r.grey_missing,
                r.anims,
                r.by_name,
                r.dropped,
                sounds_ok,
                r.sounds.len(),
                if script.is_empty() {
                    "NO".to_owned()
                } else {
                    script
                },
                pic.unwrap_or_else(|| "-".to_owned()),
            );
            for e in &r.emissive {
                println!("    {}: emissive {e}", r.gun);
            }
            if !r.emissive.is_empty() {
                println!(
                    "    {}: {} of {} glowing materials drawn with their glow",
                    r.gun,
                    r.glowing,
                    r.emissive.len()
                );
            }
            {
                let mut by_pak: std::collections::BTreeMap<Vec<u16>, usize> = Default::default();
                for (_, _, p) in &r.pictures {
                    *by_pak.entry(p.clone()).or_default() += 1;
                }
                println!("    {}: pictures by pak {:?}", r.gun, by_pak);
            }
            println!(
                "    {}: empty animation slots: {}",
                r.gun,
                r.empty_slots.join(" ")
            );
            if !r.no_picture.is_empty() {
                println!("    {}: without a picture: {:?}", r.gun, r.no_picture);
            }
            if r.bad_blend > 0 || !r.undefined.is_empty() || !r.missing_anims.is_empty() {
                println!(
                    "    {}: {} blend bones out of range; undefined materials {:?}; animations not found {:?}",
                    r.gun, r.bad_blend, r.undefined, r.missing_anims
                );
            }
        }
        // Pictures: native sizes and the paks they stream from.
        {
            let mut sizes: std::collections::BTreeMap<u32, usize> = Default::default();
            let mut paks: std::collections::BTreeSet<u16> = Default::default();
            let mut seen: HashSet<&str> = HashSet::new();
            for r in &g.built {
                for (n, top, p) in &r.pictures {
                    if seen.insert(n) {
                        *sizes.entry(*top).or_default() += 1;
                        paks.extend(p);
                    }
                }
            }
            println!(
                "\ngun pictures by largest stream side: {sizes:?}; imagefile paks needed: {paks:?}"
            );
        }
        if let Some(r) = g.built.first() {
            println!(
                "\n{}'s idle drives, besides the gun's and the arms' bones: {:?}",
                r.gun, r.idle_missing
            );
        }
        // Effects: per gun, each slot -> elements read / modules dropped.
        println!("\nown effects (flash, eject, tracer; BO2MP_IW7_OWN_FX=0 gives the donors'):");
        for r in &g.built {
            for line in &r.fx {
                println!("    {}: {line}", r.gun);
            }
        }
        println!(
            "effects in the catalog: {}, effect models {}, tracers {}",
            g.fx.len(),
            g.fx_models.len(),
            g.tracers.len()
        );
        // Sounds: IW7's own where their clips are here, else the donor's.
        println!(
            "\n| gun | donor | own sounds | donor's for | own reload notes | donor's for notes |"
        );
        println!("|---|---|---|---|---|---|");
        let (mut own, mut donor, mut nown, mut ndonor) = (0, 0, 0, 0);
        let mut needs: Vec<&str> = Vec::new();
        for r in &g.built {
            own += r.sounds_own.len();
            donor += r.sounds_donor.len();
            nown += r.notes_own.len();
            ndonor += r.notes_donor.len();
            needs.extend(
                r.sounds_donor
                    .iter()
                    .chain(&r.notes_donor)
                    .map(String::as_str),
            );
            println!(
                "| {} | {} | {} | {} | {} | {} |",
                r.gun,
                DONORS.iter().find(|d| d.0 == r.gun).map_or("-", |d| d.1),
                r.sounds_own.join(" "),
                r.sounds_donor.join(" "),
                r.notes_own.len(),
                r.notes_donor.len()
            );
        }
        needs.sort_unstable();
        needs.dedup();
        println!(
            "\nsounds: {own} own, {donor} the donor's; reload notes: {nown} own, {ndonor} the donor's; {} aliases whose clips are in absent banks",
            needs.len()
        );
        assert!(own > 0, "some of IW7's own sounds play");
        // The view models: whole, skinned within their skeletons, drawn
        // (a stand-in finish where the picture is absent), and on Black
        // Ops II-make arms held in the hands.
        for r in &g.built {
            assert_eq!(
                (r.garbled, r.weight_over),
                (0, 0),
                "{}: garbage vertices or weights",
                r.gun
            );
            assert!(
                r.colour_not_albedo.is_empty(),
                "{}: colour slot {:?}",
                r.gun,
                r.colour_not_albedo
            );
            assert!(
                r.view_offset_max < 10.0,
                "{}: stance offsets {}",
                r.gun,
                r.view_offset_max
            );
            let [_, (ri, le)] = r
                .arms_fit
                .unwrap_or_else(|| panic!("{}: no arms fit", r.gun));
            assert!(
                ri < 2.5 && !(le > 2.5),
                "{}: hands {ri:.1} / {le:.1} from the grips",
                r.gun
            );
        }
        assert!(
            g.fpv
                .index_by_name(NS, asset_game::IW7_REFERENCE_ARMS)
                .is_some(),
            "IW7's rig is in the first-person catalog"
        );
        // The model checks: where the idle holds the gun in IW7's own
        // hands, its bounds in front of the eye, its pictures and skin.
        println!(
            "\n| gun | idle | grip ri / le per tag | hangs on | BO2-make arms: wrists to grips straight -> retargeted | view box (tag_view: fwd, left, up) | colour not albedo | stand-in | weights > 1 | garbled | stance offsets max |"
        );
        println!("|---|---|---|---|---|---|---|---|---|---|---|");
        for r in &g.built {
            let grip: Vec<String> = r
                .grip
                .iter()
                .map(|(t, a, b)| format!("{t} {a:.1}/{b:.1}"))
                .collect();
            println!(
                "| {} | {} | {} | {} | {} | {} | {} | {} | {} | {} | {:.1} |",
                r.gun,
                r.idle,
                grip.join(", "),
                r.hangs_on,
                r.arms_fit.map_or("-".to_owned(), |[a, b]| format!(
                    "{:.1}/{:.1} -> {:.1}/{:.1}",
                    a.0, a.1, b.0, b.1
                )),
                r.view_box.map_or("-".to_owned(), |(lo, hi)| format!(
                    "{:.1}..{:.1}, {:.1}..{:.1}, {:.1}..{:.1}",
                    lo[0], hi[0], lo[1], hi[1], lo[2], hi[2]
                )),
                r.colour_not_albedo.len(),
                r.stand_in,
                r.weight_over,
                r.garbled,
                r.view_offset_max
            );
            if !r.colour_not_albedo.is_empty() {
                println!("    {}: colour slot holds {:?}", r.gun, r.colour_not_albedo);
            }
        }
        println!(
            "\n{} of {} guns built; {caught} captured; {anims} animations; materials {mats} ({absent} with their picture in an absent pak, {nopic} without one); sounds {snd_ok} of {snd_all} in the banks; {pics} pictures",
            g.built.len(),
            bo2_profile::guest::IW7.len()
        );
        assert_eq!(caught, g.built.len(), "every built gun is in the catalog");
        assert!(
            g.built.len() * 10 >= bo2_profile::guest::IW7.len() * 9,
            "guns built: {}",
            g.built.len()
        );
        assert!(
            g.built.iter().all(|r| r.bad_blend == 0),
            "every blend bone is the model's own"
        );
    }
}
