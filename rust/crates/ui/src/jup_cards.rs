//! bo2mp: Modern Warfare III's (2023) emblems and calling cards, read from
//! the owner's install at runtime (`fastfile_jup::cards`): the names from
//! its `global` zone, a picture from the package entry its image names,
//! decoded from BC1/BC7 when it is first shown. Nothing of the game is
//! copied or kept on disk.
//!
//! The shell names one `jup:<name>` (`jup:ui_emblem_005`,
//! `jup:jup_playercard_animated_opc_..._atlas`): a still picture, or a
//! flipbook's fullest frame (some fade in from blank ones); its frames are
//! `jup:<name>#<frame>`, from `#0`. Their titles are
//! encrypted in the game's files and are not read: the names stand in.
//!
//! The install is `$BO2_JUP_INSTALL`, else the `jup` line of the game
//! folders file. Without it (or off Windows, where the game's Oodle DLL
//! cannot load) the lists are empty.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, OnceLock};

use fastfile_jup::cards::{self as jc, Item, Kind, Packages, Rgba};
use fastfile_jup::oodle::Decompressor;

/// The shell's prefix for these.
pub(crate) const PREFIX: &str = "jup";
/// Where the install is, when not in the game folders file.
pub(crate) const INSTALL_ENV: &str = "BO2_JUP_INSTALL";

/// The largest side a picture is kept at: an emblem's frame 256 square, a
/// calling card's 512x128.
const EMBLEM_SIDE: u32 = 256;
const CARD_SIZE: (u32, u32) = (512, 128);
/// Flipbooks decoded and kept (each up to 16 frames).
const KEEP_BOOKS: usize = 12;

/// Every one there is, read once.
struct Catalog {
    install: PathBuf,
    items: HashMap<String, Item>,
    cards: Vec<String>,
    emblems: Vec<String>,
    oodle: Box<dyn Decompressor>,
    packages: Mutex<Packages>,
}

/// The install folder, when it has the zone the names are in, or is a small
/// copy holding them (`jup_pack`: `looks/looks.txt`).
pub(crate) fn install() -> Option<PathBuf> {
    let set = std::env::var_os(INSTALL_ENV).filter(|p| !p.is_empty()).map(PathBuf::from).or_else(|| asset_transport::game_paths::get(PREFIX));
    set.filter(|set| set.join("zone").join(format!("{}.ff", jc::ZONE)).is_file() || set.join(LOOKS_INDEX).is_file())
        // (Without the game: the folder its guns, cards and emblems were
        // exported to, `BO2_JUP_EXPORT`.)
        .or_else(|| asset_transport::jup_export::dir().filter(|d| d.join(LOOKS_INDEX).is_file()))
}

/// bo2mp: a small copy's calling cards and emblems (`jup_pack`): each
/// `looks/<name>.png`, its frames one under another, and the list
/// (`<card|emblem>\t<name>\t<frames>\t<poster>` a line).
pub(crate) const LOOKS_DIR: &str = "looks";
pub(crate) const LOOKS_INDEX: &str = "looks/looks.txt";

struct PackLooks {
    dir: PathBuf,
    cards: Vec<String>,
    emblems: Vec<String>,
    /// Frames and the still one, by name.
    frames: HashMap<String, (u32, usize)>,
    /// An export's pictures, in packs (`CodLegacy export-jup`): else each is its
    /// own `looks/<name>.png`.
    blobs: Option<asset_transport::jup_export::PackIndex>,
}

/// The small copy's looks, when the install is one (no `global` zone).
fn pack() -> Option<&'static PackLooks> {
    static PACK: OnceLock<Option<PackLooks>> = OnceLock::new();
    PACK.get_or_init(|| {
        let set = install()?;
        if set.join("zone").join(format!("{}.ff", jc::ZONE)).is_file() {
            return None;
        }
        let text = std::fs::read_to_string(set.join(LOOKS_INDEX)).ok()?;
        let dir = set.join(LOOKS_DIR);
        let blobs = asset_transport::jup_export::PackIndex::open(&dir, LOOKS_DIR);
        let mut p = PackLooks { dir, cards: Vec::new(), emblems: Vec::new(), frames: HashMap::new(), blobs };
        for line in text.lines() {
            let f: Vec<&str> = line.split('\t').collect();
            let [kind, name, n, poster] = f[..] else { continue };
            let shell = format!("{PREFIX}:{name}");
            match kind {
                "card" => p.cards.push(shell),
                "emblem" => p.emblems.push(shell),
                _ => continue,
            }
            p.frames.insert(name.to_owned(), (n.parse().unwrap_or(1).max(1), poster.parse().unwrap_or(0)));
        }
        diag::info!(Ui, "bo2mp: Modern Warfare III: {} emblems and {} calling cards from the small copy {}", p.emblems.len(), p.cards.len(), set.display());
        Some(p)
    })
    .as_ref()
}

