//! bo2mp: Black Ops III's AI, the engine's half (`docs/T7_ZOMBIES_PLAN.md`,
//! milestone 4-5). Black Ops III's zombies think in a behaviour tree the
//! engine runs; the tree's services, conditions and actions are script
//! functions the archetype scripts register (`behaviortreenetworkutility::
//! registerbehaviortreescriptapi` / `registerbehaviortreeaction`, kept in
//! two level arrays by lower-case name). This module is that engine:
//! - spawning (`spawner spawnfromspawner(...)`): an actor with its
//!   archetype, then `callbacks_shared::codecallback_actorspawned(spawner)`
//!   (the spawn functions);
//! - the map's nav mesh (`nav_t7`, the zone's Havok packfiles, included by
//!   path: no new crate for the sim) with each traversal and window linked,
//!   and the nav builtins (`getclosestpointonnavmesh`, `ispointonnavmesh`,
//!   `setgoal`, `isatgoal`, ...);
//! - each tick, per zombie: the tree's services, then the first branch
//!   whose condition holds (enter the playable area, tear a board, melee,
//!   move), its action's start / update / terminate, and the walk along a
//!   nav mesh path to the goal the scripts set.
//!
//! The tree itself is written here from the registered names (the zone's
//! behaviour tree asset is not read yet), and walking is at a speed per
//! `zombie_move_speed` until the animation state machine's clips drive it.
//!
//! Not part of upstream IW4L.

use std::collections::HashMap;

use bevy_ecs::prelude::{Resource, World};
use gsc_t6::{ObjKind, ObjRef, Value, Vm};

use super::actors::Actor;
use super::{Ent, Zm, entnum, origin_of};

#[path = "../../../nav_t7/src/hkx.rs"]
#[allow(dead_code)]
mod hkx;
#[path = "../../../nav_t7/src/mesh.rs"]
#[allow(dead_code)]
pub(crate) mod mesh;

/// A behaviour tree action's "still running" (Black Ops III's
/// `BHTN_RUNNING`); the registered functions return it from start and
/// update while they go on.
const BHTN_RUNNING: i32 = 5;

/// The engine's per-zombie state beside `Actor`.
#[derive(Clone, Debug, Default)]
pub(crate) struct Ai {
    pub archetype: String,
    /// The running action: its registered name, when it started (ms) and
    /// how long it plays before its update is asked (ms).
    pub action: Option<(String, i64, i64)>,
    /// The planned path's goal and the waypoints left.
    pub planned: Option<[f32; 3]>,
    pub path: Vec<mesh::Waypoint>,
    pub planned_ms: i64,
    /// A melee swing: when it started and whether it has hit.
    pub melee: Option<(i64, bool)>,
    /// A traversal in progress: from, to, start ms, length ms.
    pub traverse: Option<([f32; 3], [f32; 3], i64, i64)>,
    /// The blackboard (`setblackboardattribute`).
    pub blackboard: HashMap<String, Value>,
    /// How far it walked this tick (its locomotion clip's pace).
    pub moved: f32,
    /// The running melee's length and hit time (ms).
    pub melee_ms: (i64, i64),
    /// When the running action's notetrack falls (ms in), from its clip.
    pub note_ms: Option<i64>,
}

#[derive(Resource, Default)]
pub(crate) struct T7Ai {
    pub nav: Option<mesh::NavMesh>,
    pub ai: HashMap<u32, Ai>,
    /// The living vehicles spawned from vehicle spawners (entity numbers).
    pub vehicles: std::collections::BTreeSet<u32>,
    /// Every vehicle spawned so (living or a wreck).
    pub spawned_vehicles: std::collections::BTreeSet<u32>,
    /// A vehicle's animation state (`asmrequestsubstate`): the substate and
    /// when it began, and its turret's target (`setturrettargetent`).
    pub vehicle_asm: HashMap<u32, (String, i64, u8)>,
    pub turret_target: HashMap<u32, ObjRef>,
    /// Scripted animations playing (`animscripted`): when each ends, on
    /// whom, under which notify.
    pub anim_ends: Vec<(i64, ObjRef, String)>,
    /// The notetracks of those animations (its clip's, where loaded): when
    /// each falls, on whom, the notify and the note (`self waittill(notify,
    /// note)`: the scenes pass a note on by name, Zetsubou's power plants
    /// wait for theirs).
    pub anim_notes: Vec<(i64, ObjRef, String, String)>,
    /// A vehicle's goal anchor (`setgoal` on a parasite: its spawn point,
    /// radius 999999): what `positionquery_filter_distancetogoal` measures to.
    pub vehicle_goal: HashMap<u32, [f32; 3]>,
    /// The AI types' characters (`T6Install::t7_cast`), by AI type.
    pub cast: HashMap<String, Vec<(String, Vec<String>, Vec<String>)>>,
    /// The zombies' clips by kind (`clip_kinds`), from the match's
    /// animations, built on first use.
    pub clips: Option<HashMap<&'static str, Vec<String>>>,
}

/// When a vehicle's attack substate (`fire@stationary`) gives its notes:
/// "pre_fire" (the tell), the shot, then `asm_terminated`.
const PRE_FIRE_MS: i64 = 350;
const FIRE_MS: i64 = 600;
const FIRE_END_MS: i64 = 900;
/// A parasite's spit (its turret's projectile) on a hit.
const SPIT_DAMAGE: i32 = 40;

/// Each tick: the vehicles' attack substates. A shot hits the turret's
/// target if the vehicle sees it (its spit flies straight at it).
pub(crate) fn vehicle_tick(world: &mut World) {
    anim_tick(world);
    let Some(st) = world.get_resource::<T7Ai>() else { return };
    if st.vehicle_asm.is_empty() {
        return;
    }
    let now = world.resource::<Zm>().now_ms;
    let list: Vec<(u32, String, i64, u8)> = st.vehicle_asm.iter().map(|(n, (s, t, step))| (*n, s.clone(), *t, *step)).collect();
    super::with_vm(world, |vm, world| {
        for (n, sub, start, step) in list {
            let Some(obj) = world.resource::<Zm>().ents.get(&n).and_then(|e| e.obj) else {
                world.resource_mut::<T7Ai>().vehicle_asm.remove(&n);
                continue;
            };
            if !sub.starts_with("fire@") {
                continue;
            }
            let age = now - start;
            let mut next = step;
            if step == 0 && age >= PRE_FIRE_MS {
                vm.notify_str(world, obj, "pre_fire", &[]);
                next = 1;
            }
            if next == 1 && age >= FIRE_MS {
                let target = world.resource::<T7Ai>().turret_target.get(&n).copied();
                let from = world.resource::<Zm>().ents.get(&n).map(|e| e.origin);
                if let (Some(t), Some(from)) = (target, from)
                    && vm.alive(t)
                {
                    let to = super::origin_of(vm, world, &Value::Object(t));
                    let near = to.is_some_and(|to| (0..3).map(|i| (to[i] - from[i]).powi(2)).sum::<f32>() < 1200.0 * 1200.0);
                    if near && let Some(dd) = vm.native_of("dodamage", true) {
                        let means = vm.string("MOD_PROJECTILE");
                        let w = vm.string("parasite_projectile");
                        let none = vm.string("none");
                        let _ = dd(vm, world, &Value::Object(t), &[Value::Int(SPIT_DAMAGE), Value::Vec3(from), Value::Object(obj), Value::Object(obj), none, means, Value::Int(0), w]);
                    }
                }
                next = 2;
            }
            if next == 2 && age >= FIRE_END_MS {
                let s = vm.string(&sub);
                vm.notify_str(world, obj, "asm_terminated", &[s]);
                world.resource_mut::<T7Ai>().vehicle_asm.remove(&n);
                continue;
            }
            if let Some(e) = world.resource_mut::<T7Ai>().vehicle_asm.get_mut(&n) {
                e.2 = next;
            }
        }
    });
}

// ---- the nav mesh ---------------------------------------------------------

/// The map's entities as key maps (the entity string's blocks).
fn parse_ents(text: &str) -> Vec<HashMap<String, String>> {
    let mut out = Vec::new();
    for block in text.split('{').skip(1) {
        let body = block.split('}').next().unwrap_or("");
        let mut m = HashMap::new();
        let mut q = body.split('"').skip(1).step_by(2);
        while let (Some(k), Some(v)) = (q.next(), q.next()) {
            m.insert(k.to_ascii_lowercase(), v.to_owned());
        }
        out.push(m);
    }
    out
}

fn vec3_of(s: &str) -> Option<[f32; 3]> {
    let v: Vec<f32> = s.split_whitespace().filter_map(|x| x.parse().ok()).collect();
    (v.len() == 3).then(|| [v[0], v[1], v[2]])
}

/// Read the nav mesh from the map zone's packfiles and link the map's
/// traversals (`node_negotiation_begin` -> its target) and windows (the
/// outside goal in front of the boards through the barrier).
pub(super) fn install(world: &mut World, image: &[u8], entities: &[String], cast: Vec<super::T7Cast>) {
    let mut ai = T7Ai::default();
    ai.cast = cast.into_iter().collect();
    match mesh::NavMesh::from_image(image) {
        Ok(Some(mut nav)) => {
            let ents: Vec<HashMap<String, String>> = entities.iter().flat_map(|t| parse_ents(t)).collect();
            let named = |n: &str| ents.iter().filter(|e| e.get("targetname").is_some_and(|t| t == n)).collect::<Vec<_>>();
            let (mut links, mut traversals) = (0, 0);
            for b in ents.iter().filter(|e| e.get("classname").is_some_and(|c| c == "node_negotiation_begin")) {
                let end = b.get("target").map_or_else(Vec::new, |t| named(t)).first().and_then(|e| e.get("origin")).and_then(|o| vec3_of(o));
                if let (Some(o), Some(e)) = (b.get("origin").and_then(|o| vec3_of(o)), end) {
                    traversals += 1;
                    let kind = b.get("animscript").map_or("traverse", String::as_str);
                    if nav.link_traversal(o, e, kind).is_some() {
                        links += 1;
                    }
                }
            }
            let mut windows = (0, 0);
            for g in named("exterior_goal") {
                let barrier = g
                    .get("target")
                    .map_or_else(Vec::new, |t| named(t))
                    .into_iter()
                    .find(|e| e.get("classname").is_some_and(|c| c.starts_with("zbarrier")))
                    .and_then(|e| e.get("origin"))
                    .and_then(|o| vec3_of(o));
                let (Some(o), Some(b)) = (g.get("origin").and_then(|o| vec3_of(o)), barrier) else { continue };
                windows.0 += 1;
                let d = [b[0] - o[0], b[1] - o[1]];
                let l = d[0].hypot(d[1]).max(1.0);
                let inside = [b[0] + d[0] / l * 48.0, b[1] + d[1] / l * 48.0, o[2]];
                if nav.link_traversal(o, inside, "zbarrier").is_some() {
                    windows.1 += 1;
                }
            }
            diag::info!(
                Sim,
                "bo2mp t7 nav mesh: {} faces, {} edges; {links} of {traversals} traversals and {} of {} windows linked",
                nav.faces.len(),
                nav.edges.len(),
                windows.1,
                windows.0
            );
            ai.nav = Some(nav);
        }
        Ok(None) => diag::warn!(Sim, "bo2mp t7 nav mesh: none in the map's packfiles; zombies stand where they spawn"),
        Err(e) => diag::warn!(Sim, "bo2mp t7 nav mesh: not read ({e}); zombies stand where they spawn"),
    }
    world.insert_resource(ai);
}

/// The closest point on the nav mesh within `radius` of `p` (on the face
/// below or above it first, else the nearest face's closest point).
pub(crate) fn closest_point(nav: &mesh::NavMesh, p: [f32; 3], radius: f32) -> Option<[f32; 3]> {
    if let Some(f) = nav.locate(p, radius.max(36.0), radius.max(36.0)) {
        return Some([p[0], p[1], nav.height(f, p[0], p[1])]);
    }
    let f = nav.nearest(p, radius)?;
    let poly = nav.polygon(f);
    let mut best: Option<([f32; 3], f32)> = None;
    for i in 0..poly.len() {
        let (a, b) = (poly[i], poly[(i + 1) % poly.len()]);
        let ab = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
        let len2 = ab[0] * ab[0] + ab[1] * ab[1] + ab[2] * ab[2];
        let t = if len2 > 0.0 { (((p[0] - a[0]) * ab[0] + (p[1] - a[1]) * ab[1] + (p[2] - a[2]) * ab[2]) / len2).clamp(0.0, 1.0) } else { 0.0 };
        let q = [a[0] + ab[0] * t, a[1] + ab[1] * t, a[2] + ab[2] * t];
        let d = (q[0] - p[0]).powi(2) + (q[1] - p[1]).powi(2) + (q[2] - p[2]).powi(2);
        if best.is_none_or(|(_, bd)| d < bd) {
            best = Some((q, d));
        }
    }
    best.filter(|(_, d)| d.sqrt() <= radius).map(|(q, _)| q)
}

// ---- spawning -------------------------------------------------------------

