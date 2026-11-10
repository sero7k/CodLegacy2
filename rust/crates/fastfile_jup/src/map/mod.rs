//! A Modern Warfare III map read into plain data: the world's surfaces,
//! the entities, the bounds.
//!
//! - [`index`]: the zone image, its asset list, the name hash.
//! - [`world`]: the world's surfaces (`gfx_map` records over the
//!   `gfx_map_trzone` buffers).
//! - [`tiles`]: the streamed world zones (`<map>_<x>_<y>_0_<lod>_tr`, the
//!   swap zones) a big map keeps most of its world in.
//! - [`materials`]: each surface's material, the materials' pictures and
//!   their pixels from the install's packages.
//! - [`coverage`]: where each picture comes from (zone, streamed part,
//!   download package, nowhere), read from the packages' indexes alone.
//! - [`lighting`]: the zone's reflection probes and volume lighting images.
//! - [`atmosphere`]: the zone's vision set (fog, the sun-fog direction) and
//!   the placed lights' records.
//! - [`havok`], [`clip`]: the zone's collision (`col_map`: Havok shape
//!   tagfiles) as brushes and triangles.
//! - [`ents`]: the entities (the zone's `map_ents`, else the install's
//!   entity text) and the spawn points a multiplayer match needs.

pub mod atmosphere;
pub mod clip;
pub mod coverage;
pub mod ents;
pub mod foreign;
pub mod handover;
pub mod havok;
pub mod index;
pub mod lighting;
pub mod materials;
pub mod models;
pub mod terrain;
pub mod tiles;
pub mod world;

use std::path::{Path, PathBuf};

use crate::oodle::Decompressor;
pub use index::{ZoneImage, name_hash};
pub use world::WorldSurface;

/// Where a map's files are: the install root and the map's name.
#[derive(Clone, Debug)]
pub struct MapSources {
    pub install: PathBuf,
    pub name: String,
}

impl MapSources {
    /// From the map zone's path (`<install>/zone/<map>.ff`).
    pub fn beside(zone: &Path) -> Self {
        let name = zone
            .file_stem()
            .map(|s| s.to_string_lossy().to_ascii_lowercase())
            .unwrap_or_default();
        let install = zone
            .parent()
            .and_then(Path::parent)
            .map(Path::to_path_buf)
            .unwrap_or_default();
        Self { install, name }
    }

    pub fn zone(&self, name: &str) -> PathBuf {
        self.install.join("zone").join(format!("{name}.ff"))
    }
}

/// A map, read.
#[derive(Default)]
pub struct JupMap {
    pub name: String,
    pub surfaces: Vec<WorldSurface>,
    /// The entity text (Radiant's `{ "key" "value" }` blocks).
    pub entities: String,
    pub mins: [f32; 3],
    pub maxs: [f32; 3],
    /// The materials the surfaces draw with, and their pictures.
    pub materials: Vec<materials::MapMaterial>,
    pub images: Vec<materials::MapImage>,
    /// Each image's usage byte, by index in `images`.
    pub image_usage: Vec<u8>,
    /// The placed static models: the models (first LODs) and where each
    /// stands.
    pub prop_models: Vec<models::PropModel>,
    pub props: Vec<models::Prop>,
    /// The install's packages (opened for the models' meshes; the pictures
    /// come from them too).
    pub packages: Option<std::sync::Arc<materials::Packages>>,
    /// The sky's pictures ([`materials::sky_images`]).
    pub sky_images: Vec<materials::MapImage>,
    /// The map's colour grade volume ([`materials::colour_grade`]).
    pub grade: Option<materials::ColourGrade>,
    /// The map has a water body asset (Shipment's `<map>_water_01`): its flat
    /// sheets are the sea. Without one, flat far sheets are ground.
    pub has_water: bool,
    /// The terrain's coarse heights and its virtual texture
    /// ([`terrain`]), where the map has a terrain.
    pub terrain: Option<terrain::Terrain>,
    pub virtual_texture: Option<terrain::VirtualTexture>,
    /// The zone's own light: reflection probes and the volume images.
    pub lighting: lighting::MapLighting,
    /// The zone's vision set (fog, the sun-fog direction) and the records
    /// of its placed `light` entities.
    pub atmosphere: Option<atmosphere::Atmosphere>,
    pub lights: Vec<atmosphere::PlacedLight>,
    /// What was read and what was missing, line by line.
    pub report: Vec<String>,
}

/// Reads one zone into its image.
/// The map's own name inside its zone: the zone's name when the world zone
/// goes by it, else that name without the `jup_` the port added (a map
/// carried over from Modern Warfare II keeps its old name inside).
pub fn inner_name(z: &[u8], zone_name: &str) -> String {
    let candidates = [zone_name.to_owned(), zone_name.replacen("mp_jup_", "mp_", 1)];
    candidates
        .iter()
        .find(|n| world::find_buffers(z, name_hash(n)).is_some())
        .cloned()
        .unwrap_or_else(|| zone_name.to_owned())
}

