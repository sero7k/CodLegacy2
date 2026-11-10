//! A Black Ops III map zone (`mp_*.ff`, `zm_*.ff`) read whole, in plain
//! terms a converter can use: the drawn world (vertices, indices, surfaces
//! and their materials), the placed static models, the materials with their
//! image names (pixels from the zone or the `.xpak` packages, `texture`),
//! the collision (brushes, BSP, triangle mesh), the entities and the sun.
//!
//! The zone's walk (`walk`) keeps file offsets of the map assets' arrays
//! (`walk::world`, `walk::gfxworld`); this reads them.
//!
//! World vertices (read on `mp_nuketown_x`): positions (12 bytes) in one
//! buffer, attributes (20 bytes: colour, UV as two floats, normal and
//! tangent as unsigned 10:10:10 over 1023) in another; a surface's
//! vertices start at its position offset / 12, its triangles index the
//! index buffer from its base index, relative to that start. Several
//! surfaces share a vertex range: those have a vertex count of 0 (the count
//! is then the largest index + 1).

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::texture::{self, ImageLevels};
use crate::walk::capture::{Capture, ImageRec};
use crate::walk::world::ClipMapRec;
use crate::xpak::XpakSet;

/// One world vertex, unpacked.
#[derive(Clone, Copy, Debug, Default)]
pub struct WorldVertex {
    pub xyz: [f32; 3],
    /// RGBA bytes as stored.
    pub color: u32,
    pub uv: [f32; 2],
    pub normal: [f32; 3],
    pub tangent: [f32; 3],
}

/// Black Ops III's packed unit vector: unsigned 10:10:10 over 1023.
pub fn unpack_unit(packed: u32) -> [f32; 3] {
    std::array::from_fn(|k| ((packed >> (10 * k)) & 0x3ff) as f32 / 1023.0 * 2.0 - 1.0)
}

/// One world surface: its triangles index the index buffer from
/// `base_index`, relative to `first_vertex`.
#[derive(Clone, Copy, Debug, Default)]
pub struct WorldSurface {
    pub first_vertex: u32,
    pub vertex_count: u32,
    pub tri_count: u16,
    pub base_index: u32,
    pub material: Option<usize>,
    pub flags: u8,
    pub mins: [f32; 3],
    pub maxs: [f32; 3],
}

/// A material: its technique set's name and its textures (name hash, image).
#[derive(Clone, Debug, Default)]
pub struct MapMaterial {
    pub name: String,
    pub technique_set: String,
    pub textures: Vec<(u32, String)>,
    /// bo2mp: the material's own numbers by the name its shaders give them
    /// (`colorTint`, `glossRange`, `detailScale1`, `baseNormalHeight`, ...):
    /// its `$Globals` buffers read with the layout its technique's programs
    /// declare (`crate::dxbc`). Empty when the technique set's programs are
    /// not in the zone.
    pub constants: HashMap<String, Vec<f32>>,
    /// bo2mp: the texture slots its technique set's pixel programs bind
    /// (`colorMap`, `specColorMap`, `glossMap`, `detailMap1`, ...).
    pub bound: Vec<String>,
}

impl MapMaterial {
    /// A constant's first `N` floats.
    pub fn constant<const N: usize>(&self, name: &str) -> Option<[f32; N]> {
        let v = self.constants.get(name)?;
        (v.len() >= N).then(|| std::array::from_fn(|k| v[k]))
    }
    /// The image in the texture slot called `slot` (by `R_HashString`),
    /// engine placeholders (`$...`) left out.
    pub fn texture(&self, slot: &str) -> Option<&str> {
        let h = hash_string(slot);
        self.textures.iter().find(|(k, n)| *k == h && !n.starts_with('$')).map(|(_, n)| n.as_str())
    }
    /// The placeholder or image in the slot, as the zone names it.
    pub fn texture_raw(&self, slot: &str) -> Option<&str> {
        let h = hash_string(slot);
        self.textures.iter().find(|(k, _)| *k == h).map(|(_, n)| n.as_str())
    }
}

/// `R_HashString`: case-folded, each byte xor 33 times the running hash
/// (the texture table's slot names).
pub fn hash_string(s: &str) -> u32 {
    s.bytes().fold(0u32, |h, c| u32::from(c.to_ascii_lowercase()) ^ h.wrapping_mul(33))
}

/// bo2mp: per technique (12) of a technique set, its vertex and pixel
/// programs' reflection.
pub type TechsetLayout = Vec<[Option<crate::dxbc::Reflection>; 2]>;

/// A technique set's programs reflected (`crate::dxbc`).
fn techset_layout(ts: &crate::walk::capture::TechsetRec, image: &[u8]) -> TechsetLayout {
    let reflect = |p: Option<(usize, usize)>| p.and_then(|(at, n)| image.get(at..at + n)).and_then(crate::dxbc::reflect);
    ts.programs
        .iter()
        .map(|t| match t {
            Some([vs, ps]) => [reflect(*vs), reflect(*ps)],
            None => [None, None],
        })
        .collect()
}

/// bo2mp: the technique sets of the zones a map's materials borrow theirs
/// from (a map's `,mc/lit...` technique sets are references to the ones
/// the zones loaded before it hold: `mc/lit` itself is in `core_pre_gfx`,
/// most others in `core_common`, `mp_common`, `zm_levelcommon`), by name,
/// read once per folder. (A zone whose walk stops still gives the
/// technique sets it read before the stop.)
/// The shared zones a map's materials, effects and pictures can come from.
pub const SHARED_ZONES: [&str; 10] = ["core_pre_gfx", "core_gfx", "core_common", "mp_common", "zm_common", "zm_levelcommon", "core_post_gfx", "core_ui", "core_patch", "mp_patch"];

/// `common_layouts` per folder.
static COMMON: std::sync::OnceLock<std::sync::Mutex<HashMap<PathBuf, std::sync::Arc<Common>>>> = std::sync::OnceLock::new();

impl Common {
    fn pixel_bytes(&self) -> usize {
        self.pixels.lock().unwrap_or_else(|poison| poison.into_inner()).values().map(|px| px.len()).sum()
    }

    /// A kept picture's pixels; once they were let go (`release_pixels`),
    /// read again from its zone.
    fn pixels_of(&self, name: &str) -> Option<std::sync::Arc<[u8]>> {
        if let Some(px) = self.pixels.lock().unwrap_or_else(|poison| poison.into_inner()).get(name) {
            return Some(px.clone());
        }
        if !self.released.load(std::sync::atomic::Ordering::Acquire) {
            return None;
        }
        let (zone, ..) = *self.sources.get(name)?;
        self.restore(zone);
        self.pixels.lock().unwrap_or_else(|poison| poison.into_inner()).get(name).cloned()
    }

    /// Reads `zone` again (the kept one, or its file) and puts back the
    /// pixels of every picture that came from it. Once per zone after a
    /// release: a zone that does not read is not tried again.
    fn restore(&self, zone: &'static str) {
        let mut restored = self.restored.lock().unwrap_or_else(|poison| poison.into_inner());
        if !restored.insert(zone) {
            return;
        }
        let t = std::time::Instant::now();
        let image = match crate::shared::held(&self.dir, zone) {
            Some(z) => z.image.clone(),
            None => match crate::Zone::open(&self.dir.join(format!("{zone}.ff"))) {
                Ok(z) => std::sync::Arc::new(z.image),
                Err(e) => {
                    eprintln!("memory: t7 shared-zone pictures of {zone} could not be read again: {e:?}");
                    return;
                }
            },
        };
        let mut pixels = self.pixels.lock().unwrap_or_else(|poison| poison.into_inner());
        let (mut count, mut bytes) = (0, 0);
        for (name, &(from, at, n)) in &self.sources {
            if from != zone || pixels.contains_key(name) {
                continue;
            }
            let Some(px) = image.get(at..at + n) else { continue };
            pixels.insert(name.clone(), std::sync::Arc::from(px));
            count += 1;
            bytes += n;
        }
        eprintln!(
            "memory: t7 shared-zone pictures of {zone} read again ({count} pictures, {:.1} MiB, {:.2}s): one was asked for after they were let go",
            bytes as f64 / (1024.0 * 1024.0),
            t.elapsed().as_secs_f32()
        );
    }

    /// bo2mp: lets go of the kept pictures' pixels, a copy of the shared
    /// zones' bytes (590 MB on Skyjacked), once the native renderer has
    /// loaded the pictures it needs up front; one asked for later (drawn
    /// lazily, a smaller retry, the next map) reads its zone again
    /// (`restore`), so what is drawn is the same.
    /// `BO2MP_T7_KEEP_COMMON_PIXELS=1` keeps them (the old behaviour).
    /// Returns the bytes let go.
    fn release_pixels(&self) -> usize {
        if std::env::var_os("BO2MP_T7_KEEP_COMMON_PIXELS").is_some_and(|v| v == "1") {
            return 0;
        }
        let mut restored = self.restored.lock().unwrap_or_else(|poison| poison.into_inner());
        self.released.store(true, std::sync::atomic::Ordering::Release);
        restored.clear();
        let taken = std::mem::take(&mut *self.pixels.lock().unwrap_or_else(|poison| poison.into_inner()));
        drop(restored);
        taken.values().map(|px| px.len()).sum()
    }
}

/// bo2mp: a map of another game loads (or none): what is kept for the next
/// Black Ops III map goes, the shared zones' layouts and pictures
/// (`common_layouts`) and the shared zones themselves, all read again by
/// the next one. Logs what went (`stderr`).
pub fn release_folder_caches() {
    let common = COMMON.get().and_then(|c| c.lock().ok().map(|mut c| std::mem::take(&mut *c))).unwrap_or_default();
    let bytes: usize = common.values().map(|c| c.pixel_bytes()).sum();
    let folders = common.len();
    drop(common);
    crate::shared::release();
    if folders > 0 {
        eprintln!("memory: t7 shared-zone pictures let go: {folders} folder(s), {:.1} MiB", bytes as f64 / (1024.0 * 1024.0));
    }
    // (The other zones' index is records only but a whole folder's; the
    // pictures it found hold their pixels.)
    let index = OTHER_INDEX.get().and_then(|c| c.lock().ok().map(|mut c| std::mem::take(&mut *c))).unwrap_or_default();
    let records: usize = index.values().filter_map(|i| i.get()).map(HashMap::len).sum();
    drop(index);
    let found = OTHER_FOUND.get().and_then(|f| f.lock().ok().map(|mut f| std::mem::take(&mut *f))).unwrap_or_default();
    let px: usize = found.values().flatten().map(|(_, p)| p.len()).sum();
    if records > 0 || !found.is_empty() {
        eprintln!("memory: t7 other zones' pictures let go: {records} indexed, {} found, {:.1} MiB of pixels", found.len(), px as f64 / (1024.0 * 1024.0));
    }
    // (So do the folders' package indexes, every xpak's hash table:
    // `BO2MP_T7_KEEP_PACKAGES=1` keeps them, the old behaviour.)
    if !std::env::var_os("BO2MP_T7_KEEP_PACKAGES").is_some_and(|v| v == "1") {
        let sets = PACKAGES.get().and_then(|c| c.lock().ok().map(|mut c| std::mem::take(&mut *c))).unwrap_or_default();
        let entries: usize = sets.values().flat_map(|s| &s.paks).map(|p| p.entries.len()).sum();
        if !sets.is_empty() {
            eprintln!(
                "memory: t7 package indexes let go: {} folder(s), {entries} entries ({:.1} MiB)",
                sets.len(),
                (entries * std::mem::size_of::<crate::xpak::XpakEntry>()) as f64 / (1024.0 * 1024.0)
            );
        }
    }
}

fn common_layouts(dir: &Path) -> std::sync::Arc<Common> {
    use std::sync::Arc;
    let cache = COMMON.get_or_init(Default::default);
    if let Some(c) = cache.lock().ok().and_then(|c| c.get(dir).cloned()) {
        return c;
    }
    let mut out = Common { dir: dir.to_owned(), ..Default::default() };
    // (The zones are read and walked side by side; the layouts are then
    // taken in order.)
    crate::shared::preload(dir, &SHARED_ZONES);
    for zone in SHARED_ZONES {
        // (Read and walked once per process, shared with the native renderer.)
        let Some(z) = crate::shared::zone(dir, zone) else { continue };
        let w = &z;
        for ts in &w.capture.techsets {
            out.layouts.entry(bare(&ts.name).to_owned()).or_insert_with(|| techset_layout(ts, &z.image));
        }
        keep_effects(&w.capture, &mut out.effects, &mut out.effect_materials, &mut out.light_descriptions, &mut out.impact_fields);
        // The pictures (their records; pixels in the zone kept as bytes).
        for im in &w.capture.images {
            if im.name.starts_with(',') || out.images.contains_key(&im.name) {
                continue;
            }
            let mut rec = im.clone();
            if im.xpak.is_empty() {
                let Some((at, n)) = im.pixels else { continue };
                let Some(px) = z.image.get(at..at + n) else { continue };
                out.pixels.get_mut().unwrap_or_else(|poison| poison.into_inner()).insert(im.name.clone(), std::sync::Arc::from(px));
                out.sources.insert(im.name.clone(), (zone, at, n));
                rec.pixels = Some((0, n));
                rec.fallback = None;
            }
            out.images.insert(im.name.clone(), rec);
        }
    }
    eprintln!("t7 shared zones: {} pictures kept, {:.1} MiB of their pixels", out.images.len(), out.pixel_bytes() as f64 / (1024.0 * 1024.0));
    let out = Arc::new(out);
    if let Ok(mut c) = cache.lock() {
        c.insert(dir.to_owned(), out.clone());
    }
    out
}

type OtherIndex = HashMap<String, (crate::walk::capture::ImageRec, PathBuf)>;
type OtherFound = HashMap<(PathBuf, String), Option<(crate::walk::capture::ImageRec, Vec<u8>)>>;
/// `other_zones_image`'s index per folder and the pictures it found.
static OTHER_INDEX: std::sync::OnceLock<std::sync::Mutex<HashMap<PathBuf, std::sync::Arc<std::sync::OnceLock<OtherIndex>>>>> = std::sync::OnceLock::new();
static OTHER_FOUND: std::sync::OnceLock<std::sync::Mutex<OtherFound>> = std::sync::OnceLock::new();

