//! bo2mp: a Ghosts map's ambient effects (waterfalls, mist, smoke, fire,
//! dust, god rays: the effects its createfx script places) played in the
//! match.
//!
//! The placements come from the map's compiled createfx script
//! (`fastfile_iw6::createfx`); the effects they name come from the map zone
//! or, for the shared ones, from `common_mp` and `common_core_mp`
//! (`fastfile_iw6::shared`). Each is converted to the effect catalog through
//! the Advanced Warfare path (`FxCatalog::capture_s1_as`: Ghosts' element
//! record is AW's 8 bytes shorter, its visual samples 80 bytes where AW's are
//! 112, its element types two fewer from the trails on), with the sprites'
//! materials linked into the map's material catalog. Models, flares, particle
//! simulations, spot lights, distortion and blur have no counterpart and are
//! left out; the effect's other parts play.
//!
//! Exploders (script-triggered), sounds and placements whose effect is in no
//! zone read are not played. `BO2MP_MAP_FX_IW6=0` turns it all off.
//!
//! The lane hands the effects and placements over (`set_pending`); Black Ops
//! II's rules loader adds them to the match's effect catalog and the map's
//! placed effects (`take_pending`), where a Black Ops II map's createfx goes.

use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
use std::sync::Mutex;

use asset_audio::CreateFxOneshot;
use asset_core::AssetNamespace;
use asset_game::{S1FxElemView, S1FxTrail, S1FxView, S1FxVisual};
use asset_material::TS_COLOR_MAP;
use fastfile_iw6::fxview::{OwnedElem, OwnedFx, OwnedVisual};

use super::iw6::Builder;

/// The largest level kept of an effect's pictures.
const FX_MAX: u32 = 512;

/// `BO2MP_MAP_FX_<game>`: on unless `0`.
pub(super) fn map_fx_on(var: &str) -> bool {
    codlegacy_env::var(var).map_or(true, |v| v.trim() != "0")
}

/// A guest map's effects for the rules loader.
pub(crate) struct GuestMapFx {
    /// The models its model elements use (the map zone's, built from the
    /// Ghosts xmodels; `BO2MP_IW6_FX_MODELS=0` leaves them out).
    pub models: asset_game::FxModelCatalog,
    pub views: Vec<S1FxView>,
    pub shots: Vec<CreateFxOneshot>,
    /// Where the sprites' materials are looked up.
    pub material_ns: AssetNamespace,
}

static PENDING: Mutex<Option<GuestMapFx>> = Mutex::new(None);

pub(crate) fn set_pending(fx: Option<GuestMapFx>) {
    if let Ok(mut g) = PENDING.lock() {
        *g = fx;
    }
}

pub(crate) fn take_pending() -> Option<GuestMapFx> {
    PENDING.lock().ok().and_then(|mut g| g.take())
}

/// Ghosts' element type as AW's (`None`: no counterpart; it plays as a
/// silent element). Ghosts (`docs/exe_notes/IW6_VFX_AUDIO.md` A1): 0
/// sprite, 1 oriented, 2 tail, 3 trail, 4 flare, 5 particle simulation, 6
/// and 7 cloud-like (unused by the retail zones read), 8 and 9 the clouds
/// (the third draw list, `fx_drawClouds`), 10 spark fountain, 11 model, then
/// AW's numbers. AW's clouds are 8 and 9 too. With `BO2MP_IW6_FX_CLOUDS=0`
/// types 8 and 9 are dropped (as before: `common_mp`'s glass and mud
/// impacts and `mp_frag`'s snow, indoor dust and sprinklers lost them).
fn aw_elem_type(raw: u8) -> u8 {
    aw_elem_type_with(raw, clouds_on())
}

fn aw_elem_type_with(raw: u8, clouds: bool) -> u8 {
    match raw {
        0 | 1 => raw,
        2 => 3,
        3 => 5,
        4 => 6,
        5 => 7,
        6 => 8,
        7 => 9,
        8 | 9 if clouds => raw,
        10..=17 => raw,
        _ => 255,
    }
}

/// `BO2MP_IW6_FX_CLOUDS` (OFF since the owner's snow-map screenshot, 2026-11-04: giant radial sheets; not proven correct offline): Ghosts' cloud elements (types 8, 9) play.
pub(super) fn clouds_on() -> bool {
    asset_audio::iw6_rules::switch("FX_CLOUDS", false)
}

const IW6_VIS: usize = 80;
const S1_VIS: usize = 112;

fn put(dst: &mut [u8], at: usize, src: &[u8], from: usize, len: usize) {
    if let (Some(d), Some(s)) = (dst.get_mut(at..at + len), src.get(from..from + len)) {
        d.copy_from_slice(s);
    }
}

/// One Ghosts visual sample as AW's: the colour, the rotation, the size and
/// the scale in AW's places, the emissive scale neutral.
fn vis_sample(s: &[u8]) -> [u8; S1_VIS] {
    let mut out = [0u8; S1_VIS];
    for half in 0..2 {
        let (i, o) = (half * 40, half * 56);
        put(&mut out, o, s, i, 16);
        out[o + 16..o + 28]
            .copy_from_slice(&[0, 0, 0x80, 0x3f, 0, 0, 0x80, 0x3f, 0, 0, 0x80, 0x3f]);
        // rotation delta, total, size x2, scale
        put(&mut out, o + 28, s, i + 20, 20);
    }
    out
}

