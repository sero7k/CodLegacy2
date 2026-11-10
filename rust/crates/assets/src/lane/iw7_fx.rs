//! bo2mp: Infinite Warfare guns' own muzzle flashes, shell ejects and
//! tracers (`BO2MP_IW7_OWN_FX=0` gives the donor's back).
//!
//! A gun's `WeaponVFXPackage` names its effects, all of them particle
//! systems (`fastfile_iw7::walk::vfx`). Per effect slot this converts the
//! system (and the systems its runner modules pick from) to the game's
//! effect elements through [`ParticleFxView`] (`asset_game::fx_catalog_iw7`):
//!
//! - a sprite emitter (billboard, oriented sprite, tail) is an element of
//!   its material(s), its colour, alpha and size graphs sampled at even
//!   steps over the particle's life, its velocity graph, gravity, first
//!   rotation, flipbook and spawn (a burst, or a loop for an emitter that
//!   lives);
//! - a model emitter (a shell casing) is a model element when its model is
//!   in the zones (its materials linked the gun's way);
//! - a runner picks one of its child systems;
//! - everything else (lights, beams, flares, decals, geometry trails, cloud
//!   and volumetric emitters) and every module that is not read is dropped
//!   and counted, per gun, in the probe.
//!
//! Sprite materials come from the technique-set zones with their
//! `colorTint` (a 200 times exposure multiplier in an additive effect
//! material) left out, drawn unlit, in the blend their technique set names.
//! A sprite whose picture is not here (the `imagefile` pak) is not drawn at
//! all; a slot with nothing drawable keeps the donor's effect. The pictures
//! are not replaced by a stand-in: a grey card is worse than Black Ops II's
//! flash.
//!
//! Guesses (not seen on a screen yet): sizes are halved (Infinite Warfare's
//! sprite size is a width, the element's a radius), curve values are times
//! the curve's scale as the graph is authored, the velocity is the local
//! frame's, `gravity` is the engine's gravity times the module's number, a
//! shell with no velocity of its own gets an eject velocity of right, up and
//! back.

use std::collections::{BTreeMap, BTreeSet};

use asset_game::{
    FX_SLOT_TRACER, FX_SLOT_VIEW_EJECT, FX_SLOT_VIEW_FLASH, FX_SLOT_VIEW_LAST_EJECT,
    FX_SLOT_WORLD_EJECT, FX_SLOT_WORLD_FLASH, FX_SLOT_WORLD_LAST_EJECT, FxCatalog, FxModelCatalog,
    ParticleElemView, ParticleFxView, ParticleKind, ParticleStep, TracerCatalog,
};
use fastfile_iw7::layout::ParticleEmitterDef as E;
use fastfile_iw7::walk::vfx::{
    CurveRec, EmitterRec, LinkRec, ModuleRec, VfxRec, module_type as mt,
};

use super::*;

/// The effect slots of `WeaponVFXPackageDefs` that have a Black Ops II
/// counterpart: (offset, `FX_SLOT_*` bit, first person).
const SLOTS: [(usize, u8, bool); 6] = [
    (0, FX_SLOT_VIEW_FLASH, true),
    (16, FX_SLOT_WORLD_FLASH, false),
    (96, FX_SLOT_VIEW_EJECT, true),
    (112, FX_SLOT_WORLD_EJECT, false),
    (128, FX_SLOT_VIEW_LAST_EJECT, true),
    (144, FX_SLOT_WORLD_LAST_EJECT, false),
];

/// The slots with no counterpart (counted in the probe): the aimed flash and
/// the magazine eject.
const SLOTS_UNUSED: [(usize, &str); 2] = [(32, "view ADS flash"), (160, "view magazine eject")];

/// The picture size sprites are read at at most.
const FX_MAX: u32 = 512;

/// Samples of a graph over a particle's life when it bends.
const STEPS: usize = 8;

/// Infinite Warfare's sprite size is a width; an element's is a radius.
const SIZE_FACTOR: f32 = 0.5;

/// What the engine's gravity is, per unit of a gravity module's number.
const GRAVITY_UNIT: f32 = 800.0;

/// A shell with no velocity: right, up and back, local frame, per second.
const EJECT_VELOCITY: [[f32; 3]; 2] = [[-30.0, -90.0, 40.0], [30.0, -60.0, 90.0]];

/// Whether the gun lane plays the guns' own effects.
pub(super) fn own_enabled() -> bool {
    std::env::var("BO2MP_IW7_OWN_FX").map_or(true, |v| v.trim() != "0")
}

/// What the converter built.
#[derive(Default)]
pub(super) struct OwnFx {
    pub fx: FxCatalog,
    pub fx_models: FxModelCatalog,
    pub tracers: TracerCatalog,
    pub guns: Vec<GunFx>,
    pub report: Vec<String>,
}

/// One gun's effects.
#[derive(Default)]
pub(super) struct GunFx {
    pub gun: String,
    /// (`FX_SLOT_*` bit, effect name) the gun plays.
    pub slots: Vec<(u8, String)>,
    /// The probe: per slot, its elements and dropped modules.
    pub lines: Vec<String>,
}

/// A converted system, before the pictures are known.
struct Sys {
    view: ParticleFxView,
    children: Vec<String>,
    /// Elements converted (before pictures), and what was dropped.
    elems: usize,
    dropped: BTreeMap<String, usize>,
}

