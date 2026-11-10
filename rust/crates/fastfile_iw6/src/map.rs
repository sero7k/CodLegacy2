//! bo2mp: a Ghosts multiplayer map's world, collision and entities as plain
//! data, from what the exact walk kept (`walk::GfxWorldRec`, `ClipMapRec`,
//! `MapEntsRec`, `ComWorldRec`).
//!
//! Ghosts' world vertex is CoD4's layout (position, binormal sign, colour,
//! texture and lightmap coordinates, normal, tangent) with its unit vectors
//! packed unsigned 10:10:10 over 1023 (as its models'); surfaces index the
//! vertex table from `firstVertex` and the index table from `baseIndex`.
//! Collision is IW5's shape: brushes (bounds plus their non-axial sides),
//! terrain triangles under an AABB forest, brush models (`cmodel_t`) whose
//! brush trees are the leaf brush nodes. Spawn points are not in the entity
//! string but in `MapEnts::spawnList`; [`IwMap::entity_string`] writes them
//! back as entities so every consumer of IW entity text finds them.

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
    pub first_vertex: u32,
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
    pub flags: u16,
    /// Linear light where the model stands (`groundLighting`); all zero
    /// for a model lit per vertex.
    pub ground_lighting: [f32; 4],
    /// The reflection probe the zone gives the model
    /// (`reflectionProbeIndex`; `probe` module).
    pub reflection_probe_index: u8,
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
    pub indices: Vec<u16>,
    pub surfaces: Vec<Surface>,
    /// Surface ranges [begin, end): lit opaque, lit decal, lit transparent,
    /// emissive.
    pub ranges: [(u32, u32); 4],
    pub static_models: Vec<StaticModel>,
    pub brush_models: Vec<BrushModel>,
    pub mins: [f32; 3],
    pub maxs: [f32; 3],
    /// The sky surfaces (every sky's `skyStartSurfs`).
    pub sky_surfaces: Vec<u32>,
    pub sky_images: Vec<Option<AssetId>>,
    pub lightmaps: Vec<(Option<AssetId>, Option<AssetId>)>,
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

/// A `ScriptableInstance`: where it stands and its def's base model.
#[derive(Clone, Debug, Default)]
pub struct PlacedScriptable {
    pub name: String,
    pub model: Option<AssetId>,
    pub origin: [f32; 3],
    /// Pitch, yaw, roll in degrees.
    pub angles: [f32; 3],
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
    /// The scriptable objects the map places (destructible cars, fans,
    /// lights, ...), each at its def's base model.
    pub scriptables: Vec<PlacedScriptable>,
    /// Sides whose plane was not found (should be 0).
    pub missing_planes: usize,
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

/// One hull of a trigger model: its box and the slabs that cut it.
#[derive(Clone, Debug, Default)]
pub struct TriggerHull {
    pub mid: [f32; 3],
    pub half: [f32; 3],
    /// (direction, mid point, half size).
    pub slabs: Vec<([f32; 3], f32, f32)>,
}

/// The entity keys Ghosts stores as numbers (its canonical script strings),
/// by name. Read off the retail maps' entities by their values (a key whose
/// values are spawn classes is `classname`, ...); keys not here are written
/// as `iw6_<number>`.
pub const IW6_KEYS: &[(u32, &str)] = &[
    (47, "angles"),
    (52, "animscript"),
    (97, "classname"),
    (272, "height"),
    (392, "model"),
    (430, "origin"),
    (476, "radius"),
    (504, "script_linkname"),
    (506, "script_noteworthy"),
    (542, "spawnflags"),
    (640, "target"),
    (642, "targetname"),
    (759, "sunlight"),
    (760, "suncolor"),
    (761, "sundirection"),
    (1619, "_color"),
    (27614, "script_gameobjectname"),
    (27658, "script_label"),
    (27665, "script_linkto"),
];

/// The name of a numbered entity key.
pub fn key_name(id: u32) -> Option<&'static str> {
    IW6_KEYS.iter().find(|(k, _)| *k == id).map(|(_, n)| *n)
}

