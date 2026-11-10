//! What a walk keeps besides the asset list: the weapons' records, and
//! for following a weapon to its art, the models (LOD meshes and their
//! materials), materials (technique set and images) and images (size,
//! format, where the pixels are: in the zone or the `.xpak` entries).

use std::collections::HashMap;

use crate::asset_type::AssetType;
use crate::walk::image::XPakEntry;

/// What a weapon field names.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FieldValue {
    /// An asset of `ty`: its name when it was loaded here or by an earlier
    /// asset the reference names (`<ref 0x...>` when the walk cannot tell).
    Asset { ty: AssetType, name: String },
    /// A string (sound alias, weapon name, tag, hint, ...).
    Str(String),
}

/// One weapon (`WeaponVariantDef` and its `WeaponDef`).
#[derive(Clone, Debug, Default)]
pub struct WeaponRec {
    /// The variant's name (`ar_standard_mp`) and its `WeaponDef`'s name field
    /// (`ar_standard`).
    pub name: String,
    pub base_name: String,
    pub display_name: String,
    /// File offsets of the variant header and of the `WeaponDef` struct.
    pub header_at: usize,
    pub def_at: Option<usize>,
    /// When the `WeaponDef` is a reference to another variant's: that variant.
    pub def_shared_with: Option<String>,
    /// Every non-null pointer field (but empty strings), in stream order:
    /// (field name, value).
    /// Unnamed fields read as `def+0x...`/`variant+0x...`; arrays as
    /// `field[i]`.
    pub fields: Vec<(String, FieldValue)>,
    /// The animation table (197 slots): (slot, slot name, animation name).
    pub anims: Vec<(usize, String, String)>,
    /// bo2mp: the animation table's file offset when this variant loaded it
    /// (another variant of the gun may point at it instead of loading one).
    pub anims_at: Option<usize>,
    /// `hideTags`: script string indices (32 slots, zero ones dropped).
    pub hide_tags: Vec<u32>,
    /// bo2mp: the `WeaponDef`'s location damage multipliers (21 f32), when
    /// it loaded them: file offset.
    pub location_damage_at: Option<usize>,
}

impl WeaponRec {
    /// The first field called `name`.
    pub fn field(&self, name: &str) -> Option<&FieldValue> {
        self.fields.iter().find(|(n, _)| n == name).map(|(_, v)| v)
    }
}

/// One LOD of a model: its mesh and the materials of its surfaces.
#[derive(Clone, Debug, Default)]
pub struct LodRec {
    pub mesh: String,
    pub materials: Vec<String>,
}

/// bo2mp: an `XModel` collision surface as the zone holds it.
#[derive(Clone, Debug, Default)]
pub struct CollSurfRec {
    /// File offset of its first triangle (48 bytes each), and how many.
    pub tris_at: Option<usize>,
    pub tri_count: usize,
    pub bone: i32,
    pub contents: u32,
    pub surface_flags: u32,
}

/// An `XModel` loaded in the zone.
#[derive(Clone, Debug, Default)]
pub struct ModelRec {
    pub name: String,
    /// File offset of the model's header.
    pub header_at: usize,
    pub bones: usize,
    pub lods: Vec<LodRec>,
    /// The collision LOD mesh and the physics preset, when named.
    pub collision_mesh: Option<String>,
    pub phys_preset: Option<String>,
    /// bo2mp: the collision surfaces' table (file offset, count; 48 bytes
    /// each, `XModelCollSurf`) and the collmaps' table (file offset,
    /// count; 16 each), when the zone holds them.
    pub coll_surfs_at: Option<(usize, usize)>,
    pub collmaps_at: Option<(usize, usize)>,
    /// bo2mp: each collision surface (`XModelCollSurf`, 48: triangles
    /// pointer, count, bounds, bone, contents, surface flags) with where
    /// its triangles (`XModelCollTri`, 48: plane, s and t vectors) were read.
    pub coll_surfs: Vec<CollSurfRec>,
    /// bo2mp: the skeleton (`walk::model::xmodel`).
    pub skeleton: Skeleton,
}

