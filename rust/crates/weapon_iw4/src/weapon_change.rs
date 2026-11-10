use crate::tick::WeaponCmd;
use playerstate_iw4::pm_flags;

pub const PMF_CHANGE_BLOCK: u32 = 0xc00;

#[inline]
pub fn traversal_forces_holster(cmd: &WeaponCmd) -> bool {
    cmd.mantle_weapon_inactive || (cmd.pm_flags & pm_flags::LADDER) != 0
}

