//! bo2mp: a Black Ops III Zombies map played with Black Ops III's own
//! scripts (`docs/T7_ZOMBIES_PLAN.md`). The map's compiled scripts
//! (`T6Install::t7_objects`, from its patch zones) are translated for the
//! Black Ops II VM (`gsc_t6::t7`) and run on this module's engine side,
//! which is Black Ops II's (`super`) with Black Ops III's differences:
//! - the level start: the autoexec functions first (each script after
//!   those it includes), then the map's entities, each script struct made
//!   with its keys and `struct::init` run on it, then `callbacks_shared`'s
//!   `codecallback_preinitialization` (the systems' `__init__`s), the game
//!   type's `main`, the map's `main`, `codecallback_finalizeinitialization`
//!   (their `__main__`s) and `codecallback_startgametype`;
//! - the engine's callbacks are `callbacks_shared`'s (same names).
//!
//! Not part of upstream IW4L.

use bevy_ecs::prelude::World;
use gsc_t6::{ObjRef, ScriptObject, Value, Vm};

/// Black Ops III's engine callbacks script.
pub(crate) const CALLBACKS: &str = "scripts/shared/callbacks_shared";

/// The program's objects from Black Ops III's script objects (zone load
/// order), and the autoexec functions to start with.
pub(super) fn program_objects(bytes: &[Vec<u8>]) -> Result<(Vec<ScriptObject>, Vec<(String, String)>), String> {
    let mut objects = Vec::with_capacity(bytes.len());
    // bo2mp: an object that does not decode is left out and named (a
    // Workshop map's own copy of a script, built with other tools: Town
    // Remastered's `_zm_perk_electric_cherry.csc`), so the rest of the
    // map's scripts still run; `BO2MP_T7_STRICT_OBJECTS=1` fails the
    // install on it, as before.
    let strict = std::env::var("BO2MP_T7_STRICT_OBJECTS").is_ok_and(|v| v.trim() == "1");
    let mut left_out = 0;
    for b in bytes {
        match gsc_t6::t7::T7Object::parse(b) {
            Ok(o) => objects.push(o),
            Err(e) if !strict => {
                left_out += 1;
                diag::warn!(Sim, "bo2mp t7: script object left out: {e}");
            }
            Err(e) => return Err(e),
        }
    }
    if left_out > 0 {
        diag::warn!(Sim, "bo2mp t7: {left_out} of {} script objects left out (do not decode)", bytes.len());
    }
    let (translated, summary) = gsc_t6::t7::translate(&objects)?;
    let autoexecs = gsc_t6::t7::autoexecs(&translated, &gsc_t6::t7::include_lists(&objects));
    let unnamed = summary.builtins.iter().filter(|b| b.starts_with('#')).count();
    diag::info!(
        Sim,
        "bo2mp t7: {} Black Ops III scripts ({} functions, {} autoexecs), {} builtins called ({unnamed} unnamed), {} calls into other modes' scripts",
        summary.scripts,
        summary.functions,
        autoexecs.len(),
        summary.builtins.len(),
        summary.missing.len()
    );
    Ok((translated, autoexecs))
}

/// Run the autoexec functions, in the linker's order.
pub(super) fn run_autoexecs(vm: &mut Vm<World>, world: &mut World, autoexecs: &[(String, String)]) {
    let level = Value::Object(vm.level);
    let mut ran = 0;
    for (script, func) in autoexecs {
        if vm.spawn_named(world, script, func, level.clone(), Vec::new()).is_some() {
            ran += 1;
        }
    }
    diag::info!(Sim, "bo2mp t7: {ran} of {} autoexecs ran, {} threads", autoexecs.len(), vm.thread_count());
}

/// The engine's level start calls after the autoexecs and entities:
/// (script, function, required).
pub(super) fn start_calls(map: &str, gametype: &str) -> Vec<(String, &'static str, bool)> {
    let mut calls = vec![
        // (Runs every registered system's `__init__`, dependencies first.)
        (CALLBACKS.to_owned(), "codecallback_preinitialization", true),
        (format!("scripts/zm/gametypes/{gametype}"), "main", true),
        (format!("scripts/zm/{map}"), "main", true),
        // (The systems' `__main__`s: `spawnlogic::main` makes
        // `level.players`, which the game type's start reads.)
        (CALLBACKS.to_owned(), "codecallback_finalizeinitialization", true),
        (CALLBACKS.to_owned(), "codecallback_startgametype", true),
    ];
    // The difficulty tables (`gameskill_shared`'s setup, `setSkill` in the
    // game): its tables are string tables of the shared zones
    // (`core_common`), which a headless run does not have, so every
    // player connect's co-op health scale reads undefined there. Off
    // unless `BO2MP_T7_GAMESKILL=1` (it reads those tables when they are
    // loaded).
    if std::env::var("BO2MP_T7_GAMESKILL").is_ok_and(|v| matches!(v.as_str(), "1" | "on" | "true")) {
        calls.insert(2, ("scripts/shared/gameskill_shared".to_owned(), "#a1bbf812", false));
    }
    calls
}

