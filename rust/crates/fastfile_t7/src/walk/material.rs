//! Materials and technique sets (x64-zt `t7/structs.hpp` layouts; the
//! stream order read on retail `core_post_gfx`, `en_mp_common`, `mp_common`).
//!
//! ```text
//! MaterialTechniqueSet 0x70: 0x00 char* name; 0x08 u32 renderFlags; 0x10 MaterialTechnique* techniques[12]
//! MaterialTechnique  0x2d0: 0x00 char* name; 0x08 MaterialDrawMethod drawMethods[8]; 0x2c8 GfxStateMap* stateMap
//! MaterialDrawMethod  0x58: 0x00 pipeline; 0x08 MaterialVertexDeclaration* (0x78 bytes);
//!                     0x10 primitiveType, indexCount; 0x18 VS; 0x20 PS; 0x28 GS; 0x30 LS; 0x38 HS; 0x40 DS;
//!                     0x48 u8 argCount (x64-zt: stableArgCount), materialType, drawMethodType; 0x50 args
//! Material*Shader     0x28: char* name; char* extraDef; void* shader; u8* program; u32 programSize; u32 crc
//! MaterialShaderArgument 16: u16 type, location, size, buffer; union { float* literalConst (vec4); hashes }
//! ```
//!
//! Stream order of a technique (read, not x64-zt's): the technique struct
//! (aligned 8, no slot of its own), then per draw
//! method its vertex shader (struct, name, extraDef, program aligned 4),
//! **then** its vertex declaration (aligned 4), the pixel shader, the other
//! stages, the arguments (a literal's vec4 follows only when its union
//! pointer is `-1`); after all methods the technique's name and its state
//! map (112 bytes, aligned 4). A draw method's vertex declaration is often
//! a reference to the one an earlier method loaded.
//!
//! (The slot + 16-alignment an earlier version used only made up for
//! two miscounts before `core_post_gfx`'s technique sets: its sound
//! bank's snapshots are 32-aligned and an empty font icon's name is in
//! VIRTUAL like any other; with those, every zone resolves with plain
//! 8-alignment.)
//!
//! ```text
//! Material 0x2a0: 0x00 MaterialInfo (char* name; flags; sort key; surface bits; contents) 0x30
//!                 0x30 MaterialStaticConstantBuffer* constantBuffers[12][6]
//!                 0x270 u8 textureCount, samplerCount, constantCount; cameraRegion
//!                 0x278 techniqueSet; 0x280 MaterialTextureDef* (32 each, aligned 8: GfxImage* image; hashes, uv);
//!                 0x288 MaterialSamplerDef* (8 each, aligned 8); 0x290 MaterialConstantDef* (32 each, aligned 16);
//!                 0x298 Material* enemyMaterial
//! MaterialStaticConstantBuffer 0x48 (aligned 16): 5 runtime buffers; 0x28 u8* initData;
//!                 0x30 u32 initDataSize; 0x38 char* name; 0x40 u32 deviceSlot
//! ```
//! Order: name, the constant buffers (each: struct, initData in PHYSICAL
//! aligned 256, name), the technique set (an asset, struct in TEMP; its
//! pointer is `-2` when inline, so it takes an 8-byte VIRTUAL slot first),
//! the texture table then each texture's image (an asset), the sampler
//! table (aligned 16, also when empty), the constant table, the enemy
//! material. Later references to an image name its texture def's `image`
//! field. (Read on retail `core_post_gfx`, `en_mp_common`, `mp_patch`:
//! every reference resolves and VIRTUAL and PHYSICAL end at the header's
//! sizes.)

use super::capture::MaterialRec;
use crate::asset_type::AssetType;
use crate::cursor::Result;
use crate::stream::{PHYSICAL, Stream};

/// A material's own constant buffer, as retail zones name it.
const GLOBALS: &str = "$Globals";
/// bo2mp: the same buffer as the BO3 mod tools name it (Workshop maps).
pub(crate) const MOD_TOOLS_GLOBALS: &str = "__GLOBAL_CB__";

const TECHNIQUES: usize = 12;
const TECHNIQUE: usize = 0x2d0;
const DRAW_METHODS: usize = 8;
const DRAW_METHOD: usize = 0x58;
const VERTEX_DECL: usize = 0x78;
const SHADER: usize = 0x28;
const ARG: usize = 16;
const STATE_MAP: usize = 112;

const CONSTANT_BUFFERS: usize = 0x30;
const CONSTANT_BUFFER_SLOTS: usize = TECHNIQUES * 6;
const CONSTANT_BUFFER: usize = 0x48;
const TEXTURE_DEF: usize = 32;
const SAMPLER_DEF: usize = 8;
const CONSTANT_DEF: usize = 32;

