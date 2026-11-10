//! Is a Black Ops III map complete? What the map holds against what the
//! scene draws, and where the collision has solid geometry that nothing
//! drawn covers (a hole in the map).
//!
//! [`tally`]: every world surface and every static model placement surface
//! is drawn by the G-buffer (a slot 3 technique), by the forward lists (a
//! slot 6 technique) or not at all; the undrawn are grouped by why (the
//! material's technique error, shadow-only, no material, model not built)
//! and by technique set family (the technique set's name without its
//! `#hash`). The scene logs it as one `completeness:` line at load.
//!
//! [`Holes`]: rays from the map's spawns against the collision (solid
//! brushes, the clip map's triangles, the collision triangles of the static
//! models it lists) and against the scene's triangles (world surfaces and
//! placed model meshes, drawn and undrawn). A ray whose first collision hit
//! has no drawn surface at or before it is a hole; it is put down to the
//! undrawn draw it meets there (that draw's reason), else to the collision
//! that has nothing drawn (its material / brush). Run by the
//! `t7_map_complete` example over every staged map.

use std::collections::BTreeMap;

use glam::{Affine3A, Mat3A, Vec3, Vec3A};

use crate::scene::{Draw, Scene, is_shadow_caster};

/// How one draw is drawn.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Status {
    GBuffer,
    Forward,
    /// Not drawn, and why.
    Undrawn(String),
}

/// The status of draw `d`.
pub fn status(scene: &Scene, d: &Draw) -> Status {
    let m = &scene.materials[d.material];
    if is_shadow_caster(&m.name, &m.techset) {
        return Status::Undrawn("shadow-only (shadowcaster / onlycast)".into());
    }
    if let Some(t) = m.technique.as_ref().or(m.forward.as_ref())
        && let Some(why) = missing_cbuffer(m, t)
    {
        return Status::Undrawn(why);
    }
    if m.technique.is_some() {
        Status::GBuffer
    } else if m.forward.is_some() {
        Status::Forward
    } else {
        Status::Undrawn(m.undrawn.clone().unwrap_or_else(|| "no technique".into()))
    }
}

/// A constant buffer technique `t`'s programs read from the material
/// (not a code buffer) that the material does not hold: its draws are
/// skipped at binding (`constant buffer ... not in the material`).
pub fn missing_cbuffer(m: &crate::scene::Material, t: &crate::scene::Technique) -> Option<String> {
    for (st, program) in [(0u16, &t.vs), (1u16, &t.ps)] {
        let index = (t.slot * 6) as u8 + t.buffer_set + if st == 0 { 2 } else { 4 };
        if m.cbuffers.iter().any(|(i, _, _)| *i == index) {
            continue;
        }
        for b in crate::rdef::bindings(program).iter().filter(|b| b.kind == crate::rdef::CBUFFER) {
            if !t.args.iter().any(|a| a.ty & 0xff00 == 0x400 && a.ty & 0xff == st && u32::from(a.buffer) == b.point) {
                return Some("constant buffer not in the material (unresolved reference)".into());
            }
        }
    }
    None
}

/// A technique set's family: its name without the `#hash`.
pub fn family(techset: &str) -> &str {
    techset.split('#').next().unwrap_or(techset)
}

/// Counts in surfaces: world surfaces, and static model surfaces per
/// placement.
#[derive(Clone, Debug, Default)]
pub struct Counts {
    pub world: usize,
    pub models: usize,
}

impl Counts {
    pub fn total(&self) -> usize {
        self.world + self.models
    }
}

#[derive(Clone, Debug, Default)]
pub struct Tally {
    pub total: Counts,
    pub gbuffer: Counts,
    pub forward: Counts,
    /// (reason, family) -> (counts, materials).
    pub undrawn: BTreeMap<(String, String), (Counts, Vec<String>)>,
}

impl Tally {
    pub fn undrawn_total(&self) -> Counts {
        let mut c = Counts::default();
        for (n, _) in self.undrawn.values() {
            c.world += n.world;
            c.models += n.models;
        }
        c
    }

