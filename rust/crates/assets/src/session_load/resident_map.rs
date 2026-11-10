use super::*;

#[derive(Clone, Debug, PartialEq, Eq)]
struct ZoneStamp {
    pub(super) path: PathBuf,
    pub(super) len: u64,
    modified: Option<std::time::SystemTime>,
}

impl ZoneStamp {
    pub(super) fn of(path: &Path) -> Option<Self> {
        let meta = std::fs::metadata(path).ok()?;
        Some(Self {
            path: path.to_path_buf(),
            len: meta.len(),
            modified: meta.modified().ok(),
        })
    }
}

struct ResidentMap {
    pub(super) zone: ZoneStamp,
    pub(super) common: Arc<CommonSet>,
    pub(super) prepared: PreparedMatch,
    /// Its decoded images were let go once the card had them all
    /// ([`release_resident_cpu_images`]): it can stand in for a walk only
    /// while the card still holds that walk's images.
    stripped: bool,
}

static RESIDENT_MAP: std::sync::Mutex<Option<ResidentMap>> = std::sync::Mutex::new(None);

/// The walk whose map images the card holds now (0: none). The render
/// frontend says so when a map's upload completes and when it lets them go.
static GPU_RESIDENT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

pub fn note_gpu_resident(products_id: u64) {
    GPU_RESIDENT.store(products_id, std::sync::atomic::Ordering::Relaxed);
}

/// The walk whose images the card is still meant to keep for a same-map
/// load (0 once that map was dropped for another).
pub fn gpu_resident() -> u64 {
    GPU_RESIDENT.load(std::sync::atomic::Ordering::Relaxed)
}

/// The card has every image of walk `products_id`: the copy kept for a
/// same-map load lets go of its decoded images (each one is the texture's
/// bytes a second time, ~580 MB on Nuketown; 8 games at once on one PC
/// cannot carry that). Its zone payloads go too: a stripped copy stands in
/// only while the card holds every texture, so nothing decodes them again
/// (`BO2MP_KEEP_MAP_PAYLOADS=1` keeps them, the old behaviour). Returns how
/// many images it let go and the bytes that were actually freed (an image
/// or payload the shared common set also holds stays).
pub fn release_resident_cpu_images(products_id: u64) -> Option<(usize, u64)> {
    let mut slot = RESIDENT_MAP
        .lock()
        .unwrap_or_else(|poison| poison.into_inner());
    let resident = slot.as_mut()?;
    if resident.stripped || resident.prepared.materials.products_id != products_id {
        return None;
    }
    let held = Arc::strong_count(&resident.prepared.materials.population);
    let Some(population) = Arc::get_mut(&mut resident.prepared.materials.population) else {
        diag::info!(
            World,
            "resident map: walk #{products_id} keeps its decoded images (its materials are held {held} times)"
        );
        return None;
    };
    let keep_payloads = std::env::var_os("BO2MP_KEEP_MAP_PAYLOADS").is_some_and(|v| v == "1");
    let (mut count, mut freed, mut payload_freed) = (0, 0u64, 0u64);
    for image in &mut population.images {
        if let Some(decoded) = image.decoded.take() {
            count += 1;
            if Arc::strong_count(&decoded) == 1 {
                freed += decoded.data.as_ref().map_or(0, |data| data.len() as u64);
            }
        }
        if !keep_payloads && !image.payload.is_empty() {
            let payload = std::mem::take(&mut image.payload);
            if Arc::strong_count(&payload) == 1 {
                payload_freed += payload.len() as u64;
            }
        }
    }
    resident.stripped = true;
    drop(slot);
    if payload_freed > 0 {
        diag::info!(
            World,
            "resident map: walk #{products_id} let go of its zone payloads ({:.0} MB freed)",
            payload_freed as f64 / (1024.0 * 1024.0)
        );
    }
    // The pixels were made on the loader threads; their pages go back there.
    super::release_pool_heaps();
    Some((count, freed + payload_freed))
}

/// bo2mp: what the load's caches hold, for the `memory:` lines: the shared
/// common set (its kept payloads and decoded pixels), the map kept for a
/// same-map load, and the zone files still open (the biggest named). A
/// stage's own rss change overlaps every stage running beside it, so this
/// names the holders instead.
pub(crate) fn held_memory_line() -> String {
    const MIB: f64 = 1024.0 * 1024.0;
    let mut parts = Vec::new();
    match super::common_cache::current_common() {
        Some(common) => parts.push(format!(
            "common set {:.0} MiB kept payloads + {:.0} MiB decoded",
            common.retained_bytes() as f64 / MIB,
            common.decoded_bytes() as f64 / MIB
        )),
        None => parts.push("no common set".to_owned()),
    }
    if let Some(parked) = super::common_cache::parked_common() {
        parts.push(format!(
            "parked common set {:.0} MiB kept payloads + {:.0} MiB decoded",
            parked.retained_bytes() as f64 / MIB,
            parked.decoded_bytes() as f64 / MIB
        ));
    }
    {
        let slot = RESIDENT_MAP.lock().unwrap_or_else(|poison| poison.into_inner());
        match slot.as_ref() {
            Some(resident) => {
                let images = &resident.prepared.materials.population.images;
                let decoded: u64 = images
                    .iter()
                    .filter_map(|image| image.decoded.as_ref()?.data.as_ref().map(|data| data.len() as u64))
                    .sum();
                let payload: u64 = images.iter().map(|image| image.payload.len() as u64).sum();
                parts.push(format!(
                    "kept map {:.0} MiB decoded + {:.0} MiB payloads{}",
                    decoded as f64 / MIB,
                    payload as f64 / MIB,
                    if resident.stripped { " (stripped)" } else { "" }
                ));
            }
            None => parts.push("no kept map".to_owned()),
        }
    }
    let zones = asset_transport::live_shared_zones();
    let total: u64 = zones.iter().map(|(_, bytes)| bytes).sum();
    let named = zones
        .iter()
        .take(4)
        .map(|(name, bytes)| format!("{name} {:.0}", *bytes as f64 / MIB))
        .collect::<Vec<_>>()
        .join(", ");
    parts.push(format!("{} zones open {:.0} MiB ({named})", zones.len(), total as f64 / MIB));
    format!("memory held: {}", parts.join("; "))
}