/// A map script struct: a struct with its keys (typed as the engine does),
/// then `self struct::init()`.
pub(super) fn spawn_struct(vm: &mut Vm<World>, world: &mut World, keys: &[(String, String)], typed: impl Fn(&mut Vm<World>, &str, &str) -> Value) {
    let s = vm.new_struct();
    let Value::Object(o) = s else { return };
    for (k, v) in keys {
        let key = k.to_ascii_lowercase();
        let val = typed(vm, &key, v);
        set_key(vm, o, &key, val);
    }
    if vm.spawn_named(world, "scripts/codescripts/struct", "init", s, Vec::new()).is_none() {
        vm.report_once("bo2mp t7: no struct::init; map structs are not filed".to_owned());
    }
}

/// A map key on an entity, struct or node: under its name, and under the
/// translated scripts' spelling when the dictionary lacks it (`#<hash>`):
/// Zetsubou No Shima's skull altars are keyed by `script_special`, which
/// its scripts read as `#7cc697d0`.
pub(crate) fn set_key(vm: &mut Vm<World>, o: ObjRef, key: &str, val: Value) {
    let f = vm.intern(key);
    let h = field_name(vm, key);
    if h != f {
        vm.set_raw_field(o, h, val.clone());
    }
    vm.set_raw_field(o, f, val);
}

/// A field name as the translated scripts spell it: its name when the
/// dictionary has it, else `#<canonical hash>`.
pub(crate) fn field_name(vm: &mut Vm<World>, name: &str) -> gsc_t6::Str {
    let text = gsc_t6::t7::names::display(gsc_t6::t7::canon(name));
    vm.intern(&text)
}

/// The zones' script bundles as the engine hands them to the scripts at
/// level load: per bundle a struct with its pairs as fields (kind 1 an
/// int, 2 a float, the rest their text) and its objects' as structs in
/// `objects`, then `struct::createstruct(bundle, type, name)` (which files
/// it in `level.scriptbundles[type][name]`; a scene's is readied there).
pub(super) fn create_bundles(vm: &mut Vm<World>, world: &mut World) {
    let bundles = world.get_resource_mut::<T7State>().map(|mut s| std::mem::take(&mut s.bundles)).unwrap_or_else(Vec::new);
    let count = bundles.len();
    for (name, pairs) in &bundles {
        let Value::Object(b) = vm.new_struct() else { continue };
        let mut objects: std::collections::BTreeMap<i32, ObjRef> = std::collections::BTreeMap::new();
        let mut ty = String::new();
        // A scene's objects carry the editor's player keys whatever their
        // type; the scripts take `isdefined(_s.player)` for "this object
        // is a player" (and a prop or vehicle so marked is deleted when its
        // scene ends: the train). Only a player object keeps them.
        let player_objects: std::collections::BTreeSet<i32> =
            pairs.iter().filter(|p| p.0 >= 0 && p.1.eq_ignore_ascii_case("type") && p.3.eq_ignore_ascii_case("player")).map(|p| p.0).collect();
        for (object, key, kind, value, data) in pairs {
            if *object >= 0 && key.eq_ignore_ascii_case("player") && !player_objects.contains(object) {
                continue;
            }
            let v = match kind {
                1 => Value::Int(*data as i32),
                2 => Value::Float(f32::from_bits(*data)),
                _ => vm.string(value),
            };
            let target = if *object < 0 {
                if key == "type" {
                    ty.clone_from(value);
                }
                b
            } else if let Some(o) = objects.get(object) {
                *o
            } else {
                let Value::Object(o) = vm.new_struct() else { continue };
                objects.insert(*object, o);
                o
            };
            let f = field_name(vm, &key.to_ascii_lowercase());
            vm.set_raw_field(target, f, v);
        }
        let mut list = gsc_t6::Array::new();
        for o in objects.values() {
            list.push(Value::Object(*o));
        }
        let f = field_name(vm, "objects");
        vm.set_raw_field(b, f, Value::array(list));
        if ty.is_empty() {
            continue;
        }
        // (A vehicle's settings, by the vehicle type they are for.)
        if ty == "vehiclecustomsettings" {
            let vt = pairs.iter().find(|p| p.0 < 0 && p.1.eq_ignore_ascii_case("vehicletype")).map(|p| p.3.to_ascii_lowercase());
            if let (Some(vt), Some(mut st)) = (vt, world.get_resource_mut::<T7State>()) {
                st.vehicle_settings.push((vt, name.to_ascii_lowercase()));
            }
        }
        let (t, n) = (vm.string(&ty), vm.string(&name.to_ascii_lowercase()));
        vm.spawn_named(world, "scripts/codescripts/struct", "createstruct", Value::Object(vm.level), vec![Value::Object(b), t, n]);
    }
    diag::info!(Sim, "bo2mp t7: {count} script bundles made (struct::createstruct)");
}

/// A map path node (`node_*`): an object with its keys, kept for
/// `getnode` & co.
pub(super) fn spawn_node(vm: &mut Vm<World>, world: &mut World, keys: &[(String, String)], typed: impl Fn(&mut Vm<World>, &str, &str) -> Value) {
    let s = vm.new_struct();
    let Value::Object(o) = s else { return };
    for (k, v) in keys {
        let key = k.to_ascii_lowercase();
        let val = typed(vm, &key, v);
        set_key(vm, o, &key, val);
    }
    if let Some(mut st) = world.get_resource_mut::<T7State>() {
        st.nodes.push(o);
    }
}

// ---- the engine side's differences ---------------------------------------

