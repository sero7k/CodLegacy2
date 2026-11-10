//! Black Ops III's out-of-bounds (`scripts/shared/_oob.gsc` / `_oob.csc`,
//! read from the owner's `core_patch.ff`): a player inside a
//! `trigger_out_of_bounds` (or `trigger_radius_out_of_bounds`) is not killed
//! on touch, as Black Ops II's `trigger_hurt` death barriers kill, but shown
//! the "return to the combat zone" screen with a countdown, and killed when
//! it runs out.
//!
//! Numbers, from `_oob.gsc`'s multiplayer branch (its `getdvarint` defaults,
//! read off the bytecode's constants, campaign values in brackets):
//! - `oob_timelimit_ms` 3000 (6000): the countdown;
//! - `oob_timekeep_ms` 3000: leaving the volume and coming back within it
//!   keeps the time already spent (how the script uses it is not decoded:
//!   here the spent time is kept whole, UNVERIFIED);
//! - `oob_damage_interval_ms` 3000 (1000), `oob_damage_per_interval` 999 (5):
//!   once the time is up, 999 `MOD_TRIGGER_HURT` at once and every 3 s
//!   after, a death in multiplayer;
//! - `oob_max_distance_before_black` 100000 (400) and
//!   `oob_time_remaining_before_black` -1 (1000): multiplayer never fades to
//!   black.
//!
//! The dvars of those names override them, as the script's `getdvarint` does.
//!
//! The client side (`_oob.csc`, the LUI widget `CoD.OutOfBounds` in
//! mp_patch, the filter `generic_filter_out_of_bounds`) is drawn by the
//! client from the client dvar [`OOB_DVAR`] (`<ms left>,<ms spent>`, empty
//! when in bounds): the screen by `ui::oob_screen`, the filter by
//! `render_t7`'s post chain. The loop is `mp_common`'s `mpl_oob_static_lp`
//! (the csc's own `uin_out_of_bounds_loop` is defined by no staged zone).

use std::collections::BTreeMap;

use bevy_ecs::prelude::World;

use super::with_vm;

pub(crate) const OOB_TIMELIMIT_MS: i64 = 3000;
pub(crate) const OOB_TIMEKEEP_MS: i64 = 3000;
pub(crate) const OOB_DAMAGE_INTERVAL_MS: i64 = 3000;
pub(crate) const OOB_DAMAGE_PER_INTERVAL: i32 = 999;

/// The client dvar the client draws the screen and filter from.
pub const OOB_DVAR: &str = "bo2mp_oob";
/// The loop while out of bounds (Black Ops III's `mp_common` alias).
pub(crate) const OOB_LOOP_SOUND: &str = "t7:mpl_oob_static_lp";

/// The timings in force (the script's dvars, or its defaults).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct OobRules {
    pub timelimit_ms: i64,
    pub timekeep_ms: i64,
    pub damage_interval_ms: i64,
    pub damage_per_interval: i32,
}

impl Default for OobRules {
    fn default() -> Self {
        Self {
            timelimit_ms: OOB_TIMELIMIT_MS,
            timekeep_ms: OOB_TIMEKEEP_MS,
            damage_interval_ms: OOB_DAMAGE_INTERVAL_MS,
            damage_per_interval: OOB_DAMAGE_PER_INTERVAL,
        }
    }
}

/// One player's out-of-bounds state.
#[derive(Clone, Debug, Default)]
pub(crate) struct OobPlayer {
    /// Inside a volume since (ms, moved back by kept time).
    start: Option<i64>,
    /// Left at (ms) with this much spent.
    left: Option<(i64, i64)>,
    /// Next damage once the time is up.
    next_damage: i64,
    /// What his client dvar says.
    sent: String,
}

#[derive(Default)]
pub(crate) struct Oob {
    pub players: BTreeMap<u32, OobPlayer>,
}

