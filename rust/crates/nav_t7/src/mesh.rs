//! Black Ops III's nav mesh (`hkaiNavMesh`) and paths over it.
//!
//! The object (Havok 2014, 64-bit): after the 0x10-byte referenced-object
//! head, `hkArray`s of faces (0x10), edges (0x20), vertices (0x30),
//! streaming sets (0x40), face data (0x50) and edge data (0x60). A face
//! is 16 bytes (first edge i32, first user edge i32, edge count i16, user
//! edge count i16, cluster i16, pad), an edge 20 (from vertex i32, to
//! vertex i32, opposite edge u32, opposite face u32 (0xffffffff: none; the
//! low 22 bits index, the rest a section), flags u8, pad u8, user cost
//! u16), a vertex an `hkVector4` in metres (x, y horizontal, z up): the
//! game's units are inches, so positions here are scaled by 1 / 0.0254.
//! Checked on `zm_zod`: 3582 faces (3 to 12 edges), 15652 edges whose
//! twins all agree, the players' start structs above faces 16 units below.
//!
//! Paths: A* over faces through the edges they share (and traversal
//! [`Link`]s), then the funnel ("simple stupid funnel") over the portals in
//! the ground plane, heights from the faces crossed.

use std::cmp::Ordering;
use std::collections::BinaryHeap;

use super::hkx::Packfile;

/// Metres to the game's inches.
pub const UNITS_PER_METRE: f32 = 1.0 / 0.0254;

/// How far a path keeps off the mesh's edges at its corners: about a
/// zombie's radius (Black Ops III's AI capsule is 15 units).
pub const AGENT_RADIUS: f32 = 15.0;

#[derive(Clone, Copy, Debug)]
pub struct Face {
    pub first_edge: u32,
    pub edge_count: u16,
    pub cluster: i16,
    /// The centre of its vertices.
    pub centre: [f32; 3],
    pub min: [f32; 3],
    pub max: [f32; 3],
}

#[derive(Clone, Copy, Debug)]
pub struct Edge {
    pub a: u32,
    pub b: u32,
    /// The face on the other side.
    pub opposite: Option<u32>,
}

/// A traversal between two faces (a window's mantle, a jump down): the
/// AI walks to `from` on `from_face`, plays its traversal, arrives at `to`.
#[derive(Clone, Debug)]
pub struct Link {
    pub from_face: u32,
    pub to_face: u32,
    pub from: [f32; 3],
    pub to: [f32; 3],
    /// Extra cost (units) on top of the distance.
    pub cost: f32,
    /// Its kind (the traversal's animscript, `mantle_over_40`, ...).
    pub kind: String,
}

/// A step of a path: walk to `point`; `link` says a traversal starts there.
#[derive(Clone, Debug, PartialEq)]
pub struct Waypoint {
    pub point: [f32; 3],
    /// The traversal this point begins (index into [`NavMesh::links`]).
    pub link: Option<u32>,
}

#[derive(Clone, Debug, Default)]
pub struct NavMesh {
    pub verts: Vec<[f32; 3]>,
    pub faces: Vec<Face>,
    pub edges: Vec<Edge>,
    pub links: Vec<Link>,
    /// Faces paths may not cross (a closed door's: `disconnectpaths`).
    pub blocked: Vec<bool>,
    /// Links out of each face.
    links_from: Vec<Vec<u32>>,
    grid: Grid,
    /// Faces wind counter-clockwise seen from above.
    ccw: bool,
}

#[derive(Clone, Debug, Default)]
struct Grid {
    origin: [f32; 2],
    cell: f32,
    w: usize,
    h: usize,
    cells: Vec<Vec<u32>>,
}

fn sub(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn dist(a: [f32; 3], b: [f32; 3]) -> f32 {
    let d = sub(a, b);
    (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt()
}

/// Twice the signed area of (a, b, c) in the ground plane (positive:
/// counter-clockwise).
fn area2(a: [f32; 3], b: [f32; 3], c: [f32; 3]) -> f32 {
    (b[0] - a[0]) * (c[1] - a[1]) - (c[0] - a[0]) * (b[1] - a[1])
}

fn le_i32(b: &[u8], o: usize) -> i32 {
    i32::from_le_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]])
}

