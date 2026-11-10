use super::*;

static NEXT_COMMON_PROFILE_ID: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CommonKey {
    pub(super) runtime: Option<PathBuf>,
    pub(super) foreign: Option<PathBuf>,
    /// The games whose guest guns the set holds (`bo2_profile::guest::match_games`).
    pub(super) guests: u32,
}

impl CommonKey {
    pub(super) fn for_match(
        zone_ff: Option<&Path>,
        runtime: Option<&Path>,
        report: &mut Vec<String>,
    ) -> Self {
        crate::direct_start_guest_games();
        Self {
            runtime: runtime.map(Path::to_path_buf),
            foreign: resolve_foreign_material_donor(zone_ff, runtime, report),
            guests: bo2_profile::guest::match_games(),
        }
    }

    fn shell(games: &asset_transport::GamesRoot, report: &mut Vec<String>) -> Self {
        let runtime = match find_common_mp_for_envelope(games, fastfile_iw4::ZONE_VERSION_PC) {
            Ok(found) => Some(found.path),
            Err(error) => {
                report.push(format!("CAC iw4: {error}"));
                None
            }
        };
        // (Black Ops II's menus list the guest guns from the games found
        // installed, `ui::foreign_maps`: none are read for them.)
        Self {
            runtime,
            foreign: None,
            guests: if crate::t6_rules() { 0 } else { u32::MAX },
        }
    }
}

impl std::fmt::Display for CommonKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.runtime {
            Some(path) => write!(f, "{}", path.display())?,
            None => f.write_str("<no runtime common_mp>")?,
        }
        if let Some(foreign) = &self.foreign {
            write!(f, " + {}", foreign.display())?;
        }
        if self.guests != u32::MAX {
            write!(f, " (guest games {:#x})", self.guests)?;
        }
        Ok(())
    }
}

#[derive(Clone, Copy)]
pub(super) struct CommonCounts {
    pub(super) startup_count: usize,
    pub(super) t5_mat_count: usize,
    pub(super) t5_reuse_mat: usize,
    pub(super) t5_xanim_n: usize,
    pub(super) foreign_count: usize,
    pub(super) seed_mat: usize,
    pub(super) seed_img: usize,
    pub(super) common_reuse_mat: usize,
    pub(super) common_reuse_img: usize,
}

#[derive(Clone)]
pub(super) struct CommonProducts {
    pub(super) scripts: crate::ScriptSources,
    pub(super) t5_scene_models: asset_world::MapXModelSceneCatalog,
    pub(super) material_seed: MaterialCatalog,
    pub(super) shared_surfaces: asset_model::SharedXModelSurfaces,
    pub(super) scene_models: asset_world::MapXModelSceneCatalog,
    pub(super) light_defs: Vec<asset_world::CapturedLightDef>,
    pub(super) pen_table: weapon_iw4::PenetrationDepthTable,
    pub(super) pen_table_loaded: bool,
    pub(super) lochit_table: Option<[f32; weapon_iw4::HITLOC_COUNT]>,
    pub(super) tracers: asset_game::TracerCatalog,
    pub(super) xmodel_walk: crate::PreparedXModelWalkCensus,
    pub(super) s1_common_bytes: usize,
    pub(super) teamsets: std::collections::HashMap<String, asset_game::MapTeamSettings>,
    pub(super) film_visions: std::collections::BTreeMap<
        String,
        Result<asset_world::FilmVision, asset_world::FilmVisionParseError>,
    >,
    pub(super) weapons: WeaponBuild,
    pub(super) fpv_meshes: FpvMeshBuild,
    pub(super) world_weapons: WorldWeaponBuild,
    pub(super) projectile_meshes: ProjectileMeshBuild,
    pub(super) xanims: XAnimBuild,
    pub(super) player_anim_sources: asset_anim::PlayerAnimSources,
    pub(super) fx: FxCatalog,
    pub(super) fx_models: asset_game::FxModelCatalog,
    pub(super) impact_fx: Option<asset_game::OwnedFxImpactTable>,
    pub(super) t5_xanims: XAnimBuild,
    pub(super) t5_fx: FxCatalog,
    pub(super) t5_impact_fx: Option<asset_game::OwnedFxImpactTable>,
    pub(super) iw5_materials: MaterialCatalog,
    pub(super) iw5_scene_models: asset_world::MapXModelSceneCatalog,
    pub(super) iw5_shared_surfaces: asset_model::SharedXModelSurfaces,
    pub(super) strings: LocalizeCatalog,
    pub(super) counts: CommonCounts,
    pub(super) report: Vec<String>,
    pub(super) localize_report: Vec<String>,
}

pub(super) struct KeptImages {
    pub(super) label: &'static str,
    pub(super) namespace: &'static str,
    pub(super) batch: asset_material::material_images::DecodedImageBatch,
    pub(super) job: load_jobs::Job,
}

pub struct CommonSet {
    pub(super) id: u64,
    pub(super) key: CommonKey,
    /// (Behind a lock only so `release_cpu_images` can swap the card-held
    /// pictures for pixel-less ones; everything else reads.)
    pub(super) products: std::sync::RwLock<CommonProducts>,
    donor_images: async_lock::OnceCell<Vec<KeptImages>>,
    /// Its pictures' pixels were let go (`release_cpu_images`).
    stripped: std::sync::atomic::AtomicBool,
    pub(super) fpv_plan: Option<ImageDemandPlan>,
    pub(super) retained: std::sync::Mutex<asset_material::material_images::PayloadRetention>,
    pub(super) cac_tables: Vec<(asset_core::AssetNamespace, asset_game::CapturedStringTable)>,
    pub(super) prepared_ms: f32,
    pub(super) ready_at: std::time::Instant,
}

impl CommonSet {
    pub(super) async fn donor_images(&self) -> &[KeptImages] {
        self.donor_images.wait().await
    }

    pub(super) fn donor_batches(&self) -> usize {
        self.donor_images.get().map_or(0, Vec::len)
    }

    pub(super) fn retain(&self, batch: &asset_material::material_images::DecodedImageBatch) -> u64 {
        self.retained
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .keep(batch)
    }

    pub(super) fn retained_bytes(&self) -> u64 {
        self.retained
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .bytes()
    }

    pub(super) fn retained_payloads(&self) -> usize {
        self.retained
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .payloads()
    }
}

struct CommonFlight {
    pub(super) key: CommonKey,
    set: async_lock::OnceCell<Option<Arc<CommonSet>>>,
}

static COMMON: std::sync::Mutex<Option<Arc<CommonFlight>>> = std::sync::Mutex::new(None);

/// lane B (map to map): the set a match of another game replaced, kept for
/// that game's next map. MW2's maps, MW3's maps and every other game's each
/// key their own set; Terminal's took 31.8 s, and a hop to Skyjacked threw it
/// away, so the next MW3 map walked it all again. One is kept at a time, only
/// when it took `PARK_MIN_MS` to make and holds at most
/// `CODLEGACY_PARK_COMMON_MIB` (2048 by default) of payloads and pixels.
/// `CODLEGACY_PARK_COMMON=0` turns this off.
static PARKED: std::sync::Mutex<Option<Arc<CommonFlight>>> = std::sync::Mutex::new(None);

const PARK_MIN_MS: f32 = 2000.0;

fn parks(prepared_ms: f32, held: u64, cap: u64) -> bool {
    prepared_ms >= PARK_MIN_MS && held <= cap
}

fn worth_parking(flight: &CommonFlight) -> bool {
    if std::env::var("CODLEGACY_PARK_COMMON").is_ok_and(|v| v.trim() == "0") {
        return false;
    }
    let Some(Some(set)) = flight.set.get() else {
        return false;
    };
    let cap_mib = std::env::var("CODLEGACY_PARK_COMMON_MIB")
        .ok()
        .and_then(|v| v.trim().parse::<u64>().ok())
        .unwrap_or(2048);
    parks(set.prepared_ms, set.retained_bytes() + set.decoded_bytes(), cap_mib << 20)
}

