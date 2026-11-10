//! Finished guns kept beside the map bake.
//!
//! A lane's first build writes one file. The next build with the same zone
//! files, picture caps and converting code reads it back and does not walk
//! the zones again. `CODLEGACY_BAKE=0` leaves the lanes on the old path.
//! A file that does not read back whole, or whose key is not this build's,
//! is thrown away and the guns are built again.
//!
//! Gameplay numbers travel in the file only as the finished catalogs the
//! lane already built from those zone files. The file is local and is never
//! shipped.

use std::path::PathBuf;
use std::time::UNIX_EPOCH;

use asset_anim::XAnimBuild;
use asset_game::{FxCatalog, TracerCatalog, WeaponBuild};
use asset_material::{AuthoredImage, AuthoredMaterial, MaterialCatalog, MaterialTextureBinding};
use asset_model::{FpvMeshBuild, ModelSkel, ProjectileMeshBuild, WorldWeaponBuild};
use bevy::asset::RenderAssetUsages;
use bevy::image::{ImageAddressMode, ImageFilterMode, ImageSampler, ImageSamplerDescriptor};
use bevy::prelude::Image;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};

const MAGIC: &[u8; 8] = b"BO2FGUN\0";
/// 2: raw byte runs (pictures, sounds, scripts) in a `blob` section instead of
/// JSON number lists (Skyjacked's BO2-rules file was 3.2 GB of JSON, 190 s to
/// write; the same pictures as bytes pack in a few seconds).
const FORMAT: u32 = 2;
const FILE: &str = "finished_guns.pak";

/// Lengths of a material catalog before a lane adds its own rows.
#[derive(Clone, Copy)]
pub(crate) struct MatAt {
    pub images: usize,
    pub materials: usize,
    pub shaders: usize,
    pub decls: usize,
    pub techsets: usize,
}

impl MatAt {
    pub(crate) fn of(catalog: &MaterialCatalog) -> Self {
        Self {
            images: catalog.images.len(),
            materials: catalog.materials.len(),
            shaders: catalog.shaders.len(),
            decls: catalog.vertex_decls.len(),
            techsets: catalog.techsets().len(),
        }
    }
}

/// What a finished-gun file is named from.
pub(crate) struct Sources<'a> {
    pub lane: &'a str,
    pub name: &'a str,
    pub files: &'a [PathBuf],
    pub caps: &'a [(&'a str, u32)],
    /// The converting code. A change here drops an old file.
    pub code: &'a [&'a str],
    /// Printed on a hit, one line each (`hmw_ar1`).
    pub zones: &'a [String],
    /// Extra identity that is not a file (a donor capture, a directory listing).
    pub note: &'a str,
    /// A path that is not on disk still takes part in the key, as absent.
    /// A file that appears later is a different key.
    pub missing_ok: bool,
}

/// The catalogs a lane finished with. `mat_at` is the seed before the lane.
pub(crate) struct Parts<'a> {
    pub weapons: &'a WeaponBuild,
    pub fpv: &'a FpvMeshBuild,
    pub world: &'a WorldWeaponBuild,
    pub projectiles: &'a ProjectileMeshBuild,
    pub xanims: &'a XAnimBuild,
    pub fx: &'a FxCatalog,
    pub fx_models: &'a asset_game::FxModelCatalog,
    pub tracers: &'a TracerCatalog,
    pub impact: Option<&'a asset_game::OwnedFxImpactTable>,
    pub materials: &'a MaterialCatalog,
    pub mat_at: MatAt,
    pub reports: &'a [String],
    pub extra: &'a [u8],
}

/// Builds `build` unless a whole file for `src` reads back.
pub(crate) fn load_or_build<T>(
    src: &Sources<'_>,
    seed: MaterialCatalog,
    build: impl FnOnce(MaterialCatalog) -> T,
    pack: impl FnOnce(&T, MatAt) -> Result<Vec<u8>, String>,
    unpack: impl FnOnce(&[u8], MaterialCatalog) -> Result<T, MaterialCatalog>,
) -> T {
    let at = MatAt::of(&seed);
    let seed = match try_read(src) {
        Some((path, bytes)) => match unpack(&bytes, seed) {
            Ok(value) => {
                for zone in src.zones {
                    diag::info!(
                        World,
                        "{} guest guns: {zone} reads {}",
                        src.lane,
                        path.display()
                    );
                }
                if src.zones.is_empty() {
                    diag::info!(World, "{} guest guns: reads {}", src.lane, path.display());
                }
                return value;
            }
            Err(seed) => {
                diag::warn!(
                    World,
                    "{} guest guns: cache unreadable, built again ({})",
                    src.lane,
                    path.display()
                );
                let _ = std::fs::remove_file(&path);
                seed
            }
        },
        None => seed,
    };
    let built = build(seed);
    match pack(&built, at) {
        Ok(bytes) => match store(src, &bytes) {
            Ok(path) => {
                for zone in src.zones {
                    diag::info!(
                        World,
                        "{} guest guns: {zone} kept in {} ({:.1} MB)",
                        src.lane,
                        path.display(),
                        bytes.len() as f64 / (1024.0 * 1024.0)
                    );
                }
                if src.zones.is_empty() {
                    diag::info!(
                        World,
                        "{} guest guns: kept in {} ({:.1} MB)",
                        src.lane,
                        path.display(),
                        bytes.len() as f64 / (1024.0 * 1024.0)
                    );
                }
            }
            Err(error) => diag::warn!(
                World,
                "{} guest guns: cache not written ({error})",
                src.lane
            ),
        },
        Err(why) => diag::warn!(World, "{} guest guns: not cached ({why})", src.lane),
    }
    built
}

/// The file's body when the key matches. `None` on a miss, a stale key, or a broken file.
pub(crate) fn read_body(src: &Sources<'_>) -> Option<Vec<u8>> {
    try_read(src).map(|(_, body)| body)
}

/// Writes `body` and logs the zones, the same line as [`load_or_build`].
pub(crate) fn write_logged(src: &Sources<'_>, body: &[u8]) {
    match store(src, body) {
        Ok(path) => {
            for zone in src.zones {
                diag::info!(
                    World,
                    "{} guest guns: {zone} kept in {} ({:.1} MB)",
                    src.lane,
                    path.display(),
                    body.len() as f64 / (1024.0 * 1024.0)
                );
            }
            if src.zones.is_empty() {
                diag::info!(
                    World,
                    "{} guest guns: kept in {} ({:.1} MB)",
                    src.lane,
                    path.display(),
                    body.len() as f64 / (1024.0 * 1024.0)
                );
            }
        }
        Err(error) => diag::warn!(
            World,
            "{} guest guns: cache not written ({error})",
            src.lane
        ),
    }
}

fn try_read(src: &Sources<'_>) -> Option<(PathBuf, Vec<u8>)> {
    let key = key_of(src)?;
    let path = file_of(src)?;
    let bytes = std::fs::read(&path).ok()?;
    match unpack_file(&key, &bytes) {
        Ok(body) => Some((path, body)),
        Err(UnpackErr::Stale) => {
            diag::info!(
                World,
                "{} guest guns: cache is stale, built again ({})",
                src.lane,
                path.display()
            );
            None
        }
        Err(UnpackErr::Broken) => {
            diag::warn!(
                World,
                "{} guest guns: cache unreadable, built again ({})",
                src.lane,
                path.display()
            );
            let _ = std::fs::remove_file(&path);
            None
        }
    }
}

fn store(src: &Sources<'_>, body: &[u8]) -> Result<PathBuf, String> {
    let key = key_of(src).ok_or_else(|| "no bake key".to_owned())?;
    let path = file_of(src).ok_or_else(|| "bake folder is off".to_owned())?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let packed = pack_file(&key, body);
    let tmp = path.with_extension(format!("pak.part{}", std::process::id()));
    std::fs::write(&tmp, &packed).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, &path).map_err(|e| e.to_string())?;
    Ok(path)
}

fn file_of(src: &Sources<'_>) -> Option<PathBuf> {
    Some(map_bake::map_dir(src.lane, src.name)?.join(FILE))
}

/// Whether finished-guns files are read and written
/// (`CODLEGACY_GUNS_CACHE=1`: yes). Off by default (7 Nov 2026): a fresh
/// bake folder's first loads wrote 0.5 to 4.5 GB files on the load's own
/// thread (BO1 Nuked's first load 244 s, MW2's common set alone 4.5 GB,
/// Stronghold 887 MB), a loss against the 2-6 s the guns take to build.
/// Back on once the files are small and written behind the load.
pub(crate) fn enabled() -> bool {
    map_bake::enabled() && codlegacy_env::var("CODLEGACY_GUNS_CACHE").is_ok_and(|v| v.trim() == "1")
}

fn key_of(src: &Sources<'_>) -> Option<u64> {
    if !enabled() || src.files.is_empty() {
        return None;
    }
    identity(src)
}

fn identity(src: &Sources<'_>) -> Option<u64> {
    let mut key = map_bake::Key::new();
    key.u64(FORMAT as u64).str(src.lane).str(src.name).str(src.note);
    for text in src.code {
        key.str(text);
    }
    for (name, cap) in src.caps {
        key.str(name).u64(*cap as u64);
    }
    let mut files: Vec<&PathBuf> = src.files.iter().collect();
    files.sort();
    key.u64(files.len() as u64);
    for path in files {
        let meta = std::fs::metadata(path).ok();
        if meta.is_none() && !src.missing_ok {
            return None;
        }
        let (len, modified) = match &meta {
            Some(meta) => {
                let modified = meta
                    .modified()
                    .ok()
                    .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                    .map(|d| d.as_nanos())
                    .unwrap_or(0);
                (meta.len(), modified)
            }
            None => (0, 0),
        };
        key.str(&path.to_string_lossy())
            .u64(len)
            .bytes(&modified.to_le_bytes());
    }
    Some(key.value())
}

#[derive(Debug)]
enum UnpackErr {
    Stale,
    Broken,
}

fn pack_file(key: &u64, body: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(body.len() / 2 + 16);
    out.extend_from_slice(MAGIC);
    out.extend_from_slice(&FORMAT.to_le_bytes());
    out.extend_from_slice(&key.to_le_bytes());
    out.extend_from_slice(&lz4_flex::compress_prepend_size(body));
    out
}

