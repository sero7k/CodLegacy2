//! A Ghosts map ready to draw: its zones read once (the map, the
//! technique set zones that hold its techniques and programs, the shared
//! zones that hold what it only names), its materials, the world and
//! placed model meshes, the draws, the placed models' light (light grid
//! tables, baked vertex light, ground light), lightmaps, probes, sun, fog
//! and tone. Pictures are read on demand.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use fastfile_iw6::image::{ImagePaks, image_pixels};
use fastfile_iw6::layout as L;
use fastfile_iw6::walk::{AssetId, Capture, MaterialConstant};
use glam::{Mat4, Vec3};

use crate::code::code_index;
use crate::look::{Fog, FogConsts, Tone, Vision};
use crate::pool::{Pool, key};

pub struct Zone {
    pub path: PathBuf,
    pub zone: fastfile_iw6::Zone,
    pub cap: Capture,
}

/// A mesh's vertex layout.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum VertexKind {
    /// `GfxWorldVertex`, 44 bytes.
    World,
    /// `GfxPackedVertex` (model surfaces), 32 bytes.
    Model,
}

impl VertexKind {
    pub fn stride(self) -> u64 {
        match self {
            Self::World => 44,
            Self::Model => 32,
        }
    }

    /// Where a vertex shader input is in the vertex (offset, format).
    pub fn attribute(self, semantic: &str, index: u32) -> Option<(u64, wgpu::VertexFormat)> {
        use wgpu::VertexFormat as F;
        let packed = F::Unorm10_10_10_2;
        let s = semantic.to_ascii_uppercase();
        Some(match (self, s.as_str(), index) {
            (_, "POSITION" | "SV_POSITION", 0) => (0, F::Float32x4),
            (_, "COLOR", 0) => (16, F::Unorm8x4),
            // World: texture uv then lightmap uv (`TEXCOORD0.xy`, `.zw`).
            (Self::World, "TEXCOORD", 0) => (20, F::Float32x4),
            (Self::World, "NORMAL", 0) => (36, packed),
            (Self::World, "TANGENT", 0) | (Self::World, "TEXCOORD", 2) => (40, packed),
            (Self::Model, "TEXCOORD", 0) => (20, F::Float16x2),
            (Self::Model, "NORMAL", 0) => (24, packed),
            (Self::Model, "TANGENT", 0) | (Self::Model, "TEXCOORD", 2) => (28, packed),
            _ => return None,
        })
    }
}

/// The baked vertex light stream of a vertex-lit placement
/// (`GfxStaticModelVertexLighting`, 20 bytes a vertex): `TEXCOORD4`
/// visibility (4 bytes), `TEXCOORD5` ambient and `TEXCOORD6` highlight
/// colours (four halves each).
pub fn vertex_light_input(semantic: &str, index: u32) -> Option<(u64, wgpu::VertexFormat)> {
    use wgpu::VertexFormat as F;
    if !semantic.eq_ignore_ascii_case("TEXCOORD") {
        return None;
    }
    Some(match index {
        4 => (0, F::Unorm8x4),
        5 => (4, F::Float16x4),
        6 => (12, F::Float16x4),
        _ => return None,
    })
}

pub const VERTEX_LIGHT_STRIDE: u64 = 20;

#[derive(Clone, Debug, Default)]
pub struct Material {
    pub name: String,
    pub techset: String,
    /// (name hash, sampler state, semantic, image name)
    pub textures: Vec<(u32, u8, u8, String)>,
    pub constants: Vec<MaterialConstant>,
    /// `GfxStateBits` blocks and, per technique slot, the first block its
    /// passes use (255: none).
    pub state_bits: Vec<[u8; 28]>,
    pub state_entry: Vec<u8>,
    pub sort_key: u8,
    pub camera_region: u8,
}

impl Material {
    pub fn state(&self, slot: usize, pass: usize) -> crate::state::PassState {
        match self.state_entry.get(slot) {
            Some(&e) if e != 255 => self
                .state_bits
                .get(usize::from(e) + pass)
                .map_or_else(Default::default, |b| crate::state::PassState::from_bits(b)),
            _ => Default::default(),
        }
    }
}

/// Which surfaces a draw belongs to (the world's draw lists).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum List {
    Opaque,
    Decal,
    Trans,
    Emissive,
    Sky,
    Model,
}

/// One draw: a run of a surface's triangles.
#[derive(Clone, Copy, Debug)]
pub struct Draw {
    pub mesh: usize,
    pub object: Option<u32>,
    pub material: usize,
    pub lightmap: u8,
    pub probe: u8,
    pub first_index: u32,
    pub index_count: u32,
    pub base_vertex: i32,
    /// Byte offset of its run in the model mesh's vertex light stream.
    pub layer: Option<u32>,
    /// Whether its primary light is the sun.
    pub sun: bool,
    /// Its primary light when that is a spot or omni light (`Scene::lights`).
    pub light: Option<u16>,
    pub list: List,
    /// The distances from its object's origin it is drawn between (its LOD's
    /// band, and the placement's cull distance), before the quality's
    /// scales; None: at any distance.
    pub range: Option<[f32; 2]>,
    /// The world surface or static placement it draws (the cells that
    /// hold it decide whether it is seen); None: not in the cells.
    pub cell: Option<crate::cells::CellItem>,
    /// Whether it goes into the sun's and the spot lights' shadow maps
    /// (`casts`).
    pub casts: [bool; 2],
}

/// `BO2MP_IW6_CASTER_FLAGS=0`: every surface with a shadow technique casts
/// (the zone's no-shadow flags not read).
pub fn caster_flags_on() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| !std::env::var("BO2MP_IW6_CASTER_FLAGS").is_ok_and(|v| v.trim() == "0"))
}

/// `BO2MP_IW6_SHADOW_OPT=0`: the sun's casters without the map's own
/// optimised caster sets (`r_useShadowGeomOpt` off): every surface and
/// placement not flagged out casts.
pub fn shadow_opt_on() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| !std::env::var("BO2MP_IW6_SHADOW_OPT").is_ok_and(|v| v.trim() == "0"))
}

/// The bit of the map's optimised sun caster sets a frame uses, when the
/// map has them (`sunShadowOptCount` > 0) and they are on
/// (`r_useShadowGeomOpt`, default on; `shadow_opt_on`). READ: set `k` is
/// `min(sun light, sunShadowOptCount) - 1`, the sun light being the first
/// sun primary light seen (1 to `lastSunPrimaryLightIndex`). INFERRED: the
/// first one, 1 (one sun on every map here); and a further per-view switch
/// the placement cull tests alongside is taken as on.
pub fn shadow_opt_bit(opt_count: u32) -> Option<u32> {
    (caster_flags_on() && shadow_opt_on() && opt_count > 0).then_some(0)
}

/// Whether a world surface with these `flags` (`GfxSurface` light flags)
/// casts into the sun's shadow map and a spot light's. READ: at load the
/// engine marks a surface of the world model in its sun caster bits
/// (`surfaceCastsSunShadow`) when bit 0 of the flags is set, and in
/// optimised set `k` (`surfaceCastsSunShadowOpt`) when bit `3 + k` is set
/// too; the sun cascades draw the set's bits when there is a set
/// (`shadow_opt_bit`), else the caster bits. On mp_frag 17,510 of 18,001
/// surfaces have bit 0 and 3,824 of those bit 3. INFERRED: the spot
/// lights' casters are not filtered by it (their lists are the zone's own
/// per-light shadow geometry, not read).
pub fn surface_casts(flags: u8, opt: Option<u32>) -> [bool; 2] {
    if !caster_flags_on() {
        return [true; 2];
    }
    let sun = flags & 1 != 0 && opt.is_none_or(|k| u32::from(flags) >> (3 + k) & 1 != 0);
    [sun, true]
}

/// Whether a static placement with these `flags` and `sun_flags`
/// (`GfxStaticModelDrawInst::flags`, `sunShadowFlags`) of a model with
/// these `model_flags` (`XModel`) casts into the sun's shadow map and a
/// spot light's. READ: the sun cascades' placement cull leaves out
/// placements with flag 0x10, models with flag 2 and, with an optimised
/// set `k` (`shadow_opt_bit`), placements without bit `k` of their sun
/// flags; the spot lights' shadow geometry leaves out flag 0x10 and model
/// flag 2. On mp_frag 3,706 of 8,779 placements have 0x10 and 1,577 of the
/// rest bit 0 of their sun flags.
pub fn placement_casts(flags: u16, sun_flags: u8, model_flags: u8, opt: Option<u32>) -> [bool; 2] {
    if !caster_flags_on() {
        return [true; 2];
    }
    let cast = model_flags & 2 == 0 && flags & 0x10 == 0;
    [cast && opt.is_none_or(|k| u32::from(sun_flags) >> k & 1 != 0), cast]
}

/// Code constants one object sets for itself, by index, and its placement.
#[derive(Clone, Debug, Default)]
pub struct Object {
    pub rows: Vec<(u16, Vec<[f32; 4]>)>,
    pub world: Mat4,
    /// Its model lighting table, if it has one (index into `tiles`).
    pub tile: Option<usize>,
}

/// What a moving entity is known by (the sim and this scene name it the
/// same way): its brush model, or its model and where the map put it.
#[derive(Clone, Debug, PartialEq)]
pub enum MoverKey {
    Brush(u32),
    Model { name: String, origin: [f32; 3] },
    /// A model a script spawned or swapped in: one of the hidden instances kept of it.
    Spawned { name: String, slot: u32 },
}

impl MoverKey {
    /// Whether `other` names the same entity (a model's place within a unit).
    pub fn matches(&self, other: &MoverKey) -> bool {
        match (self, other) {
            (Self::Brush(a), Self::Brush(b)) => a == b,
            (Self::Model { name: a, origin: p }, Self::Model { name: b, origin: q }) => {
                a.eq_ignore_ascii_case(b) && (0..3).all(|k| (p[k] - q[k]).abs() < 1.0)
            }
            (Self::Spawned { name: a, slot: p }, Self::Spawned { name: b, slot: q }) => a.eq_ignore_ascii_case(b) && p == q,
            _ => false,
        }
    }
}

static POOL_MODELS: std::sync::RwLock<Vec<String>> = std::sync::RwLock::new(Vec::new());

/// The models of the map zone the map's scripts may spawn or swap in (from the sim's script
/// pack, set before the scene loads): a few hidden instances of each are kept to be shown where
/// the scripts put them (`MoverKey::Spawned`).
pub fn set_pool_models(names: Vec<String>) {
    if let Ok(mut p) = POOL_MODELS.write() {
        *p = names;
    }
}

/// Hidden instances kept of each such model (`BO2MP_IW6_SPAWN_POOL`, 0 turns it off).
pub fn pool_slots() -> u32 {
    std::env::var("BO2MP_IW6_SPAWN_POOL").ok().and_then(|v| v.trim().parse().ok()).unwrap_or(4)
}

/// An entity a script may move: the objects that draw it.
#[derive(Clone, Debug)]
pub struct Mover {
    pub key: MoverKey,
    pub objects: Vec<u32>,
    /// A skinned model drawn in parts: (object, bone) per part (`skin`); empty for the rest.
    pub bones: Vec<(u32, u16)>,
}

