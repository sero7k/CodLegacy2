//! IW7's world drawn with its own techniques: every surface and placed
//! model with its material's technique (slot 8, the lit forward technique:
//! `lmap_*` on the world, `lprobe_*` on models, the unlit and sky sets'
//! own), its vertex and pixel programs translated by `dxbc_sm5`, fed the
//! zone's raw vertex streams (`vertex`), the material's pictures and
//! constant buffers, its draw state (`state`), the light maps, the light
//! grid's probes and the code constants the shaders read
//! (`docs/IW7_ENGINE_PLAN.md`, stage 2).
//!
//! What the shader maths shows (`techsets_mp_riot`):
//! - vertex b0 (code 320, per primitive) is the world matrix as three rows,
//!   the translation in w; the program subtracts the eye (code 19) from it,
//!   so b1 (code 304, per object) is the view-projection about the eye, as
//!   rows (`clip.i = dot(row i, p)`);
//! - the pixel program's sun: code 8 is the direction toward the sun (its w
//!   turns the sun on), code 9 its colour (HDR linear), the primary
//!   lightmap's red the sun's visibility;
//! - code 109 is the target size and its reciprocal (`w, h, 1/w, 1/h`);
//! - fog (codes 210 to 214) is the vision's ([`Fog`]); the volumetric fog
//!   (code textures 51 inscatter and 52 transmittance, read through codes
//!   144 and 145) is our own froxel march (`volumetric`); code 147 is the
//!   light clusters' words per tile (`lights`);
//! - models (`lprobe_*`): the vertex program reads the light grid's probes
//!   (64 bytes each: the `GfxLightGrid` probe data) from t20; code 250
//!   (per primitive) holds the model's first probe (y, a float), the probe
//!   loop's count (z, an integer, low 4 bits) and its probe count (w):
//!   the first loop step reads probe `y + w - 1` weighted by code 22's
//!   level-of-detail fade (x zero: w's fraction above 0.75, times four),
//!   the others the per-vertex probes of the lighting stream. Each model
//!   is given its nearest probe this way (`MODEL_PROBE_*`).
//!
//! Left at zero (unproven, listed in [`UNPROVEN`]): every other code
//! constant, among them code 192 (the sun visibility's softness).

use std::collections::{BTreeMap, HashMap};

use dxbc_sm5::{Container, Options, Reflection, Stage, TextureDim, TextureKind};
use fastfile_iw7::map::{Iw7Map, MapMaterial};
use fastfile_iw7::walk::{PassRec, TechniqueSetRec};
use fastfile_iw7::{AssetType, Zone};
use glam::{Mat4, Vec3};
use render_h1::gpu::{self, Gpu};
use render_h1::state::PassState;
use wgpu::util::DeviceExt;

use crate::bind::{self, Target};
use crate::vertex::{self, Base, Stream};

/// IW7's maps drawn by their own techniques, their only renderer
/// (`BO2MP_IW7_NATIVE=0`: the converted world, for comparison).
pub fn enabled() -> bool {
    !std::env::var("BO2MP_IW7_NATIVE").is_ok_and(|v| v.trim() == "0")
}

/// `BO2MP_IW7_LIGHTING_ONLY=1`: white pictures and material constants one.
pub fn lighting_only() -> bool {
    std::env::var("BO2MP_IW7_LIGHTING_ONLY").is_ok_and(|v| v == "1")
}

/// The shared technique set zones beside a map whose programs and
/// technique sets its materials may name (multiplayer or Zombies), where
/// present.
pub fn shared_technique_zones(map_path: &std::path::Path) -> Vec<std::path::PathBuf> {
    let Some(dir) = map_path.parent() else { return Vec::new() };
    let zombies = map_path.file_stem().is_some_and(|s| s.to_string_lossy().trim_start_matches("iw7_").starts_with("cp_"));
    let names: &[&str] = if zombies {
        &["techsets_global_cp", "techsets_common_cp", "techsets_common", "techsets_global"]
    } else {
        &["techsets_global_core_mp", "techsets_common_core_mp", "techsets_common_mp", "techsets_global_mp", "techsets_common", "techsets_global"]
    };
    names.iter().map(|n| dir.join(format!("{n}.ff"))).filter(|p| p.is_file()).collect()
}

/// The technique slot drawn: the lit forward technique (`lmap_*` on the
/// world, `lprobe_*` on models, the unlit and sky sets' forward pass).
pub const DRAW_SLOT: usize = 8;

/// Code constants read from the shader maths.
pub const CODE_WORLD_MATRIX: u16 = 320;
pub const CODE_VIEW_PROJECTION: u16 = 304;
pub const CODE_EYE: u16 = 19;
pub const CODE_SUN_DIRECTION: u16 = 8;
pub const CODE_SUN_COLOUR: u16 = 9;
pub const CODE_TARGET_SIZE: u16 = 109;
/// The sky's view-projection (per primitive; the sky program dots only
/// its rows' xyz with the eye-relative position, so the sky lies at the
/// far plane).
pub const CODE_SKY_VIEW_PROJECTION: u16 = 328;
/// The sky picture's rotation rows (taken as none).
pub const CODE_SKY_ROTATION: [u16; 3] = [217, 218, 219];
/// The sky picture's colour scale (taken as one).
pub const CODE_SKY_SCALE: u16 = 205;
/// The light clusters' words per tile (an integer; `lights`).
pub const CODE_CLUSTER_WORDS: u16 = 147;
/// A model's light probes (per primitive).
pub const CODE_MODEL_PROBES: u16 = 250;
/// A model's level-of-detail fade (per primitive).
pub const CODE_MODEL_FADE: u16 = 22;
/// A lightmapped placed model's light map uv scale (xy) and offset (zw):
/// the `ml_` vertex programs make the light map uv as `c.xy * unwrap +
/// c.zw` (read on `techsets_mp_fallen`).
pub const CODE_MODEL_LIGHTMAP: u16 = 117;
/// Code textures read from the shader maths.
pub const CODE_TEXTURE_FOG_INSCATTER: u16 = 51;
/// The scene behind (a copy of the colour drawn so far), which the
/// distortion surfaces (`distortion_*`) sample at a bent screen uv.
pub const CODE_TEXTURE_SCENE: u16 = 18;
/// The reflection probes' cubes (`reflect`).
pub const CODE_TEXTURE_PROBES: u16 = 37;
/// The eye, as the pixel programs add it to a position about the eye.
pub const CODE_PIXEL_EYE: u16 = 17;
/// The probes' blend by gloss and their luminance match's clamp (`reflect`).
pub const CODE_PROBE_BLEND: u16 = 141;
pub const CODE_PROBE_CLAMP: u16 = 151;
/// The time in seconds (all four lanes; the distortion programs turn their
/// pictures by the w lane).
pub const CODE_TIME: u16 = 14;
/// Clip position to screen uv times w: `clip * c265 + clip.w * c266`.
pub const CODE_CLIP_TO_UV_SCALE: u16 = 265;
pub const CODE_CLIP_TO_UV_OFFSET: u16 = 266;
/// The scene copy's uv clamp (min xy, max zw).
pub const CODE_SCENE_CLAMP: u16 = 160;
/// How many frames a profile line averages.
pub const PROFILE_EVERY: u32 = 120;

/// The specular environment's lookup (`brdf`): without it (white) the lit
/// programs take every surface as a mirror and leave out its diffuse light.
pub const CODE_TEXTURE_ENV_BRDF: u16 = 64;
/// The specular occlusion lookup (`spec_occlusion`).
pub const CODE_TEXTURE_SPEC_OCCLUSION: u16 = 65;

/// The spot lights' shadow maps (`spot`).
pub const CODE_TEXTURE_SPOT_SHADOWS: u16 = 15;
/// The volumetric fog's transmittance (`volumetric`).
pub const CODE_TEXTURE_FOG_TRANSMIT: u16 = 52;
/// The volumetric fog's froxel depth segments' lengths (`volumetric`).
pub const CODE_FROXEL_SEGMENTS: u16 = 144;
/// Their reciprocals.
pub const CODE_FROXEL_SCALE: u16 = 145;
/// The sun's shadow at the pixel (screen uv; `screen`).
pub const CODE_TEXTURE_SUN_SHADOW: u16 = 10;
/// Ambient occlusion at the pixel (screen uv; `screen`).
pub const CODE_TEXTURE_AO: u16 = 43;
/// Engine-bound texture slots (no argument feeds them): the light map's
/// primary, secondary and second-unorm pictures.
pub const LIGHTMAP_SLOTS: [u32; 3] = [2, 3, 4];
/// The light grid's probes (vertex program, raw buffer).
pub const PROBE_SLOT: u32 = 20;

/// Code constants this frame leaves at zero because nothing proves their
/// value yet (sun softness, fog, volumetric fog, clusters).
pub const UNPROVEN: &str = "every code constant but 8, 9, 19, 22, 109, 210-214 (fog, from the vision), 250, 304, 320 and 328 (192: sun visibility softness; 144, 145, 147: volumetric fog; the rest); 205 (sky colour scale) is taken as one and 217-219 (sky rotation) as none";

/// The camera: the eye in world units and the view-projection about it.
#[derive(Clone, Copy, Debug)]
pub struct Camera {
    pub eye: Vec3,
    /// The view's forward axis (world).
    pub forward: Vec3,
    /// Clip from eye-relative world position.
    pub view_projection: Mat4,
    pub width: u32,
    pub height: u32,
}

impl Camera {
    /// The game's view: `view_from_world` (world units, any translation)
    /// and its clip matrix (reversed depth); the rotation alone is kept,
    /// since the vertex programs place vertices about the eye.
    pub fn from_view(eye: Vec3, view_from_world: Mat4, clip_from_view: Mat4, width: u32, height: u32) -> Self {
        let mut rotation = view_from_world;
        rotation.w_axis = glam::Vec4::W;
        // (Rows of the view: right, up, back.)
        let forward = -Vec3::new(view_from_world.x_axis.z, view_from_world.y_axis.z, view_from_world.z_axis.z);
        Self { eye, forward, view_projection: clip_from_view * rotation, width, height }
    }

    /// Looking along `forward` (Z up), `fov_y` in degrees.
    pub fn looking(eye: Vec3, forward: Vec3, fov_y: f32, width: u32, height: u32) -> Self {
        let view = Mat4::look_to_rh(Vec3::ZERO, forward.normalize(), Vec3::Z);
        // Reversed infinite depth (1 near, 0 far), as the game's frame.
        let proj = Mat4::perspective_infinite_reverse_rh(fov_y.to_radians(), width as f32 / height as f32, 4.0);
        Self { eye, forward: forward.normalize(), view_projection: proj * view, width, height }
    }
}

/// The sun the pixel programs light with.
#[derive(Clone, Copy, Debug, Default)]
pub struct Sun {
    pub toward: [f32; 3],
    pub colour: [f32; 3],
}

impl Sun {
    /// The map's primary light of type 1 (direction toward the sun,
    /// HDR colour), if it has one.
    pub fn of(map: &Iw7Map) -> Option<Self> {
        let l = map.lights.iter().find(|l| l.kind == 1 && l.color.iter().any(|&c| c > 0.0))?;
        let d = Vec3::from(l.dir).normalize_or_zero();
        Some(Self { toward: d.into(), colour: l.color })
    }
}

/// The map's fog, from its vision file (`fogStartDist`, `fogDensity` as
/// the half distance, `fogMaxOpacity`, `fogColor` / `fogColorIntensity`,
/// the sun fog's colour, direction and fade angles), as the lit pixel
/// programs' fog maths takes it:
/// - transmittance `max(exp(d * c212.w + c210.w), c210.y)`, d the eye
///   distance: so `c212.w = -ln2 / half`, `c210.w = ln2 / half * start`,
///   `c210.y = 1 - max opacity` (the sun's density `c212.x` the same);
/// - the colour `mix(c211, c213, saturate((dot(c214, view) - c212.y) *
///   c212.z))`: `c214` the sun fog's direction, `c212.y` the cosine of
///   its end fade angle, `c212.z` one over (cos begin - cos end).
///
/// The colours are linear times their intensity, in the HDR units the
/// sun's colour (code 9) has. `BO2MP_IW7_FOG=0`: no fog.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Fog {
    pub start: f32,
    pub half: f32,
    pub opacity: f32,
    pub colour: [f32; 3],
    pub sun_colour: [f32; 3],
    pub sun_dir: [f32; 3],
    pub sun_begin_deg: f32,
    pub sun_end_deg: f32,
}

impl Fog {
    /// The fog a vision file's text sets, if any.
    pub fn from_vision(text: &str) -> Option<Self> {
        let mut vals: HashMap<String, Vec<f32>> = HashMap::new();
        for line in text.lines() {
            let Some((key, value)) = line.trim().split_once(' ') else { continue };
            let v: Vec<f32> = value.trim().trim_matches('"').split_whitespace().filter_map(|x| x.parse().ok()).collect();
            if !v.is_empty() {
                vals.entry(key.to_ascii_lowercase()).or_insert(v);
            }
        }
        let one = |k: &str| vals.get(k).and_then(|v| v.first().copied());
        let three = |k: &str| vals.get(k).filter(|v| v.len() >= 3).map(|v| [v[0], v[1], v[2]]);
        let half = one("fogdensity")?;
        let opacity = one("fogmaxopacity").unwrap_or(1.0).clamp(0.0, 1.0);
        if half <= 0.0 || opacity <= 0.0 {
            return None;
        }
        let lin = |c: f32| if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) };
        let hdr = |c: [f32; 3], i: f32| c.map(|v| lin(v.clamp(0.0, 1.0)) * i.max(0.0));
        let colour = hdr(three("fogcolor")?, one("fogcolorintensity").unwrap_or(1.0));
        let sun_i = one("fogsuncolorintensity").unwrap_or(0.0);
        let (sun_colour, sun_dir) = match (three("fogsuncolor"), three("fogsundir")) {
            (Some(c), Some(d)) if sun_i > 0.0 => (hdr(c, sun_i), Vec3::from(d).normalize_or_zero().into()),
            _ => (colour, [0.0, 0.0, 1.0]),
        };
        Some(Self {
            start: one("fogstartdist").unwrap_or(0.0).max(0.0),
            half,
            opacity,
            colour,
            sun_colour,
            sun_dir,
            sun_begin_deg: one("fogsunbeginfadeangle").unwrap_or(0.0),
            sun_end_deg: one("fogsunendfadeangle").unwrap_or(90.0),
        })
    }

    /// The map's fog (none with `BO2MP_IW7_FOG=0`).
    pub fn of(map: &Iw7Map) -> Option<Self> {
        if std::env::var("BO2MP_IW7_FOG").is_ok_and(|v| v == "0") {
            return None;
        }
        map.vision.as_ref().and_then(|(_, text)| Self::from_vision(text))
    }

    fn code(&self, index: u16) -> Option<[f32; 4]> {
        let density = std::f32::consts::LN_2 / self.half;
        let (cb, ce) = (self.sun_begin_deg.to_radians().cos(), self.sun_end_deg.to_radians().cos());
        let span = if (cb - ce).abs() > 1e-4 { 1.0 / (cb - ce) } else { 0.0 };
        Some(match index {
            210 => [0.0, 1.0 - self.opacity, 0.0, density * self.start],
            212 => [-density, ce, span, -density],
            214 => [self.sun_dir[0], self.sun_dir[1], self.sun_dir[2], 0.0],
            211 => [self.colour[0], self.colour[1], self.colour[2], 0.0],
            213 => [self.sun_colour[0], self.sun_colour[1], self.sun_colour[2], 0.0],
            _ => return None,
        })
    }
}

/// What a draw did and did not do.
#[derive(Debug, Default)]
pub struct Report {
    pub surfaces: usize,
    pub drawn: usize,
    pub models_placed: usize,
    pub model_draws: usize,
    pub pipelines: usize,
    pub pictures: usize,
    pub pictures_missing: usize,
    /// Reason -> surfaces.
    pub skipped: BTreeMap<String, usize>,
    /// Code constant index -> passes that read it at zero.
    pub zero_constants: BTreeMap<u16, usize>,
    /// Where the build's time went (seconds): technique set walks,
    /// pipelines, pictures, bind groups, everything.
    pub seconds: BuildTimes,
}

/// The host build's time by part (seconds).
#[derive(Clone, Copy, Debug, Default)]
pub struct BuildTimes {
    pub walks: f32,
    pub pipelines: f32,
    pub pictures: f32,
    pub groups: f32,
    pub total: f32,
}

impl std::fmt::Display for BuildTimes {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "{:.1}s (technique set walks {:.1}s, pipelines {:.1}s, pictures {:.1}s, other bind group work {:.1}s)", self.total, self.walks, self.pipelines, self.pictures, (self.groups - self.pictures).max(0.0))
    }
}

