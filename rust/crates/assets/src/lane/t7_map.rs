//! bo2mp: Call of Duty: Black Ops III (T7) maps, multiplayer and Zombies.
//!
//! A Black Ops III map zone (`TAff0000`, version 0x251) is read whole by
//! `fastfile_t7::map` (the zone's exact walk; placed models' meshes and
//! pictures from the zone or the `.xpak` packages beside it); this turns
//! it into the match's world the way the Infinite Warfare lane does
//! (`super::iw7`): the world goes to the IW3 world builder (Black Ops III's
//! world vertex is CoD4's layout but for its unit vectors and its UVs as
//! floats), the placed models through Black Ops II's model builder (their
//! surfaces packed as Black Ops II's vertices), materials onto BO2's draw
//! path with their colour maps.
//!
//! Collision is the zone's own (`asset_world::world_t7`): brushes under the
//! BSP and the triangle mesh, plus each static model the clip map lists,
//! through its model's collision triangles (`XModelCollSurf`). The entity string is plain text with Black Ops II's class
//! names (`mp_tdm_spawn`, ...), so Black Ops II's rules read it as a
//! Treyarch map (`attach_t6_rules`).
//!
//! No ZoneGame of its own: the map walk sends a zone of Black Ops III's
//! version here (`asset_transport::legacy_game_for_path` says `t7`).

use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

use asset_core::{AssetEdge, AssetNamespace, AssetRef, T6Blend, T6Cull, T6Draw, ZoneOwner};
use asset_material::{AuthoredImage, AuthoredMaterial, MaterialCatalog, MaterialTextureBinding, TS_COLOR_MAP};
use asset_transport::progress::{LoadProgress, StageId};
use asset_world::WorldDrawPolicy;
use fastfile_t7::map::{MapSources, T7Map};

use super::LoadedWorld;
use crate::lane_capability::PreparedCapability;
use crate::session_load::PreparedWorld;

/// The largest level kept of a surface's or model's colour map
/// (`BO2MP_T7_MAP_TEXTURE=<size>` changes it).
const TEXTURE_MAX: u32 = 512;

/// A placed model smaller than this (its radius, scaled) does not collide.
const COLLIDE_MIN_RADIUS: f32 = 16.0;

fn texture_max() -> u32 {
    std::env::var("BO2MP_T7_MAP_TEXTURE")
        .ok()
        .and_then(|v| v.trim().parse().ok())
        .filter(|&v: &u32| (64..=4096).contains(&v))
        .unwrap_or(TEXTURE_MAX)
}

/// CoD4's packed unit vector (as `asset_model::unpack_unit_vec` reads it,
/// scale byte 63: 1/127 a step).
fn iw3_unit_vec(v: [f32; 3]) -> u32 {
    let b = |x: f32| (x.clamp(-1.0, 1.0) * 127.0 + 127.0).round().clamp(0.0, 255.0) as u32;
    b(v[0]) | b(v[1]) << 8 | b(v[2]) << 16 | 63 << 24
}

/// Black Ops II's packed unit vector: signed 10:10:10 over 511.
pub(super) fn t6_unit_vec(v: [f32; 3]) -> u32 {
    let mut out = 0u32;
    for (k, c) in v.iter().enumerate() {
        let s = (c.clamp(-1.0, 1.0) * 511.0).round() as i32;
        out |= ((s as u32) & 0x3ff) << (10 * k);
    }
    out
}

/// IEEE half from f32 (round to nearest; UVs only, so no NaN care).
pub(super) fn f16_bits(x: f32) -> u16 {
    let b = x.to_bits();
    let sign = ((b >> 16) & 0x8000) as u16;
    let exp = ((b >> 23) & 0xff) as i32 - 127 + 15;
    let man = b & 0x7f_ffff;
    if exp >= 31 {
        return sign | 0x7bff;
    }
    if exp <= 0 {
        if exp < -10 {
            return sign;
        }
        let m = (man | 0x80_0000) >> (1 - exp);
        return sign | ((m + 0x1000) >> 13) as u16;
    }
    let h = (sign as u32) | ((exp as u32) << 10) | (man >> 13);
    (h + ((man >> 12) & 1)) as u16
}

/// One vertex as Black Ops II's packed one (32 bytes: position, binormal
/// sign, colour, UV halves, normal, tangent).
pub(super) fn t6_vertex_signed(p: [f32; 3], sign: f32, uv: [f32; 2], n: [f32; 3], t: [f32; 3]) -> [u8; 32] {
    let mut v = [0u8; 32];
    for k in 0..3 {
        v[4 * k..4 * k + 4].copy_from_slice(&p[k].to_le_bytes());
    }
    v[12..16].copy_from_slice(&sign.to_le_bytes());
    v[16..20].copy_from_slice(&[255; 4]);
    let tex = u32::from(f16_bits(uv[0])) | u32::from(f16_bits(uv[1])) << 16;
    v[20..24].copy_from_slice(&tex.to_le_bytes());
    v[24..28].copy_from_slice(&t6_unit_vec(n).to_le_bytes());
    v[28..32].copy_from_slice(&t6_unit_vec(t).to_le_bytes());
    v
}

/// A tangent for a normal (any vector across it).
fn tangent_of(n: [f32; 3]) -> [f32; 3] {
    let a = if n[2].abs() < 0.9 { [0.0, 0.0, 1.0] } else { [1.0, 0.0, 0.0] };
    let t = [a[1] * n[2] - a[2] * n[1], a[2] * n[0] - a[0] * n[2], a[0] * n[1] - a[1] * n[0]];
    let l = (t[0] * t[0] + t[1] * t[1] + t[2] * t[2]).sqrt().max(1e-6);
    t.map(|x| x / l)
}

/// A model vertex as Black Ops II's packed one (32 bytes: position,
/// binormal sign, colour, UV halves, normal, tangent).
fn t6_vertex(p: [f32; 3], n: [f32; 3], uv: [f32; 2]) -> [u8; 32] {
    let mut v = [0u8; 32];
    for k in 0..3 {
        v[4 * k..4 * k + 4].copy_from_slice(&p[k].to_le_bytes());
    }
    v[12..16].copy_from_slice(&1.0f32.to_le_bytes());
    v[16..20].copy_from_slice(&[255, 255, 255, 255]);
    let tex = u32::from(f16_bits(uv[0])) | u32::from(f16_bits(uv[1])) << 16;
    v[20..24].copy_from_slice(&tex.to_le_bytes());
    v[24..28].copy_from_slice(&t6_unit_vec(n).to_le_bytes());
    v[28..32].copy_from_slice(&t6_unit_vec(tangent_of(n)).to_le_bytes());
    v
}

/// What a material is not drawn as, by the words of its name and its
/// technique set's.
fn hidden_kind(material: &str, technique_set: &str) -> Option<&'static str> {
    let words: Vec<&str> = material.split(['_', '/', '#']).chain(technique_set.split(['_', '/', '#'])).collect();
    // (A glowing dome over the sky is drawn: Nuk3town's hex grid,
    // `mtl_p7_ntx_skybox1`, `lit_emissive_scroll_transparent_sky`.)
    if words.iter().any(|w| w.starts_with("sky")) && !technique_set.contains("emissive") {
        Some("sky")
    } else if words.iter().any(|w| matches!(*w, "tools" | "tool" | "caulk" | "clip" | "portal" | "trigger" | "volume" | "hint" | "occluder")) {
        Some("tools")
    } else if words.iter().any(|w| matches!(*w, "shadowcaster" | "onlycast")) {
        Some("shadow only")
    } else if words.contains(&"distortion") {
        Some("distortion")
    } else {
        None
    }
}

/// A Black Ops III material as Black Ops II's draw: by its technique set's
/// and its own name (Black Ops III's technique names are not Black Ops
/// II's: blended and alpha-tested ones are told by their words), single
/// layered, drawn double-sided for models.
/// The looks the materials step links, in the order it links them: the
/// drawn world surfaces' materials, then each placed model's (first placed
/// first), each material once, with the cover its draw gives (an emissive
/// one, drawn unlit from its emissive map, gets none).
fn looks_wanted(map: &T7Map, hidden: &[bool]) -> Vec<(usize, fastfile_t7::look::Cover)> {
    let world = map.surfaces.iter().enumerate().filter(|(i, _)| !hidden[*i]).filter_map(|(_, s)| Some((s.material?, false)));
    let mut models_seen = std::collections::HashSet::new();
    let models = map
        .static_models
        .iter()
        .filter(|sm| models_seen.insert(sm.model))
        .filter_map(|sm| map.models.get(sm.model))
        .flat_map(|m| m.surfaces.iter().filter_map(|s| Some((s.material?, true))));
    let mut seen = std::collections::HashSet::new();
    world
        .chain(models)
        .filter(|&(i, _)| i < map.materials.len() && seen.insert(i))
        .filter(|&(i, _)| super::t7_look::emissive(map, i).is_none())
        .filter_map(|(i, model)| {
            let m = &map.materials[i];
            Some((i, super::t7_look::Looks::cover(&t7_draw(&m.name, &m.technique_set, model))?))
        })
        .collect()
}

pub(super) fn t7_draw(material: &str, technique_set: &str, model: bool) -> T6Draw {
    let mut draw = T6Draw::from_technique_set(technique_set, 0, None);
    draw.layered = false;
    draw.layers = [0, 0];
    draw.water = false;
    draw.flow = false;
    draw.tile = false;
    draw.flag = false;
    draw.tile_blend = false;
    draw.objective = false;
    // (Black Ops III's technique sets read `mc/lit_decal#<hash>`,
    // `mc/lit_micro_tile_blend`, `mc/vegetation`, `mc/glass`, ...)
    let words: Vec<&str> = material.split(['_', '/', '#']).chain(technique_set.split(['_', '/', '#'])).collect();
    let has = |w: &str| words.contains(&w);
    let tiled = has("tile") || has("tiled");
    if has("decal") || has("dcl") || has("transparent") || has("water") || (has("glass") && !has("opaque")) || (has("blend") && !tiled) {
        draw.blend = T6Blend::Blend;
        if has("decal") || has("dcl") {
            draw.polygon_offset = draw.polygon_offset.max(1);
        }
    } else if has("alphatest") || has("vegetation") || has("foliage") || has("leaf") || has("leaves") || has("grass") || has("chainlink") {
        draw.alpha_test = true;
    }
    if model {
        draw.cull = T6Cull::None;
        draw.foliage = false;
        if draw.blend == T6Blend::Sky {
            draw.blend = T6Blend::Opaque;
        }
    }
    // bo2mp: how it moves: a `flag` technique waves its vertices (a model's
    // or the world's), a world material by the name of a stream, a
    // waterfall or a scroll flows. (Black Ops III's own maps draw natively
    // with their shaders; this is the fallback pass's.)
    if !draw.unlit || draw.anim == asset_core::T6Anim::Cloth {
        draw.anim = draw.anim.or(asset_core::t6_anim_kind(technique_set, material, !model));
    } else {
        draw.anim = asset_core::T6Anim::None;
    }
    if draw.effect || draw.blend == T6Blend::ShadowOnly {
        draw.anim = asset_core::T6Anim::None;
    }
    draw
}

/// Is a picture's format sRGB (a colour map)?
fn is_srgb(format: u32) -> bool {
    matches!(format, 29 | 72 | 75 | 78 | 91 | 93 | 99)
}

/// The colour map of a material: its first sRGB picture, else the first
/// whose name reads as a colour map (`_c`, `_col`), else the first.
fn colour_map(map: &T7Map, textures: &[(u32, String)]) -> Option<(u32, String)> {
    let srgb = textures.iter().find(|(_, n)| map.image_info(n).is_some_and(|i| is_srgb(i.format)));
    let named = || {
        textures.iter().find(|(_, n)| {
            let n = n.to_ascii_lowercase();
            n.ends_with("_c") || n.ends_with("_col") || n.contains("_c_") || n.ends_with("_color")
        })
    };
    srgb.or_else(named).or_else(|| textures.first()).cloned()
}

/// Materials into the catalog: their colour maps' pixels from the zone or
/// the packages.
struct Materials<'a> {
    map: &'a T7Map,
    catalog: MaterialCatalog,
    owner: ZoneOwner,
    max: u32,
    /// (map material, as a model's) -> catalog material.
    linked: HashMap<(usize, bool), Option<usize>>,
    images: HashMap<String, Option<usize>>,
    decoded: usize,
    failures: Vec<String>,
    /// bo2mp: Black Ops III's own look (`super::t7_look`), when on.
    looks: Option<super::t7_look::Looks>,
    /// bo2mp: with Black Ops III's own light, water is drawn as a clear
    /// tinted sheet (`water_sheet`): its colour map (a flow or ramp picture
    /// for BO3's water shader, which this draw has not) read as a colour
    /// drew mp_waterpark's pools in rainbow stripes. The slot, once made.
    water: Option<Option<usize>>,
}

impl Materials<'_> {
    /// The water sheet's picture: 2 x 2 of a pale turquoise (sRGB 95, 170,
    /// 185) at alpha 120 (the pool's floor shows through).
    fn water_sheet(&mut self) -> Option<usize> {
        if let Some(Some(slot)) = self.water {
            return Some(slot);
        }
        use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
        let px: Vec<u8> = [95u8, 170, 185, 120].repeat(4);
        let image = bevy::prelude::Image::new(
            Extent3d { width: 2, height: 2, depth_or_array_layers: 1 },
            TextureDimension::D2,
            px,
            TextureFormat::Rgba8UnormSrgb,
            bevy::asset::RenderAssetUsages::RENDER_WORLD,
        );
        let slot = self.catalog.link_image(AuthoredImage {
            namespace: AssetNamespace::T6,
            name: AssetRef::Real("t7:#water_sheet".to_owned()),
            map_type: 0,
            semantic: TS_COLOR_MAP,
            category: 0,
            use_srgb_reads: true,
            width: 2,
            height: 2,
            depth: 1,
            level_count: 1,
            format: 0,
            payload: Arc::new(Vec::new()),
            decoded: Some(Arc::new(image)),
            common_owned: false,
            decoded_variant: None,
            decoded_by: None,
            pending_decode: None,
        });
        self.water = Some(Some(slot));
        Some(slot)
    }

    fn image(&mut self, name: &str) -> Option<usize> {
        if let Some(slot) = self.images.get(name) {
            return *slot;
        }
        let slot = match self.map.image(name, self.max).and_then(|l| {
            let px = fastfile_iw6::image::ImagePixels { width: l.width, height: l.height, levels: l.levels, dxgi_format: l.format, data: l.data };
            super::iw6::gpu_image(&px, true).map(|img| (px, img))
        }) {
            Ok((px, gpu)) => {
                self.decoded += 1;
                Some(self.catalog.link_image(AuthoredImage {
                    namespace: AssetNamespace::T6,
                    name: AssetRef::Real(format!("t7:{name}#{}", self.max)),
                    map_type: 0,
                    semantic: TS_COLOR_MAP,
                    category: 0,
                    use_srgb_reads: true,
                    width: px.width as u16,
                    height: px.height as u16,
                    depth: 1,
                    level_count: px.levels as u8,
                    format: 0,
                    payload: Arc::new(Vec::new()),
                    decoded: Some(Arc::new(gpu)),
                    common_owned: false,
                    decoded_variant: None,
                    decoded_by: None,
                    pending_decode: None,
                }))
            }
            Err(e) => {
                if self.failures.len() < 64 {
                    self.failures.push(e);
                }
                None
            }
        };
        self.images.insert(name.to_owned(), slot);
        slot
    }

    /// bo2mp: a picture multiplied by a linear colour (an emissive tint),
    /// linked once per picture and tint.
    fn tinted_image(&mut self, name: &str, tint: [f32; 3]) -> Option<usize> {
        self.lifted_image(name, tint, 0.0)
    }

    /// bo2mp: a picture multiplied by a linear colour with `floor` added
    /// (linear), linked once per picture, tint and floor.
    fn lifted_image(&mut self, name: &str, tint: [f32; 3], floor: f32) -> Option<usize> {
        let key = if floor > 0.0 {
            format!("{name}*{:.3},{:.3},{:.3}+{floor:.3}", tint[0], tint[1], tint[2])
        } else {
            format!("{name}*{:.3},{:.3},{:.3}", tint[0], tint[1], tint[2])
        };
        if let Some(slot) = self.images.get(&key) {
            return *slot;
        }
        use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
        let slot = self.map.rgba(name, self.max).ok().map(|c| {
            let mut px = c.px;
            for t in px.chunks_exact_mut(4) {
                for k in 0..3 {
                    let lin = floor + fastfile_t7::look::to_linear(t[k]) * tint[k];
                    t[k] = fastfile_t7::look::to_srgb(lin);
                }
            }
            let image = bevy::prelude::Image::new(
                Extent3d { width: c.w, height: c.h, depth_or_array_layers: 1 },
                TextureDimension::D2,
                px,
                TextureFormat::Rgba8UnormSrgb,
                bevy::asset::RenderAssetUsages::RENDER_WORLD,
            );
            self.decoded += 1;
            self.catalog.link_image(AuthoredImage {
                namespace: AssetNamespace::T6,
                name: AssetRef::Real(format!("t7:{key}")),
                map_type: 0,
                semantic: TS_COLOR_MAP,
                category: 0,
                use_srgb_reads: true,
                width: c.w as u16,
                height: c.h as u16,
                depth: 1,
                level_count: 1,
                format: 0,
                payload: Arc::new(Vec::new()),
                decoded: Some(Arc::new(image)),
                common_owned: false,
                decoded_variant: None,
                decoded_by: None,
                pending_decode: None,
            })
        });
        self.images.insert(key, slot);
        slot
    }

    fn material(&mut self, index: usize, model: bool) -> Option<usize> {
        if let Some(&m) = self.linked.get(&(index, model)) {
            return m;
        }
        // (A map drawn natively, `render_jup`, keeps no converted materials.)
        if index >= self.map.materials.len() {
            return None;
        }
        let m = &self.map.materials[index];
        let mut draw = t7_draw(&m.name, &m.technique_set, model);
        let mut textures = Vec::new();
        // bo2mp: an emissive surface drawn unlit from its emissive map
        // (`super::t7_look::emissive`).
        let glow = if self.looks.is_some() { super::t7_look::emissive(self.map, index) } else { None };
        // (Its emissive tint, when not white: Nuk3town's blue fence
        // patterns, drawn white without it.)
        let tint = {
            let m = &self.map.materials[index];
            m.constant::<3>("emissiveTint1").filter(|t| t.iter().all(|c| c.is_finite() && *c >= 0.0) && t.iter().any(|c| (c - 1.0).abs() > 0.02))
        };
        // A transparent emissive is added over what is behind it: at its
        // full HDR scale Nuk3town's fence pattern drew a cyan glare over
        // the vista where the game shows a faint grid; kept at a third.
        let added = {
            let ts = &self.map.materials[index].technique_set;
            ts.contains("transparent") || ts.contains("decal_emissive")
        };
        // (A sky dome glows far over that: its picture is dark, its lines
        // about sRGB 41 (Nuk3town's hex grid; BO3 lifts it by `hdrScale`
        // 15000, relative to the sky): eight times, sRGB about 117.)
        let sky_dome = self.map.materials[index].technique_set.contains("_sky");
        let tint = match (tint, added) {
            (t, true) if sky_dome => Some(t.unwrap_or([1.0; 3]).map(|c| c * SKY_DOME_GLOW)),
            (Some(t), true) => Some(t.map(|c| c * ADDED_GLOW)),
            (None, true) => Some([ADDED_GLOW; 3]),
            (t, false) => t,
        };
        // (A sky dome's emissive picture at full size: Nuk3town's hex grid
        // over the sky, a 4096 x 2048 picture of thin lines, vanished at
        // the colour maps' size.)
        let sky = self.map.materials[index].technique_set.contains("_sky");
        let max = self.max;
        if sky {
            self.max = 4096;
        }
        let glow_slot = glow.as_ref().and_then(|(name, _)| match tint {
            Some(t) => self.tinted_image(name, t),
            None => self.image(name),
        });
        self.max = max;
        // bo2mp: a moving screen (Black Ops III's `_scroll` emissives): a
        // flipbook (`rowCount` x `columnCount` frames, `imageTime` seconds
        // a frame) or a scroll (`uvScroll`), drawn as Black Ops II's
        // scrolling sheet on a placed model (`sheet`, its constants below).
        let mut sheet_constants: Vec<asset_material::MaterialConstant> = Vec::new();
        if model && glow_slot.is_some() && self.map.materials[index].technique_set.contains("scroll") {
            let m = &self.map.materials[index];
            let one = |n: &str| m.constant::<1>(n).map(|v| v[0]);
            let rows = one("rowCount").unwrap_or(0.0).round();
            let cols = one("columnCount").unwrap_or(0.0).round();
            let time = one("imageTime").unwrap_or(0.0);
            let scroll = m.constant::<2>("uvScroll").unwrap_or([0.0; 2]);
            let hdr = 2f32.powi(i32::from(glow.as_ref().map_or(0, |g| g.1)));
            let constant = |name: &str, v: [f32; 4]| {
                let mut n = [0u8; 12];
                n[..name.len().min(12)].copy_from_slice(&name.as_bytes()[..name.len().min(12)]);
                asset_material::MaterialConstant { name_hash: 0, name: n, literal: v }
            };
            let flip = rows * cols > 1.0 && time > 0.0;
            if flip || scroll.iter().any(|v| v.abs() > 1e-5) {
                sheet_constants = vec![
                    constant("FadeFactor", [0.0; 4]),
                    constant("ColorSpeed", if flip { [cols.max(1.0), 1.0 / time, 0.0, 0.0] } else { [scroll[0], scroll[1], 0.0, 0.0] }),
                    // (Negative: a flipbook of that many rows.)
                    constant("ColorScale", [if flip { -rows } else { 1.0 }; 4]),
                    constant("hdrAmount", [hdr.sqrt(); 4]),
                ];
                draw.sheet = true;
            }
        }
        if let (Some(slot), Some((_, scale))) = (glow_slot, glow) {
            // A transparent emissive (screens' graphics, the fence's
            // pattern, holograms) glows over what is behind it: added.
            if added {
                draw.blend = T6Blend::Add;
            }
            draw.unlit = true;
            draw.unlit_scale_exp = if added { 0 } else { scale };
            if let Some(looks) = self.looks.as_mut() {
                looks.stats.emissive += 1;
            }
            textures.push(MaterialTextureBinding { name_hash: 0xa0ab_1041, name_start: 0, name_end: 0, sampler_state: 0x13, semantic: TS_COLOR_MAP, image: Some(slot) });
        }
        // bo2mp: Black Ops III's look (`super::t7_look`): its colour map as
        // its G-buffer makes it, its normal and specular maps.
        let look = match (self.looks.as_mut(), super::t7_look::Looks::cover(&draw)) {
            (Some(looks), Some(cover)) => looks.link(self.map, &mut self.catalog, index, cover),
            _ => [None; 3],
        };
        let m = &self.map.materials[index];
        let glowing = !textures.is_empty();
        let water = self.water.is_some() && !glowing && {
            let words: Vec<&str> = m.name.split(['_', '/', '#']).chain(m.technique_set.split(['_', '/', '#'])).collect();
            words.contains(&"water")
        };
        if let Some(slot) = water.then(|| self.water_sheet()).flatten() {
            textures.push(MaterialTextureBinding { name_hash: 0xa0ab_1041, name_start: 0, name_end: 0, sampler_state: 0x13, semantic: TS_COLOR_MAP, image: Some(slot) });
        } else if let Some(slot) = look[0].filter(|_| !glowing) {
            textures.push(MaterialTextureBinding { name_hash: 0xa0ab_1041, name_start: 0, name_end: 0, sampler_state: 0x13, semantic: TS_COLOR_MAP, image: Some(slot) });
        } else if !glowing
            && let Some((hash, name)) = colour_map(self.map, &m.textures)
            && let Some(slot) = self.image(&name)
        {
            textures.push(MaterialTextureBinding { name_hash: hash, name_start: 0, name_end: 0, sampler_state: 0x13, semantic: TS_COLOR_MAP, image: Some(slot) });
        }
        textures.extend(super::t7_look::Looks::bindings(&look));
        let m = &self.map.materials[index];
        let index_in_catalog = self.catalog.link_material(AuthoredMaterial {
            name: AssetRef::Real(format!("t7:{}{}", m.name, if model { "#m" } else { "" })),
            namespace: AssetNamespace::T6,
            technique_set: AssetRef::default(),
            technique_set_edge: AssetEdge::Absent,
            draw_surf: 0,
            sort_key: draw.sort_key,
            info_game_flags: 0,
            texture_atlas: None,
            surface_type_bits: None,
            t5_layered_surface_types: None,
            state_flags: 0,
            camera_region: 0,
            state_bits: Vec::new(),
            state_bits_entry: None,
            t5_state_bits_entry: None,
            iw5_state_bits_entry: None,
            technique_table: None,
            route: None,
            textures,
            constants: sheet_constants,
            zone: self.owner,
            t6_draw: Some(draw),
        });
        self.linked.insert((index, model), Some(index_in_catalog));
        Some(index_in_catalog)
    }
}

impl Materials<'_> {
    /// bo2mp: the sun's sprite and lens flare for a Black Ops III map (its
    /// own flare is in its post shaders, not read): a soft warm disc and a
    /// wide warm glow with a ghost ring, as RGBA8 pictures made here, each
    /// in an unlit added material. Their catalog indices.
    fn sun_materials(&mut self) -> Option<(usize, usize)> {
        use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
        let n = 128u32;
        let make = |ring: bool| -> Vec<u8> {
            let mut px = vec![0u8; (n * n * 4) as usize];
            for y in 0..n {
                for x in 0..n {
                    let dx = (x as f32 + 0.5) / n as f32 * 2.0 - 1.0;
                    let dy = (y as f32 + 0.5) / n as f32 * 2.0 - 1.0;
                    let r = (dx * dx + dy * dy).sqrt();
                    let (c, a) = if ring {
                        let glow = (1.0 - r).max(0.0).powf(2.2);
                        let ghost = (-((r - 0.62) / 0.05).powi(2)).exp() * 0.35;
                        ([1.0, 0.72, 0.42], (glow + ghost).min(1.0))
                    } else {
                        let disc = (-(r / 0.28).powi(2)).exp();
                        ([1.0, 0.94, 0.82], disc)
                    };
                    let o = ((y * n + x) * 4) as usize;
                    px[o..o + 4].copy_from_slice(&[(c[0] * 255.0) as u8, (c[1] * 255.0) as u8, (c[2] * 255.0) as u8, (a * 255.0).round() as u8]);
                }
            }
            px
        };
        let mut link = |name: &str, px: Vec<u8>| -> Option<usize> {
            let image = bevy::prelude::Image::new(
                Extent3d { width: n, height: n, depth_or_array_layers: 1 },
                TextureDimension::D2,
                px,
                TextureFormat::Rgba8UnormSrgb,
                bevy::asset::RenderAssetUsages::RENDER_WORLD,
            );
            let slot = self.catalog.link_image(AuthoredImage {
                namespace: AssetNamespace::T6,
                name: AssetRef::Real(format!("t7:#{name}")),
                map_type: 0,
                semantic: TS_COLOR_MAP,
                category: 0,
                use_srgb_reads: true,
                width: n as u16,
                height: n as u16,
                depth: 1,
                level_count: 1,
                format: 0,
                payload: Arc::new(Vec::new()),
                decoded: Some(Arc::new(image)),
                common_owned: false,
                decoded_variant: None,
                decoded_by: None,
                pending_decode: None,
            });
            // (The draw an added emissive world surface has, whose pipeline
            // the world warms anyway: a draw of its own left the world's
            // pipeline wait never done, the map never shown.)
            let mut draw = t7_draw(name, "wc/lit_emissive_transparent", false);
            draw.blend = T6Blend::Add;
            draw.unlit = true;
            draw.unlit_scale_exp = 0;
            Some(self.catalog.link_material(AuthoredMaterial {
                name: AssetRef::Real(format!("t7:#{name}")),
                namespace: AssetNamespace::T6,
                technique_set: AssetRef::default(),
                technique_set_edge: AssetEdge::Absent,
                draw_surf: 0,
                sort_key: draw.sort_key,
                info_game_flags: 0,
                texture_atlas: None,
                surface_type_bits: None,
                t5_layered_surface_types: None,
                state_flags: 0,
                camera_region: 0,
                state_bits: Vec::new(),
                state_bits_entry: None,
                t5_state_bits_entry: None,
                iw5_state_bits_entry: None,
                technique_table: None,
                route: None,
                textures: vec![MaterialTextureBinding { name_hash: 0xa0ab_1041, name_start: 0, name_end: 0, sampler_state: 0x13, semantic: TS_COLOR_MAP, image: Some(slot) }],
                constants: Vec::new(),
                zone: self.owner,
                t6_draw: Some(draw),
            }))
        };
        Some((link("sun_sprite", make(false))?, link("sun_flare", make(true))?))
    }

    /// bo2mp: an effect sprite's material (`super::t7_fx`), linked as
    /// `t7fx:<name>` with `draw` and its colour map: its catalog name, or
    /// `None` when the picture is not there.
    fn effect_material(&mut self, name: &str, draw: T6Draw, image: &str, glow: bool) -> Option<String> {
        let slot = if glow { self.lifted_image(image, [EFFECT_GLOW; 3], EFFECT_GLOW_FLOOR)? } else { self.image(image)? };
        let catalog_name = format!("t7fx:{name}");
        self.catalog.link_material(AuthoredMaterial {
            name: AssetRef::Real(catalog_name.clone()),
            namespace: AssetNamespace::T6,
            technique_set: AssetRef::default(),
            technique_set_edge: AssetEdge::Absent,
            draw_surf: 0,
            sort_key: draw.sort_key,
            info_game_flags: 0,
            texture_atlas: None,
            surface_type_bits: None,
            t5_layered_surface_types: None,
            state_flags: 0,
            camera_region: 0,
            state_bits: Vec::new(),
            state_bits_entry: None,
            t5_state_bits_entry: None,
            iw5_state_bits_entry: None,
            technique_table: None,
            route: None,
            textures: vec![MaterialTextureBinding { name_hash: 0xa0ab_1041, name_start: 0, name_end: 0, sampler_state: 0x13, semantic: TS_COLOR_MAP, image: Some(slot) }],
            constants: Vec::new(),
            zone: self.owner,
            t6_draw: Some(draw),
        });
        Some(catalog_name)
    }
}