/// A spawner's archetype by its AI type (the zone's AI type assets are not
/// read: its class name tells).
fn archetype_of(classname: &str) -> &'static str {
    let c = classname.to_ascii_lowercase();
    if c.contains("margwa") {
        "margwa"
    } else if c.contains("parasite") {
        "parasite"
    } else if c.contains("raps") {
        "raps"
    } else if c.contains("apothicon_fury") {
        "apothicon_fury"
    } else if c.contains("mechz") {
        "mechz"
    } else {
        "zombie"
    }
}

/// `spawner spawnfromspawner([targetname | "actor_<aitype>"], [force], ...)`.
fn spawn_from_spawner(vm: &mut Vm<World>, world: &mut World, spawner: ObjRef, a: &[Value]) -> Option<ObjRef> {
    let sn = match vm.kind(spawner)? {
        ObjKind::Entity(n) => n,
        _ => return None,
    };
    let sp = world.resource::<Zm>().ents.get(&sn).cloned()?;
    let first = a.first().map(|v| vm.to_text(v)).unwrap_or_else(String::new);
    let classname = if first.starts_with("actor_") { first.clone() } else { sp.classname.clone() };
    // The spawner's count (2147483647: no limit).
    let count_s = vm.intern("count");
    let count = vm.raw_field(spawner, count_s).as_int().unwrap_or(1);
    if count <= 0 {
        return None;
    }
    if count < i32::MAX {
        vm.set_raw_field(spawner, count_s, Value::Int(count - 1));
    }
    // A vehicle spawner (`script_vehicle` with `SPAWNER`: the parasites,
    // the Meatballs, the sword's glaive) makes a vehicle.
    if sp.classname == "script_vehicle" {
        return spawn_vehicle(vm, world, spawner, &sp, count_s, a);
    }
    let n = world.resource_mut::<Zm>().alloc_entnum();
    let obj = vm.alloc_object(ObjKind::Entity(n));
    // The spawner's script fields come along (its keys, and `spawn_funcs`:
    // `spawner_shared`'s spawn think reads them on the AI), but its count.
    for f in vm.field_names(spawner) {
        if f == count_s {
            continue;
        }
        let v = vm.raw_field(spawner, f);
        vm.set_raw_field(obj, f, v);
    }
    let targetname = if first.is_empty() || first.starts_with("actor_") {
        let t = vm.intern("targetname");
        let base = vm.to_text(&vm.raw_field(spawner, t));
        (!base.is_empty()).then(|| format!("{base}_ai"))
    } else {
        Some(first)
    };
    if let Some(t) = targetname {
        let f = vm.intern("targetname");
        let v = vm.string(&t);
        vm.set_raw_field(obj, f, v);
    }
    let archetype = archetype_of(&classname);
    for (k, v) in [("archetype", vm.string(archetype)), ("team", vm.string("axis")), ("classname", vm.string(&classname)), ("isdog", Value::Int(0))] {
        let f = vm.intern(k);
        vm.set_raw_field(obj, f, v);
    }
    let mut model = {
        let f = vm.intern("model");
        vm.to_text(&vm.raw_field(spawner, f))
    };
    // The engine dresses an actor as one of its AI type's characters
    // (`actor_spawner_zm_zod_zombie` spawns AI type `spawner_zm_zod_zombie`):
    // the character's body, one of its heads, its hat (the spawner's
    // `model` is only Radiant's preview, in no zone).
    let mut attached = Vec::new();
    let dressed = world.get_resource::<T7Ai>().and_then(|s| s.cast.get(classname.trim_start_matches("actor_")).filter(|c| !c.is_empty()).cloned());
    if let Some(chars) = dressed {
        let (body, heads, extra) = &chars[vm.rand_u32() as usize % chars.len()];
        model = body.clone();
        if !heads.is_empty() {
            let head = heads[vm.rand_u32() as usize % heads.len()].clone();
            let f = vm.intern("head");
            let v = vm.string(&head);
            vm.set_raw_field(obj, f, v);
            attached.push((head, String::new()));
        }
        if let Some(hat) = extra.first() {
            let f = vm.intern("hatmodel");
            let v = vm.string(hat);
            vm.set_raw_field(obj, f, v);
        }
        attached.extend(extra.iter().map(|m| (m.clone(), String::new())));
        let f = vm.intern("model");
        let v = vm.string(&model);
        vm.set_raw_field(obj, f, v);
    }
    world.resource_mut::<Zm>().ents.insert(
        n,
        Ent { obj: Some(obj), classname: classname.clone(), origin: sp.origin, angles: sp.angles, model, attached, solid: true, ..Default::default() },
    );
    world.resource_mut::<Zm>().actors.insert(
        n,
        Actor {
            obj: Some(obj),
            aitype: classname.trim_start_matches("actor_").to_owned(),
            alive: true,
            health: 100,
            maxhealth: 100,
            team: "axis".into(),
            radius: 15.0,
            height: 72.0,
            orientmode: "face default".into(),
            animmode: "normal".into(),
            t7: true,
            ..Default::default()
        },
    );
    if let Some(mut s) = world.get_resource_mut::<T7Ai>() {
        s.ai.insert(n, Ai { archetype: archetype.to_owned(), ..Default::default() });
    }
    // The engine's AI set-up before the spawn callback: its weapons (a
    // zombie's are all "none") and `init::main` (the `self.a` struct the
    // death and gib scripts write into: a trap's kill set `a.gib_ref` on
    // nothing and the zombie lived).
    let none = super::t7::weapon_object(vm, world, "none");
    for k in ["weapon", "primaryweapon", "secondaryweapon", "sidearm"] {
        let f = vm.intern(k);
        vm.set_raw_field(obj, f, none.clone());
    }
    vm.spawn_named(world, "scripts/shared/ai/systems/init", "main", Value::Object(obj), Vec::new());
    vm.spawn_named(world, super::t7::CALLBACKS, "codecallback_actorspawned", Value::Object(obj), vec![Value::Object(spawner)]);
    vm.alive(obj).then_some(obj)
}

/// A vehicle from its spawner: an entity with the spawner's fields (its
/// `vehicletype`, `script_*`), its health, then
/// `callbacks_shared::codecallback_vehiclespawned(spawner)` (its type's
/// spawn functions start its script state machine, which flies it with
/// `setvehgoalpos`).
fn spawn_vehicle(vm: &mut Vm<World>, world: &mut World, spawner: ObjRef, sp: &Ent, count_s: gsc_t6::Str, a: &[Value]) -> Option<ObjRef> {
    let n = world.resource_mut::<Zm>().alloc_entnum();
    let obj = vm.alloc_object(ObjKind::Entity(n));
    for f in vm.field_names(spawner) {
        if f == count_s {
            continue;
        }
        let v = vm.raw_field(spawner, f);
        vm.set_raw_field(obj, f, v);
    }
    let (spf, hf) = (vm.intern("spawner"), vm.intern("health"));
    vm.set_raw_field(obj, spf, Value::Undefined);
    // Its targetname: the one asked for (`zombie_wasp`, which the round
    // counts), else the spawner's with `_vh`.
    let tf = vm.intern("targetname");
    let first = a.first().map(|v| vm.to_text(v)).unwrap_or_else(String::new);
    let name = if first.is_empty() || first.starts_with("actor_") {
        let base = vm.to_text(&vm.raw_field(spawner, tf));
        (!base.is_empty()).then(|| format!("{base}_vh"))
    } else {
        Some(first)
    };
    let name = name.map_or(Value::Undefined, |n| vm.string(&n));
    vm.set_raw_field(obj, tf, name);
    if vm.raw_field(obj, hf).is_undefined() {
        vm.set_raw_field(obj, hf, Value::Int(100));
    }
    // Its settings bundle, as its vehicle def names it.
    let sbs = super::t7::field_name(vm, "scriptbundlesettings");
    if vm.raw_field(obj, sbs).is_undefined() {
        let vt = super::t7::field_name(vm, "vehicletype");
        let vt = vm.to_text(&vm.raw_field(obj, vt));
        if let Some(b) = world.get_resource::<super::t7::T7State>().and_then(|st| st.settings_for(&vt)) {
            let v = vm.string(&b);
            vm.set_raw_field(obj, sbs, v);
        }
    }
    let model = {
        let f = vm.intern("model");
        vm.to_text(&vm.raw_field(spawner, f))
    };
    world.resource_mut::<Zm>().ents.insert(n, Ent { obj: Some(obj), classname: "script_vehicle".to_owned(), origin: sp.origin, angles: sp.angles, model, solid: true, ..Default::default() });
    if let Some(mut s) = world.get_resource_mut::<T7Ai>() {
        s.vehicles.insert(n);
        s.spawned_vehicles.insert(n);
    }
    // (The engine's goal for it: where it spawned, a default radius.)
    {
        let (fp, fr) = (super::t7::field_name(vm, "goalpos"), vm.intern("goalradius"));
        let at = world.resource::<Zm>().ents.get(&n).map_or([0.0; 3], |e| e.origin);
        vm.set_raw_field(obj, fp, Value::Vec3(at));
        vm.set_raw_field(obj, fr, Value::Float(128.0));
    }
    vm.spawn_named(world, super::t7::CALLBACKS, "codecallback_vehiclespawned", Value::Object(obj), vec![Value::Object(spawner)]);
    vm.alive(obj).then_some(obj)
}

/// A hit on a Black Ops III vehicle: `codecallback_vehicledamage` with its
/// 15 arguments (the weapon an object); its scripts call
/// `finishvehicledamage`.
#[allow(clippy::too_many_arguments)]
pub(crate) fn vehicle_damage(vm: &mut Vm<World>, world: &mut World, obj: ObjRef, inflictor: Value, attacker: Value, amount: i32, means: &str, weapon: &str, point: [f32; 3], dir: [f32; 3]) {
    let means = vm.string(means);
    let weapon = vm.string(weapon);
    let weapon = super::t7::weapon_value(vm, world, weapon);
    let (none, empty) = (vm.string("none"), vm.string(""));
    let args = vec![inflictor, attacker, Value::Int(amount), Value::Int(0), means, weapon, Value::Vec3(point), Value::Vec3(dir), none, Value::Vec3(point), Value::Int(0), Value::Int(0), Value::Int(0), empty, Value::Vec3([0.0, 0.0, 1.0])];
    vm.spawn_named(world, super::t7::CALLBACKS, "codecallback_vehicledamage", Value::Object(obj), args);
}

/// `self finishvehicledamage(inflictor, attacker, damage, flags, means,
/// weapon, point, dir, ...)`: the health goes down, "damage" goes out;
/// at none, `codecallback_vehiclekilled` and "death".
fn finish_vehicle_damage(vm: &mut Vm<World>, world: &mut World, s: &Value, a: &[Value]) -> Result<Value, String> {
    let Value::Object(obj) = s else { return Ok(Value::Undefined) };
    let obj = *obj;
    let amount = a.get(2).and_then(Value::as_int).unwrap_or(0).max(0);
    let hf = vm.intern("health");
    let health = vm.raw_field(obj, hf).as_int().unwrap_or(100) - amount;
    vm.set_raw_field(obj, hf, Value::Int(health));
    let arg = |i: usize| a.get(i).cloned().unwrap_or(Value::Undefined);
    let weapon = super::t7::weapon_value(vm, world, arg(5));
    let empty = vm.string("");
    vm.notify_str(world, obj, "damage", &[Value::Int(amount), arg(1), arg(7), arg(6), arg(4), empty.clone(), empty.clone(), empty, weapon.clone()]);
    if health <= 0 && vm.alive(obj) {
        let n = super::entnum(vm, s);
        if let (Some(n), Some(mut st)) = (n, world.get_resource_mut::<T7Ai>()) {
            if !st.vehicles.remove(&n) {
                return Ok(Value::Undefined);
            }
        }
        let killed = vec![arg(0), arg(1), Value::Int(amount), arg(4), weapon, arg(7), arg(8), Value::Int(0)];
        vm.spawn_named(world, super::t7::CALLBACKS, "codecallback_vehiclekilled", Value::Object(obj), killed);
        if vm.alive(obj) {
            vm.notify_str(world, obj, "death", &[arg(1)]);
        }
    }
    Ok(Value::Undefined)
}

// ---- the behaviour tree ---------------------------------------------------

/// A registered script function (`registerbehaviortreescriptapi`) by
/// lower-case name.
fn api(vm: &mut Vm<World>, name: &str) -> Option<Value> {
    let table = vm.intern("#d59a4e7a");
    let Value::Array(a) = vm.raw_field(vm.level, table) else { return None };
    let k = vm.string(name);
    let key = vm.to_key(&k).ok()?;
    a.snapshot().get(&key).cloned()
}

