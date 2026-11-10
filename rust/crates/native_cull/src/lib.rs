//! What cannot be seen is not drawn.
//!
//! Two tests the native renderers share, both conservative (a draw is only
//! dropped when it certainly cannot show):
//!
//! * `Occlusion`: a small software depth buffer. The big opaque triangles
//!   of the map (walls, floors, roofs) are rasterised each frame at low
//!   resolution, keeping for each pixel the *farthest* depth of a triangle
//!   that covers the pixel's centre (so the seam between two triangles of
//!   a wall hides as well as the wall). A draw whose bounds start behind
//!   that depth over every pixel they cover, and the ring of pixels around
//!   them (which keeps a sliver of an occluder's edge pixel from deciding),
//!   is behind the walls and is skipped.
//! * `too_small`: a draw whose bounds subtend less than a fraction of a
//!   pixel is skipped.
//!
//! Views are eye-relative clip matrices with reversed infinite depth, as
//! the renderers keep them (`w` is the distance along the view direction).

use glam::{Mat4, Vec3};

/// One world-space triangle.
pub type Tri = [Vec3; 3];

/// A triangle that may hide things, and which way it faces.
#[derive(Clone, Copy, Debug)]
pub struct Occluder {
    pub tri: Tri,
    /// 0: seen from both sides. +1 / -1: only hides from the side where
    /// its screen-space winding (y down, `edge` positive) is positive /
    /// negative; a surface the game does not draw from behind must not
    /// hide what is behind it from there.
    pub front: i8,
}

/// Occluders grouped into spatial clusters (triangles near one another in
/// the map, in runs of `CLUSTER`, each with its box), so a view skips whole
/// clusters outside its frustum before touching a triangle. A map has
/// tens of thousands of occluding triangles and a view sees a third of them.
#[derive(Clone, Debug, Default)]
pub struct OccluderSet {
    tris: Vec<Occluder>,
    /// Per cluster: its triangles `first .. first + count` and its box.
    clusters: Vec<(u32, u32, Vec3, Vec3)>,
}

/// Triangles to a cluster.
pub const CLUSTER: usize = 24;

impl OccluderSet {
    pub fn new(mut tris: Vec<Occluder>) -> Self {
        if tris.is_empty() {
            return Self::default();
        }
        let (mut lo, mut hi) = (Vec3::splat(f32::MAX), Vec3::splat(f32::MIN));
        for t in &tris {
            for p in &t.tri {
                lo = lo.min(*p);
                hi = hi.max(*p);
            }
        }
        let size = (hi - lo).max(Vec3::splat(1e-3));
        let spread = |mut v: u32| {
            v &= 0x3ff;
            v = (v | (v << 16)) & 0x0300_00ff;
            v = (v | (v << 8)) & 0x0300_f00f;
            v = (v | (v << 4)) & 0x030c_30c3;
            (v | (v << 2)) & 0x0924_9249
        };
        let morton = |t: &Occluder| {
            let c = (t.tri[0] + t.tri[1] + t.tri[2]) / 3.0;
            let q = ((c - lo) / size * 1023.0).clamp(Vec3::ZERO, Vec3::splat(1023.0));
            spread(q.x as u32) | spread(q.y as u32) << 1 | spread(q.z as u32) << 2
        };
        tris.sort_by_cached_key(morton);
        let clusters = tris
            .chunks(CLUSTER)
            .scan(0u32, |first, chunk| {
                let (mut l, mut h) = (Vec3::splat(f32::MAX), Vec3::splat(f32::MIN));
                for t in chunk {
                    for p in &t.tri {
                        l = l.min(*p);
                        h = h.max(*p);
                    }
                }
                let c = (*first, chunk.len() as u32, l, h);
                *first += chunk.len() as u32;
                Some(c)
            })
            .collect();
        Self { tris, clusters }
    }

    pub fn len(&self) -> usize {
        self.tris.len()
    }

    pub fn is_empty(&self) -> bool {
        self.tris.is_empty()
    }
}

/// The occlusion buffer of one view.
#[derive(Clone)]
pub struct Occlusion {
    w: usize,
    h: usize,
    /// Per pixel: the farthest view depth hidden behind it (infinity: no
    /// occluder covers the pixel).
    depth: Vec<f32>,
    view_projection: Mat4,
    eye: Vec3,
    /// Margin added to a draw's nearest depth before it counts as hidden.
    margin: f32,
    /// Triangles rasterised and pixels written (for the stats line).
    pub tris: usize,
    pub pixels: usize,
}

