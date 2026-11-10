//! A Modern Warfare Remastered map's world, collision and entities as plain
//! data, from what the exact walk kept (`walk::GfxWorldRec`, `ClipMapRec`,
//! `MapEntsRec`, `ComWorldRec`). The public API mirrors
//! `fastfile_iw6::map` (H1's world descends from Ghosts') so the game side
//! can read both the same way; where H1's data differs it says so here.
//!
//! * World vertex: Ghosts' 44-byte layout (position, binormal sign,
//!   colour, uv, lightmap uv, packed normal and tangent; unit vectors
//!   unsigned 10:10:10 over 1023: their mean length is 1.000 that way on
//!   mp_rust and mp_courtyard_ss, 1.2-1.3 read signed).
//! * Surfaces: H1 groups the world's vertices in blocks of at most 65536:
//!   every surface of a block has the block's `firstVertex` and
//!   `vertexCount` (mp_rust: five blocks), and its indices
//!   (`baseIndex .. + 3 * triCount`) count from `firstVertex`. Ghosts gave
//!   each surface its own range. The indices are the same either way:
//!   `vertex = first_vertex + index`.
//! * Static models: placement and model in the draw instance (80 bytes);
//!   lighting is a separate union (`GfxStaticModelLighting`), its ground
//!   lighting four half floats (Ghosts: four floats).
//! * Collision: H1 moved the terrain triangles, partitions, AABB trees and
//!   static model placements into the `ClipInfo`; brush sides name their
//!   plane by index (Ghosts: by pointer).
//! * Entities: usermaps (and H1) store entity keys by name, not by
//!   number, and keep their spawn points both in the entity text and in
//!   `MapEnts::spawnList`; [`IwMap::entity_string`] writes a spawn record as
//!   an entity only when the text does not already have it.
//! * Lightmaps: primary `R8_UNORM` (Ghosts: BC4), secondary
//!   `R16G16B16A16_FLOAT` two pages stacked, as Ghosts'.

use std::collections::HashMap;

use crate::layout as L;
use crate::walk::{AssetId, Capture, ClipMapRec, GfxWorldRec};

#[derive(Clone, Copy, Debug, Default)]
pub struct WorldVertex {
    pub xyz: [f32; 3],
    pub binormal_sign: f32,
    /// D3DCOLOR bytes as stored (B, G, R, A).
    pub color: u32,
    pub tex_coord: [f32; 2],
    pub lmap_coord: [f32; 2],
    pub normal: [f32; 3],
    pub tangent: [f32; 3],
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Surface {
    /// The surface's vertex block (shared by every surface of the block);
    /// its indices count from here.
    pub first_vertex: u32,
    /// Byte offset of its first vertex's layer data (`World::layer_bytes`).
    pub vertex_layer_data: u32,
    pub vertex_count: u16,
    pub tri_count: u16,
    pub base_index: u32,
    pub material: Option<AssetId>,
    pub lightmap_index: u8,
    pub reflection_probe_index: u8,
    pub primary_light_env: u8,
    pub flags: u8,
    pub mins: [f32; 3],
    pub maxs: [f32; 3],
}

#[derive(Clone, Copy, Debug, Default)]
pub struct StaticModel {
    pub model: Option<AssetId>,
    pub origin: [f32; 3],
    /// Rows: the model's forward, left and up axes in world space.
    pub axis: [[f32; 3]; 3],
    pub scale: f32,
    pub cull_dist: f32,
    /// `StaticModelFlag`: 0x20 ground lit, 0x40 light grid, 0x80 vertex
    /// lit, 0x100 model lightmap.
    pub flags: u16,
    /// Linear light where the model stands (`groundLighting`, from half
    /// floats); all zero unless the model is ground lit (flag 0x20).
    pub ground_lighting: [f32; 4],
    pub reflection_probe_index: u8,
    pub primary_light_env: u16,
    /// Its light grid colour (`ambientLightingInfo.colorIndex`, into
    /// `World::light_grid_colors`) when it is lit by the grid (flag 0x40).
    pub grid_color: Option<u32>,
    /// Lit by the grid (flag 0x40): the weight of the primary light (the
    /// sun) at the model, baked (`ambientLightingInfo.primaryLightWeight`).
    pub primary_light_weight: Option<f32>,
    /// Model lightmap (flag 0x100): `modelLightmapInfo` offset, scale and
    /// lightmap page.
    pub model_lightmap: Option<ModelLightmap>,
    /// Vertex-lit placements (flag 0x80): one `GfxStaticModelVertexLighting`
    /// (20 bytes: visibility, ambient colour as four halves, highlight
    /// colour as four halves) per vertex of the model, as stored: (file
    /// position in the zone image, length).
    pub vertex_lighting: Option<(usize, usize)>,
}

/// Where a model-lightmapped placement's unwrap lands on its page.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ModelLightmap {
    pub offset: [f32; 2],
    pub scale: [f32; 2],
    pub page: u32,
}

/// One light grid colour: 56 linear HDR samples, the shell of a 4x4x4
/// block (layers of 16, 12, 12 and 16) the game's model shaders sample
/// toward the normal.
pub type GridColor = [[f32; 3]; 56];

/// Call of Duty 4's row light grid as Modern Warfare Remastered keeps it
/// (the usermaps, MW2's own; retail maps use the tree, not read): points
/// every 32 x 32 x 64 units from -131072; each row (along `row_axis`) at
/// `rows[4 * row_start[row]]`: column start and count, z start and count
/// (u16), its first entry (u32), then runs of (columns, z count, and the z
/// start when the count is not 0) until the row's columns are spent or a
/// run of 0 columns; each column's entries are its z points in order.
#[derive(Clone, Debug, Default)]
pub struct LightGrid {
    pub mins: [u16; 3],
    pub row_axis: usize,
    pub col_axis: usize,
    pub row_start: Vec<u16>,
    pub rows: Vec<u8>,
    /// Each entry's colour (`GfxLightGridEntry::colorsIndex`).
    pub entry_colors: Vec<u32>,
}