fn le_u32(b: &[u8], o: usize) -> u32 {
    u32::from_le_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]])
}

fn le_f32(b: &[u8], o: usize) -> f32 {
    f32::from_le_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]])
}

fn le_i16(b: &[u8], o: usize) -> i16 {
    i16::from_le_bytes([b[o], b[o + 1]])
}

#[derive(Clone, Copy, PartialEq)]
struct Open {
    f: f32,
    face: u32,
}

impl Eq for Open {}

impl Ord for Open {
    fn cmp(&self, o: &Self) -> Ordering {
        o.f.total_cmp(&self.f).then(o.face.cmp(&self.face))
    }
}

impl PartialOrd for Open {
    fn partial_cmp(&self, o: &Self) -> Option<Ordering> {
        Some(self.cmp(o))
    }
}

/// How a face was entered on a path: through an edge or a link.
#[derive(Clone, Copy, Debug)]
enum Via {
    Start,
    Edge(u32),
    Link(u32),
}

impl NavMesh {
    /// The nav mesh in a packfile, if it holds one.
    pub fn from_packfile(pf: &Packfile<'_>) -> Result<Option<Self>, String> {
        let Some(&at) = pf.objects_of("hkaiNavMesh").first() else {
            return Ok(None);
        };
        let arr = |k: usize, stride: usize| -> Result<&[u8], String> {
            let (data, n) = pf.array(at + k).ok_or_else(|| format!("nav mesh array at {:#x} unreadable", at + k))?;
            if n == 0 {
                return Ok(&[]);
            }
            pf.data_bytes(data, n * stride).ok_or_else(|| format!("nav mesh array at {:#x} past the end", at + k))
        };
        let fb = arr(0x10, 16)?;
        let eb = arr(0x20, 20)?;
        let vb = arr(0x30, 16)?;
        let verts: Vec<[f32; 3]> = vb.chunks_exact(16).map(|c| [le_f32(c, 0) * UNITS_PER_METRE, le_f32(c, 4) * UNITS_PER_METRE, le_f32(c, 8) * UNITS_PER_METRE]).collect();
        let edges: Vec<Edge> = eb
            .chunks_exact(20)
            .map(|c| {
                let opp = le_u32(c, 12);
                Edge { a: le_i32(c, 0) as u32, b: le_i32(c, 4) as u32, opposite: (opp != u32::MAX).then_some(opp & 0x3f_ffff) }
            })
            .collect();
        let mut faces = Vec::with_capacity(fb.len() / 16);
        for c in fb.chunks_exact(16) {
            let first = le_i32(c, 0) as u32;
            let count = le_i16(c, 8) as u16;
            let mut min = [f32::MAX; 3];
            let mut max = [f32::MIN; 3];
            let mut sum = [0.0f32; 3];
            for k in 0..u32::from(count) {
                let e = edges.get((first + k) as usize).ok_or("face edge past the end")?;
                let v = *verts.get(e.a as usize).ok_or("edge vertex past the end")?;
                for i in 0..3 {
                    min[i] = min[i].min(v[i]);
                    max[i] = max[i].max(v[i]);
                    sum[i] += v[i];
                }
            }
            let n = f32::from(count.max(1));
            faces.push(Face { first_edge: first, edge_count: count, cluster: le_i16(c, 12), centre: [sum[0] / n, sum[1] / n, sum[2] / n], min, max });
        }
        let mut m = NavMesh { blocked: vec![false; faces.len()], links_from: vec![Vec::new(); faces.len()], verts, faces, edges, ..Default::default() };
        let total: f32 = (0..m.faces.len() as u32).map(|f| m.signed_area(f)).sum();
        m.ccw = total >= 0.0;
        m.build_grid();
        Ok(Some(m))
    }

