//! The specular occlusion lookup (code texture 65, a 3D table): the lit
//! pixel programs read it at
//! - x: `sqrt(1 - v)`, the sine of the visibility cone's half angle (the
//!   ambient occlusion `ao = sin^2`),
//! - y: `2 / (2^(20 gloss) + 2)`, the specular lobe's width (`2 / (n + 2)`
//!   for a Phong exponent `n`),
//! - z: the cosine between the visibility cone's axis and the reflection,
//!
//! and scale the reflection by its x: the share of the specular lobe's
//! cone inside the visibility cone. Made here once (the game's own table is
//! made by its code): the lobe as a cone where a Phong lobe of exponent `n`
//! falls to a tenth, the share counted over directions spread evenly in it.
//! Bound white before, every reflection went unoccluded.

use glam::Vec3;

pub const SIZE: usize = 32;
const SAMPLES: u32 = 96;

/// The share of the lobe (half angle `cos_lobe`) inside the visibility cone
/// (half angle with sine `sin_vis`), their axes `cos_between` apart.
pub fn share(sin_vis: f32, cos_lobe: f32, cos_between: f32) -> f32 {
    let cos_vis = (1.0 - sin_vis * sin_vis).max(0.0).sqrt();
    let sin_b = (1.0 - cos_between * cos_between).max(0.0).sqrt();
    let vis_axis = Vec3::new(sin_b, 0.0, cos_between);
    let mut inside = 0u32;
    for i in 0..SAMPLES {
        // (Evenly by solid angle within the lobe's cap: a Fibonacci spiral.)
        let t = (i as f32 + 0.5) / SAMPLES as f32;
        let z = 1.0 - t * (1.0 - cos_lobe);
        let r = (1.0 - z * z).max(0.0).sqrt();
        let a = i as f32 * 2.399_963;
        let d = Vec3::new(r * a.cos(), r * a.sin(), z);
        if d.dot(vis_axis) >= cos_vis {
            inside += 1;
        }
    }
    inside as f32 / SAMPLES as f32
}

/// The table's texels (x fastest, then y, then z).
pub fn table() -> Vec<f32> {
    let mut out = Vec::with_capacity(SIZE * SIZE * SIZE);
    let at = |i: usize| (i as f32 + 0.5) / SIZE as f32;
    for zi in 0..SIZE {
        let cos_between = at(zi);
        for yi in 0..SIZE {
            let width = at(yi);
            let n = (2.0 / width - 2.0).max(0.0);
            let cos_lobe = 0.1f32.powf(1.0 / n.max(1e-3)).clamp(0.0, 0.999_99);
            for xi in 0..SIZE {
                out.push(share(at(xi), cos_lobe, cos_between));
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_open_sky_leaves_the_lobe_whole_and_a_closed_one_hides_it() {
        // Visibility cone nearly a hemisphere round the reflection: all in.
        assert!(share(0.999, 0.9, 1.0) > 0.99);
        // A narrow cone pointing away: nothing in.
        assert!(share(0.1, 0.9, 0.0) < 0.01);
        // Half-way: some.
        let s = share(0.6, 0.8, 0.8);
        assert!(s > 0.05 && s < 0.95, "{s}");
    }
}
