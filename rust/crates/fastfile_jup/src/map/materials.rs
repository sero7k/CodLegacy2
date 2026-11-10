//! A map's materials and their pictures: which material each world surface
//! draws with, which pictures a material has, and the pictures' pixels
//! from the install's packages.
//!
//! - The `gfx_map` keeps a table of material pointers (each a back-pointer
//!   to an entry of the zone's asset list, all of them materials) and a
//!   40-byte record per surface:
//!   ```text
//!   +0x00 u32 its surface record (the 128-byte list, see [`super::world`])
//!   +0x04 u32 its material, in that table
//!   +0x0e u16 the surface's world zone (as the surface record has it)
//!   +0x14 u16 0xffff
//!   ```
//! - A material (asset type 11) is a 0x78-byte header:
//!   ```text
//!   +0x00 u64 name hash
//!   +0x1b u8  pictures, +0x1c u8 constant rows (16 bytes each)
//!   +0x20, +0x28, +0x38 pointers (techniques, states: shared data, or
//!         data that follows)
//!   +0x30 pointer to its pictures: `pictures` pointers to entries of the
//!         asset list (images), following when -2/-1
//!   +0x40 pointer to its constants, following when -2/-1
//!   ```
//!   then what its pointers lead to. The headers are not walked: they are
//!   found by their shape (a hash, then counts and pointers of the forms
//!   above; most also hold 0x2c73b6c0 at +0x08), and the run of them that
//!   is exactly the zone's material count is the materials in asset order.
//!   A material whose run cannot be found that way gets no pictures.
//! - The pictures are the zone's image run ([`crate::image::ImageRun`]);
//!   a picture's pixels are the largest of its streamed mip groups the
//!   install has (`zone/*.xpak` over their `.xsub` parts), or its pixels in
//!   the zone.

use std::path::Path;

use super::index::{ZoneImage, u16_at, u32_at, u64_at};
use crate::image::{self, ImageRec, MipGroup};
use crate::oodle::Decompressor;
use crate::ptr::{FOLLOW, INSERT, MINUS3};
use crate::xsub::XPak;

/// A picture of the map: what it takes to fetch its pixels.
#[derive(Clone, Debug, Default)]
pub struct MapImage {
    pub hash: u64,
    pub width: u16,
    pub height: u16,
    /// The image's format byte ([`crate::image::dxgi`]).
    pub format: u8,
    pub groups: Vec<MipGroup>,
    /// Its pixels in the zone (a small picture), whole chain.
    pub inline: Option<Vec<u8>>,
}

/// A material: its hash and its pictures (indices into the map's images).
#[derive(Clone, Debug, Default)]
pub struct MapMaterial {
    pub hash: u64,
    pub images: Vec<usize>,
    /// The picture it is coloured by: its first of usage 0x36 (BC1 or BC7;
    /// else its first of format 0x25). Formerly its first of format 0x25 (BC1, the
    /// form colour maps take; 0x2a, also BC1, holds masks and terrain
    /// blend maps).
    pub colour: Option<usize>,
    /// The header's fixed fields, where the header was found.
    pub header: Option<MaterialHeader>,
    /// The picture of each of the material's slot pairs, in the pairs'
    /// order (the record's picture entries are very probably filled so,
    /// `docs/JUP.md`): an index into the map's images, `None` where the
    /// picture was not found.
    pub entries: Vec<Option<usize>>,
    /// Per layer, in order, the colour pairs of its constant rows that have the
    /// shape `r g b f  r g b f  1.0 1.0 1.0 1.0` (halves; `f` 0x00 or 0x0a, read
    /// as off or on): `(first, first on, second, second on)`. What each colour
    /// does is not decoded (`docs/JUP.md`).
    pub colour_rows: Vec<([u8; 3], bool, [u8; 3], bool)>,
}

/// What was found: per surface record its material (index into
/// `materials`), the materials and the images they use.
#[derive(Default)]
pub struct Found {
    pub surface_material: Vec<Option<usize>>,
    /// A material's asset-list entry -> its index in `materials`.
    pub by_entry: std::collections::HashMap<usize, usize>,
    pub materials: Vec<MapMaterial>,
    pub images: Vec<MapImage>,
    /// Each image's usage byte, by its index in `images` (0x36 colour,
    /// 0x37 packed normal, 0x32 mask, 0x33 one-channel mask, ...).
    pub image_usage: Vec<u8>,
    /// Pictures the materials name by hash only (a stub in the zone): the
    /// index of the placeholder in `images` and the hash. Filled in by
    /// [`fill_foreign`] from the zones the game loads beside the map.
    pub foreign: Vec<(usize, u64)>,
}

const MATERIAL_HEADER: usize = 0x78;

/// Where a material's own picture table starts (when its +0x30 pointer
/// says it follows): after the header and what the header's earlier
/// pointers put first. A +0x20 of -3 puts 48 bytes there; a +0x28 that
/// follows puts a byte pair per +0x18 count. (Checked on global_stream_mp:
/// 49,446 of the 49,561 following tables then hold only zone pointers,
/// against 46,000 at the header's end alone.)
#[must_use]
pub fn picture_table(z: &[u8], h: usize) -> usize {
    let mut at = h + MATERIAL_HEADER;
    if u64_at(z, h + 0x20) == MINUS3 {
        at += 48;
    }
    if matches!(u64_at(z, h + 0x28), FOLLOW | INSERT) {
        at += 2 * usize::from(z.get(h + 0x18).copied().unwrap_or(0));
    }
    at
}
/// The fixed fields of a material header, by offset. Only what is read from
/// the bytes; what each field means for the engine's choice of program is
/// not known (the zone stores no program reference, see `docs/JUP.md`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct MaterialHeader {
    pub hash: u64,
    /// `+0x08`: `0x2c73b6c0` on nearly all materials.
    pub word_08: u32,
    /// `+0x0c`: usually `0`.
    pub word_0c: u32,
    /// `+0x0e`: `0x0068`, `0x0020`, `0x0158`, `0x00c8`, `0x0021`, ... (a
    /// small set per zone: a state or sort class).
    pub class_0e: u16,
    /// `+0x12`: `8` on materials that have slot pairs, else `0`.
    pub word_12: u16,
    /// `+0x15` and `+0x16`: `02`/`00` and `83`/`82`/`03`/`02` (bit 7 and
    /// bit 0 vary).
    pub flags_15: u8,
    pub flags_16: u8,
    /// `+0x18`: slot pairs (`33` to `49`; `0` when the material has none).
    pub pairs: u8,
    /// `+0x19` (and `+0x1a`): `30`, `32` or `37` on materials with pairs.
    pub layout: u8,
    /// `+0x1b`: pictures; `+0x1c`: constant rows of 16 bytes.
    pub pictures: u8,
    pub rows: u8,
}

