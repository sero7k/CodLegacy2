//! bo2mp: the Black Ops III look's bake (`super::bo3_look`), kept on disk.
//!
//! The sun map and the real-time ambient's grid are traced over the map's
//! own triangles (millions of them on a Ghosts map) and are the same every
//! time for the same map, sun, sky and settings. What `bo3_look::apply`
//! leaves in the world (the colour-table picture with the sun map and the
//! grid below it, and the light-table rows it wrote) is written to the
//! game's cache (`codlegacy-artifacts/cache/rt_look`) the first time; the next
//! load of the map reads it instead of tracing.
//!
//! An entry is found by a key over everything the bake reads: the world's
//! vertices and indices, the placed models (where, which, their triangles'
//! count), what each material does to the sun, the sun and sky given, the
//! settings (`BO2MP_RT_*`, `BO2MP_BO3_*`, `BO2MP_T7_*`) and the text of the
//! code that bakes (a change to it makes a new entry, never a stale one).
//! `BO2MP_RT_CACHE=0` leaves the cache out; a file that does not read back
//! whole is a miss.

use asset_world::{ModelMesh, StaticModelPlacement, T6Lut, WorldDraw};

/// The layout's number (raised when what is written changes).
const FORMAT: u32 = 1;
const KIND: &str = "rt_look";

/// The text of the code a cached bake comes out of.
const CODE: &[&str] = &[
    include_str!("rt_cache.rs"),
    include_str!("bo3_look.rs"),
    include_str!("rt_ambient.rs"),
    include_str!("../../../fastfile_t7/src/bake.rs"),
    include_str!("../../../fastfile_t7/src/light.rs"),
];

/// Whether the cache is used (`BO2MP_RT_CACHE=0` turns it off; so does
/// `BO2MP_RT_DUMP`, which wants the grid itself).
pub(super) fn on() -> bool {
    std::env::var("BO2MP_RT_CACHE").map_or(true, |v| v.trim() != "0")
        && std::env::var_os("BO2MP_RT_DUMP").is_none()
}

/// A quick 64-bit hash over words (the key reads tens of megabytes).
struct Hash(u64);

impl Hash {
    fn new() -> Self {
        Hash(0xcbf2_9ce4_8422_2325)
    }
    fn word(&mut self, w: u64) {
        self.0 = (self.0 ^ w).wrapping_mul(0x9E37_79B9_7F4A_7C15);
        self.0 ^= self.0 >> 29;
    }
    fn f3(&mut self, v: [f32; 3]) {
        self.word(u64::from(v[0].to_bits()) | u64::from(v[1].to_bits()) << 32);
        self.word(u64::from(v[2].to_bits()));
    }
    fn f3s(&mut self, v: &[[f32; 3]]) {
        self.word(v.len() as u64);
        for p in v {
            self.f3(*p);
        }
    }
    fn u32s(&mut self, v: &[u32]) {
        self.word(v.len() as u64);
        for p in v.chunks(2) {
            self.word(u64::from(p[0]) | u64::from(p.get(1).copied().unwrap_or(0)) << 32);
        }
    }
    fn text(&mut self, s: &str) {
        self.word(s.len() as u64);
        for b in s.as_bytes().chunks(8) {
            let mut w = [0u8; 8];
            w[..b.len()].copy_from_slice(b);
            self.word(u64::from_le_bytes(w));
        }
    }
}

/// What the key reads besides the world: the sun and sky, the settings the
/// bake made its choices from.
pub(super) struct Inputs<'a> {
    pub spawns: &'a [[f32; 3]],
    pub sun: [f32; 3],
    pub sun_colour: [f32; 3],
    pub exposure: Option<f32>,
    /// The mid-grey display value the exposure was solved for, if it was.
    pub auto_grey: Option<f32>,
    pub sky: Option<([f32; 3], [f32; 3])>,
    pub rt_free: bool,
    pub rt_wanted: bool,
    pub rt_local: bool,
    /// What a material does to the sun (0 none, 1 partly, 2 fully).
    pub material_class: &'a dyn Fn(Option<usize>) -> u8,
}

