//! bo2mp lane C: Black Ops II's multiplayer front end needs no map: its UI
//! scripts, the pictures they name, its fonts, string tables and English
//! text come from the zones the front end loads (`code_post_gfx_mp`,
//! `ui_mp`, `patch_mp`, `patch_ui_mp`, a later one winning a name) and their
//! language zones (`zone/english/en_*`).

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::{Condvar, Mutex, OnceLock};

use asset_t6::{PackSet, ZoneCapture};

/// The front end's zones, in load order.
const FRONTEND_ZONES: [&str; 4] = ["code_post_gfx_mp", "ui_mp", "patch_mp", "patch_ui_mp"];

/// What the front end draws with: its scripts and their data, the pictures
/// they name, the fonts; and a report (one line per step).
pub struct T6Frontend {
    pub ui: crate::T6Ui,
    pub icons: crate::T6HudIcons,
    pub fonts: crate::T6HudFonts,
    /// The front end's sounds (its menus' `Engine.PlaySound` aliases and
    /// music), from the zones' sound banks and the bank files beside them.
    pub sound: Result<asset_audio::SoundCatalog, String>,
    pub report: Vec<String>,
}

/// Read the front end from his install's `zone/all` folder.
pub fn load_t6_frontend(zone_all: &Path) -> T6Frontend {
    let mut report = Vec::new();
    let mut last = std::time::Instant::now();
    lap(&mut Vec::new(), &mut last, "");
    let lang_dir = zone_all.parent().map(|d| d.join("english"));
    let front_paths: Vec<PathBuf> = FRONTEND_ZONES.iter().map(|z| zone_all.join(format!("{z}.ff"))).collect();
    let lang_paths: Vec<PathBuf> = match &lang_dir {
        Some(d) => FRONTEND_ZONES.iter().map(|z| d.join(format!("en_{z}.ff"))).collect(),
        None => Vec::new(),
    };
    // BO2's lobby loads common_mp (`loadcommonff`): the menus' own sounds
    // (cac_grid_nav, ... in mpl_common) and Create-a-Class's pictures (the
    // wildcards' menu_mp_bonuscard_*) are there.
    let common_paths = [zone_all.join("common_mp.ff"), lang_dir.clone().unwrap_or(PathBuf::new()).join("en_common_mp.ff")];
    // The map packs' load zones (their loading screen pictures).
    let load_paths: Vec<PathBuf> = (0..8).map(|n| zone_all.join(format!("dlc{n}_load_mp.ff"))).filter(|p| p.is_file()).collect();
    let (read, packs, lang_packs) =
        read_zones_and_packs(&[&front_paths, &lang_paths, &common_paths, &load_paths], zone_all, lang_dir.as_deref());
    let mut read = read.into_iter();
    let mut captures: Vec<ZoneCapture> = Vec::new();
    for (z, r) in FRONTEND_ZONES.iter().zip(read.by_ref().take(front_paths.len())) {
        match r {
            Ok(c) => captures.push(c),
            Err(e) => report.push(format!("t6 front end: {z}: {e}")),
        }
    }
    let lang_caps: Vec<ZoneCapture> = read.by_ref().take(lang_paths.len()).filter_map(Result::ok).collect();
    let common: Vec<ZoneCapture> = common_paths
        .iter()
        .zip(read.by_ref().take(common_paths.len()))
        .filter_map(|(p, r)| r.map_err(|e| report.push(format!("t6 front end sound: {}: {e}", p.display()))).ok())
        .collect();
    let loads: Vec<ZoneCapture> = read.filter_map(Result::ok).collect();
    lap(&mut report, &mut last, "its zones, their English zones, common_mp, the load zones and the image packs");
    let packs = match packs {
        Ok(p) => Some(p),
        Err(e) => {
            report.push(format!("t6 front end image packs: {e}"));
            None
        }
    };
    let refs: Vec<&ZoneCapture> = captures.iter().collect();
    // The pictures: the front end's zones (the latest first), then common_mp's.
    // (A later front-end zone's picture wins: patch_ui_mp holds a 2048 square
    // `menu_mp_soldiers` that replaces ui_mp's 1024 one, which shows far
    // brighter on the left than BO2's menu.)
    let pics: Vec<&ZoneCapture> = captures.iter().rev().chain(common.iter().take(1)).collect();
    // bo2mp: the zones' named visions (mpOutro, mpIntro, ...) for the
    // scripts' `visionsetnaked`.
    asset_world::register_t6_visions(
        captures
            .iter()
            .chain(common.iter())
            .flat_map(|c| c.raw_files.iter())
            .filter_map(|(name, bytes)| {
                let n = name.replace('\\', "/");
                let n = n.strip_prefix("vision/")?.strip_suffix(".vision")?.to_owned();
                let v = asset_world::parse_t6_vision(&String::from_utf8_lossy(bytes))?;
                Some((n, v))
            })
            .collect(),
    );
    let scripts = super::t6_m2::ui_scripts(&refs);
    let mut names = super::t6_m2::ui_strings(&scripts);
    names.extend(table_picture_names(&pics));
    // The button glyphs' pictures (BO2 PC's ^BBUTTON_CYCLE_LEFT^ arrows).
    // (BO2's own icon set, the FontIcon asset, names each glyph's picture.)
    names.extend(glyph_pictures(&refs));
    names.extend(["ui_arrow_left", "ui_arrow_right"].map(str::to_owned));
    // The pips the engine draws for `setupDashes` (code_post_gfx_mp's own
    // materials: grey filled, hollow outline, green better, red worse).
    names.extend(["menu_mp_pip_blue", "menu_mp_pip_outline", "menu_mp_pip_green", "menu_mp_pip_red"].map(str::to_owned));
    // The lobby's map pictures: its scripts make the names ("menu_" .. map
    // .. "_map_select_final").
    for c in &refs {
        for m in &c.materials {
            if m.name.starts_with("menu_mp_") && m.name.ends_with("_map_select_final") {
                names.insert(m.name.clone());
            }
        }
    }
    let icons = super::t6_m2::hud_icons(&pics, packs.as_ref(), &names, &mut report, super::t6_m2::IconShare::Keep);
    lap(&mut report, &mut last, "pictures");
    let fonts = super::t6_m2::hud_fonts(&lang_caps, lang_packs.as_ref().or(packs.as_ref()), &mut report);
    lap(&mut report, &mut last, "fonts");
    let sound = frontend_sound(zone_all, &refs, &lang_caps, &common, &mut report);
    lap(&mut report, &mut last, "sounds");
    // Create-a-Class asks each grenade's most (Engine.GetMaxAmmoForItem:
    // the weapon file's maxAmmo): a two-column table "bo2mp/maxammo.csv"
    // (weapon name, maxAmmo) from the MP weapons in common_mp.
    let mut max_ammo: Vec<String> = Vec::new();
    for w in common.iter().chain(captures.iter()).flat_map(|c| c.weapons.iter()) {
        if !w.name.contains('+') && !max_ammo.iter().step_by(2).any(|n| *n == w.name) {
            max_ammo.push(w.name.clone());
            max_ammo.push(w.def_i32(fastfile_t6::layout::WeaponDef::iMaxAmmo).to_string());
        }
    }
    drop(common);
    let strings: Vec<(String, String)> = lang_caps.iter().flat_map(|c| c.localize.iter().cloned()).collect();
    let mut tables: Vec<(String, usize, usize, Vec<String>)> = captures
        .iter()
        .flat_map(|c| c.string_tables.iter())
        .map(|t| (t.name.clone(), t.columns, t.rows, t.cells.clone()))
        .collect();
    report.push(format!("t6 front end: {} weapons' maxAmmo for Create-a-Class", max_ammo.len() / 2));
    tables.push(("bo2mp/maxammo.csv".to_owned(), 2, max_ammo.len() / 2, max_ammo));
    report.push(format!(
        "t6 front end: {} scripts, {} English strings, {} string tables, {} pictures",
        scripts.len(),
        strings.len(),
        tables.len(),
        icons.0.len()
    ));
    if let Some(packs) = &packs {
        preload_extra_pictures(packs, zone_all, &loads, &refs, &icons);
    }
    let icons = with_loadscreens(icons, zone_all, &loads, &refs, packs.as_ref(), &mut report);
    let icons = with_scene_textures(icons, &refs, packs.as_ref(), &mut report);
    if let Some(packs) = &packs {
        packs.drop_preloaded();
    }
    lap(&mut report, &mut last, "tables, loading screens and 3D backdrop");
    T6Frontend {
        ui: crate::T6Ui {
            game_settings: Default::default(),
            scripts,
            strings: std::sync::Arc::new(strings),
            tables: std::sync::Arc::new(tables),
            configs: std::sync::Arc::new(super::t6_m2::ui_configs(&refs)),
        },
        icons,
        fonts,
        sound,
        report,
    }
}

