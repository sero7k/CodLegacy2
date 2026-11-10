//! bo2mp: an Infinite Warfare model material's own light (its emissive map
//! times `emissiveParams.x`: `fastfile_iw7::glow`): Spaceland's neon tubes,
//! lit signs and arcade panels, wall lamps.
//!
//! Drawn through Black Ops II's emissive tile path (`T6Draw::tile`, the
//! fallback draw's `TILE` prop style: the colour map lit, plus a glow
//! picture squared, in the specular map's place; constants in an auxiliary
//! texel), which BO2's own lava rocks use. Its constants are a uv scale of
//! one and no macro normal (`Micro_Scale` 1, 1; `Macro_Height` 0).

use std::sync::Arc;

use asset_core::{AssetNamespace, AssetRef};
use asset_material::{AuthoredImage, MaterialCatalog, MaterialTextureBinding, TS_EMISSIVE_MAP, TS_SPECULAR_MAP};
use bevy::asset::RenderAssetUsages;
use bevy::prelude::Image;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use fastfile_iw7::map::{Iw7Map, MapMaterial};

/// The largest glow picture (it varies slowly; a sign's lettering keeps).
pub(super) const GLOW_MAX: u32 = 256;

/// The brightest a glow texel is drawn, in the draw's light units (a lit
/// white surface under a unit light is 1): brighter keeps its hue, scaled
/// down to this (IW7 blooms a neon core white; BO2's draw has no bloom, so
/// the hue is kept instead of clipping every channel).
pub(super) const GLOW_CAP: f32 = 2.5;

/// The auxiliary texel's semantic (the tile path's constants).
const TILE_AUX_SEMANTIC: u8 = 0xf1;

fn half_bits(v: f32) -> u16 {
    let b = v.to_bits();
    let sign = ((b >> 16) & 0x8000) as u16;
    let exp = ((b >> 23) & 0xff) as i32 - 127 + 15;
    let mant = b & 0x7f_ffff;
    if v.is_nan() {
        return 0x7e00;
    }
    if exp >= 31 {
        return sign | 0x7c00;
    }
    if exp <= 0 {
        return sign;
    }
    let half = sign | ((exp as u16) << 10) | (mant >> 13) as u16;
    if mant & 0x1000 != 0 { half + 1 } else { half }
}

/// An RGBA16F picture `w` x `h` of `rows`.
fn rgba16f(w: u32, h: u32, rows: &[[f32; 4]]) -> Image {
    let data: Vec<u8> = rows.iter().flatten().flat_map(|&v| half_bits(v).to_le_bytes()).collect();
    let mut image = Image::new_uninit(Extent3d { width: w, height: h, depth_or_array_layers: 1 }, TextureDimension::D2, TextureFormat::Rgba16Float, RenderAssetUsages::RENDER_WORLD);
    image.data = Some(data);
    image
}

/// A glow texel in the draw's units (HDR light times `scale`, the map's
/// HDR-to-draw scale), hue kept under `GLOW_CAP`, as the square root the
/// shader squares.
pub(super) fn glow_texel(light: [f32; 3], scale: f32) -> [f32; 4] {
    let mut v = light.map(|c| (c * scale).max(0.0));
    let peak = v.iter().copied().fold(0.0, f32::max);
    if peak > GLOW_CAP {
        v = v.map(|c| c * GLOW_CAP / peak);
    }
    [v[0].sqrt(), v[1].sqrt(), v[2].sqrt(), 1.0]
}

/// The largest picture of a gun's glow: the sights' reticles are small
/// parts, so more than a map's neon tubes keep.
pub(super) const GUN_GLOW_MAX: u32 = 512;

/// The gain on a gun's glow in the draw's light units (`BO2MP_IW7_GLOW`,
/// default 1; 0 turns the glow off). IW7 adds `emissive * emissiveParams.x`
/// to its lit colour; the guns' parameters run 1 to 4, and a lit surface
/// under a unit light is 1 here.
pub(super) fn gun_gain() -> f32 {
    std::env::var("BO2MP_IW7_GLOW").ok().and_then(|v| v.trim().parse::<f32>().ok()).filter(|g| g.is_finite() && *g >= 0.0).unwrap_or(1.0)
}