/// A registered action's (start, update, terminate) functions.
fn action_fns(vm: &mut Vm<World>, name: &str) -> Option<[Value; 3]> {
    let table = vm.intern("#5e7deba7");
    let Value::Array(a) = vm.raw_field(vm.level, table) else { return None };
    let k = vm.string(name);
    let key = vm.to_key(&k).ok()?;
    let Some(Value::Array(fns)) = a.snapshot().get(&key).cloned() else { return None };
    let fns = fns.snapshot();
    let mut out: [Value; 3] = Default::default();
    for (i, part) in ["bhtn_action_start", "bhtn_action_update", "bhtn_action_terminate"].iter().enumerate() {
        let k = vm.string(part);
        if let Ok(key) = vm.to_key(&k) {
            out[i] = fns.get(&key).cloned().unwrap_or(Value::Undefined);
        }
    }
    Some(out)
}

/// Ask a registered condition or service about the zombie (it is both
/// the argument and `self`, as the engine calls them).
fn ask(vm: &mut Vm<World>, world: &mut World, name: &str, obj: ObjRef) -> bool {
    let Some(f) = api(vm, name) else { return false };
    gsc_t6::truthy(&vm.call_value_now(world, &f, Value::Object(obj), vec![Value::Object(obj)]))
}

/// Run one part of a registered action; its status (running unless it
/// says otherwise).
fn act(vm: &mut Vm<World>, world: &mut World, name: &str, part: usize, obj: ObjRef) -> i32 {
    let Some(fns) = action_fns(vm, name) else { return 4 };
    if fns[part].is_undefined() {
        return if part == 1 { 4 } else { BHTN_RUNNING };
    }
    let state = vm.string(name);
    let r = vm.call_value_now(world, &fns[part], Value::Object(obj), vec![Value::Object(obj), state]);
    r.as_int().unwrap_or(BHTN_RUNNING)
}

/// `CODLEGACY_T7_AI_TRACE=1`: each zombie's branch choices (once per change).
fn trace_on() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| codlegacy_env::var_os("CODLEGACY_T7_AI_TRACE").is_some())
}

/// The services the zombie tree runs every tick, in order.
/// The services the zombie tree runs every tick, in order. (The shared
/// zombie tree's `zombieTargetService` is not Zombies': its tree finds
/// flesh through the windows with `zombieFindFleshService`.)
const SERVICES: &[&str] = &[
    "zombiefindfleshservice",
    "zombieenteredplayableservice",
    "zombieattackableobjectservice",
    "zombietraversalservice",
    "zombiecrawlercollisionservice",
];

/// The services of the branch outside the playable area (the way to a
/// window and a spot at it).
const OUTSIDE_SERVICES: &[&str] = &["findnodesservice", "updateattackspotservice"];

/// How long an action's animation plays (ms; its clip, until the
/// animation state machine plays them), and the notetrack it fires
/// halfway (`animationstatenetwork::runnotetrackhandler`).
fn action_clip(name: &str) -> (i64, Option<&'static str>) {
    match name {
        "zombiemantleaction" => (1200, None),
        "grabboardaction" => (500, None),
        "holdboardaction" => (400, None),
        "pullboardaction" => (800, Some("destroy_piece")),
        "zombieattackthroughboardsaction" => (1500, Some("zombie_window_melee")),
        "zombietauntaction" => (2000, None),
        _ => (600, None),
    }
}

/// The zombies' clips by kind, each the names after `ai_zombie_base_` (or
/// `bo3_ai_zombie_`) that start so (`lane::t7::zombie_clip` loads them
/// from the map zone). In the game the animation state machine's selector
/// tables pick among them by blackboard (`_locomotion_speed`,
/// `_has_legs`, the board, the window); here the kind does, a zombie
/// keeping its own variant of each.
pub(crate) const CLIP_KINDS: &[(&str, &str)] = &[
    ("walk", "walk_ad_v"),
    ("run", "run_ad_v"),
    ("sprint", "sprint_ad_v"),
    ("super_sprint", "supersprint_ad_v"),
    ("idle", "idle_ad_v"),
    ("crawl", "crawl_v"),
    ("crawl_sprint", "crawl_sprint_v"),
    ("crawl_idle", "crawl_idle_v"),
    ("melee", "ad_attack_v"),
    ("crawl_melee", "crawl_attack_v"),
    ("grab", "boardtear_aligned_m_1_grab"),
    ("hold", "boardtear_aligned_m_1_hold"),
    ("pull", "boardtear_aligned_m_1_pull"),
    ("crawl_grab", "crawl_boardtear_aligned_m_1_grab"),
    ("crawl_hold", "crawl_boardtear_aligned_m_1_hold"),
    ("crawl_pull", "crawl_boardtear_aligned_m_1_pull"),
    ("window_attack", "win_attack_arm_"),
    ("mantle_walk", "walk_ad_win_trav_m_v1"),
    ("mantle_run", "run_ad_win_trav_m_v1"),
    ("mantle_sprint", "sprint_ad_win_trav_m_v1"),
    ("crawl_mantle", "crawl_win_trav_m_v1"),
    ("death", "death_v"),
    ("crawl_death", "crawl_death_v"),
    ("taunt", "taunts_"),
];

/// A clip name (after its prefix) of a kind's pattern: a `_v` pattern
/// takes only its numbered variants (`walk_ad_v2`, not
/// `walk_ad_v2_pain_chest_f`).
pub(crate) fn clip_matches(rest: &str, pattern: &str) -> bool {
    match rest.strip_prefix(pattern) {
        Some(tail) if pattern.ends_with("_v") => !tail.is_empty() && tail.bytes().all(|b| b.is_ascii_digit()),
        Some(_) => true,
        None => false,
    }
}

/// The kinds that loop whatever their clip's flag (the locomotion and
/// idles).
fn loops(kind: &str) -> bool {
    matches!(kind, "walk" | "run" | "sprint" | "super_sprint" | "idle" | "crawl" | "crawl_sprint" | "crawl_idle")
}

/// The clip names of each kind among the match's animations.
fn clip_index(zm: &Zm) -> HashMap<&'static str, Vec<String>> {
    let mut out: HashMap<&'static str, Vec<String>> = HashMap::new();
    for name in zm.anims.keys() {
        let Some(rest) = name.strip_prefix("ai_zombie_base_").or_else(|| name.strip_prefix("bo3_ai_zombie_")) else { continue };
        if let Some((kind, _)) = CLIP_KINDS.iter().find(|(_, p)| clip_matches(rest, p)) {
            out.entry(kind).or_default().push(name.clone());
        }
    }
    for v in out.values_mut() {
        v.sort();
    }
    out
}

/// Actor `n`'s clip of `kind` (its own variant: by its entity number and a
/// salt, so a horde does not step in time), if the map has one.
fn clip_for(world: &mut World, n: u32, kind: &str, salt: u32) -> Option<String> {
    if world.get_resource::<T7Ai>()?.clips.is_none() {
        let idx = clip_index(&world.resource::<Zm>());
        world.resource_mut::<T7Ai>().clips = Some(idx);
    }
    let s = world.get_resource::<T7Ai>()?;
    let list = s.clips.as_ref()?.get(kind)?;
    let h = n.wrapping_mul(0x9e37_79b9).wrapping_add(salt.wrapping_mul(0x85eb_ca6b));
    list.get((h >> 7) as usize % list.len()).cloned()
}

/// Play `clip` on actor `n` from now (a loop already playing goes on;
/// `restart` starts a one-shot over), at `rate`.
fn play_clip(world: &mut World, n: u32, kind: &str, clip: &str, now: i64, rate: f32, restart: bool) {
    let (length_ms, looping) = world.resource::<Zm>().anims.get(clip).map_or((1000, false), |a| (super::actors::anim_length_ms(a).max(1), a.looping));
    let looping = looping || loops(kind);
    let mut zm = world.resource_mut::<Zm>();
    let Some(a) = zm.actors.get_mut(&n) else { return };
    if let Some(p) = a.playing.as_mut()
        && p.anim == clip
        && !restart
    {
        // (A loop changing speed keeps its place: no pop.)
        if (p.rate - rate).abs() > 0.01 {
            p.base_ms = p.elapsed(now);
            p.start_ms = now;
            p.rate = rate;
        }
        return;
    }
    a.playing = Some(super::actors::Playing {
        state: kind.to_owned(),
        anim: clip.to_owned(),
        start_ms: now,
        length_ms,
        looping,
        rate,
        ..Default::default()
    });
}

/// Show the clip of `kind` on actor `n` (a one-shot from its start): its
/// length (ms) and when its `note` notetrack falls (ms), if it has one.
fn show(world: &mut World, vm: &mut Vm<World>, n: u32, obj: ObjRef, kind: &str, now: i64, restart: bool, note: Option<&str>) -> Option<(i64, Option<i64>)> {
    let crawler = crawls(vm, obj);
    let kind = if crawler && CLIP_KINDS.iter().any(|(k, _)| k.strip_prefix("crawl_") == Some(kind)) { format!("crawl_{kind}") } else { kind.to_owned() };
    let salt = if restart { (now / 50) as u32 } else { 0 };
    let clip = clip_for(world, n, &kind, salt)?;
    play_clip(world, n, &kind, &clip, now, 1.0, restart);
    let zm = world.resource::<Zm>();
    let a = zm.anims.get(&clip)?;
    let len = super::actors::anim_length_ms(a).max(1);
    let at = note.and_then(|w| a.notifies.iter().find(|(name, _)| name == w || name.ends_with(w))).map(|(_, t)| (*t * len as f32) as i64);
    Some((len, at))
}

/// A zombie with its legs gone (`missinglegs`: it crawls).
fn crawls(vm: &mut Vm<World>, obj: ObjRef) -> bool {
    let f = vm.intern("missinglegs");
    vm.raw_field(obj, f).as_int().is_some_and(|v| v != 0)
}

/// The locomotion clip for how fast it moved this tick (units/s): its
/// speed's loop at the rate that matches the ground it covered, the idle
/// when it stood.
fn show_locomotion(world: &mut World, vm: &mut Vm<World>, n: u32, obj: ObjRef, ai: &Ai, moved: f32, now: i64) {
    let nominal = speed(vm, ai, obj);
    let crawler = crawls(vm, obj);
    let per_s = moved * 1000.0 / f32::from(crate::MATCH_TICK_MS as u16);
    let kind = if per_s < 4.0 {
        if crawler { "crawl_idle" } else { "idle" }
    } else if crawler {
        if nominal >= 200.0 { "crawl_sprint" } else { "crawl" }
    } else if nominal >= 250.0 {
        "super_sprint"
    } else if nominal >= 200.0 {
        "sprint"
    } else if nominal >= 130.0 {
        "run"
    } else {
        "walk"
    };
    let clip = clip_for(world, n, kind, 0).or_else(|| (kind == "super_sprint").then(|| clip_for(world, n, "sprint", 0)).flatten());
    let Some(clip) = clip else { return };
    // (Path-driven: the feet keep up with the ground covered, a slowed
    // zombie's stride slows with it.)
    let rate = if per_s < 4.0 { 1.0 } else { (per_s / ground_speed(world, vm, n, ai, obj).max(1.0)).clamp(0.5, 1.5) };
    play_clip(world, n, kind, &clip, now, rate, false);
}

/// The death clip, once (`actors::die` on a Black Ops III actor).
pub(crate) fn show_death(vm: &mut Vm<World>, world: &mut World, n: u32, now: i64) {
    let Some(obj) = world.resource::<Zm>().actors.get(&n).and_then(|a| a.obj) else { return };
    if world.get_resource::<T7Ai>().is_none() {
        return;
    }
    show(world, vm, n, obj, "death", now, true, None);
}

/// The living player nearest a point (his script object).
fn nearest_player(world: &mut World, at: [f32; 3]) -> Option<ObjRef> {
    let players: Vec<(u32, ObjRef)> = world.resource::<Zm>().players.iter().map(|(c, p)| (*c, p.obj)).collect();
    let mut best: Option<(f32, ObjRef)> = None;
    for (c, o) in players {
        let Some(ps) = super::frame(world).player(crate::world::ClientId(c)).copied() else { continue };
        let d = (0..3).map(|i| (ps.origin[i] - at[i]).powi(2)).sum::<f32>();
        if best.is_none_or(|b| d < b.0) {
            best = Some((d, o));
        }
    }
    best.map(|b| b.1)
}

/// Where a Black Ops III actor's tag is when its model's skeleton is not
/// loaded: the Margwa's three heads (`j_chunk_head_bone`, `_le`, `_ri`)
/// stand above its shoulders, the middle one highest.
pub(crate) fn tag_guess(world: &World, n: u32, tag: &str) -> Option<[f32; 3]> {
    let st = world.get_resource::<T7Ai>()?;
    if st.ai.get(&n)?.archetype != "margwa" {
        return None;
    }
    let (side, up) = match tag.to_ascii_lowercase().as_str() {
        "j_chunk_head_bone" => (0.0, 112.0),
        "j_chunk_head_bone_le" => (24.0, 100.0),
        "j_chunk_head_bone_ri" => (-24.0, 100.0),
        _ => return None,
    };
    let e = world.resource::<Zm>().ents.get(&n)?;
    let yaw = e.angles[1].to_radians();
    // (Left of its facing is +side.)
    Some([e.origin[0] - yaw.sin() * side, e.origin[1] + yaw.cos() * side, e.origin[2] + up])
}