/// A shader stage: struct, name, extraDef, program (aligned 4). bo2mp:
/// where its program is (file offset, bytes) when it loaded here.
/// (A reference to a shader or technique loaded earlier finds it by where
/// that one's struct was read: `Capture::programs_at`.) The second value:
/// a reference to a shader not read yet, its VIRTUAL offset (the mod
/// tools' Workshop zones name shaders, vertex declarations and state maps
/// that a later asset loads: `Capture::settle_technique_refs`).
fn shader(s: &mut Stream, field: usize) -> Result<(Option<(usize, usize)>, Option<u64>)> {
    let p = s.ptr_at(field)?;
    if let Some(sh) = s.array(field, SHADER, 7)? {
        s.no_loads(sh, SHADER, &[0, 8, 0x18])?;
        s.string(sh)?;
        s.string(sh + 8)?;
        let size = s.u32_at(sh + 0x20)? as usize;
        let program = s.array(sh + 0x18, size, 3)?.map(|at| (at, size));
        if let Some(pr) = program {
            s.capture.shader_programs.insert(sh, pr);
        }
        return Ok((program, None));
    }
    // (bo2mp: on Shadows of Evil 3410 references land 12 or 16 bytes
    // before the shader struct they name, as the virtual map has them: its
    // sky's forward technique had no vertex program, so the sky was not
    // drawn. The struct that starts in the next 16 bytes is the one named:
    // shader structs lie 0x28 and more apart.)
    let r = earlier(s, p).and_then(|at| (0..=16).find_map(|d| s.capture.shader_programs.get(&(at + d)).copied()));
    Ok((r, if r.is_none() { p.virtual_offset() } else { None }))
}

/// The file offset of the struct a reference names.
fn earlier(s: &Stream, p: crate::ptr::Ptr) -> Option<usize> {
    p.virtual_offset().and_then(|v| s.vmap.file_pos(v))
}

/// bo2mp: a technique's first draw method's vertex and pixel programs.
type Programs = Option<[Option<(usize, usize)>; 2]>;

/// bo2mp: a technique (its first draw method's programs, its draw
/// methods), and the VIRTUAL offset of a technique a reference names that
/// was not read yet.
type TechniqueRead = (Programs, Option<super::capture::TechniqueInfo>, Option<u64>);

