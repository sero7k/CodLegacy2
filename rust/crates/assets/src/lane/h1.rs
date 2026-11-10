//! bo2mp: Modern Warfare Remastered's (H1) materials, pictures and models
//! in the engine's catalogs, from a zone `fastfile_h1` walked: a map's
//! world surfaces and placed models (`super::h1_map`), and whatever else
//! names H1 materials and models (its guns).
//!
//! H1 is Advanced Warfare's engine, so its materials and models read as
//! AW's guest guns do (`super::s1`), which read as Ghosts' (`super::iw6`):
//! the packed vertex is Black Ops II's but for its unit vectors (unsigned
//! 10:10:10), its eight blend buckets fold into BO2's four, model materials
//! go on BO2's draw path by their technique set's name (`m_`/`mc_`), world
//! materials by Ghosts' `wc_l_sm_*` names, colour, normal and specular maps
//! from the zone itself, its own `<zone>.pak` (a usermap's, index 96), the
//! game's `imagefile<N>.pak`s or, for a picture or material the zone only
//! names (`,name`), the zones beside it and the shared zones
//! (`H1Shared`): a retail map's materials are all names, defined in its
//! `techsets_<map>.ff`, their pictures the map's own or `common_mp`'s.
//!
//! [`H1Builder`] is the reusable part: `material`, `world_material`,
//! `sky_material`, `image`, `model_ref` (a model's first LOD in BO2's
//! captured form, composite models' parts joined) and `anim_ref`.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use asset_core::{AssetEdge, AssetNamespace, AssetRef, T6Blend, T6Draw, ZoneOwner};
use asset_material::{
    AuthoredImage, AuthoredMaterial, MaterialCatalog, MaterialTextureBinding, TS_COLOR_MAP,
    TS_NORMAL_MAP, TS_SPECULAR_MAP,
};
use fastfile_h1::walk::{
    AssetId, Capture, ImageRec, MaterialConstant, MaterialRec, XAnimRec, XModelRec,
};

use super::iw6::{
    fold_blend, ghosts_layer_tokens, gpu_image, iw6_model_draw, iw6_vertex_to_t6, le_u16s, le_u32s,
};

/// `R_HashString` of the texture slots read (H1 hashes them as Ghosts).
pub(super) const COLOR_MAP_HASH: u32 = 0xa0ab_1041;
pub(super) const NORMAL_MAP_HASH: u32 = 0x59d3_0d0f;
pub(super) const SPECULAR_MAP_HASH: u32 = 0x34ec_ccb3;

/// H1 pixels as the shared GPU image builder takes them.
fn as_iw6(px: fastfile_h1::image::ImagePixels) -> fastfile_iw6::image::ImagePixels {
    fastfile_iw6::image::ImagePixels {
        width: px.width,
        height: px.height,
        levels: px.levels,
        dxgi_format: px.dxgi_format,
        data: px.data,
    }
}

/// Where Modern Warfare Remastered keeps its zones (`<install>/zone`, with
/// `common_mp.ff` and the `imagefile<N>.pak`s), found from a zone of it:
/// a retail map's own folder, or for a Horizon MW usermap
/// (`<install>/hmw-usermaps/<map>/<map>.ff`) a folder up the tree; else
/// the `mwr` game folder set in Game Folders. `None` when none has
/// `common_mp.ff` (the usermap then reads its own pictures only).
pub(crate) fn h1_zone_dir(zone_path: &Path) -> Option<PathBuf> {
    let is_zone_dir = |d: &Path| d.join("common_mp.ff").is_file();
    let mut at = zone_path.parent();
    for _ in 0..4 {
        let Some(dir) = at else { break };
        if is_zone_dir(dir) {
            return Some(dir.to_path_buf());
        }
        let zone = dir.join("zone");
        if is_zone_dir(&zone) {
            return Some(zone);
        }
        at = dir.parent();
    }
    let set = asset_transport::game_paths::get("mwr")?;
    [set.join("zone"), set].into_iter().find(|d| is_zone_dir(d))
}

/// Horizon MW's own zones beside an install's `zone/` folder
/// (`<install>/hmw-mod/zone/hmw_*.ff`), which may hold materials usermaps
/// built against them name.
fn hmw_zones(zone_dir: &Path) -> Vec<PathBuf> {
    let Some(dir) = zone_dir.parent().map(|p| p.join("hmw-mod").join("zone")) else {
        return Vec::new();
    };
    let Ok(read) = std::fs::read_dir(&dir) else {
        return Vec::new();
    };
    let mut out: Vec<PathBuf> = read
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.extension().is_some_and(|e| e.eq_ignore_ascii_case("ff"))
                && p.file_stem()
                    .is_some_and(|s| s.to_string_lossy().to_ascii_lowercase().starts_with("hmw_"))
        })
        .collect();
    out.sort();
    out
}

/// The images and materials an H1 map names but does not hold, from the
/// zones beside it and the shared zones (`fastfile_h1::shared`).
pub(crate) struct H1Shared {
    pub images: fastfile_h1::shared::SharedImages,
    /// What each zone read gave, for the report.
    pub lines: Vec<String>,
    /// Zones wanted but missing or unreadable, for the report.
    pub missing: Vec<String>,
    /// What none of the zones held (materials, pictures).
    pub unfound: (usize, usize),
}

/// bo2mp: the zones Modern Warfare Remastered's own renderer (`render_h1`)
/// draws a map from: the map, then the zones it draws on
/// (`h1_shared_zones`). Always since 31 Oct (the owner: MWR maps use MWR's
/// renderer only, as Black Ops III's maps use theirs; the converted world
/// is not drawn on them).
pub(crate) fn native_zones(map_path: &Path) -> Option<Vec<PathBuf>> {
    let map = map_path.file_stem()?.to_string_lossy().into_owned();
    let mut zones = vec![map_path.to_path_buf()];
    if let Some(dir) = h1_zone_dir(map_path) {
        // A retail map (in the install's own zone folder) draws from the
        // retail zones only, as the game does; Horizon MW's zones (about
        // 14 s of reading) are for Horizon MW's usermaps.
        let retail = map_path.parent().is_some_and(|p| p == dir);
        let shared = if retail {
            fastfile_h1::shared::retail_zones(&dir, &map)
        } else {
            h1_shared_zones(&dir, &map)
        };
        zones.extend(shared.into_iter().map(|(p, _)| p));
    }
    Some(zones)
}

