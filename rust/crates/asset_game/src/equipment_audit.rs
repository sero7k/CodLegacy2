//! Black Ops II multiplayer equipment: which sound and effect each piece
//! names in its weapon file, and whether the loaded banks have it.
//!
//! The weapon file (`WeaponDef`) names an alias for each thing the engine
//! plays for a piece of equipment (pull-back, throw, bounce by surface,
//! projectile ignition, explosion, detonator key) and the animations'
//! notetracks name more (`notetrack_sound_map`); the effects name the
//! projectile's trail, ignition and explosion. This module walks all of
//! them for every piece of equipment and says for each: authored or not,
//! and present in the sound bank / effect catalog or not. The runtime logs
//! the table with `BO2MP_EQUIP_AUDIT=1` (one line per slot; see
//! `docs/EQUIPMENT_AUDIT.md`), and the pure parts are tested here.
//!
//! The sounds and lights Black Ops II's *client scripts* add (the Semtex's
//! beeping, the Shock Charge's mask crackle) are not in any weapon file;
//! [`ENGINE_SUPPLIED`] lists them so the same check covers them.

use crate::{AssetNamespace, FxDefinitions, WeaponRegistry, WeaponSoundAliases, WeaponSoundSlot};

/// The Semtex's (and the crossbow bolt's) beep, `_sticky_grenade.csc`.
pub const SEMTEX_ALERT: &str = "wpn_semtex_alert";
/// The stock explosion of the frag, the Semtex and the crossbow's bolt when
/// the weapon file's own alias is not in the bank.
pub const GRENADE_EXPLODE: &str = "wpn_grenade_explode";
/// The Shock Charge's burst when its weapon file names none.
pub const SHOCK_CHARGE_ZAP: &str = "wpn_taser_mine_zap";
/// The mask crackle a shocked player hears (`_proximity_grenade.csc`).
pub const SHOCK_CHARGE_TACMASK: &str = "wpn_taser_mine_tacmask";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EquipmentKind {
    Lethal,
    Tactical,
    /// Planted or placed by the player (mines, charges, spikes, sensors).
    Placeable,
    Other,
}

#[derive(Clone, Copy, Debug)]
pub struct EquipmentItem {
    pub weapon: &'static str,
    pub label: &'static str,
    pub kind: EquipmentKind,
}

const fn item(weapon: &'static str, label: &'static str, kind: EquipmentKind) -> EquipmentItem {
    EquipmentItem { weapon, label, kind }
}

/// Black Ops II multiplayer's lethal and tactical equipment, by the weapon
/// file's name. A name the loaded weapon data does not have is reported as
/// missing, not guessed at.
pub const BO2_MP_EQUIPMENT: &[EquipmentItem] = &[
    item("frag_grenade_mp", "Frag", EquipmentKind::Lethal),
    item("sticky_grenade_mp", "Semtex", EquipmentKind::Lethal),
    item("hatchet_mp", "Combat Axe / Tomahawk", EquipmentKind::Lethal),
    item("satchel_charge_mp", "C4 (and detonator)", EquipmentKind::Placeable),
    item("claymore_mp", "Claymore", EquipmentKind::Placeable),
    item("bouncingbetty_mp", "Bouncing Betty", EquipmentKind::Placeable),
    item("concussion_grenade_mp", "Concussion", EquipmentKind::Tactical),
    item("flash_grenade_mp", "Flash", EquipmentKind::Tactical),
    item("emp_grenade_mp", "EMP grenade", EquipmentKind::Tactical),
    item("sensor_grenade_mp", "Sensor grenade", EquipmentKind::Tactical),
    item("proximity_grenade_mp", "Shock Charge", EquipmentKind::Tactical),
    item("willy_pete_mp", "Smoke", EquipmentKind::Tactical),
    item("nightingale_mp", "Nightingale (decoy)", EquipmentKind::Tactical),
    item("trophy_system_mp", "Trophy System", EquipmentKind::Placeable),
    item("tactical_insertion_mp", "Tactical Insertion", EquipmentKind::Placeable),
    item("acoustic_sensor_mp", "Motion Sensor", EquipmentKind::Placeable),
    item("camera_spike_mp", "Camera Spike", EquipmentKind::Placeable),
    item("scrambler_mp", "Scrambler", EquipmentKind::Placeable),
    item("ballistic_knife_mp", "Ballistic Knife", EquipmentKind::Other),
];

