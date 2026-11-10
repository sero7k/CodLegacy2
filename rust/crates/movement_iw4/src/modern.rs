//! bo2mp: Modern Warfare III (2023)'s movement, for the Warzone playlists
//! only (`Bo2Feel::modern`). Everything else keeps Black Ops II's.
//!
//! On top of Black Ops II's walk, air move, jump and dive:
//! - sprint never runs out; sprint pressed again while sprinting starts a
//!   tactical sprint (faster, the gun down) until its time is spent, then
//!   it recharges;
//! - crouch pressed while sprinting on the ground slides: a burst of speed
//!   that bleeds off, steered a little. Jump ends it in a jump, letting go
//!   of crouch ends it standing and sprinting again; both refill the
//!   tactical sprint (the slide cancel);
//! - a climb onto any ledge up to [`Mw3Tuning::mantle_max_height`] (no
//!   mantle brushes needed: Modern Warfare III's maps have none), a vault
//!   over a low, thin obstacle or through a window;
//! - swimming in the map's sea (`Bo2Feel::water_z`): no gravity, the view's
//!   pitch steers under water, jump rises and crouch sinks, the gun down;
//! - its stance, strafe and backpedal speeds and its fall heights.
//!
//! There is no Modern Warfare III engine to port from: the numbers are in
//! [`MW3_2023`], each noted with where it comes from; the `fastfile_jup`
//! example `jup_movement` prints what the install's own settings hold, to
//! replace them.
//!
//! State rides in the replicated player state (`mw_*`), so the host and
//! the local prediction agree.

use playerstate_iw4::{ENTITYNUM_NONE, PlayerState, UserCmd, buttons, pm_flags};

use crate::stance::{CROUCH_MAXS_Z, STAND_MAXS_Z, view_height};
use crate::{
    CollisionBackend, GroundTraceInput, MoveBounds, Pml, PmoveSingleContext, add_predictable_event,
    end_sprint,
};

/// The landing event of surface type `water` (0x70 + 20), and the walk
/// footstep event; surface type 20 is `water`.
const EV_LANDING_WATER: i32 = 0x84;
const EV_FOOTSTEP_WALK: i32 = 0x6d;
const WATER_SURFACE: i32 = 20;

/// Tactical sprint is on.
pub const MW_TAC: u32 = 1 << 0;
/// Sliding.
pub const MW_SLIDE: u32 = 1 << 1;
/// Climbing (`mw_from` to `mw_to`).
pub const MW_MANTLE: u32 = 1 << 2;
/// The climb is a vault: the player goes on over the far side.
pub const MW_VAULT: u32 = 1 << 3;
/// Swimming.
pub const MW_SWIM: u32 = 1 << 4;
/// The eye is under the water.
pub const MW_UNDER: u32 = 1 << 5;
/// A slide just ended standing: the sprint starts again this frame.
pub const MW_RESUME_SPRINT: u32 = 1 << 6;

/// Black Ops II's "forced to stand" event: the client's crouch toggle lets
/// go with it.
const EV_STANCE_FORCE_STAND: i32 = 6;

/// Modern Warfare III (2023)'s movement numbers. Speeds are scales of
/// Black Ops II's (`g_speed` 190 and its own stance scales), so a gun's
/// move speed scale still applies.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Mw3Tuning {
    pub sprint_scale: f32,
    pub tac_sprint_scale: f32,
    pub tac_sprint_ms: i32,
    pub tac_sprint_min_ms: i32,
    pub tac_recharge_ms: i32,
    pub back_scale: f32,
    pub strafe_scale: f32,
    pub sprint_strafe_scale: f32,
    pub crouch_ratio: f32,
    pub prone_ratio: f32,
    pub jump_height: f32,
    pub jump_slowdown: bool,
    pub fall_damage_min_height: f32,
    pub fall_damage_max_height: f32,
    pub slide_min_speed: f32,
    pub slide_start_speed: f32,
    pub slide_boost: f32,
    pub slide_ms: i32,
    pub slide_friction: f32,
    pub slide_end_speed: f32,
    pub slide_steer_deg: f32,
    pub mantle_max_height: f32,
    pub mantle_reach: f32,
    pub mantle_ms_base: i32,
    pub mantle_ms_per_unit: f32,
    pub vault_max_height: f32,
    pub vault_max_depth: f32,
    pub vault_exit_speed: f32,
    pub swim_depth: f32,
    pub swim_exit_depth: f32,
    pub swim_float_depth: f32,
    pub swim_max_depth: f32,
    pub swim_scale: f32,
    pub swim_sprint_scale: f32,
    pub swim_accel: f32,
    pub swim_drag: f32,
    pub swim_entry_fall_speed: f32,
    pub wade_depth: f32,
    pub wade_scale: f32,
}