impl LightGrid {
    /// The colour index at grid point `g`.
    pub fn colour_at(&self, g: [i64; 3]) -> Option<u32> {
        let r = g[self.row_axis] - i64::from(self.mins[self.row_axis]);
        let start = *self.row_start.get(usize::try_from(r).ok()?)?;
        if start == u16::MAX {
            return None;
        }
        let o = 4 * usize::from(start);
        let u16_at = |k: usize| {
            self.rows
                .get(o + k..o + k + 2)
                .map(|b| i64::from(u16::from_le_bytes([b[0], b[1]])))
        };
        let (cs, cc, zs, zc) = (u16_at(0)?, u16_at(2)?, u16_at(4)?, u16_at(6)?);
        let first = self
            .rows
            .get(o + 8..o + 12)
            .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))?;
        let (mut col, z) = (g[self.col_axis] - cs, g[2] - zs);
        if col < 0 || col >= cc || z < 0 || z >= zc {
            return None;
        }
        let (mut p, mut e) = (o + 12, i64::from(first));
        loop {
            let c = i64::from(*self.rows.get(p)?);
            if c == 0 {
                return None;
            }
            let n = i64::from(*self.rows.get(p + 1)?);
            let z0 = if n > 0 {
                i64::from(*self.rows.get(p + 2)?)
            } else {
                0
            };
            p += if n > 0 { 3 } else { 2 };
            if col < c {
                return (n > 0 && z >= z0 && z < z0 + n)
                    .then(|| {
                        self.entry_colors
                            .get(usize::try_from(e + col * n + (z - z0)).ok()?)
                            .copied()
                    })
                    .flatten();
            }
            col -= c;
            e += c * n;
        }
    }

    /// The light at `pos`: the eight grid points round it blended by
    /// distance (those with no entry left out).
    pub fn sample(&self, colours: &[GridColor], pos: [f32; 3]) -> Option<GridColor> {
        const SIZE: [f32; 3] = [32.0, 32.0, 64.0];
        let f: [f32; 3] = std::array::from_fn(|k| (pos[k] + 131072.0) / SIZE[k]);
        let base: [i64; 3] = std::array::from_fn(|k| f[k].floor() as i64);
        let mut out = [[0f32; 3]; 56];
        let mut total = 0f32;
        for corner in 0..8 {
            let g: [i64; 3] = std::array::from_fn(|k| base[k] + ((corner >> k) & 1) as i64);
            let w: f32 = (0..3)
                .map(|k| {
                    if (corner >> k) & 1 == 1 {
                        f[k] - base[k] as f32
                    } else {
                        1.0 - (f[k] - base[k] as f32)
                    }
                })
                .product();
            let Some(c) = self.colour_at(g).and_then(|i| colours.get(i as usize)) else {
                continue;
            };
            for t in 0..56 {
                for ch in 0..3 {
                    out[t][ch] += c[t][ch] * w;
                }
            }
            total += w;
        }
        (total > 1e-4).then(|| out.map(|t| t.map(|v| v / total)))
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct BrushModel {
    pub mins: [f32; 3],
    pub maxs: [f32; 3],
    pub start_surf: u32,
    pub surface_count: u32,
}

/// The drawn world.
#[derive(Clone, Debug, Default)]
pub struct World {
    pub name: String,
    pub vertices: Vec<WorldVertex>,
    /// The vertices as the zone stores them (`GfxWorldVertex`, 44 bytes
    /// each), for drawing with the game's own vertex shaders.
    pub vertex_bytes: Vec<u8>,
    /// The vertex layer stream (`draw.vld.data`): per-vertex data of
    /// layered surfaces, read by their shaders as a second vertex stream.
    pub layer_bytes: Vec<u8>,
    pub indices: Vec<u16>,
    pub surfaces: Vec<Surface>,
    /// Surface ranges [begin, end): lit opaque, lit decal, lit transparent,
    /// emissive.
    pub ranges: [(u32, u32); 4],
    pub static_models: Vec<StaticModel>,
    /// The light grid's colours (`GfxLightGrid::colors`).
    pub light_grid_colors: Vec<GridColor>,
    /// The row light grid, when the map has one.
    pub light_grid: Option<LightGrid>,
    /// The palette light grid (retail maps; `light_grid_palette`).
    pub light_grid_palette: Option<crate::light_grid_palette::Palette>,
    pub brush_models: Vec<BrushModel>,
    pub mins: [f32; 3],
    pub maxs: [f32; 3],
    /// `shadowBounds`: the box the sun's shadow map depth range is fitted
    /// to (the engine projects it on the sun axis each frame).
    pub shadow_mins: [f32; 3],
    pub shadow_maxs: [f32; 3],
    /// The sky surfaces (every sky's `skyStartSurfs`).
    pub sky_surfaces: Vec<u32>,
    pub sky_images: Vec<Option<AssetId>>,
    pub lightmaps: Vec<(Option<AssetId>, Option<AssetId>)>,
    /// Reflection probes: image and origin.
    pub reflection_probes: Vec<(Option<AssetId>, [f32; 3])>,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Plane {
    pub normal: [f32; 3],
    pub dist: f32,
}

#[derive(Clone, Debug, Default)]
pub struct Brush {
    pub mins: [f32; 3],
    pub maxs: [f32; 3],
    pub contents: i32,
    /// Its non-axial sides: plane index (into `Clip::planes`), material.
    pub sides: Vec<(u32, u16)>,
    /// Material per axial side: [min x, y, z], [max x, y, z].
    pub axial_materials: [[i16; 3]; 2],
}

#[derive(Clone, Copy, Debug, Default)]
pub struct AabbNode {
    pub origin: [f32; 3],
    pub half_size: [f32; 3],
    pub material_index: u16,
    pub child_count: u16,
    /// First child when `child_count > 0`, else a partition index.
    pub index: i32,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct CModel {
    pub mins: [f32; 3],
    pub maxs: [f32; 3],
    pub radius: f32,
    /// H1 widened these to 32 bits; kept as Ghosts' u16 when they fit.
    pub first_coll_aabb: u16,
    pub coll_aabb_count: u16,
    /// Index into `Clip::leaf_brush_nodes` (-1: none, or its own ClipInfo).
    pub leaf_brush_node: i32,
}

#[derive(Clone, Debug, Default)]
pub struct LeafBrushNode {
    pub leaf_brush_count: i16,
    pub brushes: Vec<u16>,
    pub child_offsets: [u16; 2],
}

/// A static model's collision placement (`cStaticModel_s`).
#[derive(Clone, Copy, Debug, Default)]
pub struct ClipStaticModel {
    pub model: Option<AssetId>,
    pub origin: [f32; 3],
    pub inv_scaled_axis: [[f32; 3]; 3],
    pub mins: [f32; 3],
    pub maxs: [f32; 3],
    /// The byte at +80 (the layout's `lightingHandle`): the game's model
    /// trace takes a placement only when it is 0 or equals a value of the
    /// running map it keeps beside the clip map (read; what sets that value
    /// was not followed).
    pub layer: u8,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct DynEnt {
    /// 1 clutter, 2 destructible (`DynEntityType`).
    pub kind: i32,
    pub quat: [f32; 4],
    pub origin: [f32; 3],
    pub model: Option<AssetId>,
    pub brush_model: u16,
    pub health: i32,
    pub contents: i32,
}

/// The map's clutter and destructible dynamic entities (`DynEntityDef`
/// types 1 and 2, with a model) as placed models in their starting pose:
/// the game draws them from its dynamic entity list, which no static model
/// placement covers (Shipment 2,007, Crash 630, none among the static
/// models). The pose quaternion is (x, y, z, w) (READ from the layout, the
/// handedness is INFERRED as the static models'). Their light is the
/// nearest static model's within `LIGHT_REACH` units (INFERRED: the engine
/// lights a dynamic entity from the light grid at run time, which is not
/// translated); a model with none near it takes none and draws as an
/// unlit-grid model. `BO2MP_H1_DYNENTS=0` leaves them out.
pub fn dyn_ent_models(clip: &Clip, statics: &[StaticModel]) -> Vec<StaticModel> {
    const LIGHT_REACH: f32 = 256.0;
    if std::env::var("BO2MP_H1_DYNENTS").is_ok_and(|v| v.trim() == "0") {
        return Vec::new();
    }
    let lit: Vec<&StaticModel> = statics.iter().filter(|m| m.grid_color.is_some()).collect();
    clip.dyn_ents
        .iter()
        .filter(|d| matches!(d.kind, 1 | 2) && d.model.is_some())
        .map(|d| {
            let [x, y, z, w] = d.quat;
            let n = (x * x + y * y + z * z + w * w).sqrt();
            let [x, y, z, w] = if n > 1e-6 { [x / n, y / n, z / n, w / n] } else { [0.0, 0.0, 0.0, 1.0] };
            // The rotation's columns: where the model's forward, left and
            // up axes point.
            let axis = [
                [1.0 - 2.0 * (y * y + z * z), 2.0 * (x * y + z * w), 2.0 * (x * z - y * w)],
                [2.0 * (x * y - z * w), 1.0 - 2.0 * (x * x + z * z), 2.0 * (y * z + x * w)],
                [2.0 * (x * z + y * w), 2.0 * (y * z - x * w), 1.0 - 2.0 * (x * x + y * y)],
            ];
            let near = lit
                .iter()
                .map(|m| {
                    let dx = [m.origin[0] - d.origin[0], m.origin[1] - d.origin[1], m.origin[2] - d.origin[2]];
                    (dx[0] * dx[0] + dx[1] * dx[1] + dx[2] * dx[2], *m)
                })
                .filter(|(d2, _)| *d2 <= LIGHT_REACH * LIGHT_REACH)
                .min_by(|a, b| a.0.total_cmp(&b.0))
                .map(|(_, m)| m);
            StaticModel {
                model: d.model,
                origin: d.origin,
                axis,
                scale: 1.0,
                cull_dist: 0.0,
                flags: 0,
                ground_lighting: [0.0; 4],
                reflection_probe_index: near.map_or(0, |m| m.reflection_probe_index),
                primary_light_env: near.map_or(0, |m| m.primary_light_env),
                grid_color: near.and_then(|m| m.grid_color),
                primary_light_weight: near.and_then(|m| m.primary_light_weight),
                model_lightmap: None,
                vertex_lighting: None,
            }
        })
        .collect()
}

/// The collision.
#[derive(Clone, Debug, Default)]
pub struct Clip {
    pub name: String,
    pub planes: Vec<Plane>,
    /// (name, surface flags, contents).
    pub materials: Vec<(String, i32, i32)>,
    pub brushes: Vec<Brush>,
    pub verts: Vec<[f32; 3]>,
    pub tri_indices: Vec<u16>,
    /// (triangle count, first triangle, first vertex segment: a
    /// partition's triangles index the vertices from `segment * 1024`).
    pub partitions: Vec<(u8, i32, u8)>,
    pub aabb_trees: Vec<AabbNode>,
    pub cmodels: Vec<CModel>,
    pub leaf_brush_nodes: Vec<LeafBrushNode>,
    /// The static models' collision placements.
    pub static_models: Vec<ClipStaticModel>,
    pub dyn_ents: Vec<DynEnt>,
    /// The placed scriptables (`scriptableMapEnts`), their defs by name in
    /// `Capture::scriptable_defs`.
    pub scriptables: Vec<crate::scriptable::Instance>,
    /// Sides whose plane was not found (should be 0).
    pub missing_planes: usize,
    /// The map's stages (`clipMap_t::stages`): stage 0 is the whole map,
    /// the others are volumes given by their trigger model.
    pub stages: Vec<Stage>,
    /// `clipMap_t::stageTrigger`'s models (hulls with their slabs).
    pub stage_triggers: Vec<Vec<TriggerHull>>,
}

/// A stage of the clip map (`Stage`, 32 bytes).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Stage {
    pub origin: [f32; 3],
    /// Model index into `Clip::stage_triggers`.
    pub trigger_index: u16,
    pub sun_primary_light_index: u8,
    /// The renderer's per-stage HDR scale (`HDR_STAGE_PARMS.x` is 1000
    /// times it).
    pub radiometric_scale: f32,
}

/// A spawn point of `MapEnts::spawnList`.
#[derive(Clone, Debug, Default)]
pub struct SpawnRecord {
    pub classname: String,
    pub targetname: String,
    pub target: String,
    pub script_noteworthy: String,
    pub origin: [f32; 3],
    pub angles: [f32; 3],
}

/// The sun (the ComWorld's first primary light when it is a sun).
#[derive(Clone, Copy, Debug, Default)]
pub struct Sun {
    /// Toward the sun.
    pub dir: [f32; 3],
    pub color: [f32; 3],
}

/// A `ComPrimaryLight` as the zone stores it (type 1 sun, 2 spot, 3 omni;
/// light 0 is the "no light" slot).
#[derive(Clone, Debug, Default)]
pub struct PrimaryLight {
    pub ty: u8,
    pub can_use_shadow_map: u8,
    pub physically_based: u8,
    pub exponent: u8,
    pub lighting_state: u8,
    pub color: [f32; 3],
    pub dir: [f32; 3],
    pub up: [f32; 3],
    pub origin: [f32; 3],
    pub fade_offset: [f32; 2],
    pub bulb_radius: f32,
    /// `bulbLength` and the two floats after it.
    pub bulb_length: [f32; 3],
    pub radius: f32,
    pub cos_half_fov_outer: f32,
    pub cos_half_fov_inner: f32,
    pub cos_half_fov_expanded: f32,
    pub rotation_limit: f32,
    pub translation_limit: f32,
    pub cuc_rotation_offset: f32,
    pub cuc_rotation_speed: f32,
    pub cuc_scroll: [f32; 2],
    pub cuc_scale: [f32; 2],
    pub cuc_trans: [f32; 2],
    pub def_name: String,
}

/// One hull of a trigger model: its box and the slabs that cut it.
#[derive(Clone, Debug, Default)]
pub struct TriggerHull {
    pub mid: [f32; 3],
    pub half: [f32; 3],
    /// (direction, mid point, half size).
    pub slabs: Vec<([f32; 3], f32, f32)>,
}

/// The entity keys retail H1 maps store as numbers (canonical script
/// strings, as Ghosts'; the zone tools' usermaps write names). Read off
/// `mp_shipment` and `mp_crash_snow` by their values: the numbers follow
/// the names' alphabetical order (`angles` 65 < `classname` 170 < `height`
/// 488 < `model` 668 < `origin` 738 < `radius` 849 < `script_label` 916 <
/// `script_linkname` 918 < `script_noteworthy` 920 < `spawnflags` 986 <
/// `suncolor` 1046 < `sundirection` 1047 < `sunlight` 1048 < `target` 1188
/// < `targetname` 1190), and the values say which is which (`classname`:
/// `worldspawn`, `script_model`; `model`: `?0`, `*3`, model names;
/// `targetname`: `minimap_corner`; `script_gameobjectname`: `dom ctf sd`;
/// `script_linkto`: space-separated `script_linkname`s). Keys not here are
/// written as `h1_<number>`.
///
/// Added 30 Oct from the multiplayer exe (docs/H1_NATIVE.md "Map logic"):
/// the spawn field table and the spawn functions ask for these keys by
/// number: `dmg` 293 (`trigger_hurt`'s damage; Shipment's and Crash's
/// kill volumes say 10000), `health` 486, `northyaw` 714 (worldspawn: its
/// sine and cosine in degrees), `wait` 1293 (a touch trigger fires once
/// when 0 or less), `cursorhint` 229 (compared with `HINT_INHERIT`),
/// `hintstring` 497; `script_delay` 914, `script_bombmode_original`
/// 31072 and `script_exploder` 31167 by their values and counts against
/// the usermap mp_courtyard_ss, which names them (the same prefabs: 4, 5
/// and 14 on every map).
pub const H1_KEYS: &[(u32, &str)] = &[
    (65, "angles"),
    (170, "classname"),
    (229, "cursorhint"),
    (293, "dmg"),
    (486, "health"),
    (488, "height"),
    (497, "hintstring"),
    (668, "model"),
    (714, "northyaw"),
    (738, "origin"),
    (849, "radius"),
    (914, "script_delay"),
    (916, "script_label"),
    (918, "script_linkname"),
    (920, "script_noteworthy"),
    (986, "spawnflags"),
    (1046, "suncolor"),
    (1047, "sundirection"),
    (1048, "sunlight"),
    (1188, "target"),
    (1190, "targetname"),
    (1293, "wait"),
    (1395, "_color"),
    (31072, "script_bombmode_original"),
    (31167, "script_exploder"),
    (31220, "script_gameobjectname"),
    (31270, "script_linkto"),
];

/// The name of a numbered entity key.
pub fn key_name(id: u32) -> Option<&'static str> {
    H1_KEYS.iter().find(|(k, _)| *k == id).map(|(_, n)| *n)
}

/// The entity text with its keys named. The usermaps' keys are already
/// names; a retail map's are numbers ([`H1_KEYS`]; one not known is kept
/// as `h1_<number>`), and key `0` is followed by a key of its own name
/// (`0 "node_name" "node_negotiation_begin_1"`).
pub fn named_entity_text(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + text.len() / 3);
    for line in text.lines() {
        let t = line.trim();
        match t.split_once(' ') {
            Some((key, value)) if !key.is_empty() && key.bytes().all(|b| b.is_ascii_digit()) => {
                let value = value.trim();
                match key.parse::<u32>() {
                    Ok(0) => out.push_str(value),
                    Ok(id) => match key_name(id) {
                        Some(name) => out.push_str(&format!("\"{name}\" {value}")),
                        None => out.push_str(&format!("\"h1_{id}\" {value}")),
                    },
                    Err(_) => out.push_str(&format!("\"h1_{key}\" {value}")),
                }
            }
            _ => out.push_str(t),
        }
        out.push('\n');
    }
    out
}

#[derive(Clone, Debug, Default)]
pub struct IwMap {
    pub world: Option<World>,
    pub clip: Option<Clip>,
    /// The trigger models (`"model" "?N"`), each its hulls.
    pub trigger_models: Vec<Vec<TriggerHull>>,
    /// The entity string as stored.
    pub entities: String,
    pub spawns: Vec<SpawnRecord>,
    pub sun: Option<Sun>,
    /// Every `ComPrimaryLight` (index 0 included).
    pub primary_lights: Vec<PrimaryLight>,
    /// Each `ComPrimaryLightEnv`: its light indices (up to four).
    pub primary_light_envs: Vec<Vec<u16>>,
    pub notes: Vec<String>,
}

/// The H1 name for the same type.
pub type H1Map = IwMap;

impl IwMap {
    /// Walks a zone and reads its map (`walk::walk_zone` + [`read_map`]).
    pub fn load(zone: &crate::Zone) -> (Self, Capture) {
        let (cap, stop) = crate::walk::walk_zone(zone);
        let mut map = read_map(&cap, &zone.image, &zone.assets.script_strings);
        if let Some(s) = stop {
            map.notes.insert(
                0,
                format!(
                    "h1 walk stopped at asset {} ({:?}): {}",
                    s.asset_index, s.asset_type, s.error
                ),
            );
        }
        (map, cap)
    }

    /// The entity string, keys named, with the spawn records the text does
    /// not already hold written in as entities (`classname` the record's
    /// name: `mp_tdm_spawn`, ...).
    pub fn entity_string(&self) -> String {
        let mut out = named_entity_text(self.entities.trim_end_matches('\0'));
        if !out.is_empty() && !out.ends_with('\n') {
            out.push('\n');
        }
        let have: std::collections::HashSet<(String, [i64; 3])> = entity_blocks(&out)
            .filter_map(|kv| {
                let c = kv.get("classname")?.to_string();
                let o = parse_vec3(kv.get("origin")?)?;
                Some((c, o.map(|v| (v * 8.0).round() as i64)))
            })
            .collect();
        for s in &self.spawns {
            if s.classname.is_empty()
                || have.contains(&(
                    s.classname.clone(),
                    s.origin.map(|v| (v * 8.0).round() as i64),
                ))
            {
                continue;
            }
            out.push_str("{\n");
            out.push_str(&format!("\"classname\" \"{}\"\n", s.classname));
            out.push_str(&format!(
                "\"origin\" \"{} {} {}\"\n",
                s.origin[0], s.origin[1], s.origin[2]
            ));
            out.push_str(&format!(
                "\"angles\" \"{} {} {}\"\n",
                s.angles[0], s.angles[1], s.angles[2]
            ));
            for (k, v) in [
                ("targetname", &s.targetname),
                ("target", &s.target),
                ("script_noteworthy", &s.script_noteworthy),
            ] {
                if !v.is_empty() {
                    out.push_str(&format!("\"{k}\" \"{v}\"\n"));
                }
            }
            out.push_str("}\n");
        }
        out
    }
}

/// The key/value pairs of each entity of a named entity text.
fn entity_blocks(text: &str) -> impl Iterator<Item = HashMap<&str, &str>> {
    text.split('{').skip(1).map(|block| {
        let mut kv = HashMap::new();
        for line in block.lines() {
            let mut parts = line.split('"').skip(1).step_by(2);
            if let (Some(k), Some(v)) = (parts.next(), parts.next()) {
                kv.insert(k, v);
            }
        }
        kv
    })
}

fn parse_vec3(s: &str) -> Option<[f32; 3]> {
    let mut it = s.split_whitespace().map(|x| x.parse::<f32>().ok());
    Some([it.next()??, it.next()??, it.next()??])
}

struct R<'a>(&'a [u8]);