/// bo2mp: Black Ops III's effects without a map (a guest gun's stand-ins,
/// `super::fx_standin`): what `choose` picks out of the shared zones'
/// effect names, and the effects of the impact tables it names.
pub(super) struct SharedFx {
    /// The effects chosen and every effect they run, as the catalog takes them.
    pub views: Vec<asset_game::T7FxView>,
    /// Per impact table asked for (`bullet_small_enter`): its 31 non-flesh
    /// cells and 4 flesh cells (empty names: none).
    pub impacts: Vec<(String, Vec<String>, Vec<String>)>,
    pub material_seed: MaterialCatalog,
    pub report: Vec<String>,
}

/// What `shared_effects` asks of its caller: given every effect name the
/// shared zones define, the effects wanted and the impact tables wanted.
pub(super) type ChooseFx<'a> = dyn FnOnce(&[String]) -> (Vec<String>, Vec<String>) + 'a;

pub(super) fn shared_effects(dir: &Path, material_seed: MaterialCatalog, choose: Box<ChooseFx>) -> SharedFx {
    let started = std::time::Instant::now();
    let Some(map) = T7Map::shared_only(dir) else {
        return SharedFx {
            views: Vec::new(),
            impacts: Vec::new(),
            material_seed,
            report: vec![format!("t7 stand-in effects: no core_common.ff in {}", dir.display())],
        };
    };
    let names = map.effect_names();
    let (mut wanted, tables) = choose(&names);
    let mut report = Vec::new();
    let mut impacts = Vec::new();
    for table in &tables {
        let Some((surf, flesh)) = map.impact_effects(table) else {
            report.push(format!("t7 stand-in effects: no impact table {table}"));
            continue;
        };
        let surf: Vec<String> = surf.into_iter().take(31).map(Option::unwrap_or_default).collect();
        let flesh: Vec<String> = flesh.into_iter().map(Option::unwrap_or_default).collect();
        wanted.extend(surf.iter().chain(&flesh).filter(|n| !n.is_empty()).cloned());
        impacts.push((table.clone(), surf, flesh));
    }
    wanted.sort();
    wanted.dedup();
    let mut catalog = material_seed;
    catalog.set_capture_ns(AssetNamespace::T6);
    let mut b = Materials {
        map: &map,
        catalog,
        owner: ZoneOwner::from_zone_path(&dir.join("core_common.ff")),
        max: texture_max(),
        linked: HashMap::new(),
        images: HashMap::new(),
        decoded: 0,
        failures: Vec::new(),
        looks: None,
        water: None,
    };
    let views = super::t7_fx::effects(&map, &[], &wanted, &mut |name, draw, image, glow| b.effect_material(name, draw, image, glow), &mut report);
    report.push(format!(
        "t7 stand-in effects: {} of {} wanted (+ the effects they run), {} pictures decoded, {} not [{:.1} s]",
        views.len(),
        wanted.len(),
        b.decoded,
        b.failures.len(),
        started.elapsed().as_secs_f64()
    ));
    for f in b.failures.iter().take(4) {
        report.push(format!("t7 stand-in effects: picture: {f}"));
    }
    SharedFx { views, impacts, material_seed: b.catalog, report }
}

/// An emissive-blend effect picture's brightening (linear): its grey
/// bodies (about 0.3) glow near white, so the element's colour shows.
const EFFECT_GLOW: f32 = 8.0;

/// What an emissive-blend effect picture's darkest covered texel glows at
/// (linear): the voxels' black cells inside the shape glow too (blended
/// from black they drew Nuk3town's data streams rust grey).
const EFFECT_GLOW_FLOOR: f32 = 0.6;

/// How far a sky dome is drawn (see the placed models).
const SKY_DOME_RADIUS: f32 = 12000.0;

/// A sky dome's added glow (see `Materials::material`).
const SKY_DOME_GLOW: f32 = 8.0;

/// What a transparent emissive's light is kept at when added (see
/// `Materials::material`).
const ADDED_GLOW: f32 = 0.33;

/// The sun: the zone's sun volume (pitch and yaw toward the sun, colour,
/// intensity), else the worldspawn's `sundirection`/`suncolor`.
fn sun_of(map: &T7Map, entities: &[iw3::ents::Entity]) -> ([f32; 3], [f32; 3], f32) {
    if let Some(s) = map.sun
        && s.pitch.is_finite()
        && s.yaw.is_finite()
        && s.color.iter().all(|c| c.is_finite() && *c >= 0.0)
        && s.color.iter().any(|c| *c > 0.0)
    {
        let peak = s.color.iter().copied().fold(0.0f32, f32::max).max(1e-3);
        let colour = s.color.map(|c| (c / peak).clamp(0.0, 1.0));
        // (Black Ops III's sun intensity is in its HDR units.)
        let light = if s.intensity.is_finite() && s.intensity > 0.0 { (s.intensity / 5.0).clamp(0.6, 2.0) } else { 1.0 };
        return ([-s.pitch.abs(), s.yaw, 0.0], colour, light);
    }
    let world = entities.iter().find(|e| e.classname() == "worldspawn");
    let vec3 = |k: &str| -> Option<[f32; 3]> {
        let v: Vec<f32> = world?.get(k)?.split_whitespace().filter_map(|x| x.parse().ok()).collect();
        (v.len() == 3).then(|| [v[0], v[1], v[2]])
    };
    (vec3("sundirection").unwrap_or([-60.0, 45.0, 0.0]), vec3("suncolor").unwrap_or([1.0; 3]), 1.0)
}

/// bo2mp: the light levels of Black Ops III's look on a map whose light was
/// baked into vertex colours (`T7Map::bo3_look`), against the baked light's
/// (`baked_ambient`, CoD4's gamma-space page): the scale that brings the
/// page's linear luminance to `SHADE` (as `h1_map`'s look: shaded texels at
/// 0.4), and the sun's light over its colour.
pub(super) const LOOK_SHADE: f32 = 0.4;
pub(super) const LOOK_SUN: f32 = 2.6;

pub(super) fn look_scale(ambient: [f32; 3]) -> f32 {
    let l = 0.2126 * ambient[0] * ambient[0] + 0.7152 * ambient[1] * ambient[1] + 0.0722 * ambient[2] * ambient[2];
    (LOOK_SHADE / l.max(1e-4)).clamp(0.05, 64.0)
}

/// bo2mp: one Call of Duty 4 lightmap of a single light everywhere: page A
/// the ambient light (gamma space), its dominant direction straight along
/// the normal (both alphas the middle), page B none, the sun's baked
/// visibility whole (the vertex colours carry the shadows).
fn uniform_lightmap(ambient: [f32; 3]) -> Result<asset_world::WorldLightmap, asset_world::WorldLightmapGap> {
    use asset_world::world_iw3::{IW3_LIGHTMAP_PRIMARY_FORMAT, IW3_LIGHTMAP_SECONDARY_FORMAT, iw3_lightmap};
    const SIZE: u16 = 4;
    let byte = |c: f32| (c.clamp(0.0, 1.0) * 255.0).round() as u8;
    let texels = usize::from(SIZE) * usize::from(SIZE);
    // A8R8G8B8 as stored: B G R A; x = A.a * 4.08 - 2.08 and y = B.a *
    // 4.064516 - 2.064516 are 0 at alpha 130.
    let mut secondary = Vec::with_capacity(texels * 2 * 4);
    for _ in 0..texels {
        secondary.extend_from_slice(&[byte(ambient[2]), byte(ambient[1]), byte(ambient[0]), 130]);
    }
    for _ in 0..texels {
        secondary.extend_from_slice(&[0, 0, 0, 130]);
    }
    let primary = vec![255u8; texels];
    iw3_lightmap(
        (SIZE, SIZE, IW3_LIGHTMAP_PRIMARY_FORMAT, &primary),
        (SIZE, SIZE * 2, IW3_LIGHTMAP_SECONDARY_FORMAT, &secondary),
        ("bo2mp_baked_sun", "bo2mp_baked_ambient"),
    )
}

/// The world as CoD4's `GfxWorld` (materials beside it, not in it).
fn iw3_world(map: &T7Map, sun: ([f32; 3], [f32; 3], f32), probes: &dyn Fn(usize) -> u8) -> iw3::zone::GfxWorld {
    iw3::zone::GfxWorld {
        name: map.name.clone(),
        base_name: map.name.clone(),
        indices: map.indices.clone(),
        vertices: map
            .vertices
            .iter()
            .map(|v| iw3::zone::WorldVertex {
                xyz: v.xyz,
                binormal_sign: 1.0,
                color: v.color,
                tex_coord: v.uv,
                lmap_coord: [0.0, 0.0],
                normal: iw3_unit_vec(v.normal),
                tangent: iw3_unit_vec(v.tangent),
            })
            .collect(),
        surfaces: map
            .surfaces
            .iter()
            .enumerate()
            .map(|(i, s)| iw3::zone::GfxSurface {
                first_vertex: s.first_vertex as i32,
                vertex_count: s.vertex_count.min(u32::from(u16::MAX)) as u16,
                tri_count: s.tri_count,
                base_index: s.base_index as i32,
                material: None,
                lightmap_index: 0,
                reflection_probe_index: probes(i),
                primary_light_index: 0,
                flags: s.flags,
                bounds: [s.mins, s.maxs],
            })
            .collect(),
        static_models: Vec::new(),
        sky_start_surfs: Vec::new(),
        sky_image: None,
        sun: iw3::zone::SunParse {
            name: String::new(),
            ambient_scale: 0.0,
            ambient_color: [0.0; 3],
            diffuse_fraction: 0.0,
            sun_light: sun.2,
            sun_color: sun.1,
            diffuse_color: sun.1,
            angles: sun.0,
        },
        sun_flare: Default::default(),
        sun_color_from_bsp: sun.1,
        lightmaps: Vec::new(),
        reflection_probes: Vec::new(),
        mins: map.mins,
        maxs: map.maxs,
        lit_surfs: 0..0,
        decal_surfs: 0..0,
        emissive_surfs: 0..0,
        models: Vec::new(),
        light_grid: Default::default(),
        dpvs: None,
    }
}

/// The spawn points in the map's entities (`mp_*_spawn*`, the player
/// starts, Zombies' `initial_spawn_points` structs).
fn spawns_of(entities: &[iw3::ents::Entity]) -> Vec<asset_world::SpawnPoint> {
    entities
        .iter()
        .filter(|e| {
            let c = e.classname();
            c.starts_with("mp_") && c.contains("spawn")
                || c == "info_player_start"
                || e.get("targetname") == Some("initial_spawn_points")
                || e.get("targetname") == Some("player_respawn_point")
        })
        .filter_map(|e| {
            Some(asset_world::SpawnPoint {
                classname: e.classname().to_owned(),
                origin: e.origin()?,
                angles: e.angles(),
                script_linkto: e.get("script_linkto").unwrap_or_default().to_owned(),
                script_destructable_area: e.get("script_destructable_area").unwrap_or_default().to_owned(),
            })
        })
        .collect()
}

/// The origins of the spawn points in an entity text.
pub(super) fn spawn_origins(entities: &str) -> Vec<[f32; 3]> {
    spawns_of(&iw3::ents::parse(entities)).iter().map(|s| s.origin).collect()
}

/// What material `m` does to the sun's light (the sun map's rule).
fn light_blocks(map: &T7Map, m: usize) -> fastfile_t7::light::Blocks {
    let Some(mat) = map.materials.get(m) else {
        return fastfile_t7::light::Blocks::No;
    };
    if hidden_kind(&mat.name, &mat.technique_set).is_some() {
        return fastfile_t7::light::Blocks::No;
    }
    let d = t7_draw(&mat.name, &mat.technique_set, false);
    if d.blend != T6Blend::Opaque || d.unlit {
        fastfile_t7::light::Blocks::No
    } else if d.alpha_test {
        fastfile_t7::light::Blocks::Partly
    } else {
        fastfile_t7::light::Blocks::Fully
    }
}

/// bo2mp (prepare all maps): a Black Ops III map's sun map made now into
/// its finished file (`map.pak`), as a load drawn by the native renderer
/// writes it (`t7_light_cache`), so even the map's first load reads it
/// back. Left alone when it is already there and current, when the map
/// brings its own light, or when the settings mean a load would not keep it
/// (the native renderer off, the tracer on, the light or its cache off).
/// `Ok`: one line on how it went.
pub(crate) fn sun_map_ahead(zone: &Path, map: &T7Map) -> Result<String, String> {
    let native = frame::settings::T7_NATIVE_RENDER.load(std::sync::atomic::Ordering::Relaxed) && !std::env::var("BO2MP_T7_NATIVE").is_ok_and(|v| v == "0");
    if !native || super::t7_light::native_tracer() || !super::t7_light::on() || !super::t7_light_cache::on() {
        return Ok(format!("sun map: {} not kept on disk with these settings", map.name));
    }
    if map.baked_ambient.is_some() || map.sky_cube.is_some() {
        return Ok(format!("sun map: {} has its own light", map.name));
    }
    let spawns = spawn_origins(&map.entities);
    let blocks = |m: usize| light_blocks(map, m);
    let slot = super::t7_light_cache::Slot::new(zone, map, &spawns, &blocks).ok_or("the bake is off or has nowhere to go")?;
    if slot.read().is_some() {
        return Ok(format!("sun map: {} already made", map.name));
    }
    let read = super::t7_light::Lit::read(map)?;
    let job = map_bake::status::begin("t7", &map.name, "sun map", 1);
    let t = std::time::Instant::now();
    let _ = read.finish(zone, map, &spawns, &blocks, false);
    job.add(1);
    let made = slot.read().is_some();
    job.end(made);
    if made {
        Ok(format!("sun map: {} made in {:.1}s", map.name, t.elapsed().as_secs_f32()))
    } else {
        Err(format!("sun map: {} not written to {}", map.name, slot.path().display()))
    }
}

/// Loads a Black Ops III map zone into the match's world. A zone that
/// cannot be read becomes a gap with the reason, never a crash.
pub(crate) fn load_t7_world(path: &Path, progress: &LoadProgress, material_seed: MaterialCatalog) -> LoadedWorld {
    load_t7_world_for(path, progress, material_seed, false)
}

/// As [`load_t7_world`]; `native`: the map is drawn by `render_t7` (the way
/// BlackOps3.exe draws it), so what only the converted world draws is not
/// built at all (its surface and prop materials, pictures and meshes, the
/// vertex lighting, the sun sprite), and the map read is handed on to the
/// renderer's scene read (`fastfile_t7::map::hand_over`) instead of being
/// read twice. Kept: collision, spawns, entities, scripts, effects, audio,
/// the map's light for players and the gun.
pub(crate) fn load_t7_world_for(path: &Path, progress: &LoadProgress, material_seed: MaterialCatalog, native: bool) -> LoadedWorld {
    let mut report = vec![format!("game: T7 / Black Ops III ({})", path.display())];
    let stage = progress.begin_scoped(StageId::MapAssets, "black ops iii zone", None);
    let started = std::time::Instant::now();
    // bo2mp: a Steam Workshop map or a usermap (its zone and `.xpak` in a
    // folder of their own): the shared zones and packages are BO3's own
    // `zone` folder's.
    if !path.parent().is_some_and(|d| d.join("core_common.ff").is_file()) {
        let game = asset_transport::workshop::bo3_zone_dir()
            .filter(|d| d.join("core_common.ff").is_file())
            .or_else(|| super::t7::bo3_zone_dir().ok());
        fastfile_t7::map::set_game_zone_dir(game.clone());
        let line = format!(
            "t7 map outside BO3's zone folder ({}): its own packages from {}, shared zones from {}",
            asset_transport::workshop::bo3_map(&path.file_stem().map(|s| s.to_string_lossy().to_ascii_lowercase()).unwrap_or_default())
                .map_or_else(|| "not a known Workshop map".to_owned(), |m| format!("'{}', {:?}", m.title, m.source)),
            path.parent().map_or_else(String::new, |d| d.display().to_string()),
            game.map_or_else(|| "nowhere (BO3's zone folder not found)".to_owned(), |d| d.display().to_string())
        );
        diag::info!(World, "bo2mp {line}");
        report.push(line);
    }
    let read = std::panic::catch_unwind(|| T7Map::load(&MapSources::beside(path)));
    if std::env::var_os("BO2MP_T7_TIMING").is_some() {
        eprintln!("t7 load timing: T7Map::load: {:.2}s", started.elapsed().as_secs_f32());
    }
    let map = match read {
        Ok(Ok(map)) => {
            stage.done();
            map
        }
        Ok(Err(error)) => {
            stage.fail();
            native_gave_up(path, native);
            diag::warn!(World, "bo2mp t7: {}: {error}", path.display());
            // bo2mp: a Steam Workshop map or usermap that does not read says
            // so in one line, with its title and item.
            let stem = path.file_stem().map(|s| s.to_string_lossy().to_ascii_lowercase()).unwrap_or_default();
            if let Some(m) = asset_transport::workshop::bo3_map(&stem) {
                diag::warn!(
                    World,
                    "bo3 workshop: map {} '{}' ({}) could not be loaded: {error}",
                    m.stem,
                    m.title,
                    if m.source == asset_transport::workshop::MapSource::Workshop { format!("Workshop item {}", m.id) } else { "usermaps".to_owned() }
                );
            }
            return LoadedWorld::with_gap(
                WorldDrawPolicy::t5(),
                PreparedCapability::PreparedWorld,
                format!("Black Ops III zone: {error}"),
                Some("assets::lane::t7_map::load_world/zone"),
            );
        }
        Err(panic) => {
            stage.fail();
            native_gave_up(path, native);
            let what = panic
                .downcast_ref::<&str>()
                .map(|s| (*s).to_owned())
                .or_else(|| panic.downcast_ref::<String>().cloned())
                .unwrap_or_else(|| "unknown error".to_owned());
            diag::warn!(World, "bo2mp t7: {}: reader panicked: {what}", path.display());
            return LoadedWorld::with_gap(
                WorldDrawPolicy::t5(),
                PreparedCapability::PreparedWorld,
                format!("Black Ops III zone could not be read: {what}"),
                Some("assets::lane::t7_map::load_world/panic"),
            );
        }
    };
    report.push(format!("t7 zone: read in {:.1}s", started.elapsed().as_secs_f32()));
    report.extend(map.report.iter().map(|l| format!("t7 {l}")));
    let loaded = build_t7_world(path, &map, report, progress, material_seed, None, native);
    // bo2mp: the map read once, kept for the native renderer's scene so it
    // does not read the zone again.
    if native {
        fastfile_t7::map::hand_over(path, map);
    }
    diag::info!(
        World,
        "bo2mp t7: {} loaded: {}",
        path.display(),
        loaded.report.iter().filter(|l| l.starts_with("t7 ") || l.starts_with("world mesh")).cloned().collect::<Vec<_>>().join("; ")
    );
    loaded
}

/// The native renderer has no map to wait for (the load failed).
fn native_gave_up(path: &Path, native: bool) {
    if native {
        fastfile_t7::map::cancel_handed(path);
        frame::settings::t7_native_load::set(frame::settings::t7_native_load::FAILED);
    }
}

/// A model's bounds and radius about its origin.
fn model_bounds(m: &fastfile_t7::map::MapModel) -> ([f32; 3], [f32; 3], f32) {
    let mut lo = [f32::MAX; 3];
    let mut hi = [f32::MIN; 3];
    let mut r2 = 0.0f32;
    for s in &m.surfaces {
        for p in &s.positions {
            for k in 0..3 {
                lo[k] = lo[k].min(p[k]);
                hi[k] = hi[k].max(p[k]);
            }
            r2 = r2.max(p[0] * p[0] + p[1] * p[1] + p[2] * p[2]);
        }
    }
    if lo[0] > hi[0] {
        return ([0.0; 3], [0.0; 3], 0.0);
    }
    (lo, hi, r2.sqrt())
}

/// Toward the sun from its angles (pitch, yaw in degrees).
fn sun_toward(angles: [f32; 3]) -> [f32; 3] {
    let (p, y) = (angles[0].to_radians(), angles[1].to_radians());
    // (The sun's angles point the light's way: toward the sun is back.)
    [-(p.cos() * y.cos()), -(p.cos() * y.sin()), p.sin()]
}

/// The map's scene entities (`scriptbundle_scene`): bundle, origin, angles.
fn scene_bundles(entities: &str) -> Vec<(String, String, String)> {
    let mut out = Vec::new();
    for block in entities.split('}') {
        let value = |key: &str| {
            let at = block.find(&format!("\"{key}\" \""))? + key.len() + 4;
            let rest = &block[at..];
            Some(rest[..rest.find('"')?].to_owned())
        };
        // (The fxanim scenes: the end-game ones pose players.)
        if value("classname").as_deref() == Some("scriptbundle_scene")
            && let Some(bundle) = value("scriptbundlename").filter(|b| b.contains("fxanim"))
        {
            out.push((bundle, value("origin").unwrap_or_else(|| "0 0 0".into()), value("angles").unwrap_or_else(|| "0 0 0".into())));
        }
    }
    out
}

/// The models the map's own script models show (`"classname"
/// "script_model"`: Nuk3town's destructible mannequins and cars), once each.
/// bo2mp: the AI types of the map's actor spawners (`actor_spawner_zm_zod_zombie`
/// spawns the AI type `spawner_zm_zod_zombie`).
fn actor_types(entities: &str) -> Vec<String> {
    let mut out: Vec<String> = entities
        .split('{')
        .filter_map(|b| {
            let at = b.find("\"classname\" \"actor_spawner_")? + 19;
            let r = &b[at..];
            Some(r[..r.find('"')?].to_owned())
        })
        .collect();
    out.sort();
    out.dedup();
    out
}

fn plain_script_models(entities: &str) -> Vec<String> {
    let mut out: Vec<String> = entities
        .split('{')
        .filter(|b| b.contains("\"classname\" \"script_model\""))
        .filter_map(|b| {
            let at = b.find("\"model\" \"")? + 9;
            let r = &b[at..];
            let m = &r[..r.find('"')?];
            (!m.is_empty() && !m.starts_with('*')).then(|| m.to_owned())
        })
        .collect();
    out.sort();
    out.dedup();
    out
}

/// The entity text without the `script_brushmodel`s that script models
/// showing an unbuilt model (`unbuilt`) target (their clip); and how many.
fn without_clips_of(entities: &str, unbuilt: &dyn Fn(&str) -> bool) -> (String, usize) {
    let value = |block: &str, key: &str| -> Option<String> {
        let at = block.find(&format!("\"{key}\" \""))? + key.len() + 4;
        let r = &block[at..];
        Some(r[..r.find('"')?].to_owned())
    };
    let targets: std::collections::HashSet<String> = entities
        .split('{')
        .filter(|b| b.contains("\"classname\" \"script_model\""))
        .filter(|b| value(b, "model").is_some_and(|m| unbuilt(&m)))
        .filter_map(|b| value(b, "target"))
        .collect();
    let mut out = String::with_capacity(entities.len());
    let mut dropped = 0;
    let mut rest = entities;
    while let Some(open) = rest.find('{') {
        out.push_str(&rest[..open]);
        let Some(close) = rest[open..].find('}') else {
            out.push_str(&rest[open..]);
            return (out, dropped);
        };
        let block = &rest[open..open + close + 1];
        let clip = block.contains("\"classname\" \"script_brushmodel\"") && value(block, "targetname").is_some_and(|t| targets.contains(&t));
        if clip {
            dropped += 1;
        } else {
            out.push_str(block);
        }
        rest = &rest[open + close + 1..];
    }
    out.push_str(rest);
    (out, dropped)
}

/// bo2mp: each `zbarrier_<def>` entity's boards as Black Ops III builds
/// them, from its classname's `ZBarrierDef` in the zones (`def`: the
/// def's fields by offset, `fastfile_t7::walk::world_misc::zbarrier`),
/// not from the entity's own `zbarrier*` keys: a prefab can leave those
/// stale (Shadows of Evil's 15 scissor gates carry a magic box's five
/// boards, so the gates stood as boxes). The entity's keys stay when its
/// def is not in the zones. Returns the text and how many were rebuilt.
pub(crate) fn zbarriers_from_defs(entities: &str, def: &dyn Fn(&str) -> Option<Vec<(usize, String)>>) -> (String, usize) {
    const BOARDS: usize = 6;
    const BOARD: usize = 0x70;
    let mut out = String::with_capacity(entities.len());
    let mut rebuilt = 0;
    for block in entities.split_inclusive('}') {
        const CLASS: &str = "\"classname\" \"zbarrier_";
        let class = block.find(CLASS).and_then(|at| {
            let r = &block[at + CLASS.len()..];
            Some(r[..r.find('"')?].to_ascii_lowercase())
        });
        let Some(fields) = class.as_deref().and_then(def) else {
            out.push_str(block);
            continue;
        };
        let get = |o: usize| fields.iter().find(|f| f.0 == o).map(|f| f.1.as_str());
        let count: usize = get(0x38).and_then(|c| c.parse().ok()).unwrap_or(0).min(BOARDS);
        let mut keys = format!("\"zbarrierNumBoards\" \"{count}\"\n");
        for i in 0..count {
            let b = 0x40 + BOARD * i;
            for (o, key) in [(0x20, "zbarrierboardModel"), (0x30, "zbarrierupgradedBoardModel"), (0x38, "zbarriertearAnim"), (0x40, "zbarrierboardAnim")] {
                if let Some(v) = get(b + o) {
                    keys.push_str(&format!("\"{key}{}\" \"{v}\"\n", i + 1));
                }
            }
        }
        let mut lines: Vec<&str> = block.split_inclusive('\n').collect();
        lines.retain(|l| {
            let k = l.trim_start().trim_start_matches('"').to_ascii_lowercase();
            !(k.starts_with("zbarrierboardmodel")
                || k.starts_with("zbarrierupgradedboardmodel")
                || k.starts_with("zbarriertearanim")
                || k.starts_with("zbarrierboardanim")
                || k.starts_with("zbarriernumboards"))
        });
        // (After the opening brace's line.)
        let at = lines.iter().position(|l| l.contains('{')).map_or(0, |p| p + 1);
        lines.insert(at, &keys);
        out.extend(lines);
        rebuilt += 1;
    }
    (out, rebuilt)
}

/// The entity text without Black Ops III's game-mode clips
/// (`script_brushmodel`s with a `script_gameobjectname`: bomb sites,
/// domination, headquarters, ...).
fn without_bomb_clips(entities: &str) -> String {
    let mut out = String::with_capacity(entities.len());
    let mut rest = entities;
    while let Some(open) = rest.find('{') {
        out.push_str(&rest[..open]);
        let Some(close) = rest[open..].find('}') else {
            out.push_str(&rest[open..]);
            return out;
        };
        let block = &rest[open..open + close + 1];
        // (bo2mp: and every other game-mode brush, `script_gameobjectname`
        // set: BO3's gametype scripts delete those of the modes not being
        // played (Nuk3town's "twar dom hq sd dem" clips stood invisible in
        // every mode here); Black Ops II's own scripts place their own.)
        let bomb = block.contains("\"classname\" \"script_brushmodel\"")
            && block.contains("\"script_gameobjectname\" \"");
        if !bomb {
            out.push_str(block);
        }
        rest = &rest[open + close + 1..];
    }
    out.push_str(rest);
    out
}