/// bo2mp: a picture some other zone of the folder defines (name -> record,
/// pixels when they are in the zone, providing zone), for a name neither the
/// map nor the common zones define. The index (records and the zone each is
/// in, no pixels) is built once, on the first such miss, by walking every
/// other zone one at a time; a found picture's pixels are read from its own
/// zone when asked for and kept by name. `BO2MP_T7_NO_IMAGE_FALLBACK=1`
/// turns it off. Logs the providers (`stderr`, once per name).
/// (The pictures load on many threads: the first miss builds the index and
/// the others wait for it, rather than each walking the whole folder. The
/// index is saved to a file, `other_index`, and read back next time. Logs
/// the build's or the read's time.)
fn other_zones_image(dir: &Path, name: &str) -> Option<(crate::walk::capture::ImageRec, Vec<u8>)> {
    type Index = OtherIndex;
    let (cache, found) = (&OTHER_INDEX, &OTHER_FOUND);
    if std::env::var("BO2MP_T7_NO_IMAGE_FALLBACK").is_ok_and(|v| v == "1") || dir.as_os_str().is_empty() {
        return None;
    }
    let found = found.get_or_init(Default::default);
    let key = (dir.to_owned(), name.to_owned());
    if let Some(f) = found.lock().ok().and_then(|f| f.get(&key).cloned()) {
        return f;
    }
    let cache = cache.get_or_init(Default::default);
    let slot = cache.lock().ok()?.entry(dir.to_owned()).or_default().clone();
    let index = slot.get_or_init(|| {
        let t = std::time::Instant::now();
        // (Saved to a small file after a walk: the next process, or the next
        // BO3 map after another game's let it go, reads it back instead of
        // walking every zone. `BO2MP_T7_OTHER_INDEX_CACHE=0` = always walk.)
        let saved = crate::other_index::enabled();
        let (names, folder_key) = crate::other_index::zones_and_key(dir);
        if saved && let Some(idx) = crate::other_index::load(dir, folder_key) {
            eprintln!("t7 image: the other zones' picture index read from its saved file ({} pictures) in {:.2}s", idx.len(), t.elapsed().as_secs_f32());
            return idx;
        }
        let mut idx = Index::new();
        let zones = names.len();
        for p in names {
            let Ok(z) = crate::Zone::open(&p) else { continue };
            let w = crate::walk::walk(&z);
            for im in &w.capture.images {
                if im.name.starts_with(',') || idx.contains_key(&im.name) {
                    continue;
                }
                if im.xpak.is_empty() && im.pixels.is_none() {
                    continue;
                }
                idx.insert(im.name.clone(), (im.clone(), p.clone()));
            }
        }
        eprintln!("t7 image: the other zones' picture index ({zones} zones, {} pictures) built in {:.1}s, for {name}", idx.len(), t.elapsed().as_secs_f32());
        if saved && zones > 0 {
            crate::other_index::save(dir, folder_key, &idx);
        }
        idx
    });
    let out = index.get(name).and_then(|(im, path)| {
        let mut rec = im.clone();
        let mut px = Vec::new();
        if im.xpak.is_empty() {
            let (at, n) = im.pixels?;
            let z = crate::Zone::open(path).ok()?;
            px = z.image.get(at..at + n)?.to_vec();
            rec.pixels = Some((0, n));
            rec.fallback = None;
        }
        let zone = path.file_stem().and_then(|s| s.to_str()).unwrap_or_default();
        eprintln!("t7 image: {name} is defined in zone {zone} (not in the map's zones)");
        Some((rec, px))
    });
    if let Ok(mut f) = found.lock() {
        f.insert(key, out.clone());
    }
    out
}

/// bo2mp: the effects a walk defined (not the references to other
/// zones'), and the materials their sprites name, each once.
fn keep_effects(
    cap: &Capture,
    effects: &mut HashMap<String, crate::walk::capture::FxRec>,
    materials: &mut HashMap<String, crate::walk::capture::MaterialRec>,
    lights: &mut HashMap<String, Vec<u8>>,
    impacts: &mut HashMap<(crate::AssetType, String), Vec<(usize, String)>>,
) {
    for (n, d) in &cap.light_descriptions {
        lights.entry(n.clone()).or_insert_with(|| d.clone());
    }
    for (k, f) in &cap.impact_fields {
        impacts.entry(k.clone()).or_insert_with(|| f.clone());
    }
    for fx in &cap.fx {
        if fx.elems.is_empty() || effects.contains_key(&fx.name) {
            continue;
        }
        for e in &fx.elems {
            if e.raw.get(0xc8).is_some_and(|&t| t <= 6) {
                for v in &e.visuals {
                    let n = bare(v);
                    if materials.contains_key(n) {
                        continue;
                    }
                    if let Some(m) = cap.material(n).filter(|m| !m.name.starts_with(',')) {
                        materials.insert(n.to_owned(), m.clone());
                    }
                }
            }
        }
        effects.insert(fx.name.clone(), fx.clone());
    }
}

/// bo2mp: what the zones loaded before a map give its materials: their
/// technique sets' layouts, and their pictures (the shared `i_t7_micro_*`
/// tiles, decals' grunge): records, and the pixels of those kept in the
/// zone (from offset 0 of their own bytes).
#[derive(Default)]
pub struct Common {
    pub layouts: HashMap<String, TechsetLayout>,
    images: HashMap<String, ImageRec>,
    pixels: std::sync::Mutex<HashMap<String, std::sync::Arc<[u8]>>>,
    /// Where each kept picture's pixels are: its zone, their offset and
    /// length (read again from there once let go: `Common::restore`).
    sources: HashMap<String, (&'static str, usize, usize)>,
    /// The folder the shared zones are in.
    dir: PathBuf,
    /// The pixels were let go (`Common::release_pixels`).
    released: std::sync::atomic::AtomicBool,
    /// The zones read again since.
    restored: std::sync::Mutex<std::collections::HashSet<&'static str>>,
    /// bo2mp: the effects the common zones define, and the materials
    /// their sprites draw with (`T7Map::effect`).
    effects: HashMap<String, crate::walk::capture::FxRec>,
    effect_materials: HashMap<String, crate::walk::capture::MaterialRec>,
    /// bo2mp: the light descriptions (FX light elements' lights).
    light_descriptions: HashMap<String, Vec<u8>>,
    impact_fields: HashMap<(crate::AssetType, String), Vec<(usize, String)>>,
}

/// bo2mp: a material's named constants and bound texture slots, from its
/// technique set's programs (`MapMaterial::constants`): the zone's own
/// technique set, else the common zones' (`common`).
pub(crate) fn material_constants(
    cap: &Capture,
    image: &[u8],
    common: &Common,
    m: &crate::walk::capture::MaterialRec,
) -> (HashMap<String, Vec<f32>>, Vec<String>) {
    let mut constants: HashMap<String, Vec<f32>> = HashMap::new();
    let mut bound: Vec<String> = Vec::new();
    let Some(name) = m.techset.as_deref() else { return (constants, bound) };
    let own;
    let reflected: &TechsetLayout = match cap.techset(name).filter(|t| t.programs.iter().any(Option::is_some)) {
        Some(ts) => {
            own = techset_layout(ts, image);
            &own
        }
        None => match common.layouts.get(bare(name)) {
            Some(l) => l,
            None => return (constants, bound),
        },
    };
    for r in reflected.iter().filter_map(|r| r[1].as_ref()) {
        for (name, _) in &r.textures {
            if !bound.contains(name) {
                bound.push(name.clone());
            }
        }
    }
    for (slot, data) in &m.globals {
        let t = usize::from(*slot) / 6;
        // The program of that technique whose `$Globals` is this buffer's
        // size (the pixel program first), else any technique's.
        let fits = |r: &crate::dxbc::Reflection| r.globals().is_some_and(|g| g.size as usize == data.len());
        let layout = reflected
            .get(t)
            .and_then(|r| [&r[1], &r[0]].into_iter().flatten().find(|r| fits(r)))
            .or_else(|| reflected.iter().flat_map(|r| [&r[1], &r[0]]).flatten().find(|r| fits(r)))
            .and_then(crate::dxbc::Reflection::globals);
        let Some(g) = layout else { continue };
        for v in &g.variables {
            let (o, n) = (v.offset as usize, v.size as usize);
            if o + n > data.len() || n == 0 || n % 4 != 0 {
                continue;
            }
            let value: Vec<f32> = data[o..o + n].chunks_exact(4).map(|b| f32::from_le_bytes(b.try_into().unwrap())).collect();
            // (The first technique's buffer names it, unless that one is
            // all zeros where a later one is not: Nuk3town's tree leaves
            // (`fxanim_vegetation`) carry zeros in their first technique's
            // buffer and their pink, red and orange `colorTint` in the
            // lit one's; read as zeros, the leaves drew white.)
            let zeros = |v: &[f32]| v.iter().all(|x| *x == 0.0);
            match constants.get(&v.name) {
                Some(old) if !(zeros(old) && !zeros(&value)) => {}
                _ => {
                    constants.insert(v.name.clone(), value);
                }
            }
        }
    }
    (constants, bound)
}

/// bo2mp: a material's named constants as its own zone's technique set
/// declares them (for dumping a gun camo's numbers: `t7_camo_consts`).
#[must_use]
pub fn zone_material_constants(cap: &Capture, image: &[u8], m: &crate::walk::capture::MaterialRec) -> HashMap<String, Vec<f32>> {
    material_constants(cap, image, &Common::default(), m).0
}

/// One model surface: positions, normals, UVs and triangles.
#[derive(Clone, Debug, Default)]
pub struct ModelSurface {
    pub positions: Vec<[f32; 3]>,
    pub normals: Vec<[f32; 3]>,
    pub uvs: Vec<[f32; 2]>,
    pub tris: Vec<[u16; 3]>,
    pub material: Option<usize>,
}

/// A static model's finest LOD (and its collision LOD, else its coarsest).
#[derive(Clone, Debug, Default)]
pub struct MapModel {
    pub name: String,
    pub surfaces: Vec<ModelSurface>,
    pub coarse: Vec<ModelSurface>,
}

/// A placed static model.
#[derive(Clone, Copy, Debug, Default)]
pub struct StaticModel {
    pub model: usize,
    pub origin: [f32; 3],
    /// Rows: the model's forward, left and up axes in the world.
    pub axis: [[f32; 3]; 3],
    pub scale: f32,
    /// bo2mp: past this distance it is not drawn (0: always drawn).
    pub cull_dist: f32,
}

/// A static model the collision lists (`cStaticModel_t`): its model's name
/// and its placement (axes and scale from the inverse scaled axes).
#[derive(Clone, Debug, Default)]
pub struct ClipStaticModel {
    pub name: String,
    pub contents: u32,
    pub origin: [f32; 3],
    /// Rows: the model's forward, left and up axes in the world.
    pub axis: [[f32; 3]; 3],
    pub scale: f32,
    /// Its posed bones (`DObjAnimMat`: quaternion xyzw, translation,
    /// translation weight), by bone index.
    pub bones: Vec<[f32; 8]>,
}

/// One of an `XModel`'s collision triangles (`XModelCollSurf`'s
/// `XModelCollTri`: a plane `n.p = d` and two edge vectors, a point's `s =
/// svec.p - svec.w`, `t = tvec.p - tvec.w`; the triangle is `s, t >= 0, s +
/// t <= 1`), its corners solved at (0,0), (1,0), (0,1), in the space of
/// its surface's bone.
#[derive(Clone, Copy, Debug, Default)]
pub struct CollTriangle {
    pub corners: [[f32; 3]; 3],
    pub bone: i32,
    pub contents: u32,
    pub surface_flags: u32,
}

/// A brush sub-model (`*<n>`): its world surfaces and its bounds.
#[derive(Clone, Copy, Debug, Default)]
pub struct BrushModel {
    pub first_surface: u32,
    pub surface_count: u32,
    pub mins: [f32; 3],
    pub maxs: [f32; 3],
}

/// A collision brush: its planes (normal, distance) with each plane's
/// surface flags, the six axial ones first (+x, -x, +y, -y, +z, -z).
#[derive(Clone, Debug, Default)]
pub struct ClipBrush {
    pub contents: u32,
    pub planes: Vec<[f32; 4]>,
    pub plane_surface_flags: Vec<u32>,
    /// bo2mp: the brush's corners as the zone keeps them (`verts`, 12 each,
    /// count at +88): what its planes must enclose.
    pub verts: Vec<[f32; 3]>,
}

/// A BSP node: its plane and children (negative: leaf `-1 - n`).
#[derive(Clone, Copy, Debug, Default)]
pub struct ClipNode {
    pub plane: [f32; 4],
    pub children: [i32; 2],
}

/// A BSP leaf: its brushes and its triangle trees.
#[derive(Clone, Debug, Default)]
pub struct ClipLeaf {
    pub brushes: Vec<u16>,
    pub first_coll_aabb: u32,
    pub coll_aabb_count: u32,
    pub mins: [f32; 3],
    pub maxs: [f32; 3],
}

/// A collision partition: `tri_count` triangles from `first_tri`.
#[derive(Clone, Copy, Debug, Default)]
pub struct ClipPartition {
    pub tri_count: u8,
    pub first_tri: u32,
}

/// A collision AABB tree node (`u`: first child, or the partition of a leaf).
#[derive(Clone, Copy, Debug, Default)]
pub struct ClipAabb {
    pub origin: [f32; 3],
    pub half_size: [f32; 3],
    pub material: u16,
    pub child_count: u16,
    pub u: u32,
}

/// A collision sub-model (`*<n>`; 0 is the world).
#[derive(Clone, Debug, Default)]
pub struct ClipModel {
    pub mins: [f32; 3],
    pub maxs: [f32; 3],
    pub radius: f32,
    pub leaf: ClipLeaf,
}

/// The map's collision as the zone has it.
#[derive(Clone, Debug, Default)]
pub struct MapClip {
    /// (name, surface flags, content flags).
    pub materials: Vec<(String, u32, u32)>,
    pub brushes: Vec<ClipBrush>,
    pub nodes: Vec<ClipNode>,
    pub leafs: Vec<ClipLeaf>,
    pub verts: Vec<[f32; 3]>,
    pub tris: Vec<[u32; 3]>,
    /// One bit per triangle edge (`3 * tri + k`).
    pub walkable: Vec<u8>,
    pub partitions: Vec<ClipPartition>,
    pub aabb_trees: Vec<ClipAabb>,
    pub models: Vec<ClipModel>,
    /// The static models that collide.
    pub static_models: Vec<ClipStaticModel>,
    /// The collision triangles of the models those static models use, by
    /// model name (what the game traces them with; held in the zone).
    pub model_collision: std::collections::HashMap<String, Vec<CollTriangle>>,
    /// bo2mp: how far the planes were found from where the zone's reference
    /// landed (bytes; see `read_clip`), and how many brushes have a corner
    /// outside one of their planes (by more than half a unit; 0 when the
    /// planes are read right).
    pub plane_shift: i64,
    pub brushes_off_corners: usize,
    /// bo2mp: brushes given a side from their corners (`sides_from_corners`).
    pub brushes_sides_added: usize,
}

/// The sun (the first sun volume's first state): pitch and yaw (degrees),
/// colour and intensity.
#[derive(Clone, Copy, Debug, Default)]
pub struct Sun {
    pub pitch: f32,
    pub yaw: f32,
    pub color: [f32; 3],
    pub intensity: f32,
}

/// Where a map's files are.
#[derive(Clone, Debug)]
pub struct MapSources {
    pub zone: PathBuf,
    /// The folder of the shared zones and the `.xpak` packages: BO3's
    /// `zone` folder (the map zone's own for a retail map).
    pub pak_dir: PathBuf,
    /// bo2mp: the map's own folder when it is not `pak_dir` (a Steam
    /// Workshop item, a `usermaps/<map>/zone`): its `.xpak` packages are
    /// searched first, then `pak_dir`'s.
    pub own_dir: Option<PathBuf>,
}

impl MapSources {
    pub fn beside(zone: &Path) -> Self {
        let own = zone.parent().map(Path::to_path_buf).unwrap_or_default();
        let pak_dir = shared_dir_for(zone);
        Self { zone: zone.to_owned(), own_dir: (pak_dir != own).then_some(own), pak_dir }
    }
}

/// bo2mp: BO3's `zone` folder as the game's folders set it (`core_common.ff`
/// in it), for maps kept elsewhere (`set_game_zone_dir`).
static GAME_ZONE_DIR: std::sync::Mutex<Option<PathBuf>> = std::sync::Mutex::new(None);

/// bo2mp: names BO3's own `zone` folder (where its shared zones and
/// packages are), for maps whose zone is not in it (Steam Workshop items,
/// `usermaps`).
pub fn set_game_zone_dir(dir: Option<PathBuf>) {
    if let Ok(mut d) = GAME_ZONE_DIR.lock() {
        *d = dir;
    }
}

/// bo2mp: the folder of the shared zones (`core_common`, `mp_common`, ...)
/// and BO3's packages for the map zone `zone`: its own folder when they
/// are there (a retail map); else BO3's `zone` folder as named
/// (`set_game_zone_dir`), above a usermap (`<bo3>/usermaps/<map>/zone`), or
/// in the Steam library of a Workshop item
/// (`<lib>/steamapps/workshop/content/311210/<id>` ->
/// `<lib>/steamapps/common/Call of Duty Black Ops III/zone`); else the
/// zone's own folder.
pub fn shared_dir_for(zone: &Path) -> PathBuf {
    let own = zone.parent().map(Path::to_path_buf).unwrap_or_default();
    let has_common = |d: &Path| d.join("core_common.ff").is_file();
    if has_common(&own) {
        return own;
    }
    let named = GAME_ZONE_DIR.lock().ok().and_then(|d| d.clone()).filter(|d| has_common(d));
    let above = own.ancestors().skip(1).take(4).map(|d| d.join("zone")).find(|d| has_common(d));
    let library = own.ancestors().find(|d| d.file_name().is_some_and(|n| n.eq_ignore_ascii_case("steamapps"))).map(|s| s.join("common").join("Call of Duty Black Ops III").join("zone")).filter(|d| has_common(d));
    named.or(above).or(library).unwrap_or(own)
}

/// bo2mp: a map's packages: its own folder's first (a Workshop item's
/// `<map>.xpak`), then the shared folder's.
/// bo2mp (load speed): the streamed mesh data of every model the static
/// models place (each LOD and collision mesh), read in package and file
/// order on worker threads and decoded there, by `.xpak` key. Read one at a
/// time at random places as each model was built, mp_skyjacked's took
/// 108 s off a cold disk.
fn fetch_meshes(cap: &Capture, names: &[Option<String>], paks: &XpakSet) -> HashMap<u64, Vec<u8>> {
    let mut models = std::collections::HashSet::new();
    let mut keys = std::collections::HashSet::new();
    let mut jobs = Vec::new();
    for name in names.iter().flatten() {
        let name = bare(name);
        if !models.insert(name) {
            continue;
        }
        let Some(x) = cap.model(name).or_else(|| cap.model(&format!(",{name}"))) else { continue };
        let ranked = lods_by_tris(cap, x);
        let coarse = x.collision_mesh.as_deref().or_else(|| ranked.first().filter(|_| ranked.len() > 1).map(|l| l.mesh.as_str()));
        for mesh in ranked.last().map(|l| l.mesh.as_str()).into_iter().chain(coarse) {
            let Some(me) = cap.mesh(bare(mesh)).or_else(|| cap.mesh(mesh)) else { continue };
            let (None, Some(e)) = (me.data, me.xpak.as_ref()) else { continue };
            if keys.insert(e.key) && let Some((pak, entry)) = paks.find_all(e.key).into_iter().next() {
                jobs.push((e.key, pak, entry));
            }
        }
    }
    jobs.sort_by(|a, b| (&a.1.path, a.2.offset).cmp(&(&b.1.path, b.2.offset)));
    let t = std::time::Instant::now();
    // (Up to 8 readers: with 8 reads in flight the disk orders them better
    // than with 4; mp_skyjacked's mesh reads off a hard disk took ~21% less,
    // and 16 no less than 4.)
    let threads = std::thread::available_parallelism().map_or(4, |n| n.get()).clamp(1, 8).min(jobs.len().max(1));
    let take = std::sync::atomic::AtomicUsize::new(0);
    std::thread::scope(|scope| {
        let workers: Vec<_> = (0..threads)
            .map(|_| {
                scope.spawn(|| {
                    let mut out = Vec::new();
                    loop {
                        let k = take.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                        let Some(&(key, pak, entry)) = jobs.get(k) else { break };
                        if let Ok(data) = pak.read_raw(entry).and_then(|raw| crate::xpak::decode(&raw)) {
                            out.push((key, data));
                        }
                    }
                    out
                })
            })
            .collect();
        let out: HashMap<u64, Vec<u8>> = workers.into_iter().flat_map(|w| w.join().unwrap_or_default()).collect();
        if std::env::var_os("BO2MP_T7_TIMING").is_some() {
            let mut files: Vec<&Path> = jobs.iter().map(|j| j.1.path.as_path()).collect();
            files.dedup();
            let raw: u64 = jobs.iter().map(|j| j.2.size).sum();
            eprintln!(
                "t7 map timing:   {} meshes ({:.0} MB stored, {:.0} MB decoded) from {} packages read in {:.2}s",
                jobs.len(),
                raw as f64 / 1048576.0,
                out.values().map(Vec::len).sum::<usize>() as f64 / 1048576.0,
                files.len(),
                t.elapsed().as_secs_f32()
            );
        }
        out
    })
}

/// bo2mp (load speed): a model's LODs with mesh data, fewest triangles
/// first (the zone lists them coarsest first, but not always), counted from
/// the zone's surface records so only the LODs kept need reading.
fn lods_by_tris<'a>(cap: &Capture, x: &'a crate::walk::capture::ModelRec) -> Vec<&'a crate::walk::capture::LodRec> {
    let tris = |mesh: &str| {
        cap.mesh(bare(mesh))
            .or_else(|| cap.mesh(mesh))
            .filter(|me| me.data.is_some() || me.xpak.is_some())
            .map(|me| me.surfaces.iter().map(|s| s.tri_count as usize).sum::<usize>())
    };
    let mut ranked: Vec<(usize, &crate::walk::capture::LodRec)> = x.lods.iter().filter_map(|l| tris(&l.mesh).map(|t| (t, l))).collect();
    ranked.sort_by_key(|(t, _)| *t);
    ranked.into_iter().map(|(_, l)| l).collect()
}