    /// A mesh from vertices and faces as vertex loops (twin edges found by
    /// their shared vertices): for tests and generated meshes.
    pub fn from_loops(verts: Vec<[f32; 3]>, loops: &[&[u32]]) -> Self {
        let mut edges = Vec::new();
        let mut faces = Vec::new();
        let mut owner = std::collections::HashMap::new();
        for (fi, l) in loops.iter().enumerate() {
            let first = edges.len() as u32;
            for k in 0..l.len() {
                let (a, b) = (l[k], l[(k + 1) % l.len()]);
                owner.insert((a, b), fi as u32);
                edges.push(Edge { a, b, opposite: None });
            }
            let pts: Vec<[f32; 3]> = l.iter().map(|&i| verts[i as usize]).collect();
            let n = pts.len() as f32;
            let mut min = [f32::MAX; 3];
            let mut max = [f32::MIN; 3];
            let mut c = [0.0f32; 3];
            for p in &pts {
                for i in 0..3 {
                    min[i] = min[i].min(p[i]);
                    max[i] = max[i].max(p[i]);
                    c[i] += p[i] / n;
                }
            }
            faces.push(Face { first_edge: first, edge_count: l.len() as u16, cluster: 0, centre: c, min, max });
        }
        for e in &mut edges {
            e.opposite = owner.get(&(e.b, e.a)).copied();
        }
        let mut m = NavMesh { blocked: vec![false; faces.len()], links_from: vec![Vec::new(); faces.len()], verts, faces, edges, ..Default::default() };
        let total: f32 = (0..m.faces.len() as u32).map(|f| m.signed_area(f)).sum();
        m.ccw = total >= 0.0;
        m.build_grid();
        m
    }

    /// The (first) nav mesh among a zone image's packfiles.
    pub fn from_image(image: &[u8]) -> Result<Option<Self>, String> {
        for pf in super::hkx::find_all(image) {
            if let Some(m) = Self::from_packfile(&pf)? {
                return Ok(Some(m));
            }
        }
        Ok(None)
    }

    fn signed_area(&self, f: u32) -> f32 {
        let face = self.faces[f as usize];
        let mut a = 0.0;
        for k in 0..u32::from(face.edge_count) {
            let e = self.edges[(face.first_edge + k) as usize];
            let (p, q) = (self.verts[e.a as usize], self.verts[e.b as usize]);
            a += p[0] * q[1] - q[0] * p[1];
        }
        a * 0.5
    }

    fn build_grid(&mut self) {
        let mut min = [f32::MAX; 2];
        let mut max = [f32::MIN; 2];
        for f in &self.faces {
            for i in 0..2 {
                min[i] = min[i].min(f.min[i]);
                max[i] = max[i].max(f.max[i]);
            }
        }
        if self.faces.is_empty() {
            return;
        }
        let cell = 128.0;
        let w = (((max[0] - min[0]) / cell) as usize + 1).min(4096);
        let h = (((max[1] - min[1]) / cell) as usize + 1).min(4096);
        let mut cells = vec![Vec::new(); w * h];
        for (i, f) in self.faces.iter().enumerate() {
            let x0 = ((f.min[0] - min[0]) / cell) as usize;
            let x1 = (((f.max[0] - min[0]) / cell) as usize).min(w - 1);
            let y0 = ((f.min[1] - min[1]) / cell) as usize;
            let y1 = (((f.max[1] - min[1]) / cell) as usize).min(h - 1);
            for y in y0..=y1 {
                for x in x0..=x1 {
                    cells[y * w + x].push(i as u32);
                }
            }
        }
        self.grid = Grid { origin: min, cell, w, h, cells };
    }

    /// A face's polygon.
    pub fn polygon(&self, f: u32) -> Vec<[f32; 3]> {
        let face = self.faces[f as usize];
        (0..u32::from(face.edge_count)).map(|k| self.verts[self.edges[(face.first_edge + k) as usize].a as usize]).collect()
    }

    fn contains_2d(&self, f: u32, x: f32, y: f32) -> bool {
        let poly = self.polygon(f);
        let mut inside = false;
        for i in 0..poly.len() {
            let (p, q) = (poly[i], poly[(i + 1) % poly.len()]);
            if (p[1] > y) != (q[1] > y) && x < (q[0] - p[0]) * (y - p[1]) / (q[1] - p[1]) + p[0] {
                inside = !inside;
            }
        }
        inside
    }

