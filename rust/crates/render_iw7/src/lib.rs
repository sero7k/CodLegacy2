//! Infinite Warfare's maps drawn the way IW7 draws them, on `render_h1`'s
//! pattern: each surface with its material's own technique, the technique's
//! programs translated from D3D11 bytecode (`dxbc_sm5`), its constant
//! buffers, textures and samplers filled by the pass's own argument table.
//!
//! IW7's exe is not read (retail is protected), so the code constants have
//! no names from it: [`code`] names an index only where the data proves what
//! it is, and lists the rest.

pub mod bind;
pub mod world;
pub mod state;
pub mod vertex;
pub mod post;
pub mod screen;
pub mod lights;
pub mod volumetric;
pub mod spot;
pub mod behind;
pub mod reflect;
pub mod profile;
pub mod cull;
pub mod blur;
pub mod brdf;
pub mod spec_occlusion;
pub mod dof;
