//! Havok tagfiles: the container the map's collision shapes come in.
//!
//! Modern Warfare III keeps a map's collision (the zone's `col_map`) as
//! Havok 2021 `hknp` shapes, each in a tagfile of its own inside the zone:
//! a mesh shape (the terrain and the drawn-surface collision, quantised
//! triangles) and a compound shape (instances of convex hulls) for every
//! 256 m tile. This reads just what those two need: the tagfile's sections
//! (big-endian `size` + four-letter name, nested), its type names (for the
//! root object's type) and its item table (an object's place in the data).
//!
//! Everything is metres and Havok's axes (the same axes as the game's).

use super::index::{f32_at, find_all, u16_at, u32_at};

/// One entry of a tagfile's item table: an object or array in the data.
#[derive(Clone, Copy, Debug, Default)]
pub struct Item {
    pub type_index: u32,
    pub offset: usize,
    pub count: usize,
}

/// A parsed tagfile: its data and item table.
pub struct Tagfile<'a> {
    pub data: &'a [u8],
    /// Item 0 is null; item 1 is the root object.
    pub items: Vec<Item>,
    type_names: Vec<String>,
}

/// A packed integer of the type section (1 to 4 bytes).
fn packed(b: &[u8], o: &mut usize) -> Option<usize> {
    let first = *b.get(*o)?;
    *o += 1;
    let rest = |o: &mut usize, n: usize| -> Option<usize> {
        let mut v = 0usize;
        for _ in 0..n {
            v = (v << 8) | usize::from(*b.get(*o)?);
            *o += 1;
        }
        Some(v)
    };
    match first {
        0..=0x7F => Some(usize::from(first)),
        0x80..=0xBF => Some((usize::from(first & 0x3F) << 8) | rest(o, 1)?),
        0xC0..=0xDF => Some((usize::from(first & 0x1F) << 16) | rest(o, 2)?),
        0xE0..=0xEF => Some((usize::from(first & 0x0F) << 24) | rest(o, 3)?),
        _ => None,
    }
}

/// The sections between `from` and `to`: name and byte range of each
/// (`size` includes its 8 bytes of header).
fn sections(z: &[u8], from: usize, to: usize) -> Vec<([u8; 4], usize, usize)> {
    let mut out = Vec::new();
    let mut o = from;
    while o + 8 <= to {
        let size = (u32::from_be_bytes([z[o], z[o + 1], z[o + 2], z[o + 3]]) & 0x3FFF_FFFF) as usize;
        if size < 8 || o + size > to {
            break;
        }
        out.push(([z[o + 4], z[o + 5], z[o + 6], z[o + 7]], o + 8, o + size));
        o += size;
    }
    out
}

fn section(list: &[([u8; 4], usize, usize)], name: &[u8; 4]) -> Option<(usize, usize)> {
    list.iter().find(|s| &s.0 == name).map(|s| (s.1, s.2))
}

impl<'a> Tagfile<'a> {
    /// Parses the tagfile whose header (size, `TAG0`) is at `at` in `z`.
    pub fn parse(z: &'a [u8], at: usize) -> Option<Self> {
        let total = (u32::from_be_bytes(z.get(at..at + 4)?.try_into().ok()?) & 0x3FFF_FFFF) as usize;
        if z.get(at + 4..at + 8)? != b"TAG0" || total < 16 || at + total > z.len() {
            return None;
        }
        let top = sections(z, at + 8, at + total);
        let (d0, d1) = section(&top, b"DATA")?;
        let (t0, t1) = section(&top, b"TYPE")?;
        let (i0, i1) = section(&top, b"INDX")?;
        let types = sections(z, t0, t1);
        let (s0, s1) = section(&types, b"TST1")?;
        let strings: Vec<&[u8]> = z[s0..s1].split(|&b| b == 0).collect();
        let (n0, _) = section(&types, b"TNA1")?;
        let mut o = n0;
        let count = packed(z, &mut o)?;
        let mut type_names = vec![String::new()];
        for _ in 1..count.min(100_000) {
            let name = packed(z, &mut o)?;
            let params = packed(z, &mut o)?;
            for _ in 0..params {
                packed(z, &mut o)?;
                packed(z, &mut o)?;
            }
            type_names.push(String::from_utf8_lossy(strings.get(name).copied().unwrap_or(&[])).into_owned());
        }
        let index = sections(z, i0, i1);
        let (m0, m1) = section(&index, b"ITEM")?;
        let mut items = vec![Item::default()];
        let mut o = m0 + 12;
        while o + 12 <= m1 {
            items.push(Item {
                type_index: u32_at(z, o) & 0x00FF_FFFF,
                offset: u32_at(z, o + 4) as usize,
                count: u32_at(z, o + 8) as usize,
            });
            o += 12;
        }
        Some(Self { data: &z[d0..d1], items, type_names })
    }