/// When the countdown of a player entering at `now` started: earlier by the
/// time he already spent if he left less than `timekeep` ago.
pub(crate) fn entry_start(now: i64, left: Option<(i64, i64)>, rules: OobRules) -> i64 {
    match left {
        Some((at, spent)) if now - at < rules.timekeep_ms => now - spent.clamp(0, rules.timelimit_ms),
        _ => now,
    }
}

/// What a frame inside does.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct OobTick {
    /// Milliseconds left on the countdown (0 when up).
    pub left_ms: i64,
    /// Milliseconds spent inside.
    pub spent_ms: i64,
    /// Damage to deal now.
    pub damage: Option<i32>,
}

/// One frame inside: the countdown, and once it is up, the damage at once
/// and every interval after.
pub(crate) fn tick(p: &mut OobPlayer, now: i64, rules: OobRules) -> OobTick {
    let start = *p.start.get_or_insert(now);
    let spent = (now - start).max(0);
    let left_ms = (rules.timelimit_ms - spent).max(0);
    let mut damage = None;
    if left_ms == 0 && now >= p.next_damage {
        p.next_damage = now + rules.damage_interval_ms.max(1);
        damage = Some(rules.damage_per_interval);
    }
    OobTick { left_ms, spent_ms: spent, damage }
}

/// The client dvar's value: `<ms left>,<ms spent>` (tenths of a second).
pub(crate) fn dvar_value(t: &OobTick) -> String {
    format!("{},{}", t.left_ms / 100 * 100, t.spent_ms / 100 * 100)
}

fn rules(world: &mut World) -> OobRules {
    let d = OobRules::default();
    with_vm(world, |vm, _| {
        let get = |name: &str, default: i64| {
            vm.dvars
                .iter()
                .find(|(k, _)| k.eq_ignore_ascii_case(name))
                .and_then(|(_, v)| v.trim().parse::<i64>().ok())
                .unwrap_or(default)
        };
        OobRules {
            timelimit_ms: get("oob_timelimit_ms", d.timelimit_ms),
            timekeep_ms: get("oob_timekeep_ms", d.timekeep_ms),
            damage_interval_ms: get("oob_damage_interval_ms", d.damage_interval_ms),
            damage_per_interval: get("oob_damage_per_interval", i64::from(d.damage_per_interval)) as i32,
        }
    })
    .unwrap_or(d)
}

fn send(world: &mut World, p: &mut OobPlayer, client: u32, value: String) {
    if p.sent != value {
        super::set_client_dvar(world, client, OOB_DVAR, &value);
        p.sent = value;
    }
}