fn elem(
    el: &OwnedElem,
    material: &mut dyn FnMut(&str) -> Option<String>,
    model: &mut dyn FnMut(&str) -> Option<String>,
    stats: &mut Stats,
) -> S1FxElemView {
    use fastfile_iw6::layout::FxElemDef as e;
    let mut record = vec![0u8; 320];
    put(&mut record, 0, &el.record, 0, 292.min(el.record.len()));
    // (Ghosts: fade info at 291, random seed at 292; AW: fade-out info at
    // 292, random seed at 296.)
    put(&mut record, 296, &el.record, 292, 4);
    record[292] = el.record.get(291).copied().unwrap_or(0);
    let raw_type = el.record.get(e::elemType).copied().unwrap_or(0);
    record[e::elemType] = aw_elem_type(raw_type);
    if aw_elem_type(raw_type) == 255 {
        stats.unsupported += 1;
    }
    let visuals = el
        .visuals
        .iter()
        .map(|v| match v {
            OwnedVisual::None | OwnedVisual::Other => S1FxVisual::None,
            OwnedVisual::Material(n) => {
                stats.sprites += 1;
                match material(n) {
                    Some(m) => {
                        stats.sprites_ok += 1;
                        S1FxVisual::Material(m)
                    }
                    None => S1FxVisual::None,
                }
            }
            OwnedVisual::Mark(pair) => {
                let mut linked: [Option<String>; 2] = [None, None];
                for (slot, name) in linked.iter_mut().zip(pair.iter()) {
                    *slot = name.as_deref().and_then(&mut *material);
                }
                S1FxVisual::Mark(linked)
            }
            OwnedVisual::Model(n) => {
                stats.models += 1;
                match model(n) {
                    Some(m) => {
                        stats.models_ok += 1;
                        S1FxVisual::Model(m)
                    }
                    None => S1FxVisual::None,
                }
            }
            OwnedVisual::Effect(n) => S1FxVisual::Effect(n.clone()),
            OwnedVisual::Sound(n) => S1FxVisual::Sound(n.clone()),
        })
        .collect();
    let vis_samples = el
        .vis_samples
        .chunks_exact(IW6_VIS)
        .flat_map(vis_sample)
        .collect();
    S1FxElemView {
        record,
        vel_samples: el.vel_samples.clone(),
        vis_samples,
        visuals,
        effect_on_impact: el.effect_on_impact.clone(),
        effect_on_death: el.effect_on_death.clone(),
        effect_emitted: el.effect_emitted.clone(),
        trail: el.trail.as_ref().map(|(record, verts, inds)| {
            // (Ghosts' trail vertex is 24 bytes, AW's 32.)
            let verts = verts
                .chunks_exact(24)
                .flat_map(|v| {
                    let mut o = [0u8; 32];
                    o[..24].copy_from_slice(v);
                    o
                })
                .collect();
            S1FxTrail {
                record: record.clone(),
                verts,
                inds: inds.clone(),
            }
        }),
        extended: el.extended.clone(),
        // (Ghosts' own record and samples: `fx_iw4::iw6`'s rules read them.)
        iw6: Some(std::sync::Arc::new(asset_game::Iw6ElemSource::read(&el.record, &el.vis_samples).with_flare(&el.flare))),
    }
}

#[cfg(test)]
mod model_visual_tests {
    use super::*;

    #[test]
    fn a_model_element_keeps_its_built_model() {
        let el = OwnedElem { record: vec![0; 312], visuals: vec![OwnedVisual::Model("fx_paper_sheet".into()), OwnedVisual::Model("absent".into())], ..Default::default() };
        let mut st = Stats::default();
        let out = elem(&el, &mut |_| None, &mut |n| (n == "fx_paper_sheet").then(|| "built".to_owned()), &mut st);
        assert!(matches!(&out.visuals[0], S1FxVisual::Model(m) if m == "built"));
        assert!(matches!(out.visuals[1], S1FxVisual::None));
        assert_eq!((st.models, st.models_ok), (2, 1));
    }
}

#[cfg(test)]
mod type_tests {
    use super::aw_elem_type_with;

    #[test]
    fn clouds_keep_their_numbers() {
        assert_eq!(aw_elem_type_with(8, true), 8);
        assert_eq!(aw_elem_type_with(9, true), 9);
        assert_eq!(aw_elem_type_with(8, false), 255);
        assert_eq!(aw_elem_type_with(2, true), 3);
        assert_eq!(aw_elem_type_with(10, true), 10);
    }
}

#[derive(Default)]
struct Stats {
    sprites: usize,
    sprites_ok: usize,
    models: usize,
    /// Model visuals whose model was built into the effect model catalog.
    models_ok: usize,
    unsupported: usize,
}

/// How a placement plays: once, or again every so many seconds
/// (`CreateFxOneshot::repeat_secs`, 0 = once). With
/// `BO2MP_IW6_CREATEFX_KIND` (the default) by the placement (its stored
/// delay: `fx_iw4::iw6::createfx::placement_of_delay`), an effect that
/// loops by itself played once whatever its placement (a replay would stack
/// it); without, by whether the effect loops by itself.
fn repeat_secs(by_placement: bool, effect_loops: bool, delay: f32) -> f32 {
    use fx_iw4::iw6::createfx::{Placement, placement_of_delay};
    if effect_loops {
        return 0.0;
    }
    if !by_placement {
        return asset_audio::ghosts_repeat_secs(delay);
    }
    match placement_of_delay(delay) {
        Placement::Oneshot => 0.0,
        Placement::Loop { repeat_msec } => repeat_msec as f32 / 1000.0,
    }
}

