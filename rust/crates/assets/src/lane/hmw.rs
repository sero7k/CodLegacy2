//! bo2mp: Horizon MW's guns (Modern Warfare 2's guns remastered for Modern
//! Warfare Remastered, `h2_<gun>_mp`) as guest guns in a Black Ops II
//! match, under the H1 namespace (`h2_ak47_mp`).
//!
//! `fastfile_h1`'s exact walk reads Horizon MW's weapon zones
//! (`<mwr>/hmw-mod/zone/hmw_ar1`, `hmw_smg`, `hmw_shotgun`), each complete
//! in itself: weapons, models, materials, pictures (inline, in the zone's
//! own `<zone>.pak`, or in the install's `zone/imagefile<N>.pak`),
//! animations, effects and tracers. The zones are read one at a time, each
//! dropped once its guns are built.
//!
//! H1 is Advanced Warfare's engine. A gun goes through AW's lane
//! (`super::s1`): its `WeaponDef` is laid out as AW's (`hmw_fields`) and
//! captured by `WeaponCatalog::capture_s1` (Black Ops II's gunplay rules,
//! as AW's guns), its animation slots are AW's but for the inspection H1
//! adds, its effects and tracers are AW's records. A composite model's
//! parts are joined on their root bones as AW's are (`join`). Materials and
//! pictures are `H1Builder`'s; arms are Black Ops II's.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use asset_anim::XAnimBuild;
use asset_core::{AssetNamespace, ZoneOwner};
use asset_game::{
    FxCatalog, FxModelCatalog, S1FxElemView, S1FxTrail, S1FxView, S1FxVisual, S1WeaponView,
    TracerCatalog, WeaponBuild, WeaponCatalog,
};
use asset_material::{MaterialCatalog, TS_COLOR_MAP};
use asset_model::{FpvMeshBuild, WorldWeaponBuild};
use fastfile_h1::walk::{AssetId, Capture, FxRec, FxVisualRec, WeaponRec, XModelRec};

use super::h1::H1Builder;
use super::iw6::{fold_blend, le_u16s};
use super::s1::{Pose, fx_technique_unsupported, s1_vertex_to_t6};

const NS: AssetNamespace = AssetNamespace::H1;

/// The largest level kept of a gun's pictures (first person), and of its
/// third-person model's and its effects'.
/// (Horizon MW's guns are authored up to 2048; `BO2MP_HMW_TEXTURE=<size>` or
/// `BO2MP_GUN_TEXTURE` changes it.)
fn first_person_max() -> u32 {
    super::helpers::gun_texture_cap("BO2MP_HMW_TEXTURE", 2048)
}
const WORLD_MAX: u32 = 512;
const FX_MAX: u32 = 512;

/// Advanced Warfare's animation slots (`weapAnimFiles_t`).
const S1_ANIM_SLOTS: usize = 180;

/// The weapon a roster gun is in Horizon MW's zones (`ak47` is
/// `h2_ak47_mp`; the AT4 is `at4_mp`).
pub(crate) fn hmw_weapon_name(gun: &str) -> String {
    if gun == "at4" {
        "at4_mp".to_owned()
    } else {
        format!("h2_{gun}_mp")
    }
}

/// Horizon MW's guns, ready to join the match's catalogs.
pub(crate) struct HmwGuests {
    pub weapons: WeaponBuild,
    pub fpv: FpvMeshBuild,
    pub world_guns: WorldWeaponBuild,
    pub xanims: XAnimBuild,
    pub material_seed: MaterialCatalog,
    pub fx: FxCatalog,
    pub fx_models: FxModelCatalog,
    pub tracers: TracerCatalog,
    pub report: Vec<String>,
    /// Per gun built: what it came to (for checks and reports).
    pub built: Vec<HmwGunReport>,
}

/// One gun as the lane built it.
#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
pub(crate) struct HmwGunReport {
    pub gun: String,
    pub zone: String,
    pub view_model: String,
    /// (vertices, triangles, bones, parts joined), first person.
    pub view: Option<(usize, usize, usize, usize)>,
    pub world: Option<(usize, usize, usize, usize)>,
    /// The view model's root bone.
    pub root: String,
    /// The idle animation's bones: (in the gun model, the arms'), and the
    /// gun bones it drives that the model lacks.
    pub idle_bones: (usize, usize),
    pub idle_missing: Vec<String>,
    /// Materials linked, and those with no colour map (drawn grey).
    pub materials: usize,
    pub grey: usize,
    /// Animations kept, and empty slots left out.
    pub anims: usize,
    pub stub_anims: usize,
    /// The sound aliases the weapon names.
    pub sounds: Vec<String>,
    pub hud_icon: Option<String>,
    /// Its default scope, if it has one.
    pub scope: Option<HmwScopeReport>,
}

/// A gun's default scope as the lane put it on.
#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
pub(crate) struct HmwScopeReport {
    /// The attachment (`cheytacscope`).
    pub attachment: String,
    /// The view model and the gun's tag it is on.
    pub view: Option<(String, String)>,
    /// The world model: on a tag, part of the gun's model, or `-`.
    pub world: String,
}

/// An attachment the game puts on its weapon by itself (a sniper's scope).
fn automatic(a: &fastfile_h1::walk::AttachmentRec, image: &[u8]) -> bool {
    a.record
        .get(image)
        .get(fastfile_h1::layout::WeaponAttachment::automaticAttachment)
        .is_some_and(|&b| b != 0)
}

fn tag_names(tags: &[u32], strings: &[Option<String>]) -> Vec<String> {
    tags.iter()
        .map(|&s| string(strings, s))
        .filter(|s| !s.is_empty())
        .collect()
}

/// An attachment's first model built and added to the first- or
/// third-person models, when its root is one of the gun's bones `on`:
/// its name and that bone.
fn attach_model(
    w: &Walked,
    b: &mut H1Builder<'_>,
    out: &mut Out,
    ids: &[Option<AssetId>],
    on: &[String],
    first_person: bool,
) -> Option<(String, String)> {
    let image = &w.zone.image[..];
    let strings = &w.zone.assets.script_strings;
    let m = ids
        .first()
        .copied()
        .flatten()
        .filter(|id| id.ty == fastfile_h1::AssetType::XModel)
        .and_then(|id| w.cap.xmodels.get(id.index as usize))?;
    let (xref, keys, _) = model_ref(&w.cap, image, strings, m)?;
    let root = xref.bone_names.first()?.clone();
    if !on.iter().any(|b| b.eq_ignore_ascii_case(&root)) {
        return None;
    }
    let max = if first_person {
        first_person_max()
    } else {
        WORLD_MAX
    };
    let linked: HashMap<usize, usize> = keys
        .into_iter()
        .filter_map(|k| Some((k, b.material(k, max)?)))
        .collect();
    let skel = asset_model::capture_model_skel_t6(&xref, |k| linked.get(&k.index).copied())?;
    if first_person {
        out.fpv.insert_in(NS, skel, Some(&b.catalog));
    } else {
        out.world_guns.insert_in(NS, skel, Some(&b.catalog));
    }
    Some((xref.name.clone(), root))
}

impl HmwGuests {
    fn empty(material_seed: MaterialCatalog, report: Vec<String>) -> Self {
        Self {
            weapons: WeaponBuild::default(),
            fpv: FpvMeshBuild::default(),
            world_guns: WorldWeaponBuild::default(),
            xanims: XAnimBuild::default(),
            material_seed,
            fx: FxCatalog::default(),
            fx_models: FxModelCatalog::default(),
            tracers: TracerCatalog::default(),
            report,
            built: Vec::new(),
        }
    }
}

/// Where Horizon MW's guns are, found ahead of the build.
pub(crate) enum HmwPrefetch {
    Off(Vec<String>),
    Ready { dir: PathBuf, zones: Vec<PathBuf> },
}

