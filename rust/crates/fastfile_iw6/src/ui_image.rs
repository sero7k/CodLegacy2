//! Ghosts' 2-D pictures (Create-a-Class weapon and attachment pictures,
//! kill icons) by material name, as RGBA, from `zone/ui_mp.ff` (and
//! `common_core_mp.ff` for what ui_mp lacks).
//!
//! The first call walks the zones once and keeps only the top level of
//! the inline pictures of 2-D materials whose names start with one of
//! `KEPT` (a few MB); later calls decode from that.

use std::collections::HashMap;
use std::path::Path;
use std::sync::{Mutex, OnceLock};

use crate::image::{dxgi_unit, level_bytes};

/// Material name prefixes kept.
const KEPT: &[&str] = &["weapon_", "hud_icon_", "scope_", "mod_", "hud_"];

/// The attachment pictures (`<prefix>_360`) besides `KEPT`'s.
const ATTACHMENT_PICTURES: &[&str] = &[
    "armor_piercing",
    "burst_fire",
    "full_auto",
    "single_fire",
    "extended_mags",
    "flash_suppressor",
    "iron_sight",
    "muzzle_break",
    "rapid_fire",
    "slug_round",
    "chrome_lined",
    "titanium_frame",
];

#[derive(Clone)]
struct Picture {
    width: u32,
    height: u32,
    format: u32,
    data: Vec<u8>,
}

type Pictures = HashMap<String, Picture>;

fn store() -> &'static Mutex<HashMap<std::path::PathBuf, Pictures>> {
    static STORE: OnceLock<Mutex<HashMap<std::path::PathBuf, Pictures>>> = OnceLock::new();
    STORE.get_or_init(Default::default)
}

fn kept(name: &str) -> bool {
    KEPT.iter().any(|p| name.starts_with(p))
        || ATTACHMENT_PICTURES.iter().any(|p| name.starts_with(p))
}

fn read_pictures(zone_dir: &Path) -> Pictures {
    let mut out = Pictures::new();
    for zone in ["ui_mp", "common_core_mp"] {
        let Ok(z) = crate::Zone::open(&zone_dir.join(format!("{zone}.ff"))) else {
            continue;
        };
        let (cap, _) = crate::walk::walk_zone(&z);
        for m in &cap.materials {
            let name = m.name.trim_start_matches(',');
            if out.contains_key(name) || !kept(name) {
                continue;
            }
            let Some(img) = m
                .textures
                .iter()
                .find(|t| t.semantic == 0 || t.semantic == 2)
                .and_then(|t| t.image)
            else {
                continue;
            };
            let rec = &cap.images[img.index as usize];
            if rec.streamed {
                continue;
            }
            let (w, h) = (u32::from(rec.width), u32::from(rec.height));
            let Some(len) = level_bytes(rec.format, w, h) else {
                continue;
            };
            let Some(bytes) = rec
                .pixels
                .map(|p| p.get(&z.image))
                .and_then(|b| b.get(..len))
            else {
                continue;
            };
            out.insert(
                name.to_owned(),
                Picture {
                    width: w,
                    height: h,
                    format: rec.format,
                    data: bytes.to_vec(),
                },
            );
        }
    }
    out
}

/// The first of `names` the install's 2-D pictures have, as RGBA.
pub fn read_ui_image(install: &Path, names: &[String]) -> Option<(u32, u32, Vec<u8>)> {
    let zone_dir = install.join("zone");
    let mut guard = store().lock().ok()?;
    let pictures = guard
        .entry(zone_dir.clone())
        .or_insert_with(|| read_pictures(&zone_dir));
    names
        .iter()
        .find_map(|n| pictures.get(n.as_str()))
        .and_then(decode_rgba)
}

/// bo2mp: the menu zones searched for a map's picture.
const MAP_PICTURE_ZONES: &[&str] = &["patch_ui_mp", "ui_mp", "patch_common_core_mp", "common_core_mp", "patch_common_mp"];

