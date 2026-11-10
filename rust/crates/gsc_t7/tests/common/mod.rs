//! Shared by the zone tests: the staged zones and their script objects.

use std::path::PathBuf;

use gsc_t7::T7Object;

pub fn zone_dir() -> Option<PathBuf> {
    let dir = std::env::var_os("T7_ZONES").map_or_else(|| PathBuf::from("/mnt/user-data/uploads/zone"), PathBuf::from);
    dir.join("zm_zod_patch.ff").is_file().then_some(dir)
}

/// Each script object in a zone's image (found by its magic).
pub fn objects(image: &[u8]) -> Vec<&[u8]> {
    let magic = gsc_t7::object::MAGIC;
    let mut starts = Vec::new();
    let mut i = 0;
    while let Some(p) = image[i..].windows(8).position(|w| w == magic) {
        starts.push(i + p);
        i += p + 8;
    }
    starts.iter().enumerate().map(|(k, &s)| &image[s..starts.get(k + 1).copied().unwrap_or(image.len())]).collect()
}

/// Shadows of Evil's scripts, in zone order.
#[allow(dead_code)]
pub fn zod_objects() -> Option<Vec<T7Object>> {
    let dir = zone_dir()?;
    let mut all = Vec::new();
    // (T7_MAP=zm_castle etc.: another map's patch zone in place of
    // Shadows of Evil's.)
    let map = std::env::var("T7_MAP").unwrap_or_else(|_| "zm_zod".to_owned());
    for zone in ["core_patch".to_owned(), "zm_patch".to_owned(), format!("{map}_patch")] {
        let z = fastfile_t7::Zone::open(&dir.join(format!("{zone}.ff"))).expect("zone");
        for bytes in objects(&z.image) {
            all.push(T7Object::parse(bytes).expect("object"));
        }
    }
    Some(all)
}