impl R<'_> {
    fn u8(&self, at: usize) -> u8 {
        self.0.get(at).copied().unwrap_or(0)
    }
    fn u16(&self, at: usize) -> u16 {
        self.0
            .get(at..at + 2)
            .map_or(0, |b| u16::from_le_bytes([b[0], b[1]]))
    }
    fn i16(&self, at: usize) -> i16 {
        self.u16(at) as i16
    }
    fn u32(&self, at: usize) -> u32 {
        self.0
            .get(at..at + 4)
            .map_or(0, |b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }
    fn i32(&self, at: usize) -> i32 {
        self.u32(at) as i32
    }
    fn f32(&self, at: usize) -> f32 {
        f32::from_bits(self.u32(at))
    }
    fn vec3(&self, at: usize) -> [f32; 3] {
        [self.f32(at), self.f32(at + 4), self.f32(at + 8)]
    }
    /// `Bounds { midPoint, halfSize }` as (mins, maxs).
    fn bounds(&self, at: usize) -> ([f32; 3], [f32; 3]) {
        let (m, h) = (self.vec3(at), self.vec3(at + 12));
        (
            std::array::from_fn(|k| m[k] - h[k]),
            std::array::from_fn(|k| m[k] + h[k]),
        )
    }
    fn fits(&self, at: usize, len: usize) -> bool {
        at.checked_add(len).is_some_and(|end| end <= self.0.len())
    }
}

/// H1's packed unit vector (unsigned 10:10:10 over 1023, as Ghosts').
pub fn unpack_unit_vec(packed: u32) -> [f32; 3] {
    std::array::from_fn(|k| ((packed >> (10 * k)) & 0x3ff) as f32 / 1023.0 * 2.0 - 1.0)
}

