//! bo2mp: Modern Warfare III's (2023) Warzone HUD pictures, read from the
//! owner's install at runtime for the Warzone HUD (`hud_warzone`). Nothing
//! of the game is copied or kept on disk.
//!
//! The pictures' names are hashes in the game's zones (its UI scripts that
//! name and place them are compiled and are not read): each one here was
//! picked by eye from contact sheets of the in-game zones' pictures
//! (`fastfile_jup` example `jup_hud_pics`) and is kept by its zone and name
//! hash. They are white (or grey) glyphs drawn tinted in the Warzone HUD's
//! colours (`shell::wz_theme::HUD`); one the install lacks is drawn as a
//! plain shape instead.
//!
//! The install is the emblems' (`jup_cards::install`): `$BO2_JUP_INSTALL`,
//! else the game folders file's `jup` line.

use std::collections::HashMap;
use std::sync::OnceLock;

use fastfile_jup::map::materials::{MapImage, Packages};

/// The zones the pictures are in.
const ZONES: &[&str] = &["ingame", "ingame_br", "ingame_shared_mp", "ingame_base_mp"];

/// The largest side a picture is decoded at.
const MAX_SIDE: u32 = 256;

/// A HUD picture: our name for it, its zone (in `ZONES`) and its name
/// hash there. (What each one shows, as seen on the contact sheets.)
pub(crate) const PICTURES: &[(&str, &str, u64)] = &[
    // Plates, health, armor.
    ("plate", "ingame_shared_mp", 0x7152_b586_6253_24d7),           // a ballistic plate, flat white
    ("plate_carrier", "ingame_br", 0x6ee9_d5b2_3804_6e98),          // a vest in a hexagon
    ("plate_bar", "ingame_base_mp", 0x0924_1024_ad50_aba2),         // three bar segments
    ("armor_break", "ingame_base_mp", 0x0606_294e_7363_0ca3),       // a plate breaking
    ("armor_hit_glow", "ingame_base_mp", 0x3cf4_6547_8f40_8543),    // a soft glow
    ("self_revive", "ingame_br", 0x68bd_d0ba_9aa6_8cbf),            // a medic cross in a hexagon
    ("revive", "ingame_base_mp", 0x148c_50fa_6ab2_213e),            // a heart in a map pin
    // Cash and buying.
    ("cash", "ingame_shared_mp", 0x4891_1602_9e8c_451b),            // "$"
    ("cash_chrome", "ingame_br", 0x7cd4_c6a4_8c93_bde3),            // a chrome "$"
    ("money_bag", "ingame_shared_mp", 0x176e_9c4c_108f_b360),
    ("buy_station", "ingame", 0x3f78_9aac_2253_591c),               // "$" on a screen
    ("buy_cart", "ingame_br", 0x44e3_b151_7243_a342),
    ("loadout_drop", "ingame_br", 0x56cc_82a0_51ed_e86c),           // parachutes
    ("loadout_crate", "ingame", 0x3d31_9c51_c3e3_80ce),             // "$" and guns in a box
    ("uav", "ingame_shared_mp", 0x301b_2945_faa4_1b06),             // a dish
    // The squad.
    ("squad", "ingame_br", 0x22cb_b629_d6fb_24b4),                  // three people
    ("players", "ingame_base_mp", 0x411f_f6de_971b_3e50),           // one person in a circle
    ("downed", "ingame", 0x2ed0_0f76_22f8_1893),                    // a man lying down
    ("skull", "ingame_shared_mp", 0x4162_5dbb_9df4_d954),
    ("eliminated", "ingame_br", 0x35a6_bde2_cd52_66d2),             // skull and scythes
    ("redeploy", "ingame_shared_mp", 0x51bf_5736_3efe_3898),        // an arrow up out of a star
    ("redeploy_shield", "ingame_br", 0x43c3_48a9_0478_d125),
    ("kill_marker", "ingame_shared_mp", 0x5207_c773_b427_5cdb),     // a skull over a chevron
    // The circle and the gas.
    ("gas", "ingame_br", 0x6345_f610_3e5e_2c63),                    // a skull in a hazard sign
    ("gas_mask", "ingame_br", 0x7d19_a6d6_24a9_4b32),
    ("circle_timer", "ingame_base_mp", 0x1788_9934_64ad_41fb),      // a ring in segments
    ("ring", "ingame_shared_mp", 0x55b7_11df_3ff1_a4bf),
    ("arc", "ingame_shared_mp", 0x293a_1291_f471_6355),             // a quarter ring
    ("clock_wipe", "ingame_br", 0x4a90_9c8d_ceab_3438),
    ("dashed_ring", "ingame_br", 0x028a_f3c5_1bba_7daa),
    ("plane", "ingame_shared_mp", 0x3458_941e_ccdc_3a54),           // the infil plane
    // Contracts.
    ("contract", "ingame", 0x163a_1a0e_2d6d_8e2a),                  // the tablet
    ("contract_bounty", "ingame_br", 0x1c2a_89f1_86f5_358d),        // crosshair on a shield
    ("contract_recon", "ingame_br", 0x3f26_4810_b0fd_f3dc),         // magnifier on a shield
    ("contract_supply", "ingame_br", 0x072c_e8f2_b3c8_d17c),        // stopwatch on a shield
    ("contract_mostwanted", "ingame_br", 0x6116_1b22_0587_80bf),    // crown on a shield
    ("contract_scavenger", "ingame", 0x7c30_1f12_4202_839b),        // a "?" box
    // Pings, markers, hits.
    ("ping", "ingame_br", 0x3f9d_0c15_5cb1_2227),                   // a white flag on a ring
    ("ping_danger", "ingame_shared_mp", 0x3039_9ec5_df0c_22ed),     // a red diamond
    ("player_arrow", "ingame_br", 0x3242_ca65_a1a9_773f),
    ("hitmarker", "ingame_base_mp", 0x3429_ec5c_987c_3664),         // an X with ends
    ("hitmarker_kill", "ingame_shared_mp", 0x23ba_5104_1bf9_db30),  // four corner strokes
    // Ammo.
    ("ammo_rifle", "ingame_shared_mp", 0x01b4_9bdc_91cc_aab9),      // four rounds
    ("ammo_shells", "ingame_shared_mp", 0x1262_5fee_24b4_8bfd),
    ("ammo_pistol", "ingame_shared_mp", 0x2bb6_c1d6_4113_7f88),
    ("ammo_sniper", "ingame_shared_mp", 0x75aa_3799_54b6_b1a4),     // one round
];

