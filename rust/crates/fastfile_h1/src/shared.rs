//! Pictures and materials a zone names but does not hold.
//!
//! As in Ghosts (`fastfile_iw6::shared`), a material can name an image
//! another zone loads: the image's name starts with `,` and its record is
//! empty (format 0, no pixels, no stream parts). A model can name a whole
//! material the same way (no technique set, no textures). The retail map
//! zones take them from `common_mp` (and its patches); the zone tools'
//! usermaps hold their own (none on mp_rust or mp_courtyard_ss), but a map
//! built against Horizon MW's zones may name theirs. The engine's own
//! pictures (`$white`, `$black`, `$identitynormalmap`) are in no zone.
//!
//! `SharedImages` keeps what zones hold by name: images (the record, its
//! stream parts naming the same `imagefile<N>.pak`s, and the bytes of
//! inline pixels) and materials (technique set name, textures by image
//! name). A zone whose walk stops early adds what it walked.
//!
//! **Retail maps.** A retail map zone (`mp_shipment.ff`) holds none of its
//! materials: every one is a name (`,wc/h1_ch_brick_01`, `,m/...`). Their
//! definitions are in `techsets_<map>.ff` beside it (as IW7's maps), a few
//! in `techsets_common_mp.ff`; the definitions name their pictures only
//! (`,h1_ch_brick_01_col`), which are the map zone's own (most streamed
//! from the retail `imagefile<N>.pak`s), or `common_mp`'s and its patch's.
//! The game loads `common_mp`, its techsets and patch, then the map's
//! techsets, the map and the map's patch; a later zone's asset of a name
//! replaces an earlier one's. So a material is taken from the first of
//! `patch_<map>`, `techsets_<map>`, `patch_common_mp`, `techsets_common_mp`
//! (and the cores and Horizon MW's after), a picture from `patch_<map>`,
//! the map itself, then `patch_common_mp`, `common_mp`,
//! `code_post_gfx_mp` ([`retail_zones`]).
//!
//! `patch_common_mp` inflates to 628 MB with 525 MB of inline pictures and
//! `common_mp` to 1354 MB, so [`SharedImages::add_zone_for`] keeps only what
//! a [`Wanted`] list names (the map's placeholders and what their
//! definitions name), and the zone is dropped when it returns.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use crate::image::{ImagePaks, ImagePixels, image_pixels};
use crate::walk::{ImageRec, MaterialConstant, Span};

/// A material another zone holds, its images by name.
#[derive(Clone, Debug, Default)]
pub struct SharedMaterial {
    pub name: String,
    pub techset: String,
    pub sort_key: u8,
    /// (name hash, sampler state, semantic, image name)
    pub textures: Vec<(u32, u8, u8, String)>,
    pub constants: Vec<MaterialConstant>,
}

/// A shared image: its record, the bytes of its inline pixels, and whether
/// its zone loads after the map (`patch_<map>`: it replaces the map's own
/// picture of the name).
struct Held {
    rec: ImageRec,
    bytes: Vec<u8>,
    over_map: bool,
}

/// The images and materials some zones hold, by name (lower case, no
/// leading `,`).
#[derive(Default)]
pub struct SharedImages {
    by_name: HashMap<String, Held>,
    /// Each material with the rank of the zone it came from (lower wins).
    materials: HashMap<String, (u32, SharedMaterial)>,
    /// The zones read, for reports.
    pub zones: Vec<String>,
    /// Bytes of inline pixels kept.
    pub inline_bytes: usize,
    /// While set, a zone's own-pak parts (file index 96) are given this
    /// index, and its 1 x 1 stand-ins are not taken (a guest zone read
    /// for another guest zone's pictures).
    guest_pak: Option<u16>,
}

