mod createfx;
// bo2mp: how Ghosts plays its aliases (Ghosts maps and sounds).
pub mod iw6_rules;
mod ent_channel;
mod map_script_sound;
mod sound_catalog;
mod sound_load;
mod sound_load_iw5;
// bo2mp: Ghosts' weapon sounds (its guest guns).
mod sound_load_iw6;
mod sound_load_s1;
// bo2mp: Horizon MW's weapon sounds (its guest guns).
mod sound_load_h1;
// bo2mp: where Infinite Warfare's guns are (its guest guns).
#[cfg(feature = "iw7")]
mod iw7_zones;
// bo2mp: Infinite Warfare's weapon sounds (its guest guns).
#[cfg(feature = "iw7")]
mod sound_load_iw7;
// bo2mp: Black Ops III's weapon sounds (its guest guns).
mod sound_load_t7;
// bo2mp: Modern Warfare III's (2023) sounds (Resurgence).
#[cfg(feature = "jup")]
mod sound_load_jup;
// bo2mp: World at War's weapon sounds (its guest guns).
mod sound_load_t4;
mod sound_load_t5;
// bo2zm: Black Ops II sound banks.
mod sound_load_t6;
mod sound_wma_t5;
mod wma_profile;
mod wma_t5;
pub use wma_profile::T5WmaProfile;
mod zone_sound;

pub use asset_core::*;
pub use asset_transport::*;
pub use createfx::*;
pub use ent_channel::*;
pub use map_script_sound::*;
pub use sound_catalog::*;
pub use sound_load::*;
pub use sound_load_iw5::*;
pub use sound_load_iw6::*;
pub use sound_load_s1::*;
pub use sound_load_h1::*;
#[cfg(feature = "iw7")]
pub use iw7_zones::*;
#[cfg(feature = "iw7")]
pub use sound_load_iw7::*;
pub use sound_load_t7::*;
#[cfg(feature = "jup")]
pub use sound_load_jup::*;
// bo2mp: stand-ins for the games left out of the build.
mod dropped;
#[cfg(not(feature = "iw7"))]
pub use dropped::iw7::*;
#[cfg(not(feature = "jup"))]
pub use dropped::jup::*;
pub use sound_load_t4::*;
pub use sound_load_t5::*;
pub use sound_load_t6::*;
pub use sound_wma_t5::*;
pub use zone_sound::*;

pub mod asset_graph {
    pub use asset_core::*;
}
pub mod discover {
    pub use asset_transport::*;
}
pub mod zone {
    pub use asset_transport::*;
}