struct Conv<'a> {
    core: &'a Core,
    shared: &'a SharedZones,
    by_name: HashMap<String, (usize, usize)>,
    packages: HashMap<String, (usize, usize)>,
    /// Catalog name -> system; `order` has children before parents.
    systems: HashMap<String, Sys>,
    order: Vec<String>,
    sprite_materials: BTreeSet<String>,
    models: BTreeSet<String>,
}

fn catalog_name(name: &str, view: bool) -> String {
    if view {
        format!("{name}#vm")
    } else {
        name.to_owned()
    }
}

/// The unsupported module types, named for the probe.
fn module_label(ty: u32) -> String {
    match ty {
        mt::INIT_BEAM => "beam",
        mt::INIT_CAMERA_OFFSET => "cameraOffset",
        mt::INIT_CLOUD => "cloud",
        mt::INIT_DECAL => "decal",
        mt::INIT_FLARE => "flare",
        mt::INIT_GEO_TRAIL => "geoTrail",
        mt::INIT_LIGHT_OMNI => "omniLight",
        mt::INIT_LIGHT_SPOT => "spotLight",
        mt::INIT_MIRROR_TEXTURE => "mirrorTexture",
        mt::INIT_OCCLUSION_QUERY => "occlusionQuery",
        mt::INIT_ORIENTED_SPRITE => "orientedSprite",
        mt::INIT_PARTICLE_SIM => "particleSim",
        mt::INIT_PLAYER_FACING => "playerFacing",
        mt::INIT_RELATIVE_VELOCITY => "relativeVelocity",
        mt::INIT_ROTATION_3D => "rotation3D",
        mt::INIT_SOUND => "sound",
        mt::INIT_SPAWN_BOX => "spawnBox",
        mt::INIT_SPAWN_CYLINDER => "spawnCylinder",
        mt::INIT_SPAWN_ELLIPSOID => "spawnEllipsoid",
        mt::INIT_SPAWN_MESH => "spawnMesh",
        mt::INIT_SPAWN_SPHERE => "spawnSphere",
        mt::INIT_TAIL => "tail",
        mt::INIT_VECTOR_FIELD => "vectorField",
        mt::INIT_VOLUMETRIC => "volumetric",
        mt::ATTRACTOR => "attractor",
        mt::COLOR_LERP => "colorLerp",
        mt::EMISSION_GRAPH => "emission",
        mt::EMISSIVE_GRAPH => "emissive",
        mt::FORCE => "force",
        mt::PARENT_VELOCITY_GRAPH => "parentVelocity",
        mt::PHYSICS_LIGHT => "physicsLight",
        mt::PHYSICS_RAY_CAST => "physicsRayCast",
        mt::POSITION_GRAPH => "positionGraph",
        mt::ROTATION_GRAPH => "rotationGraph",
        mt::ROTATION_GRAPH_3D => "rotationGraph3D",
        mt::SIZE_LERP => "sizeLerp",
        mt::TEST_AGE..=mt::TEST_POS => "event",
        _ => return format!("module {ty:#04x}"),
    }
    .to_owned()
}

/// The modules the conversion reads.
const READ: [u32; 16] = [
    mt::INIT_SPAWN,
    mt::INIT_SPAWN_BOX,
    mt::INIT_MATERIAL,
    mt::INIT_ATLAS,
    mt::INIT_ATTRIBUTES,
    mt::INIT_ROTATION,
    mt::COLOR_GRAPH,
    mt::INTENSITY_GRAPH,
    mt::SIZE_GRAPH,
    mt::VELOCITY_GRAPH,
    mt::GRAVITY,
    mt::INIT_MODEL,
    mt::INIT_RUNNER,
    mt::FORCE,
    mt::INIT_ORIENTED_SPRITE,
    mt::INIT_TAIL,
];

/// A spawn box module's volume, per axis (minimum, maximum), in the effect's
/// local frame (x forward): two float4 at data words 12 and 16. Checked on
/// `common_core_mp`'s 3917 box modules: minimum <= maximum on every axis in
/// all of them and the fourth floats zero. `None` for a box that is not
/// finite or is turned inside out. (Word 3, 0 to 2, is probably the spawn
/// mode: read as a filled volume for all three.)
fn spawn_box(m: &ModuleRec) -> Option<[[f32; 2]; 3]> {
    let lo = [m.f32_at(48), m.f32_at(52), m.f32_at(56)];
    let hi = [m.f32_at(64), m.f32_at(68), m.f32_at(72)];
    let ok = (0..3).all(|k| lo[k].is_finite() && hi[k].is_finite() && lo[k] <= hi[k] && hi[k] - lo[k] <= 10_000.0);
    ok.then(|| core::array::from_fn(|k| [lo[k], hi[k]]))
}

/// The module of type `ty` in a state: the first not flagged off (bit 0),
/// else the first.
fn find(groups: &[Vec<ModuleRec>; 3], ty: u32) -> Option<&ModuleRec> {
    let mut all = groups.iter().flatten().filter(|m| m.ty == ty);
    let first = all.next()?;
    if first.u32_at(4) & 1 == 0 {
        return Some(first);
    }
    groups
        .iter()
        .flatten()
        .find(|m| m.ty == ty && m.u32_at(4) & 1 == 0)
        .or(Some(first))
}

