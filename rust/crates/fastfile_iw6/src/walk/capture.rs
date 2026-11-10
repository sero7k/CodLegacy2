//! What a walk keeps. Bulk data (vertices, indices, pixels, animation
//! streams) stays in the zone image: records hold its file offsets.

use std::collections::HashMap;

use crate::asset_type::AssetType;
use crate::container::StreamFile;

/// An asset the walk read: its type and its index in that type's list
/// (`Capture::others` for the types kept by name only).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct AssetId {
    pub ty: AssetType,
    pub index: u32,
}

/// A byte range of the zone image.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct Span {
    pub pos: usize,
    pub len: usize,
}

impl Span {
    pub fn get<'a>(&self, image: &'a [u8]) -> &'a [u8] {
        image.get(self.pos..self.pos + self.len).unwrap_or(&[])
    }
}

#[derive(Clone, Debug, Default)]
pub struct TechniqueSetRec {
    pub name: String,
    pub world_vert_format: u8,
    /// (technique index, technique name, pass count)
    pub techniques: Vec<(usize, String, u16)>,
    /// (technique index, index into `Capture::techniques`), for the same
    /// slots as `techniques` whose technique the walk read or had read.
    pub technique_ids: Vec<(usize, u32)>,
}

/// One technique: its name, flags and passes.
#[derive(Clone, Debug, Default)]
pub struct TechniqueRec {
    pub name: String,
    pub flags: u16,
    pub passes: Vec<PassRec>,
}

/// One pass of a technique: its shaders and the arguments that fill their
/// constants and samplers.
#[derive(Clone, Debug, Default)]
pub struct PassRec {
    pub vertex_shader: Option<AssetId>,
    pub vertex_decl: Option<AssetId>,
    pub hull_shader: Option<AssetId>,
    pub domain_shader: Option<AssetId>,
    pub pixel_shader: Option<AssetId>,
    /// Argument counts: per primitive, per object, stable (in that order in
    /// `args`).
    pub arg_counts: [u8; 3],
    /// Byte sizes of the per-primitive, per-object and stable constant
    /// blocks.
    pub arg_sizes: [u16; 3],
    pub args: Vec<ShaderArgRec>,
}

/// One `MaterialShaderArgument`: type, shader stage byte, destination and
/// the eight bytes of its union (code constant index / first row / row
/// count, sampler index or a name hash); `literal` holds the four floats a
/// literal argument points at.
#[derive(Clone, Debug, Default)]
pub struct ShaderArgRec {
    pub ty: u8,
    pub shader: u8,
    pub dest: u16,
    pub u: [u8; 8],
    pub literal: Option<[f32; 4]>,
}

impl ShaderArgRec {
    /// The union read as a code constant: (index, first row, row count).
    pub fn code_const(&self) -> (u16, u8, u8) {
        (
            u16::from_le_bytes([self.u[0], self.u[1]]),
            self.u[2],
            self.u[3],
        )
    }
    /// The union's first u32 (a sampler index or a name hash).
    pub fn u32(&self) -> u32 {
        u32::from_le_bytes([self.u[0], self.u[1], self.u[2], self.u[3]])
    }
}

#[derive(Clone, Debug, Default)]
pub struct MaterialTexture {
    pub name_hash: u32,
    pub name_start: u8,
    pub name_end: u8,
    pub sampler_state: u8,
    pub semantic: u8,
    pub image: Option<AssetId>,
}

#[derive(Clone, Debug, Default)]
pub struct MaterialConstant {
    pub name_hash: u32,
    pub name: String,
    pub literal: [f32; 4],
}

#[derive(Clone, Debug, Default)]
pub struct MaterialRec {
    pub name: String,
    pub game_flags: u8,
    pub sort_key: u8,
    pub texture_atlas: (u8, u8),
    pub state_flags: u8,
    pub camera_region: u8,
    pub material_type: u8,
    pub techset: Option<AssetId>,
    pub textures: Vec<MaterialTexture>,
    pub constants: Vec<MaterialConstant>,
    /// GfxStateBits, 28 bytes each.
    pub state_bits: Vec<[u8; 28]>,
    pub state_bits_entry: Vec<u8>,
}

#[derive(Clone, Debug, Default)]
pub struct ImageRec {
    pub name: String,
    /// DXGI_FORMAT.
    pub format: u32,
    pub map_type: u8,
    pub semantic: u8,
    pub category: u8,
    pub flags: u8,
    pub width: u16,
    pub height: u16,
    pub depth: u16,
    pub num_elements: u16,
    pub level_count: u8,
    pub streamed: bool,
    pub data_len1: u32,
    pub data_len2: u32,
    /// Inline pixels (all levels).
    pub pixels: Option<Span>,
    /// Streamed: per stream (biggest last): (width, height, cumulative pixel
    /// bytes, level count), and its `imagefile<N>.pak` range.
    pub streams: [(u16, u16, u32, u8); 4],
    pub stream_files: [Option<StreamFile>; 4],
}

