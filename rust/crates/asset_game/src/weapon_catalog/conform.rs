//! bo2mp: a client joining a match lays its weapons out as the host's, so
//! a weapon's number means the same gun on every machine. A gun the client
//! lacks (its game is not installed) becomes a copy of a Black Ops II gun of
//! its kind: that gun's models, sounds, animations and HUD pictures, under
//! the host's name, with the host's handling.

use std::collections::HashMap;

use super::{WeaponRegistry, WeaponRow, mint_weapon_revision, normalize_weapon_name};
use crate::AssetNamespace;

/// A weapon as both machines name it.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct WeaponRowKey {
    pub namespace: AssetNamespace,
    pub name: String,
    pub attachments: Vec<String>,
    pub alternate: bool,
}

/// What a missing gun takes from the host.
#[derive(Clone, Debug, Default)]
pub struct StandIn {
    pub weap_type: i32,
    pub weap_class: i32,
    pub inventory_type: i32,
    pub offhand_class: i32,
    pub fire_type: i32,
    pub fire_time_ms: i32,
    pub clip_size: i32,
    pub max_ammo: i32,
    pub start_ammo: i32,
    pub display_name_key: String,
}

#[derive(Clone, Debug, Default)]
pub struct ConformReport {
    /// The client's own rows kept.
    pub kept: usize,
    /// Stand-ins: (index, the host's name, the Black Ops II gun copied).
    pub stood_in: Vec<(u32, String, String)>,
    /// The client's rows the host does not have.
    pub dropped: usize,
}

impl WeaponRegistry {
    pub fn row_key(&self, id: u32) -> Option<WeaponRowKey> {
        let row = self.rows.get(id as usize).filter(|_| id != 0)?;
        Some(WeaponRowKey {
            namespace: row.namespace,
            name: row.name.clone(),
            attachments: row.prepared_attachments.clone(),
            alternate: row.facts.inventory_type == 3,
        })
    }

    /// This registry laid out as `keys` (rows 1..): a row the client has
    /// keeps its own content; a missing one with a stand-in is a copy of a
    /// Black Ops II gun of its kind; a missing one without is an empty row.
    pub fn conformed(&self, keys: &[(WeaponRowKey, Option<StandIn>)]) -> (WeaponRegistry, ConformReport) {
        let mut report = ConformReport::default();
        let mut local: HashMap<WeaponRowKey, usize> = HashMap::with_capacity(self.rows.len());
        for id in 1..self.rows.len() {
            if let Some(key) = self.row_key(id as u32) {
                local.entry(key).or_insert(id);
            }
        }
        let donors: Vec<usize> = (1..self.rows.len())
            .filter(|&i| {
                let r = &self.rows[i];
                r.namespace == AssetNamespace::T6
                    && r.donor_namespace.is_none()
                    && r.iw5_configuration.is_none()
                    && r.prepared_attachments.is_empty()
                    && r.gun_xmodel.is_some()
            })
            .collect();
        let pick_donor = |want: &StandIn| -> Option<usize> {
            let f = |i: usize| self.rows[i].facts;
            let same_kind = |i: usize| {
                let g = f(i);
                g.weap_type == want.weap_type
                    && g.inventory_type == want.inventory_type
                    && g.weap_class == want.weap_class
                    && g.offhand_class == want.offhand_class
            };
            donors
                .iter()
                .copied()
                .find(|&i| same_kind(i) && f(i).fire_type == want.fire_type)
                .or_else(|| donors.iter().copied().find(|&i| same_kind(i)))
                .or_else(|| {
                    donors.iter().copied().find(|&i| {
                        f(i).weap_class == want.weap_class && f(i).inventory_type == want.inventory_type
                    })
                })
                .or_else(|| donors.iter().copied().find(|&i| f(i).inventory_type == want.inventory_type))
        };

        let mut rows: Vec<WeaponRow> = Vec::with_capacity(keys.len() + 1);
        rows.push(self.rows.first().cloned().unwrap_or_else(WeaponRow::default));
        let mut old_to_new: HashMap<usize, u32> = HashMap::with_capacity(keys.len());
        for (key, stand_in) in keys {
            let new = rows.len() as u32;
            if let Some(&old) = local.get(key)
                && !old_to_new.contains_key(&old)
            {
                old_to_new.insert(old, new);
                rows.push(self.rows[old].clone());
                report.kept += 1;
                continue;
            }
            let donor = stand_in.as_ref().and_then(pick_donor);
            let mut row = match donor {
                Some(d) => {
                    let mut row = self.rows[d].clone();
                    row.donor_namespace = Some(row.donor_namespace.unwrap_or(row.namespace));
                    row
                }
                None => WeaponRow::default(),
            };
            row.name = normalize_weapon_name(&key.name);
            row.namespace = key.namespace;
            row.prepared_attachments = key.attachments.clone();
            row.iw5_configuration = None;
            row.alternate_weapon = None;
            row.alternate_index = 0;
            row.dual_wield_weapon = None;
            if let Some(s) = stand_in {
                row.display_name_key = (!s.display_name_key.is_empty()).then(|| s.display_name_key.clone());
                row.facts.weap_type = s.weap_type;
                row.facts.weap_class = s.weap_class;
                row.facts.inventory_type = s.inventory_type;
                row.facts.offhand_class = s.offhand_class;
                row.facts.fire_type = s.fire_type;
                row.facts.fire_time_ms = s.fire_time_ms;
                row.facts.clip_size = s.clip_size;
                row.facts.max_ammo = s.max_ammo;
                row.facts.start_ammo = s.start_ammo;
            }
            report.stood_in.push((
                new,
                key.name.clone(),
                donor.map_or_else(String::new, |d| self.rows[d].name.clone()),
            ));
            rows.push(row);
        }
        report.dropped = (self.rows.len().saturating_sub(1)).saturating_sub(old_to_new.len());

        let remap = |old: u32| old_to_new.get(&(old as usize)).copied();
        for row in &mut rows {
            if let Some((base, selection)) = row.iw5_configuration.take() {
                row.iw5_configuration = remap(base).map(|b| (b, selection));
            }
        }
        let configurations = self
            .configurations
            .iter()
            .filter_map(|(sel, &id)| Some((sel.clone(), remap(id)?)))
            .collect();
        let alternate_fpv = self
            .alternate_fpv
            .iter()
            .filter_map(|(&(a, b), sides)| Some(((remap(a)?, remap(b)?), sides.clone())))
            .collect();
        let mut out = WeaponRegistry {
            rows,
            world_catalog_identity: self.world_catalog_identity,
            iw5_attachments: self.iw5_attachments.clone(),
            configurations,
            by_name: HashMap::new(),
            by_namespaced: HashMap::new(),
            item_groups: self.item_groups.clone(),
            families: crate::WeaponFamilies::default(),
            alternate_fpv,
            fpv_clip_tracks: self.fpv_clip_tracks.clone(),
            vehicle_turrets: self.vehicle_turrets.clone(),
            vehicle_compass: self.vehicle_compass.clone(),
            revision: mint_weapon_revision(),
            guest_names: self.guest_names,
            guest_script: Vec::new(),
            family_tables: self.family_tables.clone(),
            bo2_attachments: self.bo2_attachments.clone(),
            bo2_attachment_sounds: self.bo2_attachment_sounds.clone(),
        };
        out.rebuild_name_maps();
        out.families = crate::WeaponFamilies::build(&out.family_tables, &out);
        (out, report)
    }
}