fn scene_fpv_has(fpv: &asset_model::FpvMeshBuild, model: &str) -> bool {
    fpv.get(AssetNamespace::T6, model).is_some()
}

/// The Black Ops II map a Black Ops III map remakes on the same
/// coordinates.
fn bo2_layout_of(map: &str) -> Option<&'static str> {
    match map.to_ascii_lowercase().as_str() {
        "mp_nuketown_x" => Some("mp_nuketown_2020"),
        _ => None,
    }
}

/// The collision: the zone's clip map as Black Ops II's tables, with the
/// listed static models' collision triangles as extra triangles. `None` when the
/// zone has no clip map (the drawn world stands in then).
fn collision_of(map: &T7Map, hidden: &[bool], report: &mut Vec<String>) -> asset_world::ClipCollision {
    use asset_world::world_iw7::{IW7_SOLID, SoupTri};
    use asset_world::world_t7::{T7AabbNode, T7Brush, T7ClipInput, T7Cmodel, build_t7_clip_collision};
    let by_name: HashMap<&str, usize> = map.models.iter().enumerate().map(|(i, m)| (m.name.as_str(), i)).collect();
    let mut extra = Vec::new();
    let place = |p: [f32; 3], origin: [f32; 3], axis: [[f32; 3]; 3], scale: f32| -> [f32; 3] {
        std::array::from_fn(|k| origin[k] + (p[0] * axis[0][k] + p[1] * axis[1][k] + p[2] * axis[2][k]) * scale)
    };
    let mut missing = 0usize;
    let mut models_used = 0usize;
    let add_model = |m: &fastfile_t7::map::MapModel, origin: [f32; 3], axis: [[f32; 3]; 3], scale: f32, contents: u32, extra: &mut Vec<SoupTri>| {
        let surfaces = if m.coarse.is_empty() { &m.surfaces } else { &m.coarse };
        for s in surfaces {
            for t in &s.tris {
                let (Some(a), Some(b), Some(c)) = (s.positions.get(usize::from(t[0])), s.positions.get(usize::from(t[1])), s.positions.get(usize::from(t[2]))) else {
                    continue;
                };
                extra.push(SoupTri { p: [place(*a, origin, axis, scale), place(*b, origin, axis, scale), place(*c, origin, axis, scale)], surface_flags: 0, contents });
            }
        }
    };
    let Some(clip) = &map.clip else {
        // No clip map read: the drawn world and every placed model.
        for (i, s) in map.surfaces.iter().enumerate() {
            if hidden.get(i).copied().unwrap_or(false) || map.no_collision.get(i).copied().unwrap_or(false) {
                continue;
            }
            for t in 0..s.tri_count as usize {
                let v = |k: usize| {
                    let ix = map.indices.get(s.base_index as usize + 3 * t + k).copied().unwrap_or(0);
                    map.vertices.get((s.first_vertex + u32::from(ix)) as usize).map_or([0.0; 3], |v| v.xyz)
                };
                extra.push(SoupTri { p: [v(0), v(1), v(2)], surface_flags: 0, contents: IW7_SOLID });
            }
        }
        let radius: Vec<f32> = map.models.iter().map(|m| model_bounds(m).2).collect();
        for (i, sm) in map.static_models.iter().enumerate() {
            if map.static_no_collision.get(i).copied().unwrap_or(false) {
                continue;
            }
            let m = &map.models[sm.model];
            if radius[sm.model] * sm.scale >= COLLIDE_MIN_RADIUS {
                add_model(m, sm.origin, sm.axis, sm.scale, IW7_SOLID, &mut extra);
            }
        }
        report.push(format!("t7 clip: none in the zone; {} triangles from the drawn world and placed models", extra.len()));
        return asset_world::world_iw7::build_soup_clip_collision(&extra, &[]);
    };
    // Each static model the clip map lists collides through its model's
    // collision triangles (`XModelCollSurf`, held in the zone, so the game
    // traces them on a server too), each in its surface's bone posed as the
    // clip map stores it, with that surface's contents and surface flags
    // (foliage 0x2 lets a player through, glass 0x10 does not). Every
    // listed model, however small; a model with no collision surfaces
    // collides with nothing.
    for sm in &clip.static_models {
        let tris = clip.model_collision.get(&sm.name).map(Vec::as_slice).unwrap_or(&[]);
        if tris.is_empty() {
            missing += 1;
            continue;
        }
        models_used += 1;
        for t in tris {
            let bone = usize::try_from(t.bone).ok().and_then(|b| sm.bones.get(b));
            let corner = |p: [f32; 3]| {
                let p = bone.map_or(p, |m| fastfile_t7::map::bone_to_model(m, p));
                place(p, sm.origin, sm.axis, sm.scale)
            };
            let contents = if t.contents != 0 { t.contents } else { sm.contents };
            extra.push(SoupTri { p: t.corners.map(corner), surface_flags: t6_surface_flags(t.surface_flags), contents });
        }
    }
    let _ = (&by_name, &add_model);
    let input = T7ClipInput {
        materials: clip.materials.iter().map(|(n, s, c)| (n.clone(), t6_surface_flags(*s), *c)).collect(),
        brushes: clip
            .brushes
            .iter()
            .map(|b| T7Brush { contents: b.contents, planes: b.planes.clone(), plane_surface_flags: b.plane_surface_flags.iter().map(|&s| t6_surface_flags(s)).collect() })
            .collect(),
        nodes: clip.nodes.iter().map(|n| (n.plane, n.children)).collect(),
        leaf_brushes: clip.leafs.iter().map(|l| l.brushes.clone()).collect(),
        leaf_aabbs: clip.leafs.iter().map(|l| (l.first_coll_aabb, l.coll_aabb_count)).collect(),
        verts: clip.verts.clone(),
        tris: clip.tris.clone(),
        walkable: clip.walkable.clone(),
        partitions: clip.partitions.iter().map(|p| (p.tri_count, p.first_tri)).collect(),
        aabb_trees: clip
            .aabb_trees
            .iter()
            .map(|a| T7AabbNode { origin: a.origin, half_size: a.half_size, material: a.material, child_count: a.child_count, u: a.u })
            .collect(),
        cmodels: clip.models.iter().map(|m| T7Cmodel { mins: m.mins, maxs: m.maxs, radius: m.radius, brushes: m.leaf.brushes.clone() }).collect(),
    };
    let (collision, stats) = build_t7_clip_collision(&input, &extra);
    report.push(format!(
        "t7 clip: {} brushes, {} leaves, {} mesh triangles, {} model collision triangles from {} static models ({} without collision surfaces), {} tree roots gathered, {} partitions out of range",
        stats.brushes, stats.leaves, stats.tris, stats.extra_tris, models_used, missing, stats.roots, stats.bad_partitions
    ));
    report.push(solid_coverage(&collision));
    collision
}

/// bo2mp: how much of the collision stops a player (`MASK_PLAYERSOLID`, the
/// mask Black Ops III's player trace uses as Black Ops II's does) and how
/// much only bullets, vehicles or AI (a floor of those is walked through):
/// brushes and triangles, and the commonest contents of the rest.
fn solid_coverage(c: &asset_world::ClipCollision) -> String {
    const MASK: u32 = asset_world::MASK_PLAYER_SOLID;
    let mut other: HashMap<u32, usize> = HashMap::new();
    let solid_brushes = c.brushes.iter().filter(|b| b.contents & MASK != 0).count();
    for b in c.brushes.iter().filter(|b| b.contents & MASK == 0) {
        *other.entry(b.contents).or_default() += 1;
    }
    let flags = &c.mesh.tri_content_flags;
    let solid_tris = flags.iter().filter(|&&f| f & MASK != 0).count();
    for &f in flags.iter().filter(|&&f| f & MASK == 0) {
        *other.entry(f).or_default() += 1;
    }
    let mut rest: Vec<(u32, usize)> = other.into_iter().collect();
    rest.sort_by(|a, b| b.1.cmp(&a.1));
    let rest: Vec<String> = rest.iter().take(6).map(|(k, n)| format!("{k:#x} x{n}")).collect();
    format!(
        "t7 collision: player-solid {solid_brushes} of {} brushes, {solid_tris} of {} triangles; the rest by contents: {}",
        c.brushes.len(),
        flags.len(),
        rest.join(", ")
    )
}

/// Triangles to lay the Zombies walk graph over: the drawn world's opaque
/// surfaces and the clip map's triangle mesh.
fn walk_soup(map: &T7Map, hidden: &[bool]) -> Vec<asset_world::world_iw7::SoupTri> {
    use asset_world::world_iw7::{IW7_SOLID, SoupTri};
    let mut tris = Vec::new();
    for (i, s) in map.surfaces.iter().enumerate() {
        if hidden.get(i).copied().unwrap_or(false) {
            continue;
        }
        if let Some(m) = s.material.and_then(|m| map.materials.get(m))
            && t7_draw(&m.name, &m.technique_set, false).blend != T6Blend::Opaque
        {
            continue;
        }
        for t in 0..s.tri_count as usize {
            let v = |k: usize| {
                let ix = map.indices.get(s.base_index as usize + 3 * t + k).copied().unwrap_or(0);
                map.vertices.get((s.first_vertex + u32::from(ix)) as usize).map_or([0.0; 3], |v| v.xyz)
            };
            tris.push(SoupTri { p: [v(0), v(1), v(2)], surface_flags: 0, contents: IW7_SOLID });
        }
    }
    if let Some(c) = &map.clip {
        for t in &c.tris {
            let v = |k: usize| c.verts.get(t[k] as usize).copied().unwrap_or([0.0; 3]);
            tris.push(SoupTri { p: [v(0), v(1), v(2)], surface_flags: 0, contents: IW7_SOLID });
        }
    }
    tris
}

/// (bo2mp: also Modern Warfare III's maps, read into a `T7Map` by
/// `super::jup_map`, which may hand over the collision it built from the
/// same map before: `collision`.)
pub(super) fn build_t7_world(
    path: &Path,
    map: &T7Map,
    mut report: Vec<String>,
    progress: &LoadProgress,
    material_seed: MaterialCatalog,
    collision: Option<asset_world::ClipCollision>,
    native: bool,
) -> LoadedWorld {
    super::bo2_look::set_pending_grade(None);
    let lap_clock = std::cell::Cell::new(std::time::Instant::now());
    // (Each lap also goes to the log, lane B 11-05: a game's load shows
    // which part of the materials stage holds it, 10.1 s on Skyjacked.)
    let lap = |what: &str| {
        let now = std::time::Instant::now();
        let secs = now.duration_since(lap_clock.get()).as_secs_f32();
        if std::env::var_os("BO2MP_T7_TIMING").is_some() {
            eprintln!("t7 load timing: {what}: {secs:.2}s");
        }
        diag::info!(World, "t7 load lap: {what}: {secs:.2}s");
        lap_clock.set(now);
    };
    let map_name = path.file_stem().map_or_else(String::new, |s| s.to_string_lossy().into_owned());
    // bo2mp: the map's scene props (`scriptbundle_scene` fxanims: Nuk3town's
    // derezzing buildings), as map script models named "fxanim" playing
    // their scene's animation (`sim::t6::fxanims`), built by
    // `super::t7::map_props`; their animations' timing waits for the
    // rules (`take_pending_props`).
    let scenes = if std::env::var("BO2MP_T7_SCENES").map_or(true, |v| v.trim() != "0") { scene_bundles(&map.entities) } else { Vec::new() };
    let plain = plain_script_models(&map.entities);
    // bo2mp: a Zombies map's actors (its spawners' AI types), drawn as
    // their characters and played with their own animations.
    let actors = if map_name.starts_with("zm_") { actor_types(&map.entities) } else { Vec::new() };
    // (Lane B 11-05: the props read their zone again, walk it keeping its
    // animations, open its packages and read the package bytes they build
    // from; that runs on a thread of its own
    // from here, beside the collision, the light and the materials, and is
    // joined where the props are built. `CODLEGACY_PROPS_READ_AHEAD=0`
    // reads it there instead.)
    let props_ahead = (!(scenes.is_empty() && plain.is_empty() && actors.is_empty())
        && std::env::var("CODLEGACY_PROPS_READ_AHEAD").map_or(true, |v| v.trim() != "0"))
    .then(|| {
        let zone_path = path.to_path_buf();
        // (The same scene names, sorted, the props are built from below;
        // their package bytes are read ahead too, `t7::warm_props`.)
        let mut names: Vec<String> = scenes.iter().map(|s| s.0.clone()).collect();
        names.sort();
        names.dedup();
        let plain = plain.clone();
        std::thread::Builder::new()
            .name("t7 props read".into())
            .spawn(move || super::t7::props_read(&zone_path, &names, &plain))
            .ok()
    })
    .flatten();

    // Surfaces not drawn: the sky's, tool, shadow-only and distortion
    // surfaces.
    let mut hidden = vec![false; map.surfaces.len()];
    let mut hidden_kinds: HashMap<&'static str, usize> = HashMap::new();
    for (i, s) in map.surfaces.iter().enumerate() {
        if let Some(kind) = s.material.and_then(|m| map.materials.get(m)).and_then(|m| hidden_kind(&m.name, &m.technique_set)) {
            hidden[i] = true;
            *hidden_kinds.entry(kind).or_default() += 1;
        }
    }

    lap("hidden surfaces");
    // Entities and spawns.
    let entities = iw3::ents::parse(&map.entities);
    let spawns = spawns_of(&entities);

    // Collision.
    let stage = progress.begin_scoped(StageId::MapAssets, "black ops iii collision", None);
    let mut collision = match collision {
        Some(c) => {
            report.push(format!("t7 clip: from the cache ({} triangles)", c.mesh.tri_count()));
            c
        }
        None => collision_of(map, &hidden, &mut report),
    };
    // (With their slabs: Nuk3town's slanted out-of-bounds volumes have
    // boxes reaching over the play area; without the slabs everyone died
    // on spawning.)
    collision.trigger_models = map
        .trigger_models
        .iter()
        .enumerate()
        .map(|(m, hulls)| {
            hulls
                .iter()
                .enumerate()
                .map(|(h, &(mid, half))| asset_world::MapTriggerHull {
                    mid,
                    half,
                    slabs: map.trigger_slabs.get(m).and_then(|t| t.get(h)).cloned().unwrap_or_default(),
                })
                .collect()
        })
        .collect();
    // bo2mp: a Zombies map: its layout in the Infinite Warfare Zombies
    // pipeline's terms and a walk graph over the world (played as Black
    // Ops II Zombies under `attach_t6_zm_rules`). Built only with the `iw7`
    // feature: Black Ops III Zombies is left out of the game.
    #[cfg(feature = "iw7")]
    if map_name.starts_with("zm_") {
        let (layout, triggers, lines) = zm::read(map, collision.trigger_models.len());
        report.extend(lines);
        collision.trigger_models.extend(triggers);
        let soup = walk_soup(map, &hidden);
        super::register_foreign_zm_map(&map_name);
        report.extend(super::iw7_zombies::prepare_layout(&map_name, layout, &soup));
    }
    let census = asset_world::world_iw3::spawn_ground_census(&collision, &spawns.iter().map(|s| s.origin).collect::<Vec<_>>());
    report.push(format!("t7 spawns: {census}"));
    if census.spawns > 0 && census.on_ground * 2 < census.spawns {
        diag::warn!(World, "bo2mp t7: {map_name}: most spawn points have no floor of their own in the collision ({census})");
    }
    stage.done();
    lap("collision, spawns");

    report.push(format!("t7 entities: {}, spawn points {}", entities.len(), spawns.len()));
    let mut scripts = crate::ScriptSources::default();
    scripts.set_entities(map.entities.clone());
    // bo2mp: Black Ops III's remakes of Black Ops II maps share their
    // layout and coordinates (Nuk3town's start spawns are Nuketown 2025's
    // to the unit), so BO2's bots walk the original's path nodes.
    if let Some(donor) = bo2_layout_of(&map_name) {
        match asset_transport::games_root_from_env()
            .and_then(|root| asset_transport::find_zone_file(&root, &format!("t6:{donor}")))
            .and_then(|z| asset_t6::capture_zone(&z.path).map_err(|e| e.to_string()))
        {
            Ok(c) => {
                report.push(format!("t7 nav: {} path nodes from Black Ops II's {donor}", c.path_nodes.len()));
                scripts.foreign_path_nodes = c.path_nodes;
            }
            Err(e) => report.push(format!("t7 nav: {donor} not read ({e}); the engine's walk graph instead")),
        }
    }
    let mut facts = crate::MapFacts::default();
    let corners: Vec<[f32; 2]> =
        entities.iter().filter(|e| e.get("targetname") == Some("minimap_corner")).filter_map(|e| e.origin().map(|o| [o[0], o[1]])).collect();
    if let [a, b] = corners[..] {
        facts.minimap_corners = Some(asset_world::MinimapCorners { a, b });
    }
    for compass_name in [format!("compass_map_{}", map_name.to_ascii_lowercase()), format!("minimap_{}", map_name.to_ascii_lowercase())] {
        if map.image_info(&compass_name).is_some() {
            facts.compass.material = Some(compass_name.clone());
            facts.compass.image = Some(compass_name);
            break;
        }
    }
    // Black Ops III's own: `i_compass_map_<map>_t7` (its HUD minimap's
    // material, `compass_map_<map>_t7`), else `_full` (the full-screen
    // map's, see-through off the map); handed to Black Ops II's HUD under
    // the name its script's `setminimap` asks for, flattened (see
    // `t7_compass_opaque`).
    super::t7_fx::set_pending_compass(None);
    if facts.compass.image.is_none() {
        let lower = map_name.to_ascii_lowercase();
        for picture in [format!("i_compass_map_{lower}_t7"), format!("i_compass_map_{lower}_full")] {
            let Ok(level) = map.image(&picture, 2048) else { continue };
            let Some((w, h, mut px)) = fastfile_t7::texture::level_rgba(&level, 0) else { continue };
            t7_compass_opaque(&mut px);
            let name = format!("compass_map_{lower}");
            super::t7_fx::set_pending_compass(Some((name.clone(), w, h, px)));
            facts.compass.material = Some(name.clone());
            facts.compass.image = Some(name);
            report.push(format!("t7 compass: {picture} {w}x{h}"));
            break;
        }
    }
    report.push(format!(
        "t7 compass: corners {}, picture {}",
        if facts.minimap_corners.is_some() { "found" } else { "missing" },
        facts.compass.image.as_deref().unwrap_or("missing")
    ));

    lap("nav, compass");
    // bo2mp: Black Ops III's own light (`super::t7_light`), when on.
    let stage_light = progress.begin_scoped(StageId::MapAssets, "black ops iii light", None);
    // (Only a Black Ops III zone: another game's map read into this form,
    // with its own baked light or sky, keeps its own.)
    let blocks = |m: usize| light_blocks(map, m);
    let places: Vec<[f32; 3]> = spawns.iter().map(|s| s.origin).collect();
    let lit_read = if map.baked_ambient.is_some() || map.sky_cube.is_some() {
        None
    } else if super::t7_light::on() {
        match super::t7_light::Lit::read(map) {
            Ok(read) => Some(read),
            Err(e) => {
                report.push(format!("t7 light: none ({e}); the flat light"));
                None
            }
        }
    } else {
        report.push("t7 light: off (BO2MP_T7_LIGHT=0): the flat light".to_owned());
        None
    };
    let lit_on = lit_read.is_some();
    lap("Lit::read (light)");

    // Materials: world surfaces, then the placed models'.
    let stage = progress.begin_scoped(StageId::MapAssets, "black ops iii materials", None);
    let mut catalog = material_seed;
    catalog.set_capture_ns(AssetNamespace::T6);
    let mut b = Materials {
        map,
        catalog,
        owner: ZoneOwner::from_zone_path(path),
        max: texture_max(),
        linked: HashMap::new(),
        images: HashMap::new(),
        decoded: 0,
        failures: Vec::new(),
        looks: (!native && super::t7_look::on()).then(|| super::t7_look::Looks::new(texture_max(), lit_on)),
        water: (lit_on && !map_name.contains("jup")).then_some(None),
    };
    // bo2mp (load speed): the looks the surfaces and placed models below
    // will link, in the order they link them, made a batch ahead on worker
    // threads.
    if !native && let Some(looks) = b.looks.as_mut() {
        looks.ahead(looks_wanted(map, &hidden));
    }
    // (Native: no material of the converted world's is made.)
    let (lit, surface_materials) = std::thread::scope(|scope| {
        let bake = lit_read.map(|read| {
            std::thread::Builder::new()
                .name("t7 light bake".to_owned())
                .spawn_scoped(scope, || read.finish(path, map, &places, &blocks, !native || super::t7_light::native_tracer()))
                .expect("t7 light bake thread")
        });
        let surface_materials: Vec<Option<usize>> = map
            .surfaces
            .iter()
            .enumerate()
            .map(|(i, s)| if hidden[i] || native { None } else { s.material.and_then(|m| b.material(m, false)) })
            .collect();
        let lit = bake.map(|bake| bake.join().unwrap_or_else(|panic| std::panic::resume_unwind(panic)));
        (lit, surface_materials)
    });
    if let Some(lit) = lit.as_ref() {
        report.extend(lit.report.iter().cloned());
    }
    stage_light.done();
    lap("world surface materials, the light's bake beside them");
    // The placed models.
    let look_scale_props = map.baked_ambient.filter(|_| map.bo3_look).map(look_scale);
    let (prop_meshes, prop_placements) = if native {
        (Vec::new(), Vec::new())
    } else {
        use bevy::prelude::{Mat3, Quat, Transform, Vec3};
        let mut meshes: Vec<asset_world::ModelMesh> = Vec::new();
        let mut built: HashMap<usize, Option<usize>> = HashMap::new();
        let mut placements = Vec::with_capacity(map.static_models.len());
        let mut bounds: HashMap<usize, ([f32; 3], [f32; 3])> = HashMap::new();
        for (placed, sm) in map.static_models.iter().enumerate() {
            let entry = *built.entry(sm.model).or_insert_with(|| {
                let m = &map.models[sm.model];
                let (mins, maxs, radius) = model_bounds(m);
                let key = |i: usize| asset_t6::AssetKey { ty: fastfile_t6::AssetType::Material, index: i };
                let lod0: Vec<asset_t6::XSurfaceRef> = m
                    .surfaces
                    .iter()
                    .filter(|s| !s.positions.is_empty() && s.positions.len() <= usize::from(u16::MAX))
                    .filter(|s| !s.material.and_then(|k| map.materials.get(k)).is_some_and(|m| hidden_kind(&m.name, &m.technique_set).is_some()))
                    .map(|s| asset_t6::XSurfaceRef {
                        verts: (0..s.positions.len())
                            .flat_map(|i| t6_vertex(s.positions[i], s.normals.get(i).copied().unwrap_or([0.0, 0.0, 1.0]), s.uvs.get(i).copied().unwrap_or_default()))
                            .collect(),
                        vert_count: s.positions.len() as u16,
                        indices: s.tris.iter().flatten().copied().collect(),
                        vert_lists: Vec::new(),
                        flags: 0,
                        material: s.material.map(key),
                        blend_counts: [0; 4],
                        blend: Vec::new(),
                        part_bits: [0; 6],
                    })
                    .collect();
                let xref = asset_t6::XModelRef {
                    lighting_origin_offset: [0.0; 3],
                    lighting_origin_range: 0.0,
                    name: m.name.clone(),
                    numsurfs: lod0.len().min(255) as u8,
                    materials: lod0.iter().map(|s| s.material).collect(),
                    num_bones: 1,
                    num_lods: 1,
                    mins,
                    maxs,
                    radius,
                    lod0,
                    bone_names: vec!["tag_origin".to_owned()],
                    num_root_bones: 1,
                    parent_list: Vec::new(),
                    quats: Vec::new(),
                    trans: Vec::new(),
                    base_mat: vec![([0.0, 0.0, 0.0, 1.0], [0.0; 3])],
                    part_classification: vec![0],
                    bone_info: vec![None],
                    lods: vec![(0.0, 0, 0)],
                    contents: 1,
                    coll_lod: -1,
                    coll_surfs: Vec::new(),
                    phys_preset: None,
                };
                let linked: HashMap<usize, usize> =
                    m.surfaces.iter().filter_map(|s| s.material).filter_map(|k| Some((k, b.material(k, true)?))).collect();
                let mesh = asset_world::build_t6_model_mesh(&xref, |k| linked.get(&k.index).copied())?;
                meshes.push(mesh);
                Some(meshes.len() - 1)
            });
            let Some(mesh) = entry else {
                placements.push(None);
                continue;
            };
            let basis = Mat3::from_cols(Vec3::from_array(sm.axis[0]), Vec3::from_array(sm.axis[1]), Vec3::from_array(sm.axis[2]));
            // (A sky dome far past the view's reach is drawn smaller about
            // its origin: Nuk3town's hex grid dome, 36000 units round,
            // never drew. It glows, unlit and added, so its size does not
            // show.)
            let dome = {
                let m = &map.models[sm.model];
                m.surfaces.iter().any(|s| s.material.and_then(|k| map.materials.get(k)).is_some_and(|m| m.technique_set.contains("_sky")))
            };
            let radius = bounds
                .entry(sm.model)
                .or_insert_with(|| {
                    let (lo, hi, _) = model_bounds(&map.models[sm.model]);
                    (lo, hi)
                })
                .1
                .iter()
                .fold(0.0f32, |r, v| r.max(v.abs()))
                * sm.scale;
            let scale = if dome && radius > SKY_DOME_RADIUS { sm.scale * SKY_DOME_RADIUS / radius } else { sm.scale };
            placements.push(Some(asset_world::StaticModelPlacement {
                mesh,
                transform: Transform { translation: Vec3::from_array(sm.origin), rotation: Quat::from_mat3(&basis), scale: Vec3::splat(scale) },
                origin: sm.origin,
                axis: sm.axis,
                scale,
                cull_dist: sm.cull_dist.clamp(0.0, f32::from(u16::MAX)) as u16,
                reflection_probe_index: lit.as_ref().map_or(0, |l| l.probe_for(sm.origin)),
                primary_light_index: 0,
                flags: 0,
                // No light grid read: the light baked for it, else a
                // neutral light (sqrt(1 / 32)); with Black Ops III's own
                // light, its ambient light there (the sun is the
                // renderer's, from the sun map).
                packed_lighting: Some(match lit.as_ref() {
                    Some(l) => {
                        let (lo, hi) = *bounds.entry(sm.model).or_insert_with(|| {
                            let (lo, hi, _) = model_bounds(&map.models[sm.model]);
                            (lo, hi)
                        });
                        l.prop_light_box(sm.origin, sm.axis, sm.scale, lo, hi, placed as u32)
                    }
                    None => {
                        let l = map.static_light.get(placed).copied().unwrap_or([45, 45, 45, 0]);
                        // (The look: the light in its linear units; the byte is
                        // sqrt(light / 32).)
                        match look_scale_props {
                            Some(k) => {
                                let p = |b: u8| (f32::from(b) * k.sqrt()).round().clamp(1.0, 255.0) as u8;
                                [p(l[0]), p(l[1]), p(l[2]), l[3]]
                            }
                            None => l,
                        }
                    }
                }),
                t6_primary: [0, 0],
            }));
        }
        report.push(format!(
            "t7 static models: {} placed of {}, {} distinct models built",
            placements.iter().filter(|p| p.is_some()).count(),
            map.static_models.len(),
            meshes.len()
        ));
        (meshes, placements)
    };
    lap("static model meshes + their materials");
    report.push(format!(
        "t7 materials: {} linked, {} colour maps decoded (at most {}px), {} failed{}; not drawn: {hidden_kinds:?}",
        b.linked.len(),
        b.decoded,
        b.max,
        b.failures.len(),
        if b.failures.is_empty() { String::new() } else { format!(" (first: {})", b.failures.iter().take(4).cloned().collect::<Vec<_>>().join("; ")) },
    ));
    report.push(b.looks.as_ref().map_or_else(|| "t7 look: off (BO2MP_T7_MATERIALS=0): colour maps only".to_owned(), super::t7_look::Looks::report));
    // bo2mp: a sky cube another game's reader made, for the sky pass.
    if let Some(sky) = &map.sky_cube {
        let px = fastfile_iw6::image::ImagePixels {
            width: sky.width,
            height: sky.height,
            levels: sky.levels,
            dxgi_format: sky.format,
            data: sky.data.clone(),
        };
        report.push(match super::iw6::link_sky(&mut b.catalog, &format!("t7:sky:{}", map.name), &px, b.owner) {
            Ok(_) => format!("t7 sky: {}x{} cube", sky.width, sky.height),
            Err(e) => format!("t7 sky: {e}"),
        });
    }
    // bo2mp: the map's placed effects (`super::t7_fx`), BO2MP_T7_FX=0 leaves them out.
    let fx_on = std::env::var("BO2MP_T7_FX").map_or(true, |v| v != "0");
    // (With the native renderer, `render_t7::forward::fx_draw` draws the
    // placed effects unless BO2MP_T7_NATIVE_FX=0; they are still read here
    // and handed over, and the effect runtime leaves them out only while
    // the native renderer says it draws them
    // (`frame::settings::t7_native_fx`), so a native scene that fails to
    // load falls back to these. Impacts stay.)
    let shots = if fx_on { super::t7_fx::placements(&map.entities) } else { Vec::new() };
    if native && !std::env::var("BO2MP_T7_NATIVE_FX").is_ok_and(|v| v.trim() == "0") {
        report.push(format!("t7 effects: {} placed effects kept as the fallback; the native renderer draws them once its effect pass is ready", shots.len()));
    }
    // bo2mp: Black Ops III's own impact tables (`super::t7_fx::impact_rows`).
    let impacts = if fx_on { super::t7_fx::impact_rows(map) } else { Vec::new() };
    if shots.is_empty() && impacts.is_empty() {
        super::t7_fx::set_pending(None);
    } else {
        let extra: Vec<String> = impacts.iter().flat_map(|(_, s, f)| s.iter().chain(f.iter())).filter(|n| !n.is_empty()).cloned().collect();
        let views = super::t7_fx::effects(map, &shots, &extra, &mut |name, draw, image, glow| b.effect_material(name, draw, image, glow), &mut report);
        report.push(format!("t7 impacts: {} Black Ops II rows from Black Ops III's tables", impacts.len()));
        super::t7_fx::set_pending(Some((views, shots, impacts)));
    }
    lap("fx");
    let sun_mats = if !native && std::env::var("BO2MP_T7_FLARE").map_or(true, |v| v.trim() != "0") { b.sun_materials() } else { None };
    let mut materials = std::mem::take(&mut b.catalog);
    let owner = b.owner;
    let reveals = b.looks.as_ref().map(|l| l.reveals.clone()).unwrap_or_default();
    drop(b);
    if let Some(lit) = lit.as_ref() {
        report.extend(lit.link(map, &map_name, &mut materials, owner));
    }
    lap("sun materials, lit.link");
    // (The scene props, their actors and plain script models: read at the
    // top, `props_ahead`.)
    super::t7_fx::set_pending_cast(None);
    let (mut scene_fpv, mut scene_xanims) = (asset_model::FpvMeshBuild::default(), asset_anim::XAnimBuild::default());
    super::t7_fx::set_pending_props(None);
    // (Black Ops III's bomb site clips stand only in its bomb modes, whose
    // site props Black Ops II's rules never place: left out, they were
    // invisible walls round Nuk3town's middle and the blue house.)
    let mut entity_text = without_bomb_clips(&map.entities);
    // (The blocks Black Ops III's map load spawns, by the exe's rules:
    // `fastfile_t7::spawn`. Fog, sun and nav volumes, fx placements and the
    // like have no spawn function there; here they were entities, some with
    // brush models round the whole map.)
    {
        let mp = !map_name.starts_with("zm_") && !map_name.starts_with("cp_");
        let (kept, dropped) = fastfile_t7::spawn::as_spawned(&entity_text, mp);
        entity_text = kept;
        report.push(format!("t7 entities not spawned by the game (left out): {dropped:?}"));
    }
    if map_name.starts_with("zm_") {
        let def = |c: &str| map.impact_fields(fastfile_t7::AssetType::ZBarrier, c).cloned();
        let (text, n) = zbarriers_from_defs(&entity_text, &def);
        entity_text = text;
        report.push(format!("t7 zbarriers built from their classnames' defs: {n}"));
    }
    if !scenes.is_empty() || !plain.is_empty() || !actors.is_empty() {
        let mut names: Vec<String> = scenes.iter().map(|s| s.0.clone()).collect();
        names.sort();
        names.dedup();
        // (A read-ahead thread that panicked: read here, as before.)
        let ahead = props_ahead.and_then(|h| h.join().ok());
        let props = super::t7::map_props(path, ahead, &names, &plain, &actors, materials);
        super::t7_fx::set_pending_cast(Some(props.cast.clone()));
        materials = props.material_seed;
        report.extend(props.report);
        let mut extra = String::new();
        for (bundle, origin, angles) in &scenes {
            for (_, model, anim) in props.props.iter().filter(|p| p.0 == *bundle) {
                if scene_fpv_has(&props.fpv, model) {
                    extra.push_str(&format!(
                        "{{\n\"classname\" \"script_model\"\n\"model\" \"{model}\"\n\"origin\" \"{origin}\"\n\"angles\" \"{angles}\"\n\"targetname\" \"fxanim\"\n\"fxanim_scene_1\" \"{anim}\"\n}}\n"
                    ));
                }
            }
        }
        if !extra.is_empty() {
            if !entity_text.ends_with('\n') {
                entity_text.push('\n');
            }
            entity_text.push_str(&extra);
        }
        // (A destructible whose model could not be built here (Nuk3town's
        // female mannequins are put together from sub-models) leaves its
        // clip post out too: unseen, it was an invisible pole.)
        let unbuilt: Vec<&String> = plain.iter().filter(|m| !scene_fpv_has(&props.fpv, m)).collect();
        if !unbuilt.is_empty() {
            let (text, dropped) = without_clips_of(&entity_text, &|m| unbuilt.iter().any(|u| u.as_str() == m));
            entity_text = text;
            report.push(format!("t7 script models: {} not built ({:?}); {dropped} of their clips left out", unbuilt.len(), &unbuilt[..unbuilt.len().min(4)]));
        }
        scene_fpv = props.fpv;
        scene_xanims = props.xanims;
        super::t7_fx::set_pending_props(Some(props.anims));
    }
    if entity_text != map.entities {
        scripts.set_entities(entity_text);
    }
    stage.done();

    lap("scene props, entities");
    let sun = sun_of(map, &entities);
    let probe_of = |i: usize| {
        let s = &map.surfaces[i];
        lit.as_ref().map_or(0, |l| l.probe_for(std::array::from_fn(|k| (s.mins[k] + s.maxs[k]) * 0.5)))
    };
    let iw3w = iw3_world(map, sun, &probe_of);
    let (pitch, yaw) = (sun.0[0].to_radians(), sun.0[1].to_radians());
    // (With Black Ops III's own light, the sun lies opposite the way its
    // angles aim: `fastfile_t7::light::sun_toward`, as its probes saw it.)
    let dir = match lit.as_ref() {
        Some(l) => l.sun,
        None => [pitch.cos() * yaw.cos(), pitch.cos() * yaw.sin(), -pitch.sin()],
    };
    // (bo2mp: a map with its light baked into its vertex colours is drawn
    // through the fallback draw's Call of Duty 4 branch, the sun colour's
    // alpha 3, over one uniform page of its ambient light.)
    // (In Black Ops III's look the page is read as the look's linear pages,
    // the square root of the light over 4: the gamma page's colour times
    // sqrt(scale) / 2.)
    let look_scale = map.baked_ambient.filter(|_| map.bo3_look).map(look_scale);
    let baked = map.baked_ambient.map(|a| look_scale.map_or(a, |k| a.map(|c| c * k.sqrt() * 0.5))).and_then(|a| match uniform_lightmap(a) {
        Ok(l) => Some(l),
        Err(e) => {
            report.push(format!("t7 baked light: no page ({e:?})"));
            None
        }
    });
    let colour = if look_scale.is_some() {
        // (The look: the sun's hue; its light is the look's row.)
        let peak = sun.1.iter().copied().fold(0.0f32, f32::max).max(1e-3);
        [sun.1[0] / peak, sun.1[1] / peak, sun.1[2] / peak, 1.0]
    } else {
        [sun.1[0] * sun.2, sun.1[1] * sun.2, sun.1[2] * sun.2, if baked.is_some() { 3.0 } else { 1.0 }]
    };
    report.push(format!(
        "t7 sun: toward {dir:?} colour {colour:?}; sky models {:?} ({})",
        map.sky_models,
        if lit.is_some() { "the sky picture drawn" } else { "not drawn" }
    ));
    let lighting = asset_world::T6WorldLighting {
        sun: asset_world::WorldSunLight { direction: dir, diffuse: colour, specular: colour },
        exposure_scale: 1.0,
        sky: [0.0, 1.0, 0.0, 1.0],
        sky_parms: [1.0, 0.0],
        saturation: None,
        grade: None,
    };
    let baked_lit = baked.is_some();
    let pages = if lit.is_some() { vec![Some(super::t7_light::blank_page())] } else { baked.map(|l| vec![Some(l)]).unwrap_or_default() };
    lap("iw3_world");
    // (Native: every surface hidden, so the draw holds no mesh, only the
    // map's light for the players and the gun.)
    let draw_hidden = if native { vec![true; hidden.len()] } else { hidden.clone() };
    let built_draw = asset_world::world_iw3::build_iw3_world_draw(&iw3w, &surface_materials, &draw_hidden, pages, Some(lighting));
    lap("build_iw3_world_draw");
    match built_draw {
        Ok(mut draw) => {
            // bo2mp: the sun's sprite and lens flare (`sun_materials`).
            if let Some((sprite, flare)) = sun_mats {
                let d = sun_toward(sun.0);
                draw.sun_effects = Some(asset_world::SunEffectsCapture {
                    sprite_material: Some(sprite),
                    flare_material: Some(flare),
                    sprite_size: 40.0,
                    flare_min_size: 0.0,
                    flare_min_dot: 0.85,
                    flare_max_size: 420.0,
                    flare_max_dot: 0.99,
                    flare_max_alpha: 0.45,
                    flare_fade_in_ms: 200,
                    flare_fade_out_ms: 200,
                    blind_min_dot: 0.0,
                    blind_max_dot: 0.0,
                    blind_max_darken: 0.0,
                    blind_fade_in_ms: 0,
                    blind_fade_out_ms: 0,
                    glare_min_dot: 0.9,
                    glare_max_dot: 0.995,
                    glare_max_lighten: 0.2,
                    glare_fade_in_ms: 300,
                    glare_fade_out_ms: 300,
                    direction: d,
                });
            }
            if let Some(lit) = lit.as_ref() {
                // Black Ops III's own light: the vertices' ambient light,
                // the sun map, the probes and the branch's numbers.
                let t = std::time::Instant::now();
                if !native {
                    draw.colors = lit.world_colours(map, &draw.colors, &reveals);
                }
                draw.t6_lut = Some(Arc::new(lit.sun_map_lut()));
                draw.t6_probes = lit.probe_rows();
                let mut rows = vec![[0.0f32; 16]; asset_world::T7_LIGHT_ROW + 2];
                rows[asset_world::T7_LIGHT_ROW..].copy_from_slice(&lit.light_rows(sun.2));
                if let Some(fog) = lit.fog.as_ref() {
                    for (i, r) in fog.iter().enumerate() {
                        let row = super::t7_light::T7_FOG_ROW + i / 4;
                        rows[row][4 * (i % 4)..4 * (i % 4) + 4].copy_from_slice(r);
                    }
                }
                draw.t6_lights = rows;
                draw.t7_light = true;
                asset_world::install_t6_probe_origins(lit.keep.iter().map(|&i| lit.bake.probes[i].origin).collect());
                // Its colour table and bloom in the final pass (taken up by
                // `t6::attach_t6_rules`).
                super::bo2_look::set_pending_grade(super::t7_light::grade(map, &mut report));
                lap("world_colours + light rows");
                report.push(format!("t7 light: world vertices lit in {:.1}s; exposure {}", t.elapsed().as_secs_f32(), lit.exposure));
            } else if map.baked_ambient.is_none() && map.sky_cube.is_none() {
                asset_world::install_t6_probe_origins(Vec::new());
            }
            // bo2mp: a map with its light baked into vertex colours, drawn in
            // Black Ops III's look (`bo3_look`, as Carrier): the sun's hue and
            // light, the sun map and the real-time ambient over the spawn
            // points, the filmic tone curve. The page and the models' light
            // are in the look's units (above); the sky's colours are the
            // baked ambient's.
            if let (Some(k), Some(a)) = (look_scale, map.baked_ambient) {
                let places: Vec<[f32; 3]> = spawns.iter().map(|s| s.origin).collect();
                let sun_light = LOOK_SUN;
                let peak = sun.1.iter().copied().fold(0.0f32, f32::max).max(1e-3);
                let hue = sun.1.map(|c| c / peak);
                let horizon = a.map(|c| c * c);
                let rt_options = super::rt_ambient::Options {
                    sky: Some(([horizon[0] * 0.85, horizon[1] * 0.95, horizon[2] * 1.15], horizon)),
                    local: false,
                };
                super::bo3_look::apply(
                    &mut draw,
                    &materials,
                    &prop_meshes,
                    &prop_placements,
                    &places,
                    dir,
                    hue.map(|h| h * sun_light),
                    Some(1.0),
                    &rt_options,
                    &mut report,
                );
                if draw.t7_light {
                    draw.t6_lights[asset_world::T7_LIGHT_ROW][1] = sun_light;
                    draw.t6_lights[asset_world::T7_LIGHT_ROW + 1][9] = 1.0;
                    // (Albedo past 0.5 rolls off at a quarter on an MW3
                    // map: `t7_shade`; `BO2MP_JUP_ROLLOFF=0` off.)
                    draw.t6_lights[asset_world::T7_LIGHT_ROW + 1][13] = if std::env::var("BO2MP_JUP_ROLLOFF").is_ok_and(|v| v.trim() == "0") { 0.0 } else { 1.0 };
                    report.push(format!("jup look: on (BO2MP_BO3_LOOK_JUP=0 turns it off): baked ambient x{k:.2} to the look's units, sun x{sun_light:.2}"));
                }
            }
            report.push(format!(
                "world mesh: {} vertices, {} triangles, {} surfaces (Black Ops III, fallback draw with colour maps, {})",
                draw.stats.vertices,
                draw.stats.triangles,
                draw.stats.surfaces,
                if draw.t7_light {
                    "Black Ops III's own light"
                } else if baked_lit {
                    "light baked into vertex colours"
                } else {
                    "no lightmaps"
                }
            ));
            let (min, max) = if native { (map.mins, map.maxs) } else { (draw.stats.min, draw.stats.max) };
            materials.set_capture_ns(AssetNamespace::T6);
            LoadedWorld {
                scripts,
                world: PreparedWorld {
                    draw: Some(draw),
                    min,
                    max,
                    policy: WorldDrawPolicy::t5(),
                    static_model_meshes: prop_meshes,
                    static_model_instances: prop_placements,
                    ..Default::default()
                },
                materials,
                fpv_meshes: scene_fpv,
                xanims: scene_xanims,
                collision: Some(collision),
                spawns,
                facts,
                report,
                ..Default::default()
            }
        }
        Err(error) => {
            let mut loaded = LoadedWorld::with_gap(
                WorldDrawPolicy::t5(),
                PreparedCapability::PreparedWorld,
                format!("Black Ops III world mesh: {error}"),
                Some("assets::lane::t7_map::load_world/world_mesh"),
            );
            loaded.scripts = scripts;
            loaded.collision = Some(collision);
            loaded.spawns = spawns;
            loaded.materials = materials;
            loaded.facts = facts;
            loaded.report.extend(report);
            loaded
        }
    }
}