/// The cache key of a bake: `<map tag>-<hash>`, the tag from the world's
/// bounds and spawns (what a newer entry of the same map replaces).
pub(super) fn key(
    draw: &WorldDraw,
    meshes: &[ModelMesh],
    placements: &[Option<StaticModelPlacement>],
    i: &Inputs,
) -> String {
    let mut tag = Hash::new();
    tag.f3(draw.stats.min);
    tag.f3(draw.stats.max);
    tag.f3s(i.spawns);
    let mut h = Hash::new();
    h.word(u64::from(FORMAT));
    for code in CODE {
        h.text(code);
    }
    for v in [i.sun, i.sun_colour] {
        h.f3(v);
    }
    h.word(i.exposure.map_or(u64::MAX, |e| u64::from(e.to_bits())));
    h.word(i.auto_grey.map_or(u64::MAX, |e| u64::from(e.to_bits())));
    if let Some((z, hz)) = i.sky {
        h.f3(z);
        h.f3(hz);
    }
    h.word(u64::from(i.rt_free) | u64::from(i.rt_wanted) << 1 | u64::from(i.rt_local) << 2);
    let mut vars: Vec<(String, String)> = std::env::vars()
        .filter(|(k, _)| {
            (k.starts_with("BO2MP_RT_")
                || k.starts_with("BO2MP_BO3_")
                || k.starts_with("BO2MP_T7_"))
                && k != "BO2MP_RT_CACHE"
        })
        .collect();
    vars.sort();
    for (k, v) in &vars {
        h.text(k);
        h.text(v);
    }
    h.f3s(i.spawns);
    h.word(draw.stats.vertices as u64);
    h.word(draw.stats.triangles as u64);
    h.f3s(&draw.positions);
    h.u32s(&draw.packed_indices);
    for (s, &(start, count)) in draw.surface_index_ranges.iter().enumerate() {
        h.word(u64::from(start) | u64::from(count) << 32);
        h.word(u64::from((i.material_class)(
            draw.surface_materials.get(s).copied().flatten(),
        )));
    }
    // The placed models: each mesh used (its triangles' count, what its
    // surfaces do to the sun) and where every placement is.
    h.word(placements.len() as u64);
    for pl in placements.iter().flatten() {
        h.word(pl.mesh as u64);
        h.f3(pl.origin);
        for a in pl.axis {
            h.f3(a);
        }
        h.word(u64::from(pl.scale.to_bits()));
        h.f3(pl.transform.translation.to_array());
        h.f3(pl.transform.scale.to_array());
        h.word(
            pl.transform
                .rotation
                .to_array()
                .iter()
                .fold(0u64, |a, v| a.rotate_left(13) ^ u64::from(v.to_bits())),
        );
    }
    h.word(meshes.len() as u64);
    for m in meshes {
        for s in m.surfaces() {
            h.word((i.material_class)(s.material).into());
            if let Some(bevy::render::mesh::VertexAttributeValues::Float32x3(pos)) =
                s.mesh.attribute(bevy::prelude::Mesh::ATTRIBUTE_POSITION)
            {
                h.word(pos.len() as u64);
                for p in pos.iter().step_by(5) {
                    h.f3(*p);
                }
            }
            h.word(s.mesh.indices().map_or(0, |ix| ix.len() as u64));
        }
    }
    format!("look{:08x}-{:016x}", tag.0 >> 32, h.0)
}

/// What a bake left in the world.
pub(super) struct Baked {
    pub lut: std::sync::Arc<T6Lut>,
    /// (light-table row, its values).
    pub rows: Vec<(usize, [f32; 16])>,
    /// The bake's report lines.
    pub report: Vec<String>,
}

fn encode(b: &Baked) -> Vec<u8> {
    let mut o = Vec::with_capacity(b.lut.rgba.len() + 4096);
    o.extend_from_slice(&b.lut.width.to_le_bytes());
    o.extend_from_slice(&b.lut.height.to_le_bytes());
    o.extend_from_slice(&(b.lut.rgba.len() as u64).to_le_bytes());
    o.extend_from_slice(&b.lut.rgba);
    o.extend_from_slice(&(b.rows.len() as u32).to_le_bytes());
    for (row, v) in &b.rows {
        o.extend_from_slice(&(*row as u32).to_le_bytes());
        for f in v {
            o.extend_from_slice(&f.to_le_bytes());
        }
    }
    o.extend_from_slice(&(b.report.len() as u32).to_le_bytes());
    for l in &b.report {
        o.extend_from_slice(&(l.len() as u32).to_le_bytes());
        o.extend_from_slice(l.as_bytes());
    }
    o
}

fn decode(b: &[u8]) -> Option<Baked> {
    let mut at = 0usize;
    let mut take = |n: usize| -> Option<&[u8]> {
        let s = b.get(at..at.checked_add(n)?)?;
        at += n;
        Some(s)
    };
    let u32_of = |s: &[u8]| u32::from_le_bytes([s[0], s[1], s[2], s[3]]);
    let width = u32_of(take(4)?);
    let height = u32_of(take(4)?);
    let len = u64::from_le_bytes(take(8)?.try_into().ok()?) as usize;
    if len != width as usize * height as usize * 4 {
        return None;
    }
    let rgba = take(len)?.to_vec();
    let n = u32_of(take(4)?) as usize;
    let mut rows = Vec::with_capacity(n);
    for _ in 0..n {
        let row = u32_of(take(4)?) as usize;
        let raw = take(64)?;
        let v: [f32; 16] = std::array::from_fn(|k| f32::from_bits(u32_of(&raw[k * 4..k * 4 + 4])));
        if row > 1024 {
            return None;
        }
        rows.push((row, v));
    }
    let n = u32_of(take(4)?) as usize;
    let mut report = Vec::with_capacity(n);
    for _ in 0..n {
        let l = u32_of(take(4)?) as usize;
        report.push(String::from_utf8(take(l)?.to_vec()).ok()?);
    }
    (at == b.len()).then(|| Baked {
        lut: std::sync::Arc::new(T6Lut {
            width,
            height,
            rgba,
        }),
        rows,
        report,
    })
}

/// The bake kept under `key`, if there is one that reads back whole.
pub(super) fn load(key: &str) -> Option<Baked> {
    decode(&super::disk_cache::read(KIND, key)?)
}

/// Keeps a bake under `key` (written on a thread of its own).
pub(super) fn store(key: String, baked: Baked) {
    super::disk_cache::write_later(KIND, key, move || encode(&baked));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_bake_round_trips() {
        let mut row = [0f32; 16];
        row[3] = 64.0;
        row[15] = -1.5;
        let b = Baked {
            lut: std::sync::Arc::new(T6Lut {
                width: 2,
                height: 3,
                rgba: (0..24).collect(),
            }),
            rows: vec![(250, row), (255, [1.0; 16])],
            report: vec!["rt ambient: on".to_owned(), "bo3 look: on".to_owned()],
        };
        let back = decode(&encode(&b)).expect("reads back");
        assert_eq!(back.lut, b.lut);
        assert_eq!(back.rows, b.rows);
        assert_eq!(back.report, b.report);
        let mut cut = encode(&b);
        cut.pop();
        assert!(decode(&cut).is_none());
    }
}
