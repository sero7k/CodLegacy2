//! bo2mp: Shadows of Evil's level start with Black Ops III's own scripts,
//! headless (no assets but the scripts and the map's entities): what runs,
//! what the scripts call that the engine side does not have yet, and the
//! script errors. Reads the objects `t7_gsc_scan <zone> <dir>` wrote, one
//! folder per zone in load order, from `T7_SCRIPTS_DIR` (its `core_patch`,
//! `zm_patch`, `zm_zod_patch`), and the entity string `t7_ents` printed
//! from `T7_ENTS`, and the string tables `t7_tables --dump` wrote from
//! `T7_TABLES` (optional); skipped when the first two are not given.

use crate::world::Tick;
use crate::{SimWorld, T6Install};

/// The map under test (`T7_MAP`, default Shadows of Evil).
fn t7_map() -> String {
    std::env::var("T7_MAP").unwrap_or_else(|_| "zm_zod".to_owned())
}

fn inputs() -> Option<(Vec<Vec<u8>>, String)> {
    let dir = std::path::PathBuf::from(std::env::var_os("T7_SCRIPTS_DIR")?);
    let ents = std::fs::read_to_string(std::env::var_os("T7_ENTS")?).ok()?;
    // (T7_MAP=zm_castle etc.: another map. Its scripts come from the shared
    // level zone `zm_levelcommon` and its patch, in load order; a later
    // zone's object of the same name replaces an earlier one. Shadows of
    // Evil's patch holds all of its own. The map zone itself holds the
    // scripts no patch repeats, read before the patch: Gorod Krovi's
    // sentinel drone and dragon whelp vehicles, its Mangler (`ai/raz`) and
    // elemental zombies; without them a sentinel round never ended.)
    let map = t7_map();
    let mut zones = vec!["core_patch".to_owned(), "zm_patch".to_owned()];
    if map != "zm_zod" {
        zones.push("zm_levelcommon".to_owned());
    }
    zones.push(map.clone());
    zones.push(format!("{map}_patch"));
    let mut order: Vec<std::ffi::OsString> = Vec::new();
    let mut by_name: std::collections::HashMap<std::ffi::OsString, Vec<u8>> = std::collections::HashMap::new();
    for zone in zones {
        let Ok(rd) = std::fs::read_dir(dir.join(&zone)) else {
            // (core_patch and zm_patch are required; a Workshop map has no
            // `<map>_patch`, its scripts are in the map zone.)
            if zone == "core_patch" || zone == "zm_patch" {
                return None;
            }
            continue;
        };
        let mut files: Vec<_> = rd.filter_map(|e| e.ok().map(|e| e.path())).collect();
        files.sort();
        for f in files {
            let name = f.file_name()?.to_owned();
            if !by_name.contains_key(&name) {
                order.push(name.clone());
            }
            by_name.insert(name, std::fs::read(f).ok()?);
        }
    }
    let objects = order.iter().filter_map(|n| by_name.remove(n)).collect();
    Some((objects, ents))
}

/// `t7_tables --dump`'s file: `T\tname\tcolumns` then `C\tcell` lines.
fn tables() -> std::collections::BTreeMap<String, crate::T6Table> {
    let mut out = std::collections::BTreeMap::new();
    let Some(text) = std::env::var_os("T7_TABLES").and_then(|f| std::fs::read_to_string(f).ok()) else {
        return out;
    };
    let mut cur: Option<(String, crate::T6Table)> = None;
    for line in text.lines() {
        if let Some(rest) = line.strip_prefix("T\t") {
            if let Some((n, t)) = cur.take() {
                out.insert(n, t);
            }
            let (name, cols) = rest.rsplit_once('\t').unwrap_or((rest, "1"));
            cur = Some((name.to_ascii_lowercase(), crate::T6Table { columns: cols.parse().unwrap_or(1), rows: 0, cells: Vec::new() }));
        } else if let (Some(c), Some((_, t))) = (line.strip_prefix("C\t"), cur.as_mut()) {
            t.cells.push(c.to_owned());
            t.rows = t.cells.len() / t.columns.max(1);
        }
    }
    if let Some((n, t)) = cur {
        out.insert(n, t);
    }
    out
}

/// A weapon catalog for the headless run: "none", then every weapon (and
/// upgrade) the zombies weapons tables name and the start-up ones, each a
/// plain gun (8-round clip, 80 rounds), the melee ones melee.
fn weapon_catalog(tables: &std::collections::BTreeMap<String, crate::T6Table>) -> (Vec<String>, Vec<weapon_iw4::WeaponCombatFacts>) {
    let mut names = vec!["none".to_owned()];
    let mut add = |n: &str| {
        let n = n.trim().to_ascii_lowercase();
        if !n.is_empty() && !names.contains(&n) {
            names.push(n);
        }
    };
    // (The remastered maps' weapons are in `zm_levelcommon_weapons`: Kino's
    // start pistol, `pistol_m1911`.)
    for t in ["gamedata/weapons/zm/zm_weapons.csv".to_owned(), "gamedata/weapons/zm/zm_levelcommon_weapons.csv".to_owned(), format!("gamedata/weapons/zm/{}_weapons.csv", t7_map())] {
        let Some(t) = tables.get(&t) else { continue };
        for row in t.cells.chunks(t.columns.max(1)).skip(1) {
            row.first().map(|c| add(c));
            row.get(1).map(|c| add(c));
        }
    }
    for n in ["pistol_standard", "pistol_standard_upgraded", "pistol_m1911", "pistol_m1911_upgraded", "glaive_apothicon_0", "glaive_apothicon_1", "glaive_apothicon_2", "glaive_apothicon_3", "glaive_keeper_0", "glaive_keeper_1", "glaive_keeper_2", "glaive_keeper_3", "zombie_fists", "zombie_beast_grapple_dwr", "zombie_beast_lightning_dwl", "zombie_beast_lightning_dwl2", "zombie_beast_lightning_dwl3", "syrette", "zombie_perk_bottle_doubletap", "zombie_perk_bottle_jugg", "zombie_perk_bottle_revive", "zombie_perk_bottle_sleight", "zombie_perk_bottle_staminup", "zombie_perk_bottle_widows_wine", "zombie_knuckle_crack", "zombie_builder", "zombie_bgb_grab", "zombie_bgb_use"] {
        add(n);
    }
    // (And the quest weapons the maps give from their zones, in no table:
    // Der Eisendrache's bows, Gorod Krovi's gauntlet, Zetsubou's KT-4.)
    for n in ["elemental_bow", "elemental_bow_wolf_howl", "elemental_bow_wolf_howl4", "elemental_bow_storm", "elemental_bow_storm4", "elemental_bow_rune_prison", "elemental_bow_rune_prison4", "elemental_bow_demongate", "elemental_bow_demongate4", "dragon_gauntlet_flamethrower", "dragon_gauntlet", "hero_mirg2000", "hero_mirg2000_upgraded"] {
        add(n);
    }
    // (Inventory: guns primary, grenades and mines offhand, the rest
    // items: the knife, perk bottles, the beast's hands.)
    let primary = ["pistol_", "smg_", "ar_", "lmg_", "shotgun_", "sniper_", "launcher_", "ray_gun", "idgun", "elemental_bow", "hero_mirg2000"];
    let offhand = ["grenade", "bouncingbetty", "octobomb"];
    let combat = names
        .iter()
        .map(|n| {
            let mut f = weapon_iw4::WeaponCombatFacts::none();
            f.clip_size = 8;
            f.start_ammo = 80;
            f.max_ammo = 80;
            f.inventory_type = if primary.iter().any(|p| n.starts_with(p)) {
                0
            } else if offhand.iter().any(|p| n.contains(p)) {
                1
            } else {
                2
            };
            f
        })
        .collect();
    (names, combat)
}

/// The map zone's Havok packfiles, `t7_havok_dump <zone> <dir>`'s files
/// from `T7_NAV` (a folder), one after another.
fn nav() -> Vec<u8> {
    let Some(dir) = std::env::var_os("T7_NAV") else { return Vec::new() };
    let Ok(rd) = std::fs::read_dir(dir) else { return Vec::new() };
    let mut files: Vec<_> = rd.filter_map(|e| e.ok().map(|e| e.path())).collect();
    files.sort();
    files.iter().filter_map(|f| std::fs::read(f).ok()).flatten().collect()
}

