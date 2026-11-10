//! A Black Ops III map read for drawing the way BlackOps3.exe draws it: the
//! world's vertex streams and the static models' meshes as the zone and the
//! `.xpak` packages store them (bytes untouched), each surface's material
//! with its technique set's G-buffer technique (slot 3, draw method 0) in
//! full: programs, vertex declaration, arguments, state map.
//!
//! Vertex streams (the exe's input layouts, `layout`): the world is layout
//! group 3 (positions 12 bytes in slot 0; colour, UV, normal, tangent in 20
//! bytes in slot 1), the models group 1 (positions 12 bytes in slot 0;
//! colour, half UV, normal, tangent in 16 bytes in slot 1; skin weights 12
//! bytes in slot 2). Slot 15 is the per-instance index into the model
//! instance buffer (24 bytes: +0 first bone of a skinned draw (0: none), +12
//! the bone matrix placing it (0: none)); the bone matrices are 64 bytes
//! (rows: rotation | translation, three of them; the fourth unused by these
//! programs).

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use fastfile_t7::walk::capture::{Capture, ShaderArg};

use crate::state::StateMap;
use crate::zone_index::{self, Kind};

/// The technique a draw is made with (slot 3, draw method 0).
#[derive(Clone, Debug)]
pub struct Technique {
    pub techset: String,
    pub vs: Vec<u8>,
    pub ps: Vec<u8>,
    /// The vertex declaration (count, component ids).
    pub decl: Vec<u8>,
    pub args: Vec<ShaderArg>,
    pub state: StateMap,
    /// Which of the material's constant buffers the method takes
    /// (`technique * 6 + this (+2 vertex, +4 pixel)`).
    pub buffer_set: u8,
    /// The technique slot it was read from.
    pub slot: usize,
}

/// The G-buffer technique slot.
pub const GBUFFER_TECHNIQUE: usize = 3;
/// The depth prepass technique slot.
pub const PREPASS_TECHNIQUE: usize = 2;
/// The technique the exe draws its forward lists with (emissive, sky,
/// transparent, decals not in the G-buffer): the view's default technique
/// byte, set to 6 at renderer start (`141dad840`: `DAT_1486beea4 = 0x606`)
/// and read by the list draw (`141d45d40`) for every list whose table
/// entry is -1 or 255 (`142ed1220`).
pub const FORWARD_TECHNIQUE: usize = 6;
/// The draw method ordinary views use (the view's method byte; methods 0,
/// 2, 3, 4 carry the same programs on every Nuketown technique read).
pub const DRAW_METHOD: usize = 0;

#[derive(Clone, Debug, Default)]
pub struct Material {
    pub name: String,
    pub techset: String,
    pub technique: Option<Arc<Technique>>,
    /// The forward technique (slot 6) of a material with no G-buffer
    /// technique (`FORWARD_TECHNIQUE`).
    pub forward: Option<Arc<Technique>>,
    /// The depth prepass technique (slot 2), of a G-buffer material or a
    /// forward one (see [`Material::forward_depth`]).
    pub prepass: Option<Arc<Technique>>,
    /// The material's draw surface sort key (MaterialInfo +0x10), the
    /// order the exe's lists are sorted in.
    pub sort_key: u64,
    /// (name hash, image name) in table order.
    pub textures: Vec<(u32, String)>,
    /// (name hash, sampler state).
    pub samplers: Vec<(u32, u16)>,
    /// (slot = technique * 6 + k, buffer name, initial data).
    pub cbuffers: Vec<(u8, String, Vec<u8>)>,
    /// Why neither a G-buffer nor a forward technique was made (its draws
    /// are not drawn; `crate::completeness`).
    pub undrawn: Option<String>,
}

impl Material {
    /// The depth prepass technique of an opaque forward-only material
    /// (Nuk3town's road, `wc/lit_emissive_scroll_3layer_plus`): its forward
    /// technique tests depth (greater-equal) without writing it and does not
    /// blend, so it draws over the depth its slot 2 technique laid down
    /// before; without that depth a later forward surface behind it (the
    /// vista water under Nuk3town) drew over it (the cyan plaza). The
    /// G-buffer pass draws this technique (depth only: its pixel program
    /// writes no target). Sky techniques are left out (their vertex program
    /// places the dome at the eye). `T7_FORWARD_DEPTH=0`: none (the old way).
    pub fn forward_depth(&self) -> Option<&Arc<Technique>> {
        static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
        if !*ON.get_or_init(|| !std::env::var("T7_FORWARD_DEPTH").is_ok_and(|v| v == "0")) {
            return None;
        }
        let (f, p) = (self.forward.as_ref()?, self.prepass.as_ref()?);
        let s = &f.state;
        let opaque = crate::state::target_blend(s.blend[0]).is_ok_and(|(b, _)| b.is_none());
        let tests_without_write = s.depth & 0xe != 0 && s.depth & 1 == 0;
        (self.technique.is_none() && opaque && tests_without_write && !self.techset.contains("sky")).then_some(p)
    }

    /// What the G-buffer pass draws for this material: its G-buffer
    /// technique, else its forward depth ([`Self::forward_depth`]).
    pub fn gbuffer_pass(&self) -> Option<&Arc<Technique>> {
        self.technique.as_ref().or_else(|| self.forward_depth())
    }
}

/// One draw: a material's triangles from one vertex source.
#[derive(Clone, Copy, Debug)]
pub struct Draw {
    pub material: usize,
    /// 3: world streams, 1: model streams.
    pub group: u8,
    /// Into the source's index buffer.
    pub first_index: u32,
    pub index_count: u32,
    pub base_vertex: i32,
    /// Index into the model instance buffer (the first of `instances`).
    pub instance: u32,
    /// How many consecutive instance records this draw draws: every
    /// placement of one model shares a draw per surface.
    pub instances: u32,
    /// World-space bounds (for culling by the caller; the exe culls by
    /// its own visibility, not done here).
    pub mins: [f32; 3],
    pub maxs: [f32; 3],
    /// The view draw list the exe puts it in (see `forward`): world
    /// surfaces by their flags byte (+0x50); models and the sky in list 7
    /// (their lists are chosen in the model traversal, not decoded).
    pub list: u8,
    /// What it was made from: a world surface's index (group 3), an index
    /// into [`Scene::model_names`] (group 1), `u32::MAX` the sky.
    pub source: u32,
    /// A world surface's range in the GfxWorld (0..=3: A..D, see
    /// `surf_debug`; 4: in none of them); 255 for models.
    pub range: u8,
    /// A world surface's flags byte (+0x50); a static model's +0x60 byte.
    pub flags: u8,
    /// Its instances' bounds for the G-buffer's frustum culling: an index
    /// into [`Scene::cull_ranges`] ([`NO_CULL`]: the draw's own bounds, or
    /// none when they are empty).
    pub cull: u32,
    /// The static model LOD (index into the model's LODs, 0 the coarsest)
    /// this draw draws when its cull range picks LODs (`crate::lod`): the
    /// culling gives it only the placements at that LOD. [`ANY_LOD`]: every
    /// placement in view (no selection; the old way).
    pub lod: u8,
}

/// [`Draw::lod`] for a draw that takes every placement in view.
pub const ANY_LOD: u8 = 255;

impl Scene {
    /// Whether `d` is a coarser LOD's draw of a range split by LOD (drawn
    /// only through the GPU culling's picks; a path without them draws
    /// the finest LOD's draws over every placement instead).
    pub fn lod_alternate(&self, d: &Draw) -> bool {
        d.lod != ANY_LOD && self.cull_ranges.get(d.cull as usize).and_then(|r| r.lod.as_ref()).is_some_and(|l| l.finest != d.lod)
    }
}

/// [`Draw::cull`] for a draw with no per-instance bounds.
pub const NO_CULL: u32 = u32::MAX;

/// The bounding spheres (centre, radius) of one instanced draw's instances,
/// in instance order from `first` (static model placements, volume decals):
/// the G-buffer culls them one by one against the view (`crate::cull`).
#[derive(Clone, Debug, Default)]
pub struct CullRange {
    pub first: u32,
    pub spheres: Vec<[f32; 4]>,
    /// The model's LODs and its placements' LOD inputs, when this range's
    /// draws are split by LOD (`crate::lod::Mode::Exe`).
    pub lod: Option<crate::lod::RangeLod>,
}

/// The vertex data of one source, as stored.
#[derive(Default)]
pub struct Streams {
    pub positions: Vec<u8>,
    pub attributes: Vec<u8>,
    /// Skin weights (models only; 12 bytes per vertex, zero for rigid ones).
    pub weights: Vec<u8>,
    pub indices: Vec<u16>,
}

pub struct Scene {
    pub name: String,
    pub map: fastfile_t7::map::T7Map,
    pub materials: Vec<Material>,
    pub world: Streams,
    pub models: Streams,
    pub draws: Vec<Draw>,
    /// Model instance records (24 bytes each), entry 0 the world's.
    pub instances: Vec<[u32; 6]>,
    /// Bone matrices (16 floats each), entry 0 unused (index 0 means none).
    pub bones: Vec<[f32; 16]>,
    /// The sky box instance's bone matrix: moved to the eye every frame
    /// (`GBuffer::update`), see [`Scene::read_sky`].
    pub sky_bone: Option<usize>,
    /// What could not be read or will not be drawn, line by line.
    pub report: Vec<String>,
    /// The static models' names (`Draw::source` of group 1 draws).
    pub model_names: Vec<String>,
    /// Surfaces the scene made no draw of, by reason (counted per world
    /// surface / per model placement surface).
    pub not_drawn: Vec<(String, usize)>,
    /// Static model placements left out because their model's mesh could
    /// not be built: (model, origin, why). For finding which missing model
    /// leaves a hole (`t7_surf_at`).
    pub unbuilt: Vec<(String, [f32; 3], String)>,
    /// The world's four surface ranges (begin, end), when they read sanely.
    pub world_ranges: Option<[(u32, u32); 4]>,
    /// The folder the map zone is in (the shared zones, `core_patch`'s code
    /// shaders among them, are beside it).
    pub zone_dir: PathBuf,
    /// bo2mp: the map zone is not in BO3's own `zone` folder (a Steam
    /// Workshop item or a usermap: its shared zones are elsewhere).
    pub workshop: bool,
    /// The map's placed effects and their sprite materials (`crate::fx`).
    pub fx: crate::fx::FxScene,
    /// The model elements' XModels as model instance pools (with
    /// `BO2MP_T7_FX_MODELS=1`): per pool the index into
    /// `fx.model_names`, its first bone matrix and how many it has. Each
    /// frame (`GBuffer::update_fx_models`) the nearest particles of that
    /// model take its bones; the rest are all zero (drawn as nothing).
    pub fx_models: Vec<(usize, u32, u32)>,
    /// bo2mp: each placed static model instance and its draw record's index
    /// (`GfxWorld` +0x438), for the siege animations.
    pub instance_smodels: Vec<(u32, u32)>,
    /// bo2mp: the auto-played siege animations (`crate::siege`), advanced
    /// each frame by `GBuffer::update_siege`.
    pub siege: std::sync::Mutex<crate::siege::Siege>,
    /// Constant buffers made up for materials whose record held them only
    /// as unresolved references (`fill_missing_cbuffers`), one line each.
    pub cbuffers_filled: Vec<String>,
    /// Techniques completed from donors (`Zones::technique_donor`): (set,
    /// slot).
    pub donated: Vec<(String, usize)>,
    /// bo2mp: the volume decals' `volDecalInstanceBuffer` elements
    /// (`crate::volume_decals`), in draw order: each material's decals are
    /// one instanced draw of the box mesh (group 1), its instance range
    /// indexing these.
    pub volume_decals: Vec<[f32; crate::volume_decals::ELEMENT_FLOATS]>,
    /// Per-instance bounds of the instanced draws that have them
    /// ([`Draw::cull`]).
    pub cull_ranges: Vec<CullRange>,
}

/// Instances (bone matrices) per effect model pool.
pub const FX_MODEL_POOL: u32 = 32;

/// The zones whose technique sets a map's materials can use, beside the
/// map's own (searched in this order).
/// Says whether a scene being read is still wanted (`Scene::load_unless`).
pub type GivenUp = Arc<dyn Fn() -> bool + Send + Sync>;

/// The error of a scene read that stopped because it was no longer wanted.
pub const LET_GO: &str = "the scene read stopped: the map changed";

const COMMON_ZONES: [&str; 5] = ["mp_common", "core_common", "core_post_gfx", "core_pre_gfx", "core_gfx"];

/// Nanoseconds spent opening shared zones / reading package meshes.
static ZONE_OPEN_NS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
static MESH_READ_NS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// The zones read so far: the map's (first), then shared zones as a
/// material's technique set needs them (each read and walked once per
/// process: `fastfile_t7::shared`).
struct Zones {
    list: Vec<(String, Arc<Vec<u8>>, Arc<Capture>)>,
    /// Each technique read once per (technique set, slot), shared by every
    /// material of the set (one pipeline each, as the exe has one
    /// technique).
    techniques: HashMap<(String, usize), Result<Arc<Technique>, String>>,
    dir: PathBuf,
    /// The shared zones in search order, and those not opened yet.
    order: Vec<String>,
    unopened: Vec<String>,
    /// (zones they were made from, the donors): `technique_donor`.
    donors: (usize, Donors),
    /// Techniques put together by `technique_donor` (set, slot).
    donated: Vec<(String, usize)>,
    /// Whether the scene is no longer wanted (`Scene::load_unless`): no
    /// more zones are opened and the read ends.
    given_up: GivenUp,
}

