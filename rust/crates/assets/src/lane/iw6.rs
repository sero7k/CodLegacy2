//! bo2mp: Call of Duty: Ghosts' guns as guest guns in a Black Ops II match
//! (`gh_<gun>_mp`), from Ghosts' `common_core_mp` zone.
//!
//! `fastfile_iw6`'s exact walk reads the zone; this turns what it kept into
//! the engine's catalogs the way Black Ops II's own guns go in:
//!
//! - models into Black Ops II's captured form (`asset_t6::XModelRef`) and
//!   through its skinned-model builder: Ghosts' packed vertex is BO2's but
//!   for its unit vectors (unsigned 10:10:10 here, signed there), and its
//!   eight blend buckets fold into BO2's four;
//! - animations into BO2's captured form (the same IW-family part types and
//!   packing) under the Ghosts namespace;
//! - materials onto BO2's draw path (`T6Draw` from the technique set's
//!   name: Ghosts' `m_`/`mc_` model techniques read like BO2's `mc_` ones)
//!   with their colour, normal and specular maps, whose pixels come from
//!   the zone or, streamed, from `imagefile<N>.pak` beside it;
//! - weapons through `WeaponCatalog::capture_iw6`.
//!
//! Arms are Black Ops II's (the catalog's hands resolve to BO2's).

use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use asset_anim::XAnimBuild;
use asset_core::{AssetEdge, AssetNamespace, AssetRef, T6Blend, T6Cull, T6Draw, ZoneOwner};
use asset_game::{Iw6WeaponView, WeaponBuild, WeaponCatalog};
use asset_material::{
    AuthoredImage, AuthoredMaterial, MaterialCatalog, MaterialTextureBinding, TS_COLOR_MAP,
    TS_NORMAL_MAP, TS_SPECULAR_MAP,
};
use asset_model::{FpvMeshBuild, WorldWeaponBuild};
use bevy::asset::RenderAssetUsages;
use bevy::image::{ImageAddressMode, ImageFilterMode, ImageSampler, ImageSamplerDescriptor};
use bevy::prelude::Image;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use fastfile_iw6::AssetType as G;
use fastfile_iw6::walk::{AssetId, Capture, ImageRec, MaterialRec, XAnimRec, XModelRec};

/// `R_HashString` of the texture slots Black Ops II's lit model draw reads
/// (Ghosts hashes them the same way).
const COLOR_MAP_HASH: u32 = 0xa0ab_1041;
const NORMAL_MAP_HASH: u32 = 0x59d3_0d0f;
const SPECULAR_MAP_HASH: u32 = 0x34ec_ccb3;

/// The largest level kept of a gun's pictures (first person), and of the
/// third-person models' (`BO2MP_GH_TEXTURE=<size>` changes the first).
const FIRST_PERSON_MAX: u32 = 1024;
const WORLD_MAX: u32 = 512;

/// Ghosts' guns, ready to join the match's catalogs.
pub(crate) struct Iw6Guests {
    pub weapons: WeaponBuild,
    pub fpv: FpvMeshBuild,
    pub world_guns: WorldWeaponBuild,
    pub xanims: XAnimBuild,
    pub material_seed: MaterialCatalog,
    pub report: Vec<String>,
    /// bo2mp: the guns' own impact effects and tracers, under the Ghosts
    /// namespace (`iw6_gun_fx`).
    pub fx: asset_game::FxCatalog,
    pub tracers: asset_game::TracerCatalog,
}

impl Iw6Guests {
    pub(crate) fn empty(material_seed: MaterialCatalog, report: Vec<String>) -> Self {
        Self {
            weapons: WeaponBuild::default(),
            fpv: FpvMeshBuild::default(),
            world_guns: WorldWeaponBuild::default(),
            xanims: XAnimBuild::default(),
            material_seed,
            report,
            fx: Default::default(),
            tracers: Default::default(),
        }
    }
}

/// Ghosts' `zone/common_core_mp.ff`, if Ghosts is installed.
pub(crate) fn ghosts_common_core() -> Result<PathBuf, String> {
    let root = asset_transport::games_root_from_env().ok().map(|r| r.0);
    let extra: Vec<PathBuf> = asset_transport::game_paths::get("ghosts")
        .into_iter()
        .collect();
    let install = fastfile_iw6::install::find_install(
        root.as_deref().filter(|p| !p.as_os_str().is_empty()),
        &extra,
    )
    .ok_or("Ghosts is not installed (no zone/code_post_gfx_mp.ff found)")?;
    let path = install.join("zone").join("common_core_mp.ff");
    if path.is_file() {
        Ok(path)
    } else {
        Err(format!("{} is missing", path.display()))
    }
}

/// Ghosts' common_core_mp read ahead of the build (inflated and walked),
/// or why there is none.
pub(crate) enum Iw6Prefetch {
    Off(Vec<String>),
    Ready(Box<Iw6Zone>),
}

pub(crate) struct Iw6Zone {
    path: PathBuf,
    zone: fastfile_iw6::Zone,
    cap: Capture,
    report: Vec<String>,
    started: std::time::Instant,
}

/// bo2mp: reads Ghosts' common_core_mp for its guns (`wanted`: Black Ops II
/// rules with guest guns on), off the match's critical path; the build
/// (`build_iw6_guest_guns`) links them after.
pub(crate) fn prefetch_iw6_guest_guns(wanted: bool) -> Iw6Prefetch {
    if !wanted {
        return Iw6Prefetch::Off(vec!["iw6 guest guns: off".into()]);
    }
    let path = match ghosts_common_core() {
        Ok(path) => path,
        Err(e) => return Iw6Prefetch::Off(vec![format!("iw6 guest guns: {e}")]),
    };
    let started = std::time::Instant::now();
    let zone = match fastfile_iw6::Zone::open(&path) {
        Ok(zone) => zone,
        Err(e) => {
            return Iw6Prefetch::Off(vec![format!(
                "iw6 guest guns: open {}: {e}",
                path.display()
            )]);
        }
    };
    let (cap, stop) = fastfile_iw6::walk::walk_zone(&zone);
    let pen_line = iw6_pen_table(&path, &zone);
    let report = vec![pen_line, format!(
        "iw6 guest guns: walked {} of {} assets of {} ({} weapons, {} models, {} animations, {} materials, {} images) [{:.1} s]{}",
        cap.walked,
        zone.assets.assets.len(),
        path.display(),
        cap.weapons.len(),
        cap.xmodels.len(),
        cap.xanims.len(),
        cap.materials.len(),
        cap.images.len(),
        started.elapsed().as_secs_f64(),
        stop.map(|s| format!(
            "; stopped at asset {} ({:?}): {}",
            s.asset_index, s.asset_type, s.error
        ))
        .unwrap_or_default()
    )];
    Iw6Prefetch::Ready(Box::new(Iw6Zone {
        path,
        zone,
        cap,
        report,
        started,
    }))
}

/// bo2mp: Ghosts' own penetration depths (`info/bullet_penetration_mp`)
/// for its guns, looked for once a process: in common_core_mp (open
/// already), then the install's code and common zones
/// (`fastfile_iw6::penetration::ZONES`). `BO2MP_IW6_PEN_TABLE=0`: not read
/// (Ghosts' guns then use Black Ops II's table, as before).
fn iw6_pen_table(common_core: &std::path::Path, zone: &fastfile_iw6::Zone) -> String {
    static TRIED: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    if std::env::var("BO2MP_IW6_PEN_TABLE").is_ok_and(|v| v.trim() == "0") {
        asset_world::set_iw6_pen_table(None);
        return "iw6 penetration table: off (BO2MP_IW6_PEN_TABLE=0; Ghosts' guns use Black Ops II's)".into();
    }
    TRIED
        .get_or_init(|| {
            use fastfile_iw6::penetration as pen;
            let found = pen::find_in_image(&zone.image)
                .and_then(|b| pen::parse(&String::from_utf8_lossy(&b)))
                .map(|t| (t, "common_core_mp".to_owned()))
                .ok_or_else(String::new)
                .or_else(|_| pen::find_in_zones(common_core.parent().unwrap_or(std::path::Path::new("."))));
            match found {
                Ok((t, from)) => {
                    asset_world::set_iw6_pen_table(Some((asset_world::iw6_pen_depths_to_t6(&t.depths), from.clone())));
                    format!(
                        "iw6 penetration table: {} keys from {from}'s {} (Ghosts' guns use it; BO2MP_IW6_PEN_TABLE=0 for Black Ops II's)",
                        t.keys_set,
                        pen::RAW_FILE
                    )
                }
                Err(e) => format!("iw6 penetration table: not found ({e}); Ghosts' guns use Black Ops II's"),
            }
        })
        .clone()
}

/// bo2mp: the match's Ghosts guest guns from what `prefetch_iw6_guest_guns`
/// read, their materials linked into `material_seed`.
pub(crate) fn build_iw6_guest_guns(
    prefetch: Iw6Prefetch,
    material_seed: MaterialCatalog,
    progress: &asset_transport::LoadProgress,
) -> Iw6Guests {
    let zone = match prefetch {
        Iw6Prefetch::Off(report) => return Iw6Guests::empty(material_seed, report),
        Iw6Prefetch::Ready(zone) => zone,
    };
    let stage = progress.begin_scoped(
        asset_transport::StageId::CommonAssets,
        "iw6_guest_weapons",
        None,
    );
    let guests = load_iw6_guests(*zone, material_seed);
    stage.done();
    guests
}

/// bo2mp: the match's Ghosts guest guns read and built in one go
/// (`CodLegacy check-guns`).
pub(crate) fn walk_iw6_guest_guns(
    wanted: bool,
    material_seed: MaterialCatalog,
    progress: &asset_transport::LoadProgress,
) -> Iw6Guests {
    build_iw6_guest_guns(prefetch_iw6_guest_guns(wanted), material_seed, progress)
}

