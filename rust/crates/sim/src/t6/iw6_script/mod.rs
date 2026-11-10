//! bo2mp: a Ghosts (IW6) map's events, run from the map's own compiled scripts.
//!
//! The map load hands over the scripts of the map, patch and common zones and the map's entity
//! blocks (`asset_world::iw6_scripts`). `gsc_iw6_vm` runs them over `host::Iw6Host`, whose
//! entities are the map's. This module is the glue to the sim:
//!
//! - a tick's work is split around `movers::advance`: `pre` hands the script this tick's events
//!   (door-switch presses, damage, players joining), steps the VM and applies what the builtins
//!   asked (`host::Effect`: movers, sounds, effects, exploders, hide/show/delete, ...);
//!   `post` sees which tracks finished and says `movedone` / `rotatedone`;
//! - the sim's `Ent` is the body: scripts move it through `Zm.movers`, collision follows;
//! - Ghosts' renderer is told where every script-touched model stands
//!   (`asset_world::set_iw6_mover_poses`, with hidden / deleted).
//!
//! - entities a script makes (`spawn`) get sim entities too (damage taps, triggers); a model the
//!   map zone holds is drawn through one of the renderer's hidden instances of it;
//! - effects a script names play by the catalog name the assets lane converted them to
//!   (`Iw6ScriptPack::script_fx`); looping and bolted ones are rate limited and bounded.
//!
//! `BO2MP_IW6_SCRIPTS=0` turns it off (the door driver `iw6_doors` then works as before).

pub mod anim;
pub mod boot;
pub mod builtins;
pub mod census;
pub mod events;
pub mod host;
pub mod player;

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::Instant;

use bevy_ecs::prelude::{Resource, World};
use gsc_iw6_vm::{EntityId, Name, Value, Vm};

use self::host::{Effect, HostEnt, Iw6Host};
use super::movers::Track;
use super::{Ent, Zm};
use crate::world::ClientId;

/// The map's scripts run (on; `BO2MP_IW6_SCRIPTS=0` leaves the doors to `iw6_doors`).
pub(crate) fn on() -> bool {
    std::env::var("BO2MP_IW6_SCRIPTS").map_or(true, |v| v.trim() != "0")
}

/// Whether the script runtime drives this map (so `iw6_doors` stands aside).
pub(super) fn active(world: &World) -> bool {
    world.contains_resource::<Iw6Script>()
}

/// Damage the sim resolved onto an entity, to be handed to the scripts.
#[derive(Clone, Debug)]
struct Damage {
    ent: u32,
    amount: i32,
    attacker: u32,
    dir: [f32; 3],
    point: [f32; 3],
    means: &'static str,
    weapon: String,
}

/// An entity linked to a bone (see `Iw6Script::bone_links`).
#[derive(Clone, Debug)]
struct BoneLink {
    parent: u32,
    tag: String,
    local: iw6_pose::Bone,
}

/// An exploder effect waiting for its delay.
#[derive(Clone, Debug)]
struct Pending {
    due: i64,
    effect: String,
    origin: [f32; 3],
    forward: [f32; 3],
    up: [f32; 3],
    sound: String,
}

/// `playloopedfx`: the effect plays again every `period_ms`.
#[derive(Clone, Debug)]
struct LoopFx {
    name: String,
    origin: [f32; 3],
    forward: [f32; 3],
    period_ms: i64,
    next: i64,
}

/// Effects played in one tick, at most (the rest are dropped).
const MAX_FX_PER_TICK: u32 = 24;
/// The least time between two plays of one effect bolted to one tag.
const MIN_FX_GAP_MS: i64 = 150;
/// Bolted effects remembered (for `stopfxontag` and deletes), at most.
const MAX_FX_KEYS: usize = 256;
/// `playloopedfx` loops, at most.
const MAX_LOOPS: usize = 32;

#[derive(Default, Clone, Debug)]
struct Stats {
    ticks: u64,
    total_us: u128,
    max_us: u128,
    events: u64,
    logged: bool,
}

#[derive(Resource)]
pub(crate) struct Iw6Script {
    vm: Vm<Iw6Host>,
    pack: Arc<asset_world::Iw6ScriptPack>,
    /// Host entity -> sim entity, and back.
    zm_of: HashMap<u32, u32>,
    host_of: HashMap<u32, u32>,
    /// Where the map placed each host entity (origin, angles).
    authored: HashMap<u32, ([f32; 3], [f32; 3])>,
    /// Tracks the host runs itself for entities the sim does not hold.
    own: HashMap<u32, (Option<Track>, Option<Track>)>,
    /// Entities linked to a bone of a skeleton: the sim child -> (parent, tag, the child's pose in
    /// the bone's frame). `movers::follow_links` places them by the bone's animated pose.
    bone_links: HashMap<u32, BoneLink>,
    /// Movers that cut or open the walkers' paths (`disconnectpaths` / `connectpaths`), by host
    /// entity; published to the bots' navigation.
    gates: std::collections::BTreeMap<u32, asset_world::Iw6PathGate>,
    /// Host entities with a sim track in flight (position, rotation).
    tracking: HashMap<u32, (bool, bool)>,
    /// Entities a script changed (their pose is published).
    touched: HashSet<u32>,
    presses: Vec<(u32, u32)>,
    damages: Vec<Damage>,
    /// Client -> host entity.
    players: HashMap<u32, u32>,
    prematch_sent: bool,
    /// `game_ended` went to the level once the match flag for it was set.
    game_ended_sent: bool,
    /// The Tactical Nuke's blast was told to the level (until its flag drops).
    moab_sent: bool,
    /// The Tactical Nuke's call for the map's own nuke hook was answered (until its flag drops).
    moab_call_sent: bool,
    exploders: Vec<Pending>,
    /// Bolted effects: (host entity, effect, tag) -> (where it last played, when).
    fx_keys: HashMap<(u32, String, String), ([f32; 3], i64)>,
    looped: Vec<LoopFx>,
    fx_budget: u32,
    /// The hidden instance of a model that draws a host entity: entity -> (model, slot).
    slots: HashMap<u32, (String, u32)>,
    /// Slots whose entity is gone: published hidden once more, then freed.
    retired: Vec<(String, u32)>,
    /// The model the map put on each entity (lower case).
    authored_model: HashMap<u32, String>,
    /// Builtins already named in the log.
    logged_missing: HashSet<(bool, u16)>,
    /// Spawned touch triggers: who stood inside last tick.
    inside: HashSet<(u32, u32)>,
    published: Vec<asset_world::Iw6MoverPose>,
    last_ms: Option<i64>,
    stats: Stats,
    errors_logged: usize,
}