/// The map in `cap` (its first GfxWorld, ClipMap, MapEnts and ComWorld).
pub fn read_map(cap: &Capture, image: &[u8], script_strings: &[Option<String>]) -> IwMap {
    let r = R(image);
    let mut out = IwMap::default();
    if let Some(g) = cap.gfx_worlds.first() {
        match read_world(&r, g) {
            Ok(w) => out.world = Some(w),
            Err(e) => out.notes.push(format!("h1 world: {e}")),
        }
    } else {
        out.notes.push("h1 world: the zone has no GfxWorld".into());
    }
    if let Some(c) = cap.clip_maps.first() {
        let clip = read_clip(&r, c, cap);
        if clip.missing_planes > 0 {
            out.notes.push(format!(
                "h1 clip: {} brush sides without their plane",
                clip.missing_planes
            ));
        }
        out.clip = Some(clip);
    } else {
        out.notes.push("h1 clip: the zone has no clipMap".into());
    }
    if let Some(m) = cap.map_ents.first() {
        out.entities = m.entity_string.clone();
        out.trigger_models = read_triggers(&r, m.triggers);
        let sz = L::SpawnPointEntityRecord::SIZE;
        if let Some(span) = m.spawns {
            let string = |at: usize| -> String {
                let i = r.u32(at) as usize;
                script_strings
                    .get(i)
                    .cloned()
                    .flatten()
                    .unwrap_or_else(String::new)
            };
            use L::SpawnPointEntityRecord as P;
            for i in 0..span.len / sz {
                let s = span.pos + sz * i;
                out.spawns.push(SpawnRecord {
                    classname: string(s + P::name),
                    targetname: string(s + P::targetname),
                    target: string(s + P::target),
                    script_noteworthy: string(s + P::script_noteworthy),
                    origin: r.vec3(s + P::origin),
                    angles: r.vec3(s + P::angles),
                });
            }
        }
    } else {
        out.notes
            .push("h1 entities: the zone has no MapEnts".into());
    }
    if let Some(cw) = cap.com_worlds.first()
        && let Some(lights) = cw.primary_lights
    {
        // Light 0 is the "no light" slot; the sun is the first of type 1.
        let pl = L::ComPrimaryLight::SIZE;
        for i in 0..cw.primary_light_count as usize {
            let l = lights.pos + pl * i;
            if r.u8(l + L::ComPrimaryLight::r#type) == 1 {
                let dir = r.vec3(l + L::ComPrimaryLight::dir);
                out.sun = Some(Sun {
                    dir,
                    color: r.vec3(l + L::ComPrimaryLight::color),
                });
                break;
            }
        }
        use L::ComPrimaryLight as P;
        let v2 = |at: usize| [r.f32(at), r.f32(at + 4)];
        for i in 0..cw.primary_light_count as usize {
            let l = lights.pos + pl * i;
            out.primary_lights.push(PrimaryLight {
                ty: r.u8(l + P::r#type),
                can_use_shadow_map: r.u8(l + P::canUseShadowMap),
                physically_based: r.u8(l + P::physicallyBased),
                exponent: r.u8(l + P::exponent),
                lighting_state: r.u8(l + P::lightingState),
                color: r.vec3(l + P::color),
                dir: r.vec3(l + P::dir),
                up: r.vec3(l + P::up),
                origin: r.vec3(l + P::origin),
                fade_offset: v2(l + P::fadeOffset),
                bulb_radius: r.f32(l + P::bulbRadius),
                bulb_length: r.vec3(l + P::bulbLength),
                radius: r.f32(l + P::radius),
                cos_half_fov_outer: r.f32(l + P::cosHalfFovOuter),
                cos_half_fov_inner: r.f32(l + P::cosHalfFovInner),
                cos_half_fov_expanded: r.f32(l + P::cosHalfFovExpanded),
                rotation_limit: r.f32(l + P::rotationLimit),
                translation_limit: r.f32(l + P::translationLimit),
                cuc_rotation_offset: r.f32(l + P::cucRotationOffsetRad),
                cuc_rotation_speed: r.f32(l + P::cucRotationSpeedRad),
                cuc_scroll: v2(l + P::cucScrollVector),
                cuc_scale: v2(l + P::cucScaleVector),
                cuc_trans: v2(l + P::cucTransVector),
                def_name: String::new(),
            });
        }
    }
    if let Some(cw) = cap.com_worlds.first()
        && let Some(envs) = cw.primary_light_envs
    {
        use L::ComPrimaryLightEnv as E;
        for i in 0..cw.primary_light_env_count as usize {
            let e = envs.pos + E::SIZE * i;
            let n = (r.u16(e + E::numIndices) as usize).min(4);
            out.primary_light_envs
                .push((0..n).map(|k| r.u16(e + E::primaryLightIndices + 2 * k)).collect());
        }
    }
    out
}

fn read_world(r: &R, g: &GfxWorldRec) -> Result<World, String> {
    use L::GfxWorld as G;
    let h = g.header;
    let mut w = World {
        name: g.name.clone(),
        ..Default::default()
    };
    let vs = L::GfxWorldVertex::SIZE;
    let vat = g.vertices.ok_or("no vertices in the file")?;
    let vn = g.vertex_count as usize;
    if !r.fits(vat, vs * vn) {
        return Err(format!("{vn} vertices run past the zone"));
    }
    w.vertices = (0..vn)
        .map(|i| {
            let v = vat + vs * i;
            use L::GfxWorldVertex as V;
            WorldVertex {
                xyz: r.vec3(v + V::xyz),
                binormal_sign: r.f32(v + V::binormalSign),
                color: r.u32(v + V::color),
                tex_coord: [r.f32(v + V::texCoord), r.f32(v + V::texCoord + 4)],
                lmap_coord: [r.f32(v + V::lmapCoord), r.f32(v + V::lmapCoord + 4)],
                normal: unpack_unit_vec(r.u32(v + V::normal)),
                tangent: unpack_unit_vec(r.u32(v + V::tangent)),
            }
        })
        .collect();
    w.vertex_bytes = r.0[vat..vat + vs * vn].to_vec();
    if let Some(l) = g.vertex_layer_data
        && r.fits(l.pos, l.len)
    {
        w.layer_bytes = r.0[l.pos..l.pos + l.len].to_vec();
    }
    let iat = g.indices.ok_or("no indices in the file")?;
    let n = g.index_count as usize;
    if !r.fits(iat, 2 * n) {
        return Err(format!("{n} indices run past the zone"));
    }
    w.indices = (0..n).map(|i| r.u16(iat + 2 * i)).collect();
    let sat = g.surfaces.ok_or("no surfaces in the file")?;
    let gs = L::GfxSurface::SIZE;
    let sb = L::GfxSurfaceBounds::SIZE;
    for i in 0..g.surface_count as usize {
        let s = sat + gs * i;
        use L::GfxSurface as S;
        let (mins, maxs) = g
            .surface_bounds
            .map_or(([0.0; 3], [0.0; 3]), |b| r.bounds(b + sb * i));
        w.surfaces.push(Surface {
            first_vertex: r.u32(s + S::tris__firstVertex),
            vertex_layer_data: r.u32(s + S::tris__vertexLayerData),
            vertex_count: r.u16(s + S::tris__vertexCount),
            tri_count: r.u16(s + S::tris__triCount),
            base_index: r.u32(s + S::tris__baseIndex),
            material: g.surface_materials.get(i).copied().flatten(),
            lightmap_index: r.u8(s + S::laf__fields__lightmapIndex),
            reflection_probe_index: r.u8(s + S::laf__fields__reflectionProbeIndex),
            primary_light_env: r.u8(s + S::laf__fields__primaryLightEnvIndex),
            flags: r.u8(s + S::laf__fields__flags),
            mins,
            maxs,
        });
    }
    let range = |b: usize, e: usize| (r.u32(h + b), r.u32(h + e));
    w.ranges = [
        range(G::dpvs__litOpaqueSurfsBegin, G::dpvs__litOpaqueSurfsEnd),
        range(G::dpvs__litDecalSurfsBegin, G::dpvs__litDecalSurfsEnd),
        range(G::dpvs__litTransSurfsBegin, G::dpvs__litTransSurfsEnd),
        range(G::dpvs__emissiveSurfsBegin, G::dpvs__emissiveSurfsEnd),
    ];
    if let Some(c) = g.light_grid_colors {
        let n = L::GfxLightGridColors::SIZE;
        for k in 0..c.len / n {
            let at = c.pos + n * k;
            w.light_grid_colors.push(std::array::from_fn(|t| {
                std::array::from_fn(|ch| half_to_f32(r.u16(at + 6 * t + 2 * ch)))
            }));
        }
    }
    if let (Some(rs), Some(rows), Some(ents)) = (
        g.light_grid_row_start,
        g.light_grid_rows,
        g.light_grid_entries,
    ) {
        let n = L::GfxLightGridEntry::SIZE;
        w.light_grid = Some(LightGrid {
            mins: std::array::from_fn(|k| r.u16(h + G::lightGrid__mins + 2 * k)),
            row_axis: (r.u32(h + G::lightGrid__rowAxis) as usize).min(2),
            col_axis: (r.u32(h + G::lightGrid__colAxis) as usize).min(2),
            row_start: (0..rs.len / 2).map(|k| r.u16(rs.pos + 2 * k)).collect(),
            rows: rows.get(r.0).to_vec(),
            entry_colors: (0..ents.len / n)
                .map(|k| r.u32(ents.pos + n * k + L::GfxLightGridEntry::colorsIndex))
                .collect(),
        });
    }
    if let (Some(pal), Some(bits)) = (g.light_grid_palette, g.light_grid_bitstream) {
        w.light_grid_palette = Some(crate::light_grid_palette::Palette {
            exponents: [
                r.u8(h + G::lightGrid__rangeExponent8BitsEncoding) as i8,
                r.u8(h + G::lightGrid__rangeExponent12BitsEncoding) as i8,
                r.u8(h + G::lightGrid__rangeExponent16BitsEncoding) as i8,
            ],
            offsets: (0..pal.len / 4)
                .map(|k| r.u32(pal.pos + 4 * k) as i32)
                .collect(),
            bits: bits.get(r.0).to_vec(),
            missing: r.u32(h + G::lightGrid__missingGridColorIndex),
        });
    }
    if let Some(at) = g.smodels {
        let sd = L::GfxStaticModelDrawInst::SIZE;
        let sl = L::GfxStaticModelLighting::SIZE;
        for i in 0..g.smodel_count as usize {
            let d = at + sd * i;
            use L::GfxStaticModelDrawInst as D;
            let a = d + D::placement__axis;
            let flags = r.u16(d + D::flags);
            let ground_lighting = match g.smodel_lighting {
                Some(l) if flags & 0x20 != 0 => {
                    let gl =
                        l + sl * i + L::GfxStaticModelLighting::ambientLightingInfo__groundLighting;
                    std::array::from_fn(|k| half_to_f32(r.u16(gl + 2 * k)))
                }
                _ => [0.0; 4],
            };
            w.static_models.push(StaticModel {
                model: g.smodel_models.get(i).copied().flatten(),
                origin: r.vec3(d + D::placement__origin),
                axis: [r.vec3(a), r.vec3(a + 12), r.vec3(a + 24)],
                scale: r.f32(d + D::placement__scale),
                cull_dist: f32::from(r.u16(d + D::cullDist)),
                flags,
                ground_lighting,
                reflection_probe_index: r.u8(d + D::reflectionProbeIndex),
                primary_light_env: r.u16(d + D::primaryLightEnvIndex),
                grid_color: match g.smodel_lighting {
                    Some(l) if flags & 0x40 != 0 => Some(r.u32(
                        l + sl * i + L::GfxStaticModelLighting::ambientLightingInfo__colorIndex,
                    )),
                    _ => None,
                },
                primary_light_weight: match g.smodel_lighting {
                    Some(l) if flags & 0x40 != 0 => Some(r.f32(
                        l + sl * i
                            + L::GfxStaticModelLighting::ambientLightingInfo__primaryLightWeight,
                    )),
                    _ => None,
                },
                model_lightmap: match g.smodel_lighting {
                    Some(l) if flags & 0x100 != 0 => {
                        use L::GfxStaticModelLighting as M;
                        let b = l + sl * i;
                        Some(ModelLightmap {
                            offset: [
                                r.f32(b + M::modelLightmapInfo__offset),
                                r.f32(b + M::modelLightmapInfo__offset + 4),
                            ],
                            scale: [
                                r.f32(b + M::modelLightmapInfo__scale),
                                r.f32(b + M::modelLightmapInfo__scale + 4),
                            ],
                            page: r.u32(b + M::modelLightmapInfo__lightmapIndex),
                        })
                    }
                    _ => None,
                },
                vertex_lighting: g
                    .smodel_vertex_lighting
                    .get(i)
                    .copied()
                    .flatten()
                    .filter(|s| r.fits(s.pos, s.len))
                    .map(|s| (s.pos, s.len)),
            });
        }
    }
    if let Some(at) = g.models {
        let bm = L::GfxBrushModel::SIZE;
        for i in 0..g.model_count as usize {
            let m = at + bm * i;
            let (mins, maxs) = r.bounds(m + L::GfxBrushModel::bounds);
            w.brush_models.push(BrushModel {
                mins,
                maxs,
                start_surf: r.u32(m + L::GfxBrushModel::startSurfIndex),
                surface_count: r.u32(m + L::GfxBrushModel::surfaceCount),
            });
        }
    }
    (w.mins, w.maxs) = r.bounds(h + G::bounds);
    (w.shadow_mins, w.shadow_maxs) = r.bounds(h + G::shadowBounds);
    for s in g.sky_surfaces.iter().flatten() {
        for k in 0..s.len / 4 {
            w.sky_surfaces.push(r.u32(s.pos + 4 * k));
        }
    }
    w.sky_images = g.sky_images.clone();
    w.lightmaps = g.lightmaps.clone();
    let rp = L::GfxReflectionProbe::SIZE;
    w.reflection_probes = g
        .reflection_probes
        .iter()
        .enumerate()
        .map(|(i, img)| {
            let origin = g
                .reflection_probe_origins
                .map_or([0.0; 3], |o| r.vec3(o + rp * i));
            (*img, origin)
        })
        .collect();
    Ok(w)
}

fn read_clip(r: &R, c: &ClipMapRec, cap: &Capture) -> Clip {
    let info = &c.info;
    let mut clip = Clip {
        name: c.name.clone(),
        materials: info.materials.clone(),
        ..Default::default()
    };
    let ps = L::cplane_s::SIZE;
    let read_plane = |at: usize| Plane {
        normal: r.vec3(at),
        dist: r.f32(at + 12),
    };
    if let Some(at) = info.planes {
        clip.planes = (0..info.plane_count as usize)
            .map(|i| read_plane(at + ps * i))
            .collect();
    }
    let bs = L::cbrushside_t::SIZE;
    let b = L::cbrush_t::SIZE;
    if let Some(at) = info.brushes {
        for i in 0..info.brush_count as usize {
            let br = at + b * i;
            let numsides = r.u16(br + L::cbrush_t::numsides) as usize;
            let (mins, maxs) = info
                .brush_bounds
                .map_or(([0.0; 3], [0.0; 3]), |bb| r.bounds(bb + 24 * i));
            let contents = info.brush_contents.map_or(0, |bc| r.i32(bc + 4 * i));
            let am = br + L::cbrush_t::axialMaterialNum;
            let axial_materials = [
                [r.i16(am), r.i16(am + 2), r.i16(am + 4)],
                [r.i16(am + 6), r.i16(am + 8), r.i16(am + 10)],
            ];
            let mut sides = Vec::with_capacity(numsides);
            // (The tools write a brush's sides with `write_s`, which
            // refers into the info's side table when they are already in
            // it: the walk resolves either form to a file offset.)
            let run = info.brush_first_sides.get(i).copied().flatten();
            match run {
                Some(first) => {
                    for k in 0..numsides {
                        let side = first + bs * k;
                        let plane = r.u32(side + L::cbrushside_t::planeIndex);
                        let material = r.u16(side + L::cbrushside_t::materialNum);
                        if (plane as usize) < clip.planes.len() {
                            sides.push((plane, material));
                        } else {
                            clip.missing_planes += 1;
                        }
                    }
                }
                None => clip.missing_planes += numsides,
            }
            clip.brushes.push(Brush {
                mins,
                maxs,
                contents,
                sides,
                axial_materials,
            });
        }
    }
    if let Some(at) = info.leaf_brush_nodes {
        let ln = L::cLeafBrushNode_s::SIZE;
        for i in 0..info.leaf_brush_node_count as usize {
            let node = at + ln * i;
            use L::cLeafBrushNode_s as N;
            let count = r.i16(node + N::leafBrushCount);
            let brushes = match info.leaf_brush_lists.get(i).copied().flatten() {
                Some(list) if count > 0 => {
                    (0..count as usize).map(|k| r.u16(list + 2 * k)).collect()
                }
                _ => Vec::new(),
            };
            let child_offsets = if count > 0 {
                [0, 0]
            } else {
                [
                    r.u16(node + N::data__children__childOffset),
                    r.u16(node + N::data__children__childOffset + 2),
                ]
            };
            clip.leaf_brush_nodes.push(LeafBrushNode {
                leaf_brush_count: count,
                brushes,
                child_offsets,
            });
        }
    }
    if let Some(at) = c.verts {
        clip.verts = (0..c.vert_count as usize)
            .map(|i| r.vec3(at + 12 * i))
            .collect();
    }
    if let Some(at) = c.tri_indices {
        clip.tri_indices = (0..3 * c.tri_count as usize)
            .map(|i| r.u16(at + 2 * i))
            .collect();
    }
    if let Some(at) = c.partitions {
        let cp = L::CollisionPartition::SIZE;
        clip.partitions = (0..c.partition_count as usize)
            .map(|i| {
                let p = at + cp * i;
                (
                    r.u8(p + L::CollisionPartition::triCount),
                    r.i32(p + L::CollisionPartition::firstTri),
                    r.u8(p + L::CollisionPartition::firstVertSegment),
                )
            })
            .collect();
    }
    if let Some(at) = c.aabb_trees {
        let ab = L::CollisionAabbTree::SIZE;
        use L::CollisionAabbTree as A;
        clip.aabb_trees = (0..c.aabb_tree_count as usize)
            .map(|i| {
                let t = at + ab * i;
                AabbNode {
                    origin: r.vec3(t + A::midPoint),
                    half_size: r.vec3(t + A::halfSize),
                    material_index: r.u16(t + A::materialIndex),
                    child_count: r.u16(t + A::childCount),
                    index: r.i32(t + A::u),
                }
            })
            .collect();
    }
    if let Some(at) = c.cmodels {
        let cm = L::cmodel_t::SIZE;
        use L::cmodel_t as M;
        for i in 0..c.cmodel_count as usize {
            let m = at + cm * i;
            let (mins, maxs) = r.bounds(m + M::bounds);
            let own = c.cmodel_own_info.get(i).copied().unwrap_or(false);
            clip.cmodels.push(CModel {
                mins,
                maxs,
                radius: r.f32(m + M::radius),
                first_coll_aabb: u16::try_from(r.u32(m + M::leaf__firstCollAabbIndex))
                    .unwrap_or(u16::MAX),
                coll_aabb_count: u16::try_from(r.u32(m + M::leaf__collAabbCount))
                    .unwrap_or(u16::MAX),
                leaf_brush_node: if own {
                    -1
                } else {
                    r.i32(m + M::leaf__leafBrushNode)
                },
            });
        }
    }
    if let Some(at) = c.static_models {
        let sm = L::cStaticModel_s::SIZE;
        use L::cStaticModel_s as S;
        for (i, model) in c.static_model_models.iter().enumerate() {
            let s = at + sm * i;
            let a = s + S::invScaledAxis;
            let (mins, maxs) = r.bounds(s + S::absBounds);
            clip.static_models.push(ClipStaticModel {
                model: *model,
                origin: r.vec3(s + S::origin),
                inv_scaled_axis: [r.vec3(a), r.vec3(a + 12), r.vec3(a + 24)],
                mins,
                maxs,
                layer: r.u8(s + S::lightingHandle),
            });
        }
    }
    for k in 0..2 {
        let Some(at) = c.dyn_ent_defs[k] else {
            continue;
        };
        let de = L::DynEntityDef::SIZE;
        use L::DynEntityDef as D;
        for i in 0..c.dyn_ent_counts[k] as usize {
            let d = at + de * i;
            clip.dyn_ents.push(DynEnt {
                kind: r.i32(d + D::r#type),
                quat: std::array::from_fn(|q| r.f32(d + D::pose__quat + 4 * q)),
                origin: r.vec3(d + D::pose__origin),
                model: c.dyn_ent_models[k].get(i).copied().flatten(),
                brush_model: r.u16(d + D::brushModel),
                health: i32::from(r.i16(d + D::health)),
                contents: r.i32(d + D::contents),
            });
        }
    }
    if let Some(at) = c.scriptables {
        use L::ScriptableInstance as S;
        for i in 0..c.scriptable_count as usize {
            let s = at + S::SIZE * i;
            let Some(def) = c.scriptable_defs.get(i).copied().flatten() else {
                continue;
            };
            clip.scriptables.push(crate::scriptable::Instance {
                def: cap.name_of(def).to_owned(),
                origin: r.vec3(s + S::origin),
                angles: r.vec3(s + S::angles),
                targetname: r.u32(s + S::targetname),
                pre_brush_model: r.u16(s + S::preBrushModel),
                post_brush_model: r.u16(s + S::postBrushModel),
                flags: r.u8(s + S::flags),
                target_count: r.u8(s + S::targetCount),
            });
        }
    }
    if let Some(at) = c.stages {
        use L::Stage as S;
        clip.stages = (0..c.stage_count as usize)
            .map(|i| at + S::SIZE * i)
            .filter(|&s| r.fits(s, S::SIZE))
            .map(|s| Stage {
                origin: r.vec3(s + S::origin),
                trigger_index: r.u16(s + S::triggerIndex),
                sun_primary_light_index: r.u8(s + S::sunPrimaryLightIndex),
                radiometric_scale: r.f32(s + S::radiometricScale),
            })
            .collect();
    }
    clip.stage_triggers = read_triggers(r, c.stage_triggers);
    clip
}

fn read_triggers(
    r: &R,
    [models, hulls, slabs]: [Option<crate::walk::Span>; 3],
) -> Vec<Vec<TriggerHull>> {
    let (Some(models), Some(hulls)) = (models, hulls) else {
        return Vec::new();
    };
    let hull_count = hulls.len / L::TriggerHull::SIZE;
    let slab_count = slabs.map_or(0, |s| s.len / L::TriggerSlab::SIZE);
    (0..models.len / L::TriggerModel::SIZE)
        .map(|i| {
            let m = models.pos + L::TriggerModel::SIZE * i;
            let count = usize::from(r.u16(m + L::TriggerModel::hullCount));
            let first = usize::from(r.u16(m + L::TriggerModel::firstHull));
            if first + count > hull_count {
                return Vec::new();
            }
            (first..first + count)
                .map(|n| {
                    let h = hulls.pos + L::TriggerHull::SIZE * n;
                    let sc = usize::from(r.u16(h + L::TriggerHull::slabCount));
                    let sf = usize::from(r.u16(h + L::TriggerHull::firstSlab));
                    let slabs = match slabs {
                        Some(sp) if sf + sc <= slab_count => (sf..sf + sc)
                            .map(|k| {
                                let s = sp.pos + L::TriggerSlab::SIZE * k;
                                (
                                    r.vec3(s + L::TriggerSlab::dir),
                                    r.f32(s + L::TriggerSlab::midPoint),
                                    r.f32(s + L::TriggerSlab::halfSize),
                                )
                            })
                            .collect(),
                        _ => Vec::new(),
                    };
                    TriggerHull {
                        mid: r.vec3(h + L::TriggerHull::bounds__midPoint),
                        half: r.vec3(h + L::TriggerHull::bounds__halfSize),
                        slabs,
                    }
                })
                .collect()
        })
        .collect()
}

/// A raw file of the zone by name (case-insensitive), inflated when stored
/// compressed (zlib).
pub fn raw_file(cap: &Capture, image: &[u8], name: &str) -> Option<Vec<u8>> {
    let r = cap
        .raw_files
        .iter()
        .find(|r| r.name.eq_ignore_ascii_case(name))?;
    let data = r.data?.get(image);
    if r.compressed_len == 0 {
        return Some(data.strip_suffix(&[0]).unwrap_or(data).to_vec());
    }
    miniz_oxide::inflate::decompress_to_vec_zlib(data).ok()
}

/// bo2mp: raw files a retail map keeps outside its own zone. A retail
/// map's createfx, fog and sun scripts sit in `patch_<map>.ff` (the map zone
/// holds only compiled copies), and its film vision in `patch_common_mp.ff`.
#[derive(Clone, Debug, Default)]
pub struct PatchRaws {
    /// From `patch_<map>.ff`: loaded after the map, so these win.
    over: std::collections::HashMap<String, Vec<u8>>,
    /// From `patch_common_mp.ff`: used when the map zone has none.
    under: std::collections::HashMap<String, Vec<u8>>,
    /// The zones read, for a report.
    pub zones: Vec<String>,
}

impl PatchRaws {
    /// Reads `names` (case-insensitive) from `patch_<map>.ff` and
    /// `patch_common_mp.ff` in `zone_dir`; absent zones and unreadable ones
    /// give nothing. The common patch is opened only for a name neither the
    /// map's own patch nor the map zone `cap` holds.
    pub fn read(
        zone_dir: &std::path::Path,
        map: &str,
        names: &[String],
        cap: &Capture,
        image: &[u8],
    ) -> Self {
        let mut out = Self::default();
        let map = map.to_ascii_lowercase();
        let take = |zone: &str,
                    wanted: &[&String],
                    into: &mut std::collections::HashMap<String, Vec<u8>>|
         -> bool {
            let path = zone_dir.join(format!("{zone}.ff"));
            if !path.is_file() {
                return false;
            }
            let walked = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                let z = crate::Zone::open(&path).ok()?;
                let (c, _) = crate::walk::walk_zone(&z);
                let mut got = Vec::new();
                for n in wanted {
                    if let Some(b) = raw_file(&c, &z.image, n) {
                        got.push((n.to_ascii_lowercase(), b));
                    }
                }
                Some(got)
            }));
            let Ok(Some(got)) = walked else { return false };
            into.extend(got);
            true
        };
        let all: Vec<&String> = names.iter().collect();
        if take(&format!("patch_{map}"), &all, &mut out.over) {
            out.zones.push(format!("patch_{map}"));
        }
        let missing: Vec<&String> = names
            .iter()
            .filter(|n| {
                !out.over.contains_key(&n.to_ascii_lowercase()) && raw_file(cap, image, n).is_none()
            })
            .collect();
        if !missing.is_empty() && take("patch_common_mp", &missing, &mut out.under) {
            out.zones.push("patch_common_mp".to_owned());
        }
        out
    }

    /// The raw file: the map's patch, else the map zone, else the common
    /// patch.
    pub fn get(&self, cap: &Capture, image: &[u8], name: &str) -> Option<Vec<u8>> {
        let key = name.to_ascii_lowercase();
        self.over
            .get(&key)
            .cloned()
            .or_else(|| raw_file(cap, image, name))
            .or_else(|| self.under.get(&key).cloned())
    }
}

