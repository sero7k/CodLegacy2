//! A flight loop that rides a thrown or fired projectile (the weapon file's
//! `projectileSound`: a tomahawk's spin, a grenade's whistle, a rocket's
//! burn), and stops when the projectile lands, sticks or goes off.
//!
//! The host's presented projectiles say which shots are in the air and
//! where; this reconciles that set against positional emitters that carry the
//! loop (the destructible props' loops are done the same way). Which alias a
//! weapon plays is only its file's: a weapon that names none plays none.
//! Nothing here allocates per frame once the scratch lists have grown.

use crate::ambient::{MapAmbient, MapEmitter};
use crate::clip_store::{ClipStore, clip_keys_for_alias};
use crate::pcm::PcmAudio;
use crate::playback::{MissingAliasGaps, SharedPlayAssets, SoundBank};
use asset_game::WeaponSoundSlot;
use assets::PreparedWeapons;
use bevy::prelude::*;
use entity_iw4::TR_STATIONARY;
use net::{PresentedProjectile, PresentedSnapshot};
use std::sync::Arc;

/// One loop in the world: which projectile it rides, and with which weapon's
/// alias (a different weapon on the same id restarts it).
#[derive(Component)]
pub(crate) struct ProjectileLoop {
    id: u32,
    weapon: u32,
}

/// A projectile that should have a loop: (id, weapon).
pub(crate) type LoopKey = (u32, u32);

/// Splits wanted loops against the live ones: `starts` get the indices into
/// `wanted` with no live loop, `stops` the indices into `live` with no
/// wanted projectile (landed, gone, or another weapon's). Fills the caller's
/// lists (cleared first) so a frame allocates nothing once they have grown.
pub(crate) fn reconcile(
    live: &[LoopKey],
    wanted: &[LoopKey],
    starts: &mut Vec<usize>,
    stops: &mut Vec<usize>,
) {
    starts.clear();
    stops.clear();
    for (i, key) in wanted.iter().enumerate() {
        if !live.contains(key) {
            starts.push(i);
        }
    }
    for (i, key) in live.iter().enumerate() {
        if !wanted.contains(key) {
            stops.push(i);
        }
    }
}

/// Whether a projectile's trajectory is one in flight. A shot at rest (stuck
/// in a wall, lying where it landed) has stopped whistling.
pub(crate) fn in_flight(trajectory_type: i32) -> bool {
    trajectory_type != TR_STATIONARY
}

#[derive(Default)]
pub(crate) struct Scratch {
    wanted: Vec<(LoopKey, [f32; 3])>,
    wanted_keys: Vec<LoopKey>,
    live: Vec<(Entity, LoopKey)>,
    live_keys: Vec<LoopKey>,
    starts: Vec<usize>,
    stops: Vec<usize>,
}

