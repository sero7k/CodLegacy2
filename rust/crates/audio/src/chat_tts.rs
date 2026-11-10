//! bo2mp: chat lines read aloud in R.E.P.O.'s text-to-speech voice (its own
//! Klattersynth, run by `repo_tts`). Off unless Settings > Sound > CHAT
//! TEXT-TO-SPEECH is on, and only when R.E.P.O. is installed on this PC.

use std::sync::mpsc::{Receiver, TryRecvError};
use std::sync::Arc;

use bevy::audio::Volume;
use bevy::prelude::*;
use frame::{ChatLineReceived, GameSettings};
use repo_tts::{Install, Tts, Voice, SAMPLE_RATE};

use crate::backend::{self, AudioScope, MatchEpoch};
use crate::pcm::PcmAudio;

/// Lines still being made; more than this are dropped (a flood of chat).
const MAX_PENDING: usize = 4;

#[derive(Default)]
struct VoiceState {
    tts: Option<Tts>,
    /// REPO was looked for (and not found), so it is not looked for again.
    missing: bool,
    pending: Vec<Receiver<Result<Vec<f32>, String>>>,
}

pub(crate) fn register(app: &mut App) {
    app.init_non_send_resource::<VoiceNonSend>().add_systems(Update, speak_chat);
}

// (Receivers are not `Sync`, so the state lives outside the `Resource`s.)
#[derive(Default)]
struct VoiceNonSend(VoiceState);

fn start() -> Option<Tts> {
    let Some(root) = repo_tts::find_install() else {
        diag::info!(Audio, "chat voice: R.E.P.O. is not installed here (set BO2MP_REPO_DIR to its folder)");
        return None;
    };
    let Some(install) = Install::at(&root) else {
        diag::warn!(Audio, "chat voice: {} lacks Klattersynth or its Mono runtime", root.display());
        return None;
    };
    Some(Tts::start(install))
}

fn speak_chat(
    mut state: NonSendMut<VoiceNonSend>,
    mut lines: MessageReader<ChatLineReceived>,
    settings: Option<Res<GameSettings>>,
    epoch: Res<MatchEpoch>,
    mut pcm_assets: ResMut<Assets<PcmAudio>>,
    mut commands: Commands,
) {
    let state = &mut state.0;
    let on = settings.as_deref().is_some_and(|s| s.chat_tts);
    if !on {
        // (Lines heard while it is off are not read later.)
        lines.clear();
        state.pending.clear();
        return;
    }
    for line in lines.read() {
        if state.tts.is_none() && !state.missing {
            state.tts = start();
            state.missing = state.tts.is_none();
        }
        let Some(tts) = &state.tts else { continue };
        if state.pending.len() < MAX_PENDING && !line.text.trim().is_empty() {
            state.pending.push(tts.speak(&line.text, Voice::DEFAULT));
        }
    }
    let volume = settings.as_deref().map_or(1.0, |s| s.master_volume * s.voice_volume);
    state.pending.retain(|rx| match rx.try_recv() {
        Err(TryRecvError::Empty) => true,
        Err(TryRecvError::Disconnected) => false,
        Ok(Err(e)) => {
            diag::warn!(Audio, "chat voice: {e}");
            false
        }
        Ok(Ok(samples)) => {
            if let Some(clip) = PcmAudio::from_prepared(Arc::from(samples), 1, SAMPLE_RATE) {
                let handle = pcm_assets.add(clip);
                backend::spawn_oneshot(&mut commands, handle, Volume::Linear(volume.max(0.0)), 1.0, epoch.0, AudioScope::Match);
            }
            false
        }
    });
}
