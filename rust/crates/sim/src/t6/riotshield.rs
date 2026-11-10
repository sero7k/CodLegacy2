//! The Zombie Shield as BO2's engine gives it to the zombies scripts
//! (`_zm_weap_riotshield`, `_zm_riotshield`; TranZit, Mob of the Dead and
//! Origins build one). Fire pressed with the shield in hand raises it (the
//! weapon's deploy states): the engine notifies `raise_riotshield`, the
//! script asks where it would stand (`canplaceriotshield`), and when fire
//! is let go the engine notifies `deploy_riotshield` (1 = planted, 0 = put
//! back). The
//! script then makes the standing shield's model and calls
//! `deployriotshield(player, shield)`, which hands back the trigger he uses
//! to pick it up again.

use bevy_ecs::prelude::World;
use gsc_t6::{Key, ObjKind, Value, Vm};

use super::{Ent, Zm, arg, entnum, origin_of, with_vm};
use crate::world::ClientId;

/// The pick-up trigger round the planted shield.
const PICKUP_RADIUS: f32 = 64.0;
const PICKUP_HEIGHT: f32 = 72.0;
/// Half a standing body: an actor's or player's middle (`getcentroid`).
const CENTROID_UP: f32 = 36.0;

pub(super) fn bind(vm: &mut Vm<World>) {
    // player canplaceriotshield(event): where the shield would stand, in
    // front of him (`["result"]`, `["origin"]`, `["angles"]`).
    vm.bind("canplaceriotshield", true, |vm, world, s, _| {
        let Some(me) = entnum(vm, s) else {
            return Err("canplaceriotshield needs a player".into());
        };
        let (ok, origin, angles) = super::turrets::place_spot(world, ClientId(me), None);
        let mut arr = gsc_t6::Array::new();
        let k = |vm: &mut Vm<World>, s: &str| Key::Str(vm.intern(s));
        arr.set(k(vm, "result"), Value::bool(ok));
        arr.set(k(vm, "origin"), Value::Vec3(origin));
        arr.set(k(vm, "angles"), Value::Vec3(angles));
        Ok(Value::array(arr))
    });
    // deployriotshield(player, shield): the trigger that picks it up again.
    vm.bind("deployriotshield", false, |vm, world, _, a| {
        let at = origin_of(vm, world, arg(a, 1)).unwrap_or([0.0; 3]);
        let angles = entnum(vm, arg(a, 1))
            .and_then(|n| world.resource::<Zm>().ents.get(&n).map(|e| e.angles))
            .unwrap_or([0.0; 3]);
        let n = world.resource_mut::<Zm>().alloc_entnum();
        let obj = vm.alloc_object(ObjKind::Entity(n));
        world.resource_mut::<Zm>().ents.insert(
            n,
            Ent {
                obj: Some(obj),
                classname: "trigger_radius_use".to_owned(),
                origin: at,
                angles,
                radius: PICKUP_RADIUS,
                height: PICKUP_HEIGHT,
                ..Default::default()
            },
        );
        if codlegacy_env::var_os("CODLEGACY_T6_PLACE").is_some() {
            diag::info!(
                Sim,
                "bo2zm t6 riotshield planted at ({:.0} {:.0} {:.0}), pick-up trigger ent{n}",
                at[0],
                at[1],
                at[2]
            );
        }
        Ok(Value::Object(obj))
    });
    // entity getcentroid(): the middle of a body, else its origin.
    vm.bind("getcentroid", true, |vm, world, s, _| {
        let n = entnum(vm, s);
        let o = origin_of(vm, world, s).unwrap_or([0.0; 3]);
        let zm = world.resource::<Zm>();
        let body = n.is_some_and(|n| zm.players.contains_key(&n) || zm.actors.contains_key(&n));
        Ok(Value::Vec3([o[0], o[1], o[2] + if body { CENTROID_UP } else { 0.0 }]))
    });
    // The held / standing shield's look and the "can't place" hint: drawing
    // only, nothing the game keeps.
    for name in [
        "setheldweaponmodel",
        "setplacementhint",
        "setriotshieldfailhint",
        "setscriptmoverflag",
    ] {
        vm.bind(name, true, |_, _, _, _| Ok(Value::Undefined));
    }
}

/// Each tick: the weapon tick's riot shield raises and plants (or
/// call-offs), told to the scripts.
pub(super) fn deploy_events(world: &mut World) {
    let events = std::mem::take(&mut world.resource_mut::<Zm>().shield_events);
    for (c, attempt) in events {
        let Some(obj) = world.resource::<Zm>().players.get(&c).map(|p| p.obj) else {
            continue;
        };
        let name = if attempt.is_some() {
            "deploy_riotshield"
        } else {
            "raise_riotshield"
        };
        if codlegacy_env::var_os("CODLEGACY_T6_PLACE").is_some() {
            diag::info!(Sim, "bo2zm t6 riotshield: {name} {attempt:?}");
        }
        with_vm(world, |vm, world| {
            let args: Vec<Value> = attempt.map(Value::bool).into_iter().collect();
            vm.notify_str(world, obj, name, &args);
        });
    }
}