/// A curve at `t` (0..1), linear between its control points.
fn curve_at(c: &CurveRec, t: f32) -> Option<f32> {
    let p = &c.points;
    let first = p.first()?;
    if t <= first.0 {
        return Some(first.1);
    }
    for w in p.windows(2) {
        if t <= w[1].0 {
            let span = (w[1].0 - w[0].0).max(1e-6);
            return Some(w[0].1 + (w[1].1 - w[0].1) * ((t - w[0].0) / span));
        }
    }
    p.last().map(|l| l.1)
}

/// Curve `i` of a graph module at `t`, times its scale.
fn graph(m: Option<&ModuleRec>, i: usize, t: f32, default: f32) -> f32 {
    m.and_then(|m| m.curves.get(i))
        .and_then(|c| curve_at(c, t).map(|v| v * c.scale))
        .filter(|v| v.is_finite())
        .unwrap_or(default)
}

fn finite_or(v: f32, default: f32) -> f32 {
    if v.is_finite() { v } else { default }
}

impl<'a> Conv<'a> {
    fn new(core: &'a Core, shared: &'a SharedZones) -> Self {
        let mut by_name = HashMap::new();
        let mut packages = HashMap::new();
        for (z, w) in core.zones.iter().enumerate() {
            for (i, v) in w.cap.vfx.iter().enumerate() {
                if !v.emitters.is_empty() {
                    by_name.entry(bare(&v.name).to_owned()).or_insert((z, i));
                }
            }
            for (i, p) in w.cap.vfx_packages.iter().enumerate() {
                if !p.effects.is_empty() {
                    packages.entry(bare(&p.name).to_owned()).or_insert((z, i));
                }
            }
        }
        Self {
            core,
            shared,
            by_name,
            packages,
            systems: HashMap::new(),
            order: Vec::new(),
            sprite_materials: BTreeSet::new(),
            models: BTreeSet::new(),
        }
    }

    /// Whether a material is drawn additively (its technique set says so).
    fn additive(&self, name: &str) -> bool {
        self.shared.material(name).is_some_and(|m| {
            fastfile_iw7::look::look(&m.technique_set).blend == fastfile_iw7::look::Blend::Add
        })
    }

    /// A system (by name) as a catalog effect; its catalog name.
    fn system(&mut self, name: &str, view: bool, depth: usize) -> Option<String> {
        let cname = catalog_name(name, view);
        if self.systems.contains_key(&cname) {
            return Some(cname);
        }
        let &(z, i) = self.by_name.get(name)?;
        if depth > 6 {
            return None;
        }
        // (Reserve the name: a system that runs itself is left alone.)
        let rec: VfxRec = self.core.zones[z].cap.vfx[i].clone();
        let mut sys = Sys {
            view: ParticleFxView {
                name: cname.clone(),
                ..ParticleFxView::default()
            },
            children: Vec::new(),
            elems: 0,
            dropped: BTreeMap::new(),
        };
        for (n, e) in rec.emitters.iter().enumerate() {
            if let Some(el) = self.emitter(e, n, view, z, depth, &mut sys) {
                sys.view.elems.push(el);
            }
        }
        sys.elems = sys.view.elems.len();
        self.order.push(cname.clone());
        self.systems.insert(cname.clone(), sys);
        Some(cname)
    }

