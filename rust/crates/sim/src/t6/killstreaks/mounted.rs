//! A map's mounted gun (`misc_turret`: Turbine's Death Machine on the wall
//! inside the building), as BO2's engine runs one: a living player within
//! reach sees its use prompt, presses use and takes it over (his own gun put
//! away for the turret's, held where he stands, his view kept to its arcs,
//! the gun turning with his view), and presses use again to let go (or
//! dies).

use std::collections::BTreeSet;

use bevy_ecs::prelude::World;
use gsc_t6::Value;

use super::super::{Zm, frame};
use super::streaks;
use crate::world::ClientId;

#[derive(Clone, Debug)]
pub(crate) struct Mounted {
    pub ent: u32,
    /// The gun he fires on it (0 when he already carried it), and the one he
    /// held before.
    pub weapon: u32,
    pub prev: u32,
    /// Its angles as the map set them.
    pub base: [f32; 3],
    /// right, left, up, down (the map's `rightarc` ...).
    pub arcs: [f32; 4],
}

/// How near he stands to take one (BO2's player_useRadius).
const REACH: f32 = 128.0;
/// BO2's mounted-gun prompt, and its English when the zone has none.
const HINT: &str = "PLATFORM_USETURRET";
const HINT_EN: &str = "Press [{+activate}] to use";
/// The gun he fires on it: the Death Machine (BO2's minigun, the model on
/// Turbine's wall), else the map's own turret weapon.
const GUN: &str = "minigun_mp";

fn dist(a: [f32; 3], b: [f32; 3]) -> f32 {
    ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)).sqrt()
}

fn wrap(a: f32) -> f32 {
    let a = a.rem_euclid(360.0);
    if a > 180.0 { a - 360.0 } else { a }
}

/// The free mounted gun nearest him within reach.
fn nearest(world: &World, at: [f32; 3]) -> Option<u32> {
    let taken: BTreeSet<u32> = world
        .get_resource::<super::Streaks>()
        .map(|s| s.mounted.values().map(|m| m.ent).collect())
        .unwrap_or_default();
    world
        .resource::<Zm>()
        .ents
        .iter()
        .filter(|(n, e)| e.map && e.classname == "misc_turret" && !e.hidden && !taken.contains(n))
        .map(|(n, e)| (dist(e.origin, at), *n))
        .filter(|(d, _)| *d <= REACH)
        .min_by(|a, b| a.0.total_cmp(&b.0))
        .map(|(_, n)| n)
}

/// The prompt of the free mounted gun within his reach.
pub(super) fn hint(world: &World, at: [f32; 3]) -> Option<String> {
    nearest(world, at)?;
    let zm = world.resource::<Zm>();
    Some(zm.strings.get(HINT).cloned().unwrap_or_else(|| HINT_EN.to_owned()))
}

/// The gun he fires on it and its arcs, from the map's keys.
fn setup(world: &mut World, n: u32) -> Option<(u32, [f32; 4])> {
    let obj = world.resource::<Zm>().ents.get(&n)?.obj;
    let (name, arcs) = match obj {
        Some(obj) => super::super::with_vm(world, |vm, _| {
            let mut read = |k: &str| {
                let f = vm.intern(k);
                vm.raw_field(obj, f)
            };
            let name = read("weaponinfo");
            let raw = ["rightarc", "leftarc", "toparc", "bottomarc"].map(&mut read);
            let arcs = raw.map(|v| match v {
                Value::Int(i) => Some(i as f32),
                Value::Float(f) => Some(f),
                v => vm.to_text(&v).trim().parse::<f32>().ok(),
            });
            (vm.to_text(&name), arcs)
        })?,
        None => (String::new(), [None; 4]),
    };
    let arcs = [
        arcs[0].unwrap_or(60.0),
        arcs[1].unwrap_or(60.0),
        arcs[2].unwrap_or(30.0),
        arcs[3].unwrap_or(30.0),
    ];
    let w = [GUN, name.as_str()]
        .into_iter()
        .filter(|s| !s.is_empty())
        .find_map(|s| super::super::weapon(world, s).ok().filter(|&w| w != 0))?;
    diag::info!(Sim, "bo2mp mounted gun {n} ('{name}'): fires weapon {w}, arcs {arcs:?}");
    Some((w, arcs))
}