/// One zone file read and walked (`asset_t6::capture_zone`).
type ZoneRead = Result<ZoneCapture, String>;

/// How many of the front end's zone files are read at once: 4, or
/// `CODLEGACY_FRONTEND_ZONE_READERS` (1 to 8; 1 = one after another).
fn frontend_zone_readers() -> usize {
    codlegacy_env::var("CODLEGACY_FRONTEND_ZONE_READERS")
        .ok()
        .and_then(|v| v.trim().parse::<usize>().ok())
        .map_or(4, |n| n.clamp(1, 8))
}

/// Busy PC: the front end's zone files (its 4, their English ones,
/// common_mp and its English one, the map packs' load zones) and both
/// image-pack folders' indexes were read one after another, each read
/// waiting its turn behind every other program's on a shared hard drive, and
/// the zones' walks (CPU) waited for each read. They are read a few at once,
/// the biggest first (common_mp is 40 of the 46 MB), each by its own thread,
/// while the packs' indexes are read beside them. What is read is the same:
/// each group's captures in its order, then the packs.
/// `CODLEGACY_FRONTEND_ZONE_READERS=1` reads them one after another.
fn read_zones_and_packs(
    groups: &[&[PathBuf]],
    zone_all: &Path,
    lang_dir: Option<&Path>,
) -> (Vec<ZoneRead>, Result<PackSet, String>, Option<PackSet>) {
    let paths: Vec<&PathBuf> = groups.iter().flat_map(|g| g.iter()).collect();
    let readers = frontend_zone_readers();
    if readers <= 1 {
        let read = paths.iter().map(|p| asset_t6::capture_zone(p)).collect();
        return (read, PackSet::open_dir(zone_all), lang_dir.and_then(|d| PackSet::open_dir(d).ok()));
    }
    let sizes: Vec<u64> = paths.iter().map(|p| std::fs::metadata(p).map_or(0, |m| m.len())).collect();
    let mut order: Vec<usize> = (0..paths.len()).collect();
    order.sort_by_key(|&i| std::cmp::Reverse(sizes[i]));
    std::thread::scope(|s| {
        let packs = s.spawn(|| PackSet::open_dir(zone_all));
        let lang = s.spawn(|| lang_dir.and_then(|d| PackSet::open_dir(d).ok()));
        let read = asset_t6::on_threads(&paths, &order, readers, |p| asset_t6::capture_zone(p))
            .into_iter()
            .zip(&paths)
            .map(|(r, p)| r.unwrap_or_else(|| Err(format!("{}: not read (its reader stopped)", p.display()))))
            .collect();
        let packs = packs
            .join()
            .unwrap_or_else(|_| Err(format!("{}: image packs not opened (their thread stopped)", zone_all.display())));
        (read, packs, lang.join().ok().flatten())
    })
}

