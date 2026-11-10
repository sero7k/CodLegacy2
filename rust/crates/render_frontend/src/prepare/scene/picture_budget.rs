//! bo2mp: the pictures a match hands to the card against the card's
//! memory. A Black Ops II-rules match hands every material picture of the
//! map and of every guest gun pack to `Assets<Image>` (a Ghosts map with
//! the owner's gun packs: 20,000 pictures, 7.6 GB), which a 4 GB laptop
//! card cannot hold. Before they are handed over, the top levels of the
//! large ones are left out until the whole set fits a share of the card's
//! memory: each picture keeps its own mip chain from the first level that
//! fits the cap, so nothing is resampled, and the card samples the levels
//! it would have sampled at a distance anyway.

use bevy::prelude::Image;
use bevy::render::render_resource::{Extent3d, TextureFormat};
use frame::settings::native_quality::{self, VramClass};

/// A picture's size as handed over.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PictureSize {
    pub width: u32,
    pub height: u32,
    pub levels: u32,
    pub layers: u32,
    pub bytes: u64,
}

/// The picture caps tried, largest first.
pub const CAPS: [u32; 4] = [2048, 1024, 512, 256];

/// How many top levels a picture loses under `cap`: levels until its
/// longer side is at or under the cap, keeping at least one; a picture
/// with layers (a cube map) or one level keeps every level.
pub fn levels_dropped(p: &PictureSize, cap: u32) -> u32 {
    if p.layers > 1 || p.levels <= 1 {
        return 0;
    }
    let mut drop = 0;
    while drop + 1 < p.levels && (p.width >> drop).max(p.height >> drop) > cap {
        drop += 1;
    }
    drop
}

/// A picture's bytes once `drop` top levels are gone (a mip chain is a
/// quarter smaller for each level dropped).
pub fn bytes_after(p: &PictureSize, drop: u32) -> u64 {
    p.bytes >> (2 * drop.min(31))
}

/// The largest cap under which the set fits `budget` bytes (`u32::MAX`
/// when it fits as it is); the smallest cap when none does.
pub fn cap_for_budget(sizes: &[PictureSize], budget: u64) -> u32 {
    let total = |cap: u32| sizes.iter().map(|p| bytes_after(p, levels_dropped(p, cap))).sum::<u64>();
    if total(u32::MAX) <= budget {
        return u32::MAX;
    }
    CAPS.into_iter().find(|&cap| total(cap) <= budget).unwrap_or(CAPS[CAPS.len() - 1])
}

/// The share of the card's memory the match's pictures may take: 60% of a
/// card with plenty, 40% of a 4 GB one (the frame's targets, shadow maps,
/// the map's own pictures under a native renderer and the driver take the
/// rest), 35% of a 2 GB one. A card whose memory is not reported is taken
/// as 4 GB (`Low`) or 2 GB (`Tight`); with plenty and no figure there is
/// no budget. `BO2MP_PICTURE_BUDGET_MB=<n>` sets it; `=0` turns it off.
pub fn budget_for(vram_bytes: Option<u64>, class: VramClass) -> Option<u64> {
    if let Ok(v) = std::env::var("BO2MP_PICTURE_BUDGET_MB")
        && let Ok(mb) = v.trim().parse::<u64>()
    {
        return (mb > 0).then_some(mb << 20);
    }
    const GIB: u64 = 1 << 30;
    let (share, assumed) = match class {
        VramClass::Plenty => (60, None),
        VramClass::Low => (40, Some(4 * GIB)),
        VramClass::Tight => (35, Some(2 * GIB)),
    };
    let bytes = vram_bytes.or(assumed)?;
    Some(bytes / 100 * share)
}

/// The budget for the card the renderer found (None: none).
pub fn current_budget() -> Option<u64> {
    let v = native_quality::vram();
    budget_for(v.as_ref().and_then(|v| v.bytes), native_quality::vram_class())
}

/// The size of one level of a picture, in bytes, for its format.
fn level_bytes(format: TextureFormat, width: u32, height: u32) -> Option<u64> {
    let (bw, bh) = format.block_dimensions();
    let block = u64::from(format.block_copy_size(None)?);
    let w = u64::from(width.max(1)).div_ceil(u64::from(bw));
    let h = u64::from(height.max(1)).div_ceil(u64::from(bh));
    Some(w * h * block)
}

pub fn size_of(image: &Image) -> PictureSize {
    let d = &image.texture_descriptor;
    PictureSize {
        width: d.size.width,
        height: d.size.height,
        levels: d.mip_level_count,
        layers: d.size.depth_or_array_layers,
        bytes: image.data.as_ref().map_or(0, |d| d.len() as u64),
    }
}