fn map_packages(src: &MapSources) -> Result<XpakSet, String> {
    let shared = packages(&src.pak_dir);
    let Some(own) = src.own_dir.as_deref() else { return shared };
    let mut set = packages(own)?;
    match shared {
        Ok(s) => set.paks.extend(s.paks),
        Err(e) => {
            if set.paks.is_empty() {
                return Err(e);
            }
        }
    }
    Ok(set)
}

/// bo2mp: what a map keeps of its inflated zone.
#[derive(Clone)]
enum HeldZone {
    Whole(std::sync::Arc<Vec<u8>>),
    /// The pixels of the pictures kept in the zone, at their old offsets.
    Pictures(std::sync::Arc<crate::kept::KeptBytes>),
}

impl Default for HeldZone {
    fn default() -> Self {
        Self::Whole(Default::default())
    }
}

impl HeldZone {
    /// A map zone picture and the bytes its pixel offsets are into.
    fn picture<'a>(&'a self, im: &'a crate::walk::capture::ImageRec, moved: &'a mut Option<crate::walk::capture::ImageRec>) -> (&'a crate::walk::capture::ImageRec, &'a [u8]) {
        match self {
            Self::Whole(image) => (im, image.as_slice()),
            Self::Pictures(kept) => match im.pixels.and_then(|(at, len)| kept.get(at, len).map(|b| (b, len))) {
                Some((bytes, len)) => {
                    let moved = moved.insert(im.clone());
                    moved.pixels = Some((0, len));
                    (moved, bytes)
                }
                None => (im, &[]),
            },
        }
    }
}

/// A map, read.
#[derive(Default)]
pub struct T7Map {
    pub name: String,
    pub vertices: Vec<WorldVertex>,
    pub indices: Vec<u16>,
    pub surfaces: Vec<WorldSurface>,
    pub materials: Vec<MapMaterial>,
    pub models: Vec<MapModel>,
    pub static_models: Vec<StaticModel>,
    /// The entity string as the zone has it.
    pub entities: String,
    /// The trigger models (`"model" "?N"`): each its hulls (middle, half
    /// size).
    pub trigger_models: Vec<Vec<([f32; 3], [f32; 3])>>,
    /// Per trigger model, per hull, its slabs (direction, middle, half
    /// width): the planes cutting the hull's box down to its shape (a
    /// slanted out-of-bounds volume's box covers the play area; its slabs
    /// do not).
    pub trigger_slabs: Vec<Vec<Vec<([f32; 3], f32, f32)>>>,
    pub brush_models: Vec<BrushModel>,
    /// bo2mp: per world surface, where its brush model is drawn
    /// ([`brush_places`]); `surfaces` already has the placed ones moved
    /// and the hidden ones emptied (no triangles).
    pub brush_places: Vec<BrushPlace>,
    pub clip: Option<MapClip>,
    pub sun: Option<Sun>,
    /// The sky box models (first sun volume).
    pub sky_models: Vec<String>,
    pub mins: [f32; 3],
    pub maxs: [f32; 3],
    /// What was read and what was missing, line by line.
    pub report: Vec<String>,
    /// (bo2mp: surfaces drawn but left out of the collision built from the
    /// drawn world, by index; empty: none. Set by the Modern Warfare III
    /// lane for a big map's sea sheets and far backdrop.)
    pub no_collision: Vec<bool>,
    /// (bo2mp: placed models left out of the collision built from the
    /// drawn world, by index into `static_models`; empty: none.)
    pub static_no_collision: Vec<bool>,
    /// (bo2mp: a map whose light is baked into its vertex colours: the
    /// ambient light of the uniform lightmap it is drawn with, in gamma
    /// space; the sun comes from worldspawn. Set by the Modern Warfare III
    /// lane.)
    pub baked_ambient: Option<[f32; 3]>,
    /// (bo2mp: such a map drawn in Black Ops III's look instead
    /// (`assets::lane::bo3_look`): its vertex colours are white (the sun's
    /// shadow map and the real-time ambient do the shading), the uniform
    /// page and the placed models' light are in the look's linear units.
    /// Set by the Modern Warfare III lane.)
    pub bo3_look: bool,
    /// (bo2mp: per placed model, its light as a Black Ops II light grid
    /// sample gives it (rgb = sqrt(light / 32)); empty: a neutral light.)
    pub static_light: Vec<[u8; 4]>,
    /// (bo2mp: a sky cube another game's reader made (six faces, +x -x
    /// +y -y +z -z, z up), drawn behind the world.)
    pub sky_cube: Option<ImageLevels>,
    /// bo2mp: where the first sun volume keeps its light (`crate::light`).
    pub(crate) light_refs: Option<crate::light::LightRefs>,
    images: HashMap<String, ImageRec>,
    /// bo2mp: the zone's own effects and their sprites' materials.
    effects: HashMap<String, crate::walk::capture::FxRec>,
    /// bo2mp: the zone's light descriptions (`scene_light`).
    light_descriptions: HashMap<String, Vec<u8>>,
    /// bo2mp: the impact tables (`walk::capture::Capture::impact_fields`).
    impact_fields: HashMap<(crate::AssetType, String), Vec<(usize, String)>>,
    effect_materials: HashMap<String, crate::walk::capture::MaterialRec>,
    /// (bo2mp: pictures another game's reader decoded for its materials,
    /// by name: [`Self::add_image`].)
    added_images: HashMap<String, ImageLevels>,
    /// bo2mp: the inflated map zone, until the renderer has read what it
    /// needs from it: then only its pictures' bytes ([`Self::release_zone`]).
    zone: std::sync::RwLock<HeldZone>,
    /// bo2mp: the map zone's walk, kept for the native renderer.
    capture: Option<std::sync::Arc<crate::walk::capture::Capture>>,
    /// bo2mp: the common zones' pictures (`Common`).
    common: Option<std::sync::Arc<Common>>,
    paks: Option<XpakSet>,
    /// The folder the zones and packages are in (for [`other_zones_image`]).
    pak_dir: PathBuf,
    /// bo2mp (map bake): the map zone read and the map's own folder, if
    /// any ([`MapSources`]).
    zone_path: PathBuf,
    own_dir: Option<PathBuf>,
}

fn f32_at(b: &[u8], o: usize) -> f32 {
    f32::from_le_bytes(b[o..o + 4].try_into().unwrap())
}
fn u32_at(b: &[u8], o: usize) -> u32 {
    u32::from_le_bytes(b[o..o + 4].try_into().unwrap())
}
fn u16_at(b: &[u8], o: usize) -> u16 {
    u16::from_le_bytes(b[o..o + 2].try_into().unwrap())
}
fn u64_at(b: &[u8], o: usize) -> u64 {
    u64::from_le_bytes(b[o..o + 8].try_into().unwrap())
}
fn v3(b: &[u8], o: usize) -> [f32; 3] {
    [f32_at(b, o), f32_at(b, o + 4), f32_at(b, o + 8)]
}

fn bare(name: &str) -> &str {
    name.trim_start_matches(',')
}

/// The packages a map's pictures and meshes can be in, opened once per
/// folder (every `*.xpak` there: base, patches, the map's own).
static PACKAGES: std::sync::OnceLock<std::sync::Mutex<HashMap<PathBuf, XpakSet>>> = std::sync::OnceLock::new();