/// What a map wants of the shared zones, by key (`image_key`): the
/// materials it only names, the pictures it only names or its materials'
/// definitions name, and the pictures the map holds itself (wanted from a
/// zone only when that zone replaces the map's, `over_map`).
#[derive(Clone, Debug, Default)]
pub struct Wanted {
    pub materials: HashSet<String>,
    pub images: HashSet<String>,
    pub map_held: HashSet<String>,
}

impl Wanted {
    /// What a walked map zone names but does not hold, of what the
    /// materials at `used` (capture indices: the drawn world's and placed
    /// models') need: those of them it only names, and the pictures its
    /// own definitions of them only name.
    pub fn of_map_materials(
        cap: &crate::walk::Capture,
        used: &std::collections::BTreeSet<usize>,
    ) -> Self {
        let mut w = Wanted::default();
        for i in &cap.images {
            if !is_reference(i) {
                w.map_held.insert(image_key(&i.name));
            }
        }
        for m in used.iter().filter_map(|&i| cap.materials.get(i)) {
            if m.techset.is_none() && m.textures.is_empty() {
                w.materials.insert(image_key(&m.name));
                continue;
            }
            for t in &m.textures {
                let Some(rec) = t.image.and_then(|i| cap.images.get(i.index as usize)) else {
                    continue;
                };
                let k = image_key(&rec.name);
                if is_reference(rec) && builtin_pixels(&k).is_none() && !w.map_held.contains(&k) {
                    w.images.insert(k);
                }
            }
        }
        w
    }

    /// Whether anything is still wanted.
    pub fn is_empty(&self) -> bool {
        self.materials.is_empty() && self.images.is_empty()
    }
}

/// What one zone gave (`SharedImages::add_zone_for`).
#[derive(Clone, Debug, Default)]
pub struct ZoneGave {
    pub name: String,
    pub materials: usize,
    pub images: usize,
    pub inline_bytes: usize,
    /// The asset the walk stopped at, if it did.
    pub stopped_at: Option<usize>,
}

impl std::fmt::Display for ZoneGave {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{} ({} materials, {} pictures, {} KB inline{})",
            self.name,
            self.materials,
            self.images,
            self.inline_bytes / 1024,
            self.stopped_at
                .map_or_else(String::new, |a| format!("; walk stopped at asset {a}"))
        )
    }
}

/// The zones a retail map's materials and pictures may be in, beside it
/// in `zone_dir`, highest priority first, with whether each loads after
/// the map (its pictures replace the map's): `patch_<map>`,
/// `techsets_<map>`, then the common zones (`patch_common_mp`,
/// `techsets_common_mp`, `common_mp`, the core ones) and the engine's
/// (`code_post_gfx_mp`, loaded first of all). Those present.
pub fn retail_zones(zone_dir: &Path, map: &str) -> Vec<(PathBuf, bool)> {
    let map = map.to_ascii_lowercase();
    [
        (format!("patch_{map}"), true),
        (format!("techsets_{map}"), false),
        ("patch_common_mp".to_owned(), false),
        ("techsets_common_mp".to_owned(), false),
        ("common_mp".to_owned(), false),
        ("patch_common_core_mp".to_owned(), false),
        ("techsets_common_core_mp".to_owned(), false),
        ("common_core_mp".to_owned(), false),
        // (The engine's own pictures: `h1_og_shadow`, the BRDF tables.)
        ("code_post_gfx_mp".to_owned(), false),
    ]
    .into_iter()
    .map(|(z, over)| (zone_dir.join(format!("{z}.ff")), over))
    .filter(|(p, _)| p.is_file())
    .collect()
}

/// Whether a zone can hold pictures a map wants (a `techsets_*` zone holds
/// materials whose pictures are all names).
pub fn holds_pictures(path: &Path) -> bool {
    !path.file_stem().is_some_and(|s| {
        s.to_string_lossy()
            .to_ascii_lowercase()
            .starts_with("techsets_")
    })
}