/// bo2mp: one timing line per part of the front end's read (its 50 s on his
/// hard drive came as one number).
fn lap(report: &mut Vec<String>, last: &mut std::time::Instant, part: &str) {
    // The program's reads since the last part: on a cold hard drive the
    // count (a seek each, at worst) is most of the time.
    static READS: std::sync::Mutex<(u64, u64)> = std::sync::Mutex::new((0, 0));
    let reads = diag::process_reads().map_or_else(String::new, |(n, bytes)| {
        let mut prev = READS.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let (pn, pb) = std::mem::replace(&mut *prev, (n, bytes));
        if pn == 0 {
            return String::new();
        }
        format!(", {} reads, {:.1} MB", n - pn, (bytes - pb) as f64 / 1_048_576.0)
    });
    report.push(format!("t6 front end: {part} in {:.1} s{reads}", last.elapsed().as_secs_f64()));
    *last = std::time::Instant::now();
}

/// The front end's sound catalog: its zones' banks (`mpl_ui`, the music)
/// and the shared multiplayer ones, their clips in the install's `sound`
/// folder.
fn frontend_sound(
    zone_all: &Path,
    captures: &[&ZoneCapture],
    lang_caps: &[ZoneCapture],
    common: &[ZoneCapture],
    report: &mut Vec<String>,
) -> Result<asset_audio::SoundCatalog, String> {
    let dir = zone_all
        .parent()
        .and_then(Path::parent)
        .map(|install| install.join("sound"))
        .filter(|d| d.is_dir())
        .ok_or_else(|| "Black Ops II sound folder not found beside the zones".to_owned())?;
    let bases = super::t6_m2::bank_bases(captures, "mp_frontend");
    let base_refs: Vec<&str> = bases.iter().map(String::as_str).collect();
    let (index, bank_report) = asset_audio::T6BankIndex::open(&dir, &base_refs);
    report.extend(bank_report);
    let banks: Vec<&asset_t6::SndBankRef> = captures
        .iter()
        .copied()
        .chain(lang_caps.iter())
        .chain(common.iter())
        .flat_map(|c| c.sound_banks.iter())
        .collect();
    let globals = captures.iter().find_map(|c| c.snd_globals.as_ref());
    let (catalog, census) =
        asset_audio::build_t6_sound_catalog(&banks, globals, &index, asset_core::ZoneOwner::intern("mp_frontend"));
    report.push(format!(
        "t6 front end sound: {} aliases, {} variants ({} with a clip)",
        census.aliases, census.variants, census.with_clip
    ));
    Ok(catalog)
}

/// The pictures the menus build from table cells (`menu_mp_weapons_tar21`
/// + `_big`, `perk_lightweight` + `_256`): every material whose name is a
/// cell, or a cell and a `_` suffix.
pub(super) fn table_picture_names(captures: &[&ZoneCapture]) -> std::collections::BTreeSet<String> {
    let cells: std::collections::HashSet<&str> = captures
        .iter()
        .flat_map(|c| c.string_tables.iter())
        .flat_map(|t| t.cells.iter())
        .map(String::as_str)
        .filter(|c| c.len() >= 4 && !c.contains(' '))
        .collect();
    let mut out = std::collections::BTreeSet::new();
    for c in captures {
        for m in &c.materials {
            let n = m.name.as_str();
            if cells.contains(n) || n.match_indices('_').any(|(i, _)| cells.contains(&n[..i])) {
                out.insert(n.to_owned());
            }
        }
    }
    out
}

/// The loading screens' and the 3D backdrop's pictures (about 20 at 2048
/// square, 58 MB) as one bundle file, as the front end's other pictures
/// are read (`PackSet::preload`): read one by one, a cold boot sought for
/// each and wrote each to the cache as well (5.6 s with the tables on his
/// hard drive). The pictures `with_loadscreens` and `with_scene_textures`
/// decode next come from memory; nothing else changes.
fn preload_extra_pictures(
    packs: &PackSet,
    zone_all: &Path,
    loads: &[ZoneCapture],
    refs: &[&ZoneCapture],
    icons: &crate::T6HudIcons,
) {
    let mut seen: HashSet<&str> = HashSet::new();
    let mut images: Vec<&asset_t6::ImageRef> = Vec::new();
    // The copy `with_loadscreens` takes: the latest zone's.
    for c in loads.iter().rev().chain(refs.iter().copied().rev()) {
        for m in &c.materials {
            let Some(map) = m.name.strip_prefix("loadscreen_") else { continue };
            if !zone_all.join(format!("{map}.ff")).is_file()
                || icons.0.iter().any(|(k, _)| *k == m.name)
                || !seen.insert(m.name.as_str())
            {
                continue;
            }
            if let Some(image) = super::t6_materials::colour_texture(m, c).and_then(|t| c.images.get(t.image?.index)) {
                images.push(image);
            }
        }
    }
    for name in ["ui_globe", "ui_holotable_grid", "ui_holotable_grid2", "ui_holotable_grid3"] {
        let Some((m, c)) = refs.iter().find_map(|c| c.materials.iter().find(|m| m.name == name).map(|m| (m, *c))) else {
            continue;
        };
        images.extend(m.textures.iter().filter_map(|t| t.image.and_then(|k| c.images.get(k.index))));
    }
    let mut entries: Vec<(usize, ipak_t6::IndexEntry)> = images
        .into_iter()
        .filter(|image| !super::t6_materials::mod_holds(image))
        .filter_map(|image| match packs.locate(image) {
            Some(asset_t6::ImageSource::Pack(i, e)) => Some((i, e)),
            _ => None,
        })
        .collect();
    sorted_once(&mut entries);
    if !entries.is_empty() {
        packs.preload("t6frontextra", &entries);
    }
}