/// Black Ops III's engine state beside Black Ops II's (`Zm`).
#[derive(bevy_ecs::prelude::Resource, Default)]
pub(crate) struct T7State {
    /// The weapon objects (`getweapon`): one struct per weapon name, so the
    /// scripts compare and key arrays by them as Black Ops III does.
    weapons: std::collections::HashMap<String, ObjRef>,
    weapon_names: std::collections::HashMap<ObjRef, String>,
    /// The map's path nodes (`spawn_node`).
    pub nodes: Vec<ObjRef>,
    /// The zones' script bundles until the level starts (`create_bundles`).
    pub bundles: Vec<super::T7Bundle>,
    /// Players in a developer move mode ("ufo", "noclip"): `isinmovemode`
    /// (the scripts leave such a player alone out of the playable area).
    pub move_mode: std::collections::HashMap<u32, String>,
    /// Kit weapon name -> the Black Ops III weapon it was given for (the
    /// engine's answers name the weapon the scripts asked for).
    stand_in_of: std::collections::HashMap<String, String>,
    /// (vehicle type, its `vehiclecustomsettings` bundle).
    pub vehicle_settings: Vec<(String, String)>,
}

impl T7State {
    /// A vehicle's settings bundle (`scriptbundlesettings`) for its vehicle
    /// type: the bundle whose vehicle type its name holds, the longest
    /// such (`spawner_bo3_parasite_enemy_tool`: `parasitesettings`).
    pub(crate) fn settings_for(&self, vehicletype: &str) -> Option<String> {
        let vt = vehicletype.to_ascii_lowercase();
        self.vehicle_settings.iter().filter(|(t, _)| !t.is_empty() && vt.contains(t.as_str())).max_by_key(|(t, _)| t.len()).map(|(_, n)| n.clone())
    }
}

impl T7State {
    pub(super) fn with_bundles(bundles: Vec<super::T7Bundle>) -> Self {
        Self { bundles, ..Default::default() }
    }
}

/// The weapon object of a name ("none" included).
pub(crate) fn weapon_object(vm: &mut Vm<World>, world: &mut World, name: &str) -> Value {
    // (Black Ops III's weapon lookup ignores the blanks a table cell can
    // carry: Gorod Krovi's box list names `dragon_gauntlet `, which the
    // box gave and nothing could hold.)
    let name = name.trim().to_ascii_lowercase();
    if let Some(&o) = world.get_resource::<T7State>().and_then(|s| s.weapons.get(&name)) {
        if vm.alive(o) {
            return Value::Object(o);
        }
    }
    let v = vm.new_struct();
    let Value::Object(o) = v else { return Value::Undefined };
    let text = vm.string(&name);
    for f in ["name", "rootweapon", "displayname"] {
        let k = field_name(vm, f);
        let val = if f == "rootweapon" { v.clone() } else { text.clone() };
        vm.set_raw_field(o, k, val);
    }
    let isnone = field_name(vm, "isnone");
    vm.set_raw_field(o, isnone, Value::Int(i32::from(name == "none")));
    if let Some(mut s) = world.get_resource_mut::<T7State>() {
        s.weapons.insert(name.clone(), o);
        s.weapon_names.insert(o, name.clone());
    }
    weapon_fields(vm, world, o, &name);
    v
}

/// What a weapon is, from its name (Black Ops III's naming: `ar_standard`,
/// `pistol_revolver38_upgraded`, `frag_grenade`): (weapclass,
/// inventorytype, primary, lethal grenade, tactical grenade, melee).
fn weapon_kind(name: &str) -> (&'static str, &'static str, bool, bool, bool, bool) {
    let base = name.trim_end_matches("_upgraded");
    let class = |p: &str| base.starts_with(p);
    if name == "none" {
        ("", "", false, false, false, false)
    } else if class("pistol_") {
        ("pistol", "primary", true, false, false, false)
    } else if class("smg_") {
        ("smg", "primary", true, false, false, false)
    } else if class("ar_") || class("sniper_") {
        ("rifle", "primary", true, false, false, false)
    } else if class("lmg_") {
        ("mg", "primary", true, false, false, false)
    } else if class("shotgun_") {
        ("spread", "primary", true, false, false, false)
    } else if class("launcher_") || class("ray_gun") || class("raygun") || class("idgun") || class("thundergun") {
        ("rocketlauncher", "primary", true, false, false, false)
    } else if base.contains("grenade") || class("bouncingbetty") {
        ("grenade", "offhand", false, true, false, false)
    } else if class("octobomb") || class("cymbal_monkey") || class("emp_grenade") {
        ("grenade", "offhand", false, false, true, false)
    } else if base.contains("knife") || class("zombie_fists") || class("bowie") || base.contains("sword") || base.contains("glaive") {
        ("melee", "melee", false, false, false, true)
    } else {
        ("", "item", false, false, false, false)
    }
}