#[cfg(test)]
pub(crate) mod tests {
    /// bo2mp: the clip map's raw numbers (contents by brush / material /
    /// static model / collision surface, winding and walkable-edge facts).
    #[test]
    #[ignore]
    fn t7_clip_census() {
        use std::collections::{BTreeMap, HashMap};
        let Ok(paths) = std::env::var("T7_TEST_MAP") else { return };
        for path in paths.split(',') {
            let map = super::T7Map::load(&super::MapSources::beside(std::path::Path::new(path))).expect("map");
            let clip = map.clip.as_ref().expect("clip");
            println!("== {path}");
            let mut mats: BTreeMap<u32, Vec<String>> = BTreeMap::new();
            for (n, s, c) in &clip.materials {
                mats.entry(*c).or_default().push(format!("{n}(s{s:#x})"));
            }
            for (c, v) in &mats {
                println!("  material contents {c:#010x}: {}", v.join(" "));
            }
            let reach = asset_world::world_t7::bsp_leaves_reached(&clip.nodes.iter().map(|n| (n.plane, n.children)).collect::<Vec<_>>(), clip.leafs.len()).unwrap_or_default();
            let mut world = vec![false; clip.brushes.len()];
            for &l in &reach {
                for &b in &clip.leafs[l].brushes {
                    if let Some(w) = world.get_mut(usize::from(b)) {
                        *w = true;
                    }
                }
            }
            let mut by: BTreeMap<u32, (usize, usize)> = BTreeMap::new();
            for (i, b) in clip.brushes.iter().enumerate() {
                let e = by.entry(b.contents).or_default();
                e.0 += 1;
                e.1 += usize::from(world[i]);
            }
            println!("  brushes by contents (all, world): {by:x?}");
            // Brushes whose planes enclose more than their corners: a
            // corner of the planes' solid more than 1 unit outside the hull
            // of the stored corners (a side missing: bigger than built).
            let mut loose: BTreeMap<u32, (usize, usize)> = BTreeMap::new();
            let mut worst: Vec<(f32, usize)> = Vec::new();
            for (i, b) in clip.brushes.iter().enumerate() {
                if b.contents & asset_world::MASK_PLAYER_SOLID == 0 || b.verts.is_empty() || b.planes.len() > 40 {
                    continue;
                }
                let far = fastfile_t7::map::brush_excess(&b.planes, &b.verts);
                let e = loose.entry(b.contents).or_default();
                e.1 += 1;
                if far > 1.0 {
                    e.0 += 1;
                    worst.push((far, i));
                }
            }
            worst.sort_by(|a, b| b.0.total_cmp(&a.0));
            println!("  player-solid brushes bigger than their corners (loose, all) by contents: {loose:x?}");
            for (far, i) in worst.iter().take(6) {
                let b = &clip.brushes[*i];
                println!("    brush {i} contents {:#x} planes {} corners {} solid {far:.1} past its corners", b.contents, b.planes.len(), b.verts.len());
                if std::env::var_os("T7_CENSUS_DUMP").is_some() {
                    println!("      planes {:?}\n      corners {:?}", b.planes, b.verts);
                }
            }
            let mut sm: BTreeMap<u32, usize> = BTreeMap::new();
            let mut cs: BTreeMap<(u32, u32), usize> = BTreeMap::new();
            for s in &clip.static_models {
                *sm.entry(s.contents).or_default() += 1;
                for t in clip.model_collision.get(&s.name).into_iter().flatten() {
                    *cs.entry((s.contents, t.contents)).or_default() += 1;
                }
            }
            println!("  static models by contents: {sm:x?}");
            println!("  coll tris by (static model contents, surface contents): {cs:x?}");
            // Winding: the trace's front is (c - a) x (b - a); for each
            // model, the share of triangles whose front faces away from the
            // centre of its collision triangles.
            let (mut out_n, mut all_n) = (0usize, 0usize);
            for tris in clip.model_collision.values() {
                if tris.is_empty() {
                    continue;
                }
                let mut c = [0.0f32; 3];
                for t in tris {
                    for p in t.corners {
                        for k in 0..3 {
                            c[k] += p[k] / (3 * tris.len()) as f32;
                        }
                    }
                }
                for t in tris {
                    let [a, b, cc] = t.corners;
                    let e1 = [cc[0] - a[0], cc[1] - a[1], cc[2] - a[2]];
                    let e2 = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
                    let n = [e1[1] * e2[2] - e1[2] * e2[1], e1[2] * e2[0] - e1[0] * e2[2], e1[0] * e2[1] - e1[1] * e2[0]];
                    let m: [f32; 3] = std::array::from_fn(|k| (a[k] + b[k] + cc[k]) / 3.0 - c[k]);
                    all_n += 1;
                    out_n += usize::from(n[0] * m[0] + n[1] * m[1] + n[2] * m[2] > 0.0);
                }
            }
            println!("  model coll tris facing out of their model: {out_n} of {all_n}");
            // World mesh: near-horizontal triangles facing up, and the
            // walkable bit on edges shared by two coplanar triangles.
            let tri_n = |t: &[u32; 3]| {
                let v = |k: usize| clip.verts.get(t[k] as usize).copied().unwrap_or([0.0; 3]);
                let (a, b, c) = (v(0), v(1), v(2));
                let e1 = [c[0] - a[0], c[1] - a[1], c[2] - a[2]];
                let e2 = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
                let n = [e1[1] * e2[2] - e1[2] * e2[1], e1[2] * e2[0] - e1[0] * e2[2], e1[0] * e2[1] - e1[1] * e2[0]];
                let l = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt().max(1e-9);
                [n[0] / l, n[1] / l, n[2] / l]
            };
            let (mut up, mut down) = (0usize, 0usize);
            for t in &clip.tris {
                let n = tri_n(t);
                if n[2] > 0.7 {
                    up += 1;
                } else if n[2] < -0.7 {
                    down += 1;
                }
            }
            println!("  world mesh tris {}: facing up {up}, down {down}", clip.tris.len());
            let mut edges: HashMap<(u32, u32), Vec<(usize, usize)>> = HashMap::new();
            for (i, t) in clip.tris.iter().enumerate() {
                for k in 0..3 {
                    let (a, b) = (t[k], t[(k + 1) % 3]);
                    edges.entry((a.min(b), a.max(b))).or_default().push((i, k));
                }
            }
            let bit = |i: usize, k: usize| clip.walkable.get((3 * i + k) / 8).is_some_and(|b| b & (1 << ((3 * i + k) % 8)) != 0);
            let mut flat = [0usize; 2];
            let mut border = [0usize; 2];
            let mut sharp = [0usize; 2];
            for v in edges.values() {
                if v.len() == 1 {
                    border[usize::from(bit(v[0].0, v[0].1))] += 1;
                } else if v.len() == 2 {
                    let (n0, n1) = (tri_n(&clip.tris[v[0].0]), tri_n(&clip.tris[v[1].0]));
                    let d = n0[0] * n1[0] + n0[1] * n1[1] + n0[2] * n1[2];
                    let b = usize::from(bit(v[0].0, v[0].1));
                    if d > 0.999 && n0[2] > 0.7 {
                        flat[b] += 1;
                    } else if d < 0.5 {
                        sharp[b] += 1;
                    }
                }
            }
            println!("  walkable bit (unset, set): flat floor shared edges {flat:?}, sharp shared edges {sharp:?}, border edges {border:?}");
        }
    }

