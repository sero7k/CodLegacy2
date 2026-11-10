//! Call of Duty: Ghosts' maps drawn the way Ghosts draws them: each
//! surface with its material's own technique, the technique's shaders
//! translated from D3D11 bytecode (`dxbc_sm5`), its constant buffers
//! filled by the pass's own argument table, and the engine's values
//! (camera, sun, fog, light grid, probes) supplied as code constants;
//! then Ghosts' own post techniques (fog, tone map and grade) with the
//! engine's luminance and iris between them.

pub mod bake;
pub mod bind;
pub mod caster;
pub mod cells;
pub mod decal;
pub mod distort;
pub mod code;
pub mod fx_light;
pub mod host;
pub mod instancing;
pub mod layers;
pub mod layout;
pub mod local_light;
pub mod look;
pub mod model_lighting;
pub mod names;
pub mod pool;
pub mod post;
pub mod profile;
pub mod scene;
pub mod scope;
pub mod skin;
pub mod sky;
pub mod spot_shadow;
pub mod dof;
pub mod mblur;
pub mod ssao;
pub mod sun_fx;
pub mod state;
pub mod sun_cache;
pub mod sun_shadow;
pub mod loadtime;
pub mod pipecache;
pub mod switch;
pub mod world;
