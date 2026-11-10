//! bo2mp: the engine half of Black Ops II's bots.
//!
//! The game's own bot scripts (`maps/mp/bots/_bot*`) decide everything a
//! bot does: where to go (named goals with priorities: "wander",
//! "enemy_patrol", "cover"...), whom to fight (`getthreats`), where to look,
//! when the trigger may be pulled, when to aim down the sights, throw,
//! reload, melee or dive. The engine carries it out: a test client joins
//! (`addtestclient`), walks the best goal's route over the map's path
//! nodes, turns its view toward what it looks at (or down its route) at its
//! difficulty's speed, and presses the buttons. Each tick a bot's command
//! replaces its client's own, as the zombies test player does.

use std::collections::BTreeMap;

use bevy_ecs::prelude::World;
use gsc_t6::{Array, ObjKind, ObjRef, Value, Vm};
use playerstate_iw4::buttons;

use super::{Player, Zm, arg, entnum, frame, list, origin_of, text};
use crate::world::ClientId;

/// A named place a bot wants to be (`addgoal`): the highest priority wins,
/// the newest among equals.
#[derive(Clone, Debug)]
struct Goal {
    name: String,
    pos: [f32; 3],
    radius: f32,
    priority: i32,
    order: u64,
}

/// What a bot's scripts set up for its custom classes (`botclassadditem`,
/// `botclassaddattachment`, `botsetdefaultclass`): read by the loadout.
#[derive(Clone, Debug, Default)]
pub(crate) struct BotClass {
    pub default: Option<String>,
    pub items: Vec<String>,
    /// (weapon, attachment, slot such as "primaryattachment1").
    pub attachments: Vec<(String, String, String)>,
}

/// A negotiation link on a route (BO2's path nodes mark ledges, railings
/// and drops with a begin node linked to an end node): the begin node's
/// spawnflags say how it is taken (0x800000 a jump, 0x2400000 a drop,
/// 0x2000000 run straight across).
#[derive(Clone, Copy, Debug)]
struct NegLink {
    begin: [f32; 3],
    end: [f32; 3],
    flags: u32,
}

const NEG_JUMP: u32 = 0x80_0000;
const NEG_DROP: u32 = 0x240_0000;
const NEG_RUN: u32 = 0x200_0000;

#[derive(Clone, Debug, Default)]
pub(crate) struct Bot {
    goals: Vec<Goal>,
    order: u64,
    /// Route to the best goal: points, the goal last.
    path: Vec<[f32; 3]>,
    path_to: Option<[f32; 3]>,
    replan_at: i64,
    look: Option<[f32; 3]>,
    allow_attack: bool,
    ads: bool,
    attack_ticks: u32,
    use_until: i64,
    melee: bool,
    dive: bool,
    /// 0 stand, 1 crouch, 2 prone (`setstance`).
    stance: u8,
    no_sprint: bool,
    /// The node `botsetfailsafenode` gave it (its place, and the node
    /// handed back with "goal"): it walks there and nothing else.
    failsafe: Option<([f32; 3], Option<ObjRef>)>,
    tick: u32,
    /// Where it was when it last made headway, and when (stuck checks).
    progress: Option<([f32; 3], i64)>,
    /// The route point it was stuck short of, and when that stuck began.
    stuck_next: Option<([f32; 3], i64)>,
    /// The point its route came from to `path[0]` (where it planned, or
    /// the last point it passed): it has passed `path[0]` once it is
    /// beyond the line across that point.
    prev: Option<[f32; 3]>,
    /// Counts new routes; with the route's length it names the step a
    /// cached far-sight answer belongs to.
    path_gen: u32,
    path_new: bool,
    /// BO2's lookahead amount for this route (32768 when planned, 0 =
    /// not set) and the last lookahead point the walk there was clear to.
    la_amount: f32,
    la_pos: Option<[f32; 3]>,
    /// BO2's lookahead direction (its z the rise per unit) and distance to
    /// the point the walk headed for at its last think, and whether that
    /// walk was clear: `getlookaheaddir`/`dist` answer from them, and route
    /// points behind it along them are done.
    la_dir: Option<([f32; 3], f32)>,
    la_clear: bool,
    /// The farthest route point seen from the one it heads to, for
    /// (route, points left, within 192 units of the next).
    seen: Option<(u32, usize, bool, [f32; 3])>,
    /// BO2's steering: the angles its view chases (pitch, yaw), smoothed
    /// toward what it wants to face, and the current turn speeds
    /// (degrees a second) that ramp up toward it.
    aim_avg: Option<[f32; 2]>,
    turn_speed: [f32; 2],
    /// A semi-automatic or burst weapon's trigger rests until then.
    fire_delay_until: i64,
    /// When its current sprint began.
    sprint_since: Option<i64>,
    /// The negotiation links its route crosses, the one it is taking (and
    /// since when), last tick's buttons, and whether it was mantling.
    negs: Vec<NegLink>,
    neg: Option<(NegLink, i64)>,
    last_press: u32,
    was_mantling: bool,
    /// BO2MP_BOTFIRELOG: the weapon it held and its magazine at the last
    /// tick, to tell whether the trigger pull before it let a shot out.
    fire_dbg: Option<(u32, i32)>,
    pub classes: BTreeMap<i32, BotClass>,
    /// Bot Warfare's overrides (`botbuttonoverride`, `botmovementoverride`,
    /// `botaimoverride`): buttons a script holds down until it lets go, a
    /// stick it sets, and whether a script turns the view itself.
    held: u32,
    stick: Option<(i8, i8)>,
    script_aims: bool,
    /// `botweaponoverride`: the weapon its command asks for (a script's
    /// switch moves it to that weapon), and `botmeleeparamsoverride`: the
    /// entity its knife lunges at and how far.
    weapon_override: Option<u32>,
    melee_target: Option<(u32, f32)>,
    /// bo2mp fixes: the way it faces when it has nowhere to head and a
    /// wall in front (and when that was worked out), the enemy in front of
    /// it since when, the enemy its reflex aims at (last seen), and the
    /// BO2MP_BOTSTATS counters' per-bot state.
    idle_look: Option<(Option<f32>, i64)>,
    front_seen: Option<(u32, i64)>,
    reflex: Option<(u32, i64)>,
    st_stuck_counted: bool,
    st_nofire_since: Option<i64>,
    st_missed_counted: bool,
    st_air: Option<(f32, bool)>,
}

impl Bot {
    /// The level restarts (`map_restart`): what its scripts set up for its
    /// classes stays; where it was going and what it was doing go.
    pub(crate) fn restarted(self) -> Self {
        Self {
            classes: self.classes,
            ..Self::default()
        }
    }

    fn best_goal(&self) -> Option<&Goal> {
        self.goals
            .iter()
            .max_by_key(|g| (g.priority, g.order))
    }
}

fn ent_origin(world: &mut World, n: u32) -> Option<[f32; 3]> {
    if world.resource::<Zm>().players.contains_key(&n) {
        return frame(world).player(ClientId(n)).map(|ps| ps.origin);
    }
    world.resource::<Zm>().ents.get(&n).map(|e| e.origin)
}

fn dist3(a: [f32; 3], b: [f32; 3]) -> f32 {
    ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)).sqrt()
}

fn dist2d(a: [f32; 3], b: [f32; 3]) -> f32 {
    ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2)).sqrt()
}

/// Yaw and pitch (degrees, the game's: pitch down positive) from a to b.
fn angles_to(a: [f32; 3], b: [f32; 3]) -> (f32, f32) {
    let d = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
    let yaw = d[1].atan2(d[0]).to_degrees();
    let flat = (d[0] * d[0] + d[1] * d[1]).sqrt();
    let pitch = -d[2].atan2(flat).to_degrees();
    (yaw, pitch)
}

fn angle_delta(from: f32, to: f32) -> f32 {
    (to - from + 540.0).rem_euclid(360.0) - 180.0
}

/// Weapon states (the movement code's numbering): reloading, any kind.
fn reloading(state: i32) -> bool {
    use weapon_iw4::WeaponState as S;
    (S::Reloading as i32..=S::ReloadEnd as i32).contains(&state)
}

/// A player's state, as `read` sees it (not a player: false).
fn player_is(vm: &Vm<World>, world: &mut World, s: &Value, read: fn(&playerstate_iw4::PlayerState) -> bool) -> Value {
    let n = entnum(vm, s).unwrap_or(u32::MAX);
    Value::bool(frame(world).player(ClientId(n)).is_some_and(read))
}

/// The danger count's slot for a team (BO2 keeps one per team).
fn danger_slot(team: &str) -> Option<usize> {
    match team {
        "allies" => Some(0),
        "axis" => Some(1),
        _ => None,
    }
}

/// The settings BO2's engine drives bots by (its own dvars; the bot
/// scripts raise `bot_yawspeed` and set the fire gaps per difficulty).
struct BotDvars {
    yaw_speed: f32,
    min_fire: i64,
    max_fire: i64,
    sprint_distance: f32,
    /// aim_turnrate_pitch / _ads, aim_turnrate_yaw / _ads (degrees a
    /// second) and aim_accel_turnrate_lerp (degrees a second, a second).
    pitch_rate: [f32; 2],
    yaw_rate: [f32; 2],
    accel: f32,
}

impl BotDvars {
    fn read(world: &mut World) -> Self {
        super::with_vm(world, |vm, _| {
            let f = |k: &str, d: f32| {
                vm.dvars
                    .get(k)
                    .and_then(|s| s.trim().parse::<f32>().ok())
                    .unwrap_or(d)
            };
            let (min_fire, max_fire) = (f("bot_minfiretime", 400.0) as i64, f("bot_maxfiretime", 600.0) as i64);
            Self {
                yaw_speed: f("bot_yawspeed", 1.0),
                min_fire,
                max_fire: max_fire.max(min_fire),
                sprint_distance: f("bot_sprintdistance", 256.0),
                pitch_rate: [f("aim_turnrate_pitch", 90.0), f("aim_turnrate_pitch_ads", 55.0)],
                yaw_rate: [f("aim_turnrate_yaw", 260.0), f("aim_turnrate_yaw_ads", 90.0)],
                accel: f("aim_accel_turnrate_lerp", 1200.0),
            }
        })
        .unwrap_or(Self {
            yaw_speed: 1.0,
            min_fire: 400,
            max_fire: 600,
            sprint_distance: 256.0,
            pitch_rate: [90.0, 55.0],
            yaw_rate: [260.0, 90.0],
            accel: 1200.0,
        })
    }
}

/// A number in lo..hi from the bot and the time (splitmix).
fn irand(n: u32, now: i64, lo: i64, hi: i64) -> i64 {
    if hi <= lo {
        return lo;
    }
    let mut z = (now as u64) ^ ((n as u64) << 40) ^ 0x9E37_79B9_7F4A_7C15;
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^= z >> 31;
    lo + (z % (hi - lo) as u64) as i64
}

/// BO2's lookahead down a route: along the route from where the bot is
/// until the corner it would cut (each leg's length times how far its line
/// passes from the bot, summed) reaches the amount; straight legs cost
/// nothing. The point there, and how far it is.
fn lookahead(origin: [f32; 3], path: &[[f32; 3]], amount: f32) -> Option<([f32; 3], f32)> {
    const AMOUNT_START: f32 = 32768.0;
    let amount = if amount > 0.0 { amount } else { AMOUNT_START };
    let mut area = 0.0f32;
    let mut a = origin;
    for &b in path {
        let d = [b[0] - a[0], b[1] - a[1]];
        let len = (d[0] * d[0] + d[1] * d[1]).sqrt();
        if len < 1e-3 {
            a = b;
            continue;
        }
        let dir = [d[0] / len, d[1] / len];
        let off = (dir[0] * (b[1] - origin[1]) - dir[1] * (b[0] - origin[0])).abs();
        let before = area;
        area += len * off;
        if off > 0.0 && area >= amount {
            let s = ((amount - before) / off).min(len) / len;
            let p = [a[0] + d[0] * s, a[1] + d[1] * s, a[2] + (b[2] - a[2]) * s];
            return Some((p, dist2d(origin, p)));
        }
        a = b;
    }
    path.last().map(|&p| (p, dist2d(origin, p)))
}

/// BO2's far sight down a route: from the point it heads to (the one
/// after when that is within 192 units), the farthest later point seen
/// from there; it looks toward that while it walks.
fn farthest_seen(world: &mut World, origin: [f32; 3], path: &[[f32; 3]]) -> Option<[f32; 3]> {
    // The nodes: all but the goal point at the end.
    let nodes = &path[..path.len().saturating_sub(1)];
    let start = usize::from(nodes.len() > 1 && dist2d(origin, nodes[1]) < 192.0);
    let raise = |p: [f32; 3]| [p[0], p[1], p[2] + 60.0];
    let from = raise(*nodes.get(start)?);
    let last = nodes.len().checked_sub(1)?.min(start + 20);
    for &p in nodes.get(start + 1..=last)?.iter().rev() {
        if dist2d(nodes[start], p) < 2048.0 && sight(world, from, raise(p)) {
            return Some(p);
        }
    }
    None
}

/// BO2's bot steering, one 50 ms tick: the view (pitch, yaw) turns toward
/// the angles it chases at a speed that scales with how far off it is
/// (a quarter second to close the gap from the hip, a fifth aimed in), up
/// to the turn rates, ramping up by the acceleration and dropping at once.
fn steer(view: [f32; 2], aim: [f32; 2], speed: &mut [f32; 2], ads: f32, dv: &BotDvars) -> [f32; 2] {
    let lerp = |r: [f32; 2]| r[0] + (r[1] - r[0]) * ads;
    let time = 0.2 * ads + 0.25 * (1.0 - ads);
    let rate = [lerp(dv.pitch_rate), lerp(dv.yaw_rate)];
    let mut axis: [f32; 2] =
        std::array::from_fn(|i| (angle_delta(view[i], aim[i]) / (time * rate[i]).max(1e-3)).clamp(-1.0, 1.0));
    // The smaller axis gives way to the larger (aim_scale_view_axis).
    let (p, y) = (axis[0].abs(), axis[1].abs());
    if y < p {
        axis[1] *= 1.0 - (p - y);
    } else {
        axis[0] *= 1.0 - (y - p);
    }
    let mut out = view;
    for i in 0..2 {
        let target = axis[i].abs() * rate[i] * dv.yaw_speed;
        speed[i] = if target <= speed[i] {
            target
        } else {
            (speed[i] + dv.accel * dv.yaw_speed * 0.05).min(target)
        };
        out[i] = view[i] + speed[i] * 0.05 * if axis[i] >= 0.0 { 1.0 } else { -1.0 };
    }
    out
}

/// BO2's crowd avoidance: each bot keeps a circle (30 units; a player's is
/// 50) and picks the velocity nearest the way it wants to go that misses
/// its teammates for the next two seconds (reciprocal velocity obstacles).
const AVOID_HORIZON: f32 = 2.0;
const BOT_AVOID_RADIUS: f32 = 30.0;
const BOT_AVOID_SPEED: f32 = 100.0;
const PLAYER_AVOID_RADIUS: f32 = 50.0;
/// Clip brushes the avoidance check traces against (AI clip).
const AVOID_CLIP: u32 = 0x0002_0000;

/// A player in the crowd, as the bots avoid it.
struct Avoider {
    id: u32,
    pos: [f32; 3],
    vel: [f32; 2],
    radius: f32,
    /// A player (not a bot): it does not dodge, so a bot takes all of it.
    firm: bool,
    team: String,
}

/// Everyone in play this tick, for the bots' avoidance; empty without
/// teams (free-for-all bots do not avoid).
fn crowd(world: &mut World) -> Vec<Avoider> {
    let ids: Vec<u32> = world.resource::<Zm>().players.keys().copied().collect();
    let mut out = Vec::new();
    for id in ids {
        if !alive(world, id) {
            continue;
        }
        let Some((pos, vel)) = frame(world)
            .player(ClientId(id))
            .map(|ps| (ps.origin, [ps.velocity[0], ps.velocity[1]]))
        else {
            continue;
        };
        let bot = world.resource::<Zm>().bots.contains_key(&id);
        let radius = if bot { BOT_AVOID_RADIUS } else { PLAYER_AVOID_RADIUS };
        out.push(Avoider { id, pos, vel, radius, firm: !bot, team: String::new() });
    }
    let teams = super::with_vm(world, |vm, world| {
        teambased(vm).then(|| out.iter().map(|a| team_of(vm, world, a.id)).collect::<Vec<_>>())
    })
    .flatten();
    match teams {
        Some(teams) => {
            for (a, t) in out.iter_mut().zip(teams) {
                a.team = t;
            }
            out
        }
        None => Vec::new(),
    }
}

