use std::collections::HashMap;
use std::sync::Arc;

use asset_iw4::attenuate;
use asset_core::AssetNamespace;
use asset_audio::{
    LoadedSoundBank, SoundCatalog, compose_sound_bank, gather_sound_sources, namespace_for_zone,
};
use asset_transport::GamesRoot;
use assets::{NamespaceSoundIwd, NamespaceTrees};
use bevy::{
    audio::{AudioSink, AudioSinkPlayback, Volume},
    prelude::*,
    tasks::{AsyncComputeTaskPool, Task, TaskPool, futures_lite::future},
};
use frame::{MatchTornDown, ReturnedToMenu};

use crate::backend::{AudioScope, MatchEpoch, Voice};
use crate::pcm::{LoopingPcmAudio, PcmAudio};
use crate::playback::{
    AmbientListener, MissingAliasGaps, SharedPlayAssets, SoundBank, world_oneshot_channel_gains,
};
use crate::space::{distance_inches, transform_inches};

#[derive(Component)]
pub struct MapAmbient;

/// A bed's rms the beds are brought to (about -38 dBFS), the most they are
/// raised, and the volume they play at.
const BED_TARGET_RMS: f32 = 0.0126;
const BED_MAX_GAIN: f32 = 8.0;
const BED_VOLUME: f32 = 0.6;

pub const MAX_ACTIVE_MAP_EMITTERS: usize = 8;

pub const MIN_AUDIBLE_EMITTER_GAIN: f32 = 0.002;

#[derive(Component)]
pub struct MapEmitter {
    pub origin_inches: [f32; 3],
    pub dist_min: f32,
    pub dist_max: f32,
    pub knots: Arc<[[f32; 2]]>,
    pub base_gain: f32,
    pub pcm: Handle<PcmAudio>,
    /// Playback speed (pitch scale; 1 = as recorded): a vehicle engine's
    /// loop follows its speed (BO2's `setloopstate`).
    pub speed: f32,

    pub live_pan: Option<crate::pcm::LivePan>,
    /// bo2zm: a line emitter's other end (heard from the nearest point).
    pub line_end_inches: Option<[f32; 3]>,
}

impl MapEmitter {
    /// Where the listener hears it from: its place, or a line emitter's
    /// point nearest the listener (`closest_point_on_line_to_point`).
    pub fn heard_from(&self, ear_inches: [f32; 3]) -> [f32; 3] {
        let Some(b) = self.line_end_inches else {
            return self.origin_inches;
        };
        let a = self.origin_inches;
        let ab = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
        let len2 = ab[0] * ab[0] + ab[1] * ab[1] + ab[2] * ab[2];
        if len2 <= f32::EPSILON {
            return a;
        }
        let t = (((ear_inches[0] - a[0]) * ab[0]
            + (ear_inches[1] - a[1]) * ab[1]
            + (ear_inches[2] - a[2]) * ab[2])
            / len2)
            .clamp(0.0, 1.0);
        [a[0] + ab[0] * t, a[1] + ab[1] * t, a[2] + ab[2] * t]
    }
}

#[derive(Resource, Clone)]
pub struct SoundIwd(pub Arc<NamespaceSoundIwd>);

#[derive(Resource, Default)]
pub struct MapAmbientBooted(pub bool);

/// bo2zm: a Black Ops II map's now-and-then sounds (Nuketown's thunder):
/// each waits a whole number of seconds in its range (`randomintrange`),
/// then plays at one of its places.
pub(crate) fn play_random_point_sounds(
    time: Res<Time>,
    bank: Option<Res<SoundBank>>,
    booted: Res<MapAmbientBooted>,
    epoch: Res<MatchEpoch>,
    mut timers: Local<(u64, Vec<f64>, u32)>,
    mut play: MessageWriter<crate::AliasCommand>,
) {
    if !booted.0 {
        return;
    }
    let Some(bank) = bank else {
        return;
    };
    let sounds = bank.0.random_point_sounds();
    if sounds.is_empty() {
        return;
    }
    let next = |rng: &mut u32| {
        *rng ^= *rng << 13;
        *rng ^= *rng >> 17;
        *rng ^= *rng << 5;
        *rng
    };
    let wait = |s: &asset_audio::RandomPointSound, rng: &mut u32| {
        let lo = s.wait_secs[0].max(0.0) as u32;
        let hi = (s.wait_secs[1].max(0.0) as u32).max(lo + 1);
        f64::from(lo + next(rng) % (hi - lo))
    };
    let now = time.elapsed_secs_f64();
    if timers.0 != epoch.0 || timers.1.len() != sounds.len() {
        timers.0 = epoch.0;
        timers.2 = 0x9e37_79b9 ^ (now.to_bits() as u32);
        let mut rng = timers.2;
        timers.1 = sounds.iter().map(|s| now + wait(s, &mut rng)).collect();
        timers.2 = rng;
    }
    let mut rng = timers.2;
    for (i, s) in sounds.iter().enumerate() {
        if now < timers.1[i] || s.places_inches.is_empty() {
            continue;
        }
        let place = s.places_inches[next(&mut rng) as usize % s.places_inches.len()];
        // (An alias another game's map names carries its prefix: `h1:`.)
        let (namespace, alias) = asset_game::alias_namespace(AssetNamespace::T6, &s.soundalias);
        play.write(crate::AliasCommand::Play(crate::PlayAlias {
            namespace,
            alias: alias.to_owned(),
            fallback: None,
            origin_inches: Some(place),
            snd_ent: None,
        }));
        timers.1[i] = now + wait(s, &mut rng);
    }
    timers.2 = rng;
}

