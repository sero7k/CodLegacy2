use crate::tick::WeaponCombatFacts;
use playerstate_iw4::pm_flags;

pub const BUTTON_MELEE: u32 = playerstate_iw4::buttons::MELEE_CHARGE;

pub const PLAYER_MELEE_RANGE_DEFAULT: f32 = 64.0;

pub const PLAYER_MELEE_WIDTH_DEFAULT: f32 = 10.0;
pub const PLAYER_MELEE_HEIGHT_DEFAULT: f32 = 10.0;

pub const MELEE_TRACE_OFFSETS: [[f32; 2]; 9] = [
    [0.0, 0.0],
    [0.5, 0.0],
    [-0.5, 0.0],
    [0.0, 0.5],
    [0.0, -0.5],
    [1.0, 1.0],
    [-1.0, 1.0],
    [1.0, -1.0],
    [-1.0, -1.0],
];

pub fn melee_trace_count(width: f32, height: f32) -> usize {
    if width > 0.0 || height > 0.0 {
        MELEE_TRACE_OFFSETS.len()
    } else {
        1
    }
}

pub fn melee_trace_end(
    muzzle: [f32; 3],
    forward: [f32; 3],
    right: [f32; 3],
    up: [f32; 3],
    range: f32,
    width: f32,
    height: f32,
    offset: [f32; 2],
) -> [f32; 3] {
    let wr = width * offset[0];
    let hu = height * offset[1];
    [
        muzzle[0] + forward[0] * range + right[0] * wr + up[0] * hu,
        muzzle[1] + forward[1] * range + right[1] * wr + up[1] * hu,
        muzzle[2] + forward[2] * range + right[2] * wr + up[2] * hu,
    ]
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct MeleeChargeState {
    pub pm_flags: u32,
    pub pm_type: i32,
    pub e_flags: u32,
    pub melee_charge_yaw: f32,
    pub melee_charge_dist: i32,
    pub melee_charge_time: i32,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MeleeWeaponFacts {
    pub melee_damage: i32,

    pub overlay_reticle: i32,

    pub melee_time_ms: i32,

    pub melee_delay_ms: i32,

    pub melee_charge_time_ms: i32,

    pub melee_charge_delay_ms: i32,

    pub melee_charge_anim: bool,

    pub knife_model: u32,

    pub quick_raise_time_ms: i32,
}

impl MeleeWeaponFacts {
    pub fn from_combat(facts: &WeaponCombatFacts) -> Self {
        Self {
            melee_damage: facts.melee_damage,
            overlay_reticle: facts.overlay_reticle,
            melee_time_ms: facts.melee_time_ms,
            melee_delay_ms: facts.melee_delay_ms,
            melee_charge_time_ms: facts.melee_charge_time_ms,
            melee_charge_delay_ms: facts.melee_charge_delay_ms,
            melee_charge_anim: facts.melee_charge_anim,
            knife_model: facts.knife_model,
            quick_raise_time_ms: facts.quick_raise_time_ms,
        }
    }
}

pub fn weapon_has_charge_melee(facts: &MeleeWeaponFacts) -> bool {
    facts.melee_charge_anim && facts.melee_charge_time_ms > 0
}

pub fn weapon_start_melee_uses_charge(
    charge: &MeleeChargeState,
    player_melee_range: f32,
    facts: &MeleeWeaponFacts,
) -> bool {
    (charge.pm_flags & pm_flags::MELEE_CHARGE) != 0
        && (charge.melee_charge_dist as f32) > player_melee_range
        && weapon_has_charge_melee(facts)
}

pub fn melee_charge_start(
    charge: &mut MeleeChargeState,
    cmd_melee_charge_yaw: f32,
    cmd_melee_charge_dist: u8,
    is_in_air: bool,
) {
    let can_start = (charge.pm_flags & pm_flags::MELEE_CHARGE) == 0
        && cmd_melee_charge_dist != 0
        && charge.pm_type == 0
        && (charge.e_flags & 0xc00) == 0
        && (charge.pm_flags & 0xc) == 0
        && !is_in_air;
    if can_start {
        charge.pm_flags |= pm_flags::MELEE_CHARGE;
        charge.melee_charge_yaw = cmd_melee_charge_yaw;
        charge.melee_charge_dist = i32::from(cmd_melee_charge_dist);
        charge.melee_charge_time = 0;
    } else {
        charge.pm_flags &= !pm_flags::MELEE_CHARGE;
        charge.melee_charge_yaw = 0.0;
        charge.melee_charge_dist = 0;
        charge.melee_charge_time = 0;
    }
}

