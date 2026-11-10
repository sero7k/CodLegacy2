//! bo2mp: guest guns' attachments and sniper scopes.
//!
//! A guest gun's attachments are variants of it, as Black Ops II's sights
//! are (`gh_ak12_mp+reflex`, `aw_mors_mp+silencer`): each a copy of the
//! gun's row with the attachment's models (its own game's, or Black Ops
//! II's), animations and sounds, Black Ops II's numbers for the attachment
//! of its kind (`extclip`'s magazine, `silencer`'s range and quiet shots),
//! and, aimed, a Black Ops II sight or scope (reticle, overlay, zoom).
//! A lane lists what each gun offers (`offer_guest_attachments`) and gives
//! a sniper its default scope (`give_guest_scope`); the match makes the
//! variants once every game's guns and Black Ops II's own are in
//! (`build_guest_variants`).

use super::*;

/// bo2mp: one attachment of a guest gun.
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct GuestAttachment {
    /// The word Create-a-Class gives it (`reflex`).
    pub word: String,
    /// A sight: one per variant, in place of a default scope.
    pub sight: bool,
    /// View models: at their placement, else on the gun's bone named as
    /// their root.
    pub view_models: Vec<(String, Option<asset_model::FpvMountOffset>)>,
    /// World models, on the gun's bone named as their root.
    pub world_models: Vec<String>,
    pub hide_tags: Vec<String>,
    /// Its own animations (catalog slot, animation).
    pub anims: Vec<(usize, String)>,
    pub sounds: Vec<(WeaponSoundSlot, String)>,
    /// Black Ops II's attachment whose numbers it takes (`extclip_lmg`).
    pub bo2: Option<String>,
    /// The Black Ops II weapon whose sight or scope shows aimed
    /// (`an94_mp+reflex`, `dsr50`): reticle, overlay and zoom.
    /// [`SNIPER_SCOPE`]: Black Ops II's sniper scope of the gun's kind.
    pub sight_from: Option<String>,
    /// Its own zoom (field of view) over the borrowed one.
    pub zoom_fov: Option<f32>,
    /// The variant is a copy of this Black Ops II weapon (a sight variant
    /// of the gun's donor) with the gun's own models and handling.
    pub copy_of: Option<String>,
    /// Without sounds of its own: the Black Ops II gun's attachment whose
    /// fire sounds it takes (`an94_silencer`).
    pub sounds_from: Option<String>,
    /// The gun's own view models it takes the place of (its magazine).
    pub replaces: Vec<String>,
    /// bo2mp: its game's effect overrides (`fxType`, effect): the flash and
    /// eject effects the gun plays with it on (Ghosts' `FXOverrideEntry`,
    /// chosen by `fx_iw4::iw6::weapon::best_override`). Kinds 1..6 (view and
    /// world flash, shell and last-shot eject), 7 (view magazine eject) and
    /// 8 (view flash while aimed).
    pub fx: Vec<(u32, String)>,
}

/// bo2mp: Black Ops II's numbers for one attachment (`WeaponAttachment`):
/// scales on the gun's own (1: unchanged).
#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Bo2AttachmentStats {
    pub damage_range: f32,
    pub ads_in: f32,
    pub ads_out: f32,
    pub ads_kick_center: f32,
    pub ads_idle: f32,
    pub ads_move: f32,
    pub hip_spread_min: f32,
    pub hip_spread_max: f32,
    pub fire_time: f32,
    pub reload: f32,
    pub reload_empty: f32,
    pub reload_add: f32,
    pub clip: f32,
    pub silenced: bool,
}

impl Default for Bo2AttachmentStats {
    fn default() -> Self {
        Self {
            damage_range: 1.0,
            ads_in: 1.0,
            ads_out: 1.0,
            ads_kick_center: 1.0,
            ads_idle: 1.0,
            ads_move: 1.0,
            hip_spread_min: 1.0,
            hip_spread_max: 1.0,
            fire_time: 1.0,
            reload: 1.0,
            reload_empty: 1.0,
            reload_add: 1.0,
            clip: 1.0,
            silenced: false,
        }
    }
}