/// bo2mp: a model's skeleton, as Black Ops II's `XModel` keeps it: bone
/// names (script strings), the root count, per non-root bone its parent
/// (how many bones back) and its rest pose relative to the parent
/// (quaternion xyzw, stored as halves; translation), per bone its part
/// classification and bind pose in model space (`DObjAnimMat`: unit
/// quaternion xyzw, translation, translation weight).
#[derive(Clone, Debug, Default)]
pub struct Skeleton {
    pub bone_names: Vec<u32>,
    pub num_root_bones: u8,
    pub parent_list: Vec<u8>,
    pub quats: Vec<[f32; 4]>,
    pub trans: Vec<[f32; 3]>,
    pub part_classification: Vec<u8>,
    pub base_mat: Vec<[f32; 8]>,
}

/// bo2mp: one sound alias (`SndAlias`, 0xd8): its list's name, the
/// `assetId` its sound has in the `.sabl`/`.sabs` banks, its file name,
/// its secondary alias, and the record (volumes, distances, flags).
#[derive(Clone, Debug, Default)]
pub struct SoundAliasRec {
    pub name: String,
    pub asset_id: u32,
    pub file: String,
    pub secondary: String,
    pub record: Vec<u8>,
}

/// bo2mp: one ambient room of a sound bank (the 0x184 records after the
/// ducks): `0x00 char name[64]; 0x40 u32 nameHash; 0x48 u32 radverbHash;
/// 0x4c char radverb[64..]; 0x114 f32 dry; 0x118 f32 wet; 0x11c u32 the
/// room tone's alias id; 0x120 char snapshot[64]` (read on the retail map
/// banks, `mp_metro.all`: `mp_metro_room_sml`, `global_smallroom`, 0.7,
/// 1.0, `amb_wind_interior_2d`, `default`).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SoundRoomRec {
    pub bank: String,
    pub name: String,
    pub hash: u32,
    pub radverb: String,
    pub radverb_hash: u32,
    pub dry: f32,
    pub wet: f32,
    pub tone_id: u32,
    pub snapshot: String,
    pub record: Vec<u8>,
}

/// bo2mp: a sound bank's radverb (`SndRadverb`, 0xa0): name, id and its
/// 23 floats (layout not decoded).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SoundRadverbRec {
    pub name: String,
    pub id: u32,
    pub values: Vec<f32>,
}

/// bo2mp: an `XAnimParts` loaded in the zone: its header (0xf8 bytes) and
/// where its arrays are, as (file offset, bytes).
#[derive(Clone, Debug, Default)]
pub struct AnimRec {
    pub name: String,
    pub header: Vec<u8>,
    pub numframes: u16,
    /// The animated bones' names (script strings).
    pub names: Vec<u32>,
    /// Notetracks: (kind script string: `sound`, `rumble`, `self notify`,
    /// time 0..1, the two parameters' script strings: a `sound` note's
    /// alias first).
    pub notifies: Vec<(u32, f32, u32, u32)>,
    pub data_byte: Option<(usize, usize)>,
    pub data_short: Option<(usize, usize)>,
    pub data_int: Option<(usize, usize)>,
    pub random_data_short: Option<(usize, usize)>,
    pub random_data_byte: Option<(usize, usize)>,
    pub random_data_int: Option<(usize, usize)>,
    /// Key frame indices: u8 each when `numframes < 256`, else u16.
    pub indices: Option<(usize, usize)>,
    /// The root motion's translation keys (frame, position), decoded at the walk.
    pub delta_trans: Vec<(u16, [f32; 3])>,
}

