//! Particle systems (x64-zt `iw7/assets/particle_system.cpp`): emitters,
//! their states, module groups and modules, in the writer's order. A
//! module's type selects which union member (and so which pointers) it has.
//!
//! The walk keeps what a converter needs in a [`VfxRec`]: each emitter's
//! record, each state's three module groups, and per module its type, its
//! 144 data bytes (read by `layout::ParticleModuleTypeDef` offsets), its
//! curves' control points and the assets it links (materials, models,
//! effects, sounds).

use super::{AssetId, Walker};
use crate::asset_type::AssetType;
use crate::cursor::Result;
use crate::layout as L;
use crate::stream::VIRTUAL;

use L::ParticleModuleDef as M;

/// A module's type number (`ParticleModuleType`: the order of
/// `ParticleModuleTypeDef`'s members, checked against the types the walk
/// reads pointers for).
pub mod module_type {
    pub const INIT_ATLAS: u32 = 0x00;
    pub const INIT_ATTRIBUTES: u32 = 0x01;
    pub const INIT_BEAM: u32 = 0x02;
    pub const INIT_CAMERA_OFFSET: u32 = 0x03;
    pub const INIT_CLOUD: u32 = 0x04;
    pub const INIT_DECAL: u32 = 0x05;
    pub const INIT_FLARE: u32 = 0x06;
    pub const INIT_GEO_TRAIL: u32 = 0x07;
    pub const INIT_LIGHT_OMNI: u32 = 0x08;
    pub const INIT_LIGHT_SPOT: u32 = 0x09;
    pub const INIT_MATERIAL: u32 = 0x0a;
    pub const INIT_MIRROR_TEXTURE: u32 = 0x0b;
    pub const INIT_MODEL: u32 = 0x0c;
    pub const INIT_OCCLUSION_QUERY: u32 = 0x0d;
    pub const INIT_ORIENTED_SPRITE: u32 = 0x0e;
    pub const INIT_PARTICLE_SIM: u32 = 0x0f;
    pub const INIT_PLAYER_FACING: u32 = 0x10;
    pub const INIT_RELATIVE_VELOCITY: u32 = 0x11;
    pub const INIT_ROTATION: u32 = 0x12;
    pub const INIT_ROTATION_3D: u32 = 0x13;
    pub const INIT_RUNNER: u32 = 0x14;
    pub const INIT_SOUND: u32 = 0x15;
    pub const INIT_SPAWN: u32 = 0x16;
    pub const INIT_SPAWN_BOX: u32 = 0x17;
    pub const INIT_SPAWN_CYLINDER: u32 = 0x18;
    pub const INIT_SPAWN_ELLIPSOID: u32 = 0x19;
    pub const INIT_SPAWN_MESH: u32 = 0x1a;
    pub const INIT_SPAWN_SPHERE: u32 = 0x1b;
    pub const INIT_TAIL: u32 = 0x1c;
    pub const INIT_VECTOR_FIELD: u32 = 0x1d;
    pub const INIT_VOLUMETRIC: u32 = 0x1e;
    pub const ATTRACTOR: u32 = 0x1f;
    pub const COLOR_GRAPH: u32 = 0x20;
    pub const COLOR_LERP: u32 = 0x21;
    pub const EMISSION_GRAPH: u32 = 0x22;
    pub const EMISSIVE_GRAPH: u32 = 0x23;
    pub const FORCE: u32 = 0x24;
    pub const GRAVITY: u32 = 0x25;
    pub const INTENSITY_GRAPH: u32 = 0x26;
    pub const PARENT_VELOCITY_GRAPH: u32 = 0x27;
    pub const PHYSICS_LIGHT: u32 = 0x28;
    pub const PHYSICS_RAY_CAST: u32 = 0x29;
    pub const POSITION_GRAPH: u32 = 0x2a;
    pub const ROTATION_GRAPH: u32 = 0x2b;
    pub const ROTATION_GRAPH_3D: u32 = 0x2c;
    pub const SIZE_GRAPH: u32 = 0x2d;
    pub const SIZE_LERP: u32 = 0x2e;
    pub const VELOCITY_GRAPH: u32 = 0x2f;
    pub const TEST_AGE: u32 = 0x30;
    pub const TEST_BIRTH: u32 = 0x31;
    pub const TEST_DEATH: u32 = 0x32;
    pub const TEST_GRAVITY: u32 = 0x33;
    pub const TEST_IMPACT: u32 = 0x34;
    pub const TEST_POS: u32 = 0x35;
}

/// One animation curve: its scale and control points (time, value).
#[derive(Clone, Debug, Default)]
pub struct CurveRec {
    pub scale: f32,
    pub points: Vec<(f32, f32)>,
}