#[derive(Resource, Default)]
pub(crate) struct SoundBankLoadAttempted(pub bool);

const SOUND_BANK_COMPOSE_STALL: std::time::Duration = std::time::Duration::from_secs(20);

#[derive(Resource)]
pub(crate) struct SoundBankCompose {
    load_key: frame::LocalLoadKey,
    zone: String,
    iwd: IwdOpen,
    bank: Option<Task<ComposedBank>>,
    common_profile_id: u64,
    products_id: u64,
    started: std::time::Instant,
    stall_reported: bool,
}

enum IwdOpen {
    Opening(Task<(NamespaceSoundIwd, Vec<String>)>),
    Open(Arc<NamespaceSoundIwd>),
}

struct ComposedBank {
    loaded: Result<(Arc<SoundCatalog>, usize), String>,
    namespace: AssetNamespace,
    reused: bool,
}

#[derive(Resource, Default)]
pub(crate) struct ResidentSoundBank(Option<ResidentBank>);

struct ResidentBank {
    zone: String,
    iwd: Arc<NamespaceSoundIwd>,
    bank: Option<ResidentComposed>,
}

struct ResidentComposed {
    products_id: u64,
    common_profile_id: u64,
    catalog: Arc<SoundCatalog>,
    namespace: AssetNamespace,
    gaps: usize,
}

#[derive(Resource)]
pub(crate) struct SoundBankNamespace {
    pub(crate) zone: String,
    pub(crate) namespace: AssetNamespace,
}

pub fn stop_map_ambient(commands: &mut Commands, ambient: &Query<Entity, With<MapAmbient>>) {
    for entity in ambient.iter() {
        commands.entity(entity).despawn();
    }
}

pub(crate) fn stop_map_ambient_on_match_end(
    mut torn: MessageReader<MatchTornDown>,
    mut returned: MessageReader<ReturnedToMenu>,
    ambient: Query<Entity, With<MapAmbient>>,
    mut booted: ResMut<MapAmbientBooted>,
    mut attempted: ResMut<SoundBankLoadAttempted>,
    mut epoch: ResMut<MatchEpoch>,
    compose: Option<Res<SoundBankCompose>>,
    accepted: Option<Res<assets::MatchLoadAccepted>>,
    mut commands: Commands,
) {
    let retired: Vec<_> = torn.read().map(|fact| fact.world_generation).collect();
    let left_session = returned.read().count() > 0;
    if retired.is_empty() && !left_session {
        return;
    }
    epoch.bump();
    stop_map_ambient(&mut commands, &ambient);
    booted.0 = false;
    commands.remove_resource::<asset_audio::CreateFxOneshotEmitters>();

    let compose_is_for_the_incoming_map = !left_session
        && compose
            .as_ref()
            .zip(accepted.as_ref())
            .is_some_and(|(compose, accepted)| {
                compose.load_key == accepted.load_key
                    && !retired.contains(&frame::WorldGeneration::from_install(
                        compose.load_key.local_load_request_id,
                    ))
            });
    if !compose_is_for_the_incoming_map {
        commands.remove_resource::<SoundBankCompose>();
        commands.remove_resource::<assets::PreparedMatchSound>();
        attempted.0 = false;
    }
    commands.queue(|world: &mut bevy::prelude::World| {
        frame::retire::retire_resources(world, |batch| {
            batch
                .resource::<SoundBankNamespace>()
                .resource::<SoundBank>()
                .resource::<SoundIwd>()
                .resource::<crate::ClipStore>();
        });
    });
    perf::ambient_hold(i64::from(booted.0));
    diag::info!(Audio, "audio: map ambient stopped");
}

/// bo2mp: the BO2 map zone whose neighbours give a map from a game the sound
/// reader cannot open its common sound zones.
const BO2_SOUND_ANCHOR: &str = "mp_la";

