//! bo2mp: Call of Duty: Ghosts' player movement numbers, as data only.
//!
//! Black Ops II movement stays authoritative: nothing in the game reads
//! this table. `iw6_movement_requested` hands it out only when
//! `BO2MP_IW6_MOVEMENT_TABLE=1` asks (for comparisons and tools). The
//! numbers, how each was identified and how sure it is are in
//! `docs/exe_notes/IW6_MOVEMENT.md`. Units: inches, seconds, milliseconds
//! where named `_ms`; scales are multipliers of the player's speed.

/// Ghosts' movement numbers (multiplayer).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Iw6Movement {
    /// Gravity, units/s^2 (the player state's gravity at spawn).
    pub gravity: f32,
    /// Base run speed, units/s (the player state's speed every frame).
    pub speed: f32,
    /// Backpedal scale on the forward input.
    pub back_speed_scale: f32,
    /// Strafe scale on the side input.
    pub strafe_speed_scale: f32,
    /// Sprint speed scale.
    pub sprint_speed_scale: f32,
    /// Side input scale while sprinting.
    pub sprint_strafe_scale: f32,
    /// Forward input a sprint needs (of 127), strictly greater.
    pub sprint_forward_min: i8,
    /// Full sprint, ms, times the weapon's sprint duration scale.
    pub sprint_time_ms: f32,
    /// Sprint left a new sprint needs, ms (strictly more).
    pub sprint_min_ms: f32,
    /// Pause before the sprint meter refills, ms.
    pub sprint_recharge_pause_ms: f32,
    /// Meter refill: ms of sprint regained per ms not sprinting.
    pub sprint_refill_rate: f32,
    /// Longest sprint meter, ms.
    pub sprint_time_cap_ms: i32,
    /// Marathon-class perk scale on the sprint time.
    pub perk_sprint_multiplier: f32,
    /// Stance scales: crouched, prone.
    pub crouch_speed_scale: f32,
    pub prone_speed_scale: f32,
    /// Walking (and the aim-down-sight class) speed scale.
    pub walk_speed_scale: f32,
    /// Ground acceleration: standing, crouched, prone; in the air.
    pub accel_stand: f32,
    pub accel_crouch: f32,
    pub accel_prone: f32,
    pub accel_air: f32,
    /// Friction and its stop speed.
    pub friction: f32,
    pub stop_speed: f32,
    /// Jump height, units; least time between jumps, ms.
    pub jump_height: f32,
    pub jump_min_interval_ms: i32,
    /// Jump slowdown after a landing: scale 1 + 1.5 * t / 1700 ms,
    /// 2.5 from 1700 ms, cleared at 1800 ms.
    pub jump_slowdown_window_ms: i32,
    pub jump_slowdown_max: f32,
    /// Step height standing / in the air.
    pub step_height: f32,
    pub step_height_air: f32,
    /// Fall damage from / to these heights.
    pub fall_damage_min_height: f32,
    pub fall_damage_max_height: f32,
    /// Slide: start speed scale, slide length (times the script move speed
    /// scale), forward speed needed, ms after a sprint starts, ms between
    /// slides.
    pub slide_speed_scale: f32,
    pub slide_length: f32,
    pub slide_min_speed: f32,
    pub slide_after_sprint_ms: i32,
    pub slide_cooldown_ms: i32,
    /// Mantle: reach in front (grounded / airborne), behind, and the most
    /// the view may face away from the ledge, degrees.
    pub mantle_reach: f32,
    pub mantle_reach_air: f32,
    pub mantle_back: f32,
    pub mantle_max_angle: f32,
    /// Mantle animation heights, highest first.
    pub mantle_heights: [f32; 7],
}

/// Ghosts' numbers (see the doc for each one's evidence).
pub const IW6_MOVEMENT: Iw6Movement = Iw6Movement {
    gravity: 800.0,
    speed: 190.0,
    back_speed_scale: 0.7,
    strafe_speed_scale: 0.8,
    sprint_speed_scale: 1.4,
    sprint_strafe_scale: 0.667,
    sprint_forward_min: 105,
    sprint_time_ms: 4000.0,
    sprint_min_ms: 1000.0,
    sprint_recharge_pause_ms: 0.0,
    sprint_refill_rate: 1.0,
    sprint_time_cap_ms: 0x3fff,
    perk_sprint_multiplier: 2.0,
    crouch_speed_scale: 0.65,
    prone_speed_scale: 0.15,
    walk_speed_scale: 0.4,
    accel_stand: 9.0,
    accel_crouch: 12.0,
    accel_prone: 19.0,
    accel_air: 1.0,
    friction: 5.5,
    stop_speed: 100.0,
    jump_height: 39.0,
    jump_min_interval_ms: 500,
    jump_slowdown_window_ms: 1700,
    jump_slowdown_max: 2.5,
    step_height: 18.0,
    step_height_air: 10.0,
    fall_damage_min_height: 128.0,
    fall_damage_max_height: 300.0,
    slide_speed_scale: 1.1,
    slide_length: 60.0,
    slide_min_speed: 150.0,
    slide_after_sprint_ms: 200,
    slide_cooldown_ms: 1200,
    mantle_reach: 54.9,
    mantle_reach_air: 78.9,
    mantle_back: 14.9,
    mantle_max_angle: 60.0,
    mantle_heights: [64.0, 56.0, 48.0, 40.0, 32.0, 24.0, 16.0],
};

/// The Ghosts table, only when `BO2MP_IW6_MOVEMENT_TABLE=1` asks for it.
pub fn iw6_movement_requested() -> Option<&'static Iw6Movement> {
    let on = std::env::var("BO2MP_IW6_MOVEMENT_TABLE")
        .is_ok_and(|v| matches!(v.trim(), "1" | "on" | "true" | "yes"));
    on.then_some(&IW6_MOVEMENT)
}

/// Ghosts' sprint meter at full, ms, for a weapon's sprint duration scale.
pub fn iw6_sprint_time_ms(m: &Iw6Movement, weapon_duration_scale: f32, marathon: bool) -> i32 {
    let mut t = weapon_duration_scale * m.sprint_time_ms;
    if marathon {
        t *= m.perk_sprint_multiplier;
    }
    (t as i32).min(m.sprint_time_cap_ms)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ghosts_jump_reaches_its_height_under_its_gravity() {
        let m = IW6_MOVEMENT;
        let v = (2.0 * m.gravity * m.jump_height).sqrt();
        assert!((v * v / (2.0 * m.gravity) - 39.0).abs() < 1e-3);
    }

    #[test]
    fn ghosts_sprint_meter_scales_and_caps() {
        let m = IW6_MOVEMENT;
        assert_eq!(iw6_sprint_time_ms(&m, 1.0, false), 4000);
        assert_eq!(iw6_sprint_time_ms(&m, 1.0, true), 8000);
        assert_eq!(iw6_sprint_time_ms(&m, 5.0, true), 0x3fff);
    }

    #[test]
    fn the_table_is_off_unless_asked() {
        if std::env::var_os("BO2MP_IW6_MOVEMENT_TABLE").is_none() {
            assert!(iw6_movement_requested().is_none());
        }
    }
}
