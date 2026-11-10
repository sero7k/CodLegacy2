//! Tests of the Ghosts script host: the pure helpers, and (when the Ghosts zones are installed:
//! `BO2MP_IW6_ZONES`, else the owner's upload folder) mp_frag's real scripts booted over the host
//! with a stand-in for the sim's movers.

use std::collections::BTreeMap;
use std::path::PathBuf;

use gsc_iw6_vm::{Name, Value, Vm};

use super::boot::{self, BootData};
use super::events;
use super::host::{self, typed, Effect, HostEnt, Iw6Host};

#[test]
fn map_keys_are_typed_as_the_scripts_read_them() {
    assert_eq!(format!("{:?}", typed(430, "1 2 3")), format!("{:?}", Value::Vector([1.0, 2.0, 3.0])));
    assert_eq!(format!("{:?}", typed(542, "3")), format!("{:?}", Value::Int(3)));
    assert_eq!(format!("{:?}", typed(546, "2.5")), format!("{:?}", Value::Float(2.5)));
    // Names stay text even when they look like numbers.
    assert_eq!(format!("{:?}", typed(642, "12")), format!("{:?}", Value::str("12")));
    // Other keys become numbers when they read as one, and keep text otherwise.
    assert_eq!(format!("{:?}", typed(60000, "7")), format!("{:?}", Value::Int(7)));
    assert_eq!(format!("{:?}", typed(60000, "0.5")), format!("{:?}", Value::Float(0.5)));
    assert_eq!(format!("{:?}", typed(60000, "a b")), format!("{:?}", Value::str("a b")));
}

#[test]
fn blocks_become_entities_and_structs() {
    let blocks = vec![
        vec![(97u32, "script_model".to_owned()), (430, "1 2 3".to_owned()), (642, "x".to_owned())],
        vec![(97, "script_struct".to_owned()), (430, "9 9 9".to_owned())],
        vec![(97, "node_pathnode".to_owned())],
    ];
    let mut h = Iw6Host::default();
    let structs = h.load_blocks(&blocks);
    assert_eq!(structs.len(), 1);
    assert_eq!(h.ents.len(), 1);
    assert_eq!(h.ents[0].block, Some(0));
    assert_eq!(h.origin(0), [1.0, 2.0, 3.0]);
    assert_eq!(h.find("targetname", "X"), vec![0]);
    assert!(h.find("targetname", "y").is_empty());
}


/// A map's scripts (the common zones beside it, then the map's and its patch), tables and entity
/// blocks, read from the install.
struct Loaded {
    scripts: Vec<(String, Vec<u8>, Vec<u8>)>,
    tables: Vec<(String, usize, usize, Vec<String>)>,
    blocks: Vec<Vec<(u32, String)>>,
    bank: super::anim::AnimBank,
}

impl Loaded {
    fn data<'a>(&'a self, map: &'a str) -> BootData<'a> {
        BootData { map, scripts: &self.scripts, tables: &self.tables, entities: &self.blocks }
    }
}

fn load_map(dir: &std::path::Path, map: &str) -> Loaded {
    let mut scripts: Vec<(String, Vec<u8>, Vec<u8>)> = Vec::new();
    let patch = format!("patch_{map}");
    for z in ["common", "common_mp", "common_core_mp", "common_bots_mp", "patch_common", "patch_common_mp", "patch_common_core_mp", map, patch.as_str()] {
        let p = dir.join(format!("{z}.ff"));
        if !p.is_file() {
            continue;
        }
        let zone = fastfile_iw6::scripts::read_zone(&p).unwrap();
        for s in zone.scripts {
            scripts.retain(|(n, _, _): &(String, Vec<u8>, Vec<u8>)| *n != s.name);
            scripts.push((s.name, s.stream, s.code));
        }
    }
    let zone = fastfile_iw6::Zone::open(&dir.join(format!("{map}.ff"))).unwrap();
    let (cap, _) = fastfile_iw6::walk::walk_zone(&zone);
    let m = fastfile_iw6::map::read_map(&cap, &zone.image, &zone.assets.script_strings);
    let blocks = fastfile_iw6::scripts::raw_blocks(&m.entities);
    let tables = cap.string_tables.iter().map(|t| (t.name.clone(), t.columns, t.rows, t.cells.clone())).collect();
    let strings = &zone.assets.script_strings;
    let (clips, bad) = fastfile_iw6::anim::clips(&cap, &zone.image, strings);
    assert!(bad.is_empty(), "animations refused: {bad:?}");
    let skeletons: Vec<_> = cap.xmodels.iter().filter(|m| m.num_bones > 1 && !m.name.starts_with(',')).filter_map(|m| fastfile_iw6::anim::skeleton(m, strings)).collect();
    Loaded { scripts, tables, blocks, bank: super::anim::AnimBank::new(&clips, &skeletons) }
}

/// Every staged Ghosts map zone beside the install's `common` zones (`mp_*.ff`, not the load
/// screens, patches or technique sets).
fn staged_maps(dir: &std::path::Path) -> Vec<String> {
    let mut maps: Vec<String> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| e.file_name().to_str().map(str::to_owned))
        .filter_map(|n| n.strip_suffix(".ff").map(str::to_owned))
        .filter(|n| n.starts_with("mp_") && !n.ends_with("_load") && !n.ends_with("_path"))
        // (A map zone is large; a stray small `mp_*` zone is not one.)
        .filter(|n| std::fs::metadata(dir.join(format!("{n}.ff"))).is_ok_and(|m| m.len() > 20 << 20))
        .collect();
    maps.sort();
    maps
}

#[test]
fn every_staged_map_boots_and_runs_without_aborting() {
    let Some(dir) = zone_dir() else {
        eprintln!("no Ghosts zones installed: skipped");
        return;
    };
    let maps = staged_maps(&dir);
    assert!(!maps.is_empty());
    for map in maps {
        let loaded = load_map(&dir, &map);
        let data = loaded.data(&map);
        let (mut vm, rep) = boot::boot(&data).unwrap();
        assert!(rep.main_found, "{map}: {rep:?}");
        let mut sb = Sandbox::default();
        sb.apply(&mut vm);
        let mut errors: Vec<String> = Vec::new();
        sb.run(&mut vm, 200, &mut errors);
        eprintln!("{map}: {} scripts, {} entities, {} errors, turned away {:?}", rep.scripts, rep.entities, errors.len(), vm.unimplemented_builtins());
        assert!(errors.is_empty(), "{map}: script errors {errors:?}");
    }
}

#[test]
fn an_unknown_builtin_does_not_abort_a_map() {
    // The VM counts it, pushes undefined and goes on (host and VM contract; the glue logs once).
    let mut host = Iw6Host::default();
    let mut ctx = gsc_iw6_vm::CallCtx::detached(&Value::Undefined, &Value::Undefined, 0);
    use gsc_iw6_vm::Host;
    let r = host.call_function(0x7fff, &mut ctx, &[]);
    assert!(matches!(r, Ok(gsc_iw6_vm::HostReturn::Unimplemented)));
}

#[test]
fn script_entities_take_damage_by_the_engine_rules() {
    use gsc_iw6_vm::ScriptSet;
    let mut h = Iw6Host::default();
    let mut e = HostEnt { spawned: true, takedamage: true, ..Default::default() };
    e.fields.insert(Name::Id(host::F_CLASSNAME), Value::str("script_model"));
    e.fields.insert(Name::Id(host::F_HEALTH), Value::Int(100));
    let id = h.add_ent(e);
    let mut plain = HostEnt::default();
    plain.fields.insert(Name::Id(host::F_CLASSNAME), Value::str("script_model"));
    let other = h.add_ent(plain);
    let mut vm = Vm::new(ScriptSet::new(), h);
    let hit = |ent, amount| events::DamageEvent { ent, amount, attacker: None, dir: [0.0; 3], point: [0.0; 3], means: "MOD_RIFLE_BULLET".into(), weapon: "w".into() };
    // An entity no script made damageable hears nothing.
    assert_eq!(events::deliver_damage(&mut vm, &hit(other, 50)), events::Outcome::Ignored);
    assert_eq!(events::deliver_damage(&mut vm, &hit(id, 60)), events::Outcome::Hurt);
    assert_eq!(vm.host().field(id, host::F_HEALTH).and_then(|v| v.as_int()), Some(40));
    // Zero damage counts as one; the second hit kills.
    assert_eq!(events::deliver_damage(&mut vm, &hit(id, 0)), events::Outcome::Hurt);
    assert_eq!(vm.host().field(id, host::F_HEALTH).and_then(|v| v.as_int()), Some(39));
    assert_eq!(events::deliver_damage(&mut vm, &hit(id, 60)), events::Outcome::Died);
}