    /// bo2mp: the invisible-barrier count (headless, no GPU). From every
    /// spawn (put on the ground), and from the ends of its open 200-unit
    /// walks in 4 directions, the player box raised a step (19) is swept 400
    /// units in 24 directions; a stop with nothing drawn (world surfaces
    /// not hidden as tools, and the placed models' drawn triangles) within
    /// 12 units of the face it stands against is a barrier the player
    /// cannot see. Each is put down to what stopped it: a placed model's
    /// collision (named), a brush (by contents: the `*clip*` materials are
    /// the map's own player clip, there in Black Ops III too) or the world
    /// mesh. Run with the clip map read as before (`BO2MP_T7_OLD_CLIP_AXES=1`,
    /// `BO2MP_T7_OLD_BRUSH_SIDES=1`) and as now (`T7_TEST_MAP=<zone.ff>[,...]
    /// cargo test -p assets --lib t7_barriers -- --ignored --nocapture`;
    /// `T7_BARRIER_MODES=before,axes,now` adds the axes-only step).
    #[test]
    #[ignore]
    fn t7_barriers() {
        use std::collections::{BTreeMap, HashMap};
        let Ok(paths) = std::env::var("T7_TEST_MAP") else { return };
        const CELL: f32 = 32.0;
        const MASK: u32 = asset_world::MASK_PLAYER_SOLID;
        let key = |p: [f32; 3]| [(p[0] / CELL).floor() as i32, (p[1] / CELL).floor() as i32, (p[2] / CELL).floor() as i32];
        type Grid<T> = HashMap<[i32; 3], Vec<T>>;
        fn add<T: Clone>(grid: &mut Grid<T>, t: [[f32; 3]; 3], v: T) {
            let k = |p: [f32; 3]| [(p[0] / CELL).floor() as i32, (p[1] / CELL).floor() as i32, (p[2] / CELL).floor() as i32];
            let lo: [f32; 3] = std::array::from_fn(|i| t[0][i].min(t[1][i]).min(t[2][i]));
            let hi: [f32; 3] = std::array::from_fn(|i| t[0][i].max(t[1][i]).max(t[2][i]));
            let (a, b) = (k(lo), k(hi));
            if (i64::from(b[0] - a[0]) + 1) * (i64::from(b[1] - a[1]) + 1) * (i64::from(b[2] - a[2]) + 1) > 4096 {
                return;
            }
            for x in a[0]..=b[0] {
                for y in a[1]..=b[1] {
                    for z in a[2]..=b[2] {
                        grid.entry([x, y, z]).or_default().push(v.clone());
                    }
                }
            }
        }
        fn dist(p: [f32; 3], t: &[[f32; 3]; 3]) -> f32 {
            let sub = |a: [f32; 3], b: [f32; 3]| [a[0] - b[0], a[1] - b[1], a[2] - b[2]];
            let dot = |a: [f32; 3], b: [f32; 3]| a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
            let cross = |a: [f32; 3], b: [f32; 3]| [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]];
            let seg = |a: [f32; 3], b: [f32; 3]| {
                let ab = sub(b, a);
                let u = (dot(sub(p, a), ab) / dot(ab, ab).max(1e-6)).clamp(0.0, 1.0);
                let q = [a[0] + ab[0] * u, a[1] + ab[1] * u, a[2] + ab[2] * u];
                dot(sub(p, q), sub(p, q)).sqrt()
            };
            let mut d = seg(t[0], t[1]).min(seg(t[1], t[2])).min(seg(t[2], t[0]));
            let n = cross(sub(t[1], t[0]), sub(t[2], t[0]));
            let nl = dot(n, n).sqrt();
            if nl > 1e-6 {
                let n = [n[0] / nl, n[1] / nl, n[2] / nl];
                let h = dot(sub(p, t[0]), n);
                let q = [p[0] - n[0] * h, p[1] - n[1] * h, p[2] - n[2] * h];
                if (0..3).all(|i| dot(cross(sub(t[(i + 1) % 3], t[i]), sub(q, t[i])), n) >= 0.0) {
                    d = d.min(h.abs());
                }
            }
            d
        }
        let place = |p: [f32; 3], o: [f32; 3], a: [[f32; 3]; 3], sc: f32| -> [f32; 3] { std::array::from_fn(|k| o[k] + (p[0] * a[0][k] + p[1] * a[1][k] + p[2] * a[2][k]) * sc) };
        let mut summary = Vec::new();
        for path in paths.split(',') {
            let mut map = super::T7Map::load(&super::MapSources::beside(std::path::Path::new(path))).expect("map");
            let hidden: Vec<bool> = map
                .surfaces
                .iter()
                .map(|s| s.material.is_some_and(|m| super::hidden_kind(&map.materials[m].name, &map.materials[m].technique_set).is_some()))
                .collect();
            let mut drawn: Grid<[[f32; 3]; 3]> = HashMap::new();
            for (i, s) in map.surfaces.iter().enumerate() {
                if hidden[i] {
                    continue;
                }
                for t in 0..s.tri_count as usize {
                    let v = |k: usize| {
                        let ix = map.indices.get(s.base_index as usize + 3 * t + k).copied().unwrap_or(0);
                        map.vertices.get((s.first_vertex + u32::from(ix)) as usize).map_or([0.0; 3], |v| v.xyz)
                    };
                    let t = [v(0), v(1), v(2)];
                    add(&mut drawn, t, t);
                }
            }
            for sm in &map.static_models {
                for s in &map.models[sm.model].surfaces {
                    for t in &s.tris {
                        let v = |k: usize| place(s.positions[usize::from(t[k])], sm.origin, sm.axis, sm.scale);
                        let t = [v(0), v(1), v(2)];
                        add(&mut drawn, t, t);
                    }
                }
            }
            let near_drawn = |p: [f32; 3]| -> f32 {
                let c = key(p);
                let mut best = f32::MAX;
                for x in c[0] - 1..=c[0] + 1 {
                    for y in c[1] - 1..=c[1] + 1 {
                        for z in c[2] - 1..=c[2] + 1 {
                            for t in drawn.get(&[x, y, z]).into_iter().flatten() {
                                best = best.min(dist(p, t));
                            }
                        }
                    }
                }
                best
            };
            let spawns: Vec<[f32; 3]> = super::spawns_of(&iw3::ents::parse(&map.entities)).iter().map(|s| s.origin).collect();
            let (mins, maxs) = ([-15.0f32, -15.0, 0.0], [15.0f32, 15.0, 70.0]);
            // The modes (`T7_BARRIER_MODES`, default `before,now`): before
            // (both old readings), `axes` (the static models' axes fixed,
            // the brushes' sides not), now (both fixed). Each reloads the
            // map with the switches set.
            let modes = std::env::var("T7_BARRIER_MODES").unwrap_or_else(|_| "before,now".to_owned());
            // How high the box is lifted off the floor (`T7_BARRIER_LIFT`,
            // default a step, 19; 60 is a jump's top).
            let lift: f32 = std::env::var("T7_BARRIER_LIFT").ok().and_then(|v| v.parse().ok()).unwrap_or(19.0);
            for mode in modes.split(',') {
                let (old_axes, old_sides) = match mode {
                    "before" => (true, true),
                    "axes" => (false, true),
                    _ => (false, false),
                };
                // SAFETY: this ignored test runs alone (`--ignored <name>`);
                // nothing else reads the environment meanwhile.
                unsafe {
                    std::env::set_var("BO2MP_T7_OLD_CLIP_AXES", if old_axes { "1" } else { "0" });
                    std::env::set_var("BO2MP_T7_OLD_BRUSH_SIDES", if old_sides { "1" } else { "0" });
                }
                map = super::T7Map::load(&super::MapSources::beside(std::path::Path::new(path))).expect("map");
                let mut report = Vec::new();
                let full = super::collision_of(&map, &hidden, &mut report);
                let clip = map.clip.as_mut().expect("clip");
                let models = std::mem::take(&mut clip.static_models);
                let no_models = super::collision_of(&map, &hidden, &mut report);
                let clip = map.clip.as_mut().expect("clip");
                clip.static_models = models;
                let mut brushes_only = full.clone();
                brushes_only.mesh = std::sync::Arc::new(Default::default());
                // The placed models' collision triangles, by model.
                let clip = map.clip.as_ref().expect("clip");
                let mut coll: Grid<(usize, [[f32; 3]; 3])> = HashMap::new();
                for (ci, sm) in clip.static_models.iter().enumerate() {
                    for t in clip.model_collision.get(&sm.name).into_iter().flatten() {
                        let bone = usize::try_from(t.bone).ok().and_then(|b| sm.bones.get(b));
                        let c = t.corners.map(|p| place(bone.map_or(p, |m| fastfile_t7::map::bone_to_model(m, p)), sm.origin, sm.axis, sm.scale));
                        add(&mut coll, c, (ci, c));
                    }
                }
                let mut starts: Vec<[f32; 3]> = Vec::new();
                for o in &spawns {
                    let down = full.sweep_box([o[0], o[1], o[2] + 32.0], [o[0], o[1], o[2] - 128.0], mins, maxs, MASK);
                    if down.startsolid || down.fraction >= 1.0 {
                        continue;
                    }
                    let g = [down.endpos[0], down.endpos[1], down.endpos[2] + lift];
                    starts.push(g);
                    for k in 0..4 {
                        let a = k as f32 * std::f32::consts::TAU / 4.0 + 0.4;
                        let e = [g[0] + a.cos() * 200.0, g[1] + a.sin() * 200.0, g[2]];
                        let h = full.sweep_box(g, e, mins, maxs, MASK);
                        if h.fraction >= 1.0 && !h.startsolid {
                            let d = full.sweep_box(e, [e[0], e[1], e[2] - 160.0], mins, maxs, MASK);
                            if !d.startsolid && d.fraction < 1.0 {
                                starts.push([d.endpos[0], d.endpos[1], d.endpos[2] + lift]);
                            }
                        }
                    }
                }
                let (mut sweeps, mut hits, mut invisible) = (0usize, 0usize, 0usize);
                let mut kinds: BTreeMap<&'static str, usize> = BTreeMap::new();
                let mut sources: HashMap<String, usize> = HashMap::new();
                for start in &starts {
                    for k in 0..24 {
                        let a = k as f32 * std::f32::consts::TAU / 24.0;
                        let end = [start[0] + a.cos() * 400.0, start[1] + a.sin() * 400.0, start[2]];
                        sweeps += 1;
                        let h = full.sweep_box(*start, end, mins, maxs, MASK);
                        if h.startsolid || h.fraction >= 1.0 || h.normal[2] > 0.7 {
                            continue;
                        }
                        hits += 1;
                        let n = h.normal;
                        let at = |zz: f32| [h.endpos[0] - n[0] * 15.5, h.endpos[1] - n[1] * 15.5, h.endpos[2] + zz];
                        if [8.0f32, 24.0, 40.0, 56.0].into_iter().any(|zz| near_drawn(at(zz)) <= 12.0) {
                            continue;
                        }
                        invisible += 1;
                        let b = brushes_only.sweep_box(*start, end, mins, maxs, MASK);
                        let w = no_models.sweep_box(*start, end, mins, maxs, MASK);
                        let (kind, source) = if (b.fraction - h.fraction).abs() < 1e-3 {
                            let e = h.endpos;
                            let br = full.brushes.iter().enumerate().filter(|(_, br)| br.planes.len() >= 6 && br.contents & MASK != 0).find(|(_, br)| {
                                br.planes.iter().all(|pl| {
                                    let c: [f32; 3] = std::array::from_fn(|k| e[k] - n[k] * 0.5 + if pl[k] < 0.0 { maxs[k] } else { mins[k] });
                                    pl[0] * c[0] + pl[1] * c[1] + pl[2] * c[2] - pl[3] <= 1.0
                                })
                            });
                            // Stopped where the brush's planes reach past its
                            // own corners (a face of their hull parts the box
                            // from them): a side the zone left out.
                            let past_corners = br.is_some_and(|(i, _)| {
                                let corners = clip.brushes.get(i).map(|b| b.verts.as_slice()).unwrap_or(&[]);
                                let pushed: [f32; 3] = std::array::from_fn(|k| e[k] - n[k] * 1.0);
                                let boxc: Vec<[f64; 3]> = (0..8).map(|m| std::array::from_fn(|k| f64::from(pushed[k] + if m >> k & 1 == 0 { mins[k] } else { maxs[k] }))).collect();
                                fastfile_t7::map::corner_hull_faces(corners).iter().any(|f| boxc.iter().all(|c| f[0] * c[0] + f[1] * c[1] + f[2] * c[2] - f[3] > 1.0))
                            });
                            match br.map(|(_, br)| br.contents) {
                                Some(c) if past_corners => ("brush past its corners", format!("brush past its corners, contents {c:#x}")),
                                Some(c) if c & 0x1_0000 != 0 && c & 1 == 0 => ("player clip brush", format!("clip brush contents {c:#x}")),
                                Some(c) => ("solid brush", format!("solid brush contents {c:#x}")),
                                None => ("solid brush", "brush (unfound)".to_owned()),
                            }
                        } else if (w.fraction - h.fraction).abs() < 1e-3 {
                            ("world mesh", "world mesh".to_owned())
                        } else {
                            let p = at(30.0);
                            let c = key(p);
                            let mut best: Option<(f32, usize)> = None;
                            for x in c[0] - 1..=c[0] + 1 {
                                for y in c[1] - 1..=c[1] + 1 {
                                    for z in c[2] - 2..=c[2] + 1 {
                                        for (ci, t) in coll.get(&[x, y, z]).into_iter().flatten() {
                                            let d = dist(p, t);
                                            if best.is_none_or(|(bd, _)| d < bd) {
                                                best = Some((d, *ci));
                                            }
                                        }
                                    }
                                }
                            }
                            ("placed model", best.map_or_else(|| "placed model (unfound)".to_owned(), |(_, ci)| format!("model {}", clip.static_models[ci].name)))
                        };
                        *kinds.entry(kind).or_default() += 1;
                        *sources.entry(source).or_default() += 1;
                    }
                }
                let label = match mode {
                    "before" => "before (axes as rows, sides as stored)",
                    "axes" => "axes fixed only",
                    _ => "now (axes as columns, sides from corners)",
                };
                println!("== {path} {label}: starts {}, sweeps {sweeps}, stopped {hits}, invisible {invisible}; by kind {kinds:?}", starts.len());
                let mut v: Vec<_> = sources.into_iter().collect();
                v.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
                for (s, n) in v.iter().take(12) {
                    println!("    {n:5} {s}");
                }
                let name = std::path::Path::new(path).file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
                summary.push(format!(
                    "{name} {label}: invisible {invisible} of {hits} stops ({sweeps} sweeps, lift {lift}); placed models {}, brushes past their corners {}, the map's player clip {}, solid brushes {}, world mesh {}",
                    kinds.get("placed model").unwrap_or(&0),
                    kinds.get("brush past its corners").unwrap_or(&0),
                    kinds.get("player clip brush").unwrap_or(&0),
                    kinds.get("solid brush").unwrap_or(&0),
                    kinds.get("world mesh").unwrap_or(&0)
                ));
            }
            // SAFETY: as above.
            unsafe {
                std::env::remove_var("BO2MP_T7_OLD_CLIP_AXES");
                std::env::remove_var("BO2MP_T7_OLD_BRUSH_SIDES");
            }
        }
        println!("summary:");
        for s in &summary {
            println!("  {s}");
        }
    }

    /// bo2mp: each listed static model's collision triangles against its
    /// drawn model: the placements' axes and scales, and how far the
    /// collision corners stand outside the drawn model's box.
    #[test]
    #[ignore]
    fn t7_smodel_fit() {
        use std::collections::BTreeMap;
        let Ok(paths) = std::env::var("T7_TEST_MAP") else { return };
        for path in paths.split(',') {
            let map = super::T7Map::load(&super::MapSources::beside(std::path::Path::new(path))).expect("map");
            let clip = map.clip.as_ref().expect("clip");
            println!("== {path}");
            let place = |p: [f32; 3], o: [f32; 3], a: [[f32; 3]; 3], sc: f32| -> [f32; 3] { std::array::from_fn(|k| o[k] + (p[0] * a[0][k] + p[1] * a[1][k] + p[2] * a[2][k]) * sc) };
            let (mut matched, mut axis_off, mut scale_off, mut transposed) = (0usize, 0usize, 0usize, 0usize);
            let mut worst: Vec<(f32, String, usize, usize)> = Vec::new();
            let mut by_bone: BTreeMap<String, usize> = BTreeMap::new();
            for (ci, sm) in clip.static_models.iter().enumerate() {
                // (Several placements can share an origin, a tank's quarters:
                // the one whose axes are nearest, read either way.)
                let diff = |d: &fastfile_t7::map::StaticModel| {
                    let f = |t: bool| (0..9).map(|i| (d.axis[i / 3][i % 3] - if t { sm.axis[i % 3][i / 3] } else { sm.axis[i / 3][i % 3] }).abs()).fold(0.0f32, f32::max);
                    f(false).min(f(true))
                };
                let Some(d) = map
                    .static_models
                    .iter()
                    .filter(|d| map.models[d.model].name == sm.name && (0..3).all(|k| (d.origin[k] - sm.origin[k]).abs() < 0.5))
                    .min_by(|a, b| diff(a).total_cmp(&diff(b)))
                else {
                    continue;
                };
                matched += 1;
                let ad = (0..3).flat_map(|r| (0..3).map(move |c| (r, c))).map(|(r, c)| (d.axis[r][c] - sm.axis[r][c]).abs()).fold(0.0f32, f32::max);
                let at = (0..3).flat_map(|r| (0..3).map(move |c| (r, c))).map(|(r, c)| (d.axis[r][c] - sm.axis[c][r]).abs()).fold(0.0f32, f32::max);
                if ad > 0.01 {
                    axis_off += 1;
                    if at <= 0.01 {
                        transposed += 1;
                    } else if axis_off - transposed < 4 {
                        println!("    axes {} draw {:?} clip {:?}", sm.name, d.axis, sm.axis);
                    }
                }
                if (d.scale - sm.scale).abs() > 0.01 {
                    scale_off += 1;
                }
                let m = &map.models[d.model];
                let mut lo = [f32::MAX; 3];
                let mut hi = [f32::MIN; 3];
                for s in &m.surfaces {
                    for p in &s.positions {
                        let w = place(*p, d.origin, d.axis, d.scale);
                        for k in 0..3 {
                            lo[k] = lo[k].min(w[k]);
                            hi[k] = hi[k].max(w[k]);
                        }
                    }
                }
                if lo[0] > hi[0] {
                    continue;
                }
                let mut far = 0.0f32;
                let mut out = 0usize;
                let tris = clip.model_collision.get(&sm.name).map(Vec::as_slice).unwrap_or(&[]);
                for t in tris {
                    let bone = usize::try_from(t.bone).ok().and_then(|b| sm.bones.get(b));
                    for p in t.corners {
                        let p = bone.map_or(p, |mm| fastfile_t7::map::bone_to_model(mm, p));
                        let w = place(p, sm.origin, sm.axis, sm.scale);
                        let e = (0..3).map(|k| (lo[k] - w[k]).max(w[k] - hi[k]).max(0.0)).fold(0.0f32, f32::max);
                        if e > 4.0 {
                            out += 1;
                        }
                        far = far.max(e);
                    }
                    if t.bone != 0 {
                        *by_bone.entry(sm.name.clone()).or_default() += 1;
                    }
                }
                worst.push((far, sm.name.clone(), ci, out));
            }
            worst.sort_by(|a, b| b.0.total_cmp(&a.0));
            let bad = worst.iter().filter(|w| w.0 > 8.0).count();
            println!("  clip static models {}, matched to a drawn one {matched}, axes differ {axis_off} (the transpose {transposed}), scale differs {scale_off}, collision past the drawn box by > 8: {bad}", clip.static_models.len());
            for (far, name, ci, out) in worst.iter().take(25) {
                let sm = &clip.static_models[*ci];
                println!("    {far:7.1} {name} corners out {out} origin {:?} bones {} scale {:.2}", sm.origin.map(|v| v as i32), sm.bones.len(), sm.scale);
            }
            println!("  models with collision on a bone other than 0: {}", by_bone.len());
        }
    }

    /// bo2mp: a zbarrier's boards come from its classname's def: Shadows
    /// of Evil's scissor gate entity carries a magic box's five boards
    /// (a prefab's leftovers); its def `zm_zod_gate_scissor` has one, the
    /// gate (offsets as read from `zm_zod.ff`).
    #[test]
    fn zbarrier_boards_come_from_the_def() {
        let ents = "{\n\"zbarriertearAnim1\" \"o_zombie_magic_box_fake_idle_twitch_b\"\n\"zbarrierboardModel1\" \"p6_anim_zm_magic_box_fake\"\n\"zbarrierboardModel2\" \"p6_anim_zm_magic_box\"\n\"zbarrierNumBoards\" \"5\"\n\"origin\" \"2080 -4384 139\"\n\"classname\" \"zbarrier_zm_zod_gate_scissor\"\n}\n{\n\"classname\" \"zbarrier_unknown\"\n\"zbarrierNumBoards\" \"2\"\n}\n";
        let def = |c: &str| {
            (c == "zm_zod_gate_scissor").then(|| {
                vec![
                    (0x38, "1".to_owned()),
                    (0x60, "p7_fxanim_zm_zod_gate_scissor_mod".to_owned()),
                    (0x78, "p7_fxanim_zm_zod_gate_scissor_open_anim".to_owned()),
                    (0x80, "o_zombie_dr_buyable_barrier_lab_desk_idle".to_owned()),
                ]
            })
        };
        let (text, n) = super::zbarriers_from_defs(ents, &def);
        assert_eq!(n, 1);
        let blocks = iw3::ents::parse(&text);
        let gate = &blocks[0];
        let get = |k: &str| gate.fields.iter().find(|(n, _)| n.eq_ignore_ascii_case(k)).map(|(_, v)| v.as_str());
        assert_eq!(get("zbarrierNumBoards"), Some("1"));
        assert_eq!(get("zbarrierboardModel1"), Some("p7_fxanim_zm_zod_gate_scissor_mod"));
        assert_eq!(get("zbarriertearAnim1"), Some("p7_fxanim_zm_zod_gate_scissor_open_anim"));
        assert_eq!(get("zbarrierboardModel2"), None);
        assert_eq!(get("origin"), Some("2080 -4384 139"));
        // (No def in the zones: its own keys stay.)
        assert!(text.contains("zbarrier_unknown\"\n\"zbarrierNumBoards\" \"2\""));
    }

    /// bo2mp: a Black Ops III Zombies map's actors as the game builds them
    /// (`actor_types` -> `t7::map_props`, the path `build_t7_world` takes)
    /// and as the client poses them (`render_anim`'s T6 script models: every
    /// model of the composition in the T6 catalog with a pose source, the
    /// clip by name in the T6 animations, its tracks bound by bone name): a
    /// zombie that would draw in its bind pose (the PC's T-posed zombies)
    /// fails. `T7_ZM_ZONE=<zm_zod.ff> T7_ZM_ENTS=<its entity string> cargo
    /// test -p assets --lib zombie_actors_pose -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn zombie_actors_pose_with_their_clips() {
        let (Ok(zone), Ok(ents)) = (std::env::var("T7_ZM_ZONE"), std::env::var("T7_ZM_ENTS")) else { return };
        let entities = std::fs::read_to_string(ents).expect("entities");
        let actors = super::actor_types(&entities);
        println!("actor types: {actors:?}");
        let props = super::super::t7::map_props(std::path::Path::new(&zone), None, &[], &[], &actors, Default::default());
        for l in &props.report {
            println!("report: {l}");
        }
        let ns = asset_core::AssetNamespace::T6;
        // (A model whose mesh's xpak is not staged here: its pose source
        // from its skeleton in the zone, `T7_ZM_SKELETONS=1`.)
        let z = fastfile_t7::Zone::open(std::path::Path::new(&zone)).expect("zone");
        let w = fastfile_t7::walk::walk_with(&z, fastfile_t7::walk::Options { resync: true, ..Default::default() });
        let skeletons = std::env::var_os("T7_ZM_SKELETONS").is_some();
        let src = |m: &str| {
            props.fpv.get(ns, m).and_then(|e| e.skel.pose.clone()).or_else(|| {
                skeletons.then(|| w.capture.model(m).or_else(|| w.capture.model(&format!(",{m}"))).and_then(|r| super::super::t7::skeleton_pose_src(&z, r))).flatten()
            })
        };
        let walk = props.anims.iter().map(|a| a.name.clone()).find(|n| n.starts_with("ai_zombie_base_walk_ad_v") || n.starts_with("bo3_ai_zombie_walk_ad_v"));
        println!("walk clip: {walk:?}");
        let clip = walk.as_deref().and_then(|w| props.xanims.clip(ns, w));
        let mut bad = Vec::new();
        for cast in &props.cast {
            for ch in &cast.characters {
                let mut models = vec![ch.body.clone()];
                models.extend(ch.heads.first().cloned());
                models.extend(ch.attached.iter().cloned());
                let missing: Vec<&String> = models.iter().filter(|m| src(m).is_none()).collect();
                let Some(body) = src(&ch.body) else {
                    bad.push(format!("{}: body {} has no pose source", cast.aitype, ch.body));
                    continue;
                };
                let Some(clip) = clip.as_ref() else { continue };
                // (Its head joined as the client joins it: an empty tag,
                // at the body's bone of the head's root.)
                let head = ch.heads.first().and_then(|h| src(h));
                let mut parts: Vec<(&xmodel_runtime::ModelPoseSrc, Option<xmodel_runtime::Attach>)> = vec![(&body, None)];
                if let Some(h) = head.as_ref() {
                    parts.push((h, Some(xmodel_runtime::Attach { parent_model: 0, tag: xmodel_runtime::empty_tag_attach(&body, h) })));
                }
                let dobj = xmodel_runtime::DObj::build(&parts).expect("dobj");
                let tracks: Vec<Option<usize>> = clip.tracks.iter().map(|t| dobj.find(&t.name)).collect();
                let bound = tracks.iter().flatten().count();
                let inst = xmodel_runtime::AnimInstance { clip, tracks: &tracks, time: clip.duration() * 0.3, weight: 1.0, parts: None };
                let bind = dobj.pose(&[], &dobj.all_parts(), glam::Mat4::IDENTITY);
                let posed = dobj.pose(&[inst], &dobj.all_parts(), glam::Mat4::IDENTITY);
                let moved = bind.iter().zip(&posed).filter(|(a, b)| !a.abs_diff_eq(**b, 0.01)).count();
                println!(
                    "{} / {}: body {} ({} bones), {bound} of {} clip tracks bound, {moved} bones moved; not posable: {missing:?}",
                    cast.aitype,
                    ch.name,
                    ch.body,
                    dobj.bind_locals().len(),
                    clip.tracks.len()
                );
                if moved == 0 || !missing.is_empty() {
                    bad.push(format!("{} / {}: {moved} bones moved, not posable {missing:?}", cast.aitype, ch.name));
                }
            }
        }
        assert!(clip.is_some(), "no walk clip built");
        assert!(bad.is_empty(), "zombies that would draw in their bind pose: {bad:#?}");
    }

    /// bo2mp: a map's spawns on its collision, and what holds them in solid
    /// (`T7_TEST_MAP=<zone.ff> cargo test -p assets --lib spawns_on_t7 -- --ignored --nocapture`).
    #[test]
    #[ignore]
    fn spawns_on_t7_collision() {
        let Ok(path) = std::env::var("T7_TEST_MAP") else { return };
        let map = super::T7Map::load(&super::MapSources::beside(std::path::Path::new(&path))).expect("map");
        let entities = iw3::ents::parse(&map.entities);
        let spawns: Vec<[f32; 3]> = super::spawns_of(&entities).iter().map(|s| s.origin).collect();
        let hidden = vec![false; map.surfaces.len()];
        let mut report = Vec::new();
        let full = super::collision_of(&map, &hidden, &mut report);
        println!("{report:?}");
        println!("full: {}", asset_world::world_iw3::spawn_ground_census(&full, &spawns));
        let mut brushes_only = full.clone();
        brushes_only.mesh = std::sync::Arc::new(Default::default());
        println!("brushes only: {}", asset_world::world_iw3::spawn_ground_census(&brushes_only, &spawns));
        // The brushes holding the first spawn's box at its floor.
        let o = spawns[0];
        println!("spawn 0 at {o:?}");
        let mins = [-15.0f32, -15.0, 0.0];
        let maxs = [15.0f32, 15.0, 70.0];
        let mut holding = Vec::new();
        for (i, b) in full.brushes.iter().enumerate() {
            let inside = b.planes.iter().all(|p| {
                // The box's corner furthest against the plane.
                let c: [f32; 3] = std::array::from_fn(|k| o[k] + if p[k] < 0.0 { maxs[k] } else { mins[k] });
                p[0] * c[0] + p[1] * c[1] + p[2] * c[2] - p[3] <= 0.0
            });
            if inside && !b.planes.is_empty() {
                holding.push((i, b.contents, b.planes.len()));
            }
        }
        println!("brushes the box at spawn 0 overlaps: {} {:?}", holding.len(), &holding[..holding.len().min(20)]);
        for &(i, c, _) in holding.iter().take(8) {
            println!("  brush {i} contents {c:#x} planes {:?}", &full.brushes[i].planes[..full.brushes[i].planes.len().min(8)]);
        }
        let clip = map.clip.as_ref().unwrap();
        let mut by_contents: std::collections::BTreeMap<u32, usize> = Default::default();
        for b in &clip.brushes {
            *by_contents.entry(b.contents).or_default() += 1;
        }
        println!("brush contents: {by_contents:x?}");
        // Census with only solid-contents brushes and no mesh, and the mesh without the placed models.
        let mut solid_only = brushes_only.clone();
        for b in &mut solid_only.brushes {
            if b.contents & 1 == 0 {
                b.contents = 0;
            }
        }
        println!("solid brushes only: {}", asset_world::world_iw3::spawn_ground_census(&solid_only, &spawns));
        let (mesh_only, _) = asset_world::world_t7::build_t7_clip_collision(
            &asset_world::world_t7::T7ClipInput { brushes: Vec::new(), nodes: Vec::new(), leaf_brushes: Vec::new(), cmodels: Vec::new(), ..Default::default() },
            &[],
        );
        let _ = mesh_only;
        for want in [3310u16, 3270] {
            let owners: Vec<usize> = clip.models.iter().enumerate().filter(|(_, m)| m.leaf.brushes.contains(&want)).map(|(i, _)| i).collect();
            let leaves = clip.leafs.iter().filter(|l| l.brushes.contains(&want)).count();
            println!("brush {want}: in sub-models {owners:?}, in {leaves} leaves");
        }
        // Brushes whose box holds most spawns.
        let mut big = Vec::new();
        for (i, b) in full.brushes.iter().enumerate() {
            if b.planes.len() < 6 { continue; }
            let n = spawns.iter().filter(|o| b.planes[..6].iter().all(|p| p[0] * o[0] + p[1] * o[1] + p[2] * o[2] - p[3] <= 0.0)).count();
            if n * 4 > spawns.len() { big.push((i, b.contents, n)); }
        }
        println!("brushes holding a quarter of the spawns or more: {big:?}");
        for (i, m) in clip.models.iter().enumerate().take(4) {
            println!("sub-model {i}: {} brushes, bounds {:?}..{:?}", m.leaf.brushes.len(), m.mins, m.maxs);
        }
    }

    /// bo2mp: invisible walls: from every spawn, a player box swept out
    /// in 24 directions; a hit with nothing drawn near it is a wall the
    /// player cannot see, put down to a brush (its contents) or a placed
    /// model's collision mesh (`T7_TEST_MAP=<zone.ff> cargo test --release
    /// -p assets --lib invisible_walls -- --ignored --nocapture`).
    #[test]
    #[ignore]
    fn invisible_walls_t7() {
        use std::collections::{BTreeMap, HashMap};
        let Ok(path) = std::env::var("T7_TEST_MAP") else { return };
        let map = super::T7Map::load(&super::MapSources::beside(std::path::Path::new(&path))).expect("map");
        let zone = fastfile_t7::Zone::open(std::path::Path::new(&path)).expect("zone");
        let walk = fastfile_t7::walk::walk(&zone);
        let entities = iw3::ents::parse(&map.entities);
        let spawns: Vec<[f32; 3]> = super::spawns_of(&entities).iter().map(|s| s.origin).collect();
        let hidden = vec![false; map.surfaces.len()];
        let mut report = Vec::new();
        let full = super::collision_of(&map, &hidden, &mut report);
        println!("{report:?}");
        let mut brushes_only = full.clone();
        brushes_only.mesh = std::sync::Arc::new(Default::default());
        // Drawn triangles in 32-unit cells.
        const CELL: f32 = 32.0;
        let key = |p: [f32; 3]| [(p[0] / CELL).floor() as i32, (p[1] / CELL).floor() as i32, (p[2] / CELL).floor() as i32];
        let mut grid: HashMap<[i32; 3], Vec<[[f32; 3]; 3]>> = HashMap::new();
        let add = |t: [[f32; 3]; 3], grid: &mut HashMap<[i32; 3], Vec<[[f32; 3]; 3]>>| {
            let lo: [f32; 3] = std::array::from_fn(|k| t[0][k].min(t[1][k]).min(t[2][k]));
            let hi: [f32; 3] = std::array::from_fn(|k| t[0][k].max(t[1][k]).max(t[2][k]));
            let (a, b) = (key(lo), key(hi));
            if (b[0] - a[0]) as i64 * (b[1] - a[1]) as i64 * (b[2] - a[2]) as i64 > 4096 {
                return;
            }
            for x in a[0]..=b[0] {
                for y in a[1]..=b[1] {
                    for z in a[2]..=b[2] {
                        grid.entry([x, y, z]).or_default().push(t);
                    }
                }
            }
        };
        for s in &map.surfaces {
            for t in 0..s.tri_count as usize {
                let v = |k: usize| {
                    let ix = map.indices.get(s.base_index as usize + 3 * t + k).copied().unwrap_or(0);
                    map.vertices.get((s.first_vertex + u32::from(ix)) as usize).map_or([0.0; 3], |v| v.xyz)
                };
                add([v(0), v(1), v(2)], &mut grid);
            }
        }
        let place = |p: [f32; 3], o: [f32; 3], a: [[f32; 3]; 3], sc: f32| -> [f32; 3] { std::array::from_fn(|k| o[k] + (p[0] * a[0][k] + p[1] * a[1][k] + p[2] * a[2][k]) * sc) };
        for sm in &map.static_models {
            for s in &map.models[sm.model].surfaces {
                for t in &s.tris {
                    let v = |k: usize| place(s.positions[t[k] as usize], sm.origin, sm.axis, sm.scale);
                    add([v(0), v(1), v(2)], &mut grid);
                }
            }
        }
        fn closest(p: [f32; 3], t: &[[f32; 3]; 3]) -> f32 {
            // Distance to the triangle's points, edge midpoints and plane (inside).
            let sub = |a: [f32; 3], b: [f32; 3]| [a[0] - b[0], a[1] - b[1], a[2] - b[2]];
            let dot = |a: [f32; 3], b: [f32; 3]| a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
            let cross = |a: [f32; 3], b: [f32; 3]| [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]];
            let seg = |a: [f32; 3], b: [f32; 3]| {
                let ab = sub(b, a);
                let l = dot(ab, ab).max(1e-6);
                let u = (dot(sub(p, a), ab) / l).clamp(0.0, 1.0);
                let q = [a[0] + ab[0] * u, a[1] + ab[1] * u, a[2] + ab[2] * u];
                dot(sub(p, q), sub(p, q)).sqrt()
            };
            let mut d = seg(t[0], t[1]).min(seg(t[1], t[2])).min(seg(t[2], t[0]));
            let n = cross(sub(t[1], t[0]), sub(t[2], t[0]));
            let nl = dot(n, n).sqrt();
            if nl > 1e-6 {
                let n = [n[0] / nl, n[1] / nl, n[2] / nl];
                let h = dot(sub(p, t[0]), n);
                let q = [p[0] - n[0] * h, p[1] - n[1] * h, p[2] - n[2] * h];
                let inside = (0..3).all(|i| dot(cross(sub(t[(i + 1) % 3], t[i]), sub(q, t[i])), n) >= 0.0);
                if inside {
                    d = d.min(h.abs());
                }
            }
            d
        }
        let near_drawn = |p: [f32; 3]| -> f32 {
            let c = key(p);
            let mut best = f32::MAX;
            for x in c[0] - 1..=c[0] + 1 {
                for y in c[1] - 1..=c[1] + 1 {
                    for z in c[2] - 1..=c[2] + 1 {
                        if let Some(ts) = grid.get(&[x, y, z]) {
                            for t in ts {
                                best = best.min(closest(p, t));
                            }
                        }
                    }
                }
            }
            best
        };
        // Placed collision models (for naming a mesh hit).
        let by_name: HashMap<&str, usize> = map.models.iter().enumerate().map(|(i, m)| (m.name.as_str(), i)).collect();
        let clip = map.clip.as_ref().unwrap();
        let mut coll_tris: HashMap<[i32; 3], Vec<(usize, [[f32; 3]; 3])>> = HashMap::new();
        for (ci, sm) in clip.static_models.iter().enumerate() {
            let Some(&i) = by_name.get(sm.name.as_str()) else { continue };
            let m = &map.models[i];
            let surfaces = if m.coarse.is_empty() { &m.surfaces } else { &m.coarse };
            for s in surfaces {
                for t in &s.tris {
                    let v = |k: usize| place(s.positions[t[k] as usize], sm.origin, sm.axis, sm.scale);
                    let tri = [v(0), v(1), v(2)];
                    let lo: [f32; 3] = std::array::from_fn(|k| tri[0][k].min(tri[1][k]).min(tri[2][k]));
                    let hi: [f32; 3] = std::array::from_fn(|k| tri[0][k].max(tri[1][k]).max(tri[2][k]));
                    let (a, b) = (key(lo), key(hi));
                    for x in a[0]..=b[0] {
                        for y in a[1]..=b[1] {
                            for z in a[2]..=b[2] {
                                coll_tris.entry([x, y, z]).or_default().push((ci, tri));
                            }
                        }
                    }
                }
            }
        }
        const MASK: u32 = 0x0281_0011;
        let (mins, maxs) = ([-15.0f32, -15.0, 0.0], [15.0f32, 15.0, 70.0]);
        let mut by_source: BTreeMap<String, (usize, Vec<[i32; 3]>)> = BTreeMap::new();
        let mut v_all: Vec<(String, Vec<[i32; 3]>)> = Vec::new();
        let mut all_hits: Vec<[i32; 3]> = Vec::new();
        let mut brush_ids: std::collections::BTreeSet<usize> = Default::default();
        let (mut hits, mut invisible) = (0usize, 0usize);
        for o in &spawns {
            let down = full.sweep_box([o[0], o[1], o[2] + 32.0], [o[0], o[1], o[2] - 128.0], mins, maxs, MASK);
            if down.startsolid {
                continue;
            }
            let g = down.endpos;
            let start = [g[0], g[1], g[2] + 19.0];
            for k in 0..24 {
                let a = k as f32 * std::f32::consts::TAU / 24.0;
                let end = [start[0] + a.cos() * 400.0, start[1] + a.sin() * 400.0, start[2]];
                let h = full.sweep_box(start, end, mins, maxs, MASK);
                if h.startsolid || h.fraction >= 1.0 || h.normal[2] > 0.7 {
                    continue;
                }
                hits += 1;
                // The touched point: the box's face against the wall, at chest height.
                let n = h.normal;
                let mut best = f32::MAX;
                for zz in [8.0f32, 24.0, 40.0, 56.0] {
                    let p = [h.endpos[0] - n[0] * 15.5, h.endpos[1] - n[1] * 15.5, h.endpos[2] + zz];
                    best = best.min(near_drawn(p));
                }
                if best <= 12.0 {
                    continue;
                }
                invisible += 1;
                let p = [h.endpos[0] - n[0] * 15.5, h.endpos[1] - n[1] * 15.5, h.endpos[2] + 30.0];
                let b = brushes_only.sweep_box(start, end, mins, maxs, MASK);
                let source = if (b.fraction - h.fraction).abs() < 1e-3 {
                    // The brush the box stands against.
                    let e = h.endpos;
                    let mut found = None;
                    for (i, br) in full.brushes.iter().enumerate() {
                        if br.planes.is_empty() || br.contents & MASK == 0 {
                            continue;
                        }
                        let touch = br.planes.iter().all(|pl| {
                            let c: [f32; 3] = std::array::from_fn(|k| e[k] - n[k] * 0.5 + if pl[k] < 0.0 { maxs[k] } else { mins[k] });
                            pl[0] * c[0] + pl[1] * c[1] + pl[2] * c[2] - pl[3] <= 1.0
                        });
                        if touch {
                            let mut lo = [f32::MIN; 3];
                            let mut hi = [f32::MAX; 3];
                            for pl in &br.planes {
                                for k in 0..3 {
                                    if (pl[k] - 1.0).abs() < 1e-4 { hi[k] = hi[k].min(pl[3]); }
                                    if (pl[k] + 1.0).abs() < 1e-4 { lo[k] = lo[k].max(-pl[3]); }
                                }
                            }
                            brush_ids.insert(i);
                            found = Some(format!("brush {i} contents {:#x} box {:?}..{:?} planes {}", br.contents, lo.map(|v| v as i32), hi.map(|v| v as i32), br.planes.len()));
                            break;
                        }
                    }
                    found.unwrap_or_else(|| "brush (unfound)".into())
                } else {
                    let c = key(p);
                    let mut best: Option<(f32, usize)> = None;
                    for x in c[0] - 1..=c[0] + 1 {
                        for y in c[1] - 1..=c[1] + 1 {
                            for z in c[2] - 2..=c[2] + 1 {
                                for (ci, t) in coll_tris.get(&[x, y, z]).into_iter().flatten() {
                                    let d = closest(p, t);
                                    if best.is_none_or(|(bd, _)| d < bd) {
                                        best = Some((d, *ci));
                                    }
                                }
                            }
                        }
                    }
                    match best {
                        Some((d, ci)) if d < 48.0 => {
                            let sm = &clip.static_models[ci];
                            let has_coll = walk.capture.model(&sm.name).or_else(|| walk.capture.model(&format!(",{}", sm.name))).is_some_and(|x| x.collision_mesh.is_some());
                            format!("model {} contents {:#x} collision_lod {has_coll}", sm.name, sm.contents)
                        }
                        _ => "clip mesh".into(),
                    }
                };
                all_hits.push([p[0] as i32, p[1] as i32, p[2] as i32]);
                let e = by_source.entry(source).or_default();
                e.0 += 1;
                if e.1.len() < 4 {
                    e.1.push([p[0] as i32, p[1] as i32, p[2] as i32]);
                }
            }
        }
        v_all.push(("all".into(), all_hits.clone()));
        println!("spawns {}, wall hits {hits}, invisible {invisible}", spawns.len());
        // T7_PROBE_PPM=<file>: a plan of the play area (4 units a pixel):
        // drawn triangles grey by height (floor-level ones dark), invisible
        // hits red, the brushes behind them outlined yellow, spawns green.
        if let Ok(out) = std::env::var("T7_PROBE_PPM") {
            let (x0, y0, w, h) = (-2600.0f32, -1300.0f32, 1300usize, 750usize);
            let mut img = vec![[20u8, 20, 40]; w * h];
            let px = |x: f32, y: f32| -> Option<usize> {
                let (i, j) = (((x - x0) / 4.0) as isize, (h as isize - 1) - ((y - y0) / 4.0) as isize);
                (i >= 0 && j >= 0 && (i as usize) < w && (j as usize) < h).then(|| j as usize * w + i as usize)
            };
            let mut top = vec![f32::MIN; w * h];
            for ts in grid.values() {
                for t in ts {
                    let zc = (t[0][2] + t[1][2] + t[2][2]) / 3.0;
                    if zc > 250.0 { continue; }
                    // Rasterise roughly: points across the triangle.
                    for a in 0..=6 {
                        for b in 0..=(6 - a) {
                            let (u, v) = (a as f32 / 6.0, b as f32 / 6.0);
                            let q: [f32; 3] = std::array::from_fn(|k| t[0][k] + (t[1][k] - t[0][k]) * u + (t[2][k] - t[0][k]) * v);
                            if let Some(i) = px(q[0], q[1]) { top[i] = top[i].max(q[2]); }
                        }
                    }
                }
            }
            for i in 0..w * h {
                if top[i] > f32::MIN {
                    let g = (60.0 + (top[i] + 60.0).clamp(0.0, 300.0) * 0.6) as u8;
                    img[i] = [g, g, g];
                }
            }
            for (s, at) in &v_all {
                let _ = s;
                for p in at {
                    if let Some(i) = px(p[0] as f32, p[1] as f32) { img[i] = [255, 0, 0]; }
                }
            }
            for &bi in &brush_ids {
                let br = &full.brushes[bi];
                let mut lo = [f32::MIN; 3];
                let mut hi = [f32::MAX; 3];
                for pl in &br.planes {
                    for k in 0..3 {
                        if (pl[k] - 1.0).abs() < 1e-4 { hi[k] = hi[k].min(pl[3]); }
                        if (pl[k] + 1.0).abs() < 1e-4 { lo[k] = lo[k].max(-pl[3]); }
                    }
                }
                let mut x = lo[0];
                while x <= hi[0] {
                    for y in [lo[1], hi[1]] { if let Some(i) = px(x, y) { img[i] = [255, 220, 0]; } }
                    x += 2.0;
                }
                let mut y = lo[1];
                while y <= hi[1] {
                    for x in [lo[0], hi[0]] { if let Some(i) = px(x, y) { img[i] = [255, 220, 0]; } }
                    y += 2.0;
                }
            }
            for o in &spawns {
                if let Some(i) = px(o[0], o[1]) { img[i] = [0, 255, 0]; }
            }
            let mut f = format!("P6 {w} {h} 255\n").into_bytes();
            for p in &img { f.extend_from_slice(p); }
            std::fs::write(out, f).expect("ppm");
        }
        let mut v: Vec<_> = by_source.into_iter().collect();
        v.sort_by_key(|(_, (n, _))| std::cmp::Reverse(*n));
        for (s, (n, at)) in v.iter().take(60) {
            println!("{n:5} {s} at {at:?}");
        }
        // Spawns whose standing box a brush holds, by the brush's contents
        // (a brush that holds the map's own spawns cannot stop players).
        let mut held: BTreeMap<u32, (usize, usize)> = BTreeMap::new();
        for br in &full.brushes {
            if br.planes.is_empty() || br.contents & MASK == 0 {
                continue;
            }
            let e = held.entry(br.contents).or_default();
            e.0 += 1;
            let n = spawns
                .iter()
                .filter(|o| {
                    br.planes.iter().all(|pl| {
                        let c: [f32; 3] = std::array::from_fn(|k| o[k] + if pl[k] < 0.0 { maxs[k] } else { mins[k] });
                        pl[0] * c[0] + pl[1] * c[1] + pl[2] * c[2] - pl[3] <= -1.0
                    })
                })
                .count();
            e.1 += n;
        }
        println!("brushes by contents (count, spawns held): {held:x?}");
        // Collision models without a collision LOD, by name.
        let mut no_lod: BTreeMap<&str, usize> = BTreeMap::new();
        for sm in &clip.static_models {
            let has = walk.capture.model(&sm.name).or_else(|| walk.capture.model(&format!(",{}", sm.name))).is_some_and(|x| x.collision_mesh.is_some());
            if !has {
                *no_lod.entry(sm.name.as_str()).or_default() += 1;
            }
        }
        println!("clip static models {}, without a collision LOD {} ({} kinds)", clip.static_models.len(), no_lod.values().sum::<usize>(), no_lod.len());
    }

    #[test]
    fn bomb_clips_left_out_and_script_models_listed() {
        let ents = "{\n\"classname\" \"worldspawn\"\n}\n{\n\"model\" \"*2\"\n\"script_gameobjectname\" \"bombzone_dem\"\n\"classname\" \"script_brushmodel\"\n}\n{\n\"model\" \"*37\"\n\"targetname\" \"pf1159_auto1\"\n\"classname\" \"script_brushmodel\"\n}\n{\n\"model\" \"p7_dest_a\"\n\"classname\" \"script_model\"\n}\n{\n\"model\" \"p7_dest_a\"\n\"classname\" \"script_model\"\n}\n";
        let kept = super::without_bomb_clips(ents);
        assert!(!kept.contains("bombzone_dem") && kept.contains("pf1159_auto1") && kept.contains("worldspawn"));
        assert_eq!(super::plain_script_models(ents), vec!["p7_dest_a".to_owned()]);
        let with_target = "{\n\"model\" \"*37\"\n\"targetname\" \"pf1\"\n\"classname\" \"script_brushmodel\"\n}\n{\n\"model\" \"p7_dest_f\"\n\"target\" \"pf1\"\n\"classname\" \"script_model\"\n}\n{\n\"model\" \"*38\"\n\"targetname\" \"pf2\"\n\"classname\" \"script_brushmodel\"\n}\n";
        let (kept, n) = super::without_clips_of(with_target, &|m| m == "p7_dest_f");
        assert_eq!(n, 1);
        assert!(!kept.contains("\"targetname\" \"pf1\"") && kept.contains("\"targetname\" \"pf2\"") && kept.contains("p7_dest_f"));
    }

    #[test]
    fn halves_and_unit_vectors() {
        for x in [0.0f32, 1.0, -1.0, 0.5, 2.25, -3.75, 1024.0] {
            assert_eq!(asset_model::half_to_f32(super::f16_bits(x)), x);
        }
        let v = super::t6_unit_vec([0.6, 0.0, -0.8]);
        assert_eq!(((v & 0x3ff) as i32) << 22 >> 22, 307);
        let t = super::tangent_of([0.0, 0.0, 1.0]);
        assert!(t[2].abs() < 1e-6 && (t[0] * t[0] + t[1] * t[1] - 1.0).abs() < 1e-5);
    }

    /// bo2mp: fall-throughs: a player box walked from every spawn (random
    /// walks, 16-unit steps with BO2's 18-unit step up) over the map's
    /// collision; a step whose box finds no collision floor where a drawn
    /// floor lies under its centre is a hole the player falls through. The
    /// holes are listed by the drawn surface's material (or placed model)
    /// with the contents bits a trace of every bit finds there (`T7_TEST_MAP=
    /// <zone.ff>[,<zone.ff>...] cargo test -p assets --lib fall_through_t7 --
    /// --ignored --nocapture`).
    #[test]
    #[ignore]
    fn fall_through_t7() {
        use std::collections::{BTreeMap, HashMap};
        let Ok(paths) = std::env::var("T7_TEST_MAP") else { return };
        let walks: usize = std::env::var("T7_WALKS").ok().and_then(|v| v.parse().ok()).unwrap_or(6);
        for path in paths.split(',') {
            let map = super::T7Map::load(&super::MapSources::beside(std::path::Path::new(path))).expect("map");
            let entities = iw3::ents::parse(&map.entities);
            let spawns: Vec<[f32; 3]> = super::spawns_of(&entities).iter().map(|s| s.origin).collect();
            let hidden: Vec<bool> = map
                .surfaces
                .iter()
                .map(|s| s.material.is_some_and(|m| super::hidden_kind(&map.materials[m].name, &map.materials[m].technique_set).is_some()))
                .collect();
            let mut report = Vec::new();
            let full = super::collision_of(&map, &hidden, &mut report);
            println!("== {path}: {report:?}");
            // Drawn floors (up-facing triangles) in 64-unit columns, named.
            const CELL: f32 = 64.0;
            let col = |x: f32, y: f32| [(x / CELL).floor() as i32, (y / CELL).floor() as i32];
            let mut names: Vec<String> = Vec::new();
            let mut grid: HashMap<[i32; 2], Vec<([[f32; 3]; 3], u32)>> = HashMap::new();
            let add = |t: [[f32; 3]; 3], name: u32, grid: &mut HashMap<[i32; 2], Vec<([[f32; 3]; 3], u32)>>| {
                // Floors only: up-facing enough to stand on (either winding).
                let e1 = [t[1][0] - t[0][0], t[1][1] - t[0][1], t[1][2] - t[0][2]];
                let e2 = [t[2][0] - t[0][0], t[2][1] - t[0][1], t[2][2] - t[0][2]];
                let n = [e1[1] * e2[2] - e1[2] * e2[1], e1[2] * e2[0] - e1[0] * e2[2], e1[0] * e2[1] - e1[1] * e2[0]];
                if n[2].abs() < 1e-3 || n[2].abs() < 0.7 * (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt() {
                    return;
                }
                let lo = col(t[0][0].min(t[1][0]).min(t[2][0]), t[0][1].min(t[1][1]).min(t[2][1]));
                let hi = col(t[0][0].max(t[1][0]).max(t[2][0]), t[0][1].max(t[1][1]).max(t[2][1]));
                if (hi[0] - lo[0]) as i64 * (hi[1] - lo[1]) as i64 > 4096 {
                    return;
                }
                for x in lo[0]..=hi[0] {
                    for y in lo[1]..=hi[1] {
                        grid.entry([x, y]).or_default().push((t, name));
                    }
                }
            };
            let mut mat_name: HashMap<usize, u32> = HashMap::new();
            for (i, s) in map.surfaces.iter().enumerate() {
                if hidden[i] {
                    continue;
                }
                let Some(m) = s.material else { continue };
                if super::t7_draw(&map.materials[m].name, &map.materials[m].technique_set, false).blend != asset_core::T6Blend::Opaque {
                    continue;
                }
                let n = *mat_name.entry(m).or_insert_with(|| {
                    names.push(format!("world {}", map.materials[m].name));
                    (names.len() - 1) as u32
                });
                for t in 0..s.tri_count as usize {
                    let v = |k: usize| {
                        let ix = map.indices.get(s.base_index as usize + 3 * t + k).copied().unwrap_or(0);
                        map.vertices.get((s.first_vertex + u32::from(ix)) as usize).map_or([0.0; 3], |v| v.xyz)
                    };
                    add([v(0), v(1), v(2)], n, &mut grid);
                }
            }
            let place = |p: [f32; 3], o: [f32; 3], a: [[f32; 3]; 3], sc: f32| -> [f32; 3] { std::array::from_fn(|k| o[k] + (p[0] * a[0][k] + p[1] * a[1][k] + p[2] * a[2][k]) * sc) };
            let mut model_name: HashMap<usize, u32> = HashMap::new();
            for sm in &map.static_models {
                let m = &map.models[sm.model];
                let n = *model_name.entry(sm.model).or_insert_with(|| {
                    names.push(format!("model {}", m.name));
                    (names.len() - 1) as u32
                });
                for s in &m.surfaces {
                    for t in &s.tris {
                        let v = |k: usize| place(s.positions.get(usize::from(t[k])).copied().unwrap_or_default(), sm.origin, sm.axis, sm.scale);
                        add([v(0), v(1), v(2)], n, &mut grid);
                    }
                }
            }
            // (The map is not needed past here: memory for the walk.)
            drop(entities);
            drop(map);
            // The highest drawn floor under (x, y) between z0 and z1.
            let drawn_floor = |x: f32, y: f32, z0: f32, z1: f32| -> Option<(f32, u32)> {
                let mut best: Option<(f32, u32)> = None;
                for (t, n) in grid.get(&col(x, y)).into_iter().flatten() {
                    let [a, b, c] = *t;
                    let d = (b[1] - c[1]) * (a[0] - c[0]) + (c[0] - b[0]) * (a[1] - c[1]);
                    let l1 = ((b[1] - c[1]) * (x - c[0]) + (c[0] - b[0]) * (y - c[1])) / d;
                    let l2 = ((c[1] - a[1]) * (x - c[0]) + (a[0] - c[0]) * (y - c[1])) / d;
                    let l3 = 1.0 - l1 - l2;
                    if l1 < -1e-4 || l2 < -1e-4 || l3 < -1e-4 {
                        continue;
                    }
                    let z = l1 * a[2] + l2 * b[2] + l3 * c[2];
                    // Up-facing enough to stand on.
                    let n3 = {
                        let e1 = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
                        let e2 = [c[0] - a[0], c[1] - a[1], c[2] - a[2]];
                        let n = [e1[1] * e2[2] - e1[2] * e2[1], e1[2] * e2[0] - e1[0] * e2[2], e1[0] * e2[1] - e1[1] * e2[0]];
                        n[2].abs() / (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt()
                    };
                    if n3 < 0.7 || z < z0 || z > z1 {
                        continue;
                    }
                    if best.is_none_or(|(bz, _)| z > bz) {
                        best = Some((z, *n));
                    }
                }
                best
            };
            const MASK: u32 = asset_world::MASK_PLAYER_SOLID;
            let (mins, maxs) = ([-15.0f32, -15.0, 0.0], [15.0f32, 15.0, 70.0]);
            let mut seed = 0x2545_f491_4f6c_dd1du64;
            let mut rand = move || {
                seed ^= seed << 13;
                seed ^= seed >> 7;
                seed ^= seed << 17;
                (seed >> 11) as f32 / (1u64 << 53) as f32
            };
            let mut holes: BTreeMap<[i32; 3], (u32, f32, Option<f32>, u32, [f32; 3])> = BTreeMap::new();
            let (mut steps, mut started) = (0usize, 0usize);
            let every = (spawns.len() / 300).max(1);
            for o in spawns.iter().step_by(every) {
                let down = full.sweep_box([o[0], o[1], o[2] + 32.0], [o[0], o[1], o[2] - 128.0], mins, maxs, MASK);
                if down.startsolid || down.fraction >= 1.0 {
                    continue;
                }
                started += 1;
                for _ in 0..walks {
                    let mut p = down.endpos;
                    let mut a = rand() * std::f32::consts::TAU;
                    for _ in 0..96 {
                        steps += 1;
                        let up = full.sweep_box(p, [p[0], p[1], p[2] + 18.0], mins, maxs, MASK);
                        let s = up.endpos;
                        let f = full.sweep_box(s, [s[0] + a.cos() * 16.0, s[1] + a.sin() * 16.0, s[2]], mins, maxs, MASK);
                        if f.startsolid || f.fraction < 0.1 {
                            a = rand() * std::f32::consts::TAU;
                            continue;
                        }
                        let e = f.endpos;
                        let g = full.sweep_box(e, [e[0], e[1], e[2] - 18.0 - 64.0], mins, maxs, MASK);
                        if g.fraction < 1.0 && g.normal[2] >= 0.7 && !g.startsolid {
                            p = g.endpos;
                            continue;
                        }
                        // No floor for the box within 64 below: a drawn floor
                        // under its centre there is a hole in the collision.
                        if let Some((dz, n)) = drawn_floor(e[0], e[1], p[2] - 64.0, p[2] + 2.0) {
                            let far = full.sweep_box(e, [e[0], e[1], e[2] - 8192.0], mins, maxs, MASK);
                            let col_z = (far.fraction < 1.0).then_some(far.endpos[2]);
                            // (A floor at the drawn one the box only met on
                            // an edge or a slope is not a hole.)
                            if col_z.is_some_and(|z| z > dz - 48.0) {
                                a = rand() * std::f32::consts::TAU;
                                continue;
                            }
                            // Which contents bits stop the box at the drawn floor.
                            let mut bits = 0u32;
                            for b in 0..32 {
                                let h = full.sweep_box(e, [e[0], e[1], dz - 4.0], mins, maxs, 1 << b);
                                if h.fraction < 1.0 && h.endpos[2] > dz - 8.0 {
                                    bits |= 1 << b;
                                }
                            }
                            let key = [(e[0] / 128.0) as i32, (e[1] / 128.0) as i32, (dz / 128.0) as i32];
                            holes.entry(key).or_insert((n, dz, col_z, bits, e));
                        }
                        a = rand() * std::f32::consts::TAU;
                    }
                }
            }
            let mut by_name: BTreeMap<(String, u32), (usize, Vec<String>)> = BTreeMap::new();
            for (n, dz, col_z, bits, e) in holes.values() {
                let v = by_name.entry((names[*n as usize].clone(), *bits)).or_default();
                v.0 += 1;
                if v.1.len() < 3 {
                    v.1.push(format!("({:.0} {:.0} {:.0}) floor drawn {dz:.0}, collision {}", e[0], e[1], e[2], col_z.map_or("none".to_owned(), |z| format!("{z:.0}"))));
                }
            }
            let mut rows: Vec<_> = by_name.into_iter().collect();
            rows.sort_by(|a, b| b.1.0.cmp(&a.1.0));
            println!("{} spawns walked from, {steps} steps, {} holes (128-unit cells)", started, holes.len());
            for ((name, bits), (n, at)) in rows.iter().take(40) {
                println!("  {n:4} {name} (contents bits there {bits:#x}): {}", at.join("; "));
            }
            // The match's own movement (BO2's pmove, 50 ms steps) run by a
            // bot from each spawn: forward, sprinting, turning, jumping and
            // crouching at random; a fall begun over a drawn floor at his
            // feet that ends 200 or more below it went through that floor.
            let pmove_secs: i32 = std::env::var("T7_PMOVE_SECS").ok().and_then(|v| v.parse().ok()).unwrap_or(20);
            let mut falls: BTreeMap<[i32; 3], String> = BTreeMap::new();
            let mut ticks = 0usize;
            let clip = PmoveClip(&full);
            // T7_PMOVE_AT="x y z": 200 runs from that point instead, each
            // fall printed with its last ticks.
            let probe: Option<[f32; 3]> = std::env::var("T7_PMOVE_AT").ok().and_then(|v| {
                let n: Vec<f32> = v.split_whitespace().filter_map(|x| x.parse().ok()).collect();
                (n.len() == 3).then(|| [n[0], n[1], n[2]])
            });
            let starts: Vec<[f32; 3]> = probe.map_or_else(|| spawns.iter().step_by(every).copied().collect(), |p| vec![p; 200]);
            for o in &starts {
                let mut history: std::collections::VecDeque<String> = Default::default();
                let mut ps = playerstate_iw4::PlayerState::ZERO;
                ps.origin = *o;
                ps.speed = 190;
                ps.health = 100;
                ps.max_health = 100;
                ps.move_speed_scale_multiplier = 1.0;
                ps.view_height_target = 60;
                ps.view_height_current = 60.0;
                ps.gravity = 800;
                ps.ground_entity_num = 0x7fe;
                ps.other_flags |= playerstate_iw4::other_flags::PLAYER;
                ps.corpse_index = -1;
                let mut yaw = rand() * 360.0;
                let mut buttons = 0u32;
                let mut old_buttons = 0u32;
                let mut left: Option<([f32; 3], f32, u32)> = None;
                let mut stuck = 0;
                for t in 0..pmove_secs * 20 {
                    ticks += 1;
                    if t % 40 == 0 || stuck > 10 {
                        yaw = rand() * 360.0;
                        stuck = 0;
                        buttons = if rand() < 0.5 { playerstate_iw4::buttons::SPRINT } else { 0 };
                        if rand() < 0.1 {
                            buttons |= playerstate_iw4::buttons::CROUCH;
                        }
                    }
                    let mut b = buttons;
                    if rand() < 0.04 {
                        b |= playerstate_iw4::buttons::JUMP;
                    }
                    yaw += (rand() - 0.5) * 6.0;
                    let mut cmd = playerstate_iw4::UserCmd {
                        server_time: ps.command_time + 50,
                        buttons: b,
                        angles: [0, (yaw.rem_euclid(360.0) * movement_iw4::ANGLE2SHORT) as i32, 0],
                        forwardmove: 127,
                        rightmove: ((rand() - 0.5) * 60.0) as i8,
                        ..Default::default()
                    };
                    let before = ps.origin;
                    let was_ground = ps.ground_entity_num != 0x7ff;
                    movement_iw4::pmove(&mut ps, &mut cmd, pmove_context(old_buttons), &clip, &movement_iw4::FlatMantleAnimLength { up_msec: 500, over_msec: 500 }, &movement_iw4::ZeroMantleRootDelta);
                    old_buttons = b;
                    if probe.is_some() {
                        let d = full.sweep_box(before, ps.origin, [-15.0, -15.0, 0.0], [15.0, 15.0, 70.0], MASK);
                        history.push_back(format!(
                            "t{t} {:?} -> {:?} ground {} vel {:?} pm_flags {:#x} maxs_z {} (sweep of that move: frac {:.3} solid {} n {:?})",
                            before.map(|v| (v * 100.0).round() / 100.0),
                            ps.origin.map(|v| (v * 100.0).round() / 100.0),
                            ps.ground_entity_num,
                            ps.velocity.map(|v| v.round()),
                            ps.pm_flags,
                            ps.view_height_target,
                            d.fraction,
                            d.startsolid,
                            d.normal.map(|v| (v * 100.0).round() / 100.0)
                        ));
                        if history.len() > 8 {
                            history.pop_front();
                        }
                    }
                    let moved = ((ps.origin[0] - before[0]).powi(2) + (ps.origin[1] - before[1]).powi(2)).sqrt();
                    stuck = if moved < 1.0 { stuck + 1 } else { 0 };
                    let on_ground = ps.ground_entity_num != 0x7ff;
                    if on_ground {
                        left = None;
                    } else if was_ground {
                        // Left the ground here: a drawn floor at his feet?
                        left = drawn_floor(before[0], before[1], before[2] - 40.0, before[2] + 2.0).map(|(z, n)| (before, z, n));
                    }
                    if let Some((at, dz, n)) = left
                        && ps.origin[2] < dz - 200.0
                    {
                        let far = full.sweep_box(at, [at[0], at[1], at[2] - 8192.0], [-15.0, -15.0, 0.0], [15.0, 15.0, 70.0], MASK);
                        let key = [(at[0] / 128.0) as i32, (at[1] / 128.0) as i32, (dz / 128.0) as i32];
                        falls.entry(key).or_insert_with(|| {
                            format!(
                                "({:.0} {:.0} {:.0}) over {} drawn at {dz:.0}; collision under it {}",
                                at[0],
                                at[1],
                                at[2],
                                names[n as usize],
                                if far.fraction < 1.0 { format!("{:.0}{}", far.endpos[2], if far.startsolid { " (in solid)" } else { "" }) } else { "none".into() }
                            )
                        });
                        left = None;
                        if probe.is_some() {
                            println!("fall:");
                            for h in &history {
                                println!("    {h}");
                            }
                        }
                    }
                    if ps.origin[2] < o[2] - 4096.0 {
                        break;
                    }
                }
            }
            println!("pmove: {ticks} ticks from {} starts, {} falls through a drawn floor", starts.len(), falls.len());
            for f in falls.values().take(40) {
                println!("  {f}");
            }
        }
    }

    /// The drawn floors (up-facing opaque world triangles and placed
    /// models' drawn triangles) in 64-unit columns, each named by its
    /// material or model.
    pub(crate) struct DrawnFloors {
        pub(crate) names: Vec<String>,
        grid: std::collections::HashMap<[i32; 2], Vec<([[f32; 3]; 3], u32)>>,
    }

    impl DrawnFloors {
        const CELL: f32 = 64.0;

        fn col(x: f32, y: f32) -> [i32; 2] {
            [(x / Self::CELL).floor() as i32, (y / Self::CELL).floor() as i32]
        }

        fn of(map: &super::T7Map, hidden: &[bool]) -> Self {
            let mut out = Self { names: Vec::new(), grid: Default::default() };
            let mut mat_name: std::collections::HashMap<usize, u32> = Default::default();
            for (i, s) in map.surfaces.iter().enumerate() {
                if hidden[i] {
                    continue;
                }
                let Some(m) = s.material else { continue };
                if super::t7_draw(&map.materials[m].name, &map.materials[m].technique_set, false).blend != asset_core::T6Blend::Opaque {
                    continue;
                }
                let n = *mat_name.entry(m).or_insert_with(|| {
                    out.names.push(format!("world {}", map.materials[m].name));
                    (out.names.len() - 1) as u32
                });
                for t in 0..s.tri_count as usize {
                    let v = |k: usize| {
                        let ix = map.indices.get(s.base_index as usize + 3 * t + k).copied().unwrap_or(0);
                        map.vertices.get((s.first_vertex + u32::from(ix)) as usize).map_or([0.0; 3], |v| v.xyz)
                    };
                    out.add([v(0), v(1), v(2)], n);
                }
            }
            let place = |p: [f32; 3], o: [f32; 3], a: [[f32; 3]; 3], sc: f32| -> [f32; 3] { std::array::from_fn(|k| o[k] + (p[0] * a[0][k] + p[1] * a[1][k] + p[2] * a[2][k]) * sc) };
            let mut model_name: std::collections::HashMap<usize, u32> = Default::default();
            for sm in &map.static_models {
                let m = &map.models[sm.model];
                let n = *model_name.entry(sm.model).or_insert_with(|| {
                    out.names.push(format!("model {}", m.name));
                    (out.names.len() - 1) as u32
                });
                for s in &m.surfaces {
                    for t in &s.tris {
                        let v = |k: usize| place(s.positions.get(usize::from(t[k])).copied().unwrap_or_default(), sm.origin, sm.axis, sm.scale);
                        out.add([v(0), v(1), v(2)], n);
                    }
                }
            }
            out
        }

        fn up(t: &[[f32; 3]; 3]) -> f32 {
            let [a, b, c] = *t;
            let e1 = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
            let e2 = [c[0] - a[0], c[1] - a[1], c[2] - a[2]];
            let n = [e1[1] * e2[2] - e1[2] * e2[1], e1[2] * e2[0] - e1[0] * e2[2], e1[0] * e2[1] - e1[1] * e2[0]];
            n[2].abs() / (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt().max(1e-9)
        }

        pub(crate) fn empty() -> Self {
            Self { names: Vec::new(), grid: Default::default() }
        }

        pub(crate) fn add(&mut self, t: [[f32; 3]; 3], name: u32) {
            if Self::up(&t) < 0.7 {
                return;
            }
            let lo = Self::col(t[0][0].min(t[1][0]).min(t[2][0]), t[0][1].min(t[1][1]).min(t[2][1]));
            let hi = Self::col(t[0][0].max(t[1][0]).max(t[2][0]), t[0][1].max(t[1][1]).max(t[2][1]));
            if (hi[0] - lo[0]) as i64 * (hi[1] - lo[1]) as i64 > 4096 {
                return;
            }
            for x in lo[0]..=hi[0] {
                for y in lo[1]..=hi[1] {
                    self.grid.entry([x, y]).or_default().push((t, name));
                }
            }
        }

        /// Every drawn floor under (x, y) between z0 and z1: (z, name).
        pub(crate) fn at(&self, x: f32, y: f32, z0: f32, z1: f32) -> Vec<(f32, u32)> {
            let mut out = Vec::new();
            for (t, n) in self.grid.get(&Self::col(x, y)).into_iter().flatten() {
                let [a, b, c] = *t;
                let d = (b[1] - c[1]) * (a[0] - c[0]) + (c[0] - b[0]) * (a[1] - c[1]);
                if d.abs() < 1e-9 {
                    continue;
                }
                let l1 = ((b[1] - c[1]) * (x - c[0]) + (c[0] - b[0]) * (y - c[1])) / d;
                let l2 = ((c[1] - a[1]) * (x - c[0]) + (a[0] - c[0]) * (y - c[1])) / d;
                let l3 = 1.0 - l1 - l2;
                if l1 < -1e-4 || l2 < -1e-4 || l3 < -1e-4 {
                    continue;
                }
                let z = l1 * a[2] + l2 * b[2] + l3 * c[2];
                if z >= z0 && z <= z1 {
                    out.push((z, *n));
                }
            }
            out.sort_by(|a, b| b.0.total_cmp(&a.0));
            out
        }
    }

    /// bo2mp: the headless drop test. The player box (BO2's 30 x 30 x 70,
    /// `MASK_PLAYERSOLID`) dropped at every spawn and over a grid (`T7_GRID`
    /// units, default 96) of points on every drawn floor of the play space
    /// (the spawns' bounds and 512 round them): it must land within 48 units
    /// of the floor it was dropped on (not below the map, not through the
    /// floor), not start or end in solid; a spawn must not stand in solid.
    /// Then the match's own movement (`T7_PMOVE_DROPS` of the drops, 2 s
    /// each: fall, then run and jump) must not leave a drawn floor through
    /// it. And every player-clip brush (`CONTENTS_PLAYERCLIP` without
    /// solid; the invisible walls) in the play space must stop the box
    /// walked through it. Failure counts per map and the commonest causes
    /// by drawn material or model (`T7_TEST_MAP=<zone.ff>[,...] cargo test
    /// -p assets --lib drop_test_t7 -- --ignored --nocapture`; `T7_STRICT=1`
    /// fails the test on any failure).
    #[test]
    #[ignore]
    fn drop_test_t7() {
        let Ok(paths) = std::env::var("T7_TEST_MAP") else {
            return;
        };
        let env = |k: &str, d: usize| {
            codlegacy_env::var(k)
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(d)
        };
        let grid_step = env("T7_GRID", 96) as f32;
        let pmove_drops = env("T7_PMOVE_DROPS", 600);
        let mut seed = 0x9e37_79b9_7f4a_7c15u64;
        let mut rand = move || {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            (seed >> 11) as f32 / (1u64 << 53) as f32
        };
        let mut summary: Vec<String> = Vec::new();
        let mut total_failures = 0usize;
        for path in paths.split(',') {
            let map = super::T7Map::load(&super::MapSources::beside(std::path::Path::new(path))).expect("map");
            let map_name = std::path::Path::new(path).file_stem().and_then(|s| s.to_str()).unwrap_or(path).to_owned();
            let entities = iw3::ents::parse(&map.entities);
            let spawns: Vec<[f32; 3]> = super::spawns_of(&entities).iter().map(|s| s.origin).collect();
            let hidden: Vec<bool> = map
                .surfaces
                .iter()
                .map(|s| s.material.is_some_and(|m| super::hidden_kind(&map.materials[m].name, &map.materials[m].technique_set).is_some()))
                .collect();
            let mut report = Vec::new();
            let full = super::collision_of(&map, &hidden, &mut report);
            println!("== {map_name}: {report:?}");
            let floors = DrawnFloors::of(&map, &hidden);
            // Each brush's corners' centre (inside it: a brush is convex).
            let centres: Vec<Option<[f32; 3]>> = map.clip.as_ref().map_or_else(Vec::new, |c| {
                c.brushes
                    .iter()
                    .map(|b| {
                        (!b.verts.is_empty()).then(|| {
                            let n = b.verts.len() as f32;
                            std::array::from_fn(|k| b.verts.iter().map(|v| v[k]).sum::<f32>() / n)
                        })
                    })
                    .collect()
            });
            drop(entities);
            drop(map);
            if spawns.is_empty() {
                summary.push(format!("{map_name}: no spawns"));
                continue;
            }
            let (line, failures) = drop_test_world(
                &map_name,
                &full,
                &spawns,
                &floors,
                &centres,
                grid_step,
                pmove_drops,
                env("T7_REACH_NODES", 60000),
                &mut rand,
            );
            total_failures += failures;
            summary.push(line);
        }
        println!("== drop test summary");
        for l in &summary {
            println!("{l}");
        }
        if std::env::var("T7_STRICT").is_ok() {
            assert_eq!(total_failures, 0, "drop test failures");
        }
    }

    /// bo2mp: the drop test's checks on one map's collision (`drop_test_t7`
    /// says what they are; the MWR lane's `drop_test_h1` runs them too):
    /// the summary line and the failure count.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn drop_test_world(
        map_name: &str,
        full: &asset_world::ClipCollision,
        spawns: &[[f32; 3]],
        floors: &DrawnFloors,
        centres: &[Option<[f32; 3]>],
        grid_step: f32,
        pmove_drops: usize,
        reach_nodes: usize,
        rand: &mut dyn FnMut() -> f32,
    ) -> (String, usize) {
        const MASK: u32 = asset_world::MASK_PLAYER_SOLID;
        const PLAYERCLIP: u32 = 0x1_0000;
        let (mins, maxs) = ([-15.0f32, -15.0, 0.0], [15.0f32, 15.0, 70.0]);
        let spawns: Vec<[f32; 3]> = spawns.to_vec();
        let (mut lo, mut hi) = ([f32::MAX; 3], [f32::MIN; 3]);
        for s in &spawns {
            for k in 0..3 {
                lo[k] = lo[k].min(s[k]);
                hi[k] = hi[k].max(s[k]);
            }
        }
        let lowest_spawn = lo[2];
        let (lo, hi) = ([lo[0] - 512.0, lo[1] - 512.0, lo[2] - 768.0], [hi[0] + 512.0, hi[1] + 512.0, hi[2] + 1536.0]);
        // Where a player can get: from every spawn, steps of 48 units
        // in 8 directions over the collision (stepping up 18, or a
        // jump and mantle up 60), falling at most 300 (a longer fall
        // kills) and never below 300 under the lowest spawn; cells of
        // 64 x 64 x 64. A step whose ground lies over 64 below a drawn
        // floor at the box's centre went through that floor.
        let key = |p: [f32; 3]| [(p[0] / 64.0).floor() as i32, (p[1] / 64.0).floor() as i32, (p[2] / 64.0).floor() as i32];
        let mut reached: std::collections::HashSet<[i32; 3]> = Default::default();
        let mut walk_holes: Vec<([f32; 3], f32, u32, f32)> = Vec::new();
        {
            let max_nodes = reach_nodes;
            let mut queue: std::collections::VecDeque<[f32; 3]> = Default::default();
            let mut seen: std::collections::HashSet<[i32; 3]> = Default::default();
            let cell = |p: [f32; 3]| [(p[0] / 24.0).round() as i32, (p[1] / 24.0).round() as i32, (p[2] / 16.0).round() as i32];
            for o in &spawns {
                let d = full.sweep_box([o[0], o[1], o[2] + 8.0], [o[0], o[1], o[2] - 64.0], mins, maxs, MASK);
                if !d.startsolid && d.fraction < 1.0 && seen.insert(cell(d.endpos)) {
                    queue.push_back(d.endpos);
                }
            }
            let dirs: Vec<[f32; 2]> = (0..8).map(|k| (k as f32) * std::f32::consts::FRAC_PI_4).map(|a| [a.cos() * 48.0, a.sin() * 48.0]).collect();
            while let Some(p) = queue.pop_front() {
                reached.insert(key(p));
                if seen.len() > max_nodes {
                    continue;
                }
                for d in &dirs {
                    for lift in [18.0f32, 60.0] {
                        let up = full.sweep_box(p, [p[0], p[1], p[2] + lift], mins, maxs, MASK);
                        if up.startsolid {
                            break;
                        }
                        let s = up.endpos;
                        let f = full.sweep_box(s, [s[0] + d[0], s[1] + d[1], s[2]], mins, maxs, MASK);
                        if f.startsolid || f.fraction < 1.0 {
                            continue;
                        }
                        let e = f.endpos;
                        let g = full.sweep_box(e, [e[0], e[1], e[2] - lift - 300.0], mins, maxs, MASK);
                        let drawn = floors.at(e[0], e[1], p[2] - 64.0, p[2] + lift + 2.0).first().copied();
                        let ground = (g.fraction < 1.0 && !g.startsolid && g.normal[2] >= 0.7).then_some(g.endpos);
                        if let Some((dz, n)) = drawn
                            && ground.is_none_or(|q| q[2] < dz - 64.0)
                        {
                            let far = full.sweep_box(e, [e[0], e[1], e[2] - 8192.0], mins, maxs, MASK);
                            // (A floor the box met on an edge or a slope
                            // near the drawn one is not a hole.)
                            if far.fraction >= 1.0 || far.endpos[2] < dz - 64.0 {
                                walk_holes.push((e, dz, n, if far.fraction < 1.0 { far.endpos[2] } else { f32::NEG_INFINITY }));
                            }
                        }
                        if let Some(q) = ground
                            && q[2] > lowest_spawn - 300.0
                            && seen.insert(cell(q))
                        {
                            queue.push_back(q);
                        }
                        break;
                    }
                }
            }
            println!("reach: {} places walked to ({} cells of 64), {} steps over a hole", seen.len(), reached.len(), walk_holes.len());
        }
        let near_reached = |p: [f32; 3]| {
            let k = key(p);
            (-1..=1).any(|dz| reached.contains(&[k[0], k[1], k[2] + dz]))
        };
        // Samples: (x, y, floor z, name; u32::MAX a spawn): the spawns and
        // the drawn floors where a player can get.
        let mut samples: Vec<([f32; 3], u32)> = spawns.iter().map(|s| (*s, u32::MAX)).collect();
        let mut unreached = 0usize;
        let mut x = lo[0];
        while x <= hi[0] {
            let mut y = lo[1];
            while y <= hi[1] {
                let mut last = f32::MAX;
                for (z, n) in floors.at(x, y, lo[2], hi[2]) {
                    if last - z > 40.0 {
                        last = z;
                        if near_reached([x, y, z]) {
                            samples.push(([x, y, z], n));
                        } else {
                            unreached += 1;
                        }
                    }
                }
                y += grid_step;
            }
            x += grid_step;
        }
        let name_of = |n: u32| if n == u32::MAX { "spawn".to_owned() } else { floors.names[n as usize].clone() };
        // Failures by kind, then by name: (count, examples).
        let mut fails: std::collections::BTreeMap<&'static str, std::collections::BTreeMap<String, (usize, Vec<String>)>> = std::collections::BTreeMap::new();
        let mut fail = |kind: &'static str, name: String, at: String| {
            let e = fails.entry(kind).or_default().entry(name).or_default();
            e.0 += 1;
            if e.1.len() < 3 {
                e.1.push(at);
            }
        };
        {
            let mut by_cell: std::collections::HashSet<[i32; 3]> = Default::default();
            for (e, dz, n, col) in &walk_holes {
                if by_cell.insert([(e[0] / 128.0) as i32, (e[1] / 128.0) as i32, (dz / 128.0) as i32]) {
                    fail("walk: stepped through a floor", name_of(*n), format!("({:.0} {:.0} {:.0}) floor {dz:.0}, collision {}", e[0], e[1], e[2], if col.is_finite() { format!("{col:.0}") } else { "none".into() }));
                }
            }
        }
        let (mut dropped, mut start_solid) = (0usize, 0usize);
        for &(p, n) in &samples {
            let [x, y, z] = p;
            // (A spawn stands on or a little over its floor: the drawn
            // floor under it is the one it must land on.)
            let z = if n == u32::MAX {
                let at = [x, y, z + 1.0];
                if full.sweep_box(at, at, mins, maxs, MASK).startsolid {
                    fail("spawn in solid", "spawn".into(), format!("({x:.0} {y:.0} {z:.0})"));
                }
                floors.at(x, y, z - 128.0, z + 2.0).first().map_or(z, |f| f.0)
            } else {
                z
            };
            let start = [x, y, z + 40.0];
            if full.sweep_box(start, start, mins, maxs, MASK).startsolid {
                // (Under a prop, inside a wall or a clip: not a place a
                // player can be dropped at.)
                start_solid += 1;
                continue;
            }
            dropped += 1;
            let d = full.sweep_box(start, [x, y, z - 8192.0], mins, maxs, MASK);
            let below = floors.at(x, y, z - 8192.0, z - 48.0).first().map_or("nothing drawn".to_owned(), |(bz, bn)| format!("next drawn {bz:.0} {}", name_of(*bn)));
            if d.fraction >= 1.0 {
                fail("fell out of the map", name_of(n), format!("({x:.0} {y:.0} {z:.0}) {below}"));
            } else if d.endpos[2] < z - 48.0 {
                fail("fell through the floor", name_of(n), format!("({x:.0} {y:.0} {z:.0}) landed {:.0}, {below}", d.endpos[2]));
            } else if full.sweep_box(d.endpos, d.endpos, mins, maxs, MASK).startsolid || d.normal[2] < 0.0 {
                fail("landed in solid", name_of(n), format!("({x:.0} {y:.0} {z:.0}) at {:.1}", d.endpos[2]));
            }
        }
        // The match's movement: a fall from 40 above, 0.8 s still (it
        // must have landed, within 48 of the floor), then 1.5 s running
        // at random with jumps; moving down past a drawn floor with the
        // box's centre over it is a fall through it.
        let clip = PmoveClip(&full);
        let open: Vec<([f32; 3], u32)> = samples
            .iter()
            .copied()
            .filter(|(p, _)| {
                let s = [p[0], p[1], p[2] + 40.0];
                !full.sweep_box(s, s, mins, maxs, MASK).startsolid
            })
            .collect();
        let every = (open.len() / pmove_drops.max(1)).max(1);
        let mut pmoved = 0usize;
        for &(p, n) in open.iter().step_by(every) {
            pmoved += 1;
            let mut ps = playerstate_iw4::PlayerState::ZERO;
            ps.origin = [p[0], p[1], p[2] + 40.0];
            ps.speed = 190;
            ps.health = 100;
            ps.max_health = 100;
            ps.move_speed_scale_multiplier = 1.0;
            ps.view_height_target = 60;
            ps.view_height_current = 60.0;
            ps.gravity = 800;
            ps.ground_entity_num = 0x7ff;
            ps.other_flags |= playerstate_iw4::other_flags::PLAYER;
            ps.corpse_index = -1;
            let yaw = rand() * 360.0;
            let mut old_buttons = 0u32;
            let mut left: Option<([f32; 3], f32, u32)> = None;
            let mut landed = false;
            for t in 0..46 {
                let still = t < 16;
                let b = if !still && rand() < 0.08 { playerstate_iw4::buttons::JUMP } else { 0 } | if still { 0 } else { playerstate_iw4::buttons::SPRINT };
                let mut cmd = playerstate_iw4::UserCmd {
                    server_time: ps.command_time + 50,
                    buttons: b,
                    angles: [0, ((yaw + t as f32 * 4.0).rem_euclid(360.0) * movement_iw4::ANGLE2SHORT) as i32, 0],
                    forwardmove: if still { 0 } else { 127 },
                    ..Default::default()
                };
                let before = ps.origin;
                let was_ground = ps.ground_entity_num != 0x7ff;
                movement_iw4::pmove(&mut ps, &mut cmd, pmove_context(old_buttons), &clip, &movement_iw4::FlatMantleAnimLength { up_msec: 500, over_msec: 500 }, &movement_iw4::ZeroMantleRootDelta);
                old_buttons = b;
                let on_ground = ps.ground_entity_num != 0x7ff;
                if still {
                    landed |= on_ground;
                    if t == 15 && (!landed || ps.origin[2] < p[2] - 48.0) {
                        fail("pmove: dropped and did not land", name_of(n), format!("({:.0} {:.0} {:.0}) now {:.0}", p[0], p[1], p[2], ps.origin[2]));
                        break;
                    }
                    continue;
                }
                let _ = (&mut left, was_ground);
                // Moving down past a drawn floor's height with the box's
                // centre over it: through that floor.
                if !on_ground && ps.origin[2] < before[2] - 1.0 {
                    let o = ps.origin;
                    if let Some(&(dz, fnm)) = floors.at(o[0], o[1], o[2] + 8.0, before[2] + 1.0).first() {
                        fail("pmove: ran through a floor", name_of(fnm), format!("({:.0} {:.0} {:.0}) -> {:.0}, floor {dz:.0}", before[0], before[1], before[2], o[2]));
                        break;
                    }
                }
            }
        }
        // Player clips (the invisible walls) stop the box.
        // (The world's: a sub-model's brushes stand only through its
        // entity.)
        let mut world_brush = vec![false; full.brushes.len()];
        for l in &full.leaves {
            for &b in full.leafbrushes.iter().skip(l.first_brush as usize).take(usize::from(l.num_brushes)) {
                if let Some(w) = world_brush.get_mut(usize::from(b)) {
                    *w = true;
                }
            }
        }
        let (mut clips, mut clips_tried) = (0usize, 0usize);
        for (bi, b) in full.brushes.iter().enumerate().filter(|(i, b)| world_brush[*i] && b.contents & PLAYERCLIP != 0 && b.contents & 1 == 0 && b.planes.len() >= 6) {
            let g = |i: usize| b.planes[i][3];
            let (blo, bhi) = ([-g(1), -g(3), -g(5)], [g(0), g(2), g(4)]);
            if bhi[0] < lo[0] || blo[0] > hi[0] || bhi[1] < lo[1] || blo[1] > hi[1] || bhi[2] < lo[2] || blo[2] > hi[2] {
                continue;
            }
            clips += 1;
            // Through its thinner horizontal extent at its corners'
            // centre (inside it, however slanted its sides), the box's
            // middle there.
            let c: [f32; 3] = centres.get(bi).copied().flatten().unwrap_or_else(|| std::array::from_fn(|k| (blo[k] + bhi[k]) * 0.5));
            let a = if bhi[0] - blo[0] <= bhi[1] - blo[1] { 0 } else { 1 };
            let z = c[2] - 35.0;
            let (mut s, mut e) = ([c[0], c[1], z], [c[0], c[1], z]);
            s[a] = blo[a] - 17.0;
            e[a] = bhi[a] + 17.0;
            // (Another solid at either end: not a wall to walk into.)
            if full.sweep_box(s, s, mins, maxs, MASK).startsolid || full.sweep_box(e, e, mins, maxs, MASK).startsolid {
                continue;
            }
            clips_tried += 1;
            let t = full.sweep_box(s, e, mins, maxs, MASK);
            let back = full.sweep_box(e, s, mins, maxs, MASK);
            if t.fraction >= 1.0 || back.fraction >= 1.0 {
                fail("player clip let the player through", format!("contents {:#x}", b.contents), format!("({:.0} {:.0} {:.0})..({:.0} {:.0} {:.0})", blo[0], blo[1], blo[2], bhi[0], bhi[1], bhi[2]));
            }
        }
        let count = |k: &str| fails.get(k).map_or(0, |m| m.values().map(|v| v.0).sum::<usize>());
        let kinds = [
            "fell out of the map",
            "fell through the floor",
            "landed in solid",
            "spawn in solid",
            "walk: stepped through a floor",
            "pmove: dropped and did not land",
            "pmove: ran through a floor",
            "player clip let the player through",
        ];
        let line = format!(
            "{map_name}: {} drops ({} spawns, {start_solid} starts in solid and {unreached} floors no player reaches skipped), {pmoved} pmove runs, {clips_tried} of {clips} player clips walked into; failures: {}",
            dropped,
            spawns.len(),
            kinds.iter().map(|k| format!("{k} {}", count(k))).collect::<Vec<_>>().join(", ")
        );
        let failures = kinds.iter().map(|k| count(k)).sum::<usize>();
        println!("{line}");
        for (kind, by) in &fails {
            let mut rows: Vec<_> = by.iter().collect();
            rows.sort_by(|a, b| b.1.0.cmp(&a.1.0));
            println!("  {kind}:");
            for (name, (n, at)) in rows.iter().take(12) {
                println!("    {n:5} {name}: {}", at.join("; "));
            }
        }
        (line, failures)
    }

    /// bo2mp: what lies under points (`T7_PROBE="x y z;x y z"`): per
    /// contents bit, where the player box dropped from 40 above first meets
    /// it (brush or mesh), and the brushes round the column with their
    /// contents (`T7_TEST_MAP=<zone.ff> cargo test -p assets --lib probe_t7
    /// -- --ignored --nocapture`).
    #[test]
    #[ignore]
    fn probe_t7() {
        let (Ok(path), Ok(points)) = (std::env::var("T7_TEST_MAP"), std::env::var("T7_PROBE")) else { return };
        let map = super::T7Map::load(&super::MapSources::beside(std::path::Path::new(&path))).expect("map");
        let hidden: Vec<bool> = map
            .surfaces
            .iter()
            .map(|s| s.material.is_some_and(|m| super::hidden_kind(&map.materials[m].name, &map.materials[m].technique_set).is_some()))
            .collect();
        let mut report = Vec::new();
        let full = super::collision_of(&map, &hidden, &mut report);
        let floors = DrawnFloors::of(&map, &hidden);
        let (mins, maxs) = ([-15.0f32, -15.0, 0.0], [15.0f32, 15.0, 70.0]);
        {
            let clip = map.clip.as_ref().unwrap();
            let reach = asset_world::world_t7::bsp_leaves_reached(&clip.nodes.iter().map(|n| (n.plane, n.children)).collect::<Vec<_>>(), clip.leafs.len());
            println!("zone: {} nodes, {} leaves, reached from node 0 {:?}", clip.nodes.len(), clip.leafs.len(), reach.as_ref().map(Vec::len));
            for (k, l) in clip.leafs.iter().enumerate() {
                let owned: Vec<usize> = clip.models.iter().enumerate().filter(|(_, m)| !m.leaf.brushes.is_empty() && m.leaf.brushes == l.brushes).map(|(i, _)| i).collect();
                println!("  leaf {k}: {} brushes, aabbs {}+{}, bounds {:?}..{:?}, reached {}, same as sub-model {owned:?}", l.brushes.len(), l.first_coll_aabb, l.coll_aabb_count, l.mins, l.maxs, reach.as_ref().is_some_and(|r| r.contains(&k)));
            }
            for (k, m) in clip.models.iter().enumerate().take(64) {
                println!("  sub-model *{k}: {} brushes, aabbs {}+{}, {:?}..{:?}", m.leaf.brushes.len(), m.leaf.first_coll_aabb, m.leaf.coll_aabb_count, m.mins, m.maxs);
            }
        }
        for p in points.split(';') {
            let v: Vec<f32> = p.split_whitespace().filter_map(|x| x.parse().ok()).collect();
            let [x, y, z] = [v[0], v[1], v[2]];
            println!("== ({x} {y} {z})");
            for (fz, n) in floors.at(x, y, z - 600.0, z + 200.0) {
                println!("  drawn floor {fz:.1} {}", floors.names[n as usize]);
            }
            let (s, e) = ([x, y, z + 40.0], [x, y, z - 600.0]);
            for b in 0..32 {
                let ext = clipmap_iw4::TraceExtents::new(s, e, mins, maxs, 1 << b);
                let map_ref = clipmap_iw4::ClipMapRef { nodes: &full.nodes, leaves: &full.leaves, leafbrushes: &full.leafbrushes, brushes: &full.brushes };
                let mesh = clipmap_iw4::ClipMeshTables::as_ref(&full.mesh);
                let (t, w) = clipmap_iw4::trace_brush_and_mesh_with_winner(&map_ref, &mesh, &ext, &|_| true);
                if t.fraction < 1.0 || t.startsolid != 0 {
                    println!("  bit {:#010x}: hit at {:.1} ({w:?}) startsolid {} normal {:?}", 1u32 << b, t.endpos[2], t.startsolid, t.normal);
                }
            }
            // Where the player box lands, and what holds it there in solid.
            let land = full.sweep_box(s, e, mins, maxs, asset_world::MASK_PLAYER_SOLID);
            println!("  player box lands at {:.2} (fraction {:.4}, normal {:?}, startsolid {})", land.endpos[2], land.fraction, land.normal, land.startsolid);
            for b in 0..32 {
                let ext = clipmap_iw4::TraceExtents::new(land.endpos, land.endpos, mins, maxs, 1 << b);
                let map_ref = clipmap_iw4::ClipMapRef { nodes: &full.nodes, leaves: &full.leaves, leafbrushes: &full.leafbrushes, brushes: &full.brushes };
                let mesh = clipmap_iw4::ClipMeshTables::as_ref(&full.mesh);
                let (t, w) = clipmap_iw4::trace_brush_and_mesh_with_winner(&map_ref, &mesh, &ext, &|_| true);
                if t.startsolid != 0 {
                    println!("    in solid there for bit {:#010x} ({w:?})", 1u32 << b);
                }
            }
            // The brushes round the column.
            for (i, b) in full.brushes.iter().enumerate() {
                if b.planes.len() < 6 {
                    continue;
                }
                let g = |k: usize| b.planes[k][3];
                let (lo, hi) = ([-g(1), -g(3), -g(5)], [g(0), g(2), g(4)]);
                if lo[0] <= x + 16.0 && hi[0] >= x - 16.0 && lo[1] <= y + 16.0 && hi[1] >= y - 16.0 && lo[2] <= z + 110.0 && hi[2] >= z - 600.0 {
                    let want = i as u16;
                    let clip = map.clip.as_ref().unwrap();
                    let owners: Vec<usize> = clip.models.iter().enumerate().filter(|(_, m)| m.leaf.brushes.contains(&want)).map(|(k, _)| k).collect();
                    let leaves: Vec<usize> = clip.leafs.iter().enumerate().filter(|(_, l)| l.brushes.contains(&want)).map(|(k, _)| k).collect();
                    let world = full.leaves.iter().any(|l| full.leafbrushes.iter().skip(l.first_brush as usize).take(usize::from(l.num_brushes)).any(|&x| x == want));
                    println!("  brush {i} contents {:#x} {:?}..{:?} sides {} world {world} sub-models {owners:?} zone leaves {leaves:?}", b.contents, lo, hi, b.planes.len());
                }
            }
        }
    }

    /// The clip map as the match's movement traces it (world brushes and
    /// mesh; no entities).
    struct PmoveClip<'a>(&'a asset_world::ClipCollision);

    impl movement_iw4::CollisionBackend for PmoveClip<'_> {
        fn trace(&self, input: movement_iw4::GroundTraceInput) -> trace_iw4::Trace {
            let c = self.0;
            let map = clipmap_iw4::ClipMapRef { nodes: &c.nodes, leaves: &c.leaves, leafbrushes: &c.leafbrushes, brushes: &c.brushes };
            let ext = clipmap_iw4::TraceExtents::new(input.start, input.end, input.mins, input.maxs, input.tracemask);
            let mesh = clipmap_iw4::ClipMeshTables::as_ref(&c.mesh);
            clipmap_iw4::trace_brush_and_mesh(&map, &mesh, &ext, &|_| true)
        }
    }

    /// The match's movement settings (`sim::step::pmove_context` for a
    /// rifle at full speed, no aiming).
    fn pmove_context(old_buttons: u32) -> movement_iw4::PmoveSingleContext {
        use movement_iw4::*;
        let feel = Bo2Feel::DEFAULTS;
        let air = AirMoveContext { player_spectate_speed_scale: 1.0, shellshock_gravity_scale: 1.0, shellshock_gravity_bias: 0.0 };
        PmoveSingleContext {
            walk: WalkMoveContext {
                cmd_scale: CmdScaleWalkContext {
                    player_back_speed_scale: 0.7,
                    player_strafe_speed_scale: 0.8,
                    player_sprint_speed_scale: 1.5,
                    player_last_stand_crawl_speed_scale: 0.15,
                    weapon_move_speed_scale: 1.0,
                    weapon_ads_move_speed_scale: 1.0,
                    shellshock_movement_scale: 1.0,
                    prone_lerp_ms: PRONE_LERP_MS,
                },
                weapon_move_scale: 1.0,
                old_buttons,
                jump: JumpLaunchContext { jump_height: 39.0, dive: false, crouch_jump_scale: 1.0, jump_ladder_push_vel: feel.ladder_push_vel },
                air,
                feel,
            },
            air,
            bounds: MoveBounds { mins: [-15.0, -15.0, 0.0], maxs: [15.0, 15.0, 70.0], tracemask: 0x0281_0011 },
            view_angles: ViewAngleClamp { pitch_up: 85.0, pitch_down: 85.0, unclamped_pitch_bit: false },
            sprint: SprintContext {
                weapon_max_sprint_time: 4000,
                sprint_forever: true,
                min_sprint_time_seconds: 1.0,
                sprint_delay_seconds: 0.0,
                sprint_forward_minimum: 105,
                stand_up_clear: true,
                sprint_recharge_pause_seconds: 0.0,
            },
            ads_intent: AdsIntentContext { ads_allowed: false, weapon_def_scope: false, sprint_hold_ads: false },
            ads_frac: AdsFracContext {
                aim_down_sight: false,
                ads_in_rate: 1.0,
                ads_out_rate: 1.0,
                rechamber_while_ads: false,
                ads_fire_only: false,
                reload_lowers_ads: false,
                position_reload_trans_ms: 0,
                segmented_reload: false,
            },
            melee_charge: MeleeChargeWeaponDelays { melee_delay_ms: 0, melee_charge_delay_ms: 0 },
            player_melee_range: MELEE_CHARGE_PLAYER_MELEE_RANGE_DEFAULT,
            old_buttons,
            old_move: [0, 0],
            weapon_blocks_prone: false,
        }
    }
}