/// Reads the header at `h` (a place [`material_headers`] returned).
#[must_use]
pub fn header(z: &[u8], h: usize) -> MaterialHeader {
    MaterialHeader {
        hash: u64_at(z, h),
        word_08: u32_at(z, h + 0x08),
        word_0c: u32_at(z, h + 0x0c),
        class_0e: u16_at(z, h + 0x0e),
        word_12: u16_at(z, h + 0x12),
        flags_15: z[h + 0x15],
        flags_16: z[h + 0x16],
        pairs: z[h + 0x18],
        layout: z[h + 0x19],
        pictures: z[h + 0x1b],
        rows: z[h + 0x1c],
    }
}

/// A material's `(slot id, picture index)` pairs, where it has them (its
/// `+0x28` pointer follows): `pairs` byte pairs just before its picture
/// table. The picture index is into the material's picture table.
#[must_use]
pub fn slot_pairs(z: &[u8], h: usize) -> Vec<(u8, u8)> {
    let n = usize::from(z[h + 0x18]);
    if n == 0 || !matches!(u64_at(z, h + 0x28), FOLLOW | INSERT) {
        return Vec::new();
    }
    let t = picture_table(z, h);
    let Some(from) = t.checked_sub(2 * n) else { return Vec::new() };
    z.get(from..t)
        .map(|b| b.chunks_exact(2).map(|c| (c[0], c[1])).collect())
        .unwrap_or_default()
}

const SURFACE_INFO: usize = 40;

/// Whether a pointer is a back-pointer (an offset into block 0x0f).
#[must_use]
pub fn is_back(v: u64) -> bool {
    v & 1 == 1 && v >> 32 == 0x0f
}

fn pointer_like(v: u64) -> bool {
    matches!(v, 0 | FOLLOW | INSERT | MINUS3) || is_back(v) || v >> 63 == 1
}

/// The map's material table: the longest array of pointers that lead to
/// material entries of the asset list; (where it starts, its length, the
/// list base that makes the pointers so).
///
/// An element is a back-pointer to a material of the asset list, or `-2`: the
/// material's header is stored inline, right after the array (Highrise: 607
/// elements, two of them inline). The array is the run of back-pointers
/// extended over those `-2` elements and the back-pointers beyond them.
pub fn material_table(zone: &ZoneImage) -> Option<(usize, usize, usize)> {
    let z = &zone.bytes;
    let mut runs: Vec<(usize, usize)> = Vec::new();
    let mut at = 0usize;
    while at + 8 <= z.len() {
        if z[at + 4..at + 8] != [0x0f, 0, 0, 0] || z[at] & 1 == 0 {
            at += 1;
            continue;
        }
        let mut n = 0;
        while at + 8 * (n + 1) <= z.len() && is_back(u64_at(z, at + 8 * n)) {
            n += 1;
        }
        if n >= 8 {
            runs.push((at, n));
        }
        at += 8 * n.max(1);
    }
    // (Each run grown over the inline elements and the back-pointers around
    // them; the shorter run is kept as the fallback.)
    let element = |o: usize| is_back(u64_at(z, o)) || u64_at(z, o) == FOLLOW;
    let mut grown: Vec<(usize, usize, usize, usize)> = Vec::new();
    for &(a, n) in &runs {
        let (mut from, mut to) = (a, a + 8 * n);
        while from >= 8 && element(from - 8) {
            from -= 8;
        }
        while to + 8 <= z.len() && element(to) {
            to += 8;
        }
        grown.push((from, (to - from) / 8, a, n));
    }
    grown.sort_by_key(|g| std::cmp::Reverse(g.1));
    runs.sort_by_key(|r| std::cmp::Reverse(r.1));
    let lo = zone.list_at.saturating_sub(0x40);
    // The entry i's pointer is base + 16 i (+1): the base that makes every
    // pointer of the run a material's.
    let base_of = |at: usize, n: usize| -> Option<usize> {
        let offs: Vec<usize> = (0..n)
            .filter(|&k| is_back(u64_at(z, at + 8 * k)))
            .map(|k| (u64_at(z, at + 8 * k) & 0xffff_ffff) as usize - 1)
            .collect();
        (lo..zone.list_at + 0x40).find(|&base| {
            offs.iter().all(|&o| {
                o >= base && (o - base) % 16 == 0 && zone.types.get((o - base) / 16) == Some(&11)
            })
        })
    };
    grown
        .into_iter()
        .find_map(|(at, n, _, _)| base_of(at, n).map(|base| (at, n, base)))
        .or_else(|| runs.into_iter().find_map(|(at, n)| base_of(at, n).map(|base| (at, n, base))))
}

/// The 40-byte record per surface record: where the run starts.
fn surface_infos(z: &[u8], zones: &[u32], materials: usize) -> Option<usize> {
    let n = zones.len();
    let fits = |at: usize, k: usize| {
        let r = at + SURFACE_INFO * k;
        let s = u32_at(z, r) as usize;
        s < n
            && (u32_at(z, r + 4) as usize) < materials.max(0x10000)
            && u16_at(z, r + 0x14) == 0xffff
            && u32::from(u16_at(z, r + 0x0e)) == zones[s]
    };
    let last = z.len().checked_sub(SURFACE_INFO * n)?;
    (0..=last).find(|&at| (0..n.min(8)).all(|k| fits(at, k)) && (0..n).all(|k| fits(at, k)))
}