/// The zones a map's names are looked up in, highest priority first, with
/// whether each loads after the map: `fastfile_h1::shared::retail_zones`
/// (the map's `patch_` and `techsets_` zones, `common_mp` and its patch
/// and techsets), then Horizon MW's `hmw_*` zones.
fn h1_shared_zones(zone_dir: &Path, map: &str) -> Vec<(PathBuf, bool)> {
    let mut zones = fastfile_h1::shared::retail_zones(zone_dir, map);
    zones.extend(hmw_zones(zone_dir).into_iter().map(|p| (p, false)));
    zones
}

/// What `map` (walked: `wanted` is what it names but does not hold) takes
/// from the zones of the install at `zone_dir` (`h1_shared_zones`): each
/// read once, in `fastfile_h1::shared::read_order`, keeping only what is
/// wanted, and dropped. A zone that would add nothing more (no material
/// or picture still wanted) is not opened. A zone that is missing or does
/// not read is skipped (and named in `missing`). The last map's result is
/// kept for a reload of the same map; another map's replaces it.
pub(crate) fn h1_shared(
    zone_dir: &Path,
    map: &str,
    mut wanted: fastfile_h1::shared::Wanted,
) -> Arc<H1Shared> {
    use std::sync::{Mutex, OnceLock};
    type Slot = Option<((PathBuf, String), Arc<H1Shared>)>;
    static LAST: OnceLock<Mutex<Slot>> = OnceLock::new();
    let last = LAST.get_or_init(Default::default);
    let key = (zone_dir.to_path_buf(), map.to_ascii_lowercase());
    if let Some(hit) = last.lock().ok().and_then(|g| {
        g.as_ref()
            .filter(|(k, _)| *k == key)
            .map(|(_, v)| v.clone())
    }) {
        return hit;
    }
    // (Release the previous map's before reading.)
    if let Ok(mut g) = last.lock() {
        *g = None;
    }
    let mut images = fastfile_h1::shared::SharedImages::default();
    let (mut lines, mut missing) = (Vec::new(), Vec::new());
    if !zone_dir.join("common_mp.ff").is_file() {
        missing.push("common_mp (not found)".to_owned());
    }
    // (A retail map's materials are all defined there.)
    let techsets = format!("techsets_{}", map.to_ascii_lowercase());
    if !wanted.materials.is_empty() && !zone_dir.join(format!("{techsets}.ff")).is_file() {
        missing.push(format!("{techsets} (not found: the map's materials)"));
    }
    let zones = h1_shared_zones(zone_dir, map);
    for i in fastfile_h1::shared::read_order(&zones) {
        let (p, over_map) = &zones[i];
        let name = p
            .file_stem()
            .map_or_else(String::new, |s| s.to_string_lossy().into_owned());
        let useful = if fastfile_h1::shared::holds_pictures(p) {
            !wanted.is_empty() || *over_map
        } else {
            !wanted.materials.is_empty()
        };
        if !useful {
            lines.push(format!("{name} (not read: nothing left to find)"));
            continue;
        }
        let rank = i as u32;
        match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            images.add_zone_for(p, Some(&mut wanted), rank, *over_map)
        })) {
            Ok(Ok(gave)) => lines.push(gave.to_string()),
            Ok(Err(e)) => {
                diag::warn!(World, "bo2mp h1: shared zone: {e}");
                missing.push(format!("{name} ({e})"));
            }
            Err(_) => {
                diag::warn!(World, "bo2mp h1: shared zone {} panicked", p.display());
                missing.push(format!("{name} (reader panicked)"));
            }
        }
    }
    let shared = Arc::new(H1Shared {
        images,
        lines,
        missing,
        unfound: (wanted.materials.len(), wanted.images.len()),
    });
    if let Ok(mut g) = last.lock() {
        *g = Some((key, shared.clone()));
    }
    shared
}

/// A picture's top level: width, height, RGBA8 bytes.
pub(crate) type Rgba = (u32, u32, Vec<u8>);

/// Whether a world or model material's colour map is drawn, and if not,
/// why (`H1Builder::colour`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum ColourMap {
    /// Bound, with pixels.
    Decoded,
    /// Bound to a picture streamed from a pak that is not installed (the
    /// retail `imagefile<N>.pak`s): the path.
    PakAbsent(String),
    /// No picture: the material or its colour map is in no zone read, or
    /// its pixels do not decode.
    Missing(String),
}

/// Where a material's picture is: the zone's image table, or by name in
/// the shared zones.
#[derive(Clone, Debug)]
enum MatImage {
    Zone(usize),
    Named(String),
}

/// A material as the builder reads it.
struct MatView {
    name: String,
    techset: String,
    sort_key: u8,
    /// (name hash, sampler state, semantic, image)
    textures: Vec<(u32, u8, u8, MatImage)>,
    constants: Vec<MaterialConstant>,
    /// The zone only names it and no zone read defines it.
    unresolved: bool,
}

/// Turns a walked H1 zone's materials, pictures and models into catalog
/// entries.
pub(crate) struct H1Builder<'a> {
    pub cap: &'a Capture,
    pub image: &'a [u8],
    pub strings: &'a [Option<String>],
    pub paks: fastfile_h1::image::ImagePaks,
    pub catalog: MaterialCatalog,
    pub owner: ZoneOwner,
    /// The namespace models' materials and pictures are linked under:
    /// Black Ops II's for a map's (as the other map lanes'; world
    /// materials always are). Names carry an `h1:` prefix either way.
    pub ns: AssetNamespace,
    /// What a model's material is named with in the catalog: `h1:` for a
    /// map's (in Black Ops II's namespace beside its own), none for the
    /// guns' (in the H1 namespace, where the weapons name them as they are).
    pub name_prefix: &'static str,
    /// Capture material index (`| WORLD` for a world material) -> catalog
    /// material index.
    materials: HashMap<usize, usize>,
    /// (capture image index, size cap, semantic) -> catalog image slot.
    images: HashMap<(usize, u32, u8), Option<usize>>,
    /// (image name, size cap, semantic) -> catalog image slot.
    named_images: HashMap<(String, u32, u8), Option<usize>>,
    pub image_failures: Vec<String>,
    pub decoded: usize,
    /// Pictures that came from the shared zones or the built-ins.
    pub from_shared: usize,
    /// Materials the zone only names, read from the shared zones.
    pub shared_materials: usize,
    /// Materials the zone only names that no shared zone holds (drawn
    /// with no picture: the draw's grey).
    pub unresolved_materials: Vec<String>,
    pub shared: Option<Arc<H1Shared>>,
    /// The zone's own pictures (not mere names) by key.
    zone_images: HashMap<String, usize>,
    /// Per catalog material linked: whether its colour map is drawn.
    pub colour: HashMap<usize, ColourMap>,
    /// Catalog materials of reveal decals (their reveal map in the colour
    /// map's alpha; the vertex alpha sets how much shows: `h1_map`).
    pub reveal: std::collections::HashSet<usize>,
    /// Whether world materials take their normal and specular maps too.
    pub world_shine: bool,
    /// A zone's 1 x 1 stand-in pictures are read from the retail zones
    /// instead (`use_retail_standins`).
    retail_standins: bool,
}