fn det2(a: [f32; 2], b: [f32; 2]) -> f32 {
    a[0] * b[1] - a[1] * b[0]
}

fn dot2(a: [f32; 2], b: [f32; 2]) -> f32 {
    a[0] * b[0] + a[1] * b[1]
}

/// The velocity nearest `pref` (no faster than `max_speed`) that keeps
/// `me` clear of `others` over the horizon: each neighbour gives a
/// half-plane of allowed velocities (half the dodge each between bots, all
/// of it around a player), met one at a time; when one cannot be met it
/// keeps what it had.
fn avoid_velocity(me: &Avoider, pref: [f32; 2], max_speed: f32, others: &[&Avoider]) -> [f32; 2] {
    let inv_t = 1.0 / AVOID_HORIZON;
    let inv_tick = 1000.0 / crate::MATCH_TICK_MS as f32;
    // Each half-plane as (a point on its edge, the edge's direction); the
    // allowed side is to the left.
    let mut lines: Vec<([f32; 2], [f32; 2])> = Vec::with_capacity(others.len());
    for o in others {
        let rel_pos = [o.pos[0] - me.pos[0], o.pos[1] - me.pos[1]];
        let rel_vel = [me.vel[0] - o.vel[0], me.vel[1] - o.vel[1]];
        let dist_sq = dot2(rel_pos, rel_pos);
        let r = me.radius + o.radius;
        let share = if o.firm { 1.0 } else { 0.5 };
        let (dir, u);
        // Off the edge nearest the velocity: the cut-off circle at the
        // horizon, either side cone leg, or (already touching) the circle
        // one tick ahead.
        let toward = |w: [f32; 2], k: f32| {
            let len = dot2(w, w).sqrt().max(1e-6);
            let unit = [w[0] / len, w[1] / len];
            ([unit[1], -unit[0]], [(k - len) * unit[0], (k - len) * unit[1]])
        };
        if dist_sq > r * r {
            let w = [rel_vel[0] - inv_t * rel_pos[0], rel_vel[1] - inv_t * rel_pos[1]];
            let along = dot2(w, rel_pos);
            if along < 0.0 && along * along > r * r * dot2(w, w) {
                (dir, u) = toward(w, r * inv_t);
            } else {
                let leg = (dist_sq - r * r).sqrt();
                dir = if det2(rel_pos, w) > 0.0 {
                    [(rel_pos[0] * leg - rel_pos[1] * r) / dist_sq, (rel_pos[0] * r + rel_pos[1] * leg) / dist_sq]
                } else {
                    [-(rel_pos[0] * leg + rel_pos[1] * r) / dist_sq, -(-rel_pos[0] * r + rel_pos[1] * leg) / dist_sq]
                };
                let d = dot2(rel_vel, dir);
                u = [d * dir[0] - rel_vel[0], d * dir[1] - rel_vel[1]];
            }
        } else {
            let w = [rel_vel[0] - inv_tick * rel_pos[0], rel_vel[1] - inv_tick * rel_pos[1]];
            (dir, u) = toward(w, r * inv_tick);
        }
        lines.push(([me.vel[0] + share * u[0], me.vel[1] + share * u[1]], dir));
    }
    let len = dot2(pref, pref).sqrt();
    let mut out = if len > max_speed { [pref[0] / len * max_speed, pref[1] / len * max_speed] } else { pref };
    for i in 0..lines.len() {
        let (p, d) = lines[i];
        if det2(d, [p[0] - out[0], p[1] - out[1]]) > 0.0 {
            match on_line(&lines[..=i], max_speed, pref) {
                Some(v) => out = v,
                None => break,
            }
        }
    }
    out
}

/// The point on the last line nearest `opt` inside the speed circle and the
/// earlier half-planes, if any.
fn on_line(lines: &[([f32; 2], [f32; 2])], radius: f32, opt: [f32; 2]) -> Option<[f32; 2]> {
    let (&(p, d), rest) = lines.split_last()?;
    let dp = dot2(p, d);
    let disc = dp * dp + radius * radius - dot2(p, p);
    if disc < 0.0 {
        return None;
    }
    let s = disc.sqrt();
    let (mut lo, mut hi) = (-dp - s, -dp + s);
    for &(q, e) in rest {
        let den = det2(d, e);
        let num = det2(e, [p[0] - q[0], p[1] - q[1]]);
        if den.abs() <= 1e-3 {
            if num < 0.0 {
                return None;
            }
            continue;
        }
        let t = num / den;
        if den >= 0.0 {
            hi = hi.min(t);
        } else {
            lo = lo.max(t);
        }
        if lo > hi {
            return None;
        }
    }
    let t = dot2(d, [opt[0] - p[0], opt[1] - p[1]]).max(lo).min(hi);
    Some([p[0] + t * d[0], p[1] + t * d[1]])
}

/// Body steering (any team, free-for-all too): a body closer than this
/// (centre to centre, flat) pushes a bot off it; two players' boxes touch
/// at 30.
const BODY_SEP_RADIUS: f32 = 44.0;
/// A body within this of the bot along its way, and this close sideways, is
/// in its way: it sidesteps round it rather than walking into its back.
const BODY_AHEAD_RANGE: f32 = 72.0;
const BODY_AHEAD_SIDE: f32 = 34.0;
/// Overlapping or touching: the bot counts as body-blocked.
const BODY_TOUCH: f32 = 33.0;

/// The way a bot walks (`wish`, flat) bent off the players' bodies around
/// it (`bodies`: everyone alive but itself): pushed apart from a body it
/// stands in or against, and round one standing in its way. None: nothing
/// close, walk on as wished. The tie side for a body dead ahead comes from
/// `n` (the bot's number) so two bots meeting head-on pass on opposite
/// sides.
fn body_steer(n: u32, origin: [f32; 3], wish: [f32; 2], bodies: &[(u32, [f32; 3])]) -> Option<[f32; 2]> {
    let wl = wish[0].hypot(wish[1]);
    if wl < 1e-3 {
        return None;
    }
    let w = [wish[0] / wl, wish[1] / wl];
    let mut push = [0.0f32; 2];
    let mut any = false;
    for &(id, p) in bodies {
        if id == n || (p[2] - origin[2]).abs() >= 64.0 {
            continue;
        }
        let to = [p[0] - origin[0], p[1] - origin[1]];
        let d = to[0].hypot(to[1]);
        // Apart: straight away from it, harder the deeper it is.
        if d < BODY_SEP_RADIUS {
            let away = if d > 1e-3 {
                [-to[0] / d, -to[1] / d]
            } else if n < id {
                [-w[1], w[0]]
            } else {
                [w[1], -w[0]]
            };
            let k = 2.0 * (1.0 - d / BODY_SEP_RADIUS);
            push[0] += away[0] * k;
            push[1] += away[1] * k;
            any = true;
        }
        // In the way: off to the side it is not on.
        let along = dot2(to, w);
        if along > 0.0 && along < BODY_AHEAD_RANGE {
            let side = det2(w, to);
            if side.abs() < BODY_AHEAD_SIDE {
                let left = [-w[1], w[0]];
                let s = if side.abs() < 2.0 {
                    if (n + id) % 2 == 0 { 1.0 } else { -1.0 }
                } else if side > 0.0 {
                    -1.0
                } else {
                    1.0
                };
                let k = 1.2 * (1.0 - along / BODY_AHEAD_RANGE) * (1.0 - side.abs() / BODY_AHEAD_SIDE);
                push[0] += left[0] * s * k;
                push[1] += left[1] * s * k;
                any = true;
            }
        }
    }
    if !any {
        return None;
    }
    let out = [w[0] + push[0], w[1] + push[1]];
    let ol = out[0].hypot(out[1]);
    if ol < 1e-3 {
        // Pushes cancel the wish: step sideways.
        return Some(if n % 2 == 0 { [-w[1], w[0]] } else { [w[1], -w[0]] });
    }
    Some([out[0] / ol, out[1] / ol])
}

/// The nearest other body touching (or inside) the bot, if any.
fn body_touching(n: u32, origin: [f32; 3], bodies: &[(u32, [f32; 3])]) -> Option<(u32, f32)> {
    bodies
        .iter()
        .filter(|(id, p)| *id != n && (p[2] - origin[2]).abs() < 64.0)
        .map(|(id, p)| (*id, dist2d(*p, origin)))
        .filter(|(_, d)| *d < BODY_TOUCH)
        .min_by(|a, b| a.1.total_cmp(&b.1))
}

/// BO2's check on a dodge: a thin line at knee height toward the bent
/// lookahead, failing inside a clip brush or on one within 15 units of the
/// bot (measured where the line halves).
fn avoid_clear(world: &mut World, origin: [f32; 3], to: [f32; 3]) -> bool {
    let a = [origin[0], origin[1], origin[2] + 18.0];
    let b = [to[0], to[1], to[2] + 18.0];
    let t = frame(world).trace_static_world(a, b, [0.0; 3], [0.0; 3], AVOID_CLIP);
    if t.startsolid != 0 || t.allsolid != 0 {
        return false;
    }
    if t.fraction < 1.0 {
        let h = t.fraction * 0.5;
        let m = [a[0] + (b[0] - a[0]) * h, a[1] + (b[1] - a[1]) * h];
        let d = [origin[0] - m[0], origin[1] - m[1]];
        return dot2(d, d) >= 225.0;
    }
    true
}

/// The eye of a player: his origin plus his view height.
fn eye(world: &mut World, n: u32) -> Option<[f32; 3]> {
    frame(world)
        .player(ClientId(n))
        .map(|ps| [ps.origin[0], ps.origin[1], ps.origin[2] + ps.view_height_current])
}

/// A player in play: on a team, playing, with health.
fn alive(world: &mut World, n: u32) -> bool {
    let playing = world
        .resource::<Zm>()
        .players
        .get(&n)
        .is_some_and(|p| p.sessionstate == "playing");
    playing && frame(world).player(ClientId(n)).is_some_and(|ps| ps.health > 0)
}

fn team_of(vm: &mut Vm<World>, world: &World, n: u32) -> String {
    let Some(p) = world.resource::<Zm>().players.get(&n) else {
        return String::new();
    };
    let f = vm.intern("team");
    let v = vm.raw_field(p.obj, f);
    vm.to_text(&v)
}

/// Is the way toward `to` blocked at step height (a player's box raised by
/// the walk's step, 24 units on)?
fn blocked_ahead(world: &mut World, origin: [f32; 3], to: [f32; 3]) -> bool {
    let d = [to[0] - origin[0], to[1] - origin[1]];
    let len = (d[0] * d[0] + d[1] * d[1]).sqrt();
    if len < 1.0 {
        return false;
    }
    let end = [origin[0] + d[0] / len * 24.0, origin[1] + d[1] / len * 24.0, origin[2]];
    let t = step_trace(world, origin, end);
    t.allsolid != 0 || t.fraction < 1.0
}

/// A player's box raised by the 18-unit step, from `from` to `to` (both at
/// the feet), as BO2's Path_PredictionTrace sweeps it: a box that starts
/// inside the map (a spawn set into a clip brush) is shrunk by 5 units
/// (lookaheadTraceFudge) across and on top, down to 10 across, and is then
/// judged by the sweep alone, so a bot can walk out of the brush.
fn step_trace(world: &mut World, from: [f32; 3], to: [f32; 3]) -> trace_iw4::Trace {
    use crate::bullet_collision::{MASK_PLAYER_SOLID, PLAYER_MAXS, PLAYER_MINS};
    let start = [from[0], from[1], from[2] + 18.0];
    let end = [to[0], to[1], to[2] + 18.0];
    let mut last = None;
    for (r, top) in step_boxes(PLAYER_MAXS[0], PLAYER_MAXS[2] - 18.0) {
        let t = frame(world).trace_clip(start, end, [-r, -r, PLAYER_MINS[2]], [r, r, top], MASK_PLAYER_SOLID);
        if t.startsolid == 0 {
            return t;
        }
        last = Some(t);
    }
    last.expect("step_boxes yields at least one box")
}

/// The boxes `step_trace` tries, largest first: (half width, top), each 5
/// smaller than the last, ending at the first no wider than 10.
fn step_boxes(r: f32, top: f32) -> impl Iterator<Item = (f32, f32)> {
    let mut next = Some((r, top));
    std::iter::from_fn(move || {
        let cur = next?;
        next = (cur.0 > 10.0).then(|| (cur.0 - 5.0, cur.1 - 5.0));
        Some(cur)
    })
}

/// BO2's prediction trace to the lookahead: can a player walk straight
/// from `origin` to `to` (its box raised by the 18-unit step, against the
/// map and what stands in it, as the walk itself collides)?
fn walk_clear(world: &mut World, origin: [f32; 3], to: [f32; 3]) -> bool {
    if dist2d(origin, to) < 1.0 {
        return true;
    }
    let t = step_trace(world, origin, to);
    t.allsolid == 0 && t.fraction >= 1.0
}

/// A bo2mp fix's switch: on unless its variable is "0" (read once).
fn fix_on(name: &str, cell: &'static std::sync::OnceLock<bool>) -> bool {
    *cell.get_or_init(|| std::env::var(name).map_or(true, |v| v.trim() != "0"))
}

/// BO2MP_BOT_LOOKAHEAD_GROUND=0: the walk to the lookahead is judged by
/// its walls alone (the old way), so a corner cut can run off a balcony.
fn lookahead_ground_on() -> bool {
    static C: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    fix_on("BO2MP_BOT_LOOKAHEAD_GROUND", &C)
}

/// BO2MP_BOT_LEDGE_GUARD=0: no hold at a ledge it did not plan to drop off.
fn ledge_guard_on() -> bool {
    static C: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    fix_on("BO2MP_BOT_LEDGE_GUARD", &C)
}

/// BO2MP_BOT_UNSTICK=0: the old stuck handling (no straight walk to the
/// route's next point, back off to a fixed side, route dropped at 3 s).
fn unstick_on() -> bool {
    static C: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    fix_on("BO2MP_BOT_UNSTICK", &C)
}

/// BO2MP_BOT_IDLE_LOOK=0: a bot with nowhere to head keeps facing a wall.
fn idle_look_on() -> bool {
    static C: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    fix_on("BO2MP_BOT_IDLE_LOOK", &C)
}

/// BO2MP_BOT_REFLEX=0: only its scripts aim and fire (an enemy walking
/// across in front of it while they look elsewhere goes unshot).
fn reflex_on() -> bool {
    static C: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    fix_on("BO2MP_BOT_REFLEX", &C)
}

/// Is there floor under the straight walk from `from` to `to` (feet)? A
/// point every 40 units or more (at most 10) is traced down from 36 above
/// the line to 48 below it; a miss is a drop the walk would run off.
fn ground_along(world: &mut World, from: [f32; 3], to: [f32; 3]) -> bool {
    use crate::bullet_collision::MASK_PLAYER_SOLID;
    let len = dist2d(from, to);
    if len < 48.0 {
        return true;
    }
    let step = (len / 10.0).max(40.0);
    let count = (len / step).ceil() as usize;
    for i in 1..=count {
        let s = ((i as f32) * step).min(len) / len;
        let p = [from[0] + (to[0] - from[0]) * s, from[1] + (to[1] - from[1]) * s, from[2] + (to[2] - from[2]) * s];
        let t = frame(world).trace_clip(
            [p[0], p[1], p[2] + 36.0],
            [p[0], p[1], p[2] - 48.0],
            [-6.0, -6.0, 0.0],
            [6.0, 6.0, 1.0],
            MASK_PLAYER_SOLID,
        );
        if t.startsolid == 0 && t.allsolid == 0 && t.fraction >= 1.0 {
            return false;
        }
    }
    true
}

/// The world direction (unit, flat) a stick of `forward` and `right`
/// walks with the view's yaw at `yaw` (the inverse of the stick's sums).
fn stick_world_dir(forward: i8, right: i8, yaw: f32) -> [f32; 2] {
    let (s, c) = yaw.to_radians().sin_cos();
    let (f, r) = (f32::from(forward), f32::from(right));
    let d = [f * c + r * s, f * s - r * c];
    let len = d[0].hypot(d[1]);
    if len < 1e-3 { [0.0, 0.0] } else { [d[0] / len, d[1] / len] }
}