/// BO2's loading screen pictures (its Loading menu shows
/// `loadscreen_<map>`): one per map whose zone is in his install, from the
/// front end's zones or the map packs' `dlc<N>_load_mp` zones (Nuketown
/// 2025's is in dlc0_load_mp), added to the front end's pictures.
fn with_loadscreens(
    mut icons: crate::T6HudIcons,
    zone_all: &Path,
    loads: &[ZoneCapture],
    refs: &[&ZoneCapture],
    packs: Option<&PackSet>,
    report: &mut Vec<String>,
) -> crate::T6HudIcons {
    let Some(packs) = packs else { return icons };
    let mut added = Vec::new();
    // The latest zone's picture wins, as for every front-end picture (patch_mp
    // holds 2048 square `loadscreen_mp_*` that replace code_post_gfx_mp's):
    // walk the zones latest first and keep the first copy of a name.
    for c in loads.iter().rev().chain(refs.iter().copied().rev()) {
        for m in &c.materials {
            let Some(map) = m.name.strip_prefix("loadscreen_") else { continue };
            if !zone_all.join(format!("{map}.ff")).is_file() || icons.0.iter().any(|(k, _)| *k == m.name) {
                continue;
            }
            let Some(image) = super::t6_materials::colour_texture(m, c).and_then(|t| c.images.get(t.image?.index)) else {
                continue;
            };
            match super::t6_materials::decode(packs, image) {
                Ok(img) => {
                    icons.0.push((m.name.clone(), std::sync::Arc::new(img)));
                    added.push(m.name.clone());
                }
                Err(e) => report.push(format!("t6 front end: {}: {e}", m.name)),
            }
        }
    }
    report.push(format!("t6 front end: loading screen pictures {added:?}"));
    icons
}

/// The textures of the front end's 3D background (main.lua's lobby
/// backdrop: the holotable grids `ui_holotable_grid*` and the globe
/// `ui_globe`), as plain pixels by image name (`globe_map`, `plus_tile`,
/// ...): the UI draws them itself (`lui_scene`), as a 2D picture cannot
/// show a turned plane or a sphere.
fn with_scene_textures(
    mut icons: crate::T6HudIcons,
    refs: &[&ZoneCapture],
    packs: Option<&PackSet>,
    report: &mut Vec<String>,
) -> crate::T6HudIcons {
    let Some(packs) = packs else { return icons };
    let mut added = Vec::new();
    for name in ["ui_globe", "ui_holotable_grid", "ui_holotable_grid2", "ui_holotable_grid3"] {
        let Some((m, c)) = refs.iter().find_map(|c| c.materials.iter().find(|m| m.name == name).map(|m| (m, *c))) else {
            continue;
        };
        for t in &m.textures {
            let Some(image) = t.image.and_then(|k| c.images.get(k.index)) else {
                report.push(format!("t6 front end: {name} texture without an image"));
                continue;
            };
            report.push(format!("t6 front end: {name} texture {}", image.name));
            let key = format!("scene:{}", image.name);
            if icons.0.iter().any(|(k, _)| *k == key) {
                continue;
            }
            if let Ok(img) = super::t6_materials::decode(packs, image)
                && let Some(img) = super::t6_materials::to_rgba8(&img)
            {
                icons.0.push((key, std::sync::Arc::new(img)));
                added.push(image.name.clone());
            }
        }
    }
    report.push(format!("t6 front end: 3D backdrop textures {added:?}"));
    // Pictures the engine itself puts in the menus (no script names them):
    // the lobby rows' speaker.
    let glyphs = glyph_pictures(refs);
    for name in ["nottalkingicon", "talkingicon", "ui_arrow_left", "ui_arrow_right", "menu_theater_nodata", "lui_loader"]
        .into_iter()
        .map(str::to_owned)
        .chain(glyphs)
    {
        let name = name.as_str();
        let Some((m, c)) = refs.iter().find_map(|c| c.materials.iter().find(|m| m.name == name).map(|m| (m, *c))) else {
            continue;
        };
        if icons.0.iter().any(|(k, _)| k == name) {
            continue;
        }
        let Some(image) = super::t6_materials::colour_texture(m, c).and_then(|t| c.images.get(t.image?.index)) else { continue };
        if let Ok(img) = super::t6_materials::decode(packs, image) {
            icons.0.push((name.to_owned(), std::sync::Arc::new(img)));
        }
    }
    icons
}

/// The pictures of BO2's button glyphs (`^BBUTTON_MOUSE_LEFT^` ...): the
/// material each FontIcon entry names (the capture's `bo2mp/fonticon.csv`).
fn glyph_pictures(refs: &[&ZoneCapture]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for t in refs.iter().flat_map(|c| c.string_tables.iter()).filter(|t| t.name == "bo2mp/fonticon.csv") {
        for row in t.cells.chunks(t.columns.max(1)) {
            if let Some(m) = row.get(2).filter(|m| !m.is_empty()) && !out.contains(m) {
                out.push(m.clone());
            }
        }
    }
    out
}

/// bo2mp: a picture's top level as plain RGBA8 (block formats decoded), for
/// the UI to blur.
pub fn plain_rgba8(image: &bevy::prelude::Image) -> Option<bevy::prelude::Image> {
    super::t6_materials::to_rgba8(image)
}