/// The order to read `zones` (highest priority first, as
/// `retail_zones` gives them) in: the zones that only define materials
/// first, so that every material is chosen before the pictures it names
/// are looked for; the rank each material from a zone gets is its place
/// in `zones`.
pub fn read_order(zones: &[(PathBuf, bool)]) -> Vec<usize> {
    let mut order: Vec<usize> = (0..zones.len())
        .filter(|&i| !holds_pictures(&zones[i].0))
        .collect();
    order.extend((0..zones.len()).filter(|&i| holds_pictures(&zones[i].0)));
    order
}

/// An image or material name as a key: lower case, the reference mark
/// dropped.
pub fn image_key(name: &str) -> String {
    name.trim_start_matches(',').to_ascii_lowercase()
}

/// Whether an image record only names an image held elsewhere.
pub fn is_reference(rec: &ImageRec) -> bool {
    rec.format == 0 && rec.pixels.is_none() && !rec.streamed
}

impl SharedImages {
    /// Adds every image and material `path` holds (one already known keeps
    /// its first record: read a patch zone before the zone it patches).
    /// The number of images added.
    pub fn add_zone(&mut self, path: &Path) -> Result<usize, String> {
        let rank = self.zones.len() as u32;
        self.add_zone_for(path, None, rank, false)
            .map(|gave| gave.images)
    }

    /// Adds what `path` holds of `wanted` (everything with `None`): its
    /// materials first, each kept unless one of a lower `rank` is (and
    /// the pictures a kept one names joining `wanted`), then its pictures
    /// not yet held. `over_map`: the zone loads after the map, so its
    /// pictures replace the map's own (`Wanted::map_held`) too. What was
    /// found is taken out of `wanted`. The zone is dropped on return.
    pub fn add_zone_for(
        &mut self,
        path: &Path,
        mut wanted: Option<&mut Wanted>,
        rank: u32,
        over_map: bool,
    ) -> Result<ZoneGave, String> {
        let zone = crate::Zone::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
        let (cap, stop) = crate::walk::walk_zone(&zone);
        let mut gave = ZoneGave {
            name: path
                .file_stem()
                .map_or_else(String::new, |s| s.to_string_lossy().into_owned()),
            stopped_at: stop.map(|s| s.asset_index),
            ..Default::default()
        };
        for m in &cap.materials {
            let Some(ts) = m.techset else { continue };
            let key = image_key(&m.name);
            let known = self.materials.get(&key).map(|(r, _)| *r);
            if known.is_some_and(|r| r <= rank) {
                continue;
            }
            if let Some(w) = wanted.as_deref()
                && known.is_none()
                && !w.materials.contains(&key)
            {
                continue;
            }
            let textures: Vec<(u32, u8, u8, String)> = m
                .textures
                .iter()
                .filter_map(|t| {
                    let img = &cap.images.get(t.image?.index as usize)?.name;
                    Some((t.name_hash, t.sampler_state, t.semantic, img.clone()))
                })
                .collect();
            if let Some(w) = wanted.as_deref_mut() {
                w.materials.remove(&key);
                for (_, _, _, img) in &textures {
                    let k = image_key(img);
                    if builtin_pixels(&k).is_none()
                        && !w.map_held.contains(&k)
                        && !self.by_name.contains_key(&k)
                    {
                        w.images.insert(k);
                    }
                }
            }
            self.materials.insert(
                key,
                (
                    rank,
                    SharedMaterial {
                        name: m.name.trim_start_matches(',').to_owned(),
                        techset: cap.name_of(ts).trim_start_matches(',').to_owned(),
                        sort_key: m.sort_key,
                        textures,
                        constants: m.constants.clone(),
                    },
                ),
            );
            gave.materials += 1;
        }
        for rec in &cap.images {
            if is_reference(rec) {
                continue;
            }
            let key = image_key(&rec.name);
            if self.by_name.contains_key(&key) {
                continue;
            }
            if self.guest_pak.is_some() && is_standin(rec) {
                continue;
            }
            if let Some(w) = wanted.as_deref_mut() {
                let take = w.images.remove(&key) || (over_map && w.map_held.contains(&key));
                if !take {
                    continue;
                }
            }
            let mut rec = rec.clone();
            if let Some(idx) = self.guest_pak {
                for part in rec.stream_files.iter_mut().flatten() {
                    if part.file_index == crate::image::ZONE_PAK_INDEX {
                        part.file_index = idx;
                    }
                }
            }
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
            gave.inline_bytes += bytes.len();
            self.by_name.insert(
                key,
                Held {
                    rec,
                    bytes,
                    over_map,
                },
            );
            gave.images += 1;
        }
        self.inline_bytes += gave.inline_bytes;
        self.zones.push(gave.name.clone());
        Ok(gave)
    }