    /// The root object's type name (`hknpMeshShape`, `hknpCompoundShape`,
    /// `HavokPhysicsShapeList`).
    pub fn root_type(&self) -> &str {
        self.items
            .get(1)
            .and_then(|i| self.type_names.get(i.type_index as usize))
            .map_or("", String::as_str)
    }

    /// The name of `item`'s type.
    pub fn type_of(&self, item: usize) -> &str {
        self.items
            .get(item)
            .and_then(|i| self.type_names.get(i.type_index as usize))
            .map_or("", String::as_str)
    }
}

/// Every tagfile in the zone image whose root is a shape (a mesh or a
/// compound), in stream order: `(position, tagfile)`.
pub fn shape_tagfiles(z: &[u8]) -> Vec<(usize, Tagfile<'_>)> {
    let mut out = Vec::new();
    for p in find_all(z, b"TAG0") {
        let Some(at) = p.checked_sub(4) else { continue };
        let Some(t) = Tagfile::parse(z, at) else { continue };
        if matches!(t.root_type(), "hknpMeshShape" | "hknpCompoundShape") {
            out.push((at, t));
        }
    }
    out
}

/// A relative array view (`i32` offset from the view's own address, `i32`
/// count) at `addr` in `d`: the array's address and length.
fn view(d: &[u8], addr: usize) -> Option<(usize, usize)> {
    let off = u32_at(d, addr) as i32;
    let n = u32_at(d, addr + 4) as i32;
    let a = usize::try_from(addr as i64 + i64::from(off)).ok()?;
    let n = usize::try_from(n).ok()?;
    (n < 1 << 24 && a <= d.len()).then_some((a, n))
}

/// A convex hull in its own space: corners and faces (corner lists, in
/// order round each face).
#[derive(Clone, Debug, Default)]
pub struct Hull {
    pub verts: Vec<[f32; 3]>,
    pub faces: Vec<Vec<u8>>,
}

/// One instance of a compound shape: a hull, placed.
#[derive(Clone, Debug)]
pub struct Instance {
    pub hull: Hull,
    /// Rotation quaternion (x, y, z, w), scale and translation: the hull's
    /// corner `v` stands at `rotate(scale * v) + translation`.
    pub rotation: [f32; 4],
    pub scale: [f32; 3],
    pub translation: [f32; 3],
    /// The instance's shape tag (an index into the map's table of surface
    /// kinds).
    pub shape_tag: u16,
}

impl Instance {
    /// The hull's corners where the instance puts them.
    pub fn placed(&self) -> Vec<[f32; 3]> {
        let m = quat_matrix(self.rotation);
        self.hull
            .verts
            .iter()
            .map(|v| {
                let s = [v[0] * self.scale[0], v[1] * self.scale[1], v[2] * self.scale[2]];
                std::array::from_fn(|r| m[r][0] * s[0] + m[r][1] * s[1] + m[r][2] * s[2] + self.translation[r])
            })
            .collect()
    }
}

/// The rotation a unit quaternion (x, y, z, w) stands for, row-major.
pub fn quat_matrix(q: [f32; 4]) -> [[f32; 3]; 3] {
    let [x, y, z, w] = q;
    [
        [1.0 - 2.0 * (y * y + z * z), 2.0 * (x * y - z * w), 2.0 * (x * z + y * w)],
        [2.0 * (x * y + z * w), 1.0 - 2.0 * (x * x + z * z), 2.0 * (y * z - x * w)],
        [2.0 * (x * z - y * w), 2.0 * (y * z + x * w), 1.0 - 2.0 * (x * x + y * y)],
    ]
}