/// The set kept for another game's next map (the memory census).
pub(super) fn parked_common() -> Option<Arc<CommonSet>> {
    let parked = PARKED.lock().unwrap_or_else(|poison| poison.into_inner());
    parked.as_ref()?.set.get().cloned().flatten()
}

struct FlightGuard(Option<Arc<CommonFlight>>);

impl Drop for FlightGuard {
    fn drop(&mut self) {
        let Some(flight) = self.0.take() else {
            return;
        };
        let mut slot = COMMON.lock().unwrap_or_else(|poison| poison.into_inner());
        if slot
            .as_ref()
            .is_some_and(|current| Arc::ptr_eq(current, &flight))
        {
            *slot = None;
        }
        drop(slot);
        let _ = flight.set.set_blocking(None);
    }
}

/// The common set in the slot now, whatever its key (the memory census).
pub(super) fn current_common() -> Option<Arc<CommonSet>> {
    let slot = COMMON.lock().unwrap_or_else(|poison| poison.into_inner());
    slot.as_ref()?.set.get().cloned().flatten()
}

impl CommonSet {
    /// The decoded pixels its two material catalogs hold (the census).
    pub(super) fn decoded_bytes(&self) -> u64 {
        let products = self.products();
        [&products.material_seed, &products.iw5_materials]
            .into_iter()
            .flat_map(|catalog| catalog.images.iter())
            .filter_map(|image| image.decoded.as_ref()?.data.as_ref().map(|data| data.len() as u64))
            .sum()
    }

    pub(super) fn products(&self) -> std::sync::RwLockReadGuard<'_, CommonProducts> {
        self.products.read().unwrap_or_else(|poison| poison.into_inner())
    }

    /// bo2mp: lets go of the pixels of the common pictures the card already
    /// holds. A later map with this common set takes the card's texture for
    /// them (`ResidentGpuImages`, keyed by this set's id and the picture's
    /// variant); only the pixels go, so a picture's size and format, and
    /// that it was decoded, still read the same. `live` says which variants
    /// have a texture on the card now; the others keep their pixels.
    /// Returns how many pictures and the bytes actually freed (a picture
    /// another holder shares stays until that one lets go too).
    fn release_cpu_images(&self, live: &dyn Fn(asset_material::ImageVariantId) -> bool) -> (usize, u64) {
        let mut products = self.products.write().unwrap_or_else(|poison| poison.into_inner());
        let products = &mut *products;
        let (mut count, mut freed) = (0, 0u64);
        for catalog in [&mut products.material_seed, &mut products.iw5_materials] {
            for image in &mut catalog.images {
                let (Some(decoded), Some(variant)) = (image.decoded.as_mut(), image.decoded_variant) else {
                    continue;
                };
                if !image.common_owned || decoded.data.is_none() || !live(variant) {
                    continue;
                }
                let bare = bevy::image::Image {
                    data: None,
                    data_order: decoded.data_order,
                    texture_descriptor: decoded.texture_descriptor.clone(),
                    sampler: decoded.sampler.clone(),
                    texture_view_descriptor: decoded.texture_view_descriptor.clone(),
                    asset_usage: decoded.asset_usage,
                    copy_on_resize: decoded.copy_on_resize,
                };
                let old = std::mem::replace(decoded, Arc::new(bare));
                count += 1;
                if Arc::strong_count(&old) == 1 {
                    freed += old.data.as_ref().map_or(0, |data| data.len() as u64);
                }
            }
        }
        if count > 0 {
            self.stripped.store(true, std::sync::atomic::Ordering::Release);
        }
        (count, freed)
    }
}

/// bo2mp: once a map's pictures are on the card, the common set `profile_id`
/// lets go of the pixels of its pictures the card holds
/// (`CommonSet::release_cpu_images`): before this they stayed in memory for
/// as long as the common set did, across every map that shared it.
/// `BO2MP_COMMON_CPU_STRIP=1` turns it on (off until a run shows no missing
/// texture on the map after). Returns the pictures and bytes let go.
pub fn release_common_cpu_images(
    profile_id: u64,
    live: &dyn Fn(asset_material::ImageVariantId) -> bool,
) -> Option<(usize, u64)> {
    if std::env::var_os("BO2MP_COMMON_CPU_STRIP").is_none_or(|v| v != "1") {
        return None;
    }
    let common = current_common().filter(|common| common.id == profile_id)?;
    let (count, freed) = common.release_cpu_images(live);
    if count == 0 {
        return None;
    }
    // The pixels were made on the loader threads; their pages go back there.
    super::release_pool_heaps();
    Some((count, freed))
}

/// bo2mp: the common picture `variant` of the set `profile_id` built again
/// from the kept payloads, for a picture whose pixels were let go
/// (`release_common_cpu_images`) and whose card texture is gone too.
/// `None` when its payload was not kept (over `IW4L_COMMON_KEEP_MIB`).
pub fn rebuild_common_image(profile_id: u64, variant: asset_material::ImageVariantId) -> Option<bevy::image::Image> {
    current_common()
        .filter(|common| common.id == profile_id)
        .and_then(|common| {
            let rebuilt = common
                .retained
                .lock()
                .unwrap_or_else(|poison| poison.into_inner())
                .rebuild(variant);
            rebuilt
        })
        // A Black Ops II shared picture kept on the card whose texture is
        // gone: decoded again from the packs.
        .or_else(|| crate::lane::rebuild_card_image(variant))
}

/// bo2mp: a picture of the common set `profile_id` lost its pixels
/// (`release_common_cpu_images`), its card texture is gone and its payload
/// was not kept, so the map being drawn cannot have it. The set is dropped
/// from the slot so the next load prepares it again with every picture
/// decoded; one log line.
pub fn forget_stripped_common(profile_id: u64) {
    let mut slot = COMMON.lock().unwrap_or_else(|poison| poison.into_inner());
    let Some(flight) = slot.as_ref() else {
        return;
    };
    let Some(Some(set)) = flight.set.get() else {
        return;
    };
    if set.id != profile_id || !set.stripped.load(std::sync::atomic::Ordering::Acquire) {
        return;
    }
    let key = flight.key.clone();
    *slot = None;
    drop(slot);
    diag::info!(
        World,
        "common set: {key} let go: a picture whose pixels were let go has no texture on the card any more (the next load prepares it again)"
    );
}

pub(super) fn landed_common(key: &CommonKey) -> Option<Arc<CommonSet>> {
    let slot = COMMON.lock().unwrap_or_else(|poison| poison.into_inner());
    let flight = slot.as_ref().filter(|flight| flight.key == *key)?;
    flight.set.get().cloned().flatten()
}

/// bo2mp (bake all): readies the common set a load of `zone_ff` takes, as the
/// lobby does before a map loads, so a map's baked copy holds its own
/// pictures only. Says whether it was made or reused.
pub async fn ready_match_common(zone_ff: &Path, common_mp: Option<&Path>) -> &'static str {
    ensure_common(CommonKey::for_match(Some(zone_ff), common_mp, &mut Vec::new())).await.1
}

