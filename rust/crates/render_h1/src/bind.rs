//! What a pass binds where, from its argument table and the material.
//!
//! A pass's constant buffers are one per update rate, the same slot in
//! every stage: `b0` per primitive (the world matrix), `b1` per object
//! (camera, primary light, shadow lookup), `b2` stable (shadow cascades,
//! fog, script values), and `b3` the material's own constants. Arguments
//! come per primitive first, then per object, then stable; an argument's
//! `dest` is the 16-byte register it starts at. Texture and sampler
//! arguments name the `t#` / `s#` slot. The custom sampler flags add the
//! reflection probe (`t1`/`s1`) and the lightmaps (`t2`, `t3`).

use std::collections::BTreeMap;

use fastfile_h1::code_source::arg;
use fastfile_h1::walk::{MaterialConstant, PassRec};

pub const PER_PRIM: u32 = 0;
pub const PER_OBJECT: u32 = 1;
pub const STABLE: u32 = 2;
pub const MATERIAL: u32 = 3;

/// Added to a pixel-stage buffer's slot: in D3D each stage has its own
/// `b#` buffers, so a vertex and a pixel argument may name the same
/// register of the same slot with different values (the wind techniques'
/// vertex `flagWave0Parms` and pixel `transmittance` are both `b3[8]`).
/// They were one shared buffer, the later write winning (magenta foliage).
/// The per-primitive block (`b0`) stays shared.
pub const PS_BUFFER: u32 = 8;

/// The buffer a stage's `b{slot}` is filled from.
pub fn buffer_key(slot: u32, is_ps: bool) -> u32 {
    static SHARED: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    let shared = *SHARED.get_or_init(|| std::env::var("BO2MP_H1_SHARED_CB").is_ok());
    if is_ps && slot != PER_PRIM && !shared {
        slot + PS_BUFFER
    } else {
        slot
    }
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
    /// when the material has no texture of the hash).
    Material {
        hash: u32,
        image: Option<String>,
        semantic: u8,
    },
    /// A code texture (`code_source::CODE_TEXTURE`).
    Code(u32),
    ReflectionProbe,
    LightmapPrimary,
    LightmapSecondary,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SampSource {
    /// A material sampler state byte (filter, mip, clamp bits).
    Material(u8),
    /// A code sampler (`code_source::CODE_TEXTURE` index).
    Code(u32),
    /// The reflection probe's sampler.
    Probe,
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

pub fn resolve(pass: &PassRec, mat: &MaterialView) -> PassBinds {
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
        let stages: Vec<&mut StageBinds> = {
            let mut v = Vec::new();
            let (vs, ps) = (&mut out.vs, &mut out.ps);
            if a.shader & STAGE_VS != 0 {
                v.push(vs);
            }
            if a.shader & STAGE_PS != 0 {
                v.push(ps);
            }
            v
        };
        let hash = a.word();
        // A constant goes to each stage's own buffer of its slot.
        let mut keys: Vec<bool> = Vec::new();
        if a.shader & STAGE_VS != 0 {
            keys.push(false);
        }
        if a.shader & STAGE_PS != 0 {
            keys.push(true);
        }
        if keys.is_empty() {
            keys.push(false);
        }
        let mut push = |slot: u32, src: CbSource| {
            let mut seen = Vec::new();
            for &ps in &keys {
                let buffer = buffer_key(slot, ps);
                if !seen.contains(&buffer) {
                    seen.push(buffer);
                    out.cbuffer.push(CbWrite {
                        buffer,
                        dest: a.dest,
                        src: src.clone(),
                    });
                }
            }
        };
        match a.kind {
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
    // Custom samplers (`CUSTOM_SAMPLER_FLAG_*`): the pixel stage's.
    let f = pass.custom_sampler_flags;
    if f & 1 != 0 {
        out.ps
            .textures
            .entry(1)
            .or_insert(TexSource::ReflectionProbe);
        out.ps.samplers.entry(1).or_insert(SampSource::Probe);
    }
    if f & 2 != 0 {
        out.ps
            .textures
            .entry(2)
            .or_insert(TexSource::LightmapPrimary);
    }
    if f & 4 != 0 {
        out.ps
            .textures
            .entry(3)
            .or_insert(TexSource::LightmapSecondary);
    }
    out
}