/// Ghosts' guns from its walked `common_core_mp`, their materials linked
/// into `material_seed`. A later start reads the finished guns back.
fn load_iw6_guests(z: Iw6Zone, material_seed: MaterialCatalog) -> Iw6Guests {
    let fp = std::env::var("BO2MP_GH_TEXTURE").ok().and_then(|v| v.trim().parse().ok()).unwrap_or(FIRST_PERSON_MAX);
    let caps = [("fp", fp), ("world", WORLD_MAX)];
    let code = [include_str!("finished_gun_cache.rs"), include_str!("iw6.rs"), include_str!("iw6_gun_fx.rs")];
    let name = z.path.file_stem().map_or_else(|| "common_core_mp".to_owned(), |s| s.to_string_lossy().into_owned());
    let names = vec![name.clone()];
    let files = [z.path.clone()];
    let src = super::finished_gun_cache::Sources { lane: "iw6", name: &name, files: &files, caps: &caps, code: &code, zones: &names, note: "", missing_ok: false };
    super::finished_gun_cache::load_or_build(&src, material_seed, |seed| load_iw6_guests_now(z, seed), pack_iw6, unpack_iw6)
}

fn pack_iw6(guests: &Iw6Guests, at: super::finished_gun_cache::MatAt) -> Result<Vec<u8>, String> {
    let projectiles = asset_model::ProjectileMeshBuild::default();
    let fx_models = asset_game::FxModelCatalog::default();
    super::finished_gun_cache::pack_parts(&super::finished_gun_cache::Parts {
        weapons: &guests.weapons,
        fpv: &guests.fpv,
        world: &guests.world_guns,
        projectiles: &projectiles,
        xanims: &guests.xanims,
        fx: &guests.fx,
        fx_models: &fx_models,
        tracers: &guests.tracers,
        impact: None,
        materials: &guests.material_seed,
        mat_at: at,
        reports: &guests.report,
        extra: &[],
    })
}

fn unpack_iw6(bytes: &[u8], seed: MaterialCatalog) -> Result<Iw6Guests, MaterialCatalog> {
    match super::finished_gun_cache::unpack_parts(bytes, seed) {
        Ok(rest) => Ok(Iw6Guests {
            weapons: rest.weapons,
            fpv: rest.fpv,
            world_guns: rest.world,
            xanims: rest.xanims,
            material_seed: rest.materials,
            report: rest.reports,
            fx: rest.fx,
            tracers: rest.tracers,
        }),
        Err(seed) => Err(seed),
    }
}

fn load_iw6_guests_now(z: Iw6Zone, material_seed: MaterialCatalog) -> Iw6Guests {
    let Iw6Zone {
        path,
        zone,
        cap,
        mut report,
        started,
    } = z;
    let path = path.as_path();
    let mut b = Builder {
        cap: &cap,
        image: &zone.image,
        strings: &zone.assets.script_strings,
        paks: fastfile_iw6::image::ImagePaks::new(path.parent().unwrap_or(Path::new("."))),
        catalog: material_seed,
        owner: ZoneOwner::from_zone_path(path),
        materials: HashMap::new(),
        images: HashMap::new(),
        image_failures: Vec::new(),
        decoded: 0,
        shared: None,
        from_shared: 0,
        world_shine: false,
        nocast: std::collections::HashSet::new(),
        named_images: HashMap::new(),
    };
    let first_person_max = std::env::var("BO2MP_GH_TEXTURE")
        .ok()
        .and_then(|v| v.trim().parse().ok())
        .unwrap_or(FIRST_PERSON_MAX);

    // The guns: Ghosts' own multiplayer weapons (`iw6_*_mp`).
    let guns: Vec<&fastfile_iw6::walk::WeaponRec> = cap
        .weapons
        .iter()
        .filter(|w| {
            let n = w.name.to_ascii_lowercase();
            n.starts_with("iw6_") && n.ends_with("_mp")
        })
        .collect();
    let mut first_person: BTreeSet<u32> = BTreeSet::new();
    let mut third_person: BTreeSet<u32> = BTreeSet::new();
    for w in &guns {
        first_person.extend(w.gun_models.first().copied().flatten().map(|id| id.index));
        third_person.extend(w.world_models.first().copied().flatten().map(|id| id.index));
        for (off, id) in &w.models {
            use fastfile_iw6::layout::WeaponDef as d;
            if [d::projectileModel, d::rocketModel, d::knifeModel].contains(off)
                && id.ty == G::XModel
            {
                first_person.insert(id.index);
            }
        }
    }

    // bo2mp: each gun's attachments and a sniper's own scope: their models
    // (kept where their root is a bone of the gun), animations and sounds.
    let attachments: Vec<(String, Option<asset_game::GuestAttachment>, Vec<asset_game::GuestAttachment>)> =
        guns.iter().map(|w| b.guest_attachments(w, &mut first_person, &mut third_person)).collect();
    let mut attachment_anims: BTreeSet<u32> = BTreeSet::new();
    for w in &guns {
        attachment_anims.extend(w.anim_overrides.iter().filter_map(|o| o.override_anim).map(|id| id.index));
    }

    let mut fpv = FpvMeshBuild::default();
    fpv.set_capture_ns(AssetNamespace::Iw6);
    let mut world_guns = WorldWeaponBuild::default();
    world_guns.set_capture_ns(AssetNamespace::Iw6);
    let mut unreadable = Vec::new();
    for (set, max, first) in [
        (&first_person, first_person_max, true),
        (&third_person, WORLD_MAX, false),
    ] {
        for &mi in set {
            let Some(model) = cap.xmodels.get(mi as usize) else {
                continue;
            };
            let Some((xref, keys)) = b.model_ref(model) else {
                unreadable.push(format!("{} (no surfaces)", model.name));
                continue;
            };
            let linked: HashMap<usize, usize> = keys
                .into_iter()
                .filter_map(|k| Some((k, b.material(k, max)?)))
                .collect();
            match asset_model::capture_model_skel_t6(&xref, |k| linked.get(&k.index).copied()) {
                Some(skel) if first => fpv.insert_in(AssetNamespace::Iw6, skel, Some(&b.catalog)),
                Some(skel) => world_guns.insert_in(AssetNamespace::Iw6, skel, Some(&b.catalog)),
                None => unreadable.push(format!("{} (unreadable)", model.name)),
            }
        }
    }
    report.push(format!(
        "iw6 guest guns: models: {} first person, {} third person built; {} not {:?}",
        fpv.len(),
        world_guns.len(),
        unreadable.len(),
        &unreadable[..unreadable.len().min(8)]
    ));
    report.push(format!(
        "iw6 guest guns: materials: {} linked, {} pictures decoded, {} failed {:?}",
        b.materials.len(),
        b.decoded,
        b.image_failures.len(),
        &b.image_failures[..b.image_failures.len().min(6)]
    ));

    // Animations every gun names.
    let mut xanims = XAnimBuild::default();
    xanims.set_capture_ns(AssetNamespace::Iw6);
    let mut wanted_anims: BTreeSet<u32> = BTreeSet::new();
    for w in &guns {
        for id in w.anims.iter().chain(&w.anims_right).flatten() {
            wanted_anims.insert(id.index);
        }
    }
    wanted_anims.extend(attachment_anims);
    let mut anim_n = 0usize;
    for &ai in &wanted_anims {
        if let Some(a) = cap.xanims.get(ai as usize)
            && let Some(r) = b.anim_ref(a)
        {
            anim_n += usize::from(xanims.insert_t6_as(AssetNamespace::Iw6, &r));
        }
    }
    report.push(format!(
        "iw6 guest guns: animations: {anim_n} of {} named kept ({} gaps)",
        wanted_anims.len(),
        xanims.capture_gaps
    ));

    // The weapon catalog.
    let mut catalog = WeaponCatalog::default();
    catalog.set_capture_ns(AssetNamespace::Iw6);
    let mut no_def = Vec::new();
    for w in &guns {
        let view = b.weapon_view(w);
        if view.def.is_empty() {
            no_def.push(w.name.clone());
        }
        catalog.capture_iw6(&view);
    }
    let captured = catalog.len();
    let mut weapons = catalog.into_build();
    weapons.stamp_namespace(AssetNamespace::Iw6);
    weapons.resolve_sz_xanim_edges(&xanims);
    weapons.resolve_fpv_mesh_edges(&fpv);
    weapons.resolve_world_model_edges(&world_guns);
    let mut scoped = Vec::new();
    for (gun, scope, offered) in attachments {
        let Some(id) = weapons.resolve_index(&gun).ok().flatten().filter(|&i| i != 0) else {
            continue;
        };
        if let Some(scope) = scope {
            scoped.push(gun.clone());
            weapons.give_guest_scope(id, scope);
        }
        report.push(format!("iw6 guest guns: {gun} attachments: {}", super::guest_attach::words(&offered)));
        weapons.offer_guest_attachments(id, offered);
    }
    report.push(format!("iw6 guest guns: own scopes on {scoped:?}"));
    // bo2mp: the guns' own impacts and tracers (`iw6_gun_fx`).
    let gun_fx = if super::iw6_gun_fx::own_impacts_on() || super::iw6_gun_fx::own_tracers_on() || super::iw6_gun_fx::ads_flash_on() {
        b.shared = path.parent().and_then(super::iw6_map::shared_images);
        let gun_fx = super::iw6_gun_fx::build(&mut b, &guns);
        for (gun, impact) in &gun_fx.impacts {
            if let Some(id) = weapons.resolve_index(gun).ok().flatten().filter(|&i| i != 0) {
                weapons.set_guest_effects(id, asset_game::GuestEffects::new(Vec::new(), Some(impact.clone())));
            }
        }
        gun_fx
    } else {
        super::iw6_gun_fx::GunFx {
            report: vec!["iw6 guest guns: own impacts and tracers off (BO2MP_IW6_OWN_IMPACTS=0, BO2MP_IW6_OWN_TRACERS=0)".into()],
            ..Default::default()
        }
    };
    report.extend(gun_fx.report.iter().cloned());
    report.push(format!(
        "iw6 guest guns: {captured} weapons captured, {} in the catalog ({} without a WeaponDef {:?}) [{:.1} s]",
        weapons.len(),
        no_def.len(),
        &no_def[..no_def.len().min(6)],
        started.elapsed().as_secs_f64()
    ));
    let Builder {
        catalog: material_seed,
        ..
    } = b;
    Iw6Guests {
        weapons,
        fpv,
        world_guns,
        xanims,
        material_seed,
        report,
        fx: gun_fx.fx,
        tracers: gun_fx.tracers,
    }
}

