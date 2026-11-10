use std::collections::HashMap;
use std::f32::consts::FRAC_PI_4;
use std::sync::Arc;
use std::time::Instant;

use asset_audio::{SoundCatalog, lerp_range, unit_random};
use asset_core::AssetNamespace;
use asset_iw4::{SND_CURVE_MAX_KNOTS, has_free_voice};
use assets::NamespaceSoundIwd;
use bevy::{
    audio::{AddAudioSource, AudioSink, AudioSinkPlayback, Volume},
    prelude::*,
};
use frame::{ClientSet, FxSoundPublished, MatchTornDown, SessionSwapApplied};
use net::{LastAdoptedSnapshot, SvcLocalSound};

use crate::ambient::SoundIwd;
use crate::attenuation::distance_attenuation;
use crate::backend::MatchEpoch;
use crate::clip_store::{
    ClipError, ClipKey, ClipStore, PendingOneshot, PendingStarts, clip_key_for_variant,
    clip_keys_for_alias, deadline_for,
};
use crate::messages::{
    AliasCommand, BoundWeaponSound, Footstep, LandSound, PlayAlias, SND_ENT_LOCAL,
    ViewmodelNotetracks, WeaponSound,
};
use crate::pcm::{LoopingPcmAudio, PcmAudio};
use crate::space::{distance_inches, transform_inches};
use crate::start::{
    SoundClass, StartDecision, StartDecisions, StartFailure, StartOutcome, SuppressReason,
};
use crate::voice::{VoiceLease, VoiceOccupancy, reclaim_finished_voices};

#[derive(Component, Default)]
pub struct AmbientListener;

#[derive(Component)]
pub struct Channel3d {
    pub origin_inches: [f32; 3],
    pub dist_min: f32,
    pub dist_max: f32,
    pub knots: Arc<[[f32; 2]]>,
    pub near_knots: Option<Arc<[[f32; 2]]>>,
    pub priority: Option<asset_audio::VoicePriority>,
    pub base_volume: f32,

    pub live_pan: crate::pcm::LivePan,
}

#[derive(Resource, Clone)]
pub struct SoundBank(pub Arc<SoundCatalog>);

#[derive(Resource)]
pub struct SoundPickState {
    pub lcg: u32,
    pub last_variant: HashMap<(AssetNamespace, String), usize>,
}

impl Default for SoundPickState {
    fn default() -> Self {
        Self {
            lcg: 0x00a5_5a5a,
            last_variant: HashMap::new(),
        }
    }
}

#[derive(Component)]
pub struct AliasPlayback {
    pub namespace: AssetNamespace,
    pub snd_ent: Option<u32>,
    pub alias: String,
}

#[derive(Resource, Default, Debug)]
pub struct MissingAliasGaps {
    pub aliases: Vec<String>,
}

impl MissingAliasGaps {
    pub fn record(&mut self, alias: &str) {
        if !self.aliases.iter().any(|a| a == alias) {
            diag::warn!(Audio, "audio: missing alias `{alias}` (typed gap)");
            self.aliases.push(alias.to_owned());
        }
    }

    pub fn len(&self) -> usize {
        self.aliases.len()
    }

    pub fn is_empty(&self) -> bool {
        self.aliases.is_empty()
    }
}

#[derive(Resource, Default)]
pub(crate) struct SharedPlayAssets {
    dry: HashMap<ClipKey, Handle<PcmAudio>>,
    curves: HashMap<String, Arc<[[f32; 2]]>>,
    common_dry_reuses: usize,
}

impl SharedPlayAssets {
    fn dry_handle(
        &mut self,
        pcm_assets: &mut Assets<PcmAudio>,
        clip: &ClipKey,
        pcm: PcmAudio,
        clips: Option<&ClipStore>,
    ) -> Handle<PcmAudio> {
        if let Some(handle) = self.dry.get(clip) {
            return handle.clone();
        }
        let handle = if let Some((handle, reused)) =
            clips.and_then(|store| store.resident_dry_handle(clip, pcm_assets, &pcm))
        {
            if reused && self.common_dry_reuses == 0 {
                diag::info!(Audio, "audio: first common dry handle reused: {clip:?}");
            }
            self.common_dry_reuses += usize::from(reused);
            handle
        } else {
            pcm_assets.add(pcm)
        };
        self.dry.insert(clip.clone(), handle.clone());
        handle
    }

    pub(crate) fn intern_curve(&mut self, name: &str, knots: &[(f32, f32)]) -> Arc<[[f32; 2]]> {
        if let Some(existing) = self.curves.get(name) {
            return Arc::clone(existing);
        }
        let packed: Vec<[f32; 2]> = knots
            .iter()
            .take(SND_CURVE_MAX_KNOTS)
            .map(|&(x, y)| [x, y])
            .collect();
        let arc: Arc<[[f32; 2]]> = packed.into();
        self.curves.insert(name.to_owned(), Arc::clone(&arc));
        arc
    }
}

pub(crate) struct PlayerSoundPlugin;

impl Plugin for PlayerSoundPlugin {
    fn build(&self, app: &mut App) {
        if app.is_plugin_added::<bevy::audio::AudioPlugin>() {
            app.add_audio_source::<PcmAudio>()
                .add_audio_source::<LoopingPcmAudio>();
        } else {
            // CODLEGACY_HEADLESS=1: no sound device, so nothing plays the clips;
            // they are still assets, and the volume setting still has a home.
            app.init_asset::<PcmAudio>()
                .init_asset::<LoopingPcmAudio>()
                .init_resource::<bevy::audio::GlobalVolume>();
        }
        app.init_resource::<SoundPickState>()
            .init_resource::<crate::clip_store::ResidentClipCache>()
            .init_resource::<MissingAliasGaps>()
            .init_resource::<SharedPlayAssets>()
            .init_resource::<StartDecisions>()
            .init_resource::<PendingStarts>()
            .init_resource::<VoiceOccupancy>()
            .init_resource::<crate::ambient::MapAmbientBooted>()
            .init_resource::<crate::ambient::SoundBankLoadAttempted>()
            .init_resource::<crate::ambient::ResidentSoundBank>()
            .init_resource::<crate::BobCycleTracker>()
            .add_message::<AliasCommand>()
            .add_message::<Footstep>()
            .add_message::<WeaponSound>()
            .add_message::<crate::Iw6BulletPass>()
            .add_message::<BoundWeaponSound>()
            .add_message::<ViewmodelNotetracks>()
            .init_resource::<crate::entity_events::NotetrackSoundTable>()
            .add_message::<LandSound>()
            // bo2zm: the Sound sliders' gains, before anything plays.
            .add_systems(Update, crate::bus_gain::sync.before(play_alias_messages))
            // bo2mp: a Black Ops III map's room tones.
            .add_systems(Update, crate::ambient::update_room_tone.after(crate::ambient::boot_map_ambient_once).in_set(ClientSet::Effects))
            .add_systems(
                Update,
                (
                    crate::ambient::boot_map_ambient_once,
                    crate::ambient::play_random_point_sounds.before(play_alias_messages),
                    reclaim_finished_voices
                        .before(drain_pending_oneshots)
                        .before(play_alias_messages)
                        .before(play_footstep_messages)
                        .before(play_weapon_sound_messages)
                        .before(play_bound_weapon_sounds)
                        .before(play_land_sound_messages),
                    drain_pending_oneshots
                        .after(play_alias_messages)
                        .before(play_footstep_messages)
                        .before(play_weapon_sound_messages)
                        .before(play_land_sound_messages),
                    apply_svc_local_sound
                        .before(play_alias_messages)
                        .run_if(resource_exists::<LastAdoptedSnapshot>),
                    crate::shellshock::update_shellshock_tinnitus.before(play_alias_messages),
                    play_alias_messages.after(FxSoundPublished),
                    play_footstep_messages,
                    crate::entity_events::play_viewmodel_notetrack_messages
                        .before(play_weapon_sound_messages)
                        .before(play_bound_weapon_sounds),
                    crate::ads_sounds::play_ads_sounds.before(play_weapon_sound_messages),
                    play_weapon_sound_messages,
                    play_bound_weapon_sounds.after(drain_pending_oneshots),
                    play_land_sound_messages,
                    (
                        crate::destructible_loops::update,
                        crate::projectile_loops::update,
                    )
                        .before(crate::ambient::update_map_emitter_gain),
                    crate::zm_music::update.after(crate::bus_gain::sync),
                    crate::dive_vox::update.before(play_alias_messages),
                    crate::laststand::update
                        .before(play_alias_messages)
                        .after(crate::bus_gain::sync),
                    crate::room_echo::update.before(play_alias_messages),
                    crate::ambient::update_map_emitter_gain.after(crate::bus_gain::sync),
                    update_all_channels,
                )
                    .in_set(ClientSet::Effects),
            )
            .add_systems(
                Update,
                update_iw6_doppler
                    .after(update_all_channels)
                    .in_set(ClientSet::Effects),
            )
            .add_systems(
                Update,
                (
                    crate::iw6_sound::track_listener_speed.before(play_alias_messages),
                    crate::iw6_sound::play_iw6_whizby.before(play_weapon_sound_messages),
                    crate::iw6_sound::update_iw6_occlusion.before(update_all_channels),
                )
                    .in_set(ClientSet::Effects),
            )
            .add_systems(
                Update,
                (
                    crate::ambient::start_sound_bank_compose
                        .after(crate::ambient::stop_map_ambient_on_match_end),
                    crate::ambient::install_sound_bank
                        .after(crate::ambient::start_sound_bank_compose),
                    crate::entity_events::bind_notetrack_sounds
                        .after(crate::ambient::install_sound_bank),
                    crate::ambient::stop_map_ambient_on_match_end.after(SessionSwapApplied),
                    crate::ambient::boot_silent_map_effects
                        .after(crate::ambient::stop_map_ambient_on_match_end),
                    reset_clip_prep_on_match_torn_down,
                )
                    .in_set(ClientSet::Load),
            );
    }
}