/// An IEEE half float.
pub fn half_to_f32(h: u16) -> f32 {
    let sign = if h & 0x8000 != 0 { -1.0 } else { 1.0 };
    let exp = (h >> 10) & 0x1f;
    let frac = f32::from(h & 0x3ff);
    sign * match exp {
        0 => frac * 2f32.powi(-24),
        31 => f32::INFINITY,
        e => (1.0 + frac / 1024.0) * 2f32.powi(i32::from(e) - 15),
    }
}

/// Where a map keeps a second level above the one it starts on: the height
/// to cut at, keeping what lies below (as `fastfile_iw6::map`: two groups
/// of spawn points at least 8,000 units apart in height, each of at least
/// eight; the cut is half way between them).
pub fn alternate_level_cut(spawns: &[SpawnRecord]) -> Option<f32> {
    let mut z: Vec<f32> = spawns
        .iter()
        .map(|s| s.origin[2])
        .filter(|z| z.is_finite())
        .collect();
    z.sort_by(f32::total_cmp);
    let (i, gap) = z
        .windows(2)
        .enumerate()
        .map(|(i, w)| (i, w[1] - w[0]))
        .max_by(|a, b| a.1.total_cmp(&b.1))?;
    let (below, above) = (i + 1, z.len() - i - 1);
    (gap >= 8000.0 && below >= 8 && above >= 8).then(|| (z[i] + z[i + 1]) / 2.0)
}

