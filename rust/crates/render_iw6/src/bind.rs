//! What a pass binds where, from its argument table and the material.
//!
//! A pass's constant buffers are one per update rate: `b0` per primitive
//! (`perPrimArgSize`: the world matrix, the lighting coordinates), `b1`
//! per object (view-projection, primary light, shadow lookup), `b2`
//! stable (fog, eye offset, tone values) and `b3` the material's own
//! constants. One block of each rate is shared by the vertex and pixel
//! stages (a pass's `b1` holds the view-projection for the vertex shader
//! and the light for the pixel shader in the same buffer), so a write goes
//! to its slot whatever stage it names. Arguments come per primitive
//! first, then per object, then stable; `dest` is the 16-byte register a
//! constant starts at, or the `t#` / `s#` slot of a texture or sampler.
//! The engine binds the reflection probe (`t1`), the lightmaps (`t2`
//! primary, `t3` secondary) itself: no argument names them.

use std::collections::BTreeMap;

use fastfile_iw6::walk::MaterialConstant;

use crate::pool::Pass;

pub const PER_PRIM: u32 = 0;
pub const PER_OBJECT: u32 = 1;
pub const STABLE: u32 = 2;
pub const MATERIAL: u32 = 3;

/// `MaterialShaderArgument::type`.
pub mod arg {
    pub const CODE_CONST: u8 = 0;
    pub const CODE_TEXTURE: u8 = 1;
    pub const CODE_SAMPLER: u8 = 2;
    pub const MATERIAL_CONST: u8 = 3;
    pub const LITERAL_CONST: u8 = 4;
    pub const MATERIAL_TEXTURE: u8 = 5;
    pub const MATERIAL_SAMPLER: u8 = 6;
}

/// Shader stage bits of `MaterialShaderArgument::shader`.
pub const STAGE_VS: u8 = 0x02;
pub const STAGE_PS: u8 = 0x10;

#[derive(Clone, Debug, PartialEq)]
pub enum CbSource {
    /// Rows `first_row .. first_row + rows` of a code constant.
    Code {
        index: u16,
        first_row: u8,
        rows: u8,
    },
    Literal([f32; 4]),
    Material([f32; 4]),
    /// A material constant the material doesn't hold (its name hash).
    MissingMaterial(u32),
}