fn reset_clip_prep_on_match_torn_down(
    mut torn: MessageReader<MatchTornDown>,
    mut pending: ResMut<PendingStarts>,
    mut shared: ResMut<SharedPlayAssets>,
) {
    if torn.read().count() == 0 {
        return;
    }
    pending.clear();
    diag::info!(
        Audio,
        "audio: common dry handles reused this match={}",
        shared.common_dry_reuses
    );
    *shared = SharedPlayAssets::default();
}

fn listener_pose(listeners: &Query<&Transform, With<AmbientListener>>) -> Option<(Vec3, Vec3)> {
    let n = listeners.iter().len();
    if n > 1 {
        panic!("more than one ambient listener");
    }
    listeners
        .iter()
        .next()
        .map(|t| (t.translation, t.rotation * Vec3::X))
}

/// bo2mp: a Ghosts voice's Doppler state (its emitter stays put; the
/// listener's movement is what shifts it).
#[derive(Component)]
pub(crate) struct Iw6Doppler {
    state: asset_audio::iw6_rules::Doppler,
    base_pitch: f32,
    last_ear: Option<[f32; 3]>,
}

fn update_iw6_doppler(
    time: Res<Time>,
    listeners: Query<&Transform, With<AmbientListener>>,
    mut voices: Query<(&Channel3d, &mut Iw6Doppler, &AudioSink)>,
) {
    let Some((ear, _)) = listener_pose(&listeners) else {
        return;
    };
    let ear = transform_inches(ear);
    let dt = time.delta_secs();
    for (ch, mut d, sink) in &mut voices {
        let delta = d.last_ear.map_or([0.0; 3], |l| [ear[0] - l[0], ear[1] - l[1], ear[2] - l[2]]);
        d.last_ear = Some(ear);
        let to_source = [ch.origin_inches[0] - ear[0], ch.origin_inches[1] - ear[1], ch.origin_inches[2] - ear[2]];
        let ratio = d.state.step(to_source, delta, [0.0; 3], dt);
        sink.set_speed((d.base_pitch * ratio).max(0.01));
    }
}

fn update_all_channels(
    listeners: Query<&Transform, With<AmbientListener>>,
    mut channels: Query<(Entity, &Channel3d, &mut AudioSink, Option<&crate::iw6_sound::Iw6Occlusion>)>,
    mut occupancy: ResMut<VoiceOccupancy>,
    settings: Res<frame::GameSettings>,
) {
    let Some((ear, right)) = listener_pose(&listeners) else {
        return;
    };
    let ear_inches = transform_inches(ear);
    for (entity, ch, mut sink, occlusion) in &mut channels {
        let dist = distance_inches(ear_inches, ch.origin_inches);
        if let Some(priority) = &ch.priority {
            occupancy.update_alias_priority(entity, priority.evaluate(Some(dist)));
        }
        let atten = if ch.knots.is_empty() {
            0.0
        } else {
            let value = distance_attenuation(
                &ch.knots,
                ch.near_knots.as_deref(),
                dist,
                ch.dist_min,
                ch.dist_max,
            );
            if value < 0.0 { 0.0 } else { value }
        };
        let emitter = Vec3::from_array(ch.origin_inches);
        let (pan_l, pan_r) = world_oneshot_channel_gains(ear, right, emitter, 1.0);
        ch.live_pan.set(pan_l, pan_r);
        // bo2mp: Ghosts' world occlusion (`iw6_sound`).
        let occluded = occlusion.map_or(1.0, |o| o.gain);
        sink.set_volume(Volume::Linear(
            (ch.base_volume * atten * occluded * settings.master_volume).max(0.0),
        ));
    }
}

fn apply_svc_local_sound(
    mut cmds: MessageReader<SvcLocalSound>,
    adopted: Res<LastAdoptedSnapshot>,
    announcer: Res<crate::match_voices::AnnouncerRoutes>,
    bank: Option<Res<SoundBank>>,
    mut play: MessageWriter<crate::AliasCommand>,
    mut warned: Local<std::collections::HashSet<String>>,
    epoch: Res<crate::backend::MatchEpoch>,
    mut heard: Local<(u64, std::collections::HashSet<String>)>,
) {
    if heard.0 != epoch.0 {
        *heard = (epoch.0, Default::default());
    }
    for cmd in cmds.read() {
        // bo2mp: a game-tagged name (`t6:mpl_hit_alert`, Black Ops II's
        // `playlocalsound`) plays from that game's aliases, through the
        // announcer's and interface sounds' routes as the event path does.
        let Some((namespace, alias)) = adopted.sound_alias_name(cmd.index).map(|alias| {
            match alias.split_once(':').and_then(|(ns, name)| AssetNamespace::parse(ns).map(|ns| (ns, name))) {
                // bo2mp: Modern Warfare III's (2023) aliases are kept by
                // their hash (`jup:<name>` or `jup:#<hash>`).
                Some((AssetNamespace::Jup, name)) => (AssetNamespace::Jup, asset_audio::jup_alias_key(name)),
                Some((ns, name)) => {
                    let (ns, name) = announcer.routed(ns, name).unwrap_or((ns, name));
                    match bank
                        .as_deref()
                        .filter(|_| ns == AssetNamespace::T6)
                        .and_then(|bank| crate::ui_sounds::ui_sound(&bank.0, name))
                    {
                        Some((ns, to)) => (ns, to.to_owned()),
                        None => (bank.as_deref().map_or(ns, |bank| crate::ui_sounds::bo3_script_alias(&bank.0, ns, name)), name.to_owned()),
                    }
                }
                None => {
                    let (ns, name) = announcer.route(alias);
                    (ns, name.to_owned())
                }
            }
        }) else {
            diag::warn!(
                Audio,
                "audio: svc local sound CS index {} is unresolved (typed gap)",
                cmd.index
            );
            continue;
        };
        // bo2mp: hit marker sounds (`t6:mpl_hit_alert`) the bank lacks play as
        // the same sound from another game's aliases; none: named in the log.
        let asked = format!("{}:{alias}", namespace.as_str());
        let resolved = bank
            .as_deref()
            .filter(|_| !cmd.stop)
            .and_then(|bank| crate::feedback_sounds::resolve(&bank.0, namespace, &alias));
        let feedback = resolved.is_some();
        let (namespace, alias) = match bank.as_deref().filter(|_| !cmd.stop) {
            Some(_) => match resolved {
                Some(crate::feedback_sounds::Resolved::Fallback(ns, to)) => {
                    if warned.insert(format!("{}:{alias}", namespace.as_str())) {
                        diag::warn!(
                            Audio,
                            "audio: hit/score alias `{}:{alias}` is not in the sound bank; playing `{}:{to}` instead",
                            namespace.as_str(),
                            ns.as_str()
                        );
                    }
                    (ns, to.to_owned())
                }
                Some(crate::feedback_sounds::Resolved::Missing) => {
                    if warned.insert(format!("{}:{alias}", namespace.as_str())) {
                        diag::warn!(
                            Audio,
                            "audio: hit/score alias `{}:{alias}` could not be resolved in any game's aliases: SILENT",
                            namespace.as_str()
                        );
                    }
                    (namespace, alias)
                }
                _ => (namespace, alias),
            },
            None => (namespace, alias),
        };
        // bo2mp: the first time each hit / kill sound is asked for on this
        // map, the log says it arrived and what it plays as (a silent hit
        // marker on a Black Ops map: was it never sent, or not found?).
        if feedback && heard.1.insert(asked.clone()) {
            diag::info!(
                Audio,
                "feedback: first `{asked}` this map plays as `{}:{alias}`",
                namespace.as_str()
            );
        }
        if cmd.stop {
            play.write(AliasCommand::Stop {
                namespace,
                alias,
                snd_ent: Some(SND_ENT_LOCAL),
            });
        } else {
            play.write(crate::AliasCommand::Play(PlayAlias {
                namespace,
                alias,
                fallback: None,
                origin_inches: None,
                snd_ent: Some(SND_ENT_LOCAL),
            }));
        }
    }
}