/// Could `name` be a map's menu picture (a preview or loading screen)?
fn is_map_picture_name(name: &str) -> bool {
    !name.starts_with("compass_")
        && (name.contains("loadscreen")
            || name.contains("preview")
            || name.contains("map_select")
            || name.ends_with("_load")
            || name.starts_with("mp_"))
}

/// A picture's largest level at most `max` on a side (inline, or streamed
/// from `imagefile<N>.pak`).
fn picture_of(rec: &crate::walk::ImageRec, zone_image: &[u8], paks: &mut crate::image::ImagePaks, max: u32) -> Result<Picture, String> {
    let px = crate::image::image_pixels(rec, zone_image, paks, max)?;
    let len = level_bytes(px.dxgi_format, px.width, px.height).ok_or_else(|| format!("DXGI format {} is not read", px.dxgi_format))?;
    let data = px.data.get(..len).ok_or("its top level is cut short")?.to_vec();
    Ok(Picture { width: px.width, height: px.height, format: px.dxgi_format, data })
}

type MapPictures = HashMap<String, Result<Picture, String>>;

/// Every map-like picture of the menu zones (by material and by image
/// name), with the zones that were not read.
fn menu_map_pictures(zone_dir: &Path) -> (MapPictures, Vec<String>) {
    let mut out = MapPictures::new();
    let mut unread = Vec::new();
    let mut paks = crate::image::ImagePaks::new(zone_dir);
    for zone in MAP_PICTURE_ZONES {
        let path = zone_dir.join(format!("{zone}.ff"));
        if !path.is_file() {
            continue;
        }
        let z = match crate::Zone::open(&path) {
            Ok(z) => z,
            Err(e) => {
                unread.push(format!("{zone}: {e}"));
                continue;
            }
        };
        let Ok((cap, _)) = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| crate::walk::walk_zone(&z))) else {
            unread.push(format!("{zone}: its walk failed"));
            continue;
        };
        let mut add = |name: String, img: usize, out: &mut MapPictures| {
            if out.contains_key(&name) {
                return;
            }
            let Some(rec) = cap.images.get(img) else { return };
            out.insert(name, picture_of(rec, &z.image, &mut paks, 1024).map_err(|e| format!("{e} ({zone})")));
        };
        for m in &cap.materials {
            let name = m.name.trim_start_matches(',').to_ascii_lowercase();
            if !is_map_picture_name(&name) {
                continue;
            }
            if let Some(img) = m.textures.iter().find(|t| t.semantic == 0 || t.semantic == 2).and_then(|t| t.image) {
                add(name, img.index as usize, &mut out);
            }
        }
        for (i, rec) in cap.images.iter().enumerate() {
            let name = rec.name.trim_start_matches(',').to_ascii_lowercase();
            if is_map_picture_name(&name) {
                add(name, i, &mut out);
            }
        }
    }
    (out, unread)
}