/// One sound or light the engine's client scripts add to a piece of
/// equipment, which no weapon file names.
#[derive(Clone, Copy, Debug)]
pub struct EngineSupplied {
    pub weapon: &'static str,
    pub what: &'static str,
    pub alias: &'static str,
    pub source: &'static str,
}

pub const ENGINE_SUPPLIED: &[EngineSupplied] = &[
    EngineSupplied {
        weapon: "sticky_grenade_mp",
        what: "beep, 0.3 s shrinking to 0.08 s",
        alias: SEMTEX_ALERT,
        source: "_sticky_grenade.csc",
    },
    EngineSupplied {
        weapon: "explosive_bolt_mp",
        what: "beep (the crossbow's explosive bolt)",
        alias: SEMTEX_ALERT,
        source: "_sticky_grenade.csc",
    },
    EngineSupplied {
        weapon: "sticky_grenade_mp",
        what: "stock explosion when the weapon file's is missing",
        alias: GRENADE_EXPLODE,
        source: "weapon file projExplosionSound",
    },
    EngineSupplied {
        weapon: "frag_grenade_mp",
        what: "stock explosion when the weapon file's is missing",
        alias: GRENADE_EXPLODE,
        source: "weapon file projExplosionSound",
    },
    EngineSupplied {
        weapon: "proximity_grenade_mp",
        what: "stock burst when the weapon file's is missing",
        alias: SHOCK_CHARGE_ZAP,
        source: "weapon file projExplosionSound",
    },
    EngineSupplied {
        weapon: "proximity_grenade_mp",
        what: "mask crackle on the shocked player",
        alias: SHOCK_CHARGE_TACMASK,
        source: "_proximity_grenade.csc (client field `tazered`)",
    },
];

/// How a piece of equipment's client script is handled in this rebuild.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClientScript {
    /// Its rules run in Rust (`render_frontend::adapters::fx::equipment_lights`
    /// and friends): what they add is listed in the module.
    Hooked,
    /// Black Ops II's client script adds lights or effects this rebuild does
    /// not play, because no decompiled client script is staged to read them
    /// from (the repo links server scripts only; client `.csc` objects are
    /// skipped). Only the server script's own `playsound` / `playfx` plays.
    NotRun,
}

/// A piece of equipment whose look comes partly from a client script, with
/// words to search the loaded zones' script names for (not asset names: a
/// search needle that a script path would contain).
#[derive(Clone, Copy, Debug)]
pub struct ClientScriptItem {
    pub weapon: &'static str,
    pub needles: &'static [&'static str],
    pub state: ClientScript,
    pub adds: &'static str,
}

pub const CLIENT_SCRIPT_ITEMS: &[ClientScriptItem] = &[
    ClientScriptItem { weapon: "sticky_grenade_mp", needles: &["sticky"], state: ClientScript::Hooked, adds: "blink light, beep" },
    ClientScriptItem { weapon: "satchel_charge_mp", needles: &["satchel"], state: ClientScript::Hooked, adds: "steady light" },
    ClientScriptItem { weapon: "bouncingbetty_mp", needles: &["bouncingbetty", "betty"], state: ClientScript::Hooked, adds: "steady light" },
    ClientScriptItem { weapon: "proximity_grenade_mp", needles: &["proximity", "taser", "shock"], state: ClientScript::Hooked, adds: "scan light, mask crackle; NOT the victim's electrocution effect" },
    ClientScriptItem { weapon: "claymore_mp", needles: &["claymore"], state: ClientScript::NotRun, adds: "laser/lights" },
    ClientScriptItem { weapon: "trophy_system_mp", needles: &["trophy"], state: ClientScript::NotRun, adds: "lights, intercept effect" },
    ClientScriptItem { weapon: "sensor_grenade_mp", needles: &["sensor_grenade", "sensorgrenade"], state: ClientScript::NotRun, adds: "pulse/lights" },
    ClientScriptItem { weapon: "camera_spike_mp", needles: &["camera_spike", "cameraspike"], state: ClientScript::NotRun, adds: "lights" },
    ClientScriptItem { weapon: "acoustic_sensor_mp", needles: &["acoustic", "motion_sensor"], state: ClientScript::NotRun, adds: "lights, ping" },
    ClientScriptItem { weapon: "scrambler_mp", needles: &["scrambler"], state: ClientScript::NotRun, adds: "lights" },
    ClientScriptItem { weapon: "tactical_insertion_mp", needles: &["insertion"], state: ClientScript::NotRun, adds: "marker effect" },
    ClientScriptItem { weapon: "nightingale_mp", needles: &["nightingale", "decoy"], state: ClientScript::NotRun, adds: "decoy effects" },
];

