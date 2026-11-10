//! The Ghosts (IW6 MP) map-script host, built without the sim.
//!
//! The source of truth is `crates/sim/src/t6/iw6_script/` (`host.rs`, `builtins.rs`, `boot.rs`,
//! `events.rs`, `census.rs`, `tests.rs`): those files only use their siblings (`super::host`,
//! `super::builtins`, ...) and `gsc_iw6` / `gsc_iw6_vm`, so this crate compiles the very same files
//! at its own root. The sim links a 160 MB test binary and the whole renderer stack; here the host,
//! its tests and the `iw6_script_census` example build in seconds. Nothing is duplicated by hand.

#[path = "../../sim/src/t6/iw6_script/anim.rs"]
pub mod anim;

#[path = "../../sim/src/t6/iw6_script/host.rs"]
pub mod host;

#[path = "../../sim/src/t6/iw6_script/player.rs"]
pub mod player;

#[path = "../../sim/src/t6/iw6_script/builtins.rs"]
pub mod builtins;

#[path = "../../sim/src/t6/iw6_script/boot.rs"]
pub mod boot;

#[path = "../../sim/src/t6/iw6_script/events.rs"]
pub mod events;

#[path = "../../sim/src/t6/iw6_script/census.rs"]
pub mod census;

#[cfg(test)]
#[path = "../../sim/src/t6/iw6_script/tests.rs"]
mod tests;