/// Each tick: scripted animations that have played out tell their
/// notify "end" (`animation::play` waits for it).
fn anim_tick(world: &mut World) {
    let now = world.resource::<Zm>().now_ms;
    let notes: Vec<(ObjRef, String, String)> = match world.get_resource_mut::<T7Ai>() {
        Some(mut st) if !st.anim_notes.is_empty() => {
            let (due, keep): (Vec<_>, Vec<_>) = std::mem::take(&mut st.anim_notes).into_iter().partition(|e| e.0 <= now);
            st.anim_notes = keep;
            due.into_iter().map(|(_, o, n, k)| (o, n, k)).collect()
        }
        _ => Vec::new(),
    };
    if !notes.is_empty() {
        super::with_vm(world, |vm, world| {
            for (o, name, note) in notes {
                // (The notify gets the note, and the note itself is
                // notified: `self waittill("water_drain_complete")`.)
                if vm.alive(o) {
                    let v = vm.string(&note);
                    vm.notify_str(world, o, &name, &[v]);
                }
                if vm.alive(o) && note != name {
                    vm.notify_str(world, o, &note, &[]);
                }
            }
        });
    }
    let due: Vec<(ObjRef, String)> = match world.get_resource_mut::<T7Ai>() {
        Some(mut st) if !st.anim_ends.is_empty() => {
            let (due, keep): (Vec<_>, Vec<_>) = std::mem::take(&mut st.anim_ends).into_iter().partition(|e| e.0 <= now);
            st.anim_ends = keep;
            due.into_iter().map(|(_, o, n)| (o, n)).collect()
        }
        _ => return,
    };
    super::with_vm(world, |vm, world| {
        for (o, name) in due {
            if vm.alive(o) {
                let end = vm.string("end");
                vm.notify_str(world, o, &name, &[end]);
            }
        }
    });
}

/// `animscripted(notify, origin, angles, anim, ...)` on any entity: it is
/// put at the origin and angles and its notify gets "end" when the clip
/// has played (its length where the clip is loaded, else a second).
fn animscripted(vm: &mut Vm<World>, world: &mut World, s: &Value, a: &[Value]) -> Result<Value, String> {
    let Value::Object(o) = s else { return Ok(Value::Undefined) };
    let note = a.first().map(|v| vm.to_text(v)).unwrap_or_else(String::new);
    let anim = a.get(3).map(|v| vm.to_text(v)).unwrap_or_else(|| note.clone());
    if let Some(n) = super::entnum(vm, s) {
        let mut zm = world.resource_mut::<Zm>();
        if let Some(e) = zm.ents.get_mut(&n) {
            if let Some(p) = a.get(1).and_then(Value::as_vec3) {
                e.origin = p;
            }
            if let Some(g) = a.get(2).and_then(Value::as_vec3) {
                e.angles = g;
            }
        }
    }
    let len = world.resource::<Zm>().anims.get(&anim).map_or(1000, super::actors::anim_length_ms);
    let marks: Vec<(String, f32)> = world.resource::<Zm>().anims.get(&anim).map(|a| a.notifies.clone()).unwrap_or_default();
    let now = world.resource::<Zm>().now_ms;
    if !note.is_empty()
        && let Some(mut st) = world.get_resource_mut::<T7Ai>()
    {
        st.anim_ends.retain(|e| e.1 != *o);
        st.anim_notes.retain(|e| e.1 != *o);
        for (mark, at) in marks {
            // (A note that names nothing, "end" included, is the end's.)
            if !mark.is_empty() && mark != "end" {
                st.anim_notes.push((now + (at.clamp(0.0, 1.0) * len as f32) as i64, *o, note.clone(), mark.to_ascii_lowercase()));
            }
        }
        st.anim_ends.push((now + len.max(50), *o, note));
    }
    Ok(Value::Undefined)
}

/// Guessed tags a hit can name (`tag_guess`): bone index 1000 + k.
const GUESSED_TAGS: &[&str] = &["j_chunk_head_bone", "j_chunk_head_bone_le", "j_chunk_head_bone_ri"];

/// The bone a hit at `point` struck, as the engine gives scripts its index
/// (`getpartname(model, index)` names it): a guessed tag within 24 units
/// (a Margwa's head), else 0.
pub(crate) fn bone_for_point(world: &World, n: u32, point: [f32; 3]) -> i32 {
    GUESSED_TAGS
        .iter()
        .enumerate()
        .filter_map(|(k, t)| tag_guess(world, n, t).map(|p| (k, (0..3).map(|i| (p[i] - point[i]).powi(2)).sum::<f32>())))
        .filter(|(_, d)| *d < 24.0 * 24.0)
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map_or(0, |(k, _)| 1000 + k as i32)
}

/// The Margwa's timed behaviours: (name, its start and terminate functions
/// (or its registered action), how long it plays (ms), its notetrack and
/// when (ms)).
struct MargwaBehaviour {
    name: &'static str,
    start: &'static str,
    terminate: &'static str,
    action: Option<&'static str>,
    len: i64,
    note: Option<(&'static str, i64)>,
}

const MARGWA: &[MargwaBehaviour] = &[
    MargwaBehaviour { name: "spawn", start: "margwaspawnstart", terminate: "", action: None, len: 2000, note: None },
    MargwaBehaviour { name: "teleport_in", start: "margwateleportinstart", terminate: "margwateleportinterminate", action: None, len: 1200, note: None },
    MargwaBehaviour { name: "teleport_out", start: "margwateleportoutstart", terminate: "margwateleportoutterminate", action: None, len: 1200, note: None },
    MargwaBehaviour { name: "pain", start: "margwapainstart", terminate: "margwapainterminate", action: None, len: 1500, note: None },
    MargwaBehaviour { name: "react_stun", start: "margwareactstunstart", terminate: "margwareactstunterminate", action: Some("margwareactstunaction"), len: 3000, note: None },
    MargwaBehaviour { name: "react_idgun", start: "margwareactidgunstart", terminate: "margwareactidgunterminate", action: None, len: 3000, note: None },
    MargwaBehaviour { name: "react_sword", start: "margwareactswordstart", terminate: "margwareactswordterminate", action: None, len: 2000, note: None },
    MargwaBehaviour { name: "smash", start: "margwasmashattackstart", terminate: "margwasmashattackterminate", action: None, len: 2600, note: Some(("margwa_smash_attack", 1300)) },
    MargwaBehaviour { name: "swipe", start: "margwaswipeattackstart", terminate: "margwaswipeattackterminate", action: Some("margwaswipeattackaction"), len: 1600, note: Some(("margwa_melee_fire", 700)) },
];

/// The Margwa's tree (`margwa.gsc`'s registered functions): its target
/// service, then the first behaviour whose condition holds (spawn,
/// teleport in and out, wait, pain, the stun / Apothicon Servant / sword
/// reactions, smash, swipe), else it walks to its goal.
fn think_margwa(vm: &mut Vm<World>, world: &mut World, n: u32, obj: ObjRef, ai: &mut Ai, now: i64) {
    if let Some((name, start, _)) = ai.action.clone() {
        let Some(b) = MARGWA.iter().find(|b| b.name == name) else {
            ai.action = None;
            return;
        };
        if trace_on() && (now - start) % 500 < 50 {
            let h = vm.intern("head");
            let of = vm.intern("#67079016");
            let opens: Vec<String> = match vm.raw_field(obj, h) {
                Value::Array(a) => a.snapshot().values_in_order().map(|v| if let Value::Object(o) = v { vm.to_text(&vm.raw_field(*o, of)) } else { String::new() }).collect(),
                _ => Vec::new(),
            };
            diag::info!(Sim, "bo2mp t7 margwa ent{n} {name} +{}ms heads open {opens:?}", now - start);
        }
        if let Some((note, at)) = b.note {
            let key = format!("__note_{name}");
            if now - start >= at && !ai.blackboard.contains_key(&key) {
                ai.blackboard.insert(key, Value::Int(1));
                notetrack(vm, world, obj, note);
            }
        }
        if let Some(a) = b.action
            && act(vm, world, a, 1, obj) != BHTN_RUNNING
            && now - start >= b.len / 2
        {
            act(vm, world, a, 2, obj);
            if !b.terminate.is_empty() {
                ask(vm, world, b.terminate, obj);
            }
            ai.blackboard.remove(&format!("__note_{name}"));
            ai.action = None;
            return;
        }
        if now - start >= b.len {
            if let Some(a) = b.action {
                act(vm, world, a, 2, obj);
            }
            if !b.terminate.is_empty() {
                ask(vm, world, b.terminate, obj);
            }
            ai.blackboard.remove(&format!("__note_{name}"));
            ai.action = None;
        }
        return;
    }
    ask(vm, world, "margwatargetservice", obj);
    if !vm.alive(obj) {
        return;
    }
    // (Near its enemy it turns to face him, as its tree's orient mode
    // does, so its attacks' facing checks hold.)
    let enemy_at = {
        let f = vm.intern("enemy");
        let e = vm.raw_field(obj, f);
        super::origin_of(vm, world, &e)
    };
    if let (Some(p), Some(e)) = (enemy_at, world.resource_mut::<Zm>().ents.get_mut(&n))
        && (p[0] - e.origin[0]).hypot(p[1] - e.origin[1]) < 400.0
    {
        e.angles[1] = (p[1] - e.origin[1]).atan2(p[0] - e.origin[0]).to_degrees();
    }
    let pick = if ask(vm, world, "margwashouldspawn", obj) {
        Some("spawn")
    } else if ask(vm, world, "margwashouldfreeze", obj) || ask(vm, world, "margwashouldwait", obj) {
        // (Frozen, or waiting: it stands.)
        return;
    } else if ask(vm, world, "margwashouldteleportin", obj) {
        Some("teleport_in")
    } else if ask(vm, world, "margwashouldteleportout", obj) {
        Some("teleport_out")
    } else if ask(vm, world, "margwashouldshowpain", obj) {
        Some("pain")
    } else if ask(vm, world, "margwashouldreactstun", obj) {
        Some("react_stun")
    } else if ask(vm, world, "margwashouldreactidgun", obj) {
        Some("react_idgun")
    } else if ask(vm, world, "margwashouldreactsword", obj) {
        Some("react_sword")
    } else if ask(vm, world, "margwashouldsmashattack", obj) {
        Some("smash")
    } else if ask(vm, world, "margwashouldswipeattack", obj) {
        Some("swipe")
    } else {
        None
    };
    if trace_on() {
        let enemy = {
            let f = vm.intern("enemy");
            vm.raw_field(obj, f)
        };
        let yaw = format!(
            "smash {} swipe {} wait {} freeze {} at {:?}",
            u8::from(ask(vm, world, "margwashouldsmashattack", obj)),
            u8::from(ask(vm, world, "margwashouldswipeattack", obj)),
            u8::from(ask(vm, world, "margwashouldwait", obj)),
            u8::from(ask(vm, world, "margwashouldfreeze", obj)),
            world.resource::<Zm>().ents.get(&n).map(|e| (e.origin, e.angles[1]))
        );
        vm.report_once(format!("bo2mp t7 margwa ent{n}: {pick:?} enemy {} angles {yaw}", vm.to_text(&enemy)));
    }
    match pick.and_then(|p| MARGWA.iter().find(|b| b.name == p)) {
        Some(b) => {
            ask(vm, world, b.start, obj);
            if let Some(a) = b.action {
                act(vm, world, a, 0, obj);
            }
            ai.blackboard.remove("__margwa_moving");
            ai.action = Some((b.name.to_owned(), now, b.len));
        }
        None => {
            if !ai.blackboard.contains_key("__margwa_moving") {
                ai.blackboard.insert("__margwa_moving".to_owned(), Value::Int(1));
                ask(vm, world, "margwamovestart", obj);
            }
            walk(vm, world, n, obj, ai, now);
        }
    }
}

/// Actions that last while the zombie walks to its goal.
fn is_move_action(name: &str) -> bool {
    matches!(name, "zombiemovetoentranceaction" | "zombiemovetoattackspotaction" | "zombiemoveaway")
}

/// The engine's notetrack callout: the handlers the archetype scripts
/// registered for `note`.
fn notetrack(vm: &mut Vm<World>, world: &mut World, obj: ObjRef, note: &str) {
    let mut list = gsc_t6::Array::new();
    list.push(vm.string(note));
    vm.spawn_named(world, "scripts/shared/ai/systems/animation_state_machine_notetracks", "runnotetrackhandler", Value::Object(obj), vec![Value::Object(obj), Value::array(list)]);
}

/// Walking speed (units/s) by `zombie_move_speed` (the blackboard's
/// `_locomotion_speed` first).
fn speed(vm: &mut Vm<World>, ai: &Ai, obj: ObjRef) -> f32 {
    let bb = ai.blackboard.get("_locomotion_speed").map(|v| vm.to_text(v));
    let f = vm.intern("zombie_move_speed");
    let s = bb.unwrap_or_else(|| vm.to_text(&vm.raw_field(obj, f)));
    if s.contains("super_sprint") {
        250.0
    } else if s.contains("sprint") {
        200.0
    } else if s.contains("run") {
        130.0
    } else {
        60.0
    }
}

/// The locomotion class a `zombie_move_speed` text names.
fn speed_kind(vm: &mut Vm<World>, ai: &Ai, obj: ObjRef) -> &'static str {
    let s = speed_text(vm, ai, obj);
    if s.contains("super_sprint") {
        "super_sprint"
    } else if s.contains("sprint") {
        "sprint"
    } else if s.contains("run") {
        "run"
    } else {
        "walk"
    }
}