#[test]
fn a_blast_falls_off_with_the_distance_to_the_entity_box() {
    let mut h = Iw6Host::default();
    let mk = |h: &mut Iw6Host, at: [f32; 3], no_radius: bool| {
        let mut e = HostEnt { takedamage: true, no_radius_damage: no_radius, ..Default::default() };
        e.fields.insert(Name::Id(host::F_CLASSNAME), Value::str("script_model"));
        e.fields.insert(Name::Id(host::F_ORIGIN), Value::Vector(at));
        h.add_ent(e)
    };
    let near = mk(&mut h, [0.0, 0.0, 0.0], false);
    let mid = mk(&mut h, [128.0, 0.0, 0.0], false);
    let far = mk(&mut h, [400.0, 0.0, 0.0], false);
    let shielded = mk(&mut h, [10.0, 0.0, 0.0], true);
    let inflictor = mk(&mut h, [0.0, 0.0, 0.0], false);
    let t = events::radius_targets(&h, [0.0, 0.0, 0.0], 256.0, 100.0, 20.0, inflictor);
    let amount = |id| t.iter().find(|x| x.0 == id).map(|x| x.1);
    assert_eq!(amount(near), Some(100));
    assert_eq!(amount(mid), Some(60));
    assert_eq!(amount(far), None);
    assert_eq!(amount(shielded), None);
    assert_eq!(amount(inflictor), None);
}

// ---- the real mp_frag ---------------------------------------------------------------------

fn zone_dir() -> Option<PathBuf> {
    let p = std::env::var_os("BO2MP_IW6_ZONES").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("/mnt/user-data/uploads/ghosts_game_files/zone"));
    p.join("mp_frag.ff").is_file().then_some(p)
}

type Track = ([f32; 3], [f32; 3], i64, i64);

/// A stand-in for the sim: moves entities along their tracks and says `movedone`.
#[derive(Default)]
struct Sandbox {
    now: i64,
    pos: BTreeMap<u32, Track>,
    rot: BTreeMap<u32, Track>,
    seen: Vec<(i64, Effect)>,
    /// Notifies the animator sent: (time, entity, name).
    notified: Vec<(i64, u32, String)>,
}

fn lerp(a: [f32; 3], b: [f32; 3], f: f32) -> [f32; 3] {
    std::array::from_fn(|k| a[k] + (b[k] - a[k]) * f)
}

impl Sandbox {
    fn apply(&mut self, vm: &mut Vm<Iw6Host>) {
        for e in vm.host_mut().take_effects() {
            match &e {
                Effect::MoveTo { ent, to, secs, .. } => {
                    let from = vm.host().origin(*ent);
                    self.pos.insert(*ent, (from, *to, self.now, (*secs * 1000.0).round() as i64));
                }
                Effect::RotateTo { ent, to, secs, .. } => {
                    let from = vm.host().angles(*ent);
                    self.rot.insert(*ent, (from, *to, self.now, (*secs * 1000.0).round() as i64));
                }
                Effect::Delete { ent } => vm.entity_removed(gsc_iw6_vm::EntityId(*ent)),
                _ => {}
            }
            self.seen.push((self.now, e));
        }
    }

    fn tick(&mut self, vm: &mut Vm<Iw6Host>) -> Vec<String> {
        self.now += 50;
        vm.host_mut().now_ms = self.now;
        let mut done = Vec::new();
        for (e, (a, b, s, d)) in self.pos.clone() {
            let f = ((self.now - s) as f32 / d.max(1) as f32).clamp(0.0, 1.0);
            let r = vm.host().angles(e);
            vm.host_mut().set_pose(e, lerp(a, b, f), r);
            if f >= 1.0 {
                self.pos.remove(&e);
                done.push((e, "movedone"));
            }
        }
        for (e, (a, b, s, d)) in self.rot.clone() {
            let f = ((self.now - s) as f32 / d.max(1) as f32).clamp(0.0, 1.0);
            let o = vm.host().origin(e);
            vm.host_mut().set_pose(e, o, lerp(a, b, f));
            if f >= 1.0 {
                self.rot.remove(&e);
                done.push((e, "rotatedone"));
            }
        }
        for (e, n) in done {
            vm.notify(&Iw6Host::value(e), n, vec![]);
        }
        for (e, n) in vm.host_mut().anim.advance(self.now) {
            self.notified.push((self.now, e, n.clone()));
            vm.notify(&Iw6Host::value(e), &n, vec![]);
        }
        let now = self.now;
        let due: Vec<_> = {
            let h = vm.host_mut();
            let (d, keep): (Vec<_>, Vec<_>) = std::mem::take(&mut h.timers).into_iter().partition(|t| t.0 <= now);
            h.timers = keep;
            d
        };
        for (_, t, n) in due {
            vm.notify(&t, &n, vec![]);
        }
        let errs = vm.step(50).errors.iter().map(|e| e.to_string()).collect();
        self.apply(vm);
        errs
    }

    fn run(&mut self, vm: &mut Vm<Iw6Host>, ticks: usize, errors: &mut Vec<String>) -> usize {
        let from = self.seen.len();
        for _ in 0..ticks {
            errors.extend(self.tick(vm));
        }
        from
    }
}

fn has(seen: &[(i64, Effect)], f: impl Fn(&Effect) -> bool) -> bool {
    seen.iter().any(|(_, e)| f(e))
}