pub(super) async fn ensure_common(key: CommonKey) -> (Arc<CommonSet>, &'static str) {
    let (flight, reach) = {
        let mut slot = COMMON.lock().unwrap_or_else(|poison| poison.into_inner());
        match slot.as_ref() {
            Some(flight) if flight.key == key => {
                let reach = if flight.set.get().is_some() {
                    "reused"
                } else {
                    "joined in flight"
                };
                (Arc::clone(flight), reach)
            }
            _ => {
                let mut parked = PARKED.lock().unwrap_or_else(|poison| poison.into_inner());
                // (Only a landed set is parked.)
                if let Some(back) = parked.take_if(|flight| flight.key == key) {
                    if let Some(previous) = slot.replace(Arc::clone(&back))
                        && worth_parking(&previous)
                    {
                        diag::info!(World, "common set: {} kept for its game's next map", previous.key);
                        *parked = Some(previous);
                    }
                    drop(parked);
                    diag::info!(World, "common set: kept from an earlier map for {key}, not walked again");
                    return match back.set.get() {
                        Some(Some(set)) => (Arc::clone(set), "kept from an earlier map"),
                        _ => panic!("common set for {key} was parked unprepared"),
                    };
                }
                let flight = Arc::new(CommonFlight {
                    key: key.clone(),
                    set: async_lock::OnceCell::new(),
                });
                if let Some(previous) = slot.replace(Arc::clone(&flight)) {
                    diag::info!(World, "common set: {} replaced by {key}", previous.key);
                    if worth_parking(&previous) {
                        diag::info!(World, "common set: {} kept for its game's next map", previous.key);
                        *parked = Some(previous);
                    }
                }
                drop(parked);
                let running = Arc::clone(&flight);
                load_pool()
                    .spawn(async move {
                        let mut guard = FlightGuard(Some(running));
                        let set = prepare_common(key).await;
                        // The zone and decode buffers the walks freed go back
                        // before the map's walk adds its own.
                        super::release_pool_heaps();
                        if let Some(flight) = guard.0.take() {
                            let _ = flight.set.set(Some(set)).await;
                        }
                    })
                    .detach();
                (flight, "prepared")
            }
        }
    };
    diag::info!(World, "common set: {reach} for {}", flight.key);
    let waiting = std::time::Instant::now();
    match flight.set.wait().await {
        Some(set) => {
            // bo2mp: a long wait names itself in the log.
            let waited = waiting.elapsed().as_secs_f32();
            if waited >= 1.0 {
                diag::info!(World, "common set: waited {waited:.1} s on {} ({reach})", flight.key);
            }
            (Arc::clone(set), reach)
        }
        None => panic!("common set for {} was not prepared", flight.key),
    }
}

pub struct ShellCommon {
    pub weapons: WeaponRegistry,
    pub strings: LocalizeCatalog,
    pub tables: Vec<(asset_core::AssetNamespace, asset_game::CapturedStringTable)>,
    pub report: Vec<String>,
}

pub async fn load_shell_common(games: asset_transport::GamesRoot) -> ShellCommon {
    let mut report = Vec::new();
    let key = CommonKey::shell(&games, &mut report);
    let (common, reach) = ensure_common(key).await;
    let weapons = common.products().weapons.clone().publish();
    report.push(format!(
        "CAC: {reach} common set {}; weapons={} (iw4={} iw5={} t5={}) tables={}",
        common.key,
        weapons.len(),
        weapons.namespace_count(asset_core::AssetNamespace::Iw4),
        weapons.namespace_count(asset_core::AssetNamespace::Iw5),
        weapons.namespace_count(asset_core::AssetNamespace::T5),
        common.cac_tables.len(),
    ));
    for (namespace, table) in &common.cac_tables {
        report.push(format!(
            "CAC {}: {} rows={}",
            namespace.as_str(),
            table.name,
            table.rows
        ));
    }
    ShellCommon {
        weapons,
        strings: common.products().strings.clone(),
        tables: common.cac_tables.clone(),
        report,
    }
}

/// bo2mp: the common set a match of any game but Modern Warfare 2's and
/// Black Ops II's asks for (no runtime common_mp, no foreign donor: every
/// Ghosts, Remastered, Black Ops III and Infinite Warfare map), made while
/// the menus are up instead of after "start". It was 47 s of the 100 s a
/// first match took (every game's guns walked once); the match now joins it
/// in flight or finds it done. It runs after the menu's own set, so the two
/// do not contend. Costs the same background work and memory the first
/// match would, earlier.
static PREFETCH_SINCE: std::sync::Mutex<Option<std::time::Instant>> = std::sync::Mutex::new(None);

/// Whole seconds the menu-time preload has been running, `None` when it is
/// not (never started, switched off, or done): the front end shows it.
pub fn prefetch_status() -> Option<u32> {
    let since = (*PREFETCH_SINCE.lock().ok()?)?;
    Some(since.elapsed().as_secs() as u32)
}

pub fn prefetch_match_common(games: asset_transport::GamesRoot) {
    if std::env::var("BO2MP_PREFETCH_COMMON").is_ok_and(|v| v.trim() == "0") {
        return;
    }
    // bo2mp: under Black Ops II's rules it waits for the lobby
    // (`prefetch_lobby_common`), which knows his classes: made at launch,
    // with every game's guns, it kept the HDD busy through his matches.
    if crate::t6_rules() {
        PREFETCH_ARMED.store(true, std::sync::atomic::Ordering::Relaxed);
        return;
    }
    if let Ok(mut since) = PREFETCH_SINCE.lock() {
        *since = Some(std::time::Instant::now());
    }
    load_pool()
        .spawn(async move {
            let mut report = Vec::new();
            let shell = CommonKey::shell(&games, &mut report);
            let _ = ensure_common(shell).await;
            let key = CommonKey { runtime: None, foreign: None, guests: bo2_profile::guest::match_games() };
            diag::info!(World, "common set: prefetching {key} while the menus are up");
            let _ = ensure_common(key).await;
            if let Ok(mut since) = PREFETCH_SINCE.lock() {
                *since = None;
            }
        })
        .detach();
}

static PREFETCH_ARMED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// bo2mp: the lobby picked a map: the common set a Black Ops II match (and
/// every other game's but Modern Warfare 2's) asks for is made now, while
/// the menus are up, with the guest guns his classes hold. Once per set of
/// guest games; nothing before `prefetch_match_common`.
pub(crate) fn prefetch_lobby_common() {
    static STARTED: std::sync::Mutex<Option<u32>> = std::sync::Mutex::new(None);
    if !PREFETCH_ARMED.load(std::sync::atomic::Ordering::Relaxed) {
        return;
    }
    let guests = bo2_profile::guest::match_games();
    let Ok(mut started) = STARTED.lock() else {
        return;
    };
    if *started == Some(guests) {
        return;
    }
    *started = Some(guests);
    if let Ok(mut since) = PREFETCH_SINCE.lock() {
        *since = Some(std::time::Instant::now());
    }
    load_pool()
        .spawn(async move {
            let key = CommonKey { runtime: None, foreign: None, guests };
            diag::info!(World, "common set: prefetching {key} for the lobby's match");
            let _ = ensure_common(key).await;
            if let Ok(mut since) = PREFETCH_SINCE.lock() {
                *since = None;
            }
        })
        .detach();
}

