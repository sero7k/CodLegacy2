//! bo2mp: one weapon layout for a whole online match. The host sends its
//! own (`sim::guest_manifest`); a joining client lays its weapons out the
//! same way before its match is built, and a gun from a game it does not
//! have becomes a Black Ops II gun of the same kind with the host's numbers.
//! The host, its bots and a dedicated server keep the real guns.

use std::collections::HashSet;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use asset_core::AssetNamespace;
use asset_game::{LocalizeCatalog, StandIn, WeaponRegistry, WeaponRowKey};
use sim::guest_manifest::{GuestFacts, GuestManifest, ManifestRow, MatchId};
use weapon_iw4::WeaponCombatFacts;

/// How long a joining client waits for the host's layout before building
/// its match with its own (and being refused if they differ).
const WAIT: Duration = Duration::from_secs(30);

fn mod_names() -> HashSet<String> {
    bo2_profile::guest::MOD
        .iter()
        .map(|g| bo2_profile::guest::script_name(g).to_ascii_lowercase())
        .collect()
}

/// Whether a row is another game's gun (one a client may lack).
fn is_guest(weapons: &WeaponRegistry, id: u32, mods: &HashSet<String>) -> bool {
    weapons.namespace_of(id) != Some(AssetNamespace::T6)
        || weapons.borrowed_namespace_of(id) != weapons.namespace_of(id)
        || mods.contains(&weapons.script_name_of(id).to_ascii_lowercase())
}

/// The host's layout, from its built tables.
pub(crate) fn host_manifest(
    weapons: &WeaponRegistry,
    combat: &[WeaponCombatFacts],
    equipment: &[sim::EquipmentRuntimeFacts],
    runnable: Vec<bool>,
    transition_groups: Vec<u32>,
    strings: &LocalizeCatalog,
) -> GuestManifest {
    let mods = mod_names();
    let scales = weapons.scales_table();
    let display = weapons.display_names_table();
    let rows = (1..=weapons.len() as u32)
        .map(|id| {
            let key = weapons.row_key(id);
            let guest = weapons.facts_of(id).filter(|_| is_guest(weapons, id, &mods)).map(|facts| {
                let display_key = display.get(id as usize).cloned().unwrap_or_else(String::new);
                GuestFacts {
                    weap_type: facts.weap_type,
                    weap_class: facts.weap_class,
                    inventory_type: facts.inventory_type,
                    offhand_class: facts.offhand_class,
                    script_name: weapons.script_name_of(id),
                    display_text: strings.text(&display_key).map_or_else(String::new, str::to_owned),
                    display_key,
                    combat: combat.get(id as usize).copied().unwrap_or_else(WeaponCombatFacts::none),
                    equipment: equipment.get(id as usize).copied().unwrap_or_else(sim::EquipmentRuntimeFacts::default),
                    scales: scales.get(id as usize).copied().unwrap_or((1.0, 1.0, 1.0)),
                }
            });
            match key {
                Some(k) => ManifestRow {
                    namespace: k.namespace.as_str().to_owned(),
                    name: k.name,
                    attachments: k.attachments,
                    alternate: k.alternate,
                    guest,
                },
                None => ManifestRow {
                    namespace: String::new(),
                    name: String::new(),
                    attachments: Vec::new(),
                    alternate: false,
                    guest,
                },
            }
        })
        .collect();
    GuestManifest {
        rows,
        runnable,
        transition_groups,
    }
}

fn row_key(row: &ManifestRow) -> WeaponRowKey {
    WeaponRowKey {
        namespace: AssetNamespace::parse(&row.namespace).unwrap_or(AssetNamespace::T6),
        name: row.name.clone(),
        attachments: row.attachments.clone(),
        alternate: row.alternate,
    }
}

