use crate::weaponstate::{FireType, WeaponDecodeError, WeaponState};

pub const PERK_FASTRELOAD: u32 = 4;

pub const PERK_WEAP_RELOAD_MULTIPLIER_DEFAULT: f32 = 0.5;

pub fn perk_fastreload_eligible(perks0: u32, inherits_perks: bool) -> bool {
    (perks0 & PERK_FASTRELOAD) != 0 && inherits_perks
}

/// bo2mp: Black Ops II's Fast Hands (`specialty_fastweaponswitch`), a bit
/// IW4 leaves free. Its scripts give it (`setperk`); without an engine side
/// the swap ran at full length.
pub const PERK_FASTWEAPONSWITCH: u32 = 1 << 12;

/// BO2's `perk_weapSwitchMultiplier` default (0.5, the engine's registered
/// value, as `t6::install` seeds it): raise and drop take this share of the
/// gun's own `iRaiseTime` / `iDropTime` / quick times, the way Fast Mag
/// (`perk_weapReloadMultiplier`) shortens the reload states.
pub const PERK_WEAP_SWITCH_MULTIPLIER_DEFAULT: f32 = 0.5;

/// Off: Fast Hands leaves the swap alone (the pre-fix behaviour).
pub const FAST_WEAPON_SWITCH_ENABLED: bool = true;