#[derive(Clone, Debug, Default)]
pub struct SurfaceRec {
    pub flags: u16,
    pub vert_count: u16,
    pub tri_count: u16,
    pub blend_vert_counts: [u16; 8],
    /// `GfxPackedVertex` x vert_count (32 bytes each).
    pub verts: Option<Span>,
    /// u16 x 3 x tri_count.
    pub tris: Option<Span>,
    /// (bone offset, vert count, tri offset, tri count)
    pub rigid: Vec<(u16, u16, u16, u16)>,
    /// u16 stream: blend verts (see `XSurface` docs).
    pub blend_verts: Option<Span>,
    pub part_bits: [u32; 8],
}

#[derive(Clone, Debug, Default)]
pub struct ModelSurfsRec {
    pub name: String,
    pub surfs: Vec<SurfaceRec>,
}

#[derive(Clone, Debug, Default)]
pub struct LodRec {
    pub dist: f32,
    pub numsurfs: u16,
    pub surf_index: u16,
    pub surfs: Option<AssetId>,
}

#[derive(Clone, Debug, Default)]
pub struct XModelRec {
    pub name: String,
    pub num_bones: u8,
    pub num_root_bones: u8,
    pub numsurfs: u8,
    pub scale: f32,
    /// Script string indices.
    pub bone_names: Vec<u32>,
    pub parent_list: Vec<u8>,
    /// i16 x 4 quaternions per non-root bone.
    pub tag_angles: Vec<[i16; 4]>,
    pub tag_positions: Vec<[f32; 3]>,
    pub part_classification: Vec<u8>,
    /// DObjAnimMat per bone: quat[4], trans[3], transWeight.
    pub base_mat: Vec<[f32; 8]>,
    pub materials: Vec<Option<AssetId>>,
    pub lods: Vec<LodRec>,
    pub num_lods: u8,
    pub coll_lod: i8,
    pub flags: u8,
    pub contents: i32,
    pub radius: f32,
    pub bounds: ([f32; 3], [f32; 3]),
    pub phys_preset: Option<AssetId>,
    pub phys_collmap: Option<AssetId>,
    /// bo2mp: `XModelCollSurf_s[numCollSurfs]` (bounds, bone, contents,
    /// surface flags: Ghosts keeps no collision triangles in them).
    pub coll_surfs: Option<Span>,
}

#[derive(Clone, Debug, Default)]
pub struct XAnimRec {
    pub name: String,
    pub data_byte_count: u16,
    pub data_short_count: u16,
    pub data_int_count: u16,
    pub random_data_byte_count: u16,
    pub random_data_int_count: u16,
    pub numframes: u16,
    pub flags: u8,
    pub bone_count: [u8; 10],
    pub notify_count: u8,
    pub asset_type: u8,
    pub ik_type: u8,
    pub random_data_short_count: u32,
    pub index_count: u32,
    pub framerate: f32,
    pub frequency: f32,
    pub names: Vec<u32>,
    /// (script string, time)
    pub notify: Vec<(u32, f32)>,
    pub delta: Option<DeltaRec>,
    pub data_byte: Option<Span>,
    pub data_short: Option<Span>,
    pub data_int: Option<Span>,
    pub random_data_short: Option<Span>,
    pub random_data_byte: Option<Span>,
    pub random_data_int: Option<Span>,
    pub indices: Option<Span>,
}

/// Positions in the zone image of a delta translation's parts: with `size` 0 the single frame (3
/// floats) is at `head`; else `head` holds mins (3 floats), step (3 floats) and the frames pointer,
/// `indices` the frame numbers (bytes under 256 frames, else u16s) and `frames` the values
/// (3 bytes each when small, else 3 u16s), `size + 1` of each.
#[derive(Clone, Copy, Debug, Default)]
pub struct DeltaTransAt {
    pub head: Option<usize>,
    pub indices: Option<usize>,
    pub frames: Option<usize>,
}

/// The root bone's delta motion.
#[derive(Clone, Debug, Default)]
pub struct DeltaRec {
    /// Translation: frame count - 1 (0: a single `frame0`), small (u8) frames.
    pub trans: Option<(u16, bool, Span)>,
    /// Where the translation's parts stand in the image (`fastfile_iw6::anim` decodes them).
    pub trans_at: Option<DeltaTransAt>,
    pub quat2: Option<(u16, Span)>,
    pub quat: Option<(u16, Span)>,
}