/// bo2mp: finds Horizon MW's weapon zones (`wanted`: Black Ops II rules
/// with guest guns on).
pub(crate) fn prefetch_hmw_guest_guns(wanted: bool) -> HmwPrefetch {
    if !wanted {
        return HmwPrefetch::Off(vec!["h1 guest guns: off".into()]);
    }
    let Some(dir) = asset_audio::hmw_zone_dir() else {
        return HmwPrefetch::Off(vec![
            "h1 guest guns: Horizon MW's weapon zones not found (Game Folders: mwr, `<mwr>/hmw-mod/zone`)".into(),
        ]);
    };
    let zones = asset_audio::hmw_gun_zones(&dir);
    HmwPrefetch::Ready { dir, zones }
}

/// bo2mp: the match's Horizon MW guest guns, their materials linked into
/// `material_seed`.
pub(crate) fn build_hmw_guest_guns(
    prefetch: HmwPrefetch,
    material_seed: MaterialCatalog,
    progress: &asset_transport::LoadProgress,
) -> HmwGuests {
    let (dir, zones) = match prefetch {
        HmwPrefetch::Off(report) => return HmwGuests::empty(material_seed, report),
        HmwPrefetch::Ready { dir, zones } => (dir, zones),
    };
    let stage = progress.begin_scoped(
        asset_transport::StageId::CommonAssets,
        "h1_guest_weapons",
        None,
    );
    let guests = load_hmw_guests(&dir, &zones, material_seed);
    stage.done();
    guests
}

/// bo2mp: read and built in one go (`CodLegacy check-guns`).
pub(crate) fn walk_hmw_guest_guns(
    wanted: bool,
    material_seed: MaterialCatalog,
    progress: &asset_transport::LoadProgress,
) -> HmwGuests {
    let mut g = build_hmw_guest_guns(prefetch_hmw_guest_guns(wanted), material_seed, progress);
    let lines: Vec<String> = g
        .built
        .iter()
        .map(|r| {
            format!(
                "h1 {} ({}): view {:?}, world {:?}, {} materials ({} grey), {} animations ({} empty left out), idle drives {} gun / {} arms bones{}",
                r.gun,
                r.zone,
                r.view,
                r.world,
                r.materials,
                r.grey,
                r.anims,
                r.stub_anims,
                r.idle_bones.0,
                r.idle_bones.1,
                if r.idle_missing.is_empty() { String::new() } else { format!(", not on the model: {:?}", r.idle_missing) }
            )
        })
        .collect();
    g.report.extend(lines);
    g
}

/// `self` undone.
fn inverse(p: &Pose) -> Pose {
    let [x, y, z, w] = p.q;
    let q = Pose {
        q: [-x, -y, -z, w],
        t: [0.0; 3],
    };
    let t = q.rotate(p.t);
    Pose {
        q: q.q,
        t: t.map(|c| -c),
    }
}

/// A model built from H1 parts (or one whole model) in Black Ops II's
/// terms.
#[derive(Default)]
struct Joined {
    bone_names: Vec<String>,
    num_root_bones: u8,
    parents: Vec<usize>,
    quats: Vec<[i16; 4]>,
    trans: Vec<[f32; 3]>,
    poses: Vec<Pose>,
    part_classification: Vec<u8>,
    lod0: Vec<asset_t6::XSurfaceRef>,
    /// Capture material index per surface.
    materials: Vec<Option<usize>>,
    mins: [f32; 3],
    maxs: [f32; 3],
    parts: usize,
}

fn string(strings: &[Option<String>], index: u32) -> String {
    strings
        .get(index as usize)
        .cloned()
        .flatten()
        .unwrap_or_else(String::new)
}

/// Joins `part` into `j`: its root bone onto `j`'s bone of the same name
/// (none yet: it starts the skeleton), its other bones appended, its
/// first-LOD surfaces moved from where its root is in its own space to
/// where that bone is in `j`'s (a part made at its origin, as AW's, moves
/// by the bone's bind pose; one made in the gun's space stays).
fn join(
    j: &mut Joined,
    cap: &Capture,
    image: &[u8],
    strings: &[Option<String>],
    part: &XModelRec,
) -> bool {
    let names: Vec<String> = part
        .bone_names
        .iter()
        .map(|&s| string(strings, s))
        .collect();
    let roots = usize::from(part.num_root_bones).max(1);
    let first = j.bone_names.is_empty();
    let own_root = part
        .base_mat
        .first()
        .map_or(Pose::IDENTITY, Pose::from_base_mat);
    let attach = if first {
        Pose::IDENTITY
    } else {
        match names
            .first()
            .and_then(|n| j.bone_names.iter().position(|b| b == n))
        {
            Some(i) => j.poses[i].compose(&inverse(&own_root)),
            None => return false,
        }
    };
    let mut map: Vec<usize> = Vec::with_capacity(names.len());
    for (i, name) in names.iter().enumerate() {
        let pose = attach.compose(
            &part
                .base_mat
                .get(i)
                .map_or(Pose::IDENTITY, Pose::from_base_mat),
        );
        let tag = |k: usize| part.tag_angles.get(k).copied().unwrap_or([0, 0, 0, 0x7fff]);
        let pos = |k: usize| part.tag_positions.get(k).copied().unwrap_or([0.0; 3]);
        if first {
            map.push(i);
            j.bone_names.push(name.clone());
            j.poses.push(pose);
            j.part_classification
                .push(part.part_classification.get(i).copied().unwrap_or(0));
            if i >= roots {
                let step = usize::from(part.parent_list.get(i - roots).copied().unwrap_or(0));
                j.parents.push(i.saturating_sub(step));
                j.quats.push(tag(i - roots));
                j.trans.push(pos(i - roots));
            }
            continue;
        }
        if let Some(at) = j.bone_names.iter().position(|b| b == name) {
            map.push(at);
            continue;
        }
        if i < roots {
            map.push(map.first().copied().unwrap_or(0));
            continue;
        }
        let step = usize::from(part.parent_list.get(i - roots).copied().unwrap_or(0));
        let parent = map.get(i.saturating_sub(step)).copied().unwrap_or(0);
        map.push(j.bone_names.len());
        j.bone_names.push(name.clone());
        j.poses.push(pose);
        j.part_classification.push(0);
        j.parents.push(parent);
        j.quats.push(tag(i - roots));
        j.trans.push(pos(i - roots));
    }
    if first {
        j.num_root_bones = roots as u8;
    }
    j.parts += 1;
    let bone = |offset: u16| -> u16 {
        (map.get(usize::from(offset / 64)).copied().unwrap_or(0) * 64) as u16
    };
    let Some(lod) = part.lods.first() else {
        return true;
    };
    let Some(surfs) = lod
        .surfs
        .and_then(|s| cap.model_surfs.get(s.index as usize))
    else {
        return true;
    };
    for (i, s) in surfs.surfs.iter().enumerate() {
        let verts: Vec<u8> = s
            .verts
            .map_or(&[][..], |sp| sp.get(image))
            .chunks_exact(32)
            .flat_map(|v| s1_vertex_to_t6(v, &attach))
            .collect();
        for v in verts.chunks_exact(32) {
            for k in 0..3 {
                let c = f32::from_le_bytes(v[4 * k..4 * k + 4].try_into().unwrap_or([0; 4]));
                j.mins[k] = j.mins[k].min(c);
                j.maxs[k] = j.maxs[k].max(c);
            }
        }
        let indices = le_u16s(s.tris.map_or(&[][..], |sp| sp.get(image)));
        let mut words = le_u16s(s.blend_verts.map_or(&[][..], |sp| sp.get(image)));
        let mut at = 0usize;
        for (bucket, &count) in s.blend_vert_counts.iter().enumerate() {
            for _ in 0..count {
                let per = 1 + 2 * bucket;
                for k in (0..per).filter(|&k| k == 0 || k % 2 == 1) {
                    if let Some(word) = words.get_mut(at + k) {
                        *word = bone(*word);
                    }
                }
                at += per;
            }
        }
        let (blend_counts, blend) = fold_blend(s.blend_vert_counts, &words);
        let material = part
            .materials
            .get(usize::from(lod.surf_index) + i)
            .copied()
            .flatten()
            .map(|id| id.index as usize);
        let mut part_bits = [0u32; 6];
        part_bits.copy_from_slice(&s.part_bits[..6]);
        j.lod0.push(asset_t6::XSurfaceRef {
            verts,
            vert_count: s.vert_count,
            indices,
            vert_lists: s
                .rigid
                .iter()
                .map(|&(b, n, t, c)| (bone(b), n, t, c))
                .collect(),
            flags: 0,
            material: None,
            blend_counts,
            blend,
            part_bits,
        });
        j.materials.push(material);
    }
    true
}

