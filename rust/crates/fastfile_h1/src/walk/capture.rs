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
    pub state_bits: Vec<[u8; crate::layout::GfxStateBits::SIZE]>,
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
    /// `triIndices2`: the copy the game's model trace reads (u16 x 3 x
    /// tri_count).
    pub tris2: Option<Span>,
    /// (bone offset, vert count, tri offset, tri count)
    pub rigid: Vec<(u16, u16, u16, u16)>,
    /// u16 stream: blend verts (see `XSurface` docs).
    pub blend_verts: Option<Span>,
    pub part_bits: [u32; 8],
    /// The model lightmap unwrap: one 4-byte coordinate pair a vertex
    /// (`TEXCOORD4` of the model lightmap techniques).
    pub lmap_unwrap: Option<Span>,
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
    /// AW's composite models: the part models drawn together (a gun's
    /// base, sight, stock ...), each with its own surfaces.
    pub composite: Vec<Option<AssetId>>,
    /// `XModelCollSurf_s`: one per rigid vertex list of the collision
    /// LOD's surfaces, in order (what the game's model trace pairs them by).
    pub coll_surfs: Vec<CollSurfRec>,
}

/// An `XModelCollSurf_s` (36 bytes): the bounds of one rigid vertex list
/// of the collision LOD, its bone, contents and surface flags.
#[derive(Clone, Copy, Debug, Default)]
pub struct CollSurfRec {
    pub mid: [f32; 3],
    pub half: [f32; 3],
    pub bone: i32,
    pub contents: u32,
    pub surf_flags: u32,
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

/// The root bone's delta motion.
#[derive(Clone, Debug, Default)]
pub struct DeltaRec {
    /// Translation: frame count - 1 (0: a single `frame0`), small (u8) frames.
    pub trans: Option<(u16, bool, Span)>,
    pub quat2: Option<(u16, Span)>,
    pub quat: Option<(u16, Span)>,
}

#[derive(Clone, Debug, Default)]
pub struct WeaponRec {
    pub name: String,
    pub display_name: String,
    /// The WeaponDef's bytes, as stored.
    pub def: Span,
    pub gun_models: Vec<Option<AssetId>>,
    pub world_models: Vec<Option<AssetId>>,
    pub hand_model: Option<AssetId>,
    pub anims: Vec<Option<AssetId>>,
    pub anims_right: Vec<Option<AssetId>>,
    pub anims_left: Vec<Option<AssetId>>,
    pub hide_tags: Vec<u32>,
    pub attachments: Vec<Option<AssetId>>,
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
    pub sound_overrides: Vec<(u32, u32, Option<String>, Option<String>)>,
    pub location_damage: Option<Span>,
    pub accuracy_graph: [Vec<[f32; 2]>; 2],
}

#[derive(Clone, Debug, Default)]
pub struct AttachmentField {
    pub index: u8,
    pub ty: u8,
    pub code: u8,
    pub text: Option<String>,
    pub raw: u64,
    /// `fieldOffsets[i]`: where the field is in the WeaponDef.
    pub offset: u16,
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
    /// Script strings: the gun's tags the attachment hides, and those it
    /// shows (0: none).
    pub hide_tags: Vec<u32>,
    pub show_tags: Vec<u32>,
    /// The weapon fields the attachment changes: (field index, type, op,
    /// the value's text for strings and asset names; else the raw 8 bytes).
    pub fields: Vec<AttachmentField>,
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
    /// A decal's three materials: model, world, world with auto
    /// displacement (`mc/`, `wc/`, `wc/..._autodisplacement`).
    Mark([Option<AssetId>; 3]),
    /// A light definition, vector field or particle simulation.
    Other,
}

/// bo2mp: a `TracerDef`: its name, material and record.
#[derive(Clone, Debug, Default)]
pub struct TracerRec {
    pub name: String,
    pub material: Option<AssetId>,
    pub record: Span,
}

/// bo2mp: an effect table (`ImpactFx`): `IMPACT_FX_ROW` effects per
/// impact type (`[surface types][flesh]`), in table order.
#[derive(Clone, Debug, Default)]
pub struct FxTableRec {
    pub name: String,
    pub cells: Vec<Option<AssetId>>,
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

#[derive(Clone, Debug, Default)]
pub struct RawFileRec {
    pub name: String,
    pub compressed_len: u32,
    pub len: u32,
    pub data: Option<Span>,
}

/// A `ComWorld`: its record and primary lights (`ComPrimaryLight[n]`;
/// light 0 is the "no light" slot, the sun is the first of type 1).
#[derive(Clone, Debug, Default)]
pub struct ComWorldRec {
    pub name: String,
    pub record: Span,
    pub primary_lights: Option<Span>,
    pub primary_light_count: u32,
    /// `ComPrimaryLightEnv`s (10 bytes: four light indices, a count).
    pub primary_light_envs: Option<Span>,
    pub primary_light_env_count: u32,
}

/// A `GfxWorld`: where its arrays are in the file (`None`: not inline).
/// Offsets are file positions in the zone image; counts are as stored.
#[derive(Clone, Debug, Default)]
pub struct GfxWorldRec {
    pub name: String,
    /// The `GfxWorld` header (`layout::GfxWorld`).
    pub header: usize,
    /// `GfxWorldVertex[vertex_count]` (44 bytes: xyz, binormal sign,
    /// colour, uv, lightmap uv, packed normal, packed tangent).
    pub vertices: Option<usize>,
    pub vertex_count: u32,
    /// The vertex layer data (`draw.vld.data`, `vertexLayerDataSize` bytes).
    pub vertex_layer_data: Option<Span>,
    /// `u16[index_count]`.
    pub indices: Option<usize>,
    pub index_count: u32,
    /// `GfxSurface[surface_count]` (40 bytes).
    pub surfaces: Option<usize>,
    pub surface_count: u32,
    pub surface_materials: Vec<Option<AssetId>>,
    /// `GfxSurfaceBounds[surface_count]`.
    pub surface_bounds: Option<usize>,
    /// `u32[static_surface_count]`.
    pub sorted_surfs: Option<usize>,
    pub static_surface_count: u32,
    /// `GfxStaticModelDrawInst[smodel_count]` (80 bytes).
    pub smodels: Option<usize>,
    pub smodel_count: u32,
    pub smodel_models: Vec<Option<AssetId>>,
    /// `GfxStaticModelInst[smodel_count]` (bounds, lighting origin).
    pub smodel_insts: Option<usize>,
    /// `GfxStaticModelLighting[smodel_count]` (24 bytes; a union: ground
    /// lighting, vertex lighting or model lightmap, by the draw's flags).
    pub smodel_lighting: Option<usize>,
    /// Per static model, its vertex lighting values
    /// (`GfxStaticModelVertexLighting[n]`) when stored.
    pub smodel_vertex_lighting: Vec<Option<Span>>,
    /// The light grid's colours (`GfxLightGridColors[colorCount]`, 56 RGB
    /// half floats each).
    pub light_grid_colors: Option<Span>,
    /// The light grid's rows (`rowDataStart`, u16 each), their data
    /// (`rawRowData`) and its entries (`GfxLightGridEntry[entryCount]`).
    pub light_grid_row_start: Option<Span>,
    pub light_grid_rows: Option<Span>,
    pub light_grid_entries: Option<Span>,
    /// The palette light grid: each entry's byte offset into the
    /// bitstream (`paletteEntryAddress`, i32 each) and the bitstream.
    pub light_grid_palette: Option<Span>,
    pub light_grid_bitstream: Option<Span>,
    /// `GfxBrushModel[model_count]`.
    pub models: Option<usize>,
    pub model_count: u32,
    /// Lightmap pages: (primary, secondary).
    pub lightmaps: Vec<(Option<AssetId>, Option<AssetId>)>,
    pub sky_images: Vec<Option<AssetId>>,
    /// Each sky's start surfaces (`skyStartSurfs`, u32 each).
    pub sky_surfaces: Vec<Option<Span>>,
    /// `GfxCell[cell_count]` (bounds, portals).
    pub cells: Option<usize>,
    pub cell_count: u32,
    /// Reflection probe images and `GfxReflectionProbe[n]` (origins).
    pub reflection_probes: Vec<Option<AssetId>>,
    pub reflection_probe_origins: Option<usize>,
    /// `GfxHeroOnlyLight[n]`.
    pub hero_only_lights: Option<Span>,
    pub outdoor_image: Option<AssetId>,
    pub sun_sprite: Option<AssetId>,
    pub sun_flare: Option<AssetId>,
}

/// A `MapEnts`: the entity string, spawn points and trigger models.
#[derive(Clone, Debug, Default)]
pub struct MapEntsRec {
    pub name: String,
    pub entity_string: String,
    /// `SpawnPointEntityRecord[n]` (44 bytes).
    pub spawns: Option<Span>,
    /// The trigger models (`"model" "?N"`): models, hulls, slabs.
    pub triggers: [Option<Span>; 3],
}

/// A `clipMap_t`: where its collision is in the file (see `crate::map`).
/// H1 keeps terrain triangles, partitions and AABB trees in the
/// `ClipInfo`; the fields here are the world info's, as IW6's record.
#[derive(Clone, Debug, Default)]
pub struct ClipMapRec {
    pub name: String,
    pub header: usize,
    /// `vec3[vert_count]`.
    pub verts: Option<usize>,
    pub vert_count: u32,
    /// `u16[3 * tri_count]`.
    pub tri_indices: Option<usize>,
    pub tri_count: u32,
    /// The world's `ClipInfo` (brushes, planes, materials).
    pub info: ClipInfoRec,
    /// `CollisionPartition[partition_count]`.
    pub partitions: Option<usize>,
    pub partition_count: u32,
    /// `CollisionAabbTree[aabb_tree_count]`.
    pub aabb_trees: Option<usize>,
    pub aabb_tree_count: u32,
    /// `cmodel_t[numSubModels]`.
    pub cmodels: Option<usize>,
    pub cmodel_count: u32,
    /// Sub-models whose `ClipInfo` is their own (inline), by index.
    pub cmodel_own_info: Vec<bool>,
    /// `cStaticModel_s[numStaticModels]` and their models.
    pub static_models: Option<usize>,
    pub static_model_models: Vec<Option<AssetId>>,
    /// `DynEntityDef[dynEntCount[k]]` (clutter, destructibles) and their
    /// base models.
    pub dyn_ent_defs: [Option<usize>; 2],
    pub dyn_ent_counts: [u32; 2],
    pub dyn_ent_models: [Vec<Option<AssetId>>; 2],
    /// `ScriptableInstance[scriptable_count]` and their defs.
    pub scriptables: Option<usize>,
    pub scriptable_count: u32,
    pub scriptable_defs: Vec<Option<AssetId>>,
    pub map_ents: Option<AssetId>,
    /// `Stage[stage_count]` (32 bytes: name, origin, trigger index, sun
    /// primary light index, radiometric scale).
    pub stages: Option<usize>,
    pub stage_count: u32,
    /// The stages' trigger models, hulls and slabs (`stageTrigger`).
    pub stage_triggers: [Option<Span>; 3],
}

/// A `ClipInfo`'s arrays, by file offset.
#[derive(Clone, Debug, Default)]
pub struct ClipInfoRec {
    /// `cplane_s[plane_count]` (20 bytes).
    pub planes: Option<usize>,
    pub plane_count: u32,
    /// (name, surface flags, contents).
    pub materials: Vec<(String, i32, i32)>,
    /// `cLeafBrushNode_s[n]`.
    pub leaf_brush_nodes: Option<usize>,
    pub leaf_brush_node_count: u32,
    /// Per leaf brush node, its brush list's file offset (leaves only).
    pub leaf_brush_lists: Vec<Option<usize>>,
    /// `cbrushside_t[n]` (8 bytes: plane index, material, adjacency).
    pub brush_sides: Option<usize>,
    pub brush_side_count: u32,
    /// `cbrush_t[brush_count]` (48 bytes).
    pub brushes: Option<usize>,
    pub brush_count: u32,
    /// Per brush, its first side's file offset.
    pub brush_first_sides: Vec<Option<usize>>,
    /// `Bounds[brush_count]` and `i32[brush_count]`.
    pub brush_bounds: Option<usize>,
    pub brush_contents: Option<usize>,
    pub verts: Option<usize>,
    pub vert_count: u32,
    pub tri_indices: Option<usize>,
    pub tri_count: u32,
    pub partitions: Option<usize>,
    pub partition_count: u32,
    pub aabb_trees: Option<usize>,
    pub aabb_tree_count: u32,
    pub static_models: Option<usize>,
    pub static_model_models: Vec<Option<AssetId>>,
}

/// A `GlassWorld`: its glass pieces (`G_GlassPiece[n]`) and names.
#[derive(Clone, Debug, Default)]
pub struct GlassWorldRec {
    pub name: String,
    pub pieces: Option<Span>,
    /// (name, piece indices as u16).
    pub names: Vec<(String, Option<Span>)>,
}

/// One glass definition of an `FxWorld` (`FxGlassDef`).
#[derive(Clone, Debug, Default)]
pub struct FxGlassDefRec {
    pub half_thickness: f32,
    /// `texVecs` as [[u from x, u from y], [v from x, v from y]].
    pub tex_vecs: [[f32; 2]; 2],
    pub color: [u8; 4],
    pub material: Option<AssetId>,
    pub material_shattered: Option<AssetId>,
    pub is_opaque: bool,
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

/// One technique's passes: its vertex and pixel shaders by name.
#[derive(Clone, Debug, Default)]
pub struct TechniqueRec {
    pub name: String,
    pub passes: Vec<(Option<String>, Option<String>)>,
    /// The technique header's flags.
    pub flags: u16,
    /// Each pass as the game binds it (same order as `passes`).
    pub pass_info: Vec<PassRec>,
}

/// One technique pass: its shaders, vertex declaration, constant-buffer
/// bookkeeping and the argument table that says what goes in each
/// constant-buffer slot and sampler for this pass.
#[derive(Clone, Debug, Default)]
pub struct PassRec {
    pub vertex_shader: Option<String>,
    pub pixel_shader: Option<String>,
    pub hull_shader: Option<String>,
    pub domain_shader: Option<String>,
    pub vertex_decl: Option<String>,
    pub pixel_output_mask: u8,
    /// Argument counts per update rate (per primitive, per object, stable).
    pub arg_counts: [u8; 3],
    /// Constant-buffer sizes in bytes per update rate.
    pub arg_sizes: [u16; 3],
    /// The global constant-buffer index used per update rate.
    pub constant_buffers: [u8; 3],
    pub zone: u16,
    pub custom_buffer_flags: u32,
    pub custom_sampler_flags: u8,
    pub precompiled_index: u8,
    pub stage_config: u8,
    /// Per-primitive args first, then per-object, then stable.
    pub args: Vec<ShaderArgRec>,
}

/// One `MaterialShaderArgument`.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ShaderArgRec {
    /// Argument kind (code constant, code sampler, material constant,
    /// material sampler, literal, ...).
    pub kind: u8,
    /// The shader stage it binds to.
    pub shader: u8,
    /// Destination: a constant-buffer offset or a sampler/texture slot.
    pub dest: u16,
    /// The raw 8-byte union (code constant index/firstRow/rowCount, code
    /// sampler index, or a material name hash).
    pub raw: [u8; 8],
    /// A literal constant's four floats (kind 4).
    pub literal: Option<[f32; 4]>,
}

impl ShaderArgRec {
    /// The union read as a code constant: (index, first row, row count).
    pub fn code_const(&self) -> (u16, u8, u8) {
        (
            u16::from_le_bytes([self.raw[0], self.raw[1]]),
            self.raw[2],
            self.raw[3],
        )
    }
    /// The union read as a 32-bit value (code sampler index or name hash).
    pub fn word(&self) -> u32 {
        u32::from_le_bytes([self.raw[0], self.raw[1], self.raw[2], self.raw[3]])
    }
}

#[derive(Default, Debug)]
pub struct Capture {
    pub techsets: Vec<TechniqueSetRec>,
    /// Every technique this zone loads (its own, not those it refers to).
    pub techniques: Vec<TechniqueRec>,
    /// Every shader's program (DXBC) by (asset type, name).
    pub shader_programs: Vec<(AssetType, String, Span)>,
    /// Vertex declarations: name, stream count, and the routing (source
    /// stream component, destination input, mask) of each stream.
    pub vertex_decls: Vec<(String, u8, Vec<(u8, u8, u8)>)>,
    /// String tables: name, column count, cells row by row.
    pub string_tables: Vec<(String, usize, Vec<String>)>,
    /// Colour look-up tables: name, (width, height, depth), RGBA8 texels.
    pub cluts: Vec<(String, [u32; 3], Option<Span>)>,
    pub materials: Vec<MaterialRec>,
    pub images: Vec<ImageRec>,
    pub xmodels: Vec<XModelRec>,
    pub model_surfs: Vec<ModelSurfsRec>,
    pub xanims: Vec<XAnimRec>,
    pub weapons: Vec<WeaponRec>,
    pub attachments: Vec<AttachmentRec>,
    pub fx: Vec<FxRec>,
    /// bo2mp: tracers (`TracerDef`), in walk order.
    pub tracers: Vec<TracerRec>,
    /// bo2mp: `ImpactFx` tables (`fx/impacts/...`), their cells in order.
    pub impact_tables: Vec<FxTableRec>,
    pub raw_files: Vec<RawFileRec>,
    /// bo2mp: compiled scripts, in walk order.
    pub script_files: Vec<ScriptFileRec>,
    pub com_worlds: Vec<ComWorldRec>,
    pub gfx_worlds: Vec<GfxWorldRec>,
    pub map_ents: Vec<MapEntsRec>,
    pub clip_maps: Vec<ClipMapRec>,
    /// Every `ScriptableDef` this zone loads (its own, in walk order).
    pub scriptable_defs: Vec<crate::scriptable::Def>,
    pub glass_worlds: Vec<GlassWorldRec>,
    pub fx_glass: Vec<FxGlassRec>,
    /// Sound alias lists (exact reads of `sound.rs`'s shape); a shared
    /// loaded sound or sound file reads as the one it names.
    pub sounds: Vec<super::sound::SoundAliasList>,
    /// Loaded sounds (table assets and those read inside sound files).
    pub loaded: Vec<super::sound::LoadedSound>,
    /// Sound files read inline, by VIRTUAL offset (what shared ones name).
    pub sound_files: HashMap<u64, super::sound::SoundFile>,
    /// Sound curves this zone loads (falloff, low-pass): name -> knots.
    pub snd_curves: HashMap<String, Vec<[f32; 2]>>,
    /// Localized strings: key (`WEAPON_AK47`) -> text, as the zone has
    /// them (the `eng_*` zones).
    pub localize: HashMap<String, String>,
    /// Everything else, by name.
    pub others: Vec<OtherRec>,
    /// The table index -> what it loaded.
    pub by_table: HashMap<usize, AssetId>,
    /// Table entries walked.
    pub walked: usize,
    /// Where each walked table entry's header was read (file offset).
    pub starts: Vec<usize>,
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
            T::GlassWorld => self.glass_worlds.get(i).map(|r| r.name.as_str()),
            T::Sound => self.sounds.get(i).map(|r| r.name.as_str()),
            T::LoadedSound => self.loaded.get(i).map(|r| r.name.as_str()),
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
            AssetType::GlassWorld => self.glass_worlds.len(),
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