fn technique(s: &mut Stream, field: usize) -> Result<TechniqueRead> {
    let p = s.ptr_at(field)?;
    if !p.is_inline() {
        s.array(field, 0, 0)?;
        let at = earlier(s, p);
        let info = at.and_then(|at| s.capture.technique_infos.get(&at).cloned());
        let pending = if info.is_none() { p.virtual_offset() } else { None };
        return Ok((at.and_then(|at| s.capture.technique_programs.get(&at).copied()), info, pending));
    }
    let Some(t) = s.array(field, TECHNIQUE, 7)? else { return Ok((None, None, None)) };
    let mut programs: Programs = None;
    let mut info: Option<super::capture::TechniqueInfo> = None;
    let mut methods: [Option<super::capture::DrawMethodInfo>; DRAW_METHODS] = Default::default();
    let mut any_pending = false;
    for m in 0..DRAW_METHODS {
        let d = t + 8 + DRAW_METHOD * m;
        s.no_loads(d, DRAW_METHOD, &[0x08, 0x18, 0x20, 0x28, 0x30, 0x38, 0x40, 0x50])?;
        let (vs, vs_pending) = shader(s, d + 0x18)?;
        let dp = s.ptr_at(d + 0x08)?;
        let decl = match s.array(d + 0x08, VERTEX_DECL, 3)? {
            Some(at) => {
                s.capture.decls.insert(at, at);
                Some(at)
            }
            None => earlier(s, dp).and_then(|at| s.capture.decls.get(&at).copied()),
        };
        let decl_pending = if decl.is_none() { dp.virtual_offset() } else { None };
        let (ps, ps_pending) = shader(s, d + 0x20)?;
        for stage in [0x28, 0x30, 0x38, 0x40] {
            shader(s, d + stage)?;
        }
        let pending = [vs_pending, ps_pending, decl_pending];
        let first = programs.is_none() && (vs.is_some() || ps.is_some());
        if first {
            programs = Some([vs, ps]);
        }
        let n = s.u8_at(d + 0x48)? as usize;
        let mut args = Vec::new();
        if let Some(a) = s.records(d + 0x50, n, ARG, 7)? {
            for i in 0..n {
                let r = a + ARG * i;
                // A literal constant's vec4 follows when its pointer does
                // (otherwise the union holds hashes and indices).
                let literal = if s.ptr_at(r + 8)?.is_inline() {
                    match s.array(r + 8, 16, 3)? {
                        Some(l) => Some([s.f32_at(l)?, s.f32_at(l + 4)?, s.f32_at(l + 8)?, s.f32_at(l + 12)?]),
                        None => None,
                    }
                } else {
                    None
                };
                args.push(super::capture::ShaderArg {
                    ty: s.u16_at(r)?,
                    location: s.u16_at(r + 2)?,
                    size: s.u16_at(r + 4)?,
                    buffer: s.u16_at(r + 6)?,
                    u: [s.u32_at(r + 8)?, s.u32_at(r + 12)?],
                    literal,
                });
            }
        }
        // (A method whose shaders are references not read yet is kept for
        // `Capture::settle_technique_refs`.)
        let waits = vs_pending.is_some() || ps_pending.is_some();
        if vs.is_some() || ps.is_some() || waits {
            any_pending |= pending.iter().any(Option::is_some);
            methods[m] = Some(super::capture::DrawMethodInfo { programs: [vs, ps], decl, buffer_set: s.u8_at(d + 0x4a)?, args: args.clone(), pending });
        }
        if first {
            info = Some(super::capture::TechniqueInfo { programs: [vs, ps], decl, args, methods: Default::default(), state_map: None, pending_state: None });
        }
    }
    if info.is_none() && any_pending {
        info = Some(super::capture::TechniqueInfo::default());
    }
    if let Some(i) = info.as_mut() {
        i.methods = methods;
    }
    s.string(t)?;
    let sp = s.ptr_at(t + 0x2c8)?;
    let state_map = s.records_at(t + 0x2c8, 1, STATE_MAP, 3)?;
    if let Some(i) = info.as_mut() {
        i.state_map = state_map;
        if state_map.is_none() {
            i.pending_state = sp.virtual_offset();
        }
    }
    if let Some(pr) = programs {
        s.capture.technique_programs.insert(t, pr);
    }
    if let Some(i) = &info {
        s.capture.technique_infos.insert(t, i.clone());
    }
    Ok((programs, info, None))
}

pub fn technique_set(s: &mut Stream, h: usize) -> Result<Option<String>> {
    let name = s.string(h)?;
    let mut rec = super::capture::TechsetRec { name: name.clone().unwrap_or_default(), ..Default::default() };
    for i in 0..TECHNIQUES {
        let (programs, info, pending) = technique(s, h + 0x10 + 8 * i)?;
        rec.programs[i] = programs;
        rec.techniques[i] = info;
        rec.pending[i] = pending;
    }
    s.capture.add_techset(rec);
    Ok(name)
}