    pub fn len(&self) -> usize {
        self.by_name.len()
    }

    pub fn is_empty(&self) -> bool {
        self.by_name.is_empty()
    }

    pub fn material_count(&self) -> usize {
        self.materials.len()
    }

    /// A material by name (`,name` or `name`).
    pub fn material(&self, name: &str) -> Option<&SharedMaterial> {
        self.materials.get(&image_key(name)).map(|(_, m)| m)
    }

    /// The record of an image by name (`,name` or `name`).
    pub fn get(&self, name: &str) -> Option<&ImageRec> {
        self.by_name.get(&image_key(name)).map(|h| &h.rec)
    }

    /// Whether the image of this name comes from a zone loaded after the
    /// map (it replaces the map's own).
    pub fn replaces_map(&self, name: &str) -> bool {
        self.by_name
            .get(&image_key(name))
            .is_some_and(|h| h.over_map)
    }

    /// An image's pixels by name, as `image::image_pixels` reads them
    /// (streamed parts from `paks`).
    pub fn pixels(
        &self,
        name: &str,
        paks: &mut ImagePaks,
        max_size: u32,
    ) -> Result<ImagePixels, String> {
        let held = self.by_name.get(&image_key(name)).ok_or_else(|| {
            format!(
                "{} is in none of {:?}",
                name.trim_start_matches(','),
                self.zones
            )
        })?;
        image_pixels(&held.rec, &held.bytes, paks, max_size)
    }
}

/// Whether a zone's own picture is a 1 x 1 stand-in for a picture held
/// elsewhere: inline, one pixel, not the engine's own (`$white`, ...).
/// Horizon MW's weapon zones hold pure red ones (`255 0 0 255`) in place of
/// the muzzle flash pictures (`muzzle_flash_round_01_tip`,
/// `muzzle_flash_glow_01`, `smk_white_muzzle_tip`) and of other pictures of
/// the retail `common_mp`: drawn as they are, a flash sprite is a solid red
/// square (the "red mist" when a gun is fired) and a gun's material a red
/// patch.
pub fn is_standin(rec: &ImageRec) -> bool {
    !is_reference(rec)
        && !rec.streamed
        && rec.width == 1
        && rec.height == 1
        && rec.pixels.is_some()
        && builtin_pixels(&rec.name).is_none()
}

/// A gun's camo layer slot (`camo_transparent_col`, `camo_nml`,
/// `~camo_spcmaps_01...`): a 1 x 1 red picture in every zone (retail ones
/// too), swapped for the chosen camo at run time. With no camo chosen the
/// slot is neutral; drawn as it is, the layer paints the whole gun red.
pub fn is_camo_slot(rec: &ImageRec) -> bool {
    let n = rec.name.trim_start_matches(',').trim_start_matches('~');
    n.starts_with("camo_") && is_standin(rec)
}

/// What the retail zones hold of the pictures a guest zone stands in for.
#[derive(Default)]
struct StandinStore {
    dir: Option<PathBuf>,
    images: SharedImages,
    /// Keys already looked for (found or not): never read for again.
    searched: HashSet<String>,
}

static STANDINS: std::sync::Mutex<Option<StandinStore>> = std::sync::Mutex::new(None);

