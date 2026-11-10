//! The run-time side of Modern Warfare Remastered's scriptables: damage to the next state, events
//! on entering a state, burning (health drain) and delayed state changes. Pure logic over a
//! [`Def`]; the caller owns the world (models, effects, sounds, radius damage).
//!
//! READ from the exe's server-side scriptable code (`docs/exe_notes/MWR_H1_SCRIPTABLES.md`): the
//! damage routine (health, carry-over into the next state, the two-group damage flag test), the
//! damage entry (how a hit becomes a kind mask, the parent share) and the health drain tick.
//! INFERRED items are marked on the items.

use crate::scriptable::{Def, Event, State};

/// Damage kind bits the engine builds for a hit. The low three say how it hurt, the next three who
/// hurt (READ from the damage entry): a state takes the hit only when its `damage_flags` share a
/// bit with the kind in BOTH groups ([`accepts`]).
pub mod kind {
    /// A direct hit (bullet, impact).
    pub const DIRECT: u8 = 1;
    /// An explosion's radius damage.
    pub const RADIAL: u8 = 2;
    /// A melee hit (the engine's means-of-death number 8 takes this bit instead of `DIRECT`;
    /// INFERRED to be the melee death).
    pub const MELEE: u8 = 4;
    /// The attacker is a player.
    pub const BY_PLAYER: u8 = 8;
    /// The attacker is an entity of type 1 or 18 (INFERRED: a vehicle or a turret).
    pub const BY_VEHICLE: u8 = 0x10;
    /// Anything else: the world, a trigger, a script.
    pub const BY_WORLD: u8 = 0x20;
    /// Self damage of a burning part (the health drain): everything.
    pub const ANY: u8 = 0x3f;
}

/// Whether a state with `flags` takes a hit of `kind` (READ).
pub fn accepts(flags: u8, kind: u8) -> bool {
    flags & kind & 0x07 != 0 && flags & kind & 0x38 != 0
}

/// What a part is doing: the health left in its state and which state it is in.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PartRun {
    pub health: u16,
    pub state: u8,
}

/// Something the engine would do now: an event of a state just entered, with the part it came
/// from. The caller turns it into effects, sounds, a model swap, radius damage...
#[derive(Clone, Debug, PartialEq)]
pub struct Effect {
    pub part: usize,
    pub state: usize,
    /// The state's tag (script string number): where on the model it happens.
    pub tag: u32,
    pub event: Event,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Timer {
    /// Health drain of a part, every `interval` ms for `amount`.
    Drain { part: usize, amount: i16, interval: u16, next: u64 },
    /// A state change after a delay.
    Change { part: usize, target: u8, at: u64 },
}

/// One placed scriptable at run time. It costs nothing while it has no timer ([`Machine::active`]).
#[derive(Clone, Debug)]
pub struct Machine {
    pub parts: Vec<PartRun>,
    timers: Vec<Timer>,
    seed: u32,
}

impl Machine {
    /// A fresh instance: every part in state 0 with that state's health. `seed` picks the random
    /// delays of state changes.
    pub fn new(def: &Def, seed: u32) -> Machine {
        let parts = def
            .parts
            .iter()
            .map(|p| PartRun { health: p.states.first().map_or(0, |s| s.max_health.min(0xffff) as u16), state: 0 })
            .collect();
        Machine { parts, timers: Vec::new(), seed: seed | 1 }
    }

    /// Runs the first states' events (INFERRED: the engine does so when the map starts; the car's
    /// and the bowl's first states have none, a light's has).
    pub fn start(&mut self, def: &Def, now: u64) -> Vec<Effect> {
        let mut out = Vec::new();
        for part in 0..self.parts.len() {
            self.run_state(def, part, 0, now, &mut out);
        }
        out
    }

    fn rand(&mut self) -> f32 {
        self.seed = self.seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        (self.seed >> 8) as f32 / (1u32 << 24) as f32
    }