async fn prepare_common(key: CommonKey) -> Arc<CommonSet> {
    let started = std::time::Instant::now();
    let progress = LoadProgress::default();
    let pool = load_pool();
    let anchor = key.runtime.clone();
    // bo2mp: a game's guest guns are read only when the set holds them (the
    // games his classes use).
    let guests = key.guests;
    let wants = move |game: &str| {
        crate::t6_rules() && crate::guest_guns() && guests & bo2_profile::guest::game_bit(game) != 0
    };

    let startup_walk = {
        let anchor = anchor.clone();
        let progress = progress.clone();
        pool.spawn(async move { walk_startup_material_zones(anchor.as_ref(), &progress).await })
    };

    let mut donor_report = Vec::new();
    let material_donor = key.foreign.clone();
    let weapon_donor = resolve_iw5_weapon_donor(anchor.as_deref(), &mut donor_report);
    // bo2mp: under Black Ops II's rules MW3's guns join only as guest guns
    // (`mw3_acr_mp+reflex`, on with the other games' guest guns); with
    // `BO2MP_GUEST_GUNS=0` MW3's common zone is walked only when an MW3
    // map needs its materials.
    let weapon_donor = if crate::t6_rules() && !wants("iw5") {
        let kept = weapon_donor.filter(|donor| material_donor.as_ref() == Some(donor));
        if kept.is_none() {
            donor_report.push("iw5 weapons: skip, Black Ops II rules (its own guns)".into());
        }
        kept
    } else {
        weapon_donor
    };
    let iw5_startup_walk = weapon_donor.clone().map(|donor| {
        let progress = progress.clone();
        pool.spawn(async move { walk_iw5_startup(&donor, &progress) })
    });
    let shared_donor = match (&material_donor, &weapon_donor) {
        (Some(material), Some(weapons)) if material == weapons => Some(material.clone()),
        _ => None,
    };
    let (data_common_walk, mut iw5_weapon_walk) = match shared_donor {
        Some(donor) => {
            let progress = progress.clone();
            let job = load_jobs::open(JobKind::ImageDecode).namespace("iw5");
            (
                ForeignCommonWork::Shared(
                    pool.spawn(async move { walk_shared_iw5_common(&donor, &progress, job) }),
                ),
                None,
            )
        }
        None => {
            let material = material_donor.map(|donor| {
                let progress = progress.clone();
                let job = load_jobs::open(JobKind::ImageDecode).namespace("iw5");
                pool.spawn(async move { walk_foreign_material_common(&donor, &progress, job) })
            });
            let weapons = weapon_donor.map(|donor| {
                let progress = progress.clone();
                let job = load_jobs::open(JobKind::ImageDecode).namespace("iw5");
                pool.spawn(async move { walk_iw5_weapon_bundle(&donor, &progress, job) })
            });
            (ForeignCommonWork::Split(material), weapons)
        }
    };

    let common_open = {
        let anchor = anchor.clone();
        let progress = progress.clone();
        pool.spawn(async move {
            let Some(path) = anchor else {
                progress.record_skipped_scoped(StageId::CommonAssets, "common_mp");
                return None;
            };
            let stage = progress.begin_scoped(StageId::CommonAssets, "common_mp", None);
            let opened = open_zone_shared(&path);
            finish_zone_open(stage, &opened);
            Some((path, opened))
        })
    };

    // bo2mp: Black Ops' guns join Black Ops II's as extra weapons (listed
    // as `bo1_<gun>` in Create-a-Class), so its common zone is walked for
    // every match; `BO2MP_GUEST_GUNS=0` leaves them out (then only a Black
    // Ops map walks it, for its own materials).
    let t5_map = key
        .foreign
        .as_deref()
        .and_then(peek_zone_version)
        .is_some_and(|v| v == fastfile_t5::ZONE_VERSION_PC);
    let skip_t5 = crate::t6_rules() && !t5_map && !wants("t5");
    let t5_common_prep = {
        let anchor = anchor.clone();
        let progress = progress.clone();
        pool.spawn(async move {
            if skip_t5 {
                return T5CommonPrep::Skip(vec!["t5 weapon common: skip, Black Ops II rules and no Black Ops guest guns this match".into()]);
            }
            t5_weapon_common_prep(anchor.as_deref(), &progress)
        })
    };
    // bo2mp: MW2's guns as extra weapons, when the map does not already
    // bring MW2's common zone.
    let iw4_guest_prep = {
        let anchor = anchor.clone();
        let progress = progress.clone();
        let wanted = wants("iw4");
        pool.spawn(async move {
            if !wanted {
                return T5CommonPrep::Skip(vec!["iw4 guest guns: off".into()]);
            }
            iw4_guest_common_prep(anchor.as_deref(), &progress)
        })
    };
    // bo2mp: World at War's and CoD4's guns as extra weapons: their common
    // zones are read ahead here, side by side; their materials are linked
    // after Ghosts' on the same pool.
    // bo2mp: Ghosts' guns: its common_core_mp inflated and walked ahead
    // (linked after MW2's on the same material pool).
    let iw6_prefetch = {
        let wanted = wants("iw6");
        pool.spawn(async move { crate::lane::prefetch_iw6_guest_guns(wanted) })
    };
    // bo2mp: Advanced Warfare's guns: its common_mp and the guns' transient
    // zones inflated and walked ahead.
    let s1_prefetch = {
        let wanted = wants("s1");
        pool.spawn(async move { crate::lane::prefetch_s1_guest_guns(wanted) })
    };
    // bo2mp: Horizon MW's guns: its weapon zones found ahead (each is
    // read and built in turn after AW's).
    let hmw_prefetch = crate::lane::prefetch_hmw_guest_guns(wants("h1"));
    // bo2mp: Infinite Warfare's guns: its zones found ahead (read and built
    // after Horizon MW's).
    let iw7_prefetch = crate::lane::prefetch_iw7_guest_guns(wants("iw7"));
    // bo2mp: Black Ops III's guns: its core_common inflated and walked
    // ahead.
    let t7_prefetch = {
        let wanted = wants("t7");
        pool.spawn(async move { crate::lane::prefetch_t7_guest_guns(wanted) })
    };
    // bo2mp: the Black Ops III Workshop weapon packs' guns: their zones
    // inflated and walked ahead.
    let t7_pack_prefetch = {
        let wanted = bo2_profile::guest::WORKSHOP_PACKS.iter().any(|p| wants(p.game));
        pool.spawn(async move { crate::lane::prefetch_t7_pack_guns(wanted) })
    };
    // bo2mp: Modern Warfare III's (2023) guns: their part zones and
    // global_stream_mp's materials read ahead.
    // bo2mp: Modern Warfare III's operators read ahead (the soldiers).
    // bo2mp: or Black Ops' soldiers (whichever the match draws).
    match crate::soldiers() {
        frame::settings::Soldiers::Mw3 => pool.spawn(async move { crate::lane::jup_ops::prefetch() }).detach(),
        frame::settings::Soldiers::BlackOps => pool.spawn(async move { crate::lane::t5_soldiers::prefetch() }).detach(),
        frame::settings::Soldiers::BlackOps2 => {}
    }
    let jup_prefetch = {
        let wanted = wants("jup");
        pool.spawn(async move { crate::lane::prefetch_jup_guest_guns(wanted) })
    };
    let legacy_prefetch = [crate::lane::Legacy::T4, crate::lane::Legacy::Iw3].map(|game| {
        let wanted = wants(match game {
            crate::lane::Legacy::T4 => "t4",
            crate::lane::Legacy::Iw3 => "iw3",
        });
        pool.spawn(async move { crate::lane::prefetch_legacy_guest_guns(game, wanted) })
    });
    let localize_walk = {
        let anchor = anchor.clone();
        let progress = progress.clone();
        pool.spawn(async move {
            let mut report = Vec::new();
            let strings = match &anchor {
                Some(path) => {
                    let stage = progress.begin(StageId::Localization, None);
                    let catalog = load_localized_strings_beside(path, &mut report, &stage);
                    stage.done();
                    catalog
                }
                None => {
                    progress.record_skipped(StageId::Localization);
                    report.push(
                        "localize: no runtime common_mp — every on-screen string is a gap".into(),
                    );
                    LocalizeCatalog::default()
                }
            };
            (strings, report)
        })
    };

    let (material_seed, mut common_report, iw4_stats, startup_light_defs, startup_scripts) =
        startup_walk.await;
    let startup_count = material_seed.materials.len();

    let t5_weapon_walk = {
        let progress = progress.clone();
        let job = load_jobs::open(JobKind::ImageDecode).namespace("t5");
        pool.spawn(async move {
            walk_t5_weapon_common(t5_common_prep.await, &progress, material_seed, job)
        })
    };

    let T5WeaponCommon {
        weapons: t5_weapons,
        fpv: t5_fpv,
        world_guns: t5_world_guns,
        mut material_seed,
        xanims: t5_xanims,
        fx: t5_fx,
        impact_fx: t5_impact_fx,
        projectiles: t5_projectiles,
        teamsets: t5_teamsets,
        scene_models: t5_scene_models,
        images: t5_images,
        stats_tables: t5_stats,
        report: t5_report,
    } = t5_weapon_walk.await;
    // (MW2's guest guns walk after Black Ops', on the same material pool.)
    let T5WeaponCommon {
        weapons: iw4_guest_weapons,
        fpv: iw4_guest_fpv,
        world_guns: iw4_guest_world_guns,
        material_seed,
        xanims: iw4_guest_xanims,
        fx: iw4_guest_fx,
        projectiles: iw4_guest_projectiles,
        images: iw4_guest_images,
        stats_tables: (_, iw4_guest_stats),
        report: iw4_guest_report,
        ..
    } = {
        let progress = progress.clone();
        let job = load_jobs::open(JobKind::ImageDecode).namespace("iw4");
        let prep = iw4_guest_prep.await;
        pool.spawn(async move { walk_t5_weapon_common(prep, &progress, material_seed, job) }).await
    };
    common_report.extend(iw4_guest_report.into_iter().map(|l| l.replacen("t5 weapon common", "iw4 guest guns", 1)));
    // bo2mp: Ghosts' guest guns, after MW2's on the same material pool.
    let crate::lane::Iw6Guests {
        weapons: iw6_guest_weapons,
        fpv: iw6_guest_fpv,
        world_guns: iw6_guest_world_guns,
        xanims: iw6_guest_xanims,
        material_seed,
        report: iw6_guest_report,
        fx: iw6_guest_fx,
        tracers: iw6_guest_tracers,
    } = {
        let progress = progress.clone();
        let prefetch = iw6_prefetch.await;
        pool.spawn(async move { crate::lane::build_iw6_guest_guns(prefetch, material_seed, &progress) }).await
    };
    common_report.extend(iw6_guest_report);
    // bo2mp: Advanced Warfare's guest guns, after Ghosts' on the same pool.
    let crate::lane::S1Guests {
        weapons: s1_guest_weapons,
        fpv: s1_guest_fpv,
        world_guns: s1_guest_world_guns,
        xanims: s1_guest_xanims,
        material_seed,
        fx: s1_guest_fx,
        fx_models: s1_guest_fx_models,
        tracers: s1_guest_tracers,
        report: s1_guest_report,
    } = {
        let progress = progress.clone();
        let prefetch = s1_prefetch.await;
        pool.spawn(async move { crate::lane::build_s1_guest_guns(prefetch, material_seed, &progress) }).await
    };
    common_report.extend(s1_guest_report);
    // bo2mp: Horizon MW's guest guns, after AW's on the same pool.
    let crate::lane::HmwGuests {
        weapons: hmw_guest_weapons,
        fpv: hmw_guest_fpv,
        world_guns: hmw_guest_world_guns,
        xanims: hmw_guest_xanims,
        material_seed,
        fx: hmw_guest_fx,
        fx_models: hmw_guest_fx_models,
        tracers: hmw_guest_tracers,
        report: hmw_guest_report,
        ..
    } = {
        let progress = progress.clone();
        pool.spawn(async move { crate::lane::build_hmw_guest_guns(hmw_prefetch, material_seed, &progress) }).await
    };
    common_report.extend(hmw_guest_report);
    // bo2mp: Infinite Warfare's guest guns, after Horizon MW's on the same
    // pool.
    let crate::lane::Iw7Guests {
        weapons: iw7_guest_weapons,
        fpv: iw7_guest_fpv,
        world_guns: iw7_guest_world_guns,
        xanims: iw7_guest_xanims,
        material_seed,
        report: iw7_guest_report,
        fx: iw7_guest_fx,
        fx_models: iw7_guest_fx_models,
        tracers: iw7_guest_tracers,
        ..
    } = {
        let progress = progress.clone();
        pool.spawn(async move { crate::lane::build_iw7_guest_guns(iw7_prefetch, material_seed, &progress) }).await
    };
    common_report.extend(iw7_guest_report);
    // bo2mp: Black Ops III's guest guns, after AW's on the same pool.
    let crate::lane::T7Guests {
        weapons: t7_guest_weapons,
        fpv: t7_guest_fpv,
        world_guns: t7_guest_world_guns,
        xanims: t7_guest_xanims,
        material_seed,
        report: t7_guest_report,
    } = {
        let progress = progress.clone();
        let prefetch = t7_prefetch.await;
        pool.spawn(async move { crate::lane::build_t7_guest_guns(prefetch, material_seed, &progress) }).await
    };
    common_report.extend(t7_guest_report);
    // bo2mp: the Workshop packs' guns, after Black Ops III's own.
    let (mut t7_pack_guests, material_seed, t7_pack_report) = {
        let progress = progress.clone();
        let prefetch = t7_pack_prefetch.await;
        pool.spawn(async move { crate::lane::build_t7_pack_guns(prefetch, material_seed, &progress) }).await
    };
    common_report.extend(t7_pack_report);
    for g in &mut t7_pack_guests {
        common_report.append(&mut g.report);
    }
    // bo2mp: Modern Warfare III's guest guns' models, after Black Ops III's
    // on the same pool (their weapons are made per match from Black Ops
    // II's, `lane::jup_guest_rows`).
    let crate::lane::JupGuests {
        fpv: jup_guest_fpv,
        world_guns: jup_guest_world_guns,
        material_seed,
        fx: jup_guest_fx,
        fx_models: jup_guest_fx_models,
        report: jup_guest_report,
    } = {
        let prefetch = jup_prefetch.await;
        pool.spawn(async move { crate::lane::build_jup_guest_guns(prefetch, material_seed) }).await
    };
    common_report.extend(jup_guest_report);
    let mut legacy_guests = Vec::new();
    let mut material_seed = material_seed;
    for prefetch in legacy_prefetch {
        let prefetch = prefetch.await;
        let progress = progress.clone();
        let mut guests =
            pool.spawn(async move { crate::lane::build_legacy_guest_guns(prefetch, material_seed, &progress) }).await;
        material_seed = std::mem::take(&mut guests.material_seed);
        common_report.append(&mut guests.report);
        legacy_guests.push(guests);
    }
    let mut t5_xanims = t5_xanims;
    let mut t5_fx = t5_fx;
    let iw4_guest_xanim_n = t5_xanims.absorb(iw4_guest_xanims);
    let iw6_guest_xanim_n = t5_xanims.absorb(iw6_guest_xanims);
    let s1_guest_xanim_n = t5_xanims.absorb(s1_guest_xanims);
    let hmw_guest_xanim_n = t5_xanims.absorb(hmw_guest_xanims);
    let iw7_guest_xanim_n = t5_xanims.absorb(iw7_guest_xanims);
    let t7_guest_xanim_n = t5_xanims.absorb(t7_guest_xanims);
    let t7_pack_xanim_n: Vec<usize> =
        t7_pack_guests.iter_mut().map(|g| t5_xanims.absorb(std::mem::take(&mut g.xanims))).collect();
    let legacy_xanim_n: Vec<usize> =
        legacy_guests.iter_mut().map(|g| t5_xanims.absorb(std::mem::take(&mut g.xanims))).collect();
    t5_fx.absorb_missing(iw4_guest_fx);
    // bo2mp: Advanced Warfare's guns' own effects (S1 namespace: no name
    // meets another game's).
    let s1_guest_fx_n = s1_guest_fx.len();
    t5_fx.absorb(s1_guest_fx);
    // bo2mp: Horizon MW's guns' own effects (H1 namespace).
    let hmw_guest_fx_n = hmw_guest_fx.len();
    t5_fx.absorb(hmw_guest_fx);
    // bo2mp: the effects Modern Warfare III's guns' stand-ins play (H1 and
    // T7 namespaces).
    let jup_guest_fx_n = jup_guest_fx.len();
    t5_fx.absorb(jup_guest_fx);
    // bo2mp: Ghosts' guns' own impact effects (Ghosts namespace).
    let iw6_guest_fx_n = iw6_guest_fx.len();
    t5_fx.absorb(iw6_guest_fx);
    // bo2mp: Infinite Warfare's guns' own effects (IW7 namespace).
    let iw7_guest_fx_n = iw7_guest_fx.len();
    t5_fx.absorb(iw7_guest_fx);
    let t5_ids = t5_weapons.len();
    let t5_fpv_n = t5_fpv.len();
    let t5_xanim_n = t5_xanims.len();
    let t5_reuse_mat = material_seed.link_reused_materials;
    let t5_mat_count = material_seed
        .materials
        .len()
        .saturating_sub(startup_count)
        .saturating_add(t5_reuse_mat);
    common_report.extend(t5_report);
    let (foreign_materials, mut foreign_report, shared_bundle, foreign_images) =
        match data_common_walk {
            ForeignCommonWork::Shared(task) => {
                let (materials, bundle, images, report) = task.await;
                (materials, report, Some(bundle), images)
            }
            ForeignCommonWork::Split(Some(task)) => {
                let (materials, images, report) = task.await;
                (materials, report, None, images)
            }
            ForeignCommonWork::Split(None) => (MaterialCatalog::default(), Vec::new(), None, None),
        };

    common_report.append(&mut donor_report);
    common_report.append(&mut foreign_report);

    let foreign_count = foreign_materials.materials.len();
    let foreign_linked = material_seed.absorb_asset_population(foreign_materials);
    let mut shared_bundle = shared_bundle;
    if let Some(bundle) = &mut shared_bundle {
        bundle.scene_models.remap_walk_materials(&foreign_linked);
    }

    let mut shared_surfaces = asset_model::SharedXModelSurfaces::default();
    let mut common_scene_models = asset_world::MapXModelSceneCatalog::default();
    let mut common_light_defs = startup_light_defs;
    let mut common_pen_table = weapon_iw4::PenetrationDepthTable::empty();
    let mut common_pen_loaded = false;
    let mut common_lochit_table = None;

    let mut common_tracers = asset_game::TracerCatalog::default();
    let mut xmodel_walk = crate::PreparedXModelWalkCensus::default();
    let mut s1_common_bytes = 0;
    let mut teamsets = t5_teamsets;
    let mut scripts = startup_scripts;
    let mut common_film_visions = std::collections::BTreeMap::new();
    let mut fpv_plan = None;
    let mut iw4_census_stats = Vec::new();

    let common_opened = common_open.await;

    let (
        mut weapons,
        mut fpv_meshes,
        mut world_weapons,
        mut projectile_meshes,
        mut xanims,
        mut player_anim_sources,
        common_fx,
        mut common_fx_models,
        common_impact,
        material_seed,
        mut common_walk_report,
    ) = match common_opened {
        Some((path, Ok(image))) => {
            let mut census =
                lane(image.game).load_common_mp(&path, &image, &progress, true, material_seed);
            fpv_plan = census.pending_images.take().filter(|plan| !plan.is_empty());
            shared_surfaces = census.shared_surfaces;
            common_scene_models = census.scene_models;
            common_light_defs.extend(census.light_defs);
            common_pen_table = census.pen_table;
            common_pen_loaded = census.pen_table_loaded;
            common_lochit_table = census.lochit_table;
            common_tracers = census.tracers;
            xmodel_walk = census.xmodel_walk;
            s1_common_bytes = census.s1_common_bytes;
            teamsets.extend(census.teamsets);
            common_film_visions = census.film_visions;
            // patch_mp loads after common_mp, so its scripts win.
            scripts = {
                let mut ordered = census.scripts;
                ordered.overlay(std::mem::take(&mut scripts));
                ordered
            };
            iw4_census_stats = census.cac_tables;
            (
                census.weapons,
                census.fpv,
                census.world_weapons,
                census.projectile_meshes,
                census.xanims,
                census.player_anim_sources,
                census.fx,
                census.fx_models,
                census.impact_fx,
                census.material_population,
                census.report,
            )
        }
        Some((_, Err(error))) => (
            asset_game::WeaponBuild::default(),
            FpvMeshBuild::default(),
            WorldWeaponBuild::default(),
            ProjectileMeshBuild::default(),
            XAnimBuild::default(),
            asset_anim::PlayerAnimSources::default(),
            asset_game::FxCatalog::default(),
            asset_game::FxModelCatalog::default(),
            None,
            material_seed,
            vec![format!("common_mp models: open zone: {error}")],
        ),
        None => (
            asset_game::WeaponBuild::default(),
            FpvMeshBuild::default(),
            WorldWeaponBuild::default(),
            ProjectileMeshBuild::default(),
            XAnimBuild::default(),
            asset_anim::PlayerAnimSources::default(),
            asset_game::FxCatalog::default(),
            asset_game::FxModelCatalog::default(),
            None,
            material_seed,
            Vec::new(),
        ),
    };
    common_report.append(&mut common_walk_report);
    weapons.apply_stats_tables(&iw4_stats);

    let common_reuse_mat = material_seed.link_reused_materials;
    let common_reuse_img = material_seed.link_reused_images;
    let seed_mat = material_seed.materials.len();
    let seed_img = material_seed.images.len();

    player_anim_sources.compile();
    common_report.push(player_anim_sources.compile_report_line());
    common_report.push(player_anim_sources.parse_report_line());

    let (bundle, bundle_images, mut iw5_report) = match (iw5_weapon_walk.take(), shared_bundle) {
        (Some(task), _) => task.await,
        (None, Some(bundle)) => (bundle, None, Vec::new()),
        (None, None) => (Iw5WeaponBundle::default(), None, Vec::new()),
    };
    let Iw5WeaponBundle {
        weapons: mut iw5_weapons,
        fpv: iw5_fpv,
        world_guns: iw5_world_guns,
        xanims: iw5_xanims,
        materials: iw5_materials,
        stats_tables: iw5_census_stats,
        scene_models: iw5_scene_models,
        shared_surfaces: iw5_shared_surfaces,
    } = bundle;
    let mut material_seed = material_seed;
    let iw5_stats = match iw5_startup_walk {
        Some(task) => {
            let population = task.await;
            common_report.extend(population.report);
            common_light_defs.extend(population.light_defs);
            material_seed.absorb_asset_population(population.materials);
            population.cac_tables
        }
        None => Vec::new(),
    };
    iw5_weapons.apply_stats_tables(&iw5_stats);
    let iw4_weapon_n = weapons.len();
    let fpv_common_n = fpv_meshes.len();
    let absorbed = iw5_weapons.len();
    weapons.absorb(iw5_weapons);
    let iw5_fpv_added = fpv_meshes.absorb(iw5_fpv);
    let world_gun_common_n = world_weapons.len();
    let iw5_world_added = world_weapons.absorb(iw5_world_guns);
    let xanim_before_iw5 = xanims.len();
    let iw5_xanim_added = xanims.absorb(iw5_xanims);
    let iw5_mat_n = iw5_materials.materials.len();
    common_report.append(&mut iw5_report);
    common_report.push(format!(
        "iw5 weapon bundle: donor={} absorbed={absorbed} catalog {iw4_weapon_n}→{} fpv {fpv_common_n}→{} (+{iw5_fpv_added} iw5 keys) world guns {world_gun_common_n}→{} (+{iw5_world_added} iw5 keys) xanims {xanim_before_iw5}→{} (+{iw5_xanim_added} iw5 keys) materials={iw5_mat_n} (absorbed after IW4 pool)",
        absorbed,
        weapons.len(),
        fpv_meshes.len(),
        world_weapons.len(),
        xanims.len(),
    ));
    match weapons.resolve_index("iw5_msr") {
        Ok(Some(id)) => {
            let facts = weapons.facts_of(id);
            common_report.push(format!(
                "iw5_msr: id={id} gun={} world={} fire={:?} clip={:?} class={:?} type={:?} ftype={:?} bolt={:?} raise={:?} start={:?} dmg={:?} overlay={:?} overlay_img={:?} ov_w={:?} ov_h={:?}",
                weapons.gun_xmodel_of(id).unwrap_or("<none>"),
                weapons.world_model_of(id).unwrap_or("<none>"),
                facts.map(|f| f.fire_time_ms),
                facts.map(|f| f.clip_size),
                facts.map(|f| f.weap_class),
                facts.map(|f| f.weap_type),
                facts.map(|f| f.fire_type),
                facts.map(|f| f.bolt_action),
                facts.map(|f| f.raise_time_ms),
                facts.map(|f| f.start_ammo),
                facts.map(|f| f.damage),
                weapons.overlay_material_of(id),
                weapons.overlay_image_of(id),
                facts.map(|f| f.ads_overlay_width),
                facts.map(|f| f.ads_overlay_height),
            ));
        }
        Ok(None) | Err(_) => common_report.push("iw5_msr: not in merged catalog".into()),
    }

    let mut t5_weapons = t5_weapons;
    let (t5_code_stats, t5_census_stats) = t5_stats;
    t5_weapons.apply_stats_tables(&t5_code_stats);
    let t5_absorbed = t5_weapons.len();
    weapons.absorb(t5_weapons);
    if !iw4_guest_weapons.is_empty() {
        let mut guests = iw4_guest_weapons;
        guests.apply_stats_tables(&iw4_guest_stats);
        let n = guests.len();
        weapons.absorb(guests);
        projectile_meshes.absorb(iw4_guest_projectiles);
        let fpv = fpv_meshes.absorb(iw4_guest_fpv);
        let world = world_weapons.absorb(iw4_guest_world_guns);
        common_report.push(format!(
            "iw4 guest guns absorbed: {n} weapons, fpv=+{fpv} world_guns=+{world} xanims=+{iw4_guest_xanim_n}; registry now {} (iw4={})",
            weapons.len(),
            weapons.namespace_count(asset_core::AssetNamespace::Iw4)
        ));
    }
    if !iw6_guest_weapons.is_empty() {
        let n = iw6_guest_weapons.len();
        weapons.absorb(iw6_guest_weapons);
        let fpv = fpv_meshes.absorb(iw6_guest_fpv);
        let world = world_weapons.absorb(iw6_guest_world_guns);
        let tracer_n = iw6_guest_tracers.len();
        common_tracers.absorb(iw6_guest_tracers);
        common_report.push(format!(
            "iw6 guest guns absorbed: {n} weapons, fpv=+{fpv} world_guns=+{world} xanims=+{iw6_guest_xanim_n}, {iw6_guest_fx_n} own impact effects, {tracer_n} tracers; registry now {} (iw6={})",
            weapons.len(),
            weapons.namespace_count(asset_core::AssetNamespace::Iw6)
        ));
    }
    if !s1_guest_weapons.is_empty() {
        let n = s1_guest_weapons.len();
        weapons.absorb(s1_guest_weapons);
        let fpv = fpv_meshes.absorb(s1_guest_fpv);
        let world = world_weapons.absorb(s1_guest_world_guns);
        common_report.push(format!(
            "s1 guest guns absorbed: {n} weapons, fpv=+{fpv} world_guns=+{world} xanims=+{s1_guest_xanim_n}; registry now {} (s1={})",
            weapons.len(),
            weapons.namespace_count(asset_core::AssetNamespace::S1)
        ));
        // bo2mp: their effects (joined to the leftover effects above),
        // effect models and tracers.
        let models = common_fx_models.absorb(s1_guest_fx_models);
        let tracer_n = s1_guest_tracers.len();
        common_tracers.absorb(s1_guest_tracers);
        common_report.push(format!(
            "s1 guest guns: {s1_guest_fx_n} effects, +{models} effect models, {tracer_n} tracers joined"
        ));
    }
    if !hmw_guest_weapons.is_empty() {
        let n = hmw_guest_weapons.len();
        weapons.absorb(hmw_guest_weapons);
        let fpv = fpv_meshes.absorb(hmw_guest_fpv);
        let world = world_weapons.absorb(hmw_guest_world_guns);
        let models = common_fx_models.absorb(hmw_guest_fx_models);
        let tracer_n = hmw_guest_tracers.len();
        common_tracers.absorb(hmw_guest_tracers);
        common_report.push(format!(
            "h1 guest guns absorbed: {n} weapons, fpv=+{fpv} world_guns=+{world} xanims=+{hmw_guest_xanim_n}, {hmw_guest_fx_n} effects, +{models} effect models, {tracer_n} tracers; registry now {} (h1={})",
            weapons.len(),
            weapons.namespace_count(asset_core::AssetNamespace::H1)
        ));
    }
    if !iw7_guest_weapons.is_empty() {
        let n = iw7_guest_weapons.len();
        weapons.absorb(iw7_guest_weapons);
        let fpv = fpv_meshes.absorb(iw7_guest_fpv);
        let world = world_weapons.absorb(iw7_guest_world_guns);
        let models = common_fx_models.absorb(iw7_guest_fx_models);
        let tracer_n = iw7_guest_tracers.len();
        common_tracers.absorb(iw7_guest_tracers);
        common_report.push(format!(
            "iw7 guest guns absorbed: {n} weapons, fpv=+{fpv} world_guns=+{world} xanims=+{iw7_guest_xanim_n}, {iw7_guest_fx_n} own effects, +{models} effect models, {tracer_n} tracers; registry now {} (iw7={})",
            weapons.len(),
            weapons.namespace_count(asset_core::AssetNamespace::Iw7)
        ));
    }
    if !t7_guest_weapons.is_empty() {
        let n = t7_guest_weapons.len();
        weapons.absorb(t7_guest_weapons);
        let fpv = fpv_meshes.absorb(t7_guest_fpv);
        let world = world_weapons.absorb(t7_guest_world_guns);
        common_report.push(format!(
            "t7 guest guns absorbed: {n} weapons, fpv=+{fpv} world_guns=+{world} xanims=+{t7_guest_xanim_n}; registry now {} (t7={})",
            weapons.len(),
            weapons.namespace_count(asset_core::AssetNamespace::T7)
        ));
    }
    for (guests, xanim_n) in t7_pack_guests.into_iter().zip(t7_pack_xanim_n) {
        if guests.weapons.is_empty() {
            continue;
        }
        let n = guests.weapons.len();
        weapons.absorb(guests.weapons);
        let fpv = fpv_meshes.absorb(guests.fpv);
        let world = world_weapons.absorb(guests.world_guns);
        common_report.push(format!(
            "t7 workshop guns absorbed: {n} weapons, fpv=+{fpv} world_guns=+{world} xanims=+{xanim_n}; registry now {} (t7={})",
            weapons.len(),
            weapons.namespace_count(asset_core::AssetNamespace::T7)
        ));
    }
    if jup_guest_fpv.len() + jup_guest_world_guns.len() > 0 {
        let fpv = fpv_meshes.absorb(jup_guest_fpv);
        let world = world_weapons.absorb(jup_guest_world_guns);
        common_report.push(format!("jup guest guns absorbed: fpv=+{fpv} world_guns=+{world}"));
    }
    if jup_guest_fx_n > 0 {
        let models = common_fx_models.absorb(jup_guest_fx_models);
        common_report.push(format!("jup guest guns: {jup_guest_fx_n} stand-in effects, +{models} effect models joined"));
    }
    for (guests, xanim_n) in legacy_guests.into_iter().zip(legacy_xanim_n) {
        if guests.weapons.is_empty() {
            continue;
        }
        let ns = guests.weapons.namespace_of(1);
        let n = guests.weapons.len();
        weapons.absorb(guests.weapons);
        let fpv = fpv_meshes.absorb(guests.fpv);
        let world = world_weapons.absorb(guests.world_guns);
        common_report.push(format!(
            "{} guest guns absorbed: {n} weapons, fpv=+{fpv} world_guns=+{world} xanims=+{xanim_n}; registry now {}",
            ns.map_or("?", |ns| ns.as_str()),
            weapons.len(),
        ));
    }
    projectile_meshes.absorb(t5_projectiles);
    let t5_fpv_added = fpv_meshes.absorb(t5_fpv);
    let t5_world_added = world_weapons.absorb(t5_world_guns);
    common_report.push(format!(
        "t5 weapon absorb: donor={t5_ids} unique={t5_absorbed} fpv=+{t5_fpv_added}/{t5_fpv_n} world_guns=+{t5_world_added}; registry now {} (t5={})",
        weapons.len(),
        weapons.namespace_count(asset_core::AssetNamespace::T5)
    ));
    common_report.push(format!(
        "FPV generation: common={fpv_common_n} iw5_keys={iw5_fpv_added} t5={t5_fpv_n} t5_keys={t5_fpv_added} collide={} merged={}",
        fpv_meshes.collide_name_count(),
        fpv_meshes.len()
    ));

    let cac_tables: Vec<(asset_core::AssetNamespace, asset_game::CapturedStringTable)> = [
        (asset_core::AssetNamespace::Iw4, iw4_stats, iw4_census_stats),
        (asset_core::AssetNamespace::Iw5, iw5_stats, iw5_census_stats),
        (
            asset_core::AssetNamespace::T5,
            t5_code_stats,
            t5_census_stats,
        ),
    ]
    .into_iter()
    .flat_map(|(namespace, code, common)| {
        code.into_iter()
            .chain(common)
            .map(move |table| (namespace, table))
    })
    .collect();

    weapons.set_family_tables(cac_tables.clone());
    let iw5_prepared = weapons.prepare_iw5_configurations();
    weapons.resolve_fpv_mesh_edges(&fpv_meshes);
    weapons.resolve_fpv_hands(&fpv_meshes, &asset_model::BodyMeshCatalog::default());
    weapons.resolve_world_model_edges(&world_weapons);

    common_report.push(format!(
        "IW5 configurations: prepared={} refused={} {:?}",
        iw5_prepared.prepared,
        iw5_prepared.refused.len(),
        iw5_prepared
            .refused
            .iter()
            .map(|selection| format!(
                "{} [{}]",
                selection
                    .family
                    .as_ref()
                    .map(|key| key.to_string())
                    .unwrap_or_default(),
                selection.attachments.join(" ")
            ))
            .collect::<Vec<_>>()
    ));
    let (strings, localize_report) = localize_walk.await;
    common_report.extend(progress.timing_report());
    let prepared_ms = started.elapsed().as_secs_f32() * 1000.0;
    diag::info!(
        World,
        "common set: {key} prepared in {prepared_ms:.0}ms; donor images still decoding"
    );

    material_seed.mark_images_common_owned();
    let mut iw5_materials = iw5_materials;
    iw5_materials.mark_images_common_owned();
    let set = Arc::new(CommonSet {
        id: NEXT_COMMON_PROFILE_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
        key,
        products: std::sync::RwLock::new(CommonProducts {
            scripts,
            t5_scene_models,
            material_seed,
            shared_surfaces,
            scene_models: common_scene_models,
            light_defs: common_light_defs,
            pen_table: common_pen_table,
            pen_table_loaded: common_pen_loaded,
            lochit_table: common_lochit_table,
            tracers: common_tracers,
            xmodel_walk,
            s1_common_bytes,
            teamsets,
            film_visions: common_film_visions,
            weapons,
            fpv_meshes,
            world_weapons,
            projectile_meshes,
            xanims,
            player_anim_sources,
            fx: common_fx,
            fx_models: common_fx_models,
            impact_fx: common_impact,
            t5_xanims,
            t5_fx,
            t5_impact_fx,
            iw5_materials,
            iw5_scene_models,
            iw5_shared_surfaces,
            strings,
            counts: CommonCounts {
                startup_count,
                t5_mat_count,
                t5_reuse_mat,
                t5_xanim_n,
                foreign_count,
                seed_mat,
                seed_img,
                common_reuse_mat,
                common_reuse_img,
            },
            report: common_report,
            localize_report,
        }),
        donor_images: async_lock::OnceCell::new(),
        stripped: std::sync::atomic::AtomicBool::new(false),
        fpv_plan,
        retained: std::sync::Mutex::new(Default::default()),
        cac_tables,
        prepared_ms,
        ready_at: std::time::Instant::now(),
    });

    let keeping = Arc::clone(&set);
    load_pool()
        .spawn(async move {
            let mut kept = Vec::new();
            for (namespace, pending) in [
                ("t5", t5_images),
                ("iw4", iw4_guest_images),
                ("iw5", foreign_images),
                ("iw5", bundle_images),
            ] {
                let Some(pending) = pending else {
                    continue;
                };
                let job = pending.job;
                let (label, batch) = pending.join().await;
                let batch = batch.into_kept();
                keeping.retain(&batch);
                kept.push(KeptImages {
                    label,
                    namespace,
                    batch,
                    job,
                });
            }
            diag::info!(
                World,
                "common set: {} donor image batches kept, {} payloads ({:.1}MiB)",
                kept.len(),
                keeping.retained_payloads(),
                keeping.retained_bytes() as f64 / (1024.0 * 1024.0),
            );
            let _ = keeping.donor_images.set(kept).await;
        })
        .detach();
    set
}

fn walk_iw5_startup(donor: &Path, progress: &LoadProgress) -> crate::lane::MaterialPopulation {
    let found = match find_zone_for_tree(donor, "code_post_gfx_mp") {
        Ok(found) => found,
        Err(error) => {
            return crate::lane::MaterialPopulation {
                report: vec![format!("IW5 startup code_post_gfx_mp: {error}")],
                ..Default::default()
            };
        }
    };
    let mut population = walk_population_file(&found.path, progress, MaterialCatalog::default());
    population.report.push(format!(
        "IW5 startup: {} tables={} materials={} images={} light_defs={}",
        found.path.display(),
        population.cac_tables.len(),
        population.materials.materials.len(),
        population.materials.images.len(),
        population.light_defs.len()
    ));
    population
}

#[cfg(test)]
mod tests {
    #[test]
    fn only_a_slow_set_within_the_cap_is_parked() {
        let cap = 2048 << 20;
        assert!(super::parks(31_801.0, 1_200 << 20, cap));
        assert!(!super::parks(10.0, 0, cap), "a set made in 10 ms is made again");
        assert!(!super::parks(31_801.0, cap + 1, cap), "over the cap");
    }
}
