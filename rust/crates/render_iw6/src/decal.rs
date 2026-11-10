//! Ghosts decals (effect element type 15, and the impact marks that are the
//! same element) on the map's own world mesh.
//!
//! Black Ops II's world-mark path clips against its own world; a Ghosts map
//! has none, so the mark is cut here: the decal's box (the radius along its
//! three axes, `marks_iw4::mark_fragment_clip_planes`) is clipped against the
//! triangles of every world draw that reaches it (`project`, the engine's
//! mark clipper in `marks_iw4`: `mark_clip_world_triangle_points` and
//! `mark_emit_brush_fragment`), the fragments are packed as world vertices
//! (`pack_mark_world_vertex`: the decal's texture coordinates from the
//! mark's axes and radius, the lightmap coordinates and normals of the
//! surface) and written into a ring of slots.
//!
//! A ring is one dynamic world mesh and one draw (`Renderer::add_dynamic_draw`)
//! per decal material and lighting of the surface (lightmap page, reflection
//! probe, sun / light flags): `SLOTS` marks of at most `SLOT_TRIS` triangles,
//! a slot's unused triangles being degenerate. A new mark takes the ring's
//! next slot (the oldest goes), so nothing is allocated per frame; the draw
//! lists are made again only when a ring is first made (a hitch at the first
//! mark of a material on a surface lit from a new page), and a mark that
//! lives fades over its last second in `FADE_STEPS` alpha steps (a slot's
//! vertices rewritten at each step).
//!
//! The material is the effect element's own (`Scene::material_named`: any
//! zone's material), drawn with its technique set's techniques as a world
//! surface of the decal list is. INFERRED: the clipper and the vertex layout
//! are the IW4 engine's (`marks_iw4`; Ghosts' world vertex is the same 44
//! byte record); not seen on a GPU. `BO2MP_IW6_DECALS=0` turns it off.

use glam::Vec3;
use marks_iw4 as mk;

use crate::scene::{Draw, List, MeshData, VertexKind};
use crate::world::Renderer;

/// `BO2MP_IW6_DECALS=0`: no decals on a Ghosts map.
pub fn on() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| !std::env::var("BO2MP_IW6_DECALS").is_ok_and(|v| v.trim() == "0"))
}

/// The exe's mark budgets: `fx_marks_nearlimit` (5; marks within a new
/// mark's radius on one surface, the oldest goes at the limit; 0 unlimited,
/// `BO2MP_IW6_DECAL_NEARLIMIT`) and its pool of 512 marks at once
/// (`BO2MP_IW6_DECAL_POOL`, 0: only the rings' own slots).
fn budget() -> (usize, usize) {
    static N: crate::switch::Number = crate::switch::Number::new("BO2MP_IW6_DECAL_NEARLIMIT", 5.0);
    static P: crate::switch::Number = crate::switch::Number::new("BO2MP_IW6_DECAL_POOL", 512.0);
    (N.get().max(0.0) as usize, P.get().max(0.0) as usize)
}

/// Marks a ring holds.
pub const SLOTS: usize = 48;
/// Vertices a mark holds (a fan fragment of up to nine points per triangle
/// clipped; a mark that needs more is cut short).
pub const SLOT_POINTS: usize = 32;
/// Triangles a mark holds.
pub const SLOT_TRIS: usize = 24;
const SLOT_INDICES: usize = SLOT_TRIS * 3;
const STRIDE: usize = 44;
/// The most rings (decal material x surface lighting) at once.
pub const MAX_RINGS: usize = 32;
/// A mark fades out over its last `FADE_MS`, in `FADE_STEPS` steps.
pub const FADE_MS: u64 = 1000;
pub const FADE_STEPS: u32 = 8;

/// What a surface is lit by: marks on surfaces of one lighting share a ring.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Lighting {
    pub lightmap: u8,
    pub probe: u8,
    pub sun: bool,
    pub light: Option<u16>,
}

