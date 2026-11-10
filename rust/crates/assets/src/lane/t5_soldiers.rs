//! bo2mp: Call of Duty: Black Ops' (2010) multiplayer soldiers as Black Ops
//! II's: the CIA's for the allies and the Spetsnaz for the axis (the
//! factions of the Black Ops team emblems the HUD shows), one per class
//! outfit (standard, utility, flak, armor, camo), each with its head.
//!
//! Both are read from Nuketown's zone (`mp_nuked`, whose teamset is those
//! two), their pictures from Black Ops' IWDs. A soldier's body and head are
//! made one model on Black Ops II's soldier skeleton (the match's own `*_fb`
//! body's bones, names and poses), so Black Ops II's animations, tags and
//! hit locations drive it unchanged. The two games share the skeleton's
//! bone names and axes: each vertex keeps its place in its bone's space,
//! moved from that bone's bind in Black Ops to its bind in Black Ops II (a
//! bone Black Ops II lacks moves with the nearest one it has).
//!
//! Pictures are made what Black Ops II's lit model draw reads: the colour
//! map as it is; the normal map's x and y (Black Ops keeps x in alpha in its
//! DXT5 normal maps, in red in its DXN ones) as BC5; the specular colour
//! with the gloss map in alpha.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};

use asset_core::{AssetEdge, AssetNamespace, AssetRef, ZoneOwner};
use asset_material::{
    AuthoredImage, AuthoredMaterial, MaterialCatalog, MaterialTextureBinding, TS_COLOR_MAP,
    TS_NORMAL_MAP, TS_SPECULAR_MAP,
};
use asset_model::{FpvMeshBuild, ModelSkel};
use bevy::image::Image;
use bevy::math::{Quat, Vec3};

/// The allies' soldiers (body, head): the CIA's.
pub const ALLIES: &[(&str, &str)] = &[
    ("c_usa_cia_mp_body_standard", "c_usa_cia_mp_head_1"),
    ("c_usa_cia_mp_body_utility", "c_usa_cia_mp_head_2"),
    ("c_usa_cia_mp_body_flak", "c_usa_cia_mp_head_3"),
    ("c_usa_cia_mp_body_armor", "c_usa_cia_mp_head_4"),
    ("c_usa_cia_mp_body_camo", "c_usa_cia_mp_head_5"),
];
/// The axis' soldiers: the Spetsnaz.
pub const AXIS: &[(&str, &str)] = &[
    ("c_rus_spet_mp_body_standard", "c_rus_spet_mp_head_1"),
    ("c_rus_spet_mp_body_utility", "c_rus_spet_mp_head_2"),
    ("c_rus_spet_mp_body_flak", "c_rus_spet_mp_head_3"),
    ("c_rus_spet_mp_body_armor", "c_rus_spet_mp_head_4"),
    ("c_rus_spet_mp_body_camo", "c_rus_spet_mp_head_5"),
];

/// The zone that holds them.
const ZONE: &str = "t5:mp_nuked";

/// The largest normal map side kept.
const MAX_NORMAL: u32 = 1024;
/// The largest specular map side kept.
const MAX_SPECULAR: u32 = 512;

/// The model a soldier is drawn as.
#[must_use]
pub fn model_name(body: &str) -> String {
    format!("t5_sol_{body}")
}

/// A picture as RGBA8, top level only.
#[derive(Clone)]
struct Rgba {
    width: u32,
    height: u32,
    px: Vec<u8>,
}

/// What a soldier's material is drawn with.
#[derive(Clone, Default)]
struct Pictures {
    colour: Option<Arc<Image>>,
    normal: Option<Arc<Image>>,
    specular: Option<Arc<Image>>,
    /// The material's draw (`soldier_draw`).
    draw: Option<asset_core::T6Draw>,
}

/// A soldier read: his body, then his head (moved into the body's space).
struct Soldier {
    body: &'static str,
    axis: bool,
    parts: Vec<ModelSkel>,
    /// Per part, per surface: its material's name.
    materials: Vec<Vec<Option<String>>>,
}

struct Read {
    zone: PathBuf,
    soldiers: Vec<Soldier>,
    pictures: HashMap<String, Pictures>,
    report: Vec<String>,
}

fn read() -> Option<&'static Read> {
    static READ: OnceLock<Option<Read>> = OnceLock::new();
    READ.get_or_init(|| {
        let root = asset_transport::discover::games_root_from_env().ok()?;
        let zone = asset_transport::find_zone_file(&root, ZONE).ok()?.path;
        let main = asset_material::game_main_for_zone(&zone).ok();
        Some(read_zone(&zone, main.as_deref()))
    })
    .as_ref()
}

/// Reads the soldiers ahead (off the match's critical path).
pub(crate) fn prefetch() {
    let _ = read();
}

/// The walk: Black Ops' materials and the soldiers' models.
#[derive(Default)]
struct Sink {
    strings: fastfile_t5::ScriptStrings,
    materials: MaterialCatalog,
    models: HashMap<String, ModelSkel>,
}

impl fastfile_t5::AssetSink for Sink {
    fn set_script_strings(&mut self, strings: fastfile_t5::ScriptStrings) {
        self.strings = strings;
    }

