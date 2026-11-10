use std::path::PathBuf;

use bevy::prelude::*;
use bevy::tasks::{Task, futures_lite::future};
use frame::{ClientSet, LaunchIdentity, MapLoadApproved, MapLoadFailed};

use crate::map_load_process::MapLoadProcess;
use crate::session_load::{MatchLoadOutcome, PreparedMatch, load_prepared_match};
use asset_transport::progress::LoadProgress;

#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct MapLoadApproval;

#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct MatchLoadDispatch;

#[derive(Resource, Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct MatchLoadBusy(pub bool);

#[derive(Resource, Debug, Clone, Copy, PartialEq, Eq)]
pub struct MatchLoadAbort(pub u64);

#[derive(Resource)]
pub struct MatchLoadRequest {
    pub request_id: u64,
    pub load_key: frame::LocalLoadKey,

    pub zone: String,
    pub zone_ff: Result<PathBuf, String>,
    pub common_mp: Result<PathBuf, String>,
    pub progress: LoadProgress,
}

#[derive(Resource)]
struct MatchLoadTask {
    task: Task<Option<PreparedMatchReady>>,
    request_id: u64,
    load_key: frame::LocalLoadKey,
    zone: String,
    progress: LoadProgress,
    sound_sent: bool,
}

/// bo2mp (load timing): the map's sound, handed over by the walk the moment
/// it has it, so the audio compose runs beside the pictures instead of after
/// the whole walk. Only the walk `start_match_load` asked for fills it (no
/// copy for a probe or an export walk).
enum EarlyMapSound {
    Wanted(u64),
    Sent(u64, u64, u64, Result<asset_audio::SoundCatalog, String>),
}

static EARLY_MAP_SOUND: std::sync::Mutex<Option<EarlyMapSound>> = std::sync::Mutex::new(None);

fn early_map_sound() -> std::sync::MutexGuard<'static, Option<EarlyMapSound>> {
    EARLY_MAP_SOUND.lock().unwrap_or_else(|poison| poison.into_inner())
}

pub(crate) fn send_map_sound_early(
    request_id: u64,
    common_profile_id: u64,
    products_id: u64,
    sound: &Result<asset_audio::SoundCatalog, String>,
) {
    let mut slot = early_map_sound();
    if matches!(*slot, Some(EarlyMapSound::Wanted(wanted)) if wanted == request_id) {
        *slot = Some(EarlyMapSound::Sent(request_id, common_profile_id, products_id, sound.clone()));
    }
}

#[derive(Resource, Clone, Debug)]
pub struct MatchLoadAccepted {
    pub request_id: u64,
    pub load_key: frame::LocalLoadKey,
    pub zone: String,
}

#[derive(Resource)]
pub struct PreparedMatchReady {
    pub request_id: u64,
    pub load_key: frame::LocalLoadKey,
    pub zone: String,
    pub prepared: PreparedMatch,
}

#[derive(Resource)]
pub struct PreparedMatchSound {
    pub load_key: frame::LocalLoadKey,
    pub zone: String,
    pub common_profile_id: u64,
    pub products_id: u64,
    pub sound: Result<asset_audio::SoundCatalog, String>,
}

fn start_match_load(
    mut commands: Commands,
    request: Option<Res<MatchLoadRequest>>,
    busy: Option<Res<MatchLoadTask>>,
    retiring: Option<Res<frame::Retiring>>,
    abort: Option<Res<MatchLoadAbort>>,
    mut waiting_for_retirement: Local<Option<u64>>,
    mut inflight: ResMut<MatchLoadBusy>,
) {
    let Some(request) = request else {
        return;
    };
    if abort
        .as_ref()
        .is_some_and(|abort| abort.0 == request.request_id)
    {
        *waiting_for_retirement = None;
        commands.remove_resource::<MatchLoadRequest>();
        if busy.is_none() {
            inflight.0 = false;
        }
        diag::info!(
            World,
            "match load: abort request #{} before the walk started",
            request.request_id
        );
        return;
    }
    if busy.is_some() {
        return;
    }
    if retiring.as_ref().is_some_and(|retiring| retiring.busy()) {
        if *waiting_for_retirement != Some(request.request_id) {
            diag::info!(
                World,
                "match load: request #{} waits for retired resources",
                request.request_id
            );
            *waiting_for_retirement = Some(request.request_id);
        }
        return;
    }
    if *waiting_for_retirement == Some(request.request_id) {
        diag::info!(
            World,
            "match load: request #{} resumes after retirement",
            request.request_id
        );
    }
    *waiting_for_retirement = None;
    let zone_ff = request.zone_ff.clone();
    let common_mp = request.common_mp.clone();
    let progress = request.progress.clone();
    let request_id = request.request_id;
    let load_key = request.load_key;
    let zone = request.zone.clone();
    let zone_accepted = zone.clone();
    let zone_task = zone.clone();
    let cancel_handle = progress.clone();
    // The process outlives the overlay and every task this spawns: it is what
    // the table reads, what admission finishes, and what the bench still has
    // once the screen is gone.
    commands.insert_resource(MapLoadProcess::new(
        request_id,
        load_key,
        zone.clone(),
        cancel_handle.clone(),
    ));

    *early_map_sound() = Some(EarlyMapSound::Wanted(request_id));
    let task = crate::session_load::load_pool().spawn(async move {
        // (A guest map's pictures from its one-time bake, or baked now.)
        if let Ok(path) = &zone_ff {
            asset_material::map_pak::begin(request_id, path);
        }
        match load_prepared_match(zone_ff, common_mp, progress).await {
            MatchLoadOutcome::Ready(prepared) => Some(PreparedMatchReady {
                request_id,
                load_key,
                zone,
                prepared,
            }),
            MatchLoadOutcome::Canceled => None,
        }
    });
    inflight.0 = true;
    commands.insert_resource(MatchLoadAccepted {
        request_id,
        load_key,
        zone: zone_accepted,
    });
    commands.insert_resource(MatchLoadTask {
        task,
        request_id,
        load_key,
        zone: zone_task,
        progress: cancel_handle,
        sound_sent: false,
    });
    commands.remove_resource::<MatchLoadRequest>();
}