    fn emitter(
        &mut self,
        e: &EmitterRec,
        n: usize,
        view: bool,
        zone: usize,
        depth: usize,
        sys: &mut Sys,
    ) -> Option<ParticleElemView> {
        let drop = |sys: &mut Sys, what: String| *sys.dropped.entry(what).or_default() += 1;
        let Some(state) = e.states.first() else {
            drop(sys, "emitter without a state".into());
            return None;
        };
        if e.states.len() > 1 {
            for _ in 1..e.states.len() {
                drop(sys, "extra state".into());
            }
        }
        let kind = match state.element_type {
            0 => ParticleKind::Billboard,
            10 => ParticleKind::Oriented,
            12 => ParticleKind::Tail,
            9 => ParticleKind::Model,
            11 => ParticleKind::Runner,
            1 => return self.skip(sys, "beam emitter"),
            2 | 3 => return self.skip(sys, "cloud emitter"),
            4 => return self.skip(sys, "decal emitter"),
            5 => return self.skip(sys, "flare emitter"),
            6 => return self.skip(sys, "geometry trail emitter"),
            7 => return self.skip(sys, "omni light"),
            8 => return self.skip(sys, "spot light"),
            other => return self.skip(sys, &format!("element type {other}")),
        };
        let groups = &state.groups;
        // Modules not read, counted.
        for m in groups.iter().flatten() {
            if !READ.contains(&m.ty) && m.u32_at(4) & 1 == 0 {
                drop(sys, module_label(m.ty));
            }
        }
        let life_min = e.f32_at(E::particleLife__min);
        let life_max = e.f32_at(E::particleLife__max);
        let life = if life_max > 0.0 && life_min.is_finite() && life_max.is_finite() {
            [life_min.clamp(0.0, life_max), life_max]
        } else {
            [0.1, 0.1]
        };
        let delay_min = e.f32_at(E::particleDelay__min) + e.f32_at(E::emitterDelay__min);
        let delay_max = e.f32_at(E::particleDelay__max) + e.f32_at(E::emitterDelay__max);
        let delay = [delay_min.max(0.0), delay_max.max(delay_min).max(0.0)];
        let (rate_min, rate_max) = (
            e.f32_at(E::particleSpawnRate__min),
            e.f32_at(E::particleSpawnRate__max),
        );
        let rate = finite_or((rate_min + rate_max) * 0.5, 0.0);
        let emitter_life = e.f32_at(E::emitterLife__max);
        let looping = (emitter_life > 0.0 && rate > 0.0).then(|| {
            sys.view.looping_life_ms = sys.view.looping_life_ms.max((emitter_life * 1000.0) as i32);
            (((1000.0 / rate) as i32).max(16), 1)
        });
        let burst = {
            let (lo, hi) = (
                e.i32_at(E::particleBurstCount__min),
                e.i32_at(E::particleBurstCount__max),
            );
            if hi <= 0 {
                [1, 1]
            } else {
                [lo.clamp(0, hi), hi]
            }
        };

        let mut el = ParticleElemView {
            kind,
            looping,
            burst,
            delay,
            life,
            steps: Vec::new(),
            has_velocity: false,
            gravity: [0.0; 2],
            origin: [[0.0; 2]; 3],
            offset_radius: [0.0; 2],
            initial_rotation: [0.0; 2],
            materials: Vec::new(),
            models: Vec::new(),
            effects: Vec::new(),
            atlas: None,
            view_model: view,
            sort_order: (n as u32).min(255) as u8,
        };

        // (`BO2MP_IW7_FX_SPAWN_BOX=0` spawns every particle at the origin, as before.)
        if std::env::var_os("BO2MP_IW7_FX_SPAWN_BOX").is_none_or(|v| v != "0")
            && let Some(b) = find(groups, mt::INIT_SPAWN_BOX).and_then(spawn_box)
        {
            el.origin = b;
        }

        if kind == ParticleKind::Runner {
            let Some(run) = find(groups, mt::INIT_RUNNER) else {
                drop(sys, "runner without links".into());
                return None;
            };
            for l in &run.links {
                if let LinkRec::Asset(id) = l
                    && id.ty == T::Vfx
                {
                    let name = bare(self.core.zones[zone].cap.name_of(*id)).to_owned();
                    match self.system(&name, view, depth + 1) {
                        Some(child) => {
                            if !el.effects.contains(&child) {
                                el.effects.push(child.clone());
                                sys.children.push(child);
                            }
                        }
                        None => drop(sys, "runner child not found".into()),
                    }
                }
            }
            if el.effects.is_empty() {
                return None;
            }
            el.steps = vec![ParticleStep::default(); 2];
            return Some(el);
        }

        // Materials or the model.
        if kind == ParticleKind::Model {
            if let Some(m) = find(groups, mt::INIT_MODEL) {
                for l in &m.links {
                    if let LinkRec::Asset(id) = l
                        && id.ty == T::XModel
                    {
                        let name = bare(self.core.zones[zone].cap.name_of(*id)).to_owned();
                        if !name.is_empty() && !el.models.contains(&name) {
                            self.models.insert(name.clone());
                            el.models.push(name);
                        }
                    }
                }
            }
            if el.models.is_empty() {
                // (The other variants of a shell leave the model to one
                // variant: the walk dedups the link.)
                drop(sys, "model emitter without a model".into());
                return None;
            }
        } else {
            if let Some(m) = find(groups, mt::INIT_MATERIAL) {
                for l in &m.links {
                    if let LinkRec::Asset(id) = l
                        && id.ty == T::Material
                    {
                        let name = bare(self.core.zones[zone].cap.name_of(*id)).to_owned();
                        if !name.is_empty() && !el.materials.contains(&name) {
                            self.sprite_materials.insert(name.clone());
                            el.materials.push(name);
                        }
                    }
                }
            }
            if el.materials.is_empty() {
                drop(sys, "sprite emitter without a material".into());
                return None;
            }
        }

        // The graphs.
        let attrs = find(groups, mt::INIT_ATTRIBUTES);
        let colour = find(groups, mt::COLOR_GRAPH);
        let intensity = find(groups, mt::INTENSITY_GRAPH);
        let size = find(groups, mt::SIZE_GRAPH);
        let velocity = find(groups, mt::VELOCITY_GRAPH);
        let bends = [colour, intensity, size, velocity]
            .into_iter()
            .flatten()
            .flat_map(|m| m.curves.iter())
            .any(|c| c.points.len() > 2);
        let steps = if bends { STEPS } else { 2 };
        let additive = el.materials.iter().any(|m| self.additive(m));
        // (Attributes: the colour multipliers of the two sets, at 48 and
        // 64; the size at 16 and 32 when there is no size graph.)
        let mult = |k: usize| -> [f32; 4] {
            core::array::from_fn(|c| {
                attrs
                    .map(|a| a.f32_at(48 + 16 * k + 4 * c))
                    .filter(|v| v.is_finite() && *v > 0.0)
                    .unwrap_or(1.0)
            })
        };
        let (mult_min, mult_max) = (mult(0), mult(1));
        let attr_size = |k: usize| -> [f32; 2] {
            let at = 16 + 16 * k;
            [
                attrs.map_or(10.0, |a| finite_or(a.f32_at(at), 10.0)),
                attrs.map_or(10.0, |a| finite_or(a.f32_at(at + 4), 10.0)),
            ]
        };
        if size.is_none() {
            drop(sys, "no size graph (attributes' size)".into());
        }
        for s in 0..steps {
            let t = s as f32 / (steps - 1) as f32;
            let mut gain = graph(intensity, 0, t, 1.0) * graph(intensity, 1, t, 1.0);
            if !gain.is_finite() || gain < 0.0 {
                gain = 1.0;
            }
            if !additive {
                gain = gain.min(1.0);
            }
            let set = |base: usize, m: [f32; 4]| -> [f32; 4] {
                [
                    graph(colour, base, t, 1.0) * m[0] * gain,
                    graph(colour, base + 1, t, 1.0) * m[1] * gain,
                    graph(colour, base + 2, t, 1.0) * m[2] * gain,
                    graph(colour, base + 3, t, 1.0) * m[3],
                ]
            };
            let size_at = |base: usize, k: usize| -> [f32; 2] {
                if size.is_some() {
                    [
                        graph(size, base, t, 0.0) * SIZE_FACTOR,
                        graph(size, base + 1, t, 0.0) * SIZE_FACTOR,
                    ]
                } else {
                    let a = attr_size(k);
                    [a[0] * SIZE_FACTOR, a[1] * SIZE_FACTOR]
                }
            };
            let vel = |base: usize| -> [f32; 3] {
                [
                    graph(velocity, base, t, 0.0),
                    graph(velocity, base + 1, t, 0.0),
                    graph(velocity, base + 2, t, 0.0),
                ]
            };
            el.steps.push(ParticleStep {
                colour: [set(0, mult_min), set(4, mult_max)],
                size: [size_at(0, 0), size_at(3, 1)],
                velocity: [vel(0), vel(3)],
            });
        }
        // (The colour graph's sets: red, green, blue, alpha; the second
        // set's red is curve 4. The minimum and maximum come out unordered:
        // the sample takes the span.)
        el.has_velocity = velocity.is_some();

        // Gravity: the module's number (min, max) times the engine's, and
        // a force pulling down (a shell's).
        let mut gravity = find(groups, mt::GRAVITY)
            .map(|g| [finite_or(g.f32_at(8), 0.0), finite_or(g.f32_at(12), 0.0)])
            .unwrap_or([0.0; 2]);
        gravity = [gravity[0] * GRAVITY_UNIT, gravity[1] * GRAVITY_UNIT];
        if let Some(f) = find(groups, mt::FORCE) {
            let down = [-finite_or(f.f32_at(24), 0.0), -finite_or(f.f32_at(40), 0.0)];
            gravity = [gravity[0] + down[0].max(0.0), gravity[1] + down[1].max(0.0)];
        }
        el.gravity = gravity;
        if let Some(r) = find(groups, mt::INIT_ROTATION) {
            let (a, b) = (finite_or(r.f32_at(16), 0.0), finite_or(r.f32_at(20), 0.0));
            el.initial_rotation = [a.min(b).to_degrees(), a.max(b).to_degrees()];
        }
        if kind == ParticleKind::Model && !el.has_velocity {
            // (Its velocity is the game's: a shell is thrown out.)
            for s in &mut el.steps {
                s.velocity = EJECT_VELOCITY;
            }
            el.has_velocity = true;
            drop(sys, "~shell eject velocity made up".into());
        }
        if kind == ParticleKind::Model {
            // (A model is its own size: the graph's size is a scale.)
            for s in &mut el.steps {
                s.size = [[1.0, 1.0]; 2];
            }
        }
        // The flipbook: the atlas module, the material's grid.
        if let Some(a) = find(groups, mt::INIT_ATLAS)
            && let Some(m) = el.materials.first().and_then(|m| self.shared.material(m))
            && let Some(grid) = m.constants.iter().find(|c| c.0 == "textureAtlas")
        {
            let (cols, rows) = (
                grid.1[0].round().max(1.0) as u32,
                grid.1[1].round().max(1.0) as u32,
            );
            if cols * rows > 1 {
                let fps = a.u32_at(8);
                el.atlas = Some((cols, rows, 0, fps > 0 && fps < 100_000));
            }
        }
        Some(el)
    }