/// A drop of more than 54 units 36 units ahead along `dir`, with no wall
/// before it (walking into a wall is no fall).
fn drop_ahead(world: &mut World, origin: [f32; 3], dir: [f32; 2]) -> bool {
    use crate::bullet_collision::MASK_PLAYER_SOLID;
    if dir == [0.0, 0.0] {
        return false;
    }
    let p = [origin[0] + dir[0] * 36.0, origin[1] + dir[1] * 36.0, origin[2]];
    let t = frame(world).trace_clip(
        [p[0], p[1], p[2] + 18.0],
        [p[0], p[1], p[2] - 54.0],
        [-6.0, -6.0, 0.0],
        [6.0, 6.0, 1.0],
        MASK_PLAYER_SOLID,
    );
    if t.startsolid != 0 || t.allsolid != 0 || t.fraction < 1.0 {
        return false;
    }
    walk_clear(world, origin, p)
}

/// The nearest enemy in sight of `from` (eye to eye or chest) within
/// `cone` degrees of `yaw` and 2500 units: (client, his chest, distance,
/// degrees off the view).
fn front_enemy(world: &mut World, n: u32, from: [f32; 3], yaw: f32, cone: f32) -> Option<(u32, [f32; 3], f32, f32)> {
    let enemies = super::with_vm(world, |vm, world| enemies_of(vm, world, n, true)).unwrap_or_default();
    let mut near = Vec::new();
    for e in enemies {
        let Some((o, h)) = frame(world).player(ClientId(e)).map(|ps| (ps.origin, ps.view_height_current)) else {
            continue;
        };
        let to = [o[0], o[1], o[2] + h];
        let d = dist3(from, to);
        if !(1.0..=2500.0).contains(&d) {
            continue;
        }
        let off = angle_delta(yaw, angles_to(from, to).0).abs();
        if off > cone {
            continue;
        }
        let chest = [o[0], o[1], (o[2] + h * 0.5 + 8.0).min(to[2])];
        near.push((d, e, to, chest, off));
    }
    near.sort_by(|a, b| a.0.total_cmp(&b.0));
    for (d, e, to, chest, off) in near.into_iter().take(3) {
        if sight(world, from, chest) || sight(world, from, to) {
            return Some((e, chest, d, off));
        }
    }
    None
}

/// With a wall within 96 units in front of its eye: the yaw (of eight
/// round it) that sees farthest, when that is past 96 units.
fn open_yaw(world: &mut World, eye: [f32; 3], yaw: f32) -> Option<f32> {
    let reach = |world: &mut World, y: f32, len: f32| {
        let (s, c) = y.to_radians().sin_cos();
        let end = [eye[0] + c * len, eye[1] + s * len, eye[2]];
        frame(world).trace_static_world(eye, end, [0.0; 3], [0.0; 3], crate::bullet_collision::MASK_SHOT).fraction * len
    };
    if reach(world, yaw, 96.0) >= 95.0 {
        return None;
    }
    let mut best = (0.0f32, yaw);
    for k in 1..8 {
        let y = yaw + k as f32 * 45.0;
        let r = reach(world, y, 1024.0);
        if r > best.0 + 1.0 {
            best = (r, y);
        }
    }
    (best.0 > 96.0).then(|| angle_delta(0.0, best.1))
}

/// For a stuck bot's step aside: the side (the stick's right, +127 or
/// -127) that is open 40 units out when the other is not.
fn open_side(world: &mut World, origin: [f32; 3], yaw: f32) -> Option<i8> {
    let (s, c) = yaw.to_radians().sin_cos();
    let mut clear = |sign: f32| walk_clear(world, origin, [origin[0] + s * 40.0 * sign, origin[1] - c * 40.0 * sign, origin[2]]);
    match (clear(1.0), clear(-1.0)) {
        (true, false) => Some(127),
        (false, true) => Some(-127),
        _ => None,
    }
}

/// How long an enemy stays in front of a bot whose scripts hold fire
/// before the engine aims and fires itself.
const REFLEX_DELAY_MS: i64 = 400;

/// The reflex (BO2MP_BOT_REFLEX): an enemy in front of it for
/// REFLEX_DELAY_MS while its scripts neither fire nor aim for it (they
/// look elsewhere, or not at all): the engine turns to his chest and fires.
/// Its scripts firing, aiming the view themselves, a failsafe or a held use
/// end it, as does losing sight of him for 300 ms. The point to aim at.
fn reflex_point(b: &mut Bot, front: Option<(u32, [f32; 3], f32, f32)>, now: i64) -> Option<[f32; 3]> {
    if !reflex_on() || b.script_aims || b.failsafe.is_some() || b.use_until > now {
        b.reflex = None;
        b.front_seen = None;
        return None;
    }
    let scripted = b.allow_attack || b.attack_ticks > 0;
    let Some((e, pt, ..)) = front else {
        b.front_seen = None;
        if b.reflex.is_some_and(|(_, last)| now - last > 300 || scripted) {
            b.reflex = None;
        }
        return None;
    };
    let since = match b.front_seen {
        Some((id, s)) if id == e => s,
        _ => {
            b.front_seen = Some((e, now));
            now
        }
    };
    if scripted {
        b.reflex = None;
        return None;
    }
    if b.reflex.is_some_and(|(id, _)| id == e) || now - since >= REFLEX_DELAY_MS {
        b.reflex = Some((e, now));
        Some(pt)
    } else {
        None
    }
}

/// Is the view (pitch, yaw) on `pt` from `eye`: within 15 units of it
/// across, 2.5 degrees at the least (pitch a little looser)?
fn reflex_aligned(view: [f32; 3], eye: [f32; 3], pt: [f32; 3]) -> bool {
    let (yaw, pitch) = angles_to(eye, pt);
    let tol = (15.0f32).atan2(dist3(eye, pt).max(1.0)).to_degrees().max(2.5);
    angle_delta(view[1], yaw).abs() < tol && angle_delta(view[0], pitch).abs() < tol.max(4.0) + 2.0
}

/// BO2MP_BOTSTATS=1 (test aid): counts of stuck time, enemies in front left
/// unshot, falls and ledge holds over the match, logged every 15 s.
fn botstats() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var("BO2MP_BOTSTATS").is_ok_and(|v| v.trim() != "0"))
}

#[derive(Clone, Copy, Debug)]
struct BotStats {
    last_now: i64,
    last_log: i64,
    /// Bot-milliseconds alive.
    bot_ms: i64,
    /// With a route and no headway for over 600 ms; episodes past 1200.
    stuck_ms: i64,
    stuck_events: u32,
    /// An enemy in sight within 30 degrees of its view and 2500 units, and
    /// of that, with its trigger idle (not reloading); episodes of 750 ms
    /// idle in a row are missed shots.
    front_ms: i64,
    nofire_ms: i64,
    missed: u32,
    /// Landed more than 64 units below where it left the ground, with no
    /// drop on its route.
    falls: u32,
    ledge_hold_ticks: u32,
    reflex_shots: u32,
}

static STATS: std::sync::Mutex<BotStats> = std::sync::Mutex::new(BotStats {
    last_now: 0,
    last_log: 0,
    bot_ms: 0,
    stuck_ms: 0,
    stuck_events: 0,
    front_ms: 0,
    nofire_ms: 0,
    missed: 0,
    falls: 0,
    ledge_hold_ticks: 0,
    reflex_shots: 0,
});

/// The time since the last drive (0 with the counters off), and the
/// counters' line every 15 s.
fn bot_stats_tick(now: i64) -> i64 {
    if !botstats() {
        return 0;
    }
    let mut s = STATS.lock().unwrap_or_else(|e| e.into_inner());
    let dt = if s.last_now > 0 { (now - s.last_now).clamp(0, 200) } else { 50 };
    s.last_now = now;
    if now - s.last_log >= 15000 {
        s.last_log = now;
        diag::info!(
            Sim,
            "bo2mp botstats t={now} bot_s={:.0} stuck_ms={} stuck_events={} front_ms={} nofire_ms={} missed={} falls={} ledge_hold_ticks={} reflex_shots={}",
            s.bot_ms as f64 / 1000.0,
            s.stuck_ms,
            s.stuck_events,
            s.front_ms,
            s.nofire_ms,
            s.missed,
            s.falls,
            s.ledge_hold_ticks,
            s.reflex_shots
        );
    }
    dt
}

/// One bot's tick for the counters.
struct StatTick {
    n: u32,
    now: i64,
    dt: i64,
    origin: [f32; 3],
    routed: bool,
    airborne: bool,
    /// An enemy in front (within 30 degrees).
    front: bool,
    fired: bool,
    reloading: bool,
    reflex_shot: bool,
    ledge_held: bool,
    drop_planned: bool,
}

fn note_stats(b: &mut Bot, t: &StatTick) {
    let mut s = STATS.lock().unwrap_or_else(|e| e.into_inner());
    s.bot_ms += t.dt;
    let stuck = if t.routed {
        b.progress
            .filter(|(at, _)| dist2d(*at, t.origin) < 12.0)
            .map_or(0, |(_, since)| t.now - since)
    } else {
        0
    };
    if stuck > 600 {
        s.stuck_ms += t.dt;
    }
    if stuck >= 1200 {
        if !b.st_stuck_counted {
            s.stuck_events += 1;
            b.st_stuck_counted = true;
        }
    } else if stuck < 600 {
        b.st_stuck_counted = false;
    }
    // Firing, or resting between a semi-automatic's shots, is engaging.
    let engaged = t.fired || b.fire_delay_until > t.now;
    if t.front && !t.reloading {
        s.front_ms += t.dt;
        if engaged {
            b.st_nofire_since = None;
            b.st_missed_counted = false;
        } else {
            s.nofire_ms += t.dt;
            let since = *b.st_nofire_since.get_or_insert(t.now);
            if t.now - since >= 750 && !b.st_missed_counted {
                s.missed += 1;
                b.st_missed_counted = true;
            }
        }
    } else {
        b.st_nofire_since = None;
        b.st_missed_counted = false;
    }
    s.reflex_shots += u32::from(t.reflex_shot);
    s.ledge_hold_ticks += u32::from(t.ledge_held);
    if t.airborne {
        if b.st_air.is_none() {
            b.st_air = Some((t.origin[2], t.drop_planned));
        }
    } else if let Some((z, planned)) = b.st_air.take()
        && z - t.origin[2] > 64.0
        && !planned
    {
        s.falls += 1;
        diag::info!(Sim, "bo2mp botstats fall bot={} from_z={z:.0} at {:.0} {:.0} {:.0}", t.n, t.origin[0], t.origin[1], t.origin[2]);
    }
}

/// BO2's nearest node to a place (Path_NearestNode, as its path search
/// starts and ends): of `near_cylinder`'s nodes, the first the place stands
/// in (the node's box: 15 each way across, 48 up) or that a body-sized box
/// (-10 -10 17 to 10 10 48) reaches in a straight line; a bot upstairs no
/// longer starts from the node on the floor below. None: our closest node.
fn nearest_reachable(world: &mut World, p: [f32; 3]) -> Option<u32> {
    use crate::bullet_collision::MASK_PLAYER_SOLID;
    let near = world.resource::<Zm>().nav.near_cylinder(p);
    for (i, o) in near {
        let inside = (p[0] - o[0]).abs() < 16.0 && (p[1] - o[1]).abs() < 16.0 && p[2] > o[2] - 1.0 && p[2] < o[2] + 49.0;
        if inside {
            return Some(i);
        }
        let t = frame(world).trace_clip(p, o, [-10.0, -10.0, 17.0], [10.0, 10.0, 48.0], MASK_PLAYER_SOLID);
        if t.startsolid == 0 && t.fraction >= 1.0 {
            return Some(i);
        }
    }
    world.resource::<Zm>().nav.nearest(p)
}

/// BO2MP_BOTLOG=1 (test aid), read once.
fn botlog() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var_os("BO2MP_BOTLOG").is_some())
}

/// BO2MP_BOTFIRELOG=1 (test aid), read once: one line per bot per tick while
/// an enemy is in sight or within 400 units, to show why a bot holds fire.
fn botfirelog() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var_os("BO2MP_BOTFIRELOG").is_some())
}

/// What a bot's think settled on for the tick, for the fire log.
struct FireLog {
    now: i64,
    /// The view yaw the command carries (after the steering).
    view_yaw: f32,
    /// How far the steering still has to turn toward what it chases.
    steer_err: f32,
    weapon: u32,
    wstate: i32,
    sprinting: bool,
    /// Its think wanted to sprint (before the trigger took it off).
    sprint_wanted: bool,
    allow: bool,
    delay_until: i64,
    looking: bool,
    /// The buttons in its command.
    press: u32,
}

/// The fire log's line (BO2MP_BOTFIRELOG): the nearest enemy that is in
/// sight or within 400 units, how far off its aim is, what the trigger and
/// sprint did, the gun's state, and whether the pull before this tick let a
/// shot out (its magazine dropped since).
fn log_botfire(world: &mut World, n: u32, l: &FireLog) {
    let clip = frame(world).combat_facts_for(l.weapon).and_then(|facts| {
        let key = weapon_iw4::clip_table_key(facts.clip_index, l.weapon);
        frame(world)
            .player(ClientId(n))
            .filter(|ps| weapon_iw4::clip_row_present(&ps.ammoclip, key))
            .map(|ps| weapon_iw4::get_clip_for_hand(&ps.ammoclip, key, 0))
    });
    let wtime = frame(world).player(ClientId(n)).map_or(0, |ps| ps.weapon_time);
    let shot = {
        let mut zm = world.resource_mut::<Zm>();
        let Some(b) = zm.bots.get_mut(&n) else { return };
        let prev = b.fire_dbg.replace((l.weapon, clip.unwrap_or(-1)));
        prev.is_some_and(|(w, c)| w == l.weapon && c >= 0 && clip.is_some_and(|now| now < c))
    };
    let Some(from) = eye(world, n) else { return };
    let enemies = super::with_vm(world, |vm, world| enemies_of(vm, world, n, true)).unwrap_or_default();
    let mut best: Option<(f32, u32, bool, [f32; 3])> = None;
    for e in enemies {
        let Some(to) = eye(world, e) else { continue };
        let d = dist3(from, to);
        if best.is_some_and(|(bd, ..)| bd <= d) {
            continue;
        }
        let seen = sight(world, from, to);
        if seen || d < 400.0 {
            best = Some((d, e, seen, to));
        }
    }
    let Some((d, e, seen, to)) = best else { return };
    let (yaw, _) = angles_to(from, to);
    diag::info!(
        Sim,
        "bo2mp botfire t={} bot={n} enemy={e} dist={d:.0} sight={} yawerr={:.1} steer={:.1} look={} allow={} attack={} sprintbtn={} sprintwant={} sprinting={} wstate={} wtime={wtime} clip={} delay_until={} wait={} shot={}",
        l.now,
        u8::from(seen),
        angle_delta(l.view_yaw, yaw).abs(),
        l.steer_err,
        u8::from(l.looking),
        u8::from(l.allow),
        u8::from(l.press & buttons::ATTACK != 0),
        u8::from(l.press & buttons::SPRINT != 0),
        u8::from(l.sprint_wanted),
        u8::from(l.sprinting),
        l.wstate,
        clip.unwrap_or(-1),
        l.delay_until,
        (l.delay_until - l.now).max(0),
        u8::from(shot),
    );
}

fn sight(world: &mut World, from: [f32; 3], to: [f32; 3]) -> bool {
    let t = frame(world).trace_static_world(
        from,
        to,
        [0.0; 3],
        [0.0; 3],
        crate::bullet_collision::MASK_SHOT,
    );
    t.fraction >= 1.0
}

/// Players on other teams (or every other player without teams), in play.
/// Is this a team game (`level.teambased`)? Free-for-all players all
/// stand on one team name and are each other's enemies.
fn teambased(vm: &mut Vm<World>) -> bool {
    let f = vm.intern("teambased");
    gsc_t6::truthy(&vm.raw_field(vm.level, f))
}

fn enemies_of(vm: &mut Vm<World>, world: &mut World, n: u32, alive_only: bool) -> Vec<u32> {
    let mine = team_of(vm, world, n);
    let teambased = teambased(vm) && mine != "free" && !mine.is_empty();
    let ids: Vec<u32> = world.resource::<Zm>().players.keys().copied().collect();
    let mut out = Vec::new();
    for id in ids {
        if id == n {
            continue;
        }
        let theirs = team_of(vm, world, id);
        if theirs == "spectator" || theirs.is_empty() {
            continue;
        }
        if teambased && theirs == mine {
            continue;
        }
        if alive_only && !alive(world, id) {
            continue;
        }
        out.push(id);
    }
    out
}

