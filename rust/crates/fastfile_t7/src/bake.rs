//! bo2mp: light baked at load time from a map's own geometry, for games
//! whose own bake is not read (Black Ops III's sun shadow tree and probe
//! volume): a bounding volume tree over the map's opaque triangles, and per
//! point how much of the sun it sees (`sun_visibility`) and how open it is
//! to the sky and its surroundings (`openness`, a few short rays over the
//! hemisphere round its normal). Game-neutral: triangles in, numbers out.

/// A triangle as its first corner and two edges.
#[derive(Clone, Copy, Debug)]
struct Tri {
    p: [f32; 3],
    e1: [f32; 3],
    e2: [f32; 3],
    /// The centroid (kept: the build's median splits read it many times).
    c: [f32; 3],
}

#[derive(Clone, Copy, Debug)]
struct Node {
    lo: [f32; 3],
    hi: [f32; 3],
    /// Leaf: first triangle; inner: the right child (the left is next).
    at: u32,
    /// Leaf: triangle count; inner: 0.
    count: u32,
    /// Inner: the axis the children were split on (the left holds the
    /// lower centres), so a ray visits the near child first.
    axis: u32,
}

/// The tree.
pub struct Tracer {
    tris: Vec<Tri>,
    nodes: Vec<Node>,
}

fn sub(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn cross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}
fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

const LEAF: usize = 4;

impl Tracer {
    /// A tree over the triangles (corners), degenerate ones left out. The
    /// triangles are read and the tree's upper levels built on every core.
    pub fn new(corners: &[[[f32; 3]; 3]]) -> Self {
        let keep = |t: &[[f32; 3]; 3]| -> Option<Tri> {
            if !t.iter().all(|p| p.iter().all(|v| v.is_finite())) {
                return None;
            }
            let tri = Tri { p: t[0], e1: sub(t[1], t[0]), e2: sub(t[2], t[0]), c: std::array::from_fn(|k| (t[0][k] + t[1][k] + t[2][k]) / 3.0) };
            let n = cross(tri.e1, tri.e2);
            (dot(n, n) > 1e-8).then_some(tri)
        };
        let parts = parallel_chunks(corners.len(), 1 << 16, |r| corners[r].iter().filter_map(keep).collect::<Vec<Tri>>());
        let mut tris: Vec<Tri> = Vec::with_capacity(parts.iter().map(Vec::len).sum());
        for p in parts {
            tris.extend(p);
        }
        let nodes = if tris.is_empty() { Vec::new() } else { build_par(&mut tris, 0) };
        Tracer { tris, nodes }
    }

    pub fn len(&self) -> usize {
        self.tris.len()
    }

    pub fn is_empty(&self) -> bool {
        self.tris.is_empty()
    }

    /// Whether the segment from `o` along `d` (unit) up to `max` hits any
    /// triangle (either side).
    pub fn occluded(&self, o: [f32; 3], d: [f32; 3], max: f32) -> bool {
        if self.nodes.is_empty() {
            return false;
        }
        let inv = d.map(|v| if v.abs() > 1e-12 { 1.0 / v } else { 1e12f32.copysign(v) });
        let mut stack = [0u32; 64];
        let mut sp = 1usize;
        while sp > 0 {
            sp -= 1;
            let node = &self.nodes[stack[sp] as usize];
            // Slab test.
            let (mut t0, mut t1) = (0.0f32, max);
            for k in 0..3 {
                let a = (node.lo[k] - o[k]) * inv[k];
                let b = (node.hi[k] - o[k]) * inv[k];
                t0 = t0.max(a.min(b));
                t1 = t1.min(a.max(b));
            }
            if t0 > t1 {
                continue;
            }
            if node.count > 0 {
                for t in &self.tris[node.at as usize..(node.at + node.count) as usize] {
                    let pv = cross(d, t.e2);
                    let det = dot(t.e1, pv);
                    if det.abs() < 1e-9 {
                        continue;
                    }
                    let id = 1.0 / det;
                    let tv = sub(o, t.p);
                    let u = dot(tv, pv) * id;
                    if !(0.0..=1.0).contains(&u) {
                        continue;
                    }
                    let qv = cross(tv, t.e1);
                    let v = dot(d, qv) * id;
                    if v < 0.0 || u + v > 1.0 {
                        continue;
                    }
                    let dist = dot(t.e2, qv) * id;
                    if dist > 1e-3 && dist < max {
                        return true;
                    }
                }
            } else if sp + 2 <= stack.len() {
                // The child the ray reaches first is popped first.
                let (left, right) = (stack[sp] + 1, node.at);
                let (near, far) = if d[node.axis as usize] >= 0.0 { (left, right) } else { (right, left) };
                stack[sp] = far;
                stack[sp + 1] = near;
                sp += 2;
            }
        }
        false
    }

