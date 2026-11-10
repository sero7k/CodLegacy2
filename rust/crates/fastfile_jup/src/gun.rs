//! A gun's part models and the gun they make.
//!
//! A Modern Warfare III gun is built from parts at run time: a receiver
//! and the attachments it carries (barrel, magazine, grip, ...), each part
//! its own model in a `_tr` zone of its own (`mp_av_*` the view model's,
//! `mp_aw_*` the world model's): the mesh (`xmodelsurfs`, [`crate::surfs`])
//! then the model (`xmodel`) and its skin variants.
//!
//! The model, after its mesh:
//!
//! ```text
//! 0x00 u64 name hash, ptr name (-2), u16 surface count, u16 LOD count
//! ...  the name (NUL-terminated, plain), the LOD records
//! materials: one pointer per surface, -1/-2 for a reference stub that
//!   follows the pointers (0x78 bytes: the material's name hash with the
//!   top bit set), else a pointer back to a material loaded before
//! bones: per bone 32 bytes, its pose in the model (f32 quaternion x y z
//!   w, f32 position, f32 2.0), then per bone the FNV-1a 32 hash of its
//!   name (`j_gun`, `tag_barrel_attach`, `tag_flash`, ...)
//! ```
//!
//! Mesh positions are in the model's space (every bone's vertices where
//! the bone's pose puts them). A part's model is in its own space, its
//! root at the origin; the gun places it on the receiver's tag of its
//! kind (`tag_barrel_attach`, `tag_mag_attach`, ...). Checked on the
//! Renetti's receiver, slide, magazine and grip: placed so, they make
//! the pistol whole.
#![forbid(unsafe_code)]

use crate::ptr::{FOLLOW, INSERT, MINUS3, Ptr};

use crate::{frame, surfs, zone};

/// A bone name's hash as the models store it: 32-bit FNV-1a over the
/// lower-cased name.
#[must_use]
pub fn bone_hash(name: &str) -> u32 {
    let mut h: u32 = 0x811c_9dc5;
    for b in name.bytes() {
        h = (h ^ u32::from(b.to_ascii_lowercase())).wrapping_mul(0x0100_0193);
    }
    h
}

/// A bone of a part model: its name's hash and its pose in the model.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Bone {
    pub name: u32,
    /// Quaternion (x, y, z, w).
    pub rot: [f32; 4],
    pub pos: [f32; 3],
}

impl Bone {
    /// A point of the bone's space in the model's.
    #[must_use]
    pub fn apply(&self, v: [f32; 3]) -> [f32; 3] {
        let r = rotate(self.rot, v);
        [r[0] + self.pos[0], r[1] + self.pos[1], r[2] + self.pos[2]]
    }

    /// This bone's pose seen from `parent` placed by `self` (`self * child`).
    #[must_use]
    pub fn then(&self, child: &Bone) -> Bone {
        Bone {
            name: child.name,
            rot: quat_mul(self.rot, child.rot),
            pos: self.apply(child.pos),
        }
    }

    /// The pose that undoes this one (`inverse().apply(apply(v)) == v`).
    #[must_use]
    pub fn inverse(&self) -> Bone {
        let q = [-self.rot[0], -self.rot[1], -self.rot[2], self.rot[3]];
        let p = rotate(q, self.pos);
        Bone {
            name: self.name,
            rot: q,
            pos: [-p[0], -p[1], -p[2]],
        }
    }
}

/// `v` rotated by the unit quaternion `q` (x, y, z, w).
#[must_use]
pub fn rotate(q: [f32; 4], v: [f32; 3]) -> [f32; 3] {
    let u = [q[0], q[1], q[2]];
    let w = q[3];
    let cross = |a: [f32; 3], b: [f32; 3]| {
        [
            a[1] * b[2] - a[2] * b[1],
            a[2] * b[0] - a[0] * b[2],
            a[0] * b[1] - a[1] * b[0],
        ]
    };
    let t = cross(u, v).map(|c| 2.0 * c);
    let c2 = cross(u, t);
    [
        v[0] + w * t[0] + c2[0],
        v[1] + w * t[1] + c2[1],
        v[2] + w * t[2] + c2[2],
    ]
}