/// The engine's fields of a weapon object that the scripts read: its
/// ammo (the catalog's, else a pistol's), its alternate and dual-wield
/// weapons (none), its class and inventory type, and its flags.
fn weapon_fields(vm: &mut Vm<World>, world: &mut World, o: gsc_t6::ObjRef, name: &str) {
    let (class, inv, primary, lethal, tactical, melee) = weapon_kind(name);
    let facts = super::weapon(world, name).ok().filter(|&w| w != 0).and_then(|w| super::frame(world).combat_facts_for(w));
    let (clip, max, start) = match (&facts, name) {
        (_, "none") => (0, 0, 0),
        (Some(f), _) => (f.clip_size, f.max_ammo, f.start_ammo),
        (None, _) if primary => (8, 80, 80),
        (None, _) => (if lethal || tactical { 1 } else { 0 }, if lethal || tactical { 4 } else { 0 }, if lethal || tactical { 2 } else { 0 }),
    };
    let none = if name == "none" { Value::Object(o) } else { weapon_object(vm, world, "none") };
    let text = |vm: &mut Vm<World>, t: &str| vm.string(t);
    let ints: [(&str, i32); 22] = [
        ("clipsize", clip),
        ("maxammo", max),
        ("startammo", start),
        ("fuellife", 0),
        ("isprimary", i32::from(primary)),
        ("islethalgrenade", i32::from(lethal)),
        ("istacticalgrenade", i32::from(tactical)),
        ("isgrenadeweapon", i32::from(lethal || tactical)),
        ("ismeleeweapon", i32::from(melee)),
        ("isbulletweapon", i32::from(primary && class != "rocketlauncher")),
        ("isprojectileweapon", i32::from(class == "rocketlauncher")),
        ("isdualwield", 0),
        ("isaltmode", 0),
        ("isgadget", 0),
        ("isheroweapon", 0),
        ("isballisticknife", 0),
        ("isriotshield", 0),
        ("isclipbased", i32::from(primary)),
        ("iscliponly", 0),
        ("isreloadable", i32::from(primary)),
        ("isitem", i32::from(inv == "item")),
        ("dualwieldcount", 0),
    ];
    for (f, n) in ints {
        let k = field_name(vm, f);
        vm.set_raw_field(o, k, Value::Int(n));
    }
    for (f, t) in [("weapclass", class), ("inventorytype", inv), ("type", if class == "rocketlauncher" { "projectile" } else if lethal || tactical { "grenade" } else if melee { "melee" } else { "bullet" }), ("firetype", "Full Auto")] {
        let k = field_name(vm, f);
        let v = text(vm, t);
        vm.set_raw_field(o, k, v);
    }
    for f in ["altweapon", "dualwieldweapon"] {
        let k = field_name(vm, f);
        vm.set_raw_field(o, k, none.clone());
    }
}

/// For Black Ops III's scripts: a weapon name as its weapon object (the
/// engine's callbacks and notifies hand scripts weapons); anything else
/// as it is, and everything as it is on a Black Ops II map.
pub(crate) fn weapon_value(vm: &mut Vm<World>, world: &mut World, v: Value) -> Value {
    if world.get_resource::<T7State>().is_none() {
        return v;
    }
    match v {
        Value::Str(s) => {
            let n = vm.str(s).to_owned();
            returned_weapon(vm, world, &n)
        }
        v => v,
    }
}

/// A weapon object's name, if the value is one.
fn weapon_name(world: &World, v: &Value) -> Option<String> {
    let Value::Object(o) = v else { return None };
    world.get_resource::<T7State>()?.weapon_names.get(o).cloned()
}

/// Builtins whose Black Ops II form gives a weapon's name where Black Ops
/// III's gives the weapon (or a list of them).
const RETURNS_WEAPONS: &[&str] = &[
    "getcurrentweapon",
    "getcurrentoffhand",
    "getweaponslist",
    "getweaponslistprimaries",
    "getstowedweapon",
    "getcurrentweaponaltweapon",
    "getlastweapon",
    "getweaponaltweapon",
];

/// Black Ops III Zombies weapons the match's catalog has under Black Ops
/// II's Zombies name (the kit's): Black Ops III's guns are in the catalog
/// under their own names (`lane::t7`'s guests), these are not.
const KIT_NAMES: &[(&str, &str)] = &[
    ("knife", "knife_zm"),
    ("bowie_knife", "bowie_knife_zm"),
    ("frag_grenade", "frag_grenade_zm"),
    ("bouncingbetty", "claymore_zm"),
    ("cymbal_monkey", "cymbal_monkey_zm"),
    ("zombie_fists", "zombie_fists_zm"),
];

/// Builtins bound here that read weapon objects as they are.
const KEEPS_WEAPONS: &[&str] = &["isweapon", "getbuildkitweapon", "getbuildkitweaponoptions", "getbuildkitattachmentcosmeticvariantindexes"];

/// A weapon name for Black Ops II's builtins: the catalog's, else the
/// kit's stand-in.
fn kit_name(world: &mut World, name: String) -> String {
    // (The kit's Zombies equipment first when the match has it: a catalog
    // entry under the Black Ops III name, the headless test's, is no
    // grenade to throw.)
    if let Some((_, t6)) = KIT_NAMES.iter().find(|(t7, _)| *t7 == name)
        && super::weapon(world, t6).is_ok_and(|w| w != 0)
    {
        return (*t6).to_owned();
    }
    if super::weapon(world, &name).is_ok() {
        return name;
    }
    if let Some((_, t6)) = KIT_NAMES.iter().find(|(t7, _)| *t7 == name) {
        return (*t6).to_owned();
    }
    // Its `_zm` form, else a Black Ops II gun of its class (the kit's
    // stand-ins until Black Ops III's own weapons are read): an upgraded
    // one's upgraded form first.
    let upgraded = name.contains("_upgraded");
    let base = name.replace("_upgraded", "");
    let mut tries = vec![format!("{name}_zm")];
    for (prefix, guns) in STAND_INS {
        if base.starts_with(prefix) {
            for g in *guns {
                if upgraded {
                    tries.push(format!("{g}_upgraded_zm"));
                }
                tries.push(format!("{g}_zm"));
                tries.push((*g).to_owned());
            }
            break;
        }
    }
    tries.into_iter().find(|t| super::weapon(world, t).is_ok_and(|w| w != 0)).unwrap_or(name)
}