/// Whether a set has any pass (some zones hold a set's name with none).
pub fn has_passes(set: &TechniqueSetRec) -> bool {
    set.passes.iter().any(|p| !p.is_empty())
}

/// The drawn technique of a set: slot 8's first pass. A tessellated one
/// (`*_worldtess_*`: a hull and domain program follow its vertex program,
/// which then writes no position) gives way to the set's untessellated
/// twin (`*_world_*`, the same technique otherwise).
pub fn draw_pass(set: &TechniqueSetRec) -> Option<(&str, &PassRec, usize)> {
    let mut techniques = set.techniques.iter().zip(&set.passes).filter(|(_, p)| !p.is_empty());
    let Some((t, p)) = techniques.clone().find(|(t, _)| t.0 == DRAW_SLOT) else {
        return effect_pass(set);
    };
    if t.1.contains("worldtess") {
        // (Names are cut at a fixed length before their hash: compare the
        // shorter of the two.)
        let stem = |n: &str| n.split('.').next().unwrap_or("").to_owned();
        let twin = stem(&t.1).replace("worldtess", "world");
        if let Some((t2, p2)) = techniques.find(|(t2, _)| {
            let s2 = stem(&t2.1);
            let n = twin.len().min(s2.len());
            !s2.contains("worldtess") && s2.len() > 8 && s2[..n] == twin[..n]
        }) {
            return Some((t2.1.as_str(), &p2[0], DRAW_SLOT));
        }
    }
    Some((t.1.as_str(), &p[0], DRAW_SLOT))
}

/// The drawn technique of an effect-style set (the unlit glows, screens
/// and holograms on placed models: `mo_`, `mco_`, `mopw_`, ...), which
/// fills only the first few slots (no slot 8): its first technique that
/// draws colour (not a depth-only, shadow, thermal or untextured debug
/// one). Off until checked on a GPU (the owner's maps went black with them
/// on, most likely a non-finite value from one of them):
/// `BO2MP_IW7_EFFECT_SETS=1` draws them.
fn effect_pass(set: &TechniqueSetRec) -> Option<(&str, &PassRec, usize)> {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    if !*ON.get_or_init(|| std::env::var("BO2MP_IW7_EFFECT_SETS").is_ok_and(|v| v.trim() == "1")) {
        return None;
    }
    let last = set.techniques.iter().map(|t| t.0).max()?;
    // (Shadow casters and tool surfaces fill the same few slots: never.)
    let name = set.name.as_str();
    if last >= DRAW_SLOT || name.starts_with("w_") || name.starts_with(",w_") || name.contains("shadowcaster") || name.contains("tools") {
        return None;
    }
    set.techniques.iter().zip(&set.passes).filter(|(t, p)| !p.is_empty() && t.0 <= 1).find_map(|(t, p)| {
        let n = t.1.as_str();
        let skip = ["position_only", "transform_only", "shadow", "thermal", "untextured", "_edbg", "tools"].iter().any(|k| n.contains(k));
        (!skip).then(|| (n, &p[0], t.0))
    })
}

/// What one draw object is: the world (identity) or a placed model.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum Object {
    World,
    Model(usize),
}

/// The per-primitive rows an object gives: its world matrix, its probes
/// and its fade.
#[derive(Clone, Copy, Debug)]
struct ObjectRows {
    world: [[f32; 4]; 4],
    probes: [f32; 4],
    fade: [f32; 4],
    /// A lightmapped placed model's light map uv scale (xy) and offset
    /// (zw) (code constant 117).
    lmap: [f32; 4],
}

impl ObjectRows {
    fn world() -> Self {
        let m = Mat4::IDENTITY;
        Self { world: std::array::from_fn(|i| m.row(i).to_array()), probes: [0.0; 4], fade: [0.0; 4], lmap: [0.0; 4] }
    }

    /// A placed model: rows `(axis[k][i] * scale, origin[i])` (forward,
    /// left, up in the world), lit by probe `probe` alone.
    fn model(origin: [f32; 3], axis: [[f32; 3]; 3], scale: f32, probe: Option<u32>) -> Self {
        let mut world = [[0.0; 4]; 4];
        for (i, row) in world.iter_mut().enumerate().take(3) {
            *row = [axis[0][i] * scale, axis[1][i] * scale, axis[2][i] * scale, origin[i]];
        }
        world[3] = [0.0, 0.0, 0.0, 1.0];
        let (probes, fade) = match probe {
            // y: the first probe; z: one loop step (an integer); w: one
            // probe, so the step reads probe y. The fade's w fraction past
            // 0.75 gives it full weight.
            Some(p) => ([0.0, p as f32, f32::from_bits(1), 1.0], [0.0, 0.0, 0.0, 0.9999]),
            None => ([0.0; 4], [0.0; 4]),
        };
        Self { world, probes, fade, lmap: [0.0; 4] }
    }

    /// With its light map place (`GfxStaticModelLightmapInfo`: offset,
    /// scale).
    fn with_lightmap(mut self, offset: [f32; 2], scale: [f32; 2]) -> Self {
        self.lmap = [scale[0], scale[1], offset[0], offset[1]];
        self
    }

    fn code(&self, index: u16) -> Option<Vec<[f32; 4]>> {
        Some(match index {
            CODE_WORLD_MATRIX => self.world.to_vec(),
            CODE_MODEL_PROBES => vec![self.probes],
            CODE_MODEL_FADE => vec![self.fade],
            CODE_MODEL_LIGHTMAP => vec![self.lmap],
            _ => return None,
        })
    }
}

/// Up to four rows of a code constant (no allocation).
#[derive(Clone, Copy, Debug, Default)]
pub struct Rows {
    pub r: [[f32; 4]; 4],
    pub n: usize,
}

impl Rows {
    fn one(r: [f32; 4]) -> Self {
        Self { r: [r, [0.0; 4], [0.0; 4], [0.0; 4]], n: 1 }
    }

    fn of(m: Mat4) -> Self {
        Self { r: std::array::from_fn(|i| m.row(i).to_array()), n: 4 }
    }

    pub fn get(&self, i: usize) -> [f32; 4] {
        if i < self.n { self.r[i] } else { [0.0; 4] }
    }
}

impl std::ops::Index<usize> for Rows {
    type Output = [f32; 4];
    fn index(&self, i: usize) -> &[f32; 4] {
        &self.r[i]
    }
}

/// The code constant `index`'s rows that the camera and sun give.
fn code_rows(index: u16, cam: &Camera, sun: Option<Sun>, fog: Option<Fog>, words: u32, sky: f32) -> Option<Rows> {
    if index == CODE_CLUSTER_WORDS {
        return Some(Rows::one([f32::from_bits(words), 0.0, 0.0, 0.0]));
    }
    match index {
        CODE_CLIP_TO_UV_SCALE => return Some(Rows::one([0.5, -0.5, 1.0, 1.0])),
        CODE_CLIP_TO_UV_OFFSET => return Some(Rows::one([0.5, 0.5, 0.0, 0.0])),
        CODE_SCENE_CLAMP => {
            let (hx, hy) = (0.5 / cam.width.max(1) as f32, 0.5 / cam.height.max(1) as f32);
            return Some(Rows::one([hx, hy, 1.0 - hx, 1.0 - hy]));
        }
        _ => {}
    }
    if index == CODE_FROXEL_SEGMENTS || index == CODE_FROXEL_SCALE {
        let (lengths, scales) = crate::volumetric::mapping_rows();
        return Some(Rows::one(if index == CODE_FROXEL_SEGMENTS { lengths } else { scales }));
    }
    if (210..=214).contains(&index) {
        return fog.and_then(|f| f.code(index)).map(Rows::one);
    }
    Some(match index {
        CODE_VIEW_PROJECTION | CODE_SKY_VIEW_PROJECTION => Rows::of(cam.view_projection),
        217 => Rows::one([1.0, 0.0, 0.0, 0.0]),
        218 => Rows::one([0.0, 1.0, 0.0, 0.0]),
        219 => Rows::one([0.0, 0.0, 1.0, 0.0]),
        CODE_SKY_SCALE => Rows::one([sky; 4]),
        CODE_EYE | CODE_PIXEL_EYE => Rows::one([cam.eye.x, cam.eye.y, cam.eye.z, 1.0]),
        CODE_SUN_DIRECTION => {
            let s = sun?;
            Rows::one([s.toward[0], s.toward[1], s.toward[2], 1.0])
        }
        CODE_SUN_COLOUR => {
            let s = sun?;
            Rows::one([s.colour[0], s.colour[1], s.colour[2], 0.0])
        }
        CODE_TARGET_SIZE => {
            let (w, h) = (cam.width as f32, cam.height as f32);
            Rows::one([w, h, 1.0 / w, 1.0 / h])
        }
        _ => return None,
    })
}

/// Whether the frame or the object gives code constant `index`.
fn known(index: u16) -> bool {
    matches!(index, CODE_WORLD_MATRIX | CODE_VIEW_PROJECTION | CODE_EYE | CODE_SUN_DIRECTION | CODE_SUN_COLOUR | CODE_TARGET_SIZE | CODE_MODEL_PROBES | CODE_MODEL_FADE | CODE_SKY_VIEW_PROJECTION | CODE_SKY_SCALE | 217..=219 | 210..=214 | CODE_CLUSTER_WORDS | CODE_FROXEL_SEGMENTS | CODE_FROXEL_SCALE | CODE_TIME | CODE_PIXEL_EYE | CODE_PROBE_BLEND | CODE_PROBE_CLAMP | CODE_CLIP_TO_UV_SCALE | CODE_CLIP_TO_UV_OFFSET | CODE_SCENE_CLAMP)
}

/// Debugging: `IW7_DBG_WHITE=<hex slot hashes>`: those material pictures read as the stand-in.
fn dbg_white(h: u32) -> bool {
    std::env::var("IW7_DBG_WHITE").is_ok_and(|v| v.split(',').any(|x| u32::from_str_radix(x.trim(), 16) == Ok(h)))
}

struct Program<'z> {
    container: Container<'z>,
    program: dxbc_sm5::Program,
}

/// Where programs are read: the map's technique set zone, then by name the
/// other zones given (the global technique sets, whose programs the map's
/// passes may name).
/// Program scans by (stage, name): the bytes found, or none.
type Scanned<'z> = HashMap<(AssetType, String), Option<&'z [u8]>>;

struct Programs<'z> {
    /// The map's technique set zone first, then the others.
    zones: Vec<(&'z Zone, &'z fastfile_iw7::walk::Capture)>,
    by_name: HashMap<(AssetType, String), &'z [u8]>,
    /// What [`Programs::scan`] found for each name, misses too: a scan
    /// reads every zone, and the same program is asked for again by every
    /// pipeline and state that uses it (uncached, the owner's mp_geneva
    /// took more than four minutes and drew nothing).
    scanned: std::sync::Mutex<Scanned<'z>>,
}

impl<'z> Programs<'z> {
    fn new(zones: Vec<(&'z Zone, &'z fastfile_iw7::walk::Capture)>) -> Self {
        let mut by_name = HashMap::new();
        for (z, c) in &zones {
            for (&(ty, index), sp) in &c.shader_programs {
                let name = c.name_of(fastfile_iw7::walk::AssetId { ty, index });
                by_name.entry((ty, name.trim_start_matches(',').to_owned())).or_insert(&z.image[sp.pos..sp.pos + sp.len]);
            }
        }
        Self { zones, by_name, scanned: Default::default() }
    }

    fn cap(&self, zone: usize) -> &'z fastfile_iw7::walk::Capture {
        self.zones[zone].1
    }

    fn bytes(&self, zone: usize, ty: AssetType, id: fastfile_iw7::walk::AssetId) -> Option<&'z [u8]> {
        let (z, c) = self.zones[zone];
        if let Some(s) = c.shader_programs.get(&(ty, id.index)) {
            return Some(&z.image[s.pos..s.pos + s.len]);
        }
        let name = c.name_of(id).trim_start_matches(',').to_owned();
        if let Some(b) = self.by_name.get(&(ty, name.clone())) {
            return Some(b);
        }
        self.scan(ty, &name)
    }

    /// A program the walks didn't record under its name (a reference in
    /// every zone read, its body stored where the walk files it under
    /// another asset): the first DXBC container of the wanted stage after
    /// the name's text in any zone.
    fn scan(&self, ty: AssetType, name: &str) -> Option<&'z [u8]> {
        if name.len() < 8 || std::env::var("BO2MP_IW7_PROGRAM_SCAN").is_ok_and(|v| v.trim() == "0") {
            return None;
        }
        let key = (ty, name.to_owned());
        if let Some(&found) = self.scanned.lock().unwrap_or_else(std::sync::PoisonError::into_inner).get(&key) {
            return found;
        }
        let found = self.scan_zones(ty, name);
        self.scanned.lock().unwrap_or_else(std::sync::PoisonError::into_inner).insert(key, found);
        found
    }

    fn scan_zones(&self, ty: AssetType, name: &str) -> Option<&'z [u8]> {
        let want = match ty {
            AssetType::VertexShader => dxbc_sm5::Stage::Vertex,
            AssetType::PixelShader => dxbc_sm5::Stage::Pixel,
            _ => return None,
        };
        let finder = memchr::memmem::Finder::new(name.as_bytes());
        for (z, _) in &self.zones {
            let img = &z.image[..];
            for k in finder.find_iter(img) {
                let at = k + name.len();
                let end = (at + 8192).min(img.len());
                let Some(d) = memchr::memmem::find(&img[at..end], b"DXBC") else { continue };
                let start = at + d;
                let Some(len) = img.get(start + 24..start + 28).map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]) as usize) else { continue };
                let Some(bytes) = img.get(start..start + len) else { continue };
                let Ok(container) = Container::parse(bytes) else { continue };
                if dxbc_sm5::decode(container.shader).is_ok_and(|p| p.stage == want) {
                    return Some(bytes);
                }
            }
        }
        None
    }

    fn get(&self, zone: usize, ty: AssetType, id: Option<fastfile_iw7::walk::AssetId>) -> Result<Program<'z>, String> {
        let id = id.ok_or("no program")?;
        let bytes = self.bytes(zone, ty, id).ok_or_else(|| format!("{} not in the zones", if ty == AssetType::VertexShader { "vertex program" } else { "pixel program" }))?;
        let container = Container::parse(bytes).map_err(|e| format!("{e:?}"))?;
        let program = dxbc_sm5::decode(container.shader).map_err(|e| format!("{e:?}"))?;
        Ok(Program { container, program })
    }

    /// A program's identity for pipelines (its bytes' place and length).
    fn identity(&self, zone: usize, ty: AssetType, id: Option<fastfile_iw7::walk::AssetId>) -> (usize, usize) {
        id.and_then(|id| self.bytes(zone, ty, id)).map_or((0, 0), |b| (b.as_ptr() as usize, b.len()))
    }
}

/// One distinct pass as drawn: the pass, the base stream and the world
/// vertex format (for the layer stream).
#[derive(Clone)]
struct PassInfo {
    /// The zone whose capture holds the pass (`Programs::zones`).
    zone: usize,
    pass: PassRec,
    base: Base,
    wvf: u8,
    /// The second stream's stride (0: none bound, read as zeros).
    second_stride: u64,
}

/// What a pipeline depends on: programs (by their place in the zone),
/// the declaration's routing, the streams.
fn program_key(programs: &Programs, p: &PassInfo) -> String {
    let routing = p.pass.vertex_decl.and_then(|d| programs.cap(p.zone).vertex_decls.get(&d.index));
    format!("{:?}:{:?}:{:?}:{:?}:{}:{}", programs.identity(p.zone, AssetType::VertexShader, p.pass.vertex_shader), programs.identity(p.zone, AssetType::PixelShader, p.pass.pixel_shader), routing, p.base, p.wvf, p.second_stride)
}

/// A pass's identity: its programs and streams, and its arguments.
fn pass_key(programs: &Programs, p: &PassInfo) -> String {
    let mut k = program_key(programs, p);
    for a in &p.pass.args {
        k.push_str(&format!("{},{},{},{},{},{};", a.ty, a.stage, a.dest, a.index, a.first_row, a.row_count));
    }
    k
}

struct Pipe {
    /// Being compiled on a worker thread (the driver's compile dominates
    /// the load) until [`Pipe::finish`].
    job: Option<std::thread::JoinHandle<Result<wgpu::RenderPipeline, String>>>,
    pipeline: Option<wgpu::RenderPipeline>,
    layout: gpu::PassLayout,
    vs: Reflection,
    /// The pixel program discards (alpha test).
    discards: bool,
    /// Debugging (`IW7_DBG_WGSL`): the translated pixel program.
    ps_wgsl: Option<String>,
    vs_wgsl: Option<String>,
}