/// `t7_characters <zone> --cast`'s file from `T7_CAST`: the AI types'
/// characters (`C\taitype\tbody\theads\tattached`) and the zombies'
/// clips (`A\tname\tframes\tframerate\tlooping`), as the game lane gives
/// them (`lane::t7::map_props`).
fn cast() -> (Vec<super::T7Cast>, Vec<super::T6Anim>) {
    let Some(text) = std::env::var_os("T7_CAST").and_then(|f| std::fs::read_to_string(f).ok()) else {
        return (Vec::new(), Vec::new());
    };
    let list = |s: &str| s.split(',').filter(|x| !x.is_empty()).map(str::to_owned).collect::<Vec<_>>();
    let (mut cast, mut anims): (Vec<super::T7Cast>, Vec<super::T6Anim>) = (Vec::new(), Vec::new());
    for line in text.lines() {
        let f: Vec<&str> = line.split('\t').collect();
        match f.as_slice() {
            ["C", ai, body, heads, attached] => {
                let c = (body.to_string(), list(heads), list(attached));
                match cast.iter_mut().find(|x| x.0 == *ai) {
                    Some(x) => x.1.push(c),
                    None => cast.push((ai.to_string(), vec![c])),
                }
            }
            ["N", name, time] => {
                if let Some(a) = anims.last_mut() {
                    a.notifies.push((name.to_string(), time.parse().unwrap_or(0.0)));
                }
            }
            ["D", frame, x, y, z] => {
                if let Some(a) = anims.last_mut() {
                    a.delta_trans.push((frame.parse().unwrap_or(0), [x.parse().unwrap_or(0.0), y.parse().unwrap_or(0.0), z.parse().unwrap_or(0.0)]));
                }
            }
            ["A", name, frames, rate, looping] => anims.push(super::T6Anim {
                name: name.to_string(),
                numframes: frames.parse().unwrap_or(30),
                framerate: rate.parse().unwrap_or(30.0),
                looping: *looping == "1",
                delta_trans: Vec::new(),
                delta_quat: Vec::new(),
                notifies: Vec::new(),
            }),
            _ => {}
        }
    }
    (cast, anims)
}

/// `t7_bundle <zone> "" --dump`'s file from `T7_BUNDLES`: `B\tname` then
/// `K\tobject\tkey\tkind\tvalue\tdata` lines.
fn bundles() -> Vec<super::T7Bundle> {
    let Some(text) = std::env::var_os("T7_BUNDLES").and_then(|f| std::fs::read_to_string(f).ok()) else {
        return Vec::new();
    };
    let mut out: Vec<super::T7Bundle> = Vec::new();
    for line in text.lines() {
        if let Some(name) = line.strip_prefix("B\t") {
            out.push((name.to_owned(), Vec::new()));
        } else if let (Some(rest), Some(b)) = (line.strip_prefix("K\t"), out.last_mut()) {
            let f: Vec<&str> = rest.split('\t').collect();
            if f.len() == 5 {
                b.1.push((f[0].parse().unwrap_or(-1), f[1].to_owned(), f[2].parse().unwrap_or(0), f[3].to_owned(), f[4].parse().unwrap_or(0)));
            }
        }
    }
    out
}

/// `t7_cmodels <map.ff> --planes`'s lines from `T7_CMODELS`: the map's
/// brush models (`*N radius [mins]..[maxs] ...`) and their brushes (`B *N
/// <brush> <contents> nx,ny,nz,d ...`), so the scripts' volumes
/// (`istouching` a zone's `info_volume`) are the map's own shapes.
/// The map's trigger models (`T ?<n> ...` lines of `t7_cmodels --planes`):
/// each one's hulls about its entity.
fn trigger_hulls() -> Vec<Vec<crate::world::SimTriggerHull>> {
    let Some(text) = std::env::var_os("T7_CMODELS").and_then(|f| std::fs::read_to_string(f).ok()) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for line in text.lines() {
        let Some(rest) = line.strip_prefix("T ?") else { continue };
        let mut w = rest.split_whitespace();
        let Some(n) = w.next().and_then(|n| n.parse::<usize>().ok()) else { continue };
        let hulls: Vec<crate::world::SimTriggerHull> = w
            .filter_map(|h| {
                let v: Vec<f32> = h.split(',').filter_map(|x| x.parse().ok()).collect();
                (v.len() == 6).then(|| crate::world::SimTriggerHull { mid: [v[0], v[1], v[2]], half: [v[3], v[4], v[5]], slabs: Vec::new() })
            })
            .collect();
        if out.len() <= n {
            out.resize(n + 1, Vec::new());
        }
        out[n] = hulls;
    }
    out
}

fn cmodels() -> (Vec<clipmap_iw4::ClipCmodel>, Vec<crate::world::SimBrush>, Vec<u16>) {
    let Some(text) = std::env::var_os("T7_CMODELS").and_then(|f| std::fs::read_to_string(f).ok()) else {
        return Default::default();
    };
    let vec3 = |t: &str| -> Option<[f32; 3]> {
        let v: Vec<f32> = t.trim_matches(|c| c == '[' || c == ']').split(',').filter_map(|x| x.trim().parse().ok()).collect();
        (v.len() == 3).then(|| [v[0], v[1], v[2]])
    };
    let mut models = Vec::new();
    let mut brushes = Vec::new();
    let mut leafbrushes: Vec<u16> = Vec::new();
    for line in text.lines() {
        if let Some(rest) = line.strip_prefix("B ") {
            let mut w = rest.split_whitespace();
            let (Some(_), Some(_), Some(contents)) = (w.next(), w.next(), w.next()) else { continue };
            let planes: Vec<[f32; 4]> = w
                .filter_map(|p| {
                    let v: Vec<f32> = p.split(',').filter_map(|x| x.parse().ok()).collect();
                    (v.len() == 4).then(|| [v[0], v[1], v[2], v[3]])
                })
                .collect();
            let Some(m) = models.last_mut() else { continue };
            let m: &mut clipmap_iw4::ClipCmodel = m;
            leafbrushes.push(u16::try_from(brushes.len()).unwrap_or(u16::MAX));
            m.num_brushes += 1;
            // (Contents none: these are brush models' brushes in their own
            // space, read for the volumes' shapes; as world clip, a nav
            // volume's box at the map's middle blocked every trace there,
            // so no ritual altar could be looked at.)
            let _ = contents;
            brushes.push(crate::world::SimBrush { plane_surface_flags: vec![0; planes.len()], planes, contents: 0, glass_encoded: 0 });
            continue;
        }
        if !line.starts_with('*') {
            continue;
        }
        let mut w = line.split_whitespace();
        let (Some(_), Some(radius)) = (w.next(), w.next()) else { continue };
        let rest: String = w.collect::<Vec<_>>().join(" ");
        let Some(bounds) = rest.split(" brushes").next() else { continue };
        let Some((lo, hi)) = bounds.split_once("..") else { continue };
        let (Some(mins), Some(maxs)) = (vec3(lo), vec3(hi)) else { continue };
        let first = u32::try_from(leafbrushes.len()).unwrap_or(0);
        models.push(clipmap_iw4::ClipCmodel { mins, maxs, radius: radius.parse().unwrap_or(0.0), first_brush: first, num_brushes: 0 });
    }
    (models, brushes, leafbrushes)
}

/// The living AI's distance to the first player: (nearest, mean), and how
/// many stand on the nav mesh.
fn ai_distances(ecs: &mut bevy_ecs::prelude::World) -> Option<(f32, f32)> {
    let p = super::frame(ecs).player(crate::world::ClientId(0))?.origin;
    let zm = ecs.resource::<super::Zm>();
    let d: Vec<f32> = zm
        .actors
        .iter()
        .filter(|(_, a)| a.alive)
        .filter_map(|(n, _)| zm.ents.get(n))
        .map(|e| ((e.origin[0] - p[0]).powi(2) + (e.origin[1] - p[1]).powi(2)).sqrt())
        .collect();
    (!d.is_empty()).then(|| (d.iter().copied().fold(f32::MAX, f32::min), d.iter().sum::<f32>() / d.len() as f32))
}

/// The round (`level.round_number`) and the AI alive.
fn state(ecs: &mut bevy_ecs::prelude::World) -> (String, usize) {
    let zombies = ecs.resource::<super::Zm>().actors.values().filter(|a| a.alive).count();
    let round = super::with_vm(ecs, |vm, world| {
        let f = vm.intern("round_number");
        let v = vm.raw_field(vm.level, f);
        let p = vm.intern("players");
        let players = match vm.raw_field(vm.level, p) {
            gsc_t6::Value::Array(a) => a.len().to_string(),
            other => vm.to_text(&other),
        };
        // The first player's session state, and the level flags the round
        // start waits on.
        let mut extra = String::new();
        if let gsc_t6::Value::Array(a) = vm.raw_field(vm.level, p) {
            if let Some(gsc_t6::Value::Object(o)) = a.snapshot().values_in_order().next().cloned() {
                let f = vm.intern("sessionstate");
                let st = vm.get_field(world, o, f);
                let pf = vm.intern("pers");
                let lives = match vm.get_field(world, o, pf) {
                    gsc_t6::Value::Array(p) => {
                        let k = vm.string("lives");
                        vm.to_key(&k).ok().and_then(|k| p.snapshot().get(&k).cloned()).map_or_else(|| "-".to_owned(), |v| vm.to_text(&v))
                    }
                    other => vm.to_text(&other),
                };
                let nl = vm.intern("numlives");
                extra = format!(", player 0 {} lives {lives} of {}", vm.to_text(&st), vm.to_text(&vm.raw_field(vm.level, nl)));
            }
        }
        // T7_FIELDS=a,b: those level fields too.
        for name in std::env::var("T7_FIELDS").unwrap_or_else(|_| String::new()).split(',').filter(|n| !n.is_empty()) {
            let f = vm.intern(name);
            extra.push_str(&format!(", level.{name} {}", vm.to_text(&vm.raw_field(vm.level, f))));
        }
        let fl = vm.intern("flag");
        if let gsc_t6::Value::Array(flags) = vm.raw_field(vm.level, fl) {
            let flags = flags.snapshot();
            let extra_flags = std::env::var("T7_FLAGS").unwrap_or_else(|_| String::new());
            let wanted: Vec<&str> = ["initial_blackscreen_passed", "start_zombie_round_logic", "begin_spawning", "power_on"].into_iter().chain(extra_flags.split(',').filter(|f| !f.is_empty())).collect();
            for name in wanted {
                let key = vm.string(name);
                if let Ok(k) = vm.to_key(&key) {
                    extra.push_str(&format!(", {name} {}", flags.get(&k).map_or_else(|| "-".to_owned(), |v| vm.to_text(v))));
                }
            }
        }
        let alive: Vec<u32> = world.get_resource::<super::t7_ai::T7Ai>().map_or_else(Vec::new, |s| s.vehicles.iter().copied().collect());
        let at = world.contains_resource::<crate::step::StepRequest>().then(|| super::frame(world).player(crate::world::ClientId(0)).map(|ps| ps.origin)).flatten();
        let near = alive
            .iter()
            .filter_map(|n| world.resource::<super::Zm>().ents.get(n).map(|e| e.origin))
            .filter_map(|o| at.map(|p| ((o[0] - p[0]).powi(2) + (o[1] - p[1]).powi(2) + (o[2] - p[2]).powi(2)).sqrt()))
            .fold(f32::MAX, f32::min);
        let near = if alive.is_empty() { String::new() } else { format!(" (nearest {near:.0})") };
        format!("{} (level.players {players}{extra}, {} vehicles{near})", vm.to_text(&v), alive.len())
    })
    .unwrap_or_else(String::new);
    (round, zombies)
}

