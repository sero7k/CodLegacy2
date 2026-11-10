//! Events the sim hands to the scripts, with the engine's rules for them: damage on a script
//! entity (health, `damage` and `death` notifies) and a script's `radiusdamage`. World-independent
//! so the sim glue and the headless tests run the very same code.

use gsc_iw6_vm::{Value, Vm};

use super::host::{text_of, Iw6Host, F_CLASSNAME, F_HEALTH};

/// A hit on an entity, as the sim resolved it.
#[derive(Clone, Debug)]
pub struct DamageEvent {
    pub ent: u32,
    pub amount: i32,
    /// The attacker's host entity (None: the world / unknown).
    pub attacker: Option<u32>,
    pub dir: [f32; 3],
    pub point: [f32; 3],
    /// `MOD_RIFLE_BULLET`, `MOD_GRENADE_SPLASH`, ...
    pub means: String,
    pub weapon: String,
}

/// What a hit did to the entity.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// The entity does not take damage: nothing was said.
    Ignored,
    /// `damage` was notified.
    Hurt,
    /// Its health ran out: `damage`, then `death`.
    Died,
}

/// Damage triggers (placed by the map) tell the scripts about every hit; other entities only once
/// a script called `setcandamage(true)` on them (the engine's takedamage byte).
pub fn takes_damage(h: &Iw6Host, id: u32) -> bool {
    let Some(e) = h.ent(id) else { return false };
    e.takedamage || h.class(id).starts_with("trigger_damage")
}

/// A notify to send: (target, name, arguments).
pub type Notify = (Value, String, Vec<Value>);

/// What a hit says to the scripts, and the host-side effect of it (the health goes down):
/// at least 1 damage, `health -= damage`, `damage` with (amount, attacker, direction, point, MOD,
/// model, tag, part, flags, weapon), and when health is under 1 (not below -999) `death` with the
/// attacker. Damage triggers get the `damage` notify only. READ from the engine's damage function
/// (G_Damage on a non-client entity); the trailing arguments are what the scripts read.
pub fn damage_notifies(h: &mut Iw6Host, d: &DamageEvent) -> (Outcome, Vec<Notify>) {
    if !takes_damage(h, d.ent) {
        return (Outcome::Ignored, Vec::new());
    }
    let trigger = h.class(d.ent).starts_with("trigger");
    let amount = d.amount.max(1);
    let attacker = d.attacker.map_or(Value::Undefined, Iw6Host::value);
    let ent = Iw6Host::value(d.ent);
    let mut died = false;
    if !trigger {
        let health = h.field(d.ent, F_HEALTH).and_then(Value::as_int).unwrap_or(0);
        let left = (health - amount).max(-999);
        h.set_field(d.ent, F_HEALTH, Value::Int(left));
        died = left < 1;
    }
    let mut out = vec![(
        ent.clone(),
        "damage".to_owned(),
        vec![
            Value::Int(amount),
            attacker.clone(),
            Value::Vector(d.dir),
            Value::Vector(d.point),
            Value::str(&d.means),
            Value::str(""),
            Value::str(""),
            Value::str(""),
            Value::Int(0),
            Value::str(&d.weapon),
        ],
    )];
    if died {
        out.push((ent, "death".to_owned(), vec![attacker]));
    }
    (if died { Outcome::Died } else { Outcome::Hurt }, out)
}

/// `damage_notifies`, sent through the VM.
pub fn deliver_damage(vm: &mut Vm<Iw6Host>, d: &DamageEvent) -> Outcome {
    let (o, notes) = damage_notifies(vm.host_mut(), d);
    for (t, n, a) in notes {
        vm.notify(&t, &n, a);
    }
    o
}

/// The host's entities a blast at `origin` hurts, with the damage each takes: the engine's rule
/// (G_RadiusDamage) measures from the point to the entity's box, `damage = min + (max - min) *
/// (1 - dist / range)`, skips the inflictor, entities that cannot take (radius) damage and those
/// out of range. The line-of-sight trace of the engine is not made here. Returns (entity, amount,
/// direction from the blast).
pub fn radius_targets(h: &Iw6Host, origin: [f32; 3], range: f32, max: f32, min: f32, inflictor: u32) -> Vec<(u32, i32, [f32; 3])> {
    let range = range.max(1.0);
    let mut out = Vec::new();
    for id in 0..h.ents.len() as u32 {
        let Some(e) = h.ent(id) else { continue };
        if id == inflictor || !e.takedamage || e.no_radius_damage || e.client.is_some() || e.hidden {
            continue;
        }
        let c = h.origin(id);
        let (lo, hi) = h
            .models
            .get(&h.model_of(id))
            .map_or(([0.0; 3], [0.0; 3]), |m| (m.mins, m.maxs));
        let mut d2 = 0.0f32;
        for k in 0..3 {
            let (a, b) = (c[k] + lo[k], c[k] + hi[k]);
            let d = if origin[k] < a { a - origin[k] } else if origin[k] > b { origin[k] - b } else { 0.0 };
            d2 += d * d;
        }
        if d2 >= range * range {
            continue;
        }
        let f = 1.0 - d2.sqrt() / range;
        let amount = (min + (max - min) * f) as i32;
        if amount <= 0 {
            continue;
        }
        let mut dir = [c[0] - origin[0], c[1] - origin[1], c[2] - origin[2] + 24.0];
        let l = (dir[0] * dir[0] + dir[1] * dir[1] + dir[2] * dir[2]).sqrt();
        if l > 0.0 {
            dir = [dir[0] / l, dir[1] / l, dir[2] / l];
        }
        out.push((id, amount, dir));
    }
    out
}

/// The notifies of a script blast on the host's entities (the health goes down).
#[allow(clippy::too_many_arguments)]
pub fn radius_notifies(
    h: &mut Iw6Host,
    origin: [f32; 3],
    range: f32,
    max: f32,
    min: f32,
    attacker: Option<u32>,
    inflictor: u32,
    means: &str,
    weapon: &str,
) -> Vec<Notify> {
    let targets = radius_targets(h, origin, range, max, min, inflictor);
    let mut out = Vec::new();
    for (ent, amount, dir) in targets {
        let (_, mut n) = damage_notifies(h, &DamageEvent { ent, amount, attacker, dir, point: origin, means: means.to_owned(), weapon: weapon.to_owned() });
        out.append(&mut n);
    }
    out
}

/// Runs a script blast on the host's entities (`radiusdamage` from a script); returns the
/// outcomes.
pub fn apply_radius(
    vm: &mut Vm<Iw6Host>,
    origin: [f32; 3],
    range: f32,
    max: f32,
    min: f32,
    attacker: Option<u32>,
    inflictor: u32,
    means: &str,
    weapon: &str,
) -> Vec<(u32, Outcome)> {
    let targets = radius_targets(vm.host(), origin, range, max, min, inflictor);
    targets
        .into_iter()
        .map(|(ent, amount, dir)| {
            let o = deliver_damage(vm, &DamageEvent { ent, amount, attacker, dir, point: origin, means: means.to_owned(), weapon: weapon.to_owned() });
            (ent, o)
        })
        .collect()
}

/// For logs: the class of a host entity.
pub fn describe(h: &Iw6Host, id: u32) -> String {
    format!("{}#{id} ({})", h.class(id), h.field(id, F_CLASSNAME).map(text_of).unwrap_or_default())
}
