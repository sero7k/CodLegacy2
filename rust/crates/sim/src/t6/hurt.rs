//! Death barriers: the map's `trigger_hurt` volumes hurt (and mostly kill)
//! the players inside them, as the engine's `hurt_touch` does, and a fall
//! kill plane below the map's lowest collision ends a fall off the map that
//! would otherwise never land.
//!
//! `trigger_hurt` (the Quake III / RTCW lineage every IW and Treyarch engine
//! keeps, `SP_trigger_hurt` / `hurt_touch`):
//! - `dmg` per touch, 5 when the map leaves it 0;
//! - a touch hurts once per server frame (50 ms), or once a second with
//!   spawnflag SLOW (16);
//! - spawnflag NO_PROTECTION (8) passes the scripts' `level.idflags_no_protection`;
//! - the hit is `G_Damage(player, self, self, ..., MOD_TRIGGER_HURT)`: the
//!   trigger itself is the inflictor and attacker (BO2's
//!   `callback_playerdamage` reads `einflictor.script_noteworthy ==
//!   "ragdoll_now"` off it);
//! - START_OFF (1) is logged, not honoured (`HURT_HONOUR_START_OFF`): the
//!   IW/Treyarch scripts toggle hazards with `trigger_off` / `trigger_on`
//!   (moving the trigger 10000 down), which the volume test follows.
//!
//! BO3's `trigger_out_of_bounds` / `trigger_radius_out_of_bounds` (only
//! Black Ops III maps place them) are not death barriers: they run BO3's own
//! out-of-bounds countdown and screen (`super::oob`, from its `_oob.gsc`),
//! killing only when the time runs out. Triggers whose names merely suggest
//! "out of bounds" (a `trigger_multiple` named `oob`...) are listed in the
//! log but left to the map's scripts, which own what their `trigger` notify
//! means.
//!
//! The fall kill plane: no engine plane exists in BO2; its maps close the
//! void with `trigger_hurt`. Maps from other games don't always (and their
//! own scripts aren't here), so a plane is derived per map from its
//! collision: the lowest floor of any collision that stops a player (clip
//! brushes' tops, the clip mesh, static models; the world model's bounds
//! only without them) minus a margin, and
//! never above the lowest spawn point minus the same margin. Nothing can be
//! stood on below it, so it cannot cut playable space.

use std::collections::BTreeMap;

use bevy_ecs::prelude::{Resource, World};
use gsc_t6::{ObjRef, Value};

use super::{Ent, Zm, frame, natives_ai, triggers, with_vm};
use crate::world::ClientId;

/// `SP_trigger_hurt`: a 0 `dmg` hurts 5.
pub(crate) const HURT_DEFAULT_DMG: i32 = 5;
/// `hurt_touch`: once per server frame...
pub(crate) const HURT_INTERVAL_MS: i64 = crate::MATCH_TICK_MS as i64;
/// ...or once a second with spawnflag SLOW.
pub(crate) const HURT_SLOW_INTERVAL_MS: i64 = 1000;
pub(crate) const HURT_SPAWNFLAG_START_OFF: i32 = 1;
pub(crate) const HURT_SPAWNFLAG_NO_PROTECTION: i32 = 8;
pub(crate) const HURT_SPAWNFLAG_SLOW: i32 = 16;
/// Off: a START_OFF `trigger_hurt` still hurts (see the module notes).
pub(crate) const HURT_HONOUR_START_OFF: bool = false;
/// A hurt trigger's hit is logged from this much (a lethal-sized one).
const HURT_LOG_DAMAGE: i32 = 100;
/// Damage a kill plane / out-of-bounds trigger deals: more than any health.
pub(crate) const KILL_DAMAGE: i32 = 100_000;
/// The kill plane sits this far (at least) below the lowest collision...
pub(crate) const KILL_PLANE_MARGIN_MIN: f32 = 512.0;
/// ...or this share of the map's collision height, if more (a tall map
/// gets a longer visible fall before the plane), capped.
pub(crate) const KILL_PLANE_MARGIN_SPAN_FRACTION: f32 = 0.1;
pub(crate) const KILL_PLANE_MARGIN_MAX: f32 = 2048.0;
/// Dvar `bo2mp_killplane` 0 turns the fall kill plane off.
pub(crate) const KILL_PLANE_DVAR: &str = "bo2mp_killplane";

/// `next_hurt`'s key for the kill plane (no entity has this number).
const KILL_PLANE_KEY: u32 = u32::MAX;

/// Per-map barrier state (outlives a round restart's `Zm`).
#[derive(Resource, Default)]
pub(crate) struct Barriers {
    map: String,
    planned: bool,
    kill_z: Option<f32>,
    /// (trigger entity, client) -> when it may hurt again.
    next_hurt: BTreeMap<(u32, u32), i64>,
    /// Trigger entities already described in the log.
    reported: std::collections::BTreeSet<(String, u32)>,
    /// The map's barrier summary is logged.
    summarized: bool,
    /// bo2mp: the fall-through detector: the lowest spawn point, each
    /// player's last place on the ground, and the players whose current
    /// fall is already logged.
    lowest_spawn: Option<f32>,
    last_ground: BTreeMap<u32, [f32; 3]>,
    fall_logged: std::collections::BTreeSet<u32>,
    /// bo2mp: Black Ops III's out-of-bounds countdowns.
    oob: super::oob::Oob,
    /// bo2mp: when the barriers were first planned for this map (the match
    /// start grace, `MATCH_START_GRACE_MS`).
    started_at: Option<i64>,
    /// bo2mp: barrier volumes (by entity) that hold one of the map's spawn
    /// points, worked out once per volume.
    holds_spawn: BTreeMap<u32, bool>,
    /// bo2mp: (client, volume or `KILL_PLANE_KEY`, reason) already said.
    spared_said: std::collections::BTreeSet<(u32, u32, &'static str)>,
}

/// bo2mp: for this long after a spawn, Black Ops III's out-of-bounds volumes
/// start no countdown, and on another game's Zombies map no hurt trigger and
/// no kill plane hurts him (a spawn our reading of that map puts inside one
/// was an instant death: 7ecf2eb, 70d2290).
pub(crate) const SPAWN_GRACE_MS: i64 = 5000;
/// bo2mp: for this long after the match starts, no out-of-bounds countdown.
pub(crate) const MATCH_START_GRACE_MS: i64 = 10_000;

/// What can hurt a player for where he is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum BarrierHit {
    /// A `trigger_hurt`.
    Hurt,
    /// Black Ops III's out-of-bounds volume (its countdown).
    OutOfBounds,
    /// The fall kill plane.
    KillPlane,
}