/// Vertex declarations by vertex program and state maps by vertex and
/// pixel program (and by pixel program), from every technique read whole.
#[derive(Default)]
struct Donors {
    decl: HashMap<u64, Vec<u8>>,
    state: HashMap<u64, Vec<u8>>,
    state_ps: HashMap<u64, Vec<u8>>,
    /// By technique set kind and slot ([`kind`]: `wc/lit_decal_reveal#86de3377`
    /// takes `mc/lit_decal_reveal#e4cce504`'s: the world and model variants
    /// of a set differ in their vertex layout, not in what they blend into
    /// which target).
    state_family: HashMap<(String, usize), Vec<u8>>,
}

impl Zones {
    fn given_up(&self) -> bool {
        (self.given_up)()
    }

    /// An error once the scene is no longer wanted.
    fn go_on(&self) -> Result<(), String> {
        if self.given_up() { Err(LET_GO.to_owned()) } else { Ok(()) }
    }

    fn technique(&mut self, techset: &str, slot: usize) -> Option<Result<Arc<Technique>, String>> {
        let key = (techset.to_owned(), slot);
        if let Some(t) = self.techniques.get(&key) {
            return Some(t.clone());
        }
        let mut r = {
            let (z, ts) = self.techset(techset)?;
            technique(z, ts, slot).map(Arc::new)
        };
        // A technique whose vertex declaration or state map the zone holds
        // only as a reference the walk could not follow (Workshop maps,
        // Gauntlet, Metro: `wc/lit#882572b8`, the decals): the same
        // technique set whole in another zone, else the missing part from
        // a technique of this zone with the same programs.
        // `CODLEGACY_T7_NO_DONOR=1`: not (the old way, for before/after counts).
        if let Err(e) = &r
            && (e == "no vertex declaration" || e == "no state map")
            && codlegacy_env::var("CODLEGACY_T7_NO_DONOR").is_err()
            && let Some(t) = self.technique_elsewhere(techset, slot)
        {
            r = Ok(Arc::new(t));
        }
        self.techniques.insert(key, r.clone());
        Some(r)
    }

    /// Technique `slot` of technique set `name` from a zone other than the
    /// first that has the set (opening the shared zones not read yet), else
    /// put together: the set's own technique with its missing vertex
    /// declaration / state map from any technique read with the same vertex
    /// / both programs.
    fn technique_elsewhere(&mut self, name: &str, slot: usize) -> Option<Technique> {
        let whole = |z: &[u8], c: &Capture| c.techset(name).and_then(|ts| technique(z, ts, slot).ok());
        if let Some(k) = self.search(Kind::Technique, &format!("{slot}:{}", bare(name)), true, &|z, c| whole(z, c).is_some()) {
            return whole(&self.list[k].1, &self.list[k].2);
        }
        let mut t = self.technique_donor(name, slot);
        // (The donors come from the zones read: with the zone index fewer
        // are, so a technique without one reads the rest, as before it.)
        if t.is_none() && zone_index::on() && !self.unopened.is_empty() {
            while !self.unopened.is_empty() && !self.given_up() {
                let zn = self.unopened.remove(0);
                self.open(zn);
            }
            t = self.technique_donor(name, slot);
        }
        if t.is_some() {
            self.donated.push((bare(name).to_owned(), slot));
        }
        t
    }

    /// Technique `slot` of set `name` with its unresolved vertex
    /// declaration from a technique with the same vertex program and its
    /// unresolved state map from one with the same vertex and pixel
    /// programs (else the same pixel program), in any zone read.
    fn technique_donor(&mut self, name: &str, slot: usize) -> Option<Technique> {
        use std::hash::{Hash, Hasher};
        let h = |parts: &[&[u8]]| {
            let mut s = std::collections::hash_map::DefaultHasher::new();
            parts.hash(&mut s);
            s.finish()
        };
        self.techset(name)?;
        if self.donors.0 != self.list.len() {
            let mut d = Donors::default();
            for (_, z, c) in &self.list {
                for ts in &c.techsets {
                    let fam = kind(&ts.name).to_owned();
                    for (slot, info) in ts.techniques.iter().enumerate() {
                        let Some(info) = info else { continue };
                        let Some(m) = info.methods[DRAW_METHOD].as_ref() else { continue };
                        let (Some(vs), Some(ps)) = (m.programs[0].and_then(|(a, n)| z.get(a..a + n)), m.programs[1].and_then(|(a, n)| z.get(a..a + n))) else { continue };
                        if let Some(decl) = m.decl.and_then(|a| z.get(a..a + 0x78)) {
                            d.decl.entry(h(&[vs])).or_insert_with(|| decl.to_vec());
                        }
                        if let Some(st) = info.state_map.and_then(|a| z.get(a..a + 112)) {
                            d.state.entry(h(&[vs, ps])).or_insert_with(|| st.to_vec());
                            d.state_ps.entry(h(&[ps])).or_insert_with(|| st.to_vec());
                            d.state_family.entry((fam.clone(), slot)).or_insert_with(|| st.to_vec());
                        }
                    }
                }
            }
            self.donors = (self.list.len(), d);
        }
        let (img, ts) = self.find(name)?;
        let info = ts.techniques[slot].as_ref()?;
        let m = info.methods[DRAW_METHOD].as_ref()?;
        let vs = m.programs[0].and_then(|(a, n)| img.get(a..a + n))?.to_vec();
        let ps = m.programs[1].and_then(|(a, n)| img.get(a..a + n))?.to_vec();
        let d = &self.donors.1;
        let decl = m.decl.and_then(|a| img.get(a..a + 0x78)).map(<[u8]>::to_vec).or_else(|| d.decl.get(&h(&[&vs])).cloned())?;
        let state = info.state_map.and_then(|a| img.get(a..a + 112)).or_else(|| {
                let fam = (kind(&ts.name).to_owned(), slot);
                d.state.get(&h(&[&vs, &ps])).or_else(|| d.state_ps.get(&h(&[&ps]))).or_else(|| d.state_family.get(&fam)).map(Vec::as_slice)
            }).and_then(StateMap::read)?;
        Some(Technique { techset: bare(&ts.name).to_owned(), vs, ps, decl, args: m.args.clone(), state, buffer_set: m.buffer_set, slot })
    }

    fn find(&self, name: &str) -> Option<(&[u8], &fastfile_t7::walk::capture::TechsetRec)> {
        self.list.iter().find_map(|(_, z, c)| c.techset(name).filter(|t| findable(t)).map(|t| (z.as_slice(), t)))
    }

    /// The first zone in search order (the map's, unless `skip_map`, then
    /// the shared zones) that `hit` says has what is looked for, by its
    /// place in `list`: opening on the way the shared zones not read yet,
    /// one at a time, but those the zone index says lack `name` as `kind`.
    fn search(&mut self, kind: Kind, name: &str, skip_map: bool, hit: &dyn Fn(&[u8], &Capture) -> bool) -> Option<usize> {
        if !skip_map && hit(&self.list[0].1, &self.list[0].2) {
            return Some(0);
        }
        for i in 0..self.order.len() {
            let zn = self.order[i].clone();
            if let Some(k) = self.list.iter().skip(1).position(|(n, _, _)| *n == zn).map(|k| k + 1) {
                if hit(&self.list[k].1, &self.list[k].2) {
                    return Some(k);
                }
                continue;
            }
            let Some(u) = self.unopened.iter().position(|n| *n == zn) else { continue };
            if self.given_up() {
                return None;
            }
            if zone_index::lacks(&self.dir.join(format!("{zn}.ff")), kind, name) {
                continue;
            }
            let zn = self.unopened.remove(u);
            if self.open(zn) && hit(&self.list[self.list.len() - 1].1, &self.list[self.list.len() - 1].2) {
                return Some(self.list.len() - 1);
            }
        }
        None
    }

    /// Opens the shared zone `zn` (false: not there or not readable),
    /// writing its names to the zone index the first time.
    fn open(&mut self, zn: String) -> bool {
        let t_open = std::time::Instant::now();
        let opened = fastfile_t7::shared::zone(&self.dir, &zn);
        ZONE_OPEN_NS.fetch_add(t_open.elapsed().as_nanos() as u64, std::sync::atomic::Ordering::Relaxed);
        let Some(z) = opened else { return false };
        zone_index::note(&self.dir.join(format!("{zn}.ff")), || zone_names(&z.image, &z.capture));
        self.list.push((zn, z.image.clone(), z.capture.clone()));
        true
    }

    /// The full record of material `name` (one with a technique set): from
    /// the zones read, else from the next shared zones, opened one at a
    /// time until one has it. A map zone holds a model's material from
    /// `mp_common` or `core_common` only as a reference (`,mc/...`, no
    /// technique set read): Skyjacked's chrome trims, robots, weapons and
    /// vehicle decals (149 draws) were left out for that.
    fn material_def(&mut self, name: &str) -> Option<Arc<Capture>> {
        let k = self.search(Kind::Material, name, false, &|_, c| material_whole(c, name))?;
        Some(self.list[k].2.clone())
    }

    /// A model whole (`read` says it has a mesh with data) in a zone other
    /// than the map's: (that zone's index, its LODs and meshes), opening
    /// shared zones one at a time until one has it.
    #[allow(clippy::type_complexity)]
    fn model_def<L, M>(&mut self, name: &str, read: &dyn Fn(&Capture) -> Option<(Vec<L>, Vec<M>, bool)>) -> Option<(usize, Vec<L>, Vec<M>)> {
        let k = self.search(Kind::Model, name, true, &|_, c| read(c).is_some_and(|r| r.2))?;
        let (l, m, _) = read(&self.list[k].2)?;
        Some((k, l, m))
    }

    /// The technique set `name`: from the zones read, else from the next
    /// shared zones, opened one at a time until one has it.
    fn techset(&mut self, name: &str) -> Option<(&[u8], &fastfile_t7::walk::capture::TechsetRec)> {
        let k = self.search(Kind::Techset, bare(name), false, &|_, c| c.techset(name).is_some_and(findable))?;
        let (_, z, c) = &self.list[k];
        Some((z.as_slice(), c.techset(name)?))
    }
}

/// A technique set the scene can draw with (a G-buffer technique or programs).
fn findable(t: &fastfile_t7::walk::capture::TechsetRec) -> bool {
    t.techniques[GBUFFER_TECHNIQUE].is_some() || t.programs.iter().any(Option::is_some)
}

/// Whether a zone has material `name` whole (with its technique set).
fn material_whole(c: &Capture, name: &str) -> bool {
    c.material(name).is_some_and(|m| m.techset.as_deref().is_some_and(|t| !bare(t).is_empty()))
}

/// Model `name`'s LODs and meshes in a zone, and whether any mesh has
/// data of its own.
#[allow(clippy::type_complexity)]
fn model_parts(cap: &Capture, name: &str) -> Option<(Vec<fastfile_t7::walk::capture::LodRec>, Vec<Option<fastfile_t7::walk::capture::MeshRec>>, bool)> {
    let x = cap.model(name).or_else(|| cap.model(&format!(",{name}")))?;
    let lods = x.lods.clone();
    let meshes: Vec<_> = lods.iter().map(|l| cap.mesh(bare(&l.mesh)).or_else(|| cap.mesh(&l.mesh)).cloned()).collect();
    let has = meshes.iter().flatten().any(|m| m.data.is_some() || m.xpak.is_some());
    Some((lods, meshes, has))
}

/// The names a shared zone answers, for the zone index: each as
/// `material_def`, `techset`, `technique_elsewhere` and `model_def` look
/// it up, so the index says no only where the lookup would.
fn zone_names(img: &[u8], c: &Capture) -> zone_index::Names {
    let mut n = zone_index::Names::default();
    for m in &c.materials {
        if material_whole(c, &m.name) {
            n.add(Kind::Material, m.name.clone());
        }
    }
    for ts in &c.techsets {
        let b = bare(&ts.name);
        let Some(t) = c.techset(b) else { continue };
        if findable(t) {
            n.add(Kind::Techset, b.to_owned());
        }
        for slot in 0..t.techniques.len() {
            if technique(img, t, slot).is_ok() {
                n.add(Kind::Technique, format!("{slot}:{b}"));
            }
        }
    }
    for x in &c.models {
        for q in [x.name.as_str(), bare(&x.name)] {
            if model_parts(c, q).is_some_and(|r| r.2) {
                n.add(Kind::Model, q.to_owned());
            }
        }
    }
    n
}

/// A shadow-only material, by the words of its name or its technique
/// set's (`wc/shadowcaster`, `mc/shadowcaster#...`, `..._onlycast`).
pub(crate) fn is_shadow_caster(material: &str, techset: &str) -> bool {
    material.split(['_', '/', '#']).chain(techset.split(['_', '/', '#'])).any(|w| w.eq_ignore_ascii_case("shadowcaster") || w.eq_ignore_ascii_case("onlycast"))
}