/// A look of the small copy, its frames split from its picture.
fn decode_pack(p: &PackLooks, name: &str) -> Option<Arc<Book>> {
    let &(n, poster) = p.frames.get(name)?;
    let img = match &p.blobs {
        Some(blobs) => image::load_from_memory_with_format(&blobs.get(name)?, image::ImageFormat::Png).ok()?.into_rgba8(),
        None => image::open(p.dir.join(format!("{name}.png"))).ok()?.into_rgba8(),
    };
    let (w, h) = (img.width(), img.height() / n.max(1));
    let row = (w * 4) as usize;
    let all = img.into_raw();
    let frames: Vec<Rgba> = (0..n as usize)
        .map(|i| Rgba { width: w, height: h, pixels: all[i * h as usize * row..(i + 1) * h as usize * row].to_vec() })
        .collect();
    Some(Arc::new(Book { frames, poster: poster.min(n as usize - 1) }))
}

/// bo2mp: writes every calling card and emblem of the install, decoded at
/// the sizes they are shown at, into `dest`'s `looks` folder (see
/// [`LOOKS_INDEX`]). `say` hears how far it is. Returns how many and the
/// bytes written.
pub(crate) fn export_looks(dest: &std::path::Path, say: &mut dyn FnMut(&str)) -> Result<(usize, u64), String> {
    let cat = catalog().ok_or("Modern Warfare III's calling cards are not readable here (no install, or no Oodle)")?;
    let dir = dest.join(LOOKS_DIR);
    std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let mut index = String::new();
    let (mut n, mut bytes) = (0usize, 0u64);
    let all: Vec<(&str, &String)> = cat.cards.iter().map(|c| ("card", c)).chain(cat.emblems.iter().map(|e| ("emblem", e))).collect();
    for (k, (kind, shell)) in all.iter().enumerate() {
        if k % 100 == 0 {
            say(&format!("calling cards and emblems: {k} of {}", all.len()));
        }
        let name = shell.split_once(':').map_or(shell.as_str(), |(_, n)| n);
        let Some(book) = book(name) else { continue };
        let Some(first) = book.frames.first() else { continue };
        let (w, h) = (first.width, first.height);
        let mut px = Vec::with_capacity((w * h * 4) as usize * book.frames.len());
        for f in &book.frames {
            if f.width != w || f.height != h {
                continue;
            }
            px.extend_from_slice(&f.pixels);
        }
        let frames = px.len() / (w * h * 4) as usize;
        let Some(img) = image::RgbaImage::from_raw(w, h * frames as u32, px) else { continue };
        let path = dir.join(format!("{name}.png"));
        img.save(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        bytes += std::fs::metadata(&path).map_or(0, |m| m.len());
        index.push_str(&format!("{kind}\t{name}\t{frames}\t{}\n", book.poster.min(frames.saturating_sub(1))));
        n += 1;
    }
    std::fs::write(dest.join(LOOKS_INDEX), index).map_err(|e| format!("{LOOKS_INDEX}: {e}"))?;
    Ok((n, bytes))
}

fn catalog() -> Option<&'static Catalog> {
    static CAT: OnceLock<Option<Catalog>> = OnceLock::new();
    CAT.get_or_init(|| {
        let install = install()?;
        // (A small copy has no zone of names: `pack`.)
        if !install.join("zone").join(format!("{}.ff", jc::ZONE)).is_file() {
            return None;
        }
        let oodle = match fastfile_jup::oodle::load(&install) {
            Ok(o) => o,
            Err(e) => {
                diag::warn!(Ui, "bo2mp: Modern Warfare III emblems and calling cards: no Oodle ({e})");
                return None;
            }
        };
        let t = std::time::Instant::now();
        let zone_dir = install.join("zone");
        let zone = match fastfile_jup::read_zone(&zone_dir.join(format!("{}.ff", jc::ZONE)), oodle.as_ref()) {
            Ok((_, z)) => z,
            Err(e) => {
                diag::warn!(Ui, "bo2mp: Modern Warfare III's {}.ff not read: {e}", jc::ZONE);
                return None;
            }
        };
        // (Every picture is in a package: none is kept in the zone, which
        // is dropped once its names are read.)
        // (Only those whose pixels the install has: a few are fetched by
        // the game on demand.)
        let mut packages = Packages::new(&zone_dir, false);
        let found: Vec<Item> = jc::find_items(&zone)
            .into_iter()
            .filter(|i| i.image.inline.is_none() && fastfile_jup::image::dxgi(i.image.format).is_some())
            .filter(|i| i.image.groups.last().is_some_and(|g| packages.find(g.key).is_some()))
            .collect();
        let (mut cards, mut emblems) = (Vec::new(), Vec::new());
        for it in &found {
            let name = format!("{PREFIX}:{}", it.name);
            match it.kind {
                Kind::Card => cards.push(name),
                Kind::Emblem => emblems.push(name),
            }
        }
        diag::info!(Ui, "bo2mp: Modern Warfare III: {} emblems and {} calling cards ({:?})", emblems.len(), cards.len(), t.elapsed());
        Some(Catalog {
            items: found.into_iter().map(|i| (i.name.clone(), i)).collect(),
            cards,
            emblems,
            oodle,
            packages: Mutex::new(packages),
            install,
        })
    })
    .as_ref()
}