/// Where and when a barrier would reach a player.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct BarrierCase {
    /// Zombies on another game's map (BO3, IW, ...: `foreign_zm`).
    pub foreign_zombies: bool,
    /// Since his last spawn (ms), when known.
    pub since_spawn_ms: Option<i64>,
    /// Since the match (the map's barriers) started (ms).
    pub since_start_ms: i64,
    /// The volume holds a spawn point (or where he spawned).
    pub volume_holds_spawn: bool,
}

/// bo2mp: why this barrier leaves him alone (`None`: it hurts). Black Ops
/// III's out-of-bounds volumes never count in a spawn area, in the first
/// seconds of a life or of the match; on another game's Zombies map neither
/// does a hurt trigger over a spawn or in the first seconds of a life, nor
/// the kill plane then. Black Ops II's own maps keep their barriers as they
/// are.
pub(crate) fn barrier_spares(hit: BarrierHit, case: BarrierCase) -> Option<&'static str> {
    let in_spawn_grace = case.since_spawn_ms.is_some_and(|ms| ms < SPAWN_GRACE_MS);
    match hit {
        BarrierHit::OutOfBounds => {
            if case.volume_holds_spawn {
                Some("spawn area")
            } else if case.since_start_ms < MATCH_START_GRACE_MS {
                Some("match start grace")
            } else if in_spawn_grace {
                Some("spawn grace")
            } else {
                None
            }
        }
        BarrierHit::Hurt if case.foreign_zombies => {
            if case.volume_holds_spawn {
                Some("spawn area")
            } else if in_spawn_grace {
                Some("spawn grace")
            } else {
                None
            }
        }
        BarrierHit::KillPlane if case.foreign_zombies && in_spawn_grace => Some("spawn grace"),
        _ => None,
    }
}

/// A player-sized box standing at `at`.
fn standing_box(at: [f32; 3]) -> ([f32; 3], [f32; 3]) {
    ([at[0] - 15.0, at[1] - 15.0, at[2]], [at[0] + 15.0, at[1] + 15.0, at[2] + 72.0])
}

/// Whether barrier volume `n` holds one of the map's spawn points (once per
/// volume) or a place a player spawned at this match.
fn volume_holds_spawn(world: &mut World, n: u32, e: &Ent) -> bool {
    let known = world.resource::<Barriers>().holds_spawn.get(&n).copied();
    let map_spawn = match known {
        Some(v) => v,
        None => {
            let spawns: Vec<[f32; 3]> = world
                .resource::<Zm>()
                .ents
                .values()
                .filter(|s| is_spawn_point(&s.classname))
                .map(|s| s.origin)
                .collect();
            let v = spawns.iter().any(|&o| {
                let (lo, hi) = standing_box(o);
                triggers::touches(world, e, lo, hi)
            });
            world.resource_mut::<Barriers>().holds_spawn.insert(n, v);
            v
        }
    };
    if map_spawn {
        return true;
    }
    let spawned: Vec<[f32; 3]> = world
        .get_resource::<SpawnLog>()
        .map(|l| l.0.values().map(|(_, at)| *at).collect())
        .unwrap_or_default();
    spawned.iter().any(|&o| {
        let (lo, hi) = standing_box(o);
        triggers::touches(world, e, lo, hi)
    })
}

/// His time since his last spawn (ms), when known.
fn since_spawn(world: &World, client: u32) -> Option<i64> {
    let now = super::tick(world).0;
    world
        .get_resource::<SpawnLog>()
        .and_then(|l| l.0.get(&client))
        .map(|(at, _)| since_spawn_ms(*at, now))
}

/// Says once per (player, volume, reason) that a barrier spared him.
fn say_spared(world: &mut World, client: u32, n: u32, what: &str, why: &'static str) {
    if world.resource_mut::<Barriers>().spared_said.insert((client, n, why)) {
        diag::info!(Sim, "bo2mp death barrier: player {client} spared by {what}: {why}");
    }
}

/// A player this far under the lowest spawn point, in the air, is logged
/// as a possible fall through the map.
pub(crate) const FALL_REPORT_DEPTH: f32 = 256.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Barrier {
    Hurt,
    /// Black Ops III's out-of-bounds volume: its countdown screen.
    OutOfBounds,
}

fn barrier(classname: &str) -> Option<Barrier> {
    match classname {
        "trigger_hurt" => Some(Barrier::Hurt),
        "trigger_out_of_bounds" | "trigger_radius_out_of_bounds" => Some(Barrier::OutOfBounds),
        _ => None,
    }
}

fn int_key(vm: &mut gsc_t6::Vm<World>, obj: ObjRef, key: &str) -> i32 {
    let f = vm.intern(key);
    match vm.raw_field(obj, f) {
        Value::Int(i) => i,
        Value::Float(x) => x as i32,
        _ => 0,
    }
}