/// One mark to make.
#[derive(Clone, Copy, Debug)]
pub struct Spawn<'a> {
    pub origin: [f32; 3],
    /// The surface normal, the texture axis and the third axis
    /// (`fx_iw4::impact_mark_axis`).
    pub axis: [[f32; 3]; 3],
    pub radius: f32,
    /// Native (BGRA) colour.
    pub color: u32,
    /// 0: until its slot is taken again.
    pub life_ms: u32,
    pub material: &'a str,
}

/// The clipped mark: staging points and triangles (fixed arrays).
pub struct Fragment {
    pub points: [mk::FxMarkStagingPoint; SLOT_POINTS],
    pub tris: [mk::FxMarkStagingTri; SLOT_TRIS],
    pub used_points: u32,
    pub used_tris: u32,
    /// The lighting of the surfaces it was cut from (the first reached; a
    /// surface of another lighting is left out).
    pub lighting: Option<Lighting>,
}

impl Default for Fragment {
    fn default() -> Self {
        Self {
            points: [mk::FxMarkStagingPoint::ZERO; SLOT_POINTS],
            tris: [mk::FxMarkStagingTri::ZERO; SLOT_TRIS],
            used_points: 0,
            used_tris: 0,
            lighting: None,
        }
    }
}

impl Fragment {
    fn clear(&mut self) {
        self.used_points = 0;
        self.used_tris = 0;
        self.lighting = None;
    }
}

/// Ghosts' packed unit vector (unsigned 10:10:10 over 1023,
/// `fastfile_iw6::map::unpack_unit_vec`'s inverse); the engine's mark
/// packer (`marks_iw4`) writes the IW4 form (bytes), so the normal and
/// tangent words are written again in this one.
pub fn pack_unit_vec(n: [f32; 3]) -> u32 {
    let c = |v: f32| (((v.clamp(-1.0, 1.0) * 0.5 + 0.5) * 1023.0 + 0.5) as u32).min(1023);
    c(n[0]) | c(n[1]) << 10 | c(n[2]) << 20
}

fn f32_at(b: &[u8], at: usize) -> f32 {
    b.get(at..at + 4).map_or(0.0, |s| f32::from_le_bytes([s[0], s[1], s[2], s[3]]))
}

fn u32_at(b: &[u8], at: usize) -> u32 {
    b.get(at..at + 4).map_or(0, |s| u32::from_le_bytes([s[0], s[1], s[2], s[3]]))
}

/// A world vertex's position, lightmap coordinates and unpacked normal.
fn read_vertex(bytes: &[u8], v: usize) -> ([f32; 3], [f32; 2], [f32; 3]) {
    let at = v * STRIDE;
    (
        [f32_at(bytes, at), f32_at(bytes, at + 4), f32_at(bytes, at + 8)],
        [f32_at(bytes, at + 28), f32_at(bytes, at + 32)],
        fastfile_iw6::map::unpack_unit_vec(u32_at(bytes, at + 36)),
    )
}