/// bo2zm: what an IW4 movement sound does on a Black Ops II map.
enum T6Movement {
    /// Play BO2's own sound of that name.
    Play(&'static str),
    /// Nothing: BO2 chains its gear sound on each step (the step alias's
    /// secondary), and has no sound for some (walking gear, quiet steps).
    Skip,
    /// Not a movement sound, or not a BO2 map.
    NotT6,
}

fn t6_movement(bank: &SoundCatalog, namespace: AssetNamespace, alias: &str) -> T6Movement {
    if namespace != AssetNamespace::Iw4
        || bank
            .index_in(AssetNamespace::T6, "fly_step_run_plr_default")
            .is_none()
    {
        return T6Movement::NotT6;
    }
    match crate::aliases::t6_movement_alias(alias) {
        Some(t6) if t6.starts_with("fly_gear_") && !t6.starts_with("fly_gear_mantle") => {
            T6Movement::Skip
        }
        Some(t6) if bank.index_in(AssetNamespace::T6, t6).is_some() => T6Movement::Play(t6),
        Some(_) => T6Movement::Skip,
        None => T6Movement::NotT6,
    }
}

fn play_alias_messages(
    mut events: MessageReader<AliasCommand>,
    mut commands: Commands,
    mut pcm_assets: ResMut<Assets<PcmAudio>>,
    mut shared: ResMut<SharedPlayAssets>,
    mut pick: ResMut<SoundPickState>,
    mut gaps: ResMut<MissingAliasGaps>,
    mut clips: Option<ResMut<ClipStore>>,
    mut pending: ResMut<PendingStarts>,
    mut occupancy: ResMut<VoiceOccupancy>,
    mut decisions: ResMut<StartDecisions>,
    bank: Option<Res<SoundBank>>,
    iwd: Option<Res<SoundIwd>>,
    epoch: Res<MatchEpoch>,
    listeners: Query<&Transform, With<AmbientListener>>,
    settings: Option<Res<frame::GameSettings>>,
) {
    let pose = listener_pose(&listeners);
    let iwd = iwd.as_deref().map(|s| s.0.as_ref());
    for command in events.read() {
        let (event, pitch_scale) = match command {
            AliasCommand::Play(event) => (event, 1.0),
            AliasCommand::PlayPitched { sound, pitch } => (sound, *pitch),
            AliasCommand::Stop {
                namespace,
                alias,
                snd_ent,
            } => {
                pending.cancel_alias(*namespace, alias, *snd_ent, epoch.0);
                let (namespace, alias, snd_ent, epoch) =
                    (*namespace, alias.clone(), *snd_ent, epoch.0);
                // Ordered after prior spawns, including ones queued by Play in
                // this same message batch; a later Play remains a new voice.
                commands.queue(move |world: &mut World| {
                    let entities: Vec<_> = world
                        .query::<(Entity, &AliasPlayback, &crate::backend::Voice)>()
                        .iter(world)
                        .filter(|(_, tag, voice)| {
                            tag.namespace == namespace
                                && tag.alias == alias
                                && tag.snd_ent == snd_ent
                                && voice.epoch == epoch
                                && voice.scope == crate::backend::AudioScope::Match
                        })
                        .map(|(entity, _, _)| entity)
                        .collect();
                    for entity in entities {
                        world.despawn(entity);
                    }
                });
                continue;
            }
        };
        // bo2mp: the match start countdown ticks with Ghosts' sound.
        if crate::ghosts_countdown::is_countdown(&event.alias)
            && let Some(clip) = crate::ghosts_countdown::clip()
        {
            let volume = settings.as_deref().map_or(1.0, |s| s.master_volume) * crate::bus_gain::sfx();
            let handle = pcm_assets.add(clip);
            crate::backend::spawn_oneshot(
                &mut commands,
                handle,
                Volume::Linear(volume.max(0.0)),
                1.0,
                epoch.0,
                crate::backend::AudioScope::Match,
            );
            continue;
        }
        let Some(bank) = bank.as_ref() else {
            drop_without_bank(std::iter::once(event.alias.as_str()), &mut decisions);
            continue;
        };
        let (namespace, alias) = match t6_movement(&bank.0, event.namespace, &event.alias) {
            T6Movement::Play(t6) => (AssetNamespace::T6, t6),
            T6Movement::Skip => continue,
            T6Movement::NotT6 => (event.namespace, event.alias.as_str()),
        };
        let outcome = play_oneshot_recorded(
            &mut commands,
            &mut pcm_assets,
            &mut shared,
            &bank.0,
            iwd,
            namespace,
            alias,
            None,
            event.origin_inches,
            pose,
            &mut pick,
            clips.as_deref_mut(),
            &mut pending,
            &mut occupancy,
            &mut decisions,
            event.snd_ent,
            SoundClass::World,
            epoch.0,
            pitch_scale,
        );
        finish_binding_start(
            outcome,
            &event.alias,
            event.fallback.as_deref(),
            |fb| {
                play_oneshot_recorded(
                    &mut commands,
                    &mut pcm_assets,
                    &mut shared,
                    &bank.0,
                    iwd,
                    event.namespace,
                    fb,
                    None,
                    event.origin_inches,
                    pose,
                    &mut pick,
                    clips.as_deref_mut(),
                    &mut pending,
                    &mut occupancy,
                    &mut decisions,
                    event.snd_ent,
                    SoundClass::World,
                    epoch.0,
                    pitch_scale,
                )
            },
            &mut gaps,
        );
    }
}

fn play_footstep_messages(
    mut events: MessageReader<Footstep>,
    mut commands: Commands,
    mut pcm_assets: ResMut<Assets<PcmAudio>>,
    mut shared: ResMut<SharedPlayAssets>,
    mut pick: ResMut<SoundPickState>,
    mut gaps: ResMut<MissingAliasGaps>,
    mut clips: Option<ResMut<ClipStore>>,
    mut pending: ResMut<PendingStarts>,
    mut occupancy: ResMut<VoiceOccupancy>,
    mut decisions: ResMut<StartDecisions>,
    bank: Option<Res<SoundBank>>,
    iwd: Option<Res<SoundIwd>>,
    epoch: Res<MatchEpoch>,
    listeners: Query<&Transform, With<AmbientListener>>,
) {
    let Some(bank) = bank else {
        drop_without_bank(events.read().map(|e| e.alias), &mut decisions);
        return;
    };
    let pose = listener_pose(&listeners);
    let iwd = iwd.as_deref().map(|s| s.0.as_ref());
    for event in events.read() {
        let outcome = play_surface_alias_chain(
            &mut commands,
            &mut pcm_assets,
            &mut shared,
            &bank.0,
            iwd,
            event.alias,
            event.fallback,
            event.origin_inches,
            pose,
            &mut pick,
            clips.as_deref_mut(),
            &mut pending,
            &mut occupancy,
            &mut decisions,
            event.snd_ent,
            SoundClass::World,
            epoch.0,
        );
        if outcome.allows_binding_fallback() {
            gaps.record(event.alias);
            gaps.record(event.fallback);
        }
    }
}

fn play_weapon_sound_messages(
    mut events: MessageReader<WeaponSound>,
    mut commands: Commands,
    mut pcm_assets: ResMut<Assets<PcmAudio>>,
    mut shared: ResMut<SharedPlayAssets>,
    mut pick: ResMut<SoundPickState>,
    mut gaps: ResMut<MissingAliasGaps>,
    mut clips: Option<ResMut<ClipStore>>,
    mut pending: ResMut<PendingStarts>,
    mut occupancy: ResMut<VoiceOccupancy>,
    mut decisions: ResMut<StartDecisions>,
    bank: Option<Res<SoundBank>>,
    iwd: Option<Res<SoundIwd>>,
    epoch: Res<MatchEpoch>,
    listeners: Query<&Transform, With<AmbientListener>>,
) {
    let Some(bank) = bank else {
        drop_without_bank(events.read().map(|e| e.alias.as_str()), &mut decisions);
        return;
    };
    let pose = listener_pose(&listeners);
    let iwd = iwd.as_deref().map(|s| s.0.as_ref());
    for event in events.read() {
        let (namespace, alias) = match t6_movement(&bank.0, event.namespace, &event.alias) {
            T6Movement::Play(t6) => (AssetNamespace::T6, t6),
            T6Movement::Skip => continue,
            T6Movement::NotT6 => (event.namespace, event.alias.as_str()),
        };
        let outcome = play_alias_oneshot(
            &mut commands,
            &mut pcm_assets,
            &mut shared,
            &bank.0,
            iwd,
            namespace,
            alias,
            event.origin_inches,
            pose,
            &mut pick,
            clips.as_deref_mut(),
            &mut pending,
            &mut occupancy,
            &mut decisions,
            event.snd_ent,
            SoundClass::Weapon,
            epoch.0,
        );
        if outcome.allows_binding_fallback() {
            gaps.record(alias);
        }
    }
}

fn play_bound_weapon_sounds(
    mut events: MessageReader<BoundWeaponSound>,
    mut commands: Commands,
    mut pcm_assets: ResMut<Assets<PcmAudio>>,
    mut shared: ResMut<SharedPlayAssets>,
    mut pick: ResMut<SoundPickState>,
    mut gaps: ResMut<MissingAliasGaps>,
    mut clips: Option<ResMut<ClipStore>>,
    mut pending: ResMut<PendingStarts>,
    mut occupancy: ResMut<VoiceOccupancy>,
    mut decisions: ResMut<StartDecisions>,
    bank: Option<Res<SoundBank>>,
    iwd: Option<Res<SoundIwd>>,
    epoch: Res<MatchEpoch>,
    listeners: Query<&Transform, With<AmbientListener>>,
) {
    let Some(bank) = bank else {
        for _ in events.read() {}
        return;
    };
    let pose = listener_pose(&listeners);
    let iwd = iwd.as_deref().map(|s| s.0.as_ref());
    for event in events.read() {
        if event.bank_revision != bank.0.revision() {
            continue;
        }
        let Some(alias) = bank.0.name_at(event.index) else {
            continue;
        };
        let namespace = bank.0.namespace_of_alias(event.index);
        let outcome = play_oneshot_recorded(
            &mut commands,
            &mut pcm_assets,
            &mut shared,
            &bank.0,
            iwd,
            namespace,
            alias,
            Some(event.index),
            event.origin_inches,
            pose,
            &mut pick,
            clips.as_deref_mut(),
            &mut pending,
            &mut occupancy,
            &mut decisions,
            event.snd_ent,
            SoundClass::Weapon,
            epoch.0,
            1.0,
        );
        if outcome.allows_binding_fallback() {
            gaps.record(alias);
        }
    }
}

fn play_land_sound_messages(
    mut events: MessageReader<LandSound>,
    mut commands: Commands,
    mut pcm_assets: ResMut<Assets<PcmAudio>>,
    mut shared: ResMut<SharedPlayAssets>,
    mut pick: ResMut<SoundPickState>,
    mut gaps: ResMut<MissingAliasGaps>,
    mut clips: Option<ResMut<ClipStore>>,
    mut pending: ResMut<PendingStarts>,
    mut occupancy: ResMut<VoiceOccupancy>,
    mut decisions: ResMut<StartDecisions>,
    bank: Option<Res<SoundBank>>,
    iwd: Option<Res<SoundIwd>>,
    epoch: Res<MatchEpoch>,
    listeners: Query<&Transform, With<AmbientListener>>,
) {
    let Some(bank) = bank else {
        drop_without_bank(events.read().map(|e| e.alias), &mut decisions);
        return;
    };
    let pose = listener_pose(&listeners);
    let iwd = iwd.as_deref().map(|s| s.0.as_ref());
    for event in events.read() {
        let outcome = play_surface_alias_chain(
            &mut commands,
            &mut pcm_assets,
            &mut shared,
            &bank.0,
            iwd,
            event.alias,
            event.fallback,
            event.origin_inches,
            pose,
            &mut pick,
            clips.as_deref_mut(),
            &mut pending,
            &mut occupancy,
            &mut decisions,
            event.snd_ent,
            SoundClass::World,
            epoch.0,
        );
        if outcome.allows_binding_fallback() {
            gaps.record(event.alias);
            gaps.record(event.fallback);
        }
    }
}

fn drop_without_bank<'a>(aliases: impl Iterator<Item = &'a str>, decisions: &mut StartDecisions) {
    for alias in aliases {
        if !crate::AudioSilent::active() {
            diag::warn!(
                Audio,
                "audio: alias `{alias}` dropped — no SoundBank (typed gap)"
            );
        }
        decisions.record(StartDecision {
            namespace: AssetNamespace::Iw4,
            alias: alias.to_owned(),
            variant: None,
            outcome: StartOutcome::Failed(StartFailure::BankMissing),
            secondary: None,
            detail: None,
        });
    }
}