/// Black Ops III's Zombies weapons by name prefix, and the Black Ops II
/// weapons (without `_zm`) that stand in for them, nearest first.
const STAND_INS: &[(&str, &[&str])] = &[
    ("pistol_revolver", &["python", "judge", "m1911"]),
    ("pistol_", &["m1911", "fiveseven", "beretta93r", "kard", "c96"]),
    ("smg_", &["mp5k", "ak74u", "pdw57", "qcw05", "thompson", "mp40", "evoskorpion"]),
    ("ar_", &["m14", "m16", "galil", "an94", "fnfal", "tar21", "type95", "mp44", "scar"]),
    ("lmg_", &["rpd", "hamr", "lsat", "mg08"]),
    ("shotgun_", &["870mcs", "rottweil72", "ksg", "saiga12", "srm1216"]),
    ("sniper_", &["dsr50", "barretm82", "svu", "ballista"]),
    ("launcher_", &["usrpg", "m32"]),
    // (Black Ops III's wonder and quest weapons, its own models not read
    // yet: the nearest Black Ops II weapon that fires so a box or quest
    // gives something that shoots. Origins' staffs keep Black Ops II's
    // own, `staff_fire` -> `staff_fire_zm`, tried first above.)
    ("thundergun", &["thundergun", "ray_gun"]),
    ("tesla_gun", &["ray_gun"]),
    ("shrink_ray", &["slipgun", "ray_gun"]),
    ("microwavegun", &["ray_gun"]),
    ("raygun_mark3", &["raygun_mark2", "ray_gun"]),
    ("elemental_bow", &["ray_gun"]),
    ("dragon_gauntlet", &["ray_gun"]),
    ("hero_mirg2000", &["raygun_mark2", "ray_gun"]),
    ("hero_annihilator", &["python", "judge"]),
    ("hero_gravityspikes", &["tomb_shield", "riotshield"]),
    ("glaive_", &["knife_ballistic", "bowie_knife"]),
    ("ray_gun", &["ray_gun"]),
    ("raygun_mark2", &["raygun_mark2"]),
    ("idgun", &["raygun_mark2", "ray_gun"]),
    ("octobomb", &["cymbal_monkey"]),
    ("zombie_perk_bottle_staminup", &["zombie_perk_bottle_marathon"]),
    ("zombie_perk_bottle_widows_wine", &["zombie_perk_bottle_cherry"]),
    ("zombie_perk_bottle_doubletap", &["zombie_perk_bottle_doubletap"]),
    ("zombie_perk_bottle_", &["zombie_perk_bottle_jugg"]),
    // (The beast's lightning a gun that fires, so his shots reach what it
    // powers; his grapple and hands the fists.)
    ("zombie_beast_lightning", &["ray_gun", "m1911"]),
    ("zombie_beast_", &["zombie_fists"]),
    ("syrette", &["syrette"]),
    ("zombie_knuckle_crack", &["zombie_knuckle_crack"]),
    ("zombie_builder", &["zombie_builder"]),
];

#[allow(clippy::ptr_arg)] // (The hook's type.)
fn native_args(vm: &mut Vm<World>, world: &mut World, id: u32, args: &mut Vec<Value>) {
    // (Black Ops III's own builtins here take the weapon object.)
    let (name, _) = vm.program.builtins[id as usize];
    if KEEPS_WEAPONS.contains(&vm.str(name)) {
        return;
    }
    for a in args.iter_mut() {
        if let Some(n) = weapon_name(world, a) {
            let kit = kit_name(world, n.clone());
            if kit != n
                && let Some(mut st) = world.get_resource_mut::<T7State>()
            {
                st.stand_in_of.insert(kit.clone(), n);
            }
            *a = vm.string(&kit);
        }
    }
}

/// The weapon object a builtin's weapon name means: the Black Ops III
/// weapon a kit stand-in was given for, else its own.
fn returned_weapon(vm: &mut Vm<World>, world: &mut World, n: &str) -> Value {
    let n = if n.is_empty() { "none" } else { n };
    let own = world.get_resource::<T7State>().and_then(|st| st.stand_in_of.get(n).cloned());
    weapon_object(vm, world, own.as_deref().unwrap_or(n))
}

fn native_return(vm: &mut Vm<World>, world: &mut World, id: u32, v: &mut Value) {
    let (name, _) = vm.program.builtins[id as usize];
    if !RETURNS_WEAPONS.contains(&vm.str(name)) {
        return;
    }
    match v.clone() {
        Value::Str(s) => {
            let n = vm.str(s).to_owned();
            *v = returned_weapon(vm, world, &n);
        }
        Value::Array(a) => {
            let mut out = gsc_t6::Array::new();
            for x in a.snapshot().values_in_order().cloned().collect::<Vec<_>>() {
                if let Value::Str(s) = x {
                    let n = vm.str(s).to_owned();
                    out.push(returned_weapon(vm, world, &n));
                } else {
                    out.push(x);
                }
            }
            *v = Value::array(out);
        }
        Value::Undefined => *v = weapon_object(vm, world, "none"),
        _ => {}
    }
}

