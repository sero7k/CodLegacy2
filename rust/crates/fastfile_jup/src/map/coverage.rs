//! Where a map's pictures come from, read offline: which of them the zone
//! carries itself, which the install's streamed packages hold (and in which
//! part), which only a download package names, and which nothing names.
//!
//! The zone `.xpak` files are indexes: a row per package entry, the part an
//! entry sits in being `offset / stride`. Resolving a picture therefore needs
//! only the indexes, not the `.xsub` parts (about 900 MB each), so this
//! works on a machine that has the zones and the small indexes alone. The
//! same answer names the parts (and byte ranges) a full read would open, which
//! is what a bounded extraction from the install needs.
//!
//! Per picture the groups of mip levels are looked up from the largest
//! down ([`crate::image`]): the picture's [`Source`] is where its best
//! available level is.

use std::collections::{BTreeMap, HashMap};
use std::path::Path;

use super::index::ZoneImage;
use super::materials::{self, MapImage};
use super::{MapSources, models, world};
use crate::image;
use crate::oodle::Decompressor;
use crate::xsub::{self, XSubEntry};

/// One zone `.xpak` index, whether or not its parts are present.
pub struct PackRows {
    pub name: String,
    /// `*_cdn` indexes name packages the install does not hold.
    pub cdn: bool,
    pub part_count: u64,
    /// Bytes per part slot in an entry's offset.
    pub stride: u64,
    /// Sizes of the parts, where known (the parts themselves or a listing).
    pub part_sizes: Vec<Option<u64>>,
    pub entries: Vec<XSubEntry>,
}

/// Where one package entry is.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Located {
    pub pack: String,
    pub cdn: bool,
    pub part: usize,
    pub offset: u64,
    pub size: u32,
    pub decompressed: u32,
    pub compressed: bool,
    /// Whether the part exists in the install (when known: `None` from an
    /// index read with no listing of the parts).
    pub part_present: Option<bool>,
}

/// Every zone index of an install.
#[derive(Default)]
pub struct PackIndex {
    pub packs: Vec<PackRows>,
}

/// Part sizes from a listing (`zone\name-00000.xsub<TAB>bytes` per line, the
/// `file_list.txt` the owner's listing script writes).
#[must_use]
pub fn read_listing(text: &str) -> HashMap<String, u64> {
    text.lines()
        .filter_map(|l| {
            let (name, size) = l.trim_end().rsplit_once('\t')?;
            let file = name.rsplit(['\\', '/']).next()?.to_ascii_lowercase();
            file.ends_with(".xsub").then(|| size.trim().parse().ok().map(|s| (file, s)))?
        })
        .collect()
}

impl PackIndex {
    /// Reads `<install>/zone/*.xpak`. Parts are looked for beside them; where
    /// they are absent `listing` (file name -> size) stands in, else the
    /// stride is the usual one (256 MiB for `boot`, 1 GiB elsewhere).
    pub fn open(install: &Path, listing: &HashMap<String, u64>) -> Self {
        let mut files: Vec<_> = std::fs::read_dir(install.join("zone"))
            .map(|d| d.flatten().map(|e| e.path()).collect::<Vec<_>>())
            .unwrap_or_default();
        files.retain(|p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("xpak")));
        files.sort();
        let mut packs = Vec::new();
        for f in files {
            let Ok(mut file) = std::fs::File::open(&f) else { continue };
            let Ok((header, entries)) = xsub::read_index(&mut file) else { continue };
            let name = f.file_stem().and_then(|s| s.to_str()).unwrap_or("").to_owned();
            let cdn = header.part_count == 0 || name.ends_with("_cdn");
            let dir = f.parent().unwrap_or(Path::new("."));
            let part_sizes: Vec<Option<u64>> = (0..header.part_count)
                .map(|i| {
                    let file = format!("{name}-{i:05}.xsub");
                    std::fs::metadata(dir.join(&file))
                        .map(|m| m.len())
                        .ok()
                        .or_else(|| listing.get(&file.to_ascii_lowercase()).copied())
                })
                .collect();
            let largest = part_sizes.iter().flatten().copied().max().unwrap_or(0);
            let stride = if cdn {
                0
            } else if largest > 0 {
                largest.next_power_of_two()
            } else if name == "boot" {
                1 << 28
            } else {
                1 << 30
            };
            packs.push(PackRows { name, cdn, part_count: header.part_count, stride, part_sizes, entries });
        }
        Self { packs }
    }

    /// The entry under `key`: the first local index that has it, else a
    /// download index's row.
    #[must_use]
    pub fn locate(&self, key: u64) -> Option<Located> {
        let mut cdn_hit = None;
        for p in &self.packs {
            let Ok(i) = p.entries.binary_search_by_key(&key, |e| e.key) else { continue };
            let e = p.entries[i];
            if p.cdn {
                cdn_hit.get_or_insert(Located {
                    pack: p.name.clone(),
                    cdn: true,
                    part: 0,
                    offset: e.offset,
                    size: e.size,
                    decompressed: e.decompressed,
                    compressed: e.compressed,
                    part_present: Some(false),
                });
                continue;
            }
            let part = (e.offset / p.stride.max(1)) as usize;
            let offset = e.offset % p.stride.max(1);
            let present = p.part_sizes.get(part).map(|s| s.is_some_and(|s| offset + u64::from(e.size) <= s));
            return Some(Located {
                pack: p.name.clone(),
                cdn: false,
                part,
                offset,
                size: e.size,
                decompressed: e.decompressed,
                compressed: e.compressed,
                part_present: present,
            });
        }
        cdn_hit
    }
}

