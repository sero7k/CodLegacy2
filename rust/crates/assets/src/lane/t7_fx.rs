//! bo2mp: a Black Ops III map's placed effects (`"classname" "fx"`
//! entities: Nuk3town's data streams, sparkles, butterflies and birds).
//!
//! The map's entities name each effect (`fxdef`) with its place, angles and
//! `delay` (negative milliseconds: how long it has run when the map
//! starts). The effects and every effect they run come from the map's zone
//! or the common zones (`T7Map::effect`); their sprites' materials are
//! linked into the map's material catalog as `t7fx:<name>`, drawn as Black
//! Ops II's effect techniques draw (vertex colour times the picture, unlit,
//! blended or added by the technique's name). Distortion (heat haze, water
//! rings) bends the picture behind it in a pass this renderer has not, and
//! is left out.
//!
//! The lane hands the effects and placements over (`set_pending`); Black
//! Ops II's rules loader adds them to the match's effect catalog and the
//! map's placed effects (`take_pending`), where a Black Ops II map's
//! createfx goes.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Mutex;

use asset_audio::CreateFxOneshot;
use asset_core::{T6Blend, T6Cull, T6Draw};
use asset_game::{T7FxElemView, T7FxView, T7FxVisual};
use fastfile_t7::map::T7Map;

/// The colour map's slot (`R_HashString("colorMap")`).
const COLOUR_SLOT: u32 = 0xa0ab_1041;

/// Does the technique bend the picture behind it?
fn distorts(technique_set: &str, material: &str) -> bool {
    let t = technique_set.trim_start_matches(',');
    t.contains("distort") || material.contains("distort")
}

/// Is the technique an emissive blend, its picture drawn brightened?
pub(super) fn glows(technique_set: &str) -> bool {
    technique_set.contains("emissive_blend")
}

/// How Black Ops II's effect pass draws a Black Ops III effect technique.
pub(super) fn effect_draw(technique_set: &str) -> T6Draw {
    let t = technique_set.trim_start_matches(',');
    let mut draw = T6Draw::from_technique_set("", 0, None);
    // (An emissive blend glows in Black Ops III: its sprite's picture is
    // the light's shape, brightened past white by the element's emissive
    // stops. Blended here it drew Nuk3town's voxel streams as dark
    // cubes over the sky; added, the picture's dark body adds little and
    // its bright edges glow, as in the game.)
    // (An emissive blend's picture is its light's shape: grey bodies and
    // white edges, brightened past white by the element's emissive stops,
    // then blended. Its picture is drawn brightened (`glows`) and blended,
    // so a data stream's voxels cover the sky gold as in the game; added,
    // they washed the sky white.)
    draw.blend = if t.contains("_add") || t.ends_with("add") || (t.contains("emissive") && !t.contains("emissive_blend")) { T6Blend::Add } else { T6Blend::Blend };
    draw.unlit = true;
    draw.effect = true;
    draw.cull = T6Cull::None;
    draw.layered = false;
    draw.alpha_test = false;
    draw.sort_key = 48;
    draw
}

/// One placed effect from an entity block.
fn placement(block: &str) -> Option<CreateFxOneshot> {
    let value = |key: &str| {
        let at = block.find(&format!("\"{key}\" \""))? + key.len() + 4;
        let rest = &block[at..];
        Some(&rest[..rest.find('"')?])
    };
    let floats = |s: &str| -> Option<[f32; 3]> {
        let v: Vec<f32> = s.split_whitespace().filter_map(|w| w.parse().ok()).collect();
        (v.len() == 3).then(|| [v[0], v[1], v[2]])
    };
    if value("classname")? != "fx" {
        return None;
    }
    // (As the client's radiant effect loader takes the block, 0x14029a8e0:
    // an effect bound to a game mode, `script_gameobjectname`, plays only
    // in that mode (left out here: the mode is not known to the map
    // load); `fxstate<k>` (default 1) enables it in lighting state k, and
    // the match runs lighting state 1; an effect with exploders
    // (`exploder_count` > 0) waits for its exploder, which scripts fire.)
    if value("script_gameobjectname").is_some_and(|v| !v.is_empty()) {
        return None;
    }
    if value("fxstate1").is_some_and(|v| v.trim().parse::<i32>().unwrap_or(0) == 0) {
        return None;
    }
    if value("exploder_count").is_some_and(|v| v.trim().parse::<i32>().unwrap_or(0) > 0) {
        return None;
    }
    Some(CreateFxOneshot {
        fxid: value("fxdef")?.to_owned(),
        origin_inches: floats(value("origin")?)?,
        angles_deg: value("angles").and_then(floats).unwrap_or_default(),
        // (Milliseconds in the entity; seconds here.)
        delay: value("delay").and_then(|d| d.parse::<f32>().ok()).unwrap_or(0.0) / 1000.0,
        repeat_secs: 0.0,
    })
}

/// The map's placed effects.
pub(super) fn placements(entities: &str) -> Vec<CreateFxOneshot> {
    entities.split('{').filter_map(placement).collect()
}