fn friends_of(vm: &mut Vm<World>, world: &mut World, n: u32, alive_only: bool) -> Vec<u32> {
    if !teambased(vm) {
        return Vec::new();
    }
    let mine = team_of(vm, world, n);
    let ids: Vec<u32> = world.resource::<Zm>().players.keys().copied().collect();
    let mut out = Vec::new();
    for id in ids {
        if id == n || team_of(vm, world, id) != mine {
            continue;
        }
        if alive_only && !alive(world, id) {
            continue;
        }
        out.push(id);
    }
    out
}

fn objs(world: &World, ids: &[u32]) -> Vec<Value> {
    let zm = world.resource::<Zm>();
    ids.iter()
        .filter_map(|id| zm.players.get(id).map(|p| Value::Object(p.obj)))
        .collect()
}

/// A target's place: a point, an entity, a path node (struct origin).
fn place(vm: &mut Vm<World>, world: &mut World, v: &Value) -> Option<[f32; 3]> {
    if let Some(p) = v.as_vec3() {
        return Some(p);
    }
    if let Some(p) = origin_of(vm, world, v) {
        return Some(p);
    }
    if let Value::Object(o) = v {
        let f = vm.intern("origin");
        return vm.raw_field(*o, f).as_vec3();
    }
    None
}

fn bot_mut<'w>(world: &'w mut World, n: u32) -> Option<bevy_ecs::prelude::Mut<'w, Zm>> {
    let zm = world.resource_mut::<Zm>();
    zm.bots.contains_key(&n).then_some(zm)
}

/// The free client slots: the local player is 0; bots take the next free.
fn free_client(vm: &Vm<World>, world: &mut World) -> Option<u32> {
    let max = vm
        .dvars
        .get("sv_maxclients")
        .and_then(|s| s.parse::<u32>().ok())
        .unwrap_or(18);
    let mut used: Vec<u32> = frame(world)
        .client_ids_sorted()
        .into_iter()
        .map(|c| c.0)
        .collect();
    let zm = world.resource::<Zm>();
    used.extend(zm.players.keys().copied());
    used.extend(zm.test_client_requests.iter().copied());
    (1..max).find(|i| !used.contains(i))
}

pub(super) fn bind(vm: &mut Vm<World>) {
    macro_rules! f {
        ($name:literal, $body:expr) => {
            vm.bind($name, false, $body);
        };
    }
    macro_rules! m {
        ($name:literal, $body:expr) => {
            vm.bind($name, true, $body);
        };
    }
    // A bot joins: its player entity at once (the script marks it a bot and
    // starts its spawn thread before it connects), its client next frames.
    f!("addtestclient", |vm, world, _, _| {
        let Some(n) = free_client(vm, world) else {
            return Ok(Value::Undefined);
        };
        let obj = vm.alloc_object(ObjKind::Entity(n));
        let pers = vm.intern("pers");
        vm.set_raw_field(obj, pers, Value::array(Array::new()));
        let mut zm = world.resource_mut::<Zm>();
        let mut player = Player::new(obj);
        player.test_client = true;
        player.pending_since = Some(zm.now_ms);
        zm.players.insert(n, player);
        zm.bots.insert(n, Bot::default());
        zm.test_client_requests.push(n);
        diag::info!(Sim, "bo2mp bot: test client {n} joins");
        Ok(Value::Object(obj))
    });
    m!("istestclient", |vm, world, s, _| {
        let n = entnum(vm, s).unwrap_or(u32::MAX);
        Ok(Value::bool(
            world
                .resource::<Zm>()
                .players
                .get(&n)
                .is_some_and(|p| p.test_client),
        ))
    });
    m!("botleavegame", |vm, world, s, _| {
        if let Some(n) = entnum(vm, s) {
            world.resource_mut::<Zm>().bot_leaves.push(n);
        }
        Ok(Value::Undefined)
    });
    // Goals.
    m!("addgoal", |vm, world, s, a| {
        let n = entnum(vm, s).unwrap_or(u32::MAX);
        let Some(pos) = place(vm, world, arg(a, 0)) else {
            return Ok(Value::Undefined);
        };
        let radius = arg(a, 1).as_float().unwrap_or(24.0);
        let priority = arg(a, 2).as_int().unwrap_or(1);
        let name = text(vm, a, 3);
        if let Some(mut zm) = bot_mut(world, n) {
            let b = zm.bots.get_mut(&n).unwrap();
            b.order += 1;
            let order = b.order;
            b.goals.retain(|g| g.name != name);
            b.goals.push(Goal {
                name,
                pos,
                radius,
                priority,
                order,
            });
        }
        Ok(Value::Undefined)
    });
    m!("cancelgoal", |vm, world, s, a| {
        let n = entnum(vm, s).unwrap_or(u32::MAX);
        let name = text(vm, a, 0);
        if let Some(mut zm) = bot_mut(world, n) {
            zm.bots.get_mut(&n).unwrap().goals.retain(|g| g.name != name);
        }
        Ok(Value::Undefined)
    });
    m!("hasgoal", |vm, world, s, a| {
        let n = entnum(vm, s).unwrap_or(u32::MAX);
        let name = text(vm, a, 0);
        Ok(Value::bool(world.resource::<Zm>().bots.get(&n).is_some_and(|b| {
            b.goals.iter().any(|g| g.name == name)
        })))
    });
    m!("getgoal", |vm, world, s, a| {
        let n = entnum(vm, s).unwrap_or(u32::MAX);
        let name = text(vm, a, 0);
        Ok(world
            .resource::<Zm>()
            .bots
            .get(&n)
            .and_then(|b| b.goals.iter().find(|g| g.name == name))
            .map_or(Value::Undefined, |g| Value::Vec3(g.pos)))
    });
    // At a goal (named, else the one it walks to): within its radius. No
    // goal at all: where it wants to be.
    m!("atgoal", |vm, world, s, a| {
        let n = entnum(vm, s).unwrap_or(u32::MAX);
        let name = arg(a, 0);
        let name = if vm.is_defined(name) {
            Some(vm.to_text(name))
        } else {
            None
        };
        let origin = frame(world).player(ClientId(n)).map(|ps| ps.origin);
        let zm = world.resource::<Zm>();
        let Some(b) = zm.bots.get(&n) else {
            return Ok(Value::Int(0));
        };
        let goal = match &name {
            Some(name) => b.goals.iter().find(|g| &g.name == name),
            None => b.best_goal(),
        };
        let at = match (goal, origin) {
            (Some(g), Some(o)) => dist2d(o, g.pos) <= g.radius.max(16.0) && (o[2] - g.pos[2]).abs() < 72.0,
            (None, _) => name.is_none(),
            _ => false,
        };
        Ok(Value::bool(at))
    });
    // BO2's lookahead: the direction (z the rise per unit) and distance to
    // the point its route walk heads for; undefined and 0 with no route or
    // at its goal (its scripts then pick somewhere to look).
    m!("getlookaheaddir", |vm, world, s, _| {
        let n = entnum(vm, s).unwrap_or(u32::MAX);
        let la = world.resource::<Zm>().bots.get(&n).and_then(|b| b.la_dir);
        Ok(la.map_or(Value::Undefined, |(d, _)| Value::Vec3(d)))
    });
    m!("getlookaheaddist", |vm, world, s, _| {
        let n = entnum(vm, s).unwrap_or(u32::MAX);
        let la = world.resource::<Zm>().bots.get(&n).and_then(|b| b.la_dir);
        Ok(Value::Float(la.map_or(0.0, |(_, d)| d)))
    });
    m!("botsetfailsafenode", |vm, world, s, a| {
        let n = entnum(vm, s).unwrap_or(u32::MAX);
        let node = match arg(a, 0) {
            Value::Object(o) => Some(*o),
            _ => None,
        };
        let p = place(vm, world, arg(a, 0));
        if let Some(mut zm) = bot_mut(world, n) {
            zm.bots.get_mut(&n).unwrap().failsafe = p.map(|p| (p, node));
        }
        Ok(Value::Undefined)
    });
    // Looking and buttons.
    m!("lookat", |vm, world, s, a| {
        let n = entnum(vm, s).unwrap_or(u32::MAX);
        let p = place(vm, world, arg(a, 0));
        if let Some(mut zm) = bot_mut(world, n) {
            zm.bots.get_mut(&n).unwrap().look = p;
        }
        Ok(Value::Undefined)
    });
    m!("clearlookat", |vm, world, s, _| {
        let n = entnum(vm, s).unwrap_or(u32::MAX);
        if let Some(mut zm) = bot_mut(world, n) {
            zm.bots.get_mut(&n).unwrap().look = None;
        }
        Ok(Value::Undefined)
    });
    m!("allowattack", |vm, world, s, a| {
        let n = entnum(vm, s).unwrap_or(u32::MAX);
        let on = gsc_t6::truthy(arg(a, 0));
        if let Some(mut zm) = bot_mut(world, n) {
            zm.bots.get_mut(&n).unwrap().allow_attack = on;
        }
        Ok(Value::Undefined)
    });
    m!("pressads", |vm, world, s, a| {
        let n = entnum(vm, s).unwrap_or(u32::MAX);
        let on = gsc_t6::truthy(arg(a, 0));
        if let Some(mut zm) = bot_mut(world, n) {
            zm.bots.get_mut(&n).unwrap().ads = on;
        }
        Ok(Value::Undefined)
    });
    // One pull of the trigger (an argument: held that many ticks).
    m!("pressattackbutton", |vm, world, s, a| {
        let n = entnum(vm, s).unwrap_or(u32::MAX);
        let ticks = arg(a, 0).as_int().unwrap_or(1).clamp(1, 40) as u32;
        if let Some(mut zm) = bot_mut(world, n) {
            zm.bots.get_mut(&n).unwrap().attack_ticks = ticks;
        }
        Ok(Value::Undefined)
    });
    // Use held for that long (reloads when nothing is there to use).
    m!("pressusebutton", |vm, world, s, a| {
        let n = entnum(vm, s).unwrap_or(u32::MAX);
        let secs = arg(a, 0).as_float().unwrap_or(0.1);
        if let Some(mut zm) = bot_mut(world, n) {
            let until = zm.now_ms + (secs * 1000.0) as i64;
            zm.bots.get_mut(&n).unwrap().use_until = until;
        }
        Ok(Value::Undefined)
    });
    m!("pressmelee", |vm, world, s, _| {
        let n = entnum(vm, s).unwrap_or(u32::MAX);
        if let Some(mut zm) = bot_mut(world, n) {
            zm.bots.get_mut(&n).unwrap().melee = true;
        }
        Ok(Value::Undefined)
    });
    m!("pressdtpbutton", |vm, world, s, _| {
        let n = entnum(vm, s).unwrap_or(u32::MAX);
        if let Some(mut zm) = bot_mut(world, n) {
            zm.bots.get_mut(&n).unwrap().dive = true;
        }
        Ok(Value::Undefined)
    });
    m!("setstance", |vm, world, s, a| {
        let n = entnum(vm, s).unwrap_or(u32::MAX);
        let stance = match text(vm, a, 0).as_str() {
            "crouch" => 1,
            "prone" => 2,
            _ => 0,
        };
        if let Some(mut zm) = bot_mut(world, n) {
            zm.bots.get_mut(&n).unwrap().stance = stance;
        }
        Ok(Value::Undefined)
    });
    m!("allowsprint", |vm, world, s, a| {
        let n = entnum(vm, s).unwrap_or(u32::MAX);
        let on = gsc_t6::truthy(arg(a, 0));
        if let Some(mut zm) = bot_mut(world, n) {
            zm.bots.get_mut(&n).unwrap().no_sprint = !on;
        }
        // bo2mp: the same switch every player has (the server drops sprint).
        let id = crate::world::ClientId(n);
        let mut f = super::frame(world);
        if f.client_meta(id).is_some() {
            f.client_meta_mut(id).controls.sprint_disabled = !on;
        }
        Ok(Value::Undefined)
    });
    // Bot Warfare's overrides. `+melee` holds a button down until `~melee`.
    m!("botbuttonoverride", |vm, world, s, a| {
        let n = entnum(vm, s).unwrap_or(u32::MAX);
        let cmd = text(vm, a, 0);
        let (on, name) = match cmd.split_at_checked(1) {
            Some(("+", rest)) => (true, rest),
            Some(("~" | "-", rest)) => (false, rest),
            _ => return Ok(Value::Undefined),
        };
        let Some(bits) = super::trickshot::button_bits(name) else { return Ok(Value::Undefined) };
        if let Some(mut zm) = bot_mut(world, n) {
            let b = zm.bots.get_mut(&n).unwrap();
            b.held = if on { b.held | bits } else { b.held & !bits };
        }
        Ok(Value::Undefined)
    });
    // Its stick (forward, right), or `false` to give it back.
    m!("botmovementoverride", |vm, world, s, a| {
        let n = entnum(vm, s).unwrap_or(u32::MAX);
        let axis = |v: &Value| match v {
            Value::Int(i) => Some((*i).clamp(-127, 127) as i8),
            Value::Float(f) => Some(f.clamp(-127.0, 127.0) as i8),
            _ => None,
        };
        let stick = if a.len() >= 2 { axis(arg(a, 0)).zip(axis(arg(a, 1))) } else { None };
        if let Some(mut zm) = bot_mut(world, n) {
            zm.bots.get_mut(&n).unwrap().stick = stick;
        }
        Ok(Value::Undefined)
    });
    m!("botaimoverride", |vm, world, s, a| {
        let n = entnum(vm, s).unwrap_or(u32::MAX);
        let on = gsc_t6::truthy(arg(a, 0));
        if let Some(mut zm) = bot_mut(world, n) {
            zm.bots.get_mut(&n).unwrap().script_aims = on;
        }
        Ok(Value::Undefined)
    });
    m!("botclearoverride", |vm, world, s, _| {
        let n = entnum(vm, s).unwrap_or(u32::MAX);
        if let Some(mut zm) = bot_mut(world, n) {
            let b = zm.bots.get_mut(&n).unwrap();
            (b.held, b.stick, b.script_aims) = (0, None, false);
            (b.weapon_override, b.melee_target) = (None, None);
        }
        Ok(Value::Undefined)
    });
    // The weapon its command holds, so nothing but a script's switch takes
    // it off it (`none`, `false` or no weapon: given back).
    m!("botweaponoverride", |vm, world, s, a| {
        let n = entnum(vm, s).unwrap_or(u32::MAX);
        let name = text(vm, a, 0);
        let w = (!name.is_empty() && name != "none")
            .then(|| super::weapon(world, &name).ok())
            .flatten()
            .filter(|&w| w != 0);
        if let Some(mut zm) = bot_mut(world, n) {
            zm.bots.get_mut(&n).unwrap().weapon_override = w;
        }
        Ok(Value::Undefined)
    });
    // The entity its knife lunges at and the distance (`false`: its own
    // aim again).
    m!("botmeleeparamsoverride", |vm, world, s, a| {
        let n = entnum(vm, s).unwrap_or(u32::MAX);
        let num = |v: &Value| match v {
            Value::Int(i) => Some(*i as f32),
            Value::Float(f) => Some(*f),
            _ => None,
        };
        let target = match (arg(a, 0), a.len()) {
            (Value::Int(e), 2..) if *e >= 0 => num(arg(a, 1)).map(|d| (*e as u32, d)),
            _ => None,
        };
        if let Some(mut zm) = bot_mut(world, n) {
            zm.bots.get_mut(&n).unwrap().melee_target = target;
        }
        Ok(Value::Undefined)
    });
    // A grenade at a point: look there and throw.
    m!("throwgrenade", |vm, world, s, a| {
        let n = entnum(vm, s).unwrap_or(u32::MAX);
        let p = place(vm, world, arg(a, 1));
        if let Some(mut zm) = bot_mut(world, n) {
            let b = zm.bots.get_mut(&n).unwrap();
            if p.is_some() {
                b.look = p;
            }
            b.tick = 0;
            b.attack_ticks = 0;
        }
        world.resource_mut::<Zm>().bot_throws.push(n);
        Ok(Value::Undefined)
    });
    // Seeing.
    m!("botsighttracepassed", |vm, world, s, a| {
        let n = entnum(vm, s).unwrap_or(u32::MAX);
        let Some(from) = eye(world, n) else {
            return Ok(Value::Int(0));
        };
        let target = arg(a, 0);
        let to = match entnum(vm, target) {
            Some(t) if world.resource::<Zm>().players.contains_key(&t) => eye(world, t),
            _ => place(vm, world, target).map(|p| [p[0], p[1], p[2] + 32.0]),
        };
        Ok(Value::bool(to.is_some_and(|to| sight(world, from, to))))
    });
    // Enemies it can see, nearest first, inside its field of view: the
    // argument is the smallest cosine between its view and the enemy (its
    // scripts' `bot.fov`: 0.0872 = 85 degrees each side on normal; -1 =
    // all round).
    m!("getthreats", |vm, world, s, a| {
        let n = entnum(vm, s).unwrap_or(u32::MAX);
        let fov = arg(a, 0).as_float().unwrap_or(-1.0);
        let Some(from) = eye(world, n) else {
            return Ok(list(Vec::new()));
        };
        let view = frame(world).player(ClientId(n)).map_or([0.0; 3], |ps| ps.viewangles);
        let mut seen: Vec<(f32, u32)> = Vec::new();
        for e in enemies_of(vm, world, n, true) {
            let Some(to) = eye(world, e) else { continue };
            if fov > -1.0 {
                let (yaw, pitch) = (view[1].to_radians(), view[0].to_radians());
                let forward = [pitch.cos() * yaw.cos(), pitch.cos() * yaw.sin(), -pitch.sin()];
                let d = [to[0] - from[0], to[1] - from[1], to[2] - from[2]];
                let len = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt().max(1e-3);
                let dot = (forward[0] * d[0] + forward[1] * d[1] + forward[2] * d[2]) / len;
                if dot < fov {
                    continue;
                }
            }
            if sight(world, from, to) {
                seen.push((dist2d(from, to), e));
            }
        }
        seen.sort_by(|x, y| x.0.partial_cmp(&y.0).unwrap_or(std::cmp::Ordering::Equal));
        let ids: Vec<u32> = seen.into_iter().map(|(_, e)| e).collect();
        Ok(list(objs(world, &ids)))
    });
    m!("getenemies", |vm, world, s, a| {
        let n = entnum(vm, s).unwrap_or(u32::MAX);
        let alive_only = gsc_t6::truthy(arg(a, 0));
        let ids = enemies_of(vm, world, n, alive_only);
        Ok(list(objs(world, &ids)))
    });
    m!("getfriendlies", |vm, world, s, a| {
        let n = entnum(vm, s).unwrap_or(u32::MAX);
        let alive_only = gsc_t6::truthy(arg(a, 0));
        let ids = friends_of(vm, world, n, alive_only);
        Ok(list(objs(world, &ids)))
    });
    // Where a player will be in that many frames at his speed.
    m!("predictposition", |vm, world, _, a| {
        let Some(t) = entnum(vm, arg(a, 0)) else {
            return Ok(Value::Undefined);
        };
        let frames = arg(a, 1).as_float().unwrap_or(1.0);
        Ok(frame(world)
            .player(ClientId(t))
            .map_or(Value::Undefined, |ps| {
                Value::Vec3(std::array::from_fn(|i| {
                    ps.origin[i] + ps.velocity[i] * frames * 0.05
                }))
            }))
    });
    // A player's eye height over his feet (where bots aim: torso = feet +
    // height / 1.6).
    m!("getplayerviewheight", |vm, world, s, _| {
        let n = entnum(vm, s).unwrap_or(u32::MAX);
        Ok(Value::Float(
            frame(world)
                .player(ClientId(n))
                .map_or(60.0, |ps| ps.view_height_current),
        ))
    });
    // An entity's middle: a player's chest, else 32 over its origin.
    m!("getcentroid", |vm, world, s, _| {
        let n = entnum(vm, s).unwrap_or(u32::MAX);
        if let Some(ps) = frame(world).player(ClientId(n)) {
            let o = ps.origin;
            return Ok(Value::Vec3([o[0], o[1], o[2] + ps.view_height_current * 0.6]));
        }
        Ok(origin_of(vm, world, s).map_or(Value::Undefined, |o| Value::Vec3([o[0], o[1], o[2] + 32.0])))
    });
    f!("weaponisdualwield", |vm, _, _, a| {
        let w = text(vm, a, 0);
        Ok(Value::bool(w.contains("+dw") || w.ends_with("dw_mp")))
    });
    // A scoped weapon (BO2: it has an overlay reticle): its bots hold
    // full sights a second before the shot.
    f!("isweaponscopeoverlay", |vm, world, _, a| {
        let w = super::weapon(world, &text(vm, a, 0)).ok();
        let facts = w.and_then(|w| frame(world).combat_facts_for(w));
        Ok(Value::bool(facts.is_some_and(|f| f.overlay_reticle != 0)))
    });
    f!("target_istarget", |_, _, _, _| Ok(Value::Int(0)));
    m!("getplayercamerapos", |vm, world, s, _| {
        let n = entnum(vm, s).unwrap_or(u32::MAX);
        Ok(eye(world, n).map_or(Value::Undefined, Value::Vec3))
    });
    // A bot's own classes (its scripts build five from his item table).
    m!("botsetdefaultclass", |vm, world, s, a| {
        let n = entnum(vm, s).unwrap_or(u32::MAX);
        let class = arg(a, 0).as_int().unwrap_or(0);
        let name = text(vm, a, 1);
        if let Some(mut zm) = bot_mut(world, n) {
            zm.bots.get_mut(&n).unwrap().classes.entry(class).or_default().default = Some(name);
        }
        Ok(Value::Undefined)
    });
    m!("botclassadditem", |vm, world, s, a| {
        let n = entnum(vm, s).unwrap_or(u32::MAX);
        let class = arg(a, 0).as_int().unwrap_or(0);
        let item = text(vm, a, 1);
        // bo2mp: now and then one of the mod's guns instead (its scripts
        // list only Black Ops II's items).
        let seed = n.wrapping_mul(31).wrapping_add(class as u32 * 7).wrapping_add(item.bytes().map(u32::from).sum::<u32>());
        let item = super::loadout::bot_item_swap(world, &item, seed).unwrap_or(item);
        if let Some(mut zm) = bot_mut(world, n) {
            zm.bots.get_mut(&n).unwrap().classes.entry(class).or_default().items.push(item);
        }
        Ok(Value::Undefined)
    });
    m!("botclassaddattachment", |vm, world, s, a| {
        let n = entnum(vm, s).unwrap_or(u32::MAX);
        let class = arg(a, 0).as_int().unwrap_or(0);
        let (weapon, attachment, slot) = (text(vm, a, 1), text(vm, a, 2), text(vm, a, 3));
        if let Some(mut zm) = bot_mut(world, n) {
            zm.bots
                .get_mut(&n)
                .unwrap()
                .classes
                .entry(class)
                .or_default()
                .attachments
                .push((weapon, attachment, slot));
        }
        Ok(Value::Undefined)
    });
    m!("botclasssetweaponoption", |_, _, _, _| Ok(Value::Undefined));
    // The player the bots count teams around: the local player (client 0).
    f!("gethostplayerforbots", |_, world, _, _| Ok(world
        .resource::<Zm>()
        .players
        .get(&0)
        .map_or(Value::Undefined, |p| Value::Object(p.obj))));
    m!("ishostforbots", |vm, _, s, _| Ok(Value::bool(entnum(vm, s) == Some(0))));
    // The map's path nodes as the bot scripts ask about them.
    fn node_obj(world: &World, i: u32) -> Value {
        world
            .resource::<Zm>()
            .node_objs
            .get(i as usize)
            .map_or(Value::Undefined, |o| Value::Object(*o))
    }
    fn node_index(vm: &mut Vm<World>, world: &World, v: &Value) -> Option<u32> {
        let Value::Object(o) = v else { return None };
        world
            .resource::<Zm>()
            .node_of
            .get(o)
            .copied()
            .or_else(|| {
                let f = vm.intern("origin");
                let p = vm.raw_field(*o, f).as_vec3()?;
                world.resource::<Zm>().nav.nearest(p)
            })
    }
    fn node_eye(world: &World, i: u32) -> [f32; 3] {
        let o = world.resource::<Zm>().nav.nodes[i as usize].origin;
        [o[0], o[1], o[2] + 32.0]
    }
    f!("getnearestnode", |vm, world, _, a| {
        let Some(p) = place(vm, world, arg(a, 0)) else {
            return Ok(Value::Undefined);
        };
        let i = world.resource::<Zm>().nav.nearest(p);
        Ok(i.map_or(Value::Undefined, |i| node_obj(world, i)))
    });
    // A node near `from` that can see `to` (nearest first, within 512).
    f!("getvisiblenode", |vm, world, _, a| {
        let (Some(from), Some(to)) = (place(vm, world, arg(a, 0)), place(vm, world, arg(a, 1)))
        else {
            return Ok(Value::Undefined);
        };
        let near = world.resource::<Zm>().nav.within(from, 512.0);
        for (i, _) in near.into_iter().take(12) {
            let e = node_eye(world, i);
            if sight(world, e, [to[0], to[1], to[2] + 32.0]) {
                return Ok(node_obj(world, i));
            }
        }
        Ok(Value::Undefined)
    });
    // The nodes a node sees: its linked neighbours that it can see.
    f!("getvisiblenodes", |vm, world, _, a| {
        let Some(i) = node_index(vm, world, arg(a, 0)) else {
            return Ok(list(Vec::new()));
        };
        let links: Vec<u32> = world.resource::<Zm>().nav.nodes[i as usize]
            .links
            .iter()
            .map(|l| u32::from(l.0))
            .collect();
        let from = node_eye(world, i);
        let mut out = Vec::new();
        for j in links {
            if sight(world, from, node_eye(world, j)) {
                out.push(node_obj(world, j));
            }
        }
        Ok(list(out))
    });
    // BO2 reads the map compiler's sight table (`Path_NodesVisible`); a
    // node past it (or a map without one) is traced.
    f!("nodesvisible", |vm, world, _, a| {
        let (Some(i), Some(j)) = (node_index(vm, world, arg(a, 0)), node_index(vm, world, arg(a, 1)))
        else {
            return Ok(Value::Int(0));
        };
        if let Some(seen) = world.resource::<Zm>().nav.baked.visible(i, j) {
            return Ok(Value::bool(seen));
        }
        let (x, y) = (node_eye(world, i), node_eye(world, j));
        Ok(Value::bool(sight(world, x, y)))
    });
    // BO2's `Scr_NodesCanPath`: a straight walk joins them (the map
    // compiler's smooth table) or the first links to the second; not a
    // search for a route.
    f!("nodescanpath", |vm, world, _, a| {
        let (Some(i), Some(j)) = (node_index(vm, world, arg(a, 0)), node_index(vm, world, arg(a, 1)))
        else {
            return Ok(Value::Int(0));
        };
        let nav = &world.resource::<Zm>().nav;
        let linked = nav.nodes[i as usize].links.iter().any(|l| u32::from(l.0) == j);
        Ok(Value::bool(nav.baked.smooth(i, j) || linked))
    });
    // A route between two points: its nodes in order (undefined: none).
    f!("findpath", |vm, world, _, a| {
        let (Some(from), Some(to)) = (place(vm, world, arg(a, 0)), place(vm, world, arg(a, 1)))
        else {
            return Ok(Value::Undefined);
        };
        let route = match (nearest_reachable(world, from), nearest_reachable(world, to)) {
            (Some(x), Some(y)) => world.resource::<Zm>().nav.path(x, y),
            _ => None,
        };
        Ok(route.map_or(Value::Undefined, |r| {
            list(r.into_iter().map(|i| node_obj(world, i)).collect())
        }))
    });
    f!("canclaimnode", |_, _, _, _| Ok(Value::Int(1)));
    // BO2's danger marks: a count per team on a node (0..15), raised and
    // lowered by `setdangerous(team, on)`, dangerous while above 0 (bots
    // keep clear of a killstreak's landing spot).
    m!("setdangerous", |vm, world, s, a| {
        let (Some(i), Some(t)) = (node_index(vm, world, s), danger_slot(&text(vm, a, 0))) else {
            return Ok(Value::Undefined);
        };
        let on = arg(a, 1).as_int().unwrap_or(0) != 0;
        let mut zm = world.resource_mut::<Zm>();
        let c = &mut zm.node_danger.entry(i).or_default()[t];
        *c = if on { (*c + 1).min(15) } else { c.saturating_sub(1) };
        Ok(Value::Undefined)
    });
    m!("isdangerous", |vm, world, s, a| {
        let (Some(i), Some(t)) = (node_index(vm, world, s), danger_slot(&text(vm, a, 0))) else {
            return Ok(Value::Int(0));
        };
        Ok(Value::bool(world.resource::<Zm>().node_danger.get(&i).is_some_and(|c| c[t] > 0)))
    });
    f!("claimnode", |_, _, _, _| Ok(Value::Undefined));
    f!("releaseclaimednode", |_, _, _, _| Ok(Value::Undefined));
    // Bots have every item (their scripts ask before building a class).
    m!("isitemlocked", |_, _, _, _| Ok(Value::Int(0)));
    // What a player is doing, from his state.
    m!("isreloading", |vm, world, s, _| {
        let n = entnum(vm, s).unwrap_or(u32::MAX);
        Ok(Value::bool(frame(world).player(ClientId(n)).is_some_and(|ps| {
            reloading(ps.weaponstate_primary)
        })))
    });
    // As BO2 reads them: switching = the weapon raising or dropping,
    // throwing = an offhand (grenade) state, mantling and on a ladder =
    // the move flags.
    m!("isswitchingweapons", |vm, world, s, _| {
        use weapon_iw4::WeaponState as S;
        Ok(player_is(vm, world, s, |ps| {
            (S::Raising as i32..=S::DroppingAltswitch as i32).contains(&ps.weaponstate_primary)
        }))
    });
    m!("isthrowinggrenade", |vm, world, s, _| {
        use weapon_iw4::WeaponState as S;
        Ok(player_is(vm, world, s, |ps| {
            (S::OffhandInit as i32..=S::OffhandEnd as i32).contains(&ps.weaponstate_primary)
        }))
    });
    m!("ismantling", |vm, world, s, _| {
        Ok(player_is(vm, world, s, |ps| ps.pm_flags & playerstate_iw4::pm_flags::MANTLE != 0))
    });
    m!("isonladder", |vm, world, s, _| {
        Ok(player_is(vm, world, s, |ps| ps.pm_flags & playerstate_iw4::pm_flags::LADDER != 0))
    });
    for name in ["isfiring", "isremotecontrolling", "iscarryingturret"] {
        vm.bind(name, true, |_, _, _, _| Ok(Value::Int(0)));
    }
}

