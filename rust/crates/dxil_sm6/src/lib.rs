//! DXIL, the shader bytecode of Shader Model 6: LLVM 3.7 bitcode in a `DXIL`
//! container part. This crate reads the bitstream ([`bitstream`]) and the
//! module in it ([`ir`]: types, globals, constants, metadata, function
//! bodies) and prints it as text ([`text`]). It knows nothing about any game.
#![no_std]
#![forbid(unsafe_code)]

extern crate alloc;

pub mod analysis;
pub mod bitstream;
pub mod ir;
pub mod meta;
pub mod text;
pub mod wgsl;

pub use ir::{Module, read, read_part};
