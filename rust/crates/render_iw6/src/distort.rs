//! Ghosts distortion effects (heat haze, shockwaves: effect sprites whose
//! material is a `distortion*` technique set) drawn over a copy of the scene.
//!
//! The effect pass never draws them (they would be flat sprites); the
//! effect system hands each visible one over as a quad
//! (`asset_world::Iw6DistortionQuad`), and `Host::set_distortion` writes the
//! quads into a small dynamic world mesh per distortion material (a draw of
//! the blended list, made on first sight like a decal ring: 32 quads, the
//! unused ones degenerate). The technique samples the scene behind it as the
//! code texture `RESOLVED_POST_SUN`, so when any distortion quad is in view
//! `Host::record_fog` copies the fogged scene into that texture before the
//! blended surfaces are drawn (one full-target copy; none when no quad is
//! visible, and the texture is made at the first quad). The quad carries the
//! sprite's colour in its vertex colour, its uv 0..1 and its facing as the
//! normal; the technique's own shaders do the rest.
//!
//! INFERRED: that `RESOLVED_POST_SUN` is the scene after the fog and before
//! the blended surfaces (the exe resolves it at the "post sun" point), and
//! the quad's vertex contents. Not seen on a GPU. `BO2MP_IW6_DISTORTION=0`
//! turns it off.

use crate::world::Renderer;

/// `BO2MP_IW6_DISTORTION=0`: no distortion effects (they are left out).
pub fn on() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| !std::env::var("BO2MP_IW6_DISTORTION").is_ok_and(|v| v.trim() == "0"))
}

/// Quads one material's mesh holds.
pub const MAX_QUADS: usize = 32;
/// The most distortion materials at once.
pub const MAX_RINGS: usize = 8;
const STRIDE: usize = 44;

/// One distortion sprite.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Quad {
    pub center: [f32; 3],
    /// Half-extent vectors of the two sides.
    pub right: [f32; 3],
    pub up: [f32; 3],
    /// Native (BGRA) colour.
    pub color: u32,
}

fn norm(v: [f32; 3]) -> [f32; 3] {
    let l = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
    if l > 1e-9 { [v[0] / l, v[1] / l, v[2] / l] } else { [0.0, 0.0, 1.0] }
}

fn cross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}

/// The quad's four world vertices (44-byte records) in the order bottom
/// left, bottom right, top right, top left (uv 0,1 / 1,1 / 1,0 / 0,0).
pub fn quad_vertices(q: &Quad) -> [[u8; STRIDE]; 4] {
    let normal = norm(cross(q.right, q.up));
    let tangent = norm(q.right);
    let corners = [(-1.0f32, -1.0f32, 0.0f32, 1.0f32), (1.0, -1.0, 1.0, 1.0), (1.0, 1.0, 1.0, 0.0), (-1.0, 1.0, 0.0, 0.0)];
    corners.map(|(sx, sy, u, v)| {
        let mut row = [0u8; STRIDE];
        for k in 0..3 {
            let p = q.center[k] + sx * q.right[k] + sy * q.up[k];
            row[4 * k..4 * k + 4].copy_from_slice(&p.to_le_bytes());
        }
        row[12..16].copy_from_slice(&1.0f32.to_le_bytes());
        row[16..20].copy_from_slice(&q.color.to_le_bytes());
        row[20..24].copy_from_slice(&u.to_le_bytes());
        row[24..28].copy_from_slice(&v.to_le_bytes());
        row[36..40].copy_from_slice(&crate::decal::pack_unit_vec(normal).to_le_bytes());
        row[40..44].copy_from_slice(&crate::decal::pack_unit_vec(tangent).to_le_bytes());
        row
    })
}

/// A quad's six indices from vertex `base` (clockwise, the world's front face).
pub fn quad_indices(base: u16) -> [u16; 6] {
    [base, base + 2, base + 1, base, base + 3, base + 2]
}

struct Ring {
    material: usize,
    mesh: usize,
    vertices: Vec<u8>,
    indices: Vec<u16>,
    /// Quads written last frame (the indices past this frame's are cleared).
    last: usize,
}

/// The distortion state of a native Ghosts frame.
#[derive(Default)]
pub struct Distort {
    rings: Vec<Ring>,
    unknown: Vec<String>,
    /// A ring was made: the lists are to be made again.
    pub dirty: bool,
    /// Quads in view this frame (the scene is copied for them).
    pub visible: usize,
    /// The scene copy the technique samples (`RESOLVED_POST_SUN`) and its size.
    copy: Option<((u32, u32), wgpu::Texture)>,
}

impl Distort {
    pub fn ring_count(&self) -> usize {
        self.rings.len()
    }

