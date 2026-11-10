//! The specular environment's lookup (code texture 64, read by the lit
//! pixel programs at `(n . v, smoothness)` with half-texel insets for 64
//! texels): x scales the surface's specular colour and y is added
//! (`spec = f0 * x + y`), and the diffuse light is scaled by
//! `1 - max(spec)`. It is the split-sum GGX integral (Karis): for
//! roughness `1 - smoothness` (alpha its square), the mean of
//! `G_vis (1 - Fc)` and `G_vis Fc` over importance-sampled half vectors,
//! `Fc = (1 - v . h)^5`. Made here once; the game's own table is made by
//! its code (not read).

use glam::Vec3;

pub const SIZE: usize = 64;
const SAMPLES: u32 = 256;

fn radical_inverse(mut b: u32) -> f32 {
    b = b.rotate_right(16);
    b = ((b & 0x5555_5555) << 1) | ((b & 0xAAAA_AAAA) >> 1);
    b = ((b & 0x3333_3333) << 2) | ((b & 0xCCCC_CCCC) >> 2);
    b = ((b & 0x0F0F_0F0F) << 4) | ((b & 0xF0F0_F0F0) >> 4);
    b = ((b & 0x00FF_00FF) << 8) | ((b & 0xFF00_FF00) >> 8);
    b as f32 * 2.328_306_4e-10
}

/// (scale, bias) for `n . v` and roughness.
pub fn integrate(n_dot_v: f32, roughness: f32) -> (f32, f32) {
    let n_dot_v = n_dot_v.max(1e-3);
    let v = Vec3::new((1.0 - n_dot_v * n_dot_v).sqrt(), 0.0, n_dot_v);
    let a = (roughness * roughness).max(1e-4);
    let k = a / 2.0;
    let g1 = |x: f32| x / (x * (1.0 - k) + k);
    let (mut sa, mut sb) = (0.0f32, 0.0f32);
    for i in 0..SAMPLES {
        let (u1, u2) = (i as f32 / SAMPLES as f32, radical_inverse(i));
        let phi = std::f32::consts::TAU * u1;
        let cos_t = ((1.0 - u2) / (1.0 + (a * a - 1.0) * u2)).sqrt();
        let sin_t = (1.0 - cos_t * cos_t).max(0.0).sqrt();
        let h = Vec3::new(sin_t * phi.cos(), sin_t * phi.sin(), cos_t);
        let l = 2.0 * v.dot(h) * h - v;
        let (n_l, n_h, v_h) = (l.z.max(0.0), h.z.max(0.0), v.dot(h).max(0.0));
        if n_l > 0.0 {
            let g_vis = g1(n_l) * g1(n_dot_v) * v_h / (n_h * n_dot_v).max(1e-6);
            let fc = (1.0 - v_h).powi(5);
            sa += (1.0 - fc) * g_vis;
            sb += fc * g_vis;
        }
    }
    (sa / SAMPLES as f32, sb / SAMPLES as f32)
}

/// The table's texels (row by smoothness, column by `n . v`): RGBA floats.
pub fn table() -> Vec<[f32; 4]> {
    let mut out = Vec::with_capacity(SIZE * SIZE);
    for y in 0..SIZE {
        let smooth = (y as f32 + 0.5) / SIZE as f32;
        for x in 0..SIZE {
            let n_dot_v = (x as f32 + 0.5) / SIZE as f32;
            let (a, b) = integrate(n_dot_v, 1.0 - smooth);
            out.push([a, b, 0.0, 1.0]);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_table_conserves_energy_and_brightens_at_grazing_angles() {
        // Facing a rough surface: about f0 itself, little added.
        let (a, b) = integrate(1.0, 1.0);
        assert!(a > 0.3 && a < 1.0 && b < 0.1, "{a} {b}");
        // At a grazing angle on a smooth one: more added.
        let (_, b2) = integrate(0.05, 0.0);
        assert!(b2 > b);
        // Never more than all the light (the diffuse keeps 1 - max).
        for t in table() {
            assert!(t[0] + t[1] <= 1.05 && t[0] >= 0.0 && t[1] >= 0.0, "{t:?}");
        }
    }
}
