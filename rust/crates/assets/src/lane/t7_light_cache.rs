//! bo2mp (load speed): a Black Ops III map's baked sun map kept on disk.
//!
//! The sun map (`fastfile_t7::light::SunMap`: the map seen from the sun,
//! 4096 texels a side on Skyjacked) is the same every load: a function of
//! the zone, which materials block light, and the spawn points. Baking it
//! costs 3.5-4 s a load (its blockers and the rasterising). The first load
//! writes the finished map as the `SUNMAP` section of the map's one
//! finished file, `<bake root>/t7/<map>/map.pak` (`map_bake::map_file`,
//! `docs/MAP_CONVERTER.md`); later loads read it back in well under a
//! second. The section carries a key over everything it was made from (the
//! zone file's name, size and time, the blockers' rule per material, the
//! spawns, and this code), so a changed zone or rule rebakes instead of
//! serving a stale map.
//!
//! `BO2MP_T7_LIGHT_CACHE=0`: the old way (bake every load, write nothing).
//! `CODLEGACY_BAKE=0` (the bake root off) also leaves it off.

use std::path::{Path, PathBuf};

use fastfile_t7::bake::SunDepth;
use fastfile_t7::light::{Blocks, SunMap};
use fastfile_t7::map::T7Map;

use map_bake::map_file::{self, Tag};

const FORMAT: u32 = 2;
const TAG: Tag = map_file::tag("SUNMAP");

pub(super) fn on() -> bool {
    std::env::var("BO2MP_T7_LIGHT_CACHE").map_or(true, |v| v.trim() != "0") && map_bake::enabled()
}

/// A sun map as baked, with the blocker counts its report line shows.
pub(super) struct Baked {
    pub sun_map: SunMap,
    pub casters: usize,
    pub partial: usize,
}

/// Where the map's sun map lives, and the keys it must carry (the file's,
/// over the zone; the section's, over everything the sun map depends on).
pub(super) struct Slot {
    path: PathBuf,
    file_key: u64,
    key: u64,
}

impl Slot {
    /// `zone`: the map's zone file; `blocks`: the rule per material index.
    pub fn new(zone: &Path, map: &T7Map, spawns: &[[f32; 3]], blocks: &dyn Fn(usize) -> Blocks) -> Option<Slot> {
        let dir = map_bake::map_dir("t7", &map.name)?;
        let file_key = map_bake::Key::new().file(zone).str(&map.name).value();
        let mut key = map_bake::Key::new();
        key.u64(u64::from(FORMAT)).file(zone).str(&map.name);
        key.u64(spawns.len() as u64);
        for p in spawns {
            for x in p {
                key.bytes(&x.to_le_bytes());
            }
        }
        key.u64(map.materials.len() as u64);
        for m in 0..map.materials.len() {
            key.bytes(&[match blocks(m) {
                Blocks::No => 0,
                Blocks::Partly => 1,
                Blocks::Fully => 2,
            }]);
        }
        key.str(include_str!("t7_light.rs"));
        key.str(include_str!("../../../fastfile_t7/src/light.rs"));
        key.str(include_str!("../../../fastfile_t7/src/bake.rs"));
        Some(Slot { path: dir.join(map_file::FILE), file_key, key: key.value() })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// The sun map on disk, when its keys are this map's (a stale section
    /// reads as missing and is written afresh; a stale file is removed).
    pub fn read(&self) -> Option<Baked> {
        let body = map_file::read_section(&self.path, self.file_key, TAG, self.key)?;
        unpack(&body)
    }

    /// Writes the baked sun map into the map's file (the file's other
    /// sections kept; a temp file, then one rename). Returns the file's size.
    pub fn write(&self, baked: &Baked) -> std::io::Result<u64> {
        map_file::write_section(&self.path, self.file_key, TAG, self.key, &pack(baked))
    }
}

fn pack(baked: &Baked) -> Vec<u8> {
    let d = &baked.sun_map.depth;
    let mut body = Vec::with_capacity(64 + d.height.len() * 4);
    body.extend_from_slice(&d.size.to_le_bytes());
    for x in d.dir.iter().chain(&d.u).chain(&d.v).chain(&d.origin).chain([&d.texel, &baked.sun_map.base, &baked.sun_map.range]) {
        body.extend_from_slice(&x.to_le_bytes());
    }
    body.extend_from_slice(&(baked.casters as u64).to_le_bytes());
    body.extend_from_slice(&(baked.partial as u64).to_le_bytes());
    body.extend_from_slice(&(d.height.len() as u64).to_le_bytes());
    for h in &d.height {
        body.extend_from_slice(&h.to_le_bytes());
    }
    body
}

fn unpack(body: &[u8]) -> Option<Baked> {
    let mut r = Reader { b: body, at: 0 };
    let size = r.u32()?;
    let dir = r.f3()?;
    let u = r.f3()?;
    let v = r.f3()?;
    let origin = [r.f32()?, r.f32()?];
    let texel = r.f32()?;
    let base = r.f32()?;
    let range = r.f32()?;
    let casters = r.u64()? as usize;
    let partial = r.u64()? as usize;
    let n = r.u64()? as usize;
    if n != (size as usize).checked_mul(size as usize)? || !(1..=16384).contains(&size) {
        return None;
    }
    let tail = r.b.get(r.at..)?;
    if tail.len() != n * 4 {
        return None;
    }
    let height: Vec<f32> = tail.chunks_exact(4).map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]])).collect();
    let depth = SunDepth { size, dir, u, v, origin, texel, height };
    Some(Baked { sun_map: SunMap { depth, base, range }, casters, partial })
}

