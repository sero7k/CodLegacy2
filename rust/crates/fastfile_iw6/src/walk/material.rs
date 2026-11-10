//! Shaders, vertex declarations, technique sets, images and materials.

use super::{AssetId, Walker};
use crate::asset_type::AssetType;
use crate::cursor::Result;
use crate::layout as L;
use crate::ptr::Ptr;
use crate::stream::{CALLBACK, TEMP, VIRTUAL};
use crate::walk::{
    ImageRec, MaterialConstant, MaterialRec, MaterialTexture, PassRec, ShaderArgRec, Span,
    TechniqueRec, TechniqueSetRec,
};

/// Vertex, pixel, hull and domain shaders keep their program in VIRTUAL;
/// compute shaders in TEMP (unverified: no zone read so far holds one).
pub(super) fn shader(
    w: &mut Walker,
    ty: AssetType,
    h: usize,
    temp_program: bool,
) -> Result<AssetId> {
    use L::MaterialVertexShader as S;
    // (Every shader asset: name, then the program pointer and size at the
    // same place: `prog.loadDef.program`, `programSize`.)
    w.s.push(VIRTUAL);
    let name = w.xstr(h + S::name)?.unwrap_or_default();
    let size = w.s.u32_at(h + S::prog__loadDef__programSize)? as usize;
    if temp_program {
        w.s.push(TEMP);
    }
    let at = w.blob(h + S::prog__loadDef__program, size, 3)?;
    if temp_program {
        w.s.pop();
    }
    w.s.pop();
    let id = w.name_only(ty, name);
    if let Some(pos) = at {
        w.cap.shader_programs.insert(id, Span { pos, len: size });
    }
    Ok(id)
}

pub(super) fn vertex_decl(w: &mut Walker, h: usize) -> Result<AssetId> {
    w.s.push(VIRTUAL);
    let name = w
        .xstr(h + L::MaterialVertexDeclaration::name)?
        .unwrap_or_default();
    w.s.pop();
    Ok(w.name_only(AssetType::VertexDecl, name))
}

const TECHNIQUE_COUNT: usize = 412;

pub(super) fn technique_set(w: &mut Walker, h: usize) -> Result<AssetId> {
    use L::MaterialTechniqueSet as T;
    w.s.push(VIRTUAL);
    let name = w.xstr(h + T::name)?.unwrap_or_default();
    let mut rec = TechniqueSetRec {
        name,
        world_vert_format: w.s.u8_at(h + T::worldVertFormat)?,
        techniques: Vec::new(),
        technique_ids: Vec::new(),
    };
    for t in 0..TECHNIQUE_COUNT {
        let field = h + T::techniques + 8 * t;
        match w.s.ptr_at(field)? {
            p @ (Ptr::Follow | Ptr::Insert) => {
                if p == Ptr::Insert {
                    w.s.insert_slot();
                }
                let (tname, passes, id) = technique(w)?;
                rec.techniques.push((t, tname, passes));
                rec.technique_ids.push((t, id));
            }
            Ptr::Ref {
                block: VIRTUAL,
                offset,
            } => {
                // A technique shared with an earlier set: its name.
                let name = w
                    .vmap
                    .file_pos(offset as u64)
                    .and_then(|at| w.s.ptr_at(at + L::MaterialTechniqueHeader::name).ok())
                    .and_then(|p| p.virtual_offset())
                    .and_then(|v| w.string_at(v))
                    .unwrap_or_default();
                let passes = w
                    .vmap
                    .file_pos(offset as u64)
                    .and_then(|at| w.s.u16_at(at + L::MaterialTechniqueHeader::passCount).ok())
                    .unwrap_or(0);
                rec.techniques.push((t, name, passes));
                if let Some(id) = w
                    .vmap
                    .file_pos(offset as u64)
                    .and_then(|at| w.cap.technique_at.get(&at).copied())
                {
                    rec.technique_ids.push((t, id));
                }
            }
            _ => {}
        }
    }
    w.s.pop();
    w.cap.techsets.push(rec);
    Ok(AssetId {
        ty: AssetType::TechniqueSet,
        index: (w.cap.techsets.len() - 1) as u32,
    })
}