pub(crate) fn start_sound_bank_compose(
    mut attempted: ResMut<SoundBankLoadAttempted>,
    accepted: Option<Res<assets::MatchLoadAccepted>>,
    abort: Option<Res<assets::MatchLoadAbort>>,
    identity: Option<Res<frame::LaunchIdentity>>,
    silent: Option<Res<crate::AudioSilent>>,
    mut resident: ResMut<ResidentSoundBank>,
    resident_clips: Res<crate::clip_store::ResidentClipCache>,
    mut commands: Commands,
) {
    if attempted.0 {
        return;
    }
    if silent.is_some() && !crate::AudioSilent::composes() {
        attempted.0 = true;
        return;
    }
    let Some(accepted) = accepted else {
        return;
    };
    if abort.is_some_and(|abort| abort.0 == accepted.request_id) {
        return;
    }
    let Some(identity) = identity else {
        return;
    };
    if accepted.zone.is_empty() || identity.games_root.as_os_str().is_empty() {
        return;
    }
    attempted.0 = true;
    let games = GamesRoot(identity.games_root.clone());
    let zone = accepted.zone.clone();
    let opened_zone = zone.clone();

    if resident.0.as_ref().is_some_and(|resident| resident.zone != zone) {
        // The map just played's bank and decoded clips stayed until the next
        // bank was composed, after the whole walk; another map reuses none
        // of them, so they go as its load starts (the common clips stay).
        resident.0 = None;
        resident_clips.drop_map_clips();
    }
    let iwd = match resident.0.as_ref().filter(|resident| resident.zone == zone) {
        Some(resident) => {
            diag::info!(Audio, "audio: sound archives for `{zone}` reused");
            IwdOpen::Open(Arc::clone(&resident.iwd))
        }
        None => {
            let pool = AsyncComputeTaskPool::get_or_init(TaskPool::default);
            IwdOpen::Opening(pool.spawn(async move {
                let mut trees = NamespaceTrees::discover(&games);
                if let Ok(zone) = asset_transport::find_zone_file(&games, &opened_zone) {
                    trees.adopt_zone(&zone.path);
                }
                // The MW3 announcer's streamed lines, from the voice-only folder.
                if trees.get(AssetNamespace::Iw5).is_none()
                    && let Some(voice) = asset_audio::voice_games_root()
                {
                    let voice = NamespaceTrees::discover(&voice);
                    if let Some(mw3) = voice.get(AssetNamespace::Iw5) {
                        trees.adopt_zone(&mw3.anchor);
                    }
                }
                let (mut iwd, mut lines) = NamespaceSoundIwd::open(&trees);
                // bo2mp: World at War's IWDs, for its guest guns' streamed
                // sounds (nothing when it is not installed).
                if assets::guest_guns_for("t4")
                    && let Some(main) = asset_audio::waw_main(&games)
                {
                    lines.push(iwd.open_t4(&main));
                }
                (iwd, lines)
            }))
        }
    };
    commands.insert_resource(SoundBankCompose {
        load_key: accepted.load_key,
        zone,
        iwd,
        bank: None,
        common_profile_id: 0,
        products_id: 0,
        started: std::time::Instant::now(),
        stall_reported: false,
    });
}