/// Does a material header stand at `h`? (Its hash, the counts' zero
/// neighbours, its pointers' forms; `magic`: its +0x08 word is the one
/// most materials have.)
fn material_at(z: &[u8], h: usize) -> Option<bool> {
    if h + MATERIAL_HEADER > z.len() || z[h + 0x17] != 0 || z[h + 0x1f] != 0 {
        return None;
    }
    let hash = u64_at(z, h);
    if hash == 0 || hash >> 63 == 1 || hash.count_ones() < 16 {
        return None;
    }
    if ![0x20, 0x28, 0x30, 0x38, 0x40]
        .iter()
        .all(|&o| pointer_like(u64_at(z, h + o)))
    {
        return None;
    }
    let magic = u32_at(z, h + 8) == 0x2c73_b6c0;
    let counts = matches!(z[h + 0x1d], 1 | 2 | 4) && matches!(z[h + 0x1e], 1 | 2 | 4);
    (magic || counts).then_some(magic)
}

/// The material headers, in asset order: the tightest run of `count`
/// header-shaped places (overlapping ones resolved toward the usual
/// +0x08 word).
pub fn material_headers(z: &[u8], count: usize) -> Option<Vec<usize>> {
    let mut found: Vec<(usize, bool)> = Vec::new();
    for h in 0..z.len().saturating_sub(MATERIAL_HEADER) {
        if let Some(magic) = material_at(z, h) {
            match found.last_mut() {
                Some(last) if h - last.0 < MATERIAL_HEADER => {
                    if magic && !last.1 {
                        *last = (h, magic);
                    }
                }
                _ => found.push((h, magic)),
            }
        }
    }
    if found.len() < count || count == 0 {
        return None;
    }
    let span = |i: usize| found[i + count - 1].0 - found[i].0;
    let i = (0..=found.len() - count).min_by_key(|&i| span(i))?;
    Some(found[i..i + count].iter().map(|f| f.0).collect())
}

/// Picture tables a material shares with an earlier one: its header holds
/// a back-pointer (a block offset) to the table that follows the earlier
/// material's header. Block offsets run a fixed shift from image places
/// over long stretches; the shifts are the ones most often found between
/// the shared tables' offsets and the places of the tables that follow
/// their headers.
pub struct SharedTables {
    /// (shift, votes), most voted first.
    shifts: Vec<(i64, usize)>,
    wanted: usize,
}

impl SharedTables {
    #[must_use]
    pub fn new(z: &[u8], headers: &[usize]) -> Self {
        let mut own: Vec<i64> = Vec::new();
        let mut back: Vec<i64> = Vec::new();
        for &h in headers {
            match u64_at(z, h + 0x30) {
                FOLLOW | INSERT => own.push(picture_table(z, h) as i64),
                v if is_back(v) => back.push((v & 0xffff_ffff) as i64 - 1),
                _ => {}
            }
        }
        own.sort_unstable();
        back.sort_unstable();
        back.dedup();
        // Votes for each shift within 64 MiB, every pair of a shared
        // offset and an own table's place.
        let mut votes: std::collections::HashMap<i64, usize> = Default::default();
        let span = 64i64 << 20;
        for &x in &back {
            let lo = own.partition_point(|&p| p < x - span);
            for &p in own[lo..].iter().take_while(|&&p| p <= x + span) {
                *votes.entry(p - x).or_default() += 1;
            }
        }
        let mut shifts: Vec<(i64, usize)> = votes.into_iter().filter(|v| v.1 >= 3).collect();
        shifts.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
        shifts.truncate(64);
        Self {
            shifts,
            wanted: back.len(),
        }
    }

    /// The picture a back-pointer to one entry of another material's
    /// table names (that entry's value): the first shift that lands on a
    /// picture.
    pub fn entry_at(&self, z: &[u8], v: u64, is_picture: &dyn Fn(u64) -> bool) -> Option<u64> {
        let x = (v & 0xffff_ffff) as i64 - 1;
        self.shifts.iter().find_map(|&(d, _)| {
            let t = usize::try_from(x + d).ok()?;
            (t + 8 <= z.len()).then(|| u64_at(z, t)).filter(|&w| is_picture(w))
        })
    }

    /// The image place of the table a back-pointer names: the first shift
    /// that lands on `pics` entries all of which are pictures.
    pub fn at(&self, z: &[u8], v: u64, pics: usize, is_picture: &dyn Fn(u64) -> bool) -> Option<usize> {
        let x = (v & 0xffff_ffff) as i64 - 1;
        self.shifts.iter().find_map(|&(d, _)| {
            let t = usize::try_from(x + d).ok()?;
            // (A picture of a shared zone's is its hash, the top bit set.)
            let fits = pics > 0
                && t + 8 * pics <= z.len()
                && (0..pics).all(|k| {
                    let p = u64_at(z, t + 8 * k);
                    is_picture(p) || p >> 63 == 1
                })
                && (0..pics).any(|k| is_picture(u64_at(z, t + 8 * k)));
            fits.then_some(t)
        })
    }
}


/// What a picture table entry is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Entry {
    /// A picture of the zone's asset list (an asset index).
    Asset(usize),
    /// A picture named by hash only: a 64-byte stub follows the table.
    Stub(u64),
    /// A picture header stored in the table's data (its place in the zone).
    Inline(usize),
    /// A pointer to an earlier entry that the stream marked `-1`.
    Cell(u64),
    Nothing,
}

/// How the materials share data.
///
/// The stream marks a pointer `-1` where a later pointer will point back to
/// the object and `-2` where nothing will (data follows either way). Every
/// `-1` object is pointed to at least once, and block offsets grow with the
/// stream, so the k-th distinct back-pointer value of a field is the k-th
/// `-1` object of that field: the distinct values and the `-1` objects are
/// as many on Highrise in each of the five header pointers (232 tables, 127
/// slot-pair lists, 40, 49, ...), and a shared table is exactly that table.
#[derive(Default)]
struct Sharing {
    /// Ordinal -> its own table's entries.
    own: std::collections::HashMap<usize, Vec<Entry>>,
    /// A `+0x30` back-pointer -> the ordinal whose table it names.
    tables: std::collections::HashMap<u64, usize>,
    /// A `+0x28` back-pointer -> the ordinal whose slot pairs it names.
    pairs: std::collections::HashMap<u64, usize>,
    /// Entry back-pointers resolved to the entry they name (how many of how many).
    cells: (usize, usize),
}