/// The map's line on Ghosts' effect rules: the switches and what the
/// map's elements ask of them.
fn rules_line(fx: &[&OwnedFx], once: usize, repeating: usize) -> String {
    let mut lit = [0usize; 6];
    let (mut elems, mut unlit_scaled) = (0usize, 0usize);
    for f in fx {
        for el in &f.elems {
            elems += 1;
            let r = fx_iw4::iw6::Iw6ElemRules::read(&el.record);
            lit[usize::from(r.lit_type).min(5)] += 1;
            let scaled = el.vis_samples.chunks_exact(IW6_VIS).any(|s| {
                [fx_iw4::iw6::vis::UNLIT_HDR_SCALE, fx_iw4::iw6::vis::HALF + fx_iw4::iw6::vis::UNLIT_HDR_SCALE]
                    .iter()
                    .any(|&o| s.get(o..o + 4).is_some_and(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]]) != 1.0))
            });
            unlit_scaled += usize::from(scaled);
        }
    }
    let sw = |on: bool| if on { "on" } else { "off" };
    format!(
        "iw6 fx rules: {} (BO2MP_IW6_FX_RULES), lit blend {} (BO2MP_IW6_FX_LIT), createfx kind {} (BO2MP_IW6_CREATEFX_KIND): {} elements in {} effects, lit types 0/1/2/3/4/other {:?}, {} with an unlit HDR scale other than 1; placements {} once, {} repeating",
        sw(asset_game::iw6_fx_rules_on()),
        sw(asset_game::iw6_fx_lit_on()),
        sw(asset_game::iw6_createfx_kind_on()),
        elems,
        fx.len(),
        lit,
        unlit_scaled,
        once,
        repeating
    )
}

/// The names of the effects an effect runs.
fn children(fx: &OwnedFx) -> Vec<&str> {
    let mut out = Vec::new();
    for el in &fx.elems {
        for v in &el.visuals {
            if let OwnedVisual::Effect(n) = v {
                out.push(n.as_str());
            }
        }
        for n in [
            &el.effect_on_impact,
            &el.effect_on_death,
            &el.effect_emitted,
        ]
        .into_iter()
        .flatten()
        {
            out.push(n.as_str());
        }
    }
    out
}

/// `BO2MP_IW6_FX_DRAW` (on): Ghosts' effect materials draw by their technique's
/// own blend.
pub(crate) fn fx_draw_on() -> bool {
    asset_audio::iw6_rules::switch("FX_DRAW", true)
}

/// How Black Ops II's effect pass draws a Ghosts effect technique set. The
/// blend is the technique's: `add` families ONE/ONE (colour times alpha in the
/// pixel shader), `abl` ("alpha blend with bloom": SRC1_ALPHA / INV_SRC1_ALPHA,
/// whose second output is the alpha itself, so exactly an alpha blend for the
/// colour), `screen` (INV_DEST_COLOR / ONE, drawn added), the rest blended.
/// Techniques that are not effects (`particle_cloud_*` are) keep `base`.
pub(crate) fn fx_draw(technique_set: &str, base: asset_core::T6Draw) -> asset_core::T6Draw {
    use asset_core::{T6Blend, T6Cull};
    let name = technique_set.trim_start_matches(',');
    if !(name.starts_with("effect") || name.starts_with("particle_cloud")) {
        return base;
    }
    let has = |w: &str| name.split('_').any(|t| t == w);
    let mut d = base;
    d.blend = if has("add") || has("screen") {
        T6Blend::Add
    } else if has("multiply") {
        T6Blend::Multiply
    } else {
        T6Blend::Blend
    };
    // (`screen`: INV_DEST_COLOR / ONE, which the GPU pass blends as such;
    // `BO2MP_IW6_FX_SCREEN=0` draws it added, as before.)
    d.screen = has("screen") && asset_audio::iw6_rules::switch("FX_SCREEN", true);
    d.effect = true;
    d.unlit = true;
    d.cull = T6Cull::None;
    d.layered = false;
    d.alpha_test = false;
    d
}

/// A sprite material by name, linked into the builder's catalog when it
/// can be drawn (a supported technique and a colour picture); its catalog
/// name.
fn link_fx_material(b: &mut Builder, cache: &mut HashMap<String, Option<String>>, name: &str) -> Option<String> {
    let k = name.trim_start_matches(',').to_ascii_lowercase();
    if let Some(hit) = cache.get(&k) {
        return hit.clone();
    }
    let r = (|| {
        let Some((tech, slot, canon)) = b.material_named(name, FX_MAX) else {
            if std::env::var_os("BO2MP_FX_DEBUG").is_some() {
                eprintln!("iw6 fx material {name}: not found / not linked");
            }
            return None;
        };
        // bo2mp: Ghosts' effect techniques by their own blend (decoded from the
        // DXBC, `docs/exe_notes/IW6_FX_EXE.md`), not the model reader's name
        // guess: `particle_cloud_*_replace_*_abl_*` was read as an opaque lit
        // surface (the word `replace`).
        if fx_draw_on() {
            if let Some(d) = b.catalog.materials[slot].t6_draw.as_mut() {
                *d = fx_draw(&tech, *d);
            }
        }
        // bo2mp: a distortion technique is kept (the native renderer draws it
        // over a copy of the scene, `render_iw6::distort`); the effect pass
        // never draws it as a sprite.
        if tech.trim_start_matches(',').starts_with("distortion") && asset_game::iw6_distortion_on() {
            asset_game::set_iw6_fx_distortion(&canon);
            return Some(canon);
        }
        if super::s1::fx_technique_unsupported(&tech) {
            if std::env::var_os("BO2MP_FX_DEBUG").is_some() {
                eprintln!("iw6 fx material {name}: technique {tech} unsupported");
            }
            return None;
        }
        let ok = b.catalog.materials[slot]
            .textures
            .iter()
            .any(|t| t.semantic == TS_COLOR_MAP && t.image.is_some());
        if !ok && std::env::var_os("BO2MP_FX_DEBUG").is_some() {
            eprintln!(
                "iw6 fx material {name}: no colour picture ({} textures)",
                b.catalog.materials[slot].textures.len()
            );
        }
        if ok {
            // bo2mp: what its technique does that Black Ops II's shaders do not
            // (`fx_iw4::iw6::material`): the falloff, the near ramp, the eye offset.
            let consts = &b.catalog.materials[slot].constants;
            let fx = fx_iw4::iw6::material::read(&tech, |want| {
                let want = &want.as_bytes()[..want.len().min(12)];
                consts.iter().find(|c| {
                    let n = c.name.iter().take_while(|&&x| x != 0).copied().collect::<Vec<u8>>();
                    n.as_slice() == want
                }).map(|c| c.literal)
            });
            asset_game::set_iw6_fx_material(&canon, fx);
        }
        ok.then_some(canon)
    })();
    cache.insert(k, r.clone());
    r
}