/// A technique set's kind: its name without the vertex type prefix (`wc/`,
/// `mc/`, `vd/`...) and the `#hash`.
fn kind(techset: &str) -> &str {
    let n = bare(techset).split('#').next().unwrap_or("");
    n.split_once('/').map_or(n, |(_, k)| k)
}

fn bare(s: &str) -> &str {
    s.trim_start_matches(',')
}

fn technique(img: &[u8], ts: &fastfile_t7::walk::capture::TechsetRec, slot: usize) -> Result<Technique, String> {
    let info = ts.techniques[slot].as_ref().ok_or(if slot == GBUFFER_TECHNIQUE { "no G-buffer technique".to_owned() } else { format!("no technique in slot {slot}") })?;
    let m = info.methods[DRAW_METHOD].as_ref().ok_or("no draw method 0")?;
    let prog = |p: Option<(usize, usize)>, what: &str| -> Result<Vec<u8>, String> {
        let (at, n) = p.ok_or_else(|| format!("no {what} program"))?;
        img.get(at..at + n).map(<[u8]>::to_vec).ok_or_else(|| format!("{what} program past the zone"))
    };
    let decl = m.decl.and_then(|d| img.get(d..d + 0x78)).ok_or("no vertex declaration")?.to_vec();
    let state = info.state_map.and_then(|s| img.get(s..s + 112)).and_then(StateMap::read).ok_or("no state map")?;
    Ok(Technique {
        techset: bare(&ts.name).to_owned(),
        vs: prog(m.programs[0], "vertex")?,
        ps: prog(m.programs[1], "pixel")?,
        decl,
        args: m.args.clone(),
        state,
        buffer_set: m.buffer_set,
        slot,
    })
}

fn f32_at(b: &[u8], o: usize) -> f32 {
    f32::from_le_bytes(b[o..o + 4].try_into().unwrap())
}
fn v3(b: &[u8], o: usize) -> [f32; 3] {
    [f32_at(b, o), f32_at(b, o + 4), f32_at(b, o + 8)]
}

impl Scene {
    /// Reads the map zone `zone` (its packages and the common zones beside
    /// it). Errors only when the zone or its world cannot be read.
    pub fn load(zone: &Path) -> Result<Self, String> {
        Self::load_unless(zone, Arc::new(|| false))
    }

    /// As `load`, but stops (`LET_GO`) once `given_up` says the scene is no
    /// longer wanted (the game went to another map while it was read: a
    /// cold read can take minutes), letting go of the map, the scene so far
    /// and the shared zones with it; a map the map load hands over after
    /// that is not kept.
    pub fn load_unless(zone: &Path, given_up: GivenUp) -> Result<Self, String> {
        let ticket = fastfile_t7::map::handed_ticket();
        let stopped = || {
            fastfile_t7::map::not_wanted(zone, ticket);
            fastfile_t7::shared::release_keeping(&["core_common", "mp_common"]);
            LET_GO.to_owned()
        };
        // The map the map load read (handed over), else read here.
        let t = std::time::Instant::now();
        let end = t + std::time::Duration::from_secs(1800);
        let handed = loop {
            if given_up() {
                return Err(stopped());
            }
            if let Some(m) = fastfile_t7::map::wait_handed(zone, std::time::Duration::from_millis(250)) {
                break Some(m);
            }
            // (Handed over or given up since the wait: no wait is left.)
            if !fastfile_t7::map::handed_expected(zone) || std::time::Instant::now() >= end {
                break fastfile_t7::map::take_handed(zone);
            }
        };
        let (map, handed) = match handed {
            Some(m) => (m, true),
            None => (fastfile_t7::map::T7Map::load(&fastfile_t7::map::MapSources::beside(zone))?, false),
        };
        let read = t.elapsed().as_secs_f32();
        let mut scene = match Self::from_map_unless(zone, map, given_up) {
            Err(e) if e == LET_GO => return Err(stopped()),
            s => s?,
        };
        scene.report.push(format!("timing: zone map {} {:.2}s; scene from the map {:.2}s", if handed { "handed over" } else { "read" }, read, t.elapsed().as_secs_f32() - read));
        // (The scene is made: of the shared zones the post chain's vision
        // is still to be read; the others are let go, their memory with them.)
        fastfile_t7::shared::release_keeping(&["core_common", "mp_common"]);
        Ok(scene)
    }

    /// The scene of a map already read (`T7Map::load`), without reading its
    /// zone again; the shared zones come from the process-wide cache.
    pub fn from_map(zone: &Path, map: fastfile_t7::map::T7Map) -> Result<Self, String> {
        Self::from_map_unless(zone, map, Arc::new(|| false))
    }

    /// As `from_map`, stopping (`LET_GO`) once `given_up` says so.
    pub fn from_map_unless(zone: &Path, mut map: fastfile_t7::map::T7Map, given_up: GivenUp) -> Result<Self, String> {
        // (The shared zones' folder: the map's own for a retail map, BO3's
        // `zone` for a Workshop item or usermap: `shared_dir_for`.)
        let dir: PathBuf = fastfile_t7::map::shared_dir_for(zone);
        let sky = sky_picture(&mut map);
        let (image, capture) = map.zone_data().ok_or("the map was read without its zone")?;
        let name = zone.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
        // The shared zones a map's technique sets can come from: the common
        // ones first, then any other shared zone beside the map (core_*,
        // code_*, *patch*, common*; not other maps).
        let mut unopened: Vec<String> = COMMON_ZONES.iter().map(|z| (*z).to_owned()).collect();
        if let Ok(rd) = std::fs::read_dir(&dir) {
            let mut extra: Vec<String> = rd
                .flatten()
                .filter_map(|e| e.path().file_stem().map(|s| s.to_string_lossy().into_owned()).filter(|_| e.path().extension().is_some_and(|x| x == "ff")))
                .filter(|n| (n.starts_with("core_") || n.starts_with("code_") || n.contains("patch") || n.starts_with("common")) && !unopened.contains(n))
                .collect();
            extra.sort();
            unopened.extend(extra);
        }
        let mut zones = Zones { list: vec![(name.clone(), image, capture)], techniques: HashMap::new(), dir: dir.clone(), order: unopened.clone(), unopened, donors: (0, Donors::default()), donated: Vec::new(), given_up };
        let mut s = Scene {
            name,
            map,
            materials: Vec::new(),
            world: Streams::default(),
            models: Streams::default(),
            draws: Vec::new(),
            instances: vec![[0; 6]],
            bones: vec![bone_zero()],
            sky_bone: None,
            report: Vec::new(),
            model_names: Vec::new(),
            not_drawn: Vec::new(),
            unbuilt: Vec::new(),
            world_ranges: None,
            zone_dir: dir.clone(),
            workshop: zone.parent().is_some_and(|own| own != dir.as_path()),
            fx: crate::fx::FxScene::default(),
            fx_models: Vec::new(),
            instance_smodels: Vec::new(),
            siege: Default::default(),
            cbuffers_filled: Vec::new(),
            donated: Vec::new(),
            volume_decals: Vec::new(),
            cull_ranges: Vec::new(),
        };
        let mut by_name: HashMap<String, Option<usize>> = HashMap::new();
        let t = std::time::Instant::now();
        s.read_world(&mut zones, &dir, &mut by_name)?;
        let t_world = t.elapsed().as_secs_f32();
        s.read_models(&mut zones, &dir, &mut by_name);
        zones.go_on()?;
        s.read_siege(&zones);
        let t_models = t.elapsed().as_secs_f32() - t_world;
        if let Some(line) = sky {
            s.report.push(line);
        }
        s.read_sky(&mut zones, &dir, &mut by_name);
        s.read_effects(&mut zones, &dir, &mut by_name);
        zones.go_on()?;
        // (Workshop zones: technique and constant buffer references to data
        // a later asset loads, looked up after the walk.)
        if let Some((_, _, c)) = zones.list.first()
            && c.technique_refs_settled > 0
        {
            s.report.push(format!("zone references to shaders, vertex declarations, state maps and constant buffers read later, looked up after the walk: {}", c.technique_refs_settled));
        }
        if !zones.donated.is_empty() {
            s.report.push(format!(
                "techniques whose vertex declaration / state map was an unresolved reference, completed from a technique with the same programs: {} ({}{})",
                zones.donated.len(),
                zones.donated.iter().take(12).map(|(n, k)| format!("{n} slot {k}")).collect::<Vec<_>>().join(", "),
                if zones.donated.len() > 12 { ", ..." } else { "" }
            ));
        }
        if !s.cbuffers_filled.is_empty() {
            let zeros = s.cbuffers_filled.iter().filter(|l| l.ends_with("not made")).count();
            s.report.push(format!(
                "constant buffers the material records hold only as unresolved references, made from the programs' layouts: {} ({zeros} with no source, not made): {}{}",
                s.cbuffers_filled.len(),
                s.cbuffers_filled.iter().take(8).cloned().collect::<Vec<_>>().join("; "),
                if s.cbuffers_filled.len() > 8 { "; ..." } else { "" }
            ));
        }
        s.donated = std::mem::take(&mut zones.donated);
        s.leave_out_shadow_casters();
        s.apply_debug_switches();
        // One line per map: what is in the map and what is not drawn, why.
        s.report.push(crate::completeness::tally(&s).line(&s.name));
        // Which way the world and the models wind (decides the world's front
        // face), the largest floors and the draws per technique set.
        let lines = crate::winding::analyse(&s);
        s.report.extend(lines);
        // (After the winding check: the box is not map geometry.)
        s.read_volume_decals(&mut zones, &dir, &mut by_name);
        s.report.push(format!(
            "timing: scene world {t_world:.2}s, models {t_models:.2}s, sky {:.2}s (opening shared zones {:.2}s, package meshes {:.2}s)",
            t.elapsed().as_secs_f32() - t_world - t_models,
            ZONE_OPEN_NS.load(std::sync::atomic::Ordering::Relaxed) as f32 * 1e-9,
            MESH_READ_NS.load(std::sync::atomic::Ordering::Relaxed) as f32 * 1e-9
        ));
        Ok(s)
    }

    fn material(&mut self, zones: &mut Zones, _dir: &Path, by_name: &mut HashMap<String, Option<usize>>, name: &str) -> Option<usize> {
        let name = bare(name).to_owned();
        if let Some(i) = by_name.get(&name) {
            return *i;
        }
        let mut cap = zones.list[0].2.clone();
        let Some(local) = cap.material(&name).or_else(|| cap.material(&format!(",{name}"))) else {
            self.report.push(format!("material {name}: not in the zone"));
            by_name.insert(name, None);
            return None;
        };
        // A reference without its technique set: the full record from the
        // shared zone that has it (its technique set, pictures, constants).
        let mut shared = false;
        if local.techset.as_deref().map(bare).unwrap_or_default().is_empty()
            && !crate::surf_debug::get().no_shared
            && let Some(c) = zones.material_def(&name)
        {
            cap = c;
            shared = true;
        }
        let m = cap.material(&name).or_else(|| cap.material(&format!(",{name}")))?;
        let techset = m.techset.as_deref().map(bare).unwrap_or_default().to_owned();
        // The image names as the map reads them (references named by their
        // siblings), else the walk's.
        // (A shared zone's record: its own names.)
        let textures = self.map.materials.iter().find(|mm| !shared && mm.name == name).map(|mm| mm.textures.clone()).unwrap_or_else(|| m.textures.iter().map(|(h, n)| (*h, bare(n).to_owned())).collect());
        let samplers = m.sampler_defs.iter().map(|d| (u32::from_le_bytes(d[0..4].try_into().unwrap()), u16::from_le_bytes([d[4], d[5]]) & 0x3fff)).collect();
        let cbuffers = m.cbuffers.clone();
        let mut mat = Material { name: name.clone(), techset: techset.clone(), textures, samplers, cbuffers, sort_key: m.sort_key, ..Default::default() };
        match zones.technique(&techset, GBUFFER_TECHNIQUE) {
            Some(Ok(t)) => {
                mat.technique = Some(t);
                mat.prepass = zones.technique(&techset, PREPASS_TECHNIQUE).and_then(Result::ok);
            }
            Some(Err(e)) => match zones.technique(&techset, FORWARD_TECHNIQUE) {
                Some(Ok(t)) => {
                    mat.forward = Some(t);
                    mat.prepass = zones.technique(&techset, PREPASS_TECHNIQUE).and_then(Result::ok);
                }
                Some(Err(f)) => {
                    self.report.push(format!("material {name} ({techset}): {e}; {f} (not drawn)"));
                    mat.undrawn = Some(format!("{e}; {f}"));
                }
                None => mat.undrawn = Some(e.clone()),
            },
            None => {
                self.report.push(format!("material {name}: technique set {techset} in no zone read"));
                mat.undrawn = Some(if techset.is_empty() { "no technique set (shared-zone reference not found)".into() } else { "technique set in no zone read".into() });
            }
        }
        self.fill_missing_cbuffers(&mut mat);
        self.materials.push(mat);
        let i = Some(self.materials.len() - 1);
        by_name.insert(name, i);
        i
    }