/// What a module links.
#[derive(Clone, Debug, PartialEq)]
pub enum LinkRec {
    Asset(AssetId),
    Sound(String),
}

/// One module: its type, its data bytes (`ParticleModuleTypeDef`, 144),
/// its curves (in the order the walk reads them) and linked assets.
#[derive(Clone, Debug, Default)]
pub struct ModuleRec {
    pub ty: u32,
    pub data: Vec<u8>,
    pub curves: Vec<CurveRec>,
    pub links: Vec<LinkRec>,
}

fn f32_in(b: &[u8], at: usize) -> f32 {
    b.get(at..at + 4).and_then(|b| b.try_into().ok()).map_or(0.0, f32::from_le_bytes)
}

impl ModuleRec {
    pub fn f32_at(&self, at: usize) -> f32 {
        f32_in(&self.data, at)
    }
    pub fn u32_at(&self, at: usize) -> u32 {
        self.f32_at(at).to_bits()
    }
}

/// A state: its three module groups (init, update, events), element type
/// and flags.
#[derive(Clone, Debug, Default)]
pub struct StateRec {
    pub groups: [Vec<ModuleRec>; 3],
    pub group_disabled: [u32; 3],
    pub element_type: u32,
    pub flags: u32,
}

/// An emitter: its `ParticleEmitterDef` bytes, states and fade curve.
#[derive(Clone, Debug, Default)]
pub struct EmitterRec {
    pub record: Vec<u8>,
    pub states: Vec<StateRec>,
    pub fade: CurveRec,
}

impl EmitterRec {
    pub fn f32_at(&self, at: usize) -> f32 {
        f32_in(&self.record, at)
    }
    pub fn i32_at(&self, at: usize) -> i32 {
        self.f32_at(at).to_bits() as i32
    }
}

/// A particle system as the walk read it.
#[derive(Clone, Debug, Default)]
pub struct VfxRec {
    pub name: String,
    pub version: u32,
    pub flags: u32,
    pub emitters: Vec<EmitterRec>,
}

/// `ParticleCurveDef`s at `field` (16 bytes each): their control points.
fn curves(w: &mut Walker, field: usize, n: usize) -> Result<Vec<CurveRec>> {
    let mut out = Vec::with_capacity(n);
    for i in 0..n {
        out.push(curve(w, field + L::ParticleCurveDef::SIZE * i)?);
    }
    Ok(out)
}

fn curve(w: &mut Walker, c: usize) -> Result<CurveRec> {
    let point = L::ParticleCurveControlPointDef::SIZE;
    let k = w.s.u32_at(c + L::ParticleCurveDef::numControlPoints)? as usize;
    let scale = w.s.f32_at(c + L::ParticleCurveDef::scale)?;
    let at = w.blob(c + L::ParticleCurveDef::controlPoints, point * k, 15)?;
    let mut points = Vec::new();
    if let Some(at) = at {
        let d = w.s.data();
        for j in 0..k {
            let p = at + point * j;
            points.push((
                f32_in(d, p + L::ParticleCurveControlPointDef::time),
                f32_in(d, p + L::ParticleCurveControlPointDef::value),
            ));
        }
    }
    Ok(CurveRec { scale, points })
}

/// One `ParticleLinkedAssetDef`: which member is set depends on the
/// owning module's type.
fn linked_asset(w: &mut Walker, ty: u32, a: usize, out: &mut Vec<LinkRec>) -> Result<()> {
    use AssetType as T;
    use L::ParticleLinkedAssetDef as D;
    match ty {
        0x0a => {
            let id = w.asset(T::Material, a + D::material)?;
            out.extend(id.map(LinkRec::Asset));
        }
        0x0c | 0x1a => {
            let id = w.asset(T::XModel, a + D::model)?;
            out.extend(id.map(LinkRec::Asset));
        }
        0x28 => {
            w.asset(T::PhysicsFxPipeline, a + D::physicsFXData__physicsFXPipeline)?;
            w.asset(T::PhysicsFxShape, a + D::physicsFXData__physicsFXShape)?;
        }
        0x0f => {
            w.asset(T::ParticleSimAnimation, a + D::particleSim)?;
        }
        0x05 => {
            for i in 0..3 {
                w.asset(T::Material, a + D::decal__materials + 8 * i)?;
            }
        }
        0x15 => {
            if let Some(s) = w.xstr(a + D::sound)? {
                out.push(LinkRec::Sound(s));
            }
        }
        0x1d => {
            w.asset(T::VectorField, a + D::vectorField)?;
        }
        0x08 | 0x09 => {
            w.asset(T::LightDef, a + D::lightDef)?;
        }
        0x02 | 0x14 | 0x30..=0x35 => {
            let id = w.asset(T::Vfx, a + D::particleSystem)?;
            out.extend(id.map(LinkRec::Asset));
        }
        _ => {}
    }
    Ok(())
}