/// Every picture, decoded once (`None`: no install, or not there).
fn decoded() -> &'static HashMap<&'static str, (u32, u32, Vec<u8>)> {
    static ALL: OnceLock<HashMap<&'static str, (u32, u32, Vec<u8>)>> = OnceLock::new();
    ALL.get_or_init(|| {
        let t = std::time::Instant::now();
        let out = read_all().unwrap_or_default();
        diag::info!(
            Ui,
            "bo2mp Warzone HUD: {} of {} Modern Warfare III pictures read ({:?})",
            out.len(),
            PICTURES.len(),
            t.elapsed()
        );
        out
    })
}

fn read_all() -> Option<HashMap<&'static str, (u32, u32, Vec<u8>)>> {
    let install = crate::jup_cards::install()?;
    let oodle = match fastfile_jup::oodle::load(&install) {
        Ok(o) => o,
        Err(e) => {
            diag::warn!(Ui, "bo2mp Warzone HUD: no Oodle ({e}); simple shapes stand in");
            return None;
        }
    };
    let packages = Packages::open(&install);
    let mut out = HashMap::new();
    for zone_name in ZONES {
        let wanted: Vec<&(&str, &str, u64)> = PICTURES.iter().filter(|p| p.1 == *zone_name).collect();
        if wanted.is_empty() {
            continue;
        }
        let path = install.join("zone").join(format!("{zone_name}.ff"));
        let zone = match fastfile_jup::read_zone(&path, oodle.as_ref()) {
            Ok((_, z)) => z,
            Err(e) => {
                diag::warn!(Ui, "bo2mp Warzone HUD: {zone_name}.ff not read: {e}");
                continue;
            }
        };
        let index = fastfile_jup::zone::HashIndex::build(&zone, wanted.iter().map(|p| p.2), 8);
        for &&(name, _, hash) in &wanted {
            let rec = index.plain(hash).iter().find_map(|&at| match fastfile_jup::image::parse(&zone, at) {
                Ok(Some(r)) if r.hash == hash && r.depth == 1 && r.array == 1 => Some(r),
                _ => None,
            });
            let Some(rec) = rec else {
                diag::warn!(Ui, "bo2mp Warzone HUD: `{name}` ({hash:016x}) is not in {zone_name}");
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
            let pic = packages.level(&img, MAX_SIDE, oodle.as_ref()).ok().and_then(|l| {
                let px = fastfile_jup::cards::decode_bcn(l.dxgi, l.width, l.height, &l.data)?;
                Some((l.width, l.height, px))
            });
            match pic {
                Some(p) => {
                    out.insert(name, p);
                }
                None => diag::warn!(Ui, "bo2mp Warzone HUD: `{name}`'s pixels are not in the install"),
            }
        }
    }
    Some(out)
}