impl Bo2AttachmentStats {
    pub fn from_t6(a: &asset_t6::AttachmentRef) -> Self {
        use fastfile_t6::layout::WeaponAttachment as w;
        // (0: the field unset, as on `defaultattachment`.)
        let scale = |off| {
            let v = a.f32_at(off);
            if v > 0.0 && v.is_finite() { v } else { 1.0 }
        };
        Self {
            damage_range: scale(w::fDamageRangeScale),
            ads_in: scale(w::fAdsTransInTimeScale),
            ads_out: scale(w::fAdsTransOutTimeScale),
            ads_kick_center: scale(w::fAdsViewKickCenterSpeedScale),
            ads_idle: scale(w::fAdsIdleAmountScale),
            ads_move: scale(w::adsMoveSpeedScale),
            hip_spread_min: scale(w::fHipSpreadMinScale),
            hip_spread_max: scale(w::fHipSpreadMaxScale),
            fire_time: scale(w::fFireTimeScale),
            reload: scale(w::fReloadTimeScale),
            reload_empty: scale(w::fReloadEmptyTimeScale),
            reload_add: scale(w::fReloadAddTimeScale),
            clip: scale(w::clipSizeScale),
            silenced: a.u8_at(w::bSilenced) != 0,
        }
    }

    fn apply(&self, f: &mut WeaponBodyFacts) {
        let ms = |v: i32, s: f32| {
            if s == 1.0 {
                v
            } else {
                (v as f32 * s).round() as i32
            }
        };
        if self.clip != 1.0 && f.clip_size > 0 {
            f.clip_size = ((f.clip_size as f32 * self.clip).round() as i32).max(f.clip_size + 1);
        }
        f.fire_time_ms = ms(f.fire_time_ms, self.fire_time).max(1);
        f.reload_time_ms = ms(f.reload_time_ms, self.reload);
        f.reload_empty_time_ms = ms(f.reload_empty_time_ms, self.reload_empty);
        f.reload_add_time_ms = ms(f.reload_add_time_ms, self.reload_add);
        f.reload_empty_add_time_ms = ms(f.reload_empty_add_time_ms, self.reload_add);
        f.max_damage_range *= self.damage_range;
        f.min_damage_range *= self.damage_range;
        for point in &mut f.damage_points {
            point[0] *= self.damage_range;
        }
        // (A rate: a shorter time is a faster one.)
        f.ads_in_rate /= self.ads_in;
        f.ads_out_rate /= self.ads_out;
        f.kick.f_ads_view_kick_center_speed *= self.ads_kick_center;
        f.idle.ads_idle_amount *= self.ads_idle;
        f.ads_move_speed_scale *= self.ads_move;
        for v in [
            &mut f.hip_spread_stand_min,
            &mut f.hip_spread_ducked_min,
            &mut f.hip_spread_prone_min,
        ] {
            *v *= self.hip_spread_min;
        }
        for v in [
            &mut f.hip_spread_stand_max,
            &mut f.hip_spread_ducked_max,
            &mut f.hip_spread_prone_max,
        ] {
            *v *= self.hip_spread_max;
        }
        f.silenced |= self.silenced;
    }
}

/// bo2mp: a row's guest attachment state.
#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
pub(super) struct GuestRow {
    /// A variant: its gun's row name (same namespace) and its words.
    pub(super) base: Option<String>,
    pub(super) words: Vec<String>,
    /// A gun: the attachments it offers.
    pub(super) offered: Vec<GuestAttachment>,
    /// The scope it was given (a sight takes its place).
    pub(super) scope: Option<GuestAttachment>,
    pub(super) sight_from: Option<String>,
    pub(super) zoom: Option<f32>,
    /// The namespace its borrowed overlay is named in.
    pub(super) overlay_ns: Option<crate::AssetNamespace>,
    /// The Black Ops II sight whose reticle picture it shows.
    pub(super) reticle: Option<String>,
}

/// What `build_guest_variants` made.
#[derive(Clone, Debug, Default)]
pub struct GuestVariantCensus {
    pub guns: usize,
    pub variants: usize,
    pub sights_borrowed: usize,
    /// Sights or attachments named but not in the catalog (Black Ops II
    /// weapons and attachments).
    pub missing: Vec<String>,
}

/// `GuestAttachment::sight_from` for Black Ops II's sniper scope of the
/// gun's kind: the DSR 50's on a bolt action, else the SVU's.
pub const SNIPER_SCOPE: &str = "sniper";

/// Words paired with a sight in a variant of two.
const PAIRED: [&str; 4] = ["silencer", "xmags", "extclip", "grip"];

