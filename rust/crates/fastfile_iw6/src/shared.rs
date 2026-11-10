//! Pictures and materials a map zone names but does not hold.
//!
//! A Ghosts map's material can name an image another zone loads: the
//! image's name then starts with `,` (the linker's "use the loaded one")
//! and its record is empty (format 0, 0 x 0, no pixels, no stream parts).
//! Most are `common_mp`'s (shared floors, walls and trims: Chasm's
//! `ch_floor_broken02_*` cover 1.5 % of its triangles), a few are the
//! engine's own (`$white`, `$black`, `$identitynormalmap`), which no zone
//! holds. A placed model can name a whole material the same way
//! (`,m/mtl_sz_crate_federation`: no technique set, no textures here):
//! Strikezone's and Zebra's crates are `common_core_mp`'s, Sovereign's
//! glass `common_mp`'s.
//!
//! `SharedImages` reads zones once and keeps every image they hold by name
//! (the records, whose stream parts point into the same
//! `imagefile<N>.pak`s, and the bytes of those whose pixels are inline),
//! and every material they hold by name (its technique set's name and its
//! textures by image name).

use std::collections::HashMap;
use std::path::Path;

use crate::image::{ImagePaks, ImagePixels, image_pixels};
use crate::walk::{ImageRec, Span};

/// The zones whose images and materials map zones name, in the order they
/// are read (an image keeps its first record: a patch before the zone it
/// patches). The first four are the base game's; the last two are the DLC
/// maps' (`common_core_dlc_updated_mp` before `common_core_dlc_mp`).
/// The zones the effect and model lanes also search, after `SHARED_ZONES`:
/// `common` holds 513 effects (the `fx/fire`, `fx/misc` families the maps'
/// scripts name) and 26 models that no other shared zone has (2026-11-04:
/// before, an effect or model only `common` holds was "in no zone read").
/// The renderer does not load them (`Scene::zones_for`).
pub const EXTRA_SHARED_ZONES: [&str; 2] = ["patch_common", "common"];

/// `SHARED_ZONES` then `EXTRA_SHARED_ZONES` (`BO2MP_IW6_COMMON_ZONE=0`:
/// the first only), the lanes' search order.
pub fn lane_zones() -> Vec<&'static str> {
    let mut v: Vec<&'static str> = SHARED_ZONES.to_vec();
    if !std::env::var("BO2MP_IW6_COMMON_ZONE").is_ok_and(|x| x.trim() == "0") {
        v.extend(EXTRA_SHARED_ZONES);
    }
    v
}

pub const SHARED_ZONES: [&str; 6] = [
    "patch_common_mp",
    "common_mp",
    "patch_common_core_mp",
    "common_core_mp",
    "common_core_dlc_updated_mp",
    "common_core_dlc_mp",
];

/// A material another zone holds, its images by name.
#[derive(Clone, Debug, Default)]
pub struct SharedMaterial {
    pub name: String,
    pub techset: String,
    pub sort_key: u8,
    /// (name hash, sampler state, semantic, image name)
    pub textures: Vec<(u32, u8, u8, String)>,
    pub constants: Vec<crate::walk::MaterialConstant>,
}

/// The images and materials some zones hold, by name (lower case, no
/// leading `,`).
#[derive(Default)]
pub struct SharedImages {
    by_name: HashMap<String, (ImageRec, Vec<u8>)>,
    materials: HashMap<String, SharedMaterial>,
    /// Effects by lower-case path (an effect keeps its first definition).
    effects: HashMap<String, crate::fxview::OwnedFx>,
    /// Effect paths by their last part.
    effects_by_base: HashMap<String, String>,
    /// bo2mp: impact effect tables (`ImpactFx`) by zone order, the first
    /// zone's first.
    impact_tables: Vec<crate::walk::ImpactTableRec>,
    /// bo2mp: tracer definitions by lower-case name (no leading `,`).
    tracers: HashMap<String, crate::walk::TracerRec>,
    /// The zones read, for reports.
    pub zones: Vec<String>,
}

/// An image name as a key: lower case, the reference mark dropped.
pub fn image_key(name: &str) -> String {
    name.trim_start_matches(',').to_ascii_lowercase()
}

/// Whether an image record only names an image held elsewhere.
pub fn is_reference(rec: &ImageRec) -> bool {
    rec.format == 0 && rec.pixels.is_none() && !rec.streamed
}