/// One surface of a mesh (`XSurface`).
#[derive(Clone, Debug, Default)]
pub struct SurfaceRec {
    pub name: String,
    pub vert_count: u16,
    pub tri_count: u16,
    /// Where its vertices and indices start in the mesh's shared data.
    pub base_vert: u32,
    pub base_index: u32,
    /// `XSurfaceFlags` (0x2: skinned, the vertices have skin weights).
    pub flags: u16,
    /// Rigid vertex lists (unskinned surfaces): (boneOffset, vertCount,
    /// triOffset, triCount), the vertices in order.
    pub rigid: Vec<[u16; 4]>,
}

/// An `XModelMesh` (one LOD's geometry) loaded in the zone.
#[derive(Clone, Debug, Default)]
pub struct MeshRec {
    pub name: String,
    pub surfaces: Vec<SurfaceRec>,
    /// `XSurfaceShared`: flags (bit 0: streamed), counts, and the byte
    /// offsets of each stream in the data.
    pub shared_flags: u32,
    pub vert_count: u32,
    pub skin_vert_count: u32,
    pub index_count: u32,
    pub data_size: u32,
    /// (positions, vertices, indices, skin weights) offsets in the data.
    pub offsets: [u32; 4],
    /// The vertex/index data in the zone (file offset, bytes), when not streamed.
    pub data: Option<(usize, usize)>,
    /// The mesh's `.xpak` entry (streamed data).
    pub xpak: Option<XPakEntry>,
}

/// A `Material` loaded in the zone.
#[derive(Clone, Debug, Default)]
pub struct MaterialRec {
    pub name: String,
    pub techset: Option<String>,
    /// The texture table: (name hash, image).
    pub textures: Vec<(u32, String)>,
    /// bo2mp: floats 40..48 of the material's first `$Globals` constant
    /// buffer of at least 192 bytes: its colour tint (40..43) and its
    /// specular colour (44..47), what a material without a colour map is
    /// drawn with (measured on core_common's gun materials: a bullet's
    /// brass, a tritium dot's red; 1, 1, 1 where a map carries the colour).
    pub colour_consts: Option<[f32; 8]>,
    /// bo2mp: the material's `$Globals` constant buffers' initial data,
    /// each distinct one once, with its slot (technique * 6 + stage): the
    /// material's own numbers (tints, gloss ranges, detail scales), laid out
    /// as its technique's shaders declare them (a map material's constant
    /// table is empty).
    pub globals: Vec<(u8, Vec<u8>)>,
    /// bo2mp: every constant buffer the material carries: (slot = technique
    /// * 6 + stage, buffer name, initial data).
    pub cbuffers: Vec<(u8, String, Vec<u8>)>,
    /// bo2mp: the texture table's raw defs (32 bytes each, in table order).
    pub texture_defs: Vec<[u8; 32]>,
    /// bo2mp: the sampler table's raw defs (8 bytes each).
    pub sampler_defs: Vec<[u8; 8]>,
    /// bo2mp: MaterialInfo +0x10, the draw surface sort key template.
    pub sort_key: u64,
    /// bo2mp: constant buffer slots that were references to buffers not
    /// read yet: (slot, VIRTUAL offset), for `Capture::settle_technique_refs`.
    pub pending_cbuffers: Vec<(u8, u64)>,
}

/// bo2mp: one `MaterialShaderArgument` (16 bytes): type, location, size,
/// buffer, then the union (two words: hashes / indices), or its literal
/// vec4 when the union points to one.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ShaderArg {
    pub ty: u16,
    pub location: u16,
    pub size: u16,
    pub buffer: u16,
    pub u: [u32; 2],
    pub literal: Option<[f32; 4]>,
}