/// The retail zones that can hold a stand-in's real picture, highest
/// priority first: `retail_zones` of no map, without the techset zones.
fn standin_zones(dir: &Path) -> Vec<(PathBuf, bool)> {
    retail_zones(dir, "")
        .into_iter()
        .filter(|(p, _)| holds_pictures(p))
        .collect()
}

const GUEST_PAK_BASE: u16 = 1000;

/// Horizon MW's zones (`hmw-mod/zone/hmw_*.ff`, beside the install's
/// `zone/`), the weapon and attachment ones first.
fn guest_zones(dir: &Path) -> Vec<PathBuf> {
    let Some(mod_zone) = dir.parent().map(|p| p.join("hmw-mod").join("zone")) else {
        return Vec::new();
    };
    let Ok(rd) = std::fs::read_dir(&mod_zone) else {
        return Vec::new();
    };
    let mut v: Vec<PathBuf> = rd
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| {
            p.extension().is_some_and(|x| x == "ff")
                && p.file_stem()
                    .and_then(|n| n.to_str())
                    .is_some_and(|n| n.starts_with("hmw_") && !n.contains("launcher") && !n.contains("_ui_"))
        })
        .collect();
    // (The patch zones last: they are the largest.)
    v.sort_by_key(|p| (p.to_string_lossy().contains("patch_"), p.clone()));
    v
}

/// bo2mp: which zone held each stand-in's real picture, kept between runs
/// (`codlegacy-artifacts/h1_standins.cache`). The lookup reads every retail
/// and guest zone for the names it does not find (20 s a match start);
/// with this a repeat run reads only the zones that held something, and
/// skips the names no zone had. It is thrown away when any zone file's
/// size or time changes.
const STANDIN_CACHE: &str = "codlegacy-artifacts/h1_standins.cache";

fn standin_fingerprint(zones: &[PathBuf]) -> String {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for p in zones {
        let (len, secs) = std::fs::metadata(p)
            .map(|m| {
                let t = m.modified().ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map_or(0, |d| d.as_secs());
                (m.len(), t)
            })
            .unwrap_or((0, 0));
        for b in format!("{}|{len}|{secs};", p.display()).bytes() {
            h ^= u64::from(b);
            h = h.wrapping_mul(0x0100_0000_01b3);
        }
    }
    format!("v1 {h:016x}")
}

/// name -> the zone that held it (`None`: no zone did), when the saved
/// cache is for these zones.
fn read_standin_cache(fp: &str) -> std::collections::HashMap<String, Option<String>> {
    let mut out = std::collections::HashMap::new();
    let Ok(text) = std::fs::read_to_string(STANDIN_CACHE) else {
        return out;
    };
    let mut lines = text.lines();
    if lines.next() != Some(fp) {
        return out;
    }
    for l in lines {
        let mut f = l.split('\t');
        match (f.next(), f.next(), f.next()) {
            (Some("F"), Some(n), Some(z)) => {
                out.insert(n.to_owned(), Some(z.to_owned()));
            }
            (Some("U"), Some(n), _) => {
                out.insert(n.to_owned(), None);
            }
            _ => {}
        }
    }
    out
}

fn write_standin_cache(fp: &str, map: &std::collections::HashMap<String, Option<String>>) {
    let mut text = format!("{fp}\n");
    for (n, z) in map {
        match z {
            Some(z) => text.push_str(&format!("F\t{n}\t{z}\n")),
            None => text.push_str(&format!("U\t{n}\n")),
        }
    }
    let _ = std::fs::create_dir_all("codlegacy-artifacts");
    let _ = std::fs::write(STANDIN_CACHE, text);
}