#[derive(Clone, Debug, Default)]
pub struct WeaponRec {
    pub name: String,
    pub display_name: String,
    /// The WeaponCompleteDef and WeaponDef bytes, as stored.
    pub complete: Span,
    pub def: Option<Span>,
    pub gun_models: Vec<Option<AssetId>>,
    pub world_models: Vec<Option<AssetId>>,
    pub hand_model: Option<AssetId>,
    pub anims: Vec<Option<AssetId>>,
    pub anims_right: Vec<Option<AssetId>>,
    pub anims_left: Vec<Option<AssetId>>,
    pub hide_tags: Vec<u32>,
    pub scopes: Vec<Option<AssetId>>,
    pub under_barrels: Vec<Option<AssetId>>,
    pub others: Vec<Option<AssetId>>,
    /// (field offset in WeaponDef, alias name)
    pub sounds: Vec<(usize, String)>,
    /// (field offset in WeaponDef, effect)
    pub fx: Vec<(usize, AssetId)>,
    /// (field offset in WeaponDef, model)
    pub models: Vec<(usize, AssetId)>,
    /// (field offset in WeaponDef or WeaponCompleteDef, material)
    pub materials: Vec<(usize, AssetId)>,
    /// (field offset in WeaponDef, string)
    pub strings: Vec<(usize, String)>,
    pub notetrack_sound_keys: Vec<u32>,
    pub notetrack_sound_values: Vec<u32>,
    pub alt_weapon_name: String,
    pub tracer: Option<AssetId>,
    pub anim_overrides: Vec<AnimOverride>,
    /// (attachment1, attachment2, overrideSound, altmodeSound, soundType):
    /// soundType 1 fire, 2 player fire, 3 player akimbo fire, 4 last shot
    /// (docs/exe_notes/IW6_VFX_AUDIO.md, fire sounds).
    pub sound_overrides: Vec<(u32, u32, Option<String>, Option<String>, u32)>,
    /// bo2mp: `FXOverrideEntry`s: attachments, `fxType`, the effect and its
    /// alternate mode's.
    pub fx_overrides: Vec<FxOverride>,
    pub location_damage: Option<Span>,
    pub accuracy_graph: [Vec<[f32; 2]>; 2],
}

/// bo2mp: one attachment effect override of a weapon.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FxOverride {
    pub attachment1: u32,
    pub attachment2: u32,
    pub fx_type: u32,
    pub override_fx: Option<AssetId>,
    pub altmode_fx: Option<AssetId>,
}

#[derive(Clone, Debug, Default)]
pub struct AnimOverride {
    pub attachment1: u32,
    pub attachment2: u32,
    pub anim_index: u32,
    pub override_anim: Option<AssetId>,
    pub altmode_anim: Option<AssetId>,
    pub anim_time: i32,
    pub altmode_time: i32,
}

#[derive(Clone, Debug, Default)]
pub struct AttachmentRec {
    pub name: String,
    pub display_name: String,
    pub ty: i32,
    pub weap_type: i32,
    pub weap_class: i32,
    pub record: Span,
    pub world_models: Vec<Option<AssetId>>,
    pub view_models: Vec<Option<AssetId>>,
    pub reticle_view_models: Vec<Option<AssetId>>,
    /// The sub-structs present (name, span).
    pub parts: Vec<(&'static str, Span)>,
}

#[derive(Clone, Debug, Default)]
pub struct FxRec {
    pub name: String,
    pub flags: i32,
    pub elem_counts: (i32, i32, i32),
    /// Per element: type, visual count, visuals (materials, models, ...).
    pub elems: Vec<FxElemRec>,
    pub record: Span,
}

#[derive(Clone, Debug, Default)]
pub struct FxElemRec {
    pub elem_type: u8,
    pub record: Span,
    pub materials: Vec<AssetId>,
    pub models: Vec<AssetId>,
    /// Every effect the element names: its runners' children, then its
    /// impact, death and emitted effects.
    pub effects: Vec<String>,
    pub sounds: Vec<String>,
    /// bo2mp: `FxElemVelStateSample` x (velIntervalCount + 1).
    pub vel_samples: Option<Span>,
    /// bo2mp: `FxElemVisStateSample` x (visStateIntervalCount + 1).
    pub vis_samples: Option<Span>,
    /// bo2mp: the visuals in order, one per `visualCount` (a null one too).
    pub visuals: Vec<FxVisualRec>,
    /// bo2mp: `effectOnImpact`, `effectOnDeath`, `effectEmitted`.
    pub effect_on_impact: Option<String>,
    pub effect_on_death: Option<String>,
    pub effect_emitted: Option<String>,
    /// bo2mp: a trail's `FxTrailDef` and its vertices and indices.
    pub trail: Option<FxTrailRec>,
    /// bo2mp: the `extended` record of a spark fountain or a light.
    pub extended: Option<Span>,
    /// bo2mp: a flare's `FxFlareDef` and its four gradient tables (the
    /// screen x and y intensity, the source cosine intensity and scale).
    pub flare: Option<FxFlareRec>,
}

/// bo2mp: a flare element's definition: the 112-byte record and its tables
/// (each `intervalCount + 1` floats).
#[derive(Clone, Copy, Debug, Default)]
pub struct FxFlareRec {
    pub def: Span,
    pub tables: [Span; 4],
}

/// bo2mp: one visual of an effect element.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum FxVisualRec {
    #[default]
    None,
    Material(AssetId),
    Model(AssetId),
    Effect(String),
    Sound(String),
    /// A decal's two materials (model, world).
    Mark([Option<AssetId>; 2]),
    /// A light definition, vector field or particle simulation.
    Other,
}