/// Where a picture's best level is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Source {
    /// Every level is in the zone.
    Inline,
    /// The largest level is in a streamed part of the install.
    Xsub,
    /// Only smaller levels are in the install; the larger ones are in
    /// download packages or in none (the picture is drawn at a lower size).
    XsubPartial,
    /// Only a download package names it.
    Cdn,
    /// Nothing names it, and the zone holds no pixels.
    Missing,
    /// The format byte is not one this reader decodes.
    Unsupported,
}

impl Source {
    pub const ALL: [Source; 6] = [Source::Inline, Source::Xsub, Source::XsubPartial, Source::Cdn, Source::Missing, Source::Unsupported];

    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Source::Inline => "inline",
            Source::Xsub => "xsub",
            Source::XsubPartial => "xsub-partial",
            Source::Cdn => "cdn",
            Source::Missing => "missing",
            Source::Unsupported => "unsupported",
        }
    }
}

/// One picture, resolved.
#[derive(Clone, Debug)]
pub struct PictureRow {
    pub hash: u64,
    pub width: u16,
    pub height: u16,
    pub format: u8,
    pub usage: u8,
    pub source: Source,
    /// The largest side the install can give (0 if none).
    pub best_side: u32,
    /// The part holding the best level.
    pub located: Option<Located>,
    /// The groups: found in a local part, in a download index, in none.
    pub groups: (usize, usize, usize),
    /// Where the largest group is: `Some(false)` in a local index, `Some(true)`
    /// in a download index only, `None` in no index.
    pub top_cdn: Option<bool>,
    /// Every group found in a local index: (key, where, its largest level's side).
    pub local_groups: Vec<(u64, Located, u32)>,
}

/// What a map's pictures come to.
#[derive(Default)]
pub struct MapCoverage {
    pub name: String,
    /// Material-bound pictures (surfaces' and placed models' materials).
    pub materials: usize,
    pub materials_without_header: usize,
    pub materials_without_pictures: usize,
    /// Materials whose colour picture is not found.
    pub materials_without_colour: usize,
    pub pictures: Vec<PictureRow>,
    /// The other pictures of the zone (sky, probes, grades, terrain tiles).
    pub others: Vec<PictureRow>,
    pub report: Vec<String>,
}

/// Resolves one picture against the indexes.
#[must_use]
pub fn resolve(img: &MapImage, usage: u8, idx: &PackIndex) -> PictureRow {
    let mut row = PictureRow {
        hash: img.hash,
        width: img.width,
        height: img.height,
        format: img.format,
        usage,
        source: Source::Missing,
        best_side: 0,
        located: None,
        groups: (0, 0, 0),
        top_cdn: None,
        local_groups: Vec::new(),
    };
    let dxgi = image::dxgi(img.format);
    // A picture of formats this reader cannot decode is counted as such,
    // whatever its source (the sky, probes and grade are others').
    let side_of = |g: usize| -> u32 {
        let rec = image::ImageRec {
            at: 0,
            hash: img.hash,
            flags: 0,
            data_size: 0,
            width: img.width,
            height: img.height,
            depth: 1,
            array: 1,
            format: img.format,
            usage: 0,
            levels: 0,
            groups: img.groups.clone(),
            inline: None,
            end: 0,
        };
        dxgi.and_then(|d| image::group_levels(&rec, g, d))
            .and_then(|ls| ls.iter().map(|l| l.1.max(l.2)).max())
            .unwrap_or(0)
    };
    // The groups are looked up largest first.
    let mut local_best: Option<(usize, Located)> = None;
    let mut cdn_seen = false;
    for (g, group) in img.groups.iter().enumerate().rev() {
        let found = idx.locate(group.key);
        if g + 1 == img.groups.len() {
            row.top_cdn = found.as_ref().map(|l| l.cdn);
        }
        match found {
            Some(l) if !l.cdn => {
                row.groups.0 += 1;
                row.local_groups.push((group.key, l.clone(), side_of(g)));
                if local_best.is_none() {
                    local_best = Some((g, l));
                }
            }
            Some(_) => {
                row.groups.1 += 1;
                cdn_seen = true;
            }
            None => row.groups.2 += 1,
        }
    }
    if let Some((g, l)) = local_best {
        row.best_side = side_of(g);
        row.source = if g + 1 == img.groups.len() { Source::Xsub } else { Source::XsubPartial };
        row.located = Some(l);
    } else if img.inline.is_some() {
        row.source = Source::Inline;
        row.best_side = u32::from(img.width.max(img.height));
    } else if cdn_seen {
        row.source = Source::Cdn;
    }
    if dxgi.is_none() && row.source != Source::Inline {
        row.source = Source::Unsupported;
    }
    row
}

