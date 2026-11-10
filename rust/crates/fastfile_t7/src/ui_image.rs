//! bo2mp: Black Ops III pictures for the menus (its weapons' icons, for
//! Create-a-Class): every image of a zone whose name a filter keeps,
//! decoded to RGBA8 at most `max_dim` on a side (pixels from the zone or
//! the `.xpak` packages beside it).

use std::path::Path;

use crate::texture::{image_levels_max, level_rgba};
use crate::walk::{Options, walk_with};
use crate::xpak::XpakSet;

/// One picture: (name, width, height, RGBA8).
pub type Picture = (String, u32, u32, Vec<u8>);

/// The images of the zone at `zone_path` whose names `keep` says yes to,
/// decoded (the ones that do not decode are left out). The walk goes as far
/// as it reads.
pub fn read_images(zone_path: &Path, keep: impl Fn(&str) -> bool, max_dim: u32) -> Result<Vec<Picture>, String> {
    read_images_report(zone_path, keep, max_dim).map(|(found, _)| found)
}

/// bo2mp: `read_images`, with why each kept image that did not decode
/// failed: (name, reason), e.g. its `.xpak` part not in any package beside
/// the zone.
pub fn read_images_report(zone_path: &Path, keep: impl Fn(&str) -> bool, max_dim: u32) -> Result<(Vec<Picture>, Vec<(String, String)>), String> {
    let zone = crate::Zone::open(zone_path).map_err(|e| format!("{}: {e}", zone_path.display()))?;
    let w = walk_with(&zone, Options::default());
    let dir = zone_path.parent().unwrap_or(Path::new("."));
    let paks = XpakSet::open_dir(dir).map(|(p, _)| p).unwrap_or_default();
    let (mut out, mut failed) = (Vec::new(), Vec::new());
    for im in w.capture.images.iter().filter(|im| !im.name.starts_with(',') && keep(&im.name)) {
        match image_levels_max(im, &zone.image, &paks, max_dim) {
            Ok(levels) => match level_rgba(&levels, 0) {
                Some((width, height, rgba)) => out.push((im.name.clone(), width, height, rgba)),
                None => failed.push((im.name.clone(), format!("format {} does not decode", im.format))),
            },
            Err(e) => failed.push((im.name.clone(), e.to_string())),
        }
    }
    Ok((out, failed))
}