/// bo2mp: an `FxTrailDef`.
#[derive(Clone, Copy, Debug, Default)]
pub struct FxTrailRec {
    pub record: Span,
    /// `FxTrailVertex` x vertCount.
    pub verts: Option<Span>,
    /// u16 x indCount.
    pub inds: Option<Span>,
}

#[derive(Clone, Debug, Default)]
pub struct OtherRec {
    pub ty: Option<AssetType>,
    pub name: String,
}

/// A compiled script (`ScriptFile`): its buffer (zlib, `compressed_len`
/// bytes, `len` inflated) and its bytecode (`bytecode_len`).
#[derive(Clone, Debug, Default)]
pub struct ScriptFileRec {
    pub name: String,
    pub compressed_len: u32,
    pub len: u32,
    pub bytecode_len: u32,
    pub buffer: Option<Span>,
    pub bytecode: Option<Span>,
}

/// bo2mp: a `StringTable`'s cells.
#[derive(Clone, Debug, Default)]
pub struct StringTableRec {
    pub name: String,
    pub columns: usize,
    pub rows: usize,
    pub cells: Vec<String>,
}

impl StringTableRec {
    pub fn cell(&self, row: usize, column: usize) -> &str {
        if column >= self.columns {
            return "";
        }
        self.cells.get(row * self.columns + column).map_or("", String::as_str)
    }
}

#[derive(Clone, Debug, Default)]
pub struct RawFileRec {
    pub name: String,
    pub compressed_len: u32,
    pub len: u32,
    pub data: Option<Span>,
}

/// A `ToneMapping` asset's floats (`layout::ToneMapping`, 26 from the header's
/// start: toe strength, toe angle numerator, toe angle denominator, linear
/// strength, linear angle, shoulder strength, white point, exposure as three
/// floats each, then two adaptation rates).
/// bo2mp: a `GfxLightDef`: its attenuation (falloff) and cucoloris
/// pictures and `lmapLookupStart`, where the compiler placed its falloff
/// ramp in the lightmap pages.
#[derive(Clone, Debug, Default)]
pub struct LightDefRec {
    pub name: String,
    pub attenuation: Option<String>,
    pub cucoloris: Option<String>,
    pub lmap_lookup_start: i32,
}

#[derive(Clone, Debug, Default)]
pub struct ToneMappingRec {
    pub name: String,
    pub v: [f32; 26],
}

/// bo2mp: a `Colorization` (HDR colour grade): power (xyz) and
/// desaturation (w), scale, bias.
#[derive(Clone, Debug, Default)]
pub struct ColorizationRec {
    pub name: String,
    pub power_and_desaturation: [f32; 4],
    pub scale: [f32; 3],
    pub bias: [f32; 3],
}

/// bo2mp: a `ColorizationSet`: its end points and colorizations (main,
/// alternate, low, mid, high) by lower-case name.
#[derive(Clone, Debug, Default)]
pub struct ColorizationSetRec {
    pub name: String,
    pub low_and_high_end_points: [f32; 4],
    pub mid_end_points: [f32; 4],
    /// The near/far blend by distance (`COLORIZATION_DEPTH_SCALE_AND_BIAS`).
    pub alternate_depth_scale_and_bias: [f32; 2],
    pub flags: u32,
    pub colorizations: [Option<String>; 5],
}

#[derive(Clone, Debug, Default)]
pub struct ComWorldRec {
    pub name: String,
    pub record: Span,
    pub primary_lights: Option<Span>,
    pub primary_light_count: u32,
    /// bo2mp: `ComPrimaryLightEnv[primaryLightEnvCount]` (up to four
    /// primary light indices and their count, 10 bytes each).
    pub primary_light_envs: Option<Span>,
    /// bo2mp: each primary light's light def name (`defName`).
    pub primary_light_defs: Vec<Option<String>>,
}

/// bo2mp: the map's `sunflare_t` (`GfxWorld.sun`): the sun sprite and lens
/// flare, and how the sun darkens (blind) and lightens (glare) the view.
/// Angles are stored as cosines (`...Dot`), times in milliseconds.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct SunFlareRec {
    pub has_valid_data: bool,
    pub sprite_size: f32,
    pub flare_min_size: f32,
    pub flare_min_dot: f32,
    pub flare_max_size: f32,
    pub flare_max_dot: f32,
    pub flare_max_alpha: f32,
    pub flare_fade_in: i32,
    pub flare_fade_out: i32,
    pub blind_min_dot: f32,
    pub blind_max_dot: f32,
    pub blind_max_darken: f32,
    pub blind_fade_in: i32,
    pub blind_fade_out: i32,
    pub glare_min_dot: f32,
    pub glare_max_dot: f32,
    pub glare_max_lighten: f32,
    pub glare_fade_in: i32,
    pub glare_fade_out: i32,
    /// The direction toward the sun (`sunFxPosition`).
    pub position: [f32; 3],
}