/// Dead, its steering starts over: BO2's G_BotThink zeroes the whole bot
/// state each think while its health is under 1 (the failsafe node with
/// it). What its scripts set up (goals, classes, overrides) stays.
fn clear_for_new_life(b: &mut Bot) {
    // Its route goes: a new life plans from where it spawns (one kept from
    // the last life is trimmed as walked past and aims it at the far goal
    // through walls).
    b.path.clear();
    b.path_to = None;
    b.replan_at = 0;
    b.prev = None;
    b.negs.clear();
    b.neg = None;
    b.la_amount = 0.0;
    b.la_pos = None;
    b.la_dir = None;
    b.la_clear = false;
    b.seen = None;
    b.progress = None;
    b.stuck_next = None;
    // A failsafe kept from the last life walks it at that life's node with
    // the view held and no buttons (frozen into a wall after a respawn).
    b.failsafe = None;
    b.idle_look = None;
    b.front_seen = None;
    b.reflex = None;
    b.st_stuck_counted = false;
    b.st_nofire_since = None;
    b.st_missed_counted = false;
    b.st_air = None;
    if b.aim_avg.is_some() {
        b.aim_avg = None;
        b.turn_speed = [0.0; 2];
        b.sprint_since = None;
        b.fire_delay_until = 0;
    }
}

/// A stuck (begun at `since`) short of route point `pt`: true once it is a
/// second stuck short of the same point, else it remembers this one.
fn stuck_again(stuck_next: &mut Option<([f32; 3], i64)>, pt: [f32; 3], since: i64) -> bool {
    match *stuck_next {
        Some((was, began)) if began != since && dist2d(was, pt) < 1.0 && (was[2] - pt[2]).abs() < 1.0 => true,
        Some((_, began)) if began == since => false,
        _ => {
            *stuck_next = Some((pt, since));
            false
        }
    }
}

/// The stick that walks a bot from its eye toward a point (a failsafe
/// node, a link's end): the direction in the view's yaw axes only (BO2's
/// Bot_UpdateFailsafe and link moves), so a view pitched up or down does
/// not slow or turn the walk; `scale` is full stick.
/// The buttons for a climb with no room to stand: crouch held, prone let go.
fn low_climb_press(press: u32) -> u32 {
    (press & !buttons::PRONE) | buttons::CROUCH
}