/// Bind Black Ops III's own builtins (over Black Ops II's of the same
/// name) and the weapon hooks.
pub(super) fn bind(vm: &mut Vm<World>) {
    vm.t7_events = true;
    vm.hooks.native_args = Some(native_args);
    vm.hooks.native_return = Some(native_return);
    vm.bind("getweapon", false, |vm, world, _, a| {
        let name = match a.first() {
            Some(Value::Str(s) | Value::IStr(s)) => vm.str(*s).to_owned(),
            Some(other) => weapon_name(world, other).unwrap_or_else(|| "none".to_owned()),
            None => "none".to_owned(),
        };
        Ok(weapon_object(vm, world, &name))
    });
    // A player's build kit (his chosen attachments and camo for a gun):
    // none here, so the gun as asked, no options.
    vm.bind("getbuildkitweapon", true, |vm, world, _, a| Ok(weapon_value(vm, world, a.first().cloned().unwrap_or(Value::Undefined))));
    vm.bind("getbuildkitweaponoptions", true, |_, _, _, a| Ok(a.get(1).and_then(Value::as_int).map_or(Value::Int(0), Value::Int)));
    vm.bind("getbuildkitattachmentcosmeticvariantindexes", true, |_, _, _, _| Ok(Value::Undefined));
    vm.bind("isinmovemode", true, |vm, world, s, a| {
        let mode = super::entnum(vm, s).and_then(|n| world.get_resource::<T7State>().and_then(|st| st.move_mode.get(&n).cloned()));
        Ok(Value::Int(i32::from(mode.is_some_and(|m| a.iter().any(|v| vm.to_text(v).eq_ignore_ascii_case(&m))))))
    });
    vm.bind("isweapon", false, |_, world, _, a| Ok(Value::Int(i32::from(a.first().is_some_and(|v| weapon_name(world, v).is_some())))));
    // getdvarstring(name [, default]).
    vm.bind("getdvarstring", false, |vm, _, _, a| {
        let name = a.first().map(|v| vm.to_text(v).to_ascii_lowercase()).unwrap_or_else(String::new);
        let v = vm.dvars.get(&name).cloned();
        Ok(match (v, a.get(1)) {
            (Some(v), _) => vm.string(&v),
            (None, Some(d)) => d.clone(),
            (None, None) => vm.string(""),
        })
    });
    // getdvarint / getdvarfloat (name [, default]): Black Ops III's take a
    // default for a dvar nobody set.
    vm.bind("getdvarint", false, |vm, _, _, a| {
        let name = a.first().map(|v| vm.to_text(v).to_ascii_lowercase()).unwrap_or_else(String::new);
        Ok(match (vm.dvars.get(&name), a.get(1)) {
            (Some(v), _) => Value::Int(dvar_number(v) as i32),
            (None, Some(d)) => Value::Int(dvar_number(&vm.to_text(d)) as i32),
            (None, None) => Value::Int(0),
        })
    });
    vm.bind("getdvarfloat", false, |vm, _, _, a| {
        let name = a.first().map(|v| vm.to_text(v).to_ascii_lowercase()).unwrap_or_else(String::new);
        Ok(match (vm.dvars.get(&name), a.get(1)) {
            (Some(v), _) => Value::Float(dvar_number(v) as f32),
            (None, Some(d)) => Value::Float(dvar_number(&vm.to_text(d)) as f32),
            (None, None) => Value::Float(0.0),
        })
    });
    // modvar(name, value): the Mod Tools' devgui variable (a dvar the
    // developer menu edits). Workshop maps poll theirs (`add_bots`,
    // `round`, `camo`...) and reset it to 0 or "" after acting; unbound,
    // the reset never happened and the poll loop acted every frame (Five
    // added a test client per frame and never ran its round).
    vm.bind("modvar", false, |vm, _, _, a| {
        let name = a.first().map(|v| vm.to_text(v).to_ascii_lowercase()).unwrap_or_else(String::new);
        let value = a.get(1).map(|v| vm.to_text(v)).unwrap_or_else(String::new);
        vm.dvars.insert(name, value);
        Ok(Value::Undefined)
    });
    // addtestclient(): no bots in the Zombies sim.
    vm.bind("addtestclient", false, |_, _, _, _| Ok(Value::Undefined));
    // tablelookuprow(table, row): the row's cells.
    vm.bind("tablelookuprow", false, |vm, world, _, a| {
        let table = a.first().map(|v| vm.to_text(v).to_ascii_lowercase()).unwrap_or_else(String::new);
        let row = a.get(1).and_then(Value::as_int).unwrap_or(-1);
        let cells: Option<Vec<String>> = world.resource::<super::Zm>().tables.get(&table).and_then(|t| {
            let (r, c) = (usize::try_from(row).ok()?, t.columns.max(1));
            (r < t.rows).then(|| t.cells[r * c..(r + 1) * c].to_vec())
        });
        let Some(cells) = cells else { return Ok(Value::Undefined) };
        let mut out = gsc_t6::Array::new();
        for c in cells {
            out.push(vm.string(&c));
        }
        Ok(Value::array(out))
    });
    bind_strings(vm);
    bind_level(vm);
}