/// Cuts the mark (`origin`, `axis`, `radius`) out of the world draws that
/// reach it (`bounds` are the draws' world bounds, `Renderer::bounds`) into
/// `out`. False when nothing of the world is in it. Draws of movers, models,
/// the sky and blended surfaces are left out.
pub fn project(draws: &[Draw], meshes: &[MeshData], bounds: &[Option<(Vec3, Vec3)>], origin: [f32; 3], axis: [[f32; 3]; 3], radius: f32, out: &mut Fragment) -> bool {
    out.clear();
    let planes = mk::mark_fragment_clip_planes(origin, axis, radius);
    let mark_dir = axis[0];
    let o = Vec3::from(origin);
    // (The box is oriented; its axis-aligned cover is the sphere of its corners.)
    let reach = Vec3::splat(radius * 1.733_f32);
    let (blo, bhi) = (o - reach, o + reach);
    let mut full = false;
    for (di, d) in draws.iter().enumerate() {
        if full {
            break;
        }
        if d.object.is_some() || !matches!(d.list, List::Opaque | List::Decal) {
            continue;
        }
        let Some((lo, hi)) = bounds.get(di).copied().flatten() else { continue };
        if lo.cmpgt(bhi).any() || hi.cmplt(blo).any() {
            continue;
        }
        let Some(m) = meshes.get(d.mesh) else { continue };
        if m.kind != VertexKind::World || m.vertex_bytes.is_empty() {
            continue;
        }
        let lighting = Lighting { lightmap: d.lightmap, probe: d.probe, sun: d.sun, light: d.light };
        if out.lighting.is_some_and(|l| l != lighting) {
            continue;
        }
        let first = d.first_index as usize;
        let Some(idx) = m.indices.get(first..first + d.index_count as usize) else { continue };
        for tri in idx.chunks_exact(3) {
            let at = |k: usize| (d.base_vertex + i32::from(tri[k])).max(0) as usize;
            let (p0, l0, n0) = read_vertex(&m.vertex_bytes, at(0));
            let (p1, l1, n1) = read_vertex(&m.vertex_bytes, at(1));
            let (p2, l2, n2) = read_vertex(&m.vertex_bytes, at(2));
            // (Cheap reject before the six-plane clip.)
            let (tlo, thi) = (Vec3::from(p0).min(Vec3::from(p1)).min(Vec3::from(p2)), Vec3::from(p0).max(Vec3::from(p1)).max(Vec3::from(p2)));
            if tlo.cmpgt(bhi).any() || thi.cmplt(blo).any() {
                continue;
            }
            let mut pts = [mk::FxWorldMarkPoint::ZERO; mk::R_MARK_CHOP_MAX_POINTS];
            let n = mk::mark_clip_world_triangle_points(mark_dir, &planes, p0, p1, p2, &mut pts) as usize;
            if n < 3 {
                continue;
            }
            match mk::mark_emit_brush_fragment(
                out.used_tris,
                out.used_points,
                (SLOT_TRIS * 3) as u32,
                SLOT_POINTS as u32,
                &pts[..n],
                l0,
                l1,
                l2,
                n0,
                n1,
                n2,
                [0; 7],
                &mut out.tris,
                &mut out.points,
            ) {
                Ok((t, p)) => {
                    out.used_tris = t;
                    out.used_points = p;
                    out.lighting.get_or_insert(lighting);
                }
                Err(mk::FxMarkEmitRefuse::Overflow) => {
                    full = true;
                    break;
                }
                Err(mk::FxMarkEmitRefuse::FragmentTooSmall) => {}
            }
        }
    }
    out.used_tris > 0
}

/// The alpha step (1..=`FADE_STEPS`) of a mark of `life_ms` at `age_ms`;
/// `FADE_STEPS` before its last second, 0 when it is over.
pub fn fade_step(age_ms: u64, life_ms: u32) -> u32 {
    let life = u64::from(life_ms);
    if life == 0 || age_ms + FADE_MS < life {
        return FADE_STEPS;
    }
    if age_ms >= life {
        return 0;
    }
    let left = life - age_ms;
    (((left * u64::from(FADE_STEPS)).div_ceil(FADE_MS)) as u32).clamp(1, FADE_STEPS)
}

#[derive(Clone, Copy, Default)]
struct Slot {
    born_ms: u64,
    life_ms: u32,
    live: bool,
    step: u32,
    alpha: u8,
    origin: [f32; 3],
    /// Making order (the oldest goes first).
    seq: u32,
}

struct Ring {
    material: usize,
    lighting: Lighting,
    mesh: usize,
    vertices: Vec<u8>,
    indices: Vec<u16>,
    slots: [Slot; SLOTS],
    next: usize,
}

