//! bo2mp: a Ghosts map's door switches, driven by the map's own data.
//!
//! The map's entities say which `trigger_use_touch` switches (`door_switch`)
//! move which brush models and script models, and where each goes (the
//! `script_struct` it targets; `fastfile_iw6::maplogic`, handed over as
//! `asset_world::iw6_doors`). A press of a switch sends every piece of its
//! group to the far end, the next press back. Their collision follows
//! (`brushes::sync` for the brushes, the script movers' presence for the
//! models), and Ghosts' own renderer draws them where they stand
//! (`asset_world::set_iw6_mover_poses`).
//!
//! INFERRED (Ghosts' common door script is in a zone that is not the
//! map's): the time a move takes (`BO2MP_IW6_DOOR_SECS`, 1.5 s, eased in
//! and out over a fifth each), that a press while moving turns the doors
//! round, and that nothing blocks them (the map's blockers, `prone_only`
//! and `not_opening` triggers, are not read). So `BO2MP_IW6_DOORS=1` turns
//! it on; off by default.

use bevy_ecs::prelude::{Resource, World};

use super::Zm;

/// The door switches work (on; `BO2MP_IW6_DOORS=0` leaves the doors shut).
pub(crate) fn on() -> bool {
    std::env::var("BO2MP_IW6_DOORS").map_or(true, |v| v.trim() != "0")
}

fn move_secs() -> f32 {
    std::env::var("BO2MP_IW6_DOOR_SECS")
        .ok()
        .and_then(|v| v.trim().parse::<f32>().ok())
        .filter(|s| *s > 0.0 && *s < 30.0)
        .unwrap_or(1.5)
}

struct Piece {
    ent: u32,
    def: asset_world::Iw6DoorPiece,
}

struct Group {
    name: String,
    switches: Vec<u32>,
    pieces: Vec<Piece>,
    button_sound: Option<String>,
    open: bool,
}

#[derive(Resource, Default)]
pub(crate) struct Iw6Doors {
    resolved: bool,
    groups: Vec<Group>,
    /// Trigger entities pressed since the last advance.
    pressed: Vec<u32>,
    last_published: Vec<asset_world::Iw6MoverPose>,
}

fn near(a: [f32; 3], b: [f32; 3]) -> bool {
    (0..3).all(|k| (a[k] - b[k]).abs() < 1.0)
}

/// The trigger entities `fired` (by a use) this tick: the door switches
/// among them are acted on at the next advance.
pub(super) fn note_presses(world: &mut World, fired: &[u32]) {
    if fired.is_empty() || !on() {
        return;
    }
    if let Some(mut d) = world.get_resource_mut::<Iw6Doors>() {
        d.pressed.extend_from_slice(fired);
    }
}

/// Finds the switches' and pieces' entities (once the map's entities are
/// there); logs what was found.
fn resolve(world: &mut World, doors: &[asset_world::Iw6DoorGroup]) -> Vec<Group> {
    let zm = world.resource::<Zm>();
    let mut out = Vec::new();
    let (mut found, mut missing) = (0usize, 0usize);
    for g in doors {
        let switches: Vec<u32> = zm
            .ents
            .iter()
            .filter(|(_, e)| {
                e.classname.starts_with("trigger_use")
                    && g.switches.iter().any(|s| near(*s, e.origin))
            })
            .map(|(n, _)| *n)
            .collect();
        let mut pieces = Vec::new();
        for p in &g.pieces {
            let ent = zm.ents.iter().find(|(_, e)| {
                e.map
                    && match (p.brush, &p.model) {
                        (Some(b), _) => e.brush.as_deref() == Some(format!("*{b}").as_str()),
                        (None, Some(m)) => {
                            e.model.eq_ignore_ascii_case(m) && near(e.origin, p.start)
                        }
                        _ => false,
                    }
            });
            match ent {
                Some((n, _)) => {
                    found += 1;
                    pieces.push(Piece {
                        ent: *n,
                        def: p.clone(),
                    });
                }
                None => missing += 1,
            }
        }
        if !switches.is_empty() && !pieces.is_empty() {
            out.push(Group {
                name: g.name.clone(),
                switches,
                pieces,
                button_sound: g.button_sound.clone(),
                open: false,
            });
        }
    }
    diag::info!(
        Sim,
        "iw6 doors: {} of {} door groups have their switch, {found} pieces found, {missing} not (BO2MP_IW6_DOORS=0 turns them off; move {:.1} s, INFERRED)",
        out.len(),
        doors.len(),
        move_secs()
    );
    out
}