    fn load_asset(
        &mut self,
        s: &mut fastfile_t5::ZoneStream<'_>,
        _index: usize,
        ty: fastfile_t5::AssetType,
        slot: fastfile_t5::Ptr,
    ) -> fastfile_t5::Result<()> {
        // lane B: every soldier's model read (each with its materials,
        // read before it), the rest of the map's zone is not wanted: the
        // walk stops here (`walk` keeps what was read).
        if self.models.len() == 2 * (ALLIES.len() + AXIS.len()) {
            return Err(fastfile_t5::ZoneError::NoAssetLoader(ty));
        }
        fastfile_t5::load_asset_at_observed(s, ty, slot, self)?;
        Ok(())
    }
}

impl fastfile_t5::AssetLinkSink for Sink {
    fn loaded(
        &mut self,
        s: &fastfile_t5::ZoneStream<'_>,
        ty: fastfile_t5::AssetType,
        slot: fastfile_t5::Ptr,
        insert_slot: Option<fastfile_t5::Ptr>,
    ) -> fastfile_t5::Result<()> {
        self.materials.t5_loaded(s, ty, slot, insert_slot);
        if ty == fastfile_t5::AssetType::XModel
            && let Some(geometry) = s.latest_xmodel()
            && let Some(name) = geometry.name.and_then(|p| s.cstr(p).ok())
            && ALLIES
                .iter()
                .chain(AXIS)
                .any(|(b, h)| *b == name || *h == name)
            && let Some(skel) =
                asset_model::capture_xmodel_skel_t5(s, &self.strings, geometry, &self.materials)
        {
            self.models.insert(name.to_owned(), skel);
        }
        Ok(())
    }

    fn alias(
        &mut self,
        ty: fastfile_t5::AssetType,
        slot: fastfile_t5::Ptr,
        target: fastfile_t5::Ptr,
    ) -> fastfile_t5::Result<()> {
        self.materials.t5_alias(ty, slot, target);
        Ok(())
    }
}

fn read_zone(zone: &Path, main: Option<&Path>) -> Read {
    let started = std::time::Instant::now();
    let mut report = Vec::new();
    let mut read = Read {
        zone: zone.to_path_buf(),
        soldiers: Vec::new(),
        pictures: HashMap::new(),
        report: Vec::new(),
    };
    let sink = match walk(zone) {
        Ok(s) => s,
        Err(e) => {
            read.report
                .push(format!("bo1 soldiers: {}: {e}", zone.display()));
            return read;
        }
    };
    let index = main.and_then(|m| asset_transport::IwdIndex::open(m).ok());
    if index.is_none() {
        report.push("bo1 soldiers: Black Ops' IWDs not found (drawn without pictures)".into());
    }
    for (axis, list) in [(false, ALLIES), (true, AXIS)] {
        for &(body, head) in list {
            let Some(b) = sink.models.get(body) else {
                report.push(format!("bo1 soldiers: {body} is not in {}", zone.display()));
                continue;
            };
            let mut parts = vec![lod0(b)];
            match sink.models.get(head).map(lod0) {
                Some(mut h) => {
                    into_body_space(&mut h, &parts[0]);
                    parts.push(h);
                }
                None => report.push(format!("bo1 soldiers: {body}: no head {head}")),
            }
            let materials = parts
                .iter()
                .map(|p| {
                    p.surface_materials
                        .iter()
                        .map(|m| {
                            sink.materials
                                .materials
                                .get(m.as_ref()?.get())
                                .map(|m| m.name.as_str().to_owned())
                        })
                        .collect()
                })
                .collect();
            read.soldiers.push(Soldier {
                body,
                axis,
                parts,
                materials,
            });
        }
    }
    let walked = started.elapsed().as_secs_f32();
    // Each material's pictures, once: what each names read off the walk,
    // then made on several threads.
    let mut names: Vec<&str> = read
        .soldiers
        .iter()
        .flat_map(|s| s.materials.iter().flatten().flatten())
        .map(String::as_str)
        .collect();
    names.sort_unstable();
    names.dedup();
    let jobs: Vec<PictureJob> = names
        .into_iter()
        .filter_map(|name| {
            let m = sink
                .materials
                .materials
                .iter()
                .find(|m| m.name.as_str() == name)?;
            // A character material names several pictures in a slot: its
            // colour map and face paint, a tiled detail normal map before
            // its own, its gloss map and a rim mask next to its specular
            // map.
            let images = m
                .textures
                .iter()
                .filter_map(|t| {
                    let i = sink.materials.images.get(t.image?)?;
                    Some((
                        t.semantic,
                        AssetRef::bare_name(i.name.as_str()).to_ascii_lowercase(),
                    ))
                })
                .collect();
            Some(PictureJob {
                name: name.to_owned(),
                technique_set: m.technique_set.as_str().to_owned(),
                sort_key: m.sort_key,
                images,
            })
        })
        .collect();
    // (lane B: one material after another took most of the 98.5 s the
    // soldiers cost a Nuketown load.)
    let threads = std::thread::available_parallelism()
        .map_or(1, usize::from)
        .clamp(1, 8);
    let chunk = jobs.len().div_ceil(threads).max(1);
    let index = index.as_deref();
    let made: Vec<(String, Pictures, Vec<String>)> = std::thread::scope(|s| {
        let parts: Vec<_> = jobs
            .chunks(chunk)
            .map(|part| {
                s.spawn(move || {
                    part.iter()
                        .map(|job| material_pictures(job, index, main))
                        .collect::<Vec<_>>()
                })
            })
            .collect();
        parts
            .into_iter()
            .flat_map(|p| p.join().unwrap_or_default())
            .collect()
    });
    for (name, pictures, lines) in made {
        report.extend(lines);
        read.pictures.insert(name, pictures);
    }
    report.push(format!(
        "bo1 soldiers: {} read from {} in {:.1} s ({} materials; the walk {walked:.1} s, \
         the pictures {:.1} s on {threads} threads)",
        read.soldiers.len(),
        zone.display(),
        started.elapsed().as_secs_f32(),
        read.pictures.len(),
        started.elapsed().as_secs_f32() - walked,
    ));
    read.report = report;
    read
}