fn packages(dir: &Path) -> Result<XpakSet, String> {
    let cache = PACKAGES.get_or_init(Default::default);
    if let Some(set) = cache.lock().map_err(|_| "xpak cache poisoned".to_owned())?.get(dir) {
        return Ok(set.clone());
    }
    let (set, _bad) = XpakSet::open_dir(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    cache.lock().map_err(|_| "xpak cache poisoned".to_owned())?.insert(dir.to_owned(), set.clone());
    Ok(set)
}

/// bo2mp: a map the map load has read, handed on to the native renderer so
/// it does not read the zone again (`expect_handed` when the load starts,
/// `hand_over` / `cancel_handed` when it ends, `take_handed` /
/// `wait_handed` on the renderer's side).
struct Handed {
    /// The zone a load announced and has not yet handed over or given up.
    expected: Option<PathBuf>,
    map: Option<(PathBuf, T7Map)>,
    /// The zone whose scene read stopped (`not_wanted`): its map is not
    /// kept when it is handed over.
    unwanted: Option<PathBuf>,
    /// Counts `expect_handed`: a map load's own number.
    ticket: u64,
}

static HANDED: std::sync::Mutex<Handed> = std::sync::Mutex::new(Handed { expected: None, map: None, unwanted: None, ticket: 0 });
static HANDED_CV: std::sync::Condvar = std::sync::Condvar::new();

/// bo2mp: a map load for `zone` has begun and will end in `hand_over` or
/// `cancel_handed`; `wait_handed` blocks until then. Any map kept from
/// before is dropped.
pub fn expect_handed(zone: &Path) {
    if let Ok(mut h) = HANDED.lock() {
        h.expected = Some(zone.to_owned());
        h.map = None;
        h.unwanted = None;
        h.ticket += 1;
    }
    HANDED_CV.notify_all();
}

/// bo2mp: the load of `zone` ended without a map to hand over.
pub fn cancel_handed(zone: &Path) {
    if let Ok(mut h) = HANDED.lock() {
        if h.expected.as_deref() == Some(zone) {
            h.expected = None;
        }
    }
    HANDED_CV.notify_all();
}

/// bo2mp: keeps `map` (read from `zone`) for the native renderer's scene
/// read; any map handed before is dropped.
pub fn hand_over(zone: &Path, map: T7Map) {
    let mut gone = None;
    if let Ok(mut h) = HANDED.lock() {
        if h.unwanted.as_deref() == Some(zone) {
            // (The renderer stopped waiting for it: it goes now.)
            h.unwanted = None;
            gone = Some(map);
        } else {
            h.map = Some((zone.to_owned(), map));
        }
        h.expected = None;
    }
    HANDED_CV.notify_all();
    drop(gone);
}

/// bo2mp: the number of the latest map load announced (`expect_handed`),
/// for `not_wanted`.
pub fn handed_ticket() -> u64 {
    HANDED.lock().map_or(0, |h| h.ticket)
}

/// bo2mp: the native renderer no longer wants `zone`'s map (the game went
/// to another map while its scene was read): a map kept for it goes, and
/// one the map load hands over later is not kept. Nothing changes when a
/// map load began after `ticket` (`handed_ticket`): that one is wanted.
pub fn not_wanted(zone: &Path, ticket: u64) {
    let mut gone = None;
    if let Ok(mut h) = HANDED.lock()
        && h.ticket == ticket
    {
        if h.map.as_ref().is_some_and(|(z, _)| z == zone) {
            gone = h.map.take();
        }
        if h.expected.as_deref() == Some(zone) {
            h.expected = None;
            h.unwanted = Some(zone.to_owned());
        }
    }
    HANDED_CV.notify_all();
    drop(gone);
}

/// bo2mp: whether a load of `zone` is announced and has not handed its map
/// over or given up.
pub fn handed_expected(zone: &Path) -> bool {
    HANDED.lock().is_ok_and(|h| h.expected.as_deref() == Some(zone))
}

#[cfg(test)]
mod handed_tests {
    use super::*;

    /// Another game's map lets go of the shared zones' pictures kept per
    /// folder.
    #[test]
    fn another_games_map_lets_go_of_the_shared_pictures() {
        let mut common = Common::default();
        common.pixels.get_mut().unwrap().insert("p".into(), std::sync::Arc::from(vec![0u8; 64]));
        COMMON.get_or_init(Default::default).lock().unwrap().insert(PathBuf::from("nowhere/zone"), std::sync::Arc::new(common));
        OTHER_FOUND.get_or_init(Default::default).lock().unwrap().insert((PathBuf::from("nowhere/zone"), "p".into()), None);
        release_folder_caches();
        assert!(COMMON.get().unwrap().lock().unwrap().is_empty());
        assert!(OTHER_FOUND.get().unwrap().lock().unwrap().is_empty());
    }

    /// The shared pictures' pixels let go are read again from their zone
    /// when asked for, once per zone; one whose zone does not read stays
    /// missing and is not tried again.
    #[test]
    fn let_go_shared_pictures_are_read_again_from_their_zone() {
        let mut common = Common { dir: PathBuf::from("nowhere/zone"), ..Default::default() };
        common.pixels.get_mut().unwrap().insert("p".into(), std::sync::Arc::from(vec![7u8; 64]));
        common.sources.insert("p".into(), ("core_common", 0, 64));
        assert_eq!(common.pixels_of("p").as_deref().map(<[u8]>::len), Some(64));
        assert_eq!(common.release_pixels(), 64);
        assert_eq!(common.pixel_bytes(), 0);
        assert!(common.pixels_of("p").is_none(), "no zone file to read again");
        assert!(common.restored.lock().unwrap().contains("core_common"));
        assert!(common.pixels_of("q").is_none(), "never kept");
    }

    /// A map the renderer stopped waiting for is not kept when its load
    /// hands it over; a map load begun since then is still handed over.
    #[test]
    fn a_map_nobody_waits_for_is_not_kept() {
        let zone = Path::new("zone/mp_handed_test.ff");
        expect_handed(zone);
        let ticket = handed_ticket();
        assert!(handed_expected(zone));
        not_wanted(zone, ticket);
        assert!(!handed_expected(zone), "no wait is left");
        hand_over(zone, T7Map::default());
        assert!(take_handed(zone).is_none(), "the map went");
        // (A newer load: an old read's stop leaves it alone.)
        expect_handed(zone);
        not_wanted(zone, ticket);
        assert!(handed_expected(zone));
        hand_over(zone, T7Map::default());
        assert!(take_handed(zone).is_some());
    }
}

/// bo2mp: the map handed over for `zone`, if it is the one kept.
pub fn take_handed(zone: &Path) -> Option<T7Map> {
    let mut h = HANDED.lock().ok()?;
    if h.map.as_ref().is_some_and(|(z, _)| z == zone) { h.map.take().map(|(_, m)| m) } else { None }
}

/// bo2mp: as `take_handed`, but while a load of `zone` is announced and not
/// over, waits for it (up to `limit`), so the zone is read once.
pub fn wait_handed(zone: &Path, limit: std::time::Duration) -> Option<T7Map> {
    let end = std::time::Instant::now() + limit;
    let mut h = HANDED.lock().ok()?;
    loop {
        if h.map.as_ref().is_some_and(|(z, _)| z == zone) {
            return h.map.take().map(|(_, m)| m);
        }
        if h.expected.as_deref() != Some(zone) {
            return None;
        }
        let left = end.checked_duration_since(std::time::Instant::now())?;
        h = HANDED_CV.wait_timeout(h, left).ok()?.0;
    }
}

impl T7Map {
    /// bo2mp: no map at all, only what the shared zones of `pak_dir` (BO3's
    /// `zone` folder) define: their effects (`effect`), the sprite
    /// materials and pictures those draw with, and the impact tables
    /// (`impact_effects`). For effects wanted without a map: a guest gun's.
    /// `None` when the folder has no `core_common.ff`.
    pub fn shared_only(pak_dir: &Path) -> Option<Self> {
        if !pak_dir.join("core_common.ff").is_file() {
            return None;
        }
        let common = common_layouts(pak_dir);
        Some(T7Map {
            name: "shared".to_owned(),
            common: Some(common),
            paks: packages(pak_dir).ok(),
            pak_dir: pak_dir.to_owned(),
            ..Default::default()
        })
    }

    /// Reads a map. Errors only when the zone cannot be read or has no
    /// world; what is missing otherwise is in `report`.
    pub fn load(src: &MapSources) -> Result<Self, String> {
        let t_load = std::time::Instant::now();
        let lap = |what: &str| {
            if std::env::var_os("BO2MP_T7_TIMING").is_some() {
                eprintln!("t7 map timing: {what} at {:.2}s", t_load.elapsed().as_secs_f32());
            }
        };
        // (The shared zones are read side by side with the map's own.)
        {
            let dir = src.pak_dir.clone();
            let _ = std::thread::Builder::new().name("t7_shared_zones".into()).spawn(move || crate::shared::preload(&dir, &SHARED_ZONES));
        }
        let zone = crate::Zone::open(&src.zone).map_err(|e| format!("{}: {e}", src.zone.display()))?;
        lap("map zone opened (inflated)");
        let w = crate::walk::walk(&zone);
        lap("map zone walked");
        let mut out = T7Map {
            name: src.zone.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default(),
            ..Default::default()
        };
        match &w.stop {
            crate::walk::Stop::Done { .. } => out.report.push(format!("zone: walked to its end ({} assets)", w.walked.len())),
            other => out.report.push(format!("zone: walk stopped ({other:?}); reading what was walked")),
        }
        let cap = w.capture;
        let image = &zone.image;
        // bo2mp: a world the walk did not reach says where the walk stopped
        // (a Workshop map's zone holding an asset layout the reader does
        // not know yet), not only that there is no world.
        if let Err(e) = out.read_world(&cap, image) {
            return Err(match w.stop.why() {
                Some(why) => format!("{e}: the zone's walk stopped before it, at {why}"),
                None => e,
            });
        }
        lap("world read");
        out.read_models(&cap, image, src);
        lap("static models read");
        out.read_clip(&cap, image);
        lap("clip read");
        // bo2mp: the materials' named constants and bound texture slots.
        out.pak_dir = src.pak_dir.clone();
        out.zone_path = src.zone.clone();
        out.own_dir = src.own_dir.clone();
        let common = common_layouts(&src.pak_dir);
        lap("common layouts");
        out.common = Some(common.clone());
        let mut named = 0;
        for i in 0..out.materials.len() {
            let n = out.materials[i].name.clone();
            if let Some(m) = cap.material(&n).or_else(|| cap.material(&format!(",{n}"))) {
                let (c, b) = material_constants(&cap, image, &common, m);
                named += usize::from(!c.is_empty());
                out.materials[i].constants = c;
                out.materials[i].bound = b;
            }
        }
        lap("material constants");
        out.report.push(format!("materials: {named} of {} with their constants named by their shaders", out.materials.len()));
        if let Some(m) = cap.world.map_ents.first()
            && let Some((at, n)) = m.entity_string
        {
            let raw = &image[at..at + n];
            let end = raw.iter().position(|&b| b == 0).unwrap_or(raw.len());
            out.entities = String::from_utf8_lossy(&raw[..end]).into_owned();
        }
        if let Some(m) = cap.world.map_ents.first()
            && let (Some((mat, mn)), Some((hat, hn))) = (m.trigger_models, m.trigger_hulls)
        {
            // TriggerModel (8: contents, u16 hull count, u16 first hull);
            // TriggerHull (32: middle, half size, contents, slabs).
            // TriggerSlab (20: direction, middle, half width); a hull's
            // slabs are a u16 count and a u16 first at 28.
            let image = &zone.image;
            let slabs = m.trigger_slabs;
            for i in 0..mn {
                let at = mat + 8 * i;
                let (count, first) = (u16_at(image, at + 4) as usize, u16_at(image, at + 6) as usize);
                let hulls = first..(first + count).min(hn);
                out.trigger_models.push(hulls.clone().map(|h| (v3(image, hat + 32 * h), v3(image, hat + 32 * h + 12))).collect());
                out.trigger_slabs.push(
                    hulls
                        .map(|h| {
                            let (sn, sf) = (u16_at(image, hat + 32 * h + 28) as usize, u16_at(image, hat + 32 * h + 30) as usize);
                            match slabs {
                                Some((sat, total)) => (sf..(sf + sn).min(total))
                                    .map(|k| (v3(image, sat + 20 * k), f32_at(image, sat + 20 * k + 12), f32_at(image, sat + 20 * k + 16)))
                                    .collect(),
                                None => Vec::new(),
                            }
                        })
                        .collect(),
                );
            }
        }
        out.place_brush_models();
        out.report.push(format!(
            "entities: {} bytes, {} entities, {} trigger models",
            out.entities.len(),
            out.entities.matches('{').count(),
            out.trigger_models.len()
        ));
        for im in &cap.images {
            if !im.name.starts_with(',') {
                out.images.entry(im.name.clone()).or_insert_with(|| im.clone());
            }
        }
        keep_effects(&cap, &mut out.effects, &mut out.effect_materials, &mut out.light_descriptions, &mut out.impact_fields);
        out.report.push(format!("effects: {} in the zone ({} sprite materials)", out.effects.len(), out.effect_materials.len()));
        out.name_unread_textures();
        // bo2mp: the sun volume's light (sky, probes) and the sky material's
        // constants (`skyScale`).
        out.light_refs = crate::light::LightRefs::read(&cap, &zone.image, &common);
        out.zone = std::sync::RwLock::new(HeldZone::Whole(std::sync::Arc::new(zone.image)));
        out.capture = Some(std::sync::Arc::new(cap));
        match map_packages(src) {
            Ok(p) => out.paks = Some(p),
            Err(e) => out.report.push(format!("packages: {e} (streamed pictures not drawn)")),
        }
        Ok(out)
    }

    /// bo2mp: a material's texture whose image the walk could only see as a
    /// reference (`<ref ...>`: an image an earlier asset loaded, at a slot
    /// the walk did not name; the reveal decals' colour and normal maps on
    /// mp_redwood's forest floor) is named by its siblings: Black Ops III
    /// names a material's pictures `<stem>_<letter>` by slot (colour `_c`,
    /// normal `_n`, gloss `_g`, occlusion `_o`, reveal `_r`), so a sibling's
    /// stem and the slot's letter name it, kept only when that picture is
    /// in the zone.
    fn name_unread_textures(&mut self) {
        const LETTERS: [(u32, &str); 5] = [(0xa0ab_1041, "c"), (0x59d3_0d0f, "n"), (0x6d0a_6c98, "g"), (0x0717_6bf2, "o"), (0x34d8_49d5, "r")];
        let mut named = 0;
        for i in 0..self.materials.len() {
            let stems: Vec<String> = self.materials[i]
                .textures
                .iter()
                .filter(|(_, n)| !n.starts_with('<') && !n.starts_with('$'))
                .filter_map(|(_, n)| n.rsplit_once('_').filter(|(_, l)| l.len() == 1).map(|(s, _)| s.to_owned()))
                .collect();
            for t in 0..self.materials[i].textures.len() {
                let (hash, name) = self.materials[i].textures[t].clone();
                if !name.starts_with("<ref") {
                    continue;
                }
                let Some((_, letter)) = LETTERS.iter().find(|(h, _)| *h == hash) else { continue };
                if let Some(found) = stems.iter().map(|s| format!("{s}_{letter}")).find(|n| self.image_info(n).is_some()) {
                    self.materials[i].textures[t].1 = found;
                    named += 1;
                }
            }
        }
        if named > 0 {
            self.report.push(format!("materials: {named} textures the walk saw only as references named by their siblings"));
        }
    }

    fn material_index(&mut self, cap: &Capture, index: &mut HashMap<String, Option<usize>>, name: &str) -> Option<usize> {
        let name = bare(name);
        if let Some(i) = index.get(name) {
            return *i;
        }
        let found = cap.material(name).or_else(|| cap.material(&format!(",{name}"))).map(|m| {
            self.materials.push(MapMaterial {
                name: name.to_owned(),
                technique_set: m.techset.as_deref().map(bare).unwrap_or_default().to_owned(),
                textures: m.textures.iter().map(|(h, i)| (*h, bare(i).to_owned())).collect(),
                ..Default::default()
            });
            self.materials.len() - 1
        });
        index.insert(name.to_owned(), found);
        found
    }

    /// bo2mp: the brush models' surfaces moved to their entities
    /// ([`brush_places`]) and those no drawn entity names emptied, with a
    /// `brush models:` report line.
    fn place_brush_models(&mut self) {
        let places = brush_places(&self.entities, &self.brush_models, self.surfaces.len(), self.name.starts_with("mp_"));
        let mut moved = vec![false; self.vertices.len()];
        let (mut placed, mut hidden) = (0usize, 0usize);
        let mut hidden_by: std::collections::BTreeMap<String, usize> = std::collections::BTreeMap::new();
        for (si, p) in places.iter().enumerate() {
            let s = &mut self.surfaces[si];
            match p {
                BrushPlace::World => {}
                BrushPlace::Hidden(class) => {
                    s.tri_count = 0;
                    hidden += 1;
                    *hidden_by.entry(if class.is_empty() { "(no entity)".into() } else { class.clone() }).or_default() += 1;
                }
                BrushPlace::Placed { origin, axis, .. } => {
                    placed += 1;
                    let verts = s.first_vertex as usize..(s.first_vertex as usize + s.vertex_count as usize).min(self.vertices.len());
                    let rot = |v: [f32; 3]| brush_point(v, [0.0; 3], axis);
                    for vi in verts.clone() {
                        if std::mem::replace(&mut moved[vi], true) {
                            continue;
                        }
                        let v = &mut self.vertices[vi];
                        v.xyz = brush_point(v.xyz, *origin, axis);
                        v.normal = rot(v.normal);
                        v.tangent = rot(v.tangent);
                    }
                    let (mut lo, mut hi) = ([f32::MAX; 3], [f32::MIN; 3]);
                    for v in &self.vertices[verts] {
                        for c in 0..3 {
                            lo[c] = lo[c].min(v.xyz[c]);
                            hi[c] = hi[c].max(v.xyz[c]);
                        }
                    }
                    if lo[0] <= hi[0] {
                        (s.mins, s.maxs) = (lo, hi);
                    }
                }
            }
        }
        if placed + hidden > 0 {
            self.report.push(format!("brush models: {placed} surfaces moved to their entities, {hidden} not drawn (no drawn entity names them: {hidden_by:?})"));
        }
        self.brush_places = places;
    }

    fn read_world(&mut self, cap: &Capture, image: &[u8]) -> Result<(), String> {
        use crate::walk::gfxworld::{BRUSH_MODEL, SURFACE};
        let g = cap.world.gfx_worlds.first().ok_or("no GfxWorld in the zone")?;
        let mut materials: HashMap<String, Option<usize>> = HashMap::new();
        let (Some((pos, pos_len)), Some((att, att_len))) = (g.positions, g.attributes) else {
            return Err("GfxWorld: no vertex data in the zone".into());
        };
        let n = (pos_len / 12).min(att_len / 20);
        self.vertices = (0..n)
            .map(|i| {
                let a = att + 20 * i;
                WorldVertex {
                    xyz: v3(image, pos + 12 * i),
                    color: u32_at(image, a),
                    uv: [f32_at(image, a + 4), f32_at(image, a + 8)],
                    normal: unpack_unit(u32_at(image, a + 12)),
                    tangent: unpack_unit(u32_at(image, a + 16)),
                }
            })
            .collect();
        if let Some((at, n)) = g.indices {
            self.indices = (0..n).map(|i| u16_at(image, at + 2 * i)).collect();
        }
        let mut no_material = 0;
        if let Some((at, n)) = g.surfaces {
            for i in 0..n {
                let s = at + SURFACE * i;
                let first = u32_at(image, s + 12) / 12;
                let tri_count = u16_at(image, s + 42);
                let base_index = u32_at(image, s + 44);
                let mut vertex_count = u32::from(u16_at(image, s + 40));
                if vertex_count == 0 {
                    let tris = self.indices.get(base_index as usize..(base_index as usize + 3 * tri_count as usize).min(self.indices.len()));
                    vertex_count = tris.and_then(|t| t.iter().max()).map_or(0, |&m| u32::from(m) + 1);
                }
                let material = g.surface_materials.get(i).cloned().flatten().and_then(|m| self.material_index(cap, &mut materials, &m));
                if material.is_none() {
                    no_material += 1;
                }
                self.surfaces.push(WorldSurface {
                    first_vertex: first,
                    vertex_count,
                    tri_count,
                    base_index,
                    material,
                    flags: image[s + 80],
                    mins: v3(image, s + 48),
                    maxs: v3(image, s + 60),
                });
            }
        }
        let h = g.header_at;
        self.mins = v3(image, h + 0x2c0);
        self.maxs = v3(image, h + 0x2cc);
        if let Some((at, n)) = g.brush_models {
            for i in 0..n {
                let b = at + BRUSH_MODEL * i;
                self.brush_models.push(BrushModel {
                    mins: v3(image, b + 32),
                    maxs: v3(image, b + 44),
                    surface_count: u32_at(image, b + 56),
                    first_surface: u32_at(image, b + 60),
                });
            }
        }
        if g.sun_at != 0 {
            // GfxConfig_Sun: settings[0] at 60 (pitch, yaw, colour, intensity).
            let st = g.sun_at + 60;
            self.sun = Some(Sun {
                pitch: f32_at(image, st),
                yaw: f32_at(image, st + 4),
                color: v3(image, st + 8),
                intensity: f32_at(image, st + 20),
            });
        }
        self.sky_models = g.sky_models.iter().map(|m| bare(m).to_owned()).collect();
        let mut mean = [0u64; 4];
        for v in &self.vertices {
            for (k, m) in mean.iter_mut().enumerate() {
                *m += u64::from((v.color >> (8 * k)) & 0xff);
            }
        }
        let n = self.vertices.len().max(1) as u64;
        self.report.push(format!("world vertex colour: mean bytes {:?}", mean.map(|m| m / n)));
        self.report.push(format!(
            "world: {} vertices, {} indices, {} surfaces ({} without a material), {} materials, {} brush models, bounds {:?}..{:?}",
            self.vertices.len(),
            self.indices.len(),
            self.surfaces.len(),
            no_material,
            self.materials.len(),
            self.brush_models.len(),
            self.mins,
            self.maxs
        ));
        Ok(())
    }

    /// The static models: each distinct model's first LOD (and coarsest),
    /// decoded by `mesh`; placements from the draw instances.
    fn read_models(&mut self, cap: &Capture, image: &[u8], src: &MapSources) {
        use crate::walk::gfxworld::SMODEL_DRAW;
        let Some(g) = cap.world.gfx_worlds.first() else { return };
        let Some((at, n)) = g.smodels else { return };
        let timing = std::env::var_os("BO2MP_T7_TIMING").is_some();
        let t = std::time::Instant::now();
        let paks = map_packages(src).ok();
        if timing {
            eprintln!("t7 map timing:   {} packages opened in {:.2}s", paks.as_ref().map_or(0, |p| p.paks.len()), t.elapsed().as_secs_f32());
        }
        let mut fetched = paks.as_ref().map(|p| fetch_meshes(cap, &g.smodel_models[..n.min(g.smodel_models.len())], p)).unwrap_or_default();
        let mut materials: HashMap<String, Option<usize>> = self.materials.iter().enumerate().map(|(i, m)| (m.name.clone(), Some(i))).collect();
        let mut built: HashMap<String, Option<usize>> = HashMap::new();
        let mut failures: Vec<String> = Vec::new();
        for i in 0..n {
            let d = at + SMODEL_DRAW * i;
            let Some(name) = g.smodel_models.get(i).cloned().flatten() else { continue };
            let name = bare(&name).to_owned();
            let model = match built.get(&name) {
                Some(m) => *m,
                None => {
                    let r = self.build_model(cap, image, paks.as_ref(), &mut fetched, &mut materials, &name);
                    let m = match r {
                        Ok(m) => Some(m),
                        Err(e) => {
                            if failures.len() < 8 {
                                failures.push(format!("{name}: {e}"));
                            }
                            None
                        }
                    };
                    built.insert(name.clone(), m);
                    m
                }
            };
            let Some(model) = model else { continue };
            self.static_models.push(StaticModel {
                model,
                origin: v3(image, d),
                axis: [v3(image, d + 12), v3(image, d + 24), v3(image, d + 36)],
                scale: f32_at(image, d + 48),
                cull_dist: 0.0,
            });
        }
        self.report.push(format!(
            "static models: {} placed of {}, {} models built of {}{}",
            self.static_models.len(),
            n,
            self.models.len(),
            built.len(),
            if failures.is_empty() { String::new() } else { format!("; not built: {}", failures.join("; ")) }
        ));
    }

    fn build_model(
        &mut self,
        cap: &Capture,
        image: &[u8],
        paks: Option<&XpakSet>,
        fetched: &mut HashMap<u64, Vec<u8>>,
        materials: &mut HashMap<String, Option<usize>>,
        name: &str,
    ) -> Result<usize, String> {
        let x = cap.model(name).or_else(|| cap.model(&format!(",{name}"))).ok_or("not in the zone")?;
        // A mesh's surfaces (with the LOD's materials, when it is one).
        let decode = |this: &mut Self, fetched: &mut HashMap<u64, Vec<u8>>, materials: &mut HashMap<String, Option<usize>>, mesh: &str, mats: &[String]| -> Result<Option<Vec<ModelSurface>>, String> {
            let Some(me) = cap.mesh(bare(mesh)).or_else(|| cap.mesh(mesh)) else { return Ok(None) };
            let data = match (me.data, me.xpak) {
                (Some((at, n)), _) => image.get(at..at + n).map(<[u8]>::to_vec),
                (None, Some(e)) => fetched.remove(&e.key).or_else(|| paks.and_then(|p| p.read(e.key).ok())),
                _ => None,
            };
            let Some(data) = data else { return Ok(None) };
            let geo = crate::mesh::decode(me, &data).map_err(|e| e.to_string())?;
            let mut surfaces = Vec::new();
            for (si, s) in geo.surfaces.iter().enumerate() {
                let mut remap: HashMap<u32, u16> = HashMap::new();
                let mut sur = ModelSurface::default();
                for t in s.indices.chunks_exact(3) {
                    let mut tri = [0u16; 3];
                    for (j, &v) in t.iter().enumerate() {
                        let next = remap.len() as u16;
                        tri[j] = *remap.entry(v).or_insert_with(|| {
                            let v = v as usize;
                            sur.positions.push(geo.positions.get(v).copied().unwrap_or_default());
                            sur.normals.push(geo.normals.get(v).copied().unwrap_or([0.0, 0.0, 1.0]));
                            sur.uvs.push(geo.uvs.get(v).copied().unwrap_or_default());
                            next
                        });
                    }
                    sur.tris.push(tri);
                }
                sur.material = mats.get(si).and_then(|m| this.material_index(cap, materials, m));
                surfaces.push(sur);
            }
            Ok(Some(surfaces))
        };
        // The zone lists a model's LODs coarsest first (`mp_nuketown_x`'s
        // cypress: 604 triangles first, 23095 last), so the LODs go by
        // their triangle counts: the finest drawn; collided with, the
        // collision LOD mesh BO3 itself traces (a tree's trunk, not its
        // canopy), else the coarsest. bo2mp (load speed): only those two
        // are decoded (the next finer or coarser when one has no data).
        let ranked = lods_by_tris(cap, x);
        let mut finest = None;
        for (k, lod) in ranked.iter().enumerate().rev() {
            if let Some(surfaces) = decode(self, fetched, materials, &lod.mesh, &lod.materials)? {
                finest = Some((k, surfaces));
                break;
            }
        }
        let (top, finest) = finest.ok_or("no LOD mesh with data")?;
        let coll = match x.collision_mesh.as_deref() {
            Some(c) => decode(self, fetched, materials, c, &[])?,
            None => None,
        };
        let coarse = match coll {
            Some(c) => c,
            None => {
                let mut coarse = None;
                for lod in &ranked[..top] {
                    if let Some(surfaces) = decode(self, fetched, materials, &lod.mesh, &lod.materials)? {
                        coarse = Some(surfaces);
                        break;
                    }
                }
                coarse.unwrap_or_default()
            }
        };
        self.models.push(MapModel { name: name.to_owned(), surfaces: finest, coarse });
        Ok(self.models.len() - 1)
    }

    fn read_clip(&mut self, cap: &Capture, image: &[u8]) {
        let Some(c) = cap.world.clip_maps.first() else {
            self.report.push("collision: no clip map".into());
            return;
        };
        match read_clip(c, image, cap) {
            Ok(clip) => {
                self.report.push(format!(
                    "collision: {} brushes ({} with a corner outside their planes, {} given a side from their corners; planes found {:+} bytes from the reference), {} nodes, {} leafs, {} vertices, {} triangles, {} partitions, {} AABB nodes, {} sub-models, {} materials (references {:+#x} off)",
                    clip.brushes.len(),
                    clip.brushes_off_corners,
                    clip.brushes_sides_added,
                    clip.plane_shift,
                    clip.nodes.len(),
                    clip.leafs.len(),
                    clip.verts.len(),
                    clip.tris.len(),
                    clip.partitions.len(),
                    clip.aabb_trees.len(),
                    clip.models.len(),
                    clip.materials.len(),
                    c.virt_delta
                ));
                self.clip = Some(clip);
            }
            Err(e) => self.report.push(format!("collision: {e}")),
        }
    }

    /// bo2mp (map bake): where the map was read from: its zone (empty for
    /// [`Self::shared_only`]), the shared zones' and packages' folder, and
    /// the map's own folder if it has one. [`Self::image`]'s output depends
    /// only on these files.
    pub fn sources(&self) -> (&Path, &Path, Option<&Path>) {
        (&self.zone_path, &self.pak_dir, self.own_dir.as_deref())
    }

    /// An image's pixels: from the largest streamed part whose top level is
    /// at most `max_dim` down, its whole mip chain (each larger part holds
    /// one level, the smallest the rest of the chain:
    /// [`texture::image_levels_max`]), or the zone's own pixels. (It used to
    /// hand out the largest part's single level, so every streamed picture
    /// was drawn with no mips: the native renderer's distant shimmer.)
    pub fn image(&self, name: &str, max_dim: u32) -> Result<ImageLevels, String> {
        if let Some(l) = self.added_images.get(name) {
            return Ok(l.clone());
        }
                let other;
        let common_px;
        let held = self.zone.read().unwrap_or_else(|poison| poison.into_inner()).clone();
        let mut moved = None;
        let (im, zone_image) = match self.images.get(bare(name)) {
            // (A record without pixels or packages is a reference: the
            // common zones' copy, when they have one.)
            Some(im) if im.pixels.is_some() || !im.xpak.is_empty() => held.picture(im, &mut moved),
            own => match self.common.as_ref().and_then(|c| Some((c.images.get(bare(name))?, c))) {
                Some((im, c)) => {
                    common_px = c.pixels_of(bare(name));
                    (im, common_px.as_deref().unwrap_or(&[]))
                }
                // Defined by none of the zones this map loads: another
                // zone of the folder may define it (a picture a material
                // names that the exe's own load order resolves elsewhere).
                None => match other_zones_image(&self.pak_dir, bare(name)) {
                    Some(o) => {
                        other = o;
                        (&other.0, other.1.as_slice())
                    }
                    None => held.picture(own.ok_or_else(|| format!("{name}: not in the zone"))?, &mut moved),
                },
            },
        };
        if im.xpak.is_empty() {
            return texture::zone_levels(im, zone_image).map_err(|e| format!("{name}: {e}"));
        }
        let paks = self.paks.as_ref().ok_or_else(|| format!("{name}: no packages"))?;
        // The whole chain, as the exe streams it in; a part that does not
        // read falls back to the largest part's own levels below.
        if let Ok(l) = texture::image_levels_max(im, zone_image, paks, max_dim) {
            return Ok(l);
        }
        let dim = |k: usize| u32::from(im.xpak[k].width.max(im.xpak[k].height));
        let k = (0..im.xpak.len()).rev().find(|&k| dim(k) <= max_dim).unwrap_or(0);
        let p = &im.xpak[k];
        let data = paks.read(p.key).map_err(|e| format!("{name}: {e}"))?;
        let (w, h) = (u32::from(p.width), u32::from(p.height));
        let mut levels = if k == 0 { texture::full_levels(w, h) } else { 1 };
        // A smallest part that holds fewer levels than its full chain
        // (Warhawk's `i_generic_lookup`: 128x64 RGBA, 0x8000 bytes, its top
        // level alone where the chain is 0xaab8): the levels it holds.
        while levels > 1 && texture::chain_size(im.format, w, h, levels).is_some_and(|n| n > data.len()) {
            levels -= 1;
        }
        let want = texture::chain_size(im.format, w, h, levels).ok_or_else(|| format!("{name}: format {} not handled", im.format))?;
        if data.len() < want {
            return Err(format!("{name}: part {k} decodes to {:#x} bytes, expected {want:#x}", data.len()));
        }
        Ok(ImageLevels { format: im.format, width: w, height: h, levels, data: data[..want].to_vec(), part: Some(k) })
    }

    /// (bo2mp: a picture another game's reader decoded, for a material of
    /// its own to name; [`Self::image`] hands it out as it is.)
    pub fn add_image(&mut self, name: String, levels: ImageLevels) {
        self.added_images.insert(name, levels);
    }

    /// bo2mp: the map zone as read (inflated) and its walk: the native
    /// renderer reads its programs, meshes and materials from these rather
    /// than reading the zone again.
    /// bo2mp: where the first sun volume keeps its light (probes, probe
    /// volume, sky), for the native renderer.
    pub fn light_refs(&self) -> Option<&crate::light::LightRefs> {
        self.light_refs.as_ref()
    }

    /// (None once [`Self::release_zone`] let the zone go.)
    pub fn zone_data(&self) -> Option<(std::sync::Arc<Vec<u8>>, std::sync::Arc<crate::walk::capture::Capture>)> {
        match &*self.zone.read().unwrap_or_else(|poison| poison.into_inner()) {
            HeldZone::Whole(image) => Some((image.clone(), self.capture.clone()?)),
            HeldZone::Pictures(_) => None,
        }
    }

    /// bo2mp: the native renderer has read what it needs from the zone (its
    /// stages keep their own copies of the records they read each frame):
    /// the inflated zone goes, but for the pixels of the pictures kept in it,
    /// which [`Self::image`] may still be asked for. Returns the bytes held
    /// before and after (None when it was let go already).
    /// bo2mp: lets go of the shared zones' picture copies
    /// (`Common::release_pixels`): the bytes let go.
    pub fn release_common_pixels(&self) -> usize {
        self.common.as_ref().map_or(0, |c| c.release_pixels())
    }

    pub fn release_zone(&self) -> Option<(usize, usize)> {
        let mut held = self.zone.write().unwrap_or_else(|poison| poison.into_inner());
        let HeldZone::Whole(image) = &*held else { return None };
        let before = image.len();
        let kept = crate::kept::KeptBytes::new(image, self.images.values().filter_map(|im| im.pixels));
        let after = kept.len();
        *held = HeldZone::Pictures(std::sync::Arc::new(kept));
        Some((before, after))
    }

    /// bo2mp: the `.xpak` packages beside the zone, when they opened.
    pub fn paks(&self) -> Option<&XpakSet> {
        self.paks.as_ref()
    }

    /// The image record of a name (format, size).
    /// bo2mp: the effect called `name`: the zone's own, else the common
    /// zones'.
    /// bo2mp: a light description's 0x240 bytes (the zone's, else the
    /// common zones').
    pub fn light_description(&self, name: &str) -> Option<&Vec<u8>> {
        let n = bare(name);
        self.light_descriptions.get(n).or_else(|| self.common.as_ref()?.light_descriptions.get(n))
    }

    /// bo2mp: an impact table's effect names by field offset (the zone's,
    /// else the common zones').
    pub fn impact_fields(&self, ty: crate::AssetType, name: &str) -> Option<&Vec<(usize, String)>> {
        let k = (ty, name.to_owned());
        self.impact_fields.get(&k).or_else(|| self.common.as_ref()?.impact_fields.get(&k))
    }

    /// bo2mp: Black Ops III's impact effects for one `ImpactFx` table
    /// (`bullet_small_enter`, ...): per surface type 0..=39 the
    /// `SurfaceFxTable`'s effect (field 0x8 + 8 x type) and the
    /// `EntityFxImpacts`' first four (body / head, non-fatal / fatal, from
    /// 0x38), as the exe's impact code indexes them.
    pub fn impact_effects(&self, impact: &str) -> Option<(Vec<Option<String>>, Vec<Option<String>>)> {
        use crate::AssetType as T;
        let f = self.impact_fields(T::ImpactFx, impact)?;
        let named = |off: usize| f.iter().find(|(o, _)| *o == off).map(|(_, n)| n.clone());
        let table = |ty: T, name: Option<String>, first: usize, n: usize| -> Vec<Option<String>> {
            let fields = name.and_then(|n| self.impact_fields(ty, &n));
            (0..n).map(|i| fields.and_then(|f| f.iter().find(|(o, _)| *o == first + 8 * i).map(|(_, n)| n.clone()))).collect()
        };
        Some((table(T::SurfaceFxTable, named(0x8), 0x8, 40), table(T::EntityFxImpacts, named(0x10), 0x38, 4)))
    }

    /// bo2mp: the name of every effect the zone or the shared zones define
    /// (sorted).
    pub fn effect_names(&self) -> Vec<String> {
        let mut names: std::collections::BTreeSet<&String> = self.effects.keys().collect();
        if let Some(c) = &self.common {
            names.extend(c.effects.keys());
        }
        names.into_iter().cloned().collect()
    }

    pub fn effect(&self, name: &str) -> Option<&crate::walk::capture::FxRec> {
        let n = bare(name);
        self.effects.get(n).or_else(|| self.common.as_ref()?.effects.get(n))
    }

    /// bo2mp: an effect sprite's material (technique set, textures): the
    /// zone's own, else the common zones'.
    pub fn effect_material(&self, name: &str) -> Option<&crate::walk::capture::MaterialRec> {
        let n = bare(name);
        self.effect_materials.get(n).or_else(|| self.common.as_ref()?.effect_materials.get(n))
    }

    pub fn image_info(&self, name: &str) -> Option<&ImageRec> {
        self.images.get(bare(name)).or_else(|| self.common.as_ref()?.images.get(bare(name)))
    }
}

/// The clip map's arrays as plain records.
fn read_clip(c: &ClipMapRec, image: &[u8], cap: &Capture) -> Result<MapClip, String> {
    use crate::walk::world::{CLIP_BRUSH, CLIP_CMODEL, CLIP_LEAF, LEAF_BRUSH_NODE};
    let mut out = MapClip {
        materials: c.materials.iter().map(|(n, s, f)| (n.clone(), *s as u32, *f as u32)).collect(),
        ..Default::default()
    };
    let (pat0, pn) = c.planes.ok_or("no planes")?;
    // bo2mp: where the zone's plane reference lands (`pat0`, the GfxWorld's
    // DPVS planes on retail) sits a few bytes off the planes when the walk's
    // VIRTUAL count drifted before them (mp_apartments: 4, every non-axial
    // brush side and BSP plane read as (ny, nz, dist, type)): the start
    // whose planes are planes (unit normals), the reference's own first.
    // (`CODLEGACY_T7_NO_PLANE_FIX=1`: the reference's start as it lands.)
    let shift = if codlegacy_env::var_os("CODLEGACY_T7_NO_PLANE_FIX").is_some() {
        0
    } else {
        plane_start_shift(image, pat0, pn)
    };
    let pat = (pat0 as i64 + shift) as usize;
    out.plane_shift = shift;
    let plane = |i: usize| -> [f32; 4] {
        let p = pat + 20 * i.min(pn.saturating_sub(1));
        [f32_at(image, p), f32_at(image, p + 4), f32_at(image, p + 8), f32_at(image, p + 12)]
    };
    // Brushes: the six axial planes from the bounds, then the sides.
    // bo2mp: then any face of the brush's own corners its sides leave out
    // (`sides_from_corners`; `BO2MP_T7_OLD_BRUSH_SIDES=1`: not added).
    let old_sides = std::env::var("BO2MP_T7_OLD_BRUSH_SIDES").is_ok_and(|v| v.trim() == "1");
    if let Some((at, n)) = c.brushes {
        let mut unresolved = 0;
        for i in 0..n {
            let b = at + CLIP_BRUSH * i;
            let (mins, maxs) = (v3(image, b), v3(image, b + 16));
            let contents = u32_at(image, b + 12);
            let mut planes =
                vec![[1.0, 0.0, 0.0, maxs[0]], [-1.0, 0.0, 0.0, -mins[0]], [0.0, 1.0, 0.0, maxs[1]], [0.0, -1.0, 0.0, -mins[1]], [0.0, 0.0, 1.0, maxs[2]], [
                    0.0, 0.0, -1.0, -mins[2],
                ]];
            // axial_sflags[2][3] at 64: [min x, y, z], [max x, y, z].
            let sf: [u32; 6] = std::array::from_fn(|k| u32_at(image, b + 64 + 4 * k));
            let mut flags = vec![sf[3], sf[0], sf[4], sf[1], sf[5], sf[2]];
            let sides = u32_at(image, b + 28) as usize;
            match c.brush_side_lists.get(i).copied().flatten() {
                Some(list) => {
                    for k in 0..sides {
                        let side = list + 16 * k;
                        match c.plane_index(u64_at(image, side)) {
                            Some(p) => {
                                planes.push(plane(p));
                                flags.push(u32_at(image, side + 12));
                            }
                            None => unresolved += 1,
                        }
                    }
                }
                None if sides > 0 => unresolved += sides,
                None => {}
            }
            let nv = u32_at(image, b + 88) as usize;
            let verts = match c.brush_vert_lists.get(i).copied().flatten() {
                Some(list) if list + 12 * nv <= image.len() => (0..nv).map(|k| v3(image, list + 12 * k)).collect(),
                _ => Vec::new(),
            };
            if !old_sides {
                let added = sides_from_corners(&planes, &verts);
                if !added.is_empty() {
                    out.brushes_sides_added += 1;
                    for p in added {
                        // (The surface flags of the axial side it leans most toward.)
                        let k = (0..3).max_by(|&a, &b| p[a].abs().total_cmp(&p[b].abs())).unwrap_or(0);
                        flags.push(flags[2 * k + usize::from(p[k] < 0.0)]);
                        planes.push(p);
                    }
                }
            }
            out.brushes.push(ClipBrush { contents, planes, plane_surface_flags: flags, verts });
        }
        if unresolved > 0 {
            return Err(format!("{unresolved} brush sides whose plane could not be resolved"));
        }
        out.brushes_off_corners = out
            .brushes
            .iter()
            .filter(|b| b.planes.iter().any(|p| b.verts.iter().any(|v| v[0] * p[0] + v[1] * p[1] + v[2] * p[2] - p[3] > 0.5)))
            .count();
    }
    // Leaf brush lists, flattened per leaf (the node tree under a leaf).
    let nodes_at = c.leaf_brush_nodes;
    let leaf_brushes = |root: usize| -> Vec<u16> {
        let mut out = Vec::new();
        let Some((at, n)) = nodes_at else { return out };
        let mut pending = vec![(root, 0usize)];
        while let Some((i, depth)) = pending.pop() {
            if i >= n || depth > 64 {
                continue;
            }
            let p = at + LEAF_BRUSH_NODE * i;
            let count = u16_at(image, p + 2) as i16;
            if count > 0 {
                if let Some((list, k)) = c.leaf_brush_lists.get(i).copied().flatten() {
                    out.extend((0..k).map(|j| u16_at(image, list + 2 * j)));
                }
            } else {
                // (As Black Ops and Black Ops II: a negative count also visits
                // the next node; the offsets are relative to this one.)
                if count < 0 {
                    pending.push((i + 1, depth + 1));
                }
                let (a, b) = (u16_at(image, p + 16) as usize, u16_at(image, p + 18) as usize);
                if a > 0 {
                    pending.push((i + a, depth + 1));
                }
                if b > 0 {
                    pending.push((i + b, depth + 1));
                }
            }
        }
        out
    };
    let leaf_at = |p: usize, node_at: usize| -> ClipLeaf {
        let node = u64_at(image, p + node_at) as i64;
        ClipLeaf {
            brushes: if node > 0 { leaf_brushes(node as usize) } else { Vec::new() },
            first_coll_aabb: u32_at(image, p),
            coll_aabb_count: u32_at(image, p + 4),
            mins: v3(image, p + 16),
            maxs: v3(image, p + 28),
        }
    };
    if let Some((at, n)) = c.nodes {
        for i in 0..n {
            let p = at + 16 * i;
            let pl = c.plane_index(u64_at(image, p)).map(plane).ok_or("a BSP node's plane could not be resolved")?;
            out.nodes.push(ClipNode { plane: pl, children: [u32_at(image, p + 8) as i32, u32_at(image, p + 12) as i32] });
        }
    }
    if let Some((at, n)) = c.leafs {
        for i in 0..n {
            out.leafs.push(leaf_at(at + CLIP_LEAF * i, 40));
        }
    }
    if let Some((at, n)) = c.verts {
        out.verts = (0..n).map(|i| v3(image, at + 12 * i)).collect();
    }
    if let Some((at, n)) = c.tri_indices {
        out.tris = (0..n).map(|i| std::array::from_fn(|k| u32_at(image, at + 12 * i + 4 * k))).collect();
    }
    if let Some((at, n)) = c.walkable {
        out.walkable = image[at..at + n].to_vec();
    }
    if let Some((at, n)) = c.partitions {
        out.partitions = (0..n).map(|i| ClipPartition { tri_count: image[at + 16 * i], first_tri: u32_at(image, at + 16 * i + 4) }).collect();
    }
    if let Some((at, n)) = c.aabb_trees {
        out.aabb_trees = (0..n)
            .map(|i| {
                let p = at + 32 * i;
                ClipAabb {
                    origin: v3(image, p),
                    material: u16_at(image, p + 12),
                    child_count: u16_at(image, p + 14),
                    half_size: v3(image, p + 16),
                    u: u32_at(image, p + 28),
                }
            })
            .collect();
    }
    // bo2mp: the inverse scaled axes are stored by column: the model's
    // forward, left and up axes are the matrix's columns (every turned
    // static model on Nuk3town and Sector has its drawn placement's axes as
    // their transpose). Read as rows, each turned model collided turned the
    // other way: invisible walls beside props, open space through them.
    // (`BO2MP_T7_OLD_CLIP_AXES=1`: rows, as before.)
    let by_rows = std::env::var("BO2MP_T7_OLD_CLIP_AXES").is_ok_and(|v| v.trim() == "1");
    for m in &c.static_models {
        let e = m.at;
        let (axis, scale) = clip_model_axes([v3(image, e + 32), v3(image, e + 44), v3(image, e + 56)], by_rows);
        out.static_models.push(ClipStaticModel {
            name: m.model.as_deref().map(bare).unwrap_or_default().to_owned(),
            contents: u32_at(image, e + 16),
            origin: v3(image, e + 20),
            axis,
            scale,
            bones: m
                .bones_at
                .map(|b| (0..m.bone_count).map(|i| std::array::from_fn(|k| f32_at(image, b + 32 * i + 4 * k))).collect())
                .unwrap_or_default(),
        });
    }
    for sm in &out.static_models {
        if out.model_collision.contains_key(&sm.name) {
            continue;
        }
        let tris = cap
            .model(&sm.name)
            .or_else(|| cap.model(&format!(",{}", sm.name)))
            .map(|m| model_collision_tris(m, image))
            .unwrap_or_default();
        out.model_collision.insert(sm.name.clone(), tris);
    }
    if let Some((at, n)) = c.cmodels {
        for i in 0..n {
            let p = at + CLIP_CMODEL * i;
            out.models.push(ClipModel { mins: v3(image, p), maxs: v3(image, p + 12), radius: f32_at(image, p + 24), leaf: leaf_at(p + 40, 40) });
        }
    }
    Ok(out)
}

/// bo2mp: the corners of the solid a brush's planes bound (each three
/// planes' meeting point inside all of them; in f64: Black Ops III's
/// brushes carry near-twin sides that f32 cannot part).
#[must_use]
pub fn plane_solid_corners(planes: &[[f32; 4]]) -> Vec<[f64; 3]> {
    let p: Vec<[f64; 4]> = planes.iter().map(|q| q.map(f64::from)).collect();
    let det = |m: [[f64; 3]; 3]| {
        m[0][0] * (m[1][1] * m[2][2] - m[1][2] * m[2][1]) - m[0][1] * (m[1][0] * m[2][2] - m[1][2] * m[2][0])
            + m[0][2] * (m[1][0] * m[2][1] - m[1][1] * m[2][0])
    };
    let mut out = Vec::new();
    for x in 0..p.len() {
        for y in x + 1..p.len() {
            for z in y + 1..p.len() {
                let m = [[p[x][0], p[x][1], p[x][2]], [p[y][0], p[y][1], p[y][2]], [p[z][0], p[z][1], p[z][2]]];
                let r = [p[x][3], p[y][3], p[z][3]];
                let d = det(m);
                if d.abs() < 1e-6 {
                    continue;
                }
                let c: [f64; 3] = std::array::from_fn(|col| {
                    let mut a = m;
                    for row in 0..3 {
                        a[row][col] = r[row];
                    }
                    det(a) / d
                });
                if p.iter().all(|q| q[0] * c[0] + q[1] * c[1] + q[2] * c[2] - q[3] <= 0.1) {
                    out.push(c);
                }
            }
        }
    }
    out
}

/// bo2mp: the faces of a brush's corners (the zone's `cbrush_t` vertices)
/// that its planes leave out, where leaving them out makes the brush
/// bigger than its corners. Black Ops III's zones store many brushes with
/// fewer sides than they have (Nuk3town: 931 of its 1826 player-solid
/// brushes; a turned box keeps three of its four upright sides, some keep
/// none): traced by their planes alone, the missing side's corner of the
/// bounding box stands solid, up to 250 units out from anything built
/// (`t7_clip_census`). The corners are what Radiant built, every one
/// inside the planes (`brushes_off_corners`), so a face of their hull never
/// cuts the built brush.
#[must_use]
pub fn sides_from_corners(planes: &[[f32; 4]], corners: &[[f32; 3]]) -> Vec<[f32; 4]> {
    if planes.len() > 64 {
        return Vec::new();
    }
    let solid = plane_solid_corners(planes);
    let dot = |a: [f64; 3], b: [f64; 3]| a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
    let mut added: Vec<[f64; 4]> = Vec::new();
    for f in corner_hull_faces(corners) {
        let n = [f[0], f[1], f[2]];
        let known = planes.iter().any(|p| dot(n, [p[0], p[1], p[2]].map(f64::from)) > 0.9999 && (f64::from(p[3]) - f[3]).abs() < 0.1);
        // A face that cuts nothing of the planes' solid is left out.
        if !known && solid.iter().any(|c| dot(n, *c) - f[3] > 0.5) {
            added.push(f);
        }
    }
    added.into_iter().map(|p| p.map(|x| x as f32)).collect()
}

/// bo2mp: the faces of the hull of a brush's corners (normal, distance;
/// out to the farthest corner, twins merged). Radiant's corners are
/// rounded, so a face's corners are taken coplanar to a quarter unit, and
/// a face is spanned by edges of 2 units at least.
#[must_use]
pub fn corner_hull_faces(corners: &[[f32; 3]]) -> Vec<[f64; 4]> {
    let n = corners.len();
    if !(4..=64).contains(&n) {
        return Vec::new();
    }
    let v: Vec<[f64; 3]> = corners.iter().map(|c| c.map(f64::from)).collect();
    let sub = |a: [f64; 3], b: [f64; 3]| [a[0] - b[0], a[1] - b[1], a[2] - b[2]];
    let dot = |a: [f64; 3], b: [f64; 3]| a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
    let mut faces: Vec<[f64; 4]> = Vec::new();
    for i in 0..n {
        for j in i + 1..n {
            for k in j + 1..n {
                let (e1, e2) = (sub(v[j], v[i]), sub(v[k], v[i]));
                if dot(e1, e1) < 4.0 || dot(e2, e2) < 4.0 {
                    continue;
                }
                let c = [e1[1] * e2[2] - e1[2] * e2[1], e1[2] * e2[0] - e1[0] * e2[2], e1[0] * e2[1] - e1[1] * e2[0]];
                let l = dot(c, c).sqrt();
                if l < 1e-3 {
                    continue;
                }
                let mut nrm = c.map(|x| x / l);
                let mut d = dot(nrm, v[i]);
                let (lo, hi) = v.iter().map(|p| dot(nrm, *p) - d).fold((0.0f64, 0.0f64), |(lo, hi), s| (lo.min(s), hi.max(s)));
                if hi > 0.25 && lo < -0.25 {
                    continue; // corners on both sides: not a face
                }
                if hi > 0.25 {
                    nrm = nrm.map(|x| -x);
                    d = -d;
                }
                // Out to the farthest corner: the face never cuts one.
                d += v.iter().map(|p| dot(nrm, *p) - d).fold(0.0f64, f64::max);
                if !faces.iter().any(|p| dot(nrm, [p[0], p[1], p[2]]) > 0.999 && (p[3] - d).abs() < 0.5) {
                    faces.push([nrm[0], nrm[1], nrm[2], d]);
                }
            }
        }
    }
    faces
}

/// bo2mp: how far the solid a brush's planes bound stands outside the hull
/// of its corners (0 when the planes are the brush as built).
#[must_use]
pub fn brush_excess(planes: &[[f32; 4]], corners: &[[f32; 3]]) -> f32 {
    let faces = corner_hull_faces(corners);
    if faces.is_empty() {
        return 0.0;
    }
    plane_solid_corners(planes)
        .iter()
        .map(|c| faces.iter().map(|f| f[0] * c[0] + f[1] * c[1] + f[2] * c[2] - f[3]).fold(f64::MIN, f64::max))
        .fold(0.0f64, f64::max) as f32
}

/// bo2mp: a clip map static model's axes (rows: forward, left, up) and
/// scale from its inverse scaled axes as stored: a point goes into the
/// model's space as a row vector times that matrix (`local[j] = sum_i
/// d[i] * inv[i][j]`), so each axis is a column, over the scale squared.
/// `by_rows`: the rows, as read before (`BO2MP_T7_OLD_CLIP_AXES=1`).
#[must_use]
pub fn clip_model_axes(stored: [[f32; 3]; 3], by_rows: bool) -> ([[f32; 3]; 3], f32) {
    let inv: [[f32; 3]; 3] = if by_rows { stored } else { std::array::from_fn(|r| std::array::from_fn(|k| stored[k][r])) };
    let len = (inv[0][0] * inv[0][0] + inv[0][1] * inv[0][1] + inv[0][2] * inv[0][2]).sqrt();
    let scale = if len > 1e-6 { 1.0 / len } else { 1.0 };
    (inv.map(|r| r.map(|x| x * scale)), scale)
}

/// bo2mp: how many bytes from `at` the `n` planes (20 each: normal,
/// distance, type, sign bits) really start: of the starts 16 bytes either
/// side, the one whose first planes have unit normals (the nearest of
/// equals, `at` itself first).
#[must_use]
pub fn plane_start_shift(image: &[u8], at: usize, n: usize) -> i64 {
    let unit = |at: usize| {
        (0..n.min(512))
            .filter(|&i| {
                let p = at + 20 * i;
                p + 20 <= image.len() && {
                    let v = v3(image, p);
                    ((v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt() - 1.0).abs() < 1e-3
                }
            })
            .count()
    };
    [0i64, -4, 4, -8, 8, -12, 12, -16, 16]
        .into_iter()
        .filter(|d| at as i64 + d >= 0)
        .max_by_key(|&d| (unit((at as i64 + d) as usize), -d.abs()))
        .unwrap_or(0)
}

/// Solves `m p = r` (rows of `m`); `None` when singular.
#[must_use]
pub fn solve3(m: [[f32; 3]; 3], r: [f32; 3]) -> Option<[f32; 3]> {
    let det = |a: [[f32; 3]; 3]| {
        a[0][0] * (a[1][1] * a[2][2] - a[1][2] * a[2][1]) - a[0][1] * (a[1][0] * a[2][2] - a[1][2] * a[2][0])
            + a[0][2] * (a[1][0] * a[2][1] - a[1][1] * a[2][0])
    };
    let d = det(m);
    if d.abs() < 1e-12 {
        return None;
    }
    Some(std::array::from_fn(|c| {
        let mut a = m;
        for row in 0..3 {
            a[row][c] = r[row];
        }
        det(a) / d
    }))
}

/// An `XModel`'s collision triangles as the zone holds them.
fn model_collision_tris(m: &crate::walk::capture::ModelRec, image: &[u8]) -> Vec<CollTriangle> {
    let mut out = Vec::new();
    for cs in &m.coll_surfs {
        let Some(at) = cs.tris_at else { continue };
        for t in 0..cs.tri_count {
            let o = at + 48 * t;
            if o + 48 > image.len() {
                break;
            }
            let v: [f32; 12] = std::array::from_fn(|k| f32_at(image, o + 4 * k));
            let rows = [[v[0], v[1], v[2]], [v[4], v[5], v[6]], [v[8], v[9], v[10]]];
            let corner = |sv: f32, tv: f32| solve3(rows, [v[3], v[7] + sv, v[11] + tv]);
            if let (Some(a), Some(b), Some(c)) = (corner(0.0, 0.0), corner(1.0, 0.0), corner(0.0, 1.0)) {
                out.push(CollTriangle { corners: [a, b, c], bone: cs.bone, contents: cs.contents, surface_flags: cs.surface_flags });
            }
        }
    }
    out
}

/// A point in a posed bone's space moved into its model's space
/// (`DObjAnimMat`: the quaternion's rotation, then the translation).
#[must_use]
pub fn bone_to_model(mat: &[f32; 8], p: [f32; 3]) -> [f32; 3] {
    let (x, y, z, w) = (mat[0], mat[1], mat[2], mat[3]);
    let n = x * x + y * y + z * z + w * w;
    if n < 1e-12 {
        return [p[0] + mat[4], p[1] + mat[5], p[2] + mat[6]];
    }
    let s = 2.0 / n;
    let (xx, yy, zz) = (x * x * s, y * y * s, z * z * s);
    let (xy, xz, yz) = (x * y * s, x * z * s, y * z * s);
    let (wx, wy, wz) = (w * x * s, w * y * s, w * z * s);
    let r = [[1.0 - yy - zz, xy - wz, xz + wy], [xy + wz, 1.0 - xx - zz, yz - wx], [xz - wy, yz + wx, 1.0 - xx - yy]];
    std::array::from_fn(|i| r[i][0] * p[0] + r[i][1] * p[1] + r[i][2] * p[2] + mat[4 + i])
}

#[cfg(test)]
mod collision_tests {
    use super::{bone_to_model, clip_model_axes, plane_start_shift, sides_from_corners, solve3};

    /// Nuk3town's brush 1248 (a box turned 18 degrees, 8 units thick): the
    /// zone keeps three of its four upright sides; the fourth is found from
    /// its corners, and a whole brush gets nothing.
    #[test]
    fn a_missing_side_comes_from_the_corners() {
        let (a, b) = (0.949_972_2f32, 0.312_334_36f32);
        let mut planes = vec![
            [1.0, 0.0, 0.0, 2095.3599],
            [-1.0, 0.0, 0.0, -1596.1566],
            [0.0, 1.0, 0.0, 960.1527],
            [0.0, -1.0, 0.0, -80.91718],
            [0.0, 0.0, 1.0, -61.0],
            [0.0, 0.0, -1.0, 69.0],
            [a, b, 0.0, 2040.0],
            [-b, a, 0.0, 340.0],
            [b, -a, 0.0, 504.0],
        ];
        let corners: Vec<[f32; 3]> = [[1596.1566, 882.6938], [1859.7668, 80.91718], [2095.3599, 158.3761], [1831.75, 960.1527]]
            .iter()
            .flat_map(|&[x, y]| [[x, y, -69.0], [x, y, -61.0]])
            .collect();
        let added = sides_from_corners(&planes, &corners);
        assert_eq!(added.len(), 1, "{added:?}");
        let p = added[0];
        assert!((p[0] + a).abs() < 1e-3 && (p[1] + b).abs() < 1e-3 && p[2].abs() < 1e-3, "{p:?}");
        assert!((p[3] + 1792.0).abs() < 1.0, "{p:?}");
        // The bounding box's (-x, -y) corner was solid; it is not now.
        let c = [1600.0f32, 85.0, -65.0];
        let inside = |planes: &[[f32; 4]]| planes.iter().all(|q| q[0] * c[0] + q[1] * c[1] + q[2] * c[2] - q[3] <= 0.0);
        assert!(inside(&planes));
        planes.push(p);
        assert!(!inside(&planes));
        assert!(sides_from_corners(&planes, &corners).is_empty());
    }

    /// A model turned 30 degrees and scaled 2: the stored matrix is the one
    /// a row vector is multiplied by to reach the model's space; its axes
    /// come back as the placement's (forward, left, up) and its scale.
    #[test]
    fn clip_model_axes_are_the_columns() {
        let (c, s) = (30f32.to_radians().cos(), 30f32.to_radians().sin());
        let axis = [[c, s, 0.0], [-s, c, 0.0], [0.0, 0.0, 1.0]];
        let scale = 2.0;
        // local[j] = d . axis[j] / scale  =>  inv[i][j] = axis[j][i] / scale.
        let stored: [[f32; 3]; 3] = std::array::from_fn(|i| std::array::from_fn(|j| axis[j][i] / scale));
        let (got, sc) = clip_model_axes(stored, false);
        assert!((sc - scale).abs() < 1e-5);
        for r in 0..3 {
            for k in 0..3 {
                assert!((got[r][k] - axis[r][k]).abs() < 1e-5, "{got:?}");
            }
        }
        // The old reading turned it the other way.
        let (old, _) = clip_model_axes(stored, true);
        assert!((old[0][1] + s).abs() < 1e-5, "{old:?}");
    }

    /// Planes 4 bytes past where their reference lands (mp_apartments) are
    /// found there; planes where it lands stay put.
    #[test]
    fn planes_found_past_a_drifted_reference() {
        let plane = |n: [f32; 3], d: f32| -> Vec<u8> {
            let mut b: Vec<u8> = n.iter().chain([d].iter()).flat_map(|x| x.to_le_bytes()).collect();
            b.extend([3, 0, 0, 0]);
            b
        };
        let mut planes = Vec::new();
        for i in 0..40 {
            let a = i as f32 * 0.3;
            planes.extend(plane([a.cos() * 0.8, a.sin() * 0.8, 0.6], 100.0 + i as f32));
        }
        let mut image = vec![0u8; 8];
        image.extend(&planes);
        image.extend([0u8; 8]);
        assert_eq!(plane_start_shift(&image, 8, 40), 0);
        assert_eq!(plane_start_shift(&image, 4, 40), 4);
        assert_eq!(plane_start_shift(&image, 12, 39), -4);
    }

    /// `mp_nuketown_x`'s `p7_ntx_bedroom_artwork_01a`, collision triangle
    /// 0: the bottom face (z = -38.115) of a 32.2 x 2.23 box, its corners
    /// inside the surface's bounds (-16.1, -1.1, -38.1)..(16.1, 1.1, 38.1).
    #[test]
    fn a_collision_triangle_solves_to_its_corners() {
        let v = [-0.0f32, -0.0, -1.0, 38.115475, -0.031026091, 0.0, 0.0, -0.5, 0.0, -0.44824156, 0.0, -0.49999997];
        let rows = [[v[0], v[1], v[2]], [v[4], v[5], v[6]], [v[8], v[9], v[10]]];
        for (sv, tv) in [(0.0, 0.0), (1.0, 0.0), (0.0, 1.0)] {
            let p = solve3(rows, [v[3], v[7] + sv, v[11] + tv]).unwrap();
            assert!((p[2] + 38.115475).abs() < 1e-3, "{p:?}");
            assert!(p[0].abs() <= 16.2 && p[1].abs() <= 1.2, "{p:?}");
        }
    }

    #[test]
    fn a_posed_bone_rotates_then_moves() {
        // 90 degrees about z, then +10 x.
        let h = std::f32::consts::FRAC_1_SQRT_2;
        let p = bone_to_model(&[0.0, 0.0, h, h, 10.0, 0.0, 0.0, 1.0], [1.0, 0.0, 0.0]);
        assert!((p[0] - 10.0).abs() < 1e-5 && (p[1] - 1.0).abs() < 1e-5 && p[2].abs() < 1e-5, "{p:?}");
    }
}

#[cfg(test)]
mod workshop_dir_tests {
    use super::*;

    #[test]
    fn workshop_and_usermap_zones_find_bo3s_zone_folder() {
        let t = std::env::temp_dir().join(format!("bo2mp-t7-shared-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&t);
        let lib = t.join("Steam").join("steamapps");
        let bo3 = lib.join("common").join("Call of Duty Black Ops III");
        let zone = bo3.join("zone");
        let item = lib.join("workshop").join("content").join("311210").join("1705100612");
        let usermap = bo3.join("usermaps").join("mp_mine").join("zone");
        for d in [&zone, &item, &usermap] {
            std::fs::create_dir_all(d).unwrap();
        }
        std::fs::write(zone.join("core_common.ff"), b"").unwrap();
        // A retail map: its own folder, nothing else.
        let retail = MapSources::beside(&zone.join("mp_nuketown_x.ff"));
        assert_eq!((retail.pak_dir.as_path(), retail.own_dir.as_deref()), (zone.as_path(), None));
        // A Workshop item in the same library: BO3's zone folder, its own packages first.
        let ws = MapSources::beside(&item.join("mp_warhawk.ff"));
        assert_eq!((ws.pak_dir.as_path(), ws.own_dir.as_deref()), (zone.as_path(), Some(item.as_path())));
        // A usermap: the zone folder above it.
        assert_eq!(shared_dir_for(&usermap.join("mp_mine.ff")), zone);
        // Nowhere to be found: its own folder.
        let lone = t.join("elsewhere");
        assert_eq!(shared_dir_for(&lone.join("mp_x.ff")), lone);
        let _ = std::fs::remove_dir_all(&t);
    }
}

/// bo2mp: an entity's axes (forward, left, up) from its pitch, yaw and roll
/// in degrees, as the engine's `AnglesToAxis`.
pub fn axis_of(angles: [f32; 3]) -> [[f32; 3]; 3] {
    let [p, y, r] = angles.map(f32::to_radians);
    let (sp, cp, sy, cy, sr, cr) = (p.sin(), p.cos(), y.sin(), y.cos(), r.sin(), r.cos());
    let forward = [cp * cy, cp * sy, -sp];
    let right = [-sr * sp * cy + cr * sy, -sr * sp * sy - cr * cy, -sr * cp];
    let up = [cr * sp * cy + sr * sy, cr * sp * sy - sr * cy, cr * cp];
    [forward, right.map(|v| -v), up]
}

/// bo2mp: what becomes of a brush sub-model's (`*<n>`) world surfaces.
#[derive(Clone, Debug, PartialEq)]
pub enum BrushPlace {
    /// Part of the world (`*0`): drawn where it is.
    World,
    /// An entity the game spawns (`script_brushmodel`, ...), the glass
    /// system's (`glass`) or the water simulation's (`wave_water`,
    /// `wave_clamp`) names it: drawn moved to its origin and axes (a brush
    /// model's vertices are its own, about its entity's origin; Metro's
    /// river box drawn at the world's origin was a flat water slab over the
    /// station plaza, 144 units above where it belongs).
    Placed { origin: [f32; 3], axis: [[f32; 3]; 3], class: String },
    /// No drawn entity names it: not drawn (the exe draws a brush model
    /// only through its entity). The class of the entity naming it, if any.
    Hidden(String),
}

/// bo2mp: per world surface, what [`BrushPlace`] its brush model gets, by
/// the exe's spawn rules (`crate::spawn::spawn_class`; glass, which the
/// spawn drops, goes to the glass system, drawn here at its entity).
pub fn brush_places(entities: &str, brush_models: &[BrushModel], surfaces: usize, mp: bool) -> Vec<BrushPlace> {
    let mut by_model: HashMap<usize, BrushPlace> = HashMap::new();
    for (_, keys) in crate::spawn::blocks(entities) {
        let get = |k: &str| keys.iter().find(|(a, _)| a == k).map(|(_, v)| v.as_str());
        let Some(k) = get("model").and_then(|m| m.strip_prefix('*')).and_then(|m| m.parse::<usize>().ok()) else { continue };
        let class = get("classname").unwrap_or("").to_owned();
        let vec3 = |key: &str| -> Option<[f32; 3]> {
            let v: Vec<f32> = get(key)?.split_whitespace().filter_map(|x| x.parse().ok()).collect();
            (v.len() == 3).then(|| [v[0], v[1], v[2]])
        };
        // (`wave_water` / `wave_clamp`, which the spawn drops too, go to the
        // water simulation: Metro's river between y -3392 and 3392 is
        // `*33`'s box at its entity, 144 units down, and nothing else.)
        let drawn = class.starts_with("glass")
            || class == "wave_water"
            || class == "wave_clamp"
            || crate::spawn::spawn_class(&keys, mp) == crate::spawn::T7Spawn::Entity;
        let place = if drawn {
            let angles = vec3("angles").unwrap_or_else(|| [0.0, get("angle").and_then(|a| a.trim().parse().ok()).unwrap_or(0.0), 0.0]);
            BrushPlace::Placed { origin: vec3("origin").unwrap_or_default(), axis: axis_of(angles), class }
        } else {
            BrushPlace::Hidden(class)
        };
        // (A drawn entity wins over another naming the same model.)
        match by_model.get(&k) {
            Some(BrushPlace::Placed { .. }) => {}
            _ => {
                by_model.insert(k, place);
            }
        }
    }
    let mut out = vec![BrushPlace::World; surfaces];
    for (k, b) in brush_models.iter().enumerate().skip(1) {
        if b.first_surface == u32::MAX || b.surface_count == 0 {
            continue;
        }
        let p = by_model.remove(&k).unwrap_or_else(|| BrushPlace::Hidden(String::new()));
        let start = b.first_surface as usize;
        for o in out.iter_mut().take((start + b.surface_count as usize).min(surfaces)).skip(start) {
            *o = p.clone();
        }
    }
    out
}

/// bo2mp: `p` (a brush model's own) in the world by its entity's origin and
/// axes (rows: forward, left, up).
pub fn brush_point(p: [f32; 3], origin: [f32; 3], axis: &[[f32; 3]; 3]) -> [f32; 3] {
    std::array::from_fn(|c| origin[c] + axis[0][c] * p[0] + axis[1][c] * p[1] + axis[2][c] * p[2])
}

#[cfg(test)]
mod image_chain_tests {
    /// Streamed pictures come with their whole mip chain (they came with
    /// the largest part's one level: no mips, the native renderer's
    /// shimmer). `T7_ZONE=<map.ff> cargo test -p fastfile_t7 --lib
    /// image_chain -- --ignored --nocapture`.
    #[test]
    #[ignore = "needs a staged map zone with its packages (T7_ZONE)"]
    fn streamed_pictures_have_their_mip_chain() {
        let zone = std::path::PathBuf::from(std::env::var("T7_ZONE").expect("T7_ZONE"));
        let map = super::T7Map::load(&super::MapSources::beside(&zone)).expect("map");
        let mut names: Vec<&String> = map.images.iter().filter(|(_, im)| im.xpak.len() > 1).map(|(n, _)| n).collect();
        names.sort();
        let (mut read, mut short) = (0usize, Vec::new());
        for n in names.iter().take(200) {
            let l = match map.image(n, u32::MAX) {
                Ok(l) => l,
                Err(e) => {
                    if read == 0 && short.len() < 3 {
                        println!("not read: {e}");
                    }
                    continue;
                }
            };
            read += 1;
            if read <= 5 {
                println!("{n}: {}x{} format {} {} levels", l.width, l.height, l.format, l.levels);
            }
            let full = super::texture::full_levels(l.width, l.height);
            // (The exe's smallest part may stop the chain a few levels early.)
            if l.levels + 3 < full {
                short.push(format!("{n}: {}x{} {} of {full} levels", l.width, l.height, l.levels));
            }
            let want: usize = (0..l.levels).filter_map(|k| super::texture::level_size(l.format, (l.width >> k).max(1), (l.height >> k).max(1))).sum();
            assert_eq!(l.data.len(), want, "{n}: bytes for {} levels", l.levels);
        }
        println!("{read} streamed pictures of {} read; short chains: {short:?}", names.len());
        assert!(read > 0);
        assert!(short.is_empty(), "{short:?}");
    }
}