/// bo2mp: a Ghosts map's menu picture, else its loading screen (its
/// `<map>_load` zone), else its minimap (`compass_map_<map>` in its own
/// zone, returned as found: the caller puts it on a card). The picture's
/// name and where it was found, or why each step failed.
pub fn read_map_picture(install: &Path, stem: &str) -> Result<(String, bool, (u32, u32, Vec<u8>)), String> {
    static MENU: OnceLock<Mutex<HashMap<std::path::PathBuf, (MapPictures, Vec<String>)>>> = OnceLock::new();
    let zone_dir = install.join("zone");
    let short = stem.strip_prefix("mp_").unwrap_or(stem);
    let wanted = [
        format!("loadscreen_{stem}"),
        format!("preview_{stem}"),
        format!("{stem}_load"),
        format!("{stem}_preview"),
        format!("mp_map_select_{short}"),
        format!("map_select_{short}"),
    ];
    let mut why = Vec::new();
    {
        let mut guard = MENU.get_or_init(Default::default).lock().map_err(|_| "menu pictures poisoned")?;
        let (pics, unread) = guard.entry(zone_dir.clone()).or_insert_with(|| menu_map_pictures(&zone_dir));
        // The exact names, then any map picture naming the map.
        let fuzzy = |n: &str| n.split(|c: char| !c.is_ascii_alphanumeric()).any(|w| w == short) || n.contains(stem);
        let mut names: Vec<&String> = wanted.iter().filter(|n| pics.contains_key(n.as_str())).collect();
        let mut more: Vec<&String> = pics.keys().filter(|n| fuzzy(n) && !names.contains(n)).collect();
        more.sort();
        names.extend(more);
        for n in &names {
            match &pics[n.as_str()] {
                Ok(p) => {
                    if let Some(rgba) = decode_rgba(p) {
                        return Ok((format!("`{n}` (menu zones)"), false, rgba));
                    }
                    why.push(format!("`{n}`: DXGI format {} not decoded", p.format));
                }
                Err(e) => why.push(format!("`{n}`: {e}")),
            }
        }
        if names.is_empty() {
            let mut sample: Vec<&String> = pics.keys().collect();
            sample.sort();
            why.push(format!(
                "none of `{}` nor any map picture naming `{short}` in the menu zones ({} map-like pictures there{}{})",
                wanted[..3].join("`, `"),
                pics.len(),
                if sample.is_empty() { String::new() } else { format!(": {}", sample.iter().take(8).map(|s| s.as_str()).collect::<Vec<_>>().join(", ")) },
                if unread.is_empty() { String::new() } else { format!("; unread: {}", unread.join("; ")) }
            ));
        }
    }
    let find_zone = |name: &str| -> Option<std::path::PathBuf> {
        let direct = zone_dir.join(format!("{name}.ff"));
        if direct.is_file() {
            return Some(direct);
        }
        std::fs::read_dir(&zone_dir).ok()?.flatten().map(|e| e.path().join(format!("{name}.ff"))).find(|p| p.is_file())
    };
    let mut paks = crate::image::ImagePaks::new(&zone_dir);
    // Its loading zone.
    match find_zone(&format!("{stem}_load")) {
        None => why.push(format!("no {stem}_load.ff")),
        Some(path) => match read_zone_picture(&path, &mut paks, |n| n.starts_with("loadscreen") || n.contains("_load"), 2048) {
            Ok((n, rgba)) => return Ok((format!("`{n}` ({stem}_load)"), false, rgba)),
            Err(e) => why.push(format!("{stem}_load: {e}")),
        },
    }
    // Its minimap.
    let compass = format!("compass_map_{stem}");
    match find_zone(stem) {
        None => why.push(format!("no {stem}.ff for its minimap")),
        Some(path) => match read_zone_picture(&path, &mut paks, |n| n == compass, 2048) {
            Ok((n, rgba)) => return Ok((format!("its minimap `{n}`"), true, rgba)),
            Err(e) => why.push(format!("minimap `{compass}`: {e}")),
        },
    }
    Err(why.join("; "))
}

/// bo2mp: a Ghosts map's minimap (`compass_map_<map>`), a streamed picture in
/// the map's own zone (the menu zones do not hold it), as RGBA.
pub fn read_minimap(install: &Path, picture: &str) -> Result<(u32, u32, Vec<u8>), String> {
    let stem = picture.trim_start_matches(',').to_ascii_lowercase();
    let stem = stem.strip_prefix("compass_map_").unwrap_or(&stem).to_owned();
    let zone_dir = install.join("zone");
    let want = format!("compass_map_{stem}");
    let path = [zone_dir.join(format!("{stem}.ff"))]
        .into_iter()
        .chain(std::fs::read_dir(&zone_dir).into_iter().flatten().flatten().map(|e| e.path().join(format!("{stem}.ff"))))
        .find(|p| p.is_file())
        .ok_or_else(|| format!("no {stem}.ff for its minimap"))?;
    let mut paks = crate::image::ImagePaks::new(&zone_dir);
    read_zone_picture(&path, &mut paks, |n| n == want, 2048).map(|(_, rgba)| rgba)
}