/// A model of the zone (its composite parts joined) in Black Ops II's
/// captured form, the capture materials its surfaces use and how many
/// parts were joined.
fn model_ref(
    cap: &Capture,
    image: &[u8],
    strings: &[Option<String>],
    rec: &XModelRec,
) -> Option<(asset_t6::XModelRef, Vec<usize>, usize)> {
    let parts: Vec<&XModelRec> = if rec.composite.is_empty() {
        vec![rec]
    } else {
        rec.composite
            .iter()
            .flatten()
            .filter_map(|id| cap.xmodels.get(id.index as usize))
            .collect()
    };
    let mut j = Joined {
        mins: [f32::MAX; 3],
        maxs: [f32::MIN; 3],
        ..Joined::default()
    };
    for part in parts {
        join(&mut j, cap, image, strings, part);
    }
    if j.lod0.is_empty() || j.bone_names.is_empty() || j.bone_names.len() > 255 {
        return None;
    }
    let key = |k: usize| asset_t6::AssetKey {
        ty: fastfile_t6::AssetType::Material,
        index: k,
    };
    for (s, m) in j.lod0.iter_mut().zip(&j.materials) {
        s.material = m.map(key);
    }
    let keys: Vec<usize> = j.materials.iter().flatten().copied().collect();
    let n = j.bone_names.len();
    let roots = usize::from(j.num_root_bones);
    let parent_list = j
        .parents
        .iter()
        .enumerate()
        .map(|(k, &p)| (roots + k).saturating_sub(p).min(255) as u8)
        .collect();
    let radius = (0..3)
        .map(|k| j.mins[k].abs().max(j.maxs[k].abs()).powi(2))
        .sum::<f32>()
        .sqrt();
    let parts = j.parts;
    Some((
        asset_t6::XModelRef {
            lighting_origin_offset: [0.0; 3],
            lighting_origin_range: 0.0,
            name: rec.name.trim_start_matches(',').to_owned(),
            numsurfs: j.lod0.len().min(255) as u8,
            materials: j.materials.iter().map(|m| m.map(key)).collect(),
            num_bones: n as u8,
            num_lods: 1,
            mins: j.mins,
            maxs: j.maxs,
            radius,
            lods: vec![(0.0, j.lod0.len() as u16, 0)],
            lod0: j.lod0,
            bone_names: j.bone_names,
            num_root_bones: j.num_root_bones,
            parent_list,
            quats: j.quats,
            trans: j.trans,
            base_mat: j.poses.iter().map(Pose::base_mat).collect(),
            part_classification: j.part_classification,
            bone_info: vec![None; n],
            contents: 0,
            coll_lod: -1,
            coll_surfs: Vec::new(),
            phys_preset: None,
        },
        keys,
        parts,
    ))
}

/// A Horizon MW `WeaponDef` laid out as Advanced Warfare's, and a field
/// offset of it as AW's.
fn s1_def(h1: &[u8]) -> Vec<u8> {
    let mut out = vec![0u8; fastfile_s1::layout::WeaponDef::SIZE];
    for &(h, s, n) in super::hmw_fields::FIELDS {
        if let (Some(src), Some(dst)) = (h1.get(h..h + n), out.get_mut(s..s + n)) {
            dst.copy_from_slice(src);
        }
    }
    out
}

fn s1_offset(h1: usize) -> Option<usize> {
    super::hmw_fields::FIELDS
        .iter()
        .find(|(h, ..)| *h == h1)
        .map(|&(_, s, _)| s)
}

/// H1's animation slots as AW's: the same up to its scripted slot, then H1
/// has an inspection slot AW does not, so the rest move one down.
fn s1_anim_slots(h1: &[String]) -> Vec<String> {
    const H1_INSPECTION: usize = 112;
    (0..S1_ANIM_SLOTS)
        .map(|k| {
            let at = if k < H1_INSPECTION { k } else { k + 1 };
            h1.get(at).cloned().unwrap_or_else(String::new)
        })
        .collect()
}

/// One zone, walked.
struct Walked {
    path: PathBuf,
    zone: fastfile_h1::Zone,
    cap: Capture,
}

fn walk(path: &Path) -> Result<Walked, String> {
    let zone =
        fastfile_h1::Zone::open(path).map_err(|e| format!("open {}: {e}", path.display()))?;
    let (cap, stop) = fastfile_h1::walk::walk_zone(&zone);
    if let Some(s) = stop {
        return Err(format!(
            "{} stopped at asset {} ({:?}): {}",
            path.display(),
            s.asset_index,
            s.asset_type,
            s.error
        ));
    }
    Ok(Walked {
        path: path.to_path_buf(),
        zone,
        cap,
    })
}

/// The builders' shared output, carried from zone to zone.
struct Out {
    catalog: WeaponCatalog,
    fpv: FpvMeshBuild,
    world_guns: WorldWeaponBuild,
    xanims: XAnimBuild,
    fx: FxCatalog,
    fx_models: FxModelCatalog,
    tracers: TracerCatalog,
    built: Vec<HmwGunReport>,
    decoded: usize,
    failures: Vec<String>,
    fx_named: usize,
    fx_missing: Vec<String>,
    /// bo2mp: per gun, its own scope and the attachments it offers.
    attachments: Vec<(String, Option<asset_game::GuestAttachment>, Vec<asset_game::GuestAttachment>)>,
}

/// Arms bones as Black Ops II's and Modern Warfare's arms name them.
fn is_arms_bone(name: &str) -> bool {
    let n = name.to_ascii_lowercase();
    n.starts_with("j_shoulder")
        || n.starts_with("j_elbow")
        || n.starts_with("j_wrist")
        || n.starts_with("j_clavicle")
        || n.starts_with("j_thumb")
        || n.starts_with("j_index")
        || n.starts_with("j_mid")
        || n.starts_with("j_ring")
        || n.starts_with("j_pinky")
        || n.starts_with("j_palm")
        || n.starts_with("j_webbing")
        || n.starts_with("j_sleeve")
        || n.starts_with("j_myhand")
        || n.starts_with("j_wristtwist")
        || n == "tag_view"
        || n == "tag_torso"
        || n == "tag_weapon"
        || n == "tag_weapon_right"
        || n == "tag_weapon_left"
        || n == "tag_cambone"
        || n == "tag_camera"
        || n == "tag_ik_hand_le"
        || n == "tag_ik_hand_ri"
}