    /// A constant buffer a technique's program reads from the material
    /// (not a code buffer) that the material's record does not hold: the
    /// zone has it as a reference to an earlier material's buffer that the
    /// walk could not follow (Rome, Stalingrad, Genesis, Western, Warhawk:
    /// `constant buffer $Globals (b0, material slot 22) not in the
    /// material`, 70% of Rome's draws). Made from the program's layout,
    /// each variable from the material's own numbers by name (its other
    /// techniques' buffers, `MapMaterial::constants`), else the whole
    /// buffer of the first material of the same technique set that has it
    /// (the reference shares one of them). `CODLEGACY_T7_NO_CBUF_FILL=1`: not.
    fn fill_missing_cbuffers(&mut self, mat: &mut Material) {
        if codlegacy_env::var("CODLEGACY_T7_NO_CBUF_FILL").is_ok() {
            return;
        }
        let techs: Vec<Arc<Technique>> = [&mat.technique, &mat.prepass, &mat.forward].into_iter().flatten().cloned().collect();
        for t in techs {
            for (st, program) in [(0u16, &t.vs), (1u16, &t.ps)] {
                let index = (t.slot * 6) as u8 + t.buffer_set + if st == 0 { 2 } else { 4 };
                if mat.cbuffers.iter().any(|(i, _, _)| *i == index) {
                    continue;
                }
                let binds = crate::rdef::bindings(program);
                let Some(refl) = fastfile_t7::dxbc::reflect(program) else { continue };
                for b in binds.iter().filter(|b| b.kind == crate::rdef::CBUFFER) {
                    // (Code buffers are filled by the renderer.)
                    if t.args.iter().any(|a| a.ty & 0xff00 == 0x400 && a.ty & 0xff == st && u32::from(a.buffer) == b.point) {
                        continue;
                    }
                    let Some(layout) = refl.buffers.iter().find(|c| c.name == b.name) else { continue };
                    let own = self.map.materials.iter().find(|mm| mm.name == mat.name).map(|mm| &mm.constants).filter(|c| !c.is_empty());
                    let mut data = vec![0u8; layout.size as usize];
                    let mut named = 0;
                    if let Some(c) = own {
                        for v in &layout.variables {
                            let Some(val) = c.get(&v.name) else { continue };
                            for (k, x) in val.iter().enumerate().take(v.size as usize / 4) {
                                let o = v.offset as usize + 4 * k;
                                if let Some(d) = data.get_mut(o..o + 4) {
                                    d.copy_from_slice(&x.to_le_bytes());
                                }
                            }
                            named += 1;
                        }
                    }
                    let how = if named > 0 {
                        // The variables the material does not name (a vehicle
                        // paint's tint, alpha and blend factors among them)
                        // would stay zero: see-through white. Seeded from a
                        // sibling of the same technique set that holds the
                        // buffer, the named ones kept (2026-10-27;
                        // `CODLEGACY_T7_CBUF_PARTIAL_ZERO=1`: left zero).
                        let mut seeded = String::new();
                        if named < layout.variables.len()
                            && codlegacy_env::var("CODLEGACY_T7_CBUF_PARTIAL_ZERO").is_err()
                            && let Some(sib) = self.materials.iter().find(|m| {
                                m.techset == mat.techset
                                    && m.cbuffers.iter().any(|(i, n, d)| {
                                        *i == index && n == &b.name && d.len() == data.len()
                                    })
                            })
                            && let Some((_, _, sd)) = sib
                                .cbuffers
                                .iter()
                                .find(|(i, n, _)| *i == index && n == &b.name)
                        {
                            let mut covered = vec![false; data.len()];
                            if let Some(c) = own {
                                for v in layout.variables.iter().filter(|v| c.contains_key(&v.name)) {
                                    for k in v.offset as usize..(v.offset + v.size) as usize {
                                        if let Some(x) = covered.get_mut(k) {
                                            *x = true;
                                        }
                                    }
                                }
                            }
                            for (k, d) in data.iter_mut().enumerate() {
                                if !covered[k] {
                                    *d = sd[k];
                                }
                            }
                            seeded = format!(", the rest from {}", sib.name);
                        }
                        format!("{named} of {} variables by name{seeded}", layout.variables.len())
                    } else if let Some(sib) = self.materials.iter().find(|m| m.techset == mat.techset && m.cbuffers.iter().any(|(i, n, d)| *i == index && n == &b.name && d.len() == data.len())) {
                        data = sib.cbuffers.iter().find(|(i, n, _)| *i == index && n == &b.name).map(|c| c.2.clone()).unwrap_or(data);
                        format!("from {}", sib.name)
                    } else {
                        // (No numbers of its own anywhere: left out, its
                        // draws skipped as before.)
                        self.cbuffers_filled.push(format!("{} {} slot {index}: no source, not made", mat.name, b.name));
                        continue;
                    };
                    self.cbuffers_filled.push(format!("{} {} slot {index}: {how}", mat.name, b.name));
                    mat.cbuffers.push((index, b.name.clone(), data));
                }
            }
        }
    }

    /// The placed effects (`crate::fx`), their sprite materials made as
    /// any other forward material (technique slot 6).
    fn read_effects(&mut self, zones: &mut Zones, dir: &Path, by_name: &mut HashMap<String, Option<usize>>) {
        let mut names: Vec<String> = Vec::new();
        let mut fx = crate::fx::FxScene::build(&self.map, &mut |n| {
            Some(names.iter().position(|m| m == n).unwrap_or_else(|| {
                names.push(n.to_owned());
                names.len() - 1
            }))
        });
        let mut resolved = Vec::with_capacity(names.len());
        let mut failed = Vec::new();
        for n in &names {
            let m = self.material(zones, dir, by_name, n).filter(|&i| self.materials[i].forward.is_some());
            if m.is_none() {
                failed.push(n.clone());
            }
            resolved.push(m);
        }
        for d in &mut fx.defs {
            for e in &mut d.elems {
                for m in &mut e.materials {
                    *m = m.and_then(|i| resolved[i]);
                }
            }
        }
        if !failed.is_empty() {
            fx.report.push(format!("t7 fx: {} sprite materials without a forward technique here (not drawn): {:?}", failed.len(), &failed[..failed.len().min(8)]));
        }
        // (Counted again now the materials are resolved.)
        let (mut sprites, mut ok) = (0, 0);
        for e in fx.defs.iter().flat_map(|d| &d.elems) {
            sprites += e.materials.len();
            ok += e.materials.iter().flatten().count();
        }
        fx.report.push(format!("t7 fx: {ok} of {sprites} sprite visuals resolved to {} scene materials", resolved.iter().flatten().collect::<std::collections::HashSet<_>>().len()));
        self.report.extend(fx.report.drain(..));
        // The model elements' XModels: a pool of instances each, placed per
        // frame (unverified on the GPU: `BO2MP_T7_FX_MODELS=1`).
        if crate::fx::models_on() && crate::forward::fx_draw::enabled() && !fx.model_names.is_empty() {
            let mut built = Vec::new();
            let mut failed = Vec::new();
            for (mi, name) in fx.model_names.iter().enumerate() {
                match self.build_model(zones, dir, by_name, name, false, None) {
                    Ok(b) => {
                        let first_bone = self.bones.len() as u32;
                        let first_instance = self.instances.len() as u32;
                        for k in 0..FX_MODEL_POOL {
                            self.bones.push([0.0; 16]);
                            self.instances.push([0, 0, 0, first_bone + k, 0, 0]);
                        }
                        let source = self.model_names.len() as u32;
                        self.model_names.push(name.clone());
                        // (Bounds all zero: always in view for the audits.)
                        for &(first_index, index_count, base_vertex, material) in &b.surfaces {
                            let Some(material) = material else { continue };
                            self.draws.push(Draw { material, group: 1, first_index, index_count, base_vertex, instance: first_instance, instances: FX_MODEL_POOL, mins: [0.0; 3], maxs: [0.0; 3], list: 7, source, range: 255, flags: 0, cull: NO_CULL, lod: ANY_LOD });
                        }
                        self.fx_models.push((mi, first_bone, FX_MODEL_POOL));
                        built.push(name.as_str());
                    }
                    Err(e) => failed.push(format!("{name}: {e}")),
                }
            }
            self.report.push(format!("t7 fx: models: {} of {} XModels built as pools of {FX_MODEL_POOL} instances ({}){}", built.len(), fx.model_names.len(), built.join(", "), if failed.is_empty() { String::new() } else { format!("; not built: {}", failed.join("; ")) }));
        }
        self.fx = fx;
    }

    /// bo2mp: the world's volume decals as the exe's instanced path draws
    /// them (`crate::volume_decals`): the visible ones by material, one
    /// instanced box draw per material (`t7 voldecals:` line).
    fn read_volume_decals(&mut self, zones: &mut Zones, dir: &Path, by_name: &mut HashMap<String, Option<usize>>) {
        use crate::volume_decals as vd;
        // (`BO2MP_T7_VOLUME_DECALS=0`: left out.)
        if std::env::var("BO2MP_T7_VOLUME_DECALS").is_ok_and(|v| v.trim() == "0") {
            return;
        }
        let (g, img) = {
            let (_, z, c) = &zones.list[0];
            let Some(g) = c.world.gfx_worlds.first() else { return };
            (g.clone(), z.clone())
        };
        let Some((at, n)) = g.volume_decals else { return };
        #[allow(clippy::type_complexity)]
        let mut by_material: Vec<(usize, Vec<[f32; vd::ELEMENT_FLOATS]>, [f32; 3], [f32; 3], Vec<[f32; 4]>)> = Vec::new();
        let (mut hidden, mut unmade) = (0, 0);
        for i in 0..n.min(vd::MAX) {
            let Some(rec) = img.get(at + vd::RECORD * i..at + vd::RECORD * (i + 1)) else { break };
            if !vd::visible(rec) {
                hidden += 1;
                continue;
            }
            let Some(m) = g.volume_decal_materials.get(i).cloned().flatten().and_then(|name| self.material(zones, dir, by_name, &name)) else {
                unmade += 1;
                continue;
            };
            let (c, r) = (vd::centre(rec), vd::radius(rec));
            let (lo, hi) = ([c[0] - r, c[1] - r, c[2] - r], [c[0] + r, c[1] + r, c[2] + r]);
            match by_material.iter_mut().find(|e| e.0 == m) {
                Some(e) => {
                    e.1.push(vd::pack(rec));
                    e.4.push(cull_sphere(lo, hi));
                    for k in 0..3 {
                        e.2[k] = e.2[k].min(lo[k]);
                        e.3[k] = e.3[k].max(hi[k]);
                    }
                }
                None => by_material.push((m, vec![vd::pack(rec)], lo, hi, vec![cull_sphere(lo, hi)])),
            }
        }
        if by_material.is_empty() {
            self.report.push(format!("t7 voldecals: {n} in the world, none drawn ({hidden} hidden, {unmade} without a material)"));
            return;
        }
        // The box: in the model streams (positions 12, attributes 16 zero,
        // weights 12 zero), wound as the models are.
        let (corners, idx) = vd::box_mesh();
        let base_vertex = (self.models.positions.len() / 12) as i32;
        for c in corners {
            for x in c {
                self.models.positions.extend_from_slice(&x.to_le_bytes());
            }
            self.models.attributes.extend_from_slice(&[0u8; 16]);
            self.models.weights.extend_from_slice(&[0u8; 12]);
        }
        let first_index = self.models.indices.len() as u32;
        self.models.indices.extend_from_slice(&idx);
        let mut drawn = 0;
        for (m, elements, mins, maxs, spheres) in by_material {
            let instance = self.volume_decals.len() as u32;
            let count = elements.len() as u32;
            let cull = self.cull_ranges.len() as u32;
            self.cull_ranges.push(CullRange { first: instance, spheres, lod: None });
            self.volume_decals.extend(elements);
            drawn += count;
            self.draws.push(Draw { material: m, group: 1, first_index, index_count: 36, base_vertex, instance, instances: count, mins, maxs, list: 0, source: u32::MAX - 1, range: 255, flags: 0, cull, lod: ANY_LOD });
        }
        // (The instance index stream is 0..instances: room for every decal.)
        while self.instances.len() < self.volume_decals.len() {
            self.instances.push([0; 6]);
        }
        self.report.push(format!("t7 voldecals: {drawn} of {n} drawn ({} materials; {hidden} hidden, {unmade} without a material)", self.draws.iter().filter(|d| d.source == u32::MAX - 1).count()));
    }