/// Acts on this tick's presses and publishes where the pieces stand.
pub(super) fn advance(world: &mut World, now: i64) {
    if !on() {
        return;
    }
    let Some(doors) = asset_world::iw6_doors() else {
        return;
    };
    if world.get_resource::<Iw6Doors>().is_none() {
        world.insert_resource(Iw6Doors::default());
    }
    if !world.resource::<Iw6Doors>().resolved {
        if world.resource::<Zm>().ents.is_empty() {
            return;
        }
        let groups = resolve(world, &doors);
        let mut d = world.resource_mut::<Iw6Doors>();
        d.groups = groups;
        d.resolved = true;
    }
    let pressed = std::mem::take(&mut world.resource_mut::<Iw6Doors>().pressed);
    let secs = move_secs();
    let mut sounds: Vec<([f32; 3], String)> = Vec::new();
    let mut groups = std::mem::take(&mut world.resource_mut::<Iw6Doors>().groups);
    {
        let mut zm = world.resource_mut::<Zm>();
        let zm = &mut *zm;
        for g in &mut groups {
            if !g.switches.iter().any(|s| pressed.contains(s)) {
                continue;
            }
            g.open = !g.open;
            if let (Some(s), Some(e)) = (
                &g.button_sound,
                g.switches.first().and_then(|n| zm.ents.get(n)),
            ) {
                sounds.push((e.origin, s.clone()));
            }
            diag::info!(
                Sim,
                "iw6 doors: {} {}",
                g.name,
                if g.open { "opens" } else { "closes" }
            );
            for p in &g.pieces {
                let Some(e) = zm.ents.get(&p.ent) else {
                    continue;
                };
                let (to, to_angles) = if g.open {
                    (p.def.end, p.def.end_angles)
                } else {
                    (p.def.start, p.def.start_angles)
                };
                // A move turned round takes the share of the time left.
                let full = (0..3)
                    .map(|k| (p.def.end[k] - p.def.start[k]).powi(2))
                    .sum::<f32>()
                    .sqrt();
                let left = (0..3)
                    .map(|k| (to[k] - e.origin[k]).powi(2))
                    .sum::<f32>()
                    .sqrt();
                let t = if full > 1e-3 {
                    (secs * left / full).clamp(0.05, secs)
                } else {
                    secs
                };
                let (from, from_angles) = (e.origin, e.angles);
                zm.movers.move_to(p.ent, from, to, now, t, 0.2 * t, 0.2 * t);
                if from_angles != to_angles {
                    zm.movers
                        .rotate_to(p.ent, from_angles, to_angles, now, t, 0.2 * t, 0.2 * t);
                }
                if let Some(s) = if g.open {
                    &p.def.start_sound
                } else {
                    &p.def.stop_sound
                } {
                    sounds.push((e.origin, s.clone()));
                }
            }
        }
    }
    world.resource_mut::<Iw6Doors>().groups = groups;
    for (at, s) in sounds {
        crate::script::host::natives::engine::play_sound_at(world, at, &s);
    }
    // Where each piece stands, for Ghosts' renderer.
    let poses: Vec<asset_world::Iw6MoverPose> = {
        let d = world.resource::<Iw6Doors>();
        let zm = world.resource::<Zm>();
        d.groups
            .iter()
            .flat_map(|g| g.pieces.iter())
            .filter_map(|p| {
                let e = zm.ents.get(&p.ent)?;
                Some(asset_world::Iw6MoverPose {
                    brush: p.def.brush,
                    model: p.def.model.clone(),
                    spawn_origin: p.def.start,
                    origin: e.origin,
                    angles: e.angles,
                    hidden: false,
                    slot: None,
                    skin: None,
                })
            })
            .collect()
    };
    let mut d = world.resource_mut::<Iw6Doors>();
    if poses != d.last_published {
        d.last_published = poses.clone();
        asset_world::set_iw6_mover_poses(poses);
    }
}