const NEAR: f32 = 8.0;

impl Occlusion {
    pub fn new(
        view_projection: Mat4,
        eye: Vec3,
        occluders: &[Occluder],
        w: usize,
        h: usize,
        margin: f32,
    ) -> Self {
        let mut o = Self {
            w,
            h,
            depth: vec![f32::INFINITY; w * h],
            view_projection,
            eye,
            margin,
            tris: 0,
            pixels: 0,
        };
        for t in occluders {
            o.raster(t);
        }
        o
    }

    /// As `new`, from clustered occluders: clusters wholly outside the view
    /// are skipped without looking at their triangles.
    pub fn from_set(view_projection: Mat4, eye: Vec3, set: &OccluderSet, w: usize, h: usize, margin: f32) -> Self {
        let mut o = Self {
            w,
            h,
            depth: Vec::new(),
            view_projection,
            eye,
            margin,
            tris: 0,
            pixels: 0,
        };
        o.rebuild_from_set(view_projection, eye, set, w, h, margin);
        o
    }

    /// `from_set` into this buffer (its allocation reused when the size is
    /// the same: a moving view builds one every frame).
    pub fn rebuild_from_set(&mut self, view_projection: Mat4, eye: Vec3, set: &OccluderSet, w: usize, h: usize, margin: f32) {
        self.w = w;
        self.h = h;
        self.depth.clear();
        self.depth.resize(w * h, f32::INFINITY);
        self.view_projection = view_projection;
        self.eye = eye;
        self.margin = margin;
        self.tris = 0;
        self.pixels = 0;
        for &(first, count, lo, hi) in &set.clusters {
            if self.box_outside(lo, hi) {
                continue;
            }
            for t in &set.tris[first as usize..(first + count) as usize] {
                self.raster(t);
            }
        }
    }

    /// `rebuild_from_set` with the clusters split over `threads` threads,
    /// each filling its own buffer, the buffers then merged by their
    /// smallest value per pixel: the very buffer the one thread makes (a
    /// pixel holds the least of the depths written to it, whatever the
    /// order).
    #[allow(clippy::too_many_arguments)]
    pub fn rebuild_from_set_threads(&mut self, view_projection: Mat4, eye: Vec3, set: &OccluderSet, w: usize, h: usize, margin: f32, threads: usize) {
        if threads <= 1 {
            return self.rebuild_from_set(view_projection, eye, set, w, h, margin);
        }
        self.w = w;
        self.h = h;
        self.depth.clear();
        self.depth.resize(w * h, f32::INFINITY);
        self.view_projection = view_projection;
        self.eye = eye;
        self.margin = margin;
        self.tris = 0;
        self.pixels = 0;
        let visible: Vec<(u32, u32)> = set.clusters.iter().filter(|c| !self.box_outside(c.2, c.3)).map(|c| (c.0, c.1)).collect();
        if visible.is_empty() {
            return;
        }
        let per = visible.len().div_ceil(threads);
        let (own, rest) = {
            let mut chunks = visible.chunks(per);
            (chunks.next().unwrap_or(&[]), chunks.collect::<Vec<_>>())
        };
        let params = (view_projection, eye, margin);
        let parts: Vec<(Vec<f32>, usize, usize)> = std::thread::scope(|sc| {
            let handles: Vec<_> = rest
                .iter()
                .map(|chunk| {
                    sc.spawn(move || {
                        let mut o = Self { w, h, depth: vec![f32::INFINITY; w * h], view_projection: params.0, eye: params.1, margin: params.2, tris: 0, pixels: 0 };
                        for &(first, count) in *chunk {
                            for t in &set.tris[first as usize..(first + count) as usize] {
                                o.raster(t);
                            }
                        }
                        (o.depth, o.tris, o.pixels)
                    })
                })
                .collect();
            for &(first, count) in own {
                for t in &set.tris[first as usize..(first + count) as usize] {
                    self.raster(t);
                }
            }
            handles.into_iter().map(|h| h.join().expect("occlusion thread")).collect()
        });
        for (depth, tris, pixels) in parts {
            for (d, o) in self.depth.iter_mut().zip(&depth) {
                if *o < *d {
                    *d = *o;
                }
            }
            self.tris += tris;
            self.pixels += pixels;
        }
    }