/// The numbers in use. "MW3 2023" = its known behaviour as played (no
/// engine read); "estimated" = picked to match the feel, to replace with
/// the install's settings (the `fastfile_jup` example `jup_movement`).
pub const MW3_2023: Mw3Tuning = Mw3Tuning {
    // MW3 2023: sprint is unlimited. Speed estimated (Black Ops II's 1.5).
    sprint_scale: 1.5,
    // Estimated: tactical sprint about a fifth faster than sprint.
    tac_sprint_scale: 1.8,
    // Estimated: about 3 s of tactical sprint, at least 0.3 s to start one,
    // a full refill in 2.5 s (MW3 2023 refills it on a slide cancel).
    tac_sprint_ms: 3000,
    tac_sprint_min_ms: 300,
    tac_recharge_ms: 2500,
    // Estimated: backpedal and strafe nearer full speed than Black Ops II's
    // 0.7 / 0.8, and sprint strafing freer than its 0.667.
    back_scale: 0.8,
    strafe_scale: 0.9,
    sprint_strafe_scale: 0.8,
    // Estimated: crouched a little faster, prone a little slower than Black
    // Ops II's stance speeds.
    crouch_ratio: 1.1,
    prone_ratio: 0.9,
    // Estimated: Black Ops II's 39 units.
    jump_height: 39.0,
    // MW3 2023: no repeated-jump slowdown.
    jump_slowdown: false,
    // Estimated: falls hurt from higher up than Black Ops II's 128 / 300.
    fall_damage_min_height: 160.0,
    fall_damage_max_height: 400.0,
    // MW3 2023: a slide from a sprint. Speeds and times estimated.
    slide_min_speed: 150.0,
    slide_start_speed: 330.0,
    slide_boost: 1.15,
    slide_ms: 800,
    slide_friction: 260.0,
    slide_end_speed: 110.0,
    slide_steer_deg: 60.0,
    // MW3 2023: any ledge to about the top of a reach; heights estimated.
    mantle_max_height: 112.0,
    mantle_reach: 24.0,
    mantle_ms_base: 300,
    mantle_ms_per_unit: 4.0,
    // MW3 2023: vaults over low walls and through windows; estimated.
    vault_max_height: 56.0,
    vault_max_depth: 40.0,
    vault_exit_speed: 160.0,
    // Estimated: swims from chest-deep water, floats with the eye above the
    // surface, dives no deeper than this under it.
    swim_depth: 46.0,
    swim_exit_depth: 38.0,
    swim_float_depth: 52.0,
    swim_max_depth: 160.0,
    swim_scale: 0.6,
    swim_sprint_scale: 0.85,
    swim_accel: 4.0,
    swim_drag: 2.0,
    swim_entry_fall_speed: 100.0,
    // Estimated: knee-deep water slows a little.
    wade_depth: 18.0,
    wade_scale: 0.85,
};

fn pressed(cmd: &UserCmd, old_buttons: u32, button: u32) -> bool {
    cmd.buttons & button != 0 && old_buttons & button == 0
}

fn released(cmd: &UserCmd, old_buttons: u32, button: u32) -> bool {
    cmd.buttons & button == 0 && old_buttons & button != 0
}

fn on_ground(ps: &PlayerState) -> bool {
    ps.ground_entity_num != ENTITYNUM_NONE
}

fn speed_xy(ps: &PlayerState) -> f32 {
    libm::sqrtf(ps.velocity[0] * ps.velocity[0] + ps.velocity[1] * ps.velocity[1])
}

fn water_depth(ps: &PlayerState, water_z: f32) -> f32 {
    if water_z.is_finite() {
        water_z - ps.origin[2]
    } else {
        f32::NEG_INFINITY
    }
}

