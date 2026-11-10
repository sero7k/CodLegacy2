//! bo2mp: Ghosts' client sound rules that need the frame: bullet pass-bys
//! (whizby bands), the listener's speed (an alias's `velocityMin`) and
//! world occlusion of a voice (the alias's occlusion shape gain), from
//! `docs/exe_notes/IW6_VFX_AUDIO.md` B1, B3 and B7; the maths is in
//! `asset_audio::iw6_rules`.
//!
//! * `BO2MP_IW6_WHIZBY` (on): a Ghosts gun's bullet that passes the
//!   listener (`Iw6BulletPass`, written by the effects pass for each shot
//!   segment of another player) plays the near, medium or far
//!   `whizby_<band>_NN_L/R` pair at the two emitters either side of the
//!   closest point, the variation from a shuffled bag; `whizby` when the
//!   pair is not in the bank.
//! * `BO2MP_IW6_LISTENER_SPEED` (on): the listener's speed (inches a
//!   second, from the listener's motion) for the start gate.
//! * `BO2MP_IW6_OCCLUSION` (on): a positioned Ghosts voice whose alias
//!   carries a non-default occlusion shape is traced from its emitter (10
//!   units up) to the listener against the world, no more often than every
//!   150 ms and one voice a frame (the earliest due); while the trace is
//!   blocked its volume moves to the shape's occluded volume over 500 ms
//!   (the lerp's use is INFERRED: `snd_occlusionLerpTime`'s use site was
//!   not found). The occlusion table's EQ is not applied (no sound table in
//!   the zones read).

use std::sync::atomic::{AtomicU32, Ordering};

use bevy::prelude::*;

use asset_audio::iw6_rules::{self, OcclusionShape, ShuffleBag, WhizbyBand, WhizbyDistances};
use asset_core::AssetNamespace;

use crate::playback::{Channel3d, SoundBank};
use crate::{AmbientListener, WeaponSound, transform_inches};

/// A Ghosts gun's shot segment heard by the local client (another
/// player's bullet).
#[derive(Message, Clone, Copy, Debug)]
pub struct Iw6BulletPass {
    pub start: [f32; 3],
    pub end: [f32; 3],
}

pub fn whizby_on() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| iw6_rules::switch("WHIZBY", true))
}

pub fn listener_speed_on() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| iw6_rules::switch("LISTENER_SPEED", true))
}

pub fn occlusion_on() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| iw6_rules::switch("OCCLUSION", true))
}

/// The listener's speed (f32 bits; NaN: not known).
static LISTENER_SPEED: AtomicU32 = AtomicU32::new(0x7fc0_0000);

/// The listener's speed in inches a second, when known and the switch is
/// on (the start gate's `velocityMin`).
pub fn listener_speed() -> Option<f32> {
    let v = f32::from_bits(LISTENER_SPEED.load(Ordering::Relaxed));
    (listener_speed_on() && v.is_finite()).then_some(v)
}

/// A step of the listener longer than this in a frame is a teleport (a
/// spawn, a killcam): no speed.
const TELEPORT_INCHES: f32 = 256.0;

/// The listener's speed from its motion, smoothed over a few frames.
pub(crate) fn track_listener_speed(
    time: Res<Time>,
    listeners: Query<&Transform, With<AmbientListener>>,
    mut last: Local<Option<([f32; 3], f32)>>,
) {
    let Some(t) = listeners.iter().next() else {
        LISTENER_SPEED.store(f32::NAN.to_bits(), Ordering::Relaxed);
        *last = None;
        return;
    };
    let ear = transform_inches(t.translation);
    let dt = time.delta_secs();
    let speed = match *last {
        Some((prev, smooth)) if dt > 0.0 => {
            let d = crate::distance_inches(ear, prev);
            if d > TELEPORT_INCHES {
                None
            } else {
                Some(smooth + (d / dt - smooth) * (dt * 8.0).min(1.0))
            }
        }
        _ => None,
    };
    *last = Some((ear, speed.unwrap_or(0.0)));
    LISTENER_SPEED.store(speed.unwrap_or(f32::NAN).to_bits(), Ordering::Relaxed);
}

/// Variations of a band in the bank (`whizby_<band>_NN_L` from 00 up).
fn variation_count(bank: &asset_audio::SoundCatalog, band: WhizbyBand) -> u32 {
    (0..16)
        .take_while(|&i| bank.index_in(AssetNamespace::Iw6, &iw6_rules::whizby_aliases(band, i)[0]).is_some())
        .count() as u32
}

#[derive(Default)]
pub(crate) struct WhizbyState {
    bags: [ShuffleBag; 3],
    seed: u32,
    counts: Option<(u64, [u32; 3])>,
    noted: bool,
}

fn band_index(b: WhizbyBand) -> usize {
    match b {
        WhizbyBand::Near => 0,
        WhizbyBand::Medium => 1,
        WhizbyBand::Far => 2,
    }
}