/// The weapon states Fast Hands shortens: every raise and drop, the
/// alternate-mode swap's included.
pub fn perk_fastweaponswitch_applies(perks0: u32, weaponstate: i32) -> bool {
    FAST_WEAPON_SWITCH_ENABLED
        && (perks0 & PERK_FASTWEAPONSWITCH) != 0
        && (0x1..=0x5).contains(&weaponstate)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MissingCombatFacts {
    BurstCooldown,

    SegmentedReloadAmmoAdd,

    BulletRange,

    UnknownFireType,

    LocationDamage,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct CapturedCombatInput {
    pub t6_weap_class: i32,
    pub t6_fire_type: i32,
    pub offhand_slot: i32,
    pub can_use_in_vehicle: bool,
    pub no_drops_or_raises: bool,
    pub no_quick_drop_when_empty: bool,
    pub alt_weapon_ads_only: bool,
    pub alt_weapon_disable_switching: bool,
    pub has_bayonet: bool,
    pub no_ads_auto_reload: bool,
    pub hold_button_to_throw: bool,
    pub is_hold_use_grenade: bool,
    pub t6_weap_type: i32,
    pub offhand_class: i32,
    pub hold_fire_time_ms: i32,
    pub fuse_time_ms: i32,
    pub cook_off_hold: bool,
    pub has_detonator: bool,
    pub detonate_time_ms: i32,
    pub detonate_delay_ms: i32,
    /// bo2mp: Black Ops II's other switch timers: the first raise of a
    /// life (`iFirstRaiseTime`), raising and dropping with an empty clip
    /// (`iEmptyRaiseTime`, `iEmptyDropTime`), and the time after the clip's
    /// last shot (`iLastFireTime`); 0: the usual timer.
    pub first_raise_time_ms: i32,
    pub empty_raise_time_ms: i32,
    pub empty_drop_time_ms: i32,
    pub last_fire_time_ms: i32,
    pub fire_time_ms: i32,
    /// bo2zm: Black Ops II's faster first shots of a trigger pull
    /// (`iIntroFireTime`, `iIntroFireLength`): the HAMR, AN-94 and B23R.
    pub intro_fire_time_ms: i32,
    pub intro_fire_length: i32,
    pub fire_delay_ms: i32,
    pub raise_time_ms: i32,
    pub drop_time_ms: i32,
    pub alternate_weapon: u32,
    pub alternate_raise_time_ms: i32,
    pub alternate_drop_time_ms: i32,
    pub reload_time_ms: i32,
    pub reload_empty_time_ms: i32,
    pub clip_size: i32,
    pub start_ammo: i32,
    pub max_ammo: i32,
    pub ammo_index: i32,
    pub clip_index: i32,
    pub fire_type: i32,

    pub weap_type: i32,

    pub weap_class: i32,

    pub player_anim_type: i32,

    pub inventory_type: i32,

    pub impact_type: i32,
    pub shots_per_fire: i32,
    pub burst_cooldown_ms: i32,
    pub bolt_action: bool,
    /// bo2mp: shots before the rechamber (0 = every shot).
    pub shots_before_rechamber: i32,
    pub rechamber_time_ms: i32,
    pub rechamber_bolt_time_ms: i32,

    pub rechamber_bolt_delay_ms: i32,
    pub segmented_reload: bool,
    pub reload_start_time_ms: i32,
    pub reload_end_time_ms: i32,
    pub reload_ammo_add: i32,

    pub reload_add_time_ms: i32,

    pub reload_empty_add_time_ms: i32,

    pub reload_start_add_time_ms: i32,

    pub reload_start_add: i32,
    pub no_partial_reload: bool,

    pub unlimited_ammo: bool,
    pub sprint_raise_time_ms: i32,
    pub sprint_drop_time_ms: i32,
    /// bo2mp: Black Ops II's damage over range, up to six (range, damage)
    /// points (`damageRange[]`, `damage[]`; a point with no range is
    /// unused): damage runs straight between neighbouring points, the
    /// first point's damage before it, the last's beyond it. Black Ops
    /// II's guns step by putting two points one unit apart. Empty: the
    /// near/far fields below.
    pub damage_points: [[f32; 2]; 6],
    pub damage: i32,
    pub min_damage: i32,
    pub max_damage_range: f32,
    pub min_damage_range: f32,
    pub hip_spread_stand_min: f32,
    pub hip_spread_ducked_min: f32,
    pub hip_spread_prone_min: f32,
    pub hip_spread_stand_max: f32,
    pub hip_spread_ducked_max: f32,
    pub hip_spread_prone_max: f32,
    pub hip_spread_decay_rate: f32,
    pub hip_spread_fire_add: f32,
    pub hip_spread_turn_add: f32,
    pub hip_spread_move_add: f32,
    pub hip_spread_ducked_decay: f32,
    pub hip_spread_prone_decay: f32,
    pub ads_spread: f32,

    pub aim_down_sight: bool,

    pub no_ads_when_mag_empty: bool,

    pub inherits_perks: bool,

    pub ads_in_rate: f32,

    pub ads_out_rate: f32,

    pub rechamber_while_ads: bool,

    /// BO2: a reload lowers the sights (!bReloadWhileAds).
    pub reload_lowers_ads: bool,

    /// BO2 iPositionReloadTransTime: the sights come back up when this
    /// much of the reload is left.
    pub position_reload_trans_ms: i32,

    pub ads_fire_only: bool,

    /// bo2mp: Hybrid Optic gun: its alternate weapon is the other sight.
    pub optic_switch: bool,

    pub melee_damage: i32,

    pub overlay_reticle: i32,

    pub melee_time_ms: i32,

    pub melee_delay_ms: i32,

    pub melee_charge_time_ms: i32,

    pub melee_charge_delay_ms: i32,

    pub melee_charge_anim: bool,

    pub knife_model: u32,

    pub quick_raise_time_ms: i32,

    pub quick_drop_time_ms: i32,

    pub select_requires_ammo: Option<bool>,

    pub offhand_hold_is_cancelable: Option<bool>,

    pub ads_gun_kick_reduced_kick_bullets: i32,

    pub hip_gun_kick_reduced_kick_bullets: i32,

    pub location_damage: [f32; crate::HITLOC_COUNT],

    pub dual_mag: Option<crate::reload::DualMagTimes>,

    /// bo2zm fix list 2: aim each shot with the spread before its own
    /// fire add (Black Ops II); false = Modern Warfare 2's order.
    pub spread_before_fire_add: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct AimAssistRanges {
    pub auto_aim: f32,
    pub hip: f32,
    pub ads: f32,
}

impl AimAssistRanges {
    pub const NONE: Self = Self {
        auto_aim: 0.0,
        hip: 0.0,
        ads: 0.0,
    };
}

#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct WeaponCombatFacts {
    pub aim_assist: AimAssistRanges,
    /// bo2zm: Black Ops II footstep rhythm scales while sprinting, sprinting
    /// crouched and diving (1.0: unchanged).
    pub sprint_cycle_scale: f32,
    pub ducked_sprint_cycle_scale: f32,
    pub dtp_cycle_scale: f32,
    /// bo2mp: Black Ops II's other switch timers: the first raise of a
    /// life (`iFirstRaiseTime`), raising and dropping with an empty clip
    /// (`iEmptyRaiseTime`, `iEmptyDropTime`), and the time after the clip's
    /// last shot (`iLastFireTime`); 0: the usual timer.
    /// bo2mp: Black Ops II's own def values (see `pm_weapon`).
    pub t6_weap_class: i32,
    pub t6_fire_type: i32,
    pub offhand_slot: i32,
    pub can_use_in_vehicle: bool,
    pub no_drops_or_raises: bool,
    pub no_quick_drop_when_empty: bool,
    pub alt_weapon_ads_only: bool,
    pub alt_weapon_disable_switching: bool,
    pub has_bayonet: bool,
    pub no_ads_auto_reload: bool,
    pub hold_button_to_throw: bool,
    pub is_hold_use_grenade: bool,
    pub t6_weap_type: i32,
    pub offhand_class: i32,
    pub hold_fire_time_ms: i32,
    pub fuse_time_ms: i32,
    pub cook_off_hold: bool,
    pub has_detonator: bool,
    pub detonate_time_ms: i32,
    pub detonate_delay_ms: i32,
    pub first_raise_time_ms: i32,
    pub empty_raise_time_ms: i32,
    pub empty_drop_time_ms: i32,
    pub last_fire_time_ms: i32,
    pub fire_time_ms: i32,
    /// bo2zm: the first `intro_fire_length` shots of a trigger pull come
    /// `intro_fire_time_ms` apart (Black Ops II's HAMR, AN-94, B23R).
    pub intro_fire_time_ms: i32,
    pub intro_fire_length: i32,
    pub fire_delay_ms: i32,
    pub raise_time_ms: i32,
    pub drop_time_ms: i32,
    pub alternate_weapon: u32,
    pub alternate_raise_time_ms: i32,
    pub alternate_drop_time_ms: i32,
    pub reload_time_ms: i32,
    pub reload_empty_time_ms: i32,
    pub clip_size: i32,
    pub start_ammo: i32,
    pub max_ammo: i32,

    pub ammo_index: i32,

    pub clip_index: i32,
    pub fire_type: i32,

    pub weap_type: i32,

    pub weap_class: i32,

    pub player_anim_type: i32,

    pub inventory_type: i32,

    pub impact_type: i32,

    pub shots_per_fire: i32,

    pub burst_cooldown_ms: i32,

    pub bolt_action: bool,
    /// bo2mp: Black Ops II `iShotsBeforeRechamber` (0 = every shot).
    pub shots_before_rechamber: i32,

    pub rechamber_time_ms: i32,

    pub rechamber_bolt_time_ms: i32,

    pub rechamber_bolt_delay_ms: i32,

    pub segmented_reload: bool,
    pub reload_start_time_ms: i32,
    pub reload_end_time_ms: i32,

    pub reload_ammo_add: i32,

    pub reload_add_time_ms: i32,

    pub reload_empty_add_time_ms: i32,

    pub reload_start_add_time_ms: i32,

    pub reload_start_add: i32,

    pub no_partial_reload: bool,

    pub unlimited_ammo: bool,

    pub inherits_perks: bool,

    pub sprint_raise_time_ms: i32,

    pub sprint_drop_time_ms: i32,
    /// bo2mp: Black Ops II's damage over range, up to six (range, damage)
    /// points (`damageRange[]`, `damage[]`; a point with no range is
    /// unused): damage runs straight between neighbouring points, the
    /// first point's damage before it, the last's beyond it. Black Ops
    /// II's guns step by putting two points one unit apart. Empty: the
    /// near/far fields below.
    pub damage_points: [[f32; 2]; 6],
    pub damage: i32,
    pub min_damage: i32,
    pub max_damage_range: f32,
    pub min_damage_range: f32,
    pub hip_spread_stand_min: f32,
    pub hip_spread_ducked_min: f32,
    pub hip_spread_prone_min: f32,
    pub hip_spread_stand_max: f32,
    pub hip_spread_ducked_max: f32,
    pub hip_spread_prone_max: f32,
    pub hip_spread_decay_rate: f32,
    pub hip_spread_fire_add: f32,
    pub hip_spread_turn_add: f32,
    pub hip_spread_move_add: f32,
    pub hip_spread_ducked_decay: f32,
    pub hip_spread_prone_decay: f32,
    pub ads_spread: f32,

    pub aim_down_sight: bool,

    pub no_ads_when_mag_empty: bool,

    pub ads_in_rate: f32,

    pub ads_out_rate: f32,

    pub rechamber_while_ads: bool,

    /// BO2: a reload lowers the sights (!bReloadWhileAds).
    pub reload_lowers_ads: bool,

    /// BO2 iPositionReloadTransTime: the sights come back up when this
    /// much of the reload is left.
    pub position_reload_trans_ms: i32,

    pub ads_fire_only: bool,

    pub optic_switch: bool,

    pub melee_damage: i32,

    pub overlay_reticle: i32,

    pub melee_time_ms: i32,

    pub melee_delay_ms: i32,

    pub melee_charge_time_ms: i32,

    pub melee_charge_delay_ms: i32,

    pub melee_charge_anim: bool,

    pub knife_model: u32,

    pub quick_raise_time_ms: i32,

    pub quick_drop_time_ms: i32,

    pub select_requires_ammo: Option<bool>,

    pub offhand_hold_is_cancelable: Option<bool>,

    pub ads_gun_kick_reduced_kick_bullets: i32,

    pub hip_gun_kick_reduced_kick_bullets: i32,

    pub location_damage: [f32; crate::HITLOC_COUNT],

    pub dual_mag: Option<crate::reload::DualMagTimes>,

    /// bo2zm fix list 2: see `CapturedCombatInput::spread_before_fire_add`.
    pub spread_before_fire_add: bool,
    /// bo2zm: a Black Ops II dual-wield weapon's left-hand weapon (0: not
    /// dual wield): given, it holds two clips and fires from both hands.
    pub dual_wield_weapon: u32,
    /// bo2zm: the grenade a Black Ops II gun's shot becomes for the
    /// scripts (`szSpawnedGrenadeWeaponName`: the Sliquifier's slip bolt);
    /// 0: none.
    pub spawned_grenade: u32,
    /// bo2zm: picked back up once placed (Black Ops II `bRetrievable`).
    pub retrievable: bool,
    /// bo2mp: Black Ops II's prone crawl weapon times: in, out after a shot
    /// cut the crawl short, out (`crawlInTime`, `crawlOutFireTime`,
    /// `crawlOutTime`).
    pub crawl_in_time_ms: i32,
    pub crawl_out_fire_time_ms: i32,
    pub crawl_out_time_ms: i32,
    /// bo2mp: Black Ops II's dive-to-prone weapon times: take-off and
    /// touch-down (`dtpInTime`, `dtpOutTime`).
    pub dtp_in_time_ms: i32,
    pub dtp_out_time_ms: i32,
    /// bo2mp: Black Ops II's `freezeMovementWhenFiring`: no shot while off
    /// the ground.
    pub freeze_movement_when_firing: bool,
}

impl Default for WeaponCombatFacts {
    fn default() -> Self {
        Self::none()
    }
}

impl WeaponCombatFacts {
    pub const fn none() -> Self {
        Self {
            aim_assist: AimAssistRanges::NONE,
            t6_weap_class: 0,
            t6_fire_type: 0,
            offhand_slot: 0,
            can_use_in_vehicle: false,
            no_drops_or_raises: false,
            no_quick_drop_when_empty: false,
            alt_weapon_ads_only: false,
            alt_weapon_disable_switching: false,
            has_bayonet: false,
            no_ads_auto_reload: false,
            hold_button_to_throw: false,
            is_hold_use_grenade: false,
            t6_weap_type: 0,
            offhand_class: 0,
            hold_fire_time_ms: 0,
            fuse_time_ms: 0,
            cook_off_hold: false,
            has_detonator: false,
            detonate_time_ms: 0,
            detonate_delay_ms: 0,
            first_raise_time_ms: 0,
            empty_raise_time_ms: 0,
            empty_drop_time_ms: 0,
            last_fire_time_ms: 0,
            fire_time_ms: 0,
            intro_fire_time_ms: 0,
            intro_fire_length: 0,
            fire_delay_ms: 0,
            raise_time_ms: 0,
            drop_time_ms: 0,
            alternate_weapon: 0,
            sprint_cycle_scale: 1.0,
            ducked_sprint_cycle_scale: 1.0,
            dtp_cycle_scale: 1.0,
            alternate_raise_time_ms: 0,
            alternate_drop_time_ms: 0,
            reload_time_ms: 0,
            reload_empty_time_ms: 0,
            clip_size: 0,
            start_ammo: 0,
            max_ammo: 0,
            ammo_index: 0,
            clip_index: 0,
            fire_type: 0,
            weap_type: 0,
            weap_class: 0,
            player_anim_type: 0,
            inventory_type: 0,
            impact_type: 0,
            shots_per_fire: 0,
            burst_cooldown_ms: 0,
            bolt_action: false,
            shots_before_rechamber: 0,
            rechamber_time_ms: 0,
            rechamber_bolt_time_ms: 0,
            rechamber_bolt_delay_ms: 0,
            segmented_reload: false,
            reload_start_time_ms: 0,
            reload_end_time_ms: 0,
            reload_ammo_add: 0,
            reload_add_time_ms: 0,
            reload_empty_add_time_ms: 0,
            reload_start_add_time_ms: 0,
            reload_start_add: 0,
            no_partial_reload: false,
            unlimited_ammo: false,
            sprint_raise_time_ms: 0,
            sprint_drop_time_ms: 0,
            damage_points: [[0.0; 2]; 6],
            damage: 0,
            min_damage: 0,
            max_damage_range: 0.0,
            min_damage_range: 0.0,
            hip_spread_stand_min: 0.0,
            hip_spread_ducked_min: 0.0,
            hip_spread_prone_min: 0.0,
            hip_spread_stand_max: 0.0,
            hip_spread_ducked_max: 0.0,
            hip_spread_prone_max: 0.0,
            hip_spread_decay_rate: 0.0,
            hip_spread_fire_add: 0.0,
            hip_spread_turn_add: 0.0,
            hip_spread_move_add: 0.0,
            hip_spread_ducked_decay: 0.0,
            hip_spread_prone_decay: 0.0,
            ads_spread: 0.0,
            aim_down_sight: false,
            no_ads_when_mag_empty: false,
            inherits_perks: false,
            ads_in_rate: 0.0,
            ads_out_rate: 0.0,
            rechamber_while_ads: true,
            reload_lowers_ads: false,
            position_reload_trans_ms: 0,
            ads_fire_only: false,
            optic_switch: false,
            melee_damage: 0,
            overlay_reticle: 0,
            melee_time_ms: 0,
            melee_delay_ms: 0,
            melee_charge_time_ms: 0,
            melee_charge_delay_ms: 0,
            melee_charge_anim: false,
            knife_model: 0,
            quick_raise_time_ms: 0,
            quick_drop_time_ms: 0,
            select_requires_ammo: Some(false),
            offhand_hold_is_cancelable: Some(false),
            ads_gun_kick_reduced_kick_bullets: 0,
            hip_gun_kick_reduced_kick_bullets: 0,
            location_damage: crate::LOCATION_DAMAGE_IDENTITY,
            dual_mag: None,
            spread_before_fire_add: false,
            dual_wield_weapon: 0,
            spawned_grenade: 0,
            retrievable: false,
            crawl_in_time_ms: 0,
            crawl_out_fire_time_ms: 0,
            crawl_out_time_ms: 0,
            dtp_in_time_ms: 0,
            dtp_out_time_ms: 0,
            freeze_movement_when_firing: false,
        }
    }

    pub fn test_fixture(_reason: &'static str, facts: Self) -> Self {
        facts
    }

    pub fn try_from_captured(input: CapturedCombatInput) -> Result<Self, MissingCombatFacts> {
        if input.fire_time_ms <= 0
            && input.raise_time_ms <= 0
            && input.damage <= 0
            && input.clip_size <= 0
        {
            return Ok(Self::none());
        }
        let fire_ty =
            FireType::from_i32(input.fire_type).map_err(|_| MissingCombatFacts::UnknownFireType)?;
        if fire_ty.is_burst() && input.burst_cooldown_ms <= 0 {
            return Err(MissingCombatFacts::BurstCooldown);
        }
        if input.segmented_reload && input.reload_ammo_add <= 0 {
            return Err(MissingCombatFacts::SegmentedReloadAmmoAdd);
        }
        if input.damage > 0 && input.max_damage_range <= 0.0 && input.min_damage_range <= 0.0 {
            return Err(MissingCombatFacts::BulletRange);
        }
        if !crate::location_damage_is_valid(&input.location_damage) {
            return Err(MissingCombatFacts::LocationDamage);
        }
        Ok(Self {
            aim_assist: AimAssistRanges::NONE,
            t6_weap_class: input.t6_weap_class,
            t6_fire_type: input.t6_fire_type,
            offhand_slot: input.offhand_slot,
            can_use_in_vehicle: input.can_use_in_vehicle,
            no_drops_or_raises: input.no_drops_or_raises,
            no_quick_drop_when_empty: input.no_quick_drop_when_empty,
            alt_weapon_ads_only: input.alt_weapon_ads_only,
            alt_weapon_disable_switching: input.alt_weapon_disable_switching,
            has_bayonet: input.has_bayonet,
            no_ads_auto_reload: input.no_ads_auto_reload,
            hold_button_to_throw: input.hold_button_to_throw,
            is_hold_use_grenade: input.is_hold_use_grenade,
            t6_weap_type: input.t6_weap_type,
            offhand_class: input.offhand_class,
            hold_fire_time_ms: input.hold_fire_time_ms,
            fuse_time_ms: input.fuse_time_ms,
            cook_off_hold: input.cook_off_hold,
            has_detonator: input.has_detonator,
            detonate_time_ms: input.detonate_time_ms,
            detonate_delay_ms: input.detonate_delay_ms,
            first_raise_time_ms: input.first_raise_time_ms,
            empty_raise_time_ms: input.empty_raise_time_ms,
            empty_drop_time_ms: input.empty_drop_time_ms,
            last_fire_time_ms: input.last_fire_time_ms,
            fire_time_ms: input.fire_time_ms,
            intro_fire_time_ms: input.intro_fire_time_ms,
            intro_fire_length: input.intro_fire_length,
            fire_delay_ms: input.fire_delay_ms,
            raise_time_ms: input.raise_time_ms,
            drop_time_ms: input.drop_time_ms,
            alternate_weapon: input.alternate_weapon,
            sprint_cycle_scale: 1.0,
            ducked_sprint_cycle_scale: 1.0,
            dtp_cycle_scale: 1.0,
            alternate_raise_time_ms: input.alternate_raise_time_ms,
            alternate_drop_time_ms: input.alternate_drop_time_ms,
            reload_time_ms: input.reload_time_ms,
            reload_empty_time_ms: input.reload_empty_time_ms,
            clip_size: input.clip_size,
            start_ammo: input.start_ammo,
            max_ammo: input.max_ammo,
            ammo_index: input.ammo_index,
            clip_index: input.clip_index,
            fire_type: input.fire_type,
            weap_type: input.weap_type,
            weap_class: input.weap_class,
            player_anim_type: input.player_anim_type,
            inventory_type: input.inventory_type,
            impact_type: input.impact_type,
            shots_per_fire: input.shots_per_fire,
            burst_cooldown_ms: input.burst_cooldown_ms,
            bolt_action: input.bolt_action,
            shots_before_rechamber: input.shots_before_rechamber,
            rechamber_time_ms: input.rechamber_time_ms,
            rechamber_bolt_time_ms: input.rechamber_bolt_time_ms,
            rechamber_bolt_delay_ms: input.rechamber_bolt_delay_ms,
            segmented_reload: input.segmented_reload,
            reload_start_time_ms: input.reload_start_time_ms,
            reload_end_time_ms: input.reload_end_time_ms,
            reload_ammo_add: input.reload_ammo_add,
            reload_add_time_ms: input.reload_add_time_ms,
            reload_empty_add_time_ms: input.reload_empty_add_time_ms,
            reload_start_add_time_ms: input.reload_start_add_time_ms,
            reload_start_add: input.reload_start_add,
            no_partial_reload: input.no_partial_reload,
            unlimited_ammo: input.unlimited_ammo,
            inherits_perks: input.inherits_perks,
            sprint_raise_time_ms: input.sprint_raise_time_ms,
            sprint_drop_time_ms: input.sprint_drop_time_ms,
            damage_points: input.damage_points,
            damage: input.damage,
            min_damage: input.min_damage,
            max_damage_range: input.max_damage_range,
            min_damage_range: input.min_damage_range,
            hip_spread_stand_min: input.hip_spread_stand_min,
            hip_spread_ducked_min: input.hip_spread_ducked_min,
            hip_spread_prone_min: input.hip_spread_prone_min,
            hip_spread_stand_max: input.hip_spread_stand_max,
            hip_spread_ducked_max: input.hip_spread_ducked_max,
            hip_spread_prone_max: input.hip_spread_prone_max,
            hip_spread_decay_rate: input.hip_spread_decay_rate,
            hip_spread_fire_add: input.hip_spread_fire_add,
            hip_spread_turn_add: input.hip_spread_turn_add,
            hip_spread_move_add: input.hip_spread_move_add,
            hip_spread_ducked_decay: input.hip_spread_ducked_decay,
            hip_spread_prone_decay: input.hip_spread_prone_decay,
            ads_spread: input.ads_spread,
            aim_down_sight: input.aim_down_sight,
            no_ads_when_mag_empty: input.no_ads_when_mag_empty,
            ads_in_rate: input.ads_in_rate,
            ads_out_rate: input.ads_out_rate,
            rechamber_while_ads: input.rechamber_while_ads,
            reload_lowers_ads: input.reload_lowers_ads,
            position_reload_trans_ms: input.position_reload_trans_ms,
            ads_fire_only: input.ads_fire_only,
            optic_switch: input.optic_switch,
            melee_damage: input.melee_damage,
            overlay_reticle: input.overlay_reticle,
            melee_time_ms: input.melee_time_ms,
            melee_delay_ms: input.melee_delay_ms,
            melee_charge_time_ms: input.melee_charge_time_ms,
            melee_charge_delay_ms: input.melee_charge_delay_ms,
            melee_charge_anim: input.melee_charge_anim,
            knife_model: input.knife_model,
            quick_raise_time_ms: input.quick_raise_time_ms,
            quick_drop_time_ms: input.quick_drop_time_ms,
            select_requires_ammo: input.select_requires_ammo,
            offhand_hold_is_cancelable: input.offhand_hold_is_cancelable,
            ads_gun_kick_reduced_kick_bullets: input.ads_gun_kick_reduced_kick_bullets,
            hip_gun_kick_reduced_kick_bullets: input.hip_gun_kick_reduced_kick_bullets,
            location_damage: input.location_damage,
            dual_mag: input.dual_mag,
            spread_before_fire_add: input.spread_before_fire_add,
            dual_wield_weapon: 0,
            spawned_grenade: 0,
            retrievable: false,
            crawl_in_time_ms: 0,
            crawl_out_fire_time_ms: 0,
            crawl_out_time_ms: 0,
            dtp_in_time_ms: 0,
            dtp_out_time_ms: 0,
            freeze_movement_when_firing: false,
        })
    }

    pub fn is_none(self) -> bool {
        self.fire_time_ms <= 0 && self.raise_time_ms <= 0 && self.damage <= 0 && self.clip_size <= 0
    }

    pub fn is_usable(self) -> bool {
        !self.is_none()
    }

    pub fn fire_type_enum(self) -> Result<FireType, WeaponDecodeError> {
        FireType::from_i32(self.fire_type)
    }

    pub fn ammo_per_shot(self) -> i32 {
        1
    }

    pub fn pellet_count(self) -> i32 {
        if self.weap_class == crate::WEAPCLASS_SPREAD {
            if self.shots_per_fire <= 0 {
                1
            } else {
                self.shots_per_fire
            }
        } else {
            1
        }
    }

    pub fn reload_duration_ms(self, clip_empty: bool) -> i32 {
        if clip_empty && self.reload_empty_time_ms > 0 {
            self.reload_empty_time_ms
        } else {
            self.reload_time_ms
        }
    }

    pub fn burst_cooldown(self) -> i32 {
        self.burst_cooldown_ms
    }

    pub fn shells_per_reload_add(self) -> i32 {
        self.reload_ammo_add
    }

    pub fn bullet_range(self) -> f32 {
        if self.weap_class == crate::WEAPCLASS_SPREAD {
            self.min_damage_range
        } else {
            crate::BULLET_MAX_RANGE
        }
    }

    pub fn location_scale(self, hitloc: u8) -> f32 {
        crate::location_damage_scale(&self.location_damage, hitloc)
    }

    pub fn spread_facts(self) -> crate::WeaponSpreadFacts {
        crate::WeaponSpreadFacts {
            stand_min: self.hip_spread_stand_min,
            ducked_min: self.hip_spread_ducked_min,
            prone_min: self.hip_spread_prone_min,
            stand_max: self.hip_spread_stand_max,
            ducked_max: self.hip_spread_ducked_max,
            prone_max: self.hip_spread_prone_max,
        }
    }

    pub fn aim_spread_decay_facts(self) -> crate::WeaponAimSpreadDecayFacts {
        crate::WeaponAimSpreadDecayFacts {
            decay_rate: self.hip_spread_decay_rate,
            fire_add: self.hip_spread_fire_add,
            turn_add: self.hip_spread_turn_add,
            move_add: self.hip_spread_move_add,
            ducked_decay: self.hip_spread_ducked_decay,
            prone_decay: self.hip_spread_prone_decay,
        }
    }

    pub fn ads_allow_facts(self) -> crate::AdsAllowWeaponFacts {
        crate::AdsAllowWeaponFacts {
            aim_down_sight: self.aim_down_sight,
            no_ads_when_mag_empty: self.no_ads_when_mag_empty,
            clip_index: self.clip_index,
            optic_switch: self.optic_switch,
        }
    }

    pub fn ads_frac_context(self) -> (f32, f32, bool, bool) {
        (
            self.ads_in_rate,
            self.ads_out_rate,
            self.rechamber_while_ads,
            self.ads_fire_only,
        )
    }

    /// BO2's reload rule for the sights: (a reload lowers them, how much of
    /// the reload is left when they come back up, segmented reload).
    pub fn ads_reload_rule(self) -> (bool, i32, bool) {
        (
            self.reload_lowers_ads,
            self.position_reload_trans_ms,
            self.segmented_reload,
        )
    }

    pub fn melee_facts(self) -> crate::MeleeWeaponFacts {
        crate::MeleeWeaponFacts::from_combat(&self)
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct WeaponHandState {
    pub weapon: u32,
    pub weaponstate: i32,
    pub weapon_time: i32,
    pub weapon_delay: i32,

    pub weap_anim: i32,

    pub hand_index: u8,
    pub clip: i32,
    pub stock: i32,

    pub shot_count: u8,

    pub burst_latch: bool,

    pub rechamber_pending: bool,

    pub delayed_rechamber: bool,

    pub weapon_restrict_kick_time: i32,

    pub quick_reload: bool,
    /// bo2mp: Black Ops II's weapon flags (`pm_weapon::flags`).
    pub flags: u32,
    /// bo2mp: shots since the trigger was pulled (Black Ops II's intro
    /// fire counter, capped at 250).
    pub firing_counter: u8,
}

#[derive(Clone, Copy, Debug)]
pub struct WeaponCmd {
    pub msec: i32,
    pub buttons: u32,
    pub old_buttons: u32,

    pub cmd_weapon: u16,

    pub pm_flags: u32,

    pub weap_flags: u32,

    pub pm_type: i32,

    pub e_flags: u32,

    pub last_weapon_hand: i32,

    pub f_weapon_pos_frac: f32,

    pub melee_charge_yaw: f32,

    pub melee_charge_dist: u8,

    pub player_melee_range: f32,

    pub is_in_air: bool,

    pub melee_charge: crate::MeleeChargeState,

    pub melee_started: Option<bool>,

    pub mantle_weapon_inactive: bool,

    pub mantle_quick_raise: bool,

    /// bo2zm: a low Black Ops II climb: the gun stays up and fires, but
    /// cannot be switched or start a reload until the climb ends.
    pub mantle_weapon_up: bool,

    pub cmd_weapon_owned: bool,

    pub cmd_weapon_pistol_quick: bool,

    pub switch_raise_time_ms: i32,
    pub switch_alternate_raise_time_ms: i32,
    pub alternate_switch: bool,

    pub switch_quick_raise_time_ms: i32,
    /// bo2mp: the gun being switched to has an empty clip and its own
    /// `iEmptyRaiseTime` (`switch_raise_time_ms` is then that time): Black
    /// Ops II raises it with the empty raise, ahead of a quick raise.
    pub switch_raise_empty: bool,
    /// bo2mp: the player's shots since his last rechamber (Black Ops II
    /// keeps one count per player, for `shots_before_rechamber`).
    pub rechamber_shot_count: u8,

    pub offhand: crate::offhand::OffhandCmd,

    pub perks0: u32,

    pub perk_weap_reload_multiplier: f32,

    /// bo2zm: the move input (forward, right) the crawl follows.
    pub move_input: [i8; 2],
    /// bo2mp: the player stands on something (a ground entity).
    pub on_ground: bool,
    /// bo2mp: the still moment after a dive's slide.
    pub dive_pause: bool,
    /// bo2mp: the raised riot shield has room to stand.
    pub shield_can_deploy: bool,
    /// bo2mp: the weapon asked for: its facts, its clip and whether it has
    /// made its first raise of the life yet.
    pub cmd_facts: WeaponCombatFacts,
    pub cmd_clip: i32,
    pub cmd_first_raise: bool,
    /// bo2mp: out: the weapon that just began its raise (no longer first).
    pub first_raised: Option<u32>,
    /// bo2mp: the weapon in hand is still the player's.
    pub held_weapon_owned: bool,
    /// bo2mp: the knife a gun without a bayonet melees with.
    pub knife: Option<crate::MeleeWeaponFacts>,
}

impl Default for WeaponCmd {
    fn default() -> Self {
        Self {
            msec: 0,
            buttons: 0,
            old_buttons: 0,
            cmd_weapon: 0,
            pm_flags: 0,
            weap_flags: 0,
            pm_type: 0,
            e_flags: 0,
            last_weapon_hand: 0,
            f_weapon_pos_frac: 0.0,
            melee_charge_yaw: 0.0,
            melee_charge_dist: 0,
            player_melee_range: crate::PLAYER_MELEE_RANGE_DEFAULT,
            is_in_air: false,
            melee_charge: crate::MeleeChargeState::default(),
            melee_started: None,
            mantle_weapon_inactive: false,
            mantle_quick_raise: false,
            mantle_weapon_up: false,
            cmd_weapon_owned: false,
            cmd_weapon_pistol_quick: false,
            switch_raise_time_ms: 0,
            switch_alternate_raise_time_ms: 0,
            alternate_switch: false,
            switch_quick_raise_time_ms: 0,
            switch_raise_empty: false,
            rechamber_shot_count: 0,
            offhand: crate::offhand::OffhandCmd::default(),
            perks0: 0,
            perk_weap_reload_multiplier: PERK_WEAP_RELOAD_MULTIPLIER_DEFAULT,
            move_input: [0; 2],
            on_ground: false,
            dive_pause: false,
            shield_can_deploy: true,
            cmd_facts: WeaponCombatFacts::none(),
            cmd_clip: 0,
            cmd_first_raise: false,
            first_raised: None,
            held_weapon_owned: true,
            knife: None,
        }
    }
}

pub const BUTTON_RELOAD: u32 = playerstate_iw4::buttons::RELOAD;

pub const BUTTON_ATTACK: u32 = playerstate_iw4::buttons::ATTACK;

pub const BUTTON_THROW: u32 = playerstate_iw4::buttons::THROW;

pub fn get_weapon_fire_button(last_weapon_hand: i32, hand_index: i32) -> u32 {
    let left = hand_index != 0;
    if last_weapon_hand == 1 {
        if left { BUTTON_ATTACK } else { BUTTON_THROW }
    } else if left {
        BUTTON_THROW
    } else {
        BUTTON_ATTACK
    }
}

pub fn ads_fire_only_delay_ms(frac: f32, ads_in_rate: f32) -> i32 {
    if ads_in_rate <= 0.0 {
        return 0;
    }
    let remaining = (1.0 - frac).max(0.0);
    (remaining / ads_in_rate) as i32
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WeaponTickEvent {
    Detonated {
        weapon: u32,
    },
    ShotAccepted {
        ammo_used: i32,
    },
    EmptyClick,
    ReloadStarted,

    ReloadInsert,

    ReloadEnded,

    ReloadAmmoAdded {
        shells: i32,
    },

    RechamberWeapon,

    EjectBrass,
    RaiseFinished,
    DropFinished,

    PutawayStarted,
    AlternateStarted,

    RaiseStarted,

    OffhandUsed {
        weapon: u32,
        remaining_fuse_ms: Option<i32>,
    },

    OffhandCookedOff {
        weapon: u32,
    },

    OffhandPrepare {
        weapon: u32,
    },

    MeleeFired,
    /// A weapon change cut into a reload (the state it cut into).
    ReloadCancelled {
        weaponstate: i32,
    },
    /// The weapon in hand begins going down for another.
    SwitchStarted {
        weapon: u32,
    },
    /// The riot shield is raised to be planted.
    ShieldRaised,
    /// The raised riot shield is planted (true) or called off (false).
    ShieldDeployed {
        planted: bool,
    },
}

/// One hand's weapon frame; the first event it raised.
pub fn weapon_ordinary(
    hand: &mut WeaponHandState,
    facts: &WeaponCombatFacts,
    cmd: &mut WeaponCmd,
) -> Option<WeaponTickEvent> {
    crate::pm_weapon::pm_weapon(hand, facts, cmd).first()
}

/// Every event one frame of both hands raised, with the hand that raised it.
pub type HandEvents = [Option<(u8, WeaponTickEvent)>; 2 * crate::pm_weapon::MAX_EVENTS];

pub fn weapon_hands(
    hands: &mut [WeaponHandState],
    facts: &WeaponCombatFacts,
    cmd: &mut WeaponCmd,
    last_hand: i32,
) -> HandEvents {
    let mut out: HandEvents = [None; 2 * crate::pm_weapon::MAX_EVENTS];
    let mut k = 0;
    cmd.melee_charge.pm_flags = cmd.pm_flags;
    cmd.melee_charge.pm_type = cmd.pm_type;
    cmd.melee_charge.e_flags = cmd.e_flags;
    let last = last_hand.clamp(0, 1) as usize;
    let n = hands.len().min(last + 1);
    for i in 0..n {
        hands[i].hand_index = i as u8;
        // bo2zm: one stock for both hands (the engine's single ammo pool).
        if i > 0 {
            hands[i].stock = hands[i - 1].stock;
        }
        for ev in crate::pm_weapon::pm_weapon(&mut hands[i], facts, cmd).iter() {
            if k < out.len() {
                out[k] = Some((i as u8, ev));
                k += 1;
            }
        }
    }
    if n > 1 {
        let stock = hands[n - 1].stock;
        for hand in hands.iter_mut().take(n) {
            hand.stock = stock;
        }
    }
    out
}

pub fn spawn_clip_stock(facts: &WeaponCombatFacts, last_hand: i32) -> (i32, i32, i32) {
    let total = facts.start_ammo.max(0);
    let mag = facts.clip_size.max(0);
    let clip0 = if mag > 0 { total.min(mag) } else { 0 };
    let rest = (total - clip0).max(0);
    let clip1 = if last_hand >= 1 && mag > 0 {
        rest.min(mag)
    } else {
        0
    };
    let stock = (rest - clip1).max(0);
    (clip0, clip1, stock)
}

pub fn spawn_weapon_hand(weapon: u32, facts: &WeaponCombatFacts) -> WeaponHandState {
    let total = facts.start_ammo.max(0);
    let clip = if facts.clip_size > 0 {
        total.min(facts.clip_size)
    } else {
        0
    };
    let stock = (total - clip).max(0);
    // bo2mp: a weapon given at spawn makes its first raise of the life
    // (BO2's `iFirstRaiseTime`, the first-raise animation the view plays).
    let raise = if facts.first_raise_time_ms > 0 {
        facts.first_raise_time_ms
    } else if facts.raise_time_ms > 0 {
        facts.raise_time_ms
    } else {
        0
    };
    let mut weap_anim = 0;
    if raise > 0 {
        crate::weap_anim::start_weapon_anim(
            &mut weap_anim,
            if facts.first_raise_time_ms > 0 {
                crate::weap_anim::weap_anim_event::FIRST_RAISE
            } else {
                crate::weap_anim::weap_anim_event::RAISE
            },
        );
    }
    WeaponHandState {
        weapon,
        weaponstate: if raise > 0 {
            WeaponState::Raising as i32
        } else {
            WeaponState::Ready as i32
        },
        weapon_time: raise,
        weapon_delay: 0,
        weap_anim,
        hand_index: 0,
        clip,
        stock,
        shot_count: 0,
        burst_latch: false,
        rechamber_pending: false,
        delayed_rechamber: false,
        weapon_restrict_kick_time: 0,
        quick_reload: facts.dual_mag.is_some(),
        flags: 0,
        firing_counter: 0,
    }
}

#[cfg(test)]
mod intro_fire_tests {
    extern crate std;
    use super::*;
    use std::vec::Vec;

    /// Holds the trigger for `ms` (or lets it go) and returns the times,
    /// from the start, of each shot.
    fn hold(
        hand: &mut WeaponHandState,
        facts: &WeaponCombatFacts,
        attack: bool,
        ms: i32,
    ) -> Vec<i32> {
        let mut shots = Vec::new();
        let mut t = 0;
        while t < ms {
            let clip = hand.clip;
            let mut cmd = WeaponCmd {
                msec: 8,
                buttons: if attack { BUTTON_ATTACK } else { 0 },
                old_buttons: if attack { BUTTON_ATTACK } else { 0 },
                cmd_weapon: hand.weapon as u16,
                ..WeaponCmd::default()
            };
            weapon_ordinary(hand, facts, &mut cmd);
            if hand.clip < clip {
                shots.push(t);
            }
            t += 8;
        }
        shots
    }

    fn gaps(shots: &[i32]) -> Vec<i32> {
        shots.windows(2).map(|w| w[1] - w[0]).collect()
    }

    /// Black Ops II's HAMR: its first 6 shots of a pull 64 ms apart, then 96.
    #[test]
    fn the_hamr_fires_its_first_shots_faster_every_pull() {
        let facts = WeaponCombatFacts {
            fire_time_ms: 96,
            intro_fire_time_ms: 64,
            intro_fire_length: 6,
            clip_size: 100,
            start_ammo: 100,
            fire_type: FireType::FullAuto as i32,
            ..WeaponCombatFacts::default()
        };
        let mut hand = WeaponHandState {
            weapon: 1,
            clip: 100,
            ..WeaponHandState::default()
        };
        let pull = hold(&mut hand, &facts, true, 1000);
        assert_eq!(gaps(&pull)[..8], [64, 64, 64, 64, 64, 64, 96, 96]);
        hold(&mut hand, &facts, false, 200);
        assert_eq!(hand.shot_count, 0);
        let again = hold(&mut hand, &facts, true, 1000);
        assert_eq!(gaps(&again)[..8], [64, 64, 64, 64, 64, 64, 96, 96]);
    }

    #[test]
    fn a_gun_without_first_shots_fires_evenly() {
        let facts = WeaponCombatFacts {
            fire_time_ms: 96,
            clip_size: 100,
            start_ammo: 100,
            fire_type: FireType::FullAuto as i32,
            ..WeaponCombatFacts::default()
        };
        let mut hand = WeaponHandState {
            weapon: 1,
            clip: 100,
            ..WeaponHandState::default()
        };
        let pull = hold(&mut hand, &facts, true, 1000);
        assert!(gaps(&pull).iter().all(|&g| g == 96), "{:?}", gaps(&pull));
        assert_eq!(hand.shot_count, 0);
    }
}

#[cfg(test)]
mod fast_switch_tests {
    use super::*;

    fn decay(state: WeaponState, perks0: u32) -> i32 {
        let hand = WeaponHandState {
            weaponstate: state as i32,
            weapon_time: 600,
            ..WeaponHandState::default()
        };
        let cmd = WeaponCmd {
            msec: 50,
            perks0,
            ..WeaponCmd::default()
        };
        crate::pm_weapon::timer_msec(&hand, &WeaponCombatFacts::default(), &cmd)
    }

    #[test]
    fn fast_hands_halves_every_raise_and_drop() {
        let fh = PERK_FASTWEAPONSWITCH;
        // perk_weapSwitchMultiplier 0.5: the timers run twice as fast.
        assert_eq!(decay(WeaponState::Raising, fh), 100);
        assert_eq!(decay(WeaponState::Dropping, fh), 100);
        assert_eq!(decay(WeaponState::DroppingQuick, fh), 100);
        // Without the perk, and outside the swap, nothing changes.
        assert_eq!(decay(WeaponState::Raising, 0), 50);
        assert_eq!(decay(WeaponState::Firing, fh), 50);
        assert_eq!(decay(WeaponState::RaisingAltswitch, fh), 100);
        assert_eq!(decay(WeaponState::SprintOut, fh), 50);
    }

    #[test]
    fn fast_hands_bit_is_free() {
        for other in [PERK_FASTRELOAD, crate::PERK_SPRINTRECOVERY, crate::PERK_BULLETACCURACY] {
            assert_eq!(PERK_FASTWEAPONSWITCH & other, 0);
        }
    }
}

#[cfg(test)]
mod bo2_timer_tests {
    extern crate std;
    use super::*;

    fn gun() -> WeaponCombatFacts {
        WeaponCombatFacts {
            fire_time_ms: 100,
            raise_time_ms: 500,
            drop_time_ms: 400,
            clip_size: 3,
            start_ammo: 30,
            fire_type: FireType::SingleShot as i32,
            ..WeaponCombatFacts::default()
        }
    }

    /// Fires one shot from a ready hand; the weapon time it is left with.
    fn one_shot(hand: &mut WeaponHandState, facts: &WeaponCombatFacts) -> i32 {
        let mut cmd = WeaponCmd {
            msec: 1,
            buttons: BUTTON_ATTACK,
            old_buttons: 0,
            cmd_weapon: hand.weapon as u16,
            ..WeaponCmd::default()
        };
        weapon_ordinary(hand, facts, &mut cmd);
        hand.weapon_time
    }

    /// BO2's KSG: the clip's last shot waits `iLastFireTime` (512), the
    /// others `iFireTime`.
    #[test]
    fn the_last_shot_of_a_clip_takes_the_last_fire_time() {
        let facts = WeaponCombatFacts { last_fire_time_ms: 512, ..gun() };
        let mut hand = WeaponHandState { weapon: 1, clip: 2, ..WeaponHandState::default() };
        assert!((99..=100).contains(&one_shot(&mut hand, &facts)), "an ordinary shot");
        hand.weapon_time = 0;
        hand.weaponstate = WeaponState::Ready as i32;
        assert!((511..=512).contains(&one_shot(&mut hand, &facts)), "the last shot");
        assert_eq!(hand.clip, 0);
    }

    /// A gun with no last-fire time fires its last shot like any other.
    #[test]
    fn without_a_last_fire_time_the_last_shot_is_ordinary() {
        let facts = gun();
        let mut hand = WeaponHandState { weapon: 1, clip: 1, ..WeaponHandState::default() };
        assert!((99..=100).contains(&one_shot(&mut hand, &facts)));
    }

    /// The weapon given at spawn makes its first raise (`iFirstRaiseTime`).
    #[test]
    fn a_spawned_weapon_makes_its_first_raise() {
        let facts = WeaponCombatFacts { first_raise_time_ms: 1600, ..gun() };
        let hand = spawn_weapon_hand(1, &facts);
        assert_eq!(hand.weaponstate, WeaponState::Raising as i32);
        assert_eq!(hand.weapon_time, 1600);
        assert_eq!(spawn_weapon_hand(1, &gun()).weapon_time, 500);
    }

    /// An empty gun drops on `iEmptyDropTime`; a loaded one on its drop time.
    #[test]
    fn an_empty_gun_drops_on_its_empty_drop_time() {
        let facts = WeaponCombatFacts { empty_drop_time_ms: 750, ..gun() };
        let switch = |clip: i32| {
            let mut hand = WeaponHandState { weapon: 1, clip, ..WeaponHandState::default() };
            let mut cmd = WeaponCmd {
                msec: 1,
                cmd_weapon: 2,
                cmd_weapon_owned: true,
                cmd_facts: gun(),
                ..WeaponCmd::default()
            };
            weapon_ordinary(&mut hand, &facts, &mut cmd);
            assert_eq!(hand.weaponstate, WeaponState::Dropping as i32);
            hand.weapon_time
        };
        assert_eq!(switch(0), 750);
        assert_eq!(switch(3), 400);
    }
}

#[cfg(test)]
mod fire_delay_tests {
    extern crate std;
    use super::*;
    use std::vec::Vec;

    /// Taps the trigger on the first tick only, then runs for `ms` and
    /// returns the times of each shot and the state at the end.
    fn tap(hand: &mut WeaponHandState, facts: &WeaponCombatFacts, ms: i32) -> Vec<i32> {
        let mut shots = Vec::new();
        let mut t = 0;
        while t < ms {
            let mut cmd = WeaponCmd {
                msec: 8,
                buttons: if t == 0 { BUTTON_ATTACK } else { 0 },
                cmd_weapon: hand.weapon as u16,
                ..WeaponCmd::default()
            };
            if let Some(WeaponTickEvent::ShotAccepted { .. }) =
                weapon_ordinary(hand, facts, &mut cmd)
            {
                shots.push(t);
            }
            t += 8;
        }
        shots
    }

    /// Black Ops II's claymore (claymore_zm: fireTime 992, fireDelay 127):
    /// a tap plants it once, when the delay runs out.
    #[test]
    fn a_tap_fires_a_weapon_with_a_fire_delay_once_after_the_delay() {
        let facts = WeaponCombatFacts {
            weap_type: crate::WEAPTYPE_GRENADE,
            fire_time_ms: 992,
            fire_delay_ms: 127,
            clip_size: 1,
            start_ammo: 2,
            fire_type: FireType::SingleShot as i32,
            ..WeaponCombatFacts::default()
        };
        let mut hand = WeaponHandState {
            weapon: 1,
            clip: 1,
            ..WeaponHandState::default()
        };
        let shots = tap(&mut hand, &facts, 1500);
        assert_eq!(shots.len(), 1, "{:?}", shots);
        assert!((120..=136).contains(&shots[0]), "{:?}", shots);
        assert_eq!(hand.clip, 0);
        assert_ne!(hand.weaponstate, WeaponState::Firing as i32);
    }

    /// No delay: the pull fires at once, as before.
    #[test]
    fn a_tap_fires_a_weapon_without_a_delay_at_once() {
        let facts = WeaponCombatFacts {
            fire_time_ms: 200,
            clip_size: 10,
            start_ammo: 10,
            fire_type: FireType::SingleShot as i32,
            ..WeaponCombatFacts::default()
        };
        let mut hand = WeaponHandState {
            weapon: 1,
            clip: 10,
            ..WeaponHandState::default()
        };
        assert_eq!(tap(&mut hand, &facts, 600), [0]);
        assert_eq!(hand.clip, 9);
    }
}