/// This frame's rules as Modern Warfare III's: unlimited sprint, its sprint
/// and tactical sprint, strafe, backpedal and stance speeds, its jump.
pub fn adjust_context(context: &mut PmoveSingleContext, ps: &PlayerState, t: &Mw3Tuning) {
    context.sprint.sprint_forever = true;
    let scale = &mut context.walk.cmd_scale;
    scale.player_back_speed_scale = t.back_scale;
    scale.player_strafe_speed_scale = t.strafe_scale;
    scale.player_sprint_speed_scale = if ps.mw_flags & MW_TAC != 0 {
        t.tac_sprint_scale
    } else {
        t.sprint_scale
    };
    let mut factor = if ps.pm_flags & pm_flags::PRONE != 0 {
        t.prone_ratio
    } else if ps.pm_flags & pm_flags::CROUCH != 0 {
        t.crouch_ratio
    } else {
        1.0
    };
    let depth = water_depth(ps, context.walk.feel.water_z);
    if depth > t.wade_depth {
        factor *= t.wade_scale;
    }
    scale.weapon_move_speed_scale *= factor;
    scale.weapon_ads_move_speed_scale *= factor;
    context.walk.jump.jump_height = t.jump_height;
    context.walk.feel.sprint_strafe_speed_scale = t.sprint_strafe_scale;
}

/// Tactical sprint, before Black Ops II's sprint update: sprint pressed
/// again while sprinting starts it; it spends its time, then refills.
/// `mw_tac` is the time spent (0: full).
pub fn tactical_sprint(
    ps: &mut PlayerState,
    cmd: &UserCmd,
    old_buttons: u32,
    msec: i32,
    t: &Mw3Tuning,
) {
    if ps.mw_flags & MW_TAC != 0 {
        ps.mw_tac += msec;
        if ps.mw_tac >= t.tac_sprint_ms {
            ps.mw_tac = t.tac_sprint_ms;
            ps.mw_flags &= !MW_TAC;
        }
        return;
    }
    if ps.pm_flags & pm_flags::SPRINTING != 0
        && pressed(cmd, old_buttons, buttons::SPRINT)
        && t.tac_sprint_ms - ps.mw_tac >= t.tac_sprint_min_ms
    {
        ps.mw_flags |= MW_TAC;
        return;
    }
    if ps.mw_tac > 0 {
        let refill = (msec * t.tac_sprint_ms) / t.tac_recharge_ms.max(1);
        ps.mw_tac = (ps.mw_tac - refill.max(1)).max(0);
    }
}

/// After the sprint update: no sprint, no tactical sprint.
pub fn after_sprint(ps: &mut PlayerState) {
    if ps.pm_flags & pm_flags::SPRINTING == 0 {
        ps.mw_flags &= !MW_TAC;
    }
}

/// The command the sprint update sees: no sprint while sliding; a slide
/// cancelled standing sprints again at once.
#[must_use]
pub fn sprint_cmd(ps: &mut PlayerState, cmd: &UserCmd, old_buttons: u32) -> (UserCmd, u32) {
    let mut out = *cmd;
    let mut old = old_buttons;
    if ps.mw_flags & MW_SLIDE != 0 {
        out.buttons &= !buttons::SPRINT;
    } else if ps.mw_flags & MW_RESUME_SPRINT != 0 {
        ps.mw_flags &= !MW_RESUME_SPRINT;
        out.buttons |= buttons::SPRINT;
        old &= !buttons::SPRINT;
        ps.sprint_button_up_required = 0;
        ps.sprint_delay = 0;
    }
    (out, old)
}