fn put_models(row: &mut WeaponRow, a: &GuestAttachment) {
    for model in &a.replaces {
        while let Some(i) = row.attachment_view_models.iter().position(|m| m == model) {
            row.attachment_view_models.remove(i);
            if i < row.attachment_view_offsets.len() {
                row.attachment_view_offsets.remove(i);
            }
        }
    }
    for (model, at) in &a.view_models {
        row.attachment_view_models.push(model.clone());
        row.attachment_view_offsets.push(at.clone());
    }
    row.attachment_world_models
        .extend(a.world_models.iter().cloned());
    for tag in &a.hide_tags {
        if !row.hide_tags.iter().any(|t| t.eq_ignore_ascii_case(tag)) {
            row.hide_tags.push(tag.clone());
        }
    }
    for (slot, anim) in &a.anims {
        if let Some(s) = row.sz_xanims.get_mut(*slot) {
            *s = Some(anim.to_ascii_lowercase());
        }
    }
    for (slot, alias) in &a.sounds {
        *row.sounds.slot_mut(*slot) = Some(alias.clone());
    }
    for (fx_type, name) in &a.fx {
        let c = &mut row.combat_fx;
        let hint = match *fx_type {
            1 => &mut c.view_flash_hint,
            2 => &mut c.world_flash_hint,
            3 => &mut c.view_shell_eject_hint,
            4 => &mut c.world_shell_eject_hint,
            5 => &mut c.view_last_shot_eject_hint,
            6 => &mut c.world_last_shot_eject_hint,
            7 => &mut c.view_mag_eject_hint,
            8 => &mut c.view_flash_ads_hint,
            _ => continue,
        };
        *hint = Some(name.clone());
    }
    if a.sight_from.is_some() {
        row.guest.sight_from = a.sight_from.clone();
    }
    if a.zoom_fov.is_some() {
        row.guest.zoom = a.zoom_fov;
    }
}

/// Takes the scope `scope` put on `row` back off.
fn take_scope_off(row: &mut WeaponRow, scope: &GuestAttachment) {
    for (model, _) in &scope.view_models {
        if let Some(i) = row.attachment_view_models.iter().position(|m| m == model) {
            row.attachment_view_models.remove(i);
            if i < row.attachment_view_offsets.len() {
                row.attachment_view_offsets.remove(i);
            }
        }
    }
    for model in &scope.world_models {
        if let Some(i) = row.attachment_world_models.iter().position(|m| m == model) {
            row.attachment_world_models.remove(i);
        }
    }
    row.hide_tags
        .retain(|t| !scope.hide_tags.iter().any(|s| s.eq_ignore_ascii_case(t)));
    row.guest.sight_from = None;
    row.guest.zoom = None;
    row.guest.reticle = None;
    row.overlay_material = None;
    row.overlay_image = None;
    row.facts.overlay_reticle = 0;
}

impl WeaponBuild {
    /// bo2mp: Black Ops II's attachment numbers by attachment name (the
    /// Black Ops II lane's), for guest guns' variants.
    pub fn set_bo2_attachment_stats(&mut self, stats: HashMap<String, Bo2AttachmentStats>) {
        self.registry.bo2_attachments.extend(stats);
    }

    /// bo2mp: the fire sounds Black Ops II's guns' attachments give them
    /// (`an94_silencer`: its suppressed shots), as `t6:<alias>`.
    pub fn set_bo2_attachment_sounds(
        &mut self,
        sounds: HashMap<String, Vec<(WeaponSoundSlot, String)>>,
    ) {
        self.registry.bo2_attachment_sounds.extend(sounds);
    }

    /// bo2mp: every row whose name carries a thermal scope
    /// (`name_has_thermal_scope`: Modern Warfare 2's `_thermal` weapons,
    /// Black Ops' `_ir`) sees heat aimed. How many were marked.
    pub fn mark_thermal_scopes(&mut self) -> usize {
        let mut n = 0;
        for row in self.registry.rows.iter_mut().skip(1) {
            if !row.facts.thermal_scope && name_has_thermal_scope(&row.name, row.namespace) {
                row.facts.thermal_scope = true;
                n += 1;
            }
        }
        if n > 0 {
            self.registry.revision = mint_weapon_revision();
        }
        n
    }