fn mount(world: &mut World, c: u32, n: u32) {
    let id = ClientId(c);
    let Some((w, arcs)) = setup(world, n) else {
        diag::warn!(Sim, "bo2mp mounted gun {n}: no weapon to fire on it");
        return;
    };
    let Some(base) = world.resource::<Zm>().ents.get(&n).map(|e| e.angles) else {
        return;
    };
    let prev = frame(world).player(id).map_or(0, |ps| ps.weapon as u32);
    let had = crate::script_player::has_weapon(&frame(world), id, w);
    if !had && crate::script_player::give_weapon(&mut frame(world), id, w, false).is_err() {
        return;
    }
    crate::script_player::switch_to_weapon(&mut frame(world), id, w);
    {
        let mut f = frame(world);
        if f.client_meta(id).is_some() {
            let m = f.client_meta_mut(id);
            m.controls.linked = true;
            m.controls.switch_disabled = true;
            m.view_pitch_clamp = Some([arcs[2] - base[0], (arcs[3] + base[0]).min(85.0)]);
        }
    }
    diag::info!(Sim, "bo2mp mounted gun {n}: player {c} took it");
    streaks(world).mounted.insert(
        c,
        Mounted {
            ent: n,
            weapon: if had { 0 } else { w },
            prev,
            base,
            arcs,
        },
    );
}

fn dismount(world: &mut World, c: u32) {
    let Some(m) = streaks(world).mounted.remove(&c) else {
        return;
    };
    let id = ClientId(c);
    {
        let mut f = frame(world);
        if f.client_meta(id).is_some() {
            let meta = f.client_meta_mut(id);
            meta.controls.linked = false;
            meta.controls.switch_disabled = false;
            meta.view_pitch_clamp = None;
        }
    }
    if m.weapon != 0 {
        crate::script_player::take_weapon(&mut frame(world), id, m.weapon);
    }
    if m.prev != 0 && crate::script_player::has_weapon(&frame(world), id, m.prev) {
        crate::script_player::switch_to_weapon(&mut frame(world), id, m.prev);
    }
    if let Some(e) = world.resource_mut::<Zm>().ents.get_mut(&m.ent) {
        e.angles = m.base;
    }
    diag::info!(Sim, "bo2mp mounted gun {}: player {c} let go", m.ent);
}

/// Each tick, before the usable models: a press of use takes or lets go of
/// a map's mounted gun; a player on one keeps to its arcs and turns it.
pub(super) fn advance(world: &mut World) {
    let players: Vec<(u32, bool)> = world
        .resource::<Zm>()
        .players
        .iter()
        .map(|(c, p)| (*c, p.sessionstate == "playing"))
        .collect();
    for (c, playing) in players {
        let id = ClientId(c);
        let Some((origin, health)) = frame(world).player(id).map(|ps| (ps.origin, ps.health)) else {
            continue;
        };
        let held = crate::script_player::buttons(&mut frame(world), id)
            & (playerstate_iw4::buttons::USE | playerstate_iw4::buttons::USE_RELOAD)
            != 0;
        let pressed = held && !world.resource::<Zm>().use_held.contains(&c);
        let Some(m) = streaks(world).mounted.get(&c).cloned() else {
            if pressed && playing && health > 0 && !super::riding(world, c) {
                if let Some(n) = nearest(world, origin) {
                    mount(world, c, n);
                }
            }
            continue;
        };
        let gone = world.resource::<Zm>().ents.get(&m.ent).is_none_or(|e| e.hidden);
        if pressed || gone || !playing || health <= 0 {
            dismount(world, c);
            continue;
        }
        // He stays behind the gun; his view keeps to its arcs about the way
        // the map set it, and the gun turns with him.
        let cmd_yaw = {
            let mut req = world.resource_mut::<crate::step::StepRequest>();
            let mut last = None;
            for (cid, cmd) in &mut req.input.cmds {
                if *cid != id {
                    continue;
                }
                cmd.forwardmove = 0;
                cmd.rightmove = 0;
                last = Some(cmd.angles[1]);
            }
            last
        };
        let aim = {
            let mut f = frame(world);
            let Some(ps) = f.player_mut(id) else {
                continue;
            };
            let commanded = cmd_yaw.map(|a| a as f32 * movement_iw4::SHORT2ANGLE);
            let view = commanded.map_or(ps.viewangles[1], |a| a + ps.delta_angles[1]);
            let rel = wrap(view - m.base[1]);
            let kept = rel.clamp(-m.arcs[0], m.arcs[1]);
            if kept != rel {
                ps.delta_angles[1] = wrap(ps.delta_angles[1] + kept - rel);
                ps.viewangles[1] = wrap(ps.viewangles[1] + kept - rel);
            }
            [ps.viewangles[0], wrap(m.base[1] + kept), m.base[2]]
        };
        if let Some(e) = world.resource_mut::<Zm>().ents.get_mut(&m.ent) {
            e.angles = aim;
        }
    }
}