#[test]
fn mp_frag_events_run_from_its_scripts() {
    let Some(dir) = zone_dir() else {
        eprintln!("mp_frag.ff not installed: skipped");
        return;
    };
    let loaded = load_map(&dir, "mp_frag");
    let data = loaded.data("mp_frag");
    let (mut vm, rep) = boot::boot(&data).unwrap();
    assert!(rep.main_found, "{rep:?}");
    vm.host_mut().anim.bank = loaded.bank.clone();
    let mut sb = Sandbox::default();
    sb.apply(&mut vm);
    let mut errors: Vec<String> = Vec::new();
    sb.run(&mut vm, 100, &mut errors);

    let mut pl = HostEnt { client: Some(0), solid: true, ..Default::default() };
    pl.fields.insert(Name::Id(97), Value::str("player"));
    pl.fields.insert(Name::Id(430), Value::Vector([0.0; 3]));
    pl.fields.insert(Name::Id(47), Value::Vector([0.0; 3]));
    let player = vm.host_mut().add_ent(pl);

    // A door switch: the pieces move over the script's own time, with its sounds.
    let switch = vm.host().find("targetname", "door_switch")[0];
    vm.notify(&Iw6Host::value(switch), "trigger", vec![Iw6Host::value(player)]);
    let from = sb.run(&mut vm, 200, &mut errors);
    let seen = &sb.seen[from..];
    assert!(has(seen, |e| matches!(e, Effect::Sound { alias, .. } if alias == "scn_tele_door_button_push")));
    assert!(has(seen, |e| matches!(e, Effect::MoveTo { secs, accel, decel, .. } if (*secs - 3.0).abs() < 1e-3 && (*accel - 0.75).abs() < 1e-3 && (*decel - 0.75).abs() < 1e-3)));
    assert!(has(seen, |e| matches!(e, Effect::Usable { ent, on: false } if *ent == switch)));
    // The doors the map shuts to walkers at the start (`disconnectpaths` on brush models), and the
    // pair that is given back when the switch's doors have run (`connectpaths`).
    let shut = sb.seen.iter().filter(|(_, e)| matches!(e, Effect::Paths { connect: false, .. })).count();
    let given = sb.seen.iter().filter(|(_, e)| matches!(e, Effect::Paths { connect: true, .. })).count();
    assert!(shut >= 8 && given >= 2, "paths: {shut} shut, {given} given back");

    // Damage: the gate and the warehouse door open; the ladder drops; a toy smokes.
    let damage = |vm: &mut Vm<Iw6Host>, ent: u32| {
        let at = vm.host().origin(ent);
        vm.notify(
            &Iw6Host::value(ent),
            "damage",
            vec![
                Value::Int(100),
                Iw6Host::value(player),
                Value::Vector([1.0, 0.0, 0.0]),
                Value::Vector(at),
                Value::str("MOD_GRENADE_SPLASH"),
                Value::str(""),
                Value::str(""),
                Value::str(""),
                Value::Int(0),
                Value::str("frag_grenade_mp"),
            ],
        );
    };
    let gate = vm.host().find("targetname", "gate_trigger")[0];
    damage(&mut vm, gate);
    let from = sb.run(&mut vm, 60, &mut errors);
    assert!(has(&sb.seen[from..], |e| matches!(e, Effect::Sound { alias, .. } if alias == "frag_gate_iron_open")), "gate");
    assert!(has(&sb.seen[from..], |e| matches!(e, Effect::Delete { ent } if *ent == gate)));

    let wh = vm.host().find("targetname", "warehouse_door_trigger")[0];
    damage(&mut vm, wh);
    let from = sb.run(&mut vm, 60, &mut errors);
    assert!(has(&sb.seen[from..], |e| matches!(e, Effect::Sound { alias, .. } if alias == "scn_breach_swingindoor_left")), "warehouse");

    let ladder = vm.host().find("targetname", "ladder_damage_trigger")[0];
    let model = vm.host().find("targetname", "scripted_ladder")[0];
    damage(&mut vm, ladder);
    let from = sb.run(&mut vm, 60, &mut errors);
    assert!(has(&sb.seen[from..], |e| matches!(e, Effect::Delete { ent } if *ent == ladder)), "ladder");
    let _ = model;

    // A toy transformer breaks: through the engine's damage rules (health goes down, `damage`
    // then `death`), the script's own path to its end.
    let toy = vm.host().find("targetname", "destructible_toy")[0];
    let before_model = vm.host().model_of(toy);
    let health = vm.host().field(toy, host::F_HEALTH).and_then(|v| v.as_int());
    assert!(vm.host().ent(toy).unwrap().takedamage, "the toy's script made it damageable");
    let _ = (before_model, health);
    let from = sb.seen.len();
    for _ in 0..8 {
        let at = vm.host().origin(toy);
        let out = events::deliver_damage(
            &mut vm,
            &events::DamageEvent {
                ent: toy,
                amount: 100,
                attacker: Some(player),
                dir: [1.0, 0.0, 0.0],
                point: at,
                means: "MOD_GRENADE_SPLASH".into(),
                weapon: "frag_grenade_mp".into(),
            },
        );
        sb.run(&mut vm, 4, &mut errors);
        if out == events::Outcome::Died {
            break;
        }
    }
    sb.run(&mut vm, 120, &mut errors);
    let toy_seen = &sb.seen[from..];
    assert!(has(toy_seen, |e| matches!(e, Effect::FxOnTag { path, ent, .. } if *ent == toy && path.contains("smoke"))), "toy smoke");
    assert!(has(toy_seen, |e| matches!(e, Effect::SetModel { ent, model } if *ent == toy && model.ends_with("_dest"))), "toy model swap");
    assert!(has(toy_seen, |e| matches!(e, Effect::Fx { path, .. } if path == "fx/explosions/transformer_explosion")), "toy explosion");
    assert!(has(toy_seen, |e| matches!(e, Effect::Sound { alias, .. } if alias == "transformer01_explode")), "toy sound");
    assert!(has(toy_seen, |e| matches!(e, Effect::RadiusDamage { inflictor, .. } if *inflictor == toy)), "toy blast");
    assert!(has(toy_seen, |e| matches!(e, Effect::CanDamage { ent, on: false } if *ent == toy)), "toy stops taking damage");
    // The spawned helpers (script_origins for the loop sounds) are deleted when it is over.
    let spawned: Vec<u32> = toy_seen.iter().filter_map(|(_, e)| if let Effect::Spawn { ent, .. } = e { Some(*ent) } else { None }).collect();
    assert!(!spawned.is_empty());
    assert!(has(toy_seen, |e| matches!(e, Effect::Delete { ent } if spawned.contains(ent))), "spawned entities deleted");
    assert_eq!(vm.host().model_of(toy), "utility_transformer_small01_dest");

    // The underground trigger sets the sprinklers off: exploder 1.
    let unders = vm.host().find("targetname", "underground_damage_trigger");
    for u in &unders[..1] {
        vm.notify(&Iw6Host::value(*u), "trigger", vec![Iw6Host::value(player)]);
    }
    let from = sb.run(&mut vm, 80, &mut errors);
    assert!(has(&sb.seen[from..], |e| matches!(e, Effect::Exploder { id: 1, .. })), "exploder");

    assert!(errors.is_empty(), "script errors: {errors:?}");
    assert!(vm.unimplemented_builtins().is_empty(), "builtins turned away: {:?}", vm.unimplemented_builtins());
}

/// Calls a builtin by name on a host (function when `this` is None, else a method on it).
fn call(h: &mut Iw6Host, name: &str, this: Option<u32>, args: &[Value]) -> Value {
    use gsc_iw6_vm::{Host, HostReturn};
    let (kind, id) = gsc_iw6::builtins::lookup(name).unwrap_or_else(|| panic!("no builtin {name}"));
    let me = this.map_or(Value::Undefined, Iw6Host::value);
    let mut ctx = gsc_iw6_vm::CallCtx::detached(&me, &Value::Undefined, 0);
    let r = match (kind, this) {
        (gsc_iw6::builtins::Kind::Method, Some(_)) => h.call_method(id, &mut ctx, &me, args),
        _ => h.call_function(id, &mut ctx, args),
    };
    match r.unwrap_or_else(|e| panic!("{name}: {e}")) {
        HostReturn::Value(v) => v,
        HostReturn::Unimplemented => panic!("{name} is not implemented"),
        _ => Value::Undefined,
    }
}

#[test]
fn a_spawned_model_moves_hides_takes_damage_and_goes() {
    let mut h = Iw6Host::default();
    let m = call(&mut h, "spawn", None, &[Value::str("script_model"), Value::Vector([10.0, 20.0, 30.0])]);
    let id = m.as_entity().expect("an entity").0;
    assert!(h.ent(id).unwrap().spawned);
    assert!(matches!(h.out.last(), Some(Effect::Spawn { ent, class, .. }) if *ent == id && class == "script_model"));
    call(&mut h, "setmodel", Some(id), &[Value::str("com_barrel")]);
    assert_eq!(h.model_of(id), "com_barrel");
    call(&mut h, "moveto", Some(id), &[Value::Vector([10.0, 20.0, 130.0]), Value::Float(2.0)]);
    assert!(h.out.iter().any(|e| matches!(e, Effect::MoveTo { ent, secs, .. } if *ent == id && (*secs - 2.0).abs() < 1e-3)));
    call(&mut h, "hide", Some(id), &[]);
    assert!(h.ent(id).unwrap().hidden);
    call(&mut h, "setcandamage", Some(id), &[Value::Int(1)]);
    assert!(events::takes_damage(&h, id));
    let t = call(&mut h, "spawn", None, &[Value::str("trigger_damage"), Value::Vector([0.0; 3]), Value::Int(0), Value::Float(64.0), Value::Float(32.0)]);
    let tid = t.as_entity().unwrap().0;
    assert!(matches!(h.out.last(), Some(Effect::Spawn { radius, height, .. }) if (*radius - 64.0).abs() < 1e-3 && (*height - 32.0).abs() < 1e-3));
    assert!(events::takes_damage(&h, tid), "a spawned damage trigger hears every hit");
    call(&mut h, "delete", Some(id), &[]);
    assert!(h.ent(id).is_none_or(|e| e.gone));
}