impl Pipe {
    /// Waits for the compile; false when it failed.
    fn finish(&mut self) -> bool {
        if let Some(j) = self.job.take() {
            self.pipeline = j.join().ok().and_then(Result::ok);
        }
        self.pipeline.is_some()
    }
}

/// Limits the pipelines compiling at once.
struct Slots(std::sync::Mutex<usize>, std::sync::Condvar, usize);
struct Permit(&'static Slots);

fn compile_slots() -> &'static Slots {
    static S: std::sync::OnceLock<Slots> = std::sync::OnceLock::new();
    S.get_or_init(|| {
        let n = std::env::var("BO2MP_IW7_PIPE_THREADS").ok().and_then(|v| v.trim().parse().ok()).unwrap_or_else(|| std::thread::available_parallelism().map_or(2, |n| n.get()).clamp(2, 12));
        Slots(std::sync::Mutex::new(0), std::sync::Condvar::new(), n)
    })
}

impl Slots {
    fn acquire(&'static self) -> Permit {
        let mut used = self.0.lock().unwrap();
        while *used >= self.2 {
            used = self.1.wait(used).unwrap();
        }
        *used += 1;
        Permit(self)
    }
}

impl Drop for Permit {
    fn drop(&mut self) {
        *self.0 .0.lock().unwrap() -= 1;
        self.0 .1.notify_one();
    }
}

fn make_pipe(g: &Gpu, programs: &Programs, layouts: &HashMap<u8, vertex::Layout>, info: &PassInfo, state: PassState) -> Result<Pipe, String> {
    let pass = &info.pass;
    let cap = programs.cap(info.zone);
    let vs = programs.get(info.zone, AssetType::VertexShader, pass.vertex_shader)?;
    let ps = programs.get(info.zone, AssetType::PixelShader, pass.pixel_shader)?;
    let mut po = Options::for_stage(Stage::Pixel);
    // (The programs read SV_Position.w as the view depth: light clusters,
    // volumetric fog.)
    po.d3d_position_w = true;
    po.varyings = dxbc_sm5::link(&vs.container.outputs, &ps.container.inputs);
    let pt = dxbc_sm5::translate_program(&ps.program, &ps.container, &po).map_err(|e| format!("pixel program: {e:?}"))?;
    let mut vo = Options::for_stage(Stage::Vertex);
    vo.varyings = po.varyings.clone();
    vo.output_interp = pt.reflection.inputs.iter().map(|i| (i.semantic.to_ascii_uppercase(), i.index, i.interp)).collect();
    let vt = dxbc_sm5::translate_program(&vs.program, &vs.container, &vo).map_err(|e| format!("vertex program: {e:?}"))?;
    let routing = pass.vertex_decl.and_then(|d| cap.vertex_decls.get(&d.index)).map(Vec::as_slice);
    let layout_l = layouts.get(&info.wvf).filter(|_| info.wvf != 0);
    let (mut base_attrs, mut second_attrs, mut zero_attrs) = (Vec::new(), Vec::new(), Vec::new());
    for i in &vt.reflection.inputs {
        match vertex::input(info.base, routing, layout_l, &i.semantic, i.index) {
            Some((stream, offset, format)) => {
                let a = wgpu::VertexAttribute { format: vertex::as_declared(format, i.component_type), offset, shader_location: i.location };
                match stream {
                    Stream::Base => base_attrs.push(a),
                    Stream::Second => second_attrs.push(a),
                }
            }
            // An input the declaration does not route reads zeros (as a
            // missing D3D11 element does).
            None => zero_attrs.push(wgpu::VertexAttribute { format: vertex::as_declared(wgpu::VertexFormat::Float32x4, i.component_type), offset: 0, shader_location: i.location }),
        }
    }
    if !second_attrs.is_empty() && info.base == Base::World && layout_l.is_none() {
        return Err("layer inputs without a layer layout".into());
    }
    if vt.reflection.textures.iter().chain(&pt.reflection.textures).any(|t| t.dim == TextureDim::D2Ms) {
        return Err("multisampled texture".into());
    }
    let layout = gpu::pass_layout(g, &vt.reflection, &pt.reflection, None);
    let (device, pipeline_layout) = (g.device.clone(), layout.pipeline_layout.clone());
    let (vs_src, ps_src) = (vt.wgsl.clone(), pt.wgsl.clone());
    let (base_stride, second_stride) = (info.base.stride(), info.second_stride);
    let permit = compile_slots().acquire();
    let job = std::thread::spawn(move || {
        let _permit = permit;
        let module = |label: &str, src: &str| device.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some(label), source: wgpu::ShaderSource::Wgsl(src.into()) });
        let scope = device.push_error_scope(wgpu::ErrorFilter::Validation);
        let vm = module("iw7 vs", &vs_src);
        let pm = module("iw7 ps", &ps_src);
        let mut buffers = vec![wgpu::VertexBufferLayout { array_stride: base_stride, step_mode: wgpu::VertexStepMode::Vertex, attributes: &base_attrs }];
        // (Slot 1 the second stream, slot 2 the zeros; both always bound.)
        buffers.push(wgpu::VertexBufferLayout { array_stride: second_stride, step_mode: wgpu::VertexStepMode::Vertex, attributes: &second_attrs });
        buffers.push(wgpu::VertexBufferLayout { array_stride: 0, step_mode: wgpu::VertexStepMode::Vertex, attributes: &zero_attrs });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("iw7 world"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState { module: &vm, entry_point: Some("main"), compilation_options: Default::default(), buffers: &buffers },
            primitive: wgpu::PrimitiveState {
                // D3D's front faces are clockwise.
                front_face: wgpu::FrontFace::Cw,
                cull_mode: state.cull_mode(),
                ..Default::default()
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: wgpu::TextureFormat::Depth32Float,
                depth_write_enabled: Some(state.depth_write),
                depth_compare: Some(state.depth_compare()),
                stencil: Default::default(),
                bias: state.depth_bias(),
            }),
            multisample: Default::default(),
            fragment: Some(wgpu::FragmentState {
                module: &pm,
                entry_point: Some("main"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState { format: wgpu::TextureFormat::Rgba16Float, blend: state.blend_state(), write_mask: state.color_writes() })],
            }),
            multiview_mask: None,
            cache: None,
        });
        match pollster::block_on(scope.pop()) {
            Some(e) => Err(format!("pipeline: {e}")),
            None => Ok(pipeline),
        }
    });
    let ps_wgsl = std::env::var("IW7_DBG_WGSL").ok().map(|_| pt.wgsl.clone());
    let vs_wgsl = std::env::var("IW7_DBG_WGSL").ok().map(|_| vt.wgsl.clone());
    Ok(Pipe { job: Some(job), pipeline: None, layout, vs: vt.reflection, discards: pt.wgsl.contains("discard"), ps_wgsl, vs_wgsl })
}

/// A one-texel texture of `dim` and `kind` holding `v`.
fn solid(g: &Gpu, dim: TextureDim, kind: TextureKind, v: f32) -> wgpu::TextureView {
    use wgpu::TextureViewDimension as V;
    let (view_dim, dimension, layers) = match dim {
        TextureDim::D1 => (V::D1, wgpu::TextureDimension::D1, 1),
        TextureDim::D3 => (V::D3, wgpu::TextureDimension::D3, 1),
        TextureDim::D2Array => (V::D2Array, wgpu::TextureDimension::D2, 1),
        TextureDim::Cube => (V::Cube, wgpu::TextureDimension::D2, 6),
        TextureDim::CubeArray => (V::CubeArray, wgpu::TextureDimension::D2, 6),
        _ => (V::D2, wgpu::TextureDimension::D2, 1),
    };
    if kind == TextureKind::Depth {
        return gpu::solid_depth(g, v, view_dim).view;
    }
    let (format, texel): (wgpu::TextureFormat, Vec<u8>) = match kind {
        TextureKind::Uint => (wgpu::TextureFormat::Rgba32Uint, [0u32; 4].iter().flat_map(|x| x.to_le_bytes()).collect()),
        TextureKind::Sint => (wgpu::TextureFormat::Rgba32Sint, vec![0; 16]),
        _ => (wgpu::TextureFormat::Rgba16Float, [v, v, v, v].iter().flat_map(|&x| half(x).to_le_bytes()).collect()),
    };
    let data: Vec<u8> = (0..layers).flat_map(|_| texel.clone()).collect();
    let t = g.device.create_texture_with_data(
        &g.queue,
        &wgpu::TextureDescriptor {
            label: Some("iw7 solid"),
            size: wgpu::Extent3d { width: 1, height: 1, depth_or_array_layers: layers },
            mip_level_count: 1,
            sample_count: 1,
            dimension,
            format,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        },
        wgpu::util::TextureDataOrder::LayerMajor,
        &data,
    );
    t.create_view(&wgpu::TextureViewDescriptor { dimension: Some(view_dim), ..Default::default() })
}

/// f32 to IEEE half (finite, normal range: the solid colours).
fn half(v: f32) -> u16 {
    if v == 0.0 {
        return 0;
    }
    let b = v.to_bits();
    let sign = ((b >> 16) & 0x8000) as u16;
    let exp = ((b >> 23) & 0xff) as i32 - 127 + 15;
    let man = ((b >> 13) & 0x3ff) as u16;
    sign | ((exp.clamp(0, 31) as u16) << 10) | man
}

/// The map's light maps uploaded as the zone holds them (primary,
/// secondary, second unorm).
fn lightmaps(g: &Gpu, map: &Iw7Map) -> Vec<[Option<wgpu::TextureView>; 3]> {
    map.light_maps
        .iter()
        .map(|names| {
            names.clone().map(|n| {
                let l = map.image(&n, 1 << 14).ok()?;
                if l.format == DXGI_R9G9B9E5 {
                    return Some(upload_rgb9e5(g, &n, l.width, l.height, &l.data));
                }
                let px = fastfile_h1::image::ImagePixels { width: l.width, height: l.height, levels: l.levels.max(1), dxgi_format: l.format, data: l.data };
                // (Only the top level when the chain is short.)
                let px = top_levels(px, 1);
                gpu::upload(g, &n, &px, 1).ok().map(|t| t.view)
            })
        })
        .collect()
}

/// DXGI's shared-exponent HDR format (Geneva's secondary light map; the
/// shared upload doesn't know it, and a light map left out reads black).
const DXGI_R9G9B9E5: u32 = 67;

/// A shared-exponent picture's top level.
fn upload_rgb9e5(g: &Gpu, label: &str, width: u32, height: u32, data: &[u8]) -> wgpu::TextureView {
    use wgpu::util::DeviceExt;
    let need = (width * height * 4) as usize;
    let mut top = data.get(..need).map_or_else(Vec::new, <[u8]>::to_vec);
    top.resize(need, 0);
    let t = g.device.create_texture_with_data(
        &g.queue,
        &wgpu::TextureDescriptor {
            label: Some(label),
            size: wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgb9e5Ufloat,
            usage: wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        },
        wgpu::util::TextureDataOrder::LayerMajor,
        &top,
    );
    t.create_view(&Default::default())
}

/// Keeps the levels the data holds whole.
fn top_levels(mut px: fastfile_h1::image::ImagePixels, faces: u32) -> fastfile_h1::image::ImagePixels {
    let Some(f) = gpu::dxgi_format(px.dxgi_format) else { return px };
    let (bw, bh) = f.block_dimensions();
    let size = f.block_copy_size(None).unwrap_or(4) as usize;
    let mut total = 0usize;
    let mut levels = 0;
    for i in 0..px.levels {
        let (w, h) = ((px.width >> i).max(1), (px.height >> i).max(1));
        let n = (w.div_ceil(bw) * h.div_ceil(bh)) as usize * size;
        if (total + n) * faces as usize > px.data.len() {
            break;
        }
        total += n;
        levels += 1;
    }
    px.levels = levels.max(1);
    px
}

/// The specular occlusion lookup as a 3D texture (`Rgba16Float`, the share in each channel).
fn spec_occlusion_table(g: &Gpu) -> wgpu::TextureView {
    use wgpu::util::DeviceExt;
    let n = crate::spec_occlusion::SIZE as u32;
    let data: Vec<u8> = crate::spec_occlusion::table().iter().flat_map(|&x| { let h = half(x).to_le_bytes(); [h[0], h[1], h[0], h[1], h[0], h[1], 0, 0x3c] }).collect();
    let t = g.device.create_texture_with_data(
        &g.queue,
        &wgpu::TextureDescriptor {
            label: Some("iw7 specular occlusion"),
            size: wgpu::Extent3d { width: n, height: n, depth_or_array_layers: n },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D3,
            format: wgpu::TextureFormat::Rgba16Float,
            usage: wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        },
        wgpu::util::TextureDataOrder::LayerMajor,
        &data,
    );
    t.create_view(&Default::default())
}

/// The specular environment's lookup as a texture (`Rgba16Float`).
fn env_brdf_table(g: &Gpu) -> wgpu::TextureView {
    use wgpu::util::DeviceExt;
    let n = crate::brdf::SIZE as u32;
    let data: Vec<u8> = crate::brdf::table().iter().flat_map(|t| t.iter().flat_map(|&x| half(x).to_le_bytes()).collect::<Vec<u8>>()).collect();
    let t = g.device.create_texture_with_data(
        &g.queue,
        &wgpu::TextureDescriptor {
            label: Some("iw7 env brdf"),
            size: wgpu::Extent3d { width: n, height: n, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba16Float,
            usage: wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        },
        wgpu::util::TextureDataOrder::LayerMajor,
        &data,
    );
    t.create_view(&Default::default())
}

/// A one-texel float texture of `dim` holding `rgba`.
fn solid_rgba(g: &Gpu, dim: TextureDim, rgba: [f32; 4]) -> wgpu::TextureView {
    let v = solid(g, dim, TextureKind::Float, 1.0);
    let t = v.texture();
    let layers = t.depth_or_array_layers();
    let texel: Vec<u8> = rgba.iter().flat_map(|&x| half(x).to_le_bytes()).collect();
    for layer in 0..layers {
        g.queue.write_texture(
            wgpu::TexelCopyTextureInfo { texture: t, mip_level: 0, origin: wgpu::Origin3d { x: 0, y: 0, z: layer }, aspect: wgpu::TextureAspect::All },
            &texel,
            wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(8), rows_per_image: Some(1) },
            wgpu::Extent3d { width: 1, height: 1, depth_or_array_layers: 1 },
        );
    }
    v
}


/// One constant buffer written each frame with the camera's constants.
struct CameraBuffer {
    /// Its arena chunk and byte offset there.
    chunk: usize,
    offset: usize,
    /// Its registers.
    registers: usize,
    /// (register, code index, first row, rows)
    writes: Vec<(usize, u16, usize, usize)>,
}

/// The per-frame constants of every camera buffer, packed into a few
/// large uniform buffers (bound at offsets), mirrored on the CPU and
/// uploaded with one write a chunk a frame.
struct ConstArena {
    align: usize,
    chunks: Vec<(wgpu::Buffer, Vec<u8>)>,
    /// Bytes used in the last chunk.
    used: usize,
}

/// Each arena chunk's size.
const ARENA_CHUNK: usize = 1 << 20;

impl ConstArena {
    fn new(g: &Gpu) -> Self {
        Self { align: g.device.limits().min_uniform_buffer_offset_alignment.max(16) as usize, chunks: Vec::new(), used: ARENA_CHUNK }
    }

    /// Places `regs`: (chunk, offset, the chunk's buffer).
    fn alloc(&mut self, g: &Gpu, regs: &[[f32; 4]]) -> (usize, usize, wgpu::Buffer) {
        let len = regs.len() * 16;
        let at = self.used.next_multiple_of(self.align);
        if self.chunks.is_empty() || at + len > ARENA_CHUNK {
            let buffer = g.device.create_buffer(&wgpu::BufferDescriptor { label: Some("iw7 frame constants"), size: ARENA_CHUNK as u64, usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST, mapped_at_creation: false });
            self.chunks.push((buffer, Vec::with_capacity(ARENA_CHUNK)));
            self.used = 0;
        }
        let at = self.used.next_multiple_of(self.align);
        let k = self.chunks.len() - 1;
        let mirror = &mut self.chunks[k].1;
        mirror.resize(at, 0);
        mirror.extend_from_slice(bytemuck::cast_slice(regs));
        self.used = at + len;
        (k, at, self.chunks[k].0.clone())
    }

    /// Uploads every chunk's used bytes; returns the bytes written.
    fn upload(&self, g: &Gpu) -> u64 {
        let mut n = 0;
        for (buffer, mirror) in &self.chunks {
            g.queue.write_buffer(buffer, 0, mirror);
            n += mirror.len() as u64;
        }
        n
    }
}