fn quat_mul(a: [f32; 4], b: [f32; 4]) -> [f32; 4] {
    let [ax, ay, az, aw] = a;
    let [bx, by, bz, bw] = b;
    [
        aw * bx + ax * bw + ay * bz - az * by,
        aw * by - ax * bz + ay * bw + az * bx,
        aw * bz + ax * by - ay * bx + az * bw,
        aw * bw - ax * bx - ay * by - az * bz,
    ]
}

/// One surface of a part, decoded.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PartSurface {
    pub positions: Vec<[f32; 3]>,
    pub normals: Vec<[f32; 3]>,
    pub tangents: Vec<[f32; 3]>,
    pub uvs: Vec<[f32; 2]>,
    pub colours: Vec<[u8; 4]>,
    /// Triangle list (indices into the vertices).
    pub indices: Vec<u32>,
    /// Its material's name hash (`None`: a material the model points back
    /// to, loaded before it).
    pub material: Option<u64>,
}

/// A part model read from its zone.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PartModel {
    pub name: String,
    pub hash: u64,
    pub bones: Vec<Bone>,
    pub surfaces: Vec<PartSurface>,
}

impl PartModel {
    /// The bone named `name`.
    #[must_use]
    pub fn bone(&self, name: &str) -> Option<&Bone> {
        let h = bone_hash(name);
        self.bones.iter().find(|b| b.name == h)
    }

    /// The part moved by `at` (vertices, normals, tangents and bones).
    #[must_use]
    pub fn placed(&self, at: &Bone) -> PartModel {
        let mut out = self.clone();
        for s in &mut out.surfaces {
            for p in &mut s.positions {
                *p = at.apply(*p);
            }
            for n in s.normals.iter_mut().chain(s.tangents.iter_mut()) {
                *n = rotate(at.rot, *n);
            }
        }
        for b in &mut out.bones {
            *b = at.then(b);
        }
        out
    }
}

fn u64_at(z: &[u8], o: usize) -> u64 {
    z.get(o..o + 8)
        .map_or(0, |b| u64::from_le_bytes(b.try_into().unwrap_or([0; 8])))
}

fn u32_at(z: &[u8], o: usize) -> u32 {
    z.get(o..o + 4)
        .map_or(0, |b| u32::from_le_bytes(b.try_into().unwrap_or([0; 4])))
}

fn f32_at(z: &[u8], o: usize) -> f32 {
    f32::from_bits(u32_at(z, o))
}

/// The model's header after its mesh: a hash, `-2`, its surface count
/// (all its LODs'), its LOD count, and at +0x24 the word every model
/// header has (`c0 b6 73 2c`, then 1.0). A part zone of one LOD (a view
/// model's) names a surface count its mesh has; a world model's or a
/// character's names its LODs' together.
fn model_header(z: &[u8], from: usize, surfaces: &[usize]) -> Option<usize> {
    let end = from.saturating_add(0x40000).min(z.len().saturating_sub(0x30));
    let is_header = |o: usize| {
        let h = u64_at(z, o);
        h != 0 && !crate::hash::is_ref(h) && u64_at(z, o + 8) == FOLLOW
    };
    let marked = |o: usize| z[o + 0x24..o + 0x2c] == [0xc0, 0xb6, 0x73, 0x2c, 0, 0, 0x80, 0x3f];
    let count = |o: usize| usize::from(u16::from_le_bytes([z[o + 0x10], z[o + 0x11]]));
    (from..end)
        .find(|&o| is_header(o) && surfaces.contains(&count(o)))
        .or_else(|| (from..end).find(|&o| is_header(o) && marked(o) && (1..=8).contains(&z[o + 0x12])))
}