struct Reader<'a> {
    b: &'a [u8],
    at: usize,
}

impl Reader<'_> {
    fn take<const N: usize>(&mut self) -> Option<[u8; N]> {
        let s = self.b.get(self.at..self.at + N)?;
        self.at += N;
        Some(std::array::from_fn(|i| s[i]))
    }
    fn u32(&mut self) -> Option<u32> {
        self.take::<4>().map(u32::from_le_bytes)
    }
    fn u64(&mut self) -> Option<u64> {
        self.take::<8>().map(u64::from_le_bytes)
    }
    fn f32(&mut self) -> Option<f32> {
        self.take::<4>().map(f32::from_le_bytes)
    }
    fn f3(&mut self) -> Option<[f32; 3]> {
        Some([self.f32()?, self.f32()?, self.f32()?])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Baked {
        let depth = SunDepth { size: 2, dir: [0.0, 0.0, 1.0], u: [1.0, 0.0, 0.0], v: [0.0, 1.0, 0.0], origin: [-8.0, 4.5], texel: 1.25, height: vec![1.0, f32::MIN, 3.5, 0.0] };
        Baked { sun_map: SunMap { depth, base: -16.0, range: 48.0 }, casters: 7, partial: 2 }
    }

    #[test]
    fn round_trips() {
        let b = sample();
        let bytes = pack(&b);
        let back = unpack(&bytes).expect("reads back");
        assert_eq!(back.sun_map.depth.size, 2);
        assert_eq!(back.sun_map.depth.height, b.sun_map.depth.height);
        assert_eq!(back.sun_map.depth.origin, [-8.0, 4.5]);
        assert_eq!((back.sun_map.base, back.sun_map.range), (-16.0, 48.0));
        assert_eq!((back.casters, back.partial), (7, 2));
    }

    #[test]
    fn damage_is_nothing() {
        let bytes = pack(&sample());
        assert!(unpack(&bytes[..bytes.len() - 3]).is_none());
        assert!(unpack(&bytes[..40]).is_none());
        assert!(unpack(b"junk").is_none());
    }

    #[test]
    fn lives_in_the_map_file() {
        let dir = std::env::temp_dir().join(format!("t7_light_cache_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let slot = Slot { path: dir.join(map_file::FILE), file_key: 5, key: 6 };
        assert!(slot.read().is_none());
        slot.write(&sample()).unwrap();
        let back = slot.read().expect("reads back from map.pak");
        assert_eq!(back.sun_map.depth.height, sample().sun_map.depth.height);
        // A stale section key reads as missing; the file itself stays.
        let stale = Slot { path: slot.path.clone(), file_key: 5, key: 7 };
        assert!(stale.read().is_none());
        assert!(slot.path.is_file());
        // A stale file key (the zone changed): the file is removed.
        let other = Slot { path: slot.path.clone(), file_key: 9, key: 6 };
        assert!(other.read().is_none());
        assert!(!slot.path.is_file());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
