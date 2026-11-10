//! bo2mp: `setslowmotion( start, end, seconds )`: the game's time scale,
//! ramped from `start` to `end` over `seconds` (Black Ops II's final killcam
//! slows to a quarter on the killing shot, then back). The killcam's replay
//! reads it and falls behind live time by what the slow motion takes; so do
//! the scripts' clock and waits (`script_lag_ms`), as BO2's game time slows
//! with its time scale: its final killcam waits out the quarter-speed hold in
//! game time, so the hold lands on the killing shot. With the waits in real
//! time the ramp back up began before the replay reached the kill, which then
//! played at nearly full speed.

use std::sync::Mutex;
use std::time::Instant;

struct Ramp {
    start: f32,
    end: f32,
    seconds: f32,
    at: Instant,
}

static RAMP: Mutex<Option<Ramp>> = Mutex::new(None);

/// How far the scripts' clock has fallen behind the server's (ms, as a
/// float so short ticks add up) and the server time it was last moved at.
static LAG: Mutex<(f64, Option<i64>)> = Mutex::new((0.0, None));

/// The scripts' clock at server time `now_ms`: behind it by what slow motion
/// has taken so far (each tick adds its length times one less the scale).
pub(super) fn script_time_ms(now_ms: i64) -> i64 {
    let scale = f64::from(scale().clamp(0.0, 1.0));
    let Ok(mut lag) = LAG.lock() else { return now_ms };
    if let Some(last) = lag.1 {
        let dt = (now_ms - last).max(0) as f64;
        lag.0 += (1.0 - scale) * dt;
    }
    lag.1 = Some(now_ms);
    now_ms - lag.0 as i64
}

pub(super) fn set(start: f32, end: f32, seconds: f32) {
    if let Ok(mut r) = RAMP.lock() {
        *r = Some(Ramp { start: start.clamp(0.05, 4.0), end: end.clamp(0.05, 4.0), seconds: seconds.max(0.0), at: Instant::now() });
    }
}

/// The time scale now (1 when nothing slowed it).
pub fn scale() -> f32 {
    let Ok(r) = RAMP.lock() else { return 1.0 };
    let Some(r) = r.as_ref() else { return 1.0 };
    if r.seconds <= 0.0 {
        return r.end;
    }
    let t = (r.at.elapsed().as_secs_f32() / r.seconds).min(1.0);
    r.start + (r.end - r.start) * t
}

/// Back to normal speed (a new match).
pub fn reset() {
    if let Ok(mut r) = RAMP.lock() {
        *r = None;
    }
    if let Ok(mut l) = LAG.lock() {
        *l = (0.0, None);
    }
}