fn finish_binding_start(
    outcome: StartOutcome,
    alias: &str,
    fallback: Option<&str>,
    play_fallback: impl FnOnce(&str) -> StartOutcome,
    gaps: &mut MissingAliasGaps,
) {
    if outcome.is_open() || !outcome.allows_binding_fallback() {
        return;
    }
    if let Some(fb) = fallback {
        let fb_out = play_fallback(fb);
        if fb_out.is_open() {
            return;
        }
        if fb_out.allows_binding_fallback() {
            gaps.record(fb);
        }
    }
    gaps.record(alias);
}

fn play_surface_alias_chain(
    commands: &mut Commands,
    pcm_assets: &mut Assets<PcmAudio>,
    shared: &mut SharedPlayAssets,
    bank: &SoundCatalog,
    iwd: Option<&NamespaceSoundIwd>,
    alias: &str,
    fallback: &str,
    origin_inches: Option<[f32; 3]>,
    listener: Option<(Vec3, Vec3)>,
    pick: &mut SoundPickState,
    mut clips: Option<&mut ClipStore>,
    pending: &mut PendingStarts,
    occupancy: &mut VoiceOccupancy,
    decisions: &mut StartDecisions,
    snd_ent: Option<u32>,
    class: SoundClass,
    epoch: u64,
) -> StartOutcome {
    let mut last = StartOutcome::Failed(StartFailure::MissingAlias);
    // bo2mp: a Ghosts map's own movement sounds by surface
    // (`BO2MP_IW6_FOOTSTEPS`): the same `<base>_<surface>` names, then
    // `<base>_default`, from the map zone.
    if asset_audio::iw6_rules::Iw6Switches::get().footsteps {
        let names: Vec<String> = [alias, fallback]
            .into_iter()
            .filter_map(asset_audio::iw6_rules::ghosts_movement_alias)
            .filter(|name| bank.index_in(AssetNamespace::Iw6, name).is_some())
            .collect();
        for candidate in &names {
            let outcome = play_alias_oneshot(
                commands,
                pcm_assets,
                shared,
                bank,
                iwd,
                AssetNamespace::Iw6,
                candidate,
                origin_inches,
                listener,
                pick,
                clips.as_deref_mut(),
                pending,
                occupancy,
                decisions,
                snd_ent,
                class,
                epoch,
            );
            if outcome.is_open() || !outcome.allows_binding_fallback() {
                return outcome;
            }
            last = outcome;
        }
    }
    // bo2zm: a Black Ops II map plays its own movement sounds, by BO2's
    // names, before the IW4 chain.
    for candidate in [alias, fallback]
        .into_iter()
        .filter_map(crate::aliases::t6_movement_alias)
        .filter(|name| bank.index_in(AssetNamespace::T6, name).is_some())
    {
        let outcome = play_alias_oneshot(
            commands,
            pcm_assets,
            shared,
            bank,
            iwd,
            AssetNamespace::T6,
            candidate,
            origin_inches,
            listener,
            pick,
            clips.as_deref_mut(),
            pending,
            occupancy,
            decisions,
            snd_ent,
            class,
            epoch,
        );
        if outcome.is_open() || !outcome.allows_binding_fallback() {
            return outcome;
        }
        last = outcome;
    }
    for candidate in crate::aliases::surface_alias_candidates(alias, fallback) {
        let outcome = play_alias_oneshot(
            commands,
            pcm_assets,
            shared,
            bank,
            iwd,
            AssetNamespace::Iw4,
            candidate,
            origin_inches,
            listener,
            pick,
            clips.as_deref_mut(),
            pending,
            occupancy,
            decisions,
            snd_ent,
            class,
            epoch,
        );
        if outcome.is_open() || !outcome.allows_binding_fallback() {
            return outcome;
        }
        last = outcome;
    }
    last
}

