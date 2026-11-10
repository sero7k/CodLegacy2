//! D3D11 shader bytecode (DXBC, Shader Model 4/5): the container, the
//! token stream as instructions, and WGSL emission for vertex and pixel
//! shaders. It knows nothing about any game; the caller decides what goes
//! in each constant buffer, texture and sampler slot.
#![no_std]
#![forbid(unsafe_code)]

extern crate alloc;

pub mod container;
pub mod decode;
pub mod wgsl;

pub use container::{Container, ContainerError, SigElement};
pub use decode::{DecodeError, Instr, Operand, Program, Stage, decode};
pub use wgsl::{
    BindingPlan, CbufferRef, Options, Reflection, SamplerRef, TextureDim, TextureKind, TextureRef,
    TranslateError, Translation, VaryingMap, VaryingRef, link, translate, translate_program,
};
