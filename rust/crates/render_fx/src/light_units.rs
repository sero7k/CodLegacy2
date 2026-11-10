//! bo2mp: effect lights in the map's units (`fx_iw4::fx_light_normalise`),
//! a muzzle flash's spot light dropped, and a line in the log, once per
//! namespace and kind of clamp, when a light was changed.

use std::collections::HashSet;
use std::sync::Mutex;

use asset_core::AssetNamespace;
use fx_iw4::{FxLightClampKind, FxLightUnits};

/// The units an effect of this game hands its lights over in.
pub fn effect_light_units(ns: AssetNamespace) -> FxLightUnits {
    match ns {
        AssetNamespace::T6 | AssetNamespace::T7 => FxLightUnits::T6Hdr,
        _ => FxLightUnits::Iw,
    }
}

/// The units a map's lighting expects of an effect light, by the game the
/// map came from (`assets::map_game`, else the effect catalog's map
/// namespace). Black Ops II's maps light at their own exposure (2^-exposure,
/// about a sixteenth: `lane::t6`), Black Ops' at its sunParse exposure;
/// every other game's map (CoD4, MW2, MW3, Ghosts, MWR, IW, BO3, read into
/// Black Ops II's lighting by their lanes) draws at exposure 1
/// (`exposure_scale: 1.0`), as does IW4's own pipeline (MW2 / MW3 loaded
/// natively): IW units there.
pub fn map_light_units(map_game: Option<AssetNamespace>, catalog_ns: AssetNamespace) -> FxLightUnits {
    match map_game.unwrap_or(catalog_ns) {
        AssetNamespace::T6 | AssetNamespace::T5 => FxLightUnits::T6Hdr,
        _ => FxLightUnits::Iw,
    }
}

static WEAPON_FLASH_FX: Mutex<Option<HashSet<String>>> = Mutex::new(None);

/// A weapon's muzzle flash effect, as it is played (its lights then count as
/// a muzzle flash's whatever its name).
pub fn note_weapon_flash_fx(name: &str) {
    let name = name.to_ascii_lowercase();
    if let Ok(mut set) = WEAPON_FLASH_FX.lock() {
        let set = set.get_or_insert_with(HashSet::new);
        if !set.contains(&name) {
            set.insert(name);
        }
    }
}

/// The effects a weapon's flash plays under it (runners, emitted, on death
/// and on impact), down to `depth` levels: each counts as the flash itself.
fn flash_children<'a>(
    catalog: &'a asset_game::FxDefinitions,
    ns: AssetNamespace,
    name: &str,
    depth: u8,
    out: &mut Vec<&'a asset_game::OwnedFxEffectDef>,
) {
    let Some(def) = catalog.get_in(ns, name) else { return };
    if out.iter().any(|d| std::ptr::eq(*d, def)) {
        return;
    }
    out.push(def);
    if depth == 0 {
        return;
    }
    for elem in &def.elems {
        let runners = elem.visuals.iter().filter_map(|v| match v {
            asset_game::OwnedFxVisual::Runner { hint, .. } => hint.as_deref(),
            _ => None,
        });
        let hints = [
            elem.effect_emitted_hint.as_deref(),
            elem.effect_on_death_hint.as_deref(),
            elem.effect_on_impact_hint.as_deref(),
        ];
        for child in runners.chain(hints.into_iter().flatten()) {
            if !child.is_empty() {
                flash_children(catalog, def.namespace, child, depth - 1, out);
            }
        }
    }
}

static FIRST_SHOT: Mutex<Option<HashSet<(u32, String)>>> = Mutex::new(None);