impl SharedImages {
    /// Adds every image `zone` holds (an image already known keeps its
    /// first record: read a patch zone before the zone it patches).
    pub fn add_zone(&mut self, path: &Path) -> Result<usize, String> {
        let zone = crate::Zone::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
        let (cap, _) = crate::walk::walk_zone(&zone);
        let mut added = 0usize;
        for m in &cap.materials {
            let Some(ts) = m.techset else { continue };
            let key = image_key(&m.name);
            if self.materials.contains_key(&key) {
                continue;
            }
            let textures = m
                .textures
                .iter()
                .filter_map(|t| {
                    let img = &cap.images.get(t.image?.index as usize)?.name;
                    Some((t.name_hash, t.sampler_state, t.semantic, img.clone()))
                })
                .collect();
            self.materials.insert(
                key,
                SharedMaterial {
                    name: m.name.trim_start_matches(',').to_owned(),
                    techset: cap.name_of(ts).trim_start_matches(',').to_owned(),
                    sort_key: m.sort_key,
                    textures,
                    constants: m.constants.clone(),
                },
            );
        }
        self.impact_tables.extend(cap.impact_tables.iter().cloned());
        for (k, t) in &cap.tracers {
            if t.speed > 0.0 {
                self.tracers.entry(k.trim_start_matches(',').to_owned()).or_insert_with(|| t.clone());
            }
        }
        for f in &cap.fx {
            if f.name.starts_with(',') || f.elems.is_empty() {
                continue;
            }
            let key = f.name.to_ascii_lowercase();
            if self.effects.contains_key(&key) {
                continue;
            }
            let base = key.rsplit('/').next().unwrap_or(&key).to_owned();
            self.effects_by_base.entry(base).or_insert_with(|| key.clone());
            self.effects.insert(key, crate::fxview::own(f, &cap, &zone.image));
        }
        for rec in &cap.images {
            if is_reference(rec) {
                continue;
            }
            let key = image_key(&rec.name);
            if self.by_name.contains_key(&key) {
                continue;
            }
            let mut rec = rec.clone();
            let bytes = match rec.pixels {
                Some(span) => {
                    let b = span.get(&zone.image).to_vec();
                    rec.pixels = Some(Span {
                        pos: 0,
                        len: b.len(),
                    });
                    b
                }
                None => Vec::new(),
            };
            self.by_name.insert(key, (rec, bytes));
            added += 1;
        }
        self.zones.push(
            path.file_stem()
                .map_or_else(String::new, |s| s.to_string_lossy().into_owned()),
        );
        Ok(added)
    }

    /// bo2mp: the bullet impact table: the first `ImpactFx` read whose
    /// rows are all there (15 rows of 38 cells).
    pub fn impact_table(&self) -> Option<&crate::walk::ImpactTableRec> {
        self.impact_tables.iter().find(|t| t.cells.len() >= 15 * 38)
    }

    /// bo2mp: a tracer definition by name (a leading `,` ignored).
    pub fn tracer(&self, name: &str) -> Option<&crate::walk::TracerRec> {
        self.tracers.get(&name.trim_start_matches(',').to_ascii_lowercase())
    }

    /// Bytes kept of inline pixels.
    pub fn inline_bytes(&self) -> usize {
        self.by_name.values().map(|(_, b)| b.len()).sum()
    }

    pub fn len(&self) -> usize {
        self.by_name.len()
    }

    pub fn is_empty(&self) -> bool {
        self.by_name.is_empty()
    }

    /// A material by name (`,name` or `name`).
    pub fn material(&self, name: &str) -> Option<&SharedMaterial> {
        self.materials.get(&image_key(name))
    }

    /// An effect by path (`,path` or `path`) or, failing that, by its last part.
    pub fn effect(&self, name: &str) -> Option<&crate::fxview::OwnedFx> {
        let key = image_key(name);
        self.effects.get(&key).or_else(|| {
            let base = key.rsplit('/').next().unwrap_or(&key);
            self.effects.get(self.effects_by_base.get(base)?)
        })
    }

    pub fn effect_count(&self) -> usize {
        self.effects.len()
    }

    pub fn material_count(&self) -> usize {
        self.materials.len()
    }

    /// The record of an image by name (`,name` or `name`).
    pub fn get(&self, name: &str) -> Option<&ImageRec> {
        self.by_name.get(&image_key(name)).map(|(r, _)| r)
    }

    /// An image's pixels by name, as `image::image_pixels` reads them
    /// (streamed parts from the paks in `paks`' folder).
    pub fn pixels(
        &self,
        name: &str,
        paks: &mut ImagePaks,
        max_size: u32,
    ) -> Result<ImagePixels, String> {
        let (rec, bytes) = self.by_name.get(&image_key(name)).ok_or_else(|| {
            format!(
                "{} is in none of {:?}",
                name.trim_start_matches(','),
                self.zones
            )
        })?;
        image_pixels(rec, bytes, paks, max_size)
    }
}

/// The engine's built-in pictures a material can name (`$white`, `$black`,
/// `$identitynormalmap`, ...): 4 x 4 `R8G8B8A8_UNORM`, one level, or `None`
/// for a name that is not one of them.
pub fn builtin_pixels(name: &str) -> Option<ImagePixels> {
    let texel: [u8; 4] = match image_key(name).as_str() {
        "$white" => [255, 255, 255, 255],
        "$black" => [0, 0, 0, 255],
        "$gray" | "$grey" => [128, 128, 128, 255],
        // Flat: x and y at the middle, z up.
        "$identitynormalmap" => [128, 128, 255, 255],
        _ => return None,
    };
    Some(ImagePixels {
        width: 4,
        height: 4,
        levels: 1,
        dxgi_format: 28,
        data: texel.repeat(16),
    })
}

#[cfg(test)]
mod tests {
    #[test]
    fn keys_drop_the_reference_mark() {
        assert_eq!(
            super::image_key(",CH_Floor_Broken02_col"),
            "ch_floor_broken02_col"
        );
        assert!(
            super::builtin_pixels(",$white")
                .is_some_and(|p| p.data.len() == 64 && p.data[0] == 255)
        );
        assert!(super::builtin_pixels("ch_floor").is_none());
    }
}