/// A soldier material's draw: opaque and lit (a technique's own blend
/// state is its added-light pass's, not how the surface lays over what is
/// behind it), blended only for the see-through ones (the eyes' cornea).
fn soldier_draw(technique_set: &str, sort_key: u8) -> asset_core::T6Draw {
    let mut draw = asset_core::T6Draw::from_technique_set("", sort_key, None);
    draw.cull = asset_core::T6Cull::None;
    if technique_set.contains("transparent") {
        draw.blend = asset_core::T6Blend::Blend;
    }
    draw
}

/// One soldier material's pictures to make: what the walk said of it.
struct PictureJob {
    name: String,
    technique_set: String,
    sort_key: u8,
    /// Its pictures (semantic, lower-case name), in the material's order.
    images: Vec<(u8, String)>,
}

/// A material's pictures (its name, them, what went wrong).
fn material_pictures(
    job: &PictureJob,
    index: Option<&asset_transport::IwdIndex>,
    main: Option<&Path>,
) -> (String, Pictures, Vec<String>) {
    let mut report = Vec::new();
    let name = job.name.as_str();
    let images = &job.images;
    let image = |semantic: u8, suffix: &str, skip: &[&str]| {
        images
            .iter()
            .find(|(s, n)| {
                *s == semantic && n.ends_with(suffix) && !skip.iter().any(|k| n.contains(k))
            })
            .map(|(_, n)| n.clone())
    };
    let mut pictures = Pictures {
        draw: Some(soldier_draw(&job.technique_set, job.sort_key)),
        ..Default::default()
    };
    if let Some(index) = index {
        let main = main.unwrap_or(Path::new(""));
        if let Some(colour) = image(TS_COLOR_MAP, "", &["facepaint"]) {
            match asset_material::decode_iwd_colour_image(main, &colour) {
                Some(Ok(img)) => pictures.colour = Some(Arc::new(img)),
                Some(Err(e)) => report.push(format!("bo1 soldiers: {colour}: {e}")),
                None => report.push(format!("bo1 soldiers: {colour} is not in the IWDs")),
            }
        }
        let detail = ["char_base_", "skin_pore", "detail"];
        pictures.normal = images
            .iter()
            .rev()
            .find(|(s, n)| *s == TS_NORMAL_MAP && !detail.iter().any(|k| n.contains(k)))
            .and_then(|(_, n)| {
                let how = format!("n{MAX_NORMAL}");
                let made = cached_levels(index, &[n.as_str()], &how, &mut report, |report| {
                    let p = normal_xy(iwd_rgba(index, n, report)?);
                    let made = fastfile_t7::texture::normal_map_bc5(&levels(&p));
                    if made.is_none() {
                        report.push(format!("bo1 soldiers: {n}: not encoded"));
                    }
                    made
                })?;
                image_of(&made, false)
                    .map_err(|e| report.push(format!("bo1 soldiers: {n}: {e}")))
                    .ok()
            })
            .map(Arc::new);
        let gloss = image(TS_SPECULAR_MAP, "_g", &[]);
        pictures.specular = image(TS_SPECULAR_MAP, "_s", &[])
            .and_then(|s| {
                let how = format!("s{MAX_SPECULAR}");
                let named: Vec<&str> = std::iter::once(s.as_str()).chain(gloss.as_deref()).collect();
                cached_levels(index, &named, &how, &mut report, |report| {
                    let gloss = gloss.as_deref().and_then(|g| iwd_rgba(index, g, report));
                    let spec = iwd_rgba(index, &s, report)?;
                    Some(levels(&spec_gloss(spec, gloss.as_ref())))
                })
            })
            .and_then(|l| {
                image_of(&l, false)
                    .map_err(|e| report.push(format!("bo1 soldiers: {name}: {e}")))
                    .ok()
            })
            .map(Arc::new);
    }
    (job.name.clone(), pictures, report)
}

/// The artifact cache kind for the soldiers' made pictures.
const LEVELS_CACHE_KIND: &str = "t5soldier";

/// Bump when what a `t5soldier` entry holds, or how it is made, changes.
const LEVELS_CACHE_FORMAT: u32 = 1;