/// bo2mp: a weapon's flash effect and every effect it plays under it noted
/// as the flash (`note_weapon_flash_fx`: muzzle light caps; for MWR / HMW
/// also the first-person sprite cap, `present`). The MWR / HMW flashes run
/// children with names of their own (`flash_smoke_fv_child`) that the name
/// test does not know: before, those drew uncapped (the red mist). On the
/// first own shot of each gun, one line naming every effect, element,
/// material, size and colour it plays.
pub fn note_weapon_flash_tree(
    catalog: &asset_game::FxDefinitions,
    ns: AssetNamespace,
    name: &str,
    weapon: u32,
    own_gun: bool,
) {
    let mut defs = Vec::new();
    flash_children(catalog, ns, name, 3, &mut defs);
    note_weapon_flash_fx(name);
    for def in &defs {
        note_weapon_flash_fx(&def.name);
    }
    if !own_gun {
        return;
    }
    let first = FIRST_SHOT.lock().ok().is_some_and(|mut set| {
        set.get_or_insert_with(HashSet::new)
            .insert((weapon, name.to_ascii_lowercase()))
    });
    if !first {
        return;
    }
    let mut parts = Vec::new();
    for def in &defs {
        let elems: Vec<String> = def
            .elems
            .iter()
            .map(|elem| {
                let v = &elem.view;
                let n = v.vis_state_interval_count;
                let size = fx_iw4::evaluate_size0(&elem.vis_samples, n, 0.0, 1.0);
                let colour = fx_iw4::evaluate_color_bgra(&elem.vis_samples, n, 0.0, 1.0)
                    .map(|[b, g, r, a]| [r, g, b, a]);
                let material = match elem.primary_material() {
                    Some((_, m)) => m.to_owned(),
                    None => {
                        let named = elem.visuals.iter().find_map(|vis| match vis {
                            asset_game::OwnedFxVisual::Material { hint, .. } => hint.clone(),
                            _ => None,
                        });
                        match named {
                            Some(m) => format!("{m} (unbound: not drawn)"),
                            None => "-".to_owned(),
                        }
                    }
                };
                format!(
                    "type {} {material} size {} rgba {} flags {:#x}",
                    v.elem_type,
                    size.map_or_else(|| "?".to_owned(), |s| format!("{s:.1}")),
                    colour.map_or_else(|| "?".to_owned(), |c| format!("{c:?}")),
                    v.flags,
                )
            })
            .collect();
        parts.push(format!("{} {:?} [{}]", def.name, def.namespace, elems.join("; ")));
    }
    diag::info!(
        World,
        "first shot of weapon {weapon}: flash {ns:?} {name} plays {} effects: {}",
        defs.len(),
        parts.join(" | ")
    );
}

/// Whether this effect is a weapon's muzzle flash.
pub fn is_weapon_flash_fx(name: &str) -> bool {
    if fx_iw4::fx_is_muzzle_flash_name(name) {
        return true;
    }
    let name = name.to_ascii_lowercase();
    WEAPON_FLASH_FX
        .lock()
        .ok()
        .is_some_and(|set| set.as_ref().is_some_and(|set| set.contains(&name)))
}