/// bo2mp `CodLegacy check-guns`: whether each Ghosts gun's sounds play — its
/// fire sound and its notetrack (reload) sounds found in Ghosts' sound
/// zones with a clip to play (as the match's sound bank takes them).
pub(crate) fn iw6_sound_check(weapons: &asset_game::WeaponRegistry) -> Vec<String> {
    let root = asset_transport::games_root_from_env().ok();
    let Some(install) = asset_audio::ghosts_install(root.as_ref().map(|r| r.0.as_path())) else {
        return vec!["iw6 sounds: Ghosts is not installed".into()];
    };
    let mut bank = asset_audio::SoundCatalog::default();
    let mut lines = Vec::new();
    for zone in asset_audio::ghosts_sound_zones(&install) {
        match asset_audio::ghosts_weapon_sounds(&zone) {
            Ok(c) => {
                lines.push(format!(
                    "iw6 sounds: {}: {} alias lists, {} clips",
                    zone.display(),
                    c.sounds.len(),
                    c.loaded.len()
                ));
                bank.absorb_missing_aliases((*c).clone());
            }
            Err(e) => lines.push(format!("iw6 sounds: {e}")),
        }
    }
    let ns = AssetNamespace::Iw6;
    let playable = |alias: &str| {
        bank.sound_in(ns, alias).is_some_and(|s| {
            s.aliases
                .iter()
                .any(|a| a.loaded.is_bound() || a.streamed.is_some())
        })
    };
    let (mut fire_ok, mut guns, mut nt_ok, mut nt_all) = (0, 0, 0, 0);
    let mut missing = Vec::new();
    for i in 1..=weapons.len() as u32 {
        if weapons.namespace_of(i) != Some(ns) {
            continue;
        }
        let Some(sounds) = weapons.sounds_of(i) else {
            continue;
        };
        guns += 1;
        let fire = sounds.fire_player.as_deref().or(sounds.fire.as_deref());
        if fire.is_some_and(playable) {
            fire_ok += 1;
        } else {
            missing.push(format!(
                "{} fire `{}`",
                weapons.name_of(i),
                fire.unwrap_or("-")
            ));
        }
        // (The match links these to the animations' notetracks.)
        for (_, alias) in &sounds.notetrack_sound_map {
            nt_all += 1;
            nt_ok += usize::from(playable(alias));
        }
    }
    lines.push(format!(
        "iw6 sounds: fire sound playable for {fire_ok} of {guns} weapons; notetrack (reload/handling) sounds {nt_ok} of {nt_all}{}",
        if missing.is_empty() { String::new() } else { format!("; missing: {}", missing.iter().take(8).cloned().collect::<Vec<_>>().join(", ")) }
    ));
    lines
}

