//! bo2mp: Ghosts guns hit and trace with Ghosts' own effects.
//!
//! * **Impacts** (`BO2MP_IW6_OWN_IMPACTS`, on): a Ghosts gun's bullet hits
//!   play the cells of Ghosts' impact table (`ImpactFx`, read from
//!   `common_mp`) on the row of the gun's own impact type
//!   (`fx_iw4::iw6::weapon::impact_rows`: the row going in, the alternate
//!   row coming out), each Black Ops II surface on its Ghosts column
//!   (`impact_column`), the four flesh cells after. The effects (and every
//!   effect they run) are converted as a Ghosts map's are (`iw6_fx`) and
//!   join the match's effects under the Ghosts namespace; a cell whose
//!   effect did not convert plays the match's table as before.
//! * **Tracers** (`BO2MP_IW6_OWN_TRACERS`, on): the gun's `TracerDef`
//!   (`common_core_mp`'s, its numbers as read: draw interval, speed, beam
//!   length and width, screw, five colours) with its material
//!   (`gfx_tracer_new`), so the gun's tracer hint finds it; without, the
//!   gun takes a Black Ops II gun's tracer of its class. `gfx_tracer_new` is
//!   in neither `common_mp` nor `common_core_mp` (it is `code_post_gfx_mp`'s,
//!   whose walk stops early): with `BO2MP_IW6_TRACER_STANDIN` (on) the
//!   tracer keeps Ghosts' numbers and draws with Black Ops II's tracer
//!   material.
//!
//! Black Ops II's guns are not touched: they keep the match's table and
//! their own tracers.

use std::collections::{BTreeMap, BTreeSet};

use asset_core::AssetNamespace;
use asset_game::{FxCatalog, GuestImpacts, TracerCatalog};
use fx_iw4::iw6::weapon::{IMPACT_COLUMNS, IMPACT_FLESH_COLUMN, impact_column, impact_rows, is_bullet_impact};

use super::iw6::Builder;

/// Our impact cells: the non-flesh surfaces, then the four flesh cells
/// (`WeaponCombatFx::impact_row`'s layout).
const SURFACES: usize = fx_iw4::FX_IMPACT_NONFLESH_COUNT;
const FLESH: usize = fx_iw4::FX_IMPACT_FLESH_COUNT;

pub(super) fn own_impacts_on() -> bool {
    asset_audio::iw6_rules::switch("OWN_IMPACTS", true)
}

pub(super) fn own_tracers_on() -> bool {
    asset_audio::iw6_rules::switch("OWN_TRACERS", true)
}

/// `BO2MP_IW6_ADS_FLASH` (on): the gun's view flash while fully aimed and
/// its view magazine eject (Ghosts' `fxType` 8 and 7) are converted and
/// bound to their slots.
pub(super) fn ads_flash_on() -> bool {
    asset_audio::iw6_rules::switch("ADS_FLASH", true)
}

/// `BO2MP_IW6_TRACER_STANDIN` (on): a tracer whose material is in no zone
/// read draws with Black Ops II's tracer material, its numbers Ghosts'.
pub(super) fn tracer_standin_on() -> bool {
    asset_audio::iw6_rules::switch("TRACER_STANDIN", true)
}

/// One row of Ghosts' table in our cell layout (empty names where the cell
/// is empty).
pub(super) fn row_cells(cells: &[Option<String>], row: usize) -> Vec<String> {
    let cell = |col: usize| {
        cells
            .get(row * IMPACT_COLUMNS + col)
            .cloned()
            .flatten()
            .map(|n| n.trim_start_matches(',').to_owned())
            .unwrap_or_default()
    };
    (0..SURFACES)
        .map(|surf| cell(impact_column(surf)))
        .chain((0..FLESH).map(|k| cell(IMPACT_FLESH_COLUMN + k)))
        .collect()
}

fn key(name: &str) -> String {
    name.trim_start_matches(',').to_ascii_lowercase()
}

/// Whether an effect (or one it runs) has a sprite whose material linked.
pub(super) fn drawable(views: &[asset_game::S1FxView], name: &str, depth: u32) -> bool {
    use asset_game::S1FxVisual as V;
    if depth > 8 {
        return false;
    }
    let Some(v) = views.iter().find(|v| key(&v.name) == name) else {
        return false;
    };
    v.elems.iter().any(|el| {
        el.visuals.iter().any(|x| match x {
            V::Material(_) => true,
            V::Effect(c) => drawable(views, &key(c), depth + 1),
            _ => false,
        }) || [&el.effect_on_impact, &el.effect_on_death, &el.effect_emitted]
            .into_iter()
            .flatten()
            .any(|c| drawable(views, &key(c), depth + 1))
    })
}