/// The k-th distinct back-pointer value of the header field at `off` -> the
/// k-th material whose field is `-1`; empty where the counts differ.
fn rank_map(z: &[u8], headers: &[usize], off: usize) -> std::collections::HashMap<u64, usize> {
    let marked: Vec<usize> = headers.iter().enumerate().filter(|(_, h)| u64_at(z, **h + off) == INSERT).map(|(i, _)| i).collect();
    let mut back: Vec<u64> = headers.iter().map(|&h| u64_at(z, h + off)).filter(|&v| is_back(v)).collect();
    back.sort_unstable();
    back.dedup();
    if back.len() != marked.len() {
        return Default::default();
    }
    back.into_iter().zip(marked).collect()
}

impl Sharing {
    fn build(z: &[u8], headers: &[usize], image_asset: &dyn Fn(u64) -> Option<usize>, is_asset: &dyn Fn(u64) -> bool, report: &mut Vec<String>) -> Self {
        let mut out = Self { tables: rank_map(z, headers, 0x30), pairs: rank_map(z, headers, 0x28), ..Default::default() };
        let mut marked_cells: Vec<(usize, usize, Entry)> = Vec::new();
        for (ordinal, &h) in headers.iter().enumerate() {
            if !matches!(u64_at(z, h + 0x30), FOLLOW | INSERT) {
                continue;
            }
            let t = picture_table(z, h);
            let pics = usize::from(z[h + 0x1b]);
            let mut at = t + 8 * pics;
            let mut entries = Vec::with_capacity(pics);
            let mut lost = false;
            for i in 0..pics {
                let v = u64_at(z, t + 8 * i);
                let e = if matches!(v, FOLLOW | INSERT) {
                    let hash = u64_at(z, at);
                    let e = if lost {
                        Entry::Nothing
                    } else if hash >> 63 == 1 && z.get(at + 8..at + 0x40).is_some_and(|b| b.iter().all(|&x| x == 0)) {
                        at += 0x40;
                        Entry::Stub(hash & 0x7fff_ffff_ffff_ffff)
                    } else if let Ok(Some(r)) = image::parse(z, at) {
                        let e = Entry::Inline(at);
                        at = r.end;
                        e
                    } else {
                        lost = true;
                        Entry::Nothing
                    };
                    if v == INSERT {
                        marked_cells.push((ordinal, i, e));
                    }
                    e
                } else if image_asset(v).is_some() {
                    Entry::Asset(image_asset(v).unwrap_or(0))
                } else if is_back(v) && !is_asset(v) {
                    Entry::Cell(v)
                } else {
                    Entry::Nothing
                };
                entries.push(e);
            }
            out.own.insert(ordinal, entries);
        }
        // Where each pointed-to entry cell is. A cell of a `-1` table is at
        // that table's place plus 8 a cell. The tables of one use (`-2`) are
        // placed between the `-1` objects whose places are known: in the
        // stream the places grow, so the cells of a gap in stream order take
        // the gap's distinct pointer values in order, where as many.
        let mut wanted: Vec<u64> = out.own.values().flatten().filter_map(|e| if let Entry::Cell(v) = e { Some(*v) } else { None }).collect();
        wanted.sort_unstable();
        wanted.dedup();
        let at_of = |v: u64| (v & 0xffff_ffff).saturating_sub(1);
        let mut by_cell: std::collections::HashMap<u64, Entry> = Default::default();
        for (&x, ordinal) in &out.tables {
            if let Some(list) = out.own.get(ordinal) {
                for (k, e) in list.iter().enumerate() {
                    by_cell.insert(x + 8 * k as u64, *e);
                }
            }
        }
        let mut anchors: Vec<(usize, u8, u64)> = Vec::new();
        for (order, map) in [(0u8, rank_map(z, headers, 0x28)), (1, out.tables.clone()), (3, rank_map(z, headers, 0x38)), (4, rank_map(z, headers, 0x40))] {
            anchors.extend(map.into_iter().map(|(v, ordinal)| (ordinal, order, at_of(v))));
        }
        anchors.sort_unstable();
        let keys: Vec<(usize, u8)> = anchors.iter().map(|a| (a.0, a.1)).collect();
        let places: Vec<u64> = anchors.iter().map(|a| a.2).collect();
        let mut cells_in_gap: std::collections::BTreeMap<usize, Vec<Entry>> = Default::default();
        let table_ordinals: std::collections::HashSet<usize> = out.tables.values().copied().collect();
        for (ordinal, _, e) in &marked_cells {
            if !table_ordinals.contains(ordinal) {
                cells_in_gap.entry(keys.partition_point(|key| *key < (*ordinal, 1))).or_default().push(*e);
            }
        }
        let mut values_in_gap: std::collections::BTreeMap<usize, Vec<u64>> = Default::default();
        for &v in wanted.iter().filter(|v| !by_cell.contains_key(v)) {
            values_in_gap.entry(places.partition_point(|&p| p < at_of(v))).or_default().push(v);
        }
        for (g, values) in &values_in_gap {
            if let Some(list) = cells_in_gap.get(g).filter(|l| l.len() == values.len()) {
                for (v, e) in values.iter().zip(list) {
                    by_cell.insert(*v, *e);
                }
            }
        }
        out.cells = (wanted.iter().filter(|v| by_cell.contains_key(v)).count(), wanted.len());
        for e in out.own.values_mut().flatten() {
            if let Entry::Cell(v) = *e {
                *e = by_cell.get(&v).copied().unwrap_or(Entry::Nothing);
            }
        }
        report.push(format!(
            "materials: shared tables {} of {}, slot-pair lists {} of {}, entry cells {} of {}",
            out.tables.len(),
            headers.iter().filter(|&&h| is_back(u64_at(z, h + 0x30))).map(|&h| u64_at(z, h + 0x30)).collect::<std::collections::HashSet<_>>().len(),
            out.pairs.len(),
            headers.iter().filter(|&&h| is_back(u64_at(z, h + 0x28))).map(|&h| u64_at(z, h + 0x28)).collect::<std::collections::HashSet<_>>().len(),
            out.cells.0,
            out.cells.1
        ));
        out
    }
}