#[test]
fn the_event_methods_have_their_names() {
    use gsc_iw6::builtins::{name, Kind};
    for (n, id) in [
        ("attach", 0x8018u16),
        ("attach", 0x8019),
        ("detach", 0x8026),
        ("detach", 0x8027),
        ("moveshieldmodel", 0x8028),
        ("radiusdamage", 0x81d0),
        ("setcandamage", 0x82b5),
        ("setcanradiusdamage", 0x82b6),
        ("getcurrentweapon", 0x8305),
        ("setscriptmoverkillcam", 0x833d),
        ("getlinkedchildren", 0x8402),
    ] {
        assert_eq!(name(Kind::Method, id), Some(n), "{id:#x}");
    }
}

fn dist(a: [f32; 3], b: [f32; 3]) -> f32 {
    (0..3).map(|k| (a[k] - b[k]).powi(2)).sum::<f32>().sqrt()
}

#[test]
fn mp_frag_script_animations_play_to_their_end() {
    let Some(dir) = zone_dir() else {
        eprintln!("mp_frag.ff not installed: skipped");
        return;
    };
    let loaded = load_map(&dir, "mp_frag");
    let data = loaded.data("mp_frag");
    let (mut vm, rep) = boot::boot(&data).unwrap();
    assert!(rep.main_found, "{rep:?}");
    vm.host_mut().anim.bank = loaded.bank.clone();
    let mut sb = Sandbox::default();
    sb.apply(&mut vm);
    let mut errors: Vec<String> = Vec::new();
    sb.run(&mut vm, 100, &mut errors);
    let mut pl = HostEnt { client: Some(0), solid: true, ..Default::default() };
    pl.fields.insert(Name::Id(97), Value::str("player"));
    pl.fields.insert(Name::Id(430), Value::Vector([0.0; 3]));
    pl.fields.insert(Name::Id(47), Value::Vector([0.0; 3]));
    let player = vm.host_mut().add_ent(pl);

    // At the start: the crane sways, the two locks idle, the gate's and the warehouse door's
    // carriers hold their closed pose (the starts `useanimtree` makes).
    let h = vm.host();
    let playing = |clip: &str| -> Vec<u32> { h.anim.playing.iter().filter(|(_, p)| p.clip == clip).map(|(e, _)| *e).collect() };
    let crane = playing("mp_frag_crane_sway");
    assert_eq!(crane.len(), 1, "the crane");
    assert_eq!(h.model_of(crane[0]), "mp_frag_crane_anim");
    assert_eq!(playing("mp_frag_metal_door_chain").len(), 1, "the gate's lock");
    assert_eq!(playing("mp_frag_large_door_chain_idle").len(), 1, "the warehouse door's lock");
    let closed = playing("mp_frag_metal_door_closed_loop");
    assert_eq!(closed.len(), 1, "the gate's carrier");
    let gate_carrier = closed[0];
    assert_eq!(h.model_of(gate_carrier), "generic_prop_raven");

    // The two halves of the gate are linked to the carrier's two tags, with no offset: they stand
    // where the tags do, and follow them as the clip moves them.
    let links: Vec<(u32, String, [f32; 3])> = sb
        .seen
        .iter()
        .filter_map(|(_, e)| match e {
            Effect::Link { ent, parent, tag, offset, .. } if *parent == gate_carrier => Some((*ent, tag.clone(), *offset)),
            _ => None,
        })
        .collect();
    let mut tags: Vec<String> = links.iter().map(|l| l.1.clone()).collect();
    tags.sort();
    assert_eq!(tags, vec!["j_prop_1", "j_prop_2"], "{links:?}");
    let models: Vec<String> = links.iter().map(|l| vm.host().model_of(l.0)).collect();
    assert!(models.iter().all(|m| m.contains("iron_gate")), "{models:?}");
    let carrier_at = vm.host().origin(gate_carrier);
    for (child, tag, offset) in &links {
        let (o, _) = vm.host().tag_world(gate_carrier, tag);
        eprintln!("gate half {} on {tag}: linked at offset {offset:?}; tag stands at {o:?} (carrier {carrier_at:?}), the half's own origin {:?}", vm.host().model_of(*child), vm.host().origin(*child));
        assert!(dist(o, carrier_at) < 1.0, "the closed pose has the tag at the carrier");
    }
    // Linked with no offsets, each half keeps where it stands relative to its tag (READ from the
    // handler), so at the closed pose it has not moved, and at the open pose it stands where the
    // tag's animation carries it.
    let (op, ang) = (carrier_at, vm.host().angles(gate_carrier));
    let mut open_at = Vec::new();
    for (child, tag, _) in &links {
        let own = (vm.host().origin(*child), vm.host().angles(*child));
        let local = vm.host().anim.link_local(gate_carrier, "generic_prop_raven", op, ang, tag, 0, own).expect("the tag");
        let closed = vm.host().anim.link_world(gate_carrier, "generic_prop_raven", op, ang, tag, 0, local).unwrap();
        assert!(dist(closed.0, own.0) < 0.01, "the half stays where it is while the gate is closed: {closed:?} vs {own:?}");
        open_at.push((*child, tag.clone(), own, local));
    }
    // The crane really moves: its container swings between the first frame and the middle of the
    // loop, and is back where it started after one loop.
    let started = h.anim.playing[&crane[0]].start_ms;
    let at = |ms: i64| h.anim.pose(crane[0], "mp_frag_crane_anim", started + ms).unwrap();
    let len = h.anim.bank.clips["mp_frag_crane_sway"].length_ms();
    let (p0, p1, p2) = (at(0), at(len / 2), at(len));
    let moved = (1..p0.len()).map(|b| dist(p0[b].1, p1[b].1) + (1.0 - p0[b].0.iter().zip(&p1[b].0).map(|(a, b)| a * b).sum::<f32>().abs()) * 100.0).fold(0.0f32, f32::max);
    assert!(moved > 0.05, "the crane does not sway: {moved}");
    for b in 0..p0.len() {
        assert!(dist(p0[b].1, p2[b].1) < 0.05, "bone {b} is not back after a loop");
    }
    eprintln!("crane sway: {len} ms loop, largest bone move {moved:.2}");

    // The gate's chain is broken: the carrier plays the opening clip once, to its `end`; its two
    // tags (what the gate's halves are linked to) differ between the first and the last frame by
    // what the animation says.
    let gate = vm.host().find("targetname", "gate_trigger")[0];
    let at_damage = vm.host().anim.pose(gate_carrier, "generic_prop_raven", 0).unwrap();
    let hit = |vm: &mut Vm<Iw6Host>, ent: u32| {
        let p = vm.host().origin(ent);
        vm.notify(
            &Iw6Host::value(ent),
            "damage",
            vec![Value::Int(100), Iw6Host::value(player), Value::Vector([1.0, 0.0, 0.0]), Value::Vector(p), Value::str("MOD_GRENADE_SPLASH"), Value::str(""), Value::str(""), Value::str(""), Value::Int(0), Value::str("frag_grenade_mp")],
        );
    };
    hit(&mut vm, gate);
    let from = sb.run(&mut vm, 20, &mut errors);
    let seen = &sb.seen[from..];
    let anim = seen.iter().find_map(|(t, e)| match e {
        Effect::Anim { ent, name, via_tree: false } if *ent == gate_carrier => Some((*t, name.clone())),
        _ => None,
    });
    let (t0, clip_name) = anim.expect("the gate's carrier plays an animation when the chain is hit");
    assert_eq!(clip_name, "mp_frag_metal_door_open_out");
    let clip = vm.host().anim.bank.clips[&clip_name].clone();
    assert!(!clip.looping);
    let before = sb.notified.len();
    sb.run(&mut vm, (clip.length_ms() / 50) as usize + 6, &mut errors);
    let ended: Vec<_> = sb.notified[before..].iter().filter(|(_, e, n)| *e == gate_carrier && n == "end").collect();
    assert_eq!(ended.len(), 1, "one `end` for the carrier: {:?}", &sb.notified[before..]);
    let t_end = ended[0].0;
    assert!((t_end - t0 - clip.length_ms()).abs() <= 100, "`end` came at {} ms after the start, the clip is {} ms", t_end - t0, clip.length_ms());
    assert!(!vm.host().anim.moving(gate_carrier, sb.now), "it holds the last frame");
    let first = vm.host().anim.pose(gate_carrier, "generic_prop_raven", t0).unwrap();
    let last = vm.host().anim.pose(gate_carrier, "generic_prop_raven", sb.now).unwrap();
    let moved_tag = |b: usize| dist(first[b].1, last[b].1) + (1.0 - first[b].0.iter().zip(&last[b].0).map(|(a, b)| a * b).sum::<f32>().abs()) * 100.0;
    eprintln!("gate carrier: first frame vs last: j_prop_1 {:.2}, j_prop_2 {:.2}; before the hit: {:?}", moved_tag(1), moved_tag(2), at_damage[1]);
    assert!(moved_tag(1) > 1.0 && moved_tag(2) > 1.0, "both halves of the gate move");
    // The first frame of the opening clip is where the closed pose is (the bind pose).
    for b in 0..3 {
        assert!(dist(first[b].1, at_damage[b].1) < 0.5, "bone {b} starts the opening where the closed pose is");
    }
    // The halves, carried by the opened tags, stand elsewhere, and at the angles the tags turned to.
    for (child, tag, own, local) in &open_at {
        let end = vm.host().anim.link_world(gate_carrier, "generic_prop_raven", op, ang, tag, sb.now, *local).unwrap();
        let swing = dist(end.0, own.0);
        let turned = (0..3).map(|k| ((end.1[k] - own.1[k] + 540.0) % 360.0 - 180.0).abs()).fold(0.0f32, f32::max);
        eprintln!("gate half {} opens: moves {swing:.1} units, turns {turned:.1} degrees ({:?} -> {:?})", vm.host().model_of(*child), own.0, end.0);
        assert!(swing > 5.0 || turned > 20.0, "the half {tag} does not move");
    }
    // A clip that does not exist changes nothing.
    assert!(!vm.host_mut().anim.play(gate_carrier, "no_such_animation", sb.now));
    assert!(vm.host().anim.playing.contains_key(&gate_carrier));
    vm.host_mut().anim.clear(gate_carrier);
    assert!(!vm.host().anim.playing.contains_key(&gate_carrier));
    assert!(errors.is_empty(), "script errors: {errors:?}");
}