/// bo2mp: a Black Ops III Zombies map (`zm_*`) in the terms of the
/// Infinite Warfare Zombies pipeline (`super::iw7_zombies`), which writes
/// it out as Black Ops II Zombies on Buried's kit. Black Ops III keeps
/// Black Ops II's own vocabulary, so this is mostly a reading:
/// - zones: `info_volume` `player_volume`s (their brush model's box, a
///   trigger model of its own is added for it), their spawners the structs
///   their `target` names (`riser_location` climb out of the ground,
///   `dog_location`s are the dog spots);
/// - windows: `exterior_goal` structs, the boards where the zbarrier they
///   target stands;
/// - doors and debris: `zombie_door`/`zombie_debris` triggers (cost, the
///   zone their `script_flag` opens, the brushes their `target` names);
/// - perk machines by their `specialty_*` (as the pipeline's machine
///   names), Mystery Box spots (`treasure_chest_use`), Pack-a-Punch, the
///   power switch, wall buys (each weapon as one of its kind), traversals.
/// Not carried: teleporters, traps, the Gobblegum machines, the Easter egg.
#[cfg(feature = "iw7")]
pub(crate) mod zm {
    use std::collections::HashMap;

    use fastfile_iw7::zombies::{Area, Blocker, BlockerKind, Generator, Machine, Spawner, Spot, Traversal, WallBuy, Window, ZmLayout};
    use fastfile_t7::map::T7Map;

