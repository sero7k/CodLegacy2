//! bo2mp: Black Ops III's client scripts (`.csc`) for a Zombies map played
//! with Black Ops III's own scripts (`docs/T7_ZOMBIES_PLAN.md`, "Client
//! scripts").
//!
//! The client side of the game is a second script program: the `.csc`
//! objects of the same zones, translated for the Black Ops II VM
//! (`gsc_t6::t7::translate_ext`) and run by a VM of its own on the same
//! world. What the server scripts do to the client is a stream of client
//! field changes (`clientfield::set`, `set_world`, `set_player_uimodel`);
//! the client scripts registered a callback for each field
//! (`registerclientfield(pool, name, version, bits, type, callback)`) and
//! the engine calls it, on the entity, with `(localClientNum, oldValue,
//! newValue, newEntity, initialSnapshot, fieldName, wasTimeJump)`. That is
//! what is done here: the server natives that set a field `note` it, and
//! each tick `advance` runs the callbacks of the fields that changed, on
//! stand-in objects for the entities. The callbacks draw with the engine
//! calls the client has (`playfx`, `playfxontag`, `playsound`...), which
//! are the same events the server-side natives raise, so their effects and
//! sounds reach the renderer and the mixer the way every other one does.
//!
//! What runs: the level start (the autoexecs, the map's and the game
//! type's `main`, `codecallback_preinitialization` /
//! `finalizeinitialization` / `localclientconnect`) and the field
//! callbacks. What does not yet draw anything: lighting states, fog and
//! sun banks, post-effect (`pstfx`) and visionset calls, exploders by
//! name, light and water-surface setters (they are accepted and counted:
//! `T7_CLIENT_REPORT`, `docs/T7_ZOMBIES_PLAN.md`). `BO2MP_T7_CLIENT=0`
//! leaves the client scripts off.

use std::collections::{BTreeMap, HashMap};

use bevy_ecs::prelude::{Resource, World};
use gsc_t6::{Hooks, ObjKind, ObjRef, Program, ScriptObject, Strings, Value, Vm};

use super::{Zm, arg, entnum};

/// Whether the client scripts are on (`BO2MP_T7_CLIENT=0|off|false` turns
/// them off).
pub(crate) fn enabled() -> bool {
    !std::env::var("BO2MP_T7_CLIENT").is_ok_and(|v| matches!(v.trim(), "0" | "off" | "false"))
}

/// A client field change waiting for its callbacks.
#[derive(Clone, Debug)]
struct Change {
    /// The entity it is set on (`None`: the world's).
    ent: Option<u32>,
    name: String,
    value: i64,
}

/// The client side's state: the registered callbacks, the changes not yet
/// delivered and what was seen (for the report).
#[derive(Resource, Default)]
pub(crate) struct T7Client {
    /// Field name -> (pool, callback) of each registration.
    callbacks: HashMap<String, Vec<(String, Value)>>,
    queue: Vec<Change>,
    /// A field's value as last delivered, by (entity or 0 for the world's, name).
    last: HashMap<(u32, String), i64>,
    started: bool,
    /// Stand-in objects in the client VM for the entities.
    proxies: HashMap<u32, ObjRef>,
    /// Callbacks run, by field name.
    pub(crate) delivered: BTreeMap<String, u64>,
    /// Builtins the client scripts called that are not bound, with counts.
    pub(crate) unbound: BTreeMap<String, u64>,
    /// Effects and sounds the client scripts raised.
    pub(crate) drawn: u64,
}

/// The client VM, held apart from the world as the server's is.
#[derive(Resource)]
pub(crate) struct T7ClientRt {
    vm: Vm<World>,
    autoexecs: Vec<(String, String)>,
    map: String,
    gametype: String,
}

/// The client scripts of a map from its zones' script objects: the
/// translated program and its autoexec functions, if it has any.
pub(super) fn program_objects(bytes: &[Vec<u8>]) -> Option<(Vec<ScriptObject>, Vec<(String, String)>)> {
    let objects: Vec<_> = bytes.iter().filter_map(|b| gsc_t6::t7::T7Object::parse(b).ok()).collect();
    if !objects.iter().any(|o| o.name.to_ascii_lowercase().ends_with(".csc")) {
        return None;
    }
    match gsc_t6::t7::translate_ext(&objects, ".csc") {
        Ok((translated, summary)) => {
            let autoexecs = gsc_t6::t7::autoexecs(&translated, &gsc_t6::t7::include_lists(&objects));
            diag::info!(Sim, "bo2mp t7 client: {} client scripts ({} functions, {} autoexecs), {} builtins called", summary.scripts, summary.functions, autoexecs.len(), summary.builtins.len());
            Some((translated, autoexecs))
        }
        Err(e) => {
            diag::warn!(Sim, "bo2mp t7 client: client scripts not translated: {e}");
            None
        }
    }
}