// ---- the rest of the census (2026-11-05) -----------------------------------------------------

/// A player entity on a bare host.
fn player_host() -> (Iw6Host, u32) {
    let mut h = Iw6Host::default();
    let mut e = HostEnt { client: Some(0), ..Default::default() };
    e.fields.insert(Name::Id(host::F_CLASSNAME), Value::str("player"));
    e.fields.insert(Name::Id(host::F_ORIGIN), Value::Vector([100.0, 100.0, 0.0]));
    let id = h.add_ent(e);
    h.players.insert(0, id);
    (h, id)
}

#[test]
fn the_player_data_banks_keep_what_a_script_stored() {
    let (mut h, p) = player_host();
    call(&mut h, "setcommonplayerdata", Some(p), &[Value::str("squad"), Value::Int(2), Value::str("name"), Value::str("alpha")]);
    assert_eq!(format!("{:?}", call(&mut h, "getcommonplayerdata", Some(p), &[Value::str("squad"), Value::Int(2), Value::str("name")])), format!("{:?}", Value::str("alpha")));
    // Another bank, another path and an unset path read as 0.
    assert_eq!(call(&mut h, "getrankedplayerdata", Some(p), &[Value::str("squad"), Value::Int(2), Value::str("name")]).as_int(), Some(0));
    assert_eq!(call(&mut h, "getcommonplayerdata", Some(p), &[Value::str("squad"), Value::Int(3), Value::str("name")]).as_int(), Some(0));
    call(&mut h, "setplayerdata", Some(p), &[Value::str("xp"), Value::Int(900)]);
    call(&mut h, "setcoopplayerdata", Some(p), &[Value::str("xp"), Value::Int(5)]);
    assert_eq!(call(&mut h, "getplayerdata", Some(p), &[Value::str("xp")]).as_int(), Some(900));
    assert_eq!(call(&mut h, "getcoopplayerdata", Some(p), &[Value::str("xp")]).as_int(), Some(5));
    call(&mut h, "setrankedplayerdatareservedint", Some(p), &[Value::str("slot"), Value::Int(7)]);
    assert_eq!(call(&mut h, "getrankedplayerdatareservedint", Some(p), &[Value::str("slot")]).as_int(), Some(7));
    // The custom-class bank is read only: it reads what is in it, else 0.
    assert_eq!(call(&mut h, "getcacplayerdata", Some(p), &[Value::str("customClasses"), Value::Int(0), Value::str("name")]).as_int(), Some(0));
}

#[test]
fn the_match_data_is_one_bank_for_the_whole_match() {
    let mut h = Iw6Host::default();
    call(&mut h, "setmatchdata", None, &[Value::str("players"), Value::Int(3), Value::str("kills"), Value::Int(12)]);
    assert_eq!(call(&mut h, "getmatchdata", None, &[Value::str("players"), Value::Int(3), Value::str("kills")]).as_int(), Some(12));
    // The client match data is a bank of its own.
    assert_eq!(call(&mut h, "getclientmatchdata", None, &[Value::str("players"), Value::Int(3), Value::str("kills")]).as_int(), Some(0));
    call(&mut h, "setclientmatchdata", None, &[Value::str("lifeCount"), Value::Int(4)]);
    assert_eq!(call(&mut h, "getclientmatchdata", None, &[Value::str("lifeCount")]).as_int(), Some(4));
    // (No effect is queued: the sim has nothing to send.)
    call(&mut h, "sendmatchdata", None, &[]);
    assert!(h.out.is_empty());
}

#[test]
fn the_point_nearest_a_segment_clamps_to_its_ends() {
    use super::builtins::nearest_on_segment as near;
    let (a, b) = ([0.0, 0.0, 0.0], [10.0, 0.0, 0.0]);
    assert_eq!(near(a, b, [4.0, 7.0, 3.0]), [4.0, 0.0, 0.0]);
    assert_eq!(near(a, b, [-5.0, 1.0, 0.0]), a);
    assert_eq!(near(a, b, [50.0, 1.0, 0.0]), b);
    assert_eq!(near(a, a, [1.0, 2.0, 3.0]), a, "a point of no length");
    let mut h = Iw6Host::default();
    let v = call(&mut h, "pointonsegmentnearesttopoint", None, &[Value::Vector(a), Value::Vector(b), Value::Vector([2.0, 2.0, 2.0])]);
    assert_eq!(v.as_vector(), Some([2.0, 0.0, 0.0]));
}

#[test]
fn team_scores_are_kept_per_team_and_other_names_are_refused() {
    let mut h = Iw6Host::default();
    assert_eq!(call(&mut h, "getteamscore", None, &[Value::str("allies")]).as_int(), Some(0));
    call(&mut h, "setteamscore", None, &[Value::str("allies"), Value::Int(30)]);
    call(&mut h, "setteamscore", None, &[Value::str("axis"), Value::Int(11)]);
    assert_eq!(call(&mut h, "getteamscore", None, &[Value::str("allies")]).as_int(), Some(30));
    assert_eq!(call(&mut h, "getteamscore", None, &[Value::str("axis")]).as_int(), Some(11));
    use gsc_iw6_vm::Host;
    let (kind, id) = gsc_iw6::builtins::lookup("getteamscore").unwrap();
    assert_eq!(kind, gsc_iw6::builtins::Kind::Function);
    let me = Value::Undefined;
    let mut ctx = gsc_iw6_vm::CallCtx::detached(&me, &me, 0);
    assert!(h.call_function(id, &mut ctx, &[Value::str("spectator")]).is_err());
}