    fn run_state(&mut self, def: &Def, part: usize, state: usize, now: u64, out: &mut Vec<Effect>) {
        let Some(st) = def.parts.get(part).and_then(|p| p.states.get(state)) else {
            return;
        };
        self.timers.retain(|t| !matches!(t, Timer::Drain { part: p, .. } | Timer::Change { part: p, .. } if *p == part));
        for ev in &st.events {
            match ev {
                Event::HealthDrain { amount, interval, .. } => {
                    // (INFERRED: the first tick is at once, then every interval.)
                    self.timers.push(Timer::Drain { part, amount: *amount, interval: (*interval).max(1), next: now });
                }
                Event::StateChange { target, delay_min, delay_max } => {
                    let span = (f32::from(*delay_max) - f32::from(*delay_min)).max(0.0);
                    let d = f32::from(*delay_min) + span * self.rand();
                    self.timers.push(Timer::Change { part, target: *target, at: now + d as u64 });
                }
                other => out.push(Effect { part, state, tag: st.tag, event: other.clone() }),
            }
        }
    }

    /// One hit on one part (READ: the engine's damage routine). A state that takes the kind loses
    /// health; damage over the state's health carries into the next state's, state after state,
    /// until a state keeps some or the last one is reached; entering a state runs its events. A
    /// state that does not take the kind starts with all its health.
    pub fn damage_part(&mut self, def: &Def, part: usize, kind: u8, amount: i32, now: u64) -> Vec<Effect> {
        let mut out = Vec::new();
        let (Some(run), Some(p)) = (self.parts.get(part).copied(), def.parts.get(part)) else {
            return out;
        };
        let Some(cur) = p.states.get(run.state as usize) else {
            return out;
        };
        if run.health == 0 || !accepts(cur.damage_flags, kind) {
            return out;
        }
        let mut left = i32::from(run.health) - amount;
        if left >= 1 {
            self.parts[part].health = left as u16;
            return out;
        }
        let mut state = run.state as usize;
        loop {
            state += 1;
            let Some(next) = p.states.get(state) else {
                break;
            };
            self.parts[part].state = state as u8;
            self.run_state(def, part, state, now, &mut out);
            let max = i32::from(next.max_health.min(0xffff) as u16);
            // (The remainder stays signed for the next state; the stored health is clamped.)
            left = if max == 0 { 0 } else { left + max };
            self.parts[part].health = left.clamp(0, 0xffff) as u16;
            if state + 1 == p.states.len() {
                break;
            }
            if !accepts(next.damage_flags, kind) {
                left = max;
                self.parts[part].health = max as u16;
            }
            if left >= 1 {
                break;
            }
        }
        out
    }

    /// A hit on `part` (0 is the whole), with the parent rule (READ): a part whose state passes
    /// some of its damage to the whole (`parent_transfer_rate` / 255) hurts part 0 by that share
    /// first, at least 1. (Not followed: the share the whole gives its children.)
    pub fn hit(&mut self, def: &Def, part: usize, kind: u8, amount: i32, now: u64) -> Vec<Effect> {
        let mut out = Vec::new();
        if part != 0
            && let Some(rate) = self.current(def, part).map(|s| f32::from(s.parent_transfer_rate) / 255.0)
            && rate > 0.0
        {
            out.extend(self.damage_part(def, 0, kind, ((amount as f32 * rate) as i32).max(1), now));
        }
        out.extend(self.damage_part(def, part, kind, amount.max(1), now));
        out
    }