    /// The face's height at (x, y) (its plane through the first vertex).
    pub fn height(&self, f: u32, x: f32, y: f32) -> f32 {
        let poly = self.polygon(f);
        // Newell normal.
        let mut n = [0.0f32; 3];
        for i in 0..poly.len() {
            let (p, q) = (poly[i], poly[(i + 1) % poly.len()]);
            n[0] += (p[1] - q[1]) * (p[2] + q[2]);
            n[1] += (p[2] - q[2]) * (p[0] + q[0]);
            n[2] += (p[0] - q[0]) * (p[1] + q[1]);
        }
        let p = poly[0];
        if n[2].abs() < 1e-6 {
            return self.faces[f as usize].centre[2];
        }
        p[2] - (n[0] * (x - p[0]) + n[1] * (y - p[1])) / n[2]
    }

    /// The face under `p`: the one whose surface at p's (x, y) is closest
    /// to it, from `below` units under it to `above` over it.
    pub fn locate(&self, p: [f32; 3], below: f32, above: f32) -> Option<u32> {
        let g = &self.grid;
        if g.cells.is_empty() {
            return None;
        }
        let cx = ((p[0] - g.origin[0]) / g.cell).floor();
        let cy = ((p[1] - g.origin[1]) / g.cell).floor();
        if cx < 0.0 || cy < 0.0 || cx as usize >= g.w || cy as usize >= g.h {
            return None;
        }
        let mut best: Option<(f32, u32)> = None;
        for &f in &g.cells[cy as usize * g.w + cx as usize] {
            if !self.contains_2d(f, p[0], p[1]) {
                continue;
            }
            let dz = p[2] - self.height(f, p[0], p[1]);
            if dz > below || -dz > above {
                continue;
            }
            if best.is_none_or(|b| dz.abs() < b.0) {
                best = Some((dz.abs(), f));
            }
        }
        best.map(|b| b.1)
    }

    /// The face nearest `p` (by its centre) within `radius`, for points just
    /// off the mesh (a spawner in a closet, a node by a wall).
    pub fn nearest(&self, p: [f32; 3], radius: f32) -> Option<u32> {
        if let Some(f) = self.locate(p, 64.0, 32.0) {
            return Some(f);
        }
        let mut best: Option<(f32, u32)> = None;
        for (i, f) in self.faces.iter().enumerate() {
            if p[0] < f.min[0] - radius || p[0] > f.max[0] + radius || p[1] < f.min[1] - radius || p[1] > f.max[1] + radius {
                continue;
            }
            let d = dist(p, f.centre);
            if d <= radius * 2.0 && best.is_none_or(|b| d < b.0) {
                best = Some((d, i as u32));
            }
        }
        best.map(|b| b.1)
    }

    /// Add a traversal link.
    pub fn add_link(&mut self, link: Link) -> u32 {
        let id = self.links.len() as u32;
        self.links_from[link.from_face as usize].push(id);
        self.links.push(link);
        id
    }

    /// A map traversal (a `node_negotiation_begin` and the
    /// `node_negotiation_end` it targets, its `animscript`) as a link
    /// between the faces under its two ends. `None` when an end is not
    /// near the mesh.
    pub fn link_traversal(&mut self, begin: [f32; 3], end: [f32; 3], kind: &str) -> Option<u32> {
        let from_face = self.nearest(begin, 64.0)?;
        let to_face = self.nearest(end, 64.0)?;
        if from_face == to_face {
            return None;
        }
        // (A traversal is slower than walking its length: as BO3's
        // traversals cost, about a second's walk more.)
        Some(self.add_link(Link { from_face, to_face, from: begin, to: end, cost: 64.0, kind: kind.to_owned() }))
    }

    /// Block (or unblock) the faces whose centre lies in a box (a door's
    /// brush: `disconnectpaths` / `connectpaths`). Returns how many changed.
    pub fn block_box(&mut self, min: [f32; 3], max: [f32; 3], blocked: bool) -> usize {
        let mut n = 0;
        for (i, f) in self.faces.iter().enumerate() {
            let c = f.centre;
            let overlaps = (0..3).all(|k| f.max[k] >= min[k] && f.min[k] <= max[k]);
            let centre_in = (0..2).all(|k| c[k] >= min[k] && c[k] <= max[k]) && c[2] >= min[2] - 32.0 && c[2] <= max[2] + 32.0;
            if overlaps && centre_in && self.blocked[i] != blocked {
                self.blocked[i] = blocked;
                n += 1;
            }
        }
        n
    }