#[test]
fn spectate_permissions_and_player_commands_are_stored_for_the_player() {
    let (mut h, p) = player_host();
    call(&mut h, "allowspectateteam", Some(p), &[Value::str("allies"), Value::Int(1)]);
    call(&mut h, "allowspectateteam", Some(p), &[Value::str("freelook"), Value::Int(0)]);
    assert_eq!(h.stores.spectate.get(&(p, "allies".to_owned())), Some(&true));
    assert_eq!(h.stores.spectate.get(&(p, "freelook".to_owned())), Some(&false));
    use gsc_iw6_vm::Host;
    let (_, id) = gsc_iw6::builtins::lookup("allowspectateteam").unwrap();
    let me = Iw6Host::value(p);
    let mut ctx = gsc_iw6_vm::CallCtx::detached(&me, &Value::Undefined, 0);
    assert!(h.call_method(id, &mut ctx, &me, &[Value::str("spectators"), Value::Int(1)]).is_err(), "an unknown target is refused");
    call(&mut h, "notifyonplayercommand", Some(p), &[Value::str("respawn_random"), Value::str("+usereload")]);
    call(&mut h, "notifyonplayercommand", Some(p), &[Value::str("respawn_random"), Value::str("+usereload")]);
    call(&mut h, "notifyonplayercommand", Some(p), &[Value::str("use_pressed"), Value::str("+usereload")]);
    assert_eq!(super::builtins::commands_for(&h, p, "+usereload"), vec!["respawn_random".to_owned(), "use_pressed".to_owned()]);
    assert!(super::builtins::commands_for(&h, p, "+attack").is_empty());
}

#[test]
fn small_player_state_calls_keep_their_values() {
    let (mut h, p) = player_host();
    assert_eq!(call(&mut h, "setcontents", Some(p), &[Value::Int(8)]).as_int(), Some(0));
    assert_eq!(call(&mut h, "setcontents", Some(p), &[Value::Int(0)]).as_int(), Some(8), "it answers the value it replaced");
    call(&mut h, "setmovespeedscale", Some(p), &[Value::Float(0.8)]);
    assert_eq!(h.stores.move_speed_scale.get(&p), Some(&0.8));
    call(&mut h, "setstance", Some(p), &[Value::str("crouch")]);
    assert_eq!(h.stores.stance.get(&p).map(String::as_str), Some("crouch"));
    call(&mut h, "setcamerapoint", Some(p), &[Value::Int(2), Value::Vector([1.0, 2.0, 3.0]), Value::Vector([0.0, 90.0, 0.0])]);
    call(&mut h, "setcamerapoint", Some(p), &[Value::Int(9), Value::Vector([1.0; 3]), Value::Vector([0.0; 3])]);
    assert_eq!(h.stores.camera.len(), 1, "only an index under 4 is kept");
    call(&mut h, "setclientdvars", Some(p), &[Value::str("cg_fov"), Value::str("80"), Value::str("cg_drawgun"), Value::Int(1)]);
    assert_eq!(h.stores.client_dvars.get(&(p, "cg_fov".to_owned())).map(String::as_str), Some("80"));
    assert_eq!(h.stores.client_dvars.get(&(p, "cg_drawgun".to_owned())).map(String::as_str), Some("1"));
    assert_eq!(call(&mut h, "getxuid", Some(p), &[]).as_str().map(str::len), Some(15));
    assert_eq!(call(&mut h, "playerads", Some(p), &[]).as_float(), Some(0.0));
}

#[test]
fn an_item_is_unlocked_unless_the_table_has_no_row_for_it() {
    let (mut h, p) = player_host();
    assert_eq!(call(&mut h, "isitemunlocked", Some(p), &[Value::str("anything")]).as_int(), Some(1), "no table loaded: all open");
    h.tables.insert("mp/unlocktable.csv".into(), host::Table { columns: 2, rows: 2, cells: vec!["iw6_ak12".into(), "0".into(), "iw6_vks".into(), "1".into()] });
    assert_eq!(call(&mut h, "isitemunlocked", Some(p), &[Value::str("IW6_AK12")]).as_int(), Some(1));
    assert_eq!(call(&mut h, "isitemunlocked", Some(p), &[Value::str("iw6_none")]).as_int(), Some(2), "READ: 2 is the answer for a name with no row");
}

#[test]
fn a_position_where_a_player_stands_would_telefrag() {
    let (mut h, _p) = player_host();
    assert_eq!(call(&mut h, "positionwouldtelefrag", None, &[Value::Vector([110.0, 100.0, 0.0])]).as_int(), Some(1));
    assert_eq!(call(&mut h, "positionwouldtelefrag", None, &[Value::Vector([200.0, 100.0, 0.0])]).as_int(), Some(0));
    assert_eq!(call(&mut h, "positionwouldtelefrag", None, &[Value::Vector([100.0, 100.0, 80.0])]).as_int(), Some(0), "a player's height is 70");
}

#[test]
fn spawn_arrays_come_from_the_map_entities_of_that_class() {
    let blocks = vec![
        vec![(97u32, "mp_tdm_spawn".to_owned()), (430, "1 2 3".to_owned())],
        vec![(97, "mp_tdm_spawn".to_owned()), (430, "4 5 6".to_owned())],
        vec![(97, "mp_dom_spawn".to_owned()), (430, "7 8 9".to_owned())],
    ];
    let mut h = Iw6Host::default();
    h.load_blocks(&blocks);
    let a = call(&mut h, "getspawnarray", None, &[Value::str("mp_tdm_spawn")]);
    assert_eq!(a.as_array().map(|x| x.values().len()), Some(2));
    let b = call(&mut h, "getspawnarray", None, &[Value::str("mp_nothing")]);
    assert_eq!(b.as_array().map(|x| x.values().len()), Some(0));
}

/// Every builtin id the mp_frag scripts call that the host once turned away (the census of
/// 2026-11-04: 47 functions, 93 methods) is taken now.
#[test]
fn the_unimplemented_ids_of_the_mp_frag_census_are_all_taken() {
    let functions: [u16; 47] = [
        0x0122, 0x0129, 0x0253, 0x0150, 0x0187, 0x0030, 0x0123, 0x0158, 0x015f, 0x0061, 0x00a5, 0x012a, 0x0159, 0x0160, 0x0198, 0x021e, 0x005a, 0x0060, 0x0062, 0x0082, 0x0083,
        0x0085, 0x00ba, 0x00bb, 0x00e0, 0x00ee, 0x00f1, 0x0124, 0x012b, 0x012c, 0x012f, 0x0132, 0x0134, 0x0135, 0x014c, 0x0151, 0x0157, 0x0169, 0x018a, 0x019e, 0x01a4, 0x023a, 0x023b,
        0x0248, 0x024f, 0x0250, 0x0251,
    ];
    let methods: [u16; 93] = [
        0x823e, 0x8269, 0x823b, 0x823f, 0x821f, 0x83fd, 0x83fe, 0x821a, 0x8291, 0x82c1, 0x82d1, 0x81aa, 0x8265, 0x82dc, 0x8306, 0x8335, 0x8082, 0x8086, 0x82de, 0x832c, 0x8337, 0x83a7,
        0x83fa, 0x8406, 0x841d, 0x8015, 0x8016, 0x801f, 0x80cf, 0x8178, 0x81dc, 0x8293, 0x8296, 0x82bd, 0x82f3, 0x831c, 0x8352, 0x83e8, 0x83f1, 0x83fb, 0x840c, 0x841c, 0x8005, 0x800a,
        0x8017, 0x8029, 0x804e, 0x8083, 0x808f, 0x80a6, 0x80a8, 0x80ab, 0x80ac, 0x80b1, 0x80b7, 0x80c2, 0x8176, 0x817f, 0x81ae, 0x81db, 0x8247, 0x8267, 0x828c, 0x8295, 0x8299, 0x82bf,
        0x82c0, 0x82c2, 0x82c3, 0x82c8, 0x82d4, 0x82d5, 0x8304, 0x8309, 0x831e, 0x8321, 0x8330, 0x8334, 0x833e, 0x833f, 0x8340, 0x836d, 0x83b7, 0x83b8, 0x83d8, 0x83d9, 0x83da, 0x83fc,
        0x8405, 0x840d, 0x841e, 0x841f, 0x8423,
    ];
    let mut turned_away = Vec::new();
    for id in functions {
        if super::census::status(false, id) == super::census::Status::Unimplemented {
            turned_away.push(format!("function {id:#06x}"));
        }
    }
    for id in methods {
        if super::census::status(true, id) == super::census::Status::Unimplemented {
            turned_away.push(format!("method {id:#06x}"));
        }
    }
    assert!(turned_away.is_empty(), "{turned_away:?}");
}