    /// True when the box lies wholly outside one of the view's planes (the
    /// near distance and the four sides), tested in clip space.
    fn box_outside(&self, lo: Vec3, hi: Vec3) -> bool {
        let mut out = [true; 5];
        for c in 0..8 {
            let p = Vec3::new(
                if c & 1 == 0 { lo.x } else { hi.x },
                if c & 2 == 0 { lo.y } else { hi.y },
                if c & 4 == 0 { lo.z } else { hi.z },
            );
            let q = self.view_projection * (p - self.eye).extend(1.0);
            out[0] &= q.w < NEAR;
            out[1] &= q.x < -q.w;
            out[2] &= q.x > q.w;
            out[3] &= q.y < -q.w;
            out[4] &= q.y > q.w;
        }
        out.iter().any(|&o| o)
    }

    /// Clip-space position (eye-relative) of a world point: x, y and w
    /// (the view depth).
    fn clip(&self, p: Vec3) -> Vec3 {
        let c = self.view_projection * (p - self.eye).extend(1.0);
        Vec3::new(c.x, c.y, c.w)
    }

    /// Clip-space position to the screen: pixels, and the view depth.
    fn screen(&self, c: Vec3) -> (f32, f32, f32) {
        (
            (c.x / c.z * 0.5 + 0.5) * self.w as f32,
            (0.5 - c.y / c.z * 0.5) * self.h as f32,
            c.z,
        )
    }

    /// The point on screen with its view depth, or None behind the near
    /// distance.
    fn project(&self, p: Vec3) -> Option<(f32, f32, f32)> {
        let c = self.clip(p);
        (c.z >= NEAR).then(|| self.screen(c))
    }

    /// Writes a triangle's farthest depth into every pixel whose centre it
    /// covers. The triangle is first clipped to the near distance and the
    /// screen's sides (so no coordinate runs away) and the polygon left is
    /// filled as a fan; its farthest vertex is the depth for all of it.
    fn raster(&mut self, o: &Occluder) {
        self.tris += 1;
        let c3: [Vec3; 3] = std::array::from_fn(|k| self.clip(o.tri[k]));
        if o.front != 0 {
            // Orientation of the whole triangle (homogeneous determinant,
            // valid even partly behind the eye): screen winding positive
            // when it is negative.
            let (a, b, c) = (c3[0], c3[1], c3[2]);
            let det = a.x * (b.y * c.z - b.z * c.y) - a.y * (b.x * c.z - b.z * c.x)
                + a.z * (b.x * c.y - b.y * c.x);
            if (det < 0.0) != (o.front > 0) {
                return;
            }
        }
        // Planes as (a, b, c) with a x + b y + c w >= 0; the near plane is
        // w >= NEAR. A triangle wholly outside one is dropped before any
        // clipping (most of the map is off screen).
        const PLANES: [[f32; 3]; 5] = [
            [0.0, 0.0, 1.0],
            [1.0, 0.0, 1.0],
            [-1.0, 0.0, 1.0],
            [0.0, 1.0, 1.0],
            [0.0, -1.0, 1.0],
        ];
        let dist = |p: &[f32; 3], v: Vec3, k: usize| {
            p[0] * v.x + p[1] * v.y + p[2] * v.z - if k == 0 { NEAR } else { 0.0 }
        };
        for (k, p) in PLANES.iter().enumerate() {
            if c3.iter().all(|&v| dist(p, v, k) < 0.0) {
                return;
            }
        }
        // The polygon clipped by each plane: at most 3 + 5 corners.
        let mut poly = [Vec3::ZERO; 8];
        let mut n = 3;
        poly[..3].copy_from_slice(&c3);
        for (k, p) in PLANES.iter().enumerate() {
            let mut out = [Vec3::ZERO; 8];
            let mut m = 0;
            for e in 0..n {
                let (a, b) = (poly[e], poly[(e + 1) % n]);
                let (da, db) = (dist(p, a, k), dist(p, b, k));
                if da >= 0.0 {
                    out[m] = a;
                    m += 1;
                }
                if (da >= 0.0) != (db >= 0.0) {
                    out[m] = a + (b - a) * (da / (da - db));
                    m += 1;
                }
            }
            if m < 3 {
                return;
            }
            poly = out;
            n = m;
        }
        let poly = &poly[..n];
        let mut pts = [(0.0f32, 0.0f32, 0.0f32); 8];
        for (k, &c) in poly.iter().enumerate() {
            pts[k] = self.screen(c);
        }
        let pts = &pts[..poly.len()];
        let far = pts.iter().fold(0.0f32, |m, p| m.max(p.2));
        for k in 1..pts.len() - 1 {
            self.fill(pts[0], pts[k], pts[k + 1], far);
        }
    }