pub(crate) fn install_sound_bank(
    mut compose: Option<ResMut<SoundBankCompose>>,
    identity: Option<Res<frame::LaunchIdentity>>,
    accepted: Option<Res<assets::MatchLoadAccepted>>,
    abort: Option<Res<assets::MatchLoadAbort>>,
    mut map_sound: Option<ResMut<assets::PreparedMatchSound>>,
    mut epoch: ResMut<MatchEpoch>,
    resident_clips: Res<crate::clip_store::ResidentClipCache>,
    mut resident: ResMut<ResidentSoundBank>,
    mut commands: Commands,
) {
    let Some(compose) = compose.as_deref_mut() else {
        return;
    };

    let superseded = accepted
        .as_ref()
        .is_some_and(|accepted| accepted.load_key != compose.load_key);
    if abort.is_some_and(|abort| abort.0 == compose.load_key.local_load_request_id) || superseded {
        diag::info!(
            Audio,
            "audio: sound bank for `{}` dropped — the session moved on",
            compose.zone
        );
        commands.remove_resource::<SoundBankCompose>();
        return;
    }
    // Install on the load key, not `identity.zone`: gating on a display spelling
    // that never converges would hold AudioReady and the world spawn forever.
    let identity_zone = identity.as_ref().map(|identity| identity.zone.as_str());
    if let Some(zone) = identity_zone.filter(|zone| *zone != compose.zone) {
        diag::warn!(
            Audio,
            "audio: launch identity says `{zone}` while the accepted load walked `{}` — installing on the load key",
            compose.zone
        );
    }

    if let IwdOpen::Opening(task) = &mut compose.iwd {
        if let Some((indices, lines)) = future::block_on(future::poll_once(task)) {
            for line in lines {
                diag::info!(Audio, "{line}");
            }
            compose.iwd = IwdOpen::Open(Arc::new(indices));
        }
    }
    if compose.bank.is_none() {
        if let Some(arrived) = map_sound.as_deref_mut() {
            if arrived.load_key == compose.load_key {
                compose.common_profile_id = arrived.common_profile_id;
                let products_id = arrived.products_id;
                let map = std::mem::replace(&mut arrived.sound, Err(String::new()));
                commands.remove_resource::<assets::PreparedMatchSound>();
                let kept = resident
                    .0
                    .as_ref()
                    .filter(|resident| resident.zone == compose.zone)
                    .and_then(|resident| resident.bank.as_ref())
                    .filter(|bank| {
                        bank.products_id == products_id
                            && bank.common_profile_id == compose.common_profile_id
                            // bo2mp: a Resurgence match wants its MW3 (2023) sounds in.
                            && (!sim::resurgence_on() || asset_audio::has_jup_sounds(&bank.catalog))
                    })
                    .map(|bank| ComposedBank {
                        loaded: Ok((Arc::clone(&bank.catalog), bank.gaps)),
                        namespace: bank.namespace,
                        reused: true,
                    });
                let games = identity
                    .as_ref()
                    .map(|identity| GamesRoot(identity.games_root.clone()));
                let zone = compose.zone.clone();
                let pool = AsyncComputeTaskPool::get_or_init(TaskPool::default);
                compose.bank = Some(pool.spawn(async move {
                    if let Some(kept) = kept {
                        return kept;
                    }
                    let Some(games) = games else {
                        return ComposedBank {
                            loaded: Err("no launch identity to find the zones by".to_owned()),
                            namespace: AssetNamespace::Iw4,
                            reused: false,
                        };
                    };
                    // bo2mp: a map whose zone the sound reader cannot open
                    // (World at War's and Call of Duty 4's, read by their
                    // own lane) is played by Black Ops II's rules: its bank
                    // is Black Ops II's own common sounds, found beside
                    // BO2's Aftermath. Failing here left AudioReady unset
                    // and the load sat on "admission" for good.
                    let (found, namespace) = match asset_transport::find_zone_file(&games, &zone) {
                        Ok(found) => (Ok(found), namespace_for_zone(&games, &zone)),
                        Err(error) => match asset_transport::find_zone_file(&games, BO2_SOUND_ANCHOR) {
                            Ok(anchor) => {
                                diag::info!(
                                    Audio,
                                    "audio: `{zone}` has no zone the sound reader opens ({error}); its bank is Black Ops II's common sounds"
                                );
                                (Ok(anchor), AssetNamespace::T6)
                            }
                            Err(_) => (Err(error), AssetNamespace::Iw4),
                        },
                    };
                    let loaded = found.map(|found| {
                        // bo2mp: Resurgence takes in Modern Warfare III's (2023)
                        // sounds, Rebirth Island's own zone with them (its
                        // ambience plays on whatever map the mode is on).
                        asset_audio::want_jup_sounds(sim::resurgence_on(), true);
                        // (And a Modern Warfare III map's own ambience beds.)
                        asset_audio::want_jup_map(Some(&zone));
                        // bo2mp: a Black Ops III map's own ambience (its
                        // bank's aliases, rooms and room tones, its sound
                        // structs: `asset_audio::bo3_map_sounds`).
                        let map = match (namespace, map) {
                            (AssetNamespace::T7, map) if asset_audio::t7_map_ambience_enabled() => match asset_audio::bo3_map_sounds(&found.path) {
                                Ok(mut bo3) => {
                                    diag::info!(Audio, "t7 ambience: {zone}: the map's own sound bank, rooms and emitters");
                                    match map {
                                        Ok(mut m) => {
                                            m.scripted_map_fx = bo3.scripted_map_fx.take();
                                            m.absorb_missing_aliases_unresolved(bo3);
                                            Ok(m)
                                        }
                                        Err(_) => Ok(bo3),
                                    }
                                }
                                Err(e) => {
                                    diag::warn!(Audio, "t7 ambience: {zone}: not read ({e})");
                                    map
                                }
                            },
                            // bo2mp: an Infinite Warfare map's own ambience (its
                            // bank's aliases and its sound zones' room tones:
                            // `asset_audio::iw7_map_sounds`).
                            (_, map) if asset_transport::legacy_game_for_path(&found.path) == Some("iw7") && asset_audio::iw7_map_ambience_enabled() => match asset_audio::iw7_map_sounds(&found.path) {
                                Ok(mut iw7) => {
                                    diag::info!(Audio, "iw7 ambience: {zone}: the map's own sound bank and sound zones");
                                    match map {
                                        Ok(mut m) => {
                                            m.scripted_map_fx = iw7.scripted_map_fx.take();
                                            m.absorb_missing_aliases_unresolved(iw7);
                                            Ok(m)
                                        }
                                        Err(_) => Ok(iw7),
                                    }
                                }
                                Err(e) => {
                                    diag::warn!(Audio, "iw7 ambience: {zone}: not read ({e})");
                                    map
                                }
                            },
                            (_, map) => map,
                        };
                        let sources = gather_sound_sources(&games, &found.path);
                        let LoadedSoundBank { catalog, gaps, .. } =
                            compose_sound_bank(sources, &zone, namespace, map);
                        (Arc::new(catalog), gaps.len())
                    });
                    ComposedBank {
                        loaded,
                        namespace,
                        reused: false,
                    }
                }));
                compose.products_id = products_id;
            }
        }
    }

    let composed = match (&compose.iwd, compose.bank.as_mut()) {
        (IwdOpen::Open(_), Some(bank)) => future::block_on(future::poll_once(bank)),
        _ => None,
    };
    let Some(composed) = composed else {
        if !compose.stall_reported && compose.started.elapsed() >= SOUND_BANK_COMPOSE_STALL {
            compose.stall_reported = true;
            diag::warn!(
                Audio,
                "audio: sound bank for `{}` still not composed after {:.0}s (archives {}, map sound {}) — AudioReady, the world spawn and host admission all wait on it",
                compose.zone,
                compose.started.elapsed().as_secs_f32(),
                if matches!(compose.iwd, IwdOpen::Open(_)) {
                    "open"
                } else {
                    "opening"
                },
                if compose.bank.is_some() {
                    "arrived"
                } else {
                    "pending"
                },
            );
        }
        return;
    };
    commands.remove_resource::<SoundBankCompose>();
    let IwdOpen::Open(iwd) = std::mem::replace(&mut compose.iwd, IwdOpen::Open(Arc::default()))
    else {
        unreachable!("the bank is polled only once the archives are open");
    };
    let ComposedBank {
        loaded,
        namespace,
        reused,
    } = composed;
    match loaded {
        Ok((bank, gaps)) => {
            epoch.bump();
            if iwd.is_empty() {
                diag::warn!(
                    Audio,
                    "audio: no IWD sound archives for `{}` — streamed aliases will gap",
                    compose.zone
                );
            }
            commands.insert_resource(SoundIwd(Arc::clone(&iwd)));
            resident.0 = Some(ResidentBank {
                zone: compose.zone.clone(),
                iwd: Arc::clone(&iwd),
                bank: Some(ResidentComposed {
                    products_id: compose.products_id,
                    common_profile_id: compose.common_profile_id,
                    catalog: Arc::clone(&bank),
                    namespace,
                    gaps,
                }),
            });

            let new_clips = crate::ClipStore::start_with_common(
                Arc::clone(&bank),
                Some(iwd),
                compose.common_profile_id,
                Some(resident_clips.clone()),
            );
            commands.queue(move |world: &mut World| {
                frame::retire::retire_resources(world, |batch| {
                    batch.resource::<crate::ClipStore>();
                });
                world.insert_resource(new_clips);
            });
            commands.insert_resource(crate::clip_store::PendingStarts::default());
            commands.insert_resource(crate::playback::SharedPlayAssets::default());
            commands.insert_resource(SoundBank(bank));
            diag::info!(
                Audio,
                "audio: sound bank {} for {} ({} zone gaps) in {:.0}ms",
                if reused { "reused" } else { "ready" },
                compose.zone,
                gaps,
                compose.started.elapsed().as_secs_f32() * 1000.0
            );
            commands.insert_resource(SoundBankNamespace {
                zone: compose.zone.clone(),
                namespace,
            });
        }
        Err(e) => {
            diag::warn!(
                Audio,
                "audio: sound bank load failed for {}: {e}",
                compose.zone
            );
            // No namespace for this load: the match set sees the bank
            // failed and lets the match go on silent.
            commands.remove_resource::<SoundBankNamespace>();
        }
    }
}