/// Reads a map's materials and pictures (no meshes) and resolves every
/// picture against `idx`.
pub fn analyse(src: &MapSources, oodle: &dyn Decompressor, idx: &PackIndex) -> Result<MapCoverage, String> {
    let mut out = MapCoverage { name: src.name.clone(), ..Default::default() };
    let zone: ZoneImage = super::read_image(&src.zone(&src.name), oodle)?;
    let z = &zone.bytes;
    let (list, count) = world::find_surfaces(z).ok_or("no surface list (gfx_map) in the map zone")?;
    let recs = world::records(z, list, count);
    let mut rep = Vec::new();
    let placed = models::read(&zone, list, &mut rep);
    let mut extra: Vec<usize> = placed.models.values().flat_map(|d| d.materials.iter().flatten().copied()).collect();
    extra.sort_unstable();
    extra.dedup();
    let found = materials::read_with(&zone, &recs.iter().map(|r| r.zone).collect::<Vec<_>>(), &extra, &mut rep, &|wanted, report| super::foreign::resolve(&src.install, oodle, wanted, report));
    out.report = rep;
    out.materials = found.materials.len();
    out.materials_without_header = found.materials.iter().filter(|m| m.header.is_none()).count();
    out.materials_without_pictures = found.materials.iter().filter(|m| m.images.is_empty()).count();
    out.materials_without_colour = found.materials.iter().filter(|m| m.colour.is_none()).count();
    for (i, img) in found.images.iter().enumerate() {
        out.pictures.push(resolve(img, found.image_usage.get(i).copied().unwrap_or(0), idx));
    }
    // The zone's other pictures.
    let images = zone.types.iter().filter(|&&t| t == 21).count();
    if let Some(run) = image::ImageRun::find(z, images) {
        let used: std::collections::HashSet<u64> = found.images.iter().map(|i| i.hash).collect();
        for &at in &run.at {
            let Ok(Some(r)) = image::parse(z, at) else { continue };
            if used.contains(&r.hash) {
                continue;
            }
            let mi = materials::map_image(z, at);
            out.others.push(resolve(&mi, r.usage, idx));
        }
    }
    Ok(out)
}

/// The package keys of every streamed mesh a map's placed models draw with
/// (each LOD of each placed model, so both the drawn LOD and the coarse one
/// the collision and the far view use), with the mesh's length. A model
/// whose mesh the zone holds has no key here.
pub fn mesh_keys(src: &MapSources, oodle: &dyn Decompressor) -> Result<Vec<(u64, usize)>, String> {
    let zone: ZoneImage = super::read_image(&src.zone(&src.name), oodle)?;
    let (list, _) = world::find_surfaces(&zone.bytes).ok_or("no surface list (gfx_map) in the map zone")?;
    let mut rep = Vec::new();
    let placed = models::read(&zone, list, &mut rep);
    let mut used: Vec<usize> = placed.instances.iter().map(|i| i.smodel).collect();
    used.sort_unstable();
    used.dedup();
    let mut keys = std::collections::BTreeMap::new();
    for sm in used {
        let Some(def) = placed.table.get(sm).copied().flatten().and_then(|e| placed.models.get(&e)) else { continue };
        for lod in &def.lods {
            let Some(&h) = placed.surfs_at.get(&lod.surfs) else { continue };
            if let Ok(rec) = crate::surfs::parse(&zone.bytes, h)
                && let Some((key, len)) = rec.streamed
                && key != 0
            {
                keys.insert(key, len);
            }
        }
    }
    Ok(keys.into_iter().collect())
}

impl MapCoverage {
    /// Counts by source.
    #[must_use]
    pub fn tally(rows: &[PictureRow]) -> BTreeMap<Source, usize> {
        let mut m = BTreeMap::new();
        for r in rows {
            *m.entry(r.source).or_default() += 1;
        }
        m
    }
}
