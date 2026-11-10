//! The load itself, as something that outlives the screen drawn over it.
//!
//! One request, one process. The overlay reads it; the overlay coming down or
//! never existing does not change what the load does, what it waits for or what
//! it reports. A new request takes a new process with its own [`LoadProgress`],
//! so a task still finishing the map before it writes into state nobody reads
//! rather than into the rows of the map that replaced it.

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

use bevy::prelude::Resource;

use asset_transport::progress::{LoadProgress, StageHandle, StageId};

/// Loads finished in this run: the first is the first map, any later one came
/// after a map change.
static LOADS_DONE: AtomicU64 = AtomicU64::new(0);

/// Loads started in this run (the last map was let go just before each).
static LOADS_STARTED: AtomicU64 = AtomicU64::new(0);

/// Match memory: the allocator gives back what a thread that has ended left
/// behind (a BO3 map's load, scene and lighting threads) only when the main
/// thread itself asks; the trim at load complete runs on a worker thread, so
/// after Skyjacked 1.7-2.5 GB stayed held through every later map. This asks
/// from the main thread once a load starts (the last map is gone) and once it
/// completes. `BO2MP_MAIN_HEAP_TRIM=1` turns it on (off until measured).
pub fn trim_main_heap_after_load(
    mut seen: bevy::prelude::Local<(u64, u64)>,
    _main_thread: bevy::ecs::system::NonSendMarker,
) {
    let now = (
        LOADS_STARTED.load(Ordering::Relaxed),
        LOADS_DONE.load(Ordering::Relaxed),
    );
    if now == *seen {
        return;
    }
    let when = if now.1 != seen.1 {
        "the map load completed"
    } else {
        "the last map was let go"
    };
    *seen = now;
    if !codlegacy_env::var("BO2MP_MAIN_HEAP_TRIM").is_ok_and(|v| v.trim() == "1") {
        return;
    }
    let before = asset_transport::process_resident_bytes().map_or(0, |b| b >> 20);
    let ms = diag::release_freed_heap().as_secs_f32() * 1000.0;
    let after = asset_transport::process_resident_bytes().map_or(0, |b| b >> 20);
    diag::info!(
        World,
        "memory: main thread heap trimmed after {when} ({ms:.1} ms, rss {before}->{after} MiB)"
    );
}

/// bo2mp (map bake): the row a load shows while it writes a map's baked copy.
#[derive(Default)]
pub struct BakeRow {
    /// The job the row is for (a job gets one row, once).
    job: Option<std::sync::Arc<map_bake::status::Job>>,
    stage: Option<StageHandle>,
    request_id: u64,
}

/// bo2mp (map bake): the first load of a map writes its baked copy
/// (`map_bake::status`); this shows it on the loading screen as its own row
/// ("Black Ops 3 - Skyjacked - pictures 412/2190"). Cancelling the load
/// cancels the bake, which then throws its half-written file away; a load
/// that ends or changes only closes the row (the bake finishes on its own).
pub fn mirror_bake_stage(load: Option<bevy::prelude::Res<MapLoadProcess>>, mut row: bevy::prelude::Local<BakeRow>) {
    // (A match loading stops Options > BAKE ALL: the load has the machine.)
    if load.is_some() && crate::bake_all::running() {
        crate::bake_all::stop();
    }
    let job = map_bake::status::current();
    let same_load = load.as_ref().is_some_and(|l| l.request_id == row.request_id);
    if !same_load && let Some(stage) = row.stage.take() {
        stage.cancel();
    }
    if let (Some(job), Some(load)) = (&job, &load) {
        let new_job = !row.job.as_ref().is_some_and(|j| std::sync::Arc::ptr_eq(j, job));
        if new_job && job.outcome().is_none() && !load.complete {
            if let Some(stage) = row.stage.take() {
                stage.cancel();
            }
            let (_, total) = job.counts();
            // ("Black Ops 3 - Skyjacked - pictures (game updated)".)
            let why = job.why();
            let label = if why.is_empty() { job.label().to_owned() } else { format!("{} ({why})", job.label()) };
            row.stage = Some(load.progress.begin_scoped(StageId::Bake, label, (total > 0).then_some(total)));
            row.job = Some(job.clone());
            row.request_id = load.request_id;
        }
    }
    let (Some(job), Some(stage)) = (row.job.clone(), row.stage.as_ref()) else { return };
    if load.as_ref().is_some_and(|l| l.progress.is_canceled()) || stage.is_canceled() {
        job.cancel();
    }
    stage.set_completed(job.counts().0);
    if let Some(kept) = job.outcome()
        && let Some(stage) = row.stage.take()
    {
        if kept { stage.done() } else { stage.cancel() }
    }
}