    /// The nearest hit along the segment from `o` along `d` (unit) up to
    /// `max`: its distance and the triangle's unit normal turned to face the
    /// ray (either side is hit). bo2mp: the ambient rebake's bounce
    /// (`assets::lane::rt_ambient`).
    pub fn nearest(&self, o: [f32; 3], d: [f32; 3], max: f32) -> Option<(f32, [f32; 3])> {
        if self.nodes.is_empty() {
            return None;
        }
        let inv = d.map(|v| if v.abs() > 1e-12 { 1.0 / v } else { 1e12f32.copysign(v) });
        let mut best = max;
        let mut hit: Option<usize> = None;
        let mut stack = [0u32; 64];
        let mut sp = 1usize;
        while sp > 0 {
            sp -= 1;
            let node = &self.nodes[stack[sp] as usize];
            let (mut t0, mut t1) = (0.0f32, best);
            for k in 0..3 {
                let a = (node.lo[k] - o[k]) * inv[k];
                let b = (node.hi[k] - o[k]) * inv[k];
                t0 = t0.max(a.min(b));
                t1 = t1.min(a.max(b));
            }
            if t0 > t1 {
                continue;
            }
            if node.count > 0 {
                for i in node.at as usize..(node.at + node.count) as usize {
                    let t = &self.tris[i];
                    let pv = cross(d, t.e2);
                    let det = dot(t.e1, pv);
                    if det.abs() < 1e-9 {
                        continue;
                    }
                    let id = 1.0 / det;
                    let tv = sub(o, t.p);
                    let u = dot(tv, pv) * id;
                    if !(0.0..=1.0).contains(&u) {
                        continue;
                    }
                    let qv = cross(tv, t.e1);
                    let v = dot(d, qv) * id;
                    if v < 0.0 || u + v > 1.0 {
                        continue;
                    }
                    let dist = dot(t.e2, qv) * id;
                    if dist > 1e-3 && dist < best {
                        best = dist;
                        hit = Some(i);
                    }
                }
            } else if sp + 2 <= stack.len() {
                // The child the ray reaches first is popped first.
                let (left, right) = (stack[sp] + 1, node.at);
                let (near, far) = if d[node.axis as usize] >= 0.0 { (left, right) } else { (right, left) };
                stack[sp] = far;
                stack[sp + 1] = near;
                sp += 2;
            }
        }
        hit.map(|i| {
            let t = &self.tris[i];
            let n = cross(t.e1, t.e2);
            let l = dot(n, n).sqrt().max(1e-12);
            let n = n.map(|v| v / l);
            (best, if dot(n, d) > 0.0 { n.map(|v| -v) } else { n })
        })
    }

    /// How much of the sun (direction `sun`, unit, toward it) point `p` on a
    /// surface facing `n` sees: 0..1, over a few rays spread over the sun's
    /// width `spread` (radians), offset `bias` units off the surface.
    pub fn sun_visibility(&self, p: [f32; 3], n: [f32; 3], sun: [f32; 3], spread: f32, bias: f32) -> f32 {
        let o: [f32; 3] = std::array::from_fn(|k| p[k] + n[k] * bias + sun[k] * bias);
        let (t, b) = basis(sun);
        const RAYS: [[f32; 2]; 5] = [[0.0, 0.0], [1.0, 0.0], [-1.0, 0.0], [0.0, 1.0], [0.0, -1.0]];
        let mut lit = 0.0;
        for r in RAYS {
            let d: [f32; 3] = std::array::from_fn(|k| sun[k] + (t[k] * r[0] + b[k] * r[1]) * spread);
            let l = dot(d, d).sqrt();
            let d = d.map(|v| v / l);
            if !self.occluded(o, d, 65536.0) {
                lit += 1.0;
            }
        }
        lit / RAYS.len() as f32
    }