/// A gun glow texel: linear light times `gain`, its hue kept under
/// `GLOW_CAP` (not square-rooted: the gun path reads it as it is).
pub(super) fn gun_glow_texel(light: [f32; 3], gain: f32) -> [f32; 4] {
    let mut v = light.map(|c| (c * gain).max(0.0));
    let peak = v.iter().copied().fold(0.0, f32::max);
    if peak > GLOW_CAP {
        v = v.map(|c| c * GLOW_CAP / peak);
    }
    [v[0], v[1], v[2], 1.0]
}

/// Links a gun material's glow picture (`TS_EMISSIVE_MAP`); `None` when the
/// picture is dark all over (nothing would glow).
pub(super) fn link_gun_glow(catalog: &mut MaterialCatalog, map: &Iw7Map, m: &MapMaterial, gain: f32) -> Result<Option<MaterialTextureBinding>, String> {
    let glow = map.glow(m, GUN_GLOW_MAX)?;
    let rows: Vec<[f32; 4]> = glow.light.iter().map(|&l| gun_glow_texel(l, gain)).collect();
    if rows.iter().all(|t| t[0].max(t[1]).max(t[2]) < 0.004) {
        return Ok(None);
    }
    let image = rgba16f(glow.width, glow.height, &rows);
    let slot = catalog.link_image(AuthoredImage {
        namespace: AssetNamespace::T6,
        name: AssetRef::Real(format!("iw7:{}#gun_glow", m.name)),
        map_type: 0,
        semantic: TS_EMISSIVE_MAP,
        category: 0,
        use_srgb_reads: false,
        width: glow.width as u16,
        height: glow.height as u16,
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
    Ok(Some(MaterialTextureBinding { name_hash: 0, name_start: 0, name_end: 0, sampler_state: 0x14, semantic: TS_EMISSIVE_MAP, image: Some(slot) }))
}

/// Links a material's glow and the tile constants; the two texture
/// bindings (glow in the specular map's place, constants) to add, or why
/// not.
pub(super) fn link_glow(catalog: &mut MaterialCatalog, map: &Iw7Map, m: &MapMaterial, scale: f32) -> Result<[MaterialTextureBinding; 2], String> {
    let glow = map.glow(m, GLOW_MAX)?;
    let rows: Vec<[f32; 4]> = glow.light.iter().map(|&l| glow_texel(l, scale)).collect();
    let pictures = [(rgba16f(glow.width, glow.height, &rows), "glow", TS_SPECULAR_MAP), (rgba16f(1, 1, &[[1.0, 1.0, 0.0, 0.0]]), "tile", TILE_AUX_SEMANTIC)];
    let mut out = Vec::with_capacity(2);
    for (image, suffix, semantic) in pictures {
        let size = image.texture_descriptor.size;
        let slot = catalog.link_image(AuthoredImage {
            namespace: AssetNamespace::T6,
            name: AssetRef::Real(format!("iw7:{}#tile_{suffix}", m.name)),
            map_type: 0,
            semantic,
            category: 0,
            use_srgb_reads: false,
            width: size.width as u16,
            height: size.height as u16,
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
        out.push(MaterialTextureBinding { name_hash: 0, name_start: 0, name_end: 0, sampler_state: 0x14, semantic, image: Some(slot) });
    }
    let [a, b]: [MaterialTextureBinding; 2] = out.try_into().map_err(|_| "two bindings".to_owned())?;
    Ok([a, b])
}

#[cfg(test)]
mod tests {
    #[test]
    fn gun_glow_is_linear_light_with_its_hue_kept() {
        let t = super::gun_glow_texel([0.5, 0.25, 0.0], 2.0);
        assert!((t[0] - 1.0).abs() < 1e-6 && (t[1] - 0.5).abs() < 1e-6 && t[2] == 0.0);
        let hot = super::gun_glow_texel([10.0, 5.0, 0.0], 1.0);
        assert!((hot[0] - super::GLOW_CAP).abs() < 1e-5 && (hot[1] / hot[0] - 0.5).abs() < 1e-5);
    }

    #[test]
    fn glow_keeps_its_hue_under_the_cap() {
        let t = super::glow_texel([10.0, 3.0, 0.5], 1.0);
        let sq = [t[0] * t[0], t[1] * t[1], t[2] * t[2]];
        assert!((sq[0] - super::GLOW_CAP).abs() < 1e-4);
        assert!((sq[1] / sq[0] - 0.3).abs() < 1e-4);
        let dim = super::glow_texel([0.5, 0.25, 0.0], 1.0);
        assert!((dim[0] * dim[0] - 0.5).abs() < 1e-5);
    }
}