fn text_key(vm: &mut gsc_t6::Vm<World>, obj: ObjRef, key: &str) -> String {
    let f = vm.intern(key);
    match vm.raw_field(obj, f) {
        Value::Undefined => String::new(),
        v => vm.to_text(&v),
    }
}

/// The damage one touch of a hurt trigger deals (`dmg`, 5 when 0).
pub(crate) fn hurt_damage(dmg: i32) -> i32 {
    if dmg == 0 { HURT_DEFAULT_DMG } else { dmg }
}

/// How long after a touch the trigger hurts the same player again.
pub(crate) fn hurt_interval(spawnflags: i32) -> i64 {
    if spawnflags & HURT_SPAWNFLAG_SLOW != 0 {
        HURT_SLOW_INTERVAL_MS
    } else {
        HURT_INTERVAL_MS
    }
}

/// The kill plane from the map's lowest collision, its highest, and its
/// lowest spawn point. None without collision.
pub(crate) fn kill_plane_z(
    lowest_collision: Option<f32>,
    highest_collision: Option<f32>,
    lowest_spawn: Option<f32>,
) -> Option<(f32, f32)> {
    let low = lowest_collision.filter(|z| z.is_finite())?;
    let span = highest_collision
        .filter(|z| z.is_finite() && *z > low)
        .map_or(0.0, |h| h - low);
    let margin = (span * KILL_PLANE_MARGIN_SPAN_FRACTION)
        .clamp(KILL_PLANE_MARGIN_MIN, KILL_PLANE_MARGIN_MAX);
    let mut z = low - margin;
    if let Some(s) = lowest_spawn.filter(|z| z.is_finite()) {
        z = z.min(s - margin);
    }
    Some((z, margin))
}

/// The lowest point a brush's axial bottom plane allows (a plane
/// `n.x <= d` with `n = (0, 0, -1)` holds `z >= -d`).
#[cfg(test)]
fn brush_bottom(planes: &[[f32; 4]]) -> Option<f32> {
    planes
        .iter()
        .filter(|p| p[2] < -0.999 && p[0].abs() < 1e-3 && p[1].abs() < 1e-3)
        .map(|p| -p[3] / -p[2])
        .reduce(f32::max)
}

/// The lowest and highest corner of the clip mesh's triangles that stop a
/// player (`MASK_PLAYERSOLID`; all of them with no contents table).
pub(crate) fn player_solid_mesh_z(mesh: &clipmap_iw4::ClipMeshTables) -> Option<(f32, f32)> {
    let mut range: Option<(f32, f32)> = None;
    for p in &mesh.partitions {
        let base = usize::from(p.first_vert_segment) * clipmap_iw4::VERTS_PER_SEGMENT;
        for t in p.first_tri.max(0) as usize..p.first_tri.max(0) as usize + usize::from(p.tri_count) {
            if mesh
                .tri_content_flags
                .get(t)
                .is_some_and(|c| c & crate::bullet_collision::MASK_PLAYER_SOLID == 0)
            {
                continue;
            }
            for k in 0..3 {
                let Some(&i) = mesh.tri_indices.get(3 * t + k) else {
                    continue;
                };
                let Some(v) = mesh.verts.get(base + usize::from(i)) else {
                    continue;
                };
                range = Some(range.map_or((v[2], v[2]), |(lo, hi)| (lo.min(v[2]), hi.max(v[2]))));
            }
        }
    }
    range
}

fn brush_top(planes: &[[f32; 4]]) -> Option<f32> {
    planes
        .iter()
        .filter(|p| p[2] > 0.999 && p[0].abs() < 1e-3 && p[1].abs() < 1e-3)
        .map(|p| p[3] / p[2])
        .reduce(f32::min)
}