    /// The scene copy's texture, made (or made again at another size) for a
    /// target of `size` and `format`; true when it was made now (the lists
    /// holding the old one are to be dropped).
    pub fn ensure_copy(&mut self, r: &mut Renderer, size: (u32, u32), format: wgpu::TextureFormat) -> bool {
        if self.copy.as_ref().is_some_and(|c| c.0 == size) {
            return false;
        }
        let texture = r.gpu.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("iw6_resolved_post_sun"),
            size: wgpu::Extent3d { width: size.0, height: size.1, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::COPY_DST | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let view = texture.create_view(&Default::default());
        r.code_textures.insert(
            "RESOLVED_POST_SUN".to_string(),
            render_h1::gpu::Tex { texture: texture.clone(), view, dim: wgpu::TextureViewDimension::D2 },
        );
        self.copy = Some((size, texture));
        true
    }

    pub fn copy_texture(&self) -> Option<&wgpu::Texture> {
        self.copy.as_ref().map(|c| &c.1)
    }

    /// The frame's quads (`(material name, quad)`): written into each
    /// material's mesh; `visible` is how many were taken. A material no zone
    /// holds is left out for good.
    pub fn set(&mut self, r: &mut Renderer, quads: &[(&str, Quad)]) {
        self.visible = 0;
        if !on() {
            return;
        }
        for ring in &mut self.rings {
            ring.vertices.fill(0);
        }
        let mut counts = [0usize; MAX_RINGS];
        for (name, q) in quads {
            if self.unknown.iter().any(|n| n.eq_ignore_ascii_case(name)) {
                continue;
            }
            let Some(material) = r.scene.material_named(name) else {
                self.unknown.push((*name).to_string());
                continue;
            };
            let at = match self.rings.iter().position(|g| g.material == material) {
                Some(i) => i,
                None => {
                    if self.rings.len() >= MAX_RINGS {
                        continue;
                    }
                    let mesh = r.add_dynamic_draw((0, 0, false, None), material, MAX_QUADS * 4, MAX_QUADS * 6);
                    self.rings.push(Ring { material, mesh, vertices: vec![0; MAX_QUADS * 4 * STRIDE], indices: vec![0; MAX_QUADS * 6], last: 0 });
                    self.dirty = true;
                    self.rings.len() - 1
                }
            };
            let ring = &mut self.rings[at];
            if counts[at] >= MAX_QUADS {
                continue;
            }
            let n = counts[at];
            for (k, row) in quad_vertices(q).iter().enumerate() {
                let v = (n * 4 + k) * STRIDE;
                ring.vertices[v..v + STRIDE].copy_from_slice(row);
            }
            ring.indices[n * 6..n * 6 + 6].copy_from_slice(&quad_indices((n * 4) as u16));
            counts[at] += 1;
            self.visible += 1;
        }
        for (ring, &n) in self.rings.iter_mut().zip(&counts) {
            if n == 0 && ring.last == 0 {
                continue;
            }
            // (Quads gone since last frame: their indices cleared.)
            ring.indices[n * 6..ring.last.max(n) * 6].fill(0);
            r.write_dynamic(ring.mesh, 0, &ring.vertices[..n.max(1) * 4 * STRIDE], 0, &ring.indices[..ring.last.max(n) * 6]);
            ring.last = n;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_quad_is_four_corners_facing_the_eye() {
        let q = Quad { center: [10.0, 0.0, 5.0], right: [0.0, 2.0, 0.0], up: [0.0, 0.0, 3.0], color: 0x80ff_ffff };
        let v = quad_vertices(&q);
        let pos = |r: &[u8; STRIDE]| [0, 4, 8].map(|o| f32::from_le_bytes([r[o], r[o + 1], r[o + 2], r[o + 3]]));
        assert_eq!(pos(&v[0]), [10.0, -2.0, 2.0]);
        assert_eq!(pos(&v[2]), [10.0, 2.0, 8.0]);
        // uv: bottom left is (0, 1), top right (1, 0).
        assert_eq!(f32::from_le_bytes(v[0][24..28].try_into().unwrap()), 1.0);
        assert_eq!(f32::from_le_bytes(v[2][20..24].try_into().unwrap()), 1.0);
        // The colour rides in the vertex.
        assert_eq!(u32::from_le_bytes(v[1][16..20].try_into().unwrap()), 0x80ff_ffff);
        // The normal is right x up (toward a viewer who sees `right` to the right and `up` up): +x here.
        let n = fastfile_iw6::map::unpack_unit_vec(u32::from_le_bytes(v[0][36..40].try_into().unwrap()));
        assert!(n[0] > 0.99, "{n:?}");
    }

    #[test]
    fn indices_wind_clockwise_like_the_world() {
        // (v2 - v0) x (v1 - v0) must be the face's normal (right x up) for corners 0, 2, 1.
        let q = Quad { center: [0.0; 3], right: [0.0, 1.0, 0.0], up: [0.0, 0.0, 1.0], color: 0 };
        let v = quad_vertices(&q);
        let p = |i: usize| [0, 4, 8].map(|o| f32::from_le_bytes([v[i][o], v[i][o + 1], v[i][o + 2], v[i][o + 3]]));
        let idx = quad_indices(0);
        let (a, b, c) = (p(idx[0] as usize), p(idx[1] as usize), p(idx[2] as usize));
        let d1 = [c[0] - a[0], c[1] - a[1], c[2] - a[2]];
        let d2 = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
        let n = cross(d1, d2);
        assert!(n[0] > 0.0, "{n:?}");
        assert_eq!(quad_indices(4), [4, 6, 5, 4, 7, 6]);
    }
}