/// The first of `names` (pictures' names, hashed as the game hashes them)
/// that `zone_name` holds, as RGBA at most `max_side` on a side: the map
/// pictures (`loading_mp_jup_<map>_bg`, the menus' key art). Reads the
/// zone: call it off the main thread. Which name was read, or why none was.
pub(crate) fn read_named_report(zone_name: &str, names: &[&str], max_side: u32) -> Result<(String, (u32, u32, Vec<u8>)), String> {
    let install = crate::jup_cards::install().ok_or("no Modern Warfare III install")?;
    let oodle = fastfile_jup::oodle::load(&install).map_err(|e| format!("oodle: {e}"))?;
    let path = install.join("zone").join(format!("{zone_name}.ff"));
    if !path.is_file() {
        return Err(format!("no {zone_name}.ff"));
    }
    let zone = fastfile_jup::read_zone(&path, oodle.as_ref()).map(|(_, z)| z).map_err(|e| format!("{zone_name}.ff not read: {e}"))?;
    let hashes: Vec<u64> = names.iter().map(|n| fastfile_jup::hash::name(n)).collect();
    let index = fastfile_jup::zone::HashIndex::build(&zone, hashes.iter().copied(), 8);
    let packages = Packages::open(&install);
    let mut why = Vec::new();
    for (name, &hash) in names.iter().zip(&hashes) {
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
        let pic = packages.level(&img, max_side, oodle.as_ref()).map_err(|e| e.to_string()).and_then(|l| {
            let px = fastfile_jup::cards::decode_bcn(l.dxgi, l.width, l.height, &l.data).ok_or_else(|| format!("format {} not decoded", l.dxgi))?;
            Ok((l.width, l.height, px))
        });
        match pic {
            Ok(p) => {
                diag::info!(Ui, "bo2mp: Modern Warfare III picture `{name}` from {zone_name} ({}x{})", p.0, p.1);
                return Ok(((*name).to_owned(), p));
            }
            Err(e) => why.push(format!("`{name}` in {zone_name}, its pixels not in the install ({e})")),
        }
    }
    Err(if why.is_empty() { format!("none of {} names in {zone_name}", names.len()) } else { why.join("; ") })
}

/// The played guns' Create-a-Class pictures (`icon_weapon_jup_<gun>_mp`,
/// 1024x512 BC7 in `global_stream_mp`, streamed), read together the first
/// time one is asked for: the zone is large, so it is inflated once.
fn gun_icons() -> &'static HashMap<String, (u32, u32, Vec<u8>)> {
    static ICONS: OnceLock<HashMap<String, (u32, u32, Vec<u8>)>> = OnceLock::new();
    ICONS.get_or_init(read_gun_icons)
}

/// bo2mp: reads the gun pictures again (for a small copy of the install,
/// whose recorder notes what is read: `jup_pack`). How many were read.
pub(crate) fn touch_gun_icons() -> usize {
    read_gun_icons().len()
}

/// The zones the guns' pictures are looked for in: `global_stream_mp`, then
/// any other `global*` zone beside it (a later season's guns may keep their
/// pictures in a zone of their own), smaller than 400 MB.
fn picture_zones(install: &std::path::Path) -> Vec<std::path::PathBuf> {
    let dir = install.join("zone");
    let mut v = vec![dir.join("global_stream_mp.ff")];
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
    v.extend(more);
    v
}

/// bo2mp: the guns' and camos' pictures of an exported folder
/// (`CodLegacy export-jup`, `icons/`), by name.
fn exported_icons(dir: &std::path::Path) -> HashMap<String, (u32, u32, Vec<u8>)> {
    let mut out = HashMap::new();
    let Some(index) = asset_transport::jup_export::PackIndex::open(&dir.join(asset_transport::jup_export::ICONS_DIR), asset_transport::jup_export::ICONS_DIR) else {
        return out;
    };
    for name in index.names() {
        let Some(bytes) = index.get(name) else { continue };
        if let Ok(img) = image::load_from_memory_with_format(&bytes, image::ImageFormat::Png) {
            let img = img.into_rgba8();
            out.insert(name.clone(), (img.width(), img.height(), img.into_raw()));
        }
    }
    diag::info!(Ui, "bo2mp: Modern Warfare III gun pictures: {} from the export {}", out.len(), dir.display());
    out
}