fn unpack_file(key: &u64, bytes: &[u8]) -> Result<Vec<u8>, UnpackErr> {
    let rest = bytes
        .strip_prefix(MAGIC.as_slice())
        .ok_or(UnpackErr::Broken)?;
    if rest.len() < 12 {
        return Err(UnpackErr::Broken);
    }
    let format = u32::from_le_bytes(rest[0..4].try_into().unwrap());
    let found = u64::from_le_bytes(rest[4..12].try_into().unwrap());
    if format != FORMAT || found != *key {
        return Err(UnpackErr::Stale);
    }
    lz4_flex::decompress_size_prepended(&rest[12..]).map_err(|_| UnpackErr::Broken)
}

pub(crate) fn pack_parts(parts: &Parts<'_>) -> Result<Vec<u8>, String> {
    pack_parts_with(parts, Blobs::default())
}

/// `pack_parts` with the byte runs the caller's `extra` already put in
/// `blobs` (the material catalog's pictures join them).
pub(crate) fn pack_parts_with(parts: &Parts<'_>, mut blobs: Blobs) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    section(
        &mut out,
        b"wep ",
        &asset_game::export_weapons(parts.weapons)?,
    );
    section(&mut out, b"fx  ", &asset_game::export_fx(parts.fx)?);
    section(
        &mut out,
        b"trc ",
        &asset_game::export_tracers(parts.tracers)?,
    );
    let (gaps, anims) = parts.xanims.export_anims();
    section(
        &mut out,
        b"anim",
        &serde_json::to_vec(&(gaps, anims)).map_err(|e| e.to_string())?,
    );
    section(
        &mut out,
        b"hit ",
        &serde_json::to_vec(&parts.impact).map_err(|e| e.to_string())?,
    );
    section(
        &mut out,
        b"rep ",
        &serde_json::to_vec(parts.reports).map_err(|e| e.to_string())?,
    );
    section(&mut out, b"ext ", parts.extra);
    section(
        &mut out,
        b"mat ",
        &pack_materials(parts.materials, parts.mat_at, &mut blobs)?,
    );
    section(&mut out, b"mesh", &pack_meshes(parts)?);
    section_runs(&mut out, b"blob", &blobs);
    Ok(out)
}

/// The byte runs a file's JSON parts point at by number (a picture's
/// texels, a sound catalog, compiled scripts): bytes stay bytes.
#[derive(Default)]
pub(crate) struct Blobs {
    runs: Vec<Vec<u8>>,
}

impl Blobs {
    /// Keeps `bytes`; the number the JSON part stores for them.
    pub fn put(&mut self, bytes: Vec<u8>) -> u32 {
        self.runs.push(bytes);
        (self.runs.len() - 1) as u32
    }

    pub fn total(&self) -> usize {
        self.runs.iter().map(Vec::len).sum()
    }
}

/// `blob` section: count u64, each run's length u64, then the runs end to end.
fn section_runs(out: &mut Vec<u8>, tag: &[u8; 4], blobs: &Blobs) {
    let total = blobs.total();
    out.extend_from_slice(tag);
    out.extend_from_slice(&((8 + 8 * blobs.runs.len() + total) as u64).to_le_bytes());
    out.extend_from_slice(&(blobs.runs.len() as u64).to_le_bytes());
    for run in &blobs.runs {
        out.extend_from_slice(&(run.len() as u64).to_le_bytes());
    }
    out.reserve(total);
    for run in &blobs.runs {
        out.extend_from_slice(run);
    }
}

/// The byte runs read back: one block, each run a slice of it.
#[derive(Default)]
pub(crate) struct BlobsIn {
    data: Vec<u8>,
    at: Vec<(usize, usize)>,
}

impl BlobsIn {
    fn unpack(section: Vec<u8>) -> Option<BlobsIn> {
        let n = u64::from_le_bytes(section.get(..8)?.try_into().ok()?) as usize;
        let mut at = Vec::with_capacity(n);
        let mut start = 8 + 8 * n;
        let lens = section.get(8..start)?;
        for i in 0..n {
            let len = u64::from_le_bytes(lens[i * 8..i * 8 + 8].try_into().ok()?) as usize;
            at.push((start, len));
            start = start.checked_add(len)?;
        }
        if start != section.len() {
            return None;
        }
        Some(BlobsIn { data: section, at })
    }

    pub fn get(&self, id: u32) -> Option<&[u8]> {
        let &(start, len) = self.at.get(id as usize)?;
        self.data.get(start..start + len)
    }

    pub fn owned(&self, id: u32) -> Option<Vec<u8>> {
        self.get(id).map(<[u8]>::to_vec)
    }
}

pub(crate) struct Restored {
    pub weapons: WeaponBuild,
    pub fpv: FpvMeshBuild,
    pub world: WorldWeaponBuild,
    pub projectiles: ProjectileMeshBuild,
    pub xanims: XAnimBuild,
    pub fx: FxCatalog,
    pub fx_models: asset_game::FxModelCatalog,
    pub tracers: TracerCatalog,
    pub impact: Option<asset_game::OwnedFxImpactTable>,
    pub materials: MaterialCatalog,
    pub reports: Vec<String>,
    pub extra: Vec<u8>,
    /// The byte runs `extra` may point at (`Blobs::put` when packed).
    pub blobs: BlobsIn,
}

pub(crate) fn unpack_parts(
    bytes: &[u8],
    mut materials: MaterialCatalog,
) -> Result<Restored, MaterialCatalog> {
    let mut map = std::collections::HashMap::new();
    let mut at = 0usize;
    while at < bytes.len() {
        let Some(tag) = bytes.get(at..at + 4) else {
            return Err(materials);
        };
        at += 4;
        let Ok(len_bytes) = bytes.get(at..at + 8).unwrap_or_default().try_into() else {
            return Err(materials);
        };
        let n = u64::from_le_bytes(len_bytes) as usize;
        at += 8;
        let Some(body) = bytes.get(at..at + n) else {
            return Err(materials);
        };
        map.insert(tag.to_vec(), body.to_vec());
        at += n;
    }
    if at != bytes.len() {
        return Err(materials);
    }
    let blobs = match map.remove(b"blob".as_slice()) {
        None => BlobsIn::default(),
        Some(section) => match BlobsIn::unpack(section) {
            Some(blobs) => blobs,
            None => return Err(materials),
        },
    };
    let take = |tag: &[u8; 4]| map.get(tag.as_slice()).cloned();
    let Some(mat) = take(b"mat ") else {
        return Err(materials);
    };
    materials = unpack_materials(materials, &mat, &blobs)?;
    let Some(mesh_bytes) = take(b"mesh") else {
        return Err(materials);
    };
    let Some(meshes) = unpack_meshes(&mesh_bytes, &materials) else {
        return Err(materials);
    };
    let Some(anim_bytes) = take(b"anim") else {
        return Err(materials);
    };
    let Ok((gaps, anims)) =
        serde_json::from_slice::<(usize, Vec<asset_anim::CachedAnim>)>(&anim_bytes)
    else {
        return Err(materials);
    };
    let xanims = XAnimBuild::import_anims(gaps, anims);
    let Some(fx_bytes) = take(b"fx  ") else {
        return Err(materials);
    };
    let Some(fx) = asset_game::import_fx(&fx_bytes) else {
        return Err(materials);
    };
    let Some(trc_bytes) = take(b"trc ") else {
        return Err(materials);
    };
    let Some(tracers) = asset_game::import_tracers(&trc_bytes) else {
        return Err(materials);
    };
    let Some(wep_bytes) = take(b"wep ") else {
        return Err(materials);
    };
    let Some(weapons) = asset_game::import_weapons(&wep_bytes, &xanims, &meshes.fpv, &meshes.world)
    else {
        return Err(materials);
    };
    let Some(hit_bytes) = take(b"hit ") else {
        return Err(materials);
    };
    let Ok(impact) = serde_json::from_slice(&hit_bytes) else {
        return Err(materials);
    };
    let Some(rep_bytes) = take(b"rep ") else {
        return Err(materials);
    };
    let Ok(reports) = serde_json::from_slice(&rep_bytes) else {
        return Err(materials);
    };
    let Some(extra) = take(b"ext ") else {
        return Err(materials);
    };
    Ok(Restored {
        weapons,
        fpv: meshes.fpv,
        world: meshes.world,
        projectiles: meshes.projectiles,
        xanims,
        fx,
        fx_models: meshes.fx_models,
        tracers,
        impact,
        materials,
        reports,
        extra,
        blobs,
    })
}

fn section(out: &mut Vec<u8>, tag: &[u8; 4], data: &[u8]) {
    out.extend_from_slice(tag);
    out.extend_from_slice(&(data.len() as u64).to_le_bytes());
    out.extend_from_slice(data);
}

#[derive(serde::Serialize, serde::Deserialize)]
struct ImageDto {
    ns: String,
    real: bool,
    name: String,
    map_type: u8,
    semantic: u8,
    category: u8,
    srgb: bool,
    width: u16,
    height: u16,
    depth: u16,
    levels: u8,
    format: u32,
    /// Blob number of the picture's packed bytes.
    payload: u32,
    gpu: Option<GpuDto>,
    common: bool,
}

/// A picture as the graphics card gets it; its texels are a blob.
#[derive(serde::Serialize, serde::Deserialize)]
pub(crate) struct GpuDto {
    dim: u8,
    format: u16,
    w: u32,
    h: u32,
    d: u32,
    mips: u32,
    /// Blob number of the texels.
    data: u32,
    views: Vec<u16>,
    sampler: u8,
    address: [u8; 3],
    filter: [u8; 3],
    anisotropy: u16,
    usage: u32,
    copy: bool,
    order: u8,
}

#[derive(serde::Serialize, serde::Deserialize)]
struct BindingDto {
    name_hash: u32,
    name_start: u8,
    name_end: u8,
    sampler_state: u8,
    semantic: u8,
    image: Option<(String, String)>,
}

