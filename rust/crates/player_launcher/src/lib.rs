//! The CodLegacy launcher's logic, without any window: settings, the account-service
//! client, the build manifest and updater, game-folder lookup, and starting the game.
//! `main.rs` is the window on top of this.

pub mod api;
pub mod error;
pub mod game;
pub mod manifest;
pub mod selfupdate;
pub mod settings;
pub mod signing;
pub mod update;

pub use error::{Error, Fault};