fn stick_toward(eye: [f32; 3], to: [f32; 3], yaw: f32, scale: f32) -> (i8, i8) {
    let e = [to[0] - eye[0], to[1] - eye[1], to[2] - eye[2]];
    let len = (e[0] * e[0] + e[1] * e[1] + e[2] * e[2]).sqrt();
    let e = if len > 0.0 { [e[0] / len, e[1] / len] } else { [e[0], e[1]] };
    let (sy, cy) = yaw.to_radians().sin_cos();
    let ahead = (e[0] * cy + e[1] * sy).clamp(-1.0, 1.0);
    let left = (e[1] * cy - e[0] * sy).clamp(-1.0, 1.0);
    ((ahead * scale) as i8, (left * -scale) as i8)
}

/// Each tick: the bots' commands (route, view, buttons) replace their
/// clients' own; a bot whose client never came is dropped.
pub(super) fn drive(world: &mut World, now: i64) {
    let ids: Vec<u32> = world.resource::<Zm>().bots.keys().copied().collect();
    if ids.is_empty() {
        return;
    }
    log_negotiations(world);
    let dv = BotDvars::read(world);
    let crowd = crowd(world);
    // Every live body (bots, players, either team) for body steering.
    let bodies: Vec<(u32, [f32; 3])> = {
        let ids: Vec<u32> = world.resource::<Zm>().players.keys().copied().collect();
        let mut out = Vec::with_capacity(ids.len());
        for id in ids {
            if !alive(world, id) {
                continue;
            }
            if let Some(o) = frame(world).player(ClientId(id)).map(|ps| ps.origin) {
                out.push((id, o));
            }
        }
        out
    };
    let throws = std::mem::take(&mut world.resource_mut::<Zm>().bot_throws);
    let dt = bot_stats_tick(now);
    for n in ids {
        if !alive(world, n) {
            // Dead, its steering starts over (BO2 clears its bot state).
            if let Some(b) = world.resource_mut::<Zm>().bots.get_mut(&n) {
                clear_for_new_life(b);
            }
            continue;
        }
        let Some((origin, view, height, ladder, mantling, low_climb, sprinting, ads_frac, wstate, standing, weapon, on_ground)) = frame(world)
            .player(ClientId(n))
            .map(|ps| {
                use playerstate_iw4::pm_flags;
                (
                    ps.origin,
                    ps.viewangles,
                    ps.view_height_current,
                    ps.pm_flags & pm_flags::LADDER != 0,
                    ps.pm_flags & pm_flags::MANTLE != 0,
                    // A climb with no room to stand at the ledge (BO2's mantle flag 4).
                    ps.pm_flags & pm_flags::MANTLE != 0
                        && ps.mantle_flags & playerstate_iw4::mantle_flags::FORCE_CROUCH != 0,
                    ps.pm_flags & pm_flags::SPRINTING != 0,
                    ps.f_weapon_pos_frac.clamp(0.0, 1.0),
                    ps.weaponstate_primary,
                    ps.pm_flags & (pm_flags::CROUCH | pm_flags::PRONE) == 0,
                    ps.weapon,
                    ps.ground_entity_num != playerstate_iw4::ENTITYNUM_NONE,
                )
            })
        else {
            continue;
        };
        // Full-automatic weapons (miniguns too) hold the trigger down.
        let full_auto = frame(world)
            .combat_facts_for(weapon)
            .is_none_or(|f| f.fire_type == 0);
        // Taking a negotiation link (BO2's Bot_AtNegotiationEnd): done once
        // its mantle finishes, once it stands within 15 units of the link's
        // end, or (a drop) once it leaves the ground; then it plans on at
        // once. Ours also gives up after 4 s or when the route is gone.
        {
            let mut zm = world.resource_mut::<Zm>();
            let b = zm.bots.get_mut(&n).unwrap();
            let was_mantling = std::mem::replace(&mut b.was_mantling, mantling);
            if let Some((l, since)) = b.neg {
                let near_end = on_ground && !mantling && dist3(origin, l.end) < 15.0;
                let landed = (was_mantling && !mantling) || near_end || (l.flags & NEG_DROP != 0 && !on_ground);
                let gave_up = now - since > 4000 || b.path.is_empty();
                if landed || gave_up {
                    b.neg = None;
                    b.path.clear();
                    b.path_to = None;
                    b.replan_at = now;
                    if botlog() {
                        diag::info!(
                            Sim,
                            "bo2mp bot {n} negotiation {} flags {:#x} at {:.0} {:.0} {:.0} end {:.0} {:.0} {:.0} after {} ms",
                            if landed { "done" } else { "gave up" },
                            l.flags,
                            origin[0],
                            origin[1],
                            origin[2],
                            l.end[0],
                            l.end[1],
                            l.end[2],
                            now - since
                        );
                    }
                }
            }
        }
        // The route: replan when the best goal moved or the route ran out.
        // (BO2's Bot_Seek: moved = the goal is its radius or more from
        // where the route ends, in 3D.)
        let (goal, path_to, replan_at) = {
            let zm = world.resource::<Zm>();
            let b = &zm.bots[&n];
            (b.best_goal().cloned(), b.path_to, b.replan_at)
        };
        if let Some(g) = &goal {
            let moved = path_to.is_none_or(|p| {
                let d = [p[0] - g.pos[0], p[1] - g.pos[1], p[2] - g.pos[2]];
                g.radius * g.radius <= d[0] * d[0] + d[1] * d[1] + d[2] * d[2]
            });
            let far = dist2d(origin, g.pos) > g.radius;
            let empty = world.resource::<Zm>().bots[&n].path.is_empty();
            if (moved || (empty && far)) && now >= replan_at {
                let (route, negs) = plan(world, origin, g.pos);
                let mut zm = world.resource_mut::<Zm>();
                let b = zm.bots.get_mut(&n).unwrap();
                b.path = route;
                b.negs = negs;
                b.neg = None;
                b.path_to = Some(g.pos);
                b.replan_at = now + 500;
                b.prev = Some(origin);
                b.path_gen = b.path_gen.wrapping_add(1);
                b.path_new = true;
                b.la_amount = 0.0;
                b.la_pos = None;
                b.la_dir = None;
                b.la_clear = false;
            }
        } else {
            let mut zm = world.resource_mut::<Zm>();
            let b = zm.bots.get_mut(&n).unwrap();
            b.path.clear();
            b.path_to = None;
        }
        let throw = throws.contains(&n);
        // Something at step height between it and the next route point
        // (a ledge, a railing, a low wall), as opposed to stairs or a slope
        // the walk steps up by itself: only then does a jump (and the
        // mantle it starts) help; jumping on stairs under a ceiling holds
        // a bot in place.
        let climb_blocked: Option<([f32; 3], bool)> = {
            let peek = {
                let zm = world.resource::<Zm>();
                let path = &zm.bots[&n].path;
                path.iter()
                    .find(|q| dist2d(origin, **q) >= 24.0)
                    .or(path.last())
                    .copied()
            };
            peek.map(|p| (p, dist2d(origin, p) < 160.0 && blocked_ahead(world, origin, p)))
        };
        // Blocked at step height toward the next point: going up (a ledge)
        // it jumps at once; level or down (a railing it drops over, a low
        // fence) it jumps once it makes no headway.
        let ahead_blocked = climb_blocked.is_some_and(|(_, b)| b);
        let climb_blocked = climb_blocked
            .is_some_and(|(p, b)| b && p[2] - origin[2] > 18.0 && dist2d(origin, p) < 96.0);
        // BO2MP_BOTLOG (test aid): is it standing inside the map or a prop,
        // or against another player?
        let body = body_touching(n, origin, &bodies);
        let touching = body.map(|(id, _)| id);
        let in_solid = if botlog() {
            use crate::bullet_collision::{MASK_PLAYER_SOLID, PLAYER_MAXS, PLAYER_MINS};
            let t = frame(world).trace_clip(origin, origin, PLAYER_MINS, PLAYER_MAXS, MASK_PLAYER_SOLID);
            t.startsolid != 0
        } else {
            false
        };
        let at_goal = goal
            .as_ref()
            .is_none_or(|g| dist2d(origin, g.pos) <= g.radius.max(16.0));
        // Past a route point once beyond the line across it (the walk cuts
        // corners toward its lookahead, so it seldom touches one), or (BO2's
        // Path_TrimCompletedPath) once it lies behind the bot along its last
        // lookahead, when that walk was clear or the amount has shrunk to
        // half: a nearest node behind it is no walk back.
        let (route, seen_key, seen) = {
            let mut zm = world.resource_mut::<Zm>();
            let b = zm.bots.get_mut(&n).unwrap();
            let trim = b
                .la_dir
                .filter(|_| b.la_clear || (b.la_amount > 0.0 && b.la_amount <= 16384.0))
                .map(|(d, _)| d);
            while b.path.len() > 1 {
                let p = b.path[0];
                if b.neg.is_none() && b.negs.iter().any(|l| l.begin == p) {
                    break;
                }
                let from = b.prev.unwrap_or(origin);
                let d = [p[0] - from[0], p[1] - from[1]];
                let beyond = d[0] * d[0] + d[1] * d[1] > 1.0
                    && (origin[0] - p[0]) * d[0] + (origin[1] - p[1]) * d[1] >= 0.0;
                let behind = trim.is_some_and(|t| t[0] * (p[0] - origin[0]) + t[1] * (p[1] - origin[1]) <= 0.0);
                if !(beyond || behind || dist2d(origin, p) < 24.0) {
                    break;
                }
                b.prev = Some(p);
                b.path.remove(0);
            }
            let near = b.path.get(1).is_some_and(|p| dist2d(origin, *p) < 192.0);
            let key = (b.path_gen, b.path.len(), near);
            let seen = b.seen.filter(|s| (s.0, s.1, s.2) == key).map(|s| s.3);
            ((!at_goal && !b.path.is_empty()).then(|| b.path.clone()), key, seen)
        };
        // The link its route crosses next, when it heads for the link's
        // begin now (BO2: the lookahead's next node is the begin): the walk
        // goes no farther than the begin, and it is at the begin (BO2's
        // Bot_AtNegotiationBegin) once no farther from the end than the begin
        // is (ours: or within 20 units of the begin).
        let neg_ahead = {
            let zm = world.resource::<Zm>();
            let b = &zm.bots[&n];
            match (&route, b.neg) {
                (Some(r), None) => b.negs.iter().find(|l| l.begin == r[0]).copied(),
                _ => None,
            }
        };
        let at_begin = neg_ahead.is_some_and(|l| {
            dist3(origin, l.end) < dist3(l.begin, l.end) + 0.5 || dist3(origin, l.begin) < 20.0
        });
        let route = match (route, neg_ahead) {
            (Some(r), Some(_)) => Some(r[..2.min(r.len())].to_vec()),
            (r, _) => r,
        };
        let far_seen = match (&route, seen) {
            (Some(_), Some(p)) => Some(p),
            (Some(r), None) => {
                let p = farthest_seen(world, origin, r);
                if let Some(p) = p {
                    let mut zm = world.resource_mut::<Zm>();
                    zm.bots.get_mut(&n).unwrap().seen = Some((seen_key.0, seen_key.1, seen_key.2, p));
                }
                p
            }
            _ => None,
        };
        // BO2's Path_UpdateLookaheadAmount: the walk to the lookahead is
        // traced each think; clear, it heads there and the amount grows
        // (x1.18 + 6.4, up to 65536); blocked, the amount shrinks (x0.8),
        // which pulls the point in toward the route's next bend. Its bots
        // drop the kept point every think (Bot_UpdateLookahead), so blocked
        // it heads for the new point, but for the one think right after a
        // clear walk (that walk's heading holds).
        let amount = world.resource::<Zm>().bots[&n].la_amount;
        let la_route = route.as_deref().map(|r| if neg_ahead.is_some() { &r[..1] } else { r });
        let la = match la_route.and_then(|r| lookahead(origin, r, amount)) {
            Some((p, reach)) => {
                // bo2mp: and with floor all the way (a corner cut off a
                // balcony is no clear walk); BO2MP_BOT_LOOKAHEAD_GROUND=0 = walls only.
                let clear = walk_clear(world, origin, p)
                    && (!lookahead_ground_on() || !on_ground || ladder || ground_along(world, origin, p));
                let mut zm = world.resource_mut::<Zm>();
                let b = zm.bots.get_mut(&n).unwrap();
                let amount = if b.la_amount > 0.0 { b.la_amount } else { 32768.0 };
                let was_clear = std::mem::replace(&mut b.la_clear, clear);
                if clear {
                    b.la_amount = (amount * 1.176_470_5 + 6.4).min(65536.0);
                    b.la_pos = Some(p);
                    Some((p, reach))
                } else {
                    b.la_amount = (amount * 0.8).max(0.001);
                    match b.la_pos.filter(|q| was_clear && dist2d(*q, origin) > 1.0) {
                        Some(q) => Some((q, dist2d(origin, q))),
                        None => Some((p, reach)),
                    }
                }
            }
            None => None,
        };
        let next = la.map(|(p, _)| p);
        {
            let mut zm = world.resource_mut::<Zm>();
            zm.bots.get_mut(&n).unwrap().la_dir = la.and_then(|(p, _)| {
                let d = [p[0] - origin[0], p[1] - origin[1]];
                let len = (d[0] * d[0] + d[1] * d[1]).sqrt();
                (len > 1e-3).then(|| ([d[0] / len, d[1] / len, (p[2] - origin[2]) / len], len))
            });
        }
        // Avoidance (Bot_UpdateAvoidance): off ladders and ledges, a bot on
        // a team bends its walk around teammates close by, keeping the
        // lookahead's distance; a bend into a clip brush right in front of
        // it holds it still for the tick.
        let mut walk_to = next;
        let mut hold = false;
        if let Some((lp, _)) = la
            && !ladder
            && !mantling
            && let Some(me) = crowd.iter().find(|a| a.id == n && !a.team.is_empty() && a.team != "free")
        {
            let d = [lp[0] - origin[0], lp[1] - origin[1]];
            let dist = dot2(d, d).sqrt();
            let speed = me.vel[0].hypot(me.vel[1]);
            let range = (speed + BOT_AVOID_SPEED) * 0.5 * AVOID_HORIZON;
            let others: Vec<&Avoider> = crowd
                .iter()
                .filter(|a| {
                    a.id != n
                        && a.team == me.team
                        && dist2d(a.pos, origin) < range
                        && (a.pos[2] - origin[2]).abs() < 90.0
                })
                .take(32)
                .collect();
            if dist > 1.0 && !others.is_empty() {
                let pref = [d[0] / dist * speed, d[1] / dist * speed];
                let v = avoid_velocity(me, pref, BOT_AVOID_SPEED, &others);
                let vl = v[0].hypot(v[1]);
                if vl > 0.01 {
                    let bent = [origin[0] + v[0] / vl * dist, origin[1] + v[1] / vl * dist, lp[2]];
                    hold = dist2d(bent, lp) > 15.0 && !avoid_clear(world, origin, bent);
                    walk_to = Some(bent);
                }
            }
        }
        // bo2mp fixes (each with its switch): an enemy in front, a way to
        // face with nowhere to head and a wall before it, and which side is
        // open for a stuck bot's step aside.
        let eye_now = [origin[0], origin[1], origin[2] + height];
        let (script_aims, look_set, progress, idle_cached) = {
            let zm = world.resource::<Zm>();
            let b = &zm.bots[&n];
            (b.script_aims, b.look.is_some(), b.progress, b.idle_look)
        };
        let front = if reflex_on() || botstats() { front_enemy(world, n, eye_now, view[1], 50.0) } else { None };
        let idle_yaw = if idle_look_on() && la.is_none() && !look_set && !script_aims && on_ground && !ladder && !mantling && front.is_none() {
            match idle_cached {
                Some((y, at)) if now - at < 1500 => y,
                _ => {
                    let y = open_yaw(world, eye_now, view[1]);
                    world.resource_mut::<Zm>().bots.get_mut(&n).unwrap().idle_look = Some((y, now));
                    y
                }
            }
        } else {
            if idle_cached.is_some() {
                world.resource_mut::<Zm>().bots.get_mut(&n).unwrap().idle_look = None;
            }
            None
        };
        let stuck_ms = progress
            .filter(|(at, _)| dist2d(*at, origin) < 12.0)
            .map_or(0, |(_, since)| now - since);
        let open_side = if unstick_on() && (1000..2400).contains(&stuck_ms) && !ladder { open_side(world, origin, view[1]) } else { None };
        let mut zm = world.resource_mut::<Zm>();
        let b = zm.bots.get_mut(&n).unwrap();
        b.tick = b.tick.wrapping_add(1);
        // The view (BO2's Bot_Think / Bot_UpdateLookahead): the angles it
        // chases move toward what it looks at (an eighth of the way in
        // pitch, a quarter in yaw each tick), else toward the farthest route
        // point it can see, slower the farther its lookahead reaches; the
        // view then steers toward them.
        let eye = [origin[0], origin[1], origin[2] + height];
        let reflex_pt = reflex_point(b, front, now);
        let look_pt = reflex_pt.or(b.look);
        let desired: Option<[f32; 2]> = if let Some(l) = look_pt {
            let (yaw, pitch) = angles_to(eye, l);
            let avg = b.aim_avg.unwrap_or([view[0], view[1]]);
            b.aim_avg = Some([avg[0] + angle_delta(avg[0], pitch) / 8.0, avg[1] + angle_delta(avg[1], yaw) / 4.0]);
            Some([pitch, yaw])
        } else if let Some((lp, reach)) = la {
            let t = far_seen.filter(|p| dist2d(origin, *p) >= 1.0).unwrap_or(lp);
            if dist2d(origin, t) >= 1.0 {
                let (yaw, pitch) = angles_to(origin, t);
                let h = if b.path_new || reach < 64.0 {
                    1.0
                } else if reach < 128.0 {
                    2.0
                } else {
                    4.0
                };
                let avg = b.aim_avg.unwrap_or([view[0], view[1]]);
                b.aim_avg = Some([avg[0] + angle_delta(avg[0], pitch) / (2.0 * h), avg[1] + angle_delta(avg[1], yaw) / h]);
                Some([pitch, yaw])
            } else {
                None
            }
        } else {
            b.aim_avg = None;
            None
        };
        b.path_new = false;
        // bo2mp: nowhere to head and a wall before its eyes: it turns to the
        // most open way (BO2MP_BOT_IDLE_LOOK=0: it keeps facing the wall).
        let desired = desired.or_else(|| {
            let y = idle_yaw?;
            let avg = b.aim_avg.unwrap_or([view[0], view[1]]);
            b.aim_avg = Some([avg[0] + angle_delta(avg[0], 0.0) / 8.0, avg[1] + angle_delta(avg[1], y) / 4.0]);
            Some([0.0, y])
        });
        let mut want = view;
        match (desired, b.aim_avg) {
            (Some(_), Some(avg)) => {
                let out = steer([view[0], view[1]], avg, &mut b.turn_speed, ads_frac, &dv);
                want[0] = out[0];
                want[1] = out[1];
            }
            _ => b.turn_speed = [0.0; 2],
        }
        let yaw_err = desired.map_or(0.0, |d| angle_delta(want[1], d[1]).abs());
        // Speed (Bot_GetMaxSpeed): full until the lookahead falls under 64
        // units, down to half; walking its route (no look-at) it also slows
        // while it faces away from where it heads, down to half at 35°.
        let speed = la.map_or(0.0, |(_, reach)| {
            let mut s = (reach / 64.0).clamp(0.5, 1.0);
            if look_pt.is_none() && desired.is_some() {
                s = s.min((1.0 - yaw_err / 70.0).clamp(0.5, 1.0));
            }
            s * 127.0
        });
        // Moving toward the lookahead, relative to where it faces; on a
        // ladder straight on (up it), while mantling not at all.
        let (mut forward, mut right) = (0i8, 0i8);
        let taking = b.neg.map(|(l, _)| l).or(neg_ahead.filter(|_| at_begin));
        // bo2mp: no headway for 400 ms and no body in the way: straight at
        // the route's next point instead of the cut corner (a doorway edge,
        // a prop). BO2MP_BOT_UNSTICK=0: the old way.
        let walk_to = match (b.path.first(), walk_to) {
            (Some(&p0), Some(_))
                if unstick_on()
                    && (400..1200).contains(&stuck_ms)
                    && touching.is_none()
                    && taking.is_none()
                    && dist2d(origin, p0) > 16.0 =>
            {
                Some(p0)
            }
            (_, w) => w,
        };
        if ladder {
            forward = 127;
        } else if let Some(l) = taking {
            // At the begin or on the link: toward its end, half speed (from
            // the eye, in the view's yaw axes), or full speed and flat for a
            // link run straight across.
            if !mantling {
                if l.flags & NEG_RUN != 0 {
                    let wish = [l.end[0] - origin[0], l.end[1] - origin[1]];
                    let len = (wish[0] * wish[0] + wish[1] * wish[1]).sqrt();
                    if len > 1e-3 {
                        let (s, c) = want[1].to_radians().sin_cos();
                        forward = (((wish[0] * c + wish[1] * s) / len).clamp(-1.0, 1.0) * 127.0) as i8;
                        right = (((wish[0] * s - wish[1] * c) / len).clamp(-1.0, 1.0) * 127.0) as i8;
                    }
                } else {
                    if dist3(l.end, eye) > 1e-3 {
                        (forward, right) = stick_toward(eye, l.end, want[1], 64.0);
                    }
                }
            }
        } else if !mantling && !hold && let Some(p) = walk_to {
            let mut wish = [p[0] - origin[0], p[1] - origin[1]];
            // Bodies (BO2's players block each other): off one it stands in
            // or against, round one in its way. The one it is after (what it
            // looks at: a knife rush) it walks straight at.
            let look = b.look;
            let near: Vec<(u32, [f32; 3])> = bodies
                .iter()
                .copied()
                .filter(|(_, q)| look.is_none_or(|l| dist2d(*q, l) > 24.0))
                .collect();
            if let Some(bent) = body_steer(n, origin, wish, &near) {
                wish = bent;
            }
            let len = (wish[0] * wish[0] + wish[1] * wish[1]).sqrt();
            if len > 1e-3 {
                let (s, c) = want[1].to_radians().sin_cos();
                forward = (((wish[0] * c + wish[1] * s) / len).clamp(-1.0, 1.0) * speed) as i8;
                right = (((wish[0] * s - wish[1] * c) / len).clamp(-1.0, 1.0) * speed) as i8;
            }
        }
        let mut press = 0u32;
        let mut reflex_shot = false;
        // Sprint (Bot_UpdateMovement): on a long straight run while facing
        // the way it goes, standing and not reloading; a sprint under 3 s
        // old carries on through a turn.
        if !sprinting {
            b.sprint_since = None;
        } else if b.sprint_since.is_none() {
            b.sprint_since = Some(now);
        }
        let reach = la.map_or(0.0, |(_, r)| r);
        let sprint = if speed == 0.0 || yaw_err >= 5.0 || reach < dv.sprint_distance {
            sprinting && b.sprint_since.is_some_and(|t| now - t < 3000)
        } else {
            !reloading(wstate) && wstate != weapon_iw4::WeaponState::MeleeInit as i32 && standing
        };
        if sprint && !b.no_sprint && b.stance == 0 && !ladder && !mantling {
            press |= buttons::SPRINT;
        }
        match b.stance {
            1 => press |= buttons::CROUCH,
            2 => press |= buttons::PRONE,
            _ => {}
        }
        // BO2: through a climb with no room to stand, the bot holds crouch
        // and lets go of prone, so it ends the climb ducked under the lintel.
        if low_climb {
            press = low_climb_press(press);
        }
        // Holding use, it presses nothing else (a plant, a defuse).
        if b.use_until > now {
            press = (press & (buttons::CROUCH | buttons::PRONE)) | buttons::USE_RELOAD;
        }
        // The trigger: one press when allowed, then a rest of
        // bot_minfiretime..bot_maxfiretime ms; a full-automatic holds it.
        // Firing lets go of sprint.
        if b.attack_ticks > 0 {
            press = (press & !buttons::SPRINT) | buttons::ATTACK;
            b.attack_ticks -= 1;
        } else if b.allow_attack && now >= b.fire_delay_until {
            press = (press & !buttons::SPRINT) | buttons::ATTACK;
            b.fire_delay_until = if full_auto { 0 } else { now + irand(n, now, dv.min_fire, dv.max_fire) };
        } else if let Some(pt) = reflex_pt
            && now >= b.fire_delay_until
            && !reloading(wstate)
            && reflex_aligned(want, eye, pt)
        {
            press = (press & !buttons::SPRINT) | buttons::ATTACK;
            b.fire_delay_until = if full_auto { 0 } else { now + irand(n, now, dv.min_fire, dv.max_fire) };
            reflex_shot = true;
        }
        if b.ads {
            press |= buttons::ADS;
        }
        // A melee only while not raising, lowering, meleeing or throwing.
        if std::mem::take(&mut b.melee) && !(1..=5).contains(&wstate) && !(14..=22).contains(&wstate) {
            press |= buttons::MELEE_CHARGE;
        }
        if std::mem::take(&mut b.dive) {
            press |= buttons::PRONE;
        }
        if throw {
            press |= buttons::FRAG;
        }
        // Jumps and vaults: BO2's engine takes a bot over the negotiation
        // links its path nodes mark (ledges, railings, low walls); ours
        // jumps where the route climbs close ahead (the movement code
        // mantles from a jump), and when walking makes no headway.
        // On a ladder it climbs (looking up its route, walking forward):
        // a jump would let go of it.
        // At a jump link's begin it jumps (BO2's Bot_Seek), once; still
        // there and not mantling, again on the whole second if it let go of
        // jump the tick before. Then it is taking the link.
        if let Some(l) = neg_ahead.filter(|_| at_begin) {
            if l.flags & NEG_JUMP != 0 && !mantling {
                press |= buttons::JUMP;
            }
            b.neg = Some((l, now));
            if botlog() {
                diag::info!(
                    Sim,
                    "bo2mp bot {n} negotiation begin flags {:#x} at {:.0} {:.0} {:.0} end {:.0} {:.0} {:.0}",
                    l.flags,
                    origin[0],
                    origin[1],
                    origin[2],
                    l.end[0],
                    l.end[1],
                    l.end[2]
                );
            }
        } else if let Some((l, _)) = b.neg
            && l.flags & NEG_JUMP != 0
            && !mantling
            && on_ground
            && dist3(origin, l.end) < dist3(l.begin, l.end) + 0.5
            && b.last_press & buttons::JUMP == 0
            && now % 1000 == 0
        {
            press |= buttons::JUMP;
        }
        if next.is_some() && !ladder && climb_blocked && taking.is_none() {
            press |= buttons::JUMP;
        }
        if next.is_some() && b.failsafe.is_none() {
            match b.progress {
                Some((at, since)) if dist2d(at, origin) < 12.0 => {
                    let stuck = now - since;
                    if stuck > 600 && (stuck / 300) % 2 == 0 && !ladder && ahead_blocked && taking.is_none() {
                        press |= buttons::JUMP;
                    }
                    // No headway: back off and step aside for a moment
                    // (a doorway corner, another player, a prop), then on.
                    // Pinned against (or in) another player it reacts sooner:
                    // a short step back and aside away from that body.
                    let body_window = (400..1400).contains(&stuck) && touching.is_some();
                    if ((1200..2400).contains(&stuck) || body_window) && !ladder {
                        forward = -127;
                        right = if (n + (since / 3000) as u32) % 2 == 0 { 127 } else { -127 };
                        // bo2mp: aside to the open side when one is walled.
                        if let Some(side) = open_side {
                            right = side;
                        }
                        if let Some(other) = touching.and_then(|id| bodies.iter().find(|b| b.0 == id)) {
                            // Aside means away from it: its side of us.
                            let (s, c) = want[1].to_radians().sin_cos();
                            let to = [other.1[0] - origin[0], other.1[1] - origin[1]];
                            let its_right = to[0] * s - to[1] * c;
                            if its_right.abs() > 1.0 {
                                right = if its_right > 0.0 { -127 } else { 127 };
                            }
                        }
                    }
                    // Stuck again short of the same route point: the back-off
                    // moves it, which restarts the stuck clock, so the 3 s
                    // drop below never came and bots looped there (Raid: back
                    // below the planter with the route's next point up on
                    // top). Drop the route now; it plans again from here.
                    if stuck >= 1200
                        && !ladder
                        && let Some(pt) = next
                        && stuck_again(&mut b.stuck_next, pt, since)
                    {
                        if botlog() {
                            diag::info!(Sim, "bo2mp bot {n} stuck again at {:.0} {:.0} {:.0}: new route", origin[0], origin[1], origin[2]);
                        }
                        b.stuck_next = None;
                        b.path.clear();
                        b.path_to = None;
                        b.replan_at = now + 250;
                        b.progress = Some((origin, now));
                    }
                    if touching.is_some() && stuck > 0 && stuck % 1000 == 0 {
                        diag::info!(
                            Sim,
                            "bo2mp bot {n} body-blocked {stuck} ms by client {:?} apart {:.1} at {:.0} {:.0} {:.0} stick {forward} {right}",
                            touching,
                            body.map_or(0.0, |b| b.1),
                            origin[0],
                            origin[1],
                            origin[2]
                        );
                    }
                    // Pinned by a body past the sidestep: plan afresh.
                    if touching.is_some() && stuck > 1500 {
                        b.path.clear();
                        b.path_to = None;
                        b.replan_at = now + 250;
                        b.progress = Some((origin, now));
                    }
                    if stuck % 1000 == 0 && stuck > 0 && botlog() {
                        diag::info!(
                            Sim,
                            "bo2mp bot {n} stuck {stuck} ms at {:.0} {:.0} {:.0} next {:?} ahead_blocked {ahead_blocked} in_solid {in_solid} touching {touching:?} goal {:?} routes {} left {} hold {hold} look {} stick {forward} {right}",
                            origin[0],
                            origin[1],
                            origin[2],
                            next.map(|p| [p[0] as i32, p[1] as i32, p[2] as i32]),
                            goal.as_ref().map(|g| (g.name.as_str(), [g.pos[0] as i32, g.pos[1] as i32, g.pos[2] as i32], g.radius as i32)),
                            b.path_gen,
                            b.path.len(),
                            b.look.is_some()
                        );
                    }
                    // Still stuck: drop the route; the next tick plans again.
                    if stuck > (if unstick_on() { 2200 } else { 3000 }) {
                        b.path.clear();
                        b.path_to = None;
                        b.replan_at = now + 250;
                        b.progress = Some((origin, now));
                    }
                }
                _ => b.progress = Some((origin, now)),
            }
        } else {
            b.progress = None;
        }
        // The failsafe (BO2's Bot_UpdateFailsafe), set by the stuck script:
        // it overrides the rest of the think. No sprint and no jump of its
        // own (G_BotThink still applies the script's buttons: stance, use,
        // fire, aim, melee), the view left alone; on a ladder straight on,
        // else straight at the node from the eye in the view's yaw axes;
        // within 15 units it stands and notifies "goal" with the node each
        // think until the script clears it.
        let mut failsafe_goal = None;
        if let Some((p, node)) = b.failsafe {
            press &= !(buttons::SPRINT | buttons::JUMP);
            want = view;
            (forward, right) = (0, 0);
            if ladder {
                forward = 127;
            } else if (0..3).map(|i| (p[i] - origin[i]).powi(2)).sum::<f32>() >= 225.0 {
                (forward, right) = stick_toward(eye, p, view[1], 127.0);
            } else {
                failsafe_goal = Some(node);
            }
            b.progress = None;
        }
        b.last_press = press;
        let fire_dbg = botfirelog().then(|| (b.allow_attack, b.fire_delay_until, b.look.is_some()));
        // BO2MP_BOTLOG=1: each bot every 2 s (test aid).
        if now % 2000 == 0 && botlog() {
            diag::info!(
                Sim,
                "bo2mp botlog {n}: at {:.0} {:.0} {:.0} view {:.0} {:.0} goal {:?} path {} walk {:?} face {:?} look {:?} attack {} ads {} press {press:#x} move {forward} {right} sprint {sprint} dodge {} hold {hold}",
                origin[0],
                origin[1],
                origin[2],
                view[0],
                view[1],
                goal.as_ref().map(|g| g.name.as_str()),
                b.path.len(),
                walk_to.map(|p| [p[0] as i32, p[1] as i32]),
                far_seen.map(|p| [p[0] as i32, p[1] as i32]),
                b.look.map(|l| [l[0] as i32, l[1] as i32, l[2] as i32]),
                b.allow_attack,
                b.ads,
                walk_to != next,
            );
        }
        drop(zm);
        if let Some(node) = failsafe_goal {
            let obj = world.resource::<Zm>().players.get(&n).map(|p| p.obj);
            if let Some(obj) = obj {
                let args: Vec<Value> = node.map(Value::Object).into_iter().collect();
                super::with_vm(world, |vm, world| vm.notify_str(world, obj, "goal", &args));
            }
        }
        // BO2MP_BOTS_STILL=1: the bots stand where they are and do nothing;
        // =fire:N (or fire:N,M) and those bots fire where they face too
        // (test aids, for a target that stays put and a shooter that does).
        if let Some(still) = std::env::var_os("BO2MP_BOTS_STILL") {
            let firing = still
                .to_str()
                .and_then(|s| s.strip_prefix("fire:"))
                .is_some_and(|ids| ids.split(',').any(|i| i.trim().parse::<u32>() == Ok(n)));
            let attack = if firing && (now / 100) % 2 == 0 { buttons::ATTACK } else { 0 };
            (forward, right, press, want) = (0, 0, attack, view);
        }
        // Bot Warfare's overrides on top of what it chose. A script's
        // switch moves the weapon override to the weapon switched to.
        let switching = frame(world).client_meta(ClientId(n)).map_or(0, |m| m.controls.switch_to);
        let (mut cmd_weapon, mut lunge) = (None, None);
        {
            let mut zm = world.resource_mut::<Zm>();
            if let Some(b) = zm.bots.get_mut(&n) {
                press |= b.held;
                if let Some((f, r)) = b.stick {
                    (forward, right) = (f, r);
                }
                if b.script_aims {
                    want = view;
                }
                if switching != 0 && b.weapon_override.is_some() {
                    b.weapon_override = Some(switching);
                }
                cmd_weapon = b.weapon_override;
                lunge = b.melee_target;
            }
        }
        // bo2mp: a walk off a ledge it did not plan (Nuketown's upper
        // floor) is held at the edge; a drop on its route (the next points
        // lower, a link) is let through. BO2MP_BOT_LEDGE_GUARD=0: the old way.
        let lead: Vec<[f32; 3]> = {
            let zm = world.resource::<Zm>();
            zm.bots
                .get(&n)
                .map_or(Vec::new(), |b| b.path.iter().take(2).copied().chain(b.failsafe.map(|f| f.0)).collect())
        };
        let drop_planned = taking.is_some()
            || neg_ahead.is_some()
            || lead.iter().any(|p| origin[2] - p[2] > 40.0 && dist2d(origin, *p) < 192.0);
        let mut ledge_held = false;
        if ledge_guard_on() && on_ground && !ladder && !mantling && !drop_planned && (forward, right) != (0, 0) {
            let dir = stick_world_dir(forward, right, want[1]);
            if drop_ahead(world, origin, dir) {
                (forward, right) = (0, 0);
                press &= !buttons::SPRINT;
                ledge_held = true;
            }
        }
        if botstats() {
            let t = StatTick {
                n,
                now,
                dt,
                origin,
                routed: next.is_some(),
                airborne: !on_ground && !ladder && !mantling,
                front: front.is_some_and(|f| f.3 <= 30.0),
                fired: press & buttons::ATTACK != 0,
                reloading: reloading(wstate),
                reflex_shot,
                ledge_held,
                drop_planned,
            };
            if let Some(b) = world.resource_mut::<Zm>().bots.get_mut(&n) {
                note_stats(b, &t);
            }
        }
        let cmd_weapon = cmd_weapon.filter(|&w| frame(world).player(ClientId(n)).is_some_and(|ps| ps.weapons.contains(&(w as i32))));
        let lunge = lunge.and_then(|(e, dist)| {
            let to = ent_origin(world, e)?;
            let d = [to[0] - origin[0], to[1] - origin[1]];
            Some((d[1].atan2(d[0]).to_degrees(), dist.clamp(0.0, 255.0) as u8))
        });
        if let Some((allow, delay_until, looking)) = fire_dbg {
            let line = FireLog {
                now,
                view_yaw: want[1],
                steer_err: yaw_err,
                weapon,
                wstate,
                sprinting,
                sprint_wanted: sprint,
                allow,
                delay_until,
                looking,
                press,
            };
            log_botfire(world, n, &line);
        }
        // Its command for the tick, the view turned as a script's
        // setplayerangles turns it (the delta under the command angles).
        let mut cmd_angles = None;
        {
            let mut req = world.resource_mut::<crate::step::StepRequest>();
            for (id, cmd) in &mut req.input.cmds {
                if id.0 != n {
                    continue;
                }
                cmd.buttons = press;
                cmd.forwardmove = forward;
                cmd.rightmove = right;
                if let Some(w) = cmd_weapon {
                    cmd.weapon = w as u16;
                }
                if let Some((yaw, dist)) = lunge {
                    cmd.melee_charge_yaw = yaw;
                    cmd.melee_charge_dist = dist;
                }
                cmd_angles = Some(cmd.angles);
            }
        }
        if let Some(angles) = cmd_angles {
            let mut f = frame(world);
            if let Some(p) = f.player_mut(ClientId(n)) {
                p.delta_angles = std::array::from_fn(|i| {
                    let cmd_deg = (angles[i] as u16) as f32 * (360.0 / 65536.0);
                    want[i] - cmd_deg
                });
                p.viewangles = want;
            }
        }
    }
}