/// Builds the runtime once the map's entities are in the sim.
fn boot(world: &mut World) {
    let Some(pack) = asset_world::iw6_scripts() else {
        return;
    };
    let zm_blocks: HashMap<u32, u32> = {
        let zm = world.resource::<Zm>();
        zm.ents.iter().filter_map(|(n, e)| e.map_ordinal.map(|o| (o, *n))).collect()
    };
    if zm_blocks.is_empty() {
        return;
    }
    let started = Instant::now();
    let data = boot::BootData { map: &pack.map, scripts: &pack.scripts, tables: &pack.tables, entities: &pack.entities };
    let (vm, rep) = match boot::boot(&data) {
        Ok(v) => v,
        Err(e) => {
            diag::info!(Sim, "iw6 scripts: the map script did not start: {e}");
            return;
        }
    };
    let mut vm = vm;
    vm.host_mut().anim.bank = anim::AnimBank::new(&pack.clips, &pack.skeletons);
    for m in &pack.models {
        vm.host_mut().models.insert(m.name.clone(), host::ModelInfo { mins: m.mins, maxs: m.maxs, tags: m.tags.clone() });
    }
    {
        // The sim's damage boxes of the map's models (the engine's own model table has none).
        let host = vm.host();
        let mut zm = world.resource_mut::<Zm>();
        for e in zm.ents.values_mut() {
            if e.bounds.is_none() && !e.model.is_empty() {
                if let Some(m) = host.models.get(e.model.trim_start_matches(',').to_ascii_lowercase().as_str()) {
                    e.bounds = Some((m.mins, m.maxs));
                }
            }
        }
    }
    let mut zm_of = HashMap::new();
    let mut host_of = HashMap::new();
    let mut authored = HashMap::new();
    let mut authored_model = HashMap::new();
    for (i, e) in vm.host().ents.iter().enumerate() {
        let id = i as u32;
        authored.insert(id, (vm.host().origin(id), vm.host().angles(id)));
        authored_model.insert(id, vm.host().model_of(id));
        if let Some(n) = e.block.and_then(|b| zm_blocks.get(&(b as u32))) {
            zm_of.insert(id, *n);
            host_of.insert(*n, id);
        }
    }
    diag::info!(
        Sim,
        "iw6 scripts: {} scripts from {} ({} script errors, {} unresolved includes), {} entities ({} in the sim), {} structs; main {}; booted in {} ms (BO2MP_IW6_SCRIPTS=0 turns it off)",
        rep.scripts,
        pack.zones.join("+"),
        rep.script_errors.len(),
        rep.unresolved_includes,
        rep.entities,
        zm_of.len(),
        rep.structs,
        if rep.main_found { "found" } else { "MISSING" },
        started.elapsed().as_millis()
    );
    for e in rep.script_errors.iter().take(8) {
        diag::info!(Sim, "iw6 scripts: {e}");
    }
    asset_world::set_iw6_path_gates(Vec::new());
    world.insert_resource(Iw6Script {
        vm,
        pack,
        zm_of,
        host_of,
        authored,
        own: HashMap::new(),
        bone_links: HashMap::new(),
        gates: Default::default(),
        tracking: HashMap::new(),
        touched: HashSet::new(),
        presses: Vec::new(),
        damages: Vec::new(),
        players: HashMap::new(),
        prematch_sent: false,
        game_ended_sent: false,
        moab_sent: false,
        moab_call_sent: false,
        exploders: Vec::new(),
        fx_keys: HashMap::new(),
        looped: Vec::new(),
        fx_budget: MAX_FX_PER_TICK,
        slots: HashMap::new(),
        retired: Vec::new(),
        authored_model,
        logged_missing: HashSet::new(),
        inside: HashSet::new(),
        published: Vec::new(),
        last_ms: None,
        stats: Stats::default(),
        errors_logged: 0,
    });
}

/// Use-trigger presses (sim trigger entity, client) this tick.
pub(super) fn note_presses(world: &mut World, presses: &[(u32, u32)]) {
    if presses.is_empty() {
        return;
    }
    if let Some(mut s) = world.get_resource_mut::<Iw6Script>() {
        s.presses.extend_from_slice(presses);
    }
}

/// Sim entities a shot or blast hit (the sim resolved them); `means` is the MOD name.
#[allow(clippy::too_many_arguments)]
pub(super) fn note_damage(
    world: &mut World,
    ents: &[u32],
    amount: i32,
    attacker: u32,
    dir: [f32; 3],
    point: [f32; 3],
    means: &'static str,
    weapon: &str,
) {
    if ents.is_empty() {
        return;
    }
    if let Some(mut s) = world.get_resource_mut::<Iw6Script>() {
        for &ent in ents {
            s.damages.push(Damage { ent, amount, attacker, dir, point, means, weapon: weapon.to_owned() });
        }
    }
}

/// Before `movers::advance`: events in, the VM stepped, effects out.
/// `level.nukeoverride`: the map's replacement for Ghosts' nuke (a function returning true when it
/// ran instead).
const F_LEVEL_NUKE_HOOK: u16 = 10679;

pub(super) fn pre(world: &mut World, now: i64) {
    if !on() {
        return;
    }
    if !active(world) {
        if world.get_resource::<Zm>().is_none_or(|z| z.ents.is_empty()) {
            return;
        }
        boot(world);
        if !active(world) {
            return;
        }
    }
    let t0 = Instant::now();
    let mut s = world.remove_resource::<Iw6Script>().expect("checked");
    let events = step(world, &mut s, now);
    s.stats.ticks += 1;
    s.stats.events += events;
    let us = t0.elapsed().as_micros();
    s.stats.total_us += us;
    s.stats.max_us = s.stats.max_us.max(us);
    if s.stats.ticks == 1200 && !s.stats.logged {
        s.stats.logged = true;
        diag::info!(
            Sim,
            "iw6 scripts: {} ticks, {} events, {} us per tick on average, {} us at most; unimplemented builtins {:?}",
            s.stats.ticks,
            s.stats.events,
            s.stats.total_us / u128::from(s.stats.ticks.max(1)),
            s.stats.max_us,
            s.vm.unimplemented_builtins()
        );
    }
    world.insert_resource(s);
}