/// A slide starts: crouch pressed while sprinting on the ground, fast
/// enough. Before the sprint and stance updates.
pub fn slide_start(ps: &mut PlayerState, cmd: &UserCmd, old_buttons: u32, t: &Mw3Tuning) -> bool {
    if ps.mw_flags & (MW_SLIDE | MW_SWIM | MW_MANTLE) != 0
        || ps.pm_flags & pm_flags::SPRINTING == 0
        || !on_ground(ps)
        || !pressed(cmd, old_buttons, buttons::CROUCH)
    {
        return false;
    }
    let speed = speed_xy(ps);
    if speed < t.slide_min_speed {
        return false;
    }
    end_sprint(ps, cmd);
    ps.sprint_button_up_required = 0;
    ps.mw_flags = (ps.mw_flags & !MW_TAC) | MW_SLIDE;
    ps.mw_timer = 0;
    ps.pm_flags = (ps.pm_flags & !pm_flags::PRONE) | pm_flags::CROUCH;
    let to = (speed * t.slide_boost).max(t.slide_start_speed);
    let k = to / speed;
    ps.velocity[0] *= k;
    ps.velocity[1] *= k;
    true
}

/// How a slide ended this frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SlideCancel {
    None,
    /// Into a jump (the jump runs this frame).
    Jump,
    /// Standing, sprinting again.
    Stand,
}

/// The slide cancel: jump, or letting go of crouch, ends the slide and
/// refills the tactical sprint. Before the stance update.
pub fn slide_cancel<C: CollisionBackend>(
    ps: &mut PlayerState,
    cmd: &UserCmd,
    old_buttons: u32,
    collision: &C,
    bounds: MoveBounds,
) -> SlideCancel {
    if ps.mw_flags & MW_SLIDE == 0 {
        return SlideCancel::None;
    }
    if pressed(cmd, old_buttons, buttons::JUMP) {
        ps.mw_flags &= !MW_SLIDE;
        ps.mw_tac = 0;
        if room_to_stand(ps, collision, bounds) {
            ps.pm_flags &= !(pm_flags::CROUCH | pm_flags::PRONE);
            ps.view_height_target = view_height::STAND;
            add_predictable_event(ps, EV_STANCE_FORCE_STAND, 0);
        }
        return SlideCancel::Jump;
    }
    if released(cmd, old_buttons, buttons::CROUCH) {
        ps.mw_flags = (ps.mw_flags & !MW_SLIDE) | MW_RESUME_SPRINT;
        ps.mw_tac = 0;
        return SlideCancel::Stand;
    }
    SlideCancel::None
}

fn room_to_stand<C: CollisionBackend>(ps: &PlayerState, collision: &C, bounds: MoveBounds) -> bool {
    let mut maxs = bounds.maxs;
    maxs[2] = STAND_MAXS_Z;
    collision
        .trace(GroundTraceInput {
            start: ps.origin,
            end: ps.origin,
            mins: bounds.mins,
            maxs,
            tracemask: bounds.tracemask,
        })
        .allsolid
        == 0
}

/// The slide's move, in place of the walk: the burst bleeds off, the stick
/// steers it a little; it ends after its time or when it is too slow.
pub fn slide_move<C: CollisionBackend>(
    ps: &mut PlayerState,
    pml: &Pml,
    cmd: &UserCmd,
    bounds: MoveBounds,
    collision: &C,
    t: &Mw3Tuning,
) -> f32 {
    ps.mw_timer += pml.msec;
    let mut speed = speed_xy(ps);
    if speed > 0.0 {
        let steer =
            (f32::from(cmd.rightmove) / 127.0) * t.slide_steer_deg.to_radians() * pml.frametime;
        let (s, c) = (libm::sinf(-steer), libm::cosf(-steer));
        let (x, y) = (ps.velocity[0], ps.velocity[1]);
        ps.velocity[0] = x * c - y * s;
        ps.velocity[1] = x * s + y * c;
        let to = (speed - t.slide_friction * pml.frametime).max(0.0);
        let k = to / speed;
        ps.velocity[0] *= k;
        ps.velocity[1] *= k;
        speed = to;
    }
    let normal = [
        f32::from_bits(pml.ground_trace[1]),
        f32::from_bits(pml.ground_trace[2]),
        f32::from_bits(pml.ground_trace[3]),
    ];
    if pml.ground_plane != 0 {
        crate::project_velocity(&mut ps.velocity, &normal);
    }
    if ps.mw_timer >= t.slide_ms || speed < t.slide_end_speed {
        ps.mw_flags &= !MW_SLIDE;
    }
    crate::step_slide_move(
        ps,
        pml,
        collision,
        bounds.mins,
        bounds.maxs,
        bounds.tracemask,
        None,
        false,
    )
}