/// The maps that load with no common zone of their game: Black Ops II's
/// (bo2zm: as at launch; its guns, scripts and UI are its own, even with MW2
/// installed beside it), Call of Duty 4's and World at War's (bo2mp: by
/// Black Ops II's rules), and a server pack's (bo2mp).
fn loads_without_common(zone_ff: &std::path::Path) -> bool {
    asset_transport::zone_game_for_path(zone_ff) == Some(asset_transport::ZoneGame::T6)
        || asset_transport::legacy_game_for_path(zone_ff).is_some()
        || server_pack::is_pack_path(zone_ff)
}

/// The common zone a load of `zone_ff` takes (`Err` for a map that takes
/// none, or when it is not installed). bo2mp (bake all): the bake picks it
/// as the game does.
pub fn match_common_mp(games: &asset_transport::GamesRoot, zone_ff: &std::path::Path) -> Result<PathBuf, String> {
    if loads_without_common(zone_ff) {
        return Err("this map loads with no common zone of its game".to_owned());
    }
    asset_transport::find_runtime_common_mp(games, zone_ff).map(|zone| zone.path)
}

fn approve_map_load(
    mut commands: Commands,
    mut approved: MessageReader<MapLoadApproved>,
    mut failed: MessageWriter<MapLoadFailed>,
    identity: Res<LaunchIdentity>,
    abort: Option<Res<MatchLoadAbort>>,
) {
    for request in approved.read() {
        if abort
            .as_ref()
            .is_some_and(|abort| abort.0 == request.request_id)
        {
            diag::info!(
                World,
                "match load: abort approved request #{} (`{}`) — no walk",
                request.request_id,
                request.zone
            );
            continue;
        }
        let games = asset_transport::GamesRoot(identity.games_root.clone());
        let found = asset_transport::find_zone_file(&games, &request.zone);
        let zone = found
            .as_ref()
            .map(|zone| zone.zone_name.clone())
            .unwrap_or_else(|_| request.zone.clone());
        // bo2mp: a dedicated server without the map's game installed hosts
        // it from its pack (CODLEGACY_SERVER_PACKS).
        let zone_ff = match (found, server_pack::find_pack(&request.zone)) {
            (Err(_), Some(pack)) => Ok(pack),
            (found, _) => found.map(|zone| zone.path),
        };
        let packed = zone_ff.as_ref().is_ok_and(|path| server_pack::is_pack_path(path));
        let zone = if packed { server_pack::split_key(&request.zone).1 } else { zone };
        let t6 = zone_ff.as_ref().is_ok_and(|path| loads_without_common(path));
        let common_mp = match &zone_ff {
            Ok(path) => match_common_mp(&games, path),
            Err(error) => Err(error.clone()),
        };
        let error = zone_ff
            .as_ref()
            .err()
            .or_else(|| common_mp.as_ref().err().filter(|_| !t6))
            .cloned();
        if let Some(error) = error {
            failed.write(MapLoadFailed {
                request_id: request.request_id,
                load_key: request.load_key,
                zone,
                error,
            });
            continue;
        }
        // Its own state, never the live map's: tasks from the request before
        // this one still hold handles, and they must not land in these rows.
        let progress = LoadProgress::new(request.request_id);
        commands.insert_resource(MatchLoadRequest {
            request_id: request.request_id,
            load_key: request.load_key,
            zone,
            zone_ff,
            common_mp,
            progress,
        });
    }
}

