//! Black Ops II's equipment lights and the Semtex's beeping, which its
//! client scripts play on each thrown grenade (`_sticky_grenade.csc`,
//! `_satchel_charge.csc`, `_bouncingbetty.csc`): this rebuild does not run
//! client scripts, so the same rules run here.
//!
//! - Semtex: a blinking light on `tag_fx` (green to the thrower's side, red
//!   to the rest) and `wpn_semtex_alert`, again and again, 0.3 s apart at
//!   first, each wait 1/1.2 of the last, down to 0.08 s.
//! - C4 and the Bouncing Betty: a steady light on `tag_origin`, green or
//!   red the same way. The Shock Charge's scan light is steady on `tag_fx`;
//!   the crossbow's explosive bolt beeps as the Semtex does.

use std::collections::HashMap;

use asset_game::FxName;
use assets::PreparedWeapons;
use bevy::prelude::*;
use net::{LocalPresentClient, PresentedSnapshot};
use render_fx::combat::missile_bolt_target_on;
use render_fx::{HostFxSystem, PreparedFxCatalog, PreparedFxElemInfos};

use crate::adapters::fx::system::FxSceneAccess;

fn equip_log(line: std::fmt::Arguments<'_>) {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    if *ON.get_or_init(|| codlegacy_env::var_os("CODLEGACY_AUDIO_EQUIPLOG").is_some()) {
        diag::info!(Audio, "{line}");
    }
}