fn speed_text(vm: &mut Vm<World>, ai: &Ai, obj: ObjRef) -> String {
    let bb = ai.blackboard.get("_locomotion_speed").map(|v| vm.to_text(v));
    let f = vm.intern("zombie_move_speed");
    bb.unwrap_or_else(|| vm.to_text(&vm.raw_field(obj, f)))
}

/// Black Ops III moves a zombie by its locomotion clip's root motion: the
/// ground the clip's `delta` covers per second (its variant is the zombie's
/// own, `clip_for`), so walk, run, sprint and super sprint are as fast as
/// the game's clips. A map with no clips, or `BO2MP_T7_SPEEDS=standin`,
/// keeps `speed`'s numbers.
fn ground_speed(world: &mut World, vm: &mut Vm<World>, n: u32, ai: &Ai, obj: ObjRef) -> f32 {
    let nominal = speed(vm, ai, obj);
    if std::env::var("BO2MP_T7_SPEEDS").is_ok_and(|v| v == "standin") {
        return nominal;
    }
    let kind = speed_kind(vm, ai, obj);
    let kind = if crawls(vm, obj) { if nominal >= 200.0 { "crawl_sprint" } else { "crawl" } } else { kind };
    let clip = clip_for(world, n, kind, 0).or_else(|| (kind == "super_sprint").then(|| clip_for(world, n, "sprint", 0)).flatten());
    let measured = clip.and_then(|c| world.resource::<Zm>().anims.get(&c).map(super::actors::anim_speed)).unwrap_or(0.0);
    if measured >= 5.0 { measured } else { nominal }
}

fn ai_state(world: &World, n: u32) -> Option<Ai> {
    world.get_resource::<T7Ai>()?.ai.get(&n).cloned()
}

fn put_ai(world: &mut World, n: u32, ai: Ai) {
    if let Some(mut s) = world.get_resource_mut::<T7Ai>() {
        s.ai.insert(n, ai);
    }
}

/// Start a registered action: its start function, then it runs (its clip
/// or its walk) unless start said it is done.
fn start_action(vm: &mut Vm<World>, world: &mut World, ai: &mut Ai, obj: ObjRef, name: &str, now: i64) {
    let status = act(vm, world, name, 0, obj);
    if status == BHTN_RUNNING {
        if let ObjKind::Entity(n) = vm.kind(obj).unwrap_or(ObjKind::Level) {
            let kind = match name {
                "grabboardaction" => Some("grab"),
                "holdboardaction" => Some("hold"),
                "pullboardaction" => Some("pull"),
                "zombieattackthroughboardsaction" => Some("window_attack"),
                "zombietauntaction" => Some("taunt"),
                "zombiemantleaction" => Some(match speed(vm, ai, obj) {
                    s if s >= 200.0 => "mantle_sprint",
                    s if s >= 130.0 => "mantle_run",
                    _ => "mantle_walk",
                }),
                _ => None,
            };
            // (The action lasts its clip, the notetrack it waits for where
            // the clip has it: `action_clip` times them without.)
            let note = action_clip(name).1;
            if let Some((len, at)) = kind.and_then(|k| show(world, vm, n, obj, k, now, true, note)) {
                ai.action = Some((name.to_owned(), now, len));
                ai.note_ms = at;
                return;
            }
        }
        ai.note_ms = None;
        ai.action = Some((name.to_owned(), now, action_clip(name).0));
    } else {
        act(vm, world, name, 2, obj);
    }
}

/// The zombie at its goal (inside the goal radius).
fn at_goal(vm: &mut Vm<World>, world: &World, n: u32, obj: ObjRef) -> bool {
    let zm = world.resource::<Zm>();
    let (Some(g), Some(p)) = (zm.actors.get(&n).and_then(|a| a.goal), zm.ents.get(&n).map(|e| e.origin)) else { return true };
    let f = vm.intern("goalradius");
    let r = vm.raw_field(obj, f).as_float().unwrap_or(16.0).max(16.0);
    ((g[0] - p[0]).powi(2) + (g[1] - p[1]).powi(2)).sqrt() <= r && (g[2] - p[2]).abs() < 80.0
}

/// One zombie's tick (from `actors::think` for Black Ops III actors).
pub(crate) fn think_one(vm: &mut Vm<World>, world: &mut World, n: u32, now: i64) {
    let Some(a) = world.resource::<Zm>().actors.get(&n).cloned() else { return };
    let Some(obj) = a.obj else { return };
    if !a.alive || !vm.alive(obj) {
        return;
    }
    let Some(mut ai) = ai_state(world, n) else { return };
    // The enemy the scripts chose.
    let fav = {
        let f = vm.intern("favoriteenemy");
        vm.raw_field(obj, f)
    };
    let enemy = match fav {
        Value::Object(o) if vm.alive(o) => Some(o),
        _ => None,
    };
    // (The engine's perception: with no favorite, the nearest player.)
    let perceived = enemy.or_else(|| nearest_player(world, a.goal.or_else(|| world.resource::<Zm>().ents.get(&n).map(|e| e.origin))?));

    // A traversal (a window, a jump down) plays out first.
    if let Some((from, to, start, len)) = ai.traverse {
        let t = ((now - start) as f32 / len.max(1) as f32).min(1.0);
        set_origin(world, n, std::array::from_fn(|i| from[i] + (to[i] - from[i]) * t));
        if t >= 1.0 {
            ai.traverse = None;
            vm.notify_str(world, obj, "traverse_end", &[]);
        }
        put_ai(world, n, ai);
        return;
    }
    // The engine's `enemy` (its perception's pick) is the favorite enemy.
    if perceived != a.enemy
        && let Some(act) = world.resource_mut::<Zm>().actors.get_mut(&n)
    {
        act.enemy = perceived;
    }
    {
        let f = vm.intern("enemy");
        vm.set_raw_field(obj, f, perceived.map_or(Value::Undefined, Value::Object));
    }
    if ai.archetype == "margwa" {
        think_margwa(vm, world, n, obj, &mut ai, now);
        put_ai(world, n, ai);
        return;
    }
    for s in SERVICES {
        ask(vm, world, s, obj);
    }
    if !vm.alive(obj) {
        return;
    }
    // The running action: a walk lasts until the goal (or 20 s), a clip its
    // length; then its update (if it has one) says whether it goes on, and
    // its terminate runs.
    if let Some((name, start, len)) = ai.action.clone() {
        let mover = is_move_action(&name);
        let done = if mover {
            walk(vm, world, n, obj, &mut ai, now);
            show_locomotion(world, vm, n, obj, &ai, ai.moved, now);
            at_goal(vm, world, n, obj) || now - start > 20_000
        } else {
            let (_, note) = action_clip(&name);
            if let Some(note) = note {
                let key = format!("__note_{name}");
                if now - start >= ai.note_ms.unwrap_or(len / 2) && !ai.blackboard.contains_key(&key) {
                    ai.blackboard.insert(key, Value::Int(1));
                    notetrack(vm, world, obj, note);
                }
            }
            now - start >= len
        };
        if !done {
            put_ai(world, n, ai);
            return;
        }
        let status = act(vm, world, &name, 1, obj);
        if status == BHTN_RUNNING && now - start < len.max(1) + 4000 && !mover {
            put_ai(world, n, ai);
            return;
        }
        act(vm, world, &name, 2, obj);
        ai.blackboard.remove(&format!("__note_{name}"));
        ai.action = None;
        if name == "zombiemantleaction" {
            mantle_through(vm, world, n, obj, &mut ai, now);
        }
        // The pulled board is no one's target any more.
        if name == "pullboardaction" {
            ask(vm, world, "updatechunkservice", obj);
        }
        // A board pull goes grab, hold, pull.
        let next = match name.as_str() {
            "grabboardaction" => Some("holdboardaction"),
            "holdboardaction" => Some("pullboardaction"),
            _ => None,
        };
        if let Some(next) = next {
            start_action(vm, world, &mut ai, obj, next, now);
        }
        put_ai(world, n, ai);
        return;
    }
    // A melee swing: its notetrack (the hit) at the clip's `fire` note
    // (0.4 s in without one), over at the clip's end (1 s without one).
    if let Some((start, hit)) = ai.melee {
        let (len, at) = ai.melee_ms;
        if !hit && now - start >= at {
            ai.melee = Some((start, true));
            notetrack(vm, world, obj, "zombie_melee");
        }
        if now - start >= len {
            ai.melee = None;
        }
        put_ai(world, n, ai);
        return;
    }
    // The tree's branches, first that holds.
    if !ask(vm, world, "inplayablearea", obj) {
        for svc in OUTSIDE_SERVICES {
            // (The attack spot is the window's, so only once the way to a
            // window was found: `updateattackspotservice` reads
            // `first_node` unchecked, and a zombie that rose or came in
            // elsewhere has none.)
            if *svc == "updateattackspotservice" {
                let f = super::t7::field_name(vm, "first_node");
                if matches!(vm.raw_field(obj, f), Value::Undefined) {
                    continue;
                }
            }
            ask(vm, world, svc, obj);
        }
        let pick = if ask(vm, world, "zombieshouldenterplayable", obj) {
            Some("zombiemantleaction")
        } else if !ask(vm, world, "zombiegottoentrance", obj) {
            Some("zombiemovetoentranceaction")
        } else if ask(vm, world, "zombiehasattackspotalready", obj) && !ask(vm, world, "zombiegottoattackspot", obj) {
            Some("zombiemovetoattackspotaction")
        } else if ask(vm, world, "zombieshouldtear", obj) && {
            // The board it goes for (`getChunkService`, the tear branch's).
            ask(vm, world, "getchunkservice", obj);
            ask(vm, world, "ischunkvalid", obj)
        } {
            Some("grabboardaction")
        } else if ask(vm, world, "zombieshouldattackthroughboards", obj) {
            Some("zombieattackthroughboardsaction")
        } else if ask(vm, world, "zombieshouldtaunt", obj) {
            Some("zombietauntaction")
        } else {
            None
        };
        if trace_on() {
            let c: Vec<String> = ["zombieenteredplayableservice", "inplayablearea", "zombieshouldenterplayable", "zombiegottoentrance", "zombieisatentrance", "zombiehasattackspotalready", "zombiegottoattackspot", "zombieshouldtear", "zombieshouldattackthroughboards", "ischunkvalid"]
                .iter()
                .map(|k| format!("{k}={}", u8::from(ask(vm, world, k, obj))))
                .collect();
            let line = format!("bo2mp t7 ai ent{n} outside: {pick:?} [{}]", c.join(" "));
            vm.report_once(line);
        }
        if let Some(name) = pick {
            start_action(vm, world, &mut ai, obj, name, now);
        }
        if ai.action.is_none() {
            show_locomotion(world, vm, n, obj, &ai, 0.0, now);
        }
        put_ai(world, n, ai);
        return;
    }
    // Inside: the tree's playable branch turns its power-up drop back on
    // (`enablePowerups`; `zombie_spawn_init` spawns every zombie with
    // `no_powerups`, as BO2's emerge into the playable area cleared it), once.
    if !ai.blackboard.contains_key("__powerups_on") {
        ai.blackboard.insert("__powerups_on".to_owned(), Value::Int(1));
        ask(vm, world, "enablepowerups", obj);
    }
    if ask(vm, world, "zombieshouldmelee", obj) {
        ai.melee = Some((now, false));
        ai.melee_ms = match show(world, vm, n, obj, "melee", now, true, Some("zombie_melee")) {
            Some((len, at)) => (len, at.unwrap_or(len * 2 / 5)),
            None => (1000, 400),
        };
        put_ai(world, n, ai);
        return;
    }
    walk(vm, world, n, obj, &mut ai, now);
    show_locomotion(world, vm, n, obj, &ai, ai.moved, now);
    put_ai(world, n, ai);
}

fn set_origin(world: &mut World, n: u32, p: [f32; 3]) {
    if let Some(e) = world.resource_mut::<Zm>().ents.get_mut(&n) {
        e.origin = p;
    }
}

/// Through the window it stands at: across the barrier's link to the
/// inside, as a traversal.
fn mantle_through(vm: &mut Vm<World>, world: &mut World, n: u32, _obj: ObjRef, ai: &mut Ai, now: i64) {
    let Some(here) = world.resource::<Zm>().ents.get(&n).map(|e| e.origin) else { return };
    let to = {
        let Some(s) = world.get_resource::<T7Ai>() else { return };
        let Some(nav) = s.nav.as_ref() else { return };
        nav.links
            .iter()
            .filter(|l| l.kind == "zbarrier")
            .min_by(|a, b| d2(a.from, here).total_cmp(&d2(b.from, here)))
            .filter(|l| d2(l.from, here) < 160.0 * 160.0)
            .map(|l| l.to)
    };
    if let Some(to) = to {
        ai.traverse = Some((here, to, now, 600));
        ai.path.clear();
        ai.planned = None;
    }
    let _ = vm;
}