/// A uniform binding: buffer, offset, size (0: the whole buffer).
type UniformRange = (wgpu::Buffer, u64, u64);

struct Targets {
    width: u32,
    height: u32,
    colour: wgpu::Texture,
    colour_view: wgpu::TextureView,
    depth_view: wgpu::TextureView,
    final_view: wgpu::TextureView,
    /// The motion-blurred colour (`blur`).
    blurred_view: wgpu::TextureView,
    /// The depth-of-field colour (`dof`).
    dof_view: wgpu::TextureView,
    final_tex: wgpu::Texture,
}

/// The default display exposure (`BO2MP_IW7_EXPOSURE`): what puts riot's
/// median lit pixel near 0.18 in the offline frame.
pub const DEFAULT_EXPOSURE: f32 = 0.18;

/// One draw.
struct Draw {
    pipe: usize,
    group: usize,
    /// Base stream: buffer (0 world, 1 models) and byte offset.
    base: (u8, u64),
    /// Second stream: buffer (0 world layers, 1 model lighting, 2 zeros)
    /// and byte offset.
    second: Option<(u8, u64)>,
    /// Index buffer (0 world, 1 models), first index, count.
    index: (u8, u32, u32),
    /// Reads the scene behind (drawn after a copy of it).
    behind: bool,
    /// World bounds (min, max).
    bounds: [Vec3; 2],
    /// A placed model's cull distance (0: none).
    cull: f32,
}

/// The material's pictures, uploaded once by name.
struct Pictures {
    max: u32,
    by_name: HashMap<String, Option<(wgpu::TextureView, wgpu::TextureViewDimension)>>,
    /// Pictures whose format can't be sampled filtered (32-bit floats).
    unfilterable: std::collections::HashSet<String>,
    loaded: usize,
    missing: usize,
}

impl Pictures {
    fn view(&mut self, g: &Gpu, map: &Iw7Map, name: &str, dim: TextureDim) -> Option<wgpu::TextureView> {
        let want = match dim {
            TextureDim::D2 => wgpu::TextureViewDimension::D2,
            TextureDim::Cube => wgpu::TextureViewDimension::Cube,
            TextureDim::CubeArray => return self.cube_array(g, map, name),
            _ => return None,
        };
        if !self.by_name.contains_key(name) {
            let faces = if want == wgpu::TextureViewDimension::Cube { 6 } else { 1 };
            let made = map.image(name, self.max).ok().and_then(|l| {
                let px = fastfile_h1::image::ImagePixels { width: l.width, height: l.height, levels: l.levels.max(1), dxgi_format: l.format, data: l.data };
                let px = top_levels(px, faces);
                if std::env::var("IW7_DBG_DUMP").is_ok_and(|v| v.split(',').any(|x| x == name)) {
                    let nz = px.data.iter().filter(|&&b| b != 0).count();
                    eprintln!("dump: {name} format {} {}x{} levels {} bytes {} nonzero {} first {:?}", px.dxgi_format, px.width, px.height, px.levels, px.data.len(), nz, &px.data[..px.data.len().min(16)]);
                    if let Ok((w, h, rgba)) = fastfile_h1::image::rgba8_level0(&px) {
                        let f = std::fs::File::create(format!("{}/dump_{name}.png", std::env::var("IW7_DBG_DIR").unwrap_or_else(|_| ".".into()))).expect("png");
                        let mut e = png::Encoder::new(std::io::BufWriter::new(f), w, h);
                        e.set_color(png::ColorType::Rgba);
                        e.set_depth(png::BitDepth::Eight);
                        e.write_header().unwrap().write_image_data(&rgba).unwrap();
                    }
                }
                let t = gpu::upload(g, name, &px, faces).ok()?;
                if !matches!(t.texture.format().sample_type(None, None), Some(wgpu::TextureSampleType::Float { filterable: true })) {
                    self.unfilterable.insert(name.to_owned());
                }
                Some((t.view, t.dim))
            });
            if made.is_some() {
                self.loaded += 1;
            } else {
                self.missing += 1;
            }
            self.by_name.insert(name.to_owned(), made);
        }
        self.by_name.get(name)?.as_ref().filter(|(_, d)| *d == want).map(|(v, _)| v.clone())
    }
}

impl Pictures {
    /// A cube array (the reflection probes'): as many whole cubes as its
    /// data holds, each face with all its levels.
    fn cube_array(&mut self, g: &Gpu, map: &Iw7Map, name: &str) -> Option<wgpu::TextureView> {
        let key = format!("{name}#cubes");
        if !self.by_name.contains_key(&key) {
            let made = map.image(name, self.max).ok().and_then(|l| {
                let f = gpu::dxgi_format(l.format)?;
                let (bw, bh) = f.block_dimensions();
                let size = f.block_copy_size(None)? as usize;
                let face: usize = (0..l.levels.max(1)).map(|i| ((l.width >> i).max(1).div_ceil(bw) * (l.height >> i).max(1).div_ceil(bh)) as usize * size).sum();
                let cubes = (l.data.len() / face.max(1) / 6) as u32;
                if cubes == 0 {
                    return None;
                }
                let px = fastfile_h1::image::ImagePixels { width: l.width, height: l.height, levels: l.levels.max(1), dxgi_format: l.format, data: l.data };
                let t = gpu::upload(g, name, &px, cubes * 6).ok()?;
                let v = t.texture.create_view(&wgpu::TextureViewDescriptor { dimension: Some(wgpu::TextureViewDimension::CubeArray), ..Default::default() });
                Some((v, wgpu::TextureViewDimension::CubeArray))
            });
            if made.is_some() {
                self.loaded += 1;
            } else {
                self.missing += 1;
            }
            self.by_name.insert(key.clone(), made);
        }
        self.by_name.get(&key)?.as_ref().map(|(v, _)| v.clone())
    }
}

/// A material sampler from its state bits (as `render_h1::gpu::
/// material_sampler`), its anisotropy capped at `cap`.
fn material_sampler(g: &Gpu, state: u8, cap: u16) -> wgpu::Sampler {
    let filter = state & 7;
    let mip = state & 0x18;
    let clamp = |bit: u8| if state & bit != 0 { wgpu::AddressMode::ClampToEdge } else { wgpu::AddressMode::Repeat };
    let lin = !matches!(filter, 1);
    let f = if lin { wgpu::FilterMode::Linear } else { wgpu::FilterMode::Nearest };
    let m = if mip == 0x10 { wgpu::MipmapFilterMode::Linear } else { wgpu::MipmapFilterMode::Nearest };
    let aniso: u16 = match filter {
        3 => 2,
        4 => 4,
        6 => 16,
        _ => 1,
    };
    g.device.create_sampler(&wgpu::SamplerDescriptor {
        label: Some("iw7 material"),
        address_mode_u: clamp(0x20),
        address_mode_v: clamp(0x40),
        address_mode_w: clamp(0x80),
        mag_filter: f,
        min_filter: f,
        mipmap_filter: m,
        lod_max_clamp: if mip == 0 { 0.0 } else { 32.0 },
        anisotropy_clamp: if lin && mip == 0x10 { aniso.min(cap).max(1) } else { 1 },
        ..Default::default()
    })
}

/// The map's world and placed models prepared for drawing: buffers,
/// pictures, pipelines and bind groups made once; each frame writes the
/// camera's constants and records the draws.
pub struct Host {
    gpu: Gpu,
    vertex_buffers: [wgpu::Buffer; 2],
    second_buffers: [wgpu::Buffer; 3],
    index_buffers: [wgpu::Buffer; 2],
    pipes: Vec<Pipe>,
    groups: Vec<[wgpu::BindGroup; 3]>,
    /// The camera buffers (their writes name a frame code by its position
    /// in `frame_codes`), the arena they live in, and the frame's rows.
    camera_buffers: Vec<CameraBuffer>,
    frame_codes: Vec<u16>,
    frame_rows: Vec<Option<Rows>>,
    arena: ConstArena,
    /// Opaque draws, then blended ones (by sort key).
    draws: Vec<Draw>,
    sun: Option<Sun>,
    fog: Option<Fog>,
    /// The sky picture's colour scale: the vision's `r_hdrSkyIntensity` (`BO2MP_IW7_SKY_INTENSITY=0` takes one).
    sky_scale: f32,
    targets: Option<Targets>,
    post: crate::post::Post,
    screen: crate::screen::Screen,
    volumetric: crate::volumetric::Volumetric,
    spots: crate::spot::SpotShadows,
    behind: crate::behind::Behind,
    reflections: crate::reflect::Reflections,
    /// The last frame's costs (`profile`).
    pub stats: crate::profile::FrameStats,
    profiling: bool,
    average: crate::profile::Average,
    timer: Option<crate::profile::GpuTimer>,
    timer_slot: Option<usize>,
    /// Motion blur (`blur`) and whether the display read its image last.
    blur: crate::blur::MotionBlur,
    /// What the display read last (bit 0 depth of field, bit 1 motion blur).
    post_input: u8,
    /// Depth of field (`dof`) and the game's numbers for this frame.
    dof: crate::dof::DepthOfField,
    pub dof_frame: crate::dof::Dof,
    /// A fixed frame time for offline frames (None: the clock's).
    pub fixed_dt: Option<f32>,
    /// The graphics quality applied (`native_quality`), the scale on placed
    /// models' cull distances, and the frame's eye.
    pub quality: native_quality::Quality,
    draw_distance: f32,
    frame_eye: Vec3,
    /// The last quality line (`apply_quality`).
    pub quality_line: String,
    started: std::time::Instant,
    clusters: crate::lights::Clusters,
    /// The HDR view the post passes were made for, and when the last frame
    /// was recorded (the exposure's adaptation).
    post_for: usize,
    post_built: usize,
    last_frame: Option<std::time::Instant>,
    pub report: Report,
}

/// Everything shared while the host is made.
struct Build<'a> {
    g: &'a Gpu,
    map: &'a Iw7Map,
    programs: Programs<'a>,
    layouts: HashMap<u8, vertex::Layout>,
    lmaps: Vec<[Option<wgpu::TextureView>; 3]>,
    probes: wgpu::Buffer,
    zero_buffer: wgpu::Buffer,
    /// The light clusters' buffers by raw slot (24 records, 25 shadows,
    /// 35 tiles, 37 slices).
    cluster_buffers: Vec<(u32, wgpu::Buffer)>,
    samplers: [wgpu::Sampler; 3],
    material_samplers: HashMap<u8, wgpu::Sampler>,
    /// The quality's anisotropy cap.
    anisotropy: u16,
    pictures: Pictures,
    passes: HashMap<String, usize>,
    pass_infos: Vec<PassInfo>,
    pipe_of: HashMap<(String, PassState), Option<usize>>,
    pipes: Vec<Pipe>,
    /// (pass, stage, slot) -> shared code buffer.
    shared_cb: HashMap<(usize, u8, u32), UniformRange>,
    arena: ConstArena,
    frame_codes: std::collections::BTreeSet<u16>,
    camera_buffers: Vec<CameraBuffer>,
    group_of: HashMap<(usize, usize, usize, Object), usize>,
    groups: Vec<[wgpu::BindGroup; 3]>,
    lighting_only: bool,
    /// The screen maps (`screen`): sun shadow, ambient occlusion.
    screen_views: [wgpu::TextureView; 2],
    /// The volumetric fog's volumes (`volumetric`): inscatter, transmittance.
    volume_views: [wgpu::TextureView; 2],
    /// The spot shadows (`spot`).
    spot_view: wgpu::TextureView,
    /// The specular environment's lookup (code texture 64, `brdf`).
    env_brdf: wgpu::TextureView,
    /// The specular occlusion lookup (code texture 65, `spec_occlusion`).
    spec_occlusion: wgpu::TextureView,
    /// The scene copy (code texture 18) and the groups that read it.
    scene_view: wgpu::TextureView,
    /// The reflection probes' cube array (code texture 37), if loaded.
    probe_view: Option<wgpu::TextureView>,
    behind_groups: std::collections::HashSet<usize>,
    /// One-texel textures by (dimension, kind, value bits).
    solids: HashMap<(TextureDim, TextureKind, [u32; 4]), wgpu::TextureView>,
    report: Report,
}