    /// The state a part is in.
    pub fn current<'a>(&self, def: &'a Def, part: usize) -> Option<&'a State> {
        def.parts.get(part)?.states.get(self.parts.get(part)?.state as usize)
    }

    /// Time passes: burning parts lose their drain's health at its interval (self damage of every
    /// kind, READ), delayed state changes happen. Does nothing while no timer is set.
    pub fn tick(&mut self, def: &Def, now: u64) -> Vec<Effect> {
        let mut out = Vec::new();
        if self.timers.is_empty() {
            return out;
        }
        for _ in 0..64 {
            let due = self.timers.iter().position(|t| match t {
                Timer::Drain { next, .. } => *next <= now,
                Timer::Change { at, .. } => *at <= now,
            });
            let Some(i) = due else { break };
            match self.timers[i] {
                Timer::Drain { part, amount, interval, next } => {
                    self.timers[i] = Timer::Drain { part, amount, interval, next: next + u64::from(interval) };
                    out.extend(self.damage_part(def, part, kind::ANY, i32::from(amount), now));
                }
                Timer::Change { part, target, .. } => {
                    self.timers.remove(i);
                    let target = usize::from(target);
                    if let Some(st) = def.parts.get(part).and_then(|p| p.states.get(target)) {
                        self.parts[part] = PartRun { health: st.max_health.min(0xffff) as u16, state: target as u8 };
                        self.run_state(def, part, target, now, &mut out);
                    }
                }
            }
        }
        out
    }

    /// Whether a timer is running (the instance needs ticking).
    pub fn active(&self) -> bool {
        !self.timers.is_empty()
    }

    /// Whether the whole (part 0) is in its last state.
    pub fn destroyed(&self, def: &Def) -> bool {
        def.parts.first().zip(self.parts.first()).is_some_and(|(p, r)| usize::from(r.state) + 1 == p.states.len())
    }
}

/// The damage kind mask for a hit (READ from the damage entry; the means-of-death numbers are
/// INFERRED): how it hurt plus who hurt.
pub fn kind_of(radial: bool, melee: bool, attacker: Attacker) -> u8 {
    let how = if radial {
        kind::RADIAL
    } else if melee {
        kind::MELEE
    } else {
        kind::DIRECT
    };
    how | match attacker {
        Attacker::Player => kind::BY_PLAYER,
        Attacker::Vehicle => kind::BY_VEHICLE,
        Attacker::World => kind::BY_WORLD,
    }
}

/// Who hurt a scriptable.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Attacker {
    Player,
    Vehicle,
    World,
}