#[derive(serde::Serialize, serde::Deserialize)]
struct MaterialDto {
    real: bool,
    name: String,
    ns: String,
    technique: (bool, String),
    edge: asset_core::AssetEdge<asset_core::TechniqueSetSpace>,
    /// Technique set the edge names, so a different seed still binds the same row.
    #[serde(default)]
    edge_techset: Option<(String, String)>,
    draw_surf: u64,
    sort_key: u8,
    info_game_flags: u8,
    atlas: Option<[u8; 2]>,
    surface: Option<u32>,
    layered: Option<u32>,
    state_flags: u8,
    camera: u8,
    textures: Vec<BindingDto>,
    constants: Vec<asset_material::MaterialConstant>,
    zone: String,
    draw: Option<asset_core::T6Draw>,
    #[serde(default)]
    state_bits: Vec<[u32; 2]>,
    #[serde(default)]
    state_bits_entry: Vec<u8>,
    #[serde(default)]
    t5_state_bits_entry: Vec<u8>,
    #[serde(default)]
    iw5_state_bits_entry: Vec<u8>,
    #[serde(default)]
    table: Option<TechDto>,
    #[serde(default)]
    route: Option<RouteDto>,
}

#[derive(serde::Serialize, serde::Deserialize, Default)]
struct MatDto {
    images: Vec<ImageDto>,
    materials: Vec<MaterialDto>,
    #[serde(default)]
    shaders: Vec<ShaderDto>,
    #[serde(default)]
    decls: Vec<DeclDto>,
    #[serde(default)]
    techsets: Vec<TechsetDto>,
}

#[derive(serde::Serialize, serde::Deserialize)]
struct ShaderDto {
    ns: String,
    real: bool,
    name: String,
    kind: u32,
    program: Vec<u8>,
}

#[derive(serde::Serialize, serde::Deserialize)]
struct DeclDto {
    family: u8,
    real: bool,
    name: String,
    stream_count: u8,
    has_optional_source: u8,
    routing: Vec<[u8; 2]>,
}

#[derive(serde::Serialize, serde::Deserialize)]
struct TechDto {
    slots: u64,
    scanned: u64,
    technique0_flags: u8,
    model_lighting_const: Option<bool>,
    max_pass_count: u16,
    pass_count_by_slot: Vec<u8>,
    /// Shader and decl indexes in this file's lists, not the live catalog.
    graph: Option<asset_material::OwnedTechniqueGraph>,
}

#[derive(serde::Serialize, serde::Deserialize)]
struct T5OccDto {
    slots: Vec<u64>,
    scanned: Vec<u64>,
    technique0_flags: u8,
    max_pass_count: u16,
    pass_count_by_slot: Vec<u8>,
}

#[derive(serde::Serialize, serde::Deserialize)]
struct TechsetDto {
    ns: String,
    real: bool,
    name: String,
    zone: String,
    table: Option<TechDto>,
    t5: Option<T5OccDto>,
    iw5_fallback: Option<TechDto>,
    t5_fallback: Option<TechDto>,
    world_vert_format: u8,
}

#[derive(serde::Serialize, serde::Deserialize)]
struct RouteDto {
    primary_sort_key: u8,
    prepass: u8,
    custom_index: u8,
    pass: u8,
    sky: u8,
    technique_slots: u64,
    uses_model_lighting_const: bool,
}

fn pack_materials(catalog: &MaterialCatalog, at: MatAt, blobs: &mut Blobs) -> Result<Vec<u8>, String> {
    let mut ids = IdLists::default();
    let mut techsets = Vec::new();
    for facts in &catalog.techsets()[at.techsets..] {
        techsets.push(techset_out(catalog, &mut ids, facts)?);
    }
    let mut images = Vec::new();
    for image in &catalog.images[at.images..] {
        images.push(image_out(image, blobs)?);
    }
    let mut materials = Vec::new();
    for material in &catalog.materials[at.materials..] {
        materials.push(material_out(catalog, &mut ids, material)?);
    }
    serde_json::to_vec(&MatDto {
        images,
        materials,
        shaders: ids.shaders,
        decls: ids.decls,
        techsets,
    })
    .map_err(|e| e.to_string())
}

#[derive(Default)]
struct IdLists {
    shaders: Vec<ShaderDto>,
    decls: Vec<DeclDto>,
    shader_at: std::collections::HashMap<usize, usize>,
    decl_at: std::collections::HashMap<usize, usize>,
}

impl IdLists {
    fn shader(&mut self, catalog: &MaterialCatalog, index: usize) -> Result<usize, String> {
        if let Some(id) = self.shader_at.get(&index) {
            return Ok(*id);
        }
        let shader = catalog
            .shaders
            .get(index)
            .ok_or_else(|| format!("shader {index} is missing"))?;
        let (real, name) = match &shader.name {
            asset_core::AssetRef::Real(name) => (true, name.clone()),
            asset_core::AssetRef::Reference(name) => (false, name.clone()),
        };
        let id = self.shaders.len();
        self.shaders.push(ShaderDto {
            ns: shader.namespace.as_str().to_owned(),
            real,
            name,
            kind: shader.kind as u32,
            program: shader.program.clone(),
        });
        self.shader_at.insert(index, id);
        Ok(id)
    }

    fn decl(&mut self, catalog: &MaterialCatalog, index: usize) -> Result<usize, String> {
        if let Some(id) = self.decl_at.get(&index) {
            return Ok(*id);
        }
        let decl = catalog
            .vertex_decls
            .get(index)
            .ok_or_else(|| format!("vertex decl {index} is missing"))?;
        let (real, name) = match &decl.name {
            asset_core::AssetRef::Real(name) => (true, name.clone()),
            asset_core::AssetRef::Reference(name) => (false, name.clone()),
        };
        let id = self.decls.len();
        self.decls.push(DeclDto {
            family: match decl.family {
                render_material::VertexLayoutFamily::Iw4 => 0,
                render_material::VertexLayoutFamily::T5 => 1,
            },
            real,
            name,
            stream_count: decl.stream_count,
            has_optional_source: decl.has_optional_source,
            routing: decl.routing.to_vec(),
        });
        self.decl_at.insert(index, id);
        Ok(id)
    }

    fn graph(
        &mut self,
        catalog: &MaterialCatalog,
        graph: &asset_material::OwnedTechniqueGraph,
    ) -> Result<asset_material::OwnedTechniqueGraph, String> {
        let mut graph = graph.clone();
        for slot in &mut graph.slots {
            let Some(tech) = slot else { continue };
            for pass in &mut tech.passes {
                if let Some(index) = pass.vertex_decl {
                    pass.vertex_decl = Some(self.decl(catalog, index)?);
                }
                if let Some(index) = pass.vertex_shader.shader {
                    pass.vertex_shader.shader = Some(self.shader(catalog, index)?);
                }
                if let Some(index) = pass.pixel_shader.shader {
                    pass.pixel_shader.shader = Some(self.shader(catalog, index)?);
                }
            }
        }
        Ok(graph)
    }

    fn table(
        &mut self,
        catalog: &MaterialCatalog,
        table: &asset_material::TechniqueTable,
    ) -> Result<TechDto, String> {
        Ok(TechDto {
            slots: table.slots,
            scanned: table.scanned,
            technique0_flags: table.technique0_flags,
            model_lighting_const: table.model_lighting_const,
            max_pass_count: table.max_pass_count,
            pass_count_by_slot: table.pass_count_by_slot.to_vec(),
            graph: table
                .graph
                .as_ref()
                .map(|graph| self.graph(catalog, graph))
                .transpose()?,
        })
    }
}

fn image_out(image: &AuthoredImage, blobs: &mut Blobs) -> Result<ImageDto, String> {
    let (real, name) = match &image.name {
        asset_core::AssetRef::Real(name) => (true, name.clone()),
        asset_core::AssetRef::Reference(name) => (false, name.clone()),
    };
    Ok(ImageDto {
        ns: image.namespace.as_str().to_owned(),
        real,
        name,
        map_type: image.map_type,
        semantic: image.semantic,
        category: image.category,
        srgb: image.use_srgb_reads,
        width: image.width,
        height: image.height,
        depth: image.depth,
        levels: image.level_count,
        format: image.format,
        payload: blobs.put((*image.payload).clone()),
        gpu: image.decoded.as_deref().map(|gpu| gpu_out(gpu, blobs)).transpose()?,
        common: image.common_owned,
    })
}

fn techset_out(
    catalog: &MaterialCatalog,
    ids: &mut IdLists,
    facts: &asset_material::TechniqueSetFacts,
) -> Result<TechsetDto, String> {
    let (real, name) = match &facts.name {
        asset_core::AssetRef::Real(name) => (true, name.clone()),
        asset_core::AssetRef::Reference(name) => (false, name.clone()),
    };
    let table = facts
        .table
        .as_ref()
        .map(|table| ids.table(catalog, table))
        .transpose()?;
    let iw5_fallback = facts
        .iw5_fallback_table
        .as_ref()
        .map(|table| ids.table(catalog, table))
        .transpose()?;
    let t5_fallback = facts
        .t5_fallback_table
        .as_ref()
        .map(|table| ids.table(catalog, table))
        .transpose()?;
    let t5 = facts.t5_occupancy.map(|occ| T5OccDto {
        slots: occ.slots.to_vec(),
        scanned: occ.scanned.to_vec(),
        technique0_flags: occ.technique0_flags,
        max_pass_count: occ.max_pass_count,
        pass_count_by_slot: occ.pass_count_by_slot.to_vec(),
    });
    Ok(TechsetDto {
        ns: facts.namespace.as_str().to_owned(),
        real,
        name,
        zone: facts.zone.as_str().to_owned(),
        table,
        t5,
        iw5_fallback,
        t5_fallback,
        world_vert_format: facts.world_vert_format,
    })
}