fn poll_match_load(
    mut commands: Commands,
    mut task: Option<ResMut<MatchLoadTask>>,
    mut inflight: ResMut<MatchLoadBusy>,
    abort: Option<Res<MatchLoadAbort>>,
) {
    let Some(task) = task.as_deref_mut() else {
        return;
    };

    if abort
        .as_ref()
        .is_some_and(|abort| abort.0 == task.request_id)
        && !task.progress.is_canceled()
    {
        task.progress.cancel();
        diag::info!(
            World,
            "match load: cancel in-flight walk request #{} — the walk returns at its next checkpoint",
            task.request_id
        );
    }
    if !task.sound_sent && !task.progress.is_canceled() {
        let mut slot = early_map_sound();
        if matches!(*slot, Some(EarlyMapSound::Sent(id, ..)) if id == task.request_id)
            && let Some(EarlyMapSound::Sent(_, common_profile_id, products_id, sound)) = slot.take()
        {
            task.sound_sent = true;
            diag::info!(
                World,
                "match load: map sound for `{}` handed to audio before the walk ends",
                task.zone
            );
            commands.insert_resource(PreparedMatchSound {
                load_key: task.load_key,
                zone: task.zone.clone(),
                common_profile_id,
                products_id,
                sound,
            });
        }
    }
    let Some(outcome) = future::block_on(future::poll_once(&mut task.task)) else {
        return;
    };
    let request_id = task.request_id;
    let sound_sent = task.sound_sent;
    {
        let mut slot = early_map_sound();
        if matches!(*slot, Some(EarlyMapSound::Wanted(id) | EarlyMapSound::Sent(id, ..)) if id == request_id) {
            *slot = None;
        }
    }
    inflight.0 = false;
    commands.remove_resource::<MatchLoadTask>();
    commands.remove_resource::<MatchLoadAccepted>();
    let Some(mut ready) = outcome else {
        if sound_sent {
            commands.remove_resource::<PreparedMatchSound>();
        }
        diag::info!(
            World,
            "match load: walk request #{request_id} returned canceled — nothing prepared"
        );
        return;
    };
    if abort.is_some_and(|abort| abort.0 == ready.request_id) {
        if sound_sent {
            commands.remove_resource::<PreparedMatchSound>();
        }
        diag::info!(
            World,
            "match load: discarded aborted walk request #{} (`{}`)",
            ready.request_id,
            ready.zone
        );
        return;
    }
    if sound_sent {
        // (Its copy went to audio during the walk.)
        ready.prepared.sound = None;
        commands.insert_resource(ready);
        return;
    }
    commands.insert_resource(PreparedMatchSound {
        load_key: ready.load_key,
        zone: ready.zone.clone(),
        common_profile_id: ready.prepared.materials.common_profile_id,
        products_id: ready.prepared.materials.products_id,
        sound: ready
            .prepared
            .sound
            .take()
            .unwrap_or_else(|| Err("the map zone never opened".to_owned())),
    });
    commands.insert_resource(ready);
}

fn expire_match_load_abort(
    mut commands: Commands,
    abort: Option<Res<MatchLoadAbort>>,
    task: Option<Res<MatchLoadTask>>,
    request: Option<Res<MatchLoadRequest>>,
    ready: Option<Res<PreparedMatchReady>>,
) {
    if abort.is_none() {
        return;
    }
    if task.is_some() || request.is_some() || ready.is_some() {
        return;
    }
    commands.remove_resource::<MatchLoadAbort>();
}

pub fn register_match_load_systems(app: &mut App) {
    app.init_resource::<MatchLoadBusy>().add_systems(
        Update,
        (
            approve_map_load.in_set(MapLoadApproval),
            (
                start_match_load,
                poll_match_load.after(start_match_load),
                expire_match_load_abort.after(poll_match_load),
            )
                .in_set(MatchLoadDispatch)
                .after(MapLoadApproval),
        )
            .after(frame::SessionSwapApplied)
            .in_set(ClientSet::Load),
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The walk's early sound lands only in the slot its own request asked
    /// for: a probe's walk, or the request before, leaves it alone.
    #[test]
    fn early_map_sound_fills_only_the_wanted_request() {
        let sound = Ok(asset_audio::SoundCatalog::default());
        *early_map_sound() = None;
        send_map_sound_early(7, 1, 2, &sound);
        assert!(early_map_sound().is_none(), "nobody asked");
        *early_map_sound() = Some(EarlyMapSound::Wanted(7));
        send_map_sound_early(6, 1, 2, &sound);
        assert!(matches!(*early_map_sound(), Some(EarlyMapSound::Wanted(7))), "another request's walk");
        send_map_sound_early(7, 1, 2, &sound);
        assert!(matches!(*early_map_sound(), Some(EarlyMapSound::Sent(7, 1, 2, Ok(_)))));
        *early_map_sound() = None;
    }
}
