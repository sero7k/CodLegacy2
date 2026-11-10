//! bo2mp: a Modern Warfare Remastered map's ambient effects (rain, mist,
//! dust, lights and fog banks: the effects its createfx script places)
//! played in the match, for retail maps and Horizon MW usermaps alike.
//!
//! The placements come from the map's source scripts
//! (`fastfile_h1::createfx`: `maps/createfx/<map>_fx.gsc` and the
//! `loadfx` table of `maps/mp/<map>_fx.gsc`, in the map zone for a usermap
//! and in `patch_<map>.ff` for a retail map); the effects they name come
//! from the map zone. Each goes through the Advanced Warfare path as the
//! guns' effects do (`super::hmw::fx_view`), with the sprites' materials
//! linked into the map's material catalog. Models are left out (the map's
//! model catalog is not the effects'); the effect's other parts play.
//!
//! Exploders (script-triggered) and sounds are not played. The lane hands
//! the result over as Ghosts' createfx (`super::iw6_fx::set_pending`).
//! `BO2MP_MAP_FX_H1=0` turns it off.

use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};

use asset_audio::CreateFxOneshot;
use asset_core::AssetNamespace;
use asset_game::{FxModelCatalog, S1FxVisual};
use fastfile_h1::createfx::Kind;
use fastfile_h1::walk::FxRec;

use super::h1::H1Builder;
use super::iw6_fx::{GuestMapFx, map_fx_on, set_pending};

/// The raw files the map's effects need.
pub(super) fn wanted_raws(lower: &str) -> [String; 2] {
    [
        format!("maps/createfx/{lower}_fx.gsc"),
        format!("maps/mp/{lower}_fx.gsc"),
    ]
}

fn children(f: &FxRec) -> Vec<String> {
    f.elems
        .iter()
        .flat_map(|el| {
            el.effects
                .iter()
                .map(|n| n.trim_start_matches(',').to_owned())
        })
        .filter(|n| !n.is_empty())
        .collect()
}

/// The repeat time of a placement: none for an effect that loops by itself,
/// else Ghosts' rule on the createfx delay.
fn repeat_secs(loops: bool, delay: f32) -> f32 {
    if loops {
        0.0
    } else {
        asset_audio::ghosts_repeat_secs(delay)
    }
}