    /// bo2mp: a guest gun (`index`) given its default scope: its models
    /// on the gun, and its sight (`scope.sight_from`) when aimed.
    pub fn give_guest_scope(&mut self, index: u32, scope: GuestAttachment) {
        let Some(row) = self
            .registry
            .rows
            .get_mut(index as usize)
            .filter(|_| index != 0)
        else {
            return;
        };
        put_models(row, &scope);
        row.guest.scope = Some(scope);
    }

    /// bo2mp: the attachments a guest gun (`index`) offers (its variants
    /// are made by `build_guest_variants`).
    pub fn offer_guest_attachments(&mut self, index: u32, offered: Vec<GuestAttachment>) {
        if let Some(row) = self
            .registry
            .rows
            .get_mut(index as usize)
            .filter(|_| index != 0)
        {
            row.guest.offered = offered;
        }
    }

    /// bo2mp: every guest gun's variants: one per attachment it offers,
    /// and a sight with a suppressor, extended magazine or grip, then each
    /// guest row's borrowed Black Ops II sight or scope. Once Black Ops
    /// II's own rows are in, before the guest names are given.
    pub fn build_guest_variants(&mut self) -> GuestVariantCensus {
        let mut census = GuestVariantCensus::default();
        let bases: Vec<u32> = (1..self.registry.rows.len() as u32)
            .filter(|&i| !self.registry.rows[i as usize].guest.offered.is_empty())
            .collect();
        let t6_row = |reg: &WeaponRegistry, name: &str| {
            reg.by_namespaced
                .get(&(crate::AssetNamespace::T6, normalize_weapon_name(name)))
                .copied()
        };
        let mut made: Vec<(u32, WeaponRow)> = Vec::new();
        for base in bases {
            let row = &self.registry.rows[base as usize];
            let offered = row.guest.offered.clone();
            census.guns += 1;
            let mut sets: Vec<Vec<&GuestAttachment>> = offered.iter().map(|a| vec![a]).collect();
            for (i, a) in offered.iter().enumerate() {
                for b in &offered[i + 1..] {
                    let (sight, other) = match (a.sight, b.sight) {
                        (true, false) => (a, b),
                        (false, true) => (b, a),
                        _ => continue,
                    };
                    if PAIRED.contains(&other.word.as_str()) {
                        sets.push(vec![sight, other]);
                    }
                }
            }
            for set in sets {
                let mut words: Vec<String> = set.iter().map(|a| a.word.clone()).collect();
                words.sort();
                words.dedup();
                let name = format!("{}+{}", row.name, words.join("+"));
                let mut v = match set.iter().find_map(|a| a.copy_of.as_deref()) {
                    Some(copy) => match t6_row(&self.registry, copy) {
                        Some(src) => {
                            let src = &self.registry.rows[src as usize];
                            let donor = src
                                .name
                                .split_once('+')
                                .and_then(|(gun, _)| t6_row(&self.registry, gun))
                                .map(|i| &self.registry.rows[i as usize]);
                            guest_copy_of(src, donor, row)
                        }
                        None => {
                            census.missing.push(copy.to_owned());
                            continue;
                        }
                    },
                    None => row.clone(),
                };
                v.name = name;
                v.guest.base = Some(row.name.clone());
                v.guest.words = words;
                v.guest.offered.clear();
                v.alternate_weapon = None;
                v.alternate_index = 0;
                v.iw5_configuration = None;
                if set.iter().any(|a| a.sight)
                    && let Some(scope) = v.guest.scope.take()
                {
                    take_scope_off(&mut v, &scope);
                }
                for a in &set {
                    put_models(&mut v, a);
                    if a.sounds.is_empty()
                        && let Some(sounds) = a
                            .sounds_from
                            .as_deref()
                            .and_then(|k| self.registry.bo2_attachment_sounds.get(k))
                    {
                        for (slot, alias) in sounds {
                            *v.sounds.slot_mut(*slot) = Some(alias.clone());
                        }
                    }
                    if let Some(bo2) = a.bo2.as_deref() {
                        match self.registry.bo2_attachments.get(bo2) {
                            Some(stats) => stats.apply(&mut v.facts),
                            None => census.missing.push(format!("attachment {bo2}")),
                        }
                    }
                }
                made.push((base, v));
            }
        }
        census.variants = made.len();
        for (base, row) in made {
            let slots = self
                .combat_slots
                .get(base as usize)
                .copied()
                .unwrap_or_default();
            self.combat_slots
                .resize(self.registry.rows.len(), CombatFxSlots::default());
            self.combat_slots.push(slots);
            self.registry.rows.push(row);
        }
        self.registry.rebuild_name_maps();
        // The borrowed sights and scopes (base guns' too); a gun of a game
        // whose scope pictures are not drawn here that still aims through
        // one of its own takes Black Ops II's sniper scope.
        for id in 1..self.registry.rows.len() {
            let row = &self.registry.rows[id];
            let mut from = match row.guest.sight_from.clone() {
                Some(from) => from,
                None if needs_borrowed_scope(row) => SNIPER_SCOPE.to_owned(),
                None => continue,
            };
            if from == SNIPER_SCOPE {
                from = if self.registry.rows[id].facts.bolt_action {
                    "dsr50"
                } else {
                    "svu"
                }
                .to_owned();
            }
            let found = t6_row(&self.registry, &from).or_else(|| {
                (from == "an94_mp+ir")
                    .then(|| t6_row(&self.registry, "dsr50_mp+ir"))
                    .flatten()
            });
            let Some(src) = found else {
                census.missing.push(from);
                continue;
            };
            let src = self.registry.rows[src as usize].clone();
            let row = &mut self.registry.rows[id];
            borrow_sight(row, &src);
            census.sights_borrowed += 1;
        }
        self.registry.revision = mint_weapon_revision();
        census.missing.sort();
        census.missing.dedup();
        census
    }
}