/// `BO2MP_IW6_FX_MODELS` (on): Ghosts' model elements (type 11) draw their
/// models (debris, burning paper) when the map zone holds them.
pub(crate) fn models_on() -> bool {
    map_fx_on("BO2MP_IW6_FX_MODELS")
}

/// A model element's model built into the effect model catalog from the
/// map zone's xmodels (first LOD, its surfaces' materials linked like a
/// gun's); the name it is held under, `None` when the zone does not hold it.
fn link_fx_model(b: &mut Builder, cache: &mut HashMap<String, Option<String>>, models: &mut asset_game::FxModelCatalog, name: &str) -> Option<String> {
    let key = name.trim_start_matches(',').to_ascii_lowercase();
    if let Some(hit) = cache.get(&key) {
        return hit.clone();
    }
    let cap = b.cap;
    let r = (|| {
        let model = cap.xmodels.iter().find(|m| m.name.trim_start_matches(',').eq_ignore_ascii_case(&key))?;
        let (xref, keys) = b.model_ref(model)?;
        let linked: HashMap<usize, usize> = keys.into_iter().filter_map(|k| Some((k, b.material(k, FX_MAX)?))).collect();
        let skel = asset_model::capture_model_skel_t6(&xref, |k| linked.get(&k.index).copied())?;
        let built = skel.name.clone();
        models.insert_skel(skel, &b.catalog);
        Some(built)
    })();
    cache.insert(key, r.clone());
    r
}

/// Ghosts effects as the catalog's (the Advanced Warfare path), their
/// sprites' materials linked into the builder's catalog; with each one's
/// counts.
fn convert(b: &mut Builder, effects: &[&OwnedFx], mut models: Option<&mut asset_game::FxModelCatalog>) -> (Vec<S1FxView>, Vec<Stats>, Vec<String>) {
    let mut cache: HashMap<String, Option<String>> = HashMap::new();
    let mut model_cache: HashMap<String, Option<String>> = HashMap::new();
    let mut views = Vec::with_capacity(effects.len());
    let mut stats = Vec::with_capacity(effects.len());
    for f in effects {
        let mut st = Stats::default();
        // (The models first: building them and linking the sprites' materials
        // both need the builder.)
        if let Some(models) = models.as_deref_mut().filter(|_| models_on()) {
            for el in &f.elems {
                for v in &el.visuals {
                    if let OwnedVisual::Model(n) = v {
                        link_fx_model(b, &mut model_cache, models, n);
                    }
                }
            }
        }
        let mut material = |name: &str| link_fx_material(b, &mut cache, name);
        let mut model = |name: &str| -> Option<String> {
            model_cache.get(&name.trim_start_matches(',').to_ascii_lowercase()).cloned().flatten()
        };
        let elems = f.elems.iter().map(|el| elem(el, &mut material, &mut model, &mut st)).collect();
        views.push(S1FxView { name: f.name.clone(), header: f.header.clone(), elems, h1: false });
        stats.push(st);
    }
    // (Sprite materials that did not link: not found, an unsupported technique or no colour picture.)
    let mut unlinked: Vec<String> = cache.iter().filter(|(_, v)| v.is_none()).map(|(k, _)| k.clone()).collect();
    unlinked.sort();
    (views, stats, unlinked)
}

/// bo2mp: Ghosts effects named by the guns (their impacts), with every
/// effect they run, from the shared zones; converted as the map's are.
/// `(views, sprite visuals, of which linked, names not found)`.
pub(super) fn gun_effects(b: &mut Builder, roots: &[String]) -> (Vec<S1FxView>, usize, usize, Vec<String>) {
    let Some(shared) = b.shared.clone() else {
        return (Vec::new(), 0, 0, roots.to_vec());
    };
    let mut found: Vec<OwnedFx> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    let mut missing: Vec<String> = Vec::new();
    let mut queue: VecDeque<String> = roots.iter().cloned().collect();
    while let Some(name) = queue.pop_front() {
        let key = name.trim_start_matches(',').to_ascii_lowercase();
        if key.is_empty() || !seen.insert(key) {
            continue;
        }
        match shared.effect(&name) {
            Some(f) => {
                queue.extend(children(f).into_iter().map(str::to_owned));
                found.push(f.clone());
            }
            None => missing.push(name),
        }
    }
    let refs: Vec<&OwnedFx> = found.iter().collect();
    let (views, stats, _) = convert(b, &refs, None);
    let sprites = stats.iter().map(|s| s.sprites).sum();
    let linked = stats.iter().map(|s| s.sprites_ok).sum();
    (views, sprites, linked, missing)
}