/// The client's weapons laid out as the host's, with the stand-ins'
/// indices.
pub(crate) fn conform(weapons: &WeaponRegistry, m: &GuestManifest) -> (WeaponRegistry, Vec<u32>) {
    let keys: Vec<(WeaponRowKey, Option<StandIn>)> = m
        .rows
        .iter()
        .map(|row| {
            let stand_in = row.guest.as_ref().map(|g| StandIn {
                weap_type: g.weap_type,
                weap_class: g.weap_class,
                inventory_type: g.inventory_type,
                offhand_class: g.offhand_class,
                fire_type: g.combat.fire_type,
                fire_time_ms: g.combat.fire_time_ms,
                clip_size: g.combat.clip_size,
                max_ammo: g.combat.max_ammo,
                start_ammo: g.combat.start_ammo,
                display_name_key: g.display_key.clone(),
            });
            (row_key(row), stand_in)
        })
        .collect();
    let (out, report) = weapons.conformed(&keys);
    diag::info!(
        World,
        "bo2mp guest guns: weapons laid out as the host's: {} kept, {} stand-ins, {} of ours the host lacks{}",
        report.kept,
        report.stood_in.len(),
        report.dropped,
        if report.stood_in.is_empty() {
            String::new()
        } else {
            format!(
                ": {}",
                report
                    .stood_in
                    .iter()
                    .take(60)
                    .map(|(_, name, donor)| format!("{name}<-{}", if donor.is_empty() { "?" } else { donor }))
                    .collect::<Vec<_>>()
                    .join(" ")
            )
        }
    );
    (out, report.stood_in.iter().map(|(id, _, _)| *id).collect())
}

/// The host's names for the stand-ins, where the client has no text.
pub(crate) fn stand_in_strings(m: &GuestManifest, stand_ins: &[u32]) -> LocalizeCatalog {
    let rows: Vec<(&str, &str)> = stand_ins
        .iter()
        .filter_map(|&id| m.rows.get(id as usize - 1)?.guest.as_ref())
        .filter(|g| !g.display_key.is_empty() && !g.display_text.is_empty())
        .map(|g| (g.display_key.as_str(), g.display_text.as_str()))
        .collect();
    LocalizeCatalog::from_rows(rows)
}

fn guest_of(m: &GuestManifest, id: u32) -> Option<&GuestFacts> {
    m.rows.get((id as usize).checked_sub(1)?)?.guest.as_ref()
}

/// The stand-ins take the host's numbers; the whole layout takes the
/// host's run and attachment-switch tables.
pub(crate) struct HostNumbers<'a> {
    pub manifest: &'a GuestManifest,
    pub stand_ins: &'a [u32],
}

impl HostNumbers<'_> {
    pub fn combat(&self, combat: &mut [WeaponCombatFacts]) {
        for &id in self.stand_ins {
            if let (Some(g), Some(row)) = (guest_of(self.manifest, id), combat.get_mut(id as usize)) {
                *row = g.combat;
            }
        }
    }

    pub fn equipment(&self, equipment: &mut [sim::EquipmentRuntimeFacts]) {
        for &id in self.stand_ins {
            if let (Some(g), Some(row)) = (guest_of(self.manifest, id), equipment.get_mut(id as usize)) {
                *row = g.equipment;
            }
        }
    }

    pub fn scales(&self, scales: &mut [(f32, f32, f32)]) {
        for &id in self.stand_ins {
            if let (Some(g), Some(row)) = (guest_of(self.manifest, id), scales.get_mut(id as usize)) {
                *row = g.scales;
            }
        }
    }

    pub fn script_names(&self, names: &mut [String]) {
        for &id in self.stand_ins {
            if let (Some(g), Some(row)) = (guest_of(self.manifest, id), names.get_mut(id as usize)) {
                row.clone_from(&g.script_name);
            }
        }
    }

    pub fn runnable(&self, runnable: &mut Vec<bool>) {
        if self.manifest.runnable.len() == runnable.len() {
            runnable.clone_from(&self.manifest.runnable);
        }
    }

    pub fn transition_groups(&self, groups: &mut Vec<u32>) {
        if self.manifest.transition_groups.len() == groups.len() {
            groups.clone_from(&self.manifest.transition_groups);
        }
    }
}

static WAITING: Mutex<Option<(u64, Instant)>> = Mutex::new(None);

/// A joining client: whether to hold its match build (load `request`) for
/// the host's layout a while longer.
pub(crate) fn hold_for_layout(id: MatchId, request: u64) -> bool {
    if sim::guest_manifest::received(id).is_some() {
        return false;
    }
    let Ok(mut waiting) = WAITING.lock() else { return false };
    let started = match *waiting {
        Some((r, at)) if r == request => at,
        _ => {
            diag::info!(World, "bo2mp guest guns: waiting for the host's weapon layout");
            *waiting = Some((request, Instant::now()));
            return true;
        }
    };
    if started.elapsed() < WAIT {
        return true;
    }
    diag::warn!(
        World,
        "bo2mp guest guns: the host's weapon layout did not arrive in {}s; building with ours",
        WAIT.as_secs()
    );
    false
}