/// The colour picture of each material: its first of usage 0x36 (BC1 or BC7;
/// usage 0x32 is a gloss picture, flat grey where a material has none; 0x37
/// the normal map), else its first BC1. Pictures not found (width 0) never.
pub fn choose_colours(found: &mut Found) {
    for mat in &mut found.materials {
        let ok = |i: usize| found.images[i].width > 0;
        mat.colour = mat
            .images
            .iter()
            .copied()
            .find(|&i| ok(i) && found.image_usage[i] == 0x36 && matches!(found.images[i].format, 0x25 | 0x30))
            .or_else(|| mat.images.iter().copied().find(|&i| ok(i) && found.images[i].format == 0x25));
    }
}

/// Puts the pictures found in other zones in the placeholders [`read`] left.
pub fn fill_foreign(found: &mut Found, resolved: &std::collections::HashMap<u64, super::foreign::Resolved>) {
    for &(idx, hash) in &found.foreign {
        if let Some(r) = resolved.get(&hash) {
            found.images[idx] = r.image.clone();
            found.image_usage[idx] = r.usage;
        }
    }
    choose_colours(found);
}

/// Reads which material every surface record draws with and the pictures
/// of those materials. `zones`: each surface record's world zone.
pub fn read(zone: &ZoneImage, zones: &[u32], extra: &[usize], report: &mut Vec<String>) -> Found {
    read_with(zone, zones, extra, report, &|_, _| Default::default())
}