/// A `ParticleLinkedAssetListDef` at `field`.
fn linked_list(w: &mut Walker, ty: u32, field: usize) -> Result<Vec<LinkRec>> {
    use L::ParticleLinkedAssetListDef as D;
    let n = w.s.u32_at(field + D::numAssets)? as usize;
    let len = L::ParticleLinkedAssetDef::SIZE;
    let mut out = Vec::new();
    if let Some(list) = w.inline_array(field + D::assetList, len * n, 7)? {
        for i in 0..n {
            linked_asset(w, ty, list + len * i, &mut out)?;
        }
    }
    Ok(out)
}

fn module(w: &mut Walker, m: usize) -> Result<ModuleRec> {
    let ty = w.s.u32_at(m + M::moduleType)?;
    let data = w.s.data().get(m + M::moduleData..m + M::SIZE).unwrap_or(&[]).to_vec();
    let mut rec = ModuleRec { ty, data, ..Default::default() };
    let list = |w: &mut Walker, f: usize| linked_list(w, ty, m + f);
    match ty {
        0x02 => rec.links = list(w, M::moduleData__initBeam__m_linkedAssetList)?,
        0x04 => rec.curves = curves(w, m + M::moduleData__initCloud__curves, 2)?,
        0x05 => rec.links = list(w, M::moduleData__initDecal__m_linkedAssetList)?,
        0x06 => rec.curves = curves(w, m + M::moduleData__initFlare__m_curves, 4)?,
        0x08 => rec.links = list(w, M::moduleData__initLightOmni__m_linkedAssetList)?,
        0x09 => rec.links = list(w, M::moduleData__initLightSpot__m_linkedAssetList)?,
        0x0a => rec.links = list(w, M::moduleData__initMaterial__m_linkedAssetList)?,
        0x0c => rec.links = list(w, M::moduleData__initModel__m_linkedAssetList)?,
        0x0f => rec.links = list(w, M::moduleData__initParticleSim__m_linkedAssetList)?,
        0x14 => rec.links = list(w, M::moduleData__initRunner__m_linkedAssetList)?,
        0x15 => rec.links = list(w, M::moduleData__initSound__m_linkedAssetList)?,
        0x16 => rec.curves = curves(w, m + M::moduleData__initSpawn__m_curves, 1)?,
        0x1a => {
            rec.links = list(w, M::moduleData__initSpawnShapeMesh__m_linkedAssetList)?;
            let n = w.s.u32_at(m + M::moduleData__initSpawnShapeMesh__m_numMeshAssets)? as usize;
            w.blob(
                m + M::moduleData__initSpawnShapeMesh__m_meshAssetData,
                L::ParticleSpawnMeshAssetDef::SIZE * n,
                3,
            )?;
        }
        0x1d => rec.links = list(w, M::moduleData__initVectorField__m_linkedAssetList)?,
        0x20 => rec.curves = curves(w, m + M::moduleData__colorGraph__m_curves, 8)?,
        0x22 => rec.curves = curves(w, m + M::moduleData__emissionGraph__m_curves, 2)?,
        0x23 => rec.curves = curves(w, m + M::moduleData__emissiveGraph__m_curves, 2)?,
        0x26 => rec.curves = curves(w, m + M::moduleData__intensityGraph__m_curves, 2)?,
        0x27 => rec.curves = curves(w, m + M::moduleData__parentVelocityGraph__m_curves, 2)?,
        0x28 => rec.links = list(w, M::moduleData__physicsLight__m_linkedAssetList)?,
        0x2a => rec.curves = curves(w, m + M::moduleData__positionGraph__m_curves, 6)?,
        0x2b => rec.curves = curves(w, m + M::moduleData__rotationGraph__m_curves, 2)?,
        0x2c => rec.curves = curves(w, m + M::moduleData__rotationGraph3D__m_curves, 6)?,
        0x2d => rec.curves = curves(w, m + M::moduleData__sizeGraph__m_curves, 6)?,
        0x2f => rec.curves = curves(w, m + M::moduleData__velocityGraph__m_curves, 6)?,
        0x30 => rec.links = list(w, M::moduleData__testAge__m_eventHandlerData__m_linkedAssetList)?,
        0x31 => rec.links = list(w, M::moduleData__testBirth__m_eventHandlerData__m_linkedAssetList)?,
        0x32 => rec.links = list(w, M::moduleData__testDeath__m_eventHandlerData__m_linkedAssetList)?,
        0x33 => rec.links = list(w, M::moduleData__testGravity__m_eventHandlerData__m_linkedAssetList)?,
        0x34 => rec.links = list(w, M::moduleData__testImpact__m_eventHandlerData__m_linkedAssetList)?,
        0x35 => rec.links = list(w, M::moduleData__testPos__m_eventHandlerData__m_linkedAssetList)?,
        _ => {}
    }
    Ok(rec)
}