/// The guns of one zone, into `out`.
fn build_zone(
    w: &Walked,
    retail: Option<&Path>,
    seed: &mut MaterialCatalog,
    out: &mut Out,
    report: &mut Vec<String>,
) {
    let image = &w.zone.image[..];
    let strings = &w.zone.assets.script_strings;
    let zone_name = w
        .path
        .file_stem()
        .map_or_else(String::new, |s| s.to_string_lossy().into_owned());
    let mut b = H1Builder::new(&w.cap, &w.zone, &w.path, retail, std::mem::take(seed));
    b.ns = NS;
    b.name_prefix = "";
    // bo2mp: Horizon MW's zones hold 1 x 1 red stand-ins for pictures of
    // the retail `common_mp` (the muzzle flash sprites, some gun maps):
    // the retail pictures are read in their place.
    if let Some(retail) = retail
        && let Some(line) = b.use_retail_standins(retail)
    {
        report.push(format!("h1 guest guns: {zone_name}: {line}"));
    }
    let owner = ZoneOwner::from_zone_path(&w.path);
    let guns: Vec<(&str, &WeaponRec)> = bo2_profile::guest::HMW
        .iter()
        .filter_map(|g| {
            let want = hmw_weapon_name(g.gun);
            w.cap
                .weapons
                .iter()
                .find(|r| r.name.eq_ignore_ascii_case(&want))
                .map(|r| (g.gun, r))
        })
        .collect();
    let model_name = |id: Option<AssetId>| {
        id.map(|id| w.cap.name_of(id).trim_start_matches(',').to_owned())
            .filter(|n| !n.is_empty())
    };
    let colour = |catalog: &MaterialCatalog, slot: usize| {
        catalog.materials[slot]
            .textures
            .iter()
            .any(|t| t.semantic == TS_COLOR_MAP && t.image.is_some())
    };
    for (gun, rec) in &guns {
        let mut r = HmwGunReport {
            gun: (*gun).to_owned(),
            zone: zone_name.clone(),
            ..HmwGunReport::default()
        };
        let mut linked_slots: HashSet<usize> = HashSet::new();
        let mut view_bones: Vec<String> = Vec::new();
        let mut world_bones: Vec<String> = Vec::new();
        // bo2mp: a launcher's rocket on its first-person model (the RPG-7's
        // `projectile_rpg7`) too: the view's mount plan needs it, and the
        // gun was refused without it ("unlinked selected models").
        let rocket = rec
            .models
            .iter()
            .find(|(o, _)| *o == fastfile_h1::layout::WeaponDef::rocketModel)
            .map(|(_, id)| *id);
        for (id, max, first_person, is_rocket) in [
            (
                rec.gun_models.first().copied().flatten(),
                first_person_max(),
                true,
                false,
            ),
            (
                rec.world_models.first().copied().flatten(),
                WORLD_MAX,
                false,
                false,
            ),
            (rocket, first_person_max(), true, true),
        ] {
            let Some(id) = id else { continue };
            let Some(m) = w.cap.xmodels.get(id.index as usize) else {
                continue;
            };
            let Some((xref, keys, parts)) = model_ref(&w.cap, image, strings, m) else {
                out.failures
                    .push(format!("{}: {} (no surfaces)", gun, m.name));
                continue;
            };
            let verts: usize = xref.lod0.iter().map(|s| usize::from(s.vert_count)).sum();
            let tris: usize = xref.lod0.iter().map(|s| s.indices.len() / 3).sum();
            let shape = (verts, tris, xref.bone_names.len(), parts);
            if is_rocket {
                // (Its own first-person model; the gun's view stays as is.)
            } else if first_person {
                r.view_model = xref.name.clone();
                r.root = xref.bone_names.first().cloned().unwrap_or_else(String::new);
                view_bones = xref.bone_names.clone();
                r.view = Some(shape);
            } else {
                world_bones = xref.bone_names.clone();
                r.world = Some(shape);
            }
            let linked: HashMap<usize, usize> = keys
                .into_iter()
                .filter_map(|k| Some((k, b.material(k, max)?)))
                .collect();
            linked_slots.extend(linked.values().copied());
            match asset_model::capture_model_skel_t6(&xref, |k| linked.get(&k.index).copied()) {
                Some(skel) if first_person => out.fpv.insert_in(NS, skel, Some(&b.catalog)),
                Some(skel) => out.world_guns.insert_in(NS, skel, Some(&b.catalog)),
                None => out
                    .failures
                    .push(format!("{}: {} (unreadable)", gun, xref.name)),
            }
        }
        r.materials = linked_slots.len();
        r.grey = linked_slots
            .iter()
            .filter(|&&s| !colour(&b.catalog, s))
            .count();

        // Animations: the normal set, else the right-handed one; an empty
        // stub (no frames, no bones) leaves its slot empty.
        let anims = if rec.anims.iter().any(Option::is_some) {
            &rec.anims
        } else {
            &rec.anims_right
        };
        let mut slots: Vec<String> = Vec::with_capacity(anims.len());
        for (k, a) in anims.iter().enumerate() {
            let Some(xa) = a.and_then(|id| w.cap.xanims.get(id.index as usize)) else {
                slots.push(String::new());
                continue;
            };
            let name = xa.name.trim_start_matches(',').to_ascii_lowercase();
            if xa.numframes == 0 && xa.names.is_empty() {
                r.stub_anims += 1;
                slots.push(String::new());
                continue;
            }
            let anim = b.anim_ref(xa);
            if k == 1 {
                let gun_bones: HashSet<&str> = view_bones.iter().map(String::as_str).collect();
                let (mut on_gun, mut on_arms) = (0, 0);
                for bn in &anim.names {
                    if gun_bones.contains(bn.as_str()) {
                        on_gun += 1;
                    } else if is_arms_bone(bn) {
                        on_arms += 1;
                    } else if !h1_arms_only(bn) {
                        r.idle_missing.push(bn.clone());
                    }
                }
                r.idle_bones = (on_gun, on_arms);
            }
            if out.xanims.insert_t6_as(NS, &anim) {
                r.anims += 1;
            }
            slots.push(name);
        }

        // Effects and tracers the gun names.
        let fx_by_name: HashMap<&str, usize> = w
            .cap
            .fx
            .iter()
            .enumerate()
            .filter(|(_, f)| !f.name.starts_with(','))
            .map(|(i, f)| (f.name.as_str(), i))
            .collect();
        let mut todo: Vec<String> = rec.fx.iter().map(|(_, id)| b.name(*id)).collect();
        let mut seen = HashSet::new();
        while let Some(name) = todo.pop() {
            if name.is_empty() || !seen.insert(name.clone()) {
                continue;
            }
            out.fx_named += 1;
            let Some(&i) = fx_by_name.get(name.as_str()) else {
                out.fx_missing.push(name);
                continue;
            };
            let f = &w.cap.fx[i];
            for el in &f.elems {
                todo.extend(
                    el.effects
                        .iter()
                        .map(|n| n.trim_start_matches(',').to_owned()),
                );
            }
            let view = fx_view(&mut b, f, &mut out.fx_models);
            out.fx.set_capture_zone(owner);
            out.fx.capture_s1_in(NS, &view);
        }
        let tracer = rec.tracer.map(|id| b.name(id)).filter(|n| !n.is_empty());
        if let Some(t) = tracer
            .as_deref()
            .and_then(|n| w.cap.tracers.iter().find(|t| t.name == n))
        {
            let material = t
                .material
                .and_then(|id| {
                    let i = id.index as usize;
                    let tech = technique(&w.cap, i);
                    (!fx_technique_unsupported(&tech))
                        .then(|| b.material(i, FX_MAX))
                        .flatten()
                })
                .map(|slot| b.catalog.materials[slot].name.as_str().to_owned())
                .unwrap_or_else(String::new);
            out.tracers.set_capture_zone(owner);
            out.tracers
                .capture_s1_in(NS, &t.name, &material, t.record.get(image));
        }

        // The weapon, as AW's capture reads it.
        let def = s1_def(rec.def.get(image));
        let remap = |list: &[(usize, AssetId)]| -> Vec<(usize, String)> {
            list.iter()
                .filter_map(|(o, id)| Some((s1_offset(*o)?, b.name(*id))))
                .collect()
        };
        let sounds: Vec<(usize, String)> = rec
            .sounds
            .iter()
            .filter_map(|(o, n)| Some((s1_offset(*o)?, n.clone())))
            .collect();
        r.sounds = sounds
            .iter()
            .map(|(_, n)| n.clone())
            .filter(|n| !n.is_empty() && n != "null")
            .collect();
        r.sounds.sort();
        r.sounds.dedup();
        r.hud_icon = rec
            .materials
            .iter()
            .find(|(o, _)| *o == fastfile_h1::layout::WeaponDef::hudIcon)
            .map(|(_, id)| b.name(*id));
        let view = S1WeaponView {
            // (Named by the roster: `h2_ak47_mp` is the H1 namespace's
            // `ak47`, which a Black Ops II match gives as `h2_ak47_mp`.)
            name: format!("{gun}_mp"),
            display_name: rec.display_name.clone(),
            def,
            gun_model: rec
                .gun_models
                .first()
                .copied()
                .flatten()
                .and_then(|id| model_name(Some(id))),
            world_model: rec
                .world_models
                .first()
                .copied()
                .flatten()
                .and_then(|id| model_name(Some(id))),
            xanims: s1_anim_slots(&slots),
            hide_tags: rec
                .hide_tags
                .iter()
                .map(|&s| string(strings, s))
                .filter(|s| !s.is_empty())
                .collect(),
            alt_weapon_name: String::new(),
            sounds,
            fx: remap(&rec.fx),
            models: remap(&rec.models),
            materials: remap(&rec.materials),
            notetrack_sounds: rec
                .notetrack_sound_keys
                .iter()
                .zip(&rec.notetrack_sound_values)
                .map(|(&k, &v)| (string(strings, k), string(strings, v)))
                .filter(|(k, v)| !k.is_empty() && !v.is_empty())
                .collect(),
            location_damage: rec.location_damage.map(|s| {
                s.get(image)
                    .chunks_exact(4)
                    .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
                    .collect()
            }),
            tracer,
            impact_row: Vec::new(),
        };
        out.catalog.capture_s1(&view);
        let class = bo2_profile::guest::HMW
            .iter()
            .find(|g| g.gun == *gun)
            .map_or(super::guest_attach::GunClass::Rifle, |g| super::guest_attach::GunClass::from_group(g.group));
        let world_parts: Vec<String> = rec
            .world_models
            .first()
            .copied()
            .flatten()
            .and_then(|id| w.cap.xmodels.get(id.index as usize))
            .map(|m| {
                m.composite
                    .iter()
                    .flatten()
                    .map(|id| w.cap.name_of(*id).trim_start_matches(',').to_owned())
                    .collect()
            })
            .unwrap_or_default();
        // Its default scope: the zone's automatic attachment named after
        // it (`cheytacscope`), which the gun's own list leaves out (the
        // game adds it to the weapon by name). One its world model already
        // has as a part (`attach_h2_cheytac_scope_npc`) is not put on twice.
        let mut scope = None;
        if let Some(a) = w
            .cap
            .attachments
            .iter()
            .filter(|a| automatic(a, image) && super::guest_attach::is_own_scope(&a.name, gun))
            .find(|a| a.view_models.iter().any(Option::is_some))
        {
            let view = attach_model(w, &mut b, out, &a.view_models, &view_bones, true);
            let world_name = a
                .world_models
                .first()
                .copied()
                .flatten()
                .map(|id| w.cap.name_of(id).trim_start_matches(',').to_owned());
            let in_model = world_name.as_ref().is_some_and(|n| world_parts.contains(n));
            let world = if in_model {
                None
            } else {
                attach_model(w, &mut b, out, &a.world_models, &world_bones, false)
            };
            r.scope = Some(HmwScopeReport {
                attachment: a.name.clone(),
                view: view.clone(),
                world: match (&world, in_model) {
                    (Some((m, tag)), _) => format!("{m} on {tag}"),
                    (None, true) => format!("{} (part of the model)", world_name.unwrap_or_default()),
                    (None, false) => "-".to_owned(),
                },
            });
            let mut own = super::guest_attach::own_scope(view.map(|v| v.0), world.map(|v| v.0));
            own.hide_tags = tag_names(&a.hide_tags, strings);
            scope = Some(own);
        }
        // bo2mp: its attachments in this zone: their models on the gun's
        // bones named as their roots, the animations and fire sounds the
        // gun gives them (an override names an attachment by its place in
        // the gun's list, from 1).
        let mut offered: Vec<asset_game::GuestAttachment> = Vec::new();
        for (k, id) in rec.attachments.iter().enumerate() {
            let Some(a) = id
                .filter(|id| id.ty == fastfile_h1::AssetType::Attachment)
                .and_then(|id| w.cap.attachments.get(id.index as usize))
            else {
                continue;
            };
            if automatic(a, image) {
                continue;
            }
            // (A sight of the gun's own that is not its default, the AUG's
            // scope or the F2000's, is the sight its display name gives.)
            let kind = if super::guest_attach::is_own_scope(&a.name, gun) {
                let by = if a.display_name.to_ascii_lowercase().contains("acog") { "acog" } else { "reflex" };
                super::guest_attach::iw_kind(by, class)
            } else {
                super::guest_attach::iw_kind(&a.name, class)
            };
            let Some(kind) = kind.filter(|k| !offered.iter().any(|o| o.word == k.word)) else {
                continue;
            };
            let view_model = attach_model(w, &mut b, out, &a.view_models, &view_bones, true).map(|v| v.0);
            let world_model = attach_model(w, &mut b, out, &a.world_models, &world_bones, false).map(|v| v.0);
            if kind.sight && view_model.is_none() {
                continue;
            }
            let place = k as u32 + 1;
            let mut anims = Vec::new();
            for o in rec.anim_overrides.iter().filter(|o| o.attachment1 == place && o.attachment2 == 0) {
                // (H1's slots are AW's but for its inspection, at 112.)
                let slot = o.anim_index as usize;
                let s1 = match slot {
                    112 => continue,
                    s if s > 112 => s - 1,
                    s => s,
                };
                let (Some(to), Some(xa)) = (
                    asset_game::s1_anim_slot(s1),
                    o.override_anim.and_then(|id| w.cap.xanims.get(id.index as usize)),
                ) else {
                    continue;
                };
                if xa.numframes == 0 && xa.names.is_empty() {
                    continue;
                }
                out.xanims.insert_t6_as(NS, &b.anim_ref(xa));
                anims.push((to, xa.name.trim_start_matches(',').to_ascii_lowercase()));
            }
            let sounds = rec
                .sound_overrides
                .iter()
                .filter(|(a1, a2, ..)| *a1 == place && *a2 == 0)
                .filter_map(|(_, _, alias, _)| {
                    let alias = alias.as_deref()?;
                    Some((super::guest_attach::fire_slot(alias)?, alias.to_owned()))
                })
                .collect();
            let mut given = super::guest_attach::guest_attachment(kind, class, view_model, world_model, anims, sounds);
            given.hide_tags = tag_names(&a.hide_tags, strings);
            offered.push(given);
        }
        out.attachments.push((
            (*gun).to_owned(),
            super::guest_attach::sniper_scope_or(scope, class),
            offered,
        ));
        out.built.push(r);
    }
    out.decoded += b.decoded;
    out.failures
        .extend(b.image_failures.iter().take(8).cloned());
    report.push(format!(
        "h1 guest guns: {zone_name}: {} guns, {} materials linked, {} pictures decoded, {} failed",
        guns.len(),
        b.linked(),
        b.decoded,
        b.image_failures.len()
    ));
    *seed = std::mem::take(&mut b.catalog);
}