/// The decals of a native Ghosts frame.
pub struct Decals {
    rings: Vec<Ring>,
    frag: Box<Fragment>,
    /// Material names no zone holds.
    unknown: Vec<String>,
    /// A ring was made: the lists are to be made again.
    pub dirty: bool,
    /// Marks made, and spawns that cut nothing (a count for the log line).
    pub made: u32,
    pub missed: u32,
    seq: u32,
}

impl Default for Decals {
    fn default() -> Self {
        Self { rings: Vec::new(), frag: Box::default(), unknown: Vec::new(), dirty: false, made: 0, missed: 0, seq: 0 }
    }
}

impl Decals {
    pub fn ring_count(&self) -> usize {
        self.rings.len()
    }

    /// Makes one mark at `now_ms`; false when it was not made (off, no such
    /// material, nothing of the world in its box, no room for a ring).
    pub fn spawn(&mut self, r: &mut Renderer, s: &Spawn<'_>, now_ms: u64) -> bool {
        if !on() || s.radius <= 0.0 {
            return false;
        }
        if self.unknown.iter().any(|n| n.eq_ignore_ascii_case(s.material)) {
            return false;
        }
        let Some(material) = r.scene.material_named(s.material) else {
            self.unknown.push(s.material.to_string());
            return false;
        };
        if !project(&r.scene.draws, &r.scene.meshes, &r.bounds, s.origin, s.axis, s.radius, &mut self.frag) {
            self.missed += 1;
            return false;
        }
        let Some(lighting) = self.frag.lighting else { return false };
        let ring = match self.rings.iter().position(|g| g.material == material && g.lighting == lighting) {
            Some(i) => i,
            None => {
                if self.rings.len() >= MAX_RINGS {
                    return false;
                }
                let mesh = r.add_dynamic_draw((lighting.lightmap, lighting.probe, lighting.sun, lighting.light), material, SLOTS * SLOT_POINTS, SLOTS * SLOT_INDICES);
                self.rings.push(Ring {
                    material,
                    lighting,
                    mesh,
                    vertices: vec![0; SLOTS * SLOT_POINTS * STRIDE],
                    indices: vec![0; SLOTS * SLOT_INDICES],
                    slots: [Slot::default(); SLOTS],
                    next: 0,
                });
                self.dirty = true;
                self.rings.len() - 1
            }
        };
        let (limit, pool) = budget();
        self.seq = self.seq.wrapping_add(1);
        let seq = self.seq;
        // The pool is full: the oldest mark anywhere goes.
        if pool > 0 {
            let live = self.rings.iter().map(|g| g.slots.iter().filter(|s| s.live).count()).sum::<usize>();
            if live >= pool {
                let mut oldest: Option<(usize, usize, u32)> = None;
                for (i, g) in self.rings.iter().enumerate() {
                    for (k, sl) in g.slots.iter().enumerate() {
                        if sl.live && oldest.is_none_or(|o| seq.wrapping_sub(sl.seq) > seq.wrapping_sub(o.2)) {
                            oldest = Some((i, k, sl.seq));
                        }
                    }
                }
                if let Some((i, k, _)) = oldest {
                    kill(r, &mut self.rings[i], k);
                }
            }
        }
        let g = &mut self.rings[ring];
        // `limit - 1` marks already within this one's radius: the oldest of
        // them goes, and the new mark takes its slot.
        let mut freed = None;
        if limit > 0 {
            let (mut n, mut oldest): (usize, Option<(usize, u32)>) = (0, None);
            for (k, sl) in g.slots.iter().enumerate() {
                let d = Vec3::from(sl.origin) - Vec3::from(s.origin);
                if sl.live && d.length_squared() < s.radius * s.radius {
                    n += 1;
                    if oldest.is_none_or(|o| seq.wrapping_sub(sl.seq) > seq.wrapping_sub(o.1)) {
                        oldest = Some((k, sl.seq));
                    }
                }
            }
            if n >= limit - 1 && let Some((k, _)) = oldest {
                kill(r, g, k);
                freed = Some(k);
            }
        }
        let slot = freed.unwrap_or(g.next);
        if freed.is_none() {
            g.next = (g.next + 1) % SLOTS;
        }
        let (vbase, ibase) = (slot * SLOT_POINTS, slot * SLOT_INDICES);
        let f = &*self.frag;
        for p in 0..f.used_points as usize {
            let mut row = mk::pack_mark_world_vertex(&f.points[p], s.origin, s.radius, s.axis[1], s.color);
            row[36..40].copy_from_slice(&pack_unit_vec(f.points[p].normal).to_le_bytes());
            row[40..44].copy_from_slice(&pack_unit_vec(s.axis[1]).to_le_bytes());
            let at = (vbase + p) * STRIDE;
            g.vertices[at..at + STRIDE].copy_from_slice(&row);
        }
        for t in 0..SLOT_TRIS {
            let tri = if t < f.used_tris as usize {
                f.tris[t].indices.map(|i| (vbase + usize::from(i)) as u16)
            } else {
                [0; 3]
            };
            g.indices[ibase + 3 * t..ibase + 3 * t + 3].copy_from_slice(&tri);
        }
        g.slots[slot] = Slot { born_ms: now_ms, life_ms: s.life_ms, live: true, step: FADE_STEPS, alpha: (s.color >> 24) as u8, origin: s.origin, seq };
        r.write_dynamic(g.mesh, (vbase * STRIDE) as u64, &g.vertices[vbase * STRIDE..(vbase + SLOT_POINTS) * STRIDE], ibase as u64, &g.indices[ibase..ibase + SLOT_INDICES]);
        self.made += 1;
        true
    }