/// One technique (header + passes, VIRTUAL aligned 4), its shaders and
/// arguments, then its name. Returns the name, the pass count and the
/// technique's index in `Capture::techniques`.
fn technique(w: &mut Walker) -> Result<(String, u16, u32)> {
    use L::MaterialPass as P;
    let hdr_len = L::MaterialTechniqueHeader::SIZE;
    // The header and pass count first, then the passes.
    let at = w.s.pos();
    let passes = w.s.u16_at(at + L::MaterialTechniqueHeader::passCount)? as usize;
    let hdr = w.st(hdr_len + P::SIZE * passes, 3)?;
    let mut rec = TechniqueRec {
        flags: w.s.u16_at(hdr + L::MaterialTechniqueHeader::flags)?,
        ..Default::default()
    };
    for i in 0..passes {
        let p = hdr + hdr_len + P::SIZE * i;
        let mut pass = PassRec {
            vertex_shader: w.asset(AssetType::VertexShader, p + P::vertexShader)?,
            vertex_decl: w.asset(AssetType::VertexDecl, p + P::vertexDecl)?,
            hull_shader: w.asset(AssetType::HullShader, p + P::hullShader)?,
            domain_shader: w.asset(AssetType::DomainShader, p + P::domainShader)?,
            pixel_shader: w.asset(AssetType::PixelShader, p + P::pixelShader)?,
            arg_counts: [
                w.s.u8_at(p + P::perPrimArgCount)?,
                w.s.u8_at(p + P::perObjArgCount)?,
                w.s.u8_at(p + P::stableArgCount)?,
            ],
            arg_sizes: [
                w.s.u16_at(p + P::perPrimArgSize)?,
                w.s.u16_at(p + P::perObjArgSize)?,
                w.s.u16_at(p + P::stableArgSize)?,
            ],
            args: Vec::new(),
        };
        let n = pass.arg_counts.iter().map(|&c| c as usize).sum::<usize>();
        let a_len = L::MaterialShaderArgument::SIZE;
        if let Some(args) = w.inline_array(p + P::args, a_len * n, 3)? {
            use L::MaterialShaderArgument as A;
            for k in 0..n {
                let arg = args + a_len * k;
                let mut a = ShaderArgRec {
                    ty: w.s.u8_at(arg + A::r#type)?,
                    shader: w.s.u8_at(arg + A::shader)?,
                    dest: w.s.u16_at(arg + A::dest)?,
                    ..Default::default()
                };
                for (j, b) in a.u.iter_mut().enumerate() {
                    *b = w.s.u8_at(arg + A::u + j)?;
                }
                if a.ty == 4
                    && let Some(at) = w.blob(arg + A::u, 16, 3)?
                {
                    let mut v = [0f32; 4];
                    for (j, f) in v.iter_mut().enumerate() {
                        *f = f32::from_bits(w.s.u32_at(at + 4 * j)?);
                    }
                    a.literal = Some(v);
                }
                pass.args.push(a);
            }
        }
        rec.passes.push(pass);
    }
    rec.name = w
        .xstr(hdr + L::MaterialTechniqueHeader::name)?
        .unwrap_or_default();
    let id = w.cap.techniques.len() as u32;
    w.cap.technique_at.insert(hdr, id);
    let name = rec.name.clone();
    w.cap.techniques.push(rec);
    Ok((name, passes as u16, id))
}

pub(super) fn image(w: &mut Walker, h: usize) -> Result<AssetId> {
    use L::GfxImage as G;
    w.s.push(VIRTUAL);
    let name = w.xstr(h + G::name)?.unwrap_or_default();
    let semantic = w.s.u8_at(h + G::semantic)?;
    let len1 = w.s.u32_at(h + G::dataLen1)?;
    w.s.push(TEMP);
    let n = if semantic != 11 { len1 as usize } else { 1 };
    let pixels = w
        .blob(h + G::pixelData, n, 3)?
        .map(|pos| Span { pos, len: n });
    w.s.pop();
    w.s.pop();
    let streamed = w.s.u8_at(h + G::streamed)? != 0;
    let mut streams = [(0u16, 0u16, 0u32, 0u8); 4];
    for (i, s) in streams.iter_mut().enumerate() {
        let o = h + G::streams + L::GfxImageStreamData::SIZE * i;
        let packed = w.s.u32_at(o + L::GfxImageStreamData::levelCountAndSize)?;
        *s = (
            w.s.u16_at(o)?,
            w.s.u16_at(o + 2)?,
            packed & 0x03FF_FFFF,
            (packed >> 26) as u8,
        );
    }
    let stream_files = if streamed {
        w.take_stream_files()
    } else {
        [None; 4]
    };
    let rec = ImageRec {
        name,
        format: w.s.u32_at(h + G::imageFormat)?,
        map_type: w.s.u8_at(h + G::mapType)?,
        semantic,
        category: w.s.u8_at(h + G::category)?,
        flags: w.s.u8_at(h + G::flags)?,
        width: w.s.u16_at(h + G::width)?,
        height: w.s.u16_at(h + G::height)?,
        depth: w.s.u16_at(h + G::depth)?,
        num_elements: w.s.u16_at(h + G::numElements)?,
        level_count: w.s.u8_at(h + G::levelCount)?,
        streamed,
        data_len1: len1,
        data_len2: w.s.u32_at(h + G::dataLen2)?,
        pixels,
        streams,
        stream_files,
    };
    w.cap.images.push(rec);
    Ok(AssetId {
        ty: AssetType::Image,
        index: (w.cap.images.len() - 1) as u32,
    })
}

/// A water map's `water_t` (its height and term tables, then its images).
fn water(w: &mut Walker, field: usize) -> Result<Option<AssetId>> {
    use L::water_t as W;
    let Some(at) = w.inline_array(field, W::SIZE, 3)? else {
        return Ok(None);
    };
    let n = (w.s.u32_at(at + W::M)? as usize) * (w.s.u32_at(at + W::N)? as usize);
    w.blob(at + W::H0X, 4 * n, 3)?;
    w.blob(at + W::H0Y, 4 * n, 3)?;
    w.blob(at + W::wTerm, 4 * n, 3)?;
    let image = w.asset(AssetType::Image, at + W::image)?;
    w.asset(AssetType::Image, at + W::stagingImage)?;
    Ok(image)
}

pub(super) fn material(w: &mut Walker, h: usize) -> Result<AssetId> {
    use L::Material as M;
    w.s.push(VIRTUAL);
    let name = w.xstr(h + M::info__name)?.unwrap_or_default();
    let techset = w.asset(AssetType::TechniqueSet, h + M::techniqueSet)?;
    let tex_n = w.s.u8_at(h + M::textureCount)? as usize;
    let mut textures = Vec::new();
    if !matches!(w.s.ptr_at(h + M::textureTable)?, Ptr::Null) {
        w.s.inc(CALLBACK, (4 * tex_n) as u64);
    }
    let td = L::MaterialTextureDef::SIZE;
    match w.inline_array(h + M::textureTable, td * tex_n, 3)? {
        Some(table) => {
            for i in 0..tex_n {
                let t = table + td * i;
                let semantic = w.s.u8_at(t + L::MaterialTextureDef::semantic)?;
                let image = if semantic == 11 {
                    water(w, t + L::MaterialTextureDef::u)?
                } else {
                    w.asset(AssetType::Image, t + L::MaterialTextureDef::u)?
                };
                textures.push(MaterialTexture {
                    name_hash: w.s.u32_at(t + L::MaterialTextureDef::nameHash)?,
                    name_start: w.s.u8_at(t + L::MaterialTextureDef::nameStart)?,
                    name_end: w.s.u8_at(t + L::MaterialTextureDef::nameEnd)?,
                    sampler_state: w.s.u8_at(t + L::MaterialTextureDef::samplerState)?,
                    semantic,
                    image,
                });
            }
        }
        None => {
            if let Some(Ptr::Ref {
                block: VIRTUAL,
                offset,
            }) = w.s.ptr_at(h + M::textureTable).ok()
                && let Some(table) = w.vmap.file_pos(offset as u64)
            {
                for i in 0..tex_n {
                    let t = table + td * i;
                    let image =
                        w.s.ptr_at(t + L::MaterialTextureDef::u)
                            .ok()
                            .and_then(|p| p.virtual_offset())
                            .and_then(|v| w.resolve_slot(AssetType::Image, v));
                    textures.push(MaterialTexture {
                        name_hash: w.s.u32_at(t)?,
                        name_start: w.s.u8_at(t + 4)?,
                        name_end: w.s.u8_at(t + 5)?,
                        sampler_state: w.s.u8_at(t + 6)?,
                        semantic: w.s.u8_at(t + 7)?,
                        image,
                    });
                }
            }
        }
    }
    let const_n = w.s.u8_at(h + M::constantCount)? as usize;
    let cd = L::MaterialConstantDef::SIZE;
    let mut constants = Vec::new();
    if let Some(table) = w.blob(h + M::constantTable, cd * const_n, 15)? {
        for i in 0..const_n {
            let c = table + cd * i;
            let raw = w.s.slice(c + L::MaterialConstantDef::name, 12)?;
            let name = super::text(&raw[..raw.iter().position(|&b| b == 0).unwrap_or(12)]);
            let mut literal = [0f32; 4];
            for (k, v) in literal.iter_mut().enumerate() {
                *v = w.s.f32_at(c + L::MaterialConstantDef::literal + 4 * k)?;
            }
            constants.push(MaterialConstant {
                name_hash: w.s.u32_at(c)?,
                name,
                literal,
            });
        }
    }
    let sb_n = w.s.u8_at(h + M::stateBitsCount)? as usize;
    let sb = L::GfxStateBits::SIZE;
    let mut state_bits = Vec::new();
    if let Some(table) = w.blob(h + M::stateBitsTable, sb * sb_n, 3)? {
        for i in 0..sb_n {
            let mut b = [0u8; 28];
            b.copy_from_slice(w.s.slice(table + sb * i, sb)?);
            state_bits.push(b);
        }
    }
    let cb_n = w.s.u8_at(h + M::constantBufferCount)? as usize;
    let cbd = L::MaterialConstantBufferDef::SIZE;
    if let Some(table) = w.inline_array(h + M::constantBufferTable, cbd * cb_n, 3)? {
        use L::MaterialConstantBufferDef as C;
        for i in 0..cb_n {
            let c = table + cbd * i;
            for (data, size) in [
                (C::vsData, C::vsDataSize),
                (C::hsData, C::hsDataSize),
                (C::dsData, C::dsDataSize),
                (C::psData, C::psDataSize),
            ] {
                let n = w.s.u32_at(c + size)? as usize;
                w.blob(c + data, n, 15)?;
            }
            for (data, size) in [
                (C::vsOffsetData, C::vsOffsetDataSize),
                (C::hsOffsetData, C::hsOffsetDataSize),
                (C::dsOffsetData, C::dsOffsetDataSize),
                (C::psOffsetData, C::psOffsetDataSize),
            ] {
                let n = w.s.u32_at(c + size)? as usize;
                w.blob(c + data, 2 * n, 1)?;
            }
        }
    }
    let layers = w.s.u8_at(h + M::layerCount)? as usize;
    if let Some(table) = w.inline_array(h + M::subMaterials, 8 * layers, 7)? {
        for i in 0..layers {
            w.xstr(table + 8 * i)?;
        }
    }
    w.s.pop();
    let rec = MaterialRec {
        name,
        game_flags: w.s.u8_at(h + M::info__gameFlags)?,
        sort_key: w.s.u8_at(h + M::info__sortKey)?,
        texture_atlas: (
            w.s.u8_at(h + M::info__textureAtlasRowCount)?,
            w.s.u8_at(h + M::info__textureAtlasColumnCount)?,
        ),
        state_flags: w.s.u8_at(h + M::stateFlags)?,
        camera_region: w.s.u8_at(h + M::cameraRegion)?,
        material_type: w.s.u8_at(h + M::materialType)?,
        techset,
        textures,
        constants,
        state_bits,
        state_bits_entry: w.s.slice(h + M::stateBitsEntry, 412)?.to_vec(),
    };
    w.cap.materials.push(rec);
    Ok(AssetId {
        ty: AssetType::Material,
        index: (w.cap.materials.len() - 1) as u32,
    })
}
