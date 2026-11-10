//! Advanced Warfare's 2-D pictures (Create-a-Class `weapon_<gun>`, kill
//! icons `hud_icon_<gun>`) by material name, as RGBA, from `common_mp.ff`.
//!
//! The first call walks common_mp once and keeps the inline top level of
//! the small pictures and where the streamed ones are (`imagefile<N>.pak`);
//! the streamed ones are read when asked for.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use crate::image::{ImagePaks, ImagePixels, dxgi_unit, image_pixels};
use crate::walk::ImageRec;

/// Material name prefixes kept.
const KEPT: &[&str] = &["weapon_", "hud_icon_"];

enum Picture {
    Inline(ImagePixels),
    Streamed(ImageRec),
}

type Pictures = HashMap<String, Picture>;

fn store() -> &'static Mutex<HashMap<PathBuf, Pictures>> {
    static STORE: OnceLock<Mutex<HashMap<PathBuf, Pictures>>> = OnceLock::new();
    STORE.get_or_init(Default::default)
}

fn kept(name: &str) -> bool {
    KEPT.iter().any(|p| name.starts_with(p))
        // (the camo-loot and gold variants of each gun's picture)
        && !name.contains("_loot")
        && !name.ends_with("_gold")
        && !name.ends_with("_atlas")
}

fn read_pictures(zone_dir: &Path) -> Pictures {
    let mut out = Pictures::new();
    let Ok(z) = crate::Zone::open(&zone_dir.join("common_mp.ff")) else {
        return out;
    };
    let mut w = crate::walk::Walker::new(&z.image, &z.assets, &z.header.stream_files);
    let _ = w.walk();
    let cap = &w.cap;
    let mut paks = ImagePaks::new(zone_dir);
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
        let pic = if rec.streamed {
            Picture::Streamed(rec.clone())
        } else {
            match image_pixels(rec, &z.image, &mut paks, 1024) {
                Ok(px) => Picture::Inline(px),
                Err(_) => continue,
            }
        };
        out.insert(name.to_owned(), pic);
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
    names.iter().find_map(|n| match pictures.get(n.as_str())? {
        Picture::Inline(px) => decode_rgba(px),
        Picture::Streamed(rec) => {
            let mut paks = ImagePaks::new(&zone_dir);
            let px = image_pixels(rec, &[], &mut paks, 1024).ok()?;
            decode_rgba(&px)
        }
    })
}

/// bo2mp: Horizon MW's weapon icons (`hud_icon_<gun>`, the HUD and kill
/// icon of each of its guns) by material name, as RGBA, from its weapon
/// zones in `hmw_zone_dir` (`hmw_*.ff`). The zones are walked once a run
/// and only their icons kept; `None` when no zone has the picture.
pub fn read_hmw_icon(hmw_zone_dir: &Path, names: &[String]) -> Option<(u32, u32, Vec<u8>)> {
    static ICONS: OnceLock<Mutex<HashMap<PathBuf, Icons>>> = OnceLock::new();
    let mut guard = ICONS.get_or_init(Default::default).lock().ok()?;
    let icons = guard
        .entry(hmw_zone_dir.to_path_buf())
        .or_insert_with(|| read_hmw_icons(hmw_zone_dir));
    names
        .iter()
        .find_map(|n| icons.get(n.trim_start_matches(',')).cloned())
}

/// bo2mp: Horizon MW's killstreak pictures (the D-pad icon of the Tactical
/// Nuke, `dpad_killstreak_nuke`, ...) by material name, as RGBA, from its
/// killstreak zones (`hmw_killstreak.ff` with its `.pak`, and
/// `hmw_dlc_killstreaks.ff`) in `hmw_zone_dir`. The zones are walked once a
/// run for the pictures asked; `None` when no zone has one.
pub fn read_hmw_killstreak_icon(hmw_zone_dir: &Path, names: &[String]) -> Option<(u32, u32, Vec<u8>)> {
    static ICONS: OnceLock<Mutex<HashMap<(PathBuf, String), Option<(u32, u32, Vec<u8>)>>>> = OnceLock::new();
    let mut guard = ICONS.get_or_init(Default::default).lock().ok()?;
    for name in names {
        let name = name.trim_start_matches(',');
        let key = (hmw_zone_dir.to_path_buf(), name.to_owned());
        if let Some(found) = guard.get(&key) {
            if found.is_some() {
                return found.clone();
            }
            continue;
        }
        let found = read_killstreak_icon(hmw_zone_dir, name).map(|p| if name.starts_with("dpad_killstreak_") { dpad_frame(p) } else { p });
        guard.insert(key, found.clone());
        if found.is_some() {
            return found;
        }
    }
    None
}