pub(crate) fn update(
    mut commands: Commands,
    presented: Option<Res<PresentedSnapshot>>,
    weapons: Option<Res<PreparedWeapons>>,
    cg_clock: Option<Res<net::FrameClock>>,
    mut playing: Query<(Entity, &ProjectileLoop, &mut MapEmitter, &mut Transform)>,
    bank: Option<Res<SoundBank>>,
    mut clips: Option<ResMut<ClipStore>>,
    mut pcm: ResMut<Assets<PcmAudio>>,
    mut shared: ResMut<SharedPlayAssets>,
    mut gaps: ResMut<MissingAliasGaps>,
    mut scratch: Local<Scratch>,
) {
    let scratch = &mut *scratch;
    scratch.wanted.clear();
    scratch.wanted_keys.clear();
    if let (Some(presented), Some(weapons)) = (presented.as_deref(), weapons.as_deref()) {
        let at_time = cg_clock
            .as_ref()
            .filter(|clock| clock.started())
            .map(|clock| clock.time())
            .unwrap_or_else(|| presented.tick().map(sim::level_time_ms).unwrap_or(0));
        let at_time = presented.trajectory_time_ms(at_time);
        for row in presented.presented_projectiles() {
            // (Only the host's own shot has an id; a predicted one is
            // replaced by it the next snapshot.)
            let PresentedProjectile::Authoritative(projectile) = row else {
                continue;
            };
            if !in_flight(projectile.pos.tr_type)
                || weapons
                    .0
                    .authored_weapon_sound(projectile.weapon, WeaponSoundSlot::Projectile)
                    .is_none()
            {
                continue;
            }
            let key = (projectile.id.0, projectile.weapon);
            scratch
                .wanted
                .push((key, presented.projectile_origin_at(projectile, at_time)));
            scratch.wanted_keys.push(key);
        }
    }

    scratch.live.clear();
    scratch.live_keys.clear();
    for (entity, tag, mut emitter, mut transform) in &mut playing {
        let key = (tag.id, tag.weapon);
        scratch.live.push((entity, key));
        scratch.live_keys.push(key);
        // It rides the shot.
        if let Some((_, origin)) = scratch.wanted.iter().find(|(k, _)| *k == key) {
            if emitter.origin_inches != *origin {
                emitter.origin_inches = *origin;
                transform.translation = Vec3::from_array(*origin);
            }
        }
    }
    reconcile(
        &scratch.live_keys,
        &scratch.wanted_keys,
        &mut scratch.starts,
        &mut scratch.stops,
    );
    for &i in &scratch.stops {
        commands.entity(scratch.live[i].0).try_despawn();
    }

    let (Some(bank), Some(clips), Some(weapons)) = (bank, clips.as_mut(), weapons) else {
        return;
    };
    for &i in &scratch.starts {
        let ((id, weapon), origin) = scratch.wanted[i];
        let Some((ns, alias)) = weapons.0.weapon_sound_key(weapon, WeaponSoundSlot::Projectile, &bank.0)
        else {
            continue;
        };
        let Some(key) = clip_keys_for_alias(&bank.0, ns, alias).into_iter().next() else {
            gaps.record(alias);
            continue;
        };
        clips.request(key.clone());
        let Some(Ok(audio)) = clips.ready(&key) else {
            continue;
        };
        let Some(sound) = bank.0.sound_in(ns, alias).and_then(|s| s.aliases.first()) else {
            gaps.record(alias);
            continue;
        };
        let knots = sound
            .volume_falloff
            .as_ref()
            .map(|curve| shared.intern_curve(&curve.name, &curve.knots))
            .unwrap_or_else(|| Arc::from(Vec::<[f32; 2]>::new()));
        let vol = sound.vol_min.max(0.0);
        commands.spawn((
            ProjectileLoop { id, weapon },
            MapAmbient,
            MapEmitter {
                origin_inches: origin,
                dist_min: sound.dist_min,
                dist_max: sound.dist_max,
                knots,
                base_gain: vol,
                pcm: pcm.add(audio),
                speed: 1.0,
                live_pan: None,
                line_end_inches: None,
            },
            Transform::from_translation(Vec3::from_array(origin)),
        ));
        if codlegacy_env::var_os("CODLEGACY_AUDIO_EQUIPLOG").is_some() {
            diag::info!(Audio, "audio: projectile loop `{alias}` on projectile {id} (weapon {weapon})");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(live: &[LoopKey], wanted: &[LoopKey]) -> (Vec<usize>, Vec<usize>) {
        let (mut starts, mut stops) = (Vec::new(), Vec::new());
        reconcile(live, wanted, &mut starts, &mut stops);
        (starts, stops)
    }

    #[test]
    fn a_new_shot_starts_and_a_landed_one_stops() {
        let (starts, stops) = run(&[(1, 10), (2, 10)], &[(2, 10), (3, 11)]);
        assert_eq!(starts, vec![1], "projectile 3 is new");
        assert_eq!(stops, vec![0], "projectile 1 has landed");
    }

    #[test]
    fn a_steady_set_does_nothing() {
        let (starts, stops) = run(&[(1, 10)], &[(1, 10)]);
        assert!(starts.is_empty() && stops.is_empty());
        let (starts, stops) = run(&[], &[]);
        assert!(starts.is_empty() && stops.is_empty());
    }

    #[test]
    fn another_weapon_on_the_same_id_restarts() {
        let (starts, stops) = run(&[(1, 10)], &[(1, 11)]);
        assert_eq!((starts, stops), (vec![0], vec![0]));
    }

    #[test]
    fn lists_are_reused_not_regrown() {
        let (mut starts, mut stops) = (Vec::with_capacity(8), Vec::with_capacity(8));
        reconcile(&[], &[(1, 1), (2, 1), (3, 1)], &mut starts, &mut stops);
        let cap = (starts.capacity(), stops.capacity());
        reconcile(&[(1, 1)], &[(1, 1)], &mut starts, &mut stops);
        assert!(starts.is_empty() && stops.is_empty());
        assert_eq!((starts.capacity(), stops.capacity()), cap);
    }

    #[test]
    fn a_shot_at_rest_is_not_in_flight() {
        assert!(!in_flight(TR_STATIONARY));
        assert!(in_flight(entity_iw4::TR_GRAVITY));
        assert!(in_flight(entity_iw4::TR_LINEAR));
    }
}