fn players_in(world: &mut World) -> Vec<(u32, [f32; 3], [f32; 3])> {
    let clients: Vec<u32> = world.resource::<Zm>().players.keys().copied().collect();
    let mut out = Vec::new();
    for c in clients {
        if let Some(ps) = super::frame(world).player(ClientId(c)) {
            out.push((c, ps.origin, ps.viewangles));
        }
    }
    out
}

fn step(world: &mut World, s: &mut Iw6Script, now: i64) -> u64 {
    let mut events = 0u64;
    // Players: host entities, positions, `connected`.
    let mut joined = Vec::new();
    for (c, origin, angles) in players_in(world) {
        let id = match s.players.get(&c) {
            Some(id) => *id,
            None => {
                let mut pl = HostEnt { client: Some(c), solid: true, bot: super::is_bot(world, c), ..Default::default() };
                pl.fields.insert(Name::Id(host::F_CLASSNAME), Value::str("player"));
                pl.fields.insert(Name::Id(host::F_ORIGIN), Value::Vector(origin));
                pl.fields.insert(Name::Id(host::F_ANGLES), Value::Vector(angles));
                let id = s.vm.host_mut().add_ent(pl);
                s.vm.host_mut().players.insert(c, id);
                s.players.insert(c, id);
                joined.push(id);
                id
            }
        };
        s.vm.host_mut().set_pose(id, origin, angles);
    }
    if !joined.is_empty() {
        let list = gsc_iw6_vm::ArrRef::new();
        for id in s.players.values() {
            list.push(Iw6Host::value(*id));
        }
        // (`level.participants`, the players and the agents: here the players.)
        let all = gsc_iw6_vm::ArrRef::new();
        for id in s.players.values() {
            all.push(Iw6Host::value(*id));
        }
        s.vm.level_obj().set(Name::Id(boot::F_LEVEL_PARTICIPANTS), Value::Array(all));
        s.vm.level_obj().set(Name::Id(boot::F_LEVEL_PLAYERS), Value::Array(list));
    }
    s.vm.host_mut().now_ms = now;
    s.fx_budget = MAX_FX_PER_TICK;
    // The map's connect / prematch notifies, once.
    for id in &joined {
        let lv = Value::Object(s.vm.level_obj().clone());
        s.vm.notify(&lv, "connected", vec![Iw6Host::value(*id)]);
        events += 1;
    }
    if !joined.is_empty() && !s.prematch_sent {
        s.prematch_sent = true;
        let lv = Value::Object(s.vm.level_obj().clone());
        if let Some(set) = boot::utility_fn(&s.vm, boot::FN_FLAG_SET) {
            s.vm.start_thread(set, vec![Value::str("prematch_done")]);
        }
        s.vm.notify(&lv, "prematch_done", vec![]);
        s.vm.notify(&lv, "prematch_over", vec![]);
        events += 1;
    }
    // The match's end: the game script sets the `game_ended` match flag (bit 0x11) and the level
    // notifies `game_ended` once; the map's scripts stop their movers and waits on it.
    if !s.game_ended_sent && world.resource::<Zm>().match_flags & (1 << 0x11) != 0 {
        s.game_ended_sent = true;
        let lv = Value::Object(s.vm.level_obj().clone());
        s.vm.notify(&lv, "game_ended", vec![]);
        events += 1;
    }
    // The Tactical Nuke (`bo2mp/moab`) went off: the level gets `nuke_death`, as Ghosts' own nuke
    // sends at its blast, so the map's scripts that wait on it (Favela's death vision, other maps'
    // events) run. Once per blast: the script drops its match flag a moment after.
    let moab = world.resource::<Zm>().match_flags & (1 << super::natives_game::MATCH_FLAG_MOAB) != 0;
    if moab && !s.moab_sent {
        s.moab_sent = true;
        let lv = Value::Object(s.vm.level_obj().clone());
        s.vm.notify(&lv, "nuke_death", vec![]);
        events += 1;
    } else if !moab {
        s.moab_sent = false;
    }
    // The Tactical Nuke is called in: Ghosts' nuke asks the map's hook first (`level.nukeoverride`,
    // field 10679: Strikezone's returns true after starting its "destroyed" level event, and the
    // standard nuke does not run). The answer goes back as the `bo2mp_moab_handled` match flag.
    let call = world.resource::<Zm>().match_flags & (1 << super::natives_game::MATCH_FLAG_MOAB_CALL) != 0;
    if call && !s.moab_call_sent {
        s.moab_call_sent = true;
        let hook = s.vm.level_obj().get(&Name::Id(F_LEVEL_NUKE_HOOK));
        let handled = match hook {
            Some(Value::Function(f)) => {
                let lv = Value::Object(s.vm.level_obj().clone());
                matches!(s.vm.call(lv, f, vec![]), Some(Value::Int(n)) if n != 0)
            }
            _ => false,
        };
        let bit = 1u64 << super::natives_game::MATCH_FLAG_MOAB_HANDLED;
        let mut zm = world.resource_mut::<Zm>();
        if handled {
            zm.match_flags |= bit;
        } else {
            zm.match_flags &= !bit;
        }
        diag::info!(Sim, "iw6 scripts: the Tactical Nuke asked the map's nuke hook: handled {handled}");
        events += 1;
    } else if !call {
        s.moab_call_sent = false;
    }
    // Presses: a use trigger's `trigger` notify, with the player.
    for (zm_t, client) in std::mem::take(&mut s.presses) {
        let (Some(&t), Some(&p)) = (s.host_of.get(&zm_t), s.players.get(&client)) else {
            continue;
        };
        s.vm.notify(&Iw6Host::value(t), "trigger", vec![Iw6Host::value(p)]);
        events += 1;
    }
    // Damage: the engine's rules (`events::damage_notifies`): health, `damage` with (amount,
    // attacker, direction, point, MOD, ..., weapon), `death` when it runs out.
    for d in std::mem::take(&mut s.damages) {
        let Some(&t) = s.host_of.get(&d.ent) else { continue };
        let attacker = s.players.get(&d.attacker).copied();
        let hit = events::DamageEvent { ent: t, amount: d.amount, attacker, dir: d.dir, point: d.point, means: d.means.to_owned(), weapon: d.weapon };
        events::deliver_damage(&mut s.vm, &hit);
        events += 1;
    }
    // Spawned touch triggers.
    events += touch_triggers(world, s);
    // Timers the builtins set (`playsound(.., notify)`).
    let due: Vec<(i64, Value, String)> = {
        let h = s.vm.host_mut();
        let (due, keep): (Vec<_>, Vec<_>) = std::mem::take(&mut h.timers).into_iter().partition(|t| t.0 <= now);
        h.timers = keep;
        due
    };
    for (_, target, name) in due {
        s.vm.notify(&target, &name, vec![]);
        events += 1;
    }
    // Animations: notetracks passed and the `end` of the clips that ran their length.
    let notes = s.vm.host_mut().anim.advance(now);
    for (ent, name) in notes {
        s.vm.notify(&Iw6Host::value(ent), &name, vec![]);
        events += 1;
    }
    // Host-run tracks (entities the sim does not hold).
    own_tracks(s, now);
    // The VM.
    let dt = match s.last_ms {
        Some(l) => (now - l).clamp(0, 500) as u32,
        None => crate::MATCH_TICK_MS as u32,
    };
    s.last_ms = Some(now);
    let rep = s.vm.step(dt);
    for e in rep.errors.iter().chain(s.vm.take_errors().iter()) {
        if s.errors_logged < 40 {
            s.errors_logged += 1;
            diag::info!(Sim, "iw6 scripts: error: {e}");
        }
    }
    let new: Vec<((bool, u16), u64)> = s.vm.unimplemented_builtins().iter().filter(|(k, _)| !s.logged_missing.contains(*k)).map(|(k, n)| (*k, *n)).collect();
    for ((method, id), n) in new {
        s.logged_missing.insert((method, id));
        let kind = if method { gsc_iw6::builtins::Kind::Method } else { gsc_iw6::builtins::Kind::Function };
        diag::info!(
            Sim,
            "iw6 scripts: builtin {} {id:#06x} {} is not implemented (called {n}x; it returns undefined and the script goes on)",
            if method { "method" } else { "function" },
            gsc_iw6::builtins::name(kind, id).unwrap_or("(unnamed)")
        );
    }
    apply(world, s, now);
    // Looped effects due.
    let mut looped = std::mem::take(&mut s.looped);
    for l in &mut looped {
        if l.next <= now && s.fx_budget > 0 {
            s.fx_budget -= 1;
            l.next = now + l.period_ms;
            super::natives_fx::effect(world, &l.name, l.origin, l.forward);
        }
    }
    s.looped = looped;
    // Exploders due.
    let (due, keep): (Vec<_>, Vec<_>) = std::mem::take(&mut s.exploders).into_iter().partition(|p| p.due <= now);
    s.exploders = keep;
    for p in due {
        if !p.effect.is_empty() {
            // The createfx rule: the effect is turned by the entity's forward and up (roll counts).
            super::natives_fx::effect_axis(world, &p.effect, p.origin, p.forward, p.up);
        }
        if !p.sound.is_empty() {
            crate::script::host::natives::engine::play_sound_at(world, p.origin, &p.sound);
        }
    }
    publish(world, s);
    events
}