fn plan(world: &mut World) {
    let map = world.resource::<Zm>().map.clone();
    let lowest_spawn = world
        .resource::<Zm>()
        .ents
        .values()
        .filter(|e| is_spawn_point(&e.classname))
        .map(|e| e.origin[2])
        .reduce(f32::min);
    let spawns = world
        .resource::<Zm>()
        .ents
        .values()
        .filter(|e| is_spawn_point(&e.classname))
        .count();
    let (sources, low, high) = {
        let f = frame(world);
        let mut sources: Vec<(&str, f32, f32)> = Vec::new();
        if let Some(m) = f.clip_cmodels().models.first()
            && m.mins[2].is_finite()
            && m.maxs[2] > m.mins[2]
        {
            sources.push(("world model", m.mins[2], m.maxs[2]));
        }
        // bo2mp: only what stops a player (MASK_PLAYERSOLID), and of a
        // brush its top, the lowest place a fall can land on: Black Ops
        // III's sky brushes (STRUCTURAL | SKY) and player-clip walls reach
        // 65536 down, which put Skyjacked's plane 67 000 units under the
        // map and turned a fall off it into a long drop into nothing.
        let brushes = f.clip_brushes();
        let solid = || {
            brushes
                .iter()
                .filter(|b| b.contents & crate::bullet_collision::MASK_PLAYER_SOLID != 0)
        };
        let lo = solid()
            .filter_map(|b| brush_top(&b.planes))
            .reduce(f32::min);
        let hi = solid()
            .filter_map(|b| brush_top(&b.planes))
            .reduce(f32::max);
        if let (Some(lo), Some(hi)) = (lo, hi) {
            sources.push(("clip brush floors", lo, hi));
        }
        if let Some((lo, hi)) = player_solid_mesh_z(&f.clip_mesh().tables) {
            sources.push(("clip mesh", lo, hi));
        }
        let sm = &f.clip_mesh().static_models;
        let lo = sm
            .iter()
            .map(|s| s.model.bounds_mid[2] - s.model.bounds_half[2])
            .reduce(f32::min);
        let hi = sm
            .iter()
            .map(|s| s.model.bounds_mid[2] + s.model.bounds_half[2])
            .reduce(f32::max);
        if let (Some(lo), Some(hi)) = (lo, hi) {
            sources.push(("static models", lo, hi));
        }
        sources.retain(|(_, lo, hi)| lo.is_finite() && hi.is_finite() && lo.abs() < 1.0e6);
        // (The world model's bounds only when nothing else is there: Black
        // Ops III's span +-65537 whatever the map.)
        let counted = |n: &str| n != "world model" || sources.len() == 1;
        let low = sources
            .iter()
            .filter(|s| counted(s.0))
            .map(|s| s.1)
            .reduce(f32::min);
        let high = sources
            .iter()
            .filter(|s| counted(s.0))
            .map(|s| s.2)
            .reduce(f32::max);
        let text: Vec<String> = sources
            .iter()
            .map(|(n, lo, hi)| format!("{n} {lo:.0}..{hi:.0}"))
            .collect();
        (text.join(", "), low, high)
    };
    let enabled = with_vm(world, |vm, _| {
        vm.dvars
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(KILL_PLANE_DVAR))
            .is_none_or(|(_, v)| v.trim() != "0")
    })
    .unwrap_or(true);
    let planned = kill_plane_z(low, high, lowest_spawn);
    let kill_z = planned.filter(|_| enabled).map(|(z, _)| z);
    match planned {
        Some((z, margin)) => diag::info!(
            Sim,
            "bo2mp death barriers {map}: fall kill plane z {z:.0}{} (lowest collision {:.0}, \
             margin {margin:.0}; lowest of {spawns} spawn points {}; from {sources})",
            if enabled {
                ""
            } else {
                " OFF (dvar bo2mp_killplane 0)"
            },
            low.unwrap_or(f32::NAN),
            lowest_spawn.map_or("none".to_owned(), |z| format!("{z:.0}")),
        ),
        None => diag::warn!(
            Sim,
            "bo2mp death barriers {map}: no fall kill plane (no collision bounds: {sources})"
        ),
    }
    let mut b = world.resource_mut::<Barriers>();
    b.map = map;
    b.planned = true;
    b.kill_z = kill_z;
    b.lowest_spawn = lowest_spawn;
}

/// bo2mp: the fall-through detector. A player in the air more than
/// [`FALL_REPORT_DEPTH`] under the map's lowest spawn point is logged once
/// per fall: where, where he last stood, and what lies under his feet for a
/// player (`MASK_PLAYERSOLID`) and for any trace (a floor there that only
/// bullets or vehicles meet is collision filtered out for players).
fn detect_fall_through(world: &mut World, players: &[u32]) {
    let Some(spawn_z) = world.resource::<Barriers>().lowest_spawn else {
        return;
    };
    let map = world.resource::<Barriers>().map.clone();
    for &client in players {
        let Some((origin, ground, fall_speed)) = frame(world)
            .player(ClientId(client))
            .map(|ps| (ps.origin, ps.ground_entity_num, ps.velocity[2]))
        else {
            continue;
        };
        if ground != playerstate_iw4::ENTITYNUM_NONE {
            let mut b = world.resource_mut::<Barriers>();
            b.last_ground.insert(client, origin);
            b.fall_logged.remove(&client);
            continue;
        }
        if origin[2] >= spawn_z - FALL_REPORT_DEPTH
            || world.resource::<Barriers>().fall_logged.contains(&client)
        {
            continue;
        }
        world.resource_mut::<Barriers>().fall_logged.insert(client);
        let last = world.resource::<Barriers>().last_ground.get(&client).copied();
        let (mins, maxs) = ([-15.0, -15.0, 0.0], [15.0, 15.0, 70.0]);
        let mut under = |mask: u32, from: [f32; 3]| {
            let t = frame(world).trace_static_world(
                from,
                [from[0], from[1], from[2] - 65536.0],
                mins,
                maxs,
                mask,
            );
            if t.fraction >= 1.0 {
                "nothing".to_owned()
            } else {
                format!(
                    "z {:.0} contents {:#x} surface {:#x}",
                    t.endpos[2], t.contents, t.surface_flags
                )
            }
        };
        let solid = crate::bullet_collision::MASK_PLAYER_SOLID;
        let any = !0x4000_0000u32;
        // Over where he last stood: the floor he went through, if any.
        let at_last = match last {
            Some(l) => {
                let from = [l[0], l[1], l[2] + 4.0];
                let (p, a) = (under(solid, from), under(any, from));
                format!(
                    "({:.0} {:.0} {:.0}): under it for a player {p}, for any trace {a}",
                    l[0], l[1], l[2]
                )
            }
            None => "none".to_owned(),
        };
        let o = origin;
        let (feet_player, feet_any) = (under(solid, o), under(any, o));
        diag::warn!(
            Sim,
            "collision fall-through {map}: client {client} at ({:.0} {:.0} {:.0}) falling {:.0} \
             is {:.0} under the lowest spawn point ({spawn_z:.0}); under his feet for a player \
             {}, for any trace {}; last on the ground at {at_last}",
            o[0],
            o[1],
            o[2],
            fall_speed,
            spawn_z - o[2],
            feet_player,
            feet_any
        );
    }
}

fn is_spawn_point(classname: &str) -> bool {
    (classname.starts_with("mp_") && classname.contains("_spawn"))
        || classname.starts_with("info_player")
}

fn oob_name(s: &str) -> bool {
    let s = s.to_ascii_lowercase();
    s.contains("out_of_bounds")
        || s.contains("outofbounds")
        || s == "oob"
        || s.starts_with("oob_")
        || s.ends_with("_oob")
        || s.contains("kill_trigger")
        || s.contains("death_trigger")
}