/// `gun`'s variant as a copy of Black Ops II's `src` (a sight variant of
/// its donor, `donor`): `src`'s animations and sight model, `gun`'s own
/// models, name, sounds and handling. The donor's own parts (its
/// magazine, a sniper's scope) are its model's, not the gun's.
fn guest_copy_of(src: &WeaponRow, donor: Option<&WeaponRow>, gun: &WeaponRow) -> WeaponRow {
    let mut v = src.clone();
    v.donor_namespace = gun.donor_namespace.or(Some(src.namespace));
    v.namespace = gun.namespace;
    v.display_name_key = gun.display_name_key.clone();
    v.gun_xmodel = gun.gun_xmodel.clone();
    v.world_model = gun.world_model.clone();
    let donor_part = |m: &String| donor.is_some_and(|d| d.attachment_view_models.contains(m));
    let keep: Vec<usize> = (0..src.attachment_view_models.len())
        .filter(|&i| !donor_part(&src.attachment_view_models[i]))
        .collect();
    v.attachment_view_models = keep
        .iter()
        .map(|&i| src.attachment_view_models[i].clone())
        .collect();
    v.attachment_view_offsets = keep
        .iter()
        .map(|&i| src.attachment_view_offsets.get(i).cloned().flatten())
        .collect();
    let keep_ads: Vec<usize> = (0..src.ads_view_models.len())
        .filter(|&i| !donor_part(&src.ads_view_models[i]))
        .collect();
    v.ads_view_models = keep_ads
        .iter()
        .map(|&i| src.ads_view_models[i].clone())
        .collect();
    v.ads_view_offsets = keep_ads
        .iter()
        .map(|&i| src.ads_view_offsets.get(i).cloned().flatten())
        .collect();
    v.attachment_world_models.clear();
    v.world_parts.clear();
    v.hide_tags.clear();
    v.dual_wield_weapon = None;
    v.sounds = gun.sounds.clone();
    v.guest = gun.guest.clone();
    let f = &gun.facts;
    v.facts.ammo_index = 0;
    v.facts.clip_index = 0;
    v.facts.fire_type = f.fire_type;
    v.facts.fire_time_ms = f.fire_time_ms;
    v.facts.clip_size = f.clip_size;
    v.facts.max_ammo = f.max_ammo;
    v.facts.start_ammo = f.start_ammo;
    v
}

/// Games whose own 2D scope pictures the HUD cannot draw (their images are
/// not in loose image archives): Ghosts, Advanced Warfare, Black Ops III,
/// Modern Warfare II / III (2022-23) and Modern Warfare Remastered /
/// Horizon MW.
const UNDRAWN_SCOPE_GAMES: [crate::AssetNamespace; 6] = [
    crate::AssetNamespace::Iw6,
    crate::AssetNamespace::S1,
    crate::AssetNamespace::T7,
    crate::AssetNamespace::Jup,
    crate::AssetNamespace::H1,
    crate::AssetNamespace::Iw7,
];