/// Advances the tracks of host-only entities and says `movedone` / `rotatedone`.
fn own_tracks(s: &mut Iw6Script, now: i64) {
    let ids: Vec<u32> = s.own.keys().copied().collect();
    for id in ids {
        let (mut pos, mut rot) = s.own.remove(&id).unwrap_or_default();
        let (mut o, mut a) = (s.vm.host().origin(id), s.vm.host().angles(id));
        let mut done = Vec::new();
        if let Some(t) = &pos {
            o = t.at(now);
            if t.done(now) {
                o = t.to;
                pos = None;
                done.push("movedone");
            }
        }
        if let Some(t) = &rot {
            a = t.at(now);
            if t.done(now) {
                a = t.to;
                rot = None;
                done.push("rotatedone");
            }
        }
        s.vm.host_mut().set_pose(id, o, a);
        if pos.is_some() || rot.is_some() {
            s.own.insert(id, (pos, rot));
        }
        for n in done {
            s.vm.notify(&Iw6Host::value(id), n, vec![]);
        }
    }
}

/// The catalog name of the effect a script's `loadfx` path names: the effects the assets lane
/// converted for the scripts (`script_fx`, by path), else an exploder's of that effect id.
fn effect_name(pack: &asset_world::Iw6ScriptPack, path: &str) -> Option<String> {
    let lower = path.replace('\\', "/").to_ascii_lowercase();
    if let Some((_, n)) = pack.script_fx.iter().find(|(p, n)| !n.is_empty() && *p == lower) {
        return Some(n.clone());
    }
    let last = lower.rsplit('/').next().unwrap_or(&lower);
    if let Some((_, n)) = pack.script_fx.iter().find(|(p, n)| !n.is_empty() && p.rsplit('/').next() == Some(last)) {
        return Some(n.clone());
    }
    pack.exploders.iter().find(|e| !e.effect.is_empty() && e.fxid.eq_ignore_ascii_case(last)).map(|e| e.effect.clone())
}

/// Plays an effect once (turned by forward and up when both are given), within the tick's budget.
fn play_fx(world: &mut World, s: &mut Iw6Script, name: &str, origin: [f32; 3], forward: [f32; 3], up: [f32; 3]) {
    if s.fx_budget == 0 {
        return;
    }
    s.fx_budget -= 1;
    if up == [0.0; 3] {
        super::natives_fx::effect(world, name, origin, forward);
    } else {
        super::natives_fx::effect_axis(world, name, origin, forward, up);
    }
}

/// Turns off what plays bolted to an entity (every tag, or the one named; every effect, or the
/// one named).
fn stop_bolted(world: &mut World, s: &mut Iw6Script, ent: u32, name: Option<&str>, tag: Option<&str>) {
    let keys: Vec<_> = s
        .fx_keys
        .keys()
        .filter(|(e, n, t)| *e == ent && name.is_none_or(|x| x == n) && tag.is_none_or(|x| x.is_empty() || x.eq_ignore_ascii_case(t)))
        .cloned()
        .collect();
    for k in keys {
        if let Some((at, _)) = s.fx_keys.remove(&k) {
            super::natives_fx::effect_stop_world(world, &k.1, at);
        }
    }
}