    type Ent = iw3::ents::Entity;

    fn spot(e: &Ent) -> Option<Spot> {
        Some(Spot { origin: e.origin()?, angles: e.angles() })
    }

    fn model_index(e: &Ent, prefix: char) -> Option<u32> {
        e.get("model")?.strip_prefix(prefix)?.parse().ok()
    }

    /// A Black Ops III perk as the pipeline's machine name.
    fn machine_name(specialty: &str) -> Option<&'static str> {
        Some(match specialty {
            "specialty_armorvest" => "perk_machine_tough",
            "specialty_quickrevive" => "perk_machine_revive",
            "specialty_fastreload" => "perk_machine_flash",
            "specialty_doubletap2" | "specialty_rof" => "perk_machine_rat_a_tat",
            "specialty_staminup" | "specialty_longersprint" => "perk_machine_run",
            "specialty_additionalprimaryweapon" => "perk_machine_more",
            _ => return None,
        })
    }

    /// A Black Ops III wall weapon as the pipeline's weapon of its kind.
    fn wall_weapon(t7: &str) -> Option<&'static str> {
        let kind = t7.split('_').next()?;
        Some(match (kind, t7) {
            (_, "pistol_standard" | "pistol_burst" | "pistol_fullauto") | ("pistol", _) => "iw7_g18_zm",
            ("shotgun", _) => "iw7_spas_zm",
            (_, "ar_standard" | "ar_marksman" | "ar_longburst") => "iw7_m4_zm",
            ("ar", _) => "iw7_ar57_zm",
            (_, "smg_standard" | "smg_versatile") => "iw7_crb_zm",
            ("smg", _) => "iw7_erad_zm",
            ("lmg", _) => "iw7_rvn_zm",
            ("sniper", _) => "iw7_kbs_zm",
            _ => return None,
        })
    }

    /// The layout, the trigger models added for its zones (appended to the
    /// clip's), and report lines.
    pub(crate) fn read(map: &T7Map, first_trigger: usize) -> (ZmLayout, Vec<Vec<asset_world::MapTriggerHull>>, Vec<String>) {
        let ents = iw3::ents::parse(&map.entities);
        let by_name: HashMap<&str, Vec<&Ent>> = ents.iter().filter_map(|e| Some((e.get("targetname")?, e))).fold(HashMap::new(), |mut m, (k, e)| {
            m.entry(k).or_default().push(e);
            m
        });
        let named = |n: &str| by_name.get(n).cloned().unwrap_or_default();
        let mut l = ZmLayout::default();
        let mut report = Vec::new();
        let mut triggers = Vec::new();
        let cmodel_box = |e: &Ent| -> Option<([f32; 3], [f32; 3])> {
            let m = map.clip.as_ref()?.models.get(model_index(e, '*')? as usize)?;
            let o = e.origin().unwrap_or([0.0; 3]);
            // (Sub-models' bounds are about the entity's origin.)
            let inside = (0..3).all(|k| o[k] >= m.mins[k] - 1.0 && o[k] <= m.maxs[k] + 1.0);
            Some(if inside { (m.mins, m.maxs) } else { (std::array::from_fn(|k| o[k] + m.mins[k]), std::array::from_fn(|k| o[k] + m.maxs[k])) })
        };
        l.starts = named("initial_spawn_points").iter().filter_map(|e| spot(e)).collect();
        l.respawns = named("player_respawn_point").iter().filter_map(|e| spot(e)).collect();
        // Zones.
        let mut areas: Vec<Area> = Vec::new();
        for v in ents.iter().filter(|e| e.classname() == "info_volume" && e.get("script_noteworthy") == Some("player_volume")) {
            let Some(name) = v.get("targetname") else { continue };
            let Some(b) = cmodel_box(v) else { continue };
            let area = match areas.iter().position(|a| a.name == name) {
                Some(i) => &mut areas[i],
                None => {
                    areas.push(Area { name: name.to_owned(), origin: v.origin().unwrap_or_default(), ..Default::default() });
                    let a = areas.last_mut().unwrap();
                    for s in v.get("target").map(named).unwrap_or_default() {
                        let Some(sp) = spot(s) else { continue };
                        match s.get("script_noteworthy") {
                            Some("dog_location") => l.dog_spots.push(sp),
                            Some(n) => a.spawners.push(Spawner { spot: sp, ground: n == "riser_location", ..Default::default() }),
                            None => a.spawners.push(Spawner { spot: sp, ..Default::default() }),
                        }
                    }
                    a
                }
            };
            area.boxes.push(b);
        }
        for a in &mut areas {
            a.model = Some((first_trigger + triggers.len()) as u32);
            triggers.push(zone_hulls(a.origin, &a.boxes));
        }
        l.areas = areas;
        // Windows: the outside goal, the boards where its zbarrier is.
        for g in named("exterior_goal") {
            let Some(outside) = spot(g) else { continue };
            let boards = g.get("target").map(named).unwrap_or_default().iter().find(|e| e.classname().starts_with("zbarrier")).and_then(|e| e.origin());
            l.windows.push(Window { outside, attack: Vec::new(), inside: None, boards });
        }
        // Doors and debris. The zones a door joins: Black Ops III's map
        // scripts pair them with the door's flag (`add_adjacent_zone(a, b,
        // "connect_a_to_b")`), which are compiled; the flag names them
        // (`enter_<zone>`, `connect_<a>_to_<b>`, `activate_<zone>`), and
        // where a name is not a zone's, the zones nearest the door and its
        // brushes are taken.
        let box_dist = |p: [f32; 3], (lo, hi): &([f32; 3], [f32; 3])| -> f32 {
            (0..3).map(|k| (lo[k] - p[k]).max(p[k] - hi[k]).max(0.0).powi(2)).sum::<f32>().sqrt()
        };
        let area_dist = |a: &Area, points: &[[f32; 3]]| -> f32 {
            points.iter().flat_map(|p| a.boxes.iter().map(move |b| box_dist(*p, b))).fold(f32::MAX, f32::min)
        };
        // (A flag word to the zone it names, the nearest of those whose
        // name holds it.)
        let zone_named = |word: &str, points: &[[f32; 3]]| -> Option<String> {
            let word = word.trim_matches('_');
            if word.is_empty() {
                return None;
            }
            let exact = l.areas.iter().find(|a| a.name == format!("zone_{word}") || a.name == word || a.name.trim_end_matches("_zone") == word);
            exact
                .or_else(|| {
                    l.areas
                        .iter()
                        .filter(|a| a.name.contains(word))
                        .min_by(|a, b| area_dist(a, points).total_cmp(&area_dist(b, points)))
                })
                .map(|a| a.name.clone())
        };
        let flag_zones = |flag: &str, points: &[[f32; 3]]| -> Vec<String> {
            let words: Vec<&str> = if let Some(rest) = flag.strip_prefix("connect_") {
                rest.splitn(2, "_to_").collect()
            } else if let Some(rest) = flag.strip_prefix("enter_").or_else(|| flag.strip_prefix("activate_")) {
                vec![rest]
            } else {
                Vec::new()
            };
            words.iter().filter_map(|w| zone_named(w, points)).collect()
        };
        let nearest_zones = |points: &[[f32; 3]]| -> Vec<String> {
            let mut d: Vec<(f32, &str)> = l.areas.iter().map(|a| (area_dist(a, points), a.name.as_str())).filter(|(d, _)| *d <= 192.0).collect();
            d.sort_by(|a, b| a.0.total_cmp(&b.0));
            d.into_iter().take(2).map(|(_, n)| n.to_owned()).collect()
        };
        let mut groups: HashMap<String, (Blocker, Vec<String>, Vec<[f32; 3]>)> = HashMap::new();
        for t in named("zombie_door").into_iter().chain(named("zombie_debris")) {
            let group = t.get("target").unwrap_or_default().to_owned();
            let cost = t.get("zombie_cost").and_then(|c| c.parse().ok()).unwrap_or(1000);
            let (b, flags, points) = groups.entry(group.clone()).or_insert_with(|| {
                let parts = named(&group);
                let points: Vec<[f32; 3]> = parts
                    .iter()
                    .filter_map(|e| cmodel_box(e).map(|(lo, hi)| std::array::from_fn(|k| (lo[k] + hi[k]) * 0.5)).or_else(|| e.origin()))
                    .collect();
                (
                    Blocker {
                        kind: BlockerKind::Debris,
                        cost,
                        sides: Vec::new(),
                        triggers: Vec::new(),
                        brushes: parts.iter().filter_map(|e| Some((model_index(e, '*')?, e.origin().unwrap_or_default()))).collect(),
                        group: group.clone(),
                        props: Vec::new(),
                    },
                    Vec::new(),
                    points,
                )
            });
            if let Some(f) = t.get("script_flag") {
                flags.push(f.to_owned());
            }
            if let Some(o) = t.origin() {
                points.push(o);
            }
            if let (Some(o), Some(m)) = (t.origin(), model_index(t, '?')) {
                b.triggers.push((o, m));
            }
        }
        let groups: HashMap<String, Blocker> = groups
            .into_iter()
            .map(|(k, (mut b, flags, points))| {
                let mut zones: Vec<String> = flags.iter().flat_map(|f| flag_zones(f, &points)).collect();
                zones.sort();
                zones.dedup();
                if zones.len() < 2 {
                    for z in nearest_zones(&points) {
                        if zones.len() < 2 && !zones.contains(&z) {
                            zones.push(z);
                        }
                    }
                }
                // (Each side at the trigger nearest that zone.)
                for z in zones {
                    let Some(area) = l.areas.iter().find(|a| a.name == z) else { continue };
                    let at = b.triggers.iter().map(|t| t.0).min_by(|p, q| area_dist(area, &[*p]).total_cmp(&area_dist(area, &[*q]))).or_else(|| points.first().copied());
                    if let Some(origin) = at {
                        b.sides.push((Spot { origin, angles: [0.0; 3] }, z));
                    }
                }
                (k, b)
            })
            .collect();
        let mut blockers: Vec<Blocker> = groups.into_values().collect();
        blockers.sort_by(|a, b| a.group.cmp(&b.group));
        let no_side = blockers.iter().filter(|b| b.sides.len() < 2).count();
        l.blockers = blockers;
        // Machines, the box, Pack-a-Punch, power, wall buys.
        let mut dropped_perks = Vec::new();
        for p in named("zm_perk_machine") {
            match (p.get("script_noteworthy").and_then(machine_name), spot(p)) {
                (Some(name), Some(sp)) => l.perks.push(Machine { name: name.to_owned(), spot: sp }),
                _ => dropped_perks.push(p.get("script_noteworthy").unwrap_or("?").to_owned()),
            }
        }
        l.wheels = named("treasure_chest_use").iter().filter_map(|e| spot(e)).collect();
        l.pap = named("zm_pack_a_punch").first().and_then(|e| spot(e));
        l.generators = named("use_power_switch")
            .iter()
            .filter_map(|e| Some(Generator { spot: spot(e)?, areas: l.areas.iter().map(|a| a.name.clone()).collect(), lever: None }))
            .collect();
        let mut dropped_guns = Vec::new();
        for w in named("weapon_upgrade") {
            let gun = w.get("zombie_weapon_upgrade").unwrap_or_default();
            match (wall_weapon(gun), spot(w)) {
                (Some(g), Some(sp)) => l.wall_buys.push(WallBuy { weapon: g.to_owned(), spot: sp, model_spot: None }),
                _ => dropped_guns.push(gun.to_owned()),
            }
        }
        // Traversals: begin node to the node it targets.
        for b in ents.iter().filter(|e| e.classname() == "node_negotiation_begin") {
            let end = b.get("target").map(named).unwrap_or_default().first().and_then(|e| e.origin());
            if let (Some(begin), Some(end)) = (b.origin(), end) {
                l.traversals.push(Traversal { begin, end, animscript: b.get("animscript").unwrap_or_default().to_owned() });
            }
        }
        report.push(format!(
            "t7 zombies layout: {} starts, {} respawns, {} zones ({} spawners), {} windows, {} doors/debris ({} joining fewer than two zones), {} perks (left out: {:?}), {} box spots, pack-a-punch {}, {} power switches, {} wall buys (left out: {:?}), {} dog spots, {} traversals; not carried: teleporters, traps, Gobblegum machines",
            l.starts.len(),
            l.respawns.len(),
            l.areas.len(),
            l.areas.iter().map(|a| a.spawners.len()).sum::<usize>(),
            l.windows.len(),
            l.blockers.len(),
            no_side,
            l.perks.len(),
            dropped_perks,
            l.wheels.len(),
            if l.pap.is_some() { "found" } else { "missing" },
            l.generators.len(),
            l.wall_buys.len(),
            dropped_guns,
            l.dog_spots.len(),
            l.traversals.len()
        ));
        (l, triggers, report)
    }

    /// A zone's trigger model from its world-space boxes. The zone is
    /// written as an `info_volume` at `origin` naming this model, and a
    /// trigger model's hulls are about its entity's origin (the sim tests
    /// `player - origin` against them), so the boxes go in relative to it.
    /// (World-space hulls put every zone `origin` away from where it is:
    /// a player spawned in no zone, and Black Ops II's
    /// `player_out_of_playable_area_monitor` killed him at once.)
    pub(crate) fn zone_hulls(origin: [f32; 3], boxes: &[([f32; 3], [f32; 3])]) -> Vec<asset_world::MapTriggerHull> {
        boxes
            .iter()
            .map(|(lo, hi)| asset_world::MapTriggerHull {
                mid: std::array::from_fn(|k| (lo[k] + hi[k]) * 0.5 - origin[k]),
                half: std::array::from_fn(|k| (hi[k] - lo[k]) * 0.5),
                slabs: Vec::new(),
            })
            .collect()
    }

    #[cfg(test)]
    mod tests {
        #[test]
        fn zone_hulls_are_about_the_volume_origin() {
            // A zone at (1000, 2000, 0) covering 900..1100, 1900..2100, -10..200.
            let origin = [1000.0, 2000.0, 0.0];
            let hulls = super::zone_hulls(origin, &[([900.0, 1900.0, -10.0], [1100.0, 2100.0, 200.0])]);
            assert_eq!(hulls[0].mid, [0.0, 0.0, 95.0]);
            assert_eq!(hulls[0].half, [100.0, 100.0, 105.0]);
            // The sim's touch: (player box mid - origin) against mid +- half.
            let player = [1010.0, 2010.0, 0.0];
            let rel: [f32; 3] = std::array::from_fn(|k| player[k] + [0.0, 0.0, 35.0][k] - origin[k]);
            assert!((0..3).all(|k| (rel[k] - hulls[0].mid[k]).abs() <= hulls[0].half[k] + [15.0, 15.0, 35.0][k]));
        }
    }
}