fn resident_copy(zone: &ZoneStamp, key: &CommonKey) -> Option<PreparedMatch> {
    let common = landed_common(key)?;
    let slot = RESIDENT_MAP
        .lock()
        .unwrap_or_else(|poison| poison.into_inner());
    let resident = slot.as_ref()?;
    // Without its decoded images the copy is good only for a load the card
    // already has every texture of.
    if resident.stripped
        && GPU_RESIDENT.load(std::sync::atomic::Ordering::Relaxed)
            != resident.prepared.materials.products_id
    {
        return None;
    }
    (resident.zone == *zone && Arc::ptr_eq(&resident.common, &common))
        .then(|| resident.prepared.clone())
}

pub async fn load_prepared_match(
    zone_ff: Result<PathBuf, String>,
    common_mp: Result<PathBuf, String>,
    progress: LoadProgress,
) -> MatchLoadOutcome {
    let stamp = zone_ff.as_deref().ok().and_then(ZoneStamp::of);
    if let Some(stamp) = &stamp {
        let key = CommonKey::for_match(
            Some(&stamp.path),
            common_mp.as_deref().ok(),
            &mut Vec::new(),
        );
        let copying = std::time::Instant::now();
        if let Some(mut prepared) = resident_copy(stamp, &key) {
            progress.record_reused_scoped(StageId::CommonAssets, "shared common");
            progress.record_reused_scoped(StageId::MapAssets, "resident");
            progress.record_reused_scoped(StageId::Images, "map");
            let line = format!(
                "resident map: reused `{}` walk #{} — no zone opened, no decode; match copy {:.0}ms",
                stamp.path.display(),
                prepared.materials.products_id,
                copying.elapsed().as_secs_f32() * 1000.0
            );
            diag::info!(World, "{line}");
            prepared.report.push(line);
            prepared.report.extend(progress.timing_report());
            return MatchLoadOutcome::Ready(prepared);
        }
    }
    let dropped = RESIDENT_MAP
        .lock()
        .unwrap_or_else(|poison| poison.into_inner())
        .take();
    if let Some(dropped) = dropped {
        let (path, products_id) = (dropped.zone.path.clone(), dropped.prepared.materials.products_id);
        drop(dropped);
        // The old map's light (its grid, a copy of its collision, its
        // lightmaps and probes) stayed installed until the next map's world
        // stage replaced it, through the whole walk; nothing reads it now.
        asset_world::install_t6_light_sampler(None);
        asset_world::install_t6_lightmaps(Vec::new());
        asset_world::install_t6_probe_origins(Vec::new());
        // The card's copy of its textures goes too: the render frontend
        // lets go of the handles it kept for a same-map load when it sees this.
        note_gpu_resident(0);
        // What the old map let go of goes back to Windows before the next
        // walk piles its own on top: left to the end of the load, the two
        // maps' memory together made the map change's peak (4.2 GB on Takeoff).
        super::release_pool_heaps();
        let trim_ms = diag::release_freed_heap().as_secs_f32() * 1000.0;
        let rss = asset_transport::process_resident_bytes().map_or(0, |bytes| bytes >> 20);
        let heap = diag::process_live_heap_bytes().map_or(0, |bytes| bytes >> 20);
        diag::info!(
            World,
            "resident map: `{}` walk #{products_id} dropped before the next walk (trim {trim_ms:.1}ms, rss {rss} MiB, heap {heap} MiB)",
            path.display()
        );
    }
    let (outcome, common) = walk_prepared_match(zone_ff, common_mp, progress.clone()).await;
    let MatchLoadOutcome::Ready(mut prepared) = outcome else {
        return outcome;
    };
    if let (Some(zone), Some(common)) = (stamp, common) {
        let keeping = std::time::Instant::now();
        let resident = prepared.clone();
        prepared.report.push(format!(
            "resident map: kept `{}` walk #{} for a same-map load ({:.0}ms)",
            zone.path.display(),
            prepared.materials.products_id,
            keeping.elapsed().as_secs_f32() * 1000.0
        ));
        *RESIDENT_MAP
            .lock()
            .unwrap_or_else(|poison| poison.into_inner()) = Some(ResidentMap {
            zone,
            common,
            prepared: resident,
            stripped: false,
        });
    }
    prepared.report.extend(progress.timing_report());
    MatchLoadOutcome::Ready(prepared)
}
