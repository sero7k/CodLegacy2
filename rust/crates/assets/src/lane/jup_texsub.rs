//! bo2mp: pictures from Modern Warfare Remastered (Horizon MW's map, the H1
//! lane's reader) and Modern Warfare 2 (2009, the IW4 lane's reader)
//! standing in for a Modern Warfare III (2023) map's own where those are
//! missing or poor: a material with no colour picture (drawn grey), one whose
//! picture the install cannot read, or one whose top levels are only in the
//! download packages (`*_cdn.xpak`: the install holds a blurry small level).
//!
//! The games share maps (Highrise is `mp_jup_highrise` in MW3, `mp_highrise`
//! in MW2 and in Horizon MW, a port of MW2's), laid out alike. A material is
//! matched where it is used:
//!
//! 1. **By place.** Its world triangles are looked up among the Horizon MW
//!    map's world triangles at the same place (after the two worlds are
//!    lined up: the translation that puts the most of one on the other) and
//!    facing the same way; the Horizon MW material covering most of its area
//!    there wins when it covers enough of it.
//! 2. **By role and colour.** Otherwise (a material only on placed models,
//!    or where the worlds differ) its own small picture's colour and
//!    contrast against every candidate picture of the same role (concrete,
//!    metal, glass, wood, roofing, brick, tile, ground: from the candidate
//!    material's name; the MW3 material's role from where it lies: roofs and
//!    floors face up, walls stand). A material without any picture of its
//!    own takes the commonest candidate of its role by area.
//!
//! The picture of the Horizon MW material is its colour map, read from the
//! map zone and its `.pak`; one its zone only names (a retail picture) is
//! taken from MW2's material or picture of the same name. Where the MW3
//! material has a colour of its own (its small level), the stand-in is
//! tinted to it, so the stand-in brings detail, not another palette.
//!
//! Every stand-in is logged (`jup sub: <material> -> <game>:<picture>
//! (<why>)`); what is left as it was is counted. Switches:
//! `BO2MP_JUP_SUB=0` (off), `BO2MP_JUP_SUB_MWR=<zone.ff or folder>` and
//! `BO2MP_JUP_SUB_MW2=<zone.ff or folder>` (where the other games' maps are;
//! else the game folders: `hmw-usermaps/<map>/<map>.ff` or `zone/<map>.ff`
//! under Modern Warfare Remastered's, `zone/english/<map>.ff`,
//! `zone/dlc/<map>.ff` or `zone/<map>.ff` under Modern Warfare 2's),
//! `BO2MP_JUP_SUB_LOW=<side>` (a picture read at this many pixels or fewer
//! while its full size is larger counts as low; default 512: a low one is
//! replaced only by place, never by colour alone).

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use fastfile_t7::map::T7Map;
use fastfile_t7::texture::ImageLevels;

/// The largest a stand-in is drawn at.
const STANDIN_MAX: u32 = 1024;
/// A tinted stand-in is redrawn as RGBA at most this size.
const TINTED_MAX: u32 = 512;
/// Grid cell of the place lookup (units).
const CELL: f32 = 96.0;
/// How near a Horizon MW triangle must lie to count (units).
const NEAR: f32 = 40.0;

/// Why a material wants a stand-in.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum Why {
    /// No colour picture at all (drawn grey).
    NoPicture,
    /// Its picture is named but cannot be read from the install.
    NotRead,
    /// Read at `have` pixels a side of its `full`.
    Low { have: u32, full: u32 },
}

impl Why {
    fn text(self) -> String {
        match self {
            Self::NoPicture => "no picture".to_owned(),
            Self::NotRead => "picture not in the install".to_owned(),
            Self::Low { have, full } => format!("only {have} of {full} px in the install"),
        }
    }
}

/// A material that wants a stand-in, with what it has of its own.
pub(super) struct Need {
    /// Index into the converted map's materials (the zone's order).
    pub material: usize,
    pub name: String,
    pub why: Why,
    /// Its own (small) picture as RGBA, when it has one.
    pub own: Option<(u32, u32, Vec<u8>)>,
    /// Its first layer's tint (the colour row MW3 multiplies its picture
    /// by), where it has one: the colour a stand-in is matched to when it
    /// has no picture of its own.
    pub tint: Option<[f32; 3]>,
}

/// A stand-in chosen for a material.
pub(super) struct Sub {
    pub material: usize,
    /// The picture's key in the converted map (`T7Map::add_image`).
    pub key: String,
    pub levels: ImageLevels,
}

/// Is `BO2MP_JUP_SUB` on (default)?
pub(super) fn on() -> bool {
    !std::env::var("BO2MP_JUP_SUB").is_ok_and(|v| matches!(v.trim(), "0" | "off" | "false" | "no"))
}

/// The side at or under which a picture read smaller than its full size is
/// a low one.
pub(super) fn low_side() -> u32 {
    std::env::var("BO2MP_JUP_SUB_LOW").ok().and_then(|v| v.trim().parse().ok()).unwrap_or(512)
}

/// The surface role a material plays, from its name's words (candidates)
/// or from where it lies (MW3's materials have no names).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub(super) enum Role {
    Concrete,
    Metal,
    Glass,
    Wood,
    Roofing,
    Brick,
    Tile,
    Ground,
    Other,
}

/// A candidate material's role by the words of its name (Horizon MW's and
/// MW2's names: `mtl_concrete_wall_01`, `me_corrugated_metal2`, ...).
pub(super) fn role_of_name(name: &str) -> Role {
    let n = name.to_ascii_lowercase();
    let has = |ws: &[&str]| ws.iter().any(|w| n.contains(w));
    if has(&["glass", "window", "windo"]) {
        Role::Glass
    } else if has(&["roof", "tar_", "_tar", "gravel", "shingle", "tarpaper"]) {
        Role::Roofing
    } else if has(&["brick"]) {
        Role::Brick
    } else if has(&["tile", "marble", "terrazzo"]) {
        Role::Tile
    } else if has(&["wood", "plank", "plywood", "crate", "pallet", "board"]) {
        Role::Wood
    } else if has(&["metal", "steel", "rust", "alum", "pipe", "vent", "duct", "grate", "corrugat", "iron", "chrome", "tin_", "sheet"]) {
        Role::Metal
    } else if has(&["asphalt", "dirt", "ground", "sand", "mud", "grass", "road"]) {
        Role::Ground
    } else if has(&["concrete", "cement", "cinder", "plaster", "stucco", "wall", "floor", "ceiling", "pillar", "column", "curb", "stone", "rock"]) {
        Role::Concrete
    } else {
        Role::Other
    }
}

