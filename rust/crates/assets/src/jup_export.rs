//! bo2mp `CodLegacy export-jup <folder>`: everything the game needs to play
//! Modern Warfare III's (2023) guns, written to a standalone folder in our
//! own converted files, so the guns, camos, calling cards and emblems can
//! be used without the game installed (`docs/JUP.md`, "The exported guns").
//!
//! - the guns' models, materials and pictures, and the camos with their
//!   names: read from the install by the lane's own build path
//!   (`lane::jup_gun::prefetch`) and written by `lane::jup_gun::export`;
//! - the guns' sounds (`asset_audio::export_jup_gun_sounds`);
//! - the guns' and camos' Create-a-Class pictures (`icons/`);
//! - every calling card and emblem with its frames (`looks/`);
//! - `manifest.json` (what is in it, and how it was made) and a `README.txt`.
//!
//! The game reads the folder back when Modern Warfare III's install is not
//! set (`BO2_JUP_EXPORT`, `asset_transport::jup_export`).

use std::path::Path;

use asset_transport::jup_export as ex;
use fastfile_jup::cards as jc;
use fastfile_jup::map::materials::{MapImage, Packages};

/// The layout's number (in the manifest).
const FORMAT: u32 = 1;
const ICONS: &str = "icons";
const LOOKS: &str = "looks";
const LOOKS_INDEX: &str = "looks.txt";
const EMBLEM_SIDE: u32 = 256;
const CARD_SIZE: (u32, u32) = (512, 128);
/// The widest a Create-a-Class picture is kept.
const ICON_SIDE: u32 = 512;

fn dir_bytes(dir: &Path) -> u64 {
    std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| match e.metadata() {
            Ok(m) if m.is_dir() => dir_bytes(&e.path()),
            Ok(m) => m.len(),
            Err(_) => 0,
        })
        .sum()
}

/// bo2mp `CodLegacy export-jup <folder>`: writes the export into `dest` from the
/// install that is set (`BO2_JUP_INSTALL`, or the `jup` game folder), and
/// reads it back to check it. `say` hears how far it is. The summary.
pub fn export_jup(dest: &Path, say: &mut dyn FnMut(&str)) -> Result<String, String> {
    let started = std::time::Instant::now();
    let install = asset_audio::jup_install().ok_or("Modern Warfare III's folder is not set (game_paths.cfg's jup line, or BO2_JUP_INSTALL)")?;
    let same = |a: &Path, b: &Path| a.canonicalize().ok().zip(b.canonicalize().ok()).is_some_and(|(a, b)| a == b || b.starts_with(&a));
    if same(&install, dest) {
        return Err("pick a folder outside the game's own".to_owned());
    }
    std::fs::create_dir_all(dest).map_err(|e| format!("{}: {e}", dest.display()))?;
    // (An export made before is replaced whole, not mixed with.)
    let _ = std::fs::remove_file(dest.join(ex::MANIFEST));

    let guns = crate::lane::export_jup_guns(dest, say)?;
    say("exporting the guns' sounds...");
    let sounds = asset_audio::export_jup_gun_sounds(&install, dest, say)?;
    say("exporting the guns' and camos' Create-a-Class pictures...");
    let icons = export_icons(&install, dest, say)?;
    say("exporting the calling cards and emblems...");
    let looks = export_looks(&install, dest, say)?;

    // The manifest last: an export without one is not taken for one.
    let roster = |gun: &str| bo2_profile::guest::JUP.iter().find(|r| r.gun == gun);
    let manifest = serde_json::json!({
        "format": FORMAT,
        "kind": "bo2mp-jup-export",
        "game": "Call of Duty: Modern Warfare III (2023)",
        "made_by": "CodLegacy export-jup",
        "made_at_unix": std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs()),
        "guns": guns.guns.iter().map(|g| {
            let r = roster(g);
            serde_json::json!({ "gun": g, "name": r.map(|r| r.name), "group": r.map(|r| r.group) })
        }).collect::<Vec<_>>(),
        "camos": guns.camos.iter().map(|(number, reference, name, game)| {
            serde_json::json!({ "number": number, "reference": reference, "name": name, "from": game })
        }).collect::<Vec<_>>(),
        "sounds": { "aliases": sounds.aliases, "clips": sounds.clips, "unreadable": sounds.failed },
        "materials": guns.materials,
        "icons": icons.0,
        "calling_cards": looks.cards,
        "emblems": looks.emblems,
        "folders": {
            "guns": "one .jgun per gun: its view and world models",
            "materials": "the guns' materials and pictures (packs)",
            "camos": "the camos' pictures (packs) and camos.tsv, number/reference/name/game",
            "sounds": "the guns' sounds (packs) and aliases.tsv",
            "icons": "Create-a-Class pictures of the guns and camos (packs of PNG)",
            "looks": "calling cards and emblems (packs of PNG, frames one under another) and looks.txt",
        },
    });
    let text = serde_json::to_string_pretty(&manifest).map_err(|e| e.to_string())?;
    std::fs::write(dest.join("README.txt"), README).map_err(|e| format!("README.txt: {e}"))?;
    std::fs::write(dest.join(ex::MANIFEST), text).map_err(|e| format!("{}: {e}", ex::MANIFEST))?;

    say("reading the export back to check it...");
    let check = check_jup_export(dest)?;
    Ok(format!(
        "Exported {} guns, {} camos, {} gun sound aliases, {icons} pictures, {} calling cards and {} emblems to {} ({:.0} MB) in {:.0} min. {check}",
        guns.guns.len(),
        guns.camos.len(),
        sounds.aliases,
        looks.cards,
        looks.emblems,
        dest.display(),
        dir_bytes(dest) as f64 / 1e6,
        started.elapsed().as_secs_f64() / 60.0,
        icons = icons.0,
    ))
}