/// `T7_USE="x y z@seconds,..."`: at that time player 0 (given 5000 points
/// the first time) uses the use trigger nearest that point: its "trigger"
/// notify with him, as the engine sends when he presses use in it.
/// `T7_TRIG` the same for any trigger (a trigger_multiple he walks or,
/// as the beast, swings into: the smashable crates and doors).
fn use_triggers(ecs: &mut bevy_ecs::prelude::World, t: u32, given: &mut bool, var: &str) {
    let Ok(list) = codlegacy_env::var(var) else {
        return;
    };
    let any = var != "T7_USE";
    for entry in list.split(',') {
        let Some((at, when)) = entry.split_once('@') else { continue };
        // ("@secs+hold": use held that many seconds more, as a craftable
        // table or a ritual altar wants.)
        let (when, hold) = when.split_once('+').map_or((when, 0.0), |(w, h)| (w, h.trim().parse::<f32>().unwrap_or(0.0)));
        let Ok(secs) = when.trim().parse::<f32>() else { continue };
        // (Nothing to do outside this entry's few seconds: its points can
        // cost a thread walk, `waiter:`.)
        if t + 41 < (secs * 20.0) as u32 || t > ((secs + hold) * 20.0) as u32 + 40 {
            continue;
        }
        // ("x y z~a b c": he stands at a b c, facing x y z, as at a
        // table he must look at.)
        let (at, stand) = at.split_once('~').map_or((at, None), |(a, b)| (a, Some(b)));
        // ("box": the mystery box where it is now, `level.chests[level.chest_index]`.)
        // ("ent:N~a b c": entity N, a trigger the scripts spawn once he
        // stands near, e.g. a unitrigger's; until it is there, the stand.)
        let mut p: Vec<f32> = point_of(ecs, at);
        if p.len() != 3 && at.trim().starts_with("ent:") {
            p = stand.map_or_else(Vec::new, |b| point_of(ecs, b.trim_end().trim_end_matches('!')));
        }
        if p.len() != 3 {
            continue;
        }
        // ("a b c!": stood exactly there, not snapped onto the nav mesh: a
        // piece lying below the floor the mesh has nearest.)
        let exact = stand.is_some_and(|b| b.trim_end().ends_with('!'));
        let stand: Vec<f32> = stand.map_or_else(|| p.clone(), |b| point_of(ecs, b.trim_end().trim_end_matches('!')));
        if stand.len() != 3 {
            continue;
        }
        // (From when he is put there: a `require_look_at` stub only gets
        // its trigger while he looks at it.)
        if t + 41 >= (secs * 20.0) as u32 && t <= ((secs + hold) * 20.0) as u32 {
            // (Looking at it: a `require_look_at` stub checks his view.)
            look_at(ecs, [p[0], p[1], p[2]]);
        }
        if t > (secs * 20.0) as u32 && t <= ((secs + hold) * 20.0) as u32 {
            let cmd = playerstate_iw4::UserCmd { buttons: playerstate_iw4::buttons::USE, ..Default::default() };
            ecs.resource_mut::<crate::step::StepRequest>().input.cmds.push((crate::world::ClientId(0), cmd));
        }
        // Two seconds before, he is put next to it (on the nav mesh), so
        // the unitriggers near him are made.
        if t + 40 == (secs * 20.0) as u32 {
            let spot = ecs
                .get_resource::<super::t7_ai::T7Ai>()
                .and_then(|s| s.nav.as_ref().and_then(|nav| super::t7_ai::closest_point(nav, [stand[0], stand[1], stand[2]], 160.0)))
                // (Right at it, on the floor's height: a wall buy's use
                // box reaches a few units off the wall.)
                // (Unless that floor is another storey: then just under it.)
                .map_or([stand[0], stand[1], stand[2]], |q| if (q[2] - stand[2]).abs() < 96.0 { [stand[0], stand[1], q[2]] } else { [stand[0], stand[1], stand[2] - 40.0] });
            let spot = if exact { [stand[0], stand[1], stand[2]] } else { spot };
            super::teleport_player(ecs, crate::world::ClientId(0), spot);
            // (In "ufo" mode, as a developer moves: the scripts leave him
            // be outside the zones opened so far.)
            if let Some(mut st) = ecs.get_resource_mut::<super::t7::T7State>() {
                st.move_mode.insert(0, "ufo".to_owned());
            }
            eprintln!("t7 use: player 0 put at {spot:?}");
        }
        if t == (secs * 20.0) as u32 + 40 {
            eprintln!("t7 use: after, {}", player_line(ecs));
        }
        if t != (secs * 20.0) as u32 {
            continue;
        }
        if !*given {
            *given = true;
            super::autoplay::give_points(ecs, 5000);
        }
        let zm = ecs.resource::<super::Zm>();
        let near = zm
            .ents
            .iter()
            .filter(|(_, e)| e.classname.starts_with("trigger_") && (any || e.classname.contains("use")) && e.obj.is_some())
            .min_by(|a, b| {
                let d = |e: &super::Ent| (e.origin[0] - p[0]).powi(2) + (e.origin[1] - p[1]).powi(2) + (e.origin[2] - p[2]).powi(2);
                d(a.1).total_cmp(&d(b.1))
            })
            .and_then(|(n, e)| e.obj.map(|o| (*n, o, (e.origin, e.classname.clone()))));
        let player = zm.players.get(&0).map(|p| p.obj);
        let (Some((n, trig, origin)), Some(player)) = (near, player) else { continue };
        eprintln!("t7 use: player 0 uses {} ent{n} at {:?}", origin.1, origin.0);
        // (He holds use this tick: the scripts ask `usebuttonpressed`.)
        let cmd = playerstate_iw4::UserCmd { buttons: playerstate_iw4::buttons::USE, ..Default::default() };
        ecs.resource_mut::<crate::step::StepRequest>().input.cmds.push((crate::world::ClientId(0), cmd));
        super::with_vm(ecs, |vm, world| vm.notify_str(world, trig, "trigger", &[gsc_t6::Value::Object(player)]));
    }
}