/// bo2mp shell (Zombies): Black Ops II's own Zombies pictures - the lobby's
/// map pictures (`menu_zm_*`) and the loading pictures (`loadscreen_zm_*`) -
/// from the Zombies front end's zones in `zone_all` (TranZit's loading
/// pictures sit in `code_post_gfx_zm`, the DLC maps' in their packs' load
/// zones) and any `extra` zone files (a map pack's `mod_load.ff`), as plain
/// RGBA8 (`plain_rgba8`), for the shell's Zombies screens. Zones that are
/// not there are skipped; the report says what was read.
pub fn load_t6_zm_pictures(zone_all: &Path, extra: &[std::path::PathBuf]) -> (Vec<(String, bevy::prelude::Image)>, Vec<String>) {
    const ZONES: [&str; 9] = [
        "code_post_gfx_zm",
        "common_zm",
        "ui_zm",
        "patch_ui_zm",
        "dlczm0_load_zm",
        "dlc1_load_zm",
        "dlc2_load_zm",
        "dlc3_load_zm",
        "dlc4_load_zm",
    ];
    let mut report = Vec::new();
    let paths: Vec<std::path::PathBuf> = ZONES.iter().map(|z| zone_all.join(format!("{z}.ff"))).chain(extra.iter().cloned()).collect();
    let mut captures: Vec<ZoneCapture> = Vec::new();
    for p in paths.iter().filter(|p| p.is_file()) {
        match asset_t6::capture_zone(p) {
            Ok(c) => captures.push(c),
            Err(e) => report.push(format!("t6 zm pictures: {}: {e}", p.display())),
        }
    }
    // The install's image packs, then each extra zone's folder's.
    let mut packs: Vec<PackSet> = Vec::new();
    for dir in std::iter::once(zone_all.to_path_buf()).chain(extra.iter().filter_map(|p| p.parent().map(Path::to_path_buf))) {
        if let Ok(p) = PackSet::open_dir(&dir) {
            packs.push(p);
        }
    }
    let mut out: Vec<(String, bevy::prelude::Image)> = Vec::new();
    // (A later zone's picture wins a name: the patch zones come after.)
    for c in captures.iter().rev() {
        for m in &c.materials {
            let name = m.name.as_str();
            if !(name.starts_with("menu_zm_") || name.starts_with("loadscreen_zm_") || name.starts_with("hud_chalk_")) || out.iter().any(|(n, _)| n == name) {
                continue;
            }
            let Some(image) = super::t6_materials::colour_texture(m, c).and_then(|t| c.images.get(t.image?.index)) else {
                continue;
            };
            // (A `,name` image is one another zone defines: found in the
            // packs by its name alone, as `hud_icons` does.)
            let decoded = packs.iter().find_map(|p| {
                super::t6_materials::decode(p, image).ok().or_else(|| {
                    if !image.name.starts_with(',') {
                        return None;
                    }
                    let (i, entry) = p.locate_name(&image.name)?;
                    let bytes = p.packs[i].read(entry).ok()?;
                    let iwi = ipak_t6::parse_iwi(&bytes).ok()?;
                    super::t6_materials::gpu_image(&iwi).ok()
                })
            });
            match decoded.as_ref().and_then(super::t6_materials::to_rgba8) {
                Some(img) => out.push((name.to_owned(), img)),
                None => report.push(format!("t6 zm pictures: {name}: not decoded")),
            }
        }
    }
    report.push(format!(
        "t6 zm pictures: {} from {} zones ({})",
        out.len(),
        captures.len(),
        captures.iter().map(|c| c.zone.as_str()).collect::<Vec<_>>().join(", ")
    ));
    (out, report)
}

/// bo2mp: the first map load's slow part read while the front end is up.
/// On a cold PC most of a map load is the disk finding the shared zones'
/// pictures in the image packs (Nuketown: 73 s of 96 s; the next map 12 s).
/// This thread reads them once, at background priority and in pack order,
/// and drops the bytes: the system's file cache keeps them, so the map load
/// finds them in memory. A warm PC reads them from that cache in a second or
/// two. `CODLEGACY_T6_NO_PREWARM` turns it off.
pub fn prewarm_t6_mp_shared(zone_all: &Path) {
    if codlegacy_env::var_os("CODLEGACY_T6_NO_PREWARM").is_some() {
        return;
    }
    let zone_all = zone_all.to_path_buf();
    let spawned = std::thread::Builder::new()
        .name("t6 shared prewarm".into())
        .spawn(move || {
            asset_audio::background_io(true);
            wait_for_menu();
            diag::info!(Launch, "t6 prewarm: the game's own code: {}", prefetch_own_image());
            let started = std::time::Instant::now();
            let mut warmed = HashSet::new();
            let line = prewarm(&zone_all, &mut warmed);
            let sound = sound_dir(&zone_all).map_or_else(String::new, |dir| {
                let bases = ["cmn_root", "mpl_common", "mpl_code_post_gfx"].map(String::from);
                format!(", {}", warm_banks(&dir, &bases, &stopped))
            });
            diag::info!(Launch, "t6 shared prewarm: {line}{sound} in {:.1} s", started.elapsed().as_secs_f32());
            // Then the map the lobby has picked, and each new pick after it.
            map_prewarm_loop(&zone_all, &mut warmed);
        });
    if let Err(e) = spawned {
        diag::info!(Launch, "t6 shared prewarm: no thread: {e}");
    }
}

/// Launch speed: the game's own code read in whole the moment it starts, on
/// its own thread at normal priority, so the launch's first runs of
/// everything (the game scan, the renderer's setup, the zone readers) find it
/// in memory instead of paging it in a few KB at a time from his hard drive
/// beside the front end's reads (threads were seen stuck in page-in waits
/// for minutes). The prewarm's later read then finds it there.
/// `CODLEGACY_PREFETCH_EXE_AT_START=0` leaves it to the prewarm (the old way).
pub fn prefetch_own_image_at_launch() {
    if codlegacy_env::var("CODLEGACY_PREFETCH_EXE_AT_START").is_ok_and(|v| v.trim() == "0") {
        return;
    }
    let spawned = std::thread::Builder::new().name("exe-prefetch".into()).spawn(|| {
        let at = diag::process_elapsed_ns() as f64 / 1e9;
        let line = prefetch_own_image();
        diag::info!(Launch, "boot: the game's own code: {line} (from {at:.1} s)");
    });
    if let Err(e) = spawned {
        diag::info!(Launch, "boot: the game's own code: no thread: {e}");
    }
}