/// Each LOD's (first surface, surface count) in the model's surface list:
/// `lods` records of 0x48 bytes after its name (+0x1c the count, +0x1e
/// the first), each starting where the one before ends, `total` in all.
fn lod_ranges(z: &[u8], from: usize, lods: usize, total: usize) -> Option<Vec<(usize, usize)>> {
    const LOD: usize = 0x48;
    let u16_at = |o: usize| usize::from(u16::from_le_bytes([z[o], z[o + 1]]));
    let end = from.saturating_add(0x2000).min(z.len().saturating_sub(LOD * lods));
    (from..end).find_map(|p| {
        let mut out = Vec::with_capacity(lods);
        let mut first = 0usize;
        for l in 0..lods {
            let r = p + LOD * l;
            let n = u16_at(r + 0x1c);
            if u16_at(r + 0x1e) != first || n == 0 {
                return None;
            }
            out.push((first, n));
            first += n;
        }
        (first == total).then_some(out)
    })
}

/// The model's name: the first plain string after its header.
fn model_name(z: &[u8], from: usize) -> Option<(String, usize)> {
    let end = from.saturating_add(0x400).min(z.len());
    let mut o = from;
    while o < end {
        let run = z[o..end]
            .iter()
            .take_while(|b| b.is_ascii_graphic())
            .count();
        if run >= 6 && z.get(o + run) == Some(&0) {
            let s = String::from_utf8_lossy(&z[o..o + run]).into_owned();
            return Some((s, o + run + 1));
        }
        o += run.max(1);
    }
    None
}

/// Per surface its material (see the module doc): where the pointers are
/// and what they give. The first place after `from` where `surfaces`
/// pointers of the material forms stand, followed by a reference stub per
/// inline one.
fn materials(z: &[u8], from: usize, to: usize, surfaces: usize) -> Option<Vec<Option<u64>>> {
    const STUB: usize = 0x78;
    let to = to.min(z.len());
    (from..to).find_map(|o| {
        let mut inline = 0;
        for k in 0..surfaces {
            match Ptr::from_raw(u64_at(z, o + 8 * k)) {
                Ptr::Follow | Ptr::Insert => inline += 1,
                Ptr::Back {
                    block: 0x0f | 0x1f, ..
                } => {}
                _ => return None,
            }
        }
        if inline == 0 {
            return None;
        }
        let stubs = o + 8 * surfaces;
        let mut hashes = Vec::with_capacity(inline);
        for k in 0..inline {
            let h = u64_at(z, stubs + STUB * k);
            if !crate::hash::is_ref(h)
                || h >= MINUS3
                || crate::hash::strip(h).count_ones() < 12
                || u64_at(z, stubs + STUB * k + 8) != 0
            {
                return None;
            }
            hashes.push(crate::hash::strip(h));
        }
        let mut next = hashes.into_iter();
        Some(
            (0..surfaces)
                .map(|k| match u64_at(z, o + 8 * k) {
                    FOLLOW | INSERT => next.next(),
                    _ => None,
                })
                .collect(),
        )
    })
}

/// The bones: the longest run of poses (unit quaternion, position, 2.0)
/// after `from`, and the name hashes after it.
fn bones(z: &[u8], from: usize) -> Vec<Bone> {
    let pose = |o: usize| {
        if o + 32 > z.len() || f32_at(z, o + 28) != 2.0 {
            return false;
        }
        let q: [f32; 4] = std::array::from_fn(|k| f32_at(z, o + 4 * k));
        let n: f32 = q.iter().map(|v| v * v).sum();
        (n - 1.0).abs() < 1e-3 && (0..3).all(|k| f32_at(z, o + 16 + 4 * k).abs() < 1000.0)
    };
    let mut best = (0usize, 0usize);
    let mut o = from;
    while o + 32 <= z.len() {
        if pose(o) {
            let mut n = 1;
            while pose(o + 32 * n) {
                n += 1;
            }
            if n > best.1 {
                best = (o, n);
            }
            o += 32 * n;
        } else {
            o += 1;
        }
    }
    let (at, n) = best;
    let names = at + 32 * n;
    (0..n)
        .map(|i| {
            let r = at + 32 * i;
            Bone {
                name: u32_at(z, names + 4 * i),
                rot: std::array::from_fn(|k| f32_at(z, r + 4 * k)),
                pos: std::array::from_fn(|k| f32_at(z, r + 16 + 4 * k)),
            }
        })
        .collect()
}