    fn skip(&mut self, sys: &mut Sys, what: &str) -> Option<ParticleElemView> {
        *sys.dropped.entry(what.to_owned()).or_default() += 1;
        None
    }

    /// A system and everything it runs: (elements, dropped by label).
    fn tree(&self, name: &str) -> (usize, BTreeMap<String, usize>) {
        let (mut elems, mut dropped) = (0usize, BTreeMap::new());
        let mut seen = BTreeSet::new();
        let mut todo = vec![name.to_owned()];
        while let Some(n) = todo.pop() {
            if !seen.insert(n.clone()) {
                continue;
            }
            if let Some(s) = self.systems.get(&n) {
                elems += s.elems;
                for (k, v) in &s.dropped {
                    *dropped.entry(k.clone()).or_default() += v;
                }
                todo.extend(s.children.iter().cloned());
            }
        }
        (elems, dropped)
    }
}

fn counts(m: &BTreeMap<String, usize>) -> String {
    if m.is_empty() {
        return "none".into();
    }
    m.iter()
        .map(|(k, v)| format!("{k} x{v}"))
        .collect::<Vec<_>>()
        .join(", ")
}

/// The tracer a gun fires: its definition and the zone that holds it.
fn tracer_of<'a>(core: &'a Core, name: &str) -> Option<(usize, &'a fastfile_iw7::walk::TracerRec)> {
    core.zones.iter().enumerate().find_map(|(z, w)| {
        w.cap
            .tracers
            .iter()
            .find(|t| bare(&t.name) == name && t.material.is_some())
            .map(|t| (z, t))
    })
}