    /// Fades and removes the marks whose life is running out (rewrites only
    /// the slots whose alpha step changed).
    pub fn update(&mut self, r: &Renderer, now_ms: u64) {
        for g in &mut self.rings {
            for slot in 0..SLOTS {
                let s = g.slots[slot];
                if !s.live || s.life_ms == 0 {
                    continue;
                }
                let step = fade_step(now_ms.saturating_sub(s.born_ms), s.life_ms);
                if step == s.step {
                    continue;
                }
                let (vbase, ibase) = (slot * SLOT_POINTS, slot * SLOT_INDICES);
                if step == 0 {
                    g.indices[ibase..ibase + SLOT_INDICES].fill(0);
                    g.slots[slot].live = false;
                    r.write_dynamic(g.mesh, 0, &[], ibase as u64, &g.indices[ibase..ibase + SLOT_INDICES]);
                } else {
                    let a = (u32::from(s.alpha) * step / FADE_STEPS) as u8;
                    for v in 0..SLOT_POINTS {
                        g.vertices[(vbase + v) * STRIDE + 19] = a;
                    }
                    g.slots[slot].step = step;
                    r.write_dynamic(g.mesh, (vbase * STRIDE) as u64, &g.vertices[vbase * STRIDE..(vbase + SLOT_POINTS) * STRIDE], 0, &[]);
                }
            }
        }
    }
}

