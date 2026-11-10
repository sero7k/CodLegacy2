//! The GPU skin textures (`gpuSkinBase`, `gpuSkinQuat`, `gpuSkinPos`, code
//! textures 0x2c..0x2e) in their rest pose.
//!
//! The `mcs/` techniques (GPU-skinned fxanim static models: Evac's vines,
//! the flags, sheets and banners) place each vertex, when the instance's
//! word 4 bone row 3 carries no CPU skin base (`.w` zero, as for every
//! static model here), from those three textures: for each of its four
//! bones (texel x = BLENDINDICES, y = the animation frames in row 3 .xy,
//! lerped by .z) it rotates `position - base` by the normalised quaternion
//! and adds `base + pos`. Bound as a 1x1 black view, the quaternion was
//! (0,0,0,0): its normalisation NaN, every vertex collapsed and none of
//! those draws wrote a sample. In the rest pose the quaternion is the
//! identity (0,0,0,1) and base / pos are zero for every bone and frame, so
//! each vertex stays at its bind position (scaled by its weights' sum, as
//! in the exe) before the instance's placement and the vegetation's wind.
//! The fxanim's own animation (the exe's skinning job writing these
//! textures from the xanim) is not played.
//!
//! `CODLEGACY_T7_GPU_SKIN=black`: the old 1x1 black view.

#[allow(unused_imports)]
use crate::profile::{ComputeExt as _, EncExt as _, PassExt as _, QueueExt as _};
use render_h1::gpu::Gpu;

use crate::gpu::{self, Tex};

/// Texels across (bones): wider than any model's bone count, so every
/// BLENDINDICES load is in bounds.
pub const WIDTH: u32 = 1024;
/// Rows (animation frames read through bone row 3 .xy, zero here).
pub const HEIGHT: u32 = 2;

/// Whether the rest-pose textures are bound (else the black view).
pub fn rest_pose_on() -> bool {
    !codlegacy_env::var("CODLEGACY_T7_GPU_SKIN").is_ok_and(|v| v.trim() == "black")
}

/// The rest-pose texture for code texture `code` (0x2c base, 0x2d quat,
/// 0x2e pos): one texel per bone and frame.
pub fn rest_pose(gpu: &Gpu, code: u32) -> Tex {
    let texel: [u8; 4] = rest_texel(code);
    let label = match code {
        0x2c => "gpuSkinBase (rest pose)",
        0x2d => "gpuSkinQuat (rest pose)",
        _ => "gpuSkinPos (rest pose)",
    };
    let t = gpu::target(gpu, label, WIDTH, HEIGHT, wgpu::TextureFormat::Rgba8Unorm, wgpu::TextureUsages::COPY_DST);
    let bytes: Vec<u8> = texel.repeat((WIDTH * HEIGHT) as usize);
    gpu.queue.write_texture_p(
        t.texture.as_image_copy(),
        &bytes,
        wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(4 * WIDTH), rows_per_image: Some(HEIGHT) },
        wgpu::Extent3d { width: WIDTH, height: HEIGHT, depth_or_array_layers: 1 },
    );
    t
}

/// A rest-pose texel (RGBA8 unorm): the identity quaternion (x, y, z, w) =
/// (0, 0, 0, 1) for `gpuSkinQuat`, zero for the base and the offset.
pub fn rest_texel(code: u32) -> [u8; 4] {
    if code == 0x2d { [0, 0, 0, 255] } else { [0, 0, 0, 0] }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The program's skinning step for one bone, as translated: rotate
    /// `p - base` by the normalised quaternion, add `base + pos`.
    fn skin(p: [f32; 3], base: [f32; 3], q: [f32; 4], pos: [f32; 3]) -> [f32; 3] {
        let n = (q.iter().map(|x| x * x).sum::<f32>()).sqrt();
        let [x, y, z, w] = q.map(|c| c / n);
        let d = [p[0] - base[0], p[1] - base[1], p[2] - base[2]];
        let r = [
            [1.0 - 2.0 * (y * y + z * z), 2.0 * (x * y - w * z), 2.0 * (x * z + w * y)],
            [2.0 * (x * y + w * z), 1.0 - 2.0 * (x * x + z * z), 2.0 * (y * z - w * x)],
            [2.0 * (x * z - w * y), 2.0 * (y * z + w * x), 1.0 - 2.0 * (x * x + y * y)],
        ];
        std::array::from_fn(|i| base[i] + pos[i] + r[i][0] * d[0] + r[i][1] * d[1] + r[i][2] * d[2])
    }

    fn unorm(t: [u8; 4]) -> [f32; 4] {
        t.map(|c| f32::from(c) / 255.0)
    }

    #[test]
    fn rest_pose_keeps_the_bind_position() {
        let (b, q, o) = (unorm(rest_texel(0x2c)), unorm(rest_texel(0x2d)), unorm(rest_texel(0x2e)));
        let p = [12.5, -40.0, 300.25];
        let s = skin(p, [b[0], b[1], b[2]], q, [o[0], o[1], o[2]]);
        for k in 0..3 {
            assert!((s[k] - p[k]).abs() < 1e-4, "{s:?} vs {p:?}");
        }
    }

    #[test]
    fn black_quaternion_collapses() {
        // The old binding: (0,0,0,0) normalises to NaN: no vertex placed.
        let s = skin([1.0, 2.0, 3.0], [0.0; 3], [0.0; 4], [0.0; 3]);
        assert!(s.iter().all(|v| v.is_nan()));
    }
}