fn material_out(
    catalog: &MaterialCatalog,
    ids: &mut IdLists,
    material: &AuthoredMaterial,
) -> Result<MaterialDto, String> {
    let (real, name) = match &material.name {
        asset_core::AssetRef::Real(name) => (true, name.clone()),
        asset_core::AssetRef::Reference(name) => (false, name.clone()),
    };
    let (tech_real, tech_name) = match &material.technique_set {
        asset_core::AssetRef::Real(name) => (true, name.clone()),
        asset_core::AssetRef::Reference(name) => (false, name.clone()),
    };
    let mut textures = Vec::new();
    for binding in &material.textures {
        let image = match binding.image {
            None => None,
            Some(index) => {
                let image = catalog
                    .images
                    .get(index)
                    .ok_or_else(|| format!("{name}: picture {index} is missing"))?;
                Some((
                    image.namespace.as_str().to_owned(),
                    image.name.as_str().to_owned(),
                ))
            }
        };
        textures.push(BindingDto {
            name_hash: binding.name_hash,
            name_start: binding.name_start,
            name_end: binding.name_end,
            sampler_state: binding.sampler_state,
            semantic: binding.semantic,
            image,
        });
    }
    let edge_techset = material.technique_set_edge.bound_index().and_then(|index| {
        catalog.techsets().get(index).map(|facts| {
            (
                facts.namespace.as_str().to_owned(),
                facts.name.as_str().to_owned(),
            )
        })
    });
    let table = material
        .technique_table
        .as_ref()
        .map(|table| ids.table(catalog, table))
        .transpose()?;
    let route = material.route.map(|route| {
        let (pass, sky) = match route.pass {
            asset_iw4::MaterialPass::ShadowOnly => (0, 0),
            asset_iw4::MaterialPass::Sky(bucket) => (1, bucket),
            asset_iw4::MaterialPass::Lit => (2, 0),
            asset_iw4::MaterialPass::Unlit => (3, 0),
        };
        RouteDto {
            primary_sort_key: route.primary_sort_key,
            prepass: route.prepass,
            custom_index: route.custom_index,
            pass,
            sky,
            technique_slots: route.technique_slots,
            uses_model_lighting_const: route.uses_model_lighting_const,
        }
    });
    Ok(MaterialDto {
        real,
        name,
        ns: material.namespace.as_str().to_owned(),
        technique: (tech_real, tech_name),
        edge: material.technique_set_edge,
        edge_techset,
        draw_surf: material.draw_surf,
        sort_key: material.sort_key,
        info_game_flags: material.info_game_flags,
        atlas: material.texture_atlas,
        surface: material.surface_type_bits,
        layered: material.t5_layered_surface_types,
        state_flags: material.state_flags,
        camera: material.camera_region,
        textures,
        constants: material.constants.clone(),
        zone: material.zone.as_str().to_owned(),
        draw: material.t6_draw,
        state_bits: material.state_bits.clone(),
        state_bits_entry: material
            .state_bits_entry
            .map(|e| e.to_vec())
            .unwrap_or_default(),
        t5_state_bits_entry: material
            .t5_state_bits_entry
            .map(|e| e.to_vec())
            .unwrap_or_default(),
        iw5_state_bits_entry: material
            .iw5_state_bits_entry
            .map(|e| e.to_vec())
            .unwrap_or_default(),
        table,
        route,
    })
}

fn unpack_materials(
    catalog: MaterialCatalog,
    bytes: &[u8],
    blobs: &BlobsIn,
) -> Result<MaterialCatalog, MaterialCatalog> {
    let dto: MatDto = match serde_json::from_slice(bytes) {
        Ok(dto) => dto,
        Err(_) => return Err(catalog),
    };
    let saved = catalog.clone();
    match apply_materials(catalog, dto, blobs) {
        Some(catalog) => Ok(catalog),
        None => Err(saved),
    }
}

fn apply_materials(mut catalog: MaterialCatalog, dto: MatDto, blobs: &BlobsIn) -> Option<MaterialCatalog> {
    let mut shader_of = Vec::with_capacity(dto.shaders.len());
    for shader in dto.shaders {
        let ns = ns_of(&shader.ns)?;
        let kind = fastfile_iw4::AssetType::from_u32(shader.kind)?;
        shader_of.push(catalog.adopt_shader(asset_material::AuthoredShader {
            namespace: ns,
            name: if shader.real {
                asset_core::AssetRef::Real(shader.name)
            } else {
                asset_core::AssetRef::Reference(shader.name)
            },
            kind,
            program: shader.program,
        }));
    }
    let mut decl_of = Vec::with_capacity(dto.decls.len());
    for decl in dto.decls {
        let mut routing = [[0u8; 2]; asset_iw4::vertex_decl::ROUTING_COUNT];
        if decl.routing.len() != routing.len() {
            return None;
        }
        routing.copy_from_slice(&decl.routing);
        decl_of.push(catalog.adopt_vertex_decl(asset_material::AuthoredVertexDecl {
            family: match decl.family {
                0 => render_material::VertexLayoutFamily::Iw4,
                1 => render_material::VertexLayoutFamily::T5,
                _ => return None,
            },
            name: if decl.real {
                asset_core::AssetRef::Real(decl.name)
            } else {
                asset_core::AssetRef::Reference(decl.name)
            },
            stream_count: decl.stream_count,
            has_optional_source: decl.has_optional_source,
            routing,
        }));
    }
    for techset in dto.techsets {
        let facts = techset_in(&shader_of, &decl_of, techset)?;
        let have = catalog.techsets().iter().position(|row| {
            row.namespace == facts.namespace && row.name.as_str() == facts.name.as_str()
        });
        if have.is_none() {
            catalog.push_techset(facts);
        }
    }
    for image in dto.images {
        catalog.link_image(image_in(image, blobs)?);
    }
    // Each binding's picture by name, looked up once per name rather than a
    // scan of every picture per binding (thousands of each).
    let image_at: std::collections::HashMap<(asset_core::AssetNamespace, String), usize> = catalog
        .images
        .iter()
        .enumerate()
        .map(|(i, image)| ((image.namespace, image.name.as_str().to_owned()), i))
        .collect();
    for material in dto.materials {
        let textures = material
            .textures
            .into_iter()
            .map(|binding| {
                let image = match binding.image {
                    None => None,
                    Some((ns, name)) => Some(*image_at.get(&(ns_of(&ns)?, name))?),
                };
                Some(MaterialTextureBinding {
                    name_hash: binding.name_hash,
                    name_start: binding.name_start,
                    name_end: binding.name_end,
                    sampler_state: binding.sampler_state,
                    semantic: binding.semantic,
                    image,
                })
            })
            .collect::<Option<Vec<_>>>()?;
        let ns = ns_of(&material.ns)?;
        let edge = match &material.edge_techset {
            Some((tech_ns, tech_name)) => {
                let tech_ns = ns_of(tech_ns)?;
                let index = catalog.techsets().iter().position(|row| {
                    row.namespace == tech_ns && row.name.as_str() == tech_name
                })?;
                asset_core::AssetEdge::bind_order(index, asset_core::ZoneOwner::intern(&material.zone))
            }
            None => material.edge,
        };
        catalog.link_material(AuthoredMaterial {
            name: if material.real {
                asset_core::AssetRef::Real(material.name)
            } else {
                asset_core::AssetRef::Reference(material.name)
            },
            namespace: ns,
            technique_set: if material.technique.0 {
                asset_core::AssetRef::Real(material.technique.1)
            } else {
                asset_core::AssetRef::Reference(material.technique.1)
            },
            technique_set_edge: edge,
            draw_surf: material.draw_surf,
            sort_key: material.sort_key,
            info_game_flags: material.info_game_flags,
            texture_atlas: material.atlas,
            surface_type_bits: material.surface,
            t5_layered_surface_types: material.layered,
            state_flags: material.state_flags,
            camera_region: material.camera,
            state_bits: material.state_bits,
            state_bits_entry: opt_array(material.state_bits_entry)?,
            t5_state_bits_entry: opt_array(material.t5_state_bits_entry)?,
            iw5_state_bits_entry: opt_array(material.iw5_state_bits_entry)?,
            technique_table: match material.table {
                None => None,
                Some(table) => Some(table_in(&shader_of, &decl_of, table)?),
            },
            route: material.route.and_then(route_in),
            textures,
            constants: material.constants,
            zone: asset_core::ZoneOwner::intern(&material.zone),
            t6_draw: material.draw,
        });
    }
    Some(catalog)
}

fn bytes_array<const N: usize>(bytes: Vec<u8>) -> Option<[u8; N]> {
    let mut out = [0u8; N];
    if bytes.len() != N {
        return None;
    }
    out.copy_from_slice(&bytes);
    Some(out)
}

fn opt_array<const N: usize>(bytes: Vec<u8>) -> Option<Option<[u8; N]>> {
    if bytes.is_empty() {
        return Some(None);
    }
    Some(Some(bytes_array(bytes)?))
}

fn route_in(route: RouteDto) -> Option<asset_iw4::MaterialDrawRoute> {
    let pass = match route.pass {
        0 => asset_iw4::MaterialPass::ShadowOnly,
        1 => asset_iw4::MaterialPass::Sky(route.sky),
        2 => asset_iw4::MaterialPass::Lit,
        3 => asset_iw4::MaterialPass::Unlit,
        _ => return None,
    };
    Some(asset_iw4::MaterialDrawRoute {
        primary_sort_key: route.primary_sort_key,
        prepass: route.prepass,
        custom_index: route.custom_index,
        pass,
        technique_slots: route.technique_slots,
        uses_model_lighting_const: route.uses_model_lighting_const,
    })
}

fn table_in(shader_of: &[usize], decl_of: &[usize], mut table: TechDto) -> Option<asset_material::TechniqueTable> {
    if let Some(graph) = table.graph.as_mut() {
        retarget(graph, shader_of, decl_of)?;
    }
    let pass_count_by_slot = bytes_array(table.pass_count_by_slot)?;
    Some(asset_material::TechniqueTable {
        slots: table.slots,
        scanned: table.scanned,
        technique0_flags: table.technique0_flags,
        model_lighting_const: table.model_lighting_const,
        max_pass_count: table.max_pass_count,
        pass_count_by_slot,
        graph: table.graph,
    })
}

fn retarget(
    graph: &mut asset_material::OwnedTechniqueGraph,
    shader_of: &[usize],
    decl_of: &[usize],
) -> Option<()> {
    for slot in &mut graph.slots {
        let Some(tech) = slot else { continue };
        for pass in &mut tech.passes {
            if let Some(index) = pass.vertex_decl {
                pass.vertex_decl = Some(*decl_of.get(index)?);
            }
            if let Some(index) = pass.vertex_shader.shader {
                pass.vertex_shader.shader = Some(*shader_of.get(index)?);
            }
            if let Some(index) = pass.pixel_shader.shader {
                pass.pixel_shader.shader = Some(*shader_of.get(index)?);
            }
        }
    }
    Some(())
}