    /// Fills the pixels whose centres the triangle covers, row by row (each
    /// row's span from the triangle's edges, half-open at the top so two
    /// triangles sharing an edge cover it once), with `far`.
    fn fill(&mut self, a: (f32, f32, f32), b: (f32, f32, f32), c: (f32, f32, f32), far: f32) {
        let min_y = a.1.min(b.1).min(c.1);
        let max_y = a.1.max(b.1).max(c.1);
        // The first row whose centre is at or below the top, the last above
        // the bottom.
        let y0 = (min_y - 0.5).ceil().max(0.0) as usize;
        let y1 = ((max_y - 0.5).ceil().max(0.0) as usize).min(self.h);
        if y1 <= y0 {
            return;
        }
        let edges = [(a, b), (b, c), (c, a)];
        for y in y0..y1 {
            let yc = y as f32 + 0.5;
            let (mut xl, mut xr) = (f32::MAX, f32::MIN);
            for &(p, q) in &edges {
                if (p.1 <= yc) != (q.1 <= yc) {
                    let x = p.0 + (yc - p.1) / (q.1 - p.1) * (q.0 - p.0);
                    xl = xl.min(x);
                    xr = xr.max(x);
                }
            }
            if xl > xr {
                continue;
            }
            let x0 = (xl - 0.5).ceil().max(0.0) as usize;
            let x1 = ((xr - 0.5).floor() + 1.0).min(self.w as f32).max(0.0) as usize;
            if x1 <= x0 {
                continue;
            }
            self.pixels += x1 - x0;
            for d in &mut self.depth[y * self.w + x0..y * self.w + x1] {
                if far < *d {
                    *d = far;
                }
            }
        }
    }

    /// True when the box is certainly hidden behind the occluders.
    pub fn box_hidden(&self, lo: Vec3, hi: Vec3) -> bool {
        // The eye inside the box, or any corner behind the near distance:
        // not tested.
        let (mut x0, mut y0) = (f32::MAX, f32::MAX);
        let (mut x1, mut y1) = (f32::MIN, f32::MIN);
        let mut near = f32::MAX;
        for c in 0..8 {
            let p = Vec3::new(
                if c & 1 == 0 { lo.x } else { hi.x },
                if c & 2 == 0 { lo.y } else { hi.y },
                if c & 4 == 0 { lo.z } else { hi.z },
            );
            let Some((x, y, d)) = self.project(p) else {
                return false;
            };
            x0 = x0.min(x);
            x1 = x1.max(x);
            y0 = y0.min(y);
            y1 = y1.max(y);
            near = near.min(d);
        }
        // Off screen: the frustum test's business.
        if x1 < 0.0 || y1 < 0.0 || x0 > self.w as f32 || y0 > self.h as f32 {
            return false;
        }
        let bx0 = (x0.floor() - 1.0).max(0.0) as usize;
        let by0 = (y0.floor() - 1.0).max(0.0) as usize;
        let bx1 = ((x1.ceil() + 1.0).min(self.w as f32)) as usize;
        let by1 = ((y1.ceil() + 1.0).min(self.h as f32)) as usize;
        if bx1 <= bx0 || by1 <= by0 {
            return false;
        }
        // Parts of the box off screen cannot be judged by this buffer, but
        // they are not drawn either; only the covered pixels count.
        let limit = near - self.margin;
        for y in by0..by1 {
            for x in bx0..bx1 {
                if self.depth[y * self.w + x] > limit {
                    return false;
                }
            }
        }
        true
    }