/// The first image of a zone whose name `want` takes, as RGBA.
fn read_zone_picture(path: &Path, paks: &mut crate::image::ImagePaks, want: impl Fn(&str) -> bool, max: u32) -> Result<(String, (u32, u32, Vec<u8>)), String> {
    let z = crate::Zone::open(path).map_err(|e| e.to_string())?;
    let (cap, _) = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| crate::walk::walk_zone(&z))).map_err(|_| "its walk failed".to_owned())?;
    let mut why = Vec::new();
    for rec in &cap.images {
        let name = rec.name.trim_start_matches(',').to_ascii_lowercase();
        if !want(&name) {
            continue;
        }
        match picture_of(rec, &z.image, paks, max) {
            Ok(p) => match decode_rgba(&p) {
                Some(rgba) => return Ok((name, rgba)),
                None => why.push(format!("`{name}`: DXGI format {} not decoded", p.format)),
            },
            Err(e) => why.push(format!("`{name}`: {e}")),
        }
    }
    Err(if why.is_empty() { format!("not among its {} images", cap.images.len()) } else { why.join(", ") })
}

fn decode_rgba(p: &Picture) -> Option<(u32, u32, Vec<u8>)> {
    let (w, h) = (p.width as usize, p.height as usize);
    let (unit, block) = dxgi_unit(p.format)?;
    let mut out = vec![0u8; w * h * 4];
    if !block {
        if unit != 4 {
            return None;
        }
        out.copy_from_slice(p.data.get(..w * h * 4)?);
        if matches!(p.format, 87..=91) {
            for px in out.chunks_exact_mut(4) {
                px.swap(0, 2);
            }
        }
        return Some((p.width, p.height, out));
    }
    let bw = w.div_ceil(4);
    let mut tmp = [0u8; 64];
    for (i, b) in p
        .data
        .chunks_exact(unit)
        .take(bw * h.div_ceil(4))
        .enumerate()
    {
        let (bx, by) = (i % bw * 4, i / bw * 4);
        match p.format {
            70..=72 => bcdec_rs::bc1(b, &mut tmp, 16),
            73..=75 => bcdec_rs::bc2(b, &mut tmp, 16),
            76..=78 => bcdec_rs::bc3(b, &mut tmp, 16),
            97..=99 => bcdec_rs::bc7(b, &mut tmp, 16),
            _ => return None,
        }
        for y in 0..4 {
            for x in 0..4 {
                if bx + x < w && by + y < h {
                    let o = ((by + y) * w + bx + x) * 4;
                    out[o..o + 4].copy_from_slice(&tmp[(y * 4 + x) * 4..(y * 4 + x) * 4 + 4]);
                }
            }
        }
    }
    Some((p.width, p.height, out))
}

#[cfg(test)]
mod tests {
    //! `IW6_INSTALL=<install> cargo test -p fastfile_iw6 --release ui_image -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn ui_image() {
        let Some(install) = std::env::var_os("IW6_INSTALL") else {
            return;
        };
        let install = std::path::PathBuf::from(install);
        for n in [
            "weapon_ak12_360",
            "hud_icon_ak12",
            "scope_acog_360",
            "weapon_maaws_360",
        ] {
            let got = super::read_ui_image(&install, &[n.to_owned()]);
            eprintln!(
                "{n}: {:?}",
                got.as_ref().map(|(w, h, d)| (
                    w,
                    h,
                    d.len(),
                    d.chunks(4).filter(|p| p[3] > 0).count()
                ))
            );
            if let (Some((w, h, d)), Ok(dir)) = (got, std::env::var("IW6_DUMP")) {
                let f = std::fs::File::create(std::path::Path::new(&dir).join(format!("{n}.png")))
                    .unwrap();
                let mut e = png::Encoder::new(f, w, h);
                e.set_color(png::ColorType::Rgba);
                e.write_header().unwrap().write_image_data(&d).unwrap();
            }
        }
    }
}