/// For each item with a client script, the names among `script_names` (the
/// zones' compiled script names, as the loader keeps them) that look like
/// it, and what is missing: a `NotRun` item says so with the exact list of
/// scripts the owner's zones hold, or that none was found. One line each.
pub fn client_script_report<'a>(script_names: impl Iterator<Item = &'a str> + Clone) -> Vec<String> {
    CLIENT_SCRIPT_ITEMS
        .iter()
        .map(|item| {
            let mut found: Vec<&str> = script_names
                .clone()
                .filter(|name| {
                    let lower = name.to_ascii_lowercase();
                    lower.contains("clientscripts") && item.needles.iter().any(|n| lower.contains(n))
                })
                .collect();
            found.sort_unstable();
            found.dedup();
            let state = match item.state {
                ClientScript::Hooked => "hooked in Rust",
                ClientScript::NotRun => "NOT RUN: the lights/effects it adds are not played",
            };
            format!(
                "{} | client script ({}) | {state} | in the loaded zones: {}",
                item.weapon,
                item.adds,
                if found.is_empty() { "no script found".to_owned() } else { found.join(", ") }
            )
        })
        .collect()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    /// Named by the weapon file and present in the bank / catalog.
    Resolved,
    /// Named by the weapon file, absent from the bank / catalog: it cannot
    /// play (the owner's staged data lacks the asset).
    Missing,
    /// Named by the weapon file and in the bank, but no code plays this slot
    /// yet (see [`slot_is_played`]).
    NotPlayed,
    /// An animation note with no sound anywhere: not in the note, not in the
    /// weapon file's map. Right for a note that only drives the animation;
    /// a gap for a pin pull or a throw.
    NoSound,
}