/// An MW3 material's role from where its triangles lie: mostly facing up
/// (roofs, floors) or standing (walls), and its colour where it has one.
fn role_of_place(up: f32, side: f32, mean: Option<[f32; 3]>) -> Role {
    let grey = mean.is_none_or(|c| {
        let m = (c[0] + c[1] + c[2]) / 3.0;
        (c[0] - m).abs() + (c[1] - m).abs() + (c[2] - m).abs() < 0.12 * m.max(0.05)
    });
    let dark = mean.is_some_and(|c| 0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2] < 0.08);
    if up > 0.6 {
        if dark { Role::Roofing } else { Role::Concrete }
    } else if side > 0.6 {
        if grey { Role::Concrete } else { Role::Other }
    } else {
        Role::Other
    }
}

/// sRGB byte to linear.
fn lin(b: u8) -> f32 {
    let c = f32::from(b) / 255.0;
    if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) }
}

fn srgb(c: f32) -> u8 {
    let c = c.clamp(0.0, 1.0);
    let s = if c <= 0.003_130_8 { c * 12.92 } else { 1.055 * c.powf(1.0 / 2.4) - 0.055 };
    (s * 255.0).round() as u8
}

/// A picture's mean colour (linear) and contrast (the luminance's spread).
fn stats(rgba: &[u8]) -> ([f32; 3], f32) {
    let n = (rgba.len() / 4).max(1);
    let step = (n / 4096).max(1);
    let (mut sum, mut lum, mut lum2, mut k) = ([0f32; 3], 0f32, 0f32, 0f32);
    for p in rgba.chunks_exact(4).step_by(step) {
        let c = [lin(p[0]), lin(p[1]), lin(p[2])];
        for i in 0..3 {
            sum[i] += c[i];
        }
        let l = 0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2];
        lum += l;
        lum2 += l * l;
        k += 1.0;
    }
    let k = k.max(1.0);
    let m = lum / k;
    (sum.map(|s| s / k), (lum2 / k - m * m).max(0.0).sqrt())
}

/// A picture decoded to RGBA (its top level).
fn rgba_of(l: &ImageLevels) -> Option<(u32, u32, Vec<u8>)> {
    fastfile_t7::texture::level_rgba(l, 0)
}

/// RGBA with its mip chain (box filtered), as one picture.
fn rgba_levels(w: u32, h: u32, px: Vec<u8>) -> ImageLevels {
    let mut data = px.clone();
    let (mut cw, mut ch, mut cur) = (w, h, px);
    let mut levels = 1;
    while cw > 1 || ch > 1 {
        let (nw, nh) = ((cw / 2).max(1), (ch / 2).max(1));
        let mut next = vec![0u8; (nw * nh * 4) as usize];
        for y in 0..nh {
            for x in 0..nw {
                for c in 0..4 {
                    let mut s = 0u32;
                    for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                        let (sx, sy) = ((2 * x + dx).min(cw - 1), (2 * y + dy).min(ch - 1));
                        s += u32::from(cur[((sy * cw + sx) * 4 + c) as usize]);
                    }
                    next[((y * nw + x) * 4 + c) as usize] = (s / 4) as u8;
                }
            }
        }
        data.extend_from_slice(&next);
        cur = next;
        (cw, ch) = (nw, nh);
        levels += 1;
    }
    ImageLevels { format: 29, width: w, height: h, levels, data, part: None }
}

/// RGBA shrunk by halves to at most `max` a side.
pub(super) fn shrink(mut w: u32, mut h: u32, mut px: Vec<u8>, max: u32) -> (u32, u32, Vec<u8>) {
    while (w > max || h > max) && w > 1 && h > 1 {
        let (nw, nh) = (w / 2, h / 2);
        let mut next = vec![0u8; (nw * nh * 4) as usize];
        for y in 0..nh {
            for x in 0..nw {
                for c in 0..4 {
                    let s: u32 = [(0, 0), (1, 0), (0, 1), (1, 1)].iter().map(|(dx, dy)| u32::from(px[(((2 * y + dy) * w + 2 * x + dx) * 4 + c) as usize])).sum();
                    next[((y * nw + x) * 4 + c) as usize] = (s / 4) as u8;
                }
            }
        }
        (w, h, px) = (nw, nh, next);
    }
    (w, h, px)
}

/// A candidate picture from one of the other games.
struct Candidate {
    game: &'static str,
    material: String,
    picture: String,
    role: Role,
    mean: [f32; 3],
    contrast: f32,
    /// World area the material covers on its own map (0: unknown).
    area: f32,
    levels: ImageLevels,
}

/// A Horizon MW world triangle for the place lookup.
#[derive(Clone, Copy)]
struct Tri {
    mid: [f32; 3],
    normal: [f32; 3],
    material: u32,
}

/// The Horizon MW map: its materials' pictures and its world's triangles.
struct Mwr {
    /// Per material (the zone's order): the candidate index, when its
    /// colour picture reads.
    picture_of: Vec<Option<usize>>,
    /// Per material: its name and its colour picture's name.
    names: Vec<(String, Option<String>)>,
    tris: Vec<Tri>,
    grid: HashMap<(i32, i32, i32), Vec<u32>>,
    /// Its spawn points (where the map is played).
    spawns: Vec<[f32; 3]>,
}

fn cell_of(p: [f32; 3]) -> (i32, i32, i32) {
    ((p[0] / CELL).floor() as i32, (p[1] / CELL).floor() as i32, (p[2] / CELL).floor() as i32)
}

fn cross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}