/// Describe the map's barrier triggers once.
fn report(world: &mut World, list: &[(u32, Ent, Barrier)]) {
    let map = world.resource::<Barriers>().map.clone();
    let first = !world.resource::<Barriers>().summarized;
    {
        let b = world.resource::<Barriers>();
        if !first
            && list
                .iter()
                .all(|(n, _, _)| b.reported.contains(&(map.clone(), *n)))
        {
            return;
        }
    }
    world.resource_mut::<Barriers>().summarized = true;
    let named: Vec<(u32, String)> = if !first {
        Vec::new()
    } else {
        with_vm(world, |vm, world| {
            world
                .resource::<Zm>()
                .ents
                .iter()
                .filter(|(_, e)| e.map && triggers::kind(&e.classname).is_some())
                .filter_map(|(n, e)| {
                    let o = e.obj?;
                    let names = [
                        text_key(vm, o, "targetname"),
                        text_key(vm, o, "script_noteworthy"),
                    ];
                    names
                        .iter()
                        .any(|s| oob_name(s))
                        .then(|| (*n, format!("{} {:?}", e.classname, names)))
                })
                .collect()
        })
        .unwrap_or_default()
    };
    let mut lines = Vec::new();
    for (n, e, k) in list {
        if !world
            .resource_mut::<Barriers>()
            .reported
            .insert((map.clone(), *n))
        {
            continue;
        }
        let (dmg, flags, noteworthy) = with_vm(world, |vm, _| {
            let o = e.obj.unwrap();
            (
                int_key(vm, o, "dmg"),
                int_key(vm, o, "spawnflags"),
                text_key(vm, o, "script_noteworthy"),
            )
        })
        .unwrap_or_default();
        let volume = if let Some(b) = triggers::brush_index(e) {
            format!("brush *{b}")
        } else if let Some(h) = triggers::trigger_hull_index(e) {
            format!("trigger hulls ?{h}")
        } else if e.radius > 0.0 {
            format!("cylinder r{} h{}", e.radius, e.height)
        } else {
            "no volume".to_owned()
        };
        let centre = triggers::center(world, e);
        lines.push(format!(
            "#{n} {} {:?} dmg {} spawnflags {flags} {volume} at ({:.0} {:.0} {:.0}){}",
            e.classname,
            k,
            if *k == Barrier::OutOfBounds {
                super::oob::OOB_DAMAGE_PER_INTERVAL
            } else {
                hurt_damage(dmg)
            },
            centre[0],
            centre[1],
            centre[2],
            if noteworthy.is_empty() {
                String::new()
            } else {
                format!(" [{noteworthy}]")
            },
        ));
    }
    if first || !lines.is_empty() {
        diag::info!(
            Sim,
            "bo2mp death barriers {map}: {} hurt/oob triggers: {}; out-of-bounds-named script triggers \
             (left to the scripts): {:?}",
            lines.len(),
            lines.join("; "),
            named
        );
    }
}