/// Reads an export back the way the game does (the guns, their materials,
/// the camos, the sounds' lists, the looks' lists) and says what it holds.
pub fn check_jup_export(dir: &Path) -> Result<String, String> {
    if !ex::is_export(dir) {
        return Err(format!("{} is not an export (no {})", dir.display(), ex::MANIFEST));
    }
    let (guns, materials, camos, missing) = crate::lane::check_jup_export(dir);
    let looks = ex::PackIndex::open(&dir.join(LOOKS), LOOKS).map_or(0, |i| i.names().len());
    let icons = ex::PackIndex::open(&dir.join(ICONS), ICONS).map_or(0, |i| i.names().len());
    if guns == 0 {
        return Err(format!("{}: no gun reads back", dir.display()));
    }
    Ok(format!("Read back: {guns} guns, {materials} materials, {camos} camo pictures, {icons} icons, {looks} cards and emblems{}.", if missing.is_empty() { String::new() } else { format!("; {} problems, first: {}", missing.len(), missing[0]) }))
}

const README: &str = "Modern Warfare III (2023) guns, camos, calling cards and emblems, exported by the BO2 rebuild (CodLegacy export-jup).\r\n\
\r\n\
The files are the rebuild's own converted formats, not the game's. To use them, leave this folder where it is (Documents\\MW3_export is found by itself) or set the environment variable BO2_JUP_EXPORT to its path; the game then loads the guns, camos, banners and emblems from it when Modern Warfare III is not installed.\r\n\
manifest.json lists what is inside. To make the export again (after the rebuild's gun fixes), run: CodLegacy export-jup <folder> with Modern Warfare III's folder set (docs/JUP.md, \"The exported guns\").\r\n";

fn png_blob(w: u32, h: u32, rgba: &[u8]) -> Result<Vec<u8>, String> {
    crate::lane::png_rgba(w, h, rgba)
}