/// Reads the map's createfx and converts the effects it places; the result
/// waits for the rules loader (`iw6_fx::take_pending`). Report lines say
/// what played and what did not.
pub(super) fn prepare(
    b: &mut H1Builder<'_>,
    raws: &fastfile_h1::map::PatchRaws,
    lower: &str,
    map_path: &std::path::Path,
    zone_dir: Option<&std::path::Path>,
    report: &mut Vec<String>,
) {
    set_pending(None);
    asset_audio::set_h1_ambience(None);
    if !map_fx_on("BO2MP_MAP_FX_H1") {
        report.push("h1 createfx: off (BO2MP_MAP_FX_H1=0)".to_owned());
        return;
    }
    let (cap, image) = (b.cap, b.image);
    let [createfx_name, fx_name] = wanted_raws(lower);
    let text = |n: &str| {
        raws.get(cap, image, n)
            .map(|b| String::from_utf8_lossy(&b).into_owned())
    };
    let Some(createfx) = text(&createfx_name) else {
        report.push(format!(
            "h1 createfx: no {createfx_name} (map zone or patch zone)"
        ));
        return;
    };
    let fx = fastfile_h1::createfx::parse(&createfx, &text(&fx_name).unwrap_or_default());
    let ambient: Vec<_> = fx.ambient().collect();
    let sounds = fx.placed.iter().filter(|p| p.kind == Kind::Sound).count();
    let exploders = fx.placed.len() - sounds - ambient.len();
    // The map's sounds (loops, interval sounds) go to the match's sound
    // catalog; with `BO2MP_MAP_SOUND_H1=0` they stay silent.
    if std::env::var("BO2MP_MAP_SOUND_H1").map_or(true, |v| v.trim() != "0") {
        match ambience_of(&fx, map_path, zone_dir, lower) {
            Some(a) => {
                report.push(format!(
                    "h1 createfx sounds: {} loops, {} interval sounds placed (zones for their aliases: {})",
                    a.loops.len(),
                    a.intervals.len(),
                    a.zones.len()
                ));
                asset_audio::set_h1_ambience(Some(a));
            }
            None => report.push("h1 createfx sounds: none placed".to_owned()),
        }
    } else {
        report.push("h1 createfx sounds: off (BO2MP_MAP_SOUND_H1=0)".to_owned());
    }
    if ambient.is_empty() {
        report.push(format!("h1 createfx: {} has no ambient effect placements ({sounds} sounds, {exploders} exploders)", createfx_name));
        return;
    }

    // The map zone's effects by name and by last part.
    let mut by_name: HashMap<String, usize> = HashMap::new();
    let mut by_base: HashMap<String, usize> = HashMap::new();
    for (i, f) in cap.fx.iter().enumerate() {
        if f.name.starts_with(',') || f.elems.is_empty() {
            continue;
        }
        let key = f.name.to_ascii_lowercase();
        by_base
            .entry(key.rsplit('/').next().unwrap_or(&key).to_owned())
            .or_insert(i);
        by_name.entry(key).or_insert(i);
    }
    let lookup = |name: &str| -> Option<usize> {
        let key = name.trim_start_matches(',').to_ascii_lowercase();
        by_name
            .get(&key)
            .or_else(|| by_name.get(key.trim_start_matches("fx/").trim_start_matches("vfx/")))
            .or_else(|| by_base.get(key.rsplit('/').next().unwrap_or(&key)))
            .copied()
    };

    // The effects placed, then what they run.
    let mut by_id: BTreeMap<&str, (usize, Option<usize>)> = BTreeMap::new();
    let mut found: BTreeMap<usize, ()> = BTreeMap::new();
    let mut queue: VecDeque<usize> = VecDeque::new();
    for p in &ambient {
        let entry = by_id.entry(p.fxid.as_str()).or_insert((0, None));
        entry.0 += 1;
        if entry.0 == 1 {
            let path = fx
                .paths
                .get(&p.fxid)
                .map_or(p.fxid.as_str(), String::as_str);
            if let Some(i) = lookup(path).or_else(|| lookup(&p.fxid)) {
                entry.1 = Some(i);
                if found.insert(i, ()).is_none() {
                    queue.push_back(i);
                }
            }
        }
    }
    let mut missing_children: HashSet<String> = HashSet::new();
    while let Some(i) = queue.pop_front() {
        for k in children(&cap.fx[i]) {
            match lookup(&k) {
                Some(c) => {
                    if found.insert(c, ()).is_none() {
                        queue.push_back(c);
                    }
                }
                None => {
                    missing_children.insert(k.to_ascii_lowercase());
                }
            }
        }
    }

    // Convert.
    let mut models = FxModelCatalog::default();
    let (mut sprites, mut sprites_ok, mut model_visuals) = (0usize, 0usize, 0usize);
    let mut views = Vec::new();
    let mut drawable: HashMap<usize, bool> = HashMap::new();
    for &i in found.keys() {
        let mut view = super::hmw::fx_view(b, &cap.fx[i], &mut models);
        let mut shows = !children(&cap.fx[i]).is_empty();
        for el in &mut view.elems {
            for v in &mut el.visuals {
                match v {
                    S1FxVisual::Material(_) => {
                        sprites_ok += 1;
                        shows = true;
                    }
                    S1FxVisual::Mark([a, c]) => {
                        shows |= a.is_some() || c.is_some();
                    }
                    S1FxVisual::Model(_) => {
                        model_visuals += 1;
                        *v = S1FxVisual::None;
                    }
                    _ => {}
                }
            }
        }
        sprites += cap.fx[i]
            .elems
            .iter()
            .flat_map(|e| &e.visuals)
            .filter(|v| matches!(v, fastfile_h1::walk::FxVisualRec::Material(_)))
            .count();
        drawable.insert(i, shows);
        views.push(view);
    }

    // The placements whose effect converted with something to show.
    let mut shots = Vec::new();
    let (mut played, mut empty, mut absent) = (0usize, 0usize, 0usize);
    let mut absent_ids: Vec<String> = Vec::new();
    let mut empty_ids: Vec<String> = Vec::new();
    for p in &ambient {
        let Some(i) = by_id.get(p.fxid.as_str()).and_then(|(_, i)| *i) else {
            absent += 1;
            if !absent_ids.contains(&p.fxid) {
                absent_ids.push(p.fxid.clone());
            }
            continue;
        };
        if !drawable.get(&i).copied().unwrap_or(false) {
            empty += 1;
            if !empty_ids.contains(&p.fxid) {
                empty_ids.push(p.fxid.clone());
            }
            continue;
        }
        played += 1;
        shots.push(CreateFxOneshot {
            fxid: cap.fx[i].name.clone(),
            origin_inches: p.origin,
            angles_deg: p.angles,
            // (The createfx delay is -15 for the editor's default and a
            // repeat time otherwise, not a pre-roll: effects loop by
            // themselves, as Ghosts'.)
            delay: 0.0,
            // An effect that loops by itself plays once; any other
            // re-fires on the placement's delay, as Ghosts'.
            repeat_secs: repeat_secs(cap.fx[i].elem_counts.0 > 0, p.delay),
        });
    }
    report.push(format!(
        "h1 createfx {} (from {}): {} placed ({} kinds), {} played, {} with nothing drawable, {} with no effect found; {} exploder and {} sound placements not played; {} effects converted ({} named by them, {} sprite visuals of which {} linked, {} model visuals left out); effects named but not in the map zone: {:?}; with nothing drawable: {:?}",
        createfx_name,
        if raws.zones.is_empty() { "the map zone".to_owned() } else { format!("{}", raws.zones.join(" + ")) },
        ambient.len(),
        by_id.len(),
        played,
        empty,
        absent,
        exploders,
        sounds,
        views.len(),
        by_id.values().filter(|(_, i)| i.is_some()).count(),
        sprites,
        sprites_ok,
        model_visuals,
        absent_ids,
        empty_ids
    ));
    if !missing_children.is_empty() {
        let mut m: Vec<_> = missing_children.into_iter().collect();
        m.sort();
        report.push(format!(
            "h1 createfx: {} child effects not in the map zone: {:?}",
            m.len(),
            &m[..m.len().min(12)]
        ));
    }
    set_pending(Some(GuestMapFx {
        models: FxModelCatalog::default(),
        views,
        shots,
        material_ns: AssetNamespace::T6,
    }));
}