fn apply(world: &mut World, s: &mut Iw6Script, now: i64) {
    let effects = s.vm.host_mut().take_effects();
    for fx in effects {
        match fx {
            Effect::MoveTo { ent, to, secs, accel, decel } => {
                s.touched.insert(ent);
                let from = s.vm.host().origin(ent);
                if let Some(&n) = s.zm_of.get(&ent) {
                    world.resource_mut::<Zm>().movers.move_to(n, from, to, now, secs, accel, decel);
                    s.tracking.entry(ent).or_default().0 = true;
                } else {
                    let t = Track {
                        from,
                        to,
                        start_ms: now,
                        dur_ms: (secs * 1000.0).round().max(1.0) as i64,
                        accel_ms: (accel * 1000.0) as i64,
                        decel_ms: (decel * 1000.0) as i64,
                    };
                    s.own.entry(ent).or_default().0 = Some(t);
                }
            }
            Effect::RotateTo { ent, to, secs, accel, decel } => {
                s.touched.insert(ent);
                let from = s.vm.host().angles(ent);
                if let Some(&n) = s.zm_of.get(&ent) {
                    world.resource_mut::<Zm>().movers.rotate_to(n, from, to, now, secs, accel, decel);
                    s.tracking.entry(ent).or_default().1 = true;
                } else {
                    let t = Track {
                        from,
                        to,
                        start_ms: now,
                        dur_ms: (secs * 1000.0).round().max(1.0) as i64,
                        accel_ms: (accel * 1000.0) as i64,
                        decel_ms: (decel * 1000.0) as i64,
                    };
                    s.own.entry(ent).or_default().1 = Some(t);
                }
            }
            Effect::Place { ent, origin, angles } => {
                s.touched.insert(ent);
                with_zm(world, s, ent, |e| {
                    if let Some(o) = origin {
                        e.origin = o;
                    }
                    if let Some(a) = angles {
                        e.angles = a;
                    }
                });
            }
            Effect::Spawn { ent, class, origin, radius, height, .. } => spawn_ent(world, s, ent, &class, origin, radius, height),
            Effect::Anim { ent, .. } | Effect::ClearAnim { ent } => {
                // (The animator on the host keeps the clip; the pose is published from it.)
                s.touched.insert(ent);
            }
            Effect::Paths { ent, connect } => {
                // The brush entity's volume where it stands now: the paths through it are cut
                // (`disconnectpaths`) or given back (`connectpaths`).
                if let Some((mins, maxs)) = gate_bounds(world, s, ent) {
                    s.gates.insert(ent, asset_world::Iw6PathGate { key: ent, mins, maxs, blocked: !connect });
                    asset_world::set_iw6_path_gates(s.gates.values().cloned().collect());
                }
            }
            Effect::Earthquake { scale, duration, at, radius } => {
                // The screen shake the IW engines share (`earthquake(scale, seconds, origin, radius)`).
                let quake = crate::ScriptEarthquake { id: 0, scale, duration_ms: (duration * 1000.0).round() as i32, origin: at, radius, start_ms: 0 };
                if quake.valid() {
                    let _ = crate::script::push_earthquake(world, quake);
                }
            }
            Effect::LocalSound { ent, alias } => {
                let o = s.vm.host().origin(ent);
                crate::script::host::natives::engine::play_sound_at(world, o, &alias);
            }
            Effect::PhysicsBlast { .. }
            | Effect::Omnvar { .. }
            | Effect::Shellshock { .. }
            | Effect::Kill { .. }
            | Effect::CloneCorpse { .. }
            | Effect::LightIntensity { .. }
            | Effect::VisionSet { .. } => {}
            Effect::SetModel { ent, model } => {
                s.touched.insert(ent);
                let bounds = s.vm.host().models.get(model.trim_start_matches(',').to_ascii_lowercase().as_str()).map(|m| (m.mins, m.maxs));
                with_zm(world, s, ent, |e| {
                    e.model = model.clone();
                    e.bounds = bounds;
                });
            }
            Effect::Hide { ent, hidden } => {
                s.touched.insert(ent);
                with_zm(world, s, ent, |e| e.hidden = hidden);
            }
            Effect::Solid { ent, solid } => with_zm(world, s, ent, |e| e.solid = solid),
            Effect::CanDamage { ent, on } => with_zm(world, s, ent, |e| e.can_damage = on),
            Effect::Usable { ent, on } => with_zm(world, s, ent, |e| e.trigger_off = !on),
            Effect::Hint { ent, text } => {
                let shown = localized_hint(&s.pack, &text);
                with_zm(world, s, ent, |e| e.hint = (!shown.is_empty()).then(|| shown.clone()))
            }
            Effect::Delete { ent } => {
                s.touched.insert(ent);
                s.vm.entity_removed(EntityId(ent));
                s.own.remove(&ent);
                s.tracking.remove(&ent);
                stop_bolted(world, s, ent, None, None);
                if let Some(slot) = s.slots.remove(&ent) {
                    s.retired.push(slot);
                }
                if let Some(&n) = s.zm_of.get(&ent) {
                    let spawned = s.vm.host().ent(ent).is_some_and(|h| h.spawned);
                    let mut zm = world.resource_mut::<Zm>();
                    zm.movers.list.remove(&n);
                    if spawned {
                        // A script's own entity leaves the sim altogether.
                        zm.ents.remove(&n);
                        s.zm_of.remove(&ent);
                        s.host_of.remove(&n);
                    } else if let Some(e) = zm.ents.get_mut(&n) {
                        e.hidden = true;
                        e.solid = false;
                        e.can_damage = false;
                        e.trigger_off = true;
                        e.loop_sound = None;
                    }
                }
            }
            Effect::Link { ent, parent, tag, offset, angles, explicit } => {
                s.touched.insert(ent);
                if let (Some(&c), Some(&p)) = (s.zm_of.get(&ent), s.zm_of.get(&parent)) {
                    // A bone of a skeleton: the child keeps its frame in the bone's (READ: the
                    // engine stores the child's place relative to the tag, from where it stands
                    // now when no offsets are given), and follows the bone's animated pose.
                    s.bone_links.remove(&c);
                    if !tag.is_empty() {
                        let (po, pa) = world.resource::<Zm>().ents.get(&p).map(|e| (e.origin, e.angles)).unwrap_or_default();
                        let (co, ca) = world.resource::<Zm>().ents.get(&c).map(|e| (e.origin, e.angles)).unwrap_or_default();
                        let h = s.vm.host();
                        let model = h.model_of(parent);
                        if let Some(bone) = h.anim.tag_bone_world(parent, &model, po, pa, &tag, h.now_ms) {
                            let _ = bone;
                            let local = if explicit {
                                Some((iw6_pose::quat_of_axes(iw6_pose::axes_of_angles(angles)), offset))
                            } else {
                                h.anim.link_local(parent, &model, po, pa, &tag, h.now_ms, (co, ca))
                            };
                            if let Some(local) = local {
                                s.bone_links.insert(c, BoneLink { parent: p, tag: tag.clone(), local });
                            }
                        }
                    }
                    let mut zm = world.resource_mut::<Zm>();
                    if let Some(e) = zm.ents.get_mut(&c) {
                        e.link = Some((p, offset, angles));
                        e.link_tag = (!tag.is_empty()).then_some(tag);
                    }
                }
            }
            Effect::Unlink { ent } => {
                if let Some(&c) = s.zm_of.get(&ent) {
                    s.bone_links.remove(&c);
                }
                with_zm(world, s, ent, |e| {
                    e.link = None;
                    e.link_tag = None;
                });
            }
            Effect::Sound { ent, at, alias } => {
                let origin = at.or_else(|| ent.map(|e| s.vm.host().origin(e)));
                if let Some(o) = origin {
                    crate::script::host::natives::engine::play_sound_at(world, o, &alias);
                }
            }
            Effect::LoopSound { ent, alias } => with_zm(world, s, ent, |e| e.loop_sound = alias.clone()),
            Effect::Fx { path, origin, forward, up } => {
                if let Some(name) = effect_name(&s.pack, &path) {
                    play_fx(world, s, &name, origin, forward, up);
                }
            }
            Effect::FxOnTag { path, ent, tag } => {
                // The effect plays at the tag where it stands now; the script repeats it (the toys'
                // smoke every 400 ms), so each play is one shot, at most one per gap per tag.
                if let Some(name) = effect_name(&s.pack, &path) {
                    let (at, angles) = s.vm.host().tag_world(ent, &tag);
                    let key = (ent, name.clone(), tag.to_ascii_lowercase());
                    let recent = s.fx_keys.get(&key).is_some_and(|(_, t)| now - t < MIN_FX_GAP_MS);
                    if !recent {
                        if s.fx_keys.len() >= MAX_FX_KEYS {
                            let oldest = s.fx_keys.iter().min_by_key(|(_, (_, t))| *t).map(|(k, _)| k.clone());
                            if let Some(k) = oldest {
                                s.fx_keys.remove(&k);
                            }
                        }
                        s.fx_keys.insert(key, (at, now));
                        let (f, _, u) = builtins::angle_vectors(angles);
                        play_fx(world, s, &name, at, f, u);
                    }
                }
            }
            Effect::StopFxOnTag { path, ent, tag, .. } => {
                let name = effect_name(&s.pack, &path);
                stop_bolted(world, s, ent, name.as_deref(), Some(&tag));
            }
            Effect::LoopedFx { path, repeat, origin, forward } => {
                if let Some(name) = effect_name(&s.pack, &path)
                    && s.looped.len() < MAX_LOOPS
                {
                    s.looped.push(LoopFx { name, origin, forward, period_ms: ((repeat * 1000.0) as i64).max(250), next: now });
                }
            }
            Effect::HidePart { ent, tag, hide } => with_zm(world, s, ent, |e| {
                let tag = tag.to_ascii_lowercase();
                e.hidden_parts.retain(|t| *t != tag);
                if hide {
                    e.hidden_parts.push(tag);
                }
            }),
            Effect::Attach { ent, model, tag, on } => with_zm(world, s, ent, |e| {
                if on {
                    e.attached.push((model.clone(), tag.clone()));
                } else if let Some(i) = e.attached.iter().position(|(m, t)| m.eq_ignore_ascii_case(&model) && (tag.is_empty() || t.eq_ignore_ascii_case(&tag))) {
                    e.attached.remove(i);
                }
            }),
            Effect::RadiusDamage { origin, range, max, min, attacker, means, weapon, .. } => {
                blast_players(world, s, origin, range, max, min, attacker, &means, &weapon);
            }
            Effect::Exploder { id, delay } => {
                for e in s.pack.exploders.iter().filter(|e| e.id.trim() == id.to_string()) {
                    let (forward, _, up) = gsc_t6::math::angle_vectors(e.angles);
                    s.exploders.push(Pending {
                        due: now + ((delay + e.delay).max(0.0) * 1000.0) as i64,
                        effect: e.effect.clone(),
                        origin: e.origin,
                        forward,
                        up,
                        sound: e.soundalias.clone().unwrap_or_default(),
                    });
                }
            }
            Effect::Platform { ent, on } => with_zm(world, s, ent, |e| e.platform = on),
        }
    }
}