/// bo2mp: one `MaterialDrawMethod` as the renderer binds it: its programs
/// (file offset, bytes), vertex declaration (0x78 bytes, file offset), the
/// byte at 0x4a (which of the material's constant buffers it takes:
/// `technique * 6 + this (+2 vertex, +4 pixel)`) and its arguments.
#[derive(Clone, Debug, Default)]
pub struct DrawMethodInfo {
    pub programs: [Option<(usize, usize)>; 2],
    pub decl: Option<usize>,
    pub buffer_set: u8,
    pub args: Vec<ShaderArg>,
    /// bo2mp: the VIRTUAL offsets of its vertex shader, pixel shader and
    /// vertex declaration when they were references to data not read yet
    /// (`Capture::settle_technique_refs`).
    pub pending: [Option<u64>; 3],
}

/// bo2mp: a technique as the renderer binds it: its first draw method with
/// programs (`programs`, `decl`, `args`), every draw method by index (the
/// draw picks one by its vertex kind), and the technique's state map (112
/// bytes, file offset).
#[derive(Clone, Debug, Default)]
pub struct TechniqueInfo {
    pub programs: [Option<(usize, usize)>; 2],
    pub decl: Option<usize>,
    pub args: Vec<ShaderArg>,
    pub methods: [Option<DrawMethodInfo>; 8],
    pub state_map: Option<usize>,
    /// bo2mp: the state map's VIRTUAL offset when it was a reference to
    /// data not read yet.
    pub pending_state: Option<u64>,
}

/// bo2mp: a `MaterialTechniqueSet` loaded in the zone: per technique (12),
/// its first draw method's vertex and pixel programs (file offset, bytes),
/// for `crate::dxbc::reflect`.
#[derive(Clone, Debug, Default)]
pub struct TechsetRec {
    pub name: String,
    pub programs: [Option<[Option<(usize, usize)>; 2]>; 12],
    /// bo2mp: per technique, its first draw method in full.
    pub techniques: [Option<TechniqueInfo>; 12],
    /// bo2mp: per technique, the VIRTUAL offset of a technique a reference
    /// names that was not read yet.
    pub pending: [Option<u64>; 12],
}

/// A `GfxImage` loaded in the zone.
#[derive(Clone, Debug, Default)]
pub struct ImageRec {
    pub name: String,
    pub width: u16,
    pub height: u16,
    pub depth: u16,
    pub levels: u8,
    /// `DXGI_FORMAT`.
    pub format: u32,
    /// Pixels in the zone: (file offset, bytes) when not streamed; the
    /// fallback (lowest mips) of a streamed image likewise.
    pub pixels: Option<(usize, usize)>,
    pub fallback: Option<(usize, usize)>,
    /// `totalSize`: the pixel bytes (all mips).
    pub pixels_len: usize,
    /// The streamed parts' `.xpak` entries (empty when the pixels are in the zone).
    pub xpak: Vec<XPakEntry>,
}

/// bo2mp: an `FxEffectDef` loaded in the zone (`walk::fx`): its header and
/// each element's bytes with what they point at.
#[derive(Clone, Debug, Default)]
pub struct FxRec {
    pub name: String,
    /// The 0x90-byte header.
    pub header: Vec<u8>,
    pub elems: Vec<FxElemRec>,
}

#[derive(Clone, Debug, Default)]
pub struct FxElemRec {
    /// The 0x260-byte `FxElemDef`.
    pub raw: Vec<u8>,
    /// The four sample arrays (fields 0xd0, 0xd8, 0xe0, 0xe8), each
    /// `(count byte + 1)` records; empty when shared with an earlier element.
    pub samples: [Vec<u8>; 4],
    /// The visuals' asset names (materials, models, light descriptions or
    /// effect names by element type), then the second visuals (images).
    pub visuals: Vec<String>,
    pub visuals2: Vec<String>,
    /// Effects named at 0x198, 0x1a0, 0x1a8, 0x1d0 (impact, death, emitted, attached).
    pub effects: [Option<String>; 4],
    /// A trail's def (0x30 bytes), vertices and indices.
    pub trail: Option<(Vec<u8>, Vec<u8>, Vec<u8>)>,
}