fn sub3(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

/// A triangle's centre, unit normal and area.
fn tri_facts(a: [f32; 3], b: [f32; 3], c: [f32; 3]) -> ([f32; 3], [f32; 3], f32) {
    let n = cross(sub3(b, a), sub3(c, a));
    let len = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
    let mid = [(a[0] + b[0] + c[0]) / 3.0, (a[1] + b[1] + c[1]) / 3.0, (a[2] + b[2] + c[2]) / 3.0];
    (mid, if len > 1e-9 { n.map(|x| x / len) } else { [0.0, 0.0, 1.0] }, 0.5 * len)
}

/// Where another game's copy of the map is: an explicit zone, or a zone
/// under one of the folders given.
fn find_zone(var: &str, key: &str, map: &str, under: &[&str]) -> Option<PathBuf> {
    let file = format!("{map}.ff");
    let given = codlegacy_env::var_os(var)
        .filter(|v| !v.is_empty())
        .map(PathBuf::from);
    if let Some(g) = &given
        && g.is_file()
    {
        return Some(g.clone());
    }
    let roots: Vec<PathBuf> = given.into_iter().chain(asset_transport::game_paths::get(key)).collect();
    for root in roots {
        if root.join(&file).is_file() {
            return Some(root.join(&file));
        }
        for sub in under {
            let p = root.join(sub.replace("{map}", map)).join(&file);
            if p.is_file() {
                return Some(p);
            }
        }
    }
    None
}

/// The other games' name of an MW3 map (`mp_jup_highrise` -> `mp_highrise`).
pub(super) fn classic_name(jup: &str) -> String {
    jup.to_ascii_lowercase().replacen("mp_jup_", "mp_", 1)
}

/// Reads the Horizon MW map: its world triangles and its materials' colour
/// pictures (candidates pushed into `out`).
fn read_mwr(path: &Path, out: &mut Vec<Candidate>, report: &mut Vec<String>) -> Option<Mwr> {
    let started = std::time::Instant::now();
    let zone = match fastfile_h1::Zone::open(path) {
        Ok(z) => z,
        Err(e) => {
            report.push(format!("jup sub: Horizon MW map {} not read: {e}", path.display()));
            return None;
        }
    };
    let (cap, _stop) = fastfile_h1::walk::walk_zone(&zone);
    let map = fastfile_h1::map::read_map(&cap, &zone.image, &zone.assets.script_strings);
    let mut paks = fastfile_h1::image::ImagePaks::for_zone(path, None);
    // Area per material over the world.
    let mut tris = Vec::new();
    let mut area = vec![0f32; cap.materials.len()];
    if let Some(world) = &map.world {
        for s in &world.surfaces {
            let Some(m) = s.material.map(|m| m.index as usize) else { continue };
            for t in 0..usize::from(s.tri_count) {
                let at = s.base_index as usize + 3 * t;
                let Some(ix) = world.indices.get(at..at + 3) else { break };
                let v = |k: usize| world.vertices.get(s.first_vertex as usize + usize::from(ix[k])).map(|v| v.xyz);
                let (Some(a), Some(b), Some(c)) = (v(0), v(1), v(2)) else { continue };
                let (mid, normal, ar) = tri_facts(a, b, c);
                if ar < 1.0 {
                    continue;
                }
                if let Some(x) = area.get_mut(m) {
                    *x += ar;
                }
                tris.push(Tri { mid, normal, material: m as u32 });
            }
        }
    }
    let mut grid: HashMap<(i32, i32, i32), Vec<u32>> = HashMap::new();
    for (i, t) in tris.iter().enumerate() {
        grid.entry(cell_of(t.mid)).or_default().push(i as u32);
    }
    let mut picture_of = vec![None; cap.materials.len()];
    let mut names = Vec::with_capacity(cap.materials.len());
    let (mut read, mut unread) = (0usize, 0usize);
    for (mi, m) in cap.materials.iter().enumerate() {
        let colour = m
            .textures
            .iter()
            .find(|t| t.name_hash == super::h1::COLOR_MAP_HASH)
            .or_else(|| m.textures.iter().find(|t| t.semantic == asset_material::TS_COLOR_MAP))
            .and_then(|t| t.image)
            .and_then(|id| cap.images.get(id.index as usize));
        let name = m.name.trim_start_matches(',').to_owned();
        names.push((name.clone(), colour.map(|r| r.name.trim_start_matches(',').to_owned())));
        // (Only the materials the world draws, and the props' by name: a
        // world map's own materials. Effects and guns are left.)
        if area[mi] <= 0.0 && !name.starts_with("mc/") && !name.starts_with("m/") {
            continue;
        }
        let Some(rec) = colour else { continue };
        if rec.name.starts_with('$') || fastfile_h1::shared::is_reference(rec) {
            unread += 1;
            continue;
        }
        let px = match fastfile_h1::image::image_pixels(rec, &zone.image, &mut paks, STANDIN_MAX) {
            Ok(px) => px,
            Err(_) => {
                unread += 1;
                continue;
            }
        };
        let levels = ImageLevels { format: px.dxgi_format, width: px.width, height: px.height, levels: px.levels.max(1), data: px.data, part: None };
        let Some((_, _, rgba)) = rgba_of(&levels) else {
            unread += 1;
            continue;
        };
        let (mean, contrast) = stats(&rgba);
        read += 1;
        picture_of[mi] = Some(out.len());
        out.push(Candidate {
            game: "mwr",
            material: name.clone(),
            picture: rec.name.trim_start_matches(',').to_owned(),
            role: role_of_name(&name),
            mean,
            contrast,
            area: area[mi],
            levels,
        });
    }
    report.push(format!(
        "jup sub: Horizon MW {}: {} world triangles, {} materials, {read} colour pictures read, {unread} not (retail pictures or not in its pak) [{:.1}s]",
        path.display(),
        tris.len(),
        cap.materials.len(),
        started.elapsed().as_secs_f32()
    ));
    let spawns = map.spawns.iter().filter(|s| s.classname.contains("spawn")).map(|s| s.origin).collect();
    Some(Mwr { picture_of, names, tris, grid, spawns })
}

/// Reads Modern Warfare 2's map: its materials' colour pictures (from the
/// zone, or the archives in `main` beside it), by material and by picture
/// name.
fn read_mw2(path: &Path, wanted: &dyn Fn(&str, &str) -> bool, out: &mut Vec<Candidate>, report: &mut Vec<String>) -> HashMap<String, usize> {
    use asset_transport::{LoadProgress, StageId, ZoneMemory};
    let started = std::time::Instant::now();
    let mut by_name = HashMap::new();
    let image = match asset_transport::open_zone(path) {
        Ok(i) => i,
        Err(e) => {
            report.push(format!("jup sub: MW2 map {} not read: {e}", path.display()));
            return by_name;
        }
    };
    let Ok(header) = image.header() else { return by_name };
    let mut memory = ZoneMemory::for_header(&header);
    let Ok(mut stream) = memory.stream(&image.bytes) else { return by_name };
    let progress = LoadProgress::new(0);
    let mut sink = super::ZoneWalkSink::with_stage(progress.begin_scoped(StageId::MapAssets, "mw2 stand-ins", None));
    sink.set_capture_zone(asset_core::ZoneOwner::from_zone_path(path));
    sink.set_capture_ns(asset_core::AssetNamespace::Iw4);
    let _ = fastfile_iw4::load_zone(&mut stream, &mut sink);
    let main = asset_transport::game_main_for_zone(path).ok();
    let (mut read, mut unread) = (0usize, 0usize);
    for m in &sink.materials.materials {
        let name = m.name.as_str().trim_start_matches(',').to_owned();
        let Some(img) = m.textures.iter().find(|t| t.semantic == asset_material::TS_COLOR_MAP).and_then(|t| t.image).and_then(|i| sink.materials.images.get(i)) else { continue };
        let picture = img.name.as_str().trim_start_matches(',').to_owned();
        if picture.starts_with('$') || !wanted(&name, &picture) {
            continue;
        }
        let rgba = if !img.payload.is_empty() {
            asset_material::material_images::decode_zone_image_rgba(u32::from(img.width), u32::from(img.height), img.format, &img.payload).ok()
        } else {
            main.as_deref().and_then(|main| asset_material::material_images::decode_iwd_colour_image(main, &picture)).and_then(Result::ok).and_then(|i| asset_material::material_images::decoded_image_top_level_rgba8(&i).ok())
        };
        let Some((w, h, px)) = rgba.filter(|(w, h, _)| *w >= 16 && *h >= 16) else {
            unread += 1;
            continue;
        };
        let (w, h, px) = shrink(w, h, px, STANDIN_MAX);
        let (mean, contrast) = stats(&px);
        read += 1;
        by_name.insert(name.clone(), out.len());
        by_name.entry(picture.clone()).or_insert(out.len());
        out.push(Candidate { game: "mw2", material: name.clone(), picture, role: role_of_name(&name), mean, contrast, area: 0.0, levels: rgba_levels(w, h, px) });
    }
    report.push(format!(
        "jup sub: MW2 {}: {} materials, {read} colour pictures read, {unread} not (archives {}) [{:.1}s]",
        path.display(),
        sink.materials.materials.len(),
        main.as_ref().map_or_else(|| "not found".to_owned(), |m| m.display().to_string()),
        started.elapsed().as_secs_f32()
    ));
    by_name
}

/// One MW3 material's triangles in the world (centre, normal, area), the
/// world's and the placed models' apart.
#[derive(Default)]
struct Place {
    world: Vec<([f32; 3], [f32; 3], f32)>,
    props: Vec<([f32; 3], [f32; 3], f32)>,
}

impl Place {
    /// Shares of the area facing up and standing.
    fn facing(&self) -> (f32, f32) {
        let (mut up, mut side, mut all) = (0f32, 0f32, 0f32);
        for (_, n, a) in self.world.iter().chain(&self.props) {
            all += a;
            if n[2] > 0.7 {
                up += a;
            } else if n[2].abs() < 0.3 {
                side += a;
            }
        }
        if all <= 0.0 { (0.0, 0.0) } else { (up / all, side / all) }
    }
}

/// Each wanted material's triangles (at most a few thousand each, kept
/// spread over its surfaces).
fn places(t7: &T7Map, wanted: &HashMap<usize, usize>) -> Vec<Place> {
    let mut out: Vec<Place> = (0..wanted.len()).map(|_| Place::default()).collect();
    for s in &t7.surfaces {
        let Some(&k) = s.material.and_then(|m| wanted.get(&m)) else { continue };
        let step = (usize::from(s.tri_count) / 64).max(1);
        for t in (0..usize::from(s.tri_count)).step_by(step) {
            let at = s.base_index as usize + 3 * t;
            let Some(ix) = t7.indices.get(at..at + 3) else { break };
            let v = |k: usize| t7.vertices.get(s.first_vertex as usize + usize::from(ix[k])).map(|v| v.xyz);
            let (Some(a), Some(b), Some(c)) = (v(0), v(1), v(2)) else { continue };
            let (mid, n, ar) = tri_facts(a, b, c);
            if ar >= 1.0 {
                out[k].world.push((mid, n, ar * step as f32));
            }
        }
    }
    // Placed models: a few triangles of each placement.
    for sm in &t7.static_models {
        let Some(model) = t7.models.get(sm.model) else { continue };
        for s in &model.surfaces {
            let Some(&k) = s.material.and_then(|m| wanted.get(&m)) else { continue };
            if out[k].props.len() > 4000 {
                continue;
            }
            let step = (s.tris.len() / 8).max(1);
            for t in s.tris.iter().step_by(step) {
                let p = |i: u16| {
                    let q = s.positions[usize::from(i)];
                    std::array::from_fn::<f32, 3, _>(|k| sm.origin[k] + sm.scale * (q[0] * sm.axis[0][k] + q[1] * sm.axis[1][k] + q[2] * sm.axis[2][k]))
                };
                let (mid, n, ar) = tri_facts(p(t[0]), p(t[1]), p(t[2]));
                if ar >= 0.5 {
                    out[k].props.push((mid, n, ar * step as f32));
                }
            }
        }
    }
    out
}

/// How the MW3 world is laid on the Horizon MW world: a quarter turn about
/// the vertical (`turns`), a mirror across x first (`mirror`), then a
/// shift.
#[derive(Clone, Copy, Debug)]
struct Align {
    turns: u8,
    mirror: bool,
    off: [f32; 3],
}

impl Align {
    fn turn(&self, v: [f32; 3]) -> [f32; 3] {
        let [x, y, z] = v;
        let x = if self.mirror { -x } else { x };
        match self.turns & 3 {
            0 => [x, y, z],
            1 => [-y, x, z],
            2 => [-x, -y, z],
            _ => [y, -x, z],
        }
    }
    fn point(&self, p: [f32; 3]) -> [f32; 3] {
        let q = self.turn(p);
        [q[0] + self.off[0], q[1] + self.off[1], q[2] + self.off[2]]
    }
}

/// The median of each axis.
fn medians(pts: &[[f32; 3]]) -> [f32; 3] {
    std::array::from_fn(|k| {
        let mut v: Vec<f32> = pts.iter().map(|p| p[k]).collect();
        v.sort_by(f32::total_cmp);
        v.get(v.len() / 2).copied().unwrap_or(0.0)
    })
}

/// How the MW3 world lies on the Horizon MW world: of the eight quarter
/// turns and mirrors, the one (with its shift) that puts the most of the
/// MW3 play area's world on Horizon MW's world; the shift searched from the
/// medians' difference over +-3072 in cells of 256, then finer down to 16.
/// (The remakes keep the old layout; this finds how it was moved.)
fn line_up(t7: &T7Map, mwr: &Mwr, report: &mut Vec<String>) -> Option<Align> {
    if mwr.tris.is_empty() || t7.surfaces.is_empty() {
        return None;
    }
    // The MW3 world's points where the map is played (its collision's).
    let ours: Vec<[f32; 3]> = {
        let mut pts = Vec::new();
        for (si, s) in t7.surfaces.iter().enumerate() {
            if t7.no_collision.get(si).copied().unwrap_or(false) {
                continue;
            }
            for t in (0..usize::from(s.tri_count)).step_by(4) {
                let at = s.base_index as usize + 3 * t;
                let Some(ix) = t7.indices.get(at..at + 3) else { break };
                let v = |k: usize| t7.vertices.get(s.first_vertex as usize + usize::from(ix[k])).map(|v| v.xyz);
                let (Some(a), Some(b), Some(c)) = (v(0), v(1), v(2)) else { continue };
                let (mid, _, ar) = tri_facts(a, b, c);
                if ar >= 4.0 {
                    pts.push(mid);
                }
            }
        }
        let step = (pts.len() / 4000).max(1);
        pts.into_iter().step_by(step).collect()
    };
    if ours.is_empty() {
        return None;
    }
    let theirs: Vec<[f32; 3]> = mwr.tris.iter().map(|t| t.mid).collect();
    let set_of = |cell: f32| -> std::collections::HashSet<(i32, i32, i32)> {
        theirs.iter().map(|p| ((p[0] / cell).floor() as i32, (p[1] / cell).floor() as i32, (p[2] / cell).floor() as i32)).collect()
    };
    let sets = [(256.0f32, set_of(256.0)), (128.0, set_of(128.0)), (32.0, set_of(32.0))];
    // (The coarse levels score a sparser sample.)
    let coarse_pts: Vec<[f32; 3]> = ours.iter().step_by((ours.len() / 500).max(1)).copied().collect();
    let score = |a: &Align, level: usize| -> usize {
        let (cell, set) = (&sets[level].0, &sets[level].1);
        let pts: &[[f32; 3]] = if level < 2 { &coarse_pts } else { &ours };
        pts.iter()
            .filter(|p| {
                let q = a.point(**p);
                set.contains(&((q[0] / cell).floor() as i32, (q[1] / cell).floor() as i32, (q[2] / cell).floor() as i32))
            })
            .count()
    };
    let their_mid = medians(&theirs);
    {
        // (Where each world lies: 10th, 50th and 90th percentiles per axis.)
        let spread = |pts: &[[f32; 3]]| -> String {
            (0..3)
                .map(|k| {
                    let mut v: Vec<f32> = pts.iter().map(|p| p[k]).collect();
                    v.sort_by(f32::total_cmp);
                    let at = |q: f32| v.get(((v.len() as f32 - 1.0) * q) as usize).copied().unwrap_or(0.0);
                    format!("{:.0}/{:.0}/{:.0}", at(0.1), at(0.5), at(0.9))
                })
                .collect::<Vec<_>>()
                .join(", ")
        };
        report.push(format!("jup sub: MW3 play-area world x, y, z (10/50/90%): {}; Horizon MW world: {}", spread(&ours), spread(&theirs)));
    }
    // The MW3 map's spawn points (made over its path nodes).
    let our_spawns: Vec<[f32; 3]> = t7
        .entities
        .split('}')
        .filter(|b| b.contains("\"classname\" \"mp_") && b.contains("spawn"))
        .filter_map(|b| {
            let line = b.lines().find(|l| l.trim().starts_with("\"origin\""))?;
            let v: Vec<f32> = line.trim().split('"').nth(3)?.split_whitespace().filter_map(|x| x.parse().ok()).collect();
            (v.len() == 3).then(|| [v[0], v[1], v[2]])
        })
        .collect();
    let mut tried = Vec::new();
    let mut best: Option<(Align, usize)> = None;
    for mirror in [false, true] {
        for turns in 0..4u8 {
            let mut a = Align { turns, mirror, off: [0.0; 3] };
            let turned: Vec<[f32; 3]> = ours.iter().map(|p| a.turn(*p)).collect();
            let m = medians(&turned);
            // (Heights from the spawn points where both maps have them: a
            // world's median height is its skyline's.)
            let dz = if mwr.spawns.is_empty() || our_spawns.is_empty() { 0.0 } else { medians(&mwr.spawns)[2] - medians(&our_spawns)[2] };
            let mut bases = vec![[their_mid[0] - m[0], their_mid[1] - m[1], dz], [0.0, 0.0, dz]];
            if !mwr.spawns.is_empty() && !our_spawns.is_empty() {
                let ts: Vec<[f32; 3]> = our_spawns.iter().map(|p| a.turn(*p)).collect();
                let (sm, tm) = (medians(&mwr.spawns), medians(&ts));
                bases.insert(0, [sm[0] - tm[0], sm[1] - tm[1], dz]);
            }
            // Coarse round the spawns' and the worlds' medians' shift, and
            // round no shift at all.
            let mut local: Option<(Align, usize)> = None;
            for base in bases {
                for z in -3..=3 {
                    for y in -12..=12 {
                        for x in -12..=12 {
                            a.off = [base[0] + x as f32 * 256.0, base[1] + y as f32 * 256.0, base[2] + z as f32 * 64.0];
                            let s = score(&a, 0);
                            if local.as_ref().is_none_or(|(_, b)| s > *b) {
                                local = Some((a, s));
                            }
                        }
                    }
                }
            }
            let Some((mut a, _)) = local else { continue };
            // Finer: 128 cells by 64, then 32 cells by 16.
            for (level, step, reach) in [(1usize, 64.0f32, 4i32), (2, 16.0, 4)] {
                let base = a.off;
                let mut keep = (a, score(&a, level));
                for z in -reach..=reach {
                    for y in -reach..=reach {
                        for x in -reach..=reach {
                            let mut c = a;
                            c.off = [base[0] + x as f32 * step, base[1] + y as f32 * step, base[2] + z as f32 * step * 0.5];
                            let s = score(&c, level);
                            if s > keep.1 {
                                keep = (c, s);
                            }
                        }
                    }
                }
                a = keep.0;
            }
            let s = score(&a, 2);
            tried.push(format!("{}{}: {:.0}%", if mirror { "mirrored, " } else { "" }, u32::from(turns) * 90, s as f32 * 100.0 / ours.len() as f32));
            if best.as_ref().is_none_or(|(_, b)| s > *b) {
                best = Some((a, s));
            }
        }
    }
    let (a, s) = best?;
    let share = s as f32 / ours.len() as f32;
    report.push(format!(
        "jup sub: worlds lined up: MW3 turned {} degrees{} and moved ({:.0}, {:.0}, {:.0}) lies on Horizon MW's world at {:.0}% of {} play-area points within 32 units (by turn: {})",
        u32::from(a.turns) * 90,
        if a.mirror { ", mirrored" } else { "" },
        a.off[0],
        a.off[1],
        a.off[2],
        share * 100.0,
        ours.len(),
        tried.join(", ")
    ));
    (share > 0.25).then_some(a)
}

/// The Horizon MW material most of a place's area lies on, with its share.
fn vote(place: &[([f32; 3], [f32; 3], f32)], mwr: &Mwr, align: &Align) -> Option<(usize, f32, f32)> {
    let mut votes: HashMap<u32, f32> = HashMap::new();
    let (mut all, mut hit) = (0f32, 0f32);
    for &(mid, n, a) in place {
        all += a;
        let p = align.point(mid);
        let n = align.turn(n);
        let c = cell_of(p);
        let mut best: Option<(f32, u32)> = None;
        for dz in -1..=1 {
            for dy in -1..=1 {
                for dx in -1..=1 {
                    let Some(list) = mwr.grid.get(&(c.0 + dx, c.1 + dy, c.2 + dz)) else { continue };
                    for &ti in list {
                        let t = &mwr.tris[ti as usize];
                        let facing = (t.normal[0] * n[0] + t.normal[1] * n[1] + t.normal[2] * n[2]).abs();
                        if facing < 0.8 {
                            continue;
                        }
                        let d = sub3(t.mid, p);
                        // (Off the plane counts four times: the same wall,
                        // not the one behind it.)
                        let along = d[0] * t.normal[0] + d[1] * t.normal[1] + d[2] * t.normal[2];
                        let d2 = d[0] * d[0] + d[1] * d[1] + d[2] * d[2] + 15.0 * along * along;
                        if d2 < NEAR * NEAR * 4.0 && best.is_none_or(|(b, _)| d2 < b) {
                            best = Some((d2, t.material));
                        }
                    }
                }
            }
        }
        if let Some((_, m)) = best {
            *votes.entry(m).or_default() += a;
            hit += a;
        }
    }
    let (&m, &v) = votes.iter().max_by(|a, b| a.1.total_cmp(b.1))?;
    Some((m as usize, v / hit.max(1e-6), hit / all.max(1e-6)))
}

/// The candidate nearest in colour and contrast among those of `role`
/// (any role when none has it), with its distance.
fn by_colour(cands: &[Candidate], role: Role, mean: [f32; 3], contrast: f32) -> Option<(usize, f32)> {
    let dist = |c: &Candidate| {
        // (In a log space: a dark concrete next to a light one is far.)
        let l = |x: f32| (x.max(0.002)).ln();
        let d: f32 = (0..3).map(|k| (l(c.mean[k]) - l(mean[k])).powi(2)).sum::<f32>().sqrt();
        d + 2.0 * (c.contrast - contrast).abs()
    };
    let pool: Vec<usize> = (0..cands.len()).filter(|&i| cands[i].role == role).collect();
    let pool = if pool.is_empty() { (0..cands.len()).collect() } else { pool };
    pool.into_iter().map(|i| (i, dist(&cands[i]))).min_by(|a, b| a.1.total_cmp(&b.1))
}

/// The commonest candidate of `role` (by its area on its own map).
fn commonest(cands: &[Candidate], role: Role) -> Option<usize> {
    (0..cands.len()).filter(|&i| cands[i].role == role && cands[i].area > 0.0).max_by(|&a, &b| cands[a].area.total_cmp(&cands[b].area))
}

/// The stand-in's picture: as stored, or tinted to the MW3 material's own
/// colour where it has one (RGBA, at most `TINTED_MAX`).
fn picture_for(c: &Candidate, own_mean: Option<[f32; 3]>) -> (ImageLevels, Option<[f32; 3]>) {
    let Some(target) = own_mean else { return (c.levels.clone(), None) };
    let ratio: [f32; 3] = std::array::from_fn(|k| (target[k] / c.mean[k].max(0.002)).clamp(0.5, 2.0));
    if ratio.iter().all(|r| (r - 1.0).abs() < 0.08) {
        return (c.levels.clone(), None);
    }
    let Some((w, h, px)) = rgba_of(&c.levels) else { return (c.levels.clone(), None) };
    let (w, h, mut px) = shrink(w, h, px, TINTED_MAX);
    for p in px.chunks_exact_mut(4) {
        for k in 0..3 {
            p[k] = srgb(lin(p[k]) * ratio[k]);
        }
    }
    (rgba_levels(w, h, px), Some(ratio))
}

/// Chooses the stand-ins for `needs` (see the module's text). `zone`: the
/// MW3 map zone; `t7`: the converted map (its surfaces' materials are the
/// zone's order).
pub(super) fn substitute(zone: &Path, t7: &T7Map, needs: &[Need], report: &mut Vec<String>) -> Vec<Sub> {
    let mut subs = Vec::new();
    if needs.is_empty() {
        return subs;
    }
    let started = std::time::Instant::now();
    let stem = zone.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    let classic = classic_name(&stem);
    let mut cands: Vec<Candidate> = Vec::new();
    let mwr_path = find_zone("BO2MP_JUP_SUB_MWR", "mwr", &classic, &["hmw-usermaps/{map}", "zone", "usermaps/{map}"]);
    let mwr = mwr_path.as_deref().and_then(|p| read_mwr(p, &mut cands, report));
    if mwr_path.is_none() {
        report.push(format!("jup sub: no Horizon MW / Modern Warfare Remastered {classic} found (BO2MP_JUP_SUB_MWR, or the game folder's hmw-usermaps/{classic})"));
    }
    // MW2: the pictures of Horizon MW's materials it could not read (by
    // name), and every world-looking material for the colour match.
    let mwr_unread: std::collections::HashSet<String> = mwr
        .as_ref()
        .map(|m| m.names.iter().enumerate().filter(|(i, _)| m.picture_of[*i].is_none()).flat_map(|(_, (n, p))| std::iter::once(n.clone()).chain(p.clone())).collect())
        .unwrap_or_default();
    let mw2_path = find_zone("BO2MP_JUP_SUB_MW2", "mw2", &classic, &["zone/english", "zone/dlc", "zone"]);
    let mw2 = match mw2_path.as_deref() {
        Some(p) => read_mw2(p, &|m, pic| mwr_unread.contains(m) || mwr_unread.contains(pic) || (m.starts_with("mc/") || m.contains('_')) && role_of_name(m) != Role::Other, &mut cands, report),
        None => {
            report.push(format!("jup sub: no Modern Warfare 2 {classic} found (BO2MP_JUP_SUB_MW2, or the game folder's zone/english)"));
            HashMap::new()
        }
    };
    if cands.is_empty() {
        report.push(format!("jup sub: no stand-in pictures read; {} materials left as they are", needs.len()));
        return subs;
    }
    let wanted: HashMap<usize, usize> = needs.iter().enumerate().map(|(k, n)| (n.material, k)).collect();
    let places = places(t7, &wanted);
    let align = mwr.as_ref().and_then(|m| line_up(t7, m, report));
    let mut kept: HashMap<&'static str, usize> = HashMap::new();
    let mut by_game: HashMap<&'static str, usize> = HashMap::new();
    for (k, need) in needs.iter().enumerate() {
        let place = &places[k];
        let own = need.own.as_ref().map(|(_, _, px)| stats(px)).or(need.tint.map(|t| (t, 0.05)));
        // 1. By place (world triangles, then the props' with a stronger share).
        // A picture MW3 has, only small, is replaced only where the place
        // says so clearly (its world, most of it on one Horizon MW material):
        // never by colour alone.
        let low = matches!(need.why, Why::Low { .. });
        let mut pick: Option<(usize, String)> = None;
        if let (Some(m), Some(align)) = (mwr.as_ref(), align.as_ref()) {
            let tiers: &[(&Vec<([f32; 3], [f32; 3], f32)>, f32, f32, &str)] = if low {
                &[(&place.world, 0.6, 0.5, "world")]
            } else {
                &[(&place.world, 0.4, 0.25, "world"), (&place.props, 0.6, 0.25, "props")]
            };
            for &(tris, share_min, covered_min, what) in tiers {
                if pick.is_some() || tris.is_empty() {
                    continue;
                }
                if let Some((mat, share, covered)) = vote(tris, m, align)
                    && share >= share_min
                    && covered >= covered_min
                {
                    let cand = m.picture_of.get(mat).copied().flatten().or_else(|| {
                        let (n, p) = &m.names[mat];
                        mw2.get(n).or_else(|| p.as_ref().and_then(|p| mw2.get(p))).copied()
                    });
                    if let Some(c) = cand {
                        pick = Some((c, format!("same place as Horizon MW's {} ({what}, {:.0}% of the {:.0}% found)", m.names[mat].0, share * 100.0, covered * 100.0)));
                    }
                }
            }
        }
        // 2. By role and colour.
        let (up, side) = place.facing();
        let role = role_of_place(up, side, own.map(|o| o.0));
        if pick.is_none() && !low {
            if let Some((mean, contrast)) = own {
                if let Some((c, d)) = by_colour(&cands, role, mean, contrast)
                    && d < 1.2
                {
                    pick = Some((c, format!("{role:?} by colour (distance {d:.2})")));
                }
            } else if role != Role::Other
                && let Some(c) = commonest(&cands, role)
            {
                pick = Some((c, format!("{role:?} by where it lies ({:.0}% up, {:.0}% standing)", up * 100.0, side * 100.0)));
            }
        }
        let Some((c, how)) = pick else {
            *kept.entry(match need.why {
                Why::Low { .. } => "low, no match",
                _ => "no picture, no match",
            })
            .or_default() += 1;
            continue;
        };
        let cand = &cands[c];
        // (A small picture of MW3's own is kept where the stand-in's colour
        // is far from it: by more than 0.6..1.7 in any channel, the place
        // matched another kind of surface.)
        let own_mean = need.own.as_ref().map(|(_, _, px)| stats(px).0);
        if low
            && let Some(m) = own_mean
            && (0..3).any(|k| !(0.6..=1.7).contains(&(m[k] / cand.mean[k].max(0.002))))
        {
            *kept.entry("low, stand-in's colour too far").or_default() += 1;
            continue;
        }
        let (levels, tint) = picture_for(cand, own_mean);
        *by_game.entry(cand.game).or_default() += 1;
        report.push(format!(
            "jup sub: {} -> {}:{} ({}; {}; its material {}{})",
            need.name,
            cand.game,
            cand.picture,
            need.why.text(),
            how,
            cand.material,
            tint.map(|t| format!(", tinted x{:.2} {:.2} {:.2} to its own colour", t[0], t[1], t[2])).unwrap_or_default()
        ));
        diag::info!(World, "jup sub: {} -> {}:{} ({})", need.name, cand.game, cand.picture, how);
        subs.push(Sub { material: need.material, key: format!("jupsub_{}_{}", cand.game, cand.picture.replace(['/', '\\', ' '], "_")) + &tint.map(|t| format!("_t{:.2}_{:.2}_{:.2}", t[0], t[1], t[2])).unwrap_or_default(), levels });
    }
    let mut games: Vec<_> = by_game.into_iter().collect();
    games.sort();
    let mut left: Vec<_> = kept.into_iter().collect();
    left.sort();
    report.push(format!(
        "jup sub: {} of {} materials stood in for ({games:?}); left as they were: {left:?} [{:.1}s] (BO2MP_JUP_SUB=0 turns it off)",
        subs.len(),
        needs.len(),
        started.elapsed().as_secs_f32()
    ));
    subs
}

/// A tool picture (Caulk Shadow, Shadow, No Draw ...): at most 256 a side,
/// opaque, over half of it one light warm colour and a twentieth near
/// black (its caption). The game never draws these (shadow casters and
/// blockers); `render_jup::scene::tool_picture` reads them the same way.
pub(super) fn tool_picture(w: u32, h: u32, rgba: &[u8]) -> bool {
    if w > 256 || h > 256 || w < 16 || h < 16 || rgba.len() < (w * h * 4) as usize {
        return false;
    }
    let px: Vec<&[u8]> = rgba.chunks_exact(4).collect();
    if px.iter().any(|p| p[3] < 250) {
        return false;
    }
    let mut hist: HashMap<u16, usize> = HashMap::new();
    for p in &px {
        *hist.entry((u16::from(p[0] >> 3) << 10) | (u16::from(p[1] >> 3) << 5) | u16::from(p[2] >> 3)).or_default() += 1;
    }
    let Some((&mode, _)) = hist.iter().max_by_key(|(_, n)| **n) else { return false };
    let bg = [f32::from((mode >> 10) as u8 & 31) * 8.0 + 4.0, f32::from((mode >> 5) as u8 & 31) * 8.0 + 4.0, f32::from(mode as u8 & 31) * 8.0 + 4.0];
    let luma = |c: [f32; 3]| (0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2]) / 255.0;
    if luma(bg) < 0.25 || bg[0] < bg[2] + 40.0 {
        return false;
    }
    let (mut near_bg, mut dark) = (0usize, 0usize);
    for p in &px {
        let c = [f32::from(p[0]), f32::from(p[1]), f32::from(p[2])];
        if (0..3).all(|k| (c[k] - bg[k]).abs() <= 28.0) {
            near_bg += 1;
        } else if luma(c) < 0.12 {
            dark += 1;
        }
    }
    let n = px.len();
    near_bg * 2 > n && near_bg * 100 < n * 97 && dark * 20 > n
}