/// A slide stops in the air (off a ledge: the air move carries it on).
pub fn slide_airborne(ps: &mut PlayerState) {
    if !on_ground(ps) {
        ps.mw_flags &= !MW_SLIDE;
    }
}

fn trace<C: CollisionBackend>(
    collision: &C,
    start: [f32; 3],
    end: [f32; 3],
    mins: [f32; 3],
    maxs: [f32; 3],
    tracemask: u32,
) -> trace_iw4::Trace {
    collision.trace(GroundTraceInput {
        start,
        end,
        mins,
        maxs,
        tracemask,
    })
}

fn climb_ms(ps: &PlayerState, t: &Mw3Tuning) -> i32 {
    let height = (ps.mw_to[2] - ps.mw_from[2]).max(0.0);
    t.mantle_ms_base + (height * t.mantle_ms_per_unit) as i32
}

/// A climb onto the ledge in front: jump pressed (or held in the air)
/// moving forward into a wall with a walkable top within reach and room
/// to stand (or crouch) on it. Low, thin tops are vaulted.
pub fn mantle_check<C: CollisionBackend>(
    ps: &mut PlayerState,
    cmd: &UserCmd,
    old_buttons: u32,
    pml: &Pml,
    bounds: MoveBounds,
    collision: &C,
    t: &Mw3Tuning,
) -> bool {
    if ps.mw_flags & (MW_MANTLE | MW_SWIM) != 0 || cmd.forwardmove <= 0 {
        return false;
    }
    let jump_now = pressed(cmd, old_buttons, buttons::JUMP);
    let jump_held = cmd.buttons & buttons::JUMP != 0;
    if !(jump_now || (jump_held && !on_ground(ps))) {
        return false;
    }
    let mut fwd = [pml.forward[0], pml.forward[1], 0.0];
    let len = libm::sqrtf(fwd[0] * fwd[0] + fwd[1] * fwd[1]);
    if len < 0.01 {
        return false;
    }
    fwd[0] /= len;
    fwd[1] /= len;
    let mask = bounds.tracemask;
    let o = ps.origin;
    let r = bounds.maxs[0];
    // A wall in front, at the waist.
    let probe_min = [-r * 0.5, -r * 0.5, 0.0];
    let probe_max = [r * 0.5, r * 0.5, 4.0];
    let waist = [o[0], o[1], o[2] + 24.0];
    let ahead = [
        waist[0] + fwd[0] * (t.mantle_reach + r),
        waist[1] + fwd[1] * (t.mantle_reach + r),
        waist[2],
    ];
    let wall = trace(collision, waist, ahead, probe_min, probe_max, mask);
    if wall.fraction >= 1.0 || wall.startsolid != 0 || wall.normal[2].abs() > 0.3 {
        return false;
    }
    let into = (t.mantle_reach + r) * wall.fraction + r + 2.0;
    // The ledge's top: straight down in front of the wall, from as high as
    // a climb goes.
    let top = o[2] + t.mantle_max_height + 2.0;
    let above = [o[0] + fwd[0] * into, o[1] + fwd[1] * into, top];
    let below = [above[0], above[1], o[2] + 18.0];
    let crouch = [bounds.maxs[0], bounds.maxs[1], CROUCH_MAXS_Z];
    let ledge = trace(collision, above, below, bounds.mins, crouch, mask);
    if ledge.startsolid != 0 || ledge.fraction >= 1.0 || ledge.normal[2] < 0.7 {
        return false;
    }
    let end = [ledge.endpos[0], ledge.endpos[1], ledge.endpos[2] + 0.25];
    let height = end[2] - o[2];
    if height <= 18.0 {
        return false;
    }
    // The way up, then across.
    let up = [o[0], o[1], end[2]];
    let crouch_hull = [bounds.maxs[0], bounds.maxs[1], CROUCH_MAXS_Z];
    if trace(collision, o, up, bounds.mins, crouch_hull, mask).fraction < 1.0
        || trace(collision, up, end, bounds.mins, crouch_hull, mask).fraction < 1.0
    {
        return false;
    }
    let stand_hull = [bounds.maxs[0], bounds.maxs[1], STAND_MAXS_Z];
    let stands = trace(collision, end, end, bounds.mins, stand_hull, mask).allsolid == 0;
    // A vault: a low top with open ground a little further on, lower down.
    let mut to = end;
    let mut vault = false;
    if height <= t.vault_max_height {
        let far = [
            end[0] + fwd[0] * t.vault_max_depth,
            end[1] + fwd[1] * t.vault_max_depth,
            end[2],
        ];
        let across = trace(collision, end, far, bounds.mins, crouch_hull, mask);
        if across.fraction >= 1.0 {
            let drop = trace(
                collision,
                far,
                [far[0], far[1], end[2] - 24.0],
                bounds.mins,
                crouch_hull,
                mask,
            );
            if drop.fraction >= 1.0 {
                to = far;
                vault = true;
            }
        }
    }
    end_sprint(ps, cmd);
    ps.mw_flags = (ps.mw_flags & !(MW_TAC | MW_SLIDE)) | MW_MANTLE;
    if vault {
        ps.mw_flags |= MW_VAULT;
    }
    ps.mw_from = o;
    ps.mw_to = to;
    ps.mw_timer = 0;
    ps.velocity = [0.0; 3];
    ps.pm_flags |= pm_flags::MANTLE;
    // The body plays Black Ops II's climb nearest the height (and its climb
    // over for a vault); the gun is down for the climb.
    ps.mantle_trans_index = crate::mantle::find_transition(0.0, height);
    ps.mantle_flags = playerstate_iw4::mantle_flags::ACTIVE
        | if vault { playerstate_iw4::mantle_flags::OVER } else { 0 };
    ps.mantle_timer = 0;
    if !stands && !vault {
        ps.pm_flags = (ps.pm_flags & !pm_flags::PRONE) | pm_flags::CROUCH;
    }
    ps.ground_entity_num = ENTITYNUM_NONE;
    true
}