/// A D-pad icon is a sheet of eight by four frames of its flashing icon
/// (384 x 192: 48 pixels each); the steady lit one is the third row's first.
fn dpad_frame((w, h, px): (u32, u32, Vec<u8>)) -> (u32, u32, Vec<u8>) {
    if w != 2 * h || w % 8 != 0 {
        return (w, h, px);
    }
    let f = w / 8;
    let mut out = Vec::with_capacity((f * f * 4) as usize);
    for y in 0..f {
        let start = (((2 * f + y) * w) * 4) as usize;
        out.extend_from_slice(&px[start..start + (f * 4) as usize]);
    }
    (f, f, out)
}

fn read_killstreak_icon(dir: &Path, name: &str) -> Option<(u32, u32, Vec<u8>)> {
    let retail = dir.parent().and_then(Path::parent).map(|p| p.join("zone"));
    for zone in ["hmw_killstreak", "hmw_dlc_killstreaks"] {
        let path = dir.join(format!("{zone}.ff"));
        let Ok(z) = crate::Zone::open(&path) else {
            continue;
        };
        let (cap, _) = crate::walk::walk_zone(&z);
        let Some(rec) = cap
            .materials
            .iter()
            .find(|m| m.name.trim_start_matches(',') == name)
            .and_then(|m| m.textures.iter().find_map(|t| t.image))
            .and_then(|i| cap.images.get(i.index as usize))
        else {
            continue;
        };
        let mut paks = ImagePaks::for_zone(&path, retail.as_deref());
        if let Some(rgba) = image_pixels(rec, &z.image, &mut paks, 512)
            .ok()
            .and_then(|px| decode_rgba(&px))
            .filter(|(w, h, _)| *w > 1 && *h > 1)
        {
            return Some(rgba);
        }
    }
    None
}

/// bo2mp: a map's loading-screen picture (`loadscreen_<map>`, the
/// `mapimage` its `.arena` names) by name, as RGBA, from its loading zone
/// (`<map>_load.ff` beside the map's zone; Horizon MW's usermaps keep theirs
/// in the map's folder). A streamed one is read from the map's own `.pak`,
/// else the retail `imagefile<N>.pak` in `retail_zone_dir`. `None` when the
/// zone has no picture of those names.
pub fn read_map_picture(map_zone: &Path, retail_zone_dir: Option<&Path>, names: &[String]) -> Option<(u32, u32, Vec<u8>)> {
    let stem = map_zone.file_stem()?.to_str()?;
    let load = map_zone.with_file_name(format!("{stem}_load.ff"));
    for zone in [load.as_path(), map_zone] {
        let Ok(z) = crate::Zone::open(zone) else {
            continue;
        };
        let (cap, _) = crate::walk::walk_zone(&z);
        let image_of = |n: &str| {
            cap.images.iter().find(|r| r.name.trim_start_matches(',') == n).or_else(|| {
                let m = cap.materials.iter().find(|m| m.name.trim_start_matches(',') == n)?;
                let i = m.textures.iter().find_map(|t| t.image)?;
                cap.images.get(i.index as usize)
            })
        };
        let Some(rec) = names.iter().find_map(|n| image_of(n)) else {
            continue;
        };
        // (Its own pak is the map's, `<map>.pak`, whichever zone it is in.)
        for mut paks in [ImagePaks::for_zone(map_zone, retail_zone_dir), ImagePaks::for_zone(zone, retail_zone_dir)] {
            if let Some(rgba) = image_pixels(rec, &z.image, &mut paks, 2048)
                .ok()
                .and_then(|px| decode_rgba(&px))
                .filter(|(w, h, _)| *w > 1 && *h > 1)
            {
                return Some(rgba);
            }
        }
    }
    None
}

/// Pictures by material name, as RGBA.
type Icons = HashMap<String, (u32, u32, Vec<u8>)>;

