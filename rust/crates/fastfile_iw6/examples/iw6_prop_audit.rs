//! Counts the props a Ghosts map zone places against the ones the game
//! draws: `iw6_prop_audit <mp_map.ff>...` (`IW6_AUDIT_TOP=n` names shown).
use fastfile_iw6::map::{extra_props, model_has_geometry};
use std::collections::BTreeMap;

fn top(label: &str, m: &BTreeMap<String, usize>, n: usize) {
    let mut v: Vec<_> = m.iter().collect();
    v.sort_by(|a, b| b.1.cmp(a.1));
    for (k, c) in v.into_iter().take(n) {
        println!("    {label} x{c} {k}");
    }
}

fn main() {
    let n: usize = std::env::var("IW6_AUDIT_TOP")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(12);
    for path in std::env::args().skip(1) {
        let Ok(zone) = fastfile_iw6::Zone::open(std::path::Path::new(&path)) else {
            println!("{path}: open failed");
            continue;
        };
        let (cap, _) = fastfile_iw6::walk::walk_zone(&zone);
        if std::env::var_os("IW6_AUDIT_MODELS").is_some() {
            for (i, m) in cap.xmodels.iter().enumerate() {
                println!(
                    "MODEL {} {}",
                    if model_has_geometry(&cap, i as u32) {
                        "geo"
                    } else {
                        "ref"
                    },
                    m.name
                );
            }
            continue;
        }
        {
            let mut by: BTreeMap<String, usize> = BTreeMap::new();
            if let Some(c) = cap.clip_maps.first() {
                for i in &c.scriptables {
                    let def = cap.others.get(i.def.index as usize).map_or("?".into(), |o| o.name.clone());
                    for (_, fx, loop_ms, _) in cap.scriptable_fx.get(&i.def.index).into_iter().flatten() {
                        let n = fx.and_then(|f| cap.fx.get(f.index as usize)).map_or("?".into(), |f| f.name.clone());
                        *by.entry(format!("{def} -> {n} (loop {loop_ms} ms)")).or_default() += 1;
                    }
                }
            }
            println!("  SCRIPTABLE-FX {} placements' first-state effects", by.values().sum::<usize>());
            top("SCRIPTABLE-FX", &by, 20);
        }
        let map = fastfile_iw6::map::read_map(&cap, &zone.image, &zone.assets.script_strings);
        let cut = fastfile_iw6::map::alternate_level_cut(&map.spawns);
        println!("{path}: level cut {cut:?}");
        let name = |m: u32| {
            cap.xmodels
                .get(m as usize)
                .map_or("?".to_owned(), |x| x.name.clone())
        };
        if let Some(w) = &map.world {
            let (mut tot, mut geo, mut above) = (0, 0, 0);
            let mut missing = BTreeMap::new();
            for s in &w.static_models {
                let Some(m) = s.model else { continue };
                tot += 1;
                if cut.is_some_and(|c| s.origin[2] > c) {
                    above += 1;
                    continue;
                }
                if model_has_geometry(&cap, m.index) {
                    geo += 1;
                } else {
                    *missing.entry(name(m.index)).or_default() += 1;
                }
            }
            println!(
                "  STATIC {tot}: {geo} with geometry, {above} above the cut, {} without ({} names)",
                tot - geo - above,
                missing.len()
            );
            top("STATIC-NOGEO", &missing, n);
            {
                // Last-LOD distances of the placed models (a finite last
                // distance culls the model beyond it in `render_iw6`).
                let mut seen = std::collections::HashSet::new();
                let mut last: Vec<(f32, usize, String)> = Vec::new();
                for s in &w.static_models {
                    let Some(m) = s.model else { continue };
                    if !seen.insert(m.index) {
                        continue;
                    }
                    if let Some(x) = cap.xmodels.get(m.index as usize) {
                        let d = x.lods.iter().map(|l| l.dist).collect::<Vec<_>>();
                        last.push((d.last().copied().unwrap_or(0.0), d.len(), format!("{} {:?}", x.name, d)));
                    }
                }
                last.sort_by(|a, b| a.0.total_cmp(&b.0));
                let fin = last.iter().filter(|l| l.0 > 0.0).count();
                println!("  LAST-LOD {} models, {} with a finite last distance", last.len(), fin);
                for l in last.iter().filter(|l| l.0 > 0.0).take(8) {
                    println!("    LAST-LOD {}", l.2);
                }
                for l in last.iter().filter(|l| l.0 > 0.0).rev().take(3) {
                    println!("    LAST-LOD-MAX {}", l.2);
                }
            }
            {
                let mut cd: Vec<f32> = w.static_models.iter().map(|s| s.cull_dist).collect();
                cd.sort_by(f32::total_cmp);
                let q = |f: f32| cd.get(((cd.len() as f32 - 1.0) * f) as usize).copied();
                println!(
                    "  CULL-DIST min {:?} p5 {:?} median {:?} p95 {:?} max {:?}; zero {}, under 500: {}",
                    q(0.0),
                    q(0.05),
                    q(0.5),
                    q(0.95),
                    q(1.0),
                    cd.iter().filter(|&&c| c == 0.0).count(),
                    cd.iter().filter(|&&c| c > 0.0 && c < 500.0).count()
                );
            }
            if let Ok(pat) = std::env::var("IW6_AUDIT_GREP") {
                let mut by = BTreeMap::new();
                for s in &w.static_models {
                    if let Some(m) = s.model {
                        let nm = name(m.index);
                        if pat.split(',').any(|p| nm.contains(p)) {
                            *by.entry(nm).or_default() += 1;
                        }
                    }
                }
                top("STATIC-MATCH", &by, 60);
            }
        }
        if let Some(c) = &map.clip {
            let mut miss = BTreeMap::new();
            let mut ok = 0;
            for d in &c.dyn_ents {
                match d.model {
                    Some(m) if model_has_geometry(&cap, m.index) => ok += 1,
                    Some(m) => *miss.entry(name(m.index)).or_default() += 1,
                    None => {
                        *miss
                            .entry(format!("(brush {})", d.brush_model))
                            .or_default() += 1
                    }
                }
            }
            println!("  DYNENT {}: {ok} with geometry", c.dyn_ents.len());
            if let Some(w) = &map.world {
                let brush: Vec<_> = c
                    .dyn_ents
                    .iter()
                    .filter(|d| d.model.is_none())
                    .map(|d| {
                        let b = w.brush_models.get(d.brush_model as usize);
                        (d.brush_model, b.map(|b| (b.mins, b.maxs)), d.quat, d.origin)
                    })
                    .collect();
                println!("  DYNENT brush models (index, (start, count), kind, origin): {brush:?}");
            }
            top("DYNENT-NOGEO", &miss, n);
            let mut miss = BTreeMap::new();
            let mut all = BTreeMap::new();
            let mut ok = 0;
            for s in &c.scriptables {
                *all.entry(s.name.clone()).or_default() += 1;
                match s.model {
                    Some(m) if model_has_geometry(&cap, m.index) => ok += 1,
                    Some(m) => {
                        *miss
                            .entry(format!("{} -> {}", s.name, name(m.index)))
                            .or_default() += 1
                    }
                    None => {
                        *miss
                            .entry(format!("{} -> (no base model)", s.name))
                            .or_default() += 1
                    }
                }
            }
            println!("  SCRIPTABLE {}: {ok} with geometry", c.scriptables.len());
            top("SCRIPTABLE-NOGEO", &miss, n);
            top("SCRIPTABLE", &all, n);
        }
        // Entities that name a model.
        let text = fastfile_iw6::map::named_entity_text(map.entities.trim_end_matches('\0'));
        let have: std::collections::HashSet<String> = cap
            .xmodels
            .iter()
            .enumerate()
            .filter(|(i, _)| model_has_geometry(&cap, *i as u32))
            .map(|(_, m)| m.name.trim_start_matches(',').to_ascii_lowercase())
            .collect();
        let known: std::collections::HashSet<String> = cap
            .xmodels
            .iter()
            .map(|m| m.name.trim_start_matches(',').to_ascii_lowercase())
            .collect();
        let mut by_class: BTreeMap<String, usize> = BTreeMap::new();
        let mut gone: BTreeMap<String, usize> = BTreeMap::new();
        let mut shared: BTreeMap<String, usize> = BTreeMap::new();
        let mut keys: BTreeMap<String, usize> = BTreeMap::new();
        let mut total = 0;
        for e in text.split('{').skip(1) {
            let mut kv = std::collections::HashMap::new();
            for line in e.lines() {
                let mut p = line.split('"').skip(1).step_by(2);
                if let (Some(k), Some(v)) = (p.next(), p.next()) {
                    kv.insert(k.to_owned(), v.to_owned());
                }
            }
            let (Some(class), Some(model)) = (kv.get("classname"), kv.get("model")) else {
                continue;
            };
            if model.starts_with('*') || model.starts_with('?') {
                continue;
            }
            total += 1;
            if std::env::var_os("IW6_AUDIT_ENTS").is_some() {
                let mut v: Vec<_> = kv
                    .iter()
                    .filter(|(k, _)| k.as_str() != "iw6_15695" && k.as_str() != "iw6_20400")
                    .map(|(k, v)| format!("{k}={v}"))
                    .collect();
                v.sort();
                println!("    ENT {}", v.join(" "));
            }
            *by_class.entry(class.clone()).or_default() += 1;
            let lm = model.to_ascii_lowercase();
            if !have.contains(&lm) {
                if known.contains(&lm) {
                    *shared.entry(format!("{class} {model}")).or_default() += 1;
                } else {
                    *gone.entry(format!("{class} {model}")).or_default() += 1;
                }
            }
            if class == "script_model" {
                for k in kv.keys() {
                    if !matches!(
                        k.as_str(),
                        "classname" | "model" | "origin" | "angles" | "targetname"
                    ) {
                        *keys.entry(k.clone()).or_default() += 1;
                    }
                }
            }
        }
        println!("  ENTITIES naming a model: {total} {by_class:?}");
        top("ENT-EMPTY-RECORD", &shared, n);
        top("ENT-NOT-IN-ZONE", &gone, n);
        top("SCRIPT_MODEL-KEY", &keys, 25);
        let mut shown: BTreeMap<&str, usize> = BTreeMap::new();
        let (placed, skipped) = fastfile_iw6::map::extra_props_report(&map, &cap, cut);
        debug_assert_eq!(placed.len(), extra_props(&map, &cap, cut).len());
        for p in &placed {
            *shown.entry(p.source).or_default() += 1;
        }
        println!("  EXTRA placed: {shown:?}");
        let mut why: BTreeMap<String, usize> = BTreeMap::new();
        let mut names: BTreeMap<String, usize> = BTreeMap::new();
        for s in &skipped {
            *why.entry(format!("{} / {}", s.source, s.reason))
                .or_default() += 1;
            if s.reason != "brush model" {
                *names
                    .entry(format!("{} / {}: {}", s.source, s.reason, s.model))
                    .or_default() += 1;
            }
        }
        println!("  EXTRA left out: {}", skipped.len());
        top("LEFT-OUT", &why, 20);
        top("LEFT-OUT-MODEL", &names, n);
    }
}