/// (Calling cards, emblems) as `jup:<name>`, in the game's order; empty
/// without the install. Slow the first time (the zone is read): call it
/// off the main thread.
pub(crate) fn names() -> (Vec<String>, Vec<String>) {
    if let Some(p) = pack() {
        return (p.cards.clone(), p.emblems.clone());
    }
    catalog().map_or_else(Default::default, |c| (c.cards.clone(), c.emblems.clone()))
}

/// Whether a shell name is one of these (`jup:` and an emblem's or card's
/// name, any frame).
pub(crate) fn is_look(material: &str) -> bool {
    material.split_once(':').is_some_and(|(g, n)| g == PREFIX && jc::kind_of(split_frame(n).0).is_some())
}

/// `name#3` -> (`name`, Some(3)); `name` -> (`name`, None).
fn split_frame(name: &str) -> (&str, Option<u32>) {
    match name.rsplit_once('#') {
        Some((n, f)) => (n, f.parse().ok()),
        None => (name, None),
    }
}

/// A frame's shell name.
pub(crate) fn frame_name(name: &str, frame: u32) -> String {
    format!("{name}#{frame}")
}

/// A picture's frames (one for a still one), decoded and shrunk, and the
/// one shown when it stands still.
struct Book {
    frames: Vec<Rgba>,
    poster: usize,
}

/// The decoded books by name, the last used last; and how many frames each
/// name has once it was decoded (kept for good).
#[derive(Default)]
struct Books {
    kept: Vec<(String, Arc<Book>)>,
    counts: HashMap<String, u32>,
}

static BOOKS: Mutex<Option<Books>> = Mutex::new(None);

/// How many frames a name's picture has: 1 for a still one, and until its
/// picture is first read.
pub(crate) fn frames(material: &str) -> u32 {
    let Some((_, n)) = material.split_once(':') else { return 1 };
    BOOKS.lock().ok().and_then(|b| b.as_ref().and_then(|b| b.counts.get(split_frame(n).0).copied())).unwrap_or(1)
}

/// The kept book of a name (now the last used).
fn kept(name: &str) -> Option<Arc<Book>> {
    let mut b = BOOKS.lock().ok()?;
    let b = b.get_or_insert_with(Books::default);
    let i = b.kept.iter().position(|(n, _)| n == name)?;
    let hit = b.kept.remove(i);
    let book = hit.1.clone();
    b.kept.push(hit);
    Some(book)
}

/// A name's book: kept, else decoded (one at a time, so a flipbook's
/// frames asked for together decode it once, and the reads stay light).
fn book(name: &str) -> Option<Arc<Book>> {
    static DECODING: Mutex<()> = Mutex::new(());
    if let Some(b) = kept(name) {
        return Some(b);
    }
    let _one = DECODING.lock().ok()?;
    if let Some(b) = kept(name) {
        return Some(b);
    }
    decode(name)
}

fn decode(name: &str) -> Option<Arc<Book>> {
    if let Some(p) = pack() {
        let book = decode_pack(p, name)?;
        if let Ok(mut b) = BOOKS.lock() {
            let b = b.get_or_insert_with(Books::default);
            b.counts.insert(name.to_owned(), book.frames.len() as u32);
            b.kept.push((name.to_owned(), book.clone()));
            if b.kept.len() > KEEP_BOOKS {
                b.kept.remove(0);
            }
        }
        return Some(book);
    }
    let cat = catalog()?;
    let item = cat.items.get(name)?;
    let max = match item.kind {
        Kind::Emblem => EMBLEM_SIDE,
        Kind::Card => CARD_SIZE.0,
    };
    let mut fetch = |key: u64| {
        // (The row is found under the lock; the read and decode are not.)
        let at = cat.packages.lock().ok()?.locate(key);
        at.and_then(|(part, entry)| jc::read_located(&part, &entry, cat.oodle.as_ref()))
    };
    let Some(pic) = jc::picture(None, item, &mut fetch, max) else {
        diag::warn!(Ui, "bo2mp: Modern Warfare III's `{name}`: its picture is not in the install's packages ({})", cat.install.display());
        return None;
    };
    let grid = jc::detect_grid(item, &pic);
    let n = grid.0 * grid.1;
    let (w, h) = match item.kind {
        Kind::Emblem => (EMBLEM_SIDE, EMBLEM_SIDE),
        Kind::Card => CARD_SIZE,
    };
    let frames: Vec<Rgba> = (0..n).map(|i| jc::shrink(&jc::frame(&pic, grid, i), w, h)).collect();
    let book = Arc::new(Book { frames, poster: jc::poster_frame(&pic, grid) as usize });
    if let Ok(mut b) = BOOKS.lock() {
        let b = b.get_or_insert_with(Books::default);
        b.counts.insert(name.to_owned(), n);
        b.kept.push((name.to_owned(), book.clone()));
        if b.kept.len() > KEEP_BOOKS {
            b.kept.remove(0);
        }
    }
    Some(book)
}