/// lane B: a soldier's normal and specular maps are decoded out of the IWDs
/// and re-encoded on every launch (the colour maps already have the mip
/// cache). Made once, they are kept in the artifact cache, keyed by the
/// archive entries they are made of (crc and size) and `how`.
/// `CODLEGACY_T5_SOLDIER_CACHE=0` turns it off.
fn cached_levels(
    index: &asset_transport::IwdIndex,
    names: &[&str],
    how: &str,
    report: &mut Vec<String>,
    make: impl FnOnce(&mut Vec<String>) -> Option<fastfile_t7::texture::ImageLevels>,
) -> Option<fastfile_t7::texture::ImageLevels> {
    let key = levels_cache_key(index, names, how);
    if let Some(hit) = key
        .as_deref()
        .and_then(|k| asset_transport::cache_get(LEVELS_CACHE_KIND, k))
        .and_then(|bytes| levels_decode(&bytes))
    {
        return Some(hit);
    }
    let made = make(report)?;
    if let Some(key) = key.as_deref()
        && let Err(e) = asset_transport::cache_put(LEVELS_CACHE_KIND, key, &levels_encode(&made))
    {
        report.push(format!("bo1 soldiers: picture cache {key}: {e}"));
    }
    Some(made)
}

/// `None` (not cached) when the cache is off or a picture is not in the
/// IWDs.
fn levels_cache_key(index: &asset_transport::IwdIndex, names: &[&str], how: &str) -> Option<String> {
    static ON: OnceLock<bool> = OnceLock::new();
    let on = *ON.get_or_init(|| {
        codlegacy_env::var("CODLEGACY_T5_SOLDIER_CACHE").map_or(true, |v| v.trim() != "0")
    });
    if !on {
        return None;
    }
    let mut text = format!("{LEVELS_CACHE_FORMAT}:{how}");
    for name in names {
        let entry = index.image_candidates(name)?.first()?;
        text.push_str(&format!(":{name}:{:08x}:{:x}", entry.crc32(), entry.size()));
    }
    Some(format!(
        "{LEVELS_CACHE_FORMAT:02x}-{:016x}",
        asset_transport::fnv1a64(text.as_bytes())
    ))
}

/// Format, width, height and level count (u32 each), then the levels.
fn levels_encode(l: &fastfile_t7::texture::ImageLevels) -> Vec<u8> {
    let mut out = Vec::with_capacity(16 + l.data.len());
    for v in [l.format, l.width, l.height, l.levels] {
        out.extend_from_slice(&v.to_le_bytes());
    }
    out.extend_from_slice(&l.data);
    out
}

fn levels_decode(bytes: &[u8]) -> Option<fastfile_t7::texture::ImageLevels> {
    let word = |i: usize| Some(u32::from_le_bytes(bytes.get(i * 4..i * 4 + 4)?.try_into().ok()?));
    let data = bytes.get(16..)?;
    if data.is_empty() {
        return None;
    }
    Some(fastfile_t7::texture::ImageLevels {
        format: word(0)?,
        width: word(1)?,
        height: word(2)?,
        levels: word(3)?,
        data: data.to_vec(),
        part: None,
    })
}

fn walk(zone: &Path) -> Result<Sink, String> {
    let image = asset_transport::open_zone(zone).map_err(|e| e.to_string())?;
    let header = image.t5_header().map_err(|e| e.to_string())?;
    let mut memory = asset_transport::T5ZoneMemory::for_header(&header);
    let mut stream = memory.stream(&image.bytes).map_err(|e| e.to_string())?;
    let mut sink = Sink::default();
    sink.materials
        .set_capture_zone(ZoneOwner::from_zone_path(zone));
    sink.materials.set_capture_ns(AssetNamespace::T5);
    // (A map zone's walk may stop past its models: what was read is kept.)
    let walked = fastfile_t5::load_zone(&mut stream, &mut sink);
    if sink.models.is_empty() {
        return Err(walked
            .err()
            .map_or_else(|| "no soldier models".to_owned(), |e| e.to_string()));
    }
    Ok(sink)
}

/// A model with only its first level of detail's surfaces.
fn lod0(m: &ModelSkel) -> ModelSkel {
    let mut m = m.clone();
    let (first, count) = m.lod_surf_span[0];
    let (first, count) = (usize::from(first), usize::from(count));
    if count == 0 || first + count > m.surface_vertex_ranges.len() {
        return m;
    }
    let keep = first..first + count;
    m.surface_vertex_ranges = m.surface_vertex_ranges[keep.clone()].to_vec();
    m.surface_index_ranges = m.surface_index_ranges[keep.clone()].to_vec();
    m.surface_materials = m.surface_materials[keep].to_vec();
    m
}

fn quat(b: &asset_model::BoneBind) -> Quat {
    Quat::from_xyzw(b.quat[0], b.quat[1], b.quat[2], b.quat[3]).normalize()
}