static FIRST_FLASH: Mutex<Option<HashSet<(AssetNamespace, &'static str)>>> = Mutex::new(None);

/// One line in the log for the first muzzle flash sprite / light each game's
/// guns draw (`what`), with its asset names, size, radius or colour.
pub fn log_first_flash(effect_ns: AssetNamespace, what: &'static str, line: impl FnOnce() -> String) {
    let first = FIRST_FLASH.lock().ok().is_some_and(|mut set| {
        set.get_or_insert_with(HashSet::new).insert((effect_ns, what))
    });
    if first {
        let line = line();
        diag::info!(World, "first {effect_ns:?} muzzle flash {what}: {line}");
    }
}

static FLASH_SPRITES: Mutex<Option<HashSet<(AssetNamespace, String)>>> = Mutex::new(None);

/// Whether this is the first time a gun flash of `effect_ns` draws (or
/// leaves out) a sprite of `material`, noting it.
fn first_flash_sprite(effect_ns: AssetNamespace, material: &str) -> bool {
    FLASH_SPRITES.lock().ok().is_some_and(|mut set| {
        set.get_or_insert_with(HashSet::new)
            .insert((effect_ns, material.to_ascii_lowercase()))
    })
}

/// One line in the log the first time a gun flash of each game draws a
/// sprite of each material (`first H1 muzzle flash sprite: <effect> /
/// <material> half .. -> .. at .. rgba [..] drawn`, or `... lens overlay,
/// not drawn`): which picture a shot puts on screen, and how big.
pub fn log_flash_sprite(effect_ns: AssetNamespace, material: &str, line: impl FnOnce() -> String) {
    if first_flash_sprite(effect_ns, material) {
        let line = line();
        diag::info!(World, "first {effect_ns:?} muzzle flash sprite: {line}");
    }
}

static LOGGED: Mutex<Option<HashSet<(AssetNamespace, &'static str)>>> = Mutex::new(None);

fn log_once(effect_ns: AssetNamespace, map_ns: AssetNamespace, def_name: &str, what: &'static str, before: f32, after: f32) {
    let first = LOGGED.lock().ok().is_some_and(|mut set| {
        set.get_or_insert_with(HashSet::new).insert((effect_ns, what))
    });
    if first {
        diag::info!(
            World,
            "fx light clamp: {effect_ns:?} effect on {map_ns:?} map, {what} {before:.2} -> {after:.2} ({def_name}; once per namespace)"
        );
    }
}

/// The light as the map should add it: `None` drops it (a muzzle flash's
/// spot light, or a light that adds nothing). `CODLEGACY_FX_LIGHT_RAW=1` hands
/// lights over untouched (to compare).
pub fn normalise_effect_light(
    effect_ns: AssetNamespace,
    map_ns: AssetNamespace,
    map_units: FxLightUnits,
    def_name: &str,
    is_spot: bool,
    color_bgr: [f32; 3],
    radius: f32,
) -> Option<([f32; 3], f32)> {
    static RAW: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    if *RAW.get_or_init(|| codlegacy_env::var_os("CODLEGACY_FX_LIGHT_RAW").is_some()) {
        return Some((color_bgr, radius));
    }
    let muzzle = is_weapon_flash_fx(def_name);
    if !fx_iw4::fx_light_spot_allowed(is_spot, muzzle) {
        log_once(effect_ns, map_ns, def_name, "muzzle spot light dropped", radius, 0.0);
        return None;
    }
    let n = fx_iw4::fx_light_normalise(
        color_bgr,
        radius,
        effect_light_units(effect_ns),
        map_units,
        muzzle,
    )?;
    if let Some(note) = n.note {
        let what = match (note.kind, muzzle) {
            (FxLightClampKind::Units, _) => "units",
            (FxLightClampKind::Peak, true) => "muzzle peak",
            (FxLightClampKind::Peak, false) => "peak",
            (FxLightClampKind::Radius, true) => "muzzle radius",
            (FxLightClampKind::Radius, false) => "radius",
        };
        log_once(effect_ns, map_ns, def_name, what, note.before, note.after);
    }
    if muzzle {
        log_first_flash(
            effect_ns,
            "light",
            || format!(
                "{def_name} radius {radius:.0} -> {:.0}, colour bgr [{:.3} {:.3} {:.3}] -> [{:.3} {:.3} {:.3}]",
                n.radius, color_bgr[0], color_bgr[1], color_bgr[2], n.color_bgr[0], n.color_bgr[1], n.color_bgr[2]
            ),
        );
    }
    Some((n.color_bgr, n.radius))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bo2_flash_on_mw3_map_dimmed() {
        let (c, _) = normalise_effect_light(
            AssetNamespace::T6,
            AssetNamespace::Iw5,
            map_light_units(Some(AssetNamespace::Iw5), AssetNamespace::T6),
            "weapon/muzzleflashes/fx_muz_ar_hip_flash",
            false,
            [32.0; 3],
            250.0,
        )
        .unwrap();
        assert!(c[0] < 3.0);
    }

    #[test]
    fn each_flash_sprite_material_logged_once_per_game() {
        let m = "fx_test_flash_sprite_once_add";
        assert!(first_flash_sprite(AssetNamespace::H1, m));
        assert!(!first_flash_sprite(AssetNamespace::H1, &m.to_ascii_uppercase()));
        assert!(first_flash_sprite(AssetNamespace::T6, m));
        assert!(first_flash_sprite(AssetNamespace::H1, "fx_test_flash_sprite_other_add"));
    }

    #[test]
    fn map_units_by_game() {
        // A packed MW3 map: the catalog says Black Ops II, the game MW3.
        assert_eq!(map_light_units(Some(AssetNamespace::Iw5), AssetNamespace::T6), FxLightUnits::Iw);
        assert_eq!(map_light_units(Some(AssetNamespace::H1), AssetNamespace::T6), FxLightUnits::Iw);
        assert_eq!(map_light_units(Some(AssetNamespace::T6), AssetNamespace::T6), FxLightUnits::T6Hdr);
        assert_eq!(map_light_units(None, AssetNamespace::T6), FxLightUnits::T6Hdr);
        assert_eq!(map_light_units(None, AssetNamespace::Iw4), FxLightUnits::Iw);
        assert_eq!(map_light_units(Some(AssetNamespace::T5), AssetNamespace::T5), FxLightUnits::T6Hdr);
    }

    #[test]
    fn muzzle_spot_dropped_flashlight_kept() {
        assert!(normalise_effect_light(AssetNamespace::H1, AssetNamespace::T6, FxLightUnits::T6Hdr, "vfx/muzzleflash/x_view", true, [1.0; 3], 200.0).is_none());
        assert!(normalise_effect_light(AssetNamespace::Iw4, AssetNamespace::Iw4, FxLightUnits::Iw, "misc/flashlight", true, [1.0; 3], 200.0).is_some());
        note_weapon_flash_fx("Weapon/Custom/odd_name");
        assert!(is_weapon_flash_fx("weapon/custom/odd_name"));
        assert!(normalise_effect_light(AssetNamespace::S1, AssetNamespace::T6, FxLightUnits::T6Hdr, "weapon/custom/odd_name", true, [1.0; 3], 200.0).is_none());
    }
}
