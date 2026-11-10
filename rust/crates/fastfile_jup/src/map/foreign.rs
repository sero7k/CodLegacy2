//! Pictures a map's materials name by hash only: the map zone holds a
//! 64-byte stub (the picture's hash with the top bit set, the rest zero) in
//! the material's picture table, and the picture itself is an asset of one of
//! the zones the game loads beside the map (`global_stream_mp`,
//! `ingame_shared_mp`, `ingame_base_mp`, ...). This reads those zones and
//! returns the header of each wanted picture, so its pixels can be fetched
//! from the packages like any other picture.
//!
//! Zones are looked up by name: a decoded `<name>.zone` in `$JUP_ZONE_DIR`
//! (offline work), else `<install>/zone/<name>.ff`. `$JUP_FOREIGN_ZONES`
//! (comma separated) replaces the default list.

use std::collections::{HashMap, HashSet};
use std::path::Path;

use super::index::{ZoneImage, u64_at};
use super::materials::{MapImage, map_image};
use crate::image;
use crate::oodle::Decompressor;

/// The zones searched, in order, unless `$JUP_FOREIGN_ZONES` names others.
pub const ZONES: &[&str] = &["global_stream_mp", "ingame_shared_mp", "ingame_base_mp", "global", "ingame", "ingame_br", "code_post_gfx", "code_pre_gfx", "common_mp", "global_mp"];

/// A picture found in another zone.
#[derive(Clone, Debug)]
pub struct Resolved {
    pub image: MapImage,
    pub usage: u8,
    /// The zone it was found in.
    pub zone: String,
}

fn zone_of(install: &Path, name: &str, oodle: &dyn Decompressor) -> Option<ZoneImage> {
    if let Some(dir) = std::env::var_os("JUP_ZONE_DIR").filter(|v| !v.is_empty()) {
        let p = Path::new(&dir).join(format!("{name}.zone"));
        if let Ok(bytes) = std::fs::read(&p) {
            return ZoneImage::parse(bytes).ok();
        }
    }
    let p = install.join("zone").join(format!("{name}.ff"));
    if !p.is_file() {
        return None;
    }
    super::read_image(&p, oodle).ok()
}

/// Every offset in `z` where an image header with one of the `wanted`
/// hashes starts (hash, then eight zero bytes).
fn scan(z: &[u8], wanted: &HashSet<u64>) -> Vec<usize> {
    let mut out = Vec::new();
    if z.len() < image::HEADER_LEN {
        return out;
    }
    for at in 0..=z.len() - image::HEADER_LEN {
        // (Cheap tests first: the second word is zero, the first has the top bit clear.)
        if z[at + 8..at + 16] != [0u8; 8] || z[at + 7] & 0x80 != 0 {
            continue;
        }
        if wanted.contains(&u64_at(z, at)) {
            out.push(at);
        }
    }
    out
}

/// Looks the `wanted` picture hashes up in the zones beside the map.
pub fn resolve(install: &Path, oodle: &dyn Decompressor, wanted: &HashSet<u64>, report: &mut Vec<String>) -> HashMap<u64, Resolved> {
    let mut found: HashMap<u64, Resolved> = HashMap::new();
    if wanted.is_empty() {
        return found;
    }
    let names: Vec<String> = match std::env::var("JUP_FOREIGN_ZONES").ok().filter(|v| !v.is_empty()) {
        Some(v) => v.split(',').map(|s| s.trim().to_owned()).collect(),
        None => ZONES.iter().map(|s| (*s).to_owned()).collect(),
    };
    let mut searched = Vec::new();
    for name in names {
        let pending: HashSet<u64> = wanted.iter().copied().filter(|h| !found.contains_key(h)).collect();
        if pending.is_empty() {
            break;
        }
        let started = std::time::Instant::now();
        let Some(zi) = zone_of(install, &name, oodle) else { continue };
        let mut hits = 0usize;
        for at in scan(&zi.bytes, &pending) {
            if let Ok(Some(rec)) = image::parse(&zi.bytes, at)
                && !found.contains_key(&rec.hash)
            {
                found.insert(rec.hash, Resolved { image: map_image(&zi.bytes, at), usage: rec.usage, zone: name.clone() });
                hits += 1;
            }
        }
        searched.push(format!("{name} {hits} ({:.1}s)", started.elapsed().as_secs_f32()));
    }
    report.push(format!(
        "pictures by hash: {} of {} found in other zones [{}]",
        found.len(),
        wanted.len(),
        searched.join(", ")
    ));
    found
}