fn technique(cap: &Capture, material: usize) -> String {
    cap.materials
        .get(material)
        .and_then(|m| m.techset)
        .map(|t| cap.name_of(t).trim_start_matches(',').to_owned())
        .unwrap_or_else(String::new)
}

/// One walked effect with its names resolved (its materials and models
/// brought over as they are met: a material whose technique Black Ops II
/// cannot draw, or that has no colour map, is left out).
pub(super) fn fx_view(b: &mut H1Builder<'_>, f: &FxRec, models: &mut FxModelCatalog) -> S1FxView {
    let cap = b.cap;
    let image = b.image;
    let bytes =
        |s: Option<fastfile_h1::walk::Span>| s.map_or(Vec::new(), |s| s.get(image).to_vec());
    let material = |b: &mut H1Builder<'_>, id: AssetId| -> Option<String> {
        let i = id.index as usize;
        if fx_technique_unsupported(&technique(cap, i)) {
            return None;
        }
        let slot = b.material(i, FX_MAX)?;
        let m = &b.catalog.materials[slot];
        m.textures
            .iter()
            .any(|t| t.semantic == TS_COLOR_MAP && t.image.is_some())
            .then(|| m.name.as_str().to_owned())
    };
    let mut elems = Vec::with_capacity(f.elems.len());
    for el in &f.elems {
        let mut visuals = Vec::with_capacity(el.visuals.len());
        for v in &el.visuals {
            visuals.push(match v {
                FxVisualRec::None => S1FxVisual::None,
                FxVisualRec::Other => S1FxVisual::Other,
                FxVisualRec::Material(id) => {
                    material(b, *id).map_or(S1FxVisual::None, S1FxVisual::Material)
                }
                FxVisualRec::Mark(three) => S1FxVisual::Mark([
                    three[0].and_then(|id| material(b, id)),
                    three[1].and_then(|id| material(b, id)),
                ]),
                FxVisualRec::Model(id) => match cap.xmodels.get(id.index as usize) {
                    Some(m) => fx_model(b, m, models).map_or(S1FxVisual::None, S1FxVisual::Model),
                    None => S1FxVisual::None,
                },
                FxVisualRec::Effect(name) => {
                    S1FxVisual::Effect(name.trim_start_matches(',').to_owned())
                }
                FxVisualRec::Sound(name) => S1FxVisual::Sound(name.clone()),
            });
        }
        let child = |n: &Option<String>| n.as_ref().map(|n| n.trim_start_matches(',').to_owned());
        elems.push(S1FxElemView {
            record: el.record.get(image).to_vec(),
            vel_samples: bytes(el.vel_samples),
            vis_samples: bytes(el.vis_samples),
            visuals,
            effect_on_impact: child(&el.effect_on_impact),
            effect_on_death: child(&el.effect_on_death),
            effect_emitted: child(&el.effect_emitted),
            trail: el.trail.map(|t| S1FxTrail {
                record: t.record.get(image).to_vec(),
                verts: bytes(t.verts),
                inds: bytes(t.inds),
            }),
            extended: bytes(el.extended),
            iw6: None,
        });
    }
    S1FxView {
        name: f.name.clone(),
        header: f.record.get(image).to_vec(),
        elems,
        h1: true,
    }
}