    fn read_world(&mut self, zones: &mut Zones, dir: &Path, by_name: &mut HashMap<String, Option<usize>>) -> Result<(), String> {
        use fastfile_t7::walk::gfxworld::SURFACE;
        let (g, img) = {
            let (_, z, c) = &zones.list[0];
            (c.world.gfx_worlds.first().ok_or("no GfxWorld in the zone")?.clone(), z.clone())
        };
        let (Some((pos, pos_len)), Some((att, att_len))) = (g.positions, g.attributes) else {
            return Err("GfxWorld: no vertex data in the zone".into());
        };
        self.world.positions = img[pos..pos + pos_len].to_vec();
        self.world.attributes = img[att..att + att_len].to_vec();
        if let Some((at, n)) = g.indices {
            self.world.indices = (0..n).map(|i| u16::from_le_bytes([img[at + 2 * i], img[at + 2 * i + 1]])).collect();
        }
        let Some((at, n)) = g.surfaces else { return Ok(()) };
        // The world's surface ranges: u32 (begin, end) pairs at +0x304,
        // +0x30c, +0x314, +0x31c (the dpvs block at +0x2f8: its +4 is the
        // static model count at +0x2fc and its +0x138 the surfaces at
        // +0x430, as `141d703b0` reads them). `141d70780` collects the main
        // view's lists 0/7/0x17/0x1a from A and C and lists 0x1c/0xa/0xf
        // from B and D.
        let h = g.header_at;
        let rd = |o: usize| img.get(h + o..h + o + 4).map(|b| u32::from_le_bytes(b.try_into().unwrap()));
        let ranges: Option<[(u32, u32); 4]> = (|| {
            let mut r = [(0u32, 0u32); 4];
            for (k, o) in [0x304usize, 0x30c, 0x314, 0x31c].into_iter().enumerate() {
                r[k] = (rd(o)?, rd(o + 4)?);
                if r[k].0 > r[k].1 || r[k].1 as usize > n {
                    return None;
                }
            }
            Some(r)
        })();
        self.world_ranges = ranges;
        let range_of = |i: usize| -> u8 { ranges.and_then(|r| r.iter().position(|&(b, e)| (b as usize..e as usize).contains(&i))).map_or(4, |k| k as u8) };
        let sw = crate::surf_debug::get();
        let mut skipped = 0;
        let mut left_out = 0;
        // Per range: surfaces, and their materials (name -> count).
        let mut per_range: [(usize, HashMap<String, usize>); 5] = Default::default();
        // The brush models' surfaces (`*<n>`, after the world's): their
        // vertices are their own, about their entity's origin. Moved there
        // when a drawn entity names them, else not drawn
        // (`fastfile_t7::map::brush_places`: Metro's river, the
        // `wave_water` / `wave_clamp` boxes, drawn at the world's origin was
        // a flat water slab over the station plaza). `CODLEGACY_T7_BRUSH_AT_ORIGIN=1`:
        // the old way. (Rotated ones: positions turned, packed normals not.)
        let old_brush = codlegacy_env::var("CODLEGACY_T7_BRUSH_AT_ORIGIN").is_ok_and(|v| v != "0");
        let places = if old_brush {
            Vec::new()
        } else {
            fastfile_t7::map::brush_places(
                &self.map.entities,
                &self.map.brush_models,
                n,
                self.name.starts_with("mp_"),
            )
        };
        let mut brush_hidden = 0usize;
        {
            let mut moved = std::collections::HashSet::new();
            for (i, p) in places.iter().enumerate() {
                let fastfile_t7::map::BrushPlace::Placed { origin, axis, .. } = p else { continue };
                let r = at + SURFACE * i;
                let u32_at = |o: usize| u32::from_le_bytes(img[r + o..r + o + 4].try_into().unwrap());
                let first = (u32_at(12) / 12) as usize;
                let tri_count = u16::from_le_bytes([img[r + 42], img[r + 43]]) as usize;
                let base_index = u32_at(44) as usize;
                let top = self.world.indices.get(base_index..(base_index + 3 * tri_count).min(self.world.indices.len())).and_then(|t| t.iter().max().copied()).map_or(0, |m| m as usize + 1);
                for vi in first..first + top {
                    if !moved.insert(vi) {
                        continue;
                    }
                    let Some(b) = self.world.positions.get_mut(12 * vi..12 * vi + 12) else { break };
                    let f = |k: usize| f32::from_le_bytes(b[4 * k..4 * k + 4].try_into().unwrap());
                    let q = fastfile_t7::map::brush_point([f(0), f(1), f(2)], *origin, axis);
                    for (k, c) in q.iter().enumerate() {
                        b[4 * k..4 * k + 4].copy_from_slice(&c.to_le_bytes());
                    }
                }
            }
        }
        for i in 0..n {
            zones.go_on()?;
            let r = at + SURFACE * i;
            let u32_at = |o: usize| u32::from_le_bytes(img[r + o..r + o + 4].try_into().unwrap());
            let first = u32_at(12) / 12;
            let tri_count = u16::from_le_bytes([img[r + 42], img[r + 43]]);
            let base_index = u32_at(44);
            let range = range_of(i);
            if matches!(places.get(i), Some(fastfile_t7::map::BrushPlace::Hidden(_))) {
                brush_hidden += 1;
                continue;
            }
            let mname = g.surface_materials.get(i).cloned().flatten();
            per_range[range as usize].0 += 1;
            *per_range[range as usize].1.entry(mname.as_deref().map(bare).unwrap_or("(none)").to_owned()).or_default() += 1;
            let Some(mname) = mname else {
                skipped += 1;
                continue;
            };
            if sw.no_world || sw.ranges.is_some_and(|(keep, outside)| if range < 4 { !keep[range as usize] } else { !outside }) {
                left_out += 1;
                continue;
            }
            let Some(material) = self.material(zones, dir, by_name, &mname) else {
                skipped += 1;
                continue;
            };
            // The exe's world surface buckets (`141d703b0`, flags byte
            // +0x50): 0x20 -> allocation list 0x17 (view list 9), 0x10 -> 7
            // (view list 2, the G-buffer), 2 -> 0x1a (view list 10), none ->
            // 0 (view list 7); allocation lists merge into view lists in
            // `141de0f60`.
            let flags = img[r + 0x50];
            let list = if flags & 0x20 != 0 {
                9
            } else if flags & 0x10 != 0 {
                2
            } else if flags & 2 != 0 {
                10
            } else {
                7
            };
            self.draws.push(Draw {
                list,
                material,
                group: 3,
                first_index: base_index,
                index_count: 3 * u32::from(tri_count),
                base_vertex: first as i32,
                instance: 0,
                instances: 1,
                mins: v3(&img, r + 48),
                maxs: v3(&img, r + 60),
                source: i as u32,
                range,
                flags,
                cull: NO_CULL,
                lod: ANY_LOD,
            });
            // (A placed brush model's bounds: its own, moved.)
            if let Some(fastfile_t7::map::BrushPlace::Placed { origin, axis, .. }) = places.get(i)
                && let Some(d) = self.draws.last_mut()
            {
                let (a, b) = (d.mins, d.maxs);
                let (mut lo, mut hi) = ([f32::MAX; 3], [f32::MIN; 3]);
                for c in 0..8 {
                    let p = fastfile_t7::map::brush_point([if c & 1 == 0 { a[0] } else { b[0] }, if c & 2 == 0 { a[1] } else { b[1] }, if c & 4 == 0 { a[2] } else { b[2] }], *origin, axis);
                    for k in 0..3 {
                        lo[k] = lo[k].min(p[k]);
                        hi[k] = hi[k].max(p[k]);
                    }
                }
                (d.mins, d.maxs) = (lo, hi);
            }
        }
        if brush_hidden > 0 {
            self.not_drawn.push(("brush model surface no drawn entity names".into(), brush_hidden));
        }
        let placed = places.iter().filter(|p| matches!(p, fastfile_t7::map::BrushPlace::Placed { .. })).count();
        if placed + brush_hidden > 0 {
            self.report.push(format!("t7 brush models: {placed} surfaces moved to their entities, {brush_hidden} not drawn (no drawn entity names them)"));
        }
        self.report.push(format!("world: {} surfaces, {} drawn ({} without a material, {} left out by CODLEGACY_T7_DEBUG_SURF)", n, n - skipped - left_out - brush_hidden, skipped, left_out));
        if skipped > 0 {
            self.not_drawn.push(("world surface without a material (or its material not in the zone)".into(), skipped));
        }
        if left_out > 0 {
            self.not_drawn.push((
                "world surface left out by CODLEGACY_T7_DEBUG_SURF (ranges / no_world)".into(),
                left_out,
            ));
        }
        // Which materials each range holds (the most common first), to tell
        // what B, D and the surfaces in no range are.
        let mut line = match ranges {
            Some(r) => format!("world ranges (GfxWorld +0x304..+0x320; the main view's lists take A and C): A {:?}, B {:?}, C {:?}, D {:?}", r[0], r[1], r[2], r[3]),
            None => format!("world ranges: not read sanely at +0x304..+0x320 ({:?})", [0x304usize, 0x308, 0x30c, 0x310, 0x314, 0x318, 0x31c, 0x320].map(rd)),
        };
        for (k, (count, mats)) in per_range.iter().enumerate() {
            if *count == 0 {
                continue;
            }
            let mut m: Vec<_> = mats.iter().collect();
            m.sort_by(|a, b| b.1.cmp(a.1).then(a.0.cmp(b.0)));
            let top: Vec<String> = m.iter().take(8).map(|(n, c)| format!("{n} x{c}")).collect();
            line.push_str(&format!("; {} {count} surfaces: {}", ["A", "B", "C", "D", "in none"][k], top.join(", ")));
        }
        self.report.push(line);
        Ok(())
    }

    /// bo2mp: the auto-played siege animations started on their static
    /// models (`crate::siege`).
    fn read_siege(&mut self, zones: &Zones) {
        let (_, img, cap) = &zones.list[0];
        let paks = self.map.paks();
        let mut read = |key: u64| paks.and_then(|p| p.read(key).ok());
        let mut report = Vec::new();
        let s = crate::siege::Siege::build(img, cap, &self.instance_smodels, &mut self.bones, &mut self.instances, &mut read, &mut report);
        self.report.extend(report);
        self.siege = std::sync::Mutex::new(s);
    }