impl Build<'_> {
    fn solid(&mut self, dim: TextureDim, kind: TextureKind, rgba: [f32; 4]) -> wgpu::TextureView {
        let g = self.g;
        self.solids
            .entry((dim, kind, rgba.map(f32::to_bits)))
            .or_insert_with(|| if rgba[0] == rgba[1] && rgba[1] == rgba[2] && rgba[2] == rgba[3] { solid(g, dim, kind, rgba[0]) } else { solid_rgba(g, dim, rgba) })
            .clone()
    }

    fn pass(&mut self, info: PassInfo) -> usize {
        let k = pass_key(&self.programs, &info);
        if let Some(&i) = self.passes.get(&k) {
            return i;
        }
        self.pass_infos.push(info);
        self.passes.insert(k, self.pass_infos.len() - 1);
        self.pass_infos.len() - 1
    }

    fn dbg_zero(&self, pass: usize, index: u16, first_row: u8, rows: u8, register: u16, stage: u8) {
        if let Ok(v) = std::env::var("IW7_DBG_ZERO") {
            let info = &self.pass_infos[pass];
            let c = self.programs.cap(info.zone);
            let name = |id: Option<fastfile_iw7::walk::AssetId>| id.map_or("-".to_owned(), |i| c.name_of(i).to_owned());
            let (vs, ps) = (name(info.pass.vertex_shader), name(info.pass.pixel_shader));
            if vs.contains(v.as_str()) || ps.contains(v.as_str()) || v == "all" {
                eprintln!("zeroc: {index} row {first_row} rows {rows} reg {register} stage {stage}: ps {ps}");
            }
        }
    }

    fn pipe(&mut self, pass: usize, state: PassState) -> Option<usize> {
        let key = (program_key(&self.programs, &self.pass_infos[pass]), state);
        if let Some(&p) = self.pipe_of.get(&key) {
            return p;
        }
        let t = std::time::Instant::now();
        let made = make_pipe(self.g, &self.programs, &self.layouts, &self.pass_infos[pass], state);
        self.report.seconds.pipelines += t.elapsed().as_secs_f32();
        let p = match made {
            Ok(p) => {
                self.pipes.push(p);
                Some(self.pipes.len() - 1)
            }
            Err(e) => {
                if std::env::var("IW7_DBG_PIPES").is_ok() {
                    let info = &self.pass_infos[pass];
                    let c = self.programs.cap(info.zone);
                    let name = |id: Option<fastfile_iw7::walk::AssetId>| id.map_or("-".to_owned(), |i| c.name_of(i).to_owned());
                    eprintln!("pipeline: {e}: vs {} ps {} base {:?} wvf {}", name(info.pass.vertex_shader), name(info.pass.pixel_shader), info.base, info.wvf);
                }
                *self.report.skipped.entry(format!("pipeline (per program pair and state): {e}")).or_default() += 1;
                None
            }
        };
        self.pipe_of.insert(key, p);
        p
    }

    /// The shared buffer of a pass's code constants for (stage, slot):
    /// static rows written now, camera rows each frame.
    fn shared_buffer(&mut self, pass: usize, stage: u8, slot: u32, registers: u32) -> UniformRange {
        if let Some(b) = self.shared_cb.get(&(pass, stage, slot)) {
            return b.clone();
        }
        let p = &self.pass_infos[pass].pass;
        let mut regs = vec![[0.0f32; 4]; registers.max(1) as usize];
        let mut writes = Vec::new();
        for (k, arg) in p.args.iter().enumerate() {
            if arg.stage & stage_bit(stage) == 0 {
                continue;
            }
            match bind::target(p, k) {
                Some(Target::Constant { slot: s, register }) if u32::from(s) == slot => {
                    if std::env::var("IW7_DBG_SHARED").is_ok_and(|v| v.parse() == Ok(registers)) {
                        eprintln!("shared: regs {registers} stage {stage} reg {register} ty {} index {} row {} rows {} known {}", arg.ty, arg.index, arg.first_row, arg.row_count, arg.ty == bind::ARG_CODE_CONST && known(arg.index));
                    }
                    if arg.ty == bind::ARG_CODE_CONST {
                        if known(arg.index) {
                            writes.push((usize::from(register), arg.index, usize::from(arg.first_row), usize::from(bind::rows(arg))));
                        } else {
                            *self.report.zero_constants.entry(arg.index).or_default() += 1;
                            self.dbg_zero(pass, arg.index, arg.first_row, arg.row_count, register, stage);
                        }
                    } else if let (Some(l), Some(dst)) = (arg.literal, regs.get_mut(usize::from(register))) {
                        *dst = l;
                    }
                }
                _ => {}
            }
        }
        let range = self.uniform(regs, writes);
        self.shared_cb.insert((pass, stage, slot), range.clone());
        range
    }

    /// A uniform buffer holding `regs`: in the arena when the frame
    /// rewrites some rows (`writes`), else its own.
    fn uniform(&mut self, regs: Vec<[f32; 4]>, writes: Vec<(usize, u16, usize, usize)>) -> UniformRange {
        if writes.is_empty() {
            let buffer = self.g.device.create_buffer_init(&wgpu::util::BufferInitDescriptor { label: Some("iw7 cb"), contents: bytemuck::cast_slice(&regs), usage: wgpu::BufferUsages::UNIFORM });
            return (buffer, 0, 0);
        }
        let (chunk, offset, buffer) = self.arena.alloc(self.g, &regs);
        for w in &writes {
            self.frame_codes.insert(w.1);
        }
        self.camera_buffers.push(CameraBuffer { chunk, offset, registers: regs.len(), writes });
        (buffer, offset as u64, (regs.len() * 16) as u64)
    }

    /// The per-primitive buffer (b0) of an object (its camera rows, such
    /// as the sky's view-projection, written each frame).
    fn object_buffer(&mut self, pass: usize, stage: u8, registers: u32, rows: &ObjectRows) -> UniformRange {
        let p = self.pass_infos[pass].pass.clone();
        let mut writes = Vec::new();
        let mut regs = vec![[0.0f32; 4]; registers.max(1) as usize];
        for (k, arg) in p.args.iter().enumerate() {
            if arg.stage & stage_bit(stage) == 0 || bind::group(&p, k) != 0 {
                continue;
            }
            let Some(Target::Constant { register, .. }) = bind::target(&p, k) else { continue };
            if arg.ty == bind::ARG_CODE_CONST {
                if let Some(v) = rows.code(arg.index) {
                    for r in 0..usize::from(bind::rows(arg)) {
                        if let Some(dst) = regs.get_mut(usize::from(register) + r) {
                            *dst = v.get(usize::from(arg.first_row) + r).copied().unwrap_or([0.0; 4]);
                        }
                    }
                } else if known(arg.index) {
                    writes.push((usize::from(register), arg.index, usize::from(arg.first_row), usize::from(bind::rows(arg))));
                } else {
                    *self.report.zero_constants.entry(arg.index).or_default() += 1;
                    self.dbg_zero(pass, arg.index, arg.first_row, arg.row_count, register, stage);
                }
            } else if let (Some(l), Some(dst)) = (arg.literal, regs.get_mut(usize::from(register))) {
                *dst = l;
            }
        }
        self.uniform(regs, writes)
    }

    /// The material's constant buffer for one stage at the drawn slot.
    fn material_buffer(&self, m: &MapMaterial, slot: usize, stage: u8, registers: u32) -> wgpu::Buffer {
        let n = registers.max(1) as usize * 16;
        let mut bytes = if self.lighting_only { Vec::new() } else { m.cb_index.get(slot).and_then(|&i| m.cbuffers.get(usize::from(i))).map_or_else(Vec::new, |c| c[usize::from(stage)].clone()) };
        // The technique's own buffer can be shorter than the program reads
        // (a layered material's lit technique holds `colorTint` but reads
        // `colorTint1` and `colorTint2` too, and a missing row read as zero
        // painted every layer black): the material's longest buffer that
        // covers the program's rows stands in. `BO2MP_IW7_CB_FILL=0`: as stored.
        let need = registers.max(1) as usize * 16;
        if !bytes.is_empty() && bytes.len() < need && !std::env::var("BO2MP_IW7_CB_FILL").is_ok_and(|v| v.trim() == "0") {
            let stand_in = m.cbuffers.iter().map(|c| &c[usize::from(stage)]).filter(|c| c.len() >= need).min_by_key(|c| c.len());
            if let Some(c) = stand_in {
                bytes = c.clone();
            }
        }
        if bytes.is_empty() {
            bytes = bytemuck::cast_slice(&vec![[1.0f32; 4]; registers.max(1) as usize]).to_vec();
        }
        if std::env::var("IW7_DBG_CB").is_ok_and(|v| m.name.contains(v.as_str())) {
            let f: Vec<f32> = bytes.chunks_exact(4).map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]])).collect();
            eprintln!("cb: {} slot {slot} stage {stage} registers {registers} stored {} bytes cb_index {:?} cbuffers {} floats {:?}", m.name, bytes.len(), m.cb_index, m.cbuffers.len(), f);
        }
        bytes.resize(n, 0);
        self.g.device.create_buffer_init(&wgpu::util::BufferInitDescriptor { label: Some("iw7 b3"), contents: &bytes, usage: wgpu::BufferUsages::UNIFORM })
    }

    #[allow(clippy::too_many_arguments)]
    fn group(&mut self, pass: usize, pipe: usize, material: usize, technique: usize, lightmap: usize, object: Object, rows: &ObjectRows) -> usize {
        let key = (pipe, material, lightmap, object);
        if let Some(&i) = self.group_of.get(&key) {
            return i;
        }
        let t = std::time::Instant::now();
        let i = self.make_group(pass, pipe, material, technique, lightmap, object, rows);
        self.report.seconds.groups += t.elapsed().as_secs_f32();
        i
    }

    #[allow(clippy::too_many_arguments)]
    fn make_group(&mut self, pass: usize, pipe: usize, material: usize, technique: usize, lightmap: usize, object: Object, rows: &ObjectRows) -> usize {
        let key = (pipe, material, lightmap, object);
        let g = self.g;
        let m = &self.map.materials[material];
        let p = self.pass_infos[pass].pass.clone();
        let (vs_n, cbs, textures, samplers, cmp) = {
            let pp = &self.pipes[pipe];
            (pp.vs.cbuffers.len(), pp.layout.cbuffers.clone(), pp.layout.textures.clone(), pp.layout.samplers.clone(), pp.layout.comparison_bindings.clone())
        };
        let mut cb_entries = Vec::new();
        for (i, &(binding, slot, registers)) in cbs.iter().enumerate() {
            let stage = u8::from(i >= vs_n);
            let buffer = if slot == 0 {
                self.object_buffer(pass, stage, registers, rows)
            } else if slot == u32::from(bind::MATERIAL_BUFFER) {
                (self.material_buffer(m, technique, stage, registers), 0, 0)
            } else {
                self.shared_buffer(pass, stage, slot, registers)
            };
            cb_entries.push((binding, buffer));
        }
        // Textures: material pictures by hash, code textures, engine slots.
        let mut code_tex: HashMap<(u8, u32), u16> = HashMap::new();
        let mut mat_tex: HashMap<(u8, u32), u32> = HashMap::new();
        let mut mat_samp: HashMap<(u8, u32), u32> = HashMap::new();
        for (k, arg) in p.args.iter().enumerate() {
            for stage in [0u8, 1] {
                if arg.stage & stage_bit(stage) == 0 {
                    continue;
                }
                let hash = u32::from(arg.index) | (u32::from(arg.first_row) << 16) | (u32::from(arg.row_count) << 24);
                match (arg.ty, bind::target(&p, k)) {
                    (bind::ARG_CODE_TEXTURE, Some(Target::Texture { slot })) => {
                        code_tex.insert((stage, u32::from(slot)), arg.index);
                    }
                    (bind::ARG_MATERIAL_TEXTURE, Some(Target::Texture { slot })) => {
                        mat_tex.insert((stage, u32::from(slot)), hash);
                    }
                    (bind::ARG_MATERIAL_SAMPLER, Some(Target::Sampler { slot })) => {
                        mat_samp.insert((stage, u32::from(slot)), hash);
                    }
                    _ => {}
                }
            }
        }
        let normal_hash = fastfile_iw7::texture::name_hash("normalMap");
        let lmap = self.lmaps.get(lightmap).cloned();
        let mut views: Vec<(u32, Option<wgpu::TextureView>, bool)> = Vec::new();
        let mut raw_slots: HashMap<u32, u32> = HashMap::new();
        for &(binding, slot, dim, kind, is_ps) in &textures {
            let stage = u8::from(is_ps);
            if kind == TextureKind::RawBuffer {
                views.push((binding, None, !is_ps && slot == PROBE_SLOT));
                raw_slots.insert(binding, slot);
                continue;
            }
            let fed = code_tex.get(&(stage, slot)).copied();
            let view = if let Some(&h) = mat_tex.get(&(stage, slot)) {
                let picture = m.textures.iter().find(|t| t.0 == h);
                let flat = h == normal_hash || picture.is_some_and(|t| t.3.ends_with("_n") || t.3.ends_with("_nml"));
                let t0 = std::time::Instant::now();
                let got = if self.lighting_only || dbg_white(h) { None } else { picture.and_then(|t| self.pictures.view(g, self.map, &t.3, dim)) };
                self.report.seconds.pictures += t0.elapsed().as_secs_f32();
                match got {
                    Some(v) => v,
                    None if flat => self.solid(dim, kind, [0.5, 0.5, 1.0, 1.0]),
                    None => self.solid(dim, kind, [1.0; 4]),
                }
            } else if fed.is_none() && LIGHTMAP_SLOTS.contains(&slot) && dim == TextureDim::D2 {
                let i = LIGHTMAP_SLOTS.iter().position(|&s| s == slot).unwrap_or(0);
                match lmap.as_ref().and_then(|l| l[i].clone()) {
                    Some(v) => v,
                    None => self.solid(dim, kind, [0.0; 4]),
                }
            } else if let (Some(CODE_TEXTURE_PROBES), TextureDim::CubeArray, Some(v)) = (fed, dim, self.probe_view.as_ref()) {
                v.clone()
            } else if fed == Some(CODE_TEXTURE_SCENE) && dim == TextureDim::D2 {
                self.behind_groups.insert(self.groups.len());
                self.scene_view.clone()
            } else if fed == Some(CODE_TEXTURE_SPEC_OCCLUSION) && dim == TextureDim::D3 {
                self.spec_occlusion.clone()
            } else if fed == Some(CODE_TEXTURE_ENV_BRDF) && dim == TextureDim::D2 {
                self.env_brdf.clone()
            } else if fed == Some(CODE_TEXTURE_SPOT_SHADOWS) && dim == TextureDim::D2Array {
                self.spot_view.clone()
            } else if fed == Some(CODE_TEXTURE_FOG_INSCATTER) && dim == TextureDim::D3 {
                self.volume_views[0].clone()
            } else if fed == Some(CODE_TEXTURE_FOG_TRANSMIT) && dim == TextureDim::D3 {
                self.volume_views[1].clone()
            } else if fed == Some(CODE_TEXTURE_FOG_INSCATTER) {
                self.solid(dim, kind, [0.0; 4])
            } else if fed == Some(CODE_TEXTURE_SUN_SHADOW) && dim == TextureDim::D2 {
                self.screen_views[0].clone()
            } else if fed == Some(CODE_TEXTURE_AO) && dim == TextureDim::D2 {
                self.screen_views[1].clone()
            } else {
                self.solid(dim, kind, [1.0; 4])
            };
            views.push((binding, Some(view), false));
        }
        let mut sampler_list: Vec<(u32, wgpu::Sampler)> = Vec::new();
        for &(binding, slot, is_ps) in &samplers {
            let stage = u8::from(is_ps);
            let s = if cmp.contains(&binding) {
                self.samplers[2].clone()
            } else if let Some(&h) = mat_samp.get(&(stage, slot)) {
                let state = m.textures.iter().find(|t| t.0 == h).map_or(0, |t| t.1);
                let cap = self.anisotropy;
                self.material_samplers.entry(state).or_insert_with(|| material_sampler(g, state, cap)).clone()
            } else {
                self.samplers[0].clone()
            };
            sampler_list.push((binding, s));
        }
        let pp = &self.pipes[pipe];
        let mk = |layout: &wgpu::BindGroupLayout, entries: &[wgpu::BindGroupEntry]| g.device.create_bind_group(&wgpu::BindGroupDescriptor { label: Some("iw7"), layout, entries });
        let cb: Vec<wgpu::BindGroupEntry> = cb_entries
            .iter()
            .map(|(b, (buf, offset, size))| wgpu::BindGroupEntry { binding: *b, resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding { buffer: buf, offset: *offset, size: std::num::NonZeroU64::new(*size) }) })
            .collect();
        let tx: Vec<wgpu::BindGroupEntry> = views
            .iter()
            .map(|(b, v, probes)| wgpu::BindGroupEntry {
                binding: *b,
                resource: match v {
                    Some(v) => wgpu::BindingResource::TextureView(v),
                    None if *probes => self.probes.as_entire_binding(),
                    None => match raw_slots.get(b).and_then(|slot| self.cluster_buffers.iter().find(|(s, _)| s == slot)) {
                        Some((_, buf)) => buf.as_entire_binding(),
                        None => self.zero_buffer.as_entire_binding(),
                    },
                },
            })
            .collect();
        let sm: Vec<wgpu::BindGroupEntry> = sampler_list.iter().map(|(b, s)| wgpu::BindGroupEntry { binding: *b, resource: wgpu::BindingResource::Sampler(s) }).collect();
        self.groups.push([mk(&pp.layout.groups[0], &cb), mk(&pp.layout.groups[1], &tx), mk(&pp.layout.groups[2], &sm)]);
        self.group_of.insert(key, self.groups.len() - 1);
        self.groups.len() - 1
    }
}

fn stage_bit(stage: u8) -> u8 {
    if stage == 0 { bind::STAGE_VS } else { bind::STAGE_PS }
}

/// How a surface goes into the depth pre-pass and the sun cascades
/// (`None`: not at all): opaque and depth-writing, not sky, unlit or a
/// decal (polygon offset), not marked `_nocast` by its technique set;
/// `Some(0)` solid, `Some(n)` alpha tested through the screen's alpha map
/// `n` ([`crate::screen::Screen::add_cutout`]). An alpha-tested surface
/// (`_atest` in the technique's or the set's name, or a pixel program that
/// discards) drawn solid would cover its whole quad (a tree's canopy:
/// black canopies and the ground under them in game), so it casts through
/// its alpha: the material's `_packed_a` picture (red) where it has one,
/// else its colour map's alpha (not a `_packed_cs` colour map, whose alpha
/// is something else), and nothing without a picture.
fn caster(b: &mut Build, screen: &mut crate::screen::Screen, cutouts: &mut HashMap<String, Option<u32>>, m: &MapMaterial, technique: &str, state: &PassState, discards: bool) -> Option<u32> {
    let set = m.technique_set.as_str();
    let opaque = state.blend_state().is_none() && state.depth_write && state.polygon_offset == 0;
    if !opaque || set.contains("nocast") || technique.starts_with("sky") || technique.starts_with("vertcol") || technique.starts_with("mul_") {
        return None;
    }
    if !(technique.contains("atest") || set.contains("atest") || discards) {
        return Some(0);
    }
    if b.lighting_only {
        return None;
    }
    let colour = fastfile_iw7::texture::name_hash("colorMap");
    let (name, channel) = match m.textures.iter().find(|t| t.3.ends_with("_packed_a")) {
        Some(t) => (t.3.clone(), 0),
        None => (m.textures.iter().find(|t| t.0 == colour && !t.3.contains("_packed_cs"))?.3.clone(), 3),
    };
    if let Some(&c) = cutouts.get(&name) {
        return c;
    }
    // (Only a picture the cutout pass can sample filtered: a group with
    // another would be invalid, and with it the whole frame.)
    if std::env::var("IW7_DBG_CUTOUT").is_ok() {
        eprintln!("cutout: set {set} technique {technique} atest-named {} discards {discards} map {name} ch {channel}", technique.contains("atest") || set.contains("atest"));
    }
    let c = b.pictures.view(b.g, b.map, &name, TextureDim::D2).filter(|_| !b.pictures.unfilterable.contains(&name)).map(|v| screen.add_cutout(b.g, &v, channel));
    cutouts.insert(name, c);
    c
}

/// The light grid probe nearest each point (a coarse grid search).
fn nearest_probes(probes: &[([f32; 3], [u8; 64])], points: &[[f32; 3]]) -> Vec<Option<u32>> {
    if probes.is_empty() {
        return vec![None; points.len()];
    }
    const CELL: f32 = 256.0;
    let key = |p: [f32; 3]| ((p[0] / CELL).floor() as i32, (p[1] / CELL).floor() as i32, (p[2] / CELL).floor() as i32);
    let mut grid: HashMap<(i32, i32, i32), Vec<u32>> = HashMap::new();
    for (i, (p, _)) in probes.iter().enumerate() {
        grid.entry(key(*p)).or_default().push(i as u32);
    }
    points
        .iter()
        .map(|&q| {
            let (cx, cy, cz) = key(q);
            let d2 = |i: u32| {
                let p = probes[i as usize].0;
                (p[0] - q[0]).powi(2) + (p[1] - q[1]).powi(2) + (p[2] - q[2]).powi(2)
            };
            for r in 1..=8 {
                let mut best: Option<(f32, u32)> = None;
                for x in cx - r..=cx + r {
                    for y in cy - r..=cy + r {
                        for z in cz - r..=cz + r {
                            for &i in grid.get(&(x, y, z)).into_iter().flatten() {
                                let d = d2(i);
                                if best.is_none_or(|b| d < b.0) {
                                    best = Some((d, i));
                                }
                            }
                        }
                    }
                }
                if let Some((_, i)) = best {
                    return Some(i);
                }
            }
            (0..probes.len() as u32).min_by(|&a, &b| d2(a).total_cmp(&d2(b)))
        })
        .collect()
}

