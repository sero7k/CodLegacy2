//! Frustum culling by bounds (`BO2MP_IW7_CULL=0`: none).

use glam::{Mat4, Vec3, Vec4};

/// Whether draws are culled.
pub fn on() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| !std::env::var("BO2MP_IW7_CULL").is_ok_and(|v| v.trim() == "0"))
}

/// A view's six planes (clip space x and y in -w..w, z in 0..w), each
/// `n . p + d >= 0` inside.
#[derive(Clone, Copy, Debug)]
pub struct Frustum {
    planes: [Vec4; 6],
    /// What the view's positions are about (subtracted from the bounds).
    about: Vec3,
}

impl Frustum {
    /// From a view-projection taking positions about `about`.
    pub fn new(view_projection: Mat4, about: Vec3) -> Self {
        let r = |i: usize| view_projection.row(i);
        let (x, y, z, w) = (r(0), r(1), r(2), r(3));
        Self { planes: [w + x, w - x, w + y, w - y, z, w - z], about }
    }

    /// Whether the box `lo`..`hi` (world) can be seen.
    pub fn sees(&self, lo: Vec3, hi: Vec3) -> bool {
        let (lo, hi) = (lo - self.about, hi - self.about);
        self.planes.iter().all(|p| {
            let n = p.truncate();
            // (The corner furthest along the plane's normal.)
            let c = Vec3::select(n.cmpge(Vec3::ZERO), hi, lo);
            n.dot(c) + p.w >= 0.0
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_box_ahead_is_seen_and_one_behind_is_not() {
        let eye = Vec3::new(100.0, 0.0, 50.0);
        let vp = Mat4::perspective_infinite_reverse_rh(1.2, 16.0 / 9.0, 4.0) * Mat4::look_to_rh(Vec3::ZERO, Vec3::X, Vec3::Z);
        let f = Frustum::new(vp, eye);
        let at = |c: Vec3| f.sees(c - Vec3::splat(10.0), c + Vec3::splat(10.0));
        assert!(at(eye + Vec3::X * 500.0));
        assert!(!at(eye - Vec3::X * 500.0));
        assert!(!at(eye + Vec3::Y * 5000.0 + Vec3::X * 100.0));
        // Round the eye: seen.
        assert!(f.sees(eye - Vec3::splat(50.0), eye + Vec3::splat(50.0)));
    }
}

/// A fixed buffer of floats filled in order (per-frame uniforms without
/// allocation).
pub struct Pack<const N: usize> {
    pub a: [f32; N],
    pub n: usize,
}

impl<const N: usize> Pack<N> {
    pub fn new() -> Self {
        Self { a: [0.0; N], n: 0 }
    }

    pub fn extend_from_slice(&mut self, v: &[f32]) {
        self.a[self.n..self.n + v.len()].copy_from_slice(v);
        self.n += v.len();
    }

    pub fn len(&self) -> usize {
        self.n
    }

    pub fn is_empty(&self) -> bool {
        self.n == 0
    }

    pub fn bytes(&self) -> &[u8] {
        bytemuck::cast_slice(&self.a[..self.n])
    }
}

impl<const N: usize> Default for Pack<N> {
    fn default() -> Self {
        Self::new()
    }
}