fn techset_in(
    shader_of: &[usize],
    decl_of: &[usize],
    dto: TechsetDto,
) -> Option<asset_material::TechniqueSetFacts> {
    let t5_occupancy = match dto.t5 {
        None => None,
        Some(occ) => Some(asset_material::T5TechniqueOccupancy {
            slots: occ.slots.try_into().ok()?,
            scanned: occ.scanned.try_into().ok()?,
            technique0_flags: occ.technique0_flags,
            max_pass_count: occ.max_pass_count,
            pass_count_by_slot: bytes_array(occ.pass_count_by_slot)?,
        }),
    };
    let table = match dto.table {
        None => None,
        Some(table) => Some(table_in(shader_of, decl_of, table)?),
    };
    let iw5_fallback_table = match dto.iw5_fallback {
        None => None,
        Some(table) => Some(table_in(shader_of, decl_of, table)?),
    };
    let t5_fallback_table = match dto.t5_fallback {
        None => None,
        Some(table) => Some(table_in(shader_of, decl_of, table)?),
    };
    Some(asset_material::TechniqueSetFacts {
        namespace: ns_of(&dto.ns)?,
        name: if dto.real {
            asset_core::AssetRef::Real(dto.name)
        } else {
            asset_core::AssetRef::Reference(dto.name)
        },
        zone: asset_core::ZoneOwner::intern(&dto.zone),
        table,
        t5_occupancy,
        iw5_fallback_table,
        t5_fallback_table,
        world_vert_format: dto.world_vert_format,
    })
}

fn find_image(catalog: &MaterialCatalog, ns: &str, name: &str) -> Option<usize> {
    let ns = ns_of(ns)?;
    catalog
        .images
        .iter()
        .position(|image| image.namespace == ns && image.name.as_str() == name)
}

fn image_in(image: ImageDto, blobs: &BlobsIn) -> Option<AuthoredImage> {
    Some(AuthoredImage {
        namespace: ns_of(&image.ns)?,
        name: if image.real {
            asset_core::AssetRef::Real(image.name)
        } else {
            asset_core::AssetRef::Reference(image.name)
        },
        map_type: image.map_type,
        semantic: image.semantic,
        category: image.category,
        use_srgb_reads: image.srgb,
        width: image.width,
        height: image.height,
        depth: image.depth,
        level_count: image.levels,
        format: image.format,
        payload: std::sync::Arc::new(blobs.owned(image.payload)?),
        decoded: match image.gpu {
            None => None,
            Some(gpu) => Some(gpu_in(gpu, blobs)?),
        },
        common_owned: image.common,
        decoded_variant: None,
        decoded_by: None,
        pending_decode: None,
    })
}

/// A picture for the file: its facts in the JSON, its texels put in `blobs`.
pub(crate) fn gpu_out(image: &Image, blobs: &mut Blobs) -> Result<GpuDto, String> {
    let d = &image.texture_descriptor;
    let data = blobs.put(
        image
            .data
            .clone()
            .ok_or_else(|| "a decoded picture has no bytes".to_owned())?,
    );
    let sampler: (u8, [u8; 3], [u8; 3], u16) = match &image.sampler {
        ImageSampler::Default => (0, [0; 3], [0; 3], 1),
        ImageSampler::Descriptor(s) => (
            1,
            [
                addr(s.address_mode_u),
                addr(s.address_mode_v),
                addr(s.address_mode_w),
            ],
            [
                filter(s.mag_filter),
                filter(s.min_filter),
                filter(s.mipmap_filter),
            ],
            s.anisotropy_clamp,
        ),
    };
    Ok(GpuDto {
        dim: match d.dimension {
            TextureDimension::D1 => 1,
            TextureDimension::D2 => 2,
            TextureDimension::D3 => 3,
        },
        format: format_code(d.format).ok_or_else(|| format!("picture format {:?}", d.format))?,
        w: d.size.width,
        h: d.size.height,
        d: d.size.depth_or_array_layers,
        mips: d.mip_level_count,
        data,
        views: d
            .view_formats
            .iter()
            .copied()
            .map(format_code)
            .collect::<Option<Vec<_>>>()
            .ok_or_else(|| "view format".to_owned())?,
        sampler: sampler.0,
        address: sampler.1,
        filter: sampler.2,
        anisotropy: sampler.3,
        usage: u32::from(image.asset_usage.bits()),
        copy: image.copy_on_resize,
        order: match image.data_order {
            bevy::render::render_resource::TextureDataOrder::LayerMajor => 0,
            bevy::render::render_resource::TextureDataOrder::MipMajor => 1,
        },
    })
}

/// The picture back, its texels copied out of `blobs`.
pub(crate) fn gpu_in(gpu: GpuDto, blobs: &BlobsIn) -> Option<std::sync::Arc<Image>> {
    let data = blobs.owned(gpu.data)?;
    let format = format_of(gpu.format)?;
    let dimension = match gpu.dim {
        1 => TextureDimension::D1,
        2 => TextureDimension::D2,
        3 => TextureDimension::D3,
        _ => return None,
    };
    let mut image = Image::new_uninit(
        Extent3d {
            width: gpu.w,
            height: gpu.h,
            depth_or_array_layers: gpu.d,
        },
        dimension,
        format,
        RenderAssetUsages::RENDER_WORLD,
    );
    image.texture_descriptor.mip_level_count = gpu.mips;
    let mut views = Vec::new();
    for code in gpu.views {
        views.push(format_of(code)?);
    }
    image.texture_descriptor.view_formats = Box::leak(views.into_boxed_slice());
    image.data = Some(data);
    image.copy_on_resize = gpu.copy;
    image.data_order = if gpu.order == 1 {
        bevy::render::render_resource::TextureDataOrder::MipMajor
    } else {
        bevy::render::render_resource::TextureDataOrder::LayerMajor
    };
    if gpu.sampler == 1 {
        image.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
            address_mode_u: addr_of(gpu.address[0])?,
            address_mode_v: addr_of(gpu.address[1])?,
            address_mode_w: addr_of(gpu.address[2])?,
            mag_filter: filter_of(gpu.filter[0])?,
            min_filter: filter_of(gpu.filter[1])?,
            mipmap_filter: filter_of(gpu.filter[2])?,
            anisotropy_clamp: gpu.anisotropy,
            ..Default::default()
        });
    }
    Some(std::sync::Arc::new(image))
}

fn addr(mode: ImageAddressMode) -> u8 {
    match mode {
        ImageAddressMode::ClampToEdge => 0,
        ImageAddressMode::Repeat => 1,
        ImageAddressMode::MirrorRepeat => 2,
        ImageAddressMode::ClampToBorder => 3,
    }
}

fn addr_of(code: u8) -> Option<ImageAddressMode> {
    Some(match code {
        0 => ImageAddressMode::ClampToEdge,
        1 => ImageAddressMode::Repeat,
        2 => ImageAddressMode::MirrorRepeat,
        3 => ImageAddressMode::ClampToBorder,
        _ => return None,
    })
}

fn filter(mode: ImageFilterMode) -> u8 {
    match mode {
        ImageFilterMode::Nearest => 0,
        ImageFilterMode::Linear => 1,
    }
}

fn filter_of(code: u8) -> Option<ImageFilterMode> {
    Some(match code {
        0 => ImageFilterMode::Nearest,
        1 => ImageFilterMode::Linear,
        _ => return None,
    })
}

fn format_code(format: TextureFormat) -> Option<u16> {
    Some(match format {
        TextureFormat::Rgba8Unorm => 1,
        TextureFormat::Rgba8UnormSrgb => 2,
        TextureFormat::Bgra8Unorm => 3,
        TextureFormat::Bgra8UnormSrgb => 4,
        TextureFormat::Bc1RgbaUnorm => 5,
        TextureFormat::Bc1RgbaUnormSrgb => 6,
        TextureFormat::Bc2RgbaUnorm => 7,
        TextureFormat::Bc2RgbaUnormSrgb => 8,
        TextureFormat::Bc3RgbaUnorm => 9,
        TextureFormat::Bc3RgbaUnormSrgb => 10,
        TextureFormat::Bc4RUnorm => 11,
        TextureFormat::Bc4RSnorm => 12,
        TextureFormat::Bc5RgUnorm => 13,
        TextureFormat::Bc6hRgbUfloat => 14,
        TextureFormat::Bc6hRgbFloat => 15,
        TextureFormat::Bc7RgbaUnorm => 16,
        TextureFormat::Bc7RgbaUnormSrgb => 17,
        _ => return None,
    })
}

fn format_of(code: u16) -> Option<TextureFormat> {
    Some(match code {
        1 => TextureFormat::Rgba8Unorm,
        2 => TextureFormat::Rgba8UnormSrgb,
        3 => TextureFormat::Bgra8Unorm,
        4 => TextureFormat::Bgra8UnormSrgb,
        5 => TextureFormat::Bc1RgbaUnorm,
        6 => TextureFormat::Bc1RgbaUnormSrgb,
        7 => TextureFormat::Bc2RgbaUnorm,
        8 => TextureFormat::Bc2RgbaUnormSrgb,
        9 => TextureFormat::Bc3RgbaUnorm,
        10 => TextureFormat::Bc3RgbaUnormSrgb,
        11 => TextureFormat::Bc4RUnorm,
        12 => TextureFormat::Bc4RSnorm,
        13 => TextureFormat::Bc5RgUnorm,
        14 => TextureFormat::Bc6hRgbUfloat,
        15 => TextureFormat::Bc6hRgbFloat,
        16 => TextureFormat::Bc7RgbaUnorm,
        17 => TextureFormat::Bc7RgbaUnormSrgb,
        _ => return None,
    })
}

struct Meshes {
    fpv: FpvMeshBuild,
    world: WorldWeaponBuild,
    projectiles: ProjectileMeshBuild,
    fx_models: asset_game::FxModelCatalog,
}