/// An effect's model into `models` (its name), unless it draws only with
/// techniques left out.
fn fx_model(b: &mut H1Builder<'_>, m: &XModelRec, models: &mut FxModelCatalog) -> Option<String> {
    let (xref, keys, _) = model_ref(b.cap, b.image, b.strings, m)?;
    let keys: Vec<usize> = keys
        .into_iter()
        .filter(|&k| !fx_technique_unsupported(&technique(b.cap, k)))
        .collect();
    if keys.is_empty() {
        return None;
    }
    let linked: HashMap<usize, usize> = keys
        .into_iter()
        .filter_map(|k| Some((k, b.material(k, FX_MAX)?)))
        .collect();
    let skel = asset_model::capture_model_skel_t6(&xref, |k| linked.get(&k.index).copied())?;
    let name = skel.name.clone();
    models.insert_skel(skel, &b.catalog);
    Some(name)
}

/// bo2mp: Modern Warfare Remastered's retail zones that hold the effects
/// (`<mwr>/zone`), later ones overriding earlier ones.
const STANDIN_ZONES: [&str; 3] = ["common_core_mp", "common_mp", "patch_common_mp"];

/// bo2mp: the retail zone folder of Modern Warfare Remastered, when the
/// effect zones are in it.
pub(crate) fn standin_zone_dir() -> Option<PathBuf> {
    let set = asset_transport::game_paths::get("mwr")?;
    [set.join("zone"), set.clone()]
        .into_iter()
        .find(|d| d.join("common_mp.ff").is_file())
}

/// bo2mp: Modern Warfare Remastered's effects as stand-ins
/// (`super::fx_standin`).
pub(crate) struct StandinFx {
    pub fx: FxCatalog,
    pub fx_models: FxModelCatalog,
    pub material_seed: MaterialCatalog,
    pub report: Vec<String>,
}

/// A zone walked as far as it goes (a retail zone cut short still gives the
/// effects before the cut).
fn walk_lenient(path: &Path) -> Result<Walked, String> {
    let zone = fastfile_h1::Zone::open(path).map_err(|e| format!("open {}: {e}", path.display()))?;
    // A zone cut short leaves the walker's depth check (debug builds) failing:
    // that is a skipped zone here, not a crash.
    let walked = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| fastfile_h1::walk::walk_zone(&zone)));
    let Ok((cap, _)) = walked else {
        return Err(format!("walk {}: stopped inside an asset", path.display()));
    };
    Ok(Walked { path: path.to_path_buf(), zone, cap })
}

/// bo2mp: the effects `choose` picks from the names of the retail zones'
/// effects (and every effect those run) into catalogs under the H1
/// namespace, their sprites' materials linked into `material_seed`. Zones
/// that hold none of the picks are not built.
pub(crate) fn standin_effects(
    dir: &Path,
    material_seed: MaterialCatalog,
    choose: Box<dyn FnOnce(&[String]) -> Vec<String> + '_>,
) -> StandinFx {
    let started = std::time::Instant::now();
    let mut report = Vec::new();
    let zones: Vec<PathBuf> = STANDIN_ZONES
        .iter()
        .map(|z| dir.join(format!("{z}.ff")))
        .filter(|p| p.is_file())
        .collect();
    // The names first (a zone walked, its effects' names and the names they
    // run noted, and let go): the zone that defines an effect last owns it.
    let mut owner_of: std::collections::BTreeMap<String, usize> = std::collections::BTreeMap::new();
    let mut runs: HashMap<String, Vec<String>> = HashMap::new();
    for (k, path) in zones.iter().enumerate() {
        match walk_lenient(path) {
            Ok(w) => {
                for f in w.cap.fx.iter().filter(|f| !f.name.starts_with(',')) {
                    owner_of.insert(f.name.clone(), k);
                    let children: Vec<String> = f
                        .elems
                        .iter()
                        .flat_map(|el| el.effects.iter().map(|n| n.trim_start_matches(',').to_owned()))
                        .filter(|n| !n.is_empty())
                        .collect();
                    runs.insert(f.name.clone(), children);
                }
            }
            Err(e) => report.push(format!("h1 stand-in effects: {e}")),
        }
    }
    let names: Vec<String> = owner_of.keys().cloned().collect();
    // Every effect wanted and every effect those run.
    let mut closure: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    let mut todo = choose(&names);
    let mut missing = Vec::new();
    while let Some(n) = todo.pop() {
        if !owner_of.contains_key(&n) {
            missing.push(n);
            continue;
        }
        if closure.insert(n.clone()) {
            todo.extend(runs.get(&n).cloned().unwrap_or_default());
        }
    }
    let mut fx = FxCatalog::default();
    let mut fx_models = FxModelCatalog::default();
    fx.set_capture_ns(NS);
    fx_models.set_capture_ns(NS);
    let mut seed = material_seed;
    for (k, path) in zones.iter().enumerate() {
        let mine: Vec<&String> = closure.iter().filter(|n| owner_of.get(*n) == Some(&k)).collect();
        if mine.is_empty() {
            continue;
        }
        let w = match walk_lenient(path) {
            Ok(w) => w,
            Err(e) => {
                report.push(format!("h1 stand-in effects: {e}"));
                continue;
            }
        };
        let mut b = H1Builder::new(&w.cap, &w.zone, &w.path, Some(dir), std::mem::take(&mut seed));
        b.ns = NS;
        b.name_prefix = "";
        if let Some(line) = b.use_retail_standins(dir) {
            report.push(format!("h1 stand-in effects: {line}"));
        }
        let owner = ZoneOwner::from_zone_path(&w.path);
        let by_name: HashMap<&str, usize> =
            w.cap.fx.iter().enumerate().filter(|(_, f)| !f.name.starts_with(',')).map(|(i, f)| (f.name.as_str(), i)).collect();
        for name in mine {
            let Some(&i) = by_name.get(name.as_str()) else { continue };
            let view = fx_view(&mut b, &w.cap.fx[i], &mut fx_models);
            fx.set_capture_zone(owner);
            fx.capture_s1_in(NS, &view);
        }
        report.push(format!(
            "h1 stand-in effects: {}: {} pictures decoded, {} materials linked, {} pictures failed{}",
            w.path.file_name().and_then(|n| n.to_str()).unwrap_or("?"),
            b.decoded,
            b.linked(),
            b.image_failures.len(),
            b.image_failures.first().map_or_else(String::new, |f| format!(" (first: {f})"))
        ));
        seed = b.catalog;
    }
    report.push(format!(
        "h1 stand-in effects: {} of {} wanted (with those they run) captured, {} effect models [{:.1} s]",
        fx.len(),
        closure.len(),
        fx_models.len(),
        started.elapsed().as_secs_f64()
    ));
    if !missing.is_empty() {
        missing.sort();
        report.push(format!("h1 stand-in effects: not in the zones: {:?}", &missing[..missing.len().min(8)]));
    }
    StandinFx { fx, fx_models, material_seed: seed, report }
}

