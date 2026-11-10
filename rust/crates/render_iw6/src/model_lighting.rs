//! `MODEL_LIGHTING`: every placed model's 56-colour light table as the
//! shell of a 4 x 4 x 4 block of one 3-D texture. The model shaders read
//! it with trilinear filtering at `BASE_LIGHTING_COORDS + N / max|N| *
//! LIGHTING_LOOKUP_SCALE` (`docs/exe_notes/IW6_SHADERS.md` 7): the block's
//! centre plus the normal pushed onto the cube's shell, which lands on the
//! shell's texel centres. The table's order is the lattice's (k outer, then
//! j, then i), the shell only; the eight inner texels are never read on
//! the shell and hold the table's mean. Alpha is the sun's visibility.

use render_h1::gpu::{Gpu, Tex};

/// Blocks per row of the volume.
const ROW: usize = 64;

pub struct Volume {
    pub tex: Tex,
    size: [u32; 3],
}

/// Whether a lattice point is on the cube's shell.
fn on_shell(i: usize, j: usize, k: usize) -> bool {
    !((1..=2).contains(&i) && (1..=2).contains(&j) && (1..=2).contains(&k))
}

/// One block's 64 texels (x fastest, then y, then z) from a table.
pub fn block(table: &[[f32; 3]; 56], visibility: f32) -> [[f32; 4]; 64] {
    let mut mean = [0f32; 3];
    for c in table {
        for ch in 0..3 {
            mean[ch] += c[ch] / 56.0;
        }
    }
    let mut out = [[0f32; 4]; 64];
    let mut n = 0usize;
    for k in 0..4 {
        for j in 0..4 {
            for i in 0..4 {
                let c = if on_shell(i, j, k) {
                    n += 1;
                    table[n - 1]
                } else {
                    mean
                };
                out[i + 4 * j + 16 * k] = [c[0], c[1], c[2], visibility];
            }
        }
    }
    out
}

/// f16 bits -> f32.
pub fn unhalf(h: u16) -> f32 {
    let s = if h & 0x8000 != 0 { -1.0 } else { 1.0 };
    let e = i32::from((h >> 10) & 0x1f);
    let m = f32::from(h & 0x3ff);
    s * match e {
        0 => m * 2f32.powi(-24),
        31 => if m == 0.0 { f32::INFINITY } else { f32::NAN },
        _ => (1.0 + m / 1024.0) * 2f32.powi(e - 15),
    }
}

pub fn half(v: f32) -> u16 {
    // f32 -> f16, round to nearest (values are light: finite, small).
    let b = v.to_bits();
    let sign = ((b >> 16) & 0x8000) as u16;
    let exp = ((b >> 23) & 0xff) as i32 - 127 + 15;
    let mant = b & 0x7f_ffff;
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
        let m = (mant | 0x80_0000) >> (1 - exp);
        return sign | ((m + 0x1000) >> 13) as u16;
    }
    let r = sign as u32 | ((exp as u32) << 10) | (mant >> 13);
    (r + ((mant >> 12) & 1)) as u16
}

impl Volume {
    pub fn build(gpu: &Gpu, tiles: &[([[f32; 3]; 56], f32)]) -> Self {
        let n = tiles.len().max(1);
        let bx = n.min(ROW);
        let by = n.div_ceil(ROW);
        let size = [4 * bx as u32, 4 * by as u32, 4];
        let (w, h) = (size[0] as usize, size[1] as usize);
        let mut data = vec![0u16; w * h * 4 * 4];
        let grey = ([[0.3f32; 3]; 56], 1.0);
        for t in 0..n {
            let (table, vis) = tiles.get(t).unwrap_or(&grey);
            let b = block(table, *vis);
            let (ox, oy) = (4 * (t % ROW), 4 * (t / ROW));
            for k in 0..4 {
                for j in 0..4 {
                    for i in 0..4 {
                        let at = 4 * ((ox + i) + w * ((oy + j) + h * k));
                        for c in 0..4 {
                            data[at + c] = half(b[i + 4 * j + 16 * k][c]);
                        }
                    }
                }
            }
        }
        let texture = wgpu::util::DeviceExt::create_texture_with_data(
            &gpu.device,
            &gpu.queue,
            &wgpu::TextureDescriptor {
                label: Some("iw6_model_lighting"),
                size: wgpu::Extent3d {
                    width: size[0],
                    height: size[1],
                    depth_or_array_layers: size[2],
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D3,
                format: wgpu::TextureFormat::Rgba16Float,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            },
            wgpu::util::TextureDataOrder::LayerMajor,
            bytemuck::cast_slice(&data),
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
            size,
        }
    }

    /// `BASE_LIGHTING_COORDS` of a table: its block's centre.
    pub fn coords(&self, tile: usize) -> [f32; 4] {
        let (ox, oy) = ((4 * (tile % ROW)) as f32, (4 * (tile / ROW)) as f32);
        [
            (ox + 2.0) / self.size[0] as f32,
            (oy + 2.0) / self.size[1] as f32,
            2.0 / self.size[2] as f32,
            0.0,
        ]
    }

    /// `LIGHTING_LOOKUP_SCALE`: 1.5 texels each way, to the shell's texel
    /// centres.
    pub fn lookup_scale(&self) -> [f32; 4] {
        [
            1.5 / self.size[0] as f32,
            1.5 / self.size[1] as f32,
            1.5 / self.size[2] as f32,
            0.0,
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_shell_holds_the_table_in_lattice_order() {
        let mut t = [[0f32; 3]; 56];
        for (n, c) in t.iter_mut().enumerate() {
            *c = [n as f32; 3];
        }
        let b = block(&t, 0.5);
        // (0,0,0) is the first, (3,3,3) the last; (1,1,1) inner (the mean).
        assert_eq!(b[0][0], 0.0);
        assert_eq!(b[63][0], 55.0);
        assert!((b[1 + 4 + 16][0] - 27.5).abs() < 1e-4);
        assert_eq!(b[0][3], 0.5);
        // The table's directions agree with the shell positions.
        let dirs = fastfile_iw6::lightgrid::table_directions();
        assert_eq!(dirs.len(), 56);
        assert!(dirs[0][0] < 0.0 && dirs[0][1] < 0.0 && dirs[0][2] < 0.0);
    }

    #[test]
    fn halves() {
        for v in [0.0f32, 1.0, 0.5, 2.25, 1e-3, 65000.0] {
            let h = half(v);
            let back = {
                let e = (h >> 10) & 0x1f;
                let m = f32::from(h & 0x3ff);
                if e == 0 { m * 2f32.powi(-24) } else { (1.0 + m / 1024.0) * 2f32.powi(i32::from(e) - 15) }
            };
            assert!((back - v).abs() <= v * 1e-3 + 1e-6, "{v} {back}");
        }
    }
}