/// A `GfxWorld`: where its arrays are in the file (`None`: not inline).
#[derive(Clone, Debug, Default)]
pub struct GfxWorldRec {
    /// bo2mp: the map's sun sprite / flare / blind / glare values and the
    /// two materials.
    pub sun: Option<SunFlareRec>,
    pub sun_sprite: Option<super::AssetId>,
    pub sun_flare: Option<super::AssetId>,
    pub name: String,
    pub header: usize,
    pub vertices: Option<usize>,
    pub vertex_count: u32,
    /// bo2mp: the vertex layer stream (`vld.data`, `vertexLayerDataSize`
    /// bytes): layered surfaces' second vertex stream, each surface's run
    /// at its `vertexLayerData` byte offset.
    pub vertex_layer_data: Option<Span>,
    pub indices: Option<usize>,
    pub index_count: u32,
    pub surfaces: Option<usize>,
    pub surface_count: u32,
    pub surface_materials: Vec<Option<super::AssetId>>,
    pub sorted_surfs: Option<usize>,
    pub static_surface_count: u32,
    pub smodels: Option<usize>,
    pub smodel_count: u32,
    pub smodel_models: Vec<Option<super::AssetId>>,
    /// bo2mp: each placed model's baked vertex light
    /// (`GfxStaticModelVertexLighting[numLightingValues]`, 20 bytes a
    /// vertex: visibility, ambient and highlight colours as halves), when
    /// it has one.
    pub smodel_vertex_lighting: Vec<Option<Span>>,
    /// bo2mp: `GfxStaticModelInst[smodel_count]` (bounds, lighting origin).
    pub smodel_insts: Option<usize>,
    pub models: Option<usize>,
    pub model_count: u32,
    pub lightmaps: Vec<(Option<super::AssetId>, Option<super::AssetId>)>,
    pub sky_images: Vec<Option<super::AssetId>>,
    /// bo2mp: `GfxSurfaceBounds[surfaceCount]`.
    pub surface_bounds: Option<usize>,
    /// bo2mp: each sky's start surfaces (`skyStartSurfs`).
    pub sky_surfaces: Vec<Option<Span>>,
    /// bo2mp: where the light grid's arrays are in the file.
    pub light_grid: Option<LightGridRec>,
    /// bo2mp: the world's reflection probes, in file order.
    pub reflection_probes: Vec<ReflectionProbeRec>,
    /// bo2mp: `vec3[reflection_probe_reference_count]` (the places that pick
    /// a probe) and the `u8` probe index for each.
    pub reflection_probe_reference_count: u32,
    pub reflection_probe_reference_origins: Option<usize>,
    pub reflection_probe_references: Option<usize>,
    /// bo2mp: the DPVS: `cplane_s[plane_count]` and `u16[node_count]` (the
    /// tree that finds a point's cell), the cell count, and each cell's
    /// records.
    pub dpvs_planes: Option<usize>,
    pub plane_count: u32,
    pub dpvs_nodes: Option<usize>,
    pub node_count: u32,
    pub cell_count: u32,
    pub cells: Vec<CellRec>,
}

/// bo2mp: one DPVS cell: where its `GfxCell` record is, each portal's
/// `GfxPortal` record and vertex array, and each node of its AABB tree
/// (`GfxAabbTree` record, its static model index array).
#[derive(Clone, Debug, Default)]
pub struct CellRec {
    pub at: Option<usize>,
    pub portals: Vec<(usize, Option<usize>)>,
    pub tree: Vec<(usize, Option<usize>)>,
}

/// bo2mp: one reflection probe. `origin` is the record's four floats (the
/// capture position, then the fourth value as stored); `image` is its cube
/// map; `volumes` holds `volume_count` records of 96 bytes (planes bounding
/// the probe's influence).
#[derive(Clone, Debug, Default)]
pub struct ReflectionProbeRec {
    pub origin: [f32; 4],
    pub image: Option<super::AssetId>,
    pub volume_count: u32,
    pub volumes: Option<usize>,
}