/// A script's `spawn`: the sim gets an entity for it (a damage tap, a trigger volume, a model).
fn spawn_ent(world: &mut World, s: &mut Iw6Script, ent: u32, class: &str, origin: [f32; 3], radius: f32, height: f32) {
    let n = world.resource_mut::<Zm>().alloc_entnum();
    let Some(obj) = super::with_vm(world, |vm, _| vm.alloc_object(gsc_t6::ObjKind::Entity(n))) else {
        return;
    };
    let mut e = Ent { obj: Some(obj), classname: class.to_owned(), origin, ..Default::default() };
    if class.starts_with("trigger") {
        e.radius = radius;
        e.height = height;
    }
    world.resource_mut::<Zm>().ents.insert(n, e);
    s.zm_of.insert(ent, n);
    s.host_of.insert(n, ent);
    s.authored.insert(ent, (origin, [0.0; 3]));
    s.authored_model.insert(ent, String::new());
    s.touched.insert(ent);
}

/// Spawned touch triggers (`trigger_radius`, `trigger_multiple`, `trigger_once` the scripts make):
/// `trigger` goes to the script with the player when he steps in. INFERRED from the engine's
/// touch triggers: the notify comes once per entry (the engine's wait time is not read).
fn touch_triggers(world: &mut World, s: &mut Iw6Script) -> u64 {
    let spawned: Vec<(u32, u32)> = s
        .vm
        .host()
        .ents
        .iter()
        .enumerate()
        .filter(|(_, h)| h.spawned && !h.gone)
        .filter_map(|(i, _)| {
            let id = i as u32;
            let class = s.vm.host().class(id);
            matches!(class.as_str(), "trigger_radius" | "trigger_multiple" | "trigger_once").then_some(())?;
            Some((id, *s.zm_of.get(&id)?))
        })
        .collect();
    if spawned.is_empty() {
        s.inside.clear();
        return 0;
    }
    let mut now_inside = HashSet::new();
    let mut events = 0;
    let players: Vec<(u32, u32)> = s.players.iter().map(|(c, h)| (*c, *h)).collect();
    for (id, n) in spawned {
        let Some(e) = world.resource::<Zm>().ents.get(&n).cloned() else { continue };
        for &(client, host_id) in &players {
            let Some((lo, hi)) = super::triggers::player_box(world, client) else { continue };
            if super::triggers::touches(world, &e, lo, hi) {
                now_inside.insert((id, client));
                if !s.inside.contains(&(id, client)) {
                    s.vm.notify(&Iw6Host::value(id), "trigger", vec![Iw6Host::value(host_id)]);
                    events += 1;
                }
            }
        }
    }
    s.inside = now_inside;
    events
}

