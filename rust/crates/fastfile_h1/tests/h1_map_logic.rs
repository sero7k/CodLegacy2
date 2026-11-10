//! bo2mp: Modern Warfare Remastered's map logic on staged maps, headless:
//! `H1_MAP_ZONES=<map.ff>[:...] cargo test -p fastfile_h1 --release --test
//! h1_map_logic -- --ignored --nocapture`.
//!
//! For each map: the spawn points per game type (the `MapEnts` spawn list,
//! which `getspawnarray` reads), what the exe's map-load spawn makes of each
//! entity block (`fastfile_h1::spawn`), every trigger's volume (its `?N`
//! trigger model's hulls) and a player box touching it at its middle and
//! not far off, the kill volumes (`trigger_hurt`, `dmg` now named), the
//! brush models and models the entities place, the dynamic entities.

use std::collections::BTreeMap;

use fastfile_h1::map::{IwMap, TriggerHull};
use fastfile_h1::spawn::{self, H1Spawn};

/// A player's box (`15` each way, `72` up) at `feet` against `hulls`
/// placed at `origin`: the touch test the game's trigger models take (each
/// hull's box, then each of its slabs).
fn player_touches(origin: [f32; 3], hulls: &[TriggerHull], feet: [f32; 3]) -> bool {
    let (mins, maxs) = (
        [feet[0] - 15.0, feet[1] - 15.0, feet[2]],
        [feet[0] + 15.0, feet[1] + 15.0, feet[2] + 72.0],
    );
    let mid: [f32; 3] = std::array::from_fn(|i| (mins[i] + maxs[i]) * 0.5 - origin[i]);
    let half: [f32; 3] = std::array::from_fn(|i| (maxs[i] - mins[i]) * 0.5);
    hulls.iter().any(|h| {
        (0..3).all(|i| (mid[i] - h.mid[i]).abs() <= half[i] + h.half[i])
            && h.slabs.iter().all(|&(dir, at, width)| {
                let reach: f32 = (0..3).map(|i| dir[i].abs() * half[i]).sum();
                let along: f32 = (0..3).map(|i| dir[i] * mid[i]).sum();
                (along - at).abs() <= width + reach
            })
    })
}

fn value<'a>(keys: &'a [(String, String)], k: &str) -> Option<&'a str> {
    keys.iter()
        .find(|(key, _)| key == k)
        .map(|(_, v)| v.as_str())
}

fn vec3(s: &str) -> Option<[f32; 3]> {
    let mut it = s.split_whitespace().map(|x| x.parse::<f32>().ok());
    Some([it.next()??, it.next()??, it.next()??])
}

/// The game types a map's spawn classes serve (`mp_<type>_spawn...`).
fn game_type(class: &str) -> String {
    if class == "mp_global_intermission" || class == "info_player_start" {
        return "(intermission / start)".to_owned();
    }
    class
        .strip_prefix("mp_")
        .and_then(|c| c.split('_').next())
        .unwrap_or(class)
        .to_owned()
}