const WORLD: usize = 1 << 40;

/// A dielectric's specular byte: the shader's `raw_texel(v)^2` = 0.04.
const DIELECTRIC_SPECULAR: u8 = 9;

/// The same under Modern Warfare Remastered's own shader (its specular
/// squared as stored: 0.2^2 = 0.04).
const DIELECTRIC_SPECULAR_MWR: u8 = 51;

/// Modern Warfare Remastered's own shader on (`h1_map::mwr_on`, the look
/// on): specular pictures read as the game reads them.
fn mwr_reads() -> bool {
    super::h1_map::look_on() && std::env::var("BO2MP_H1_MWR").map_or(true, |v| v.trim() != "0")
}

/// A material's reveal map (`revealMap`).
const REVEAL_MAP_HASH: u32 = 0x34d8_49d5;

impl<'a> H1Builder<'a> {
    /// A builder over a walked zone at `zone_path`, its streamed pictures
    /// read from its own `<zone>.pak` and the paks in `retail_dir` (or
    /// beside it).
    pub fn new(
        cap: &'a Capture,
        zone: &'a fastfile_h1::Zone,
        zone_path: &Path,
        retail_dir: Option<&Path>,
        catalog: MaterialCatalog,
    ) -> Self {
        Self {
            cap,
            image: &zone.image,
            strings: &zone.assets.script_strings,
            paks: fastfile_h1::image::ImagePaks::for_zone(zone_path, retail_dir),
            catalog,
            owner: ZoneOwner::from_zone_path(zone_path),
            ns: AssetNamespace::T6,
            name_prefix: "h1:",
            materials: HashMap::new(),
            images: HashMap::new(),
            named_images: HashMap::new(),
            image_failures: Vec::new(),
            decoded: 0,
            from_shared: 0,
            shared_materials: 0,
            unresolved_materials: Vec::new(),
            shared: None,
            zone_images: cap
                .images
                .iter()
                .enumerate()
                .filter(|(_, r)| !fastfile_h1::shared::is_reference(r))
                .map(|(i, r)| (fastfile_h1::shared::image_key(&r.name), i))
                .collect(),
            colour: HashMap::new(),
            reveal: Default::default(),
            world_shine: false,
            retail_standins: false,
        }
    }

    /// The zone's 1 x 1 stand-in pictures (`fastfile_h1::shared::is_standin`:
    /// Horizon MW's red ones for the muzzle flash pictures) are taken from
    /// the retail zones in `dir` (the install's `zone/`) as the zone's own
    /// pictures of those names; one line of what was looked for, for the
    /// report.
    pub fn use_retail_standins(&mut self, dir: &Path) -> Option<String> {
        self.retail_standins = true;
        fastfile_h1::shared::load_standins(dir, &self.cap.images)
    }

    /// The retail picture standing for a zone picture, when it is a
    /// stand-in and the retail zones hold the real one.
    fn standin_for(&mut self, rec: &ImageRec, max: u32) -> Option<Result<fastfile_h1::image::ImagePixels, String>> {
        if fastfile_h1::shared::is_camo_slot(rec) {
            return Some(Ok(fastfile_h1::shared::neutral_pixels(&rec.name)));
        }
        if !self.retail_standins || !fastfile_h1::shared::is_standin(rec) {
            return None;
        }
        fastfile_h1::shared::standin_pixels(&rec.name, &mut self.paks, max)
    }

    /// A material picture's pixels wherever they are held (the zone, the
    /// shared zones, the built-ins).
    fn pixels_of(&mut self, image: &MatImage, max: u32) -> Result<fastfile_h1::image::ImagePixels, String> {
        let name = match image {
            MatImage::Zone(i) => self.cap.images.get(*i).map(|r| r.name.clone()).ok_or("no image")?,
            MatImage::Named(n) => n.clone(),
        };
        if let Some(px) = fastfile_h1::shared::builtin_pixels(&name) {
            return Ok(px);
        }
        let replaced = self.shared.as_ref().is_some_and(|s| s.images.replaces_map(&name));
        let own = match image {
            MatImage::Zone(i) => self.cap.images.get(*i).filter(|r| !fastfile_h1::shared::is_reference(r)),
            MatImage::Named(n) => self.zone_images.get(&fastfile_h1::shared::image_key(n)).and_then(|&i| self.cap.images.get(i)),
        };
        let own = own.cloned();
        match own {
            Some(rec) if !replaced => match self.standin_for(&rec, max) {
                Some(real) => real,
                // (A stand-in none of the zones holds the picture of: neutral,
                // not the stand-in's red.)
                None if self.retail_standins && fastfile_h1::shared::is_standin(&rec) => {
                    Ok(fastfile_h1::shared::neutral_pixels(&rec.name))
                }
                None => fastfile_h1::image::image_pixels(&rec, self.image, &mut self.paks, max),
            },
            _ => match &self.shared {
                Some(s) => s.images.pixels(&name, &mut self.paks, max),
                None => Err("no shared zones".to_owned()),
            },
        }
    }

    /// Materials linked so far.
    pub fn linked(&self) -> usize {
        self.materials.len()
    }

    fn string(&self, index: u32) -> String {
        self.strings
            .get(index as usize)
            .cloned()
            .flatten()
            .unwrap_or_else(String::new)
    }

    pub fn name(&self, id: AssetId) -> String {
        self.cap.name_of(id).trim_start_matches(',').to_owned()
    }

    /// A material's technique set name: the zone's, or for one the zone
    /// only names, its definition's in the zones read.
    pub fn techset_name(&self, index: usize) -> String {
        let Some(m) = self.cap.materials.get(index) else {
            return String::new();
        };
        match m.techset {
            Some(t) => self.cap.name_of(t).trim_start_matches(',').to_owned(),
            None => self
                .shared
                .as_ref()
                .and_then(|s| s.images.material(&m.name))
                .map_or_else(String::new, |s| s.techset.clone()),
        }
    }