/// Hurt the players in the map's hurt triggers; kill those below the
/// fall kill plane. Run each authority frame after the triggers.
pub(super) fn advance(world: &mut World, now: i64) {
    if !world.contains_resource::<Barriers>() {
        world.insert_resource(Barriers::default());
    }
    if !world.resource::<Zm>().started {
        return;
    }
    let map = world.resource::<Zm>().map.clone();
    if !world.resource::<Barriers>().planned || world.resource::<Barriers>().map != map {
        *world.resource_mut::<Barriers>() = Barriers::default();
        world.resource_mut::<Barriers>().map = map;
        plan(world);
    }
    let started_at = *world.resource_mut::<Barriers>().started_at.get_or_insert(now);
    let foreign_zombies = {
        let zm = world.resource::<Zm>();
        !zm.mp && super::foreign_zm::is_foreign_zm_map(&zm.map)
    };
    let list: Vec<(u32, Ent, Barrier)> = world
        .resource::<Zm>()
        .ents
        .iter()
        .filter_map(|(n, e)| barrier(&e.classname).map(|k| (*n, e.clone(), k)))
        .filter(|(_, e, _)| e.obj.is_some() && !e.trigger_off)
        .collect();
    report(world, &list);
    let players: Vec<u32> = world
        .resource::<Zm>()
        .players
        .iter()
        .filter(|(_, p)| p.sessionstate == "playing")
        .map(|(c, _)| *c)
        .collect();
    detect_fall_through(world, &players);
    let kill_z = world.resource::<Barriers>().kill_z;
    // (client, trigger or none, damage, spawnflags)
    let mut hits: Vec<(u32, Option<(u32, ObjRef)>, i32, i32)> = Vec::new();
    // (client, the out-of-bounds volume he is in)
    let mut out_of_bounds = BTreeMap::new();
    for &client in &players {
        let Some((mins, maxs)) = triggers::player_box(world, client) else {
            continue;
        };
        let case = BarrierCase {
            foreign_zombies,
            since_spawn_ms: since_spawn(world, client),
            since_start_ms: now - started_at,
            volume_holds_spawn: false,
        };
        if let Some(z) = kill_z
            && mins[2] < z
        {
            if let Some(why) = barrier_spares(BarrierHit::KillPlane, case) {
                say_spared(world, client, KILL_PLANE_KEY, "the kill plane", why);
                continue;
            }
            let mut b = world.resource_mut::<Barriers>();
            if b.next_hurt
                .get(&(KILL_PLANE_KEY, client))
                .is_none_or(|t| *t <= now)
            {
                b.next_hurt
                    .insert((KILL_PLANE_KEY, client), now + HURT_SLOW_INTERVAL_MS / 2);
                hits.push((client, None, KILL_DAMAGE, 0));
            }
            continue;
        }
        for (n, e, k) in &list {
            if e.invisible_to.contains(&client) || e.invisible_to_all {
                continue;
            }
            if !triggers::touches(world, e, mins, maxs) {
                continue;
            }
            let hit = if *k == Barrier::OutOfBounds { BarrierHit::OutOfBounds } else { BarrierHit::Hurt };
            // (Whether the volume holds a spawn only matters when the rest
            // of the case would not already spare him.)
            let spared = barrier_spares(hit, case).or_else(|| {
                let maybe = BarrierCase { volume_holds_spawn: true, ..case };
                if barrier_spares(hit, maybe).is_some() && volume_holds_spawn(world, *n, e) {
                    barrier_spares(hit, maybe)
                } else {
                    None
                }
            });
            if let Some(why) = spared {
                say_spared(world, client, *n, &format!("{} #{n}", e.classname), why);
                continue;
            }
            if *k == Barrier::OutOfBounds {
                if !out_of_bounds.contains_key(&client) {
                    let b = triggers::volume_bounds(world, e).map_or("no bounds".to_owned(), |(lo, hi)| {
                        format!("{} ({:.0} {:.0} {:.0})..({:.0} {:.0} {:.0})", e.model, lo[0], lo[1], lo[2], hi[0], hi[1], hi[2])
                    });
                    out_of_bounds.insert(client, (*n, b));
                }
                continue;
            }
            let obj = e.obj.unwrap();
            let (dmg, flags) = with_vm(world, |vm, _| {
                (int_key(vm, obj, "dmg"), int_key(vm, obj, "spawnflags"))
            })
            .unwrap_or_default();
            if HURT_HONOUR_START_OFF && *k == Barrier::Hurt && flags & HURT_SPAWNFLAG_START_OFF != 0
            {
                continue;
            }
            let mut b = world.resource_mut::<Barriers>();
            if b.next_hurt.get(&(*n, client)).is_some_and(|t| *t > now) {
                continue;
            }
            b.next_hurt.insert((*n, client), now + hurt_interval(flags));
            hits.push((client, Some((*n, obj)), hurt_damage(dmg), flags));
        }
    }
    // Black Ops III's out-of-bounds: the countdown screen, then its damage.
    let mut oob = std::mem::take(&mut world.resource_mut::<Barriers>().oob);
    for (client, amount) in super::oob::advance(world, &mut oob, now, &players, &out_of_bounds) {
        hits.push((client, None, amount, -1));
    }
    world.resource_mut::<Barriers>().oob = oob;
    // Forget touches long past (round restarts renumber entities).
    world
        .resource_mut::<Barriers>()
        .next_hurt
        .retain(|_, t| *t + HURT_SLOW_INTERVAL_MS > now);
    if hits.is_empty() {
        return;
    }
    let log = std::env::var_os("BO2MP_HITLOG").is_some();
    let mp = world.resource::<Zm>().mp;
    with_vm(world, |vm, world| {
        let np = vm.intern("idflags_no_protection");
        let no_protection = match vm.raw_field(vm.level, np) {
            Value::Int(i) => i,
            _ => 0,
        };
        for (client, trig, amount, flags) in hits {
            let point = frame(world)
                .player(ClientId(client))
                .map_or([0.0; 3], |ps| ps.origin);
            let oob = trig.is_none() && flags == -1;
            let engine_kill = trig.is_none();
            let idflags = damage_idflags(mp, if oob { 0 } else { flags }, engine_kill, no_protection);
            let who = match trig {
                Some((n, o)) if vm.alive(o) => {
                    if log || amount >= HURT_LOG_DAMAGE {
                        diag::info!(
                            Sim,
                            "bo2mp death barrier: trigger #{n} hurts player {client} for {amount} at ({:.0} {:.0} {:.0})",
                            point[0],
                            point[1],
                            point[2]
                        );
                    }
                    Value::Object(o)
                }
                Some(_) => continue,
                None if oob => {
                    diag::info!(
                        Sim,
                        "bo2mp out of bounds (BO3): player {client} out of time, {amount} damage at ({:.0} {:.0} {:.0})",
                        point[0],
                        point[1],
                        point[2]
                    );
                    Value::Undefined
                }
                None => {
                    diag::info!(
                        Sim,
                        "bo2mp death barrier: player {client} fell below the kill plane at ({:.0} {:.0} {:.0})",
                        point[0],
                        point[1],
                        point[2]
                    );
                    Value::Undefined
                }
            };
            let weapon = "none";
            natives_ai::player_damage(
                vm,
                world,
                client,
                who.clone(),
                who,
                amount,
                idflags,
                "MOD_TRIGGER_HURT",
                weapon,
                point,
                [0.0; 3],
                "none",
            );
        }
    });
}

/// The `idflags` a barrier's hit goes to the damage callback with. In
/// multiplayer never `level.idflags_no_protection`: Black Ops II's
/// `callback_playerdamage` (maps/mp/gametypes/_globallogic_player, read off
/// its bytecode) skips the whole damage block, `finishplayerdamagewrapper`
/// included, for a hit carrying it, so the fall kill plane and the
/// out-of-bounds timer never killed anyone. Zombies keep it (the engine's
/// kill plane, NO_PROTECTION triggers).
pub(crate) fn damage_idflags(mp: bool, trigger_spawnflags: i32, engine_kill: bool, no_protection: i32) -> i32 {
    if mp {
        0
    } else if trigger_spawnflags & HURT_SPAWNFLAG_NO_PROTECTION != 0 || engine_kill {
        no_protection
    } else {
        0
    }
}

/// A death (or going down) this soon after spawning is logged with its
/// cause and place.
pub(crate) const EARLY_DEATH_MS: i64 = 30_000;