    /// Undrawn surfaces by reason (most first).
    pub fn by_reason(&self) -> Vec<(String, usize)> {
        let mut m: BTreeMap<String, usize> = BTreeMap::new();
        for ((r, _), (n, _)) in &self.undrawn {
            *m.entry(r.clone()).or_default() += n.total();
        }
        let mut v: Vec<_> = m.into_iter().collect();
        v.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
        v
    }

    /// The one log line of a map.
    pub fn line(&self, map: &str) -> String {
        let u = self.undrawn_total();
        let reasons: Vec<String> = self.by_reason().iter().map(|(r, n)| format!("{r}: {n}")).collect();
        format!(
            "completeness {map}: {} surfaces in the map ({} world, {} model placement surfaces); G-buffer {}, forward {}, not drawn {} ({} world, {} model){}{}",
            self.total.total(),
            self.total.world,
            self.total.models,
            self.gbuffer.total(),
            self.forward.total(),
            u.total(),
            u.world,
            u.models,
            if reasons.is_empty() { "" } else { "; not drawn by reason: " },
            reasons.join(", ")
        )
    }
}

/// Whether `d` is the map's (a world surface or a placed static model's),
/// not the sky box's or an effect model pool's (those have no bounds).
pub fn map_surface(scene: &Scene, d: &Draw) -> bool {
    d.group == 3 || ((d.source as usize) < scene.model_names.len() && (d.mins != [0.0; 3] || d.maxs != [0.0; 3]))
}

/// What `scene` holds and draws (see the module).
pub fn tally(scene: &Scene) -> Tally {
    let mut t = Tally::default();
    for d in &scene.draws {
        // (The sky and effect model pools are not map surfaces.)
        if !map_surface(scene, d) {
            continue;
        }
        let n = if d.group == 3 { Counts { world: 1, models: 0 } } else { Counts { world: 0, models: d.instances as usize } };
        let add = |c: &mut Counts| {
            c.world += n.world;
            c.models += n.models;
        };
        add(&mut t.total);
        match status(scene, d) {
            Status::GBuffer => add(&mut t.gbuffer),
            Status::Forward => add(&mut t.forward),
            Status::Undrawn(r) => {
                let m = &scene.materials[d.material];
                let e = t.undrawn.entry((r, family(&m.techset).to_owned())).or_default();
                add(&mut e.0);
                if !e.1.contains(&m.name) {
                    e.1.push(m.name.clone());
                }
            }
        }
    }
    // What the scene made no draw of (shadow casters are counted above when
    // kept, `CODLEGACY_T7_DEBUG_SURF=shadow_casters`, else here).
    for (why, n) in &scene.not_drawn {
        let world = why.starts_with("world");
        let reason = if why.starts_with("shadow-only") { "shadow-only (shadowcaster / onlycast)".to_owned() } else { why.clone() };
        let c = if world { Counts { world: *n, models: 0 } } else { Counts { world: 0, models: *n } };
        t.total.world += c.world;
        t.total.models += c.models;
        let e = t.undrawn.entry((reason, "-".into())).or_default();
        e.0.world += c.world;
        e.0.models += c.models;
    }
    t
}

/// A bounding volume hierarchy over boxes (median split on the longest
/// axis, at most 4 per leaf).
#[derive(Default)]
pub struct Bvh {
    /// (min, max, start, count): count 0 is an inner node whose children
    /// are the next node and node `start`.
    nodes: Vec<(Vec3A, Vec3A, u32, u32)>,
    order: Vec<u32>,
}

impl Bvh {
    pub fn build(boxes: &[(Vec3A, Vec3A)]) -> Self {
        let mut b = Bvh { nodes: Vec::with_capacity(boxes.len() / 2 + 1), order: (0..boxes.len() as u32).collect() };
        if !boxes.is_empty() {
            b.split(boxes, 0, boxes.len());
        }
        b
    }