pub(crate) fn play_iw6_whizby(
    mut passes: MessageReader<Iw6BulletPass>,
    listeners: Query<&Transform, With<AmbientListener>>,
    bank: Option<Res<SoundBank>>,
    mut sounds: MessageWriter<WeaponSound>,
    mut state: Local<WhizbyState>,
) {
    if !whizby_on() {
        passes.clear();
        return;
    }
    let (Some(bank), Some(t)) = (bank, listeners.iter().next()) else {
        passes.clear();
        return;
    };
    let ear = transform_inches(t.translation);
    let rev = bank.0.revision();
    if state.counts.is_none_or(|(r, _)| r != rev) {
        let c = [WhizbyBand::Near, WhizbyBand::Medium, WhizbyBand::Far].map(|b| variation_count(&bank.0, b));
        state.counts = Some((rev, c));
        if !state.noted {
            state.noted = true;
            diag::info!(Audio, "iw6 whizby: variations near/medium/far {c:?} in the bank (BO2MP_IW6_WHIZBY)");
        }
    }
    let counts = state.counts.map_or([0; 3], |(_, c)| c);
    for p in passes.read() {
        let Some(w) = iw6_rules::whizby(p.start, p.end, ear, WhizbyDistances::default()) else {
            continue;
        };
        let k = band_index(w.band);
        let st = &mut *state;
        let pair = if counts[k] > 0 {
            let i = st.bags[k].next(counts[k], &mut st.seed);
            let [l, r, _] = iw6_rules::whizby_aliases(w.band, i);
            Some((l, r))
        } else {
            None
        };
        match pair {
            Some((l, r)) => {
                for (alias, at) in [(l, w.left), (r, w.right)] {
                    sounds.write(WeaponSound { namespace: AssetNamespace::Iw6, alias, origin_inches: Some(at), snd_ent: None });
                }
            }
            None if bank.0.index_in(AssetNamespace::Iw6, "whizby").is_some() => {
                let mid = std::array::from_fn(|c| 0.5 * (w.left[c] + w.right[c]));
                sounds.write(WeaponSound { namespace: AssetNamespace::Iw6, alias: "whizby".into(), origin_inches: Some(mid), snd_ent: None });
            }
            None => {}
        }
    }
}

/// A Ghosts voice whose alias has a non-default occlusion shape.
#[derive(Component)]
pub(crate) struct Iw6Occlusion {
    pub shape: OcclusionShape,
    pub occluded: bool,
    /// The gain applied (moves toward 1 or the occluded volume).
    pub gain: f32,
    /// When the next trace is due (seconds of `Time`).
    pub due: f32,
}

impl Iw6Occlusion {
    pub(crate) fn new(shape: OcclusionShape) -> Option<Self> {
        (!shape.is_default).then_some(Self { shape, occluded: false, gain: 1.0, due: 0.0 })
    }
}

/// `snd_occlusionDelay` (ms) and `snd_occlusionLerpTime` (ms).
const OCCLUSION_DELAY_SECS: f32 = 0.150;
const OCCLUSION_LERP_SECS: f32 = 0.500;

/// The gain a voice moves to: 1 in the open, the shape's occluded volume
/// behind the world; `step` toward it by `dt / lerp`.
pub(crate) fn occlusion_gain_step(gain: f32, occluded: bool, shape: &OcclusionShape, dt: f32) -> f32 {
    let target = if occluded { shape.occluded_volume.clamp(0.0, 1.0) } else { 1.0 };
    let step = (dt / OCCLUSION_LERP_SECS).clamp(0.0, 1.0);
    gain + (target - gain).clamp(-step, step)
}

pub(crate) fn update_iw6_occlusion(
    time: Res<Time>,
    listeners: Query<&Transform, With<AmbientListener>>,
    prediction: Option<Res<net::ClientPredictionState>>,
    mut voices: Query<(&Channel3d, &mut Iw6Occlusion)>,
) {
    let Some(t) = listeners.iter().next() else {
        return;
    };
    let ear = transform_inches(t.translation);
    let now = time.elapsed_secs();
    let dt = time.delta_secs();
    let world = prediction
        .as_deref()
        .filter(|p| p.0.is_armed() && p.0.world().has_world_clip())
        .map(|p| p.0.world());
    // (One trace a frame: the voice due longest.)
    if let Some(world) = world
        && let Some((ch, mut occ)) = voices.iter_mut().filter(|(_, o)| o.due <= now).min_by(|a, b| a.1.due.total_cmp(&b.1.due))
    {
        let from = [ch.origin_inches[0], ch.origin_inches[1], ch.origin_inches[2] + 10.0];
        let hit = world.trace_static_world(from, ear, [0.0; 3], [0.0; 3], sim::CONTENTS_SOLID);
        occ.occluded = hit.startsolid == 0 && hit.fraction < 1.0;
        occ.due = now + OCCLUSION_DELAY_SECS;
    }
    for (_, mut occ) in &mut voices {
        let g = occlusion_gain_step(occ.gain, occ.occluded, &occ.shape.clone(), dt);
        occ.gain = g;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn occlusion_gain_moves_to_the_occluded_volume_over_half_a_second() {
        let shape = OcclusionShape { is_default: false, inner_angle: 90.0, outer_angle: 180.0, occluded_volume: 0.25 };
        let mut g = 1.0;
        for _ in 0..10 {
            g = occlusion_gain_step(g, true, &shape, 0.025);
        }
        assert!((g - 0.5).abs() < 1e-4, "{g}");
        for _ in 0..20 {
            g = occlusion_gain_step(g, true, &shape, 0.025);
        }
        assert!((g - 0.25).abs() < 1e-4);
        g = occlusion_gain_step(g, false, &shape, 1.0);
        assert!((g - 1.0).abs() < 1e-4);
    }

    #[test]
    fn a_default_shape_is_never_traced() {
        assert!(Iw6Occlusion::new(OcclusionShape { is_default: true, ..Default::default() }).is_none());
    }
}