/// A script blast on the players: the engine's linear fall-off from the point (`radiusdamage`),
/// handed to the game's player damage.
#[allow(clippy::too_many_arguments)]
fn blast_players(world: &mut World, s: &Iw6Script, origin: [f32; 3], range: f32, max: f32, min: f32, attacker: Option<u32>, means: &str, weapon: &str) {
    let range = range.max(1.0);
    let attacker_client = attacker.and_then(|a| s.vm.host().ent(a)).and_then(|h| h.client);
    for (client, at, _) in players_in(world) {
        let centre = [at[0], at[1], at[2] + 36.0];
        let d: [f32; 3] = std::array::from_fn(|k| centre[k] - origin[k]);
        let dist = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt();
        if dist >= range {
            continue;
        }
        let amount = (min + (max - min) * (1.0 - dist / range)) as i32;
        if amount <= 0 {
            continue;
        }
        let dir = if dist > 0.0 { d.map(|v| v / dist) } else { [0.0, 0.0, 1.0] };
        let att = attacker_client.and_then(|c| world.resource::<Zm>().players.get(&c).map(|p| p.obj));
        super::with_vm(world, |vm, world| {
            let attacker = att.map_or(gsc_t6::Value::Undefined, gsc_t6::Value::Object);
            super::natives_ai::player_damage(vm, world, client, gsc_t6::Value::Undefined, attacker, amount, 0, means, weapon, origin, dir, "none");
        });
    }
}

/// A hint string as the player reads it: the map's English text for a localized reference
/// (`MP_FRAG_OPERATE_DOOR`), without colour codes, the use key as Black Ops II names it.
fn localized_hint(pack: &asset_world::Iw6ScriptPack, text: &str) -> String {
    let key = text.trim_start_matches('&');
    let found = pack.localized.iter().find(|(k, _)| k.eq_ignore_ascii_case(key)).map(|(_, v)| v.as_str());
    let Some(t) = found else { return text.to_owned() };
    let mut out = String::new();
    let mut it = t.chars().peekable();
    while let Some(c) = it.next() {
        if c == '^' && it.peek().is_some_and(char::is_ascii_digit) {
            it.next();
            continue;
        }
        out.push(c);
    }
    out.replace("{+activate}", "F")
}

/// The world bounds of a brush entity (or a model's box) at its pose now.
fn gate_bounds(world: &World, s: &Iw6Script, ent: u32) -> Option<([f32; 3], [f32; 3])> {
    let &n = s.zm_of.get(&ent)?;
    let zm = world.resource::<Zm>();
    let e = zm.ents.get(&n)?;
    let local = match super::triggers::brush_index(e) {
        Some(b) => s.pack.brush_bounds.iter().find(|x| x.0 == b).map(|x| (x.1, x.2))?,
        None => e.bounds?,
    };
    let axes = iw6_pose::axes_of_angles(e.angles);
    let (mut lo, mut hi) = ([f32::MAX; 3], [f32::MIN; 3]);
    for c in 0..8 {
        let p = [if c & 1 == 0 { local.0[0] } else { local.1[0] }, if c & 2 == 0 { local.0[1] } else { local.1[1] }, if c & 4 == 0 { local.0[2] } else { local.1[2] }];
        for k in 0..3 {
            let w = e.origin[k] + p[0] * axes[0][k] + p[1] * axes[1][k] + p[2] * axes[2][k];
            lo[k] = lo[k].min(w);
            hi[k] = hi[k].max(w);
        }
    }
    Some((lo, hi))
}

fn with_zm(world: &mut World, s: &Iw6Script, ent: u32, f: impl FnOnce(&mut Ent)) {
    if let Some(&n) = s.zm_of.get(&ent) {
        if let Some(e) = world.resource_mut::<Zm>().ents.get_mut(&n) {
            f(e);
        }
    }
}

/// After `movers::advance`: the host's entities stand where the sim put them; finished tracks
/// say `movedone` / `rotatedone` (next tick's step delivers them).
pub(super) fn post(world: &mut World, _now: i64) {
    if !active(world) {
        return;
    }
    let mut s = world.remove_resource::<Iw6Script>().expect("checked");
    let ids: Vec<u32> = s.tracking.keys().copied().collect();
    for id in ids {
        let Some(&n) = s.zm_of.get(&id) else { continue };
        let (pos_live, rot_live, o, a) = {
            let zm = world.resource::<Zm>();
            let m = zm.movers.list.get(&n);
            let Some(e) = zm.ents.get(&n) else { continue };
            (m.is_some_and(|m| m.pos.is_some()), m.is_some_and(|m| m.rot.is_some()), e.origin, e.angles)
        };
        s.vm.host_mut().set_pose(id, o, a);
        let (was_pos, was_rot) = s.tracking.get(&id).copied().unwrap_or_default();
        if was_pos && !pos_live {
            s.vm.notify(&Iw6Host::value(id), "movedone", vec![]);
        }
        if was_rot && !rot_live {
            s.vm.notify(&Iw6Host::value(id), "rotatedone", vec![]);
        }
        if pos_live || rot_live {
            s.tracking.insert(id, (pos_live, rot_live));
        } else {
            s.tracking.remove(&id);
        }
    }
    // Linked children follow their parents in the sim; the host sees it.
    let linked: Vec<u32> = s.touched.iter().copied().filter(|id| s.zm_of.contains_key(id)).collect();
    for id in linked {
        let n = s.zm_of[&id];
        let pose = world.resource::<Zm>().ents.get(&n).filter(|e| e.link.is_some()).map(|e| (e.origin, e.angles));
        if let Some((o, a)) = pose {
            s.vm.host_mut().set_pose(id, o, a);
        }
    }
    world.insert_resource(s);
}