    fn split(&mut self, boxes: &[(Vec3A, Vec3A)], lo: usize, hi: usize) -> usize {
        let (mut mn, mut mx) = (Vec3A::splat(f32::MAX), Vec3A::splat(f32::MIN));
        let (mut cmn, mut cmx) = (Vec3A::splat(f32::MAX), Vec3A::splat(f32::MIN));
        for &i in &self.order[lo..hi] {
            let (a, b) = boxes[i as usize];
            mn = mn.min(a);
            mx = mx.max(b);
            let c = (a + b) * 0.5;
            cmn = cmn.min(c);
            cmx = cmx.max(c);
        }
        let me = self.nodes.len();
        self.nodes.push((mn, mx, lo as u32, (hi - lo) as u32));
        if hi - lo <= 4 {
            return me;
        }
        let e = cmx - cmn;
        let axis = if e.x >= e.y && e.x >= e.z { 0 } else if e.y >= e.z { 1 } else { 2 };
        let mid = (lo + hi) / 2;
        self.order[lo..hi].select_nth_unstable_by(mid - lo, |&p, &q| {
            let c = |i: u32| (boxes[i as usize].0 + boxes[i as usize].1)[axis];
            c(p).total_cmp(&c(q))
        });
        self.split(boxes, lo, mid);
        let right = self.split(boxes, mid, hi);
        self.nodes[me].2 = right as u32;
        self.nodes[me].3 = 0;
        me
    }

    /// Visits the boxes the ray `o + t d` (t in 0..`tmax`) passes through,
    /// nearest nodes first; `hit` returns a closer `tmax` when it hits.
    pub fn trace(&self, o: Vec3A, d: Vec3A, mut tmax: f32, hit: &mut dyn FnMut(u32, f32) -> Option<f32>) -> f32 {
        if self.nodes.is_empty() {
            return tmax;
        }
        let inv = Vec3A::ONE / d;
        let slab = |n: &(Vec3A, Vec3A, u32, u32), tmax: f32| -> Option<f32> {
            let t0 = (n.0 - o) * inv;
            let t1 = (n.1 - o) * inv;
            let tn = t0.min(t1).max_element().max(0.0);
            let tf = t0.max(t1).min_element().min(tmax);
            (tn <= tf).then_some(tn)
        };
        let mut stack = vec![0usize];
        while let Some(i) = stack.pop() {
            let n = &self.nodes[i];
            if slab(n, tmax).is_none() {
                continue;
            }
            if n.3 > 0 {
                for &p in &self.order[n.2 as usize..(n.2 + n.3) as usize] {
                    if let Some(t) = hit(p, tmax) {
                        tmax = tmax.min(t);
                    }
                }
            } else {
                let (a, b) = (i + 1, n.2 as usize);
                let (ta, tb) = (slab(&self.nodes[a], tmax), slab(&self.nodes[b], tmax));
                match (ta, tb) {
                    (Some(x), Some(y)) => {
                        if x <= y {
                            stack.push(b);
                            stack.push(a);
                        } else {
                            stack.push(a);
                            stack.push(b);
                        }
                    }
                    (Some(_), None) => stack.push(a),
                    (None, Some(_)) => stack.push(b),
                    (None, None) => {}
                }
            }
        }
        tmax
    }
}

/// Ray / triangle (both faces): the distance along `d`.
pub fn ray_triangle(o: Vec3A, d: Vec3A, t: &[Vec3A; 3], tmax: f32) -> Option<f32> {
    let (e1, e2) = (t[1] - t[0], t[2] - t[0]);
    let p = d.cross(e2);
    let det = e1.dot(p);
    if det.abs() < 1e-12 {
        return None;
    }
    let inv = 1.0 / det;
    let s = o - t[0];
    let u = s.dot(p) * inv;
    if !(0.0..=1.0).contains(&u) {
        return None;
    }
    let q = s.cross(e1);
    let v = d.dot(q) * inv;
    if v < 0.0 || u + v > 1.0 {
        return None;
    }
    let tt = e2.dot(q) * inv;
    (tt > 1e-3 && tt < tmax).then_some(tt)
}

/// Triangles with a tag each, in a BVH.
#[derive(Default)]
pub struct Mesh {
    /// (Plain floats: 36 bytes a triangle, not 48.)
    pub tris: Vec<[Vec3; 3]>,
    pub tags: Vec<u32>,
    bvh: Bvh,
}

impl Mesh {
    pub fn push(&mut self, t: [Vec3A; 3], tag: u32) {
        self.tris.push(t.map(Vec3::from));
        self.tags.push(tag);
    }