/// A head is built around its root bone: placed where the body has that
/// bone (the identity when both were built in one space).
fn into_body_space(head: &mut ModelSkel, body: &ModelSkel) {
    let Some(root) = head.bone_names.first() else {
        return;
    };
    let (Some(hb), Some(bb)) = (
        head.bones.first().copied(),
        body.bone_names
            .iter()
            .position(|n| n == root)
            .and_then(|i| body.bones.get(i))
            .copied(),
    ) else {
        return;
    };
    let turn = quat(&bb) * quat(&hb).inverse();
    let (from, to) = (Vec3::from_array(hb.trans), Vec3::from_array(bb.trans));
    let place = |p: [f32; 3]| (turn * (Vec3::from_array(p) - from) + to).to_array();
    for p in &mut head.positions {
        *p = place(*p);
    }
    for n in &mut head.normals {
        *n = (turn * Vec3::from_array(*n)).to_array();
    }
    for b in &mut head.bones {
        b.trans = place(b.trans);
        b.quat = (turn * quat(b)).to_array();
    }
}

/// An IWD picture's top level as RGBA8, normal maps' x put in red (Black
/// Ops' DXT5 normal maps keep it in alpha).
fn iwd_rgba(
    index: &asset_transport::IwdIndex,
    name: &str,
    report: &mut Vec<String>,
) -> Option<Rgba> {
    let bytes = index
        .image_candidates(name)?
        .iter()
        .find_map(|c| c.read().ok());
    let Some(bytes) = bytes else {
        report.push(format!("bo1 soldiers: {name} is not in the IWDs"));
        return None;
    };
    // (IWi 13 keeps its format at byte 4, IWi 8 at byte 8: 13 is DXT5.)
    let format = match bytes.get(3) {
        Some(13) => bytes.get(4),
        Some(8) => bytes.get(8),
        _ => None,
    }
    .copied();
    match asset_material::decode_iwi_rgba(&bytes) {
        Ok((width, height, mut px)) => {
            if format == Some(13) && name.ends_with("_n") {
                for t in px.chunks_exact_mut(4) {
                    t[0] = t[3];
                    t[3] = 255;
                }
            }
            Some(Rgba { width, height, px })
        }
        Err(e) => {
            report.push(format!("bo1 soldiers: {name}: {e}"));
            None
        }
    }
}

/// A picture no larger than `max` a side (halved, box filtered).
fn fit(mut p: Rgba, max: u32) -> Rgba {
    while p.width.max(p.height) > max && p.width > 1 && p.height > 1 {
        p = half(&p);
    }
    p
}

fn half(p: &Rgba) -> Rgba {
    let (w, h) = ((p.width / 2).max(1), (p.height / 2).max(1));
    let mut px = vec![0u8; (w * h * 4) as usize];
    for y in 0..h {
        for x in 0..w {
            for c in 0..4 {
                let at = |xx: u32, yy: u32| {
                    let (xx, yy) = (xx.min(p.width - 1), yy.min(p.height - 1));
                    u32::from(p.px[((yy * p.width + xx) * 4 + c) as usize])
                };
                let s = at(2 * x, 2 * y)
                    + at(2 * x + 1, 2 * y)
                    + at(2 * x, 2 * y + 1)
                    + at(2 * x + 1, 2 * y + 1);
                px[((y * w + x) * 4 + c) as usize] = ((s + 2) / 4) as u8;
            }
        }
    }
    Rgba {
        width: w,
        height: h,
        px,
    }
}

/// The normal map's x and y in red and green (unsigned, 128 flat), the
/// rest cleared.
fn normal_xy(mut p: Rgba) -> Rgba {
    for t in p.px.chunks_exact_mut(4) {
        t[2] = 0;
        t[3] = 255;
    }
    fit(p, MAX_NORMAL)
}

/// The specular colour with the gloss in alpha (Black Ops II's specular
/// map). Without a gloss map: a middling gloss.
fn spec_gloss(spec: Rgba, gloss: Option<&Rgba>) -> Rgba {
    let mut out = spec;
    for y in 0..out.height {
        for x in 0..out.width {
            let a = gloss.map_or(128, |g| {
                let gx = (x * g.width / out.width).min(g.width - 1);
                let gy = (y * g.height / out.height).min(g.height - 1);
                g.px[((gy * g.width + gx) * 4) as usize]
            });
            out.px[((y * out.width + x) * 4 + 3) as usize] = a;
        }
    }
    fit(out, MAX_SPECULAR)
}

/// A picture with its whole mip chain (RGBA8, biggest first).
fn levels(p: &Rgba) -> fastfile_t7::texture::ImageLevels {
    let mut data = p.px.clone();
    let mut level = p.clone();
    let mut count = 1;
    while level.width > 1 || level.height > 1 {
        level = half(&level);
        data.extend_from_slice(&level.px);
        count += 1;
    }
    fastfile_t7::texture::ImageLevels {
        format: 28,
        width: p.width,
        height: p.height,
        levels: count,
        data,
        part: None,
    }
}

/// The soldiers that were built for this match (their model names), by
/// team (`false`: the allies').
static BUILT: std::sync::Mutex<(Vec<String>, Vec<String>)> =
    std::sync::Mutex::new((Vec::new(), Vec::new()));

/// The model a player of a team is drawn as (by his number, so a team is
/// its several outfits), when the soldiers were built.
#[must_use]
pub fn soldier_for(axis: bool, client: u32) -> Option<String> {
    let built = BUILT.lock().ok()?;
    let list = if axis { &built.1 } else { &built.0 };
    (!list.is_empty()).then(|| list[client as usize % list.len()].clone())
}