/// Black Ops III's surface type: six bits at 20 (`(flags >> 20) & 0x3f`,
/// as the exe's impact code reads it), the exe's name table `bark` (1) ..
/// `opaqueglass` (40). Types 0..=31 are Black Ops II's own, in its order;
/// the nine Black Ops III added become the Black Ops II type they are a kind
/// of (the game's weapons carry Black Ops II's 32-type impact and footstep
/// tables), and bit 25 is cleared so Black Ops II's five-bit reading sees
/// the type and nothing else.
pub(crate) fn t6_surface_flags(flags: u32) -> u32 {
    const METAL: u32 = 13;
    const GLASS: u32 = 9;
    const WATER: u32 = 20;
    const FLESH: u32 = 7;
    let ty = (flags >> 20) & 0x3f;
    let t6 = match ty {
        0..=31 => ty,
        // metalthin, metalhollow, metalcatwalk, metalcar
        32..=35 => METAL,
        // glasscar, glassbulletproof
        36 | 37 => GLASS,
        // watershallow
        38 => WATER,
        // bodyarmor
        39 => FLESH,
        // opaqueglass
        40 => GLASS,
        _ => 0,
    };
    (flags & !(0x3f << 20)) | (t6 << 20)
}

#[cfg(test)]
mod surface_tests {
    use super::t6_surface_flags;

    #[test]
    fn bo3_surface_types_read_six_bits() {
        // concrete stays concrete, flags beside it kept
        assert_eq!(t6_surface_flags((5 << 20) | 0x10), (5 << 20) | 0x10);
        // watershallow (38) is water, not dirt (38 & 0x1f = 6)
        assert_eq!(t6_surface_flags(38 << 20), 20 << 20);
        assert_eq!(t6_surface_flags(33 << 20), 13 << 20);
        assert_eq!(t6_surface_flags((40 << 20) | 1), (9 << 20) | 1);
    }
}

/// bo2mp: a Black Ops III minimap picture made opaque for Black Ops II's
/// HUD. BO3's pictures can be see-through round the drawn map (Workshop
/// maps' `_t7`, e.g. Nuketown '68's: alpha 0 past 9% of each side), and
/// BO3's minimap draws them over its own dark backing; BO2's minimap box
/// has only `compass_map_color_underlay` (fading to 9% at the bottom), so
/// the game showed through as a flat sky-coloured block. Each see-through
/// texel is laid over the picture's own background colour (the median of
/// its opaque texels: Nuketown '68's 21,21,21 grid ground, Nuk3town's
/// black), which is what BO3's HUD shows there.
fn t7_compass_opaque(px: &mut [u8]) {
    let mut chans: [Vec<u8>; 3] = Default::default();
    for t in px.chunks_exact(4).filter(|t| t[3] == 255) {
        for (c, v) in chans.iter_mut().zip(t) {
            c.push(*v);
        }
    }
    let ground = chans.map(|mut c| {
        if c.is_empty() {
            return 0u8;
        }
        let mid = c.len() / 2;
        *c.select_nth_unstable(mid).1
    });
    for t in px.chunks_exact_mut(4) {
        let a = u16::from(t[3]);
        for k in 0..3 {
            t[k] = ((u16::from(t[k]) * a + u16::from(ground[k]) * (255 - a) + 127) / 255) as u8;
        }
        t[3] = 255;
    }
}

#[cfg(test)]
mod compass_tests {
    #[test]
    fn t7_compass_see_through_takes_the_ground() {
        // Three opaque dark ground texels, one bright, one see-through.
        let mut px = vec![21, 21, 21, 255, 21, 21, 21, 255, 21, 21, 21, 255, 240, 240, 240, 255, 200, 50, 50, 0, 120, 120, 120, 128];
        super::t7_compass_opaque(&mut px);
        assert_eq!(&px[16..20], &[21, 21, 21, 255]);
        assert_eq!(&px[12..16], &[240, 240, 240, 255]);
        assert_eq!(px[23], 255);
        assert_eq!(px[20], ((120u16 * 128 + 21 * 127 + 127) / 255) as u8);
    }
}