/// `T7_SHOOT="x y z@seconds[:weapon],..."`: at that time player 0 hits the
/// entity nearest that point (one that can take damage, or any) with that
/// weapon (default the beast's lightning): its "damage" notify, as the
/// engine sends for a hit.
fn shoot(ecs: &mut bevy_ecs::prelude::World, t: u32) {
    let Ok(list) = std::env::var("T7_SHOOT") else { return };
    for entry in list.split(',') {
        let Some((at, rest)) = entry.split_once('@') else { continue };
        let (when, weapon) = rest.split_once(':').unwrap_or((rest, "zombie_beast_lightning_dwl"));
        // ("@a-b/s": every s seconds from a to b.)
        let due = match when.split_once('-') {
            Some((a, b)) => {
                let (b, step) = b.split_once('/').unwrap_or((b, "1"));
                let (Ok(a), Ok(b), Ok(step)) = (a.trim().parse::<f32>(), b.trim().parse::<f32>(), step.trim().parse::<f32>()) else { continue };
                let (a, b, step) = ((a * 20.0) as u32, (b * 20.0) as u32, ((step * 20.0) as u32).max(1));
                t >= a && t <= b && (t - a) % step == 0
            }
            None => when.trim().parse::<f32>().is_ok_and(|secs| t == (secs * 20.0) as u32),
        };
        if !due {
            continue;
        }
        // (A point, "model:<name>" or "ent:<n>": `point_of`.)
        let p: Vec<f32> = point_of(ecs, at);
        if p.len() != 3 {
            continue;
        }
        // (An entity named, "ent:" or "waiter:", is hit itself, not the
        // nearest to its place: Revelations' Pack-a-Punch cords share
        // theirs with the sac.)
        let named = ent_of(ecs, at);
        let zm = ecs.resource::<super::Zm>();
        let near = zm
            .ents
            .iter()
            .filter(|(n, e)| e.obj.is_some() && !zm.players.contains_key(n) && named.is_none_or(|m| m == **n))
            .min_by(|a, b| {
                let d = |e: &super::Ent| (e.origin[0] - p[0]).powi(2) + (e.origin[1] - p[1]).powi(2) + (e.origin[2] - p[2]).powi(2);
                d(a.1).total_cmp(&d(b.1))
            })
            .and_then(|(n, e)| e.obj.map(|o| (*n, o, e.classname.clone())));
        let player = zm.players.get(&0).map(|p| p.obj);
        let (Some((n, obj, class)), Some(player)) = (near, player) else { continue };
        eprintln!("t7 shoot: player 0 hits {class} ent{n} with {weapon}");
        let w = weapon.to_owned();
        super::with_vm(ecs, |vm, world| {
            let wv = super::t7::weapon_object(vm, world, &w);
            let args = [
                gsc_t6::Value::Int(100),
                gsc_t6::Value::Object(player),
                gsc_t6::Value::Vec3([1.0, 0.0, 0.0]),
                gsc_t6::Value::Vec3([p[0], p[1], p[2]]),
                vm.string("MOD_PROJECTILE"),
                vm.string(""),
                vm.string(""),
                vm.string(""),
                wv,
                gsc_t6::Value::Int(0),
            ];
            vm.notify_str(world, obj, "damage", &args);
        });
    }
}

/// `T7_STUBUSE="x y z@seconds,..."`: the unitrigger stub nearest that
/// point (`level._unitriggers.trigger_stubs`) gets "trigger" from player 0,
/// as its spawned trigger's think passes a use on (for stubs whose trigger
/// the zone check keeps from spawning here: the train's button).
fn stub_use(ecs: &mut bevy_ecs::prelude::World, t: u32) {
    let Ok(list) = std::env::var("T7_STUBUSE") else { return };
    for entry in list.split(',') {
        let Some((at, when)) = entry.split_once('@') else { continue };
        let Ok(secs) = when.trim().parse::<f32>() else { continue };
        if t != (secs * 20.0) as u32 {
            continue;
        }
        let p: Vec<f32> = point_of(ecs, at);
        if p.len() != 3 {
            continue;
        }
        let Some(player) = ecs.resource::<super::Zm>().players.get(&0).map(|p| p.obj) else { continue };
        super::with_vm(ecs, |vm, world| {
            let u = vm.intern("_unitriggers");
            let gsc_t6::Value::Object(uo) = vm.raw_field(vm.level, u) else { return };
            let ts = vm.intern("trigger_stubs");
            let gsc_t6::Value::Array(stubs) = vm.raw_field(uo, ts) else { return };
            let of = vm.intern("origin");
            let mut best: Option<(f32, gsc_t6::ObjRef)> = None;
            // (Every stub at that same spot: a defend area started twice has two.)
            let mut all: Vec<(f32, gsc_t6::ObjRef)> = Vec::new();
            for v in stubs.snapshot().values_in_order() {
                let gsc_t6::Value::Object(o) = v else { continue };
                let Some(q) = vm.get_field(world, *o, of).as_vec3() else { continue };
                let d = (0..3).map(|i| (q[i] - p[i]).powi(2)).sum::<f32>();
                all.push((d, *o));
                if best.is_none_or(|b| d < b.0) {
                    best = Some((d, *o));
                }
            }
            if let Some((d, o)) = best {
                eprintln!("t7 stub use: stub {:.0} from the point", d.sqrt());
                vm.notify_str(world, o, "trigger", &[gsc_t6::Value::Object(player)]);
                for (d2, o2) in all {
                    if o2 != o && (d2.sqrt() - d.sqrt()).abs() < 1.0 {
                        vm.notify_str(world, o2, "trigger", &[gsc_t6::Value::Object(player)]);
                    }
                }
            }
        });
    }
}

/// Player 0's view turned onto a point (from his eye).
fn look_at(ecs: &mut bevy_ecs::prelude::World, p: [f32; 3]) {
    let Some(o) = super::frame(ecs).player(crate::world::ClientId(0)).map(|ps| ps.origin) else { return };
    let d = [p[0] - o[0], p[1] - o[1], p[2] - (o[2] + 60.0)];
    let yaw = d[1].atan2(d[0]).to_degrees();
    let pitch = -(d[2].atan2((d[0] * d[0] + d[1] * d[1]).sqrt())).to_degrees();
    super::set_player_view(ecs, crate::world::ClientId(0), [pitch, yaw, 0.0]);
}

/// A test aid's point: "x y z", "box" (the mystery box now) or
/// "model:<name>" (that model's entity now, e.g. the train).
fn point_of(ecs: &mut bevy_ecs::prelude::World, at: &str) -> Vec<f32> {
    let at = at.trim();
    if at == "box" {
        return box_origin(ecs).map_or_else(Vec::new, |o| o.to_vec());
    }
    // ("waiter:<function>#k": the k-th entity (by number) a thread of that
    // script function waits on: a target the scripts spawned, e.g. the
    // Apothicon fight's keepers.)
    if let Some(n) = ent_of(ecs, at) {
        return ecs.resource::<super::Zm>().ents.get(&n).map_or_else(Vec::new, |e| e.origin.to_vec());
    }
    if at.starts_with("waiter:") || at.starts_with("ent:") {
        return Vec::new();
    }
    // ("moved:<model>": that model only once it has left where it was first
    // seen, e.g. Gorod Krovi's Dragon Network parts, parked at their
    // prefab's 0 0 -273 until a drop pod's reward moves one out.)
    if let Some(m) = at.strip_prefix("moved:") {
        let zm = ecs.resource::<super::Zm>();
        let Some((n, o)) = zm.ents.iter().find(|(_, e)| e.model == m).map(|(n, e)| (*n, e.origin)) else { return Vec::new() };
        let first = FIRST_SEEN.with(|f| *f.borrow_mut().entry(n).or_insert(o));
        return if first == o { Vec::new() } else { o.to_vec() };
    }
    if let Some(m) = at.strip_prefix("model:") {
        let zm = ecs.resource::<super::Zm>();
        let shown = |e: &super::Ent| !e.hidden && !e.invisible_to_all && !e.invisible_to.contains(&0);
        // (A shown one: a quest piece waits hidden where its prefab put it,
        // e.g. Gorod Krovi's Dragon Network parts at 0 0 -273, until the
        // scripts show it where it drops.)
        return zm.ents.values().filter(|e| e.model == m && shown(e) && e.classname == "script_vehicle").chain(zm.ents.values().filter(|e| e.model == m && shown(e))).next().map_or_else(Vec::new, |e| e.origin.to_vec());
    }
    at.split_whitespace().filter_map(|x| x.parse().ok()).collect()
}

thread_local! {
    static FIRST_SEEN: std::cell::RefCell<std::collections::HashMap<u32, [f32; 3]>> = std::cell::RefCell::new(std::collections::HashMap::new());
}

/// The entity a test aid's "ent:<n>" or "waiter:<function>#k" names (the
/// k-th entity, by number, a thread of that script function waits on: a
/// target the scripts spawned, e.g. the Apothicon fight's keepers).
fn ent_of(ecs: &mut bevy_ecs::prelude::World, at: &str) -> Option<u32> {
    let at = at.trim();
    if let Some(spec) = at.strip_prefix("waiter:") {
        let (func, k) = spec.rsplit_once('#').filter(|(f, _)| !f.is_empty()).map_or((spec, 0), |(f, k)| (f, k.parse::<usize>().unwrap_or(0)));
        let mut ents: Vec<u32> = super::with_vm(ecs, |vm, _| {
            vm.describe_threads()
                .iter()
                .filter(|l| l.split_whitespace().next().is_some_and(|f| f.ends_with(&format!("::{func}"))))
                .filter_map(|l| l.split("waittill ent").nth(1).and_then(|r| r.split_whitespace().next()).and_then(|n| n.parse::<u32>().ok()))
                .collect()
        })
        .unwrap_or_default();
        ents.sort_unstable();
        ents.dedup();
        return ents.get(k).copied().filter(|n| ecs.resource::<super::Zm>().ents.contains_key(n));
    }
    at.strip_prefix("ent:").and_then(|n| n.trim().parse::<u32>().ok()).filter(|n| ecs.resource::<super::Zm>().ents.contains_key(n))
}

/// Where the mystery box is now (its chest struct's origin).
fn box_origin(ecs: &mut bevy_ecs::prelude::World) -> Option<[f32; 3]> {
    super::with_vm(ecs, |vm, world| {
        let (c, i) = (vm.intern("chests"), vm.intern("chest_index"));
        let gsc_t6::Value::Array(chests) = vm.raw_field(vm.level, c) else { return None };
        let i = vm.raw_field(vm.level, i);
        let key = vm.to_key(&i).ok()?;
        let gsc_t6::Value::Object(chest) = chests.snapshot().get(&key).cloned()? else { return None };
        let of = vm.intern("origin");
        match vm.get_field(world, chest, of) {
            gsc_t6::Value::Vec3(v) => Some(v),
            _ => None,
        }
    })
    .flatten()
}