/// A route from a place to a goal over the path nodes (the goal last),
/// and the negotiation links it crosses; straight there when the map has no
/// route between them.
fn plan(world: &mut World, from: [f32; 3], to: [f32; 3]) -> (Vec<[f32; 3]>, Vec<NegLink>) {
    let (Some(a), Some(b)) = (nearest_reachable(world, from), nearest_reachable(world, to)) else {
        return (vec![to], Vec::new());
    };
    let nav = &world.resource::<Zm>().nav;
    let Some(nodes) = nav.path(a, b) else {
        return (vec![to], Vec::new());
    };
    let mut out: Vec<[f32; 3]> = nodes
        .iter()
        .map(|&i| nav.nodes[i as usize].origin)
        .collect();
    let negs: Vec<NegLink> = nodes
        .windows(2)
        .filter(|w| nav.nodes[w[0] as usize].ty == super::nav::NODE_NEGOTIATION_BEGIN && nav.negotiation(w[0], w[1]))
        .map(|w| NegLink {
            begin: nav.nodes[w[0] as usize].origin,
            end: nav.nodes[w[1] as usize].origin,
            flags: nav.nodes[w[0] as usize].spawnflags,
        })
        .collect();
    out.push(to);
    // BO2 smooths each path it plans (Path_SmoothPath: from a node straight
    // on to the farthest later one it can walk to). Ours does it from where
    // the bot stands, up to three nodes on (a trace each), so a nearest
    // node behind it is no U-turn it then rocks on.
    let mut skipped = 0;
    while out.len() > 1 && skipped < 3 && walk_clear(world, from, out[1]) {
        out.remove(0);
        skipped += 1;
    }
    (out, negs)
}