#[test]
#[ignore]
fn h1_map_logic() {
    let Some(paths) = std::env::var_os("H1_MAP_ZONES") else {
        eprintln!("H1_MAP_ZONES not set");
        return;
    };
    for path in std::env::split_paths(&paths) {
        let zone = fastfile_h1::Zone::open(&path).expect("zone");
        let (map, cap) = IwMap::load(&zone);
        let name = path
            .file_stem()
            .map_or_else(String::new, |s| s.to_string_lossy().into_owned());
        println!("== {name}");

        // Spawn points per game type (the spawn list).
        let mut by_type: BTreeMap<String, BTreeMap<String, usize>> = BTreeMap::new();
        for s in &map.spawns {
            *by_type
                .entry(game_type(&s.classname))
                .or_default()
                .entry(s.classname.clone())
                .or_default() += 1;
        }
        for (t, classes) in &by_type {
            let total: usize = classes.values().sum();
            println!("  spawns {t}: {total} {classes:?}");
        }
        assert!(
            map.spawns
                .iter()
                .filter(|s| s.classname == "mp_tdm_spawn")
                .count()
                > 0,
            "{name}: TDM spawns"
        );
        assert!(
            map.spawns
                .iter()
                .filter(|s| s.classname == "mp_dm_spawn")
                .count()
                > 0,
            "{name}: FFA spawns"
        );

        let text = map.entity_string();
        let blocks = spawn::blocks(&text);
        assert_eq!(
            blocks.first().and_then(|(_, k)| value(k, "classname")),
            Some("worldspawn"),
            "{name}: worldspawn first"
        );

        // What the map-load spawn makes of each block.
        let mut kinds: BTreeMap<String, usize> = BTreeMap::new();
        for (_, keys) in &blocks {
            let k = spawn::spawn_class(value(keys, "classname").unwrap_or(""), true);
            let label = match k {
                H1Spawn::Entity(t) => format!("entity {t}"),
                other => format!("{other:?}"),
            };
            *kinds.entry(label).or_default() += 1;
        }
        println!("  map-load spawn: {kinds:?}");
        let (_, logic) = spawn::as_spawned(&text, true);
        println!("  {}", logic.line());

        // Triggers: their `?N` volume, a player touching it at its middle and
        // not 50000 units off.
        let mut triggers: BTreeMap<String, (usize, usize)> = BTreeMap::new();
        for (_, keys) in &blocks {
            let class = value(keys, "classname").unwrap_or("");
            let Some(table) = spawn::table_class(class) else {
                continue;
            };
            if !table.starts_with("trigger_") {
                continue;
            }
            let origin = value(keys, "origin").and_then(vec3).unwrap_or([0.0; 3]);
            let label = format!("{class} {}", value(keys, "targetname").unwrap_or("-"));
            let entry = triggers.entry(label.clone()).or_default();
            entry.0 += 1;
            let touched = if let Some(n) = value(keys, "model")
                .and_then(|m| m.strip_prefix('?'))
                .and_then(|n| n.parse::<usize>().ok())
            {
                let hulls = map
                    .trigger_models
                    .get(n)
                    .expect("trigger model index in range");
                assert!(!hulls.is_empty(), "{name}: {label} ?{n} has hulls");
                let h = &hulls[0];
                let centre: [f32; 3] = std::array::from_fn(|i| origin[i] + h.mid[i]);
                let feet = [centre[0], centre[1], centre[2] - 36.0];
                let far = [feet[0] + 50_000.0, feet[1], feet[2]];
                let lo: [f32; 3] = std::array::from_fn(|i| origin[i] + h.mid[i] - h.half[i]);
                let hi: [f32; 3] = std::array::from_fn(|i| origin[i] + h.mid[i] + h.half[i]);
                if class == "trigger_hurt"
                    || value(keys, "targetname").is_some_and(|t| t.starts_with("out_of_bounds"))
                {
                    println!(
                        "    {label} ?{n}: {} hulls, first {lo:?}..{hi:?}, dmg {:?}",
                        hulls.len(),
                        value(keys, "dmg")
                    );
                }
                // (A hull's slabs can cut its box's middle out: the touch is
                // then looked for at each slab-kept corner instead.)
                player_touches(origin, hulls, feet)
                    || [lo, hi]
                        .iter()
                        .any(|c| player_touches(origin, hulls, [c[0], c[1], c[2] - 36.0]))
                        && !player_touches(origin, hulls, far)
            } else if table == "trigger_radius" {
                let r: f32 = value(keys, "radius")
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(0.0);
                let h: f32 = value(keys, "height")
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(0.0);
                let s = spawn::radius_shape(r, h);
                assert!(r > 0.0 && h > 0.0, "{name}: {label} radius and height");
                s.half[2] > 0.0
            } else {
                false
            };
            entry.1 += usize::from(touched);
        }
        for (label, (n, touched)) in &triggers {
            println!("  trigger {label}: {n}, {touched} touched by a player at its middle");
            assert_eq!(n, touched, "{name}: {label}");
        }

        // Kill volumes: `dmg` named and lethal in one touch.
        for (_, keys) in blocks
            .iter()
            .filter(|(_, k)| value(k, "classname") == Some("trigger_hurt"))
        {
            let dmg: i32 = value(keys, "dmg").and_then(|v| v.parse().ok()).unwrap_or(0);
            let flags: i32 = value(keys, "spawnflags")
                .and_then(|v| v.parse().ok())
                .unwrap_or(0);
            println!(
                "  trigger_hurt: dmg {dmg} -> {} per touch every {} ms, starts {}",
                spawn::hurt_damage(dmg),
                spawn::hurt_interval_ms(flags),
                if flags & spawn::HURT_START_OFF != 0 {
                    "off"
                } else {
                    "on"
                }
            );
            assert!(spawn::hurt_damage(dmg) >= 100, "{name}: a kill volume");
        }
        assert!(!text.contains("\"h1_293\""), "{name}: dmg is named");

        // Placed brush models and models.
        let cmodels = map.clip.as_ref().map_or(0, |c| c.cmodels.len());
        let (mut brush, mut models) = (0usize, 0usize);
        for (_, keys) in &blocks {
            let class = value(keys, "classname").unwrap_or("");
            match value(keys, "model") {
                Some(m) if m.starts_with('*') => {
                    let n: usize = m[1..].parse().expect("brush model number");
                    assert!(n < cmodels, "{name}: {class} {m} of {cmodels}");
                    brush += 1;
                }
                Some(m) if class == "script_model" => {
                    assert!(!m.is_empty());
                    models += 1;
                }
                _ => {}
            }
        }
        let placed = fastfile_h1::map::placed_brush_models(&map, None).len();
        let dyn_kinds = map.clip.as_ref().map(|c| {
            let mut k: BTreeMap<i32, usize> = BTreeMap::new();
            for d in &c.dyn_ents {
                *k.entry(d.kind).or_default() += 1;
            }
            k
        });
        println!(
            "  placed: {brush} brush model entities ({cmodels} clip models, {placed} drawn), {models} script models; dynamic entities by kind (1 clutter, 2 destructible): {dyn_kinds:?}"
        );
        println!(
            "  compiled scripts in the zone: {:?}",
            cap.script_files
                .iter()
                .map(|s| s.name.clone())
                .collect::<Vec<_>>()
        );
    }
}