#[derive(Clone, Debug, PartialEq)]
pub struct CbWrite {
    pub buffer: u32,
    pub dest: u16,
    pub src: CbSource,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TexSource {
    /// The material's texture of this name hash: its image name (`None`
    /// when the material has no texture of the hash) and semantic.
    Material {
        hash: u32,
        image: Option<String>,
        semantic: u8,
    },
    /// A code texture (`TEXTURE_SRC_CODE_*` index).
    Code(u32),
    ReflectionProbe,
    LightmapPrimary,
    LightmapSecondary,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SampSource {
    /// A material sampler state byte (filter, mip, clamp bits).
    Material(u8),
    /// A code sampler (`TEXTURE_SRC_CODE_*` index).
    Code(u32),
    /// A sampler the shader declares that nothing binds.
    Default,
}

#[derive(Clone, Debug, Default)]
pub struct StageBinds {
    pub textures: BTreeMap<u32, TexSource>,
    pub samplers: BTreeMap<u32, SampSource>,
}

#[derive(Clone, Debug, Default)]
pub struct PassBinds {
    pub cbuffer: Vec<CbWrite>,
    pub vs: StageBinds,
    pub ps: StageBinds,
}

/// A material as the binder needs it.
pub struct MaterialView<'a> {
    /// (name hash, sampler state, semantic, image name)
    pub textures: &'a [(u32, u8, u8, String)],
    pub constants: &'a [MaterialConstant],
}

/// The engine-bound pixel slots: (slot, source).
pub const ENGINE_SLOTS: [(u32, TexSource); 3] = [
    (1, TexSource::ReflectionProbe),
    (2, TexSource::LightmapPrimary),
    (3, TexSource::LightmapSecondary),
];

pub fn resolve(pass: &Pass, mat: &MaterialView) -> PassBinds {
    let mut out = PassBinds::default();
    let counts = pass.arg_counts.map(usize::from);
    for (k, a) in pass.args.iter().enumerate() {
        let rate = if k < counts[0] {
            PER_PRIM
        } else if k < counts[0] + counts[1] {
            PER_OBJECT
        } else {
            STABLE
        };
        let mut stages: Vec<&mut StageBinds> = Vec::new();
        {
            let (vs, ps) = (&mut out.vs, &mut out.ps);
            if a.shader & STAGE_VS != 0 {
                stages.push(vs);
            }
            if a.shader & STAGE_PS != 0 {
                stages.push(ps);
            }
        }
        let hash = a.u32();
        let mut push = |buffer: u32, src: CbSource| {
            out.cbuffer.push(CbWrite {
                buffer,
                dest: a.dest,
                src,
            })
        };
        match a.ty {
            arg::CODE_CONST => {
                let (index, first_row, rows) = a.code_const();
                push(
                    rate,
                    CbSource::Code {
                        index,
                        first_row,
                        rows: rows.max(1),
                    },
                );
            }
            arg::LITERAL_CONST => push(rate, CbSource::Literal(a.literal.unwrap_or([0.0; 4]))),
            arg::MATERIAL_CONST => {
                let src = match mat.constants.iter().find(|c| c.name_hash == hash) {
                    Some(c) => CbSource::Material(c.literal),
                    None => CbSource::MissingMaterial(hash),
                };
                push(MATERIAL, src);
            }
            arg::MATERIAL_TEXTURE => {
                let t = mat.textures.iter().find(|t| t.0 == hash);
                for s in stages {
                    s.textures.insert(
                        u32::from(a.dest),
                        TexSource::Material {
                            hash,
                            image: t.map(|t| t.3.clone()),
                            semantic: t.map_or(0, |t| t.2),
                        },
                    );
                }
            }
            arg::MATERIAL_SAMPLER => {
                let state = mat
                    .textures
                    .iter()
                    .find(|t| t.0 == hash)
                    .map_or(0x12, |t| t.1);
                for s in stages {
                    s.samplers
                        .insert(u32::from(a.dest), SampSource::Material(state));
                }
            }
            arg::CODE_TEXTURE => {
                for s in stages {
                    s.textures.insert(u32::from(a.dest), TexSource::Code(hash));
                }
            }
            arg::CODE_SAMPLER => {
                for s in stages {
                    s.samplers.insert(u32::from(a.dest), SampSource::Code(hash));
                }
            }
            _ => {}
        }
    }
    // The engine's own slots, where no argument binds anything.
    for (slot, src) in ENGINE_SLOTS {
        out.ps.textures.entry(slot).or_insert(src);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use fastfile_iw6::walk::ShaderArgRec;

    fn a(ty: u8, shader: u8, dest: u16, u: [u8; 8]) -> ShaderArgRec {
        ShaderArgRec {
            ty,
            shader,
            dest,
            u,
            literal: None,
        }
    }

    #[test]
    fn rates_follow_the_counts_and_slots_are_shared() {
        let pass = Pass {
            arg_counts: [1, 1, 1],
            args: vec![
                // per prim: world matrix rows 0..4 at b0[0]
                a(0, STAGE_VS, 0, [73, 1, 0, 4, 0, 0, 0, 0]),
                // per object: light diffuse at b1[5], pixel stage
                a(0, STAGE_PS, 5, [1, 0, 0, 1, 0, 0, 0, 0]),
                // stable: eye offset at b2[24], vertex stage
                a(0, STAGE_VS, 24, [62, 0, 0, 1, 0, 0, 0, 0]),
                a(5, STAGE_PS, 0, [0x41, 0x10, 0xab, 0xa0, 0, 0, 0, 0]),
            ],
            ..Pass::default()
        };
        let tex = [(0xa0ab_1041u32, 0x12u8, 2u8, "wall_col".to_string())];
        let b = resolve(
            &pass,
            &MaterialView {
                textures: &tex,
                constants: &[],
            },
        );
        let bufs: Vec<(u32, u16)> = b.cbuffer.iter().map(|w| (w.buffer, w.dest)).collect();
        assert_eq!(bufs, vec![(0, 0), (1, 5), (2, 24)]);
        assert_eq!(
            b.ps.textures.get(&0),
            Some(&TexSource::Material {
                hash: 0xa0ab_1041,
                image: Some("wall_col".into()),
                semantic: 2
            })
        );
        assert_eq!(b.ps.textures.get(&3), Some(&TexSource::LightmapSecondary));
        assert!(b.vs.textures.is_empty());
    }
}