    /// True when it was built for this very view.
    pub fn is_for(&self, view_projection: &Mat4, eye: Vec3) -> bool {
        self.view_projection == *view_projection && self.eye == eye
    }

    /// Pixels an occluder covers (for the stats line).
    pub fn covered(&self) -> usize {
        self.depth.iter().filter(|d| d.is_finite()).count()
    }
}

/// True when the box, seen from `eye`, spans less than `min_angle`
/// (radians, about 1/540 per pixel at 1080p) across its diagonal. The eye
/// inside or touching the box never counts as small.
pub fn too_small(eye: Vec3, lo: Vec3, hi: Vec3, min_angle: f32) -> bool {
    let d = (lo - eye).max(eye - hi).max(Vec3::ZERO).length();
    if d <= 0.0 {
        return false;
    }
    (hi - lo).length() < d * min_angle
}

/// The largest triangles of a set, up to `cap`, by area: the occluders.
pub fn largest(mut tris: Vec<Occluder>, min_area: f32, cap: usize) -> Vec<Occluder> {
    let area = |o: &Occluder| {
        let t = &o.tri;
        (t[1] - t[0]).cross(t[2] - t[0]).length() * 0.5
    };
    tris.retain(|t| area(t) >= min_area);
    tris.sort_by(|a, b| area(b).total_cmp(&area(a)));
    tris.truncate(cap);
    tris
}

#[cfg(test)]
mod tests {
    use super::*;

    fn view() -> (Mat4, Vec3) {
        // Looking down +x from the origin: x forward, y left... any
        // right-handed look-at will do.
        let eye = Vec3::ZERO;
        let v = Mat4::look_at_rh(Vec3::ZERO, Vec3::X, Vec3::Z);
        let p = Mat4::perspective_infinite_reverse_rh(1.5, 16.0 / 9.0, 4.0);
        (p * v, eye)
    }

    fn wall(x: f32, half: f32) -> Vec<Occluder> {
        wall_facing(x, half, 0)
    }

    fn wall_facing(x: f32, half: f32, front: i8) -> Vec<Occluder> {
        let c = |y: f32, z: f32| Vec3::new(x, y, z);
        vec![
            Occluder { tri: [c(-half, -half), c(half, -half), c(half, half)], front },
            Occluder { tri: [c(-half, -half), c(half, half), c(-half, half)], front },
        ]
    }

    #[test]
    fn a_clustered_set_hides_what_the_plain_list_hides() {
        let (vp, eye) = view();
        let mut tris = wall(100.0, 400.0);
        // Triangles far off to the side (their clusters are skipped).
        tris.extend(wall_facing(-500.0, 50.0, 0));
        let a = Occlusion::new(vp, eye, &tris, 128, 72, 4.0);
        let b = Occlusion::from_set(vp, eye, &OccluderSet::new(tris), 128, 72, 4.0);
        assert_eq!(a.covered(), b.covered());
        let (lo, hi) = (Vec3::new(300.0, -20.0, -20.0), Vec3::new(320.0, 20.0, 20.0));
        assert!(b.box_hidden(lo, hi));
    }

    #[test]
    fn threads_make_the_same_buffer_as_one() {
        let (vp, eye) = view();
        let mut tris = Vec::new();
        for i in 0..40 {
            tris.extend(wall(20.0 + i as f32 * 7.0, 3.0 + (i % 5) as f32 * 4.0));
        }
        let set = OccluderSet::new(tris);
        let mut one = Occlusion::from_set(vp, eye, &set, 160, 90, 1.0);
        let mut many = one.clone();
        many.rebuild_from_set_threads(vp, eye, &set, 160, 90, 1.0, 3);
        one.rebuild_from_set(vp, eye, &set, 160, 90, 1.0);
        assert_eq!(one.depth, many.depth);
        assert_eq!((one.tris, one.pixels), (many.tris, many.pixels));
        assert!(one.covered() > 0);
    }

    #[test]
    fn a_box_behind_a_wall_is_hidden() {
        let (vp, eye) = view();
        let o = Occlusion::new(vp, eye, &wall(100.0, 400.0), 128, 72, 4.0);
        assert!(o.covered() > 1000);
        assert!(o.box_hidden(Vec3::new(300.0, -20.0, -20.0), Vec3::new(320.0, 20.0, 20.0)));
    }