/// Looks the zone's 1 x 1 stand-ins (`is_standin`) up in the retail zones
/// in `dir` (the install's `zone/`) once, keeping their pictures for
/// `standin_pixels`. Names already looked for are not looked for again, so
/// a zone that only repeats the last one's reads nothing. A line for the
/// log, or `None` when nothing new was wanted.
pub fn load_standins(dir: &Path, images: &[ImageRec]) -> Option<String> {
    let mut guard = STANDINS.lock().ok()?;
    let store = guard.get_or_insert_with(StandinStore::default);
    if store.dir.as_deref() != Some(dir) {
        *store = StandinStore {
            dir: Some(dir.to_path_buf()),
            ..Default::default()
        };
    }
    let mut wanted = Wanted::default();
    for rec in images.iter().filter(|r| is_standin(r)) {
        let key = image_key(&rec.name);
        if store.searched.insert(key.clone()) {
            wanted.images.insert(key);
        }
    }
    if wanted.images.is_empty() {
        return None;
    }
    let asked = wanted.images.len();
    let started = std::time::Instant::now();
    let zones = standin_zones(dir);
    let guest_list = guest_zones(dir);
    // bo2mp: the saved lookup, when every name asked is in it: only the
    // zones that held a name are read, and the names no zone had are skipped.
    let all_zones: Vec<PathBuf> = zones.iter().map(|(p, _)| p.clone()).chain(guest_list.iter().cloned()).collect();
    let fp = standin_fingerprint(&all_zones);
    let cache = read_standin_cache(&fp);
    let mut only: Option<HashSet<String>> = None;
    let mut skipped = 0usize;
    if !cache.is_empty() && wanted.images.iter().all(|n| cache.contains_key(n)) {
        let mut hold = HashSet::new();
        wanted.images.retain(|n| match cache.get(n) {
            Some(Some(z)) => {
                hold.insert(z.clone());
                true
            }
            _ => {
                skipped += 1;
                false
            }
        });
        only = Some(hold);
    }
    let mut found_in: Vec<(String, String)> = Vec::new();
    let mut gave = Vec::new();
    for (i, (path, over_map)) in zones.iter().enumerate() {
        if wanted.images.is_empty() {
            break;
        }
        if only.as_ref().is_some_and(|o| !o.contains(&path.display().to_string())) {
            continue;
        }
        let before = if only.is_none() { Some(wanted.images.clone()) } else { None };
        let read = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            store
                .images
                .add_zone_for(path, Some(&mut wanted), i as u32, *over_map)
        }));
        if let Some(before) = before {
            found_in.extend(before.difference(&wanted.images).map(|n| (n.clone(), path.display().to_string())));
        }
        match read {
            Ok(Ok(g)) => gave.push(g.to_string()),
            Ok(Err(e)) => gave.push(e),
            Err(_) => gave.push(format!("{} (reader panicked)", path.display())),
        }
    }
    // Horizon MW's weapon zones hold each other's pictures (a gun's
    // stand-in is real in the zone of the gun that owns it).
    let mut guests = 0;
    if !wanted.images.is_empty() {
        for (k, path) in guest_list.iter().enumerate() {
            if wanted.images.is_empty() {
                break;
            }
            if only.as_ref().is_some_and(|o| !o.contains(&path.display().to_string())) {
                continue;
            }
            let before = if only.is_none() { Some(wanted.images.clone()) } else { None };
            let idx = GUEST_PAK_BASE + k as u16;
            crate::image::register_extra_pak(idx, path.with_extension("pak"));
            store.images.guest_pak = Some(idx);
            let read = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                store.images.add_zone_for(path, Some(&mut wanted), 1000 + k as u32, false)
            }));
            store.images.guest_pak = None;
            if let Some(before) = before {
                found_in.extend(before.difference(&wanted.images).map(|n| (n.clone(), path.display().to_string())));
            }
            guests += 1;
            match read {
                Ok(Ok(g)) => gave.push(g.to_string()),
                Ok(Err(e)) => gave.push(e),
                Err(_) => gave.push(format!("{} (reader panicked)", path.display())),
            }
        }
    }
    // A full lookup is saved for the next run.
    if only.is_none() {
        let mut map = cache;
        for (n, z) in found_in {
            map.insert(n, Some(z));
        }
        for n in &wanted.images {
            map.insert(n.clone(), None);
        }
        write_standin_cache(&fp, &map);
    }
    let mut unfound: Vec<&String> = wanted.images.iter().collect();
    unfound.sort();
    unfound.truncate(40);
    Some(format!(
        "{asked} stand-in pictures looked for ({}), {} not found in {} retail zones and {guests} guest zones [{:.1} s]: {}; not found: {unfound:?}",
        if only.is_some() { format!("from the saved lookup, {skipped} known to have none") } else { "all zones read, saved".to_owned() },
        wanted.images.len(),
        zones.len(),
        started.elapsed().as_secs_f64(),
        gave.join(", ")
    ))
}