/// bo2mp `CodLegacy check-guns`: every Ghosts gun animation decoded by the
/// engine's IW-family decoder (which refuses a data stream it does not use
/// up exactly), and the decoded rotations' unit length.
pub(crate) fn iw6_anim_check(xanims: &XAnimBuild) -> String {
    let (mut ok, mut bad, mut worst) = (0usize, Vec::new(), 0f32);
    let names: Vec<String> = xanims.names().map(str::to_owned).collect();
    for name in &names {
        match xanims
            .get(AssetNamespace::Iw6, name)
            .map(|c| xmodel_runtime::AnimClip::from_parts(&c.parts))
        {
            Some(Ok(clip)) => {
                ok += 1;
                for t in &clip.tracks {
                    use xmodel_runtime::Rotation as R;
                    let qs: Vec<[f32; 4]> = match &t.rotation {
                        R::HalfKeyed(k) | R::FullKeyed(k) => k.values.clone(),
                        R::HalfConstant(q) | R::FullConstant(q) => vec![*q],
                        R::Default => Vec::new(),
                    };
                    for q in qs {
                        worst =
                            worst.max((q.iter().map(|c| c * c).sum::<f32>().sqrt() - 1.0).abs());
                    }
                }
            }
            Some(Err(e)) => bad.push(format!("{name}: {e:?}")),
            None => {}
        }
    }
    format!(
        "iw6 animations: {ok} decode, {} refused{}; largest |quat|-1 = {worst:.4}",
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

pub(super) struct Builder<'a> {
    pub(super) cap: &'a Capture,
    pub(super) image: &'a [u8],
    pub(super) strings: &'a [Option<String>],
    pub(super) paks: fastfile_iw6::image::ImagePaks,
    pub(super) catalog: MaterialCatalog,
    pub(super) owner: ZoneOwner,
    /// Capture material index -> catalog material index.
    pub(super) materials: HashMap<usize, usize>,
    /// (capture image index, size cap) -> catalog image slot.
    pub(super) images: HashMap<(usize, u32), Option<usize>>,
    pub(super) image_failures: Vec<String>,
    pub(super) decoded: usize,
    /// bo2mp: images the zone names but another zone holds (`,name`:
    /// common_mp's), for a map's builder (`iw6_map::shared_images`).
    pub(super) shared: Option<std::sync::Arc<fastfile_iw6::shared::SharedImages>>,
    /// How many pictures came from `shared` or the engine's built-ins.
    pub(super) from_shared: usize,
    /// (image name, size cap) -> catalog image slot, for shared materials.
    pub(super) named_images: HashMap<(String, u32), Option<usize>>,
    /// bo2mp: world materials take their normal and specular maps too (the
    /// Black Ops III look's lit shader reads them: `iw6_map`).
    pub(super) world_shine: bool,
    /// bo2mp: catalog materials of models whose technique set says `nocast`
    /// (they cast no shadow into the sun map: `bo3_look::apply`).
    pub(super) nocast: std::collections::HashSet<usize>,
}

/// Where a material's picture is: the zone's image table, or by name in
/// the shared zones.
#[derive(Clone, Debug)]
enum MatImage {
    Zone(usize),
    Named(String),
}

/// A material as `Builder` reads it (`Builder::view`).
struct MatView {
    name: String,
    techset: String,
    sort_key: u8,
    /// (name hash, sampler state, semantic, image)
    textures: Vec<(u32, u8, u8, MatImage)>,
    constants: Vec<fastfile_iw6::walk::MaterialConstant>,
}

pub(super) fn le_u16s(b: &[u8]) -> Vec<u16> {
    b.chunks_exact(2)
        .map(|c| u16::from_le_bytes([c[0], c[1]]))
        .collect()
}

pub(super) fn le_u32s(b: &[u8]) -> Vec<u32> {
    b.chunks_exact(4)
        .map(|c| u32::from_le_bytes([c[0], c[1], c[2], c[3]]))
        .collect()
}

/// Ghosts' packed unit vector (unsigned 10:10:10 over 1023) as Black Ops
/// II's (signed 10:10:10 over 511).
pub(super) fn iw6_unit_vec_to_t6(packed: u32) -> u32 {
    let mut out = 0u32;
    for k in 0..3 {
        let v = ((packed >> (10 * k)) & 0x3ff) as f32 / 1023.0 * 2.0 - 1.0;
        let s = (v.clamp(-1.0, 1.0) * 511.0).round() as i32;
        out |= ((s as u32) & 0x3ff) << (10 * k);
    }
    out
}

/// One Ghosts `GfxPackedVertex` as Black Ops II's: the same position,
/// binormal sign, colour and texture coordinates; the unit vectors
/// repacked.
pub(super) fn iw6_vertex_to_t6(v: &[u8]) -> [u8; 32] {
    let mut out = [0u8; 32];
    out[..24].copy_from_slice(&v[..24]);
    for at in [24usize, 28] {
        let p = u32::from_le_bytes([v[at], v[at + 1], v[at + 2], v[at + 3]]);
        out[at..at + 4].copy_from_slice(&iw6_unit_vec_to_t6(p).to_le_bytes());
    }
    out
}

/// Ghosts' blend streams (eight buckets: vertices with 1..8 influences, a
/// bone word then a bone and weight word per extra influence) as Black Ops
/// II's four: a vertex with more than four keeps its first four (the rest's
/// weight goes to its first bone).
pub(super) fn fold_blend(counts: [u16; 8], words: &[u16]) -> ([i16; 4], Vec<u16>) {
    let mut out_counts = [0i16; 4];
    let mut out = Vec::with_capacity(words.len());
    let mut at = 0usize;
    for (bucket, &count) in counts.iter().enumerate() {
        let per = 1 + 2 * bucket;
        let keep = per.min(7);
        for _ in 0..count {
            let Some(w) = words.get(at..at + per) else {
                break;
            };
            out.extend_from_slice(&w[..keep]);
            at += per;
            out_counts[bucket.min(3)] += 1;
        }
    }
    (out_counts, out)
}

/// bo2mp: how a Ghosts (or Modern Warfare Remastered) world material moves:
/// by the words of its names (a stream, a waterfall, a scrolling technique;
/// `flag` techniques wave) and by its `uvAnimParms` constant (the picture's
/// scroll in u and v a second) on a lit surface. Effects, distortions and
/// shadow-only materials stand still.
pub(super) fn iw6_world_anim<'a>(
    draw: &T6Draw,
    technique_set: &str,
    material: &str,
    mut constants: impl Iterator<Item = (&'a str, [f32; 4])>,
) -> asset_core::T6Anim {
    use asset_core::T6Anim;
    if draw.effect || draw.cloud || matches!(draw.blend, T6Blend::ShadowOnly | T6Blend::Sky) {
        return T6Anim::None;
    }
    let by_name = draw.anim.or(asset_core::t6_anim_kind(technique_set, material, true));
    if by_name != T6Anim::None || draw.unlit {
        return by_name;
    }
    constants
        .find(|c| c.0 == "uvAnimParms")
        .map_or(T6Anim::None, |c| asset_core::t6_anim_for_scroll_rate([c.1[0], c.1[1]]))
}

/// Ghosts' model technique set as Black Ops II's draw: by its name (the
/// `m_`/`mc_` model techniques' tokens read as BO2's), drawn double-sided,
/// none of BO2's own special kinds.
pub(super) fn iw6_model_draw(technique_set: &str, sort_key: u8) -> T6Draw {
    let name = technique_set.trim_start_matches(',');
    let name = name.rsplit(['/', '\\']).next().unwrap_or(name);
    let name = match name.strip_prefix("m_") {
        Some(rest) => format!("mc_{rest}"),
        None => name.to_owned(),
    };
    let mut draw = T6Draw::from_technique_set(&name, sort_key, None);
    draw.cull = T6Cull::None;
    draw.water = false;
    draw.flow = false;
    draw.tile = false;
    draw.flag = false;
    draw.tile_blend = false;
    draw.foliage = false;
    draw.objective = false;
    draw.layered = false;
    draw.layers = [0, 0];
    if draw.blend == T6Blend::Sky {
        draw.blend = T6Blend::Opaque;
    }
    // The reflex and holo sights' reticle: a glowing dot, added on unlit.
    if name
        .split('_')
        .any(|t| t.contains("reflex") || t.contains("holo"))
        && draw.blend != T6Blend::Opaque
    {
        draw.unlit = true;
        draw.blend = T6Blend::Add;
        draw.alpha_test = false;
    }
    // Ghosts' cut-out foliage (`unlit_atest_lin_..._bl`: tree canopies, twigs,
    // vista trees; `atest` is its alpha test, `bl` its baked light): the
    // reader knows only Black Ops II's `t0c0` for a cut-out, so these drew
    // as opaque, unlit cards (dark sheets of leaf picture). They are alpha
    // tested, drawn two-sided (above) and lit as the other foliage is (the
    // map's light stands in for the baked one, which is not read).
    if is_alpha_tested(&name) && draw.blend != T6Blend::ShadowOnly {
        draw.alpha_test = true;
        draw.unlit = false;
        draw.blend = T6Blend::Opaque;
    }
    draw
}

/// Whether a Ghosts technique set name carries the alpha test token.
pub(super) fn is_alpha_tested(technique_set: &str) -> bool {
    technique_set.split('_').any(|t| t == "atest")
}

/// Whether a Ghosts technique set name marks its surfaces as not casting
/// shadows (`nocast`, `nocastp0`: a tree's leaf cards; the tree's own
/// `shadowcaster_atest` cards cast for it).
pub(super) fn is_nocast(technique_set: &str) -> bool {
    technique_set.split('_').any(|t| t.starts_with("nocast"))
}

/// A Ghosts technique set's layer tokens as Black Ops II's reader takes
/// them: Ghosts writes a height-blended layer as `b1ha1c1` or `b1hb1c1` (and
/// a base as `r0ha0c0`), the colour map after the `ha`; the reader wants it right
/// after the layer number (`b1c1`), so such layers were not drawn at all
/// (their surfaces showed the base layer alone).
pub(super) fn ghosts_layer_tokens(technique_set: &str) -> String {
    technique_set
        .split('_')
        .map(|t| {
            let b = t.as_bytes();
            if b.len() >= 5 && matches!(b[0], b'r' | b'b' | b'm' | b't' | b'a') && b[1].is_ascii_digit() && b[2] == b'h' && b[3].is_ascii_lowercase() {
                let rest = t[4..].trim_start_matches(|c: char| c.is_ascii_digit());
                format!("{}{rest}", &t[..2])
            } else {
                t.to_owned()
            }
        })
        .collect::<Vec<_>>()
        .join("_")
}

/// Ghosts' layer tokens: a height-blended layer reads as a blend layer.
#[cfg(test)]
mod layer_token_tests {
    #[test]
    fn height_blended_layers_read_as_layers() {
        assert_eq!(super::ghosts_layer_tokens("l_sm_r0c0nl0sl0l0_b1ha1c1nl1sl1l1_nfwpf"), "l_sm_r0c0nl0sl0l0_b1c1nl1sl1l1_nfwpf");
        assert_eq!(super::ghosts_layer_tokens("wc_l_sm_r0ha0c0l0_nfwpf"), "wc_l_sm_r0c0l0_nfwpf");
        assert_eq!(super::ghosts_layer_tokens("l_sm_r0c0nl0sl0l0_b1hb1c1z1nl1sl1l1_y2"), "l_sm_r0c0nl0sl0l0_b1c1z1nl1sl1l1_y2");
        assert_eq!(super::ghosts_layer_tokens("wc_unlit_add_lin_ndw_ua_bl_blmod"), "wc_unlit_add_lin_ndw_ua_bl_blmod");
        let layers = asset_core::T6Layers::from_technique_set(&super::ghosts_layer_tokens("l_sm_r0c0l0_b1ha1c1l1_nfwpf"));
        assert_eq!(layers.kinds[0], 1);
    }

    /// Prison Break's tree canopies (`unlit_atest_lin_..._bl`) are lit,
    /// alpha tested, two-sided cut-outs; its other cards keep their draw.
    #[test]
    fn atest_foliage_is_a_lit_two_sided_cutout() {
        use asset_core::{T6Blend, T6Cull};
        for name in [
            ",m_unlit_atest_lin_flagext_wv0_nocast_bl_nfwpf",
            ",m_unlit_atest_lin_nocast_bl_nfwpf",
        ] {
            let d = super::iw6_model_draw(name, 1);
            assert!(d.alpha_test && !d.unlit && d.blend == T6Blend::Opaque && d.cull == T6Cull::None, "{name}");
            // (Even with a blended sort key.)
            let late = super::iw6_model_draw(name, 9);
            assert!(late.alpha_test && late.blend == T6Blend::Opaque, "{name}");
            assert!(super::is_alpha_tested(name) && super::is_nocast(name));
        }
        // The tree's shadow card stays shadow only; emissive panels stay unlit.
        assert_eq!(super::iw6_model_draw(",m_shadowcaster_atest", 1).blend, T6Blend::ShadowOnly);
        let panel = super::iw6_model_draw(",m_unlit_replace_lin_bl_nfwpf", 1);
        assert!(panel.unlit && !panel.alpha_test);
        // `t0c0` stays a cut-out; `nocastp0` is a `nocast`.
        assert!(super::iw6_model_draw(",m_ambient_t0c0l0_nocast_nfwpf", 0).alpha_test);
        assert!(super::is_nocast(",m_l_sm_r0c0n0s0l0_nocastp0_nfwpf"));
        assert!(!super::is_nocast(",m_l_sm_r0c0n0s0l0_nfwpf"));
    }
}

/// Ghosts' BC5 normal maps as Black Ops II's model shader reads them.
///
/// Ghosts stores the tangent-space normal's x and y (z rebuilt), signed
/// (`BC5_SNORM`, or unsigned over 0..1 for `BC5_UNORM`). Black Ops II's lit
/// model draw reads its normal maps as unsigned bytes and takes
/// `xy * 4.0157 - 2.0157`, twice the signed value, as the slope it adds to
/// the vertex normal: so each value here becomes `v / 2` stored unsigned
/// (`128 + v / 2` in bytes). Interpolation is linear, so moving each
/// channel's two endpoints moves every texel of the block; a block whose
/// endpoints meet once halved is made flat (one value, as its texels all
/// but were).
fn bc5_normals_for_bo2(data: &mut [u8], signed: bool) {
    for channel in data.chunks_exact_mut(8) {
        let map = |b: u8| -> u8 {
            if signed {
                (128 + i16::from(b as i8) / 2).clamp(0, 255) as u8
            } else {
                64 + b / 2
            }
        };
        let (a, b) = (channel[0], channel[1]);
        let six_values = if signed { (a as i8) > (b as i8) } else { a > b };
        let (na, nb) = (map(a), map(b));
        channel[0] = na;
        channel[1] = nb;
        if six_values && na == nb {
            channel[2..8].fill(0);
        }
    }
}

/// A GPU image of Ghosts pixels: block formats as they are (BC5 signed
/// normals made unsigned), colour maps read as sRGB.
pub(super) fn gpu_image(px: &fastfile_iw6::image::ImagePixels, colour: bool) -> Result<Image, String> {
    let srgb = |linear: TextureFormat, s: TextureFormat| if colour { s } else { linear };
    let mut data = px.data.clone();
    let format = match px.dxgi_format {
        70..=72 => srgb(TextureFormat::Bc1RgbaUnorm, TextureFormat::Bc1RgbaUnormSrgb),
        73..=75 => srgb(TextureFormat::Bc2RgbaUnorm, TextureFormat::Bc2RgbaUnormSrgb),
        76..=78 => srgb(TextureFormat::Bc3RgbaUnorm, TextureFormat::Bc3RgbaUnormSrgb),
        79 | 80 => TextureFormat::Bc4RUnorm,
        81 => TextureFormat::Bc4RSnorm,
        82..=84 => {
            bc5_normals_for_bo2(&mut data, px.dxgi_format == 84);
            TextureFormat::Bc5RgUnorm
        }
        95 => TextureFormat::Bc6hRgbUfloat,
        96 => TextureFormat::Bc6hRgbFloat,
        97 | 98 | 99 => srgb(TextureFormat::Bc7RgbaUnorm, TextureFormat::Bc7RgbaUnormSrgb),
        27..=29 => srgb(TextureFormat::Rgba8Unorm, TextureFormat::Rgba8UnormSrgb),
        87 | 90 | 91 => srgb(TextureFormat::Bgra8Unorm, TextureFormat::Bgra8UnormSrgb),
        other => return Err(format!("DXGI format {other} is not drawn")),
    };
    let block = matches!(px.dxgi_format, 70..=84 | 94..=99);
    if block && (px.width % 4 != 0 || px.height % 4 != 0) {
        return Err(format!(
            "{}x{} is not whole 4x4 blocks",
            px.width, px.height
        ));
    }
    let mut image = Image::new_uninit(
        Extent3d {
            width: px.width,
            height: px.height,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        format,
        RenderAssetUsages::RENDER_WORLD,
    );
    image.texture_descriptor.mip_level_count = px.levels.max(1);
    image.data = Some(data);
    image.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
        address_mode_u: ImageAddressMode::Repeat,
        address_mode_v: ImageAddressMode::Repeat,
        mag_filter: ImageFilterMode::Linear,
        min_filter: ImageFilterMode::Linear,
        mipmap_filter: ImageFilterMode::Linear,
        anisotropy_clamp: 8,
        ..Default::default()
    });
    Ok(image)
}