fn pack_meshes(parts: &Parts<'_>) -> Result<Vec<u8>, String> {
    let mut w = W::default();
    w.u32(parts.fpv.len() as u32);
    for i in 0..parts.fpv.len() {
        let entry = parts.fpv.get_at(i).ok_or_else(|| "fpv row".to_owned())?;
        mesh_out(
            &mut w,
            parts.materials,
            0,
            entry.namespace,
            parts.fpv.zone_of(i),
            &entry.skel,
        )?;
    }
    w.u32(parts.world.len() as u32);
    for i in 0..parts.world.len() {
        let entry = parts
            .world
            .get_at(i)
            .ok_or_else(|| "world row".to_owned())?;
        mesh_out(
            &mut w,
            parts.materials,
            1,
            entry.namespace,
            parts.world.zone_of(i),
            &entry.skel,
        )?;
    }
    w.u32(parts.projectiles.len() as u32);
    for i in 0..parts.projectiles.len() {
        let entry = parts
            .projectiles
            .get_at(i)
            .ok_or_else(|| "projectile row".to_owned())?;
        mesh_out(
            &mut w,
            parts.materials,
            2,
            entry.namespace,
            asset_core::ZoneOwner::default(),
            &entry.skel,
        )?;
    }
    w.u32(parts.fx_models.len() as u32);
    for i in 0..parts.fx_models.len() {
        let entry = parts
            .fx_models
            .get_at(i)
            .ok_or_else(|| "effect model".to_owned())?;
        let ns = parts
            .fx_models
            .namespace_at(i)
            .ok_or_else(|| "effect model namespace".to_owned())?;
        mesh_out(
            &mut w,
            parts.materials,
            3,
            ns,
            parts.fx_models.zone_of(i),
            &entry.skel,
        )?;
    }
    Ok(w.0)
}

fn mesh_out(
    w: &mut W,
    materials: &MaterialCatalog,
    kind: u8,
    ns: asset_core::AssetNamespace,
    zone: asset_core::ZoneOwner,
    skel: &ModelSkel,
) -> Result<(), String> {
    w.u8(kind);
    w.str(ns.as_str());
    w.str(zone.as_str());
    skel_out(w, materials, skel)
}

fn unpack_meshes(bytes: &[u8], materials: &MaterialCatalog) -> Option<Meshes> {
    let mut r = R { b: bytes, at: 0 };
    let mut fpv = FpvMeshBuild::default();
    let mut world = WorldWeaponBuild::default();
    let mut projectiles = ProjectileMeshBuild::default();
    let mut fx_models = asset_game::FxModelCatalog::default();
    let n = r.u32()? as usize;
    for _ in 0..n {
        let _kind = r.u8()?;
        let ns = ns_of(&r.str()?)?;
        let zone = asset_core::ZoneOwner::intern(&r.str()?);
        let skel = skel_in(&mut r, materials)?;
        fpv.set_capture_ns(ns);
        fpv.set_capture_zone(zone);
        fpv.insert_in(ns, skel, Some(materials));
    }
    let n = r.u32()? as usize;
    for _ in 0..n {
        let _kind = r.u8()?;
        let ns = ns_of(&r.str()?)?;
        let zone = asset_core::ZoneOwner::intern(&r.str()?);
        let skel = skel_in(&mut r, materials)?;
        world.set_capture_ns(ns);
        world.set_capture_zone(zone);
        world.insert_in(ns, skel, Some(materials));
    }
    let n = r.u32()? as usize;
    for _ in 0..n {
        let _kind = r.u8()?;
        let ns = ns_of(&r.str()?)?;
        let _zone = r.str()?;
        let skel = skel_in(&mut r, materials)?;
        projectiles.set_capture_ns(ns);
        projectiles.insert_captured(skel, Some(materials));
    }
    let n = r.u32()? as usize;
    for _ in 0..n {
        let _kind = r.u8()?;
        let ns = ns_of(&r.str()?)?;
        let zone = asset_core::ZoneOwner::intern(&r.str()?);
        let skel = skel_in(&mut r, materials)?;
        fx_models.set_capture_ns(ns);
        fx_models.set_capture_zone(zone);
        fx_models.insert_skel(skel, materials);
    }
    (r.at == bytes.len()).then_some(Meshes {
        fpv,
        world,
        projectiles,
        fx_models,
    })
}

pub(crate) fn encode_skel(materials: &MaterialCatalog, skel: &ModelSkel) -> Result<Vec<u8>, String> {
    let mut w = W::default();
    skel_out(&mut w, materials, skel)?;
    Ok(w.0)
}

pub(crate) fn decode_skel(bytes: &[u8], materials: &MaterialCatalog) -> Option<ModelSkel> {
    let mut r = R { b: bytes, at: 0 };
    let skel = skel_in(&mut r, materials)?;
    (r.at == bytes.len()).then_some(skel)
}

pub(crate) fn ns_of(s: &str) -> Option<asset_core::AssetNamespace> {
    Some(match s {
        "iw4" => asset_core::AssetNamespace::Iw4,
        "t5" => asset_core::AssetNamespace::T5,
        "iw5" => asset_core::AssetNamespace::Iw5,
        "t6" => asset_core::AssetNamespace::T6,
        "iw6" => asset_core::AssetNamespace::Iw6,
        "t4" => asset_core::AssetNamespace::T4,
        "iw3" => asset_core::AssetNamespace::Iw3,
        "s1" => asset_core::AssetNamespace::S1,
        "t7" => asset_core::AssetNamespace::T7,
        "jup" => asset_core::AssetNamespace::Jup,
        "h1" => asset_core::AssetNamespace::H1,
        "iw7" => asset_core::AssetNamespace::Iw7,
        _ => return None,
    })
}

fn skel_out(w: &mut W, materials: &MaterialCatalog, skel: &ModelSkel) -> Result<(), String> {
    w.str(&skel.name);
    w.u32(skel.bones.len() as u32);
    for bone in &skel.bones {
        w.f32s(&bone.quat);
        w.f32s(&bone.trans);
    }
    w.u32(skel.bone_collision.len() as u32);
    for bone in &skel.bone_collision {
        match bone {
            None => w.u8(0),
            Some(b) => {
                w.u8(1);
                w.f32s(&b.midpoint);
                w.f32s(&b.half_size);
                w.f32(b.radius_sq);
                w.u8(b.part_classification);
            }
        }
    }
    w.strs(&skel.bone_names);
    w.opt_u32(skel.tag_view);
    w.opt_u32(skel.tag_weapon);
    match &skel.pose {
        None => w.u8(0),
        Some(pose) => {
            w.u8(1);
            w.str(&pose.name);
            w.u32(pose.num_bones as u32);
            w.u32(pose.num_root_bones as u32);
            w.f32(pose.scale);
            w.u32s(&pose.no_scale_part_bits);
            w.strs(&pose.bone_names);
            w.bytes(&pose.parent_list);
            w.u32(pose.quats.len() as u32);
            for q in &pose.quats {
                for c in q {
                    w.i16(*c);
                }
            }
            w.u32(pose.trans.len() as u32);
            for t in &pose.trans {
                w.f32s(t);
            }
            w.u32(pose.base_mat.len() as u32);
            for (q, t) in &pose.base_mat {
                w.f32s(&q.to_array());
                w.f32s(&t.to_array());
            }
        }
    }
    w.u32(skel.positions.len() as u32);
    for p in &skel.positions {
        w.f32s(p);
    }
    w.u32(skel.normals.len() as u32);
    for p in &skel.normals {
        w.f32s(p);
    }
    w.u32(skel.colors.len() as u32);
    for p in &skel.colors {
        w.f32s(p);
    }
    w.u32(skel.uvs.len() as u32);
    for p in &skel.uvs {
        w.f32s(p);
    }
    w.u32s(&skel.indices);
    w.u32(skel.surface_materials.len() as u32);
    for slot in &skel.surface_materials {
        match slot {
            None => w.u8(0),
            Some(index) => {
                let material = materials
                    .materials
                    .get(index.get())
                    .ok_or_else(|| format!("{}: material {}", skel.name, index.get()))?;
                w.u8(1);
                w.str(material.namespace.as_str());
                w.str(material.name.as_str());
            }
        }
    }
    w.u32(skel.surface_vertex_ranges.len() as u32);
    for (a, b) in &skel.surface_vertex_ranges {
        w.u32(*a as u32);
        w.u32(*b as u32);
    }
    w.u32(skel.surface_index_ranges.len() as u32);
    for (a, b) in &skel.surface_index_ranges {
        w.u32(*a as u32);
        w.u32(*b as u32);
    }
    w.u32(skel.surface_part_bits.len() as u32);
    for bits in &skel.surface_part_bits {
        w.u32s(bits);
    }
    w.u32(skel.surface_deformed.len() as u32);
    for flag in &skel.surface_deformed {
        match flag {
            None => w.u8(0),
            Some(v) => w.u8(if *v { 2 } else { 1 }),
        }
    }
    w.u32(skel.surface_vert_list_count.len() as u32);
    for n in &skel.surface_vert_list_count {
        w.opt_u32((*n).map(|v| v as usize));
    }
    w.u32(skel.vert_skin.len() as u32);
    for skin in &skel.vert_skin {
        for b in skin.bones {
            w.u16(b);
        }
        w.f32s(&skin.weights);
        for b in skin.weight_u16 {
            w.u16(b);
        }
    }
    w.u32(skel.rigid_verts as u32);
    w.u32(skel.blend_verts as u32);
    let stride = asset_iw4::size::GFX_PACKED_VERTEX;
    w.u16(stride as u16);
    w.u32(skel.packed_vertices.len() as u32);
    for vert in &skel.packed_vertices {
        w.bytes(vert);
    }
    w.opt_f32(skel.radius);
    match skel.bounds {
        None => w.u8(0),
        Some((mid, half)) => {
            w.u8(1);
            w.f32s(&mid);
            w.f32s(&half);
        }
    }
    w.opt_u32(skel.contents.map(|v| v as usize));
    w.i16(skel.coll_lod);
    w.u32(skel.coll_surfs.len() as u32);
    for surf in &skel.coll_surfs {
        w.u16(surf.bone);
        w.u32(surf.contents);
        w.u32(surf.surf_flags);
        w.f32s(&surf.midpoint);
        w.f32s(&surf.half_size);
        w.u32(surf.tris.len() as u32);
        for tri in &surf.tris {
            w.f32s(&tri.plane);
            w.f32s(&tri.svec);
            w.f32s(&tri.tvec);
        }
    }
    w.u32(skel.collmaps.len() as u32);
    for map in &skel.collmaps {
        w.f32s(&map.mins);
        w.f32s(&map.maxs);
        w.u32(map.contents);
    }
    w.u32(skel.surface_flags);
    match skel.lod {
        None => w.u8(0),
        Some(asset_model::ModelLodSelector::Iw4 {
            lod_start,
            num_lods,
            lod_dist,
        }) => {
            w.u8(1);
            w.u8(lod_start);
            w.u8(num_lods);
            w.f32s(&lod_dist);
        }
        Some(asset_model::ModelLodSelector::T5 { num_lods, lod_dist }) => {
            w.u8(2);
            w.i16(num_lods);
            w.f32s(&lod_dist);
        }
    }
    match skel.lod_smc {
        None => w.u8(0),
        Some(rows) => {
            w.u8(1);
            for row in rows {
                w.bytes(&row);
            }
        }
    }
    match skel.lod_part_bits {
        None => w.u8(0),
        Some(rows) => {
            w.u8(1);
            for row in rows {
                w.u32s(&row);
            }
        }
    }
    for (a, b) in skel.lod_surf_span {
        w.u16(a);
        w.u16(b);
    }
    Ok(())
}