/// The map's negotiation links by kind, once a run (test aid).
fn log_negotiations(world: &World) {
    static DONE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
    if DONE.load(std::sync::atomic::Ordering::Relaxed) {
        return;
    }
    let nav = &world.resource::<Zm>().nav;
    if nav.nodes.is_empty() {
        return;
    }
    DONE.store(true, std::sync::atomic::Ordering::Relaxed);
    let mut kinds: BTreeMap<String, u32> = BTreeMap::new();
    for node in nav.nodes.iter().filter(|n| n.ty == super::nav::NODE_NEGOTIATION_BEGIN) {
        let f = node.spawnflags;
        let kind = if f & NEG_DROP != 0 {
            "drop"
        } else if f & NEG_JUMP != 0 {
            "jump"
        } else if f & NEG_RUN != 0 {
            "run"
        } else {
            "other"
        };
        *kinds.entry(format!("{kind} {:#x}", f & (NEG_JUMP | NEG_DROP | NEG_RUN))).or_default() += 1;
    }
    diag::info!(Sim, "bo2mp negotiation links: {kinds:?}");
}

#[cfg(test)]
mod body_steer_tests {
    use super::*;

    #[test]
    fn nothing_close_walks_on() {
        let bodies = [(1, [0.0; 3]), (2, [500.0, 0.0, 0.0])];
        assert_eq!(body_steer(1, [0.0; 3], [1.0, 0.0], &bodies), None);
    }

    #[test]
    fn a_bot_inside_another_is_pushed_out() {
        // Fused 6 units apart, wanting to walk straight through it.
        let bodies = [(1, [0.0; 3]), (2, [6.0, 0.0, 0.0])];
        let w = body_steer(1, [0.0; 3], [1.0, 0.0], &bodies).unwrap();
        assert!(w[0] < 0.2, "must not keep walking into it: {w:?}");
        let w2 = body_steer(2, [6.0, 0.0, 0.0], [-1.0, 0.0], &[(1, [0.0; 3]), (2, [6.0, 0.0, 0.0])]).unwrap();
        // The pair part: each heads off its own way.
        assert!(w[0] * w2[0] + w[1] * w2[1] < 0.5, "{w:?} {w2:?}");
    }

    #[test]
    fn a_body_in_the_way_is_passed_on_its_open_side() {
        // A player 50 ahead, a little to the left (+y): pass on the right.
        let bodies = [(1, [0.0; 3]), (2, [50.0, 8.0, 0.0])];
        let w = body_steer(1, [0.0; 3], [1.0, 0.0], &bodies).unwrap();
        assert!(w[0] > 0.0 && w[1] < 0.0, "{w:?}");
    }

    #[test]
    fn head_on_bots_pick_opposite_sides() {
        let bodies = [(1, [0.0; 3]), (2, [50.0, 0.0, 0.0])];
        let a = body_steer(1, [0.0; 3], [1.0, 0.0], &bodies).unwrap();
        let b = body_steer(2, [50.0, 0.0, 0.0], [-1.0, 0.0], &bodies).unwrap();
        // Same sign of world y would collide again; opposite passes.
        assert!(a[1] * b[1] < 0.0, "{a:?} {b:?}");
    }

    #[test]
    fn touching_finds_the_nearest_body_on_its_floor() {
        let bodies = [(1, [0.0; 3]), (2, [20.0, 0.0, 0.0]), (3, [10.0, 0.0, 200.0])];
        assert_eq!(body_touching(1, [0.0; 3], &bodies).map(|b| b.0), Some(2));
        assert_eq!(body_touching(1, [0.0; 3], &bodies[..1]), None);
    }
}

#[cfg(test)]
mod new_life_tests {
    use super::*;

    #[test]
    fn a_failsafe_and_route_do_not_outlive_the_bot() {
        // The script's failsafe set and a route walked, then the bot dies:
        // the new life plans its own route, not at the last life's node.
        let mut b = Bot {
            failsafe: Some(([-808.0, -496.0, 68.0], None)),
            path_to: Some([864.0, 838.0, -1.0]),
            la_amount: 32768.0,
            progress: Some(([0.0; 3], 5000)),
            ..Default::default()
        };
        b.path.push([864.0, 838.0, -1.0]);
        b.classes.insert(0, BotClass { default: Some("class_smg".into()), ..Default::default() });
        clear_for_new_life(&mut b);
        assert!(b.failsafe.is_none());
        assert!(b.path.is_empty() && b.path_to.is_none());
        assert_eq!(b.la_amount, 0.0);
        assert!(b.progress.is_none());
        // Its scripts' classes stay.
        assert_eq!(b.classes.len(), 1);
    }

    #[test]
    fn failsafe_and_link_moves_use_the_views_yaw_axes() {
        let eye = [0.0, 0.0, 60.0];
        // Straight ahead at eye height: full forward.
        assert_eq!(stick_toward(eye, [100.0, 0.0, 60.0], 0.0, 127.0), (127, 0));
        // To its left (+y facing +x): a full left stick (right negative).
        assert_eq!(stick_toward(eye, [0.0, 100.0, 60.0], 0.0, 127.0), (0, -127));
        // Facing +y, the same node is straight ahead.
        let (f, r) = stick_toward(eye, [0.0, 100.0, 60.0], 90.0, 127.0);
        assert_eq!((f, r.abs()), (127, 0));
        // A link's end at half speed, the same whatever the view's pitch
        // (the pitch is not an input).
        assert_eq!(stick_toward(eye, [100.0, 0.0, 60.0], 0.0, 64.0), (64, 0));
    }

    #[test]
    fn stuck_twice_short_of_the_same_point_drops_the_route() {
        let pt = [3904.0, 120.0, 40.0];
        let mut s = None;
        // The first stuck there is remembered; the same stuck again is not new.
        assert!(!stuck_again(&mut s, pt, 1000));
        assert!(!stuck_again(&mut s, pt, 1000));
        // A later stuck short of the same point: drop the route.
        assert!(stuck_again(&mut s, pt, 4000));
        // Short of another point it starts over.
        let mut s = None;
        assert!(!stuck_again(&mut s, pt, 1000));
        assert!(!stuck_again(&mut s, [0.0, 0.0, 40.0], 4000));
        assert!(stuck_again(&mut s, [0.0, 0.0, 40.0], 9000));
    }

    #[test]
    fn a_box_in_solid_shrinks_by_five_down_to_ten_across() {
        let boxes: Vec<_> = step_boxes(15.0, 52.0).collect();
        assert_eq!(boxes, vec![(15.0, 52.0), (10.0, 47.0)]);
        assert_eq!(step_boxes(10.0, 52.0).count(), 1);
    }

    #[test]
    fn a_low_climb_holds_crouch_and_lets_go_of_prone() {
        let p = low_climb_press(buttons::PRONE | buttons::ATTACK);
        assert_eq!(p, buttons::CROUCH | buttons::ATTACK);
        assert_eq!(low_climb_press(0), buttons::CROUCH);
    }
}

#[cfg(test)]
mod bot_fix_tests {
    use super::*;

    #[test]
    fn the_sticks_world_direction_matches_the_view() {
        // Forward facing +y walks +y; right facing +x walks -y (left is +y).
        let d = stick_world_dir(127, 0, 90.0);
        assert!(d[0].abs() < 1e-4 && (d[1] - 1.0).abs() < 1e-4);
        let d = stick_world_dir(0, 127, 0.0);
        assert!(d[0].abs() < 1e-4 && (d[1] + 1.0).abs() < 1e-4);
        assert_eq!(stick_world_dir(0, 0, 33.0), [0.0, 0.0]);
    }

    #[test]
    fn the_reflex_waits_its_delay_and_yields_to_the_scripts() {
        let mut b = Bot::default();
        let front = Some((3u32, [100.0, 0.0, 40.0], 100.0, 0.0));
        // Seen at 1000: nothing until 400 ms in front of it.
        assert!(reflex_point(&mut b, front, 1000).is_none());
        assert!(reflex_point(&mut b, front, 1300).is_none());
        assert_eq!(reflex_point(&mut b, front, 1400), Some([100.0, 0.0, 40.0]));
        // Out of sight briefly: it keeps on him; for over 300 ms, it lets go.
        assert!(reflex_point(&mut b, None, 1500).is_none());
        assert!(b.reflex.is_some());
        assert!(reflex_point(&mut b, None, 1800).is_none());
        assert!(b.reflex.is_none());
        // Its scripts firing: no reflex.
        b.allow_attack = true;
        for t in [2000, 2500, 3000] {
            assert!(reflex_point(&mut b, front, t).is_none());
        }
        // Its scripts aiming the view: none either.
        b.allow_attack = false;
        b.script_aims = true;
        assert!(reflex_point(&mut b, front, 4000).is_none());
    }

    #[test]
    fn the_reflex_fires_only_lined_up() {
        let eye = [0.0, 0.0, 60.0];
        let pt = [500.0, 0.0, 60.0];
        assert!(reflex_aligned([0.0, 1.0, 0.0], eye, pt));
        assert!(!reflex_aligned([0.0, 10.0, 0.0], eye, pt));
        assert!(!reflex_aligned([20.0, 0.0, 0.0], eye, pt));
    }

    #[test]
    fn an_enemy_in_front_left_unshot_750_ms_is_one_missed_shot() {
        let mut b = Bot::default();
        let tick = |now, fired| StatTick {
            n: 0,
            now,
            dt: 50,
            origin: [0.0; 3],
            routed: false,
            airborne: false,
            front: true,
            fired,
            reloading: false,
            reflex_shot: false,
            ledge_held: false,
            drop_planned: false,
        };
        let before = STATS.lock().unwrap().missed;
        for i in 0..20 {
            note_stats(&mut b, &tick(10_000 + i * 50, false));
        }
        assert_eq!(STATS.lock().unwrap().missed - before, 1);
        // Firing ends it; idle again for 750 ms is another.
        note_stats(&mut b, &tick(11_000, true));
        for i in 0..20 {
            note_stats(&mut b, &tick(11_050 + i * 50, false));
        }
        assert_eq!(STATS.lock().unwrap().missed - before, 2);
    }
}