impl<'a> Builder<'a> {
    /// bo2mp: a builder over a walked zone (a Ghosts map's: `iw6_map`),
    /// its streamed pictures read from the paks in `zone_dir`.
    pub(super) fn new(
        cap: &'a Capture,
        zone: &'a fastfile_iw6::Zone,
        zone_path: &Path,
        catalog: MaterialCatalog,
    ) -> Self {
        Self {
            cap,
            image: &zone.image,
            strings: &zone.assets.script_strings,
            paks: fastfile_iw6::image::ImagePaks::new(zone_path.parent().unwrap_or(Path::new("."))),
            catalog,
            owner: ZoneOwner::from_zone_path(zone_path),
            materials: HashMap::new(),
            images: HashMap::new(),
            image_failures: Vec::new(),
            decoded: 0,
            shared: None,
            from_shared: 0,
            world_shine: false,
            nocast: std::collections::HashSet::new(),
            named_images: HashMap::new(),
        }
    }

    /// bo2mp: a world surface's material (a Ghosts map's `GfxWorld`): drawn
    /// as its technique set names it (`l_sm_r0c0..` lit, `b1c1` layers with
    /// the vertex colour as their weights, `t0` alpha tested, `b0` blended),
    /// with its colour map and its layers' (`colorMap1`, `colorMap2`).
    /// A decal (a surface of the world's decal range) sits one polygon
    /// offset step toward the eye. `None` when the material is not in the
    /// zone.
    pub(super) fn world_material(&mut self, index: usize, max: u32, decal: bool) -> Option<usize> {
        // (World materials keep their own key space apart from models'.)
        const WORLD: usize = 1 << 40;
        if let Some(&i) = self.materials.get(&(index | WORLD)) {
            return Some(i);
        }
        let m = self.view(index)?;
        let name = ghosts_layer_tokens(&m.techset);
        let name = name.as_str();
        let mut draw = T6Draw::from_technique_set(name, m.sort_key, None);
        // (Ghosts' decals are coplanar with the walls and floors they lie
        // on, drawn without depth writes; with no offset they flickered
        // through them.)
        if decal && draw.polygon_offset == 0 {
            draw.polygon_offset = 1;
        }
        // A replacing base layer (`r0c0`) is opaque whatever the sort key.
        let base = name.split('_').find(|t| {
            let b = t.as_bytes();
            b.len() >= 4 && matches!(b[0], b'r' | b'b' | b't') && b[1] == b'0' && (b[2] == b'c' || b[2] == b'h')
        });
        // (A cut-out world material that says it as `atest`, e.g. a fence.)
        if is_alpha_tested(name) && !matches!(draw.blend, T6Blend::ShadowOnly | T6Blend::Sky) {
            draw.alpha_test = true;
            if draw.blend == T6Blend::Blend {
                draw.blend = T6Blend::Opaque;
            }
        }
        match base.map(|t| t.as_bytes()[0]) {
            Some(b'r') if draw.blend == T6Blend::Blend => draw.blend = T6Blend::Opaque,
            Some(b't') => {
                draw.alpha_test = true;
                if draw.blend == T6Blend::Blend {
                    draw.blend = T6Blend::Opaque;
                }
            }
            _ => {}
        }
        draw.anim = iw6_world_anim(&draw, name, &m.name, m.constants.iter().map(|c| (c.name.as_str(), c.literal)));
        let mut textures = Vec::new();
        let colour = m
            .textures
            .iter()
            .find(|t| t.0 == COLOR_MAP_HASH)
            .or_else(|| m.textures.iter().find(|t| t.2 == 2));
        let mut wanted: Vec<(u32, u8, u8, MatImage)> = Vec::new();
        if let Some(t) = colour {
            wanted.push((t.0, t.1, TS_COLOR_MAP, t.3.clone()));
        }
        if draw.layered {
            for (hash, semantic) in super::t6_materials::LAYER_MAPS {
                if let Some(t) = m.textures.iter().find(|t| t.0 == hash) {
                    wanted.push((hash, t.1, semantic, t.3.clone()));
                }
            }
        } else if !draw.unlit && self.world_shine {
            // Its normal and specular maps (gloss in the specular's alpha):
            // the lit world shader's bumps, highlights and reflections.
            for (hash, semantic) in [(NORMAL_MAP_HASH, TS_NORMAL_MAP), (SPECULAR_MAP_HASH, TS_SPECULAR_MAP)] {
                if let Some(t) = m.textures.iter().find(|t| t.0 == hash) {
                    wanted.push((hash, t.1, semantic, t.3.clone()));
                }
            }
        }
        for (hash, sampler_state, semantic, image) in wanted {
            // Layer pictures are colour pictures (sRGB) too.
            let read_as = if semantic == TS_NORMAL_MAP || semantic == TS_SPECULAR_MAP { semantic } else { TS_COLOR_MAP };
            if let Some(slot) = self.image_of(&image, max, read_as) {
                textures.push(MaterialTextureBinding {
                    name_hash: hash,
                    name_start: 0,
                    name_end: 0,
                    sampler_state,
                    semantic,
                    image: Some(slot),
                });
            }
        }
        let index_in_catalog = self.catalog.link_material(AuthoredMaterial {
            name: AssetRef::Real(format!("iw6:{}", m.name.trim_start_matches(','))),
            namespace: AssetNamespace::T6,
            technique_set: AssetRef::default(),
            technique_set_edge: AssetEdge::Absent,
            draw_surf: 0,
            sort_key: m.sort_key,
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
        self.materials.insert(index | WORLD, index_in_catalog);
        Some(index_in_catalog)
    }

    /// bo2mp: a Ghosts map's sky (`sky_<map>_hdr`: a cube, its six faces
    /// one after another in one pak range, BC6H linear light) as a cube
    /// image under a material whose draw is Black Ops II's sky
    /// (`T6Blend::Sky`: the sky pass draws it behind the world).
    pub(super) fn sky_material(&mut self, image_index: usize, max: u32) -> Result<usize, String> {
        let rec: &ImageRec = self.cap.images.get(image_index).ok_or("sky image not in the zone")?;
        let name = rec.name.clone();
        let px = fastfile_iw6::image::image_pixels(rec, self.image, &mut self.paks, max)?;
        let m = link_sky(&mut self.catalog, &format!("iw6:sky:{name}"), &px, self.owner).map_err(|e| format!("{name}: {e}"))?;
        self.decoded += 1;
        Ok(m)
    }

    fn string(&self, index: u32) -> String {
        self.strings
            .get(index as usize)
            .cloned()
            .flatten()
            .unwrap_or_default()
    }

    fn name(&self, id: AssetId) -> String {
        self.cap.name_of(id).trim_start_matches(',').to_owned()
    }

    /// A model's first LOD in Black Ops II's captured form, and the capture
    /// materials its surfaces use.
    pub(super) fn model_ref(&self, m: &XModelRec) -> Option<(asset_t6::XModelRef, Vec<usize>)> {
        let lod = m.lods.first()?;
        let surfs = self.cap.model_surfs.get(lod.surfs?.index as usize)?;
        let material_key = |k: usize| asset_t6::AssetKey {
            ty: fastfile_t6::AssetType::Material,
            index: k,
        };
        let mut keys = Vec::new();
        let mut lod0 = Vec::with_capacity(surfs.surfs.len());
        for (i, s) in surfs.surfs.iter().enumerate() {
            let verts: Vec<u8> = s
                .verts
                .map_or(&[][..], |sp| sp.get(self.image))
                .chunks_exact(32)
                .flat_map(iw6_vertex_to_t6)
                .collect();
            let indices = le_u16s(s.tris.map_or(&[][..], |sp| sp.get(self.image)));
            let blend_words = le_u16s(s.blend_verts.map_or(&[][..], |sp| sp.get(self.image)));
            let (blend_counts, blend) = fold_blend(s.blend_vert_counts, &blend_words);
            let material = m
                .materials
                .get(usize::from(lod.surf_index) + i)
                .copied()
                .flatten()
                .map(|id| id.index as usize);
            keys.extend(material);
            let mut part_bits = [0u32; 6];
            part_bits.copy_from_slice(&s.part_bits[..6]);
            lod0.push(asset_t6::XSurfaceRef {
                verts,
                vert_count: s.vert_count,
                indices,
                vert_lists: s.rigid.clone(),
                flags: 0,
                material: material.map(material_key),
                blend_counts,
                blend,
                part_bits,
            });
        }
        let (mid, half) = m.bounds;
        let bone_names: Vec<String> = m.bone_names.iter().map(|&s| self.string(s)).collect();
        Some((
            asset_t6::XModelRef {
                lighting_origin_offset: [0.0; 3],
                lighting_origin_range: 0.0,
                name: m.name.clone(),
                numsurfs: m.numsurfs,
                materials: m
                    .materials
                    .iter()
                    .map(|k| k.map(|id| material_key(id.index as usize)))
                    .collect(),
                num_bones: m.num_bones,
                num_lods: u16::from(m.num_lods),
                mins: std::array::from_fn(|k| mid[k] - half[k]),
                maxs: std::array::from_fn(|k| mid[k] + half[k]),
                radius: m.radius,
                lod0,
                bone_names,
                num_root_bones: m.num_root_bones,
                parent_list: m.parent_list.clone(),
                quats: m.tag_angles.clone(),
                trans: m.tag_positions.clone(),
                base_mat: m
                    .base_mat
                    .iter()
                    .map(|b| ([b[0], b[1], b[2], b[3]], [b[4], b[5], b[6]]))
                    .collect(),
                part_classification: m.part_classification.clone(),
                bone_info: vec![None; usize::from(m.num_bones)],
                lods: m
                    .lods
                    .iter()
                    .map(|l| (l.dist, l.numsurfs, l.surf_index))
                    .collect(),
                contents: m.contents as u32,
                coll_lod: i16::from(m.coll_lod),
                coll_surfs: Vec::new(),
                phys_preset: None,
            },
            keys,
        ))
    }

    /// bo2mp: a material as the builder reads it: the zone's own, or, for
    /// one the zone only names (`,m/mtl_sz_crate_federation`: no technique
    /// set, no textures here), the shared zones' (`shared`).
    fn view(&self, index: usize) -> Option<MatView> {
        let m: &MaterialRec = self.cap.materials.get(index)?;
        if m.techset.is_none()
            && m.textures.is_empty()
            && let Some(shared) = self.shared.as_ref().and_then(|s| s.material(&m.name))
        {
            return Some(MatView {
                name: m.name.clone(),
                techset: shared.techset.clone(),
                sort_key: shared.sort_key,
                textures: shared.textures.iter().map(|(h, ss, sem, img)| (*h, *ss, *sem, MatImage::Named(img.clone()))).collect(),
                constants: shared.constants.clone(),
            });
        }
        Some(MatView {
            name: m.name.clone(),
            techset: m.techset.map(|t| self.cap.name_of(t).trim_start_matches(',').to_owned()).unwrap_or_default(),
            sort_key: m.sort_key,
            textures: m
                .textures
                .iter()
                .filter_map(|t| Some((t.name_hash, t.sampler_state, t.semantic, MatImage::Zone(t.image?.index as usize))))
                .collect(),
            constants: m.constants.clone(),
        })
    }

    fn image_of(&mut self, image: &MatImage, max: u32, semantic: u8) -> Option<usize> {
        match image {
            MatImage::Zone(i) => self.image(*i, max, semantic),
            MatImage::Named(name) => self.named_image(name, max, semantic),
        }
    }

    /// bo2mp: a picture a shared material names, from the shared zones.
    fn named_image(&mut self, name: &str, max: u32, semantic: u8) -> Option<usize> {
        let key = (fastfile_iw6::shared::image_key(name), max);
        if let Some(slot) = self.named_images.get(&key) {
            return *slot;
        }
        let pixels = match fastfile_iw6::shared::builtin_pixels(name) {
            Some(px) => Ok(px),
            None => match &self.shared {
                Some(shared) => shared.pixels(name, &mut self.paks, max),
                None => Err("no shared zones".to_owned()),
            },
        };
        let rec = self.shared.as_ref().and_then(|s| s.get(name)).cloned().unwrap_or_default();
        let slot = match pixels.and_then(|px| gpu_image(&px, semantic == TS_COLOR_MAP).map(|img| (px, img))) {
            Ok((px, gpu)) => {
                self.decoded += 1;
                self.from_shared += 1;
                Some(self.catalog.link_image(AuthoredImage {
                    namespace: AssetNamespace::Iw6,
                    name: AssetRef::Real(format!("{}#{max}", name.trim_start_matches(','))),
                    map_type: rec.map_type,
                    semantic,
                    category: 0,
                    use_srgb_reads: semantic == TS_COLOR_MAP,
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
                self.image_failures.push(format!("{}: {e}", name.trim_start_matches(',')));
                None
            }
        };
        self.named_images.insert(key, slot);
        slot
    }

    /// One picture into the catalog (`None` when its pixels are not here).
    fn image(&mut self, index: usize, max: u32, semantic: u8) -> Option<usize> {
        if let Some(slot) = self.images.get(&(index, max)) {
            return *slot;
        }
        let rec: &ImageRec = self.cap.images.get(index)?;
        // An image held by another zone (`,name`) or one of the engine's
        // own (`$white`): from the shared zones, or made.
        let pixels = match &self.shared {
            Some(shared) if fastfile_iw6::shared::is_reference(rec) => {
                let px = match fastfile_iw6::shared::builtin_pixels(&rec.name) {
                    Some(px) => Ok(px),
                    None => shared.pixels(&rec.name, &mut self.paks, max),
                };
                if px.is_ok() {
                    self.from_shared += 1;
                }
                px
            }
            _ => fastfile_iw6::image::image_pixels(rec, self.image, &mut self.paks, max),
        };
        let slot = match pixels
            .and_then(|px| gpu_image(&px, semantic == TS_COLOR_MAP).map(|img| (px, img)))
        {
            Ok((px, gpu)) => {
                self.decoded += 1;
                Some(self.catalog.link_image(AuthoredImage {
                    namespace: AssetNamespace::Iw6,
                    name: AssetRef::Real(format!("{}#{max}", rec.name)),
                    map_type: rec.map_type,
                    semantic,
                    category: 0,
                    use_srgb_reads: semantic == TS_COLOR_MAP,
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
                self.image_failures.push(format!("{}: {e}", rec.name));
                None
            }
        };
        self.images.insert((index, max), slot);
        slot
    }

    /// One material into the catalog, with its colour, normal and specular
    /// maps.
    pub(super) fn material(&mut self, index: usize, max: u32) -> Option<usize> {
        if let Some(&i) = self.materials.get(&index) {
            return Some(i);
        }
        let m = self.view(index)?;
        let linked = self.link_view(m, max)?;
        self.materials.insert(index, linked);
        Some(linked)
    }

    /// bo2mp: a material by name: the zone's own, else one a shared zone
    /// holds (`,name` references and effects of the common zones).
    /// `(technique set name, catalog material)`, or `None` when no zone here
    /// holds it.
    pub(super) fn material_named(&mut self, name: &str, max: u32) -> Option<(String, usize, String)> {
        let key = fastfile_iw6::shared::image_key(name);
        let own = self.cap.materials.iter().position(|m| {
            m.techset.is_some() && !m.name.starts_with(',') && m.name.eq_ignore_ascii_case(&key)
        });
        if let Some(index) = own {
            let view = self.view(index)?;
            return Some((view.techset, self.material(index, max)?, view.name));
        }
        let sm = self.shared.as_ref()?.material(name)?.clone();
        // (Shared materials keep a key space apart from the zone's.)
        let id = (1usize << 42) | (key.bytes().fold(0xcbf2_9ce4_8422_2325u64, |h, b| (h ^ u64::from(b)).wrapping_mul(0x100_0000_01b3)) as usize & ((1 << 40) - 1));
        if let Some(&i) = self.materials.get(&id) {
            return Some((sm.techset, i, sm.name));
        }
        let view = MatView {
            name: sm.name.clone(),
            techset: sm.techset.clone(),
            sort_key: sm.sort_key,
            textures: sm.textures.iter().map(|(h, ss, sem, img)| (*h, *ss, *sem, MatImage::Named(img.clone()))).collect(),
            constants: sm.constants.clone(),
        };
        let linked = self.link_view(view, max)?;
        self.materials.insert(id, linked);
        Some((sm.techset, linked, sm.name))
    }

    /// The catalog material of a read view (colour, normal and specular
    /// maps linked).
    fn link_view(&mut self, m: MatView, max: u32) -> Option<usize> {
        let draw = iw6_model_draw(&m.techset, m.sort_key);
        let nocast = is_nocast(&m.techset);
        let mut textures = Vec::new();
        let colour = m
            .textures
            .iter()
            .find(|t| t.0 == COLOR_MAP_HASH)
            .or_else(|| m.textures.iter().find(|t| t.2 == 2));
        let mut slots: Vec<(&(u32, u8, u8, MatImage), u8)> = Vec::new();
        if let Some(t) = colour {
            slots.push((t, TS_COLOR_MAP));
        }
        if !draw.unlit {
            for (hash, semantic) in [
                (NORMAL_MAP_HASH, TS_NORMAL_MAP),
                (SPECULAR_MAP_HASH, TS_SPECULAR_MAP),
            ] {
                if let Some(t) = m.textures.iter().find(|t| t.0 == hash) {
                    slots.push((t, semantic));
                }
            }
        }
        let wanted: Vec<(u32, u8, u8, MatImage)> = slots.iter().map(|(t, s)| (t.0, t.1, *s, t.3.clone())).collect();
        for (hash, sampler_state, semantic, image) in wanted {
            if let Some(slot) = self.image_of(&image, max, semantic) {
                textures.push(MaterialTextureBinding {
                    name_hash: hash,
                    name_start: 0,
                    name_end: 0,
                    sampler_state,
                    semantic,
                    image: Some(slot),
                });
            }
        }
        let index_in_catalog = self.catalog.link_material(AuthoredMaterial {
            name: AssetRef::Real(m.name.trim_start_matches(',').to_owned()),
            namespace: AssetNamespace::Iw6,
            technique_set: AssetRef::default(),
            technique_set_edge: AssetEdge::Absent,
            draw_surf: 0,
            sort_key: m.sort_key,
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
            constants: m
                .constants
                .iter()
                .map(|c| {
                    let mut short = [0u8; 12];
                    for (d, b) in short.iter_mut().zip(c.name.bytes()) {
                        *d = b;
                    }
                    asset_material::MaterialConstant {
                        name_hash: c.name_hash,
                        name: short,
                        literal: c.literal,
                    }
                })
                .collect(),
            zone: self.owner,
            t6_draw: Some(draw),
        });
        if nocast {
            self.nocast.insert(index_in_catalog);
        }
        Some(index_in_catalog)
    }

    /// An animation in Black Ops II's captured form.
    fn anim_ref(&self, a: &XAnimRec) -> Option<asset_t6::XAnimRef> {
        let bytes = |s: Option<fastfile_iw6::walk::Span>| s.map_or(&[][..], |s| s.get(self.image));
        let indices = if a.numframes >= 256 {
            le_u16s(bytes(a.indices))
        } else {
            bytes(a.indices).iter().map(|&b| u16::from(b)).collect()
        };
        Some(asset_t6::XAnimRef {
                delta_quat: Vec::new(),
            name: a.name.trim_start_matches(',').to_owned(),
            numframes: a.numframes,
            framerate: a.framerate,
            looping: a.flags & 1 != 0,
            delta: false,
            delta_trans: Vec::new(),
            bone_count: a.bone_count,
            names: a.names.iter().map(|&s| self.string(s)).collect(),
            notifies: a.notify.iter().map(|&(s, t)| (self.string(s), t)).collect(),
            data_byte: bytes(a.data_byte).to_vec(),
            data_short: le_u16s(bytes(a.data_short)),
            data_int: le_u32s(bytes(a.data_int)),
            random_data_byte: bytes(a.random_data_byte).to_vec(),
            random_data_short: le_u16s(bytes(a.random_data_short)),
            random_data_int: le_u32s(bytes(a.random_data_int)),
            indices,
        })
    }

    /// A gun's (catalog name) own scope and the attachments it offers, its
    /// attachments' models added to the sets built (`first`, `third`).
    /// An override's attachment is a condition as Modern Warfare 3 writes
    /// it (the scope's number, the under-barrel's times 8, an other's bit
    /// from 5); a gun whose sights get no aiming animation that way is
    /// read by its attachments' places in its lists, from 1.
    fn guest_attachments(
        &self,
        w: &fastfile_iw6::walk::WeaponRec,
        first: &mut BTreeSet<u32>,
        third: &mut BTreeSet<u32>,
    ) -> (String, Option<asset_game::GuestAttachment>, Vec<asset_game::GuestAttachment>) {
        use super::guest_attach as ga;
        let n = w.name.to_ascii_lowercase();
        let gun = n.strip_suffix("_mp").unwrap_or(&n);
        let gun = gun.strip_prefix("iw6_").unwrap_or(gun).to_owned();
        let class = bo2_profile::guest::GHOSTS
            .iter()
            .find(|g| g.gun == gun)
            .map_or(ga::GunClass::Rifle, |g| ga::GunClass::from_group(g.group));
        let bones = |m: &XModelRec| -> Vec<String> { m.bone_names.iter().map(|&s| self.string(s)).collect() };
        let model_of = |ids: &[Option<AssetId>]| {
            ids.first()
                .copied()
                .flatten()
                .filter(|id| id.ty == G::XModel)
                .and_then(|id| Some((id.index, self.cap.xmodels.get(id.index as usize)?)))
        };
        let gun_bones = |ids: &[Option<AssetId>]| model_of(ids).map(|(_, m)| bones(m)).unwrap_or_default();
        let (view_bones, world_bones) = (gun_bones(&w.gun_models), gun_bones(&w.world_models));
        // (cond_iw5, place from 1, record)
        let mut listed: Vec<(u32, u32, &fastfile_iw6::walk::AttachmentRec)> = Vec::new();
        let mut place = 0u32;
        for (list, kind) in [(&w.scopes, 0u32), (&w.under_barrels, 1), (&w.others, 2)] {
            for (k, id) in list.iter().enumerate() {
                place += 1;
                let Some(a) = id
                    .filter(|id| id.ty == G::Attachment)
                    .and_then(|id| self.cap.attachments.get(id.index as usize))
                else {
                    continue;
                };
                let cond = u32::from(match kind {
                    0 => super::iw6_attach::scope_code(k as u8 + 1),
                    1 => super::iw6_attach::under_barrel_code(k as u8 + 1),
                    _ => super::iw6_attach::other_code(k as u32),
                });
                listed.push((cond, place, a));
            }
        }
        let overrides = |slot: u32| -> Vec<(usize, String)> {
            w.anim_overrides
                .iter()
                .filter(|o| o.attachment1 == slot && o.attachment2 == 0)
                .filter_map(|o| Some((asset_game::iw6_anim_slot(o.anim_index as usize)?, self.name(o.override_anim?).to_ascii_lowercase())))
                .collect()
        };
        let by_place = listed
            .iter()
            .filter(|(_, _, a)| ga::iw_kind(&a.name, class).is_some_and(|k| k.sight))
            .any(|(cond, place, _)| {
                !overrides(*cond).iter().any(|(s, _)| *s == asset_iw4::size::weap_anim::ADS_UP)
                    && overrides(*place).iter().any(|(s, _)| *s == asset_iw4::size::weap_anim::ADS_UP)
            });
        let mut scope = None;
        let mut offered: Vec<asset_game::GuestAttachment> = Vec::new();
        for (cond, place, a) in listed {
            let own = ga::is_own_scope(&a.name, &gun);
            let kind = ga::iw_kind(&a.name, class);
            if !own && kind.is_none_or(|k| offered.iter().any(|o| o.word == k.word)) {
                continue;
            }
            let fits = |ids: &[Option<AssetId>], on: &[String], set: &mut BTreeSet<u32>| {
                let (index, m) = model_of(ids)?;
                let root = self.string(*m.bone_names.first()?);
                on.iter().any(|b| b.eq_ignore_ascii_case(&root)).then(|| {
                    set.insert(index);
                    m.name.trim_start_matches(',').to_owned()
                })
            };
            let view = fits(&a.view_models, &view_bones, first);
            let world = fits(&a.world_models, &world_bones, third);
            if own {
                scope.get_or_insert_with(|| ga::own_scope(view, world));
                continue;
            }
            let Some(kind) = kind else { continue };
            if kind.sight && view.is_none() {
                continue;
            }
            let slot = if by_place { place } else { cond };
            let by_type = ga::iw6_fire_sound_by_type();
            let sounds = w
                .sound_overrides
                .iter()
                .filter(|(a1, a2, ..)| *a1 == slot && *a2 == 0)
                .flat_map(|(_, _, alias, _, ty)| {
                    let Some(alias) = alias.as_deref() else {
                        return Vec::new();
                    };
                    if by_type {
                        ga::iw6_override_slots(*ty)
                            .iter()
                            .map(|s| (*s, alias.to_owned()))
                            .collect()
                    } else {
                        ga::fire_slot(alias)
                            .map(|s| (s, alias.to_owned()))
                            .into_iter()
                            .collect::<Vec<_>>()
                    }
                })
                .collect();
            let mut offer = ga::guest_attachment(kind, class, view, world, overrides(slot), sounds);
            offer.fx = self.attachment_fx(w, slot);
            offered.push(offer);
        }
        (gun, ga::sniper_scope_or(scope, class), offered)
    }

    /// bo2mp: the effects an attachment (its override condition `slot`)
    /// puts on the gun, by Ghosts' override rule
    /// (`fx_iw4::iw6::weapon::best_override`): per kind, the entry naming
    /// the most attachments on the gun. Empty with `BO2MP_IW6_FX_RULES=0`.
    fn attachment_fx(&self, w: &fastfile_iw6::walk::WeaponRec, slot: u32) -> Vec<(u32, String)> {
        attachment_fx_of(&w.fx_overrides, slot, asset_game::iw6_fx_rules_on(), |id| self.name(id))
    }

    fn weapon_view(&self, w: &fastfile_iw6::walk::WeaponRec) -> Iw6WeaponView {
        let name_of = |id: &Option<AssetId>| id.map(|id| self.name(id)).filter(|n| !n.is_empty());
        let anims = if w.anims.iter().any(Option::is_some) {
            &w.anims
        } else {
            &w.anims_right
        };
        let notetrack_sounds = w
            .notetrack_sound_keys
            .iter()
            .zip(&w.notetrack_sound_values)
            .map(|(&k, &v)| (self.string(k), self.string(v)))
            .filter(|(k, v)| !k.is_empty() && !v.is_empty())
            .collect();
        Iw6WeaponView {
            name: w.name.clone(),
            display_name: w.display_name.clone(),
            complete: w.complete.get(self.image).to_vec(),
            def: w
                .def
                .map(|s| s.get(self.image).to_vec())
                .unwrap_or_default(),
            gun_model: w.gun_models.first().and_then(name_of),
            world_model: w.world_models.first().and_then(name_of),
            xanims: anims
                .iter()
                .map(|a| name_of(a).unwrap_or_default().to_ascii_lowercase())
                .collect(),
            hide_tags: w
                .hide_tags
                .iter()
                .map(|&s| self.string(s))
                .filter(|s| !s.is_empty())
                .collect(),
            alt_weapon_name: w.alt_weapon_name.clone(),
            sounds: w.sounds.clone(),
            fx: w.fx.iter().map(|(o, id)| (*o, self.name(*id))).collect(),
            models: w
                .models
                .iter()
                .map(|(o, id)| (*o, self.name(*id)))
                .collect(),
            materials: w
                .materials
                .iter()
                .map(|(o, id)| (*o, self.name(*id)))
                .collect(),
            notetrack_sounds,
            location_damage: w.location_damage.map(|s| {
                s.get(self.image)
                    .chunks_exact(4)
                    .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
                    .collect()
            }),
            tracer: w.tracer.map(|id| self.name(id)).filter(|n| !n.is_empty()),
        }
    }
}

/// bo2mp: the effects one attachment (override condition `slot`) puts on
/// a gun: per effect kind 1..8 the best-scoring override entry's effect.
fn attachment_fx_of(
    overrides: &[fastfile_iw6::walk::FxOverride],
    slot: u32,
    on: bool,
    name: impl Fn(AssetId) -> String,
) -> Vec<(u32, String)> {
    use fx_iw4::iw6::weapon::{OverrideKey, best_override};
    if !on || slot == 0 {
        return Vec::new();
    }
    let keys: Vec<OverrideKey> = overrides
        .iter()
        .map(|o| OverrideKey { attachment1: o.attachment1, attachment2: o.attachment2, fx_type: o.fx_type })
        .collect();
    (1..=8)
        .filter_map(|t| {
            let i = best_override(keys.iter().copied(), t, &[slot])?;
            let n = name(overrides[i].override_fx?);
            (!n.is_empty()).then_some((t, n))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn world_materials_animate_by_name_and_scroll_constant() {
        use asset_core::T6Anim;
        let lit = T6Draw::from_technique_set("lit_sm_r0c0", 0, None);
        let none = || std::iter::empty::<(&str, [f32; 4])>();
        // Plain surfaces stand still.
        assert_eq!(iw6_world_anim(&lit, "lit_sm_r0c0", "wall_brick", none()), T6Anim::None);
        // A flag technique waves.
        let flag = T6Draw::from_technique_set("lit_flag_r0c0", 0, None);
        assert_eq!(iw6_world_anim(&flag, "lit_flag_r0c0", "flag_us", none()), T6Anim::Cloth);
        // A lit surface with a slow uv scroll flows; a fast one falls.
        let slow = [("uvAnimParms", [0.1, 0.0, 0.0, 0.0])];
        let fast = [("uvAnimParms", [0.0, 1.5, 0.0, 0.0])];
        assert_eq!(
            iw6_world_anim(&lit, "lit_sm_r0c0", "wall_brick", slow.into_iter()),
            T6Anim::Flow
        );
        assert_eq!(
            iw6_world_anim(&lit, "lit_sm_r0c0", "wall_brick", fast.into_iter()),
            T6Anim::Fall
        );
        // Effects never move this way.
        let mut fx = lit.clone();
        fx.effect = true;
        assert_eq!(iw6_world_anim(&fx, "lit_sm_r0c0", "wall_brick", slow.into_iter()), T6Anim::None);
    }

    #[test]
    fn normals_halve_into_bo2_range() {
        // Signed +127 / -127 endpoints (6-value mode) -> 191 / 65.
        let mut block = [127u8, 0x81, 1, 2, 3, 4, 5, 6, 127, 0x81, 0, 0, 0, 0, 0, 0];
        bc5_normals_for_bo2(&mut block, true);
        assert_eq!((block[0], block[1], block[2]), (191, 65, 1));
        // Endpoints that meet: one value.
        let mut flat = [3u8, 2, 9, 9, 9, 9, 9, 9, 0, 0, 0, 0, 0, 0, 0, 0];
        bc5_normals_for_bo2(&mut flat, true);
        assert_eq!(&flat[0..8], &[129, 129, 0, 0, 0, 0, 0, 0]);
        // Unsigned 255 / 0 -> 191 / 64.
        let mut u = [255u8, 0, 0, 0, 0, 0, 0, 0, 255, 0, 0, 0, 0, 0, 0, 0];
        bc5_normals_for_bo2(&mut u, false);
        assert_eq!((u[0], u[1]), (191, 64));
    }

    #[test]
    fn unit_vectors_repack() {
        // +x: 1023 unsigned -> 511 signed; -1 (0) -> -511 (513).
        assert_eq!(iw6_unit_vec_to_t6(1023) & 0x3ff, 511);
        assert_eq!(iw6_unit_vec_to_t6(0) & 0x3ff, 513);
        let mid = iw6_unit_vec_to_t6(512 << 20) >> 20;
        assert!(mid <= 1 || mid >= 1023);
    }

    #[test]
    fn blend_folds_into_four_buckets() {
        // One vertex with five influences: 9 words, keeps 7.
        let mut counts = [0u16; 8];
        counts[4] = 1;
        let words: Vec<u16> = (0..9).collect();
        let (c, w) = fold_blend(counts, &words);
        assert_eq!(c, [0, 0, 0, 1]);
        assert_eq!(w, vec![0, 1, 2, 3, 4, 5, 6]);
    }
}

/// bo2mp: a sky cube (Ghosts' and Infinite Warfare's `sky_<map>`: six
/// faces one after another, each with its levels, BC6H linear light) as a
/// cube image under a material whose draw is Black Ops II's sky
/// (`T6Blend::Sky`: the sky pass draws the first such material's colour
/// map behind the world). The catalog material's index.
pub(super) fn link_sky(
    catalog: &mut MaterialCatalog,
    name: &str,
    px: &fastfile_iw6::image::ImagePixels,
    owner: ZoneOwner,
) -> Result<usize, String> {
    use bevy::render::render_resource::{TextureViewDescriptor, TextureViewDimension};
    let mut face = 0usize;
    let (mut w, mut h) = (px.width, px.height);
    for _ in 0..px.levels.max(1) {
        face += fastfile_iw6::image::level_bytes(px.dxgi_format, w, h).ok_or("sky format not read")?;
        w = (w / 2).max(1);
        h = (h / 2).max(1);
    }
    if px.width != px.height || px.data.len() != 6 * face {
        return Err(format!("{}x{}, {} bytes ({} wanted for six faces)", px.width, px.height, px.data.len(), 6 * face));
    }
    // bo2mp: Modern Warfare Remastered's sky shader (`sky_cso_hss`) reads
    // its cube as stored (a UNORM picture, not decoded from sRGB), times its
    // material's scale: an H1 sky in a UNORM format is sampled raw there
    // (`BO2MP_H1_SKY_RAW=0`: decoded as before).
    let raw = name.starts_with("h1:")
        && std::env::var("BO2MP_H1_MWR").map_or(true, |v| v.trim() != "0")
        && std::env::var("BO2MP_H1_SKY_RAW").map_or(true, |v| v.trim() != "0");
    let format = match px.dxgi_format {
        27 | 28 if raw => TextureFormat::Rgba8Unorm,
        70 | 71 if raw => TextureFormat::Bc1RgbaUnorm,
        76 | 77 if raw => TextureFormat::Bc3RgbaUnorm,
        97 | 98 if raw => TextureFormat::Bc7RgbaUnorm,
        95 => TextureFormat::Bc6hRgbUfloat,
        96 => TextureFormat::Bc6hRgbFloat,
        70..=72 => TextureFormat::Bc1RgbaUnormSrgb,
        76..=78 => TextureFormat::Bc3RgbaUnormSrgb,
        97..=99 => TextureFormat::Bc7RgbaUnormSrgb,
        27..=29 => TextureFormat::Rgba8UnormSrgb,
        other => return Err(format!("DXGI format {other} is not drawn as a sky")),
    };
    let mut image = Image::new_uninit(
        Extent3d { width: px.width, height: px.height, depth_or_array_layers: 6 },
        TextureDimension::D2,
        format,
        RenderAssetUsages::RENDER_WORLD,
    );
    image.texture_descriptor.mip_level_count = px.levels.max(1);
    image.data = Some(px.data.clone());
    image.texture_view_descriptor = Some(TextureViewDescriptor { dimension: Some(TextureViewDimension::Cube), ..Default::default() });
    image.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
        address_mode_u: ImageAddressMode::ClampToEdge,
        address_mode_v: ImageAddressMode::ClampToEdge,
        address_mode_w: ImageAddressMode::ClampToEdge,
        mag_filter: ImageFilterMode::Linear,
        min_filter: ImageFilterMode::Linear,
        mipmap_filter: ImageFilterMode::Linear,
        ..Default::default()
    });
    let slot = catalog.link_image(AuthoredImage {
        namespace: AssetNamespace::T6,
        name: AssetRef::Real(name.to_owned()),
        map_type: 5,
        semantic: TS_COLOR_MAP,
        category: 0,
        use_srgb_reads: false,
        width: px.width as u16,
        height: px.height as u16,
        depth: 1,
        level_count: px.levels as u8,
        format: 0,
        payload: Arc::new(Vec::new()),
        decoded: Some(Arc::new(image)),
        common_owned: false,
        decoded_variant: None,
        decoded_by: None,
        pending_decode: None,
    });
    Ok(catalog.link_material(AuthoredMaterial {
        name: AssetRef::Real(name.to_owned()),
        namespace: AssetNamespace::T6,
        technique_set: AssetRef::default(),
        technique_set_edge: AssetEdge::Absent,
        draw_surf: 0,
        sort_key: 0,
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
        textures: vec![MaterialTextureBinding {
            name_hash: COLOR_MAP_HASH,
            name_start: 0,
            name_end: 0,
            sampler_state: 0x13,
            semantic: TS_COLOR_MAP,
            image: Some(slot),
        }],
        constants: Vec::new(),
        zone: owner,
        t6_draw: Some(T6Draw::from_technique_set("wc_sky", 0, None)),
    }))
}