struct Look {
    weapon: &'static str,
    tag: &'static str,
    friendly: &'static str,
    enemy: &'static str,
    /// The Semtex's beeps (first wait, the factor each wait shrinks by, the
    /// shortest wait), or a light lit once.
    beep: Option<(&'static str, i32, f32, i32)>,
}

const LOOKS: [Look; 5] = [
    Look {
        weapon: "explosive_bolt_mp",
        tag: "tag_origin",
        friendly: "weapon/crossbow/fx_trail_crossbow_blink_grn_os",
        enemy: "weapon/crossbow/fx_trail_crossbow_blink_red_os",
        beep: Some((asset_game::equipment_audit::SEMTEX_ALERT, 300, 1.2, 80)),
    },
    Look {
        weapon: "proximity_grenade_mp",
        tag: "tag_fx",
        friendly: "weapon/grenade/fx_prox_grenade_scan_grn",
        enemy: "weapon/grenade/fx_prox_grenade_scan_red",
        beep: None,
    },
    Look {
        weapon: "sticky_grenade_mp",
        tag: "tag_fx",
        friendly: "weapon/crossbow/fx_trail_crossbow_blink_grn_os",
        enemy: "weapon/crossbow/fx_trail_crossbow_blink_red_os",
        beep: Some((asset_game::equipment_audit::SEMTEX_ALERT, 300, 1.2, 80)),
    },
    Look {
        weapon: "satchel_charge_mp",
        tag: "tag_origin",
        friendly: "weapon/c4/fx_c4_light_green",
        enemy: "weapon/c4/fx_c4_light_red",
        beep: None,
    },
    Look {
        weapon: "bouncingbetty_mp",
        tag: "tag_origin",
        friendly: "weapon/bouncing_betty/fx_betty_light_green",
        enemy: "weapon/bouncing_betty/fx_betty_light_red",
        beep: None,
    },
];

struct Lit {
    projectile: Option<u32>,
    look: usize,
    /// The light playing (its effect's name and the bone it is on).
    light: Option<(String, u16)>,
    next_ms: i32,
    wait_ms: i32,
}

#[derive(Default)]
pub(crate) struct EquipmentLights {
    rows: HashMap<u32, Lit>,
}

/// BO2's `friendnotfoe`: his own, or (in a team game) his team's.
fn friendly(presented: &PresentedSnapshot, local: u32, owner: u32) -> bool {
    if owner == local {
        return true;
    }
    let Some(snap) = presented.snapshot() else {
        return false;
    };
    if !snap.meta.kind.is_team() {
        return false;
    }
    let team = |c: u32| snap.meta.for_client(sim::ClientId(c)).map(|m| m.client_state_team);
    matches!(team(local), Some(t @ (1 | 2)) if team(owner) == Some(t))
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn tick_equipment_lights(
    occupancy: Option<Res<crate::adapters::anim::missile::MissileOccupancy>>,
    weapons: Option<Res<PreparedWeapons>>,
    projectile_meshes: Option<Res<assets::PreparedProjectileMeshes>>,
    catalog: Option<Res<PreparedFxCatalog>>,
    mut elem_infos: ResMut<PreparedFxElemInfos>,
    mut host: ResMut<HostFxSystem>,
    poses: Option<Res<crate::adapters::anim::dobj_pose::HostDObjPoseFrame>>,
    presented: Res<PresentedSnapshot>,
    local: Res<LocalPresentClient>,
    mut sounds: Option<ResMut<Messages<audio::WeaponSound>>>,
    fx_world: FxSceneAccess,
    mut state: Local<EquipmentLights>,
) {
    let (Some(occupancy), Some(weapons), Some(meshes), Some(catalog)) = (
        occupancy.as_deref(),
        weapons.as_deref().map(|w| &w.0),
        projectile_meshes.as_deref().map(|m| &m.0),
        catalog.as_deref().map(|c| &c.0),
    ) else {
        return;
    };
    let now = host.0.msec_now;
    let mut live = Vec::new();
    for row in &occupancy.rows {
        let Some(entnum) = row.entnum else {
            continue;
        };
        let name = weapons.name_of(row.weapon);
        let Some(look) = LOOKS.iter().position(|l| {
            // Catalog rows are bare ("sticky_grenade"), the table says "_mp".
            let bare = |s: &str| s.strip_suffix("_mp").unwrap_or(s).to_owned();
            bare(l.weapon).eq_ignore_ascii_case(&bare(name))
        }) else {
            continue;
        };
        live.push(entnum);
        let fresh = state
            .rows
            .get(&entnum)
            .is_none_or(|r| r.projectile != row.id || r.look != look);
        if fresh {
            if let Some(old) = state.rows.remove(&entnum)
                && let Some((def, bone)) = old.light
            {
                host.0.stop_bolted(&def, entnum, bone);
            }
            state.rows.insert(
                entnum,
                Lit {
                    projectile: row.id,
                    look,
                    light: None,
                    next_ms: now,
                    wait_ms: LOOKS[look].beep.map_or(0, |b| b.1),
                },
            );
        }
        let l = &LOOKS[look];
        let lit = &state.rows[&entnum];
        let due = match l.beep {
            Some(_) => now >= lit.next_ms,
            None => lit.light.is_none(),
        };
        if !due {
            continue;
        }
        // The beep needs no model: the client script sounds it whether or
        // not the light finds its bone.
        if let Some((alias, _, shrink, shortest)) = l.beep {
            equip_log(format_args!(
                "audio: equip beep {} ent {entnum} alias {alias} at {:?}",
                l.weapon, row.origin
            ));
            if let Some(sounds) = sounds.as_deref_mut() {
                sounds.write(audio::WeaponSound {
                    namespace: weapons
                        .borrowed_namespace_of(row.weapon)
                        .unwrap_or(asset_core::AssetNamespace::T6),
                    alias: alias.to_owned(),
                    origin_inches: Some(row.origin),
                    snd_ent: Some(entnum),
                });
            }
            let r = state.rows.get_mut(&entnum).expect("row just made");
            r.next_ms = now.saturating_add(r.wait_ms);
            r.wait_ms = ((r.wait_ms as f32 / shrink) as i32).clamp(shortest, l.beep.map_or(0, |b| b.1));
        }
        // (`waittill_dobj`: no light until the model is posed.)
        let Some(target) =
            missile_bolt_target_on(poses.as_deref(), meshes, row.namespace, &row.name, entnum, l.tag)
        else {
            continue;
        };
        let effect = if friendly(&presented, local.0.0, row.owner) { l.friendly } else { l.enemy };
        let fx_name = FxName::new(row.namespace, effect);
        let def = fx_name.resolve(catalog).map(|e| e.name.clone());
        if let Some((old, bone)) = state.rows.get_mut(&entnum).and_then(|r| r.light.take()) {
            host.0.stop_bolted(&old, entnum, bone);
        }
        let mut played = 0;
        let ok = render_fx::combat::try_play_weapon_fx_bolted(
            &mut host.0,
            catalog,
            &mut elem_infos.0,
            Some(fx_name),
            Some(target),
            &mut played,
            fx_world.view().as_ref().map(|s| s as &dyn render_fx::present::FxScene),
        );
        let r = state.rows.get_mut(&entnum).expect("row just made");
        if ok && let Some(def) = def {
            r.light = Some((def, target.bone));
        }
        if l.beep.is_none() && !ok {
            // (Tried again next frame.)
            r.light = None;
        }
    }
    let gone: Vec<u32> = state.rows.keys().copied().filter(|n| !live.contains(n)).collect();
    for n in gone {
        if let Some(Lit { light: Some((def, bone)), .. }) = state.rows.remove(&n) {
            host.0.stop_bolted(&def, n, bone);
        }
    }
}

/// `BO2MP_EQUIP_AUDIT=1`: once the weapons, the sound bank and the effects
/// are loaded, log every sound and effect each piece of Black Ops II
/// multiplayer equipment names (and the script-supplied ones) with whether
/// the loaded data has it. One line each: `weapon | slot | asset | status`
/// (docs/EQUIPMENT_AUDIT.md). Also lists the effects whose names look like
/// the equipment's lights, for the pieces whose client script is not run.
pub(crate) fn audit_equipment_once(
    weapons: Option<Res<PreparedWeapons>>,
    bank: Option<Res<audio::SoundBank>>,
    catalog: Option<Res<PreparedFxCatalog>>,
    xanims: Option<Res<assets::PreparedXAnims>>,
    mut done: Local<bool>,
) {
    if *done {
        return;
    }
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    if !*ON.get_or_init(|| codlegacy_env::var_os("BO2MP_EQUIP_AUDIT").is_some()) {
        *done = true;
        return;
    }
    let (Some(weapons), Some(bank), Some(catalog)) = (
        weapons.as_deref().map(|w| &w.0).filter(|w| !w.is_empty()),
        bank.as_deref().map(|b| &*b.0),
        catalog.as_deref().map(|c| &c.0),
    ) else {
        return;
    };
    *done = true;
    let (rows, absent) = asset_game::equipment_audit::audit_registry(
        weapons,
        &|ns, alias| bank.index_in(ns, alias).is_some(),
        catalog,
        xanims.as_deref().map(|x| &x.0),
    );
    let missing = rows
        .iter()
        .filter(|r| r.status == asset_game::equipment_audit::Status::Missing)
        .count();
    diag::info!(
        Audio,
        "equip audit: {} rows, {} missing from the loaded data, {} listed weapons absent {:?}",
        rows.len(),
        missing,
        absent.len(),
        absent
    );
    for line in asset_game::equipment_audit::render_rows(&rows) {
        diag::info!(Audio, "equip audit: {line}");
    }
    for needle in [
        "claymore", "betty", "c4", "satchel", "prox", "taser", "trophy", "sensor", "decoy",
        "nightingale", "camera", "scrambler", "insertion", "acoustic", "flash", "concussion",
        "emp", "smoke", "willy",
    ] {
        let mut hits: Vec<&str> = catalog
            .names()
            .filter(|n| n.starts_with("weapon/") && n.contains(needle))
            .collect();
        hits.sort_unstable();
        hits.truncate(24);
        if !hits.is_empty() {
            diag::info!(Audio, "equip audit: effects named like `{needle}`: {}", hits.join(", "));
        }
    }
}
