//! bo2zm: two of Black Ops II's Advanced settings rows: Draw FPS (the frame
//! rate in the top right corner, as `cg_drawFPS "Simple"` shows it) and Max
//! Frames Per Second (`com_maxfps`: a frame cap, only without sync every
//! frame, as BO2 greys it out with sync on).

use std::time::{Duration, Instant};

use bevy::prelude::*;

/// The frame cap: each frame waits out the rest of its share of a second
/// (a sleep, then a short spin for the last millisecond).
fn frame_cap(settings: Option<Res<frame::GameSettings>>, mut last: Local<Option<Instant>>) {
    sim::client_frame_heartbeat();
    let cap = settings
        .as_deref()
        .filter(|s| !s.vsync && s.max_fps > 0)
        .map(|s| s.max_fps);
    if let (Some(cap), Some(prev)) = (cap, *last) {
        let share = Duration::from_secs_f64(1.0 / f64::from(cap));
        let spent = prev.elapsed();
        if spent < share {
            let wait = share - spent;
            if wait > Duration::from_millis(2) {
                std::thread::sleep(wait - Duration::from_millis(1));
            }
            while prev.elapsed() < share {
                std::hint::spin_loop();
            }
        }
    }
    *last = Some(Instant::now());
}

pub(crate) fn register_frame_rate_systems(app: &mut App) {
    // The counter itself (Off / Simple / Full) lives in `telemetry.rs`.
    crate::telemetry::register_telemetry_systems(app);
    app.add_systems(Last, frame_cap.after(crate::telemetry::record_cpu_end));
}