pub(crate) fn boot_map_ambient_once(
    mut booted: ResMut<MapAmbientBooted>,
    loading: Option<Res<assets::LoadingScreen>>,
    bank: Option<Res<SoundBank>>,
    identity: Option<Res<frame::LaunchIdentity>>,
    script_sound: Option<Res<asset_audio::SessionMapScriptSound>>,
    namespace: Option<Res<SoundBankNamespace>>,
    epoch: Res<MatchEpoch>,
    clips: Option<Res<crate::ClipStore>>,
    ready: Res<crate::AudioReady>,
    mut commands: Commands,
    mut pcm_assets: ResMut<Assets<PcmAudio>>,
    mut looping_assets: ResMut<Assets<LoopingPcmAudio>>,
    mut shared: ResMut<SharedPlayAssets>,
    mut gaps: ResMut<MissingAliasGaps>,
) {
    if booted.0 {
        return;
    }

    if loading.is_some_and(|screen| !screen.is_complete()) {
        return;
    }
    if !ready.0 {
        return;
    }
    let Some(bank) = bank else {
        return;
    };
    let Some(identity) = identity else {
        return;
    };
    if identity.zone.is_empty() {
        return;
    }
    let Some(namespace) = namespace.filter(|ns| ns.zone == identity.zone) else {
        return;
    };
    let ambient_alias = script_sound
        .as_deref()
        .and_then(|facts| facts.0.ambient_alias.as_deref());
    start_map_ambient_prepared(
        &mut commands,
        &mut pcm_assets,
        &mut looping_assets,
        &mut shared,
        bank.0.as_ref(),
        namespace.namespace,
        &identity.zone,
        ambient_alias,
        script_sound.as_deref().map_or(&[][..], |facts| facts.0.beds.as_slice()),
        clips.as_deref(),
        epoch.0,
        &mut gaps,
    );
    booted.0 = true;
    perf::ambient_boot(&identity.zone, ambient_alias);
    diag::info!(
        Audio,
        "audio: map ambient boot complete for {}",
        identity.zone
    );
}