/// A guest row that aims through its own game's scope (its def names an
/// overlay) with no Black Ops II sight or donor's scope to show instead.
fn needs_borrowed_scope(row: &WeaponRow) -> bool {
    UNDRAWN_SCOPE_GAMES.contains(&row.namespace)
        && row.guest.overlay_ns.is_none()
        && row.donor_namespace.is_none()
        && row
            .overlay_material
            .as_deref()
            .is_some_and(|m| !m.is_empty())
}

/// `row` aimed shows Black Ops II's `src` sight or scope: its overlay,
/// reticle and zoom.
fn borrow_sight(row: &mut WeaponRow, src: &WeaponRow) {
    row.overlay_material = src.overlay_material.clone();
    row.overlay_image = src.overlay_image.clone();
    row.overlay_material_from_slot = src.overlay_material_from_slot;
    row.guest.overlay_ns = Some(src.donor_namespace.unwrap_or(src.namespace));
    row.guest.reticle = Some(src.name.clone());
    let (f, s) = (&mut row.facts, &src.facts);
    f.overlay_reticle = s.overlay_reticle;
    f.overlay_interface = s.overlay_interface;
    f.ads_overlay_width = s.ads_overlay_width;
    f.ads_overlay_height = s.ads_overlay_height;
    f.ads_zoom_fov = s.ads_zoom_fov;
    f.variable_zoom_fovs = s.variable_zoom_fovs;
    f.hold_breath_to_steady = s.hold_breath_to_steady;
    f.sight_reticle_scale = s.sight_reticle_scale;
    f.thermal_scope = s.thermal_scope;
    f.mms = s.mms;
    if s.ads_zoom_in_frac > 0.0 {
        f.ads_zoom_in_frac = s.ads_zoom_in_frac;
    }
    if s.ads_zoom_out_frac > 0.0 {
        f.ads_zoom_out_frac = s.ads_zoom_out_frac;
    }
    if let Some(fov) = row.guest.zoom {
        f.ads_zoom_fov = fov;
    }
}