/// A sprite material for the effects: no `colorTint`, named apart from the
/// guns' and the maps' (`#fx`).
fn sprite_material(shared: &SharedZones, raw: &str) -> Option<MapMaterial> {
    let mut m = shared.material(raw).cloned()?;
    m.constants.retain(|c| c.0 != "colorTint");
    m.name = format!("{raw}#fx");
    Some(m)
}

/// The guns' own effects.
pub(super) fn build(
    core: &Core,
    shared: &Arc<SharedZones>,
    seed: &mut MaterialCatalog,
    owner: ZoneOwner,
    out_failures: &mut Vec<String>,
) -> OwnFx {
    let mut out = OwnFx::default();
    out.fx.set_capture_ns(NS);
    out.fx_models.set_capture_ns(NS);
    out.tracers.set_capture_ns(NS);
    out.fx.set_capture_zone(owner);
    out.fx_models.set_capture_zone(owner);
    out.tracers.set_capture_zone(owner);
    let mut conv = Conv::new(core, shared);

    // 1. Convert what every gun names.
    struct Plan {
        gun: String,
        slots: Vec<(u8, String, String)>,
        unused: Vec<String>,
        tracer: Option<String>,
    }
    let mut plans = Vec::new();
    let mut tracer_materials: BTreeMap<String, String> = BTreeMap::new();
    for g in bo2_profile::guest::IW7 {
        let Some((wz, rec)) = core.weapon(&iw7_weapon_name(g.gun)) else {
            continue;
        };
        let wcap = &core.zones[wz].cap;
        let field = |f: WeaponField| rec.assets.iter().find(|(x, _)| *x == f).map(|(_, id)| *id);
        let mut plan = Plan {
            gun: g.gun.to_owned(),
            slots: Vec::new(),
            unused: Vec::new(),
            tracer: None,
        };
        let package = field(WeaponField::Def(D::vfxPackage))
            .map(|id| bare(wcap.name_of(id)).to_owned())
            .or_else(|| Some(format!("iw7_{}_mp_vfxpackage", g.gun)))
            .and_then(|n| conv.packages.get(&n).copied());
        if let Some((pz, pi)) = package {
            let pcap = &core.zones[pz].cap;
            let effects = pcap.vfx_packages[pi].effects.clone();
            for &(offset, bit, view) in &SLOTS {
                let Some(&(_, id)) = effects.iter().find(|(o, _)| *o == offset) else {
                    continue;
                };
                if id.ty != T::Vfx {
                    continue;
                }
                let name = bare(pcap.name_of(id)).to_owned();
                if let Some(cname) = conv.system(&name, view, 0) {
                    plan.slots.push((bit, name, cname));
                } else {
                    plan.unused.push(format!("{name} (not in the zones)"));
                }
            }
            for &(offset, what) in &SLOTS_UNUSED {
                if effects
                    .iter()
                    .any(|(o, id)| *o == offset && id.ty == T::Vfx)
                {
                    plan.unused.push(what.to_owned());
                }
            }
        }
        if let Some(name) =
            field(WeaponField::Def(D::tracerType)).map(|id| bare(wcap.name_of(id)).to_owned())
            && !name.is_empty()
            && let Some((tz, t)) = tracer_of(core, &name)
            && let Some(mat) = t
                .material
                .map(|id| bare(core.zones[tz].cap.name_of(id)).to_owned())
            && !mat.is_empty()
        {
            tracer_materials.insert(name.clone(), mat);
            plan.tracer = Some(name);
        }
        plans.push(plan);
    }

    // 2. Link the materials (the sprites', the tracers', the models').
    let mut wanted: Vec<MapMaterial> = Vec::new();
    let mut raw_of: Vec<(String, bool)> = Vec::new();
    for raw in conv
        .sprite_materials
        .iter()
        .chain(tracer_materials.values())
    {
        if raw_of.iter().any(|(r, m)| r == raw && !*m) {
            continue;
        }
        if let Some(m) = sprite_material(shared, raw) {
            wanted.push(m);
            raw_of.push((raw.clone(), false));
        }
    }
    struct ModelPlan {
        name: String,
        xref: asset_t6::XModelRef,
        names: Vec<String>,
        surface: Vec<Option<String>>,
    }
    let mut model_plans: Vec<ModelPlan> = Vec::new();
    for name in &conv.models {
        let Some(&(z, _)) = core.index.get(&(T::XModel, name.clone())) else {
            continue;
        };
        let Some((w, m)) = find_model(name, &core.zones[z], core) else {
            continue;
        };
        let Some(ModelRef {
            xref, materials, ..
        }) = model_ref(w, m)
        else {
            continue;
        };
        let mut names: Vec<String> = Vec::new();
        for m in materials.iter().flatten() {
            if !names.contains(m) {
                names.push(m.clone());
            }
        }
        for n in &names {
            if !raw_of.iter().any(|(r, m)| r == n && *m)
                && let Some(def) = shared.material(n).cloned()
            {
                wanted.push(def);
                raw_of.push((n.clone(), true));
            }
        }
        model_plans.push(ModelPlan {
            name: name.clone(),
            xref,
            names,
            surface: materials,
        });
    }
    let map = Iw7Map::for_materials("iw7_fx", wanted, shared.clone());
    let mut b = Materials::for_gun(&map, std::mem::take(seed), owner, FX_MAX);
    let index_by_name: HashMap<String, usize> = map
        .materials
        .iter()
        .enumerate()
        .map(|(i, m)| (m.name.to_ascii_lowercase(), i))
        .collect();
    // raw sprite material -> catalog name, when it has a picture.
    let mut drawn: HashMap<String, String> = HashMap::new();
    let mut no_picture: BTreeSet<String> = BTreeSet::new();
    let mut hidden: BTreeSet<String> = BTreeSet::new();
    // (A probe: `BO2MP_IW7_FX_KEEP_UNPICTURED=1` keeps sprites whose picture is
    // not here, to see the whole conversion on a folder without the paks.)
    let keep_unpictured = std::env::var_os("BO2MP_IW7_FX_KEEP_UNPICTURED").is_some();
    for (raw, as_model) in &raw_of {
        if *as_model {
            continue;
        }
        let Some(&i) = index_by_name.get(&format!("{raw}#fx").to_ascii_lowercase()) else {
            continue;
        };
        let Some(slot) = b.material(i, false) else {
            continue;
        };
        let has_picture = b.catalog.materials[slot]
            .textures
            .iter()
            .any(|t| t.semantic == TS_COLOR_MAP && t.image.is_some());
        if let Some(d) = b.catalog.materials[slot].t6_draw.as_mut() {
            if matches!(
                d.blend,
                asset_core::T6Blend::ShadowOnly | asset_core::T6Blend::Sky
            ) {
                hidden.insert(raw.clone());
                continue;
            }
            if d.blend == asset_core::T6Blend::Opaque {
                d.blend = asset_core::T6Blend::Blend;
            }
            d.unlit = true;
            d.effect = true;
            d.alpha_test = false;
            d.cull = asset_core::T6Cull::None;
        }
        if has_picture || keep_unpictured {
            drawn.insert(
                raw.clone(),
                b.catalog.materials[slot].name.as_str().to_owned(),
            );
        } else {
            no_picture.insert(raw.clone());
        }
    }
    // Models: their surfaces' materials, then the model.
    let mut model_names: HashMap<String, String> = HashMap::new();
    for mp in model_plans {
        let mut slots: HashMap<usize, usize> = HashMap::new();
        for (k, n) in mp.names.iter().enumerate() {
            if let Some(&i) = index_by_name.get(&n.to_ascii_lowercase())
                && let Some(slot) = b.material(i, true)
            {
                slots.insert(k, slot);
            }
        }
        let mut xref = mp.xref;
        let key = |k: usize| asset_t6::AssetKey {
            ty: fastfile_t6::AssetType::Material,
            index: k,
        };
        let index_of = |m: &Option<String>| {
            m.as_ref()
                .and_then(|m| mp.names.iter().position(|n| n == m))
        };
        xref.materials = mp.surface.iter().map(|m| index_of(m).map(key)).collect();
        for (s, m) in xref.lod0.iter_mut().zip(&mp.surface) {
            s.material = index_of(m).map(key);
        }
        if let Some(skel) =
            asset_model::capture_model_skel_t6(&xref, |k| slots.get(&k.index).copied())
        {
            let n = skel.name.clone();
            out.fx_models.insert_skel(skel, &b.catalog);
            model_names.insert(mp.name, n);
        }
    }
    out_failures.extend(b.failures.iter().cloned());
    *seed = std::mem::take(&mut b.catalog);
    drop(b);

    // 3. The systems that draw something, children first.
    let mut drawable: HashSet<String> = HashSet::new();
    let mut lost: BTreeMap<String, usize> = BTreeMap::new();
    let order = conv.order.clone();
    for name in &order {
        let Some(sys) = conv.systems.get_mut(name) else {
            continue;
        };
        let mut view = std::mem::take(&mut sys.view);
        let mut kept = Vec::new();
        for mut el in view.elems.drain(..) {
            match el.kind {
                ParticleKind::Runner => {
                    el.effects.retain(|e| drawable.contains(e));
                }
                ParticleKind::Model => {
                    el.models = el
                        .models
                        .iter()
                        .filter_map(|m| model_names.get(m).cloned())
                        .collect();
                    if el.models.is_empty() {
                        *sys.dropped
                            .entry("model not in the zones".into())
                            .or_default() += 1;
                        *lost.entry("model".into()).or_default() += 1;
                        continue;
                    }
                }
                _ => {
                    el.materials = el
                        .materials
                        .iter()
                        .filter_map(|m| drawn.get(m).cloned())
                        .collect();
                    if el.materials.is_empty() {
                        *sys.dropped
                            .entry("sprite without a picture".into())
                            .or_default() += 1;
                        *lost.entry("picture".into()).or_default() += 1;
                        continue;
                    }
                }
            }
            if el.kind == ParticleKind::Runner && el.effects.is_empty() {
                *sys.dropped
                    .entry("runner without drawable children".into())
                    .or_default() += 1;
                continue;
            }
            kept.push(el);
        }
        view.elems = kept;
        if out.fx.capture_particles_in(NS, &view) {
            drawable.insert(name.clone());
        }
        sys.view = view;
    }

    // 4. Tracers.
    let mut tracer_ok: HashSet<String> = HashSet::new();
    for (name, mat) in &tracer_materials {
        let Some(catalog) = drawn.get(mat) else {
            continue;
        };
        let Some((_, t)) = tracer_of(core, name) else {
            continue;
        };
        if out.tracers.capture_iw7_in(
            NS,
            name,
            catalog,
            t.draw_interval,
            t.speed,
            [t.beam_length, t.beam_width, t.screw_radius, t.screw_dist],
            t.colors,
        ) {
            tracer_ok.insert(name.clone());
        }
    }

    // 5. Per gun: the slots played and the probe.
    let mut slot_total = 0usize;
    let mut all_dropped: BTreeMap<String, usize> = BTreeMap::new();
    for plan in plans {
        let mut gun = GunFx {
            gun: plan.gun.clone(),
            ..GunFx::default()
        };
        for (bit, name, cname) in &plan.slots {
            let (elems, dropped) = conv.tree(cname);
            for (k, v) in &dropped {
                *all_dropped.entry(k.clone()).or_default() += v;
            }
            let plays = drawable.contains(cname);
            if plays {
                gun.slots.push((*bit, cname.clone()));
                slot_total += 1;
            }
            gun.lines.push(format!(
                "{}: {} -> {} ({} elements read; dropped: {})",
                slot_label(*bit),
                name,
                if plays {
                    "own effect"
                } else {
                    "donor's (nothing drawable)"
                },
                elems,
                counts(&dropped)
            ));
        }
        for u in &plan.unused {
            gun.lines.push(format!("not used: {u}"));
        }
        match &plan.tracer {
            Some(t) if tracer_ok.contains(t) => {
                gun.slots.push((FX_SLOT_TRACER, t.clone()));
                gun.lines.push(format!("tracer: {t} -> own tracer"));
            }
            Some(t) => gun
                .lines
                .push(format!("tracer: {t} -> donor's (no picture)")),
            None => gun.lines.push("tracer: none read -> donor's".into()),
        }
        out.guns.push(gun);
    }
    out.report.push(format!(
        "iw7 guest guns: own effects: {} systems converted ({} drawn), {} slots and {} tracers for {} guns; {} sprite materials with a picture, {} without ({} hidden kinds), {} models; lost {}; dropped {}",
        conv.systems.len(),
        drawable.len(),
        slot_total,
        tracer_ok.len(),
        out.guns.iter().filter(|g| !g.slots.is_empty()).count(),
        drawn.len(),
        no_picture.len(),
        hidden.len(),
        model_names.len(),
        counts(&lost),
        counts(&all_dropped),
    ));
    out
}