/// bo2zm: with the sound off (`CODLEGACY_SOUND=off`, every lane's test game) no
/// sound bank is built, but a Black Ops II map's placed effects (fires,
/// smoke, sparks: its createfx list, read with the sound data) still start,
/// straight from the loaded map's sound data. Nothing plays.
pub(crate) fn boot_silent_map_effects(
    silent: Option<Res<crate::AudioSilent>>,
    map_sound: Option<Res<assets::PreparedMatchSound>>,
    mut done: Local<Option<frame::LocalLoadKey>>,
    mut commands: Commands,
) {
    let (Some(_), Some(map_sound)) = (silent, map_sound) else {
        return;
    };
    if *done == Some(map_sound.load_key) {
        return;
    }
    *done = Some(map_sound.load_key);
    let Some(fx) = map_sound
        .sound
        .as_ref()
        .ok()
        .and_then(|catalog| catalog.scripted_map_fx.as_ref())
    else {
        return;
    };
    diag::info!(
        Audio,
        "audio: Silent; createfx oneshots {} for {} (placed effects only)",
        fx.oneshots.len(),
        map_sound.zone
    );
    commands.insert_resource(asset_audio::CreateFxOneshotEmitters(fx.oneshots.clone()));
}

fn start_map_ambient_prepared(
    commands: &mut Commands,
    pcm_assets: &mut Assets<PcmAudio>,
    looping_assets: &mut Assets<LoopingPcmAudio>,
    shared: &mut SharedPlayAssets,
    bank: &SoundCatalog,
    map_ns: AssetNamespace,
    map_name: &str,
    ambient_alias: Option<&str>,
    beds: &[String],
    clips: Option<&crate::ClipStore>,
    epoch: u64,
    gaps: &mut MissingAliasGaps,
) {
    if let Some(alias) = ambient_alias {
        // bo2mp: an ambient bed of another game's names its game (`iw6:`).
        let (map_ns, alias) = asset_game::alias_namespace(map_ns, alias);
        if let Some(pcm) = pcm_for_map_alias(clips, bank, map_ns, alias) {
            let handle = looping_assets.add(pcm.into_looping());
            // bo2zm: a Black Ops II room tone plays at its alias's volume.
            // bo2mp: so does a Ghosts map's ambient bed (`ambient_mp_frag`:
            // 0.375), with its own map sounds on.
            let own_volume = map_ns == AssetNamespace::T6
                || (map_ns == AssetNamespace::Iw6 && asset_audio::iw6_rules::Iw6Switches::get().map_sounds);
            let volume = if own_volume {
                bank.sound_in(map_ns, alias)
                    .and_then(|s| s.aliases.first())
                    .map_or(0.55, |row| row.vol_min.max(0.0))
            } else {
                0.55
            };
            let entity = crate::backend::spawn_loop(
                commands,
                handle,
                Volume::Linear(volume),
                epoch,
                AudioScope::Match,
            );
            commands.entity(entity).insert(MapAmbient);
            diag::info!(Audio, "audio: ambient loop `{alias}`");
        } else {
            gaps.record(alias);
            diag::warn!(
                Audio,
                "audio: ambient alias `{alias}` unresolved for {map_name}"
            );
        }
    } else {
        diag::warn!(
            Audio,
            "audio: no ambientPlay in map script for `{map_name}` (typed gap)"
        );
    }

    // bo2mp: a Modern Warfare III map's ambience beds: quiet stereo loops
    // played together, each brought to a common level (they are recorded
    // from -60 to -30 dBFS).
    for bed in beds {
        let (ns, alias) = asset_game::alias_namespace(map_ns, bed);
        let Some(pcm) = pcm_for_map_alias(clips, bank, ns, alias) else {
            gaps.record(alias);
            diag::warn!(Audio, "audio: ambience bed `{bed}` unresolved for {map_name}");
            continue;
        };
        let volume = (BED_TARGET_RMS / pcm.rms().max(1e-4)).clamp(0.25, BED_MAX_GAIN) * BED_VOLUME;
        let handle = looping_assets.add(pcm.into_looping());
        let entity = crate::backend::spawn_loop(commands, handle, Volume::Linear(volume), epoch, AudioScope::Match);
        commands.entity(entity).insert(MapAmbient);
        diag::info!(Audio, "audio: ambience bed `{bed}` at {volume:.2}");
    }

    let loops = bank.createfx_loop_sounds(map_ns, map_name);
    let mut pcm_by_alias: HashMap<String, Handle<PcmAudio>> = HashMap::new();
    let mut started = 0usize;
    let mut missed = 0usize;
    for emitter in &loops {
        // (A loop another game's map names carries its prefix: `h1:`.)
        let (loop_ns, loop_alias) = asset_game::alias_namespace(map_ns, &emitter.soundalias);
        let handle = if let Some(handle) = pcm_by_alias.get(&emitter.soundalias) {
            handle.clone()
        } else {
            let Some(pcm) = pcm_for_map_alias(clips, bank, loop_ns, loop_alias) else {
                gaps.record(loop_alias);
                missed += 1;
                continue;
            };
            let handle = pcm_assets.add(pcm);
            pcm_by_alias.insert(emitter.soundalias.clone(), handle.clone());
            handle
        };
        let origin_inches = emitter.origin_inches;
        let row = bank
            .sound_in(loop_ns, loop_alias)
            .or_else(|| {
                bank.index_unique(loop_alias)
                    .and_then(|i| bank.sounds.get(i))
            })
            .and_then(|s| s.aliases.first());
        let (dist_min, dist_max, knots, base_gain) = if let Some(row) = row {
            let knots = row
                .volume_falloff
                .as_ref()
                .map(|c| shared.intern_curve(&c.name, &c.knots))
                .unwrap_or_else(|| Arc::from(Vec::<[f32; 2]>::new()));
            // (A Modern Warfare Remastered alias with no falloff curve of
            // its own fades straight from full at its near distance to
            // silence at its far one: INFERRED, the engine's default curve
            // is being read.)
            let knots = if knots.is_empty() && loop_ns == AssetNamespace::H1 && row.dist_max > row.dist_min {
                Arc::from(vec![[0.0, 1.0], [1.0, 0.0]])
            } else {
                knots
            };
            if knots.is_empty() {
                diag::warn!(
                    Audio,
                    "audio: CreateFX loop `{}` has no falloff curve (typed gap)",
                    loop_alias
                );
            }
            (row.dist_min, row.dist_max, knots, row.vol_min.max(0.0))
        } else {
            (0.0, 0.0, Arc::from(Vec::<[f32; 2]>::new()), 0.0)
        };
        commands.spawn((
            MapAmbient,
            MapEmitter {
                origin_inches,
                dist_min,
                dist_max,
                knots,
                base_gain,
                pcm: handle,
                speed: 1.0,
                live_pan: None,
                line_end_inches: emitter.line_end_inches,
            },
            Transform::from_translation(Vec3::from_array(origin_inches)),
        ));
        started += 1;
    }
    if started > 0 {
        diag::info!(
            Audio,
            "audio: createfx emitters {started}/{} ({} unique, max {} active; {missed} miss) for {map_name}",
            loops.len(),
            pcm_by_alias.len(),
            MAX_ACTIVE_MAP_EMITTERS,
        );
    } else if !loops.is_empty() {
        diag::info!(
            Audio,
            "audio: createfx listed {} emitters but none resolved PCM",
            loops.len()
        );
    }

    let oneshots = bank.createfx_oneshots(map_ns, map_name);
    let oneshot_count = oneshots.len();
    commands.insert_resource(asset_audio::CreateFxOneshotEmitters(oneshots));
    if oneshot_count > 0 {
        diag::info!(
            Audio,
            "audio: createfx oneshots {oneshot_count} parsed for {map_name} (host markers)"
        );
    }
}