/// The damage an explosion event does at `distance` (INFERRED: the linear fall-off from the
/// maximum at the middle to the minimum at the radius that every Call of Duty uses; the exe's own
/// curve was not followed).
pub fn blast_damage(damage_min: u16, damage_max: u16, radius: u16, distance: f32) -> Option<i32> {
    let r = f32::from(radius);
    if r <= 0.0 || distance > r {
        return None;
    }
    let t = (distance / r).clamp(0.0, 1.0);
    Some((f32::from(damage_max) + (f32::from(damage_min) - f32::from(damage_max)) * t) as i32)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scriptable::{Part, State};

    fn state(health: u32, flags: u8, events: Vec<Event>) -> State {
        State { max_health: health, damage_flags: flags, events, ..Default::default() }
    }

    /// The Crash sedan's root part as the zone has it (250 / 200 / 100 smoke, 300 / 400 burning
    /// with a drain of 12 every 200 ms, then destroyed with its blast), and a hood part.
    fn car() -> Def {
        let drain = Event::HealthDrain { amount: 12, interval: 200, bad_place_radius: 281 };
        let fire = |extra: bool| {
            let mut v = vec![Event::PlayFx { fx: Some("fire".into()), tag: 32, loop_time: 400, use_angles: true }];
            if extra {
                v.push(Event::PlaySound { alias: Some("flare".into()), looping: false });
            }
            v.push(drain.clone());
            v
        };
        let fx = |n: &str| vec![Event::PlayFx { fx: Some(n.into()), tag: 32, loop_time: 400, use_angles: true }];
        Def {
            name: "car".into(),
            parts: vec![
                Part {
                    states: vec![
                        state(250, 0x3b, vec![]),
                        state(200, 0x3b, fx("white")),
                        state(100, 0x3b, fx("black")),
                        state(300, 0x0b, fire(true)),
                        state(400, 0x3b, fire(false)),
                        state(
                            0,
                            0x3b,
                            vec![
                                Event::SetModel { model: Some("destroyed".into()) },
                                Event::Explosion {
                                    force_min: 4000,
                                    force_max: 5000,
                                    radius: 375,
                                    damage_min: 20,
                                    damage_max: 300,
                                    ai_avoid: false,
                                },
                            ],
                        ),
                    ],
                    ..Default::default()
                },
                Part {
                    states: vec![
                        State { max_health: 1, damage_flags: 0x38, parent_transfer_rate: 255, ..Default::default() },
                        state(
                            0,
                            0x3f,
                            vec![Event::PhysicsLaunch {
                                model: Some("hood".into()),
                                dir: [0; 3],
                                expl_force_scale: 1,
                                bullet_force_scale: 1,
                            }],
                        ),
                    ],
                    ..Default::default()
                },
            ],
            ..Default::default()
        }
    }

    const BULLET_BY_PLAYER: u8 = kind::DIRECT | kind::BY_PLAYER;

    #[test]
    fn flags_need_both_groups() {
        assert!(accepts(0x3f, BULLET_BY_PLAYER));
        assert!(accepts(0x3b, BULLET_BY_PLAYER));
        assert!(!accepts(0x3b, kind::MELEE | kind::BY_PLAYER), "bit 4 is not set");
        assert!(!accepts(0x38, BULLET_BY_PLAYER), "no how-bit at all: only the parent passes damage on");
        assert!(!accepts(0x0b, kind::DIRECT | kind::BY_VEHICLE), "only a player hurts a fire state");
        assert!(accepts(0x0b, kind::ANY));
    }

    #[test]
    fn damage_below_the_health_only_lowers_it() {
        let d = car();
        let mut m = Machine::new(&d, 1);
        assert_eq!(m.parts[0], PartRun { health: 250, state: 0 });
        assert!(m.damage_part(&d, 0, BULLET_BY_PLAYER, 100, 0).is_empty());
        assert_eq!(m.parts[0], PartRun { health: 150, state: 0 });
    }

    #[test]
    fn damage_over_the_health_carries_into_the_next_state() {
        let d = car();
        let mut m = Machine::new(&d, 1);
        // 50 over the first state's 250: the white smoke state keeps 200 - 50.
        let fx = m.damage_part(&d, 0, BULLET_BY_PLAYER, 300, 0);
        assert_eq!(m.parts[0], PartRun { health: 150, state: 1 });
        assert_eq!(fx.len(), 1);
        assert!(matches!(&fx[0].event, Event::PlayFx { fx: Some(n), .. } if n == "white"));
        // 150 + 100 + more: through the black smoke state into the fire state, which a player's
        // bullet does not hurt (flags 0x0b share no bit with... it does: 0x08 is the player bit),
        // so the fire state keeps what is left after its 300.
        let fx = m.damage_part(&d, 0, BULLET_BY_PLAYER, 150 + 100 + 10, 0);
        assert_eq!(m.parts[0].state, 3);
        assert_eq!(m.parts[0].health, 290);
        assert!(fx.iter().any(|e| matches!(&e.event, Event::PlayFx { fx: Some(n), .. } if n == "black")));
        assert!(fx.iter().any(|e| matches!(&e.event, Event::PlayFx { fx: Some(n), .. } if n == "fire")));
        assert!(m.active(), "the fire state's drain runs");
    }

    #[test]
    fn a_burning_car_drains_to_its_blast() {
        let d = car();
        let mut m = Machine::new(&d, 1);
        m.damage_part(&d, 0, kind::ANY, 250 + 200 + 100 + 1, 0);
        assert_eq!(m.parts[0].state, 3);
        let mut blast = None;
        let mut t = 0;
        while t < 30_000 && blast.is_none() {
            t += 50;
            for e in m.tick(&d, t) {
                if matches!(e.event, Event::Explosion { .. }) {
                    blast = Some(t);
                }
            }
        }
        let t = blast.expect("the car blows up");
        // 300 + 400 health at 12 per 200 ms: about 11.7 s of burning.
        assert!((11_000..=12_500).contains(&t), "blew up after {t} ms");
        assert!(m.destroyed(&d));
        assert!(!m.active(), "nothing ticks once it is destroyed");
    }

    #[test]
    fn a_part_passes_damage_to_the_whole() {
        let d = car();
        let mut m = Machine::new(&d, 1);
        let fx = m.hit(&d, 1, BULLET_BY_PLAYER, 10, 0);
        // The hood takes no direct damage in its first state (flags 0x38); the whole takes the
        // share the part passes on (255/255 of 10).
        assert!(fx.is_empty());
        assert_eq!(m.parts[1], PartRun { health: 1, state: 0 });
        assert_eq!(m.parts[0].health, 240);
    }

    #[test]
    fn a_state_change_event_waits_its_delay() {
        let d = Def {
            parts: vec![Part {
                states: vec![
                    state(0, 0x3f, vec![Event::StateChange { target: 1, delay_min: 1000, delay_max: 1000 }]),
                    state(0, 0x3f, vec![Event::SetModel { model: Some("b".into()) }]),
                ],
                ..Default::default()
            }],
            ..Default::default()
        };
        let mut m = Machine::new(&d, 7);
        assert!(m.start(&d, 0).is_empty());
        assert!(m.tick(&d, 999).is_empty());
        let fx = m.tick(&d, 1000);
        assert_eq!(fx.len(), 1);
        assert_eq!(m.parts[0].state, 1);
        assert!(!m.active());
    }

    #[test]
    fn blast_falls_off_to_the_radius() {
        assert_eq!(blast_damage(20, 300, 375, 0.0), Some(300));
        assert_eq!(blast_damage(20, 300, 375, 375.0), Some(20));
        assert_eq!(blast_damage(20, 300, 375, 400.0), None);
    }

    /// The Crash sedan and bowl from the real zone (`H1_ZONE=…/mp_crash.ff cargo test -p
    /// fastfile_h1 --lib -- --ignored real_crash`): the car's flags and healths drive the same
    /// story as the synthetic one.
    #[test]
    #[ignore = "needs a staged mp_crash.ff (H1_ZONE)"]
    fn real_crash_scriptables_run() {
        let Some(path) = std::env::var_os("H1_ZONE") else { return };
        let zone = crate::Zone::open(std::path::Path::new(&path)).expect("zone");
        let (map, cap) = crate::map::IwMap::load(&zone);
        let clip = map.clip.expect("clip");
        assert_eq!(clip.scriptables.len(), 18);
        let car = cap.scriptable_defs.iter().find(|d| d.name == "destpv_civ_80s_sedan1_h1_brn").expect("car");
        let mut m = Machine::new(car, 3);
        assert_eq!(m.parts[0], PartRun { health: 250, state: 0 });
        let mut blast = None;
        m.damage_part(car, 0, kind::ANY, 250 + 200 + 100 + 1, 0);
        let mut t = 0;
        while t < 60_000 && blast.is_none() {
            t += 50;
            for e in m.tick(car, t) {
                if let Event::Explosion { radius, damage_max, .. } = e.event {
                    assert_eq!((radius, damage_max), (375, 300));
                    blast = Some(t);
                }
            }
        }
        assert!(blast.is_some_and(|t| t > 5_000 && t < 20_000), "{blast:?}");
        let bowl = cap.scriptable_defs.iter().find(|d| d.name == "destp_rec_apple_bowl").expect("bowl");
        let mut b = Machine::new(bowl, 3);
        let fx = b.damage_part(bowl, 0, kind_of(false, false, Attacker::Player), 30, 0);
        assert!(fx.iter().any(|e| matches!(e.event, Event::PhysicsLaunch { .. })));
    }
}