pub fn world_oneshot_pan(ear: Vec3, listener_right: Vec3, emitter: Vec3) -> f32 {
    let to = emitter - ear;
    if to.length_squared() < 1e-8 {
        0.0
    } else {
        to.normalize().dot(listener_right).clamp(-1.0, 1.0)
    }
}

pub fn world_oneshot_channel_gains(
    ear: Vec3,
    listener_right: Vec3,
    emitter: Vec3,
    atten: f32,
) -> (f32, f32) {
    let pan = world_oneshot_pan(ear, listener_right, emitter);
    let angle = (pan + 1.0) * FRAC_PI_4;
    (atten * angle.cos(), atten * angle.sin())
}

fn drain_pending_oneshots(
    mut pending: ResMut<PendingStarts>,
    mut commands: Commands,
    mut pcm_assets: ResMut<Assets<PcmAudio>>,
    mut shared: ResMut<SharedPlayAssets>,
    mut pick: ResMut<SoundPickState>,
    mut occupancy: ResMut<VoiceOccupancy>,
    mut decisions: ResMut<StartDecisions>,
    mut clips: Option<ResMut<ClipStore>>,
    bank: Option<Res<SoundBank>>,
    iwd: Option<Res<SoundIwd>>,
    epoch: Res<MatchEpoch>,
    listeners: Query<&Transform, With<AmbientListener>>,
) {
    if pending.entries.is_empty() {
        return;
    }
    let Some(bank) = bank else {
        pending.clear();
        return;
    };
    let Some(clips) = clips.as_mut() else {
        return;
    };
    let pose = listener_pose(&listeners);
    let iwd = iwd.as_deref().map(|s| s.0.as_ref());
    let now = Instant::now();
    let waiting = std::mem::take(&mut pending.entries);
    for entry in waiting {
        if entry.class.scope() == crate::backend::AudioScope::Match && entry.epoch != epoch.0 {
            continue;
        }
        if now >= entry.deadline {
            diag::warn!(
                Audio,
                "audio: alias `{}:{}` expired awaiting decode (typed gap)",
                entry.namespace.as_str(),
                entry.alias
            );
            decisions.record(StartDecision {
                namespace: entry.namespace,
                alias: entry.alias,
                variant: Some(entry.variant),
                outcome: StartOutcome::Failed(StartFailure::Expired),
                secondary: None,
                detail: None,
            });
            continue;
        }
        match clips.ready(&entry.clip) {
            None => pending.entries.push(entry),
            Some(Err(_)) => {
                decisions.record(StartDecision {
                    namespace: entry.namespace,
                    alias: entry.alias,
                    variant: Some(entry.variant),
                    outcome: StartOutcome::Failed(StartFailure::DecodeFailed),
                    secondary: None,
                    detail: None,
                });
            }
            Some(Ok(pcm)) => {
                let started = submit_prepared_oneshot(
                    &mut commands,
                    &mut pcm_assets,
                    &mut shared,
                    &bank.0,
                    iwd,
                    entry.namespace,
                    &entry.alias,
                    entry.bound,
                    entry.origin_inches,
                    pose,
                    &mut pick,
                    Some(clips.as_mut()),
                    &mut pending,
                    &mut occupancy,
                    entry.snd_ent,
                    0,
                    entry.class,
                    entry.epoch,
                    pcm,
                    &entry.clip,
                    entry.variant,
                    entry.volume,
                    entry.pitch,
                    entry.layer.as_deref(),
                );
                if let Some((sec, sec_out)) = &started.secondary
                    && !sec_out.is_open()
                {
                    diag::warn!(
                        Audio,
                        "audio: secondary `{sec}` of `{}:{}` result={sec_out}",
                        entry.namespace.as_str(),
                        entry.alias
                    );
                }
                decisions.record(StartDecision {
                    namespace: entry.namespace,
                    alias: entry.alias,
                    variant: started.variant,
                    outcome: started.outcome,
                    secondary: started.secondary,
                    detail: started.detail,
                });
            }
        }
    }
}

pub(crate) fn play_alias_oneshot(
    commands: &mut Commands,
    pcm_assets: &mut Assets<PcmAudio>,
    shared: &mut SharedPlayAssets,
    bank: &SoundCatalog,
    iwd: Option<&NamespaceSoundIwd>,
    namespace: AssetNamespace,
    alias: &str,
    origin_inches: Option<[f32; 3]>,
    listener: Option<(Vec3, Vec3)>,
    pick: &mut SoundPickState,
    clips: Option<&mut ClipStore>,
    pending: &mut PendingStarts,
    occupancy: &mut VoiceOccupancy,
    decisions: &mut StartDecisions,
    snd_ent: Option<u32>,
    class: SoundClass,
    epoch: u64,
) -> StartOutcome {
    play_oneshot_recorded(
        commands,
        pcm_assets,
        shared,
        bank,
        iwd,
        namespace,
        alias,
        None,
        origin_inches,
        listener,
        pick,
        clips,
        pending,
        occupancy,
        decisions,
        snd_ent,
        class,
        epoch,
        1.0,
    )
}

#[allow(clippy::too_many_arguments)]
fn play_oneshot_recorded(
    commands: &mut Commands,
    pcm_assets: &mut Assets<PcmAudio>,
    shared: &mut SharedPlayAssets,
    bank: &SoundCatalog,
    iwd: Option<&NamespaceSoundIwd>,
    namespace: AssetNamespace,
    alias: &str,
    bound: Option<usize>,
    origin_inches: Option<[f32; 3]>,
    listener: Option<(Vec3, Vec3)>,
    pick: &mut SoundPickState,
    clips: Option<&mut ClipStore>,
    pending: &mut PendingStarts,
    occupancy: &mut VoiceOccupancy,
    decisions: &mut StartDecisions,
    snd_ent: Option<u32>,
    class: SoundClass,
    epoch: u64,
    pitch_scale: f32,
) -> StartOutcome {
    let started = play_alias_oneshot_at(
        commands,
        pcm_assets,
        shared,
        bank,
        iwd,
        namespace,
        alias,
        bound,
        origin_inches,
        listener,
        pick,
        clips,
        pending,
        occupancy,
        snd_ent,
        0,
        class,
        epoch,
        pitch_scale,
    );
    if let Some((sec, sec_out)) = &started.secondary
        && !sec_out.is_open()
    {
        diag::warn!(
            Audio,
            "audio: secondary `{sec}` of `{}:{alias}` result={sec_out}",
            namespace.as_str()
        );
    }
    let outcome = started.outcome.clone();
    decisions.record(StartDecision {
        namespace,
        alias: alias.to_owned(),
        variant: started.variant,
        outcome: started.outcome,
        secondary: started.secondary,
        detail: started.detail,
    });
    outcome
}

struct OneshotStart {
    outcome: StartOutcome,
    variant: Option<usize>,
    secondary: Option<(String, StartOutcome)>,
    detail: Option<String>,
}

impl OneshotStart {
    fn failed(failure: StartFailure) -> Self {
        Self {
            outcome: StartOutcome::Failed(failure),
            variant: None,
            secondary: None,
            detail: None,
        }
    }

    fn with_variant(mut self, variant: usize) -> Self {
        self.variant = Some(variant);
        self
    }
}

fn falloff_detail(dist: f32, dist_min: f32, dist_max: f32, atten: f32) -> String {
    format!("dist={dist:.0} dist_min={dist_min:.0} dist_max={dist_max:.0} atten={atten:.3}")
}