/// A picture multiplied by a linear colour. BC1 is tinted in place, block
/// by block: its two end colours scaled (by the colour to the power 1/2.2,
/// in the stored sRGB) and the block's mode kept (end colours swapped and
/// indices remapped where the scaling reorders them); anything else is
/// redrawn as RGBA, at most `TINTED_MAX` a side, with its mip chain.
pub(super) fn tint_levels(l: &ImageLevels, tint: [f32; 3]) -> ImageLevels {
    if matches!(l.format, 70..=72) {
        let s = tint.map(|t| t.max(0.0).powf(1.0 / 2.2));
        let mut out = l.clone();
        let scale = |c: u16| -> u16 {
            let r = (f32::from((c >> 11) & 31) * s[0]).round().clamp(0.0, 31.0) as u16;
            let g = (f32::from((c >> 5) & 63) * s[1]).round().clamp(0.0, 63.0) as u16;
            let b = (f32::from(c & 31) * s[2]).round().clamp(0.0, 31.0) as u16;
            (r << 11) | (g << 5) | b
        };
        for b in out.data.chunks_exact_mut(8) {
            let c0 = u16::from_le_bytes([b[0], b[1]]);
            let c1 = u16::from_le_bytes([b[2], b[3]]);
            let four = c0 > c1;
            let (mut n0, mut n1) = (scale(c0), scale(c1));
            let mut idx = u32::from_le_bytes([b[4], b[5], b[6], b[7]]);
            if four && n0 == n1 {
                // (Both ends one colour: every texel the first.)
                if n0 == 0 {
                    n0 = 1;
                } else {
                    n1 = n0 - 1;
                }
                idx = 0;
            } else if four && n0 < n1 {
                std::mem::swap(&mut n0, &mut n1);
                idx ^= 0x5555_5555;
            } else if !four && n0 > n1 {
                std::mem::swap(&mut n0, &mut n1);
                // (Three-colour mode: 0 and 1 trade places, 2 and 3 stay.)
                let mut r = 0u32;
                for k in 0..16 {
                    let x = (idx >> (2 * k)) & 3;
                    r |= (if x < 2 { x ^ 1 } else { x }) << (2 * k);
                }
                idx = r;
            }
            b[0..2].copy_from_slice(&n0.to_le_bytes());
            b[2..4].copy_from_slice(&n1.to_le_bytes());
            b[4..8].copy_from_slice(&idx.to_le_bytes());
        }
        return out;
    }
    let Some((w, h, px)) = rgba_of(l) else { return l.clone() };
    let (w, h, mut px) = shrink(w, h, px, TINTED_MAX);
    for p in px.chunks_exact_mut(4) {
        for k in 0..3 {
            p[k] = srgb(lin(p[k]) * tint[k]);
        }
    }
    rgba_levels(w, h, px)
}