    /// A material as the builder reads it: the zone's own, or for one the
    /// zone only names (no technique set, no textures) the shared zones'.
    fn view(&mut self, index: usize) -> Option<MatView> {
        let m: &MaterialRec = self.cap.materials.get(index)?;
        if m.techset.is_none() && m.textures.is_empty() {
            if let Some(s) = self
                .shared
                .as_ref()
                .and_then(|s| s.images.material(&m.name))
            {
                self.shared_materials += 1;
                return Some(MatView {
                    name: m.name.clone(),
                    techset: s.techset.clone(),
                    sort_key: s.sort_key,
                    textures: s
                        .textures
                        .iter()
                        .map(|(h, ss, sem, img)| (*h, *ss, *sem, MatImage::Named(img.clone())))
                        .collect(),
                    constants: s.constants.clone(),
                    unresolved: false,
                });
            }
            if self.unresolved_materials.len() < 64 {
                self.unresolved_materials
                    .push(m.name.trim_start_matches(',').to_owned());
            }
        }
        let unresolved = m.techset.is_none() && m.textures.is_empty();
        Some(MatView {
            unresolved,
            name: m.name.clone(),
            techset: m.techset.map_or_else(String::new, |t| {
                self.cap.name_of(t).trim_start_matches(',').to_owned()
            }),
            sort_key: m.sort_key,
            textures: m
                .textures
                .iter()
                .filter_map(|t| {
                    Some((
                        t.name_hash,
                        t.sampler_state,
                        t.semantic,
                        MatImage::Zone(t.image?.index as usize),
                    ))
                })
                .collect(),
            constants: m.constants.clone(),
        })
    }

    fn link_pixels(
        &mut self,
        name: String,
        map_type: u8,
        semantic: u8,
        ns: AssetNamespace,
        px: fastfile_iw6::image::ImagePixels,
    ) -> Result<usize, String> {
        let gpu = gpu_image(&px, semantic == TS_COLOR_MAP)?;
        self.decoded += 1;
        Ok(self.catalog.link_image(AuthoredImage {
            namespace: ns,
            name: AssetRef::Real(name),
            map_type,
            semantic,
            category: 0,
            use_srgb_reads: semantic == TS_COLOR_MAP,
            width: px.width.min(u32::from(u16::MAX)) as u16,
            height: px.height.min(u32::from(u16::MAX)) as u16,
            depth: 1,
            level_count: px.levels.min(255) as u8,
            format: 0,
            payload: Arc::new(Vec::new()),
            decoded: Some(Arc::new(gpu)),
            common_owned: false,
            decoded_variant: None,
            decoded_by: None,
            pending_decode: None,
        }))
    }

    /// One picture of the zone into the catalog (`None` when its pixels
    /// are not here: a missing pak, a shared zone not installed).
    pub fn image(
        &mut self,
        index: usize,
        max: u32,
        semantic: u8,
        ns: AssetNamespace,
    ) -> Option<usize> {
        if let Some(slot) = self.images.get(&(index, max, semantic)) {
            return *slot;
        }
        let rec: &ImageRec = self.cap.images.get(index)?;
        // (`patch_<map>` loads after the map: its picture of a name wins.)
        if !fastfile_h1::shared::is_reference(rec)
            && self
                .shared
                .as_ref()
                .is_some_and(|s| s.images.replaces_map(&rec.name))
        {
            let name = rec.name.clone();
            let slot = self.named_image(&name, max, semantic, ns);
            self.images.insert((index, max, semantic), slot);
            return slot;
        }
        let pixels = if fastfile_h1::shared::is_reference(rec) {
            match fastfile_h1::shared::builtin_pixels(&rec.name) {
                Some(px) => Ok(px),
                None => match self.shared.as_ref() {
                    Some(s) => s.images.pixels(&rec.name, &mut self.paks, max),
                    None => Err("held by a shared zone, none read".to_owned()),
                },
            }
            .inspect(|_| self.from_shared += 1)
        } else if let Some(px) = fastfile_h1::shared::builtin_pixels(&rec.name).filter(|_| self.retail_standins) {
            // (`$black`, `$identitynormalmap` ... are the engine's, whatever
            // a Horizon MW zone holds under the name: pure red 1 x 1
            // pictures, drawn as a red square by the sticker material on
            // every gun.)
            Ok(px)
        } else if fastfile_h1::shared::is_camo_slot(&rec) {
            Ok(fastfile_h1::shared::neutral_pixels(&rec.name))
        } else if let Some(real) = self.standin_for(&rec, max) {
            // (A 1 x 1 red stand-in: the retail zone's picture of the name.)
            real.inspect(|_| self.from_shared += 1)
        } else if self.retail_standins && fastfile_h1::shared::is_standin(&rec) {
            // (A stand-in none of the zones holds the picture of: neutral, not red.)
            Ok(fastfile_h1::shared::neutral_pixels(&rec.name))
        } else {
            // (A usermap's own pure red 1 x 1 picture is Horizon MW's stand-in
            // for a retail one it could not ship: the red windows. Neutral.)
            fastfile_h1::image::image_pixels(&rec, self.image, &mut self.paks, max).map(|px| {
                let red = px.width == 1
                    && px.height == 1
                    && fastfile_h1::shared::is_standin(&rec)
                    && px.data.len() >= 4
                    && px.data[0] > 200
                    && px.data[1] < 60
                    && px.data[2] < 60;
                if red { fastfile_h1::shared::neutral_pixels(&rec.name) } else { px }
            })
        };
        let name = format!("h1:{}#{max}", rec.name.trim_start_matches(','));
        let map_type = rec.map_type;
        let slot = match pixels
            .and_then(|px| self.link_pixels(name, map_type, semantic, ns, as_iw6(px)))
        {
            Ok(slot) => Some(slot),
            Err(e) => {
                self.image_failures.push(format!("{}: {e}", rec.name));
                None
            }
        };
        self.images.insert((index, max, semantic), slot);
        slot
    }