    /// How open point `p` (facing `n`) is: the cosine-weighted share of
    /// `rays` hemisphere rays that travel `reach` units unhit (`seed`
    /// varies the pattern per point).
    pub fn openness(&self, p: [f32; 3], n: [f32; 3], reach: f32, rays: u32, seed: u32, bias: f32) -> f32 {
        let o: [f32; 3] = std::array::from_fn(|k| p[k] + n[k] * bias);
        let (t, b) = basis(n);
        let mut open = 0u32;
        let turn = (seed.wrapping_mul(2_654_435_761) >> 8) as f32 / (1u32 << 24) as f32;
        for i in 0..rays {
            // Cosine-weighted, a golden-angle spiral.
            let u = (i as f32 + 0.5) / rays as f32;
            let r = u.sqrt();
            let phi = (i as f32 * 2.399_963 + turn * std::f32::consts::TAU) % std::f32::consts::TAU;
            let (x, y) = (r * phi.cos(), r * phi.sin());
            let z = (1.0 - u).max(0.0).sqrt();
            let d: [f32; 3] = std::array::from_fn(|k| t[k] * x + b[k] * y + n[k] * z);
            if !self.occluded(o, d, reach) {
                open += 1;
            }
        }
        open as f32 / rays.max(1) as f32
    }
}

/// Two unit vectors across `n` (unit).
fn basis(n: [f32; 3]) -> ([f32; 3], [f32; 3]) {
    let a = if n[2].abs() < 0.9 { [0.0, 0.0, 1.0] } else { [1.0, 0.0, 0.0] };
    let t = cross(a, n);
    let l = dot(t, t).sqrt().max(1e-9);
    let t = t.map(|v| v / l);
    (t, cross(n, t))
}

fn bounds(tris: &[Tri]) -> ([f32; 3], [f32; 3]) {
    let mut lo = [f32::MAX; 3];
    let mut hi = [f32::MIN; 3];
    for t in tris {
        for c in [t.p, std::array::from_fn(|k| t.p[k] + t.e1[k]), std::array::from_fn(|k| t.p[k] + t.e2[k])] {
            for k in 0..3 {
                lo[k] = lo[k].min(c[k]);
                hi[k] = hi[k].max(c[k]);
            }
        }
    }
    (lo, hi)
}

fn centre(t: &Tri, k: usize) -> f32 {
    t.c[k]
}

/// Builds the nodes for `tris` (median split on the widest axis of the
/// centres), preorder; a node's right child is `at` (an index into the
/// returned list), its left the next; a leaf's `at` is its first triangle in
/// `tris`.
fn build(tris: &mut [Tri], a: usize, b: usize, nodes: &mut Vec<Node>) -> u32 {
    let (lo, hi) = bounds(&tris[a..b]);
    let me = nodes.len();
    nodes.push(Node { lo, hi, at: a as u32, count: (b - a) as u32, axis: 0 });
    if b - a <= LEAF {
        return me as u32;
    }
    let axis = split_axis(&tris[a..b]);
    let mid = (b - a) / 2;
    tris[a..b].select_nth_unstable_by(mid, |x, y| centre(x, axis).total_cmp(&centre(y, axis)));
    build(tris, a, a + mid, nodes);
    let right = build(tris, a + mid, b, nodes);
    nodes[me].at = right;
    nodes[me].count = 0;
    nodes[me].axis = axis as u32;
    me as u32
}

fn split_axis(tris: &[Tri]) -> usize {
    let mut clo = [f32::MAX; 3];
    let mut chi = [f32::MIN; 3];
    for t in tris {
        for k in 0..3 {
            let c = centre(t, k);
            clo[k] = clo[k].min(c);
            chi[k] = chi[k].max(c);
        }
    }
    (0..3).max_by(|&i, &j| (chi[i] - clo[i]).total_cmp(&(chi[j] - clo[j]))).unwrap_or(0)
}