/// One soldier on Black Ops II's skeleton (`bo2`: a soldier body), each
/// surface's material by name.
fn rebind(
    s: &Soldier,
    bo2: &asset_t6::XModelRef,
) -> Option<(asset_t6::XModelRef, Vec<Option<String>>)> {
    let bo2_of: HashMap<&str, usize> = bo2
        .bone_names
        .iter()
        .enumerate()
        .map(|(i, n)| (n.as_str(), i))
        .collect();
    let bo2_bind: Vec<(Quat, Vec3)> = bo2
        .base_mat
        .iter()
        .map(|(q, t)| (Quat::from_array(*q).normalize(), Vec3::from_array(*t)))
        .collect();
    let (mut mins, mut maxs) = ([f32::MAX; 3], [f32::MIN; 3]);
    let mut lod0 = Vec::new();
    let mut materials = Vec::new();
    for (part, names) in s.parts.iter().zip(&s.materials) {
        let n_bones = part.bone_names.len().min(part.bones.len());
        let direct: Vec<Option<usize>> = (0..n_bones)
            .map(|i| bo2_of.get(part.bone_names[i].as_str()).copied())
            .collect();
        let named: Vec<usize> = (0..n_bones).filter(|&i| direct[i].is_some()).collect();
        if named.is_empty() {
            continue;
        }
        let pos = |i: usize| Vec3::from_array(part.bones[i].trans);
        // Each bone: the bone whose place it moves with (itself, or the
        // nearest Black Ops II has).
        let proxy: Vec<usize> = (0..n_bones)
            .map(|i| match direct[i] {
                Some(_) => i,
                None => *named
                    .iter()
                    .min_by(|&&a, &&b| {
                        pos(a)
                            .distance_squared(pos(i))
                            .total_cmp(&pos(b).distance_squared(pos(i)))
                    })
                    .unwrap_or(&named[0]),
            })
            .collect();
        // From a Black Ops bone's bind to its Black Ops II bone's.
        let moves: Vec<(Quat, Vec3, Vec3, usize)> = proxy
            .iter()
            .map(|&src| {
                let t = direct[src].unwrap_or(0);
                let (q, to) = bo2_bind[t];
                (q * quat(&part.bones[src]).inverse(), pos(src), to, t)
            })
            .collect();
        for (surface, (&(vbase, vn), &(ibase, icount))) in part
            .surface_vertex_ranges
            .iter()
            .zip(&part.surface_index_ranges)
            .enumerate()
        {
            if vn == 0
                || vn > usize::from(u16::MAX)
                || vbase + vn > part.positions.len()
                || ibase + icount > part.indices.len()
            {
                continue;
            }
            let mut positions = Vec::with_capacity(vn);
            let mut normals = Vec::with_capacity(vn);
            let mut infl: Vec<Vec<(u16, f32)>> = Vec::with_capacity(vn);
            for v in vbase..vbase + vn {
                let (p0, n0) = (
                    Vec3::from_array(part.positions[v]),
                    Vec3::from_array(part.normals[v]),
                );
                let (mut p, mut n) = (Vec3::ZERO, Vec3::ZERO);
                let mut w: Vec<(u16, f32)> = Vec::new();
                let skin = part.vert_skin.get(v);
                for k in 0..4 {
                    let Some(skin) = skin else { break };
                    let x = skin.weights[k];
                    let b = usize::from(skin.bones[k]);
                    if x <= 0.0 || b >= moves.len() {
                        continue;
                    }
                    let (turn, from, to, t) = moves[b];
                    p += x * (turn * (p0 - from) + to);
                    n += x * (turn * n0);
                    match w.iter_mut().find(|e| usize::from(e.0) == t) {
                        Some(e) => e.1 += x,
                        None => w.push((t as u16, x)),
                    }
                }
                let sum: f32 = w.iter().map(|e| e.1).sum();
                if w.is_empty() || sum <= 1e-6 {
                    w = vec![(0, 1.0)];
                    p = p0;
                    n = n0;
                } else {
                    p /= sum;
                }
                w.sort_by(|a, b| b.1.total_cmp(&a.1));
                w.truncate(4);
                let sum: f32 = w.iter().map(|e| e.1).sum();
                for e in &mut w {
                    e.1 /= sum.max(1e-6);
                }
                for k in 0..3 {
                    mins[k] = mins[k].min(p[k]);
                    maxs[k] = maxs[k].max(p[k]);
                }
                positions.push(p.to_array());
                normals.push(n.normalize_or(Vec3::Z).to_array());
                infl.push(w);
            }
            // Black Ops II orders a surface's vertices by their bone count.
            let mut order: Vec<usize> = (0..vn).collect();
            order.sort_by_key(|&i| infl[i].len());
            let mut new_of = vec![0u16; vn];
            for (new, &old) in order.iter().enumerate() {
                new_of[old] = new as u16;
            }
            let mut verts = Vec::with_capacity(vn * 32);
            let mut part_bits = [0u32; 6];
            let mut blend_counts = [0i16; 4];
            let mut blend = Vec::new();
            for &old in &order {
                let n = Vec3::from_array(normals[old]);
                verts.extend_from_slice(&super::t7_map::t6_vertex_signed(
                    positions[old],
                    1.0,
                    part.uvs.get(vbase + old).copied().unwrap_or([0.0; 2]),
                    normals[old],
                    n.any_orthonormal_vector().to_array(),
                ));
                let w = &infl[old];
                for &(bone, _) in w {
                    if let Some(word) = part_bits.get_mut(usize::from(bone) / 32) {
                        *word |= 1 << (bone % 32);
                    }
                }
                blend_counts[w.len() - 1] += 1;
                blend.push(w[0].0 * 64);
                for &(b, x) in &w[1..] {
                    blend.push(b * 64);
                    blend.push((x * 65536.0).round().clamp(0.0, 65535.0) as u16);
                }
            }
            lod0.push(asset_t6::XSurfaceRef {
                verts,
                vert_count: vn as u16,
                indices: part.indices[ibase..ibase + icount]
                    .iter()
                    .map(|&i| {
                        new_of
                            .get((i as usize).wrapping_sub(vbase))
                            .copied()
                            .unwrap_or(0)
                    })
                    .collect(),
                vert_lists: Vec::new(),
                flags: 0,
                material: None,
                blend_counts,
                blend,
                part_bits,
            });
            materials.push(names.get(surface).cloned().flatten());
        }
    }
    if lod0.is_empty() {
        return None;
    }
    let radius = (0..3)
        .map(|c| mins[c].abs().max(maxs[c].abs()).powi(2))
        .sum::<f32>()
        .sqrt();
    Some((
        asset_t6::XModelRef {
            name: model_name(s.body),
            numsurfs: lod0.len().min(255) as u8,
            materials: Vec::new(),
            num_bones: bo2.num_bones,
            num_lods: 1,
            mins,
            maxs,
            radius,
            lighting_origin_offset: bo2.lighting_origin_offset,
            lighting_origin_range: bo2.lighting_origin_range,
            lods: vec![(0.0, lod0.len() as u16, 0)],
            lod0,
            bone_names: bo2.bone_names.clone(),
            num_root_bones: bo2.num_root_bones,
            parent_list: bo2.parent_list.clone(),
            quats: bo2.quats.clone(),
            trans: bo2.trans.clone(),
            base_mat: bo2.base_mat.clone(),
            part_classification: bo2.part_classification.clone(),
            bone_info: bo2.bone_info.clone(),
            contents: bo2.contents,
            coll_lod: -1,
            coll_surfs: Vec::new(),
            phys_preset: None,
        },
        materials,
    ))
}