/// Ghosts' entity text with its keys named (`97 "worldspawn"` ->
/// `"classname" "worldspawn"`) and its trigger models (`"model" "?N"`)
/// kept as they are (`?N` is the N-th of `IwMap::trigger_models`, as IW4's
/// and IW5's; `*N` the N-th brush model).
pub fn named_entity_text(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + text.len() / 3);
    for line in text.lines() {
        let t = line.trim();
        match t.split_once(' ') {
            Some((key, value)) if !key.is_empty() && key.bytes().all(|b| b.is_ascii_digit()) => {
                let id: u32 = key.parse().unwrap_or(u32::MAX);
                match key_name(id) {
                    Some(name) => out.push_str(&format!("\"{name}\" {}", value.trim())),
                    None => out.push_str(&format!("\"iw6_{id}\" {}", value.trim())),
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
    /// The entity string as stored (numbered keys).
    pub entities: String,
    pub spawns: Vec<SpawnRecord>,
    pub sun: Option<Sun>,
    pub notes: Vec<String>,
}

impl IwMap {
    /// The entity string, keys named, with the spawn records written in as
    /// entities (`classname` the record's name: `mp_tdm_spawn`, ...).
    pub fn entity_string(&self) -> String {
        let mut out = named_entity_text(self.entities.trim_end_matches('\0'));
        if !out.is_empty() && !out.ends_with('\n') {
            out.push('\n');
        }
        for s in &self.spawns {
            if s.classname.is_empty() {
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

/// Ghosts' packed unit vector (unsigned 10:10:10 over 1023).
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
            Err(e) => out.notes.push(format!("iw6 world: {e}")),
        }
    } else {
        out.notes.push("iw6 world: the zone has no GfxWorld".into());
    }
    if let Some(c) = cap.clip_maps.first() {
        let mut clip = read_clip(&r, c);
        clip.scriptables = c
            .scriptables
            .iter()
            .map(|i| PlacedScriptable {
                name: cap
                    .others
                    .get(i.def.index as usize)
                    .map(|o| o.name.clone())
                    .unwrap_or_default(),
                model: cap.scriptable_models.get(&i.def.index).copied().flatten(),
                origin: i.origin,
                angles: i.angles,
            })
            .collect();
        if clip.missing_planes > 0 {
            out.notes.push(format!(
                "iw6 clip: {} brush sides without their plane",
                clip.missing_planes
            ));
        }
        out.clip = Some(clip);
    } else {
        out.notes.push("iw6 clip: the zone has no clipMap".into());
    }
    if let Some(m) = cap.map_ents.first() {
        out.entities = m.entity_string.clone();
        out.trigger_models = read_triggers(&r, m.triggers);
        let sz = L::SpawnPointEntityRecord::SIZE;
        if let Some(span) = m.spawns {
            let string = |at: usize| -> String {
                let i = r.u32(at) as usize;
                script_strings.get(i).cloned().flatten().unwrap_or_default()
            };
            for i in 0..span.len / sz {
                let s = span.pos + sz * i;
                use L::SpawnPointEntityRecord as P;
                out.spawns.push(SpawnRecord {
                    classname: string(s + P::name),
                    targetname: String::new(),
                    target: string(s + P::target),
                    script_noteworthy: string(s + P::script_noteworthy),
                    origin: r.vec3(s + P::origin),
                    angles: r.vec3(s + P::angles),
                });
            }
        }
    } else {
        out.notes
            .push("iw6 entities: the zone has no MapEnts".into());
    }
    if let Some(cw) = cap.com_worlds.first()
        && let Some(lights) = cw.primary_lights
        && cw.primary_light_count > 0
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
    if let Some(at) = g.smodels {
        let sd = L::GfxStaticModelDrawInst::SIZE;
        for i in 0..g.smodel_count as usize {
            let d = at + sd * i;
            use L::GfxStaticModelDrawInst as D;
            let a = d + D::placement__axis;
            w.static_models.push(StaticModel {
                model: g.smodel_models.get(i).copied().flatten(),
                origin: r.vec3(d + D::placement__origin),
                axis: [r.vec3(a), r.vec3(a + 12), r.vec3(a + 24)],
                scale: r.f32(d + D::placement__scale),
                cull_dist: f32::from(r.u16(d + D::cullDist)),
                flags: r.u16(d + D::flags),
                ground_lighting: std::array::from_fn(|k| r.f32(d + D::groundLighting + 4 * k)),
                reflection_probe_index: r.u8(d + D::reflectionProbeIndex),
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
    // `skyStartSurfs` holds positions in the dpvs sorted surface index (mp_frag:
    // position 2647 -> surface 15498, `wc/sky_mp_frag`; read as surface numbers
    // it named a cargo container door).
    for s in g.sky_surfaces.iter().flatten() {
        for k in 0..s.len / 4 {
            let pos = r.u32(s.pos + 4 * k);
            w.sky_surfaces.extend(sky_surface_of(pos, g.sorted_surfs, g.static_surface_count, |at| r.u32(at)));
        }
    }
    w.sky_images = g.sky_images.clone();
    w.lightmaps = g.lightmaps.clone();
    Ok(w)
}

/// The surface a sky's `skyStartSurfs` entry names: a position in the sorted
/// surface index (`sorted_at`: its offset in the image, `count` entries).
pub fn sky_surface_of(pos: u32, sorted_at: Option<usize>, count: u32, read: impl Fn(usize) -> u32) -> Option<u32> {
    let at = sorted_at?;
    (pos < count).then(|| read(at + 4 * pos as usize))
}

fn read_clip(r: &R, c: &ClipMapRec) -> Clip {
    let info = &c.info;
    let mut clip = Clip {
        name: c.name.clone(),
        materials: info.materials.clone(),
        ..Default::default()
    };
    // Planes: the info's array, then any a side names outside it.
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
    let mut extra: std::collections::HashMap<usize, u32> = std::collections::HashMap::new();
    let mut plane_index = |clip: &mut Clip, at: usize| -> u32 {
        if let Some(base) = info.planes {
            let end = base + ps * info.plane_count as usize;
            if (base..end).contains(&at) && (at - base).is_multiple_of(ps) {
                return ((at - base) / ps) as u32;
            }
        }
        *extra.entry(at).or_insert_with(|| {
            clip.planes.push(read_plane(at));
            (clip.planes.len() - 1) as u32
        })
    };
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
            if let Some(first) = info.brush_sides.get(i).copied().flatten() {
                for k in 0..numsides {
                    let side = first + bs * k;
                    let material = r.u16(side + L::cbrushside_t::materialNum);
                    match info.side_planes.get(&side) {
                        Some(&p) => {
                            let idx = plane_index(&mut clip, p);
                            sides.push((idx, material));
                        }
                        None => clip.missing_planes += 1,
                    }
                }
            } else if numsides > 0 {
                clip.missing_planes += numsides;
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
                first_coll_aabb: r.u16(m + M::leaf__firstCollAabbIndex),
                coll_aabb_count: r.u16(m + M::leaf__collAabbCount),
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
        for i in 0..c.static_model_models.len() {
            let s = at + sm * i;
            let a = s + S::invScaledAxis;
            let (mins, maxs) = r.bounds(s + S::absBounds);
            clip.static_models.push(ClipStaticModel {
                model: c.static_model_models[i],
                origin: r.vec3(s + S::origin),
                inv_scaled_axis: [r.vec3(a), r.vec3(a + 12), r.vec3(a + 24)],
                mins,
                maxs,
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
                quat: [
                    r.f32(d + D::pose__quat),
                    r.f32(d + D::pose__quat + 4),
                    r.f32(d + D::pose__quat + 8),
                    r.f32(d + D::pose__quat + 12),
                ],
                origin: r.vec3(d + D::pose__origin),
                model: c.dyn_ent_models[k].get(i).copied().flatten(),
                brush_model: r.u16(d + D::brushModel),
                health: i32::from(r.i16(d + D::health)),
                contents: r.i32(d + D::contents),
            });
        }
    }
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

#[cfg(test)]
mod tests {
    #[test]
    fn a_sky_start_is_a_position_in_the_sorted_surface_index() {
        // Offset 100 holds the sorted index [6327, 15498, 16035] (mp_frag's 2646..2649).
        let read = |at: usize| [6327u32, 15498, 16035][(at - 100) / 4];
        assert_eq!(super::sky_surface_of(1, Some(100), 3, read), Some(15498));
        assert_eq!(super::sky_surface_of(3, Some(100), 3, read), None);
        assert_eq!(super::sky_surface_of(0, None, 3, read), None);
    }

    #[test]
    fn iw3_flat_weight_reads_the_packed_codes() {
        let code = |v: f32| (((v + 2.08) / 4.08) * 255.0).round() as u8;
        assert!((super::iw3_flat_weight(code(0.0), code(0.0)) - 1.0).abs() < 0.02);
        let w = super::iw3_flat_weight(code(0.6), code(0.0));
        assert!((w - 1.0 / 1.36f32.sqrt()).abs() < 0.02, "{w}");
    }

    #[test]
    fn a_quaternion_turns_the_axes() {
        let h = std::f32::consts::FRAC_1_SQRT_2;
        // 90 degrees about up: forward becomes +y, left becomes -x.
        let a = super::axis_of_quat([0.0, 0.0, h, h]);
        let close = |v: [f32; 3], w: [f32; 3]| (0..3).all(|k| (v[k] - w[k]).abs() < 1e-5);
        assert!(close(a[0], [0.0, 1.0, 0.0]));
        assert!(close(a[1], [-1.0, 0.0, 0.0]));
        assert!(close(a[2], [0.0, 0.0, 1.0]));
        // The same turn as a yaw of 90 degrees.
        let b = super::axis_of([0.0, 90.0, 0.0]);
        assert!((0..3).all(|i| close(a[i], b[i])));
    }

    #[test]
    fn a_second_level_is_cut_off() {
        let at = |z: f32| super::SpawnRecord {
            origin: [0.0, 0.0, z],
            ..Default::default()
        };
        let mut spawns: Vec<super::SpawnRecord> = (0..20).map(|i| at(i as f32 * 10.0)).collect();
        assert_eq!(super::alternate_level_cut(&spawns), None);
        spawns.extend((0..12).map(|i| at(35000.0 + i as f32)));
        assert_eq!(
            super::alternate_level_cut(&spawns),
            Some((190.0 + 35000.0) / 2.0)
        );
    }

    #[test]
    fn brush_models_are_placed_by_their_entities() {
        let mut w = super::World::default();
        w.brush_models = vec![
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
        ];
        let map = super::IwMap {
            world: Some(w),
            entities: "{\n97 \"script_brushmodel\"\n430 \"10 20 30\"\n392 \"*1\"\n}\n{\n97 \"script_brushmodel\"\n642 \"exploder\"\n392 \"*2\"\n}\n".to_owned(),
            ..Default::default()
        };
        let placed = super::placed_brush_models(&map, None);
        assert_eq!(placed.len(), 1);
        assert_eq!(
            (placed[0].surfaces.clone(), placed[0].origin),
            (5..7, [10.0, 20.0, 30.0])
        );
        let a = super::axis_of([0.0, 90.0, 0.0]);
        assert!(
            (a[0][1] - 1.0).abs() < 1e-6
                && (a[1][0] + 1.0).abs() < 1e-6
                && (a[2][2] - 1.0).abs() < 1e-6
        );
    }

    #[test]
    fn keys_are_named() {
        let t = "{\n97 \"worldspawn\"\n430 \"0 0 0\"\n99999 \"x\"\n}";
        let n = super::named_entity_text(t);
        assert!(n.contains("\"classname\" \"worldspawn\""), "{n}");
        assert!(n.contains("\"origin\" \"0 0 0\""), "{n}");
        assert!(n.contains("\"iw6_99999\" \"x\""), "{n}");
    }
}

/// A raw file of the zone by name (case-insensitive), inflated when stored
/// compressed.
pub fn raw_file(cap: &Capture, image: &[u8], name: &str) -> Option<Vec<u8>> {
    let r = cap
        .raw_files
        .iter()
        .find(|r| r.name.eq_ignore_ascii_case(name))?;
    let data = r.data?.get(image);
    if r.compressed_len == 0 {
        return Some(data.to_vec());
    }
    use std::io::Read;
    let mut out = Vec::with_capacity(r.len as usize);
    flate2::read::ZlibDecoder::new(data)
        .read_to_end(&mut out)
        .ok()?;
    Some(out)
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
/// to cut at, keeping what lies below.
///
/// Strikezone builds its after-the-strike map into the same world 35,000
/// units up (half its world triangles, 5,000 placed models, its own spawn
/// points and sky brush `after_sky`); the game moves everyone there when
/// the strike lands. Drawn together, the destroyed copy hung in the sky,
/// and its spawn points put players in it. Found from the spawn points:
/// two groups at least 8,000 units apart in height, each of at least eight;
/// the cut is half way between them.
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
    /// The brush model's number (`*N`) when an entity places it (a script
    /// may move it); `None` for a dynamic entity's brush.
    pub brush: Option<u32>,
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

/// A prop the map shows that is not one of the world's static models.
#[derive(Clone, Debug, PartialEq)]
pub struct ExtraProp {
    /// `"dynent"`, `"scriptable"` or `"script_model"`.
    pub source: &'static str,
    /// Index into `Capture::xmodels`.
    pub model: u32,
    pub origin: [f32; 3],
    /// Forward, left, up in the world.
    pub axis: [[f32; 3]; 3],
}

/// Axes (forward, left, up) of a unit quaternion `[x, y, z, w]`.
pub fn axis_of_quat(q: [f32; 4]) -> [[f32; 3]; 3] {
    let n = q.iter().map(|v| v * v).sum::<f32>().sqrt();
    if n < 1e-6 {
        return [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
    }
    let [x, y, z, w] = q.map(|v| v / n);
    [
        [
            1.0 - 2.0 * (y * y + z * z),
            2.0 * (x * y + z * w),
            2.0 * (x * z - y * w),
        ],
        [
            2.0 * (x * y - z * w),
            1.0 - 2.0 * (x * x + z * z),
            2.0 * (y * z + x * w),
        ],
        [
            2.0 * (x * z + y * w),
            2.0 * (y * z - x * w),
            1.0 - 2.0 * (x * x + y * y),
        ],
    ]
}

/// Whether a model has a first LOD with vertices (a model another zone
/// holds is an empty record).
pub fn model_has_geometry(cap: &Capture, model: u32) -> bool {
    cap.xmodels
        .get(model as usize)
        .and_then(|m| m.lods.first())
        .and_then(|l| l.surfs)
        .and_then(|s| cap.model_surfs.get(s.index as usize))
        .is_some_and(|ss| ss.surfs.iter().any(|s| s.vert_count > 0))
}

/// Whether a prop may use the model: it has geometry, or it is an empty
/// record of a model another zone holds (the shared zones'
/// `weapon_uav_control_unit`; the lane looks it up by name). Joints
/// (`tag_origin`) and flags (`prop_flag_*`: the objective's own) are not
/// props.
pub fn model_is_placeable(cap: &Capture, model: u32) -> bool {
    model_has_geometry(cap, model)
        || cap.xmodels.get(model as usize).is_some_and(|m| {
            let n = m.name.trim_start_matches(',');
            !n.is_empty() && !n.starts_with("tag_") && !n.starts_with("prop_flag")
        })
}

/// A prop `extra_props` left out, and why.
#[derive(Clone, Debug, PartialEq)]
pub struct SkippedProp {
    /// `"dynent"`, `"scriptable"` or `"entity"`.
    pub source: &'static str,
    pub reason: &'static str,
    /// The model's name (an entity's `model` key), or the scriptable's name.
    pub model: String,
    /// Where an entity that names a model no loaded record of the map zone holds stands (the
    /// renderer looks the name up in the other zones): origin and forward, left, up.
    pub place: Option<([f32; 3], [[f32; 3]; 3])>,
}

/// Entity classes that place a model by name as the map starts
/// (`script_model`, a mover waiting for its trigger, prefab and map-maker
/// models).
pub fn is_model_class(class: &str) -> bool {
    // Any entity that names a model by its `model` key shows it as the map
    // starts, except the classes that only use the key as data (triggers
    // name a flag model, structs a prop for a script, nodes, lights and
    // probes have none) and the turrets (their model is the gun, spawned
    // with the turret's own def). The old list (`script_model`,
    // `script_model_mover`, `misc_prefabl`, `midc_model`, `misc_model`)
    // left out every other class a Ghosts map places props with (the
    // `misc_*`, `dyn_*`, `prop_*` and mode-specific classes of the maps
    // whose zones are not staged). `BO2MP_IW6_MODEL_CLASSES=0` returns to it.
    if std::env::var("BO2MP_IW6_MODEL_CLASSES").is_ok_and(|v| v.trim() == "0") {
        return matches!(
            class,
            "script_model" | "script_model_mover" | "misc_prefabl" | "midc_model" | "misc_model"
        );
    }
    const DENY: &[&str] = &[
        "trigger", "script_struct", "script_origin", "script_brushmodel", "scriptable", "node_",
        "info_", "mp_", "light", "reflection", "volume", "misc_turret", "script_vehicle",
        "worldspawn", "sound", "ambient", "fx", "exploder", "func_", "path", "target_",
    ];
    !class.is_empty() && !DENY.iter().any(|d| class.starts_with(d))
}

/// The props the map shows as it starts that are not static models: the
/// clip map's dynamic entities (clutter and destructibles: bottles, hard
/// hats, monitors), its scriptable objects (vending machines, water
/// coolers, panels, destructible screens, cars) at their base models (the
/// intact state: the states' models are the damaged ones), and the
/// entities that place a model by name (`is_model_class`: trees, TVs, fans,
/// the wrecks a mover waits with). Not above `cut`
/// (`alternate_level_cut`); not a dynamic entity standing where a static
/// model of the same model already stands; not an entity that is a game
/// object or a later state (bomb site missiles, care packages,
/// `exploder`s, a `*destroyed*` model standing where another model
/// stands, the lights' `light_off` twins, dogs, flags). Models the zone
/// does not hold are left out.
pub fn extra_props(map: &IwMap, cap: &Capture, cut: Option<f32>) -> Vec<ExtraProp> {
    extra_props_report(map, cap, cut).0
}

/// `extra_props` and what it left out.
pub fn extra_props_report(
    map: &IwMap,
    cap: &Capture,
    cut: Option<f32>,
) -> (Vec<ExtraProp>, Vec<SkippedProp>) {
    let mut out = Vec::new();
    let mut skipped = Vec::new();
    let model_name = |m: u32| {
        cap.xmodels
            .get(m as usize)
            .map_or_else(String::new, |x| x.name.trim_start_matches(',').to_owned())
    };
    let above = |o: [f32; 3]| cut.is_some_and(|c| o[2] > c);
    let statics: Vec<(u32, [f32; 3])> = map
        .world
        .iter()
        .flat_map(|w| w.static_models.iter())
        .filter_map(|s| Some((s.model?.index, s.origin)))
        .collect();
    if let Some(clip) = &map.clip {
        for d in &clip.dyn_ents {
            let m = d.model.map(|m| m.index);
            let mut skip = |reason: &'static str| {
                skipped.push(SkippedProp {
                    source: "dynent",
                    reason,
                    model: m.map_or_else(|| format!("(brush {})", d.brush_model), model_name),
                    place: None,
                });
            };
            let Some(m) = m else {
                skip("brush model");
                continue;
            };
            if !model_is_placeable(cap, m) {
                skip("model has no geometry");
                continue;
            }
            if above(d.origin) {
                skip("above the alternate level cut");
                continue;
            }
            let near = |a: [f32; 3], b: [f32; 3]| (0..3).all(|k| (a[k] - b[k]).abs() < 1.0);
            if statics
                .iter()
                .any(|&(sm, so)| sm == m && near(so, d.origin))
            {
                skip("a static model stands there");
                continue;
            }
            out.push(ExtraProp {
                source: "dynent",
                model: m,
                origin: d.origin,
                axis: axis_of_quat(d.quat),
            });
        }
        for s in &clip.scriptables {
            let mut skip = |reason: &'static str| {
                skipped.push(SkippedProp {
                    source: "scriptable",
                    reason,
                    model: s.name.clone(),
                    place: None,
                });
            };
            let Some(m) = s.model.map(|m| m.index) else {
                skip("no base model");
                continue;
            };
            if !model_is_placeable(cap, m) {
                skip("model has no geometry");
                continue;
            }
            if above(s.origin) {
                skip("above the alternate level cut");
                continue;
            }
            // A scriptable whose body is one of the clip map's dynamic
            // entities (the same model where it stands: the fluorescent
            // lights and fire barrels) is drawn as that entity.
            if clip.dyn_ents.iter().any(|d| {
                d.model.map(|x| x.index) == Some(m) && (0..3).all(|k| (d.origin[k] - s.origin[k]).abs() < 1.0)
            }) {
                skip("drawn as its dynamic entity");
                continue;
            }
            out.push(ExtraProp {
                source: "scriptable",
                model: m,
                origin: s.origin,
                axis: axis_of(s.angles),
            });
        }
    }
    let by_name: HashMap<String, u32> = cap
        .xmodels
        .iter()
        .enumerate()
        .filter(|(i, _)| model_is_placeable(cap, *i as u32))
        .map(|(i, m)| {
            (
                m.name.trim_start_matches(',').to_ascii_lowercase(),
                i as u32,
            )
        })
        .collect();
    let text = named_entity_text(map.entities.trim_end_matches('\0'));
    let vec3 = |kv: &HashMap<&str, &str>, k: &str| -> Option<[f32; 3]> {
        let mut it = kv.get(k)?.split_whitespace().map(|x| x.parse::<f32>().ok());
        Some([it.next()??, it.next()??, it.next()??])
    };
    let mut entities: Vec<HashMap<&str, &str>> = Vec::new();
    for block in text.split('{').skip(1) {
        let mut kv: HashMap<&str, &str> = HashMap::new();
        for line in block.lines() {
            let mut parts = line.split('"').skip(1).step_by(2);
            if let (Some(k), Some(v)) = (parts.next(), parts.next()) {
                kv.insert(k, v);
            }
        }
        if kv.get("classname").is_some_and(|c| is_model_class(c)) && kv.get("model").is_some_and(|m| !m.starts_with('*')) {
            entities.push(kv);
        }
    }
    for kv in &entities {
        let name = kv["model"].to_ascii_lowercase();
        let origin = vec3(kv, "origin").unwrap_or_default();
        let mut skip = |reason: &'static str| {
            skipped.push(SkippedProp {
                source: "entity",
                reason,
                model: name.clone(),
                place: None,
            });
        };
        // A model that is the later state of another standing in the same
        // place (the bomb site's missile and its wreck); a wreck on its own
        // is scenery (the burnt-out cars).
        let twin = name.contains("destroyed")
            && entities.iter().any(|o| {
                o["model"].to_ascii_lowercase() != name
                    && vec3(o, "origin")
                        .is_some_and(|p| (0..3).all(|k| (p[k] - origin[k]).abs() < 1.0))
            });
        if twin {
            skip("later state of the model beside it");
            continue;
        }
        if name.starts_with("fullbody_")
            || name.starts_with("prop_flag")
            || name.starts_with("viewmodel")
        {
            skip("dog, flag or view model");
            continue;
        }
        if kv.contains_key("script_gameobjectname")
            || matches!(
                kv.get("targetname").copied(),
                Some("exploder" | "airdrop_crate" | "care_package" | "sd_bomb")
            )
            || kv
                .get("script_noteworthy")
                .is_some_and(|n| n.ends_with("light_off"))
            // (Pirate's `destruction_tower_after`: shown once it falls.)
            || kv.get("targetname").is_some_and(|n| n.ends_with("_after"))
        {
            skip("a script's own (game object, exploder, crate, light_off)");
            continue;
        }
        let Some(&m) = by_name.get(&name) else {
            skip("model not in the zone or without geometry");
            if !above(origin) {
                if let Some(last) = skipped.last_mut() {
                    last.place = Some((origin, axis_of(vec3(kv, "angles").unwrap_or_default())));
                }
            }
            continue;
        };
        if above(origin) {
            skip("above the alternate level cut");
            continue;
        }
        let angles = vec3(kv, "angles").unwrap_or_default();
        out.push(ExtraProp {
            source: "script_model",
            model: m,
            origin,
            axis: axis_of(angles),
        });
    }
    (out, skipped)
}

/// The brush models (`"model" "*N"`, N > 0, with surfaces) the map's
/// `script_brushmodel*` entities place as it starts: not a later state
/// (`targetname "exploder"`: a wall once blown, as Prison Break's bomb-site
/// wall; `script_noteworthy "show"`: shown by a script, as Dart's breached
/// walls), not above `cut` (`alternate_level_cut`). Ghosts keeps these
/// surfaces outside the world's draw ranges and draws them with their
/// entities: Freight's sliding doors, Fahrenheit's elevator and its doors,
/// Flooded's running water, Prison Break's bomb-site wall. Brush models
/// with no entity are a script's to spawn and are left out. The clip map's
/// dynamic entities that are brush models (clutter built of brushes: glass
/// panes, shelves, signs; their vertices about the entity's origin) are
/// placed at their pose.
pub fn placed_brush_models(map: &IwMap, cut: Option<f32>) -> Vec<PlacedBrushModel> {
    let Some(w) = &map.world else {
        return Vec::new();
    };
    let text = named_entity_text(map.entities.trim_end_matches('\0'));
    let mut out = Vec::new();
    for block in text.split('{').skip(1) {
        let mut kv: HashMap<&str, &str> = HashMap::new();
        for line in block.lines() {
            let mut parts = line.split('"').skip(1).step_by(2);
            if let (Some(k), Some(v)) = (parts.next(), parts.next()) {
                kv.insert(k, v);
            }
        }
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
        let vec3 = |k: &str| -> Option<[f32; 3]> {
            let mut it = kv.get(k)?.split_whitespace().map(|x| x.parse::<f32>().ok());
            Some([it.next()??, it.next()??, it.next()??])
        };
        let origin = vec3("origin").unwrap_or_default();
        if cut.is_some_and(|c| origin[2] > c) {
            continue;
        }
        let angles = vec3("angles").unwrap_or_else(|| {
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
            brush: Some(n as u32),
        });
    }
    for d in map.clip.iter().flat_map(|c| c.dyn_ents.iter()) {
        if d.model.is_some() || d.brush_model == 0 || cut.is_some_and(|c| d.origin[2] > c) {
            continue;
        }
        let Some(b) = w.brush_models.get(usize::from(d.brush_model)) else {
            continue;
        };
        if b.surface_count == 0 || b.start_surf == u32::MAX {
            continue;
        }
        let start = b.start_surf as usize;
        out.push(PlacedBrushModel {
            surfaces: start..start + b.surface_count as usize,
            origin: d.origin,
            axis: axis_of_quat(d.quat),
            brush: None,
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
    /// The upper page's mean light (linear, luminance) over the texels the
    /// sun's baked visibility lights (`[0]`, `[1]` texels) and over those it
    /// leaves in shade (`[2]`, `[3]`): `iw3_lightmap_look`; zero elsewhere.
    pub split: [f64; 4],
    /// The lower page's (the directional light's) mean light over the same
    /// sunlit (`[0]`) and shaded (`[1]`) texels as `split`; with the upper
    /// page's it gives the baked light's whole level (`A + B f`).
    pub split_b: [f64; 2],
}

/// Ghosts' lightmap pair as Call of Duty 4's.
///
/// Ghosts' primary is BC4 at twice the secondary page's size (the sun's
/// baked visibility, as CoD4's L8 primary); its secondary is two pages
/// stacked (`R16G16B16A16_FLOAT`), the upper the softer, as CoD4's A and B:
/// linear light, alpha a tangent direction in -1..1. Here the light, scaled
/// by the map's exposure (`light_exposure`), becomes CoD4's gamma bytes (`sqrt`, clamped at 1),
/// the direction CoD4's alpha
/// code (`a * 4.08 - 2.08`), and the primary is box-filtered down to the
/// page's size (the full size made a 2048 x 6144 page per lightmap).
pub fn iw3_lightmap(
    cap: &Capture,
    image: &[u8],
    pair: (Option<AssetId>, Option<AssetId>),
) -> Result<Iw3Lightmap, String> {
    iw3_lightmap_exposed(cap, image, pair, LIGHT_EXPOSURE)
}

/// The exposure Ghosts' linear light is scaled by before it becomes CoD4's
/// gamma bytes (and the sun's gamma colour, `iw3_sun_gamma`), when a map's
/// own (`light_exposure`) cannot be measured.
///
/// Ghosts lights in HDR and brings it to the screen through each map's own
/// tone mapping (`toneMapping "mp_prisonbreak_tone_01"` in its vision):
/// the baked light (the secondary's two pages) of Prison Break sits at
/// 0.1 (median) .. 0.37 (90th percentile) under a sun of 9; Sovereign's at
/// 0.009 .. 0.067 under one of 0.6; Skeleton's at 0.33 .. 1.7 under 10.
/// CoD4's fallback draw sums gamma-space light (`A + B f + sun N.L vis`)
/// and squares it; taken 1 : 1 the sum was over 2 on every sunlit Prison
/// Break surface (burnt white) and Sovereign's halls were black.
pub const LIGHT_EXPOSURE: f32 = 1.0;

/// What a map's brighter baked light (its 90th percentile, linear) is
/// exposed to: CoD4's gamma bytes put it at `sqrt(EXPOSURE_KEY)`.
pub const EXPOSURE_KEY: f32 = 0.3;

/// The brightest the sun gets in CoD4's gamma space (a sunlit floor then
/// sits near 1 with its baked light): Ghosts' tone mapping compresses its
/// sun of 3..10 against the baked light the same way.
pub const SUN_GAMMA_MAX: f32 = 0.8;

/// A map's exposure from its baked light: `EXPOSURE_KEY` over the 90th
/// percentile of its lit lightmap texels' luminance (`lightmap_key`).
pub fn light_exposure(key: Option<f32>) -> f32 {
    key.filter(|k| *k > 1e-4)
        .map_or(LIGHT_EXPOSURE, |k| (EXPOSURE_KEY / k).clamp(0.05, 32.0))
}

/// The 90th percentile of the luminance of the lit texels of a map's
/// lightmap secondaries (both pages, linear light), every 7th texel read.
pub fn lightmap_key(
    cap: &Capture,
    image: &[u8],
    pairs: &[(Option<AssetId>, Option<AssetId>)],
) -> Option<f32> {
    let mut paks = crate::image::ImagePaks::new(std::path::Path::new("."));
    let mut lum: Vec<f32> = Vec::new();
    for &(_, s) in pairs {
        let Some(rec) = s.and_then(|i| cap.images.get(i.index as usize)) else {
            continue;
        };
        let Ok(px) = crate::image::image_pixels(rec, image, &mut paks, 1 << 14) else {
            continue;
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

/// Ghosts' sun (linear HDR, its brightest channel) as CoD4's gamma-space
/// `sunlight` under the same exposure as the lightmaps, at most
/// `SUN_GAMMA_MAX`.
pub fn iw3_sun_gamma(peak: f32, exposure: f32) -> f32 {
    (peak.max(0.0) * exposure).sqrt().min(SUN_GAMMA_MAX)
}

/// `iw3_lightmap` under a given exposure (`LIGHT_EXPOSURE`).
pub fn iw3_lightmap_exposed(
    cap: &Capture,
    image: &[u8],
    pair: (Option<AssetId>, Option<AssetId>),
    exposure: f32,
) -> Result<Iw3Lightmap, String> {
    iw3_lightmap_look(cap, image, pair, exposure, false)
}

/// `iw3_lightmap_exposed`, and with `linear` the light in the Black Ops III
/// look's packing (the square root of the light over 4: `t7_iw_pages` in the
/// fallback shader, as Modern Warfare Remastered's), so a map's own
/// exposure is not folded into gamma bytes; `split` is filled either way.
pub fn iw3_lightmap_look(
    cap: &Capture,
    image: &[u8],
    pair: (Option<AssetId>, Option<AssetId>),
    exposure: f32,
    linear: bool,
) -> Result<Iw3Lightmap, String> {
    iw3_lightmap_look_dir(cap, image, pair, exposure, linear, false)
}

/// The weight CoD4's lit shaders give the lower page on a flat surface,
/// `saturate(rsqrt(1 + x^2 + y^2))`, x and y decoded from the alpha bytes
/// this packing writes (the fallback shader's codes).
fn iw3_flat_weight(upper_alpha: u8, lower_alpha: u8) -> f32 {
    let x = f32::from(upper_alpha) / 255.0 * 4.08 - 2.08;
    let y = f32::from(lower_alpha) / 255.0 * 4.064_516 - 2.064_516;
    (1.0 / (1.0 + x * x + y * y).sqrt()).min(1.0)
}

/// `iw3_lightmap_look`; with `shader_dir` the lower (directional) page is
/// scaled so CoD4's flat-surface weight gives Ghosts' own: its shaders
/// weight that page by the baked direction's z (`shading::lightmap_flat_weight`,
/// `sqrt(1 - x^2 - y^2)`), not `rsqrt(1 + x^2 + y^2)`. Only the packed bytes
/// change (`split`, `split_b` are measured on the light as baked).
pub fn iw3_lightmap_look_dir(
    cap: &Capture,
    image: &[u8],
    pair: (Option<AssetId>, Option<AssetId>),
    exposure: f32,
    linear: bool,
    shader_dir: bool,
) -> Result<Iw3Lightmap, String> {
    let get = |id: Option<AssetId>| -> Result<&crate::walk::ImageRec, String> {
        id.and_then(|i| cap.images.get(i.index as usize))
            .ok_or_else(|| "image not in the zone".to_owned())
    };
    let (p, s) = (get(pair.0)?, get(pair.1)?);
    let mut paks = crate::image::ImagePaks::new(std::path::Path::new("."));
    let pp = crate::image::image_pixels(p, image, &mut paks, 1 << 14)?;
    let sp = crate::image::image_pixels(s, image, &mut paks, 1 << 14)?;
    if !matches!(pp.dxgi_format, 79 | 80) {
        return Err(format!("primary format {} (BC4 expected)", pp.dxgi_format));
    }
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
        let g = |v: f32| {
            let v = if v.is_finite() { v * exposure } else { 0.0 };
            let v = if linear { v * 0.25 } else { v };
            (v.clamp(0.0, 1.0).sqrt() * 255.0).round() as u8
        };
        let a = (((h(3).clamp(-1.0, 1.0) + 2.08) / 4.08) * 255.0)
            .round()
            .clamp(0.0, 255.0) as u8;
        secondary[i * 4..i * 4 + 4].copy_from_slice(&[g(h(2)), g(h(1)), g(h(0)), a]);
    }
    if shader_dir {
        let half = sw * (sh / 2);
        let alpha = |i: usize| {
            half_to_f32(u16::from_le_bytes([sp.data[i * 8 + 6], sp.data[i * 8 + 7]]))
                .clamp(-1.0, 1.0)
        };
        for i in half..sw * sh {
            let k = crate::shading::lightmap_flat_weight(alpha(i - half), alpha(i))
                / iw3_flat_weight(secondary[(i - half) * 4 + 3], secondary[i * 4 + 3]).max(1e-6);
            let h = |c: usize| {
                half_to_f32(u16::from_le_bytes([
                    sp.data[i * 8 + 2 * c],
                    sp.data[i * 8 + 2 * c + 1],
                ]))
            };
            let g = |v: f32| {
                let v = if v.is_finite() { v * exposure * k } else { 0.0 };
                let v = if linear { v * 0.25 } else { v };
                (v.clamp(0.0, 1.0).sqrt() * 255.0).round() as u8
            };
            secondary[i * 4..i * 4 + 3].copy_from_slice(&[g(h(2)), g(h(1)), g(h(0))]);
        }
    }
    // The primary, BC4 decoded.
    let (pw, ph) = (pp.width as usize, pp.height as usize);
    if pw % 4 != 0 || ph % 4 != 0 {
        return Err(format!("primary {pw}x{ph} is not whole blocks"));
    }
    let blocks = (pw / 4) * (ph / 4);
    if pp.data.len() < blocks * 8 {
        return Err(format!(
            "primary has {} of {} bytes",
            pp.data.len(),
            blocks * 8
        ));
    }
    let mut full = vec![0u8; pw * ph];
    for by in 0..ph / 4 {
        for bx in 0..pw / 4 {
            let b = (by * (pw / 4) + bx) * 8;
            let at = (by * 4) * pw + bx * 4;
            bcdec_rs::bc4(
                &pp.data[b..b + 8],
                &mut full[at..],
                pw,
                pp.dxgi_format == 81,
            );
        }
    }
    // Down to the page's size (whole steps only).
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
                    sum += u32::from(full[(y * fy + dy) * pw + x * fx + dx]);
                }
            }
            primary[y * ow + x] = (sum / (fx * fy) as u32) as u8;
        }
    }
    // The upper page's light (linear) against the sun's baked visibility at
    // the same place.
    let mut split = [0f64; 4];
    let (mut sl, mut ss) = (0f64, 0f64);
    let (mut bl, mut bs) = (0f64, 0f64);
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
                // (The directional page, the same texel one page down.)
                let j = (page_h + y) * sw + x;
                let hb = |c: usize| {
                    half_to_f32(u16::from_le_bytes([
                        sp.data[j * 8 + 2 * c],
                        sp.data[j * 8 + 2 * c + 1],
                    ]))
                };
                let lb = f64::from(0.3 * hb(0) + 0.59 * hb(1) + 0.11 * hb(2));
                let lb = if lb.is_finite() && lb > 0.0 { lb } else { 0.0 };
                if vis > 230 {
                    sl += l;
                    bl += lb;
                    split[1] += 1.0;
                } else if vis < 25 {
                    ss += l;
                    bs += lb;
                    split[3] += 1.0;
                }
            }
        }
    }
    split[0] = if split[1] > 0.0 { sl / split[1] } else { 0.0 };
    split[2] = if split[3] > 0.0 { ss / split[3] } else { 0.0 };
    let split_b = [
        if split[1] > 0.0 { bl / split[1] } else { 0.0 },
        if split[3] > 0.0 { bs / split[3] } else { 0.0 },
    ];
    let to16 = |v: usize| u16::try_from(v).map_err(|_| format!("{v} is too large"));
    Ok(Iw3Lightmap {
        primary: (to16(ow)?, to16(oh)?, primary),
        secondary: (to16(sw)?, to16(sh)?, secondary),
        split,
        split_b,
    })
}

/// Whether a map's baked light holds the sun's direct light, from its pages'
/// `split`s (`fastfile_h1::map::sun_in_lightmap`'s measure): the sunlit
/// texels' light over the shaded ones'.
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

/// How a map's baked light reads, over several pages (`Iw3Lightmap::split`
/// and `split_b`, weighted by their texels): the whole light (`A + 0.8 B`,
/// the directional page at the weight a typical surface gets) of the sunlit
/// and of the shaded texels, linear, and the upper page's shaded mean alone.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct BakedLight {
    pub sunlit_total: f32,
    pub shaded_total: f32,
    pub shaded_a: f32,
    pub texels_lit: f32,
    pub texels_shaded: f32,
}

/// The weight of the directional page in a surface's baked light
/// (`f = saturate(rsqrt(1 + x^2 + y^2))` of the lit shaders, on a typical
/// surface).
pub const DIRECTIONAL_WEIGHT: f64 = 0.8;

pub fn baked_light(pages: &[([f64; 4], [f64; 2])]) -> BakedLight {
    let (mut lit, mut nl, mut shade, mut ns, mut sa) = (0.0f64, 0.0f64, 0.0f64, 0.0f64, 0.0f64);
    for (s, b) in pages {
        lit += (s[0] + DIRECTIONAL_WEIGHT * b[0]) * s[1];
        nl += s[1];
        shade += (s[2] + DIRECTIONAL_WEIGHT * b[1]) * s[3];
        sa += s[2] * s[3];
        ns += s[3];
    }
    BakedLight {
        sunlit_total: if nl > 0.0 { (lit / nl) as f32 } else { 0.0 },
        shaded_total: if ns > 0.0 { (shade / ns) as f32 } else { 0.0 },
        shaded_a: if ns > 0.0 { (sa / ns) as f32 } else { 0.0 },
        texels_lit: nl as f32,
        texels_shaded: ns as f32,
    }
}

/// Whether a baked light holds the sun's own light (Modern Warfare
/// Remastered's maps do; Ghosts' do not: its sun is drawn live over a
/// sunlit-visibility mask). The sunlit texels must be well over the shaded
/// ones AND as bright as the sun itself: a sunlit texel that held the sun
/// would carry the sun's HDR light times its cosine, where a bounce-lit
/// open field is a fraction of that (mp_frag: sunlit 2.0 against a sun of
/// 8.0, ratio 4.6, was read as baked by the ratio alone).
pub fn sun_is_baked(light: &BakedLight, sun_lum: f32) -> bool {
    light.texels_lit > 100.0
        && light.texels_shaded > 100.0
        && light.shaded_total > 0.0
        && light.sunlit_total / light.shaded_total >= 2.5
        && light.sunlit_total >= 0.6 * sun_lum
}