fn read_hmw_icons(dir: &Path) -> Icons {
    let mut out = HashMap::new();
    let mut retail_wanted: HashMap<String, Vec<String>> = HashMap::new();
    // (The retail paks are in the install's `zone/`, two folders up.)
    let retail = dir.parent().and_then(Path::parent).map(|p| p.join("zone"));
    for zone in ["hmw_ar1", "hmw_smg", "hmw_shotgun"] {
        let path = dir.join(format!("{zone}.ff"));
        let Ok(z) = crate::Zone::open(&path) else {
            continue;
        };
        let (cap, _) = crate::walk::walk_zone(&z);
        let mut paks = ImagePaks::for_zone(&path, retail.as_deref());
        for w in &cap.weapons {
            for (_, id) in &w.materials {
                let Some(m) = cap.materials.get(id.index as usize) else {
                    continue;
                };
                let name = m.name.trim_start_matches(',');
                if out.contains_key(name) || !(name.contains("icon") || name.starts_with("hud_")) {
                    continue;
                }
                let Some(rec) = m
                    .textures
                    .iter()
                    .find_map(|t| t.image)
                    .and_then(|i| cap.images.get(i.index as usize))
                else {
                    continue;
                };
                match image_pixels(rec, &z.image, &mut paks, 512)
                    .ok()
                    .and_then(|px| decode_rgba(&px))
                    .filter(|(w, h, _)| *w > 1 && *h > 1)
                {
                    Some(rgba) => {
                        out.insert(name.to_owned(), rgba);
                    }
                    // (A 1 x 1 stand-in: Horizon MW draws Modern Warfare
                    // Remastered's own picture of that name, in its
                    // common_mp.)
                    None => {
                        retail_wanted
                            .entry(rec.name.trim_start_matches(',').to_owned())
                            .or_default()
                            .push(name.to_owned());
                    }
                }
            }
        }
    }
    if let Some(common) = retail
        .as_ref()
        .map(|d| d.join("common_mp.ff"))
        .filter(|p| p.is_file())
        && !retail_wanted.is_empty()
    {
        let mut shared = crate::shared::SharedImages::default();
        // (Only the pictures wanted: common_mp holds 304 MB of inline ones.)
        let mut wanted = crate::shared::Wanted {
            images: retail_wanted
                .keys()
                .map(|k| crate::shared::image_key(k))
                .collect(),
            ..Default::default()
        };
        if shared
            .add_zone_for(&common, Some(&mut wanted), 0, false)
            .is_ok()
        {
            let mut paks = ImagePaks::new(retail.as_deref().unwrap_or(dir));
            for (image, materials) in retail_wanted {
                if let Some(rgba) = shared
                    .pixels(&image, &mut paks, 512)
                    .ok()
                    .and_then(|px| decode_rgba(&px))
                    .filter(|(w, h, _)| *w > 1 && *h > 1)
                {
                    for m in materials {
                        out.entry(m).or_insert_with(|| rgba.clone());
                    }
                }
            }
        }
    }
    out
}

/// The top level of `p` as RGBA.
pub fn decode_rgba(p: &ImagePixels) -> Option<(u32, u32, Vec<u8>)> {
    let (w, h) = (p.width as usize, p.height as usize);
    let format = p.dxgi_format;
    let (unit, block) = dxgi_unit(format)?;
    let mut out = vec![0u8; w * h * 4];
    if !block {
        if unit != 4 {
            return None;
        }
        out.copy_from_slice(p.data.get(..w * h * 4)?);
        if matches!(format, 87..=91) {
            for px in out.chunks_exact_mut(4) {
                px.swap(0, 2);
            }
        }
        return Some((p.width, p.height, out));
    }
    let bw = w.div_ceil(4);
    let blocks = bw * h.div_ceil(4);
    let mut tmp = [0u8; 64];
    for (i, b) in p.data.get(..blocks * unit)?.chunks_exact(unit).enumerate() {
        let (bx, by) = (i % bw * 4, i / bw * 4);
        match format {
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
    //! `S1_INSTALL=<install> cargo test -p fastfile_s1 --release ui_image -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn ui_image() {
        let Some(install) = std::env::var_os("S1_INSTALL") else {
            return;
        };
        let install = std::path::PathBuf::from(install);
        for n in ["weapon_ak12", "hud_icon_ak12", "hud_icon_bal27"] {
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
        }
    }
}