#[test]
fn the_four_player_methods_without_arms_answer() {
    let (mut h, p) = player_host();
    call(&mut h, "setplayerangles", Some(p), &[Value::Vector([0.0, 90.0, 0.0])]);
    assert_eq!(h.angles(p), [0.0, 90.0, 0.0]);
    assert!(matches!(call(&mut h, "getspectatingplayer", Some(p), &[]), Value::Undefined));
    call(&mut h, "setblurforplayer", Some(p), &[Value::Float(2.0), Value::Float(0.5)]);
    assert_eq!(call(&mut h, "getoffhandsecondaryclass", Some(p), &[]).as_str(), Some("flash"));
}

// ---- player state and HUD elements (2026-11-05, pass 4) ----------------------------------------

fn alen(v: &Value) -> Option<usize> {
    match v {
        Value::Array(a) => Some(a.values().len()),
        _ => None,
    }
}

fn obj_of(v: &Value) -> gsc_iw6_vm::ObjRef {
    match v {
        Value::Object(o) => o.clone(),
        other => panic!("not an object: {other:?}"),
    }
}

/// A method on a HUD object (the way the VM dispatches it).
fn hud_call(h: &mut Iw6Host, name: &str, hud: &Value, args: &[Value]) -> Result<Value, String> {
    use gsc_iw6_vm::{Host, HostReturn};
    let (_, id) = gsc_iw6::builtins::lookup(name).unwrap_or_else(|| panic!("no builtin {name}"));
    let mut ctx = gsc_iw6_vm::CallCtx::detached(hud, &Value::Undefined, 0);
    match h.call_method(id, &mut ctx, hud, args)? {
        HostReturn::Value(v) => Ok(v),
        HostReturn::Unimplemented => panic!("{name} is not implemented"),
        _ => Ok(Value::Undefined),
    }
}

#[test]
fn hud_elements_keep_their_kind_text_timers_and_fades() {
    let (mut h, p) = player_host();
    let hud = call(&mut h, "newhudelem", None, &[]);
    let o = obj_of(&hud);
    o.set(h.key_name("x"), Value::Float(10.0));
    o.set(h.key_name("alpha"), Value::Float(1.0));
    hud_call(&mut h, "settext", &hud, &[Value::str("Score: "), Value::Int(5)]).unwrap();
    let v = h.hud_views(0);
    assert_eq!((v.len(), v[0].kind, v[0].text.as_str(), v[0].pos[0]), (1, super::player::HudKind::Text, "Score: 5", 10.0));
    // A timer counts down; zero seconds is an error as the handler has it.
    h.now_ms = 1000;
    assert!(hud_call(&mut h, "settimer", &hud, &[Value::Float(0.0)]).is_err());
    assert!(hud_call(&mut h, "settimer", &hud, &[Value::Float(5.0), Value::Int(1)]).is_err(), "exactly one argument");
    hud_call(&mut h, "settimer", &hud, &[Value::Float(5.0)]).unwrap();
    assert_eq!(h.hud_views(3000)[0].timer_left, 3.0);
    assert_eq!(h.hud_views(9000)[0].timer_left, 0.0);
    // setshader takes one or three arguments and a name.
    assert!(hud_call(&mut h, "setshader", &hud, &[Value::str("white"), Value::Int(4)]).is_err());
    assert!(hud_call(&mut h, "setshader", &hud, &[Value::str("")]).is_err());
    hud_call(&mut h, "setshader", &hud, &[Value::str("white"), Value::Int(64), Value::Int(8)]).unwrap();
    assert_eq!((h.hud_views(0)[0].kind, h.hud_views(0)[0].size), (super::player::HudKind::Shader, [64, 8]));
    // A fade runs from the alpha it had to the one the script sets next.
    assert!(hud_call(&mut h, "fadeovertime", &hud, &[Value::Float(61.0)]).is_err());
    hud_call(&mut h, "fadeovertime", &hud, &[Value::Float(2.0)]).unwrap();
    o.set(h.key_name("alpha"), Value::Float(0.0));
    assert_eq!(h.hud_views(1000)[0].alpha, 1.0);
    assert!((h.hud_views(2000)[0].alpha - 0.5).abs() < 1e-5);
    assert_eq!(h.hud_views(4000)[0].alpha, 0.0);
    // A client element belongs to its player; destroy takes it away.
    let mine = call(&mut h, "newclienthudelem", None, &[Iw6Host::value(p)]);
    assert_eq!(h.hud_views(0).iter().filter(|v| v.client == Some(p)).count(), 1);
    hud_call(&mut h, "destroy", &mine, &[]).unwrap();
    assert_eq!(h.hud_views(0).len(), 1);
    assert!(call_err(&mut h, "newteamhudelem", None, &[Value::str("spectator")]));
}

fn call_err(h: &mut Iw6Host, name: &str, this: Option<u32>, args: &[Value]) -> bool {
    use gsc_iw6_vm::Host;
    let (_, id) = gsc_iw6::builtins::lookup(name).unwrap();
    let me = this.map_or(Value::Undefined, Iw6Host::value);
    let mut ctx = gsc_iw6_vm::CallCtx::detached(&me, &Value::Undefined, 0);
    (if this.is_some() { h.call_method(id, &mut ctx, &me, args) } else { h.call_function(id, &mut ctx, args) }).is_err()
}

#[test]
fn weapons_ammo_and_offhands_are_kept_per_player() {
    let (mut h, p) = player_host();
    h.weapon_info.insert("iw6_ak12_mp".into(), (30, 120));
    assert_eq!(call(&mut h, "getcurrentweapon", Some(p), &[]).as_str(), Some("none"));
    call(&mut h, "giveweapon", Some(p), &[Value::str("iw6_ak12_mp")]);
    call(&mut h, "giveweapon", Some(p), &[Value::str("IW6_AK12_MP")]);
    call(&mut h, "giveweapon", Some(p), &[Value::str("frag_grenade_mp")]);
    assert_eq!(call(&mut h, "hasweapon", Some(p), &[Value::str("iw6_ak12_mp")]).as_int(), Some(1));
    assert_eq!(alen(&call(&mut h, "getweaponslistall", Some(p), &[])), Some(2), "one AK, one grenade");
    assert_eq!(alen(&call(&mut h, "getweaponslistprimaries", Some(p), &[])), Some(1));
    assert_eq!(alen(&call(&mut h, "getweaponslistoffhands", Some(p), &[])), Some(1));
    assert_eq!(call(&mut h, "getweaponammoclip", Some(p), &[Value::str("iw6_ak12_mp")]).as_int(), Some(30));
    call(&mut h, "setweaponammoclip", Some(p), &[Value::str("iw6_ak12_mp"), Value::Int(3)]);
    call(&mut h, "setweaponammostock", Some(p), &[Value::str("iw6_ak12_mp"), Value::Int(0)]);
    assert_eq!((call(&mut h, "getweaponammoclip", Some(p), &[Value::str("iw6_ak12_mp")]).as_int(), call(&mut h, "getweaponammostock", Some(p), &[Value::str("iw6_ak12_mp")]).as_int()), (Some(3), Some(0)));
    call(&mut h, "givemaxammo", Some(p), &[Value::str("iw6_ak12_mp")]);
    assert_eq!(call(&mut h, "getweaponammostock", Some(p), &[Value::str("iw6_ak12_mp")]).as_int(), Some(120));
    call(&mut h, "setspawnweapon", Some(p), &[Value::str("iw6_ak12_mp")]);
    assert_eq!(call(&mut h, "getcurrentweapon", Some(p), &[]).as_str(), Some("iw6_ak12_mp"));
    call(&mut h, "takeweapon", Some(p), &[Value::str("iw6_ak12_mp")]);
    assert_eq!((call(&mut h, "getcurrentweapon", Some(p), &[]).as_str(), call(&mut h, "hasweapon", Some(p), &[Value::str("iw6_ak12_mp")]).as_int()), (Some("none"), Some(0)));
    call(&mut h, "takeallweapons", Some(p), &[]);
    assert_eq!(alen(&call(&mut h, "getweaponslistall", Some(p), &[])), Some(0));
    // Offhand classes: a name of the six, else an error.
    call(&mut h, "setoffhandsecondaryclass", Some(p), &[Value::str("smoke")]);
    assert_eq!(call(&mut h, "getoffhandsecondaryclass", Some(p), &[]).as_str(), Some("smoke"));
    assert!(call_err(&mut h, "setoffhandprimaryclass", Some(p), &[Value::str("nonsense")]));
}

