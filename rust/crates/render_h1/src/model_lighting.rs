//! The `MODEL_LIGHTING` volume: a 3-D atlas 4 deep, one 4x4x4 tile per
//! lit model. A model's 56 light grid colours fill its tile's surface
//! texels (the inner eight repeat the corners), and the shaders read it at
//! the tile's centre plus the normal pushed out to the cube's surface
//! (`BASE_LIGHTING_COORDS` + n / max|n| x `LIGHTING_LOOKUP_SCALE`). Alpha is
//! the sun's visibility at the model.
//!
//! Read from MWR's executable (`docs/H1_NATIVE.md`, "Model lighting
//! atlas"): the atlas is **1024 texels wide** (256 tiles a row); tile
//! `h - 1` (the lighting handle less one) sits at column `(h - 1) mod 256`,
//! row `floor((h - 1) / 256)`; its upload writes the 64 texels x fastest,
//! then y, then z, from the 56 colours in exactly `SHELL_TO_TEXEL`'s order,
//! alpha the record's visibility as a half; `LIGHTING_LOOKUP_SCALE = (1.5 /
//! 1024, 1.5 / height, 1.5 / 4, 0)`. The engine's height is a power of two
//! from its tile budget; ours is the power of two that holds the map's
//! tiles (it reaches the shaders only through `1 / height`, in both
//! constants alike). `BO2MP_H1_ATLAS=mw2` brings back the earlier 256-wide
//! layout (Modern Warfare 2's, `lighting_iw4`).

use wgpu::util::DeviceExt;

use crate::gpu::{Gpu, Tex};

/// MWR's atlas width (read).
pub const ATLAS_WIDTH: u32 = 1024;
/// Modern Warfare 2's width, used before (`BO2MP_H1_ATLAS=mw2`).
pub const MW2_ATLAS_WIDTH: u32 = 256;
pub const TILE: u32 = 4;
/// The engine's step from a tile's centre to its shell: 1.5 texels.
pub const LOOKUP_TEXELS: f32 = 1.5;

/// Shell colour (0..56) each tile texel takes (x fastest, then y, then z).
pub const SHELL_TO_TEXEL: [u8; 64] = [
    0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 0, 3, 21, 22, 12, 15,
    23, 24, 25, 26, 27, 28, 29, 30, 31, 32, 40, 43, 33, 34, 52, 55, 35, 36, 37, 38, 39, 40, 41, 42,
    43, 44, 45, 46, 47, 48, 49, 50, 51, 52, 53, 54, 55,
];

/// The atlas width `BO2MP_H1_ATLAS` asks for (default MWR's 1024).
pub fn width_from_env() -> u32 {
    match std::env::var("BO2MP_H1_ATLAS") {
        Ok(v) if v.trim().eq_ignore_ascii_case("mw2") => MW2_ATLAS_WIDTH,
        _ => ATLAS_WIDTH,
    }
}

/// The atlas height for `tiles` tiles at `width`: rows of 4 texels, a
/// power of two.
pub fn atlas_height(tiles: usize, width: u32) -> u32 {
    let rows = (tiles as u32).div_ceil(width / TILE).max(1);
    (rows * TILE).next_power_of_two()
}

/// `BASE_LIGHTING_COORDS` of tile `i` (the handle less one): its centre,
/// `((i mod n) * 4 + 2) / width, (floor(i / n) * 4 + 2) / height, 0.5, 1`
/// with `n` tiles a row.
pub fn tile_coords(i: usize, width: u32, height: u32) -> [f32; 4] {
    let per_row = width / TILE;
    let tx = (i as u32 % per_row) * TILE;
    let ty = (i as u32 / per_row) * TILE;
    [
        (tx as f32 + 2.0) / width as f32,
        (ty as f32 + 2.0) / height as f32,
        0.5,
        1.0,
    ]
}

/// `LIGHTING_LOOKUP_SCALE` as the engine fills it.
pub fn lookup_scale(width: u32, height: u32) -> [f32; 4] {
    [
        LOOKUP_TEXELS / width as f32,
        LOOKUP_TEXELS / height as f32,
        LOOKUP_TEXELS / TILE as f32,
        0.0,
    ]
}

pub struct Atlas {
    pub tex: Tex,
    pub width: u32,
    pub height: u32,
}

fn half(v: f32) -> u16 {
    // Round to nearest, finite range only (lighting is small and positive).
    let b = v.to_bits();
    let sign = ((b >> 16) & 0x8000) as u16;
    let exp = ((b >> 23) & 0xff) as i32 - 127 + 15;
    let man = b & 0x7f_ffff;
    if v.is_nan() {
        return 0x7e00;
    }
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
    let h = sign as u32 | ((exp as u32) << 10) | (man >> 13);
    (h + ((man >> 12) & 1)) as u16
}

