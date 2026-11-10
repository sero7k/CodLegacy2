//! One log line per boot step: how long it took, when it ended (seconds
//! since launch) and the memory in use then. A slow launch shows where its
//! time went without a profiler.

use bevy::prelude::*;
use std::time::Instant;

pub(crate) struct BootSteps {
    last: Instant,
}

impl BootSteps {
    pub(crate) fn start() -> Self {
        Self { last: Instant::now() }
    }

    /// "boot: <name> in 1.2 s (at 3.4 s, 512 MB in use)".
    pub(crate) fn step(&mut self, name: &str) {
        let took = self.last.elapsed().as_secs_f64();
        self.last = Instant::now();
        diag::info!(Launch, "boot: {name} in {took:.1} s ({})", at_now());
    }
}

fn at_now() -> String {
    let at = diag::process_elapsed_ns() as f64 / 1e9;
    match asset_transport::process_resident_bytes() {
        Some(bytes) => format!("at {at:.1} s, {} MB in use", bytes >> 20),
        None => format!("at {at:.1} s"),
    }
}

/// "boot: <name> (at 3.4 s, 512 MB in use)" for the waits inside `app.run`
/// before the first frame; each one also beats the hang sampler, so a stall
/// it names falls between two of these lines.
fn mark(name: &str) {
    diag::info!(Launch, "boot: {name} ({})", at_now());
    diag::hang::beat();
}

/// Added before every other plugin: its `finish` is the first one run.
pub(crate) struct BootMarksFirst;

impl Plugin for BootMarksFirst {
    fn build(&self, _app: &mut App) {}

    fn finish(&self, _app: &mut App) {
        mark("plugins ready, finishing them");
    }
}

/// Added after every other plugin. Bevy asks the plugins whether they are
/// ready in the order they were added and stops at the first that is not,
/// so this one is first asked once the ones before it (the renderer's device
/// among them) are up.
pub(crate) struct BootMarksLast {
    asked: std::sync::atomic::AtomicBool,
}

impl Default for BootMarksLast {
    fn default() -> Self {
        Self {
            asked: std::sync::atomic::AtomicBool::new(false),
        }
    }
}

impl Plugin for BootMarksLast {
    fn build(&self, app: &mut App) {
        app.add_systems(PreStartup, || mark("startup systems begin"))
            .add_systems(PostStartup, || mark("startup systems done"))
            .add_systems(First, first_frame_mark("first frame begins"))
            .add_systems(PreUpdate, first_frame_mark("first frame's PreUpdate"))
            .add_systems(Last, first_frame_mark("first frame's schedules done"));
    }

    fn ready(&self, _app: &App) -> bool {
        if !self.asked.swap(true, std::sync::atomic::Ordering::Relaxed) {
            mark("renderer and other plugins ready");
        }
        true
    }

    fn finish(&self, _app: &mut App) {
        mark("plugins finished");
    }

    fn cleanup(&self, app: &mut App) {
        // Last, so every plugin's startup systems are in.
        if std::env::var("CODLEGACY_STARTUP_LAPS").map_or(true, |v| v != "0") {
            startup_laps(app, PreStartup, "PreStartup");
            startup_laps(app, Startup, "Startup");
            startup_laps(app, PostStartup, "PostStartup");
            watch_startup();
        }
        mark("plugins cleaned up, opening the window");
    }
}

/// A startup system's lap is logged once it takes this long (s).
const LAP_LOGGED_S: f64 = 0.01;

/// Times `label`'s systems one by one: a cold boot once spent 273 s between
/// "startup systems begin" and "done" with nothing to say which system.
/// A schedule with run conditions keeps bevy's own runner (they are private
/// to it): it is timed as a whole only.
fn startup_laps(app: &mut App, label: impl bevy::ecs::schedule::ScheduleLabel, name: &'static str) {
    app.edit_schedule(label, |schedule| {
        let graph = schedule.graph();
        let guarded = graph.systems.iter().filter(|(_, _, c)| !c.is_empty()).count()
            + graph.system_sets.iter().filter(|(_, _, c)| !c.is_empty()).count();
        if guarded > 0 {
            diag::info!(Launch, "boot: {name} not timed per system ({guarded} run conditions)");
        } else {
            schedule.set_executor(StartupLaps {
                name,
                unapplied: Vec::new(),
                apply_final_deferred: true,
            });
        }
    });
}

/// Runs a startup schedule's systems one at a time on this thread, in the
/// order bevy sorted them, with a lap line for each slow one (its seconds and
/// the program's reads meanwhile) and a beat of the hang sampler before each,
/// so a stall's samples fall inside one system. Its systems have no run
/// conditions (`startup_laps` checks).
struct StartupLaps {
    name: &'static str,
    /// Systems run since their commands were last applied.
    unapplied: Vec<usize>,
    apply_final_deferred: bool,
}

impl StartupLaps {
    fn apply_deferred(&mut self, systems: &mut [bevy::ecs::schedule::SystemWithAccess], world: &mut World, laps: &mut Laps) {
        if self.unapplied.is_empty() {
            return;
        }
        let lap = laps.start(format!("{} commands", self.name));
        for i in self.unapplied.drain(..) {
            systems[i].apply_deferred(world);
        }
        laps.end(lap, self.name, "their commands applied", None);
    }
}

impl bevy::ecs::schedule::SystemExecutor for StartupLaps {
    fn init(&mut self, _schedule: &bevy::ecs::schedule::SystemSchedule) {}