/// Build the client VM and file it with the world (does nothing when the
/// map has no client scripts).
pub(super) fn install(world: &mut World, bytes: &[Vec<u8>], map: &str, gametype: &str) {
    if !enabled() {
        return;
    }
    let Some((objects, autoexecs)) = program_objects(bytes) else { return };
    let mut strings = Strings::default();
    let program = match Program::link(objects, &mut strings) {
        Ok(p) => p,
        Err(e) => {
            diag::warn!(Sim, "bo2mp t7 client: not linked: {e}");
            return;
        }
    };
    let mut vm: Vm<World> = Vm::new(program, strings);
    vm.t7_events = true;
    // (The engine side is the server's where it is the same: the entities,
    // the effects and the sounds. The server's Black Ops III natives are
    // not bound: they keep their weapon objects and bundles in the world,
    // as the server VM's objects.)
    gsc_t6::natives::bind_core(&mut vm);
    super::natives_game::bind(&mut vm);
    super::natives_ent::bind(&mut vm);
    super::natives_fx::bind(&mut vm);
    bind(&mut vm);
    vm.hooks = Hooks { get_field: Some(super::fields::get), set_field: Some(super::fields::set), unbound: Some(unbound), ..Default::default() };
    world.insert_resource(T7Client::default());
    world.insert_resource(T7ClientRt { vm, autoexecs, map: map.to_owned(), gametype: gametype.to_owned() });
}

fn unbound(vm: &mut Vm<World>, world: &mut World, id: u32, _self: &Value, _args: &[Value]) -> Value {
    let (name, method) = vm.program.builtins[id as usize];
    let n = format!("{}{}", if method { "." } else { "" }, vm.str(name));
    if let Some(mut c) = world.get_resource_mut::<T7Client>() {
        *c.unbound.entry(n).or_insert(0) += 1;
    }
    Value::Undefined
}

/// A server script set a client field: its callbacks run on the next tick.
/// `ent` is the entity (`None` for the world's field).
pub(super) fn note(world: &mut World, ent: Option<u32>, name: &str, value: i64) {
    if let Some(mut c) = world.get_resource_mut::<T7Client>()
        && c.callbacks.contains_key(name)
    {
        c.queue.push(Change { ent, name: name.to_owned(), value });
    }
}

/// An effect's name as the client scripts spell it: a path, or a hash the
/// effect list does not know.
fn fx_text(vm: &Vm<World>, v: &Value) -> Option<String> {
    let t = vm.to_text(v);
    (!t.is_empty() && !t.starts_with('#') && t != "undefined").then_some(t)
}

