//! bo2zm: a player dives on a flat floor with Black Ops II's rules, and the
//! dive keeps BO2's times (air time, the hang at the top, the slide, and a
//! get-up as long as one from prone).

use playerstate_iw4::{PlayerState, UserCmd, buttons, pm_flags};
use trace_iw4::Trace;

use crate::{
    AdsFracContext, AdsIntentContext, AirMoveContext, Bo2Feel, CmdScaleWalkContext,
    CollisionBackend, FlatMantleAnimLength, GroundTraceInput, JumpLaunchContext,
    MeleeChargeWeaponDelays, MoveBounds, PMF_DIVE, PMF_DIVE_GETUP, PMF_DIVE_PRONE, PMF_DIVE_SLIDE,
    PmoveSingleContext, SprintContext, ViewAngleClamp, WalkMoveContext, ZeroMantleRootDelta, pmove,
};

/// A floor at z = 0 and nothing else.
struct Floor;

impl CollisionBackend for Floor {
    fn trace(&self, input: GroundTraceInput) -> Trace {
        let start = input.start[2] + input.mins[2];
        let end = input.end[2] + input.mins[2];
        let mut hit = Trace {
            fraction: 1.0,
            endpos: input.end,
            ..Trace::default()
        };
        if start < 0.0 {
            hit.startsolid = 1;
            hit.allsolid = u8::from(end < 0.0);
            hit.fraction = 0.0;
            hit.endpos = input.start;
            hit.normal = [0.0, 0.0, 1.0];
            return hit;
        }
        if end < 0.0 {
            let fraction = start / (start - end);
            hit.fraction = fraction;
            hit.normal = [0.0, 0.0, 1.0];
            hit.walkable = 1;
            hit.hit_type = 1;
            hit.hit_id = trace_iw4::ENTITYNUM_WORLD;
            for i in 0..3 {
                hit.endpos[i] = input.start[i] + (input.end[i] - input.start[i]) * fraction;
            }
        }
        hit
    }
}

/// The multiplayer rules with dive to prone on.
const FEEL: Bo2Feel = Bo2Feel {
    dive: true,
    ..Bo2Feel::DEFAULTS
};

/// The same numbers the game's own `pmove_context` passes, for a plain gun.
fn context(old_buttons: u32) -> PmoveSingleContext {
    let air = AirMoveContext {
        player_spectate_speed_scale: 1.0,
        shellshock_gravity_scale: 1.0,
        shellshock_gravity_bias: 0.0,
    };
    PmoveSingleContext {
        walk: WalkMoveContext {
            cmd_scale: CmdScaleWalkContext {
                player_back_speed_scale: 0.7,
                player_strafe_speed_scale: 0.8,
                player_sprint_speed_scale: 1.5,
                player_last_stand_crawl_speed_scale: 0.15,
                weapon_move_speed_scale: 1.0,
                weapon_ads_move_speed_scale: 1.0,
                shellshock_movement_scale: 1.0,
                prone_lerp_ms: crate::PRONE_LERP_MS,
            },
            weapon_move_scale: 1.0,
            old_buttons,
            jump: JumpLaunchContext {
                jump_height: 39.0,
                dive: false,
                crouch_jump_scale: 1.0,
                jump_ladder_push_vel: FEEL.ladder_push_vel,
            },
            air,
            feel: FEEL,
        },
        air,
        bounds: MoveBounds {
            mins: [-15.0, -15.0, 0.0],
            maxs: [15.0, 15.0, 70.0],
            tracemask: 0x0281_0011,
        },
        view_angles: ViewAngleClamp {
            pitch_up: 85.0,
            pitch_down: 85.0,
            unclamped_pitch_bit: false,
        },
        sprint: SprintContext {
            weapon_max_sprint_time: 4000,
            sprint_forever: false,
            min_sprint_time_seconds: 1.0,
            sprint_delay_seconds: 0.0,
            sprint_forward_minimum: 105,
            stand_up_clear: true,
            sprint_recharge_pause_seconds: 0.0,
        },
        ads_intent: AdsIntentContext {
            ads_allowed: false,
            weapon_def_scope: false,
            sprint_hold_ads: false,
        },
        ads_frac: AdsFracContext::default(),
        melee_charge: MeleeChargeWeaponDelays {
            melee_delay_ms: 0,
            melee_charge_delay_ms: 0,
        },
        player_melee_range: crate::MELEE_CHARGE_PLAYER_MELEE_RANGE_DEFAULT,
        old_buttons,
        old_move: [0, 0],
        weapon_blocks_prone: false,
    }
}

