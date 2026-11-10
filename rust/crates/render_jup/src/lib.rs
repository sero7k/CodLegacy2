//! Modern Warfare III (2023) and Modern Warfare II (2022) maps drawn the way
//! the game draws them, on the same footing as `render_t7`:
//!
//! - [`library`]: the game's programs (`zone/code_post_gfx.ff`, DXIL) found,
//!   lowered to WGSL by `dxil_sm6::wgsl::translate` and linked stage to stage
//!   by semantic.
//! - [`records`]: the material records the programs read (a material's
//!   `GPU_PIPELINE_ID` is a byte offset into one raw buffer; its picture
//!   indices sit at +516..+660) and the bindless picture table.
//! - [`mapping`]: which programs a material draws with. Not decoded in the
//!   install yet: [`mapping::choose`] answers `None` until it is, and the
//!   surface then takes the fallback shader (`fallback.wgsl`), logged.
//! - [`scene`]: the map zone, the zones beside it and its streamed pictures
//!   read into plain data (on a thread of the caller's).
//! - [`host`]: the frame: shadow, G-buffer, lighting, display; the finished
//!   image and the scene depth for the host's frame ([`present`]).
//! - [`coverage`]: the pipeline list of a map, with what is made and what
//!   is missing (`jup native:` lines).
//!
//! See `docs/JUP.md` ("Native renderer").
#![allow(clippy::too_many_arguments)]

pub mod coverage;
pub mod game_view;
pub mod host;
pub mod layers;
pub mod library;
pub mod mapping;
pub mod records;
pub mod scene;
pub mod shaders;
pub mod sky;
pub mod skylight;
pub mod terrain;

/// The hand-off of the finished frame into the host's frame (the same
/// pass `render_t7` uses: the image, and the scene depth in the host's
/// depth band).
pub use render_t7::present;
