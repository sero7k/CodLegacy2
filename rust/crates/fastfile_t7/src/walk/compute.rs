//! Compute shader sets (`core_patch`'s code compute programs), as the exe's
//! loader reads them:
//!
//! ```text
//! ComputeShaderSet 0x18: 0x00 char* name; 0x08 u32 count; 0x10 ComputeShader*[count] (aligned 8)
//! ComputeShader   0x1d0: 0x00 char* name; 0x08 char*; 0x10 eight variants (0x38 each):
//!                 +0x08 u8* program, +0x10 u32 size (the bytes follow unaligned);
//!                 +0x18 u32 thread group x, y, z (dispatches are divided by them);
//!                 +0x24 u32 count, +0x28 records[count] (16 each, aligned 8:
//!                 the shader arguments, as a material's (`ShaderArg`; type 0 a
//!                 literal: +8 points at its vec4, aligned 4);
//!                 +0x30 u8 the GPU path the variant is for (2 generic D3D11,
//!                 3 NVIDIA extensions, 4 AMD extensions; the exe creates the
//!                 one matching its device, 2 without vendor extensions)
//! ```

use super::capture::ComputeShaderRec;
use crate::asset_type::AssetType;
use crate::cursor::Result;
use crate::stream::Stream;

pub fn compute_shader_set(s: &mut Stream, h: usize) -> Result<Option<String>> {
    s.no_loads(h, AssetType::ComputeShaderSet.header_size(), &[0, 0x10])?;
    let set = s.string(h)?;
    let n = s.u32_at(h + 8)? as usize;
    if let Some(t) = s.records(h + 0x10, n, 8, 7)? {
        for i in 0..n {
            let Some(c) = s.array(t + 8 * i, 0x1d0, 7)? else { continue };
            let name = s.string(c)?;
            let extra = s.string(c + 8)?;
            let mut variants = Vec::new();
            for v in 0..8 {
                let e = c + 0x10 + 0x38 * v;
                let size = s.u32_at(e + 0x10)? as usize;
                let program = s.array(e + 8, size, 0)?.map(|at| (at, size));
                let m = s.u32_at(e + 0x24)? as usize;
                let mut args = Vec::new();
                if let Some(r) = s.records(e + 0x28, m, 16, 7)? {
                    for k in 0..m {
                        let rec = r + 16 * k;
                        let literal = if s.u16_at(rec)? == 0 {
                            match s.array(rec + 8, 16, 3)? {
                                Some(l) => Some([s.f32_at(l)?, s.f32_at(l + 4)?, s.f32_at(l + 8)?, s.f32_at(l + 12)?]),
                                None => None,
                            }
                        } else {
                            None
                        };
                        args.push(super::capture::ShaderArg {
                            ty: s.u16_at(rec)?,
                            location: s.u16_at(rec + 2)?,
                            size: s.u16_at(rec + 4)?,
                            buffer: s.u16_at(rec + 6)?,
                            u: [s.u32_at(rec + 8)?, s.u32_at(rec + 12)?],
                            literal,
                        });
                    }
                }
                let groups = [s.u32_at(e + 0x18)?, s.u32_at(e + 0x1c)?, s.u32_at(e + 0x20)?];
                variants.push(super::capture::ComputeVariant { path: s.u8_at(e + 0x30)?, program, groups, args });
            }
            s.capture.compute_shaders.push(ComputeShaderRec {
                set: set.clone().unwrap_or_default(),
                name: name.unwrap_or_default(),
                extra,
                variants,
            });
        }
    }
    Ok(set)
}