/// Whether the game plays a weapon-file sound slot. Every slot the audit
/// lists is played: `projectileSound` is a flight loop that follows the
/// shot (`audio::projectile_loops`). Kept as a function so a slot that is
/// read but not yet played can be flagged again.
pub fn slot_is_played(_slot: WeaponSoundSlot) -> bool {
    true
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AuditRow {
    pub weapon: String,
    /// What the slot is (`ProjectileExplosion`, `Bounce[5]`, `Notetrack
    /// grenade_throw`, `Fx explosion`, ...).
    pub slot: String,
    pub asset: String,
    pub status: Status,
}

/// Every sound the weapon file names (all slots, each surface's bounce, each
/// notetrack's sound), checked with `has`. Slots the file leaves empty give
/// no row: whether an empty slot is right is the weapon file's business (the
/// first-person grenade sounds are mostly notetracks).
pub fn audit_sounds(
    weapon: &str,
    sounds: &WeaponSoundAliases,
    has: &dyn Fn(&str) -> bool,
) -> Vec<AuditRow> {
    let mut rows = Vec::new();
    let mut push = |slot: String, asset: &str, played: bool| {
        rows.push(AuditRow {
            weapon: weapon.to_owned(),
            slot,
            asset: asset.to_owned(),
            status: match (has(asset), played) {
                (false, _) => Status::Missing,
                (true, true) => Status::Resolved,
                (true, false) => Status::NotPlayed,
            },
        });
    };
    for slot in WeaponSoundSlot::ALL {
        if let Some(alias) = sounds.hint(slot).filter(|a| !a.is_empty()) {
            push(format!("{slot:?}"), alias, slot_is_played(slot));
        }
    }
    for (surface, alias) in sounds.bounce.iter().enumerate() {
        if let Some(alias) = alias.as_deref().filter(|a| !a.is_empty()) {
            push(format!("Bounce[{surface}]"), alias, true);
        }
    }
    // (A Black Ops II map value may name the player's alias then the
    // others': each is its own row.)
    for (note, value) in &sounds.notetrack_sound_map {
        for (i, alias) in crate::notetrack_value_aliases(value).enumerate() {
            let who = if i == 0 { "" } else { " (others)" };
            push(format!("Notetrack {note}{who}"), alias, true);
        }
    }
    rows
}

/// The effects a thrown piece of equipment plays, checked with `has`.
pub fn audit_fx(weapon: &str, fx: &[(&str, Option<&str>)], has: &dyn Fn(&str) -> bool) -> Vec<AuditRow> {
    fx.iter()
        .filter_map(|(slot, name)| Some((*slot, (*name)?)))
        .filter(|(_, name)| !name.is_empty())
        .map(|(slot, name)| AuditRow {
            weapon: weapon.to_owned(),
            slot: format!("Fx {slot}"),
            asset: name.to_owned(),
            status: if has(name) { Status::Resolved } else { Status::Missing },
        })
        .collect()
}

/// Rows for the engine-supplied sounds of `weapon`.
pub fn audit_engine_supplied(weapon: &str, has: &dyn Fn(&str) -> bool) -> Vec<AuditRow> {
    ENGINE_SUPPLIED
        .iter()
        .filter(|e| e.weapon == weapon)
        .map(|e| AuditRow {
            weapon: weapon.to_owned(),
            slot: format!("Script `{}` ({})", e.source, e.what),
            asset: e.alias.to_owned(),
            status: if has(e.alias) { Status::Resolved } else { Status::Missing },
        })
        .collect()
}

/// The notetracks of a weapon's animations, each said what it does: a note
/// that names its own sound (`sndnt#alias`) or is mapped by the weapon file
/// is checked in the bank; a rumble is listed; a note the engine reads
/// itself is skipped; any other name with no sound is listed `NoSound`, the
/// precise list of notes that make no noise (pin pull and throw must not be
/// among them for a grenade). `mapped` says what the resolved weapon has for
/// a note's lowercase name.
pub fn audit_notetracks(
    weapon: &str,
    notes: &[String],
    mapped: &dyn Fn(&str) -> Option<String>,
    has: &dyn Fn(&str) -> bool,
) -> Vec<AuditRow> {
    use crate::NotetrackKind;
    let mut rows = Vec::new();
    for note in notes {
        let (slot, alias, status) = match crate::classify_notetrack(note) {
            NotetrackKind::Engine => continue,
            NotetrackKind::Rumble(alias) => (
                format!("Anim note {note} (rumble)"),
                alias.to_owned(),
                Status::Resolved,
            ),
            NotetrackKind::Sound(alias) => (
                format!("Anim note {note}"),
                alias.to_owned(),
                if has(alias) { Status::Resolved } else { Status::Missing },
            ),
            NotetrackKind::Named => match mapped(&note.to_ascii_lowercase()) {
                Some(alias) => {
                    let status = if has(&alias) { Status::Resolved } else { Status::Missing };
                    (format!("Anim note {note} (weapon file map)"), alias, status)
                }
                None => (format!("Anim note {note}"), String::new(), Status::NoSound),
            },
        };
        rows.push(AuditRow { weapon: weapon.to_owned(), slot, asset: alias, status });
    }
    rows
}

/// The weapon file's further effect slots: the explosion effects after the
/// first (`projExplosionEffect2` to `5`; the loader keeps the ones the file
/// named, in order, so the label counts those, not the file's slot number)
/// and `dud` (`projDudEffect`).
pub fn extra_fx_slots(combat: &crate::WeaponCombatFx) -> Vec<(String, &str)> {
    let mut out: Vec<(String, &str)> = combat
        .explosion_extra_hints
        .iter()
        .enumerate()
        .map(|(i, name)| (format!("explosion-extra#{}", i + 1), name.as_str()))
        .collect();
    if let Some(dud) = combat.dud_hint.as_deref() {
        out.push(("dud".to_owned(), dud));
    }
    out
}

/// What the registry has for the equipment list. Returns the rows and the
/// names of listed equipment the weapon data does not have.
pub fn audit_registry(
    weapons: &WeaponRegistry,
    has_alias: &dyn Fn(AssetNamespace, &str) -> bool,
    fx: &FxDefinitions,
    xanims: Option<&crate::XAnimCatalog>,
) -> (Vec<AuditRow>, Vec<&'static str>) {
    let mut rows = Vec::new();
    let mut absent = Vec::new();
    for item in BO2_MP_EQUIPMENT {
        let bare = item.weapon.strip_suffix("_mp").unwrap_or(item.weapon);
        let index = [item.weapon, bare]
            .into_iter()
            .find_map(|name| weapons.resolve_index(name).ok().flatten());
        let Some(index) = index else {
            absent.push(item.weapon);
            continue;
        };
        let ns = weapons.borrowed_namespace_of(index).unwrap_or_default();
        let name = weapons.name_of(index).to_owned();
        if let Some(sounds) = weapons.sounds_of(index) {
            rows.extend(audit_sounds(&name, sounds, &|alias| has_alias(ns, alias)));
        }
        if let Some(xanims) = xanims {
            let notes = weapons.animation_notetracks_of(index, xanims);
            let mapped = |note: &str| {
                weapons
                    .notetrack_actions_of(index)
                    .find(|(n, _)| *n == note)
                    .and_then(|(_, a)| a.sound_alias.clone())
            };
            rows.extend(audit_notetracks(&name, &notes, &mapped, &|alias| has_alias(ns, alias)));
        }
        rows.extend(audit_engine_supplied(item.weapon, &|alias| has_alias(AssetNamespace::T6, alias)));
        let fx_has = |n: &str| crate::FxName::new(ns, n).resolve(fx).is_some();
        let explosion = weapons
            .combat_fx_of(index)
            .and_then(|c| c.explosion_hint.as_deref());
        let trail = weapons.proj_trail_of(index).map(|n| n.name);
        let ignition = weapons.proj_ignition_of(index).map(|n| n.name);
        let combat = weapons.combat_fx_of(index);
        let extras = combat.map_or_else(Vec::new, |c| extra_fx_slots(c));
        let mut slots: Vec<(&str, Option<&str>)> =
            vec![("explosion", explosion), ("trail", trail), ("ignition", ignition)];
        slots.extend(extras.iter().map(|(slot, name)| (slot.as_str(), Some(*name))));
        rows.extend(audit_fx(&name, &slots, &fx_has));
    }
    (rows, absent)
}

/// One text line per row, `weapon | slot | asset | status`.
pub fn render_rows(rows: &[AuditRow]) -> Vec<String> {
    rows.iter()
        .map(|r| {
            format!(
                "{} | {} | {} | {}",
                r.weapon,
                r.slot,
                r.asset,
                match r.status {
                    Status::Resolved => "ok",
                    Status::Missing => "MISSING from the loaded data",
                    Status::NotPlayed => "in the bank, slot not played by the game",
                    Status::NoSound => "note carries no sound (nothing plays)",
                }
            )
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn equipment_list_has_no_duplicates_and_all_mp() {
        let mut seen = std::collections::HashSet::new();
        for item in BO2_MP_EQUIPMENT {
            assert!(item.weapon.ends_with("_mp"), "{}", item.weapon);
            assert!(seen.insert(item.weapon), "{} twice", item.weapon);
        }
        for must in [
            "frag_grenade_mp",
            "sticky_grenade_mp",
            "satchel_charge_mp",
            "claymore_mp",
            "bouncingbetty_mp",
            "hatchet_mp",
            "concussion_grenade_mp",
            "flash_grenade_mp",
            "emp_grenade_mp",
            "sensor_grenade_mp",
            "proximity_grenade_mp",
            "willy_pete_mp",
            "nightingale_mp",
            "trophy_system_mp",
            "tactical_insertion_mp",
            "acoustic_sensor_mp",
            "camera_spike_mp",
            "scrambler_mp",
            "ballistic_knife_mp",
        ] {
            assert!(seen.contains(must), "{must} is not audited");
        }
    }

    /// Every slot, once set, is reached by the audit and by the loader's
    /// list of aliases to prepare (a slot left out of either is a sound that
    /// never plays: the C4 detonator's was).
    #[test]
    fn every_slot_is_audited_and_prepared() {
        for slot in WeaponSoundSlot::ALL {
            let mut sounds = WeaponSoundAliases::default();
            *sounds.slot_mut(slot) = Some(format!("alias_{slot:?}"));
            let rows = audit_sounds("w", &sounds, &|_| true);
            assert_eq!(rows.len(), 1, "{slot:?}");
            assert_eq!(rows[0].asset, format!("alias_{slot:?}"));
            assert_eq!(
                sounds.reachable_aliases(),
                vec![format!("alias_{slot:?}").as_str()],
                "{slot:?} is not prepared with the weapon's sounds"
            );
        }
    }

    #[test]
    fn bounce_and_notetrack_sounds_are_checked() {
        let mut sounds = WeaponSoundAliases::default();
        sounds.bounce[0] = Some("wpn_grenade_bounce_default".into());
        sounds.bounce[5] = Some("wpn_grenade_bounce_concrete".into());
        sounds.notetrack_sound_map = vec![("grenade_throw".into(), "wpn_grenade_throw_plr".into())];
        let has = |a: &str| a != "wpn_grenade_bounce_concrete";
        let rows = audit_sounds("sticky_grenade_mp", &sounds, &has);
        assert_eq!(rows.len(), 3);
        let by = |slot: &str| rows.iter().find(|r| r.slot == slot).unwrap().status;
        assert_eq!(by("Bounce[0]"), Status::Resolved);
        assert_eq!(by("Bounce[5]"), Status::Missing);
        assert_eq!(by("Notetrack grenade_throw"), Status::Resolved);
        assert!(render_rows(&rows).iter().any(|l| l.contains("MISSING")));
    }

    #[test]
    fn projectile_flight_loop_and_placing_sounds_are_played() {
        let mut sounds = WeaponSoundAliases::default();
        for slot in [
            WeaponSoundSlot::Projectile,
            WeaponSoundSlot::ProjectileExplosion,
            WeaponSoundSlot::Deploy,
            WeaponSoundSlot::FinishDeployPlayer,
            WeaponSoundSlot::Breakdown,
            WeaponSoundSlot::ProjDud,
        ] {
            *sounds.slot_mut(slot) = Some(format!("a_{slot:?}"));
        }
        let rows = audit_sounds("w", &sounds, &|_| true);
        assert_eq!(rows.len(), 6);
        assert!(rows.iter().all(|r| r.status == Status::Resolved), "{rows:?}");
        // A name the bank lacks is still reported missing.
        let rows = audit_sounds("w", &sounds, &|a| a != "a_ProjDud");
        assert_eq!(rows.iter().filter(|r| r.status == Status::Missing).count(), 1);
    }

    #[test]
    fn notetrack_names_are_classified_and_audited() {
        let notes: Vec<String> = [
            "sndnt#wpn_grenade_pin_plr",
            "sndnt#wpn_missing",
            "rmbnt#frag_throw",
            "grenade_throw",
            "silent_note",
            "end",
        ]
        .into_iter()
        .map(str::to_owned)
        .collect();
        let mapped = |n: &str| (n == "grenade_throw").then(|| "wpn_grenade_throw_plr".to_owned());
        let rows = audit_notetracks("frag_grenade_mp", &notes, &mapped, &|a| a != "wpn_missing");
        let by = |slot: &str| rows.iter().find(|r| r.slot == slot).map(|r| r.status);
        assert_eq!(by("Anim note sndnt#wpn_grenade_pin_plr"), Some(Status::Resolved));
        assert_eq!(by("Anim note sndnt#wpn_missing"), Some(Status::Missing));
        assert_eq!(by("Anim note rmbnt#frag_throw (rumble)"), Some(Status::Resolved));
        assert_eq!(by("Anim note grenade_throw (weapon file map)"), Some(Status::Resolved));
        assert_eq!(by("Anim note silent_note"), Some(Status::NoSound));
        // The clip's end is the engine's, not a gap.
        assert_eq!(rows.len(), 5);
        assert!(render_rows(&rows).iter().any(|l| l.contains("carries no sound")));
    }

    #[test]
    fn client_script_report_lists_what_the_zones_hold() {
        let names = [
            "script:clientscripts/mp/_claymore.csc",
            "script:maps/mp/gametypes/_claymore.gsc",
            "script:clientscripts/mp/_trophy_system.csc",
        ];
        let lines = client_script_report(names.iter().copied());
        assert_eq!(lines.len(), CLIENT_SCRIPT_ITEMS.len());
        let line = |w: &str| lines.iter().find(|l| l.starts_with(w)).unwrap();
        // Only client scripts count; the server .gsc is not one.
        assert!(line("claymore_mp").contains("_claymore.csc"));
        assert!(!line("claymore_mp").contains(".gsc"));
        assert!(line("claymore_mp").contains("NOT RUN"));
        assert!(line("scrambler_mp").contains("no script found"));
        assert!(line("sticky_grenade_mp").contains("hooked in Rust"));
    }

    #[test]
    fn client_script_items_are_in_the_equipment_list() {
        for item in CLIENT_SCRIPT_ITEMS {
            assert!(
                BO2_MP_EQUIPMENT.iter().any(|e| e.weapon == item.weapon),
                "{} is not audited",
                item.weapon
            );
        }
    }

    #[test]
    fn generic_notetrack_names_classify() {
        use crate::NotetrackKind as K;
        assert_eq!(crate::classify_notetrack("sndnt#wpn_x"), K::Sound("wpn_x"));
        assert_eq!(crate::classify_notetrack("SNDNT#wpn_x"), K::Sound("wpn_x"));
        assert_eq!(crate::classify_notetrack("rmbnt#r"), K::Rumble("r"));
        assert_eq!(crate::classify_notetrack("sndnt#"), K::Named, "an empty alias names nothing");
        assert_eq!(crate::classify_notetrack("End"), K::Engine);
        assert_eq!(crate::classify_notetrack("NVG_on_powerup"), K::Engine);
        assert_eq!(crate::classify_notetrack("grenade_throw"), K::Named);
    }

    #[test]
    fn a_map_value_with_two_aliases_audits_both() {
        let mut sounds = WeaponSoundAliases::default();
        sounds.notetrack_sound_map = vec![("throw".into(), "wpn_t_plr wpn_t_npc".into())];
        let rows = audit_sounds("w", &sounds, &|a| a == "wpn_t_plr");
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].slot, "Notetrack throw");
        assert_eq!(rows[0].status, Status::Resolved);
        assert_eq!(rows[1].slot, "Notetrack throw (others)");
        assert_eq!(rows[1].status, Status::Missing);
        assert_eq!(sounds.reachable_aliases(), vec!["wpn_t_plr", "wpn_t_npc"]);
    }

    #[test]
    fn extra_effect_slots_are_audited() {
        let mut combat = crate::WeaponCombatFx::default();
        combat.explosion_extra_hints = vec!["fx/a".into(), "fx/b".into()];
        combat.dud_hint = Some("fx/dud".into());
        let slots = extra_fx_slots(&combat);
        let refs: Vec<(&str, Option<&str>)> =
            slots.iter().map(|(s, n)| (s.as_str(), Some(*n))).collect();
        let rows = audit_fx("w", &refs, &|n| n != "fx/b");
        let status: Vec<_> = rows.iter().map(|r| (r.slot.as_str(), r.status)).collect();
        assert_eq!(
            status,
            vec![
                ("Fx explosion-extra#1", Status::Resolved),
                ("Fx explosion-extra#2", Status::Missing),
                ("Fx dud", Status::Resolved),
            ]
        );
        assert!(extra_fx_slots(&crate::WeaponCombatFx::default()).is_empty());
    }

    #[test]
    fn extra_effects_play_under_the_weapons_namespace() {
        let mut combat = crate::WeaponCombatFx::default();
        combat.namespace = AssetNamespace::T6;
        combat.explosion_extra_hints = vec!["fx/a".into(), String::new(), "fx/c".into()];
        combat.dud_hint = Some("fx/dud".into());
        let names: Vec<_> = combat.explosion_extra_present().map(|n| n.name).collect();
        assert_eq!(names, vec!["fx/a", "fx/c"]);
        assert_eq!(combat.dud_present().map(|n| n.name), Some("fx/dud"));
        assert!(combat.explosion_extra_present().all(|n| n.namespace == AssetNamespace::T6));
    }

    #[test]
    fn empty_names_give_no_rows() {
        let mut sounds = WeaponSoundAliases::default();
        *sounds.slot_mut(WeaponSoundSlot::Fire) = Some(String::new());
        assert!(audit_sounds("w", &sounds, &|_| false).is_empty());
        assert!(audit_fx("w", &[("explosion", Some("")), ("trail", None)], &|_| false).is_empty());
    }

    #[test]
    fn semtex_beep_is_audited() {
        let rows = audit_engine_supplied("sticky_grenade_mp", &|a| a == SEMTEX_ALERT);
        assert!(rows.iter().any(|r| r.asset == SEMTEX_ALERT && r.status == Status::Resolved));
        assert!(rows.iter().any(|r| r.asset == GRENADE_EXPLODE && r.status == Status::Missing));
    }
}