pub fn material(s: &mut Stream, h: usize) -> Result<Option<String>> {
    let mut skip: Vec<usize> = vec![0, 0x278, 0x280, 0x288, 0x290, 0x298];
    skip.extend((0..CONSTANT_BUFFER_SLOTS).map(|i| CONSTANT_BUFFERS + 8 * i));
    s.no_loads(h, AssetType::Material.header_size(), &skip)?;
    let name = s.string(h)?;
    let mut colour_consts = None;
    let mut globals: Vec<(u8, Vec<u8>)> = Vec::new();
    let mut all_cbuffers: Vec<(u8, String, Vec<u8>)> = Vec::new();
    let mut pending_cbuffers: Vec<(u8, u64)> = Vec::new();
    for i in 0..CONSTANT_BUFFER_SLOTS {
        // bo2mp: a slot that references a constant buffer an earlier
        // material loaded (materials of one technique set with the same
        // constants share it) takes that one's name and contents.
        let field = h + CONSTANT_BUFFERS + 8 * i;
        if let p @ crate::ptr::Ptr::Ref { .. } = s.ptr_at(field)? {
            match p.virtual_offset().and_then(|v| s.vmap.file_pos(v)).and_then(|at| s.capture.cbuffer_at.get(&at).cloned()) {
                Some((n, d)) => all_cbuffers.push((i as u8, n, d)),
                // (A buffer not read yet: looked up after the walk.)
                None => pending_cbuffers.extend(p.virtual_offset().map(|v| (i as u8, v))),
            }
        }
        if let Some(c) = s.array(field, CONSTANT_BUFFER, 15)? {
            s.no_loads(c, CONSTANT_BUFFER, &[0x28, 0x38])?;
            let size = s.u32_at(c + 0x30)? as usize;
            let init = s.ptr_at(c + 0x28)?;
            s.push(PHYSICAL);
            let r = s.array(c + 0x28, size, 255);
            s.pop();
            // bo2mp: initial data the zone shares with an earlier buffer (a
            // reference into PHYSICAL: a zone keeps one copy of equal
            // constants). Read where that one was; before, such a buffer was
            // dropped and its draws skipped (`constant buffer not in the
            // material`: Rome 24304 surfaces, Shrine 6060, Warhawk 2093).
            let data = r?.or_else(|| match init {
                crate::ptr::Ptr::Ref { block, offset } if block == PHYSICAL => s.pmap.file_pos(offset).filter(|&at| size == 0 || s.u8_at(at + size - 1).is_ok()),
                _ => None,
            });
            let buffer = s.string(c + 0x38)?;
            if let Some(at) = data {
                let mut d = vec![0u8; size];
                for (k, b) in d.iter_mut().enumerate() {
                    *b = s.u8_at(at + k)?;
                }
                s.capture.cbuffer_at.insert(c, (buffer.clone().unwrap_or_default(), d.clone()));
                all_cbuffers.push((i as u8, buffer.clone().unwrap_or_default(), d));
            }
            // bo2mp: the BO3 mod tools name a material's own constant
            // buffer `__GLOBAL_CB__` where the retail zones say `$Globals`
            // (Steam Workshop Slums' and Summit's sky: its `skyScale` read as
            // 1 instead of 1448); the shaders' RDEF still says `$Globals`.
            let own = matches!(buffer.as_deref(), Some(GLOBALS | MOD_TOOLS_GLOBALS));
            if own
                && let Some(at) = data
                && !globals.iter().any(|(_, d): &(u8, Vec<u8>)| d.len() == size && (0..size).all(|k| s.u8_at(at + k).is_ok_and(|b| b == d[k])))
            {
                let mut d = vec![0u8; size];
                for (k, b) in d.iter_mut().enumerate() {
                    *b = s.u8_at(at + k)?;
                }
                globals.push((i as u8, d));
            }
            if colour_consts.is_none()
                && size >= 192
                && own
                && let Some(at) = data
            {
                let mut v = [0f32; 8];
                for (k, x) in v.iter_mut().enumerate() {
                    *x = s.f32_at(at + 4 * (40 + k))?;
                }
                colour_consts = Some(v);
            }
        }
    }
    let mut rec = MaterialRec {
        name: name.clone().unwrap_or_else(String::new),
        colour_consts,
        globals,
        cbuffers: all_cbuffers,
        pending_cbuffers,
        ..Default::default()
    };
    // MaterialInfo +0x10: the draw surface sort key the exe copies into
    // every draw of the material (`141d8ce20`: `*surf = material[+0x10] &
    // 0xfffffffffff80001`), its lists sorted by it.
    rec.sort_key = u64::from(s.u32_at(h + 0x10)?) | (u64::from(s.u32_at(h + 0x14)?) << 32);
    rec.techset = s.asset_named(AssetType::TechniqueSet, h + 0x278)?;
    let textures = s.u8_at(h + 0x270)? as usize;
    if let Some(t) = s.records(h + 0x280, textures, TEXTURE_DEF, 7)? {
        for i in 0..textures {
            let def = t + TEXTURE_DEF * i;
            let mut raw = [0u8; 32];
            for (k, b) in raw.iter_mut().enumerate() {
                *b = s.u8_at(def + k)?;
            }
            rec.texture_defs.push(raw);
            if let Some(img) = s.asset_named(AssetType::Image, def)? {
                rec.textures.push((s.u32_at(def + 8)?, img));
            }
        }
    }
    let samplers = s.u8_at(h + 0x271)? as usize;
    // The sampler table is 16-aligned. (bo2mp: a table an earlier
    // material loaded is read where it was: it holds no pointers.)
    if let Some(t) = s.records_at(h + 0x288, samplers, SAMPLER_DEF, 15)? {
        for i in 0..samplers {
            let mut raw = [0u8; 8];
            for (k, b) in raw.iter_mut().enumerate() {
                *b = s.u8_at(t + SAMPLER_DEF * i + k)?;
            }
            rec.sampler_defs.push(raw);
        }
    }
    let constants = s.u8_at(h + 0x272)? as usize;
    s.records(h + 0x290, constants, CONSTANT_DEF, 15)?;
    s.asset(AssetType::Material, h + 0x298)?;
    s.capture.add_material(rec);
    Ok(name)
}