#[test]
fn perks_omnvars_flags_and_menus_read_back() {
    let (mut h, p) = player_host();
    assert!(call_err(&mut h, "setperk", Some(p), &[Value::str("specialty_fastsprintrecovery")]), "setperk takes exactly three arguments");
    call(&mut h, "setperk", Some(p), &[Value::str("specialty_fastsprintrecovery"), Value::Int(1), Value::Int(0)]);
    assert!(h.pstates[&p].perks.contains_key("specialty_fastsprintrecovery"));
    call(&mut h, "unsetperk", Some(p), &[Value::str("SPECIALTY_FASTSPRINTRECOVERY")]);
    assert!(h.pstates[&p].perks.is_empty());
    call(&mut h, "setclientomnvar", Some(p), &[Value::str("ui_hud"), Value::Int(3)]);
    assert_eq!(h.pstates[&p].omnvars["ui_hud"].as_int(), Some(3));
    assert!(h.out.iter().any(|e| matches!(e, Effect::Omnvar { name, value, .. } if name == "ui_hud" && *value == 3.0)));
    call(&mut h, "setclientdvar", Some(p), &[Value::str("cg_fov"), Value::Int(80)]);
    assert_eq!(h.stores.client_dvars.get(&(p, "cg_fov".into())).map(String::as_str), Some("80"));
    // The unnamed flag setters: 0x8310 sets a bit, 0x8311 clears it.
    call_id(&mut h, 0x8310, p);
    assert!(h.client_flag(p, 0x80));
    call_id(&mut h, 0x8311, p);
    assert!(!h.client_flag(p, 0x80));
    call(&mut h, "openpopupmenu", Some(p), &[Value::str("class")]);
    assert_eq!(h.pstates[&p].menu.as_deref(), Some("class"));
    call(&mut h, "closepopupmenu", Some(p), &[Value::str("class")]);
    assert_eq!(h.pstates[&p].menu, None);
}

fn call_id(h: &mut Iw6Host, id: u16, e: u32) {
    use gsc_iw6_vm::Host;
    let me = Iw6Host::value(e);
    let mut ctx = gsc_iw6_vm::CallCtx::detached(&me, &Value::Undefined, 0);
    h.call_method(id, &mut ctx, &me, &[]).unwrap();
}

#[test]
fn sounds_effects_and_views_come_out_as_effects() {
    let (mut h, p) = player_host();
    call(&mut h, "playlocalsound", Some(p), &[Value::str("mp_card_slide")]);
    assert!(h.out.iter().any(|e| matches!(e, Effect::LocalSound { alias, .. } if alias == "mp_card_slide")));
    h.ent_mut(p).unwrap().bot = true;
    h.out.clear();
    call(&mut h, "playlocalsound", Some(p), &[Value::str("x")]);
    assert!(h.out.is_empty(), "a bot hears nothing");
    assert_eq!(call(&mut h, "isbot", None, &[Iw6Host::value(p)]).as_int(), Some(1));
    assert_eq!(call(&mut h, "isai", None, &[Iw6Host::value(p)]).as_int(), Some(1));
    assert_eq!(call(&mut h, "isagent", None, &[Iw6Host::value(p)]).as_int(), Some(0));
    assert!(call_err(&mut h, "playsoundtoteam", Some(p), &[Value::str("s"), Value::str("neutral")]));
    call(&mut h, "earthquake", None, &[Value::Float(0.3), Value::Float(2.0), Value::Vector([0.0; 3]), Value::Float(500.0)]);
    call(&mut h, "earthquake", None, &[Value::Float(0.0), Value::Float(2.0), Value::Vector([0.0; 3]), Value::Float(500.0)]);
    assert_eq!(h.out.iter().filter(|e| matches!(e, Effect::Earthquake { .. })).count(), 1, "a zero scale sends nothing");
    call(&mut h, "visionsetnaked", None, &[Value::str("mp_frag")]);
    assert_eq!(h.stores.vision["naked"], ("mp_frag".to_owned(), 1.0), "the default time is a second");
    assert!(call_err(&mut h, "shellshock", Some(p), &[Value::str("default"), Value::Float(61.0)]));
    call(&mut h, "geteye", Some(p), &[]);
    assert_eq!(call(&mut h, "geteye", Some(p), &[]).as_vector(), Some([100.0, 100.0, 60.0]));
    call(&mut h, "setstance", Some(p), &[Value::str("crouch")]);
    assert_eq!(call(&mut h, "geteye", Some(p), &[]).as_vector(), Some([100.0, 100.0, 40.0]));
    h.set_field(p, super::host::F_HEALTH, Value::Int(0));
    assert_eq!(call(&mut h, "isalive", None, &[Iw6Host::value(p)]).as_int(), Some(0));
    call(&mut h, "precachemodel", None, &[Value::str("Foo_Model")]);
    assert!(super::player::precached(&h, "model").contains("foo_model"));
    call(&mut h, "setlightintensity", Some(p), &[Value::Float(0.25)]);
    assert_eq!(call(&mut h, "getlightintensity", Some(p), &[]).as_float(), Some(0.25));
}

/// Strikezone swaps the nuke for its level event: the map sets `level.nukeoverride` (field 10679)
/// and, once its teleport setup has run, the hook starts the "destroyed" event and answers true.
/// Favela sets none (its nuke is Ghosts' own).
#[test]
fn strikezone_has_a_nuke_hook_and_favela_has_none() {
    let Some(dir) = zone_dir() else {
        eprintln!("no Ghosts zones installed: skipped");
        return;
    };
    for (map, has_hook) in [("mp_strikezone", true), ("mp_favela_iw6", false)] {
        if !dir.join(format!("{map}.ff")).is_file() {
            eprintln!("{map} not staged: skipped");
            continue;
        }
        let loaded = load_map(&dir, map);
        let data = loaded.data(map);
        let (mut vm, _) = boot::boot(&data).unwrap();
        let mut sb = Sandbox::default();
        sb.apply(&mut vm);
        let mut errors: Vec<String> = Vec::new();
        sb.run(&mut vm, 200, &mut errors);
        eprintln!(
            "{map}: teleport origins {}, zones {:?}, stage {:?}",
            vm.host().find("targetname", "teleport_world_origin").len(),
            vm.level_obj().get(&Name::Id(32797)),
            vm.level_obj().get(&Name::Id(32796))
        );
        let hook = vm.level_obj().get(&Name::Id(10679));
        assert_eq!(matches!(hook, Some(Value::Function(_))), has_hook, "{map}: hook {hook:?}");
        if let Some(Value::Function(f)) = hook {
            let lv = Value::Object(vm.level_obj().clone());
            let answer = vm.call(lv, f, vec![]);
            let errs: Vec<String> = vm.take_errors().iter().map(|e| e.to_string()).collect();
            eprintln!("{map}: hook answered {answer:?}, errors {errs:?}");
            assert!(answer.is_some(), "{map}: the hook ran: {errs:?}");
            // (And the event it started runs on without a script error.)
            let mut after: Vec<String> = Vec::new();
            sb.run(&mut vm, 400, &mut after);
            assert!(after.is_empty(), "{map}: the map's nuke event: {after:?}");
        }
    }
}