impl WeaponRegistry {
    /// bo2mp: the weapon whose sight reticle picture a weapon shows
    /// (`reticle:<name>`): a guest gun's borrowed Black Ops II sight's,
    /// else its own.
    pub fn sight_reticle_name_of(&self, index: u32) -> &str {
        self.rows
            .get(index as usize)
            .and_then(|row| row.guest.reticle.as_deref())
            .unwrap_or_else(|| self.name_of(index))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::AssetNamespace as Ns;

    fn row(name: &str, ns: Ns, overlay: Option<&str>) -> WeaponRow {
        let mut r = WeaponRow {
            name: name.to_owned(),
            namespace: ns,
            overlay_material: overlay.map(str::to_owned),
            ..WeaponRow::default()
        };
        r.facts.ads_zoom_fov = 65.0;
        r
    }

    fn build(rows: Vec<WeaponRow>) -> WeaponBuild {
        let mut b = WeaponBuild::default();
        b.registry.rows.push(WeaponRow::default());
        b.registry.rows.extend(rows);
        b.registry.rebuild_name_maps();
        b
    }

    fn bo2_scopes() -> Vec<WeaponRow> {
        let mut dsr = row("dsr50", Ns::T6, Some("scope_overlay_dsr50"));
        dsr.facts.ads_zoom_fov = 10.0;
        dsr.facts.ads_overlay_width = 320.0;
        dsr.facts.ads_overlay_height = 240.0;
        let mut ir = row("dsr50_mp+ir", Ns::T6, Some("scope_overlay_ir"));
        ir.facts.ads_zoom_fov = 12.0;
        ir.facts.thermal_scope = true;
        let mut svu = row("svu", Ns::T6, Some("scope_overlay_svu"));
        svu.facts.ads_zoom_fov = 20.0;
        vec![dsr, ir, svu]
    }

    #[test]
    fn thermal_scopes_are_read_from_weapon_names() {
        assert!(name_has_thermal_scope("cheytac_thermal_mp", Ns::Iw4));
        assert!(name_has_thermal_scope("gh_usr_mp+thermal", Ns::Iw6));
        assert!(name_has_thermal_scope("l96a1_ir_mp", Ns::T5));
        assert!(name_has_thermal_scope("dsr50_mp+ir", Ns::T6));
        assert!(!name_has_thermal_scope("ir_mp", Ns::T5));
        assert!(!name_has_thermal_scope("m4_fire_mp", Ns::Iw4));
        assert!(!name_has_thermal_scope("ak47_ir_mp", Ns::Iw4));
        let mut b = build(vec![
            row("cheytac_thermal", Ns::Iw4, None),
            row("cheytac", Ns::Iw4, None),
        ]);
        assert_eq!(b.mark_thermal_scopes(), 1);
        assert!(b.facts_of(1).is_some_and(|f| f.thermal_scope));
        assert!(!b.facts_of(2).is_some_and(|f| f.thermal_scope));
    }

    #[test]
    fn a_guest_sniper_scopes_in_with_black_ops_ii_s_scope() {
        let mut rows = bo2_scopes();
        // Ghosts' USR with its own scope found (bolt action).
        let mut usr = row("usr", Ns::Iw6, None);
        usr.facts.bolt_action = true;
        rows.push(usr);
        // Ghosts' VKS whose scope was not found but whose def names an
        // overlay of its own (not drawable here).
        rows.push(row("vks", Ns::Iw6, Some("iw6_scope_overlay")));
        // Modern Warfare 2's Intervention keeps its own scope.
        rows.push(row("cheytac", Ns::Iw4, Some("scope_overlay_cheytac")));
        let mut b = build(rows);
        b.give_guest_scope(
            4,
            GuestAttachment {
                sight: true,
                sight_from: Some(SNIPER_SCOPE.to_owned()),
                ..GuestAttachment::default()
            },
        );
        b.build_guest_variants();
        assert_eq!(b.overlay_material_of(4), Some("scope_overlay_dsr50"));
        assert_eq!(b.overlay_namespace_of(4), Some(Ns::T6));
        assert_eq!(b.facts_of(4).map(|f| f.ads_zoom_fov), Some(10.0));
        assert_eq!(b.facts_of(4).map(|f| f.ads_overlay_width), Some(320.0));
        // Not a bolt action: the SVU's.
        assert_eq!(b.overlay_material_of(5), Some("scope_overlay_svu"));
        assert_eq!(b.overlay_namespace_of(5), Some(Ns::T6));
        assert_eq!(b.facts_of(5).map(|f| f.ads_zoom_fov), Some(20.0));
        // A legacy gun's own picture is drawn: untouched.
        assert_eq!(b.overlay_material_of(6), Some("scope_overlay_cheytac"));
        assert_eq!(b.overlay_namespace_of(6), Some(Ns::Iw4));
    }

    #[test]
    fn a_guest_thermal_sight_sees_heat() {
        let mut rows = bo2_scopes();
        rows.push(row("usr", Ns::Iw6, None));
        let mut b = build(rows);
        b.offer_guest_attachments(
            4,
            vec![GuestAttachment {
                word: "thermal".to_owned(),
                sight: true,
                sight_from: Some("dsr50_mp+ir".to_owned()),
                ..GuestAttachment::default()
            }],
        );
        let census = b.build_guest_variants();
        assert!(census.missing.is_empty(), "{:?}", census.missing);
        let v = b
            .resolve_key(
                &crate::AssetKey::new(Ns::Iw6, crate::AssetKind::Weapon, "usr+thermal")
                    .expect("key"),
            )
            .ok()
            .flatten()
            .expect("variant");
        assert!(b.facts_of(v).is_some_and(|f| f.thermal_scope));
        assert_eq!(b.overlay_material_of(v), Some("scope_overlay_ir"));
        assert_eq!(b.overlay_namespace_of(v), Some(Ns::T6));
        assert!(!b.facts_of(4).is_some_and(|f| f.thermal_scope));
    }

    #[test]
    fn a_copy_of_a_black_ops_ii_sniper_shows_its_donor_s_scope() {
        let mut b = build(bo2_scopes());
        let id = b.add_guest_copy(
            1,
            crate::GuestCopy {
                namespace: Ns::Jup,
                name: "iw9_dm_mike24_mp".to_owned(),
                display_name_key: String::new(),
                gun_xmodel: String::new(),
                world_model: String::new(),
                fire_type: 1,
                fire_time_ms: 0,
                clip_size: 5,
                max_ammo: 25,
                sounds: Vec::new(),
                notetrack_remap: Vec::new(),
            },
        );
        assert_eq!(b.overlay_material_of(id), Some("scope_overlay_dsr50"));
        assert_eq!(b.overlay_namespace_of(id), Some(Ns::T6));
    }
}