#[derive(Resource)]
pub struct MapLoadProcess {
    pub request_id: u64,
    pub load_key: frame::LocalLoadKey,
    pub zone: String,
    pub progress: LoadProgress,
    requested_at: Instant,
    /// Opened once local preparation is done and the session has still not let
    /// this client continue, so the table can name what is holding the load.
    admission: Option<StageHandle>,
    complete: bool,
}

impl MapLoadProcess {
    pub fn new(
        request_id: u64,
        load_key: frame::LocalLoadKey,
        zone: impl Into<String>,
        progress: LoadProgress,
    ) -> Self {
        let requested_at = progress.requested_at();
        let zone = zone.into();
        LOADS_STARTED.fetch_add(1, Ordering::Relaxed);
        diag::info!(World, "{} `{zone}`", diag::memory_line("loading"));
        diag::info!(World, "{}", crate::session_load::held_memory_line());
        Self {
            request_id,
            load_key,
            zone,
            progress,
            requested_at,
            admission: None,
            complete: false,
        }
    }

    pub fn requested_at(&self) -> Instant {
        self.requested_at
    }

    pub fn is_complete(&self) -> bool {
        self.complete
    }

    /// Local preparation is done and the session has not admitted this client
    /// yet. The wait is its own stage, not a gap between two other rows.
    pub fn await_admission(&mut self) {
        if self.complete || self.admission.is_some() {
            return;
        }
        self.admission = Some(self.progress.begin(StageId::Admission, None));
    }

    /// The client is in and the world is installed. The heap maintenance that
    /// follows belongs to the load, not to whichever surface was watching.
    pub fn finish(&mut self) {
        if self.complete {
            return;
        }
        self.complete = true;
        if let Some(stage) = self.admission.take() {
            stage.done();
        } else {
            self.progress.record_skipped(StageId::Admission);
        }
        let installed_ms = self.requested_at.elapsed().as_secs_f32() * 1000.0;
        crate::session_load::release_pool_heaps();
        let trim_ms = diag::release_freed_heap().as_secs_f32() * 1000.0;
        let with_trim_ms = self.requested_at.elapsed().as_secs_f32() * 1000.0;
        let rss = asset_transport::process_resident_bytes()
            .map(|bytes| format!(" rss_mib={}", bytes >> 20))
            .unwrap_or_default();
        let heap = diag::process_live_heap_bytes()
            .map(|bytes| format!(" heap_mib={}", bytes >> 20))
            .unwrap_or_default();
        diag::info!(
            World,
            "load complete: {installed_ms:.0}ms overlay-to-world-installed (+trim {with_trim_ms:.0}ms) for `{}`{rss}{heap} trim={trim_ms:.1}ms",
            self.zone
        );
        let stage = if LOADS_DONE.fetch_add(1, Ordering::Relaxed) == 0 {
            "in match"
        } else {
            "in match after map change"
        };
        diag::info!(World, "{} `{}`", diag::memory_line(stage), self.zone);
        diag::info!(World, "{}", crate::session_load::held_memory_line());
        asset_material::map_pak::finish(self.request_id, true);
    }

    /// The session refused this client. The wait ends as a failure; whatever the
    /// load already recorded stays readable.
    pub fn fail(&mut self) {
        if self.complete {
            return;
        }
        self.complete = true;
        if let Some(stage) = self.admission.take() {
            stage.fail();
        }
        asset_material::map_pak::finish(self.request_id, false);
    }

    pub fn abandon(&mut self) {
        if let Some(stage) = self.admission.take() {
            stage.cancel();
        }
        asset_material::map_pak::finish(self.request_id, false);
    }
}
