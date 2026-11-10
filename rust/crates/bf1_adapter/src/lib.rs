//! bo2mp: the Battlefield 1 family (`bf1:<level>`, e.g. `bf1:mp_verdun`).
//!
//! The readers are the vendored `fb_*` crates (Unlicense, from
//! AaronSparks/Battlefield-1-Rust-Rewrite; see `NOTICE`, `docs/BF1.md`). They
//! read the player's own install (`Data` + `Patch`) at runtime. Nothing here
//! touches the filesystem until a BF1 folder is configured.
//!
//! What this crate does today: find the install ([`config`]), list the maps
//! ([`catalog`]) and open one to report what it holds ([`report`]). It does
//! not draw or simulate a BF1 map: that is the rendering port in docs/BF1.md.

pub mod catalog;
pub mod config;
pub mod report;

/// The family's prefix in map specs (`bf1:mp_verdun`) and its key in
/// `game_paths.cfg`.
pub const FAMILY: &str = "bf1";

/// The menu section and weapon tab title.
pub const SECTION_TITLE: &str = "BATTLEFIELD 1";

/// The message a `bf1:` map request fails with, and the one-line summary of
/// why: the map is listed, the install may be fine, but no BF1 world is
/// drawn or played yet.
pub const NOT_PLAYABLE: &str = "Battlefield 1 maps cannot be played yet: the reader works (see the log for the level report) but the renderer and gameplay port are not built. See docs/BF1.md.";
