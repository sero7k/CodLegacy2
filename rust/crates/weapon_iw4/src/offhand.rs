use playerstate_iw4::buttons;

pub const BUTTON_FRAG: u32 = buttons::FRAG;

pub const BUTTON_SMOKE: u32 = buttons::SMOKE;

pub const CURSOR_HINT_NONE: i32 = 0x7ff;

pub const OFFHAND_INV_SLOTS: usize = 15;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct OffhandInvRow {
    pub weapon: u32,

    pub offhand_class: i32,

    pub ammo: i32,

    pub hold_fire_time_ms: i32,

    pub fire_time_ms: i32,

    pub fire_delay_ms: i32,

    pub fuse_time_ms: i32,

    pub cook_off_hold: bool,

    pub offhand_hold_is_cancelable: Option<bool>,

    pub weap_type: i32,
    pub has_detonator: bool,
    pub detonate_delay_ms: i32,
    pub detonate_time_ms: i32,
    pub combat: crate::WeaponCombatFacts,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct OffhandCmd {
    pub inventory: [OffhandInvRow; OFFHAND_INV_SLOTS],

    pub offhand_primary: i32,

    pub offhand_secondary: i32,

    pub cmd_off_hand_index: u16,
    pub cmd_off_hand_owned: bool,

    pub cursor_hint_ent: i32,

    pub held_quick_drop_time_ms: i32,

    pub off_hand_index: i32,

    pub grenade_time_left: i32,
}

impl Default for OffhandCmd {
    fn default() -> Self {
        Self {
            inventory: [OffhandInvRow::default(); OFFHAND_INV_SLOTS],
            offhand_primary: 0,
            offhand_secondary: 0,
            cmd_off_hand_index: 0,
            cmd_off_hand_owned: false,
            cursor_hint_ent: CURSOR_HINT_NONE,
            held_quick_drop_time_ms: 0,
            off_hand_index: 0,
            grenade_time_left: 0,
        }
    }
}

pub fn get_first_available_offhand(
    inventory: &[OffhandInvRow; OFFHAND_INV_SLOTS],
    wanted_class: i32,
) -> u32 {
    if wanted_class == 0 {
        return 0;
    }
    for row in inventory {
        if row.weapon == 0 {
            continue;
        }
        if row.offhand_class != wanted_class {
            continue;
        }
        if row.ammo > 0 {
            return row.weapon;
        }
    }
    0
}


