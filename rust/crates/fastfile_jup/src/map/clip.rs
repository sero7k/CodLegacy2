//! A map's collision: the zone's `col_map` as convex brushes and triangles
//! in the game's inches.
//!
//! The world's collision is Havok shapes ([`super::havok`]): per 256 m
//! tile a mesh shape (the ground and every drawn surface's collision, as
//! triangles) and a compound shape (the solid things: convex hulls placed
//! by instances). Havok's metres are 32 of the game's units (a 256 m tile
//! is 8,192 units; read at 39.3701 a map's shapes stand a fifth too large
//! about the origin and hold no floor under its spawn points), axes as they
//! are. A mesh's triangles are wound the other way from the game's own
//! (the game's player trace only meets a triangle from the side its winding
//! faces), so they are turned round here.
//!
//! What this does not carry: which instance or triangle is player clip,
//! bullet clip, water or a ladder (each shape has a tag that indexes a
//! table of the map's surface kinds; the table is not read), so every
//! shape reads as solid.

use super::havok::{self, Instance};

/// Metres to the game's units (a tile of 256 m is 8,192).
pub const UNITS_PER_METRE: f32 = 32.0;

/// A convex solid: planes (`n . x = d` on the face, `n . x < d` inside).
#[derive(Clone, Debug, Default)]
pub struct ClipBrush {
    pub planes: Vec<[f32; 4]>,
    pub mins: [f32; 3],
    pub maxs: [f32; 3],
    pub shape_tag: u16,
}

/// A map's collision.
#[derive(Clone, Debug, Default)]
pub struct WorldClip {
    pub brushes: Vec<ClipBrush>,
    pub tris: Vec<[[f32; 3]; 3]>,
    /// Tagfiles read: (meshes, compounds).
    pub shapes: (usize, usize),
    /// Hulls given up on (no usable face).
    pub bad_hulls: usize,
    pub mins: [f32; 3],
    pub maxs: [f32; 3],
}

fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

/// The brush a placed hull makes: its faces' planes (equal ones once),
/// each with its normal out of the hull.
pub fn brush_of(inst: &Instance) -> Option<ClipBrush> {
    let v: Vec<[f32; 3]> = inst.placed().into_iter().map(|p| p.map(|c| c * UNITS_PER_METRE)).collect();
    if v.len() < 4 || v.iter().flatten().any(|c| !c.is_finite()) {
        return None;
    }
    let mut mins = [f32::MAX; 3];
    let mut maxs = [f32::MIN; 3];
    let mut mid = [0.0f32; 3];
    for p in &v {
        for k in 0..3 {
            mins[k] = mins[k].min(p[k]);
            maxs[k] = maxs[k].max(p[k]);
            mid[k] += p[k] / v.len() as f32;
        }
    }
    let mut planes: Vec<[f32; 4]> = Vec::new();
    for face in &inst.hull.faces {
        // (Newell's normal: right for any polygon, flat or not.)
        let mut n = [0.0f32; 3];
        let mut c = [0.0f32; 3];
        let mut count = 0.0;
        for (i, &a) in face.iter().enumerate() {
            let b = face[(i + 1) % face.len()];
            let (Some(&p), Some(&q)) = (v.get(usize::from(a)), v.get(usize::from(b))) else {
                n = [0.0; 3];
                break;
            };
            n[0] += (p[1] - q[1]) * (p[2] + q[2]);
            n[1] += (p[2] - q[2]) * (p[0] + q[0]);
            n[2] += (p[0] - q[0]) * (p[1] + q[1]);
            for k in 0..3 {
                c[k] += p[k];
            }
            count += 1.0;
        }
        let len = dot(n, n).sqrt();
        if len < 1e-6 || count < 3.0 {
            continue;
        }
        let mut n = n.map(|x| x / len);
        let c = c.map(|x| x / count);
        let mut d = dot(n, c);
        // Out of the hull: the middle of the hull is behind the face.
        if dot(n, mid) > d {
            n = n.map(|x| -x);
            d = -d;
        }
        if planes.iter().any(|p| dot([p[0], p[1], p[2]], n) > 0.9999 && (p[3] - d).abs() < 0.05) {
            continue;
        }
        planes.push([n[0], n[1], n[2], d]);
    }
    if planes.len() < 4 {
        return None;
    }
    // The box's six planes first (+x, -x, +y, -y, +z, -z): the collision
    // builder indexes brushes by them.
    let mut all = vec![
        [1.0, 0.0, 0.0, maxs[0]],
        [-1.0, 0.0, 0.0, -mins[0]],
        [0.0, 1.0, 0.0, maxs[1]],
        [0.0, -1.0, 0.0, -mins[1]],
        [0.0, 0.0, 1.0, maxs[2]],
        [0.0, 0.0, -1.0, -mins[2]],
    ];
    // (A plane the box already has is not repeated.)
    for p in planes {
        if !all[..6].iter().any(|b| dot([b[0], b[1], b[2]], [p[0], p[1], p[2]]) > 0.9999 && (b[3] - p[3]).abs() < 0.05) {
            all.push(p);
        }
    }
    Some(ClipBrush { planes: all, mins, maxs, shape_tag: inst.shape_tag })
}

/// Reads the map's collision from its zone image: every shape tagfile.
/// `None` when the zone has none.
pub fn read(z: &[u8]) -> Option<WorldClip> {
    let mut out = WorldClip::default();
    for (_, t) in havok::shape_tagfiles(z) {
        match t.root_type() {
            "hknpMeshShape" => {
                out.shapes.0 += 1;
                out.tris.extend(havok::mesh_triangles(&t).into_iter().map(|tri| {
                    let [a, b, c] = tri.map(|p| p.map(|c| c * UNITS_PER_METRE));
                    // (Turned round: see the module's note on winding.)
                    [a, c, b]
                }));
            }
            _ => {
                out.shapes.1 += 1;
                for inst in havok::compound_instances(&t) {
                    match brush_of(&inst) {
                        Some(b) => out.brushes.push(b),
                        None => out.bad_hulls += 1,
                    }
                }
            }
        }
    }
    if out.tris.is_empty() && out.brushes.is_empty() {
        return None;
    }
    let mut mins = [f32::MAX; 3];
    let mut maxs = [f32::MIN; 3];
    for b in &out.brushes {
        for k in 0..3 {
            mins[k] = mins[k].min(b.mins[k]);
            maxs[k] = maxs[k].max(b.maxs[k]);
        }
    }
    for p in out.tris.iter().flatten() {
        for k in 0..3 {
            mins[k] = mins[k].min(p[k]);
            maxs[k] = maxs[k].max(p[k]);
        }
    }
    out.mins = mins;
    out.maxs = maxs;
    Some(out)
}