/// The climb under way: up to the ledge's height, then across onto it.
pub fn mantle_advance(ps: &mut PlayerState, msec: i32, t: &Mw3Tuning) {
    ps.mw_timer += msec;
    ps.mantle_timer = ps.mw_timer;
    let total = climb_ms(ps, t).max(1);
    let f = (ps.mw_timer as f32 / total as f32).clamp(0.0, 1.0);
    let rise = 0.65;
    let (from, to) = (ps.mw_from, ps.mw_to);
    if f < rise {
        let k = f / rise;
        let k = k * k * (3.0 - 2.0 * k);
        ps.origin = [from[0], from[1], from[2] + (to[2] - from[2]) * k];
    } else {
        let k = (f - rise) / (1.0 - rise);
        ps.origin = [
            from[0] + (to[0] - from[0]) * k,
            from[1] + (to[1] - from[1]) * k,
            to[2],
        ];
    }
    if f >= 1.0 {
        ps.origin = to;
        ps.pm_flags &= !pm_flags::MANTLE;
        ps.velocity = [0.0; 3];
        if ps.mw_flags & MW_VAULT != 0 {
            let d = [to[0] - from[0], to[1] - from[1]];
            let len = libm::sqrtf(d[0] * d[0] + d[1] * d[1]).max(0.01);
            ps.velocity = [
                d[0] / len * t.vault_exit_speed,
                d[1] / len * t.vault_exit_speed,
                0.0,
            ];
        }
        ps.mw_flags &= !(MW_MANTLE | MW_VAULT);
        ps.mw_timer = 0;
        ps.mantle_flags = 0;
        ps.mantle_timer = 0;
    }
}