pub fn read_image(path: &Path, oodle: &dyn Decompressor) -> Result<ZoneImage, String> {
    let (_, bytes) =
        crate::container::read_zone(path, oodle).map_err(|e| format!("{}: {e}", path.display()))?;
    ZoneImage::parse(bytes).map_err(|e| format!("{}: {e}", path.display()))
}

impl JupMap {
    /// Reads world zone `k`'s surfaces (its buffers in `z`).
    fn read_zone(
        &mut self,
        name: &str,
        z: &[u8],
        b: &world::WorldBuffers,
        recs: &[world::SurfaceRec],
        k: u32,
    ) -> Result<(), String> {
        let read = world::read_surfaces(z, b, recs, k)?;
        let tris: usize = read.iter().map(|s| s.tris.len()).sum();
        self.report.push(format!(
            "world zone {k} {name}: {} surfaces, {tris} triangles ({:.1} MiB of vertex streams)",
            read.len(),
            b.vertex_bytes as f64 / (1024.0 * 1024.0)
        ));
        self.surfaces.extend(read);
        Ok(())
    }

    pub fn load(src: &MapSources, oodle: &dyn Decompressor) -> Result<Self, String> {
        let mut out = JupMap {
            name: src.name.clone(),
            ..Default::default()
        };
        let started = std::time::Instant::now();
        let zone = read_image(&src.zone(&src.name), oodle)?;
        out.report.push(format!(
            "zone: {} assets, {:.1} MiB, inflated in {:.1}s",
            zone.types.len(),
            zone.bytes.len() as f64 / (1024.0 * 1024.0),
            started.elapsed().as_secs_f32()
        ));
        let z = &zone.bytes;
        // The name the map's own assets go by: its zone's, else the one it
        // had in the game it came from (Modern Warfare II's Shipment is
        // `mp_shipment_iw9` inside `mp_jup_shipment_iw9`).
        let inner = inner_name(z, &src.name);
        if inner != src.name {
            out.report.push(format!("assets named as {inner}"));
        }
        let (list, count) =
            world::find_surfaces(z).ok_or("no surface list (gfx_map) in the map zone")?;
        let recs = world::records(z, list, count);
        let mut zones: Vec<u32> = recs.iter().map(|r| r.zone).collect();
        zones.sort_unstable();
        zones.dedup();
        out.report.push(format!(
            "surface list: {count} surfaces over {} world zone(s)",
            zones.len()
        ));
        // The placed models (their materials are read with the world's).
        let placed = models::read(&zone, list, &mut out.report);
        let mut extra: Vec<usize> = placed
            .models
            .values()
            .flat_map(|d| d.materials.iter().flatten().copied())
            .collect();
        extra.sort_unstable();
        extra.dedup();
        let found = materials::read_with(
            &zone,
            &recs.iter().map(|r| r.zone).collect::<Vec<_>>(),
            &extra,
            &mut out.report,
            &|wanted, report| foreign::resolve(&src.install, oodle, wanted, report),
        );
        let started_models = std::time::Instant::now();
        let paks = materials::Packages::shared(&src.install);
        (out.prop_models, out.props) = models::meshes(
            &zone,
            &placed,
            &|key| paks.fetch(key, oodle),
            &found.by_entry,
            &mut out.report,
        );
        out.report.push(format!(
            "static models: read in {:.1}s",
            started_models.elapsed().as_secs_f32()
        ));
        drop(placed);
        out.packages = Some(paks);
        let images = zone.types.iter().filter(|&&t| t == 21).count();
        let run = crate::image::ImageRun::find(z, images).unwrap_or_default();
        out.sky_images = materials::sky_images(&zone, &run);
        out.grade = materials::colour_grade(&zone, &run);
        out.has_water = [&inner, &src.name].iter().any(|n| !index::find_all(z, &name_hash(&format!("{n}_water_01")).to_le_bytes()).is_empty());
        out.lighting = lighting::read(&zone, &run, &mut out.report);
        out.atmosphere = atmosphere::Atmosphere::read(z);
        out.report.push(match &out.atmosphere {
            Some(a) => format!(
                "atmosphere: vision set {}{}",
                a.name,
                a.fog.map_or(", no fog".to_owned(), |f| format!(
                    ", fog start {} half {} opacity {} sun {:?}",
                    f.start, f.half, f.opacity, f.sun_dir
                ))
            ),
            None => "atmosphere: no vision set".to_owned(),
        });
        out.report
            .push(format!("sky: {} pictures", out.sky_images.len()));
        // The terrain: its coarse heights and its texture's colour tiles.
        out.terrain = terrain::height_grid(z, name_hash(&src.name)).or_else(|| terrain::height_grid(z, name_hash(&inner)));
        if let Some(t) = &out.terrain {
            out.virtual_texture = terrain::virtual_texture(&zone, &run, t.half);
        }
        drop(run);
        out.report.push(match (&out.terrain, &out.virtual_texture) {
            (Some(t), vt) => format!(
                "terrain: {0}x{0} heights, {1} units apart, from {2:.0}; {3}",
                t.size,
                t.cell,
                t.origin[0],
                vt.as_ref().map_or("no texture".to_owned(), |v| format!(
                    "texture of {} levels, {} of {} tiles with colour",
                    v.levels,
                    v.tiles.iter().flatten().count(),
                    v.tiles.len()
                ))
            ),
            (None, _) => "terrain: none".to_owned(),
        });
        // The zone's own entities (else the install's text copy, below).
        // (The entities go by the zone's name even where the world keeps
        // its old one: Shipment's 707 are `mp_jup_shipment_iw9`'s.)
        match ents::from_zone(z, name_hash(&src.name)).or_else(|e| if inner != src.name { ents::from_zone(z, name_hash(&inner)) } else { Err(e) }) {
            Ok(list) => {
                let vec3 = |e: &ents::Entity, key: &str| -> Option<[f32; 3]> {
                    let v = e.iter().find(|(k, _)| k == key)?;
                    let p: Vec<f32> = v.1.split_whitespace().filter_map(|x| x.parse().ok()).collect();
                    (p.len() == 3).then(|| [p[0], p[1], p[2]])
                };
                let at: Vec<([f32; 3], [f32; 3])> = list
                    .iter()
                    .filter(|e| e.iter().any(|(k, v)| k == "classname" && v == "light"))
                    .filter_map(|e| Some((vec3(e, "origin")?, vec3(e, "angles").unwrap_or([0.0; 3]))))
                    .collect();
                out.lights = atmosphere::lights(z, &at);
                if !at.is_empty() {
                    out.report.push(format!("lights: {} placed, {} records read", at.len(), out.lights.len()));
                }
                out.entities = ents::to_text(&list);
                out.report.push(format!(
                    "entities: {} in the zone's map_ents, {} bytes kept",
                    list.len(),
                    out.entities.len()
                ));
            }
            Err(e) => out.report.push(format!("entities: {e}")),
        }
        // The map's own world zone.
        if zones.contains(&0) {
            let buffers = world::find_buffers(z, name_hash(&inner))
                .ok_or("no world zone (gfx_map_trzone) of the map's name")?;
            out.read_zone(&inner, z, &buffers, &recs, 0)?;
        }
        drop(zone);
        // The streamed tiles, in the order the surfaces count them.
        let names = tiles::names(src);
        if zones.iter().any(|&k| k > 0) {
            out.report
                .push(format!("tiles: {} streamed world zones", names.len()));
        }
        for &k in zones.iter().filter(|&&k| k > 0) {
            let Some(name) = names.get(k as usize - 1) else {
                out.report
                    .push(format!("tile {k}: no zone (only {} found)", names.len()));
                continue;
            };
            if !tiles::drawn(name) {
                out.report
                    .push(format!("tile {k} {name}: another state, left out"));
                continue;
            }
            let image = match read_image(&src.zone(name), oodle) {
                Ok(i) => i,
                Err(e) => {
                    out.report.push(format!("tile {k} {name}: {e}"));
                    continue;
                }
            };
            let Some(buffers) = world::find_buffers(&image.bytes, name_hash(name)) else {
                out.report
                    .push(format!("tile {k} {name}: no world zone of its name"));
                continue;
            };
            let want = world::packed_of(&recs, k);
            if buffers.packed_bytes != want {
                out.report.push(format!(
                    "tile {k} {name}: {} packed triangle bytes, the surfaces want {want}",
                    buffers.packed_bytes
                ));
                continue;
            }
            if let Err(e) = out.read_zone(name, &image.bytes, &buffers, &recs, k) {
                out.report.push(format!("tile {k} {name}: {e}"));
            }
        }
        if out.surfaces.is_empty() {
            return Err("no world surface read".into());
        }
        for s in &mut out.surfaces {
            s.material = found.surface_material.get(s.index).copied().flatten();
        }
        (out.materials, out.images, out.image_usage) = (found.materials, found.images, found.image_usage);
        let tris: usize = out.surfaces.iter().map(|s| s.tris.len()).sum();
        out.report.push(format!(
            "world: {} surfaces, {} vertices, {tris} triangles",
            out.surfaces.len(),
            out.surfaces
                .iter()
                .map(|s| s.positions.len())
                .sum::<usize>(),
        ));
        let mut lo = [f32::MAX; 3];
        let mut hi = [f32::MIN; 3];
        for p in out.surfaces.iter().flat_map(|s| s.positions.iter()) {
            for k in 0..3 {
                lo[k] = lo[k].min(p[k]);
                hi[k] = hi[k].max(p[k]);
            }
        }
        (out.mins, out.maxs) = (lo, hi);
        if out.entities.is_empty() {
            match ents::read(src) {
                Ok(text) => {
                    out.entities = text;
                    out.report
                        .push(format!("entities: {} bytes of text", out.entities.len()));
                }
                Err(e) => out.report.push(format!("entities: {e}")),
            }
        }
        Ok(out)
    }
}