/// Where and when each player last spawned (the early-death log).
#[derive(Resource, Default)]
pub(crate) struct SpawnLog(BTreeMap<u32, (u32, [f32; 3])>);

/// The `spawn` native: remember when and where.
pub(crate) fn spawned(world: &mut World, client: u32, tick: crate::Tick, at: [f32; 3]) {
    if !world.contains_resource::<SpawnLog>() {
        world.insert_resource(SpawnLog::default());
    }
    world.resource_mut::<SpawnLog>().0.insert(client, (tick.0, at));
}

/// How long after a spawn at tick `spawned` tick `now` is (ms).
pub(crate) fn since_spawn_ms(spawned: u32, now: u32) -> i64 {
    i64::from(now.saturating_sub(spawned)) * crate::MATCH_TICK_MS as i64
}

/// A lethal hit (death or last stand) on `client`: when it lands within
/// [`EARLY_DEATH_MS`] of his spawn, say what did it, where he is, where
/// he spawned and whether any zone volume (`info_volume`) holds him.
pub(crate) fn lethal_hit(
    world: &mut World,
    client: u32,
    outcome: &str,
    amount: i32,
    means: &str,
    weapon: &str,
    attacker: Option<u32>,
) {
    let now = super::tick(world).0;
    let Some((at_tick, spawn_at)) = world
        .get_resource::<SpawnLog>()
        .and_then(|l| l.0.get(&client).copied())
    else {
        return;
    };
    let ms = since_spawn_ms(at_tick, now);
    if ms > EARLY_DEATH_MS {
        return;
    }
    let pos = super::frame(world)
        .player(ClientId(client))
        .map_or([0.0; 3], |ps| ps.origin);
    let zones: Vec<(u32, Ent)> = world
        .resource::<Zm>()
        .ents
        .iter()
        .filter(|(_, e)| e.classname == "info_volume")
        .map(|(n, e)| (*n, e.clone()))
        .collect();
    let zone_text = if zones.is_empty() {
        "no zone volumes".to_owned()
    } else {
        let box_ = triggers::player_box(world, client);
        let inside = box_.map_or(0, |(lo, hi)| {
            zones
                .iter()
                .filter(|(_, e)| triggers::touches(world, e, lo, hi))
                .count()
        });
        format!("in {inside} of {} zone volumes", zones.len())
    };
    diag::warn!(
        Sim,
        "bo2mp early death: player {client} {outcome} {:.1}s after spawning: {means} {weapon} for {amount} by {} at ({:.0} {:.0} {:.0}), spawned at ({:.0} {:.0} {:.0}); {zone_text}",
        ms as f32 / 1000.0,
        attacker.map_or("the world".to_owned(), |a| if a == client {
            "himself".to_owned()
        } else {
            format!("entity {a}")
        }),
        pos[0],
        pos[1],
        pos[2],
        spawn_at[0],
        spawn_at[1],
        spawn_at[2],
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn out_of_bounds_never_counts_at_spawn_or_match_start() {
        let late = BarrierCase {
            foreign_zombies: false,
            since_spawn_ms: Some(60_000),
            since_start_ms: 120_000,
            volume_holds_spawn: false,
        };
        assert_eq!(barrier_spares(BarrierHit::OutOfBounds, late), None);
        let at_spawn = BarrierCase { volume_holds_spawn: true, ..late };
        assert_eq!(barrier_spares(BarrierHit::OutOfBounds, at_spawn), Some("spawn area"));
        let fresh = BarrierCase { since_spawn_ms: Some(SPAWN_GRACE_MS - 50), ..late };
        assert_eq!(barrier_spares(BarrierHit::OutOfBounds, fresh), Some("spawn grace"));
        let opening = BarrierCase { since_start_ms: 0, ..late };
        assert_eq!(barrier_spares(BarrierHit::OutOfBounds, opening), Some("match start grace"));
        // Zombies on a BO3 map: the same.
        let zm = BarrierCase { foreign_zombies: true, since_spawn_ms: Some(0), since_start_ms: 0, ..late };
        assert!(barrier_spares(BarrierHit::OutOfBounds, zm).is_some());
        // Unknown spawn time: only the other tests.
        let unknown = BarrierCase { since_spawn_ms: None, ..late };
        assert_eq!(barrier_spares(BarrierHit::OutOfBounds, unknown), None);
    }

    #[test]
    fn foreign_zombies_spawn_is_never_killed_by_a_barrier() {
        let spawn = BarrierCase {
            foreign_zombies: true,
            since_spawn_ms: Some(0),
            since_start_ms: 0,
            volume_holds_spawn: false,
        };
        for hit in [BarrierHit::Hurt, BarrierHit::OutOfBounds, BarrierHit::KillPlane] {
            assert!(barrier_spares(hit, spawn).is_some(), "{hit:?}");
        }
        // A hurt trigger over a spawn point never hurts there, even later.
        let later_on_spawn = BarrierCase {
            since_spawn_ms: Some(60_000),
            since_start_ms: 600_000,
            volume_holds_spawn: true,
            ..spawn
        };
        assert_eq!(barrier_spares(BarrierHit::Hurt, later_on_spawn), Some("spawn area"));
        // Later, away from any spawn: falling off still kills.
        let later = BarrierCase { volume_holds_spawn: false, ..later_on_spawn };
        assert_eq!(barrier_spares(BarrierHit::Hurt, later), None);
        assert_eq!(barrier_spares(BarrierHit::KillPlane, later), None);
        assert_eq!(barrier_spares(BarrierHit::OutOfBounds, later), None);
    }

    #[test]
    fn black_ops_ii_barriers_unchanged() {
        // Black Ops II's own maps (and multiplayer): a trigger_hurt and the
        // kill plane hurt at once, as BO2's hurt_touch does.
        let spawn = BarrierCase {
            foreign_zombies: false,
            since_spawn_ms: Some(0),
            since_start_ms: 0,
            volume_holds_spawn: true,
        };
        assert_eq!(barrier_spares(BarrierHit::Hurt, spawn), None);
        assert_eq!(barrier_spares(BarrierHit::KillPlane, spawn), None);
    }

    #[test]
    fn early_death_window_counts_ticks() {
        let per = crate::MATCH_TICK_MS as i64;
        assert_eq!(since_spawn_ms(10, 10), 0);
        assert_eq!(since_spawn_ms(10, 12), 2 * per);
        // A respawn record newer than the tick (a restart) is not negative.
        assert_eq!(since_spawn_ms(20, 10), 0);
        assert!(since_spawn_ms(0, (EARLY_DEATH_MS / per) as u32) <= EARLY_DEATH_MS);
    }

    #[test]
    fn hurt_defaults_follow_sp_trigger_hurt() {
        assert_eq!(hurt_damage(0), 5);
        assert_eq!(hurt_damage(1000), 1000);
        assert_eq!(hurt_interval(0), 50);
        assert_eq!(hurt_interval(HURT_SPAWNFLAG_SLOW), 1000);
        assert_eq!(
            hurt_interval(HURT_SPAWNFLAG_SLOW | HURT_SPAWNFLAG_NO_PROTECTION),
            1000
        );
    }

    #[test]
    fn kill_plane_sits_below_collision_and_spawns() {
        // A 2000-unit tall map: 512 margin (10% is 200).
        let (z, m) = kill_plane_z(Some(-100.0), Some(1900.0), Some(0.0)).unwrap();
        assert_eq!(m, 512.0);
        assert_eq!(z, -612.0);
        // A tall one: 10% of 12000.
        let (z, m) = kill_plane_z(Some(-4000.0), Some(8000.0), Some(2000.0)).unwrap();
        assert_eq!(m, 1200.0);
        assert_eq!(z, -5200.0);
        // Capped margin.
        let (_, m) = kill_plane_z(Some(0.0), Some(1.0e5), None).unwrap();
        assert_eq!(m, KILL_PLANE_MARGIN_MAX);
        // A spawn below the collision (bad data) still keeps the plane below it.
        let (z, _) = kill_plane_z(Some(0.0), Some(1000.0), Some(-300.0)).unwrap();
        assert!(z <= -300.0 - KILL_PLANE_MARGIN_MIN);
        assert!(kill_plane_z(None, Some(10.0), Some(0.0)).is_none());
    }

    #[test]
    fn brush_bottom_reads_axial_planes() {
        // A box 0..64 in each axis: -x<=0, x<=64, -y<=0, y<=64, -z<=0, z<=64,
        // shifted to z -128..-64.
        let planes = [
            [-1.0, 0.0, 0.0, 0.0],
            [1.0, 0.0, 0.0, 64.0],
            [0.0, -1.0, 0.0, 0.0],
            [0.0, 1.0, 0.0, 64.0],
            [0.0, 0.0, -1.0, 128.0],
            [0.0, 0.0, 1.0, -64.0],
        ];
        assert_eq!(brush_bottom(&planes), Some(-128.0));
        assert_eq!(brush_top(&planes), Some(-64.0));
    }

    #[test]
    fn mesh_floor_range_counts_player_solid_triangles_only() {
        // Two triangles in segment 1: a floor at z 10 (solid) and a bullet
        // only one (0x2080) at z -5000.
        let mut mesh = clipmap_iw4::ClipMeshTables::default();
        mesh.verts = vec![[0.0; 3]; clipmap_iw4::VERTS_PER_SEGMENT];
        mesh.verts.extend([
            [0.0, 0.0, 10.0],
            [64.0, 0.0, 10.0],
            [0.0, 64.0, 10.0],
            [0.0, 0.0, -5000.0],
            [64.0, 0.0, -5000.0],
            [0.0, 64.0, -5000.0],
        ]);
        mesh.tri_indices = vec![0, 1, 2, 3, 4, 5];
        mesh.tri_content_flags = vec![1, 0x2080];
        mesh.partitions = vec![clipmap_iw4::ClipPartition {
            tri_count: 2,
            first_tri: 0,
            first_vert_segment: 1,
            ..Default::default()
        }];
        assert_eq!(player_solid_mesh_z(&mesh), Some((10.0, 10.0)));
        mesh.tri_content_flags[1] = 0x1_0000;
        assert_eq!(player_solid_mesh_z(&mesh), Some((-5000.0, 10.0)));
    }

    #[test]
    fn multiplayer_barrier_hits_never_carry_no_protection() {
        // BO2's MP damage callback drops a no-protection hit entirely.
        assert_eq!(damage_idflags(true, HURT_SPAWNFLAG_NO_PROTECTION, false, 512), 0);
        assert_eq!(damage_idflags(true, 0, true, 512), 0);
        // Zombies keep the engine's flag.
        assert_eq!(damage_idflags(false, 0, true, 512), 512);
        assert_eq!(damage_idflags(false, HURT_SPAWNFLAG_NO_PROTECTION, false, 512), 512);
        assert_eq!(damage_idflags(false, 0, false, 512), 0);
    }

    #[test]
    fn barrier_classes() {
        assert_eq!(barrier("trigger_hurt"), Some(Barrier::Hurt));
        assert_eq!(barrier("trigger_out_of_bounds"), Some(Barrier::OutOfBounds));
        assert_eq!(barrier("trigger_radius_out_of_bounds"), Some(Barrier::OutOfBounds));
        assert_eq!(barrier("trigger_multiple"), None);
        assert!(oob_name("out_of_bounds_trig"));
        assert!(!oob_name("flag_trigger"));
    }
}