fn prepare_voice(
    commands: &mut Commands,
    occupancy: &mut VoiceOccupancy,
    bank: &SoundCatalog,
    channel: Option<u32>,
    snd_ent: Option<u32>,
) -> Result<(), SuppressReason> {
    let Some(ch) = channel else {
        return Ok(());
    };
    let Some(info) = bank.ent_channel(ch) else {
        return Ok(());
    };
    if info.is_restricted {
        if let Some(ent) = snd_ent {
            let stopped = occupancy.take_entity_channel(ent, ch);

            for entity in stopped {
                crate::backend::stop(commands, entity);
            }
        }
    }
    let voice_n = occupancy.voice_count(ch);
    if !has_free_voice(voice_n, 0, info.max_voices) {
        return Err(SuppressReason::VoiceLimit);
    }
    Ok(())
}

fn prepare_alias_voice(
    commands: &mut Commands,
    occupancy: &mut VoiceOccupancy,
    namespace: AssetNamespace,
    alias: &str,
    row: Option<&asset_audio::CapturedAlias>,
    snd_ent: Option<u32>,
    priority: f32,
) -> Result<(), SuppressReason> {
    if namespace != AssetNamespace::T5 {
        return Ok(());
    }
    let Some(row) = row else {
        return Ok(());
    };
    let flags = row.flags.unwrap_or(0);
    for (mode, count, per_entity) in [
        ((flags >> 25) & 3, row.limit_count, false),
        ((flags >> 27) & 3, row.entity_limit_count, true),
    ] {
        if let Some(count) = count
            && let Some(entity) = occupancy.limit_alias(
                namespace,
                alias,
                snd_ent,
                (mode, count),
                per_entity,
                priority,
            )?
        {
            crate::backend::stop(commands, entity);
        }
    }
    Ok(())
}

fn voice_lease(
    bank: &SoundCatalog,
    channel: Option<u32>,
    snd_ent: Option<u32>,
) -> Option<VoiceLease> {
    let ch = channel?;
    bank.ent_channel(ch)?;
    Some(VoiceLease {
        channel: ch,
        snd_ent,
    })
}

fn track_voice(occupancy: &mut VoiceOccupancy, entity: Entity, lease: Option<VoiceLease>) {
    if let Some(lease) = lease {
        occupancy.track(entity, lease);
    }
}

fn streamed_row_volume_pitch(
    namespace: AssetNamespace,
    row: &asset_audio::CapturedAlias,
    rng: &mut u32,
) -> (f32, f32) {
    // bo2mp: a Ghosts alias takes Ghosts' per-play numbers.
    if namespace == AssetNamespace::Iw6 {
        return asset_audio::iw6_rules::volume_pitch(row, asset_audio::iw6_rules::Iw6Switches::get(), rng);
    }
    let t_vol = unit_random(rng);
    let t_pitch = unit_random(rng);
    let volume = if namespace != AssetNamespace::T5 && row.vol_min == 0.0 && row.vol_max == 0.0 {
        1.0
    } else {
        lerp_range(row.vol_min, row.vol_max, t_vol)
    };
    let pitch = if row.pitch_min == 0.0 && row.pitch_max == 0.0 {
        1.0
    } else {
        lerp_range(row.pitch_min, row.pitch_max, t_pitch)
    };
    (volume, pitch)
}

fn play_alias_oneshot_at(
    commands: &mut Commands,
    pcm_assets: &mut Assets<PcmAudio>,
    shared: &mut SharedPlayAssets,
    bank: &SoundCatalog,
    iwd: Option<&NamespaceSoundIwd>,
    namespace: AssetNamespace,
    alias: &str,
    bound: Option<usize>,
    origin_inches: Option<[f32; 3]>,
    listener: Option<(Vec3, Vec3)>,
    pick: &mut SoundPickState,
    mut clips: Option<&mut ClipStore>,
    pending: &mut PendingStarts,
    occupancy: &mut VoiceOccupancy,
    snd_ent: Option<u32>,
    depth: u8,
    class: SoundClass,
    epoch: u64,
    pitch_scale: f32,
) -> OneshotStart {
    let avoid = pick
        .last_variant
        .get(&(namespace, alias.to_owned()))
        .copied();
    let picked = match bound {
        Some(index) => bank.pick_loaded_outcome_at(index, &mut pick.lcg, avoid),
        None => bank.pick_loaded_outcome(namespace, alias, &mut pick.lcg, avoid),
    };
    let Some(outcome) = picked else {
        return OneshotStart::failed(StartFailure::MissingAlias);
    };
    let variant_index = outcome.variant_index;
    let t5_secondary = if namespace == AssetNamespace::T5 {
        let layer = match bound {
            Some(index) => bank.sound_at(index),
            None => bank.sound_in(namespace, alias),
        }
        .and_then(|sound| sound.aliases.get(variant_index))
        .and_then(|row| row.secondary.as_deref());
        play_secondary_layer(
            commands,
            pcm_assets,
            shared,
            bank,
            iwd,
            namespace,
            layer,
            origin_inches,
            listener,
            pick,
            clips.as_deref_mut(),
            pending,
            occupancy,
            snd_ent,
            depth,
            class,
            epoch,
        )
    } else {
        None
    };

    let loaded_name = outcome.picked.as_ref().map(|p| p.sound.name.as_str());
    let loaded_ns = outcome
        .picked
        .as_ref()
        .map(|p| p.sound.ns());
    let Some(clip) = clip_key_for_variant(
        bank,
        namespace,
        alias,
        bound,
        variant_index,
        loaded_name,
        loaded_ns,
    ) else {
        // bo2mp: a Black Ops II alias with no sound of its own can be there
        // only for its secondary (a jump's launch exertion carries the gear
        // sound): that still plays.
        let mut failed = OneshotStart::failed(StartFailure::NoPcm).with_variant(variant_index);
        if namespace == AssetNamespace::T6 {
            let layer = match bound {
                Some(index) => bank.sound_at(index),
                None => bank.sound_in(namespace, alias),
            }
            .and_then(|sound| sound.aliases.get(variant_index))
            .and_then(|row| row.secondary.clone());
            failed.secondary = play_secondary_layer(
                commands,
                pcm_assets,
                shared,
                bank,
                iwd,
                namespace,
                layer.as_deref(),
                origin_inches,
                listener,
                pick,
                clips.as_deref_mut(),
                pending,
                occupancy,
                snd_ent,
                depth,
                class,
                epoch,
            );
        }
        return failed;
    };
    let (volume, pitch, layer) = if let Some(picked) = &outcome.picked {
        (picked.volume, picked.pitch, picked.layer.clone())
    } else {
        let row = match bound {
            Some(index) => bank.sound_at(index),
            None => bank.sound_in(namespace, alias),
        }
        .and_then(|s| s.aliases.get(variant_index));
        let (volume, pitch) = row
            .map(|row| streamed_row_volume_pitch(namespace, row, &mut pick.lcg))
            .unwrap_or((1.0, 1.0));
        (volume, pitch, row.and_then(|r| r.secondary.clone()))
    };
    let pitch = pitch
        * if pitch_scale.is_finite() && pitch_scale > 0.0 {
            pitch_scale
        } else {
            1.0
        };
    let pcm = match take_or_pending_clip(
        bank,
        clips.as_deref_mut(),
        pending,
        clip.clone(),
        namespace,
        alias,
        bound,
        variant_index,
        volume,
        pitch,
        origin_inches,
        snd_ent,
        layer.clone(),
        class,
        epoch,
    ) {
        ClipTake::Ready(pcm) => pcm,
        ClipTake::Pending => {
            return OneshotStart {
                outcome: StartOutcome::Pending,
                variant: Some(variant_index),
                secondary: None,
                detail: None,
            };
        }
        ClipTake::Failed(failure) => {
            return OneshotStart::failed(failure).with_variant(variant_index);
        }
    };
    let mut started = submit_prepared_oneshot(
        commands,
        pcm_assets,
        shared,
        bank,
        iwd,
        namespace,
        alias,
        bound,
        origin_inches,
        listener,
        pick,
        clips,
        pending,
        occupancy,
        snd_ent,
        depth,
        class,
        epoch,
        pcm,
        &clip,
        variant_index,
        volume,
        pitch,
        layer.as_deref(),
    );
    if namespace == AssetNamespace::T5 {
        started.secondary = t5_secondary;
    }
    started
}

enum ClipTake {
    Ready(PcmAudio),
    Pending,
    Failed(StartFailure),
}