fn d2(a: [f32; 3], b: [f32; 3]) -> f32 {
    (a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)
}

/// Walk toward the goal (`setgoal`): plan over the nav mesh when the goal
/// moved or every second, then step along the corners at the zombie's
/// speed, on the mesh's height; `goal` when inside the goal radius.
fn walk(vm: &mut Vm<World>, world: &mut World, n: u32, obj: ObjRef, ai: &mut Ai, now: i64) {
    ai.moved = 0.0;
    if ai.blackboard.get("__pathmode").is_some_and(|m| vm.to_text(m) == "dont move") {
        return;
    }
    let Some(goal) = world.resource::<Zm>().actors.get(&n).and_then(|a| a.goal) else { return };
    let Some(here) = world.resource::<Zm>().ents.get(&n).map(|e| e.origin) else { return };
    let radius = {
        let f = vm.intern("goalradius");
        vm.raw_field(obj, f).as_float().unwrap_or(16.0).max(4.0)
    };
    let flat = ((goal[0] - here[0]).powi(2) + (goal[1] - here[1]).powi(2)).sqrt();
    if flat <= radius && (goal[2] - here[2]).abs() < 80.0 {
        let sent = world.resource::<Zm>().actors.get(&n).is_some_and(|a| a.goal_sent);
        if !sent {
            if let Some(a) = world.resource_mut::<Zm>().actors.get_mut(&n) {
                a.goal_sent = true;
            }
            vm.notify_str(world, obj, "goal", &[]);
        }
        ai.path.clear();
        return;
    }
    let moved = ai.planned.is_none_or(|p| d2(p, goal) > 24.0 * 24.0);
    if moved || now - ai.planned_ms > 1000 || ai.path.is_empty() {
        let path = world.get_resource::<T7Ai>().and_then(|s| s.nav.as_ref().and_then(|nav| nav.find_path(here, goal)));
        ai.planned = Some(goal);
        ai.planned_ms = now;
        match path {
            Some(p) => ai.path = p,
            None => {
                ai.path.clear();
                vm.notify_str(world, obj, "bad_path", &[]);
                return;
            }
        }
    }
    let mut step = ground_speed(world, vm, n, ai, obj) * f32::from(crate::MATCH_TICK_MS as u16) / 1000.0;
    let mut at = here;
    while step > 0.0 {
        let Some(w) = ai.path.first().cloned() else { break };
        let d = [w.point[0] - at[0], w.point[1] - at[1]];
        let len = d[0].hypot(d[1]);
        if len <= step {
            at = [w.point[0], w.point[1], w.point[2]];
            step -= len;
            ai.path.remove(0);
            // A traversal starts here: play it to its end.
            if let Some(l) = w.link {
                let to = world.get_resource::<T7Ai>().and_then(|s| s.nav.as_ref().and_then(|nav| nav.links.get(l as usize).map(|k| k.to)));
                if let Some(to) = to {
                    ai.traverse = Some((at, to, now, 800));
                    vm.notify_str(world, obj, "traverse_start", &[]);
                    break;
                }
            }
        } else {
            at = [at[0] + d[0] / len * step, at[1] + d[1] / len * step, at[2]];
            step = 0.0;
        }
    }
    // On the mesh's height where it stands.
    if let Some(z) = world.get_resource::<T7Ai>().and_then(|s| s.nav.as_ref().and_then(|nav| nav.locate(at, 64.0, 64.0).map(|f| nav.height(f, at[0], at[1])))) {
        at[2] = z;
    }
    let yaw = (at[1] - here[1]).atan2(at[0] - here[0]).to_degrees();
    if let Some(e) = world.resource_mut::<Zm>().ents.get_mut(&n) {
        e.origin = at;
        if (at[0] - here[0]).abs() + (at[1] - here[1]).abs() > 0.01 {
            e.angles[1] = yaw;
        }
    }
    ai.moved = (at[0] - here[0]).hypot(at[1] - here[1]);
}

// ---- builtins ---------------------------------------------------------------

fn actor_of(vm: &Vm<World>, world: &World, s: &Value) -> Option<u32> {
    let n = entnum(vm, s)?;
    world.resource::<Zm>().actors.contains_key(&n).then_some(n)
}

/// A point, an entity's origin or a struct's (`origin` field).
fn arg_vec(vm: &mut Vm<World>, world: &mut World, v: Option<&Value>) -> Option<[f32; 3]> {
    match v? {
        Value::Vec3(p) => Some(*p),
        v @ Value::Object(o) => origin_of(vm, world, v).or_else(|| {
            let f = vm.intern("origin");
            vm.raw_field(*o, f).as_vec3()
        }),
        _ => None,
    }
}

/// A traversal link of the nav mesh that starts within 48 units of the
/// node's origin, on (its cost as linked) or off (out of every path).
fn set_traversal(vm: &mut Vm<World>, world: &mut World, a: &[Value], on: bool) -> Value {
    let Some(p) = arg_vec(vm, world, a.first()) else { return Value::Undefined };
    if let Some(nav) = world.get_resource_mut::<T7Ai>().and_then(|s| s.into_inner().nav.as_mut()) {
        for l in nav.links.iter_mut().filter(|l| l.kind != "zbarrier" && d2(l.from, p) < 48.0 * 48.0) {
            l.cost = if on { 64.0 } else { 1.0e9 };
        }
    }
    Value::Undefined
}

/// The element of `a[1]` (entities, structs, points) closest to (or
/// farthest from) the point `a[0]`, within `a[2]` when given.
fn array_extreme(vm: &mut Vm<World>, world: &mut World, a: &[Value], far: bool) -> Value {
    let (Some(o), Some(Value::Array(arr))) = (arg_vec(vm, world, a.first()), a.get(1)) else { return Value::Undefined };
    let max = a.get(2).and_then(Value::as_float).unwrap_or(f32::MAX);
    let vals: Vec<Value> = arr.snapshot().values_in_order().cloned().collect();
    let mut best: Option<(f32, Value)> = None;
    for v in vals {
        let Some(p) = arg_vec(vm, world, Some(&v)) else { continue };
        let d = d2(p, o).sqrt();
        if d > max {
            continue;
        }
        if best.as_ref().is_none_or(|(bd, _)| if far { d > *bd } else { d < *bd }) {
            best = Some((d, v));
        }
    }
    best.map_or(Value::Undefined, |(_, v)| v)
}