    /// A picture a shared material names, from the shared zones.
    fn named_image(
        &mut self,
        name: &str,
        max: u32,
        semantic: u8,
        ns: AssetNamespace,
    ) -> Option<usize> {
        let key = (fastfile_h1::shared::image_key(name), max, semantic);
        if let Some(slot) = self.named_images.get(&key) {
            return *slot;
        }
        // The map's own picture of the name (a retail map's materials are
        // defined in `techsets_<map>`, their pictures in the map), unless
        // a zone loaded after the map replaces it.
        let replaced = self
            .shared
            .as_ref()
            .is_some_and(|s| s.images.replaces_map(name));
        if !replaced && let Some(&index) = self.zone_images.get(&key.0) {
            let slot = self.image(index, max, semantic, ns);
            self.named_images.insert(key, slot);
            return slot;
        }
        let pixels = match fastfile_h1::shared::builtin_pixels(name) {
            Some(px) => Ok(px),
            None => match &self.shared {
                Some(s) => s.images.pixels(name, &mut self.paks, max),
                None => Err("no shared zones".to_owned()),
            },
        };
        let map_type = self
            .shared
            .as_ref()
            .and_then(|s| s.images.get(name))
            .map_or(3, |r| r.map_type);
        let link = format!("h1:{}#{max}", name.trim_start_matches(','));
        let slot = match pixels
            .and_then(|px| self.link_pixels(link, map_type, semantic, ns, as_iw6(px)))
        {
            Ok(slot) => {
                self.from_shared += 1;
                Some(slot)
            }
            Err(e) => {
                self.image_failures
                    .push(format!("{}: {e}", name.trim_start_matches(',')));
                None
            }
        };
        self.named_images.insert(key, slot);
        slot
    }

    fn image_of(
        &mut self,
        image: &MatImage,
        max: u32,
        semantic: u8,
        ns: AssetNamespace,
    ) -> Option<usize> {
        match image {
            MatImage::Zone(i) => self.image(*i, max, semantic, ns),
            MatImage::Named(name) => self.named_image(name, max, semantic, ns),
        }
    }

    /// The record of a material's picture wherever it is held.
    fn locate(&self, image: &MatImage) -> Option<&ImageRec> {
        let shared = |name: &str| self.shared.as_ref().and_then(|s| s.images.get(name));
        match image {
            MatImage::Zone(i) => {
                let rec = self.cap.images.get(*i)?;
                if fastfile_h1::shared::is_reference(rec) {
                    shared(&rec.name)
                } else {
                    Some(rec)
                }
            }
            MatImage::Named(name) => {
                let key = fastfile_h1::shared::image_key(name);
                if self
                    .shared
                    .as_ref()
                    .is_some_and(|s| s.images.replaces_map(name))
                {
                    return shared(name);
                }
                match self.zone_images.get(&key) {
                    Some(&i) => self.cap.images.get(i),
                    None => shared(name),
                }
            }
        }
    }

    /// Whether a linked material's colour map (`wanted`, if it has one)
    /// was bound (`textures`), and if not, why.
    fn colour_state(
        &self,
        unresolved: bool,
        wanted: Option<&MatImage>,
        textures: &[MaterialTextureBinding],
    ) -> ColourMap {
        if textures.iter().any(|t| t.semantic == TS_COLOR_MAP) {
            return ColourMap::Decoded;
        }
        let Some(image) = wanted else {
            return ColourMap::Missing(if unresolved {
                "material in no zone read".to_owned()
            } else {
                "material names no colour map".to_owned()
            });
        };
        let name = match image {
            MatImage::Zone(i) => self
                .cap
                .images
                .get(*i)
                .map_or_else(String::new, |r| r.name.clone()),
            MatImage::Named(n) => n.clone(),
        };
        let name = name.trim_start_matches(',').to_owned();
        match self.locate(image) {
            None => ColourMap::Missing(format!("{name}: in no zone read")),
            Some(rec) if rec.streamed => match rec
                .stream_files
                .iter()
                .flatten()
                .map(|f| self.paks.path_of(f.file_index))
                .find(|p| !p.is_file())
            {
                Some(pak) => ColourMap::PakAbsent(pak.display().to_string()),
                None => ColourMap::Missing(format!("{name}: streamed pixels do not read")),
            },
            Some(_) => ColourMap::Missing(format!("{name}: pixels do not decode")),
        }
    }

    fn bind(
        &mut self,
        wanted: Vec<(u32, u8, u8, MatImage)>,
        max: u32,
        ns: AssetNamespace,
        packed_spec: bool,
    ) -> Vec<MaterialTextureBinding> {
        let mut textures = Vec::new();
        for (hash, sampler_state, semantic, image) in wanted {
            // A packed specular picture (an `os0` technique set: strength,
            // gloss, occlusion in red, green, blue) unpacked: read as a
            // colour it was a blue mirror (the props' purple sheen).
            if semantic == TS_SPECULAR_MAP && packed_spec {
                let name = match &image {
                    MatImage::Zone(i) => self.cap.images.get(*i).map(|r| r.name.clone()).unwrap_or_default(),
                    MatImage::Named(n) => n.clone(),
                };
                let key = (format!("{}#os", fastfile_h1::shared::image_key(&name)), max, semantic);
                let slot = match self.named_images.get(&key) {
                    Some(s) => *s,
                    None => {
                        let made = self
                            .pixels_of(&image, max)
                            .and_then(|px| fastfile_h1::image::unpack_spec(&px))
                            .and_then(|px| self.link_pixels(format!("h1:{}#os{max}", name.trim_start_matches(',')), 3, semantic, ns, as_iw6(px)));
                        let slot = made.map_err(|e| self.image_failures.push(format!("{name}: {e}"))).ok();
                        self.named_images.insert(key, slot);
                        slot
                    }
                };
                if let Some(slot) = slot {
                    textures.push(MaterialTextureBinding { name_hash: hash, name_start: 0, name_end: 0, sampler_state, semantic, image: Some(slot) });
                }
                continue;
            }
            // A specular map built over `$white` (`~$white-r32g32b32&<x>`):
            // its colour is the placeholder's white, not a reflectance (as
            // read, every surface was a mirror: milky, the red toolbox
            // pink); a dielectric's (0.04) with its gloss kept.
            if semantic == TS_SPECULAR_MAP {
                let name = match &image {
                    MatImage::Zone(i) => self.cap.images.get(*i).map(|r| r.name.clone()).unwrap_or_default(),
                    MatImage::Named(n) => n.clone(),
                };
                if name.contains("$white-r32g32b32") {
                    let key = (format!("{}#dielectric", fastfile_h1::shared::image_key(&name)), max, semantic);
                    let slot = match self.named_images.get(&key) {
                        Some(s) => *s,
                        None => {
                            let made = self
                                .pixels_of(&image, max)
                                .and_then(|px| fastfile_h1::image::with_rgb(&px, [if mwr_reads() { DIELECTRIC_SPECULAR_MWR } else { DIELECTRIC_SPECULAR }; 3]))
                                .and_then(|px| self.link_pixels(format!("h1:{}#dielectric{max}", name.trim_start_matches(',')), 3, semantic, ns, as_iw6(px)));
                            let slot = made.map_err(|e| self.image_failures.push(format!("{name}: {e}"))).ok();
                            self.named_images.insert(key, slot);
                            slot
                        }
                    };
                    if let Some(slot) = slot {
                        textures.push(MaterialTextureBinding { name_hash: hash, name_start: 0, name_end: 0, sampler_state, semantic, image: Some(slot) });
                    }
                    continue;
                }
            }
            let read_as = if semantic == TS_NORMAL_MAP || semantic == TS_SPECULAR_MAP {
                semantic
            } else {
                TS_COLOR_MAP
            };
            if let Some(slot) = self.image_of(&image, max, read_as, ns) {
                textures.push(MaterialTextureBinding {
                    name_hash: hash,
                    name_start: 0,
                    name_end: 0,
                    sampler_state,
                    semantic,
                    image: Some(slot),
                });
            }
        }
        textures
    }