fn take_or_pending_clip(
    bank: &SoundCatalog,
    clips: Option<&mut ClipStore>,
    pending: &mut PendingStarts,
    clip: ClipKey,
    namespace: AssetNamespace,
    alias: &str,
    bound: Option<usize>,
    variant: usize,
    volume: f32,
    pitch: f32,
    origin_inches: Option<[f32; 3]>,
    snd_ent: Option<u32>,
    layer: Option<String>,
    class: SoundClass,
    epoch: u64,
) -> ClipTake {
    if let Some(store) = clips {
        // bo2mp: name the alias behind a clip the match set left out (the
        // store's own warning only knows the clip).
        let late_before = store.late_prepares();
        store.request(clip.clone());
        if store.late_prepares() != late_before {
            diag::warn!(
                Audio,
                "audio: late clip from alias `{namespace:?}:{alias}` ({class:?})"
            );
        }
        if let Some(layer) = layer.as_deref() {
            for key in clip_keys_for_alias(bank, namespace, layer) {
                store.request(key);
            }
        }
        match store.ready(&clip) {
            Some(Ok(pcm)) => ClipTake::Ready(pcm),
            Some(Err(ClipError::Decode | ClipError::Read | ClipError::QueueClosed)) => {
                ClipTake::Failed(StartFailure::DecodeFailed)
            }
            None => {
                pending.push_oneshot(PendingOneshot {
                    namespace,
                    alias: alias.to_owned(),
                    bound,
                    variant,
                    volume,
                    pitch,
                    origin_inches,
                    snd_ent,
                    clip,
                    layer,
                    class,
                    epoch,
                    // bo2mp: a Modern Warfare III (2023) clip is read and
                    // decoded from its package when first played (music
                    // and island beds run a minute and more); Resurgence's
                    // announcer, music and stings may start late, not never.
                    deadline: if namespace == AssetNamespace::Jup {
                        std::time::Instant::now() + std::time::Duration::from_secs(8)
                    } else {
                        deadline_for(class)
                    },
                });
                ClipTake::Pending
            }
        }
    } else {
        ClipTake::Failed(StartFailure::NoPcm)
    }
}

/// bo2zm: the room echo a Black Ops II alias's play sends into (its
/// `reverbSend`), when the player is in a room that has one.
fn room_echo_for(
    namespace: AssetNamespace,
    row: Option<&asset_audio::CapturedAlias>,
) -> Option<(std::sync::Arc<crate::room_echo::EchoParams>, f32)> {
    // bo2mp: a Ghosts alias sends its dry/wet rule's wet level
    // (`BO2MP_IW6_REVERB`): the map's zone reverb, or its own level
    // (flag 0x10); nothing when no Ghosts zone reverb is in force.
    if namespace == AssetNamespace::Iw6 {
        let params = row?.iw6.as_deref()?;
        if !asset_audio::iw6_rules::Iw6Switches::get().reverb {
            return None;
        }
        let reverb = asset_audio::iw6_rules::map_reverb().map(|(_, dry, wet)| (dry, wet));
        let (_, wet) = asset_audio::iw6_rules::dry_wet(params.flags, params.wet_mix_override, reverb);
        return (wet > 0.0).then(|| crate::room_echo::current().map(|e| (e, wet))).flatten();
    }
    if namespace != AssetNamespace::T6 {
        return None;
    }
    let send = row?.reverb_send;
    if send <= 0.0 {
        return None;
    }
    Some((crate::room_echo::current()?, send))
}