    #[test]
    fn a_box_in_front_of_a_wall_shows() {
        let (vp, eye) = view();
        let o = Occlusion::new(vp, eye, &wall(100.0, 400.0), 128, 72, 4.0);
        assert!(!o.box_hidden(Vec3::new(40.0, -20.0, -20.0), Vec3::new(60.0, 20.0, 20.0)));
    }

    #[test]
    fn a_box_poking_past_the_wall_shows() {
        let (vp, eye) = view();
        // The wall covers only a small patch; the box is far wider.
        let o = Occlusion::new(vp, eye, &wall(100.0, 30.0), 128, 72, 4.0);
        assert!(!o.box_hidden(Vec3::new(300.0, -400.0, -20.0), Vec3::new(320.0, 400.0, 20.0)));
    }

    #[test]
    fn a_box_beside_the_wall_shows() {
        let (vp, eye) = view();
        let o = Occlusion::new(vp, eye, &wall(100.0, 30.0), 128, 72, 4.0);
        assert!(!o.box_hidden(Vec3::new(300.0, 200.0, -20.0), Vec3::new(320.0, 240.0, 20.0)));
    }

    #[test]
    fn a_one_sided_wall_hides_only_from_its_front() {
        let (vp, eye) = view();
        let b = (Vec3::new(300.0, -20.0, -20.0), Vec3::new(320.0, 20.0, 20.0));
        let a = Occlusion::new(vp, eye, &wall_facing(100.0, 400.0, 1), 128, 72, 4.0);
        let c = Occlusion::new(vp, eye, &wall_facing(100.0, 400.0, -1), 128, 72, 4.0);
        // Exactly one of the two windings faces the eye.
        assert_ne!(a.box_hidden(b.0, b.1), c.box_hidden(b.0, b.1));
    }

    #[test]
    fn a_huge_floor_reaching_behind_the_eye_does_not_break_the_clip() {
        let (vp, eye) = view();
        let f = |x: f32, y: f32| Vec3::new(x, y, -50.0);
        let tris = vec![
            Occluder { tri: [f(-1e6, -1e6), f(1e6, -1e6), f(1e6, 1e6)], front: 0 },
            Occluder { tri: [f(-1e6, -1e6), f(1e6, 1e6), f(-1e6, 1e6)], front: 0 },
        ];
        let o = Occlusion::new(vp, eye, &tris, 128, 72, 4.0);
        assert!(o.covered() > 0);
        // A box above the floor, in view, is never hidden by it.
        assert!(!o.box_hidden(Vec3::new(500.0, -10.0, 60.0), Vec3::new(520.0, 10.0, 90.0)));
    }

    #[test]
    fn no_occluders_hide_nothing() {
        let (vp, eye) = view();
        let o = Occlusion::new(vp, eye, &[], 128, 72, 4.0);
        assert!(!o.box_hidden(Vec3::new(300.0, -20.0, -20.0), Vec3::new(320.0, 20.0, 20.0)));
    }

    #[test]
    fn a_box_around_the_eye_is_never_hidden() {
        let (vp, eye) = view();
        let o = Occlusion::new(vp, eye, &wall(100.0, 400.0), 128, 72, 4.0);
        assert!(!o.box_hidden(Vec3::splat(-10.0), Vec3::splat(10.0)));
    }

    #[test]
    fn small_far_boxes_are_dropped() {
        let eye = Vec3::ZERO;
        let (lo, hi) = (Vec3::new(1000.0, 0.0, 0.0), Vec3::new(1001.0, 1.0, 1.0));
        assert!(too_small(eye, lo, hi, 0.002));
        assert!(!too_small(eye, lo, Vec3::new(1010.0, 10.0, 10.0), 0.002));
        assert!(!too_small(Vec3::new(1000.5, 0.5, 0.5), lo, hi, 0.002));
    }

    #[test]
    fn the_largest_triangles_are_kept() {
        let o = |tri| Occluder { tri, front: 0 };
        let big = o([Vec3::ZERO, Vec3::new(100.0, 0.0, 0.0), Vec3::new(0.0, 100.0, 0.0)]);
        let small = o([Vec3::ZERO, Vec3::X, Vec3::Y]);
        let out = largest(vec![small, big, small], 100.0, 4);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].tri, big.tri);
    }
}