impl Host {
    /// `techsets`: the map's technique set zone.
    pub fn new(g: Gpu, map: &Iw7Map, techsets: &Zone, extra: &[&Zone]) -> Result<Self, String> {
        Self::new_within(g, map, techsets, extra, &|_, _| true)
    }

    /// As [`Host::new`], drawing only the surfaces and placed models whose
    /// bounds (mins, maxs) `keep` takes (offline shots on a software
    /// device, where every pipeline costs memory).
    /// `extra`: zones whose programs the map's passes may name (the global
    /// technique sets).
    pub fn new_within(g: Gpu, map: &Iw7Map, techsets: &Zone, extra: &[&Zone], keep: &dyn Fn([f32; 3], [f32; 3]) -> bool) -> Result<Self, String> {
        let started = std::time::Instant::now();
        let (walk, _) = fastfile_iw7::walk::walk_fitted(techsets);
        let extra_walks: Vec<(&Zone, fastfile_iw7::walk::Capture)> = extra.iter().map(|z| (*z, fastfile_iw7::walk::walk_fitted(z).0.cap)).collect();
        let walks_took = started.elapsed().as_secs_f32();
        let mut zones: Vec<(&Zone, &fastfile_iw7::walk::Capture)> = vec![(techsets, &walk.cap)];
        zones.extend(extra_walks.iter().map(|(z, c)| (*z, c)));
        // Technique sets by name: the map's own first, unless its copy is
        // empty (a set another zone defines: 169 on mp_geneva, the most of
        // its placed models' sets) and a later zone's is whole.
        let mut sets: HashMap<&str, (usize, &TechniqueSetRec)> = HashMap::new();
        for (zi, (_, c)) in zones.iter().enumerate() {
            for t in &c.techsets {
                let e = sets.entry(t.name.trim_start_matches(',')).or_insert((zi, t));
                if !has_passes(e.1) && has_passes(t) {
                    *e = (zi, t);
                }
            }
        }
        let sun = Sun::of(map);
        let fog = Fog::of(map);
        if map.raw_vertices.len() != map.vertices.len() * 44 {
            return Err("no raw vertex stream".into());
        }
        let init = |label: &str, contents: &[u8], usage| {
            let mut c = contents.to_vec();
            while c.is_empty() || !c.len().is_multiple_of(4) {
                c.push(0);
            }
            g.device.create_buffer_init(&wgpu::util::BufferInitDescriptor { label: Some(label), contents: &c, usage })
        };
        // Models: every placed model's mesh, its surfaces concatenated
        // (each vertex with its light map uv: `vertex::MODEL_STRIDE`).
        let mut model_vb: Vec<u8> = Vec::new();
        let mut model_ib: Vec<u16> = Vec::new();
        // model -> per surface (byte offset, first index, index count,
        // material, first vertex in the model)
        type Part = (u64, u32, u32, Option<usize>, u64);
        let mut model_parts: Vec<Vec<Part>> = Vec::new();
        for mm in &map.models {
            let mut parts = Vec::new();
            let mut vert_in_model = 0u64;
            for s in &mm.surfaces {
                let off = model_vb.len() as u64;
                for (i, v) in s.verts.iter().enumerate() {
                    model_vb.extend_from_slice(v);
                    let uv = s.lmap_uv.get(i).copied().unwrap_or([0.0; 2]);
                    model_vb.extend_from_slice(bytemuck::cast_slice(&uv));
                }
                let first = model_ib.len() as u32;
                for t in &s.tris {
                    model_ib.extend_from_slice(t);
                }
                parts.push((off, first, (model_ib.len() as u32) - first, s.material, vert_in_model));
                vert_in_model += s.verts.len() as u64;
            }
            model_parts.push(parts);
        }
        // Placed models' lighting streams.
        let mut lighting: Vec<u8> = Vec::new();
        let mut lighting_at: Vec<Option<u64>> = Vec::new();
        let mut unlit_at: HashMap<usize, u64> = HashMap::new();
        for sm in &map.static_models {
            if sm.vertex_lighting.is_empty() {
                // No baked vertex lighting (the zones hold none: every
                // `numLightingValues` is zero): the lighting stream the
                // vertex program reads is made from the vertex normals, one
                // block per model, shared by its placements. Each entry's
                // word is the normal for the probe lookup and the full probe
                // scale (a zero word lit nothing: its scale bits read zero),
                // its first weight one on the placement's nearest probe.
                if map.models[sm.model].surfaces.iter().all(|s| s.verts.is_empty()) || std::env::var("BO2MP_IW7_UNLIT_PROBE").is_ok_and(|v| v.trim() == "0") {
                    lighting_at.push(None);
                } else {
                    let at = *unlit_at.entry(sm.model).or_insert_with(|| {
                        let at = lighting.len() as u64;
                        for surf in &map.models[sm.model].surfaces {
                            for v in &surf.verts {
                                let word = octahedral_word(u32::from_le_bytes([v[24], v[25], v[26], v[27]]));
                                let mut e = [0u8; 20];
                                e[0..4].copy_from_slice(&word.to_le_bytes());
                                e[12..14].copy_from_slice(&u16::MAX.to_le_bytes());
                                lighting.extend_from_slice(&e);
                            }
                        }
                        at
                    });
                    lighting_at.push(Some(at));
                }
            } else {
                lighting_at.push(Some(lighting.len() as u64));
                for v in &sm.vertex_lighting {
                    lighting.extend_from_slice(v);
                }
            }
        }
        let probe_bytes: Vec<u8> = map.light_probes.iter().flat_map(|(_, d)| d.iter().copied()).collect();
        let probes = init("iw7 probes", &probe_bytes, wgpu::BufferUsages::STORAGE);
        let model_probe = nearest_probes(&map.light_probes, &map.static_models.iter().map(|s| s.lighting_origin).collect::<Vec<_>>());
        // The objects' world matrices (0 the world's identity, 1 + k
        // placed model k) and the screen maps.
        let mut objects: Vec<[[f32; 4]; 3]> = vec![[[1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0]]];
        for sm in &map.static_models {
            let r = ObjectRows::model(sm.origin, sm.axis, sm.scale, None).world;
            objects.push([r[0], r[1], r[2]]);
        }
        let (sw, sh) = std::env::var("BO2MP_IW7_SCREEN")
            .ok()
            .and_then(|v| {
                let (a, b) = v.split_once('x')?;
                Some((a.trim().parse().ok()?, b.trim().parse().ok()?))
            })
            .unwrap_or((1920u32, 1080u32));
        let mut screen = crate::screen::Screen::new(&g, sw.max(64), sh.max(64), &objects);
        // (The map's own ambient occlusion strength: mp_fallen's is 0.)
        screen.ao_strength = map.vision.as_ref().and_then(|(_, t)| fastfile_iw7::light::vision_value(t, "r_ssaoStrength")).and_then(|v| v.split_whitespace().next()?.parse::<f32>().ok()).filter(|v| v.is_finite()).unwrap_or(1.0).max(0.0);
        let mut lights = crate::lights::of_map(map);
        let spot_views = crate::spot::choose(&mut lights);
        let clusters = crate::lights::Clusters::new(&g, lights);
        let spots = crate::spot::SpotShadows::new(&g, &screen, spot_views);
        let behind = crate::behind::Behind::new(&g, screen.width, screen.height);
        let volumetric = crate::volumetric::Volumetric::new(&g, map.vision.as_ref().and_then(|(_, t)| crate::volumetric::Settings::from_vision(t)), screen.sun_views());
        // The graphics quality read as the scene is made: the pictures' size
        // cap (`BO2MP_IW7_TEXTURE_MAX` wins) and the anisotropy cap.
        let quality = native_quality::current();
        let max = std::env::var("BO2MP_IW7_TEXTURE_MAX").ok().and_then(|v| v.parse().ok()).unwrap_or(quality.max_texture.clamp(64, 16384));
        let mut b = Build {
            g: &g,
            map,
            programs: Programs::new(zones.clone()),
            layouts: {
                let mut l = vertex::layer_layouts(&walk.cap);
                for (_, c) in &extra_walks {
                    for (f, lay) in vertex::layer_layouts(c) {
                        l.entry(f).or_insert(lay);
                    }
                }
                l
            },
            lmaps: lightmaps(&g, map),
            probes,
            cluster_buffers: vec![(24, clusters.records.clone()), (25, clusters.shadows.clone()), (35, clusters.tiles.clone()), (37, clusters.slices.clone())],
            zero_buffer: g.device.create_buffer(&wgpu::BufferDescriptor { label: Some("iw7 zero"), size: 1 << 22, usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::VERTEX, mapped_at_creation: false }),
            samplers: [gpu::linear_clamp(&g), gpu::linear_clamp(&g), gpu::comparison(&g)],
            material_samplers: HashMap::new(),
            anisotropy: quality.anisotropy.max(1),
            pictures: Pictures { max, by_name: HashMap::new(), unfilterable: Default::default(), loaded: 0, missing: 0 },
            passes: HashMap::new(),
            pass_infos: Vec::new(),
            pipe_of: HashMap::new(),
            pipes: Vec::new(),
            shared_cb: HashMap::new(),
            arena: ConstArena::new(&g),
            frame_codes: Default::default(),
            camera_buffers: Vec::new(),
            group_of: HashMap::new(),
            groups: Vec::new(),
            lighting_only: lighting_only(),
            screen_views: [screen.shadow.1.clone(), screen.ao.1.clone()],
            volume_views: [volumetric.inscatter.1.clone(), volumetric.transmit.1.clone()],
            spot_view: spots.array.1.clone(),
            env_brdf: env_brdf_table(&g),
            spec_occlusion: spec_occlusion_table(&g),
            scene_view: behind.view.clone(),
            behind_groups: Default::default(),
            solids: HashMap::new(),
            report: Report { surfaces: map.surfaces.len(), models_placed: map.static_models.len(), ..Default::default() },
            probe_view: None,
        };
        // The reflection probes' cubes and buffers (`reflect`).
        let cubes = match (&map.probe_image, b.lighting_only) {
            (Some(name), false) => b.pictures.view(&g, map, name, TextureDim::CubeArray),
            _ => None,
        };
        let reflections = crate::reflect::Reflections::new(&g, &map.probe_origins, cubes);
        b.probe_view = reflections.cubes.clone();
        b.cluster_buffers.extend([(31, reflections.records.clone()), (32, reflections.cells.clone()), (33, reflections.sh.clone())]);
        // (draw, blended, sort key, order)
        let mut draws: Vec<(Draw, bool, u8, usize)> = Vec::new();
        let skip = |r: &mut Report, why: &str| *r.skipped.entry(why.to_string()).or_default() += 1;
        let world_rows = ObjectRows::world();
        // (Debugging: `IW7_DBG_SKIP=a,b`: surfaces whose material or technique set name holds one of these are left out.)
        let dbg_skip: Vec<String> = std::env::var("IW7_DBG_SKIP").map(|v| v.split(',').filter(|x| !x.is_empty()).map(str::to_owned).collect()).unwrap_or_default();
        let mut cutouts: HashMap<String, Option<u32>> = HashMap::new();
        let model_lightmaps = !std::env::var("BO2MP_IW7_MODEL_LIGHTMAPS").is_ok_and(|v| v.trim() == "0");
        for s in &map.surfaces {
            if s.hidden_brush || s.tri_count == 0 {
                skip(&mut b.report, "surfaces hidden or empty");
                continue;
            }
            if !keep(s.mins, s.maxs) {
                skip(&mut b.report, "surfaces left out of this frame");
                continue;
            }
            let Some(mi) = s.material.filter(|&i| i < map.materials.len()) else {
                skip(&mut b.report, "surfaces without a material");
                continue;
            };
            let m = &map.materials[mi];
            if dbg_skip.iter().any(|x| m.name.contains(x.as_str()) || m.technique_set.contains(x.as_str())) {
                continue;
            }
            let Some(&(zone_i, set)) = sets.get(m.technique_set.as_str()) else {
                skip(&mut b.report, "surfaces whose technique set is not in the zones");
                continue;
            };
            let Some((tname, pass, slot)) = draw_pass(set) else {
                skip(&mut b.report, "surfaces with no forward technique (tools, shadow casters)");
                continue;
            };
            let wvf = set.world_vert_format;
            let second_stride = b.layouts.get(&wvf).filter(|_| wvf != 0).map_or(0, |l| l.stride);
            let pass_i = b.pass(PassInfo { zone: zone_i, pass: pass.clone(), base: Base::World, wvf, second_stride });
            let state = crate::state::of_material(m, slot);
            let Some(pipe) = b.pipe(pass_i, state) else {
                skip(&mut b.report, "surfaces of pipelines not made");
                continue;
            };
            let lm = usize::from(s.lightmap_index).min(b.lmaps.len().saturating_sub(1));
            let group = b.group(pass_i, pipe, mi, slot, lm, Object::World, &world_rows);
            let second = (second_stride > 0).then_some((0u8, u64::from(s.layer_offset)));
            let order = draws.len();
            let index = (0, s.base_index, u32::from(s.tri_count) * 3);
            let discards = b.pipes[pipe].discards;
            if std::env::var("IW7_DBG_DRAWS").is_ok() {
                eprintln!("draw: world {} set {} tech {} blend {} depthwrite {} polyoff {} discards {} tris {} bounds {:?} {:?}", m.name, m.technique_set, tname.split('.').next().unwrap_or(""), state.blend_state().is_some(), state.depth_write, state.polygon_offset, discards, s.tri_count, s.mins, s.maxs);
            }
            if let (Ok(dir), Some(w)) = (std::env::var("IW7_DBG_WGSL"), b.pipes[pipe].ps_wgsl.as_ref()) {
                let _ = std::fs::write(format!("{dir}/ps_{}_{pipe}.wgsl", m.name.replace(['*', '/'], "_")), w);
            }
            if std::env::var("IW7_DBG_LAYER").is_ok_and(|v| m.name.contains(v.as_str())) && second_stride > 0 {
                let at = s.layer_offset as usize;
                let l = b.layouts.get(&wvf);
                let mut rows = Vec::new();
                for v in 0..3usize {
                    let r = map.layer_data.get(at + v * second_stride as usize..at + (v + 1) * second_stride as usize).map(|r| r.chunks(4).map(|c| { let f = f32::from_le_bytes([c[0], c[1], c[2], c[3]]); format!("{f:.3}/{:02x}{:02x}{:02x}{:02x}", c[0], c[1], c[2], c[3]) }).collect::<Vec<_>>().join(" "));
                    rows.push(r.unwrap_or_default());
                }
                eprintln!("layer: {} wvf {wvf} stride {second_stride} layout {:?} first_vertex {} tris {} offset {} | {}", m.name, l.map(|l| l.components.clone()), s.first_vertex, s.tri_count, s.layer_offset, rows.join(" | "));
            }
            if std::env::var("IW7_DBG_TEX").is_ok_and(|v| m.name.contains(v.as_str())) {
                let list: Vec<String> = m.textures.iter().map(|t| format!("{:08x}:{}:{}", t.0, t.3, b.pictures.view(b.g, b.map, &t.3, TextureDim::D2).is_some())).collect();
                eprintln!("tex: {} tech {} -> {}", m.name, tname.split('.').next().unwrap_or(""), list.join(" "));
                eprintln!("const: {} -> {:?}", m.name, m.constants);
            }
            if let Some(cutout) = caster(&mut b, &mut screen, &mut cutouts, m, tname, &state, discards) {
                screen.occluders.push(crate::screen::Occluder { base: (0, u64::from(s.first_vertex) * 44), index, object: 0, bounds: [Vec3::from(s.mins), Vec3::from(s.maxs)], cutout });
            }
            draws.push((Draw { pipe, group, base: (0, u64::from(s.first_vertex) * 44), second, index, behind: b.behind_groups.contains(&group), bounds: [Vec3::from(s.mins), Vec3::from(s.maxs)], cull: 0.0 }, state.blend_state().is_some(), m.sort_key, order));
        }
        b.report.drawn = draws.len();
        for (k, sm) in map.static_models.iter().enumerate() {
            let Some(parts) = model_parts.get(sm.model) else { continue };
            let r = map.models[sm.model].radius * sm.scale.max(1e-3);
            if std::env::var("IW7_DBG_MODEL").is_ok_and(|v| map.models[sm.model].name.contains(v.as_str())) {
                let pr = model_probe[k].and_then(|i| map.light_probes.get(i as usize));
                eprintln!("model: {} at {:?} scale {} lit {:?} lighting origin {:?} probe {:?} at {:?} data {:?} of {} probes", map.models[sm.model].name, sm.origin, sm.scale, lighting_at[k].is_some(), sm.lighting_origin, model_probe[k], pr.map(|p| p.0), pr.map(|p| p.1.to_vec()), map.light_probes.len());
            }
            if !keep(sm.origin.map(|c| c - r), sm.origin.map(|c| c + r)) {
                continue;
            }
            // Its own light map and place in it (the `ml_` passes read the
            // light map slots at its unwrap uv); light map 0 when it has
            // none. `BO2MP_IW7_MODEL_LIGHTMAPS=0`: neither (light map 0 at
            // uv 0, as before).
            let mut rows = ObjectRows::model(sm.origin, sm.axis, sm.scale, model_probe[k]);
            let mut model_lm = 0;
            if model_lightmaps {
                rows = rows.with_lightmap(sm.lightmap.0, sm.lightmap.1);
                model_lm = usize::try_from(sm.lightmap.2).ok().filter(|&i| i < b.lmaps.len()).unwrap_or(0);
            }
            let model_bounds = [Vec3::from(sm.origin) - Vec3::splat(r), Vec3::from(sm.origin) + Vec3::splat(r)];
            for &(off, first, count, material, vert0) in parts {
                if count == 0 {
                    continue;
                }
                let Some(mi) = material.filter(|&i| i < map.materials.len()) else {
                    skip(&mut b.report, "model surfaces without a material");
                    continue;
                };
                let m = &map.materials[mi];
                if dbg_skip.iter().any(|x| m.name.contains(x.as_str()) || m.technique_set.contains(x.as_str())) {
                    continue;
                }
                let Some(&(zone_i, set)) = sets.get(m.technique_set.as_str()) else {
                    skip(&mut b.report, "model surfaces whose technique set is not in the zones");
                    continue;
                };
                let Some((tname, pass, slot)) = draw_pass(set) else {
                    skip(&mut b.report, "model surfaces with no forward technique");
                    continue;
                };
                let lit = lighting_at[k];
                let pass_i = b.pass(PassInfo { zone: zone_i, pass: pass.clone(), base: Base::Model, wvf: 0, second_stride: if lit.is_some() { vertex::MODEL_LIGHTING_STRIDE } else { 0 } });
                let state = crate::state::of_material(m, slot);
                let Some(pipe) = b.pipe(pass_i, state) else {
                    skip(&mut b.report, "model surfaces of pipelines not made");
                    continue;
                };
                if std::env::var("IW7_DBG_MDRAW").is_ok_and(|v| map.models[sm.model].name.contains(v.as_str())) {
                    let list: Vec<String> = m.textures.iter().map(|t| format!("{:08x}:{}", t.0, t.3)).collect();
                    eprintln!("mdraw: {} material {} set {} tech {} lit {} lmap {model_lm} cb {:?} textures {}", map.models[sm.model].name, m.name, m.technique_set, tname.split('.').next().unwrap_or(""), lit.is_some(), m.cb_index.get(slot), list.join(" "));
                    eprintln!("mconst: {} -> {:?}", m.name, m.constants);
                    if let (Ok(dir), Some(w)) = (std::env::var("IW7_DBG_WGSL"), b.pipes[pipe].ps_wgsl.as_ref()) {
                        let _ = std::fs::write(format!("{dir}/mps_{}_{pipe}.wgsl", m.name.replace(['*', '/'], "_")), w);
                        if let Some(v) = b.pipes[pipe].vs_wgsl.as_ref() {
                            let _ = std::fs::write(format!("{dir}/mvs_{}_{pipe}.wgsl", m.name.replace(['*', '/'], "_")), v);
                        }
                    }
                    eprintln!("mconst: {} {:?}", m.name, m.constants);
                    let unknown: Vec<String> = pass.args.iter().enumerate().filter(|(_, a)| a.ty == bind::ARG_CODE_CONST && !known(a.index)).map(|(k, a)| format!("{}(stage {} rows {}..+{} -> {:?})", a.index, a.stage, a.first_row, a.row_count, bind::target(pass, k))).collect();
                    let code_tex: Vec<String> = pass.args.iter().filter(|a| a.ty != bind::ARG_CODE_CONST).map(|a| format!("ty{}:{}:{}", a.ty, a.index, a.stage)).collect();
                    eprintln!("munknown: {} code constants not fed: {}; other args {}", m.name, unknown.join(" "), code_tex.join(" "));
                }
                let group = b.group(pass_i, pipe, mi, slot, model_lm, Object::Model(k), &rows);
                let second = Some(match lit {
                    Some(at) => (1u8, at + vert0 * vertex::MODEL_LIGHTING_STRIDE),
                    None => (2u8, 0),
                });
                let order = draws.len();
                let discards = b.pipes[pipe].discards;
                if let Some(cutout) = caster(&mut b, &mut screen, &mut cutouts, m, tname, &state, discards) {
                    screen.occluders.push(crate::screen::Occluder { base: (1, off), index: (1, first, count), object: 1 + k as u32, bounds: model_bounds, cutout });
                }
                draws.push((Draw { pipe, group, base: (1, off), second, index: (1, first, count), behind: b.behind_groups.contains(&group), bounds: model_bounds, cull: sm.cull_dist.max(0.0) }, state.blend_state().is_some(), m.sort_key, order));
                b.report.model_draws += 1;
            }
        }
        screen.sort_occluders();
        // Opaque first, batched by pipeline, then bind group and stream
        // (`BO2MP_IW7_BATCH=0`: as listed), then blended by sort key as
        // listed. (Those reading the scene behind last, after its copy.)
        let compile = std::time::Instant::now();
        let ok: Vec<bool> = b.pipes.iter_mut().map(Pipe::finish).collect();
        draws.retain(|d| ok[d.0.pipe]);
        b.report.seconds.pipelines += compile.elapsed().as_secs_f32();
        let batch = !std::env::var("BO2MP_IW7_BATCH").is_ok_and(|v| v.trim() == "0");
        draws.sort_by_key(|d| {
            let opaque = batch && !d.1;
            (d.0.behind, d.1, if d.1 { d.2 } else { 0 }, if opaque { d.0.pipe } else { 0 }, if opaque { d.0.group } else { 0 }, if opaque { d.0.base.0 } else { 0 }, d.3)
        });
        let mut report = std::mem::take(&mut b.report);
        report.seconds.walks = walks_took;
        report.seconds.total = started.elapsed().as_secs_f32();
        report.pipelines = b.pipes.len();
        report.pictures = b.pictures.loaded;
        report.pictures_missing = b.pictures.missing;
        let (pipes, groups, camera_buffers) = (std::mem::take(&mut b.pipes), std::mem::take(&mut b.groups), std::mem::take(&mut b.camera_buffers));
        let arena = std::mem::replace(&mut b.arena, ConstArena::new(&g));
        // The frame's code constants, each worked out once a frame.
        let frame_codes: Vec<u16> = b.frame_codes.iter().copied().collect();
        let camera_buffers: Vec<CameraBuffer> = camera_buffers
            .into_iter()
            .map(|mut c| {
                for w in &mut c.writes {
                    w.1 = frame_codes.iter().position(|&k| k == w.1).unwrap_or(0) as u16;
                }
                c
            })
            .collect();
        drop(b);
        let vertex_buffers = [init("iw7 world vb", &map.raw_vertices, wgpu::BufferUsages::VERTEX), init("iw7 model vb", &model_vb, wgpu::BufferUsages::VERTEX)];
        let second_buffers = [
            init("iw7 layers", &map.layer_data, wgpu::BufferUsages::VERTEX),
            init("iw7 model lighting", &lighting, wgpu::BufferUsages::VERTEX),
            init("iw7 zeros", &[0u8; 64], wgpu::BufferUsages::VERTEX),
        ];
        let index_buffers = [init("iw7 world ib", bytemuck::cast_slice(&map.indices), wgpu::BufferUsages::INDEX), init("iw7 model ib", bytemuck::cast_slice(&model_ib), wgpu::BufferUsages::INDEX)];
        let vision = map.vision_params();
        let clut = map.colour_table(&vision);
        let sky_intensity = vision.sky_intensity;
        let mut post = crate::post::Post::new(&g, vision, clut.as_deref(), wgpu::TextureFormat::Rgba8Unorm);
        if let Some((_, text)) = map.vision.as_ref() {
            post.look = crate::post::Look::from_vision(text);
        }
        let timer = if crate::profile::on() || std::env::var("IW7_SHOT_PROFILE").is_ok() { crate::profile::GpuTimer::new(&g.device, &g.queue) } else { None };
        let blur = crate::blur::MotionBlur::new(&g);
        let gpu_for_dof = g.clone();
        let mut host = Self {
            gpu: g,
            vertex_buffers,
            second_buffers,
            index_buffers,
            pipes,
            groups,
            camera_buffers,
            frame_rows: vec![None; frame_codes.len()],
            frame_codes,
            arena,
            draws: draws.into_iter().map(|d| d.0).collect(),
            sun,
            fog,
            sky_scale: if std::env::var("BO2MP_IW7_SKY_INTENSITY").is_ok_and(|v| v == "0") { 1.0 } else { sky_intensity },
            targets: None,
            post,
            screen,
            behind,
            reflections,
            stats: Default::default(),
            profiling: crate::profile::on(),
            average: Default::default(),
            timer,
            timer_slot: None,
            blur,
            post_input: 0,
            dof: crate::dof::DepthOfField::new(&gpu_for_dof),
            dof_frame: Default::default(),
            fixed_dt: None,
            quality,
            draw_distance: 1.0,
            frame_eye: Vec3::ZERO,
            quality_line: String::new(),
            started: std::time::Instant::now(),
            spots,
            volumetric,
            clusters,
            post_for: 0,
            post_built: 0,
            last_frame: None,
            report,
        };
        let applied = host.apply_quality(quality);
        host.quality_line = applied;
        Ok(host)
    }