/// The same tree as [`build`] over all of `tris` (`first`: the index of
/// `tris[0]` in the whole list), the upper levels' two halves on threads of
/// their own. Indices in the result are relative to its first node.
fn build_par(tris: &mut [Tri], first: usize) -> Vec<Node> {
    const SERIAL: usize = 1 << 15;
    let n = tris.len();
    if n <= SERIAL {
        let mut nodes = Vec::with_capacity(2 * n / LEAF + 1);
        build(tris, 0, n, &mut nodes);
        for node in nodes.iter_mut().filter(|nd| nd.count > 0) {
            node.at += first as u32;
        }
        return nodes;
    }
    let (lo, hi) = bounds(tris);
    let axis = split_axis(tris);
    let mid = n / 2;
    tris.select_nth_unstable_by(mid, |x, y| centre(x, axis).total_cmp(&centre(y, axis)));
    let (left, right) = tris.split_at_mut(mid);
    let (l, r) = std::thread::scope(|s| {
        let l = s.spawn(|| build_par(left, first));
        let r = build_par(right, first + mid);
        (l.join().expect("tree build"), r)
    });
    let mut nodes = Vec::with_capacity(1 + l.len() + r.len());
    nodes.push(Node { lo, hi, at: (1 + l.len()) as u32, count: 0, axis: axis as u32 });
    let shift = |mut v: Vec<Node>, by: u32| {
        for node in v.iter_mut().filter(|nd| nd.count == 0) {
            node.at += by;
        }
        v
    };
    let right_at = 1 + l.len() as u32;
    nodes.extend(shift(l, 1));
    nodes.extend(shift(r, right_at));
    nodes
}

/// The bound on a barycentric weight below which a texel is out of a
/// triangle when a row's span is cut (the test itself keeps -1e-4; this is
/// wider, so rounding in the span never drops a texel the test would keep).
const EDGE_SLACK: f32 = -2e-3;

/// A baked sun shadow map: the scene seen from the sun (orthographic),
/// per texel the height toward the sun of the nearest surface. A point is
/// in the sun when its own height is not below the texel's (less a bias).
pub struct SunDepth {
    pub size: u32,
    /// Toward the sun (unit), and two axes across it.
    pub dir: [f32; 3],
    pub u: [f32; 3],
    pub v: [f32; 3],
    /// The covered square in (u, v): its low corner and texel size.
    pub origin: [f32; 2],
    pub texel: f32,
    /// Heights toward the sun (`f32::MIN` where nothing is).
    pub height: Vec<f32>,
}

/// A triangle for the sun map: corners, and whether it casts a partial
/// shadow (foliage cards: every other texel, a dither PCF softens).
#[derive(Clone, Copy, Debug)]
pub struct Caster {
    pub p: [[f32; 3]; 3],
    pub partial: bool,
}