/// Horizon MW's guns from its weapon zones in `dir`, their materials
/// linked into `material_seed`. A later start with the same zones reads
/// the finished guns back (`finished_gun_cache`).
fn load_hmw_guests(dir: &Path, zones: &[PathBuf], material_seed: MaterialCatalog) -> HmwGuests {
    let names: Vec<String> = zones
        .iter()
        .map(|path| path.file_stem().map_or_else(String::new, |s| s.to_string_lossy().into_owned()))
        .collect();
    let caps = [("fp", first_person_max()), ("world", WORLD_MAX), ("fx", FX_MAX)];
    let code = [
        include_str!("finished_gun_cache.rs"),
        include_str!("hmw.rs"),
        include_str!("h1.rs"),
        include_str!("hmw_fields.rs"),
    ];
    let src = super::finished_gun_cache::Sources {
        lane: "h1",
        name: "guns",
        files: zones,
        caps: &caps,
        code: &code,
        zones: &names,
        note: "",
        missing_ok: false,
    };
    super::finished_gun_cache::load_or_build(
        &src,
        material_seed,
        |seed| load_hmw_guests_now(dir, zones, seed),
        pack_hmw,
        unpack_hmw,
    )
}

fn pack_hmw(guests: &HmwGuests, at: super::finished_gun_cache::MatAt) -> Result<Vec<u8>, String> {
    let extra = serde_json::to_vec(&guests.built).map_err(|e| e.to_string())?;
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

fn unpack_hmw(bytes: &[u8], seed: MaterialCatalog) -> Result<HmwGuests, MaterialCatalog> {
    match super::finished_gun_cache::unpack_parts(bytes, seed) {
        Ok(rest) => Ok(HmwGuests {
            weapons: rest.weapons,
            fpv: rest.fpv,
            world_guns: rest.world,
            xanims: rest.xanims,
            material_seed: rest.materials,
            fx: rest.fx,
            fx_models: rest.fx_models,
            tracers: rest.tracers,
            report: rest.reports,
            built: serde_json::from_slice(&rest.extra).unwrap_or_default(),
        }),
        Err(seed) => Err(seed),
    }
}

fn load_hmw_guests_now(dir: &Path, zones: &[PathBuf], mut material_seed: MaterialCatalog) -> HmwGuests {
    let started = std::time::Instant::now();
    // (The retail image paks are in the install's `zone/`, beside
    // `hmw-mod/`.)
    let retail = dir
        .parent()
        .and_then(Path::parent)
        .map(|p| p.join("zone"))
        .filter(|d| d.is_dir());
    let mut report = vec![format!(
        "h1 guest guns: Horizon MW's weapon zones in {} ({} of them), retail paks {}",
        dir.display(),
        zones.len(),
        retail
            .as_deref()
            .map_or_else(|| "not found".to_owned(), |d| d.display().to_string())
    )];
    let mut out = Out {
        catalog: WeaponCatalog::default(),
        fpv: FpvMeshBuild::default(),
        world_guns: WorldWeaponBuild::default(),
        xanims: XAnimBuild::default(),
        fx: FxCatalog::default(),
        fx_models: FxModelCatalog::default(),
        tracers: TracerCatalog::default(),
        built: Vec::new(),
        decoded: 0,
        failures: Vec::new(),
        fx_named: 0,
        fx_missing: Vec::new(),
        attachments: Vec::new(),
    };
    out.catalog.set_capture_ns(NS);
    out.fpv.set_capture_ns(NS);
    out.world_guns.set_capture_ns(NS);
    out.xanims.set_capture_ns(NS);
    out.fx.set_capture_ns(NS);
    out.fx_models.set_capture_ns(NS);
    out.tracers.set_capture_ns(NS);
    // bo2mp: the zones' 1 x 1 stand-ins are looked up in the retail and guest
    // zones once for all of them. Zone by zone, each lookup that does not
    // find everything reads every one of those zones again (20 s a zone,
    // 66 s a match start with the three weapon zones).
    if let Some(retail) = retail.as_deref() {
        // (The zones are walked side by side, one thread each: the walks
        // share nothing, and each hands back only its stand-in pictures.)
        let walked_at = std::time::Instant::now();
        let mut standins = Vec::new();
        std::thread::scope(|s| {
            let jobs: Vec<_> = zones
                .iter()
                .map(|path| {
                    s.spawn(move || {
                        walk(path)
                            .map(|w| w.cap.images.iter().filter(|r| fastfile_h1::shared::is_standin(r)).cloned().collect::<Vec<_>>())
                            .unwrap_or_default()
                    })
                })
                .collect();
            for job in jobs {
                if let Ok(found) = job.join() {
                    standins.extend(found);
                }
            }
        });
        report.push(format!(
            "h1 guest guns: {} zones walked for stand-ins side by side in {:.1} s",
            zones.len(),
            walked_at.elapsed().as_secs_f64()
        ));
        if let Some(line) = fastfile_h1::shared::load_standins(retail, &standins) {
            report.push(format!("h1 guest guns: all zones: {line}"));
        }
    }
    for path in zones {
        let t = std::time::Instant::now();
        match walk(path) {
            Ok(w) => {
                build_zone(
                    &w,
                    retail.as_deref(),
                    &mut material_seed,
                    &mut out,
                    &mut report,
                );
                if let Some(last) = report.last_mut() {
                    last.push_str(&format!(" [{:.1} s]", t.elapsed().as_secs_f64()));
                }
            }
            Err(e) => report.push(format!("h1 guest guns: {e}")),
        }
    }
    let missing: Vec<&str> = bo2_profile::guest::HMW
        .iter()
        .map(|g| g.gun)
        .filter(|g| !out.built.iter().any(|r| r.gun == *g))
        .collect();
    report.push(format!(
        "h1 guest guns: models: {} first person, {} third person; {} pictures decoded; problems {:?}; not found {:?}",
        out.fpv.len(),
        out.world_guns.len(),
        out.decoded,
        &out.failures[..out.failures.len().min(6)],
        missing
    ));
    report.push(format!(
        "h1 guest guns: animations {} kept ({} gaps); effects {} named, {} captured, {} not found; {} tracers; {} effect models",
        out.xanims.len(),
        out.xanims.capture_gaps,
        out.fx_named,
        out.fx.len(),
        out.fx_missing.len(),
        out.tracers.len(),
        out.fx_models.len()
    ));
    let Out {
        catalog,
        fpv,
        world_guns,
        xanims,
        fx,
        fx_models,
        tracers,
        built,
        attachments,
        ..
    } = out;
    let captured = catalog.len();
    let mut weapons = catalog.into_build();
    weapons.stamp_namespace(NS);
    let mut scoped = Vec::new();
    for (gun, scope, offered) in attachments {
        let Some(id) = weapons.resolve_index(&gun).ok().flatten().filter(|&i| i != 0) else {
            continue;
        };
        if let Some(scope) = scope {
            scoped.push(gun.clone());
            weapons.give_guest_scope(id, scope);
        }
        report.push(format!("h1 guest guns: {gun} attachments: {}", super::guest_attach::words(&offered)));
        weapons.offer_guest_attachments(id, offered);
    }
    report.push(format!("h1 guest guns: own scopes on {scoped:?}"));
    weapons.resolve_sz_xanim_edges(&xanims);
    weapons.resolve_fpv_mesh_edges(&fpv);
    weapons.resolve_world_model_edges(&world_guns);
    report.push(format!(
        "h1 guest guns: {captured} weapons captured, {} in the catalog [{:.1} s]",
        weapons.len(),
        started.elapsed().as_secs_f64()
    ));
    HmwGuests {
        weapons,
        fpv,
        world_guns,
        xanims,
        material_seed,
        fx,
        fx_models,
        tracers,
        report,
        built,
    }
}

/// bo2mp `CodLegacy check-guns`: every Horizon MW gun animation decoded by the
/// engine's IW-family decoder.
pub(crate) fn hmw_anim_check(xanims: &XAnimBuild) -> String {
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
        "h1 animations: {ok} decode, {} refused{}",
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

/// The bones Modern Warfare Remastered's arms have and Black Ops II's do
/// not (its sleeves, its gas mask, the knife it holds): an animation that
/// drives them drives nothing on a Black Ops II player.
fn h1_arms_only(bone: &str) -> bool {
    bone.starts_with("j_sleave_reshape")
        || bone.starts_with("tag_gasmask")
        || bone.starts_with("tag_knife_attach")
        || bone == "tag_player"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn horizon_mw_names_and_slots() {
        assert_eq!(hmw_weapon_name("ak47"), "h2_ak47_mp");
        assert_eq!(hmw_weapon_name("at4"), "at4_mp");
        let h1: Vec<String> = (0..190).map(|k| format!("a{k}")).collect();
        let s1 = s1_anim_slots(&h1);
        assert_eq!(s1.len(), S1_ANIM_SLOTS);
        // (Reload and raise keep their slots; H1's ADS up, 129, is AW's 128.)
        assert_eq!(s1[24], "a24");
        assert_eq!(s1[111], "a111");
        assert_eq!(s1[128], "a129");
        assert_eq!(s1[129], "a130");
    }

    #[test]
    fn weapon_fields_move_to_aw_offsets() {
        use fastfile_h1::layout::WeaponDef as h;
        use fastfile_s1::layout::WeaponDef as s;
        let mut def = vec![0u8; h::SIZE];
        def[h::clipSize..h::clipSize + 4].copy_from_slice(&30i32.to_le_bytes());
        def[h::impactType..h::impactType + 4].copy_from_slice(&2i32.to_le_bytes());
        let out = s1_def(&def);
        assert_eq!(out.len(), s::SIZE);
        assert_eq!(
            i32::from_le_bytes(out[s::clipSize..s::clipSize + 4].try_into().unwrap()),
            30
        );
        assert_eq!(
            i32::from_le_bytes(out[s::impactType..s::impactType + 4].try_into().unwrap()),
            2
        );
        assert_eq!(s1_offset(h::fireSound), Some(s::fireSound));
        assert_eq!(s1_offset(h::hudIcon), Some(s::hudIcon));
    }

    #[test]
    fn a_part_made_in_the_gun_space_stays() {
        let h = std::f32::consts::FRAC_1_SQRT_2;
        let bone = Pose {
            q: [0.0, 0.0, h, h],
            t: [5.0, 1.0, 2.0],
        };
        let at = bone.compose(&inverse(&bone));
        let v = at.apply([3.0, 4.0, 5.0]);
        assert!(
            (v[0] - 3.0).abs() < 1e-4 && (v[1] - 4.0).abs() < 1e-4 && (v[2] - 5.0).abs() < 1e-4,
            "{v:?}"
        );
    }

    /// `CODLEGACY_HMW_ZONE=<mwr>/hmw-mod/zone cargo test -p assets --release
    /// hmw_guns -- --ignored --nocapture`: every Horizon MW gun built as a
    /// match builds it, its sounds resolved in the zones' sound banks and
    /// its HUD picture read; a table.
    #[test]
    #[ignore]
    fn hmw_guns() {
        let Some(dir) = asset_audio::hmw_zone_dir() else {
            println!("CODLEGACY_HMW_ZONE not set (or no hmw_*.ff there)");
            return;
        };
        let g = walk_hmw_guest_guns(
            true,
            MaterialCatalog::default(),
            &asset_transport::LoadProgress::default(),
        );
        for line in &g.report {
            println!("{line}");
        }
        println!("{}", hmw_anim_check(&g.xanims));
        let banks: Vec<_> = asset_audio::hmw_gun_zones(&dir)
            .iter()
            .filter_map(|p| asset_audio::hmw_weapon_sounds(p).ok())
            .collect();
        let resolves = |alias: &str| banks.iter().any(|b| b.index_in(NS, alias).is_some());
        let mut catalog = g.weapons;
        catalog.set_guest_names(true);
        println!(
            "\n| gun | zone | view model | view verts / tris / bones (parts) | world verts / tris | root | idle bones gun / arms (missing) | materials (grey) | anims (stubs left out) | sounds resolved | captured | picture |"
        );
        println!("|---|---|---|---|---|---|---|---|---|---|---|---|");
        let (mut grey, mut mats, mut snd_ok, mut snd_all, mut caught, mut pics) =
            (0, 0, 0, 0, 0, 0);
        for r in &g.built {
            let sounds_ok = r.sounds.iter().filter(|s| resolves(s)).count();
            let captured = asset_core::AssetKey::new(
                NS,
                asset_core::AssetKind::Weapon,
                format!("{}_mp", r.gun),
            )
            .ok()
            .and_then(|k| catalog.resolve_key(&k).ok().flatten());
            let script = captured
                .map(|i| catalog.script_name_of(i))
                .unwrap_or_else(String::new);
            let pic = r
                .hud_icon
                .as_ref()
                .and_then(|n| fastfile_h1::ui_image::read_hmw_icon(&dir, std::slice::from_ref(n)))
                .map(|(w, h, _)| format!("{w}x{h}"));
            grey += r.grey;
            mats += r.materials;
            snd_ok += sounds_ok;
            snd_all += r.sounds.len();
            caught += usize::from(captured.is_some());
            pics += usize::from(pic.is_some());
            let shape = |s: Option<(usize, usize, usize, usize)>| {
                s.map_or("-".to_owned(), |(v, t, b, p)| {
                    format!("{v} / {t} / {b} ({p})")
                })
            };
            println!(
                "| {} | {} | {} | {} | {} | {} | {} / {}{} | {} ({}) | {} ({}) | {} / {} | {} | {} |",
                r.gun,
                r.zone,
                r.view_model,
                shape(r.view),
                r.world
                    .map_or("-".to_owned(), |(v, t, _, _)| format!("{v} / {t}")),
                r.root,
                r.idle_bones.0,
                r.idle_bones.1,
                if r.idle_missing.is_empty() {
                    String::new()
                } else {
                    format!(" ({})", r.idle_missing.join(" "))
                },
                r.materials,
                r.grey,
                r.anims,
                r.stub_anims,
                sounds_ok,
                r.sounds.len(),
                if script.is_empty() {
                    "NO".to_owned()
                } else {
                    script
                },
                pic.unwrap_or_else(|| "-".to_owned()),
            );
        }
        println!(
            "\n{} of {} guns built; {caught} captured; materials {mats} ({grey} grey); sounds {snd_ok} of {snd_all} resolved; {pics} pictures",
            g.built.len(),
            bo2_profile::guest::HMW.len()
        );
        println!("\n| gun | default scope | view model on tag | world model |");
        println!("|---|---|---|---|");
        let mut unscoped = Vec::new();
        for r in &g.built {
            let sniper = bo2_profile::guest::HMW
                .iter()
                .any(|h| h.gun == r.gun && h.group == "weapon_sniper");
            if let Some(s) = &r.scope {
                println!(
                    "| {} | {} | {} | {} |",
                    r.gun,
                    s.attachment,
                    s.view.as_ref().map_or("-".to_owned(), |(m, t)| format!("{m} on {t}")),
                    s.world,
                );
            }
            if sniper && r.scope.as_ref().and_then(|s| s.view.as_ref()).is_none() {
                unscoped.push(r.gun.clone());
            }
        }
        assert!(unscoped.is_empty(), "snipers without their scope model: {unscoped:?}");
        assert!(g.built.len() >= 40, "guns built: {}", g.built.len());
        assert_eq!(caught, g.built.len(), "every built gun is in the catalog");
    }
}