    pub fn finish(&mut self) {
        let boxes: Vec<_> = self.tris.iter().map(|t| t.map(Vec3A::from)).map(|t| (t[0].min(t[1]).min(t[2]), t[0].max(t[1]).max(t[2]))).collect();
        self.bvh = Bvh::build(&boxes);
    }

    pub fn bounds(&self) -> Option<(Vec3A, Vec3A)> {
        self.bvh.nodes.first().map(|n| (n.0, n.1))
    }

    /// The nearest hit whose tag `keep` accepts: (distance, tag).
    pub fn trace(&self, o: Vec3A, d: Vec3A, tmax: f32, keep: &dyn Fn(u32) -> bool) -> Option<(f32, u32)> {
        let mut best = None;
        self.bvh.trace(o, d, tmax, &mut |i, tm| {
            let i = i as usize;
            if !keep(self.tags[i]) {
                return None;
            }
            let t = ray_triangle(o, d, &self.tris[i].map(Vec3A::from), tm)?;
            best = Some((t, self.tags[i]));
            Some(t)
        });
        best
    }

    /// As [`Mesh::trace`], with the hit triangle's unit normal (mesh space).
    pub fn trace_normal(&self, o: Vec3A, d: Vec3A, tmax: f32, keep: &dyn Fn(u32) -> bool) -> Option<(f32, u32, Vec3A)> {
        let mut best = None;
        self.bvh.trace(o, d, tmax, &mut |i, tm| {
            let i = i as usize;
            if !keep(self.tags[i]) {
                return None;
            }
            let tri = self.tris[i].map(Vec3A::from);
            let t = ray_triangle(o, d, &tri, tm)?;
            best = Some((t, self.tags[i], (tri[1] - tri[0]).cross(tri[2] - tri[0]).normalize_or_zero()));
            Some(t)
        });
        best
    }
}

/// Meshes placed by affine transforms (mesh 0 at the identity: the world).
#[derive(Default)]
pub struct Placed {
    pub meshes: Vec<Mesh>,
    /// (mesh, world to mesh, world bounds).
    inst: Vec<(usize, Affine3A)>,
    bvh: Bvh,
}

impl Placed {
    /// The nearest hit whose tag `keep` accepts.
    pub fn trace(&self, o: Vec3A, d: Vec3A, tmax: f32, keep: &dyn Fn(u32) -> bool) -> Option<(f32, u32)> {
        let mut best = None;
        self.bvh.trace(o, d, tmax, &mut |i, tm| {
            let (m, inv) = &self.inst[i as usize];
            // (A linear map keeps the ray parameter.)
            let (lo, ld) = (inv.transform_point3a(o), inv.transform_vector3a(d));
            let (t, tag) = self.meshes[*m].trace(lo, ld, tm, keep)?;
            best = Some((t, tag));
            Some(t)
        });
        best
    }

    /// As [`Placed::trace`], with the hit surface's unit normal (world
    /// space, either face).
    pub fn trace_normal(&self, o: Vec3A, d: Vec3A, tmax: f32, keep: &dyn Fn(u32) -> bool) -> Option<(f32, u32, Vec3A)> {
        let mut best = None;
        self.bvh.trace(o, d, tmax, &mut |i, tm| {
            let (m, inv) = &self.inst[i as usize];
            let (lo, ld) = (inv.transform_point3a(o), inv.transform_vector3a(d));
            let (t, tag, n) = self.meshes[*m].trace_normal(lo, ld, tm, keep)?;
            // Normals take the inverse transpose of the mesh-to-world map.
            let n = inv.matrix3.transpose() * n;
            best = Some((t, tag, n.normalize_or_zero()));
            Some(t)
        });
        best
    }