impl SunDepth {
    /// Rasterises the casters into a `size` x `size` map over the square
    /// (in the sun's view) round `centre` of side `span` units.
    pub fn render(casters: &[Caster], dir: [f32; 3], centre: [f32; 3], span: f32, size: u32) -> SunDepth {
        let (u, v) = basis(dir);
        let texel = span / size as f32;
        let origin = [dot(centre, u) - span * 0.5, dot(centre, v) - span * 0.5];
        let n = size as usize;
        // The map is drawn in strips of rows small enough to stay in a core's
        // cache: casters land all over the map, and a strip of their texels
        // written together costs far fewer misses than the whole map at once.
        let strip = (131_072 / n).clamp(8, 128);
        let strips = n.div_ceil(strip);
        type Bin = Vec<([[f32; 3]; 3], bool)>;
        // Each caster in texel space, copied into every strip it reaches (a
        // maximum is the same in any order, so the strips can be drawn in
        // any).
        let binned: Vec<Vec<Bin>> = parallel_chunks(casters.len(), 1 << 16, |r| {
            let mut bins: Vec<Bin> = vec![Vec::new(); strips];
            for c in &casters[r] {
                let t = c.p.map(|p| [(dot(p, u) - origin[0]) / texel, (dot(p, v) - origin[1]) / texel, dot(p, dir)]);
                if !t.iter().all(|p| p.iter().all(|v| v.is_finite())) {
                    continue;
                }
                let lo_y = t[0][1].min(t[1][1]).min(t[2][1]).floor().max(0.0) as i64;
                let hi_y = t[0][1].max(t[1][1]).max(t[2][1]).ceil().min(n as f32 - 1.0) as i64;
                if lo_y > hi_y {
                    continue;
                }
                for bin in &mut bins[lo_y as usize / strip..=hi_y as usize / strip] {
                    bin.push((t, c.partial));
                }
            }
            bins
        });
        let drawn: Vec<Vec<f32>> = parallel_chunks(strips, 1, |r| {
            let b = r.start;
            let y0 = b * strip;
            let y1 = (y0 + strip).min(n);
            let mut rows = vec![f32::MIN; (y1 - y0) * n];
            for part in &binned {
                for (t, partial) in &part[b] {
                    let lo_y = t[0][1].min(t[1][1]).min(t[2][1]).floor().max(y0 as f32) as i64;
                    let hi_y = t[0][1].max(t[1][1]).max(t[2][1]).ceil().min(y1 as f32 - 1.0) as i64;
                    if lo_y > hi_y {
                        continue;
                    }
                    let lo_x = t[0][0].min(t[1][0]).min(t[2][0]).floor().max(0.0) as i64;
                    let hi_x = t[0][0].max(t[1][0]).max(t[2][0]).ceil().min(n as f32 - 1.0) as i64;
                    if lo_x > hi_x {
                        continue;
                    }
                    let area = (t[1][0] - t[0][0]) * (t[2][1] - t[0][1]) - (t[2][0] - t[0][0]) * (t[1][1] - t[0][1]);
                    if area.abs() < 1e-9 {
                        continue;
                    }
                    let inv = 1.0 / area;
                    // A triangle over many texels is walked row by row over the
                    // texels its edges leave in (the three barycentric weights
                    // are linear in x: each bounds the row on one side); the
                    // test below then decides, as for a small one, so the
                    // result is the box's, without testing the box.
                    let wide = (hi_x - lo_x + 1) * (hi_y - lo_y + 1) > 24;
                    let (s0, s1) = (t[1][1] - t[2][1], t[2][1] - t[0][1]);
                    for y in lo_y..=hi_y {
                        let (mut xs, mut xe) = (lo_x, hi_x);
                        if wide {
                            let py = y as f32 + 0.5;
                            let e0 = t[1][0] * (t[2][1] - py) - t[2][0] * (t[1][1] - py);
                            let e1 = t[2][0] * (t[0][1] - py) - t[0][0] * (t[2][1] - py);
                            // (offset, slope) of w0, w1 and w2 in px.
                            let w = [(inv * e0, inv * s0), (inv * e1, inv * s1), (1.0 - inv * (e0 + e1), -inv * (s0 + s1))];
                            let (mut lo, mut hi) = (f32::MIN, f32::MAX);
                            for (a, b) in w {
                                if b > 1e-12 {
                                    lo = lo.max((EDGE_SLACK - a) / b);
                                } else if b < -1e-12 {
                                    hi = hi.min((EDGE_SLACK - a) / b);
                                } else if a < EDGE_SLACK {
                                    hi = f32::MIN;
                                }
                            }
                            xs = ((lo - 0.5).floor() - 1.0).max(lo_x as f32) as i64;
                            xe = ((hi - 0.5).ceil() + 1.0).min(hi_x as f32) as i64;
                        }
                        for x in xs..=xe {
                            if *partial && (x + y) & 1 == 1 {
                                continue;
                            }
                            let (px, py) = (x as f32 + 0.5, y as f32 + 0.5);
                            let w0 = ((t[1][0] - px) * (t[2][1] - py) - (t[2][0] - px) * (t[1][1] - py)) * inv;
                            let w1 = ((t[2][0] - px) * (t[0][1] - py) - (t[0][0] - px) * (t[2][1] - py)) * inv;
                            let w2 = 1.0 - w0 - w1;
                            if w0 < -1e-4 || w1 < -1e-4 || w2 < -1e-4 {
                                continue;
                            }
                            let h = w0 * t[0][2] + w1 * t[1][2] + w2 * t[2][2];
                            let at = (y as usize - y0) * n + x as usize;
                            if h > rows[at] {
                                rows[at] = h;
                            }
                        }
                    }
                }
            }
            rows
        });
        let mut height = Vec::with_capacity(n * n);
        for rows in drawn {
            height.extend(rows);
        }
        SunDepth { size, dir, u, v, origin, texel, height }
    }