pub(crate) fn emitter_gain(emitter: &MapEmitter, ear_inches: [f32; 3]) -> f32 {
    let dist = distance_inches(ear_inches, emitter.heard_from(ear_inches));
    if emitter.knots.is_empty() {
        return 0.0;
    }
    let atten = attenuate(&emitter.knots, dist, emitter.dist_min, emitter.dist_max);
    if atten < 0.0 {
        0.0
    } else {
        emitter.base_gain * atten
    }
}

pub fn update_map_emitter_gain(
    listeners: Query<&Transform, With<AmbientListener>>,
    mut emitters: Query<(Entity, &mut MapEmitter, Has<Voice>)>,
    mut sinks: Query<&mut AudioSink, With<MapEmitter>>,
    pcm_assets: Res<Assets<PcmAudio>>,
    mut looping_assets: ResMut<Assets<LoopingPcmAudio>>,
    mut commands: Commands,
    settings: Res<frame::GameSettings>,
    epoch: Res<MatchEpoch>,
    mut ranked: Local<Vec<(Entity, f32, Handle<PcmAudio>, [f32; 3], bool)>>,
) {
    let n = listeners.iter().len();
    if n == 0 {
        return;
    }
    if n > 1 {
        panic!("more than one ambient listener");
    }
    let Some(listener) = listeners.iter().next() else {
        return;
    };
    let ear = listener.translation;
    let r = listener.rotation * Vec3::X;
    let right = Vec3::new(r.x, r.y, r.z);
    let ear_inches = transform_inches(ear);

    ranked.clear();
    ranked.extend(emitters.iter().map(|(entity, emitter, has_player)| {
        (
            entity,
            emitter_gain(&emitter, ear_inches),
            emitter.pcm.clone(),
            emitter.heard_from(ear_inches),
            has_player,
        )
    }));
    ranked.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

    for (rank, (entity, gain, dry_pcm, origin, has_player)) in ranked.iter().enumerate() {
        let entity = *entity;
        let gain = *gain;
        let origin = *origin;
        let has_player = *has_player;
        let audible = rank < MAX_ACTIVE_MAP_EMITTERS && gain >= MIN_AUDIBLE_EMITTER_GAIN;
        let origin_v = Vec3::from_array(origin);
        let (pan_l, pan_r) = world_oneshot_channel_gains(ear, right, origin_v, 1.0);
        if audible {
            if !has_player {
                let Some(dry) = pcm_assets.get(dry_pcm).cloned() else {
                    continue;
                };
                let live = dry.with_live_pan();
                let live_pan = live.live_pan().expect("with_live_pan").clone();
                live_pan.set(pan_l, pan_r);
                let handle = looping_assets.add(live.into_looping());
                if let Ok((_, mut emitter, _)) = emitters.get_mut(entity) {
                    emitter.live_pan = Some(live_pan);
                }
                crate::backend::attach_loop(
                    &mut commands,
                    entity,
                    handle,
                    Volume::Linear(gain),
                    epoch.0,
                );
            } else {
                let mut speed = 1.0;
                if let Ok((_, emitter, _)) = emitters.get(entity) {
                    speed = emitter.speed;
                    if let Some(pan) = emitter.live_pan.as_ref() {
                        pan.set(pan_l, pan_r);
                    }
                }
                if let Ok(mut sink) = sinks.get_mut(entity) {
                    sink.set_volume(Volume::Linear(
                        gain * settings.master_volume * crate::bus_gain::sfx(),
                    ));
                    if (sink.speed() - speed).abs() > 0.001 {
                        sink.set_speed(speed);
                    }
                    if sink.is_paused() {
                        sink.play();
                    }
                }
            }
        } else if has_player {
            if let Ok((_, mut emitter, _)) = emitters.get_mut(entity) {
                emitter.live_pan = None;
            }
            crate::backend::detach_loop(&mut commands, entity);
        }
    }
}