    fn finish(&mut self, placements: Vec<(usize, Affine3A)>) {
        let mut boxes = Vec::new();
        for (m, to_world) in placements {
            let Some((lo, hi)) = self.meshes[m].bounds() else { continue };
            let (mut a, mut b) = (Vec3A::splat(f32::MAX), Vec3A::splat(f32::MIN));
            for c in 0..8 {
                let p = Vec3A::new(if c & 1 == 0 { lo.x } else { hi.x }, if c & 2 == 0 { lo.y } else { hi.y }, if c & 4 == 0 { lo.z } else { hi.z });
                let w = to_world.transform_point3a(p);
                a = a.min(w);
                b = b.max(w);
            }
            if to_world.matrix3.determinant().abs() < 1e-12 || !a.is_finite() || !b.is_finite() {
                continue;
            }
            boxes.push((a, b));
            self.inst.push((m, to_world.inverse()));
        }
        self.bvh = Bvh::build(&boxes);
    }
}

/// A bone matrix (rows: rotation | translation) as an affine map.
fn affine(m: &[f32; 16]) -> Affine3A {
    Affine3A { matrix3: Mat3A::from_cols(Vec3A::new(m[0], m[4], m[8]), Vec3A::new(m[1], m[5], m[9]), Vec3A::new(m[2], m[6], m[10])), translation: Vec3A::new(m[3], m[7], m[11]) }
}

/// The scene's triangles: world surfaces (mesh 0) and each placed model's
/// mesh, every triangle tagged with its draw's index.
pub fn scene_triangles(scene: &Scene) -> Placed {
    let mut p = Placed { meshes: vec![Mesh::default()], ..Default::default() };
    let mut by_source: BTreeMap<u32, usize> = BTreeMap::new();
    let mut placements: Vec<(usize, Affine3A)> = vec![(0, Affine3A::IDENTITY)];
    for (di, d) in scene.draws.iter().enumerate() {
        if !map_surface(scene, d) {
            continue;
        }
        let src = if d.group == 3 { &scene.world } else { &scene.models };
        let mesh = if d.group == 3 {
            0
        } else {
            *by_source.entry(d.source).or_insert_with(|| {
                p.meshes.push(Mesh::default());
                let m = p.meshes.len() - 1;
                for i in d.instance..d.instance + d.instances {
                    if let Some(bone) = scene.instances.get(i as usize).and_then(|ins| scene.bones.get(ins[3] as usize)) {
                        placements.push((m, affine(bone)));
                    }
                }
                m
            })
        };
        for t in 0..(d.index_count / 3) as usize {
            let i0 = d.first_index as usize + 3 * t;
            let Some(ix) = src.indices.get(i0..i0 + 3) else { break };
            let v = |k: usize| crate::winding::pos(&src.positions, (d.base_vertex + i32::from(ix[k])) as usize).map(Vec3A::from);
            let (Some(a), Some(b), Some(c)) = (v(0), v(1), v(2)) else { continue };
            p.meshes[mesh].push([a, b, c], di as u32);
        }
    }
    for m in &mut p.meshes {
        m.finish();
    }
    p.finish(placements);
    p
}

/// What collision a ray met.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum CollKind {
    /// A solid brush (its hit side's surface flags).
    Brush(u32),
    /// A clip map triangle of this material.
    Tri(String),
    /// A listed static model's collision triangle.
    Model(String),
}

/// The map's solid collision.
pub struct Collision {
    /// Clip map triangles (tag: index into `names` + 1 << 31 for static
    /// models' triangles).
    pub mesh: Mesh,
    pub names: Vec<CollKind>,
    brushes: Vec<(Vec<[f32; 4]>, Vec<u32>)>,
    brush_bvh: Bvh,
}

const CONTENTS_SOLID: u32 = 1;
/// A side that is not drawn by design (clip, caulk).
const SURF_NODRAW: u32 = 0x80;