    fn targets(&mut self, width: u32, height: u32) {
        if self.targets.as_ref().is_some_and(|t| (t.width, t.height) == (width, height)) {
            return;
        }
        let g = &self.gpu;
        let mk = |format, label, usage| {
            g.device.create_texture(&wgpu::TextureDescriptor {
                label: Some(label),
                size: wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format,
                usage,
                view_formats: &[],
            })
        };
        let all = wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_SRC;
        let colour = mk(wgpu::TextureFormat::Rgba16Float, "iw7 colour", all);
        let depth = mk(wgpu::TextureFormat::Depth32Float, "iw7 depth", all);
        let fin = mk(wgpu::TextureFormat::Rgba8Unorm, "iw7 final", all);
        let colour_view = colour.create_view(&Default::default());
        let depth_view = depth.create_view(&Default::default());
        let final_view = fin.create_view(&Default::default());
        let blurred_view = mk(wgpu::TextureFormat::Rgba16Float, "iw7 motion blurred", all).create_view(&Default::default());
        let dof_view = mk(wgpu::TextureFormat::Rgba16Float, "iw7 depth of field", all).create_view(&Default::default());
        self.targets = Some(Targets { width, height, colour, colour_view, depth_view, final_view, blurred_view, dof_view, final_tex: fin });
        self.post_for += 1;
    }

    /// Writes the camera's constants and records the world, then the
    /// display pass. Depth is reversed (1 near, 0 far).
    pub fn record(&mut self, enc: &mut wgpu::CommandEncoder, cam: &Camera) {
        self.targets(cam.width, cam.height);
        let t = self.targets.take().expect("made");
        self.record_into(enc, cam, &t.colour_view, &t.depth_view, true);
        // The display: metered exposure, the vision's tone map and table.
        let now = std::time::Instant::now();
        let dt = self.fixed_dt.unwrap_or_else(|| self.last_frame.map_or(0.0, |l| now.duration_since(l).as_secs_f32().min(1.0)));
        self.last_frame = Some(now);
        let t0 = std::time::Instant::now();
        // Depth of field (aiming), then motion blur, over the HDR colour,
        // before the display reads it.
        let dof = self.quality.depth_of_field && self.dof.allowed && self.dof_frame.active();
        if dof {
            self.dof.record(enc, &t.colour_view, &t.depth_view, &t.dof_view, cam.view_projection, cam.forward, t.height, &self.dof_frame);
            self.stats.passes += 1;
        }
        let src = if dof { &t.dof_view } else { &t.colour_view };
        let blurred = if self.quality.motion_blur && self.blur.allowed {
            self.blur.record(enc, src, &t.depth_view, &t.blurred_view, cam.view_projection, cam.eye, dt)
        } else {
            self.blur.skip(cam.view_projection, cam.eye);
            false
        };
        if blurred {
            self.stats.passes += 1;
        }
        let input = u8::from(dof) | (u8::from(blurred) << 1);
        if input != self.post_input {
            // (The display reads another view: its groups again.)
            self.post_input = input;
            self.post_for += 1;
        }
        let rebuild = self.post_built != self.post_for;
        self.post_built = self.post_for;
        let hdr = if blurred { &t.blurred_view } else { src };
        self.stats.passes += self.post.record(enc, hdr, &t.final_view, t.width, t.height, dt, rebuild);
        self.stats.cpu_ms[5] = t0.elapsed().as_secs_f32() * 1000.0;
        self.targets = Some(t);
        self.end_frame(enc);
    }

    /// Applies the graphics quality: the sun cascades (on/off and size),
    /// ambient occlusion, the local lights and their shadows and the placed
    /// models' draw distance take effect on the next frame; the pictures'
    /// size cap and anisotropy were read as the scene was made, so a change
    /// there waits for the next map. Returns the log line.
    pub fn apply_quality(&mut self, q: native_quality::Quality) -> String {
        let off = |k: &str| std::env::var(k).is_ok_and(|v| v.trim() == "0");
        self.screen.shadows_on = q.sun_shadow && !off("BO2MP_IW7_SHADOWS");
        // (The shared value is one cascade's side at ULTRA 1024; ours is
        // 2048 at full quality, so twice it.)
        let size = (q.sun_shadow_res * 2).clamp(256, crate::screen::SHADOW_SIZE);
        if size != self.screen.shadow_size {
            self.screen.set_cascade_size(&self.gpu, size);
            self.volumetric.rebind(self.screen.sun_views());
        }
        self.screen.ao_on = q.ambient_occlusion && !off("BO2MP_IW7_SSAO") && self.screen.ao_strength > 0.0;
        self.clusters.enabled = q.local_lights;
        self.clusters.shadows_on = q.spot_shadows > 0;
        self.draw_distance = q.draw_distance.max(0.05);
        let reload = self.quality.needs_reload(&q);
        self.quality = q;
        format!(
            "iw7 quality: {}; sun cascades {} x {}, spot shadows {}, motion blur {}{}",
            q.summary(),
            if self.screen.shadows_on { "on" } else { "off" },
            self.screen.shadow_size,
            if self.clusters.shadows_on && self.clusters.enabled { "on (all the map allows)" } else { "off" },
            if q.motion_blur && self.blur.allowed { "on" } else { "off" },
            if reload { "; the texture size and anisotropy change waits for the next map" } else { "" }
        )
    }

    /// Closes the frame's GPU timing (after the post passes, or after
    /// [`Host::record_into`] alone) and logs its costs every
    /// [`PROFILE_EVERY`] frames when profiling.
    pub fn end_frame(&mut self, enc: &mut wgpu::CommandEncoder) {
        if let (Some(timer), Some(slot)) = (self.timer.as_mut(), self.timer_slot.take()) {
            timer.stamp(enc, slot, 4);
            timer.end(enc, slot);
        }
        self.stats.gpu_ms = self.timer.as_ref().and_then(|t| t.latest);
        if self.profiling {
            self.average.add(&self.stats);
            if self.average.frames() >= PROFILE_EVERY {
                eprintln!("iw7 profile ({} frames): {}", self.average.frames(), self.average.mean().line());
                self.average = crate::profile::Average::default();
            }
        }
    }