/// Player 0's points and weapons.
fn player_line(ecs: &mut bevy_ecs::prelude::World) -> String {
    super::with_vm(ecs, |vm, world| {
        let p = world.resource::<super::Zm>().players.get(&0).map(|p| gsc_t6::Value::Object(p.obj))?;
        let gsc_t6::Value::Object(o) = p else { return None };
        let f = vm.intern("score");
        let score = vm.get_field(world, o, f);
        let list = vm.native_of("getweaponslist", true).and_then(|g| g(vm, world, &p, &[]).ok());
        let cur = vm.native_of("getcurrentweapon", true).and_then(|g| g(vm, world, &p, &[]).ok());
        let text = |v: &Option<gsc_t6::Value>, vm: &gsc_t6::Vm<bevy_ecs::prelude::World>| match v {
            Some(gsc_t6::Value::Array(a)) => a.snapshot().values_in_order().map(|x| vm.to_text(x)).collect::<Vec<_>>().join(" "),
            Some(x) => vm.to_text(x),
            None => String::new(),
        };
        // (And the perks he holds: `perks_active`.)
        let pf = super::t7::field_name(vm, "perks_active");
        let perks = match vm.raw_field(o, pf) {
            gsc_t6::Value::Array(a) => a.snapshot().values_in_order().map(|x| vm.to_text(x)).collect::<Vec<_>>().join(" "),
            _ => String::new(),
        };
        Some(format!("points {}, weapon {}, weapons [{}], perks [{perks}]", vm.to_text(&score), text(&cur, vm), text(&list, vm)))
    })
    .flatten()
    .unwrap_or_else(String::new)
}

/// The power-ups on the ground now (entity, name, where), logged as they
/// come and go; `T7_GRAB` puts player 0 on each a second after it came.
fn powerups_seen(ecs: &mut bevy_ecs::prelude::World, t: u32, seen: &mut std::collections::HashMap<u32, u32>) {
    let now: Vec<(u32, String, [f32; 3])> = super::with_vm(ecs, |vm, world| {
        let f = vm.intern("powerup_name");
        let zm = world.resource::<super::Zm>();
        let ents: Vec<(u32, gsc_t6::ObjRef, [f32; 3])> = zm.ents.iter().filter_map(|(n, e)| e.obj.map(|o| (*n, o, e.origin))).collect();
        ents.into_iter()
            .filter_map(|(n, o, at)| match vm.raw_field(o, f) {
                gsc_t6::Value::Undefined => None,
                v => Some((n, vm.to_text(&v), at)),
            })
            .collect()
    })
    .unwrap_or_default();
    for (n, name, at) in &now {
        let first = *seen.entry(*n).or_insert_with(|| {
            eprintln!("t7 powerup at {} s: ent{n} {name} at {at:?}", t / 20);
            t
        });
        if std::env::var_os("T7_GRAB").is_some() && t == first + 20 {
            eprintln!("t7 powerup: player 0 walks onto ent{n} {name}");
            super::teleport_player(ecs, crate::world::ClientId(0), *at);
        }
    }
    seen.retain(|n, _| {
        let here = now.iter().any(|(m, _, _)| m == n);
        if !here {
            eprintln!("t7 powerup at {} s: ent{n} gone", t / 20);
        }
        here
    });
}

/// Shoot (player 0, a pistol, the head) each living zombie `age` frames
/// after it was first seen.
fn kill_old(ecs: &mut bevy_ecs::prelude::World, now: u32, age: u32, born: &mut std::collections::HashMap<u32, u32>) {
    let alive: Vec<u32> = ecs.resource::<super::Zm>().actors.iter().filter(|(_, a)| a.alive && a.t7).map(|(n, _)| *n).collect();
    let Some(player) = ecs.resource::<super::Zm>().players.get(&0).map(|p| gsc_t6::Value::Object(p.obj)) else { return };
    for n in alive {
        let first = *born.entry(n).or_insert(now);
        if now - first < age {
            continue;
        }
        let mut at = ecs.resource::<super::Zm>().ents.get(&n).map_or([0.0; 3], |e| e.origin);
        // (T7_KILL_AT="x y z r": only those within r of that point, as a
        // soul catcher or a dragon wants its kills near it; "x y z r pull":
        // each put there first, for a volume the zombies do not walk into
        // while he stands still.)
        // ("…@a-b;…": several, each for those seconds.)
        let spot = std::env::var("T7_KILL_AT").ok().and_then(|v| {
            v.split(';')
                .find(|e| match e.split_once('@').and_then(|(_, w)| w.split_once('-')) {
                    Some((a, b)) => a.trim().parse::<u32>().is_ok_and(|a| now >= a * 20) && b.trim().parse::<u32>().is_ok_and(|b| now <= b * 20),
                    None => !e.contains('@'),
                })
                .map(|e| e.split('@').next().unwrap_or("").split_whitespace().map(str::to_string).collect::<Vec<_>>())
        });
        if std::env::var_os("T7_KILL_AT").is_some() && spot.is_none() {
            continue;
        }
        if let Some(k) = spot.filter(|k| k.len() >= 4) {
            let f: Vec<f32> = k.iter().take(4).filter_map(|x| x.parse::<f32>().ok()).collect();
            if f.len() == 4 && k.get(4).is_some_and(|w| w == "pull") {
                at = [f[0], f[1], f[2]];
                if let Some(e) = ecs.resource_mut::<super::Zm>().ents.get_mut(&n) {
                    e.origin = at;
                }
            } else if f.len() == 4 && (at[0] - f[0]).powi(2) + (at[1] - f[1]).powi(2) + (at[2] - f[2]).powi(2) > f[3] * f[3] {
                continue;
            }
        }
        // (A Margwa is shot in its heads, each, as they open: 600 a hit.)
        let heads: Vec<[f32; 3]> = ["j_chunk_head_bone", "j_chunk_head_bone_le", "j_chunk_head_bone_ri"].iter().filter_map(|t| super::t7_ai::tag_guess(ecs, n, t)).collect();
        if !heads.is_empty() {
            for h in heads {
                super::with_vm(ecs, |vm, world| {
                    super::actors::damage(vm, world, n, player.clone(), player.clone(), 600, 0, "MOD_RIFLE_BULLET", "ar_standard", h, [1.0, 0.0, 0.0], "head");
                });
            }
            continue;
        }
        super::with_vm(ecs, |vm, world| {
            super::actors::damage(vm, world, n, player.clone(), player.clone(), 10_000, 0, "MOD_PISTOL_BULLET", "pistol_standard", [at[0], at[1], at[2] + 64.0], [1.0, 0.0, 0.0], "head");
        });
    }
    // (And the vehicles: the parasites of their rounds.)
    let vehicles: Vec<(u32, gsc_t6::ObjRef, [f32; 3])> = ecs
        .get_resource::<super::t7_ai::T7Ai>()
        .map(|s| s.vehicles.iter().copied().collect::<Vec<_>>())
        .unwrap_or_else(Vec::new)
        .into_iter()
        .filter_map(|n| ecs.resource::<super::Zm>().ents.get(&n).and_then(|e| e.obj.map(|o| (n, o, e.origin))))
        .collect();
    for (n, obj, at) in vehicles {
        let first = *born.entry(n).or_insert(now);
        if now - first < age {
            continue;
        }
        super::with_vm(ecs, |vm, world| {
            super::t7_ai::vehicle_damage(vm, world, obj, player.clone(), player.clone(), 10_000, "MOD_PISTOL_BULLET", "pistol_standard", at, [1.0, 0.0, 0.0]);
        });
    }
}