/// [`read`], with `resolve` finding the pictures the materials name by hash
/// only (see [`super::foreign`]).
pub fn read_with(
    zone: &ZoneImage,
    zones: &[u32],
    extra: &[usize],
    report: &mut Vec<String>,
    resolve: &dyn Fn(&std::collections::HashSet<u64>, &mut Vec<String>) -> std::collections::HashMap<u64, super::foreign::Resolved>,
) -> Found {
    let mut out = Found {
        surface_material: vec![None; zones.len()],
        ..Default::default()
    };
    let z = &zone.bytes;
    let Some((table, run_len, base)) = material_table(zone) else {
        report.push("materials: no material table".into());
        return out;
    };
    // The table goes on past entries that are not back-pointers (an inline
    // one, an other zone's material by hash): Highrise's has an inline
    // entry at 545 and 23 back-pointers after it.
    let mut table_len = run_len;
    while table + 8 * (table_len + 1) <= z.len() {
        let v = u64_at(z, table + 8 * table_len);
        if is_back(v) || matches!(v, FOLLOW | INSERT | MINUS3) || v >> 63 == 1 {
            table_len += 1;
        } else {
            break;
        }
    }
    let Some(infos) = surface_infos(z, zones, table_len) else {
        report.push(format!(
            "materials: {table_len} in the table, no per-surface records"
        ));
        return out;
    };
    let asset_of = |v: u64| -> Option<usize> {
        if !is_back(v) {
            return None;
        }
        let o = ((v & 0xffff_ffff) as usize).checked_sub(1)?;
        let rel = o.checked_sub(base)?;
        (rel % 16 == 0 && rel / 16 < zone.types.len()).then_some(rel / 16)
    };
    // The k-th material asset and image asset, by asset index.
    let mut nth = vec![usize::MAX; zone.types.len()];
    let (mut mats, mut imgs) = (0usize, 0usize);
    for (i, &t) in zone.types.iter().enumerate() {
        if t == 11 {
            nth[i] = mats;
            mats += 1;
        } else if t == 21 {
            nth[i] = imgs;
            imgs += 1;
        }
    }
    let headers = material_headers(z, mats);
    let run = image::ImageRun::find(z, imgs);
    let shared = SharedTables::new(z, headers.as_deref().unwrap_or(&[]));
    report.push(format!(
        "materials: {} picture tables of another material's, shifts {:?}",
        shared.wanted,
        &shared.shifts[..shared.shifts.len().min(4)]
    ));
    let image_asset = |v: u64| asset_of(v).filter(|&a| zone.types[a] == 21);
    let sharing = Sharing::build(z, headers.as_deref().unwrap_or(&[]), &image_asset, &|v| asset_of(v).is_some(), report);
    let mut foreign_of: std::collections::HashMap<u64, usize> = Default::default();
    report.push(format!(
        "materials: {table_len} drawn of {mats} ({}), {imgs} images ({})",
        if headers.is_some() {
            "headers found"
        } else {
            "headers not found"
        },
        if run.is_some() {
            "run found"
        } else {
            "run not found"
        }
    ));
    // Table entry -> material ordinal -> its pictures.
    let mut by_ordinal: std::collections::HashMap<usize, usize> = Default::default();
    let mut image_of: std::collections::HashMap<usize, usize> = Default::default();
    // (Each image's usage byte, by its index in `out.images`.)
    let mut usages: Vec<u8> = Vec::new();
    let mut material_of_entry = vec![None; table_len];
    // The table's entries, then the other materials asked for (the placed
    // models'), by asset index.
    // (An element `-2` is a material stored inline after the array: the
    // k-th such element is the next header at or after where the last one
    // ended, headers standing in asset order.)
    let asset_of_ordinal: Vec<usize> = zone.types.iter().enumerate().filter(|(_, t)| **t == 11).map(|(i, _)| i).collect();
    let mut inline_after = table + 8 * table_len;
    let mut inline_count = 0usize;
    let table_assets: Vec<Option<usize>> = (0..table_len)
        .map(|e| {
            let v = u64_at(z, table + 8 * e);
            if v != FOLLOW {
                return asset_of(v);
            }
            let hs = headers.as_deref()?;
            let k = hs.partition_point(|&h| h < inline_after);
            let h = *hs.get(k)?;
            inline_after = h + 1;
            inline_count += 1;
            asset_of_ordinal.get(k).copied()
        })
        .collect();
    if inline_count > 0 {
        report.push(format!("materials: {inline_count} table elements are inline materials"));
    }
    let wanted: Vec<Option<usize>> = table_assets
        .into_iter()
        .chain(extra.iter().map(|&a| (zone.types.get(a) == Some(&11)).then_some(a)))
        .collect();
    for (e, &asset) in wanted.iter().enumerate() {
        let Some(ordinal) = asset.map(|a| nth[a]) else {
            continue;
        };
        if let Some(&m) = by_ordinal.get(&ordinal) {
            if let Some(slot) = material_of_entry.get_mut(e) {
                *slot = Some(m);
            }
            if let Some(a) = asset {
                out.by_entry.insert(a, m);
            }
            continue;
        }
        let mut mat = MapMaterial::default();
        if let Some(h) = headers.as_ref().and_then(|hs| hs.get(ordinal).copied()) {
            mat.hash = u64_at(z, h);
            mat.header = Some(header(z, h));
            // (Its constant rows follow its picture table and what that table
            // puts after it; scanned for the colour rows' shape up to the next
            // header.)
            let end = headers.as_ref().and_then(|hs| hs.get(ordinal + 1).copied()).filter(|&e| e > h).unwrap_or(h + 0x800).min(h + 0x2000).min(z.len());
            let mut at = h + MATERIAL_HEADER;
            while at + 16 <= end {
                let r = &z[at..at + 16];
                if r[8..16] == [0x00, 0x3c, 0x00, 0x3c, 0x00, 0x3c, 0x00, 0x3c] && matches!(r[3], 0x00 | 0x0a) && matches!(r[7], 0x00 | 0x0a) {
                    mat.colour_rows.push(([r[0], r[1], r[2]], r[3] == 0x0a, [r[4], r[5], r[6]], r[7] == 0x0a));
                    at += 16;
                } else {
                    at += 1;
                }
            }
            // (Its slot pairs: its own, or the list a back-pointer names.)
            let pairs = match u64_at(z, h + 0x28) {
                FOLLOW | INSERT => slot_pairs(z, h),
                v if is_back(v) => sharing.pairs.get(&v).and_then(|&o| headers.as_ref().and_then(|hs| hs.get(o))).map(|&ha| slot_pairs(z, ha)).unwrap_or_default(),
                _ => Vec::new(),
            };
            let mut table_images: Vec<Option<usize>> = Vec::new();
            let pics = usize::from(z[h + 0x1b]);
            // Its picture table: following the header, or another
            // material's (a back-pointer: the k-th shared one is the k-th
            // `-1` table; where the counts did not agree, found by the
            // shift a nearby table's block offset has to its place).
            let entries: Vec<Entry> = match u64_at(z, h + 0x30) {
                FOLLOW | INSERT => sharing.own.get(&ordinal).cloned().unwrap_or_default(),
                v if is_back(v) => match sharing.tables.get(&v).and_then(|o| sharing.own.get(o)) {
                    Some(list) => list.clone(),
                    None => shared
                        .at(z, v, pics, &|p| asset_of(p).is_some_and(|a| zone.types[a] == 21))
                        .map(|t| {
                            (0..pics)
                                .map(|k| {
                                    let v = u64_at(z, t + 8 * k);
                                    match image_asset(v) {
                                        Some(a) => Entry::Asset(a),
                                        None if is_back(v) => shared
                                            .entry_at(z, v, &|w| image_asset(w).is_some())
                                            .and_then(image_asset)
                                            .map_or(Entry::Nothing, Entry::Asset),
                                        None => Entry::Nothing,
                                    }
                                })
                                .collect()
                        })
                        .unwrap_or_default(),
                },
                _ => Vec::new(),
            };
            for entry in entries {
                let idx = match entry {
                    Entry::Asset(a) => run.as_ref().and_then(|r| r.at.get(nth[a]).copied()).map(|at| {
                        *image_of.entry(at).or_insert_with(|| {
                            out.images.push(map_image(z, at));
                            usages.push(image::parse(z, at).ok().flatten().map_or(0, |r| r.usage));
                            out.images.len() - 1
                        })
                    }),
                    Entry::Inline(at) => Some(*image_of.entry(at).or_insert_with(|| {
                        out.images.push(map_image(z, at));
                        usages.push(image::parse(z, at).ok().flatten().map_or(0, |r| r.usage));
                        out.images.len() - 1
                    })),
                    Entry::Stub(hash) => Some(*foreign_of.entry(hash).or_insert_with(|| {
                        out.images.push(MapImage { hash, ..Default::default() });
                        usages.push(0);
                        out.foreign.push((out.images.len() - 1, hash));
                        out.images.len() - 1
                    })),
                    Entry::Cell(_) | Entry::Nothing => None,
                };
                if let Some(i) = idx {
                    mat.images.push(i);
                }
                table_images.push(idx);
            }
            mat.entries = pairs.iter().map(|&(_, pic)| table_images.get(usize::from(pic)).copied().flatten()).collect();
        }
        out.materials.push(mat);
        let m = out.materials.len() - 1;
        by_ordinal.insert(ordinal, m);
        if let Some(slot) = material_of_entry.get_mut(e) {
            *slot = Some(m);
        }
        if let Some(a) = asset {
            out.by_entry.insert(a, m);
        }
    }
    for k in 0..zones.len() {
        let r = infos + SURFACE_INFO * k;
        let s = u32_at(z, r) as usize;
        let e = u32_at(z, r + 4) as usize;
        if let Some(m) = material_of_entry.get(e).copied().flatten()
            && let Some(slot) = out.surface_material.get_mut(s)
        {
            *slot = Some(m);
        }
    }
    out.image_usage = usages.clone();
    choose_colours(&mut out);
    if !out.foreign.is_empty() {
        let wanted: std::collections::HashSet<u64> = out.foreign.iter().map(|f| f.1).collect();
        let resolved = resolve(&wanted, report);
        fill_foreign(&mut out, &resolved);
    }
    let coloured = out.materials.iter().filter(|m| m.colour.is_some()).count();
    let mut formats: std::collections::BTreeMap<u8, usize> = Default::default();
    for i in &out.images {
        *formats.entry(i.format).or_default() += 1;
    }
    report.push(format!(
        "materials: {} used, {coloured} with a colour picture, {} pictures (formats {formats:x?})",
        out.materials.len(),
        out.images.len()
    ));
    out
}

pub(crate) fn map_image(z: &[u8], at: usize) -> MapImage {
    match image::parse(z, at) {
        Ok(Some(ImageRec {
            hash,
            width,
            height,
            format,
            groups,
            inline,
            ..
        })) => MapImage {
            hash,
            width,
            height,
            format,
            groups,
            inline: inline.map(|(o, n)| z[o..o + n].to_vec()),
        },
        _ => MapImage::default(),
    }
}

