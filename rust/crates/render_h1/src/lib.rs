//! Modern Warfare Remastered's maps drawn the way MWR draws them: each
//! surface with its material's own technique, the technique's shaders
//! translated from D3D11 bytecode (`dxbc_sm5`), its constant buffers
//! filled by the pass's own argument table, and the engine's values
//! (camera, sun, shadows, fog) supplied as code constants.

pub mod bind;
pub mod code;
pub mod gpu;
pub mod glass;
pub mod gpu_timer;
pub mod instancing;
pub mod host;
pub mod layers;
pub mod light;
pub mod local_light;
pub mod look;
pub mod meter;
pub mod model_lighting;
pub mod pool;
pub mod post;
pub mod present;
pub mod scene;
pub mod screenfx;
pub mod stage;
pub mod state;
pub mod sun_shadow;
pub mod tonemap;
pub mod veil;
pub mod world;