fn pcm_for_map_alias(
    clips: Option<&crate::ClipStore>,
    bank: &SoundCatalog,
    ns: AssetNamespace,
    alias: &str,
) -> Option<PcmAudio> {
    let clips = clips?;
    for key in crate::clip_store::clip_keys_for_alias(bank, ns, alias) {
        if let Some(Ok(pcm)) = clips.ready(&key) {
            return Some(pcm);
        }
    }
    None
}

/// bo2mp: the room tone playing now (a Black Ops III map's rooms each carry
/// their own loop).
#[derive(Component)]
pub(crate) struct RoomTone;

/// bo2mp: a Black Ops III map's room tones (`ScriptedMapFx::room_tones`):
/// the tone of the room the server says the local player is in (client
/// dvar `bo2zm_room`, empty = the map's default room, the first) loops at
/// its alias's volume; entering a room with another tone (or none: the
/// underwater room) swaps it. Black Ops II maps have no room tones (their
/// one tone is the boot's `ambientplay`).
#[allow(clippy::too_many_arguments)]
pub(crate) fn update_room_tone(
    booted: Res<MapAmbientBooted>,
    bank: Option<Res<SoundBank>>,
    namespace: Option<Res<SoundBankNamespace>>,
    presented: Option<Res<net::PresentedSnapshot>>,
    local: Option<Res<net::LocalPresentClient>>,
    clips: Option<Res<crate::ClipStore>>,
    epoch: Res<MatchEpoch>,
    mut looping_assets: ResMut<Assets<LoopingPcmAudio>>,
    listeners: Query<&Transform, With<AmbientListener>>,
    mut playing: Local<Option<(u64, String, Option<Entity>)>>,
    mut commands: Commands,
) {
    if !booted.0 {
        *playing = None;
        return;
    }
    let (Some(bank), Some(namespace)) = (bank, namespace) else { return };
    let Some(fx) = bank.0.scripted_map_fx.as_ref().filter(|fx| !fx.room_tones.is_empty()) else { return };
    let room = presented
        .as_deref()
        .zip(local.as_deref())
        .and_then(|(p, l)| p.snapshot().map(|s| s.meta.script_dvars(l.0).string("bo2zm_room").unwrap_or_default()))
        .unwrap_or_default();
    // (No room from the server: an Infinite Warfare map's rooms are chosen
    // by where the listener stands, its audio client triggers.)
    let room = if room.is_empty() {
        listeners.iter().next().and_then(|t| fx.room_at(crate::space::transform_inches(t.translation))).map(str::to_owned).unwrap_or_default()
    } else {
        room.to_string()
    };
    let room = fx.rooms.iter().find(|r| !room.is_empty() && r.0 == room).or_else(|| fx.rooms.first()).map(|r| r.0.clone()).unwrap_or_default();
    let tone = fx.room_tones.iter().find(|(r, _)| *r == room).map(|(_, a)| a.clone());
    let key = tone.clone().unwrap_or_default();
    if playing.as_ref().is_some_and(|(e, k, _)| *e == epoch.0 && *k == key) {
        return;
    }
    // (A tone whose clip is not decoded yet is tried again next frame.)
    let pcm = match tone.as_deref() {
        Some(alias) => match pcm_for_map_alias(clips.as_deref(), &bank.0, bank.0.map_alias_ns(namespace.namespace, alias), alias) {
            Some(pcm) => Some((alias, pcm)),
            None => return,
        },
        None => None,
    };
    if let Some((_, _, Some(old))) = playing.take() {
        commands.entity(old).despawn();
    }
    let entity = pcm.map(|(alias, pcm)| {
        let volume = bank.0.sound_in(bank.0.map_alias_ns(namespace.namespace, alias), alias).and_then(|s| s.aliases.first()).map_or(0.55, |row| row.vol_min.max(0.0));
        let handle = looping_assets.add(pcm.into_looping());
        let entity = crate::backend::spawn_loop(&mut commands, handle, Volume::Linear(volume), epoch.0, AudioScope::Match);
        commands.entity(entity).insert((MapAmbient, RoomTone));
        diag::info!(Audio, "map ambience: room `{room}` tone `{alias}` at {volume:.2}");
        entity
    });
    if entity.is_none() {
        diag::info!(Audio, "map ambience: room `{room}` has no tone");
    }
    *playing = Some((epoch.0, key, entity));
}