struct Run {
    ps: PlayerState,
    time: i32,
    old_buttons: u32,
    old_move: [i8; 2],
}

impl Run {
    fn new() -> Self {
        let mut ps = PlayerState::ZERO;
        ps.gravity = 800;
        ps.speed = 190;
        ps.move_speed_scale_multiplier = 1.0;
        ps.ground_entity_num = 1022;
        ps.view_height_target = 60;
        ps.view_height_current = 60.0;
        ps.command_time = 1000;
        Self {
            ps,
            time: 1000,
            old_buttons: 0,
            old_move: [0, 0],
        }
    }

    /// Runs `ms` of 50 ms frames holding these inputs.
    fn hold(&mut self, ms: i32, forward: i8, right: i8, held: u32) {
        for _ in 0..ms / 50 {
            self.time += 50;
            let mut cmd = UserCmd {
                server_time: self.time,
                forwardmove: forward,
                rightmove: right,
                buttons: held,
                ..UserCmd::default()
            };
            let mut ctx = context(self.old_buttons);
            ctx.old_move = self.old_move;
            pmove(
                &mut self.ps,
                &mut cmd,
                ctx,
                &Floor,
                &FlatMantleAnimLength::default(),
                &ZeroMantleRootDelta,
            );
            self.old_buttons = held;
            self.old_move = [forward, right];
        }
    }

    fn flag(&self, flag: u32) -> bool {
        self.ps.pm_flags & flag != 0
    }

    fn ground_speed(&self) -> f32 {
        (self.ps.velocity[0].powi(2) + self.ps.velocity[1].powi(2)).sqrt()
    }
}

const SPRINT: u32 = buttons::SPRINT;
const JUMP: u32 = buttons::JUMP;
const PRONE: u32 = buttons::PRONE | buttons::STANCE_HELD;

/// Sprints 1 s, dives (a tap of the hold-style prone key), and runs until
/// the dive leaves the air. The time in the air (ms) and the highest point.
fn dive(run: &mut Run) -> (i32, f32) {
    run.hold(1000, 127, 0, SPRINT);
    run.hold(50, 127, 0, SPRINT | PRONE);
    assert!(run.flag(PMF_DIVE), "dived");
    let start = run.time - 50;
    let mut top = run.ps.origin[2];
    while run.flag(PMF_DIVE) {
        run.hold(50, 0, 0, SPRINT);
        top = top.max(run.ps.origin[2]);
        assert!(run.time - start < 3000, "the dive came down");
    }
    (run.time - start, top)
}

/// Runs until the view is at standing height and still.
fn ms_to_stand(run: &mut Run, held: u32) -> i32 {
    let start = run.time;
    while run.ps.view_height_current != 60.0 || run.ps.view_height_lerp_time != 0 {
        run.hold(50, 0, 0, held);
        assert!(run.time - start < 3000, "stood up");
    }
    run.time - start
}