    fn run(
        &mut self,
        schedule: &mut bevy::ecs::schedule::SystemSchedule,
        world: &mut World,
        _skip_systems: Option<&bevy::ecs::schedule::FixedBitSet>,
        error_handler: fn(bevy::ecs::error::BevyError, bevy::ecs::error::ErrorContext),
    ) {
        use bevy::ecs::system::{RunSystemError, System};
        // SAFETY: the systems are only run and asked for their buffers and
        // names (methods of `System`); none is replaced.
        let systems = unsafe { schedule.systems_mut() };
        let count = systems.len();
        let mut laps = Laps::new();
        for i in 0..count {
            if systems[i].system_type() == std::any::TypeId::of::<bevy::ecs::schedule::ApplyDeferred>() {
                self.apply_deferred(systems, world, &mut laps);
                continue;
            }
            let system = &mut systems[i];
            let named = system.name().to_string();
            let what = format!("system {} of {count}", i + 1);
            let lap = laps.start(format!("{} {what} ({named})", self.name));
            if let Err(RunSystemError::Failed(err)) = system.run_without_applying_deferred((), world) {
                error_handler(
                    err,
                    bevy::ecs::error::ErrorContext::System {
                        name: system.name(),
                        last_run: system.get_last_run(),
                    },
                );
            }
            // Bevy's debug feature names the system; without it, its number
            // and the hang sampler's stacks do.
            laps.end(lap, self.name, &what, (!named.starts_with('<')).then_some(named.as_str()));
            self.unapplied.push(i);
        }
        if self.apply_final_deferred {
            self.apply_deferred(systems, world, &mut laps);
        }
        diag::info!(
            Launch,
            "boot: {} ran {count} systems in {:.1} s (slowest: {} in {:.1} s)",
            self.name,
            laps.started.elapsed().as_secs_f64(),
            laps.slowest.0,
            laps.slowest.1
        );
        if self.name == "PostStartup" {
            WATCH_DONE.store(true, std::sync::atomic::Ordering::Relaxed);
        }
    }

    fn set_apply_final_deferred(&mut self, value: bool) {
        self.apply_final_deferred = value;
    }
}

/// One startup schedule's laps.
struct Laps {
    started: Instant,
    slowest: (String, f64),
}

impl Laps {
    fn new() -> Self {
        Self {
            started: Instant::now(),
            slowest: (String::from("none"), 0.0),
        }
    }

    /// `running` names the lap for the watch below.
    fn start(&self, running: String) -> (Instant, Option<(u64, u64)>) {
        diag::hang::beat();
        let from = Instant::now();
        *RUNNING.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = Some((running, from));
        (from, diag::process_reads())
    }

    /// "boot: Startup system 7 of 23 in 41.2 s, 1203 reads, 512.0 MB read (whole program)".
    fn end(&mut self, (from, reads): (Instant, Option<(u64, u64)>), schedule: &str, what: &str, named: Option<&str>) {
        *RUNNING.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = None;
        let took = from.elapsed().as_secs_f64();
        if took > self.slowest.1 {
            self.slowest = (what.to_string(), took);
        }
        if took < LAP_LOGGED_S {
            return;
        }
        let named = named.map(|n| format!(" ({n})")).unwrap_or_default();
        let read = match (reads, diag::process_reads()) {
            (Some((ops, bytes)), Some((ops_now, bytes_now))) => format!(
                ", {} reads, {:.1} MB read (whole program)",
                ops_now.saturating_sub(ops),
                bytes_now.saturating_sub(bytes) as f64 / 1_048_576.0
            ),
            _ => String::new(),
        };
        diag::info!(Launch, "boot: {schedule} {what}{named} in {took:.2} s{read} ({})", at_now());
    }
}

/// The startup lap running now, and since when.
static RUNNING: std::sync::Mutex<Option<(String, Instant)>> = std::sync::Mutex::new(None);
/// Set once PostStartup's laps are done: the watch ends.
static WATCH_DONE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
/// A lap is named by the watch once it runs this long, and again each time.
const WATCH_EVERY_S: u64 = 10;

/// Warm boots spent 296 s and 517 s in the startup systems with memory flat
/// (a wait, not work). Every 10 s that a startup system or its commands run
/// on, one line names it ("boot: Startup system 7 of 23 (...) still running
/// after 40 s"), so a wait that never ends is still named. It only logs:
/// the system carries on.
fn watch_startup() {
    let spawned = std::thread::Builder::new().name("startup watch".into()).spawn(|| {
        while !WATCH_DONE.load(std::sync::atomic::Ordering::Relaxed) {
            std::thread::sleep(std::time::Duration::from_secs(WATCH_EVERY_S));
            let running = RUNNING.lock().unwrap_or_else(std::sync::PoisonError::into_inner).clone();
            if let Some((what, from)) = running {
                let took = from.elapsed().as_secs();
                if took >= WATCH_EVERY_S {
                    diag::warn!(Launch, "boot: {what} still running after {took} s ({})", at_now());
                }
            }
        }
    });
    if let Err(e) = spawned {
        diag::warn!(Launch, "boot: no startup watch ({e})");
    }
}

/// A system that marks `name` on the first frame only.
fn first_frame_mark(name: &'static str) -> impl FnMut(Local<bool>) {
    move |mut done: Local<bool>| {
        if !*done {
            *done = true;
            mark(name);
        }
    }
}

/// The menu's first frame, and its memory once it has settled.
pub(crate) fn log_menu_up(mut frames: Local<u32>, mut settled: Local<Option<Instant>>) {
    *frames += 1;
    if *frames == 1 {
        diag::info!(Launch, "boot: menu up ({})", at_now());
        assets::t6_menu_up();
        *settled = Some(Instant::now());
    } else if settled.is_some_and(|t| t.elapsed().as_secs() >= 30) {
        diag::info!(Launch, "boot: menu after 30 s ({})", at_now());
        *settled = None;
    }
}