/// A picture's mean brightness (display values, 0..1), over at most 4096 of
/// its texels.
pub(super) fn mean_brightness(l: &ImageLevels) -> Option<f32> {
    let (_, _, px) = rgba_of(l)?;
    let n = px.len() / 4;
    let step = (n / 4096).max(1);
    let (mut sum, mut k) = (0f32, 0f32);
    for p in px.chunks_exact(4).step_by(step) {
        sum += (0.3 * f32::from(p[0]) + 0.59 * f32::from(p[1]) + 0.11 * f32::from(p[2])) / 255.0;
        k += 1.0;
    }
    (k > 0.0).then(|| sum / k)
}

/// Above this mean brightness a placed model's colour picture is a tint
/// base: paint over white, the colour from the material (as
/// `render_jup::scene`'s).
pub(super) const TINT_BASE: f32 = 0.70;

/// The paint a placed model's name states (`..._yellow`, `..._grey`,
/// `red::...`), as a linear albedo multiplier for its near-white tint base;
/// without a colour word a mid-grey painted steel. (The same table as
/// `render_jup::scene::paint_of`: the game takes the paint from the
/// material's constants, which are not decoded far enough.)
pub(super) fn paint_of(name: &str) -> [f32; 3] {
    let n = name.to_ascii_lowercase();
    let words: &[(&str, [f32; 3])] = &[
        ("yellow", [0.62, 0.43, 0.05]),
        ("orange", [0.62, 0.24, 0.04]),
        ("red", [0.42, 0.06, 0.04]),
        ("blue", [0.07, 0.16, 0.38]),
        ("green", [0.10, 0.22, 0.09]),
        ("black", [0.06, 0.06, 0.06]),
        ("white", [0.72, 0.72, 0.70]),
        ("grey", [0.30, 0.30, 0.30]),
        ("gray", [0.30, 0.30, 0.30]),
        ("tan", [0.45, 0.36, 0.24]),
    ];
    let parts: Vec<&str> = n.split(['_', '/', ':']).collect();
    for (w, c) in words {
        if parts.iter().any(|p| p == w) {
            return *c;
        }
    }
    [0.62, 0.62, 0.62]
}