/// A dvar's text as a number (its leading number, as the engine reads it).
fn dvar_number(v: &str) -> f64 {
    let t = v.trim();
    let end = t.char_indices().take_while(|(i, c)| c.is_ascii_digit() || *c == '.' || (*i == 0 && (*c == '-' || *c == '+'))).count();
    t[..end].parse().unwrap_or(0.0)
}

/// Black Ops III's string builtins (`table_shared::load` reads every
/// string table cell through them).
fn bind_strings(vm: &mut Vm<World>) {
    vm.bind("strstrip", false, |vm, _, _, a| {
        let t = a.first().map(|v| vm.to_text(v)).unwrap_or_else(String::new);
        Ok(vm.string(t.trim()))
    });
    vm.bind("strisint", false, |vm, _, _, a| {
        let t = a.first().map(|v| vm.to_text(v)).unwrap_or_else(String::new);
        Ok(Value::Int(i32::from(!t.is_empty() && t.trim().parse::<i64>().is_ok())))
    });
    // strisnumber(v): a number, or text that reads as one (`waittill_any_ex`
    // takes a leading timeout this way).
    vm.bind("strisnumber", false, |vm, _, _, a| {
        Ok(Value::Int(i32::from(match a.first() {
            Some(Value::Int(_) | Value::Float(_)) => true,
            Some(Value::Str(s)) => vm.str(*s).trim().parse::<f64>().is_ok(),
            _ => false,
        })))
    });
    vm.bind("strisfloat", false, |vm, _, _, a| {
        let t = a.first().map(|v| vm.to_text(v)).unwrap_or_else(String::new);
        let t = t.trim();
        Ok(Value::Int(i32::from(t.contains('.') && t.parse::<f64>().is_ok())))
    });
    // hashstring(name): the name's canonical hash (`aat` keys its kinds by it).
    vm.bind("hashstring", false, |vm, _, _, a| {
        let t = a.first().map(|v| vm.to_text(v)).unwrap_or_else(String::new);
        Ok(Value::Int(gsc_t6::t7::canon(&t) as i32))
    });
    // tablelookupfindcoreasset(name): the table's name (one table store).
    vm.bind("tablelookupfindcoreasset", false, |_, _, _, a| Ok(a.first().cloned().unwrap_or(Value::Undefined)));
}