/// The sky's pictures: the zone's half-float (format 0x2d, BC6H) pictures
/// of the sky's use (0x3a) twice as wide as high, each two hemispheres
/// side by side (see `assets::lane::jup_sky`). Rebirth Island has three
/// (a clear sky with the sun, a sky beside the ground, an overcast one).
pub fn sky_images(zone: &ZoneImage, run: &image::ImageRun) -> Vec<MapImage> {
    let z = &zone.bytes;
    let skies = |two_to_one: bool| -> Vec<MapImage> {
        run.at
            .iter()
            .filter_map(|&at| image::parse(z, at).ok().flatten().map(|r| (at, r)))
            .filter(|(_, r)| r.format == 0x2d && r.usage == 0x3a && (r.width == 2 * r.height) == two_to_one && (two_to_one || r.width == r.height) && r.height >= 256)
            .map(|(at, _)| map_image(z, at))
            .collect()
    };
    // Farm 18 and Shipment keep one square picture a sky (the dome alone); a
    // map with two-to-one ones (Afghan has 160 more square 0x3a pictures that
    // are not skies) uses only those.
    let wide = skies(true);
    if wide.is_empty() { skies(false) } else { wide }
}

/// The map's colour grade: a 32 x 32 x 32 volume of four 16-bit unorm
/// channels (format 0xc, usage 0x11) the zone carries inline. Axes x, y, z
/// are red, green and blue of the lookup; it is indexed by the scene colour
/// in the log encoding `(log2(x + 1/1024) + 10) / 20` the game's display
/// program uses, and stays within 2 per cent of the identity on Rust.
#[derive(Clone, Debug, Default)]
pub struct ColourGrade {
    pub hash: u64,
    /// 32^3 texels of r, g, b, a, x fastest.
    pub rgba: Vec<u16>,
}

/// The grade the map zone carries (the first such volume), if any.
pub fn colour_grade(zone: &ZoneImage, run: &image::ImageRun) -> Option<ColourGrade> {
    let z = &zone.bytes;
    for &at in &run.at {
        let Ok(Some(r)) = image::parse(z, at) else { continue };
        if (r.width, r.height, r.depth) != (32, 32, 32) || r.format != 0xc || r.usage != 0x11 {
            continue;
        }
        let Some((o, n)) = r.inline else { continue };
        let bytes = z.get(o..o + n)?;
        if n != 32 * 32 * 32 * 8 {
            continue;
        }
        let rgba = bytes.chunks_exact(2).map(|b| u16::from_le_bytes([b[0], b[1]])).collect();
        return Some(ColourGrade { hash: r.hash, rgba });
    }
    None
}

/// The install's streamed packages, to fetch pictures' pixels from.
pub struct Packages {
    paks: Vec<XPak>,
    /// Entries cut out of the install's parts into one file
    /// (`$JUP_PACK_DIR`: `pack.bin` and `index.tsv` of key, size, offset),
    /// resolved through the zone indexes. For machines that have the
    /// zones and indexes but not the parts.
    extracted: Option<Extracted>,
}

struct Extracted {
    index: super::coverage::PackIndex,
    /// Key -> (which pack, offset in it, size).
    rows: std::collections::HashMap<u64, (usize, u64, usize)>,
    packs: Vec<std::path::PathBuf>,
}

impl Extracted {
    fn load(install: &Path) -> Option<Self> {
        // (`$JUP_PACK_DIR` may name several folders, separated as the platform
        // separates a path list: a later extraction adds to an earlier one.)
        let dirs: Vec<std::path::PathBuf> = std::env::split_paths(&std::env::var_os("JUP_PACK_DIR").filter(|v| !v.is_empty())?).filter(|d| !d.as_os_str().is_empty()).collect();
        let mut rows = std::collections::HashMap::new();
        let mut packs = Vec::new();
        let mut listing_from = Vec::new();
        for dir in &dirs {
            let Ok(text) = std::fs::read_to_string(dir.join("index.tsv")) else { continue };
            listing_from.push(dir.join("file_list.txt"));
            // (A fourth column names the file of the folder the bytes are in,
            // e.g. `pack-001.bin`: an extraction split into files the staging
            // tools can carry. Without it, `pack.bin`.)
            let mut files: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
            for l in text.lines() {
                let mut f = l.split('\t');
                let (Some(key), Some(size), Some(off)) = (f.next(), f.next(), f.next()) else { continue };
                let file = f.next().map(str::trim).filter(|s| !s.is_empty() && !s.contains(['/', '\\'])).unwrap_or("pack.bin");
                let n = *files.entry(file.to_owned()).or_insert_with(|| {
                    packs.push(dir.join(file));
                    packs.len() - 1
                });
                if let (Ok(key), Ok(size), Ok(off)) = (u64::from_str_radix(key.trim(), 16), size.trim().parse(), off.trim().parse()) {
                    rows.entry(key).or_insert((n, off, size));
                }
            }
        }
        if packs.is_empty() {
            return None;
        }
        let listing = [install.join("file_list.txt"), install.join("zone").join("file_list.txt")]
            .into_iter()
            .chain(listing_from)
            .find_map(|p| std::fs::read_to_string(p).ok())
            .map(|t| super::coverage::read_listing(&t))
            .unwrap_or_default();
        Some(Self { index: super::coverage::PackIndex::open(install, &listing), rows, packs })
    }

    fn fetch(&self, key: u64, oodle: &dyn Decompressor) -> Option<Vec<u8>> {
        use std::io::{Read, Seek, SeekFrom};
        let &(pack, off, size) = self.rows.get(&key)?;
        let l = self.index.locate(key).filter(|l| !l.cdn)?;
        if l.size as usize != size {
            return None;
        }
        let mut f = std::fs::File::open(self.packs.get(pack)?).ok()?;
        f.seek(SeekFrom::Start(off)).ok()?;
        let mut blob = vec![0u8; size];
        f.read_exact(&mut blob).ok()?;
        let entry = crate::xsub::XSubEntry { key, offset: l.offset, size: l.size, decompressed: l.decompressed, compressed: l.compressed };
        crate::xsub::decode_entry(&entry, &blob, oodle).ok()
    }
}

/// One level of a picture: DXGI format (UNORM), size, the blocks.
pub struct Level {
    pub dxgi: u32,
    pub width: u32,
    pub height: u32,
    pub data: Vec<u8>,
}