/// A brush model an entity places as the map starts.
#[derive(Clone, Debug, PartialEq)]
pub struct PlacedBrushModel {
    /// Its surfaces (`World::surfaces`); their vertices are the model's own,
    /// about its origin.
    pub surfaces: std::ops::Range<usize>,
    pub origin: [f32; 3],
    /// Forward, left, up in the world.
    pub axis: [[f32; 3]; 3],
}

/// An entity's axes (forward, left, up) from its pitch, yaw and roll in
/// degrees, as the engine's `AnglesToAxis`.
pub fn axis_of(angles: [f32; 3]) -> [[f32; 3]; 3] {
    let [p, y, r] = angles.map(f32::to_radians);
    let (sp, cp, sy, cy, sr, cr) = (p.sin(), p.cos(), y.sin(), y.cos(), r.sin(), r.cos());
    let forward = [cp * cy, cp * sy, -sp];
    let right = [-sr * sp * cy + cr * sy, -sr * sp * sy - cr * cy, -sr * cp];
    let up = [cr * sp * cy + sr * sy, cr * sp * sy - sr * cy, cr * cp];
    [forward, right.map(|v| -v), up]
}

/// The brush models (`"model" "*N"`, N > 0, with surfaces) the map's
/// `script_brushmodel*` entities place as it starts (the rules of
/// `fastfile_iw6::map::placed_brush_models`: not an `exploder`, not shown
/// by a script, not above `cut`).
pub fn placed_brush_models(map: &IwMap, cut: Option<f32>) -> Vec<PlacedBrushModel> {
    let Some(w) = &map.world else {
        return Vec::new();
    };
    let text = named_entity_text(map.entities.trim_end_matches('\0'));
    let mut out = Vec::new();
    for kv in entity_blocks(&text) {
        if !kv
            .get("classname")
            .is_some_and(|c| c.starts_with("script_brushmodel"))
        {
            continue;
        }
        if kv.get("targetname") == Some(&"exploder") {
            continue;
        }
        if kv
            .get("script_noteworthy")
            .is_some_and(|n| n.split([',', ' ']).any(|t| t == "show"))
        {
            continue;
        }
        let Some(n) = kv
            .get("model")
            .and_then(|m| m.strip_prefix('*'))
            .and_then(|n| n.parse::<usize>().ok())
        else {
            continue;
        };
        let Some(b) = w.brush_models.get(n) else {
            continue;
        };
        if n == 0 || b.surface_count == 0 || b.start_surf == u32::MAX {
            continue;
        }
        let origin = kv
            .get("origin")
            .and_then(|v| parse_vec3(v))
            .unwrap_or([0.0; 3]);
        if cut.is_some_and(|c| origin[2] > c) {
            continue;
        }
        let angles = kv
            .get("angles")
            .and_then(|v| parse_vec3(v))
            .unwrap_or_else(|| {
                [
                    0.0,
                    kv.get("angle").and_then(|a| a.parse().ok()).unwrap_or(0.0),
                    0.0,
                ]
            });
        let start = b.start_surf as usize;
        out.push(PlacedBrushModel {
            surfaces: start..start + b.surface_count as usize,
            origin,
            axis: axis_of(angles),
        });
    }
    out
}

/// One lightmap page in Call of Duty 4's form (what the fallback draw's
/// IW3 branch reads): the primary as 8-bit luminance (`D3DFMT_L8`) and the
/// secondary's two stacked pages as `A8R8G8B8` (bytes B, G, R, A).
#[derive(Clone, Debug)]
pub struct Iw3Lightmap {
    pub primary: (u16, u16, Vec<u8>),
    pub secondary: (u16, u16, Vec<u8>),
    /// The secondary's mean luminance (linear, before the exposure) where
    /// the primary says the sun reaches (over 0.9) and where it does not
    /// (under 0.1), with their texel counts: whether the baked light holds
    /// the sun's own (`sun_in_lightmap`).
    pub split: [f64; 4],
}

/// Whether a map's baked light holds the sun's direct light, from its
/// pages' `split`s: its sunlit texels at least twice as bright as its
/// shaded ones (Modern Warfare Remastered's bake keeps the sun apart: its
/// sunlit and shaded texels read alike).
pub fn sun_in_lightmap(splits: &[[f64; 4]]) -> Option<f32> {
    let (mut lit, mut nl, mut shade, mut ns) = (0.0, 0.0, 0.0, 0.0);
    for s in splits {
        lit += s[0] * s[1];
        nl += s[1];
        shade += s[2] * s[3];
        ns += s[3];
    }
    (nl > 100.0 && ns > 100.0 && shade > 0.0).then(|| ((lit / nl) / (shade / ns)) as f32)
}

/// The exposure H1's linear light is scaled by when a map's own
/// (`light_exposure`) cannot be measured.
pub const LIGHT_EXPOSURE: f32 = 1.0;