/// The Create-a-Class pictures of the played guns and the camos, as the
/// game reads them from the install (`ui::jup_hud`): the first `ICON_SIDE`
/// level the packages have.
fn export_icons(install: &Path, dest: &Path, say: &mut dyn FnMut(&str)) -> Result<(usize, u64), String> {
    let oodle = fastfile_jup::oodle::load(install).map_err(|e| format!("oodle: {e}"))?;
    let names: Vec<String> = bo2_profile::jup_names::JUP_PLAYED
        .iter()
        .flat_map(|g| bo2_profile::jup_names::icon_names(g))
        .chain(
            asset_game::weapon_camo_models::JUP_CAMOS
                .iter()
                .chain(asset_game::weapon_camo_models::IW9_CAMOS.iter())
                .map(|n| (*n).to_owned()),
        )
        .collect();
    let hashes: Vec<u64> = names.iter().map(|n| fastfile_jup::hash::name(n)).collect();
    let packages = Packages::shared(install);
    // (The zone of the pictures, then any other `global*mp` zone beside it.)
    let dir = install.join("zone");
    let mut zones = vec![dir.join("global_stream_mp.ff")];
    let mut more: Vec<std::path::PathBuf> = std::fs::read_dir(&dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            let stem = p.file_stem().and_then(|s| s.to_str()).unwrap_or("");
            p.extension().is_some_and(|e| e == "ff")
                && stem.starts_with("global")
                && stem.contains("mp")
                && stem != "global_stream_mp"
                && p.metadata().is_ok_and(|m| m.len() < 400_000_000)
        })
        .collect();
    more.sort();
    zones.extend(more);
    let mut pack = ex::PackWriter::new(&dest.join(ICONS), ICONS)?;
    let mut done: std::collections::HashSet<&str> = std::collections::HashSet::new();
    for path in zones {
        let zone = match fastfile_jup::read_zone(&path, oodle.as_ref()) {
            Ok((_, z)) => z,
            Err(e) => {
                say(&format!("icons: {} not read: {e}", path.display()));
                continue;
            }
        };
        let index = fastfile_jup::zone::HashIndex::build(&zone, hashes.iter().copied(), 8);
        for (name, &hash) in names.iter().zip(&hashes) {
            if done.contains(name.as_str()) {
                continue;
            }
            let Some(rec) = index.plain(hash).iter().find_map(|&at| match fastfile_jup::image::parse(&zone, at) {
                Ok(Some(r)) if r.hash == hash && r.depth == 1 && r.array == 1 => Some(r),
                _ => None,
            }) else {
                continue;
            };
            let img = MapImage {
                hash: rec.hash,
                width: rec.width,
                height: rec.height,
                format: rec.format,
                groups: rec.groups.clone(),
                inline: rec.inline.map(|(o, n)| zone[o..o + n].to_vec()),
            };
            let Ok(l) = packages.level(&img, ICON_SIDE, oodle.as_ref()) else { continue };
            let Some(px) = fastfile_jup::cards::decode_bcn(l.dxgi, l.width, l.height, &l.data) else { continue };
            pack.add(name, &png_blob(l.width, l.height, &px)?)?;
            done.insert(name);
        }
    }
    let out = (pack.count, pack.bytes);
    pack.finish()?;
    say(&format!("exported {} Create-a-Class pictures ({:.1} MB)", out.0, out.1 as f64 / 1e6));
    Ok(out)
}

/// What the looks' export made.
struct Looks {
    cards: usize,
    emblems: usize,
}