/// bo2mp: a `GfxLightGrid`'s header values and where its arrays are in the
/// file (`None`: not inline). `colors` holds `GfxLightGridColors` records of
/// 56 RGB8 samples each.
#[derive(Clone, Debug, Default)]
pub struct LightGridRec {
    pub has_light_regions: bool,
    pub row_axis: u32,
    pub col_axis: u32,
    pub mins: [u16; 3],
    pub maxs: [u16; 3],
    pub row_data_start: Option<usize>,
    pub row_data_start_count: usize,
    pub raw_row_data: Option<usize>,
    pub raw_row_data_size: usize,
    pub entries: Option<usize>,
    pub entry_count: usize,
    pub colors: Option<usize>,
    pub color_count: usize,
    pub missing_color_index: u32,
    pub table_version: u32,
    pub palette_version: u32,
    pub palette_entry_count: usize,
    pub palette_entries: Option<usize>,
    pub palette_bitstream_size: usize,
    pub palette_bitstream: Option<usize>,
    pub range_exponents: [u8; 3],
    pub tree_max_depth: u32,
    pub tree_node_count: usize,
    pub tree_nodes: Option<usize>,
    pub tree_leaf_count: u32,
    pub tree_leaf_table_size: usize,
    pub tree_leaf_table: Option<usize>,
    pub tree_default_color_index_bits: u32,
    pub tree_default_light_index_bits: u32,
    pub tree_coord_min: [i32; 3],
    pub tree_coord_max: [i32; 3],
    pub tree_coord_half_size: [i32; 3],
}

/// A `MapEnts`: the entity string and the spawn points' records.
#[derive(Clone, Debug, Default)]
pub struct MapEntsRec {
    pub name: String,
    pub entity_string: String,
    pub spawns: Option<Span>,
    /// bo2mp: the trigger models (`"model" "?N"`): models, hulls, slabs.
    pub triggers: [Option<Span>; 3],
}

/// A `clipMap_t`: where its collision is in the file (see `crate::map`).
#[derive(Clone, Debug, Default)]
pub struct ClipMapRec {
    pub name: String,
    pub header: usize,
    pub verts: Option<usize>,
    pub vert_count: u32,
    pub tri_indices: Option<usize>,
    pub tri_count: u32,
    /// bo2mp: the world's `ClipInfo` (brushes, planes, materials).
    pub info: ClipInfoRec,
    pub partitions: Option<usize>,
    pub partition_count: u32,
    pub aabb_trees: Option<usize>,
    pub aabb_tree_count: u32,
    /// `cmodel_t[numSubModels]`.
    pub cmodels: Option<usize>,
    pub cmodel_count: u32,
    /// Sub-models whose `ClipInfo` is their own (inline), by index.
    pub cmodel_own_info: Vec<bool>,
    /// `cStaticModel_s[numStaticModels]` and their models.
    pub static_models: Option<usize>,
    pub static_model_models: Vec<Option<super::AssetId>>,
    /// `DynEntityDef[dynEntCount[k]]` (clutter, destructibles) and their
    /// base models.
    pub dyn_ent_defs: [Option<usize>; 2],
    pub dyn_ent_counts: [u32; 2],
    pub dyn_ent_models: [Vec<Option<super::AssetId>>; 2],
    /// The map's scriptable instances (where each stands and which
    /// `ScriptableDef` it is; the def's base model is
    /// `Capture::scriptable_models`).
    pub scriptables: Vec<ScriptableInstRec>,
}

/// A `ScriptableInstance`'s placement.
#[derive(Clone, Copy, Debug)]
pub struct ScriptableInstRec {
    pub def: AssetId,
    pub origin: [f32; 3],
    pub angles: [f32; 3],
}

/// bo2mp: a `ClipInfo`'s arrays, by file offset.
#[derive(Clone, Debug, Default)]
pub struct ClipInfoRec {
    pub planes: Option<usize>,
    pub plane_count: u32,
    /// (name, surface flags, contents).
    pub materials: Vec<(String, i32, i32)>,
    /// A brush side's file offset -> its plane's file offset.
    pub side_planes: HashMap<usize, usize>,
    pub leaf_brush_nodes: Option<usize>,
    pub leaf_brush_node_count: u32,
    /// Per leaf brush node, its brush list's file offset (leaves only).
    pub leaf_brush_lists: Vec<Option<usize>>,
    pub brushes: Option<usize>,
    pub brush_count: u32,
    /// Per brush, its first side's file offset.
    pub brush_sides: Vec<Option<usize>>,
    pub brush_bounds: Option<usize>,
    pub brush_contents: Option<usize>,
}

/// bo2mp: a `TracerDef`'s values.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TracerRec {
    pub name: String,
    pub material: Option<String>,
    pub draw_interval: i32,
    pub speed: f32,
    pub beam_length: f32,
    pub beam_width: f32,
    pub screw_radius: f32,
    pub screw_dist: f32,
    pub colors: [[f32; 4]; 5],
}

/// bo2mp: an `FxImpactTable`: row-major cells (row * 38 + column).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ImpactTableRec {
    pub name: String,
    pub cells: Vec<Option<String>>,
}