/// What a map's brighter baked light (its 90th percentile, linear) is
/// exposed to: CoD4's gamma bytes put it at `sqrt(EXPOSURE_KEY)`.
pub const EXPOSURE_KEY: f32 = 0.3;

/// The brightest the sun gets in CoD4's gamma space.
pub const SUN_GAMMA_MAX: f32 = 0.8;

/// A map's exposure from its baked light: `EXPOSURE_KEY` over the 90th
/// percentile of its lit lightmap texels' luminance (`lightmap_key`).
pub fn light_exposure(key: Option<f32>) -> f32 {
    key.filter(|k| *k > 1e-4)
        .map_or(LIGHT_EXPOSURE, |k| (EXPOSURE_KEY / k).clamp(0.05, 32.0))
}

/// H1's sun (linear HDR, its brightest channel) as CoD4's gamma-space
/// `sunlight` under the lightmaps' exposure, at most `SUN_GAMMA_MAX`.
pub fn iw3_sun_gamma(peak: f32, exposure: f32) -> f32 {
    (peak.max(0.0) * exposure).sqrt().min(SUN_GAMMA_MAX)
}

/// The pixels of a lightmap image: inline in the zone (both usermaps), or
/// streamed from `paks`.
fn lightmap_pixels(
    cap: &Capture,
    image: &[u8],
    id: Option<AssetId>,
    paks: &mut crate::image::ImagePaks,
) -> Result<crate::image::ImagePixels, String> {
    let rec = id
        .and_then(|i| cap.images.get(i.index as usize))
        .ok_or_else(|| "image not in the zone".to_owned())?;
    crate::image::image_pixels(rec, image, paks, 1 << 14)
}

/// The 90th percentile of the luminance of the lit texels of a map's
/// lightmap secondaries (both pages, linear light), every 7th texel read.
pub fn lightmap_key(
    cap: &Capture,
    image: &[u8],
    pairs: &[(Option<AssetId>, Option<AssetId>)],
    paks: &mut crate::image::ImagePaks,
) -> Option<f32> {
    let mut lum: Vec<f32> = Vec::new();
    for &(_, s) in pairs {
        let Ok(px) = lightmap_pixels(cap, image, s, paks) else {
            continue;
        };
        let px = match px.dxgi_format {
            27..=29 => rgba8_secondary_as_half(px),
            _ => px,
        };
        if px.dxgi_format != 10 {
            continue;
        }
        for t in px.data.chunks_exact(8).step_by(7) {
            let h = |c: usize| half_to_f32(u16::from_le_bytes([t[2 * c], t[2 * c + 1]]));
            let l = 0.3 * h(0) + 0.59 * h(1) + 0.11 * h(2);
            if l > 0.0 && l.is_finite() {
                lum.push(l);
            }
        }
    }
    if lum.is_empty() {
        return None;
    }
    let at = (lum.len() - 1) * 9 / 10;
    let (_, p90, _) = lum.select_nth_unstable_by(at, |a, b| a.total_cmp(b));
    Some(*p90)
}

/// A BC4 image's first level as one byte a texel (format 81 signed).
fn bc4_texels(px: &crate::image::ImagePixels) -> Result<Vec<u8>, String> {
    let (w, h) = (px.width as usize, px.height as usize);
    if w % 4 != 0 || h % 4 != 0 || w == 0 || h == 0 {
        return Err(format!("BC4 primary {w}x{h} is not whole blocks"));
    }
    let blocks = (w / 4) * (h / 4);
    if px.data.len() < blocks * 8 {
        return Err(format!(
            "BC4 primary has {} of {} bytes",
            px.data.len(),
            blocks * 8
        ));
    }
    let mut out = vec![0u8; w * h];
    for by in 0..h / 4 {
        for bx in 0..w / 4 {
            let b = (by * (w / 4) + bx) * 8;
            let at = (by * 4) * w + bx * 4;
            bcdec_rs::bc4(&px.data[b..b + 8], &mut out[at..], w, px.dxgi_format == 81);
        }
    }
    Ok(out)
}

/// H1's lightmap pair as Call of Duty 4's, under `exposure`.
///
/// The primary is `R8_UNORM` (the usermaps) or `BC4_UNORM` (retail maps,
/// as Ghosts') at twice the secondary page's
/// size: the sun's baked visibility, as CoD4's L8 primary, box-filtered
/// down to the page's size. The secondary is two pages stacked
/// (`R16G16B16A16_FLOAT`), the upper the softer, as CoD4's A and B: linear
/// light (scaled by `exposure`, then CoD4's gamma bytes: `sqrt`, at most
/// one) and in alpha a tangent direction in -1..1 (CoD4's alpha code,
/// `a * 4.08 - 2.08`).
pub fn iw3_lightmap(
    cap: &Capture,
    image: &[u8],
    pair: (Option<AssetId>, Option<AssetId>),
    exposure: f32,
    linear: bool,
    paks: &mut crate::image::ImagePaks,
) -> Result<Iw3Lightmap, String> {
    let pp = lightmap_pixels(cap, image, pair.0, paks)?;
    let sp = lightmap_pixels(cap, image, pair.1, paks)?;
    // The primary as 8-bit texels: R8 as it is (the usermaps), BC4
    // decoded (retail maps, as Ghosts').
    let pp = match pp.dxgi_format {
        60..=63 => pp,
        79..=81 => crate::image::ImagePixels {
            data: bc4_texels(&pp)?,
            dxgi_format: 61,
            levels: 1,
            ..pp
        },
        other => {
            return Err(format!("primary format {other} (R8 or BC4 expected)"));
        }
    };
    let primary_bytes = 1;
    let sp = match sp.dxgi_format {
        27..=29 => rgba8_secondary_as_half(sp),
        _ => sp,
    };
    if sp.dxgi_format != 10 {
        return Err(format!(
            "secondary format {} (R16G16B16A16_FLOAT expected)",
            sp.dxgi_format
        ));
    }
    let (sw, sh) = (sp.width as usize, sp.height as usize);
    if sw == 0 || sh < 2 || sh % 2 != 0 {
        return Err(format!("secondary {sw}x{sh}"));
    }
    let need = sw * sh * 8;
    if sp.data.len() < need {
        return Err(format!("secondary has {} of {need} bytes", sp.data.len()));
    }
    let mut secondary = vec![0u8; sw * sh * 4];
    for i in 0..sw * sh {
        let h = |c: usize| {
            half_to_f32(u16::from_le_bytes([
                sp.data[i * 8 + 2 * c],
                sp.data[i * 8 + 2 * c + 1],
            ]))
        };
        // (Linear: the square root of the light over 4, Infinite Warfare's
        // and Black Ops III's look's packing, to 4 before it clips.)
        let g = |v: f32| {
            let v = if v.is_finite() { v * exposure } else { 0.0 };
            let v = if linear { v * 0.25 } else { v };
            (v.clamp(0.0, 1.0).sqrt() * 255.0).round() as u8
        };
        let dir = if h(3).is_finite() { h(3) } else { 0.0 };
        let a = (((dir.clamp(-1.0, 1.0) + 2.08) / 4.08) * 255.0)
            .round()
            .clamp(0.0, 255.0) as u8;
        secondary[i * 4..i * 4 + 4].copy_from_slice(&[g(h(2)), g(h(1)), g(h(0)), a]);
    }
    let (pw, ph) = (pp.width as usize, pp.height as usize);
    if pp.data.len() < pw * ph * primary_bytes {
        return Err(format!(
            "primary has {} of {} bytes",
            pp.data.len(),
            pw * ph * primary_bytes
        ));
    }
    let page_h = sh / 2;
    let (fx, fy) = (pw / sw.max(1), ph / page_h.max(1));
    let (fx, fy) = if fx >= 1 && fy >= 1 && pw == sw * fx && ph == page_h * fy {
        (fx, fy)
    } else {
        (1, 1)
    };
    let (ow, oh) = (pw / fx, ph / fy);
    let mut primary = vec![0u8; ow * oh];
    for y in 0..oh {
        for x in 0..ow {
            let mut sum = 0u32;
            for dy in 0..fy {
                for dx in 0..fx {
                    sum += u32::from(pp.data[(y * fy + dy) * pw + x * fx + dx]);
                }
            }
            primary[y * ow + x] = (sum / (fx * fy) as u32) as u8;
        }
    }
    // The upper page's light (linear) against the sun's baked visibility
    // at the same place.
    let mut split = [0f64; 4];
    let (mut sl, mut ss) = (0f64, 0f64);
    if ow == sw && oh == page_h {
        for y in 0..oh {
            for x in 0..ow {
                let i = y * sw + x;
                let h = |c: usize| {
                    half_to_f32(u16::from_le_bytes([
                        sp.data[i * 8 + 2 * c],
                        sp.data[i * 8 + 2 * c + 1],
                    ]))
                };
                let l = f64::from(0.3 * h(0) + 0.59 * h(1) + 0.11 * h(2));
                if !l.is_finite() || l <= 0.0 {
                    continue;
                }
                let vis = primary[y * ow + x];
                if vis > 230 {
                    sl += l;
                    split[1] += 1.0;
                } else if vis < 25 {
                    ss += l;
                    split[3] += 1.0;
                }
            }
        }
    }
    split[0] = if split[1] > 0.0 { sl / split[1] } else { 0.0 };
    split[2] = if split[3] > 0.0 { ss / split[3] } else { 0.0 };
    let to16 = |v: usize| u16::try_from(v).map_err(|_| format!("{v} is too large"));
    Ok(Iw3Lightmap {
        primary: (to16(ow)?, to16(oh)?, primary),
        secondary: (to16(sw)?, to16(sh)?, secondary),
        split,
    })
}

/// An RGBA8 secondary (some HorizonMW usermaps: Quarry, Sub Base,
/// Underpass) as the half floats the other maps store: its colour bytes
/// decoded from sRGB to linear light (`BO2MP_H1_LM8_LINEAR=1`: read as
/// linear), times `BO2MP_H1_LM8_SCALE` (default 1), and its alpha byte the
/// tangent direction in -1..1 (128 the middle).
fn rgba8_secondary_as_half(sp: crate::image::ImagePixels) -> crate::image::ImagePixels {
    let env = |n: &str| {
        codlegacy_env::var(n)
            .ok()
            .and_then(|v| v.trim().parse::<f32>().ok())
    };
    let linear = env("BO2MP_H1_LM8_LINEAR").is_some_and(|v| v != 0.0);
    let scale = env("BO2MP_H1_LM8_SCALE")
        .filter(|v| v.is_finite() && *v >= 0.0)
        .unwrap_or(1.0);
    let n = (sp.width * sp.height) as usize;
    let mut data = vec![0u8; n * 8];
    let srgb = |b: u8| {
        let c = f32::from(b) / 255.0;
        if linear {
            c
        } else if c <= 0.04045 {
            c / 12.92
        } else {
            ((c + 0.055) / 1.055).powf(2.4)
        }
    };
    for i in 0..n.min(sp.data.len() / 4) {
        let t = &sp.data[i * 4..i * 4 + 4];
        let v = [
            srgb(t[0]) * scale,
            srgb(t[1]) * scale,
            srgb(t[2]) * scale,
            (f32::from(t[3]) - 128.0) / 127.0,
        ];
        for c in 0..4 {
            data[i * 8 + 2 * c..i * 8 + 2 * c + 2]
                .copy_from_slice(&f32_to_half(v[c]).to_le_bytes());
        }
    }
    crate::image::ImagePixels {
        data,
        dxgi_format: 10,
        levels: 1,
        ..sp
    }
}