/// Every calling card and emblem of the install, decoded at the sizes the
/// game shows them (an emblem's frame 256 square, a card's 512x128), their
/// frames one under another in a PNG, with `looks.txt` listing
/// `<card|emblem><TAB>name<TAB>frames<TAB>the still one`.
fn export_looks(install: &Path, dest: &Path, say: &mut dyn FnMut(&str)) -> Result<Looks, String> {
    let oodle = fastfile_jup::oodle::load(install).map_err(|e| format!("oodle: {e}"))?;
    let zone_dir = install.join("zone");
    let (_, zone) = fastfile_jup::read_zone(&zone_dir.join(format!("{}.ff", jc::ZONE)), oodle.as_ref()).map_err(|e| format!("{}.ff: {e}", jc::ZONE))?;
    let mut packages = jc::Packages::new(&zone_dir, false);
    // (Only those whose pixels the install has: a few are fetched by the
    // game on demand.)
    let items: Vec<jc::Item> = jc::find_items(&zone)
        .into_iter()
        .filter(|i| i.image.inline.is_none() && fastfile_jup::image::dxgi(i.image.format).is_some())
        .filter(|i| i.image.groups.last().is_some_and(|g| packages.find(g.key).is_some()))
        .collect();
    let dir = dest.join(LOOKS);
    let mut pack = ex::PackWriter::new(&dir, LOOKS)?;
    let mut index = String::new();
    let mut out = Looks { cards: 0, emblems: 0 };
    for (k, item) in items.iter().enumerate() {
        if k % 250 == 0 {
            say(&format!("calling cards and emblems: {k} of {}", items.len()));
        }
        let max = match item.kind {
            jc::Kind::Emblem => EMBLEM_SIDE,
            jc::Kind::Card => CARD_SIZE.0,
        };
        let mut fetch = |key: u64| {
            let at = packages.locate(key);
            at.and_then(|(part, entry)| jc::read_located(&part, &entry, oodle.as_ref()))
        };
        let Some(pic) = jc::picture(None, item, &mut fetch, max) else { continue };
        let grid = jc::detect_grid(item, &pic);
        let n = grid.0 * grid.1;
        let (w, h) = match item.kind {
            jc::Kind::Emblem => (EMBLEM_SIDE, EMBLEM_SIDE),
            jc::Kind::Card => CARD_SIZE,
        };
        let frames: Vec<jc::Rgba> = (0..n).map(|i| jc::shrink(&jc::frame(&pic, grid, i), w, h)).collect();
        let Some(first) = frames.first() else { continue };
        let (fw, fh) = (first.width, first.height);
        let mut px = Vec::with_capacity((fw * fh * 4) as usize * frames.len());
        let mut whole = 0u32;
        for f in &frames {
            if f.width == fw && f.height == fh {
                px.extend_from_slice(&f.pixels);
                whole += 1;
            }
        }
        let poster = (jc::poster_frame(&pic, grid)).min(whole.saturating_sub(1));
        pack.add(&item.name, &png_blob(fw, fh * whole, &px)?)?;
        let kind = match item.kind {
            jc::Kind::Emblem => {
                out.emblems += 1;
                "emblem"
            }
            jc::Kind::Card => {
                out.cards += 1;
                "card"
            }
        };
        index.push_str(&format!("{kind}\t{}\t{whole}\t{poster}\n", item.name));
    }
    let bytes = pack.bytes;
    pack.finish()?;
    std::fs::write(dir.join(LOOKS_INDEX), index).map_err(|e| format!("{LOOKS_INDEX}: {e}"))?;
    say(&format!("exported {} calling cards and {} emblems ({:.1} MB)", out.cards, out.emblems, bytes as f64 / 1e6));
    Ok(out)
}

/// Notes which package entries an export reads (from an install whose parts
/// are empty files of the right sizes: only their `.xpak` indexes are real)
/// and writes their byte ranges to `plan`, one row per entry
/// (`file<TAB>offset<TAB>size<TAB>key`), for `jup_export --plan`. The export
/// itself goes to a scratch folder and is thrown away.
pub fn plan_jup(plan: &Path, scratch: &Path, say: &mut dyn FnMut(&str)) -> Result<String, String> {
    let install = asset_audio::jup_install().ok_or("Modern Warfare III's folder is not set (BO2_JUP_INSTALL)")?;
    fastfile_jup::pack::record_start();
    let made = export_jup(scratch, say);
    let direct = fastfile_jup::pack::recorded_ranges();
    let (keys, _) = fastfile_jup::pack::record_stop();
    let _ = std::fs::remove_dir_all(scratch);
    let _ = made;
    let (rows, missing) = fastfile_jup::pack::plan(&install, &keys, &direct)?;
    let mut text = String::new();
    let mut bytes = 0u64;
    for r in &rows {
        bytes += u64::from(r.size);
        text.push_str(&format!("{}\t{}\t{}\t{:016x}\n", r.file, r.offset, r.size, r.key));
    }
    std::fs::write(plan, text).map_err(|e| format!("{}: {e}", plan.display()))?;
    Ok(format!("{} entries read, {} planned ({:.1} MB), {} in no package", keys.len(), rows.len(), bytes as f64 / 1e6, missing))
}