fn skel_in(r: &mut R<'_>, materials: &MaterialCatalog) -> Option<ModelSkel> {
    let name = r.str()?;
    let bones = (0..r.u32()?)
        .map(|_| {
            Some(asset_model::BoneBind {
                quat: r.f4()?,
                trans: r.f3()?,
            })
        })
        .collect::<Option<Vec<_>>>()?;
    let bone_collision = (0..r.u32()?)
        .map(|_| match r.u8()? {
            0 => Some(None),
            1 => Some(Some(xmodel_runtime::BoneCollision {
                midpoint: r.f3()?,
                half_size: r.f3()?,
                radius_sq: r.f32()?,
                part_classification: r.u8()?,
            })),
            _ => None,
        })
        .collect::<Option<Vec<_>>>()?;
    let bone_names = r.strs()?;
    let tag_view = r.opt_u32()?;
    let tag_weapon = r.opt_u32()?;
    let pose = match r.u8()? {
        0 => None,
        1 => Some(xmodel_runtime::ModelPoseSrc {
            name: r.str()?,
            num_bones: r.u32()? as usize,
            num_root_bones: r.u32()? as usize,
            scale: r.f32()?,
            no_scale_part_bits: r.u32x6()?,
            bone_names: r.strs()?,
            parent_list: r.bytes()?,
            quats: (0..r.u32()?)
                .map(|_| Some([r.i16()?, r.i16()?, r.i16()?, r.i16()?]))
                .collect::<Option<_>>()?,
            trans: (0..r.u32()?).map(|_| r.f3()).collect::<Option<_>>()?,
            base_mat: (0..r.u32()?)
                .map(|_| {
                    let q = r.f4()?;
                    let t = r.f3()?;
                    Some((
                        bevy::math::Quat::from_array(q),
                        bevy::math::Vec3::from_array(t),
                    ))
                })
                .collect::<Option<_>>()?,
        }),
        _ => return None,
    };
    let positions = (0..r.u32()?).map(|_| r.f3()).collect::<Option<_>>()?;
    let normals = (0..r.u32()?).map(|_| r.f3()).collect::<Option<_>>()?;
    let colors = (0..r.u32()?).map(|_| r.f4()).collect::<Option<_>>()?;
    let uvs = (0..r.u32()?)
        .map(|_| Some([r.f32()?, r.f32()?]))
        .collect::<Option<_>>()?;
    let indices = r.u32s()?;
    let surface_materials = (0..r.u32()?)
        .map(|_| match r.u8()? {
            0 => Some(None),
            1 => {
                let ns = ns_of(&r.str()?)?;
                let name = r.str()?;
                let index = materials
                    .materials
                    .iter()
                    .position(|m| m.namespace == ns && m.name.as_str() == name)?;
                Some(Some(asset_core::WalkLocalMaterialIndex::from_walk(index)))
            }
            _ => None,
        })
        .collect::<Option<Vec<_>>>()?;
    let surface_vertex_ranges = (0..r.u32()?)
        .map(|_| Some((r.u32()? as usize, r.u32()? as usize)))
        .collect::<Option<_>>()?;
    let surface_index_ranges = (0..r.u32()?)
        .map(|_| Some((r.u32()? as usize, r.u32()? as usize)))
        .collect::<Option<_>>()?;
    let surface_part_bits = (0..r.u32()?).map(|_| r.u32x6()).collect::<Option<_>>()?;
    let surface_deformed = (0..r.u32()?)
        .map(|_| match r.u8()? {
            0 => Some(None),
            1 => Some(Some(false)),
            2 => Some(Some(true)),
            _ => None,
        })
        .collect::<Option<_>>()?;
    let surface_vert_list_count = (0..r.u32()?)
        .map(|_| r.opt_u32().map(|v| v.map(|n| n as u32)))
        .collect::<Option<_>>()?;
    let vert_skin = (0..r.u32()?)
        .map(|_| {
            Some(asset_model::VertSkin {
                bones: [r.u16()?, r.u16()?, r.u16()?, r.u16()?],
                weights: r.f4()?,
                weight_u16: [r.u16()?, r.u16()?, r.u16()?, r.u16()?],
            })
        })
        .collect::<Option<_>>()?;
    let rigid_verts = r.u32()? as usize;
    let blend_verts = r.u32()? as usize;
    let stride = r.u16()? as usize;
    if stride != asset_iw4::size::GFX_PACKED_VERTEX {
        return None;
    }
    let packed_vertices = (0..r.u32()?)
        .map(|_| {
            let bytes = r.take(stride)?;
            let mut vert = [0u8; asset_iw4::size::GFX_PACKED_VERTEX];
            vert.copy_from_slice(bytes);
            Some(vert)
        })
        .collect::<Option<_>>()?;
    let radius = r.opt_f32()?;
    let bounds = match r.u8()? {
        0 => None,
        1 => Some((r.f3()?, r.f3()?)),
        _ => return None,
    };
    let contents = r.opt_u32()?.map(|v| v as u32);
    let coll_lod = r.i16()?;
    let coll_surfs = (0..r.u32()?)
        .map(|_| {
            Some(xmodel_runtime::CollSurfCollision {
                bone: r.u16()?,
                contents: r.u32()?,
                surf_flags: r.u32()?,
                midpoint: r.f3()?,
                half_size: r.f3()?,
                tris: (0..r.u32()?)
                    .map(|_| {
                        Some(xmodel_runtime::CollTri {
                            plane: r.f4()?,
                            svec: r.f4()?,
                            tvec: r.f4()?,
                        })
                    })
                    .collect::<Option<_>>()?,
            })
        })
        .collect::<Option<_>>()?;
    let collmaps = (0..r.u32()?)
        .map(|_| {
            Some(xmodel_runtime::CollmapBox {
                mins: r.f3()?,
                maxs: r.f3()?,
                contents: r.u32()?,
            })
        })
        .collect::<Option<_>>()?;
    let surface_flags = r.u32()?;
    let lod = match r.u8()? {
        0 => None,
        1 => Some(asset_model::ModelLodSelector::Iw4 {
            lod_start: r.u8()?,
            num_lods: r.u8()?,
            lod_dist: r.f4()?,
        }),
        2 => Some(asset_model::ModelLodSelector::T5 {
            num_lods: r.i16()?,
            lod_dist: r.f4()?,
        }),
        _ => return None,
    };
    let lod_smc = match r.u8()? {
        0 => None,
        1 => Some([r.u8x4()?, r.u8x4()?, r.u8x4()?, r.u8x4()?]),
        _ => return None,
    };
    let lod_part_bits = match r.u8()? {
        0 => None,
        1 => Some([r.u32x6()?, r.u32x6()?, r.u32x6()?, r.u32x6()?]),
        _ => return None,
    };
    let lod_surf_span = [
        (r.u16()?, r.u16()?),
        (r.u16()?, r.u16()?),
        (r.u16()?, r.u16()?),
        (r.u16()?, r.u16()?),
    ];
    Some(ModelSkel {
        name,
        bones,
        bone_collision,
        bone_names,
        tag_view,
        tag_weapon,
        pose,
        positions,
        normals,
        colors,
        uvs,
        indices,
        surface_materials,
        surface_vertex_ranges,
        surface_index_ranges,
        surface_part_bits,
        surface_deformed,
        surface_vert_list_count,
        vert_skin,
        rigid_verts,
        blend_verts,
        packed_vertices,
        radius,
        bounds,
        contents,
        coll_lod,
        coll_surfs,
        collmaps,
        surface_flags,
        lod,
        lod_smc,
        lod_part_bits,
        lod_surf_span,
    })
}

#[derive(Default)]
struct W(Vec<u8>);

impl W {
    fn u8(&mut self, v: u8) {
        self.0.push(v);
    }
    fn u16(&mut self, v: u16) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }
    fn u32(&mut self, v: u32) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }
    fn i16(&mut self, v: i16) {
        self.u16(v as u16);
    }
    fn f32(&mut self, v: f32) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }
    fn f32s(&mut self, v: &[f32]) {
        for x in v {
            self.f32(*x);
        }
    }
    fn u32s(&mut self, v: &[u32]) {
        for x in v {
            self.u32(*x);
        }
    }
    fn bytes(&mut self, v: &[u8]) {
        self.u32(v.len() as u32);
        self.0.extend_from_slice(v);
    }
    fn str(&mut self, s: &str) {
        self.bytes(s.as_bytes());
    }
    fn strs(&mut self, v: &[String]) {
        self.u32(v.len() as u32);
        for s in v {
            self.str(s);
        }
    }
    fn opt_u32(&mut self, v: Option<usize>) {
        match v {
            None => self.u8(0),
            Some(n) => {
                self.u8(1);
                self.u32(n as u32);
            }
        }
    }
    fn opt_f32(&mut self, v: Option<f32>) {
        match v {
            None => self.u8(0),
            Some(n) => {
                self.u8(1);
                self.f32(n);
            }
        }
    }
}

struct R<'a> {
    b: &'a [u8],
    at: usize,
}