/// bo2mp: a `PathData` asset as read (`crate::pathdata` turns it into `PathData`).
#[derive(Clone, Debug, Default)]
pub struct PathDataRec {
    pub name: String,
    pub version: u16,
    /// `pathnode_t` records: the 176 raw bytes of each node (the constant part is 80 bytes, the
    /// engine's dynamic and transient parts are runtime state).
    pub nodes: Vec<Span>,
    /// Per node: the links read (`pathlink_s`, 12 bytes each) as a span, with the count.
    pub links: Vec<(Option<Span>, usize)>,
    /// The triangular node-to-node visibility bits (`visBytes` bytes).
    pub vis: Option<Span>,
    /// The k-d tree the engine finds the nearest node with: (axis, dist, children or leaf).
    pub tree: Vec<PathTreeRec>,
    pub exposure: Option<Span>,
    pub no_peek_vis: Option<Span>,
    pub zone_count: u32,
    pub zones: Option<Span>,
    pub dyn_states: Option<Span>,
    pub dyn_group_count: u32,
    /// Dynamic node groups (a door's nodes): (parent index, the group's own tree).
    pub dyn_groups: Vec<(u32, Vec<PathTreeRec>)>,
}

/// One `pathnode_tree_t`: an inner entry splits on `axis` (0 x, 1 y, 2 z) at `dist`, with two
/// children (indices into the tree); a leaf (axis -1) lists node numbers.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PathTreeRec {
    pub axis: i32,
    pub dist: f32,
    pub children: [Option<u32>; 2],
    pub leaf: Vec<u16>,
}

/// One glass definition of an `FxWorld` (`FxGlassDef`).
#[derive(Clone, Debug, Default)]
pub struct FxGlassDefRec {
    pub half_thickness: f32,
    /// `texVecs` as [[u from x, u from y], [v from x, v from y]].
    pub tex_vecs: [[f32; 2]; 2],
    pub material: Option<AssetId>,
    pub material_shattered: Option<AssetId>,
}

/// An `FxWorld`'s intact glass: its definitions, the initial pieces
/// (`FxGlassInitPieceState`, 56 bytes each) and their geometry words
/// (`FxGlassGeometryData`, 4 bytes each: per piece `vertCount` vertex words
/// then `fanDataCount` triangle-index words).
#[derive(Clone, Debug, Default)]
pub struct FxGlassRec {
    pub name: String,
    pub defs: Vec<FxGlassDefRec>,
    pub init_piece_count: usize,
    pub init_pieces: Option<Span>,
    pub init_geo_count: usize,
    pub init_geo: Option<Span>,
}

#[derive(Default, Debug)]
pub struct Capture {
    /// Each `FxWorld`'s intact breakable glass.
    pub fx_glass: Vec<FxGlassRec>,
    pub path_data: Vec<PathDataRec>,
    pub techsets: Vec<TechniqueSetRec>,
    /// Every technique the walk read (shared ones once).
    pub techniques: Vec<TechniqueRec>,
    /// Where each technique's header was read (file offset) -> its index in
    /// `techniques`; a later set's reference finds it here.
    pub technique_at: HashMap<usize, u32>,
    /// Shader programs (DXBC) by the shader asset that holds them.
    pub shader_programs: HashMap<AssetId, Span>,
    pub materials: Vec<MaterialRec>,
    pub images: Vec<ImageRec>,
    pub xmodels: Vec<XModelRec>,
    pub model_surfs: Vec<ModelSurfsRec>,
    pub xanims: Vec<XAnimRec>,
    pub weapons: Vec<WeaponRec>,
    pub attachments: Vec<AttachmentRec>,
    pub fx: Vec<FxRec>,
    pub raw_files: Vec<RawFileRec>,
    /// Tone mapping assets by lower-case name.
    pub tone_mappings: HashMap<String, ToneMappingRec>,
    /// bo2mp: colorizations and colorization sets by lower-case name.
    pub colorizations: HashMap<String, ColorizationRec>,
    pub colorization_sets: HashMap<String, ColorizationSetRec>,
    /// bo2mp: light defs by lower-case name.
    pub light_defs: HashMap<String, LightDefRec>,
    pub script_files: Vec<ScriptFileRec>,
    /// Localized strings (reference name, text) of the zone.
    pub localized: Vec<(String, String)>,
    pub com_worlds: Vec<ComWorldRec>,
    pub gfx_worlds: Vec<GfxWorldRec>,
    pub map_ents: Vec<MapEntsRec>,
    pub clip_maps: Vec<ClipMapRec>,
    /// Sound alias lists (exact reads of `sound.rs`'s shape); a shared
    /// loaded sound or sound file reads as the one it names.
    pub sounds: Vec<crate::sound::SoundAliasList>,
    /// Loaded sounds (table assets and those read inside sound files).
    pub loaded: Vec<crate::sound::LoadedSound>,
    /// Sound files read inline, by VIRTUAL offset (what shared ones name).
    pub sound_files: HashMap<u64, crate::sound::SoundFile>,
    /// bo2mp: sound curves' knots and Doppler presets, by index in `others`.
    pub snd_curves: HashMap<u32, crate::sound::SndCurve>,
    pub doppler_presets: HashMap<u32, crate::sound::DopplerPreset>,
    /// bo2mp: occlusion shapes read inline, by VIRTUAL offset.
    pub occlusion_shapes: HashMap<u64, crate::sound::OcclusionShape>,
    /// bo2mp: string tables' cells (row-major), by table name.
    pub string_tables: Vec<StringTableRec>,
    /// bo2mp: tracer definitions by lower-case name.
    pub tracers: HashMap<String, TracerRec>,
    /// bo2mp: impact effect tables (`ImpactFx`): 15 rows of 38 cells, the
    /// effect each names (`None` for an empty cell).
    pub impact_tables: Vec<ImpactTableRec>,
    /// Everything else, by name.
    pub others: Vec<OtherRec>,
    /// The table index -> what it loaded.
    pub by_table: HashMap<usize, AssetId>,
    /// Table entries walked.
    pub walked: usize,
    /// Where each walked table entry's header was read (file offset).
    pub starts: Vec<usize>,
    /// bo2mp: each scriptable's header (file offset), by its index in
    /// `others` (scriptables load inline, where no table entry points).
    pub scriptable_headers: HashMap<u32, usize>,
    /// bo2mp: each scriptable's base model (the model it shows before any
    /// state changes it), by its index in `others`.
    pub scriptable_models: HashMap<u32, Option<AssetId>>,
    /// bo2mp: the effects each scriptable's parts play on entering their
    /// first state (the state a placed instance starts in), by the def's
    /// index in `others`: (part, effect, loop time in ms (0: once), the
    /// script string index of its tag).
    pub scriptable_fx: HashMap<u32, Vec<(u8, Option<AssetId>, u16, u32)>>,
}