/// Links a sprite material into the catalog; its catalog name, or `None`
/// when it cannot be drawn here (distortion, no colour map, not found).
pub(super) type LinkMaterial<'a> = dyn FnMut(&str, T6Draw, &str, bool) -> Option<String> + 'a;

/// The effects the placements name and every effect they run, as the
/// catalog takes them, with their sprite materials linked by `link`
/// (material name, draw, colour map picture -> catalog name).
pub(super) fn effects(map: &T7Map, shots: &[CreateFxOneshot], extra: &[String], link: &mut LinkMaterial, report: &mut Vec<String>) -> Vec<T7FxView> {
    let mut todo: Vec<String> = shots.iter().map(|s| s.fxid.clone()).chain(extra.iter().cloned()).collect::<BTreeSet<_>>().into_iter().collect();
    let mut seen = BTreeSet::new();
    let mut out = Vec::new();
    let mut missing = Vec::new();
    let mut materials: BTreeMap<String, Option<String>> = BTreeMap::new();
    while let Some(name) = todo.pop() {
        let bare = name.trim_start_matches(',').to_owned();
        if !seen.insert(bare.clone()) {
            continue;
        }
        let Some(fx) = map.effect(&bare) else {
            missing.push(bare);
            continue;
        };
        let mut elems = Vec::with_capacity(fx.elems.len());
        for e in &fx.elems {
            let ty = e.raw.get(0xc8).copied().unwrap_or(0);
            let visuals: Vec<T7FxVisual> = e
                .visuals
                .iter()
                .map(|v| {
                    let v = v.trim_start_matches(',');
                    match ty {
                        0..=6 => {
                            let linked = materials.entry(v.to_owned()).or_insert_with(|| {
                                let m = map.effect_material(v)?;
                                let ts = m.techset.as_deref().unwrap_or("");
                                if distorts(ts, v) {
                                    return None;
                                }
                                let (_, image) = m.textures.iter().find(|(h, _)| *h == COLOUR_SLOT)?;
                                link(v, effect_draw(ts), image, glows(ts))
                            });
                            linked.clone().map_or(T7FxVisual::None, T7FxVisual::Material)
                        }
                        7 => T7FxVisual::Model(v.to_owned()),
                        14 if map.effect(v).is_some() => {
                            todo.push(v.to_owned());
                            T7FxVisual::Effect(v.to_owned())
                        }
                        14 => T7FxVisual::Sound(v.to_owned()),
                        _ => T7FxVisual::None,
                    }
                })
                .collect();
            let named = |k: usize| e.effects[k].as_ref().map(|n| n.trim_start_matches(',').to_owned());
            for k in 0..4 {
                if let Some(n) = named(k) {
                    todo.push(n);
                }
            }
            elems.push(T7FxElemView {
                record: e.raw.clone(),
                vel_samples: e.samples[0].clone(),
                vis_samples: e.samples[2].clone(),
                visuals,
                effect_on_impact: named(0),
                effect_on_death: named(1),
                effect_emitted: named(2),
                trail: e.trail.clone(),
                light: (ty == 8).then(|| e.visuals.first().and_then(|v| map.light_description(v)).cloned()).flatten(),
            });
        }
        out.push(T7FxView { name: bare, header: fx.header.clone(), elems });
    }
    let drawn = materials.values().filter(|m| m.is_some()).count();
    report.push(format!(
        "t7 effects: {} placed, {} effects ({} not found{}), {drawn} of {} sprite materials drawn",
        shots.len(),
        out.len(),
        missing.len(),
        if missing.is_empty() { String::new() } else { format!(": {:?}", &missing[..missing.len().min(6)]) },
        materials.len()
    ));
    out
}

/// Black Ops II's impact table rows (`OwnedFxImpactTable::impact_row`
/// for a Black Ops II table) and the Black Ops III `ImpactFx` table each
/// takes its effects from. Rows Black Ops III has no table for (mortar and
/// tank shells, bolts) keep Black Ops II's.
pub(super) const IMPACT_ROWS: &[(usize, &str)] = &[
    (0, "bullet_small_enter"),
    (1, "bullet_small_exit"),
    (3, "bullet_large_enter"),
    (4, "bullet_large_exit"),
    (5, "shotgun_enter"),
    (6, "shotgun_exit"),
    (9, "bullet_xlarge_enter"),
    (11, "grenade_bounce"),
    (12, "grenade_explosion"),
    (13, "rocket_explosion"),
    (14, "rocket_explosion"),
    (15, "grenade_dud"),
    (20, "knife_large_enter"),
];

/// Per Black Ops II row, Black Ops III's effects: the 31 non-flesh surface
/// cells (Black Ops II's surface types 0..=30, the same as Black Ops III's)
/// and the four flesh cells.
pub(super) type ImpactRow = (usize, Vec<String>, Vec<String>);