impl Packages {
    /// [`Self::open`], once a run per install: the map, the guest guns and
    /// the Warzone HUD's pictures all read the same packages' indexes.
    pub fn shared(install: &Path) -> std::sync::Arc<Self> {
        use std::collections::HashMap;
        use std::path::PathBuf;
        use std::sync::{Arc, Mutex, OnceLock};
        type Opened = Mutex<HashMap<PathBuf, Arc<OnceLock<Arc<Packages>>>>>;
        static OPENED: OnceLock<Opened> = OnceLock::new();
        // (One slot per install, filled once: two asking at once wait for
        // the one opening them.)
        let slot = OPENED
            .get_or_init(Default::default)
            .lock()
            .map(|mut m| m.entry(install.to_path_buf()).or_default().clone())
            .unwrap_or_default();
        slot.get_or_init(|| Arc::new(Self::open(install))).clone()
    }

    /// Every zone `.xpak` that has local parts.
    pub fn open(install: &Path) -> Self {
        let mut paks = Vec::new();
        if let Ok(dir) = std::fs::read_dir(install.join("zone")) {
            let mut files: Vec<_> = dir
                .flatten()
                .map(|e| e.path())
                .filter(|p| {
                    p.extension()
                        .is_some_and(|e| e.eq_ignore_ascii_case("xpak"))
                })
                .collect();
            files.sort();
            for f in files {
                if let Ok(p) = XPak::open(&f)
                    && p.stride > 0
                {
                    paks.push(p);
                }
            }
        }
        Self { paks, extracted: Extracted::load(install) }
    }

    pub fn len(&self) -> usize {
        self.paks.len()
    }

    pub fn is_empty(&self) -> bool {
        self.paks.is_empty()
    }

    /// Whether a package of the install has the entry.
    pub fn has(&self, key: u64) -> bool {
        self.paks.iter().any(|p| p.find(key).is_some()) || self.extracted.as_ref().is_some_and(|e| e.rows.contains_key(&key))
    }

    /// An entry's bytes (a picture's mip group, a model's mesh).
    pub fn fetch(&self, key: u64, oodle: &dyn Decompressor) -> Option<Vec<u8>> {
        self.paks
            .iter()
            .find_map(|p| p.find(key).map(|e| (p, *e)))
            .and_then(|(p, e)| p.read(&e, oodle).ok())
            .or_else(|| self.extracted.as_ref().and_then(|e| e.fetch(key, oodle)))
    }

    /// The picture's largest level at most `max_dim` wide and high that the
    /// install has.
    pub fn level(
        &self,
        img: &MapImage,
        max_dim: u32,
        oodle: &dyn Decompressor,
    ) -> Result<Level, String> {
        let dxgi = image::dxgi(img.format).ok_or_else(|| format!("format {:#x}", img.format))?;
        let rec = ImageRec {
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
        // bo2mp: a small copy keeps every level a reader of this picture may
        // ask for (up to `max_dim`: a lower quality asks for less), not only
        // the one read now (`crate::pack`).
        if crate::pack::recording() {
            for (g, group) in img.groups.iter().enumerate() {
                if image::group_levels(&rec, g, dxgi).is_some_and(|ls| ls.iter().any(|l| l.1.max(l.2) <= max_dim)) {
                    crate::pack::note_key(group.key);
                }
            }
        }
        for g in (0..img.groups.len()).rev() {
            let Some(levels) = image::group_levels(&rec, g, dxgi) else {
                continue;
            };
            let Some(&(_, w, h, off)) = levels.iter().find(|l| l.1.max(l.2) <= max_dim) else {
                continue;
            };
            let Some(bytes) = self.fetch(img.groups[g].key, oodle) else {
                continue;
            };
            let n = image::level_bytes(dxgi, w, h).unwrap_or(0);
            if let Some(data) = bytes.get(off..off + n) {
                return Ok(Level {
                    dxgi,
                    width: w,
                    height: h,
                    data: data.to_vec(),
                });
            }
        }
        if let Some(px) = &img.inline {
            // The whole chain in the zone: its largest level that fits.
            let (mut w, mut h) = (u32::from(img.width), u32::from(img.height));
            let mut off = 0usize;
            while w.max(h) > max_dim && w > 4 && h > 4 {
                off += image::level_bytes(dxgi, w, h).unwrap_or(0);
                (w, h) = (w / 2, h / 2);
            }
            let n = image::level_bytes(dxgi, w, h).unwrap_or(0);
            if let Some(data) = px.get(off..off + n) {
                return Ok(Level {
                    dxgi,
                    width: w,
                    height: h,
                    data: data.to_vec(),
                });
            }
        }
        Err(format!("{:016x}: no group of it in the install", img.hash))
    }
}

#[cfg(test)]
mod header_tests {
    use super::*;

    #[test]
    fn header_fields_and_slot_pairs() {
        // A header at 0: +0x08 the usual word, +0x0e 0x68, +0x12 8, +0x15/+0x16
        // 2/0x83, three pairs, layout 30, two pictures, three rows; the
        // +0x28 pointer follows, so the pairs sit just before the picture
        // table at 0x78 + 6.
        let mut z = vec![0u8; 0x78 + 6 + 16];
        z[0..8].copy_from_slice(&0x1234_5678_9abc_def1u64.to_le_bytes());
        z[8..12].copy_from_slice(&0x2c73_b6c0u32.to_le_bytes());
        z[0x0e] = 0x68;
        z[0x12] = 8;
        z[0x15] = 2;
        z[0x16] = 0x83;
        z[0x18] = 3;
        z[0x19] = 30;
        z[0x1b] = 2;
        z[0x1c] = 3;
        z[0x28..0x30].copy_from_slice(&FOLLOW.to_le_bytes());
        z[0x78..0x7e].copy_from_slice(&[0, 0, 4, 1, 0x32, 1]);
        let h = header(&z, 0);
        assert_eq!((h.word_08, h.class_0e, h.word_12, h.flags_15, h.flags_16), (0x2c73_b6c0, 0x68, 8, 2, 0x83));
        assert_eq!((h.pairs, h.layout, h.pictures, h.rows), (3, 30, 2, 3));
        assert_eq!(slot_pairs(&z, 0), vec![(0, 0), (4, 1), (0x32, 1)]);
    }
}