/// The map's compiled scripts run in the sim (`BO2MP_IW6_SCRIPTS=0` turns that off): every
/// script model may then be hidden, moved or deleted.
pub fn script_movers_on() -> bool {
    !std::env::var("BO2MP_IW6_SCRIPTS").is_ok_and(|v| v.trim() == "0")
}

/// `BO2MP_IW6_SKIN=0`: script models that follow several bones are drawn whole at their place
/// (no animation of their parts).
pub fn skin_on() -> bool {
    !std::env::var("BO2MP_IW6_SKIN").is_ok_and(|v| v.trim() == "0")
}

/// `BO2MP_IW6_DYNENTS=0`: the clip map's dynamic entities (clutter,
/// destructibles, the scriptables' bodies) are not drawn.
pub fn dynents_on() -> bool {
    !std::env::var("BO2MP_IW6_DYNENTS").is_ok_and(|v| v.trim() == "0")
}

/// `BO2MP_IW6_SCRIPT_PROPS=0`: the scriptables, `script_model`s and
/// `script_brushmodel`s the map places as it starts are not drawn.
pub fn script_props_on() -> bool {
    !std::env::var("BO2MP_IW6_SCRIPT_PROPS").is_ok_and(|v| v.trim() == "0")
}

/// An entity's world matrix from its origin and axes (forward, left, up).
pub fn placement(origin: [f32; 3], axis: [[f32; 3]; 3], scale: f32) -> (Vec<[f32; 4]>, Mat4) {
    let a = axis;
    let rows: Vec<[f32; 4]> = (0..3)
        .map(|i| [a[0][i] * scale, a[1][i] * scale, a[2][i] * scale, origin[i]])
        .chain(std::iter::once([0.0, 0.0, 0.0, 1.0]))
        .collect();
    let m = Mat4::from_cols_array_2d(&[rows[0], rows[1], rows[2], rows[3]]).transpose();
    (rows, m)
}

/// Owned vertices and indices.
pub struct MeshData {
    pub kind: VertexKind,
    pub vertex_bytes: Vec<u8>,
    pub indices: Vec<u16>,
    /// The vertex light stream (model mesh only).
    pub layer_bytes: Vec<u8>,
}

/// The sun as the engine passes it (`LIGHT_*` of the primary light).
#[derive(Clone, Copy, Debug)]
pub struct SunLight {
    /// Toward the sun.
    pub dir: Vec3,
    pub diffuse: Vec3,
    pub specular: Vec3,
}

/// One `ComPrimaryLight`: 0 none, 1 the sun, 2 spot, 3 omni.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct PrimaryLight {
    pub ty: u8,
    pub exponent: u8,
    /// Linear colour (HDR).
    pub color: [f32; 3],
    /// The direction it shines (spot).
    pub dir: [f32; 3],
    pub origin: [f32; 3],
    pub radius: f32,
    pub cos_half_fov_outer: f32,
    pub cos_half_fov_inner: f32,
    /// Its light def's `lmapLookupStart` (where the compiler placed the
    /// def's falloff ramp in the lightmap pages) and the width of the
    /// def's attenuation picture (the ramp's length in texels), when the
    /// zones hold the def.
    pub lookup_start: Option<i32>,
    pub falloff_width: Option<u16>,
}

impl PrimaryLight {
    pub fn read(image: &[u8], at: usize) -> Self {
        use L::ComPrimaryLight as P;
        let v3 = |o: usize| [f32_at(image, at + o), f32_at(image, at + o + 4), f32_at(image, at + o + 8)];
        Self {
            ty: u8_at(image, at + P::r#type),
            exponent: u8_at(image, at + P::exponent),
            color: v3(P::color),
            dir: v3(P::dir),
            origin: v3(P::origin),
            radius: f32_at(image, at + P::radius),
            cos_half_fov_outer: f32_at(image, at + P::cosHalfFovOuter),
            cos_half_fov_inner: f32_at(image, at + P::cosHalfFovInner),
            lookup_start: None,
            falloff_width: None,
        }
    }
}

/// Pictures by name from the zones (and the `imagefile<N>.pak`s beside
/// them). A streamed picture whose largest parts are not there is read at
/// the largest size that is.
pub struct ZoneImages {
    zones: std::sync::Arc<Vec<Zone>>,
    paks: Vec<ImagePaks>,
    by_name: HashMap<String, Vec<(usize, usize)>>,
    /// Pictures a missing one is drawn with instead (name -> stand-in).
    pub stand_ins: HashMap<String, String>,
    /// Pictures built here (the sky's stand-in), six faces: name -> pixels.
    pub synthetic: HashMap<String, fastfile_h1::image::ImagePixels>,
    pub max_size: u32,
    /// Pictures read at less than their full size because a part is not
    /// there.
    pub reduced: HashSet<String>,
}

impl ZoneImages {
    /// (pixels, faces: 1 or 6).
    pub fn image(&mut self, name: &str) -> Option<(fastfile_h1::image::ImagePixels, u32)> {
        let k = key(name);
        let got = self.read_named(&k);
        if got.is_some() {
            return got;
        }
        if let Some(px) = self.synthetic.get(&k) {
            return Some((px.clone(), 6));
        }
        let s = self.stand_ins.get(&k)?.clone();
        self.read_named(&s)
    }

    fn read_named(&mut self, k: &str) -> Option<(fastfile_h1::image::ImagePixels, u32)> {
        if let Some(px) = fastfile_iw6::shared::builtin_pixels(k) {
            return Some((to_h1(px), 1));
        }
        let candidates = self.by_name.get(k)?.clone();
        candidates.into_iter().find_map(|(z, i)| self.read(z, i))
    }