/// What the converter built for the guns.
#[derive(Default)]
pub(super) struct GunFx {
    pub fx: FxCatalog,
    pub tracers: TracerCatalog,
    /// Per gun (the catalog's name, `ak12`): its impact cells.
    pub impacts: BTreeMap<String, GuestImpacts>,
    pub report: Vec<String>,
}

/// The Ghosts weapon impact type a gun's complete definition holds.
fn impact_type_of(b: &Builder, w: &fastfile_iw6::walk::WeaponRec) -> i32 {
    use fastfile_iw6::layout::WeaponCompleteDef as c;
    w.complete
        .get(b.image)
        .get(c::impactType..c::impactType + 4)
        .map_or(0, |d| i32::from_le_bytes([d[0], d[1], d[2], d[3]]))
}

/// Builds the guns' own impacts and tracers (see the module docs). `b`
/// must hold the shared zones (`Builder::shared`).
pub(super) fn build(b: &mut Builder, guns: &[&fastfile_iw6::walk::WeaponRec]) -> GunFx {
    let mut out = GunFx::default();
    out.fx.set_capture_ns(AssetNamespace::Iw6);
    out.fx.set_capture_zone(b.owner);
    out.tracers.set_capture_ns(AssetNamespace::Iw6);
    out.tracers.set_capture_zone(b.owner);
    let sw = |on: bool| if on { "on" } else { "off" };
    let (impacts_on, tracers_on) = (own_impacts_on(), own_tracers_on());
    let shared = b.shared.clone();
    if shared.is_none() && (impacts_on || tracers_on) {
        out.report.push("iw6 guest guns: own impacts and tracers: the shared zones (common_mp) are not beside common_core_mp; Black Ops II's play".into());
        return out;
    }

    // Impacts: per gun its rows' cells.
    let mut rows_by_type: BTreeMap<i32, usize> = BTreeMap::new();
    if impacts_on && let Some(table) = shared.as_ref().and_then(|s| s.impact_table()) {
        let mut plan: Vec<(String, Vec<String>, Vec<String>)> = Vec::new();
        for w in guns {
            let t = impact_type_of(b, w);
            let Some((enter, exit)) = impact_rows(t).filter(|_| is_bullet_impact(t)) else {
                continue;
            };
            *rows_by_type.entry(t).or_default() += 1;
            let n = w.name.to_ascii_lowercase();
            let n = n.strip_suffix("_mp").unwrap_or(&n);
            let gun = n.strip_prefix("iw6_").unwrap_or(n).to_owned();
            plan.push((gun, row_cells(&table.cells, enter), row_cells(&table.cells, exit)));
        }
        let roots: BTreeSet<String> = plan.iter().flat_map(|(_, a, b)| a.iter().chain(b)).filter(|n| !n.is_empty()).cloned().collect();
        let roots: Vec<String> = roots.into_iter().collect();
        let (views, sprites, linked, missing) = super::iw6_fx::gun_effects(b, &roots);
        let mut captured: BTreeSet<String> = BTreeSet::new();
        for v in &views {
            if out.fx.capture_s1_as(AssetNamespace::Iw6, AssetNamespace::Iw6, v) {
                captured.insert(key(&v.name));
            }
        }
        // (A cell whose effect did not convert, or shows nothing - its
        // pictures in a pak not here - falls back to the table.)
        let converted: BTreeSet<String> = captured.iter().filter(|n| drawable(&views, n, 0)).cloned().collect();
        let keep = |cells: Vec<String>| -> Vec<String> {
            cells.into_iter().map(|n| if converted.contains(&n.to_ascii_lowercase()) { n } else { String::new() }).collect()
        };
        for (gun, enter, exit) in plan {
            let (enter, exit) = (keep(enter), keep(exit));
            if enter.iter().chain(&exit).any(|n| !n.is_empty()) {
                out.impacts.insert(gun, GuestImpacts { namespace: AssetNamespace::Iw6, enter, exit, types: u32::MAX });
            }
        }
        out.report.push(format!(
            "iw6 guest guns: own impacts {} (BO2MP_IW6_OWN_IMPACTS): table {:?} from the shared zones, {} guns by impact type {:?}, {} effects named, {} of {} converted show something ({} sprite visuals, {} linked), not found {:?}",
            sw(true),
            table.name,
            out.impacts.len(),
            rows_by_type,
            roots.len(),
            converted.len(),
            captured.len(),
            sprites,
            linked,
            &missing[..missing.len().min(6)]
        ));
    } else {
        out.report.push(format!(
            "iw6 guest guns: own impacts {} (BO2MP_IW6_OWN_IMPACTS){}",
            sw(impacts_on),
            if impacts_on { ": no impact table in the shared zones; the match's table plays" } else { "" }
        ));
    }

    // The view flash while fully aimed (fxType 8) and the view magazine
    // eject (fxType 7): the gun's own and its attachments' overrides.
    if ads_flash_on() {
        use fastfile_iw6::layout::WeaponDef as d;
        let mut roots: BTreeSet<String> = BTreeSet::new();
        for w in guns {
            for (o, id) in &w.fx {
                if *o == d::viewFlashADSEffect || *o == d::viewMagEjectEffect {
                    roots.insert(b.cap.name_of(*id).trim_start_matches(',').to_owned());
                }
            }
            for f in w.fx_overrides.iter().filter(|f| f.fx_type == 7 || f.fx_type == 8) {
                roots.extend(f.override_fx.map(|id| b.cap.name_of(id).trim_start_matches(',').to_owned()));
            }
        }
        roots.remove("");
        let roots: Vec<String> = roots.into_iter().collect();
        let (views, sprites, linked, missing) = super::iw6_fx::gun_effects(b, &roots);
        let mut captured = 0usize;
        for v in &views {
            captured += usize::from(out.fx.capture_s1_as(AssetNamespace::Iw6, AssetNamespace::Iw6, v));
        }
        let shows = roots.iter().filter(|r| drawable(&views, &key(r), 0)).count();
        out.report.push(format!(
            "iw6 guest guns: aimed flash and magazine eject slots on (BO2MP_IW6_ADS_FLASH): {} effects named ({} show something), {captured} converted with what they run ({sprites} sprite visuals, {linked} linked), not found {:?}",
            roots.len(),
            shows,
            &missing[..missing.len().min(6)]
        ));
    } else {
        out.report.push("iw6 guest guns: aimed flash and magazine eject slots off (BO2MP_IW6_ADS_FLASH=0)".into());
    }

    // Tracers.
    if tracers_on {
        let wanted: BTreeSet<String> = guns
            .iter()
            .filter_map(|w| w.tracer)
            .map(|id| b.cap.name_of(id).trim_start_matches(',').to_ascii_lowercase())
            .filter(|n| !n.is_empty())
            .collect();
        let mut got = Vec::new();
        let mut stood_in = Vec::new();
        let mut no_picture = Vec::new();
        let standin_on = tracer_standin_on();
        for name in &wanted {
            let t = shared
                .as_ref()
                .and_then(|s| s.tracer(name).cloned())
                .or_else(|| b.cap.tracers.get(name).or_else(|| b.cap.tracers.get(&format!(",{name}"))).cloned());
            let Some(t) = t.filter(|t| t.speed > 0.0) else {
                continue;
            };
            let Some(material) = t.material.as_deref() else {
                continue;
            };
            // Ghosts' material when a zone read holds it with a picture;
            // else (it is `code_post_gfx_mp`'s, which the walk does not
            // reach) Black Ops II's tracer material stands in.
            let own = b.material_named(material, 512).filter(|(_, slot, _)| {
                b.catalog.materials[*slot]
                    .textures
                    .iter()
                    .any(|x| x.semantic == asset_material::TS_COLOR_MAP && x.image.is_some())
            });
            let (canon, standin) = match own {
                Some((_, _, canon)) => (canon, false),
                None if standin_on => (material.trim_start_matches(',').to_owned(), true),
                None => {
                    no_picture.push(name.clone());
                    continue;
                }
            };
            if out.tracers.capture_native_in(
                AssetNamespace::Iw6,
                AssetNamespace::Iw6,
                name,
                &canon,
                t.draw_interval.max(0) as u32,
                t.speed,
                [t.beam_length, t.beam_width, t.screw_radius, t.screw_dist],
                t.colors,
                standin,
            ) {
                if standin {
                    stood_in.push(name.clone());
                } else {
                    got.push(name.clone());
                }
            }
        }
        out.report.push(format!(
            "iw6 guest guns: own tracers on (BO2MP_IW6_OWN_TRACERS): {} of {} named with Ghosts' material {:?}, {} with Ghosts' numbers and Black Ops II's tracer material (BO2MP_IW6_TRACER_STANDIN {}) {:?}; Black Ops II's tracer of the class {:?}",
            got.len(),
            wanted.len(),
            got,
            stood_in.len(),
            sw(standin_on),
            stood_in,
            no_picture
        ));
    } else {
        out.report.push("iw6 guest guns: own tracers off (BO2MP_IW6_OWN_TRACERS=0): Black Ops II's of the class".into());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_row_reads_in_our_surface_order() {
        // A row whose cell names its column.
        let cells: Vec<Option<String>> = (0..2 * IMPACT_COLUMNS).map(|i| Some(format!(",c{}", i % IMPACT_COLUMNS))).collect();
        let row = row_cells(&cells, 1);
        assert_eq!(row.len(), SURFACES + FLESH);
        assert_eq!(row[5], "c5"); // concrete
        assert_eq!(row[14], "c15"); // mud, one above
        assert_eq!(row[SURFACES], "c31"); // body
        assert_eq!(row[SURFACES + 3], "c34"); // head, fatal
    }
}