    #[allow(clippy::too_many_arguments)]
    fn link(
        &mut self,
        name: String,
        ns: AssetNamespace,
        sort_key: u8,
        textures: Vec<MaterialTextureBinding>,
        constants: &[MaterialConstant],
        draw: T6Draw,
    ) -> usize {
        self.catalog.link_material(AuthoredMaterial {
            name: AssetRef::Real(name),
            namespace: ns,
            technique_set: AssetRef::default(),
            technique_set_edge: AssetEdge::Absent,
            draw_surf: 0,
            sort_key,
            info_game_flags: 0,
            texture_atlas: None,
            surface_type_bits: None,
            t5_layered_surface_types: None,
            state_flags: 0,
            camera_region: 0,
            state_bits: Vec::new(),
            state_bits_entry: None,
            t5_state_bits_entry: None,
            iw5_state_bits_entry: None,
            technique_table: None,
            route: None,
            textures,
            constants: constants
                .iter()
                .map(|c| {
                    let mut short = [0u8; 12];
                    for (d, b) in short.iter_mut().zip(c.name.bytes()) {
                        *d = b;
                    }
                    asset_material::MaterialConstant {
                        name_hash: c.name_hash,
                        name: short,
                        literal: c.literal,
                    }
                })
                .collect(),
            zone: self.owner,
            t6_draw: Some(draw),
        })
    }

    /// A world surface's material: drawn as its technique set names it
    /// (`wc_l_sm_r0c0..` lit, `t0` alpha tested, `b0` blended, `b1c1`
    /// layers with the vertex colour as their weights) with its colour map
    /// and its layers'. A decal (a surface of the world's decal range) sits
    /// one polygon offset step toward the eye. `None` when the material is
    /// not in the zone.
    pub fn world_material(&mut self, index: usize, max: u32, decal: bool) -> Option<usize> {
        if let Some(&i) = self.materials.get(&(index | WORLD)) {
            return Some(i);
        }
        let m = self.view(index)?;
        let tech = ghosts_layer_tokens(&m.techset);
        let mut draw = T6Draw::from_technique_set(&tech, m.sort_key, None);
        if decal && draw.polygon_offset == 0 {
            draw.polygon_offset = 1;
        }
        let base = tech.split('_').find(|t| {
            let b = t.as_bytes();
            b.len() >= 4
                && matches!(b[0], b'r' | b'b' | b't')
                && b[1] == b'0'
                && (b[2] == b'c' || b[2] == b'h')
        });
        match base.map(|t| t.as_bytes()[0]) {
            Some(b'r') if draw.blend == T6Blend::Blend => draw.blend = T6Blend::Opaque,
            Some(b't') => {
                draw.alpha_test = true;
                if draw.blend == T6Blend::Blend {
                    draw.blend = T6Blend::Opaque;
                }
            }
            _ => {}
        }
        draw.anim = super::iw6::iw6_world_anim(&draw, &tech, &m.name, m.constants.iter().map(|c| (c.name.as_str(), c.literal)));
        let mut wanted = Vec::new();
        if let Some(t) = m
            .textures
            .iter()
            .find(|t| t.0 == COLOR_MAP_HASH)
            .or_else(|| m.textures.iter().find(|t| t.2 == TS_COLOR_MAP))
        {
            wanted.push((t.0, t.1, TS_COLOR_MAP, t.3.clone()));
        }
        if draw.layered {
            for (hash, semantic) in super::t6_materials::LAYER_MAPS {
                if let Some(t) = m.textures.iter().find(|t| t.0 == hash) {
                    wanted.push((hash, t.1, semantic, t.3.clone()));
                }
            }
        } else if !draw.unlit && self.world_shine {
            // Its normal and specular maps (gloss in the specular's
            // alpha): the lit world shader's bumps, highlights and
            // reflections (Black Ops III's look, `h1_map`).
            for (hash, semantic) in [(NORMAL_MAP_HASH, TS_NORMAL_MAP), (SPECULAR_MAP_HASH, TS_SPECULAR_MAP)] {
                if let Some(t) = m.textures.iter().find(|t| t.0 == hash) {
                    wanted.push((hash, t.1, semantic, t.3.clone()));
                }
            }
        }
        let colour = wanted
            .first()
            .filter(|w| w.2 == TS_COLOR_MAP)
            .map(|w| w.3.clone());
        // A reveal decal (`v0` in its base token: `b0c0v0n0sd0`): its
        // reveal map into the colour map's alpha, which the vertex alpha
        // then thresholds (`t7_reveal` in the shader).
        let reveal_map = base
            .filter(|t| t.starts_with('b') && t.contains("v0"))
            .and_then(|_| m.textures.iter().find(|t| t.0 == REVEAL_MAP_HASH))
            .map(|t| t.3.clone());
        let mut revealed = None;
        if let (Some(c), Some(r), false) = (colour.as_ref(), reveal_map.as_ref(), draw.layered) {
            let joined = self
                .pixels_of(c, max)
                .and_then(|cp| Ok((cp, self.pixels_of(r, max)?)))
                .and_then(|(cp, rp)| fastfile_h1::image::with_alpha_of(&cp, &rp));
            match joined.and_then(|px| {
                self.link_pixels(format!("h1:{}#reveal{max}", m.name.trim_start_matches(',')), 3, TS_COLOR_MAP, AssetNamespace::T6, as_iw6(px))
            }) {
                Ok(slot) => {
                    revealed = Some(MaterialTextureBinding {
                        name_hash: COLOR_MAP_HASH,
                        name_start: 0,
                        name_end: 0,
                        sampler_state: wanted[0].1,
                        semantic: TS_COLOR_MAP,
                        image: Some(slot),
                    });
                    wanted.remove(0);
                }
                Err(e) => self.image_failures.push(format!("{} reveal: {e}", m.name.trim_start_matches(','))),
            }
        }
        let packed = m.techset.contains("os0");
        let mut textures = self.bind(wanted, max, AssetNamespace::T6, packed);
        if let Some(r) = revealed.clone() {
            textures.insert(0, r);
        }
        let state = self.colour_state(m.unresolved, colour.as_ref(), &textures);
        let at = self.link(
            format!("h1:{}", m.name.trim_start_matches(',')),
            AssetNamespace::T6,
            m.sort_key,
            textures,
            &[],
            draw,
        );
        self.colour.insert(at, state);
        if revealed.is_some() {
            self.reveal.insert(at);
        }
        self.materials.insert(index | WORLD, at);
        Some(at)
    }