impl Atlas {
    /// One tile per entry: 56 shell colours and a sun visibility.
    pub fn build(gpu: &Gpu, tiles: &[([[f32; 3]; 56], f32)]) -> Self {
        let width = width_from_env();
        let per_row = width / TILE;
        let height = atlas_height(tiles.len(), width);
        let mut texels = vec![0u16; (width * height * TILE * 4) as usize];
        for (i, (shell, vis)) in tiles.iter().enumerate() {
            let tx = (i as u32 % per_row) * TILE;
            let ty = (i as u32 / per_row) * TILE;
            for (t, &s) in SHELL_TO_TEXEL.iter().enumerate() {
                let (x, y, z) = (t as u32 % 4, (t as u32 / 4) % 4, t as u32 / 16);
                let at = (((z * height + ty + y) * width + tx + x) * 4) as usize;
                let c = shell[s as usize];
                texels[at..at + 4].copy_from_slice(&[
                    half(c[0]),
                    half(c[1]),
                    half(c[2]),
                    half(*vis),
                ]);
            }
        }
        let texture = gpu.device.create_texture_with_data(
            &gpu.queue,
            &wgpu::TextureDescriptor {
                label: Some("model_lighting"),
                size: wgpu::Extent3d {
                    width,
                    height,
                    depth_or_array_layers: TILE,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D3,
                format: wgpu::TextureFormat::Rgba16Float,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            },
            wgpu::util::TextureDataOrder::LayerMajor,
            bytemuck::cast_slice(&texels),
        );
        let view = texture.create_view(&wgpu::TextureViewDescriptor {
            dimension: Some(wgpu::TextureViewDimension::D3),
            ..Default::default()
        });
        Self {
            tex: Tex {
                texture,
                view,
                dim: wgpu::TextureViewDimension::D3,
            },
            width,
            height,
        }
    }

    /// `BASE_LIGHTING_COORDS` of tile `i`: its centre.
    pub fn coords(&self, i: usize) -> [f32; 4] {
        tile_coords(i, self.width, self.height)
    }

    /// `LIGHTING_LOOKUP_SCALE`: one and a half texels, centre to shell.
    pub fn lookup_scale(&self) -> [f32; 4] {
        lookup_scale(self.width, self.height)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The engine's own coordinate maths for lighting handle `h` (read):
    /// column `((h - 1) & 0xff) * 4 + 2` over 1024, row `(((h - 1) >> 6) &
    /// !3) + 2` over the height.
    fn engine_coords(h: u32, height: u32) -> [f32; 2] {
        let x = (((h - 1) & 0xff) << 2) as f32 + 2.0;
        let y = (((h - 1) >> 6) & 0x3ff_fffc) as f32 + 2.0;
        [x * (1.0 / 1024.0), y * (1.0 / height as f32)]
    }

    #[test]
    fn coords_match_the_engine() {
        for &(i, height) in &[(0usize, 4u32), (255, 4), (256, 8), (1000, 16), (4095, 64)] {
            let c = tile_coords(i, ATLAS_WIDTH, height);
            let e = engine_coords(i as u32 + 1, height);
            assert_eq!([c[0], c[1]], e, "tile {i}");
            assert_eq!(&c[2..], &[0.5, 1.0]);
        }
    }

    #[test]
    fn lookup_scale_is_the_engines() {
        // x and z as the engine's immediates (1.5 / 1024, 0.375).
        let s = lookup_scale(ATLAS_WIDTH, 64);
        assert_eq!(s[0].to_bits(), 0x3ac0_0000);
        assert_eq!(s[1], 1.5 / 64.0);
        assert_eq!(s[2].to_bits(), 0x3ec0_0000);
        assert_eq!(s[3], 0.0);
    }

    #[test]
    fn height_holds_every_tile() {
        assert_eq!(atlas_height(0, ATLAS_WIDTH), 4);
        assert_eq!(atlas_height(256, ATLAS_WIDTH), 4);
        assert_eq!(atlas_height(257, ATLAS_WIDTH), 8);
        assert_eq!(atlas_height(5073, ATLAS_WIDTH), 128);
        assert_eq!(atlas_height(5073, MW2_ATLAS_WIDTH), 512);
    }

    #[test]
    fn fill_order_is_the_engines() {
        // The upload's 64 texels, x fastest then y then z, take these of
        // the 56 colours (read from the engine's tile writer).
        let read: [u8; 64] = [
            0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 0, 3, 21, 22,
            12, 15, 23, 24, 25, 26, 27, 28, 29, 30, 31, 32, 40, 43, 33, 34, 52, 55, 35, 36, 37, 38,
            39, 40, 41, 42, 43, 44, 45, 46, 47, 48, 49, 50, 51, 52, 53, 54, 55,
        ];
        assert_eq!(SHELL_TO_TEXEL, read);
    }
}