    fn read_models(&mut self, zones: &mut Zones, dir: &Path, by_name: &mut HashMap<String, Option<usize>>) {
        use fastfile_t7::walk::gfxworld::SMODEL_DRAW;
        let (g, img) = {
            let (_, z, c) = &zones.list[0];
            let Some(g) = c.world.gfx_worlds.first() else { return };
            (g.clone(), z.clone())
        };
        let Some((at, n)) = g.smodels else { return };
        let sw = crate::surf_debug::get();
        // Each model's finest LOD once: per surface (first index, count,
        // base vertex, material), and the mesh's bounds.
        let mut built: HashMap<String, Option<Built>> = HashMap::new();
        let mut failures = Vec::new();
        let mut placed = 0;
        // The placements of each model, in the order the models first
        // appear: their instance records are consecutive, so a model's
        // surface is one draw of all its instances.
        let mut order: Vec<String> = Vec::new();
        let mut placements: HashMap<String, Vec<([f32; 16], [f32; 3], u8, u32)>> = HashMap::new();
        // The draw records' +0x60 bit 0 / +0x70 (the shadow traversals skip
        // those): placements and model names.
        let mut flagged = 0usize;
        let mut flagged_names: HashMap<String, usize> = HashMap::new();
        let mut left_out = 0usize;
        let mut unbuilt_placements = 0usize;
        let mut errors: HashMap<String, String> = HashMap::new();
        for i in 0..n {
            if zones.given_up() {
                return;
            }
            let d = at + SMODEL_DRAW * i;
            let Some(name) = g.smodel_models.get(i).cloned().flatten() else { continue };
            let name = bare(&name).to_owned();
            let f60 = img.get(d + 0x60).copied().unwrap_or(0);
            let f70 = img.get(d + 0x70).copied().unwrap_or(0);
            let shadow_skip = f60 & 1 != 0 || f70 != 0;
            if shadow_skip {
                flagged += 1;
                *flagged_names.entry(format!("{name} (+0x60 {f60:#04x}, +0x70 {f70:#04x})")).or_default() += 1;
            }
            if sw.no_models || (sw.smodel_flags && shadow_skip) {
                left_out += 1;
                continue;
            }
            if !built.contains_key(&name) {
                let r = self.build_model(zones, dir, by_name, &name, false, None);
                if let Err(e) = &r {
                    errors.insert(name.clone(), e.clone());
                    if failures.len() < 12 {
                        failures.push(format!("{name}: {e}"));
                    }
                }
                built.insert(name.clone(), r.ok());
            }
            let Some(Some(_)) = built.get(&name) else {
                unbuilt_placements += 1;
                self.unbuilt.push((name.clone(), v3(&img, d), errors.get(&name).cloned().unwrap_or_default()));
                continue;
            };
            let origin = v3(&img, d);
            let axis = [v3(&img, d + 12), v3(&img, d + 24), v3(&img, d + 36)];
            let scale = f32_at(&img, d + 48);
            // Rows: world = rotation * model + translation, the axes scaled.
            let mut m = [0.0f32; 16];
            for row in 0..3 {
                for col in 0..3 {
                    m[4 * row + col] = axis[col][row] * scale;
                }
                m[4 * row + 3] = origin[row];
            }
            if !placements.contains_key(&name) {
                order.push(name.clone());
            }
            placements.entry(name).or_default().push((m, origin, f60, i as u32));
            placed += 1;
        }
        // The exe's LOD selection (`crate::lod`, `BO2MP_T7_LOD=exe`, the
        // medium and low presets): every LOD of a model with more than one
        // is built, and its header's LOD numbers read.
        let lod_mode = crate::lod::mode();
        let mut coarse: HashMap<String, (crate::lod::ModelLods, Vec<Built>)> = HashMap::new();
        let (mut lods_built, mut lods_failed) = (0usize, 0usize);
        if lod_mode == crate::lod::Mode::Exe {
            for name in &order {
                let Some(Some(b)) = built.get(name) else { continue };
                if b.lods < 2 {
                    continue;
                }
                let (finest, count) = (b.lod, b.lods);
                let header = zones.list[0].2.model(name).or_else(|| zones.list[0].2.model(&format!(",{name}"))).map(|m| m.header_at);
                let Some(m) = header.and_then(|h| crate::lod::ModelLods::read(&img, h)).filter(|m| usize::from(m.count) == count) else { continue };
                let mut others = Vec::new();
                for k in (0..count).filter(|&k| k != finest) {
                    match self.build_model(zones, dir, by_name, name, false, Some(k)) {
                        Ok(o) => {
                            lods_built += 1;
                            others.push(o);
                        }
                        Err(_) => lods_failed += 1,
                    }
                }
                coarse.insert(name.clone(), (m, others));
            }
        }
        let mut draws_before = 0;
        let mut no_material = 0;
        for name in &order {
            let Some(Some(b)) = built.get(name) else { continue };
            let list = &placements[name];
            let first_instance = self.instances.len() as u32;
            // The draw's bounds: its largest placement's (for the log).
            let mut big = ([0.0f32; 3], [0.0f32; 3]);
            let mut big_d = -1.0f32;
            // (Each placement's sphere for the frustum culling, a little
            // larger: siege animations sway parts past the rest pose.)
            let mut spheres = Vec::with_capacity(list.len());
            for (m, _, _, j) in list {
                self.bones.push(*m);
                let bone = (self.bones.len() - 1) as u32;
                self.instances.push([0, 0, 0, bone, 0, 0]);
                self.instance_smodels.push(((self.instances.len() - 1) as u32, *j));
                let (lo, hi) = transform_bounds(m, b.mins, b.maxs);
                let dd: f32 = (0..3).map(|k| (hi[k] - lo[k]).powi(2)).sum();
                spheres.push(cull_sphere(lo, hi));
                if dd > big_d {
                    big_d = dd;
                    big = (lo, hi);
                }
            }
            let source = self.model_names.len() as u32;
            self.model_names.push(name.clone());
            let cull = self.cull_ranges.len() as u32;
            let lod = coarse.get(name).map(|(m, others)| crate::lod::RangeLod {
                model: *m,
                built: others.iter().fold(1u32 << b.lod, |a, o| a | (1 << o.lod)),
                finest: b.lod as u8,
                placements: list.iter().map(|(_, _, _, j)| crate::lod::Placement::read(&img, at + SMODEL_DRAW * *j as usize)).collect(),
            });
            let split = lod.is_some();
            self.cull_ranges.push(CullRange { first: first_instance, spheres, lod });
            draws_before += list.len() * b.surfaces.iter().filter(|s| s.3.is_some()).count();
            no_material += list.len() * b.surfaces.iter().filter(|s| s.3.is_none()).count();
            // A forward-only surface (no G-buffer technique) keeps the
            // finest LOD over every placement in view: the forward lists
            // draw their own instances.
            let materials = &self.materials;
            let forward_only = |m: usize| materials[m].technique.is_none();
            let mut push = Vec::new();
            for &(first_index, index_count, base_vertex, material) in &b.surfaces {
                let Some(material) = material else { continue };
                let lod = if split && !forward_only(material) { b.lod as u8 } else { ANY_LOD };
                push.push(Draw { material, group: 1, first_index, index_count, base_vertex, instance: first_instance, instances: list.len() as u32, mins: big.0, maxs: big.1, list: 7, source, range: 255, flags: list[0].2, cull, lod });
            }
            if split {
                for o in &coarse[name].1 {
                    for &(first_index, index_count, base_vertex, material) in &o.surfaces {
                        let Some(material) = material else { continue };
                        if forward_only(material) {
                            continue;
                        }
                        push.push(Draw { material, group: 1, first_index, index_count, base_vertex, instance: first_instance, instances: list.len() as u32, mins: big.0, maxs: big.1, list: 7, source, range: 255, flags: list[0].2, cull, lod: o.lod as u8 });
                    }
                }
                // (A material's draws of every LOD side by side: no
                // pipeline or binding switch between them.)
                push.sort_by_key(|d| d.material);
            }
            self.draws.extend(push);
        }
        if lod_mode == crate::lod::Mode::Exe {
            self.report.push(format!(
                "static model LODs: the exe's selection (BO2MP_T7_LOD=finest: the finest everywhere): {} models with LODs, {lods_built} coarser LODs built, {lods_failed} not built",
                coarse.len()
            ));
        }
        if no_material > 0 {
            self.not_drawn.push(("static model surface without a material (or its material not in the zone; per placement)".into(), no_material));
        }
        if left_out > 0 {
            self.not_drawn.push(("static model placement left out by CODLEGACY_T7_DEBUG_SURF (smodel_flags / no_models)".into(), left_out));
        }
        let failed = built.values().filter(|b| b.is_none()).count();
        if failed > 0 {
            self.not_drawn.push((format!("static model placement whose mesh could not be built ({failed} models)"), unbuilt_placements));
        }
        self.report.push(format!(
            "static models: {placed} placed of {n} ({draws_before} draws as {} instanced draws), {} models built of {}{}",
            self.draws.iter().filter(|d| d.group == 1).count(),
            built.values().filter(|b| b.is_some()).count(),
            built.len(),
            if failures.is_empty() { String::new() } else { format!("; not built: {}", failures.join("; ")) }
        ));
        let mut fl: Vec<_> = flagged_names.into_iter().collect();
        fl.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
        let fl: Vec<String> = fl.iter().take(16).map(|(n, c)| format!("{n} x{c}")).collect();
        self.report.push(format!(
            "static models with +0x60 bit 0 or +0x70 set (the exe's shadow traversals skip them; drawn here: {}): {flagged}{}{}",
            if sw.smodel_flags { "no, smodel_flags" } else { "yes" },
            if fl.is_empty() { "" } else { ": " },
            fl.join(", ")
        ));
    }

    /// Shadow-only geometry (technique set or material `shadowcaster`,
    /// `*_onlycast`) is left out of the view: the exe's main view lists
    /// never hold it (its world surfaces sit after every surface range the
    /// view walks), only the shadow passes, which render_t7 does not draw
    /// from it. Drawn, its blocker boxes and slabs (Nuk3town's houses' and
    /// roofs' stand-ins, a fifth of the spawn view) wrote depth over the
    /// real walls and floors: the walls and ground that showed black or not
    /// at all. `CODLEGACY_T7_DEBUG_SURF=shadow_casters` draws them (the old way).
    fn leave_out_shadow_casters(&mut self) {
        if crate::surf_debug::get().shadow_casters {
            return;
        }
        let before = self.draws.len();
        let materials = &self.materials;
        self.draws.retain(|d| !is_shadow_caster(&materials[d.material].name, &materials[d.material].techset));
        let n = before - self.draws.len();
        if n > 0 {
            self.report.push(format!("shadow casters: {n} draws of shadow-only geometry left out of the view"));
            self.not_drawn.push(("shadow-only geometry (shadowcaster; the exe draws it only into shadows)".into(), n));
        }
    }

    /// `CODLEGACY_T7_DEBUG_SURF` `hide=` / `only=`: the draws they name are left
    /// out (counted).
    fn apply_debug_switches(&mut self) {
        let sw = crate::surf_debug::get();
        if !sw.any() {
            return;
        }
        self.report
            .push(format!("CODLEGACY_T7_DEBUG_SURF={} ({:?})", sw.raw, sw));
        if sw.hide.is_empty() && sw.only.is_empty() {
            return;
        }
        let before = self.draws.len();
        let (materials, models) = (&self.materials, &self.model_names);
        self.draws.retain(|d| {
            let m = &materials[d.material];
            let model = if d.group == 1 { models.get(d.source as usize).map(String::as_str).unwrap_or("") } else { "" };
            !sw.hidden(&[&m.name, &m.techset, model])
        });
        let n = before - self.draws.len();
        self.report.push(format!(
            "CODLEGACY_T7_DEBUG_SURF: {n} draws hidden by hide= / only="
        ));
        if n > 0 {
            self.not_drawn.push((
                "draw hidden by CODLEGACY_T7_DEBUG_SURF hide= / only=".into(),
                n,
            ));
        }
    }

    /// The sky box models of the first sun volume, drawn with their
    /// materials (sky techniques have no G-buffer technique: the forward
    /// lists draw them). The sky program turns each vertex into a direction
    /// `normalize(world position - eyeOffset)` and ignores the instance's
    /// placement matrix; it skins with instance word 0 as first bone. So
    /// the instance names a bone whose matrix is a translation to the eye
    /// (written every frame by `GBuffer::update`), and a box without skin
    /// data gets rigid weights: unskinned at the world origin, a view far
    /// from it (Skyjacked: kilometres) saw every vertex in nearly one
    /// direction and the sky covered a few pixels.
    fn read_sky(&mut self, zones: &mut Zones, dir: &Path, by_name: &mut HashMap<String, Option<usize>>) {
        // The sky material (also when its models are not built: checks).
        if let Some(m) = self.map.light_refs().map(|l| l.sky_material.clone()).filter(|m| !m.is_empty()) {
            self.material(zones, dir, by_name, &m);
        }
        // The sun volume's first sky box (lighting state 0, as the lighting
        // stage takes): the others belong to other states.
        if let Some(name) = self.map.sky_models.first().cloned() {
            match self.build_model(zones, dir, by_name, &name, true, None) {
                Ok(Built { surfaces, .. }) => {
                    let mut m = [0.0f32; 16];
                    (m[0], m[5], m[10], m[15]) = (1.0, 1.0, 1.0, 1.0);
                    self.bones.push(m);
                    self.sky_bone = Some(self.bones.len() - 1);
                    let bone = (self.bones.len() - 1) as u32;
                    self.instances.push([bone, 0, 0, bone, 0, 0]);
                    let instance = (self.instances.len() - 1) as u32;
                    let n = surfaces.iter().filter(|s| s.3.is_some()).count();
                    // The sky program turns every vertex into a direction, so
                    // only a sky-technique surface belongs in this draw. A sky
                    // model that also holds ordinary (lit/unlit) surfaces would
                    // have them placed by the eye-translation bone at their
                    // model-space scale: giant stretched slabs and black
                    // polygon voids across the sky (mp_city / Evac). Kept
                    // whole when no surface has a sky techset (the old way);
                    // `T7_SKY_ALL_SURFACES=1`: always whole.
                    let is_sky = |m: usize| self.materials[m].techset.contains("sky");
                    let only_sky = std::env::var("T7_SKY_ALL_SURFACES").is_err() && surfaces.iter().any(|s| s.3.is_some_and(is_sky));
                    let mut skipped = Vec::new();
                    for (first_index, index_count, base_vertex, material) in surfaces {
                        let Some(material) = material else { continue };
                        if only_sky && !is_sky(material) {
                            skipped.push(self.materials[material].techset.clone());
                            continue;
                        }
                        self.draws.push(Draw { material, group: 1, first_index, index_count, base_vertex, instance, instances: 1, mins: [0.0; 3], maxs: [0.0; 3], list: 7, source: u32::MAX, range: 255, flags: 0, cull: NO_CULL, lod: ANY_LOD });
                    }
                    self.report.push(format!("sky model {name}: {n} surfaces"));
                    if !skipped.is_empty() {
                        self.report.push(format!("sky model {name}: {} non-sky surfaces left out (techsets {skipped:?})", skipped.len()));
                    }
                }
                Err(e) => self.report.push(format!("sky not drawn: sky model {name}'s mesh not read: {e} (its mesh streams from the base packages, which must be beside the map zone)")),
            }
        }
    }