/// A convex shape at `shape` in `d`: corners, and faces as lists of corner
/// indices.
fn hull_at(d: &[u8], shape: usize) -> Option<Hull> {
    let h = shape + 0x40;
    let (va, vn) = view(d, h)?;
    let (fa, fnum) = view(d, h + 0x10)?;
    let (ia, inn) = view(d, h + 0x18)?;
    if va + 12 * vn > d.len() || fa + 4 * fnum > d.len() || ia + inn > d.len() || vn > 256 {
        return None;
    }
    let verts = (0..vn).map(|k| [f32_at(d, va + 12 * k), f32_at(d, va + 12 * k + 4), f32_at(d, va + 12 * k + 8)]).collect();
    let mut faces = Vec::with_capacity(fnum);
    for k in 0..fnum {
        let first = usize::from(u16_at(d, fa + 4 * k));
        let num = usize::from(d[fa + 4 * k + 2]);
        if first + num <= inn && num >= 3 {
            faces.push(d[ia + first..ia + first + num].to_vec());
        }
    }
    Some(Hull { verts, faces })
}

/// The instances of a compound shape tagfile.
pub fn compound_instances(t: &Tagfile<'_>) -> Vec<Instance> {
    let d = t.data;
    let Some(list) = t.items.get(2) else { return Vec::new() };
    let mut out = Vec::new();
    for k in 0..list.count {
        let a = list.offset + 64 * k;
        if a + 64 > d.len() {
            break;
        }
        let rel = i64::from_le_bytes(d[a + 0x30..a + 0x38].try_into().unwrap_or([0; 8]));
        let Ok(shape) = usize::try_from(a as i64 + 0x30 + rel) else { continue };
        let Some(hull) = hull_at(d, shape) else { continue };
        out.push(Instance {
            hull,
            rotation: [f32_at(d, a), f32_at(d, a + 4), f32_at(d, a + 8), f32_at(d, a + 12)],
            translation: [f32_at(d, a + 0x10), f32_at(d, a + 0x14), f32_at(d, a + 0x18)],
            scale: [f32_at(d, a + 0x20), f32_at(d, a + 0x24), f32_at(d, a + 0x28)],
            shape_tag: u16_at(d, a + 0x2C),
        });
    }
    out
}

/// The triangles of a mesh shape tagfile (quantised sections: `u16`
/// corners plus a section offset, scaled; `u8` primitives, a triangle or a
/// quad).
pub fn mesh_triangles(t: &Tagfile<'_>) -> Vec<[[f32; 3]; 3]> {
    let d = t.data;
    let root = t.items.get(1).map_or(0, |i| i.offset);
    let scale = [f32_at(d, root + 0x60), f32_at(d, root + 0x64), f32_at(d, root + 0x68)];
    let Some((ga, gn)) = view(d, root + 0x90) else { return Vec::new() };
    let mut out = Vec::new();
    for k in 0..gn {
        let a = ga + 64 * k;
        if a + 64 > d.len() {
            break;
        }
        let (Some((pa, pn)), Some((qa, qn))) = (view(d, a + 8), view(d, a + 0x10)) else { continue };
        if pa + 4 * pn > d.len() || qa + 6 * qn > d.len() {
            continue;
        }
        let so = [u32_at(d, a + 0x20) as i32, u32_at(d, a + 0x24) as i32, u32_at(d, a + 0x28) as i32];
        let pos: Vec<[f32; 3]> = (0..qn)
            .map(|v| {
                std::array::from_fn(|c| (f32::from(u16_at(d, qa + 6 * v + 2 * c)) + so[c] as f32) * scale[c])
            })
            .collect();
        for p in 0..pn {
            let at = pa + 4 * p;
            let (i, j, l, e) = (usize::from(d[at]), usize::from(d[at + 1]), usize::from(d[at + 2]), usize::from(d[at + 3]));
            let (Some(&a0), Some(&b0), Some(&c0)) = (pos.get(i), pos.get(j), pos.get(l)) else { continue };
            out.push([a0, b0, c0]);
            if e != l
                && let Some(&e0) = pos.get(e)
            {
                out.push([a0, c0, e0]);
            }
        }
    }
    out
}