    /// A model's material, with its colour, normal and specular maps,
    /// under `self.ns`.
    pub fn material(&mut self, index: usize, max: u32) -> Option<usize> {
        if let Some(&i) = self.materials.get(&index) {
            return Some(i);
        }
        let m = self.view(index)?;
        let draw = iw6_model_draw(&m.techset, m.sort_key);
        let mut wanted = Vec::new();
        if let Some(t) = m
            .textures
            .iter()
            .find(|t| t.0 == COLOR_MAP_HASH)
            .or_else(|| m.textures.iter().find(|t| t.2 == TS_COLOR_MAP))
        {
            wanted.push((t.0, t.1, TS_COLOR_MAP, t.3.clone()));
        }
        if !draw.unlit {
            for (hash, semantic) in [
                (NORMAL_MAP_HASH, TS_NORMAL_MAP),
                (SPECULAR_MAP_HASH, TS_SPECULAR_MAP),
            ] {
                if let Some(t) = m.textures.iter().find(|t| t.0 == hash) {
                    wanted.push((t.0, t.1, semantic, t.3.clone()));
                }
            }
        }
        let ns = self.ns;
        let colour = wanted
            .first()
            .filter(|w| w.2 == TS_COLOR_MAP)
            .map(|w| w.3.clone());
        let packed = m.techset.contains("os0");
        let textures = self.bind(wanted, max, ns, packed);
        let state = self.colour_state(m.unresolved, colour.as_ref(), &textures);
        let at = self.link(
            format!("{}{}", self.name_prefix, m.name.trim_start_matches(',')),
            ns,
            m.sort_key,
            textures,
            &m.constants,
            draw,
        );
        self.colour.insert(at, state);
        self.materials.insert(index, at);
        Some(at)
    }

    /// A material's colour map, by the material's name, as RGBA (its top
    /// level no larger than `max`): the map's own material or the one the
    /// zones beside it define (a retail map's `compass_map_<map>` is in
    /// `techsets_<map>`, its picture `h1_og_compass_map_<map>` in the map).
    /// The picture's name and its pixels, or why not.
    pub fn material_picture(&mut self, material: &str, max: u32) -> Result<(String, Rgba), String> {
        let key = fastfile_h1::shared::image_key(material);
        let index = self
            .cap
            .materials
            .iter()
            .position(|m| fastfile_h1::shared::image_key(&m.name) == key)
            .ok_or_else(|| format!("no material {material} in the zone"))?;
        let m = self
            .view(index)
            .ok_or_else(|| format!("{material} unreadable"))?;
        if m.unresolved {
            return Err(format!("{material} is defined in no zone read"));
        }
        let image = m
            .textures
            .iter()
            .find(|t| t.0 == COLOR_MAP_HASH)
            .or_else(|| m.textures.iter().find(|t| t.2 == TS_COLOR_MAP))
            .or_else(|| m.textures.first())
            .map(|t| t.3.clone())
            .ok_or_else(|| format!("{material} names no picture"))?;
        let rec = self
            .locate(&image)
            .cloned()
            .ok_or_else(|| format!("{material}'s picture is in no zone read"))?;
        let name = rec.name.trim_start_matches(',').to_owned();
        let in_zone = matches!(&image, MatImage::Zone(i) if !fastfile_h1::shared::is_reference(&self.cap.images[*i]))
            || (matches!(&image, MatImage::Named(n) if self.zone_images.contains_key(&fastfile_h1::shared::image_key(n)))
                && !self
                    .shared
                    .as_ref()
                    .is_some_and(|s| s.images.replaces_map(&name)));
        let px = if in_zone {
            fastfile_h1::image::image_pixels(&rec, self.image, &mut self.paks, max)?
        } else {
            match &self.shared {
                Some(s) => s.images.pixels(&name, &mut self.paks, max)?,
                None => return Err(format!("{name} is in a shared zone, none read")),
            }
        };
        let rgba = fastfile_h1::ui_image::decode_rgba(&px)
            .ok_or_else(|| format!("{name}: DXGI format {} not decoded", px.dxgi_format))?;
        Ok((name, rgba))
    }

    /// The map's sky: the first of the world's sky images that is a cube
    /// (`map_type` 5: six faces one after another, sRGB colour or BC1 on
    /// the usermaps) under a material whose draw is Black Ops II's sky (the
    /// sky pass draws it behind the world). Its name and the material.
    pub fn sky_material(
        &mut self,
        sky_images: &[Option<AssetId>],
    ) -> Result<(String, usize, [u32; 3]), String> {
        let rec: &ImageRec = sky_images
            .iter()
            .flatten()
            .filter_map(|id| self.cap.images.get(id.index as usize))
            .find(|r| r.map_type == 5)
            .ok_or("no cube among the world's sky images")?;
        let name = rec.name.clone();
        let px = if rec.streamed {
            fastfile_h1::image::image_pixels(rec, self.image, &mut self.paks, 1 << 14)?
        } else {
            // (All six faces: `image_pixels` reads one 2-D image's levels.)
            fastfile_h1::image::ImagePixels {
                width: u32::from(rec.width),
                height: u32::from(rec.height),
                levels: u32::from(rec.level_count.max(1)),
                dxgi_format: rec.format,
                data: rec
                    .pixels
                    .ok_or("the sky has no pixels in the zone")?
                    .get(self.image)
                    .to_vec(),
            }
        };
        let shape = [px.width, px.height, px.dxgi_format];
        let m = super::iw6::link_sky(
            &mut self.catalog,
            &format!("h1:sky:{name}"),
            &as_iw6(px),
            self.owner,
        )
        .map_err(|e| format!("{name}: {e}"))?;
        self.decoded += 1;
        Ok((name, m, shape))
    }