/// bo2mp: one compute shader of a `ComputeShaderSet` (`walk::compute`):
/// per variant (8) the GPU path it is for and its program (file offset,
/// bytes).
#[derive(Clone, Debug, Default)]
pub struct ComputeShaderRec {
    pub set: String,
    pub name: String,
    pub extra: Option<String>,
    pub variants: Vec<ComputeVariant>,
}

/// bo2mp: one GPU-path variant of a compute shader.
#[derive(Clone, Debug, Default)]
pub struct ComputeVariant {
    /// 2 generic D3D11, 3 NVIDIA extensions, 4 AMD extensions.
    pub path: u8,
    pub program: Option<(usize, usize)>,
    /// Thread group size (the exe divides dispatch sizes by it).
    pub groups: [u32; 3],
    pub args: Vec<ShaderArg>,
}

#[derive(Clone, Debug, Default)]
pub struct Capture {
    /// bo2mp: the material constant buffers read so far by their record's
    /// file offset (name, contents), for the materials that reference them.
    pub cbuffer_at: std::collections::HashMap<usize, (String, Vec<u8>)>,
    /// bo2mp: compute shaders (`walk::compute`).
    pub compute_shaders: Vec<ComputeShaderRec>,
    /// bo2mp: effects.
    pub fx: Vec<FxRec>,
    pub weapons: Vec<WeaponRec>,
    pub models: Vec<ModelRec>,
    pub materials: Vec<MaterialRec>,
    pub images: Vec<ImageRec>,
    /// bo2mp: technique sets.
    pub techsets: Vec<TechsetRec>,
    /// bo2mp: where each shader struct and technique struct loaded (file
    /// offset) -> its program(s), for later references to it.
    pub shader_programs: HashMap<usize, (usize, usize)>,
    /// bo2mp: technique references to data read later, looked up after the
    /// walk (`settle_technique_refs`).
    pub technique_refs_settled: usize,
    pub technique_programs: HashMap<usize, [Option<(usize, usize)>; 2]>,
    /// bo2mp: the technique read at each struct position, in full.
    pub technique_infos: HashMap<usize, TechniqueInfo>,
    /// bo2mp: the vertex declaration read at each struct position.
    pub decls: HashMap<usize, usize>,
    pub meshes: Vec<MeshRec>,
    /// bo2mp: animations (`walk::anim`), kept only when `keep_anims` is on.
    pub anims: Vec<AnimRec>,
    /// Also the sound aliases (`walk::sound`): kept with the animations.
    pub keep_anims: bool,
    pub sound_aliases: Vec<SoundAliasRec>,
    /// bo2mp: the sound banks' ambient rooms and radverbs (always kept:
    /// small).
    pub sound_rooms: Vec<SoundRoomRec>,
    pub sound_radverbs: Vec<SoundRadverbRec>,
    /// Also the localized strings: (key, text).
    pub localize: Vec<(String, String)>,
    /// And the string tables: (name, columns, cells row by row).
    pub tables: Vec<(String, usize, Vec<String>)>,
    /// The map assets (`walk::world`).
    pub world: crate::walk::world::WorldCapture,
    /// What the fields of the assets whose walkers keep them name
    /// (attachment uniques, cosmetic variants, weapon camos): (type, name)
    /// -> (field, value), in stream order.
    pub links: HashMap<(AssetType, String), Vec<(String, FieldValue)>>,
    /// bo2mp: each attachment unique's header (file offset): its models'
    /// placements are numbers there.
    pub unique_headers: HashMap<String, usize>,
    /// bo2mp: the impact tables (`ImpactFx`, `SurfaceFxTable`,
    /// `EntityFxImpacts`) by (type, name): each pointer field's offset and
    /// the asset it names.
    pub impact_fields: HashMap<(AssetType, String), Vec<(usize, String)>>,
    /// bo2mp: light descriptions by name, each its 0x240 bytes.
    pub light_descriptions: HashMap<String, Vec<u8>>,
    by_name: HashMap<(AssetType, String), usize>,
}