    /// World point to (texel x, texel y, height toward the sun).
    pub fn project(&self, p: [f32; 3]) -> [f32; 3] {
        [(dot(p, self.u) - self.origin[0]) / self.texel, (dot(p, self.v) - self.origin[1]) / self.texel, dot(p, self.dir)]
    }

    /// How much sun `p` gets (3x3 texels, `bias` units).
    pub fn visibility(&self, p: [f32; 3], bias: f32) -> f32 {
        let [x, y, h] = self.project(p);
        let n = self.size as i64;
        let (cx, cy) = (x.floor() as i64, y.floor() as i64);
        let (mut lit, mut all) = (0.0, 0.0);
        for dy in -1..=1 {
            for dx in -1..=1 {
                let (tx, ty) = (cx + dx, cy + dy);
                all += 1.0;
                if tx < 0 || ty < 0 || tx >= n || ty >= n {
                    lit += 1.0;
                    continue;
                }
                if h + bias >= self.height[(ty * n + tx) as usize] {
                    lit += 1.0;
                }
            }
        }
        lit / all
    }

    /// The map as RGBA8 texels: the height over `range` (low corner
    /// `base`) in 24 bits (r high, g, b low), alpha 255 where something is
    /// and 0 where nothing is.
    pub fn packed(&self, base: f32, range: f32) -> Vec<u8> {
        let parts = parallel_chunks(self.height.len(), 1 << 18, |r| {
            let mut out = Vec::with_capacity(r.len() * 4);
            for &h in &self.height[r] {
                if h == f32::MIN {
                    out.extend_from_slice(&[0, 0, 0, 0]);
                    continue;
                }
                let t = ((h - base) / range).clamp(0.0, 1.0);
                let q = (t * 16_777_215.0).round() as u32;
                out.extend_from_slice(&[(q >> 16) as u8, (q >> 8) as u8, q as u8, 255]);
            }
            out
        });
        let mut out = Vec::with_capacity(self.height.len() * 4);
        for p in parts {
            out.extend(p);
        }
        out
    }

    /// The lowest and highest heights stored.
    pub fn height_range(&self) -> (f32, f32) {
        let mut lo = f32::MAX;
        let mut hi = f32::MIN;
        for &h in &self.height {
            if h != f32::MIN {
                lo = lo.min(h);
                hi = hi.max(h);
            }
        }
        (lo, hi)
    }
}

/// Runs `f` over `0..n` in ranges of `chunk` on every core (a range is taken
/// by whichever thread is free), the results in range order.
pub fn parallel_chunks<T: Send>(n: usize, chunk: usize, f: impl Fn(std::ops::Range<usize>) -> T + Sync) -> Vec<T> {
    let chunk = chunk.max(1);
    let count = n.div_ceil(chunk);
    let threads = std::thread::available_parallelism().map_or(4, |c| c.get()).clamp(1, 64).min(count.max(1));
    let next = std::sync::atomic::AtomicUsize::new(0);
    let mut done: Vec<(usize, T)> = std::thread::scope(|s| {
        let jobs: Vec<_> = (0..threads)
            .map(|_| {
                let (f, next) = (&f, &next);
                s.spawn(move || {
                    let mut mine = Vec::new();
                    loop {
                        let i = next.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                        if i >= count {
                            break;
                        }
                        mine.push((i, f(i * chunk..((i + 1) * chunk).min(n))));
                    }
                    mine
                })
            })
            .collect();
        jobs.into_iter().flat_map(|j| j.join().expect("parallel chunk")).collect()
    });
    done.sort_by_key(|d| d.0);
    done.into_iter().map(|d| d.1).collect()
}