impl Capture {
    pub(crate) fn push_other(&mut self, ty: AssetType, name: String) -> AssetId {
        self.others.push(OtherRec { ty: Some(ty), name });
        AssetId {
            ty,
            index: (self.others.len() - 1) as u32,
        }
    }

    /// The name of any captured asset.
    pub fn name_of(&self, id: AssetId) -> &str {
        let i = id.index as usize;
        use AssetType as T;
        let s: Option<&str> = match id.ty {
            T::TechniqueSet => self.techsets.get(i).map(|r| r.name.as_str()),
            T::Material => self.materials.get(i).map(|r| r.name.as_str()),
            T::Image => self.images.get(i).map(|r| r.name.as_str()),
            T::XModel => self.xmodels.get(i).map(|r| r.name.as_str()),
            T::XModelSurfs => self.model_surfs.get(i).map(|r| r.name.as_str()),
            T::XAnimParts => self.xanims.get(i).map(|r| r.name.as_str()),
            T::Weapon => self.weapons.get(i).map(|r| r.name.as_str()),
            T::Attachment => self.attachments.get(i).map(|r| r.name.as_str()),
            T::Fx => self.fx.get(i).map(|r| r.name.as_str()),
            T::RawFile => self.raw_files.get(i).map(|r| r.name.as_str()),
            T::ComWorld => self.com_worlds.get(i).map(|r| r.name.as_str()),
            T::GfxWorld => self.gfx_worlds.get(i).map(|r| r.name.as_str()),
            T::MapEnts => self.map_ents.get(i).map(|r| r.name.as_str()),
            T::ClipMap => self.clip_maps.get(i).map(|r| r.name.as_str()),
            T::Sound => self.sounds.get(i).and_then(|r| r.name.text()),
            T::LoadedSound => self.loaded.get(i).and_then(|r| r.name.text()),
            _ => self.others.get(i).map(|r| r.name.as_str()),
        };
        s.unwrap_or("")
    }

    /// Every captured asset of a type with its name, in capture order.
    pub fn names(&self, ty: AssetType) -> Vec<String> {
        let n = match ty {
            AssetType::TechniqueSet => self.techsets.len(),
            AssetType::Material => self.materials.len(),
            AssetType::Image => self.images.len(),
            AssetType::XModel => self.xmodels.len(),
            AssetType::XModelSurfs => self.model_surfs.len(),
            AssetType::XAnimParts => self.xanims.len(),
            AssetType::Weapon => self.weapons.len(),
            AssetType::Attachment => self.attachments.len(),
            AssetType::Fx => self.fx.len(),
            AssetType::RawFile => self.raw_files.len(),
            AssetType::ComWorld => self.com_worlds.len(),
            AssetType::GfxWorld => self.gfx_worlds.len(),
            AssetType::MapEnts => self.map_ents.len(),
            AssetType::ClipMap => self.clip_maps.len(),
            AssetType::Sound => self.sounds.len(),
            AssetType::LoadedSound => self.loaded.len(),
            _ => {
                return self
                    .others
                    .iter()
                    .filter(|o| o.ty == Some(ty))
                    .map(|o| o.name.clone())
                    .collect();
            }
        };
        (0..n)
            .map(|i| {
                self.name_of(AssetId {
                    ty,
                    index: i as u32,
                })
                .to_owned()
            })
            .collect()
    }
}