    fn build_model(&mut self, zones: &mut Zones, dir: &Path, by_name: &mut HashMap<String, Option<usize>>, name: &str, rigid_skin: bool, only: Option<usize>) -> Result<Built, String> {
        zones.go_on()?;
        // The model's LODs and meshes in a zone, and whether any mesh has
        // data of its own.
        let read = |cap: &Capture| model_parts(cap, name);
        let (lods, meshes, has) = read(&zones.list[0].2).ok_or("not in the zone")?;
        // A model the map zone holds only as a reference (its meshes without
        // data: Skyjacked's robot head, weapon attachments, glass shards):
        // the shared zone that has it whole, its data from that zone or the
        // packages.
        let (zi, lods, meshes) = if has || crate::surf_debug::get().no_shared { (0, lods, meshes) } else { zones.model_def(name, &read).unwrap_or((0, lods, meshes)) };
        // The finest LOD with data (the exe picks a LOD by distance; the
        // finest is drawn here).
        let mut best: Option<(usize, Vec<u8>)> = None;
        let mut best_tris = 0;
        for (k, me) in meshes.iter().enumerate() {
            let Some(me) = me else { continue };
            // (One LOD asked for: that mesh only.)
            if only.is_some_and(|o| o != k) {
                continue;
            }
            let tris: usize = me.surfaces.iter().map(|s| s.tri_count as usize).sum();
            if tris <= best_tris {
                continue;
            }
            let data = match (me.data, me.xpak) {
                (Some((at, n)), _) => zones.list[zi].1.get(at..at + n).map(<[u8]>::to_vec),
                (None, Some(e)) => {
                    let t_read = std::time::Instant::now();
                    let d = self.map.paks().and_then(|p| p.read(e.key).ok()).or_else(|| synthetic_mesh(name, me));
                    MESH_READ_NS.fetch_add(t_read.elapsed().as_nanos() as u64, std::sync::atomic::Ordering::Relaxed);
                    d
                }
                _ => None,
            };
            if let Some(data) = data {
                best = Some((k, data));
                best_tris = tris;
            }
        }
        let (k, data) = best.ok_or_else(|| {
            let why: Vec<String> = meshes
                .iter()
                .map(|me| match me {
                    None => "mesh not in the zone".into(),
                    Some(me) => match (me.data, me.xpak) {
                        (Some(_), _) => "zone data out of range".into(),
                        (None, Some(e)) => match self.map.paks().map(|p| p.read(e.key)) {
                            Some(Err(err)) => format!("package entry {:#018x}: {err}", e.key),
                            None => "no packages".into(),
                            Some(Ok(_)) => "empty".into(),
                        },
                        _ => "no data".into(),
                    },
                })
                .collect();
            format!("no LOD mesh with data ({})", why.join(", "))
        })?;
        let me = meshes[k].clone().unwrap();
        let nv = me.vert_count as usize;
        let [pos, verts, idx, skin] = me.offsets.map(|o| o as usize);
        let get = |at: usize, len: usize| data.get(at..at + len).ok_or_else(|| format!("mesh data past the end at {at:#x}"));
        let base_vertex = (self.models.positions.len() / 12) as i32;
        let pbytes = get(pos, 12 * nv)?;
        let (mut mins, mut maxs) = ([f32::MAX; 3], [f32::MIN; 3]);
        for v in pbytes.chunks_exact(12) {
            for k in 0..3 {
                let x = f32::from_le_bytes(v[4 * k..4 * k + 4].try_into().unwrap());
                if x.is_finite() {
                    mins[k] = mins[k].min(x);
                    maxs[k] = maxs[k].max(x);
                }
            }
        }
        if mins[0] > maxs[0] {
            (mins, maxs) = ([0.0; 3], [0.0; 3]);
        }
        // A sky box's vertices all on one sphere. The sky program turns each
        // vertex into a direction and hands the pixel program that vector
        // interpolated over the triangle; on the shared generic sky (a
        // sphere) every vertex is the same distance out, so the interpolation
        // runs along the pixel's own ray. A map's own low-poly dome
        // (mp_city's `skybox_dlc4_mp_city_override`: 266..786 out, an
        // ellipsoid) mixes distances inside a triangle, so the interpolated
        // vector points elsewhere than the pixel does: straight-edged
        // polygons looked up in the wrong direction (black ground, stretched
        // slabs). Each vertex keeps its direction and takes the farthest
        // vertex's distance. `T7_SKY_RAW_POSITIONS=1`: the mesh as authored.
        // The sky mesh's own diagnostics, and a clean replacement dome (see
        // `sky_dome_report` / `sky_dome`): the authored override dome is
        // an open, low-poly mesh whose winding, coverage and vertices were
        // never checked; the sky program only needs directions.
        let mut sky_replacement: Option<(Vec<[f32; 3]>, Vec<u16>, Vec<u8>)> = None;
        if rigid_skin {
            let p: Vec<[f32; 3]> = pbytes.chunks_exact(12).map(|v| [0, 4, 8].map(|k| f32::from_le_bytes(v[k..k + 4].try_into().unwrap()))).collect();
            let ix: Vec<u16> = get(idx, 2 * me.index_count as usize)?.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
            let at = get(verts, 16 * nv)?;
            let (line, majority) = sky_dome_report(&p, &ix, &me.surfaces);
            self.report.push(format!("sky mesh {name}: {line}"));
            if std::env::var("T7_SKY_AUTHORED_MESH").is_ok_and(|v| v != "0") {
                self.report.push(format!("sky mesh {name}: authored triangles kept (T7_SKY_AUTHORED_MESH=1)"));
            } else if me.surfaces.len() == 1 {
                // Each generated vertex takes the 16 attribute bytes of the
                // authored vertex nearest in direction.
                let dome = sky_dome(&p, at, majority);
                self.report.push(format!("sky mesh {name}: replaced by a generated dome of {} vertices, {} triangles (T7_SKY_AUTHORED_MESH=1: the authored one)", dome.0.len(), dome.1.len() / 3));
                sky_replacement = Some(dome);
            } else {
                self.report.push(format!("sky mesh {name}: {} surfaces, authored triangles kept (the generated dome replaces a one-surface mesh only)", me.surfaces.len()));
            }
        }
        if let Some((gp, _, _)) = &sky_replacement {
            for v in gp {
                for x in v {
                    self.models.positions.extend_from_slice(&x.to_le_bytes());
                }
            }
        } else if rigid_skin && !std::env::var("T7_SKY_RAW_POSITIONS").is_ok_and(|v| v != "0") {
            let p: Vec<[f32; 3]> = pbytes.chunks_exact(12).map(|v| [0, 4, 8].map(|k| f32::from_le_bytes(v[k..k + 4].try_into().unwrap()))).collect();
            let len = |v: &[f32; 3]| (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
            let radius = p.iter().map(len).filter(|l| l.is_finite()).fold(0f32, f32::max);
            let mut fixed = Vec::with_capacity(pbytes.len());
            let mut moved = 0usize;
            for v in &p {
                let l = len(v);
                let w = if l.is_finite() && l > 1e-3 && radius > 0.0 { v.map(|x| x / l * radius) } else { *v };
                moved += usize::from((l - radius).abs() > 0.01 * radius);
                for x in w {
                    fixed.extend_from_slice(&x.to_le_bytes());
                }
            }
            self.report.push(format!("sky mesh {name}: {moved} of {nv} vertices moved onto one sphere of radius {radius:.1} (T7_SKY_RAW_POSITIONS=1: as authored)"));
            self.models.positions.extend_from_slice(&fixed);
        } else {
            self.models.positions.extend_from_slice(pbytes);
        }
        if let Some((_, _, ga)) = &sky_replacement {
            self.models.attributes.extend_from_slice(ga);
        } else {
            self.models.attributes.extend_from_slice(get(verts, 16 * nv)?);
        }
        let skinned = (me.skin_vert_count as usize).min(nv);
        let nw = sky_replacement.as_ref().map_or(nv, |d| d.0.len());
        let mut weights = vec![0u8; 12 * nw];
        // A sky box's own skin data (a map's own sky model, not the shared
        // generic mesh: mp_city's `skybox_dlc4_mp_city_override`) can weigh
        // vertices on bones other than the first, or blend two; the sky
        // program places every vertex by the instance's one eye-placing
        // bone, so those vertices land under other models' bones: patches
        // of the sky looked up in the wrong direction (black ground half of
        // the picture, banded slabs, straight edges along the mesh's
        // triangles). `T7_SKY_KEEP_SKIN=1`: take the mesh's own weights.
        let keep_skin = !rigid_skin || std::env::var("T7_SKY_KEEP_SKIN").is_ok_and(|v| v != "0");
        if rigid_skin {
            let mut stats = String::new();
            if skinned > 0 {
                if let Ok(sk) = get(skin, 12 * skinned) {
                    let mut idx: std::collections::BTreeMap<i16, usize> = Default::default();
                    let mut blended = 0usize;
                    for v in sk.chunks_exact(12) {
                        if v[1..4].iter().any(|&w| w != 0) {
                            blended += 1;
                        }
                        *idx.entry(i16::from_le_bytes([v[4], v[5]])).or_default() += 1;
                    }
                    stats = format!("; skin data on {skinned} vertices ({blended} blended, first bone indices {:?})", idx.iter().take(6).collect::<Vec<_>>());
                }
            }
            let lens: Vec<f32> = pbytes.chunks_exact(12).map(|v| (0..3).map(|k| f32::from_le_bytes(v[4 * k..4 * k + 4].try_into().unwrap()).powi(2)).sum::<f32>().sqrt()).filter(|l| l.is_finite()).collect();
            let (lo, hi) = lens.iter().fold((f32::MAX, 0f32), |a, &l| (a.0.min(l), a.1.max(l)));
            self.report.push(format!(
                "sky mesh {name}: {nv} vertices, {} triangles, distance from the model origin {lo:.3}..{hi:.3}, bounds {mins:?}..{maxs:?}{stats}; weights {}",
                me.index_count / 3,
                if keep_skin { "as the mesh has them (T7_SKY_KEEP_SKIN)" } else { "all on the eye-placing bone" }
            ));
        }
        if sky_replacement.is_some() {
            for v in weights.chunks_exact_mut(12) {
                v[0] = 255;
            }
        } else if skinned > 0 && keep_skin {
            weights[..12 * skinned].copy_from_slice(get(skin, 12 * skinned)?);
        } else if rigid_skin {
            // A sky box without skin data: every vertex weighs 1 on its
            // instance's first bone (BLENDWEIGHT unorm8x4, BLENDINDICES 4 x
            // i16), the eye-placing matrix (see `read_sky`).
            for v in weights.chunks_exact_mut(12) {
                v[0] = 255;
            }
        }
        self.models.weights.extend_from_slice(&weights);
        let first = self.models.indices.len() as u32;
        let mats = lods[k].materials.clone();
        let mut out = Vec::new();
        if let Some((_, gi, _)) = &sky_replacement {
            self.models.indices.extend_from_slice(gi);
            let material = mats.first().and_then(|m| self.material(zones, dir, by_name, m));
            out.push((first, gi.len() as u32, base_vertex, material));
            return Ok(Built { surfaces: out, mins, maxs, lod: k, lods: lods.len() });
        }
        let ix = get(idx, 2 * me.index_count as usize)?;
        self.models.indices.extend(ix.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])));
        for (si, sf) in me.surfaces.iter().enumerate() {
            let material = mats.get(si).and_then(|m| self.material(zones, dir, by_name, m));
            out.push((first + sf.base_index, 3 * u32::from(sf.tri_count), base_vertex + sf.base_vert as i32, material));
        }
        Ok(Built { surfaces: out, mins, maxs, lod: k, lods: lods.len() })
    }
}

fn v3_norm(v: [f32; 3]) -> [f32; 3] {
    let l = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
    if l > 1e-6 && l.is_finite() { [v[0] / l, v[1] / l, v[2] / l] } else { [0.0; 3] }
}
fn v3_dot(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
fn v3_cross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}

/// What the sky program sees of a sky mesh: every vertex as a unit
/// direction (it normalises `position - eyeOffset`; the distance does not
/// matter). Per triangle the sign of the direction triple product (the
/// winding as seen from the eye) and the solid angle; the sum over the
/// triangles against the whole sphere's 4 pi says whether the mesh covers the
/// sky at all, and the largest triangles whether a few vertices fly off.
/// Returns the report text and the majority winding sign.
fn sky_dome_report(p: &[[f32; 3]], ix: &[u16], surfaces: &[fastfile_t7::walk::capture::SurfaceRec]) -> (String, f32) {
    let d: Vec<[f32; 3]> = p.iter().map(|&v| v3_norm(v)).collect();
    let (mut pos, mut neg, mut degenerate, mut bad_index) = (0usize, 0usize, 0usize, 0usize);
    let (mut total, mut biggest, mut over_half) = (0f64, 0f64, 0usize);
    let mut polar = [0f64; 3];
    for t in ix.chunks_exact(3) {
        let (Some(&a), Some(&b), Some(&c)) = (d.get(t[0] as usize), d.get(t[1] as usize), d.get(t[2] as usize)) else {
            bad_index += 1;
            continue;
        };
        let det = v3_dot(a, v3_cross(b, c));
        let omega = 2.0 * f64::from(det).atan2(f64::from(1.0 + v3_dot(a, b) + v3_dot(b, c) + v3_dot(c, a)));
        if !(omega.abs() > 1e-7) {
            degenerate += 1;
            continue;
        }
        if det > 0.0 { pos += 1 } else { neg += 1 }
        total += omega.abs();
        biggest = biggest.max(omega.abs());
        over_half += usize::from(omega.abs() > 0.5);
        // Where the coverage is: up (z > 0.3), the band round the horizon,
        // down (z < -0.3), by the triangle's centre.
        let z = (a[2] + b[2] + c[2]) / 3.0;
        polar[if z > 0.3 { 0 } else if z < -0.3 { 2 } else { 1 }] += omega.abs();
    }
    // The coverage over the sphere, by 12 sectors of azimuth (the mesh's
    // x, y: a wedge with none is a hole in the sky).
    let mut sector = [0f64; 12];
    for t in ix.chunks_exact(3) {
        let (Some(&a), Some(&b), Some(&c)) = (d.get(t[0] as usize), d.get(t[1] as usize), d.get(t[2] as usize)) else { continue };
        let m = [a[0] + b[0] + c[0], a[1] + b[1] + c[1]];
        let az = m[1].atan2(m[0]).to_degrees().rem_euclid(360.0);
        let omega = 2.0 * f64::from(v3_dot(a, v3_cross(b, c))).atan2(f64::from(1.0 + v3_dot(a, b) + v3_dot(b, c) + v3_dot(c, a)));
        sector[((az / 30.0) as usize).min(11)] += omega.abs();
    }
    let majority = if pos >= neg { 1.0 } else { -1.0 };
    let sec: Vec<String> = sector.iter().map(|s| format!("{:.2}", s / (4.0 * std::f64::consts::PI) * 12.0)).collect();
    let line = format!(
        "{} triangles: {pos} wind one way, {neg} the other, {degenerate} degenerate, {bad_index} past the vertices; solid angle {:.2} of 4 pi (up {:.2}, horizon band {:.2}, down {:.2}); largest triangle {biggest:.2} sr, {over_half} over 0.5 sr; coverage by 30 deg azimuth sector (1.00 = an even share): [{}]; {} surfaces {:?}",
        ix.len() / 3,
        total / (4.0 * std::f64::consts::PI),
        polar[0] / (4.0 * std::f64::consts::PI),
        polar[1] / (4.0 * std::f64::consts::PI),
        polar[2] / (4.0 * std::f64::consts::PI),
        sec.join(" "),
        surfaces.len(),
        surfaces.iter().map(|s| (s.base_index, s.tri_count, s.base_vert)).collect::<Vec<_>>()
    );
    (line, majority)
}