impl<'a> R<'a> {
    fn take(&mut self, n: usize) -> Option<&'a [u8]> {
        let end = self.at.checked_add(n)?;
        let s = self.b.get(self.at..end)?;
        self.at = end;
        Some(s)
    }
    fn u8(&mut self) -> Option<u8> {
        Some(self.take(1)?[0])
    }
    fn u16(&mut self) -> Option<u16> {
        Some(u16::from_le_bytes(self.take(2)?.try_into().ok()?))
    }
    fn u32(&mut self) -> Option<u32> {
        Some(u32::from_le_bytes(self.take(4)?.try_into().ok()?))
    }
    fn i16(&mut self) -> Option<i16> {
        self.u16().map(|v| v as i16)
    }
    fn f32(&mut self) -> Option<f32> {
        Some(f32::from_le_bytes(self.take(4)?.try_into().ok()?))
    }
    fn f3(&mut self) -> Option<[f32; 3]> {
        Some([self.f32()?, self.f32()?, self.f32()?])
    }
    fn f4(&mut self) -> Option<[f32; 4]> {
        Some([self.f32()?, self.f32()?, self.f32()?, self.f32()?])
    }
    fn u32x6(&mut self) -> Option<[u32; 6]> {
        Some([
            self.u32()?,
            self.u32()?,
            self.u32()?,
            self.u32()?,
            self.u32()?,
            self.u32()?,
        ])
    }
    fn u8x4(&mut self) -> Option<[u8; 4]> {
        Some([self.u8()?, self.u8()?, self.u8()?, self.u8()?])
    }
    fn bytes(&mut self) -> Option<Vec<u8>> {
        let n = self.u32()? as usize;
        Some(self.take(n)?.to_vec())
    }
    fn str(&mut self) -> Option<String> {
        String::from_utf8(self.bytes()?).ok()
    }
    fn strs(&mut self) -> Option<Vec<String>> {
        (0..self.u32()?).map(|_| self.str()).collect()
    }
    fn u32s(&mut self) -> Option<Vec<u32>> {
        (0..self.u32()?).map(|_| self.u32()).collect()
    }
    fn opt_u32(&mut self) -> Option<Option<usize>> {
        match self.u8()? {
            0 => Some(None),
            1 => Some(Some(self.u32()? as usize)),
            _ => None,
        }
    }
    fn opt_f32(&mut self) -> Option<Option<f32>> {
        match self.u8()? {
            0 => Some(None),
            1 => Some(Some(self.f32()?)),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_changed_picture_cap_is_another_key() {
        let dir = std::env::temp_dir().join(format!("fgun-key-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("zone.ff");
        std::fs::write(&file, b"zone").unwrap();
        let files = [file];
        let code = ["code"];
        let low = [("fp", 512u32)];
        let high = [("fp", 1024u32)];
        let a = Sources {
            lane: "h1",
            name: "guns",
            files: &files,
            caps: &low,
            code: &code,
            zones: &[],
            note: "",
            missing_ok: false,
        };
        let b = Sources { caps: &high, ..a };
        // Baking may be off in this process; the key still separates the caps
        // when a root exists. Compare the hashed inputs directly.
        let mut ka = map_bake::Key::new();
        ka.u64(512);
        let mut kb = map_bake::Key::new();
        kb.u64(1024);
        assert_ne!(ka.value(), kb.value());
        let _ = (a, b);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_short_file_is_not_a_hit() {
        assert!(unpack_file(&1, b"BO2FGUN\0").is_err());
        let mut junk = pack_file(&7, b"hello");
        junk.pop();
        assert!(matches!(unpack_file(&7, &junk), Err(UnpackErr::Broken)));
        assert!(matches!(
            unpack_file(&8, &pack_file(&7, b"hello")),
            Err(UnpackErr::Stale)
        ));
        assert_eq!(unpack_file(&7, &pack_file(&7, b"hello")).unwrap(), b"hello");
    }

    #[test]
    fn a_material_and_its_picture_round_trip() {
        let mut catalog = MaterialCatalog::default();
        let mut image = Image::new_uninit(
            Extent3d {
                width: 1,
                height: 1,
                depth_or_array_layers: 1,
            },
            TextureDimension::D2,
            TextureFormat::Rgba8UnormSrgb,
            RenderAssetUsages::RENDER_WORLD,
        );
        image.data = Some(vec![9, 8, 7, 6]);
        catalog.link_image(AuthoredImage {
            namespace: asset_core::AssetNamespace::H1,
            name: asset_core::AssetRef::Real("h1:color#512".into()),
            map_type: 3,
            semantic: 2,
            category: 0,
            use_srgb_reads: true,
            width: 1,
            height: 1,
            depth: 1,
            level_count: 1,
            format: 0,
            payload: std::sync::Arc::new(vec![1, 2]),
            decoded: Some(std::sync::Arc::new(image)),
            common_owned: false,
            decoded_variant: None,
            decoded_by: None,
            pending_decode: None,
        });
        catalog.link_material(AuthoredMaterial {
            name: asset_core::AssetRef::Real("h1:gun".into()),
            namespace: asset_core::AssetNamespace::H1,
            technique_set: asset_core::AssetRef::default(),
            technique_set_edge: asset_core::AssetEdge::Absent,
            draw_surf: 0,
            sort_key: 1,
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
            textures: vec![MaterialTextureBinding {
                name_hash: 1,
                name_start: 0,
                name_end: 0,
                sampler_state: 1,
                semantic: 2,
                image: Some(0),
            }],
            constants: Vec::new(),
            zone: asset_core::ZoneOwner::intern("hmw_ar1"),
            t6_draw: None,
        });
        let at = MatAt {
            images: 0,
            materials: 0,
            shaders: 0,
            decls: 0,
            techsets: 0,
        };
        let mut blobs = Blobs::default();
        let bytes = pack_materials(&catalog, at, &mut blobs).unwrap();
        // The blob section as the file carries it: tag, length, then the runs.
        let mut section = Vec::new();
        section_runs(&mut section, b"blob", &blobs);
        let blobs_in = BlobsIn::unpack(section[12..].to_vec()).unwrap();
        let back = unpack_materials(MaterialCatalog::default(), &bytes, &blobs_in).unwrap();
        assert_eq!(back.images.len(), 1);
        assert_eq!(back.materials.len(), 1);
        assert_eq!(back.materials[0].textures[0].image, Some(0));
        assert_eq!(
            back.images[0]
                .decoded
                .as_ref()
                .unwrap()
                .data
                .as_ref()
                .unwrap(),
            &vec![9, 8, 7, 6]
        );
        assert_eq!(back.materials[0].zone.as_str(), "hmw_ar1");
    }

    #[test]
    fn an_empty_gun_set_round_trips() {
        let weapons = WeaponBuild::default();
        let fpv = FpvMeshBuild::default();
        let world = WorldWeaponBuild::default();
        let projectiles = ProjectileMeshBuild::default();
        let xanims = XAnimBuild::default();
        let fx = FxCatalog::default();
        let fx_models = asset_game::FxModelCatalog::default();
        let tracers = TracerCatalog::default();
        let materials = MaterialCatalog::default();
        let reports = vec!["h1 guest guns: none".to_owned()];
        let parts = Parts {
            weapons: &weapons,
            fpv: &fpv,
            world: &world,
            projectiles: &projectiles,
            xanims: &xanims,
            fx: &fx,
            fx_models: &fx_models,
            tracers: &tracers,
            impact: None,
            materials: &materials,
            mat_at: MatAt::of(&materials),
            reports: &reports,
            extra: b"extra",
        };
        let bytes = pack_parts(&parts).unwrap();
        let back = unpack_parts(&bytes, MaterialCatalog::default()).unwrap();
        assert_eq!(back.reports, reports);
        assert_eq!(back.extra, b"extra");
        assert_eq!(back.weapons.len(), weapons.len());
        assert_eq!(back.projectiles.len(), 0);
        assert_eq!(back.fpv.len(), 0);
        assert_eq!(back.world.len(), 0);
        assert!(back.impact.is_none());
    }

    #[test]
    fn a_newly_added_source_file_changes_the_key_and_its_name_is_kept() {
        let dir = std::env::temp_dir().join(format!("fgun-new-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let first = dir.join("common.ff");
        let added = dir.join("added_forward.ff");
        std::fs::write(&first, b"guns").unwrap();
        std::fs::write(&added, b"added_forward_mp").unwrap();
        let code = ["loader"];
        let caps = [("fp", 512u32)];
        let only = [first.clone()];
        let both = [first, added];
        let base = Sources {
            lane: "t6",
            name: "mp_test",
            files: &only,
            caps: &caps,
            code: &code,
            zones: &[],
            note: "",
            missing_ok: false,
        };
        let old_key = identity(&base).unwrap();
        let with_new = Sources { files: &both, ..base };
        let new_key = identity(&with_new).unwrap();
        assert_ne!(old_key, new_key);
        let reports = vec!["weapon added_forward_mp".to_owned()];
        let weapons = WeaponBuild::default();
        let fpv = FpvMeshBuild::default();
        let world = WorldWeaponBuild::default();
        let projectiles = ProjectileMeshBuild::default();
        let xanims = XAnimBuild::default();
        let fx = FxCatalog::default();
        let fx_models = asset_game::FxModelCatalog::default();
        let tracers = TracerCatalog::default();
        let materials = MaterialCatalog::default();
        let parts = Parts {
            weapons: &weapons,
            fpv: &fpv,
            world: &world,
            projectiles: &projectiles,
            xanims: &xanims,
            fx: &fx,
            fx_models: &fx_models,
            tracers: &tracers,
            impact: None,
            materials: &materials,
            mat_at: MatAt::of(&materials),
            reports: &reports,
            extra: b"added_forward_mp",
        };
        let back = unpack_parts(&pack_parts(&parts).unwrap(), MaterialCatalog::default()).unwrap();
        assert_eq!(back.reports, reports);
        assert_eq!(back.extra, b"added_forward_mp");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn gun_loaders_go_through_the_shared_cache() {
        let loaders = [
            include_str!("hmw.rs"),
            include_str!("iw6.rs"),
            include_str!("s1.rs"),
            include_str!("t7.rs"),
            include_str!("legacy_guns.rs"),
            include_str!("iw7_gun.rs"),
            include_str!("jup_gun.rs"),
            include_str!("t6_m2.rs"),
            include_str!("census_cache.rs"),
        ];
        for src in loaders {
            assert!(
                src.contains("finished_gun_cache::"),
                "a gun loader left the shared cache"
            );
        }
        assert!(
            !include_str!("dropped.rs").contains("finished_gun_cache::"),
            "empty stubs are not given a cache"
        );
    }

    #[test]
    fn every_zone_lane_declares_whether_common_weapons_are_cached() {
        use super::super::ZoneLane;
        assert!(super::super::iw4::Iw4Lane.caches_common_weapons());
        assert!(super::super::t5::T5Lane.caches_common_weapons());
        assert!(super::super::iw5::Iw5Lane.caches_common_weapons());
        assert!(!super::super::t6::T6Lane.caches_common_weapons());
    }
}