/// An f32 as an IEEE half (round toward zero; enough for light values).
fn f32_to_half(v: f32) -> u16 {
    let b = v.to_bits();
    let sign = ((b >> 16) & 0x8000) as u16;
    let exp = ((b >> 23) & 0xff) as i32;
    let man = b & 0x7f_ffff;
    if exp == 0xff {
        return sign | 0x7c00 | if man != 0 { 0x200 } else { 0 };
    }
    let e = exp - 127 + 15;
    if e >= 0x1f {
        return sign | 0x7c00;
    }
    if e <= 0 {
        if e < -10 {
            return sign;
        }
        let m = (man | 0x80_0000) >> (14 - e);
        return sign | m as u16;
    }
    sign | ((e as u16) << 10) | (man >> 13) as u16
}

/// A world summary (counts), for probes and logs.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Summary {
    pub surfaces: usize,
    pub vertices: usize,
    pub indices: usize,
    pub static_models: usize,
    pub brush_models: usize,
    pub brushes: usize,
    pub collision_tris: usize,
    pub entity_chars: usize,
    pub spawns: usize,
    pub lightmaps: usize,
    pub images_inline: usize,
    pub images_streamed: usize,
    /// Streamed images whose parts are in the zone's own pak (index 96).
    pub images_in_zone_pak: usize,
}

/// Counts of a map and its zone's images.
pub fn summary(map: &IwMap, cap: &Capture) -> Summary {
    let w = map.world.as_ref();
    let c = map.clip.as_ref();
    let own = |i: &&crate::walk::ImageRec| {
        i.stream_files
            .iter()
            .flatten()
            .any(|s| s.file_index == crate::image::ZONE_PAK_INDEX)
    };
    Summary {
        surfaces: w.map_or(0, |w| w.surfaces.len()),
        vertices: w.map_or(0, |w| w.vertices.len()),
        indices: w.map_or(0, |w| w.indices.len()),
        static_models: w.map_or(0, |w| w.static_models.len()),
        brush_models: w.map_or(0, |w| w.brush_models.len()),
        brushes: c.map_or(0, |c| c.brushes.len()),
        collision_tris: c.map_or(0, |c| c.tri_indices.len() / 3),
        entity_chars: map.entities.len(),
        spawns: map.spawns.len(),
        lightmaps: w.map_or(0, |w| w.lightmaps.len()),
        images_inline: cap
            .images
            .iter()
            .filter(|i| !i.streamed && i.pixels.is_some())
            .count(),
        images_streamed: cap.images.iter().filter(|i| i.streamed).count(),
        images_in_zone_pak: cap.images.iter().filter(|i| i.streamed).filter(own).count(),
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn spawns_already_in_the_text_are_not_repeated() {
        let map = super::IwMap {
            entities: "{\n\"classname\" \"mp_tdm_spawn\"\n\"origin\" \"1 2 3\"\n}\n".to_owned(),
            spawns: vec![
                super::SpawnRecord {
                    classname: "mp_tdm_spawn".into(),
                    origin: [1.0, 2.0, 3.0],
                    ..Default::default()
                },
                super::SpawnRecord {
                    classname: "mp_dm_spawn".into(),
                    origin: [4.0, 5.0, 6.0],
                    ..Default::default()
                },
            ],
            ..Default::default()
        };
        let text = map.entity_string();
        assert_eq!(text.matches("mp_tdm_spawn").count(), 1, "{text}");
        assert_eq!(text.matches("mp_dm_spawn").count(), 1, "{text}");
    }

    #[test]
    fn retail_numbered_keys_are_named() {
        let text = "{\n170 \"script_origin\"\n1190 \"minimap_corner\"\n738 \"1744 -1248 248\"\n0 \"node_name\" \"n1\"\n31024 \"30\"\n}\n";
        assert_eq!(
            super::named_entity_text(text),
            "{\n\"classname\" \"script_origin\"\n\"targetname\" \"minimap_corner\"\n\"origin\" \"1744 -1248 248\"\n\"node_name\" \"n1\"\n\"h1_31024\" \"30\"\n}\n"
        );
    }

    #[test]
    fn brush_models_are_placed_by_their_entities() {
        let w = super::World {
            brush_models: vec![
                super::BrushModel {
                    start_surf: 0,
                    surface_count: 5,
                    ..Default::default()
                },
                super::BrushModel {
                    start_surf: 5,
                    surface_count: 2,
                    ..Default::default()
                },
                super::BrushModel {
                    start_surf: 7,
                    surface_count: 3,
                    ..Default::default()
                },
            ],
            ..Default::default()
        };
        let map = super::IwMap {
            world: Some(w),
            entities: "{\n\"classname\" \"script_brushmodel\"\n\"origin\" \"10 20 30\"\n\"model\" \"*1\"\n}\n{\n\"classname\" \"script_brushmodel\"\n\"targetname\" \"exploder\"\n\"model\" \"*2\"\n}\n".to_owned(),
            ..Default::default()
        };
        let placed = super::placed_brush_models(&map, None);
        assert_eq!(placed.len(), 1);
        assert_eq!(
            (placed[0].surfaces.clone(), placed[0].origin),
            (5..7, [10.0, 20.0, 30.0])
        );
    }

    #[test]
    fn halves_and_unit_vectors() {
        assert_eq!(super::half_to_f32(0x3c00), 1.0);
        assert_eq!(super::half_to_f32(0xc000), -2.0);
        let n = super::unpack_unit_vec(1023 | (511 << 10) | (511 << 20));
        assert!((n[0] - 1.0).abs() < 1e-6 && n[1].abs() < 2e-3 && n[2].abs() < 2e-3);
    }
}

#[cfg(test)]
mod light_grid_tests {
    use super::LightGrid;

    /// One row: columns 10..16, z 5..7; runs (2 columns, 2 z from 0),
    /// (2 empty), (2 columns, 2 z from 0), no terminator (as Rust's rows
    /// that fill their columns exactly).
    #[test]
    fn row_runs() {
        let mut rows = vec![];
        for v in [10u16, 6, 5, 2] {
            rows.extend_from_slice(&v.to_le_bytes());
        }
        rows.extend_from_slice(&100u32.to_le_bytes());
        rows.extend_from_slice(&[2, 2, 0, 2, 0, 2, 2, 0]);
        let grid = LightGrid {
            mins: [40, 0, 0],
            row_axis: 0,
            col_axis: 1,
            row_start: vec![0],
            rows,
            entry_colors: (0..200).collect(),
        };
        assert_eq!(grid.colour_at([40, 10, 5]), Some(100));
        assert_eq!(grid.colour_at([40, 10, 6]), Some(101));
        assert_eq!(grid.colour_at([40, 11, 5]), Some(102));
        assert_eq!(grid.colour_at([40, 12, 5]), None, "an empty column");
        assert_eq!(grid.colour_at([40, 14, 5]), Some(104));
        assert_eq!(grid.colour_at([40, 15, 6]), Some(107));
        assert_eq!(grid.colour_at([40, 16, 5]), None, "past the row");
        assert_eq!(grid.colour_at([41, 10, 5]), None, "no such row");
    }
}

#[cfg(test)]
mod half_round_trip {
    #[test]
    fn halves_round_trip() {
        for v in [0.0f32, 0.5, 1.0, -0.75, 3.25, 0.001, 1e-6] {
            let back = super::half_to_f32(super::f32_to_half(v));
            assert!((back - v).abs() <= v.abs() * 2e-3 + 1e-7, "{v} -> {back}");
        }
    }
}

#[cfg(test)]
mod dyn_ent_tests {
    use super::*;

    fn clip_with(d: &[DynEnt]) -> Clip {
        Clip { dyn_ents: d.to_vec(), ..Clip::default() }
    }

    #[test]
    fn clutter_with_a_model_becomes_a_placed_model_with_its_pose() {
        let id = Some(AssetId { ty: crate::asset_type::AssetType::XModel, index: 7 });
        // A quarter turn about z: forward (1,0,0) -> (0,1,0).
        let h = std::f32::consts::FRAC_1_SQRT_2;
        let clutter = DynEnt { kind: 1, quat: [0.0, 0.0, h, h], origin: [10.0, 20.0, 30.0], model: id, ..DynEnt::default() };
        let brush = DynEnt { kind: 1, model: None, ..DynEnt::default() };
        let other = DynEnt { kind: 0, model: id, ..DynEnt::default() };
        let out = dyn_ent_models(&clip_with(&[clutter, brush, other]), &[]);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].origin, [10.0, 20.0, 30.0]);
        assert!((out[0].axis[0][1] - 1.0).abs() < 1e-5 && out[0].axis[0][0].abs() < 1e-5);
        assert!((out[0].axis[2][2] - 1.0).abs() < 1e-5);
        assert_eq!(out[0].scale, 1.0);
    }

    #[test]
    fn a_dynamic_entity_takes_the_nearest_lit_static_models_light() {
        let id = Some(AssetId { ty: crate::asset_type::AssetType::XModel, index: 1 });
        let lit = |x: f32, c: u32| StaticModel { origin: [x, 0.0, 0.0], grid_color: Some(c), ..StaticModel::default() };
        let d = DynEnt { kind: 2, quat: [0.0, 0.0, 0.0, 1.0], origin: [100.0, 0.0, 0.0], model: id, ..DynEnt::default() };
        let out = dyn_ent_models(&clip_with(&[d]), &[lit(0.0, 1), lit(90.0, 2), lit(5000.0, 3)]);
        assert_eq!(out[0].grid_color, Some(2));
        let far = dyn_ent_models(&clip_with(&[d]), &[lit(5000.0, 3)]);
        assert_eq!(far[0].grid_color, None);
    }
}