impl Collision {
    pub fn read(scene: &Scene) -> Option<Self> {
        let clip = scene.map.clip.as_ref()?;
        let mut names: Vec<CollKind> = Vec::new();
        let mut name_of: BTreeMap<CollKind, u32> = BTreeMap::new();
        let mut tag = |k: CollKind| -> u32 {
            *name_of.entry(k.clone()).or_insert_with(|| {
                names.push(k);
                (names.len() - 1) as u32
            })
        };
        // Each clip triangle's material: the AABB tree leaves' partitions.
        let mut tri_mat: Vec<Option<u16>> = vec![None; clip.tris.len()];
        for a in &clip.aabb_trees {
            if a.child_count != 0 {
                continue;
            }
            if let Some(part) = clip.partitions.get(a.u as usize) {
                for k in 0..part.tri_count as usize {
                    if let Some(m) = tri_mat.get_mut(part.first_tri as usize + k) {
                        *m = Some(a.material);
                    }
                }
            }
        }
        let mut mesh = Mesh::default();
        for (i, t) in clip.tris.iter().enumerate() {
            let mat = tri_mat[i].and_then(|m| clip.materials.get(m as usize));
            // (Only what stops a player and a bullet: contents solid.)
            if mat.is_some_and(|(_, _, c)| c & CONTENTS_SOLID == 0) {
                continue;
            }
            let v = |k: usize| clip.verts.get(t[k] as usize).copied().map(Vec3A::from);
            let (Some(a), Some(b), Some(c)) = (v(0), v(1), v(2)) else { continue };
            mesh.push([a, b, c], tag(CollKind::Tri(mat.map_or_else(|| "(no material)".to_owned(), |m| m.0.clone()))));
        }
        for sm in &clip.static_models {
            if sm.contents != 0 && sm.contents & CONTENTS_SOLID == 0 {
                continue;
            }
            let Some(tris) = clip.model_collision.get(&sm.name) else { continue };
            let k = tag(CollKind::Model(sm.name.clone()));
            for t in tris {
                if t.contents != 0 && t.contents & CONTENTS_SOLID == 0 {
                    continue;
                }
                let bone = usize::try_from(t.bone).ok().and_then(|b| sm.bones.get(b));
                let place = |p: [f32; 3]| {
                    let p = bone.map_or(p, |m| fastfile_t7::map::bone_to_model(m, p));
                    let a = sm.axis;
                    Vec3A::from(sm.origin) + (Vec3A::from(a[0]) * p[0] + Vec3A::from(a[1]) * p[1] + Vec3A::from(a[2]) * p[2]) * sm.scale
                };
                mesh.push(t.corners.map(place), k);
            }
        }
        mesh.finish();
        let mut brushes = Vec::new();
        let mut boxes = Vec::new();
        for b in &clip.brushes {
            if b.contents & CONTENTS_SOLID == 0 || b.planes.len() < 6 {
                continue;
            }
            // The six axial sides first (+x, -x, +y, -y, +z, -z).
            let p = &b.planes;
            let lo = Vec3A::new(-p[1][3], -p[3][3], -p[5][3]);
            let hi = Vec3A::new(p[0][3], p[2][3], p[4][3]);
            if !(lo.cmple(hi).all()) {
                continue;
            }
            boxes.push((lo, hi));
            brushes.push((b.planes.clone(), b.plane_surface_flags.clone()));
        }
        let brush_bvh = Bvh::build(&boxes);
        Some(Collision { mesh, names, brushes, brush_bvh })
    }

    /// The nearest solid hit: (distance, what), an eye inside a brush not
    /// meeting that brush.
    pub fn trace(&self, o: Vec3A, d: Vec3A, tmax: f32) -> Option<(f32, CollKind)> {
        let mut best = self.mesh.trace(o, d, tmax, &|_| true).map(|(t, k)| (t, self.names[k as usize].clone()));
        let tm = best.as_ref().map_or(tmax, |b| b.0);
        self.brush_bvh.trace(o, d, tm, &mut |i, tm| {
            let (planes, flags) = &self.brushes[i as usize];
            let (mut enter, mut exit, mut side) = (0.0f32, tm, usize::MAX);
            for (k, p) in planes.iter().enumerate() {
                let n = Vec3A::new(p[0], p[1], p[2]);
                let (den, dist) = (n.dot(d), n.dot(o) - p[3]);
                if den.abs() < 1e-9 {
                    if dist > 0.0 {
                        return None;
                    }
                    continue;
                }
                let t = -dist / den;
                if den < 0.0 {
                    if t > enter {
                        enter = t;
                        side = k;
                    }
                } else if t < exit {
                    exit = t;
                }
                if enter > exit {
                    return None;
                }
            }
            if side == usize::MAX || enter <= 1e-3 {
                return None;
            }
            best = Some((enter, CollKind::Brush(flags.get(side).copied().unwrap_or(0))));
            Some(enter)
        });
        best
    }
}