    fn read(&mut self, z: usize, i: usize) -> Option<(fastfile_h1::image::ImagePixels, u32)> {
        let l = &self.zones[z];
        let rec = &l.cap.images[i];
        // map_type 5, streamed (the sky): read straight from its pak, the
        // largest part that reads and fits the size cap.
        if rec.map_type == 5 && rec.streamed {
            return streamed_cube(rec, &mut self.paks[z], self.max_size.max(1024)).map(|px| (px, 6));
        }
        // map_type 5: a cube, its six faces one after another (inline).
        if rec.map_type == 5 {
            let data = rec.pixels?.get(&l.zone.image).to_vec();
            return Some((
                fastfile_h1::image::ImagePixels {
                    width: u32::from(rec.width),
                    height: u32::from(rec.height),
                    levels: u32::from(rec.level_count.max(1)),
                    dxgi_format: rec.format,
                    data,
                },
                6,
            ));
        }
        // The engine's own pictures (lightmaps, `*`) at full size.
        let cap = if rec.name.starts_with('*') { u32::MAX } else { self.max_size };
        match image_pixels(rec, &l.zone.image, &mut self.paks[z], cap) {
            Ok(px) => Some((to_h1(px), 1)),
            Err(_) if rec.streamed => {
                // The largest part that reads.
                let mut sizes: Vec<u32> = rec
                    .streams
                    .iter()
                    .map(|s| u32::from(s.0.max(s.1)))
                    .filter(|&s| s > 0 && s < cap)
                    .collect();
                sizes.sort_unstable_by(|a, b| b.cmp(a));
                sizes.dedup();
                let px = sizes
                    .into_iter()
                    .find_map(|s| image_pixels(rec, &l.zone.image, &mut self.paks[z], s).ok())?;
                self.reduced.insert(key(&rec.name));
                Some((to_h1(px), 1))
            }
            Err(_) => None,
        }
    }
}

/// A streamed cube's top level: each part above the first adds one level
/// (`GfxImageStreamData`: its size and the cumulative bytes), which for a
/// cube is that level's six faces one after another. Only that level is
/// kept (one mip level: the order of the faces' smaller levels inside the
/// first part is not read; the sky is sampled at its top level), so the
/// faces are laid out the same either way.
fn streamed_cube(
    rec: &fastfile_iw6::walk::ImageRec,
    paks: &mut ImagePaks,
    cap: u32,
) -> Option<fastfile_h1::image::ImagePixels> {
    for k in (0..4).rev() {
        let (w, h, cum, _) = rec.streams[k];
        let Some(part) = rec.stream_files[k] else { continue };
        if w == 0 || u32::from(w.max(h)) > cap {
            continue;
        }
        let before = if k == 0 { 0 } else { rec.streams[k - 1].2 };
        let want = cum.saturating_sub(before) as usize;
        let face = fastfile_iw6::image::level_bytes(rec.format, u32::from(w), u32::from(h))?;
        let Some(bytes) = paks.read(&part, want) else { continue };
        if bytes.len() < 6 * face {
            continue;
        }
        return Some(fastfile_h1::image::ImagePixels {
            width: u32::from(w),
            height: u32::from(h),
            levels: 1,
            dxgi_format: rec.format,
            data: bytes[..6 * face].to_vec(),
        });
    }
    None
}

fn to_h1(px: fastfile_iw6::image::ImagePixels) -> fastfile_h1::image::ImagePixels {
    fastfile_h1::image::ImagePixels {
        width: px.width,
        height: px.height,
        levels: px.levels,
        dxgi_format: px.dxgi_format,
        data: px.data,
    }
}

/// Per xmodel: (material, base vertex, first index, index count, first
/// vertex in the placement's vertex light, vertex count) per surface.
type Placed = Vec<(Option<AssetId>, i32, u32, u32, u32, u32)>;

/// `BO2MP_IW6_LOD=0`: every static model draws its first LOD at any
/// distance (no LOD bands, no cull distance), as before.
pub fn lods_on() -> bool {
    !std::env::var("BO2MP_IW6_LOD").is_ok_and(|v| v.trim() == "0")
}

/// A model's LOD `lod` appended to the model mesh, its surfaces.
fn model_surfaces(
    cap: &Capture,
    image: &[u8],
    xm: &fastfile_iw6::walk::XModelRec,
    lod: usize,
    model_verts: &mut Vec<u8>,
    model_idx: &mut Vec<u16>,
) -> Placed {
    let mut out = Vec::new();
    // The placement's baked vertex light holds every LOD's vertices in
    // order, each surface's run rounded up to an even count (READ from the
    // data: on mp_frag all 96 vertex-lit models' value counts are exactly
    // that sum): where this LOD's runs start.
    let even = |n: u16| u32::from(n).next_multiple_of(2);
    let lod_start: u32 = xm.lods[..lod.min(xm.lods.len())]
        .iter()
        .filter_map(|l| l.surfs.and_then(|s| cap.model_surfs.get(s.index as usize)))
        .flat_map(|ms| ms.surfs.iter().map(|x| even(x.vert_count)))
        .sum();
    let Some(lod) = xm.lods.get(lod) else { return out };
    let Some(ms) = lod.surfs.and_then(|s| cap.model_surfs.get(s.index as usize)) else {
        return out;
    };
    let mut vstart = lod_start;
    for (j, surf) in ms.surfs.iter().enumerate() {
        let surf_vstart = vstart;
        vstart += even(surf.vert_count);
        let (Some(v), Some(t)) = (surf.verts, surf.tris) else { continue };
        let base = (model_verts.len() / 32) as i32;
        model_verts.extend_from_slice(v.get(image));
        let first = model_idx.len() as u32;
        model_idx.extend(t.get(image).chunks_exact(2).map(|b| u16::from_le_bytes([b[0], b[1]])));
        let mat = xm.materials.get(usize::from(lod.surf_index) + j).copied().flatten();
        out.push((mat, base, first, u32::from(surf.tri_count) * 3, surf_vstart, u32::from(surf.vert_count)));
    }
    out
}

pub struct Scene {
    pub name: String,
    pub zones: std::sync::Arc<Vec<Zone>>,
    pub pool: std::sync::Arc<Pool>,
    pub materials: Vec<Material>,
    /// 0: the world, 1: placed models.
    pub meshes: Vec<MeshData>,
    pub objects: Vec<Object>,
    pub draws: Vec<Draw>,
    /// Per light grid table: 56 colours (the 4 x 4 x 4 shell, lattice
    /// order) and the sun's visibility.
    pub tiles: Vec<([[f32; 3]; 56], f32)>,
    pub lightmaps: Vec<(Option<String>, Option<String>)>,
    /// Each world vertex format's stride in the layer stream, as the
    /// stream's runs give it (`layers::with_strides`).
    pub layer_strides: HashMap<u8, u32>,
    /// The lightmap secondary's width in texels (the falloff ramps'
    /// placement is in its units).
    pub lightmap_width: u32,
    /// Every spot and omni light's def is in the zones, so the shaders read
    /// the defs' falloff ramps from the pages (`local_light`).
    pub falloff_from_defs: bool,
    pub probes: Vec<Option<String>>,
    pub probe_origins: Vec<Vec3>,
    /// The map's primary lights (`ComPrimaryLight`), by index.
    pub lights: Vec<PrimaryLight>,
    pub sun: SunLight,
    pub vision: Vision,
    /// The map's sun flare, blind and glare record (`sun_fx`).
    pub sun_flare: Option<fastfile_iw6::walk::SunFlareRec>,
    pub fog: FogConsts,
    pub tone: Tone,
    /// The vision's colorization set (`colorizationSet`), if the zones
    /// hold it.
    pub colorization: Option<crate::look::Colorization>,
    /// `HDR_SKY_INTENSITY` (`r_hdrSkyIntensity` times its tint).
    pub sky_intensity: [f32; 4],
    /// The world's bounds (the sun shadow's depth range).
    pub bounds: (Vec3, Vec3),
    pub entities: String,
    pub images: ZoneImages,
    pub notes: Vec<String>,
    /// Entities a script may move (`Host::set_mover_pose`).
    pub movers: Vec<Mover>,
    /// What the map's own script data drives (`fastfile_iw6::maplogic`).
    pub logic: fastfile_iw6::maplogic::MapLogic,
    /// The map's cells and portals (`cells`), when the zone holds them.
    pub cells: Option<crate::cells::Cells>,
}

fn u8_at(image: &[u8], at: usize) -> u8 {
    image.get(at).copied().unwrap_or(0)
}

fn u16_at(image: &[u8], at: usize) -> u16 {
    image.get(at..at + 2).map_or(0, |b| u16::from_le_bytes([b[0], b[1]]))
}

fn u32_at(image: &[u8], at: usize) -> u32 {
    image
        .get(at..at + 4)
        .map_or(0, |b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
}

fn f32_at(image: &[u8], at: usize) -> f32 {
    image
        .get(at..at + 4)
        .map_or(0.0, |b| f32::from_le_bytes([b[0], b[1], b[2], b[3]]))
}

impl Scene {
    /// The index of the material called `name`: one the scene already holds,
    /// or the first the zones hold under that name (a material no surface of
    /// the map names, an effect's decal), added to the scene. None when no
    /// zone holds it or its technique set is not among the zones' sets.
    pub fn material_named(&mut self, name: &str) -> Option<usize> {
        let k = key(name);
        if let Some(i) = self.materials.iter().position(|m| key(&m.name) == k) {
            return Some(i);
        }
        let (l, m) = self.zones.iter().find_map(|l| {
            l.cap
                .materials
                .iter()
                .find(|m| m.techset.is_some() && !m.name.starts_with(',') && key(&m.name) == k)
                .map(|m| (l, m))
        })?;
        let techset = key(l.cap.name_of(m.techset?));
        if !self.pool.techsets.contains_key(&techset) {
            return None;
        }
        self.materials.push(Material {
            name: m.name.clone(),
            techset,
            textures: m
                .textures
                .iter()
                .map(|t| (t.name_hash, t.sampler_state, t.semantic, t.image.map_or(String::new(), |i| key(l.cap.name_of(i)))))
                .collect(),
            constants: m.constants.clone(),
            state_bits: m.state_bits.clone(),
            state_entry: m.state_bits_entry.clone(),
            sort_key: m.sort_key,
            camera_region: m.camera_region,
        });
        Some(self.materials.len() - 1)
    }

    /// The zones a map needs, from the map's folder: the map, its
    /// technique set zone and patch, then the shared technique set zones
    /// and the shared zones (`fastfile_iw6::shared::SHARED_ZONES`), where
    /// present.
    pub fn zones_for(map: &Path) -> Vec<PathBuf> {
        let name = map
            .file_stem()
            .map_or(String::new(), |s| s.to_string_lossy().into_owned());
        let dir = map.parent().unwrap_or(Path::new("."));
        let mut out = vec![map.to_path_buf()];
        let mut names = vec![
            format!("techsets_{name}.ff"),
            format!("patch_{name}.ff"),
            "techsets_code_post_gfx_mp.ff".into(),
            "techsets_common_core_mp.ff".into(),
            "techsets_common_mp.ff".into(),
            "techsets_common.ff".into(),
        ];
        names.extend(fastfile_iw6::shared::SHARED_ZONES.iter().map(|z| format!("{z}.ff")));
        for z in names {
            let p = dir.join(&z);
            if p.is_file() && !out.contains(&p) {
                out.push(p);
            }
        }
        out
    }

    /// `zones[0]` is the map; the others supply what it only names.
    pub fn load(zones: &[PathBuf]) -> Result<Self, String> {
        crate::loadtime::stage("(before load)");
        // (The map's baked pictures stream into the file cache while the
        // zones open.)
        crate::bake::read_ahead(zones);
        let mut notes = Vec::new();
        let mut loaded: Vec<Zone> = Vec::with_capacity(zones.len());
        // The zones open (decompress) side by side: the big ones take over a
        // second each. `BO2MP_IW6_LOAD_SERIAL=1`: one after the other.
        let mut opened: Vec<Option<Result<fastfile_iw6::Zone, String>>> = if zones.len() > 1 && !crate::loadtime::serial() {
            let next = std::sync::atomic::AtomicUsize::new(0);
            let slots: Vec<std::sync::Mutex<Option<Result<fastfile_iw6::Zone, String>>>> = zones.iter().map(|_| std::sync::Mutex::new(None)).collect();
            let workers = std::thread::available_parallelism().map_or(2, |n| n.get()).clamp(2, 4);
            std::thread::scope(|sc| {
                for _ in 0..workers {
                    sc.spawn(|| {
                        loop {
                            let i = next.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                            let Some(p) = zones.get(i) else { break };
                            let r = fastfile_iw6::Zone::open(p).map_err(|e| e.to_string());
                            *slots[i].lock().unwrap_or_else(|e| e.into_inner()) = Some(r);
                        }
                    });
                }
            });
            slots.into_iter().map(|m| m.into_inner().unwrap_or_else(|e| e.into_inner())).collect()
        } else {
            zones.iter().map(|_| None).collect()
        };
        for (i, p) in zones.iter().enumerate() {
            let zone = match opened[i].take().unwrap_or_else(|| fastfile_iw6::Zone::open(p).map_err(|e| e.to_string())) {
                Ok(z) => z,
                Err(e) if i == 0 => return Err(format!("{}: {e}", p.display())),
                Err(e) => {
                    notes.push(format!("{}: left out, does not open ({e})", p.display()));
                    continue;
                }
            };
            crate::loadtime::stage(&format!("zone open {}", p.display()));
            let (cap, stop) = fastfile_iw6::walk::walk_zone(&zone);
            crate::loadtime::stage("zone walk");
            if let Some(s) = stop {
                notes.push(format!(
                    "{}: walk stopped at asset {} ({:?})",
                    p.display(),
                    s.asset_index,
                    s.asset_type
                ));
            }
            // A shared zone keeps only what a map can name of it (its
            // pictures and materials); the rest of its image is dropped
            // with it if nothing is kept.
            loaded.push(Zone {
                path: p.clone(),
                zone,
                cap,
            });
        }
        let name = zones[0]
            .file_stem()
            .map_or(String::new(), |s| s.to_string_lossy().into_owned());
        let mut pool = Pool::default();
        for l in loaded.iter() {
            pool.add(&l.cap, &l.zone.image);
        }
        let mut mats_by_name: HashMap<String, (usize, usize)> = HashMap::new();
        let mut imgs_by_name: HashMap<String, Vec<(usize, usize)>> = HashMap::new();
        for (zi, l) in loaded.iter().enumerate() {
            for (mi, m) in l.cap.materials.iter().enumerate() {
                if m.techset.is_some() && !m.name.starts_with(',') {
                    mats_by_name.entry(key(&m.name)).or_insert((zi, mi));
                }
            }
            for (ii, r) in l.cap.images.iter().enumerate() {
                if !fastfile_iw6::shared::is_reference(r) {
                    imgs_by_name.entry(key(&r.name)).or_default().push((zi, ii));
                }
            }
        }
        let map_l = loaded.first().ok_or("no map zone")?;
        let image = &map_l.zone.image;
        let map = fastfile_iw6::map::read_map(&map_l.cap, image, &map_l.zone.assets.script_strings);
        if std::env::var("IW6_DBG_SPAWNS").is_ok() {
            // (Offline tools: where the map's spawns stand, to pick cameras.)
            for s in map.spawns.iter().filter(|s| s.classname.contains("spawn")) {
                eprintln!("spawn {} {:?} {:?}", s.classname, s.origin, s.angles);
            }
        }
        crate::loadtime::stage("read_map");
        notes.extend(map.notes.iter().cloned());
        let world = map.world.as_ref().ok_or("no world in the map zone")?;
        let g = map_l.cap.gfx_worlds.first().ok_or("no GfxWorld record")?;
        let sun_flare = g.sun;
        // The map's optimised sun caster set in use, if any (`casts`).
        let sun_opt = shadow_opt_bit(u32_at(image, g.header + L::GfxWorld::dpvs__sunShadowOptCount));

        let mut materials: Vec<Material> = Vec::new();
        let mut mat_index: HashMap<String, Option<usize>> = HashMap::new();
        let mut unresolved = 0usize;
        // (The zone whose capture names the asset ids handed in: the map's, but for a model
        // another zone holds, which is drawn with its own zone's material records.)
        let cur_zone = std::cell::Cell::new(0usize);
        let mut material_of = |id: AssetId| -> Option<usize> {
            let nm = key(loaded[cur_zone.get()].cap.name_of(id));
            if let Some(&i) = mat_index.get(&nm) {
                return i;
            }
            let Some(&(zi, mi)) = mats_by_name.get(&nm) else {
                unresolved += 1;
                mat_index.insert(nm, None);
                return None;
            };
            let l = &loaded[zi];
            let m = &l.cap.materials[mi];
            materials.push(Material {
                name: m.name.clone(),
                techset: m.techset.map_or(String::new(), |t| key(l.cap.name_of(t))),
                textures: m
                    .textures
                    .iter()
                    .map(|t| {
                        (
                            t.name_hash,
                            t.sampler_state,
                            t.semantic,
                            t.image.map_or(String::new(), |i| key(l.cap.name_of(i))),
                        )
                    })
                    .collect(),
                constants: m.constants.clone(),
                state_bits: m.state_bits.clone(),
                state_entry: m.state_bits_entry.clone(),
                sort_key: m.sort_key,
                camera_region: m.camera_region,
            });
            mat_index.insert(nm, Some(materials.len() - 1));
            Some(materials.len() - 1)
        };

        // Which primary light environments hold the sun (light type 1).
        let sun_envs: Vec<bool> = map_l
            .cap
            .com_worlds
            .first()
            .and_then(|cw| {
                let (lights, envs) = (cw.primary_lights?, cw.primary_light_envs?);
                let is_sun = |i: usize| {
                    i < cw.primary_light_count as usize
                        && u8_at(image, lights.pos + L::ComPrimaryLight::SIZE * i) == 1
                };
                Some(
                    (0..envs.len / L::ComPrimaryLightEnv::SIZE)
                        .map(|e| {
                            let at = envs.pos + L::ComPrimaryLightEnv::SIZE * e;
                            let n = usize::from(u16_at(image, at + 8)).min(4);
                            (0..n).any(|k| is_sun(usize::from(u16_at(image, at + 2 * k))))
                        })
                        .collect(),
                )
            })
            .unwrap_or_default();
        let env_sun = |e: usize| sun_envs.get(e).copied().unwrap_or(e == 1);
        // The map's primary lights, and each environment's first spot or
        // omni light (its primary light when the sun is not in it).
        let lights: Vec<PrimaryLight> = map_l
            .cap
            .com_worlds
            .first()
            .and_then(|cw| cw.primary_lights)
            .map(|sp| {
                (0..sp.len / L::ComPrimaryLight::SIZE)
                    .map(|i| PrimaryLight::read(image, sp.pos + L::ComPrimaryLight::SIZE * i))
                    .collect()
            })
            .unwrap_or_default();
        // Each light's def (from whichever zone holds it): its falloff
        // ramp's place in the lightmap pages.
        let mut lights = lights;
        if let Some(cw) = map_l.cap.com_worlds.first() {
            for (l, def) in lights.iter_mut().zip(&cw.primary_light_defs) {
                let Some(def) = def.as_deref().map(|d| d.trim_start_matches(',').to_ascii_lowercase()) else { continue };
                let Some(rec) = loaded.iter().find_map(|z| z.cap.light_defs.get(&def)) else { continue };
                l.lookup_start = Some(rec.lmap_lookup_start);
                l.falloff_width = rec.attenuation.as_deref().and_then(|a| {
                    imgs_by_name.get(&key(a)).and_then(|v| v.first()).map(|&(zi, ii)| loaded[zi].cap.images[ii].width)
                });
            }
        }
        let env_light: Vec<Option<u16>> = map_l
            .cap
            .com_worlds
            .first()
            .and_then(|cw| cw.primary_light_envs)
            .map(|envs| {
                (0..envs.len / L::ComPrimaryLightEnv::SIZE)
                    .map(|e| {
                        let at = envs.pos + L::ComPrimaryLightEnv::SIZE * e;
                        let n = usize::from(u16_at(image, at + 8)).min(4);
                        (0..n)
                            .map(|k| u16_at(image, at + 2 * k))
                            .find(|&i| lights.get(usize::from(i)).is_some_and(|l| matches!(l.ty, 2 | 3)))
                    })
                    .collect()
            })
            .unwrap_or_default();
        let light_of = |e: usize| if env_sun(e) { None } else { env_light.get(e).copied().flatten() };
        notes.push(format!(
            "iw6 lights: {} primary lights ({} spot, {} omni), {} environments with one as primary",
            lights.len(),
            lights.iter().filter(|l| l.ty == 2).count(),
            lights.iter().filter(|l| l.ty == 3).count(),
            env_light.iter().filter(|l| l.is_some()).count()
        ));
        if std::env::var("IW6_DBG_LIGHTS").is_ok() {
            for (i, l) in lights.iter().enumerate().filter(|(_, l)| matches!(l.ty, 2 | 3)).take(12) {
                eprintln!("light {i}: {l:?}");
            }
        }

        // World surfaces by list, and the sky.
        let mut draws = Vec::new();
        let mut seen = HashSet::new();
        let lists = [List::Opaque, List::Decal, List::Trans, List::Emissive];
        let sky: HashSet<u32> = world.sky_surfaces.iter().copied().collect();
        let mut ordered: Vec<(u32, List)> = Vec::new();
        for (k, &(b, e)) in world.ranges.iter().enumerate() {
            for i in b.min(e)..e {
                ordered.push((i, lists[k]));
            }
        }
        for &i in &world.sky_surfaces {
            ordered.push((i, List::Sky));
        }
        for (i, list) in ordered {
            if !seen.insert(i) {
                continue;
            }
            let Some(s) = world.surfaces.get(i as usize) else { continue };
            let Some(mi) = s.material.and_then(&mut material_of) else {
                continue;
            };
            let list = if sky.contains(&i) { List::Sky } else { list };
            // A layered surface: its run of the vertex layer stream.
            let layered = s
                .material
                .and_then(|id| mats_by_name.get(&key(map_l.cap.name_of(id))))
                .and_then(|&(zi, mj)| loaded[zi].cap.materials[mj].techset.map(|t| key(loaded[zi].cap.name_of(t))))
                .and_then(|ts| pool.world_vert_format.get(&ts).copied())
                .is_some_and(|f| f != 0);
            let layer = (layered && g.vertex_layer_data.is_some())
                .then(|| g.surfaces.map(|at| u32_at(image, at + L::GfxSurface::SIZE * i as usize)))
                .flatten();
            draws.push(Draw {
                mesh: 0,
                object: None,
                material: mi,
                lightmap: s.lightmap_index,
                probe: s.reflection_probe_index,
                first_index: s.base_index,
                index_count: u32::from(s.tri_count) * 3,
                base_vertex: s.first_vertex as i32,
                layer,
                sun: env_sun(usize::from(s.primary_light_env)),
                light: light_of(usize::from(s.primary_light_env)),
                list,
                range: None,
                cell: Some(crate::cells::CellItem::Surface(i)),
                casts: surface_casts(s.flags, sun_opt),
            });
        }
        let world_draws = draws.len();
        // Each world vertex format's stride in the layer stream, read from
        // the stream itself: a surface's run (to the next surface's start)
        // over its vertex count, the value most runs give.
        let layer_strides: HashMap<u8, u32> = {
            let mut runs: Vec<(u32, u32, u8)> = Vec::new();
            for (i, s) in world.surfaces.iter().enumerate() {
                let f = s
                    .material
                    .and_then(|id| mats_by_name.get(&key(map_l.cap.name_of(id))))
                    .and_then(|&(zi, mj)| loaded[zi].cap.materials[mj].techset.map(|t| key(loaded[zi].cap.name_of(t))))
                    .and_then(|ts| pool.world_vert_format.get(&ts).copied())
                    .unwrap_or(0);
                if f == 0 || s.vertex_count == 0 || g.vertex_layer_data.is_none() {
                    continue;
                }
                if let Some(at) = g.surfaces.map(|at| u32_at(image, at + L::GfxSurface::SIZE * i)) {
                    runs.push((at, u32::from(s.vertex_count), f));
                }
            }
            runs.sort_unstable();
            runs.dedup_by_key(|r| r.0);
            let mut votes: HashMap<u8, HashMap<u32, usize>> = HashMap::new();
            for w in runs.windows(2) {
                let (a, n, f) = w[0];
                let d = w[1].0.saturating_sub(a);
                if d > 0 && d % n == 0 {
                    *votes.entry(f).or_default().entry(d / n).or_default() += 1;
                }
            }
            votes
                .into_iter()
                .filter_map(|(f, v)| v.into_iter().max_by_key(|&(st, n)| (n, std::cmp::Reverse(st))).map(|(st, _)| (f, st)))
                .collect()
        };
        // Placed models: each xmodel's first LOD once in the model mesh.
        let mut model_verts: Vec<u8> = Vec::new();
        let mut model_idx: Vec<u16> = Vec::new();
        let mut model_layer: Vec<u8> = Vec::new();
        let mut placed: HashMap<(u32, usize), Placed> = HashMap::new();
        // (Models of another zone: by (zone, model) of `loaded`.)
        let mut placed_other: HashMap<(usize, usize), Placed> = HashMap::new();
        // LOD bands: (placements with more than one LOD, LOD draws added,
        // placements with a cull distance).
        let mut lod_counts = (0usize, 0usize, 0usize);
        let mut objects: Vec<Object> = Vec::new();
        let mut tiles: Vec<([[f32; 3]; 56], f32)> = Vec::new();
        let world_matrix = code_index("TRANSPOSE_WORLD_MATRIX0");
        let base_coords = code_index("BASE_LIGHTING_COORDS");
        let probe_ambient = code_index("LIGHT_PROBE_AMBIENT");
        let grid = g
            .light_grid
            .as_ref()
            .and_then(|r| fastfile_iw6::lightgrid::LightGrid::new(image, r));
        let dirs = fastfile_iw6::lightgrid::table_directions();
        let (mut grid_hits, mut grid_misses, mut vlit, mut vlit_short) = (0usize, 0usize, 0usize, 0usize);
        // Placements whose grid cell's environment holds the sun / does not.
        let mut grid_sun = (0usize, 0usize, 0usize);
        let sd = L::GfxStaticModelDrawInst::SIZE;
        let si = L::GfxStaticModelInst::SIZE;
        for (smi, sm) in world.static_models.iter().enumerate() {
            let Some(mid) = sm.model else { continue };
            let Some(xm) = map_l.cap.xmodels.get(mid.index as usize) else { continue };
            let d = g.smodels.map(|at| at + sd * smi);
            // The LODs drawn and each one's band: LOD i up to its own
            // distance, from the previous one's (READ: `XModelLodInfo.dist`
            // per LOD; INFERRED: the band is measured from the placement's
            // origin, unscaled by the view's zoom, and past the last LOD's
            // distance the model is not drawn when that distance is set);
            // all capped by the placement's `cullDist` (0: none).
            let cull = d.map_or(0, |d| u16_at(image, d + L::GfxStaticModelDrawInst::cullDist));
            // The model's own LOD count (`numLods`; the record always holds
            // six slots: the old `lods.len().clamp(1, 4)` made every model
            // four LODs, so a model of 5 or 6 LODs vanished past its fourth
            // distance and one of 2 or 3 had empty bands). `BO2MP_IW6_LOD_COUNT=0`
            // returns to the clamp.
            let lod_n = if !lods_on() {
                1
            } else if std::env::var("BO2MP_IW6_LOD_COUNT").is_ok_and(|v| v.trim() == "0") {
                xm.lods.len().clamp(1, 4)
            } else {
                let with_surfs = xm
                    .lods
                    .iter()
                    .take_while(|l| l.surfs.is_some())
                    .count();
                let n = if xm.num_lods > 0 { usize::from(xm.num_lods) } else { with_surfs };
                n.min(with_surfs.max(1)).clamp(1, xm.lods.len().max(1))
            };
            let mut bands: Vec<(usize, Option<[f32; 2]>)> = Vec::new();
            for li in 0..lod_n {
                if !lods_on() {
                    bands.push((0, None));
                    break;
                }
                let lo = if li == 0 { 0.0 } else { xm.lods[li - 1].dist };
                let mut hi = xm.lods[li].dist;
                if li + 1 == lod_n {
                    // The last LOD is the one drawn out to the placement's
                    // own `cullDist` (INFERRED: its `dist` is a switch
                    // distance for a LOD that is not there).
                    hi = f32::INFINITY;
                } else if !(hi > lo) {
                    continue;
                }
                if cull > 0 {
                    hi = hi.min(f32::from(cull));
                }
                if hi > lo {
                    bands.push((li, (lo > 0.0 || hi.is_finite()).then_some([lo, hi])));
                }
            }
            if bands.is_empty() {
                bands.push((0, None));
            }
            if bands.len() > 1 {
                lod_counts.0 += 1;
            }
            if cull > 0 {
                lod_counts.2 += 1;
            }
            let mut surfs: Vec<(Option<[f32; 2]>, Placed)> = Vec::new();
            for &(li, band) in &bands {
                let p = placed
                    .entry((mid.index, li))
                    .or_insert_with(|| model_surfaces(&map_l.cap, image, xm, li, &mut model_verts, &mut model_idx))
                    .clone();
                if li > 0 {
                    lod_counts.1 += p.len();
                }
                surfs.push((band, p));
            }
            let probe = d.map_or(0, |d| u8_at(image, d + L::GfxStaticModelDrawInst::reflectionProbeIndex));
            let env = d.map_or(1, |d| usize::from(u8_at(image, d + L::GfxStaticModelDrawInst::primaryLightEnvIndex)));
            let lighting_origin = g
                .smodel_insts
                .map(|at| {
                    let o = at + si * smi + L::GfxStaticModelInst::lightingOrigin;
                    [f32_at(image, o), f32_at(image, o + 4), f32_at(image, o + 8)]
                })
                .unwrap_or(sm.origin);
            // The light grid's table at the model's lighting origin.
            let table = grid.as_ref().and_then(|gr| gr.sample(lighting_origin)).map(|sh| {
                let mut t = [[0f32; 3]; 56];
                for (k, d) in dirs.iter().enumerate() {
                    t[k] = fastfile_iw6::lightgrid::evaluate(&sh, *d);
                }
                t
            });
            match table {
                Some(_) => grid_hits += 1,
                None => grid_misses += 1,
            }
            let ground = sm.ground_lighting;
            let table = table.unwrap_or_else(|| {
                // Outside the grid: the ground light (or a dim grey) in
                // every direction.
                let c = if ground[..3].iter().any(|v| *v > 0.0) {
                    [ground[0], ground[1], ground[2]]
                } else {
                    [0.3; 3]
                };
                [c; 56]
            });
            // The sun's visibility past the shadow map (the table's alpha,
            // what the `ld*` model shaders read as their baked visibility):
            // whether the primary light environment of the grid cell that
            // weighs most at the lighting origin holds the sun. A cell with
            // no environment (0) or no data: the placement's own
            // environment decides (INFERRED; on mp_frag 1,440 sunlit
            // placements' cells have environment 0). `BO2MP_IW6_GRID_SUN=0`:
            // full sun everywhere.
            let vis = match grid.as_ref().filter(|_| grid_sun_on()).and_then(|gr| gr.environment(lighting_origin)) {
                _ if !grid_sun_on() => 1.0,
                Some((e, _)) if e != 0 => {
                    if env_sun(e as usize) {
                        grid_sun.0 += 1;
                        1.0
                    } else {
                        grid_sun.1 += 1;
                        0.0
                    }
                }
                _ => {
                    grid_sun.2 += 1;
                    if env_sun(env) { 1.0 } else { 0.0 }
                }
            };
            tiles.push((table, vis));
            let tile = tiles.len() - 1;
            let (s, a) = (sm.scale, sm.axis);
            let rows: Vec<[f32; 4]> = (0..3)
                .map(|i| [a[0][i] * s, a[1][i] * s, a[2][i] * s, sm.origin[i]])
                .chain(std::iter::once([0.0, 0.0, 0.0, 1.0]))
                .collect();
            let world_m = Mat4::from_cols_array_2d(&[rows[0], rows[1], rows[2], rows[3]]).transpose();
            let mut obj = Object {
                rows: vec![(world_matrix, rows)],
                world: world_m,
                tile: Some(tile),
            };
            obj.rows.push((base_coords, vec![[tile as f32, 0.0, 0.0, 0.0]]));
            obj.rows.push((probe_ambient, vec![ground]));
            objects.push(obj);
            let oi = objects.len() - 1;
            let vl = g.smodel_vertex_lighting.get(smi).copied().flatten();
            if vl.is_some() {
                vlit += 1;
            }
            for (range, (mat, base, first, count, vstart, vcount)) in surfs.into_iter().flat_map(|(r, p)| p.into_iter().map(move |s| (r, s))) {
                let Some(mi) = mat.and_then(&mut material_of) else { continue };
                // A vertex-lit placement: its surface's light values become
                // the model mesh's second stream (each LOD's own run).
                let layer = vl.and_then(|span| {
                    let bytes = span.get(image);
                    let (a, b) = (20 * vstart as usize, 20 * (vstart + vcount) as usize);
                    match bytes.get(a..b) {
                        Some(run) => {
                            let at = model_layer.len() as u32;
                            model_layer.extend_from_slice(run);
                            Some(at)
                        }
                        None => {
                            vlit_short += 1;
                            None
                        }
                    }
                });
                draws.push(Draw {
                    mesh: 1,
                    object: Some(oi as u32),
                    material: mi,
                    lightmap: 0,
                    probe,
                    first_index: first,
                    index_count: count,
                    base_vertex: base,
                    layer,
                    sun: env_sun(env),
                    light: light_of(env),
                    list: List::Model,
                    range,
                    cell: Some(crate::cells::CellItem::Smodel(smi as u32)),
                    casts: placement_casts(sm.flags, d.map_or(0, |d| u8_at(image, d + L::GfxStaticModelDrawInst::sunShadowFlags)), xm.flags, sun_opt),
                });
            }
        }
        if lods_on() {
            notes.push(format!(
                "iw6 lods: {} placements with LOD bands ({} surfaces of coarser LODs), {} with a cull distance",
                lod_counts.0, lod_counts.1, lod_counts.2
            ));
        }
        let cells = crate::cells::Cells::read(image, g);
        if let Some(c) = &cells {
            notes.push(format!(
                "iw6 cells: {} cells, {} portals, {} world surfaces and {} placements in their trees",
                c.cell_count(),
                c.portals.iter().map(Vec::len).sum::<usize>(),
                c.surface_cells.len(),
                c.smodel_cells.len()
            ));
        }
        // The props that are not static models (the clip map's dynamic
        // entities, the scriptables, the script models) and the brush
        // models entities place, each drawn by its own technique; the
        // ones a script may move are kept as movers.
        let logic = {
            let src = fastfile_iw6::map::raw_file(&map_l.cap, image, &format!("maps/createfx/{name}_fx.gsc"))
                .map(|b| String::from_utf8_lossy(&b).into_owned());
            fastfile_iw6::maplogic::MapLogic::read(&map.entities, src.as_deref())
        };
        let mut movers: Vec<Mover> = Vec::new();
        let mut extra_counts: Vec<(&'static str, usize)> = Vec::new();
        let mut count = |k: &'static str| match extra_counts.iter_mut().find(|(n, _)| *n == k) {
            Some(c) => c.1 += 1,
            None => extra_counts.push((k, 1)),
        };
        // The nearest static placement's probe and light environment (the
        // engine lights these at run time; INFERRED as the H1 dynamic
        // entities': nearest within 256 units, else probe 0 in the sun).
        let near_static = |o: [f32; 3]| -> (u8, usize) {
            let best = world
                .static_models
                .iter()
                .enumerate()
                .map(|(i, m)| ((0..3).map(|k| (m.origin[k] - o[k]).powi(2)).sum::<f32>(), i))
                .filter(|(d2, _)| *d2 <= 256.0 * 256.0)
                .min_by(|a, b| a.0.total_cmp(&b.0));
            match (best, g.smodels) {
                (Some((_, i)), Some(at)) => {
                    let d = at + sd * i;
                    (
                        u8_at(image, d + L::GfxStaticModelDrawInst::reflectionProbeIndex),
                        usize::from(u8_at(image, d + L::GfxStaticModelDrawInst::primaryLightEnvIndex)),
                    )
                }
                _ => (0, 1),
            }
        };
        let cut = fastfile_iw6::map::alternate_level_cut(&map.spawns);
        let (extras, skipped_props) = fastfile_iw6::map::extra_props_report(&map, &map_l.cap, cut);
        // Models by name in the other zones (the shared zones hold models the map zone only
        // names: a stub record in the map zone, or no record at all): first one with geometry.
        let mut other_models: HashMap<String, (usize, usize)> = HashMap::new();
        let mut other_model_count = (0usize, 0usize);
        let mut items: Vec<(&'static str, Option<u32>, String, [f32; 3], [[f32; 3]; 3])> = Vec::new();
        for p in &extras {
            let name = map_l.cap.xmodels.get(p.model as usize).map_or(String::new(), |x| key(&x.name));
            items.push((p.source, Some(p.model), name, p.origin, p.axis));
        }
        for sp in &skipped_props {
            if let Some((o, a)) = sp.place {
                items.push(("script_model", None, key(&sp.model), o, a));
            }
        }
        if items.iter().any(|i| i.1.is_none() || !fastfile_iw6::map::model_has_geometry(&map_l.cap, i.1.unwrap_or(0))) {
            for (zi, l) in loaded.iter().enumerate().skip(1) {
                for (mi, xm) in l.cap.xmodels.iter().enumerate() {
                    if !xm.name.starts_with(',') && fastfile_iw6::map::model_has_geometry(&l.cap, mi as u32) {
                        other_models.entry(key(&xm.name)).or_insert((zi, mi));
                    }
                }
            }
        }
        let model_pieces: Vec<(String, [f32; 3])> = logic
            .doors
            .iter()
            .flat_map(|d| d.pieces.iter())
            .filter_map(|p| Some((p.model.clone()?, p.start)))
            .collect();
        let mut skinned_models: HashMap<u32, bool> = HashMap::new();
        let mut skin_parts: HashMap<u32, Vec<(u16, crate::skin::BoneDraws)>> = HashMap::new();
        for (p_source, p_model, p_name, p_origin, p_axis) in items.iter().cloned() {
            struct P {
                source: &'static str,
                origin: [f32; 3],
                axis: [[f32; 3]; 3],
            }
            let p = P { source: p_source, origin: p_origin, axis: p_axis };
            let on = if p.source == "dynent" { dynents_on() } else { script_props_on() };
            if !on {
                continue;
            }
            // A script model that plays an animation and follows more than its first bone
            // (the crane, the gate's chain): drawn in parts, one object per bone.
            if p.source == "script_model" && script_movers_on() && skin_on() {
                if let Some(own_m) = p_model.filter(|&m| fastfile_iw6::map::model_has_geometry(&map_l.cap, m)) {
                    let Some(xm) = map_l.cap.xmodels.get(own_m as usize) else { continue };
                let skinned = *skinned_models.entry(own_m).or_insert_with(|| {
                    xm.lods
                        .first()
                        .and_then(|l| l.surfs)
                        .and_then(|s| map_l.cap.model_surfs.get(s.index as usize))
                        .is_some_and(|ms| crate::skin::is_skinned(&ms.surfs, image, usize::from(xm.num_bones)))
                });
                if skinned {
                    let parts = skin_parts
                        .entry(own_m)
                        .or_insert_with(|| crate::skin::model_bone_draws(&map_l.cap, image, xm, &mut model_verts, &mut model_idx))
                        .clone();
                    let table = grid.as_ref().and_then(|gr| gr.sample(p.origin)).map(|sh| {
                        let mut t = [[0f32; 3]; 56];
                        for (k, d) in dirs.iter().enumerate() {
                            t[k] = fastfile_iw6::lightgrid::evaluate(&sh, *d);
                        }
                        t
                    });
                    tiles.push((table.unwrap_or([[0.3; 3]; 56]), 1.0));
                    let tile = tiles.len() - 1;
                    let (rows, world_m) = placement(p.origin, p.axis, 1.0);
                    let (probe, env) = near_static(p.origin);
                    let mut part_objects: Vec<(u32, u16)> = Vec::new();
                    for (bone, bone_draws) in parts {
                        objects.push(Object {
                            rows: vec![
                                (world_matrix, rows.clone()),
                                (base_coords, vec![[tile as f32, 0.0, 0.0, 0.0]]),
                                (probe_ambient, vec![[0.0; 4]]),
                            ],
                            world: world_m,
                            tile: Some(tile),
                        });
                        let oi = (objects.len() - 1) as u32;
                        part_objects.push((oi, bone));
                        for (mat, base, first, cnt) in bone_draws {
                            let Some(mi) = mat.and_then(&mut material_of) else { continue };
                            draws.push(Draw {
                                mesh: 1,
                                object: Some(oi),
                                material: mi,
                                lightmap: 0,
                                probe,
                                first_index: first,
                                index_count: cnt,
                                base_vertex: base,
                                layer: None,
                                sun: env_sun(env),
                                light: light_of(env),
                                list: List::Model,
                                range: None,
                                cell: None,
                                casts: [true; 2],
                            });
                        }
                    }
                    if !part_objects.is_empty() {
                        count("script_model (skinned, in parts)");
                        movers.push(Mover {
                            key: MoverKey::Model { name: xm.name.trim_start_matches(',').to_owned(), origin: p.origin },
                            objects: part_objects.iter().map(|x| x.0).collect(),
                            bones: part_objects,
                        });
                        continue;
                    }
                }
            }
                }
            // The map zone's own record when it has geometry, else the first other zone's.
            let own = p_model.filter(|&m| fastfile_iw6::map::model_has_geometry(&map_l.cap, m));
            let (zi, xm, surfs) = if let Some(m) = own {
                let Some(xm) = map_l.cap.xmodels.get(m as usize) else { continue };
                let surfs = placed
                    .entry((m, 0))
                    .or_insert_with(|| model_surfaces(&map_l.cap, image, xm, 0, &mut model_verts, &mut model_idx))
                    .clone();
                (0usize, xm, surfs)
            } else if let Some(&(zi, mi)) = other_models.get(&p_name) {
                let l = &loaded[zi];
                let xm = &l.cap.xmodels[mi];
                let surfs = placed_other
                    .entry((zi, mi))
                    .or_insert_with(|| model_surfaces(&l.cap, &l.zone.image, xm, 0, &mut model_verts, &mut model_idx))
                    .clone();
                other_model_count.0 += 1;
                (zi, xm, surfs)
            } else {
                other_model_count.1 += 1;
                count("left out (model in no zone read)");
                continue;
            };
            if surfs.is_empty() {
                count("left out (model has no surfaces)");
                continue;
            }
            cur_zone.set(zi);
            count(p.source);
            let table = grid.as_ref().and_then(|gr| gr.sample(p.origin)).map(|sh| {
                let mut t = [[0f32; 3]; 56];
                for (k, d) in dirs.iter().enumerate() {
                    t[k] = fastfile_iw6::lightgrid::evaluate(&sh, *d);
                }
                t
            });
            tiles.push((table.unwrap_or([[0.3; 3]; 56]), 1.0));
            let tile = tiles.len() - 1;
            let (rows, world_m) = placement(p.origin, p.axis, 1.0);
            objects.push(Object {
                rows: vec![
                    (world_matrix, rows),
                    (base_coords, vec![[tile as f32, 0.0, 0.0, 0.0]]),
                    (probe_ambient, vec![[0.0; 4]]),
                ],
                world: world_m,
                tile: Some(tile),
            });
            let oi = (objects.len() - 1) as u32;
            let name = xm.name.trim_start_matches(',').to_owned();
            // Every script model is a mover when the map's own scripts run (they hide,
            // move and delete them); else only the door pieces.
            if p.source == "script_model"
                && (script_movers_on()
                    || model_pieces
                        .iter()
                        .any(|(n, o)| n.eq_ignore_ascii_case(&name) && (0..3).all(|k| (o[k] - p.origin[k]).abs() < 1.0)))
            {
                movers.push(Mover {
                    key: MoverKey::Model { name, origin: p.origin },
                    objects: vec![oi],
                    bones: Vec::new(),
                });
            }
            let (probe, env) = near_static(p.origin);
            for (mat, base, first, cnt, _, _) in surfs {
                let Some(mi) = mat.and_then(&mut material_of) else { continue };
                draws.push(Draw {
                    mesh: 1,
                    object: Some(oi),
                    material: mi,
                    lightmap: 0,
                    probe,
                    first_index: first,
                    index_count: cnt,
                    base_vertex: base,
                    layer: None,
                    sun: env_sun(env),
                    light: light_of(env),
                    list: List::Model,
                    range: None,
                    cell: None,
                    casts: [true; 2],
                });
            }
            cur_zone.set(0);
        }
        // Models the scripts may spawn or swap in (a destructible's broken state, a prop a
        // script makes): hidden instances, shown where the scripts put them. The draw lists
        // are fixed once the scene is built, so the instances are made here.
        {
            let wanted: Vec<String> = POOL_MODELS.read().map(|p| p.clone()).unwrap_or_default();
            let slots = pool_slots();
            let mut made = 0usize;
            if slots > 0 && script_movers_on() {
                // (The map zone's records first, then the other zones': a model only a shared
                // zone holds is drawn from there.)
                let mut pooled: std::collections::HashSet<String> = std::collections::HashSet::new();
                let mut pool_src: Vec<(usize, usize)> = Vec::new();
                for (zi, l) in loaded.iter().enumerate() {
                    for (mi, xm) in l.cap.xmodels.iter().enumerate() {
                        if !xm.name.starts_with(',') || zi == 0 {
                            pool_src.push((zi, mi));
                        }
                    }
                }
                for (zi, mi) in pool_src {
                    let l = &loaded[zi];
                    let xm = &l.cap.xmodels[mi];
                    let name = xm.name.trim_start_matches(',').to_owned();
                    if !wanted.iter().any(|n| n.eq_ignore_ascii_case(&name)) || pooled.contains(&name.to_ascii_lowercase()) {
                        continue;
                    }
                    let surfs = if zi == 0 {
                        placed
                            .entry((mi as u32, 0))
                            .or_insert_with(|| model_surfaces(&l.cap, &l.zone.image, xm, 0, &mut model_verts, &mut model_idx))
                            .clone()
                    } else {
                        placed_other
                            .entry((zi, mi))
                            .or_insert_with(|| model_surfaces(&l.cap, &l.zone.image, xm, 0, &mut model_verts, &mut model_idx))
                            .clone()
                    };
                    if surfs.is_empty() {
                        continue;
                    }
                    pooled.insert(name.to_ascii_lowercase());
                    cur_zone.set(zi);
                    for slot in 0..slots {
                        tiles.push(([[0.3; 3]; 56], 1.0));
                        let tile = tiles.len() - 1;
                        let (rows, world_m) = placement([0.0; 3], [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]], 0.0);
                        objects.push(Object {
                            rows: vec![
                                (world_matrix, rows),
                                (base_coords, vec![[tile as f32, 0.0, 0.0, 0.0]]),
                                (probe_ambient, vec![[0.0; 4]]),
                            ],
                            world: world_m,
                            tile: Some(tile),
                        });
                        let oi = (objects.len() - 1) as u32;
                        movers.push(Mover { key: MoverKey::Spawned { name: name.clone(), slot }, objects: vec![oi], bones: Vec::new() });
                        for (mat, base, first, cnt, _, _) in surfs.iter().cloned() {
                            let Some(mi) = mat.and_then(&mut material_of) else { continue };
                            draws.push(Draw {
                                mesh: 1,
                                object: Some(oi),
                                material: mi,
                                lightmap: 0,
                                probe: 0,
                                first_index: first,
                                index_count: cnt,
                                base_vertex: base,
                                layer: None,
                                sun: env_sun(1),
                                light: light_of(1),
                                list: List::Model,
                                range: None,
                                cell: None,
                                casts: [true; 2],
                            });
                        }
                        made += 1;
                    }
                    cur_zone.set(0);
                }
            }
            notes.push(format!("iw6 spawned-model pool: {made} hidden instances of {} models ({slots} each, BO2MP_IW6_SPAWN_POOL)", wanted.len()));
        }
        // Brush models (doors, the hopper, a dynamic entity's brushes):
        // world surfaces outside the draw ranges, their vertices about the
        // entity's origin, drawn with the world's techniques where the
        // entity stands.
        for b in fastfile_iw6::map::placed_brush_models(&map, cut) {
            let on = if b.brush.is_some() { script_props_on() } else { dynents_on() };
            if !on {
                continue;
            }
            let (rows, world_m) = placement(b.origin, b.axis, 1.0);
            objects.push(Object {
                rows: vec![(world_matrix, rows)],
                world: world_m,
                tile: None,
            });
            let oi = (objects.len() - 1) as u32;
            let mut any = false;
            for i in b.surfaces.clone() {
                let Some(s) = world.surfaces.get(i) else { continue };
                let Some(mi) = s.material.and_then(&mut material_of) else { continue };
                any = true;
                draws.push(Draw {
                    mesh: 0,
                    object: Some(oi),
                    material: mi,
                    lightmap: s.lightmap_index,
                    probe: s.reflection_probe_index,
                    first_index: s.base_index,
                    index_count: u32::from(s.tri_count) * 3,
                    base_vertex: s.first_vertex as i32,
                    layer: None,
                    sun: env_sun(usize::from(s.primary_light_env)),
                    light: light_of(usize::from(s.primary_light_env)),
                    list: List::Opaque,
                    range: None,
                    cell: None,
                    casts: [true; 2],
                });
            }
            if !any {
                continue;
            }
            match b.brush {
                Some(n) => {
                    count("script brush models");
                    movers.push(Mover {
                        key: MoverKey::Brush(n),
                        objects: vec![oi],
                        bones: Vec::new(),
                    });
                }
                None => count("dynent brush models"),
            }
        }
        notes.push(format!(
            "iw6 props: {} (BO2MP_IW6_DYNENTS {}, BO2MP_IW6_SCRIPT_PROPS {}); props drawn from another zone's model {}, in no zone read {}; movers {}",
            extra_counts.iter().map(|(k, n)| format!("{k} {n}")).collect::<Vec<_>>().join(", "),
            if dynents_on() { "on" } else { "off" },
            if script_props_on() { "on" } else { "off" },
            other_model_count.0,
            other_model_count.1,
            movers.len()
        ));
        notes.push(format!("iw6 map logic: {}", logic.summary()));
        notes.push(format!(
            "iw6 placed models: {} ({} vertex-lit, {} surfaces past their light values), light grid tables {} (outside the grid {}), grid sun visibility 1 for {} / 0 for {} (by their own environment {})",
            world.static_models.len(),
            vlit,
            vlit_short,
            grid_hits,
            grid_misses,
            grid_sun.0,
            grid_sun.1,
            grid_sun.2
        ));
        if unresolved > 0 {
            notes.push(format!("iw6 materials named by no zone read: {unresolved}"));
        }
        // The game draws by material sort key; the order within a key is
        // the world's. The sky first: its state tests no depth (it is
        // drawn where nothing else will be).
        draws.sort_by_key(|d| (d.list != List::Sky, materials[d.material].sort_key));
        notes.push(format!(
            "iw6 draws: {} world surfaces, {} placed model surfaces",
            world_draws,
            draws.len() - world_draws
        ));

        let name_of = |id: Option<AssetId>| id.map(|id| key(map_l.cap.name_of(id)));
        let lightmaps: Vec<_> = world
            .lightmaps
            .iter()
            .map(|(p, s)| (name_of(*p), name_of(*s)))
            .collect();
        let lightmap_width = lightmaps
            .iter()
            .find_map(|l| l.1.as_ref())
            .and_then(|n| imgs_by_name.get(n).and_then(|v| v.first()))
            .map_or(0, |&(zi, ii)| u32::from(loaded[zi].cap.images[ii].width));
        let local: Vec<&PrimaryLight> = lights.iter().filter(|l| matches!(l.ty, 2 | 3)).collect();
        let falloff_from_defs = crate::local_light::falloff_read() && lightmap_width > 0 && local.iter().all(|l| l.lookup_start.is_some());
        notes.push(format!(
            "iw6 light falloff: {} ({} of {} spot/omni lights with their def, pages {} wide, ramps at {:?})",
            if falloff_from_defs { "the defs' ramps in the lightmap pages" } else { "the inferred ramp 1 - d/r" },
            local.iter().filter(|l| l.lookup_start.is_some()).count(),
            local.len(),
            lightmap_width,
            {
                let mut v: Vec<i32> = local.iter().filter_map(|l| l.lookup_start).collect();
                v.sort_unstable();
                v.dedup();
                v
            }
        ));
        let probes: Vec<Option<String>> = g.reflection_probes.iter().map(|p| name_of(p.image)).collect();
        let probe_origins = g
            .reflection_probes
            .iter()
            .map(|p| Vec3::new(p.origin[0], p.origin[1], p.origin[2]))
            .collect();
        let raw = |n: &str| {
            loaded
                .iter()
                .find_map(|l| fastfile_iw6::map::raw_file(&l.cap, &l.zone.image, n))
                .map(|b| String::from_utf8_lossy(&b).into_owned())
        };
        // The map's own files by name; when the map's name is not the file's (a
        // night or variant file), the one raw file of that kind that names the map,
        // else the only one. Never a silent default: a missing vision is a bright
        // neutral look, so it is logged with what the zones do hold.
        let names_of = |dir: &str, ext: &str| -> Vec<String> {
            let mut v: Vec<String> = loaded
                .iter()
                .flat_map(|l| l.cap.raw_files.iter().map(|r| r.name.clone()))
                .filter(|n| {
                    let n = n.to_ascii_lowercase();
                    n.starts_with(dir) && n.ends_with(ext)
                })
                .collect();
            v.sort();
            v.dedup();
            v
        };
        let pick = |dir: &str, ext: &str, exact: String, notes: &mut Vec<String>| -> Option<(String, String)> {
            if let Some(t) = raw(&exact) {
                return Some((exact, t));
            }
            let all = names_of(dir, ext);
            let short = name.strip_prefix("mp_").unwrap_or(&name).to_ascii_lowercase();
            let near: Vec<&String> = all.iter().filter(|n| n.to_ascii_lowercase().contains(&short)).collect();
            let chosen = match (near.as_slice(), all.as_slice()) {
                ([one], _) => Some((*one).clone()),
                ([], [one]) => Some(one.clone()),
                _ => None,
            };
            notes.push(format!(
                "iw6 look: `{exact}` is not in the zones; {} `{dir}*{ext}` files there: {:?}; {}",
                all.len(),
                all.iter().take(12).collect::<Vec<_>>(),
                chosen.as_ref().map_or("none taken".to_owned(), |c| format!("taking `{c}`"))
            ));
            let c = chosen?;
            raw(&c).map(|t| (c, t))
        };
        let vision = match pick("vision/", ".vision", format!("vision/{name}.vision"), &mut notes) {
            Some((file, t)) => {
                let v = Vision::parse(&t);
                notes.push(format!("iw6 vision: `{file}`"));
                v
            }
            None => {
                notes.push("iw6 vision: NONE found, neutral defaults (a night map will look like day)".into());
                Vision::default()
            }
        };
        let fog = pick("maps/createart/", "_fog.gsc", format!("maps/createart/{name}_fog.gsc"), &mut notes)
            .and_then(|(_, t)| Fog::for_map(&t, &name))
            .map(|f| {
                notes.push(format!("iw6 fog: block {}", f.name));
                FogConsts::from_fog(&f)
            })
            .unwrap_or_else(|| {
                notes.push("iw6 fog: none (no createart fog script)".into());
                FogConsts::none()
            });
        let tone_name = vision
            .s("toneMapping")
            .map(|s| s.to_ascii_lowercase())
            .unwrap_or_else(|| format!("{name}_tone"));
        let tone = loaded
            .iter()
            .find_map(|l| l.cap.tone_mappings.get(&tone_name))
            .map_or_else(Tone::published, Tone::from_asset);
        notes.push(format!("iw6 tone: {} (exposure {:?}, rates {:?})", tone.name, tone.exposure, tone.rates));
        let colorization = vision.s("colorizationSet").and_then(|n| {
            let n = n.to_ascii_lowercase();
            let set = loaded.iter().find_map(|l| l.cap.colorization_sets.get(&n))?;
            let get = |k: usize| {
                let name = set.colorizations[k].as_ref()?;
                loaded.iter().find_map(|l| l.cap.colorizations.get(name)).cloned()
            };
            Some(crate::look::Colorization {
                name: set.name.clone(),
                main: get(0)?,
                alternate: get(1),
                depth_scale_and_bias: set.alternate_depth_scale_and_bias,
                lmh: match (get(2), get(3), get(4)) {
                    (Some(lo), Some(mi), Some(hi)) => Some(crate::look::Lmh {
                        low_and_high: set.low_and_high_end_points,
                        mid: set.mid_end_points,
                        bands: [lo, mi, hi],
                    }),
                    _ => None,
                },
            })
        });
        notes.push(match &colorization {
            Some(c) => format!(
                "iw6 colorization: {} (main {:?}, alternate {:?}, depth scale and bias {:?}; low / mid / high {})",
                c.name,
                c.main.name,
                c.alternate.as_ref().map(|a| a.name.clone()),
                c.depth_scale_and_bias,
                c.lmh.as_ref().map_or_else(|| "none".to_owned(), |l| format!("{:?} end points {:?} {:?}", l.bands.iter().map(|b| (b.name.clone(), b.power_and_desaturation, b.scale, b.bias)).collect::<Vec<_>>(), l.low_and_high, l.mid))
            ),
            None => "iw6 colorization: none".into(),
        });
        let sky_i = vision.f("r_hdrSkyIntensity", 1.0);
        let tint = vision.v3("r_hdrSkyColorTint", [1.0; 3]);
        let sky_intensity = [sky_i * tint[0], sky_i * tint[1], sky_i * tint[2], 1.0];
        // The sun: its colour is linear HDR as stored; the diffuse and
        // specular scales are the engine's defaults (1 and 2.5).
        let sun = map.sun.as_ref().map_or(
            SunLight {
                dir: Vec3::Z,
                diffuse: Vec3::ONE,
                specular: Vec3::ONE * 2.5,
            },
            |s| SunLight {
                dir: Vec3::from(s.dir).normalize_or(Vec3::Z),
                diffuse: Vec3::from(s.color) * diffuse_scale(),
                specular: Vec3::from(s.color) * specular_scale(),
            },
        );
        notes.push(format!(
            "iw6 sun: dir {:?} diffuse {:?} specular {:?}; sky intensity {:?}",
            sun.dir, sun.diffuse, sun.specular, sky_intensity
        ));
        let mut world_bytes = Vec::new();
        if let Some(at) = g.vertices {
            let n = g.vertex_count as usize * 44;
            world_bytes = image.get(at..at + n).ok_or("world vertices run past the zone")?.to_vec();
        }
        let meshes = vec![
            MeshData {
                kind: VertexKind::World,
                vertex_bytes: world_bytes,
                indices: world.indices.clone(),
                layer_bytes: g.vertex_layer_data.map_or_else(Vec::new, |s| s.get(image).to_vec()),
            },
            MeshData {
                kind: VertexKind::Model,
                vertex_bytes: model_verts,
                indices: model_idx,
                layer_bytes: model_layer,
            },
        ];
        let entities = map.entity_string();
        let paks = loaded
            .iter()
            .map(|l| ImagePaks::new(l.path.parent().unwrap_or(Path::new("."))))
            .collect();
        // The sky reads straight from its pak (`streamed_cube`): the picture the
        // sky surfaces' materials sample (`sky::plan`). When its pak is not
        // beside the map (the cloud has no 3.4 GB pak 60) the sky is
        // `sky::stand_in_cube`, built from the map's fog colour and sun (the
        // old stand-in, probe 0, is a flat white), at intensity 1: the fog
        // colour is already in scene units, so the map's `r_hdrSkyIntensity`
        // (3 on mp_frag) on it would be three times too bright.
        let mut synthetic = HashMap::new();
        let sky_plan = {
            let has_pak = |zi: usize, n: u32| loaded[zi].path.parent().unwrap_or(Path::new(".")).join(format!("imagefile{n}.pak")).is_file();
            let sky_image = |id: AssetId| {
                let rec = &map_l.cap.images[id.index as usize];
                crate::sky::SkyImage::from_rec(rec, |n| has_pak(0, n), 2048)
            };
            let gfx: Vec<_> = world.sky_images.iter().flatten().map(|&id| sky_image(id)).collect();
            let mut material: Vec<crate::sky::SkyImage> = Vec::new();
            let mut sky_mats = HashSet::new();
            for &si in &world.sky_surfaces {
                let Some(m) = world.surfaces.get(si as usize).and_then(|sf| sf.material) else { continue };
                let nm = key(map_l.cap.name_of(m));
                if !sky_mats.insert(nm.clone()) {
                    continue;
                }
                let Some(&(zi, mj)) = mats_by_name.get(&nm) else { continue };
                let l = &loaded[zi];
                let mat = &l.cap.materials[mj];
                if !crate::sky::is_sky_material(&mat.name, &mat.techset.map_or(String::new(), |t| l.cap.name_of(t).to_owned())) {
                    notes.push(format!("iw6 sky: surface {si} of the sky list has material {} which is not a sky material", mat.name));
                }
                for t in &mat.textures {
                    let Some(id) = t.image else { continue };
                    let rec = &l.cap.images[id.index as usize];
                    if rec.map_type == 5 && !rec.name.starts_with('*') {
                        material.push(crate::sky::SkyImage::from_rec(rec, |n| has_pak(zi, n), 2048));
                    }
                }
            }
            crate::sky::plan(&name, &gfx, &material)
        };
        notes.extend(sky_plan.notes.iter().cloned());
        let mut sky_stand_in = false;
        if sky_plan.needs_stand_in() {
            let fog_on = fog.color[..3].iter().any(|&c| c > 0.0);
            let sun_color = map.sun.as_ref().map_or([0.0; 3], |s| s.color);
            let stand = crate::sky::StandIn {
                horizon: fog_on.then(|| [fog.color[0], fog.color[1], fog.color[2]]),
                sun_dir: sun.dir.to_array(),
                sun_color,
            };
            let px = crate::sky::stand_in_cube(&stand, 32);
            for n in &sky_plan.stand_in_for {
                synthetic.insert(key(n), px.clone());
            }
            sky_stand_in = true;
            notes.push(format!(
                "iw6 sky: stand-in for {}: horizon {:?} ({}), zenith x{}, sun halo from colour {:?} toward {:?}; intensity 1 (not the map's {sky_i})",
                sky_plan.stand_in_for.join(", "),
                stand.horizon_color(),
                if fog_on { "the map's fog colour" } else { "NO FOG BLOCK: neutral dim sky" },
                crate::sky::ZENITH_SCALE,
                sun_color,
                stand.sun_dir,
            ));
        }
        let stand_ins: HashMap<String, String> = HashMap::new();
        let sky_intensity = if sky_stand_in { [1.0, 1.0, 1.0, 1.0] } else { sky_intensity };
        let bounds = (Vec3::from(world.mins), Vec3::from(world.maxs));
        let zones = std::sync::Arc::new(loaded);
        let images = ZoneImages {
            zones: zones.clone(),
            paks,
            by_name: imgs_by_name,
            stand_ins,
            synthetic,
            max_size: 2048,
            reduced: HashSet::new(),
        };
        crate::loadtime::stage("scene assembled (materials, objects, images)");
        Ok(Self {
            name,
            zones,
            pool: std::sync::Arc::new(pool),
            materials,
            meshes,
            objects,
            draws,
            tiles,
            lightmaps,
            lightmap_width,
            falloff_from_defs,
            layer_strides,
            probes,
            probe_origins,
            lights,
            sun,
            vision,
            sun_flare,
            fog,
            tone,
            colorization,
            sky_intensity,
            bounds,
            entities,
            images,
            notes,
            movers,
            logic,
            cells,
        })
    }

    /// The map's intermission camera: (origin, angles).
    pub fn intermission(&self) -> Option<(Vec3, Vec3)> {
        for block in self.entities.split('}') {
            if !block.contains("mp_global_intermission") {
                continue;
            }
            let field = |k: &str| -> Option<Vec3> {
                let at = block.find(&format!("\"{k}\""))?;
                let rest = &block[at + k.len() + 2..];
                let q1 = rest.find('"')?;
                let q2 = rest[q1 + 1..].find('"')?;
                let v: Vec<f32> = rest[q1 + 1..q1 + 1 + q2]
                    .split_whitespace()
                    .filter_map(|s| s.parse().ok())
                    .collect();
                (v.len() == 3).then(|| Vec3::new(v[0], v[1], v[2]))
            };
            return Some((field("origin")?, field("angles").unwrap_or(Vec3::ZERO)));
        }
        None
    }
}

/// `BO2MP_IW6_GRID_SUN=0`: models see the sun everywhere past the sun
/// shadow map (the light grid's sun visibility not read).
pub fn grid_sun_on() -> bool {
    !std::env::var("BO2MP_IW6_GRID_SUN").is_ok_and(|v| v.trim() == "0")
}

/// `r_diffuseColorScale` (default 1; `BO2MP_IW6_DIFFUSE_SCALE`).
fn diffuse_scale() -> f32 {
    env_f("BO2MP_IW6_DIFFUSE_SCALE", 1.0)
}

/// `r_specularColorScale` (default 2.5; `BO2MP_IW6_SPECULAR_SCALE`).
fn specular_scale() -> f32 {
    env_f("BO2MP_IW6_SPECULAR_SCALE", 2.5)
}

pub(crate) fn env_f(name: &str, default: f32) -> f32 {
    codlegacy_env::var(name)
        .ok()
        .and_then(|v| v.trim().parse().ok())
        .unwrap_or(default)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Nothing in the scene load is one map's: every staged map resolves its
    /// own vision, fog, bounds and entities through the same code, and no
    /// two maps agree by accident (the look comes from each map's own files).
    #[test]
    fn staged_maps_load_through_the_same_code() {
        let dir = std::env::var_os("BO2MP_GHOSTS_ZONES")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("/mnt/user-data/uploads/ghosts_game_files/zone"));
        let maps: Vec<&str> = ["mp_frag", "mp_snow"].into_iter().filter(|m| dir.join(format!("{m}.ff")).is_file()).collect();
        if maps.len() < 2 {
            eprintln!("two staged maps needed: skipped");
            return;
        }
        let mut looks = Vec::new();
        for m in &maps {
            let zones = Scene::zones_for(&dir.join(format!("{m}.ff")));
            assert!(zones[0].ends_with(format!("{m}.ff")), "{m}: the map first");
            assert!(zones.iter().any(|z| z.ends_with(format!("techsets_{m}.ff"))), "{m}: its own techsets");
            let sc = Scene::load(&zones).unwrap_or_else(|e| panic!("{m}: {e}"));
            assert!(!sc.notes.iter().any(|n| n.contains("mp_frag") && *m != "mp_frag"), "{m}: a note names another map");
            assert!(sc.entities.contains("\"classname\"") || sc.entities.contains("classname"), "{m}: entities");
            let (lo, hi) = sc.bounds;
            assert!(hi.x > lo.x && hi.y > lo.y && hi.z > lo.z, "{m}: bounds");
            assert!(!sc.notes.iter().any(|n| n.contains("vision: NONE")), "{m}: vision found: {:?}", sc.notes);
            looks.push((sc.bounds.1.x - sc.bounds.0.x, sc.fog.fog));
        }
        assert_ne!(looks[0], looks[1], "two maps with one identical look");
    }

    /// The sun and spot caster rules on the flag values mp_frag holds.
    #[test]
    fn caster_flags() {
        if !caster_flags_on() {
            return;
        }
        // World surfaces: bit 0 casts; with the optimised set 0, bit 3 too.
        assert_eq!(surface_casts(9, Some(0)), [true, true]);
        assert_eq!(surface_casts(1, Some(0)), [false, true]);
        assert_eq!(surface_casts(1, None), [true, true]);
        assert_eq!(surface_casts(8, None), [false, true]);
        // Placements: 0x10 and model flag 2 never; the set's sun bit.
        assert_eq!(placement_casts(0x240, 1, 0, Some(0)), [true, true]);
        assert_eq!(placement_casts(0x240, 0, 0, Some(0)), [false, true]);
        assert_eq!(placement_casts(0x240, 0, 0, None), [true, true]);
        assert_eq!(placement_casts(0x250, 1, 0, Some(0)), [false, false]);
        assert_eq!(placement_casts(0x240, 1, 2, Some(0)), [false, false]);
    }

    /// A streamed cube read from a pak: its top part's six faces.
    #[test]
    fn streamed_cube_reads_its_top_part_from_the_pak() {
        use std::io::Write;
        let dir = std::env::temp_dir().join(format!("iw6_cube_test_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        // BC6H (16 bytes a block): 8x8 faces = 4 blocks, 64 bytes a face.
        let face = 64usize;
        let raw: Vec<u8> = (0..6 * face).map(|i| (i / face) as u8 + 1).collect();
        let mut z = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
        z.write_all(&raw).unwrap();
        let packed = z.finish().unwrap();
        let mut file = vec![0u8; 16];
        file.extend_from_slice(&packed);
        std::fs::write(dir.join("imagefile7.pak"), &file).unwrap();
        let mut rec = fastfile_iw6::walk::ImageRec {
            format: 95,
            map_type: 5,
            streamed: true,
            ..Default::default()
        };
        rec.streams[0] = (8, 8, (6 * face) as u32, 1);
        rec.stream_files[0] = Some(fastfile_iw6::container::StreamFile {
            word0: 0,
            file_index: 7,
            flags: 0,
            offset: 16,
            offset_end: 16 + packed.len() as u64,
        });
        let here = |n: u32| dir.join(format!("imagefile{n}.pak")).is_file();
        assert!(crate::sky::SkyImage::from_rec(&rec, here, 2048).readable);
        let mut paks = ImagePaks::new(&dir);
        let px = streamed_cube(&rec, &mut paks, 1024).expect("read");
        assert_eq!((px.width, px.height, px.levels, px.dxgi_format), (8, 8, 1, 95));
        assert_eq!(px.data.len(), 6 * face);
        assert_eq!(px.data[5 * face], 6);
        let _ = std::fs::remove_dir_all(&dir);
        assert!(!crate::sky::SkyImage::from_rec(&rec, |n| dir.join(format!("imagefile{n}.pak")).is_file(), 2048).readable);
    }
}
