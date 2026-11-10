//! Black Ops III maps drawn the way BlackOps3.exe draws them: the map's
//! own technique programs (DXBC translated to WGSL by `dxbc_sm5`), bound
//! with the exe's state maps (`state`), sampler states (`sampler`), input
//! layouts (`layout`), code constants (`code`) and render targets
//! (`gbuffer`). Read from the zones and `.xpak` packages (`scene`); what is
//! missing is reported (`t7 surfaces:` lines); a missing picture is stood in
//! for only where that cannot turn a cut-out or blended surface into a
//! solid card (`surf_debug::stand_in`).

pub mod ao;
pub mod audit;
mod bake;
pub mod bo2_shared;
pub mod code;
pub mod completeness;
pub mod compute;
pub mod coverage;
pub mod cull;
pub mod debug_views;
pub mod forward;
pub mod fx;
pub mod fx_check;
pub mod fx_rand;
pub mod gbuffer;
pub mod gpu;
pub mod gpu_skin;
pub mod host;
pub mod layout;
pub mod lighting;
pub mod lighting_globals;
pub mod lights;
pub mod lod;
pub mod memo;
pub mod motion;
pub mod par;
pub mod pipecache;
pub mod probes;
pub mod profile;
pub mod quality;
pub mod post;
pub mod post_aa;
pub mod post_ads;
pub mod post_bloom;
pub mod post_mblur;
pub mod post_exposure;
pub mod post_lut;
pub mod post_oob;
pub mod post_pass;
pub mod post_run;
pub mod prewarm;
pub mod present;
pub mod probe;
pub mod rdef;
pub mod sampler;
pub mod scene;
pub mod siege;
pub mod state;
pub mod spot_shadow;
pub mod sun_shadow;
pub mod surf_debug;
pub mod timing;
pub mod volumetric;
pub mod volume_decals;
pub mod volumetric_pass;
pub mod warm;
pub mod weather;
pub mod winding;
pub mod zone_index;

/// bo2mp: the folder of the shared zones for a map zone (BO3's `zone`
/// folder for a Steam Workshop map or a usermap).
pub use fastfile_t7::map::shared_dir_for;

/// bo2mp: the map's baked pictures read ahead as its load begins (see `bake`).
pub use bake::read_ahead as read_ahead_pictures;
pub use bake::{ahead_cap as bake_ahead_cap, bake_ahead, bake_ahead_of};