/// The retail picture of a zone's stand-in, when `load_standins` found
/// one: `None` for any other name (read the zone's own).
pub fn standin_pixels(
    name: &str,
    paks: &mut ImagePaks,
    max_size: u32,
) -> Option<Result<ImagePixels, String>> {
    let guard = STANDINS.lock().ok()?;
    let store = guard.as_ref()?;
    store.images.get(name)?;
    Some(store.images.pixels(name, paks, max_size))
}

/// The engine's built-in pictures a material can name (`$white`, `$black`,
/// `$identitynormalmap`, ...): 4 x 4 `R8G8B8A8_UNORM`, one level, or `None`
/// for a name that is not one of them.
pub fn builtin_pixels(name: &str) -> Option<ImagePixels> {
    let texel: [u8; 4] = match image_key(name).as_str() {
        "$white" => [255, 255, 255, 255],
        "$black" => [0, 0, 0, 255],
        "$gray" | "$grey" => [128, 128, 128, 255],
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

/// A neutral picture for a stand-in no zone holds the real one of (Horizon
/// MW's gun pictures named `i_h2_*`, `camo_*_flow_surfparms_01`, ...): by
/// the picture's suffix, a flat normal, a dark metal colour, a dull specular,
/// an open occlusion, instead of the stand-in's red.
pub fn neutral_pixels(name: &str) -> ImagePixels {
    let n = image_key(name);
    let n = n.trim_start_matches('~');
    let texel: [u8; 4] = if n.contains("transparent") {
        [0, 0, 0, 0]
    } else if n.ends_with("_n") || n.contains("normal") || n.contains("nml") {
        [128, 128, 255, 255]
    } else if n.ends_with("_o") || n.contains("occlusion") {
        [255, 255, 255, 255]
    } else if n.ends_with("_s") || n.contains("surfparms") || n.contains("spec") || n.contains("spc") {
        [48, 48, 48, 255]
    } else {
        [64, 64, 68, 255]
    };
    ImagePixels {
        width: 4,
        height: 4,
        levels: 1,
        dxgi_format: 28,
        data: texel.repeat(16),
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn neutral_stand_ins_are_not_red() {
        for n in ["i_h2_glock_body_c", "i_h2_glock_body_n", "camo_body_flow_surfparms_01", "i_h2_aa12_rail_s", "i_h2_cheytac_scope_lens_o"] {
            let p = super::neutral_pixels(n);
            assert_eq!(p.data.len(), 64);
            assert!(!(p.data[0] > 150 && p.data[1] < 100), "{n} came out red");
        }
        assert_eq!(&super::neutral_pixels("i_h2_x_n").data[..4], &[128, 128, 255, 255]);
        assert_eq!(&super::neutral_pixels("camo_transparent_col").data[..4], &[0, 0, 0, 0]);
        assert_eq!(&super::neutral_pixels("camo_nml").data[..4], &[128, 128, 255, 255]);
        assert_eq!(&super::neutral_pixels("~camo_spcmaps_01_spc-rgb&camo~b29a8732").data[..4], &[48, 48, 48, 255]);
    }

    #[test]
    fn material_zones_are_read_before_picture_zones() {
        let z = |n: &str| (std::path::PathBuf::from(format!("{n}.ff")), false);
        let zones = [
            z("patch_mp_x"),
            z("techsets_mp_x"),
            z("patch_common_mp"),
            z("techsets_common_mp"),
            z("common_mp"),
        ];
        assert_eq!(super::read_order(&zones), vec![1, 3, 0, 2, 4]);
    }

    fn pic(name: &str, w: u16, h: u16, inline: bool, streamed: bool) -> crate::walk::ImageRec {
        crate::walk::ImageRec {
            name: name.to_owned(),
            format: if inline || streamed { 28 } else { 0 },
            width: w,
            height: h,
            streamed,
            pixels: inline.then_some(crate::walk::Span { pos: 0, len: 4 }),
            ..Default::default()
        }
    }

    /// Horizon MW's muzzle flash pictures are 1 x 1 red stand-ins in its
    /// weapon zones: found as stand-ins, and not mistaken for real
    /// pictures, references or the engine's own.
    #[test]
    fn one_pixel_inline_pictures_are_standins() {
        assert!(super::is_standin(&pic(",muzzle_flash_round_01_tip", 1, 1, true, false)));
        assert!(!super::is_standin(&pic("muzzle_flash_round_01_tip", 512, 512, true, false)));
        assert!(!super::is_standin(&pic("smk_puff_a", 1, 1, false, true)));
        assert!(!super::is_standin(&pic(",wc/other", 1, 1, false, false)));
        assert!(!super::is_standin(&pic("$white", 1, 1, true, false)));
    }

    #[test]
    fn camo_slots_are_found() {
        assert!(super::is_camo_slot(&pic("camo_transparent_col", 1, 1, true, false)));
        assert!(super::is_camo_slot(&pic("~camo_spcmaps_01_spc-rgb&camo~b29a8732", 1, 1, true, false)));
        assert!(!super::is_camo_slot(&pic("camo_body_flow", 512, 512, true, false)));
        assert!(!super::is_camo_slot(&pic("i_h2_aug_body_m", 1, 1, true, false)));
    }

    /// `H1_RETAIL_ZONE=<install>/zone H1_STANDIN_ZONE=<hmw zone .ff> cargo test
    /// -p fastfile_h1 --lib standins_come_from_retail -- --ignored --nocapture`:
    /// the muzzle flash pictures of a Horizon MW weapon zone come out of the
    /// retail `common_mp` as real pictures, not 1 x 1 red.
    #[test]
    #[ignore]
    fn standins_come_from_retail() {
        let (Some(retail), Some(zone)) = (
            std::env::var_os("H1_RETAIL_ZONE"),
            std::env::var_os("H1_STANDIN_ZONE"),
        ) else {
            return;
        };
        let zone = crate::Zone::open(std::path::Path::new(&zone)).expect("zone");
        let (cap, _) = crate::walk::walk_zone(&zone);
        let line = super::load_standins(std::path::Path::new(&retail), &cap.images);
        println!("{line:?}");
        let mut paks = crate::image::ImagePaks::for_zone(
            std::path::Path::new(&retail).join("x.ff").as_path(),
            Some(std::path::Path::new(&retail)),
        );
        for name in ["muzzle_flash_round_01_tip", "muzzle_flash_glow_01", "smk_white_muzzle_tip"] {
            let px = super::standin_pixels(name, &mut paks, 256)
                .expect("held")
                .expect("decodes");
            println!("{name}: {}x{} fmt {}", px.width, px.height, px.dxgi_format);
            assert!(px.width > 1 && px.height > 1);
        }
    }

    #[test]
    fn keys_drop_the_reference_mark() {
        assert_eq!(super::image_key(",MTL_Rust_Wall"), "mtl_rust_wall");
        assert!(super::builtin_pixels(",$white").is_some_and(|p| p.data.len() == 64));
        assert!(super::builtin_pixels("rust_wall").is_none());
    }
}