fn read_gun_icons() -> HashMap<String, (u32, u32, Vec<u8>)> {
    let mut out = HashMap::new();
    // (Without the game: the pictures exported from it.)
    let Some(install) = crate::jup_cards::install().filter(|i| !asset_transport::jup_export::is_export(i)) else {
        return asset_transport::jup_export::dir().map_or(out, |d| exported_icons(&d));
    };
    let Ok(oodle) = fastfile_jup::oodle::load(&install) else {
        return out;
    };
    let names: Vec<String> = bo2_profile::jup_names::JUP_PLAYED
        .iter()
        .flat_map(|g| bo2_profile::jup_names::icon_names(g))
        // (And the camos' Create-a-Class pictures, Modern Warfare III's
        // and II's: `asset_game::weapon_camo_models`.)
        .chain(
            asset_game::weapon_camo_models::JUP_CAMOS
                .iter()
                .chain(asset_game::weapon_camo_models::IW9_CAMOS.iter())
                .map(|n| (*n).to_owned()),
        )
        .collect();
    let hashes: Vec<u64> = names.iter().map(|n| fastfile_jup::hash::name(n)).collect();
    let packages = Packages::shared(&install);
    // (Per name: why it is not read, for the log - a picture the zone does
    // not have, or one whose pixels did not decode.)
    let mut why: HashMap<&str, &'static str> = HashMap::new();
    for path in picture_zones(&install) {
        let zone = match fastfile_jup::read_zone(&path, oodle.as_ref()) {
            Ok((_, z)) => z,
            Err(e) => {
                diag::warn!(Ui, "bo2mp: Modern Warfare III's gun pictures: {} not read: {e}", path.display());
                continue;
            }
        };
        let index = fastfile_jup::zone::HashIndex::build(&zone, hashes.iter().copied(), 8);
        for (name, &hash) in names.iter().zip(&hashes) {
            if out.contains_key(name) {
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
            let level = packages.level(&img, 512, oodle.as_ref());
            match level {
                Ok(l) => match fastfile_jup::cards::decode_bcn(l.dxgi, l.width, l.height, &l.data) {
                    Some(px) => {
                        out.insert(name.clone(), (l.width, l.height, px));
                    }
                    None => {
                        why.insert(name, "format not decoded");
                    }
                },
                Err(_) => {
                    why.insert(name, "pixels not in the install's packages");
                }
            }
        }
    }
    // (Which guns have no full-colour picture: logged once.)
    for g in bo2_profile::jup_names::JUP_PLAYED {
        let candidates = bo2_profile::jup_names::icon_names(g);
        let colour = candidates.iter().find(|n| !bo2_profile::jup_names::is_silhouette(n) && out.contains_key(*n));
        if colour.is_none() {
            let hud = candidates.iter().any(|n| out.contains_key(n));
            let reasons: Vec<String> = candidates.iter().filter_map(|n| why.get(n.as_str()).map(|w| format!("{n}: {w}"))).collect();
            diag::warn!(Ui, "bo2mp: Modern Warfare III gun `{g}` has no colour picture ({}){}", if hud { "kill icon only" } else { "none at all" }, if reasons.is_empty() { String::new() } else { format!("; {}", reasons.join(", ")) });
        }
    }
    diag::info!(Ui, "bo2mp: Modern Warfare III gun pictures: {} of {} read", out.len(), names.len());
    out
}

/// A played gun's Create-a-Class picture (the first of `names` read).
pub(crate) fn gun_picture(names: &[String]) -> Option<(u32, u32, Vec<u8>)> {
    let icons = gun_icons();
    names.iter().find_map(|n| icons.get(n).cloned())
}

/// A map's picture (`loadscreen_`/`preview_` + the zone name, as the menus
/// ask): its loading screen's background, else the menus' key art.
pub(crate) fn map_picture(names: &[String]) -> Option<(u32, u32, Vec<u8>)> {
    let stem = names.iter().find_map(|n| {
        n.strip_prefix("loadscreen_").or_else(|| n.strip_prefix("preview_")).or_else(|| n.strip_suffix("_load"))
    })?;
    match map_picture_report(stem) {
        Ok((name, minimap, p)) => {
            diag::info!(Ui, "mw3 map picture: {stem}: {name}");
            Some(if minimap { crate::foreign_maps::minimap_card(p) } else { p })
        }
        Err(e) => {
            diag::warn!(Ui, "mw3 map picture: {stem}: {e}; no picture");
            None
        }
    }
}

/// The names a Modern Warfare III map's pictures may go by: its zone's
/// (`afghan_v2`), and the map's own name without the zone's suffix
/// (`afghan`, `shoothouse`, `farm18`), or as Modern Warfare II named it
/// (`mp_shipment_iw9`).
fn jup_name_variants(map: &str) -> Vec<String> {
    let mut v = vec![map.to_owned()];
    let mut push = |s: String| {
        if !s.is_empty() && !v.contains(&s) {
            v.push(s);
        }
    };
    push(map.trim_end_matches("_v2").to_owned());
    push(map.trim_end_matches("_iw9").to_owned());
    push(map.trim_end_matches(|c: char| c.is_ascii_digit()).trim_end_matches('_').to_owned());
    push(map.replace('_', ""));
    push(format!("{}_iw9", map.trim_end_matches("_iw9")));
    // (The install's map table names some loading screens otherwise:
    // `loading_mp_shoot_bg` is Shoot House's, `loading_mp_jup_tokyo_bg`
    // Tokyo's.)
    match map {
        "shoothouse2" => push("shoot".to_owned()),
        "elevated" => push("tokyo".to_owned()),
        _ => {}
    }
    v
}

/// bo2mp: a Modern Warfare III map's picture: its loading screen
/// (`loading_mp_jup_<map>_bg` and its other names, code_post_gfx then
/// global), the menus' key art, else its minimap from its own zone
/// (`true`: to be put on a card). Which name was read, or why each failed.
pub(crate) fn map_picture_report(stem: &str) -> Result<(String, bool, (u32, u32, Vec<u8>)), String> {
    // (A Modern Warfare II (2022) map the install carries is a `mp_jup_`
    // zone too: `asset_transport::MWII_MAPS`.)
    let map = stem.strip_prefix("mp_jup_").ok_or("not a mp_jup_ map")?;
    let vars = jup_name_variants(map);
    let mut loading: Vec<String> = Vec::new();
    for v in &vars {
        for n in [format!("loading_mp_jup_{v}_bg"), format!("loading_mp_{v}_bg"), format!("loadscreen_mp_jup_{v}"), format!("loadscreen_mp_{v}")] {
            if !loading.contains(&n) {
                loading.push(n);
            }
        }
    }
    let art: &[&str] = match map {
        "escape5" => &[
            "jup_menu_keyart_rebirth_resurgence_quads",
            "jup_menu_keyart_rebirth_resurgence_trios",
            "jup_menu_keyart_rebirth_resurgence_duos",
            "jup_menu_keyart_rebirth_resurgence_solos",
        ],
        "elevated" => &["jup_ui_menu_keyart_elevated_00", "jup_menu_backplate_elevated", "jup_ui_menu_keyart_elevated_button"],
        _ => &[],
    };
    let refs: Vec<&str> = loading.iter().map(String::as_str).collect();
    let mut why = Vec::new();
    match read_named_report("code_post_gfx", &refs, 2048) {
        Ok((n, p)) => return Ok((format!("`{n}` (code_post_gfx)"), false, p)),
        Err(e) => why.push(e),
    }
    let mut global = refs.clone();
    global.extend_from_slice(art);
    match read_named_report("global", &global, 2048) {
        Ok((n, p)) => return Ok((format!("`{n}` (global)"), false, p)),
        Err(e) => why.push(e),
    }
    let mut compass: Vec<String> = Vec::new();
    for v in &vars {
        for n in [format!("compass_map_mp_jup_{v}"), format!("compass_map_mp_{v}"), format!("minimap_mp_jup_{v}"), format!("minimap_mp_{v}")] {
            if !compass.contains(&n) {
                compass.push(n);
            }
        }
    }
    let mut own: Vec<&str> = refs.clone();
    own.extend(compass.iter().map(String::as_str));
    match read_named_report(stem, &own, 2048) {
        Ok((n, p)) => {
            let minimap = compass.contains(&n);
            return Ok((format!("`{n}` ({stem}{})", if minimap { ", its minimap" } else { "" }), minimap, p));
        }
        Err(e) => why.push(e),
    }
    Err(format!("tried {} loading names ({}, ...) and {} minimap names: {}", loading.len(), loading[0], compass.len(), why.join("; ")))
}

/// A picture by our name, as RGBA. Reads the install the first time: call
/// it off the main thread.
pub(crate) fn read_picture(name: &str) -> Option<(u32, u32, Vec<u8>)> {
    decoded().get(name).cloned()
}

#[cfg(test)]
mod tests {
    #[test]
    fn names_are_unique_and_zones_known() {
        let mut seen = std::collections::HashSet::new();
        for (name, zone, hash) in super::PICTURES {
            assert!(seen.insert(*name), "{name} twice");
            assert!(super::ZONES.contains(zone), "{name}: {zone}");
            assert_eq!(hash >> 63, 0, "{name}: a name hash has its top bit clear");
        }
    }
}