/// Into or out of the water; true while swimming.
pub fn update_swim(ps: &mut PlayerState, water_z: f32, t: &Mw3Tuning) -> bool {
    let depth = water_depth(ps, water_z);
    let swimming = ps.mw_flags & MW_SWIM != 0;
    let now = if swimming {
        depth > t.swim_exit_depth
    } else {
        depth > t.swim_depth
    };
    if now && !swimming {
        ps.mw_flags = (ps.mw_flags & !(MW_TAC | MW_SLIDE)) | MW_SWIM;
        ps.velocity[2] = ps.velocity[2].max(-t.swim_entry_fall_speed);
        ps.pm_flags |= pm_flags::SPRINTING;
        // The splash: the water landing, louder the faster the way in.
        add_predictable_event(ps, EV_LANDING_WATER, (-ps.velocity[2]).clamp(0.0, 255.0) as i32);
    } else if !now && swimming {
        ps.mw_flags &= !(MW_SWIM | MW_UNDER);
        ps.pm_flags &= !pm_flags::SPRINTING;
        // Out of the water: a wet step (surface type `water`).
        add_predictable_event(ps, EV_FOOTSTEP_WALK, WATER_SURFACE);
    }
    if now {
        let eye = ps.origin[2] + ps.view_height_current;
        if eye < water_z - 2.0 {
            ps.mw_flags |= MW_UNDER;
        } else {
            ps.mw_flags &= !MW_UNDER;
        }
    }
    now
}

/// The swim: standing height, no gravity; on the surface the body floats
/// and moves flat, under it the view's pitch steers; jump rises, crouch
/// sinks; sprint swims faster on the surface.
#[allow(clippy::too_many_arguments)]
pub fn swim_move<C: CollisionBackend>(
    ps: &mut PlayerState,
    pml: &Pml,
    cmd: &UserCmd,
    g_speed: f32,
    water_z: f32,
    bounds: MoveBounds,
    collision: &C,
    t: &Mw3Tuning,
) -> f32 {
    ps.pm_flags &= !(pm_flags::CROUCH | pm_flags::PRONE);
    ps.view_height_target = view_height::STAND;
    ps.ground_entity_num = ENTITYNUM_NONE;
    let under = ps.mw_flags & MW_UNDER != 0;
    let (f, r) = (f32::from(cmd.forwardmove), f32::from(cmd.rightmove));
    let mut wish: [f32; 3] = core::array::from_fn(|k| pml.forward[k] * f + pml.right[k] * r);
    let diving = pml.forward[2] < -0.5;
    if !under && !diving {
        wish[2] = 0.0;
    }
    if cmd.buttons & buttons::JUMP != 0 {
        wish[2] += 127.0;
    }
    if cmd.buttons & buttons::CROUCH != 0 {
        wish[2] -= 127.0;
    }
    let len = libm::sqrtf(wish[0] * wish[0] + wish[1] * wish[1] + wish[2] * wish[2]);
    let fast = !under && cmd.buttons & buttons::SPRINT != 0;
    let top = g_speed
        * if fast {
            t.swim_sprint_scale
        } else {
            t.swim_scale
        };
    let speed = (len / 127.0).min(1.0) * top;
    let dir = if len > 0.0 {
        [wish[0] / len, wish[1] / len, wish[2] / len]
    } else {
        [0.0; 3]
    };
    let dt = pml.frametime;
    // Water holds the body back (drag) and the stroke pulls it toward the
    // wished speed (accel).
    let drag = (1.0 - t.swim_drag * dt).max(0.0);
    let accel = (t.swim_accel * dt).min(1.0);
    for (v, d) in ps.velocity.iter_mut().zip(dir) {
        let held = *v * drag;
        *v = held + (d * speed - held) * accel;
    }
    // Floating: drawn toward the float height when not pushed down.
    let float_z = water_z - t.swim_float_depth;
    if cmd.buttons & buttons::CROUCH == 0 && !diving && ps.origin[2] < float_z {
        ps.velocity[2] = ps.velocity[2].max((float_z - ps.origin[2]) * 4.0);
    }
    if ps.origin[2] > float_z && ps.velocity[2] > 0.0 {
        ps.velocity[2] = ((float_z - ps.origin[2]) * 4.0)
            .max(0.0)
            .min(ps.velocity[2]);
    }
    let floor = water_z - t.swim_max_depth;
    if ps.origin[2] <= floor && ps.velocity[2] < 0.0 {
        ps.velocity[2] = 0.0;
    }
    let mut maxs = bounds.maxs;
    maxs[2] = STAND_MAXS_Z;
    crate::slide_move(
        ps,
        pml,
        collision,
        bounds.mins,
        maxs,
        bounds.tracemask,
        None,
    );
    if ps.origin[2] < floor {
        ps.origin[2] = floor;
    }
    0.0
}