/// The zones that hold a map's sound alias lists: the map, its patch zone
/// and the common zones (the `techsets_*` zones and the engine's picture
/// zone hold none).
pub(super) fn ambience_zones(
    map_path: &std::path::Path,
    zone_dir: Option<&std::path::Path>,
    lower: &str,
) -> Vec<std::path::PathBuf> {
    let mut zones = Vec::new();
    // (A patch zone is loaded after the map: its lists win.)
    if let Some(dir) = zone_dir {
        for (p, over) in fastfile_h1::shared::retail_zones(dir, lower) {
            if over {
                zones.push(p);
            }
        }
    }
    zones.push(map_path.to_path_buf());
    if let Some(dir) = zone_dir {
        for (p, over) in fastfile_h1::shared::retail_zones(dir, lower) {
            let stem = p.file_stem().map(|s| s.to_string_lossy().to_ascii_lowercase()).unwrap_or_default();
            if !over && !stem.starts_with("techsets_") && stem != "code_post_gfx_mp" {
                zones.push(p);
            }
        }
    }
    zones
}

/// A map's loop and interval sounds, from its createfx placements.
fn ambience_of(
    fx: &fastfile_h1::createfx::CreateFx,
    map_path: &std::path::Path,
    zone_dir: Option<&std::path::Path>,
    lower: &str,
) -> Option<asset_audio::H1MapAmbience> {
    let loops: Vec<(String, [f32; 3])> = fx.loop_sounds().map(|(a, o)| (a.to_owned(), o)).collect();
    let intervals: Vec<(String, [f32; 3], [f32; 2])> = fx
        .interval_sounds()
        .map(|p| (p.soundalias.clone(), p.origin, [p.delay_min.max(0.0), p.delay_max.max(p.delay_min).max(0.0)]))
        .collect();
    if loops.is_empty() && intervals.is_empty() {
        return None;
    }
    Some(asset_audio::H1MapAmbience { zones: ambience_zones(map_path, zone_dir, lower), loops, intervals })
}

/// The raw files the map's other scripts need from a patch zone: the HDR
/// fog, plain fog, film vision and the effects' scripts.
pub(super) fn patch_names(lower: &str) -> Vec<String> {
    let mut v = vec![
        format!("maps/createart/{lower}_fog_hdr.gsc"),
        format!("maps/createart/{lower}_fog.gsc"),
        format!("vision/{lower}.vision"),
    ];
    v.extend(wanted_raws(lower));
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sounds_become_loops_and_intervals() {
        let fx = fastfile_h1::createfx::parse(
            "ent = createLoopSound();\nent.v[ \"origin\" ] = ( 1, 2, 3 );\nent.v[ \"soundalias\" ] = \"emt_a\";\n\
             ent = createIntervalSound();\nent.v[ \"origin\" ] = ( 4, 5, 6 );\nent.v[ \"soundalias\" ] = \"emt_b\";\nent.v[ \"delay_min\" ] = 5;\nent.v[ \"delay_max\" ] = 9;\n",
            "",
        );
        let a = ambience_of(&fx, std::path::Path::new("mp_x.ff"), None, "mp_x").unwrap();
        assert_eq!(a.loops, [("emt_a".to_owned(), [1.0, 2.0, 3.0])]);
        assert_eq!(a.intervals, [("emt_b".to_owned(), [4.0, 5.0, 6.0], [5.0, 9.0])]);
        assert_eq!(a.zones, [std::path::PathBuf::from("mp_x.ff")]);
    }

    #[test]
    fn names_follow_the_map() {
        assert_eq!(wanted_raws("mp_rust")[0], "maps/createfx/mp_rust_fx.gsc");
        assert_eq!(patch_names("mp_cargoship").len(), 5);
    }

    #[test]
    fn one_shots_repeat_on_the_delay_and_loops_play_once() {
        assert_eq!(repeat_secs(false, 4.0), 4.0);
        assert_eq!(repeat_secs(false, -15.0), 15.0);
        assert_eq!(repeat_secs(false, 0.01), 0.1);
        assert_eq!(repeat_secs(true, 4.0), 0.0);
    }
}