fn bind(vm: &mut Vm<World>) {
    // registerclientfield(pool, name, version, bits, type, callback, ...).
    vm.bind("registerclientfield", false, |vm, world, _, a| {
        let pool = vm.to_text(arg(a, 0));
        let name = vm.to_text(arg(a, 1));
        if let Some(mut c) = world.get_resource_mut::<T7Client>()
            && matches!(arg(a, 5), Value::Func(_))
        {
            c.callbacks.entry(name).or_default().push((pool, arg(a, 5).clone()));
        }
        Ok(Value::Undefined)
    });
    // playfx(localClientNum, fx, origin [, forward [, up]]).
    vm.bind("playfx", false, |vm, world, _, a| {
        let origin = arg(a, 2).as_vec3().unwrap_or([0.0; 3]);
        let forward = arg(a, 3).as_vec3().unwrap_or([0.0, 0.0, 1.0]);
        let name = match arg(a, 1) {
            Value::Int(id) => super::natives_fx::fx_name(world, *id),
            v => fx_text(vm, v),
        };
        if let Some(name) = name {
            match arg(a, 4).as_vec3() {
                Some(up) => super::natives_fx::effect_axis(world, &name, origin, forward, up),
                None => super::natives_fx::effect(world, &name, origin, forward),
            }
            if let Some(mut c) = world.get_resource_mut::<T7Client>() {
                c.drawn += 1;
            }
        }
        Ok(Value::Int(0))
    });
    // playfxontag(localClientNum, fx, entity, tag): the effect follows the tag.
    vm.bind("playfxontag", false, |vm, world, _, a| {
        let name = match arg(a, 1) {
            Value::Int(id) => super::natives_fx::fx_name(world, *id),
            v => fx_text(vm, v),
        };
        if let (Some(name), Some(n)) = (name, entnum(vm, arg(a, 2))) {
            let tag = vm.to_text(arg(a, 3));
            super::clientfields::bolt_named_queued(world, n, &name, &tag);
            if let Some(mut c) = world.get_resource_mut::<T7Client>() {
                c.drawn += 1;
            }
        }
        Ok(Value::Int(0))
    });
    // The rest of the effect handles and the client-only presentation
    // calls: accepted, nothing to draw yet (see the module's notes).
    for name in [
        "stopfx",
        "deletefx",
        "killfx",
        "setfxignorepause",
        "setfxteam",
        "setlightingstate",
        "setworldfogactivebank",
        "setworldfogbank",
        "setpbgactivebank",
        "setexposureactivebank",
        "setvolumetricfogactivebank",
        "setvolumetricfogbank",
        "setvolumetricfogaltcolor",
        "setlitfogbank",
        "setsunlightcolor",
        "enablefoginwater",
        "forcestreamxmodel",
        "forcestreamxmodels",
        "streamermodelhint",
        "streamerrequestcleanup",
        "setuimodelvalue",
        "setmapcamerapos",
    ] {
        vm.bind(name, false, |_, _, _, _| Ok(Value::Int(0)));
    }
    // The dvars, with Black Ops III's defaults.
    vm.bind("getdvarstring", false, |vm, _, _, a| {
        let name = vm.to_text(arg(a, 0)).to_ascii_lowercase();
        Ok(match (vm.dvars.get(&name).cloned(), a.get(1)) {
            (Some(v), _) => vm.string(&v),
            (None, Some(d)) => d.clone(),
            (None, None) => vm.string(""),
        })
    });
    vm.bind("getdvarint", false, |vm, _, _, a| {
        let name = vm.to_text(arg(a, 0)).to_ascii_lowercase();
        let text = vm.dvars.get(&name).cloned().or_else(|| a.get(1).map(|d| vm.to_text(d))).unwrap_or_default();
        Ok(Value::Int(text.trim().parse::<f64>().unwrap_or(0.0) as i32))
    });
    vm.bind("getdvarfloat", false, |vm, _, _, a| {
        let name = vm.to_text(arg(a, 0)).to_ascii_lowercase();
        let text = vm.dvars.get(&name).cloned().or_else(|| a.get(1).map(|d| vm.to_text(d))).unwrap_or_default();
        Ok(Value::Float(text.trim().parse::<f32>().unwrap_or(0.0)))
    });
    // The field values the server last set, read back by the callbacks.
    vm.bind("codegetclientfield", false, |vm, world, _, a| {
        let name = vm.to_text(arg(a, 1));
        let v = entnum(vm, arg(a, 0)).map_or(0, |n| world.resource::<Zm>().client_fx.value(n, &name));
        Ok(Value::Int(v as i32))
    });
    vm.bind("issplitscreen", false, |_, _, _, _| Ok(Value::Int(0)));
    vm.bind("sessionmodeisonlinegame", false, |_, _, _, _| Ok(Value::Int(0)));
    vm.bind("getminbitcountfornum", false, |_, _, _, a| {
        let n = a.first().and_then(Value::as_int).unwrap_or(0).max(0) as u32;
        Ok(Value::Int((32 - n.leading_zeros()).max(1) as i32))
    });
    // The client's clock is the server's; no demo plays; the map's static
    // model lists are not kept (an empty list).
    vm.bind("getservertime", false, |vm, _, _, _| Ok(Value::Int(vm.time_ms as i32)));
    vm.bind("isdemoplaying", false, |_, _, _, _| Ok(Value::Int(0)));
    vm.bind("findstaticmodelindexarray", false, |_, _, _, _| Ok(Value::array(gsc_t6::Array::default())));
    vm.bind("associativearray", false, |_, _, _, _| Ok(Value::array(gsc_t6::Array::default())));
    // getlocalclientnumber / islocalplayer: one local client, number 0.
    vm.bind("getlocalclientnumber", false, |_, _, _, _| Ok(Value::Int(0)));
    // Zombies: the session mode the scripts ask for.
    vm.bind("sessionmodeiszombiesgame", false, |_, _, _, _| Ok(Value::Int(1)));
    vm.bind("sessionmodeismultiplayergame", false, |_, _, _, _| Ok(Value::Int(0)));
    vm.bind("sessionmodeiscampaigngame", false, |_, _, _, _| Ok(Value::Int(0)));
}

/// The stand-in object for entity `n` in the client VM.
fn proxy(c: &mut T7Client, vm: &mut Vm<World>, n: u32) -> ObjRef {
    *c.proxies.entry(n).or_insert_with(|| vm.alloc_object(ObjKind::Entity(n)))
}