impl Capture {
    pub(crate) fn add_model(&mut self, r: ModelRec) {
        self.by_name
            .insert((AssetType::XModel, r.name.clone()), self.models.len());
        self.models.push(r);
    }
    pub(crate) fn add_mesh(&mut self, r: MeshRec) {
        self.by_name
            .insert((AssetType::XModelMesh, r.name.clone()), self.meshes.len());
        self.meshes.push(r);
    }
    pub(crate) fn add_anim(&mut self, r: AnimRec) {
        self.by_name
            .insert((AssetType::XAnimParts, r.name.clone()), self.anims.len());
        self.anims.push(r);
    }
    /// bo2mp: the animation called `name` (when the walk kept animations).
    pub fn anim(&self, name: &str) -> Option<&AnimRec> {
        self.by_name
            .get(&(AssetType::XAnimParts, name.to_string()))
            .map(|&i| &self.anims[i])
    }
    pub fn mesh(&self, name: &str) -> Option<&MeshRec> {
        self.by_name
            .get(&(AssetType::XModelMesh, name.to_string()))
            .map(|&i| &self.meshes[i])
    }
    pub(crate) fn add_material(&mut self, r: MaterialRec) {
        self.by_name
            .insert((AssetType::Material, r.name.clone()), self.materials.len());
        self.materials.push(r);
    }
    pub(crate) fn add_fx(&mut self, r: FxRec) {
        self.by_name.insert((AssetType::Fx, r.name.clone()), self.fx.len());
        self.fx.push(r);
    }
    /// bo2mp: the effect called `name` (with or without its `,`).
    pub fn effect(&self, name: &str) -> Option<&FxRec> {
        let n = name.trim_start_matches(',');
        self.by_name
            .get(&(AssetType::Fx, n.to_owned()))
            .or_else(|| self.by_name.get(&(AssetType::Fx, format!(",{n}"))))
            .map(|&i| &self.fx[i])
    }
    /// bo2mp: the references in techniques to shaders, vertex
    /// declarations, state maps and techniques that were not read when the
    /// walk met them, looked up once the walk is done. The BO3 mod tools'
    /// Workshop zones name data a later asset loads (Summit: 1835 shaders,
    /// 1065 declarations, 194 state maps; its decals and walls were left
    /// out for want of them); retail zones do not.
    pub fn settle_technique_refs(&mut self, vmap: &crate::stream::VirtualMap) -> usize {
        let mut settled = 0usize;
        let (programs, decls) = (&self.shader_programs, &self.decls);
        let fix = |ti: &mut TechniqueInfo| {
            let mut settled = 0usize;
            for m in ti.methods.iter_mut().flatten() {
                for k in 0..2 {
                    if let Some(v) = m.pending[k]
                        && let Some(p) = vmap.file_pos(v).and_then(|a| programs.get(&a))
                    {
                        m.programs[k] = Some(*p);
                        m.pending[k] = None;
                        settled += 1;
                    }
                }
                if let Some(v) = m.pending[2]
                    && let Some(a) = vmap.file_pos(v).filter(|a| decls.contains_key(a))
                {
                    m.decl = Some(a);
                    m.pending[2] = None;
                    settled += 1;
                }
            }
            if ti.state_map.is_none()
                && let Some(a) = ti.pending_state.and_then(|v| vmap.file_pos(v))
            {
                ti.state_map = Some(a);
                ti.pending_state = None;
                settled += 1;
            }
            // (A method whose shaders were never read: none, as before.)
            for slot in &mut ti.methods {
                if slot.as_ref().is_some_and(|m| m.programs.iter().all(Option::is_none)) {
                    *slot = None;
                }
            }
            // The first draw method with programs, as `technique` takes it.
            if let Some(m) = ti.methods.iter().flatten().find(|m| m.programs.iter().any(Option::is_some)) {
                ti.programs = m.programs;
                ti.decl = m.decl;
                ti.args = m.args.clone();
            }
            settled
        };
        // Materials' constant buffers named by references to buffers read
        // later.
        for m in &mut self.materials {
            for (slot, v) in std::mem::take(&mut m.pending_cbuffers) {
                match vmap.file_pos(v).and_then(|a| self.cbuffer_at.get(&a)) {
                    Some((n, d)) => {
                        m.cbuffers.push((slot, n.clone(), d.clone()));
                        settled += 1;
                    }
                    None => m.pending_cbuffers.push((slot, v)),
                }
            }
        }
        let waits = |ti: &TechniqueInfo| ti.pending_state.is_some() || ti.methods.iter().flatten().any(|m| m.pending.iter().any(Option::is_some));
        for ti in self.technique_infos.values_mut().filter(|t| waits(t)) {
            settled += fix(ti);
        }
        for ts in &mut self.techsets {
            for i in 0..ts.techniques.len() {
                if let Some(v) = ts.pending[i]
                    && let Some(t) = vmap.file_pos(v).and_then(|a| self.technique_infos.get(&a))
                {
                    ts.techniques[i] = Some(t.clone());
                    ts.pending[i] = None;
                    settled += 1;
                }
                if let Some(ti) = ts.techniques[i].as_mut() {
                    if waits(ti) {
                        settled += fix(ti);
                    }
                    if ti.programs.iter().any(Option::is_some) {
                        ts.programs[i] = Some(ti.programs);
                    } else if ti.methods.iter().flatten().all(|m| m.programs.iter().all(Option::is_none)) {
                        // (Its shaders never read: as before, no technique.)
                        ts.techniques[i] = None;
                    }
                }
            }
        }
        settled
    }