/// Removes a mark at once (its triangles degenerate).
fn kill(r: &Renderer, g: &mut Ring, slot: usize) {
    let ibase = slot * SLOT_INDICES;
    g.indices[ibase..ibase + SLOT_INDICES].fill(0);
    g.slots[slot].live = false;
    r.write_dynamic(g.mesh, 0, &[], ibase as u64, &g.indices[ibase..ibase + SLOT_INDICES]);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn packed_vectors_round_trip() {
        for n in [[0.0, 0.0, 1.0], [1.0, 0.0, 0.0], [-0.6, 0.0, 0.8]] {
            let back = fastfile_iw6::map::unpack_unit_vec(pack_unit_vec(n));
            for k in 0..3 {
                assert!((back[k] - n[k]).abs() < 0.003, "{n:?} -> {back:?}");
            }
        }
    }

    #[test]
    fn fade_runs_down_in_the_last_second() {
        assert_eq!(fade_step(0, 5000), FADE_STEPS);
        assert_eq!(fade_step(3999, 5000), FADE_STEPS);
        assert_eq!(fade_step(4000, 5000), FADE_STEPS);
        assert_eq!(fade_step(4500, 5000), 4);
        assert_eq!(fade_step(4999, 5000), 1);
        assert_eq!(fade_step(5000, 5000), 0);
        // No life: never fades.
        assert_eq!(fade_step(1 << 40, 0), FADE_STEPS);
    }

    /// A 100 x 100 floor of two triangles, wound as the world's (the face
    /// normal `(v2 - v0) x (v1 - v0)` points up).
    fn floor() -> (Vec<u8>, Vec<u16>) {
        let corners = [[-50.0f32, -50.0, 0.0], [50.0, -50.0, 0.0], [50.0, 50.0, 0.0], [-50.0, 50.0, 0.0]];
        let mut b = Vec::new();
        for c in corners {
            let mut v = [0u8; STRIDE];
            for k in 0..3 {
                v[4 * k..4 * k + 4].copy_from_slice(&c[k].to_le_bytes());
            }
            // lightmap (0.25, 0.5), normal up
            v[28..32].copy_from_slice(&0.25f32.to_le_bytes());
            v[32..36].copy_from_slice(&0.5f32.to_le_bytes());
            v[36..40].copy_from_slice(&pack_unit_vec([0.0, 0.0, 1.0]).to_le_bytes());
            b.extend_from_slice(&v);
        }
        // (v2 - v0) x (v1 - v0) must point up: 0, 2, 1 and 0, 3, 2 are the
        // winding that does.
        (b, vec![0, 2, 1, 0, 3, 2])
    }

    #[test]
    fn a_mark_is_cut_from_the_floor_under_it() {
        let (bytes, indices) = floor();
        let meshes = vec![MeshData { kind: VertexKind::World, vertex_bytes: bytes, indices, layer_bytes: Vec::new() }];
        let draws = vec![Draw {
            mesh: 0,
            object: None,
            material: 0,
            lightmap: 3,
            probe: 1,
            first_index: 0,
            index_count: 6,
            base_vertex: 0,
            layer: None,
            sun: true,
            light: None,
            list: List::Opaque,
            range: None,
            cell: None,
            casts: [true, true],
        }];
        let bounds = vec![Some((Vec3::new(-50.0, -50.0, 0.0), Vec3::new(50.0, 50.0, 0.0)))];
        let axis = [[0.0, 0.0, 1.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]];
        let mut f = Fragment::default();
        assert!(project(&draws, &meshes, &bounds, [10.0, 0.0, 0.0], axis, 8.0, &mut f));
        assert!(f.used_tris >= 2 && f.used_points >= 4);
        // Every point lies in the mark's 16 x 16 square on the floor, with the surface's lightmap coordinates and normal.
        for p in &f.points[..f.used_points as usize] {
            assert!((p.xyz[0] - 10.0).abs() <= 8.5 && p.xyz[1].abs() <= 8.5 && p.xyz[2].abs() < 0.01, "{:?}", p.xyz);
            assert!((p.lmap_coord[0] - 0.25).abs() < 1e-4 && (p.lmap_coord[1] - 0.5).abs() < 1e-4);
            assert!(p.normal[2] > 0.9);
        }
        // A mark facing the other way (its normal down) cuts nothing from a floor that faces up.
        let down = [[0.0, 0.0, -1.0], [1.0, 0.0, 0.0], [0.0, -1.0, 0.0]];
        assert!(!project(&draws, &meshes, &bounds, [10.0, 0.0, 0.0], down, 8.0, &mut f));
        // Out of reach of the floor.
        assert!(!project(&draws, &meshes, &bounds, [500.0, 0.0, 0.0], axis, 8.0, &mut f));
    }
}