fn submit_prepared_oneshot(
    commands: &mut Commands,
    pcm_assets: &mut Assets<PcmAudio>,
    shared: &mut SharedPlayAssets,
    bank: &SoundCatalog,
    iwd: Option<&NamespaceSoundIwd>,
    namespace: AssetNamespace,
    alias: &str,
    bound: Option<usize>,
    origin_inches: Option<[f32; 3]>,
    listener: Option<(Vec3, Vec3)>,
    pick: &mut SoundPickState,
    clips: Option<&mut ClipStore>,
    pending: &mut PendingStarts,
    occupancy: &mut VoiceOccupancy,
    snd_ent: Option<u32>,
    depth: u8,
    class: SoundClass,
    epoch: u64,
    pcm: PcmAudio,
    clip: &ClipKey,
    variant_index: usize,
    volume: f32,
    pitch: f32,
    layer: Option<&str>,
) -> OneshotStart {
    let sound = match bound {
        Some(index) => bank.sound_at(index),
        None => bank.sound_in(namespace, alias),
    };
    let row = sound.and_then(|s| s.aliases.get(variant_index));
    // bo2zm: Black Ops II's Sound sliders (music, effects, voice) on its
    // bus.
    // bo2mp: a guest gun's sounds are effects, on the effects slider too.
    let t6_flags = if namespace == AssetNamespace::T6 { row.and_then(|r| r.flags) } else { None };
    let match_music = class != SoundClass::Ui
        && (namespace != AssetNamespace::T6 || t6_flags.is_some())
        && crate::bus_gain::is_music_alias(t6_flags, alias);
    let volume = if match_music {
        // bo2mp: every game's in-match music cue (spawn, winning, victory)
        // at in-match music's low gain, under the music slider.
        volume * crate::bus_gain::match_music()
    } else if namespace == AssetNamespace::T6 {
        volume * crate::bus_gain::t6_alias(row.and_then(|r| r.flags))
    } else if class == SoundClass::Weapon && asset_audio::is_guest_namespace(namespace) {
        volume * crate::bus_gain::sfx()
    } else {
        volume
    };
    // bo2mp: the hit marker and Kill Confirmed sounds over the gunfire
    // (`feedback_sounds::level`).
    let volume = if crate::feedback_sounds::is_feedback(namespace, alias) {
        crate::feedback_sounds::level(namespace, volume)
    } else {
        volume
    };
    let limit_name = sound.map_or(alias, |sound| sound.name.as_str());
    let channel = sound.and_then(|s| s.ent_channel(variant_index));
    let mut world_detail: Option<String> = None;
    // bo2zm: Black Ops II keeps Black Ops' pan-type bit (flags bit 1 = 3D).
    let positional = if matches!(namespace, AssetNamespace::T5 | AssetNamespace::T6) {
        row.and_then(|row| row.flags)
            .is_some_and(|flags| flags & 2 != 0)
    } else {
        channel
            .and_then(|ch| bank.ent_channel(ch))
            .is_none_or(|info| info.is_3d)
    };
    let distance = origin_inches
        .filter(|_| positional)
        .zip(listener)
        .map(|(origin, (ear, _))| distance_inches(transform_inches(ear), origin));
    let priority = row
        .and_then(|row| row.voice_priority.as_ref())
        .map_or(0.0, |priority| priority.evaluate(distance));
    match origin_inches.filter(|_| positional) {
        Some(pos) => {
            let Some((ear, right)) = listener else {
                diag::warn!(
                    Audio,
                    "audio: world alias `{alias}` has no listener (typed gap)"
                );
                return OneshotStart::failed(StartFailure::NoListener).with_variant(variant_index);
            };
            let Some(row) = row else {
                return OneshotStart::failed(StartFailure::NoPcm).with_variant(variant_index);
            };
            let ear_inches = transform_inches(ear);
            let dist = distance_inches(ear_inches, pos);
            let Some(curve) = row.volume_falloff.as_ref() else {
                diag::warn!(
                    Audio,
                    "audio: world alias `{alias}` has no falloff curve (typed gap)"
                );
                return OneshotStart::failed(StartFailure::NoFalloffCurve)
                    .with_variant(variant_index);
            };
            let knots = shared.intern_curve(&curve.name, &curve.knots);
            let near_knots = row
                .near_falloff
                .as_ref()
                .map(|curve| shared.intern_curve(&curve.name, &curve.knots));
            let atten = distance_attenuation(
                &knots,
                near_knots.as_deref(),
                dist,
                row.dist_min,
                row.dist_max,
            );
            let emitter = Vec3::from_array(pos);
            let (pan_l, pan_r) = world_oneshot_channel_gains(ear, right, emitter, 1.0);
            if atten < 0.0 {
                diag::warn!(
                    Audio,
                    "audio: world alias `{alias}` falloff curve `{}` failed evaluation (typed gap)",
                    curve.name
                );
                return OneshotStart::failed(StartFailure::FalloffEval).with_variant(variant_index);
            }
            // bo2mp: Ghosts' start rules (`BO2MP_IW6_GATING`): inside the
            // near distance of an alias flagged to drop there, or past its
            // far distance, it does not start.
            let gated = namespace == AssetNamespace::Iw6
                && asset_audio::iw6_rules::Iw6Switches::get().gating
                && row.iw6.as_deref().is_some_and(|p| {
                    asset_audio::iw6_rules::play_gate(p.flags, dist, row.dist_min, row.dist_max, p.velocity_min, crate::iw6_sound::listener_speed()).is_err()
                });
            let atten = if gated { 0.0 } else { atten };
            if atten == 0.0 {
                // bo2mp: out of this layer's reach, but its secondary is a
                // sound of its own (a gun's npc shot chains to its LFE, then
                // to the distant shot and its decay, each with a longer
                // reach): it is still started, so far gunfire is heard.
                let secondary = if namespace == AssetNamespace::T5 {
                    None
                } else {
                    play_secondary_layer(
                        commands,
                        pcm_assets,
                        shared,
                        bank,
                        iwd,
                        namespace,
                        layer,
                        origin_inches,
                        listener,
                        pick,
                        clips,
                        pending,
                        occupancy,
                        snd_ent,
                        depth,
                        class,
                        epoch,
                    )
                };
                return OneshotStart {
                    outcome: StartOutcome::Suppressed(SuppressReason::Inaudible),
                    variant: Some(variant_index),
                    secondary,
                    detail: Some(falloff_detail(dist, row.dist_min, row.dist_max, atten)),
                };
            }
            if let Err(reason) = prepare_voice(commands, occupancy, bank, channel, snd_ent)
                .and_then(|()| {
                    prepare_alias_voice(
                        commands,
                        occupancy,
                        namespace,
                        limit_name,
                        Some(row),
                        snd_ent,
                        priority,
                    )
                })
            {
                return OneshotStart {
                    outcome: StartOutcome::Suppressed(reason),
                    variant: Some(variant_index),
                    secondary: None,
                    detail: Some(falloff_detail(dist, row.dist_min, row.dist_max, atten)),
                };
            }
            world_detail = Some(falloff_detail(dist, row.dist_min, row.dist_max, atten));
            pick.last_variant
                .insert((namespace, alias.to_owned()), variant_index);
            let _ = shared.dry_handle(pcm_assets, clip, pcm.clone(), clips.as_deref());
            let live = match room_echo_for(namespace, Some(row)) {
                Some((params, send)) => pcm.with_live_pan().with_echo(params, send),
                None => pcm.with_live_pan(),
            };
            let live_pan = live.live_pan().expect("with_live_pan").clone();
            live_pan.set(pan_l, pan_r);
            let handle = pcm_assets.add(live);
            let lease = voice_lease(bank, channel, snd_ent);
            let entity = crate::backend::spawn_oneshot(
                commands,
                handle,
                Volume::Linear((volume * atten).max(0.0)),
                pitch,
                epoch,
                class.scope(),
            );
            {
                let mut spawned = commands.entity(entity);
                spawned.insert(AliasPlayback {
                    namespace,
                    snd_ent,
                    alias: alias.to_owned(),
                });
                spawned.insert(Channel3d {
                    origin_inches: pos,
                    dist_min: row.dist_min,
                    dist_max: row.dist_max,
                    knots,
                    near_knots,
                    priority: row.voice_priority.clone(),
                    base_volume: volume.max(0.0),
                    live_pan,
                });
                if let Some(lease) = lease {
                    spawned.insert(lease);
                }
                // bo2mp: Ghosts' Doppler on the aliases that allow it
                // (`BO2MP_IW6_DOPPLER`).
                if namespace == AssetNamespace::Iw6
                    && asset_audio::iw6_rules::Iw6Switches::get().doppler
                    && let Some(p) = row.iw6.as_deref().filter(|p| p.allow_doppler)
                {
                    spawned.insert(Iw6Doppler {
                        state: asset_audio::iw6_rules::Doppler::new(p.doppler.unwrap_or_default()),
                        base_pitch: pitch,
                        last_ear: None,
                    });
                }
                // bo2mp: Ghosts' world occlusion on aliases with a shape of
                // their own (`BO2MP_IW6_OCCLUSION`).
                if namespace == AssetNamespace::Iw6
                    && crate::iw6_sound::occlusion_on()
                    && let Some(occ) = row.iw6.as_deref().and_then(|p| p.occlusion).and_then(crate::iw6_sound::Iw6Occlusion::new)
                {
                    spawned.insert(occ);
                }
            }
            track_voice(occupancy, entity, lease);
            if namespace == AssetNamespace::T5 {
                let lease = occupancy.track_alias(entity, namespace, limit_name, snd_ent, priority);
                commands.entity(entity).insert(lease);
            }
        }
        None => {
            if let Err(reason) = prepare_voice(commands, occupancy, bank, channel, snd_ent)
                .and_then(|()| {
                    prepare_alias_voice(
                        commands, occupancy, namespace, limit_name, row, snd_ent, priority,
                    )
                })
            {
                return OneshotStart {
                    outcome: StartOutcome::Suppressed(reason),
                    variant: Some(variant_index),
                    secondary: None,
                    detail: None,
                };
            }
            pick.last_variant
                .insert((namespace, alias.to_owned()), variant_index);
            let handle = match room_echo_for(namespace, row) {
                Some((params, send)) => {
                    let _ = shared.dry_handle(pcm_assets, clip, pcm.clone(), clips.as_deref());
                    pcm_assets.add(pcm.with_echo(params, send))
                }
                None => shared.dry_handle(pcm_assets, clip, pcm, clips.as_deref()),
            };
            let lease = voice_lease(bank, channel, snd_ent);
            let entity = crate::backend::spawn_oneshot(
                commands,
                handle,
                Volume::Linear(volume.max(0.0)),
                pitch,
                epoch,
                class.scope(),
            );
            {
                let mut spawned = commands.entity(entity);
                spawned.insert(AliasPlayback {
                    namespace,
                    snd_ent,
                    alias: alias.to_owned(),
                });
                if let Some(lease) = lease {
                    spawned.insert(lease);
                }
            }
            track_voice(occupancy, entity, lease);
            if namespace == AssetNamespace::T5 {
                let lease = occupancy.track_alias(entity, namespace, limit_name, snd_ent, priority);
                commands.entity(entity).insert(lease);
            }
        }
    }
    let secondary = if namespace == AssetNamespace::T5 {
        None
    } else {
        play_secondary_layer(
            commands,
            pcm_assets,
            shared,
            bank,
            iwd,
            namespace,
            layer,
            origin_inches,
            listener,
            pick,
            clips,
            pending,
            occupancy,
            snd_ent,
            depth,
            class,
            epoch,
        )
    };
    OneshotStart {
        outcome: StartOutcome::Submitted,
        variant: Some(variant_index),
        secondary,
        detail: world_detail,
    }
}

fn play_secondary_layer(
    commands: &mut Commands,
    pcm_assets: &mut Assets<PcmAudio>,
    shared: &mut SharedPlayAssets,
    bank: &SoundCatalog,
    iwd: Option<&NamespaceSoundIwd>,
    namespace: AssetNamespace,
    layer: Option<&str>,
    origin_inches: Option<[f32; 3]>,
    listener: Option<(Vec3, Vec3)>,
    pick: &mut SoundPickState,
    clips: Option<&mut ClipStore>,
    pending: &mut PendingStarts,
    occupancy: &mut VoiceOccupancy,
    snd_ent: Option<u32>,
    depth: u8,
    class: SoundClass,
    epoch: u64,
) -> Option<(String, StartOutcome)> {
    // bo2zm: BO2 writes "null" for no second layer.
    let Some(sec) = layer.filter(|s| !s.is_empty() && !s.eq_ignore_ascii_case("null")) else {
        return None;
    };
    if depth >= 10 {
        return Some((sec.to_owned(), StartOutcome::Failed(StartFailure::NoPcm)));
    }
    let started = play_alias_oneshot_at(
        commands,
        pcm_assets,
        shared,
        bank,
        iwd,
        namespace,
        sec,
        None,
        origin_inches,
        listener,
        pick,
        clips,
        pending,
        occupancy,
        snd_ent,
        depth + 1,
        class,
        epoch,
        1.0,
    );
    // bo2mp: BO2's step gear (fly_gear_npc, fly_gear_plr), body falls and
    // jump exertions are aliases with no file of their own that only chain on
    // (to fly_cloth_npc, fly_weight_plr, ...); the layer came to what that
    // chained sound came to, not to the empty alias (~1500 false warnings in
    // a 10-minute match).
    let outcome = match started.secondary {
        Some((_, deeper)) if started.outcome == StartOutcome::Failed(StartFailure::NoPcm) => deeper,
        _ => started.outcome,
    };
    Some((sec.to_owned(), outcome))
}