/// The game's own code (CodLegacy.exe, 175 MB, on his hard drive) read in
/// whole: until now each part ran for the first time paged its code in a
/// few KB at a time, and the match's install (sim boot, scripts) ran 0.5 s
/// warm but 21-27 s cold with no file reads of its own. One
/// `PrefetchVirtualMemory` over the loaded image reads it in large requests
/// at this thread's background priority; the pages wait in memory, not in
/// the working set. Nothing it runs changes.
fn prefetch_own_image() -> String {
    #[cfg(windows)]
    {
        #[repr(C)]
        struct MemoryRange {
            address: *const u8,
            bytes: usize,
        }
        unsafe extern "system" {
            fn GetModuleHandleW(name: *const u16) -> *const u8;
            fn GetCurrentProcess() -> isize;
            fn PrefetchVirtualMemory(process: isize, count: usize, ranges: *const MemoryRange, flags: u32) -> i32;
        }
        let started = std::time::Instant::now();
        // SAFETY: the exe's own module handle is its loaded image's base;
        // its PE headers stay mapped for the process's life (e_lfanew at
        // 0x3C; SizeOfImage 56 bytes into the optional header, after the
        // "PE\0\0" signature and the 20-byte file header).
        let (base, size) = unsafe {
            let base = GetModuleHandleW(core::ptr::null());
            if base.is_null() {
                return "no module handle".into();
            }
            let nt = base.add(base.add(0x3C).cast::<u32>().read_unaligned() as usize);
            if nt.cast::<u32>().read_unaligned() != 0x0000_4550 {
                return "no PE header".into();
            }
            (base, nt.add(4 + 20 + 56).cast::<u32>().read_unaligned() as usize)
        };
        let range = MemoryRange { address: base, bytes: size };
        // SAFETY: one range inside this process's own mapped image.
        let ok = unsafe { PrefetchVirtualMemory(GetCurrentProcess(), 1, &range, 0) } != 0;
        let secs = started.elapsed().as_secs_f32();
        if ok {
            format!("{} MB read in {secs:.1} s", size >> 20)
        } else {
            format!("{} MB not read ({})", size >> 20, std::io::Error::last_os_error())
        }
    }
    #[cfg(not(windows))]
    {
        "not Windows".into()
    }
}

/// START MATCH was pressed: the prewarm steps aside so the load has the
/// disk to itself (on his HDD the two fought for minutes). A new lobby
/// pick after the match lets it run again.
static STOPPED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

fn stopped() -> bool {
    STOPPED.load(std::sync::atomic::Ordering::Relaxed)
}

/// The match is loading: the prewarm thread stops reading.
pub fn stop_t6_prewarm() {
    asset_audio::t6_bank_copies_may_start();
    if !STOPPED.swap(true, std::sync::atomic::Ordering::Relaxed) {
        diag::info!(Launch, "t6 prewarm: stopped for the match's load");
    }
}

/// The menu has drawn its first frame. Till then the prewarm waits: on his
/// hard drive it read beside the front end's own zones and both crawled
/// (the front end took 83 s cold).
static MENU_UP: (Mutex<bool>, Condvar) = (Mutex::new(false), Condvar::new());

/// Builds the common set with the guest guns his classes hold (once per set
/// of games; nothing before the launch armed it). Called when the menu is up
/// and again when his classes change.
pub fn prefetch_guest_guns() {
    crate::session_load::prefetch_lobby_common();
}

/// The menu is on screen: the prewarm may read.
pub fn t6_menu_up() {
    asset_audio::t6_bank_copies_may_start();
    // (The guest guns of the games his saved classes hold are built now, while
    // the menus are up, not when a map is picked: a match then starts with
    // them made.)
    prefetch_guest_guns();
    if let Ok(mut up) = MENU_UP.0.lock() {
        *up = true;
        MENU_UP.1.notify_all();
    }
}

/// Until the menu is up, or two minutes if nothing says so.
fn wait_for_menu() {
    let Ok(up) = MENU_UP.0.lock() else {
        return;
    };
    let _ = MENU_UP.1.wait_timeout_while(up, std::time::Duration::from_secs(120), |up| !*up);
}

/// The map the lobby has picked (`ui_mapname`), for the prewarm thread to
/// read ahead of the load, and a count bumped on each new pick. Then the
/// maps the rotation may play after it, read once the pick is in the cache.
struct MapWanted {
    map: String,
    generation: u64,
    running: bool,
    next: Vec<String>,
    next_generation: u64,
}

fn map_wanted() -> &'static (Mutex<MapWanted>, Condvar) {
    static WANTED: OnceLock<(Mutex<MapWanted>, Condvar)> = OnceLock::new();
    WANTED.get_or_init(|| {
        let wanted = MapWanted { map: String::new(), generation: 0, running: false, next: Vec::new(), next_generation: 0 };
        (Mutex::new(wanted), Condvar::new())
    })
}

/// The lobby's map changed (or was first shown): read its own pictures and
/// sound banks into the system's file cache in the background, so START
/// MATCH finds them there (a cold Nuketown spent 16.7 s on its pictures).
/// Cheap to call every frame: a repeat of the last map does nothing.
///
/// While the menu shell has a pick (`prewarm_t6_mp_pick`) that pick wins:
/// the lobby's `ui_mapname` stays on its default until START MATCH, so the
/// prewarm read Aftermath all session and the map he chose only in the
/// 5 s countdown.
pub fn prewarm_t6_mp_map(map: &str) {
    let pick = if pick_on() { SHELL_PICK.lock().ok() } else { None };
    want_map(pick.as_ref().and_then(|p| p.as_deref()).unwrap_or(map));
}

/// The map the menu shell shows or has under the cursor on its map grid.
static SHELL_PICK: Mutex<Option<String>> = Mutex::new(None);

/// Whether the shell's pick drives the prewarm (`CODLEGACY_T6_PREWARM_PICK=0`:
/// only `ui_mapname` does, the old way).
fn pick_on() -> bool {
    static ON: OnceLock<bool> = OnceLock::new();
    *ON.get_or_init(|| codlegacy_env::var("CODLEGACY_T6_PREWARM_PICK").map_or(true, |v| v.trim() != "0"))
}