/// One tick: the client level starts once the server's has, then the field
/// changes since the last tick are delivered and the client's threads run.
pub(crate) fn advance(world: &mut World, now: i64) {
    if !world.contains_resource::<T7ClientRt>() || !world.get_resource::<Zm>().is_some_and(|z| z.started) {
        return;
    }
    world.resource_scope(|world, mut rt: bevy_ecs::prelude::Mut<T7ClientRt>| {
        let rt = &mut *rt;
        let vm = &mut rt.vm;
        vm.time_ms = now;
        let first = {
            let mut c = world.resource_mut::<T7Client>();
            !std::mem::replace(&mut c.started, true)
        };
        if first {
            start(vm, world, &rt.autoexecs, &rt.map, &rt.gametype);
        }
        let changes = std::mem::take(&mut world.resource_mut::<T7Client>().queue);
        for ch in changes {
            deliver(vm, world, &ch);
        }
        vm.run_frame(world);
    });
}

/// The client's level start: the autoexecs, the game type's and the map's
/// `main`, the callbacks the engine makes (`codecallback_*`), the local
/// client connecting.
fn start(vm: &mut Vm<World>, world: &mut World, autoexecs: &[(String, String)], map: &str, gametype: &str) {
    let level = Value::Object(vm.level);
    let mut ran = 0;
    for (script, func) in autoexecs {
        if vm.spawn_named(world, script, func, level.clone(), Vec::new()).is_some() {
            ran += 1;
        }
    }
    vm.run_frame(world);
    let calls: [(String, &str, Vec<Value>); 5] = [
        (super::t7::CALLBACKS.to_owned(), "codecallback_preinitialization", Vec::new()),
        (format!("scripts/zm/gametypes/{gametype}"), "main", Vec::new()),
        (format!("scripts/zm/{map}"), "main", Vec::new()),
        (super::t7::CALLBACKS.to_owned(), "codecallback_finalizeinitialization", Vec::new()),
        (super::t7::CALLBACKS.to_owned(), "codecallback_localclientconnect", vec![Value::Int(0)]),
    ];
    for (script, func, args) in calls {
        vm.spawn_named(world, &script, func, level.clone(), args);
        vm.run_frame(world);
    }
    diag::info!(Sim, "bo2mp t7 client: client level started ({ran} of {} autoexecs, {} fields with callbacks)", autoexecs.len(), world.resource::<T7Client>().callbacks.len());
}

/// Run the callbacks of one field change.
fn deliver(vm: &mut Vm<World>, world: &mut World, ch: &Change) {
    let key = (ch.ent.unwrap_or(0), ch.name.clone());
    let (callbacks, old, new_ent, me) = {
        let mut c = world.resource_mut::<T7Client>();
        let Some(list) = c.callbacks.get(&ch.name).cloned() else { return };
        let prev = c.last.insert(key, ch.value);
        let me = ch.ent.map(|n| proxy(&mut c, vm, n));
        (list, prev.unwrap_or(0), prev.is_none(), me)
    };
    if prev_equal(old, ch.value, new_ent) {
        return;
    }
    for (pool, f) in callbacks {
        // A world field's callback is on the level, the rest on the entity.
        let world_pool = pool == "world";
        let s = match (world_pool, me) {
            (true, _) => Value::Object(vm.level),
            (false, Some(o)) => Value::Object(o),
            (false, None) => continue,
        };
        let args = vec![Value::Int(0), Value::Int(old as i32), Value::Int(ch.value as i32), Value::Int(i32::from(new_ent)), Value::Int(0), vm.string(&ch.name), Value::Int(0)];
        vm.call_value(world, &f, s, args);
        *world.resource_mut::<T7Client>().delivered.entry(ch.name.clone()).or_insert(0) += 1;
    }
}

/// A repeated value is not a change (the engine only sends changes).
fn prev_equal(old: i64, new: i64, first: bool) -> bool {
    !first && old == new
}

/// A line for the headless report: callbacks run by field, unbound client
/// builtins and what was drawn.
pub(crate) fn report(world: &World) -> Option<String> {
    let c = world.get_resource::<T7Client>()?;
    if let Some(rt) = world.get_resource::<T7ClientRt>() {
        let mut seen: Vec<&String> = Vec::new();
        for m in &rt.vm.messages {
            if !m.contains("unbound") && !seen.contains(&m) {
                seen.push(m);
            }
        }
        eprintln!("t7 client script messages ({}): {:#?}", seen.len(), &seen[..seen.len().min(40)]);
    }
    let delivered: u64 = c.delivered.values().sum();
    let mut un: Vec<_> = c.unbound.iter().collect();
    un.sort_by(|a, b| b.1.cmp(a.1));
    un.truncate(40);
    Some(format!("t7 client: {} fields with callbacks, {delivered} callbacks run ({:?}), {} effects raised, unbound client builtins {:?}", c.callbacks.len(), c.delivered, c.drawn, un))
}