/// A clean sky dome for the sky program (which only uses each vertex's
/// direction): a lat-long sphere of 96 x 48 around the mesh's z axis (the
/// lat-long picture's up), poles single vertices, wound like the authored
/// mesh's majority. Each vertex takes the attribute bytes of the authored
/// vertex nearest in direction. Returns positions, indices and attributes.
fn sky_dome(p: &[[f32; 3]], attrs: &[u8], majority: f32) -> (Vec<[f32; 3]>, Vec<u16>, Vec<u8>) {
    const LON: usize = 96;
    const LAT: usize = 48;
    const R: f32 = 786.0;
    let mut pos = vec![[0.0, 0.0, R]];
    for i in 1..LAT {
        let th = std::f32::consts::PI * i as f32 / LAT as f32;
        for j in 0..LON {
            let ph = std::f32::consts::TAU * j as f32 / LON as f32;
            pos.push([R * th.sin() * ph.cos(), R * th.sin() * ph.sin(), R * th.cos()]);
        }
    }
    pos.push([0.0, 0.0, -R]);
    let ring = |i: usize, j: usize| (1 + (i - 1) * LON + j % LON) as u16;
    let (north, south) = (0u16, (pos.len() - 1) as u16);
    let mut tris: Vec<[u16; 3]> = Vec::new();
    for j in 0..LON {
        tris.push([north, ring(1, j), ring(1, j + 1)]);
        for i in 1..LAT - 1 {
            tris.push([ring(i, j), ring(i + 1, j), ring(i + 1, j + 1)]);
            tris.push([ring(i, j), ring(i + 1, j + 1), ring(i, j + 1)]);
        }
        tris.push([south, ring(LAT - 1, j + 1), ring(LAT - 1, j)]);
    }
    let mut ix = Vec::with_capacity(tris.len() * 3);
    for [a, b, c] in tris {
        let det = v3_dot(v3_norm(pos[a as usize]), v3_cross(v3_norm(pos[b as usize]), v3_norm(pos[c as usize])));
        // (the sign of the triple product of a small outward-facing
        // triangle is fixed by its winding seen from outside)
        if det * majority >= 0.0 { ix.extend([a, b, c]) } else { ix.extend([a, c, b]) }
    }
    let dirs: Vec<[f32; 3]> = p.iter().map(|&v| v3_norm(v)).collect();
    let mut at = Vec::with_capacity(pos.len() * 16);
    for v in &pos {
        let d = v3_norm(*v);
        let best = dirs.iter().enumerate().max_by(|a, b| v3_dot(*a.1, d).total_cmp(&v3_dot(*b.1, d))).map_or(0, |(i, _)| i);
        match attrs.get(16 * best..16 * best + 16) {
            Some(a) => at.extend_from_slice(a),
            None => at.extend_from_slice(&[0u8; 16]),
        }
    }
    (pos, ix, at)
}

/// BC6H blocks whose mode the format reserves (decode to black): by 16
/// columns of the picture, for the report. Mode bits: low two bits 0 or 1
/// are modes 1 and 2; otherwise the low five bits, 0x13 0x17 0x1b 0x1f
/// are reserved.
fn bc6h_invalid_blocks(data: &[u8], width: u32, height: u32) -> String {
    let (bw, bh) = ((width / 4) as usize, (height / 4) as usize);
    if data.len() < bw * bh * 16 || bw == 0 {
        return "not scanned".into();
    }
    let mut bins = [0usize; 16];
    let mut zero = 0usize;
    let mut rows_bad = [0usize; 8];
    for y in 0..bh {
        for x in 0..bw {
            let b = &data[16 * (y * bw + x)..16 * (y * bw + x) + 16];
            let m = b[0];
            let invalid = m & 3 >= 2 && matches!(m & 0x1f, 0x13 | 0x17 | 0x1b | 0x1f);
            if invalid {
                bins[x * 16 / bw] += 1;
                rows_bad[y * 8 / bh] += 1;
            }
            zero += usize::from(b.iter().all(|&v| v == 0));
        }
    }
    let n: usize = bins.iter().sum();
    format!("BC6H blocks with a reserved mode {n} of {} (all-zero blocks {zero}); by 16 columns {bins:?}; by 8 rows {rows_bad:?}", bw * bh)
}

/// Bone matrix 0, the one world surfaces' instance record (all zero)
/// names: the identity (rows rotation | translation), so a vertex program
/// that places every vertex by its instance's bone matrix leaves the world
/// where it is. It was all zero, which collapsed every world vertex onto
/// the origin in the programs that do (`wc/lit_plus`, `wc/lit_advanced`...:
/// the invisible floors, 2026-10-26). `CODLEGACY_T7_BONE0=zero`: the old zeros.
fn bone_zero() -> [f32; 16] {
    let mut m = [0.0f32; 16];
    if codlegacy_env::var("CODLEGACY_T7_BONE0").is_ok_and(|v| v == "zero") {
        return m;
    }
    (m[0], m[5], m[10], m[15]) = (1.0, 1.0, 1.0, 1.0);
    m
}

/// A model's finest LOD as added to the model streams: per surface (first
/// index, index count, base vertex, material), and its mesh's bounds.
struct Built {
    surfaces: Vec<(u32, u32, i32, Option<usize>)>,
    mins: [f32; 3],
    maxs: [f32; 3],
    /// Which of the model's LODs this is (0 the coarsest), of how many.
    lod: usize,
    lods: usize,
}

/// A box's bounding sphere for the frustum culling (centre, radius), a
/// tenth and 16 units larger than the box's own.
pub fn cull_sphere(lo: [f32; 3], hi: [f32; 3]) -> [f32; 4] {
    let c = [(lo[0] + hi[0]) * 0.5, (lo[1] + hi[1]) * 0.5, (lo[2] + hi[2]) * 0.5];
    let r = ((hi[0] - lo[0]).powi(2) + (hi[1] - lo[1]).powi(2) + (hi[2] - lo[2]).powi(2)).sqrt() * 0.5;
    if !r.is_finite() || !c.iter().all(|v| v.is_finite()) {
        return [0.0, 0.0, 0.0, -1.0];
    }
    [c[0], c[1], c[2], r * 1.1 + 16.0]
}

/// The world bounds of a box `lo..hi` placed by `m` (rows: rotation |
/// translation).
fn transform_bounds(m: &[f32; 16], lo: [f32; 3], hi: [f32; 3]) -> ([f32; 3], [f32; 3]) {
    let (mut a, mut b) = ([f32::MAX; 3], [f32::MIN; 3]);
    for c in 0..8 {
        let p = [if c & 1 == 0 { lo[0] } else { hi[0] }, if c & 2 == 0 { lo[1] } else { hi[1] }, if c & 4 == 0 { lo[2] } else { hi[2] }];
        for r in 0..3 {
            let w = m[4 * r] * p[0] + m[4 * r + 1] * p[1] + m[4 * r + 2] * p[2] + m[4 * r + 3];
            a[r] = a[r].min(w);
            b[r] = b[r].max(w);
        }
    }
    (a, b)
}

/// The sun volume's streamed lat-long sky picture, put in as the sky
/// material's colour map (the zone's record of it has no pixels of its
/// own). A line for the report.
fn sky_picture(map: &mut fastfile_t7::map::T7Map) -> Option<String> {
    let sky = map.light_refs()?.sky.clone()?;
    let data = match map.paks().map(|p| p.read(sky.key)) {
        Some(Ok(d)) => d,
        Some(Err(e)) => return Some(format!("sky {}: {e}", sky.image)),
        None => return Some(format!("sky {}: no packages", sky.image)),
    };
    let Some(want) = fastfile_t7::texture::chain_size(sky.format, sky.width, sky.height, 1) else {
        return Some(format!("sky {}: format {} not handled", sky.image, sky.format));
    };
    if data.len() < want {
        return Some(format!("sky {}: {} bytes, {want} wanted", sky.image, data.len()));
    }
    let levels = fastfile_t7::texture::ImageLevels { format: sky.format, width: sky.width, height: sky.height, levels: 1, data: data[..want].to_vec(), part: Some(0) };
    map.add_image(bare(&sky.image).to_owned(), levels);
    let scan = if sky.format == 95 { format!("; {}", bc6h_invalid_blocks(&data[..want], sky.width, sky.height)) } else { String::new() };
    Some(format!("sky {}: {}x{} format {} from the packages{scan}", sky.image, sky.width, sky.height, sky.format))
}

/// `T7_SYNTHETIC_SKY_MESH=1` (offline checks only, never on by default):
/// a sky box model whose streamed mesh is in no package read gets a unit
/// sphere (a lat-long grid in as many of its vertices and indices as the
/// record has, the rest degenerate; the rest of its data zero), so the sky's
/// draw path (pipeline, bindings, recording and what it draws) can be
/// checked where the base packages are not present.
fn synthetic_mesh(model: &str, me: &fastfile_t7::walk::capture::MeshRec) -> Option<Vec<u8>> {
    if !(model.starts_with("skybox") && std::env::var("T7_SYNTHETIC_SKY_MESH").as_deref() == Ok("1")) {
        return None;
    }
    let mut data = vec![0u8; me.data_size as usize];
    let nv = me.vert_count as usize;
    let h = ((nv as f32 / 2.0).sqrt() as usize).max(2);
    let w = (2 * h).min(nv / h.max(1)).max(3);
    let [pos, _, idx, _] = me.offsets.map(|o| o as usize);
    for j in 0..h {
        for i in 0..w {
            let (th, ph) = ((j as f32 + 0.5) / h as f32 * std::f32::consts::PI, i as f32 / w as f32 * std::f32::consts::TAU);
            let p = [th.sin() * ph.cos(), th.sin() * ph.sin(), th.cos()].map(|v| v * 1000.0);
            let at = pos + 12 * (j * w + i);
            if let Some(d) = data.get_mut(at..at + 12) {
                for (k, v) in p.iter().enumerate() {
                    d[4 * k..4 * k + 4].copy_from_slice(&v.to_le_bytes());
                }
            }
        }
    }
    let mut n = 0usize;
    for j in 0..h - 1 {
        for i in 0..w {
            let (a, b, c, d) = (j * w + i, j * w + (i + 1) % w, (j + 1) * w + i, (j + 1) * w + (i + 1) % w);
            for t in [a, c, b, b, c, d] {
                let at = idx + 2 * n;
                if let Some(x) = data.get_mut(at..at + 2) {
                    x.copy_from_slice(&(t as u16).to_le_bytes());
                }
                n += 1;
                if n >= me.index_count as usize {
                    return Some(data);
                }
            }
        }
    }
    Some(data)
}

#[cfg(test)]
mod given_up_tests {
    /// A scene no longer wanted stops before it reads anything.
    #[test]
    fn a_scene_no_longer_wanted_stops() {
        let r = super::Scene::load_unless(std::path::Path::new("nowhere/mp_given_up.ff"), std::sync::Arc::new(|| true));
        assert_eq!(r.err().as_deref(), Some(super::LET_GO));
    }
}

#[cfg(test)]
mod shadow_caster_tests {
    #[test]
    fn shadow_only_materials_are_told() {
        assert!(super::is_shadow_caster("wc/shadowcaster", "wc/shadowcaster#882572b8"));
        assert!(super::is_shadow_caster("mc/mtl_wall_onlycast", "mc/lit#e6142445"));
        assert!(!super::is_shadow_caster("wc/t7_snow_powder_01", "wc/lit_detail_plus#882572b8"));
        assert!(!super::is_shadow_caster("wc/t7_shadow_panel", "wc/lit#882572b8"));
    }
}