/// A picture for the shell: the first of `names` (`<name>` or
/// `<name>#<frame>`, no prefix) it has, as RGBA. Reads the install: call
/// it off the main thread.
pub(crate) fn read_picture(names: &[String]) -> Option<(u32, u32, Vec<u8>)> {
    names.iter().find_map(|n| {
        let (name, f) = split_frame(n);
        let book = book(name)?;
        let frame = book.frames.get(f.map_or(book.poster, |f| f as usize % book.frames.len().max(1)))?;
        Some((frame.width, frame.height, frame.pixels.clone()))
    })
}

/// A name as he reads it: the words of its name without the game's
/// bookkeeping, its joined words parted
/// (`jup_emblem_animated_opc_greekmythhades_01_atlas` -> `Greek Myth Hades
/// 01`, `ui_emblem_005` -> `Emblem 005`,
/// `ui_playercard_100` -> `Calling Card 100`).
pub(crate) fn title(name: &str) -> String {
    let name = name.split_once(':').map_or(name, |(_, n)| n);
    let name = split_frame(name).0;
    let card = name.contains("playercard");
    let words: Vec<&str> = name
        .split('_')
        .filter(|w| !w.is_empty() && !matches!(*w, "ui" | "jup" | "wzm" | "emblem" | "playercard" | "animated" | "opc" | "atlas" | "altas"))
        .collect();
    let numbered = words.first().is_some_and(|w| w.chars().all(|c| c.is_ascii_digit()));
    let mut out: Vec<String> = Vec::new();
    if words.is_empty() || numbered {
        out.push(if card { "Calling Card".to_owned() } else { "Emblem".to_owned() });
    }
    // (Each a run of words with no spaces: parted, `rgbwarrior` -> `RGB
    // Warrior`.)
    for w in words {
        out.push(crate::word_split::title_token(w));
    }
    out.join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A small copy's look: its frames, one under another in its picture,
    /// come back apart, the still one as listed.
    #[test]
    fn a_saved_look_reads_back() {
        let dir = std::env::temp_dir().join(format!("jup_looks_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let (w, h, n) = (8u32, 2u32, 3u32);
        let px: Vec<u8> = (0..n).flat_map(|f| std::iter::repeat_n([f as u8 * 50, 0, 0, 255], (w * h) as usize).flatten()).collect();
        image::RgbaImage::from_raw(w, h * n, px).unwrap().save(dir.join("ui_emblem_x.png")).unwrap();
        let p = PackLooks { dir: dir.clone(), cards: Vec::new(), emblems: vec!["jup:ui_emblem_x".into()], frames: [("ui_emblem_x".to_owned(), (n, 2))].into_iter().collect(), blobs: None };
        let book = decode_pack(&p, "ui_emblem_x").unwrap();
        assert_eq!(book.frames.len(), 3);
        assert_eq!(book.poster, 2);
        assert_eq!((book.frames[1].width, book.frames[1].height), (w, h));
        assert_eq!(book.frames[2].pixels[0], 100);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn titles_and_frames() {
        assert_eq!(title("jup:jup_emblem_animated_opc_greekmythhades_01_atlas"), "Greek Myth Hades 01");
        assert_eq!(title("jup:ui_emblem_005"), "Emblem 005");
        assert_eq!(title("jup:ui_playercard_100#3"), "Calling Card 100");
        assert_eq!(title("jup:ui_emblem_mastery_gold_apapa"), "Mastery Gold Apapa");
        assert_eq!(title("jup:jup_playercard_animated_opc_rgbwarrior_06_atlas"), "RGB Warrior 06");
        assert_eq!(split_frame("ui_emblem_1#12"), ("ui_emblem_1", Some(12)));
        assert_eq!(split_frame("ui_emblem_1"), ("ui_emblem_1", None));
        assert_eq!(frame_name("jup:x", 0), "jup:x#0");
        assert!(is_look("jup:ui_emblem_005#2"));
        assert!(!is_look("jup:ui_group_bg_emblem_1"));
        assert!(!is_look("iw4:cardicon_skull"));
    }
}