/// Read the part model of a part zone (its first mesh and the model after
/// it).
pub fn read_part(z: &[u8]) -> Result<PartModel, String> {
    read_model(z).map(|(m, _)| m)
}

/// [`read_part`], and the mesh its surfaces are LOD 0 of.
pub fn read_model(z: &[u8]) -> Result<(PartModel, surfs::SurfsRec), String> {
    let h = zone::parse_header(z).map_err(|e| e.to_string())?;
    if h.assets.first().map(|a| a.kind) != Some(zone::asset_type::XMODEL_SURFS) {
        return Err("no mesh first".into());
    }
    // (A world model's zone holds more than one mesh: its LODs, read one
    // after another; the model is drawn with its densest of the surface
    // count it names.)
    let mut meshes = Vec::new();
    let mut at = h.data_start;
    for _ in h
        .assets
        .iter()
        .take_while(|a| a.kind == zone::asset_type::XMODEL_SURFS)
    {
        let rec = surfs::parse(z, at).map_err(|e| e.to_string())?;
        at = rec.end;
        meshes.push(rec);
    }
    let counts: Vec<usize> = meshes.iter().map(|m| m.surfaces.len()).collect();
    let header = model_header(z, at, &counts).ok_or("no model after the mesh")?;
    let total = usize::from(u16::from_le_bytes([z[header + 0x10], z[header + 0x11]]));
    let lods = usize::from(z[header + 0x12]).max(1);
    // (The model's name is inline after the header: plain, or empty.)
    let (name, after_name) = model_name(z, header + 0x18)
        .filter(|(_, e)| *e <= header + 0x200)
        .unwrap_or_else(|| (format!("#{:016x}", u64_at(z, header)), header + 0xe8));
    // LOD 0: the densest mesh of its surface count; its materials are the
    // first of the model's list.
    let lod0 = if lods > 1 {
        lod_ranges(z, after_name, lods, total).and_then(|r| r.first().copied())
    } else {
        None
    };
    let dense = |m: &surfs::SurfsRec| m.surfaces.iter().map(|s| s.vert_count).sum::<u32>();
    let n = lod0.map_or(total, |(_, n)| n);
    // (Else, where the zone's meshes name no count the model has: its
    // densest mesh, LOD 0 standing first in the model's lists.)
    let rec = match meshes.iter().filter(|m| m.surfaces.len() == n).max_by_key(|m| dense(m)) {
        Some(m) => m.clone(),
        None if lods > 1 => meshes.iter().max_by_key(|m| dense(m)).cloned().ok_or("no mesh")?,
        None => return Err("no mesh of the model's surface count".into()),
    };
    let n = rec.surfaces.len();
    let bones = bones(z, after_name);
    let mut mats = materials(z, after_name, z.len(), total).unwrap_or_else(|| vec![None; total]);
    mats.truncate(n);
    mats.resize(n, None);
    let mut surfaces = Vec::with_capacity(n);
    for (i, material) in mats.into_iter().enumerate() {
        let g = surfs::geometry(z, &rec, i).map_err(|e| format!("surface {i}: {e}"))?;
        let (normals, tangents): (Vec<_>, Vec<_>) = g
            .tangent_frames
            .iter()
            .map(|&w| frame::normal_tangent(w))
            .unzip();
        surfaces.push(PartSurface {
            normals: if normals.len() == g.positions.len() {
                normals
            } else {
                vec![[0.0, 0.0, 1.0]; g.positions.len()]
            },
            tangents: if tangents.len() == g.positions.len() {
                tangents
            } else {
                vec![[1.0, 0.0, 0.0]; g.positions.len()]
            },
            uvs: if g.uvs.len() == g.positions.len() {
                g.uvs
            } else {
                vec![[0.0; 2]; g.positions.len()]
            },
            colours: g.colours,
            positions: g.positions,
            indices: g.indices,
            material,
        });
    }
    Ok((
        PartModel {
            name,
            hash: u64_at(z, header),
            bones,
            surfaces,
        },
        rec,
    ))
}