/// Where an entity linked to a bone of a skeleton stands now (the bone's animated pose with the
/// child's frame in it): what `movers::follow_links` places it by. None for any other link.
pub(super) fn linked_pose(world: &World, n: u32) -> Option<([f32; 3], [f32; 3])> {
    let s = world.get_resource::<Iw6Script>()?;
    let l = s.bone_links.get(&n)?;
    let pid = *s.host_of.get(&l.parent)?;
    let pe = world.get_resource::<Zm>()?.ents.get(&l.parent)?;
    let h = s.vm.host();
    h.anim.link_world(pid, &h.model_of(pid), pe.origin, pe.angles, &l.tag, h.now_ms, l.local)
}

/// Where tag `tag` of the parent `parent` (a sim entity) stands, when the parent is a Ghosts
/// script model with a skeleton: the bone as its animation has it (what the entities linked to it
/// follow). None for anything else.
pub(super) fn tag_pose(world: &World, parent: u32, tag: &str) -> Option<([f32; 3], [f32; 3])> {
    let s = world.get_resource::<Iw6Script>()?;
    let id = *s.host_of.get(&parent)?;
    let e = world.get_resource::<Zm>()?.ents.get(&parent)?;
    let h = s.vm.host();
    let model = h.model_of(id);
    h.anim.tag_world(id, &model, e.origin, e.angles, tag, h.now_ms)
}

/// Hidden instances the renderer keeps of each model (`BO2MP_IW6_SPAWN_POOL`, as the scene's).
fn pool_slots() -> u32 {
    std::env::var("BO2MP_IW6_SPAWN_POOL").ok().and_then(|v| v.trim().parse().ok()).unwrap_or(4)
}

use anim::publishable_class;

/// Publishes where every script-touched model stands (and which are hidden or gone). A model the
/// map placed is found by its name and where the map put it; a model a script spawned, or swapped
/// in over the map's (a destructible breaking), is drawn by one of the hidden instances the
/// renderer keeps of that model.
fn publish(world: &mut World, s: &mut Iw6Script) {
    let mut poses = Vec::new();
    let slots_per_model = pool_slots();
    let ids: Vec<u32> = s.touched.iter().copied().collect();
    for id in ids {
        let Some(&n) = s.zm_of.get(&id) else { continue };
        let (class, hidden_sim, origin, angles, model, brush) = {
            let zm = world.resource::<Zm>();
            let Some(e) = zm.ents.get(&n) else { continue };
            (e.classname.clone(), e.hidden, e.origin, e.angles, e.model.trim_start_matches(',').to_ascii_lowercase(), super::triggers::brush_index(e))
        };
        let animating = s.vm.host().anim.playing.contains_key(&id);
        if !publishable_class(&class, animating, !model.is_empty()) {
            continue;
        }
        let Some(&(spawn, _)) = s.authored.get(&id) else { continue };
        let host_ent = s.vm.host().ent(id);
        let spawned = host_ent.is_some_and(|h| h.spawned);
        let hidden = hidden_sim || host_ent.is_none_or(|h| h.gone || h.hidden);
        let authored = s.authored_model.get(&id).cloned().unwrap_or_default();
        let swapped = !spawned && brush.is_none() && model != authored;
        // A model that plays an animation: its bones' skinning poses.
        let skin = if s.vm.host().anim.playing.contains_key(&id) { s.vm.host().anim.skin(id, &model, s.vm.host().now_ms) } else { None };
        if let Some(b) = brush {
            poses.push(asset_world::Iw6MoverPose { brush: Some(b), spawn_origin: spawn, origin, angles, hidden, ..Default::default() });
            continue;
        }
        // The clip's root motion carries the model off its authored place.
        let origin = if animating {
            let sh = s.vm.host().anim.root_shift(id, angles, s.vm.host().now_ms);
            [origin[0] + sh[0], origin[1] + sh[1], origin[2] + sh[2]]
        } else {
            origin
        };
        if !spawned {
            // The map's own model: gone when the scripts swapped another in.
            poses.push(asset_world::Iw6MoverPose { model: Some(authored.clone()), spawn_origin: spawn, origin, angles, hidden: hidden || swapped, skin: skin.clone(), ..Default::default() });
        }
        if (spawned || swapped) && !model.is_empty() {
            // The instance that shows this entity's model.
            if s.slots.get(&id).is_some_and(|(m, _)| *m != model) {
                if let Some(old) = s.slots.remove(&id) {
                    s.retired.push(old);
                }
            }
            if !s.slots.contains_key(&id) && s.pack.pool_models.iter().any(|m| *m == model) {
                let taken: HashSet<u32> = s.slots.values().filter(|(m, _)| *m == model).map(|(_, k)| *k).collect();
                if let Some(k) = (0..slots_per_model).find(|k| !taken.contains(k)) {
                    s.retired.retain(|r| !(r.0 == model && r.1 == k));
                    s.slots.insert(id, (model.clone(), k));
                }
            }
            if let Some((m, k)) = s.slots.get(&id) {
                poses.push(asset_world::Iw6MoverPose { model: Some(m.clone()), origin, angles, hidden, slot: Some(*k), skin: skin.clone(), ..Default::default() });
            }
        }
    }
    // Instances whose entity is gone or changed model: hidden once more.
    for (m, k) in &s.retired {
        poses.push(asset_world::Iw6MoverPose { model: Some(m.clone()), hidden: true, slot: Some(*k), ..Default::default() });
    }
    poses.sort_by(|a, b| format!("{:?}{:?}{:?}", a.brush, a.model, a.slot).cmp(&format!("{:?}{:?}{:?}", b.brush, b.model, b.slot)).then(a.spawn_origin.partial_cmp(&b.spawn_origin).unwrap_or(std::cmp::Ordering::Equal)));
    if poses != s.published {
        s.published = poses.clone();
        asset_world::set_iw6_mover_poses(poses);
    }
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod hint_tests {
    use super::*;

    #[test]
    fn a_hint_reads_as_the_maps_english_text() {
        let pack = asset_world::Iw6ScriptPack {
            localized: vec![("MP_FRAG_OPERATE_DOOR".into(), "Hold ^3[{+activate}]^7 to Operate Door".into())],
            ..Default::default()
        };
        assert_eq!(localized_hint(&pack, "MP_FRAG_OPERATE_DOOR"), "Hold [F] to Operate Door");
        assert_eq!(localized_hint(&pack, "&mp_frag_operate_door"), "Hold [F] to Operate Door");
        assert_eq!(localized_hint(&pack, "SOMETHING_ELSE"), "SOMETHING_ELSE");
    }
}