    /// Writes the camera's constants and records the draws into `colour`
    /// (`Rgba16Float`) and `depth` (`Depth32Float`, reversed), cleared
    /// first when `clear`.
    pub fn record_into(&mut self, enc: &mut wgpu::CommandEncoder, cam: &Camera, colour: &wgpu::TextureView, depth: &wgpu::TextureView, clear: bool) {
        let ms = |t: std::time::Instant| t.elapsed().as_secs_f32() * 1000.0;
        self.stats = crate::profile::FrameStats { draws_total: self.draws.len() as u32, occluders_total: self.screen.occluders.len() as u32, ..Default::default() };
        self.timer_slot = self.timer.as_mut().and_then(crate::profile::GpuTimer::begin);
        if let (Some(timer), Some(slot)) = (self.timer.as_ref(), self.timer_slot) {
            timer.stamp(enc, slot, 0);
        }
        let t0 = std::time::Instant::now();
        self.stats.cpu_ms[0] = ms(t0);
        // The screen maps the lit programs read (depth pre-pass, sun
        // cascades, shadow mask, ambient occlusion).
        let t0 = std::time::Instant::now();
        self.stats.bytes_uploaded += self.clusters.update(&self.gpu, cam.view_projection, cam.eye, cam.forward, cam.width, cam.height, self.spots.records(cam.eye));
        self.stats.cpu_ms[1] = ms(t0);
        let t0 = std::time::Instant::now();
        self.spots.record(enc, &self.screen, &self.vertex_buffers, &self.index_buffers);
        let toward = self.sun.map(|s| Vec3::from(s.toward));
        let (bytes, passes, occluders) = self.screen.record(enc, cam.view_projection, cam.eye, toward, &self.vertex_buffers, &self.index_buffers);
        self.stats.bytes_uploaded += bytes;
        self.stats.passes += passes;
        self.stats.occluders = occluders;
        if let (Some(timer), Some(slot)) = (self.timer.as_ref(), self.timer_slot) {
            timer.stamp(enc, slot, 1);
        }
        let cascades = self.screen.cascades(cam.eye, toward.filter(|_| self.screen.shadows_on));
        self.stats.bytes_uploaded += self.volumetric.record(enc, cam.view_projection, cam.eye, cam.forward, self.sun.map(|s| (Vec3::from(s.toward), s.colour)), cascades, self.screen.shadows_on && toward.is_some());
        self.stats.passes += 1;
        if let (Some(timer), Some(slot)) = (self.timer.as_ref(), self.timer_slot) {
            timer.stamp(enc, slot, 2);
        }
        self.stats.cpu_ms[3] = ms(t0);
        let t0 = std::time::Instant::now();
        let time = self.started.elapsed().as_secs_f32();
        let reflections_on = self.reflections.on();
        for (k, &code) in self.frame_codes.iter().enumerate() {
            self.frame_rows[k] = match code {
                CODE_TIME => Some(Rows::one([time; 4])),
                CODE_PROBE_BLEND => Some(Rows::one(if reflections_on { crate::reflect::BLEND } else { [0.0; 4] })),
                CODE_PROBE_CLAMP => Some(Rows::one(if reflections_on { crate::reflect::MATCH_CLAMP } else { [0.0; 4] })),
                _ => code_rows(code, cam, self.sun, self.fog, self.clusters.words, self.sky_scale),
            };
        }
        for b in &self.camera_buffers {
            let mirror = &mut self.arena.chunks[b.chunk].1;
            for &(reg, code, first, rows) in &b.writes {
                let Some(v) = self.frame_rows[usize::from(code)] else { continue };
                for r in 0..rows.min(b.registers.saturating_sub(reg)) {
                    let at = b.offset + (reg + r) * 16;
                    if let Some(dst) = mirror.get_mut(at..at + 16) {
                        dst.copy_from_slice(bytemuck::cast_slice(&v.get(first + r)));
                    }
                }
            }
        }
        self.stats.bytes_uploaded += self.arena.upload(&self.gpu);
        self.stats.cpu_ms[2] = ms(t0);
        let t0 = std::time::Instant::now();
        self.frame_eye = cam.eye;
        let split = self.draws.iter().position(|d| d.behind).unwrap_or(self.draws.len());
        let view = crate::cull::on().then(|| crate::cull::Frustum::new(cam.view_projection, cam.eye));
        self.draw_range(enc, 0..split, colour, depth, clear, view.as_ref());
        if split < self.draws.len() {
            // (The distortion surfaces bend the scene drawn so far.)
            self.behind.record(&self.gpu, enc, colour);
            self.stats.passes += 1;
            self.draw_range(enc, split..self.draws.len(), colour, depth, false, view.as_ref());
        }
        if let (Some(timer), Some(slot)) = (self.timer.as_ref(), self.timer_slot) {
            timer.stamp(enc, slot, 3);
        }
        self.stats.cpu_ms[4] = ms(t0);
    }

    #[allow(clippy::too_many_arguments)]
    fn draw_range(&mut self, enc: &mut wgpu::CommandEncoder, range: std::ops::Range<usize>, colour: &wgpu::TextureView, depth: &wgpu::TextureView, clear: bool, view: Option<&crate::cull::Frustum>) {
        let mut rp = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("iw7 world"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: colour,
                resolve_target: None,
                depth_slice: None,
                ops: wgpu::Operations { load: if clear { wgpu::LoadOp::Clear(wgpu::Color::BLACK) } else { wgpu::LoadOp::Load }, store: wgpu::StoreOp::Store },
            })],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: depth,
                depth_ops: Some(wgpu::Operations { load: if clear { wgpu::LoadOp::Clear(0.0) } else { wgpu::LoadOp::Load }, store: wgpu::StoreOp::Store }),
                stencil_ops: None,
            }),
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        rp.set_vertex_buffer(2, self.second_buffers[2].slice(..));
        let (mut pipe, mut group, mut ib) = (usize::MAX, usize::MAX, u8::MAX);
        let (mut pipe_switches, mut group_switches) = (0u32, 0u32);
        let mut n = 0u32;
        let eye = self.frame_eye;
        for d in &self.draws[range] {
            if view.is_some_and(|f| !f.sees(d.bounds[0], d.bounds[1])) {
                continue;
            }
            if d.cull > 0.0 && ((d.bounds[0] + d.bounds[1]) * 0.5).distance(eye) > d.cull * self.draw_distance {
                continue;
            }
            n += 1;
            if d.pipe != pipe {
                pipe_switches += 1;
                pipe = d.pipe;
                rp.set_pipeline(self.pipes[pipe].pipeline.as_ref().expect("compiled"));
                group = usize::MAX;
            }
            if d.group != group {
                group_switches += 1;
                group = d.group;
                for (i, bg) in self.groups[group].iter().enumerate() {
                    rp.set_bind_group(i as u32, bg, &[]);
                }
            }
            if d.index.0 != ib {
                ib = d.index.0;
                rp.set_index_buffer(self.index_buffers[usize::from(ib)].slice(..), wgpu::IndexFormat::Uint16);
            }
            rp.set_vertex_buffer(0, self.vertex_buffers[usize::from(d.base.0)].slice(d.base.1..));
            match d.second {
                Some((b, off)) => rp.set_vertex_buffer(1, self.second_buffers[usize::from(b)].slice(off..)),
                None => rp.set_vertex_buffer(1, self.second_buffers[2].slice(..)),
            }
            rp.draw_indexed(d.index.1..d.index.1 + d.index.2, 0, 0..1);
        }
        drop(rp);
        self.stats.passes += 1;
        self.stats.draws += n;
        self.stats.pipeline_switches += pipe_switches;
        self.stats.group_switches += group_switches;
    }

    /// The screen maps (`screen`): the sun shadow mask, the ambient
    /// occlusion (both `Rgba16Float`), for looking at.
    pub fn screen_maps(&self) -> [(&wgpu::Texture, u32, u32); 2] {
        let s = &self.screen;
        [(&s.shadow.0, s.width, s.height), (&s.ao.0, (s.width / 2).max(1), (s.height / 2).max(1))]
    }

    /// The last frame's display-encoded image (`Rgba8Unorm`).
    /// The last frame's display image's texture (for reading back).
    pub fn final_texture(&self) -> Option<&wgpu::Texture> {
        self.targets.as_ref().map(|t| &t.final_tex)
    }

    pub fn final_image(&self) -> Option<&wgpu::TextureView> {
        self.targets.as_ref().map(|t| &t.final_view)
    }

    /// The last frame's depth (reversed: 1 near, 0 far).
    pub fn scene_depth(&self) -> Option<&wgpu::TextureView> {
        self.targets.as_ref().map(|t| &t.depth_view)
    }

    /// The last frame's HDR colour, read back (linear).
    pub fn read_hdr(&self) -> Option<Vec<[f32; 4]>> {
        let t = self.targets.as_ref()?;
        Some(render_h1::world::read_rgba16f(&self.gpu, &t.colour, t.width, t.height))
    }
}

/// Draws `map`'s world offscreen and reads back its HDR colour (linear).
pub fn render(g: &Gpu, map: &Iw7Map, techsets: &Zone, extra: &[&Zone], cam: &Camera) -> Result<(Vec<[f32; 4]>, Report), String> {
    render_within(g, map, techsets, extra, cam, &|_, _| true)
}

/// As [`render`], with only what `keep` takes (see [`Host::new_within`]).
pub fn render_within(g: &Gpu, map: &Iw7Map, techsets: &Zone, extra: &[&Zone], cam: &Camera, keep: &dyn Fn([f32; 3], [f32; 3]) -> bool) -> Result<(Vec<[f32; 4]>, Report), String> {
    let mut host = Host::new_within(g.clone(), map, techsets, extra, keep)?;
    let mut enc = g.device.create_command_encoder(&Default::default());
    host.record(&mut enc, cam);
    g.queue.submit([enc.finish()]);
    let _ = g.device.poll(wgpu::PollType::wait_indefinitely());
    let px = host.read_hdr().ok_or("no frame")?;
    Ok((px, std::mem::take(&mut host.report)))
}

/// HDR to an 8-bit sRGB picture for looking at: exposure from the median
/// lit pixel (to 0.18), then Reinhard. Not the game's tone map.
pub fn to_srgb8(px: &[[f32; 4]]) -> Vec<u8> {
    let k = auto_exposure(px);
    let enc = |c: f32| {
        let c = if c.is_finite() { (c * k).max(0.0) } else { 0.0 };
        let c = c / (1.0 + c);
        let s = if c <= 0.003_130_8 { 12.92 * c } else { 1.055 * c.powf(1.0 / 2.4) - 0.055 };
        (s * 255.0).round().clamp(0.0, 255.0) as u8
    };
    px.iter().flat_map(|p| [enc(p[0]), enc(p[1]), enc(p[2]), 255]).collect()
}

/// The exposure that puts the median lit pixel at 0.18.
pub fn auto_exposure(px: &[[f32; 4]]) -> f32 {
    let mut lum: Vec<f32> = px.iter().map(|p| 0.2126 * p[0] + 0.7152 * p[1] + 0.0722 * p[2]).filter(|l| l.is_finite() && *l > 1e-4).collect();
    lum.sort_by(f32::total_cmp);
    let mid = lum.get(lum.len() / 2).copied().unwrap_or(1.0).max(1e-4);
    0.18 / mid
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fog_halves_the_light_at_its_half_distance() {
        let f = Fog { start: 100.0, half: 1000.0, opacity: 0.8, colour: [1.0; 3], sun_colour: [1.0; 3], sun_dir: [0.0, 0.0, 1.0], sun_begin_deg: 0.0, sun_end_deg: 90.0 };
        let (c210, c212) = (f.code(210).unwrap(), f.code(212).unwrap());
        let t = |d: f32| (d * c212[3] + c210[3]).exp().max(c210[1]).min(1.0);
        assert!((t(1100.0) - 0.5).abs() < 1e-4);
        assert_eq!(t(50.0), 1.0);
        assert!((t(1.0e6) - 0.2).abs() < 1e-4);
    }

    #[test]
    fn a_model_places_its_vertices_and_takes_one_probe() {
        let r = ObjectRows::model([10.0, 20.0, 30.0], [[0.0, 1.0, 0.0], [-1.0, 0.0, 0.0], [0.0, 0.0, 1.0]], 2.0, Some(7));
        // Local x (forward) is world +y, times the scale, plus the origin.
        let p = [1.0f32, 0.0, 0.0, 1.0];
        let w: Vec<f32> = (0..3).map(|i| (0..4).map(|k| r.world[i][k] * p[k]).sum()).collect();
        assert_eq!(w, vec![10.0, 22.0, 30.0]);
        assert_eq!(r.probes[1], 7.0);
        assert_eq!(r.probes[2].to_bits() & 15, 1);
    }

    #[test]
    fn the_camera_constants_follow_the_shader() {
        let cam = Camera::looking(Vec3::new(100.0, 0.0, 50.0), Vec3::X, 65.0, 640, 360);
        let world = ObjectRows::world().code(CODE_WORLD_MATRIX).unwrap();
        let eye = code_rows(CODE_EYE, &cam, None, None, 1, 1.0).unwrap()[0];
        // The vertex program: rows' w minus the eye, dotted with (p, 1).
        let p = [300.0f32, 0.0, 50.0, 1.0];
        let rel: Vec<f32> = (0..3).map(|r| world[r][0] * p[0] + world[r][1] * p[1] + world[r][2] * p[2] + (world[r][3] - eye[r])).collect();
        assert_eq!(rel, vec![200.0, 0.0, 0.0]);
        let vp = code_rows(CODE_VIEW_PROJECTION, &cam, None, None, 1, 1.0).unwrap();
        let clip: Vec<f32> = vp.r[..vp.n].iter().map(|row| row[0] * rel[0] + row[1] * rel[1] + row[2] * rel[2] + row[3]).collect();
        // Straight ahead: the centre of the screen, in front.
        assert!(clip[0].abs() < 1e-3 && clip[1].abs() < 1e-3);
        assert!(clip[2] > 0.0 && clip[2] < clip[3] && clip[3] > 0.0);
        let size = code_rows(CODE_TARGET_SIZE, &cam, None, None, 1, 1.0).unwrap()[0];
        assert_eq!(size, [640.0, 360.0, 1.0 / 640.0, 1.0 / 360.0]);
    }

    /// The vertex program's decode of the lighting word's normal.
    fn decode(word: u32) -> [f32; 3] {
        let (a, b) = ((word & 511) as f32 / 511.0 * 2.0 - 1.0, (word >> 9 & 511) as f32 / 511.0 * 2.0 - 1.0);
        let z = 1.0 - a.abs() - b.abs();
        let (x, y) = if z < 0.0 { ((1.0 - b.abs()) * if a >= 0.0 { 1.0 } else { -1.0 }, (1.0 - a.abs()) * if b >= 0.0 { 1.0 } else { -1.0 }) } else { (a, b) };
        let l = (x * x + y * y + z * z).sqrt();
        [x / l, y / l, z / l]
    }

    #[test]
    fn lighting_word_round_trips_normals() {
        let pack = |n: [f32; 3]| (0..3).fold(0u32, |w, i| w | (((n[i] * 0.5 + 0.5) * 1023.0).round() as u32) << (10 * i));
        for n in [[0.0f32, 0.0, 1.0], [0.0, 0.0, -1.0], [1.0, 0.0, 0.0], [0.577, -0.577, -0.577], [-0.3, 0.5, 0.81], [0.2, -0.9, -0.38]] {
            let l = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
            let n = [n[0] / l, n[1] / l, n[2] / l];
            let w = octahedral_word(pack(n));
            assert_eq!(w >> 18, 127, "full probe scale, no darkening");
            let d = decode(w);
            let dot = n[0] * d[0] + n[1] * d[1] + n[2] * d[2];
            assert!(dot > 0.998, "{n:?} came back as {d:?}");
        }
    }
}

/// The probe lookup word of a vertex from its packed normal
/// (`Unorm10_10_10_2`, model space): the low nine bits and the next nine are
/// the normal, octahedral (the vertex program's decode inverted); the next
/// seven scale the probe light (127: all of it, zero blacks the vertex out);
/// the top seven darken it (zero: not at all).
fn octahedral_word(packed: u32) -> u32 {
    let c = |shift: u32| ((packed >> shift) & 1023) as f32 / 1023.0 * 2.0 - 1.0;
    let (x, y, z) = (c(0), c(10), c(20));
    let l = x.abs() + y.abs() + z.abs();
    if l < 1e-6 {
        return 0;
    }
    let (mut u, mut v) = (x / l, y / l);
    if z < 0.0 {
        let (su, sv) = (if u >= 0.0 { 1.0 } else { -1.0 }, if v >= 0.0 { 1.0 } else { -1.0 });
        (u, v) = ((1.0 - v.abs()) * su, (1.0 - u.abs()) * sv);
    }
    let q = |t: f32| (((t * 0.5 + 0.5) * 511.0).round().clamp(0.0, 511.0)) as u32;
    q(u) | q(v) << 9 | 127 << 18
}