pub(super) fn impact_rows(map: &T7Map) -> Vec<ImpactRow> {
    IMPACT_ROWS
        .iter()
        .filter_map(|&(row, name)| {
            let (surf, flesh) = map.impact_effects(name)?;
            let s: Vec<String> = surf.into_iter().take(31).map(Option::unwrap_or_default).collect();
            let f: Vec<String> = flesh.into_iter().map(Option::unwrap_or_default).collect();
            Some((row, s, f))
        })
        .collect()
}

type Pending = (Vec<T7FxView>, Vec<CreateFxOneshot>, Vec<ImpactRow>);

static PENDING: Mutex<Option<Pending>> = Mutex::new(None);

pub(crate) fn set_pending(fx: Option<Pending>) {
    if let Ok(mut g) = PENDING.lock() {
        *g = fx;
    }
}

pub(crate) fn take_pending() -> Option<Pending> {
    PENDING.lock().ok().and_then(|mut g| g.take())
}

/// A Black Ops III map's minimap picture for Black Ops II's HUD: the name
/// its script's `setminimap` gives (`compass_map_<map>`), size, RGBA8 sRGB.
pub(crate) type PendingCompass = (String, u32, u32, Vec<u8>);

static COMPASS: Mutex<Option<PendingCompass>> = Mutex::new(None);

pub(crate) fn set_pending_compass(c: Option<PendingCompass>) {
    if let Ok(mut g) = COMPASS.lock() {
        *g = c;
    }
}

pub(crate) fn take_pending_compass() -> Option<PendingCompass> {
    COMPASS.lock().ok().and_then(|mut g| g.take())
}

#[cfg(test)]
mod radiant_tests {
    #[test]
    fn placed_effects_follow_the_clients_rules() {
        let block = |extra: &str| format!("{{\n\"origin\" \"1 2 3\"\n\"fxdef\" \"a/b\"\n{extra}\"classname\" \"fx\"\n}}\n");
        let ents = [
            block(""),
            block("\"fxstate1\" \"0\"\n"),
            block("\"fxstate1\" \"1\"\n\"fxstate2\" \"0\"\n"),
            block("\"exploder_count\" \"1\"\n\"exploder_index_0\" \"7\"\n"),
            block("\"script_gameobjectname\" \"dom\"\n"),
        ]
        .concat();
        assert_eq!(super::placements(&ents).len(), 2);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn placements_read_name_place_angles_and_delay() {
        let ents = "{\n\"classname\" \"worldspawn\"\n}\n{\n\"origin\" \"1032.94 445.145 -22.9645\"\n\"fxdef\" \"dlc0/nuketown/fx_de_rez_sm_sparkle_runner\"\n\"delay\" \"-18533\"\n\"angles\" \"0 199.297 0\"\n\"classname\" \"fx\"\n}\n";
        let p = placements(ents);
        assert_eq!(p.len(), 1);
        assert_eq!(p[0].fxid, "dlc0/nuketown/fx_de_rez_sm_sparkle_runner");
        assert_eq!(p[0].origin_inches, [1032.94, 445.145, -22.9645]);
        assert_eq!(p[0].angles_deg, [0.0, 199.297, 0.0]);
        assert!((p[0].delay + 18.533).abs() < 1e-4);
    }

    #[test]
    fn effect_techniques_blend_or_add() {
        assert_eq!(effect_draw(",ei/effect_lit_emissive_blend#2843f95a").blend, T6Blend::Blend);
        assert!(glows(",ei/effect_lit_emissive_blend#2843f95a"));
        assert_eq!(effect_draw(",ei/effect_lit_emissive_add#1").blend, T6Blend::Add);
        assert_eq!(effect_draw(",ei/effect_lit_blend#2843f95a").blend, T6Blend::Blend);
        assert_eq!(effect_draw(",ei/effect_add").blend, T6Blend::Add);
        assert!(effect_draw("x").effect && effect_draw("x").unlit);
        assert!(distorts(",ei/distortion_scale#1", "x") && distorts("y", "ei/gfx_water_ring_distort"));
    }
}

/// A Black Ops III map's scene props' animation timing (`t7_map`), for the
/// rules' script sources.
static PROPS: Mutex<Option<Vec<crate::T6AnimFacts>>> = Mutex::new(None);

pub(crate) fn set_pending_props(p: Option<Vec<crate::T6AnimFacts>>) {
    if let Ok(mut g) = PROPS.lock() {
        *g = p;
    }
}

pub(crate) fn take_pending_props() -> Option<Vec<crate::T6AnimFacts>> {
    PROPS.lock().ok().and_then(|mut g| g.take())
}

/// bo2mp: a Black Ops III Zombies map's AI types' characters (`t7_map`),
/// for the rules' script sources.
static CAST: Mutex<Option<Vec<crate::T7AiCast>>> = Mutex::new(None);

pub(crate) fn set_pending_cast(p: Option<Vec<crate::T7AiCast>>) {
    if let Ok(mut g) = CAST.lock() {
        *g = p;
    }
}

pub(crate) fn take_pending_cast() -> Option<Vec<crate::T7AiCast>> {
    CAST.lock().ok().and_then(|mut g| g.take())
}