    /// The faces reachable from `f` (ignoring links' direction for nothing:
    /// links are one way).
    pub fn reachable(&self, f: u32) -> Vec<bool> {
        let mut seen = vec![false; self.faces.len()];
        let mut stack = vec![f];
        while let Some(f) = stack.pop() {
            if std::mem::replace(&mut seen[f as usize], true) {
                continue;
            }
            for (to, _) in self.neighbours(f) {
                if !seen[to as usize] && !self.blocked[to as usize] {
                    stack.push(to);
                }
            }
        }
        seen
    }

    fn neighbours(&self, f: u32) -> impl Iterator<Item = (u32, Via)> + '_ {
        let face = self.faces[f as usize];
        let edges = (0..u32::from(face.edge_count)).filter_map(move |k| {
            let e = face.first_edge + k;
            self.edges[e as usize].opposite.map(|o| (o, Via::Edge(e)))
        });
        let links = self.links_from[f as usize].iter().map(move |&l| (self.links[l as usize].to_face, Via::Link(l)));
        edges.chain(links)
    }

    fn edge_mid(&self, e: u32) -> [f32; 3] {
        let ed = self.edges[e as usize];
        let (a, b) = (self.verts[ed.a as usize], self.verts[ed.b as usize]);
        [(a[0] + b[0]) * 0.5, (a[1] + b[1]) * 0.5, (a[2] + b[2]) * 0.5]
    }

    /// A path from `start` to `goal` (both near the mesh), as the points to
    /// walk through, the goal last; `None` when no path joins them.
    pub fn find_path(&self, start: [f32; 3], goal: [f32; 3]) -> Option<Vec<Waypoint>> {
        self.find_path_corridor(start, goal).map(|p| p.0)
    }

    /// [`Self::find_path`] and the faces it crosses.
    pub fn find_path_corridor(&self, start: [f32; 3], goal: [f32; 3]) -> Option<(Vec<Waypoint>, Vec<u32>)> {
        let sf = self.nearest(start, 96.0)?;
        let gf = self.nearest(goal, 96.0)?;
        if self.blocked[gf as usize] {
            return None;
        }
        let n = self.faces.len();
        let mut g = vec![f32::INFINITY; n];
        let mut pos = vec![[0.0f32; 3]; n];
        let mut came: Vec<(u32, Via)> = vec![(u32::MAX, Via::Start); n];
        let mut closed = vec![false; n];
        let mut open = BinaryHeap::new();
        g[sf as usize] = 0.0;
        pos[sf as usize] = start;
        open.push(Open { f: dist(start, goal), face: sf });
        while let Some(Open { face, .. }) = open.pop() {
            if face == gf {
                break;
            }
            if std::mem::replace(&mut closed[face as usize], true) {
                continue;
            }
            for (to, via) in self.neighbours(face) {
                let t = to as usize;
                if closed[t] || self.blocked[t] {
                    continue;
                }
                let (p, extra, from_p) = match via {
                    Via::Edge(e) => (if to == gf { goal } else { self.edge_mid(e) }, 0.0, pos[face as usize]),
                    Via::Link(l) => {
                        let link = &self.links[l as usize];
                        (link.to, link.cost + dist(pos[face as usize], link.from), link.from)
                    }
                    Via::Start => continue,
                };
                let ng = g[face as usize] + extra + dist(from_p, p);
                if ng < g[t] {
                    g[t] = ng;
                    pos[t] = p;
                    came[t] = (face, via);
                    open.push(Open { f: ng + dist(p, goal), face: to });
                }
            }
        }
        if !g[gf as usize].is_finite() {
            return None;
        }
        // The faces crossed, start to goal.
        let mut chain = vec![(gf, came[gf as usize].1)];
        let mut f = gf;
        while f != sf {
            let (prev, _) = came[f as usize];
            chain.push((prev, came[prev as usize].1));
            f = prev;
        }
        chain.reverse();
        // Funnel each stretch between links.
        let mut out = Vec::new();
        let mut from = start;
        let mut portals: Vec<([f32; 3], [f32; 3])> = Vec::new();
        for w in chain.windows(2) {
            let (_, via) = w[1];
            match via {
                Via::Edge(e) => {
                    let ed = self.edges[e as usize];
                    let (a, b) = (self.verts[ed.a as usize], self.verts[ed.b as usize]);
                    // Leaving a face through its edge a -> b: with
                    // counter-clockwise faces b is on the left.
                    portals.push(if self.ccw { (b, a) } else { (a, b) });
                }
                Via::Link(l) => {
                    let link = &self.links[l as usize];
                    out.extend(self.funnel(from, link.from, &portals));
                    out.push(Waypoint { point: link.from, link: Some(l) });
                    from = link.to;
                    portals.clear();
                }
                Via::Start => {}
            }
        }
        out.extend(self.funnel(from, goal, &portals));
        out.push(Waypoint { point: goal, link: None });
        self.keep_off_walls(start, &mut out);
        Some((out, chain.iter().map(|c| c.0).collect()))
    }

    /// The funnel's corners are the mesh's own corners, on its edge (a
    /// wall's foot), and the wall is in the narrow angle the path turns
    /// round: each corner is moved [`AGENT_RADIUS`] out of that angle (away
    /// along the bisector of the ways to the points before and after it),
    /// so a path keeps a body's width off the walls. Link ends stay.
    fn keep_off_walls(&self, start: [f32; 3], path: &mut [Waypoint]) {
        let pts: Vec<[f32; 3]> = std::iter::once(start).chain(path.iter().map(|w| w.point)).collect();
        for i in 1..pts.len().saturating_sub(1) {
            if path[i - 1].link.is_some() || (i >= 2 && path[i - 2].link.is_some()) {
                continue;
            }
            let c = pts[i];
            let unit = |v: [f32; 3]| {
                let l = v[0].hypot(v[1]);
                if l < 1e-3 { None } else { Some([v[0] / l, v[1] / l]) }
            };
            let (Some(p), Some(n)) = (unit(sub(pts[i - 1], c)), unit(sub(pts[i + 1], c))) else { continue };
            let Some(bis) = unit([p[0] + n[0], p[1] + n[1], 0.0]) else { continue };
            let q = [c[0] - bis[0] * AGENT_RADIUS, c[1] - bis[1] * AGENT_RADIUS, c[2]];
            // (Only where that is still over the mesh.)
            if let Some(f) = self.locate(q, 48.0, 48.0) {
                path[i - 1].point = [q[0], q[1], self.height(f, q[0], q[1])];
            }
        }
    }

    /// The corners of the shortest way from `a` to `b` through `portals`
    /// (left, right), not including `a` or `b`.
    fn funnel(&self, a: [f32; 3], b: [f32; 3], portals: &[([f32; 3], [f32; 3])]) -> Vec<Waypoint> {
        let mut ps: Vec<([f32; 3], [f32; 3])> = Vec::with_capacity(portals.len() + 1);
        ps.extend_from_slice(portals);
        ps.push((b, b));
        let mut out = Vec::new();
        let mut apex = a;
        let (mut left, mut right) = (a, a);
        let (mut li, mut ri) = (0usize, 0usize);
        let mut i = 0usize;
        let mut guard = 0;
        while i < ps.len() && guard < 10_000 {
            guard += 1;
            let (pl, pr) = ps[i];
            // (Counter-clockwise positive: the left side is counter-
            // clockwise of the right seen from the apex.) Right side: a
            // new right point further left narrows the funnel.
            if area2(apex, right, pr) >= 0.0 {
                if apex == right || area2(apex, left, pr) < 0.0 {
                    right = pr;
                    ri = i;
                } else {
                    // Right over left: left is a corner.
                    out.push(Waypoint { point: left, link: None });
                    apex = left;
                    right = apex;
                    ri = li;
                    i = li + 1;
                    left = apex;
                    continue;
                }
            }
            // Left side.
            if area2(apex, left, pl) <= 0.0 {
                if apex == left || area2(apex, right, pl) > 0.0 {
                    left = pl;
                    li = i;
                } else {
                    out.push(Waypoint { point: right, link: None });
                    apex = right;
                    left = apex;
                    li = ri;
                    i = ri + 1;
                    right = apex;
                    continue;
                }
            }
            i += 1;
        }
        out.retain(|w| w.point != a && w.point != b);
        out.dedup();
        out
    }
}