/// Reads the map's createfx and converts the effects it places; the result
/// waits for the rules loader (`take_pending`). Report lines explain what
/// played and what did not.
pub(super) fn prepare(
    b: &mut Builder,
    cap: &fastfile_iw6::walk::Capture,
    image: &[u8],
    report: &mut Vec<String>,
) {
    set_pending(None);
    if !map_fx_on("BO2MP_MAP_FX_IW6") {
        report.push("iw6 createfx: off (BO2MP_MAP_FX_IW6=0)".to_owned());
        return;
    }
    // (A map with no readable createfx still converts the effects its scripts, exploders and
    // scriptables name: they were skipped with it before.)
    let fx = fastfile_iw6::createfx::read(cap, image).unwrap_or_else(|| {
        report.push(format!(
            "iw6 createfx: no createfx script among {} scripts (the script, exploder and scriptable effects are still read)",
            cap.script_files.len()
        ));
        fastfile_iw6::createfx::CreateFx::default()
    });
    for n in &fx.notes {
        report.push(format!("iw6 createfx: {n}"));
    }
    if fx.placed.is_empty() {
        report.push(format!(
            "iw6 createfx {}: no placed effects read ({} exploder, {} sound placements)",
            fx.script, fx.exploders, fx.sounds
        ));
    }
    // The map zone's own effects by path and by last part.
    let mut zone_fx: HashMap<String, OwnedFx> = HashMap::new();
    let mut zone_base: HashMap<String, String> = HashMap::new();
    for f in &cap.fx {
        if f.name.starts_with(',') || f.elems.is_empty() {
            continue;
        }
        let key = f.name.to_ascii_lowercase();
        zone_base
            .entry(key.rsplit('/').next().unwrap_or(&key).to_owned())
            .or_insert_with(|| key.clone());
        zone_fx
            .entry(key)
            .or_insert_with(|| fastfile_iw6::fxview::own(f, cap, image));
    }
    let shared = b.shared.clone();
    let lookup = |name: &str| -> Option<OwnedFx> {
        let key = name.trim_start_matches(',').to_ascii_lowercase();
        if let Some(f) = zone_fx.get(&key) {
            return Some(f.clone());
        }
        let base = key.rsplit('/').next().unwrap_or(&key);
        if let Some(f) = zone_base.get(base).and_then(|k| zone_fx.get(k)) {
            return Some(f.clone());
        }
        shared.as_ref()?.effect(name).cloned()
    };

    // The effects placed, then what they run.
    let mut by_id: BTreeMap<&str, (usize, Option<String>)> = BTreeMap::new();
    let mut found: HashMap<String, OwnedFx> = HashMap::new();
    let mut queue: VecDeque<String> = VecDeque::new();
    for p in &fx.placed {
        let entry = by_id.entry(p.fxid.as_str()).or_insert((0, None));
        entry.0 += 1;
        if entry.0 == 1 {
            let path = fx
                .paths
                .get(&p.fxid)
                .map_or(p.fxid.as_str(), String::as_str);
            if let Some(f) = lookup(path).or_else(|| lookup(&p.fxid)) {
                entry.1 = Some(f.name.clone());
                queue.push_back(f.name.clone());
                found.insert(f.name.to_ascii_lowercase(), f);
            }
        }
    }
    // bo2mp: the effects of the map's exploders (`activateclientexploder`, driven by the map's
    // scripts) are converted too: by effect id, the name the catalog has them under.
    let mut exploder_effects: HashMap<String, String> = HashMap::new();
    let mut unresolved_exploder_pending: Vec<String> = Vec::new();
    if let Some(pack) = asset_world::iw6_scripts() {
        for e in &pack.exploders {
            if exploder_effects.contains_key(&e.fxid) {
                continue;
            }
            let path = fx.paths.get(&e.fxid).map_or(e.fxid.as_str(), String::as_str);
            if let Some(f) = lookup(path).or_else(|| lookup(&e.fxid)) {
                exploder_effects.insert(e.fxid.clone(), f.name.clone());
                if !found.contains_key(&f.name.to_ascii_lowercase()) {
                    queue.push_back(f.name.clone());
                    found.insert(f.name.to_ascii_lowercase(), f);
                }
            } else {
                unresolved_exploder_pending.push(format!("{} ({path})", e.fxid));
            }
        }
    }
    // bo2mp: the effects the map's scripts name (`loadfx`, `playfx`, destructible definitions):
    // converted too, by path, so a script's `playfx` plays whatever it names.
    let mut script_effects: HashMap<String, String> = HashMap::new();
    let mut unresolved_script: Vec<String> = Vec::new();
    let mut unresolved_scriptable: Vec<String> = Vec::new();
    if let Some(pack) = asset_world::iw6_scripts() {
        for (path, _) in &pack.script_fx {
            if let Some(f) = lookup(path) {
                script_effects.insert(path.clone(), f.name.clone());
                if !found.contains_key(&f.name.to_ascii_lowercase()) {
                    queue.push_back(f.name.clone());
                    found.insert(f.name.to_ascii_lowercase(), f);
                }
            } else {
                unresolved_script.push(path.clone());
            }
        }
        report.push(format!(
            "iw6 script fx: {} of {} effect paths the scripts name found in the zones",
            script_effects.len(),
            pack.script_fx.len()
        ));
    }
    // bo2mp: the effects the map's scriptable objects play as they start (their parts' first
    // states: the fire barrels' fire, the electrical boxes' sparks, the lights' glows), at the
    // instance's origin (INFERRED: the tag's offset is not applied; the effect points up).
    // `BO2MP_IW6_SCRIPTABLE_FX=0` leaves them out.
    let mut scriptable_places: Vec<(String, [f32; 3], f32)> = Vec::new();
    let mut scriptable_total = 0usize;
    if asset_audio::iw6_rules::switch("SCRIPTABLE_FX", true) {
        for inst in &cap.clip_maps.first().map(|c| c.scriptables.clone()).unwrap_or_default() {
            let Some(list) = cap.scriptable_fx.get(&inst.def.index) else { continue };
            for &(_, fx, loop_ms, _) in list {
                let Some(def) = fx.and_then(|id| cap.fx.get(id.index as usize)) else { continue };
                scriptable_total += 1;
                let Some(f) = lookup(&def.name) else {
                    unresolved_scriptable.push(def.name.trim_start_matches(',').to_owned());
                    continue;
                };
                if !found.contains_key(&f.name.to_ascii_lowercase()) {
                    queue.push_back(f.name.clone());
                    found.insert(f.name.to_ascii_lowercase(), f.clone());
                }
                scriptable_places.push((f.name.clone(), inst.origin, f32::from(loop_ms) / 1000.0));
            }
        }
        report.push(format!(
            "iw6 scriptable fx: {} effect events of the placed scriptables' first states, {} found in the zones",
            scriptable_total,
            scriptable_places.len()
        ));
    }
    let mut missing_children: HashSet<String> = HashSet::new();
    while let Some(name) = queue.pop_front() {
        let Some(f) = found.get(&name.to_ascii_lowercase()) else {
            continue;
        };
        let kids: Vec<String> = children(f).into_iter().map(str::to_owned).collect();
        for k in kids {
            let key = k.to_ascii_lowercase();
            if found.contains_key(&key) || missing_children.contains(&key) {
                continue;
            }
            match lookup(&k) {
                Some(c) => {
                    queue.push_back(c.name.clone());
                    found.insert(key, c);
                }
                None => {
                    missing_children.insert(key);
                }
            }
        }
    }

    // bo2mp: the effects Black Ops III has a matching one of (`bo3_vfx`):
    // by effect id, the Black Ops III effect it plays instead, read from the
    // install's shared zones; every other keeps playing as the game's own.
    let mut picks: HashMap<String, (String, &'static str)> = HashMap::new();
    let mut bo3_views: Vec<asset_game::T7FxView> = Vec::new();
    let mut vfx_line: Option<String> = None;
    if !super::bo3_vfx::enabled() {
        vfx_line = Some("iw6 vfx substitution: off (BO2MP_BO3_VFX_SUBST=0)".to_owned());
    } else if !super::bo3_look::game_on("BO2MP_BO3_LOOK_IW6", true) {
        vfx_line = Some("iw6 vfx substitution: off (the Black Ops III look is off)".to_owned());
    } else {
        let path_of = |id: &str| fx.paths.get(id).cloned().unwrap_or_else(|| id.to_owned());
        let wanted_ids: Vec<&str> = by_id.keys().copied().filter(|id| super::bo3_vfx::wants(&path_of(id))).collect();
        let mut tally = super::bo3_vfx::Tally::default();
        if wanted_ids.is_empty() {
            vfx_line = Some(format!("iw6 vfx substitution: no placed effect names a Black Ops III equivalent ({} kinds placed, all the game's own)", by_id.len()));
        } else {
            match super::t7::bo3_zone_dir() {
                Err(e) => vfx_line = Some(format!("iw6 vfx substitution: {} placed kinds could take Black Ops III's, but {e}; the game's own play", wanted_ids.len())),
                Ok(dir) => {
                    let mut chosen: Vec<(String, String, &'static str)> = Vec::new();
                    let seed = std::mem::take(&mut b.catalog);
                    let sf = super::t7_map::shared_effects(
                        &dir,
                        seed,
                        Box::new(|names| {
                            for id in &wanted_ids {
                                match super::bo3_vfx::choose(&path_of(id), names) {
                                    Some((bo3, rule)) => chosen.push(((*id).to_owned(), bo3, rule)),
                                    None => {
                                        tally.kept.insert((*id).to_owned(), "no Black Ops III effect for its rule");
                                    }
                                }
                            }
                            let mut want: Vec<String> = chosen.iter().map(|c| c.1.clone()).collect();
                            want.sort();
                            want.dedup();
                            (want, Vec::new())
                        }),
                    );
                    b.catalog = sf.material_seed;
                    // (Only effects the shared zones really gave views for.)
                    let have: std::collections::HashSet<&str> = sf.views.iter().map(|v| v.name.as_str()).collect();
                    for (id, bo3, rule) in chosen {
                        if have.contains(bo3.as_str()) {
                            tally.matched.insert(id.clone(), (bo3.clone(), rule));
                            picks.insert(id, (bo3, rule));
                        } else {
                            tally.kept.insert(id, "its Black Ops III effect could not be read");
                        }
                    }
                    for id in by_id.keys().filter(|id| !wanted_ids.contains(id)) {
                        tally.kept.insert((*id).to_owned(), "no rule: the game's own");
                    }
                    let source = sf.report.iter().rev().find(|l| l.contains("wanted")).cloned().unwrap_or_else(|| "Black Ops III shared zones".to_owned());
                    bo3_views = sf.views;
                    for p in &fx.placed {
                        if picks.contains_key(&p.fxid) { tally.placed_bo3 += 1 } else { tally.placed_own += 1 }
                    }
                    vfx_line = Some(tally.line("iw6", &source));
                }
            }
        }
    }

    // Convert.
    let mut names: Vec<&String> = found.keys().collect();
    names.sort();
    let ordered: Vec<&OwnedFx> = names.iter().map(|k| &found[*k]).collect();
    let mut fx_models = asset_game::FxModelCatalog::default();
    fx_models.set_capture_ns(AssetNamespace::Iw6);
    let (views, stats_list, unlinked_materials) = convert(b, &ordered, Some(&mut fx_models));
    let stats: HashMap<String, Stats> = names.iter().map(|k| (*k).clone()).zip(stats_list).collect();

    // The placements whose effect converted with something to show.
    let mut shots = Vec::new();
    let (mut played, mut empty, mut absent) = (0usize, 0usize, 0usize);
    let mut absent_ids: Vec<String> = Vec::new();
    let mut empty_ids: Vec<String> = Vec::new();
    let mut bo3_shots: Vec<CreateFxOneshot> = Vec::new();
    for p in &fx.placed {
        // (A Black Ops III effect for this one: placed in its own list.)
        if let Some((bo3, _)) = picks.get(&p.fxid) {
            // (Its rise where the game's own effect rose: `bo3_vfx::bo3_angles`.)
            let rise = by_id
                .get(p.fxid.as_str())
                .and_then(|(_, n)| n.as_ref())
                .and_then(|n| found.get(&n.to_ascii_lowercase()))
                .map_or(super::bo3_vfx::Rise::Still, super::bo3_vfx::rise_of);
            bo3_shots.push(CreateFxOneshot {
                fxid: bo3.clone(),
                origin_inches: p.origin,
                angles_deg: super::bo3_vfx::bo3_angles(p.angles, rise),
                delay: 0.0,
                repeat_secs: if bo3.to_ascii_lowercase().contains("loop") { 0.0 } else { asset_audio::ghosts_repeat_secs(p.delay) },
            });
            continue;
        }
        let Some(name) = by_id.get(p.fxid.as_str()).and_then(|(_, n)| n.as_ref()) else {
            absent += 1;
            if !absent_ids.contains(&p.fxid) {
                absent_ids.push(p.fxid.clone());
            }
            continue;
        };
        let st = &stats[&name.to_ascii_lowercase()];
        // (Something to show: a sprite that linked, or an effect it runs.)
        let playable =
            st.sprites_ok > 0 || !children(&found[&name.to_ascii_lowercase()]).is_empty();
        if !playable {
            empty += 1;
            if !empty_ids.contains(&p.fxid) {
                empty_ids.push(p.fxid.clone());
            }
            continue;
        }
        played += 1;
        shots.push(CreateFxOneshot {
            fxid: name.clone(),
            origin_inches: p.origin,
            angles_deg: p.angles,
            // (The createfx delay is a repeat time, not a pre-roll:
            // `t6_m2::map_scripts`.)
            delay: 0.0,
            repeat_secs: repeat_secs(
                asset_game::iw6_createfx_kind_on(),
                found[&name.to_ascii_lowercase()].loops(),
                p.delay,
            ),
        });
    }
    let mut scriptable_played = 0usize;
    for (name, origin, repeat) in &scriptable_places {
        let key = name.to_ascii_lowercase();
        let (Some(st), Some(f)) = (stats.get(&key), found.get(&key)) else { continue };
        if st.sprites_ok == 0 && children(f).is_empty() {
            continue;
        }
        scriptable_played += 1;
        shots.push(CreateFxOneshot {
            fxid: name.clone(),
            origin_inches: *origin,
            angles_deg: [270.0, 0.0, 0.0],
            delay: 0.0,
            repeat_secs: *repeat,
        });
    }
    if scriptable_total > 0 {
        report.push(format!("iw6 scriptable fx: {scriptable_played} placements played"));
    }
    // The exploders' effects, by the name the catalog holds them under.
    if let Some(mut pack) = asset_world::iw6_scripts().map(|p| (*p).clone()) {
        let mut named = 0usize;
        for e in &mut pack.exploders {
            if let Some(n) = exploder_effects.get(&e.fxid) {
                e.effect = n.clone();
                named += 1;
            }
        }
        for (path, name) in &mut pack.script_fx {
            if let Some(n) = script_effects.get(path) {
                name.clone_from(n);
            }
        }
        report.push(format!(
            "iw6 exploders: {} of {} placements have their effect converted ({} kinds)",
            named,
            pack.exploders.len(),
            exploder_effects.len()
        ));
        asset_world::set_iw6_scripts(Some(pack));
    }
    let sprites: usize = stats.values().map(|s| s.sprites).sum();
    let sprites_ok: usize = stats.values().map(|s| s.sprites_ok).sum();
    let models: usize = stats.values().map(|s| s.models).sum();
    let unsupported: usize = stats.values().map(|s| s.unsupported).sum();
    report.push(format!(
        "iw6 createfx {}: {} placed ({} kinds), {} played, {} with nothing drawable, {} with no effect found; {} exploder and {} sound placements not played; {} effects converted ({} named by them, {} sprite visuals of which {} linked, {} model visuals and {} unsupported elements left out); effects named but in no zone read: {:?}; with nothing drawable: {:?}",
        fx.script,
        fx.placed.len(),
        by_id.len(),
        played,
        empty,
        absent,
        fx.exploders,
        fx.sounds,
        views.len(),
        by_id.values().filter(|(_, n)| n.is_some()).count(),
        sprites,
        sprites_ok,
        models,
        unsupported,
        absent_ids,
        empty_ids
    ));
    let mut m: Vec<_> = missing_children.into_iter().collect();
    m.sort();
    if !m.is_empty() {
        report.push(format!(
            "iw6 createfx: {} child effects in no zone read: {:?}",
            m.len(),
            &m[..m.len().min(12)]
        ));
    }
    // ONE line per map: every effect the map names that did not resolve, by source, and what
    // resolved but has nothing drawable (the owner's log shows which effects are missing).
    {
        let mut un_script = unresolved_script;
        un_script.sort();
        un_script.dedup();
        let mut un_scriptable = unresolved_scriptable;
        un_scriptable.sort();
        un_scriptable.dedup();
        let mut un_exploder = unresolved_exploder_pending;
        un_exploder.sort();
        un_exploder.dedup();
        let mut absent_paths: Vec<String> = absent_ids
            .iter()
            .map(|id| match fx.paths.get(id) {
                Some(p) => format!("{id} ({p})"),
                None => id.clone(),
            })
            .collect();
        absent_paths.sort();
        report.push(format!(
            "iw6 fx NOT RESOLVED {}: createfx ids {} {:?}; script paths {} {:?}; exploder ids {} {:?}; scriptable effects {} {:?}; child effects {} {:?}; with nothing drawable {} {:?}; sprite materials not linked {} {:?}",
            fx.script,
            absent_paths.len(),
            absent_paths,
            un_script.len(),
            un_script,
            un_exploder.len(),
            un_exploder,
            un_scriptable.len(),
            un_scriptable,
            m.len(),
            m,
            empty_ids.len(),
            empty_ids,
            unlinked_materials.len(),
            unlinked_materials
        ));
    }
    if let Some(l) = vfx_line {
        report.push(l);
    }
    {
        let mut placed_fx: Vec<&OwnedFx> = found.values().collect();
        placed_fx.sort_by(|a, b| a.name.cmp(&b.name));
        let once = shots.iter().filter(|s| s.repeat_secs == 0.0).count();
        report.push(rules_line(&placed_fx, once, shots.len() - once));
    }
    if !bo3_shots.is_empty() {
        report.push(format!("iw6 vfx substitution: {} Black Ops III effects (and what they run) read for {} placements", bo3_views.len(), bo3_shots.len()));
        super::t7_fx::set_pending(Some((bo3_views, bo3_shots, Vec::new())));
    }
    set_pending(Some(GuestMapFx {
        models: fx_models,
        views,
        shots,
        material_ns: AssetNamespace::Iw6,
    }));
}

/// bo2mp: the offline effect harness' conversion (`render_fx`'s
/// `iw6_fx_shot` example): Ghosts effects through the same element
/// conversion the createfx lane uses, with the caller's material lookup
/// (`material` returns the name to link, `None` leaves the visual out).
/// Returns the views and, per effect name, `(sprite visuals, linked,
/// model visuals, unsupported elements)`.
pub fn harness_views(
    fx: &[OwnedFx],
    material: &mut dyn FnMut(&str) -> Option<String>,
) -> (Vec<S1FxView>, HashMap<String, (usize, usize, usize, usize)>) {
    let mut views = Vec::new();
    let mut tally = HashMap::new();
    for f in fx {
        let mut st = Stats::default();
        let elems = f.elems.iter().map(|el| elem(el, material, &mut |_| None, &mut st)).collect();
        views.push(S1FxView {
            name: f.name.clone(),
            header: f.header.clone(),
            elems,
            h1: false,
        });
        tally.insert(f.name.clone(), (st.sprites, st.sprites_ok, st.models, st.unsupported));
    }
    (views, tally)
}

#[cfg(test)]
mod fx_draw_tests {
    use super::*;
    use asset_core::{T6Blend, T6Draw};

    #[test]
    fn effect_techniques_blend_by_their_tokens() {
        let base = T6Draw::from_technique_set("x", 48, None);
        let d = |n: &str| fx_draw(n, base);
        assert_eq!(d("effect_zfeather_add_lin_nofog_ndw_bl_blmod").blend, T6Blend::Add);
        assert_eq!(d("effect_zfeather_blend_lin_ndw_bl_abl").blend, T6Blend::Blend);
        // the lit-looking `replace` of the cloud techniques is an alpha blend (state 13/14)
        let c = d("particle_cloud_outdoor_replace_bl_abl_lin");
        assert_eq!(c.blend, T6Blend::Blend);
        assert!(c.unlit && c.effect);
        assert_eq!(d("effect_zfeather_falloff_screen_lin_nofog_ndw_bl").blend, T6Blend::Add);
        // not an effect technique: unchanged
        assert_eq!(fx_draw("wc_l_sm_r0c0", base).blend, base.blend);
    }
}

#[cfg(test)]
mod cloud_switch_tests {
    use super::{aw_elem_type, aw_elem_type_with};

    #[test]
    fn ghosts_types_map_to_aw() {
        assert_eq!(aw_elem_type(0), 0);
        assert_eq!(aw_elem_type(2), 3, "tail");
        assert_eq!(aw_elem_type(3), 5, "trail");
        assert_eq!(aw_elem_type(4), 6, "flare");
        assert_eq!(aw_elem_type(10), 10, "spark fountain");
        assert_eq!(aw_elem_type(11), 11, "model");
        assert_eq!(aw_elem_type(99), 255);
    }

    #[test]
    fn clouds_are_off_unless_switched_on() {
        // The exe's draw handlers: 8 cloud, 9 spark cloud; off by default
        // since the owner's snow-map screenshot (giant radial sheets).
        assert_eq!(aw_elem_type_with(8, true), 8);
        assert_eq!(aw_elem_type_with(9, true), 9);
        assert_eq!(aw_elem_type_with(8, false), 255);
    }
}