#[test]
fn shadows_of_evil_level_starts_with_its_own_scripts() {
    let Some((objects, ents)) = inputs() else {
        eprintln!("no T7_SCRIPTS_DIR / T7_ENTS; skipped");
        return;
    };
    // A watchdog: a script that never waits names itself.
    std::thread::spawn(|| {
        std::thread::sleep(std::time::Duration::from_secs(std::env::var("T7_WATCHDOG").ok().and_then(|v| v.parse().ok()).unwrap_or(90)));
        eprintln!("t7 level start hung: {}", gsc_t6::hang_report());
        std::process::exit(2);
    });
    let mut world = SimWorld::new();
    let (models, brushes, leafbrushes) = cmodels();
    let tables = tables();
    let mut content = crate::SimContentBuilder::default();
    if !models.is_empty() {
        let bsp = crate::world::SimClipBsp { leafbrushes, ..Default::default() };
        content.set_clip_map(brushes, bsp, Default::default(), crate::world::SimClipCmodels { models, triggers: trigger_hulls() });
    }
    // The map's weapons (its weapons tables' rows and their upgrades), so
    // players hold, buy and swap Black Ops III's own weapons by name.
    let (names, combat) = weapon_catalog(&tables);
    content.set_weapon_script_names(names);
    content.set_weapon_combat_table(combat);
    world.install_content(content.finish());
    world
        .install_t6(T6Install { map: t7_map(), gametype: "zclassic".to_owned(), t7_objects: objects, entities: vec![ents], tables, t7_nav: nav(), t7_bundles: bundles(), t7_cast: cast().0, anims: cast().1, ..Default::default() })
        .expect("install");
    // (Only the script side of a frame: the IW4 runtime's stub start-up,
    // which the full schedule wants, has no part in it.)
    let ecs = world.ecs_mut();
    let mut born: std::collections::HashMap<u32, u32> = std::collections::HashMap::new();
    let mut most_alive = 0;
    let mut given = false;
    let mut seen_powerups: std::collections::HashMap<u32, u32> = std::collections::HashMap::new();
    let frames: u32 = std::env::var("T7_FRAMES").ok().and_then(|v| v.parse().ok()).unwrap_or(200);
    for t in 1..=frames {
        ecs.insert_resource(crate::step::StepRequest::authority(Tick(t), 50));
        let alive = ecs.resource::<super::Zm>().actors.values().filter(|a| a.alive).count();
        most_alive = most_alive.max(alive);
        // T7_WATCH_ENT=<n>: the first frame entity n is gone.
        if let Some(w) = std::env::var("T7_WATCH_ENT").ok().and_then(|v| v.parse::<u32>().ok()) {
            let here = ecs.resource::<super::Zm>().ents.contains_key(&w);
            eprintln!("t7 watch ent{w} frame {t}: {here}");
        }
        // T7_ANIMS=1: each Black Ops III actor's look and clip every 2 s
        // (its body, attached models, the clip, its place and rate).
        if std::env::var_os("T7_ANIMS").is_some() && t % 40 == 0 {
            let now = ecs.resource::<super::Zm>().now_ms;
            let ids: Vec<u32> = ecs.resource::<super::Zm>().actors.iter().filter(|(_, a)| a.t7).map(|(n, _)| *n).collect();
            for n in ids {
                let (model, attached, alive, at) = {
                    let zm = ecs.resource::<super::Zm>();
                    let e = zm.ents.get(&n);
                    (e.map(|e| e.model.clone()).unwrap_or_default(), e.map(|e| e.attached.iter().map(|a| a.0.clone()).collect::<Vec<_>>()).unwrap_or_default(), zm.actors.get(&n).is_some_and(|a| a.alive), e.map_or([0.0; 3], |e| e.origin))
                };
                let anim = super::actors::present_anim(ecs, n, now);
                eprintln!("t7 anims t{t} ent{n} alive {alive} at ({:.0} {:.0} {:.0}) {model} {attached:?}: {anim:?}", at[0], at[1], at[2]);
            }
        }
        // T7_TRACK=targetname: that entity's origin every 2 s (the train).
        for name in std::env::var("T7_TRACK").unwrap_or_else(|_| String::new()).split(',').filter(|n| !n.is_empty() && t % 40 == 0).map(str::to_owned).collect::<Vec<_>>() {
            if name == "player" {
                let at = super::frame(ecs).player(crate::world::ClientId(0)).map(|ps| ps.origin);
                eprintln!("t7 track player at {} s: {at:?}", t / 20);
            }
            let by_num = name.strip_prefix("ent").and_then(|n| n.parse::<u32>().ok());
            let at = super::with_vm(ecs, |vm, world| {
                if let Some(n) = by_num {
                    return world.resource::<super::Zm>().ents.get(&n).map(|e| (n, e.origin));
                }
                // (Or by its model: `model:veh_t7_zmb_zod_train`.)
                if let Some(m) = name.strip_prefix("model:") {
                    return world.resource::<super::Zm>().ents.iter().find(|(_, e)| e.model == m).map(|(n, e)| (*n, e.origin));
                }
                let tf = vm.intern("targetname");
                let zm = world.resource::<super::Zm>();
                let hits: Vec<(u32, gsc_t6::ObjRef, [f32; 3])> = zm.ents.iter().filter_map(|(n, e)| e.obj.map(|o| (*n, o, e.origin))).collect();
                hits.into_iter().find(|(_, o, _)| {
                    let v = vm.get_field(world, *o, tf);
                    vm.to_text(&v) == name
                })
                .map(|(n, _, p)| (n, p))
            })
            .flatten();
            eprintln!("t7 track {name} at {} s: {at:?}", t / 20);
        }
        if t % 200 == 0 {
            let (round, zombies) = state(ecs);
            let near = ai_distances(ecs).map_or_else(String::new, |(n, m)| format!(" (nearest {n:.0}, mean {m:.0} from player 0)"));
            let at = super::frame(ecs).player(crate::world::ClientId(0)).map_or_else(String::new, |p| format!(", player 0 at {:.0} {:.0} {:.0}", p.origin[0], p.origin[1], p.origin[2]));
            // (And player 0's points: BO3's rewards are the scripts', 10 a hit,
            // 60/100 a torso/head kill, so a run shows the economy too.)
            let points = player_line(ecs).split(',').next().map_or_else(String::new, |p| format!(", {p}"));
            eprintln!("frame {t} ({} s): round {round}, {zombies} AI alive{near}{at}{points}; {}", t / 20, gsc_t6::hang_report());
        }
        // One player joins once the level has started.
        if t == 10 {
            super::frame(ecs).client_meta_mut(crate::world::ClientId(0)).lifecycle = crate::ClientLifecycle::ChoosingClass;
        }
        // T7_KILL=<seconds>: player 0 shoots each zombie that has lived that
        // long (in the head, through the engine's damage callback): the
        // rounds go on.
        if let Some(after) = std::env::var("T7_KILL").ok().and_then(|v| v.parse::<u32>().ok())
            && t % 10 == 0
        {
            kill_old(ecs, t, after * 20, &mut born);
        }
        // Power-ups lying about (a script model with `powerup_name`): each
        // logged when it appears and goes; with T7_GRAB=1 player 0 walks
        // onto it (put there) a second after it lands.
        if t % 10 == 0 {
            powerups_seen(ecs, t, &mut seen_powerups);
        }
        // T7_CALL="script::function@seconds[:arg|arg],...": that script
        // function started on level at that time with string arguments
        // (`_` is undefined): a quest step run without its whole lead-up.
        if let Ok(list) = std::env::var("T7_CALL") {
            for entry in list.split(',') {
                let Some((func, rest)) = entry.split_once('@') else { continue };
                let (when, args) = rest.split_once(':').unwrap_or((rest, ""));
                if when.trim().parse::<f32>().map_or(true, |w| t != (w * 20.0) as u32) {
                    continue;
                }
                // ("player:script::f": run on player 0; an argument "#3"
                // is the integer 3, "p" player 0.)
                let (on_player, func) = func.strip_prefix("player:").map_or((false, func), |f| (true, f));
                let Some((script, name)) = func.rsplit_once("::") else { continue };
                let (script, name, args) = (script.to_owned(), name.to_owned(), args.to_owned());
                let player = ecs.resource::<super::Zm>().players.get(&0).map(|p| p.obj);
                super::with_vm(ecs, |vm, world| {
                    let a: Vec<gsc_t6::Value> = args
                        .split('|')
                        .filter(|x| !x.is_empty())
                        .map(|x| match x {
                            "_" => gsc_t6::Value::Undefined,
                            "p" => player.map_or(gsc_t6::Value::Undefined, gsc_t6::Value::Object),
                            _ => match x.strip_prefix('#').and_then(|n| n.parse::<i32>().ok()) {
                                Some(n) => gsc_t6::Value::Int(n),
                                None => vm.string(x),
                            },
                        })
                        .collect();
                    let lv = match (on_player, player) {
                        (true, Some(p)) => gsc_t6::Value::Object(p),
                        _ => gsc_t6::Value::Object(vm.level),
                    };
                    let ok = vm.spawn_named(world, &script, &name, lv, a).is_some();
                    eprintln!("t7 call {script}::{name}({args}): {}", if ok { "started" } else { "no such function" });
                });
            }
        }
        use_triggers(ecs, t, &mut given, "T7_USE");
        use_triggers(ecs, t, &mut given, "T7_TRIG");
        shoot(ecs, t);
        stub_use(ecs, t);
        // T7_GOD=1: player 0 is healed every frame (he outlives what the
        // run watches; once a second a crowd at round 13 downed him).
        if std::env::var_os("T7_GOD").is_some() {
            let p = ecs.resource::<super::Zm>().players.get(&0).map(|p| p.obj);
            if let Some(p) = p {
                super::with_vm(ecs, |vm, world| {
                    for name in ["maxhealth", "health"] {
                        let f = vm.intern(name);
                        vm.set_field(world, p, f, gsc_t6::Value::Int(10_000));
                    }
                });
            }
        }
        // T7_ZONES=1: at 15 s every zone is enabled (`zm_zonemgr::enable_zone`
        // for each of `level.zones`), as if every door were open: the box,
        // perks and the rest anywhere on the map can be used.
        if t == 300 && std::env::var_os("T7_ZONES").is_some() {
            super::with_vm(ecs, |vm, world| {
                let func = (0..vm.program.functions.len()).find(|&i| vm.str(vm.program.functions[i].name) == "enable_zone");
                let z = vm.intern("zones");
                let (Some(func), gsc_t6::Value::Array(zones)) = (func, vm.raw_field(vm.level, z)) else { return };
                let names: Vec<gsc_t6::Value> = zones.snapshot().keys().map(|k| k.value()).collect();
                eprintln!("t7 zones: enabling {}", names.len());
                for n in names {
                    vm.spawn(world, func as u32, gsc_t6::Value::Undefined, vec![n]);
                }
            });
        }
        // T7_SETFLAGS="a,b": those level flags set at 15 s (`flag::set`), as
        // the doors and events that set them would.
        if t == 300 {
            for name in std::env::var("T7_SETFLAGS").unwrap_or_else(|_| String::new()).split(',').filter(|n| !n.is_empty()) {
                super::with_vm(ecs, |vm, world| {
                    let v = vm.string(name);
                    vm.spawn_named(world, "scripts/shared/flag_shared", "set", gsc_t6::Value::Object(vm.level), vec![v]);
                });
            }
        }
        // T7_PEEK="levelfield.field,...": those fields of level objects
        // every 10 s (a class instance's members).
        // T7_WAITERS="function[@seconds]": the entities that function's
        // threads wait on then (number, class, place, script_noteworthy).
        // T7_DUMP_AT="secs,...": every thread's wait then.
        if std::env::var("T7_DUMP_AT").is_ok_and(|v| v.split(',').any(|s| s.trim().parse::<f32>().is_ok_and(|s| t == (s * 20.0) as u32))) {
            super::with_vm(ecs, |vm, _| {
                for l in vm.describe_threads() {
                    eprintln!("thread@{} {l}", t / 20);
                }
            });
        }
        if let Ok(spec) = std::env::var("T7_WAITERS") {
            let (func, at) = spec.split_once('@').map_or((spec.as_str(), 30.0), |(f, s)| (f, s.parse::<f32>().unwrap_or(30.0)));
            if t == (at * 20.0) as u32 {
                let mut k = 0;
                loop {
                    let p = point_of(ecs, &format!("waiter:{func}#{k}"));
                    if p.len() != 3 {
                        break;
                    }
                    let line = super::with_vm(ecs, |vm, world| {
                        let zm = world.resource::<super::Zm>();
                        let found = zm.ents.iter().find(|(_, e)| e.origin.to_vec() == p).map(|(n, e)| (*n, e.classname.clone(), e.obj));
                        let Some((n, class, obj)) = found else { return String::new() };
                        let note = obj.map(|o| { let f = vm.intern("script_noteworthy"); vm.to_text(&vm.raw_field(o, f)) }).unwrap_or_default();
                        format!("ent{n} {class} at {p:?} {note}")
                    })
                    .unwrap_or_default();
                    eprintln!("t7 waiter {func}#{k}: {line}");
                    k += 1;
                }
            }
        }
        // T7_SETPLAYER="field=int,...@seconds": player 0's script fields
        // then (a carried item's state the scripts check: Zetsubou's
        // bucket and its water, `#6fd3d65c`/`#bb2fd41c`).
        if let Ok(spec) = std::env::var("T7_SETPLAYER") {
            for part in spec.split(';') {
                let (pairs, at) = part.rsplit_once('@').map_or((part, 15.0), |(p, s)| (p, s.parse::<f32>().unwrap_or(15.0)));
                if t != (at * 20.0) as u32 {
                    continue;
                }
                let Some(p) = ecs.resource::<super::Zm>().players.get(&0).map(|p| p.obj) else { continue };
                super::with_vm(ecs, |vm, _| {
                    for pair in pairs.split(',') {
                        let Some((name, v)) = pair.split_once('=') else { continue };
                        let Ok(v) = v.trim().parse::<i32>() else { continue };
                        let f = vm.intern(name.trim());
                        vm.set_raw_field(p, f, gsc_t6::Value::Int(v));
                    }
                });
                eprintln!("t7 setplayer {pairs}");
            }
        }
        // (T7_PEEK_EVERY=<frames>: how often; every 10 s by default.)
        let every = std::env::var("T7_PEEK_EVERY").ok().and_then(|v| v.parse::<u32>().ok()).unwrap_or(200).max(1);
        if t % every == 100 % every {
            for spec in std::env::var("T7_PEEK").unwrap_or_else(|_| String::new()).split(',').filter(|n| !n.is_empty()) {
                // ("field": level.field itself; "field[key]": an entry of
                // the array level.field, e.g. zombie_vars[zombie_drop_item].)
                let (lf, f) = spec.split_once('.').unwrap_or((spec, ""));
                let text = super::with_vm(ecs, |vm, world| {
                    if let Some((a, key)) = lf.strip_suffix(']').and_then(|s| s.split_once('[')) {
                        let k = vm.intern(a);
                        let gsc_t6::Value::Array(arr) = vm.raw_field(vm.level, k) else { return format!("(level.{a} is not an array)") };
                        let key = key.parse::<i32>().map_or_else(|_| gsc_t6::value::Key::Str(vm.intern(key)), gsc_t6::value::Key::Int);
                        let v = arr.get(&key).unwrap_or(gsc_t6::Value::Undefined);
                        // ("players[0].score": a field of that entry.)
                        return match (&v, f.is_empty()) {
                            (gsc_t6::Value::Object(o), false) => {
                                let k = vm.intern(f);
                                vm.to_text(&vm.raw_field(*o, k))
                            }
                            _ => vm.to_text(&v),
                        };
                    }
                    let k = vm.intern(lf);
                    if f.is_empty() {
                        let lv = vm.level;
                        let v = vm.get_field(world, lv, k);
                        return vm.to_text(&v);
                    }
                    match vm.raw_field(vm.level, k) {
                        gsc_t6::Value::Object(o) => {
                            let k = vm.intern(f);
                            vm.to_text(&vm.raw_field(o, k))
                        }
                        other => format!("(level.{lf} is {})", vm.to_text(&other)),
                    }
                })
                .unwrap_or_else(String::new);
                eprintln!("t7 peek {spec} at {} s: {text}", t / 20);
            }
        }
        // T7_SETLEVEL="field=int,...": level fields set at 15 s (or frame
        // T7_SETLEVEL_AT; a field
        // the translation left hashed is named `#hash`).
        if t == std::env::var("T7_SETLEVEL_AT").ok().and_then(|v| v.parse().ok()).unwrap_or(300) {
            for pair in std::env::var("T7_SETLEVEL").unwrap_or_else(|_| String::new()).split(',') {
                let Some((name, v)) = pair.split_once('=') else { continue };
                let Ok(v) = v.trim().parse::<i32>() else { continue };
                super::with_vm(ecs, |vm, _| {
                    let f = vm.intern(name.trim());
                    vm.set_raw_field(vm.level, f, gsc_t6::Value::Int(v));
                });
            }
        }
        super::advance(ecs);
        // (The weapon tick does not run here: a switch the scripts asked
        // for is made at once, raised at once, with the engine's notifies.)
        let id = crate::world::ClientId(0);
        let to = {
            let mut f = super::frame(ecs);
            match f.player(id).is_some() {
                true => std::mem::take(&mut f.client_meta_mut(id).controls.switch_to),
                false => 0,
            }
        };
        // (Empty-handed, the engine raises his first gun.)
        let to = match to {
            0 => {
                let f = super::frame(ecs);
                let primaries = crate::script_player::weapons(&f, id, crate::script_player::WeaponList::Primaries);
                f.player(id).filter(|ps| ps.weapon == 0).and(primaries.first().copied()).unwrap_or(0)
            }
            to => to,
        };
        if to != 0 {
            if let Some(ps) = super::frame(ecs).player_mut(id) {
                ps.weapon = to as _;
            }
            let obj = ecs.resource::<super::Zm>().players.get(&0).map(|p| p.obj);
            let name = super::weapon_text(ecs, to);
            if let Some(obj) = obj {
                super::with_vm(ecs, |vm, world| {
                    let w = super::t7::weapon_object(vm, world, &name);
                    for e in ["weapon_change", "weapon_change_complete"] {
                        vm.notify_str(world, obj, e, &[w.clone()]);
                    }
                });
            }
        }
        ecs.remove_resource::<crate::step::StepRequest>();
    }
    let (round, zombies) = state(ecs);
    eprintln!("after {frames} frames: round {round}, {zombies} AI alive");
    if std::env::var_os("T7_DUMP_CALLBACKS").is_some() {
        super::with_vm(ecs, |vm, _| {
            let f = vm.intern("_callbacks");
            if let gsc_t6::Value::Array(a) = vm.raw_field(vm.level, f) {
                let a = a.snapshot();
                let mut k = a.first_key();
                while let Some(key) = k {
                    let n = match a.get(&key) {
                        Some(gsc_t6::Value::Array(l)) => l.len(),
                        _ => 0,
                    };
                    eprintln!("level._callbacks[{}]: {n}", vm.to_text(&key.value()));
                    k = a.next_key(&key);
                }
            }
        });
    }
    if std::env::var_os("T7_DUMP_AI").is_some() {
        let ids: Vec<(u32, Option<[f32; 3]>, Option<gsc_t6::ObjRef>)> = ecs.resource::<super::Zm>().actors.iter().filter(|(_, a)| a.alive).map(|(n, a)| (*n, a.goal, a.obj)).collect();
        for (n, goal, obj) in ids {
            let at = ecs.resource::<super::Zm>().ents.get(&n).map(|e| e.origin);
            let ai = ecs.get_resource::<super::t7_ai::T7Ai>().and_then(|s| s.ai.get(&n).cloned());
            let fields = super::with_vm(ecs, |vm, _| {
                let Some(o) = obj else { return String::new() };
                ["archetype", "#44a68a57", "#67079016", "favoriteenemy", "zombie_move_speed", "#003e8a33", "#8bcf59e8", "#4001c9c9", "#eb058044", "goalradius", "ai_state"]
                    .iter()
                    .map(|f| {
                        let k = vm.intern(f);
                        format!("{f}={}", vm.to_text(&vm.raw_field(o, k)))
                    })
                    .collect::<Vec<_>>()
                    .join(" ")
            })
            .unwrap_or_else(String::new);
            eprintln!(
                "ai ent{n} at {at:?} goal {goal:?} action {:?} path {} traverse {} | {fields}",
                ai.as_ref().and_then(|a| a.action.clone()),
                ai.as_ref().map_or(0, |a| a.path.len()),
                ai.as_ref().is_some_and(|a| a.traverse.is_some())
            );
        }
    }
    // (A Margwa's heads: each one's health and whether it is open.)
    if std::env::var_os("T7_DUMP_AI").is_some() {
        let ids: Vec<(u32, Option<gsc_t6::ObjRef>)> = ecs.resource::<super::Zm>().actors.iter().filter(|(_, a)| a.alive && a.t7).map(|(n, a)| (*n, a.obj)).collect();
        for (n, obj) in ids {
            let Some(o) = obj else { continue };
            let line = super::with_vm(ecs, |vm, _| {
                let h = vm.intern("head");
                let gsc_t6::Value::Array(heads) = vm.raw_field(o, h) else { return None };
                let (hf, of, tf) = (vm.intern("health"), vm.intern("#67079016"), vm.intern("tag"));
                let parts: Vec<String> = heads
                    .snapshot()
                    .values_in_order()
                    .filter_map(|v| match v {
                        gsc_t6::Value::Object(ho) => Some(format!("{} health {} open {}", vm.to_text(&vm.raw_field(*ho, tf)), vm.to_text(&vm.raw_field(*ho, hf)), vm.to_text(&vm.raw_field(*ho, of)))),
                        _ => None,
                    })
                    .collect();
                Some(parts.join("; "))
            })
            .flatten();
            if let Some(line) = line {
                eprintln!("heads ent{n}: {line}");
            }
        }
    }
    // (And the vehicles: where, headed where, after whom.)
    if std::env::var_os("T7_DUMP_AI").is_some() {
        let ids: Vec<u32> = ecs.get_resource::<super::t7_ai::T7Ai>().map_or_else(Vec::new, |s| s.vehicles.iter().copied().collect());
        for n in ids {
            let (at, obj) = ecs.resource::<super::Zm>().ents.get(&n).map_or(([0.0; 3], None), |e| (e.origin, e.obj));
            let flight = super::flight::describe(ecs, n).unwrap_or_else(String::new);
            let fields = super::with_vm(ecs, |vm, _| {
                let Some(o) = obj else { return String::new() };
                ["#51482eca", "health", "vehicletype", "scriptbundlesettings", "ignoreall"]
                    .iter()
                    .map(|f| {
                        let k = vm.intern(f);
                        format!("{f}={}", vm.to_text(&vm.raw_field(o, k)))
                    })
                    .collect::<Vec<_>>()
                    .join(" ")
            })
            .unwrap_or_else(String::new);
            eprintln!("vehicle ent{n} at {at:?} {flight} | {fields}");
        }
    }
    // T7_DUMP_STUBS: the unitrigger stubs (`level._unitriggers.trigger_stubs`)
    // and their type and test radius.
    if std::env::var_os("T7_DUMP_STUBS").is_some() {
        super::with_vm(ecs, |vm, world| {
            let u = vm.intern("_unitriggers");
            let gsc_t6::Value::Object(uo) = vm.raw_field(vm.level, u) else { return };
            let ts = vm.intern("trigger_stubs");
            let gsc_t6::Value::Array(stubs) = vm.raw_field(uo, ts) else { return };
            let stubs: Vec<gsc_t6::Value> = stubs.snapshot().values_in_order().cloned().collect();
            eprintln!("unitrigger stubs: {}", stubs.len());
            let ds = vm.intern("dynamic_stubs");
            if let gsc_t6::Value::Array(d) = vm.raw_field(uo, ds) {
                let of = vm.intern("origin");
                let list: Vec<String> = d.snapshot().values_in_order().filter_map(|v| v.as_obj()).map(|o| vm.to_text(&vm.raw_field(o, of))).collect();
                eprintln!("dynamic stubs: {}", list.join(" "));
            }
            for st in stubs {
                let gsc_t6::Value::Object(o) = st else { continue };
                let line: Vec<String> = ["script_unitrigger_type", "test_radius_sq", "origin", "targetname", "script_noteworthy", "prompt_and_visibility_func"]
                    .iter()
                    .map(|f| {
                        let k = vm.intern(f);
                        let v = vm.get_field(world, o, k);
                        format!("{f}={}", vm.to_text(&v))
                    })
                    .collect();
                eprintln!("stub {}", line.join(" "));
                // T7_STUBKEYS="x y" (2D): every field of the stubs within 80
                // of it, and of their `piece`.
                if let Some(p) = std::env::var("T7_STUBKEYS").ok().map(|v| v.split_whitespace().filter_map(|x| x.parse::<f32>().ok()).collect::<Vec<_>>())
                    && p.len() >= 2
                {
                    let of = vm.intern("origin");
                    if let Some(q) = vm.get_field(world, o, of).as_vec3()
                        && (q[0] - p[0]).abs() < 80.0
                        && (q[1] - p[1]).abs() < 80.0
                    {
                        for (label, obj) in [("stub", Some(o)), ("piece", { let k = vm.intern("piece"); vm.raw_field(o, k).as_obj() })] {
                            let Some(obj) = obj else { continue };
                            let kv: Vec<String> = vm.field_names(obj).into_iter().map(|k| { let v = vm.raw_field(obj, k); format!("{}={}", vm.str(k), vm.to_text(&v)) }).collect();
                            eprintln!("  {label} fields: {}", kv.join(" "));
                        }
                    }
                }
            }
        });
    }
    // T7_MODELS="part,...": the entities whose model holds one of those
    // at the end (where the scripts moved a quest piece or a pod).
    if let Ok(want) = std::env::var("T7_MODELS") {
        let zm = ecs.resource::<super::Zm>();
        let mut hits: Vec<(&u32, &super::Ent)> = zm.ents.iter().filter(|(_, e)| want.split(',').any(|w| !w.is_empty() && e.model.contains(w))).collect();
        hits.sort_by_key(|(n, _)| **n);
        for (n, e) in hits {
            eprintln!("t7 model: ent{n} {} {} at {:?}", e.classname, e.model, e.origin);
        }
    }
    // T7_NEAR="x y z": the entities within 300 of that point at the end
    // (where a trigger the scripts spawned stands).
    if let Some(p) = std::env::var("T7_NEAR").ok().map(|v| v.split_whitespace().filter_map(|x| x.parse::<f32>().ok()).collect::<Vec<_>>()).filter(|p| p.len() == 3) {
        let zm = ecs.resource::<super::Zm>();
        for (n, e) in &zm.ents {
            let d = ((e.origin[0] - p[0]).powi(2) + (e.origin[1] - p[1]).powi(2) + (e.origin[2] - p[2]).powi(2)).sqrt();
            if d < 300.0 {
                eprintln!("t7 near: ent{n} {} at {:?} ({d:.0}) model {}", e.classname, e.origin, e.model);
            }
        }
    }
    if std::env::var_os("T7_DUMP_THREADS").is_some() {
        super::with_vm(ecs, |vm, _| {
            for t in vm.describe_threads() {
                eprintln!("thread {t}");
            }
        });
    }
    // With the nav mesh, zombies come in round 1 (by 30 s); shot as they
    // come (`T7_KILL`), the rounds go on.
    if std::env::var_os("T7_NAV").is_some() && frames >= 600 {
        assert!(most_alive > 0, "no zombie spawned");
    }
    if std::env::var_os("T7_KILL").is_some() && frames >= 3000 {
        let round = super::with_vm(ecs, |vm, _| {
            let f = vm.intern("round_number");
            vm.raw_field(vm.level, f).as_int()
        })
        .flatten()
        .unwrap_or(0);
        assert!(round >= 2, "still round {round} after {frames} frames");
    }
    let zm = ecs.resource::<super::Zm>();
    assert!(zm.started, "the level started");
    let mut unbound: Vec<(&String, &u64)> = zm.unbound.iter().collect();
    unbound.sort_by(|a, b| b.1.cmp(a.1));
    eprintln!("unbound builtins called ({}): {:?}", unbound.len(), &unbound[..unbound.len().min(80)]);
    if let Some(line) = super::t7_client::report(ecs) {
        eprintln!("{line}");
    }
    let (threads, messages) = super::with_vm(ecs, |vm, _| (vm.thread_count(), vm.messages.clone())).expect("vm");
    eprintln!("{threads} threads; {} messages: {:#?}", messages.len(), &messages[..messages.len().min(60)]);
}
