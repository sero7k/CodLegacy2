
/// Sprint recovery (specialty_sprintrecovery).
pub const PERK_SPRINTRECOVERY: u32 = 1 << 10;

/// perk_sprintRecoveryMultiplier: with sprint recovery the gun comes up after
/// a sprint in this share of its own time (BO2 default 0.3).
pub const SPRINT_RECOVERY_MULTIPLIER: f32 = 0.3;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tick::{WeaponCombatFacts, WeaponHandState};
    use crate::weap_anim::weap_anim_event;
    use crate::weaponstate::WeaponState;
    use playerstate_iw4::pm_flags;

    fn frame(hand: &mut WeaponHandState, facts: &WeaponCombatFacts, pm_flags: u32, perks0: u32) {
        let mut cmd = crate::WeaponCmd {
            pm_flags,
            perks0,
            cmd_weapon: hand.weapon as u16,
            cmd_weapon_owned: true,
            ..crate::WeaponCmd::default()
        };
        crate::pm_weapon::pm_weapon(hand, facts, &mut cmd);
    }

    fn sprinting_hand() -> (WeaponHandState, WeaponCombatFacts) {
        let facts = WeaponCombatFacts {
            sprint_raise_time_ms: 200,
            sprint_drop_time_ms: 250,
            ..WeaponCombatFacts::default()
        };
        let mut hand = WeaponHandState {
            weapon: 1,
            ..WeaponHandState::default()
        };
        frame(&mut hand, &facts, pm_flags::SPRINTING, 0);
        hand.weapon_time = 0;
        frame(&mut hand, &facts, pm_flags::SPRINTING, 0);
        assert_eq!(hand.weaponstate, WeaponState::SprintLoop as i32);
        (hand, facts)
    }

    fn anim(hand: &WeaponHandState) -> u32 {
        hand.weap_anim as u32 & crate::WEAP_ANIM_EVENT_MASK
    }

    #[test]
    fn an_empty_gun_sprints_with_the_empty_animations() {
        let (mut hand, facts) = sprinting_hand();
        assert_eq!(anim(&hand), weap_anim_event::SPRINT_LOOP_EMPTY);
        frame(&mut hand, &facts, 0, 0);
        assert_eq!(anim(&hand), weap_anim_event::SPRINT_OUT_EMPTY);
        let mut full = WeaponHandState {
            weapon: 1,
            clip: 30,
            ..WeaponHandState::default()
        };
        frame(&mut full, &facts, pm_flags::SPRINTING, 0);
        assert_eq!(anim(&full), weap_anim_event::SPRINT_IN);
    }

    #[test]
    fn stopping_a_sprint_still_lowers_the_gun_out() {
        let (mut hand, facts) = sprinting_hand();
        frame(&mut hand, &facts, 0, 0);
        assert_eq!(hand.weaponstate, WeaponState::SprintOut as i32);
        assert_eq!(hand.weapon_time, 250);
    }

    #[test]
    fn sprint_recovery_brings_the_gun_up_in_30_percent() {
        let (mut hand, facts) = sprinting_hand();
        frame(&mut hand, &facts, 0, PERK_SPRINTRECOVERY);
        assert_eq!(hand.weaponstate, WeaponState::SprintOut as i32);
        assert_eq!(hand.weapon_time, 75);
        let (mut hand, facts) = sprinting_hand();
        frame(&mut hand, &facts, 0, 0);
        assert_eq!(hand.weapon_time, 250);
    }
}