const COLOR_MAP_HASH: u32 = 0xa0ab_1041;
const NORMAL_MAP_HASH: u32 = 0x59d3_0d0f;
const SPECULAR_MAP_HASH: u32 = 0x34ec_ccb3;

/// Links the soldiers' pictures and materials into the match's catalog.
struct Linker<'a> {
    catalog: MaterialCatalog,
    owner: ZoneOwner,
    pictures: &'a HashMap<String, Pictures>,
    materials: HashMap<String, Option<usize>>,
}

impl Linker<'_> {
    fn image(&mut self, key: String, semantic: u8, image: Image) -> usize {
        let colour = semantic == TS_COLOR_MAP;
        let size = image.texture_descriptor.size;
        let levels = image.texture_descriptor.mip_level_count;
        self.catalog.link_image(AuthoredImage {
            namespace: AssetNamespace::T5,
            name: AssetRef::Real(key),
            map_type: 0,
            semantic,
            category: 0,
            use_srgb_reads: colour,
            width: size.width as u16,
            height: size.height as u16,
            depth: 1,
            level_count: levels as u8,
            format: 0,
            payload: Arc::new(Vec::new()),
            decoded: Some(Arc::new(image)),
            common_owned: false,
            decoded_variant: None,
            decoded_by: None,
            pending_decode: None,
        })
    }

    fn material(&mut self, name: &str) -> Option<usize> {
        if let Some(&m) = self.materials.get(name) {
            return m;
        }
        let px = self.pictures.get(name).cloned().unwrap_or_default();
        let mut textures = Vec::new();
        let mut bind = |name_hash, semantic, image| {
            textures.push(MaterialTextureBinding {
                name_hash,
                name_start: 0,
                name_end: 0,
                sampler_state: 0x13,
                semantic,
                image: Some(image),
            });
        };
        let key = format!("t5_sol:{name}");
        if let Some(colour) = px.colour.as_deref() {
            bind(
                COLOR_MAP_HASH,
                TS_COLOR_MAP,
                self.image(key.clone(), TS_COLOR_MAP, colour.clone()),
            );
        }
        if let Some(n) = px.normal.as_deref() {
            bind(
                NORMAL_MAP_HASH,
                TS_NORMAL_MAP,
                self.image(format!("{key}#n"), TS_NORMAL_MAP, n.clone()),
            );
        }
        if let Some(sp) = px.specular.as_deref() {
            bind(
                SPECULAR_MAP_HASH,
                TS_SPECULAR_MAP,
                self.image(format!("{key}#s"), TS_SPECULAR_MAP, sp.clone()),
            );
        }
        let draw = px
            .draw
            .unwrap_or_else(|| asset_core::T6Draw::from_technique_set("", 0, None));
        let index = self.catalog.link_material(AuthoredMaterial {
            name: AssetRef::Real(key),
            namespace: AssetNamespace::T5,
            technique_set: AssetRef::default(),
            technique_set_edge: AssetEdge::Absent,
            draw_surf: 0,
            sort_key: draw.sort_key,
            info_game_flags: 0,
            texture_atlas: None,
            surface_type_bits: None,
            t5_layered_surface_types: None,
            state_flags: 0,
            camera_region: 0,
            state_bits: Vec::new(),
            state_bits_entry: None,
            t5_state_bits_entry: None,
            iw5_state_bits_entry: None,
            technique_table: None,
            route: None,
            textures,
            constants: Vec::new(),
            zone: self.owner,
            t6_draw: Some(draw),
        });
        self.materials.insert(name.to_owned(), Some(index));
        Some(index)
    }
}

