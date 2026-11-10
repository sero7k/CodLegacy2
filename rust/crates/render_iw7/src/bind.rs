//! What a pass binds where, from its argument table (`ShaderArgRec`).
//!
//! Read on `techsets_mp_riot` (checked by `iw7_bind_check` against every
//! pass's translated programs):
//! - arguments come per primitive, then per object, then stable
//!   (`PassRec::arg_counts`); a code constant fills buffer `b{group}` (b0 per
//!   primitive, b1 per object, b2 stable) and a material constant `b3`, from
//!   register `dest`. On the lightmap passes: vertex b0 and b1 are 4
//!   registers (one matrix each: code constants 320, per primitive, and 304,
//!   per object), vertex b2 18 (stable), pixel b2 21 (stable code
//!   constants), pixel b3 2 to 7 (the material's). `PassRec::cbuffer_slots`
//!   reads 0, 0, 26 on every pass: not slots;
//! - argument types: 0 code constant, 1 code texture, 2 code sampler,
//!   3 material constant, 5 material texture, 6 material sampler;
//! - a texture or sampler argument's `dest` is its `t#` / `s#` slot;
//! - stage bits: 2 vertex, 16 pixel.

use fastfile_iw7::walk::{PassRec, ShaderArgRec};

pub const ARG_CODE_CONST: u8 = 0;
pub const ARG_CODE_TEXTURE: u8 = 1;
pub const ARG_CODE_SAMPLER: u8 = 2;
pub const ARG_MATERIAL_CONST: u8 = 3;
pub const ARG_MATERIAL_TEXTURE: u8 = 5;
pub const ARG_MATERIAL_SAMPLER: u8 = 6;

/// The material's own constants' buffer.
pub const MATERIAL_BUFFER: u8 = 3;

pub const STAGE_VS: u8 = 0x02;
pub const STAGE_PS: u8 = 0x10;

/// Where one argument goes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Target {
    /// Constant buffer `slot`, from register `register`.
    Constant { slot: u8, register: u16 },
    Texture { slot: u16 },
    Sampler { slot: u16 },
}

/// The argument's update group: 0 per primitive, 1 per object, 2 stable.
pub fn group(pass: &PassRec, k: usize) -> usize {
    let [a, b, _] = pass.arg_counts.map(usize::from);
    if k < a {
        0
    } else if k < a + b {
        1
    } else {
        2
    }
}

/// Where argument `k` of `pass` goes.
pub fn target(pass: &PassRec, k: usize) -> Option<Target> {
    let arg: &ShaderArgRec = pass.args.get(k)?;
    Some(match arg.ty {
        ARG_CODE_CONST => Target::Constant { slot: group(pass, k) as u8, register: arg.dest },
        ARG_MATERIAL_CONST => Target::Constant { slot: MATERIAL_BUFFER, register: arg.dest },
        ARG_CODE_TEXTURE | ARG_MATERIAL_TEXTURE => Target::Texture { slot: arg.dest },
        ARG_CODE_SAMPLER | ARG_MATERIAL_SAMPLER => Target::Sampler { slot: arg.dest },
        _ => return None,
    })
}

/// Rows a constant argument writes (a code constant's `row_count`, a
/// material constant one row).
pub fn rows(arg: &ShaderArgRec) -> u16 {
    match arg.ty {
        ARG_CODE_CONST => u16::from(arg.row_count.max(1)),
        _ => 1,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arguments_take_their_groups_buffer() {
        let arg = |ty: u8, dest: u16| ShaderArgRec { ty, stage: STAGE_PS, dest, row_count: 1, ..Default::default() };
        let pass = PassRec {
            arg_counts: [1, 1, 2],
            cbuffer_slots: [0, 0, 26],
            args: vec![arg(ARG_CODE_CONST, 4), arg(ARG_CODE_CONST, 0), arg(ARG_MATERIAL_TEXTURE, 7), arg(ARG_CODE_SAMPLER, 3), arg(ARG_MATERIAL_CONST, 2)],
            ..Default::default()
        };
        // Per primitive -> b0, per object -> b1, a material constant -> b3.
        assert_eq!(target(&pass, 0), Some(Target::Constant { slot: 0, register: 4 }));
        assert_eq!(target(&pass, 1), Some(Target::Constant { slot: 1, register: 0 }));
        assert_eq!(target(&pass, 4), Some(Target::Constant { slot: 3, register: 2 }));
        assert_eq!(target(&pass, 2), Some(Target::Texture { slot: 7 }));
        assert_eq!(target(&pass, 3), Some(Target::Sampler { slot: 3 }));
        assert_eq!(target(&pass, 9), None);
    }
}