/// Eye positions: the map's spawn points (`mp_*spawn*` entities), 60
/// units up, at most `max` spread over the list.
pub fn spawn_eyes(scene: &Scene, max: usize) -> Vec<Vec3> {
    let mut v = Vec::new();
    for block in scene.map.entities.split('}') {
        let val = |k: &str| block.lines().find_map(|l| l.trim().strip_prefix(&format!("\"{k}\" \"")).map(|v| v.trim_end_matches('"').to_owned()));
        let Some(class) = val("classname") else { continue };
        if !(class.starts_with("mp_") && class.contains("spawn")) {
            continue;
        }
        let o: Vec<f32> = val("origin").map(|o| o.split_whitespace().filter_map(|x| x.parse().ok()).collect()).unwrap_or_else(Vec::new);
        if o.len() == 3 {
            v.push(Vec3::new(o[0], o[1], o[2] + 60.0));
        }
    }
    v.sort_by(|a, b| a.x.total_cmp(&b.x).then(a.y.total_cmp(&b.y)).then(a.z.total_cmp(&b.z)));
    v.dedup_by(|a, b| a.distance(*b) < 32.0);
    let step = v.len().div_ceil(max.max(1)).max(1);
    v.into_iter().step_by(step).collect()
}

/// `n` directions spread over the sphere (Fibonacci), straight up left out
/// (above 60 degrees: the sky).
pub fn directions(n: usize) -> Vec<Vec3A> {
    let g = std::f32::consts::PI * (3.0 - 5f32.sqrt());
    (0..n)
        .map(|i| {
            let z = 1.0 - 2.0 * (i as f32 + 0.5) / n as f32;
            let r = (1.0 - z * z).sqrt();
            Vec3A::new(r * (g * i as f32).cos(), r * (g * i as f32).sin(), z)
        })
        .filter(|d| d.z < 0.866)
        .collect()
}

/// Holes found from a map's spawns.
#[derive(Default, Debug)]
pub struct Holes {
    pub rays: usize,
    /// Rays meeting solid collision within range.
    pub solid: usize,
    /// Holes by what explains them: an undrawn draw's reason, else the
    /// collision with nothing drawn on it.
    pub by_cause: BTreeMap<String, usize>,
    /// Rays meeting collision that is invisible by design (nodraw brushes,
    /// clip triangles), not holes.
    pub expected: BTreeMap<String, usize>,
    /// A few (eye, direction, distance, cause) to look at.
    pub examples: Vec<(Vec3, Vec3, f32, String)>,
}

impl Holes {
    pub fn total(&self) -> usize {
        self.by_cause.values().sum()
    }

    /// Holes by their kind (the cause without the model / material name).
    pub fn by_kind(&self) -> Vec<(String, usize)> {
        let mut m: BTreeMap<String, usize> = BTreeMap::new();
        for (c, n) in &self.by_cause {
            let k = if c.starts_with("undrawn") {
                c.clone()
            } else if let Some(i) = c.find("clip triangles") {
                c[..i + "clip triangles".len()].to_owned()
            } else if c.contains("solid brush") {
                "nothing drawn: solid brush".to_owned()
            } else {
                c.rsplit_once(": ").map_or(c.clone(), |(a, _)| a.to_owned())
            };
            *m.entry(k).or_default() += n;
        }
        let mut v: Vec<_> = m.into_iter().collect();
        v.sort_by_key(|x| std::cmp::Reverse(x.1));
        v
    }
}

/// How far behind the collision a drawn surface may be and still count as
/// covering it (collision is often a hull a little outside the visuals).
pub const SLACK: f32 = 24.0;

