use std::time::{Duration, Instant};

use bevy::{
    audio::{AudioPlayer, AudioSink, AudioSinkPlayback, PlaybackSettings, Volume},
    prelude::*,
};
use frame::ClientSet;

use crate::pcm::{LoopingPcmAudio, LoopingPcmPlayback, PcmAudio};
use crate::voice::reclaim_finished_voices;

const STARTING_TIMEOUT: Duration = Duration::from_millis(250);

#[derive(Resource, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MatchEpoch(pub u64);

impl MatchEpoch {
    pub fn bump(&mut self) {
        self.0 = self.0.wrapping_add(1);
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AudioScope {
    Menu,
    Match,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum VoiceKind {
    Oneshot,
    Loop,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum VoiceOwner {
    Exclusive,

    Attached,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum VoicePhase {
    Starting,
    Playing,
}

#[derive(Component, Debug)]
pub struct Voice {
    pub epoch: u64,
    pub scope: AudioScope,
    kind: VoiceKind,
    owner: VoiceOwner,
    phase: VoicePhase,
    started_at: Instant,
}

pub(crate) fn register(app: &mut App) {
    app.init_resource::<MatchEpoch>().add_systems(
        Update,
        (cancel_stale_match_voices, advance_voice_phases)
            .chain()
            .before(reclaim_finished_voices)
            .in_set(ClientSet::Effects),
    );
    crate::mix_probe::register(app);
    app.add_systems(Update, count_voices.in_set(ClientSet::Effects));
}

/// bo2mp: how often the voice and device counts are logged.
const VOICE_LOG_EVERY: Duration = Duration::from_secs(10);

/// bo2mp: every 10 s, how many voices are live (by kind and phase, and how
/// many reached the mixer), how many echo, how many samples the mixer pulled,
/// and how the sound device kept up. Static in a busy fight shows here as late
/// or overrun callbacks next to a high voice count.
fn count_voices(voices: Query<(&Voice, Has<AudioSink>)>, mut last: Local<Option<Instant>>) {
    let now = Instant::now();
    let since = *last.get_or_insert(now);
    let elapsed = now.duration_since(since);
    if elapsed < VOICE_LOG_EVERY {
        return;
    }
    *last = Some(now);
    let (mut oneshots, mut loops, mut starting, mut sinks) = (0usize, 0usize, 0usize, 0usize);
    for (voice, has_sink) in &voices {
        match voice.kind {
            VoiceKind::Oneshot => oneshots += 1,
            VoiceKind::Loop => loops += 1,
        }
        starting += usize::from(voice.phase == VoicePhase::Starting);
        sinks += usize::from(has_sink);
    }
    let (mixed, sources, echoes) = crate::pcm::mixer_counts();
    diag::info!(
        Audio,
        "sound voices: {} ({oneshots} one-shot, {loops} loop, {starting} starting, {sinks} in the mixer); {sources} sources, {echoes} echoing; {:.0} samples/s",
        oneshots + loops,
        mixed as f64 / elapsed.as_secs_f64(),
    );
    let (callbacks, mean, max, wait, late, overrun) = crate::mix_probe::take_report();
    diag::info!(
        Audio,
        "sound device: {callbacks} callbacks, mix {mean:.2} ms mean / {max:.2} ms max, longest wait {wait:.1} ms, {late} late, {overrun} overrun; Pro Audio {}",
        crate::mix_probe::pro_audio(),
    );
    let (samples, over_in, over_out, peak_in, peak_out) = bevy::audio::master_bus::take_meter();
    diag::info!(
        Audio,
        "sound master: {samples} samples, {over_in} over full scale before the limiter (peak {peak_in:.2}), {over_out} after (peak {peak_out:.2})"
    );
}

pub(crate) fn spawn_oneshot(
    commands: &mut Commands,
    handle: Handle<PcmAudio>,
    volume: Volume,
    speed: f32,
    epoch: u64,
    scope: AudioScope,
) -> Entity {
    commands
        .spawn((
            AudioPlayer(handle),
            PlaybackSettings::ONCE.with_volume(volume).with_speed(speed),
            Voice {
                epoch,
                scope,
                kind: VoiceKind::Oneshot,
                owner: VoiceOwner::Exclusive,
                phase: VoicePhase::Starting,
                started_at: Instant::now(),
            },
        ))
        .id()
}

pub(crate) fn spawn_loop(
    commands: &mut Commands,
    handle: Handle<LoopingPcmAudio>,
    volume: Volume,
    epoch: u64,
    scope: AudioScope,
) -> Entity {
    commands
        .spawn((
            LoopingPcmPlayback::new(handle, volume),
            Voice {
                epoch,
                scope,
                kind: VoiceKind::Loop,
                owner: VoiceOwner::Exclusive,
                phase: VoicePhase::Starting,
                started_at: Instant::now(),
            },
        ))
        .id()
}

pub(crate) fn attach_loop(
    commands: &mut Commands,
    entity: Entity,
    handle: Handle<LoopingPcmAudio>,
    volume: Volume,
    epoch: u64,
) {
    commands.entity(entity).insert((
        LoopingPcmPlayback::new(handle, volume),
        Voice {
            epoch,
            scope: AudioScope::Match,
            kind: VoiceKind::Loop,
            owner: VoiceOwner::Attached,
            phase: VoicePhase::Starting,
            started_at: Instant::now(),
        },
    ));
}

pub(crate) fn detach_loop(commands: &mut Commands, entity: Entity) {
    commands.entity(entity).remove::<(
        AudioPlayer<LoopingPcmAudio>,
        PlaybackSettings,
        AudioSink,
        Voice,
    )>();
}

pub(crate) fn stop(commands: &mut Commands, entity: Entity) {
    commands.entity(entity).try_despawn();
}

fn advance_voice_phases(
    mut voices: Query<(Entity, &mut Voice, Option<&AudioSink>)>,
    mut commands: Commands,
) {
    let now = Instant::now();
    for (entity, mut voice, sink) in &mut voices {
        match voice.phase {
            VoicePhase::Starting => {
                if sink.is_some() {
                    voice.phase = VoicePhase::Playing;
                    continue;
                }
                // CODLEGACY_HEADLESS=1: no device, so no sink comes. A one-shot is
                // done at once; a loop plays (silently) until it is stopped.
                if crate::AudioSilent::no_device() {
                    match voice.kind {
                        VoiceKind::Oneshot => end_voice(&mut commands, entity, voice.owner),
                        VoiceKind::Loop => voice.phase = VoicePhase::Playing,
                    }
                    continue;
                }
                if now.duration_since(voice.started_at) >= STARTING_TIMEOUT {
                    diag::warn!(
                        Audio,
                        "audio: voice start timed out waiting for sink (typed gap)"
                    );
                    end_voice(&mut commands, entity, voice.owner);
                }
            }
            VoicePhase::Playing => {
                if voice.kind != VoiceKind::Oneshot {
                    continue;
                }
                if sink.is_some_and(|s| s.empty()) {
                    end_voice(&mut commands, entity, voice.owner);
                }
            }
        }
    }
}

fn cancel_stale_match_voices(
    epoch: Res<MatchEpoch>,
    voices: Query<(Entity, &Voice)>,
    mut commands: Commands,
) {
    for (entity, voice) in &voices {
        if voice.scope != AudioScope::Match {
            continue;
        }
        if voice.epoch == epoch.0 {
            continue;
        }
        end_voice(&mut commands, entity, voice.owner);
    }
}

fn end_voice(commands: &mut Commands, entity: Entity, owner: VoiceOwner) {
    match owner {
        VoiceOwner::Exclusive => {
            commands.entity(entity).try_despawn();
        }
        VoiceOwner::Attached => {
            detach_loop(commands, entity);
        }
    }
}