fn slot_label(bit: u8) -> &'static str {
    match bit {
        FX_SLOT_VIEW_FLASH => "view flash",
        FX_SLOT_WORLD_FLASH => "world flash",
        FX_SLOT_VIEW_EJECT => "view eject",
        FX_SLOT_WORLD_EJECT => "world eject",
        FX_SLOT_VIEW_LAST_EJECT => "view last eject",
        FX_SLOT_WORLD_LAST_EJECT => "world last eject",
        _ => "slot",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_curve_is_linear_between_its_points() {
        let c = CurveRec {
            scale: 2.0,
            points: vec![(0.0, 0.0), (0.5, 1.0), (1.0, 0.0)],
        };
        assert_eq!(curve_at(&c, 0.25), Some(0.5));
        assert_eq!(curve_at(&c, 0.5), Some(1.0));
        assert_eq!(curve_at(&c, 2.0), Some(0.0));
        let m = ModuleRec {
            ty: mt::SIZE_GRAPH,
            curves: vec![c],
            ..ModuleRec::default()
        };
        assert_eq!(graph(Some(&m), 0, 0.5, 9.0), 2.0);
        assert_eq!(graph(Some(&m), 3, 0.5, 9.0), 9.0);
        assert_eq!(graph(None, 0, 0.5, 9.0), 9.0);
    }

    #[test]
    fn a_spawn_box_is_two_float4s_and_must_be_the_right_way_round() {
        let words = |lo: [f32; 3], hi: [f32; 3]| {
            let mut d = vec![0u8; 144];
            for (i, v) in lo.iter().enumerate() {
                d[48 + 4 * i..52 + 4 * i].copy_from_slice(&v.to_le_bytes());
            }
            for (i, v) in hi.iter().enumerate() {
                d[64 + 4 * i..68 + 4 * i].copy_from_slice(&v.to_le_bytes());
            }
            ModuleRec { ty: mt::INIT_SPAWN_BOX, data: d, ..ModuleRec::default() }
        };
        // A plane 5 units ahead, 4 wide and 4 high (a minigun flash's layout).
        assert_eq!(spawn_box(&words([5.0, -2.0, -2.0], [5.0, 2.0, 2.0])), Some([[5.0, 5.0], [-2.0, 2.0], [-2.0, 2.0]]));
        assert_eq!(spawn_box(&words([1.0, 0.0, 0.0], [0.0, 0.0, 0.0])), None);
        assert_eq!(spawn_box(&words([0.0, 0.0, 0.0], [f32::NAN, 0.0, 0.0])), None);
        assert_eq!(spawn_box(&words([-9000.0, 0.0, 0.0], [9000.0, 0.0, 0.0])), None);
    }

    #[test]
    fn a_view_system_has_its_own_name() {
        assert_eq!(catalog_name("vfx/a", true), "vfx/a#vm");
        assert_eq!(catalog_name("vfx/a", false), "vfx/a");
        assert_eq!(module_label(mt::FORCE), "force");
        assert_eq!(module_label(0x7f), "module 0x7f");
    }
}