/// A gun put together: every part's surfaces in the receiver's space and
/// the bones of all of them (a part's moved with it).
#[derive(Clone, Debug, Default)]
pub struct Gun {
    pub surfaces: Vec<PartSurface>,
    pub bones: Vec<Bone>,
    /// What could not be placed.
    pub missing: Vec<String>,
    /// How many of the surfaces (the first) are the receiver's.
    pub receiver_surfaces: usize,
}

impl Gun {
    /// The first bone named `name`.
    #[must_use]
    pub fn bone(&self, name: &str) -> Option<&Bone> {
        let h = bone_hash(name);
        self.bones.iter().find(|b| b.name == h)
    }
}

/// The receiver with each part on the receiver's tag named with it.
#[must_use]
pub fn assemble(receiver: &PartModel, parts: &[(&PartModel, &str)]) -> Gun {
    let mut gun = Gun {
        surfaces: receiver.surfaces.clone(),
        bones: receiver.bones.clone(),
        missing: Vec::new(),
        receiver_surfaces: receiver.surfaces.len(),
    };
    for (part, tag) in parts {
        // A part goes on the bone named as its own root (`tag_scope`, a
        // bolt's joint), on the receiver or a part already placed: its root
        // onto that bone, wherever the root stands in the part's own model
        // (a scope's `tag_scope` is its eye line, above its mount). Else its
        // origin on the tag it was listed with.
        let root = part.bones.first();
        let by_root = root.and_then(|r| {
            gun.bones
                .iter()
                .find(|b| b.name == r.name)
                .map(|at| at.then(&r.inverse()))
        });
        let Some(at) = by_root.or_else(|| gun.bone(tag).copied()) else {
            gun.missing
                .push(format!("{}: no {tag} on {}", part.name, receiver.name));
            continue;
        };
        let placed = part.placed(&Bone { name: 0, ..at });
        gun.surfaces.extend(placed.surfaces);
        gun.bones.extend(placed.bones);
    }
    gun
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bone_names_hash_as_stored() {
        // (The Renetti's receiver stores these.)
        assert_eq!(bone_hash("j_gun"), bone_hash("J_GUN"));
        assert_eq!(bone_hash(""), 0x811c_9dc5);
        assert_eq!(bone_hash("a"), 0xe40c_292c);
    }

    #[test]
    fn a_bone_places_a_point() {
        // A quarter turn about z, then 1 along x.
        let s = std::f32::consts::FRAC_1_SQRT_2;
        let b = Bone {
            name: 0,
            rot: [0.0, 0.0, s, s],
            pos: [1.0, 0.0, 0.0],
        };
        let p = b.apply([1.0, 0.0, 0.0]);
        assert!((p[0] - 1.0).abs() < 1e-5 && (p[1] - 1.0).abs() < 1e-5);
        let c = b.then(&Bone {
            name: 7,
            rot: [0.0, 0.0, 0.0, 1.0],
            pos: [0.0, 1.0, 0.0],
        });
        assert_eq!(c.name, 7);
        assert!((c.pos[0] - 0.0).abs() < 1e-5 && (c.pos[1] - 0.0).abs() < 1e-5);
    }

    fn part(name: &str, bones: Vec<Bone>, points: Vec<[f32; 3]>) -> PartModel {
        PartModel {
            name: name.into(),
            bones,
            surfaces: vec![PartSurface {
                normals: vec![[0.0, 0.0, 1.0]; points.len()],
                tangents: vec![[1.0, 0.0, 0.0]; points.len()],
                uvs: vec![[0.0; 2]; points.len()],
                positions: points,
                ..PartSurface::default()
            }],
            ..PartModel::default()
        }
    }

    fn close(a: [f32; 3], b: [f32; 3]) -> bool {
        (0..3).all(|k| (a[k] - b[k]).abs() < 1e-4)
    }

    #[test]
    fn a_pose_and_its_inverse_cancel() {
        let s = std::f32::consts::FRAC_1_SQRT_2;
        let b = Bone {
            name: 3,
            rot: [0.0, s, 0.0, s],
            pos: [1.0, 2.0, 3.0],
        };
        let v = [0.5, -4.0, 7.0];
        assert!(close(b.inverse().apply(b.apply(v)), v));
        assert!(close(b.then(&b.inverse()).pos, [0.0; 3]));
    }

    #[test]
    fn a_part_s_root_goes_on_the_bone_of_its_name_wherever_it_stands_in_the_part() {
        let id = [0.0, 0.0, 0.0, 1.0];
        let scope = bone_hash("tag_scope");
        // The receiver's sight line is 3 above its top (z 2); the scope's
        // own `tag_scope` (its eye line) is 1.5 above its mount (z 0).
        let rec = part(
            "rec",
            vec![
                Bone { name: bone_hash("j_gun"), rot: id, pos: [0.0; 3] },
                Bone { name: scope, rot: id, pos: [4.0, 0.0, 5.0] },
            ],
            vec![[0.0, 0.0, 2.0]],
        );
        let optic = part(
            "scope",
            vec![Bone { name: scope, rot: id, pos: [0.0, 0.0, 1.5] }],
            vec![[0.0, 0.0, 0.0], [0.0, 0.0, 3.0]],
        );
        let gun = assemble(&rec, &[(&optic, "tag_scope_attach")]);
        assert!(gun.missing.is_empty());
        // Its eye line on the receiver's: the mount at 3.5, not 5.
        assert!(close(gun.surfaces[1].positions[0], [4.0, 0.0, 3.5]));
        assert!(close(gun.surfaces[1].positions[1], [4.0, 0.0, 6.5]));
        assert!(close(gun.bones[2].pos, [4.0, 0.0, 5.0]));
        // A part whose root is its origin hangs off the bone as it is.
        let bolt = part(
            "bolt",
            vec![Bone { name: scope, rot: id, pos: [0.0; 3] }],
            vec![[1.0, 0.0, 0.0]],
        );
        let gun = assemble(&rec, &[(&bolt, "tag_bolt_attach")]);
        assert!(close(gun.surfaces[1].positions[0], [5.0, 0.0, 5.0]));
    }

    #[test]
    fn material_pointers_and_their_stubs() {
        // Three surfaces: a stub, a pointer back, a stub.
        let mut z = vec![0u8; 4];
        z.extend(INSERT.to_le_bytes());
        z.extend(((0x1fu64 << 32) | 0x759).to_le_bytes());
        z.extend(FOLLOW.to_le_bytes());
        for h in [0x2adf_d910_d444_ce58u64, 0x3f79_5055_6986_b3a6] {
            let mut stub = vec![0u8; 0x78];
            stub[..8].copy_from_slice(&(h | crate::hash::REF_BIT).to_le_bytes());
            z.extend(stub);
        }
        let m = materials(&z, 0, z.len(), 3).unwrap();
        assert_eq!(
            m,
            vec![
                Some(0x2adf_d910_d444_ce58),
                None,
                Some(0x3f79_5055_6986_b3a6)
            ]
        );
    }
}