/// The picture with its top levels dropped to `cap` (None: nothing to
/// drop, or its data is not the plain level-after-level layout its
/// descriptor says).
pub fn capped(image: &Image, cap: u32) -> Option<Image> {
    let p = size_of(image);
    let drop = levels_dropped(&p, cap);
    if drop == 0 {
        return None;
    }
    let data = image.data.as_ref()?;
    let d = &image.texture_descriptor;
    let mut offset = 0u64;
    let mut expected = 0u64;
    for level in 0..d.mip_level_count {
        let bytes = level_bytes(d.format, d.size.width >> level, d.size.height >> level)?;
        if level < drop {
            offset += bytes;
        }
        expected += bytes;
    }
    if expected != data.len() as u64 {
        return None;
    }
    let mut out = Image::new_uninit(
        Extent3d {
            width: (d.size.width >> drop).max(1),
            height: (d.size.height >> drop).max(1),
            depth_or_array_layers: 1,
        },
        d.dimension,
        d.format,
        image.asset_usage,
    );
    out.texture_descriptor.mip_level_count = d.mip_level_count - drop;
    out.texture_descriptor.usage = d.usage;
    out.texture_descriptor.label = d.label;
    out.texture_view_descriptor = image.texture_view_descriptor.clone();
    out.sampler = image.sampler.clone();
    out.data = Some(data[offset as usize..].to_vec());
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pic(side: u32, levels: u32) -> PictureSize {
        // BC1: half a byte a texel, the chain a third more than the top.
        let top = u64::from(side) * u64::from(side) / 2;
        PictureSize { width: side, height: side, levels, layers: 1, bytes: top * 4 / 3 }
    }

    #[test]
    fn levels_drop_until_the_side_fits() {
        assert_eq!(levels_dropped(&pic(2048, 12), u32::MAX), 0);
        assert_eq!(levels_dropped(&pic(2048, 12), 1024), 1);
        assert_eq!(levels_dropped(&pic(2048, 12), 256), 3);
        assert_eq!(levels_dropped(&pic(512, 10), 1024), 0);
        // One level, or a cube map: untouched.
        assert_eq!(levels_dropped(&pic(2048, 1), 256), 0);
        assert_eq!(levels_dropped(&PictureSize { layers: 6, ..pic(2048, 12) }, 256), 0);
        // Never below the last level.
        assert_eq!(levels_dropped(&pic(2048, 2), 128), 1);
    }

    #[test]
    fn the_cap_is_the_largest_that_fits_the_budget() {
        let set: Vec<PictureSize> = (0..100).map(|_| pic(2048, 12)).collect();
        let total: u64 = set.iter().map(|p| p.bytes).sum();
        assert_eq!(cap_for_budget(&set, total), u32::MAX);
        assert_eq!(cap_for_budget(&set, total / 3), 1024);
        assert_eq!(cap_for_budget(&set, total / 10), 512);
        assert_eq!(cap_for_budget(&set, 1), 256);
        assert_eq!(cap_for_budget(&[], 0), u32::MAX);
    }

    #[test]
    fn the_budget_is_a_share_of_the_card() {
        const GIB: u64 = 1 << 30;
        assert_eq!(budget_for(Some(16 * GIB), VramClass::Plenty), Some(16 * GIB / 100 * 60));
        assert_eq!(budget_for(Some(4 * GIB), VramClass::Low), Some(4 * GIB / 100 * 40));
        assert_eq!(budget_for(None, VramClass::Low), Some(4 * GIB / 100 * 40));
        assert_eq!(budget_for(None, VramClass::Tight), Some(2 * GIB / 100 * 35));
        assert_eq!(budget_for(None, VramClass::Plenty), None);
    }

    #[test]
    fn a_capped_picture_keeps_the_lower_levels_bytes() {
        use bevy::asset::RenderAssetUsages;
        use bevy::render::render_resource::TextureDimension;
        // An 8x8 RGBA8 picture with four levels: 256 + 64 + 16 + 4 bytes,
        // each level filled with its number.
        let mut data = Vec::new();
        for (level, side) in [8u32, 4, 2, 1].into_iter().enumerate() {
            data.extend(std::iter::repeat_n(level as u8, (side * side * 4) as usize));
        }
        let mut image = Image::new_uninit(
            Extent3d { width: 8, height: 8, depth_or_array_layers: 1 },
            TextureDimension::D2,
            TextureFormat::Rgba8UnormSrgb,
            RenderAssetUsages::RENDER_WORLD,
        );
        image.texture_descriptor.mip_level_count = 4;
        image.data = Some(data);
        let c = capped(&image, 2).expect("capped");
        assert_eq!(c.texture_descriptor.size.width, 2);
        assert_eq!(c.texture_descriptor.mip_level_count, 2);
        let d = c.data.as_ref().unwrap();
        assert_eq!(d.len(), 16 + 4);
        assert!(d[..16].iter().all(|&b| b == 2) && d[16..].iter().all(|&b| b == 3));
        assert!(capped(&image, 8).is_none());
        // Data that is not the level layout is left alone.
        image.data.as_mut().unwrap().push(0);
        assert!(capped(&image, 2).is_none());
    }
}