/// Casts `dirs` from every eye (range `range`).
pub fn find_holes(scene: &Scene, tris: &Placed, coll: &Collision, eyes: &[Vec3], dirs: &[Vec3A], range: f32) -> Holes {
    let status: Vec<Status> = scene.draws.iter().map(|d| status(scene, d)).collect();
    let drawn = |t: u32| !matches!(status[t as usize], Status::Undrawn(_));
    let undrawn = |t: u32| matches!(status[t as usize], Status::Undrawn(_));
    let mut h = Holes::default();
    for &eye in eyes {
        let o = Vec3A::from(eye);
        for &d in dirs {
            h.rays += 1;
            let Some((tc, kind)) = coll.trace(o, d, range) else { continue };
            h.solid += 1;
            if tris.trace(o, d, tc + SLACK, &drawn).is_some() {
                continue;
            }
            let cause = match tris.trace(o, d, tc + SLACK, &undrawn) {
                Some((_, di)) => {
                    let m = &scene.materials[scene.draws[di as usize].material];
                    let Status::Undrawn(r) = &status[di as usize] else { unreachable!() };
                    format!("undrawn: {r} [{}]", family(&m.techset))
                }
                None => match &kind {
                    // (Surface flag 0x80, nodraw: clip and caulk, invisible
                    // by design.)
                    CollKind::Brush(f) if f & SURF_NODRAW != 0 => {
                        *h.expected.entry(format!("nodraw brush (side surface flags {f:#x})")).or_default() += 1;
                        continue;
                    }
                    CollKind::Tri(m) if ["clip", "nodraw", "caulk", "invisible"].iter().any(|w| m.contains(w)) => {
                        *h.expected.entry(format!("clip triangles {m}")).or_default() += 1;
                        continue;
                    }
                    CollKind::Brush(f) => format!("nothing drawn: solid brush (side surface flags {f:#x})"),
                    CollKind::Tri(m) => format!("nothing drawn: clip triangles {m}"),
                    CollKind::Model(m) if scene.model_names.iter().any(|n| n == m) => format!("nothing drawn: collision outside its drawn model's mesh: {m}"),
                    CollKind::Model(m) => format!("model not in the scene (mesh not built / not placed): {m}"),
                },
            };
            if h.examples.len() < 24 && !h.examples.iter().any(|e| e.3 == cause) {
                h.examples.push((eye, Vec3::from(d), tc, cause.clone()));
            }
            *h.by_cause.entry(cause).or_default() += 1;
        }
    }
    h
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rays_meet_the_nearest_triangle_through_the_bvh() {
        let mut m = Mesh::default();
        // A wall of quads at x = 10 + 5k (k = 0..50), each two triangles.
        for k in 0..50 {
            let x = 10.0 + 5.0 * k as f32;
            let q = |y: f32, z: f32| Vec3A::new(x, y, z);
            m.push([q(-1.0, -1.0), q(1.0, -1.0), q(1.0, 1.0)], 2 * k);
            m.push([q(-1.0, -1.0), q(1.0, 1.0), q(-1.0, 1.0)], 2 * k + 1);
        }
        m.finish();
        let (t, tag) = m.trace(Vec3A::ZERO, Vec3A::X, 1e9, &|_| true).unwrap();
        assert!((t - 10.0).abs() < 1e-4 && tag / 2 == 0, "{t} {tag}");
        // Skipping the first ten walls by tag.
        let (t, _) = m.trace(Vec3A::ZERO, Vec3A::X, 1e9, &|g| g >= 20).unwrap();
        assert!((t - 60.0).abs() < 1e-4, "{t}");
        // Out of range, and missing to the side.
        assert!(m.trace(Vec3A::ZERO, Vec3A::X, 9.0, &|_| true).is_none());
        assert!(m.trace(Vec3A::new(0.0, 5.0, 0.0), Vec3A::X, 1e9, &|_| true).is_none());
        // Placed twice, the copy moved 3 units towards the eye.
        let mut p = Placed { meshes: vec![m], ..Default::default() };
        p.finish(vec![(0, Affine3A::IDENTITY), (0, Affine3A::from_translation(glam::Vec3::new(-3.0, 0.0, 0.0)))]);
        let (t, _) = p.trace(Vec3A::ZERO, Vec3A::X, 1e9, &|_| true).unwrap();
        assert!((t - 7.0).abs() < 1e-4, "{t}");
    }

    #[test]
    fn families_drop_the_hash() {
        assert_eq!(family("wc/lit_decal#86de3377"), "wc/lit_decal");
        assert_eq!(family("mc/lit"), "mc/lit");
    }
}