    pub(crate) fn add_techset(&mut self, r: TechsetRec) {
        self.by_name
            .insert((AssetType::TechniqueSet, r.name.clone()), self.techsets.len());
        self.techsets.push(r);
    }
    /// bo2mp: the technique set called `name` (with or without its `,`).
    pub fn techset(&self, name: &str) -> Option<&TechsetRec> {
        let bare = name.trim_start_matches(',');
        [bare.to_string(), format!(",{bare}")]
            .iter()
            .find_map(|n| self.by_name.get(&(AssetType::TechniqueSet, n.clone())))
            .map(|&i| &self.techsets[i])
    }
    pub(crate) fn add_image(&mut self, r: ImageRec) {
        self.by_name
            .insert((AssetType::Image, r.name.clone()), self.images.len());
        self.images.push(r);
    }
    /// The model called `name` (the last one loaded under that name).
    pub fn model(&self, name: &str) -> Option<&ModelRec> {
        self.by_name
            .get(&(AssetType::XModel, name.to_string()))
            .map(|&i| &self.models[i])
    }
    pub fn material(&self, name: &str) -> Option<&MaterialRec> {
        self.by_name
            .get(&(AssetType::Material, name.to_string()))
            .map(|&i| &self.materials[i])
    }
    pub fn image(&self, name: &str) -> Option<&ImageRec> {
        self.by_name
            .get(&(AssetType::Image, name.to_string()))
            .map(|&i| &self.images[i])
    }
    pub(crate) fn add_links(&mut self, ty: AssetType, name: &str, links: Vec<(String, FieldValue)>) {
        self.links.insert((ty, name.to_string()), links);
    }
    /// What the fields of the asset `ty` `name` name (`links`); empty when
    /// the walk did not keep them.
    pub fn links(&self, ty: AssetType, name: &str) -> &[(String, FieldValue)] {
        self.links.get(&(ty, name.to_string())).map_or(&[], Vec::as_slice)
    }
    /// The weapon whose variant or `WeaponDef` is called `name`.
    pub fn weapon(&self, name: &str) -> Option<&WeaponRec> {
        self.weapons
            .iter()
            .find(|r| r.name == name || r.base_name == name)
    }
}