/// Each authority frame: `inside` are the playing players in an
/// out-of-bounds volume (with the volume's entity number and bounds). Returns (client,
/// damage) for those whose time is up.
pub(super) fn advance(world: &mut World, state: &mut Oob, now: i64, playing: &[u32], inside: &BTreeMap<u32, (u32, String)>) -> Vec<(u32, i32)> {
    let rules = rules(world);
    let mut hits = Vec::new();
    // Gone, dead or back in bounds: the screen comes down.
    let leaving: Vec<u32> = state
        .players
        .iter()
        .filter(|(c, p)| p.start.is_some() && (!inside.contains_key(c) || !playing.contains(c)))
        .map(|(c, _)| *c)
        .collect();
    for c in leaving {
        let Some(mut p) = state.players.remove(&c) else { continue };
        let spent = p.start.map_or(0, |s| now - s);
        p.start = None;
        // (A death forgets the time spent: the next life starts afresh.)
        let alive = playing.contains(&c);
        p.left = alive.then_some((now, spent));
        send(world, &mut p, c, String::new());
        super::natives_fx::stop_local_sound(world, crate::world::ClientId(c), OOB_LOOP_SOUND);
        diag::info!(
            Sim,
            "bo2mp out of bounds (BO3): player {c} {} after {spent} ms",
            if alive { "back in bounds" } else { "dead or gone" }
        );
        state.players.insert(c, p);
    }
    state.players.retain(|c, _| playing.contains(c));
    for (&c, (volume, bounds)) in inside {
        if !playing.contains(&c) {
            continue;
        }
        let mut p = state.players.remove(&c).unwrap_or_default();
        let entering = p.start.is_none();
        if entering {
            p.start = Some(entry_start(now, p.left.take(), rules));
            p.next_damage = 0;
        }
        let t = tick(&mut p, now, rules);
        if entering {
            let at = super::frame(world).player(crate::world::ClientId(c)).map_or([0.0; 3], |ps| ps.origin);
            diag::info!(
                Sim,
                "bo2mp out of bounds (BO3): player {c} entered out-of-bounds volume #{volume} [{bounds}] at ({:.0} {:.0} {:.0}), {} ms to return",
                at[0],
                at[1],
                at[2],
                t.left_ms
            );
            super::natives_fx::local_sound(world, crate::world::ClientId(c), OOB_LOOP_SOUND);
        }
        send(world, &mut p, c, dvar_value(&t));
        if let Some(d) = t.damage {
            diag::info!(Sim, "bo2mp out of bounds (BO3): player {c} out of time: {d} damage");
            hits.push((c, d));
        }
        state.players.insert(c, p);
    }
    hits
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn countdown_then_damage_at_once_and_every_interval() {
        let r = OobRules::default();
        let mut p = OobPlayer::default();
        let t = tick(&mut p, 1000, r);
        assert_eq!((t.left_ms, t.spent_ms, t.damage), (3000, 0, None));
        assert_eq!(dvar_value(&t), "3000,0");
        let t = tick(&mut p, 2550, r);
        assert_eq!((t.left_ms, t.damage), (1450, None));
        assert_eq!(dvar_value(&t), "1400,1500");
        // Time up: 999 at once (a death in multiplayer), then every 3 s.
        let t = tick(&mut p, 4000, r);
        assert_eq!((t.left_ms, t.damage), (0, Some(999)));
        assert_eq!(tick(&mut p, 5000, r).damage, None);
        assert_eq!(tick(&mut p, 7000, r).damage, Some(999));
    }

    #[test]
    fn time_kept_when_back_within_timekeep() {
        let r = OobRules::default();
        // Left 1 s ago having spent 2 s: 1 s left.
        assert_eq!(entry_start(10_000, Some((9_000, 2_000)), r), 8_000);
        // Left longer ago than timekeep: a fresh countdown.
        assert_eq!(entry_start(10_000, Some((6_000, 2_000)), r), 10_000);
        assert_eq!(entry_start(10_000, None, r), 10_000);
    }

    /// Aquarium's (mp_biodome) lagoon volume `?51` (origin 0 -2957 -688,
    /// one hull: world -220..220, -4800..-1114, -1536..160; read with
    /// `t7_oob_check`): a player on the shore is out of it, one wading past
    /// y -1114 is in. None of the map's 688 spawn points is in any of its 33
    /// volumes.
    #[test]
    fn aquarium_lagoon_volume() {
        let hulls = [crate::SimTriggerHull { mid: [0.0, 0.0, 0.0], half: [220.0, 1843.0, 848.0], slabs: Vec::new() }];
        let v = crate::script::host::triggers::Volume::Hulls { origin: [0.0, -2957.0, -688.0], hulls: &hulls };
        let player = |p: [f32; 3]| v.touches([p[0] - 15.0, p[1] - 15.0, p[2]], [p[0] + 15.0, p[1] + 15.0, p[2] + 70.0]);
        assert!(!player([0.0, -1000.0, 0.0]));
        assert!(player([0.0, -1200.0, 0.0]));
        assert!(!player([0.0, -1200.0, 170.0]));
    }

    #[test]
    fn bo3_defaults() {
        let r = OobRules::default();
        assert_eq!((r.timelimit_ms, r.timekeep_ms, r.damage_interval_ms, r.damage_per_interval), (3000, 3000, 3000, 999));
        assert_eq!(OOB_LOOP_SOUND, "t7:mpl_oob_static_lp");
    }
}