/// The level and player builtins Black Ops III's Zombies game type calls
/// that Black Ops II's has under other names or not at all.
fn bind_level(vm: &mut Vm<World>) {
    // Its `globallogic` reads the multiplayer settings: Zombies has no
    // create-a-class (the player spawns from the team pick).
    vm.bind("getgametypesetting", false, |vm, _, _, a| {
        let name = a.first().map(|v| vm.to_text(v).to_ascii_lowercase()).unwrap_or_else(String::new);
        Ok(setting(&name).map_or_else(|| super::natives_game::gametype_setting(&name), Value::Int))
    });
    for name in ["getaiteamarray", "getactorteamarray"] {
        if let Some(f) = vm.native_of("getaiarray", false) {
            vm.bind(name, false, f);
        }
    }
    // The map's AI spawners of a team (`actor_spawner_*`; all are the
    // zombies' on a Zombies map).
    vm.bind("getspawnerteamarray", false, spawner_team_array);
    vm.bind("#f68840c4", false, spawner_team_array);
    vm.bind("getvehiclespawnerarray", false, |vm, world, _, _| {
        let v = ents_where(vm, world, |e| e.classname == "script_vehicle").into_iter().filter(|v| is_spawner(vm, v)).collect();
        Ok(super::list(v))
    });
    vm.bind("isvehiclespawner", false, |vm, world, _, a| {
        let v = a.first().cloned().unwrap_or(Value::Undefined);
        let n = super::entnum(vm, &v);
        let vehicle = n.is_some_and(|n| world.resource::<super::Zm>().ents.get(&n).is_some_and(|e| e.classname == "script_vehicle"));
        Ok(Value::Int(i32::from(vehicle && is_spawner(vm, &v))))
    });
    // spawncollision(model, name, origin, angles): an invisible solid.
    vm.bind("spawncollision", false, |vm, world, s, a| {
        let Some(spawn) = vm.native_of("spawn", false) else { return Ok(Value::Undefined) };
        let cls = vm.string("script_model");
        let e = spawn(vm, world, s, &[cls, a.get(2).cloned().unwrap_or(Value::Undefined)])?;
        if let (Value::Object(o), Some(angles)) = (&e, a.get(3)) {
            let f = vm.intern("angles");
            vm.set_field(world, *o, f, angles.clone());
        }
        Ok(e)
    });
    vm.bind("setsaveddvar", false, |vm, _, _, a| {
        let name = a.first().map(|v| vm.to_text(v).to_ascii_lowercase()).unwrap_or_else(String::new);
        let v = a.get(1).map(|v| vm.to_text(v)).unwrap_or_else(String::new);
        vm.dvars.insert(name, v);
        Ok(Value::Undefined)
    });
    // getnodearray / getnode(value, key): Black Ops III's path nodes are
    // the map's `node_*` entities (the train's `train_pathnode`s).
    vm.bind("getnodearray", false, |vm, world, _, a| Ok(super::list(nodes_matching(vm, world, a))));
    vm.bind("getnode", false, |vm, world, _, a| Ok(nodes_matching(vm, world, a).into_iter().next().unwrap_or(Value::Undefined)));
    // Every vehicle path node (`info_vehicle_node*` map entities).
    vm.bind("getallvehiclenodes", false, |vm, world, _, _| {
        let zm = world.resource::<super::Zm>();
        let v = zm.ents.values().filter(|e| e.classname.starts_with("info_vehicle_node")).filter_map(|e| e.obj).filter(|o| vm.alive(*o)).map(Value::Object).collect();
        Ok(super::list(v))
    });
    vm.bind("sessionmodeismultiplayergame", false, |_, _, _, _| Ok(Value::Int(0)));
    // (`_zm`'s wait for the players holds while this is true: a loading
    // screen or level transition; neither happens here.)
    vm.bind("#d555ecb8", false, |_, _, _, _| Ok(Value::Int(0)));
    // (A node check in the AI's cover code: every node is usable.)
    vm.bind("#fd096044", false, |_, _, _, _| Ok(Value::Int(1)));
    for name in ["matchrecordnewplayer", "setmapcenter", "hidemiscmodels", "showmiscmodels", "setteamscore", "setclearanceceiling"] {
        vm.bind(name, false, |_, _, _, _| Ok(Value::Undefined));
    }
    for name in ["setrank", "setgrapplabletype"] {
        vm.bind(name, true, |_, _, _, _| Ok(Value::Undefined));
    }
    vm.bind("istestclient", true, |_, _, _, _| Ok(Value::Int(0)));
    // Telemetry with nothing to record here: `registerxp` (the XP event
    // table), a player's map event (`#bfe21698(event, time, origin,
    // round)`: a pod harvested, a ritual done).
    vm.bind("registerxp", false, |_, _, _, _| Ok(Value::Undefined));
    vm.bind("#bfe21698", true, |_, _, _, _| Ok(Value::Undefined));
    // isspawner(ent): a map entity with the spawner flag.
    vm.bind("isspawner", false, |vm, _, _, a| {
        let v = a.first().cloned().unwrap_or(Value::Undefined);
        Ok(Value::Int(i32::from(is_spawner(vm, &v))))
    });
    // animhasnotetrack(anim, note): the clips' notetracks are not read
    // here, so none.
    vm.bind("animhasnotetrack", false, |vm, world, _, a| {
        let anim = a.first().map(|v| vm.to_text(v)).unwrap_or_default();
        let note = a.get(1).map(|v| vm.to_text(v)).unwrap_or_default().to_ascii_lowercase();
        let has = world.resource::<super::Zm>().anims.get(&anim).is_some_and(|x| x.notifies.iter().any(|(n, _)| n.eq_ignore_ascii_case(&note)));
        Ok(Value::Int(i32::from(has)))
    });
    // A scene's "fake AI" object (a model that plays AI animations, the
    // keepers in the Shadows of Evil quest): it animates as any model here.
    vm.bind("makefakeai", true, |_, _, _, _| Ok(Value::Undefined));
    // No water to be under on these maps' script side.
    vm.bind("isplayerunderwater", true, |_, _, _, _| Ok(Value::Int(0)));
}

/// The map's path node entities whose `key` (targetname by default) is
/// `value`.
fn nodes_matching(vm: &mut Vm<World>, world: &mut World, a: &[Value]) -> Vec<Value> {
    let want = a.first().map(|v| vm.to_text(v)).unwrap_or_else(String::new);
    let key = match a.get(1) {
        Some(v) if !v.is_undefined() => vm.to_text(v).to_ascii_lowercase(),
        _ => "targetname".to_owned(),
    };
    let k = vm.intern(&key);
    let objs: Vec<gsc_t6::ObjRef> = world.get_resource::<T7State>().map_or_else(Vec::new, |s| s.nodes.clone());
    objs.into_iter().filter(|o| vm.alive(*o) && vm.to_text(&vm.raw_field(*o, k)) == want).map(Value::Object).collect()
}

/// Black Ops III's Zombies game type settings where they differ from
/// Black Ops II's (`gamesettings_zclassic`).
fn setting(name: &str) -> Option<i32> {
    Some(match name {
        "disablecac" | "disableclassselection" => 1,
        "teamcount" => 1,
        // (Its `mayspawn` holds a player with no lives left at the
        // spectators: the laststand, not a life count, ends his game.)
        "playernumlives" => 1,
        _ => return None,
    })
}

fn ents_where(vm: &Vm<World>, world: &World, pred: impl Fn(&super::Ent) -> bool) -> Vec<Value> {
    let zm = world.resource::<super::Zm>();
    zm.ents.values().filter(|e| pred(e)).filter_map(|e| e.obj).filter(|o| vm.alive(*o)).map(Value::Object).collect()
}

/// A map entity with the `SPAWNER` key (Radiant's spawner flag).
fn is_spawner(vm: &mut Vm<World>, v: &Value) -> bool {
    let Value::Object(o) = v else { return false };
    let f = vm.intern("spawner");
    gsc_t6::truthy(&vm.raw_field(*o, f)) || vm.to_text(&vm.raw_field(*o, f)) == "1"
}

fn spawner_team_array(vm: &mut Vm<World>, world: &mut World, _: &Value, _: &[Value]) -> Result<Value, String> {
    Ok(super::list(ents_where(vm, world, |e| e.classname.starts_with("actor_spawner"))))
}