/// A GPU picture of levels (RGBA8, or BC5 normals made Black Ops II's).
fn image_of(l: &fastfile_t7::texture::ImageLevels, colour: bool) -> Result<Image, String> {
    super::iw6::gpu_image(
        &fastfile_iw6::image::ImagePixels {
            width: l.width,
            height: l.height,
            levels: l.levels,
            dxgi_format: l.format,
            data: l.data.clone(),
        },
        colour,
    )
}

/// bo2mp: Black Ops' soldiers as models of the match's first-person and
/// body catalog (`fpv`, Black Ops II's namespace), on `bo2`'s skeleton,
/// their materials linked into `catalog`.
pub(crate) fn add_soldiers(
    fpv: &mut FpvMeshBuild,
    catalog: &mut MaterialCatalog,
    bo2: Option<&asset_t6::XModelRef>,
    report: &mut Vec<String>,
) {
    if let Ok(mut b) = BUILT.lock() {
        *b = Default::default();
    }
    if crate::soldiers() != frame::settings::Soldiers::BlackOps {
        return;
    }
    let Some(read) = read() else {
        report.push(
            "bo1 soldiers: Black Ops' Nuketown zone (mp_nuked) not found: Black Ops II's soldiers"
                .into(),
        );
        return;
    };
    report.extend(read.report.iter().cloned());
    let Some(bo2) = bo2 else {
        report.push(
            "bo1 soldiers: no Black Ops II soldier body in the match to take the skeleton from"
                .into(),
        );
        return;
    };
    let mut linker = Linker {
        catalog: std::mem::take(catalog),
        owner: ZoneOwner::from_zone_path(&read.zone),
        pictures: &read.pictures,
        materials: HashMap::new(),
    };
    let (mut allies, mut axis) = (Vec::new(), Vec::new());
    for s in &read.soldiers {
        let Some((mut xref, mats)) = rebind(s, bo2) else {
            report.push(format!("bo1 soldiers: {}: nothing on the skeleton", s.body));
            continue;
        };
        let linked: Vec<Option<usize>> = mats
            .iter()
            .map(|m| linker.material(m.as_deref()?))
            .collect();
        for (i, surface) in xref.lod0.iter_mut().enumerate() {
            surface.material = linked
                .get(i)
                .copied()
                .flatten()
                .map(|_| asset_t6::AssetKey {
                    ty: fastfile_t6::AssetType::Material,
                    index: i,
                });
        }
        xref.materials = xref.lod0.iter().map(|s| s.material).collect();
        let name = xref.name.clone();
        match asset_model::capture_model_skel_t6(&xref, |k| linked.get(k.index).copied().flatten())
        {
            Some(skel) => {
                report.push(format!(
                    "bo1 soldiers: {name}: {} surfaces, {} vertices, on {}'s skeleton",
                    xref.lod0.len(),
                    xref.lod0
                        .iter()
                        .map(|s| usize::from(s.vert_count))
                        .sum::<usize>(),
                    bo2.name
                ));
                fpv.insert_in(AssetNamespace::T6, skel, Some(&linker.catalog));
                if s.axis {
                    axis.push(name);
                } else {
                    allies.push(name);
                }
            }
            None => report.push(format!("bo1 soldiers: {name}: model not built")),
        }
    }
    *catalog = linker.catalog;
    if let Ok(mut built) = BUILT.lock() {
        *built = (allies, axis);
    }
}


/// A Black Ops II soldier body among the match's models: a full body
/// (`*_fb`) with the soldier skeleton.
pub(crate) fn soldier_skeleton<'a>(models: impl Iterator<Item = &'a asset_t6::XModelRef>) -> Option<&'a asset_t6::XModelRef> {
    let mut models = models.filter(|m| {
        m.name.ends_with("_fb")
            && ["j_mainroot", "j_head", "tag_weapon_right", "j_wrist_le"].iter().all(|b| m.bone_names.iter().any(|n| n == b))
    });
    models.next()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_cached_picture_comes_back_as_it_was_made() {
        let made = fastfile_t7::texture::ImageLevels {
            format: 83,
            width: 8,
            height: 4,
            levels: 3,
            data: (0..=255).collect(),
            part: None,
        };
        let back = levels_decode(&levels_encode(&made)).expect("decodes");
        assert_eq!(
            (back.format, back.width, back.height, back.levels),
            (83, 8, 4, 3)
        );
        assert_eq!(back.data, made.data);
        assert!(levels_decode(&levels_encode(&made)[..16]).is_none());
    }
}