#[test]
fn a_dive_rises_to_jump_height_and_lands_with_the_animation() {
    let mut run = Run::new();
    let (air_ms, top) = dive(&mut run);
    // pb_dive_prone: 0.733 s from take-off to touch-down.
    assert!((700..=800).contains(&air_ms), "air time {air_ms}");
    // The rise stops at a jump's height, past it by up to a 50 ms frame's
    // climb.
    assert!((39.0..=64.0).contains(&top), "top {top}");
    assert!(run.flag(PMF_DIVE_SLIDE));
    assert!(run.flag(pm_flags::PRONE));
    assert_eq!(run.ps.view_height_target, 11, "the view drops in the air");
}

#[test]
fn a_dive_hangs_at_the_top_for_0_4_s() {
    let mut run = Run::new();
    run.hold(1000, 127, 0, SPRINT);
    run.hold(50, 127, 0, SPRINT | PRONE);
    let take_off = run.time;
    let mut top = 0.0_f32;
    while run.time - take_off < 350 {
        run.hold(50, 0, 0, SPRINT);
        top = top.max(run.ps.origin[2]);
    }
    let at_350 = run.ps.origin[2];
    assert!(at_350 >= 39.0, "still at the top at 0.35 s: {at_350}");
    assert!(top - at_350 < 10.0, "hung, not fell: {top} to {at_350}");
    run.hold(50, 0, 0, SPRINT);
    assert!(run.ps.origin[2] < at_350 - 2.0, "falls at 0.4 s");
}

#[test]
fn a_dive_slides_300_ms_then_holds_prone() {
    let mut run = Run::new();
    dive(&mut run);
    let landed = run.time;
    while run.flag(PMF_DIVE_SLIDE) {
        assert_eq!(run.ps.pm_time, 0, "no 1.8 s prone lock");
        run.hold(50, 0, 0, SPRINT);
    }
    assert_eq!(run.time - landed, 300, "dtp_max_slide_duration");
    assert!(run.flag(PMF_DIVE_PRONE));
    assert_eq!(run.ground_speed(), 0.0);
    run.hold(500, 0, 0, 0);
    assert!(run.flag(pm_flags::PRONE), "stays prone");
}

#[test]
fn getting_up_from_a_dive_takes_as_long_as_from_prone() {
    let mut dived = Run::new();
    dive(&mut dived);
    while dived.flag(PMF_DIVE_SLIDE) || dived.ps.view_height_lerp_time != 0 {
        dived.hold(50, 0, 0, 0);
    }
    dived.hold(50, 0, 0, JUMP);
    let from_dive = 50 + ms_to_stand(&mut dived, 0);

    let mut prone = Run::new();
    prone.hold(2500, 0, 0, PRONE);
    assert_eq!(prone.ps.view_height_current, 11.0);
    let from_prone = ms_to_stand(&mut prone, 0);
    // BO2 has no quicker get-up after a dive: both take the prone time.
    // Each waits a 50 ms test frame to start, and the dive's count also
    // holds the jump press.
    assert_eq!(
        from_dive - 50 - 100,
        from_prone - 100,
        "{from_dive} vs {from_prone}"
    );
    dived.hold(50, 0, 0, 0);
    assert!(!dived.flag(PMF_DIVE_GETUP), "get-up over");
}

#[test]
fn a_get_up_pressed_mid_dive_is_kept() {
    let mut run = Run::new();
    run.hold(1000, 127, 0, SPRINT);
    run.hold(50, 127, 0, SPRINT | PRONE);
    run.hold(100, 0, 0, 0);
    run.hold(50, 0, 0, JUMP);
    assert!(run.flag(PMF_DIVE), "still in the air");
    while run.flag(PMF_DIVE) || run.flag(PMF_DIVE_SLIDE) {
        run.hold(50, 0, 0, 0);
    }
    assert!(run.flag(PMF_DIVE_PRONE));
    assert!(run.flag(PMF_DIVE_GETUP), "the get-up is kept");
    run.hold(100, 0, 0, 0);
    assert!(!run.flag(PMF_DIVE_PRONE), "getting up");
    ms_to_stand(&mut run, 0);
    assert_eq!(run.ps.pm_flags & 3, 0);
}