/// The menu shell's map: the lobby's chosen map, or the tile under the
/// cursor on the map grid, so its pictures are read while he looks, before
/// START MATCH. A short dwell (`PICK_DWELL`) lets the cursor pass over
/// tiles without reading each one. Cheap to call every frame.
pub fn prewarm_t6_mp_pick(map: &str) {
    if !pick_on() || map.is_empty() {
        return;
    }
    let Ok(mut pick) = SHELL_PICK.lock() else {
        return;
    };
    if pick.as_deref() != Some(map) {
        *pick = Some(map.to_owned());
    }
    want_map(map);
}

/// How long a pick must stay before its reads start (the map grid's cursor
/// passing over tiles).
const PICK_DWELL: std::time::Duration = std::time::Duration::from_millis(300);

fn want_map(map: &str) {
    let (lock, wake) = map_wanted();
    let Ok(mut wanted) = lock.lock() else {
        return;
    };
    if !wanted.running || wanted.map == map {
        return;
    }
    STOPPED.store(false, std::sync::atomic::Ordering::Relaxed);
    wanted.map = map.to_owned();
    wanted.generation += 1;
    wake.notify_one();
}

/// How many of the rotation's next maps are read ahead (3; 0 turns it off):
/// `CODLEGACY_T6_PREWARM_NEXT`.
fn next_maps_cap() -> usize {
    codlegacy_env::var("CODLEGACY_T6_PREWARM_NEXT").ok().and_then(|n| n.trim().parse().ok()).unwrap_or(3)
}

/// The maps the rotation may play after this one, the likeliest first: read
/// after the lobby's pick, while nothing else wants the disk (the lobby, and
/// the end of a match before the map change), so the next map's load finds
/// its pictures in the file cache too. A repeat of the same list does
/// nothing.
pub fn prewarm_t6_mp_next(maps: &[String]) {
    let mut maps = maps.to_vec();
    maps.truncate(next_maps_cap());
    let (lock, wake) = map_wanted();
    let Ok(mut wanted) = lock.lock() else {
        return;
    };
    if wanted.next == maps {
        return;
    }
    wanted.next = maps;
    wanted.next_generation += 1;
    wake.notify_one();
}

/// The match has ended (its scoreboard is up): the prewarm, stopped for
/// the match's load, reads the rotation's next maps before the map change.
pub fn t6_match_ended() {
    let (lock, wake) = map_wanted();
    let Ok(mut wanted) = lock.lock() else {
        return;
    };
    if wanted.next.is_empty() || !stopped() {
        return;
    }
    STOPPED.store(false, std::sync::atomic::Ordering::Relaxed);
    diag::info!(Launch, "t6 prewarm: the match ended, reading the next maps {:?}", wanted.next);
    wanted.next_generation += 1;
    wake.notify_one();
}

/// What the prewarm thread reads next.
enum MapJob {
    /// The lobby's pick.
    Pick(String, u64),
    /// The rotation's next maps (with the pick's count they were read under).
    Next(Vec<String>, u64),
}

fn map_prewarm_loop(zone_all: &Path, warmed: &mut HashSet<(usize, u32, u32)>) {
    let (lock, wake) = map_wanted();
    let (mut done, mut next_done) = (0u64, 0u64);
    let mut banks_done: HashSet<String> = HashSet::new();
    loop {
        let job = {
            let Ok(mut wanted) = lock.lock() else {
                return;
            };
            wanted.running = true;
            loop {
                if wanted.generation != done {
                    break MapJob::Pick(wanted.map.clone(), wanted.generation);
                }
                // (Not while a match loads or plays: its end lets them run.)
                if wanted.next_generation != next_done && !stopped() {
                    next_done = wanted.next_generation;
                    break MapJob::Next(wanted.next.clone(), wanted.generation);
                }
                wanted = match wake.wait(wanted) {
                    Ok(w) => w,
                    Err(_) => return,
                };
            }
        };
        match job {
            MapJob::Pick(map, generation) => {
                done = generation;
                if pick_on() {
                    let newer = || lock.lock().map_or(true, |w| w.generation != generation);
                    let since = std::time::Instant::now();
                    while since.elapsed() < PICK_DWELL && !newer() {
                        std::thread::sleep(std::time::Duration::from_millis(25));
                    }
                    if newer() {
                        continue;
                    }
                }
                // (The common set the match asks for, made now with his
                // classes' guest guns.)
                crate::session_load::prefetch_lobby_common();
                // A newer pick stops this one: the head moves to the map he wants now.
                let stale = || stopped() || lock.lock().map_or(true, |w| w.generation != generation);
                let started = std::time::Instant::now();
                let line = warm_map(zone_all, &map, warmed, &mut banks_done, &stale);
                diag::info!(Launch, "t6 map prewarm: {map}: {line} in {:.1} s", started.elapsed().as_secs_f32());
            }
            MapJob::Next(maps, generation) => {
                let stale = || stopped() || lock.lock().map_or(true, |w| w.generation != generation);
                for map in &maps {
                    if stale() {
                        break;
                    }
                    let started = std::time::Instant::now();
                    let line = warm_map(zone_all, map, warmed, &mut banks_done, &stale);
                    diag::info!(Launch, "t6 next-map prewarm: {map}: {line} in {:.1} s", started.elapsed().as_secs_f32());
                }
            }
        }
    }
}