fn module_group(w: &mut Walker, g: usize) -> Result<Vec<ModuleRec>> {
    use L::ParticleModuleGroupDef as G;
    let n = w.s.u32_at(g + G::numModules)? as usize;
    let mut out = Vec::new();
    if let Some(mods) = w.inline_array(g + G::moduleDefs, M::SIZE * n, 15)? {
        for i in 0..n {
            out.push(module(w, mods + M::SIZE * i)?);
        }
    }
    Ok(out)
}

fn state(w: &mut Walker, s: usize) -> Result<StateRec> {
    let len = L::ParticleModuleGroupDef::SIZE;
    let mut rec = StateRec {
        element_type: w.s.u32_at(s + L::ParticleStateDef::elementType)?,
        flags: w.s.u32_at(s + L::ParticleStateDef::flags)?,
        ..Default::default()
    };
    if let Some(groups) = w.inline_array(s + L::ParticleStateDef::moduleGroupDefs, len * 3, 15)? {
        for i in 0..3 {
            rec.group_disabled[i] = w.s.u32_at(groups + len * i + L::ParticleModuleGroupDef::disabled)?;
            rec.groups[i] = module_group(w, groups + len * i)?;
        }
    }
    Ok(rec)
}

fn emitter(w: &mut Walker, e: usize) -> Result<EmitterRec> {
    use L::ParticleEmitterDef as E;
    let n = w.s.u32_at(e + E::numStates)? as usize;
    let len = L::ParticleStateDef::SIZE;
    let record = w.s.data().get(e..e + E::SIZE).unwrap_or(&[]).to_vec();
    let mut states = Vec::new();
    if let Some(list) = w.inline_array(e + E::stateDefs, len * n, 15)? {
        for i in 0..n {
            states.push(state(w, list + len * i)?);
        }
    }
    let fade = curve(w, e + E::fadeCurveDef)?;
    Ok(EmitterRec { record, states, fade })
}

pub(super) fn vfx(w: &mut Walker, h: usize) -> Result<AssetId> {
    use L::ParticleSystemDef as P;
    w.s.push(VIRTUAL);
    let name = w.xstr(h + P::name)?.unwrap_or_default();
    let n = w.s.u32_at(h + P::numEmitters)? as usize;
    let len = L::ParticleEmitterDef::SIZE;
    let mut rec = VfxRec {
        name,
        version: w.s.u32_at(h + P::version)?,
        flags: w.s.u32_at(h + P::flags)?,
        emitters: Vec::new(),
    };
    if let Some(emitters) = w.inline_array(h + P::emitterDefs, len * n, 15)? {
        for i in 0..n {
            rec.emitters.push(emitter(w, emitters + len * i)?);
        }
    }
    let n = w.s.u32_at(h + P::numScriptedInputNodes)? as usize;
    w.blob(h + P::scriptedInputNodeDefs, L::ParticleScriptedInputNodeDef::SIZE * n, 15)?;
    w.s.pop();
    w.cap.vfx.push(rec);
    Ok(AssetId { ty: AssetType::Vfx, index: (w.cap.vfx.len() - 1) as u32 })
}

/// A particle simulation (x64-zt `iw7/assets/fxparticlesimanimation.cpp`):
/// its material, particle data (aligned 32), 3-D orientations, indices,
/// frames and colour table.
pub(super) fn particle_sim(w: &mut Walker, h: usize) -> Result<AssetId> {
    use L::FxParticleSimAnimation as P;
    w.s.push(VIRTUAL);
    let name = w.xstr(h + P::name)?.unwrap_or_default();
    w.asset(AssetType::Material, h + P::material)?;
    let n = w.s.u32_at(h + P::header__particleDataCount)? as usize;
    w.blob(h + P::particleData, L::FxParticleSimAnimationParticleData::SIZE * n, 31)?;
    w.blob(h + P::particleDataOrientation3D, L::FxParticleSimAnimationParticleDataOrientation3D::SIZE * n, 7)?;
    w.blob(h + P::particleDataIndex, L::FxParticleSimAnimationParticleDataIndex::SIZE * n, 1)?;
    let frames = w.s.u32_at(h + P::header__frameCount)? as usize;
    w.blob(h + P::frames, L::FxParticleSimAnimationFrame::SIZE * frames, 3)?;
    let colours = w.s.u32_at(h + P::header__colorTableSize)? as usize;
    w.blob(h + P::colorTable, 16 * colours, 3)?;
    w.s.pop();
    Ok(w.name_only(AssetType::ParticleSimAnimation, name))
}