    /// The parts a model draws: its own first LOD, or for a composite
    /// model (H1's guns: base, sight, stock ...) each part's.
    fn model_parts(&self, m: &'a XModelRec) -> Vec<&'a XModelRec> {
        if m.composite.is_empty() {
            return vec![m];
        }
        m.composite
            .iter()
            .flatten()
            .filter_map(|id| self.cap.xmodels.get(id.index as usize))
            .collect()
    }

    /// A model's first LOD in Black Ops II's captured form, and the
    /// capture materials its surfaces use (composite models' parts drawn
    /// together in the first part's bone space).
    pub fn model_ref(&self, m: &'a XModelRec) -> Option<(asset_t6::XModelRef, Vec<usize>)> {
        let material_key = |k: usize| asset_t6::AssetKey {
            ty: fastfile_t6::AssetType::Material,
            index: k,
        };
        let mut keys = Vec::new();
        let mut lod0 = Vec::new();
        let mut materials = Vec::new();
        for part in self.model_parts(m) {
            let Some(lod) = part.lods.first() else {
                continue;
            };
            let Some(surfs) = lod
                .surfs
                .and_then(|s| self.cap.model_surfs.get(s.index as usize))
            else {
                continue;
            };
            for (i, s) in surfs.surfs.iter().enumerate() {
                let verts: Vec<u8> = s
                    .verts
                    .map_or(&[][..], |sp| sp.get(self.image))
                    .chunks_exact(32)
                    .flat_map(iw6_vertex_to_t6)
                    .collect();
                let indices = le_u16s(s.tris.map_or(&[][..], |sp| sp.get(self.image)));
                let blend_words = le_u16s(s.blend_verts.map_or(&[][..], |sp| sp.get(self.image)));
                let (blend_counts, blend) = fold_blend(s.blend_vert_counts, &blend_words);
                let material = part
                    .materials
                    .get(usize::from(lod.surf_index) + i)
                    .copied()
                    .flatten()
                    .map(|id| id.index as usize);
                keys.extend(material);
                materials.push(material.map(material_key));
                let mut part_bits = [0u32; 6];
                part_bits.copy_from_slice(&s.part_bits[..6]);
                lod0.push(asset_t6::XSurfaceRef {
                    verts,
                    vert_count: s.vert_count,
                    indices,
                    vert_lists: s.rigid.clone(),
                    flags: 0,
                    material: material.map(material_key),
                    blend_counts,
                    blend,
                    part_bits,
                });
            }
        }
        if lod0.is_empty() {
            return None;
        }
        let (mid, half) = m.bounds;
        let bone_names: Vec<String> = m.bone_names.iter().map(|&s| self.string(s)).collect();
        Some((
            asset_t6::XModelRef {
                lighting_origin_offset: [0.0; 3],
                lighting_origin_range: 0.0,
                name: m.name.trim_start_matches(',').to_owned(),
                numsurfs: lod0.len().min(255) as u8,
                materials,
                num_bones: m.num_bones,
                num_lods: 1,
                mins: std::array::from_fn(|k| mid[k] - half[k]),
                maxs: std::array::from_fn(|k| mid[k] + half[k]),
                radius: m.radius,
                lods: vec![(0.0, lod0.len() as u16, 0)],
                lod0,
                bone_names,
                num_root_bones: m.num_root_bones,
                parent_list: m.parent_list.clone(),
                quats: m.tag_angles.clone(),
                trans: m.tag_positions.clone(),
                base_mat: m
                    .base_mat
                    .iter()
                    .map(|b| ([b[0], b[1], b[2], b[3]], [b[4], b[5], b[6]]))
                    .collect(),
                part_classification: m.part_classification.clone(),
                bone_info: vec![None; usize::from(m.num_bones)],
                contents: m.contents as u32,
                coll_lod: i16::from(m.coll_lod),
                coll_surfs: Vec::new(),
                phys_preset: None,
            },
            keys,
        ))
    }

    /// An animation in Black Ops II's captured form (H1's parts pack as
    /// Ghosts' and AW's).
    pub fn anim_ref(&self, a: &XAnimRec) -> asset_t6::XAnimRef {
        let bytes = |s: Option<fastfile_h1::walk::Span>| s.map_or(&[][..], |s| s.get(self.image));
        let indices = if a.numframes >= 256 {
            le_u16s(bytes(a.indices))
        } else {
            bytes(a.indices).iter().map(|&b| u16::from(b)).collect()
        };
        asset_t6::XAnimRef {
            delta_quat: Vec::new(),
            name: a.name.trim_start_matches(',').to_owned(),
            numframes: a.numframes,
            framerate: a.framerate,
            looping: a.flags & 1 != 0,
            delta: false,
            delta_trans: Vec::new(),
            bone_count: a.bone_count,
            names: a.names.iter().map(|&s| self.string(s)).collect(),
            notifies: a.notify.iter().map(|&(s, t)| (self.string(s), t)).collect(),
            data_byte: bytes(a.data_byte).to_vec(),
            data_short: le_u16s(bytes(a.data_short)),
            data_int: le_u32s(bytes(a.data_int)),
            random_data_byte: bytes(a.random_data_byte).to_vec(),
            random_data_short: le_u16s(bytes(a.random_data_short)),
            random_data_int: le_u32s(bytes(a.random_data_int)),
            indices,
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn usermap_zone_dir_is_found_up_the_tree() {
        let root = std::env::temp_dir().join(format!("h1_zone_dir_{}", std::process::id()));
        let map_dir = root.join("hmw-usermaps").join("mp_x");
        let zone = root.join("zone");
        std::fs::create_dir_all(&map_dir).unwrap();
        std::fs::create_dir_all(&zone).unwrap();
        std::fs::write(zone.join("common_mp.ff"), b"").unwrap();
        let found = super::h1_zone_dir(&map_dir.join("mp_x.ff"));
        assert_eq!(found.as_deref(), Some(zone.as_path()));
        assert_eq!(
            super::h1_zone_dir(&zone.join("mp_crash.ff")).as_deref(),
            Some(zone.as_path())
        );
        let _ = std::fs::remove_dir_all(&root);
    }
}