fn warm_map(
    zone_all: &Path,
    map: &str,
    warmed: &mut HashSet<(usize, u32, u32)>,
    banks_done: &mut HashSet<String>,
    stale: &dyn Fn() -> bool,
) -> String {
    let path = zone_all.join(format!("{map}.ff"));
    if !path.is_file() {
        return "no Black Ops II zone: another game's map".into();
    }
    let packs = match PackSet::open_dir(zone_all) {
        Ok(p) => p,
        Err(e) => return format!("no packs: {e}"),
    };
    // Reading the zone also leaves it in the cache for the load.
    let capture = match asset_t6::capture_zone(&path) {
        Ok(c) => c,
        Err(e) => return format!("zone not read: {e}"),
    };
    let mut entries: Vec<(usize, ipak_t6::IndexEntry)> = capture
        .images
        .iter()
        .filter_map(|image| match packs.locate(image) {
            Some(asset_t6::ImageSource::Pack(i, e)) => Some((i, e)),
            _ => None,
        })
        .filter(|&(i, e)| !warmed.contains(&(i, e.offset, e.size)))
        .collect();
    let count = sorted_once(&mut entries);
    let mut bytes = 0usize;
    for (done, chunk) in warm_chunks(&entries) {
        if stale() {
            return format!("stopped for a newer pick after {done} pictures");
        }
        bytes += warm_chunk(&packs, chunk, warmed);
    }
    let mut bases: Vec<String> = capture
        .sound_banks
        .iter()
        .map(|b| b.name.rsplit_once('.').map_or(b.name.as_str(), |(base, _)| base).to_owned())
        .filter(|b| !b.is_empty() && !banks_done.contains(b))
        .collect();
    bases.sort();
    bases.dedup();
    let sound = sound_dir(zone_all).map_or_else(String::new, |dir| warm_banks(&dir, &bases, stale));
    banks_done.extend(bases);
    format!("{count} pictures, {} MB read, {sound}", bytes >> 20)
}

/// BO2's `sound` folder, beside `zone`.
fn sound_dir(zone_all: &Path) -> Option<std::path::PathBuf> {
    let dir = zone_all.parent()?.parent()?.join("sound");
    dir.is_dir().then_some(dir)
}

/// Read every `<base>.*` file in `dir` (its loaded and streamed banks, every
/// language) and drop the bytes, so opening the banks and streaming their
/// sounds come from the file cache.
fn warm_banks(dir: &Path, bases: &[String], stale: &dyn Fn() -> bool) -> String {
    let Ok(read) = std::fs::read_dir(dir) else {
        return "no sound folder".into();
    };
    let bases: Vec<String> = bases.iter().map(|b| format!("{}.", b.to_ascii_lowercase())).collect();
    let mut files: Vec<std::path::PathBuf> = read
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| bases.iter().any(|b| n.to_ascii_lowercase().starts_with(b.as_str())))
        })
        .collect();
    files.sort();
    let mut bytes = 0u64;
    for path in &files {
        if stale() {
            break;
        }
        if let Ok(mut file) = std::fs::File::open(path) {
            bytes += std::io::copy(&mut file, &mut std::io::sink()).unwrap_or(0);
        }
    }
    format!("{} sound files, {} MB read", files.len(), bytes >> 20)
}

fn prewarm(zone_all: &Path, warmed: &mut HashSet<(usize, u32, u32)>) -> String {
    // The zones every multiplayer map loads, then the factions (small, and
    // each map names two of them).
    let mut zones: Vec<std::path::PathBuf> =
        super::t6_m2::T6_MP_PRE_ZONES.iter().map(|z| zone_all.join(format!("{z}.ff"))).collect();
    let mut factions: Vec<std::path::PathBuf> = std::fs::read_dir(zone_all)
        .map(|d| {
            d.filter_map(|e| e.ok().map(|e| e.path()))
                .filter(|p| {
                    p.file_name()
                        .and_then(|n| n.to_str())
                        .is_some_and(|n| n.starts_with("faction_") && n.ends_with("_mp.ff"))
                })
                .collect()
        })
        .unwrap_or_default();
    factions.sort();
    zones.extend(factions);
    let packs = match PackSet::open_dir(zone_all) {
        Ok(p) => p,
        Err(e) => return format!("no packs: {e}"),
    };
    let mut entries: Vec<(usize, ipak_t6::IndexEntry)> = Vec::new();
    for path in &zones {
        let Ok(capture) = asset_t6::capture_zone(path) else {
            continue;
        };
        for image in &capture.images {
            if let Some(asset_t6::ImageSource::Pack(i, e)) = packs.locate(image) {
                entries.push((i, e));
            }
        }
    }
    let count = sorted_once(&mut entries);
    let mut bytes = 0usize;
    for (done, chunk) in warm_chunks(&entries) {
        if stopped() {
            return format!("stopped for the match after {done} of {count} pictures, {} MB read", bytes >> 20);
        }
        bytes += warm_chunk(&packs, chunk, warmed);
    }
    format!("{count} pictures from {} zones, {} MB read", zones.len(), bytes >> 20)
}

/// Each entry once, in disk order within a pack (the head sweeps instead of
/// seeking). Returns how many.
fn sorted_once(entries: &mut Vec<(usize, ipak_t6::IndexEntry)>) -> usize {
    entries.sort_unstable_by_key(|&(i, e)| (i, e.offset, e.size, e.key));
    entries.dedup_by_key(|&mut (i, e)| (i, e.offset, e.size));
    entries.len()
}

/// Pictures warmed between checks for a stop.
const WARM_CHUNK: usize = 64;

/// The entries in chunks of one pack each, with how many came before.
fn warm_chunks(
    entries: &[(usize, ipak_t6::IndexEntry)],
) -> impl Iterator<Item = (usize, &[(usize, ipak_t6::IndexEntry)])> {
    let mut done = 0;
    std::iter::from_fn(move || {
        let rest = &entries[done..];
        let first = rest.first()?.0;
        let n = rest.iter().take(WARM_CHUNK).take_while(|&&(i, _)| i == first).count();
        let at = done;
        done += n;
        Some((at, &rest[..n]))
    })
}

/// lane B (B3): warms one chunk (one pack) in a few long reads, under each
/// entry's real key: the warm copies were kept under key 0, which no load
/// ever asked for, so the load read every picture from the pack again.
fn warm_chunk(
    packs: &PackSet,
    chunk: &[(usize, ipak_t6::IndexEntry)],
    warmed: &mut HashSet<(usize, u32, u32)>,
) -> usize {
    let Some(&(i, _)) = chunk.first() else {
        return 0;
    };
    let entries: Vec<_> = chunk.iter().map(|&(_, e)| e).collect();
    warmed.extend(entries.iter().map(|e| (i, e.offset, e.size)));
    packs.packs[i].warm_many(&entries)
}