/// Runs `f` over `0..n` on every core, results in order.
pub fn parallel_map<T: Send + Default + Clone>(n: usize, f: impl Fn(usize) -> T + Sync) -> Vec<T> {
    let threads = std::thread::available_parallelism().map_or(4, |c| c.get()).clamp(1, 32);
    let mut out = vec![T::default(); n];
    let chunk = n.div_ceil(threads).max(1);
    std::thread::scope(|s| {
        for (i, part) in out.chunks_mut(chunk).enumerate() {
            let f = &f;
            s.spawn(move || {
                for (j, v) in part.iter_mut().enumerate() {
                    *v = f(i * chunk + j);
                }
            });
        }
    });
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_roof_shades_the_floor() {
        // A floor at z 0 and a roof 100 units over x -50..50.
        let floor = [[[-500.0, -500.0, 0.0], [500.0, -500.0, 0.0], [500.0, 500.0, 0.0]], [[-500.0, -500.0, 0.0], [500.0, 500.0, 0.0], [-500.0, 500.0, 0.0]]];
        let roof = [[[-50.0, -50.0, 100.0], [50.0, -50.0, 100.0], [50.0, 50.0, 100.0]], [[-50.0, -50.0, 100.0], [50.0, 50.0, 100.0], [-50.0, 50.0, 100.0]]];
        let t = Tracer::new(&[floor[0], floor[1], roof[0], roof[1]]);
        let up = [0.0, 0.0, 1.0];
        assert_eq!(t.sun_visibility([0.0, 0.0, 0.0], up, up, 0.01, 0.5), 0.0);
        assert_eq!(t.sun_visibility([200.0, 0.0, 0.0], up, up, 0.01, 0.5), 1.0);
        let under = t.openness([0.0, 0.0, 0.0], up, 1000.0, 32, 1, 0.5);
        let out = t.openness([400.0, 400.0, 0.0], up, 1000.0, 32, 1, 0.5);
        assert!(under < 0.85 && out > 0.95, "{under} {out}");
    }

    #[test]
    fn the_sun_map_shades_under_a_roof() {
        let roof = [[[-50.0, -50.0, 100.0], [50.0, -50.0, 100.0], [50.0, 50.0, 100.0]], [[-50.0, -50.0, 100.0], [50.0, 50.0, 100.0], [-50.0, 50.0, 100.0]]];
        let floor = [[[-500.0, -500.0, 0.0], [500.0, -500.0, 0.0], [500.0, 500.0, 0.0]], [[-500.0, -500.0, 0.0], [500.0, 500.0, 0.0], [-500.0, 500.0, 0.0]]];
        let casters: Vec<Caster> = roof.iter().chain(floor.iter()).map(|p| Caster { p: *p, partial: false }).collect();
        let m = SunDepth::render(&casters, [0.0, 0.0, 1.0], [0.0; 3], 1000.0, 256);
        assert_eq!(m.visibility([0.0, 0.0, 0.0], 2.0), 0.0);
        assert_eq!(m.visibility([200.0, 0.0, 0.0], 2.0), 1.0);
        assert_eq!(m.visibility([0.0, 0.0, 100.0], 2.0), 1.0);
        let (lo, hi) = m.height_range();
        assert!((lo - 0.0).abs() < 1e-3 && (hi - 100.0).abs() < 1e-3);
    }

    fn lcg(s: &mut u64) -> f32 {
        *s = s.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407);
        (*s >> 40) as f32 / (1u32 << 24) as f32
    }

    /// The nearest hit of a ray over `tris` by trying every one.
    fn brute_nearest(tris: &[[[f32; 3]; 3]], o: [f32; 3], d: [f32; 3], max: f32) -> Option<f32> {
        let mut best: Option<f32> = None;
        for t in tris {
            let (e1, e2) = (sub(t[1], t[0]), sub(t[2], t[0]));
            let pv = cross(d, e2);
            let det = dot(e1, pv);
            if det.abs() < 1e-9 {
                continue;
            }
            let id = 1.0 / det;
            let tv = sub(o, t[0]);
            let u = dot(tv, pv) * id;
            let qv = cross(tv, e1);
            let v = dot(d, qv) * id;
            let dist = dot(e2, qv) * id;
            if (0.0..=1.0).contains(&u) && v >= 0.0 && u + v <= 1.0 && dist > 1e-3 && dist < best.unwrap_or(max) {
                best = Some(dist);
            }
        }
        best
    }

    #[test]
    fn the_tree_finds_what_a_search_of_every_triangle_finds() {
        // Enough triangles for the build to split across threads.
        let mut s = 7u64;
        let tris: Vec<[[f32; 3]; 3]> = (0..50_000)
            .map(|_| {
                let c = [lcg(&mut s) * 1000.0, lcg(&mut s) * 1000.0, lcg(&mut s) * 100.0];
                [0, 1, 2].map(|_| [c[0] + lcg(&mut s) * 20.0, c[1] + lcg(&mut s) * 20.0, c[2] + lcg(&mut s) * 20.0])
            })
            .collect();
        let tracer = Tracer::new(&tris);
        assert!(tracer.len() > 49_000);
        for _ in 0..300 {
            let o = [lcg(&mut s) * 1000.0, lcg(&mut s) * 1000.0, lcg(&mut s) * 100.0];
            let d = [lcg(&mut s) - 0.5, lcg(&mut s) - 0.5, lcg(&mut s) - 0.5];
            let l = dot(d, d).sqrt().max(1e-3);
            let d = d.map(|v| v / l);
            let want = brute_nearest(&tris, o, d, 800.0);
            let got = tracer.nearest(o, d, 800.0).map(|h| h.0);
            match (want, got) {
                (None, None) => {}
                (Some(a), Some(b)) => assert!((a - b).abs() < 1e-2, "{a} {b}"),
                _ => panic!("brute {want:?} tree {got:?}"),
            }
            assert_eq!(tracer.occluded(o, d, 800.0), want.is_some());
        }
    }

    #[test]
    fn the_sun_map_matches_a_plain_box_raster() {
        // Small triangles, long thin ones and big ones, some partial.
        let mut s = 3u64;
        let mut casters = Vec::new();
        for i in 0..3000 {
            let c = [lcg(&mut s) * 900.0 + 50.0, lcg(&mut s) * 900.0 + 50.0, lcg(&mut s) * 100.0];
            let r = if i % 50 == 0 { 600.0 } else if i % 7 == 0 { 60.0 } else { 6.0 };
            let p = [0, 1, 2].map(|_| [c[0] + (lcg(&mut s) - 0.5) * r, c[1] + (lcg(&mut s) - 0.5) * r, c[2] + (lcg(&mut s) - 0.5) * 20.0]);
            casters.push(Caster { p, partial: i % 5 == 0 });
        }
        let d = SunDepth::render(&casters, [0.2, 0.1, 0.97], [500.0, 500.0, 0.0], 1100.0, 512);
        let n = d.size as usize;
        let mut want = vec![f32::MIN; n * n];
        for c in &casters {
            let t = c.p.map(|p| d.project(p));
            let (lo_y, hi_y) = (t[0][1].min(t[1][1]).min(t[2][1]).floor().max(0.0) as i64, t[0][1].max(t[1][1]).max(t[2][1]).ceil().min(n as f32 - 1.0) as i64);
            let (lo_x, hi_x) = (t[0][0].min(t[1][0]).min(t[2][0]).floor().max(0.0) as i64, t[0][0].max(t[1][0]).max(t[2][0]).ceil().min(n as f32 - 1.0) as i64);
            let area = (t[1][0] - t[0][0]) * (t[2][1] - t[0][1]) - (t[2][0] - t[0][0]) * (t[1][1] - t[0][1]);
            if lo_y > hi_y || lo_x > hi_x || area.abs() < 1e-9 {
                continue;
            }
            let inv = 1.0 / area;
            for y in lo_y..=hi_y {
                for x in lo_x..=hi_x {
                    if c.partial && (x + y) & 1 == 1 {
                        continue;
                    }
                    let (px, py) = (x as f32 + 0.5, y as f32 + 0.5);
                    let w0 = ((t[1][0] - px) * (t[2][1] - py) - (t[2][0] - px) * (t[1][1] - py)) * inv;
                    let w1 = ((t[2][0] - px) * (t[0][1] - py) - (t[0][0] - px) * (t[2][1] - py)) * inv;
                    let w2 = 1.0 - w0 - w1;
                    if w0 < -1e-4 || w1 < -1e-4 || w2 < -1e-4 {
                        continue;
                    }
                    let h = w0 * t[0][2] + w1 * t[1][2] + w2 * t[2][2];
                    let at = y as usize * n + x as usize;
                    if h > want[at] {
                        want[at] = h;
                    }
                }
            }
        }
        assert!(want.iter().any(|h| *h != f32::MIN));
        assert!(d.height == want, "{} texels differ", d.height.iter().zip(&want).filter(|(a, b)| a != b).count());
    }
}