pub(super) fn bind(vm: &mut Vm<World>) {
    vm.bind("finishvehicledamage", true, finish_vehicle_damage);
    // gettagorigin: a Black Ops III actor's guessed tag (`tag_guess`) where
    // its skeleton is not loaded; else as before.
    static BASE_TAG: std::sync::OnceLock<gsc_t6::Native<World>> = std::sync::OnceLock::new();
    if let Some(base) = vm.native_of("gettagorigin", true) {
        let _ = BASE_TAG.set(base);
    }
    vm.bind("gettagorigin", true, |vm, world, s, a| {
        if let Some(n) = super::entnum(vm, s) {
            let tag = a.first().map(|v| vm.to_text(v)).unwrap_or_else(String::new);
            if let Some(p) = tag_guess(world, n, &tag) {
                return Ok(Value::Vec3(p));
            }
        }
        BASE_TAG.get().map_or(Ok(Value::Undefined), |f| f(vm, world, s, a))
    });
    // getpartname(model, bone index): a guessed tag's name for its index.
    vm.bind("getpartname", false, |vm, _, _, a| {
        let k = a.get(1).and_then(Value::as_int).unwrap_or(0);
        let name = usize::try_from(k - 1000).ok().and_then(|k| GUESSED_TAGS.get(k)).copied().unwrap_or("");
        Ok(vm.string(name))
    });
    vm.bind("ispaused", true, |_, _, _, _| Ok(Value::Int(0)));
    vm.bind("animscripted", true, animscripted);
    // spawnactor(aitype, origin, angles, [targetname], ...): an actor of
    // that AI type made where asked (Revelations' Apothicon Furies out of
    // a corruption engine), from the map's spawner of that type.
    vm.bind("spawnactor", false, |vm, world, _, a| {
        let ai = a.first().map(|v| vm.to_text(v)).unwrap_or_default();
        let class = format!("actor_{ai}");
        let own = world.resource::<Zm>().ents.iter().filter(|(_, e)| e.classname.eq_ignore_ascii_case(&class)).filter_map(|(_, e)| e.obj).next();
        // (An AI type the map places no spawner of, the Furies: another
        // actor spawner's set-up under that type's class.)
        let template = own.or_else(|| world.resource::<Zm>().ents.iter().filter(|(_, e)| e.classname.starts_with("actor_spawner_")).min_by_key(|(n, _)| **n).and_then(|(_, e)| e.obj));
        let Some(spawner) = template else { return Ok(Value::Undefined) };
        // (Whatever the spawner's count: the call makes one.)
        let count = vm.intern("count");
        let kept = vm.raw_field(spawner, count);
        vm.set_raw_field(spawner, count, Value::Int(i32::MAX));
        let args = if own.is_some() { Vec::new() } else { vec![vm.string(&class)] };
        let made = spawn_from_spawner(vm, world, spawner, &args);
        vm.set_raw_field(spawner, count, kept);
        let Some(obj) = made else { return Ok(Value::Undefined) };
        if let ObjKind::Entity(n) = vm.kind(obj).unwrap_or(ObjKind::Level)
            && let Some(e) = world.resource_mut::<Zm>().ents.get_mut(&n)
        {
            if let Some(p) = a.get(1).and_then(Value::as_vec3) {
                e.origin = p;
            }
            if let Some(g) = a.get(2).and_then(Value::as_vec3) {
                e.angles = g;
            }
        }
        if let Some(t) = a.get(3).filter(|v| !matches!(v, Value::Undefined)) {
            let f = vm.intern("targetname");
            vm.set_raw_field(obj, f, t.clone());
        }
        Ok(Value::Object(obj))
    });
    // Player movement toggles Black Ops II has no movement for.
    for name in ["allowdoublejump", "allowwallrun", "makeusable", "makeunusable"] {
        vm.bind(name, true, |_, _, _, _| Ok(Value::Undefined));
    }
    vm.bind("stopanimscripted", true, |vm, world, s, _| {
        if let (Value::Object(o), Some(mut st)) = (s, world.get_resource_mut::<T7Ai>()) {
            st.anim_ends.retain(|e| e.1 != *o);
            st.anim_notes.retain(|e| e.1 != *o);
        }
        let _ = vm;
        Ok(Value::Undefined)
    });
    // trigger useby(player): as though he used it (its "trigger" notify).
    vm.bind("useby", true, |vm, world, s, a| {
        if let Value::Object(t) = s {
            let who = a.first().cloned().unwrap_or(Value::Undefined);
            vm.notify_str(world, *t, "trigger", &[who]);
        }
        Ok(Value::Undefined)
    });
    vm.bind("codeincrementclientfield", false, |_, _, _, _| Ok(Value::Undefined));
    // The vehicle builtins Black Ops III's vehicle scripts ask (no riders,
    // full damage, no immunity).
    vm.bind("getvehoccupants", true, |_, _, _, _| Ok(Value::array(gsc_t6::Array::new())));
    vm.bind("getoccupantseat", true, |_, _, _, _| Ok(Value::Undefined));
    vm.bind("isvehicleimmunetodamage", true, |_, _, _, _| Ok(Value::Int(0)));
    // (`getvehicledamagemultiplier(means)`, by its use.)
    vm.bind("#8639c416", true, |_, _, _, _| Ok(Value::Float(1.0)));
    vm.bind("setlookatent", true, |_, _, _, _| Ok(Value::Undefined));
    vm.bind("clearlookatent", true, |_, _, _, _| Ok(Value::Undefined));
    vm.bind("setdrawinfrared", true, |_, _, _, _| Ok(Value::Undefined));
    vm.bind("playhitmarker", true, |_, _, _, _| Ok(Value::Undefined));
    // isairborne(vehicle): a flying one (parasites, the Apothicon Sword's
    // glaive, drones), by its vehicle type.
    vm.bind("isairborne", false, |vm, world, _, a| {
        let Some(Value::Object(o)) = a.first() else { return Ok(Value::Int(0)) };
        let n = super::entnum(vm, &Value::Object(*o));
        let t7 = n.is_some_and(|n| world.get_resource::<T7Ai>().is_some_and(|s| s.spawned_vehicles.contains(&n)));
        let vt = super::t7::field_name(vm, "vehicletype");
        let vt = vm.to_text(&vm.raw_field(*o, vt));
        Ok(Value::Int(i32::from(t7 && ["parasite", "wasp", "glaive", "drone", "hunter"].iter().any(|k| vt.contains(k)))))
    });
    // mapfloat(in_min, in_max, out_min, out_max, value): clamped linear map.
    vm.bind("mapfloat", false, |_, _, _, a| {
        let f = |i: usize| a.get(i).and_then(Value::as_float).unwrap_or(0.0);
        let (a0, a1, b0, b1, v) = (f(0), f(1), f(2), f(3), f(4));
        let t = if (a1 - a0).abs() > f32::EPSILON { ((v - a0) / (a1 - a0)).clamp(0.0, 1.0) } else { 0.0 };
        Ok(Value::Float(b0 + (b1 - b0) * t))
    });
    vm.bind("isanimlooping", false, |_, _, _, _| Ok(Value::Int(0)));
    vm.bind("getanimtime", true, |_, _, _, _| Ok(Value::Float(0.0)));
    vm.bind("getanimframecount", false, |vm, world, _, a| {
        let name = a.first().map(|v| vm.to_text(v)).unwrap_or_else(String::new);
        let ms = world.resource::<Zm>().anims.get(&name).map_or(1000, super::actors::anim_length_ms);
        Ok(Value::Int((ms as f32 * 0.03).round() as i32))
    });
    // Camera animations (cinematics: Die Rise's intro, scene cameras): no
    // camera here, the scene goes on and its wait is the clip's length
    // when the clip is known, else none (`getcamanimtime`, unnamed:
    // `camanimscripted` and the others below it, named).
    for name in ["camanimscripted", "endcamanimscripted", "extracamanimscripted", "endextracamanimscripted"] {
        vm.bind(name, false, |_, _, _, _| Ok(Value::Undefined));
    }
    vm.bind("#5f3e774c", false, |vm, world, _, a| {
        let name = a.first().map(|v| vm.to_text(v)).unwrap_or_else(String::new);
        let ms = world.resource::<Zm>().anims.get(&name).map_or(0, super::actors::anim_length_ms);
        Ok(Value::Int(ms as i32))
    });
    // Stats, UI and presentation calls with no server-side effect.
    // `getitemindexfromref` / `getbaseweaponitemindex` index the stat
    // tables (GobbleGums register theirs through them: no GobbleGums here,
    // so any index will do); `weaponhasattachment` finds none.
    vm.bind("getitemindexfromref", false, |_, _, _, _| Ok(Value::Int(0)));
    vm.bind("getbaseweaponitemindex", false, |_, _, _, _| Ok(Value::Int(0)));
    vm.bind("weaponhasattachment", false, |_, _, _, _| Ok(Value::Int(0)));
    vm.bind("currentsessionmode", false, |_, _, _, _| Ok(Value::Int(2)));
    vm.bind("hasseasonpass", true, |_, _, _, _| Ok(Value::Int(0)));
    vm.bind("getipaddress", true, |vm, _, _, _| Ok(vm.string("")));
    for name in ["sethighdetail", "setlightingstate", "refreshshieldattachment", "setcharacterbodytype", "setcharacterbodystyle", "setcharacterhelmetstyle", "allowslide", "zbarrierpieceuseattachweapon", "uploadleaderboards"] {
        vm.bind(name, true, |_, _, _, _| Ok(Value::Undefined));
    }
    for name in ["recordgameresult", "recordcomscoreevent"] {
        vm.bind(name, false, |_, _, _, _| Ok(Value::Undefined));
    }
    // Client-side effects and UI counters: nothing for the server to do.
    for name in ["activateclientradiantexploder", "deactivateclientradiantexploder", "codeincrementuimodelclientfield"] {
        vm.bind(name, false, |_, _, _, _| Ok(Value::Undefined));
    }
    // isalive(vehicle): a Black Ops III vehicle until it is killed (its
    // wreck stays an entity); everything else as before.
    static BASE_ISALIVE: std::sync::OnceLock<gsc_t6::Native<World>> = std::sync::OnceLock::new();
    if let Some(base) = vm.native_of("isalive", false) {
        let _ = BASE_ISALIVE.set(base);
    }
    vm.bind("isalive", false, |vm, world, s, a| {
        if let Some(n) = a.first().and_then(|v| entnum(vm, v)) {
            if let Some(st) = world.get_resource::<T7Ai>() {
                if st.vehicles.contains(&n) {
                    return Ok(Value::Int(1));
                }
                if world.resource::<Zm>().ents.get(&n).is_some_and(|e| e.classname == "script_vehicle") && st.spawned_vehicles.contains(&n) {
                    return Ok(Value::Int(0));
                }
            }
        }
        BASE_ISALIVE.get().map_or(Ok(Value::Int(0)), |f| f(vm, world, s, a))
    });
    vm.bind("isactor", false, |vm, world, _, a| Ok(Value::Int(i32::from(a.first().and_then(|v| actor_of(vm, world, v)).is_some()))));
    for name in ["aiprofile_beginentry", "aiprofile_endentry"] {
        vm.bind(name, false, |_, _, _, _| Ok(Value::Undefined));
    }
    // arraygetclosest / arraygetfarthest(origin, array [, max distance]).
    for (name, far) in [("arraygetclosest", false), ("arraygetfarthest", true)] {
        let f: gsc_t6::Native<World> = if far {
            |vm, world, _, a| Ok(array_extreme(vm, world, a, true))
        } else {
            |vm, world, _, a| Ok(array_extreme(vm, world, a, false))
        };
        vm.bind(name, false, f);
    }
    // arraysortclosest(array, origin [, max [, mindist [, maxdist]]]).
    vm.bind("arraysortclosest", false, |vm, world, _, a| {
        let Some(Value::Array(arr)) = a.first() else { return Ok(super::list(Vec::new())) };
        let Some(o) = arg_vec(vm, world, a.get(1)) else { return Ok(a[0].clone()) };
        let max = a.get(2).and_then(Value::as_int).map_or(usize::MAX, |m| usize::try_from(m).unwrap_or(0));
        let lo = a.get(3).and_then(Value::as_float).unwrap_or(0.0);
        let hi = a.get(4).and_then(Value::as_float).unwrap_or(f32::MAX);
        let vals: Vec<Value> = arr.snapshot().values_in_order().cloned().collect();
        let mut with: Vec<(f32, Value)> = vals
            .into_iter()
            .filter_map(|v| {
                let p = arg_vec(vm, world, Some(&v))?;
                let d = d2(p, o).sqrt();
                (d >= lo && d <= hi).then_some((d, v))
            })
            .collect();
        with.sort_by(|x, y| x.0.total_cmp(&y.0));
        Ok(super::list(with.into_iter().take(max).map(|(_, v)| v).collect()))
    });
    vm.bind("spawnfromspawner", true, |vm, world, s, a| {
        let Value::Object(sp) = s else { return Ok(Value::Undefined) };
        Ok(spawn_from_spawner(vm, world, *sp, a).map_or(Value::Undefined, Value::Object))
    });
    vm.bind("isactorspawner", false, |vm, world, _, a| {
        let n = a.first().and_then(|v| entnum(vm, v));
        Ok(Value::Int(i32::from(n.is_some_and(|n| world.resource::<Zm>().ents.get(&n).is_some_and(|e| e.classname.starts_with("actor_spawner"))))))
    });
    vm.bind("getactorspawnerteamarray", false, |vm, world, _, _| {
        let zm = world.resource::<Zm>();
        let v = zm.ents.values().filter(|e| e.classname.starts_with("actor_spawner")).filter_map(|e| e.obj).filter(|o| vm.alive(*o)).map(Value::Object).collect();
        Ok(super::list(v))
    });
    // getaiarchetypearray(archetype [, team]).
    vm.bind("getaiarchetypearray", false, |vm, world, _, a| {
        let want = a.first().map(|v| vm.to_text(v)).unwrap_or_else(String::new);
        let ids: Vec<(u32, ObjRef)> = world.resource::<Zm>().actors.iter().filter(|(_, x)| x.alive).filter_map(|(n, x)| x.obj.map(|o| (*n, o))).collect();
        let Some(s) = world.get_resource::<T7Ai>() else { return Ok(super::list(Vec::new())) };
        let v = ids.into_iter().filter(|(n, o)| vm.alive(*o) && s.ai.get(n).is_some_and(|x| x.archetype == want)).map(|(_, o)| Value::Object(o)).collect();
        Ok(super::list(v))
    });
    // getclosestpointonnavmesh(point [, radius = 128 [, ...]]).
    vm.bind("getclosestpointonnavmesh", false, |vm, world, _, a| {
        let Some(p) = arg_vec(vm, world, a.first()) else { return Ok(Value::Undefined) };
        let r = a.get(1).and_then(Value::as_float).unwrap_or(128.0);
        let q = world.get_resource::<T7Ai>().and_then(|s| s.nav.as_ref().and_then(|nav| closest_point(nav, p, r)));
        Ok(q.map_or(Value::Undefined, Value::Vec3))
    });
    vm.bind("ispointonnavmesh", false, |vm, world, _, a| {
        let Some(p) = arg_vec(vm, world, a.first()) else { return Ok(Value::Int(0)) };
        let tol = a.get(1).and_then(Value::as_float).unwrap_or(36.0).max(18.0);
        let on = world.get_resource::<T7Ai>().is_some_and(|s| s.nav.as_ref().is_some_and(|nav| nav.locate(p, tol, tol).is_some()));
        Ok(Value::Int(i32::from(on)))
    });
    // self setgoal(point | entity | node [, force [, radius]]): on the nav
    // mesh (a goal far under the floor, from a ground trace that found no
    // ground, is lifted onto the mesh above it).
    vm.bind("setgoal", true, |vm, world, s, a| {
        // (A vehicle's goal is its anchor for the position queries, with
        // the engine's `goalpos` / `goalradius` / `goalheight` following.)
        if let Some(v) = entnum(vm, s).filter(|v| world.get_resource::<T7Ai>().is_some_and(|st| st.vehicles.contains(v)))
            && let Some(goal) = arg_vec(vm, world, a.first())
        {
            if let Some(mut st) = world.get_resource_mut::<T7Ai>() {
                st.vehicle_goal.insert(v, goal);
            }
            if let Value::Object(o) = s {
                let fp = super::t7::field_name(vm, "goalpos");
                vm.set_raw_field(*o, fp, Value::Vec3(goal));
                for (i, k) in [(2, "goalradius"), (3, "goalheight")] {
                    if let Some(r) = a.get(i).and_then(Value::as_float) {
                        let f = vm.intern(k);
                        vm.set_raw_field(*o, f, Value::Float(r));
                    }
                }
            }
            return Ok(Value::Undefined);
        }
        let Some(n) = actor_of(vm, world, s) else { return Ok(Value::Undefined) };
        let Some(mut goal) = arg_vec(vm, world, a.first()) else { return Ok(Value::Undefined) };
        if let Some(on) = world.get_resource::<T7Ai>().and_then(|s| {
            let nav = s.nav.as_ref()?;
            let f = nav.locate(goal, 64.0, 64.0).or_else(|| nav.locate(goal, 64.0, 200_000.0))?;
            Some([goal[0], goal[1], nav.height(f, goal[0], goal[1])])
        }) {
            goal = on;
        }
        if let Some(x) = world.resource_mut::<Zm>().actors.get_mut(&n) {
            let same = x.goal.is_some_and(|g| d2(g, goal) < 1.0);
            x.goal = Some(goal);
            if !same {
                x.goal_sent = false;
            }
        }
        Ok(Value::Undefined)
    });
    // self calcapproximatepathtoposition(goal [, ...]): the nav mesh path's
    // points from where it stands, or undefined.
    vm.bind("calcapproximatepathtoposition", true, |vm, world, s, a| {
        let Some(n) = actor_of(vm, world, s) else { return Ok(Value::Undefined) };
        let Some(goal) = arg_vec(vm, world, a.first()) else { return Ok(Value::Undefined) };
        let Some(here) = world.resource::<Zm>().ents.get(&n).map(|e| e.origin) else { return Ok(Value::Undefined) };
        let path = world.get_resource::<T7Ai>().and_then(|st| st.nav.as_ref().and_then(|nav| nav.find_path(here, goal)));
        let Some(path) = path else { return Ok(Value::Undefined) };
        let mut pts = vec![Value::Vec3(here)];
        pts.extend(path.into_iter().map(|w| Value::Vec3(w.point)));
        Ok(super::list(pts))
    });
    // The animation state machine's requests (`animationstatenetworkutility`):
    // taken; the clips are not played yet.
    vm.bind("asmrequestsubstate", true, |vm, world, s, a| {
        // (A vehicle's: its attack substate gives its notes in time.)
        if let Some(n) = super::entnum(vm, s)
            && world.get_resource::<T7Ai>().is_some_and(|st| st.vehicles.contains(&n))
        {
            let sub = a.first().map(|v| vm.to_text(v)).unwrap_or_else(String::new);
            let now = world.resource::<Zm>().now_ms;
            world.resource_mut::<T7Ai>().vehicle_asm.insert(n, (sub, now, 0));
        }
        Ok(vm.string("asm_status_running"))
    });
    vm.bind("setturrettargetent", true, |vm, world, s, a| {
        if let (Some(n), Some(Value::Object(t))) = (super::entnum(vm, s), a.first())
            && let Some(mut st) = world.get_resource_mut::<T7Ai>()
        {
            st.turret_target.insert(n, *t);
        }
        Ok(Value::Undefined)
    });
    vm.bind("clearturrettarget", true, |vm, world, s, _| {
        if let (Some(n), Some(mut st)) = (super::entnum(vm, s), world.get_resource_mut::<T7Ai>()) {
            st.turret_target.remove(&n);
        }
        Ok(Value::Undefined)
    });
    vm.bind("istring", false, |_, _, _, a| Ok(a.first().cloned().unwrap_or(Value::Undefined)));
    // self kill(): dies at once (the scripts' clean-up of a zombie that
    // fell behind, `kill_zombie`).
    // (Black Ops III's `kill([origin, attacker, inflictor, weapon])`; with
    // no attacker the engine names the world, as IW engines hand
    // ENTITYNUM_WORLD: `_zm`'s killed callback reads `attacker.classname`.)
    vm.bind("kill", true, |vm, world, s, a| {
        let Some(n) = actor_of(vm, world, s) else { return Ok(Value::Undefined) };
        let health = world.resource::<Zm>().actors.get(&n).map_or(0, |a| a.health);
        let (means, none) = (vm.string("MOD_SUICIDE"), vm.string("none"));
        let attacker = match a.get(1) {
            Some(v) if !v.is_undefined() => v.clone(),
            _ => Value::Object(vm.world),
        };
        let inflictor = match a.get(2) {
            Some(v) if !v.is_undefined() => v.clone(),
            _ => attacker.clone(),
        };
        let weapon = match a.get(3) {
            Some(v) if !v.is_undefined() => super::t7::weapon_value(vm, world, v.clone()),
            _ => super::t7::weapon_value(vm, world, none.clone()),
        };
        let args = vec![inflictor, attacker.clone(), Value::Int(health), means, weapon, Value::Vec3([0.0; 3]), none, Value::Int(0)];
        super::actors::die(vm, world, n, attacker, args);
        Ok(Value::Undefined)
    });
    // positionquery_source_navigation(origin, min radius, max radius, half
    // height, spacing [, entity [, outer spacing]]): points on the nav mesh
    // in that ring (a struct whose `data` holds one struct per point, its
    // `origin` and `disttoorigin2d`).
    vm.bind("positionquery_source_navigation", false, |vm, world, _, a| {
        let Some(o) = arg_vec(vm, world, a.first()) else { return Ok(Value::Undefined) };
        let lo = a.get(1).and_then(Value::as_float).unwrap_or(0.0).max(0.0);
        let hi = a.get(2).and_then(Value::as_float).unwrap_or(128.0).max(lo + 1.0);
        let half = a.get(3).and_then(Value::as_float).unwrap_or(36.0).max(8.0);
        let step = a.get(4).and_then(Value::as_float).unwrap_or(16.0).max(8.0);
        let mut pts: Vec<[f32; 3]> = Vec::new();
        if let Some(nav) = world.get_resource::<T7Ai>().and_then(|s| s.nav.as_ref()) {
            let mut r = lo.max(step * 0.5);
            while r <= hi && pts.len() < 64 {
                let count = ((std::f32::consts::TAU * r) / step).floor().max(1.0) as usize;
                for k in 0..count {
                    let t = k as f32 / count as f32 * std::f32::consts::TAU;
                    let p = [o[0] + r * t.cos(), o[1] + r * t.sin(), o[2]];
                    if let Some(f) = nav.locate(p, half, half) {
                        pts.push([p[0], p[1], nav.height(f, p[0], p[1])]);
                    }
                }
                r += step;
            }
        }
        let centre_on_nav = world.get_resource::<T7Ai>().and_then(|s| s.nav.as_ref()).is_some_and(|nav| nav.locate(o, half, half).is_some());
        let mut data = gsc_t6::Array::new();
        // (Field names as the scripts hash them: the engine's point has
        // origin, distToOrigin2D, inClaimedLocation, score and _scoreDebug.)
        let fo = vm.intern("origin");
        let (fd, fc, fs, fsd) = (super::t7::field_name(vm, "disttoorigin2d"), super::t7::field_name(vm, "inclaimedlocation"), super::t7::field_name(vm, "score"), super::t7::field_name(vm, "_scoredebug"));
        for p in pts {
            let Value::Object(e) = vm.new_struct() else { continue };
            vm.set_raw_field(e, fo, Value::Vec3(p));
            vm.set_raw_field(e, fd, Value::Float(((p[0] - o[0]).powi(2) + (p[1] - o[1]).powi(2)).sqrt()));
            vm.set_raw_field(e, fc, Value::Int(0));
            vm.set_raw_field(e, fs, Value::Float(0.0));
            vm.set_raw_field(e, fsd, Value::array(gsc_t6::Array::new()));
            data.push(Value::Object(e));
        }
        let q = vm.new_struct();
        if let Value::Object(qo) = q {
            let (fd, fo2, fcn) = (vm.intern("data"), vm.intern("origin"), super::t7::field_name(vm, "centeronnav"));
            vm.set_raw_field(qo, fd, Value::array(data));
            vm.set_raw_field(qo, fo2, Value::Vec3(o));
            vm.set_raw_field(qo, fcn, Value::Int(i32::from(centre_on_nav)));
        }
        Ok(q)
    });
    // self vehcansee(entity): a clear line from the vehicle to it (the
    // parasites' and wasps' check before they spit).
    vm.bind("vehcansee", true, |vm, world, s, a| {
        let (Some(from), Some(to)) = (super::origin_of(vm, world, s), a.first().and_then(|v| super::origin_of(vm, world, v))) else { return Ok(Value::Int(0)) };
        let to = [to[0], to[1], to[2] + 40.0];
        let t = super::frame(world).trace_static_world(from, to, [0.0; 3], [0.0; 3], crate::bullet_collision::MASK_SHOT);
        Ok(Value::Int(i32::from(t.fraction >= 1.0)))
    });
    // A gunner seat's target and the laser sight: nothing to draw here.
    for name in ["cleargunnertarget", "laseroff", "laseron"] {
        vm.bind(name, true, |_, _, _, _| Ok(Value::Undefined));
    }
    // positionquery_filter_distancetogoal(query, entity): each point's
    // distance to the entity's goal (a vehicle's anchor, an actor's goal,
    // else where it is), as `distToGoal`.
    vm.bind("positionquery_filter_distancetogoal", false, |vm, world, _, a| {
        let (Some(Value::Object(q)), Some(ent)) = (a.first(), a.get(1)) else { return Ok(Value::Undefined) };
        let Some(n) = entnum(vm, ent) else { return Ok(Value::Undefined) };
        let goal = world
            .get_resource::<T7Ai>()
            .and_then(|s| s.vehicle_goal.get(&n).copied())
            .or_else(|| world.resource::<Zm>().actors.get(&n).and_then(|x| x.goal))
            .or_else(|| world.resource::<Zm>().ents.get(&n).map(|e| e.origin));
        let Some(goal) = goal else { return Ok(Value::Undefined) };
        let fdata = vm.intern("data");
        let Value::Array(data) = vm.raw_field(*q, fdata) else { return Ok(Value::Undefined) };
        let (fo, fg) = (vm.intern("origin"), super::t7::field_name(vm, "disttogoal"));
        for v in data.snapshot().values_in_order().cloned().collect::<Vec<_>>() {
            let Value::Object(p) = v else { continue };
            let Some(at) = vm.raw_field(p, fo).as_vec3() else { continue };
            vm.set_raw_field(p, fg, Value::Float(d2(at, goal).sqrt()));
        }
        Ok(Value::Undefined)
    });
    // positionquery_filter_inclaimedlocation(query, entity): nothing is
    // claimed (no path claims yet).
    vm.bind("positionquery_filter_inclaimedlocation", false, |_, _, _, _| Ok(Value::Undefined));
    // getstructfield(struct, name): a struct's field by its name (bundles'
    // settings).
    vm.bind("getstructfield", false, |vm, _, _, a| {
        let (Some(Value::Object(o)), Some(name)) = (a.first(), a.get(1)) else { return Ok(Value::Undefined) };
        let name = vm.to_text(name).to_ascii_lowercase();
        let f = super::t7::field_name(vm, &name);
        Ok(vm.raw_field(*o, f))
    });
    // linktraversal / unlinktraversal(node): the traversal that starts at
    // that node in or out of the nav mesh's paths (a gate's jump, a bridge).
    for (name, on) in [("linktraversal", true), ("unlinktraversal", false)] {
        let f: gsc_t6::Native<World> = if on { |vm, world, _, a| Ok(set_traversal(vm, world, a, true)) } else { |vm, world, _, a| Ok(set_traversal(vm, world, a, false)) };
        vm.bind(name, false, f);
    }
    // setenablenode(node, on): Black Ops III's cover and path nodes; the
    // zombies walk the nav mesh, nothing to do.
    vm.bind("setenablenode", false, |_, _, _, _| Ok(Value::Undefined));
    vm.bind("isnotarget", true, |_, _, _, _| Ok(Value::Int(0)));
    vm.bind("isvehicle", false, |vm, world, _, a| {
        let n = a.first().and_then(|v| entnum(vm, v));
        Ok(Value::Int(i32::from(n.is_some_and(|n| world.resource::<Zm>().ents.get(&n).is_some_and(|e| e.classname == "script_vehicle")))))
    });
    // self pathmode("move allowed" | "dont move" | ...): kept on the blackboard.
    vm.bind("pathmode", true, |vm, world, s, a| {
        let Some(n) = actor_of(vm, world, s) else { return Ok(Value::Undefined) };
        let mode = a.first().cloned().unwrap_or(Value::Undefined);
        if let Some(mut st) = world.get_resource_mut::<T7Ai>() {
            if let Some(x) = st.ai.get_mut(&n) {
                x.blackboard.insert("__pathmode".to_owned(), mode);
            }
        }
        Ok(Value::Undefined)
    });
    vm.bind("getpathmode", true, |vm, world, s, _| {
        let Some(n) = actor_of(vm, world, s) else { return Ok(Value::Undefined) };
        let m = world.get_resource::<T7Ai>().and_then(|st| st.ai.get(&n).and_then(|x| x.blackboard.get("__pathmode").cloned()));
        Ok(m.unwrap_or_else(|| vm.string("move allowed")))
    });
    vm.bind("isatgoal", true, |vm, world, s, _| {
        let Some(n) = actor_of(vm, world, s) else { return Ok(Value::Int(0)) };
        let zm = world.resource::<Zm>();
        let (Some(g), Some(p)) = (zm.actors.get(&n).and_then(|a| a.goal), zm.ents.get(&n).map(|e| e.origin)) else { return Ok(Value::Int(0)) };
        let obj = zm.actors.get(&n).and_then(|a| a.obj);
        let f = vm.intern("goalradius");
        let radius = obj.map_or(16.0, |o| vm.raw_field(o, f).as_float().unwrap_or(16.0));
        Ok(Value::Int(i32::from(((g[0] - p[0]).powi(2) + (g[1] - p[1]).powi(2)).sqrt() <= radius.max(4.0))))
    });
    vm.bind("haspath", true, |vm, world, s, _| {
        let Some(n) = actor_of(vm, world, s) else { return Ok(Value::Int(0)) };
        Ok(Value::Int(i32::from(ai_state(world, n).is_some_and(|a| !a.path.is_empty()))))
    });
    vm.bind("clearpath", true, |vm, world, s, _| {
        if let Some(n) = actor_of(vm, world, s) {
            if let Some(x) = world.resource_mut::<Zm>().actors.get_mut(&n) {
                x.goal = None;
            }
        }
        Ok(Value::Undefined)
    });
    // setblackboardattribute(entity, name, value) / getblackboardattribute.
    vm.bind("setblackboardattribute", false, |vm, world, _, a| {
        let Some(n) = a.first().and_then(|v| actor_of(vm, world, v)) else { return Ok(Value::Undefined) };
        let name = a.get(1).map(|v| vm.to_text(v)).unwrap_or_else(String::new);
        let v = a.get(2).cloned().unwrap_or(Value::Undefined);
        if let Some(mut s) = world.get_resource_mut::<T7Ai>() {
            if let Some(x) = s.ai.get_mut(&n) {
                x.blackboard.insert(name, v);
            }
        }
        Ok(Value::Undefined)
    });
    vm.bind("getblackboardattribute", false, |vm, world, _, a| {
        let Some(n) = a.first().and_then(|v| actor_of(vm, world, v)) else { return Ok(Value::Undefined) };
        let name = a.get(1).map(|v| vm.to_text(v)).unwrap_or_else(String::new);
        Ok(world.get_resource::<T7Ai>().and_then(|s| s.ai.get(&n).and_then(|x| x.blackboard.get(&name).cloned())).unwrap_or(Value::Undefined))
    });
}
